use std::time::{Duration, Instant};

use rv_config::Profile;
use rv_net::{DialOptions, Protect};
use rv_vless::Address;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{Error, Proxy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Tcp,
    Proxy,
    Icmp,
    ProxyWarm,
}

impl Mode {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Mode::Tcp,
            2 => Mode::Icmp,
            3 => Mode::ProxyWarm,
            _ => Mode::Proxy,
        }
    }
}

pub const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeUrl {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub strict: bool,
    pub body: u32,
    pub within: Duration,
    pub alt: Vec<ProbeUrl>,
}

pub const SLOW: &str = "too slow";

const REDIRECTS: usize = 3;

impl Default for ProbeUrl {
    fn default() -> Self {
        ProbeUrl {
            https: false,
            host: "www.gstatic.com".into(),
            port: 80,
            path: "/generate_204".into(),
            strict: true,
            body: 0,
            within: Duration::ZERO,
            alt: Vec::new(),
        }
    }
}

impl ProbeUrl {
    pub fn parse(url: &str) -> Option<ProbeUrl> {
        let mut list = url.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|u| !u.is_empty());
        let mut first = ProbeUrl::parse_one(list.next().unwrap_or_default())?;
        first.alt = list.map(ProbeUrl::parse_one).collect::<Option<_>>()?;
        Some(first)
    }
    fn parse_one(url: &str) -> Option<ProbeUrl> {
        let url = url.trim();
        if url.is_empty() {
            return Some(ProbeUrl::default());
        }
        let (https, rest) = if let Some(r) = url.strip_prefix("https://") { (true, r) } else { (false, url.strip_prefix("http://")?) };
        let (rest, frag) = rest.split_once('#').unwrap_or((rest, ""));
        let opt = |k: &str| frag.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('=')?.parse::<u32>().ok()).unwrap_or(0);
        let (body, within) = (opt("body"), Duration::from_millis(opt("within") as u64));
        let (hostport, path) = match rest.find(['/', '?']) {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let default_port = if https { 443 } else { 80 };
        let (host, port) = if let Some(ip) = hostport.strip_prefix('[') {
            let (h, tail) = ip.split_once(']')?;
            h.parse::<std::net::Ipv6Addr>().ok()?;
            (h, if tail.is_empty() { default_port } else { tail.strip_prefix(':')?.parse().ok()? })
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h, p.parse().ok()?),
                None => (hostport, default_port),
            }
        };
        let path = if path.starts_with('?') { format!("/{path}") } else { path.to_string() };
        (!host.is_empty()).then(|| ProbeUrl { https, host: host.to_string(), port, path, strict: false, body, within, alt: Vec::new() })
    }
    fn authority(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == if self.https { 443 } else { 80 } { host } else { format!("{host}:{}", self.port) }
    }
    fn follow(&self, location: &str) -> Option<ProbeUrl> {
        let location = location.split('#').next()?.trim();
        let next = if location.starts_with("http://") || location.starts_with("https://") {
            ProbeUrl::parse_one(location)?
        } else if let Some(rest) = location.strip_prefix("//") {
            ProbeUrl::parse_one(&format!("{}://{rest}", if self.https { "https" } else { "http" }))?
        } else if location.starts_with('/') {
            ProbeUrl { path: location.to_string(), ..self.clone() }
        } else {
            return None;
        };
        Some(ProbeUrl { body: self.body, within: self.within, alt: Vec::new(), ..next })
    }
}

pub async fn ping(profile: &Profile, mode: Mode, protect: Option<Protect>) -> Result<u32, Error> {
    ping_with(profile, mode, protect, TIMEOUT).await
}

pub async fn ping_with(profile: &Profile, mode: Mode, protect: Option<Protect>, timeout: Duration) -> Result<u32, Error> {
    ping_url(profile, mode, protect, timeout, &ProbeUrl::default()).await
}

pub async fn ping_url(profile: &Profile, mode: Mode, protect: Option<Protect>, timeout: Duration, url: &ProbeUrl) -> Result<u32, Error> {
    let main = profile.main_outbound().ok_or_else(|| Error::Config("profile has no proxy outbound".into()))?;
    let candidates = profile.balancer_members();
    if candidates.len() < 2 {
        return ping_outbound(profile, main, mode, protect, timeout, url).await;
    }
    let runs = candidates.iter().map(|ob| ping_outbound(profile, ob, mode, protect.clone(), timeout, url));
    crate::engine::first_ok(runs).await.unwrap_or_else(|| Err(Error::Config("no candidates".into())))
}

