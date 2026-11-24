use super::*;

#[test]
fn reality_mldsa65_verify_is_decoded_and_validated() {
    use base64::Engine;
    let mut stream = serde_json::json!({"security":"reality", "realitySettings": {
        "publicKey": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7;32]),
        "mldsa65Verify": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([42;1952])
    }});
    let parse = |s: serde_json::Value| convert_stream(serde_json::from_value(s).unwrap(), None);
    let Security::Reality(r) = parse(stream.clone()).unwrap().security else { panic!() };
    assert_eq!(r.mldsa65_verify, vec![42; 1952]);
    stream["realitySettings"]["mldsa65Verify"] = serde_json::json!(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([42; 1951]));
    assert!(parse(stream.clone()).unwrap_err().contains("mldsa65Verify"));
    stream["realitySettings"]["mldsa65Verify"] = serde_json::json!("bad+base64");
    assert!(parse(stream.clone()).unwrap_err().contains("mldsa65Verify"));
    stream["realitySettings"]["mldsa65Verify"] = serde_json::json!("");
    let Security::Reality(r) = parse(stream).unwrap().security else { panic!() };
    assert!(r.mldsa65_verify.is_empty());
}

#[test]
fn hysteria_hop_ranges_and_intervals_match_xray() {
    use serde_json::json;
    let parse = |h| convert_finalmask(serde_json::from_value(json!({"quicParams":{"udpHop":h}})).unwrap(), false);
    for interval in [None, Some(json!(0)), Some(json!("")), Some(json!("0-0"))] {
        let mut h = json!({"ports":"20000-30000,443"});
        if let Some(i) = interval {
            h["interval"] = i;
        }
        let q = parse(h).unwrap().quic.unwrap();
        assert_eq!(q.udp_hop_ports, (20000..=30000).chain([443]).collect::<Vec<_>>());
        assert_eq!(q.udp_hop_interval, (30, 30));
    }
    for (value, expected) in [(json!("10-5"), (5, 10)), (json!(5), (5, 5)), (json!("0-40"), (30, 40))] {
        assert_eq!(parse(json!({"ports":"443", "interval":value})).unwrap().quic.unwrap().udp_hop_interval, expected);
    }
    for interval in [
        json!(1),
        json!("4-10"),
        json!(-1),
        json!("-40"),
        json!("5-"),
        json!("-"),
        json!("bad"),
        json!(u32::MAX),
        json!("0-10"),
        json!("5-0"),
    ] {
        assert!(parse(json!({"ports":"443", "interval":interval})).is_err());
    }
    assert_eq!(parse(json!({"ports":"30000-20000,443", "interval":5})).unwrap().quic.unwrap().udp_hop_ports, vec![443]);
    for ports in [json!("bad"), json!("70000"), json!("20000-bad"), json!(70000)] {
        assert!(parse(json!({"ports":ports})).is_err());
    }
}

const FIXTURE: &str = include_str!("../tests/fixtures/profiles.json");

#[test]
fn parses_fixture_profiles() {
    let profiles = parse_subscription(FIXTURE).unwrap();
    assert_eq!(profiles.len(), 3);

    let fi = &profiles[0];
    assert_eq!(fi.remarks, "🇫🇮 Финляндия");
    let proxy = fi.main_outbound().unwrap();
    let OutboundKind::Vless(v) = &proxy.kind else { panic!() };
    assert_eq!(v.flow, Flow::Vision);
    assert_eq!(v.port, 443);
    assert_eq!(v.id, *uuid::Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap().as_bytes());
    let Security::Reality(r) = &proxy.stream.security else { panic!() };
    assert_eq!(r.fingerprint, "firefox");
    assert_eq!(&r.short_id, &[0xab, 0xcd, 0, 0, 0, 0, 0, 0]);
    assert!(matches!(proxy.stream.network, Network::Tcp));
    let he = proxy.stream.sockopt.happy_eyeballs.unwrap();
    assert_eq!(he.try_delay_ms, 250);
    assert_eq!(he.max_concurrent_try, 4);
    assert_eq!(proxy.stream.sockopt.tcp_max_seg, Some(1440));

    let wl = &profiles[1];
    let proxy = wl.main_outbound().unwrap();
    assert!(matches!(proxy.stream.network, Network::Xhttp(_)));
    assert_eq!(proxy.stream.sockopt.dialer_proxy.as_deref(), Some("fragment"));
    let OutboundKind::Freedom(f) = &wl.outbound("fragment").unwrap().kind else { panic!() };
    let frag = f.fragment.as_ref().unwrap();
    assert_eq!(frag.packets, (0, 1));
    assert_eq!(frag.length, Range { from: 50, to: 100 });
    assert_eq!(frag.interval_ms, Range { from: 10, to: 20 });

    let hy = &profiles[2];
    let OutboundKind::Hysteria2(h) = &hy.main_outbound().unwrap().kind else { panic!() };
    assert_eq!(h.auth, "secret-auth");
}

