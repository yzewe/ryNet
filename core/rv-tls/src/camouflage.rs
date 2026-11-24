use crate::{ClientConfig, Fingerprint, TlsStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::{io, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const LIMIT: usize = 512 * 1024;
const HEAD_LIMIT: usize = 16 * 1024;
fn random() -> u64 {
    use ring::rand::SecureRandom;
    let mut bytes = [0; 8];
    let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
    u64::from_ne_bytes(bytes)
}
fn sample(value: &str, cap: u64) -> Option<u64> {
    let (a, b) = value.split_once('-').unwrap_or((value, value));
    let (a, b) = (a.parse::<u64>().ok()?.min(cap), b.parse::<u64>().ok()?.min(cap));
    (a <= b).then(|| a + random() % (b - a + 1))
}
struct Options {
    path: String,
    padding: usize,
    requests: usize,
    interval: Duration,
    hold: Duration,
}
impl Options {
    fn new(path: &str) -> Self {
        let mut o = Self {
            path: "/".into(),
            padding: 30 + (random() % 256) as usize,
            requests: 3,
            interval: Duration::from_millis(100 + random() % 300),
            hold: Duration::from_millis(5000 + random() % 9000),
        };
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['\r', '\n', '#']) {
            return o;
        }
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        o.path = path.into();
        let (mut concurrency, mut times) = (1, 2);
        let mut keep = Vec::new();
        for param in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = param.split_once('=').unwrap_or((param, ""));
            match k {
                "p" => {
                    if let Some(n) = sample(v, 4096) {
                        o.padding = n as usize;
                    }
                }
                "c" => {
                    if let Some(n) = sample(v, 5) {
                        concurrency = n as usize;
                    }
                }
                "t" => {
                    if let Some(n) = sample(v, 5) {
                        times = n as usize;
                    }
                }
                "i" => {
                    if let Some(n) = sample(v, 5000) {
                        o.interval = Duration::from_millis(n);
                    }
                }
                "r" => {
                    if let Some(n) = sample(v, 15000) {
                        o.hold = Duration::from_millis(n.max(100));
                    }
                }
                _ => keep.push(param),
            }
        }
        o.requests = (1 + concurrency * times).min(6);
        if !keep.is_empty() {
            o.path.push('?');
            o.path.push_str(&keep.join("&"));
        }
        o
    }
}
fn user_agent(fp: Fingerprint) -> &'static str {
    match fp {
        Fingerprint::Chrome | Fingerprint::Chrome120 | Fingerprint::Chrome131 => {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
        }
        Fingerprint::Firefox
        | Fingerprint::Firefox120
        | Fingerprint::Firefox55
        | Fingerprint::Firefox56
        | Fingerprint::Firefox63
        | Fingerprint::Firefox65
        | Fingerprint::Firefox99
        | Fingerprint::Firefox102
        | Fingerprint::Firefox105 => "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0",
        Fingerprint::Safari | Fingerprint::Safari16 => {
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.3 Safari/605.1.15"
        }
        Fingerprint::Ios | Fingerprint::Ios13 | Fingerprint::Ios11 | Fingerprint::Ios12 => {
            "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1"
        }
        Fingerprint::Edge | Fingerprint::Edge106 => {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/85.0.4183.121 Safari/537.36 Edg/85.0.564.63"
        }
        Fingerprint::Qq | Fingerprint::Qihoo11 => {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/70.0.3538.25 Safari/537.36 QQBrowser/11.1"
        }
        _ => user_agent(Fingerprint::Chrome),
    }
}
fn same_origin_path(link: &str, host: &str, current: &str) -> Option<String> {
    let link = link.split('#').next()?.trim().replace("&amp;", "&");
    if link.is_empty() || link.len() > 2048 || link.contains(['\r', '\n', '\\', ' ']) || link.starts_with("//") {
        return None;
    }
    let path = if link.starts_with("https://") {
        let uri: http::Uri = link.parse().ok()?;
        if !uri.authority()?.as_str().eq_ignore_ascii_case(host) {
            return None;
        }
        uri.path_and_query()?.as_str().into()
    } else if link.starts_with('/') {
        link
    } else {
        if link.contains(':') {
            return None;
        }
        let base = current.split('?').next()?.rsplit_once('/').map_or("", |(p, _)| p);
        format!("{base}/{link}")
    };
    if path.contains('.') || path.starts_with("//") {
        return None;
    }
    let _: http::Uri = path.parse().ok()?;
    Some(path)
}
fn discover(body: &[u8], host: &str, current: &str, paths: &mut Vec<String>) {
    let html = String::from_utf8_lossy(body);
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while paths.len() < 64 {
        let Some(n) = lower[at..].find("href") else {
            break;
        };
        at += n + 4;
        let attr = html[at..].trim_start();
        let Some(attr) = attr.strip_prefix('=').map(str::trim_start) else {
            continue;
        };
        let Some(quote) = attr.chars().next().filter(|c| matches!(c, '\'' | '"')) else {
            continue;
        };
        let Some(end) = attr[1..].find(quote) else {
            continue;
        };
        if let Some(path) = same_origin_path(&attr[1..end + 1], host, current)
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
}
fn decode_body(encoding: &str, raw: Vec<u8>, limit: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let reader: Box<dyn Read + '_> = match encoding.trim() {
        "" | "identity" => return Ok(raw),
        "gzip" => Box::new(flate2::read::GzDecoder::new(raw.as_slice())),
        "deflate" => Box::new(flate2::read::ZlibDecoder::new(raw.as_slice())),
        "br" => Box::new(brotli_decompressor::Decompressor::new(raw.as_slice(), 4096)),
        _ => return Err(io::Error::other("unsupported HTTP content encoding")),
    };
    let mut body = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut body)?;
    if body.len() > limit {
        return Err(io::Error::other("camouflage body limit"));
    }
    Ok(body)
}
async fn h2_fetch(
    send: h2::client::SendRequest<bytes::Bytes>,
    cfg: &ClientConfig,
    path: &str,
    referer: Option<&str>,
    padding: usize,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let mut send = send.ready().await.map_err(io::Error::other)?;
    let mut req = http::Request::builder()
        .uri(format!("https://{}{path}", cfg.server_name))
        .header("user-agent", user_agent(cfg.fingerprint))
        .header("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("accept-encoding", "gzip, deflate, br")
        .header("accept-language", "en-US,en;q=0.9")
        .header("cookie", format!("padding={}", "0".repeat(padding)));
    if let Some(r) = referer {
        req = req.header("referer", r);
    }
    let (response, _) = send.send_request(req.body(()).map_err(io::Error::other)?, true).map_err(io::Error::other)?;
    let response = response.await.map_err(io::Error::other)?;
    let encoding = response.headers().get("content-encoding").and_then(|h| h.to_str().ok()).unwrap_or("").to_owned();
    let mut recv = response.into_body();
    let mut raw = Vec::new();
    while let Some(data) = recv.data().await {
        let data = data.map_err(io::Error::other)?;
        recv.flow_control().release_capacity(data.len()).map_err(io::Error::other)?;
        if raw.len() + data.len() > limit {
            return Err(io::Error::other("camouflage body limit"));
        }
        raw.extend_from_slice(&data);
    }
    decode_body(&encoding, raw, limit)
}
async fn line<S: AsyncRead + Unpin>(io: &mut BufReader<S>) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    io.take(HEAD_LIMIT as u64 + 1).read_until(b'\n', &mut line).await?;
    if line.len() > HEAD_LIMIT || !line.ends_with(b"\r\n") {
        return Err(io::Error::other("bad HTTP response line"));
    }
    Ok(line)
}
async fn h1_fetch<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut BufReader<S>,
    cfg: &ClientConfig,
    path: &str,
    referer: Option<&str>,
    padding: usize,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let mut req = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\nAccept-Encoding: gzip, deflate, br\r\nAccept-Language: en-US,en;q=0.9\r\nCookie: padding={}\r\n",
        cfg.server_name,
        user_agent(cfg.fingerprint),
        "0".repeat(padding)
    );
    if let Some(r) = referer {
        req.push_str(&format!("Referer: {r}\r\n"));
    }
    req.push_str("\r\n");
    io.write_all(req.as_bytes()).await?;
    io.flush().await?;
    let status = line(io).await?;
    if !status.starts_with(b"HTTP/1.") {
        return Err(io::Error::other("bad HTTP status"));
    }
    let mut size = status.len();
    let (mut length, mut chunked, mut encoding) = (None, false, String::new());
    loop {
        let header = line(io).await?;
        size += header.len();
        if size > HEAD_LIMIT {
            return Err(io::Error::other("camouflage header limit"));
        }
        if header == b"\r\n" {
            break;
        }
        let header = std::str::from_utf8(&header).map_err(io::Error::other)?;
        if let Some((name, value)) = header.split_once(':') {
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = Some(value.trim().parse::<usize>().map_err(io::Error::other)?),
                "transfer-encoding" => chunked = value.trim().eq_ignore_ascii_case("chunked"),
                "content-encoding" => encoding = value.trim().to_ascii_lowercase(),
                _ => {}
            }
        }
    }
    let mut raw = Vec::new();
    if chunked {
        loop {
            let chunk = line(io).await?;
            let text = std::str::from_utf8(&chunk).map_err(io::Error::other)?;
            let n = usize::from_str_radix(text.trim().split(';').next().unwrap_or(""), 16).map_err(io::Error::other)?;
            if n == 0 {
                loop {
                    let trailer = line(io).await?;
                    size += trailer.len();
                    if size > HEAD_LIMIT {
                        return Err(io::Error::other("camouflage trailer limit"));
                    }
                    if trailer == b"\r\n" {
                        break;
                    }
                }
                break;
            }
            if n > limit.saturating_sub(raw.len()) {
                return Err(io::Error::other("camouflage body limit"));
            }
            let start = raw.len();
            raw.resize(start + n, 0);
            io.read_exact(&mut raw[start..]).await?;
            if io.read_u16().await? != 0x0d0a {
                return Err(io::Error::other("bad HTTP chunk ending"));
            }
        }
    } else if let Some(n) = length {
        if n > limit {
            return Err(io::Error::other("camouflage body limit"));
        }
        raw.resize(n, 0);
        io.read_exact(&mut raw).await?;
    } else {
        io.take(limit as u64 + 1).read_to_end(&mut raw).await?;
        if raw.len() > limit {
            return Err(io::Error::other("camouflage body limit"));
        }
    }
    decode_body(&encoding, raw, limit)
}
fn pick_path(paths: &[String], n: usize) -> String {
    paths[if n == 1 && paths.len() > 1 { 1 } else { (random() as usize) % paths.len() }].clone()
}
pub(crate) static BACKGROUND: OnceLock<crate::Background> = OnceLock::new();
static CRAWLING: AtomicBool = AtomicBool::new(false);

