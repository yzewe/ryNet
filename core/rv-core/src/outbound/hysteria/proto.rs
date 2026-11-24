use ring::rand::{SecureRandom, SystemRandom};

pub const FRAME_TCP_REQUEST: u64 = 0x401;
const PAD_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub fn random_u32() -> u32 {
    let mut b = [0u8; 4];
    let _ = SystemRandom::new().fill(&mut b);
    u32::from_le_bytes(b)
}

pub fn padding(min: usize, max: usize) -> Vec<u8> {
    let n = min + random_u32() as usize % (max - min);
    let mut b = vec![0u8; n];
    let _ = SystemRandom::new().fill(&mut b);
    b.iter_mut().for_each(|x| *x = PAD_CHARS[*x as usize % PAD_CHARS.len()]);
    b
}

pub fn put_varint(o: &mut Vec<u8>, v: u64) {
    match v {
        0..=63 => o.push(v as u8),
        64..=16383 => o.extend_from_slice(&((v as u16) | 0x4000).to_be_bytes()),
        16384..=1073741823 => o.extend_from_slice(&((v as u32) | 0x8000_0000).to_be_bytes()),
        _ => o.extend_from_slice(&(v | 0xc000_0000_0000_0000).to_be_bytes()),
    }
}

pub fn get_varint(b: &[u8]) -> Option<(u64, usize)> {
    let first = *b.first()?;
    let len = 1usize << (first >> 6);
    if b.len() < len {
        return None;
    }
    let mut v = (first & 0x3f) as u64;
    for &x in &b[1..len] {
        v = (v << 8) | x as u64;
    }
    Some((v, len))
}

pub fn tcp_request(addr: &str) -> Vec<u8> {
    let pad = padding(64, 512);
    let mut o = Vec::with_capacity(8 + addr.len() + pad.len());
    put_varint(&mut o, FRAME_TCP_REQUEST);
    put_varint(&mut o, addr.len() as u64);
    o.extend_from_slice(addr.as_bytes());
    put_varint(&mut o, pad.len() as u64);
    o.extend_from_slice(&pad);
    o
}

pub const MAX_TCP_RESPONSE: usize = 64 * 1024;

pub fn tcp_response(b: &[u8]) -> Option<Result<usize, String>> {
    let status = *b.first()?;
    let too_long = || Some(Err("response header too long".to_string()));
    let (mlen, n1) = get_varint(b.get(1..)?)?;
    let mut i = 1 + n1;
    let Some(end) = usize::try_from(mlen).ok().and_then(|l| i.checked_add(l)).filter(|&e| e <= MAX_TCP_RESPONSE) else {
        return too_long();
    };
    let msg = b.get(i..end)?;
    i = end;
    let (plen, n2) = get_varint(b.get(i..)?)?;
    i += n2;
    let Some(end) = usize::try_from(plen).ok().and_then(|l| i.checked_add(l)).filter(|&e| e <= MAX_TCP_RESPONSE) else {
        return too_long();
    };
    b.get(i..end)?;
    i = end;
    Some(if status == 0 { Ok(i) } else { Err(String::from_utf8_lossy(msg).into_owned()) })
}

pub struct UdpMessage<'a> {
    pub session: u32,
    pub packet: u16,
    pub frag: u8,
    pub frags: u8,
    pub addr: &'a str,
    pub data: &'a [u8],
}

impl UdpMessage<'_> {
    fn header_len(addr: &str) -> usize {
        8 + if addr.len() <= 63 { 1 } else { 2 } + addr.len()
    }

    pub fn encode(&self, o: &mut Vec<u8>) {
        o.extend_from_slice(&self.session.to_be_bytes());
        o.extend_from_slice(&self.packet.to_be_bytes());
        o.push(self.frag);
        o.push(self.frags);
        put_varint(o, self.addr.len() as u64);
        o.extend_from_slice(self.addr.as_bytes());
        o.extend_from_slice(self.data);
    }
}

pub fn parse_udp(b: &[u8]) -> Option<UdpMessage<'_>> {
    let session = u32::from_be_bytes(b.get(0..4)?.try_into().ok()?);
    let packet = u16::from_be_bytes(b.get(4..6)?.try_into().ok()?);
    let (frag, frags) = (*b.get(6)?, *b.get(7)?);
    let (alen, n) = get_varint(&b[8..])?;
    let i = 8 + n;
    if alen == 0 || alen > 2048 || b.len() <= i + alen as usize {
        return None;
    }
    let addr = std::str::from_utf8(&b[i..i + alen as usize]).ok()?;
    Some(UdpMessage { session, packet, frag, frags, addr, data: &b[i + alen as usize..] })
}

