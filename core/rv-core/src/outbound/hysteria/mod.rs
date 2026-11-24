pub(crate) mod cc;
mod h3;
pub(crate) mod hop;
mod proto;
pub(crate) mod socket;

#[cfg(fuzzing)]
pub use h3::fuzzing as h3_fuzzing;
#[cfg(fuzzing)]
pub use proto::fuzzing as proto_fuzzing;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use bytes::Bytes;
use rv_config::{Network, Outbound, QuicParams, Security, UdpMask};
use rv_net::Protect;
use rv_vless::Address;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use crate::{BoxStream, Error};

const CLOSE_OK: u32 = 0x100;
const CLOSE_PROTOCOL: u32 = 0x101;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const FAIL_FAST: Duration = Duration::from_secs(2);
const UDP_QUEUE: usize = 1024;
const KEEPALIVE_GRACE: Duration = Duration::from_secs(10);
const KEEPALIVE_GRACE_MIN: Duration = Duration::from_secs(2);

fn keepalive_grace(period_s: u64) -> Duration {
    (Duration::from_secs(period_s) * 3 / 4).clamp(KEEPALIVE_GRACE_MIN, KEEPALIVE_GRACE)
}
const IDLE_MARGIN: Duration = Duration::from_secs(5);

pub struct Client {
    address: String,
    port: u16,
    hop_ports: Vec<u16>,
    family: rv_config::Family,
    auth: String,
    server_name: String,
    insecure: bool,
    pins: rv_tls::Pins,
    alpn: Vec<Vec<u8>>,
    salamander: Option<(String, Option<(u32, u32)>)>,
    quic: QuicParams,
    udp_idle: Duration,
    protect: Option<Protect>,
    conn: RefCell<Option<Rc<Conn>>>,
    connecting: tokio::sync::Mutex<()>,
    failed: RefCell<Option<(tokio::time::Instant, io::ErrorKind, String)>>,
    me: Weak<Client>,
}

struct Conn {
    quic: quinn::Connection,
    _endpoint: quinn::Endpoint,
    _control: quinn::SendStream,
    udp: bool,
    sessions: RefCell<HashMap<u32, mpsc::Sender<Bytes>>>,
    next_session: Cell<u32>,
    active: Cell<usize>,
    idle_since: Cell<Option<tokio::time::Instant>>,
    idle_timer: Cell<bool>,
    _awake: Option<rv_stats::KeepAlive>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        if self.quic.close_reason().is_none() {
            self.quic.close(CLOSE_OK.into(), b"");
            tracing::debug!(target: "rv::pool", "hysteria conn dropped: closed");
        }
    }
}

struct Lease {
    conn: Rc<Conn>,
    client: Weak<Client>,
}

impl Lease {
    fn new(conn: &Rc<Conn>, client: Weak<Client>) -> Self {
        conn.active.set(conn.active.get() + 1);
        conn.idle_since.set(None);
        Self { conn: conn.clone(), client }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let c = &self.conn;
        let left = c.active.get().saturating_sub(1);
        c.active.set(left);
        if left > 0 {
            return;
        }
        let Some(client) = self.client.upgrade() else {
            c.quic.close(CLOSE_OK.into(), b"");
            return;
        };
        c.idle_since.set(Some(tokio::time::Instant::now()));
        if c.idle_timer.replace(true) {
            return;
        }
        let idle = if client.quic.keep_alive_period > 0 {
            keepalive_grace(client.quic.keep_alive_period)
        } else {
            Duration::from_secs(client.quic.max_idle_timeout) + IDLE_MARGIN
        };
        let (conn, client) = (Rc::downgrade(c), self.client.clone());
        tokio::task::spawn_local(async move {
            loop {
                let Some(c) = conn.upgrade() else { return };
                let Some(since) = c.idle_since.get() else {
                    c.idle_timer.set(false);
                    return;
                };
                let deadline = since + idle;
                if tokio::time::Instant::now() >= deadline {
                    c.idle_timer.set(false);
                    let timed_out = c.quic.close_reason().is_some();
                    c.quic.close(CLOSE_OK.into(), b"");
                    tracing::debug!(target: "rv::pool", "hysteria idle close rtt_ms={} timed_out={timed_out}", c.quic.rtt().as_millis());
                    if let Some(cl) = client.upgrade() {
                        let mut slot = cl.conn.borrow_mut();
                        if slot.as_ref().is_some_and(|x| Rc::ptr_eq(x, &c)) {
                            *slot = None;
                        }
                    }
                    return;
                }
                drop(c);
                tokio::time::sleep_until(deadline).await;
            }
        });
    }
}

