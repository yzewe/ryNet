use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

use rv_core::engine::Engine;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;

async fn relay<S: AsyncRead + AsyncWrite + Unpin>(engine: &Engine, s: S, src: SocketAddr, dst: SocketAddr) -> io::Result<()> {
    rv_stats::add(rv_stats::Slot::TcpTotal, 1);
    let _active = rv_stats::Active::new(rv_stats::Slot::TcpActive);
    engine.tcp_from(rv_host::Counted(rv_host::Plain(s)), Some(src), dst).await.map_err(|e| {
        rv_host::count_error(&e);
        io::Error::other(e)
    })
}

fn dest(host: &str, port: u16) -> io::Result<SocketAddr> {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    let h = h.strip_suffix('.').unwrap_or(h);
    match h.parse::<IpAddr>() {
        Ok(ip) => Ok(SocketAddr::new(ip, port)),
        Err(_) if rv_core::dns::fake::valid(h) => Ok(SocketAddr::new(IpAddr::V4(rv_core::dns::fake::ip_for(h)), port)),
        Err(_) => Err(io::Error::new(io::ErrorKind::InvalidInput, "proxy: bad host name")),
    }
}

pub async fn handle(engine: &Engine, mut s: TcpStream, src: SocketAddr) -> io::Result<()> {
    s.set_nodelay(true)?;
    let mut first = [0u8; 1];
    if s.peek(&mut first).await? == 0 {
        return Ok(());
    }
    if first[0] == 5 {
        let dst = socks5(&mut s).await?;
        return relay(engine, s, src, dst).await;
    }
    let mut head = Vec::with_capacity(1024);
    let mut buf = [0u8; 4096];
    let end = loop {
        let n = s.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
        if let Some(i) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if head.len() > 64 * 1024 {
            return Err(io::Error::other("http: header too large"));
        }
    };
    let text = String::from_utf8_lossy(&head[..end]).into_owned();
    let mut lines = text.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split_whitespace();
    let (method, target, version) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or("HTTP/1.1"));
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split_host_port(target, 443);
        let dst = dest(&host, port)?;
        s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await?;
        let rest = head[end..].to_vec();
        return relay(engine, Prefixed { prefix: rest, at: 0, inner: s }, src, dst).await;
    }
    let Some(url) = target.strip_prefix("http://") else {
        s.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n").await?;
        return Ok(());
    };
    let (authority, path) = url.split_once('/').map_or((url, "/".to_string()), |(a, p)| (a, format!("/{p}")));
    let (host, port) = split_host_port(authority, 80);
    let mut out = format!("{method} {path} {version}\r\n");
    for l in lines.filter(|l| !l.is_empty()) {
        let name = l.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
        if name.starts_with("proxy-") || name == "connection" || name == "keep-alive" {
            continue;
        }
        out.push_str(l);
        out.push_str("\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    let mut prefix = out.into_bytes();
    prefix.extend_from_slice(&head[end..]);
    relay(engine, Prefixed { prefix, at: 0, inner: s }, src, dest(&host, port)?).await
}

fn split_host_port(s: &str, default: u16) -> (String, u16) {
    if let Some(rest) = s.strip_prefix('[')
        && let Some((h, p)) = rest.split_once(']')
    {
        return (h.to_string(), p.trim_start_matches(':').parse().unwrap_or(default));
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h.to_string(), p.parse().unwrap_or(default)),
        _ => (s.to_string(), default),
    }
}

async fn socks5(s: &mut TcpStream) -> io::Result<SocketAddr> {
    let mut hdr = [0u8; 2];
    s.read_exact(&mut hdr).await?;
    let mut methods = vec![0u8; hdr[1] as usize];
    s.read_exact(&mut methods).await?;
    s.write_all(&[5, 0]).await?;
    let mut req = [0u8; 4];
    s.read_exact(&mut req).await?;
    if req[1] != 1 {
        s.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        return Err(io::Error::other("socks: only CONNECT"));
    }
    let host = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            s.read_exact(&mut a).await?;
            IpAddr::from(a).to_string()
        }
        4 => {
            let mut a = [0u8; 16];
            s.read_exact(&mut a).await?;
            IpAddr::from(a).to_string()
        }
        3 => {
            let mut l = [0u8; 1];
            s.read_exact(&mut l).await?;
            let mut name = vec![0u8; l[0] as usize];
            s.read_exact(&mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        _ => return Err(io::Error::other("socks: bad address")),
    };
    let mut p = [0u8; 2];
    s.read_exact(&mut p).await?;
    let dst = dest(&host, u16::from_be_bytes(p))?;
    s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    Ok(dst)
}

struct Prefixed {
    prefix: Vec<u8>,
    at: usize,
    inner: TcpStream,
}

impl AsyncRead for Prefixed {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if self.at < self.prefix.len() {
            let n = (self.prefix.len() - self.at).min(buf.remaining());
            let at = self.at;
            buf.put_slice(&self.prefix[at..at + n]);
            self.at += n;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Prefixed {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, b)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port() {
        assert_eq!(split_host_port("example.com:8443", 443), ("example.com".into(), 8443));
        assert_eq!(split_host_port("example.com", 443), ("example.com".into(), 443));
        assert_eq!(split_host_port("[2001:db8::1]:80", 443), ("2001:db8::1".into(), 80));
        assert!(dest("1.2.3.4", 80).unwrap().ip().is_ipv4());
        assert!(rv_core::dns::fake::is_fake(dest("example.com", 80).unwrap().ip()));
        assert!(dest("bad host\n7 victim.test", 80).is_err() && dest(&"a".repeat(300), 80).is_err());
    }
}
