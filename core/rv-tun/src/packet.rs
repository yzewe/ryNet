use std::net::{IpAddr, SocketAddr};

use crate::tcp::csum::Csum;

pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;
pub const PROTO_ICMP: u8 = 1;
pub const PROTO_ICMPV6: u8 = 58;

pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_ACK: u8 = 0x10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub version: u8,
    pub proto: u8,
    pub ip_header_len: usize,
    pub total_len: usize,
    pub src: IpAddr,
    pub dst: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub tcp_flags: u8,
    pub payload_offset: usize,
}

pub fn parse(pkt: &[u8]) -> Option<Parsed> {
    let first = *pkt.first()?;
    let version = first >> 4;
    let (src, dst, off, total, proto) = if version == 4 {
        if pkt.len() < 20 {
            return None;
        }
        let ihl = ((first & 15) as usize) * 4;
        let total = u16::from_be_bytes([pkt[2], pkt[3]]) as usize;
        let frag = u16::from_be_bytes([pkt[6], pkt[7]]);
        if ihl < 20 || total < ihl || total > pkt.len() || frag & 0x3fff != 0 {
            return None;
        }
        (IpAddr::from([pkt[12], pkt[13], pkt[14], pkt[15]]), IpAddr::from([pkt[16], pkt[17], pkt[18], pkt[19]]), ihl, total, pkt[9])
    } else if version == 6 {
        if pkt.len() < 40 {
            return None;
        }
        let total = 40usize.checked_add(u16::from_be_bytes([pkt[4], pkt[5]]) as usize)?;
        if total > pkt.len() {
            return None;
        }
        let mut next = pkt[6];
        let mut pos = 40;
        loop {
            match next {
                0 | 43 | 60 => {
                    if pos + 2 > total {
                        return None;
                    }
                    let n = pkt[pos];
                    let len = (pkt[pos + 1] as usize + 1) * 8;
                    if pos + len > total {
                        return None;
                    }
                    next = n;
                    pos += len;
                }
                44 => return None,
                _ => break,
            }
        }
        (IpAddr::from(<[u8; 16]>::try_from(&pkt[8..24]).ok()?), IpAddr::from(<[u8; 16]>::try_from(&pkt[24..40]).ok()?), pos, total, next)
    } else {
        return None;
    };
    let (mut sport, mut dport, mut flags) = (0, 0, 0);
    let mut payload = off;
    match proto {
        PROTO_TCP => {
            if off + 20 > total {
                return None;
            }
            let h = ((pkt[off + 12] >> 4) as usize) * 4;
            if h < 20 || off + h > total {
                return None;
            }
            sport = u16::from_be_bytes([pkt[off], pkt[off + 1]]);
            dport = u16::from_be_bytes([pkt[off + 2], pkt[off + 3]]);
            flags = pkt[off + 13];
            payload = off + h;
        }
        PROTO_UDP => {
            if off + 8 > total {
                return None;
            }
            let n = u16::from_be_bytes([pkt[off + 4], pkt[off + 5]]) as usize;
            if n < 8 || off + n > total {
                return None;
            }
            sport = u16::from_be_bytes([pkt[off], pkt[off + 1]]);
            dport = u16::from_be_bytes([pkt[off + 2], pkt[off + 3]]);
            payload = off + 8;
        }
        _ => {}
    }
    Some(Parsed {
        version,
        proto,
        ip_header_len: off,
        total_len: total,
        src,
        dst,
        src_port: sport,
        dst_port: dport,
        tcp_flags: flags,
        payload_offset: payload,
    })
}

