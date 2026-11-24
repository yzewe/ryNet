use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use ring::rand::{SecureRandom, SystemRandom};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{Request, encode_request_header, parse_response_header};

const BUF_SIZE: usize = 8192;

const CMD_CONTINUE: u8 = 0;
const CMD_END: u8 = 1;
const CMD_DIRECT: u8 = 2;

const TLS13_SUPPORTED_VERSIONS: [u8; 6] = [0x00, 0x2b, 0x00, 0x02, 0x03, 0x04];
const TLS_CLIENT_HANDSHAKE_START: [u8; 2] = [0x16, 0x03];
const TLS_SERVER_HANDSHAKE_START: [u8; 3] = [0x16, 0x03, 0x03];
const TLS_APPLICATION_DATA_START: [u8; 3] = [0x17, 0x03, 0x03];

const SEED: [u32; 4] = [900, 500, 900, 256];

pub trait DirectCapable {
    fn switch_read_to_direct(&mut self) -> Vec<u8>;
    fn switch_write_to_direct(&mut self);
}

impl<S> DirectCapable for rv_tls::TlsStream<S> {
    fn switch_read_to_direct(&mut self) -> Vec<u8> {
        rv_tls::TlsStream::switch_read_to_direct(self)
    }
    fn switch_write_to_direct(&mut self) {
        rv_tls::TlsStream::switch_write_to_direct(self)
    }
}

#[derive(Debug)]
struct TrafficState {
    number_of_packet_to_filter: i32,
    enable_xtls: bool,
    is_tls12_or_above: bool,
    is_tls: bool,
    cipher: u16,
    remaining_server_hello: i32,
    within_padding_buffers: bool,
    downlink_reader_direct_copy: bool,
    remaining_command: i32,
    remaining_content: i32,
    remaining_padding: i32,
    current_command: i32,
    is_padding: bool,
    uplink_writer_direct_copy: bool,
    head: Vec<u8>,
    seen_uuid: bool,
}

impl TrafficState {
    fn new() -> Self {
        Self {
            number_of_packet_to_filter: 8,
            enable_xtls: false,
            is_tls12_or_above: false,
            is_tls: false,
            cipher: 0,
            remaining_server_hello: -1,
            within_padding_buffers: true,
            downlink_reader_direct_copy: false,
            remaining_command: -1,
            remaining_content: -1,
            remaining_padding: -1,
            current_command: 0,
            is_padding: true,
            uplink_writer_direct_copy: false,
            head: Vec::new(),
            seen_uuid: false,
        }
    }

    fn filter_tls<'a>(&mut self, chunks: impl IntoIterator<Item = &'a [u8]>) {
        for b in chunks {
            self.number_of_packet_to_filter -= 1;
            if b.len() >= 6 {
                if b[..3] == TLS_SERVER_HANDSHAKE_START && b[5] == 2 {
                    self.remaining_server_hello = ((b[3] as i32) << 8 | b[4] as i32) + 5;
                    self.is_tls12_or_above = true;
                    self.is_tls = true;
                    if b.len() >= 79 && self.remaining_server_hello >= 79 {
                        let sid_len = b[43] as usize;
                        if let Some(c) = b.get(43 + sid_len + 1..43 + sid_len + 3) {
                            self.cipher = u16::from_be_bytes([c[0], c[1]]);
                        }
                    }
                } else if b[..2] == TLS_CLIENT_HANDSHAKE_START && b[5] == 1 {
                    self.is_tls = true;
                }
            }
            if self.remaining_server_hello > 0 {
                let end = (self.remaining_server_hello as usize).min(b.len());
                self.remaining_server_hello -= b.len() as i32;
                if b[..end].windows(6).any(|w| w == TLS13_SUPPORTED_VERSIONS) {
                    if matches!(self.cipher, 0x1301..=0x1304) {
                        self.enable_xtls = true;
                    }
                    self.number_of_packet_to_filter = 0;
                    return;
                } else if self.remaining_server_hello <= 0 {
                    self.number_of_packet_to_filter = 0;
                    return;
                }
            }
        }
    }

    fn unpad(&mut self, uuid: &[u8; 16], b: &[u8], out: &mut Vec<u8>) {
        let joined;
        let mut b = if self.head.is_empty() {
            b
        } else {
            self.head.extend_from_slice(b);
            joined = std::mem::take(&mut self.head);
            &joined[..]
        };
        if self.remaining_command == -1 && self.remaining_content == -1 && self.remaining_padding == -1 {
            if b.len() >= 21 && b[..16] == uuid[..] {
                b = &b[16..];
                self.remaining_command = 5;
                self.seen_uuid = true;
            } else if !self.seen_uuid && b.len() < 21 && b[..b.len().min(16)] == uuid[..b.len().min(16)] {
                self.head = b.to_vec();
                return;
            } else {
                out.extend_from_slice(b);
                return;
            }
        }
        while !b.is_empty() {
            if self.remaining_command > 0 {
                let data = b[0] as i32;
                b = &b[1..];
                match self.remaining_command {
                    5 => self.current_command = data,
                    4 => self.remaining_content = data << 8,
                    3 => self.remaining_content |= data,
                    2 => self.remaining_padding = data << 8,
                    1 => self.remaining_padding |= data,
                    _ => unreachable!(),
                }
                self.remaining_command -= 1;
            } else if self.remaining_content > 0 {
                let n = (self.remaining_content as usize).min(b.len());
                out.extend_from_slice(&b[..n]);
                b = &b[n..];
                self.remaining_content -= n as i32;
            } else {
                let n = (self.remaining_padding.max(0) as usize).min(b.len());
                b = &b[n..];
                self.remaining_padding -= n as i32;
            }
            if self.remaining_command <= 0 && self.remaining_content <= 0 && self.remaining_padding <= 0 {
                if self.current_command == 0 {
                    self.remaining_command = 5;
                } else {
                    self.remaining_command = -1;
                    self.remaining_content = -1;
                    self.remaining_padding = -1;
                    out.extend_from_slice(b);
                    break;
                }
            }
        }
    }
}

