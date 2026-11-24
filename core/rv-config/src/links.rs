use base64::Engine;
use serde_json::{Map, Value, json};

pub fn parse(body: &str) -> Vec<Value> {
    let text = decode_body(body);
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with("//") && l.contains("://"))
        .filter(|l| !is_directive(l))
        .filter_map(parse_link)
        .collect()
}

fn is_directive(line: &str) -> bool {
    let (scheme, rest) = line.split_once("://").unwrap_or(("", line));
    let rest = rest.to_ascii_lowercase();
    scheme.eq_ignore_ascii_case("happ")
        || scheme.eq_ignore_ascii_case("incy")
        || ["routing/", "autorouting/", "onadd/"].iter().any(|p| rest.starts_with(p))
}

fn decode_body(body: &str) -> String {
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.contains("://")
        && let Some(t) = b64(&compact).and_then(|b| String::from_utf8(b).ok())
        && t.contains("://")
    {
        return t;
    }
    body.to_string()
}

pub(crate) fn b64(s: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let s = s.trim();
    [STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|e| e.decode(s).ok())
        .or_else(|| STANDARD_NO_PAD.decode(s.trim_end_matches('=')).ok())
        .or_else(|| URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok())
}

pub(crate) fn percent_decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Link {
    scheme: String,
    user: String,
    host: String,
    port: u16,
    bad_port: bool,
    ports: String,
    query: Vec<(String, String)>,
    name: String,
    description: Option<String>,
}

impl Link {
    fn get(&self, k: &str) -> Option<&str> {
        self.query.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }
    fn flag(&self, k: &str) -> bool {
        matches!(self.get(k), Some("1" | "true" | "True" | "yes"))
    }
}

impl Link {
    fn port_json(&self) -> Value {
        if self.bad_port { json!(self.ports) } else { json!(self.port) }
    }
}

fn split_link(line: &str) -> Option<Link> {
    let (scheme, rest) = line.split_once("://")?;
    let (rest, fragment) = match rest.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((a, b)) => (a, b),
        None => (rest, ""),
    };
    let rest = rest.trim_end_matches('/');
    let (user, hostport) = match rest.rsplit_once('@') {
        Some((u, h)) => (percent_decode(u), h),
        None => (String::new(), rest),
    };
    let hostport = hostport.split('/').next().unwrap_or(hostport);
    let (host, port) = if let Some(h) = hostport.strip_prefix('[') {
        let (h, p) = h.split_once(']')?;
        (h.to_string(), p.trim_start_matches(':'))
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p),
            None => (hostport.to_string(), ""),
        }
    };
    let ports = port.to_string();
    let first = port.split([',', '-']).next().unwrap_or("");
    let bad_port = !first.is_empty() && first.parse::<u16>().is_err();
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "socks" | "socks5" | "socks5h" | "socks4" | "socks4a" => 1080,
        "http" | "http-proxy" => 80,
        _ => 443,
    };
    let port = first.parse().unwrap_or(default_port);
    let query = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect();
    let fragment = fragment.map(percent_decode).unwrap_or_default();
    let (name, description) = match fragment.split_once("?serverDescription=") {
        Some((n, d)) => (n.trim().to_string(), b64(d).and_then(|b| String::from_utf8(b).ok())),
        None => (fragment.trim().to_string(), None),
    };
    Some(Link { scheme: scheme.to_ascii_lowercase(), user, host, port, bad_port, ports, query, name, description })
}

fn profile(name: &str, description: Option<String>, proxy: Value) -> Value {
    let mut p = json!({
        "remarks": name,
        "outbounds": [
            proxy,
            { "tag": "direct", "protocol": "freedom" },
            { "tag": "block", "protocol": "blackhole" }
        ]
    });
    if let Some(d) = description.filter(|d| !d.trim().is_empty()) {
        p["meta"] = json!({ "serverDescription": d });
    }
    p
}

pub type LinkHook = fn(&str) -> Option<(String, Value)>;
static LINK_HOOK: std::sync::OnceLock<LinkHook> = std::sync::OnceLock::new();

pub fn set_link_hook(hook: LinkHook) {
    let _ = LINK_HOOK.set(hook);
}

