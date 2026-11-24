use super::*;

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

#[kani::proof]
#[kani::unwind(18)]
fn take_headers_any_input() {
    let buf: [u8; 16] = kani::any();
    let b = any_slice(&buf);
    if let Some(Ok((block, used))) = take_headers(b) {
        assert!(used <= b.len());
        assert!(block.len() + 2 <= used);
        assert_eq!(&b[used - block.len()..used], &block[..]);
    }
}
