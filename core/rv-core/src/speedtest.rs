use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rv_config::Profile;
use rv_net::Protect;
use rv_vless::Address;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{BoxStream, Error, Proxy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Servers = 0,
    Ping = 1,
    Download = 2,
    Upload = 3,
}

#[derive(Clone, Debug)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub country: String,
    pub sponsor: String,
    pub host: String,
    pub distance: f64,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub server: Server,
    pub via: String,
    pub ping_ms: f64,
    pub jitter_ms: f64,
    pub download_bps: f64,
    pub upload_bps: f64,
}

impl Server {
    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "name": self.name, "country": self.country, "sponsor": self.sponsor, "host": self.host, "distance": self.distance })
    }
}

impl Report {
    pub fn to_json(&self) -> Value {
        json!({
            "server": {
                "id": self.server.id, "name": self.server.name, "country": self.server.country,
                "sponsor": self.server.sponsor, "host": self.server.host, "distance": self.server.distance,
            },
            "via": self.via,
            "ping": self.ping_ms,
            "jitter": self.jitter_ms,
            "download": self.download_bps,
            "upload": self.upload_bps,
        })
    }
}

const PHASE: Duration = Duration::from_secs(10);
const WARMUP: Duration = Duration::from_millis(2000);
const CONNS: usize = 6;
const UA: &str = "Mozilla/5.0 (Linux; Android 15) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36";
const PING_PHASE: Duration = Duration::from_secs(15);
const STUCK: Duration = Duration::from_secs(3);

fn cancelled() -> Error {
    Error::Io(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"))
}

fn timed_out(what: &str) -> Error {
    Error::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("speedtest: {what} timed out")))
}

fn other(msg: impl Into<String>) -> Error {
    Error::Io(std::io::Error::other(msg.into()))
}

pub async fn run(
    profile: &Profile,
    protect: Option<Protect>,
    cancel: &AtomicBool,
    on: impl FnMut(Phase, f64, f64),
    chosen: impl FnMut(&Server, &str),
) -> Result<Report, Error> {
    tokio::task::LocalSet::new().run_until(run_local(profile, protect, cancel, on, chosen)).await
}

async fn run_local(
    profile: &Profile,
    protect: Option<Protect>,
    cancel: &AtomicBool,
    mut on: impl FnMut(Phase, f64, f64),
    mut chosen: impl FnMut(&Server, &str),
) -> Result<Report, Error> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    on(Phase::Servers, 0.0, 0.0);
    let (via, proxy) = pick_outbound(profile, protect).await?;
    let servers = servers(&proxy).await?;
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let near: Vec<Server> = servers.into_iter().take(5).collect();
    let probes = near.iter().map(|s| async {
        let mut c = Conn::open(&proxy, &s.host).await?;
        let mut best = f64::MAX;
        for _ in 0..3 {
            best = best.min(c.latency().await?);
        }
        Ok::<_, Error>(best)
    });
    let lat = join_all(
        probes.map(|f| async { tokio::time::timeout(Duration::from_secs(6), f).await.map_err(|_| other("timeout")).and_then(|r| r) }),
    )
    .await;
    let (server, _) = near
        .into_iter()
        .zip(lat)
        .filter_map(|(s, l)| l.ok().map(|l| (s, l)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .ok_or_else(|| other("speedtest servers do not answer"))?;
    chosen(&server, &via);
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }

    on(Phase::Ping, 0.0, 0.0);
    let (ping_ms, jitter_ms) = ping_phase(Conn::open(&proxy, &server.host), cancel, &mut on).await?;

    let download_bps = transfer(&proxy, &server.host, false, cancel, |v, f| on(Phase::Download, v, f.min(0.999))).await?;
    on(Phase::Download, download_bps, 1.0);
    let upload_bps = transfer(&proxy, &server.host, true, cancel, |v, f| on(Phase::Upload, v, f.min(0.999))).await?;
    on(Phase::Upload, upload_bps, 1.0);
    Ok(Report { server, via, ping_ms, jitter_ms, download_bps, upload_bps })
}

