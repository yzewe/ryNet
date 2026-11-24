use super::*;
use crate::{Address, Command, Flow};
use std::collections::VecDeque;
use std::task::Waker;

const UUID: [u8; 16] = [0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe, 0x0f];

fn state_ok(s: &TrafficState) {
    assert!((-1..=5).contains(&s.remaining_command));
    assert!((-1..=0xffff).contains(&s.remaining_content));
    assert!((-1..=0xffff).contains(&s.remaining_padding));
    assert!(s.head.len() < 21);
}

fn filter_ok(s: &TrafficState, total: usize) {
    assert!((-1..=8).contains(&s.number_of_packet_to_filter));
    assert!(s.remaining_server_hello >= -1 - total as i32);
    if s.enable_xtls {
        assert!(matches!(s.cipher, 0x1301..=0x1304) && s.is_tls12_or_above);
    }
}

fn pieces(mut rest: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    while let Some((&n, tail)) = rest.split_first() {
        let (a, b) = tail.split_at(usize::from(n).min(tail.len()));
        out.push(a);
        rest = b;
    }
    out
}

fn request() -> Request {
    Request { id: UUID, flow: Flow::Vision, command: Command::Tcp, address: Address::Domain("fuzz.test".into()), port: 443 }
}

fn block_on<T>(limit: usize, mut f: impl FnMut(&mut Context<'_>) -> Poll<T>) -> T {
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..limit {
        if let Poll::Ready(v) = f(&mut cx) {
            return v;
        }
    }
    panic!("нет готовности за {limit} опросов");
}

struct Inner {
    reads: VecDeque<Vec<u8>>,
    stall: bool,
    tick: bool,
    step: usize,
    written: Vec<u8>,
    pending: Vec<u8>,
}

impl Inner {
    fn new(reads: VecDeque<Vec<u8>>, stall: bool, step: usize, pending: Vec<u8>) -> Self {
        Self { reads, stall, tick: false, step, written: Vec::new(), pending }
    }

    fn stalled(&mut self) -> bool {
        self.tick = !self.tick;
        self.stall && self.tick
    }
}

impl AsyncRead for Inner {
    fn poll_read(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if self.stalled() {
            return Poll::Pending;
        }
        if let Some(front) = self.reads.front_mut() {
            let n = front.len().min(buf.remaining());
            buf.put_slice(&front[..n]);
            front.drain(..n);
            if front.is_empty() {
                self.reads.pop_front();
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Inner {
    fn poll_write(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if self.stalled() {
            return Poll::Pending;
        }
        let n = buf.len().min(self.step);
        self.written.extend_from_slice(&buf[..n]);
        Poll::Ready(Ok(n))
    }
    fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.stalled() {
            return Poll::Pending;
        }
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl DirectCapable for Inner {
    fn switch_read_to_direct(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
    fn switch_write_to_direct(&mut self) {}
}

pub fn unpad(data: &[u8]) {
    let mut s = TrafficState::new();
    let (mut out, mut fed) = (Vec::new(), 0);
    for c in pieces(data) {
        s.unpad(&UUID, c, &mut out);
        fed += c.len();
        state_ok(&s);
        assert!(out.len() + s.head.len() <= fed);
    }
}

pub fn roundtrip(data: &[u8]) {
    let Some((&ctl, rest)) = data.split_first() else { return };
    let ps = pieces(rest);
    let (contents, tail) = ps.split_at((usize::from(ctl & 7) + 1).min(ps.len()));
    let empty: [&[u8]; 1] = [&[]];
    let contents = if contents.is_empty() { &empty[..] } else { contents };
    let tail = tail.concat();
    let rng = SystemRandom::new();
    let mut once = Some(UUID);
    let mut wire = Vec::new();
    for (i, c) in contents.iter().enumerate() {
        let cmd = if i + 1 < contents.len() {
            CMD_CONTINUE
        } else if ctl & 8 != 0 {
            CMD_DIRECT
        } else {
            CMD_END
        };
        padding(&rng, c, cmd, &mut once, ctl & 16 != 0 && i == 0, &mut wire);
    }
    let end = wire.len();
    wire.extend_from_slice(&tail);
    let want = [contents.concat(), tail].concat();
    let step = [1, 2, 3, 5, 21, 100, 1000, 4096][usize::from(ctl >> 5)];
    let chunks: Vec<&[u8]> = wire.chunks(step).collect();
    let reparsed = (0..chunks.len()).any(|i| i * step >= end && chunks[i].len() >= 21 && chunks[i][..16] == UUID);

    let mut s = TrafficState::new();
    let mut out = Vec::new();
    for c in &chunks {
        s.unpad(&UUID, c, &mut out);
        state_ok(&s);
    }
    if !reparsed {
        assert!(out == want, "unpad: на выходе не то, что упаковано");
        assert_eq!(s.remaining_command, -1);
    }

    let mut reads: VecDeque<Vec<u8>> = chunks.iter().map(|c| c.to_vec()).collect();
    let first = reads.pop_front().unwrap_or_default();
    reads.push_front([&[0, 0][..], &first].concat());
    let mut v = VisionStream::new(Inner::new(reads, false, usize::MAX, Vec::new()), &request()).unwrap();
    let mut got = Vec::new();
    let mut buf = [0u8; 1000];
    loop {
        let mut rb = ReadBuf::new(&mut buf);
        block_on(1, |cx| Pin::new(&mut v).poll_read(cx, &mut rb)).expect("чтение из памяти");
        if rb.filled().is_empty() {
            break;
        }
        got.extend_from_slice(rb.filled());
        state_ok(&v.state);
    }
    if !reparsed {
        assert!(got == want, "VisionStream: на выходе не то, что упаковано");
    }
}

pub fn filter(data: &[u8]) {
    let Some((&ctl, rest)) = data.split_first() else { return };
    let ps = pieces(rest);
    let mut s = TrafficState::new();
    let (mut i, mut step, mut total) = (0, 0, 0);
    while s.number_of_packet_to_filter > 0 && i < ps.len() {
        let two = ctl >> (step % 8) & 1 != 0 && i + 1 < ps.len();
        let batch = &ps[i..i + 1 + usize::from(two)];
        i += batch.len();
        step += 1;
        total += batch.iter().map(|c| c.len()).sum::<usize>();
        s.filter_tls(batch.iter().copied());
        filter_ok(&s, total);
    }
}

pub fn complete_record(data: &[u8]) {
    let mut i = 0;
    let tiles = loop {
        if i == data.len() {
            break true;
        }
        match data.get(i..i + 5) {
            Some(h) if h[..3] == TLS_APPLICATION_DATA_START => {
                let len = usize::from(u16::from_be_bytes([h[3], h[4]]));
                if len == 0 || data.len() - i - 5 < len {
                    break false;
                }
                i += 5 + len;
            }
            _ => break false,
        }
    };
    assert_eq!(is_complete_record(data), tiles);
    if data.is_empty() {
        return;
    }
    for size in [BUF_SIZE - 21, BUF_SIZE - 1, BUF_SIZE] {
        let big: Vec<u8> = data.iter().copied().cycle().take(size).collect();
        let parts = reshape(vec![&big[..]]);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.len() <= BUF_SIZE - 21));
        assert!(parts.concat() == big);
    }
}

pub fn downlink(data: &[u8]) {
    let Some((&ctl, rest)) = data.split_first() else { return };
    let reads: VecDeque<Vec<u8>> = pieces(rest).into_iter().filter(|p| !p.is_empty()).map(<[u8]>::to_vec).collect();
    let received = reads.iter().map(Vec::len).sum::<usize>() + 7;
    let mut v = VisionStream::new(Inner::new(reads, ctl & 1 != 0, usize::MAX, b"pending".to_vec()), &request()).unwrap();
    let mut buf = vec![0u8; [1, 3, 64, 8192][usize::from(ctl >> 1 & 3)]];
    let limit = 2 * data.len() + 16;
    let mut got = 0;
    loop {
        let mut rb = ReadBuf::new(&mut buf);
        match block_on(limit, |cx| Pin::new(&mut v).poll_read(cx, &mut rb)) {
            Ok(()) if rb.filled().is_empty() => return,
            Ok(()) => got += rb.filled().len(),
            Err(_) => return,
        }
        state_ok(&v.state);
        assert!(v.state.number_of_packet_to_filter >= 0 && got <= received);
    }
}

pub fn uplink(data: &[u8]) {
    let Some((&ctl, rest)) = data.split_first() else { return };
    let req = request();
    let mut header = Vec::new();
    encode_request_header(&req, &mut header).unwrap();
    let step = [1, 7, 100, 1000, 4096, 9000, 20000, usize::MAX][usize::from(ctl >> 2 & 7)];
    let step = if ctl & 2 != 0 { step.max(64) } else { step };
    let mut v = VisionStream::new(Inner::new(VecDeque::new(), ctl & 1 != 0, step, Vec::new()), &req).unwrap();
    let mut app = Vec::new();
    for p in pieces(rest) {
        let w = if ctl & 2 != 0 { p.repeat(p.len()) } else { p.to_vec() };
        let mut off = 0;
        while off < w.len() {
            let limit = 4 * (w.len() + v.wbuf.len()) + 4096;
            let n = block_on(limit, |cx| Pin::new(&mut v).poll_write(cx, &w[off..])).expect("запись в память");
            assert!(n > 0);
            off += n;
        }
        app.extend_from_slice(&w);
        if ctl & 32 != 0 {
            let limit = 4 * v.wbuf.len() + 4096;
            block_on(limit, |cx| Pin::new(&mut v).poll_flush(cx)).expect("сброс в память");
        }
    }
    let limit = 4 * v.wbuf.len() + 4096;
    block_on(limit, |cx| Pin::new(&mut v).poll_flush(cx)).expect("сброс в память");
    let wire = &v.inner.written;
    assert!(wire.starts_with(&header));
    let mut s = TrafficState::new();
    let mut out = Vec::new();
    s.unpad(&UUID, &wire[header.len()..], &mut out);
    assert!(out == app, "сервер снял паддинг и получил не то, что записало приложение");
}
