use super::*;

pub fn server(data: &[u8]) {
    if let Some((addr, used)) = parse_addr(data) {
        assert!(used <= data.len());
        let want: SocketAddr = match data[0] {
            1 => {
                assert_eq!(used, 7);
                (<[u8; 4]>::try_from(&data[1..5]).unwrap(), u16::from_be_bytes([data[5], data[6]])).into()
            }
            4 => {
                assert_eq!(used, 19);
                (<[u8; 16]>::try_from(&data[1..17]).unwrap(), u16::from_be_bytes([data[17], data[18]])).into()
            }
            _ => panic!("принят адрес типа {}", data[0]),
        };
        assert_eq!(addr, want);
    }
    let keys = Keys { cipher: Cipher::Aes128, v2022: false, key: Vec::new(), identity: Vec::new() };
    let mut codec = UdpCodec { k: &keys, rng: SystemRandom::new(), session: 0, packet: 0, aes: None, plain: true };
    let mut b = data.to_vec();
    let (len, whole) = (b.len(), b.as_ptr() as usize);
    if let Some((_, payload)) = codec.open(&mut b) {
        assert!(payload.len() + 7 <= len);
        assert_eq!(payload.as_ptr() as usize + payload.len(), whole + len);
    }
}
