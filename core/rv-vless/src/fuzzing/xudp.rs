use super::*;

fn drain(p: &mut Parser, got: &mut Vec<Packet>) -> bool {
    loop {
        match p.next_packet() {
            Ok(Some(pk)) => got.push(pk),
            Ok(None) => return true,
            Err(_) => return false,
        }
    }
}

pub fn packets(data: &[u8]) {
    let Some((&ctl, input)) = data.split_first() else { return };
    let mut whole = Parser::default();
    whole.push(input);
    let (mut want, mut total) = (Vec::new(), 0);
    let ok_whole = loop {
        match whole.next_packet() {
            Ok(Some(pk)) => {
                assert!(!pk.payload.is_empty());
                total += pk.payload.len() + 8;
                assert!(total <= input.len() && whole.pos <= whole.buf.len());
                want.push(pk);
            }
            Ok(None) => break true,
            Err(_) => break false,
        }
    };
    assert!(whole.pos <= whole.buf.len());

    let step = usize::from(ctl).max(1);
    let mut parts = Parser::default();
    let mut got = Vec::new();
    let mut ok = true;
    for c in input.chunks(step) {
        let left = parts.buf.len() - parts.pos;
        parts.push(c);
        assert_eq!(parts.buf.len(), left + c.len());
        if !drain(&mut parts, &mut got) {
            ok = false;
            break;
        }
    }
    assert_eq!(ok, ok_whole);
    assert!(got == want, "кусками разобралось не то, что целиком");
}

pub fn addr(data: &[u8]) {
    if let Ok((addr, port)) = parse_addr(data) {
        assert_eq!(port, u16::from_be_bytes([data[0], data[1]]));
        match addr {
            Address::Ipv4(a) => assert!(data[2] == 1 && data.len() >= 7 && a == data[3..7]),
            Address::Ipv6(a) => assert!(data[2] == 3 && data.len() >= 19 && a == data[3..19]),
            Address::Domain(_) => assert!(data[2] == 2 && data.len() >= 4 + usize::from(data[3])),
        }
    }
    let [kind, p0, p1, rest @ ..] = data else { return };
    let Some((gid, rest)) = rest.split_first_chunk::<8>() else { return };
    let (addr, payload) = match kind % 3 {
        0 => match rest.split_first_chunk::<4>() {
            Some((a, p)) => (Address::Ipv4(*a), p),
            None => return,
        },
        1 => match rest.split_first_chunk::<16>() {
            Some((a, p)) => (Address::Ipv6(*a), p),
            None => return,
        },
        _ => {
            let Some((&n, rest)) = rest.split_first() else { return };
            let (d, p) = rest.split_at(usize::from(n).min(rest.len()));
            let d = String::from_utf8_lossy(d).into_owned();
            if d.len() > 255 {
                return;
            }
            (Address::Domain(d), p)
        }
    };
    let (port, first) = (u16::from_be_bytes([*p0, *p1]), kind & 4 != 0);
    let mut wire = Vec::new();
    if payload.is_empty() || payload.len() > MAX_PAYLOAD {
        assert!(!encode(&mut wire, first, &addr, port, gid, payload) && wire.is_empty());
        return;
    }
    assert!(encode(&mut wire, first, &addr, port, gid, payload));
    let mut p = Parser::default();
    p.push(&wire);
    let pk = p.next_packet().unwrap().unwrap();
    assert!(pk.from == Some((addr, port)) && pk.payload == payload);
    assert_eq!(p.pos, wire.len());
    assert!(matches!(p.next_packet(), Ok(None)));
}
