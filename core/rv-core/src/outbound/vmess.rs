use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::{SystemTime, UNIX_EPOCH};

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use md5::Md5;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use rv_config::{Outbound, Profile};
use rv_net::Protect;
use rv_vless::{Address, Command};
use sha2::{Digest, Sha256};
use sha3::digest::{ExtendableOutput, Update, XofReader};
use sha3::{Shake128, Shake128Reader};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use crate::transport::Stream;
use crate::{BoxStream, Error};

const TAG: usize = 16;
const MAX_PAYLOAD: usize = 8192 - TAG - 2 - 64;

pub struct VmessOutbound {
    cmd_key: [u8; 16],
    security: BodySecurity,
    bare: bool,
    stream: Stream,
}

impl VmessOutbound {
    pub fn new(profile: &Profile, ob: &Outbound, v: &rv_config::Vmess, protect: Option<Protect>) -> Result<Self, Error> {
        let security = BodySecurity::parse(&v.security)?;
        let mut hash = Md5::new();
        Digest::update(&mut hash, v.id);
        Digest::update(&mut hash, b"c48619fe-8f02-49e0-b9e9-edf763e17e21");
        Ok(Self {
            cmd_key: hash.finalize().into(),
            security,
            bare: matches!(ob.stream.security, rv_config::Security::None),
            stream: Stream::new(profile, ob, &v.address, v.port, protect)?,
        })
    }

    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    pub async fn open(&self, command: Command, dest: Address, port: u16) -> Result<BoxStream, Error> {
        let session = Session::new(&self.cmd_key, self.security, command, &dest, port)?;
        Ok(Box::new(VmessStream::new(self.stream.open().await?, session, self.bare)?))
    }

    pub async fn udp_session(
        &self,
        first: (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        self.udp_session_id(first, rx, reply, None).await
    }
    pub(crate) async fn udp_session_id(
        &self,
        first: (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
        gid: Option<[u8; 8]>,
    ) -> Result<(), Error> {
        let stream = crate::udp_setup(Box::pin(self.open(Command::Mux, Address::Domain("v1.mux.cool".into()), 666))).await?;
        super::vless::xudp_session(stream, first, rx, reply, gid).await
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySecurity {
    AesGcm,
    Chacha,
    None,
    Zero,
}

impl BodySecurity {
    fn parse(s: &str) -> Result<Self, Error> {
        Ok(match s {
            "auto" if cfg!(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "s390x")) => Self::AesGcm,
            "auto" | "chacha20-poly1305" => Self::Chacha,
            "aes-128-gcm" => Self::AesGcm,
            "none" => Self::None,
            "zero" => Self::Zero,
            other => return Err(Error::Config(format!("vmess: unknown security `{other}`"))),
        })
    }

    fn code(self) -> u8 {
        match self {
            Self::AesGcm => 3,
            Self::Chacha => 4,
            Self::None | Self::Zero => 5,
        }
    }

    fn options(self) -> u8 {
        match self {
            Self::AesGcm | Self::Chacha => 0x0d,
            Self::None => 0x05,
            Self::Zero => 0,
        }
    }
}

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("vmess: {message}"))
}

fn random(b: &mut [u8]) -> io::Result<()> {
    SystemRandom::new().fill(b).map_err(|_| io::Error::other("vmess: rng"))
}

fn nested_hash(data: &[u8], keys: &[&[u8]]) -> [u8; 32] {
    let Some((key, rest)) = keys.split_last() else {
        return Sha256::digest(data).into();
    };
    let mut block = [0u8; 64];
    debug_assert!(key.len() <= block.len());
    block[..key.len()].copy_from_slice(key);
    let mut input = Vec::with_capacity(64 + data.len());
    input.extend(block.iter().map(|b| b ^ 0x36));
    input.extend_from_slice(data);
    let inner = nested_hash(&input, rest);
    input.clear();
    input.extend(block.iter().map(|b| b ^ 0x5c));
    input.extend_from_slice(&inner);
    nested_hash(&input, rest)
}

fn kdf(key: &[u8], path: &[&[u8]]) -> [u8; 32] {
    let mut keys = Vec::with_capacity(path.len() + 1);
    keys.push(&b"VMess AEAD KDF"[..]);
    keys.extend_from_slice(path);
    nested_hash(key, &keys)
}

fn aes_key(key: &[u8]) -> io::Result<LessSafeKey> {
    UnboundKey::new(&aead::AES_128_GCM, key).map(LessSafeKey::new).map_err(|_| bad("AES key"))
}

fn seal(key: &LessSafeKey, nonce: [u8; 12], aad: &[u8], data: &mut Vec<u8>) -> io::Result<()> {
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad), data).map_err(|_| bad("encryption"))
}