#[test]
fn range_parse() {
    use serde_json::json;
    assert_eq!(Range::parse(&json!("3-6")), Some(Range { from: 3, to: 6 }));
    assert_eq!(Range::parse(&json!(7)), Some(Range { from: 7, to: 7 }));
    assert_eq!(Range::parse(&json!("9")), Some(Range { from: 9, to: 9 }));
    assert_eq!(Range::parse(&json!("6-3")), None);
}

#[test]
fn non_uuid_id_is_uuidv5() {
    let id = parse_uuid("hello").unwrap();
    assert_eq!(uuid::Uuid::from_bytes(id).get_version_num(), 5);
}

#[test]
fn subscription_headers() {
    let mut h = HashMap::new();
    h.insert("profile-title".into(), "base64:eXpld2Ug".into());
    h.insert("subscription-userinfo".into(), "upload=0; download=463804511855; total=0; expire=0".into());
    h.insert("profile-update-interval".into(), "36".into());
    let i = SubscriptionInfo::from_headers(&h);
    assert_eq!(i.title.as_deref(), Some("yzewe "));
    assert_eq!(i.download, Some(463804511855));
    assert_eq!(i.update_interval_hours, Some(36));
}

#[test]
#[ignore]
fn parses_real_subscription() {
    let path = std::env::var("RV_SUB_FILE").expect("RV_SUB_FILE");
    let profiles = parse_subscription(&std::fs::read_to_string(path).unwrap()).unwrap();
    for p in &profiles {
        let main = p.main_outbound();
        println!(
            "{:<28} {:?}",
            p.remarks,
            main.map(|o| (&o.kind, &o.stream.network)).map(|(k, n)| format!("{} / {}", kind_name(k), net_name(n)))
        );
        for o in &p.outbounds {
            assert!(!matches!(o.kind, OutboundKind::Unsupported { .. }), "{}: {:?}", p.remarks, o.kind);
        }
    }
}

fn kind_name(k: &OutboundKind) -> &'static str {
    match k {
        OutboundKind::Vless(v) if v.flow != Flow::None => "vless+vision",
        OutboundKind::Vless(_) => "vless",
        OutboundKind::Hysteria2(_) => "hysteria2",
        _ => "other",
    }
}
fn net_name(n: &Network) -> &str {
    n.name()
}

