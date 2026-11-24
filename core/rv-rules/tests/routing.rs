use std::{net::IpAddr, str::FromStr};

use rv_rules::{Decision, GeoData, Network, RouteCtx, Router};
use serde_json::{Value, json};

fn router(file: &str, geo: &GeoData) -> Router {
    let value: Value = serde_json::from_str(file).unwrap();
    Router::new(&value, geo).unwrap()
}
fn ctx<'a>(domain: Option<&'a str>, ip: &str, port: u16, protocol: Option<&'a str>, inbound: Option<&'a str>) -> RouteCtx<'a> {
    RouteCtx { domain, ip: Some(IpAddr::from_str(ip).unwrap()), port, network: Network::Tcp, protocol, inbound_tag: inbound, process: None }
}

#[test]
fn supplied_fi_and_auto_profiles_route_as_expected() {
    let fi = router(include_str!("fixtures/tests_routing_fi.json"), &GeoData::default());
    assert_eq!(fi.route(&ctx(None, "93.184.216.34", 443, Some("bittorrent"), None)), Decision::Outbound("direct".into()));
    assert_eq!(fi.route(&ctx(None, "192.168.1.1", 443, None, None)), Decision::Outbound("direct".into()));
    assert_eq!(fi.route(&ctx(Some("api.vk.com"), "93.184.216.34", 443, None, None)), Decision::Outbound("direct".into()));
    assert_eq!(fi.route(&ctx(Some("x.api.vk.com.evil"), "93.184.216.34", 443, None, None)), Decision::Outbound("direct".into()));
    for d in ["1c.ru", "www.1c.ru"] {
        assert_eq!(fi.route(&ctx(Some(d), "93.184.216.34", 443, None, None)), Decision::Outbound("direct".into()));
    }
    assert_eq!(fi.route(&ctx(Some("1c.ru.com"), "93.184.216.34", 443, None, None)), Decision::Default);
    for port in [5223, 2197] {
        assert_eq!(fi.route(&ctx(None, "93.184.216.34", port, None, None)), Decision::Outbound("direct".into()));
    }
    assert_eq!(fi.route(&ctx(None, "17.1.2.3", 443, None, None)), Decision::Outbound("direct".into()));
    assert_eq!(fi.route(&ctx(Some("example.com"), "93.184.216.34", 443, None, None)), Decision::Default);

    let auto = router(include_str!("fixtures/tests_routing_auto.json"), &GeoData::default());
    assert_eq!(auto.route(&ctx(None, "93.184.216.34", 443, None, None)), Decision::Balancer("main-auto".into()));
    assert_eq!(auto.route(&ctx(None, "93.184.216.34", 443, None, Some("fallback-to-reserve"))), Decision::Balancer("reserve-auto".into()));
    assert_eq!(
        auto.route(&ctx(None, "93.184.216.34", 443, None, Some("fallback-to-whitelist"))),
        Decision::Balancer("whitelist-auto".into())
    );
}

#[test]
fn geo_expansion_missing_data_and_matcher_kinds() {
    let mut geo = GeoData::default();
    geo.geoip.insert("RU".into(), vec![(IpAddr::from_str("77.88.0.0").unwrap(), 18)]);
    geo.geosite.insert("category-ru".into(), vec!["domain:yandex.ru".into(), "full:ya.ru".into(), "keyword:sberbank".into()]);
    let config = json!({"rules":[{"type":"field","ip":["geoip:ru"],"outboundTag":"direct"},{"type":"field","domain":["geosite:category-ru"],"outboundTag":"direct"},{"type":"field","ip":["2001:db8::/32"],"outboundTag":"v6"}]});
    let r = Router::new(&config, &geo).unwrap();
    assert_eq!(r.route(&ctx(None, "77.88.8.8", 443, None, None)), Decision::Outbound("direct".into()));
    for d in ["yandex.ru", "a.yandex.ru", "ya.ru", "sberbank-online.ru"] {
        assert_eq!(r.route(&ctx(Some(d), "93.184.216.34", 443, None, None)), Decision::Outbound("direct".into()));
    }
    assert_eq!(r.route(&ctx(None, "2001:db8::1", 443, None, None)), Decision::Outbound("v6".into()));
    assert!(r.warnings().is_empty());
    assert_eq!(r.route(&ctx(Some("example.com"), "93.184.216.34", 443, None, None)), Decision::Default);
    let missing = Router::new(&json!({"rules":[{"type":"field","ip":["geoip:ru"],"outboundTag":"direct"}]}), &GeoData::default()).unwrap();
    assert!(!missing.warnings().is_empty());
    assert_eq!(missing.route(&ctx(None, "77.88.8.8", 443, None, None)), Decision::Default);
}

#[test]
fn geo_codes_match_in_any_case() {
    let mut geo = GeoData::default();
    geo.geoip.insert("ru".into(), vec![(IpAddr::from_str("77.88.0.0").unwrap(), 18)]);
    geo.geosite.insert("category-ru".into(), vec!["domain:yandex.ru".into()]);
    let r = Router::new(
        &json!({"rules":[
            {"type":"field","ip":["geoip:RU"],"outboundTag":"ip"},
            {"type":"field","domain":["geosite:CATEGORY-RU"],"outboundTag":"site"}
        ]}),
        &geo,
    )
    .unwrap();
    assert!(r.warnings().is_empty(), "{:?}", r.warnings());
    assert_eq!(r.route(&ctx(None, "77.88.8.8", 443, None, None)), Decision::Outbound("ip".into()));
    assert_eq!(r.route(&ctx(Some("a.yandex.ru"), "93.184.216.34", 443, None, None)), Decision::Outbound("site".into()));
}