fn rand_below(rng: &SystemRandom, n: u32) -> u32 {
    let mut b = [0u8; 4];
    rng.fill(&mut b).expect("system RNG");
    u32::from_le_bytes(b) % n
}

fn padding(rng: &SystemRandom, content: &[u8], cmd: u8, uuid: &mut Option<[u8; 16]>, long: bool, out: &mut Vec<u8>) {
    let content_len = content.len() as i32;
    let mut pad = if content_len < SEED[0] as i32 && long {
        rand_below(rng, SEED[1]) as i32 + SEED[2] as i32 - content_len
    } else {
        rand_below(rng, SEED[3]) as i32
    };
    let max = BUF_SIZE as i32 - 21 - content_len;
    if pad > max {
        pad = max;
    }
    let pad = pad.max(0) as usize;
    if let Some(u) = uuid.take() {
        out.extend_from_slice(&u);
    }
    out.extend_from_slice(&[cmd, (content_len >> 8) as u8, content_len as u8, (pad >> 8) as u8, pad as u8]);
    out.extend_from_slice(content);
    out.resize(out.len() + pad, 0);
}

fn is_complete_record(b: &[u8]) -> bool {
    let mut i = 0;
    while i < b.len() {
        if b.len() - i < 5 || b[i..i + 3] != TLS_APPLICATION_DATA_START {
            return false;
        }
        let len = u16::from_be_bytes([b[i + 3], b[i + 4]]) as usize;
        if len == 0 {
            return false;
        }
        i += 5;
        if b.len() - i < len {
            return false;
        }
        i += len;
    }
    true
}

fn reshape(chunks: Vec<&[u8]>) -> Vec<&[u8]> {
    let mut out = Vec::with_capacity(chunks.len() + 1);
    for c in chunks {
        if c.len() >= BUF_SIZE - 21 {
            let mut idx = c.windows(3).rposition(|w| w == TLS_APPLICATION_DATA_START).unwrap_or(0);
            if !(21..=BUF_SIZE - 21).contains(&idx) {
                idx = BUF_SIZE / 2;
            }
            out.push(&c[..idx]);
            out.push(&c[idx..]);
        } else {
            out.push(c);
        }
    }
    out
}

pub struct VisionStream<S> {
    inner: S,
    uuid: [u8; 16],
    state: TrafficState,
    rng: SystemRandom,
    write_once_uuid: Option<[u8; 16]>,
    header: Option<Vec<u8>>,
    wbuf: Vec<u8>,
    wpos: usize,
    ends: std::collections::VecDeque<usize>,
    flushing: bool,
    write_direct: bool,
    resp: Option<Vec<u8>>,
    rtmp: Box<[u8]>,
    rout: Vec<u8>,
    rpos: usize,
    read_direct: bool,
}

