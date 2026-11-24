use rv_rules::mihomo::{Targets, parse_rule_set, translate};
use rv_rules::{Decision, GeoData, Network, RouteCtx, Router};
use serde_json::{Value, json};
use std::net::IpAddr;

fn targets() -> Targets {
    Targets { direct: "direct".into(), reject: "block".into(), proxy: "proxy".into(), known: Vec::new() }
}
#[test]
fn common_mappings_and_targets() {
    let t = translate(
        "DOMAIN, a.test , DIRECT\nDOMAIN-SUFFIX,b.test,REJECT\nDOMAIN-KEYWORD,c,PROXY\nDOMAIN-REGEX,^d,GLOBAL\nGEOSITE,cn,unknown\nIP-CIDR,10.0.0.0/8,PROXY,no-resolve\nGEOIP,US,DIRECT\nDST-PORT,80/443,PROXY\nSRC-PORT,12,PROXY\nNETWORK,udp,DIRECT\nMATCH,REJECT\nIP-ASN,123,PROXY",
        &targets(),
    );
    assert_eq!(t.rules[0]["domain"], json!(["full:a.test"]));
    assert_eq!(t.rules[1]["domain"], json!(["domain:b.test"]));
    assert_eq!(t.rules[2]["domain"], json!(["keyword:c"]));
    assert_eq!(t.rules[3]["domain"], json!(["regexp:^d"]));
    assert_eq!(t.rules[4]["domain"], json!(["geosite:cn"]));
    assert_eq!(t.rules[5]["ip"], json!(["10.0.0.0/8"]));
    assert_eq!(t.rules[7]["port"], "80,443");
    assert_eq!(t.rules[8]["sourcePort"], "12");
    assert_eq!(t.rules[9]["network"], "udp");
    assert_eq!(t.rules[10]["outboundTag"], "block");
    assert!(t.warnings.iter().any(|w| w.contains("sourcePort")));
    assert!(t.warnings.iter().any(|w| w.contains("IP-ASN")));
    assert_eq!(t.warnings.iter().filter(|w| w.contains("unknown target")).count(), 1)
}
#[test]
fn nested_rules_bypass_and_rule_set() {
    let t = translate(
        "PROCESS-NAME,com.app,DIRECT\nPROCESS-NAME-REGEX,^foo,PROXY\nAND,((DOMAIN-SUFFIX,a.test),(NETWORK,tcp)),DIRECT\nOR,((DOMAIN,b.test),(DOMAIN,c.test)),PROXY\nRULE-SET,https://example/rules,PROXY\nRULE-SET,u,DIRECT,domain\nAND,((DOMAIN,a),(DOMAIN,b)),PROXY",
        &targets(),
    );
    assert_eq!(t.bypass_apps, vec!["com.app"]);
    assert_eq!(t.rules[0], json!({"type":"field","outboundTag":"direct","processName":["com.app"]}));
    let t = rv_rules::mihomo::Translated { rules: t.rules[1..].to_vec(), ..t };
    assert_eq!(t.rules[0], json!({"type":"field","outboundTag":"direct","domain":["domain:a.test"],"network":"tcp"}));
    assert_eq!(t.rules[1]["domain"], json!(["full:b.test"]));
    assert_eq!(t.rules[2]["domain"], json!(["full:c.test"]));
    assert!(t.rules[3]["domain"][0].as_str().unwrap().starts_with("geosite:ruleset-"));
    assert!(t.rules[4]["ip"][0].as_str().unwrap().starts_with("geoip:ruleset-"));
    assert_eq!(t.rule_sets.len(), 2);
    assert_eq!(t.rule_sets[0].key.len(), 24);
    assert_eq!(t.rule_sets[0].key, t.rule_sets[0].key);
    assert!(t.warnings.iter().any(|w| w.contains("same field")));
    assert!(t.warnings.iter().any(|w| w.contains("PROCESS-NAME-REGEX")))
}