fn parse_link(line: &str) -> Option<Value> {
    if let Some(hook) = LINK_HOOK.get()
        && let Some((scheme, _)) = line.split_once("://")
        && !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "vless"
                | "vmess"
                | "trojan"
                | "hysteria2"
                | "hy2"
                | "hysteria"
                | "ss"
                | "socks"
                | "socks5"
                | "socks5h"
                | "socks4"
                | "socks4a"
                | "http"
                | "https"
                | "http-proxy"
                | "https-proxy"
        )
        && let Some((name, outbound)) = hook(line)
    {
        return Some(profile(&name, None, outbound));
    }
    if let Some(b) = line.strip_prefix("vmess://") {
        return Some(vmess(b));
    }
    if let Some(b) = line.strip_prefix("ss://") {
        let (body, name) = b.split_once('#').map_or((b, None), |(a, n)| (a, Some(n)));
        let (body, query) = body.split_once('?').map_or((body, None), |(a, q)| (a, Some(q)));
        if !body.contains('@')
            && let Some(dec) = b64(body.trim_end_matches('/')).and_then(|d| String::from_utf8(d).ok())
            && let Some((userinfo, hostport)) = dec.rsplit_once('@')
        {
            use base64::Engine;
            let user = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(userinfo);
            let q = query.map(|q| format!("?{q}")).unwrap_or_default();
            let n = name.map(|n| format!("#{n}")).unwrap_or_default();
            return parse_link(&format!("ss://{user}@{hostport}{q}{n}"));
        }
    }
    let l = split_link(line)?;
    let name = if l.name.is_empty() { format!("{}:{}", l.host, l.port) } else { l.name.clone() };
    let proxy = match l.scheme.as_str() {
        "vless" => {
            let mut user = json!({ "id": l.user, "encryption": l.get("encryption").unwrap_or("none") });
            if let Some(f) = l.get("flow") {
                user["flow"] = json!(f);
            }
            json!({
                "tag": "proxy",
                "protocol": "vless",
                "settings": { "vnext": [{ "address": l.host, "port": l.port_json(), "users": [user] }] },
                "streamSettings": stream(&l)
            })
        }
        "trojan" => json!({
            "tag": "proxy",
            "protocol": "trojan",
            "settings": { "servers": [{ "address": l.host, "port": l.port_json(), "password": l.user }] },
            "streamSettings": stream_with_default_security(&l, "tls")
        }),
        "hysteria2" | "hy2" => hysteria2(&l),
        "hysteria" => json!({ "tag": "proxy", "protocol": "hysteria1" }),
        "socks" | "socks5" | "socks5h" | "socks4" | "socks4a" | "http" | "https" | "http-proxy" | "https-proxy" => {
            let mut server = json!({"address": l.host, "port": l.port_json()});
            let authority = line.split_once("://")?.1.split(['?', '#']).next()?;
            if let Some((raw_user, _)) = authority.rsplit_once('@') {
                let decoded;
                let raw_user = if !raw_user.contains(':') && l.scheme.starts_with("socks") {
                    decoded = b64(raw_user).and_then(|d| String::from_utf8(d).ok()).filter(|s| s.contains(':'));
                    decoded.as_deref().unwrap_or(raw_user)
                } else {
                    raw_user
                };
                let (u, p) = raw_user.split_once(':').unwrap_or((raw_user, ""));
                server["users"] = json!([{"user":percent_decode(u),"pass":percent_decode(p)}]);
            }
            let http = matches!(l.scheme.as_str(), "http" | "https" | "http-proxy" | "https-proxy");
            let version = match l.scheme.as_str() {
                "socks4" => "4",
                "socks4a" => "4a",
                _ => "5",
            };
            json!({"tag":"proxy","protocol":if http {"http"} else {"socks"},
                "settings":{"servers":[server],"version":version},
                "streamSettings":stream_with_default_security(&l,if l.scheme == "https" || l.scheme == "https-proxy" {"tls"} else {"none"})})
        }
        "ss" => shadowsocks(&l)?,
        other => json!({ "tag": "proxy", "protocol": other }),
    };
    Some(profile(&name, l.description.clone(), proxy))
}

fn stream(l: &Link) -> Value {
    stream_with_default_security(l, "none")
}

