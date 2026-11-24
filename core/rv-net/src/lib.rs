use socket2::{Domain, Protocol, Socket, Type};

type AttemptFuture<'a> = Pin<Box<dyn std::future::Future<Output = (usize, io::Result<TcpStream>)> + 'a>>;
use std::io;
use std::net::IpAddr;
#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{AsRawSocket, RawSocket as RawFd};

fn raw_socket(socket: &Socket) -> RawFd {
    #[cfg(unix)]
    {
        socket.as_raw_fd()
    }
    #[cfg(windows)]
    {
        socket.as_raw_socket()
    }
}
use std::sync::Arc;
use std::time::Duration;
use std::{future::poll_fn, net::SocketAddr, pin::Pin, task::Poll};

use tokio::net::TcpStream;

pub mod bootstrap;
mod coalesce;
pub use coalesce::Coalesced;
mod fragment;
pub use fragment::{FragmentConfig, FragmentStream};

pub type Protect = Arc<dyn Fn(RawFd) -> io::Result<()> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HappyEyeballs {
    pub try_delay: Duration,
    pub prioritize_ipv6: bool,
    pub interleave: u32,
    pub max_concurrent_try: u32,
}

impl Default for HappyEyeballs {
    fn default() -> Self {
        Self { try_delay: Duration::from_millis(250), prioritize_ipv6: false, interleave: 1, max_concurrent_try: 4 }
    }
}

#[derive(Clone, Default)]
pub struct DialOptions {
    pub tcp_keepalive: Option<(Duration, Duration)>,
    pub tcp_no_delay: bool,
    pub tcp_max_seg: Option<u32>,
    pub tcp_user_timeout: Option<Duration>,
    pub tcp_window_clamp: Option<u32>,
    pub tcp_congestion: Option<String>,
    pub family: Family,
    pub happy_eyeballs: Option<HappyEyeballs>,
    pub protect: Option<Protect>,
    pub timeout: Option<Duration>,
}

static HOSTS: std::sync::OnceLock<std::sync::RwLock<std::collections::HashMap<String, Vec<IpAddr>>>> = std::sync::OnceLock::new();

pub fn set_hosts(hosts: std::collections::HashMap<String, Vec<IpAddr>>) {
    let lock = HOSTS.get_or_init(Default::default);
    *lock.write().unwrap() = hosts;
}

pub fn add_hosts(hosts: std::collections::HashMap<String, Vec<IpAddr>>) {
    let lock = HOSTS.get_or_init(Default::default);
    let mut map = lock.write().unwrap_or_else(|e| e.into_inner());
    for (h, ips) in hosts {
        if !ips.is_empty() {
            map.insert(h, ips);
        }
    }
}

pub fn known_hosts(host: &str) -> Option<Vec<IpAddr>> {
    let mut ips = HOSTS.get()?.read().unwrap().get(host).cloned()?;
    ips.sort_by_key(|ip| ip.is_ipv6());
    (!ips.is_empty()).then_some(ips)
}

pub async fn resolve(host: &str) -> io::Result<Vec<IpAddr>> {
    resolve_protected(host, None).await
}

fn in_fake_range(ip: &IpAddr) -> bool {
    matches!(ip, IpAddr::V4(a) if a.octets()[0] == 198 && a.octets()[1] & 0xfe == 18)
}

static OWN_FAKE: std::sync::OnceLock<fn(IpAddr) -> bool> = std::sync::OnceLock::new();

pub fn set_own_fake(check: fn(IpAddr) -> bool) {
    let _ = OWN_FAKE.set(check);
}

static DIRECT_V6: std::sync::OnceLock<fn() -> bool> = std::sync::OnceLock::new();

pub fn set_direct_v6(check: fn() -> bool) {
    let _ = DIRECT_V6.set(check);
}

pub fn direct_v6() -> bool {
    DIRECT_V6.get().is_none_or(|check| check())
}

fn is_fake(ip: &IpAddr) -> bool {
    in_fake_range(ip) && OWN_FAKE.get().is_some_and(|own| own(*ip))
}

const SYSTEM_TTL: Duration = Duration::from_secs(300);

#[derive(Default)]
struct Answers(std::collections::HashMap<String, (Vec<IpAddr>, std::time::Instant)>);