impl<S> VisionStream<S> {
    pub fn new(inner: S, req: &Request) -> io::Result<Self> {
        let mut header = Vec::with_capacity(64);
        encode_request_header(req, &mut header)?;
        Ok(Self {
            inner,
            uuid: req.id,
            state: TrafficState::new(),
            rng: SystemRandom::new(),
            write_once_uuid: Some(req.id),
            header: Some(header),
            wbuf: Vec::new(),
            wpos: 0,
            ends: Default::default(),
            flushing: false,
            write_direct: false,
            resp: Some(Vec::with_capacity(8)),
            rtmp: vec![0u8; BUF_SIZE].into_boxed_slice(),
            rout: Vec::new(),
            rpos: 0,
            read_direct: false,
        })
    }

    pub fn get_ref(&self) -> &S {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }
    pub fn is_fully_direct(&self) -> bool {
        self.read_direct && self.write_direct
    }

    fn encode_write(&mut self, data: &[u8]) {
        self.wbuf.reserve(data.len() + 1024);
        let start = self.wbuf.len();
        let first = self.header.is_some();
        let mut ends = Vec::new();
        self.encode_blocks(data, &mut ends);
        if first {
            ends = (start + BUF_SIZE..self.wbuf.len()).step_by(BUF_SIZE).chain([self.wbuf.len()]).collect();
        }
        self.ends.extend(ends);
    }

    fn encode_blocks(&mut self, data: &[u8], ends: &mut Vec<usize>) {
        let out = &mut self.wbuf;
        if let Some(h) = self.header.take() {
            out.extend_from_slice(&h);
        }
        let chunks: Vec<&[u8]> = data.chunks(BUF_SIZE).collect();
        let s = &mut self.state;
        if s.number_of_packet_to_filter > 0 {
            s.filter_tls(chunks.iter().copied());
        }
        if !s.is_padding {
            for c in &chunks {
                out.extend_from_slice(c);
                ends.push(out.len());
            }
            return;
        }
        let complete = is_complete_record(data);
        let chunks = reshape(chunks);
        let mut long_padding = s.is_tls;
        let last = chunks.len() - 1;
        let mut i = 0;
        while i < chunks.len() {
            let b = chunks[i];
            if s.is_tls && b.len() >= 6 && b[..3] == TLS_APPLICATION_DATA_START && complete {
                if s.enable_xtls {
                    s.uplink_writer_direct_copy = true;
                }
                let mut cmd = CMD_CONTINUE;
                if i == last {
                    cmd = if s.enable_xtls { CMD_DIRECT } else { CMD_END };
                }
                padding(&self.rng, b, cmd, &mut self.write_once_uuid, true, out);
                ends.push(out.len());
                s.is_padding = false;
                long_padding = false;
                i += 1;
                continue;
            } else if !s.is_tls12_or_above && s.number_of_packet_to_filter <= 1 {
                s.is_padding = false;
                padding(&self.rng, b, CMD_END, &mut self.write_once_uuid, long_padding, out);
                ends.push(out.len());
                for rest in &chunks[i + 1..] {
                    out.extend_from_slice(rest);
                    ends.push(out.len());
                }
                return;
            }
            let mut cmd = CMD_CONTINUE;
            if i == last && !s.is_padding {
                cmd = if s.enable_xtls { CMD_DIRECT } else { CMD_END };
            }
            padding(&self.rng, b, cmd, &mut self.write_once_uuid, long_padding, out);
            ends.push(out.len());
            i += 1;
        }
    }

    fn encode_empty(&mut self) {
        if let Some(h) = self.header.take() {
            self.wbuf.extend_from_slice(&h);
            padding(&self.rng, &[], CMD_CONTINUE, &mut self.write_once_uuid, true, &mut self.wbuf);
            self.ends.push_back(self.wbuf.len());
        }
    }
}

