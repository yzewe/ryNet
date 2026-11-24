use serde_json::{Value, json};

pub struct Tags<'a> {
    pub direct: &'a str,
    pub proxy: &'a str,
    pub block: &'a str,
}

fn field<'a>(p: &'a Value, key: &str) -> Option<&'a Value> {
    p.get(key).or_else(|| p.as_object()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v))
}

fn list(p: &Value, key: &str) -> Vec<String> {
    field(p, key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn text<'a>(p: &'a Value, key: &str) -> &'a str {
    field(p, key).and_then(Value::as_str).map(str::trim).unwrap_or("")
}

fn order(p: &Value) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for w in text(p, "RouteOrder").to_ascii_lowercase().split(['-', ',', ' ']) {
        if let Some(g) = ["block", "proxy", "direct"].into_iter().find(|g| *g == w)
            && !out.contains(&g)
        {
            out.push(g);
        }
    }
    for g in ["block", "proxy", "direct"] {
        if !out.contains(&g) {
            out.push(g);
        }
    }
    out
}

pub fn rules(p: &Value, t: &Tags) -> Vec<Value> {
    let mut out = Vec::new();
    for group in order(p) {
        let (sites, ips, tag) = match group {
            "block" => ("BlockSites", "BlockIp", t.block),
            "proxy" => ("ProxySites", "ProxyIp", t.proxy),
            _ => ("DirectSites", "DirectIp", t.direct),
        };
        let d = list(p, sites);
        if !d.is_empty() {
            out.push(json!({ "type": "field", "domain": d, "outboundTag": tag }));
        }
        let i = list(p, ips);
        if !i.is_empty() {
            out.push(json!({ "type": "field", "ip": i, "outboundTag": tag }));
        }
    }
    let rest = if global_proxy(p) { t.proxy } else { t.direct };
    out.push(json!({ "type": "field", "network": "tcp,udp", "outboundTag": rest }));
    out
}

