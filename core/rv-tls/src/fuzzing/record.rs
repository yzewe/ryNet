use super::*;
use crate::keysched::{HashAlg, Secret, Suite, suite_by_id};
use std::task::Waker;

struct Wire<'a> {
    data: &'a [u8],
    step: usize,
    stall: bool,
    tick: bool,
}

impl<'a> Wire<'a> {
    fn new(data: &'a [u8], ctl: u8) -> Self {
        let k = usize::from(ctl >> 2);
        Self { data, step: if k == 0 { usize::MAX } else { k * k }, stall: ctl & 2 != 0, tick: false }
    }
}

impl AsyncRead for Wire<'_> {
    fn poll_read(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if self.stall {
            self.tick = !self.tick;
            if self.tick {
                return Poll::Pending;
            }
        }
        let n = self.data.len().min(buf.remaining()).min(self.step);
        buf.put_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Wire<'_> {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
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

pub fn records(data: &[u8]) {
    let Some((&ctl, input)) = data.split_first() else { return };
    let mut conn = Conn::new(Wire::new(input, ctl));
    conn.want_ccs = ctl & 1 != 0;
    let len_at = |p: usize| usize::from(u16::from_be_bytes([input[p + 3], input[p + 4]]));
    let limit = 2 * input.len() + 16;
    let (mut pos, mut ccs_left) = (0, MAX_CCS);
    loop {
        if !conn.want_ccs {
            while ccs_left > 0 && pos + 6 <= input.len() && input[pos] == CT_CCS && len_at(pos) == 1 && input[pos + 5] == 1 {
                pos += 6;
                ccs_left -= 1;
            }
        }
        match block_on(limit, |cx| conn.poll_record(cx)) {
            Ok(Some(rec)) => {
                let len = len_at(pos);
                assert_eq!(rec.ctype, input[pos]);
                assert!(rec.ctype != CT_APPDATA && (conn.want_ccs || rec.ctype != CT_CCS));
                assert_eq!(rec.end - rec.start, len);
                assert_eq!(conn.slice(rec.start, rec.end), &input[pos + 5..pos + 5 + len]);
                pos += 5 + len;
            }
            Ok(None) => {
                assert_eq!(pos, input.len());
                return;
            }
            Err(Error::Protocol("truncated record")) => {
                let rest = input.len() - pos;
                assert!(rest > 0 && (rest < 5 || rest < 5 + len_at(pos)));
                return;
            }
            Err(Error::Protocol("encrypted record before keys")) => {
                assert_eq!(input[pos], CT_APPDATA);
                return;
            }
            Err(Error::Protocol("unexpected CCS")) => {
                assert!(!conn.want_ccs && input[pos] == CT_CCS);
                assert!(ccs_left == 0 || len_at(pos) != 1 || input[pos + 5] != 1);
                return;
            }
            Err(Error::Record(_)) => {
                assert!(len_at(pos) > MAX_CT);
                return;
            }
            Err(e) => panic!("без ключей такой ошибки быть не может: {e}"),
        }
    }
}

pub fn read_app(data: &[u8]) {
    let Some((&ctl, input)) = data.split_first() else { return };
    let mut conn = Conn::new(Wire::new(input, ctl));
    conn.want_ccs = ctl & 1 != 0;
    let limit = 2 * input.len() + 16;
    let mut out = [0u8; 64];
    let mut rb = ReadBuf::new(&mut out);
    let r = block_on(limit, |cx| conn.poll_read_app(cx, &mut rb));
    assert!(rb.filled().is_empty());
    if r.is_ok() {
        let mut rb = ReadBuf::new(&mut out);
        assert!(block_on(limit, |cx| conn.poll_read_app(cx, &mut rb)).is_ok() && rb.filled().is_empty());
    }
}

fn direction(mode: u8) -> Direction {
    let gcm = Suite { aead: &ring::aead::AES_128_GCM, hash: HashAlg::Sha256 };
    match mode & 3 {
        0 => Direction::new(suite_by_id(0x1301).unwrap(), Secret::zeros(32)),
        1 => Direction::new(suite_by_id(0x1303).unwrap(), Secret::zeros(32)),
        2 => Direction::new_tls12(gcm, &[0; 16], &[0; 4]),
        _ => Direction::new_tls12_cbc(gcm, &[0; 16], ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, &[0; 20]),
    }
}

fn seal(tx: &mut Direction, ctype: u8, body: &[u8], wire: &mut Vec<u8>) {
    let start = wire.len();
    wire.extend_from_slice(&[ctype, 3, 3, 0, 0]);
    wire.resize(wire.len() + tx.explicit_nonce_len(), 0);
    wire.extend_from_slice(body);
    if tx.is_cbc() {
        tx.seal_cbc(wire, start, ctype);
        return;
    }
    let len = (wire.len() - start - 5 + TAG_LEN) as u16;
    wire[start + 3..start + 5].copy_from_slice(&len.to_be_bytes());
    let header: [u8; 5] = wire[start..start + 5].try_into().unwrap();
    let tag = tx.seal(&header, &mut wire[start + 5..]);
    wire.extend_from_slice(tag.as_ref());
}

pub fn keyed(data: &[u8]) {
    let [ctl, mode, switch_at, rest @ ..] = data else { return };
    let mut rest = rest;
    let mut tx = direction(*mode);
    let tls12 = tx.is_tls12();
    let (mut wire, mut app, mut ends) = (Vec::new(), Vec::new(), Vec::new());
    while let [kind, n, tail @ ..] = rest {
        let (body, tail) = tail.split_at(usize::from(*n).min(tail.len()));
        rest = tail;
        if kind & 0x80 != 0 {
            wire.extend_from_slice(body);
            continue;
        }
        let ctype = if tls12 { kind & 0x3f } else { CT_APPDATA };
        seal(&mut tx, ctype, body, &mut wire);
        let content = if tls12 {
            (ctype == CT_APPDATA).then_some(body)
        } else {
            body.iter().rposition(|&b| b != 0).filter(|&t| body[t] == CT_APPDATA).map(|t| &body[..t])
        };
        if let Some(c) = content.filter(|c| !c.is_empty()) {
            app.extend_from_slice(c);
            ends.push((app.len(), wire.len()));
        }
        if kind & 0x40 != 0 && !tls12 {
            tx.update();
        }
    }
    let mut conn = Conn::new(Wire::new(&wire, *ctl));
    conn.rx = Some(direction(*mode));
    conn.tx = Some(direction(*mode));
    let limit = 2 * wire.len() + 16;
    let mut buf = vec![0u8; [1, 5, 16, 100, 1000, 4096, 16384, 20000][usize::from(mode >> 2 & 7)]];
    let mut got = Vec::new();
    for reads in 1.. {
        let mut rb = ReadBuf::new(&mut buf);
        match block_on(limit, |cx| conn.poll_read_app(cx, &mut rb)) {
            Ok(()) if rb.filled().is_empty() => return,
            Ok(()) => got.extend_from_slice(rb.filled()),
            Err(_) => return,
        }
        assert!(got.len() <= app.len() && got[..] == app[..got.len()], "приложению ушло не то, что прислал сервер");
        block_on(limit, |cx| conn.poll_flush_app(cx)).expect("запись в память");
        if reads == usize::from(*switch_at) {
            let mut tail = conn.switch_read_to_direct();
            loop {
                let mut rb = ReadBuf::new(&mut buf);
                block_on(limit, |cx| conn.poll_read_app(cx, &mut rb)).expect("прямое чтение из памяти");
                if rb.filled().is_empty() {
                    break;
                }
                tail.extend_from_slice(rb.filled());
            }
            let &(app_end, wire_end) = ends.iter().find(|(a, _)| *a >= got.len()).expect("отданное — из записей");
            assert!(tail == [&app[got.len()..app_end], &wire[wire_end..]].concat(), "прямое чтение потеряло или повторило байты");
            return;
        }
    }
}

pub fn post_handshake(data: &[u8]) {
    let Some((&ctl, mut rest)) = data.split_first() else { return };
    let mut conn = Conn::new(Wire::new(&[], 0));
    if ctl & 1 != 0 {
        conn.rx = Some(direction(0));
    }
    let mut fed = Vec::new();
    while let Some((&n, tail)) = rest.split_first() {
        let (chunk, tail) = tail.split_at(usize::from(n).min(tail.len()));
        rest = tail;
        fed.extend_from_slice(chunk);
        conn.post_hs.extend_from_slice(chunk);
        if conn.process_post_handshake().is_err() {
            return;
        }
        let left = &conn.post_hs;
        assert!(fed.ends_with(left) && left.len() <= 1 << 16);
        if left.len() >= 4 {
            assert!(left.len() < 4 + u32::from_be_bytes([0, left[1], left[2], left[3]]) as usize);
        }
    }
}
