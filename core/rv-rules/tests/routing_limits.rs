use rv_rules::{GeoData, Router};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Instant;

#[test]
fn repeated_geoip_reference_is_expanded_once() {
    let mut geo = GeoData::default();
    let cidrs = (0..2048u32).map(|i| (IpAddr::V4(Ipv4Addr::from(i << 20)), 12)).collect();
    geo.geoip.insert("xx".into(), cidrs);
    let routing = json!({"rules": [{"type": "field", "ip": vec!["geoip:xx"; 2048], "outboundTag": "direct"}]});
    let t = Instant::now();
    Router::new(&routing, &geo).unwrap();
    assert!(t.elapsed().as_millis() < 1000, "2048 references to a 2048-entry geoip took {:?}", t.elapsed());
}

#[test]
fn repeated_geosite_reference_is_expanded_once() {
    let mut geo = GeoData::default();
    geo.geosite.insert("xx".into(), (0..2048).map(|i| format!("keyword:k{i}")).collect());
    let routing = json!({"rules": [{"type": "field", "domain": vec!["geosite:xx"; 512], "outboundTag": "direct"}]});
    let t = Instant::now();
    Router::new(&routing, &geo).unwrap();
    assert!(t.elapsed().as_millis() < 1000, "512 references to a 2048-entry geosite took {:?}", t.elapsed());
}

#[test]
fn rule_regex_that_expands_past_a_mebibyte_is_refused() {
    let geo = GeoData::default();
    let rule = |r: &str| json!({"rules": [{"type": "field", "domain": [format!("regexp:{r}")], "outboundTag": "direct"}]});
    assert!(Router::new(&rule("(?:.{1000}){100}"), &geo).is_err());
    let many: Vec<String> = (0..300).map(|i| format!("regexp:^.+\\.site{i}\\.example$")).collect();
    Router::new(&json!({"rules": [{"type": "field", "domain": many, "outboundTag": "direct"}]}), &geo).unwrap();
}
