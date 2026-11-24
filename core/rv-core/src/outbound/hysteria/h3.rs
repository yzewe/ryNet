use super::proto::{get_varint, padding};

pub use rv_xhttp::h3::control_stream;
use rv_xhttp::h3::{HEADERS as FRAME_HEADERS, MAX_FIELD_SECTION};
pub const STATUS_AUTH_OK: u16 = 233;

fn go_small_map_start(r: u32, len: usize) -> usize {
    let slot = r as usize % 8;
    if slot < len { slot } else { 0 }
}

pub fn auth_request(auth: &str, cc_rx: u64) -> Vec<u8> {
    let pad = String::from_utf8(padding(256, 2048)).unwrap();
    let mut fields = vec![
        (":authority".into(), "hysteria".into()),
        (":method".into(), "POST".into()),
        (":path".into(), "/auth".into()),
        (":scheme".into(), "https".into()),
    ];
    let headers = [("hysteria-auth", auth.to_string()), ("hysteria-cc-rx", cc_rx.to_string()), ("hysteria-padding", pad)];
    let start = go_small_map_start(super::proto::random_u32(), headers.len());
    for i in 0..headers.len() {
        let (n, v) = &headers[(start + i) % headers.len()];
        fields.push((n.to_string(), v.clone()));
    }
    fields.extend([
        ("content-length".into(), "0".into()),
        ("accept-encoding".into(), "gzip".into()),
        ("user-agent".into(), "quic-go HTTP/3".into()),
    ]);
    rv_xhttp::h3::frame(FRAME_HEADERS, &rv_xhttp::h3::encode(&fields))
}

#[derive(Debug, PartialEq, Eq)]
pub struct AuthResponse {
    pub status: u16,
    pub udp: bool,
    pub cc_rx: u64,
}

pub fn parse_headers(block: &[u8]) -> Option<AuthResponse> {
    let mut r = AuthResponse { status: 0, udp: false, cc_rx: 0 };
    for (n, v) in rv_xhttp::h3::decode(block)? {
        match n.as_str() {
            ":status" => r.status = v.parse().unwrap_or(0),
            "hysteria-udp" => r.udp = v.eq_ignore_ascii_case("true"),
            "hysteria-cc-rx" => r.cc_rx = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    Some(r)
}

pub fn take_headers(b: &[u8]) -> Option<Result<(Vec<u8>, usize), ()>> {
    let mut i = 0;
    loop {
        let (t, n) = get_varint(&b[i..])?;
        let (len, m) = get_varint(&b[i + n..])?;
        let start = i + n + m;
        if len > MAX_FIELD_SECTION {
            return Some(Err(()));
        }
        let end = start + len as usize;
        if b.len() < end {
            return None;
        }
        if t == FRAME_HEADERS {
            return Some(Ok((b[start..end].to_vec(), end)));
        }
        i = end;
    }
}

#[cfg(kani)]
#[path = "../../proofs/hysteria_h3.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "../../fuzzing/hysteria_h3.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_length_past_usize_is_refused() {
        assert_eq!(take_headers(&[0x01, 0xc0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]), Some(Err(())));
    }

    #[test]
    fn auth_header_order_follows_go_small_map() {
        let mut starts = [0; 3];
        for r in 0..8 {
            starts[go_small_map_start(r, 3)] += 1;
        }
        assert_eq!(starts, [6, 1, 1]);
    }

    #[test]
    fn request_roundtrip_through_our_decoder() {
        let req = auth_request("secret", 0);
        let Some(Ok((block, used))) = take_headers(&req) else { panic!() };
        assert_eq!(used, req.len());
        let r = parse_headers(&block).unwrap();
        assert_eq!(r.status, 0);
    }

    #[test]
    fn parses_quic_go_style_response() {
        let f = rv_xhttp::h3::encode(&[
            (":status".into(), "233".into()),
            ("hysteria-udp".into(), "true".into()),
            ("hysteria-cc-rx".into(), "0".into()),
        ]);
        assert_eq!(parse_headers(&f).unwrap(), AuthResponse { status: 233, udp: true, cc_rx: 0 });
        assert_eq!(control_stream(), [0x00, 0x04, 0x05, 0x06, 0x80, 0xa0, 0x00, 0x00]);
    }
}