impl Client {
    pub fn new(ob: &Outbound, h: &rv_config::Hysteria2, protect: Option<Protect>) -> Result<Rc<Self>, Error> {
        let Security::Tls(tls) = &ob.stream.security else {
            return Err(Error::Config("hysteria2 requires tls".into()));
        };
        let mut salamander = None;
        for m in &ob.stream.finalmask.udp {
            match m {
                UdpMask::Salamander { password, packet_size } if salamander.is_none() => {
                    salamander = Some((password.clone(), *packet_size))
                }
                UdpMask::Salamander { .. } => {
                    return Err(Error::Unsupported("several salamander masks".into()));
                }
                UdpMask::Other(t) => return Err(Error::Unsupported(format!("finalmask udp {t}"))),
            }
        }
        let quic = ob.stream.finalmask.quic.clone().unwrap_or_default();
        if quic.init_stream_window > quic.max_stream_window || quic.init_conn_window > quic.max_conn_window {
            return Err(Error::Config("initial QUIC receive window exceeds its maximum".into()));
        }
        if quic.congestion != "reno" {
            quinn::congestion::BbrConfig::default().go_profile(&quic.bbr_profile).map_err(|e| Error::Config(e.into()))?;
        }
        if !matches!(quic.congestion.as_str(), "" | "bbr" | "brutal" | "reno" | "force-brutal") {
            return Err(Error::Config(format!("congestion `{}`", quic.congestion)));
        }
        let udp_idle = match &ob.stream.network {
            Network::Hysteria(s) => Duration::from_secs(s.udp_idle_timeout.max(2) as u64),
            _ => crate::UDP_IDLE,
        };
        let alpn = vec![b"h3".to_vec()];
        Ok(Rc::new_cyclic(|me| Self {
            address: h.address.clone(),
            port: h.port,
            hop_ports: quic.udp_hop_ports.clone(),
            family: ob.stream.sockopt.family,
            auth: h.auth.clone(),
            server_name: if tls.server_name.is_empty() { h.address.clone() } else { tls.server_name.clone() },
            insecure: tls.allow_insecure,
            pins: rv_tls::Pins { sha256: tls.pinned_sha256.clone(), names: tls.verify_names.clone() },
            alpn,
            salamander,
            quic,
            udp_idle,
            protect,
            conn: RefCell::new(None),
            connecting: tokio::sync::Mutex::new(()),
            failed: RefCell::new(None),
            me: me.clone(),
        }))
    }

    pub fn reset(&self) {
        self.failed.take();
        if self.conn.borrow_mut().take().is_some() {
            tracing::debug!(target: "rv::pool", "hysteria reset");
        }
    }

    fn alive(&self) -> Option<Rc<Conn>> {
        self.conn.borrow().as_ref().filter(|c| c.quic.close_reason().is_none()).cloned()
    }