fn open(key: &LessSafeKey, nonce: [u8; 12], aad: &[u8], data: &mut [u8]) -> io::Result<usize> {
    key.open_in_place(Nonce::assume_unique_for_key(nonce), Aad::from(aad), data).map(|b| b.len()).map_err(|_| bad("authentication failed"))
}

fn nonce(key: &[u8], path: &[&[u8]]) -> [u8; 12] {
    kdf(key, path)[..12].try_into().unwrap()
}

fn fnv(data: &[u8]) -> u32 {
    data.iter().fold(0x811c9dc5, |h, b| (h ^ *b as u32).wrapping_mul(0x01000193))
}

struct Session {
    security: BodySecurity,
    key: [u8; 16],
    iv: [u8; 16],
    response: u8,
    header: Vec<u8>,
}

impl Session {
    fn new(cmd_key: &[u8; 16], security: BodySecurity, command: Command, dest: &Address, port: u16) -> io::Result<Self> {
        let mut entropy = [0u8; 50];
        random(&mut entropy)?;
        let key: [u8; 16] = entropy[..16].try_into().unwrap();
        let iv: [u8; 16] = entropy[16..32].try_into().unwrap();
        let response = entropy[32];
        let padding = &entropy[34..34 + (entropy[33] & 15) as usize];
        let mut plain = vec![1];
        plain.extend_from_slice(&iv);
        plain.extend_from_slice(&key);
        plain.extend([response, security.options(), (padding.len() as u8) << 4 | security.code(), 0, command as u8]);
        if command != Command::Mux {
            plain.extend_from_slice(&port.to_be_bytes());
            match dest {
                Address::Ipv4(a) => {
                    plain.push(1);
                    plain.extend_from_slice(a);
                }
                Address::Ipv6(a) => {
                    plain.push(3);
                    plain.extend_from_slice(a);
                }
                Address::Domain(d) => {
                    if d.is_empty() || d.len() > 255 {
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, "vmess: bad domain length"));
                    }
                    plain.extend([2, d.len() as u8]);
                    plain.extend_from_slice(d.as_bytes());
                }
            }
        }
        plain.extend_from_slice(padding);
        plain.extend_from_slice(&fnv(&plain).to_be_bytes());
        let header = seal_header(cmd_key, &plain)?;
        Ok(Self { security, key, iv, response, header })
    }
}

