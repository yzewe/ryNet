use base64::Engine;
use rv_config::{Flow, Network, Outbound, OutboundKind, Security, UdpMask};

pub fn share_link(ob: &Outbound, name: &str) -> Option<String> {
    let (scheme, user, host, port) = match &ob.kind {
        OutboundKind::Socks(s) => {
            return Some(basic_link(
                ob,
                &s.server,
                match s.version {
                    rv_config::SocksVersion::V4 => "socks4",
                    rv_config::SocksVersion::V4a => "socks4a",
                    rv_config::SocksVersion::V5 => "socks5",
                },
                name,
            ));
        }
        OutboundKind::Http(h) => {
            return Some(basic_link(
                ob,
                &h.server,
                if matches!(ob.stream.security, Security::Tls(_)) { "https-proxy" } else { "http-proxy" },
                name,
            ));
        }
        OutboundKind::Vless(v) => ("vless", uuid(&v.id), v.address.as_str(), v.port),
        OutboundKind::Vmess(v) => return Some(vmess_link(ob, v, name)),
        OutboundKind::Trojan(t) => ("trojan", t.password.clone(), t.address.as_str(), t.port),
        OutboundKind::Hysteria2(h) => ("hysteria2", h.auth.clone(), h.address.as_str(), h.port),
        OutboundKind::Shadowsocks(s) => {
            let user = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!("{}:{}", s.method, s.password));
            return Some(format!("ss://{user}@{}:{}#{}", host_part(&s.address), s.port, enc(name)));
        }
        _ => return None,
    };
    let mut q: Vec<(&str, String)> = Vec::new();
    match &ob.kind {
        OutboundKind::Vless(v) => {
            q.push(("encryption", v.encryption.as_ref().map_or_else(|| "none".into(), |e| e.raw.clone())));
            match v.flow {
                Flow::Vision => q.push(("flow", "xtls-rprx-vision".into())),
                Flow::VisionUdp443 => q.push(("flow", "xtls-rprx-vision-udp443".into())),
                Flow::None => {}
            }
        }
        OutboundKind::Vmess(v) => q.push(("encryption", v.security.clone())),
        OutboundKind::Hysteria2(_) => {
            if let Security::Tls(t) = &ob.stream.security {
                if !t.server_name.is_empty() {
                    q.push(("sni", t.server_name.clone()));
                }
                if t.allow_insecure {
                    q.push(("insecure", "1".into()));
                }
                if !t.pinned_sha256.is_empty() {
                    q.push(("pinSHA256", hex_list(&t.pinned_sha256)));
                }
            }
            for m in &ob.stream.finalmask.udp {
                if let UdpMask::Salamander { password, .. } = m {
                    q.push(("obfs", "salamander".into()));
                    q.push(("obfs-password", password.clone()));
                }
            }
            return Some(finish(scheme, &user, host, port, &q, name));
        }
        _ => {}
    }
    stream_params(ob, &mut q);
    Some(finish(scheme, &user, host, port, &q, name))
}

fn basic_link(ob: &Outbound, server: &rv_config::ProxyServer, scheme: &str, name: &str) -> String {
    let mut q = Vec::new();
    stream_params(ob, &mut q);
    let user = server.auth.as_ref().map(|(u, p)| format!("{}:{}@", enc(u), enc(p))).unwrap_or_default();
    let query = q.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&");
    format!("{scheme}://{user}{}:{}?{query}#{}", host_part(&server.address), server.port, enc(name))
}

fn vmess_link(ob: &Outbound, v: &rv_config::Vmess, name: &str) -> String {
    let mut q = Vec::new();
    stream_params(ob, &mut q);
    let get = |key| q.iter().find(|(k, _)| *k == key).map(|(_, value)| value.as_str()).unwrap_or("");
    let mut value = serde_json::json!({
        "v": "2", "ps": name, "add": v.address, "port": v.port.to_string(), "id": uuid(&v.id),
        "aid": "0", "scy": v.security, "net": get("type"), "type": "none",
        "host": get("host"), "path": get("path"), "tls": get("security"),
        "sni": get("sni"), "alpn": get("alpn"), "fp": get("fp"),
    });
    match &ob.stream.network {
        Network::Grpc(_) => {
            value["path"] = serde_json::json!(get("serviceName"));
            value["host"] = serde_json::json!(get("authority"));
            value["type"] = serde_json::json!(if get("mode") == "multi" { "multi" } else { "gun" });
        }
        Network::Xhttp(_) => value["type"] = serde_json::json!(get("mode")),
        Network::TcpHttpHeader => value["type"] = serde_json::json!("http"),
        _ => {}
    }
    for k in ["pbk", "sid", "allowInsecure", "pcs", "vcn", "extra"] {
        if !get(k).is_empty() {
            value[k] = serde_json::json!(get(k));
        }
    }
    if get("allowInsecure") == "1" {
        value["insecure"] = serde_json::json!("1");
    }
    format!("vmess://{}", base64::engine::general_purpose::STANDARD.encode(value.to_string()))
}

