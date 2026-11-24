use super::*;

fn utf8_lossy_cheap(v: &[u8]) -> std::borrow::Cow<'_, str> {
    match std::str::from_utf8(v) {
        Ok(s) => std::borrow::Cow::Borrowed(s),
        Err(_) => std::borrow::Cow::Owned(String::new()),
    }
}

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

const VARINT_MAX: u64 = (1 << 62) - 1;

#[kani::proof]
#[kani::unwind(10)]
fn get_varint_any_input() {
    let buf: [u8; 10] = kani::any();
    let b = any_slice(&buf);
    match get_varint(b) {
        Some((v, n)) => {
            assert_eq!(n, 1 << (b[0] >> 6));
            assert!(n <= b.len());
            assert!(v <= VARINT_MAX);
        }
        None => {
            assert!(b.is_empty() || b.len() < 1 << (b[0] >> 6));
        }
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn varint_roundtrip() {
    let v: u64 = kani::any();
    kani::assume(v <= VARINT_MAX);
    let mut o = Vec::with_capacity(16);
    put_varint(&mut o, v);
    let want = match v {
        0..=63 => 1,
        64..=16383 => 2,
        16384..=1073741823 => 4,
        _ => 8,
    };
    assert_eq!(o.len(), want);
    o.push(kani::any());
    assert_eq!(get_varint(&o), Some((v, want)));
}

#[kani::proof]
#[kani::unwind(10)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_cheap)]
fn tcp_response_any_input() {
    let buf: [u8; 12] = kani::any();
    let b = any_slice(&buf);
    if let Some(Ok(i)) = tcp_response(b) {
        assert!(i <= b.len() && i <= MAX_TCP_RESPONSE);
        assert_eq!(b[0], 0);
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn parse_udp_any_input() {
    let buf: [u8; 16] = kani::any();
    let b = any_slice(&buf);
    if let Some(m) = parse_udp(b) {
        assert!(!m.addr.is_empty() && m.addr.len() <= 2048);
        assert!(!m.data.is_empty());
        let end = b.len() - m.data.len();
        assert_eq!(m.data.as_ptr(), b[end..].as_ptr());
        assert_eq!(m.addr.as_ptr(), b[end - m.addr.len()..].as_ptr());
        assert!(end - m.addr.len() >= 9);
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn udp_message_roundtrip() {
    let abuf: [u8; 4] = kani::any();
    let alen: usize = kani::any();
    kani::assume(alen >= 1 && alen <= 4);
    let Ok(addr) = std::str::from_utf8(&abuf[..alen]) else { return };
    let dbuf: [u8; 4] = kani::any();
    let dlen: usize = kani::any();
    kani::assume(dlen >= 1 && dlen <= 4);
    let m = UdpMessage { session: kani::any(), packet: kani::any(), frag: kani::any(), frags: kani::any(), addr, data: &dbuf[..dlen] };
    let mut o = Vec::with_capacity(32);
    m.encode(&mut o);
    assert_eq!(o.len(), UdpMessage::header_len(addr) + dlen);
    let p = parse_udp(&o).unwrap();
    assert_eq!((p.session, p.packet, p.frag, p.frags), (m.session, m.packet, m.frag, m.frags));
    assert_eq!(p.addr, m.addr);
    assert_eq!(p.data, m.data);
}

fn any_message(data: &[u8; 2]) -> UdpMessage<'_> {
    let len: usize = kani::any();
    kani::assume(len <= 2);
    UdpMessage { session: 0, packet: kani::any(), frag: kani::any(), frags: kani::any(), addr: "a", data: &data[..len] }
}

#[kani::proof]
#[kani::unwind(8)]
fn defragger_any_sequence() {
    let mut d = Defragger::default();
    let bufs: [[u8; 2]; 3] = kani::any();
    let mut fed = 0;
    for buf in &bufs {
        let m = any_message(buf);
        kani::assume(m.frags <= 4);
        fed += m.data.len();
        if let Some(out) = d.feed(&m) {
            assert!(out.len() <= fed);
            if m.frags > 1 {
                fed = 0;
            }
        }
        assert!(d.got <= d.frags.len());
    }
}