    async fn conn(&self) -> Result<(Rc<Conn>, Lease), Error> {
        if let Some(c) = self.alive() {
            let l = Lease::new(&c, self.me.clone());
            return Ok((c, l));
        }
        let queued = tokio::time::Instant::now();
        let _g = self.connecting.lock().await;
        if let Some(c) = self.alive() {
            let l = Lease::new(&c, self.me.clone());
            return Ok((c, l));
        }
        if let Some((at, kind, msg)) = self.failed.borrow().as_ref()
            && *at > queued
            && at.elapsed() < FAIL_FAST
        {
            return Err(Error::Io(io::Error::new(*kind, msg.clone())));
        }
        let t0 = std::time::Instant::now();
        let dialed = tokio::time::timeout(CONNECT_TIMEOUT, Box::pin(self.dial()))
            .await
            .unwrap_or_else(|_| Err(Error::Io(io::ErrorKind::TimedOut.into())));
        let c = match dialed {
            Ok(c) => Rc::new(c),
            Err(e) => {
                let (kind, msg) = match &e {
                    Error::Io(err) => (err.kind(), err.to_string()),
                    other => (io::ErrorKind::Other, other.to_string()),
                };
                *self.failed.borrow_mut() = Some((tokio::time::Instant::now(), kind, msg));
                return Err(e);
            }
        };
        self.failed.take();
        tracing::debug!(
            target: "rv::pool",
            "hysteria new conn ms={} rtt_ms={} udp={}",
            t0.elapsed().as_millis(),
            c.quic.rtt().as_millis(),
            c.udp,
        );
        let l = Lease::new(&c, self.me.clone());
        *self.conn.borrow_mut() = Some(c.clone());
        spawn_readers(&c);
        Ok((c, l))
    }

    fn client_config(&self, bps: Arc<AtomicU64>) -> Result<quinn::ClientConfig, Error> {
        let crypto = crate::quic::crypto(&self.server_name, self.insecure, &self.pins, &self.alpn)?;

        crate::quic::client_config(crypto, &self.quic, 0, true, bps)
    }

