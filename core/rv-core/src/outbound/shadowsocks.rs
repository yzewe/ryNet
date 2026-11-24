use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::{SystemTime, UNIX_EPOCH};

use md5::Digest;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use rv_config::{Outbound, Profile};
use rv_net::Protect;
use rv_vless::Address;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use super::trojan::put_addr;
use crate::transport::Stream;
use crate::{BoxStream, Error, UDP_IDLE};

const TAG: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cipher {
    Aes128,
    Aes256,
    Chacha,
}

impl Cipher {
    fn key_len(self) -> usize {
        match self {
            Cipher::Aes128 => 16,
            Cipher::Aes256 | Cipher::Chacha => 32,
        }
    }
    fn alg(self) -> &'static aead::Algorithm {
        match self {
            Cipher::Aes128 => &aead::AES_128_GCM,
            Cipher::Aes256 => &aead::AES_256_GCM,
            Cipher::Chacha => &aead::CHACHA20_POLY1305,
        }
    }
}

fn method(name: &str) -> Option<(Cipher, bool)> {
    Some(match name {
        "aes-128-gcm" => (Cipher::Aes128, false),
        "aes-256-gcm" => (Cipher::Aes256, false),
        "chacha20-ietf-poly1305" | "chacha20-poly1305" => (Cipher::Chacha, false),
        "2022-blake3-aes-128-gcm" => (Cipher::Aes128, true),
        "2022-blake3-aes-256-gcm" => (Cipher::Aes256, true),
        "2022-blake3-chacha20-poly1305" => (Cipher::Chacha, true),
        _ => return None,
    })
}

fn psk_list(password: &str, cipher: Cipher) -> Result<Vec<Vec<u8>>, Error> {
    use base64::Engine;
    password
        .split(':')
        .map(|part| {
            let k = base64::engine::general_purpose::STANDARD
                .decode(part.trim())
                .map_err(|_| Error::Config("shadowsocks 2022: key is not base64".into()))?;
            match k.len().cmp(&cipher.key_len()) {
                std::cmp::Ordering::Less => Err(Error::Config(format!("shadowsocks 2022: key must be {} bytes", cipher.key_len()))),
                std::cmp::Ordering::Equal => Ok(k),
                std::cmp::Ordering::Greater => Ok(ring::digest::digest(&ring::digest::SHA256, &k).as_ref()[..cipher.key_len()].to_vec()),
            }
        })
        .collect()
}

fn password_key(password: &str, len: usize) -> Vec<u8> {
    let mut key = Vec::with_capacity(len + 16);
    let mut prev: Vec<u8> = Vec::new();
    while key.len() < len {
        let mut h = md5::Md5::new();
        h.update(&prev);
        h.update(password.as_bytes());
        prev = h.finalize().to_vec();
        key.extend_from_slice(&prev);
    }
    key.truncate(len);
    key
}

struct HkdfLen(usize);
impl ring::hkdf::KeyType for HkdfLen {
    fn len(&self) -> usize {
        self.0
    }
}

const SALTS_TCP: usize = 1024;
const SALTS_UDP: usize = 256;

#[derive(Default)]
struct SaltLog(VecDeque<u64>);

impl SaltLog {
    fn fresh(&mut self, salt: &[u8], keep: usize) -> bool {
        let mut id = [0u8; 8];
        let n = salt.len().min(8);
        id[..n].copy_from_slice(&salt[..n]);
        let id = u64::from_le_bytes(id);
        if self.0.contains(&id) {
            return false;
        }
        if self.0.len() >= keep {
            self.0.pop_front();
        }
        self.0.push_back(id);
        true
    }
}

struct Replay {
    top: u64,
    seen: [u64; Replay::WORDS],
}

impl Replay {
    const WORDS: usize = 16;
    const BITS: u64 = 64 * Self::WORDS as u64;

    fn new(first: u64) -> Self {
        let mut r = Self { top: first, seen: [0; Self::WORDS] };
        r.set(first);
        r
    }

    fn bit(n: u64) -> (usize, u64) {
        let i = n % Self::BITS;
        ((i / 64) as usize, 1 << (i % 64))
    }

    fn set(&mut self, n: u64) {
        let (w, m) = Self::bit(n);
        self.seen[w] |= m;
    }

    fn fresh(&mut self, n: u64) -> bool {
        if n > self.top {
            if n - self.top >= Self::BITS {
                self.seen = [0; Self::WORDS];
            } else {
                for k in self.top + 1..=n {
                    let (w, m) = Self::bit(k);
                    self.seen[w] &= !m;
                }
            }
            self.top = n;
            self.set(n);
            return true;
        }
        if self.top - n >= Self::BITS {
            return false;
        }
        let (w, m) = Self::bit(n);
        if self.seen[w] & m != 0 {
            return false;
        }
        self.seen[w] |= m;
        true
    }
}

pub struct ShadowsocksOutbound {
    keys: Keys,
    salts: std::rc::Rc<std::cell::RefCell<SaltLog>>,
    plain: bool,
    stream: Stream,
    address: String,
    port: u16,
    protect: Option<Protect>,
    family: rv_config::Family,
}

impl ShadowsocksOutbound {
    pub fn new(profile: &Profile, ob: &Outbound, s: &rv_config::Shadowsocks, protect: Option<Protect>) -> Result<Self, Error> {
        let plain = matches!(s.method.to_ascii_lowercase().as_str(), "none" | "plain");
        let (cipher, v2022) = if plain {
            (Cipher::Aes128, false)
        } else {
            method(&s.method).ok_or_else(|| Error::Unsupported(format!("shadowsocks: {}", s.method)))?
        };
        let (key, identity) = if plain {
            (Vec::new(), Vec::new())
        } else if v2022 {
            let mut keys = psk_list(&s.password, cipher)?;
            let key = keys.pop().ok_or_else(|| Error::Config("shadowsocks 2022: no key".into()))?;
            if !keys.is_empty() && cipher == Cipher::Chacha {
                return Err(Error::Unsupported("shadowsocks 2022 chacha20: multi-user keys".into()));
            }
            (key, keys)
        } else {
            (password_key(&s.password, cipher.key_len()), Vec::new())
        };
        let stream = Stream::new(profile, ob, &s.address, s.port, protect.clone())?;
        Ok(Self {
            keys: Keys { cipher, v2022, key, identity },
            salts: Default::default(),
            plain,
            stream,
            address: s.address.clone(),
            port: s.port,
            protect,
            family: ob.stream.sockopt.family,
        })
    }

    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    pub fn udp_allowed(&self) -> bool {
        !self.stream.dialer.chained()
    }