fn stream_with_default_security(l: &Link, default_security: &str) -> Value {
    let network = match l.get("type").unwrap_or("tcp").to_ascii_lowercase().as_str() {
        "raw" => "tcp",
        "splithttp" => "xhttp",
        "http" | "h2" => "h2",
        n => n,
    }
    .to_string();
    let mut s = Map::new();
    s.insert("network".into(), json!(network));
    let host = l.get("host");
    let path = l.get("path");
    match network.as_str() {
        "tcp" => {
            if l.get("headerType") == Some("http") {
                let mut request = json!({ "path": path.filter(|p| !p.is_empty()).unwrap_or("/").split(',').collect::<Vec<_>>() });
                if let Some(h) = host.filter(|h| !h.is_empty()) {
                    request["headers"] = json!({ "Host": h.split(',').collect::<Vec<_>>() });
                }
                s.insert("tcpSettings".into(), json!({ "header": { "type": "http", "request": request } }));
            }
        }
        "ws" => {
            let mut p = path.unwrap_or("/").to_string();
            if let Some(ed) = l.get("ed").filter(|e| e.parse::<u32>().is_ok())
                && !p.contains("ed=")
            {
                p.push(if p.contains('?') { '&' } else { '?' });
                p.push_str("ed=");
                p.push_str(ed);
            }
            let mut w = json!({ "path": p });
            if let Some(h) = host {
                w["host"] = json!(h);
            }
            s.insert("wsSettings".into(), w);
        }
        "httpupgrade" => {
            let mut w = json!({ "path": path.unwrap_or("/") });
            if let Some(h) = host {
                w["host"] = json!(h);
            }
            s.insert("httpupgradeSettings".into(), w);
        }
        "grpc" => {
            s.insert(
                "grpcSettings".into(),
                json!({
                    "serviceName": l.get("serviceName").unwrap_or(""),
                    "authority": l.get("authority").unwrap_or(""),
                    "multiMode": l.get("mode") == Some("multi"),
                }),
            );
        }
        "xhttp" => {
            let mut x = json!({ "path": path.unwrap_or("/"), "mode": l.get("mode").unwrap_or("auto") });
            if let Some(h) = host {
                x["host"] = json!(h);
            }
            if let Some(extra) = l.get("extra").and_then(|e| serde_json::from_str::<Value>(e).ok()) {
                x["extra"] = extra;
            }
            s.insert("xhttpSettings".into(), x);
        }
        _ => {}
    }
    let security = l.get("security").unwrap_or(default_security).to_ascii_lowercase();
    let sni = l.get("sni").or(l.get("peer")).unwrap_or("");
    let fp = l.get("fp").unwrap_or("");
    match security.as_str() {
        "tls" => {
            let mut t = json!({
                "serverName": sni,
                "fingerprint": fp,
                "allowInsecure": l.flag("allowInsecure") || l.flag("insecure"),
            });
            if let Some(a) = l.get("alpn") {
                t["alpn"] = json!(a.split(',').map(str::trim).collect::<Vec<_>>());
            }
            if let Some(p) = l.get("pcs") {
                t["pinnedPeerCertSha256"] = json!(p);
            }
            if let Some(n) = l.get("vcn") {
                t["verifyPeerCertByName"] = json!(n);
            }
            s.insert("security".into(), json!("tls"));
            s.insert("tlsSettings".into(), t);
        }
        "reality" => {
            s.insert("security".into(), json!("reality"));
            s.insert(
                "realitySettings".into(),
                json!({
                    "serverName": sni,
                    "fingerprint": if fp.is_empty() { "chrome" } else { fp },
                    "publicKey": l.get("pbk").unwrap_or(""),
                    "shortId": l.get("sid").unwrap_or(""),
                    "spiderX": l.get("spx").unwrap_or(""),
                    "mldsa65Verify": l.get("pqv").unwrap_or(""),
                }),
            );
        }
        "none" | "" => {
            s.insert("security".into(), json!("none"));
        }
        other => {
            s.insert("security".into(), json!(other));
        }
    }
    if let Some(fm) = l.get("fm").and_then(|f| serde_json::from_str::<Value>(f).ok()) {
        s.insert("finalmask".into(), fm);
    }
    if !s.contains_key("finalmask")
        && let Some(f) = l.get("fragment")
    {
        let p: Vec<&str> = f.split(',').map(str::trim).collect();
        let mut set = json!({ "length": p.first().copied().unwrap_or(""), "delay": p.get(1).copied().unwrap_or("") });
        if let Some(pk) = p.get(2) {
            set["packets"] = json!(pk);
        }
        if let Some(m) = p.get(3) {
            set["maxSplit"] = json!(m);
        }
        s.insert("finalmask".into(), json!({ "tcp": [{ "type": "fragment", "settings": set }] }));
    }
    Value::Object(s)
}