    async fn dial(&self) -> Result<Conn, Error> {
        let ips = rv_net::resolve_protected(&self.address, self.protect.as_ref()).await?;
        let ip = crate::outbound::dial::udp_ip(&ips, self.family).ok_or_else(|| Error::Io(io::ErrorKind::NotFound.into()))?;
        let port = match self.hop_ports.len() {
            0 => self.port,
            n => self.hop_ports[proto::random_u32() as usize % n],
        };
        let server = SocketAddr::new(ip, port);
        let endpoint = crate::quic::socket_endpoint(server, &self.quic, self.salamander.as_ref(), self.protect.as_ref(), 4)?;
        let bps = Arc::new(AtomicU64::new(0));
        let t0 = std::time::Instant::now();
        rv_stats::add(rv_stats::Slot::Handshakes, 1);
        let quic = endpoint
            .connect_with(self.client_config(bps.clone())?, server, &self.server_name)
            .map_err(|e| Error::Config(e.to_string()))?
            .await;
        tracing::debug!(target: "rv::dial", "{server} quic_ms={} keepalive_s={} idle_s={} err={}", t0.elapsed().as_millis(), self.quic.keep_alive_period, self.quic.max_idle_timeout, quic.as_ref().err().map(ToString::to_string).unwrap_or_default());
        let quic = quic.map_err(|e| Error::Io(io::Error::other(e)))?;

        let mut control = quic.open_uni().await.map_err(io::Error::other)?;
        control.write_all(&h3::control_stream()).await.map_err(io::Error::other)?;

        let (mut send, mut recv) = quic.open_bi().await.map_err(io::Error::other)?;
        send.write_all(&h3::auth_request(&self.auth, self.quic.brutal_down)).await.map_err(io::Error::other)?;
        let _ = send.finish();
        let mut buf = Vec::with_capacity(4096);
        let block = loop {
            match h3::take_headers(&buf) {
                Some(Ok((block, _))) => break block,
                Some(Err(())) => return Err(Error::Io(io::Error::other("hysteria: bad http/3 response"))),
                None => {}
            }
            let mut chunk = [0u8; 4096];
            let n = AsyncReadExt::read(&mut recv, &mut chunk).await?;
            if n == 0 {
                return Err(Error::Io(io::Error::other("hysteria: auth response truncated")));
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 64 * 1024 {
                return Err(Error::Io(io::Error::other("hysteria: auth response too large")));
            }
        };
        let resp = h3::parse_headers(&block).ok_or_else(|| Error::Io(io::Error::other("hysteria: bad auth response headers")))?;
        if resp.status != h3::STATUS_AUTH_OK {
            quic.close(CLOSE_PROTOCOL.into(), b"");
            return Err(Error::Io(io::Error::other(format!("hysteria: auth failed (status {})", resp.status))));
        }
        drop(recv);
        let q = &self.quic;
        match q.congestion.as_str() {
            "" | "brutal" if q.brutal_up > 0 && resp.cc_rx > 0 => bps.store(q.brutal_up.min(resp.cc_rx), Relaxed),
            "force-brutal" => bps.store(q.brutal_up, Relaxed),
            _ => {}
        }
        tracing::debug!(udp = resp.udp, "hysteria: connected to {server}");
        Ok(Conn {
            quic,
            _endpoint: endpoint,
            _control: control,
            udp: resp.udp,
            sessions: RefCell::new(HashMap::new()),
            next_session: Cell::new(1),
            active: Cell::new(0),
            idle_since: Cell::new(None),
            idle_timer: Cell::new(false),
            _awake: (q.keep_alive_period > 0).then(rv_stats::KeepAlive::new),
        })
    }

    pub async fn connect(&self, dest: Address, port: u16) -> Result<BoxStream, Error> {
        let (conn, lease) = self.conn().await?;
        let (send, recv) = conn.quic.open_bi().await.map_err(io::Error::other)?;
        let addr = match dest {
            Address::Domain(d) => format!("{d}:{port}"),
            Address::Ipv4(a) => SocketAddr::from((a, port)).to_string(),
            Address::Ipv6(a) => SocketAddr::from((a, port)).to_string(),
        };
        Ok(Box::new(HyStream {
            send,
            recv,
            head: proto::tcp_request(&addr),
            glued: false,
            resp: Some(Vec::new()),
            leftover: Vec::new(),
            _lease: lease,
        }))
    }

    pub async fn udp_session(
        &self,
        (first_dst, first): (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        let (conn, _lease) = crate::udp_setup(self.conn()).await?;
        if !conn.udp {
            return Err(Error::Unsupported("hysteria server has udp disabled".into()));
        }
        let id = conn.next_session.get();
        conn.next_session.set(id.wrapping_add(1).max(1));
        let (tx, mut srx) = mpsc::channel::<Bytes>(UDP_QUEUE);
        conn.sessions.borrow_mut().insert(id, tx);
        struct Unregister<'a>(&'a Conn, u32);
        impl Drop for Unregister<'_> {
            fn drop(&mut self) {
                self.0.sessions.borrow_mut().remove(&self.1);
            }
        }
        let _unregister = Unregister(&conn, id);
        let send = |dst: SocketAddr, data: &[u8]| {
            let max = conn.quic.max_datagram_size().unwrap_or(1200);
            for d in proto::udp_datagrams(id, &dst.to_string(), data, max) {
                if let Err(e) = conn.quic.send_datagram(Bytes::from(d)) {
                    tracing::debug!("hysteria udp send: {e}");
                }
            }
        };
        send(first_dst, &first);
        let mut defrag = proto::Defragger::default();
        let idle = tokio::time::sleep(self.udp_idle);
        tokio::pin!(idle);
        let closed = conn.quic.closed();
        tokio::pin!(closed);
        let mut last = tokio::time::Instant::now();
        loop {
            tokio::select! {
                e = &mut closed => {
                    return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::ConnectionReset, format!("hysteria: {e}"))));
                }
                p = rx.recv() => {
                    let Some((dst, data)) = p else { return Ok(()) };
                    send(dst, &data);
                    crate::udp_touch(&mut last, idle.as_mut(), self.udp_idle);
                }
                d = srx.recv() => {
                    let Some(d) = d else { return Ok(()) };
                    if let Some(m) = proto::parse_udp(&d)
                        && let Some(data) = defrag.feed(&m)
                        && let Ok(from) = m.addr.parse::<SocketAddr>()
                    {
                        reply(from, &data);
                    }
                    crate::udp_touch(&mut last, idle.as_mut(), self.udp_idle);
                }
                _ = &mut idle => {
                    let deadline = last + self.udp_idle;
                    if tokio::time::Instant::now() >= deadline {
                        return Ok(());
                    }
                    idle.as_mut().reset(deadline);
                }
            }
        }
    }
}

