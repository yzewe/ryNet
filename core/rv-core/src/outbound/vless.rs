use std::net::SocketAddr;

use rv_config::{Flow, Outbound, Profile};
use rv_net::Protect;
use rv_vless::{Address, Command, Request, VisionStream, VlessStream};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

use crate::transport::{Kind, Stream};
use crate::{BoxStream, Error, UDP_IDLE};

pub struct VlessOutbound {
    id: [u8; 16],
    flow: Flow,
    stream: Stream,
    encryption: Option<std::rc::Rc<super::vless_enc::Client>>,
}

impl VlessOutbound {
    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    pub fn new(profile: &Profile, ob: &Outbound, v: &rv_config::Vless, protect: Option<Protect>) -> Result<Self, Error> {
        let stream = Stream::new(profile, ob, &v.address, v.port, protect)?;
        let flow = match v.flow {
            Flow::None => Flow::None,
            _ if v.encryption.is_none() && stream.dialer.tls.is_none() => {
                return Err(Error::Config("vision requires tls/reality".into()));
            }
            _ if v.encryption.is_none() && !matches!(stream.kind, Kind::Raw) => {
                return Err(Error::Config("vision works only over raw tcp".into()));
            }
            f => f,
        };
        let encryption = match &v.encryption {
            Some(e) => Some(super::vless_enc::Client::new(e).map_err(Error::Config)?),
            None => None,
        };
        Ok(Self { id: v.id, flow, stream, encryption })
    }

    pub fn udp_allowed(&self, port: u16) -> bool {
        !(self.flow == Flow::Vision && port == 443)
    }

    pub async fn open(&self, command: Command, dest: Address, port: u16) -> Result<BoxStream, Error> {
        let vision = self.flow != Flow::None;
        let req =
            Request { id: self.id, flow: if vision { rv_vless::Flow::Vision } else { rv_vless::Flow::None }, command, address: dest, port };
        if vision {
            if let Some(enc) = &self.encryption {
                let s = enc.handshake(self.stream.open().await?).await?;
                return Ok(Box::new(VisionStream::new(s, &req)?));
            }
            let tls = self.stream.dialer.tls().await?;
            return Ok(Box::new(VisionStream::new(tls, &req)?));
        }
        let mut s = self.stream.open().await?;
        if let Some(enc) = &self.encryption {
            s = Box::new(enc.handshake(s).await?);
        }
        Ok(Box::new(VlessStream::new(s, &req)?))
    }

    pub async fn udp_session(
        &self,
        (first_dst, first): (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        self.udp_session_id((first_dst, first), rx, reply, None).await
    }
    pub(crate) async fn udp_session_id(
        &self,
        (first_dst, first): (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
        gid: Option<[u8; 8]>,
    ) -> Result<(), Error> {
        let stream = crate::udp_setup(Box::pin(self.open(Command::Mux, Address::Domain("v1.mux.cool".into()), 666))).await?;
        xudp_session(stream, (first_dst, first), rx, reply, gid).await
    }
}

pub(crate) async fn xudp_session(
    mut stream: BoxStream,
    (first_dst, first): (SocketAddr, Vec<u8>),
    rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
    reply: &dyn Fn(SocketAddr, &[u8]),
    global_id: Option<[u8; 8]>,
) -> Result<(), Error> {
    use rv_vless::xudp;
    let mut gid = global_id.unwrap_or([0; 8]);
    if global_id.is_none() {
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut gid).map_err(|_| Error::Config("rng".into()))?;
    }

    let mut last_dst = first_dst;
    let mut wbuf = Vec::with_capacity(2048);
    let mut opened = xudp::encode(&mut wbuf, true, &Address::from_ip(first_dst.ip()), first_dst.port(), &gid, &first);
    if opened {
        crate::udp_write(&mut stream, &wbuf).await?;
    }

    let mut parser = xudp::Parser::default();
    let mut rbuf = vec![0u8; 16 * 1024];
    let idle = tokio::time::sleep(UDP_IDLE);
    tokio::pin!(idle);
    let mut last = tokio::time::Instant::now();
    loop {
        tokio::select! {
            pkt = rx.recv() => {
                let Some((dst, data)) = pkt else { return Ok(()) };
                last_dst = dst;
                wbuf.clear();
                if xudp::encode(&mut wbuf, !opened, &Address::from_ip(dst.ip()), dst.port(), &gid, &data) {
                    opened = true;
                }
                if opened {
                    crate::udp_drain(rx, &mut wbuf, |w, dst, data| {
                        last_dst = dst;
                        xudp::encode(w, false, &Address::from_ip(dst.ip()), dst.port(), &gid, data);
                    });
                }
                if !wbuf.is_empty() {
                    crate::udp_write(&mut stream, &wbuf).await?;
                }
                crate::udp_touch(&mut last, idle.as_mut(), UDP_IDLE);
            }
            n = stream.read(&mut rbuf) => {
                let n = n?;
                if n == 0 {
                    return Ok(());
                }
                parser.push(&rbuf[..n]);
                loop {
                    match parser.next_packet() {
                        Ok(Some(p)) => {
                            let from = match p.from {
                                Some((Address::Ipv4(a), port)) => SocketAddr::from((a, port)),
                                Some((Address::Ipv6(a), port)) => SocketAddr::from((a, port)),
                                _ => last_dst,
                            };
                            reply(from, &p.payload);
                        }
                        Ok(None) => break,
                        Err(_) => return Ok(()),
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