fn hysteria2(l: &Link) -> Value {
    let sni = l.get("sni").unwrap_or(&l.host);
    let mut stream = json!({
        "network": "hysteria",
        "hysteriaSettings": { "version": 2, "auth": l.user },
        "security": "tls",
        "tlsSettings": {
            "serverName": sni,
            "alpn": ["h3"],
            "allowInsecure": l.flag("insecure") || l.flag("allowInsecure"),
        }
    });
    if let Some(p) = l.get("pinSHA256").or(l.get("pcs")) {
        stream["tlsSettings"]["pinnedPeerCertSha256"] = json!(p);
    }
    let mut fm = Map::new();
    if let Some(obfs) = l.get("obfs").filter(|o| !o.is_empty() && *o != "none") {
        let password = l.get("obfs-password").unwrap_or("");
        let mask = if obfs.eq_ignore_ascii_case("gecko") {
            let size = format!("{}-{}", l.get("minPacketSize").unwrap_or("512"), l.get("maxPacketSize").unwrap_or("1200"));
            json!({ "type": "salamander", "settings": { "password": password, "packetSize": size } })
        } else {
            json!({ "type": obfs, "settings": { "password": password } })
        };
        fm.insert("udp".into(), json!([mask]));
    }
    let hop = l.get("mport").map(str::to_string).or_else(|| l.ports.contains([',', '-']).then(|| l.ports.clone()));
    if let Some(ports) = hop {
        let mut h = json!({ "ports": ports });
        if let Some(i) = l.get("mportHopInt").or(l.get("hopInterval")).or(l.get("hop_interval")) {
            h["interval"] = json!(i.trim_end_matches('s'));
        }
        fm.insert("quicParams".into(), json!({ "udpHop": h }));
    }
    if !fm.is_empty() {
        stream["finalmask"] = Value::Object(fm);
    }
    json!({
        "tag": "proxy",
        "protocol": "hysteria",
        "settings": { "address": l.host, "port": l.port_json(), "version": 2 },
        "streamSettings": stream
    })
}

fn shadowsocks(l: &Link) -> Option<Value> {
    let (method, password, host, port) = if l.host.is_empty() || l.user.is_empty() {
        let enc = if l.user.is_empty() { &l.host } else { &l.user };
        let dec = String::from_utf8(b64(enc)?).ok()?;
        let (mp, hp) = dec.rsplit_once('@')?;
        let (m, p) = mp.split_once(':')?;
        let (h, port) = hp.rsplit_once(':')?;
        (m.to_string(), p.to_string(), h.to_string(), port.parse::<u16>().map_or_else(|_| json!(port), |p| json!(p)))
    } else {
        let userinfo = match b64(&l.user).and_then(|b| String::from_utf8(b).ok()) {
            Some(u) if u.contains(':') => u,
            _ => l.user.clone(),
        };
        let (m, p) = userinfo.split_once(':')?;
        (m.to_string(), p.to_string(), l.host.clone(), l.port_json())
    };
    let mut ob = json!({
        "tag": "proxy",
        "protocol": "shadowsocks",
        "settings": { "servers": [{ "address": host, "port": port, "method": method, "password": password }] }
    });
    if let Some(plugin) = l.get("plugin") {
        let mut parts = plugin.split(';').map(str::trim);
        let name = parts.next().unwrap_or("");
        let opts: Vec<(&str, &str)> = parts.map(|p| p.split_once('=').unwrap_or((p, ""))).collect();
        let opt = |k: &str| opts.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        match name {
            "v2ray-plugin" | "xray-plugin" if opt("mode").is_none_or(|m| m == "websocket") => {
                let host = opt("host").filter(|h| !h.is_empty()).unwrap_or(&host);
                let mut s = json!({ "network": "ws", "wsSettings": { "path": opt("path").unwrap_or("/"), "host": host } });
                if opt("tls").is_some() {
                    s["security"] = json!("tls");
                    s["tlsSettings"] = json!({ "serverName": host });
                }
                ob["streamSettings"] = s;
            }
            "" => {}
            other => ob["streamSettings"] = crate::singbox::plugin_stream(other, &opts),
        }
    }
    Some(ob)
}

