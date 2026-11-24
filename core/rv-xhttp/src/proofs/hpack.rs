use super::*;
use std::collections::hash_map::RandomState;

fn fixed_random_state() -> RandomState {
    unsafe { std::mem::transmute::<(u64, u64), RandomState>((0x736f_6d65_7073_6575, 0x646f_7261_6e64_6f6d)) }
}

fn linear_symbol(t: &Tables, len: u8, code: u32) -> Option<u16> {
    (0..257u16).find(|&s| HPACK_BITS[s as usize] == len && t.codes[s as usize] == code)
}

fn decode_upto4(e: &[u8]) -> Option<Vec<u8>> {
    match e.len() {
        0 => decode(&e[..0]),
        1 => decode(&e[..1]),
        2 => decode(&e[..2]),
        3 => decode(&e[..3]),
        4 => decode(&e[..4]),
        _ => unreachable!(),
    }
}

#[kani::proof]
#[kani::unwind(260)]
#[kani::stub(std::collections::hash_map::RandomState::new, fixed_random_state)]
#[kani::stub(symbol, linear_symbol)]
fn decode_any_input() {
    let buf: [u8; 3] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= buf.len());
    if let Some(out) = decode_upto4(&buf[..len]) {
        assert!(out.len() * 5 <= len * 8);
    }
}

#[kani::proof]
#[kani::unwind(260)]
#[kani::stub(std::collections::hash_map::RandomState::new, fixed_random_state)]
#[kani::stub(symbol, linear_symbol)]
fn roundtrip_one_symbol() {
    let c: u8 = kani::any();
    let s = [c];
    let n: usize = kani::any();
    kani::assume(n <= 1);
    let e = encode(&s[..n]);
    assert!(e.len() <= 4);
    assert!(decode_upto4(&e).as_deref() == Some(&s[..n]));
    if let Ok(t) = std::str::from_utf8(&s[..n]) {
        assert_eq!(crate::request::huffman_len(t), e.len());
    }
}

#[kani::proof]
#[kani::unwind(260)]
#[kani::stub(std::collections::hash_map::RandomState::new, fixed_random_state)]
#[kani::stub(symbol, linear_symbol)]
fn roundtrip_two_printable() {
    let s: [u8; 2] = kani::any();
    kani::assume(s.iter().all(|c| (0x20..0x7f).contains(c)));
    let e = encode(&s);
    assert!(e.len() <= 4);
    assert!(decode_upto4(&e).as_deref() == Some(&s[..]));
    assert_eq!(crate::request::huffman_len(std::str::from_utf8(&s).unwrap()), e.len());
}
