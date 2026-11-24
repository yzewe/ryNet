use std::cell::RefCell;
use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};

use aes::cipher::{KeyIvInit, StreamCipher};
use ml_kem::kem::Decapsulate;
use ml_kem::{DecapsulationKey768, EncapsulationKey768, KeyExport};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use rv_config::VlessEncryption;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::BoxStream;

type Ctr = ctr::Ctr128BE<aes::Aes256>;

const MAX_NONCE: [u8; 12] = [0xff; 12];
const RECORD: usize = 8192;

fn derive_key(ctx: &[u8], key: &[u8]) -> [u8; 32] {
    use blake3::hazmat::HasherExt;
    let mut h = blake3::Hasher::new_from_context_key(&context_key(ctx));
    h.update(key);
    *h.finalize().as_bytes()
}

fn context_key(ctx: &[u8]) -> [u8; 32] {
    const IV: [u32; 8] = [0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19];
    const CHUNK_START: u32 = 1;
    const CHUNK_END: u32 = 2;
    const PARENT: u32 = 4;
    const ROOT: u32 = 8;
    const CONTEXT: u32 = 32;
    const PERM: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

    fn g(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
        s[a] = s[a].wrapping_add(s[b]).wrapping_add(x);
        s[d] = (s[d] ^ s[a]).rotate_right(16);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_right(12);
        s[a] = s[a].wrapping_add(s[b]).wrapping_add(y);
        s[d] = (s[d] ^ s[a]).rotate_right(8);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_right(7);
    }
    fn compress(cv: &[u32; 8], block: &[u32; 16], counter: u64, len: u32, flags: u32) -> [u32; 8] {
        let mut s = [
            cv[0],
            cv[1],
            cv[2],
            cv[3],
            cv[4],
            cv[5],
            cv[6],
            cv[7],
            IV[0],
            IV[1],
            IV[2],
            IV[3],
            counter as u32,
            (counter >> 32) as u32,
            len,
            flags,
        ];
        let mut m = *block;
        for r in 0..7 {
            g(&mut s, 0, 4, 8, 12, m[0], m[1]);
            g(&mut s, 1, 5, 9, 13, m[2], m[3]);
            g(&mut s, 2, 6, 10, 14, m[4], m[5]);
            g(&mut s, 3, 7, 11, 15, m[6], m[7]);
            g(&mut s, 0, 5, 10, 15, m[8], m[9]);
            g(&mut s, 1, 6, 11, 12, m[10], m[11]);
            g(&mut s, 2, 7, 8, 13, m[12], m[13]);
            g(&mut s, 3, 4, 9, 14, m[14], m[15]);
            if r < 6 {
                m = std::array::from_fn(|i| m[PERM[i]]);
            }
        }
        std::array::from_fn(|i| s[i] ^ s[i + 8])
    }
    fn words(b: &[u8]) -> [u32; 16] {
        let mut padded = [0u8; 64];
        padded[..b.len()].copy_from_slice(b);
        std::array::from_fn(|i| u32::from_le_bytes(padded[i * 4..i * 4 + 4].try_into().unwrap()))
    }
    let chunk = |data: &[u8], counter: u64| -> ([u32; 8], [u32; 16], u32, u32) {
        let mut cv = IV;
        let blocks: Vec<&[u8]> = if data.is_empty() { vec![&[][..]] } else { data.chunks(64).collect() };
        for (i, b) in blocks.iter().enumerate() {
            let mut f = CONTEXT;
            if i == 0 {
                f |= CHUNK_START;
            }
            if i == blocks.len() - 1 {
                return (cv, words(b), b.len() as u32, f | CHUNK_END);
            }
            cv = compress(&cv, &words(b), counter, 64, f);
        }
        unreachable!()
    };
    let chunks: Vec<&[u8]> = if ctx.is_empty() { vec![&[][..]] } else { ctx.chunks(1024).collect() };
    if chunks.len() == 1 {
        let (cv, block, len, f) = chunk(chunks[0], 0);
        let out = compress(&cv, &block, 0, len, f | ROOT);
        return std::array::from_fn(|i| out[i / 4].to_le_bytes()[i % 4]);
    }
    let mut stack: Vec<[u32; 8]> = Vec::new();
    let parent = |l: &[u32; 8], r: &[u32; 8]| -> [u32; 16] { std::array::from_fn(|i| if i < 8 { l[i] } else { r[i - 8] }) };
    for (n, c) in chunks[..chunks.len() - 1].iter().enumerate() {
        let (cv, block, len, f) = chunk(c, n as u64);
        let mut cv = compress(&cv, &block, n as u64, len, f);
        let mut total = n as u64 + 1;
        while total & 1 == 0 {
            let l = stack.pop().unwrap();
            cv = compress(&IV, &parent(&l, &cv), 0, 64, PARENT | CONTEXT);
            total >>= 1;
        }
        stack.push(cv);
    }
    let last = chunks.len() - 1;
    let (cv, block, len, f) = chunk(chunks[last], last as u64);
    let (mut cv, mut block, mut counter, mut len, mut flags) = (cv, block, last as u64, len, f);
    while let Some(l) = stack.pop() {
        let out = compress(&cv, &block, counter, len, flags);
        block = parent(&l, &out);
        cv = IV;
        counter = 0;
        len = 64;
        flags = PARENT | CONTEXT;
    }
    let out = compress(&cv, &block, counter, len, flags | ROOT);
    std::array::from_fn(|i| out[i / 4].to_le_bytes()[i % 4])
}

