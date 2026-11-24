use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use rv_net::Protect;
use rv_xhttp::activity::{FrameFlush, Records};

use crate::BoxStream;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Upstream {
    Udp(SocketAddr),
    Tcp { host: String, port: u16 },
    Tls { host: String, port: u16 },
    Https { host: String, port: u16, path: String },
    Quic { host: String, port: u16 },
    System,
    Fake,
}

impl Upstream {
    pub fn endpoint(&self) -> (String, u16) {
        match self {
            Upstream::Udp(a) => (a.ip().to_string(), a.port()),
            Upstream::Tcp { host, port }
            | Upstream::Tls { host, port }
            | Upstream::Https { host, port, .. }
            | Upstream::Quic { host, port } => (host.clone(), *port),
            Upstream::System | Upstream::Fake => (String::new(), 53),
        }
    }
}

impl std::fmt::Display for Upstream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Upstream::Udp(a) => write!(f, "{a}"),
            Upstream::Tcp { host, port } => write!(f, "tcp://{host}:{port}"),
            Upstream::Tls { host, port } => write!(f, "tls://{host}:{port}"),
            Upstream::Https { host, port, path } => write!(f, "https://{host}:{port}{path}"),
            Upstream::Quic { host, port } => write!(f, "quic://{host}:{port}"),
            Upstream::System => write!(f, "localhost"),
            Upstream::Fake => write!(f, "fakedns"),
        }
    }
}

pub fn parse(addr: &str, port: Option<u16>) -> Option<(Upstream, bool)> {
    let addr = addr.trim();
    if addr.eq_ignore_ascii_case("localhost") {
        return Some((Upstream::System, true));
    }
    if addr.eq_ignore_ascii_case("fakedns") {
        return Some((Upstream::Fake, true));
    }
    let (scheme, rest) = match addr.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => (String::new(), addr),
    };
    let (scheme, local) = match scheme.strip_suffix("+local") {
        Some(s) => (s.to_string(), true),
        None => (scheme, false),
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let default_port = match scheme.as_str() {
        "https" => 443,
        "tls" | "quic" => 853,
        _ => 53,
    };
    let (host, port) = split_host_port(authority, port.unwrap_or(default_port))?;
    let up = match scheme.as_str() {
        "" | "udp" => Upstream::Udp(SocketAddr::new(host.parse::<IpAddr>().ok()?, port)),
        "tcp" => Upstream::Tcp { host, port },
        "tls" => Upstream::Tls { host, port },
        "quic" if local => Upstream::Quic { host, port },
        "quic" => return None,
        "https" => Upstream::Https { host, port, path: if path.is_empty() { "/dns-query".into() } else { path.into() } },
        _ => return None,
    };
    Some((up, local))
}

fn split_host_port(s: &str, default: u16) -> Option<(String, u16)> {
    if s.is_empty() {
        return None;
    }
    if let Some(r) = s.strip_prefix('[') {
        let (h, tail) = r.split_once(']')?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if tail.is_empty() => default,
            None => return None,
        };
        return Some((h.to_string(), port));
    }
    if s.parse::<IpAddr>().is_ok() {
        return Some((s.to_string(), default));
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => Some((h.to_string(), p.parse().ok()?)),
        Some(_) => None,
        None => Some((s.to_string(), default)),
    }
}

const INFLIGHT: usize = 64;

type Query = Rc<dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = io::Result<Vec<u8>>>>>>;

