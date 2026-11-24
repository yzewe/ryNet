use std::alloc::{GlobalAlloc, Layout, System};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};

use rv_rules::{Decision, GeoData, Network, RouteCtx, Router};
use serde_json::json;

static LIVE: AtomicIsize = AtomicIsize::new(0);
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;

fn ctx(domain: Option<&str>, ip: Ipv4Addr) -> RouteCtx<'_> {
    RouteCtx { domain, ip: Some(IpAddr::V4(ip)), port: 443, network: Network::Tcp, protocol: None, inbound_tag: None, process: None }
}

#[test]
fn geo_codes_are_shared_between_rules_and_routers() {
    let mut geo = GeoData::default();
    let cidrs = (0..2048u32).map(|i| (IpAddr::V4(Ipv4Addr::from((i * 2) << 20)), 12)).collect();
    geo.geoip.insert("xx".into(), cidrs);
    geo.geosite.insert("yy".into(), (0..2048).map(|i| format!("domain:d{i}.example")).collect());
    let mut rules: Vec<_> = (0..500).map(|i| json!({"ip": ["geoip:xx"], "outboundTag": format!("ip{i}")})).collect();
    rules.extend((0..500).map(|i| json!({"domain": ["geosite:yy"], "outboundTag": format!("site{i}")})));
    let routing = json!({ "rules": rules });

    let before = LIVE.load(Relaxed);
    let a = Router::new(&routing, &geo).unwrap();
    let b = Router::new(&routing, &geo).unwrap();
    let held = LIVE.load(Relaxed) - before;
    assert!(held < 8 << 20, "2 routers × 1000 rules over one geoip and one geosite hold {} MB", held >> 20);

    for r in [&a, &b] {
        assert_eq!(r.route(&ctx(None, Ipv4Addr::new(0, 0, 0, 1))), Decision::Outbound("ip0".into()));
        assert_eq!(r.route(&ctx(None, Ipv4Addr::new(0, 16, 0, 1))), Decision::Default);
        assert_eq!(r.route(&ctx(Some("a.d7.example"), Ipv4Addr::new(0, 16, 0, 1))), Decision::Outbound("site0".into()));
    }
}
