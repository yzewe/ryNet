pub mod basic;
pub mod dial;
pub mod hysteria;
pub mod shadowsocks;
pub mod trojan;
pub mod vless;
pub mod vless_enc;
pub mod vmess;

use std::io;
use std::net::SocketAddr;
use std::ops::Range;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use rv_config::{OutboundKind, Profile};
use rv_net::Protect;
use rv_vless::{Address, Command};
use tokio::io::{AsyncRead, ReadBuf};
use tokio::sync::mpsc;

use crate::{BoxStream, Error};

pub enum Proxy {
    Basic(basic::BasicProxy),
    Vless(vless::VlessOutbound),
    Vmess(vmess::VmessOutbound),
    Trojan(trojan::TrojanOutbound),
    Hysteria(Rc<hysteria::Client>),
    Shadowsocks(shadowsocks::ShadowsocksOutbound),
    #[cfg(feature = "plugins")]
    Plugin(crate::plugin::outbound::PluginOutbound),
}

impl Proxy {
    pub fn build(profile: &Profile, tag: &str, protect: Option<Protect>) -> Result<Self, Error> {
        let ob = profile.outbound(tag).ok_or_else(|| Error::Config(format!("no outbound `{tag}`")))?;
        match &ob.kind {
            OutboundKind::Socks(_) | OutboundKind::Http(_) => Ok(Self::Basic(basic::BasicProxy::new(profile, ob, protect)?)),
            OutboundKind::Vless(v) => Ok(Self::Vless(vless::VlessOutbound::new(profile, ob, v, protect)?)),
            OutboundKind::Vmess(v) => Ok(Self::Vmess(vmess::VmessOutbound::new(profile, ob, v, protect)?)),
            OutboundKind::Trojan(t) => Ok(Self::Trojan(trojan::TrojanOutbound::new(profile, ob, t, protect)?)),
            OutboundKind::Hysteria2(h) => {
                if let Some(dp) = &ob.stream.sockopt.dialer_proxy
                    && profile.outbound(dp).is_some_and(|o| o.kind.is_proxy())
                {
                    return Err(Error::Unsupported(format!("hysteria2 via dialerProxy `{dp}`")));
                }
                Ok(Self::Hysteria(hysteria::Client::new(ob, h, protect)?))
            }
            OutboundKind::Shadowsocks(s) => Ok(Self::Shadowsocks(shadowsocks::ShadowsocksOutbound::new(profile, ob, s, protect)?)),
            #[cfg(feature = "plugins")]
            OutboundKind::Plugin { protocol, settings } => {
                Ok(Self::Plugin(crate::plugin::outbound::PluginOutbound::new(profile, ob, protocol, settings, protect)?))
            }
            #[cfg(not(feature = "plugins"))]
            OutboundKind::Plugin { protocol, .. } => Err(Error::Unsupported(format!("protocol {protocol}"))),
            OutboundKind::Unsupported { protocol, reason } => {
                Err(Error::Unsupported(if protocol.is_empty() { reason.clone() } else { format!("{protocol}: {reason}") }))
            }
            _ => Err(Error::Config(format!("outbound `{tag}` is not a proxy"))),
        }
    }

    pub async fn connect(&self, dest: Address, port: u16) -> Result<BoxStream, Error> {
        match self {
            Self::Basic(p) => p.connect(dest, port).await,
            Self::Vless(v) => v.open(Command::Tcp, dest, port).await,
            Self::Vmess(v) => v.open(Command::Tcp, dest, port).await,
            Self::Trojan(t) => t.connect(dest, port).await,
            Self::Hysteria(h) => h.connect(dest, port).await,
            Self::Shadowsocks(s) => s.connect(dest, port).await,
            #[cfg(feature = "plugins")]
            Self::Plugin(p) => p.connect(dest, port).await,
        }
    }

    pub fn udp_allowed(&self, port: u16) -> bool {
        match self {
            Self::Basic(p) => p.udp_allowed(),
            Self::Vless(v) => v.udp_allowed(port),
            Self::Vmess(_) | Self::Trojan(_) | Self::Hysteria(_) => true,
            Self::Shadowsocks(s) => s.udp_allowed(),
            #[cfg(feature = "plugins")]
            Self::Plugin(p) => p.udp_allowed(),
        }
    }

    pub fn reset(&self) {
        match self {
            Self::Basic(p) => p.stream.reset(),
            Self::Vless(v) => v.stream().reset(),
            Self::Vmess(v) => v.stream().reset(),
            Self::Trojan(t) => t.stream().reset(),
            Self::Hysteria(h) => h.reset(),
            Self::Shadowsocks(s) => s.stream().reset(),
            #[cfg(feature = "plugins")]
            Self::Plugin(p) => p.reset(),
        }
    }

