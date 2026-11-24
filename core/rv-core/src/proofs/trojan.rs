use super::*;

fn no_format(_: std::fmt::Arguments<'_>) -> String {
    String::new()
}

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

#[kani::proof]
#[kani::unwind(28)]
#[kani::stub(std::fmt::format, no_format)]
fn parse_udp_any_input() {
    let buf: [u8; 26] = kani::any();
    let b = any_slice(&buf);
    if let Ok(Some((from, data, used))) = parse_udp(b) {
        assert!(used <= b.len());
        let start = used - data.len();
        assert_eq!(data.as_ptr(), b[start..].as_ptr());
        assert_eq!(&b[start - 2..start], b"\r\n");
        assert_eq!(start, if from.is_ipv4() { 1 + 4 + 6 } else { 1 + 16 + 6 });
        assert_eq!(u16::from_be_bytes([b[start - 6], b[start - 5]]), from.port());
        assert_eq!(u16::from_be_bytes([b[start - 4], b[start - 3]]) as usize, data.len());
    }
}

#[kani::proof]
#[kani::unwind(18)]
#[kani::stub(std::fmt::format, no_format)]
fn parse_udp_prefix_is_incomplete() {
    let buf: [u8; 16] = kani::any();
    let b = any_slice(&buf);
    let Ok(Some((_, _, used))) = parse_udp(b) else { return };
    let k: usize = kani::any();
    kani::assume(k < used);
    assert!(matches!(parse_udp(&b[..k]), Ok(None)));
}
