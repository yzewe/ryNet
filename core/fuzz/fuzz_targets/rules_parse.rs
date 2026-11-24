#![no_main]

use libfuzzer_sys::fuzz_target;
use rv_rules::mihomo::{Targets, parse_rule_set, rule_set_key, translate};
use rv_rules::{GeoData, Network, RouteCtx, Router};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let targets = Targets { direct: "direct".into(), reject: "block".into(), proxy: "proxy".into(), known: vec!["proxy-2".into()] };
    let t = translate(text, &targets);
    let mut geo = GeoData::default();
    for behavior in [None, Some("domain"), Some("ipcidr"), Some("classical")] {
        let (domains, ips) = parse_rule_set(text, behavior);
        let key = rule_set_key(behavior.unwrap_or("auto"));
        geo.geosite.insert(key.to_ascii_uppercase(), domains);
        geo.geoip.insert(key.to_ascii_uppercase(), ips);
    }
    for r in &t.rule_sets {
        let (domains, ips) = parse_rule_set(text, r.behavior.as_deref());
        geo.geosite.insert(r.key.to_ascii_uppercase(), domains);
        geo.geoip.insert(r.key.to_ascii_uppercase(), ips);
    }
    let routing = serde_json::json!({ "rules": t.rules });
    if let Ok(router) = Router::new(&routing, &geo) {
        route_some(&router);
    }
});

fn route_some(router: &Router) {
    for (domain, ip, port, network) in [
        (Some("www.example.com"), None, 443, Network::Tcp),
        (None, Some("10.1.2.3".parse().unwrap()), 53, Network::Udp),
        (Some("a.b.test"), Some("2001:db8::1".parse().unwrap()), 80, Network::Tcp),
    ] {
        let ctx = RouteCtx { domain, ip, port, network, protocol: Some("tls"), inbound_tag: None, process: Some("com.app") };
        let _ = router.route(&ctx);
        let _ = router.needs_ip_lookup(&ctx);
        let _ = router.route_resolved(&ctx, &["1.1.1.1".parse().unwrap(), "::1".parse().unwrap()]);
    }
}
