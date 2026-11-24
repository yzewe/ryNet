use super::*;

const MAX: usize = 16;

fn any_len(max: usize) -> usize {
    let n: usize = kani::any();
    kani::assume(n <= max);
    n
}

#[kani::proof]
#[kani::unwind(50)]
fn chain_op_matches_fifo() {
    let init: [u8; 2 * MAX] = kani::any();
    let (a, b) = (any_len(MAX), any_len(MAX));
    let k = any_len(a);
    let mut c = Chain::default();
    c.push(&init[..a]);
    c.consume(k);
    c.push(&init[MAX..MAX + b]);
    let mut m = [0u8; 4 * MAX];
    let mut len = 0;
    for &x in init[k..a].iter().chain(&init[MAX..MAX + b]) {
        m[len] = x;
        len += 1;
    }
    let mut start = 0;

    let n = any_len(MAX);
    let data: [u8; MAX] = kani::any();
    match kani::any::<u8>() % 5 {
        0 => {
            c.push(&data[..n]);
            m[start + len..start + len + n].copy_from_slice(&data[..n]);
            len += n;
        }
        1 => {
            c.consume(n);
            let r = n.min(len);
            start += r;
            len -= r;
        }
        2 => {
            let mut out = [0u8; MAX];
            let r = c.read(&mut out[..n]);
            assert_eq!(r, n.min(len));
            assert_eq!(&out[..r], &m[start..start + r]);
            start += r;
            len -= r;
        }
        3 => {
            let limit = any_len(MAX);
            let space = c.tail_space(limit);
            assert!(space.len() <= limit);
            let w = n.min(space.len());
            space[..w].copy_from_slice(&data[..w]);
            c.commit(w);
            m[start + len..start + len + w].copy_from_slice(&data[..w]);
            len += w;
        }
        _ => {
            c.clear();
            len = 0;
        }
    }
    assert_eq!(c.len(), len);
    assert_eq!(c.is_empty(), len == 0);
    let f = c.front();
    assert!(f.len() <= len && f.is_empty() == (len == 0));
    assert_eq!(f, &m[start..start + f.len()]);
    let mut at = start;
    c.slices(0, c.len(), |s| {
        assert_eq!(s, &m[at..at + s.len()]);
        at += s.len();
    });
    assert_eq!(at, start + len);
}

#[kani::proof]
#[kani::unwind(4)]
fn drained_chain_holds_no_blocks() {
    static ZEROS: [u8; 2 * 16] = [0; 2 * 16];
    let mut c = Chain::default();
    let n: usize = kani::any();
    kani::assume(n <= 2 * CHUNK);
    c.push(&ZEROS[..n]);
    c.consume(kani::any::<u8>() as usize);
    if c.is_empty() {
        assert!(c.chunks.is_empty() && c.head == 0);
    }
}

#[kani::proof]
#[kani::unwind(3)]
fn unused_tail_block_is_returned() {
    let mut c = Chain::default();
    let _ = c.tail_space(kani::any::<u8>() as usize);
    c.commit(0);
    assert!(c.is_empty() && c.chunks.is_empty());
}

#[kani::proof]
#[kani::unwind(8)]
fn segment_spans_at_most_five_real_chunks() {
    const REAL: usize = 16 * 1024;
    let head: usize = kani::any();
    let len: usize = kani::any();
    kani::assume(head < REAL && len <= 65535 - 40);
    let mut pos = head;
    let end = pos + len;
    let mut pieces = 0;
    while pos < end {
        let o = pos % REAL;
        let n = (REAL - o).min(end - pos);
        pos += n;
        pieces += 1;
    }
    assert!(pieces <= 5);
}