fn stream_params(ob: &Outbound, q: &mut Vec<(&'static str, String)>) {
    let net = &ob.stream.network;
    let kind = match net {
        Network::TcpHttpHeader => "tcp",
        other => other.name(),
    };
    q.push(("type", kind.into()));
    match net {
        Network::TcpHttpHeader => {
            q.push(("headerType", "http".into()));
            if let Some(h) = &ob.stream.http_header {
                q.push(("path", h.paths.join(",")));
                if let Some((_, values)) = h.request_headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("Host")) {
                    q.push(("host", values.join(",")));
                }
            }
        }
        Network::Ws(w) => {
            q.push(("path", with_ed(&w.path, w.early_data)));
            if !w.host.is_empty() {
                q.push(("host", w.host.clone()));
            }
        }
        Network::HttpUpgrade(h) => {
            q.push(("path", with_ed(&h.path, h.early_data)));
            if !h.host.is_empty() {
                q.push(("host", h.host.clone()));
            }
        }
        Network::Grpc(g) => {
            q.push(("serviceName", g.service_name.clone()));
            if !g.authority.is_empty() {
                q.push(("authority", g.authority.clone()));
            }
            if g.multi_mode {
                q.push(("mode", "multi".into()));
            }
        }
        Network::Xhttp(x) => {
            let get = |k: &str| x.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty());
            q.push(("path", get("path").unwrap_or("/").into()));
            if let Some(h) = get("host") {
                q.push(("host", h.into()));
            }
            q.push(("mode", get("mode").unwrap_or("auto").into()));
            if let Some(extra) = x.get("extra").filter(|e| !e.is_null()) {
                q.push(("extra", extra.to_string()));
            }
        }
        _ => {}
    }
    match &ob.stream.security {
        Security::None => q.push(("security", "none".into())),
        Security::Tls(t) => {
            q.push(("security", "tls".into()));
            if !t.server_name.is_empty() {
                q.push(("sni", t.server_name.clone()));
            }
            if !t.fingerprint.is_empty() {
                q.push(("fp", t.fingerprint.clone()));
            }
            if !t.alpn.is_empty() {
                q.push(("alpn", t.alpn.join(",")));
            }
            if t.allow_insecure {
                q.push(("allowInsecure", "1".into()));
            }
            if !t.pinned_sha256.is_empty() {
                q.push(("pcs", hex_list(&t.pinned_sha256)));
            }
            if !t.verify_names.is_empty() {
                q.push(("vcn", t.verify_names.join(",")));
            }
        }
        Security::Reality(r) => {
            q.push(("security", "reality".into()));
            q.push(("sni", r.server_name.clone()));
            q.push(("fp", if r.fingerprint.is_empty() { "chrome".into() } else { r.fingerprint.clone() }));
            q.push(("pbk", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r.public_key)));
            let len = r.short_id.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
            q.push(("sid", r.short_id[..len].iter().map(|b| format!("{b:02x}")).collect()));
        }
    }
}

fn hex_list(h: &[[u8; 32]]) -> String {
    h.iter().map(|x| x.iter().map(|b| format!("{b:02x}")).collect::<String>()).collect::<Vec<_>>().join(",")
}

fn with_ed(path: &str, early: usize) -> String {
    let path = if path.is_empty() { "/" } else { path };
    if early == 0 { path.to_string() } else { format!("{path}{}ed={early}", if path.contains('?') { '&' } else { '?' }) }
}

fn finish(scheme: &str, user: &str, host: &str, port: u16, q: &[(&str, String)], name: &str) -> String {
    let query: Vec<String> = q.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect();
    let mut s = format!("{scheme}://{}@{}:{port}", enc(user), host_part(host));
    if !query.is_empty() {
        s.push('?');
        s.push_str(&query.join("&"));
    }
    s.push('#');
    s.push_str(&enc(name));
    s
}

fn host_part(h: &str) -> String {
    if h.contains(':') { format!("[{h}]") } else { h.to_string() }
}