fn framed(query: Query, driver: Option<Pin<Box<dyn Future<Output = ()>>>>) -> BoxStream {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    tokio::task::spawn_local(async move {
        let (tx, mut rx) = mpsc::channel::<io::Result<Vec<u8>>>(INFLIGHT);
        let serve = async {
            let (mut rd, mut wr) = tokio::io::split(ours);
            let reader = async {
                let mut tasks = tokio::task::JoinSet::new();
                let mut len = [0u8; 2];
                loop {
                    if rd.read_exact(&mut len).await.is_err() {
                        return;
                    }
                    let mut msg = vec![0u8; u16::from_be_bytes(len) as usize];
                    if rd.read_exact(&mut msg).await.is_err() {
                        return;
                    }
                    while tasks.try_join_next().is_some() {}
                    if tasks.len() >= INFLIGHT {
                        continue;
                    }
                    let (fut, tx) = (query(msg), tx.clone());
                    tasks.spawn_local(async move {
                        let _ = tx.try_send(fut.await);
                    });
                }
            };
            let writer = async {
                while let Some(r) = rx.recv().await {
                    match r {
                        Ok(reply) if reply.len() >= 12 && reply.len() <= u16::MAX as usize => {
                            let mut frame = Vec::with_capacity(reply.len() + 2);
                            frame.extend_from_slice(&(reply.len() as u16).to_be_bytes());
                            frame.extend_from_slice(&reply);
                            if wr.write_all(&frame).await.is_err() {
                                return;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::debug!(target: "rv::dns", "{e}");
                            return;
                        }
                    }
                }
            };
            tokio::select! {
                _ = reader => {}
                _ = writer => {}
            }
        };
        match driver {
            Some(d) => tokio::select! {
                _ = serve => {}
                _ = d => {}
            },
            None => serve.await,
        }
    });
    Box::new(theirs)
}

pub async fn doh(io: BoxStream, host: &str, port: u16, path: &str) -> io::Result<BoxStream> {
    let alpn = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let tls = rv_tls::connect(io, tls_config(host, alpn)).await.map_err(io::Error::other)?;
    if tls.alpn().is_some_and(|a| a != b"h2") {
        return Err(io::Error::other("doh: server did not select h2"));
    }
    let tls = FrameFlush::new(tls, Records::GoHttp2);
    let (send, conn) = h2::client::Builder::new()
        .enable_push(false)
        .initial_window_size(4 << 20)
        .initial_connection_window_size((1 << 30) + 65535)
        .max_frame_size(1 << 20)
        .max_header_list_size(10 << 20)
        .window_refresh(h2::WindowRefresh::GoMin)
        .handshake::<_, Bytes>(tls)
        .await
        .map_err(io::Error::other)?;
    let authority = match port {
        443 => authority(host),
        p => format!("{}:{p}", authority(host)),
    };
    let uri: http::Uri = format!("https://{authority}{path}").parse().map_err(|_| io::Error::other("doh: bad url"))?;
    let q: Query = Rc::new(move |msg| Box::pin(query(send.clone(), uri.clone(), msg)));
    let driver = Box::pin(async move {
        if let Err(e) = conn.await {
            tracing::debug!(target: "rv::dns", "doh connection: {e}");
        }
    });
    Ok(framed(q, Some(driver)))
}

pub async fn dot(io: BoxStream, host: &str) -> io::Result<BoxStream> {
    let tls = rv_tls::connect(io, tls_config(host, vec![b"dot".to_vec()])).await.map_err(io::Error::other)?;
    Ok(Box::new(tls))
}

fn tls_config(host: &str, alpn: Vec<Vec<u8>>) -> Arc<rv_tls::ClientConfig> {
    Arc::new(rv_tls::ClientConfig {
        server_name: host.to_string(),
        alpn,
        alpn_override: true,
        fingerprint: rv_tls::Fingerprint::Chrome,
        verify: rv_tls::Verify::WebPki,
    })
}

pub(crate) const DOQ_ALPN: &[&[u8]] = &[b"http/1.1", b"h2", b"doq"];

pub async fn doq(host: &str, port: u16, protect: Option<&Protect>, ipv4_only: bool) -> io::Result<BoxStream> {
    let ip = rv_net::resolve_protected(host, protect)
        .await?
        .into_iter()
        .filter(|ip| !ipv4_only || ip.is_ipv4())
        .min_by_key(|i| !i.is_ipv4())
        .ok_or_else(|| io::Error::other("doq: no address"))?;
    let server = SocketAddr::new(ip, port);
    let udp = rv_net::udp_socket(server.is_ipv6(), protect)?;
    let runtime: Arc<dyn quinn::Runtime> = Arc::new(quinn::TokioRuntime);
    let socket: Arc<dyn quinn::AsyncUdpSocket> = Arc::new(crate::quic::GsoGuard(runtime.wrap_udp_socket(udp.into_std()?)?));
    let endpoint = crate::quic::endpoint(socket, runtime, 4)?;
    let alpn = DOQ_ALPN.iter().map(|p| p.to_vec()).collect::<Vec<_>>();
    let crypto = crate::quic::crypto(host, false, &rv_tls::Pins::default(), &alpn).map_err(io::Error::other)?;
    let mut t = quinn::TransportConfig::default();
    t.max_idle_timeout(Some(Duration::from_secs(30).try_into().map_err(io::Error::other)?));
    t.keep_alive_interval(None);
    t.initial_mtu(1280);
    t.max_concurrent_bidi_streams(0u8.into());
    t.max_concurrent_uni_streams(0u8.into());
    t.datagram_receive_buffer_size(None);
    t.allow_spin(false);
    let mut cfg = quinn::ClientConfig::new(Arc::new(crypto));
    cfg.transport_config(Arc::new(t));
    cfg.initial_dst_cid_provider(Arc::new(crate::quic::initial_destination_cid));
    let conn = endpoint.connect_with(cfg, server, host).map_err(io::Error::other)?.await.map_err(io::Error::other)?;
    let q: Query = Rc::new(move |msg| {
        let conn = conn.clone();
        let _keep = endpoint.clone();
        Box::pin(async move {
            let _keep = _keep;
            if msg.len() < 12 {
                return Ok(Vec::new());
            }
            let id = [msg[0], msg[1]];
            let (mut send, mut recv) = conn.open_bi().await.map_err(io::Error::other)?;
            let mut frame = Vec::with_capacity(msg.len() + 2);
            frame.extend_from_slice(&(msg.len() as u16).to_be_bytes());
            frame.extend_from_slice(&[0, 0]);
            frame.extend_from_slice(&msg[2..]);
            send.write_all(&frame).await.map_err(io::Error::other)?;
            let _ = send.finish();
            let data = recv.read_to_end(u16::MAX as usize + 2).await.map_err(io::Error::other)?;
            let Some(len) = data.get(..2).map(|l| u16::from_be_bytes([l[0], l[1]]) as usize) else {
                return Ok(Vec::new());
            };
            let mut reply = data.get(2..2 + len).map(<[u8]>::to_vec).unwrap_or_default();
            if reply.len() >= 2 {
                reply[..2].copy_from_slice(&id);
            }
            Ok(reply)
        })
    });
    Ok(framed(q, None))
}

fn authority(host: &str) -> String {
    if host.contains(':') { format!("[{host}]") } else { host.to_string() }
}

async fn query(send: h2::client::SendRequest<Bytes>, uri: http::Uri, msg: Vec<u8>) -> io::Result<Vec<u8>> {
    if msg.len() < 12 {
        return Ok(Vec::new());
    }
    let id = [msg[0], msg[1]];
    let mut msg = pad_query(&msg, 100 + rand_below(200));
    msg[..2].copy_from_slice(&[0, 0]);
    let mut headers: rv_xhttp::browser::Headers =
        vec![("Accept".into(), "application/dns-message".into()), ("Content-Type".into(), "application/dns-message".into())];
    rv_xhttp::browser::apply_defaults(&mut headers, rv_xhttp::browser::Variant::Fetch);
    rv_xhttp::browser::set(&mut headers, "X-Padding", &h2_base62_pad(100 + rand_below(900)));
    let mut req = http::Request::post(uri);
    for i in rv_xhttp::transport::shuffled(headers.len()) {
        req = req.header(headers[i].0.as_str(), headers[i].1.as_str());
    }
    let req = req.header("content-length", msg.len()).header("accept-encoding", "gzip").body(()).map_err(io::Error::other)?;
    let mut send = send.ready().await.map_err(io::Error::other)?;
    let (resp, mut body) = send.send_request(req, false).map_err(io::Error::other)?;
    body.send_data(Bytes::from(msg), true).map_err(io::Error::other)?;
    let resp = resp.await.map_err(io::Error::other)?;
    if resp.status() != http::StatusCode::OK {
        tracing::debug!(target: "rv::dns", "doh: http {}", resp.status());
        return Ok(Vec::new());
    }
    let gzip = resp.headers().get(http::header::CONTENT_ENCODING).is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"gzip"));
    let mut body = resp.into_body();
    let mut out = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.map_err(io::Error::other)?;
        let _ = body.flow_control().release_capacity(chunk.len());
        out.extend_from_slice(&chunk);
        if out.len() > u16::MAX as usize {
            return Ok(Vec::new());
        }
    }
    if gzip {
        out = gunzip(&out).unwrap_or_default();
    }
    if out.len() >= 2 {
        out[..2].copy_from_slice(&id);
    }
    Ok(out)
}

fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data).take(u16::MAX as u64 + 1).read_to_end(&mut out).ok()?;
    (out.len() <= u16::MAX as usize).then_some(out)
}

fn rand_below(n: usize) -> usize {
    use ring::rand::SecureRandom;
    let mut b = [0u8; 4];
    let _ = ring::rand::SystemRandom::new().fill(&mut b);
    u32::from_le_bytes(b) as usize % n
}

fn h2_base62_pad(n: usize) -> String {
    let len = (n as f64 * 1.2493702770780857) as usize;
    rv_xhttp::request::random_text(len, b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz")
}

fn pad_query(msg: &[u8], n: usize) -> Vec<u8> {
    let count = |i: usize| u16::from_be_bytes([msg[i], msg[i + 1]]);
    let unchanged = msg.to_vec();
    let Some(q) = question(msg) else { return unchanged };
    if count(4) != 1 || count(6) != 0 || count(8) != 0 || count(10) > 1 {
        return unchanged;
    }
    let mut option = Vec::with_capacity(4 + n);
    option.extend_from_slice(&12u16.to_be_bytes());
    option.extend_from_slice(&(n as u16).to_be_bytes());
    option.resize(4 + n, 0);
    if count(10) == 0 {
        if q.end != msg.len() {
            return unchanged;
        }
        let mut out = unchanged;
        out[11] = 1;
        out.extend_from_slice(&[0, 0, 41, 0x05, 0x46, 0xE0, 0, 0x80, 0]);
        out.extend_from_slice(&(option.len() as u16).to_be_bytes());
        out.extend_from_slice(&option);
        return out;
    }
    let i = q.end;
    let Some(h) = msg.get(i..i + 11) else { return unchanged };
    let rdlen = u16::from_be_bytes([h[9], h[10]]) as usize;
    if h[0] != 0 || u16::from_be_bytes([h[1], h[2]]) != 41 || i + 11 + rdlen != msg.len() {
        return unchanged;
    }
    let mut o = i + 11;
    while o + 4 <= msg.len() {
        if u16::from_be_bytes([msg[o], msg[o + 1]]) == 12 {
            return unchanged;
        }
        o += 4 + u16::from_be_bytes([msg[o + 2], msg[o + 3]]) as usize;
    }
    let Ok(rdlen) = u16::try_from(rdlen + option.len()) else { return unchanged };
    let mut out = unchanged;
    out[i + 9..i + 11].copy_from_slice(&rdlen.to_be_bytes());
    out.extend_from_slice(&option);
    out
}

pub mod fake {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Mutex;

    const BASE: u32 = u32::from_be_bytes([198, 19, 0, 0]);
    const SIZE: u32 = 1 << 16;
    pub const TTL: u32 = 10;
    const LIVE_MAX: usize = 16384;
    const BYTES_MAX: usize = 1 << 20;

    struct Pool {
        next: u32,
        tail: u32,
        bytes: usize,
        by_name: HashMap<String, u32>,
        by_ip: HashMap<u32, String>,
        file: Option<std::fs::File>,
        path: Option<std::path::PathBuf>,
        lines: usize,
    }

    impl Pool {
        fn new() -> Self {
            Pool { next: 1, tail: 1, bytes: 0, by_name: HashMap::new(), by_ip: HashMap::new(), file: None, path: None, lines: 0 }
        }

        fn step(off: u32) -> u32 {
            if off + 1 >= SIZE - 1 { 1 } else { off + 1 }
        }

        fn trim(&mut self) {
            while self.by_ip.len() > LIVE_MAX || self.bytes > BYTES_MAX {
                if let Some(old) = self.by_ip.remove(&self.tail) {
                    self.bytes -= old.len();
                    self.by_name.remove(&old);
                }
                self.tail = Self::step(self.tail);
            }
        }

        fn assign(&mut self, name: &str) -> (u32, bool) {
            if let Some(&off) = self.by_name.get(name) {
                return (off, false);
            }
            let off = self.next;
            self.next = Self::step(off);
            self.put(off, name);
            self.trim();
            (off, true)
        }

        fn journal_limit(&self) -> usize {
            JOURNAL_MAX.max(2 * self.by_ip.len())
        }

        fn compact(&mut self) {
            use std::io::Write;
            let Some(path) = self.path.clone() else { return };
            let mut all: Vec<_> = self.by_ip.iter().collect();
            let next = self.next;
            all.sort_by_key(|(off, _)| (**off + SIZE - next) % SIZE);
            let mut buf = String::new();
            for (off, name) in all {
                buf.push_str(&format!("{off} {name}\n"));
            }
            let tmp = path.with_extension("tmp");
            let written = std::fs::File::create(&tmp)
                .and_then(|mut f| f.write_all(buf.as_bytes()).and_then(|_| f.sync_data()))
                .and_then(|_| std::fs::rename(&tmp, &path));
            match written {
                Ok(()) => self.lines = self.by_ip.len(),
                Err(e) => {
                    tracing::warn!("fakedns journal: {e}");
                    let _ = std::fs::remove_file(&tmp);
                }
            }
            self.file = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok();
        }

        fn put(&mut self, off: u32, name: &str) {
            if let Some(old) = self.by_ip.insert(off, name.to_string()) {
                self.bytes -= old.len();
                self.by_name.remove(&old);
            }
            if let Some(prev) = self.by_name.insert(name.to_string(), off)
                && prev != off
                && self.by_ip.remove(&prev).is_some()
            {
                self.bytes -= name.len();
            }
            self.bytes += name.len();
        }
    }

    static POOL: Mutex<Option<Pool>> = Mutex::new(None);

    const JOURNAL_MAX: usize = 4096;

    pub fn persist(path: &std::path::Path) {
        use std::io::BufRead;
        let mut g = POOL.lock().unwrap_or_else(|e| e.into_inner());
        let p = g.get_or_insert_with(Pool::new);
        if p.by_ip.is_empty()
            && let Ok(f) = std::fs::File::open(path)
        {
            let mut lines = 0;
            for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
                lines += 1;
                let Some((off, name)) = line.split_once(' ') else { continue };
                let Ok(off) = off.parse::<u32>() else { continue };
                if off == 0 || off >= SIZE - 1 || name.is_empty() {
                    continue;
                }
                p.put(off, name);
                p.next = Pool::step(off);
            }
            p.tail = p.next;
            p.trim();
            p.lines = lines;
        }
        p.path = Some(path.to_path_buf());
        if p.lines > p.journal_limit() {
            p.compact();
        } else {
            p.file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
        }
    }

    pub fn valid(name: &str) -> bool {
        name.len() <= 253
            && name
                .split('.')
                .all(|l| (1..=63).contains(&l.len()) && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
    }

    pub fn overlaps(ip: IpAddr, len: u8) -> bool {
        let IpAddr::V4(a) = ip else { return false };
        let m = u32::MAX.checked_shl(32 - u32::from(len.min(16))).unwrap_or(0);
        u32::from(a) & m == BASE & m
    }

    pub fn is_fake(ip: IpAddr) -> bool {
        matches!(ip, IpAddr::V4(a) if u32::from(a) >> 16 == BASE >> 16)
    }

    pub fn ip_for(name: &str) -> Ipv4Addr {
        let mut g = POOL.lock().unwrap_or_else(|e| e.into_inner());
        let p = g.get_or_insert_with(Pool::new);
        let (off, new) = p.assign(name);
        if !new {
            return Ipv4Addr::from(BASE + off);
        }
        if let Some(f) = p.file.as_mut()
            && valid(name)
        {
            use std::io::Write;
            let _ = writeln!(f, "{off} {name}");
            p.lines += 1;
            if p.lines > 4 * p.journal_limit() {
                p.compact();
            }
        }
        Ipv4Addr::from(BASE + off)
    }

    pub fn name_for(ip: IpAddr) -> Option<String> {
        let IpAddr::V4(a) = ip else { return None };
        let off = u32::from(a).checked_sub(BASE)?;
        POOL.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.by_ip.get(&off).cloned()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fake_pool_forgets_the_oldest_names() {
            let mut p = Pool::new();
            let first = p.assign("first.test").0;
            for i in 0..LIVE_MAX {
                p.assign(&format!("n{i}.test"));
            }
            assert_eq!(p.by_ip.len(), LIVE_MAX);
            assert_eq!(p.by_name.len(), LIVE_MAX);
            assert!(!p.by_name.contains_key("first.test") && !p.by_ip.contains_key(&first));
            assert!(p.by_name.contains_key("n0.test"));
            assert_eq!(p.bytes, p.by_ip.values().map(String::len).sum::<usize>());
            let long = |i: usize| format!("{i:0>60}.{}.test", "a".repeat(63));
            for i in 0..2 * BYTES_MAX / 130 {
                p.assign(&long(i));
            }
            assert!(p.bytes <= BYTES_MAX && p.by_ip.len() < LIVE_MAX);
            assert_eq!(p.bytes, p.by_ip.values().map(String::len).sum::<usize>());
            assert_eq!(p.by_ip.len(), p.by_name.len());
            let last = long(2 * BYTES_MAX / 130 - 1);
            let off = p.by_name[&last];
            assert_eq!(p.assign(&last), (off, false));
            for i in 0..150_000 {
                p.assign(&format!("lap{i}.test"));
            }
            assert_eq!(p.by_ip.len(), LIVE_MAX);
            assert_eq!(p.by_ip.len(), p.by_name.len());
            assert!(p.by_ip.iter().all(|(off, name)| p.by_name[name] == *off));
        }
    }
}

pub fn addresses(reply: &[u8]) -> Vec<IpAddr> {
    let mut out = Vec::new();
    let Some(q) = question(reply) else { return out };
    let an = u16::from_be_bytes([reply[6], reply[7]]) as usize;
    let mut i = q.end;
    for _ in 0..an {
        loop {
            let Some(&len) = reply.get(i) else { return out };
            if len == 0 {
                i += 1;
                break;
            }
            if len & 0xc0 == 0xc0 {
                i += 2;
                break;
            }
            i += 1 + len as usize;
        }
        let Some(h) = reply.get(i..i + 10) else { return out };
        let t = u16::from_be_bytes([h[0], h[1]]);
        let len = u16::from_be_bytes([h[8], h[9]]) as usize;
        let Some(data) = reply.get(i + 10..i + 10 + len) else { return out };
        match (t, len) {
            (TYPE_A, 4) => out.push(IpAddr::from(<[u8; 4]>::try_from(data).unwrap())),
            (TYPE_AAAA, 16) => out.push(IpAddr::from(<[u8; 16]>::try_from(data).unwrap())),
            _ => {}
        }
        i += 10 + len;
    }
    out
}

pub const TYPE_A: u16 = 1;
pub const TYPE_AAAA: u16 = 28;

pub fn answer_ttl(reply: &[u8]) -> u32 {
    let Some(q) = question(reply) else { return 0 };
    let mut i = q.end;
    let mut ttl = u32::MAX;
    for _ in 0..u16::from_be_bytes([reply[6], reply[7]]) {
        loop {
            let Some(&n) = reply.get(i) else { return 0 };
            i += 1;
            if n == 0 {
                break;
            }
            if n & 0xc0 == 0xc0 {
                if reply.get(i).is_none() {
                    return 0;
                }
                i += 1;
                break;
            }
            if n > 63 {
                return 0;
            }
            i += n as usize;
        }
        let Some(h) = reply.get(i..i + 10) else { return 0 };
        ttl = ttl.min(u32::from_be_bytes(h[4..8].try_into().unwrap()));
        i += 10 + u16::from_be_bytes([h[8], h[9]]) as usize;
        if i > reply.len() {
            return 0;
        }
    }
    if ttl == u32::MAX { 0 } else { ttl }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Question {
    pub name: String,
    pub qtype: u16,
    end: usize,
}

pub fn question(msg: &[u8]) -> Option<Question> {
    if msg.len() < 12 || u16::from_be_bytes([msg[4], msg[5]]) == 0 {
        return None;
    }
    let mut i = 12;
    let mut name = String::new();
    loop {
        let len = *msg.get(i)? as usize;
        i += 1;
        if len == 0 {
            break;
        }
        if len & 0xc0 != 0 || name.len() + len > 253 {
            return None;
        }
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(&String::from_utf8_lossy(msg.get(i..i + len)?).to_ascii_lowercase());
        i += len;
    }
    let qtype = u16::from_be_bytes([*msg.get(i)?, *msg.get(i + 1)?]);
    msg.get(i + 3)?;
    Some(Question { name, qtype, end: i + 4 })
}

pub fn answer(query: &[u8], q: &Question, ips: &[IpAddr], ttl: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(q.end + ips.len() * 28);
    out.extend_from_slice(&query[..2]);
    out.push(0x80 | (query[2] & 0x01));
    out.push(0x80);
    out.extend_from_slice(&[0, 1]);
    out.extend_from_slice(&(ips.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&query[12..q.end]);
    for ip in ips {
        out.extend_from_slice(&[0xc0, 0x0c]);
        let (t, data) = match ip {
            IpAddr::V4(a) => (TYPE_A, a.octets().to_vec()),
            IpAddr::V6(a) => (TYPE_AAAA, a.octets().to_vec()),
        };
        out.extend_from_slice(&t.to_be_bytes());
        out.extend_from_slice(&[0, 1]);
        out.extend_from_slice(&ttl.to_be_bytes());
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(&data);
    }
    out
}

pub fn rcode(query: &[u8], q: &Question, code: u8) -> Vec<u8> {
    let mut r = answer(query, q, &[], 60);
    r[3] = 0x80 | (code & 0x0f);
    r
}

const LOCAL_ZONES: &[&str] = &[
    "lan",
    "local",
    "localdomain",
    "home",
    "home.arpa",
    "internal",
    "intranet",
    "corp",
    "private",
    "my.keenetic.net",
    "tplinkwifi.net",
    "tplinklogin.net",
    "routerlogin.net",
    "router.asus.com",
    "fritz.box",
    "10.in-addr.arpa",
    "168.192.in-addr.arpa",
    "254.169.in-addr.arpa",
    "d.f.ip6.arpa",
    "8.e.f.ip6.arpa",
    "9.e.f.ip6.arpa",
    "a.e.f.ip6.arpa",
    "b.e.f.ip6.arpa",
];

pub fn is_local_name(name: &str) -> bool {
    let n = name.trim_end_matches('.').to_ascii_lowercase();
    if n.is_empty() {
        return false;
    }
    if !n.contains('.') {
        return true;
    }
    if LOCAL_ZONES.iter().any(|z| n.strip_suffix(z).is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))) {
        return true;
    }
    let labels: Vec<&str> = n.split('.').collect();
    n.ends_with(".172.in-addr.arpa") && labels.len() >= 4 && labels[labels.len() - 4].parse::<u8>().is_ok_and(|b| (16..=31).contains(&b))
}

pub fn fits_query(name: &str) -> bool {
    name.len() <= 253 && name.split('.').all(|l| (1..=63).contains(&l.len()))
}

pub fn query_for(name: &str, qtype: u16) -> Vec<u8> {
    let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&[0, 1]);
    q
}