fn spawn_readers(c: &Rc<Conn>) {
    let quic = c.quic.clone();
    let conn = Rc::downgrade(c);
    tokio::task::spawn_local(async move {
        while let Ok(d) = quic.read_datagram().await {
            let Some(c) = conn.upgrade() else { break };
            if d.len() < 4 {
                continue;
            }
            let id = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
            if let Some(tx) = c.sessions.borrow().get(&id) {
                let _ = tx.try_send(d);
            }
        }
    });
    let quic = c.quic.clone();
    tokio::task::spawn_local(async move {
        let mut held = Vec::new();
        while let Ok(s) = quic.accept_uni().await {
            held.push(s);
        }
    });
}

struct HyStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    head: Vec<u8>,
    glued: bool,
    resp: Option<Vec<u8>>,
    leftover: Vec<u8>,
    _lease: Lease,
}

impl HyStream {
    fn poll_head(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.head.is_empty() {
            let n = ready!(AsyncWrite::poll_write(Pin::new(&mut self.send), cx, &self.head))?;
            self.head.drain(..n);
        }
        if self.head.capacity() != 0 {
            self.head = Vec::new();
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for HyStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        while let Some(r) = &mut me.resp {
            match proto::tcp_response(r) {
                Some(Ok(used)) => {
                    me.leftover = r.split_off(used);
                    me.resp = None;
                }
                Some(Err(msg)) => {
                    return Poll::Ready(Err(io::Error::other(format!("hysteria: {msg}"))));
                }
                None => {
                    let mut tmp = [0u8; 2048];
                    let mut rb = ReadBuf::new(&mut tmp);
                    ready!(AsyncRead::poll_read(Pin::new(&mut me.recv), cx, &mut rb))?;
                    if rb.filled().is_empty() {
                        return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                    }
                    r.extend_from_slice(rb.filled());
                }
            }
        }
        if !me.leftover.is_empty() {
            let n = me.leftover.len().min(buf.remaining());
            buf.put_slice(&me.leftover[..n]);
            me.leftover.drain(..n);
            return Poll::Ready(Ok(()));
        }
        AsyncRead::poll_read(Pin::new(&mut me.recv), cx, buf)
    }
}

impl AsyncWrite for HyStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if !self.glued {
            self.glued = true;
            let n = buf.len().min(16 * 1024);
            self.head.extend_from_slice(&buf[..n]);
            let _ = self.poll_head(cx)?;
            return Poll::Ready(Ok(n));
        }
        ready!(self.poll_head(cx))?;
        AsyncWrite::poll_write(Pin::new(&mut self.send), cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_head(cx))?;
        AsyncWrite::poll_flush(Pin::new(&mut self.send), cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_head(cx))?;
        AsyncWrite::poll_shutdown(Pin::new(&mut self.send), cx)
    }
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    fn crypto_frames(p: &[u8]) -> (Vec<(u64, Vec<u8>)>, bool) {
        let varint = |i: &mut usize| {
            let len = 1 << (p[*i] >> 6);
            let mut v = (p[*i] & 0x3f) as u64;
            for k in 1..len {
                v = v << 8 | p[*i + k] as u64;
            }
            *i += len;
            v
        };
        let (mut i, mut out, mut padding_first) = (0, Vec::new(), true);
        while i < p.len() {
            match p[i] {
                0 => {
                    padding_first &= out.is_empty();
                    i += 1
                }
                6 => {
                    i += 1;
                    let off = varint(&mut i);
                    let len = varint(&mut i) as usize;
                    out.push((off, p[i..i + len].to_vec()));
                    i += len;
                }
                t => panic!("unexpected frame {t:#x} in the first Initial"),
            }
        }
        (out, padding_first)
    }

    #[tokio::test]
    async fn quic_go_initial_padding_and_packet_number() {
        for doq in [false, true] {
            let alpn = if doq { crate::dns::DOQ_ALPN.iter().map(|p| p.to_vec()).collect() } else { vec![b"h3".to_vec()] };
            let crypto = crate::quic::crypto("test.local", true, &rv_tls::Pins::default(), &alpn).unwrap();
            let mut cfg = quinn::ClientConfig::new(Arc::new(crypto));
            let mut transport = quinn::TransportConfig::default();
            transport.initial_mtu(1280);
            cfg.transport_config(Arc::new(transport));
            let mut endpoint_cfg = quinn::EndpointConfig::default();
            endpoint_cfg.quic_go_compat(true);
            let endpoint =
                quinn::Endpoint::new(endpoint_cfg, None, std::net::UdpSocket::bind("127.0.0.1:0").unwrap(), Arc::new(quinn::TokioRuntime))
                    .unwrap();
            let receiver = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let connecting = endpoint.connect_with(cfg, receiver.local_addr().unwrap(), "test.local").unwrap();
            let mut packets = Vec::new();
            for index in 0..2 {
                let mut packet = [0; 2048];
                let (n, _) = tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut packet)).await.unwrap().unwrap();
                assert_eq!(n, 1280);
                assert_eq!(packet[0] & 0xf0, 0xc0);
                let dcid_len = packet[5] as usize;
                let suite = rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256.tls13().unwrap().quic_suite().unwrap();
                let keys = suite.keys(&packet[6..6 + dcid_len], rustls::Side::Server, rustls::quic::Version::V1);
                let mut at = 6 + dcid_len;
                at += 1 + packet[at] as usize;
                assert_eq!(packet[at], 0);
                at += 1;
                at += 1 << (packet[at] >> 6);
                let sample = packet[at + 4..at + 20].to_vec();
                let (head, payload) = packet.split_at_mut(at);
                keys.remote.header.decrypt_in_place(&sample, &mut head[0], &mut payload[..4]).unwrap();
                assert_eq!((head[0] & 3) + 1, 2);
                let (head, payload) = packet[..n].split_at_mut(at + 2);
                let pn = u16::from_be_bytes([head[at], head[at + 1]]) as u64;
                let plain = keys.remote.packet.decrypt_in_place(pn, head, payload).unwrap();
                let (frames, padding_first) = crypto_frames(plain);
                assert!(padding_first, "quic-go puts PADDING before the other frames (packet {index})");
                packets.push(frames);
            }
            let frames: Vec<_> = packets.iter().flatten().cloned().collect();
            let mut hello = vec![0; frames.iter().map(|(o, d)| *o as usize + d.len()).max().unwrap()];
            for (o, d) in &frames {
                hello[*o as usize..*o as usize + d.len()].copy_from_slice(d);
            }
            let sni = hello.windows(10).position(|w| w == b"test.local").unwrap();
            let cut = sni + 5;
            let has = |k: usize, off: usize, len: Option<usize>| {
                packets[k].iter().any(|(o, d)| *o as usize == off && len.is_none_or(|l| d.len() == l))
            };
            assert!(has(0, 0, Some(cut)), "first CRYPTO frame must stop in the middle of SNI");
            assert!(has(0, sni + 10, None), "the next frame continues after the SNI");
            assert!(has(1, cut, Some(5)), "the withheld SNI half goes in the last packet");
            let alpn: &[u8] = if doq { b"\x00\x10\x00\x12\x00\x10\x08http/1.1\x02h2\x03doq" } else { b"\x00\x10\x00\x05\x00\x03\x02h3" };
            assert!(hello.windows(alpn.len()).any(|v| v == alpn), "ALPN differs from captured Xray bytes");
            drop(connecting);
            endpoint.close(0u8.into(), b"");
        }
    }
}