fn uuid(id: &[u8; 16]) -> String {
    let h: String = id.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(link: &str) -> String {
        let p = &rv_config::parse_subscription(link).unwrap()[0];
        share_link(p.main_outbound().unwrap(), &p.remarks).unwrap()
    }

    #[test]
    fn vless_reality_roundtrip() {
        let l = "vless://9205c503-358f-4d8d-a081-20c98eddf64a@1.2.3.4:443?encryption=none&flow=xtls-rprx-vision&type=tcp&security=reality&sni=www.example.com&fp=chrome&pbk=D-nWk6c566_IKoExMW7koFwczBHF4nIaRGQo93HDNRE&sid=a1b2c3d4#%F0%9F%87%A9%F0%9F%87%AA%20Test";
        assert_eq!(roundtrip(l), l);
        assert_eq!(roundtrip(&roundtrip(l)), l);
    }

    #[test]
    fn vless_encryption_roundtrip() {
        let key = "A".repeat(43);
        let l = format!(
            "vless://9205c503-358f-4d8d-a081-20c98eddf64a@1.2.3.4:443?encryption=mlkem768x25519plus.native.0rtt.100-111-1111.{key}&type=tcp&security=none#e"
        );
        assert_eq!(roundtrip(&l), l);
    }

    #[test]
    fn transports_roundtrip() {
        for l in [
            "vless://9205c503-358f-4d8d-a081-20c98eddf64a@h.example:8443?encryption=none&type=grpc&serviceName=svc&mode=multi&security=tls&sni=h.example&fp=chrome#g",
            "vless://9205c503-358f-4d8d-a081-20c98eddf64a@h.example:443?encryption=none&type=ws&path=%2Fws&host=cdn.example&security=tls&sni=cdn.example#w",
            "vless://9205c503-358f-4d8d-a081-20c98eddf64a@h.example:443?encryption=none&type=xhttp&path=%2Fx&mode=auto&security=reality&sni=a.example&fp=chrome&pbk=D-nWk6c566_IKoExMW7koFwczBHF4nIaRGQo93HDNRE&sid=#x",
            "trojan://pa%24s@t.example:443?type=tcp&security=tls&sni=t.example#t",
            "hysteria2://auth@hy.example:443?sni=hy.example&obfs=salamander&obfs-password=pw#h",
            "vless://9205c503-358f-4d8d-a081-20c98eddf64a@31.76.60.110:443?encryption=none&type=grpc&serviceName=g&security=tls&sni=lk.x5.ru&fp=chrome&pcs=1f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a7988&vcn=lk.x5.ru#pinned",
            "hysteria2://auth@hy.example:443?sni=hy.example&pinSHA256=1f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a7988#hp",
        ] {
            assert_eq!(roundtrip(l), l, "{l}");
        }
    }

    #[test]
    fn shadowsocks_link() {
        let l = roundtrip("ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTpwYXNz@s.example:8388#s");
        assert_eq!(l, "ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTpwYXNz@s.example:8388#s");
    }

    #[test]
    fn vmess_v2rayn_share_links_preserve_transports_and_security() {
        for original in [
            "vmess://9205c503-358f-4d8d-a081-20c98eddf64a@[2001:db8::1]:8443?encryption=aes-128-gcm&type=ws&path=%2Fws%3Fed%3D2048&host=cdn.example&security=tls&sni=cdn.example&alpn=http%2F1.1&allowInsecure=1#VMess%20test",
            "vmess://9205c503-358f-4d8d-a081-20c98eddf64a@g.example:443?encryption=chacha20-poly1305&type=grpc&serviceName=svc%2Fname&authority=a.example&mode=multi&security=tls&sni=g.example#grpc",
            "vmess://9205c503-358f-4d8d-a081-20c98eddf64a@x.example:443?encryption=auto&type=xhttp&path=%2Fx&mode=packet-up&extra=%7B%22xmux%22%3A%7B%22maxConcurrency%22%3A%222-4%22%7D%7D&security=none#xhttp",
        ] {
            let p = rv_config::parse_subscription(original).unwrap().remove(0);
            let shared = share_link(p.main_outbound().unwrap(), &p.remarks).unwrap();
            let json = base64::engine::general_purpose::STANDARD.decode(shared.strip_prefix("vmess://").unwrap()).unwrap();
            let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
            assert_eq!(json["v"], "2");
            assert_eq!(json["aid"], "0");
            let restored = rv_config::parse_subscription(&shared).unwrap().remove(0);
            assert_eq!(restored.remarks, p.remarks);
            assert_eq!(format!("{:?}", restored.main_outbound().unwrap().kind), format!("{:?}", p.main_outbound().unwrap().kind));
            assert_eq!(format!("{:?}", restored.main_outbound().unwrap().stream), format!("{:?}", p.main_outbound().unwrap().stream));
            let description = crate::describe::describe(&shared).unwrap();
            assert_eq!(description[0]["supported"], true);
            assert!(description[0]["kind"].as_str().unwrap().starts_with("VMess"));
            assert_eq!(
                description[0]["server"].as_str().unwrap(),
                crate::describe::describe(original).unwrap()[0]["server"].as_str().unwrap()
            );
        }
    }
}

#[cfg(test)]
mod basic_proxy_links {
    #[test]
    fn roundtrip() {
        for link in [
            "socks5://u%3As:p%40ss@[::1]:1080#name",
            "socks4a://id@host:1080#four",
            "http-proxy://user:pass@host:3128#plain",
            "https-proxy://user:pass@host:443?sni=test.example&allowInsecure=1#secure",
        ] {
            let a = rv_config::parse_subscription(link).unwrap().remove(0);
            let out = super::share_link(a.main_outbound().unwrap(), &a.remarks).unwrap();
            let b = rv_config::parse_subscription(&out).unwrap().remove(0);
            assert_eq!(format!("{:?}", a.main_outbound().unwrap().kind), format!("{:?}", b.main_outbound().unwrap().kind), "{out}");
            assert_eq!(
                super::super::describe::outbound_kind(a.main_outbound().unwrap()),
                super::super::describe::outbound_kind(b.main_outbound().unwrap())
            );
            if link.starts_with("http") {
                assert!(out.starts_with("http-proxy://") || out.starts_with("https-proxy://"));
            }
        }
    }
}
