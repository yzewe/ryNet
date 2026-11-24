use super::*;

pub fn headers(data: &[u8]) {
    if let Some(Ok((block, used))) = take_headers(data) {
        assert!(used <= data.len() && block.len() + 2 <= used);
        assert_eq!(&data[used - block.len()..used], &block[..]);
    }
}