pub(crate) async fn start<S: AsyncRead + AsyncWrite + Unpin + 'static>(tls: TlsStream<S>, cfg: Arc<ClientConfig>) {
    let Some(spawn) = BACKGROUND.get() else { return run(tls, &cfg).await };
    if CRAWLING.swap(true, Ordering::AcqRel) {
        return;
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            CRAWLING.store(false, Ordering::Release);
        }
    }
    let done = Done;
    spawn(Box::pin(async move {
        let _done = done;
        run(tls, &cfg).await;
    }));
}

async fn run<S: AsyncRead + AsyncWrite + Unpin>(tls: TlsStream<S>, cfg: &ClientConfig) {
    let path = match &cfg.verify {
        crate::Verify::Reality(r) => r.spider_x.as_str(),
        _ => "/",
    };
    let o = Options::new(path);
    let first_url = format!("https://{}{}", cfg.server_name, o.path);
    let request = async {
        let mut paths = vec![o.path.clone()];
        let mut budget = LIMIT;
        if tls.alpn() == Some(b"h2") {
            let (send, conn) = h2::client::Builder::new()
                .enable_push(false)
                .initial_window_size(4 << 20)
                .initial_connection_window_size((1 << 30) + 65535)
                .max_frame_size(1 << 20)
                .max_header_list_size(10 << 20)
                .handshake::<_, bytes::Bytes>(tls)
                .await
                .map_err(io::Error::other)?;
            let crawl = async {
                for n in 0..o.requests {
                    let path = pick_path(&paths, n);
                    let body = h2_fetch(send.clone(), cfg, &path, (n > 0).then_some(first_url.as_str()), o.padding, budget).await?;
                    budget -= body.len();
                    discover(&body, &cfg.server_name, &path, &mut paths);
                    if budget == 0 {
                        break;
                    }
                    tokio::time::sleep(o.interval).await;
                }
                tokio::time::sleep(o.hold).await;
                Ok::<_, io::Error>(())
            };
            tokio::pin!(conn, crawl);
            tokio::select! { r=&mut conn=>r.map_err(io::Error::other),r=&mut crawl=>r }
        } else {
            let mut io = BufReader::new(tls);
            for n in 0..o.requests {
                let path = pick_path(&paths, n);
                let body = h1_fetch(&mut io, cfg, &path, (n > 0).then_some(first_url.as_str()), o.padding, budget).await?;
                budget -= body.len();
                discover(&body, &cfg.server_name, &path, &mut paths);
                if budget == 0 {
                    break;
                }
                tokio::time::sleep(o.interval).await;
            }
            tokio::time::sleep(o.hold).await;
            Ok(())
        }
    };
    let _ = tokio::time::timeout(o.hold, request).await;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_stay_on_the_authenticated_origin() {
        let mut paths = Vec::new();
        discover(br#"<a href='/news'>a</a><a href="https://example.com/about">b</a><a href="https://evil.invalid/steal">c</a><a href="//evil.invalid/steal">d</a><a href="../parent">e</a><a href="img.png">f</a>"#,"example.com","/",&mut paths);
        assert_eq!(paths, vec!["/news", "/about"]);
    }
    #[test]
    fn controls_are_not_sent_as_query_parameters() {
        let o = Options::new("/start?x=1&p=40&c=2&t=3&i=20&r=500&y=2");
        assert_eq!(o.path, "/start?x=1&y=2");
        assert_eq!(o.padding, 40);
        assert_eq!(o.requests, 6);
        assert_eq!(o.interval, Duration::from_millis(20));
        assert_eq!(o.hold, Duration::from_millis(500));
    }
    #[test]
    fn compressed_html_is_bounded() {
        use std::io::Write;
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(&vec![b'a'; 4096]).unwrap();
        let body = gzip.finish().unwrap();
        assert!(decode_body("gzip", body.clone(), 100).is_err());
        assert_eq!(decode_body("gzip", body, 4096).unwrap().len(), 4096);
    }
}
