use std::fmt;
use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::pin::Pin;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};

use blake2::Digest;
use quinn::udp::{RecvMeta, Transmit};
use quinn::{AsyncUdpSocket, UdpPoller};
use ring::rand::{SecureRandom, SystemRandom};

const SALT: usize = 8;
const RECV_SEGMENTS: usize = 2;

pub struct Salamander {
    rx: Arc<crate::quic::Rx>,
    state: Option<quinn::udp::UdpSocketState>,
    gecko: Option<Gecko>,
    psk: Vec<u8>,
    out: Mutex<Out>,
}

struct Out {
    buf: Vec<u8>,
    salts: [u8; 64 * SALT],
    used: usize,
}

fn xor_key(data: &mut [u8], key: &[u8; 32]) {
    let k: [u64; 4] = std::array::from_fn(|j| u64::from_ne_bytes(key[j * 8..j * 8 + 8].try_into().unwrap()));
    let (chunks, rest) = data.as_chunks_mut::<32>();
    for c in chunks {
        for (j, w) in c.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            let v = u64::from_ne_bytes(*w) ^ k[j];
            w.copy_from_slice(&v.to_ne_bytes());
        }
    }
    for (i, b) in rest.iter_mut().enumerate() {
        *b ^= key[i];
    }
}

impl fmt::Debug for Salamander {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Salamander").finish_non_exhaustive()
    }
}

const GECKO_HEADER: usize = 5;
const GECKO_TTL: std::time::Duration = std::time::Duration::from_secs(8);
const GECKO_MAX_PENDING: usize = 8;

struct Gecko {
    min: usize,
    max: usize,
    msg_id: std::sync::atomic::AtomicU8,
    pending: Mutex<std::collections::HashMap<u8, Pending>>,
}

struct Pending {
    chunks: Vec<Option<Vec<u8>>>,
    received: usize,
    deadline: std::time::Instant,
}

fn rand_below(n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let mut b = [0u8; 4];
    let _ = SystemRandom::new().fill(&mut b);
    u32::from_be_bytes(b) as usize % n
}

impl Gecko {
    fn frames(&self, p: &[u8]) -> Vec<Vec<u8>> {
        let total = 2 + rand_below(7);
        let size = p.len() / total;
        let id = self.msg_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed).wrapping_add(1);
        (0..total)
            .map(|i| {
                let chunk = &p[i * size..if i + 1 == total { p.len() } else { (i + 1) * size }];
                let base = SALT + GECKO_HEADER + chunk.len();
                let lo = self.min.max(base);
                let pad = if lo > self.max { 0 } else { lo - base + rand_below(self.max - lo + 1) };
                let mut f = vec![0x80, id, (i as u8) << 4 | total as u8];
                f.extend_from_slice(&(pad as u16).to_be_bytes());
                let at = f.len();
                f.resize(at + pad, 0);
                let _ = SystemRandom::new().fill(&mut f[at..]);
                f.extend_from_slice(chunk);
                f
            })
            .collect()
    }

    fn accept(&self, frame: &[u8]) -> Option<Vec<u8>> {
        if frame.len() < GECKO_HEADER {
            return None;
        }
        let (id, idx, total) = (frame[1], (frame[2] >> 4) as usize, (frame[2] & 0x0f) as usize);
        let pad = u16::from_be_bytes([frame[3], frame[4]]) as usize;
        if !(2..=8).contains(&total) || idx >= total || GECKO_HEADER + pad > frame.len() {
            return None;
        }
        let payload = &frame[GECKO_HEADER + pad..];
        let now = std::time::Instant::now();
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|_, p| p.deadline > now);
        if !pending.contains_key(&id) && pending.len() >= GECKO_MAX_PENDING {
            return None;
        }
        let e = pending.entry(id).or_insert_with(|| Pending { chunks: vec![None; total], received: 0, deadline: now + GECKO_TTL });
        if e.chunks.len() != total || e.chunks[idx].is_some() {
            return None;
        }
        e.chunks[idx] = Some(payload.to_vec());
        e.received += 1;
        if e.received < total {
            return None;
        }
        let e = pending.remove(&id)?;
        Some(e.chunks.into_iter().flatten().flatten().collect())
    }
}