fn seal_header(cmd_key: &[u8; 16], plain: &[u8]) -> io::Result<Vec<u8>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| io::Error::other("vmess: system clock"))?.as_secs();
    let mut auth_id = [0u8; 16];
    auth_id[..8].copy_from_slice(&now.to_be_bytes());
    random(&mut auth_id[8..12])?;
    let checksum = crc32fast::hash(&auth_id[..12]);
    auth_id[12..].copy_from_slice(&checksum.to_be_bytes());
    let auth_key = kdf(cmd_key, &[b"AES Auth ID Encryption"]);
    let cipher = aes::Aes128::new_from_slice(&auth_key[..16]).map_err(|_| bad("AuthID key"))?;
    cipher.encrypt_block((&mut auth_id).into());
    let mut conn_nonce = [0u8; 8];
    random(&mut conn_nonce)?;
    let path = |label: &'static [u8]| [label, &auth_id[..], &conn_nonce[..]];
    let mut len = (plain.len() as u16).to_be_bytes().to_vec();
    seal(
        &aes_key(&kdf(cmd_key, &path(b"VMess Header AEAD Key_Length"))[..16])?,
        nonce(cmd_key, &path(b"VMess Header AEAD Nonce_Length")),
        &auth_id,
        &mut len,
    )?;
    let mut payload = plain.to_vec();
    seal(
        &aes_key(&kdf(cmd_key, &path(b"VMess Header AEAD Key"))[..16])?,
        nonce(cmd_key, &path(b"VMess Header AEAD Nonce")),
        &auth_id,
        &mut payload,
    )?;
    let mut out = Vec::with_capacity(42 + payload.len());
    out.extend_from_slice(&auth_id);
    out.extend(len);
    out.extend(conn_nonce);
    out.extend(payload);
    Ok(out)
}

struct ChunkCodec {
    key: Option<LessSafeKey>,
    iv: [u8; 16],
    count: u16,
    left: Option<u32>,
    mask: Shake128Reader,
    padded: bool,
}

impl ChunkCodec {
    fn new(security: BodySecurity, key: &[u8; 16], iv: [u8; 16]) -> io::Result<Self> {
        let key = match security {
            BodySecurity::AesGcm => Some(aes_key(key)?),
            BodySecurity::Chacha => {
                let first = Md5::digest(key);
                let second = Md5::digest(first);
                let mut full = [0u8; 32];
                full[..16].copy_from_slice(&first);
                full[16..].copy_from_slice(&second);
                Some(LessSafeKey::new(UnboundKey::new(&aead::CHACHA20_POLY1305, &full).map_err(|_| bad("ChaCha key"))?))
            }
            BodySecurity::None | BodySecurity::Zero => None,
        };
        let mut shake = Shake128::default();
        Update::update(&mut shake, &iv);
        Ok(Self { key, iv, count: 0, left: None, mask: shake.finalize_xof(), padded: security.options() & 8 != 0 })
    }

    fn next_mask(&mut self) -> u16 {
        let mut b = [0u8; 2];
        self.mask.read(&mut b);
        u16::from_be_bytes(b)
    }

    fn padding(&mut self) -> usize {
        if self.padded { (self.next_mask() % 64) as usize } else { 0 }
    }

    fn guarded(mut self) -> Self {
        self.left = self.key.is_some().then_some(1 << 16);
        self
    }

    fn spent(&self) -> bool {
        self.left == Some(0)
    }

    fn next_nonce(&mut self) -> io::Result<[u8; 12]> {
        if let Some(left) = &mut self.left {
            if *left == 0 {
                return Err(io::Error::other("vmess: the connection used up all 65536 body chunks, a longer one would repeat a nonce"));
            }
            *left -= 1;
        }
        let mut n: [u8; 12] = self.iv[..12].try_into().unwrap();
        n[..2].copy_from_slice(&self.count.to_be_bytes());
        self.count = self.count.wrapping_add(1);
        Ok(n)
    }

    fn encode(&mut self, data: &[u8], out: &mut Vec<u8>) -> io::Result<()> {
        if self.spent() {
            self.next_nonce()?;
        }
        let padding = self.padding();
        let overhead = if self.key.is_some() { TAG } else { 0 };
        let size = data.len() + overhead + padding;
        let size = u16::try_from(size).map_err(|_| bad("chunk too large"))?;
        let mask = self.next_mask();
        out.extend_from_slice(&(size ^ mask).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(data);
        if self.key.is_some() {
            let nonce = self.next_nonce()?;
            let tag = self
                .key
                .as_ref()
                .unwrap()
                .seal_in_place_separate_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut out[start..])
                .map_err(|_| bad("body encryption"))?;
            out.extend_from_slice(tag.as_ref());
        }
        let pos = out.len();
        out.resize(pos + padding, 0);
        random(&mut out[pos..])?;
        Ok(())
    }

