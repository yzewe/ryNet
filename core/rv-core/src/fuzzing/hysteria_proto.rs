use super::*;

const VARINT_MAX: u64 = (1 << 62) - 1;

pub fn server(data: &[u8]) {
    match get_varint(data) {
        Some((v, n)) => {
            assert!(n == 1 << (data[0] >> 6) && n <= data.len() && v <= VARINT_MAX);
            let mut o = Vec::new();
            put_varint(&mut o, v);
            assert_eq!(get_varint(&o), Some((v, o.len())));
        }
        None => assert!(data.is_empty() || data.len() < 1 << (data[0] >> 6)),
    }
    if let Some(Ok(i)) = tcp_response(data) {
        assert!(i <= data.len() && i <= MAX_TCP_RESPONSE && data[0] == 0);
    }
    if let Some(m) = parse_udp(data) {
        assert!(!m.addr.is_empty() && m.addr.len() <= 2048 && !m.data.is_empty());
        let end = data.len() - m.data.len();
        assert_eq!(m.data.as_ptr(), data[end..].as_ptr());
        assert!(end - m.addr.len() >= 9);
        let mut o = Vec::new();
        m.encode(&mut o);
        let p = parse_udp(&o).unwrap();
        assert_eq!((p.session, p.packet, p.frag, p.frags, p.addr, p.data), (m.session, m.packet, m.frag, m.frags, m.addr, m.data));
    }
    let mut d = Defragger::default();
    let mut rest = data;
    let mut fed = 0;
    while rest.len() >= 4 {
        let len = (rest[3] as usize).min(rest.len() - 4);
        let m = UdpMessage {
            session: 0,
            packet: rest[0] as u16 % 4,
            frag: rest[1] % 8,
            frags: rest[2] % 8,
            addr: "a",
            data: &rest[4..4 + len],
        };
        rest = &rest[4 + len..];
        fed += m.data.len();
        if let Some(out) = d.feed(&m) {
            assert!(out.len() <= fed);
            if m.frags > 1 {
                fed = 0;
            }
        }
        assert!(d.got <= d.frags.len());
    }
}