fn vmess(b: &str) -> Value {
    let v: Value = b64(b.split('#').next().unwrap_or(b)).and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or(Value::Null);
    if !v.is_object() {
        if let Some(l) = split_link(&format!("vmess://{b}")) {
            let name = if l.name.is_empty() { format!("{}:{}", l.host, l.port) } else { l.name.clone() };
            let security = l.get("encryption").or(l.get("scy")).unwrap_or("auto").to_string();
            return profile(&name, l.description.clone(), vmess_proxy(&l, &security));
        }
        return profile("VMess", None, json!({ "tag": "proxy", "protocol": "vmess" }));
    }
    let f = |k: &str| match v.get(k) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    };
    let net = f("net");
    let kind = f("type");
    let mut query: Vec<(String, String)> = vec![("type".into(), if net.is_empty() { "tcp".into() } else { net.clone() })];
    let mut q = |k: &str, val: String| {
        if !val.is_empty() {
            query.push((k.into(), val))
        }
    };
    q("host", f("host"));
    q("path", f("path"));
    q("security", f("tls"));
    q("sni", f("sni"));
    q("alpn", f("alpn"));
    q("fp", f("fp"));
    q("pbk", f("pbk"));
    q("sid", f("sid"));
    q("aid", f("aid"));
    q("allowInsecure", f("allowInsecure"));
    q("insecure", f("insecure"));
    q("pcs", f("pcs"));
    q("vcn", f("vcn"));
    q("extra", v.get("extra").map(|x| x.as_str().map(str::to_owned).unwrap_or_else(|| x.to_string())).unwrap_or_default());
    match net.as_str() {
        "grpc" => {
            q("serviceName", f("path"));
            q("authority", f("host"));
            q("mode", kind.clone());
        }
        "xhttp" | "splithttp" => q("mode", kind.clone()),
        _ if kind == "http" => q("headerType", kind.clone()),
        _ => {}
    }
    let l = Link {
        scheme: "vmess".into(),
        user: f("id"),
        host: f("add"),
        port: f("port").parse().unwrap_or(443),
        bad_port: !f("port").is_empty() && f("port").parse::<u16>().is_err(),
        ports: f("port"),
        query,
        name: f("ps"),
        description: None,
    };
    let name = if l.name.is_empty() { format!("{}:{}", l.host, l.port) } else { l.name.clone() };
    let security = match f("scy") {
        s if s.is_empty() => "auto".to_string(),
        s => s,
    };
    profile(&name, None, vmess_proxy(&l, &security))
}

fn vmess_proxy(l: &Link, security: &str) -> Value {
    let mut user = json!({ "id": l.user, "security": security });
    if let Some(aid) = l.get("aid").or(l.get("alterId")) {
        user["alterId"] = json!(aid);
    }
    json!({
        "tag": "proxy",
        "protocol": "vmess",
        "settings": { "vnext": [{ "address": l.host, "port": l.port_json(), "users": [user] }] },
        "streamSettings": stream(l)
    })
}

