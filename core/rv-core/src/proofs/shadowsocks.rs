use super::*;

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

#[kani::proof]
#[kani::unwind(22)]
fn parse_addr_any_input() {
    let buf: [u8; 21] = kani::any();
    let b = any_slice(&buf);
    if let Some((addr, used)) = parse_addr(b) {
        assert!(used <= b.len());
        let want: SocketAddr = match b[0] {
            1 => {
                assert_eq!(used, 7);
                (<[u8; 4]>::try_from(&b[1..5]).unwrap(), u16::from_be_bytes([b[5], b[6]])).into()
            }
            4 => {
                assert_eq!(used, 19);
                (<[u8; 16]>::try_from(&b[1..17]).unwrap(), u16::from_be_bytes([b[17], b[18]])).into()
            }
            _ => unreachable!(),
        };
        assert_eq!(addr, want);
    }
}

#[kani::proof]
#[kani::unwind(22)]
fn udp_open_plain_any_input() {
    let keys = Keys { cipher: Cipher::Aes128, v2022: false, key: Vec::new(), identity: Vec::new() };
    let mut codec = UdpCodec { k: &keys, rng: SystemRandom::new(), session: 0, packet: 0, aes: None, plain: true };
    let mut buf: [u8; 21] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= buf.len());
    let b = &mut buf[..len];
    let whole = b.as_ptr() as usize;
    if let Some((_, data)) = codec.open(b) {
        assert!(data.len() + 7 <= len);
        assert_eq!(data.as_ptr() as usize + data.len(), whole + len);
    }
}