#[test]
fn transports_finalmask_and_tolerance() {
    let json = r#"[
      {"remarks": "ws", "meta": {"serverDescription": "классика"}, "outbounds": [{"tag": "proxy", "protocol": "vless",
        "settings": {"vnext": [{"address": "1.2.3.4", "port": 443, "users": [{"id": "8b775467-5bc8-4344-a79e-90d795f324de"}]}]},
        "streamSettings": {"network": "ws", "wsSettings": {"path": "/kto?ed=2048&b=2", "headers": {"Host": "ws.example"}}}}]},
      {"remarks": "hy", "outbounds": [{"tag": "proxy", "protocol": "hysteria", "settings": {"address": "h.example", "port": 443, "version": 2},
        "streamSettings": {"network": "hysteria", "hysteriaSettings": {"version": 2, "auth": "a"}, "security": "tls",
          "tlsSettings": {"serverName": "h.example", "alpn": ["h3"]},
          "finalmask": {"udp": [{"type": "salamander", "settings": {"password": "pw"}}],
                        "quicParams": {"congestion": "brutal", "brutalUp": "100 mbps", "keepAlivePeriod": 15, "udpHop": {"ports": "20000-20002,443", "interval": "5-10"}}}}}]},
      {"remarks": "broken", "outbounds": [{"tag": "proxy", "protocol": "vless", "settings": {"vnext": []}}]},
      {"remarks": "vmess", "outbounds": [{"tag": "proxy", "protocol": "vmess"}]}
    ]"#;
    let ps = parse_subscription(json).unwrap();
    assert_eq!(ps.len(), 4);
    assert_eq!(ps[0].description.as_deref(), Some("классика"));
    let Network::Ws(w) = &ps[0].main_outbound().unwrap().stream.network else { panic!() };
    assert_eq!((w.path.as_str(), w.early_data, w.host.as_str()), ("/kto?b=2", 2048, "ws.example"));
    assert!(w.headers.is_empty());
    let hy = ps[1].main_outbound().unwrap();
    assert_eq!(hy.stream.finalmask.udp, vec![UdpMask::Salamander { password: "pw".into(), packet_size: None }]);
    let q = hy.stream.finalmask.quic.as_ref().unwrap();
    assert_eq!(q.brutal_up, 100 * 1024 * 1024 / 8);
    assert_eq!(q.keep_alive_period, 15);
    assert_eq!(q.udp_hop_ports, vec![20000, 20001, 20002, 443]);
    assert_eq!(q.udp_hop_interval, (5, 10));
    assert_eq!(q.max_idle_timeout, 30);
    assert!(matches!(ps[2].main_outbound().unwrap().kind, OutboundKind::Unsupported { .. }));
    let OutboundKind::Unsupported { protocol, .. } = &ps[3].main_outbound().unwrap().kind else { panic!() };
    assert_eq!(protocol, "vmess");
}

#[test]
fn link_subscription() {
    let ps = parse_subscription("vless://8b775467-5bc8-4344-a79e-90d795f324de@1.2.3.4:20002?type=grpc&serviceName=svc&security=reality&pbk=yIxa_jMh3dynXe8LxIWgMlDKvICqcC7gJfvBr2UH8Eo&sid=ab&fp=qq&sni=a.example#G").unwrap();
    let ob = ps[0].main_outbound().unwrap();
    let Network::Grpc(g) = &ob.stream.network else { panic!() };
    assert_eq!(g.service_name, "svc");
    let Security::Reality(r) = &ob.stream.security else { panic!() };
    assert_eq!(r.fingerprint, "qq");
}

#[test]
fn pinned_peer_cert() {
    let hash = "1F:2E:3D:4C:5B:6A:79:88:1F:2E:3D:4C:5B:6A:79:88:1F:2E:3D:4C:5B:6A:79:88:1F:2E:3D:4C:5B:6A:79:88";
    let cfg = serde_json::json!({
        "remarks": "p",
        "outbounds": [{
            "tag": "proxy", "protocol": "vless",
            "settings": { "vnext": [{ "address": "1.2.3.4", "port": 443, "users": [{ "id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none" }] }] },
            "streamSettings": { "network": "tcp", "security": "tls", "tlsSettings": {
                "serverName": "lk.x5.ru", "pinnedPeerCertSha256": format!("{hash}, "), "verifyPeerCertByName": "lk.x5.ru,x5.ru"
            } }
        }]
    });
    let p = &parse_subscription(&cfg.to_string()).unwrap()[0];
    let Security::Tls(t) = &p.main_outbound().unwrap().stream.security else { panic!("tls") };
    assert_eq!(t.pinned_sha256.len(), 1);
    assert_eq!(t.pinned_sha256[0][..2], [0x1f, 0x2e]);
    assert_eq!(t.verify_names, ["lk.x5.ru", "x5.ru"]);
}

