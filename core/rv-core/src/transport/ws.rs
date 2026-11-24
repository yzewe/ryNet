use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, ready};

use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use rv_xhttp::browser::{self, Headers, Variant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use crate::BoxStream;
use crate::outbound::ReadBuffer;

const FRAME_PAYLOAD: usize = 4096;
const MAX_WRITE: usize = 64 * 1024;
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub struct Settings {
    path: String,
    host: String,
    headers: Headers,
    early_data: usize,
    heartbeat: Option<std::time::Duration>,
}

impl Settings {
    pub fn new(cfg: &rv_config::Ws, default_host: &str) -> Self {
        let mut headers: Headers = cfg.headers.iter().map(|(k, v)| (canonical(k), v.clone())).collect();
        browser::apply_defaults(&mut headers, Variant::Ws);
        Self {
            path: normalized_path(&cfg.path),
            host: if cfg.host.is_empty() { default_host.to_string() } else { cfg.host.clone() },
            headers,
            early_data: cfg.early_data,
            heartbeat: (cfg.heartbeat_period > 0).then(|| std::time::Duration::from_secs(cfg.heartbeat_period.into())),
        }
    }
}

pub fn normalized_path(p: &str) -> String {
    if p.is_empty() {
        "/".into()
    } else if p.starts_with('/') {
        p.into()
    } else {
        format!("/{p}")
    }
}

pub fn canonical(k: &str) -> String {
    let mut up = true;
    k.chars()
        .map(|c| {
            let r = if up { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() };
            up = c == '-';
            r
        })
        .collect()
}

fn rand_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    let _ = SystemRandom::new().fill(&mut b);
    b
}

fn frame_mask() -> [u8; 4] {
    thread_local! {
        static POOL: std::cell::RefCell<([u8; 1024], usize)> = const { std::cell::RefCell::new(([0; 1024], 1024)) };
    }
    POOL.with_borrow_mut(|(buf, pos)| {
        if *pos + 4 > buf.len() {
            let _ = SystemRandom::new().fill(buf);
            *pos = 0;
        }
        let m = [buf[*pos], buf[*pos + 1], buf[*pos + 2], buf[*pos + 3]];
        *pos += 4;
        m
    })
}

pub async fn open(settings: Rc<Settings>, io: BoxStream) -> io::Result<WsStream> {
    let mut ws = WsStream {
        phase: Phase::Idle(io),
        settings,
        rbuf: ReadBuffer::default(),
        flush_pending: false,
        data_left: 0,
        fragmented: false,
        out: Out::default(),
        read_closed: false,
        close_sent: false,
        write_closed: false,
        heartbeat: None,
        read_error: None,
        text: false,
        text_final: false,
        utf8: Utf8::default(),
    };
    if ws.settings.early_data == 0 {
        let Phase::Idle(io) = std::mem::replace(&mut ws.phase, Phase::Taken) else { unreachable!() };
        let (io, rest) = handshake(io, ws.settings.clone(), None).await?;
        ws.phase = Phase::Open(io);
        ws.rbuf = ReadBuffer::from_vec(rest);
    }
    Ok(ws)
}

type Handshake = Pin<Box<dyn Future<Output = io::Result<(BoxStream, Vec<u8>)>>>>;

enum Phase {
    Idle(BoxStream),
    Handshaking(Handshake, usize),
    Open(BoxStream),
    Taken,
}

async fn handshake(mut io: BoxStream, s: Rc<Settings>, early: Option<Vec<u8>>) -> io::Result<(BoxStream, Vec<u8>)> {
    let key = base64::engine::general_purpose::STANDARD.encode(rand_bytes::<16>());
    let mut h = s.headers.clone();
    browser::set(&mut h, "Upgrade", "websocket");
    browser::set(&mut h, "Connection", "Upgrade");
    h.push(("Sec-WebSocket-Key".into(), key.clone()));
    h.push(("Sec-WebSocket-Version".into(), "13".into()));
    if let Some(ed) = &early {
        h.push(("Sec-WebSocket-Protocol".into(), base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(ed)));
    }
    io.write_all(&browser::http1_request("GET", &s.path, &s.host, &h)).await?;
    io.flush().await?;
    let (head, rest) = read_head(&mut io).await?;
    let head = String::from_utf8_lossy(&head);
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or_default();
    if status.split(' ').nth(1) != Some("101") {
        let body: String =
            String::from_utf8_lossy(&rest[..rest.len().min(120)]).chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        let server = lines
            .clone()
            .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("server")).map(|(_, v)| v.trim().to_string()))
            .unwrap_or_default();
        return Err(io::Error::other(format!("websocket: bad handshake `{status}` server={server:?} body={:?}", body.trim())));
    }
    let hdr = |name: &str| {
        head.split("\r\n").skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    let accept = {
        let d = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, format!("{key}{GUID}").as_bytes());
        base64::engine::general_purpose::STANDARD.encode(d.as_ref())
    };
    if !hdr("Upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        || !hdr("Connection").is_some_and(|v| v.to_ascii_lowercase().contains("upgrade"))
        || hdr("Sec-WebSocket-Accept").as_deref() != Some(accept.as_str())
    {
        return Err(io::Error::other("websocket: bad handshake"));
    }
    Ok((io, rest))
}

