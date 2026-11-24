use super::*;

fn utf8<const M: usize>(buf: &[u8; M]) -> &str {
    let s = std::str::from_utf8(buf);
    kani::assume(s.is_ok());
    s.unwrap()
}

#[allow(dead_code)]
fn any_v5(_: &uuid::Uuid, _: &[u8]) -> uuid::Uuid {
    uuid::Uuid::from_bytes(kani::any())
}

#[kani::proof]
#[kani::stub(uuid::Uuid::new_v5, any_v5)]
#[kani::unwind(6)]
fn parse_uuid_short() {
    let buf: [u8; 4] = kani::any();
    let s = utf8(&buf);
    let r = parse_uuid(s);
    assert!(r.is_ok());
    if s.trim().is_empty() {
        assert_eq!(r.unwrap(), [0; 16]);
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn split_early_data_any() {
    let buf: [u8; 6] = kani::any();
    let s = utf8(&buf);
    let (path, ed) = split_early_data(s);
    if let Some((_, q)) = path.split_once('?') {
        for kv in q.split('&') {
            assert!(kv.split_once('=').map_or(kv, |(k, _)| k) != "ed");
        }
    }
    if !s.bytes().any(|b| b == b'?') {
        assert_eq!(path, s);
        assert_eq!(ed, 0);
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn range_parse_string() {
    let buf: [u8; 4] = kani::any();
    if let Some(r) = Range::parse(&Value::String(utf8(&buf).to_owned())) {
        assert!(r.from <= r.to);
    }
}

#[kani::proof]
fn range_parse_number() {
    let n: u64 = kani::any();
    assert_eq!(Range::parse(&Value::from(n)), Some(Range { from: n, to: n }));
}

#[kani::proof]
#[kani::unwind(6)]
fn hop_interval_string() {
    let buf: [u8; 4] = kani::any();
    if let Ok((a, b)) = hop_interval(Some(&Value::String(utf8(&buf).to_owned()))) {
        assert!(5 <= a && a <= b);
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn hop_interval_number() {
    let v = if kani::any() { Value::from(kani::any::<i64>()) } else { Value::from(kani::any::<u64>()) };
    let zero = v.as_u64() == Some(0);
    if let Ok((a, b)) = hop_interval(Some(&v)) {
        assert!(5 <= a && a == b);
        if zero {
            assert_eq!(a, 30);
        }
    }
}
