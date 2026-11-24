mod conn;
mod dns;
pub mod packet;
mod tcp;

#[cfg(fuzzing)]
pub mod fuzzing {
    pub use crate::dns::fuzzing as dns;
}

pub use conn::{Closer, TcpConn};

pub use dns::qname;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DnsHop {
    Direct(SocketAddr),
    Stream(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DnsRoute {
    Servers(Vec<(DnsHop, Duration)>),
    Reply(Vec<u8>),
    Lookup(String),
}

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::pin::Pin;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[cfg(unix)]
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};

use packet::{PROTO_TCP, PROTO_UDP, Parsed, TCP_ACK, TCP_SYN};

pub trait Stream: AsyncRead + AsyncWrite + Unpin {}
impl<T: AsyncRead + AsyncWrite + Unpin> Stream for T {}
pub type BoxStream = Box<dyn Stream>;
pub type LocalBoxFuture<T> = Pin<Box<dyn Future<Output = T>>>;

pub trait Handler: 'static {
    fn tcp(&self, local: TcpConn, src: SocketAddr, dst: SocketAddr) -> LocalBoxFuture<()>;
    fn dns_stream(&self, upstream: usize) -> LocalBoxFuture<io::Result<BoxStream>>;
    fn generation(&self) -> u64 {
        0
    }
    fn dns_cache(&self) -> bool {
        true
    }
    fn dns_label(&self, upstream: usize) -> String;
    fn dns_route(&self, query: &[u8]) -> DnsRoute;
    fn dns_lookup(&self, _query: Vec<u8>, _name: String) -> LocalBoxFuture<Option<Vec<u8>>> {
        Box::pin(async { None })
    }
    fn dns_direct(&self, server: SocketAddr, msg: Vec<u8>) -> LocalBoxFuture<Option<Vec<u8>>>;
    fn udp_route(&self, src: SocketAddr, dst: SocketAddr) -> Option<u64>;
    fn udp(&self, rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>, reply: UdpReply) -> LocalBoxFuture<()>;
}

pub struct UdpReply {
    app: SocketAddr,
    tun: Rc<Tun>,
}

impl UdpReply {
    pub fn app(&self) -> SocketAddr {
        self.app
    }

    pub fn send(&self, from: SocketAddr, data: &[u8]) {
        if from.is_ipv4() != self.app.is_ipv4() || data.len() > 65535 - 48 {
            return;
        }
        let mut hdr = [0u8; 48];
        let n = packet::udp_header(from, self.app, data, &mut hdr);
        rv_stats::add(rv_stats::Slot::Down, data.len() as u64);
        if self.tun.packets.full() {
            self.tun.flush_now();
        }
        self.tun.packets.push_parts(&[&hdr[..n], data]);
    }
}

const UDP_QUEUE: usize = 64;
const UDP_MAX: usize = 4096;
const UDP_FAIL_FAST: Duration = Duration::from_secs(2);
const UDP_BACKOFF: Duration = Duration::from_secs(2);
const UDP_FAILED_MAX: usize = 1024;

#[derive(Default)]
struct UdpSessions {
    live: HashMap<UdpKey, mpsc::Sender<(SocketAddr, Vec<u8>)>>,
    failed: HashMap<UdpKey, Instant>,
}

impl UdpSessions {
    fn ended(&mut self, key: UdpKey, failed: bool) {
        if !self.live.get(&key).is_some_and(|tx| tx.is_closed()) {
            return;
        }
        self.live.remove(&key);
        if failed {
            let now = Instant::now();
            if self.failed.len() >= UDP_FAILED_MAX {
                self.failed.retain(|_, at| now - *at < UDP_BACKOFF);
                if self.failed.len() >= UDP_FAILED_MAX {
                    return;
                }
            }
            self.failed.insert(key, now);
        }
    }
}

const TUN_QUEUE: usize = 256;

const NOBUFS_CAPS: [usize; 3] = [16 * 1024 - 512, 8 * 1024 - 512, 4 * 1024 - 512];
#[cfg_attr(windows, allow(dead_code))]
const NOBUFS_PROBE: Duration = Duration::from_secs(600);

#[derive(Default)]
#[cfg_attr(windows, allow(dead_code))]
struct NoBufs {
    level: Cell<usize>,
    last: Cell<Option<Instant>>,
    hits: Cell<u64>,
}

#[cfg_attr(windows, allow(dead_code))]
impl NoBufs {
    fn cap_at(level: usize) -> usize {
        if level == 0 { usize::MAX } else { NOBUFS_CAPS[level - 1] }
    }

    fn cap(&self) -> usize {
        Self::cap_at(self.level.get())
    }

    fn tick(&self, now: Instant) {
        let l = self.level.get();
        if l > 0 && self.last.get().is_some_and(|t| now.saturating_duration_since(t) >= NOBUFS_PROBE) {
            self.level.set(l - 1);
            self.last.set(Some(now));
            let cap = if l == 1 { "any size".to_string() } else { format!("up to {} bytes", Self::cap_at(l - 1)) };
            tracing::debug!("tun write: no ENOBUFS for {} s, packets {cap} (errors so far {})", NOBUFS_PROBE.as_secs(), self.hits.get());
        }
    }

    fn hit(&self, len: usize, now: Instant) -> usize {
        self.hits.set(self.hits.get() + 1);
        self.last.set(Some(now));
        let mut l = self.level.get();
        while l < NOBUFS_CAPS.len() && Self::cap_at(l) >= len {
            l += 1;
        }
        if l != self.level.get() {
            self.level.set(l);
            tracing::debug!(
                "tun write: ENOBUFS on {len} bytes, packets up to {} bytes (errors so far {})",
                Self::cap_at(l),
                self.hits.get()
            );
        }
        self.cap()
    }

    fn mss(&self) -> usize {
        self.cap().saturating_sub(40 + 60)
    }
}

#[derive(Default)]
struct Packets {
    pending: RefCell<VecDeque<Vec<u8>>>,
    blocked: Cell<bool>,
    spare: RefCell<Vec<Vec<u8>>>,
    spare_miss: Cell<Option<Instant>>,
    wakeup: tokio::sync::Notify,
    nobufs: NoBufs,
}

const SPARE_MAX: usize = 64;
const SPARE_CAP: usize = 16 * 1024;
const SPARE_KEEP: usize = 8;
const SPARE_IDLE: Duration = Duration::from_secs(30);

impl Packets {
    fn full(&self) -> bool {
        self.pending.borrow().len() >= TUN_QUEUE
    }
    fn empty(&self) -> bool {
        self.pending.borrow().is_empty()
    }

    fn push(&self, pkt: Vec<u8>) {
        let mut q = self.pending.borrow_mut();
        if q.len() >= TUN_QUEUE {
            rv_stats::add(rv_stats::Slot::TunDropped, 1);
            return;
        }
        let wake = q.is_empty();
        q.push_back(pkt);
        drop(q);
        if wake {
            self.wakeup.notify_one();
        }
    }

    fn push_parts(&self, parts: &[&[u8]]) {
        let mut b = self.spare.borrow_mut().pop().unwrap_or_else(|| {
            self.spare_miss.set(Some(Instant::now()));
            Vec::new()
        });
        b.clear();
        for p in parts {
            b.extend_from_slice(p);
        }
        self.push(b);
    }

    fn trim(&self, now: Instant) {
        let mut s = self.spare.borrow_mut();
        if s.len() > SPARE_KEEP && self.spare_miss.get().is_none_or(|t| now.saturating_duration_since(t) >= SPARE_IDLE) {
            s.truncate(SPARE_KEEP);
            s.shrink_to_fit();
        }
    }

    fn recycle(&self, b: Vec<u8>) {
        let mut s = self.spare.borrow_mut();
        if s.len() < SPARE_MAX && b.capacity() <= SPARE_CAP {
            s.push(b);
        }
    }

    async fn ready(&self) {
        while self.empty() {
            self.wakeup.notified().await;
        }
    }