pub fn rewrite(pkt: &mut [u8], p: &Parsed, src: SocketAddr, dst: SocketAddr) {
    assert_eq!(p.version, if src.is_ipv4() { 4 } else { 6 });
    assert_eq!(p.version, if dst.is_ipv4() { 4 } else { 6 });
    let ao = if p.version == 4 { 12 } else { 8 };
    let al = if p.version == 4 { 4 } else { 16 };
    let (sa, da) = match (src.ip(), dst.ip()) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let mut x = [0; 16];
            let mut y = [0; 16];
            x[..4].copy_from_slice(&a.octets());
            y[..4].copy_from_slice(&b.octets());
            (x, y)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => (a.octets(), b.octets()),
        _ => unreachable!(),
    };
    if p.version == 4 {
        let mut c = u16::from_be_bytes([pkt[10], pkt[11]]);
        c = adjust_bytes(c, &pkt[ao..ao + al], &sa[..al]);
        c = adjust_bytes(c, &pkt[ao + al..ao + 2 * al], &da[..al]);
        pkt[10..12].copy_from_slice(&c.to_be_bytes());
    }
    let t = p.ip_header_len;
    let co = match p.proto {
        PROTO_TCP => t + 16,
        PROTO_UDP => t + 6,
        _ => usize::MAX,
    };
    if co != usize::MAX {
        let old = u16::from_be_bytes([pkt[co], pkt[co + 1]]);
        if !(p.proto == PROTO_UDP && p.version == 4 && old == 0) {
            let mut c = adjust_bytes(old, &pkt[ao..ao + al], &sa[..al]);
            c = adjust_bytes(c, &pkt[ao + al..ao + 2 * al], &da[..al]);
            c = adjust_word(c, u16::from_be_bytes([pkt[t], pkt[t + 1]]), src.port());
            c = adjust_word(c, u16::from_be_bytes([pkt[t + 2], pkt[t + 3]]), dst.port());
            if p.proto == PROTO_UDP && c == 0 {
                c = 0xffff
            }
            pkt[co..co + 2].copy_from_slice(&c.to_be_bytes());
        }
    }
    pkt[ao..ao + al].copy_from_slice(&sa[..al]);
    pkt[ao + al..ao + 2 * al].copy_from_slice(&da[..al]);
    if co != usize::MAX {
        pkt[t..t + 2].copy_from_slice(&src.port().to_be_bytes());
        pkt[t + 2..t + 4].copy_from_slice(&dst.port().to_be_bytes());
    }
}

pub fn udp_header(src: SocketAddr, dst: SocketAddr, payload: &[u8], out: &mut [u8; 48]) -> usize {
    let mut c = Csum::default();
    let ulen = 8 + payload.len();
    let n = match (src.ip(), dst.ip()) {
        (IpAddr::V4(sa), IpAddr::V4(da)) => {
            out[..20].fill(0);
            out[0] = 0x45;
            out[2..4].copy_from_slice(&((20 + ulen) as u16).to_be_bytes());
            out[6] = 0x40;
            out[8] = 64;
            out[9] = PROTO_UDP;
            out[12..16].copy_from_slice(&sa.octets());
            out[16..20].copy_from_slice(&da.octets());
            let ic = checksum(&out[..20]);
            out[10..12].copy_from_slice(&ic.to_be_bytes());
            c.add(&out[12..20]);
            20
        }
        (IpAddr::V6(sa), IpAddr::V6(da)) => {
            out[..40].fill(0);
            out[0] = 0x60;
            out[4..6].copy_from_slice(&(ulen as u16).to_be_bytes());
            out[6] = PROTO_UDP;
            out[7] = 64;
            out[8..24].copy_from_slice(&sa.octets());
            out[24..40].copy_from_slice(&da.octets());
            c.add(&out[8..40]);
            40
        }
        _ => return 0,
    };
    c.add_u16(PROTO_UDP as u16);
    c.add_u16(ulen as u16);
    let u = &mut out[n..n + 8];
    u[0..2].copy_from_slice(&src.port().to_be_bytes());
    u[2..4].copy_from_slice(&dst.port().to_be_bytes());
    u[4..6].copy_from_slice(&(ulen as u16).to_be_bytes());
    u[6..8].fill(0);
    c.add(u);
    c.add(payload);
    let sum = c.finish();
    out[n + 6..n + 8].copy_from_slice(&(if sum == 0 { 0xffff } else { sum }).to_be_bytes());
    n + 8
}