#[test]
fn hy2_link_ports_and_obfs() {
    let ps =
        parse_subscription("hysteria2://pw@hy.example:443,20000-20002/?sni=a.example#A\nhysteria2://pw@hy.example:443/?obfs=gibberish#B\n")
            .unwrap();
    let a = ps[0].main_outbound().unwrap();
    assert_eq!(a.kind.server(), Some(("hy.example", 443)));
    let q = a.stream.finalmask.quic.as_ref().unwrap();
    assert_eq!(q.udp_hop_ports, vec![443, 20000, 20001, 20002]);
    assert_eq!(ps[1].main_outbound().unwrap().stream.finalmask.udp, vec![UdpMask::Other("gibberish".into())]);
}

#[test]
fn flat_outbound_settings() {
    let ps = parse_subscription(r#"[{"remarks": "f", "outbounds": [
        {"tag": "proxy", "protocol": "vless", "settings": {"address": "a.example", "port": 443, "id": "9205c503-358f-4d8d-a081-20c98eddf64a", "flow": "xtls-rprx-vision", "encryption": "none"},
         "streamSettings": {"security": "tls", "tlsSettings": {"serverName": "a.example"}}},
        {"tag": "t", "protocol": "trojan", "settings": {"address": "b.example", "port": 8443, "password": "pw"}}
    ]}]"#).unwrap();
    let OutboundKind::Vless(v) = &ps[0].outbound("proxy").unwrap().kind else { panic!() };
    assert_eq!((v.address.as_str(), v.port, v.flow), ("a.example", 443, Flow::Vision));
    let OutboundKind::Trojan(t) = &ps[0].outbound("t").unwrap().kind else { panic!() };
    assert_eq!((t.address.as_str(), t.port), ("b.example", 8443));
}

#[test]
fn hysteria2_protocol_alias() {
    let text = r#"[{"remarks":"H","outbounds":[{"tag":"proxy","protocol":"hysteria2","settings":{"address":"h.example","port":443,"password":"pw"},"streamSettings":{"security":"tls","tlsSettings":{"serverName":"h.example"}}}]}]"#;
    let p = parse_subscription(text).unwrap().remove(0);
    match &p.main_outbound().unwrap().kind {
        OutboundKind::Hysteria2(h) => assert_eq!((h.address.as_str(), h.port, h.auth.as_str()), ("h.example", 443, "pw")),
        k => panic!("{k:?}"),
    }
}

#[test]
fn base64_json_and_foreign_formats() {
    use base64::Engine;
    let json = r#"[{"remarks":"A","outbounds":[{"tag":"proxy","protocol":"freedom"}]}]"#;
    let wrapped = base64::engine::general_purpose::STANDARD.encode(json);
    let ps = parse_subscription(&wrapped).unwrap();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].remarks, "A");

    let sb = r#"{"outbounds":[{"type":"vless","tag":"proxy","server":"a"},{"type":"direct","tag":"direct"}]}"#;
    assert_eq!(parse_subscription(sb).unwrap()[0].remarks, "proxy");
    let sb_empty = r#"{"outbounds":[{"type":"direct","tag":"direct"}]}"#;
    assert!(parse_subscription(sb_empty).unwrap_err().to_string().contains("sing-box"));
    let clash = "port: 7890\nproxies:\n  - name: a\n    type: vless\n";
    assert!(parse_subscription(clash).unwrap_err().to_string().contains("clash"));
}

#[test]
fn link_security_case() {
    let ps = parse_subscription("vless://00000000-0000-0000-0000-000000000000@h.example:443?security=TLS&sni=h.example#x").unwrap();
    assert!(matches!(ps[0].outbounds[0].stream.security, Security::Tls(_)));
}

#[test]
fn legacy_ss_with_slash() {
    let ps = parse_subscription("ss://YWVzLTEyOC1nY206Pz8/QGV4YW1wbGUuY29tOjgzODg=#legacy").unwrap();
    assert_eq!(ps[0].remarks, "legacy");
    assert!(matches!(ps[0].outbounds[0].kind, OutboundKind::Shadowsocks(_)), "{:?}", ps[0].outbounds[0].kind);
}

