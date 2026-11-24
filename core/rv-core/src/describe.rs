use rv_config::{Network, Outbound, OutboundKind, Profile, Security, UdpMask};
use serde_json::{Value, json};

use crate::Proxy;

pub fn is_placeholder(p: &Profile) -> bool {
    let Some(ob) = p.main_outbound() else {
        return true;
    };
    let zero_id = match &ob.kind {
        OutboundKind::Vless(v) => v.id == [0; 16],
        OutboundKind::Vmess(v) => v.id == [0; 16],
        _ => false,
    };
    let dummy_host = ob.kind.server().is_some_and(|(h, port)| {
        port <= 1 || matches!(h.to_ascii_lowercase().as_str(), "0.0.0.0" | "127.0.0.1" | "localhost" | "::" | "::1" | "unsupported.com")
    });
    zero_id || dummy_host
}

fn net_label(n: &Network) -> Option<&str> {
    match n {
        Network::Xhttp(_) => Some("XHTTP"),
        Network::Grpc(_) => Some("gRPC"),
        Network::Ws(_) => Some("WebSocket"),
        Network::HttpUpgrade(_) => Some("HTTPUpgrade"),
        Network::TcpHttpHeader => Some("HTTP"),
        Network::Other(name) if name == "kcp" || name == "mkcp" => Some("mKCP"),
        Network::Other(name) => Some(name),
        _ => None,
    }
}

pub fn kind(p: &Profile) -> String {
    p.main_outbound().map(outbound_kind).unwrap_or_default()
}

fn protocol_name(protocol: &str) -> String {
    match protocol {
        "vmess" => "VMess".into(),
        "tuic" => "TUIC".into(),
        "hysteria1" => "Hysteria 1".into(),
        "wireguard" => "WireGuard".into(),
        "amneziawg" | "awg" => "AmneziaWG".into(),
        "naive" | "naiveproxy" => "NaiveProxy".into(),
        "socks" => "SOCKS".into(),
        "http" => "HTTP".into(),
        "" => "?".into(),
        other => other.to_string(),
    }
}

pub fn outbound_kind(ob: &Outbound) -> String {
    let mut parts: Vec<&str> = Vec::new();
    match &ob.kind {
        OutboundKind::Hysteria2(_) => {
            parts.push("Hysteria 2");
            if ob.stream.finalmask.udp.iter().any(|m| matches!(m, UdpMask::Salamander { .. })) {
                parts.push("salamander");
            }
            return parts.join(" · ");
        }
        OutboundKind::Socks(s) => {
            return match s.version {
                rv_config::SocksVersion::V4 => "SOCKS4",
                rv_config::SocksVersion::V4a => "SOCKS4a",
                rv_config::SocksVersion::V5 => "SOCKS5",
            }
            .into();
        }
        OutboundKind::Http(_) => {
            return if matches!(ob.stream.security, Security::Tls(_)) { "HTTPS CONNECT" } else { "HTTP CONNECT" }.into();
        }
        OutboundKind::Shadowsocks(s) => return format!("Shadowsocks · {}", s.method),
        #[cfg(feature = "plugins")]
        OutboundKind::Plugin { protocol, .. } => {
            return match crate::plugin::for_protocol(protocol) {
                Some(p) => p.manifest.name.clone(),
                None => protocol_name(protocol),
            };
        }
        OutboundKind::Unsupported { protocol, .. } => {
            return protocol_name(protocol);
        }
        #[cfg(not(feature = "plugins"))]
        OutboundKind::Plugin { protocol, .. } => {
            return protocol_name(protocol);
        }
        OutboundKind::Vmess(_) => parts.push("VMess"),
        OutboundKind::Trojan(_) => parts.push("Trojan"),
        _ => {}
    }
    match &ob.stream.security {
        Security::Reality(_) => parts.push("REALITY"),
        Security::Tls(_) => parts.push("TLS"),
        Security::None => {}
    }
    parts.extend(net_label(&ob.stream.network));
    if parts.is_empty() {
        parts.push("VLESS");
    }
    parts.join(" · ")
}

