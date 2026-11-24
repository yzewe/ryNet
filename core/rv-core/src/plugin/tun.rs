use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};
use std::time::Duration;

use rv_plugin_abi::{err, ev};
use rv_vless::Address;
use serde_json::Value;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use super::Ctx;
use super::outbound::PluginOutbound;
use super::res::{Op, Shared};
use crate::{BoxStream, Error};

const QUEUE: usize = 1024;
const TCP_RX: usize = 512 * 1024;
const TCP_TX: usize = 256 * 1024;
const MAX_SOCKETS: usize = 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const LINGER: Duration = Duration::from_secs(30);
const DNS_TRIES: [Duration; 3] = [Duration::from_millis(1500), Duration::from_secs(3), Duration::from_secs(4)];

struct Dev {
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
    mtu: usize,
}

struct RxTok(Vec<u8>);
struct TxTok<'a>(&'a mut VecDeque<Vec<u8>>);

impl phy::RxToken for RxTok {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl phy::TxToken for TxTok<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut b = vec![0u8; len];
        let r = f(&mut b);
        self.0.push_back(b);
        r
    }
}

impl Device for Dev {
    type RxToken<'a> = RxTok;
    type TxToken<'a> = TxTok<'a>;

    fn receive(&mut self, _: smoltcp::time::Instant) -> Option<(RxTok, TxTok<'_>)> {
        let p = self.rx.pop_front()?;
        Some((RxTok(p), TxTok(&mut self.tx)))
    }

    fn transmit(&mut self, _: smoltcp::time::Instant) -> Option<TxTok<'_>> {
        (self.tx.len() < QUEUE).then(|| TxTok(&mut self.tx))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut c = DeviceCapabilities::default();
        c.medium = Medium::Ip;
        c.max_transmission_unit = self.mtu;
        c
    }
}

pub(super) struct Stack {
    iface: RefCell<Interface>,
    dev: RefCell<Dev>,
    sockets: RefCell<SocketSet<'static>>,
    poke: tokio::sync::Notify,
    want_read: Cell<bool>,
    id: Cell<u32>,
    shared: Weak<Shared>,
    v4: bool,
    v6: bool,
    dns: Vec<IpAddr>,
    dns_cache: RefCell<HashMap<String, (Vec<IpAddr>, std::time::Instant)>>,
    orphans: RefCell<Vec<(SocketHandle, std::time::Instant)>>,
    task: RefCell<Option<tokio::task::AbortHandle>>,
    closed: Cell<bool>,
    port: Cell<u16>,
    born: std::time::Instant,
}

impl Stack {
    fn now(&self) -> smoltcp::time::Instant {
        smoltcp::time::Instant::from_millis(self.born.elapsed().as_millis() as i64)
    }

    fn poll(&self) -> Option<Duration> {
        let now = self.now();
        let mut iface = self.iface.borrow_mut();
        let mut socks = self.sockets.borrow_mut();
        iface.poll(now, &mut *self.dev.borrow_mut(), &mut socks);
        let reap = std::mem::take(&mut *self.orphans.borrow_mut());
        let mut keep = Vec::new();
        for (h, at) in reap {
            let s = socks.get_mut::<tcp::Socket>(h);
            let gone = matches!(s.state(), tcp::State::Closed | tcp::State::TimeWait);
            if gone {
                socks.remove(h);
            } else if at.elapsed() > LINGER {
                s.abort();
                keep.push((h, at));
            } else {
                keep.push((h, at));
            }
        }
        *self.orphans.borrow_mut() = keep;
        let d = iface.poll_delay(now, &socks).map(Duration::from);
        drop((iface, socks));
        if !self.dev.borrow().tx.is_empty() && self.want_read.replace(false) {
            self.push(ev::READABLE);
        }
        d
    }

    fn push(&self, kind: u32) {
        if let Some(s) = self.shared.upgrade() {
            s.push(kind, self.id.get(), 0);
        }
    }

    fn next_port(&self) -> u16 {
        let p = self.port.get();
        self.port.set(if p >= 65535 { 49152 } else { p + 1 });
        p
    }