    fn decode_size(&mut self, bytes: [u8; 2]) -> io::Result<(usize, usize)> {
        let padding = self.padding();
        let size = (u16::from_be_bytes(bytes) ^ self.next_mask()) as usize;
        let overhead = if self.key.is_some() { TAG } else { 0 };
        if size < padding + overhead {
            return Err(bad("chunk length"));
        }
        Ok((size, padding))
    }

    fn decode(&mut self, data: &mut [u8], padding: usize) -> io::Result<usize> {
        let end = data.len().checked_sub(padding).ok_or_else(|| bad("chunk padding"))?;
        if self.key.is_some() {
            let nonce = self.next_nonce()?;
            open(self.key.as_ref().unwrap(), nonce, &[], &mut data[..end])
        } else {
            Ok(end)
        }
    }
}

#[derive(Clone, Copy)]
enum ReadState {
    HeaderLength,
    Header,
    Size,
    Body { padding: usize },
    Raw,
    Eof,
    Failed,
}

struct VmessStream {
    inner: BoxStream,
    tx: ChunkCodec,
    rx: ChunkCodec,
    raw: bool,
    response: u8,
    response_key: [u8; 16],
    response_iv: [u8; 16],
    pending: Vec<u8>,
    header_pending: bool,
    sent: usize,
    shutdown: bool,
    state: ReadState,
    wire: Vec<u8>,
    received: usize,
    plain: Vec<u8>,
    consumed: usize,
}

impl VmessStream {
    fn new(inner: BoxStream, session: Session, bare: bool) -> io::Result<Self> {
        let response_key = Sha256::digest(session.key)[..16].try_into().unwrap();
        let response_iv = Sha256::digest(session.iv)[..16].try_into().unwrap();
        let guard = |c: ChunkCodec| if bare { c.guarded() } else { c };
        Ok(Self {
            inner,
            tx: guard(ChunkCodec::new(session.security, &session.key, session.iv)?),
            rx: guard(ChunkCodec::new(session.security, &response_key, response_iv)?),
            raw: session.security == BodySecurity::Zero,
            response: session.response,
            response_key,
            response_iv,
            pending: session.header,
            header_pending: true,
            sent: 0,
            shutdown: false,
            state: ReadState::HeaderLength,
            wire: vec![0; 18],
            received: 0,
            plain: Vec::new(),
            consumed: 0,
        })
    }

    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.sent < self.pending.len() {
            let n = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.pending[self.sent..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.sent += n;
        }
        self.pending.clear();
        self.sent = 0;
        Poll::Ready(Ok(()))
    }

    fn expect(&mut self, state: ReadState, n: usize) {
        self.state = state;
        self.wire.resize(n, 0);
        self.received = 0;
    }