impl Answers {
    fn get(&self, host: &str, now: std::time::Instant) -> Option<Vec<IpAddr>> {
        self.0.get(host).filter(|(_, until)| now < *until).map(|(ips, _)| ips.clone())
    }

    fn put(&mut self, host: &str, ips: &[IpAddr], now: std::time::Instant) {
        if self.0.len() >= 64 {
            self.0.retain(|_, (_, until)| now < *until);
        }
        self.0.insert(host.to_string(), (ips.to_vec(), now + SYSTEM_TTL));
    }
}

static SYSTEM_ANSWERS: std::sync::Mutex<Option<Answers>> = std::sync::Mutex::new(None);

fn system_answers() -> std::sync::MutexGuard<'static, Option<Answers>> {
    SYSTEM_ANSWERS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn forget_lookups() {
    *system_answers() = None;
    bootstrap::forget();
}

pub async fn resolve_protected(host: &str, protect: Option<&Protect>) -> io::Result<Vec<IpAddr>> {
    if let Ok(ip) = host.parse() {
        return Ok(vec![ip]);
    }
    if let Some(ips) = HOSTS.get().and_then(|h| h.read().unwrap().get(host).cloned())
        && !ips.is_empty()
    {
        return Ok(ips);
    }
    let mode = bootstrap::mode();
    let auto = mode == bootstrap::Bootstrap::Auto;
    if (auto || mode == bootstrap::Bootstrap::System)
        && let Some(ips) = system_answers().as_ref().and_then(|a| a.get(host, std::time::Instant::now()))
    {
        return Ok(ips);
    }
    if mode != bootstrap::Bootstrap::System && !(auto && bootstrap::resting(std::time::Instant::now())) {
        let r = Box::pin(bootstrap::resolve(host, protect, mode)).await;
        if auto {
            bootstrap::note(r.as_ref().map(|_| ()), std::time::Instant::now());
        }
        match r {
            Ok(ips) => {
                add_hosts([(host.to_string(), ips.clone())].into());
                return Ok(ips);
            }
            Err(e) if !auto => return Err(e),
            Err(_) => {}
        }
    }
    let lookup = async { tokio::net::lookup_host((host, 0)).await.map(|a| a.map(|s| s.ip()).collect::<Vec<_>>()) };
    let system = if SYSTEM_SERVERS.get().is_some() {
        let usable = |r: &io::Result<Vec<IpAddr>>| r.as_ref().is_ok_and(|v| v.iter().any(|ip| !is_fake(ip)));
        let direct = udp_lookup(host, protect);
        tokio::pin!(lookup, direct);
        let (mut sys, mut dir) = (None, None);
        loop {
            tokio::select! {
                r = &mut lookup, if sys.is_none() => {
                    if usable(&r) { break r; }
                    sys = Some(r);
                }
                r = &mut direct, if dir.is_none() => {
                    if usable(&r) { break r; }
                    dir = Some(r);
                }
            }
            if let (Some(_), Some(_)) = (&sys, &dir) {
                break sys.take().unwrap();
            }
        }
    } else {
        lookup.await
    };
    let mut out = Vec::new();
    let mut fake = false;
    if let Ok(addrs) = &system {
        for ip in addrs {
            if is_fake(ip) {
                fake = true;
            } else if !out.contains(ip) {
                out.push(*ip);
            }
        }
    }
    if out.is_empty() {
        return Err(match system {
            Err(e) => e,
            Ok(_) if fake => io::Error::new(io::ErrorKind::NotFound, format!("{host}: no usable address")),
            Ok(_) => io::Error::new(io::ErrorKind::NotFound, format!("{host}: no address")),
        });
    }
    system_answers().get_or_insert_with(Answers::default).put(host, &out, std::time::Instant::now());
    Ok(out)
}

static SYSTEM_SERVERS: std::sync::OnceLock<fn() -> Vec<IpAddr>> = std::sync::OnceLock::new();

pub fn set_system_servers(servers: fn() -> Vec<IpAddr>) {
    let _ = SYSTEM_SERVERS.set(servers);
}