async fn ping_phase(
    open: impl Future<Output = Result<Conn, Error>>,
    cancel: &AtomicBool,
    on: &mut impl FnMut(Phase, f64, f64),
) -> Result<(f64, f64), Error> {
    let phase = async {
        let mut c = open.await?;
        c.latency().await?;
        let mut samples = Vec::new();
        for i in 0..10 {
            if cancel.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            let ms = c.latency().await?;
            samples.push(ms);
            on(Phase::Ping, ms, (i + 1) as f64 / 10.0);
        }
        let ping_ms = samples.iter().copied().fold(f64::MAX, f64::min);
        let jitter_ms = samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (samples.len() - 1) as f64;
        Ok((ping_ms, jitter_ms))
    };
    tokio::time::timeout(PING_PHASE, phase).await.map_err(|_| timed_out("ping"))?
}

async fn pick_outbound(profile: &Profile, protect: Option<Protect>) -> Result<(String, Proxy), Error> {
    let members = profile.balancer_members();
    if members.len() < 2 {
        let main = profile.main_outbound().ok_or_else(|| Error::Config("profile has no proxy outbound".into()))?;
        return Ok((main.tag.clone(), Proxy::build(profile, &main.tag, protect)?));
    }
    let runs = members.iter().map(|ob| {
        let protect = protect.clone();
        async move {
            let p = Proxy::build(profile, &ob.tag, protect)?;
            crate::ping::probe(&p).await.ok_or_else(|| other("no answer"))?;
            Ok::<_, Error>((ob.tag.clone(), p))
        }
    });
    crate::engine::first_ok(runs).await.unwrap_or_else(|| Err(other("no servers")))
}

async fn servers(proxy: &Proxy) -> Result<Vec<Server>, Error> {
    let body = tokio::time::timeout(
        Duration::from_secs(15),
        https_get(proxy, "www.speedtest.net", "/api/js/servers?engine=js&limit=10&https_functional=true"),
    )
    .await
    .map_err(|_| other("speedtest.net: timeout"))??;
    let v: Value = serde_json::from_slice(&body).map_err(|e| other(format!("speedtest.net: {e}")))?;
    let s = |o: &Value, k: &str| match o.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let list: Vec<Server> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| !s(o, "host").is_empty())
        .map(|o| Server {
            id: s(o, "id"),
            name: s(o, "name"),
            country: s(o, "country"),
            sponsor: s(o, "sponsor"),
            host: s(o, "host"),
            distance: o.get("distance").and_then(Value::as_f64).unwrap_or(0.0),
        })
        .collect();
    if list.is_empty() {
        return Err(other("speedtest.net: no servers"));
    }
    Ok(list)
}

async fn https_get(proxy: &Proxy, host: &str, path: &str) -> Result<Vec<u8>, Error> {
    let s = proxy.connect(Address::Domain(host.into()), 443).await?;
    let cfg = std::sync::Arc::new(rv_tls::ClientConfig {
        server_name: host.into(),
        alpn: vec![b"http/1.1".to_vec()],
        alpn_override: true,
        fingerprint: rv_tls::Fingerprint::Chrome,
        verify: rv_tls::Verify::WebPki,
    });
    let mut t = rv_tls::connect(s, cfg).await?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {UA}\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
    );
    t.write_all(req.as_bytes()).await?;
    t.flush().await?;
    let mut buf = Vec::new();
    let _ = t.read_to_end(&mut buf).await;
    let end = find(&buf, b"\r\n\r\n").ok_or_else(|| other("no http response"))?;
    let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
    let code = status(head.as_bytes());
    if !(200..300).contains(&code) {
        return Err(other(format!("{host}: http {code}")));
    }
    let body = &buf[end + 4..];
    Ok(if head.contains("transfer-encoding: chunked") { dechunk(body) } else { body.to_vec() })
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn status(head: &[u8]) -> u16 {
    head.get(9..12).and_then(|c| std::str::from_utf8(c).ok()).and_then(|c| c.parse().ok()).unwrap_or(0)
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(i) = find(b, b"\r\n") {
        let size = std::str::from_utf8(&b[..i]).ok().and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok());
        let Some(n) = size.filter(|&n| n > 0) else { break };
        let start = i + 2;
        let end = start.saturating_add(n).min(b.len());
        out.extend_from_slice(&b[start..end]);
        b = b.get(end + 2..).unwrap_or_default();
    }
    out
}

