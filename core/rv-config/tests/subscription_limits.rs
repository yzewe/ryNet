use rv_config::{VlessEncryption, parse_subscription};
use std::time::Instant;

#[test]
fn udp_hop_ports_are_not_amplified_by_repeated_ranges() {
    let mport = vec!["1-65535"; 64].join(",");
    let link = format!("hy2://pw@h.example:443?sni=h.example&mport={mport}#x");
    let ps = parse_subscription(&link).unwrap();
    let q = ps[0].main_outbound().unwrap().stream.finalmask.quic.as_ref().unwrap();
    assert!(q.udp_hop_ports.len() <= 65536, "{} hop ports from a {}-byte link", q.udp_hop_ports.len(), link.len());
}

fn json_profiles(names: impl Iterator<Item = String>) -> String {
    let items: Vec<String> = names.map(|r| serde_json::json!({"remarks": r, "outbounds": [{"protocol": "freedom"}]}).to_string()).collect();
    format!("[{}]", items.join(","))
}

#[test]
fn repeated_remarks_are_numbered_in_linear_time() {
    let t = Instant::now();
    let ps = parse_subscription(&json_profiles((0..20_000).map(|_| "x".to_string()))).unwrap();
    assert!(t.elapsed().as_secs() < 5, "20000 equal remarks took {:?}", t.elapsed());
    assert_eq!(ps[1].remarks, "x (2)");
    assert_eq!(ps[19_999].remarks, "x (20000)");
}

#[test]
fn remarks_differing_in_leading_spaces_share_one_counter() {
    let names = (0..2000).flat_map(|i| {
        let name = format!("{}x", " ".repeat(i));
        [name.clone(), name]
    });
    let t = Instant::now();
    let ps = parse_subscription(&json_profiles(names)).unwrap();
    assert!(t.elapsed().as_secs() < 5, "2000 space-padded remark pairs took {:?}", t.elapsed());
    assert_eq!(ps[1].remarks, "x (2)");
    assert_eq!(ps[3].remarks, "x (3)");
    assert_eq!(ps[3999].remarks, "x (2001)");
}

#[test]
fn vless_encryption_padding_total_does_not_wrap_on_32_bit() {
    let key = "A".repeat(43);
    let s = format!("mlkem768x25519plus.native.0rtt.100-35-4294967295.0-0-0.0-36-36.{key}");
    let err = VlessEncryption::parse(&s).unwrap_err();
    assert!(err.contains("65553"), "{err}");
}
