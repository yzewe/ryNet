use super::*;

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

fn check_domain(d: &str) {
    assert!(!d.is_empty() && d.len() <= 253);
    assert!(d.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'.' | b'_')));
}

#[kani::proof]
#[kani::unwind(18)]
fn sniff_any_input() {
    let buf: [u8; 16] = kani::any();
    let b = any_slice(&buf);
    let _ = need_more(b);
    if let Some(d) = sniff(b).domain {
        check_domain(&d);
    }
}

#[kani::proof]
#[kani::unwind(22)]
fn client_hello_sni_any_tail() {
    const FIXED: usize = 5 + 4 + 2 + 32;
    let mut buf: [u8; FIXED + 20] = kani::any();
    buf[0] = 0x16;
    buf[1] = 3;
    buf[5] = 1;
    let len: usize = kani::any();
    kani::assume(len >= FIXED && len <= buf.len());
    let b = &buf[..len];
    let s = sniff(b);
    assert_eq!(s.protocol, Some("tls"));
    if let Some(d) = s.domain {
        check_domain(&d);
        assert!(FIXED + 1 + 2 + 1 + 2 + 4 + 5 + d.len() <= b.len());
    }
}

#[kani::proof]
#[kani::unwind(26)]
fn http_host_any_tail() {
    let mut buf: [u8; 24] = kani::any();
    buf[..4].copy_from_slice(b"GET ");
    let len: usize = kani::any();
    kani::assume(len >= 4 && len <= buf.len());
    let s = sniff(&buf[..len]);
    assert_eq!(s.protocol, Some("http"));
    if let Some(d) = s.domain {
        check_domain(&d);
        assert!(d.parse::<std::net::IpAddr>().is_err());
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn client_hello_sni_roundtrip() {
    const NAME_AT: usize = 5 + 4 + 2 + 32 + 7 + 2 + 4 + 2 + 3;
    let nl: usize = kani::any();
    kani::assume(nl >= 1 && nl <= 6);
    let mut rec: [u8; NAME_AT + 6] = kani::any();
    let total = NAME_AT + nl;
    let ext_len = 4 + 2 + 3 + nl;
    let body_len = total - 9;
    rec[..5].copy_from_slice(&[0x16, 3, 1, 0, (body_len + 4) as u8]);
    rec[5..9].copy_from_slice(&[1, 0, 0, body_len as u8]);
    rec[9..11].copy_from_slice(&[3, 3]);
    rec[43..50].copy_from_slice(&[0, 0, 2, 0x13, 0x01, 1, 0]);
    rec[50..52].copy_from_slice(&(ext_len as u16).to_be_bytes());
    rec[52..54].copy_from_slice(&[0, 0]);
    rec[54..56].copy_from_slice(&((ext_len - 4) as u16).to_be_bytes());
    rec[56..58].copy_from_slice(&((ext_len - 6) as u16).to_be_bytes());
    rec[58] = 0;
    rec[59..61].copy_from_slice(&(nl as u16).to_be_bytes());
    let rec = &rec[..total];
    let name = &rec[NAME_AT..];
    let got = sniff(rec).domain;
    let valid = std::str::from_utf8(name).is_ok_and(valid_domain);
    assert_eq!(got.is_some(), valid);
    if let Some(d) = got {
        assert_eq!(d.as_bytes(), name.to_ascii_lowercase());
    }
}