struct Conn {
    s: BoxStream,
    host: String,
    buf: Vec<u8>,
}

impl Conn {
    async fn open(proxy: &Proxy, hostport: &str) -> Result<Conn, Error> {
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().unwrap_or(8080)),
            None => (hostport.to_string(), 8080),
        };
        let s = proxy.connect(Address::Domain(host.clone()), port).await?;
        Ok(Conn { s, host: hostport.to_string(), buf: Vec::new() })
    }

    async fn request(&mut self, method: &str, path: &str, body_len: usize) -> Result<(), Error> {
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {UA}\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: keep-alive\r\n",
            self.host
        );
        if method == "POST" {
            req.push_str(&format!("Content-Type: application/octet-stream\r\nContent-Length: {body_len}\r\n"));
        }
        req.push_str("\r\n");
        self.s.write_all(req.as_bytes()).await?;
        if body_len == 0 {
            self.s.flush().await?;
        }
        Ok(())
    }

    async fn response(&mut self) -> Result<(u16, Option<u64>), Error> {
        let mut chunk = [0u8; 4096];
        let end = loop {
            if let Some(i) = find(&self.buf, b"\r\n\r\n") {
                break i;
            }
            if self.buf.len() > 32 * 1024 {
                return Err(other("bad http response"));
            }
            let n = self.s.read(&mut chunk).await?;
            if n == 0 {
                return Err(other("connection closed"));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&self.buf[..end]).to_ascii_lowercase();
        let code = status(head.as_bytes());
        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok());
        self.buf.drain(..end + 4);
        Ok((code, len))
    }

    async fn skip(&mut self, mut n: u64, count: &dyn Fn(u64)) -> Result<(), Error> {
        let have = (self.buf.len() as u64).min(n);
        self.buf.drain(..have as usize);
        count(have);
        n -= have;
        let mut chunk = vec![0u8; 64 * 1024];
        while n > 0 {
            let want = n.min(chunk.len() as u64) as usize;
            let m = self.s.read(&mut chunk[..want]).await?;
            if m == 0 {
                return Err(other("connection closed"));
            }
            count(m as u64);
            n -= m as u64;
        }
        Ok(())
    }

    async fn latency(&mut self) -> Result<f64, Error> {
        let start = Instant::now();
        self.request("GET", &format!("/speedtest/latency.txt?r={}", nonce()), 0).await?;
        let (code, len) = self.response().await?;
        if code != 200 {
            return Err(other(format!("latency: http {code}")));
        }
        self.skip(len.unwrap_or(0), &|_| {}).await?;
        Ok(start.elapsed().as_secs_f64() * 1000.0)
    }
}

fn nonce() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