#[test]
fn duplicate_remarks_are_unique() {
    let l = "vless://00000000-0000-0000-0000-000000000000@a.example:443?security=tls#DE\nvless://00000000-0000-0000-0000-000000000000@b.example:443?security=tls#DE";
    let ps = parse_subscription(l).unwrap();
    assert_eq!(ps[0].remarks, "DE");
    assert_eq!(ps[1].remarks, "DE (2)");
}

#[test]
fn happ_fragment_and_hy2_gecko() {
    let ps = parse_subscription("vless://00000000-0000-0000-0000-000000000000@h.example:443?security=tls&fragment=1-10,5-20,tlshello#f")
        .unwrap();
    let fm = &ps[0].outbounds[0].stream.finalmask;
    match &fm.tcp[..] {
        [TcpMask::Fragment(f)] => {
            assert_eq!(f.packets, (0, 1));
            assert_eq!(f.lengths, vec![(1, 10)]);
            assert_eq!(f.delays, vec![(5, 20)]);
        }
        other => panic!("{other:?}"),
    }
    let ps = parse_subscription("hy2://pw@h.example:443?obfs=gecko&obfs-password=s&minPacketSize=900&maxPacketSize=1400&sni=h.example#g")
        .unwrap();
    let OutboundKind::Hysteria2(h) = &ps[0].outbounds[0].kind else { panic!("{:?}", ps[0].outbounds[0].kind) };
    let _ = h;
    let fm = &ps[0].outbounds[0].stream.finalmask;
    assert!(fm.udp.iter().any(|m| matches!(m, UdpMask::Salamander { packet_size: Some((900, 1400)), .. })), "{:?}", fm.udp);
}

#[test]
fn hy2_hop_interval_from_link() {
    let ps = parse_subscription("hy2://pw@h.example:20000-20010?sni=h.example&mportHopInt=15#h").unwrap();
    let q = ps[0].outbounds[0].stream.finalmask.quic.as_ref().expect("quic");
    assert_eq!(q.udp_hop_ports.len(), 11);
    assert_eq!(q.udp_hop_interval, (15, 15));
}

#[test]
fn proxy_settings_chain() {
    let j = r#"[{"remarks":"c","outbounds":[
        {"tag":"proxy","protocol":"trojan","settings":{"servers":[{"address":"exit.example","port":443,"password":"p"}]},
         "streamSettings":{"security":"tls"},"proxySettings":{"tag":"entry","transportLayer":true}},
        {"tag":"entry","protocol":"trojan","settings":{"servers":[{"address":"entry.example","port":443,"password":"p"}]},"streamSettings":{"security":"tls"}}
    ]}]"#;
    let ps = parse_subscription(j).unwrap();
    let ob = &ps[0].outbounds[0];
    assert_eq!(ob.stream.sockopt.dialer_proxy.as_deref(), Some("entry"));
    assert!(matches!(ob.stream.security, Security::Tls(_)));
}

