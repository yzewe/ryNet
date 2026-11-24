use super::*;

fn random_any((min, max): (u64, u64)) -> io::Result<u64> {
    if max < min {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid fragment range"));
    }
    let r: u64 = kani::any();
    kani::assume(min <= r && r <= max);
    Ok(r)
}

fn any_range() -> (u64, u64) {
    (kani::any(), kani::any())
}

fn any_cfg() -> FragmentConfig {
    let delays = if kani::any() { vec![(0, 0)] } else { vec![any_range()] };
    FragmentConfig { packets: (0, 1), lengths: vec![any_range()], delays, max_split: any_range() }
}

#[kani::proof]
#[kani::unwind(13)]
#[kani::stub(random, random_any)]
fn hello_chunks_any_input() {
    let buf: [u8; 16] = kani::any();
    let b = &buf[..];
    if !is_client_hello(b) {
        return;
    }
    let cfg = any_cfg();
    let Ok(q) = hello_chunks(b, &cfg) else { return };
    let rec_len = 5 + u16::from_be_bytes([b[3], b[4]]) as usize;
    assert!(rec_len <= b.len());
    let mut payload = Vec::new();
    let mut records = 0u64;
    let mut tail_seen = false;
    for (chunk, _) in &q {
        assert!(!tail_seen);
        let mut c = &chunk[..];
        if c.is_empty() {
            assert!(rec_len == 5 && cfg.merge_hello());
            continue;
        }
        if payload.len() == rec_len - 5 && b.len() > rec_len {
            assert!(c == &b[rec_len..]);
            tail_seen = true;
            continue;
        }
        while !c.is_empty() {
            assert!(c.len() >= 5);
            assert!(c[..3] == b[..3]);
            let l = u16::from_be_bytes([c[3], c[4]]) as usize;
            assert!(l >= 1 && 5 + l <= c.len());
            payload.extend_from_slice(&c[5..5 + l]);
            records += 1;
            c = &c[5 + l..];
        }
    }
    assert!(payload == &b[5..rec_len]);
    assert_eq!(tail_seen, b.len() > rec_len);
    if cfg.max_split.0 > 0 {
        assert!(records <= cfg.max_split.1);
    }
}

#[kani::proof]
#[kani::unwind(14)]
#[kani::stub(random, random_any)]
fn raw_chunks_cover_input() {
    let buf: [u8; 12] = kani::any();
    let b = &buf[..];
    let cfg = FragmentConfig { packets: (1, 1), lengths: vec![any_range()], delays: vec![any_range()], max_split: any_range() };
    let Ok(q) = raw_chunks(b, &cfg) else { return };
    let mut all = Vec::new();
    for (c, _) in &q {
        assert!(!c.is_empty());
        all.extend_from_slice(c);
    }
    assert!(all == b);
    if cfg.max_split.0 > 0 {
        assert!(q.len() as u64 <= cfg.max_split.1);
    }
}

#[kani::proof]
#[kani::unwind(2)]
fn is_client_hello_any_input() {
    let buf: [u8; 16] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= buf.len());
    let b = &buf[..len];
    if is_client_hello(b) {
        assert!(b.len() >= 6 && b[0] == 22 && b[5] == 1);
        assert!(5 + u16::from_be_bytes([b[3], b[4]]) as usize <= b.len());
    }
}