fn decode_b64(s: &str) -> Option<String> {
    use base64::Engine;
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let t = s.trim();
    [STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|e| e.decode(t.trim_end_matches('=')).ok().or_else(|| e.decode(t).ok()))
        .and_then(|b| String::from_utf8(b).ok())
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16))
        {
            out.push((h << 4 | l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_profile(input: &str) -> Result<Value, String> {
    let text = input.trim();
    let mut data = text;
    if let Some(at) = text.to_ascii_lowercase().find("://") {
        let rest = &text[at + 3..];
        let rest = rest.strip_prefix("routing/").or_else(|| rest.strip_prefix("import-routing/")).unwrap_or(rest);
        let rest = rest.strip_prefix("add/").or_else(|| rest.strip_prefix("onadd/")).unwrap_or(rest);
        data = rest.split_once("?data=").map_or(rest, |(_, d)| d);
    }
    let data = data.trim().trim_start_matches("base64:");
    let json = if data.starts_with('{') { data.to_string() } else { decode_b64(&unescape(data)).ok_or("not a routing profile")? };
    serde_json::from_str::<Value>(&json).ok().filter(Value::is_object).ok_or_else(|| "not a routing profile".to_string())
}

fn domain_rule(d: &str, target: &str) -> Option<String> {
    let d = d.trim();
    let (kind, v) = d.split_once(':').map_or(("", d), |(k, v)| (k, v));
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    Some(match kind.to_ascii_lowercase().as_str() {
        "geosite" => format!("GEOSITE,{v},{target}"),
        "domain" => format!("DOMAIN-SUFFIX,{v},{target}"),
        "full" => format!("DOMAIN,{v},{target}"),
        "keyword" => format!("DOMAIN-KEYWORD,{v},{target}"),
        "regexp" => format!("DOMAIN-REGEX,{v},{target}"),
        "" => format!("DOMAIN-KEYWORD,{v},{target}"),
        _ => return None,
    })
}

fn ip_rule(i: &str, target: &str) -> Option<String> {
    let i = i.trim();
    if let Some(c) = i.strip_prefix("geoip:") {
        return (!c.is_empty()).then(|| format!("GEOIP,{c},{target}"));
    }
    let (addr, _) = i.split_once('/').unwrap_or((i, ""));
    let ip: std::net::IpAddr = addr.parse().ok()?;
    let cidr = if i.contains('/') { i.to_string() } else { format!("{i}/{}", if ip.is_ipv4() { 32 } else { 128 }) };
    Some(format!("{},{cidr},{target}", if ip.is_ipv4() { "IP-CIDR" } else { "IP-CIDR6" }))
}

pub fn to_rules_text(p: &Value) -> (String, Vec<String>) {
    let (mut lines, mut skipped) = (Vec::new(), Vec::new());
    for group in order(p) {
        let (sites, ips, target) = match group {
            "block" => ("BlockSites", "BlockIp", "REJECT"),
            "proxy" => ("ProxySites", "ProxyIp", "PROXY"),
            _ => ("DirectSites", "DirectIp", "DIRECT"),
        };
        for d in list(p, sites) {
            match domain_rule(&d, target) {
                Some(l) => lines.push(l),
                None => skipped.push(d),
            }
        }
        for i in list(p, ips) {
            match ip_rule(&i, target) {
                Some(l) => lines.push(l),
                None => skipped.push(i),
            }
        }
    }
    lines.push(format!("MATCH,{}", if global_proxy(p) { "PROXY" } else { "DIRECT" }));
    (lines.join("\n"), skipped)
}

pub fn import_rules(input: &str) -> Result<(String, Vec<String>), String> {
    let p = parse_profile(input)?;
    let (text, skipped) = to_rules_text(&p);
    if text.lines().count() <= 1 {
        return Err("routing profile has no rules".into());
    }
    Ok((text, skipped))
}

pub fn global_proxy(p: &Value) -> bool {
    match field(p, "GlobalProxy") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) if !s.trim().is_empty() => !s.trim().eq_ignore_ascii_case("false"),
        _ => true,
    }
}

pub fn domain_strategy(p: &Value) -> Option<String> {
    Some(text(p, "DomainStrategy")).filter(|s| !s.is_empty()).map(str::to_string)
}

fn server(p: &Value, prefix: &str, local: bool) -> Option<String> {
    let kind = text(p, &format!("{prefix}DNSType")).to_ascii_lowercase();
    let domain = text(p, &format!("{prefix}DNSDomain"));
    let ip = text(p, &format!("{prefix}DNSIP"));
    let sfx = if local { "+local" } else { "" };
    let host = |s: &str| s.split_once("://").map_or(s, |(_, r)| r).to_string();
    match kind.as_str() {
        "doh" if !domain.is_empty() => {
            let url = if domain.contains("://") { domain.to_string() } else { format!("https://{domain}/dns-query") };
            Some(url.replacen("https://", &format!("https{sfx}://"), 1))
        }
        "dot" if !domain.is_empty() || !ip.is_empty() => Some(format!("tls{sfx}://{}", host(if domain.is_empty() { ip } else { domain }))),
        _ if !ip.is_empty() => Some(if local { format!("udp+local://{ip}") } else { ip.to_string() }),
        _ => None,
    }
}

pub fn dns(p: &Value) -> Option<Value> {
    let mut servers = Vec::new();
    let direct = list(p, "DirectSites");
    if !direct.is_empty()
        && let Some(a) = server(p, "Domestic", true)
    {
        servers.push(json!({ "address": a, "domains": direct, "skipFallback": true }));
    }
    if let Some(a) = server(p, "Remote", false) {
        servers.push(json!(a));
    }
    let mut hosts = serde_json::Map::new();
    for prefix in ["Remote", "Domestic"] {
        let domain = text(p, &format!("{prefix}DNSDomain"));
        let ip = text(p, &format!("{prefix}DNSIP"));
        let host = domain.split_once("://").map_or(domain, |(_, r)| r).split(['/', ':']).next().unwrap_or("");
        if !host.is_empty() && host.parse::<std::net::IpAddr>().is_err() && ip.parse::<std::net::IpAddr>().is_ok() {
            hosts.insert(host.to_string(), json!(ip));
        }
    }
    if let Some(h) = field(p, "DnsHosts").and_then(Value::as_object) {
        for (k, v) in h {
            if v.is_string() || v.is_array() {
                hosts.insert(k.clone(), v.clone());
            }
        }
    }
    if servers.is_empty() && hosts.is_empty() {
        return None;
    }
    let mut d = json!({ "servers": servers });
    if !hosts.is_empty() {
        d["hosts"] = Value::Object(hosts);
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "Name": "RU", "GlobalProxy": "true",
            "RemoteDNSType": "DoH", "RemoteDNSDomain": "https://cloudflare-dns.com/dns-query", "RemoteDNSIP": "1.1.1.1",
            "DomesticDNSType": "DoU", "DomesticDNSDomain": "", "DomesticDNSIP": "77.88.8.8",
            "DnsHosts": { "example.org": "10.0.0.1" },
            "DirectSites": ["geosite:category-ru"], "DirectIp": ["geoip:ru", "10.0.0.0/8"],
            "ProxySites": [], "ProxyIp": [], "BlockSites": ["geosite:category-ads-all"], "BlockIp": [],
            "DomainStrategy": "IPIfNonMatch", "FakeDNS": "false"
        })
    }

    #[test]
    fn lowercase_fields_and_route_order() {
        let p = json!({
            "globalproxy": false, "routeorder": "proxy-block-direct",
            "directsites": ["geosite:category-ru"], "directip": ["geoip:ru"],
            "proxysites": ["geosite:tiktok"], "blocksites": ["domain:ads.example"],
            "domesticdnstype": "DoU", "domesticdnsip": "77.88.8.8",
            "remotednstype": "DoH", "remotednsdomain": "https://dns.google/dns-query", "remotednsip": "8.8.8.8",
            "dnshosts": { "dns.google": "8.8.8.8" }, "domainstrategy": "IPIfNonMatch"
        });
        let r = rules(&p, &Tags { direct: "d", proxy: "p", block: "b" });
        let tags: Vec<&str> = r.iter().map(|x| x["outboundTag"].as_str().unwrap()).collect();
        assert_eq!(tags, ["p", "b", "d", "d", "d"]);
        assert_eq!(r[0]["domain"][0], "geosite:tiktok");
        assert!(!global_proxy(&p));
        assert_eq!(domain_strategy(&p).as_deref(), Some("IPIfNonMatch"));
        let d = dns(&p).unwrap();
        assert_eq!(d["servers"][0]["address"], "udp+local://77.88.8.8");
        assert_eq!(d["servers"][1], "https://dns.google/dns-query");
        assert_eq!(d["hosts"]["dns.google"], "8.8.8.8");
    }

    #[test]
    fn imports_link_base64_and_json_into_rules_text() {
        use base64::Engine;
        let json = r#"{"Name":"RU","GlobalProxy":"true","DirectSites":["geosite:category-ru","domain:ya.ru","full:a.example","bad:x","ext:file.dat:tag"],"DirectIp":["geoip:ru","10.0.0.0/8","1.2.3.4","fd00::/8"],"BlockSites":["keyword:ads"]}"#;
        let b = base64::engine::general_purpose::STANDARD.encode(json);
        for input in [
            format!("happ://routing/add/{b}"),
            format!("happ://routing/onadd/{b}"),
            format!("v2raytun://routing/?data={b}"),
            b.clone(),
            json.to_string(),
        ] {
            let (text, skipped) = import_rules(&input).unwrap_or_else(|e| panic!("{input}: {e}"));
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines[0], "DOMAIN-KEYWORD,ads,REJECT", "{input}");
            assert!(lines.contains(&"GEOSITE,category-ru,DIRECT"));
            assert!(lines.contains(&"DOMAIN-SUFFIX,ya.ru,DIRECT"));
            assert!(lines.contains(&"DOMAIN,a.example,DIRECT"));
            assert!(lines.contains(&"GEOIP,ru,DIRECT"));
            assert!(lines.contains(&"IP-CIDR,1.2.3.4/32,DIRECT"));
            assert!(lines.contains(&"IP-CIDR6,fd00::/8,DIRECT"));
            assert_eq!(*lines.last().unwrap(), "MATCH,PROXY");
            assert_eq!(skipped, ["bad:x", "ext:file.dat:tag"]);
        }
        assert!(import_rules("hello").is_err());
        assert!(import_rules(r#"{"Name":"empty"}"#).is_err());
    }

    #[test]
    fn imported_text_translates_through_mihomo() {
        let (text, _) = import_rules(r#"{"GlobalProxy":"false","DirectSites":["geosite:ru"],"BlockIp":["1.1.1.1"]}"#).unwrap();
        let targets = rv_rules::mihomo::Targets { direct: "direct".into(), reject: "block".into(), proxy: "proxy".into(), known: vec![] };
        let tr = rv_rules::mihomo::translate(&text, &targets);
        assert!(tr.warnings.is_empty(), "{:?}", tr.warnings);
        assert_eq!(tr.rules.len(), 3);
    }

    #[test]
    fn converts_rules() {
        let r = rules(&sample(), &Tags { direct: "direct", proxy: "proxy", block: "block" });
        assert_eq!(r.len(), 4);
        assert_eq!(r[0]["outboundTag"], "block");
        assert_eq!(r[1]["domain"][0], "geosite:category-ru");
        assert_eq!(r[2]["ip"][1], "10.0.0.0/8");
        assert_eq!(r[3]["outboundTag"], "proxy");
        let mut off = sample();
        let mut missing = sample();
        missing.as_object_mut().unwrap().remove("GlobalProxy");
        assert_eq!(rules(&missing, &Tags { direct: "d", proxy: "p", block: "b" }).last().unwrap()["outboundTag"], "p");
        off["GlobalProxy"] = json!("false");
        assert_eq!(rules(&off, &Tags { direct: "d", proxy: "p", block: "b" }).last().unwrap()["outboundTag"], "d");
        assert_eq!(domain_strategy(&sample()).as_deref(), Some("IPIfNonMatch"));
    }

    #[test]
    fn converts_dns() {
        let d = dns(&sample()).unwrap();
        assert_eq!(d["servers"][0]["address"], "udp+local://77.88.8.8");
        assert_eq!(d["servers"][1], "https://cloudflare-dns.com/dns-query");
        assert_eq!(d["hosts"]["cloudflare-dns.com"], "1.1.1.1");
        assert_eq!(d["hosts"]["example.org"], "10.0.0.1");
        assert!(crate::dns::parse("udp+local://77.88.8.8", None).is_some());
    }
}