    pub async fn udp_session(
        &self,
        first: (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        self.udp_session_from(None, first, rx, reply).await
    }

    pub async fn udp_session_from(
        &self,
        source: Option<SocketAddr>,
        first: (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        let gid = source.map(xudp_id);
        match self {
            Self::Basic(p) => p.udp_session(first, rx, reply).await,
            Self::Vless(v) => v.udp_session_id(first, rx, reply, gid).await,
            Self::Vmess(v) => v.udp_session_id(first, rx, reply, gid).await,
            Self::Trojan(t) => t.udp_session(first, rx, reply).await,
            Self::Hysteria(h) => h.udp_session(first, rx, reply).await,
            Self::Shadowsocks(s) => s.udp_session(first, rx, reply).await,
            #[cfg(feature = "plugins")]
            Self::Plugin(p) => p.udp_session(first, rx, reply).await,
        }
    }
}

fn xudp_id(source: SocketAddr) -> [u8; 8] {
    static KEY: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    let key = KEY.get_or_init(|| {
        use ring::rand::SecureRandom;
        let mut k = [0; 32];
        ring::rand::SystemRandom::new().fill(&mut k).expect("xudp RNG");
        k
    });
    blake3::keyed_hash(key, format!("udp:{source}").as_bytes()).as_bytes()[..8].try_into().unwrap()
}

#[derive(Default)]
pub(crate) struct ReadBuffer {
    buf: Vec<u8>,
    at: usize,
    end: usize,
}

impl ReadBuffer {
    const READ: usize = 16 * 1024;

    pub(crate) fn from_vec(buf: Vec<u8>) -> Self {
        ReadBuffer { end: buf.len(), buf, at: 0 }
    }

    pub(crate) fn data(&self) -> &[u8] {
        &self.buf[self.at..self.end]
    }

    pub(crate) fn data_mut(&mut self) -> &mut [u8] {
        &mut self.buf[self.at..self.end]
    }

    pub(crate) fn len(&self) -> usize {
        self.end - self.at
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.at == self.end
    }

    pub(crate) fn consume(&mut self, n: usize) -> Range<usize> {
        let start = self.at;
        self.at += n.min(self.len());
        start..self.at
    }

    pub(crate) fn get(&self, r: Range<usize>) -> &[u8] {
        &self.buf[r]
    }

    pub(crate) fn poll_fill<R: AsyncRead + Unpin + ?Sized>(&mut self, cx: &mut Context<'_>, io: &mut R) -> Poll<io::Result<usize>> {
        if self.at > 0 {
            self.buf.copy_within(self.at..self.end, 0);
            self.end -= self.at;
            self.at = 0;
        }
        if self.buf.len() - self.end < Self::READ / 4 {
            self.buf.resize(self.end + Self::READ, 0);
        }
        let mut rb = ReadBuf::new(&mut self.buf[self.end..]);
        let Poll::Ready(r) = Pin::new(io).poll_read(cx, &mut rb) else {
            if self.end == 0 {
                self.buf = Vec::new();
            }
            return Poll::Pending;
        };
        r?;
        let n = rb.filled().len();
        self.end += n;
        Poll::Ready(Ok(n))
    }

    pub(crate) async fn fill<R: AsyncRead + Unpin + ?Sized>(&mut self, io: &mut R) -> io::Result<usize> {
        std::future::poll_fn(|cx| self.poll_fill(cx, io)).await
    }
}

#[cfg(test)]
mod read_buffer_tests {
    use super::ReadBuffer;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn frames_are_parsed_in_place_and_the_tail_moves_once_per_read() {
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let mut r = ReadBuffer::default();
        let frames: Vec<u8> = (0..5000u32).flat_map(|i| [3, i as u8, (i >> 8) as u8, 7]).collect();
        a.write_all(&frames[..frames.len() - 2]).await.unwrap();
        let mut got = Vec::new();
        while got.len() < 4999 {
            assert!(r.fill(&mut b).await.unwrap() > 0);
            while r.len() >= 4 {
                let f = r.consume(4);
                got.push(r.get(f).to_vec());
            }
            assert!(r.len() < 4);
        }
        a.write_all(&frames[frames.len() - 2..]).await.unwrap();
        r.fill(&mut b).await.unwrap();
        assert_eq!(r.data(), &frames[frames.len() - 4..]);
        assert!(got.iter().enumerate().all(|(i, f)| f[..] == frames[i * 4..i * 4 + 4]));
        drop(a);
        assert_eq!(r.fill(&mut b).await.unwrap(), 0);
        let big = vec![9u8; 70_000];
        let (mut a, mut b) = tokio::io::duplex(1 << 17);
        a.write_all(&big).await.unwrap();
        while r.len() < big.len() + 4 {
            r.fill(&mut b).await.unwrap();
        }
        r.consume(4);
        assert_eq!(r.data(), &big[..]);
    }
}
