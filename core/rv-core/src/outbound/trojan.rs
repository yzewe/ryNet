use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use rv_config::{Outbound, Profile};
use rv_net::Protect;
use rv_vless::Address;
use sha2::Digest;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use crate::transport::Stream;
use crate::{BoxStream, Error, UDP_IDLE};

const CMD_CONNECT: u8 = 1;
const CMD_UDP: u8 = 3;

pub struct TrojanOutbound {
    key: [u8; 56],
    stream: Stream,
}

pub(crate) fn fits_socks_addr(addr: &Address) -> io::Result<()> {
    match addr {
        Address::Domain(d) if d.is_empty() || d.len() > 255 => Err(io::Error::new(io::ErrorKind::InvalidInput, "bad domain length")),
        _ => Ok(()),
    }
}

pub(crate) fn put_addr(o: &mut Vec<u8>, addr: &Address, port: u16) {
    match addr {
        Address::Ipv4(a) => {
            o.push(1);
            o.extend_from_slice(a);
        }
        Address::Domain(d) => {
            o.push(3);
            o.push(d.len().min(255) as u8);
            o.extend_from_slice(&d.as_bytes()[..d.len().min(255)]);
        }
        Address::Ipv6(a) => {
            o.push(4);
            o.extend_from_slice(a);
        }
    }
    o.extend_from_slice(&port.to_be_bytes());
}

impl TrojanOutbound {
    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    pub fn new(profile: &Profile, ob: &Outbound, t: &rv_config::Trojan, protect: Option<Protect>) -> Result<Self, Error> {
        let stream = Stream::new(profile, ob, &t.address, t.port, protect)?;
        let digest = sha2::Sha224::digest(t.password.as_bytes());
        let mut key = [0u8; 56];
        for (i, b) in digest.iter().enumerate() {
            let hex = b"0123456789abcdef";
            key[2 * i] = hex[(b >> 4) as usize];
            key[2 * i + 1] = hex[(b & 15) as usize];
        }
        Ok(Self { key, stream })
    }

    fn header(&self, cmd: u8, dest: &Address, port: u16) -> Vec<u8> {
        let mut h = Vec::with_capacity(56 + 2 + 1 + 1 + 255 + 2 + 2);
        h.extend_from_slice(&self.key);
        h.extend_from_slice(b"\r\n");
        h.push(cmd);
        put_addr(&mut h, dest, port);
        h.extend_from_slice(b"\r\n");
        h
    }

    pub async fn connect(&self, dest: Address, port: u16) -> Result<BoxStream, Error> {
        fits_socks_addr(&dest)?;
        let inner = self.stream.open().await?;
        Ok(Box::new(TrojanStream { inner, header: self.header(CMD_CONNECT, &dest, port), glued: false }))
    }