    fn poll_read_inner(&mut self, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if self.consumed < self.plain.len() {
                let n = out.remaining().min(self.plain.len() - self.consumed);
                out.put_slice(&self.plain[self.consumed..self.consumed + n]);
                self.consumed += n;
                return Poll::Ready(Ok(()));
            }
            match self.state {
                ReadState::Raw => return Pin::new(&mut self.inner).poll_read(cx, out),
                ReadState::Eof => return Poll::Ready(Ok(())),
                ReadState::Failed => return Poll::Ready(Err(bad("stream failed"))),
                _ => {}
            }
            while self.received < self.wire.len() {
                let mut buf = ReadBuf::new(&mut self.wire[self.received..]);
                ready!(Pin::new(&mut self.inner).poll_read(cx, &mut buf))?;
                let n = buf.filled().len();
                if n == 0 {
                    if self.received == 0 && matches!(self.state, ReadState::Size) {
                        self.state = ReadState::Eof;
                        return Poll::Ready(Ok(()));
                    }
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::UnexpectedEof, "vmess: truncated response")));
                }
                self.received += n;
            }
            match self.state {
                ReadState::HeaderLength => {
                    let key = aes_key(&kdf(&self.response_key, &[b"AEAD Resp Header Len Key"])[..16])?;
                    open(&key, nonce(&self.response_iv, &[b"AEAD Resp Header Len IV"]), &[], &mut self.wire)?;
                    let n = u16::from_be_bytes(self.wire[..2].try_into().unwrap()) as usize;
                    if !(4..=259).contains(&n) {
                        return Poll::Ready(Err(bad("response header length")));
                    }
                    self.expect(ReadState::Header, n + TAG);
                }
                ReadState::Header => {
                    let key = aes_key(&kdf(&self.response_key, &[b"AEAD Resp Header Key"])[..16])?;
                    let n = open(&key, nonce(&self.response_iv, &[b"AEAD Resp Header IV"]), &[], &mut self.wire)?;
                    if self.wire[0] != self.response {
                        return Poll::Ready(Err(bad("response marker")));
                    }
                    if n != 4 + self.wire[3] as usize {
                        return Poll::Ready(Err(bad("response command length")));
                    }
                    self.expect(if self.raw { ReadState::Raw } else { ReadState::Size }, 2);
                }
                ReadState::Size => {
                    let (n, padding) = self.rx.decode_size(self.wire[..2].try_into().unwrap())?;
                    self.expect(ReadState::Body { padding }, n);
                }
                ReadState::Body { padding } => {
                    let n = self.rx.decode(&mut self.wire, padding)?;
                    if n == 0 {
                        self.state = ReadState::Eof;
                        return Poll::Ready(Ok(()));
                    }
                    self.plain.clear();
                    self.plain.extend_from_slice(&self.wire[..n]);
                    self.consumed = 0;
                    self.expect(ReadState::Size, 2);
                }
                _ => unreachable!(),
            }
        }
    }
}

impl AsyncRead for VmessStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let result = self.poll_read_inner(cx, out);
        if matches!(result, Poll::Ready(Err(_))) {
            self.state = ReadState::Failed;
        }
        result
    }
}