fn new_ctr(key: &[u8], iv: &[u8]) -> Ctr {
    let k = derive_key(b"VLESS", key);
    let iv: [u8; 16] = iv.try_into().expect("CTR IV is 16 bytes");
    Ctr::new(&k.into(), &iv.into())
}

fn rand<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    SystemRandom::new().fill(&mut b).expect("system RNG");
    b
}

fn rand_between(from: u32, to: u32) -> u32 {
    if to <= from {
        return from;
    }
    let r = u32::from_le_bytes(rand::<4>());
    (to - from).checked_add(1).map_or(r, |w| from + r % w)
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("vless encryption: {msg}"))
}

struct Aead {
    key: LessSafeKey,
    nonce: [u8; 12],
}

impl Aead {
    fn new(ctx: &[u8], key: &[u8]) -> Self {
        let k = derive_key(ctx, key);
        Aead { key: LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &k).expect("32-byte key")), nonce: [0; 12] }
    }

    fn next(&mut self, explicit: Option<[u8; 12]>) -> Nonce {
        let n = explicit.unwrap_or_else(|| {
            for i in (0..12).rev() {
                self.nonce[i] = self.nonce[i].wrapping_add(1);
                if self.nonce[i] != 0 {
                    break;
                }
            }
            self.nonce
        });
        Nonce::assume_unique_for_key(n)
    }

    fn seal(&mut self, out: &mut Vec<u8>, plain: &[u8], ad: &[u8]) {
        let start = out.len();
        out.extend_from_slice(plain);
        let n = self.next(None);
        let tag = self.key.seal_in_place_separate_tag(n, Aad::from(ad), &mut out[start..]).expect("seal");
        out.extend_from_slice(tag.as_ref());
    }

    fn open(&mut self, data: &mut [u8], ad: &[u8], explicit: Option<[u8; 12]>) -> io::Result<usize> {
        let n = self.next(explicit);
        self.key.open_in_place(n, Aad::from(ad), data).map(|p| p.len()).map_err(|_| bad("authentication failed"))
    }
}

fn encode_header(len: usize) -> [u8; 5] {
    [23, 3, 3, (len >> 8) as u8, len as u8]
}

fn decode_header(h: &[u8]) -> Result<usize, usize> {
    let mut l = (h[3] as usize) << 8 | h[4] as usize;
    if h[0] != 23 || h[1] != 3 || h[2] != 3 {
        l = 0;
    }
    if (17..=16640).contains(&l) { Ok(l) } else { Err(l) }
}

struct Xor {
    ctr: Ctr,
    skip: usize,
    header: Vec<u8>,
}

impl Xor {
    fn new(ctr: Ctr, skip: usize) -> Self {
        Xor { ctr, skip, header: Vec::with_capacity(5) }
    }

