use rv_rules::{GeoData, Network, RouteCtx, Router};
use std::time::Instant;

fn main() {
    let domains: Vec<String> = (0..5700).map(|i| format!("domain:site{i}.example.ru")).collect();
    let regex: Vec<String> = (0..860).map(|i| format!("regexp:^ad{i}\\..*\\.example\\.com$")).collect();
    let cidr: Vec<String> = (0..1000).map(|i| format!("10.{}.{}.0/24", i / 256, i % 256)).collect();
    let routing = serde_json::json!({"domainStrategy": "IPIfNonMatch", "rules": [
        {"type": "field", "domain": domains, "outboundTag": "direct"},
        {"type": "field", "domain": regex, "outboundTag": "block"},
        {"type": "field", "ip": cidr, "outboundTag": "direct"},
        {"type": "field", "port": "0-65535", "outboundTag": "proxy"}
    ]});
    let r = Router::new(&routing, &GeoData::default()).unwrap();
    let hosts: Vec<String> =
        (0..10_000).map(|i| if i % 4 == 0 { format!("rare{i}.cdn.net") } else { format!("hot{}.app.com", i % 40) }).collect();
    let mut t = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        for h in &hosts {
            let c =
                RouteCtx { domain: Some(h), ip: None, port: 443, network: Network::Tcp, protocol: None, inbound_tag: None, process: None };
            std::hint::black_box(r.needs_ip_lookup(&c));
            std::hint::black_box(r.route(&c));
        }
        t.push(t0.elapsed().as_secs_f64() * 1e6 / hosts.len() as f64);
    }
    t.sort_by(f64::total_cmp);
    println!("route a new connection: median {:.1} µs (min {:.1})", t[2], t[0]);
}
