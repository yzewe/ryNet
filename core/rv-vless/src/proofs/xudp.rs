use super::*;
use std::borrow::Cow;

const N: usize = 24;

fn no_format(_: std::fmt::Arguments<'_>) -> String {
    String::new()
}

fn utf8_lossy_cheap(v: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(v) {
        Ok(s) => Cow::Borrowed(s),
        Err(_) => Cow::Owned(String::new()),
    }
}

fn utf8_lossy_any(_: &[u8]) -> Cow<'_, str> {
    Cow::Borrowed("")
}

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

fn drain(p: &mut Parser, got: &mut Vec<Packet>) -> bool {
    loop {
        match p.next_packet() {
            Ok(Some(pk)) => got.push(pk),
            Ok(None) => return true,
            Err(_) => return false,
        }
    }
}

#[kani::proof]
#[kani::unwind(8)]
#[kani::stub(std::fmt::format, no_format)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_any)]
fn next_packet_any_input() {
    let buf: [u8; N] = kani::any();
    let b = any_slice(&buf);
    let mut p = Parser::default();
    p.push(b);
    let mut total = 0;
    while let Ok(Some(pk)) = p.next_packet() {
        assert!(!pk.payload.is_empty());
        total += pk.payload.len() + 8;
        assert!(total <= b.len());
        assert!(p.pos <= p.buf.len());
    }
    assert!(p.pos <= p.buf.len());
}

#[kani::proof]
#[kani::unwind(10)]
#[kani::stub(std::fmt::format, no_format)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_any)]
fn next_packet_split_invariant() {
    let buf: [u8; 16] = kani::any();
    let b = any_slice(&buf);
    let k: usize = kani::any();
    kani::assume(k <= b.len());

    let mut whole = Parser::default();
    let mut want = Vec::new();
    whole.push(b);
    let ok_whole = drain(&mut whole, &mut want);

    let mut parts = Parser::default();
    let mut got = Vec::new();
    parts.push(&b[..k]);
    if !drain(&mut parts, &mut got) {
        assert!(!ok_whole);
        return;
    }
    parts.push(&b[k..]);
    let ok_parts = drain(&mut parts, &mut got);
    assert_eq!(ok_parts, ok_whole);
    assert!(got == want);
}

#[kani::proof]
#[kani::unwind(20)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_any)]
fn parse_addr_any_input() {
    let buf: [u8; N] = kani::any();
    let b = any_slice(&buf);
    if let Ok((addr, port)) = parse_addr(b) {
        assert_eq!(port, u16::from_be_bytes([b[0], b[1]]));
        match addr {
            Address::Ipv4(a) => assert!(b[2] == 1 && b.len() >= 7 && a == b[3..7]),
            Address::Ipv6(a) => assert!(b[2] == 3 && b.len() >= 19 && a == b[3..19]),
            Address::Domain(_) => assert!(b[2] == 2 && b.len() >= 4 + b[3] as usize),
        }
    }
}

fn roundtrip(addr: Address) {
    let port: u16 = kani::any();
    let gid: [u8; 8] = kani::any();
    let first: bool = kani::any();
    let data: [u8; 4] = kani::any();
    let n: usize = kani::any();
    kani::assume(n >= 1 && n <= data.len());
    let payload = &data[..n];

    let mut wire = Vec::new();
    assert!(encode(&mut wire, first, &addr, port, &gid, payload));
    let mut p = Parser::default();
    p.push(&wire);
    let pk = p.next_packet().unwrap().unwrap();
    assert!(pk.from == Some((addr, port)));
    assert!(pk.payload == payload);
    assert_eq!(p.pos, wire.len());
    assert!(matches!(p.next_packet(), Ok(None)));
}

#[kani::proof]
#[kani::unwind(18)]
#[kani::stub(std::fmt::format, no_format)]
fn encode_then_parse_ipv4() {
    roundtrip(Address::Ipv4(kani::any()));
}

#[kani::proof]
#[kani::unwind(18)]
#[kani::stub(std::fmt::format, no_format)]
fn encode_then_parse_ipv6() {
    roundtrip(Address::Ipv6(kani::any()));
}

#[kani::proof]
#[kani::unwind(18)]
#[kani::stub(std::fmt::format, no_format)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_cheap)]
fn encode_then_parse_domain() {
    let raw: [u8; 4] = kani::any();
    let n: usize = kani::any();
    kani::assume(n >= 1 && n <= raw.len());
    let Ok(s) = std::str::from_utf8(&raw[..n]) else { return };
    roundtrip(Address::Domain(s.to_string()));
}

#[kani::proof]
fn encode_rejects_bad_payload_len() {
    let len: usize = kani::any();
    kani::assume(len == 0 || len == MAX_PAYLOAD + 1);
    let payload = vec![0u8; len];
    let mut out = Vec::new();
    assert!(!encode(&mut out, kani::any(), &Address::Ipv4(kani::any()), kani::any(), &kani::any(), &payload));
    assert!(out.is_empty());
}
