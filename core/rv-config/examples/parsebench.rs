use std::time::Instant;

fn main() {
    let mut profiles = Vec::new();
    for p in 0..26 {
        let domains: Vec<String> = (0..5700).map(|i| format!("domain:site{i}-{p}.example.ru")).collect();
        let regex: Vec<String> = (0..860).map(|i| format!("regexp:^ad{i}\\..*\\.example\\.com$")).collect();
        let cidr: Vec<String> = (0..1000).map(|i| format!("10.{}.{}.0/24", i / 256, i % 256)).collect();
        profiles.push(serde_json::json!({
            "remarks": format!("🇳🇱 Нидерланды {p}"),
            "outbounds": [
                {"tag": "proxy", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.1", "port": 443, "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none", "flow": "xtls-rprx-vision"}]}]},
                 "streamSettings": {"network": "tcp", "security": "reality", "realitySettings": {"serverName": "www.example.com", "fingerprint": "chrome", "publicKey": "Aw3QJQ1ykQyQjtE5hVCe0xWJy2EI3MzmG6nZ6H8cS2w", "shortId": "ab"}}},
                {"tag": "direct", "protocol": "freedom"}, {"tag": "block", "protocol": "blackhole"}
            ],
            "routing": {"domainStrategy": "IPIfNonMatch", "rules": [
                {"type": "field", "domain": domains, "outboundTag": "direct"},
                {"type": "field", "domain": regex, "outboundTag": "block"},
                {"type": "field", "ip": cidr, "outboundTag": "direct"},
                {"type": "field", "port": "0-65535", "outboundTag": "proxy"}
            ]},
            "dns": {"servers": ["1.1.1.1"]}
        }));
    }
    let text = serde_json::to_string(&profiles).unwrap();
    println!("subscription {:.1} MB", text.len() as f64 / 1048576.0);
    for (name, f) in [("full", rv_config::parse_subscription as fn(&str) -> _), ("dial", dial)] {
        let mut t = Vec::new();
        for _ in 0..7 {
            let t0 = Instant::now();
            let all = f(&text).unwrap();
            let picked: Vec<_> = all.iter().take(26).map(|p| rv_config::find_profile(&all, &p.remarks).unwrap()).collect();
            t.push(t0.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(picked);
        }
        t.sort_by(f64::total_cmp);
        println!("{name:5} parse + 26 profiles: median {:.0} ms (min {:.0})", t[3], t[0]);
    }
}

#[allow(dead_code)]
fn dial(text: &str) -> Result<Vec<rv_config::Profile>, rv_config::Error> {
    #[cfg(not(base))]
    return rv_config::parse_subscription_dial(text);
    #[cfg(base)]
    return rv_config::parse_subscription(text);
}