#[test]
fn process_name_proxy_and_block_are_rules() {
    let t = translate("PROCESS-NAME,org.telegram.messenger,PROXY\nPROCESS-NAME,com.ads,REJECT\nMATCH,DIRECT", &targets());
    assert!(t.bypass_apps.is_empty());
    let config = json!({ "rules": t.rules });
    let router = Router::new(&config, &GeoData::default()).unwrap();
    assert!(router.uses_process());
    let ctx = |process| RouteCtx {
        domain: None,
        ip: Some("93.184.216.34".parse::<IpAddr>().unwrap()),
        port: 443,
        network: Network::Tcp,
        protocol: None,
        inbound_tag: None,
        process,
    };
    assert_eq!(router.route(&ctx(Some("org.telegram.messenger"))), Decision::Outbound("proxy".into()));
    assert_eq!(router.route(&ctx(Some("com.ads"))), Decision::Outbound("block".into()));
    assert_eq!(router.route(&ctx(Some("other.app"))), Decision::Outbound("direct".into()));
    assert_eq!(router.route(&ctx(None)), Decision::Outbound("direct".into()));
}

#[test]
fn translated_rules_precede_subscription_and_final_catches_remaining() {
    let translated = translate("DOMAIN,api.vk.com,REJECT\nMATCH,PROXY", &targets());
    let mut config: Value = serde_json::from_str(include_str!("fixtures/tests_routing_fi.json")).unwrap();
    let rules = config["rules"].as_array_mut().unwrap();
    rules.splice(0..0, translated.rules);
    let router = Router::new(&config, &GeoData::default()).unwrap();
    let ctx = |domain| RouteCtx {
        domain,
        ip: Some("93.184.216.34".parse::<IpAddr>().unwrap()),
        port: 443,
        network: Network::Tcp,
        protocol: None,
        inbound_tag: None,
        process: None,
    };
    assert_eq!(router.route(&ctx(Some("api.vk.com"))), Decision::Outbound("block".into()));
    assert_eq!(router.route(&ctx(Some("unlisted.test"))), Decision::Outbound("proxy".into()));
    let mut after = config.clone();
    let rules = after["rules"].as_array_mut().unwrap();
    let final_rule = rules.remove(1);
    rules.push(final_rule);
    let reordered = Router::new(&after, &GeoData::default()).unwrap();
    assert_eq!(reordered.route(&ctx(Some("api.vk.com"))), Decision::Outbound("block".into()));
    assert_eq!(reordered.route(&ctx(Some("unlisted.test"))), Decision::Outbound("proxy".into()));
}
#[test]
fn rule_set_formats_and_detection() {
    let (d, i) = parse_rule_set("+.example.com\n*.wild.test\n.example.org\nplain.test", Some("domain"));
    assert_eq!(d, vec!["domain:example.com", "domain:wild.test", "domain:example.org", "full:plain.test"]);
    assert!(i.is_empty());
    let (d, i) = parse_rule_set("192.0.2.1\n2001:db8::/32", Some("ipcidr"));
    assert!(d.is_empty());
    assert_eq!(i.len(), 2);
    assert_eq!(i[0].1, 32);
    let (d, i) = parse_rule_set("DOMAIN,a.test\nDOMAIN-SUFFIX,b.test\nIP-CIDR,10.0.0.0/8,no-resolve\nMATCH,x", Some("classical"));
    assert_eq!(d, vec!["full:a.test", "domain:b.test"]);
    assert_eq!(i.len(), 1);
    let (d, _) = parse_rule_set("payload:\n  - '+.yaml.test'\n  - plain.test", None);
    assert_eq!(d, vec!["domain:yaml.test", "full:plain.test"]);
    let (d, _) = parse_rule_set("DOMAIN,c.test\nDOMAIN-SUFFIX,d.test", None);
    assert_eq!(d.len(), 2);
    let (d, i) = parse_rule_set("10.2.0.0/16\n192.0.2.1", None);
    assert!(d.is_empty());
    assert_eq!(i.len(), 2)
}