    fn room(&self) -> Result<(), Error> {
        if self.closed.get() {
            return Err(Error::Io(io::Error::new(io::ErrorKind::NotConnected, "tunnel is down")));
        }
        if self.sockets.borrow().iter().count() >= MAX_SOCKETS {
            return Err(Error::Io(io::Error::other("tunnel: too many connections")));
        }
        Ok(())
    }

    fn family_ok(&self, ip: IpAddr) -> Result<(), Error> {
        match ip {
            IpAddr::V4(_) if !self.v4 => Err(Error::Io(io::Error::new(io::ErrorKind::AddrNotAvailable, "the tunnel has no IPv4 address"))),
            IpAddr::V6(_) if !self.v6 => Err(Error::Io(io::Error::new(io::ErrorKind::AddrNotAvailable, "the tunnel has no IPv6 address"))),
            _ => Ok(()),
        }
    }

    fn shutdown(&self) {
        if self.closed.replace(true) {
            return;
        }
        if let Some(t) = self.task.borrow_mut().take() {
            t.abort();
        }
        for (_, s) in self.sockets.borrow_mut().iter_mut() {
            match s {
                smoltcp::socket::Socket::Tcp(t) => t.abort(),
                smoltcp::socket::Socket::Udp(u) => u.close(),
            }
        }
        self.dev.borrow_mut().rx.clear();
        self.dev.borrow_mut().tx.clear();
        if let Some(s) = self.shared.upgrade() {
            s.tun.borrow_mut().take();
        }
    }

    async fn resolve(self: &Rc<Self>, name: &str) -> Result<Vec<IpAddr>, Error> {
        if let Some((ips, until)) = self.dns_cache.borrow().get(name)
            && *until > std::time::Instant::now()
        {
            return Ok(ips.clone());
        }
        let mut last = Error::Io(io::Error::new(io::ErrorKind::NotFound, format!("{name}: no address")));
        let mut types = Vec::new();
        if self.v4 {
            types.push(1u16);
        }
        if self.v6 {
            types.push(28u16);
        }
        for server in self.dns.clone() {
            if self.family_ok(server).is_err() {
                continue;
            }
            for &qt in &types {
                let q = crate::dns::query_for(name, qt);
                match self.exchange(SocketAddr::new(server, 53), &q).await {
                    Ok(reply) => {
                        let ips: Vec<IpAddr> = crate::dns::addresses(&reply).into_iter().filter(|ip| self.family_ok(*ip).is_ok()).collect();
                        if !ips.is_empty() {
                            let ttl = crate::dns::answer_ttl(&reply).clamp(30, 600);
                            let mut c = self.dns_cache.borrow_mut();
                            if c.len() > 512 {
                                c.clear();
                            }
                            c.insert(name.to_string(), (ips.clone(), std::time::Instant::now() + Duration::from_secs(ttl.into())));
                            return Ok(ips);
                        }
                    }
                    Err(e) => last = e,
                }
            }
        }
        Err(last)
    }

    async fn exchange(self: &Rc<Self>, to: SocketAddr, q: &[u8]) -> Result<Vec<u8>, Error> {
        let u = UdpSock::new(self)?;
        for wait in DNS_TRIES {
            u.send(to, q)?;
            match tokio::time::timeout(wait, poll_fn(|cx| u.poll_recv(cx))).await {
                Ok(Ok((_, d))) => return Ok(d),
                Ok(Err(e)) => return Err(Error::Io(e)),
                Err(_) => {}
            }
        }
        Err(Error::Io(io::Error::new(io::ErrorKind::TimedOut, format!("DNS {to} did not answer through the tunnel"))))
    }
}