#[test]
fn ss_v2ray_plugin_websocket() {
    let l = "ss://YWVzLTEyOC1nY206c2VjcmV0@1.2.3.4:443?plugin=v2ray-plugin%3Bmode%3Dwebsocket%3Btls%3Bhost%3Dcdn.example%3Bpath%3D%2Fws#p";
    let ps = parse_subscription(l).unwrap();
    let ob = &ps[0].outbounds[0];
    assert!(matches!(ob.kind, OutboundKind::Shadowsocks(_)), "{:?}", ob.kind);
    assert!(matches!(&ob.stream.network, Network::Ws(w) if w.path == "/ws" && w.host == "cdn.example"));
    assert!(matches!(&ob.stream.security, Security::Tls(t) if t.server_name == "cdn.example"));
    let l = "ss://YWVzLTEyOC1nY206c2VjcmV0@1.2.3.4:443?plugin=obfs-local%3Bobfs%3Dhttp%3Bobfs-host%3Dwww.example.com#o";
    let ps = parse_subscription(l).unwrap();
    let ob = &ps[0].outbounds[0];
    assert!(matches!(ob.kind, OutboundKind::Shadowsocks(_)), "{:?}", ob.kind);
    assert!(matches!(&ob.stream.network, Network::Other(n) if n == "obfs-local"));
    assert_eq!(ob.stream.plugin_settings, serde_json::json!({ "obfs": "http", "obfs-host": "www.example.com" }));
    let sb = r#"{"outbounds": [{"type": "shadowsocks", "tag": "proxy", "server": "1.2.3.4", "server_port": 443, "method": "aes-128-gcm", "password": "x",
        "plugin": "obfs-local", "plugin_opts": "obfs=tls;obfs-host=a.example"},
        {"type": "vless", "tag": "v", "server": "1.2.3.4", "server_port": 443, "uuid": "9205c503-358f-4d8d-a081-20c98eddf64a", "transport": {"type": "kcp", "seed": "s"}}]}"#;
    let ps = parse_subscription(sb).unwrap();
    let ss = ps.iter().flat_map(|p| &p.outbounds).find(|o| matches!(o.kind, OutboundKind::Shadowsocks(_))).unwrap();
    assert_eq!(ss.stream.plugin_settings["obfs"], "tls");
    let v = ps.iter().flat_map(|p| &p.outbounds).find(|o| matches!(&o.stream.network, Network::Other(n) if n == "kcp")).unwrap();
    assert_eq!(v.stream.plugin_settings, serde_json::json!({ "seed": "s" }));
}

#[test]
fn balancer_member_profile() {
    let text = r#"[{"remarks": "auto", "outbounds": [
        {"tag": "proxy", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.1", "port": 443,
            "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}},
        {"tag": "proxy-2", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.2", "port": 443,
            "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}},
        {"tag": "direct", "protocol": "freedom"}
    ], "routing": {
        "balancers": [{"tag": "b", "selector": ["proxy"]}],
        "rules": [
            {"type": "field", "domain": ["example.org"], "outboundTag": "proxy"},
            {"type": "field", "network": "tcp,udp", "balancerTag": "b"}
        ]}}]"#;
    let all = parse_subscription(text).unwrap();
    let tags: Vec<&str> = all[0].balancer_members().iter().map(|o| o.tag.as_str()).collect();
    assert_eq!(tags, ["proxy", "proxy-2"]);
    let m = find_profile(&all, "auto\u{1f}proxy-2").unwrap();
    assert_eq!(m.outbounds[0].tag, "proxy-2");
    assert_eq!(m.main_outbound().unwrap().tag, "proxy-2");
    assert!(m.balancer_members().is_empty());
    let rules = m.routing.as_ref().unwrap()["rules"].as_array().unwrap();
    assert_eq!(rules[0]["outboundTag"], "proxy\u{1f}");
    assert_eq!(rules[1]["outboundTag"], "proxy-2");
    assert!(find_profile(&all, "auto\u{1f}direct").is_none());
    assert_eq!(find_profile(&all, "auto").unwrap().outbounds[0].tag, "proxy");
}

#[test]
fn enabled_tcp_mux_is_ignored_not_unsupported() {
    let p = parse_subscription(r#"{"outbounds":[{"protocol":"vless","settings":{"vnext":[{"address":"proxy.example","port":443,"users":[{"id":"11111111-2222-3333-4444-555555555555"}]}]},"mux":{"enabled":true}}]}"#).unwrap();
    assert!(!matches!(&p[0].main_outbound().unwrap().kind, OutboundKind::Unsupported { .. }));
}

#[test]
fn json_array_mixes_links_and_configs() {
    let body = r#"["vless://8b775467-5bc8-4344-a79e-90d795f324de@1.2.3.4:443?security=tls&sni=a.example#Link",
        {"remarks":"Config","outbounds":[{"tag":"proxy","protocol":"freedom"}]},
        "trojan://pw@t.example:443?sni=t.example#Trojan"]"#;
    let ps = parse_subscription(body).unwrap();
    assert_eq!(ps.iter().map(|p| p.remarks.as_str()).collect::<Vec<_>>(), ["Link", "Config", "Trojan"]);
}