pub async fn read_head(io: &mut BoxStream) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(p + 4);
            return Ok((buf, rest));
        }
        if buf.len() > 16 * 1024 {
            return Err(io::Error::other("http: response header too long"));
        }
        let n = io.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

pub struct WsStream {
    phase: Phase,
    settings: Rc<Settings>,
    rbuf: ReadBuffer,
    flush_pending: bool,
    data_left: u64,
    fragmented: bool,
    out: Out,
    read_closed: bool,
    close_sent: bool,
    write_closed: bool,
    heartbeat: Option<Pin<Box<tokio::time::Sleep>>>,
    read_error: Option<io::Error>,
    text: bool,
    text_final: bool,
    utf8: Utf8,
}

#[derive(Default)]
struct Utf8 {
    pending: [u8; 4],
    used: usize,
}
impl Utf8 {
    fn feed(&mut self, mut data: &[u8], last: bool) -> bool {
        if self.used > 0 {
            while !data.is_empty() && self.used < 4 {
                self.pending[self.used] = data[0];
                self.used += 1;
                data = &data[1..];
                match std::str::from_utf8(&self.pending[..self.used]) {
                    Ok(_) => {
                        self.used = 0;
                        break;
                    }
                    Err(e) if e.error_len().is_some() => return false,
                    Err(_) => {}
                }
            }
            if self.used > 0 {
                return !last && self.used < 4;
            }
        }
        match std::str::from_utf8(data) {
            Ok(_) => true,
            Err(e) if e.error_len().is_none() && !last => {
                let tail = &data[e.valid_up_to()..];
                self.pending[..tail.len()].copy_from_slice(tail);
                self.used = tail.len();
                true
            }
            Err(_) => false,
        }
    }
}

fn valid_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999)
}

#[derive(Debug)]
struct InvalidText;
impl std::fmt::Display for InvalidText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("websocket: invalid text UTF-8")
    }
}
impl std::error::Error for InvalidText {}

#[derive(Default)]
struct Out {
    buf: Vec<u8>,
    frames: std::collections::VecDeque<usize>,
}
impl Out {
    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    fn len(&self) -> usize {
        self.buf.len()
    }
}