async fn transfer(proxy: &Proxy, host: &str, upload: bool, cancel: &AtomicBool, mut on: impl FnMut(f64, f64)) -> Result<f64, Error> {
    let bytes = Cell::new(0u64);
    let errors = Cell::new(0usize);
    let last_error: Cell<Option<String>> = Cell::new(None);
    let start = Instant::now();
    let deadline = start + PHASE;
    let stop = || cancel.load(Ordering::Relaxed) || Instant::now() >= deadline;
    let count = |n: u64| bytes.set(bytes.get() + n);
    let worker = |i: usize| {
        let (stop, count, errors, last_error) = (&stop, &count, &errors, &last_error);
        async move {
            tokio::time::sleep(Duration::from_millis(i as u64 * 60)).await;
            let work =
                async { if upload { up_worker(proxy, host, stop, count).await } else { down_worker(proxy, host, stop, count).await } };
            let r = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline + STUCK), work)
                .await
                .unwrap_or_else(|_| Err(timed_out(if upload { "upload" } else { "download" })));
            if let Err(e) = r {
                errors.set(errors.get() + 1);
                last_error.set(Some(e.to_string()));
            }
        }
    };
    let workers: Vec<Pin<Box<dyn Future<Output = ()> + '_>>> = (0..CONNS).map(|i| Box::pin(worker(i)) as _).collect();
    let mut at_warmup: Option<(Instant, u64)> = None;
    let mut window: std::collections::VecDeque<(Instant, u64)> = Default::default();
    let report = async {
        loop {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let now = Instant::now();
            let b = bytes.get();
            if at_warmup.is_none() && now - start >= WARMUP {
                at_warmup = Some((now, b));
            }
            window.push_back((now, b));
            while window.front().is_some_and(|(t, _)| now - *t > Duration::from_millis(1500)) {
                window.pop_front();
            }
            let (t0, b0) = window.front().copied().unwrap_or((start, 0));
            let dt = (now - t0).as_secs_f64();
            let bps = if dt > 0.05 { (b - b0) as f64 * 8.0 / dt } else { 0.0 };
            on(bps, ((now - start).as_secs_f64() / PHASE.as_secs_f64()).min(1.0));
            if stop() {
                break;
            }
        }
    };
    tokio::join!(join_all(workers.into_iter()), report);
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let end = Instant::now();
    let total = bytes.get();
    if total == 0 {
        return Err(other(last_error.take().unwrap_or_else(|| "no data".into())));
    }
    let (t0, b0) = at_warmup.unwrap_or((start, 0));
    let dt = (end - t0).as_secs_f64().max(0.001);
    Ok((total - b0) as f64 * 8.0 / dt)
}

async fn down_worker(proxy: &Proxy, host: &str, stop: &dyn Fn() -> bool, count: &dyn Fn(u64)) -> Result<(), Error> {
    let mut c = Conn::open(proxy, host).await?;
    let mut legacy = false;
    while !stop() {
        let path = if legacy {
            format!("/speedtest/random4000x4000.jpg?r={}", nonce())
        } else {
            format!("/download?nocache={}&size=25000000", nonce())
        };
        c.request("GET", &path, 0).await?;
        let (code, len) = c.response().await?;
        if code != 200 {
            if legacy {
                return Err(other(format!("download: http {code}")));
            }
            legacy = true;
            c = Conn::open(proxy, host).await?;
            continue;
        }
        let len = len.unwrap_or(25_000_000);
        tokio::select! {
            r = c.skip(len, count) => r?,
            _ = wait_until(stop) => return Ok(()),
        }
    }
    Ok(())
}

async fn up_worker(proxy: &Proxy, host: &str, stop: &dyn Fn() -> bool, count: &dyn Fn(u64)) -> Result<(), Error> {
    static BLOCK: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let block = BLOCK.get_or_init(|| {
        let mut b = vec![0u8; 64 * 1024];
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for v in b.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *v = x as u8;
        }
        b
    });
    let mut c = Conn::open(proxy, host).await?;
    let mut size: usize = 256 * 1024;
    let mut legacy = false;
    while !stop() {
        let path = if legacy { format!("/speedtest/upload.php?r={}", nonce()) } else { format!("/upload?nocache={}", nonce()) };
        let t = Instant::now();
        let send = async {
            c.request("POST", &path, size).await?;
            let mut left = size;
            while left > 0 {
                let n = left.min(block.len());
                c.s.write_all(&block[..n]).await?;
                left -= n;
            }
            c.s.flush().await?;
            c.response().await
        };
        let (code, len) = tokio::select! {
            r = send => r?,
            _ = wait_until(stop) => return Ok(()),
        };
        if !(200..300).contains(&code) {
            if legacy {
                return Err(other(format!("upload: http {code}")));
            }
            legacy = true;
            c = Conn::open(proxy, host).await?;
            continue;
        }
        c.skip(len.unwrap_or(0), &|_| {}).await?;
        count(size as u64);
        if t.elapsed() < Duration::from_millis(500) {
            size = (size * 2).min(16 << 20);
        }
    }
    Ok(())
}