pub fn build_udp(src: SocketAddr, dst: SocketAddr, payload: &[u8], out: &mut Vec<u8>) {
    out.clear();
    if src.is_ipv4() != dst.is_ipv4() || payload.len() > 65535 - 48 {
        return;
    }
    if src.is_ipv4() {
        let (IpAddr::V4(sa), IpAddr::V4(da)) = (src.ip(), dst.ip()) else { unreachable!() };
        let n = 28 + payload.len();
        out.resize(n, 0);
        out[0] = 0x45;
        out[2..4].copy_from_slice(&(n as u16).to_be_bytes());
        out[6] = 0x40;
        out[8] = 64;
        out[9] = 17;
        out[12..16].copy_from_slice(&sa.octets());
        out[16..20].copy_from_slice(&da.octets());
        out[20..22].copy_from_slice(&src.port().to_be_bytes());
        out[22..24].copy_from_slice(&dst.port().to_be_bytes());
        out[24..26].copy_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
        out[28..].copy_from_slice(payload);
        let c = checksum(&out[..20]);
        out[10..12].copy_from_slice(&c.to_be_bytes());
        let c = transport_checksum(out, 20, 17, 4);
        out[26..28].copy_from_slice(&(if c == 0 { 0xffff } else { c }).to_be_bytes());
    } else {
        let (IpAddr::V6(sa), IpAddr::V6(da)) = (src.ip(), dst.ip()) else { unreachable!() };
        let n = 48 + payload.len();
        out.resize(n, 0);
        out[0] = 0x60;
        out[4..6].copy_from_slice(&((n - 40) as u16).to_be_bytes());
        out[6] = 17;
        out[7] = 64;
        out[8..24].copy_from_slice(&sa.octets());
        out[24..40].copy_from_slice(&da.octets());
        out[40..42].copy_from_slice(&src.port().to_be_bytes());
        out[42..44].copy_from_slice(&dst.port().to_be_bytes());
        out[44..46].copy_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
        out[48..].copy_from_slice(payload);
        let c = transport_checksum(out, 40, 17, 6);
        out[46..48].copy_from_slice(&(if c == 0 { 0xffff } else { c }).to_be_bytes());
    }
}