fn push_frame(o: &mut Out, first: u8, payload: &[u8]) {
    o.buf.reserve(payload.len() + 14);
    let before = o.buf.len();
    let out = &mut o.buf;
    out.push(first);
    let len = payload.len();
    if len < 126 {
        out.push(0x80 | len as u8);
    } else if len <= 0xffff {
        out.push(0x80 | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    let mask = frame_mask();
    out.extend_from_slice(&mask);
    let start = out.len();
    out.extend_from_slice(payload);
    apply_mask(&mut out[start..], mask);
    o.frames.push_back(o.buf.len() - before);
}

fn apply_mask(b: &mut [u8], mask: [u8; 4]) {
    let m = u64::from_ne_bytes([mask[0], mask[1], mask[2], mask[3], mask[0], mask[1], mask[2], mask[3]]);
    let (chunks, rest) = b.as_chunks_mut::<8>();
    for c in chunks {
        let v = u64::from_ne_bytes(*c) ^ m;
        c.copy_from_slice(&v.to_ne_bytes());
    }
    for (i, x) in rest.iter_mut().enumerate() {
        *x ^= mask[i % 4];
    }
}

fn push_message(out: &mut Out, data: &[u8]) {
    let n = data.len().div_ceil(FRAME_PAYLOAD).max(1);
    for (i, c) in data.chunks(FRAME_PAYLOAD).enumerate() {
        let opcode = if i == 0 { 0x2 } else { 0x0 };
        let fin = if i + 1 == n { 0x80 } else { 0 };
        push_frame(out, fin | opcode, c);
    }
}

impl WsStream {
    fn fail(&mut self, code: u16, error: io::Error) {
        if !self.close_sent {
            push_frame(&mut self.out, 0x88, &code.to_be_bytes());
            self.close_sent = true;
        }
        self.read_error = Some(error);
        self.read_closed = true;
        self.data_left = 0;
        self.rbuf.consume(self.rbuf.len());
        self.heartbeat = None;
    }
    fn io(&mut self) -> Option<&mut BoxStream> {
        match &mut self.phase {
            Phase::Open(io) => Some(io),
            _ => None,
        }
    }

    fn poll_open(&mut self, cx: &mut Context<'_>, early: Option<&[u8]>) -> Poll<io::Result<usize>> {
        loop {
            match std::mem::replace(&mut self.phase, Phase::Taken) {
                Phase::Open(io) => {
                    self.phase = Phase::Open(io);
                    return Poll::Ready(Ok(0));
                }
                Phase::Idle(io) => {
                    let ed = early.filter(|b| !b.is_empty() && b.len() <= self.settings.early_data).map(<[u8]>::to_vec);
                    let used = ed.as_ref().map_or(0, Vec::len);
                    self.phase = Phase::Handshaking(Box::pin(handshake(io, self.settings.clone(), ed)), used);
                }
                Phase::Handshaking(mut fut, used) => match fut.as_mut().poll(cx) {
                    Poll::Pending => {
                        self.phase = Phase::Handshaking(fut, used);
                        return Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Ready(Ok((io, rest))) => {
                        self.phase = Phase::Open(io);
                        self.rbuf = ReadBuffer::from_vec(rest);
                        return Poll::Ready(Ok(used));
                    }
                },
                Phase::Taken => return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            }
        }
    }

    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while let Some(&left) = self.out.frames.front() {
            let Phase::Open(io) = &mut self.phase else {
                return Poll::Ready(Err(io::ErrorKind::NotConnected.into()));
            };
            match ready!(Pin::new(io).poll_write(cx, &self.out.buf[..left]))? {
                0 => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
                n => {
                    self.out.buf.drain(..n);
                    if n == left {
                        self.out.frames.pop_front();
                    } else {
                        self.out.frames[0] = left - n;
                    }
                }
            }
        }
        if self.out.buf.capacity() != 0 {
            self.out.buf = Vec::new();
        }
        Poll::Ready(Ok(()))
    }

    fn parse(&mut self) -> io::Result<bool> {
        let b = self.rbuf.data();
        if b.len() < 2 {
            return Ok(false);
        }
        let opcode = b[0] & 0x0f;
        let fin = b[0] & 0x80 != 0;
        let masked = b[1] & 0x80 != 0;
        if b[0] & 0x70 != 0 || masked || (opcode >= 8 && (!fin || b[1] & 0x7f > 125)) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "websocket: invalid RSV, MASK or control FIN"));
        }
        let (len, mut off) = match b[1] & 0x7f {
            126 if b.len() >= 4 => (u16::from_be_bytes([b[2], b[3]]) as u64, 4),
            127 if b.len() >= 10 => (u64::from_be_bytes(b[2..10].try_into().unwrap()), 10),
            126 | 127 => return Ok(false),
            n => (n as u64, 2),
        };
        if len & (1u64 << 63) != 0 || (b[1] & 0x7f == 126 && len < 126) || (b[1] & 0x7f == 127 && len <= 65535) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "websocket: invalid payload length"));
        }
        let mask = if masked {
            if b.len() < off + 4 {
                return Ok(false);
            }
            let m = [b[off], b[off + 1], b[off + 2], b[off + 3]];
            off += 4;
            Some(m)
        } else {
            None
        };
        match opcode {
            0x0..=0x2 => {
                if (opcode == 0 && !self.fragmented) || (opcode != 0 && self.fragmented) {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "websocket: invalid continuation"));
                }
                self.fragmented = !fin;
                if opcode != 0 {
                    self.text = opcode == 1;
                    self.utf8 = Utf8::default();
                }
                self.text_final = fin;
                if len == 0 && self.text && !self.utf8.feed(&[], fin) {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, InvalidText));
                }
                if mask.is_some() {
                    return Err(io::Error::other("websocket: masked frame from server"));
                }
                self.rbuf.consume(off);
                self.data_left = len;
                Ok(true)
            }
            0x8..=0xa => {
                let end = off + len as usize;
                if len > 125 {
                    return Err(io::Error::other("websocket: bad control frame"));
                }
                if b.len() < end {
                    return Ok(false);
                }
                let mut payload = b[off..end].to_vec();
                if let Some(m) = mask {
                    apply_mask(&mut payload, m);
                }
                self.rbuf.consume(end);
                match opcode {
                    0x9 if !self.close_sent => push_frame(&mut self.out, 0x8a, &payload),
                    0x8 => {
                        if payload.len() == 1 {
                            return Err(io::Error::new(io::ErrorKind::InvalidData, "websocket: one-byte Close"));
                        }
                        if payload.len() >= 2 {
                            let code = u16::from_be_bytes([payload[0], payload[1]]);
                            if !valid_close_code(code) || std::str::from_utf8(&payload[2..]).is_err() {
                                return Err(io::Error::new(io::ErrorKind::InvalidData, "websocket: invalid Close code or reason"));
                            }
                        }
                        if !self.close_sent {
                            push_frame(&mut self.out, 0x88, &payload[..payload.len().min(2)]);
                            self.close_sent = true;
                        }
                        self.read_closed = true;
                    }
                    _ => {}
                }
                Ok(true)
            }
            _ => Err(io::Error::other("websocket: unknown opcode")),
        }
    }
}

