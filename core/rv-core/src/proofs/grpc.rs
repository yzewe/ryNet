use super::*;

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

#[kani::proof]
#[kani::unwind(12)]
fn get_varint_any_input() {
    let buf: [u8; 11] = kani::any();
    let b = any_slice(&buf);
    let mut m = Bytes::copy_from_slice(b);
    if get_varint(&mut m).is_ok() {
        let used = b.len() - m.remaining();
        assert!(used >= 1 && used <= 10);
        assert!(b[used - 1] & 0x80 == 0);
        assert!(b[..used - 1].iter().all(|x| x & 0x80 != 0));
    }
}

#[kani::proof]
#[kani::unwind(12)]
fn varint_roundtrip() {
    let v: u64 = kani::any();
    let mut b = BytesMut::new();
    put_varint(&mut b, v);
    let len = b.len();
    assert_eq!(len, (64 - v.leading_zeros()).max(1).div_ceil(7) as usize);
    b.put_u8(kani::any());
    let mut m = b.freeze();
    assert_eq!(get_varint(&mut m).unwrap(), v);
    assert_eq!(m.remaining(), 1);
}

#[kani::proof]
#[kani::unwind(14)]
fn decode_hunks_any_input() {
    let buf: [u8; 12] = kani::any();
    let b = any_slice(&buf);
    let mut out = VecDeque::new();
    let ok = decode_hunks(Bytes::copy_from_slice(b), &mut out).is_ok();
    assert!(out.len() <= 1);
    assert!(ok || out.is_empty());
    let mut total = 0;
    for h in &out {
        assert!(!h.is_empty());
        total += h.len();
    }
    assert!(total <= b.len());
}

#[kani::proof]
#[kani::unwind(10)]
fn hunk_roundtrip() {
    let buf: [u8; 6] = kani::any();
    let data = any_slice(&buf);
    let f = encode_hunk(data);
    assert_eq!(f[0], 0);
    assert_eq!(u32::from_be_bytes([f[1], f[2], f[3], f[4]]) as usize, f.len() - 5);
    let mut out = VecDeque::new();
    decode_hunks(f.slice(5..), &mut out).unwrap();
    if data.is_empty() {
        assert!(out.is_empty());
    } else {
        assert_eq!(out.len(), 1);
        assert_eq!(&out[0][..], data);
    }
}

fn decode_hunks_any(m: Bytes, out: &mut VecDeque<Bytes>) -> io::Result<()> {
    if kani::any() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    if !m.is_empty() && kani::any() {
        let n: usize = kani::any();
        kani::assume(n >= 1 && n <= m.len());
        out.push_back(m.slice(..n));
    }
    Ok(())
}

#[kani::proof]
#[kani::stub(super::decode_hunks, decode_hunks_any)]
#[kani::unwind(13)]
fn stream_parse_any_input() {
    let buf: [u8; 11] = kani::any();
    let b = any_slice(&buf);
    let mut s = std::mem::ManuallyDrop::new(GrpcStream {
        resp: None,
        recv: None,
        send: None,
        inbuf: BytesMut::from(b),
        out: VecDeque::new(),
        pending: Bytes::new(),
        header_status: false,
        _lease: Lease { pool: Weak::new(), active: Rc::new(Cell::new(2)), idle: Rc::new(Idle::default()), dead: Rc::new(Cell::new(false)) },
    });
    let ok = s.parse().is_ok();
    let total: usize = s.out.iter().map(|h| h.len()).sum();
    assert!(total + s.inbuf.len() <= b.len());
    assert!(s.out.len() <= 1);
    if ok {
        let rest = &s.inbuf;
        assert_eq!(&b[b.len() - rest.len()..], &rest[..]);
        if rest.len() >= 5 && s.out.is_empty() {
            let len = u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
            assert!(rest.len() < 5 + len);
        }
    }
}