pub fn hosts(p: &Profile) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ob in &p.outbounds {
        if let Some((h, _)) = ob.kind.server()
            && h.parse::<std::net::IpAddr>().is_err()
            && !out.iter().any(|x| x == h)
        {
            out.push(h.to_string());
        }
    }
    let servers = p.dns.as_ref().and_then(|d| d.get("servers")).and_then(|s| s.as_array());
    for s in servers.into_iter().flatten() {
        let addr = s.as_str().or_else(|| s.get("address").and_then(|a| a.as_str())).unwrap_or_default();
        if let Some((up, _)) = crate::dns::parse(addr, None) {
            let (h, _) = up.endpoint();
            if h.parse::<std::net::IpAddr>().is_err() && !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out
}

pub fn describe(text: &str) -> Result<Value, rv_config::Error> {
    let profiles = rv_config::parse_subscription_dial(text)?;
    Ok(Value::Array(
        profiles
            .iter()
            .map(|p| {
                let placeholder = is_placeholder(p);
                let main = p.main_outbound();
                let check = match main {
                    None => Err("no proxy outbound".to_string()),
                    Some(_) if placeholder => Err("placeholder".to_string()),
                    Some(ob) if matches!(&ob.kind, OutboundKind::Unsupported { reason, .. } if reason.starts_with("protocol ")) => {
                        Err(match kind(p).as_str() {
                            "Hysteria 1" => crate::lang::t(
                                "Hysteria 1 не поддерживается, только Hysteria 2",
                                "Hysteria 1 is not supported, only Hysteria 2",
                            )
                            .to_string(),
                            k => format!("{k} {}", crate::lang::t("не поддерживается", "is not supported")),
                        })
                    }
                    Some(ob) => Proxy::build(p, &ob.tag, None).map(|_| ()).map_err(|e| e.to_string()),
                };
                json!({
                    "remarks": p.remarks,
                    "description": p.description,
                    "outbound": main.map(|o| o.tag.as_str()).unwrap_or_default(),
                    "kind": kind(p),
                    "supported": check.is_ok(),
                    "reason": check.err(),
                    "placeholder": placeholder,
                    "hosts": hosts(p),
                    "sni": main.and_then(sni),
                    "fp": main.and_then(fp),
                    "link": main.filter(|_| !placeholder).and_then(|o| crate::link::share_link(o, &p.remarks)),
                    "server": main.and_then(server),
                    "members": members(p),
                })
            })
            .collect(),
    ))
}

fn sni(o: &Outbound) -> Option<&str> {
    match &o.stream.security {
        Security::Tls(t) => Some(t.server_name.as_str()),
        Security::Reality(r) => Some(r.server_name.as_str()),
        Security::None => None,
    }
    .filter(|s| !s.is_empty())
}

fn fp(o: &Outbound) -> Option<&str> {
    match &o.stream.security {
        Security::Tls(t) => Some(t.fingerprint.as_str()),
        Security::Reality(r) => Some(r.fingerprint.as_str()),
        Security::None => None,
    }
    .filter(|s| !s.is_empty())
}

fn server(o: &Outbound) -> Option<String> {
    o.kind.server().map(|(h, port)| if h.contains(':') { format!("[{h}]:{port}") } else { format!("{h}:{port}") })
}

fn members(p: &Profile) -> Value {
    let list = p.balancer_members();
    if list.len() < 2 {
        return Value::Array(Vec::new());
    }
    Value::Array(
        list.into_iter()
            .map(|o| {
                let check = Proxy::build(p, &o.tag, None).map(|_| ()).map_err(|e| e.to_string());
                json!({
                    "name": format!("{}{}{}", p.remarks, rv_config::MEMBER_SEP, o.tag),
                    "tag": o.tag,
                    "kind": outbound_kind(o),
                    "supported": check.is_ok(),
                    "reason": check.err(),
                    "sni": sni(o),
                    "fp": fp(o),
                    "server": server(o),
                    "link": crate::link::share_link(o, &o.tag),
                })
            })
            .collect(),
    )
}

pub fn details(text: &str, remarks: &str) -> Result<Value, String> {
    use crate::lang::t;
    let values = rv_config::subscription_values(text).map_err(|e| e.to_string())?;
    let profiles = rv_config::parse_subscription(text).map_err(|e| e.to_string())?;
    let (i, p, tag) = profiles
        .iter()
        .enumerate()
        .find_map(|(i, p)| {
            if p.remarks == remarks {
                return p.main_outbound().map(|o| (i, p, o.tag.clone()));
            }
            let (r, tag) = remarks.split_once(rv_config::MEMBER_SEP)?;
            (p.remarks == r).then(|| (i, p, tag.to_string()))
        })
        .ok_or_else(|| t("Сервер не найден", "Server not found").to_string())?;
    let ob = p.outbound(&tag).ok_or_else(|| t("Сервер не найден", "Server not found").to_string())?;
    let raw = values
        .get(i)
        .and_then(|v| v.get("outbounds"))
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|o| o.get("tag").and_then(Value::as_str) == Some(tag.as_str())).or_else(|| a.first()))
        .cloned();

    let mut groups: Vec<Value> = Vec::new();
    let row = |k: &str, v: String| json!({ "k": k, "v": v });
    let mono = |k: &str, v: String| json!({ "k": k, "v": v, "mono": true });
    let secret = |k: &str, v: String| json!({ "k": k, "v": v, "mono": true, "secret": true });
    let mut push = |title: String, rows: Vec<Value>| {
        if !rows.is_empty() {
            groups.push(json!({ "title": title, "rows": rows }));
        }
    };
    let id = |b: &[u8; 16]| {
        let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
        format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
    };

    let mut conn = Vec::new();
    let proto = match &ob.kind {
        OutboundKind::Socks(_) | OutboundKind::Http(_) => outbound_kind(ob),
        OutboundKind::Vless(_) => "VLESS".to_string(),
        OutboundKind::Vmess(_) => "VMess".to_string(),
        OutboundKind::Trojan(_) => "Trojan".to_string(),
        OutboundKind::Shadowsocks(_) => "Shadowsocks".to_string(),
        OutboundKind::Hysteria2(_) => "Hysteria 2".to_string(),
        OutboundKind::Unsupported { protocol, .. } => protocol.clone(),
        OutboundKind::Plugin { .. } => outbound_kind(ob),
        _ => outbound_kind(ob),
    };
    conn.push(row(t("Протокол", "Protocol"), proto));
    if let Some((h, port)) = ob.kind.server() {
        conn.push(json!({ "k": t("Адрес", "Address"), "v": h, "copy": true }));
        conn.push(row(t("Порт", "Port"), port.to_string()));
    }
    match &ob.kind {
        OutboundKind::Vless(v) => {
            conn.push(secret("UUID", id(&v.id)));
            let flow = match v.flow {
                rv_config::Flow::None => None,
                rv_config::Flow::Vision => Some("xtls-rprx-vision"),
                rv_config::Flow::VisionUdp443 => Some("xtls-rprx-vision-udp443"),
            };
            if let Some(f) = flow {
                conn.push(mono("Flow", f.into()));
            }
            conn.push(mono(
                t("Шифрование", "Encryption"),
                v.encryption.as_ref().map_or("none".into(), |e| e.raw.chars().take(28).collect::<String>() + "…"),
            ));
        }
        OutboundKind::Vmess(v) => {
            conn.push(secret("UUID", id(&v.id)));
            conn.push(mono(t("Шифрование", "Encryption"), v.security.clone()));
        }
        OutboundKind::Socks(rv_config::Socks { server, .. }) | OutboundKind::Http(rv_config::HttpProxy { server }) => {
            if let Some((user, pass)) = &server.auth {
                conn.push(secret(t("Пользователь", "Username"), user.clone()));
                conn.push(secret(t("Пароль", "Password"), pass.clone()));
            }
            conn.push(row(
                "UDP",
                if Proxy::build(p, &ob.tag, None).is_ok_and(|proxy| proxy.udp_allowed(53)) {
                    "SOCKS5 UDP ASSOCIATE"
                } else {
                    t("Не поддерживается", "Not supported")
                }
                .into(),
            ));
        }
        OutboundKind::Trojan(v) => conn.push(secret(t("Пароль", "Password"), v.password.clone())),
        OutboundKind::Shadowsocks(v) => {
            conn.push(mono(t("Метод", "Method"), v.method.clone()));
            conn.push(secret(t("Пароль", "Password"), v.password.clone()));
        }
        OutboundKind::Hysteria2(v) => conn.push(secret(t("Пароль", "Password"), v.auth.clone())),
        OutboundKind::Unsupported { reason, .. } => conn.push(row(t("Почему не работает", "Why it doesn't work"), reason.clone())),
        _ => {}
    }
    push(t("Соединение", "Connection").into(), conn);

    match &ob.stream.security {
        Security::Tls(s) => {
            let mut r = Vec::new();
            if !s.server_name.is_empty() {
                r.push(row("SNI", s.server_name.clone()));
            }
            if !s.fingerprint.is_empty() {
                r.push(row(t("Отпечаток", "Fingerprint"), s.fingerprint.clone()));
            }
            if !s.alpn.is_empty() {
                r.push(mono("ALPN", s.alpn.join(", ")));
            }
            if s.allow_insecure {
                r.push(row(t("Проверка сертификата", "Certificate check"), t("выключена", "off").into()));
            }
            if !s.pinned_sha256.is_empty() {
                r.push(row(t("Закреплённые сертификаты", "Pinned certificates"), s.pinned_sha256.len().to_string()));
            }
            if !s.verify_names.is_empty() {
                r.push(row(t("Проверять имя", "Verify name"), s.verify_names.join(", ")));
            }
            push("TLS".into(), r);
        }
        Security::Reality(s) => {
            use base64::Engine;
            let mut r = Vec::new();
            if !s.server_name.is_empty() {
                r.push(row("SNI", s.server_name.clone()));
            }
            if !s.fingerprint.is_empty() {
                r.push(row(t("Отпечаток", "Fingerprint"), s.fingerprint.clone()));
            }
            r.push(secret(t("Публичный ключ", "Public key"), base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.public_key)));
            let sid: String = s.short_id.iter().map(|x| format!("{x:02x}")).collect::<String>();
            r.push(mono("Short ID", sid.trim_end_matches("00").to_string()));
            if !s.spider_x.is_empty() {
                r.push(mono("SpiderX", s.spider_x.clone()));
            }
            if !s.mldsa65_verify.is_empty() {
                r.push(row("ML-DSA-65", t("есть", "yes").into()));
            }
            push("REALITY".into(), r);
        }
        Security::None => {}
    }

    let mut tr = Vec::new();
    let net = &ob.stream.network;
    match net {
        Network::Grpc(g) => {
            if !g.service_name.is_empty() {
                tr.push(mono("Service", g.service_name.clone()));
            }
            tr.push(row(t("Режим", "Mode"), if g.multi_mode { "multi" } else { "gun" }.into()));
            if !g.authority.is_empty() {
                tr.push(row("Authority", g.authority.clone()));
            }
            if !g.user_agent.is_empty() {
                tr.push(mono("User-Agent", g.user_agent.clone()));
            }
            if g.idle_timeout > 0 {
                tr.push(row(t("Простой", "Idle timeout"), format!("{} с", g.idle_timeout)));
            }
        }
        Network::Ws(w) => {
            tr.push(mono(t("Путь", "Path"), if w.path.is_empty() { "/".into() } else { w.path.clone() }));
            if !w.host.is_empty() {
                tr.push(row("Host", w.host.clone()));
            }
            for (k, v) in &w.headers {
                tr.push(mono(k, v.clone()));
            }
            if w.early_data > 0 {
                tr.push(row("Early data", w.early_data.to_string()));
            }
            if w.heartbeat_period > 0 {
                tr.push(row("Heartbeat", format!("{} с", w.heartbeat_period)));
            }
        }
        Network::HttpUpgrade(w) => {
            tr.push(mono(t("Путь", "Path"), if w.path.is_empty() { "/".into() } else { w.path.clone() }));
            if !w.host.is_empty() {
                tr.push(row("Host", w.host.clone()));
            }
            for (k, v) in &w.headers {
                tr.push(mono(k, v.clone()));
            }
        }
        Network::Xhttp(v) => {
            if let Some(o) = v.as_object() {
                for (k, x) in o {
                    match x {
                        Value::String(s) if !s.is_empty() => tr.push(mono(k, s.clone())),
                        Value::Number(_) | Value::Bool(_) => tr.push(mono(k, x.to_string())),
                        Value::Object(_) | Value::Array(_) => tr.push(mono(k, x.to_string().chars().take(60).collect())),
                        _ => {}
                    }
                }
            }
        }
        Network::Hysteria(h) => {
            if h.udp_idle_timeout > 0 {
                tr.push(row(t("Простой UDP", "UDP idle"), format!("{} с", h.udp_idle_timeout)));
            }
        }
        Network::TcpHttpHeader => tr.push(row(t("Маскировка", "Disguise"), "HTTP".into())),
        _ => {}
    }
    for m in &ob.stream.finalmask.udp {
        match m {
            UdpMask::Salamander { password, .. } => tr.push(secret("Salamander", password.clone())),
            UdpMask::Other(n) => tr.push(row(t("Маска UDP", "UDP mask"), n.clone())),
        }
    }
    for m in &ob.stream.finalmask.tcp {
        match m {
            rv_config::TcpMask::Fragment(f) => {
                let r = |v: &[(u64, u64)]| {
                    v.iter().map(|(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") }).collect::<Vec<_>>().join(", ")
                };
                tr.push(row(t("Фрагментация", "Fragmentation"), format!("{} · {} {}", r(&f.lengths), r(&f.delays), t("мс", "ms"))));
            }
            rv_config::TcpMask::Other(n) => tr.push(row(t("Маска TCP", "TCP mask"), n.clone())),
        }
    }
    if let Some(q) = &ob.stream.finalmask.quic {
        if !q.congestion.is_empty() {
            tr.push(row(t("Управление потоком", "Congestion"), q.congestion.clone()));
        }
        if q.brutal_up > 0 || q.brutal_down > 0 {
            tr.push(row("Brutal", format!("↑ {} · ↓ {}", q.brutal_up, q.brutal_down)));
        }
        if !q.udp_hop_ports.is_empty() {
            tr.push(row(t("Прыжки портов", "Port hopping"), format!("{} {}", q.udp_hop_ports.len(), t("портов", "ports"))));
        }
    }
    let title = match net_label(net) {
        Some(l) => format!("{} · {l}", t("Транспорт", "Transport")),
        None => format!("{} · {}", t("Транспорт", "Transport"), net.name().to_uppercase()),
    };
    push(title, tr);

    let mut more = Vec::new();
    if let Some(m) = raw.as_ref().and_then(|o| o.get("mux")).and_then(|m| m.get("enabled")).and_then(Value::as_bool) {
        more.push(row("Mux", if m { t("включён", "on") } else { t("выключен", "off") }.into()));
    }
    let so = &ob.stream.sockopt;
    if let Some(d) = &so.dialer_proxy {
        more.push(row(t("Через сервер", "Via server"), d.clone()));
    }
    let fam = match so.family {
        rv_config::Family::Any => None,
        rv_config::Family::V4 { .. } => Some("IPv4"),
        rv_config::Family::V6 { .. } => Some("IPv6"),
        rv_config::Family::V4First => Some("IPv4, IPv6"),
        rv_config::Family::V6First => Some("IPv6, IPv4"),
    };
    if let Some(f) = fam {
        more.push(row(t("Адреса", "Addresses"), f.into()));
    }
    if let Some(c) = &so.tcp_congestion {
        more.push(row("TCP congestion", c.clone()));
    }
    if let Some((a, b)) = so.tcp_keepalive {
        more.push(row("Keep-alive", format!("{a} / {b} с")));
    }
    if so.happy_eyeballs.is_some() {
        more.push(row("Happy Eyeballs", t("включено", "on").into()));
    }
    if ob.reject_udp443 {
        more.push(row("QUIC", t("заблокирован", "blocked").into()));
    }
    if let Some(d) = &p.description {
        more.push(row(t("Описание", "Description"), d.clone()));
    }
    if p.outbounds.iter().filter(|o| o.kind.is_proxy()).count() > 1 && p.balancer_members().len() < 2 {
        more.push(row(
            t("Цепочка", "Chain"),
            p.outbounds.iter().filter(|o| o.kind.is_proxy()).map(|o| o.tag.as_str()).collect::<Vec<_>>().join(" → "),
        ));
    }
    push(t("Дополнительно", "More").into(), more);

    Ok(json!({
        "groups": groups,
        "link": crate::link::share_link(ob, &p.remarks),
        "json": raw.map(|r| serde_json::to_string_pretty(&r).unwrap_or_default()),
    }))
}

pub fn routing(text: &str, profile: &str, options: &Value) -> Result<Value, String> {
    let profiles = rv_config::parse_subscription(text).map_err(|e| e.to_string())?;
    let mut p = rv_config::find_profile(&profiles, profile).ok_or_else(|| "no such profile".to_string())?;
    crate::tune::Tune::from_json(options)?.apply(&mut p)?;
    let string = |key: &str| options.get(key).and_then(Value::as_str).unwrap_or("");
    let opts = crate::engine::Options {
        routing: crate::engine::RoutingMode::from_name(string("routingMode")).unwrap_or_default(),
        user_rules: string("userRules").to_string(),
        sub_routing: options.get("subRouting").filter(|v| v.is_object()).cloned(),
        dns: options
            .get("dns")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default(),
        ipv4_only: options.get("ipv4Only").and_then(Value::as_bool).unwrap_or(false),
        ..Default::default()
    };
    let geo = if string("geoDir").is_empty() {
        rv_rules::GeoData::default()
    } else {
        crate::geo::load_dir(std::path::Path::new(string("geoDir")))
    };
    crate::engine::Engine::inspect(&p, &geo, &opts).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn details_group_everything_about_the_server() {
        let l = "vless://9205c503-358f-4d8d-a081-20c98eddf64a@fi.example.net:443?encryption=none&flow=xtls-rprx-vision&type=grpc&serviceName=grpc&mode=multi\
                 &security=reality&sni=www.microsoft.com&fp=chrome&pbk=D-nWk6c566_IKoExMW7koFwczBHF4nIaRGQo93HDNRE&sid=6ba85179#Fi";
        let d = super::details(l, "Fi").unwrap();
        let rows = |title: &str| {
            d["groups"].as_array().unwrap().iter().find(|g| g["title"].as_str().unwrap().contains(title)).unwrap()["rows"].clone()
        };
        let get = |title: &str, k: &str| {
            rows(title).as_array().unwrap().iter().find(|r| r["k"] == k).map(|r| r["v"].as_str().unwrap().to_string())
        };
        assert_eq!(get("Соединение", "Адрес").as_deref(), Some("fi.example.net"));
        assert_eq!(get("Соединение", "UUID").as_deref(), Some("9205c503-358f-4d8d-a081-20c98eddf64a"));
        assert_eq!(get("REALITY", "Short ID").as_deref(), Some("6ba85179"));
        assert_eq!(get("gRPC", "Service").as_deref(), Some("grpc"));
        assert!(rows("REALITY").as_array().unwrap().iter().any(|r| r["secret"] == true));
        assert!(d["json"].as_str().unwrap().contains("\"serviceName\""));
        assert!(super::details(l, "Nope").is_err());
    }
}
