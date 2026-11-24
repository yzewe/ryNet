#![no_main]

use libfuzzer_sys::fuzz_target;
use rv_tun::packet::{PROTO_TCP, PROTO_UDP, build_port_unreachable, build_tcp_rst, parse, rewrite};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

fuzz_target!(|data: &[u8]| {
    let Some(p) = parse(data) else { return };
    assert!(p.ip_header_len <= p.payload_offset && p.payload_offset <= p.total_len && p.total_len <= data.len());
    let pkt = &data[..p.total_len];
    let mut out = Vec::new();
    build_tcp_rst(pkt, &p, &mut out);
    if p.proto == PROTO_TCP {
        let r = parse(&out).expect("RST разбирается");
        assert_eq!((r.total_len, r.src, r.dst), (out.len(), p.dst, p.src));
    }
    build_port_unreachable(pkt, &p, &mut out);
    if !out.is_empty() {
        assert!(out.len() <= 1280);
        let r = parse(&out).expect("ICMP разбирается");
        assert_eq!((r.total_len, r.src, r.dst), (out.len(), p.dst, p.src));
    }
    if p.proto != PROTO_TCP && p.proto != PROTO_UDP {
        return;
    }
    let k = data.len() as u16;
    let (src, dst) = if p.version == 4 {
        (SocketAddr::new(IpAddr::V4(Ipv4Addr::from(k as u32 * 7919)), k), SocketAddr::new(IpAddr::V4(Ipv4Addr::from(!(k as u32))), !k))
    } else {
        (SocketAddr::new(IpAddr::V6(Ipv6Addr::from(k as u128 * 7919)), k), SocketAddr::new(IpAddr::V6(Ipv6Addr::from(!(k as u128))), !k))
    };
    let mut buf = data.to_vec();
    rewrite(&mut buf, &p, src, dst);
    let q = parse(&buf).expect("после NAT пакет разбирается");
    assert_eq!((q.src, q.src_port, q.dst, q.dst_port), (src.ip(), src.port(), dst.ip(), dst.port()));
});
