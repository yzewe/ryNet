use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{Request, encode_request_header, parse_response_header};

pub struct VlessStream<S> {
    inner: S,
    wbuf: Vec<u8>,
    wpos: usize,
    header_sent: bool,
    resp: Option<Vec<u8>>,
    rbuf: Vec<u8>,
    rpos: usize,
}

impl<S> VlessStream<S> {
    pub fn new(inner: S, req: &Request) -> io::Result<Self> {
        let mut wbuf = Vec::with_capacity(64);
        encode_request_header(req, &mut wbuf)?;
        Ok(Self { inner, wbuf, wpos: 0, header_sent: false, resp: Some(Vec::with_capacity(8)), rbuf: Vec::new(), rpos: 0 })
    }
    pub fn get_ref(&self) -> &S {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }
}

impl<S: AsyncWrite + Unpin> VlessStream<S> {
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.wpos < self.wbuf.len() {
            let n = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.wbuf[self.wpos..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.wpos += n;
        }
        if self.wpos > 0 {
            self.wbuf = Vec::new();
            self.wpos = 0;
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for VlessStream<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.rpos < this.rbuf.len() {
            let n = (this.rbuf.len() - this.rpos).min(buf.remaining());
            buf.put_slice(&this.rbuf[this.rpos..this.rpos + n]);
            this.rpos += n;
            if this.rpos == this.rbuf.len() {
                this.rbuf = Vec::new();
                this.rpos = 0;
            }
            return Poll::Ready(Ok(()));
        }
        if let Some(resp) = this.resp.as_mut() {
            let mut tmp = [0u8; 512];
            loop {
                let mut rb = ReadBuf::new(&mut tmp);
                ready!(Pin::new(&mut this.inner).poll_read(cx, &mut rb))?;
                if rb.filled().is_empty() {
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::UnexpectedEof, "vless: no response header")));
                }
                resp.extend_from_slice(rb.filled());
                if let Some(n) = parse_response_header(resp)? {
                    let rest = resp.split_off(n);
                    this.resp = None;
                    let k = rest.len().min(buf.remaining());
                    buf.put_slice(&rest[..k]);
                    if k < rest.len() {
                        this.rbuf = rest;
                        this.rpos = k;
                    }
                    if k > 0 {
                        return Poll::Ready(Ok(()));
                    }
                    break;
                }
            }
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for VlessStream<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if !this.header_sent {
            this.header_sent = true;
            let n = buf.len().min(8192);
            this.wbuf.extend_from_slice(&buf[..n]);
            let _ = this.poll_drain(cx)?;
            return Poll::Ready(Ok(n));
        }
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.header_sent = true;
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.header_sent = true;
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}