async fn udp_lookup(host: &str, protect: Option<&Protect>) -> io::Result<Vec<IpAddr>> {
    use ring::rand::SecureRandom;
    let all = SYSTEM_SERVERS.get().map(|f| f()).unwrap_or_default();
    let v6 = !all.iter().any(IpAddr::is_ipv4);
    let servers: Vec<IpAddr> = all.into_iter().filter(|ip| ip.is_ipv6() == v6).take(3).collect();
    if servers.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no network DNS"));
    }
    let mut id = [0; 2];
    ring::rand::SystemRandom::new().fill(&mut id).map_err(|_| io::Error::other("DNS RNG"))?;
    let id = u16::from_ne_bytes(id);
    let q = dns_query(host, id).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad DNS name"))?;
    let sock = udp_socket(v6, protect)?;
    for server in &servers {
        let _ = sock.send_to(&q, SocketAddr::new(*server, 53)).await;
    }
    let answer = async {
        let mut buf = [0u8; 1500];
        loop {
            let (n, from) = sock.recv_from(&mut buf).await?;
            if servers.contains(&from.ip())
                && let Some(ips) = dns_answer(&buf[..n], id)
            {
                return Ok::<_, io::Error>(ips);
            }
        }
    };
    match tokio::time::timeout(Duration::from_secs(3), answer).await {
        Ok(Ok(ips)) if !ips.is_empty() => Ok(ips),
        Ok(Ok(_)) => Err(io::Error::new(io::ErrorKind::NotFound, format!("{host}: no address"))),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "network DNS timed out")),
    }
}

fn dns_query(host: &str, id: u16) -> Option<Vec<u8>> {
    let mut q = Vec::with_capacity(host.len() + 18);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    Some(q)
}