    fn drain(&self, max: usize, mut write: impl FnMut(&[u8], &mut Vec<Vec<u8>>) -> io::Result<()>) -> io::Result<()> {
        let mut q = self.pending.borrow_mut();
        let mut rest = Vec::new();
        for _ in 0..max {
            let Some(pkt) = q.front() else { break };
            loop {
                match write(pkt, &mut rest) {
                    Ok(()) => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.blocked.set(true);
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::debug!("tun write: {e}");
                        rv_stats::add(rv_stats::Slot::TunDropped, 1);
                        break;
                    }
                }
            }
            if let Some(b) = q.pop_front() {
                self.recycle(b);
            }
            if !rest.is_empty() {
                for b in rest.drain(..).rev() {
                    q.push_front(b);
                }
                self.blocked.set(true);
                return Ok(());
            }
        }
        if q.is_empty() {
            self.blocked.set(false);
        }
        Ok(())
    }
}

#[cfg(windows)]
pub struct Device {
    pub rx: mpsc::Receiver<Vec<u8>>,
    pub send: Box<dyn Fn(&[u8])>,
}

pub struct Config {
    #[cfg(unix)]
    pub fd: RawFd,
    #[cfg(windows)]
    pub device: Device,
    pub mtu: usize,
    pub v4: Option<(Ipv4Addr, Ipv4Addr)>,
    pub v6: Option<(Ipv6Addr, Ipv6Addr)>,
    pub network: watch::Receiver<u64>,
    pub stack: Stack,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stack {
    System,
    #[default]
    User,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FlowKey {
    src: SocketAddr,
    dst: SocketAddr,
}

struct NatEntry {
    key: FlowKey,
    closed_at: Option<Instant>,
    created: Instant,
    accepted: bool,
}

struct Nat {
    by_key: HashMap<FlowKey, u16>,
    by_port: HashMap<u16, NatEntry>,
    next: u16,
    swept: Instant,
}

const NAT_PORT_MIN: u16 = 10000;
const NAT_LINGER: Duration = Duration::from_secs(60);
const NAT_UNACCEPTED_TTL: Duration = Duration::from_secs(60);

impl Nat {
    fn new() -> Self {
        Self { by_key: HashMap::new(), by_port: HashMap::new(), next: NAT_PORT_MIN, swept: Instant::now() }
    }

    fn lookup(&self, key: &FlowKey) -> Option<u16> {
        self.by_key.get(key).copied()
    }

    fn insert(&mut self, key: FlowKey) -> Option<u16> {
        let since = self.swept.elapsed();
        if (self.by_port.len() > 4096 && since >= Duration::from_secs(1)) || since >= NAT_LINGER {
            self.sweep();
        }
        self.alloc(key).or_else(|| {
            self.sweep();
            self.alloc(key)
        })
    }

    fn alloc(&mut self, key: FlowKey) -> Option<u16> {
        for _ in 0..=(u16::MAX - NAT_PORT_MIN) {
            let p = self.next;
            self.next = if self.next == u16::MAX { NAT_PORT_MIN } else { self.next + 1 };
            if let std::collections::hash_map::Entry::Vacant(v) = self.by_port.entry(p) {
                v.insert(NatEntry { key, closed_at: None, created: Instant::now(), accepted: false });
                self.by_key.insert(key, p);
                return Some(p);
            }
        }
        None
    }

    fn sweep(&mut self) {
        let now = Instant::now();
        self.swept = now;
        let by_key = &mut self.by_key;
        let before = self.by_port.len();
        self.by_port.retain(|_, e| {
            let dead = e.closed_at.is_some_and(|t| now - t > NAT_LINGER) || (!e.accepted && now - e.created > NAT_UNACCEPTED_TTL);
            if dead {
                by_key.remove(&e.key);
            }
            !dead
        });
        if before > 1024 && self.by_port.len() < before / 4 {
            self.by_port.shrink_to(self.by_port.len() * 2);
            self.by_key.shrink_to(self.by_key.len() * 2);
        }
    }

    fn close(&mut self, port: u16) {
        if let Some(e) = self.by_port.get_mut(&port) {
            e.closed_at = Some(Instant::now());
        }
    }
}

#[cfg(unix)]
struct Tun {
    fd: AsyncFd<OwnedFd>,
    packets: Rc<Packets>,
}

#[cfg(unix)]
impl Tun {
    async fn readable(&self) -> io::Result<Ready<'_>> {
        Ok(Ready { guard: self.fd.readable().await? })
    }
    fn write(&self, pkt: &[u8]) -> io::Result<()> {
        self.send(&[pkt]);
        Ok(())
    }

    fn send(&self, parts: &[&[u8]]) {
        if !self.packets.blocked.get() {
            let mut rest = Vec::new();
            match self.put(parts, &mut rest) {
                Ok(()) => {
                    if !rest.is_empty() {
                        self.packets.blocked.set(true);
                        for b in rest {
                            self.packets.push(b);
                        }
                    }
                    return;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => self.packets.blocked.set(true),
                Err(e) => {
                    tracing::debug!("tun write: {e}");
                    rv_stats::add(rv_stats::Slot::TunDropped, 1);
                    return;
                }
            }
        }
        self.packets.push(parts.concat());
    }

    fn put(&self, parts: &[&[u8]], rest: &mut Vec<Vec<u8>>) -> io::Result<()> {
        let nb = &self.packets.nobufs;
        let len: usize = parts.iter().map(|p| p.len()).sum();
        if nb.level.get() > 0 {
            nb.tick(Instant::now());
            if len > nb.cap()
                && let Some(r) = self.put_split(parts, rest)
            {
                return r;
            }
        }
        loop {
            match Self::writev_fd(self.fd.get_ref(), parts) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.raw_os_error() == Some(libc::ENOBUFS) => {
                    if len > nb.hit(len, Instant::now())
                        && let Some(r) = self.put_split(parts, rest)
                    {
                        return r;
                    }
                    return Err(e);
                }
                r => return r,
            }
        }
    }

    fn put_split(&self, parts: &[&[u8]], rest: &mut Vec<Vec<u8>>) -> Option<io::Result<()>> {
        let head = parts.first()?;
        let tcp = match head.first()? >> 4 {
            4 => head.get(9) == Some(&packet::PROTO_TCP),
            6 => head.get(6) == Some(&packet::PROTO_TCP),
            _ => false,
        };
        if !tcp {
            return None;
        }
        let pkt = parts.concat();
        let p = packet::parse(&pkt).filter(packet::tcp_splittable)?;
        let nb = &self.packets.nobufs;
        let hl = p.payload_offset;
        let total = p.total_len - hl;
        let mut hdr = [0u8; packet::TCP_PIECE_HDR];
        let (mut off, mut wrote) = (0, false);
        while off < total {
            let n = nb.cap().saturating_sub(hl).max(536).min(total - off);
            let h = packet::tcp_piece(&pkt, &p, off, n, &mut hdr);
            let data = &pkt[hl + off..hl + off + n];
            match Self::writev_fd(self.fd.get_ref(), &[&hdr[..h], data]) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if !wrote {
                        return Some(Err(e));
                    }
                    while off < total {
                        let n = nb.cap().saturating_sub(hl).max(536).min(total - off);
                        let h = packet::tcp_piece(&pkt, &p, off, n, &mut hdr);
                        rest.push([&hdr[..h], &pkt[hl + off..hl + off + n]].concat());
                        off += n;
                    }
                    return Some(Ok(()));
                }
                Err(e) if e.raw_os_error() == Some(libc::ENOBUFS) && h + n > nb.hit(h + n, Instant::now()) => continue,
                Err(e) => {
                    tracing::debug!("tun write: {e}");
                    rv_stats::add(rv_stats::Slot::TunDropped, 1);
                }
            }
            wrote = true;
            off += n;
        }
        Some(Ok(()))
    }

    #[cfg(not(target_vendor = "apple"))]
    fn writev_fd(fd: &OwnedFd, parts: &[&[u8]]) -> io::Result<()> {
        let mut iov = [libc::iovec { iov_base: std::ptr::null_mut(), iov_len: 0 }; 8];
        let mut len = 0;
        for (v, p) in iov.iter_mut().zip(parts) {
            *v = libc::iovec { iov_base: p.as_ptr() as *mut _, iov_len: p.len() };
            len += p.len();
        }
        debug_assert!(parts.len() <= 8);
        #[cfg(test)]
        if len > tests::NOBUFS_OVER.get() {
            tests::NOBUFS_CALLS.set(tests::NOBUFS_CALLS.get() + 1);
            return Err(io::Error::from_raw_os_error(libc::ENOBUFS));
        }
        let n = unsafe { libc::writev(fd.as_raw_fd(), iov.as_ptr(), parts.len().min(8) as i32) };
        written(n, len)
    }
    #[cfg(target_vendor = "apple")]
    fn writev_fd(fd: &OwnedFd, parts: &[&[u8]]) -> io::Result<()> {
        let af: u32 =
            if parts.first().and_then(|p| p.first()).is_some_and(|b| b >> 4 == 6) { libc::AF_INET6 as u32 } else { libc::AF_INET as u32 };
        let hdr = af.to_be_bytes();
        let mut iov = [libc::iovec { iov_base: std::ptr::null_mut(), iov_len: 0 }; 9];
        iov[0] = libc::iovec { iov_base: hdr.as_ptr() as *mut _, iov_len: 4 };
        let mut len = 4;
        for (v, p) in iov[1..].iter_mut().zip(parts) {
            *v = libc::iovec { iov_base: p.as_ptr() as *mut _, iov_len: p.len() };
            len += p.len();
        }
        let n = unsafe { libc::writev(fd.as_raw_fd(), iov.as_ptr(), (parts.len().min(8) + 1) as i32) };
        written(n, len)
    }

    async fn flush(&self) -> io::Result<()> {
        self.packets.ready().await;
        let mut guard = self.fd.writable().await?;
        self.packets.drain(TUN_BATCH, |pkt, rest| match guard.try_io(|_| self.put(&[pkt], rest)) {
            Ok(r) => r,
            Err(_) => Err(io::ErrorKind::WouldBlock.into()),
        })
    }

    fn flush_now(&self) {
        if !self.packets.blocked.get() {
            let _ = self.packets.drain(TUN_QUEUE, |pkt, rest| self.put(&[pkt], rest));
        }
    }

    #[cfg(not(target_vendor = "apple"))]
    #[cfg_attr(not(test), allow(dead_code))]
    fn write_fd(fd: &OwnedFd, pkt: &[u8]) -> io::Result<()> {
        let n = unsafe { libc::write(fd.as_raw_fd(), pkt.as_ptr().cast(), pkt.len()) };
        written(n, pkt.len())
    }
    #[cfg(target_vendor = "apple")]
    #[cfg_attr(not(test), allow(dead_code))]
    fn write_fd(fd: &OwnedFd, pkt: &[u8]) -> io::Result<()> {
        let af: u32 = if pkt.first().is_some_and(|b| b >> 4 == 6) { libc::AF_INET6 as u32 } else { libc::AF_INET as u32 };
        let hdr = af.to_be_bytes();
        let iov = [
            libc::iovec { iov_base: hdr.as_ptr() as *mut _, iov_len: 4 },
            libc::iovec { iov_base: pkt.as_ptr() as *mut _, iov_len: pkt.len() },
        ];
        let n = unsafe { libc::writev(fd.as_raw_fd(), iov.as_ptr(), 2) };
        written(n, pkt.len() + 4)
    }
}

