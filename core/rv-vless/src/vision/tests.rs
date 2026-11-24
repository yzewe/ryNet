use super::*;

fn state() -> TrafficState {
    TrafficState::new()
}

#[test]
fn padding_roundtrip_any_split() {
    let rng = SystemRandom::new();
    let uuid = [7u8; 16];
    let mut once = Some(uuid);
    let mut wire = Vec::new();
    let payloads: [&[u8]; 3] = [b"first", &[0x42; 3000], b"third"];
    padding(&rng, payloads[0], CMD_CONTINUE, &mut once, true, &mut wire);
    padding(&rng, payloads[1], CMD_CONTINUE, &mut once, false, &mut wire);
    padding(&rng, payloads[2], CMD_END, &mut once, false, &mut wire);
    wire.extend_from_slice(b"after-padding-raw");
    let expected: Vec<u8> = payloads.concat().into_iter().chain(b"after-padding-raw".iter().copied()).collect();

    for split in [1usize, 3, 7, 21, 100, 4096, wire.len()] {
        let mut s = state();
        let mut out = Vec::new();
        for c in wire.chunks(split) {
            s.unpad(&uuid, c, &mut out);
        }
        assert!(out == expected, "split {split}: got {} bytes, want {}", out.len(), expected.len());
    }
    let mut s = state();
    let mut out = Vec::new();
    s.unpad(&uuid, b"plain data that is not vision", &mut out);
    assert_eq!(out, b"plain data that is not vision");
}

#[test]
fn long_padding_sizes() {
    let rng = SystemRandom::new();
    for _ in 0..200 {
        let mut once = None;
        let mut out = Vec::new();
        padding(&rng, &[1; 100], CMD_CONTINUE, &mut once, true, &mut out);
        let pad = out.len() - 5 - 100;
        assert!((800..1300).contains(&pad), "{pad}");
        let mut out = Vec::new();
        padding(&rng, &[1; 100], CMD_CONTINUE, &mut once, false, &mut out);
        assert!(out.len() - 105 < 256);
    }
}

#[test]
fn padding_never_exceeds_buffer() {
    let rng = SystemRandom::new();
    let mut once = Some([0; 16]);
    let mut out = Vec::new();
    padding(&rng, &[0; 8000], CMD_CONTINUE, &mut once, true, &mut out);
    assert!(out.len() <= 16 + BUF_SIZE);
}

#[test]
fn complete_record_detection() {
    let rec = |n: usize| {
        let mut v = vec![0x17, 3, 3, (n >> 8) as u8, n as u8];
        v.resize(5 + n, 0xaa);
        v
    };
    assert!(is_complete_record(&rec(10)));
    assert!(is_complete_record(&[rec(10), rec(300)].concat()));
    assert!(!is_complete_record(&rec(10)[..9]));
    assert!(!is_complete_record(&[0x16, 3, 3, 0, 1, 0]));
    assert!(!is_complete_record(&[rec(10), vec![0x17, 3]].concat()));
}

#[test]
fn reshape_splits_big_chunks_at_record_start() {
    let mut big = vec![0u8; 8190];
    big[5000..5003].copy_from_slice(&TLS_APPLICATION_DATA_START);
    let parts = reshape(vec![&big]);
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].len(), 5000);
    let plain = vec![0u8; 8190];
    let parts = reshape(vec![&plain]);
    assert_eq!(parts[0].len(), BUF_SIZE / 2);
}

#[test]
fn filter_detects_tls13_server_hello() {
    let mut body = vec![0x02, 0, 0, 0, 0x03, 0x03];
    body.extend([0u8; 32]);
    body.push(32);
    body.extend([0u8; 32]);
    body.extend([0x13, 0x01, 0x00]);
    body.extend([0x00, 0x06, 0x00, 0x2b, 0x00, 0x02, 0x03, 0x04]);
    let len = body.len();
    body[1..4].copy_from_slice(&[((len - 4) >> 16) as u8, ((len - 4) >> 8) as u8, (len - 4) as u8]);
    let mut rec = vec![0x16, 0x03, 0x03, (len >> 8) as u8, len as u8];
    rec.extend(body);
    let mut s = state();
    s.filter_tls([&rec[..]]);
    assert!(s.is_tls && s.is_tls12_or_above);
    assert_eq!(s.cipher, 0x1301);
    assert!(s.enable_xtls);
    assert_eq!(s.number_of_packet_to_filter, 0);
}

#[test]
fn filter_detects_client_hello() {
    let mut s = state();
    s.filter_tls([&[0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0, 0][..]]);
    assert!(s.is_tls);
    assert!(!s.is_tls12_or_above);
    assert_eq!(s.number_of_packet_to_filter, 7);
}