fn dns_answer(b: &[u8], id: u16) -> Option<Vec<IpAddr>> {
    if b.len() < 12 || u16::from_be_bytes([b[0], b[1]]) != id || b[2] & 0x82 != 0x80 || b[3] & 0x0f != 0 {
        return None;
    }
    let (qd, an) = (u16::from_be_bytes([b[4], b[5]]), u16::from_be_bytes([b[6], b[7]]));
    let skip_name = |mut i: usize| -> Option<usize> {
        loop {
            let l = *b.get(i)? as usize;
            if l == 0 {
                return Some(i + 1);
            }
            if l & 0xc0 == 0xc0 {
                b.get(i + 1)?;
                return Some(i + 2);
            }
            if l > 63 {
                return None;
            }
            i += 1 + l;
        }
    };
    let mut i = 12;
    for _ in 0..qd {
        i = skip_name(i)? + 4;
    }
    let mut out = Vec::new();
    for _ in 0..an {
        i = skip_name(i)?;
        let h = b.get(i..i + 10)?;
        let (ty, len) = (u16::from_be_bytes([h[0], h[1]]), u16::from_be_bytes([h[8], h[9]]) as usize);
        let data = b.get(i + 10..i + 10 + len)?;
        if h[2..4] == [0, 1] {
            if ty == 1 && len == 4 {
                out.push(IpAddr::from([data[0], data[1], data[2], data[3]]));
            } else if ty == 28 && len == 16 {
                out.push(IpAddr::from(<[u8; 16]>::try_from(data).unwrap()));
            }
        }
        i += 10 + len;
    }
    Some(out)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Family {
    #[default]
    Any,
    V4 {
        force: bool,
    },
    V6 {
        force: bool,
    },
    V4First,
    V6First,
}

impl Family {
    pub fn pick(self, mut ips: Vec<IpAddr>) -> io::Result<Vec<IpAddr>> {
        let only = |ips: &mut Vec<IpAddr>, v4: bool, force: bool| {
            let all = ips.clone();
            ips.retain(|ip| ip.is_ipv4() == v4);
            if ips.is_empty() {
                if force {
                    return Err(io::Error::new(io::ErrorKind::NotFound, "no address of the required family"));
                }
                *ips = all;
            }
            Ok(())
        };
        match self {
            Self::Any => {}
            Self::V4 { force } => only(&mut ips, true, force)?,
            Self::V6 { force } => only(&mut ips, false, force)?,
            Self::V4First => ips.sort_by_key(IpAddr::is_ipv6),
            Self::V6First => ips.sort_by_key(IpAddr::is_ipv4),
        }
        Ok(ips)
    }
}

pub async fn dial_tcp(host: &str, port: u16, opts: &DialOptions) -> io::Result<TcpStream> {
    dial_tcp_resolved(resolve_protected(host, opts.protect.as_ref()).await?, port, opts).await
}

pub async fn dial_tcp_resolved(ips: Vec<IpAddr>, port: u16, opts: &DialOptions) -> io::Result<TcpStream> {
    let ips = opts.family.pick(ips)?;
    if ips.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "host resolved to no addresses"));
    }
    let timeout = opts.timeout.unwrap_or(Duration::from_secs(16));
    let deadline = tokio::time::Instant::now() + timeout;
    tokio::time::timeout_at(deadline, async {
        if let Some(he) = opts.happy_eyeballs.filter(|h| h.try_delay != Duration::ZERO && h.max_concurrent_try != 0 && ips.len() > 1) {
            race_dial(&ips, port, opts, he).await
        } else {
            dial_serial(&ips, port, opts, deadline).await
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TCP dial timed out"))?
}

const MIN_ATTEMPT: Duration = Duration::from_secs(2);

fn attempt_budget(left: Duration, addrs: usize) -> Duration {
    let share = left / u32::try_from(addrs.max(1)).unwrap_or(u32::MAX);
    if share >= MIN_ATTEMPT { share } else { left.min(MIN_ATTEMPT) }
}

async fn dial_serial(ips: &[IpAddr], port: u16, opts: &DialOptions, deadline: tokio::time::Instant) -> io::Result<TcpStream> {
    let mut last = None;
    for (i, &ip) in ips.iter().enumerate() {
        let budget = attempt_budget(deadline.saturating_duration_since(tokio::time::Instant::now()), ips.len() - i);
        match tokio::time::timeout(budget, attempt(ip, port, opts)).await {
            Ok(Ok(s)) => return Ok(s),
            Ok(Err(e)) => last = Some(e),
            Err(_) => last = Some(io::Error::new(io::ErrorKind::TimedOut, "TCP dial timed out")),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no addresses")))
}

async fn attempt(ip: IpAddr, port: u16, opts: &DialOptions) -> io::Result<TcpStream> {
    let socket = Socket::new(if ip.is_ipv4() { Domain::IPV4 } else { Domain::IPV6 }, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_nonblocking(true)?;
    if let Some(protect) = &opts.protect {
        protect(raw_socket(&socket))?;
    }
    if opts.tcp_no_delay {
        socket.set_tcp_nodelay(true)?;
    }
    #[cfg(unix)]
    if let Some(mss) = opts.tcp_max_seg
        && let Err(e) = socket.set_tcp_mss(mss)
    {
        if !matches!(e.raw_os_error(), Some(22) | Some(92)) {
            return Err(e);
        }
        tracing::debug!(%e, "ignoring unsupported TCP_MAXSEG");
    }
    if let Some((idle, interval)) = opts.tcp_keepalive {
        let keepalive = socket2::TcpKeepalive::new().with_time(idle).with_interval(interval);
        if let Err(e) = socket.set_tcp_keepalive(&keepalive) {
            tracing::debug!(%e, "ignoring unsupported TCP keepalive");
        }
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(t) = opts.tcp_user_timeout
        && let Err(e) = socket.set_tcp_user_timeout(Some(t))
    {
        tracing::debug!(%e, "ignoring TCP_USER_TIMEOUT");
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(c) = opts.tcp_window_clamp {
        let v = libc::c_int::try_from(c).unwrap_or(libc::c_int::MAX);
        let r = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::IPPROTO_TCP,
                libc::TCP_WINDOW_CLAMP,
                (&v as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if r != 0 {
            tracing::debug!(e = %io::Error::last_os_error(), "ignoring TCP_WINDOW_CLAMP");
        }
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(c) = &opts.tcp_congestion {
        let r = unsafe {
            libc::setsockopt(socket.as_raw_fd(), libc::IPPROTO_TCP, libc::TCP_CONGESTION, c.as_ptr().cast(), c.len() as libc::socklen_t)
        };
        if r != 0 {
            tracing::debug!(e = %io::Error::last_os_error(), congestion = %c, "ignoring TCP_CONGESTION");
        }
    }
    let addr = SocketAddr::new(ip, port);
    match socket.connect(&addr.into()) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.raw_os_error() == Some(115) => {}
        Err(e) => return Err(e),
    }
    let stream: std::net::TcpStream = socket.into();
    let stream = TcpStream::from_std(stream)?;
    stream.writable().await?;
    if let Some(e) = stream.take_error()? {
        return Err(e);
    }
    Ok(stream)
}

async fn race_dial(ips: &[IpAddr], port: u16, opts: &DialOptions, he: HappyEyeballs) -> io::Result<TcpStream> {
    let ordered = order_ips(ips, he.prioritize_ipv6, he.interleave);
    let mut pending: Vec<AttemptFuture<'_>> = Vec::new();
    let mut next = 0usize;
    let mut last = None;
    let mut delay = Box::pin(tokio::time::sleep(Duration::ZERO));
    loop {
        if next < ordered.len() && pending.len() < he.max_concurrent_try as usize {
            let i = next;
            next += 1;
            let ip = ordered[i];
            pending.push(Box::pin(async move { (i, attempt(ip, port, opts).await) }));
            delay.as_mut().reset(tokio::time::Instant::now() + he.try_delay);
        }
        if pending.is_empty() {
            return Err(last.unwrap_or_else(|| io::Error::other("all connection attempts failed")));
        }
        let (idx, (_completed_index, result)) = poll_fn(|cx| {
            for (i, f) in pending.iter_mut().enumerate() {
                if let Poll::Ready(v) = f.as_mut().poll(cx) {
                    return Poll::Ready((i, v));
                }
            }
            if next < ordered.len() && pending.len() < he.max_concurrent_try as usize && delay.as_mut().poll(cx).is_ready() {
                return Poll::Ready((usize::MAX, (0, Err(io::Error::new(io::ErrorKind::WouldBlock, "launch")))));
            }
            Poll::Pending
        })
        .await;
        if idx == usize::MAX {
            continue;
        }
        drop(pending.swap_remove(idx));
        match result {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
}

pub(crate) fn order_ips(ips: &[IpAddr], prioritize_ipv6: bool, interleave: u32) -> Vec<IpAddr> {
    let (v4, v6): (Vec<_>, Vec<_>) = ips.iter().copied().partition(IpAddr::is_ipv4);
    if v4.is_empty() || v6.is_empty() {
        return ips.to_vec();
    }
    let (a, b) = if prioritize_ipv6 { (v6, v4) } else { (v4, v6) };
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(ips.len());
    let mut first = true;
    let step = (interleave.max(1) as usize).min(ips.len());
    while i < a.len() && j < b.len() {
        let take_a = first;
        for _ in 0..step {
            if take_a {
                if i < a.len() {
                    out.push(a[i]);
                    i += 1;
                }
            } else if j < b.len() {
                out.push(b[j]);
                j += 1;
            }
        }
        first = !first;
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

pub fn udp_socket(ipv6: bool, protect: Option<&Protect>) -> io::Result<tokio::net::UdpSocket> {
    let domain = if ipv6 { Domain::IPV6 } else { Domain::IPV4 };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    if let Some(protect) = protect {
        protect(raw_socket(&socket))?;
    }
    let bind: SocketAddr = if ipv6 { "[::]:0".parse().unwrap() } else { "0.0.0.0:0".parse().unwrap() };
    socket.bind(&bind.into())?;
    tokio::net::UdpSocket::from_std(socket.into())
}

#[cfg(unix)]
pub fn icmp_allowed() -> bool {
    static OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OK.get_or_init(|| Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).is_ok())
}

#[cfg(unix)]
pub async fn icmp_ping(ip: IpAddr, protect: Option<&Protect>, timeout: Duration) -> io::Result<Duration> {
    let (domain, proto, request, reply) = match ip {
        IpAddr::V4(_) => (Domain::IPV4, Protocol::ICMPV4, 8u8, 0u8),
        IpAddr::V6(_) => (Domain::IPV6, Protocol::ICMPV6, 128u8, 129u8),
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(proto))?;
    socket.set_nonblocking(true)?;
    if let Some(protect) = protect {
        protect(raw_socket(&socket))?;
    }
    socket.connect(&SocketAddr::new(ip, 0).into())?;
    let sock = tokio::net::UdpSocket::from_std(std::net::UdpSocket::from(std::os::fd::OwnedFd::from(socket)))?;
    let seq: u16 = 1;
    let mut pkt = vec![request, 0, 0, 0, 0, 0];
    pkt.extend_from_slice(&seq.to_be_bytes());
    pkt.extend_from_slice(b"rynet-ping-rynet-ping-rynet-ping");
    let start = std::time::Instant::now();
    sock.send(&pkt).await?;
    let mut buf = [0u8; 1500];
    tokio::time::timeout(timeout, async {
        loop {
            let n = sock.recv(&mut buf).await?;
            if n >= 8 && buf[0] == reply && buf[6..8] == seq.to_be_bytes() {
                return Ok(start.elapsed());
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "icmp: no reply"))?
}

#[cfg(windows)]
pub fn icmp_allowed() -> bool {
    false
}

#[cfg(windows)]
pub async fn icmp_ping(_ip: IpAddr, _protect: Option<&Protect>, _timeout: Duration) -> io::Result<Duration> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "ICMP ping is not available on Windows yet"))
}

#[cfg(kani)]
#[path = "proofs/lib.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_interleave_orders_like_a_whole_family() {
        let ips: Vec<IpAddr> = vec![[1, 1, 1, 1].into(), [2, 2, 2, 2].into(), "::1".parse().unwrap(), "::2".parse().unwrap()];
        let t0 = std::time::Instant::now();
        assert_eq!(order_ips(&ips, false, u32::MAX), order_ips(&ips, false, 4));
        assert!(t0.elapsed() < Duration::from_millis(200), "{:?}", t0.elapsed());
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[tokio::test(flavor = "current_thread")]
    async fn window_clamp_past_i32_does_not_shrink_the_window() {
        use std::os::fd::AsRawFd;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let mut clamps = Vec::new();
        for clamp in [None, Some(3_000_000_000)] {
            let opts = DialOptions { tcp_window_clamp: clamp, ..Default::default() };
            let s = dial_tcp_resolved(vec![IpAddr::from([127, 0, 0, 1])], port, &opts).await.unwrap();
            let (mut v, mut len): (libc::c_int, libc::socklen_t) = (0, std::mem::size_of::<libc::c_int>() as libc::socklen_t);
            let r = unsafe {
                libc::getsockopt(s.as_raw_fd(), libc::IPPROTO_TCP, libc::TCP_WINDOW_CLAMP, (&mut v as *mut libc::c_int).cast(), &mut len)
            };
            assert_eq!(r, 0);
            clamps.push(v);
        }
        assert_eq!(clamps[1], clamps[0]);
    }
    #[test]
    fn router_fake_ip_is_usable() {
        assert!(in_fake_range(&"198.18.52.204".parse().unwrap()));
        assert!(!is_fake(&"198.18.52.204".parse().unwrap()));
    }

    #[test]
    fn dns_answer_parses() {
        let q = dns_query("a.example", 0x1234).unwrap();
        let mut r = q.clone();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 3;
        r.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 3, 1, b'b', 0]);
        r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 1, 2, 3, 4]);
        r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 5, 6, 7, 8]);
        let ips = dns_answer(&r, 0x1234).unwrap();
        assert_eq!(ips, vec![IpAddr::from([1, 2, 3, 4]), IpAddr::from([5, 6, 7, 8])]);
        assert!(dns_answer(&r, 0x9999).is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn icmp_ping_localhost() {
        match icmp_ping("127.0.0.1".parse().unwrap(), None, Duration::from_secs(2)).await {
            Ok(d) => assert!(d < Duration::from_secs(1)),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => eprintln!("icmp sockets not allowed here"),
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn xray_interleave_order() {
        let ips: Vec<IpAddr> = ["4.1.1.1", "2001:db8::1", "4.2.2.2", "2001:db8::2", "4.3.3.3"].iter().map(|s| s.parse().unwrap()).collect();
        assert_eq!(order_ips(&ips, false, 1), vec![ips[0], ips[1], ips[2], ips[3], ips[4]]);
        assert_eq!(order_ips(&ips, true, 2), vec![ips[1], ips[3], ips[0], ips[2], ips[4]]);
    }

    #[test]
    fn system_answers_expire() {
        let mut a = Answers::default();
        let t = std::time::Instant::now();
        let ip = IpAddr::from([192, 0, 2, 7]);
        a.put("srv.test", &[ip], t);
        assert_eq!(a.get("srv.test", t + Duration::from_secs(299)), Some(vec![ip]));
        assert_eq!(a.get("srv.test", t + SYSTEM_TTL), None);
        assert_eq!(a.get("other.test", t), None);
        for i in 0..64 {
            a.put(&format!("h{i}.test"), &[ip], t);
        }
        a.put("late.test", &[ip], t + SYSTEM_TTL);
        assert_eq!(a.0.len(), 1, "expired answers are pruned once the table fills up");
    }

    #[test]
    fn doh_failure_kinds() {
        use io::ErrorKind::*;
        for k in [TimedOut, ConnectionRefused, ConnectionReset, PermissionDenied, Other, UnexpectedEof] {
            assert!(bootstrap::unreachable(&io::Error::from(k)), "{k:?}");
        }
        for k in [NotFound, InvalidData, InvalidInput] {
            assert!(!bootstrap::unreachable(&io::Error::from(k)), "{k:?}");
        }
    }

    #[tokio::test]
    async fn auto_dns_rests_after_doh_failure_and_keeps_system_answers() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        bootstrap::configure("auto").unwrap();
        forget_lookups();
        let tries = Arc::new(AtomicUsize::new(0));
        let t = tries.clone();
        let protect: Protect = Arc::new(move |_| {
            t.fetch_add(1, SeqCst);
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "DoH blocked in test"))
        });
        let first = resolve_protected("localhost", Some(&protect)).await.expect("system answer");
        assert!(first.iter().all(IpAddr::is_loopback), "{first:?}");
        let doh_tries = tries.load(SeqCst);
        assert!(doh_tries > 0, "DoH is tried first");
        assert!(bootstrap::resting(std::time::Instant::now()));
        assert_eq!(resolve_protected("localhost", Some(&protect)).await.unwrap(), first, "system answer is kept");
        *system_answers() = None;
        assert_eq!(resolve_protected("localhost", Some(&protect)).await.unwrap(), first);
        assert_eq!(tries.load(SeqCst), doh_tries, "no DoH while it rests");
        forget_lookups();
        assert!(!bootstrap::resting(std::time::Instant::now()));
        bootstrap::configure("system").unwrap();
    }

    #[test]
    fn serial_dial_splits_the_timeout() {
        let s = Duration::from_secs;
        assert_eq!(attempt_budget(s(16), 2), s(8));
        assert_eq!(attempt_budget(s(16), 1), s(16));
        assert_eq!(attempt_budget(s(3), 4), s(2), "short share is raised to the minimum");
        assert_eq!(attempt_budget(Duration::from_millis(1500), 3), Duration::from_millis(1500), "less than the minimum left: all of it");
        assert_eq!(attempt_budget(Duration::ZERO, 2), Duration::ZERO);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn silent_and_live(port_tries: usize) -> (Socket, std::net::TcpListener, u16) {
        use std::net::Ipv4Addr;
        for _ in 0..port_tries {
            let live = std::net::TcpListener::bind((Ipv4Addr::new(127, 0, 0, 2), 0)).unwrap();
            let port = live.local_addr().unwrap().port();
            let silent = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
            if silent.bind(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)).into()).is_ok() {
                silent.listen(0).unwrap();
                return (silent, live, port);
            }
        }
        panic!("no free port pair on 127.0.0.1/127.0.0.2");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[tokio::test]
    async fn serial_dial_moves_past_a_silent_address() {
        let (_silent, _live, port) = silent_and_live(10);
        let mut held = Vec::new();
        let mut drops_syn = false;
        for _ in 0..4 {
            match tokio::time::timeout(Duration::from_millis(300), TcpStream::connect(("127.0.0.1", port))).await {
                Ok(Ok(s)) => held.push(s),
                Ok(Err(_)) => break,
                Err(_) => {
                    drops_syn = true;
                    break;
                }
            }
        }
        let opts = DialOptions { timeout: Some(Duration::from_secs(4)), ..Default::default() };
        let t0 = std::time::Instant::now();
        let s = dial_tcp_resolved(vec![IpAddr::from([127, 0, 0, 1]), IpAddr::from([127, 0, 0, 2])], port, &opts)
            .await
            .expect("the second address gets its turn");
        assert_eq!(s.peer_addr().unwrap(), SocketAddr::from(([127, 0, 0, 2], port)));
        if drops_syn {
            let took = t0.elapsed();
            assert!(took >= Duration::from_millis(1500) && took < Duration::from_secs(3), "first address got {took:?} of 4 s");
        }
    }
}