    fn out(&mut self, mut p: &mut [u8]) {
        loop {
            if p.len() <= self.skip {
                self.skip -= p.len();
                return;
            }
            p = &mut p[self.skip..];
            self.skip = 0;
            let need = 5 - self.header.len();
            if p.len() < need {
                self.header.extend_from_slice(p);
                self.ctr.apply_keystream(p);
                return;
            }
            let mut h = std::mem::take(&mut self.header);
            h.extend_from_slice(&p[..need]);
            self.skip = decode_header(&h).unwrap_or_else(|l| l);
            h.clear();
            self.header = h;
            self.ctr.apply_keystream(&mut p[..need]);
            p = &mut p[need..];
        }
    }

    fn inp(&mut self, mut p: &mut [u8]) {
        loop {
            if p.len() <= self.skip {
                self.skip -= p.len();
                return;
            }
            p = &mut p[self.skip..];
            self.skip = 0;
            let need = 5 - self.header.len();
            if p.len() < need {
                self.ctr.apply_keystream(p);
                self.header.extend_from_slice(p);
                return;
            }
            self.ctr.apply_keystream(&mut p[..need]);
            let mut h = std::mem::take(&mut self.header);
            h.extend_from_slice(&p[..need]);
            self.skip = decode_header(&h).unwrap_or_else(|l| l);
            h.clear();
            self.header = h;
            p = &mut p[need..];
        }
    }
}

enum NfsKey {
    X25519(PublicKey),
    MlKem(Box<EncapsulationKey768>),
}

struct Ticket {
    expire: Instant,
    pfs_key: [u8; 64],
    ticket: [u8; 16],
}

pub struct Client {
    keys: Vec<NfsKey>,
    raw: Vec<Vec<u8>>,
    hashes: Vec<[u8; 32]>,
    relays_len: usize,
    xor_mode: u8,
    zero_rtt: bool,
    padding_lens: Vec<[u32; 3]>,
    padding_gaps: Vec<[u32; 3]>,
    ticket: RefCell<Option<Ticket>>,
}

impl Client {
    pub fn new(cfg: &VlessEncryption) -> Result<Rc<Self>, String> {
        let mut keys = Vec::new();
        let mut relays_len = 0;
        for k in &cfg.keys {
            if k.len() == 32 {
                let pk: [u8; 32] = k[..].try_into().unwrap();
                keys.push(NfsKey::X25519(PublicKey::from(pk)));
                relays_len += 32 + 32;
            } else {
                let arr = k[..].try_into().map_err(|_| "vless encryption: bad ML-KEM-768 key".to_string())?;
                let ek = EncapsulationKey768::new(arr).map_err(|_| "vless encryption: bad ML-KEM-768 key".to_string())?;
                keys.push(NfsKey::MlKem(Box::new(ek)));
                relays_len += 1088 + 32;
            }
        }
        relays_len -= 32;
        let (padding_lens, padding_gaps) = if cfg.padding_lens.is_empty() {
            (vec![[100, 111, 1111], [50, 0, 3333]], vec![[75, 0, 111]])
        } else {
            (cfg.padding_lens.clone(), cfg.padding_gaps.clone())
        };
        Ok(Rc::new(Client {
            hashes: cfg.keys.iter().map(|k| *blake3::hash(k).as_bytes()).collect(),
            raw: cfg.keys.clone(),
            keys,
            relays_len,
            xor_mode: cfg.xor_mode,
            zero_rtt: cfg.zero_rtt,
            padding_lens,
            padding_gaps,
            ticket: RefCell::new(None),
        }))
    }

