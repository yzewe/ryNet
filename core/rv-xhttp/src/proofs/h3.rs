use super::*;

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

#[kani::proof]
#[kani::unwind(10)]
fn get_varint_any_input() {
    let buf: [u8; 9] = kani::any();
    let b = any_slice(&buf);
    match get_varint(b) {
        Some((v, n)) => {
            assert!(n == 1 || n == 2 || n == 4 || n == 8);
            assert!(n <= b.len());
            assert_eq!(n, 1 << (b[0] >> 6));
            assert!(v < 1 << 62);
        }
        None => assert!(b.is_empty() || b.len() < 1 << (b[0] >> 6)),
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn varint_roundtrip() {
    let v: u64 = kani::any();
    kani::assume(v < 1 << 62);
    let mut o = Vec::new();
    put_varint(&mut o, v);
    assert_eq!(get_varint(&o), Some((v, o.len())));
    let min = if v < 64 {
        1
    } else if v < 16384 {
        2
    } else if v < 1 << 30 {
        4
    } else {
        8
    };
    assert_eq!(o.len(), min);
}

#[kani::proof]
#[kani::unwind(13)]
fn get_int_any_input() {
    let buf: [u8; 12] = kani::any();
    let b = any_slice(&buf);
    let bits: u8 = kani::any();
    kani::assume((1..=8).contains(&bits));
    let start: usize = kani::any();
    kani::assume(start <= b.len());
    let mut i = start;
    if get_int(b, &mut i, bits).is_some() {
        assert!(i > start && i <= b.len());
    }
}

#[kani::proof]
#[kani::unwind(12)]
fn int_roundtrip() {
    let bits: u8 = kani::any();
    kani::assume((1..=8).contains(&bits));
    let flags: u8 = kani::any();
    kani::assume(bits == 8 || flags >> bits << bits == flags);
    let flags = if bits == 8 { 0 } else { flags };
    let v: u64 = kani::any();
    kani::assume(v < 1 << 62);
    let mut o = Vec::new();
    put_int(&mut o, flags, bits, v);
    let mut i = 0;
    assert_eq!(get_int(&o, &mut i, bits), Some(v));
    assert_eq!(i, o.len());
    if bits < 8 {
        assert_eq!(o[0] >> bits << bits, flags);
    }
}

fn hpack_decode_any(b: &[u8]) -> Option<Vec<u8>> {
    if kani::any() {
        return None;
    }
    let raw: [u8; 12] = kani::any();
    let n: usize = kani::any();
    kani::assume(n <= raw.len() && n * 5 <= b.len() * 8);
    Some(raw[..n].to_vec())
}

fn hpack_identity(s: &[u8]) -> Vec<u8> {
    s.to_vec()
}

fn hpack_identity_decode(s: &[u8]) -> Option<Vec<u8>> {
    Some(s.to_vec())
}

#[kani::proof]
#[kani::unwind(14)]
#[kani::stub(crate::hpack::decode, hpack_decode_any)]
fn get_string_any_input() {
    let buf: [u8; 12] = kani::any();
    let b = any_slice(&buf);
    let bits: u8 = kani::any();
    kani::assume((1..=7).contains(&bits));
    let mut i = 0;
    if let Some(s) = get_string(b, &mut i, bits) {
        assert!(i <= b.len());
        if b[0] & (1 << bits) == 0 {
            assert!(s.len() < b.len());
        }
    }
}

#[kani::proof]
#[kani::unwind(14)]
#[kani::stub(crate::hpack::decode, hpack_decode_any)]
fn qpack_decode_any_input() {
    let buf: [u8; 10] = kani::any();
    let b = any_slice(&buf);
    if let Some(h) = decode(b) {
        assert!(b.len() >= 2 && b[0] == 0 && b[1] == 0);
        assert!(h.len() <= b.len() - 2);
    }
}

fn any_ascii(n: usize) -> String {
    let raw: [u8; 2] = kani::any();
    kani::assume(raw.iter().all(u8::is_ascii));
    unsafe { String::from_utf8_unchecked(raw[..n].to_vec()) }
}

fn decode_exact(o: &[u8]) -> Option<Vec<(String, String)>> {
    match o.len() {
        0 => decode(&o[..0]),
        1 => decode(&o[..1]),
        2 => decode(&o[..2]),
        3 => decode(&o[..3]),
        4 => decode(&o[..4]),
        5 => decode(&o[..5]),
        6 => decode(&o[..6]),
        7 => decode(&o[..7]),
        8 => decode(&o[..8]),
        _ => unreachable!(),
    }
}

fn roundtrip_one(name: String, value: String) {
    let h = vec![(name, value)];
    let o = encode(&h);
    assert!(o.len() <= 8);
    assert!(decode_exact(&o) == Some(h));
}

#[kani::proof]
#[kani::unwind(101)]
#[kani::stub(crate::hpack::encode, hpack_identity)]
#[kani::stub(crate::hpack::decode, hpack_identity_decode)]
fn qpack_roundtrip_static_fields() {
    for &(n, v) in STATIC {
        let h = vec![(n.to_string(), v.to_string())];
        assert!(decode(&encode(&h)) == Some(h));
    }
}

#[kani::proof]
#[kani::unwind(101)]
#[kani::stub(crate::hpack::encode, hpack_identity)]
#[kani::stub(crate::hpack::decode, hpack_identity_decode)]
fn qpack_roundtrip_static_name() {
    for &(n, _) in STATIC {
        roundtrip_one(n.to_string(), any_ascii(1));
    }
}

#[kani::proof]
#[kani::unwind(101)]
#[kani::stub(crate::hpack::encode, hpack_identity)]
#[kani::stub(crate::hpack::decode, hpack_identity_decode)]
fn qpack_roundtrip_literal() {
    for nl in 0..=2 {
        for vl in 0..=2 {
            roundtrip_one(any_ascii(nl), any_ascii(vl));
        }
    }
}