fn readable(left: u64, avail: usize) -> usize {
    usize::try_from(left).unwrap_or(usize::MAX).min(avail)
}

impl AsyncRead for WsStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        ready!(me.poll_open(cx, None))?;
        if let Some(period) = me.settings.heartbeat
            && !me.close_sent
        {
            let t = me.heartbeat.get_or_insert_with(|| Box::pin(tokio::time::sleep(period)));
            if t.as_mut().poll(cx).is_ready() {
                push_frame(&mut me.out, 0x89, &[]);
                t.as_mut().reset(tokio::time::Instant::now() + period);
                let _ = t.as_mut().poll(cx);
            }
        }
        loop {
            if !me.out.is_empty() {
                if me.out.len() > MAX_WRITE + 16 * 1024 {
                    return Poll::Ready(Err(io::Error::other("websocket: control frames backlog")));
                }
                match me.poll_drain(cx) {
                    Poll::Ready(Ok(())) => me.flush_pending = true,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => {}
                }
            }
            if me.flush_pending
                && let Some(io) = me.io()
            {
                match Pin::new(io).poll_flush(cx) {
                    Poll::Ready(Ok(())) => me.flush_pending = false,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => {}
                }
            }
            if me.data_left > 0 && !me.rbuf.is_empty() {
                let n = readable(me.data_left, me.rbuf.len().min(buf.remaining()));
                if me.text && !me.utf8.feed(&me.rbuf.data()[..n], me.text_final && n as u64 == me.data_left) {
                    me.fail(1007, io::Error::new(io::ErrorKind::InvalidData, "websocket: invalid text UTF-8"));
                    continue;
                }
                buf.put_slice(&me.rbuf.data()[..n]);
                me.rbuf.consume(n);
                me.data_left -= n as u64;
                return Poll::Ready(Ok(()));
            }
            if me.read_closed {
                if (me.flush_pending || !me.out.is_empty()) && me.io().is_some() {
                    return Poll::Pending;
                }
                return Poll::Ready(me.read_error.take().map_or(Ok(()), Err));
            }
            if me.data_left == 0 {
                match me.parse() {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(e) => {
                        let code = if e.get_ref().is_some_and(|e| e.is::<InvalidText>()) { 1007 } else { 1002 };
                        me.fail(code, e);
                        continue;
                    }
                }
            }
            let Phase::Open(io) = &mut me.phase else { unreachable!("open") };
            if ready!(me.rbuf.poll_fill(cx, io))? == 0 {
                me.read_closed = true;
                return Poll::Ready(Ok(()));
            }
        }
    }
}