    pub async fn handshake(self: &Rc<Self>, mut io: BoxStream) -> io::Result<EncStream> {
        let iv: [u8; 16] = rand();
        let mut hello = Vec::with_capacity(16 + self.relays_len + 1250 + 1200);
        hello.extend_from_slice(&iv);
        let mut relays = vec![0u8; self.relays_len];
        let mut nfs_key = Vec::new();
        let mut last_ctr: Option<Ctr> = None;
        let mut off = 0;
        for (j, k) in self.keys.iter().enumerate() {
            let index = match k {
                NfsKey::X25519(pk) => {
                    let sk = StaticSecret::random();
                    relays[off..off + 32].copy_from_slice(PublicKey::from(&sk).as_bytes());
                    let ss = sk.diffie_hellman(pk);
                    if !ss.was_contributory() {
                        return Err(bad("X25519 low-order key"));
                    }
                    nfs_key = ss.as_bytes().to_vec();
                    32
                }
                NfsKey::MlKem(ek) => {
                    let m: [u8; 32] = rand();
                    let (ct, ss) = ek.encapsulate_deterministic(&m.into());
                    relays[off..off + 1088].copy_from_slice(&ct);
                    nfs_key = ss.to_vec();
                    1088
                }
            };
            if self.xor_mode > 0 {
                new_ctr(&self.raw[j], &iv).apply_keystream(&mut relays[off..off + index]);
            }
            if let Some(c) = last_ctr.as_mut() {
                c.apply_keystream(&mut relays[off..off + 32]);
            }
            if j == self.keys.len() - 1 {
                break;
            }
            let mut c = new_ctr(&nfs_key, &iv);
            let mut h = self.hashes[j + 1];
            c.apply_keystream(&mut h);
            relays[off + index..off + index + 32].copy_from_slice(&h);
            last_ctr = Some(c);
            off += index + 32;
        }
        hello.extend_from_slice(&relays);
        let mut nfs = Aead::new(&iv, &nfs_key);

        if self.zero_rtt {
            let t = self.ticket.borrow();
            if let Some(t) = t.as_ref().filter(|t| Instant::now() < t.expire) {
                let mut united = t.pfs_key.to_vec();
                united.extend_from_slice(&nfs_key);
                nfs.seal(&mut hello, &[0, 32], &[]);
                let at = hello.len();
                nfs.seal(&mut hello, &t.ticket, &[]);
                let aead = Aead::new(&hello[at..at + 32], &united);
                tracing::debug!(target: "rv::dial", "vless encryption: 0-rtt ticket");
                let xor = (self.xor_mode == 2).then(|| Xor::new(new_ctr(&united, &iv), 0));
                return Ok(EncStream::new(io, aead, None, united, Some(hello), Read::ServerRandom, xor, Some(self.clone())));
            }
        }

        let pfs_len = 18 + 1184 + 32 + 16;
        nfs.seal(&mut hello, &((pfs_len - 18) as u16).to_be_bytes(), &[]);
        let dk = DecapsulationKey768::from_seed(rand::<64>().into());
        let xsk = StaticSecret::random();
        let mut pfs_public = dk.encapsulation_key().to_bytes().to_vec();
        pfs_public.extend_from_slice(PublicKey::from(&xsk).as_bytes());
        nfs.seal(&mut hello, &pfs_public, &[]);

        let lens: Vec<usize> =
            self.padding_lens.iter().map(|y| if y[0] >= rand_between(0, 100) { rand_between(y[1], y[2]) as usize } else { 0 }).collect();
        let gaps: Vec<u64> =
            self.padding_gaps.iter().map(|y| if y[0] >= rand_between(0, 100) { rand_between(y[1], y[2]) as u64 } else { 0 }).collect();
        let padding: usize = lens.iter().sum::<usize>().max(34);
        nfs.seal(&mut hello, &((padding - 18) as u16).to_be_bytes(), &[]);
        nfs.seal(&mut hello, &vec![0u8; padding - 34], &[]);

        let mut rest = &hello[..];
        let mut first = 16 + self.relays_len + pfs_len;
        for (i, l) in lens.iter().enumerate() {
            let l = (first + l).min(rest.len());
            first = 0;
            if l > 0 {
                io.write_all(&rest[..l]).await?;
                io.flush().await?;
                rest = &rest[l..];
            }
            if let Some(&g) = gaps.get(i).filter(|&&g| g > 0) {
                tokio::time::sleep(Duration::from_millis(g)).await;
            }
        }
        if !rest.is_empty() {
            io.write_all(rest).await?;
            io.flush().await?;
        }

        let mut peer_pfs = vec![0u8; 1088 + 32 + 16];
        io.read_exact(&mut peer_pfs).await?;
        nfs.open(&mut peer_pfs, &[], Some(MAX_NONCE))?;
        let ct = ml_kem::ml_kem_768::Ciphertext::try_from(&peer_pfs[..1088]).map_err(|_| bad("bad ML-KEM ciphertext"))?;
        let mlkem_key = dk.decapsulate(&ct);
        let peer_x: [u8; 32] = peer_pfs[1088..1120].try_into().unwrap();
        let x_key = xsk.diffie_hellman(&PublicKey::from(peer_x));
        if !x_key.was_contributory() {
            return Err(bad("X25519 low-order key"));
        }
        let mut pfs_key = [0u8; 64];
        pfs_key[..32].copy_from_slice(&mlkem_key);
        pfs_key[32..].copy_from_slice(x_key.as_bytes());
        let mut united = pfs_key.to_vec();
        united.extend_from_slice(&nfs_key);
        let aead = Aead::new(&pfs_public, &united);
        let mut peer = Aead::new(&peer_pfs[..1120], &united);

        let mut ticket = [0u8; 32];
        io.read_exact(&mut ticket).await?;
        peer.open(&mut ticket, &[], None)?;
        let seconds = u16::from_be_bytes([ticket[0], ticket[1]]);
        let ticket16: [u8; 16] = ticket[..16].try_into().unwrap();
        if self.zero_rtt && seconds > 0 {
            *self.ticket.borrow_mut() =
                Some(Ticket { expire: Instant::now() + Duration::from_secs(seconds as u64), pfs_key, ticket: ticket16 });
        }

        let mut len = [0u8; 18];
        io.read_exact(&mut len).await?;
        peer.open(&mut len, &[], None)?;
        let pad = u16::from_be_bytes([len[0], len[1]]) as usize;

        tracing::debug!(target: "rv::dial", "vless encryption: 1-rtt handshake ticket_s={seconds} padding={pad}");
        let xor = (self.xor_mode == 2).then(|| Xor::new(new_ctr(&united, &iv), 0));
        let mut s = EncStream::new(io, aead, Some(peer), united, None, Read::Padding(pad), xor, None);
        if let Some(x) = s.xor.as_mut() {
            x.in_ctr = Some(Xor::new(new_ctr(&s.united, &ticket16), pad));
        }
        Ok(s)
    }
}

