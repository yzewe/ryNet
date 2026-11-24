use crate::{DialOptions, Protect};
use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bootstrap {
    #[default]
    System,
    Auto,
    Cloudflare,
    Google,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hop {
    Udp(SocketAddr),
    Tls { host: String, port: u16 },
    Https { host: String, port: u16, path: String },
}

static MODE: RwLock<Bootstrap> = RwLock::new(Bootstrap::System);
static CUSTOM: RwLock<Vec<Hop>> = RwLock::new(Vec::new());
pub fn configure(name: &str) -> io::Result<()> {
    let bad = |what| io::Error::new(io::ErrorKind::InvalidInput, what);
    let mut hops = Vec::new();
    let mode = match name {
        "system" => Bootstrap::System,
        "auto" => Bootstrap::Auto,
        "cloudflare" => Bootstrap::Cloudflare,
        "google" => Bootstrap::Google,
        _ => {
            let list = name.strip_prefix("custom:").ok_or_else(|| bad("unknown server DNS"))?;
            hops = list
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|a| !a.is_empty())
                .map(parse_hop)
                .collect::<Option<Vec<_>>>()
                .filter(|h| !h.is_empty())
                .ok_or_else(|| bad("bad custom server DNS"))?;
            Bootstrap::Custom
        }
    };
    let mut changed = false;
    if mode == Bootstrap::Custom {
        changed = std::mem::replace(&mut *CUSTOM.write().unwrap(), hops.clone()) != hops;
    }
    changed |= std::mem::replace(&mut *MODE.write().unwrap(), mode) != mode;
    if changed {
        forget();
    }
    Ok(())
}

pub fn parse_hop(s: &str) -> Option<Hop> {
    let (scheme, rest) = match s.split_once("://") {
        Some((a, b)) => (a.to_ascii_lowercase(), b),
        None => (String::new(), s),
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let default = match scheme.as_str() {
        "" | "udp" => 53,
        "tls" => 853,
        "https" => 443,
        _ => return None,
    };
    let (host, port) = split_host_port(authority, default)?;
    Some(match scheme.as_str() {
        "" | "udp" => Hop::Udp(SocketAddr::new(host.parse::<IpAddr>().ok()?, port)),
        "tls" => Hop::Tls { host, port },
        _ => Hop::Https { host, port, path: if path.is_empty() { "/dns-query".into() } else { path.into() } },
    })
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
pub fn mode() -> Bootstrap {
    *MODE.read().unwrap()
}

pub const DOH_REST: Duration = Duration::from_secs(300);
static DOH_DOWN_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

pub(crate) fn resting(now: Instant) -> bool {
    DOH_DOWN_UNTIL.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|until| now < until)
}

pub(crate) fn note(result: Result<(), &io::Error>, now: Instant) {
    let mut down = DOH_DOWN_UNTIL.lock().unwrap_or_else(|e| e.into_inner());
    match result {
        Ok(()) => *down = None,
        Err(e) if unreachable(e) => *down = Some(now + DOH_REST),
        Err(_) => {}
    }
}

pub(crate) fn forget() {
    *DOH_DOWN_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub(crate) fn unreachable(e: &io::Error) -> bool {
    !matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput)
}

impl Bootstrap {
    fn endpoint(self) -> Option<(&'static str, [&'static str; 2])> {
        match self {
            Self::System | Self::Auto | Self::Custom => None,
            Self::Cloudflare => Some(("cloudflare-dns.com", ["1.1.1.1", "1.0.0.1"])),
            Self::Google => Some(("dns.google", ["8.8.8.8", "8.8.4.4"])),
        }
    }
}

pub const AUTO_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

pub async fn resolve(host: &str, protect: Option<&Protect>, mode: Bootstrap) -> io::Result<Vec<IpAddr>> {
    if mode == Bootstrap::Auto {
        let race = async {
            let cloudflare = resolve_one(host, protect, Bootstrap::Cloudflare);
            let google = resolve_one(host, protect, Bootstrap::Google);
            tokio::pin!(cloudflare, google);
            let (mut a, mut b) = (None, None);
            while a.is_none() || b.is_none() {
                tokio::select! {
                    r = &mut cloudflare, if a.is_none() => match r { Ok(v) => return Ok(v), Err(e) => a = Some(e) },
                    r = &mut google, if b.is_none() => match r { Ok(v) => return Ok(v), Err(e) => b = Some(e) },
                }
            }
            let (a, b) = (a.unwrap(), b.unwrap());
            Err(if unreachable(&a) && !unreachable(&b) { b } else { a })
        };
        return tokio::time::timeout(AUTO_WAIT, race)
            .await
            .unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "encrypted server DNS timed out")));
    }
    resolve_one(host, protect, mode).await
}

