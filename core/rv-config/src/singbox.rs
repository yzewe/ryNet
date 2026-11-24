use serde_json::{Map, Value, json};

pub fn parse(v: &Value) -> Vec<Value> {
    let configs: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        o => vec![o],
    };
    let mut groups = Vec::new();
    let mut servers = Vec::new();
    for c in configs {
        let obs: Vec<&Value> = c.get("outbounds").and_then(Value::as_array).into_iter().flatten().collect();
        let by_tag = |t: &str| obs.iter().copied().find(|o| tag(o) == t);
        for o in &obs {
            match kind(o) {
                "urltest" => {
                    let members: Vec<&Value> = o
                        .get("outbounds")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .filter_map(by_tag)
                        .filter(|m| is_server(m))
                        .collect();
                    if members.len() > 1 {
                        groups.push(balancer(tag(o), &members));
                    }
                }
                _ if is_server(o) => servers.push(profile(tag(o), proxy(o, "proxy"))),
                _ => {}
            }
        }
    }
    groups.extend(servers);
    groups
}

fn kind(o: &Value) -> &str {
    o.get("type").and_then(Value::as_str).unwrap_or_default()
}

fn tag(o: &Value) -> &str {
    o.get("tag").and_then(Value::as_str).unwrap_or_default()
}

fn is_server(o: &Value) -> bool {
    !matches!(kind(o), "" | "direct" | "block" | "dns" | "selector" | "urltest")
}

fn profile(name: &str, proxy: Value) -> Value {
    json!({
        "remarks": name,
        "outbounds": [
            proxy,
            { "tag": "direct", "protocol": "freedom" },
            { "tag": "block", "protocol": "blackhole" }
        ]
    })
}

fn balancer(name: &str, members: &[&Value]) -> Value {
    let mut outbounds: Vec<Value> = members.iter().map(|m| proxy(m, tag(m))).collect();
    outbounds.push(json!({ "tag": "direct", "protocol": "freedom" }));
    outbounds.push(json!({ "tag": "block", "protocol": "blackhole" }));
    let selector: Vec<&str> = members.iter().map(|m| tag(m)).collect();
    json!({
        "remarks": name,
        "outbounds": outbounds,
        "routing": {
            "balancers": [{ "tag": "auto", "selector": selector, "strategy": { "type": "leastPing" } }],
            "rules": [{ "type": "field", "network": "tcp,udp", "balancerTag": "auto" }]
        }
    })
}

fn s<'a>(o: &'a Value, k: &str) -> &'a str {
    o.get(k).and_then(Value::as_str).unwrap_or_default()
}

fn port(o: &Value) -> u64 {
    o.get("server_port").and_then(Value::as_u64).unwrap_or(443)
}

fn proxy(o: &Value, tag: &str) -> Value {
    let server = s(o, "server");
    match kind(o) {
        "vless" => {
            let mut user = json!({ "id": s(o, "uuid"), "encryption": "none" });
            if let Some(f) = o.get("flow").and_then(Value::as_str).filter(|f| !f.is_empty()) {
                user["flow"] = json!(f);
            }
            json!({
                "tag": tag,
                "protocol": "vless",
                "settings": { "vnext": [{ "address": server, "port": port(o), "users": [user] }] },
                "streamSettings": stream(o)
            })
        }
        "vmess" => json!({
            "tag": tag,
            "protocol": "vmess",
            "settings": { "vnext": [{ "address": server, "port": port(o), "users": [{ "id": s(o, "uuid"), "security": o.get("security").and_then(Value::as_str).unwrap_or("auto"), "alterId": o.get("alter_id").cloned().unwrap_or(json!(0)) }] }] },
            "streamSettings": stream(o)
        }),
        "trojan" => json!({
            "tag": tag,
            "protocol": "trojan",
            "settings": { "servers": [{ "address": server, "port": port(o), "password": s(o, "password") }] },
            "streamSettings": stream(o)
        }),
        "socks" | "http" => {
            let mut server =
                json!({"address":server,"port":o.get("server_port").cloned().unwrap_or(json!(if kind(o) == "socks" { 1080 } else { 0 }))});
            if !s(o, "username").is_empty() || !s(o, "password").is_empty() {
                server["users"] = json!([{"user":s(o,"username"),"pass":s(o,"password")}]);
            }
            let mut settings = json!({"servers":[server],"version":o.get("version").cloned().unwrap_or(json!("5"))});
            if let Some(headers) = o.get("headers") {
                settings["headers"] = headers.clone();
            }
            json!({"tag":tag,"protocol":kind(o),"settings":settings,"streamSettings":stream(o)})
        }
        "hysteria2" => hysteria2(o, tag),
        "shadowsocks" => shadowsocks(o, tag),
        other => {
            let mut settings = o.as_object().cloned().unwrap_or_default();
            settings.remove("type");
            settings.remove("tag");
            json!({ "tag": tag, "protocol": other, "settings": settings })
        }
    }
}

