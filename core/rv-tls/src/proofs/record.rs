use super::*;
use std::task::Waker;

fn no_format(_: std::fmt::Arguments<'_>) -> String {
    String::new()
}

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

struct Wire<'a>(&'a [u8]);

impl AsyncRead for Wire<'_> {
    fn poll_read(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let n: usize = kani::any();
        kani::assume(n >= 1);
        let n = n.min(self.0.len()).min(buf.remaining());
        buf.put_slice(&self.0[..n]);
        self.0 = &self.0[n..];
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

const N: usize = 16;

fn small_conn(io: Wire<'_>) -> Conn<Wire<'_>> {
    Conn {
        io,
        rbuf: vec![0; 2 * N],
        rpos: 0,
        rend: 0,
        pt_start: 0,
        pt_end: 0,
        rx: None,
        want_ccs: false,
        ccs_left: MAX_CCS,
        hello_sent: false,
        read_direct: false,
        read_eof: false,
        records: 0,
        post_hs: Vec::new(),
        key_update_requested: false,
        wbuf: Vec::new(),
        wpos: 0,
        sealed_end: 0,
        open: false,
        tx: None,
        write_direct: false,
        write_closed: false,
        app_records: 0,
        app_bytes: 0,
        open_limit: MAX_PT,
        record_limit: MAX_PT,
    }
}

#[kani::proof]
#[kani::unwind(24)]
#[kani::stub(std::fmt::format, no_format)]
fn plaintext_records_frame_input() {
    let buf: [u8; N] = kani::any();
    let input = any_slice(&buf);
    let mut conn = small_conn(Wire(input));
    conn.want_ccs = kani::any();
    let mut cx = Context::from_waker(Waker::noop());
    let mut pos = 0;
    for _ in 0..N / 5 + 1 {
        if !conn.want_ccs {
            while pos + 5 <= input.len() && input[pos] == CT_CCS {
                let len = u16::from_be_bytes([input[pos + 3], input[pos + 4]]) as usize;
                if pos + 5 + len > input.len() || input[pos + 5..pos + 5 + len] != [1] {
                    break;
                }
                pos += 5 + len;
            }
        }
        let Poll::Ready(r) = conn.poll_record(&mut cx) else { panic!("заглушка не возвращает Pending") };
        match r {
            Ok(Some(rec)) => {
                let len = u16::from_be_bytes([input[pos + 3], input[pos + 4]]) as usize;
                assert_eq!(rec.ctype, input[pos]);
                assert!(rec.ctype != CT_APPDATA);
                assert_eq!(rec.end - rec.start, len);
                assert_eq!(conn.slice(rec.start, rec.end), &input[pos + 5..pos + 5 + len]);
                pos += 5 + len;
            }
            Ok(None) => {
                assert_eq!(pos, input.len());
                return;
            }
            Err(Error::Protocol("truncated record")) => {
                assert!(pos < input.len());
                return;
            }
            Err(Error::Protocol("encrypted record before keys")) => {
                assert_eq!(input[pos], CT_APPDATA);
                return;
            }
            Err(Error::Protocol("unexpected CCS")) => {
                let len = u16::from_be_bytes([input[pos + 3], input[pos + 4]]) as usize;
                assert!(!conn.want_ccs && input[pos] == CT_CCS && input[pos + 5..pos + 5 + len] != [1]);
                return;
            }
            Err(Error::Record(_)) => {
                let len = u16::from_be_bytes([input[pos + 3], input[pos + 4]]) as usize;
                assert!(len > MAX_CT);
                return;
            }
            Err(_) => panic!("другой ошибки без ключей быть не может"),
        }
    }
}

#[kani::proof]
#[kani::unwind(8)]
#[kani::stub(std::fmt::format, no_format)]
fn oversized_record_rejected() {
    let mut hdr: [u8; 5] = kani::any();
    let len: u16 = kani::any();
    kani::assume(len as usize > MAX_CT);
    hdr[3..5].copy_from_slice(&len.to_be_bytes());
    let mut conn = small_conn(Wire(&hdr));
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..6 {
        match conn.poll_record(&mut cx) {
            Poll::Ready(Err(Error::Record(_))) => return,
            Poll::Ready(Err(Error::Protocol("truncated record"))) => unreachable!(),
            Poll::Ready(_) => panic!("запись длиннее MAX_CT не должна приниматься"),
            Poll::Pending => unreachable!(),
        }
    }
}

#[kani::proof]
#[kani::unwind(24)]
#[kani::stub(std::fmt::format, no_format)]
fn read_app_any_input() {
    let buf: [u8; N] = kani::any();
    let mut conn = small_conn(Wire(any_slice(&buf)));
    let mut cx = Context::from_waker(Waker::noop());
    let mut out = [0u8; 8];
    for _ in 0..3 {
        let mut rb = ReadBuf::new(&mut out);
        match conn.poll_read_app(&mut cx, &mut rb) {
            Poll::Ready(Ok(())) if rb.filled().is_empty() => return,
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(_)) => return,
            Poll::Pending => unreachable!(),
        }
    }
}

#[kani::proof]
#[kani::unwind(14)]
fn post_handshake_any_input() {
    let buf: [u8; 12] = kani::any();
    let input = any_slice(&buf);
    let mut conn = small_conn(Wire(&[]));
    conn.post_hs = input.to_vec();
    if conn.process_post_handshake().is_ok() {
        let rest = &conn.post_hs;
        assert!(rest.len() <= input.len());
        assert_eq!(&input[input.len() - rest.len()..], &rest[..]);
        if rest.len() >= 4 {
            let len = u32::from_be_bytes([0, rest[1], rest[2], rest[3]]) as usize;
            assert!(rest.len() < 4 + len);
        }
    }
}