#[test]
fn broken_fragment_settings_are_refused_like_xray() {
    for frag in
        [r#"{"packets":"tlshello"}"#, r#"{"packets":"tlshello","length":"100-200"}"#, r#"{"packets":5,"length":"1-2","interval":"1-2"}"#]
    {
        let text = format!(
            r#"{{"outbounds":[{{"tag":"proxy","protocol":"vless","settings":{{"vnext":[{{"address":"proxy.example","port":443,"users":[{{"id":"11111111-2222-3333-4444-555555555555"}}]}}]}},"streamSettings":{{"sockopt":{{"dialerProxy":"fragment"}}}}}},{{"tag":"fragment","protocol":"freedom","settings":{{"fragment":{frag}}}}}]}}"#
        );
        let ps = parse_subscription(&text).unwrap();
        let k = &ps[0].outbound("fragment").unwrap().kind;
        assert!(matches!(k, OutboundKind::Unsupported { .. }), "{frag}: {k:?}");
    }
}

#[test]
fn grpc_timeouts_past_u32_saturate_not_wrap() {
    let p = parse_subscription(r#"{"outbounds":[{"tag":"proxy","protocol":"vless","settings":{"vnext":[{"address":"proxy.example","port":443,"users":[{"id":"11111111-2222-3333-4444-555555555555"}]}]},"streamSettings":{"network":"grpc","grpcSettings":{"idle_timeout":4294967306,"health_check_timeout":4294967296}}}]}"#).unwrap();
    let Network::Grpc(g) = &p[0].main_outbound().unwrap().stream.network else { panic!() };
    assert_eq!((g.idle_timeout, g.health_check_timeout), (u32::MAX, u32::MAX));
}

#[test]
fn bad_link_ports_are_refused_not_replaced_with_443() {
    let vmess = |port: &str| {
        format!(
            "vmess://{}",
            base64::engine::general_purpose::STANDARD.encode(format!(
                r#"{{"v":"2","ps":"m","add":"h.example","port":"{port}","id":"11111111-2222-3333-4444-555555555555","net":"tcp"}}"#
            ))
        )
    };
    for link in [
        "vless://11111111-2222-3333-4444-555555555555@h.example:70000?security=tls#v".to_string(),
        "trojan://pw@h.example:abc?security=tls#t".into(),
        "ss://YWVzLTEyOC1nY206cHc@h.example:65536#s".into(),
        vmess("70000"),
    ] {
        let p = parse_subscription(&link).unwrap();
        let k = &p[0].outbounds[0].kind;
        assert!(matches!(k, OutboundKind::Unsupported { .. }), "{link}: {k:?}");
    }
    for (link, port) in [("vless://11111111-2222-3333-4444-555555555555@h.example?security=tls#v".to_string(), 443), (vmess("8443"), 8443)]
    {
        let p = parse_subscription(&link).unwrap();
        assert_eq!(p[0].outbounds[0].kind.server().map(|s| s.1), Some(port), "{link}");
    }
}

#[test]
fn basic_proxy_imports_and_validates_accounts() {
    for (scheme, expected) in [
        ("socks", "SOCKS"),
        ("socks5", "SOCKS"),
        ("socks5h", "SOCKS"),
        ("socks4", "SOCKS"),
        ("socks4a", "SOCKS"),
        ("http", "HTTP"),
        ("https", "HTTP"),
        ("http-proxy", "HTTP"),
        ("https-proxy", "HTTP"),
    ] {
        let s = format!("{scheme}://host.example:1234#proxy");
        let p = parse_subscription(&s).unwrap();
        let ob = p[0].main_outbound().unwrap();
        assert!(ob.kind.is_proxy());
        assert_eq!(ob.kind.server(), Some(("host.example", 1234)));
        assert!(
            match &ob.kind {
                OutboundKind::Socks(_) => expected == "SOCKS",
                OutboundKind::Http(_) => expected == "HTTP",
                _ => false,
            },
            "{scheme}: {:?}",
            ob.kind
        );
        assert_eq!(matches!(ob.stream.security, Security::Tls(_)), scheme.starts_with("https"));
    }
    let p = parse_subscription("socks5://u%3As:p%40ss@host:1080").unwrap();
    let OutboundKind::Socks(s) = &p[0].main_outbound().unwrap().kind else { panic!() };
    assert_eq!(s.server.auth, Some(("u:s".into(), "p@ss".into())));
    let p = parse_subscription("socks://dXNlcjpwYXNz@host:1080").unwrap();
    let OutboundKind::Socks(s) = &p[0].main_outbound().unwrap().kind else { panic!() };
    assert_eq!(s.server.auth, Some(("user".into(), "pass".into())));
    for link in ["socks5://u:@host:1080", "socks4://u:p@host:1080", "http-proxy://host:70000", "socks5://host:0"] {
        let p = parse_subscription(link).unwrap();
        assert!(matches!(p[0].outbounds[0].kind, OutboundKind::Unsupported { .. }), "{link}");
    }
}

#[test]
fn basic_proxy_xray_and_singbox() {
    for protocol in ["socks", "http"] {
        let raw = serde_json::json!({"outbounds":[{"tag":"proxy","protocol":protocol,"settings":{"address":"127.0.0.1","port":8080,"user":"u","pass":"p"}}]});
        let p = parse_subscription(&raw.to_string()).unwrap();
        assert!(p[0].main_outbound().unwrap().kind.server().is_some());
        let raw = serde_json::json!({"outbounds":[{"tag":"test","type":protocol,"server":"127.0.0.1","server_port":8080,"username":"u","password":"p","tls":{"enabled":true,"server_name":"test.example"}}]});
        let p = parse_subscription(&raw.to_string()).unwrap();
        let ob = p[0].main_outbound().unwrap();
        assert!(ob.kind.server().is_some());
        assert!(matches!(ob.stream.security, Security::Tls(_)));
    }
    for settings in [
        serde_json::json!({"servers":[]}),
        serde_json::json!({"servers":[{"address":"x","port":80},{"address":"y","port":81}]}),
        serde_json::json!({"address":"x","port":80,"headers":{"X-Test":"1"}}),
    ] {
        let p = parse_subscription(&serde_json::json!({"outbounds":[{"protocol":"http","settings":settings}]}).to_string()).unwrap();
        assert!(matches!(p[0].outbounds[0].kind, OutboundKind::Unsupported { .. }));
    }
}

#[test]
fn socks_outbound_with_and_without_auth() {
    let ps = parse_subscription("socks5://user:pw@s.example:1081#A\nsocks5://s2.example#B\n").unwrap();
    let OutboundKind::Socks(a) = &ps[0].outbounds[0].kind else { panic!() };
    assert_eq!((a.server.address.as_str(), a.server.port, a.server.auth.clone()), ("s.example", 1081, Some(("user".into(), "pw".into()))));
    let OutboundKind::Socks(b) = &ps[1].outbounds[0].kind else { panic!() };
    assert_eq!((b.server.port, b.server.auth.clone()), (1080, None));
    assert!(ps[0].outbounds[0].kind.is_proxy());

    let flat = r#"[{"remarks":"S","outbounds":[{"tag":"proxy","protocol":"socks","settings":{"address":"f.example","port":1090,"user":"u","pass":"p"}}]}]"#;
    let OutboundKind::Socks(f) = &parse_subscription(flat).unwrap()[0].outbounds[0].kind else { panic!() };
    assert_eq!((f.server.port, f.server.auth.clone()), (1090, Some(("u".into(), "p".into()))));
    let long = format!(
        r#"[{{"remarks":"S","outbounds":[{{"tag":"proxy","protocol":"socks","settings":{{"servers":[{{"address":"x","port":1,"users":[{{"user":"{}","pass":"p"}}]}}]}}}}]}}]"#,
        "a".repeat(256)
    );
    let ps = parse_subscription(&long).unwrap();
    assert!(matches!(ps[0].outbounds[0].kind, OutboundKind::Unsupported { .. }));
}