#[cfg(kani)]
#[path = "proofs/links.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vless_reality_xhttp_link() {
        let l = "vless://6b749590-1931-489d-be46-7a096f9ade06@nl.example.net:8443?encryption=none&type=xhttp&path=%2Fassets%2Fv2&mode=stream-one&extra=%7B%22xmux%22%3A%7B%22maxConcurrency%22%3A%2216-32%22%7D%7D&security=reality&sni=s3.example.com&fp=chrome&pbk=Gu_QdtOX6E8pZcYvDWgHKA5dxcit7hrpU27sizCEWXg&sid=e0&spx=%2F#%F0%9F%87%B3%F0%9F%87%B1%20Netherlands";
        let p = &parse(l)[0];
        assert_eq!(p["remarks"], "🇳🇱 Netherlands");
        let ob = &p["outbounds"][0];
        assert_eq!(ob["settings"]["vnext"][0]["port"], 8443);
        assert_eq!(ob["streamSettings"]["network"], "xhttp");
        assert_eq!(ob["streamSettings"]["xhttpSettings"]["path"], "/assets/v2");
        assert_eq!(ob["streamSettings"]["xhttpSettings"]["extra"]["xmux"]["maxConcurrency"], "16-32");
        assert_eq!(ob["streamSettings"]["realitySettings"]["shortId"], "e0");
    }

    #[test]
    fn client_directives_are_not_servers() {
        let body = "incy://routing/onadd/eyJOYW1lIjoiUlUifQ==\n\
                    incy://autorouting/onadd/https://example.com/r.json\n\
                    ://routing/add?data=eyJOYW1lIjoiUlUifQ==\n\
                    happ://routing/onadd/eyJOYW1lIjoiUlUifQ==\n\
                    trojan://pw@t.example:443?sni=t.example#T\n";
        let ps = parse(body);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0]["remarks"], "T");
    }

    #[test]
    fn base64_body_and_happ_description() {
        let body = base64::engine::general_purpose::STANDARD.encode(
            "vless://00000000-0000-0000-0000-000000000000@127.0.0.1:1?encryption=none&security=none#━━ ОСНОВНЫЕ ━━?serverDescription=0JTQu9GPIFdpRmk=\n\
             hysteria2://secret@nl.example.net:443/?sni=www.bing.com&obfs=salamander&obfs-password=pw&mport=20000-30000#NL%20Hy2\n",
        );
        let ps = parse(&body);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0]["remarks"], "━━ ОСНОВНЫЕ ━━");
        assert_eq!(ps[0]["meta"]["serverDescription"], "Для WiFi");
        let hy = &ps[1]["outbounds"][0];
        assert_eq!(hy["protocol"], "hysteria");
        assert_eq!(hy["streamSettings"]["hysteriaSettings"]["auth"], "secret");
        assert_eq!(hy["streamSettings"]["tlsSettings"]["serverName"], "www.bing.com");
        assert_eq!(hy["streamSettings"]["finalmask"]["udp"][0]["settings"]["password"], "pw");
        assert_eq!(hy["streamSettings"]["finalmask"]["quicParams"]["udpHop"]["ports"], "20000-30000");
    }

    #[test]
    fn shadowsocks_and_trojan() {
        let ss = parse("ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTpwYXNz@1.2.3.4:8388#SS");
        let s = &ss[0]["outbounds"][0]["settings"]["servers"][0];
        assert_eq!(s["method"], "chacha20-ietf-poly1305");
        assert_eq!(s["password"], "pass");
        assert_eq!(s["port"], 8388);
        let tr = parse("trojan://pw@t.example.com:443?sni=t.example.com&type=ws&path=%2Fws#T");
        let ob = &tr[0]["outbounds"][0];
        assert_eq!(ob["settings"]["servers"][0]["password"], "pw");
        assert_eq!(ob["streamSettings"]["security"], "tls");
        assert_eq!(ob["streamSettings"]["wsSettings"]["path"], "/ws");
    }

    #[test]
    fn socks_links() {
        let ps = parse("socks5://user:p%40ss@1.2.3.4:1081#S5\nsocks://example.com#Bare\nsocks://dXNlcjpwdw==@5.6.7.8:9000");
        assert_eq!(ps.len(), 3);
        let ob = &ps[0]["outbounds"][0];
        assert_eq!(ob["protocol"], "socks");
        assert_eq!(ob["settings"]["servers"][0]["address"], "1.2.3.4");
        assert_eq!(ob["settings"]["servers"][0]["port"], 1081);
        assert_eq!(ob["settings"]["servers"][0]["users"][0]["pass"], "p@ss");
        let bare = &ps[1]["outbounds"][0]["settings"]["servers"][0];
        assert_eq!(bare["port"], 1080);
        assert!(bare.get("users").is_none());
        let b = &ps[2]["outbounds"][0]["settings"]["servers"][0];
        assert_eq!(b["users"][0]["user"], "user");
        assert_eq!(b["users"][0]["pass"], "pw");
    }
}