async fn run(s: Rc<Stack>) {
    loop {
        let poked = s.poke.notified();
        tokio::pin!(poked);
        poked.as_mut().enable();
        let delay = s.poll().unwrap_or(Duration::from_secs(60));
        if delay.is_zero() {
            tokio::task::yield_now().await;
            continue;
        }
        tokio::select! {
            _ = poked => {}
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

pub(super) struct TunRes {
    pub op: Op,
    stack: Rc<Stack>,
}

impl TunRes {
    pub fn recv(&self, out: &mut [u8]) -> Result<usize, (i32, String)> {
        if self.stack.closed.get() {
            return Err((err::CLOSED, "tunnel closed".into()));
        }
        let mut dev = self.stack.dev.borrow_mut();
        let Some(p) = dev.tx.front() else {
            self.stack.want_read.set(true);
            return Err((err::AGAIN, String::new()));
        };
        if p.len() > out.len() {
            return Err((err::TOO_SMALL, format!("packet of {} bytes", p.len())));
        }
        let p = dev.tx.pop_front().unwrap();
        out[..p.len()].copy_from_slice(&p);
        drop(dev);
        self.stack.poke.notify_one();
        Ok(p.len())
    }

    pub fn send(&self, data: &[u8]) -> Result<usize, (i32, String)> {
        if self.stack.closed.get() {
            return Err((err::CLOSED, "tunnel closed".into()));
        }
        let mtu = self.stack.dev.borrow().mtu;
        if data.len() < 20 || data.len() > mtu.max(1280) + 80 || !matches!(data[0] >> 4, 4 | 6) {
            return Err((err::INVALID, format!("not an IP packet ({} bytes)", data.len())));
        }
        let mut dev = self.stack.dev.borrow_mut();
        if dev.rx.len() < QUEUE {
            dev.rx.push_back(data.to_vec());
        }
        drop(dev);
        self.stack.poke.notify_one();
        Ok(data.len())
    }

    pub fn close(&self) {
        self.stack.shutdown();
    }
}

fn arg_err(m: impl Into<String>) -> (i32, String) {
    (err::INVALID, m.into())
}

pub(super) fn open(ctx: &Rc<Ctx>, shared: &Rc<Shared>, v: &Value) -> Result<u32, (i32, String)> {
    if ctx.plugin.manifest.kind != super::Kind::Tunnel {
        return Err((err::DENIED, "only tunnel plugins can open a TUN".into()));
    }
    if shared.tun.borrow().is_some() {
        return Err(arg_err("the TUN is already open"));
    }
    let mtu = v.get("mtu").and_then(Value::as_u64).unwrap_or(1420);
    if !(576..=9000).contains(&mtu) {
        return Err(arg_err("mtu must be 576..9000"));
    }
    let list = |k: &str| -> Vec<String> {
        match v.get(k) {
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(String::from).collect(),
            Some(Value::String(s)) => s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            _ => Vec::new(),
        }
    };
    let mut addrs = Vec::new();
    for a in list("address") {
        let (ip, len) = match a.split_once('/') {
            Some((ip, l)) => (
                ip.parse::<IpAddr>().map_err(|_| arg_err(format!("bad address {a}")))?,
                l.parse::<u8>().map_err(|_| arg_err(format!("bad prefix in {a}")))?,
            ),
            None => {
                let ip = a.parse::<IpAddr>().map_err(|_| arg_err(format!("bad address {a}")))?;
                (ip, if ip.is_ipv4() { 32 } else { 128 })
            }
        };
        addrs.push(IpCidr::new(IpAddress::from(ip), len));
    }
    if addrs.is_empty() || addrs.len() > 4 {
        return Err(arg_err("1..4 tunnel addresses are needed"));
    }
    let mut dns: Vec<IpAddr> = list("dns").iter().filter_map(|s| s.parse().ok()).collect();
    let v4 = addrs.iter().any(|c| matches!(c, IpCidr::Ipv4(_)));
    let v6 = addrs.iter().any(|c| matches!(c, IpCidr::Ipv6(_)));
    if dns.is_empty() {
        if v4 {
            dns.push(IpAddr::from([1, 1, 1, 1]));
        }
        if v6 {
            dns.push("2606:4700:4700::1111".parse().unwrap());
        }
    }
    dns.truncate(4);
    let mut dev = Dev { rx: VecDeque::new(), tx: VecDeque::new(), mtu: mtu as usize };
    let mut cfg = Config::new(HardwareAddress::Ip);
    let mut seed = [0u8; 8];
    {
        use ring::rand::SecureRandom;
        let _ = ring::rand::SystemRandom::new().fill(&mut seed);
    }
    cfg.random_seed = u64::from_le_bytes(seed);
    let born = std::time::Instant::now();
    let mut iface = Interface::new(cfg, &mut dev, smoltcp::time::Instant::from_millis(0));
    iface.update_ip_addrs(|a| {
        for c in &addrs {
            let _ = a.push(*c);
        }
    });
    for c in &addrs {
        match c {
            IpCidr::Ipv4(c) => {
                let _ = iface.routes_mut().add_default_ipv4_route(c.address());
            }
            IpCidr::Ipv6(c) => {
                let _ = iface.routes_mut().add_default_ipv6_route(c.address());
            }
        }
    }
    let port = 49152 + (u16::from_le_bytes([seed[0], seed[1]]) % 16000);
    let stack = Rc::new(Stack {
        iface: RefCell::new(iface),
        dev: RefCell::new(dev),
        sockets: RefCell::new(SocketSet::new(Vec::new())),
        poke: tokio::sync::Notify::new(),
        want_read: Cell::new(false),
        id: Cell::new(0),
        shared: Rc::downgrade(shared),
        v4,
        v6,
        dns,
        dns_cache: RefCell::new(HashMap::new()),
        orphans: RefCell::new(Vec::new()),
        task: RefCell::new(None),
        closed: Cell::new(false),
        port: Cell::new(port),
        born,
    });
    let r = Rc::new(TunRes { op: Op::default(), stack: stack.clone() });
    let h = shared.table.borrow_mut().insert(super::res::Res::Tun(r)).ok_or((err::LIMIT, "too many handles".to_string()))?;
    stack.id.set(h);
    *stack.task.borrow_mut() = Some(tokio::task::spawn_local(run(stack.clone())).abort_handle());
    *shared.tun.borrow_mut() = Some(stack);
    shared.tun_up.notify_waiters();
    Ok(h)
}

async fn stack(o: &PluginOutbound) -> Result<(Rc<super::instance::Instance>, Rc<Stack>), Error> {
    let inst = o.instance_for_tun()?;
    let wait = async {
        loop {
            let up = inst.shared.tun_up.notified();
            tokio::pin!(up);
            up.as_mut().enable();
            if let Some(s) = inst.shared.tun.borrow().clone() {
                return Ok(s);
            }
            if inst.dead() {
                return Err(Error::Io(io::Error::other("plugin stopped")));
            }
            up.await;
        }
    };
    let s = tokio::time::timeout(CONNECT_TIMEOUT, wait)
        .await
        .map_err(|_| Error::Io(io::Error::new(io::ErrorKind::TimedOut, "the plugin did not open its tunnel")))??;
    Ok((inst, s))
}

struct AppGuard(Rc<Shared>);

impl AppGuard {
    fn new(s: &Rc<Shared>) -> Self {
        s.apps.set(s.apps.get() + 1);
        Self(s.clone())
    }
}

impl Drop for AppGuard {
    fn drop(&mut self) {
        self.0.apps.set(self.0.apps.get().saturating_sub(1));
    }
}

pub(super) async fn connect(o: &PluginOutbound, dest: Address, port: u16) -> Result<BoxStream, Error> {
    let (inst, s) = stack(o).await?;
    let guard = AppGuard::new(&inst.shared);
    drop(inst);
    let ips = match &dest {
        Address::Ipv4(a) => vec![IpAddr::from(*a)],
        Address::Ipv6(a) => vec![IpAddr::from(*a)],
        Address::Domain(d) => match d.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => s.resolve(d).await?,
        },
    };
    let ip = ips
        .into_iter()
        .find(|ip| s.family_ok(*ip).is_ok())
        .ok_or_else(|| Error::Io(io::Error::new(io::ErrorKind::AddrNotAvailable, "no address of the tunnel's family")))?;
    s.room()?;
    let mut sock = tcp::Socket::new(tcp::SocketBuffer::new(vec![0; TCP_RX]), tcp::SocketBuffer::new(vec![0; TCP_TX]));
    sock.set_nagle_enabled(false);
    let h = s.sockets.borrow_mut().add(sock);
    let stream = TunStream { s: s.clone(), h, shut: false, _app: guard };
    {
        let mut socks = s.sockets.borrow_mut();
        let mut iface = s.iface.borrow_mut();
        let local = s.next_port();
        socks
            .get_mut::<tcp::Socket>(h)
            .connect(iface.context(), IpEndpoint::new(IpAddress::from(ip), port), local)
            .map_err(|e| Error::Io(io::Error::new(io::ErrorKind::AddrNotAvailable, format!("tunnel connect: {e}"))))?;
    }
    s.poke.notify_one();
    let ready = poll_fn(|cx| {
        if s.closed.get() {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::NotConnected, "tunnel is down")));
        }
        let mut socks = s.sockets.borrow_mut();
        let t = socks.get_mut::<tcp::Socket>(h);
        match t.state() {
            tcp::State::SynSent | tcp::State::SynReceived => {
                t.register_send_waker(cx.waker());
                Poll::Pending
            }
            tcp::State::Closed => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionRefused, format!("{ip}:{port} refused through the tunnel"))))
            }
            _ => Poll::Ready(Ok(())),
        }
    });
    match tokio::time::timeout(CONNECT_TIMEOUT, ready).await {
        Ok(Ok(())) => Ok(Box::new(stream)),
        Ok(Err(e)) => Err(Error::Io(e)),
        Err(_) => Err(Error::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("{ip}:{port}: no answer through the tunnel (is the handshake done?)"),
        ))),
    }
}