pub fn build_port_unreachable(orig: &[u8], p: &Parsed, out: &mut Vec<u8>) {
    out.clear();
    let n = p.total_len.min(orig.len());
    if p.version == 4 {
        let q = (p.ip_header_len + 8).min(n);
        let total = 28 + q;
        out.resize(total, 0);
        out[0] = 0x45;
        out[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        out[8] = 64;
        out[9] = 1;
        out[12..16].copy_from_slice(&orig[16..20]);
        out[16..20].copy_from_slice(&orig[12..16]);
        out[20] = 3;
        out[21] = 3;
        out[28..].copy_from_slice(&orig[..q]);
        let c = checksum(&out[20..]);
        out[22..24].copy_from_slice(&c.to_be_bytes());
        let c = checksum(&out[..20]);
        out[10..12].copy_from_slice(&c.to_be_bytes());
    } else {
        let q = n.min(1232);
        let total = 48 + q;
        out.resize(total, 0);
        out[0] = 0x60;
        out[4..6].copy_from_slice(&((total - 40) as u16).to_be_bytes());
        out[6] = 58;
        out[7] = 64;
        out[8..24].copy_from_slice(&orig[24..40]);
        out[24..40].copy_from_slice(&orig[8..24]);
        out[40] = 1;
        out[41] = 4;
        out[48..].copy_from_slice(&orig[..q]);
        let c = transport_checksum(out, 40, 58, 6);
        out[42..44].copy_from_slice(&c.to_be_bytes());
    }
}

pub fn build_tcp_rst(orig: &[u8], p: &Parsed, out: &mut Vec<u8>) {
    out.clear();
    let t = p.ip_header_len;
    if p.proto != PROTO_TCP || orig.len() < t + 20 {
        return;
    }
    let seq = u32::from_be_bytes([orig[t + 4], orig[t + 5], orig[t + 6], orig[t + 7]]);
    let ack_in = u32::from_be_bytes([orig[t + 8], orig[t + 9], orig[t + 10], orig[t + 11]]);
    let flags = p.tcp_flags;
    let seg = (p.total_len.saturating_sub(p.payload_offset) as u32) + u32::from(flags & TCP_SYN != 0) + u32::from(flags & TCP_FIN != 0);
    let (rst_seq, rst_ack, rst_flags) =
        if flags & TCP_ACK != 0 { (ack_in, 0, TCP_RST) } else { (0, seq.wrapping_add(seg), TCP_RST | TCP_ACK) };
    let ip = if p.version == 4 { 20 } else { 40 };
    out.resize(ip + 20, 0);
    if p.version == 4 {
        out[0] = 0x45;
        out[2..4].copy_from_slice(&((ip + 20) as u16).to_be_bytes());
        out[8] = 64;
        out[9] = PROTO_TCP;
        out[12..16].copy_from_slice(&orig[16..20]);
        out[16..20].copy_from_slice(&orig[12..16]);
        let c = checksum(&out[..20]);
        out[10..12].copy_from_slice(&c.to_be_bytes());
    } else {
        out[0] = 0x60;
        out[4..6].copy_from_slice(&20u16.to_be_bytes());
        out[6] = PROTO_TCP;
        out[7] = 64;
        out[8..24].copy_from_slice(&orig[24..40]);
        out[24..40].copy_from_slice(&orig[8..24]);
    }
    out[ip..ip + 2].copy_from_slice(&p.dst_port.to_be_bytes());
    out[ip + 2..ip + 4].copy_from_slice(&p.src_port.to_be_bytes());
    out[ip + 4..ip + 8].copy_from_slice(&rst_seq.to_be_bytes());
    out[ip + 8..ip + 12].copy_from_slice(&rst_ack.to_be_bytes());
    out[ip + 12] = 5 << 4;
    out[ip + 13] = rst_flags;
    let c = transport_checksum(out, ip, PROTO_TCP, p.version);
    out[ip + 16..ip + 18].copy_from_slice(&c.to_be_bytes());
}

pub fn tcp_splittable(p: &Parsed) -> bool {
    p.proto == PROTO_TCP
        && p.payload_offset < p.total_len
        && p.payload_offset <= TCP_PIECE_HDR
        && (p.version == 4 || p.ip_header_len == 40)
        && p.tcp_flags & (TCP_SYN | TCP_RST) == 0
}

pub const TCP_PIECE_HDR: usize = 120;

pub fn tcp_piece(pkt: &[u8], p: &Parsed, off: usize, n: usize, hdr: &mut [u8; TCP_PIECE_HDR]) -> usize {
    let (ip, hl) = (p.ip_header_len, p.payload_offset);
    let data = &pkt[hl + off..hl + off + n];
    let last = hl + off + n == p.total_len;
    hdr[..hl].copy_from_slice(&pkt[..hl]);
    let tcp_len = hl - ip + n;
    let mut c = Csum::default();
    if p.version == 4 {
        hdr[2..4].copy_from_slice(&((hl + n) as u16).to_be_bytes());
        hdr[10..12].copy_from_slice(&[0, 0]);
        let sum = checksum(&hdr[..ip]);
        hdr[10..12].copy_from_slice(&sum.to_be_bytes());
        c.add(&hdr[12..20]);
    } else {
        hdr[4..6].copy_from_slice(&(tcp_len as u16).to_be_bytes());
        c.add(&hdr[8..40]);
    }
    let t = &mut hdr[ip..hl];
    let seq = u32::from_be_bytes([t[4], t[5], t[6], t[7]]).wrapping_add(off as u32);
    t[4..8].copy_from_slice(&seq.to_be_bytes());
    if !last {
        t[13] &= !(TCP_FIN | 0x08);
    }
    t[16..18].copy_from_slice(&[0, 0]);
    c.add_u16(PROTO_TCP as u16);
    c.add_u16(tcp_len as u16);
    c.add(t);
    c.add(data);
    let sum = c.finish();
    hdr[ip + 16..ip + 18].copy_from_slice(&sum.to_be_bytes());
    hl
}

fn adjust_word(c: u16, old: u16, new: u16) -> u16 {
    let s = (!c as u32) + (!old as u32) + (new as u32);
    let s = (s & 0xffff) + (s >> 16);
    !(((s & 0xffff) + (s >> 16)) as u16)
}
fn adjust_bytes(mut c: u16, old: &[u8], new: &[u8]) -> u16 {
    for i in (0..old.len()).step_by(2) {
        c = adjust_word(
            c,
            u16::from_be_bytes([old[i], old.get(i + 1).copied().unwrap_or(0)]),
            u16::from_be_bytes([new[i], new.get(i + 1).copied().unwrap_or(0)]),
        );
    }
    c
}
fn checksum(data: &[u8]) -> u16 {
    let mut c = Csum::default();
    c.add(data);
    c.finish()
}
fn transport_checksum(pkt: &[u8], off: usize, proto: u8, version: u8) -> u16 {
    let len = pkt.len() - off;
    let mut c = Csum::default();
    if version == 4 {
        c.add(&pkt[12..20]);
    } else {
        c.add(&pkt[8..40]);
        c.add_u16((len >> 16) as u16);
    }
    c.add_u16(proto as u16);
    c.add_u16(len as u16);
    c.add(&pkt[off..]);
    c.finish()
}

#[cfg(kani)]
#[path = "proofs/packet.rs"]
mod proofs;

#[cfg(test)]
mod tests {

    #[test]
    fn udp_header_matches_build_udp() {
        for (a, b) in [("172.19.0.2:53", "172.19.0.1:40000"), ("[2001:db8::1]:443", "[fd00::2]:5000")] {
            let (a, b): (SocketAddr, SocketAddr) = (a.parse().unwrap(), b.parse().unwrap());
            for len in [0usize, 1, 7, 1200, 1351] {
                let payload: Vec<u8> = (0..len).map(|i| (i * 13 + 5) as u8).collect();
                let mut want = Vec::new();
                build_udp(a, b, &payload, &mut want);
                let mut hdr = [0u8; 48];
                let n = udp_header(a, b, &payload, &mut hdr);
                assert_eq!([&hdr[..n], &payload[..]].concat(), want, "{a} {len}");
            }
        }
    }

    use super::*;

    #[test]
    fn tcp_pieces_are_valid_segments() {
        for v6 in [false, true] {
            let data: Vec<u8> = (0..40_001u32).map(|i| (i * 7 + 3) as u8).collect();
            let (ip, tl) = (if v6 { 40 } else { 20 }, 20 + 12 + data.len());
            let mut pkt = vec![0u8; ip + tl];
            if v6 {
                pkt[0] = 0x60;
                pkt[4..6].copy_from_slice(&(tl as u16).to_be_bytes());
                pkt[6] = PROTO_TCP;
                pkt[7] = 64;
                pkt[8..24].copy_from_slice(&"fd00::1".parse::<std::net::Ipv6Addr>().unwrap().octets());
                pkt[24..40].copy_from_slice(&"fd00::2".parse::<std::net::Ipv6Addr>().unwrap().octets());
            } else {
                pkt[0] = 0x45;
                pkt[2..4].copy_from_slice(&((ip + tl) as u16).to_be_bytes());
                pkt[6] = 0x40;
                pkt[8] = 64;
                pkt[9] = PROTO_TCP;
                pkt[12..16].copy_from_slice(&[198, 18, 0, 1]);
                pkt[16..20].copy_from_slice(&[172, 19, 0, 1]);
            }
            let t = &mut pkt[ip..];
            t[0..2].copy_from_slice(&443u16.to_be_bytes());
            t[2..4].copy_from_slice(&40000u16.to_be_bytes());
            t[4..8].copy_from_slice(&0xffff_f000u32.to_be_bytes());
            t[12] = 8 << 4;
            t[13] = TCP_ACK | TCP_FIN | 0x08;
            t[20..32].copy_from_slice(&[1, 1, 8, 10, 0, 0, 0, 1, 0, 0, 0, 2]);
            t[32..].copy_from_slice(&data);
            let c = transport_checksum(&pkt, ip, PROTO_TCP, if v6 { 6 } else { 4 });
            pkt[ip + 16..ip + 18].copy_from_slice(&c.to_be_bytes());
            let p = parse(&pkt).unwrap();
            assert!(tcp_splittable(&p));
            let mut hdr = [0u8; TCP_PIECE_HDR];
            let (mut off, mut got, mut pieces) = (0, Vec::new(), 0);
            while off < data.len() {
                let n = 7000.min(data.len() - off);
                let h = tcp_piece(&pkt, &p, off, n, &mut hdr);
                let piece = [&hdr[..h], &pkt[p.payload_offset + off..p.payload_offset + off + n]].concat();
                let q = parse(&piece).expect("кусок разбирается");
                assert_eq!(q.total_len, piece.len());
                if !v6 {
                    assert_eq!(checksum(&piece[..20]), 0, "IPv4");
                }
                assert_eq!(transport_checksum(&piece, ip, PROTO_TCP, if v6 { 6 } else { 4 }), 0, "TCP");
                let seq = u32::from_be_bytes(piece[ip + 4..ip + 8].try_into().unwrap());
                assert_eq!(seq, 0xffff_f000u32.wrapping_add(off as u32));
                let last = off + n == data.len();
                assert_eq!(q.tcp_flags & (TCP_FIN | 0x08) != 0, last, "FIN и PSH у последнего");
                assert_eq!(q.tcp_flags & TCP_ACK, TCP_ACK);
                assert_eq!(&piece[ip + 20..ip + 32], &pkt[ip + 20..ip + 32], "опции те же");
                got.extend_from_slice(&piece[q.payload_offset..]);
                off += n;
                pieces += 1;
            }
            assert_eq!(pieces, 6);
            assert_eq!(got, data);
        }
    }
    fn addr(v6: bool, tail: u8, port: u16) -> SocketAddr {
        if v6 {
            SocketAddr::new(IpAddr::from([0x2001, 0xdb8, 0, 0, 0, 0, 0, tail as u16]), port)
        } else {
            SocketAddr::new(IpAddr::from([10, 0, 0, tail]), port)
        }
    }
    fn tcp_packet(v6: bool, payload: &[u8]) -> Vec<u8> {
        let ip = if v6 { 40 } else { 20 };
        let mut b = vec![0; ip + 20 + payload.len()];
        let packet_len = b.len();
        if v6 {
            b[0] = 0x60;
            b[4..6].copy_from_slice(&((packet_len - 40) as u16).to_be_bytes());
            b[6] = 6;
            b[7] = 64;
            b[8..24].copy_from_slice(&[0x20, 1, 0xdb, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            b[24..40].copy_from_slice(&[0x20, 1, 0xdb, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
        } else {
            b[0] = 0x45;
            b[2..4].copy_from_slice(&(packet_len as u16).to_be_bytes());
            b[8] = 64;
            b[9] = 6;
            b[12..16].copy_from_slice(&[10, 0, 0, 1]);
            b[16..20].copy_from_slice(&[10, 0, 0, 2]);
        }
        b[ip..ip + 2].copy_from_slice(&1234u16.to_be_bytes());
        b[ip + 2..ip + 4].copy_from_slice(&443u16.to_be_bytes());
        b[ip + 12] = 0x50;
        b[ip + 13] = TCP_SYN;
        b[ip + 20..].copy_from_slice(payload);
        let c = transport_checksum(&b, ip, 6, if v6 { 6 } else { 4 });
        b[ip + 16..ip + 18].copy_from_slice(&c.to_be_bytes());
        if !v6 {
            let c = checksum(&b[..20]);
            b[10..12].copy_from_slice(&c.to_be_bytes());
        }
        b
    }
    #[test]
    fn builders_parse_and_checksums() {
        for v6 in [false, true] {
            let mut b = Vec::new();
            build_udp(addr(v6, 1, 1234), addr(v6, 2, 53), b"payload", &mut b);
            let p = parse(&b).unwrap();
            assert_eq!(p.proto, PROTO_UDP);
            assert_eq!(p.src_port, 1234);
            assert_eq!(p.dst_port, 53);
            if !v6 {
                assert_eq!(checksum(&b[..20]), 0);
            }
            assert_eq!(transport_checksum(&b, if v6 { 40 } else { 20 }, 17, if v6 { 6 } else { 4 }), 0);
        }
    }
    #[test]
    fn parse_extensions_fragments_and_truncation() {
        let mut b = Vec::new();
        build_udp(addr(true, 1, 3), addr(true, 2, 4), b"x", &mut b);
        b[6] = 0;
        b.splice(40..40, [17, 0, 0, 0, 0, 0, 0, 0]);
        let n = (b.len() - 40) as u16;
        b[4..6].copy_from_slice(&n.to_be_bytes());
        assert_eq!(parse(&b).unwrap().ip_header_len, 48);
        let mut frag = b.clone();
        frag[6] = 44;
        assert!(parse(&frag).is_none());
        assert!(parse(&b[..b.len() - 1]).is_none());
        let mut v4 = Vec::new();
        build_udp(addr(false, 1, 1), addr(false, 2, 2), b"", &mut v4);
        v4[6] = 0x20;
        assert!(parse(&v4).is_none());
    }
    #[test]
    fn rewrite_preserves_valid_checksums() {
        for v6 in [false, true] {
            let mut b = Vec::new();
            build_udp(addr(v6, 1, 111), addr(v6, 2, 222), b"some odd payload!", &mut b);
            let p = parse(&b).unwrap();
            rewrite(&mut b, &p, addr(v6, 3, 333), addr(v6, 4, 444));
            let off = if v6 { 40 } else { 20 };
            if !v6 {
                assert_eq!(checksum(&b[..20]), 0)
            }
            assert_eq!(transport_checksum(&b, off, 17, if v6 { 6 } else { 4 }), 0);
        }
    }
    #[test]
    fn tcp_syn_and_randomized_rewrites() {
        for v6 in [false, true] {
            let mut b = tcp_packet(v6, b"syn payload");
            let p = parse(&b).unwrap();
            assert_eq!(p.tcp_flags, TCP_SYN);
            rewrite(&mut b, &p, addr(v6, 3, 333), addr(v6, 4, 444));
            let off = if v6 { 40 } else { 20 };
            assert_eq!(transport_checksum(&b, off, 6, if v6 { 6 } else { 4 }), 0);
            if !v6 {
                assert_eq!(checksum(&b[..20]), 0);
            }
            let mut state = 0x12345678u32;
            for _ in 0..5000 {
                let size = (state as usize) % 96;
                let mut data = vec![0; size];
                for x in &mut data {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    *x = state as u8;
                }
                let mut pkt = if state & 1 == 0 {
                    tcp_packet(v6, &data)
                } else {
                    let mut x = Vec::new();
                    build_udp(addr(v6, 1, 1000), addr(v6, 2, 2000), &data, &mut x);
                    x
                };
                let p = parse(&pkt).unwrap();
                rewrite(&mut pkt, &p, addr(v6, (state >> 8) as u8, state as u16), addr(v6, (state >> 16) as u8, (state >> 4) as u16));
                let off = if v6 { 40 } else { 20 };
                assert_eq!(transport_checksum(&pkt, off, p.proto, if v6 { 6 } else { 4 }), 0);
                if !v6 {
                    assert_eq!(checksum(&pkt[..20]), 0);
                }
            }
        }
    }
    #[test]
    fn unreachable_quotes_and_checksums() {
        for v6 in [false, true] {
            let mut orig = Vec::new();
            build_udp(addr(v6, 1, 1), addr(v6, 2, 2), &[7; 1400], &mut orig);
            let p = parse(&orig).unwrap();
            let mut reply = Vec::new();
            build_port_unreachable(&orig, &p, &mut reply);
            assert_eq!(reply.len(), if v6 { 1280 } else { 56 });
            if v6 {
                assert_eq!(transport_checksum(&reply, 40, 58, 6), 0)
            } else {
                assert_eq!(checksum(&reply[..20]), 0);
                assert_eq!(checksum(&reply[20..]), 0)
            }
        }
    }
    #[test]
    fn rst_answers_syn() {
        for v6 in [false, true] {
            let orig = tcp_packet(v6, b"");
            let p = parse(&orig).unwrap();
            let mut rst = Vec::new();
            build_tcp_rst(&orig, &p, &mut rst);
            let r = parse(&rst).unwrap();
            assert_eq!((r.src, r.dst), (p.dst, p.src));
            assert_eq!((r.src_port, r.dst_port), (443, 1234));
            assert_eq!(r.tcp_flags, TCP_RST | TCP_ACK);
            let off = if v6 { 40 } else { 20 };
            assert_eq!(&rst[off + 8..off + 12], &1u32.to_be_bytes());
            assert_eq!(transport_checksum(&rst, off, 6, if v6 { 6 } else { 4 }), 0);
            if !v6 {
                assert_eq!(checksum(&rst[..20]), 0);
            }
        }
    }
}