impl AsyncWrite for WsStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let me = &mut *self;
        let used = ready!(me.poll_open(cx, Some(buf)))?;
        if used > 0 {
            return Poll::Ready(Ok(used));
        }
        if me.close_sent || me.write_closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        ready!(me.poll_drain(cx))?;
        let n = buf.len().min(rv_xhttp::activity::XRAY_BUFFER);
        push_message(&mut me.out, &buf[..n]);
        let _ = me.poll_drain(cx)?;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        if me.io().is_none() {
            return Poll::Ready(Ok(()));
        }
        ready!(me.poll_drain(cx))?;
        Pin::new(me.io().unwrap()).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        if me.io().is_none() {
            me.write_closed = true;
            return Poll::Ready(Ok(()));
        }
        if me.write_closed {
            if !me.close_sent {
                push_frame(&mut me.out, 0x88, &1000u16.to_be_bytes());
                me.close_sent = true;
            }
            ready!(me.poll_drain(cx))?;
            return Pin::new(me.io().unwrap()).poll_shutdown(cx);
        }
        ready!(me.poll_drain(cx))?;
        ready!(Pin::new(me.io().unwrap()).poll_flush(cx))?;
        me.write_closed = true;
        Poll::Ready(Ok(()))
    }
}

#[cfg(fuzzing)]
#[path = "../fuzzing/ws.rs"]
pub mod fuzzing;