#[test]
fn domain_alone_helper_and_suffix_full_semantics() {
    let r = Router::new(
        &json!({"rules":[{"type":"field","domain":["full:exact.test","domain:tree.test","keyword:part"],"outboundTag":"direct"}]}),
        &GeoData::default(),
    )
    .unwrap();
    assert!(r.needs_domain());
    assert!(r.domain_goes_to("exact.test", "direct"));
    assert!(!r.domain_goes_to("x.exact.test", "direct"));
    assert!(r.domain_goes_to("x.tree.test", "direct"));
    assert!(r.domain_goes_to("xpartx.test", "direct"));
}

#[test]
fn array_entries_keep_commas_like_xray() {
    let r = Router::new(
        &json!({"rules": [{"domain": ["regexp:^speed\\.(coe|open)\\.ad\\.[a-z]{2,3}$"], "outboundTag": "block"}]}),
        &GeoData::default(),
    )
    .unwrap();
    assert_eq!(r.route(&ctx(Some("speed.open.ad.xyz"), "1.1.1.1", 443, None, None)), Decision::Outbound("block".into()));
    assert_ne!(r.route(&ctx(Some("speed.open.ad.abcd"), "1.1.1.1", 443, None, None)), Decision::Outbound("block".into()));
    let s =
        Router::new(&json!({"rules": [{"domain": "domain:a.test,domain:b.test", "outboundTag": "direct"}]}), &GeoData::default()).unwrap();
    assert_eq!(s.route(&ctx(Some("b.test"), "1.1.1.1", 443, None, None)), Decision::Outbound("direct".into()));
}

#[test]
fn regex_domains_are_lowercased_and_invalid_patterns_rejected() {
    let r = Router::new(&json!({"rules": [{"domain": ["regexp:^api\\.test$"], "outboundTag": "direct"}]}), &GeoData::default()).unwrap();
    assert!(r.domain_goes_to("API.TEST", "direct"));
    assert!(Router::new(&json!({"rules": [{"domain": ["regexp:["], "outboundTag": "bad"}]}), &GeoData::default()).is_err());
}

#[test]
fn rule_without_type_is_field() {
    let r = Router::new(&json!({"rules":[{"domain":["domain:example.test"],"outboundTag":"direct"}]}), &GeoData::default()).unwrap();
    assert!(r.warnings().is_empty(), "{:?}", r.warnings());
    assert!(r.domain_goes_to("a.example.test", "direct"));
}

#[test]
fn empty_domain_and_ip_lists_do_not_disable_other_conditions() {
    let r = Router::new(
        &serde_json::json!({"rules": [{"domain": [], "ip": [], "network": "tcp", "outboundTag": "direct"}]}),
        &GeoData::default(),
    )
    .unwrap();
    assert_eq!(r.route(&ctx(None, "192.0.2.1", 443, None, None)), Decision::Outbound("direct".into()));
}

#[test]
fn numeric_and_overlapping_ports() {
    let r = router(
        r#"{"rules":[
            {"port":443,"outboundTag":"a"},
            {"port":"1000-9000,8443","outboundTag":"b"},
            {"port":[53,"5000-5001"],"outboundTag":"c"},
            {"port":"junk","outboundTag":"never"},
            {"domain":["full:Exact.example","domain:Suffix.example"],"outboundTag":"d"},
            {"domain":["regexp:^ads\\d+\\.example\\.org$","regexp:(^|\\.)track\\.net$"],"outboundTag":"re"}
        ]}"#,
        &GeoData::default(),
    );
    let at = |port| r.route(&ctx(None, "93.184.216.34", port, None, None));
    assert_eq!(at(443), Decision::Outbound("a".into()));
    assert_eq!(at(80), Decision::Default);
    assert_eq!(at(8443), Decision::Outbound("b".into()));
    assert_eq!(at(1000), Decision::Outbound("b".into()));
    assert_eq!(at(53), Decision::Outbound("c".into()));
    assert_eq!(at(9999), Decision::Default);
    let d = |name| r.route(&ctx(Some(name), "93.184.216.34", 80, None, None));
    assert_eq!(d("exact.EXAMPLE"), Decision::Outbound("d".into()));
    assert_eq!(d("www.exact.example"), Decision::Default);
    assert_eq!(d("a.b.suffix.example"), Decision::Outbound("d".into()));
    assert_eq!(d("suffix.example"), Decision::Outbound("d".into()));
    assert_eq!(d("notsuffix.example"), Decision::Default);
    assert_eq!(d("ads42.example.org"), Decision::Outbound("re".into()));
    assert_eq!(d("x.track.net"), Decision::Outbound("re".into()));
    assert_eq!(d("adsx.example.org"), Decision::Default);
}

#[test]
fn unevaluable_conditions_do_not_widen_rules() {
    let r = router(
        r#"{"rules":[
            {"user":["a@b"],"outboundTag":"never"},
            {"sourceIP":["10.0.0.0/8"],"outboundTag":"never"},
            {"localOS":["linux"],"outboundTag":"never"},
            {"localOS":["android"],"port":"444","outboundTag":"os"}
        ]}"#,
        &GeoData::default(),
    );
    assert_eq!(r.route(&ctx(None, "93.184.216.34", 443, None, None)), Decision::Default);
    assert_eq!(r.route(&ctx(None, "93.184.216.34", 444, None, None)), Decision::Outbound("os".into()));
}