async fn ping_outbound(
    profile: &Profile,
    ob: &rv_config::Outbound,
    mode: Mode,
    protect: Option<Protect>,
    timeout: Duration,
    url: &ProbeUrl,
) -> Result<u32, Error> {
    let chained = ob.stream.sockopt.dialer_proxy.as_deref().is_some_and(|d| d != crate::tune::FRAGMENT);
    let mode = match mode {
        Mode::Icmp | Mode::Tcp if chained => Mode::Proxy,
        Mode::Icmp if !rv_net::icmp_allowed() => match ob.kind {
            rv_config::OutboundKind::Hysteria2(_) => Mode::Proxy,
            _ => Mode::Tcp,
        },
        m => m,
    };
    let mut start = Instant::now();
    let run = async {
        match mode {
            Mode::Icmp => {
                let (address, _) = ob.kind.server().ok_or_else(|| Error::Unsupported("no server".into()))?;
                let ip = rv_net::resolve_protected(address, protect.as_ref())
                    .await?
                    .into_iter()
                    .min_by_key(|i| !i.is_ipv4())
                    .ok_or_else(|| Error::Io(std::io::Error::other("no address")))?;
                let rtt = rv_net::icmp_ping(ip, protect.as_ref(), timeout).await?;
                Ok(Some(rtt))
            }
            Mode::ProxyWarm => probe_twice(&Proxy::build(profile, &ob.tag, protect)?, url).await.map(Some),
            Mode::Tcp => {
                if matches!(ob.kind, rv_config::OutboundKind::Hysteria2(_)) {
                    return Err(Error::Unsupported("tcp ping of udp protocol".into()));
                }
                let (address, port) = ob.kind.server().ok_or_else(|| Error::Unsupported("no server".into()))?;
                let opts = DialOptions { tcp_no_delay: true, protect, timeout: Some(timeout), ..Default::default() };
                rv_net::dial_tcp(address, port, &opts).await?;
                Ok(None)
            }
            Mode::Proxy => probe_once(&Proxy::build(profile, &ob.tag, protect)?, url).await.map(|_| None),
        }
    };
    let measured = tokio::time::timeout(timeout, run).await.map_err(|_| Error::Io(std::io::ErrorKind::TimedOut.into()))??;
    if let Some(d) = measured {
        start = Instant::now() - d;
    }
    Ok(start.elapsed().as_millis().min(u32::MAX as u128) as u32)
}

fn refused_by_server(e: &Error) -> bool {
    e.to_string().contains("unexpected http status")
}

impl ProbeUrl {
    fn https_fallback(&self) -> Option<ProbeUrl> {
        (*self == ProbeUrl::default()).then(|| ProbeUrl { https: true, port: 443, ..ProbeUrl::default() })
    }
}

async fn probe_once(out: &Proxy, url: &ProbeUrl) -> Result<(), Error> {
    if url.alt.is_empty() {
        let r = Box::pin(probe_request(out, url, false)).await;
        if let Err(e) = &r
            && refused_by_server(e)
            && let Some(https) = url.https_fallback()
        {
            return Box::pin(probe_request(out, &https, false)).await;
        }
        return r;
    }
    let runs = std::iter::once(url).chain(&url.alt).map(|u| Box::pin(probe_request(out, u, false)));
    crate::engine::first_ok(runs).await.unwrap_or_else(|| Err(Error::Config("no probe address".into())))
}
async fn probe_request(out: &Proxy, url: &ProbeUrl, observatory: bool) -> Result<(), Error> {
    if url.body > 0 && !observatory {
        return body_probe(|u: &ProbeUrl| Box::pin(out.connect(Address::Domain(u.host.clone()), u.port)), url).await;
    }
    let s = Box::pin(out.connect(Address::Domain(url.host.clone()), url.port)).await?;
    let req = if observatory {
        let mut h = rv_xhttp::browser::Headers::new();
        rv_xhttp::browser::apply_defaults(&mut h, rv_xhttp::browser::Variant::Nav);
        rv_xhttp::browser::set(&mut h, "Connection", "close");
        rv_xhttp::browser::http1_request("GET", &url.path, &url.authority(), &h)
    } else {
        format!("HEAD {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Go-http-client/1.1\r\nConnection: close\r\n\r\n", url.path, url.authority())
            .into_bytes()
    };
    if url.https {
        let cfg = std::sync::Arc::new(rv_tls::ClientConfig {
            server_name: url.host.clone(),
            alpn: vec![b"http/1.1".to_vec()],
            alpn_override: true,
            fingerprint: rv_tls::Fingerprint::Chrome,
            verify: rv_tls::Verify::WebPki,
        });
        let mut t = Box::pin(rv_tls::connect(s, cfg)).await?;
        status_expected(&mut t, &req, url.strict, observatory).await
    } else {
        let mut s = s;
        status_expected(&mut s, &req, url.strict, observatory).await
    }
}

#[derive(Default)]
struct Seen {
    answered: bool,
    body: usize,
}

enum Hop {
    Done,
    Redirect(ProbeUrl),
}