#[cfg(test)]
mod wire_tests {
    use super::*;
    #[test]
    fn frame_rest_past_usize_is_not_eof() {
        assert_eq!(readable(1 << 32, 5), 5);
        assert_eq!(readable((1 << 32) + 3, 10), 10);
        assert_eq!(readable(3, 10), 3);
    }
    #[tokio::test]
    async fn half_close_keeps_the_response_readable() {
        let (client, mut peer) = tokio::io::duplex(65536);
        let server = tokio::spawn(async move {
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(peer.read_u8().await.unwrap());
            }
            let text = std::str::from_utf8(&head).unwrap();
            let key = text
                .lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key")).map(|(_, v)| v.trim()))
                .unwrap();
            let hash = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, format!("{key}{GUID}").as_bytes());
            let accept = base64::engine::general_purpose::STANDARD.encode(hash);
            peer.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes()).await.unwrap();
            let mut frame = [0; 13];
            peer.read_exact(&mut frame).await.unwrap();
            assert_eq!(frame[0], 0x82);
            assert!(tokio::time::timeout(std::time::Duration::from_millis(30), peer.read_u8()).await.is_err());
            peer.write_all(b"\x82\x05reply").await.unwrap();
        });
        let settings = Rc::new(Settings::new(&rv_config::Ws::default(), "test.local"));
        let mut ws = open(settings, Box::new(client)).await.unwrap();
        ws.write_all(b"request").await.unwrap();
        ws.shutdown().await.unwrap();
        let mut response = [0; 5];
        ws.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"reply");
        server.await.unwrap();
        assert!(ws.write_all(b"after EOF").await.is_err());
    }
    async fn receive(frames: &[u8]) -> (io::Result<Vec<u8>>, Option<u16>) {
        let (client, mut peer) = tokio::io::duplex(65536);
        let frames = frames.to_vec();
        let server = tokio::spawn(async move {
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(peer.read_u8().await.unwrap());
            }
            let text = std::str::from_utf8(&head).unwrap();
            let key = text
                .lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key")).map(|(_, v)| v.trim()))
                .unwrap();
            let hash = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, format!("{key}{GUID}").as_bytes());
            let accept = base64::engine::general_purpose::STANDARD.encode(hash);
            peer.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes()).await.unwrap();
            peer.write_all(&frames).await.unwrap();
            let mut header = [0; 2];
            peer.read_exact(&mut header).await.unwrap();
            assert_eq!(header[0], 0x88, "a Close must precede the read error/EOF");
            assert_ne!(header[1] & 0x80, 0, "client Close must be masked");
            let len = usize::from(header[1] & 0x7f);
            let mut mask = [0; 4];
            peer.read_exact(&mut mask).await.unwrap();
            let mut payload = vec![0; len];
            peer.read_exact(&mut payload).await.unwrap();
            apply_mask(&mut payload, mask);
            (len >= 2).then(|| u16::from_be_bytes([payload[0], payload[1]]))
        });
        let settings = Rc::new(Settings::new(&rv_config::Ws::default(), "test.local"));
        let mut ws = open(settings, Box::new(client)).await.unwrap();
        let mut bytes = Vec::new();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), ws.read_to_end(&mut bytes)).await.unwrap();
        (result.map(|_| bytes), server.await.unwrap())
    }
    #[tokio::test]
    async fn invalid_frames_flush_protocol_close() {
        for frame in [
            b"\x88\x01\x00".as_slice(),
            b"\x88\x02\x03\xed",
            b"\x88\x03\x03\xe8\xff",
            b"\x89\x80\x00\x00\x00\x00",
            b"\x89\x7e\x00\x01A",
            b"\xc2\x00",
            b"\x09\x00",
            b"\x83\x00",
            b"\x80\x00",
            b"\x82\x7e\x00\x01A",
        ] {
            let (result, code) = receive(frame).await;
            assert!(result.is_err(), "accepted {frame:02x?}");
            assert_eq!(code, Some(1002), "frame {frame:02x?}");
        }
    }
    #[tokio::test]
    async fn frames_split_across_reads_arrive_whole() {
        let (mut wire, mut want, mut pings) = (Vec::new(), Vec::new(), 0);
        for (i, len) in [0usize, 1, 125, 126, 300, 65535, 65536, 70000, 3].into_iter().enumerate() {
            let data: Vec<u8> = (0..len).map(|j| (i * 31 + j) as u8).collect();
            wire.push(0x82);
            match len {
                0..=125 => wire.push(len as u8),
                126..=65535 => {
                    wire.push(126);
                    wire.extend_from_slice(&(len as u16).to_be_bytes());
                }
                _ => {
                    wire.push(127);
                    wire.extend_from_slice(&(len as u64).to_be_bytes());
                }
            }
            wire.extend_from_slice(&data);
            want.extend_from_slice(&data);
            if i % 3 == 0 {
                wire.extend_from_slice(b"\x89\x02hi");
                pings += 1;
            }
        }
        wire.extend_from_slice(b"\x88\x02\x03\xe8");
        let (client, mut peer) = tokio::io::duplex(64);
        let server = tokio::spawn(async move {
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(peer.read_u8().await.unwrap());
            }
            let text = std::str::from_utf8(&head).unwrap();
            let key = text
                .lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key")).map(|(_, v)| v.trim()))
                .unwrap();
            let hash = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, format!("{key}{GUID}").as_bytes());
            let accept = base64::engine::general_purpose::STANDARD.encode(hash);
            peer.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes()).await.unwrap();
            let (mut rd, mut wr) = tokio::io::split(peer);
            let writer = async move {
                for piece in wire.chunks(7) {
                    wr.write_all(piece).await.unwrap();
                    tokio::task::yield_now().await;
                }
                wr
            };
            let reader = async {
                let mut pongs = 0;
                loop {
                    let mut h = [0; 2];
                    rd.read_exact(&mut h).await.unwrap();
                    let mut mask = [0; 4];
                    rd.read_exact(&mut mask).await.unwrap();
                    let mut payload = vec![0; usize::from(h[1] & 0x7f)];
                    rd.read_exact(&mut payload).await.unwrap();
                    apply_mask(&mut payload, mask);
                    match h[0] {
                        0x8a => {
                            assert_eq!(payload, b"hi");
                            pongs += 1;
                        }
                        0x88 => return pongs,
                        f => panic!("frame {f:02x}"),
                    }
                }
            };
            let (_, pongs) = tokio::join!(writer, reader);
            pongs
        });
        let settings = Rc::new(Settings::new(&rv_config::Ws::default(), "test.local"));
        let mut ws = open(settings, Box::new(client)).await.unwrap();
        let (mut got, mut b) = (Vec::new(), [0; 3]);
        loop {
            let n = tokio::time::timeout(std::time::Duration::from_secs(5), ws.read(&mut b)).await.unwrap().unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&b[..n]);
        }
        assert!(got == want, "{} of {} bytes", got.len(), want.len());
        ws.shutdown().await.unwrap();
        assert_eq!(server.await.unwrap(), pings);
    }
    #[tokio::test]
    async fn utf8_is_validated_across_fragment_boundaries() {
        let (result, code) = receive(b"\x01\x01\xe2\x00\x01\x82\x80\x01\xac\x88\x02\x03\xe8").await;
        assert_eq!(result.unwrap(), "€".as_bytes());
        assert_eq!(code, Some(1000));
        for frames in [b"\x81\x01\xff".as_slice(), b"\x01\x01\xe2\x80\x00", b"\x01\x01\xe2\x80\x01A"] {
            let (result, code) = receive(frames).await;
            assert!(result.is_err());
            assert_eq!(code, Some(1007));
        }
    }
}
