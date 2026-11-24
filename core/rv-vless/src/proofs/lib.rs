use super::*;

fn no_format(_: std::fmt::Arguments<'_>) -> String {
    String::new()
}

#[kani::proof]
#[kani::unwind(26)]
#[kani::stub(std::fmt::format, no_format)]
fn response_header_any_input() {
    let buf: [u8; 24] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= buf.len());
    let b = &buf[..len];
    match parse_response_header(b) {
        Ok(Some(n)) => {
            assert!(n <= b.len());
            assert_eq!(n, 2 + b[1] as usize);
            assert_eq!(b[0], 0);
        }
        Ok(None) => assert!(b.len() < 2 || b.len() < 2 + b[1] as usize),
        Err(_) => assert!(b.len() >= 2 && b[0] != 0),
    }
}