async fn body_probe<C, F>(mut connect: C, url: &ProbeUrl) -> Result<(), Error>
where
    C: FnMut(&ProbeUrl) -> F,
    F: std::future::Future<Output = Result<crate::BoxStream, Error>>,
{
    let deadline = (!url.within.is_zero()).then(|| tokio::time::Instant::now() + url.within);
    let mut url = url.clone();
    for _ in 0..=REDIRECTS {
        let mut seen = Seen::default();
        let hop = body_hop(&mut connect, &url, &mut seen);
        let r = match deadline {
            Some(d) => tokio::time::timeout_at(d, hop).await,
            None => Ok(hop.await),
        };
        match r.unwrap_or_else(|_| Err(late(&seen, &url)))? {
            Hop::Done => return Ok(()),
            Hop::Redirect(next) => url = next,
        }
    }
    Err(Error::Io(std::io::Error::other("too many redirects")))
}

async fn body_hop<C, F>(connect: &mut C, url: &ProbeUrl, seen: &mut Seen) -> Result<Hop, Error>
where
    C: FnMut(&ProbeUrl) -> F,
    F: std::future::Future<Output = Result<crate::BoxStream, Error>>,
{
    let s = connect(url).await?;
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Mozilla/5.0\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        url.path,
        url.authority()
    );
    if url.https {
        let cfg = std::sync::Arc::new(rv_tls::ClientConfig {
            server_name: url.host.clone(),
            alpn: vec![b"http/1.1".to_vec()],
            alpn_override: true,
            fingerprint: rv_tls::Fingerprint::Chrome,
            verify: rv_tls::Verify::WebPki,
        });
        let mut t = Box::pin(rv_tls::connect(s, cfg)).await?;
        body_read(&mut t, req.as_bytes(), url, seen).await
    } else {
        let mut s = s;
        body_read(&mut s, req.as_bytes(), url, seen).await
    }
}

async fn body_read<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    s: &mut S,
    req: &[u8],
    url: &ProbeUrl,
    seen: &mut Seen,
) -> Result<Hop, Error> {
    s.write_all(req).await?;
    s.flush().await?;
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let m = s.read(&mut chunk).await?;
        if m == 0 || buf.len() > 16 * 1024 {
            return Err(Error::Io(std::io::Error::other("no http response")));
        }
        seen.answered = true;
        buf.extend_from_slice(&chunk[..m]);
    };
    if !buf.starts_with(b"HTTP/") {
        return Err(Error::Io(std::io::Error::other("no http response")));
    }
    let code = buf.get(9..12).and_then(|c| std::str::from_utf8(c).ok()).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    if matches!(code, 301 | 302 | 303 | 307 | 308) {
        let head = String::from_utf8_lossy(&buf[..end]);
        let location = head
            .lines()
            .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("location")).map(|(_, v)| v.trim().to_string()));
        if let Some(next) = location.and_then(|l| url.follow(&l)) {
            return Ok(Hop::Redirect(next));
        }
    }
    if !(200..300).contains(&code) {
        let status = String::from_utf8_lossy(&buf[..buf.len().min(12)]).into_owned();
        return Err(Error::Io(std::io::Error::other(format!("unexpected http status `{status}`"))));
    }
    seen.body = buf.len() - end;
    while seen.body < url.body as usize {
        let m = s.read(&mut chunk).await?;
        if m == 0 {
            return Err(Error::Io(std::io::Error::other(format!("body ended at {} bytes", seen.body))));
        }
        seen.body += m;
    }
    Ok(Hop::Done)
}

fn late(seen: &Seen, url: &ProbeUrl) -> Error {
    if seen.answered {
        Error::Io(std::io::Error::other(format!("{SLOW}: {} of {} bytes in {} ms", seen.body, url.body, url.within.as_millis())))
    } else {
        Error::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "no answer to the body check in time"))
    }
}

async fn probe_twice(out: &Proxy, url: &ProbeUrl) -> Result<std::time::Duration, Error> {
    let warmed = match warm(out, url).await {
        Err(e) if refused_by_server(&e) => match url.https_fallback() {
            Some(https) => warm(out, &https).await?,
            None => return Err(e),
        },
        r => r?,
    };
    match warmed {
        Some(d) => Ok(d),
        None => {
            let start = Instant::now();
            probe_once(out, url).await?;
            Ok(start.elapsed())
        }
    }
}