struct TunStream {
    s: Rc<Stack>,
    h: SocketHandle,
    shut: bool,
    _app: AppGuard,
}

impl TunStream {
    fn down(&self) -> Option<io::Error> {
        self.s.closed.get().then(|| io::Error::new(io::ErrorKind::ConnectionAborted, "tunnel closed"))
    }
}

impl AsyncRead for TunStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if let Some(e) = self.down() {
            return Poll::Ready(Err(e));
        }
        let mut socks = self.s.sockets.borrow_mut();
        let t = socks.get_mut::<tcp::Socket>(self.h);
        if t.can_recv() {
            let n = t.recv_slice(buf.initialize_unfilled()).map_err(|e| io::Error::other(e.to_string()))?;
            buf.advance(n);
            drop(socks);
            self.s.poke.notify_one();
            return Poll::Ready(Ok(()));
        }
        if !t.may_recv() && !matches!(t.state(), tcp::State::SynSent | tcp::State::SynReceived) {
            if t.state() == tcp::State::Closed && !self.shut {
                return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
            }
            return Poll::Ready(Ok(()));
        }
        t.register_recv_waker(cx.waker());
        Poll::Pending
    }
}

impl AsyncWrite for TunStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        if let Some(e) = self.down() {
            return Poll::Ready(Err(e));
        }
        let mut socks = self.s.sockets.borrow_mut();
        let t = socks.get_mut::<tcp::Socket>(self.h);
        if matches!(t.state(), tcp::State::SynSent | tcp::State::SynReceived) {
            t.register_send_waker(cx.waker());
            return Poll::Pending;
        }
        if !t.may_send() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if t.can_send() {
            let n = t.send_slice(data).map_err(|e| io::Error::other(e.to_string()))?;
            drop(socks);
            self.s.poke.notify_one();
            return Poll::Ready(Ok(n));
        }
        t.register_send_waker(cx.waker());
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.shut && !self.s.closed.get() {
            self.shut = true;
            self.s.sockets.borrow_mut().get_mut::<tcp::Socket>(self.h).close();
            self.s.poke.notify_one();
        }
        Poll::Ready(Ok(()))
    }
}