impl Salamander {
    pub fn new(io: tokio::net::UdpSocket, password: &str, gecko: Option<(u32, u32)>) -> io::Result<Self> {
        if password.len() < 4 {
            return Err(io::Error::other("salamander: password must be at least 4 bytes"));
        }
        let io = io.into_std()?;
        Ok(Self {
            state: match gecko {
                Some(_) => None,
                None => Some(quinn::udp::UdpSocketState::new((&io).into())?),
            },
            gecko: gecko.map(|(min, max)| Gecko {
                min: min as usize,
                max: max as usize,
                msg_id: std::sync::atomic::AtomicU8::new(0),
                pending: Mutex::new(std::collections::HashMap::new()),
            }),
            rx: Arc::new(crate::quic::Rx::new(io)?),
            psk: password.as_bytes().to_vec(),
            out: Mutex::new(Out { buf: Vec::with_capacity(2048), salts: [0; 64 * SALT], used: 64 * SALT }),
        })
    }

    fn unmask(&self, buf: &mut [u8], m: &mut RecvMeta) {
        let stride = m.stride.max(1);
        if stride <= SALT {
            m.len = 0;
            return;
        }
        let mut out = 0;
        let mut at = 0;
        while at < m.len {
            let end = (at + stride).min(m.len);
            if end - at > SALT {
                let key = self.key(&buf[at..at + SALT]);
                let n = end - at - SALT;
                buf.copy_within(at + SALT..end, out);
                xor_key(&mut buf[out..out + n], &key);
                out += n;
            }
            at = end;
        }
        m.len = out;
        m.stride = stride - SALT;
    }

    fn push_masked(&self, out: &mut Out, d: &[u8]) -> io::Result<()> {
        if out.used == out.salts.len() {
            SystemRandom::new().fill(&mut out.salts).map_err(|_| io::Error::other("rng"))?;
            out.used = 0;
        }
        let salt: [u8; SALT] = out.salts[out.used..out.used + SALT].try_into().unwrap();
        out.used += SALT;
        out.buf.extend_from_slice(&salt);
        let at = out.buf.len();
        out.buf.extend_from_slice(d);
        xor_key(&mut out.buf[at..], &self.key(&salt));
        Ok(())
    }

    fn key(&self, salt: &[u8]) -> [u8; 32] {
        let mut h = blake2::Blake2b::<blake2::digest::consts::U32>::new();
        h.update(&self.psk);
        h.update(salt);
        h.finalize().into()
    }

    fn poll_recv_gecko(&self, g: &Gecko, cx: &mut Context, buf: &mut IoSliceMut<'_>, meta: &mut RecvMeta) -> Poll<io::Result<usize>> {
        let mut errors = 0;
        loop {
            let r = ready!(self.rx.poll_recv(cx, |sock| {
                let raw = unsafe { &mut *(&mut buf[..] as *mut [u8] as *mut [std::mem::MaybeUninit<u8>]) };
                let (n, from) = sock.recv_from(raw)?;
                let from = from.as_socket().ok_or_else(|| io::Error::other("salamander: bad peer"))?;
                *meta = RecvMeta { addr: from, len: n, stride: n, ecn: None, dst_ip: None };
                Ok((1, n))
            }));
            match r {
                Ok(_) => {
                    let n = meta.len;
                    if n <= SALT {
                        continue;
                    }
                    let key = self.key(&buf[..SALT]);
                    buf.copy_within(SALT..n, 0);
                    let n = n - SALT;
                    xor_key(&mut buf[..n], &key);
                    let len = if buf[0] & 0x80 == 0 {
                        n
                    } else {
                        let Some(packet) = g.accept(&buf[..n]) else { continue };
                        if packet.len() > buf.len() {
                            continue;
                        }
                        buf[..packet.len()].copy_from_slice(&packet);
                        packet.len()
                    };
                    meta.len = len;
                    meta.stride = len;
                    return Poll::Ready(Ok(1));
                }
                Err(e) => {
                    errors += 1;
                    if errors > 8 {
                        return Poll::Ready(Err(e));
                    }
                }
            }
        }
    }
}

