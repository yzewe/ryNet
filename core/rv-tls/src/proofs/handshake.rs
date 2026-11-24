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

#[kani::proof]
#[kani::unwind(10)]
fn reader_primitives() {
    let buf: [u8; 8] = kani::any();
    let input = any_slice(&buf);
    let mut r = Reader(input);
    let op: u8 = kani::any();
    let (hdr, got) = match op {
        0 => {
            let n: usize = kani::any();
            (
                0,
                r.take(n).map(|b| {
                    assert_eq!(b.len(), n);
                    b
                }),
            )
        }
        1 => (
            0,
            r.u8().map(|v| {
                assert_eq!(v, input[0]);
                &input[..1]
            }),
        ),
        2 => (
            0,
            r.u16().map(|v| {
                assert_eq!(&v.to_be_bytes()[..], &input[..2]);
                &input[..2]
            }),
        ),
        3 => (
            0,
            r.u24().map(|v| {
                assert!(v < 1 << 24);
                &input[..3]
            }),
        ),
        4 => (
            1,
            r.vec8().map(|b| {
                assert_eq!(b.len(), input[0] as usize);
                b
            }),
        ),
        5 => (
            2,
            r.vec16().map(|b| {
                assert_eq!(b.len(), u16::from_be_bytes([input[0], input[1]]) as usize);
                b
            }),
        ),
        _ => (
            3,
            r.vec24().map(|b| {
                assert_eq!(b.len(), u32::from_be_bytes([0, input[0], input[1], input[2]]) as usize);
                b
            }),
        ),
    };
    match got {
        Ok(b) => {
            assert_eq!(offset_in(input, b), hdr);
            assert_eq!(r.0.len(), input.len() - hdr - b.len());
            assert_eq!(offset_in(input, r.0), hdr + b.len());
        }
        Err(_) if op < 4 => {
            assert_eq!(r.0.len(), input.len());
        }
        Err(_) => {}
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn extensions_any_input() {
    let buf: [u8; 20] = kani::any();
    let input = any_slice(&buf);
    let mut r = Reader(input);
    if let Ok(exts) = r.extensions() {
        let declared = u16::from_be_bytes([input[0], input[1]]) as usize;
        let mut covered = 0;
        let mut at = 2;
        for (i, (t, b)) in exts.iter().enumerate() {
            for (u, _) in &exts[..i] {
                assert_ne!(t, u);
            }
            assert_eq!(u16::from_be_bytes([input[at], input[at + 1]]), *t);
            assert_eq!(offset_in(input, b), at + 4);
            at += 4 + b.len();
            covered += 4 + b.len();
        }
        assert_eq!(covered, declared);
        assert_eq!(r.0.len(), input.len() - 2 - declared);
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn certificate_body_any_input() {
    let buf: [u8; 14] = kani::any();
    let body = any_slice(&buf);
    if let Ok(certs) = parse_certificate_body(body) {
        assert!(!certs.is_empty());
        let total: usize = certs.iter().map(|c| 5 + c.len()).sum();
        assert!(1 + 3 + total <= body.len());
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn ct_eq_is_equality() {
    let a: [u8; 6] = kani::any();
    let b: [u8; 6] = kani::any();
    let (a, b) = (any_slice(&a), any_slice(&b));
    assert_eq!(ct_eq(a, b), a == b);
}
