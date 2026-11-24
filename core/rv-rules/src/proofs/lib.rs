use super::*;
use std::net::Ipv4Addr;

pub(crate) fn templated<'a, const M: usize>(buf: &'a mut [u8; M], tpl: &[u8; M]) -> &'a str {
    for i in 0..M {
        if tpl[i] != b'?' {
            buf[i] = tpl[i];
        }
    }
    let s = std::str::from_utf8(buf);
    kani::assume(s.is_ok());
    s.unwrap()
}

pub(crate) const V4_CIDR: &[u8; 10] = b"?.?.?.?/??";
pub(crate) const V6_CIDR: &[u8; 7] = b"::?/???";

fn any_range(max: u128) -> IpRange {
    let (lo, hi): (u128, u128) = (kani::any(), kani::any());
    kani::assume(lo <= hi && hi <= max);
    IpRange { lo, hi }
}

fn check_ip_range(s: &str) {
    if let Some((four, r)) = parse_ip_range(s) {
        assert!(r.lo <= r.hi);
        let span = r.hi - r.lo;
        assert!(span & span.wrapping_add(1) == 0);
        assert!(r.lo & span == 0);
        if four {
            assert!(r.hi <= u32::MAX as u128);
        }
    }
}

#[kani::proof]
#[kani::unwind(7)]
fn port_ranges_any_string() {
    let mut buf: [u8; 5] = kani::any();
    let s = templated(&mut buf, b"?????");
    let rs = port_ranges(&Value::String(s.to_owned()));
    for i in 0..rs.len() {
        assert!(rs[i].0 <= rs[i].1);
        if i > 0 {
            assert!((rs[i - 1].1 as u32) + 1 < rs[i].0 as u32);
        }
    }
    let p: u16 = kani::any();
    assert_eq!(port_contains(&rs, p), rs.iter().any(|r| r.0 <= p && p <= r.1));
}

#[kani::proof]
#[kani::unwind(6)]
fn parse_ip_range_any4() {
    let mut buf: [u8; 4] = kani::any();
    check_ip_range(templated(&mut buf, b"????"));
}

#[kani::proof]
#[kani::unwind(9)]
fn parse_ip_range_v6() {
    let mut buf: [u8; 7] = kani::any();
    check_ip_range(templated(&mut buf, V6_CIDR));
}

#[kani::proof]
#[kani::unwind(12)]
fn parse_ip_range_v4() {
    let mut buf: [u8; 10] = kani::any();
    check_ip_range(templated(&mut buf, V4_CIDR));
}

#[kani::proof]
#[kani::unwind(5)]
fn merge_v6_full_width() {
    let n: usize = kani::any();
    kani::assume(n <= 3);
    let mut orig = Vec::new();
    for _ in 0..n {
        orig.push(any_range(u128::MAX));
    }
    let mut rs = orig.clone();
    merge(&mut rs);
    for i in 1..rs.len() {
        assert!(rs[i - 1].hi < rs[i].lo && rs[i].lo - rs[i - 1].hi > 1);
    }
    let x: u128 = kani::any();
    assert_eq!(contains(&rs, x), orig.iter().any(|r| r.lo <= x && x <= r.hi));
}

#[kani::proof]
#[kani::unwind(5)]
fn merge_preserves_coverage() {
    let n: usize = kani::any();
    kani::assume(n <= 3);
    let mut orig = Vec::new();
    for _ in 0..n {
        orig.push(any_range(u32::MAX as u128));
    }
    let mut rs = orig.clone();
    merge(&mut rs);
    assert!(rs.len() <= orig.len());
    for i in 0..rs.len() {
        assert!(rs[i].lo <= rs[i].hi);
        if i > 0 {
            assert!(rs[i - 1].hi + 1 < rs[i].lo);
        }
    }
    let x: u32 = kani::any();
    let want = orig.iter().any(|r| r.lo <= x as u128 && x as u128 <= r.hi);
    assert_eq!(contains(&rs, x as u128), want);
    let negate: bool = kani::any();
    let set = IpSet { v4: rs.iter().map(|r| (r.lo as u32, r.hi as u32)).collect(), v6: Box::new([]) };
    let m = IpMatcher { sets: vec![std::sync::Arc::new(set)], negate };
    assert_eq!(m.matches(IpAddr::V4(Ipv4Addr::from(x))), want != negate);
}
