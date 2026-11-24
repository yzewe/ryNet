use super::*;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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

#[kani::proof]
#[kani::unwind(26)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_cheap)]
fn question_any_input() {
    let buf: [u8; 24] = kani::any();
    let msg = any_slice(&buf);
    if let Some(q) = question(msg) {
        assert!(q.end >= 17 && q.end <= msg.len());
        assert_eq!(q.qtype, u16::from_be_bytes([msg[q.end - 4], msg[q.end - 3]]));
    }
}

#[kani::proof]
#[kani::unwind(22)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_cheap)]
fn journaled_name_is_one_line() {
    let buf: [u8; 20] = kani::any();
    let msg = any_slice(&buf);
    if let Some(q) = question(msg)
        && fake::valid(&q.name)
    {
        assert!(!q.name.bytes().any(|c| c.is_ascii_control() || c == b' '));
    }
}

#[kani::proof]
#[kani::unwind(22)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_cheap)]
fn reply_any_answers() {
    let mut buf: [u8; 37] = kani::any();
    kani::assume(buf[4] != 0 || buf[5] != 0);
    buf[12] = 0;
    let len: usize = kani::any();
    kani::assume(len >= 17 && len <= buf.len());
    let reply = &buf[..len];
    let _ = answer_ttl(reply);
    for ip in addresses(reply) {
        let found = match ip {
            IpAddr::V4(a) => reply[17..].windows(4).any(|w| w == a.octets()),
            IpAddr::V6(a) => reply[17..].windows(16).any(|w| w == a.octets()),
        };
        assert!(found);
    }
}

#[kani::proof]
#[kani::unwind(18)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_cheap)]
fn reply_any_input() {
    let buf: [u8; 16] = kani::any();
    let reply = any_slice(&buf);
    let _ = answer_ttl(reply);
    let _ = addresses(reply);
}

#[kani::proof]
#[kani::unwind(24)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_cheap)]
fn answer_roundtrip() {
    let buf: [u8; 20] = kani::any();
    let query = any_slice(&buf);
    let Some(q) = question(query) else { return };
    let n: usize = kani::any();
    kani::assume(n <= 2);
    let mut ips = Vec::new();
    for _ in 0..n {
        ips.push(if kani::any() {
            IpAddr::V4(Ipv4Addr::from(kani::any::<[u8; 4]>()))
        } else {
            IpAddr::V6(Ipv6Addr::from(kani::any::<[u8; 16]>()))
        });
    }
    let ttl: u32 = kani::any();
    let r = answer(query, &q, &ips, ttl);
    assert_eq!(question(&r), Some(Question { name: q.name.clone(), qtype: q.qtype, end: q.end }));
    assert_eq!(addresses(&r), ips);
    assert_eq!(answer_ttl(&r), if ips.is_empty() || ttl == u32::MAX { 0 } else { ttl });
    let code: u8 = kani::any();
    let e = rcode(query, &q, code);
    assert_eq!(e[3] & 0x0f, code & 0x0f);
    assert!(addresses(&e).is_empty());
}

#[kani::proof]
#[kani::unwind(60)]
#[kani::stub(std::string::String::from_utf8_lossy, utf8_lossy_cheap)]
fn pad_query_any_input() {
    let mut buf: [u8; 33] = kani::any();
    buf[12] = 0;
    let len: usize = kani::any();
    kani::assume(len >= 17 && len <= buf.len());
    let msg = &buf[..len];
    let n: usize = kani::any();
    kani::assume(n <= 8);
    let out = pad_query(msg, n);
    if out != msg {
        assert_eq!(question(&out), question(msg));
        let had_opt = u16::from_be_bytes([msg[10], msg[11]]) == 1;
        assert_eq!(out.len(), msg.len() + 4 + n + if had_opt { 0 } else { 11 });
        assert_eq!(u16::from_be_bytes([out[10], out[11]]), 1);
        if !had_opt {
            assert_eq!(pad_query(&out, n), out);
        }
    }
}