async fn resolve_one(host: &str, protect: Option<&Protect>, mode: Bootstrap) -> io::Result<Vec<IpAddr>> {
    let (a, aaaa) = tokio::join!(query(host, 1, protect, mode), query(host, 28, protect, mode));
    let mut ips = Vec::new();
    if let Ok(a) = a.as_ref() {
        ips.extend(a.iter().copied());
    }
    if let Ok(a) = aaaa.as_ref() {
        ips.extend(a.iter().copied());
    }
    ips.retain(|ip| !crate::is_fake(ip));
    ips.dedup();
    if ips.is_empty() {
        return Err(a
            .err()
            .or_else(|| aaaa.err())
            .unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "encrypted server DNS: no address")));
    }
    Ok(ips)
}

fn packet(host: &str, ty: u16) -> io::Result<(u16, Vec<u8>)> {
    use ring::rand::SecureRandom;
    let mut id = [0; 2];
    ring::rand::SystemRandom::new().fill(&mut id).map_err(|_| io::Error::other("DNS RNG"))?;
    let id = u16::from_ne_bytes(id);
    let mut body = crate::dns_query(host, id).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad DNS name"))?;
    let end = body.len();
    body[end - 4..end - 2].copy_from_slice(&ty.to_be_bytes());
    Ok((id, body))
}

async fn query(host: &str, ty: u16, protect: Option<&Protect>, mode: Bootstrap) -> io::Result<Vec<IpAddr>> {
    if mode == Bootstrap::Custom {
        return custom(host, ty, protect).await;
    }
    let (name, ips) = mode.endpoint().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not DoH"))?;
    let (id, body) = packet(host, ty)?;
    let opts = DialOptions { tcp_no_delay: true, protect: protect.cloned(), ..Default::default() };
    let run = async {
        let mut last = None;
        for ip in ips {
            let result = async {
                let tcp = crate::attempt(ip.parse().unwrap(), 443, &opts).await?;
                doh(tcp, name, 443, "/dns-query", &body, id).await
            };
            match tokio::time::timeout(std::time::Duration::from_secs(5), result).await {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(e)) => last = Some(e),
                Err(_) => last = Some(io::Error::new(io::ErrorKind::TimedOut, "encrypted server DNS timed out")),
            }
        }
        Err(last.unwrap())
    };
    run.await
}

async fn doh(tcp: tokio::net::TcpStream, name: &str, port: u16, path: &str, body: &[u8], id: u16) -> io::Result<Vec<IpAddr>> {
    let cfg = Arc::new(rv_tls::ClientConfig {
        server_name: name.into(),
        alpn: vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        alpn_override: true,
        fingerprint: rv_tls::Fingerprint::Chrome,
        verify: rv_tls::Verify::WebPki,
    });
    let mut tls = rv_tls::connect(tcp, cfg).await.map_err(io::Error::other)?;
    let authority = match (port, name.contains(':')) {
        (443, false) => name.to_string(),
        (443, true) => format!("[{name}]"),
        (_, false) => format!("{name}:{port}"),
        (_, true) => format!("[{name}]:{port}"),
    };
    if tls.alpn() == Some(b"h2") {
        let packet = h2_query(tls, &authority, path, body).await?;
        return crate::dns_answer(&packet, id).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad DoH DNS response"));
    }
    let header = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: Go-http-client/1.1\r\nContent-Type: application/dns-message\r\nAccept: application/dns-message\r\nAccept-Encoding: identity\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    tls.write_all(header.as_bytes()).await?;
    tls.write_all(body).await?;
    tls.flush().await?;
    let mut response = Vec::new();
    let _ = tls.take(65537).read_to_end(&mut response).await;
    let packet = http_body(&response)?;
    crate::dns_answer(&packet, id).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad DoH DNS response"))
}