async fn wait_until(stop: &dyn Fn() -> bool) {
    while !stop() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn join_all<T, F: Future<Output = T>>(futs: impl Iterator<Item = F>) -> Vec<T> {
    let mut futs: Vec<Pin<Box<F>>> = futs.map(Box::pin).collect();
    let mut out: Vec<Option<T>> = futs.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (i, f) in futs.iter_mut().enumerate() {
            if out[i].is_some() {
                continue;
            }
            match f.as_mut().poll(cx) {
                std::task::Poll::Ready(v) => out[i] = Some(v),
                std::task::Poll::Pending => pending = true,
            }
        }
        if pending { std::task::Poll::Pending } else { std::task::Poll::Ready(()) }
    })
    .await;
    out.into_iter().map(|v| v.expect("ready")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dechunk_body() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n6\r\npedia \r\n0\r\n\r\n"), b"Wikipedia ");
    }

    #[test]
    fn dechunk_survives_huge_chunk_sizes() {
        assert_eq!(dechunk(b"ffffffffffffffff\r\nabc"), b"abc");
        assert_eq!(dechunk(b"ffffffff\r\nabc\r\n0\r\n\r\n"), b"abc\r\n0\r\n\r\n");
        assert_eq!(dechunk(b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"), b"abcde");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn content_length_past_usize_reads_the_body() {
        use tokio::io::AsyncWriteExt;
        let (a, mut b) = tokio::io::duplex(64);
        let mut c = Conn { s: Box::new(a), host: "h".into(), buf: Vec::new() };
        b.write_all(b"0123456789").await.unwrap();
        drop(b);
        let seen = std::cell::Cell::new(0u64);
        assert!(c.skip(1 << 32, &|n| seen.set(seen.get() + n)).await.is_err());
        assert_eq!(seen.get(), 10);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn stuck_ping_ends_with_a_timeout() {
        let timed_out = |e: &Error| matches!(e, Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut);
        let cancel = AtomicBool::new(false);
        let (a, _peer) = tokio::io::duplex(64);
        let start = tokio::time::Instant::now();
        let silent = async { Ok(Conn { s: Box::new(a), host: "h".into(), buf: Vec::new() }) };
        let e = ping_phase(silent, &cancel, &mut |_, _, _| {}).await.expect_err("ping hung");
        assert!(timed_out(&e), "{e}");
        assert_eq!(start.elapsed(), PING_PHASE);
        let e = ping_phase(std::future::pending::<Result<Conn, Error>>(), &cancel, &mut |_, _, _| {}).await.expect_err("open hung");
        assert!(timed_out(&e), "{e}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_before_start_does_not_dial() {
        use std::sync::atomic::AtomicUsize;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let accepted = std::sync::Arc::new(AtomicUsize::new(0));
        let a = accepted.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = l.accept().await {
                a.fetch_add(1, Ordering::SeqCst);
                drop(s);
            }
        });
        let sub = serde_json::json!({"outbounds":[{"protocol":"vless","tag":"proxy",
            "settings":{"address":"127.0.0.1","port":port,"id":"9205c503-358f-4d8d-a081-20c98eddf64a","encryption":"none"},
            "streamSettings":{"network":"tcp"}}]});
        let p = rv_config::parse_subscription(&sub.to_string()).unwrap().remove(0);
        let cancel = AtomicBool::new(true);
        let e = run(&p, None, &cancel, |_, _, _| {}, |_, _| {}).await.expect_err("замер отменён");
        assert!(matches!(&e, Error::Io(io) if io.kind() == std::io::ErrorKind::Interrupted), "{e}");
        tokio::task::yield_now().await;
        assert_eq!(accepted.load(Ordering::SeqCst), 0, "отменённый замер не звонит прокси");
    }
}
