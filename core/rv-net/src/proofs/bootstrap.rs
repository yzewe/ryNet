use super::*;

fn with_tail<const M: usize>(prefix: &[u8]) -> Vec<u8> {
    let tail: [u8; M] = kani::any();
    let n: usize = kani::any();
    kani::assume(n <= M);
    let mut raw = prefix.to_vec();
    raw.extend_from_slice(&tail[..n]);
    raw
}

#[kani::proof]
#[kani::unwind(14)]
fn http_body_any_input() {
    let buf: [u8; 12] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= buf.len());
    let _ = http_body(&buf[..len]);
}

#[kani::proof]
#[kani::unwind(86)]
fn http_body_chunked_any_body() {
    let raw = with_tail::<8>(b"HTTP/1.1 200 OK\r\ncontent-type:application/dns-message\r\ntransfer-encoding:chunked\r\n\r\n");
    if let Ok(body) = http_body(&raw) {
        assert!(body.is_empty() || body.len() + 10 <= 8);
    }
}

#[kani::proof]
#[kani::unwind(80)]
fn http_body_length_any_tail() {
    let raw = with_tail::<8>(b"HTTP/1.1 200 OK\r\ncontent-type:application/dns-message\r\ncontent-length:");
    if let Ok(body) = http_body(&raw) {
        assert!(body.len() + 5 <= 8);
    }
}
