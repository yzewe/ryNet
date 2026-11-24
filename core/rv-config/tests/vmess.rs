use base64::Engine;
use rv_config::{Network, OutboundKind, Security, parse_subscription};
use serde_json::{Value, json};

const ID: &str = "9205c503-358f-4d8d-a081-20c98eddf64a";

fn link(v: Value) -> String {
    format!("vmess://{}", base64::engine::general_purpose::STANDARD.encode(v.to_string()))
}

#[test]
fn imports_v2rayn_websocket_tls_and_base64_subscription() {
    for port in [json!(8443), json!("8443")] {
        let body = link(json!({"v": "2", "ps": "VMess 🇷🇺", "add": "edge.example", "port": port,
            "id": ID, "aid": "0", "scy": " AES-128-GCM ", "net": "ws", "host": "cdn.example", "path": "/ws?ed=2048",
            "tls": "tls", "sni": "cdn.example", "alpn": "http/1.1,h2", "fp": "chrome", "allowInsecure": true}));
        for body in [body.clone(), base64::engine::general_purpose::STANDARD.encode(body)] {
            let p = parse_subscription(&body).unwrap().remove(0);
            assert_eq!(p.remarks, "VMess 🇷🇺");
            let ob = p.main_outbound().unwrap();
            let OutboundKind::Vmess(v) = &ob.kind else { panic!("{:?}", ob.kind) };
            assert_eq!((v.address.as_str(), v.port, v.security.as_str()), ("edge.example", 8443, "aes-128-gcm"));
            assert_eq!(v.id, [0x92, 5, 0xc5, 3, 0x35, 0x8f, 0x4d, 0x8d, 0xa0, 0x81, 0x20, 0xc9, 0x8e, 0xdd, 0xf6, 0x4a]);
            assert_eq!(ob.kind.server(), Some(("edge.example", 8443)));
            assert!(ob.kind.is_proxy());
            let Network::Ws(ws) = &ob.stream.network else { panic!("websocket") };
            assert_eq!(ws.host, "cdn.example");
            assert_eq!(ws.early_data, 2048);
            let Security::Tls(tls) = &ob.stream.security else { panic!("tls") };
            assert_eq!(tls.server_name, "cdn.example");
            assert!(tls.allow_insecure);
            assert_eq!(tls.alpn, ["http/1.1", "h2"]);
        }
    }
}

#[test]
fn imports_grpc_xhttp_url_and_singbox() {
    let p = parse_subscription(&link(json!({"add":"g.example","port":443,"id":ID,"net":"grpc",
        "path":"service/name","host":"authority.example","type":"multi"})))
    .unwrap()
    .remove(0);
    let Network::Grpc(grpc) = &p.main_outbound().unwrap().stream.network else { panic!("grpc") };
    assert_eq!(grpc.service_name, "service/name");
    assert_eq!(grpc.authority, "authority.example");
    assert!(grpc.multi_mode);

    let p = parse_subscription(&link(json!({"add":"x.example","port":443,"id":ID,"net":"xhttp",
        "path":"/x","type":"packet-up","extra":{"xmux":{"maxConcurrency":"2-4"}}})))
    .unwrap()
    .remove(0);
    let Network::Xhttp(x) = &p.main_outbound().unwrap().stream.network else { panic!("xhttp") };
    assert_eq!(x["mode"], "packet-up");
    assert_eq!(x["extra"]["xmux"]["maxConcurrency"], "2-4");

    let p = parse_subscription(&format!("vmess://{ID}@[2001:db8::1]:8443?encryption=chacha20-poly1305&type=tcp&security=none#url"))
        .unwrap()
        .remove(0);
    let OutboundKind::Vmess(v) = &p.main_outbound().unwrap().kind else { panic!("VMess URL") };
    assert_eq!((v.address.as_str(), v.port, v.security.as_str()), ("2001:db8::1", 8443, "chacha20-poly1305"));

    let p = parse_subscription(
        &json!({"outbounds":[{"type":"vmess","tag":"proxy","server":"s.example",
        "server_port":443,"uuid":ID,"security":"auto","alter_id":0,"tls":{"enabled":true,"server_name":"s.example"},
        "transport":{"type":"ws","path":"/ws"}}]})
        .to_string(),
    )
    .unwrap()
    .remove(0);
    assert!(matches!(p.main_outbound().unwrap().kind, OutboundKind::Vmess(_)));
    assert!(matches!(p.main_outbound().unwrap().stream.network, Network::Ws(_)));
    assert!(matches!(p.main_outbound().unwrap().stream.security, Security::Tls(_)));
}

#[test]
fn imports_nested_and_flat_xray_and_rejects_invalid_settings() {
    for security in ["auto", "aes-128-gcm", "chacha20-poly1305", "none", "zero"] {
        for nested in [false, true] {
            let settings = if nested {
                json!({"vnext":[{"address":"edge.example","port":443,"users":[{"id":ID,"security":security,"alterId":0}]}]})
            } else {
                json!({"address":"edge.example","port":443,"id":ID,"security":security,"alterId":"0"})
            };
            let p = parse_subscription(&json!({"outbounds":[{"protocol":"vmess","settings":settings}]}).to_string()).unwrap().remove(0);
            let OutboundKind::Vmess(v) = &p.main_outbound().unwrap().kind else { panic!("{:?}", p.outbounds) };
            assert_eq!(v.security, if matches!(security, "none" | "zero") { "auto" } else { security });
        }
    }
    for settings in [
        json!({"address":"","port":443,"id":ID}),
        json!({"address":"edge.example","port":0,"id":ID}),
        json!({"address":"edge.example","port":443,"id":""}),
    ] {
        let p = parse_subscription(&json!({"outbounds":[{"protocol":"vmess","settings":settings}]}).to_string()).unwrap().remove(0);
        assert!(matches!(p.outbounds[0].kind, OutboundKind::Unsupported { .. }), "{:?}", p.outbounds);
    }
    let p = parse_subscription(&link(json!({"add":"edge.example","port":443,"id":ID,"aid":"16"}))).unwrap().remove(0);
    assert!(matches!(p.outbounds[0].kind, OutboundKind::Vmess(_)), "{:?}", p.outbounds);
    let p = parse_subscription(
        &json!({"outbounds":[{"protocol":"vmess","settings":{"address":"edge.example","port":443,"id":ID,"alterId":1}}]}).to_string(),
    )
    .unwrap()
    .remove(0);
    assert!(matches!(p.outbounds[0].kind, OutboundKind::Vmess(_)), "{:?}", p.outbounds);
}

#[test]
fn v2rayn_insecure_flag_disables_certificate_check() {
    for key in ["insecure", "allowInsecure"] {
        let p = parse_subscription(&link(
            json!({"add":"edge.example","port":443,"id":ID,"net":"tcp","tls":"tls","sni":"edge.example",key:"1"}),
        ))
        .unwrap()
        .remove(0);
        let Security::Tls(t) = &p.main_outbound().unwrap().stream.security else { panic!("{:?}", p.outbounds) };
        assert!(t.allow_insecure, "{key}");
    }
}