fn stream(o: &Value) -> Value {
    let mut st = Map::new();
    let t = o.get("transport").unwrap_or(&Value::Null);
    let host = t.get("headers").and_then(|h| h.get("Host")).and_then(|h| match h {
        Value::String(s) => Some(s.as_str()),
        Value::Array(a) => a.first().and_then(Value::as_str),
        _ => None,
    });
    let path = t.get("path").and_then(Value::as_str).unwrap_or("/");
    match kind(t) {
        "" => {
            st.insert("network".into(), json!("tcp"));
        }
        "ws" => {
            let mut p = path.to_string();
            if let Some(ed) = t.get("max_early_data").and_then(Value::as_u64).filter(|&e| e > 0)
                && !p.contains("ed=")
            {
                p.push(if p.contains('?') { '&' } else { '?' });
                p.push_str(&format!("ed={ed}"));
            }
            let mut w = json!({ "path": p });
            if let Some(h) = host {
                w["host"] = json!(h);
            }
            st.insert("network".into(), json!("ws"));
            st.insert("wsSettings".into(), w);
        }
        "httpupgrade" => {
            let mut w = json!({ "path": path });
            if let Some(h) = t.get("host").and_then(Value::as_str).or(host) {
                w["host"] = json!(h);
            }
            st.insert("network".into(), json!("httpupgrade"));
            st.insert("httpupgradeSettings".into(), w);
        }
        "grpc" => {
            st.insert("network".into(), json!("grpc"));
            st.insert("grpcSettings".into(), json!({ "serviceName": s(t, "service_name") }));
        }
        "http" => {
            let hosts: Vec<&str> = match t.get("host") {
                Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
                Some(Value::String(h)) => vec![h.as_str()],
                _ => host.into_iter().collect(),
            };
            st.insert("network".into(), json!("h2"));
            st.insert("httpSettings".into(), json!({ "path": path, "host": hosts }));
        }
        other => {
            let mut rest = t.as_object().cloned().unwrap_or_default();
            rest.remove("type");
            st.insert("network".into(), json!(other));
            st.insert(format!("{other}Settings"), Value::Object(rest));
        }
    }
    let tls = o.get("tls").filter(|t| t.get("enabled").and_then(Value::as_bool).unwrap_or(false));
    match tls {
        None => {
            st.insert("security".into(), json!("none"));
        }
        Some(tls) => {
            let fp = tls
                .get("utls")
                .filter(|u| u.get("enabled").and_then(Value::as_bool).unwrap_or(false))
                .map(|u| s(u, "fingerprint"))
                .unwrap_or_default();
            let reality = tls.get("reality").filter(|r| r.get("enabled").and_then(Value::as_bool).unwrap_or(false));
            if let Some(r) = reality {
                st.insert("security".into(), json!("reality"));
                st.insert(
                    "realitySettings".into(),
                    json!({
                        "serverName": s(tls, "server_name"),
                        "fingerprint": if fp.is_empty() { "chrome" } else { fp },
                        "publicKey": s(r, "public_key"),
                        "shortId": s(r, "short_id"),
                    }),
                );
            } else {
                let mut ts = json!({
                    "serverName": s(tls, "server_name"),
                    "fingerprint": fp,
                    "allowInsecure": tls.get("insecure").and_then(Value::as_bool).unwrap_or(false),
                });
                if let Some(a) = alpn(tls) {
                    ts["alpn"] = a;
                }
                st.insert("security".into(), json!("tls"));
                st.insert("tlsSettings".into(), ts);
            }
        }
    }
    Value::Object(st)
}

fn alpn(tls: &Value) -> Option<Value> {
    match tls.get("alpn")? {
        Value::String(a) => Some(json!([a])),
        Value::Array(a) if !a.is_empty() => Some(Value::Array(a.clone())),
        _ => None,
    }
}

