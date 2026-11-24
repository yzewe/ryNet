use super::*;

pub fn frames(data: &[u8]) {
    let mut m = Bytes::copy_from_slice(data);
    if get_varint(&mut m).is_ok() {
        assert!((1..=10).contains(&(data.len() - m.remaining())));
    }
    let mut out = VecDeque::new();
    let ok = decode_hunks(Bytes::copy_from_slice(data), &mut out).is_ok();
    assert!(out.len() <= 1 && (ok || out.is_empty()));
    assert!(out.iter().all(|h| !h.is_empty()) && out.iter().map(Bytes::len).sum::<usize>() <= data.len());
    let f = encode_hunk(data);
    let mut back = VecDeque::new();
    decode_hunks(f.slice(5..), &mut back).unwrap();
    assert_eq!(back.iter().flat_map(|h| h.iter().copied()).collect::<Vec<u8>>(), data);
    let mut s = GrpcStream {
        resp: None,
        recv: None,
        send: None,
        inbuf: BytesMut::from(data),
        out: VecDeque::new(),
        pending: Bytes::new(),
        header_status: false,
        _lease: Lease { pool: Weak::new(), active: Rc::new(Cell::new(2)), idle: Rc::new(Idle::default()), dead: Rc::new(Cell::new(false)) },
    };
    let mut taken = 0;
    while s.parse().is_ok() {
        assert!(s.out.len() <= 1);
        let Some(h) = s.out.pop_front() else {
            let rest = &s.inbuf;
            if rest.len() >= 5 {
                assert!(rest.len() < 5 + u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize);
            }
            break;
        };
        taken += h.len();
        assert!(taken + s.inbuf.len() <= data.len());
    }
}
