use super::*;

#[kani::proof]
#[kani::unwind(34)]
fn dns_answer_any_input() {
    let buf: [u8; 32] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= buf.len());
    let id: u16 = kani::any();
    for qd in 0..=5u8 {
        for an in 0..=2u8 {
            let mut m = buf;
            m[4..8].copy_from_slice(&[0, qd, 0, an]);
            let b = &m[..len];
            if let Some(ips) = dns_answer(b, id) {
                assert!(b.len() >= 12 && u16::from_be_bytes([b[0], b[1]]) == id);
                assert!(12 + ips.len() * 15 <= b.len());
            }
        }
    }
}
