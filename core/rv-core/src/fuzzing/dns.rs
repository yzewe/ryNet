use super::*;

pub fn message(data: &[u8]) {
    if let Some(q) = question(data) {
        assert!(q.end >= 17 && q.end <= data.len());
        assert_eq!(q.qtype, u16::from_be_bytes([data[q.end - 4], data[q.end - 3]]));
        if fake::valid(&q.name) {
            assert!(!q.name.bytes().any(|c| c.is_ascii_control() || c == b' '));
        }
        let ips: Vec<IpAddr> = data[q.end..].chunks_exact(4).take(3).map(|c| IpAddr::from([c[0], c[1], c[2], c[3]])).collect();
        let r = answer(data, &q, &ips, 60);
        assert_eq!(question(&r).map(|x| (x.name, x.qtype)), Some((q.name.clone(), q.qtype)));
        assert_eq!(addresses(&r), ips);
        let e = rcode(data, &q, data[2]);
        assert_eq!(e[3] & 0x0f, data[2] & 0x0f);
        assert!(addresses(&e).is_empty());
    }
    let _ = answer_ttl(data);
    for ip in addresses(data) {
        let found = match ip {
            IpAddr::V4(a) => data.windows(4).any(|w| w == a.octets()),
            IpAddr::V6(a) => data.windows(16).any(|w| w == a.octets()),
        };
        assert!(found);
    }
    let out = pad_query(data, data.len() % 9);
    if out != data {
        assert_eq!(question(&out), question(data));
    }
}