#[cfg(unix)]
fn written(n: isize, len: usize) -> io::Result<()> {
    if n < 0 {
        Err(io::Error::last_os_error())
    } else if n as usize != len {
        Err(io::Error::new(io::ErrorKind::WriteZero, "short TUN packet write"))
    } else {
        Ok(())
    }
}

#[cfg(unix)]
struct Ready<'a> {
    guard: tokio::io::unix::AsyncFdReadyGuard<'a, OwnedFd>,
}

#[cfg(unix)]
impl Ready<'_> {
    #[cfg(not(target_vendor = "apple"))]
    fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe { libc::read(self.guard.get_inner().as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
    }
    #[cfg(target_vendor = "apple")]
    fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut hdr = [0u8; 4];
        let iov = [
            libc::iovec { iov_base: hdr.as_mut_ptr().cast(), iov_len: 4 },
            libc::iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() },
        ];
        let n = unsafe { libc::readv(self.guard.get_inner().as_raw_fd(), iov.as_ptr(), 2) };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok((n as usize).saturating_sub(4)) }
    }
    fn clear_ready(&mut self) {
        self.guard.clear_ready();
    }
}

#[cfg(windows)]
struct Tun {
    rx: std::cell::RefCell<mpsc::Receiver<Vec<u8>>>,
    send: Box<dyn Fn(&[u8])>,
    packets: Rc<Packets>,
}

#[cfg(windows)]
impl Tun {
    async fn readable(&self) -> io::Result<Ready<'_>> {
        #[allow(clippy::await_holding_refcell_ref)]
        let first = self.rx.borrow_mut().recv().await;
        Ok(Ready { tun: self, first: Some(first) })
    }
    fn write(&self, pkt: &[u8]) -> io::Result<()> {
        (self.send)(pkt);
        Ok(())
    }
    fn send(&self, parts: &[&[u8]]) {
        (self.send)(&parts.concat());
    }
    async fn flush(&self) -> io::Result<()> {
        self.packets.ready().await;
        self.packets.drain(TUN_BATCH, |pkt, _| {
            (self.send)(pkt);
            Ok(())
        })
    }
    fn flush_now(&self) {
        let _ = self.packets.drain(TUN_QUEUE, |pkt, _| {
            (self.send)(pkt);
            Ok(())
        });
    }
}

#[cfg(windows)]
struct Ready<'a> {
    tun: &'a Tun,
    first: Option<Option<Vec<u8>>>,
}

#[cfg(windows)]
impl Ready<'_> {
    fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let pkt = match self.first.take() {
            Some(Some(p)) => p,
            Some(None) => return Ok(0),
            None => match self.tun.rx.borrow_mut().try_recv() {
                Ok(p) => p,
                Err(mpsc::error::TryRecvError::Empty) => return Err(io::ErrorKind::WouldBlock.into()),
                Err(mpsc::error::TryRecvError::Disconnected) => return Ok(0),
            },
        };
        let n = pkt.len().min(buf.len());
        buf[..n].copy_from_slice(&pkt[..n]);
        Ok(n)
    }
    fn clear_ready(&mut self) {}
}

enum Event {
    Closed(u16),
    UdpClosed(UdpKey),
    UdpFailed(UdpKey),
}

pub async fn run(cfg: Config, handler: Rc<dyn Handler>, stop: oneshot::Receiver<()>, ready: impl FnOnce(io::Result<()>)) -> io::Result<()> {
    let mut ready = Some(ready);
    let r = run_inner(cfg, handler, stop, &mut ready).await;
    if let Some(f) = ready.take() {
        f(r.as_ref().map(|_| ()).map_err(|e| io::Error::new(e.kind(), e.to_string())));
    }
    r
}

