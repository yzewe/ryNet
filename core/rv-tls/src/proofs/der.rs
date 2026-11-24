use super::*;

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

fn offset_in(outer: &[u8], inner: &[u8]) -> usize {
    let (o, i) = (outer.as_ptr() as usize, inner.as_ptr() as usize);
    assert!(i >= o && i - o + inner.len() <= outer.len());
    i - o
}

fn encode_len(len: usize, out: &mut [u8; 5]) -> usize {
    if len < 0x80 {
        out[0] = len as u8;
        return 1;
    }
    let bytes = (len as u32).to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count();
    let n = 4 - skip;
    out[0] = 0x80 | n as u8;
    out[1..1 + n].copy_from_slice(&bytes[skip..]);
    1 + n
}

#[kani::proof]
#[kani::unwind(6)]
fn next_any_input() {
    let buf: [u8; 12] = kani::any();
    let input = any_slice(&buf);
    let mut rest = input;
    match next(&mut rest) {
        Some((tag, body)) => {
            assert_eq!(tag, input[0]);
            let start = offset_in(input, body);
            let mut enc = [0u8; 5];
            let n = encode_len(body.len(), &mut enc);
            assert_eq!(start, 1 + n);
            assert_eq!(&input[1..start], &enc[..n]);
            assert_eq!(offset_in(input, rest), start + body.len());
            assert_eq!(rest.len(), input.len() - start - body.len());
        }
        None => {
            assert_eq!(rest.len(), input.len());
        }
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn length_roundtrip() {
    const MAX: usize = 300;
    let len: usize = kani::any();
    kani::assume(len <= MAX);
    let tag: u8 = kani::any();
    let mut buf = [0u8; 1 + 5 + MAX + 1];
    let mut enc = [0u8; 5];
    let n = encode_len(len, &mut enc);
    buf[0] = tag;
    buf[1..1 + n].copy_from_slice(&enc[..n]);
    let extra: usize = kani::any();
    kani::assume(extra <= 1);
    let input = &buf[..1 + n + len + extra];
    let mut rest = input;
    let (t, body) = next(&mut rest).unwrap();
    assert_eq!(t, tag);
    assert_eq!(body.len(), len);
    assert_eq!(offset_in(input, body), 1 + n);
    assert_eq!(rest.len(), extra);
}

#[kani::proof]
#[kani::unwind(6)]
fn parse_any_input() {
    let buf: [u8; 24] = kani::any();
    let der = any_slice(&buf);
    if let Some(c) = parse(der) {
        offset_in(der, c.key_alg);
        offset_in(der, c.key);
        offset_in(der, c.sig);
        offset_in(der, c.extensions);
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn parse_minimal_cert() {
    let t1 = if kani::any() { UTC_TIME } else { GENERALIZED_TIME };
    let t2 = if kani::any() { UTC_TIME } else { GENERALIZED_TIME };
    let der: [u8; 38] = [
        SEQ,
        35,
        SEQ,
        26,
        INTEGER,
        1,
        kani::any(),
        SEQ,
        2,
        OID,
        0,
        SEQ,
        0,
        SEQ,
        4,
        t1,
        0,
        t2,
        0,
        SEQ,
        0,
        SEQ,
        7,
        SEQ,
        2,
        OID,
        0,
        BITS,
        1,
        0,
        SEQ,
        2,
        OID,
        0,
        BITS,
        1,
        0,
        kani::any(),
    ];
    let len: usize = kani::any();
    kani::assume(len == 37 || len == 38);
    let c = parse(&der[..len]);
    assert_eq!(c.is_some(), len == 37);
    if let Some(c) = c {
        assert!(c.key.is_empty() && c.sig.is_empty() && c.key_alg.is_empty());
        assert!(!c.is_ca());
    }
}

#[kani::proof]
#[kani::unwind(12)]
fn parse_tbs_tail_any() {
    const TRAILER: [u8; 7] = [SEQ, 2, OID, 0, BITS, 1, 0];
    let t: usize = kani::any();
    kani::assume(t <= 10);
    let mut buf: [u8; 47] = kani::any();
    buf[..30].copy_from_slice(&[
        SEQ,
        35 + t as u8,
        SEQ,
        26 + t as u8,
        INTEGER,
        1,
        1,
        SEQ,
        2,
        OID,
        0,
        SEQ,
        0,
        SEQ,
        4,
        UTC_TIME,
        0,
        UTC_TIME,
        0,
        SEQ,
        0,
        SEQ,
        7,
        SEQ,
        2,
        OID,
        0,
        BITS,
        1,
        0,
    ]);
    for (i, b) in TRAILER.iter().enumerate() {
        buf[30 + t + i] = *b;
    }
    let der = &buf[..37 + t];
    if let Some(c) = parse(der) {
        let ext = offset_in(der, c.extensions);
        assert!(c.extensions.is_empty() || (ext >= 30 + 4 && ext + c.extensions.len() <= 30 + t));
        if t > 0 {
            assert!(matches!(der[30], 0x81 | 0x82 | 0xa3));
        }
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn is_ca_any_input() {
    let buf: [u8; 16] = kani::any();
    let extensions = any_slice(&buf);
    let c = Cert { key_alg: &[], key: &[], sig: &[], extensions };
    let _ = c.is_ca();
}

#[kani::proof]
#[kani::unwind(6)]
fn first_extension_any_input() {
    let buf: [u8; 16] = kani::any();
    let extensions = any_slice(&buf);
    let c = Cert { key_alg: &[], key: &[], sig: &[], extensions };
    if let Some(v) = c.first_extension() {
        offset_in(extensions, v);
    }
}