enum Read {
    ServerRandom,
    Padding(usize),
    Records,
}

struct XorPair {
    out: Xor,
    in_ctr: Option<Xor>,
}

pub struct EncStream {
    io: BoxStream,
    aead: Aead,
    peer: Option<Aead>,
    united: Vec<u8>,
    out: Vec<u8>,
    out_pos: usize,
    read: Read,
    inb: super::ReadBuffer,
    plain: std::ops::Range<usize>,
    xor: Option<XorPair>,
    client: Option<Rc<Client>>,
    eof: bool,
    read_direct: bool,
    write_direct: bool,
}

impl EncStream {
    #[allow(clippy::too_many_arguments)]
    fn new(
        io: BoxStream,
        aead: Aead,
        peer: Option<Aead>,
        united: Vec<u8>,
        prewrite: Option<Vec<u8>>,
        read: Read,
        xor: Option<Xor>,
        client: Option<Rc<Client>>,
    ) -> Self {
        EncStream {
            io,
            aead,
            peer,
            united,
            out: prewrite.unwrap_or_default(),
            out_pos: 0,
            read,
            inb: super::ReadBuffer::default(),
            plain: 0..0,
            xor: xor.map(|out| XorPair { out, in_ctr: None }),
            client,
            eof: false,
            read_direct: false,
            write_direct: false,
        }
    }

    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.out_pos < self.out.len() {
            let n = ready!(Pin::new(&mut self.io).poll_write(cx, &self.out[self.out_pos..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.out_pos += n;
        }
        self.out.clear();
        self.out_pos = 0;
        Poll::Ready(Ok(()))
    }

    fn parse(&mut self) -> io::Result<bool> {
        match self.read {
            Read::ServerRandom => {
                if self.inb.len() < 16 {
                    return Ok(false);
                }
                let r = self.inb.consume(16);
                let r: [u8; 16] = self.inb.get(r).try_into().expect("16 bytes");
                self.peer = Some(Aead::new(&r, &self.united));
                if let Some(x) = self.xor.as_mut() {
                    let mut ix = Xor::new(new_ctr(&self.united, &r), 0);
                    ix.inp(self.inb.data_mut());
                    x.in_ctr = Some(ix);
                }
                self.read = Read::Records;
                Ok(true)
            }
            Read::Padding(n) => {
                if self.inb.len() < n {
                    return Ok(false);
                }
                self.peer.as_mut().expect("1-RTT peer").open(&mut self.inb.data_mut()[..n], &[], None)?;
                self.inb.consume(n);
                self.read = Read::Records;
                Ok(true)
            }
            Read::Records => {
                if self.inb.len() < 5 {
                    return Ok(false);
                }
                let l = match decode_header(&self.inb.data()[..5]) {
                    Ok(l) => l,
                    Err(_) => {
                        if let Some(c) = self.client.take() {
                            let mut t = c.ticket.borrow_mut();
                            if t.as_ref().is_some_and(|t| self.united.starts_with(&t.pfs_key)) {
                                *t = None;
                            }
                            return Err(bad("new handshake needed"));
                        }
                        return Err(bad("invalid record header"));
                    }
                };
                if self.inb.len() < 5 + l {
                    return Ok(false);
                }
                self.client = None;
                let peer = self.peer.as_mut().expect("peer AEAD");
                let rekey = (peer.nonce == MAX_NONCE).then(|| Aead::new(&self.inb.data()[..5 + l], &self.united));
                let (h, rest) = self.inb.data_mut().split_at_mut(5);
                let n = peer.open(&mut rest[..l], h, None)?;
                if let Some(a) = rekey {
                    self.peer = Some(a);
                }
                let r = self.inb.consume(5 + l);
                self.plain = r.start + 5..r.start + 5 + n;
                Ok(true)
            }
        }
    }
}

impl AsyncRead for EncStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if me.read_direct {
            if !me.inb.is_empty() {
                let n = buf.remaining().min(me.inb.len());
                buf.put_slice(&me.inb.data()[..n]);
                me.inb.consume(n);
                return Poll::Ready(Ok(()));
            }
            let start = buf.filled().len();
            ready!(Pin::new(&mut me.io).poll_read(cx, buf))?;
            if let Some(ix) = me.xor.as_mut().and_then(|x| x.in_ctr.as_mut()) {
                ix.inp(&mut buf.filled_mut()[start..]);
            }
            return Poll::Ready(Ok(()));
        }
        loop {
            if !me.plain.is_empty() {
                let n = me.plain.len().min(buf.remaining());
                buf.put_slice(me.inb.get(me.plain.start..me.plain.start + n));
                me.plain.start += n;
                return Poll::Ready(Ok(()));
            }
            if me.parse()? {
                continue;
            }
            if me.eof {
                return Poll::Ready(if me.inb.is_empty() { Ok(()) } else { Err(bad("truncated record")) });
            }
            let got = ready!(me.inb.poll_fill(cx, &mut me.io))?;
            if got == 0 {
                me.eof = true;
                continue;
            }
            if let Some(x) = me.xor.as_mut()
                && let Some(ix) = x.in_ctr.as_mut()
            {
                let data = me.inb.data_mut();
                let at = data.len() - got;
                ix.inp(&mut data[at..]);
            }
        }
    }
}

