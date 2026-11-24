use super::*;
use std::net::{Ipv4Addr, Ipv6Addr};

fn any_len(max: usize) -> usize {
    let len: usize = kani::any();
    kani::assume(len <= max);
    len
}

fn any_addr(v6: bool) -> SocketAddr {
    if v6 {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::from(kani::any::<[u8; 16]>())), kani::any())
    } else {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(kani::any::<[u8; 4]>())), kani::any())
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn parse_any_input() {
    let buf: [u8; 96] = kani::any();
    let pkt = &buf[..any_len(96)];
    if let Some(p) = parse(pkt) {
        assert!(p.version == 4 || p.version == 6);
        assert!(p.ip_header_len >= if p.version == 4 { 20 } else { 40 });
        assert!(p.ip_header_len <= p.payload_offset);
        assert!(p.payload_offset <= p.total_len);
        assert!(p.total_len <= pkt.len());
        if p.proto == PROTO_TCP {
            assert!(p.payload_offset >= p.ip_header_len + 20);
        }
        if p.proto == PROTO_UDP {
            assert!(p.payload_offset == p.ip_header_len + 8);
        }
        assert_eq!(p.src.is_ipv4(), p.version == 4);
        assert_eq!(p.dst.is_ipv4(), p.version == 4);
    }
}

fn rewrite_keeps_checksums<const M: usize>(v6: bool, proto: u8) {
    let mut buf: [u8; M] = kani::any();
    kani::assume(buf[0] >> 4 == if v6 { 6 } else { 4 });
    if v6 {
        kani::assume(buf[6] == proto);
    } else {
        kani::assume(buf[9] == proto);
    }
    let len = any_len(M);
    let Some(p) = parse(&buf[..len]) else { return };
    let off = p.ip_header_len;
    let total = p.total_len;
    let ver = p.version;
    let co = if proto == PROTO_TCP { off + 16 } else { off + 6 };
    let no_sum = proto == PROTO_UDP && !v6 && buf[co] == 0 && buf[co + 1] == 0;
    kani::assume(no_sum || transport_checksum(&buf[..total], off, proto, ver) == 0);
    if !v6 {
        kani::assume(checksum(&buf[..off]) == 0);
    }
    let (src, dst) = (any_addr(v6), any_addr(v6));
    rewrite(&mut buf[..len], &p, src, dst);
    if no_sum {
        assert!(buf[co] == 0 && buf[co + 1] == 0);
    } else {
        assert_eq!(transport_checksum(&buf[..total], off, proto, ver), 0);
    }
    if !v6 {
        assert_eq!(checksum(&buf[..off]), 0);
    }
    let q = parse(&buf[..len]).unwrap();
    assert_eq!((q.src, q.src_port, q.dst, q.dst_port), (src.ip(), src.port(), dst.ip(), dst.port()));
}

#[kani::proof]
#[kani::unwind(10)]
fn rewrite_v4_tcp() {
    rewrite_keeps_checksums::<48>(false, PROTO_TCP);
}

#[kani::proof]
#[kani::unwind(10)]
fn rewrite_v4_udp() {
    rewrite_keeps_checksums::<36>(false, PROTO_UDP);
}

#[kani::proof]
#[kani::unwind(10)]
fn rewrite_v6_tcp() {
    rewrite_keeps_checksums::<68>(true, PROTO_TCP);
}

#[kani::proof]
#[kani::unwind(10)]
fn rewrite_v6_udp() {
    rewrite_keeps_checksums::<56>(true, PROTO_UDP);
}

fn rst_reply<const M: usize>(v6: bool) {
    let buf: [u8; M] = kani::any();
    kani::assume(buf[0] >> 4 == if v6 { 6 } else { 4 });
    let pkt = &buf[..any_len(M)];
    let Some(p) = parse(pkt) else { return };
    let mut out = Vec::new();
    build_tcp_rst(&pkt[..p.total_len], &p, &mut out);
    if p.proto != PROTO_TCP {
        assert!(out.is_empty());
        return;
    }
    let r = parse(&out).unwrap();
    assert_eq!(r.total_len, out.len());
    assert_eq!((r.src, r.dst, r.src_port, r.dst_port), (p.dst, p.src, p.dst_port, p.src_port));
    assert!(r.tcp_flags & TCP_RST != 0);
    assert_eq!(transport_checksum(&out, r.ip_header_len, PROTO_TCP, r.version), 0);
    if !v6 {
        assert_eq!(checksum(&out[..20]), 0);
    }
}