    pub async fn udp_session(
        &self,
        (first_dst, first): (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        let mut s = crate::udp_setup(Box::pin(self.stream.open())).await?;
        let mut w = self.header(CMD_UDP, &Address::from_ip(first_dst.ip()), first_dst.port());
        let pkt = |w: &mut Vec<u8>, dst: SocketAddr, data: &[u8]| {
            put_addr(w, &Address::from_ip(dst.ip()), dst.port());
            w.extend_from_slice(&(data.len() as u16).to_be_bytes());
            w.extend_from_slice(b"\r\n");
            w.extend_from_slice(data);
        };
        pkt(&mut w, first_dst, &first);
        crate::udp_write(&mut s, &w).await?;
        let mut rbuf = super::ReadBuffer::default();
        let idle = tokio::time::sleep(UDP_IDLE);
        tokio::pin!(idle);
        let mut last = tokio::time::Instant::now();
        loop {
            tokio::select! {
                p = rx.recv() => {
                    let Some((dst, data)) = p else { return Ok(()) };
                    w.clear();
                    pkt(&mut w, dst, &data);
                    crate::udp_drain(rx, &mut w, pkt);
                    crate::udp_write(&mut s, &w).await?;
                    crate::udp_touch(&mut last, idle.as_mut(), UDP_IDLE);
                }
                n = rbuf.fill(&mut s) => {
                    let n = n?;
                    if n == 0 {
                        return Ok(());
                    }
                    loop {
                        match parse_udp(rbuf.data()) {
                            Ok(Some((from, data, used))) => {
                                reply(from, data);
                                rbuf.consume(used);
                            }
                            Ok(None) => break,
                            Err(e) => return Err(Error::Io(e)),
                        }
                    }
                    crate::udp_touch(&mut last, idle.as_mut(), UDP_IDLE);
                }
                _ = &mut idle => {
                    let deadline = last + UDP_IDLE;
                    if tokio::time::Instant::now() >= deadline {
                        return Ok(());
                    }
                    idle.as_mut().reset(deadline);
                }
            }
        }
    }
}

fn parse_udp(b: &[u8]) -> io::Result<Option<(SocketAddr, &[u8], usize)>> {
    let bad = |what: &str| Err(io::Error::new(io::ErrorKind::InvalidData, format!("trojan udp: {what}")));
    let Some(&atyp) = b.first() else { return Ok(None) };
    let (ip, mut i): (std::net::IpAddr, usize) = match atyp {
        1 => match b.get(1..5) {
            Some(a) => (<[u8; 4]>::try_from(a).unwrap().into(), 5),
            None => return Ok(None),
        },
        4 => match b.get(1..17) {
            Some(a) => (<[u8; 16]>::try_from(a).unwrap().into(), 17),
            None => return Ok(None),
        },
        _ => return bad("address type"),
    };
    let Some(p) = b.get(i..i + 4) else { return Ok(None) };
    let port = u16::from_be_bytes([p[0], p[1]]);
    let len = u16::from_be_bytes([p[2], p[3]]) as usize;
    i += 4;
    let Some(crlf) = b.get(i..i + 2) else { return Ok(None) };
    if crlf != b"\r\n" {
        return bad("no CRLF");
    }
    i += 2;
    let Some(data) = b.get(i..i + len) else { return Ok(None) };
    Ok(Some((SocketAddr::new(ip, port), data, i + len)))
}

pub(super) fn prefixed(inner: BoxStream, header: Vec<u8>) -> BoxStream {
    Box::new(TrojanStream { inner, header, glued: false })
}

struct TrojanStream {
    inner: BoxStream,
    header: Vec<u8>,
    glued: bool,
}

impl TrojanStream {
    fn poll_header(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.header.is_empty() {
            let n = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.header))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.header.drain(..n);
        }
        if self.header.capacity() != 0 {
            self.header = Vec::new();
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for TrojanStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for TrojanStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if !self.glued {
            self.glued = true;
            let n = buf.len().min(rv_xhttp::activity::XRAY_BUFFER);
            self.header.extend_from_slice(&buf[..n]);
            let _ = self.poll_header(cx)?;
            return Poll::Ready(Ok(n));
        }
        ready!(self.poll_header(cx))?;
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_header(cx))?;
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_header(cx))?;
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(kani)]
#[path = "../proofs/trojan.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "../fuzzing/trojan.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn domain_over_255_bytes_is_refused_not_truncated() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        for (proto, settings) in [
            ("trojan", serde_json::json!({"servers": [{"address": "127.0.0.1", "port": port, "password": "pw"}]})),
            (
                "shadowsocks",
                serde_json::json!({"servers": [{"address": "127.0.0.1", "port": port, "method": "aes-128-gcm", "password": "pw"}]}),
            ),
        ] {
            let text = serde_json::json!({"outbounds": [{"tag": "proxy", "protocol": proto, "settings": settings}]}).to_string();
            let p = rv_config::parse_subscription(&text).unwrap().remove(0);
            let proxy = crate::Proxy::build(&p, "proxy", None).unwrap();
            let long = "a".repeat(250) + ".example";
            assert!(proxy.connect(Address::Domain(long), 443).await.is_err(), "{proto}");
        }
        assert!(tokio::time::timeout(std::time::Duration::from_millis(50), l.accept()).await.is_err());
    }

    #[test]
    fn udp_parse_partial_bad_and_ok() {
        let mut pkt = vec![1, 1, 2, 3, 4, 0, 53, 0, 2];
        pkt.extend_from_slice(b"\r\nhi");
        for cut in 0..pkt.len() {
            assert!(matches!(parse_udp(&pkt[..cut]), Ok(None)), "cut {cut}");
        }
        let (from, data, used) = parse_udp(&pkt).unwrap().unwrap();
        assert_eq!((from, data, used), ("1.2.3.4:53".parse().unwrap(), &b"hi"[..], pkt.len()));
        assert!(parse_udp(&[3, 5, b'a']).is_err());
        let mut bad = pkt.clone();
        bad[9] = b'x';
        assert!(parse_udp(&bad).is_err());
    }
}