#[cfg(test)]
mod pool_tests {
    use super::*;

    fn client(server: SocketAddr, quic: QuicParams) -> Rc<Client> {
        Rc::new_cyclic(|me| Client {
            address: server.ip().to_string(),
            port: server.port(),
            hop_ports: Vec::new(),
            family: rv_config::Family::Any,
            auth: String::new(),
            server_name: "test.local".into(),
            insecure: true,
            pins: rv_tls::Pins::default(),
            alpn: vec![b"h3".to_vec()],
            salamander: None,
            quic,
            udp_idle: crate::UDP_IDLE,
            protect: None,
            conn: RefCell::new(None),
            connecting: tokio::sync::Mutex::new(()),
            failed: RefCell::new(None),
            me: me.clone(),
        })
    }

    fn server() -> (quinn::Endpoint, SocketAddr) {
        let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![include_bytes!("../../../tests/fixtures/h3-cert.der").to_vec().into()],
                rustls::pki_types::PrivateKeyDer::Pkcs8(include_bytes!("../../../tests/fixtures/h3-key.der").to_vec().into()),
            )
            .unwrap();
        tls.alpn_protocols = vec![b"h3".to_vec()];
        let mut cfg = quinn::ServerConfig::with_crypto(Arc::new(quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap()));
        Arc::get_mut(&mut cfg.transport).unwrap().assume_peer_datagram_size(Some(1200));
        let server = quinn::Endpoint::server(cfg, "127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();
        (server, addr)
    }