impl Drop for TunStream {
    fn drop(&mut self) {
        if self.s.closed.get() {
            return;
        }
        self.s.sockets.borrow_mut().get_mut::<tcp::Socket>(self.h).close();
        self.s.orphans.borrow_mut().push((self.h, std::time::Instant::now()));
        self.s.poke.notify_one();
    }
}

struct UdpSock {
    s: Rc<Stack>,
    h: SocketHandle,
}

impl UdpSock {
    fn new(s: &Rc<Stack>) -> Result<Self, Error> {
        s.room()?;
        let meta = || vec![udp::PacketMetadata::EMPTY; 64];
        let mut u =
            udp::Socket::new(udp::PacketBuffer::new(meta(), vec![0; 128 * 1024]), udp::PacketBuffer::new(meta(), vec![0; 128 * 1024]));
        let mut bound = false;
        for _ in 0..16 {
            if u.bind(s.next_port()).is_ok() {
                bound = true;
                break;
            }
        }
        if !bound {
            return Err(Error::Io(io::Error::other("tunnel: no free UDP port")));
        }
        let h = s.sockets.borrow_mut().add(u);
        Ok(Self { s: s.clone(), h })
    }

    fn send(&self, to: SocketAddr, data: &[u8]) -> Result<(), Error> {
        self.s.family_ok(to.ip())?;
        if self.s.closed.get() {
            return Err(Error::Io(io::Error::new(io::ErrorKind::NotConnected, "tunnel is down")));
        }
        let _ = self
            .s
            .sockets
            .borrow_mut()
            .get_mut::<udp::Socket>(self.h)
            .send_slice(data, IpEndpoint::new(IpAddress::from(to.ip()), to.port()));
        self.s.poke.notify_one();
        Ok(())
    }

    fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<io::Result<(SocketAddr, Vec<u8>)>> {
        if self.s.closed.get() {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionAborted, "tunnel closed")));
        }
        let mut socks = self.s.sockets.borrow_mut();
        let u = socks.get_mut::<udp::Socket>(self.h);
        match u.recv() {
            Ok((d, meta)) => {
                let from = SocketAddr::new(IpAddr::from(meta.endpoint.addr), meta.endpoint.port);
                Poll::Ready(Ok((from, d.to_vec())))
            }
            Err(_) => {
                u.register_recv_waker(cx.waker());
                Poll::Pending
            }
        }
    }
}

impl Drop for UdpSock {
    fn drop(&mut self) {
        if !self.s.closed.get() {
            self.s.sockets.borrow_mut().remove(self.h);
        }
    }
}

pub(super) async fn udp_session(
    o: &PluginOutbound,
    first: (SocketAddr, Vec<u8>),
    rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
    reply: &dyn Fn(SocketAddr, &[u8]),
) -> Result<(), Error> {
    let (inst, s) = stack(o).await?;
    let _app = AppGuard::new(&inst.shared);
    drop(inst);
    let u = UdpSock::new(&s)?;
    u.send(first.0, &first.1)?;
    let idle = tokio::time::sleep(crate::UDP_IDLE);
    tokio::pin!(idle);
    let mut last = tokio::time::Instant::now();
    loop {
        tokio::select! {
            p = rx.recv() => {
                let Some((dst, data)) = p else { return Ok(()) };
                u.send(dst, &data)?;
                crate::udp_touch(&mut last, idle.as_mut(), crate::UDP_IDLE);
            }
            r = poll_fn(|cx| u.poll_recv(cx)) => {
                let (src, d) = r.map_err(Error::Io)?;
                reply(src, &d);
                crate::udp_touch(&mut last, idle.as_mut(), crate::UDP_IDLE);
            }
            _ = &mut idle => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_speaks_tcp_and_dns_through_packets() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async {
            let m = serde_json::json!({ "id": "dev.rynet.tuntest", "name": "T", "version": "1", "api": 1, "kind": "tunnel", "protocols": ["tuntest"] });
            let data: String = m.to_string().bytes().map(|b| format!("\\{b:02x}")).collect();
            let wasm = wat::parse_str(format!(r#"(module (@custom "rynet.manifest" "{data}") (memory (export "memory") 1) (func (export "rn_init") (result i32) i32.const 0) (func (export "rn_event") (param i32 i32 i32) (result i32) i32.const 0))"#)).unwrap();
            let (manifest, module) = super::super::load(&wasm).unwrap();
            let plugin = std::sync::Arc::new(super::super::Plugin {
                sha256: String::new(),
                size: 0,
                manifest,
                module,
                grants: Default::default(),
                diag: Default::default(),
                kv: Default::default(),
                dir: None,
            });
            let ctx = Rc::new(Ctx::offline(plugin));
            let shared = Shared::new();
            let h = open(&ctx, &shared, &serde_json::json!({ "address": ["10.7.0.2/32"], "mtu": 1400, "dns": ["10.7.0.1"] })).unwrap();
            let Some(super::super::res::Res::Tun(tun)) = shared.get(h) else { panic!() };
            let st = shared.tun.borrow().clone().unwrap();
            assert!(open(&ctx, &shared, &serde_json::json!({ "address": ["10.7.0.3"] })).is_err(), "one TUN per instance");

            let mut sdev = Dev { rx: VecDeque::new(), tx: VecDeque::new(), mtu: 1400 };
            let mut siface = Interface::new(Config::new(HardwareAddress::Ip), &mut sdev, smoltcp::time::Instant::from_millis(0));
            siface.update_ip_addrs(|a| {
                let _ = a.push(IpCidr::new(IpAddress::v4(10, 7, 0, 1), 24));
            });
            let _ = siface.routes_mut().add_default_ipv4_route(smoltcp::wire::Ipv4Address::new(10, 7, 0, 1));
            let mut ssocks = SocketSet::new(Vec::new());
            let mut l = tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 65536]), tcp::SocketBuffer::new(vec![0; 65536]));
            l.listen(7).unwrap();
            let lh = ssocks.add(l);
            let meta = || vec![udp::PacketMetadata::EMPTY; 8];
            let mut d = udp::Socket::new(udp::PacketBuffer::new(meta(), vec![0; 4096]), udp::PacketBuffer::new(meta(), vec![0; 4096]));
            d.bind(53).unwrap();
            let dh = ssocks.add(d);

            let server = async {
                let mut buf = vec![0u8; 2048];
                loop {
                    loop {
                        match tun.recv(&mut buf) {
                            Ok(n) => sdev.rx.push_back(buf[..n].to_vec()),
                            Err((e, _)) if e == err::AGAIN => break,
                            Err(e) => panic!("{e:?}"),
                        }
                    }
                    siface.poll(st.now(), &mut sdev, &mut ssocks);
                    let t = ssocks.get_mut::<tcp::Socket>(lh);
                    let room = t.send_capacity() - t.send_queue();
                    if t.can_recv() && room > 0 {
                        let mut b = vec![0u8; room];
                        let n = t.recv_slice(&mut b).unwrap();
                        assert_eq!(t.send_slice(&b[..n]).unwrap(), n);
                    }
                    if t.state() == tcp::State::CloseWait && t.recv_queue() == 0 {
                        t.close();
                    }
                    let u = ssocks.get_mut::<udp::Socket>(dh);
                    if let Ok((q, meta)) = u.recv() {
                        let q = q.to_vec();
                        let qn = crate::dns::question(&q).unwrap();
                        let a = crate::dns::answer(&q, &qn, &[IpAddr::from([10, 7, 0, 1])], 300);
                        u.send_slice(&a, meta.endpoint).unwrap();
                    }
                    siface.poll(st.now(), &mut sdev, &mut ssocks);
                    while let Some(p) = sdev.tx.pop_front() {
                        tun.send(&p).unwrap();
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            };
            let client = async {
                let ips = st.resolve("echo.test").await.unwrap();
                assert_eq!(ips, vec![IpAddr::from([10, 7, 0, 1])]);
                let s = shared.clone();
                let o = (s.apps.get(),);
                assert_eq!(o.0, 0);
                let guard = AppGuard::new(&shared);
                let mut sock = tcp::Socket::new(tcp::SocketBuffer::new(vec![0; TCP_RX]), tcp::SocketBuffer::new(vec![0; TCP_TX]));
                sock.set_nagle_enabled(false);
                let h = st.sockets.borrow_mut().add(sock);
                {
                    let mut socks = st.sockets.borrow_mut();
                    let mut iface = st.iface.borrow_mut();
                    socks.get_mut::<tcp::Socket>(h).connect(iface.context(), IpEndpoint::new(IpAddress::v4(10, 7, 0, 1), 7), st.next_port()).unwrap();
                }
                st.poke.notify_one();
                let mut stream = TunStream { s: st.clone(), h, shut: false, _app: guard };
                assert_eq!(shared.apps.get(), 1);
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
                let (mut r, mut w) = tokio::io::split(&mut stream);
                let wr = async {
                    w.write_all(&data).await.unwrap();
                    w.shutdown().await.unwrap();
                };
                let rd = async {
                    let mut got = Vec::new();
                    r.read_to_end(&mut got).await.unwrap();
                    got
                };
                let (_, got) = tokio::join!(wr, rd);
                assert_eq!(got.len(), data.len());
                assert!(got == data);
                drop(stream);
                assert_eq!(shared.apps.get(), 0);
            };
            tokio::select! {
                _ = server => unreachable!(),
                r = tokio::time::timeout(Duration::from_secs(20), client) => r.expect("the exchange stalled"),
            }
            tun.close();
            assert!(shared.tun.borrow().is_none());
            assert_eq!(tun.recv(&mut [0u8; 64]).unwrap_err().0, err::CLOSED);
        });
    }
}
