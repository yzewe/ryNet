use crate::BoxStream;
use rv_config::HttpHeader;
use rv_xhttp::{
    browser,
    request::{Range, rand_range},
};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const MAX_HEADER: usize = 8192;

fn pick(values: &[String]) -> &str {
    if values.is_empty() { "" } else { &values[rand_range(Range { from: 0, to: values.len() as i64 - 1 }) as usize] }
}

fn request(cfg: &HttpHeader) -> Vec<u8> {
    let mut out = format!("{} {} HTTP/{}\r\n", cfg.method, pick(&cfg.paths), cfg.request_version);
    let defaults;
    let headers = if cfg.request_headers.is_empty() {
        defaults = vec![
            ("Host".into(), vec!["www.baidu.com".into(), "www.bing.com".into()]),
            ("User-Agent".into(), vec![browser::chrome_ua()]),
            ("Sec-CH-UA".into(), vec![browser::chrome_ch_ua(browser::chrome_version(), "chrome")]),
            ("Sec-CH-UA-Mobile".into(), vec!["?0".into()]),
            ("Sec-CH-UA-Platform".into(), vec!["Windows".into()]),
            ("Sec-Fetch-Mode".into(), vec!["no-cors".into(), "cors".into(), "same-origin".into()]),
            ("Sec-Fetch-Dest".into(), vec!["empty".into()]),
            ("Sec-Fetch-Site".into(), vec!["none".into()]),
            ("Sec-Fetch-User".into(), vec!["?1".into()]),
            ("Accept-Encoding".into(), vec!["gzip, deflate".into()]),
            ("Connection".into(), vec!["keep-alive".into()]),
            ("Pragma".into(), vec!["no-cache".into()]),
        ];
        &defaults
    } else {
        &cfg.request_headers
    };
    for (k, values) in headers {
        out.push_str(&format!("{k}: {}\r\n", pick(values)));
    }
    out.push_str("\r\n");
    out.into_bytes()
}

pub struct HeaderStream {
    inner: BoxStream,
    request: Vec<u8>,
    sent: usize,
    response: Option<Vec<u8>>,
    leftover: Vec<u8>,
}

impl HeaderStream {
    pub fn new(inner: BoxStream, cfg: &HttpHeader) -> Self {
        Self { inner, request: request(cfg), sent: 0, response: Some(Vec::new()), leftover: Vec::new() }
    }
    fn poll_prefix(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.sent < self.request.len() {
            let n = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.request[self.sent..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.sent += n;
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for HeaderStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let me = &mut *self;
        while let Some(head) = &mut me.response {
            if let Some(end) = head.windows(4).position(|w| w == b"\r\n\r\n") {
                if end + 4 > MAX_HEADER {
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, "tcp http header: response too long")));
                }
                me.leftover = head.split_off(end + 4);
                me.response = None;
                break;
            }
            if head.len() >= MAX_HEADER {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, "tcp http header: response too long")));
            }
            let mut tmp = [0; 2048];
            let mut rb = ReadBuf::new(&mut tmp);
            ready!(Pin::new(&mut me.inner).poll_read(cx, &mut rb))?;
            if rb.filled().is_empty() {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
            head.extend_from_slice(rb.filled());
        }
        if !me.leftover.is_empty() {
            let n = buf.remaining().min(me.leftover.len());
            buf.put_slice(&me.leftover[..n]);
            me.leftover.drain(..n);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut me.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for HeaderStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        ready!(self.poll_prefix(cx))?;
        Pin::new(&mut self.inner).poll_write(cx, buf)
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn prefix_is_lazy_once_and_split_response_preserves_raw_payload_and_half_close() {
        let cfg = HttpHeader::from_json(
            &serde_json::json!({"request": {"method": "POST", "path": ["/fake"], "headers": {"Host": ["test.local"], "X-Test": "ok"}}}),
        )
        .unwrap();
        let (client, mut peer) = tokio::io::duplex(2);
        let mut stream = HeaderStream::new(Box::new(client), &cfg);
        stream.flush().await.unwrap();
        let mut byte = [0];
        assert!(tokio::time::timeout(std::time::Duration::from_millis(10), peer.read(&mut byte)).await.is_err());
        let server = async {
            let mut received = Vec::new();
            peer.read_to_end(&mut received).await.unwrap();
            assert_eq!(received, b"POST /fake HTTP/1.1\r\nHost: test.local\r\nX-Test: ok\r\n\r\nfirstsecond");
            for &b in b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\nContent-Encoding: gzip\r\n\r\nraw\0reply" {
                peer.write_all(&[b]).await.unwrap();
            }
            peer.shutdown().await.unwrap();
        };
        let client = async {
            stream.write_all(b"first").await.unwrap();
            stream.write_all(b"second").await.unwrap();
            stream.shutdown().await.unwrap();
            let mut back = Vec::new();
            stream.read_to_end(&mut back).await.unwrap();
            assert_eq!(back, b"raw\0reply");
        };
        tokio::join!(server, client);
    }

    #[tokio::test]
    async fn truncated_and_oversized_response_are_errors() {
        for data in [b"HTTP/1.1 200 OK\r\n".to_vec(), vec![b'x'; MAX_HEADER + 1]] {
            let cfg = HttpHeader::from_json(&serde_json::json!({})).unwrap();
            let (client, mut peer) = tokio::io::duplex(MAX_HEADER + 2);
            peer.write_all(&data).await.unwrap();
            peer.shutdown().await.unwrap();
            let mut stream = HeaderStream::new(Box::new(client), &cfg);
            let err = stream.read_u8().await.unwrap_err();
            assert_eq!(err.kind(), if data.len() > MAX_HEADER { io::ErrorKind::InvalidData } else { io::ErrorKind::UnexpectedEof });
        }
    }
}
