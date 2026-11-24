use rv_config::{Network, OutboundKind, Security, parse_subscription};
use serde_json::json;
const ID: &str = "9205c503-358f-4d8d-a081-20c98eddf64a";

#[test]
fn vmess_uses_xray_security_fallback_and_ignores_removed_alter_id() {
    for security in ["", "none", "zero", "aes-128-cfb", "unknown", "AUTO", "AES-128-GCM", "chacha20-poly1305"] {
        let p = parse_subscription(&json!({"outbounds": [{"protocol": "vmess", "settings": {"address": "test.local", "port": 443, "id": ID, "security": security, "alterId": 16}}]}).to_string()).unwrap().remove(0);
        let OutboundKind::Vmess(v) = &p.outbounds[0].kind else { panic!("{security}: {:?}", p.outbounds[0].kind) };
        assert_eq!(
            v.security,
            match security {
                "AES-128-GCM" => "aes-128-gcm",
                "chacha20-poly1305" => security,
                _ => "auto",
            }
        );
    }
}

#[test]
fn go_json_field_case_and_string_list_are_tolerated() {
    let p = parse_subscription(&json!({"OUTBOUNDS": [{"TAG": "proxy", "PROTOCOL": "VLESS", "SETTINGS": {"Address": "test.local", "Port": 443, "ID": ID, "Encryption": "none"},
        "StreamSettings": {"Network": "WS", "Security": "TLS", "TLSSettings": {"ServerName": "test.local", "ALPN": "h2"}, "WSSettings": {"Path": "/p", "Headers": {"HOST": "Test.LOCAL", "x-Custom": "Value"}}}}]}).to_string()).unwrap().remove(0);
    assert!(matches!(p.outbounds[0].kind, OutboundKind::Vless(_)), "{:?}", p.outbounds);
    let Security::Tls(t) = &p.outbounds[0].stream.security else { panic!() };
    assert_eq!(t.alpn, ["h2"]);
    let Network::Ws(w) = &p.outbounds[0].stream.network else { panic!() };
    assert_eq!(w.host, "Test.LOCAL");
    assert_eq!(w.headers, [("x-Custom".into(), "Value".into())]);
}
