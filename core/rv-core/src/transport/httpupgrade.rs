use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, ready};

use rv_xhttp::browser::{self, Headers, Variant};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf, ReadHalf, WriteHalf};

use super::ws::normalized_path;
use crate::BoxStream;

pub struct Settings {
    path: String,
    host: String,
    headers: Headers,
}

impl Settings {
    pub fn new(cfg: &rv_config::HttpUpgrade, default_host: &str) -> Self {
        let mut headers: Headers = cfg.headers.clone();
        browser::apply_defaults(&mut headers, Variant::Ws);
        browser::set(&mut headers, "Connection", "Upgrade");
        browser::set(&mut headers, "Upgrade", "websocket");
        Self {
            path: normalized_path(&cfg.path),
            host: if cfg.host.is_empty() { default_host.to_string() } else { cfg.host.clone() },
            headers,
        }
    }
}

pub struct Upgraded {
    rd: ReadHalf<BoxStream>,
    wr: WriteHalf<BoxStream>,
    head: Option<Vec<u8>>,
    leftover: Vec<u8>,
}

pub async fn open(s: Rc<Settings>, mut io: BoxStream) -> io::Result<Upgraded> {
    io.write_all(&browser::http1_request("GET", &s.path, &s.host, &s.headers)).await?;
    io.flush().await?;
    let (rd, wr) = tokio::io::split(io);
    let mut up = Upgraded { rd, wr, head: Some(Vec::new()), leftover: Vec::new() };
    std::future::poll_fn(|cx| up.poll_head(cx)).await?;
    Ok(up)
}

fn check(head: &[u8]) -> io::Result<()> {
    let head = String::from_utf8_lossy(head);
    let status = head.split("\r\n").next().unwrap_or_default();
    let hdr = |name: &str| {
        head.split("\r\n").skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_ascii_lowercase())
        })
    };
    if status.split_once(' ').map(|(_, s)| s) != Some("101 Switching Protocols")
        || hdr("Upgrade").as_deref() != Some("websocket")
        || hdr("Connection").as_deref() != Some("upgrade")
    {
        return Err(io::Error::other(format!("httpupgrade: unrecognized reply `{status}`")));
    }
    Ok(())
}

impl Upgraded {
    fn poll_head(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while let Some(head) = &mut self.head {
            if let Some(p) = head.windows(4).position(|w| w == b"\r\n\r\n") {
                let rest = head.split_off(p + 4);
                check(head)?;
                self.leftover = rest;
                self.head = None;
                break;
            }
            if head.len() > 16 * 1024 {
                return Poll::Ready(Err(io::Error::other("httpupgrade: header too long")));
            }
            let mut tmp = [0u8; 2048];
            let mut rb = ReadBuf::new(&mut tmp);
            ready!(Pin::new(&mut self.rd).poll_read(cx, &mut rb))?;
            if rb.filled().is_empty() {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
            head.extend_from_slice(rb.filled());
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for Upgraded {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        ready!(me.poll_head(cx))?;
        if !me.leftover.is_empty() {
            let n = me.leftover.len().min(buf.remaining());
            buf.put_slice(&me.leftover[..n]);
            me.leftover.drain(..n);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut me.rd).poll_read(cx, buf)
    }
}

impl AsyncWrite for Upgraded {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.wr).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.wr).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.wr).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn early_data_waits_for_the_upgrade_response() {
        let cfg = rv_config::HttpUpgrade { path: "/test?ed=2048".into(), host: String::new(), headers: Vec::new(), early_data: 2048 };
        let (client, mut server) = tokio::io::duplex(4096);
        let peer = async {
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(server.read_u8().await.unwrap());
            }
            let mut b = [0; 4];
            assert!(tokio::time::timeout(std::time::Duration::from_millis(30), server.read_exact(&mut b)).await.is_err());
            server.write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\nready").await.unwrap();
            server.read_exact(&mut b).await.unwrap();
            assert_eq!(&b, b"ping");
        };
        let user = async {
            let mut s = open(Rc::new(Settings::new(&cfg, "test.local")), Box::new(client)).await.unwrap();
            s.write_all(b"ping").await.unwrap();
            let mut b = [0; 5];
            s.read_exact(&mut b).await.unwrap();
            assert_eq!(&b, b"ready");
        };
        tokio::join!(peer, user);
    }
}
