use super::*;

pub fn first_bytes(data: &[u8]) {
    let _ = need_more(data);
    let s = sniff(data);
    if let Some(d) = s.domain {
        assert!(!d.is_empty() && d.len() <= 253);
        assert!(d.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'.' | b'_')));
        if s.protocol == Some("http") {
            assert!(d.parse::<std::net::IpAddr>().is_err());
        }
    }
}
