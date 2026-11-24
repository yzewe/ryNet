use std::io;

use crate::Address;

const STATUS_NEW: u8 = 1;
const STATUS_KEEP: u8 = 2;
const STATUS_END: u8 = 3;
const STATUS_KEEPALIVE: u8 = 4;
const OPTION_DATA: u8 = 1;
const NETWORK_UDP: u8 = 2;

pub const MAX_PAYLOAD: usize = 8192 - 666;

fn put_addr(out: &mut Vec<u8>, addr: &Address, port: u16) {
    out.extend_from_slice(&port.to_be_bytes());
    match addr {
        Address::Ipv4(a) => {
            out.push(1);
            out.extend_from_slice(a);
        }
        Address::Domain(d) => {
            out.push(2);
            out.push(d.len() as u8);
            out.extend_from_slice(d.as_bytes());
        }
        Address::Ipv6(a) => {
            out.push(3);
            out.extend_from_slice(a);
        }
    }
}

pub fn encode(out: &mut Vec<u8>, first: bool, addr: &Address, port: u16, global_id: &[u8; 8], payload: &[u8]) -> bool {
    if payload.is_empty() || payload.len() > MAX_PAYLOAD {
        return false;
    }
    let start = out.len();
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.push(if first { STATUS_NEW } else { STATUS_KEEP });
    out.push(OPTION_DATA);
    out.push(NETWORK_UDP);
    put_addr(out, addr, port);
    if first {
        out.extend_from_slice(global_id);
    }
    let meta = (out.len() - start - 2) as u16;
    out[start..start + 2].copy_from_slice(&meta.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    true
}

#[derive(Debug, PartialEq, Eq)]
pub struct Packet {
    pub from: Option<(Address, u16)>,
    pub payload: Vec<u8>,
}

#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
    pos: usize,
}

impl Parser {
    pub fn push(&mut self, data: &[u8]) {
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.extend_from_slice(data);
    }

    pub fn next_packet(&mut self) -> io::Result<Option<Packet>> {
        loop {
            let b = &self.buf[self.pos..];
            if b.len() < 2 {
                return Ok(None);
            }
            let meta = u16::from_be_bytes([b[0], b[1]]) as usize;
            if meta < 4 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "xudp: end"));
            }
            if b.len() < 2 + meta {
                return Ok(None);
            }
            let m = &b[2..2 + meta];
            let (status, option) = (m[2], m[3]);
            let mut from = None;
            match status {
                STATUS_KEEP | STATUS_NEW if meta > 4 && m[4] == NETWORK_UDP => from = Some(parse_addr(&m[5..])?),
                STATUS_KEEP | STATUS_NEW | STATUS_KEEPALIVE => {}
                STATUS_END => {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "xudp: end"));
                }
                s => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, format!("xudp: status {s}")));
                }
            }
            let mut consumed = 2 + meta;
            let mut payload = Vec::new();
            if option & OPTION_DATA != 0 {
                if b.len() < consumed + 2 {
                    return Ok(None);
                }
                let len = u16::from_be_bytes([b[consumed], b[consumed + 1]]) as usize;
                if b.len() < consumed + 2 + len {
                    return Ok(None);
                }
                payload.extend_from_slice(&b[consumed + 2..consumed + 2 + len]);
                consumed += 2 + len;
            }
            self.pos += consumed;
            if status == STATUS_KEEPALIVE || payload.is_empty() {
                continue;
            }
            return Ok(Some(Packet { from, payload }));
        }
    }
}

fn parse_addr(b: &[u8]) -> io::Result<(Address, u16)> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "xudp: bad address");
    if b.len() < 3 {
        return Err(bad());
    }
    let port = u16::from_be_bytes([b[0], b[1]]);
    let a = &b[3..];
    let addr = match b[2] {
        1 => Address::Ipv4(a.get(..4).ok_or_else(bad)?.try_into().unwrap()),
        3 => Address::Ipv6(a.get(..16).ok_or_else(bad)?.try_into().unwrap()),
        2 => {
            let n = *a.first().ok_or_else(bad)? as usize;
            Address::Domain(String::from_utf8_lossy(a.get(1..1 + n).ok_or_else(bad)?).into_owned())
        }
        _ => return Err(bad()),
    };
    Ok((addr, port))
}

#[cfg(kani)]
#[path = "proofs/xudp.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/xudp.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_frame_layout_matches_xray() {
        let mut out = Vec::new();
        assert!(encode(&mut out, true, &Address::Ipv4([8, 8, 8, 8]), 53, &[7; 8], b"hi"));
        assert_eq!(&out[..2], &[0, 20]);
        assert_eq!(&out[2..7], &[0, 0, 1, 1, 2]);
        assert_eq!(&out[7..14], &[0, 53, 1, 8, 8, 8, 8]);
        assert_eq!(&out[14..22], &[7; 8]);
        assert_eq!(&out[22..], &[0, 2, b'h', b'i']);

        let mut keep = Vec::new();
        encode(&mut keep, false, &Address::Ipv4([1, 1, 1, 1]), 443, &[0; 8], b"x");
        assert_eq!(&keep[..2], &[0, 12]);
        assert_eq!(keep[4], 2);
        assert!(!encode(&mut keep, false, &Address::Ipv4([1, 1, 1, 1]), 1, &[0; 8], &[0; MAX_PAYLOAD + 1]));
    }

    #[test]
    fn parser_buffer_stays_bounded() {
        let frame = [0u8, 4, 0, 0, STATUS_KEEP, OPTION_DATA, 0, 100].iter().copied().chain([7u8; 100]).collect::<Vec<_>>();
        let wire: Vec<u8> = frame.iter().copied().cycle().take(frame.len() * 1000).collect();
        let mut p = Parser::default();
        let mut n = 0;
        for c in wire.chunks(frame.len() + 3) {
            p.push(c);
            while let Ok(Some(_)) = p.next_packet() {
                n += 1;
            }
            assert!(p.buf.len() < 3 * frame.len(), "buffer grew to {}", p.buf.len());
        }
        assert_eq!(n, 1000);
    }

    #[test]
    fn parser_streams_frames_in_pieces() {
        let mut wire = Vec::new();
        encode(&mut wire, false, &Address::Ipv6([1; 16]), 3478, &[0; 8], b"stun");
        wire.extend_from_slice(&[0, 4, 0, 0, STATUS_KEEPALIVE, 0]);
        wire.extend_from_slice(&[0, 4, 0, 0, STATUS_KEEP, OPTION_DATA, 0, 3, b'a', b'b', b'c']);
        wire.extend_from_slice(&[0, 4, 0, 0, STATUS_END, 0]);
        for step in [1, 3, 7, wire.len()] {
            let mut p = Parser::default();
            let mut got = Vec::new();
            let mut ended = false;
            for c in wire.chunks(step) {
                p.push(c);
                loop {
                    match p.next_packet() {
                        Ok(Some(pk)) => got.push(pk),
                        Ok(None) => break,
                        Err(_) => {
                            ended = true;
                            break;
                        }
                    }
                }
            }
            assert!(ended, "step {step}");
            assert_eq!(got.len(), 2, "step {step}");
            assert_eq!(got[0].from, Some((Address::Ipv6([1; 16]), 3478)));
            assert_eq!(got[0].payload, b"stun");
            assert_eq!(got[1].from, None);
            assert_eq!(got[1].payload, b"abc");
        }
    }
}
