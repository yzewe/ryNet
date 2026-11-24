#![no_main]

use libfuzzer_sys::fuzz_target;
use rv_rules::{GeoData, Network, RouteCtx, Router};

fuzz_target!(|data: &[u8]| {
    let Ok(routing) = serde_json::from_slice::<serde_json::Value>(data) else { return };
    let mut geo = GeoData::default();
    geo.geosite.insert("CN".into(), vec!["domain:cn".into(), "full:example.cn".into(), "keyword:baidu".into()]);
    geo.geoip.insert("RU".into(), vec![("5.0.0.0".parse().unwrap(), 8), ("2a00::".parse().unwrap(), 12)]);
    let Ok(router) = Router::new(&routing, &geo) else { return };
    let _ = router.warnings();
    let _ = router.needs_domain();
    let _ = router.uses_process();
    for (domain, ip, port, network) in [
        (Some("www.Example.com"), None, 443, Network::Tcp),
        (None, Some("5.1.2.3".parse().unwrap()), 53, Network::Udp),
        (Some("x.example.cn"), Some("2a00::1".parse().unwrap()), 0, Network::Tcp),
    ] {
        let ctx = RouteCtx { domain, ip, port, network, protocol: Some("http"), inbound_tag: Some("tun"), process: Some("org.app") };
        let _ = router.route(&ctx);
        let _ = router.needs_ip_lookup(&ctx);
        let _ = router.route_resolved(&ctx, &["5.5.5.5".parse().unwrap(), "fe80::1".parse().unwrap()]);
        let _ = router.domain_goes_to("www.example.com", "direct");
    }
});
