use super::*;

fn utf8<const M: usize>(buf: &[u8; M]) -> &str {
    let s = std::str::from_utf8(buf);
    kani::assume(s.is_ok());
    s.unwrap()
}

#[kani::proof]
#[kani::unwind(6)]
fn decode_body_any() {
    let buf: [u8; 4] = kani::any();
    let s = utf8(&buf);
    let out = decode_body(s);
    if out != s {
        assert!(!s.contains("://"));
        assert!(out.contains("://"));
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn b64_any() {
    let buf: [u8; 4] = kani::any();
    let s = utf8(&buf);
    if let Some(d) = b64(s) {
        assert!(d.len() <= 3);
    }
}

#[kani::proof]
#[kani::unwind(11)]
fn split_link_any() {
    let buf: [u8; 5] = kani::any();
    let tail = utf8(&buf);
    let mut line = String::from("x://");
    line.push_str(tail);
    let Some(l) = split_link(&line) else { return };
    assert_eq!(l.scheme, "x");
    assert!(!l.host.bytes().any(|b| matches!(b, b'@' | b'?' | b'#' | b'/')));
    if let Ok(p) = l.ports.parse::<u16>() {
        assert_eq!(l.port, p);
    }
    if l.ports.is_empty() {
        assert_eq!(l.port, 443);
    }
}

fn roundtrip<const M: usize>() {
    let raw: [u8; M] = kani::any();
    let mut enc = String::new();
    for &b in &raw {
        let digits: &[u8; 16] = if kani::any() { b"0123456789ABCDEF" } else { b"0123456789abcdef" };
        enc.push('%');
        enc.push(digits[(b >> 4) as usize] as char);
        enc.push(digits[(b & 15) as usize] as char);
    }
    let d = percent_decode(&enc);
    match std::str::from_utf8(&raw) {
        Ok(t) => {
            assert_eq!(d, t);
        }
        Err(_) => {
            assert!(d.contains('\u{FFFD}'));
        }
    }
}

#[kani::proof]
#[kani::unwind(14)]
fn percent_roundtrip4() {
    roundtrip::<4>();
}

#[kani::proof]
#[kani::unwind(8)]
fn percent_roundtrip2() {
    roundtrip::<2>();
}

#[kani::proof]
#[kani::unwind(7)]
fn percent_decode_any() {
    let buf: [u8; 5] = kani::any();
    let s = utf8(&buf);
    let d = percent_decode(s);
    if !s.bytes().any(|b| b == b'%') {
        assert_eq!(d, s);
    }
}