impl AsyncUdpSocket for Salamander {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Box::pin(crate::quic::Writable::new(self.rx.clone()))
    }

    fn try_send(&self, t: &Transmit) -> io::Result<()> {
        let mut guard = self.out.lock().unwrap_or_else(|e| e.into_inner());
        let out = &mut *guard;
        out.buf.clear();
        let Some(state) = &self.state else {
            let d = t.contents;
            let parts = match &self.gecko {
                Some(g) if d.first().is_some_and(|b| b & 0x80 != 0) => g.frames(d),
                _ => vec![d.to_vec()],
            };
            for part in parts {
                out.buf.clear();
                self.push_masked(out, &part)?;
                let to = socket2::SockAddr::from(t.destination);
                self.rx.try_send(|sock| sock.send_to(&out.buf, &to).map(|_| ()))?;
            }
            return Ok(());
        };
        let seg = t.segment_size.unwrap_or(t.contents.len()).max(1);
        for d in t.contents.chunks(seg) {
            self.push_masked(out, d)?;
        }
        let masked = Transmit {
            destination: t.destination,
            ecn: t.ecn,
            contents: &out.buf,
            segment_size: t.segment_size.map(|s| s + SALT),
            src_ip: t.src_ip,
        };
        self.rx.try_send(|sock| state.send((&*sock).into(), &masked))
    }

    fn poll_recv(&self, cx: &mut Context, bufs: &mut [IoSliceMut<'_>], meta: &mut [RecvMeta]) -> Poll<io::Result<usize>> {
        let mut errors = 0;
        if let Some(g) = &self.gecko {
            return self.poll_recv_gecko(g, cx, &mut bufs[0], &mut meta[0]);
        }
        let state = self.state.as_ref().expect("state without gecko");
        loop {
            match ready!(self.rx.poll_recv(cx, |sock| {
                let n = state.recv((&*sock).into(), bufs, meta)?;
                Ok((n, crate::quic::recv_bytes(meta, n)))
            })) {
                Ok(n) => {
                    for (buf, m) in bufs.iter_mut().zip(meta.iter_mut()).take(n) {
                        self.unmask(buf, m);
                    }
                    return Poll::Ready(Ok(n));
                }
                Err(e) => {
                    errors += 1;
                    if errors > 8 {
                        return Poll::Ready(Err(e));
                    }
                    tracing::debug!("salamander recv: {e}");
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.rx.local_addr()
    }

    fn may_fragment(&self) -> bool {
        self.state.as_ref().is_none_or(|s| s.may_fragment())
    }

    fn max_transmit_segments(&self) -> usize {
        self.state.as_ref().map_or(1, |s| s.max_gso_segments())
    }

    fn max_receive_segments(&self) -> usize {
        self.state.as_ref().map_or(1, |s| s.gro_segments()).max(RECV_SEGMENTS)
    }
}

#[cfg(test)]
use crate::quic::GsoGuard;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct NoGso {
        segments: std::sync::atomic::AtomicUsize,
        sent: Mutex<Vec<usize>>,
    }

    impl AsyncUdpSocket for NoGso {
        fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
            unimplemented!()
        }
        fn try_send(&self, t: &Transmit) -> io::Result<()> {
            if t.segment_size.is_some_and(|s| s < t.contents.len()) {
                self.segments.store(1, Ordering::Relaxed);
            } else {
                self.sent.lock().unwrap().push(t.contents.len());
            }
            Ok(())
        }
        fn poll_recv(&self, _: &mut Context, _: &mut [IoSliceMut<'_>], _: &mut [RecvMeta]) -> Poll<io::Result<usize>> {
            Poll::Pending
        }
        fn local_addr(&self) -> io::Result<SocketAddr> {
            Ok("0.0.0.0:0".parse().unwrap())
        }
        fn max_transmit_segments(&self) -> usize {
            self.segments.load(Ordering::Relaxed).max(1)
        }
    }

    #[test]
    fn broken_gso_resends_lost_batch_and_stays_off_for_new_sockets() {
        let send = |g: &GsoGuard, len: usize| {
            g.try_send(&Transmit {
                destination: "1.2.3.4:443".parse().unwrap(),
                ecn: None,
                contents: &vec![0; len],
                segment_size: Some(1280),
                src_ip: None,
            })
            .unwrap()
        };
        let first = Arc::new(NoGso { segments: 10.into(), ..Default::default() });
        let g = GsoGuard(first.clone());
        send(&g, 2480);
        assert_eq!(*first.sent.lock().unwrap(), [1280, 1200], "lost batch is sent again one by one");
        let second = Arc::new(NoGso { segments: 10.into(), ..Default::default() });
        let g = GsoGuard(second.clone());
        assert_eq!(g.max_transmit_segments(), 1);
        send(&g, 2560);
        assert_eq!(*second.sent.lock().unwrap(), [1280, 1280]);
        assert_eq!(second.segments.load(Ordering::Relaxed), 10, "new socket never tries GSO");
    }

    #[test]
    fn xor_matches_bytewise() {
        let key: [u8; 32] = std::array::from_fn(|i| (i * 37 + 5) as u8);
        for n in [0, 1, 31, 32, 33, 100, 1500] {
            let data: Vec<u8> = (0..n).map(|i| (i * 13) as u8).collect();
            let mut fast = data.clone();
            xor_key(&mut fast, &key);
            let slow: Vec<u8> = data.iter().enumerate().map(|(i, b)| b ^ key[i % 32]).collect();
            assert_eq!(fast, slow);
        }
    }

    #[test]
    fn unmask_gro_batch() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let io = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let s = Salamander::new(io, "powered-by-test", None).unwrap();
            let plain: [Vec<u8>; 3] = [vec![1; 100], vec![2; 100], vec![3; 40]];
            let mut buf = Vec::new();
            for (i, p) in plain.iter().enumerate() {
                let salt = [i as u8 + 1; SALT];
                buf.extend_from_slice(&salt);
                let at = buf.len();
                buf.extend_from_slice(p);
                xor_key(&mut buf[at..], &s.key(&salt));
            }
            let mut m = RecvMeta { addr: "127.0.0.1:1".parse().unwrap(), len: buf.len(), stride: 108, ecn: None, dst_ip: None };
            s.unmask(&mut buf, &mut m);
            assert_eq!((m.len, m.stride), (240, 100));
            assert_eq!(buf[..240], plain.concat()[..]);
        });
    }

    #[tokio::test]
    async fn gecko_frames_longer_than_a_quic_packet_arrive_whole() {
        let rx = Salamander::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap(), "powered-by-test", Some((1900, 2000))).unwrap();
        let tx = Salamander::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap(), "powered-by-test", Some((1900, 2000))).unwrap();
        let packet: Vec<u8> = (0..1300u32).map(|i| (i * 7) as u8 | 0x80).collect();
        std::future::poll_fn(|cx| crate::quic::Writable::new(tx.rx.clone()).poll_writable_pinned(cx)).await.unwrap();
        tx.try_send(&Transmit { destination: rx.local_addr().unwrap(), ecn: None, contents: &packet, segment_size: None, src_ip: None })
            .unwrap();
        let mut buf = vec![0u8; 1452 * rx.max_receive_segments()];
        let mut meta = [RecvMeta::default()];
        let recv = std::future::poll_fn(|cx| rx.poll_recv(cx, &mut [IoSliceMut::new(&mut buf)], &mut meta));
        assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(5), recv).await.unwrap().unwrap(), 1);
        assert_eq!(&buf[..meta[0].len], &packet[..]);
    }

    #[test]
    fn gecko_frames_roundtrip() {
        let g = Gecko { min: 900, max: 1400, msg_id: std::sync::atomic::AtomicU8::new(0), pending: Mutex::new(Default::default()) };
        let packet: Vec<u8> = (0..1250u32).map(|i| (i * 7) as u8 | 0x80).collect();
        for _ in 0..20 {
            let mut frames = g.frames(&packet);
            assert!((2..=8).contains(&frames.len()));
            for f in &frames {
                assert_eq!(f[0], 0x80);
                assert!(
                    f.len() + SALT <= 1400
                        || f.len() + SALT == SALT + GECKO_HEADER + packet.len() / frames.len() + packet.len() % frames.len()
                );
            }
            frames.reverse();
            let mut out = None;
            for f in &frames {
                if let Some(p) = g.accept(f) {
                    out = Some(p);
                }
            }
            assert_eq!(out.unwrap(), packet);
        }
        assert!(g.accept(&[0x80, 1, 0x19, 0, 0]).is_none());
    }

    #[test]
    fn keystream_matches_reference() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let io = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let s = Salamander::new(io, "powered-by-test", None).unwrap();
            let k = s.key(&[1, 2, 3, 4, 5, 6, 7, 8]);
            let mut h = blake2::Blake2b::<blake2::digest::consts::U32>::new();
            h.update(b"powered-by-test\x01\x02\x03\x04\x05\x06\x07\x08");
            assert_eq!(k.to_vec(), h.finalize().to_vec());
        });
    }
}