async fn custom(host: &str, ty: u16, protect: Option<&Protect>) -> io::Result<Vec<IpAddr>> {
    let (id, body) = packet(host, ty)?;
    let hops = CUSTOM.read().unwrap().clone();
    let opts = DialOptions { tcp_no_delay: true, protect: protect.cloned(), ..Default::default() };
    let mut last = None;
    for hop in &hops {
        match tokio::time::timeout(Duration::from_secs(5), custom_hop(hop, &body, id, protect, &opts)).await {
            Ok(Ok(v)) => return Ok(v),
            Ok(Err(e)) => last = Some(e),
            Err(_) => last = Some(io::Error::new(io::ErrorKind::TimedOut, "server DNS timed out")),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no custom server DNS")))
}

async fn custom_hop(hop: &Hop, body: &[u8], id: u16, protect: Option<&Protect>, opts: &DialOptions) -> io::Result<Vec<IpAddr>> {
    async fn dial(host: &str, port: u16, opts: &DialOptions) -> io::Result<tokio::net::TcpStream> {
        let ips = match host.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => tokio::net::lookup_host((host, 0)).await?.map(|a| a.ip()).collect(),
        };
        crate::dial_tcp_resolved(ips, port, opts).await
    }
    match hop {
        Hop::Udp(server) => {
            let sock = crate::udp_socket(server.is_ipv6(), protect)?;
            sock.connect(server).await?;
            sock.send(body).await?;
            let mut buf = [0u8; 1500];
            loop {
                let n = sock.recv(&mut buf).await?;
                if n >= 2 && u16::from_be_bytes([buf[0], buf[1]]) == id {
                    return crate::dns_answer(&buf[..n], id)
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad server DNS response"));
                }
            }
        }
        Hop::Https { host, port, path } => doh(dial(host, *port, opts).await?, host, *port, path, body, id).await,
        Hop::Tls { host, port } => {
            let tcp = dial(host, *port, opts).await?;
            let cfg = Arc::new(rv_tls::ClientConfig {
                server_name: host.clone(),
                alpn: vec![b"dot".to_vec()],
                alpn_override: true,
                fingerprint: rv_tls::Fingerprint::Chrome,
                verify: rv_tls::Verify::WebPki,
            });
            let mut tls = rv_tls::connect(tcp, cfg).await.map_err(io::Error::other)?;
            let mut frame = (body.len() as u16).to_be_bytes().to_vec();
            frame.extend_from_slice(body);
            tls.write_all(&frame).await?;
            tls.flush().await?;
            let mut len = [0u8; 2];
            tls.read_exact(&mut len).await?;
            let mut reply = vec![0u8; u16::from_be_bytes(len) as usize];
            tls.read_exact(&mut reply).await?;
            crate::dns_answer(&reply, id).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad DoT DNS response"))
        }
    }
}

async fn h2_query<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    tls: S,
    host: &str,
    path: &str,
    body: &[u8],
) -> io::Result<Vec<u8>> {
    let (send, conn) = h2::client::Builder::new()
        .enable_push(false)
        .initial_window_size(4 << 20)
        .initial_connection_window_size((1 << 30) + 65535)
        .max_frame_size(1 << 20)
        .max_header_list_size(10 << 20)
        .handshake::<_, bytes::Bytes>(tls)
        .await
        .map_err(io::Error::other)?;
    let exchange = async {
        let mut send = send.ready().await.map_err(io::Error::other)?;
        let req = http::Request::builder()
            .method("POST")
            .uri(format!("https://{host}{path}"))
            .header("content-type", "application/dns-message")
            .header("accept", "application/dns-message")
            .header("user-agent", "Go-http-client/2.0")
            .header("accept-encoding", "identity")
            .header("content-length", body.len())
            .body(())
            .map_err(io::Error::other)?;
        let (response, mut request) = send.send_request(req, false).map_err(io::Error::other)?;
        request.send_data(bytes::Bytes::copy_from_slice(body), true).map_err(io::Error::other)?;
        let response = response.await.map_err(io::Error::other)?;
        if response.status() != http::StatusCode::OK {
            return Err(io::Error::other(format!("DoH HTTP status {}", response.status())));
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.split(';').next() == Some("application/dns-message"))
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bad DoH content type"));
        }
        let expected = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok());
        let mut recv = response.into_body();
        let mut packet = Vec::new();
        while let Some(data) = recv.data().await {
            let data = data.map_err(io::Error::other)?;
            if packet.len() + data.len() > 65536 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "DoH response too large"));
            }
            packet.extend_from_slice(&data);
            recv.flow_control().release_capacity(data.len()).map_err(io::Error::other)?;
        }
        if expected.is_some_and(|n| n != packet.len()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated DoH response"));
        }
        Ok(packet)
    };
    tokio::pin!(conn, exchange);
    tokio::select! { result=&mut exchange=>result, result=&mut conn=>{
        result.map_err(io::Error::other)?;
        Err(io::Error::new(io::ErrorKind::UnexpectedEof,"DoH connection ended before its response"))
    }}
}