    pub async fn connect(&self, dest: Address, port: u16) -> Result<BoxStream, Error> {
        super::trojan::fits_socks_addr(&dest)?;
        let inner = self.stream.open().await?;
        if self.plain {
            let mut addr = Vec::with_capacity(260);
            put_addr(&mut addr, &dest, port);
            return Ok(super::trojan::prefixed(inner, addr));
        }
        let k = &self.keys;
        let mut salt = vec![0u8; k.cipher.key_len()];
        SystemRandom::new().fill(&mut salt).map_err(|_| io::Error::other("rng"))?;
        let mut addr = Vec::with_capacity(260);
        put_addr(&mut addr, &dest, port);
        Ok(Box::new(SsStream {
            inner,
            v2022: k.v2022,
            max_chunk: if k.v2022 { 0xffff } else { 0x3fff },
            enc: Sealer { key: k.session(&salt), nonce: 0 },
            header: Some((salt.clone(), addr)),
            request_salt: salt,
            out: Vec::new(),
            written: 0,
            dec: None,
            salt_len: k.cipher.key_len(),
            raw: super::ReadBuffer::default(),
            plain: 0..0,
            want: Want::Salt,
            eof: false,
            owner: k.clone(),
            salts: self.salts.clone(),
        }))
    }

    pub async fn udp_session(
        &self,
        (first_dst, first): (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        let ip =
            crate::outbound::dial::udp_ip(&Box::pin(rv_net::resolve_protected(&self.address, self.protect.as_ref())).await?, self.family)
                .ok_or_else(|| io::Error::other("shadowsocks: no address"))?;
        let server = SocketAddr::new(ip, self.port);
        let sock = rv_net::udp_socket(server.is_ipv6(), self.protect.as_ref())?;
        sock.connect(server).await?;
        let mut codec = UdpCodec::new(&self.keys)?;
        codec.plain = self.plain;
        sock.send(&codec.seal(first_dst, &first)?).await?;
        let idle = tokio::time::sleep(UDP_IDLE);
        tokio::pin!(idle);
        let mut last = tokio::time::Instant::now();
        loop {
            tokio::select! {
                p = rx.recv() => {
                    let Some((dst, data)) = p else { return Ok(()) };
                    sock.send(&codec.seal(dst, &data)?).await?;
                    crate::udp_touch(&mut last, idle.as_mut(), UDP_IDLE);
                }
                r = sock.readable() => {
                    r?;
                    let got = crate::with_datagram_buf(|buf| {
                        let got = sock.try_recv(buf);
                        if let Ok(n) = got
                            && let Some((from, data)) = codec.open(&mut buf[..n])
                        {
                            reply(from, data);
                        }
                        got
                    });
                    match got {
                        Ok(_) => crate::udp_touch(&mut last, idle.as_mut(), UDP_IDLE),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                        Err(e) => return Err(e.into()),
                    }
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

struct UdpCodec<'a> {
    k: &'a Keys,
    rng: SystemRandom,
    session: u64,
    packet: u64,
    aes: Option<UdpAes>,
    plain: bool,
    replay: VecDeque<(u64, Replay)>,
    salts: SaltLog,
}

const UDP_CLIENT: u8 = 0;
const UDP_SERVER: u8 = 1;
const UDP_SESSION_KEYS: usize = 8;

struct UdpAes {
    send: LessSafeKey,
    header: HeaderCipher,
    identity: Vec<(HeaderCipher, [u8; 16])>,
    recv: VecDeque<(u64, LessSafeKey)>,
}

impl UdpAes {
    fn new(k: &Keys, id: &[u8; 8]) -> Self {
        Self {
            send: k.session(id),
            header: HeaderCipher::new(&k.key),
            identity: k
                .identity
                .iter()
                .enumerate()
                .map(|(i, key)| (HeaderCipher::new(key), blake3::hash(k.next_key(i)).as_bytes()[..16].try_into().unwrap()))
                .collect(),
            recv: VecDeque::with_capacity(UDP_SESSION_KEYS),
        }
    }

    fn remember(&mut self, id: u64, key: LessSafeKey) {
        if self.recv.len() == UDP_SESSION_KEYS {
            self.recv.pop_front();
        }
        self.recv.push_back((id, key));
    }
}

enum HeaderCipher {
    Aes128(Box<aes::Aes128>),
    Aes256(Box<aes::Aes256>),
}

impl HeaderCipher {
    fn new(key: &[u8]) -> Self {
        use aes::cipher::KeyInit;
        if key.len() == 16 {
            Self::Aes128(Box::new(aes::Aes128::new_from_slice(key).expect("aes-128 key")))
        } else {
            Self::Aes256(Box::new(aes::Aes256::new_from_slice(key).expect("aes-256 key")))
        }
    }

    fn apply(&self, b: &mut [u8; 16], encrypt: bool) {
        use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt};
        let mut blk = aes::Block::from(*b);
        match self {
            Self::Aes128(c) => {
                if encrypt {
                    c.encrypt_block(&mut blk)
                } else {
                    c.decrypt_block(&mut blk)
                }
            }
            Self::Aes256(c) => {
                if encrypt {
                    c.encrypt_block(&mut blk)
                } else {
                    c.decrypt_block(&mut blk)
                }
            }
        }
        b.copy_from_slice(&blk);
    }
}

impl<'a> UdpCodec<'a> {
    fn new(k: &'a Keys) -> io::Result<Self> {
        let rng = SystemRandom::new();
        let mut id = [0u8; 8];
        rng.fill(&mut id).map_err(|_| io::Error::other("rng"))?;
        let aes = (k.v2022 && k.cipher != Cipher::Chacha).then(|| UdpAes::new(k, &id));
        Ok(Self {
            k,
            rng,
            session: u64::from_be_bytes(id),
            packet: 0,
            aes,
            plain: false,
            replay: VecDeque::new(),
            salts: SaltLog::default(),
        })
    }

    fn seal(&mut self, dst: SocketAddr, data: &[u8]) -> io::Result<Vec<u8>> {
        let k = self.k;
        if self.plain {
            let mut out = Vec::with_capacity(data.len() + 19);
            put_addr(&mut out, &Address::from_ip(dst.ip()), dst.port());
            out.extend_from_slice(data);
            return Ok(out);
        }
        if !k.v2022 {
            let mut salt = vec![0u8; k.cipher.key_len()];
            self.rng.fill(&mut salt).map_err(|_| io::Error::other("rng"))?;
            let mut body = Vec::with_capacity(data.len() + 32);
            put_addr(&mut body, &Address::from_ip(dst.ip()), dst.port());
            body.extend_from_slice(data);
            k.session(&salt)
                .seal_in_place_append_tag(Nonce::assume_unique_for_key([0; 12]), Aad::empty(), &mut body)
                .map_err(|_| io::Error::other("seal"))?;
            salt.extend_from_slice(&body);
            return Ok(salt);
        }
        let mut hdr = [0u8; 16];
        hdr[..8].copy_from_slice(&self.session.to_be_bytes());
        hdr[8..].copy_from_slice(&self.packet.to_be_bytes());
        self.packet += 1;
        let mut body = Vec::with_capacity(data.len() + 64);
        body.push(UDP_CLIENT);
        body.extend_from_slice(&unix_now().to_be_bytes());
        let pad = if dst.port() == 53 && data.len() < 900 { random_padding(900 - data.len()) } else { 0 };
        body.extend_from_slice(&(pad as u16).to_be_bytes());
        body.resize(body.len() + pad, 0);
        put_addr(&mut body, &Address::from_ip(dst.ip()), dst.port());
        body.extend_from_slice(data);
        if k.cipher == Cipher::Chacha {
            let mut nonce = [0u8; 24];
            self.rng.fill(&mut nonce).map_err(|_| io::Error::other("rng"))?;
            let mut plain = hdr.to_vec();
            plain.extend_from_slice(&body);
            let (key, n) = xchacha(&k.key, &nonce);
            key.seal_in_place_append_tag(n, Aad::empty(), &mut plain).map_err(|_| io::Error::other("seal"))?;
            let mut out = nonce.to_vec();
            out.extend_from_slice(&plain);
            return Ok(out);
        }
        let mut out = Vec::with_capacity(16 * (1 + k.identity.len()) + body.len() + TAG);
        let aes = self.aes.as_ref().unwrap();
        let mut eh = hdr;
        aes.identity.first().map_or(&aes.header, |(cipher, _)| cipher).apply(&mut eh, true);
        out.extend_from_slice(&eh);
        for (cipher, hash) in &aes.identity {
            let mut b = *hash;
            for (x, h) in b.iter_mut().zip(hdr) {
                *x ^= h;
            }
            cipher.apply(&mut b, true);
            out.extend_from_slice(&b);
        }
        aes.send
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(hdr[4..].try_into().unwrap()), Aad::empty(), &mut body)
            .map_err(|_| io::Error::other("seal"))?;
        out.extend_from_slice(&body);
        Ok(out)
    }

    fn open<'b>(&mut self, b: &'b mut [u8]) -> Option<(SocketAddr, &'b [u8])> {
        let k = self.k;
        if self.plain {
            let (from, used) = parse_addr(b)?;
            return Some((from, &b[used..]));
        }
        if !k.v2022 {
            let s = k.cipher.key_len();
            if b.len() < s + TAG {
                return None;
            }
            let (salt, body) = b.split_at_mut(s);
            let plain = k.session(salt).open_in_place(Nonce::assume_unique_for_key([0; 12]), Aad::empty(), body).ok()?;
            if !self.salts.fresh(salt, SALTS_UDP) {
                return None;
            }
            let (from, used) = parse_addr(plain)?;
            return Some((from, &plain[used..]));
        }
        let mut derived = None;
        let ids: (u64, u64);
        let plain: &'b mut [u8] = if k.cipher == Cipher::Chacha {
            if b.len() < 24 + 16 + TAG {
                return None;
            }
            let (nonce, rest) = b.split_at_mut(24);
            let (key, n) = xchacha(&k.key, nonce);
            let p = key.open_in_place(n, Aad::empty(), rest).ok()?;
            ids = (u64::from_be_bytes(p[..8].try_into().ok()?), u64::from_be_bytes(p[8..16].try_into().ok()?));
            &mut p[16..]
        } else {
            if b.len() < 16 + TAG {
                return None;
            }
            let (h, rest) = b.split_at_mut(16);
            let mut hdr: [u8; 16] = (&*h).try_into().ok()?;
            let aes = self.aes.as_ref()?;
            aes.header.apply(&mut hdr, false);
            let id = u64::from_be_bytes(hdr[..8].try_into().ok()?);
            ids = (id, u64::from_be_bytes(hdr[8..].try_into().ok()?));
            let nonce = Nonce::assume_unique_for_key(hdr[4..].try_into().ok()?);
            if let Some((_, key)) = aes.recv.iter().find(|(session, _)| *session == id) {
                key.open_in_place(nonce, Aad::empty(), rest).ok()?
            } else {
                let key = k.session(&hdr[..8]);
                let plain = key.open_in_place(nonce, Aad::empty(), rest).ok()?;
                derived = Some((id, key));
                plain
            }
        };
        if plain.len() < 1 + 8 + 8 + 2 || plain[0] != UDP_SERVER {
            return None;
        }
        let ts = u64::from_be_bytes(plain[1..9].try_into().ok()?);
        if unix_now().abs_diff(ts) > 30 || u64::from_be_bytes(plain[9..17].try_into().ok()?) != self.session {
            return None;
        }
        let pad = u16::from_be_bytes([plain[17], plain[18]]) as usize;
        let rest = plain.get(19 + pad..)?;
        let (from, used) = parse_addr(rest)?;
        match self.replay.iter_mut().find(|(session, _)| *session == ids.0) {
            Some((_, window)) => {
                if !window.fresh(ids.1) {
                    return None;
                }
            }
            None => {
                if self.replay.len() == UDP_SESSION_KEYS {
                    self.replay.pop_front();
                }
                self.replay.push_back((ids.0, Replay::new(ids.1)));
            }
        }
        if let Some((id, key)) = derived {
            self.aes.as_mut()?.remember(id, key);
        }
        Some((from, &rest[used..]))
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn hchacha20(key: &[u8; 32], nonce: &[u8; 16]) -> [u8; 32] {
    let mut s = [0u32; 16];
    s[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for i in 0..8 {
        s[4 + i] = u32::from_le_bytes(key[4 * i..4 * i + 4].try_into().unwrap());
    }
    for i in 0..4 {
        s[12 + i] = u32::from_le_bytes(nonce[4 * i..4 * i + 4].try_into().unwrap());
    }
    fn qr(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(16);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(12);
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(8);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(7);
    }
    for _ in 0..10 {
        qr(&mut s, 0, 4, 8, 12);
        qr(&mut s, 1, 5, 9, 13);
        qr(&mut s, 2, 6, 10, 14);
        qr(&mut s, 3, 7, 11, 15);
        qr(&mut s, 0, 5, 10, 15);
        qr(&mut s, 1, 6, 11, 12);
        qr(&mut s, 2, 7, 8, 13);
        qr(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 32];
    for (i, w) in s[..4].iter().chain(&s[12..]).enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

fn xchacha(key: &[u8], nonce: &[u8]) -> (LessSafeKey, Nonce) {
    let sub = hchacha20(key.try_into().expect("32-byte key"), nonce[..16].try_into().unwrap());
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&nonce[16..24]);
    (LessSafeKey::new(UnboundKey::new(&aead::CHACHA20_POLY1305, &sub).expect("key")), Nonce::assume_unique_for_key(n))
}

fn parse_addr(b: &[u8]) -> Option<(SocketAddr, usize)> {
    match *b.first()? {
        1 => {
            let a: [u8; 4] = b.get(1..5)?.try_into().ok()?;
            let p = u16::from_be_bytes(b.get(5..7)?.try_into().ok()?);
            Some((SocketAddr::from((a, p)), 7))
        }
        4 => {
            let a: [u8; 16] = b.get(1..17)?.try_into().ok()?;
            let p = u16::from_be_bytes(b.get(17..19)?.try_into().ok()?);
            Some((SocketAddr::from((a, p)), 19))
        }
        _ => None,
    }
}

#[derive(Clone)]
struct Keys {
    cipher: Cipher,
    v2022: bool,
    key: Vec<u8>,
    identity: Vec<Vec<u8>>,
}

fn aes_block(key: &[u8], b: &mut [u8; 16], encrypt: bool) {
    use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
    let mut blk = aes::Block::from(*b);
    match key.len() {
        16 => {
            let c = aes::Aes128::new_from_slice(key).expect("aes-128 key");
            if encrypt { c.encrypt_block(&mut blk) } else { c.decrypt_block(&mut blk) }
        }
        _ => {
            let c = aes::Aes256::new_from_slice(key).expect("aes-256 key");
            if encrypt { c.encrypt_block(&mut blk) } else { c.decrypt_block(&mut blk) }
        }
    }
    b.copy_from_slice(&blk);
}

impl Keys {
    fn next_key(&self, i: usize) -> &[u8] {
        self.identity.get(i + 1).unwrap_or(&self.key)
    }

    fn tcp_eih(&self, salt: &[u8], out: &mut Vec<u8>) {
        let n = self.cipher.key_len();
        for (i, psk) in self.identity.iter().enumerate() {
            let mut material = psk.clone();
            material.extend_from_slice(salt);
            let sub = blake3::derive_key("shadowsocks 2022 identity subkey", &material);
            let mut b: [u8; 16] = blake3::hash(self.next_key(i)).as_bytes()[..16].try_into().unwrap();
            aes_block(&sub[..n], &mut b, true);
            out.extend_from_slice(&b);
        }
    }

    fn session(&self, salt: &[u8]) -> LessSafeKey {
        let n = self.cipher.key_len();
        let mut sub = vec![0u8; n];
        if self.v2022 {
            let mut material = self.key.clone();
            material.extend_from_slice(salt);
            sub.copy_from_slice(&blake3::derive_key("shadowsocks 2022 session subkey", &material)[..n]);
        } else {
            let prk = ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA1_FOR_LEGACY_USE_ONLY, salt).extract(&self.key);
            prk.expand(&[b"ss-subkey"], HkdfLen(n)).and_then(|okm| okm.fill(&mut sub)).expect("hkdf length");
        }
        LessSafeKey::new(UnboundKey::new(self.cipher.alg(), &sub).expect("key length"))
    }
}

struct Sealer {
    key: LessSafeKey,
    nonce: u128,
}

fn nonce(n: &mut u128) -> Nonce {
    let b = n.to_le_bytes();
    *n += 1;
    Nonce::assume_unique_for_key(b[..12].try_into().unwrap())
}

impl Sealer {
    fn seal(&mut self, data: &[u8], out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(data);
        let tag = self.key.seal_in_place_separate_tag(nonce(&mut self.nonce), Aad::empty(), &mut out[start..]).expect("seal");
        out.extend_from_slice(tag.as_ref());
    }
}

fn random_padding(max: usize) -> usize {
    use ring::rand::SecureRandom;
    let mut b = [0; 4];
    ring::rand::SystemRandom::new().fill(&mut b).expect("padding RNG");
    let ceiling = u32::MAX - u32::MAX % max as u32;
    while u32::from_ne_bytes(b) >= ceiling {
        ring::rand::SystemRandom::new().fill(&mut b).expect("padding RNG");
    }
    1 + (u32::from_ne_bytes(b) % max as u32) as usize
}

enum Want {
    Salt,
    Header,
    Len,
    Payload(usize),
}

struct SsStream {
    inner: BoxStream,
    v2022: bool,
    max_chunk: usize,
    enc: Sealer,
    header: Option<(Vec<u8>, Vec<u8>)>,
    request_salt: Vec<u8>,
    out: Vec<u8>,
    written: usize,
    dec: Option<(LessSafeKey, u128)>,
    salt_len: usize,
    raw: super::ReadBuffer,
    plain: std::ops::Range<usize>,
    want: Want,
    eof: bool,
    owner: Keys,
    salts: std::rc::Rc<std::cell::RefCell<SaltLog>>,
}

impl SsStream {
    fn stage(&mut self, data: &[u8]) {
        if let Some((salt, addr)) = self.header.take() {
            self.out.extend_from_slice(&salt);
            if self.v2022 {
                self.owner.tcp_eih(&salt, &mut self.out);
                let pad = if data.len() < 900 { random_padding(900) } else { 0 };
                let mut var = addr;
                var.extend_from_slice(&(pad as u16).to_be_bytes());
                var.resize(var.len() + pad, 0);
                let n = data.len().min(self.max_chunk - var.len());
                var.extend_from_slice(&data[..n]);
                let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let mut fixed = vec![0u8];
                fixed.extend_from_slice(&ts.to_be_bytes());
                fixed.extend_from_slice(&(var.len() as u16).to_be_bytes());
                self.enc.seal(&fixed, &mut self.out);
                self.enc.seal(&var, &mut self.out);
                self.chunks(&data[n..]);
                return;
            }
            let mut first = addr;
            first.extend_from_slice(data);
            self.chunks(&first);
            return;
        }
        self.chunks(data);
    }

    fn chunks(&mut self, data: &[u8]) {
        for part in data.chunks(self.max_chunk) {
            self.enc.seal(&(part.len() as u16).to_be_bytes(), &mut self.out);
            self.enc.seal(part, &mut self.out);
        }
    }

    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.written < self.out.len() {
            let n = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.out[self.written..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.written += n;
        }
        self.out = Vec::new();
        self.written = 0;
        Poll::Ready(Ok(()))
    }

    fn open(&mut self, len: usize) -> io::Result<std::ops::Range<usize>> {
        let (key, n) = self.dec.as_mut().ok_or_else(|| io::Error::other("shadowsocks: no key"))?;
        let n_plain = key
            .open_in_place(nonce(n), Aad::empty(), &mut self.raw.data_mut()[..len + TAG])
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "shadowsocks: decrypt failed (wrong password or method)"))?
            .len();
        let r = self.raw.consume(len + TAG);
        Ok(r.start..r.start + n_plain)
    }

    fn decode(&mut self) -> io::Result<bool> {
        loop {
            match self.want {
                Want::Salt => {
                    if self.raw.len() < self.salt_len {
                        return Ok(false);
                    }
                    let salt = self.raw.consume(self.salt_len);
                    if !self.v2022 && !self.salts.borrow_mut().fresh(self.raw.get(salt.clone()), SALTS_TCP) {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "shadowsocks: repeated response salt (replayed response)"));
                    }
                    self.dec = Some((self.owner.session(self.raw.get(salt)), 0));
                    self.want = if self.v2022 { Want::Header } else { Want::Len };
                }
                Want::Header => {
                    let len = 1 + 8 + self.salt_len + 2;
                    if self.raw.len() < len + TAG {
                        return Ok(false);
                    }
                    let h = self.open(len)?;
                    let h = self.raw.get(h);
                    if h[0] != 1 || h[9..9 + self.salt_len] != self.request_salt[..] {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "shadowsocks 2022: bad response header"));
                    }
                    let ts = u64::from_be_bytes(h[1..9].try_into().unwrap());
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                    if now.abs_diff(ts) > 30 {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "shadowsocks 2022: stale response"));
                    }
                    let n = u16::from_be_bytes([h[len - 2], h[len - 1]]) as usize;
                    self.want = Want::Payload(n);
                }
                Want::Len => {
                    if self.raw.len() < 2 + TAG {
                        return Ok(false);
                    }
                    let l = self.open(2)?;
                    let l = self.raw.get(l);
                    self.want = Want::Payload(u16::from_be_bytes([l[0], l[1]]) as usize);
                }
                Want::Payload(n) => {
                    if self.raw.len() < n + TAG {
                        return Ok(false);
                    }
                    let p = self.open(n)?;
                    self.want = Want::Len;
                    if !p.is_empty() {
                        self.plain = p;
                        return Ok(true);
                    }
                }
            }
        }
    }
}