async fn run_inner(
    cfg: Config,
    handler: Rc<dyn Handler>,
    mut stop: oneshot::Receiver<()>,
    ready: &mut Option<impl FnOnce(io::Result<()>)>,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        let flags = unsafe { libc::fcntl(cfg.fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(cfg.fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let system = cfg.stack == Stack::System;
    let mut l4 = match cfg.v4 {
        Some((local, _)) if system => Some(TcpListener::bind((local, 0)).await?),
        _ => None,
    };
    let mut l6 = match cfg.v6 {
        Some((local, _)) if system => Some(TcpListener::bind((local, 0)).await?),
        _ => None,
    };
    let mut lport4 = l4.as_ref().map(|l| l.local_addr().map(|a| a.port())).transpose()?.unwrap_or(0);
    let mut lport6 = l6.as_ref().map(|l| l.local_addr().map(|a| a.port())).transpose()?.unwrap_or(0);
    #[cfg(unix)]
    let tun = Rc::new(match AsyncFd::try_new(unsafe { OwnedFd::from_raw_fd(cfg.fd) }) {
        Ok(fd) => Tun { fd, packets: Rc::new(Packets::default()) },
        Err(e) => {
            let (fd, e) = e.into_parts();
            let _ = fd.into_raw_fd();
            return Err(e);
        }
    });
    #[cfg(windows)]
    let tun = Rc::new(Tun { rx: std::cell::RefCell::new(cfg.device.rx), send: cfg.device.send, packets: Rc::new(Packets::default()) });
    tracing::info!(lport4, lport6, stack = ?cfg.stack, "tun stack up");
    if let Some(f) = ready.take() {
        f(Ok(()));
    }

    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<Event>();
    let dns = dns::Client::new(handler.clone(), cfg.network.clone(), tun.packets.clone());
    let mut nat = Nat::new();
    let mut udp = UdpSessions::default();
    let mut retry4: Option<ListenerRetry> = None;
    let mut retry6: Option<ListenerRetry> = None;
    let mut buf = vec![0u8; cfg.mtu.clamp(1500, 65535) + 64];
    let mut scratch = Vec::with_capacity(1500);
    let mut user = (!system).then(|| tcp::Stack::new(cfg.mtu));
    let shared = user.as_ref().map(|u| u.shared());
    let mut sleep = Box::pin(tokio::time::sleep(Duration::from_secs(3600)));
    let mut armed: Option<tokio::time::Instant> = None;

    loop {
        if let Some(u) = &mut user {
            u.set_seg_cap(tun.packets.nobufs.mss());
            let next = u.next_timer();
            if next != armed {
                sleep.as_mut().reset(next.unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86400)));
                armed = next;
            }
        }
        tokio::select! {
            biased;
            _ = &mut stop => return Ok(()),
            r = tun.flush() => r?,
            _ = &mut sleep, if armed.is_some() => {
                armed = None;
                if let Some(u) = &mut user {
                    u.on_timer(tokio::time::Instant::now(), &mut |parts| send_tcp(&tun, parts));
                    u.flush(tokio::time::Instant::now(), &mut |parts| send_tcp(&tun, parts), false);
                }
            },
            _ = notified(&shared) => if let Some(u) = &mut user {
                u.flush(tokio::time::Instant::now(), &mut |parts| send_tcp(&tun, parts), false);
            },
            ev = ev_rx.recv() => match ev {
                Some(Event::Closed(p)) => nat.close(p),
                Some(Event::UdpClosed(src)) => udp.ended(src, false),
                Some(Event::UdpFailed(src)) => udp.ended(src, true),
                None => {}
            },
            r = accept(&l4, &retry4) => match r {
                Ok(c) => { retry4 = None; spawn_conn(c, cfg.v4.map(|v| IpAddr::V4(v.1)), &mut nat, &handler, &dns, &ev_tx); }
                Err(e) if listener_dead(&e) => rebind(&mut l4, &mut lport4, cfg.v4.map(|v| IpAddr::V4(v.0)), &mut retry4, e.to_string()).await?,
                Err(e) => { tracing::warn!("accept IPv4: {e}"); if exhausted(&e) { listener_backoff(&mut retry4, Duration::from_millis(50), e)?; } }
            },
            r = accept(&l6, &retry6) => match r {
                Ok(c) => { retry6 = None; spawn_conn(c, cfg.v6.map(|v| IpAddr::V6(v.1)), &mut nat, &handler, &dns, &ev_tx); }
                Err(e) if listener_dead(&e) => rebind(&mut l6, &mut lport6, cfg.v6.map(|v| IpAddr::V6(v.0)), &mut retry6, e.to_string()).await?,
                Err(e) => { tracing::warn!("accept IPv6: {e}"); if exhausted(&e) { listener_backoff(&mut retry6, Duration::from_millis(50), e)?; } }
            },
            _ = retry_at(&retry4), if l4.is_none() => rebind(&mut l4, &mut lport4, cfg.v4.map(|v| IpAddr::V4(v.0)), &mut retry4, "retry".into()).await?,
            _ = retry_at(&retry6), if l6.is_none() => rebind(&mut l6, &mut lport6, cfg.v6.map(|v| IpAddr::V6(v.0)), &mut retry6, "retry".into()).await?,
            guard = tun.readable(), if !tun.packets.full() => {
                let mut guard = guard?;
                let mut batch = 0;
                let now = tokio::time::Instant::now();
                loop {
                    if tun.packets.full() { break; }
                    batch += 1;
                    if batch > TUN_BATCH {
                        #[allow(clippy::drop_non_drop)]
                        drop(guard);
                        tokio::task::yield_now().await;
                        break;
                    }
                    match guard.try_read(&mut buf) {
                        Ok(0) => return Ok(()),
                        Ok(n) => handle_packet(&mut buf[..n], cfg.v4, cfg.v6, lport4, lport6, &mut nat, &mut user, now, &mut udp, &handler, &ev_tx, &tun, &dns, &mut scratch)?,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            guard.clear_ready();
                            break;
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(e),
                    }
                }
                tcp::trim_pool(now.into_std());
                tun.packets.trim(now.into_std());
                if let Some(u) = &mut user && u.has_dirty() {
                    u.flush(now, &mut |parts| send_tcp(&tun, parts), true);
                    if u.has_deferred() {
                        tokio::task::yield_now().await;
                        u.flush_deferred(tokio::time::Instant::now(), &mut |parts| send_tcp(&tun, parts));
                    }
                }
            }
        }
    }
}

fn send_tcp(tun: &Tun, parts: &[&[u8]]) {
    #[cfg(feature = "chaos")]
    {
        thread_local! {
            static DROP: u64 = std::env::var("RV_TCP_DROP").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            static RNG: Cell<u64> = const { Cell::new(0x9e37_79b9_7f4a_7c15) };
        }
        let x = RNG.with(|r| {
            let mut x = r.get();
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            r.set(x);
            x
        });
        if x % 1000 < DROP.with(|d| *d) {
            return;
        }
    }
    tun.send(parts)
}

async fn notified(shared: &Option<Rc<tcp::Shared>>) {
    match shared {
        Some(s) => s.notified().await,
        None => std::future::pending().await,
    }
}

const TUN_BATCH: usize = 256;

const LISTENER_FAILURE: Duration = Duration::from_secs(60);

struct ListenerRetry {
    since: tokio::time::Instant,
    at: tokio::time::Instant,
    pause: Duration,
}

fn listener_backoff(retry: &mut Option<ListenerRetry>, initial: Duration, e: io::Error) -> io::Result<()> {
    let now = tokio::time::Instant::now();
    let since = retry.as_ref().map_or(now, |r| r.since);
    if now - since >= LISTENER_FAILURE {
        return Err(io::Error::new(e.kind(), format!("TUN listener unavailable for {LISTENER_FAILURE:?}: {e}")));
    }
    let pause = retry.as_ref().map_or(initial, |r| (r.pause * 2).min(Duration::from_secs(30)));
    *retry = Some(ListenerRetry { since, at: (now + pause).min(since + LISTENER_FAILURE), pause });
    Ok(())
}

async fn accept(l: &Option<TcpListener>, retry: &Option<ListenerRetry>) -> io::Result<(TcpStream, SocketAddr)> {
    match l {
        Some(l) => {
            if retry.is_some() {
                retry_at(retry).await;
            }
            l.accept().await
        }
        None => std::future::pending().await,
    }
}

fn listener_dead(e: &io::Error) -> bool {
    #[cfg(unix)]
    return matches!(e.raw_os_error(), Some(libc::EINVAL | libc::EBADF | libc::ENOTSOCK));
    #[cfg(windows)]
    return matches!(e.raw_os_error(), Some(10009 | 10022 | 10038));
}

fn exhausted(e: &io::Error) -> bool {
    #[cfg(unix)]
    return matches!(e.raw_os_error(), Some(libc::EMFILE | libc::ENFILE | libc::ENOBUFS | libc::ENOMEM));
    #[cfg(windows)]
    return matches!(e.raw_os_error(), Some(10024 | 10055));
}

async fn rebind(
    l: &mut Option<TcpListener>,
    port: &mut u16,
    local: Option<IpAddr>,
    retry: &mut Option<ListenerRetry>,
    cause: String,
) -> io::Result<()> {
    let Some(local) = local else { return Ok(()) };
    match TcpListener::bind((local, 0)).await.and_then(|n| Ok((n.local_addr()?.port(), n))) {
        Ok((p, n)) => {
            tracing::warn!(old = *port, new = p, "listener lost ({cause}), rebound");
            *l = Some(n);
            *port = p;
            *retry = None;
        }
        Err(e) => {
            tracing::warn!("listener lost ({cause}), rebind failed: {e}");
            *l = None;
            listener_backoff(retry, Duration::from_millis(100), e)?;
        }
    }
    Ok(())
}

async fn retry_at(r: &Option<ListenerRetry>) {
    match r {
        Some(r) => tokio::time::sleep_until(r.at).await,
        None => std::future::pending().await,
    }
}

fn spawn_conn(
    (stream, peer): (TcpStream, SocketAddr),
    nat_peer: Option<IpAddr>,
    nat: &mut Nat,
    handler: &Rc<dyn Handler>,
    dns: &dns::Client,
    ev: &mpsc::UnboundedSender<Event>,
) {
    let port = peer.port();
    let Some(entry) = nat.by_port.get_mut(&port).filter(|e| Some(peer.ip()) == nat_peer && !e.accepted) else {
        tracing::debug!("rejected listener connection from {peer}");
        return;
    };
    entry.accepted = true;
    let (src, dst) = (entry.key.src, entry.key.dst);
    let fut = if dst.port() == 53 {
        let dns = dns.clone();
        Box::pin(async move { dns.serve_tcp(TcpConn::kernel(stream), src, dst).await }) as LocalBoxFuture<()>
    } else {
        handler.tcp(TcpConn::kernel(stream), src, dst)
    };
    let ev = ev.clone();
    tokio::task::spawn_local(async move {
        fut.await;
        let _ = ev.send(Event::Closed(port));
    });
}

#[allow(clippy::too_many_arguments)]
fn handle_packet(
    pkt: &mut [u8],
    v4: Option<(Ipv4Addr, Ipv4Addr)>,
    v6: Option<(Ipv6Addr, Ipv6Addr)>,
    lport4: u16,
    lport6: u16,
    nat: &mut Nat,
    user: &mut Option<tcp::Stack>,
    now: tokio::time::Instant,
    udp: &mut UdpSessions,
    handler: &Rc<dyn Handler>,
    ev: &mpsc::UnboundedSender<Event>,
    tun: &Rc<Tun>,
    dns: &dns::Client,
    scratch: &mut Vec<u8>,
) -> io::Result<()> {
    let Some(p) = packet::parse(pkt) else { return Ok(()) };
    let (local, peer, lport) = match (p.src, v4, v6) {
        (IpAddr::V4(_), Some((l, pe)), _) => (IpAddr::V4(l), IpAddr::V4(pe), lport4),
        (IpAddr::V6(_), _, Some((l, pe))) => (IpAddr::V6(l), IpAddr::V6(pe), lport6),
        _ => {
            match p.proto {
                PROTO_TCP if p.tcp_flags & packet::TCP_RST == 0 => packet::build_tcp_rst(&pkt[..p.total_len], &p, scratch),
                PROTO_UDP => packet::build_port_unreachable(&pkt[..p.total_len], &p, scratch),
                _ => return Ok(()),
            }
            if !scratch.is_empty() {
                tun.write(scratch)?;
            }
            return Ok(());
        }
    };
    if local_only(p.dst, local, peer) {
        return Ok(());
    }
    if p.dst == peer
        && !(matches!(p.proto, PROTO_UDP | PROTO_TCP) && p.dst_port == 53)
        && !(user.is_none() && p.src == local && p.src_port == lport)
    {
        match p.proto {
            PROTO_TCP if p.tcp_flags & packet::TCP_RST == 0 => packet::build_tcp_rst(&pkt[..p.total_len], &p, scratch),
            PROTO_UDP => packet::build_port_unreachable(&pkt[..p.total_len], &p, scratch),
            _ => return Ok(()),
        }
        if !scratch.is_empty() {
            tun.write(scratch)?;
        }
        return Ok(());
    }
    match p.proto {
        PROTO_TCP => match user {
            Some(u) => match u.input(pkt, &p, now) {
                tcp::Verdict::None => {}
                tcp::Verdict::Accept(sock) => {
                    let (src, dst) = sock.peer();
                    if dst.port() == 53 {
                        let dns = dns.clone();
                        tokio::task::spawn_local(async move { dns.serve_tcp(TcpConn::user(sock), src, dst).await });
                    } else {
                        tokio::task::spawn_local(handler.tcp(TcpConn::user(sock), src, dst));
                    }
                }
                tcp::Verdict::Reject => {
                    if p.tcp_flags & packet::TCP_RST == 0 {
                        packet::build_tcp_rst(&pkt[..p.total_len], &p, scratch);
                        if !scratch.is_empty() {
                            tun.write(scratch)?;
                        }
                    }
                }
            },
            None => tcp_packet(pkt, &p, local, peer, lport, nat, tun)?,
        },
        PROTO_UDP => {
            if p.dst_port == 53 {
                rv_stats::add(rv_stats::Slot::DnsQueries, 1);
                dns.query(&pkt[p.payload_offset..p.total_len], SocketAddr::new(p.src, p.src_port), SocketAddr::new(p.dst, p.dst_port));
            } else {
                let src = SocketAddr::new(p.src, p.src_port);
                let dst = SocketAddr::new(p.dst, p.dst_port);
                let accepted = match handler.udp_route(src, dst) {
                    Some(route) => udp_packet(udp, (src, route), dst, &pkt[p.payload_offset..p.total_len], handler, ev, tun),
                    None => false,
                };
                if !accepted {
                    packet::build_port_unreachable(&pkt[..p.total_len], &p, scratch);
                    return tun.write(scratch);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn local_only(dst: IpAddr, local: IpAddr, peer: IpAddr) -> bool {
    match (dst, local) {
        (IpAddr::V4(d), IpAddr::V4(l)) => {
            d.is_multicast() || d.is_broadcast() || (d != l && dst != peer && u32::from(d) & !3 == u32::from(l) & !3)
        }
        (IpAddr::V6(d), _) => d.is_multicast(),
        _ => false,
    }
}

type UdpKey = (SocketAddr, u64);

fn udp_packet(
    udp: &mut UdpSessions,
    key: UdpKey,
    dst: SocketAddr,
    data: &[u8],
    handler: &Rc<dyn Handler>,
    ev: &mpsc::UnboundedSender<Event>,
    tun: &Rc<Tun>,
) -> bool {
    let generation = handler.generation();
    if generation != UDP_GEN.get() {
        UDP_GEN.set(generation);
        udp.live.clear();
        udp.failed.clear();
    }
    if let Some(tx) = udp.live.get(&key) {
        rv_stats::add(rv_stats::Slot::Up, data.len() as u64);
        let closed = match tx.try_reserve() {
            Ok(permit) => {
                permit.send((dst, data.to_vec()));
                return true;
            }
            Err(mpsc::error::TrySendError::Full(())) => return true,
            Err(mpsc::error::TrySendError::Closed(())) => true,
        };
        if closed {
            udp.live.remove(&key);
        }
    }
    if let Some(at) = udp.failed.get(&key) {
        if at.elapsed() < UDP_BACKOFF {
            return false;
        }
        udp.failed.remove(&key);
    }
    if udp.live.len() >= UDP_MAX {
        udp.live.retain(|_, tx| !tx.is_closed());
        if udp.live.len() >= UDP_MAX {
            rv_stats::add(rv_stats::Slot::TunDropped, 1);
            return false;
        }
    }
    rv_stats::add(rv_stats::Slot::Up, data.len() as u64);
    let (tx, rx) = mpsc::channel(UDP_QUEUE);
    let _ = tx.try_send((dst, data.to_vec()));
    udp.live.insert(key, tx);
    let reply = UdpReply { app: key.0, tun: tun.clone() };
    let fut = handler.udp(rx, reply);
    let ev = ev.clone();
    tokio::task::spawn_local(async move {
        let _active = rv_stats::Active::new(rv_stats::Slot::UdpActive);
        let born = Instant::now();
        fut.await;
        let failed = born.elapsed() < UDP_FAIL_FAST;
        let _ = ev.send(if failed { Event::UdpFailed(key) } else { Event::UdpClosed(key) });
    });
    true
}

thread_local! {
    static UDP_GEN: Cell<u64> = const { Cell::new(0) };
}

fn tcp_packet(pkt: &mut [u8], p: &Parsed, local: IpAddr, peer: IpAddr, lport: u16, nat: &mut Nat, tun: &Tun) -> io::Result<()> {
    let pkt = &mut pkt[..p.total_len];
    if p.src == local && p.src_port == lport {
        if p.dst != peer {
            return Ok(());
        }
        let Some(e) = nat.by_port.get(&p.dst_port).filter(|e| e.key.dst.is_ipv4() == p.dst.is_ipv4()) else {
            return Ok(());
        };
        let key = e.key;
        packet::rewrite(pkt, p, key.dst, key.src);
        return tun.write(pkt);
    }
    let key = FlowKey { src: SocketAddr::new(p.src, p.src_port), dst: SocketAddr::new(p.dst, p.dst_port) };
    let syn = p.tcp_flags & (TCP_SYN | TCP_ACK) == TCP_SYN;
    let port = match nat.lookup(&key) {
        Some(port) => {
            if syn
                && let Some(e) = nat.by_port.get_mut(&port)
                && e.closed_at.is_some()
            {
                e.closed_at = None;
                e.accepted = false;
                e.created = Instant::now();
            }
            port
        }
        None if syn => match nat.insert(key) {
            Some(port) => port,
            None => return Ok(()),
        },
        None => return Ok(()),
    };
    packet::rewrite(pkt, p, SocketAddr::new(peer, port), SocketAddr::new(local, lport));
    tun.write(pkt)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn nat_alloc_finds_the_last_free_port() {
        let mut nat = Nat::new();
        let key = |i: u32| FlowKey {
            src: SocketAddr::new(IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + i)), 1),
            dst: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 443),
        };
        let all = (u16::MAX - NAT_PORT_MIN) as u32 + 1;
        for i in 0..all {
            assert!(nat.alloc(key(i)).is_some());
        }
        assert_eq!(nat.next, NAT_PORT_MIN);
        let e = nat.by_port.remove(&u16::MAX).unwrap();
        nat.by_key.remove(&e.key);
        assert_eq!(nat.alloc(key(all)), Some(u16::MAX));
    }

    #[test]
    fn failed_udp_flows_are_capped() {
        let mut udp = UdpSessions::default();
        let app: SocketAddr = "10.0.0.2:40000".parse().unwrap();
        for i in 0..5000u64 {
            let (tx, rx) = mpsc::channel(1);
            drop(rx);
            udp.live.insert((app, i), tx);
            udp.ended((app, i), true);
            assert!(udp.failed.len() <= UDP_FAILED_MAX, "{} после {i}", udp.failed.len());
        }
        assert!(udp.live.is_empty());
        assert!(udp.failed.contains_key(&(app, 0)), "ранние отказы держат свою паузу");
    }

    #[test]
    fn packet_queue_keeps_boundaries_and_counts_only_overflow() {
        let q = Packets::default();
        let before = rv_stats::get(rv_stats::Slot::TunDropped);
        let expected: Vec<Vec<u8>> = (0..TUN_QUEUE).map(|i| vec![i as u8; 1 + i % 7]).collect();
        for p in &expected {
            q.push(p.clone());
        }
        assert!(q.full());
        let mut calls = 0;
        q.drain(TUN_BATCH, |_, _| {
            calls += 1;
            Err(if calls == 1 { io::ErrorKind::Interrupted } else { io::ErrorKind::WouldBlock }.into())
        })
        .unwrap();
        assert_eq!(q.pending.borrow().len(), TUN_QUEUE);
        assert_eq!(rv_stats::get(rv_stats::Slot::TunDropped), before);
        q.push(vec![0xff]);
        assert_eq!(q.pending.borrow().len(), TUN_QUEUE);
        assert_eq!(rv_stats::get(rv_stats::Slot::TunDropped), before + 1);
        let mut got = Vec::new();
        q.drain(TUN_BATCH, |p, _| {
            got.push(p.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(got, expected);
        assert!(q.empty());
        assert!(!q.full());
        q.push(vec![1, 2, 3]);
        q.push(vec![4]);
        let mut sent = Vec::new();
        q.drain(TUN_BATCH, |p, _| {
            if p.len() == 3 {
                written(1, p.len())
            } else {
                sent.push(p.to_vec());
                Ok(())
            }
        })
        .unwrap();
        assert!(q.empty());
        assert_eq!(sent, vec![vec![4]]);
        assert_eq!(rv_stats::get(rv_stats::Slot::TunDropped), before + 2);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn empty_packet_queue_has_no_wakeups() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let q = Rc::new(Packets::default());
                let polls = Rc::new(Cell::new(0));
                let (queue, count) = (q.clone(), polls.clone());
                let task = tokio::task::spawn_local(async move {
                    let mut wait = Box::pin(queue.ready());
                    std::future::poll_fn(|cx| {
                        count.set(count.get() + 1);
                        wait.as_mut().poll(cx)
                    })
                    .await;
                });
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                let before = polls.get();
                tokio::time::advance(Duration::from_secs(60)).await;
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                assert_eq!(polls.get(), before);
                assert!(!task.is_finished());
                q.push(vec![1]);
                task.await.unwrap();
                assert!(polls.get() > before);
            })
            .await;
    }

    #[cfg(not(target_vendor = "apple"))]
    #[tokio::test]
    async fn tun_eagain_waits_for_writable_without_losing_packets() {
        let (fd, peer) = std::os::unix::net::UnixDatagram::pair().unwrap();
        fd.set_nonblocking(true).unwrap();
        peer.set_nonblocking(true).unwrap();
        let tun = Tun { fd: AsyncFd::new(OwnedFd::from(fd)).unwrap(), packets: Rc::new(Packets::default()) };
        let filler = vec![0x55; 1024];
        let mut filled = 0;
        loop {
            match Tun::write_fd(tun.fd.get_ref(), &filler) {
                Ok(()) => filled += 1,
                Err(e) => {
                    assert_eq!(e.kind(), io::ErrorKind::WouldBlock);
                    break;
                }
            }
        }
        assert!(filled > 0);
        let expected = [vec![1; 17], vec![2; 61], vec![3; 997]];
        for p in &expected {
            tun.write(p).unwrap();
        }
        assert_eq!(tun.packets.pending.borrow().len(), expected.len());
        let drain = async {
            while !tun.packets.empty() {
                tun.flush().await.unwrap();
            }
        };
        tokio::pin!(drain);
        assert!(tokio::time::timeout(Duration::from_millis(10), &mut drain).await.is_err());
        let mut b = [0; 2048];
        for _ in 0..filled {
            assert_eq!(peer.recv(&mut b).unwrap(), filler.len());
        }
        tokio::time::timeout(Duration::from_secs(1), drain).await.unwrap();
        for p in expected {
            let n = peer.recv(&mut b).unwrap();
            assert_eq!(&b[..n], p);
        }
        assert!(tun.packets.empty());
        assert_eq!(peer.recv(&mut b).unwrap_err().kind(), io::ErrorKind::WouldBlock);
    }

    thread_local! {
        pub(super) static NOBUFS_OVER: Cell<usize> = const { Cell::new(usize::MAX) };
        pub(super) static NOBUFS_CALLS: Cell<u32> = const { Cell::new(0) };
    }

    fn tcp_pkt(len: usize, seq: u32) -> Vec<u8> {
        let mut p = vec![0u8; 40 + len];
        p[0] = 0x45;
        p[2..4].copy_from_slice(&((40 + len) as u16).to_be_bytes());
        p[6] = 0x40;
        p[9] = packet::PROTO_TCP;
        p[12..16].copy_from_slice(&[198, 18, 0, 1]);
        p[16..20].copy_from_slice(&[172, 19, 0, 1]);
        p[24..28].copy_from_slice(&seq.to_be_bytes());
        p[32] = 5 << 4;
        p[33] = packet::TCP_ACK | 0x08;
        for (i, b) in p[40..].iter_mut().enumerate() {
            *b = (i * 31 + 7) as u8;
        }
        p
    }

    #[cfg(not(target_vendor = "apple"))]
    #[tokio::test]
    async fn enobufs_writes_tcp_in_pieces_instead_of_dropping_it() {
        let (fd, peer) = std::os::unix::net::UnixDatagram::pair().unwrap();
        fd.set_nonblocking(true).unwrap();
        peer.set_nonblocking(true).unwrap();
        let tun = Tun { fd: AsyncFd::new(OwnedFd::from(fd)).unwrap(), packets: Rc::new(Packets::default()) };
        NOBUFS_OVER.set(20_000);
        let recv = || {
            let mut b = vec![0u8; 70_000];
            let mut got = Vec::new();
            while let Ok(n) = peer.recv(&mut b) {
                got.push(b[..n].to_vec());
            }
            got
        };
        let check = |got: &[Vec<u8>], pkt: &[u8]| {
            assert!(got.len() >= 4 && got.iter().all(|g| g.len() <= NOBUFS_CAPS[0]), "{:?}", got.iter().map(Vec::len).collect::<Vec<_>>());
            let data: Vec<u8> = got.iter().flat_map(|g| g[40..].to_vec()).collect();
            assert_eq!(data, pkt[40..], "данные целиком и по порядку");
            let seq = |g: &[u8]| u32::from_be_bytes(g[24..28].try_into().unwrap());
            assert_eq!(seq(&got[1]), seq(&got[0]).wrapping_add((got[0].len() - 40) as u32));
        };
        let pkt = tcp_pkt(60_000, 77);
        tun.send(&[&pkt[..40], &pkt[40..]]);
        check(&recv(), &pkt);
        assert_eq!((NOBUFS_CALLS.get(), tun.packets.nobufs.level.get()), (1, 1));
        let pkt = tcp_pkt(50_000, 9);
        tun.write(&pkt).unwrap();
        check(&recv(), &pkt);
        assert_eq!(NOBUFS_CALLS.get(), 1);
        let nb = &tun.packets.nobufs;
        nb.tick(Instant::now() + NOBUFS_PROBE / 2);
        assert_eq!(nb.level.get(), 1);
        nb.tick(Instant::now() + NOBUFS_PROBE + Duration::from_secs(1));
        assert_eq!((nb.level.get(), nb.cap()), (0, usize::MAX));
        NOBUFS_OVER.set(9_000);
        let pkt = tcp_pkt(60_000, 5);
        tun.send(&[&pkt]);
        let got = recv();
        assert!(got.iter().all(|g| g.len() <= NOBUFS_CAPS[1]), "{:?}", got.iter().map(Vec::len).collect::<Vec<_>>());
        assert_eq!(got.iter().map(|g| g.len() - 40).sum::<usize>(), 60_000);
        assert_eq!(nb.level.get(), 2);
        assert!(nb.mss() + 100 <= NOBUFS_CAPS[1]);
        NOBUFS_OVER.set(usize::MAX);
    }

    #[test]
    fn pieces_left_after_eagain_go_first_in_the_queue() {
        let q = Packets::default();
        q.push(vec![1; 10]);
        q.push(vec![2; 10]);
        let mut first = true;
        q.drain(TUN_BATCH, |_, rest| {
            if first {
                first = false;
                rest.push(vec![3; 5]);
                rest.push(vec![4; 5]);
            }
            Ok(())
        })
        .unwrap();
        assert!(q.blocked.get());
        let left: Vec<Vec<u8>> = q.pending.borrow().iter().cloned().collect();
        assert_eq!(left, vec![vec![3; 5], vec![4; 5], vec![2; 10]]);
    }

    #[cfg(not(target_vendor = "apple"))]
    fn pipe_tun() -> (Rc<Tun>, OwnedFd) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) }, 0);
        let (r, w) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        (Rc::new(Tun { fd: AsyncFd::new(w).unwrap(), packets: Rc::new(Packets::default()) }), r)
    }

    #[cfg(not(target_vendor = "apple"))]
    fn pipe_read(r: &OwnedFd) -> Vec<u8> {
        let mut b = vec![0u8; 1 << 20];
        let n = unsafe { libc::read(r.as_raw_fd(), b.as_mut_ptr().cast(), b.len()) };
        b.truncate(n.max(0) as usize);
        b
    }

    #[cfg(not(target_vendor = "apple"))]
    #[tokio::test]
    async fn udp_burst_past_the_queue_is_written_not_dropped() {
        let (tun, r) = pipe_tun();
        let reply = UdpReply { app: "172.19.0.1:5000".parse().unwrap(), tun: tun.clone() };
        let n = TUN_QUEUE + 50;
        for i in 0..n {
            reply.send("1.1.1.1:443".parse().unwrap(), &[i as u8; 20]);
        }
        while !tun.packets.empty() {
            tun.flush().await.unwrap();
        }
        assert_eq!(pipe_read(&r).len(), n * (20 + 28), "все ответы в TUN");
    }

    #[cfg(not(target_vendor = "apple"))]
    #[tokio::test]
    async fn tcp_is_written_at_once_while_udp_replies_wait_for_the_batch() {
        let (tun, r) = pipe_tun();
        let reply = UdpReply { app: "172.19.0.1:5000".parse().unwrap(), tun: tun.clone() };
        reply.send("1.1.1.1:443".parse().unwrap(), &[1; 20]);
        tun.send(&[&[0x45; 40], &[2; 100]]);
        assert_eq!(pipe_read(&r).len(), 140, "TCP ушёл сразу, без копии в очередь");
        assert_eq!(tun.packets.pending.borrow().len(), 1, "UDP ждёт пачку");
        tun.flush().await.unwrap();
        assert_eq!(pipe_read(&r).len(), 48);
    }

    struct FailingUdp {
        sessions: Cell<u32>,
    }

    impl Handler for FailingUdp {
        fn tcp(&self, _: TcpConn, _: SocketAddr, _: SocketAddr) -> LocalBoxFuture<()> {
            Box::pin(async {})
        }
        fn dns_stream(&self, _: usize) -> LocalBoxFuture<io::Result<BoxStream>> {
            Box::pin(async { Err(io::Error::other("none")) })
        }
        fn dns_label(&self, _: usize) -> String {
            String::new()
        }
        fn dns_route(&self, _: &[u8]) -> DnsRoute {
            DnsRoute::Servers(Vec::new())
        }
        fn dns_direct(&self, _: SocketAddr, _: Vec<u8>) -> LocalBoxFuture<Option<Vec<u8>>> {
            Box::pin(async { None })
        }
        fn udp_route(&self, _: SocketAddr, _: SocketAddr) -> Option<u64> {
            Some(0)
        }
        fn udp(&self, _: mpsc::Receiver<(SocketAddr, Vec<u8>)>, _: UdpReply) -> LocalBoxFuture<()> {
            self.sessions.set(self.sessions.get() + 1);
            Box::pin(async {})
        }
    }

    struct LiveUdp;

    impl Handler for LiveUdp {
        fn tcp(&self, _: TcpConn, _: SocketAddr, _: SocketAddr) -> LocalBoxFuture<()> {
            Box::pin(async {})
        }
        fn dns_stream(&self, _: usize) -> LocalBoxFuture<io::Result<BoxStream>> {
            Box::pin(async { Err(io::Error::other("none")) })
        }
        fn dns_label(&self, _: usize) -> String {
            String::new()
        }
        fn dns_route(&self, _: &[u8]) -> DnsRoute {
            DnsRoute::Servers(Vec::new())
        }
        fn dns_direct(&self, _: SocketAddr, _: Vec<u8>) -> LocalBoxFuture<Option<Vec<u8>>> {
            Box::pin(async { None })
        }
        fn udp_route(&self, _: SocketAddr, _: SocketAddr) -> Option<u64> {
            Some(0)
        }
        fn udp(&self, mut rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>, _: UdpReply) -> LocalBoxFuture<()> {
            Box::pin(async move { while rx.recv().await.is_some() {} })
        }
    }

    #[cfg(not(target_vendor = "apple"))]
    #[tokio::test]
    async fn udp_sessions_are_capped() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (tun, _r) = pipe_tun();
                let handler: Rc<dyn Handler> = Rc::new(LiveUdp);
                let (ev_tx, _ev_rx) = mpsc::unbounded_channel();
                let mut udp = UdpSessions::default();
                let dst = "1.1.1.1:443".parse().unwrap();
                for i in 0..UDP_MAX as u32 + 100 {
                    let src = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + i / 60000)), (i % 60000 + 1) as u16);
                    udp_packet(&mut udp, (src, 0), dst, b"x", &handler, &ev_tx, &tun);
                }
                assert_eq!(udp.live.len(), UDP_MAX);
            })
            .await;
    }

    #[cfg(not(target_vendor = "apple"))]
    #[tokio::test]
    async fn failing_udp_flow_is_refused_for_a_while_instead_of_redialed_per_packet() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (tun, _r) = pipe_tun();
                let h = Rc::new(FailingUdp { sessions: Cell::new(0) });
                let handler: Rc<dyn Handler> = h.clone();
                let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
                let mut udp = UdpSessions::default();
                let key = ("172.19.0.1:5000".parse().unwrap(), 0);
                let dst = "1.1.1.1:443".parse().unwrap();
                assert!(udp_packet(&mut udp, key, dst, b"x", &handler, &ev_tx, &tun));
                let Some(Event::UdpFailed(k)) = ev_rx.recv().await else { panic!("ждали отказ") };
                udp.ended(k, true);
                for _ in 0..20 {
                    assert!(!udp_packet(&mut udp, key, dst, b"x", &handler, &ev_tx, &tun), "пока отсрочка — ICMP, без дозвона");
                }
                assert_eq!(h.sessions.get(), 1);
                udp.failed.insert(key, Instant::now() - UDP_BACKOFF);
                assert!(udp_packet(&mut udp, key, dst, b"x", &handler, &ev_tx, &tun), "после отсрочки — снова попытка");
                assert_eq!(h.sessions.get(), 2);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn listener_backoff_grows_and_stops_after_sustained_exhaustion() {
        for code in [libc::EMFILE, libc::ENOMEM] {
            let start = tokio::time::Instant::now();
            let mut retry = None;
            let mut previous = Duration::ZERO;
            loop {
                match listener_backoff(&mut retry, Duration::from_millis(50), io::Error::from_raw_os_error(code)) {
                    Ok(()) => {
                        let r = retry.as_ref().unwrap();
                        assert!(r.pause > previous || r.pause == Duration::from_secs(30));
                        assert!(r.at <= start + LISTENER_FAILURE);
                        previous = r.pause;
                        tokio::time::advance(r.at - tokio::time::Instant::now()).await;
                    }
                    Err(e) => {
                        assert!(e.to_string().contains("TUN listener unavailable"));
                        break;
                    }
                }
            }
            assert_eq!(tokio::time::Instant::now() - start, LISTENER_FAILURE);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn backoff_leaves_other_listener_and_packets_running() {
        let a = Some(TcpListener::bind("127.0.0.1:0").await.unwrap());
        let b = Some(TcpListener::bind("127.0.0.1:0").await.unwrap());
        let _ca = TcpStream::connect(a.as_ref().unwrap().local_addr().unwrap()).await.unwrap();
        let _cb = TcpStream::connect(b.as_ref().unwrap().local_addr().unwrap()).await.unwrap();
        let mut retry = None;
        listener_backoff(&mut retry, Duration::from_millis(50), io::Error::from_raw_os_error(libc::EMFILE)).unwrap();
        let now = tokio::time::Instant::now();
        tokio::select! { biased; _ = accept(&a, &retry) => panic!("accept bypassed backoff"), r = accept(&b, &None) => { r.unwrap(); } }
        assert_eq!(tokio::time::Instant::now(), now);
        let q = Packets::default();
        q.push(vec![4, 5]);
        let mut got = Vec::new();
        q.drain(TUN_BATCH, |p, _| {
            got.extend_from_slice(p);
            Ok(())
        })
        .unwrap();
        assert_eq!(got, [4, 5]);
        tokio::time::advance(Duration::from_millis(50)).await;
        accept(&a, &retry).await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn failed_listener_restore_exits_after_one_minute() {
        let mut l = None;
        let mut port = 0;
        let mut retry = None;
        let start = tokio::time::Instant::now();
        loop {
            let r = rebind(&mut l, &mut port, Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1))), &mut retry, "test".into()).await;
            if let Err(e) = r {
                assert!(e.to_string().contains("TUN listener unavailable"));
                break;
            }
            assert!(l.is_none());
            retry_at(&retry).await;
        }
        assert_eq!(tokio::time::Instant::now() - start, LISTENER_FAILURE);
    }

    #[tokio::test]
    async fn destroyed_listener_is_rebound() {
        let local = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mut l = Some(TcpListener::bind((local, 0)).await.unwrap());
        let mut port = l.as_ref().unwrap().local_addr().unwrap().port();
        unsafe { libc::shutdown(l.as_ref().unwrap().as_raw_fd(), libc::SHUT_RDWR) };
        let e = accept(&l, &None).await.unwrap_err();
        assert!(listener_dead(&e), "{e}");

        let mut retry = None;
        rebind(&mut l, &mut port, Some(local), &mut retry, e.to_string()).await.unwrap();
        let (_c, (_s, _)) = tokio::join!(TcpStream::connect((local, port)), async { accept(&l, &retry).await.unwrap() });
    }

    struct DnsOnly {
        tcp_calls: Cell<u32>,
    }

    impl Handler for DnsOnly {
        fn tcp(&self, _: TcpConn, _: SocketAddr, _: SocketAddr) -> LocalBoxFuture<()> {
            self.tcp_calls.set(self.tcp_calls.get() + 1);
            Box::pin(async {})
        }
        fn dns_stream(&self, _: usize) -> LocalBoxFuture<io::Result<BoxStream>> {
            Box::pin(async { Err(io::Error::other("none")) })
        }
        fn dns_label(&self, _: usize) -> String {
            String::new()
        }
        fn dns_route(&self, q: &[u8]) -> DnsRoute {
            let mut r = q.to_vec();
            r[2] = 0x81;
            DnsRoute::Reply(r)
        }
        fn dns_direct(&self, _: SocketAddr, _: Vec<u8>) -> LocalBoxFuture<Option<Vec<u8>>> {
            Box::pin(async { None })
        }
        fn udp_route(&self, _: SocketAddr, _: SocketAddr) -> Option<u64> {
            None
        }
        fn udp(&self, _: mpsc::Receiver<(SocketAddr, Vec<u8>)>, _: UdpReply) -> LocalBoxFuture<()> {
            Box::pin(async {})
        }
    }

    #[tokio::test]
    async fn tcp_to_port_53_is_served_by_the_core_dns() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::task::LocalSet::new()
            .run_until(async {
                let local = IpAddr::V4(Ipv4Addr::LOCALHOST);
                let listener = TcpListener::bind((local, 0)).await.unwrap();
                let addr = listener.local_addr().unwrap();
                let h = Rc::new(DnsOnly { tcp_calls: Cell::new(0) });
                let handler: Rc<dyn Handler> = h.clone();
                let (_net_tx, net) = watch::channel(0u64);
                let dns = dns::Client::new(handler.clone(), net, Rc::new(Packets::default()));
                let (ev_tx, _ev_rx) = mpsc::unbounded_channel();
                let mut nat = Nat::new();
                for (dst, want_dns) in [("192.168.77.1:53", true), ("192.168.77.1:443", false)] {
                    let key = FlowKey { src: "10.0.0.2:40000".parse().unwrap(), dst: dst.parse().unwrap() };
                    let port = nat.alloc(key).unwrap();
                    let sock = tokio::net::TcpSocket::new_v4().unwrap();
                    sock.bind((local, port).into()).unwrap();
                    let (client, accepted) = tokio::join!(sock.connect(addr), listener.accept());
                    let mut client = client.unwrap();
                    spawn_conn(accepted.unwrap(), Some(local), &mut nat, &handler, &dns, &ev_tx);
                    if want_dns {
                        let q = [0xab, 0xcd, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, b'a', 0, 0, 1, 0, 1];
                        client.write_all(&[0, q.len() as u8]).await.unwrap();
                        client.write_all(&q).await.unwrap();
                        let mut len = [0u8; 2];
                        client.read_exact(&mut len).await.unwrap();
                        let mut r = vec![0u8; u16::from_be_bytes(len) as usize];
                        client.read_exact(&mut r).await.unwrap();
                        assert_eq!(&r[..2], &q[..2]);
                        assert_eq!(r[2], 0x81);
                        assert_eq!(h.tcp_calls.get(), 0, "в обработчик обычных соединений не ушло");
                    } else {
                        tokio::task::yield_now().await;
                        assert_eq!(h.tcp_calls.get(), 1);
                    }
                }
            })
            .await;
    }
}
