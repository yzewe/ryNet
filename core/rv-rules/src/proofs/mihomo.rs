use super::*;
use crate::proofs::{V4_CIDR, V6_CIDR, templated};

#[kani::proof]
#[kani::unwind(7)]
fn sub_rules_any() {
    let mut buf: [u8; 5] = kani::any();
    let s = templated(&mut buf, b"?????");
    if let Ok(subs) = sub_rules(s) {
        assert!(!subs.is_empty());
        for p in &subs {
            assert!(p.len() + 4 <= s.len());
        }
    }
}

#[kani::proof]
#[kani::unwind(7)]
fn split_top_any() {
    let mut buf: [u8; 5] = kani::any();
    let s = templated(&mut buf, b"?????");
    let parts = split_top(s);
    if !s.bytes().any(|b| b == b'(' || b == b')') {
        assert_eq!(parts.len(), s.bytes().filter(|&b| b == b',').count() + 1);
    }
}

#[kani::proof]
#[kani::unwind(9)]
fn validate_cidr_matches_router_v6() {
    let mut buf: [u8; 7] = kani::any();
    let s = templated(&mut buf, V6_CIDR);
    assert_eq!(validate_cidr(s).is_ok(), crate::parse_ip_range(s).is_some());
}

#[kani::proof]
#[kani::unwind(12)]
fn validate_cidr_matches_router_v4() {
    let mut buf: [u8; 10] = kani::any();
    let s = templated(&mut buf, V4_CIDR);
    assert_eq!(validate_cidr(s).is_ok(), crate::parse_ip_range(s).is_some());
}