#[kani::proof]
#[kani::unwind(64)]
fn tcp_rst_reply_v4() {
    rst_reply::<48>(false);
}

#[kani::proof]
#[kani::unwind(64)]
fn tcp_rst_reply_v6() {
    rst_reply::<64>(true);
}

fn unreachable_reply<const M: usize>(v6: bool) {
    let buf: [u8; M] = kani::any();
    kani::assume(buf[0] >> 4 == if v6 { 6 } else { 4 });
    let pkt = &buf[..any_len(M)];
    let Some(p) = parse(pkt) else { return };
    let mut out = Vec::new();
    build_port_unreachable(&pkt[..p.total_len], &p, &mut out);
    assert!(out.len() <= 1280);
    let r = parse(&out).unwrap();
    assert_eq!(r.total_len, out.len());
    assert_eq!((r.src, r.dst), (p.dst, p.src));
    if v6 {
        assert_eq!(transport_checksum(&out, 40, PROTO_ICMPV6, 6), 0);
    } else {
        assert_eq!(checksum(&out[..20]), 0);
        assert_eq!(checksum(&out[20..]), 0);
    }
}

#[kani::proof]
#[kani::unwind(80)]
fn port_unreachable_v4() {
    unreachable_reply::<44>(false);
}

#[kani::proof]
#[kani::unwind(100)]
fn port_unreachable_v6() {
    unreachable_reply::<52>(true);
}

fn tun_addrs(v6: bool) -> (SocketAddr, SocketAddr) {
    if v6 {
        (
            SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 2)), kani::any()),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)), kani::any()),
        )
    } else {
        (
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), kani::any()),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 19, 0, 1)), kani::any()),
        )
    }
}

fn udp_header_equals<const V6: bool>() {
    let (src, dst) = tun_addrs(V6);
    let data: [u8; 6] = kani::any();
    let payload = &data[..any_len(6)];
    let mut hdr = [0u8; 48];
    let n = udp_header(src, dst, payload, &mut hdr);
    let mut want = Vec::new();
    build_udp(src, dst, payload, &mut want);
    assert_eq!(n, if V6 { 48 } else { 28 });
    let ip = n - 8;
    assert_eq!(&hdr[..ip + 6], &want[..ip + 6]);
    assert_eq!(&want[n..], payload);
    let p = parse(&want).unwrap();
    assert_eq!((p.src, p.src_port, p.dst, p.dst_port), (src.ip(), src.port(), dst.ip(), dst.port()));
}

#[kani::proof]
#[kani::unwind(60)]
fn udp_header_v4() {
    udp_header_equals::<false>();
}

#[kani::proof]
#[kani::unwind(60)]
fn udp_header_v6() {
    udp_header_equals::<true>();
}

fn udp_header_checksum<const V6: bool>() {
    let (mut src, mut dst) = tun_addrs(V6);
    src.set_port(53);
    dst.set_port(40000);
    let data: [u8; 3] = kani::any();
    let payload = &data[..any_len(3)];
    let mut hdr = [0u8; 48];
    let n = udp_header(src, dst, payload, &mut hdr);
    let mut pkt = [0u8; 51];
    pkt[..n].copy_from_slice(&hdr[..n]);
    pkt[n..n + payload.len()].copy_from_slice(payload);
    let pkt = &pkt[..n + payload.len()];
    assert_eq!(transport_checksum(pkt, n - 8, PROTO_UDP, if V6 { 6 } else { 4 }), 0);
    if !V6 {
        assert_eq!(checksum(&pkt[..20]), 0);
    }
}

#[kani::proof]
#[kani::unwind(60)]
fn udp_checksum_v4() {
    udp_header_checksum::<false>();
}

#[kani::proof]
#[kani::unwind(60)]
fn udp_checksum_v6() {
    udp_header_checksum::<true>();
}

#[kani::proof]
fn adjust_word_is_rfc1624() {
    let (c, old, new, rest): (u16, u16, u16, u16) = kani::any();
    let add = |a: u16, b: u16| {
        let s = a as u32 + b as u32;
        ((s & 0xffff) + (s >> 16)) as u16
    };
    kani::assume(rest != 0);
    kani::assume(add(add(rest, old), c) == 0xffff);
    let c2 = adjust_word(c, old, new);
    assert_eq!(add(add(rest, new), c2), 0xffff);
}