fn hysteria2(o: &Value, tag: &str) -> Value {
    let tls = o.get("tls").unwrap_or(&Value::Null);
    let mut ts = json!({
        "serverName": if s(tls, "server_name").is_empty() { s(o, "server") } else { s(tls, "server_name") },
        "alpn": alpn(tls).unwrap_or_else(|| json!(["h3"])),
        "allowInsecure": tls.get("insecure").and_then(Value::as_bool).unwrap_or(false),
    });
    if ts["serverName"] == "" {
        ts.as_object_mut().unwrap().remove("serverName");
    }
    let mut st = json!({
        "network": "hysteria",
        "hysteriaSettings": { "version": 2, "auth": s(o, "password") },
        "security": "tls",
        "tlsSettings": ts,
    });
    let mut fm = Map::new();
    if let Some(obfs) = o.get("obfs").filter(|ob| !s(ob, "type").is_empty()) {
        fm.insert("udp".into(), json!([{ "type": s(obfs, "type"), "settings": { "password": s(obfs, "password") } }]));
    }
    let mut quic = Map::new();
    let ports: Vec<String> = o
        .get("server_ports")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|p| p.replace(':', "-"))
        .collect();
    if !ports.is_empty() {
        let mut h = json!({ "ports": ports.join(",") });
        let interval = s(o, "hop_interval").trim_end_matches('s');
        if !interval.is_empty() {
            h["interval"] = json!(interval);
        }
        quic.insert("udpHop".into(), h);
    }
    let mbps = |k: &str| o.get(k).and_then(Value::as_u64).filter(|&m| m > 0);
    if let Some(up) = mbps("up_mbps") {
        quic.insert("congestion".into(), json!("brutal"));
        quic.insert("brutalUp".into(), json!(format!("{up} mbps")));
        if let Some(down) = mbps("down_mbps") {
            quic.insert("brutalDown".into(), json!(format!("{down} mbps")));
        }
    }
    if !quic.is_empty() {
        fm.insert("quicParams".into(), Value::Object(quic));
    }
    if !fm.is_empty() {
        st["finalmask"] = Value::Object(fm);
    }
    json!({
        "tag": tag,
        "protocol": "hysteria",
        "settings": { "address": s(o, "server"), "port": port(o), "version": 2 },
        "streamSettings": st
    })
}

pub(crate) fn plugin_stream(plugin: &str, opts: &[(&str, &str)]) -> Value {
    let settings: Map<String, Value> = opts
        .iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| (k.to_string(), if v.is_empty() { json!(true) } else { json!(v) }))
        .collect();
    json!({ "network": plugin, format!("{plugin}Settings"): settings })
}