    async fn connected(server: &quinn::Endpoint, peer: SocketAddr, q: &QuicParams) -> (Rc<Conn>, quinn::Connection) {
        let dial = async {
            let endpoint = crate::quic::socket_endpoint(peer, q, None, None, 4).unwrap();
            let crypto = crate::quic::crypto("test.local", true, &rv_tls::Pins::default(), &[b"h3".to_vec()]).unwrap();
            let cfg = crate::quic::client_config(crypto, q, 0, true, Arc::new(AtomicU64::new(0))).unwrap();
            let quic = endpoint.connect_with(cfg, peer, "test.local").unwrap().await.unwrap();
            let control = quic.open_uni().await.unwrap();
            Conn {
                quic,
                _endpoint: endpoint,
                _control: control,
                udp: true,
                sessions: RefCell::new(HashMap::new()),
                next_session: Cell::new(1),
                active: Cell::new(0),
                idle_since: Cell::new(None),
                idle_timer: Cell::new(false),
                _awake: None,
            }
        };
        let (c, s) = tokio::join!(dial, async { server.accept().await.unwrap().await.unwrap() });
        (Rc::new(c), s)
    }

    #[tokio::test]
    async fn udp_burst_from_the_server_is_not_dropped() {
        tokio::task::LocalSet::new()
            .run_until(async {
                const BURST: usize = 300;
                let (server, peer) = server();
                let q = QuicParams::default();
                let (c, remote) = connected(&server, peer, &q).await;
                let client = client(peer, q);
                *client.conn.borrow_mut() = Some(c.clone());
                spawn_readers(&c);
                let got = Rc::new(Cell::new(0));
                let (tx, mut rx) = mpsc::channel(8);
                let seen = got.clone();
                let reply = move |_: SocketAddr, _: &[u8]| seen.set(seen.get() + 1);
                let session = client.udp_session(("1.2.3.4:53".parse().unwrap(), b"hi".to_vec()), &mut rx, &reply);
                let burst = async {
                    let first = remote.read_datagram().await.unwrap();
                    let id = u32::from_be_bytes(first[..4].try_into().unwrap());
                    for i in 0..BURST {
                        for d in proto::udp_datagrams(id, "1.2.3.4:53", &[i as u8; 64], 1200) {
                            remote.send_datagram(d.into()).unwrap();
                        }
                    }
                    let _ = tokio::time::timeout(Duration::from_secs(3), async {
                        while got.get() < BURST {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    })
                    .await;
                    drop(tx);
                };
                let (r, ()) = tokio::join!(session, burst);
                r.unwrap();
                assert_eq!(got.get(), BURST);
            })
            .await;
    }

    #[tokio::test]
    async fn keepalive_conn_outlives_its_last_stream_for_a_grace() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (server, peer) = server();
                let q = QuicParams { keep_alive_period: 1, ..QuicParams::default() };
                let (c, _remote) = connected(&server, peer, &q).await;
                let client = client(peer, q);
                *client.conn.borrow_mut() = Some(c.clone());
                let grace = keepalive_grace(1);
                assert_eq!(grace, KEEPALIVE_GRACE_MIN);
                assert_eq!(keepalive_grace(15), KEEPALIVE_GRACE, "default keepalive: closed before the first ping");
                assert!(keepalive_grace(15) < Duration::from_secs(15));
                drop(Lease::new(&c, client.me.clone()));
                assert!(c.quic.close_reason().is_none(), "the last stream still has time to send its tail");
                tokio::time::sleep(grace / 2).await;
                let (again, lease) = client.conn().await.unwrap();
                assert!(Rc::ptr_eq(&again, &c), "a stream within the grace reuses the connection");
                drop((again, lease));
                tokio::time::sleep(grace * 3 / 4).await;
                assert!(c.quic.close_reason().is_none(), "the grace counts from the last stream");
                tokio::time::sleep(grace / 2).await;
                assert!(c.quic.close_reason().is_some());
                assert!(client.conn.borrow().is_none());
            })
            .await;
    }

    #[tokio::test]
    async fn idle_conn_without_keepalive_times_out_silently_like_quic_go() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (server, peer) = server();
                let q = QuicParams { max_idle_timeout: 4, ..QuicParams::default() };
                let (c, remote) = connected(&server, peer, &q).await;
                let client = client(peer, q);
                *client.conn.borrow_mut() = Some(c.clone());
                drop(Lease::new(&c, client.me.clone()));
                drop(c);
                tokio::time::sleep(Duration::from_millis(50)).await;
                remote.send_datagram(Bytes::from_static(b"tail")).unwrap();
                let reason = tokio::time::timeout(Duration::from_secs(12), remote.closed()).await.expect("connection closes");
                assert!(
                    matches!(reason, quinn::ConnectionError::TimedOut),
                    "no CONNECTION_CLOSE at the idle deadline, the idle timeout closes it: {reason:?}"
                );
                assert!(client.alive().is_none());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn waiters_share_a_failed_dial() {
        let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let c = client(silent.local_addr().unwrap(), QuicParams::default());
        let t0 = tokio::time::Instant::now();
        let (a, b, d, e) = tokio::join!(c.conn(), c.conn(), c.conn(), c.conn());
        assert!(a.is_err() && b.is_err() && d.is_err() && e.is_err());
        assert!(t0.elapsed() < CONNECT_TIMEOUT * 2, "one dial for the whole queue, took {:?}", t0.elapsed());
        let t1 = tokio::time::Instant::now();
        assert!(c.conn().await.is_err());
        assert!(t1.elapsed() >= CONNECT_TIMEOUT, "a caller after the failure dials again");
    }
}