#[cfg(kani)]
#[path = "proofs/dns.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/dns.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    fn up(s: &str) -> Option<(Upstream, bool)> {
        parse(s, None)
    }

    #[test]
    fn answers_built_locally() {
        let q = query_for("Example.COM", TYPE_A);
        let qs = question(&q).unwrap();
        assert_eq!((qs.name.as_str(), qs.qtype), ("example.com", TYPE_A));
        let r = answer(&q, &qs, &["10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap()], 60);
        assert_eq!(&r[..2], &q[..2]);
        assert_eq!(r[2] & 0x80, 0x80);
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 2);
        assert_eq!(&r[r.len() - 4..], &[10, 0, 0, 2]);
        let empty = answer(&q, &qs, &[], 60);
        assert_eq!(empty.len(), q.len());
        assert_eq!(u16::from_be_bytes([empty[6], empty[7]]), 0);
    }

    #[test]
    fn local_network_names() {
        for n in [
            "nas",
            "router.",
            "nas.lan",
            "printer.local",
            "router.home.arpa",
            "my.keenetic.net",
            "tplinkwifi.net",
            "box.fritz.box",
            "1.1.168.192.in-addr.arpa",
            "1.0.20.172.in-addr.arpa",
            "5.10.in-addr.arpa",
            "ROUTER.LAN",
        ] {
            assert!(is_local_name(n), "{n}");
        }
        for n in [
            "example.com",
            "lan.example.com",
            "mylan.com",
            "1.0.40.172.in-addr.arpa",
            "8.8.8.8.in-addr.arpa",
            "keenetic.net",
            "evilfritz.box.example",
            "",
        ] {
            assert!(!is_local_name(n), "{n}");
        }
    }

    #[test]
    fn fake_pool_roundtrip() {
        let a = fake::ip_for("example.test");
        assert!(fake::is_fake(a.into()));
        assert_eq!(fake::ip_for("example.test"), a);
        assert_ne!(fake::ip_for("other.test"), a);
        assert_eq!(fake::name_for(a.into()).as_deref(), Some("example.test"));
        assert!(!fake::is_fake("1.1.1.1".parse().unwrap()));
        assert!(fake::is_fake("198.19.255.1".parse().unwrap()));
        assert!(!fake::is_fake("198.18.0.5".parse().unwrap()));
    }

    #[test]
    fn fake_names_are_plain_hostnames() {
        for ok in ["example.com", "_sip._tcp.example.com", "xn--p1ai", "a-b.c_d.ru"] {
            assert!(fake::valid(ok), "{ok}");
        }
        let long = format!("{}.com", "a".repeat(64));
        for bad in ["", "a..b", ".a", "a.", "x\n7 victim.test", "a b.test", "a\0.test", long.as_str()] {
            assert!(!fake::valid(bad), "{bad:?}");
        }
        assert!(!fake::valid(&"a.".repeat(127)));
    }

    #[test]
    fn fake_pool_journal_reload() {
        let dir = std::env::temp_dir().join(format!("rv-fake-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fakedns");
        std::fs::write(&path, "7 kept.test\nbad line\n0 zero.test\n9 later.test\n").unwrap();
        fake::persist(&path);
        let a = fake::ip_for("journal-new.test");
        let text = std::fs::read_to_string(&path).unwrap();
        let off = u32::from(a) - u32::from(std::net::Ipv4Addr::new(198, 19, 0, 0));
        assert!(text.contains(&format!("{off} journal-new.test\n")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn answer_addresses_parsed_back() {
        let q = query_for("x.test", TYPE_A);
        let qs = question(&q).unwrap();
        let ips: Vec<IpAddr> = vec!["10.1.2.3".parse().unwrap(), "10.1.2.4".parse().unwrap()];
        assert_eq!(addresses(&answer(&q, &qs, &ips, 60)), ips);
    }

    #[test]
    fn server_addresses() {
        assert_eq!(up("1.1.1.1"), Some((Upstream::Udp("1.1.1.1:53".parse().unwrap()), false)));
        assert_eq!(up("8.8.8.8:5353"), Some((Upstream::Udp("8.8.8.8:5353".parse().unwrap()), false)));
        assert_eq!(parse("9.9.9.9", Some(9953)), Some((Upstream::Udp("9.9.9.9:9953".parse().unwrap()), false)));
        assert_eq!(up("udp://77.88.8.8"), Some((Upstream::Udp("77.88.8.8:53".parse().unwrap()), false)));
        assert_eq!(up("2606:4700:4700::1111"), Some((Upstream::Udp("[2606:4700:4700::1111]:53".parse().unwrap()), false)));
        assert_eq!(
            up("https://8.8.8.8/dns-query"),
            Some((Upstream::Https { host: "8.8.8.8".into(), port: 443, path: "/dns-query".into() }, false))
        );
        assert_eq!(
            up("https+local://dns.google:8443/resolve"),
            Some((Upstream::Https { host: "dns.google".into(), port: 8443, path: "/resolve".into() }, true))
        );
        assert_eq!(
            up("HTTPS://cloudflare-dns.com"),
            Some((Upstream::Https { host: "cloudflare-dns.com".into(), port: 443, path: "/dns-query".into() }, false))
        );
        assert_eq!(up("tcp://1.1.1.1"), Some((Upstream::Tcp { host: "1.1.1.1".into(), port: 53 }, false)));
        assert_eq!(up("tcp+local://[2001:db8::1]:5353"), Some((Upstream::Tcp { host: "2001:db8::1".into(), port: 5353 }, true)));
        assert_eq!(up("tls://1.1.1.1"), Some((Upstream::Tls { host: "1.1.1.1".into(), port: 853 }, false)));
        assert_eq!(up("quic+local://dns.adguard.com"), Some((Upstream::Quic { host: "dns.adguard.com".into(), port: 853 }, true)));
        assert_eq!(up("localhost"), Some((Upstream::System, true)));
        assert_eq!(up("fakedns"), Some((Upstream::Fake, true)));
    }

    #[tokio::test]
    async fn doh_frames_roundtrip() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (client, server) = tokio::io::duplex(64 * 1024);
                tokio::task::spawn_local(async move {
                    let mut h2 = h2::server::handshake(server).await.unwrap();
                    while let Some(Ok((req, mut respond))) = h2.accept().await {
                        assert_eq!(req.method(), http::Method::POST);
                        assert_eq!(req.uri().path(), "/dns-query");
                        let h = req.headers();
                        assert_eq!(h["accept"], "application/dns-message");
                        assert_eq!(h["accept-encoding"], "gzip");
                        assert!(h["user-agent"].to_str().unwrap().contains("Chrome/"));
                        assert_eq!(h["sec-fetch-mode"], "cors");
                        let pad = h["x-padding"].len();
                        assert!((124..=1249).contains(&pad), "{pad}");
                        let names: Vec<_> = h.keys().map(|k| k.as_str()).collect();
                        assert_eq!(&names[names.len() - 2..], ["content-length", "accept-encoding"]);
                        let mut body = req.into_body();
                        let mut msg = Vec::new();
                        while let Some(c) = body.data().await {
                            msg.extend_from_slice(&c.unwrap());
                        }
                        assert_eq!(&msg[..2], &[0, 0], "id 0, как у xray");
                        assert_eq!(&msg[10..12], &[0, 1], "OPT с паддингом");
                        assert!((25 + 15 + 100..25 + 15 + 300).contains(&msg.len()), "{}", msg.len());
                        msg[2] |= 0x80;
                        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                        std::io::Write::write_all(&mut z, &msg).unwrap();
                        let resp = http::Response::builder().status(200).header("content-encoding", "gzip").body(()).unwrap();
                        let mut s = respond.send_response(resp, false).unwrap();
                        s.send_data(Bytes::from(z.finish().unwrap()), true).unwrap();
                    }
                });
                let (send, conn) = h2::client::handshake(client).await.unwrap();
                tokio::task::spawn_local(async move {
                    let _ = conn.await;
                });
                let uri: http::Uri = "https://dns.test/dns-query".parse().unwrap();
                let q = query_for("fp.test", TYPE_A);
                let r = query(send, uri, q.clone()).await.unwrap();
                assert_eq!(&r[..2], &q[..2]);
                assert_eq!(r[2] & 0x80, 0x80);
            })
            .await;
    }

    #[test]
    fn edns_padding_like_xray() {
        let q = query_for("fp.test", TYPE_A);
        let p = pad_query(&q, 120);
        assert_eq!(p.len(), q.len() + 11 + 4 + 120);
        assert_eq!(&p[10..12], &[0, 1]);
        assert_eq!(&p[q.len()..q.len() + 11], &[0, 0, 41, 0x05, 0x46, 0xE0, 0, 0x80, 0, 0, 124]);
        assert_eq!(&p[q.len() + 11..q.len() + 15], &[0, 12, 0, 120]);
        let mut q2 = q.clone();
        q2[11] = 1;
        q2.extend_from_slice(&[0, 0, 41, 0x10, 0, 0, 0, 0x80, 0, 0, 12, 0, 10, 0, 8, 1, 2, 3, 4, 5, 6, 7, 8]);
        let p2 = pad_query(&q2, 100);
        assert_eq!(p2.len(), q2.len() + 104);
        assert_eq!(&p2[q.len() + 9..q.len() + 11], &(12u16 + 104).to_be_bytes());
        assert_eq!(&p2[q2.len()..q2.len() + 4], &[0, 12, 0, 100]);
        assert_eq!(pad_query(&p2, 100), p2);
        let mut resp = q.clone();
        resp[7] = 1;
        assert_eq!(pad_query(&resp, 100), resp);
    }
}