impl<S: AsyncWrite + DirectCapable + Unpin> VisionStream<S> {
    fn poll_switch_write(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(Pin::new(&mut self.inner).poll_flush(cx))?;
        self.inner.switch_write_to_direct();
        self.write_direct = true;
        if self.wbuf.is_empty() {
            self.wbuf = Vec::new();
        }
        tracing::trace!("vision: uplink direct");
        self.state.uplink_writer_direct_copy = false;
        Poll::Ready(Ok(()))
    }

    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            if self.flushing {
                ready!(Pin::new(&mut self.inner).poll_flush(cx))?;
                self.flushing = false;
            }
            if self.wpos >= self.wbuf.len() {
                break;
            }
            let end = self.ends.front().copied().unwrap_or(self.wbuf.len());
            while self.wpos < end {
                let n = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.wbuf[self.wpos..end]))?;
                if n == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                self.wpos += n;
            }
            if self.ends.pop_front().is_some() {
                self.flushing = true;
            }
        }
        self.wbuf = Vec::new();
        self.wpos = 0;
        self.ends.clear();
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + AsyncWrite + DirectCapable + Unpin> AsyncWrite for VisionStream<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        ready!(this.poll_drain(cx))?;
        if this.write_direct {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }
        if this.state.uplink_writer_direct_copy {
            ready!(this.poll_switch_write(cx))?;
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = buf.len().min(if this.header.is_some() { BUF_SIZE } else { 2 * BUF_SIZE });
        this.encode_write(&buf[..n]);
        let _ = this.poll_drain(cx)?;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.header.is_some() {
            this.encode_empty();
        }
        ready!(this.poll_drain(cx))?;
        if this.state.uplink_writer_direct_copy {
            ready!(this.poll_switch_write(cx))?;
        }
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.header.is_some() {
            this.encode_empty();
        }
        ready!(this.poll_drain(cx))?;
        if this.state.uplink_writer_direct_copy {
            ready!(this.poll_switch_write(cx))?;
        }
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

impl<S: AsyncRead + AsyncWrite + DirectCapable + Unpin> AsyncRead for VisionStream<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if this.rpos < this.rout.len() {
                let n = (this.rout.len() - this.rpos).min(buf.remaining());
                buf.put_slice(&this.rout[this.rpos..this.rpos + n]);
                this.rpos += n;
                if this.rpos == this.rout.len() {
                    this.rout.clear();
                    this.rpos = 0;
                }
                return Poll::Ready(Ok(()));
            }
            if this.read_direct {
                if !this.rtmp.is_empty() {
                    this.rtmp = Vec::new().into_boxed_slice();
                    this.rout = Vec::new();
                }
                return Pin::new(&mut this.inner).poll_read(cx, buf);
            }

            if this.rtmp.is_empty() {
                this.rtmp = vec![0u8; BUF_SIZE].into_boxed_slice();
            }
            let mut rb = ReadBuf::new(&mut this.rtmp);
            let Poll::Ready(r) = Pin::new(&mut this.inner).poll_read(cx, &mut rb) else {
                this.rtmp = Box::default();
                this.rout = Vec::new();
                this.rpos = 0;
                return Poll::Pending;
            };
            if let Err(e) = r {
                let s = &this.state;
                return Poll::Ready(Err(io::Error::new(
                    e.kind(),
                    format!(
                        "{e} [vision pad={} filter={} xtls={} tls13={} cmd={} up_direct={}]",
                        s.within_padding_buffers,
                        s.number_of_packet_to_filter,
                        s.enable_xtls,
                        s.is_tls12_or_above,
                        s.current_command,
                        this.write_direct,
                    ),
                )));
            }
            let n = rb.filled().len();
            if n == 0 {
                return Poll::Ready(if this.resp.is_some() {
                    Err(io::Error::new(io::ErrorKind::UnexpectedEof, "vless: no response header"))
                } else {
                    Ok(())
                });
            }
            let mut data = &this.rtmp[..n];

            let mut hdr_rest = Vec::new();
            if let Some(resp) = this.resp.as_mut() {
                resp.extend_from_slice(data);
                match parse_response_header(resp)? {
                    None => continue,
                    Some(h) => {
                        hdr_rest = resp.split_off(h);
                        this.resp = None;
                        data = &[];
                    }
                }
            }
            let chunk: &[u8] = if hdr_rest.is_empty() { data } else { &hdr_rest };
            if chunk.is_empty() {
                continue;
            }

            let s = &mut this.state;
            let start = this.rout.len();
            if s.within_padding_buffers || s.number_of_packet_to_filter > 0 {
                s.unpad(&this.uuid, chunk, &mut this.rout);
                if s.remaining_command > 0
                    || !s.head.is_empty()
                    || s.remaining_content > 0
                    || s.remaining_padding > 0
                    || s.current_command == 0
                {
                    s.within_padding_buffers = true;
                } else if s.current_command == 1 {
                    s.within_padding_buffers = false;
                } else if s.current_command == 2 {
                    s.within_padding_buffers = false;
                    s.downlink_reader_direct_copy = true;
                }
            } else {
                this.rout.extend_from_slice(chunk);
            }
            if s.number_of_packet_to_filter > 0 && this.rout.len() > start {
                s.filter_tls([&this.rout[start..]]);
            }
            if s.downlink_reader_direct_copy {
                let input = this.inner.switch_read_to_direct();
                this.rout.extend_from_slice(&input);
                this.read_direct = true;
                tracing::trace!(pending = input.len(), "vision: downlink direct");
            }
        }
    }
}

#[cfg(kani)]
#[path = "proofs/vision.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/vision.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests;