fn shadowsocks(o: &Value, tag: &str) -> Value {
    let mut ob = json!({
        "tag": tag,
        "protocol": "shadowsocks",
        "settings": { "servers": [{ "address": s(o, "server"), "port": port(o), "method": s(o, "method"), "password": s(o, "password") }] }
    });
    let plugin = s(o, "plugin");
    if plugin.is_empty() {
        return ob;
    }
    let opts: Vec<(&str, &str)> = s(o, "plugin_opts").split(';').map(str::trim).map(|p| p.split_once('=').unwrap_or((p, ""))).collect();
    let opt = |k: &str| opts.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    if plugin == "v2ray-plugin" && opt("mode").is_none_or(|m| m == "websocket") {
        let host = opt("host").filter(|h| !h.is_empty()).unwrap_or(s(o, "server"));
        let mut st = json!({ "network": "ws", "wsSettings": { "path": opt("path").unwrap_or("/"), "host": host } });
        if opt("tls").is_some() {
            st["security"] = json!("tls");
            st["tlsSettings"] = json!({ "serverName": host });
        }
        ob["streamSettings"] = st;
    } else {
        ob["streamSettings"] = plugin_stream(plugin, &opts);
    }
    ob
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{
      "log": {"level": "warn"},
      "outbounds": [
        {"type": "selector", "tag": "proxy", "outbounds": ["auto", "🇩🇪 DE", "🇳🇱 NL", "HY"]},
        {"type": "urltest", "tag": "auto", "outbounds": ["🇩🇪 DE", "🇳🇱 NL", "HY"], "interval": "3m"},
        {"type": "vless", "tag": "🇩🇪 DE", "server": "de.example.net", "server_port": 443,
         "uuid": "9205c503-358f-4d8d-a081-20c98eddf64a", "flow": "xtls-rprx-vision",
         "tls": {"enabled": true, "server_name": "www.microsoft.com",
                 "utls": {"enabled": true, "fingerprint": "firefox"},
                 "reality": {"enabled": true, "public_key": "Gu_QdtOX6E8pZcYvDWgHKA5dxcit7hrpU27sizCEWXg", "short_id": "e0"}}},
        {"type": "trojan", "tag": "🇳🇱 NL", "server": "nl.example.net", "server_port": 8443, "password": "pw",
         "tls": {"enabled": true, "server_name": "nl.example.net", "alpn": ["http/1.1"]},
         "transport": {"type": "ws", "path": "/ws", "headers": {"Host": "cdn.example.net"}, "max_early_data": 2048, "early_data_header_name": "Sec-WebSocket-Protocol"}},
        {"type": "hysteria2", "tag": "HY", "server": "hy.example.net", "server_port": 443,
         "server_ports": ["20000:30000"], "hop_interval": "30s", "up_mbps": 50, "down_mbps": 200,
         "password": "secret", "obfs": {"type": "salamander", "password": "ob"},
         "tls": {"enabled": true, "server_name": "www.bing.com", "insecure": true}},
        {"type": "tuic", "tag": "TUIC", "server": "t.example.net", "server_port": 443},
        {"type": "shadowsocks", "tag": "SS", "server": "1.2.3.4", "server_port": 8388, "method": "2022-blake3-aes-128-gcm", "password": "k"},
        {"type": "direct", "tag": "direct"},
        {"type": "block", "tag": "block"},
        {"type": "dns", "tag": "dns-out"}
      ],
      "route": {"final": "proxy"}
    }"#;

    #[test]
    fn servers_and_urltest_balancer() {
        let v: Value = serde_json::from_str(CONFIG).unwrap();
        let ps = parse(&v);
        let names: Vec<&str> = ps.iter().map(|p| p["remarks"].as_str().unwrap()).collect();
        assert_eq!(names, ["auto", "🇩🇪 DE", "🇳🇱 NL", "HY", "TUIC", "SS"]);

        let b = &ps[0];
        assert_eq!(b["routing"]["balancers"][0]["selector"], json!(["🇩🇪 DE", "🇳🇱 NL", "HY"]));
        assert_eq!(b["outbounds"].as_array().unwrap().len(), 5);

        let de = &ps[1]["outbounds"][0];
        assert_eq!(de["tag"], "proxy");
        assert_eq!(de["settings"]["vnext"][0]["users"][0]["flow"], "xtls-rprx-vision");
        assert_eq!(de["streamSettings"]["security"], "reality");
        assert_eq!(de["streamSettings"]["realitySettings"]["fingerprint"], "firefox");
        assert_eq!(de["streamSettings"]["realitySettings"]["shortId"], "e0");

        let nl = &ps[2]["outbounds"][0]["streamSettings"];
        assert_eq!(nl["wsSettings"]["path"], "/ws?ed=2048");
        assert_eq!(nl["wsSettings"]["host"], "cdn.example.net");
        assert_eq!(nl["tlsSettings"]["alpn"], json!(["http/1.1"]));

        let hy = &ps[3]["outbounds"][0]["streamSettings"];
        assert_eq!(hy["hysteriaSettings"]["auth"], "secret");
        assert_eq!(hy["tlsSettings"]["allowInsecure"], true);
        assert_eq!(hy["finalmask"]["udp"][0]["type"], "salamander");
        assert_eq!(hy["finalmask"]["quicParams"]["udpHop"]["ports"], "20000-30000");
        assert_eq!(hy["finalmask"]["quicParams"]["udpHop"]["interval"], "30");
        assert_eq!(hy["finalmask"]["quicParams"]["brutalUp"], "50 mbps");

        assert_eq!(ps[4]["outbounds"][0]["protocol"], "tuic");
    }

    #[test]
    fn socks_outbound() {
        let v: Value = serde_json::from_str(
            r#"{"outbounds": [{"type": "socks", "tag": "S", "server": "1.2.3.4", "server_port": 1081, "username": "u", "password": "p"},
                              {"type": "socks", "tag": "S2", "server": "5.6.7.8"}]}"#,
        )
        .unwrap();
        let ps = parse(&v);
        let a = &ps[0]["outbounds"][0];
        assert_eq!(a["protocol"], "socks");
        assert_eq!(a["settings"]["servers"][0]["port"], 1081);
        assert_eq!(a["settings"]["servers"][0]["users"][0]["user"], "u");
        let b = &ps[1]["outbounds"][0]["settings"]["servers"][0];
        assert_eq!(b["port"], 1080);
        assert!(b.get("users").is_none());
    }

    #[test]
    fn whole_subscription_parses() {
        let ps = crate::parse_subscription(CONFIG).unwrap();
        assert_eq!(ps.len(), 6);
        assert!(ps[1].main_outbound().is_some());
        assert!(ps[3].main_outbound().is_some(), "hysteria2: {:?}", ps[3].outbounds[0].kind);
        assert!(ps[5].main_outbound().is_some(), "ss: {:?}", ps[5].outbounds[0].kind);
    }
}
