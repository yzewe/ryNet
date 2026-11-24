use std::io;

mod plain;
mod vision;
pub mod xudp;

pub use plain::VlessStream;
pub use vision::{DirectCapable, VisionStream};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    Ipv4([u8; 4]),
    Ipv6([u8; 16]),
    Domain(String),
}

impl Address {
    pub fn from_ip(ip: std::net::IpAddr) -> Self {
        match ip {
            std::net::IpAddr::V4(v) => Self::Ipv4(v.octets()),
            std::net::IpAddr::V6(v) => Self::Ipv6(v.octets()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Tcp = 1,
    Udp = 2,
    Mux = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    None,
    Vision,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub id: [u8; 16],
    pub flow: Flow,
    pub command: Command,
    pub address: Address,
    pub port: u16,
}

const VISION_FLOW: &[u8] = b"xtls-rprx-vision";

pub fn encode_request_header(req: &Request, out: &mut Vec<u8>) -> io::Result<()> {
    out.push(0);
    out.extend_from_slice(&req.id);
    match req.flow {
        Flow::Vision => {
            out.push((2 + VISION_FLOW.len()) as u8);
            out.push(0x0a);
            out.push(VISION_FLOW.len() as u8);
            out.extend_from_slice(VISION_FLOW);
        }
        Flow::None => out.push(0),
    }
    out.push(req.command as u8);
    if req.command != Command::Mux {
        out.extend_from_slice(&req.port.to_be_bytes());
        match &req.address {
            Address::Ipv4(a) => {
                out.push(1);
                out.extend_from_slice(a);
            }
            Address::Domain(d) => {
                let d = d.as_bytes();
                if d.is_empty() || d.len() > 255 {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "vless: bad domain length"));
                }
                out.push(2);
                out.push(d.len() as u8);
                out.extend_from_slice(d);
            }
            Address::Ipv6(a) => {
                out.push(3);
                out.extend_from_slice(a);
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_response_header(buf: &[u8]) -> io::Result<Option<usize>> {
    if buf.len() < 2 {
        return Ok(None);
    }
    if buf[0] != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("vless: unexpected response version {}", buf[0])));
    }
    let n = 2 + buf[1] as usize;
    Ok((buf.len() >= n).then_some(n))
}

#[cfg(kani)]
#[path = "proofs/lib.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/lib.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_header_vision_domain() {
        let req = Request {
            id: [0x11; 16],
            flow: Flow::Vision,
            command: Command::Tcp,
            address: Address::Domain("example.com".into()),
            port: 443,
        };
        let mut out = Vec::new();
        encode_request_header(&req, &mut out).unwrap();
        let mut exp = vec![0u8];
        exp.extend([0x11; 16]);
        exp.extend([18, 0x0a, 16]);
        exp.extend(b"xtls-rprx-vision");
        exp.extend([1, 0x01, 0xbb, 2, 11]);
        exp.extend(b"example.com");
        assert_eq!(out, exp);
    }

    #[test]
    fn request_header_plain_ipv6_and_mux() {
        let mut req = Request { id: [0; 16], flow: Flow::None, command: Command::Udp, address: Address::Ipv6([0xfe; 16]), port: 53 };
        let mut out = Vec::new();
        encode_request_header(&req, &mut out).unwrap();
        assert_eq!(
            &out[17..],
            &[0, 2, 0, 53, 3, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe]
        );
        req.command = Command::Mux;
        out.clear();
        encode_request_header(&req, &mut out).unwrap();
        assert_eq!(&out[17..], &[0, 3]);
    }

    #[test]
    fn response_header() {
        assert_eq!(parse_response_header(&[0]).unwrap(), None);
        assert_eq!(parse_response_header(&[0, 0]).unwrap(), Some(2));
        assert_eq!(parse_response_header(&[0, 2, 9]).unwrap(), None);
        assert_eq!(parse_response_header(&[0, 2, 9, 9, 7]).unwrap(), Some(4));
        assert!(parse_response_header(&[1, 0]).is_err());
    }
}