async fn warm(out: &Proxy, url: &ProbeUrl) -> Result<Option<std::time::Duration>, Error> {
    let s = out.connect(Address::Domain(url.host.clone()), url.port).await?;
    let req = format!("HEAD {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Go-http-client/1.1\r\n\r\n", url.path, url.authority());
    async fn twice<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
        s: &mut S,
        req: &[u8],
        strict: bool,
    ) -> Result<Option<std::time::Duration>, Error> {
        head(s, req, strict).await?;
        let start = Instant::now();
        Ok(head(s, req, strict).await.ok().map(|_| start.elapsed()))
    }
    if url.https {
        let cfg = std::sync::Arc::new(rv_tls::ClientConfig {
            server_name: url.host.clone(),
            alpn: vec![b"http/1.1".to_vec()],
            alpn_override: true,
            fingerprint: rv_tls::Fingerprint::Chrome,
            verify: rv_tls::Verify::WebPki,
        });
        let mut t = rv_tls::connect(s, cfg).await?;
        twice(&mut t, req.as_bytes(), url.strict).await
    } else {
        let mut s = s;
        twice(&mut s, req.as_bytes(), url.strict).await
    }
}

async fn head<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(s: &mut S, req: &[u8], strict: bool) -> Result<(), Error> {
    s.write_all(req).await?;
    s.flush().await?;
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && !buf.windows(2).any(|w| w == b"\n\n") {
        let m = s.read(&mut chunk).await?;
        if m == 0 || buf.len() > 16 * 1024 {
            return Err(Error::Io(std::io::Error::other("no http response")));
        }
        buf.extend_from_slice(&chunk[..m]);
    }
    if !buf.starts_with(b"HTTP/") {
        return Err(Error::Io(std::io::Error::other("no http response")));
    }
    let code = buf.get(9..12).and_then(|c| std::str::from_utf8(c).ok()).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    let ok = if strict { code == 204 } else { (200..400).contains(&code) };
    if !ok {
        let status = String::from_utf8_lossy(&buf[..buf.len().min(12)]).into_owned();
        return Err(Error::Io(std::io::Error::other(format!("unexpected http status `{status}`"))));
    }
    Ok(())
}

#[cfg(test)]
async fn status<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(s: &mut S, req: &[u8], strict: bool) -> Result<(), Error> {
    status_expected(s, req, strict, false).await
}
async fn status_expected<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    s: &mut S,
    req: &[u8],
    strict: bool,
    any_status: bool,
) -> Result<(), Error> {
    s.write_all(req).await?;
    s.flush().await?;
    let mut buf = [0u8; 64];
    let mut n = 0;
    while n < 12 {
        let m = s.read(&mut buf[n..]).await?;
        if m == 0 {
            break;
        }
        n += m;
    }
    if n == 0 || !buf[..n].starts_with(b"HTTP/") {
        return Err(Error::Io(std::io::Error::other("no http response")));
    }
    let code = buf.get(9..n.min(12)).and_then(|c| std::str::from_utf8(c).ok()).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    let ok = if any_status {
        (100..600).contains(&code)
    } else if strict {
        code == 204
    } else {
        (200..400).contains(&code)
    };
    if !ok {
        let status = String::from_utf8_lossy(&buf[..n.min(12)]).into_owned();
        return Err(Error::Io(std::io::Error::other(format!("unexpected http status `{status}`"))));
    }
    Ok(())
}

pub async fn direct(url: &ProbeUrl, protect: Option<Protect>, timeout: Duration) -> Result<u32, Error> {
    let start = Instant::now();
    let deadline = tokio::time::Instant::now() + timeout;
    let opts = DialOptions { tcp_no_delay: true, protect, timeout: Some(timeout), ..Default::default() };
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Mozilla/5.0\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        url.path,
        url.authority()
    );
    let timed_out = || Error::Io(std::io::ErrorKind::TimedOut.into());
    let answer = async {
        let mut s: crate::BoxStream = Box::new(rv_net::dial_tcp(&url.host, url.port, &opts).await?);
        if url.https {
            let cfg = std::sync::Arc::new(rv_tls::ClientConfig {
                server_name: url.host.clone(),
                alpn: vec![b"http/1.1".to_vec()],
                alpn_override: true,
                fingerprint: rv_tls::Fingerprint::Chrome,
                verify: rv_tls::Verify::WebPki,
            });
            s = Box::new(Box::pin(rv_tls::connect(s, cfg)).await?);
        }
        s.write_all(req.as_bytes()).await?;
        s.flush().await?;
        let mut buf = Vec::with_capacity(4096);
        let mut chunk = [0u8; 8192];
        let end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            let m = s.read(&mut chunk).await?;
            if m == 0 || buf.len() > 16 * 1024 {
                return Err(Error::Io(std::io::Error::other("no http response")));
            }
            buf.extend_from_slice(&chunk[..m]);
        };
        if !buf.starts_with(b"HTTP/") {
            return Err(Error::Io(std::io::Error::other("no http response")));
        }
        let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
        let length = head.lines().find_map(|l| l.strip_prefix("content-length:")?.trim().parse::<usize>().ok());
        Ok((s, buf.len() - end, length))
    };
    let (mut s, mut got, length) = tokio::time::timeout_at(deadline, answer).await.map_err(|_| timed_out())??;
    let ms = start.elapsed().as_millis().clamp(1, u32::MAX as u128) as u32;
    let want = url.body as usize;
    let until = tokio::time::Instant::now() + url.within;
    let mut chunk = [0u8; 8192];
    while got < want {
        match tokio::time::timeout_at(until, s.read(&mut chunk)).await {
            Err(_) => {
                return Err(Error::Io(std::io::Error::other(format!("{SLOW}: {got} of {want} bytes in {} ms", url.within.as_millis()))));
            }
            Ok(Ok(0)) if length.is_some_and(|l| got >= l) => break,
            Ok(Ok(0)) => return Err(Error::Io(std::io::Error::other(format!("body ended at {got} bytes")))),
            Ok(Ok(m)) => got += m,
            Ok(Err(e)) => return Err(e.into()),
        }
    }
    Ok(ms)
}

