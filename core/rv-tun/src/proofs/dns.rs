use super::*;

const N: usize = 32;

fn any_msg(buf: &[u8; N]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= N);
    &buf[..len]
}

#[kani::proof]
#[kani::unwind(34)]
fn dns_parsers_any_input() {
    let buf: [u8; N] = kani::any();
    let m = any_msg(&buf);
    let r2: [u8; N] = kani::any();
    let r = any_msg(&r2);
    if let Some(e) = skip_name(m, 12) {
        assert!(e > 12);
    }
    let _ = qname(m);
    let _ = answers(m, r);
    let _ = usable(m, r);
    let _ = has_record(r, kani::any());
    if let Some(t) = reply_ttl(r) {
        assert!(t <= CACHE_TTL_MAX);
    }
    let mut c = [0u8; N];
    c[..r.len()].copy_from_slice(r);
    age_ttls(&mut c[..r.len()], kani::any());
}

#[kani::proof]
#[kani::unwind(34)]
fn cache_key_is_the_question() {
    let buf: [u8; N] = kani::any();
    let m = any_msg(&buf);
    if let Some(k) = cache_key(m) {
        assert!(m.len() >= 12 + k.len());
        assert!(k.len() >= 5);
        assert_eq!(u16::from_be_bytes([m[4], m[5]]), 1);
    }
}

#[kani::proof]
#[kani::unwind(34)]
fn servfail_answers_the_query() {
    let buf: [u8; N] = kani::any();
    let q = any_msg(&buf);
    let Some(r) = servfail(q) else { return };
    assert!(r.len() >= 12 && r.len() <= q.len());
    assert_eq!(&r[..2], &q[..2]);
    assert!(r[2] & 0x80 != 0 && r[3] & 0x0f == 2);
    assert_eq!(&r[4..12], &[0, 1, 0, 0, 0, 0, 0, 0]);
    if u16::from_be_bytes([q[4], q[5]]) == 1 {
        assert!(answers(q, &r));
    }
    assert!(!usable(q, &r));
}

#[kani::proof]
#[kani::unwind(34)]
fn age_ttls_keeps_header_and_question() {
    let buf: [u8; N] = kani::any();
    let m = any_msg(&buf);
    let mut c = [0u8; N];
    c[..m.len()].copy_from_slice(m);
    let age: u32 = kani::any();
    age_ttls(&mut c[..m.len()], age);
    if let Some(e) = skip_name(m, 12) {
        let q_end = (e + 4).min(m.len());
        assert_eq!(&c[..q_end], &m[..q_end]);
    } else {
        assert_eq!(&c[..m.len()], m);
    }
}