impl AsyncRead for SsStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if !this.plain.is_empty() {
                let n = this.plain.len().min(buf.remaining());
                buf.put_slice(this.raw.get(this.plain.start..this.plain.start + n));
                this.plain.start += n;
                return Poll::Ready(Ok(()));
            }
            if this.decode()? {
                continue;
            }
            if this.eof {
                if !this.raw.is_empty() || matches!(this.want, Want::Header | Want::Payload(_)) {
                    return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                }
                return Poll::Ready(Ok(()));
            }
            if ready!(this.raw.poll_fill(cx, &mut this.inner))? == 0 {
                this.eof = true;
            }
        }
    }
}

impl AsyncWrite for SsStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        ready!(this.poll_drain(cx))?;
        let n = data.len().min(this.max_chunk * 4);
        this.out.reserve(n + 1024);
        this.stage(&data[..n]);
        match this.poll_drain(cx) {
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            _ => Poll::Ready(Ok(n)),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if this.header.is_some() {
            this.stage(&[]);
        }
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

#[cfg(kani)]
#[path = "../proofs/shadowsocks.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "../fuzzing/shadowsocks.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    fn udp_response(k: &Keys, server: u64, packet: u64, client: u64, dst: SocketAddr, data: &[u8]) -> Vec<u8> {
        let mut hdr = [0; 16];
        hdr[..8].copy_from_slice(&server.to_be_bytes());
        hdr[8..].copy_from_slice(&packet.to_be_bytes());
        let mut body = vec![UDP_SERVER];
        body.extend_from_slice(&unix_now().to_be_bytes());
        body.extend_from_slice(&client.to_be_bytes());
        body.extend_from_slice(&[0, 0]);
        put_addr(&mut body, &Address::from_ip(dst.ip()), dst.port());
        body.extend_from_slice(data);
        k.session(&hdr[..8])
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(hdr[4..].try_into().unwrap()), Aad::empty(), &mut body)
            .unwrap();
        aes_block(&k.key, &mut hdr, true);
        let mut out = hdr.to_vec();
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn udp_aes_keys_are_reused_and_rotations_are_bounded() {
        for cipher in [Cipher::Aes128, Cipher::Aes256] {
            let k = Keys { cipher, v2022: true, key: (0..cipher.key_len()).map(|i| i as u8).collect(), identity: Vec::new() };
            let mut codec = UdpCodec::new(&k).unwrap();
            let dst: SocketAddr = "127.0.0.1:8080".parse().unwrap();
            let mut b = udp_response(&k, 1, 0, codec.session, dst, b"reply");
            assert_eq!(codec.open(&mut b), Some((dst, &b"reply"[..])));
            let key = &codec.aes.as_ref().unwrap().recv[0].1 as *const LessSafeKey;
            for packet in 1..64 {
                let mut b = udp_response(&k, 1, packet, codec.session, dst, b"again");
                assert_eq!(codec.open(&mut b), Some((dst, &b"again"[..])));
                let cache = &codec.aes.as_ref().unwrap().recv;
                assert_eq!(cache.len(), 1);
                assert_eq!(&cache[0].1 as *const LessSafeKey, key);
            }
            for id in 2..=UDP_SESSION_KEYS as u64 + 2 {
                let mut b = udp_response(&k, id, 0, codec.session, dst, &[id as u8]);
                assert_eq!(codec.open(&mut b), Some((dst, &[id as u8][..])));
            }
            let cache = &codec.aes.as_ref().unwrap().recv;
            assert_eq!(cache.len(), UDP_SESSION_KEYS);
            assert_eq!(cache.front().unwrap().0, 3);
            let old_ids: Vec<_> = cache.iter().map(|(id, _)| *id).collect();
            let mut bad = udp_response(&k, 99, 0, codec.session, dst, b"bad");
            *bad.last_mut().unwrap() ^= 1;
            assert!(codec.open(&mut bad).is_none());
            let mut foreign = udp_response(&k, 100, 0, codec.session ^ 1, dst, b"foreign");
            assert!(codec.open(&mut foreign).is_none());
            assert_eq!(codec.aes.as_ref().unwrap().recv.iter().map(|(id, _)| *id).collect::<Vec<_>>(), old_ids);
            let mut b = udp_response(&k, 1, 100, codec.session, dst, b"late");
            assert_eq!(codec.open(&mut b), Some((dst, &b"late"[..])));
            assert_eq!(codec.aes.as_ref().unwrap().recv.len(), UDP_SESSION_KEYS);
        }
    }

    #[test]
    fn replayed_udp_responses_are_dropped() {
        let dst: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let k = Keys { cipher: Cipher::Aes128, v2022: true, key: (0..16).collect(), identity: Vec::new() };
        let mut codec = UdpCodec::new(&k).unwrap();
        let wire = |packet: u64, codec: &UdpCodec| udp_response(&k, 7, packet, codec.session, dst, b"data");
        for packet in [0u64, 1, 5, 3, 2000, 1999] {
            let mut b = wire(packet, &codec);
            assert!(codec.open(&mut b).is_some(), "свежий пакет {packet}");
        }
        for packet in [0u64, 1, 5, 3, 2000, 1999] {
            let mut b = wire(packet, &codec);
            assert!(codec.open(&mut b).is_none(), "повтор пакета {packet}");
        }
        let mut b = wire(2000 - Replay::BITS, &codec);
        assert!(codec.open(&mut b).is_none());
        let mut b = wire(2000 - Replay::BITS + 1, &codec);
        assert!(codec.open(&mut b).is_some());

        let k = Keys { cipher: Cipher::Aes128, v2022: false, key: vec![9; 16], identity: Vec::new() };
        let mut codec = UdpCodec::new(&k).unwrap();
        let legacy = |salt: u8| {
            let mut body = Vec::new();
            put_addr(&mut body, &Address::from_ip(dst.ip()), dst.port());
            body.extend_from_slice(b"data");
            let salt = vec![salt; 16];
            k.session(&salt).seal_in_place_append_tag(Nonce::assume_unique_for_key([0; 12]), Aad::empty(), &mut body).unwrap();
            [salt, body].concat()
        };
        let mut first = legacy(1);
        let mut again = first.clone();
        assert_eq!(codec.open(&mut first), Some((dst, &b"data"[..])));
        assert!(codec.open(&mut again).is_none());
        let mut other = legacy(2);
        assert_eq!(codec.open(&mut other), Some((dst, &b"data"[..])));
    }

    #[test]
    fn replay_window_and_salt_log() {
        let mut w = Replay::new(10);
        assert!(!w.fresh(10));
        assert!(w.fresh(9) && !w.fresh(9));
        assert!(w.fresh(10 + Replay::BITS * 3) && !w.fresh(10 + Replay::BITS * 3));
        assert!(!w.fresh(9), "ушло за окно");
        let mut log = SaltLog::default();
        assert!(log.fresh(&[1; 16], 2) && log.fresh(&[2; 16], 2) && !log.fresh(&[1; 16], 2));
        assert!(log.fresh(&[3; 16], 2));
        assert!(log.fresh(&[1; 16], 2), "самая старая соль вытеснена");
    }

    #[test]
    fn udp_aes_cached_header_and_eih_match_uncached_wire() {
        for cipher in [Cipher::Aes128, Cipher::Aes256] {
            for identities in 0..=2 {
                let n = cipher.key_len();
                let k = Keys { cipher, v2022: true, key: vec![0x33; n], identity: (0..identities).map(|i| vec![i + 1; n]).collect() };
                let mut codec = UdpCodec::new(&k).unwrap();
                let dst: SocketAddr = "[::1]:8080".parse().unwrap();
                for packet in 0..32u64 {
                    let data = vec![packet as u8; 1 + packet as usize * 31];
                    let wire = codec.seal(dst, &data).unwrap();
                    let mut hdr: [u8; 16] = wire[..16].try_into().unwrap();
                    aes_block(k.identity.first().unwrap_or(&k.key), &mut hdr, false);
                    assert_eq!(&hdr[..8], codec.session.to_be_bytes());
                    assert_eq!(&hdr[8..], packet.to_be_bytes());
                    for (i, psk) in k.identity.iter().enumerate() {
                        let mut block: [u8; 16] = wire[16 * (i + 1)..16 * (i + 2)].try_into().unwrap();
                        aes_block(psk, &mut block, false);
                        for (b, h) in block.iter_mut().zip(hdr) {
                            *b ^= h;
                        }
                        assert_eq!(block, blake3::hash(k.next_key(i)).as_bytes()[..16]);
                    }
                    let mut body = wire[16 * (1 + identities as usize)..].to_vec();
                    let plain = k
                        .session(&hdr[..8])
                        .open_in_place(Nonce::assume_unique_for_key(hdr[4..].try_into().unwrap()), Aad::empty(), &mut body)
                        .unwrap();
                    assert_eq!(plain[0], UDP_CLIENT);
                    assert!(unix_now().abs_diff(u64::from_be_bytes(plain[1..9].try_into().unwrap())) <= 1);
                    assert_eq!(&plain[9..11], &[0, 0]);
                    let (from, used) = parse_addr(&plain[11..]).unwrap();
                    assert_eq!(from, dst);
                    assert_eq!(&plain[11 + used..], data);
                    let mut reply = udp_response(&k, 42, packet, codec.session, dst, &data);
                    assert_eq!(codec.open(&mut reply), Some((dst, &data[..])));
                }
            }
        }
    }

    #[tokio::test]
    async fn udp_session_gets_big_and_small_datagrams() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        let json = format!(
            r#"[{{"remarks": "ss", "outbounds": [{{"tag": "proxy", "protocol": "shadowsocks",
            "settings": {{"servers": [{{"address": "127.0.0.1", "port": {port}, "method": "none", "password": "x"}}]}}}}]}}]"#
        );
        let p = rv_config::parse_subscription(&json).unwrap().remove(0);
        let crate::Proxy::Shadowsocks(ss) = crate::Proxy::build(&p, "proxy", None).unwrap() else { panic!() };
        let dst: SocketAddr = "1.2.3.4:53".parse().unwrap();
        let got = std::cell::RefCell::new(Vec::new());
        let reply = |from: SocketAddr, data: &[u8]| got.borrow_mut().push((from, data.to_vec()));
        let (tx, mut rx) = mpsc::channel(4);
        let big: Vec<u8> = (0..40_000u32).map(|i| i as u8).collect();
        let peer = async {
            let mut b = vec![0u8; 2048];
            let (n, client) = server.recv_from(&mut b).await.unwrap();
            assert_eq!(&b[7..n], b"query");
            for data in [&big[..], b"tail"] {
                let mut d = vec![1, 1, 2, 3, 4, 0, 53];
                d.extend_from_slice(data);
                server.send_to(&d, client).await.unwrap();
            }
            while got.borrow().len() < 2 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            drop(tx);
        };
        let session = ss.udp_session((dst, b"query".to_vec()), &mut rx, &reply);
        let (r, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async { tokio::join!(session, peer) }).await.unwrap();
        r.unwrap();
        assert_eq!(*got.borrow(), [(dst, big.clone()), (dst, b"tail".to_vec())]);
    }

    #[test]
    fn udp_plain_is_address_and_data() {
        let k = Keys { cipher: Cipher::Aes128, v2022: false, key: Vec::new(), identity: Vec::new() };
        let mut codec = UdpCodec::new(&k).unwrap();
        codec.plain = true;
        let dst: SocketAddr = "1.2.3.4:53".parse().unwrap();
        let mut wire = codec.seal(dst, b"query").unwrap();
        assert_eq!(wire, [&[1, 1, 2, 3, 4, 0, 53][..], b"query"].concat());
        assert_eq!(codec.open(&mut wire), Some((dst, &b"query"[..])));
    }

    #[tokio::test]
    async fn tcp_plain_sends_address_with_first_data() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (client_io, mut server_io) = tokio::io::duplex(1 << 16);
        let mut addr = Vec::new();
        put_addr(&mut addr, &Address::Domain("example.test".into()), 443);
        let mut s = super::super::trojan::prefixed(Box::new(client_io), addr.clone());
        s.write_all(b"hello").await.unwrap();
        s.flush().await.unwrap();
        let mut got = vec![0u8; addr.len() + 5];
        server_io.read_exact(&mut got).await.unwrap();
        assert_eq!(got, [&addr[..], b"hello"].concat());
        server_io.write_all(b"world").await.unwrap();
        let mut back = [0u8; 5];
        s.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"world");
    }

    #[test]
    fn hchacha20_vector() {
        let key: [u8; 32] = std::array::from_fn(|i| i as u8);
        let nonce = [0, 0, 0, 9, 0, 0, 0, 0x4a, 0, 0, 0, 0, 0x31, 0x41, 0x59, 0x27];
        let hex: String = hchacha20(&key, &nonce).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "82413b4227b27bfed30e42508a877d73a0f9e4d58a74a853c12ec41326d3ecdc");
    }

    #[test]
    fn longer_2022_keys_are_hashed_like_sing_shadowsocks() {
        let hex = |s: &str| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect::<Vec<u8>>();
        let k32 = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
        let k64 = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4vMDEyMzQ1Njc4OTo7PD0+Pw==";
        assert_eq!(psk_list(k32, Cipher::Aes128).unwrap(), [hex("3eb1bd439947eb762998e566ccc2e099")]);
        assert_eq!(psk_list(k64, Cipher::Aes256).unwrap(), [hex("fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108")]);
        assert_eq!(psk_list(&format!("{k64}:{k32}"), Cipher::Aes128).unwrap()[1], hex("3eb1bd439947eb762998e566ccc2e099"));
        assert_eq!(psk_list("AAECAwQFBgcICQoLDA0ODw==", Cipher::Aes128).unwrap(), [(0..16).collect::<Vec<u8>>()]);
        assert!(psk_list("AAECAwQFBgcICQoLDA0ODw==", Cipher::Aes256).is_err());
        assert!(psk_list("notbase64!", Cipher::Aes128).is_err());
    }

    #[test]
    fn password_key_matches_reference() {
        let k = password_key("password", 32);
        assert_eq!(&k[..4], &[0x5f, 0x4d, 0xcc, 0x3b]);
        assert_eq!(k.len(), 32);
    }

    #[tokio::test]
    async fn many_small_chunks_and_split_reads() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (m, pw) in [("aes-128-gcm", "secret"), ("2022-blake3-aes-128-gcm", "AAECAwQFBgcICQoLDA0ODw==")] {
            let (cipher, v2022) = method(m).unwrap();
            let key = if v2022 {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.decode(pw).unwrap()
            } else {
                password_key(pw, cipher.key_len())
            };
            let keys = Keys { cipher, v2022, key, identity: Vec::new() };
            let data: Vec<u8> = (0..3000u32).map(|i| (i * 13) as u8).collect();
            for cap in [7, 1 << 16] {
                let (client_io, mut server_io) = tokio::io::duplex(cap);
                let salt = vec![5u8; cipher.key_len()];
                let mut s = SsStream {
                    inner: Box::new(client_io),
                    v2022,
                    max_chunk: if v2022 { 0xffff } else { 0x3fff },
                    enc: Sealer { key: keys.session(&salt), nonce: 0 },
                    header: None,
                    request_salt: salt.clone(),
                    out: Vec::new(),
                    written: 0,
                    dec: None,
                    salt_len: cipher.key_len(),
                    raw: Default::default(),
                    plain: 0..0,
                    want: Want::Salt,
                    eof: false,
                    owner: keys.clone(),
                    salts: Default::default(),
                };
                let mut resp = vec![3u8; cipher.key_len()];
                let mut w = Sealer { key: keys.session(&resp), nonce: 0 };
                let mut chunks = data.chunks(3);
                if v2022 {
                    let first = chunks.next().unwrap();
                    let mut h = vec![1u8];
                    h.extend_from_slice(&unix_now().to_be_bytes());
                    h.extend_from_slice(&salt);
                    h.extend_from_slice(&(first.len() as u16).to_be_bytes());
                    w.seal(&h, &mut resp);
                    w.seal(first, &mut resp);
                }
                for c in chunks {
                    w.seal(&(c.len() as u16).to_be_bytes(), &mut resp);
                    w.seal(c, &mut resp);
                }
                let writer = async {
                    server_io.write_all(&resp).await.unwrap();
                    server_io.shutdown().await.unwrap();
                };
                let mut back = Vec::new();
                let (_, r) = tokio::join!(writer, s.read_to_end(&mut back));
                r.unwrap();
                assert_eq!(back, data, "{m} cap {cap}");
            }
        }
    }

    #[tokio::test]
    async fn aead_and_2022_roundtrip() {
        for (m, pw) in
            [("aes-128-gcm", "secret"), ("chacha20-ietf-poly1305", "secret"), ("2022-blake3-aes-128-gcm", "AAECAwQFBgcICQoLDA0ODw==")]
        {
            let (cipher, v2022) = method(m).unwrap();
            let key = if v2022 {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.decode(pw).unwrap()
            } else {
                password_key(pw, cipher.key_len())
            };
            let keys = Keys { cipher, v2022, key: key.clone(), identity: Vec::new() };
            let (client_io, mut server_io) = tokio::io::duplex(1 << 16);
            let mut salt = vec![7u8; cipher.key_len()];
            salt[0] = 9;
            let mut addr = Vec::new();
            put_addr(&mut addr, &Address::Domain("example.test".into()), 443);
            let mut s = SsStream {
                inner: Box::new(client_io),
                v2022,
                max_chunk: if v2022 { 0xffff } else { 0x3fff },
                enc: Sealer { key: keys.session(&salt), nonce: 0 },
                header: Some((salt.clone(), addr.clone())),
                request_salt: salt.clone(),
                out: Vec::new(),
                written: 0,
                dec: None,
                salt_len: cipher.key_len(),
                raw: Default::default(),
                plain: 0..0,
                want: Want::Salt,
                eof: false,
                owner: Keys { cipher, v2022, key: key.clone(), identity: Vec::new() },
                salts: Default::default(),
            };
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            s.write_all(b"hello").await.unwrap();
            s.flush().await.unwrap();
            let mut got = vec![0u8; 4096];
            let n = server_io.read(&mut got).await.unwrap();
            let got = &mut got[..n];
            let (csalt, rest) = got.split_at_mut(cipher.key_len());
            let k = keys.session(csalt);
            let mut ctr = 0u128;
            let mut rest = rest.to_vec();
            let mut next = |len: usize, rest: &mut Vec<u8>| -> Vec<u8> {
                let mut c: Vec<u8> = rest.drain(..len + TAG).collect();
                k.open_in_place(nonce(&mut ctr), Aad::empty(), &mut c).unwrap().to_vec()
            };
            let payload = if v2022 {
                let fixed = next(11, &mut rest);
                assert_eq!(fixed[0], 0);
                let var = next(u16::from_be_bytes([fixed[9], fixed[10]]) as usize, &mut rest);
                assert_eq!(&var[..addr.len()], &addr[..]);
                let padding = u16::from_be_bytes([var[addr.len()], var[addr.len() + 1]]) as usize;
                assert!((1..=900).contains(&padding));
                var[addr.len() + 2 + padding..].to_vec()
            } else {
                let l = next(2, &mut rest);
                let first = next(u16::from_be_bytes([l[0], l[1]]) as usize, &mut rest);
                assert_eq!(&first[..addr.len()], &addr[..]);
                first[addr.len()..].to_vec()
            };
            assert_eq!(payload, b"hello");
            let ssalt = vec![3u8; cipher.key_len()];
            let mut w = Sealer { key: keys.session(&ssalt), nonce: 0 };
            let mut resp = ssalt.clone();
            if v2022 {
                let mut h = vec![1u8];
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                h.extend_from_slice(&now.to_be_bytes());
                h.extend_from_slice(&salt);
                h.extend_from_slice(&5u16.to_be_bytes());
                w.seal(&h, &mut resp);
                w.seal(b"world", &mut resp);
            } else {
                w.seal(&5u16.to_be_bytes(), &mut resp);
                w.seal(b"world", &mut resp);
            }
            server_io.write_all(&resp).await.unwrap();
            let mut back = [0u8; 5];
            s.read_exact(&mut back).await.unwrap();
            assert_eq!(&back, b"world", "{m}");
        }
    }
}
