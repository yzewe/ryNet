use super::*;

fn zero_instant() -> std::time::Instant {
    unsafe { std::mem::transmute::<[u64; 2], std::time::Instant>([0, 0]) }
}

#[kani::proof]
#[kani::unwind(26)]
#[kani::stub(std::time::Instant::now, zero_instant)]
fn frames_observe_any_input() {
    let buf: [u8; 24] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= buf.len());
    let b = &buf[..len];
    let (i, j): (usize, usize) = (kani::any(), kani::any());
    kani::assume(i <= j && j <= len);
    let activity = Activity(Rc::new(Cell::new(Instant::now())));
    let bdp = Bdp::new();
    let mut f = Frames::default();
    for c in [&b[..i], &b[i..j], &b[j..]] {
        f.observe(c, &activity, Some(&*bdp));
        assert!(f.used <= 9);
        assert!(f.left <= f.payload);
        assert!(f.payload < 1 << 24);
    }
    assert!(bdp.sample.get() as usize <= len);
}