impl AsyncWrite for EncStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let me = self.get_mut();
        if me.out_pos < me.out.len() && me.out.len() > RECORD {
            ready!(me.poll_drain(cx))?;
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = buf.len().min(RECORD);
        let start = me.out.len();
        if me.write_direct {
            me.out.extend_from_slice(&buf[..n]);
        } else {
            me.out.extend_from_slice(&encode_header(n + 16));
            let rekey = me.aead.nonce == MAX_NONCE;
            let mut h = [0u8; 5];
            h.copy_from_slice(&me.out[start..start + 5]);
            me.aead.seal(&mut me.out, &buf[..n], &h);
            if rekey {
                me.aead = Aead::new(&me.out[start..], &me.united);
            }
        }
        if let Some(x) = me.xor.as_mut() {
            x.out.out(&mut me.out[start..]);
        }
        let _ = me.poll_drain(cx)?;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        ready!(me.poll_drain(cx))?;
        Pin::new(&mut me.io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        ready!(me.poll_drain(cx))?;
        Pin::new(&mut me.io).poll_shutdown(cx)
    }
}

impl rv_vless::DirectCapable for EncStream {
    fn switch_read_to_direct(&mut self) -> Vec<u8> {
        self.read_direct = true;
        let mut rest = self.inb.get(self.plain.clone()).to_vec();
        rest.extend_from_slice(self.inb.data());
        self.inb.consume(self.inb.len());
        self.plain = 0..0;
        rest
    }
    fn switch_write_to_direct(&mut self) {
        self.write_direct = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_key_matches_blake3() {
        for n in [0usize, 1, 16, 63, 64, 65, 1023, 1024, 1025, 2048, 2049, 3072, 4096, 5000, 16645] {
            let text: String = (0..n).map(|i| (b'a' + (i * 7 % 26) as u8) as char).collect();
            assert_eq!(context_key(text.as_bytes()), blake3::hazmat::hash_derive_key_context(&text), "len {n}");
        }
        assert_eq!(derive_key(b"VLESS", b"k"), blake3::derive_key("VLESS", b"k"));
    }

    #[test]
    fn header_rules_like_xray() {
        assert_eq!(decode_header(&[23, 3, 3, 0, 17]), Ok(17));
        assert_eq!(decode_header(&[23, 3, 3, 0x41, 0x00]), Ok(16640));
        assert!(decode_header(&[23, 3, 3, 0x41, 0x01]).is_err());
        assert_eq!(decode_header(&[22, 3, 3, 0, 40]), Err(0));
    }

    #[tokio::test]
    async fn records_split_and_batched_reads() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let united = vec![1u8; 64];
        let data: Vec<u8> = (0..20_000u32).map(|i| (i * 7) as u8).collect();
        for xor in [false, true] {
            for cap in [5, 1 << 16] {
                let (a, mut b) = tokio::io::duplex(cap);
                let mut s = EncStream::new(
                    Box::new(a),
                    Aead::new(b"up", &united),
                    Some(Aead::new(b"down", &united)),
                    united.clone(),
                    None,
                    Read::Records,
                    xor.then(|| Xor::new(new_ctr(&united, &[3; 16]), 0)),
                    None,
                );
                if let Some(x) = s.xor.as_mut() {
                    x.in_ctr = Some(Xor::new(new_ctr(&united, &[2; 16]), 0));
                }
                let mut seal = Aead::new(b"down", &united);
                let mut wire = Vec::new();
                for c in data.chunks(100) {
                    let h = encode_header(c.len() + 16);
                    wire.extend_from_slice(&h);
                    seal.seal(&mut wire, c, &h);
                }
                if xor {
                    Xor::new(new_ctr(&united, &[2; 16]), 0).out(&mut wire);
                }
                let writer = async {
                    b.write_all(&wire).await.unwrap();
                    b.shutdown().await.unwrap();
                };
                let mut back = Vec::new();
                let (_, r) = tokio::join!(writer, s.read_to_end(&mut back));
                r.unwrap();
                assert_eq!(back, data, "xor {xor} cap {cap}");
            }
        }
    }

    #[test]
    fn xor_roundtrip_masks_only_headers() {
        let key = [7u8; 32];
        let iv = [9u8; 16];
        let mut wire = Vec::new();
        for n in [20usize, 100, 17] {
            wire.extend_from_slice(&encode_header(n));
            wire.extend(std::iter::repeat_n(0xAB, n));
        }
        let plain = wire.clone();
        let mut out = Xor::new(new_ctr(&key, &iv), 0);
        for c in wire.chunks_mut(3) {
            out.out(c);
        }
        assert_ne!(wire[..5], plain[..5]);
        assert_eq!(wire[5..25], plain[5..25]);
        let mut inp = Xor::new(new_ctr(&key, &iv), 0);
        for c in wire.chunks_mut(7) {
            inp.inp(c);
        }
        assert_eq!(wire, plain);
    }
}