impl AsyncWrite for VmessStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let s = self.get_mut();
        if s.shutdown {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if data.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if !s.header_pending || s.sent != 0 {
            ready!(s.poll_pending(cx))?;
        }
        let n = data.len().min(MAX_PAYLOAD);
        if s.raw {
            s.pending.extend_from_slice(&data[..n]);
        } else {
            s.tx.encode(&data[..n], &mut s.pending)?;
        }
        s.header_pending = false;
        let _ = s.poll_pending(cx)?;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_pending(cx))?;
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let s = self.get_mut();
        if !s.shutdown {
            if !s.raw && !s.tx.spent() {
                s.tx.encode(&[], &mut s.pending)?;
            }
            s.shutdown = true;
        }
        ready!(s.poll_pending(cx))?;
        Pin::new(&mut s.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn kdf_matches_xray_vectors() {
        let paths: &[(&[&[u8]], &str)] = &[
            (&[], "76dfb9fefccab1e16280787cb1ec533ff64c4919d01a3602e0e7b58c6d1edc21"),
            (&[b"AES Auth ID Encryption"], "ebde3a0b17bf94c86fa140917ec6a0c9773a777693a18a40bb09b38fb9b0b816"),
            (
                &[b"VMess Header AEAD Key", b"0123456789abcdef", b"nonce123"],
                "68e6bdc64e98172571abcc523119bbca718e9a35ddea6cfe60b89289894c222a",
            ),
            (&[b"AEAD Resp Header Len Key"], "479c6badcf9598809025e56a79d5757eb297e53fede01d78b9c851694cdefc0f"),
            (
                &[
                    b"VMess Header AEAD Key",
                    b"\x00\xff\x01\xfe\x02\xfd\x03\xfc\x04\xfb\x05\xfa\x06\xf9\x07\xf8",
                    b"\x00\x80\x01\x81\x02\x82\x03\x83",
                ],
                "2fe44049eb9c91820e6743559362b966a01520cf86478e84adb27e95b88edbaf",
            ),
        ];
        for (path, expected) in paths {
            let actual: String = kdf(b"0123456789abcdef", path).iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(actual, *expected);
        }
        assert_eq!(fnv(b"hello"), 0x4f9f2cab);
    }

    fn response(s: &VmessStream, marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut len = 4u16.to_be_bytes().to_vec();
        seal(
            &aes_key(&kdf(&s.response_key, &[b"AEAD Resp Header Len Key"])[..16]).unwrap(),
            nonce(&s.response_iv, &[b"AEAD Resp Header Len IV"]),
            &[],
            &mut len,
        )
        .unwrap();
        let mut header = vec![marker, 0, 0, 0];
        seal(
            &aes_key(&kdf(&s.response_key, &[b"AEAD Resp Header Key"])[..16]).unwrap(),
            nonce(&s.response_iv, &[b"AEAD Resp Header IV"]),
            &[],
            &mut header,
        )
        .unwrap();
        len.extend(header);
        let mut rx = ChunkCodec::new(BodySecurity::AesGcm, &s.response_key, s.response_iv).unwrap();
        for chunk in payload.chunks(MAX_PAYLOAD) {
            rx.encode(chunk, &mut len).unwrap();
        }
        rx.encode(&[], &mut len).unwrap();
        len
    }

    fn stream(inner: BoxStream) -> VmessStream {
        VmessStream::new(
            inner,
            Session::new(&[1; 16], BodySecurity::AesGcm, Command::Tcp, &Address::Domain("example.com".into()), 443).unwrap(),
            false,
        )
        .unwrap()
    }

    #[test]
    fn bare_stream_stops_before_a_body_nonce_repeats() {
        let mut tx = ChunkCodec::new(BodySecurity::AesGcm, &[2; 16], [3; 16]).unwrap().guarded();
        let mut rx = ChunkCodec::new(BodySecurity::AesGcm, &[2; 16], [3; 16]).unwrap().guarded();
        let mut wire = Vec::new();
        for i in 0..1u32 << 16 {
            wire.clear();
            tx.encode(&i.to_be_bytes(), &mut wire).unwrap();
            let (_, pad) = rx.decode_size(wire[..2].try_into().unwrap()).unwrap();
            let n = rx.decode(&mut wire[2..], pad).unwrap();
            assert_eq!(wire[2..2 + n], i.to_be_bytes());
        }
        assert!(tx.spent() && rx.spent());
        let len = wire.len();
        assert!(tx.encode(b"more", &mut wire).is_err());
        assert_eq!(wire.len(), len, "отказ ничего не дописал");
        let (_, pad) = rx.decode_size(wire[..2].try_into().unwrap()).unwrap();
        assert!(rx.decode(&mut wire[2..], pad).is_err());

        let mut free = ChunkCodec::new(BodySecurity::AesGcm, &[2; 16], [3; 16]).unwrap();
        for _ in 0..(1u32 << 16) + 1 {
            wire.clear();
            free.encode(b"x", &mut wire).unwrap();
        }
        let mut plain = ChunkCodec::new(BodySecurity::None, &[2; 16], [3; 16]).unwrap().guarded();
        assert!(plain.left.is_none());
        for _ in 0..(1u32 << 16) + 1 {
            wire.clear();
            plain.encode(b"x", &mut wire).unwrap();
        }
    }

    #[test]
    fn body_modes_and_corruption() {
        for mode in [BodySecurity::AesGcm, BodySecurity::Chacha, BodySecurity::None] {
            let mut tx = ChunkCodec::new(mode, &[2; 16], [3; 16]).unwrap();
            let mut rx = ChunkCodec::new(mode, &[2; 16], [3; 16]).unwrap();
            for data in [&b"hello"[..], &vec![7; MAX_PAYLOAD], &[]] {
                let mut wire = Vec::new();
                tx.encode(data, &mut wire).unwrap();
                let (size, pad) = rx.decode_size(wire[..2].try_into().unwrap()).unwrap();
                assert_eq!(wire.len(), 2 + size);
                let n = rx.decode(&mut wire[2..], pad).unwrap();
                assert_eq!(&wire[2..2 + n], data);
            }
            if mode != BodySecurity::None {
                let mut wire = Vec::new();
                tx.encode(b"tamper", &mut wire).unwrap();
                let (_, pad) = rx.decode_size(wire[..2].try_into().unwrap()).unwrap();
                wire[2] ^= 1;
                assert!(rx.decode(&mut wire[2..], pad).is_err());
            }
        }
    }

    #[tokio::test]
    async fn fragmented_response_and_empty_read() {
        let (inner, mut peer) = tokio::io::duplex(7);
        let mut s = stream(Box::new(inner));
        let data: Vec<_> = (0..20_000).map(|i| i as u8).collect();
        let wire = response(&s, s.response, &data);
        let sender = async {
            for b in wire {
                peer.write_all(&[b]).await.unwrap();
            }
        };
        let reader = async {
            assert_eq!(s.read(&mut []).await.unwrap(), 0);
            let mut back = Vec::new();
            s.read_to_end(&mut back).await.unwrap();
            assert_eq!(back, data);
            assert_eq!(s.read(&mut [0]).await.unwrap(), 0);
        };
        tokio::join!(sender, reader);
    }

    #[tokio::test]
    async fn rejects_header_marker_tag_and_truncation() {
        for case in 0..4 {
            let (inner, mut peer) = tokio::io::duplex(2048);
            let mut s = stream(Box::new(inner));
            let mut wire = response(&s, s.response ^ if case == 0 { 1 } else { 0 }, b"hello");
            match case {
                1 => wire[0] ^= 1,
                2 => wire[40] ^= 1,
                3 => {
                    wire.truncate(12);
                }
                _ => {}
            }
            peer.write_all(&wire).await.unwrap();
            peer.shutdown().await.unwrap();
            assert!(s.read_to_end(&mut Vec::new()).await.is_err(), "case {case}");
            assert!(s.read(&mut [0]).await.is_err(), "failed stream must stay failed");
        }
    }

    #[tokio::test]
    async fn partial_writes_flush_and_shutdown() {
        let (inner, mut peer) = tokio::io::duplex(3);
        let session = Session::new(&[1; 16], BodySecurity::Chacha, Command::Mux, &Address::Domain("v1.mux.cool".into()), 666).unwrap();
        let mut decoder = ChunkCodec::new(session.security, &session.key, session.iv).unwrap();
        let header2 = session.header.clone();
        let mut s = VmessStream::new(Box::new(inner), session, false).unwrap();
        let data = vec![0x5a; 50_000];
        let writer = async {
            assert_eq!(s.write(&[]).await.unwrap(), 0);
            s.write_all(&data).await.unwrap();
            s.flush().await.unwrap();
            s.shutdown().await.unwrap();
            assert!(s.write(b"x").await.is_err());
        };
        let reader = async {
            let mut got_header = vec![0; header2.len()];
            peer.read_exact(&mut got_header).await.unwrap();
            assert_eq!(got_header, header2);
            let mut back = Vec::new();
            loop {
                let mut size = [0; 2];
                peer.read_exact(&mut size).await.unwrap();
                let (n, padding) = decoder.decode_size(size).unwrap();
                let mut body = vec![0; n];
                peer.read_exact(&mut body).await.unwrap();
                let used = decoder.decode(&mut body, padding).unwrap();
                if used == 0 {
                    break;
                }
                back.extend_from_slice(&body[..used]);
            }
            assert_eq!(back, data);
        };
        tokio::join!(writer, reader);
    }
}
