use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use rv_core::engine::{Engine, Options, Out, RoutingMode};
use rv_core::geo::GeoData;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn engine(config: serde_json::Value, routing: RoutingMode) -> Engine {
    let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
    Engine::new(&p, &GeoData::default(), None, &Options { routing, ..Default::default() }).unwrap()
}

#[test]
fn probe_started_before_a_network_change_does_not_override_the_new_one() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let answer = Rc::new(Cell::new(false));
        let ans = answer.clone();
        tokio::task::spawn_local(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                let ok = ans.get();
                tokio::task::spawn_local(async move {
                    let mut b = vec![0u8; 4096];
                    let mut got = Vec::new();
                    while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = s.read(&mut b).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        got.extend_from_slice(&b[..n]);
                    }
                    if !ok {
                        std::future::pending::<()>().await;
                    }
                    let _ = s.write_all(b"\0\0HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n").await;
                });
            }
        });
        let vless = |tag: &str| {
            json!({"tag": tag, "protocol": "vless", "settings": {"vnext": [{"address": "127.0.0.1", "port": port,
                "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}})
        };
        let e = Rc::new(engine(
            json!({
                "outbounds": [vless("a"), vless("b"), {"tag": "deny", "protocol": "blackhole"}, {"tag": "direct", "protocol": "freedom"}],
                "routing": {"balancers": [{"tag": "auto", "selector": ["a", "b"], "strategy": {"type": "leastLoad"}, "fallbackTag": "deny"}],
                    "rules": [{"network": "tcp,udp", "balancerTag": "auto"}]},
                "observatory": {"probeURL": "http://probe.test/"}
            }),
            RoutingMode::Subscription,
        ));
        let ctx = rv_rules::RouteCtx {
            domain: Some("example.com"), ip: None, port: 443, network: rv_rules::Network::Tcp, protocol: None, inbound_tag: None, process: None,
        };
        let name = |o: Out| match o {
            Out::Proxy(_) => "proxy",
            Out::Direct => "direct",
            Out::Block => "block (fallbackTag deny)",
        };
        let p1 = {
            let e = e.clone();
            tokio::task::spawn_local(async move { e.probe_balancers().await })
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        answer.set(true);
        e.reset_pools();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let fresh = name(e.route(&ctx));
        assert_eq!(fresh, "proxy");
        p1.await.unwrap();
        let stale = name(e.route(&ctx));
        assert_eq!(stale, "proxy", "stale probe results from before reset_pools marked both live candidates dead (fresh for 300 s)");
    });
}