pub fn udp_datagrams(session: u32, addr: &str, data: &[u8], max: usize) -> Vec<Vec<u8>> {
    let h = UdpMessage::header_len(addr);
    if h + data.len() <= max {
        let mut o = Vec::with_capacity(h + data.len());
        UdpMessage { session, packet: 0, frag: 0, frags: 1, addr, data }.encode(&mut o);
        return vec![o];
    }
    let room = max.saturating_sub(h);
    if room == 0 {
        return Vec::new();
    }
    let count = data.len().div_ceil(room);
    if count > 255 {
        return Vec::new();
    }
    let packet = (random_u32() % 0xffff) as u16 + 1;
    data.chunks(room)
        .enumerate()
        .map(|(i, c)| {
            let mut o = Vec::with_capacity(h + c.len());
            UdpMessage { session, packet, frag: i as u8, frags: count as u8, addr, data: c }.encode(&mut o);
            o
        })
        .collect()
}

#[derive(Default)]
pub struct Defragger {
    packet: u16,
    frags: Vec<Option<Vec<u8>>>,
    got: usize,
}

impl Defragger {
    pub fn feed<'a>(&mut self, m: &UdpMessage<'a>) -> Option<std::borrow::Cow<'a, [u8]>> {
        if m.frags <= 1 {
            return Some(std::borrow::Cow::Borrowed(m.data));
        }
        if m.frag >= m.frags {
            return None;
        }
        if m.packet != self.packet || m.frags as usize != self.frags.len() {
            self.packet = m.packet;
            self.frags = vec![None; m.frags as usize];
            self.got = 0;
        }
        let slot = &mut self.frags[m.frag as usize];
        if slot.is_none() {
            *slot = Some(m.data.to_vec());
            self.got += 1;
        }
        if self.got == self.frags.len() {
            self.got = 0;
            let parts = std::mem::take(&mut self.frags);
            return Some(std::borrow::Cow::Owned(parts.into_iter().flatten().flatten().collect()));
        }
        None
    }
}

#[cfg(kani)]
#[path = "../../proofs/hysteria_proto.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "../../fuzzing/hysteria_proto.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_response_lengths_are_rejected() {
        let mut b = vec![0u8];
        put_varint(&mut b, 1 << 40);
        assert!(matches!(tcp_response(&b), Some(Err(_))));
        let mut b = vec![0u8];
        put_varint(&mut b, 0);
        put_varint(&mut b, (MAX_TCP_RESPONSE as u64) * 2);
        assert!(matches!(tcp_response(&b), Some(Err(_))));
        let mut b = vec![0u8];
        put_varint(&mut b, 2);
        b.extend_from_slice(b"ok");
        put_varint(&mut b, 3);
        b.extend_from_slice(&[0; 3]);
        assert_eq!(tcp_response(&b), Some(Ok(b.len())));
    }

    #[test]
    fn varints() {
        for v in [0u64, 63, 64, 16383, 16384, 1073741823, 1073741824] {
            let mut o = Vec::new();
            put_varint(&mut o, v);
            assert_eq!(get_varint(&o), Some((v, o.len())));
        }
        let mut o = Vec::new();
        put_varint(&mut o, FRAME_TCP_REQUEST);
        assert_eq!(o, [0x44, 0x01]);
    }

    #[test]
    fn tcp_response_parse() {
        let mut b = vec![0u8];
        put_varint(&mut b, 0);
        put_varint(&mut b, 3);
        b.extend_from_slice(b"pad");
        b.extend_from_slice(b"DATA");
        assert_eq!(tcp_response(&b), Some(Ok(b.len() - 4)));
        assert_eq!(tcp_response(&b[..3]), None);
        let mut e = vec![1u8];
        put_varint(&mut e, 4);
        e.extend_from_slice(b"nope");
        put_varint(&mut e, 0);
        assert_eq!(tcp_response(&e), Some(Err("nope".into())));
    }

    #[test]
    fn udp_fragments_roundtrip() {
        let data: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
        let dgs = udp_datagrams(7, "1.2.3.4:53", &data, 1200);
        assert_eq!(dgs.len(), 3);
        let mut d = Defragger::default();
        let mut out = None;
        for g in &dgs {
            let m = parse_udp(g).unwrap();
            assert_eq!(m.session, 7);
            out = d.feed(&m).or(out);
        }
        assert_eq!(out.unwrap(), data);
        let one = udp_datagrams(1, "[::1]:443", b"x", 1200);
        let m = parse_udp(&one[0]).unwrap();
        assert_eq!((m.addr, m.data, m.frags), ("[::1]:443", &b"x"[..], 1));
    }
}