fn http_body(raw: &[u8]) -> io::Result<Vec<u8>> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "bad DoH HTTP response");
    if raw.len() > 65536 {
        return Err(bad());
    }
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(bad)?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| bad())?.to_ascii_lowercase();
    if head.split_whitespace().nth(1) != Some("200") {
        return Err(bad());
    }
    let headers: Vec<_> = head.lines().skip(1).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim(), v.trim())).collect();
    if !headers.iter().any(|(k, v)| *k == "content-type" && v.split(';').next() == Some("application/dns-message")) {
        return Err(bad());
    }
    if headers.iter().any(|(k, v)| *k == "content-encoding" && *v != "identity") {
        return Err(bad());
    }
    let mut body = &raw[end + 4..];
    if headers.iter().any(|(k, v)| *k == "transfer-encoding" && *v == "chunked") {
        let mut out = Vec::new();
        loop {
            let i = body.windows(2).position(|w| w == b"\r\n").ok_or_else(bad)?;
            let n = usize::from_str_radix(std::str::from_utf8(&body[..i]).map_err(|_| bad())?.split(';').next().unwrap(), 16)
                .map_err(|_| bad())?;
            body = &body[i + 2..];
            if n == 0 {
                if !body.starts_with(b"\r\n") {
                    return Err(bad());
                }
                return Ok(out);
            }
            if n > body.len() || body.get(n..n + 2) != Some(b"\r\n") {
                return Err(bad());
            }
            out.extend_from_slice(&body[..n]);
            body = &body[n + 2..];
        }
    }
    let len = headers.iter().find(|(k, _)| *k == "content-length").and_then(|(_, v)| v.parse::<usize>().ok()).ok_or_else(bad)?;
    if len != body.len() {
        return Err(bad());
    }
    Ok(body.to_vec())
}

#[cfg(kani)]
#[path = "proofs/bootstrap.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doh_requires_complete_authenticated_message_framing() {
        let prefix = b"HTTP/1.1 200 OK\r\nContent-Type: application/dns-message\r\nContent-Length: 3\r\n\r\n";
        let mut raw = prefix.to_vec();
        raw.extend_from_slice(b"abc");
        assert_eq!(http_body(&raw).unwrap(), b"abc");
        raw.pop();
        assert!(http_body(&raw).is_err());
        assert!(http_body(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 3\r\n\r\nabc").is_err());
    }
    #[test]
    fn custom_servers_parse() {
        assert_eq!(parse_hop("1.1.1.1"), Some(Hop::Udp("1.1.1.1:53".parse().unwrap())));
        assert_eq!(parse_hop("[2606:4700::1111]:5353"), Some(Hop::Udp("[2606:4700::1111]:5353".parse().unwrap())));
        assert_eq!(parse_hop("2606:4700::1111"), Some(Hop::Udp("[2606:4700::1111]:53".parse().unwrap())));
        assert_eq!(parse_hop("tls://dns.example"), Some(Hop::Tls { host: "dns.example".into(), port: 853 }));
        assert_eq!(parse_hop("HTTPS://dns.example:8443/q"), Some(Hop::Https { host: "dns.example".into(), port: 8443, path: "/q".into() }),);
        assert_eq!(parse_hop("https://1.1.1.1"), Some(Hop::Https { host: "1.1.1.1".into(), port: 443, path: "/dns-query".into() }),);
        assert_eq!(parse_hop("dns.example"), None);
        assert_eq!(parse_hop("quic://dns.example"), None);
        assert_eq!(parse_hop("https://"), None);
    }
    #[test]
    fn custom_mode_needs_a_valid_list() {
        assert!(configure("custom:").is_err());
        assert!(configure("custom:nonsense").is_err());
        assert!(configure("custom:1.1.1.1, https://dns.example/dns-query").is_ok());
        assert_eq!(CUSTOM.read().unwrap().len(), 2);
        configure("system").unwrap();
    }
    #[test]
    fn doh_chunked_requires_final_chunk() {
        let p = "HTTP/1.1 200 OK\r\nContent-Type: application/dns-message\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(http_body(format!("{p}3\r\nabc\r\n0\r\n\r\n").as_bytes()).unwrap(), b"abc");
        assert!(http_body(format!("{p}3\r\nabc\r\n").as_bytes()).is_err());
    }
}