pub async fn probe(out: &Proxy) -> Option<Duration> {
    let start = Instant::now();
    match tokio::time::timeout(TIMEOUT, probe_once(out, &ProbeUrl::default())).await {
        Ok(Ok(())) => Some(start.elapsed()),
        _ => None,
    }
}

pub(crate) async fn probe_observatory(out: &Proxy, url: &ProbeUrl) -> Option<Duration> {
    let start = Instant::now();
    match tokio::time::timeout(TIMEOUT, Box::pin(probe_request(out, url, true))).await {
        Ok(Ok(())) => Some(start.elapsed()),
        _ => None,
    }
}

pub async fn ping_all(profiles: Vec<Profile>, mode: Mode, protect: Option<Protect>) -> Vec<Result<u32, String>> {
    ping_stream(profiles, mode, protect, Opts::default(), |_, _| {}).await
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub parallel: usize,
    pub timeout: Duration,
    pub url: ProbeUrl,
    pub cancel: Option<&'static std::sync::atomic::AtomicBool>,
}

pub const CANCELLED: &str = "cancelled";

impl Default for Opts {
    fn default() -> Self {
        Self { parallel: 0, timeout: TIMEOUT, url: ProbeUrl::default(), cancel: None }
    }
}

pub async fn ping_stream(
    profiles: Vec<Profile>,
    mode: Mode,
    protect: Option<Protect>,
    opts: Opts,
    mut on_each: impl FnMut(usize, &Result<u32, String>),
) -> Vec<Result<u32, String>> {
    let n = profiles.len();
    let limit = if opts.parallel == 0 { n.max(1) } else { opts.parallel };
    let gate = std::rc::Rc::new(tokio::sync::Semaphore::new(limit));
    let url = std::rc::Rc::new(opts.url.clone());
    let cancel = opts.cancel;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let local = tokio::task::LocalSet::new();
    for (i, p) in profiles.into_iter().enumerate() {
        let protect = protect.clone();
        let gate = gate.clone();
        let tx = tx.clone();
        let url = url.clone();
        local.spawn_local(async move {
            let _permit = gate.acquire().await;
            let r = ping_url(&p, mode, protect, opts.timeout, &url).await.map_err(|e| {
                tracing::debug!("ping {}: {e}", p.remarks);
                crate::explain::explain(&e)
            });
            let _ = tx.send((i, r));
        });
    }
    drop(tx);
    local
        .run_until(async move {
            let mut out: Vec<Result<u32, String>> = (0..n).map(|_| Err(CANCELLED.to_string())).collect();
            let stopped = async {
                let Some(c) = cancel else { return std::future::pending().await };
                while !c.load(std::sync::atomic::Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            };
            tokio::pin!(stopped);
            loop {
                let next = tokio::select! {
                    r = rx.recv() => r,
                    () = &mut stopped => { tracing::debug!("ping cancelled"); None }
                };
                let Some((i, r)) = next else { break };
                on_each(i, &r);
                out[i] = r;
            }
            out
        })
        .await
}

const COUNTRY_URL: &str = "https://ipwho.is/?fields=success,country_code";

pub async fn countries(profiles: Vec<Profile>, protect: Option<Protect>, parallel: usize, timeout: Duration) -> Vec<Option<String>> {
    let gate = std::rc::Rc::new(tokio::sync::Semaphore::new(if parallel == 0 { profiles.len().max(1) } else { parallel }));
    let url = std::rc::Rc::new(ProbeUrl::parse(COUNTRY_URL).unwrap_or_default());
    let local = tokio::task::LocalSet::new();
    let runs: Vec<_> = profiles
        .into_iter()
        .map(|p| {
            let (gate, url, protect) = (gate.clone(), url.clone(), protect.clone());
            local.spawn_local(async move {
                let _permit = gate.acquire().await;
                let ob = p.main_outbound()?;
                let out = Proxy::build(&p, &ob.tag, protect).ok()?;
                let body = tokio::time::timeout(timeout, Box::pin(fetch(&out, &url)))
                    .await
                    .ok()?
                    .map_err(|e| tracing::debug!("country {}: {e}", p.remarks))
                    .ok()?;
                country_code(&body)
            })
        })
        .collect();
    local
        .run_until(async move {
            let mut out = Vec::with_capacity(runs.len());
            for r in runs {
                out.push(r.await.ok().flatten());
            }
            out
        })
        .await
}

async fn fetch(out: &Proxy, url: &ProbeUrl) -> Result<Vec<u8>, Error> {
    let s = Box::pin(out.connect(Address::Domain(url.host.clone()), url.port)).await?;
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Mozilla/5.0\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        url.path,
        url.authority()
    );
    async fn all<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(s: &mut S, req: &[u8]) -> Result<Vec<u8>, Error> {
        s.write_all(req).await?;
        s.flush().await?;
        let mut buf = Vec::with_capacity(1024);
        let mut chunk = [0u8; 4096];
        while buf.len() < 16 * 1024 {
            match s.read(&mut chunk).await {
                Ok(0) => break,
                Ok(m) => buf.extend_from_slice(&chunk[..m]),
                Err(_) if !buf.is_empty() => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(buf)
    }
    if url.https {
        let cfg = std::sync::Arc::new(rv_tls::ClientConfig {
            server_name: url.host.clone(),
            alpn: vec![b"http/1.1".to_vec()],
            alpn_override: true,
            fingerprint: rv_tls::Fingerprint::Chrome,
            verify: rv_tls::Verify::WebPki,
        });
        let mut t = Box::pin(rv_tls::connect(s, cfg)).await?;
        all(&mut t, req.as_bytes()).await
    } else {
        let mut s = s;
        all(&mut s, req.as_bytes()).await
    }
}

fn country_code(resp: &[u8]) -> Option<String> {
    if !resp.starts_with(b"HTTP/") || resp.get(9..12) != Some(b"200") {
        return None;
    }
    let text = std::str::from_utf8(resp).ok()?;
    let body = &text[text.find("\r\n\r\n")? + 4..];
    let json = &body[body.find('{')?..=body.rfind('}')?];
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("success").and_then(|s| s.as_bool()) == Some(false) {
        return None;
    }
    let code = v.get("country_code")?.as_str()?.to_ascii_uppercase();
    (code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase())).then_some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_url_parse() {
        assert_eq!(ProbeUrl::parse(""), Some(ProbeUrl::default()));
        let u = ProbeUrl::parse("https://cp.cloudflare.com/generate_204").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.path.as_str(), u.strict), (true, "cp.cloudflare.com", 443, "/generate_204", false));
        let u = ProbeUrl::parse("http://1.1.1.1:8080").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.path.as_str()), (false, "1.1.1.1", 8080, "/"));
        assert_eq!(ProbeUrl::parse("ftp://x"), None);
        let u = ProbeUrl::parse("http://[::1]?probe=1#local").unwrap();
        assert_eq!((&*u.host, u.port, &*u.path, u.authority()), ("::1", 80, "/?probe=1", "[::1]".into()));
        let u = ProbeUrl::parse("https://[::1]:8443/probe").unwrap();
        assert_eq!(u.authority(), "[::1]:8443");
        assert_eq!(ProbeUrl::parse("http://[::1"), None);
        let u = ProbeUrl::parse("https://www.google.com/#body=32768&within=3000").unwrap();
        assert_eq!((&*u.path, u.body, u.within), ("/", 32768, Duration::from_millis(3000)));
        assert_eq!(ProbeUrl::parse("https://x/#local").unwrap().body, 0);
        let next = u.follow("https://consent.google.com/ml?continue=x#frag").unwrap();
        assert_eq!(
            (next.https, &*next.host, &*next.path, next.body, next.within),
            (true, "consent.google.com", "/ml?continue=x", 32768, u.within)
        );
        assert_eq!(u.follow("/?gws_rd=ssl").unwrap().authority(), "www.google.com");
        assert_eq!(u.follow("//www.google.de/").unwrap().host, "www.google.de");
        assert_eq!(u.follow("relative.html"), None);
    }

    #[test]
    fn probe_url_https_fallback_only_for_default() {
        let f = ProbeUrl::default().https_fallback().unwrap();
        assert_eq!((f.https, f.host.as_str(), f.port, f.path.as_str(), f.strict), (true, "www.gstatic.com", 443, "/generate_204", true));
        assert!(ProbeUrl::parse("http://cp.cloudflare.com/generate_204").unwrap().https_fallback().is_none());
    }

    #[test]
    fn probe_url_several_addresses() {
        let u = ProbeUrl::parse("https://discord.com/, https://t.me/x;http://1.1.1.1\nhttps://a.example/#body=10").unwrap();
        assert_eq!(u.host, "discord.com");
        assert_eq!(u.alt.iter().map(|a| a.host.as_str()).collect::<Vec<_>>(), ["t.me", "1.1.1.1", "a.example"]);
        assert_eq!(u.alt[2].body, 10);
        assert_eq!(ProbeUrl::parse("https://x.example ftp://bad"), None);
        assert!(ProbeUrl::parse("https://x.example").unwrap().alt.is_empty());
    }

    #[test]
    fn country_code_from_answer() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"success\":true,\"country_code\":\"nl\"}";
        assert_eq!(country_code(ok).as_deref(), Some("NL"));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n24\r\n{\"success\":true,\"country_code\":\"DE\"}\r\n0\r\n\r\n";
        assert_eq!(country_code(chunked).as_deref(), Some("DE"));
        assert_eq!(country_code(b"HTTP/1.1 200 OK\r\n\r\n{\"success\":false,\"message\":\"Reserved range\"}"), None);
        assert_eq!(country_code(b"HTTP/1.1 429 Too Many Requests\r\n\r\n{\"country_code\":\"DE\"}"), None);
        assert_eq!(country_code(b"HTTP/1.1 200 OK\r\n\r\n{\"success\":true,\"country_code\":\"\"}"), None);
    }

    type Dial = std::pin::Pin<Box<dyn std::future::Future<Output = Result<crate::BoxStream, Error>>>>;

    fn dial(script: impl Fn(&ProbeUrl) -> Vec<(Duration, Vec<u8>)>) -> impl FnMut(&ProbeUrl) -> Dial {
        move |u| {
            let steps = script(u);
            Box::pin(async move {
                let (app, mut server) = tokio::io::duplex(256 * 1024);
                tokio::spawn(async move {
                    let mut b = [0u8; 1024];
                    let _ = server.read(&mut b).await;
                    for (pause, bytes) in steps {
                        tokio::time::sleep(pause).await;
                        if bytes.is_empty() || server.write_all(&bytes).await.is_err() {
                            return;
                        }
                    }
                    std::future::pending::<()>().await;
                });
                Ok(Box::new(app) as crate::BoxStream)
            })
        }
    }

    fn head(status: &str, extra: &str) -> Vec<u8> {
        format!("HTTP/1.1 {status}\r\nContent-Type: text/html\r\n{extra}\r\n").into_bytes()
    }

    #[tokio::test(start_paused = true)]
    async fn body_check_tells_squeezed_from_broken() {
        let url = ProbeUrl::parse("http://example.com/#body=32768&within=3000").unwrap();
        let ms = Duration::from_millis;
        let slow = |e: &Error| e.to_string().contains(SLOW);
        assert!(body_probe(dial(|_| vec![(ms(0), head("200 OK", "")), (ms(50), vec![b'x'; 40_000])]), &url).await.is_ok());

        let trickle: Vec<_> = std::iter::once((ms(0), head("200 OK", ""))).chain((0..40).map(|_| (ms(500), vec![b'x'; 4096]))).collect();
        let t0 = tokio::time::Instant::now();
        let e = body_probe(dial(move |_| trickle.clone()), &url).await.unwrap_err();
        assert!(slow(&e), "{e}");
        assert!(t0.elapsed() <= ms(3050), "fails at the deadline, not after the page: {:?}", t0.elapsed());
        let text = crate::explain::explain(&e);
        assert!(text.starts_with("Слишком медленно") || text.starts_with("Too slow"), "{text}");

        let short =
            body_probe(dial(|_| vec![(ms(0), head("200 OK", "")), (ms(0), vec![b'x'; 8192]), (ms(0), vec![])]), &url).await.unwrap_err();
        assert!(!slow(&short), "{short}");
        let refused = body_probe(dial(|_| vec![(ms(0), head("429 Too Many Requests", ""))]), &url).await.unwrap_err();
        assert!(!slow(&refused), "{refused}");
        let silent = body_probe(dial(|_| vec![]), &url).await.unwrap_err();
        assert!(!slow(&silent) && matches!(&silent, Error::Io(e) if e.kind() == std::io::ErrorKind::TimedOut), "{silent}");
    }

    #[tokio::test(start_paused = true)]
    async fn body_check_follows_redirects() {
        let url = ProbeUrl::parse("http://example.com/#body=1000&within=3000").unwrap();
        let visited = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = visited.clone();
        let r = body_probe(
            dial(move |u| {
                log.lock().unwrap().push(format!("{}{}", u.host, u.path));
                if u.host == "example.com" {
                    vec![(Duration::ZERO, head("302 Found", "Location: http://consent.example/ok?x=1\r\n"))]
                } else {
                    vec![(Duration::ZERO, head("200 OK", "")), (Duration::ZERO, vec![b'x'; 2000])]
                }
            }),
            &url,
        )
        .await;
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(*visited.lock().unwrap(), ["example.com/", "consent.example/ok?x=1"]);
        let e = body_probe(dial(|_| vec![(Duration::ZERO, head("302 Found", "Location: /again\r\n"))]), &url).await.unwrap_err();
        assert!(!e.to_string().contains(SLOW), "{e}");
    }

    #[tokio::test]
    async fn direct_times_the_page_from_the_answer() {
        async fn site(script: Vec<(u64, Vec<u8>)>) -> u16 {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = l.local_addr().unwrap().port();
            tokio::spawn(async move {
                let (mut s, _) = l.accept().await.unwrap();
                let mut b = [0u8; 1024];
                let _ = s.read(&mut b).await;
                for (pause, bytes) in script {
                    tokio::time::sleep(Duration::from_millis(pause)).await;
                    if bytes.is_empty() || s.write_all(&bytes).await.is_err() {
                        return;
                    }
                }
                std::future::pending::<()>().await;
            });
            port
        }
        let url = |port: u16, opts: &str| ProbeUrl::parse(&format!("http://127.0.0.1:{port}/{opts}")).unwrap();
        let t = Duration::from_secs(3);
        let p = site(vec![(0, head("403 Forbidden", ""))]).await;
        assert!(direct(&url(p, ""), None, t).await.is_ok());
        let p = site(vec![(400, head("200 OK", "")), (0, vec![b'x'; 2000])]).await;
        assert!(direct(&url(p, "#body=2000&within=300"), None, t).await.is_ok());
        let trickle = vec![(0, head("200 OK", "")), (200, vec![b'x'; 500]), (200, vec![b'x'; 500]), (200, vec![b'x'; 500])];
        let e = direct(&url(site(trickle).await, "#body=2000&within=300"), None, t).await.unwrap_err();
        assert!(e.to_string().contains(SLOW), "{e}");
        let short = vec![(0, head("200 OK", "Content-Length: 5\r\n")), (0, b"hello".to_vec()), (0, vec![])];
        assert!(direct(&url(site(short).await, "#body=2000&within=300"), None, t).await.is_ok());
        let cut = vec![(0, head("200 OK", "Content-Length: 5000\r\n")), (0, vec![b'x'; 100]), (0, vec![])];
        assert!(direct(&url(site(cut).await, "#body=2000&within=300"), None, t).await.is_err());
        let silent = site(vec![]).await;
        assert!(direct(&url(silent, ""), None, Duration::from_millis(300)).await.is_err());
    }

    #[tokio::test]
    async fn short_status_line_is_an_error() {
        for reply in [&b"HTTP/"[..], b"HTTP/1", b"HTTP/1.1", b"HTTP/1.1 2"] {
            let (mut app, mut server) = tokio::io::duplex(256);
            let reply = reply.to_vec();
            let srv = tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut b = [0u8; 256];
                let _ = server.read(&mut b).await;
                server.write_all(&reply).await.unwrap();
            });
            assert!(status(&mut app, b"HEAD / HTTP/1.1\r\n\r\n", false).await.is_err());
            srv.await.unwrap();
        }
    }

    #[tokio::test]
    async fn stream_reports_each_once() {
        let links: String =
            (0..6).map(|i| format!("vless://9205c503-358f-4d8d-a081-20c98eddf64a@127.0.0.1:1?type=tcp&security=none#p{i}\n")).collect();
        let profiles = rv_config::parse_subscription(&links).unwrap();
        let mut seen = Vec::new();
        let opts = Opts { parallel: 2, timeout: Duration::from_secs(2), ..Default::default() };
        let out = ping_stream(profiles, Mode::Tcp, None, opts, |i, r| {
            assert!(r.is_err());
            seen.push(i);
        })
        .await;
        seen.sort_unstable();
        assert_eq!(seen, (0..6).collect::<Vec<_>>());
        assert_eq!(out.len(), 6);
        assert!(out.iter().all(|r| r.as_ref().is_err_and(|e| !e.is_empty())));
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_at_once() {
        static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        let links: String =
            (0..4).map(|i| format!("vless://9205c503-358f-4d8d-a081-20c98eddf64a@192.0.2.1:9?type=tcp&security=none#p{i}\n")).collect();
        let profiles = rv_config::parse_subscription(&links).unwrap();
        let opts = Opts { parallel: 1, timeout: Duration::from_secs(60), cancel: Some(&STOP), ..Default::default() };
        let out = tokio::time::timeout(Duration::from_secs(5), ping_stream(profiles, Mode::Tcp, None, opts, |_, _| {})).await.unwrap();
        assert!(out.iter().all(|r| r.as_ref().is_err_and(|e| e == CANCELLED)));
    }
}
