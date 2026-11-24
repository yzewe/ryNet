use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rv_config::{OutboundKind, Profile};
use rv_net::{DialOptions, Protect};
use rv_rules::{Decision, GeoData, Network, RouteCtx, Router};
use rv_vless::Address;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::dns::Upstream;
use crate::{BoxStream, Error, Proxy};

#[derive(Clone)]
pub enum Out {
    Proxy(Rc<Proxy>),
    Direct,
    Block,
}

struct DnsServer {
    definition: Value,
    up: Upstream,
    outbound: Option<String>,
    local: bool,
    domains: Option<Router>,
    skip_fallback: bool,
    final_query: bool,
    timeout: Duration,
    families: (bool, bool),
    expected: Option<DnsFilter>,
    unexpected: Option<DnsFilter>,
}
struct DnsFilter {
    rules: Router,
    prefer: bool,
}
#[derive(Debug)]
struct DnsRcode(u8);
impl std::fmt::Display for DnsRcode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dns rcode {}", self.0)
    }
}
impl std::error::Error for DnsRcode {}
impl DnsFilter {
    fn apply(&self, ips: &mut Vec<IpAddr>, invert: bool) {
        let keep: Vec<_> = ips
            .iter()
            .copied()
            .filter(|ip| {
                let c = RouteCtx {
                    domain: None,
                    ip: Some(*ip),
                    port: 0,
                    network: Network::Udp,
                    protocol: None,
                    inbound_tag: None,
                    process: None,
                };
                (self.rules.route(&c) == Decision::Outbound("dns".into())) != invert
            })
            .collect();
        if !self.prefer || !keep.is_empty() {
            *ips = keep;
        }
    }
}

struct Refused {
    strikes: u8,
    until: Option<Instant>,
    ttl: Duration,
}

struct RefusedHits {
    streak: u32,
    last: Instant,
    logged: Option<Instant>,
    quiet: u32,
}

struct DialFails {
    n: u32,
    since: Instant,
    until: Option<Instant>,
    backoff: Duration,
    probe: Option<Instant>,
}

struct Stalls {
    streak: u32,
    last: Instant,
    next: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DnsHop {
    Direct(SocketAddr),
    Stream(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DnsTarget {
    Servers(Vec<(DnsHop, Duration)>),
    Reply(Vec<u8>),
    Lookup(String),
}

const DEFAULT_DNS: &str = "1.1.1.1:53";
const DNS_TIMEOUT: Duration = Duration::from_secs(4);
const DNS_HEDGE: Duration = Duration::from_millis(800);
const AAAA_GRACE: Duration = Duration::from_millis(100);
const HTTP_403: &[u8] = b"HTTP/1.1 403 Forbidden\r\nCache-Control: max-age=3600, public\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
type UdpFlow = (Option<SocketAddr>, SocketAddr);
const UDP_ROUTE_TTL: Duration = Duration::from_secs(10);
const UDP_OWNER_TTL: Duration = Duration::from_secs(30);
const UDP_OWNERS_MAX: usize = 4096;
const REAL_MAX: usize = 1024;
const DNS_ADDRS_MAX: usize = 32;
const REAL_TTL_MAX: u32 = 600;
const SETUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_quiet(on: bool) -> bool {
    rv_xhttp::transport::set_quiet(on);
    QUIET.swap(on, std::sync::atomic::Ordering::Relaxed) != on
}

pub fn quiet() -> bool {
    QUIET.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Default)]
pub struct Options {
    pub user_rules: String,
    pub routing: RoutingMode,
    pub system_dns: SystemDns,
    pub fake_dns: bool,
    pub ipv4_only: bool,
    pub owner: Option<Owner>,
    pub log_apps: bool,
    pub sub_routing: Option<Value>,
    pub on_failing: Option<Failing>,
    pub dns: Vec<String>,
    pub background: bool,
}

pub type Owner = std::sync::Arc<dyn Fn(bool, SocketAddr, SocketAddr) -> Option<String> + Send + Sync>;

pub type Failing = std::sync::Arc<dyn Fn() + Send + Sync>;

pub const FAILING_STREAK: u32 = 6;
pub const FAILING_SPAN: Duration = Duration::from_secs(8);

pub type SystemDns = std::sync::Arc<std::sync::Mutex<Vec<SocketAddr>>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RoutingMode {
    Global,
    Smart,
    #[default]
    Subscription,
    Custom,
    Direct,
}

impl RoutingMode {
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "global" => Some(Self::Global),
            "smart" => Some(Self::Smart),
            "subscription" => Some(Self::Subscription),
            "custom" => Some(Self::Custom),
            "direct" => Some(Self::Direct),
            _ => None,
        }
    }
}

pub const SMART_RU_DOMAINS: &[&str] = &[
    "geosite:category-ru",
    "domain:ru",
    "domain:su",
    "domain:xn--p1ai",
    "domain:yandex.net",
    "domain:yandex.com",
    "domain:vk.com",
    "domain:userapi.com",
    "domain:mycdn.me",
    "domain:ok.ru",
    "domain:mail.ru",
];

pub const SMART_RU_DNS: &str = "77.88.8.8:53";

pub const TUNNEL_DNS: &str = "1.1.1.1";

const LOCAL_NAMES: &[&str] = &[
    "regexp:^[^.]+$",
    "domain:lan",
    "domain:local",
    "domain:localdomain",
    "domain:home",
    "domain:home.arpa",
    "domain:internal",
    "domain:intranet",
    "domain:corp",
    "domain:private",
    "domain:10.in-addr.arpa",
    "domain:168.192.in-addr.arpa",
    "domain:254.169.in-addr.arpa",
    r"regexp:(^|\.)(1[6-9]|2[0-9]|3[01])\.172\.in-addr\.arpa$",
    "domain:d.f.ip6.arpa",
    "domain:8.e.f.ip6.arpa",
    "domain:9.e.f.ip6.arpa",
    "domain:a.e.f.ip6.arpa",
    "domain:b.e.f.ip6.arpa",
    "domain:my.keenetic.net",
    "domain:tplinkwifi.net",
    "domain:tplinklogin.net",
    "domain:routerlogin.net",
    "domain:router.asus.com",
    "domain:fritz.box",
];

pub fn tunnel_dns(p: &mut rv_config::Profile, opts: &Options, search: &[String]) -> bool {
    if !opts.dns.is_empty() || opts.routing == RoutingMode::Direct {
        return false;
    }
    let own_rules = p.routing.as_ref().and_then(|r| r.get("rules")).and_then(Value::as_array).is_some_and(|r| !r.is_empty());
    let happ =
        (opts.routing == RoutingMode::Subscription && !own_rules).then(|| opts.sub_routing.as_ref().and_then(crate::happ::dns)).flatten();
    let mut dns = match p.dns.clone().or(happ) {
        Some(Value::Object(o)) => o,
        _ => Default::default(),
    };
    if dns.get("servers").and_then(Value::as_array).is_some_and(|s| !s.is_empty()) {
        return false;
    }
    let local: Vec<String> = LOCAL_NAMES.iter().map(|d| d.to_string()).chain(search.iter().map(|d| format!("domain:{d}"))).collect();
    let mut servers = vec![serde_json::json!({ "address": "localhost", "domains": local, "skipFallback": true })];
    if opts.routing == RoutingMode::Smart {
        servers.push(serde_json::json!({ "address": SMART_RU_DNS, "domains": SMART_RU_DOMAINS, "skipFallback": true }));
    }
    servers.push(serde_json::json!(TUNNEL_DNS));
    dns.insert("servers".into(), serde_json::json!(servers));
    p.dns = Some(Value::Object(dns));
    true
}

const SMART_DIRECT: &str = "rv-smart-direct";
const SMART_DIRECT_TRY: Duration = Duration::from_secs(5);
const SMART_DIRECT_TTL: Duration = Duration::from_secs(600);
const SMART_DIRECT_SILENT: Duration = Duration::from_secs(6);
const REFUSED_STRIKES: u8 = 2;
const REFUSED_TTL: Duration = Duration::from_secs(30);
const REFUSED_TTL_MAX: Duration = Duration::from_secs(600);
const PROXY_ALIVE: Duration = Duration::from_secs(60);
const REFUSED_HOLD: Duration = Duration::from_secs(1);
const REFUSED_HOLD_MAX: Duration = Duration::from_secs(3);
const REFUSED_STREAK_GAP: Duration = Duration::from_secs(10);
const REFUSED_HELD_MAX: usize = 128;
const REFUSED_LOG_EVERY: Duration = Duration::from_secs(30);
const EMPTY_FREE: u32 = 3;
const EMPTY_SHORT: Duration = Duration::from_secs(5);
const DIAL_FAILS: u32 = 5;
const DIAL_FAIL_SPAN: Duration = Duration::from_secs(2);
const DIAL_BACKOFF: Duration = Duration::from_secs(2);
const DIAL_BACKOFF_MAX: Duration = Duration::from_secs(30);
const DIAL_HELD: &str = "server dials keep failing, retry held";
const STALL_QUIET: Duration = Duration::from_secs(5);
const STALL_FREE: u32 = 5;
const STALL_GAP: Duration = Duration::from_secs(60);
const STALL_DIAL: Duration = Duration::from_secs(1);
const STALL_BURST: Duration = Duration::from_secs(2);
const STALL_HELD: &str = "app connections get no reply via the server, retry held";
const FAIL_LINES_FREE: u32 = 3;

const LAN_RANGES: [&str; 9] =
    ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16", "127.0.0.0/8", "224.0.0.0/4", "fc00::/7", "fe80::/10", "ff00::/8"];

fn smart_rules(direct: &str, proxy: &str) -> Vec<Value> {
    serde_json::from_value(serde_json::json!([
        { "type": "field", "protocol": ["bittorrent"], "outboundTag": direct },
        { "type": "field", "ip": ["geoip:private"], "outboundTag": direct },
        { "type": "field", "domain": SMART_RU_DOMAINS, "outboundTag": direct },
        { "type": "field", "ip": ["geoip:ru"], "outboundTag": direct },
        { "type": "field", "network": "tcp,udp", "outboundTag": proxy }
    ]))
    .unwrap_or_default()
}

pub fn smart_rules_json() -> Value {
    serde_json::json!({
        "rules": smart_rules("direct", "proxy"),
        "private": LAN_RANGES,
        "dns": SMART_RU_DNS,
    })
}

fn bypassable(ip: IpAddr, len: u8, servers: &[IpAddr]) -> bool {
    len >= if ip.is_ipv4() { 8 } else { 16 }
        && !ip.is_loopback()
        && !crate::dns::fake::overlaps(ip, len)
        && !servers.iter().any(|&s| in_net(ip, len, s))
}

fn in_net(net: IpAddr, len: u8, a: IpAddr) -> bool {
    match (net, a) {
        (IpAddr::V4(n), IpAddr::V4(a)) => {
            let m = u32::MAX.checked_shl(32 - u32::from(len.min(32))).unwrap_or(0);
            u32::from(n) & m == u32::from(a) & m
        }
        (IpAddr::V6(n), IpAddr::V6(a)) => {
            let m = u128::MAX.checked_shl(128 - u32::from(len.min(128))).unwrap_or(0);
            u128::from(n) & m == u128::from(a) & m
        }
        _ => false,
    }
}

fn direct_blocked(e: &io::Error) -> bool {
    use io::ErrorKind::*;
    matches!(e.kind(), TimedOut | ConnectionRefused | ConnectionReset | ConnectionAborted | HostUnreachable | NetworkUnreachable | NotFound)
}

fn probe_every(observatory: Option<&Value>) -> Duration {
    let v = observatory.and_then(|o| o.get("pingConfig").and_then(|p| p.get("interval")).or_else(|| o.get("probeInterval")));
    let secs = v.map(crate::balance::duration).filter(|s| *s > 0.0).unwrap_or(300.0);
    Duration::from_secs_f64(secs.clamp(30.0, 900.0))
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.balancer_wake.notify_one();
    }
}

async fn balancer_due(used: &tokio::sync::Notify, every: Duration, alive: impl Fn() -> bool) -> bool {
    used.notified().await;
    if !alive() {
        return false;
    }
    tokio::time::sleep(jittered(every, crate::balance::random(301))).await;
    true
}

fn jittered(every: Duration, permille_off: usize) -> Duration {
    every.mul_f64((850 + permille_off.min(300)) as f64 / 1000.0).max(Duration::from_millis(1))
}

#[derive(Default)]
struct Trace {
    path: String,
    balancer: Option<String>,
    last: Option<String>,
}

pub struct Engine {
    router: Router,
    user_router: Router,
    outs: HashMap<String, Out>,
    default: Out,
    known: std::collections::HashSet<String>,
    balancers: RefCell<HashMap<String, Out>>,
    strategies: HashMap<String, crate::balance::Strategy>,
    choices: RefCell<HashMap<String, Vec<(String, Out)>>>,
    round_robin: RefCell<HashMap<String, usize>>,
    health: RefCell<HashMap<String, crate::balance::Health>>,
    health_gen: std::cell::Cell<u64>,
    probe_url: crate::ping::ProbeUrl,
    probe_every: Duration,
    background: bool,
    balancer_used: std::cell::Cell<bool>,
    balancer_wake: Rc<tokio::sync::Notify>,
    candidates: HashMap<String, Vec<(String, Rc<Proxy>)>>,
    fallbacks: HashMap<String, String>,
    loopbacks: HashMap<String, String>,
    no_udp443: std::collections::HashSet<*const Proxy>,
    dns: Vec<DnsServer>,
    default_tag: String,
    http_blocks: std::collections::HashSet<String>,
    dns_reject_non_ip: bool,
    dns_block_types: Vec<u16>,
    no_fallback: bool,
    no_fallback_if_match: bool,
    hosts: Vec<(Router, Hosted)>,
    no_cache: bool,
    dns_families: (bool, bool),
    ipv4_only: bool,
    system_dns: SystemDns,
    fake_all: bool,
    real: RefCell<HashMap<String, (Vec<IpAddr>, Instant)>>,
    protect: Option<Protect>,
    direct_dial: DialOptions,
    owner: Option<Owner>,
    owner_routes: bool,
    udp_owners: RefCell<HashMap<SocketAddr, (Option<String>, Instant)>>,
    udp_owners_sweep: std::cell::Cell<usize>,
    udp_routes: RefCell<HashMap<UdpFlow, (Option<u64>, Instant)>>,
    conn_idle: Duration,
    smart_fallback: Option<Out>,
    direct_failed: RefCell<HashMap<String, Instant>>,
    refused: RefCell<HashMap<String, Refused>>,
    refused_hits: RefCell<HashMap<String, RefusedHits>>,
    empty_hits: RefCell<HashMap<String, RefusedHits>>,
    refused_held: std::cell::Cell<usize>,
    dial_fails: RefCell<HashMap<*const Proxy, DialFails>>,
    stalls: RefCell<HashMap<String, Stalls>>,
    fail_lines: RefCell<HashMap<String, RefusedHits>>,
    own_name: Option<String>,
    servers: Vec<(String, u16)>,
    proxy_alive: std::cell::Cell<Option<Instant>>,
    on_failing: Option<Failing>,
    fail_streak: std::cell::Cell<u32>,
    fail_since: std::cell::Cell<Option<Instant>>,
    fail_signaled: std::cell::Cell<u32>,
    pub warnings: Vec<String>,
    pub routing_summary: String,
    pub routing_view: Value,
    direct_nets: Vec<(IpAddr, u8)>,
}

type PendingDns<'a> = Vec<(usize, std::pin::Pin<Box<dyn std::future::Future<Output = Option<Vec<u8>>> + 'a>>)>;

impl Engine {
    pub fn new(profile: &Profile, geo: &GeoData, protect: Option<Protect>, opts: &Options) -> Result<Engine, Error> {
        Self::build(profile, geo, protect, opts, false)
    }

    pub fn inspect(profile: &Profile, geo: &GeoData, opts: &Options) -> Result<Value, Error> {
        Self::build(profile, geo, None, opts, true).map(|mut e| std::mem::take(&mut e.routing_view))
    }

    fn build(profile: &Profile, geo: &GeoData, protect: Option<Protect>, opts: &Options, inspect: bool) -> Result<Engine, Error> {
        let only_v4 = opts.ipv4_only.then(|| {
            let mut p = profile.clone();
            for ob in &mut p.outbounds {
                ob.stream.sockopt.family = rv_config::Family::V4 { force: true };
            }
            p
        });
        let profile = only_v4.as_ref().unwrap_or(profile);
        let mut warnings = Vec::new();
        let _regexes = rv_rules::RegexBudget::new(REGEX_BUDGET);
        rv_net::set_own_fake(|ip| crate::dns::fake::name_for(ip).is_some());
        let direct_tag = profile
            .outbounds
            .iter()
            .find(|o| matches!(&o.kind, OutboundKind::Freedom(f) if f.fragment.is_none()))
            .map(|o| o.tag.clone())
            .unwrap_or_else(|| "direct".into());
        let block_tag = profile
            .outbounds
            .iter()
            .find(|o| matches!(o.kind, OutboundKind::Blackhole { .. }))
            .map(|o| o.tag.clone())
            .unwrap_or_else(|| "block".into());
        let proxy_tag = profile.main_outbound().map(|o| o.tag.clone()).unwrap_or_else(|| "proxy".into());
        let targets = rv_rules::mihomo::Targets {
            direct: direct_tag.clone(),
            reject: block_tag.clone(),
            proxy: proxy_tag.clone(),
            known: profile.outbounds.iter().filter(|o| o.tag.starts_with("rynet-app")).map(|o| o.tag.clone()).collect(),
        };
        let own = opts.user_rules.as_str();
        let user = rv_rules::mihomo::translate(own, &targets);
        let user_domain_targets: std::collections::HashSet<String> = user
            .rules
            .iter()
            .filter(|r| r.get("domain").is_some())
            .filter_map(|r| r.get("outboundTag").and_then(Value::as_str).map(str::to_owned))
            .collect();
        let user_router = Router::new(&serde_json::json!({ "rules": user.rules }), geo).map_err(|e| Error::Config(e.to_string()))?;
        warnings.extend(user.warnings.iter().map(|w| format!("свои правила: {w}")));
        let mut routing = profile.routing.clone().unwrap_or_else(|| serde_json::json!({ "rules": [] }));
        let own_rules = routing.get("rules").and_then(Value::as_array).is_some_and(|r| !r.is_empty());
        let happ = opts.sub_routing.as_ref().filter(|_| opts.routing == RoutingMode::Subscription && !own_rules);
        if let Some(ds) = happ.and_then(crate::happ::domain_strategy)
            && let Some(obj) = routing.as_object_mut()
        {
            obj.insert("domainStrategy".into(), Value::String(ds));
        }
        let sub_rules: Vec<Value> = match opts.routing {
            RoutingMode::Subscription if happ.is_some() => crate::happ::rules(
                happ.unwrap_or(&Value::Null),
                &crate::happ::Tags { direct: &direct_tag, proxy: &proxy_tag, block: &block_tag },
            ),
            RoutingMode::Subscription => routing.get("rules").and_then(Value::as_array).cloned().unwrap_or_default(),
            RoutingMode::Smart => smart_rules(SMART_DIRECT, &proxy_tag),
            RoutingMode::Global | RoutingMode::Custom => {
                vec![serde_json::json!({ "type": "field", "network": "tcp,udp", "outboundTag": proxy_tag })]
            }
            RoutingMode::Direct => vec![serde_json::json!({ "type": "field", "network": "tcp,udp", "outboundTag": direct_tag })],
        };
        let source = match opts.routing {
            RoutingMode::Subscription if happ.is_some() => {
                format!("happ global_proxy={}", crate::happ::global_proxy(happ.unwrap_or(&Value::Null)))
            }
            RoutingMode::Subscription if own_rules => "config".into(),
            RoutingMode::Subscription => "none".into(),
            RoutingMode::Smart => "smart".into(),
            RoutingMode::Global | RoutingMode::Custom => "global".into(),
            RoutingMode::Direct => "direct".into(),
        };
        let last = sub_rules
            .last()
            .and_then(|r| r.get("outboundTag").or_else(|| r.get("balancerTag")))
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_string();
        let lan: Vec<Value> = match opts.routing {
            RoutingMode::Smart => Vec::new(),
            _ => vec![serde_json::json!({ "type": "field", "ip": LAN_RANGES, "outboundTag": direct_tag })],
        };
        let rule_count = user.rules.len() + sub_rules.len();
        let built_in = match opts.routing {
            RoutingMode::Smart => "smart",
            RoutingMode::Subscription => "subscription",
            RoutingMode::Direct => "direct",
            _ => "global",
        };
        let view_rules: Vec<Value> = if inspect {
            user.rules
                .iter()
                .map(|r| serde_json::json!({"source":"user","rule":r}))
                .chain(lan.iter().map(|r| serde_json::json!({"source":"local","rule":r})))
                .chain(sub_rules.iter().map(|r| serde_json::json!({"source":built_in,"rule":r})))
                .collect()
        } else {
            Vec::new()
        };
        let rules: Vec<Value> = user.rules.into_iter().chain(lan).chain(sub_rules).collect();
        if let Some(obj) = routing.as_object_mut() {
            obj.insert("rules".into(), Value::Array(rules));
        }
        let router = Router::new(&routing, geo).map_err(|e| Error::Config(e.to_string()))?;
        warnings.extend(router.warnings().iter().cloned());

        let mut outs = HashMap::new();
        let mut default = None;
        let mut default_tag = "-";
        let mut broken: Vec<String> = Vec::new();
        for ob in &profile.outbounds {
            let out = match &ob.kind {
                k if k.is_proxy() => match Proxy::build(profile, &ob.tag, protect.clone()) {
                    Ok(p) => Some(Out::Proxy(Rc::new(p))),
                    Err(e) => {
                        warnings.push(format!("outbound {}: {e}", ob.tag));
                        broken.push(ob.tag.clone());
                        if default.is_none() {
                            default = Some(Out::Block);
                        }
                        None
                    }
                },
                OutboundKind::Freedom(f) if f.fragment.is_none() => Some(Out::Direct),
                OutboundKind::Blackhole { .. } => Some(Out::Block),
                _ => None,
            };
            if let Some(out) = out {
                if default.is_none() {
                    default = Some(out.clone());
                    default_tag = &ob.tag;
                }
                outs.insert(ob.tag.clone(), out);
            }
        }
        let default = default.ok_or_else(|| Error::Config("no usable outbound in profile".into()))?;
        let routing_summary = format!("source={source} rules={rule_count} last={last} default={default_tag} proxy={proxy_tag}");
        for t in broken {
            outs.entry(t).or_insert(Out::Block);
        }
        outs.entry(direct_tag).or_insert(Out::Direct);
        outs.entry(block_tag).or_insert(Out::Block);
        let smart_fallback = match outs.get(&proxy_tag) {
            Some(p @ Out::Proxy(_)) if opts.routing == RoutingMode::Smart => Some(p.clone()),
            _ => None,
        };
        if opts.routing == RoutingMode::Smart {
            outs.insert(SMART_DIRECT.into(), Out::Direct);
        }
        let servers: Vec<IpAddr> =
            profile.outbounds.iter().filter_map(|o| o.kind.server()?.0.trim_matches(['[', ']']).parse().ok()).collect();
        let direct_nets = router
            .direct_nets(|t| matches!(outs.get(t), Some(Out::Direct)))
            .into_iter()
            .filter(|&(ip, len)| (!opts.ipv4_only || ip.is_ipv4()) && bypassable(ip, len, &servers))
            .collect();

        let mut balancers = HashMap::new();
        let mut candidates = HashMap::new();
        let mut fallbacks = HashMap::new();
        let mut strategies = HashMap::new();
        let mut choices = HashMap::new();
        if let Some(bs) = profile.routing.as_ref().and_then(|r| r.get("balancers")).and_then(Value::as_array) {
            for b in bs {
                let tag = b.get("tag").and_then(Value::as_str).unwrap_or_default();
                if let Some(f) = b.get("fallbackTag").and_then(Value::as_str).filter(|f| !f.is_empty()) {
                    fallbacks.insert(tag.to_string(), f.to_string());
                }
                let selectors: Vec<&str> =
                    b.get("selector").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                let strategy = crate::balance::Strategy::parse(&b["strategy"]).map_err(Error::Config)?;
                if strategy.measured() {
                    tracing::info!(
                        "balancer {tag}: замеры при старте, при трафике и раз в {:?} (interval из observatory), пока идёт трафик",
                        probe_every(profile.observatory.as_ref())
                    );
                }
                let all: Vec<_> = profile
                    .outbounds
                    .iter()
                    .filter(|o| selectors.iter().any(|s| o.tag.starts_with(s)))
                    .filter_map(|o| outs.get(&o.tag).map(|out| (o.tag.clone(), out.clone())))
                    .collect();
                if !strategy.measured() {
                    choices.insert(tag.to_string(), all);
                }
                strategies.insert(tag.to_string(), strategy);
                let list: Vec<(String, Rc<Proxy>)> = profile
                    .outbounds
                    .iter()
                    .filter(|o| selectors.iter().any(|s| o.tag.starts_with(s)))
                    .filter_map(|o| match outs.get(&o.tag) {
                        Some(Out::Proxy(p)) => Some((o.tag.clone(), p.clone())),
                        _ => None,
                    })
                    .collect();
                if let Some((_, first)) = list.first() {
                    balancers.insert(tag.to_string(), Out::Proxy(first.clone()));
                }
                if !list.is_empty() {
                    candidates.insert(tag.to_string(), list);
                }
            }
        }

        let happ_dns = if profile.dns.is_none() { happ.and_then(crate::happ::dns) } else { None };
        let dns_json = profile.dns.as_ref().or(happ_dns.as_ref());
        let mut dns = parse_dns(dns_json, geo, &mut warnings);
        let hosts = parse_hosts(dns_json, geo, &mut warnings);
        if !opts.dns.is_empty() {
            let own = parse_dns(Some(&serde_json::json!({ "servers": opts.dns })), geo, &mut warnings);
            if !own.is_empty() {
                dns = own;
            }
        }
        if opts.ipv4_only {
            dns.retain(|s| !s.up.endpoint().0.parse::<IpAddr>().is_ok_and(|ip| ip.is_ipv6()));
            for s in &mut dns {
                s.families.1 = false;
            }
        }
        if dns.is_empty() {
            dns.push(DnsServer {
                definition: serde_json::json!("localhost"),
                outbound: None,
                up: Upstream::System,
                local: true,
                domains: None,
                skip_fallback: false,
                final_query: false,
                timeout: DNS_TIMEOUT,
                families: (true, true),
                expected: None,
                unexpected: None,
            });
        }
        if opts.routing == RoutingMode::Smart && !dns.iter().any(|s| s.domains.is_some()) {
            let rule = serde_json::json!({ "rules": [
                { "type": "field", "domain": SMART_RU_DOMAINS, "outboundTag": "dns" }
            ] });
            if let (Ok(addr), Ok(domains)) = (SMART_RU_DNS.parse(), Router::new(&rule, geo)) {
                dns.insert(
                    0,
                    DnsServer {
                        definition: serde_json::json!({"address":SMART_RU_DNS,"domains":SMART_RU_DOMAINS,"skipFallback":true}),
                        outbound: None,
                        up: Upstream::Udp(addr),
                        local: false,
                        domains: Some(domains),
                        skip_fallback: true,
                        final_query: false,
                        timeout: DNS_TIMEOUT,
                        families: (true, true),
                        expected: None,
                        unexpected: None,
                    },
                );
            }
        }
        for ob in profile.outbounds.iter().filter(|ob| ob.kind.is_proxy() && user_domain_targets.contains(&ob.tag)) {
            dns.push(DnsServer {
                definition: serde_json::json!({"address":"100.100.100.100","outboundTag":ob.tag,"finalQuery":true,"skipFallback":true}),
                up: Upstream::Udp("100.100.100.100:53".parse().unwrap()),
                outbound: Some(ob.tag.clone()),
                local: false,
                domains: None,
                skip_fallback: true,
                final_query: true,
                timeout: DNS_TIMEOUT,
                families: (true, true),
                expected: None,
                unexpected: None,
            });
        }
        let direct_dial = DialOptions {
            family: if opts.ipv4_only { rv_net::Family::V4 { force: true } } else { rv_net::Family::Any },
            tcp_no_delay: true,
            protect: protect.clone(),
            ..Default::default()
        };
        let no_udp443 = profile
            .outbounds
            .iter()
            .filter(|o| o.reject_udp443)
            .filter_map(|o| match outs.get(&o.tag) {
                Some(Out::Proxy(p)) => Some(Rc::as_ptr(p)),
                _ => None,
            })
            .collect();
        let dns_flag = |k: &str| profile.dns.as_ref().and_then(|d| d.get(k)).and_then(Value::as_bool).unwrap_or(false);
        let routing_view = if inspect {
            serde_json::json!({
                "version":1,
                "mode": match opts.routing { RoutingMode::Smart => "smart", RoutingMode::Subscription => "subscription", RoutingMode::Global => "global", RoutingMode::Custom => "custom", RoutingMode::Direct => "direct" },
                "source": source.split(' ').next().unwrap_or("none"),
                "rules":view_rules,
                "domainStrategy":routing.get("domainStrategy").and_then(Value::as_str).unwrap_or("AsIs"),
                "defaultRoute":default_tag,
                "balancers":profile.routing.as_ref().and_then(|r| r.get("balancers")).cloned().unwrap_or_else(|| serde_json::json!([])),
                "outbounds":profile.outbounds.iter().map(|o| serde_json::json!({"tag":o.tag,"kind":crate::describe::outbound_kind(o)})).collect::<Vec<_>>(),
                "ipv4Only":opts.ipv4_only,
                "warnings":warnings,
                "smartDirect":(opts.routing == RoutingMode::Smart).then(|| serde_json::json!({"tag":SMART_DIRECT,"fallbackTag":proxy_tag,"retryAfterSeconds":SMART_DIRECT_TTL.as_secs(),"connectTimeoutMs":SMART_DIRECT_TRY.as_millis()})),
                "dns": {"policy":dns_json, "override":opts.dns,
                    "servers":dns.iter().map(|s| serde_json::json!({"definition":s.definition,"address":s.up.to_string(), "outboundTag":s.outbound, "local":s.local,
                        "skipFallback":s.skip_fallback,"finalQuery":s.final_query,"ipv4":s.families.0,"ipv6":s.families.1,"timeoutMs":s.timeout.as_millis()})).collect::<Vec<_>>()}
            })
        } else {
            Value::Null
        };
        let owner_routes = router.uses_process();
        let owner = if owner_routes || opts.log_apps { opts.owner.clone() } else { None };
        let own_name = owner.as_ref().and_then(|_| std::env::current_exe().ok()?.file_name()?.to_str().map(str::to_string));
        Ok(Engine {
            router,
            user_router,
            known: profile.outbounds.iter().map(|o| o.tag.clone()).collect(),
            outs,
            default,
            balancers: RefCell::new(balancers),
            strategies,
            choices: RefCell::new(choices),
            round_robin: RefCell::new(HashMap::new()),
            health: RefCell::new(HashMap::new()),
            health_gen: std::cell::Cell::new(0),
            probe_every: probe_every(profile.observatory.as_ref()),
            background: opts.background,
            balancer_used: std::cell::Cell::new(false),
            balancer_wake: Rc::new(tokio::sync::Notify::new()),
            probe_url: {
                let o = profile.observatory.as_ref();
                let burst = o.is_some_and(|o| o.get("pingConfig").is_some());
                let url = if burst {
                    o.and_then(|o| o.get("pingConfig")?.get("destination"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("https://connectivitycheck.gstatic.com/generate_204")
                } else {
                    o.and_then(|o| o.get("probeURL"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("https://www.google.com/generate_204")
                };
                crate::ping::ProbeUrl::parse(url).ok_or_else(|| Error::Config("observatory: invalid probe URL".into()))?
            },
            candidates,
            fallbacks,
            no_udp443,
            loopbacks: profile
                .outbounds
                .iter()
                .filter_map(|o| match &o.kind {
                    OutboundKind::Loopback { inbound_tag } => Some((o.tag.clone(), inbound_tag.clone())),
                    _ => None,
                })
                .collect(),
            dns,
            default_tag: profile.outbounds.first().map(|o| o.tag.clone()).unwrap_or_default(),
            http_blocks: profile
                .outbounds
                .iter()
                .filter(|o| matches!(o.kind, OutboundKind::Blackhole { http: true }))
                .map(|o| o.tag.clone())
                .collect(),
            dns_reject_non_ip: profile.outbounds.iter().any(|o| matches!(o.kind, OutboundKind::Dns { reject_non_ip: true, .. })),
            dns_block_types: profile
                .outbounds
                .iter()
                .flat_map(|o| match &o.kind {
                    OutboundKind::Dns { block_types, .. } => block_types.clone(),
                    _ => Vec::new(),
                })
                .collect(),
            no_fallback: dns_flag("disableFallback"),
            no_fallback_if_match: dns_flag("disableFallbackIfMatch"),
            hosts,
            no_cache: profile.dns.as_ref().and_then(|d| d.get("disableCache")).and_then(Value::as_bool).unwrap_or(false),
            dns_families: if opts.ipv4_only { (true, false) } else { dns_families(dns_json) },
            ipv4_only: opts.ipv4_only,
            system_dns: opts.system_dns.clone(),
            fake_all: opts.fake_dns,
            real: RefCell::new(HashMap::new()),
            protect,
            direct_dial,
            owner,
            owner_routes,
            udp_owners: RefCell::new(HashMap::new()),
            udp_owners_sweep: std::cell::Cell::new(256),
            udp_routes: RefCell::new(HashMap::new()),
            conn_idle: profile.conn_idle.map_or(crate::pipe::CONN_IDLE, Duration::from_secs),
            smart_fallback,
            direct_failed: RefCell::new(HashMap::new()),
            refused: RefCell::new(HashMap::new()),
            refused_hits: RefCell::new(HashMap::new()),
            empty_hits: RefCell::new(HashMap::new()),
            refused_held: std::cell::Cell::new(0),
            dial_fails: RefCell::new(HashMap::new()),
            stalls: RefCell::new(HashMap::new()),
            fail_lines: RefCell::new(HashMap::new()),
            own_name,
            servers: profile.outbounds.iter().filter_map(|o| o.kind.server()).map(|(a, p)| (a.to_string(), p)).collect(),
            proxy_alive: std::cell::Cell::new(None),
            on_failing: opts.on_failing.clone(),
            fail_streak: std::cell::Cell::new(0),
            fail_since: std::cell::Cell::new(None),
            fail_signaled: std::cell::Cell::new(0),
            warnings,
            routing_summary,
            routing_view,
            direct_nets,
        })
    }

    pub fn route(&self, ctx: &RouteCtx<'_>) -> Out {
        self.resolve(ctx, None)
    }

    fn route_tagged(&self, ctx: &RouteCtx<'_>) -> (Out, Trace) {
        let mut t = Trace::default();
        let out = self.resolve(ctx, Some(&mut t));
        (out, t)
    }

    fn resolve(&self, ctx: &RouteCtx<'_>, mut trace: Option<&mut Trace>) -> Out {
        self.resolve_with_ips(ctx, &[], trace.take())
    }

    fn resolve_with_ips(&self, ctx: &RouteCtx<'_>, ips: &[IpAddr], mut trace: Option<&mut Trace>) -> Out {
        if self.ipv4_only && ctx.ip.is_some_and(|ip| ip.is_ipv6()) {
            return Out::Block;
        }
        use std::fmt::Write;
        let note = |t: &mut Option<&mut Trace>, args: std::fmt::Arguments| {
            if let Some(t) = t.as_deref_mut() {
                let _ = t.path.write_fmt(args);
            }
        };
        let finish = |t: &mut Option<&mut Trace>, balancer: Option<&str>, last: Option<&str>| {
            if let Some(t) = t.as_deref_mut() {
                t.balancer = balancer.map(str::to_string);
                t.last = last.map(str::to_string);
            }
        };
        let mut ctx = ctx.clone();
        for _ in 0..4 {
            let tag = match self.router.route_resolved(&ctx, ips) {
                Decision::Outbound(t) => t,
                Decision::Balancer(t) => {
                    if let Some((tag, out)) = self.balance_choice(&t) {
                        note(&mut trace, format_args!("balancer:{t}"));
                        finish(&mut trace, Some(&t), Some(&tag));
                        return out;
                    }
                    match self.fallbacks.get(&t) {
                        Some(f) => {
                            note(&mut trace, format_args!("balancer:{t}>"));
                            f.clone()
                        }
                        None => {
                            note(&mut trace, format_args!("balancer:{t}"));
                            finish(&mut trace, Some(&t), Some(&self.default_tag));
                            return self.default.clone();
                        }
                    }
                }
                Decision::Default => {
                    note(&mut trace, format_args!("default"));
                    finish(&mut trace, None, Some(&self.default_tag));
                    return self.default.clone();
                }
            };
            if let Some(inbound) = self.loopbacks.get(&tag) {
                note(&mut trace, format_args!("{tag}>"));
                ctx.inbound_tag = Some(inbound.as_str());
                continue;
            }
            note(&mut trace, format_args!("{tag}"));
            finish(&mut trace, None, Some(&tag));
            return self.outs.get(&tag).cloned().unwrap_or_else(
                || {
                    if self.known.contains(&tag) { self.default.clone() } else { Out::Block }
                },
            );
        }
        note(&mut trace, format_args!("loop"));
        finish(&mut trace, None, None);
        Out::Block
    }

    pub async fn watch_balancers(self: Rc<Self>) {
        self.probe_balancers().await;
        if !self.background {
            return;
        }
        let (weak, every, used) = (Rc::downgrade(&self), self.probe_every, self.balancer_wake.clone());
        drop(self);
        loop {
            if !balancer_due(&used, every, || weak.strong_count() > 0).await {
                break;
            }
            let Some(e) = weak.upgrade() else { break };
            if e.balancer_used.replace(false) {
                e.probe_balancers().await;
            }
        }
    }

    pub async fn probe_balancers(&self) {
        let epoch = self.health_gen.get();
        let mut probed = std::collections::HashSet::new();
        for (tag, list) in &self.candidates {
            let start = Instant::now();
            if !matches!(self.strategies.get(tag), Some(crate::balance::Strategy::LeastPing)) {
                let mut probes: Vec<_> = list
                    .iter()
                    .filter(|(t, _)| probed.insert(t.clone()))
                    .map(|(t, p)| {
                        let (t, p) = (t.clone(), p.clone());
                        Some(Box::pin(async move { (t, crate::ping::probe_observatory(&p, &self.probe_url).await) }))
                    })
                    .collect();
                std::future::poll_fn(|cx| {
                    use std::future::Future;
                    for slot in &mut probes {
                        if let Some(f) = slot
                            && let std::task::Poll::Ready((t, r)) = f.as_mut().poll(cx)
                        {
                            if self.health_gen.get() == epoch {
                                self.health.borrow_mut().entry(t).or_default().observe(r);
                            }
                            *slot = None;
                        }
                    }
                    if probes.iter().all(Option::is_none) { std::task::Poll::Ready(()) } else { std::task::Poll::Pending }
                })
                .await;
                if self.health_gen.get() != epoch {
                    return;
                }
                if self.strategies.get(tag).is_some_and(crate::balance::Strategy::measured) {
                    self.refresh_balance(tag);
                }
                self.udp_routes.borrow_mut().clear();
                continue;
            }
            let probes = list.iter().map(|(t, p)| {
                let p = p.clone();
                let t = t.clone();
                async move { crate::ping::probe_observatory(&p, &self.probe_url).await.map(|rtt| (t, p, rtt)).ok_or(()) }
            });
            let first = first_ok(probes).await;
            if self.health_gen.get() != epoch {
                return;
            }
            match first {
                Some(Ok((t, p, rtt))) => {
                    tracing::info!("balancer {tag}: {t} ({rtt:?}, {:?} total)", start.elapsed());
                    self.balancers.borrow_mut().insert(tag.clone(), Out::Proxy(p));
                    self.health.borrow_mut().entry(t).or_default().observe(Some(rtt));
                    self.udp_routes.borrow_mut().clear();
                }
                _ => tracing::warn!("balancer {tag}: no candidate answered"),
            }
        }
    }

    fn balance_choice(&self, tag: &str) -> Option<(String, Out)> {
        if !self.balancer_used.replace(true) {
            self.balancer_wake.notify_one();
        }
        let measured = self.strategies.get(tag).is_some_and(crate::balance::Strategy::measured);
        if measured && !self.health.borrow().is_empty() {
            self.refresh_balance(tag);
        }
        let choices = self.choices.borrow();
        let h = self.health.borrow();
        let dead = |t: &str| h.get(t).is_some_and(|h| h.fresh() && !h.alive);
        if measured && choices.get(tag).is_none_or(Vec::is_empty) {
            let all = self.candidates.get(tag)?;
            let mut list: Vec<_> = all.iter().filter(|(t, _)| !dead(t)).collect();
            if list.is_empty() {
                if self.fallbacks.contains_key(tag) {
                    return None;
                }
                list = all.iter().collect();
            }
            if list.is_empty() {
                return None;
            }
            let (t, p) = list[crate::balance::random(list.len())];
            return Some((t.clone(), Out::Proxy(p.clone())));
        }
        let all = choices.get(tag)?;
        let mut list: Vec<_> = all.iter().filter(|(t, _)| !dead(t)).collect();
        if list.is_empty() {
            if self.fallbacks.contains_key(tag) {
                return None;
            }
            list = all.iter().collect();
        }
        if list.is_empty() {
            return None;
        }
        let index = if matches!(self.strategies.get(tag), Some(crate::balance::Strategy::RoundRobin)) {
            let mut rr = self.round_robin.borrow_mut();
            let i = rr.entry(tag.to_string()).or_default();
            let n = *i % list.len();
            *i = (n + 1) % list.len();
            n
        } else {
            crate::balance::random(list.len())
        };
        Some((*list[index]).clone())
    }

    fn refresh_balance(&self, tag: &str) {
        let (Some(strategy), Some(list)) = (self.strategies.get(tag), self.candidates.get(tag)) else { return };
        let h = self.health.borrow();
        let nodes: Vec<_> = list.iter().map(|(t, _)| (t.as_str(), h.get(t).cloned().unwrap_or_default())).collect();
        let selected = strategy.select(&nodes);
        drop(h);
        let choices: Vec<_> = selected.into_iter().map(|i| (list[i].0.clone(), Out::Proxy(list[i].1.clone()))).collect();
        self.balancers.borrow_mut().remove(tag);
        self.choices.borrow_mut().insert(tag.to_string(), choices);
    }

    fn observe_proxy(&self, proxy: &Rc<Proxy>, rtt: Option<Duration>, epoch: u64) {
        if epoch != self.health_gen.get() {
            return;
        }
        let Some((tag, _)) = self.outs.iter().find(|(_, out)| matches!(out, Out::Proxy(p) if Rc::ptr_eq(p, proxy))) else { return };
        self.health.borrow_mut().entry(tag.clone()).or_default().transport(rtt);
        for (b, list) in &self.candidates {
            if list.iter().any(|(t, _)| t == tag) && self.strategies.get(b).is_some_and(crate::balance::Strategy::measured) {
                self.refresh_balance(b);
            }
        }
    }

    async fn failover(
        &self,
        tag: &str,
        failed: &Rc<Proxy>,
        dest: Address,
        port: u16,
        err: Error,
        pending: Option<&std::cell::Cell<Option<Rc<Proxy>>>>,
    ) -> Result<crate::BoxStream, Error> {
        let Some(list) = self.candidates.get(tag) else { return Err(err) };
        for (t, p) in list {
            if Rc::ptr_eq(p, failed) {
                continue;
            }
            let current = match self.balancers.borrow().get(tag) {
                Some(Out::Proxy(c)) if !Rc::ptr_eq(c, failed) => Some(c.clone()),
                _ => None,
            };
            let p = current.as_ref().unwrap_or(p);
            let (started, epoch) = (Instant::now(), self.health_gen.get());
            let r = self.dial(p, dest.clone(), port, pending).await;
            if !r.as_ref().err().is_some_and(dial_held) {
                self.observe_proxy(p, r.as_ref().ok().map(|_| started.elapsed()), epoch);
            }
            if let Ok(s) = r {
                if current.is_none() {
                    tracing::info!("balancer {tag}: {t} (previous failed: {err})");
                    self.balancers.borrow_mut().insert(tag.to_string(), Out::Proxy(p.clone()));
                    self.udp_routes.borrow_mut().clear();
                }
                return Ok(s);
            }
            if current.is_some() {
                break;
            }
        }
        Err(err)
    }

    pub fn reset_pools(self: &Rc<Self>) {
        for out in self.outs.values() {
            if let Out::Proxy(p) = out {
                p.reset();
            }
        }
        self.health.borrow_mut().clear();
        self.health_gen.set(self.health_gen.get() + 1);
        self.direct_failed.borrow_mut().clear();
        self.refused.borrow_mut().clear();
        self.refused_hits.borrow_mut().clear();
        self.empty_hits.borrow_mut().clear();
        self.dial_fails.borrow_mut().clear();
        self.stalls.borrow_mut().clear();
        self.real.borrow_mut().clear();
        for (tag, s) in &self.strategies {
            if s.measured() {
                self.choices.borrow_mut().remove(tag);
                self.balancers.borrow_mut().remove(tag);
            }
        }
        if self.strategies.iter().any(|(tag, s)| s.measured() && self.candidates.contains_key(tag)) {
            let e = self.clone();
            tokio::task::spawn_local(async move { e.probe_balancers().await });
        }
    }

    fn note_proxy(&self, ok: bool) {
        self.note_proxy_at(ok, Instant::now());
    }

    fn note_proxy_at(&self, ok: bool, now: Instant) {
        if ok {
            self.fail_streak.set(0);
            self.fail_since.set(None);
            self.fail_signaled.set(0);
            return;
        }
        let n = self.fail_streak.get() + 1;
        self.fail_streak.set(n);
        let since = self.fail_since.get().unwrap_or(now);
        self.fail_since.set(Some(since));
        let last = self.fail_signaled.get();
        let due = if last == 0 { n >= FAILING_STREAK && now.duration_since(since) >= FAILING_SPAN } else { n >= last + FAILING_STREAK * 4 };
        if due && let Some(f) = &self.on_failing {
            self.fail_signaled.set(n);
            tracing::info!(target: "rv::core", "server failing: {n} connections in a row over {}s", now.duration_since(since).as_secs());
            f();
        }
    }

    fn dial_allowed(&self, p: &Rc<Proxy>, now: Instant) -> bool {
        let mut m = self.dial_fails.borrow_mut();
        let Some(f) = m.get_mut(&Rc::as_ptr(p)) else { return true };
        let Some(until) = f.until else { return true };
        if now < until || f.probe.is_some_and(|t| now.saturating_duration_since(t) < SETUP_TIMEOUT) {
            return false;
        }
        f.probe = Some(now);
        true
    }

    fn dial_done(&self, p: &Rc<Proxy>, ok: bool, now: Instant) {
        let mut m = self.dial_fails.borrow_mut();
        let key = Rc::as_ptr(p);
        if ok {
            if m.remove(&key).is_some_and(|f| f.until.is_some()) {
                tracing::info!(target: "rv::core", "server dials work again");
            }
            return;
        }
        let f = m.entry(key).or_insert(DialFails { n: 0, since: now, until: None, backoff: Duration::ZERO, probe: None });
        f.n += 1;
        if f.until.is_some_and(|u| now < u) {
            return;
        }
        if f.until.is_none() && (f.n < DIAL_FAILS || now.saturating_duration_since(f.since) < DIAL_FAIL_SPAN) {
            return;
        }
        f.backoff = if f.until.is_none() { DIAL_BACKOFF } else { (f.backoff * 2).min(DIAL_BACKOFF_MAX) };
        f.until = Some(now + f.backoff);
        f.probe = None;
        tracing::info!(target: "rv::core", "server dials failing: {} in a row over {}s, next try in {}s", f.n, now.saturating_duration_since(f.since).as_secs(), f.backoff.as_secs());
    }

    async fn dial(
        &self,
        p: &Rc<Proxy>,
        dest: Address,
        port: u16,
        pending: Option<&std::cell::Cell<Option<Rc<Proxy>>>>,
    ) -> Result<BoxStream, Error> {
        if self.ipv4_only && matches!(dest, Address::Ipv6(_)) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "IPv6 is disabled").into());
        }
        let now = Instant::now();
        if !self.dial_allowed(p, now) {
            return Err(Error::Io(io::Error::new(io::ErrorKind::ConnectionRefused, DIAL_HELD)));
        }
        struct Probe<'a>(&'a RefCell<HashMap<*const Proxy, DialFails>>, *const Proxy, Instant);
        impl Drop for Probe<'_> {
            fn drop(&mut self) {
                if let Some(f) = self.0.borrow_mut().get_mut(&self.1)
                    && f.probe == Some(self.2)
                {
                    f.probe = None;
                }
            }
        }
        let _probe = Probe(&self.dial_fails, Rc::as_ptr(p), now);
        if let Some(c) = pending {
            c.set(Some(p.clone()));
        }
        let r = p.connect(dest, port).await;
        if let Some(c) = pending {
            c.set(None);
        }
        if !r.as_ref().err().is_some_and(offline) {
            self.dial_done(p, r.is_ok(), Instant::now());
        }
        r
    }

    fn stalled(&self, app: &str, now: Instant) -> bool {
        self.stalls.borrow().get(app).is_some_and(|s| s.streak >= STALL_FREE && now.saturating_duration_since(s.last) < STALL_GAP)
    }

    fn stall_note(&self, app: &str, got: bool, now: Instant) {
        let mut m = self.stalls.borrow_mut();
        if got {
            m.remove(app);
            return;
        }
        if m.len() >= 256 && !m.contains_key(app) {
            m.retain(|_, s| now.saturating_duration_since(s.last) < STALL_GAP);
            if m.len() >= 256 {
                m.clear();
            }
        }
        let s = m.entry(app.to_string()).or_insert(Stalls { streak: 0, last: now, next: now });
        s.streak = if now.saturating_duration_since(s.last) < STALL_GAP { s.streak + 1 } else { 1 };
        s.last = now;
        if s.streak == STALL_FREE {
            tracing::info!(target: "rv::core", "app {app}: {STALL_FREE} connections in a row got no reply via the server, its new ones are limited");
        }
    }

    fn stall_dial(&self, app: &str, now: Instant) -> bool {
        let mut m = self.stalls.borrow_mut();
        let Some(s) = m.get_mut(app) else { return true };
        let next = s.next.max(now);
        if next > now + STALL_BURST {
            return false;
        }
        s.next = next + STALL_DIAL;
        true
    }

    fn own_loop(&self, process: Option<&str>, host: &str, dst: SocketAddr) -> bool {
        let (Some(own), Some(p)) = (self.own_name.as_deref(), process) else { return false };
        if !(p == own || (p.len() == 15 && own.starts_with(p))) {
            return false;
        }
        self.servers.iter().any(|(a, port)| {
            *port == dst.port()
                && (a == host
                    || a.parse::<IpAddr>().is_ok_and(|ip| ip == dst.ip())
                    || rv_net::known_hosts(a).is_some_and(|ips| ips.contains(&dst.ip())))
        })
    }

    fn refused_now(&self, key: &str) -> bool {
        self.refused.borrow().get(key).and_then(|r| r.until).is_some_and(|u| Instant::now() < u)
    }

    fn refused_hit(&self, key: &str, now: Instant) -> (Duration, Option<u32>) {
        let (streak, log) = streak_hit(&mut self.refused_hits.borrow_mut(), key, now, 0);
        ((REFUSED_HOLD * (streak + 1)).min(REFUSED_HOLD_MAX), log)
    }

    fn empty_hit(&self, key: &str, now: Instant) -> Option<u32> {
        streak_hit(&mut self.empty_hits.borrow_mut(), key, now, EMPTY_FREE).1
    }

    fn empty_hold(&self, key: &str, now: Instant) -> Option<Duration> {
        let m = self.empty_hits.borrow();
        let h = m.get(key)?;
        (now.saturating_duration_since(h.last) < REFUSED_STREAK_GAP && h.streak + 1 >= EMPTY_FREE)
            .then(|| (REFUSED_HOLD * (h.streak + 2 - EMPTY_FREE)).min(REFUSED_HOLD_MAX))
    }

    fn empty_clear(&self, refused_key: &str) {
        let mut m = self.empty_hits.borrow_mut();
        if !m.is_empty() {
            m.retain(|k, _| !k.split_once('|').is_some_and(|(_, rest)| rest == refused_key));
        }
    }

    async fn refused_hold(&self, hold: Duration) {
        let held = self.refused_held.get();
        if held >= REFUSED_HELD_MAX {
            return;
        }
        self.refused_held.set(held + 1);
        struct Release<'a>(&'a std::cell::Cell<usize>);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get().saturating_sub(1));
            }
        }
        let _release = Release(&self.refused_held);
        tokio::time::sleep(hold).await;
    }

    fn note_refused(&self, key: &str, refused: bool) {
        let mut m = self.refused.borrow_mut();
        if !refused {
            m.remove(key);
            return;
        }
        let now = Instant::now();
        if !self.proxy_alive.get().is_some_and(|t| now - t < PROXY_ALIVE) {
            return;
        }
        if m.len() >= 256 && !m.contains_key(key) {
            m.retain(|_, r| r.until.is_some_and(|u| now < u));
            if m.len() >= 256 {
                m.clear();
            }
        }
        let r = m.entry(key.to_string()).or_insert(Refused { strikes: 0, until: None, ttl: REFUSED_TTL });
        r.strikes += 1;
        if r.strikes >= REFUSED_STRIKES {
            match r.until {
                Some(u) if now < u => {}
                Some(_) => {
                    r.ttl = (r.ttl * 2).min(REFUSED_TTL_MAX);
                    r.until = Some(now + r.ttl);
                }
                None => r.until = Some(now + r.ttl),
            }
            r.strikes = REFUSED_STRIKES - 1;
        }
    }

    fn direct_failed_recently(&self, key: &str) -> bool {
        self.direct_failed.borrow().get(key).is_some_and(|t| t.elapsed() < SMART_DIRECT_TTL)
    }

    fn remember_direct_failure(&self, key: String) {
        let mut m = self.direct_failed.borrow_mut();
        if m.len() >= 256 {
            m.retain(|_, t| t.elapsed() < SMART_DIRECT_TTL);
            if m.len() >= 256 {
                m.clear();
            }
        }
        m.insert(key, Instant::now());
    }

    pub fn direct_nets(&self) -> &[(IpAddr, u8)] {
        &self.direct_nets
    }

    pub fn tunnel_nets(&self) -> Vec<(IpAddr, u8)> {
        self.user_router.explicit_nets(|tag| !matches!(self.outs.get(tag), Some(Out::Direct)))
    }

    pub fn has_balancers(&self) -> bool {
        self.candidates.values().any(|l| !l.is_empty())
    }

    pub fn default_proxy(&self) -> Option<Rc<Proxy>> {
        match &self.default {
            Out::Proxy(p) => Some(p.clone()),
            _ => self.outs.values().find_map(|o| if let Out::Proxy(p) = o { Some(p.clone()) } else { None }),
        }
    }

    pub async fn tcp<S: crate::pipe::Local>(&self, local: S, dst: SocketAddr) -> Result<(), Error> {
        self.tcp_from(local, None, dst).await
    }

    fn process(&self, tcp: bool, src: Option<SocketAddr>, dst: SocketAddr) -> Option<String> {
        self.process_when(quiet(), tcp, src, dst)
    }

    fn process_when(&self, quiet: bool, tcp: bool, src: Option<SocketAddr>, dst: SocketAddr) -> Option<String> {
        let (owner, src) = (self.owner.as_ref()?, src?);
        if quiet && !self.owner_routes {
            return None;
        }
        owner(tcp, src, dst)
    }

    fn udp_process(&self, src: Option<SocketAddr>, dst: SocketAddr) -> Option<String> {
        let key = src.filter(|_| self.owner.is_some())?;
        let now = Instant::now();
        if let Some((p, at)) = self.udp_owners.borrow().get(&key)
            && now - *at < UDP_OWNER_TTL
        {
            return p.clone();
        }
        let p = self.process(false, src, dst);
        let mut owners = self.udp_owners.borrow_mut();
        if owners.len() >= self.udp_owners_sweep.get() {
            owners.retain(|_, (_, at)| now - *at < UDP_OWNER_TTL);
            if owners.len() >= UDP_OWNERS_MAX / 2 {
                owners.clear();
            }
            self.udp_owners_sweep.set((2 * owners.len()).max(256));
        }
        owners.insert(key, (p.clone(), now));
        p
    }

    pub async fn tcp_from<S: crate::pipe::Local>(&self, mut local: S, src: Option<SocketAddr>, dst: SocketAddr) -> Result<(), Error> {
        if self.ipv4_only && dst.is_ipv6() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "IPv6 is disabled").into());
        }
        let t0 = Instant::now();
        let mut first = vec![0u8; 16 * 1024 + 5];
        let sniff_until = tokio::time::Instant::now() + Duration::from_millis(500);
        let mut n = match tokio::time::timeout_at(sniff_until, local.read(&mut first)).await {
            Ok(Ok(0)) => return Ok(()),
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => 0,
        };
        while n > 0 && n < first.len() && crate::sniff::need_more(&first[..n]) {
            match tokio::time::timeout_at(sniff_until, local.read(&mut first[n..])).await {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(m)) => n += m,
                Ok(Err(e)) => return Err(e.into()),
            }
        }
        first.truncate(n);
        first.shrink_to_fit();
        let sniffed = crate::sniff::sniff(&first);
        let fake = self.is_fake(dst.ip());
        let fake_name = if fake { crate::dns::fake::name_for(dst.ip()) } else { None };
        if fake && fake_name.is_none() {
            return Err(Error::Io(io::Error::other(format!("fakedns: unknown address {dst}"))));
        }
        let domain = fake_name.or(sniffed.domain);
        let process = self.process(true, src, dst);
        let ctx = RouteCtx {
            domain: domain.as_deref(),
            ip: (!fake).then(|| dst.ip()),
            port: dst.port(),
            network: Network::Tcp,
            protocol: sniffed.protocol,
            inbound_tag: None,
            process: process.as_deref(),
        };
        let (out, trace) = Box::pin(self.route_async(&ctx)).await;
        let via = trace.path.as_str();
        if matches!(out, Out::Block) && trace.last.as_ref().is_some_and(|t| self.http_blocks.contains(t)) {
            let _ = local.write_all(HTTP_403).await;
            let _ = local.flush().await;
        }
        let host = domain.clone().unwrap_or_default();
        let smart = match &self.smart_fallback {
            Some(Out::Proxy(p)) if trace.last.as_deref() == Some(SMART_DIRECT) => Some(p.clone()),
            _ => None,
        };
        let looped = self.own_loop(process.as_deref(), &host, dst);
        let direct_key = if fake { host.clone() } else { dst.ip().to_string() };
        let refused_key = refused_key(trace.last.as_deref(), &host, dst);
        let fell_back = std::cell::Cell::new(false);
        let refused_log = std::cell::Cell::new(None::<Option<u32>>);
        let app_key = || format!("{}|{refused_key}", process.as_deref().unwrap_or_default());
        let stall_app = process.as_deref().filter(|p| !p.is_empty());
        let stalled = stall_app.is_some_and(|a| self.stalled(a, Instant::now()));
        let is_direct = matches!(out, Out::Direct);
        let is_proxy = main_proxy(&out, trace.last.as_deref()) && !looped;
        let pending = std::cell::Cell::new(None::<Rc<Proxy>>);
        let mut setup = None;
        let mut idle = false;
        let mut bytes = (n as u64, 0);
        let mut ends = (None, Duration::ZERO, Duration::ZERO);
        let r: Result<(), Error> = async {
            let setup_io = async {
                if looped {
                    crate::stage::set("held");
                    self.refused_hold(REFUSED_HOLD).await;
                    return Err(Error::Io(io::Error::new(io::ErrorKind::ConnectionRefused, OWN_LOOP)));
                }
                let held = async |e: Error| {
                    if dial_held(&e) {
                        crate::stage::set("held");
                        self.refused_hold(self.refused_hit(&app_key(), Instant::now()).0).await;
                    }
                    e
                };
                let mut remote: crate::BoxStream = match out {
                    Out::Block => return Ok(None),
                    Out::Direct => {
                        let name = host.as_str();
                        let direct = |opts: DialOptions| async move {
                            if fake { crate::stage::set("dns"); }
                            let ips = if fake { self.resolve_addresses(name).await? } else { vec![dst.ip()] };
                            crate::stage::set("tcp");
                            rv_net::dial_tcp_resolved(ips, dst.port(), &opts).await
                        };
                        match &smart {
                            None => Box::new(direct(self.direct_dial.clone()).await?),
                            Some(p) => {
                                let quick = DialOptions { timeout: Some(SMART_DIRECT_TRY), ..self.direct_dial.clone() };
                                let tried = if stalled {
                                    Some(direct(quick).await)
                                } else if self.direct_failed_recently(&direct_key) {
                                    None
                                } else {
                                    match direct(quick).await {
                                        Ok(s) => Some(Ok(s)),
                                        Err(e) if direct_blocked(&e) => {
                                            tracing::debug!(target: "rv::conn", "direct {dst} host={host} failed: {e}; via proxy for {}s", SMART_DIRECT_TTL.as_secs());
                                            self.remember_direct_failure(direct_key.clone());
                                            None
                                        }
                                        Err(e) => Some(Err(e)),
                                    }
                                };
                                match tried {
                                    Some(r) => Box::new(r?),
                                    None => {
                                        fell_back.set(true);
                                        let dest = if fake && self.ipv4_only { Address::from_ip(self.resolve_real(&host).await?) }
                                            else if fake { Address::Domain(host.clone()) } else { Address::from_ip(dst.ip()) };
                                        crate::stage::set("connect");
                                        match self.dial(p, dest, dst.port(), Some(&pending)).await {
                                            Ok(s) => s,
                                            Err(e) => return Err(held(e).await),
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Out::Proxy(p) => {
                        let dest = match (fake, domain) {
                            (true, Some(d)) if self.ipv4_only => Address::from_ip(self.resolve_real(&d).await?),
                            (true, Some(d)) => Address::Domain(d),
                            _ => Address::from_ip(dst.ip()),
                        };
                        if is_proxy && self.refused_now(&refused_key) {
                            let (hold, log) = self.refused_hit(&app_key(), Instant::now());
                            refused_log.set(Some(log));
                            crate::stage::set("refused");
                            self.refused_hold(hold).await;
                            return Err(Error::Io(io::Error::new(io::ErrorKind::ConnectionRefused, REFUSED_RECENTLY)));
                        }
                        if is_proxy && stalled && stall_app.is_some_and(|a| !self.stall_dial(a, Instant::now())) {
                            crate::stage::set("held");
                            self.refused_hold(REFUSED_HOLD).await;
                            return Err(Error::Io(io::Error::new(io::ErrorKind::ConnectionRefused, STALL_HELD)));
                        }
                        if is_proxy && let Some(hold) = self.empty_hold(&app_key(), Instant::now()) {
                            crate::stage::set("held");
                            self.refused_hold(hold).await;
                        }
                        let (started, epoch) = (Instant::now(), self.health_gen.get());
                        match self.dial(&p, dest.clone(), dst.port(), Some(&pending)).await {
                            Ok(s) => { self.observe_proxy(&p, Some(started.elapsed()), epoch); s }
                            Err(e) => {
                                if !dial_held(&e) { self.observe_proxy(&p, None, epoch); }
                                if is_proxy && !offline(&e) { self.note_proxy(false); }
                                let r = match trace.balancer.as_deref() {
                                    Some(tag) => self.failover(tag, &p, dest, dst.port(), e, Some(&pending)).await,
                                    None => Err(e),
                                };
                                match r {
                                    Ok(s) => s,
                                    Err(e) => return Err(held(e).await),
                                }
                            }
                        }
                    }
                };
                crate::stage::set("send");
                if !first.is_empty() {
                    remote.write_all(&first).await?;
                }
                remote.flush().await?;
                Ok::<_, Error>(Some(remote))
            };
            let stage = Rc::new(std::cell::Cell::new("connect"));
            let Some(mut remote) = tokio::time::timeout(SETUP_TIMEOUT, Box::pin(crate::stage::scope(stage.clone(), setup_io)))
                .await
                .map_err(|_| {
                    if let Some(p) = pending.take() { self.dial_done(&p, false, Instant::now()); }
                    if is_proxy && stage.get() != "send" { self.note_proxy(false); }
                    Error::Io(io::Error::new(io::ErrorKind::TimedOut, format!("connection setup timed out at {}", stage.get())))
                })??
            else {
                return Ok(());
            };
            setup = Some(t0.elapsed());
            drop(first);
            let relay_t0 = Instant::now();
            let proxied = is_proxy || fell_back.get();
            let got = || {
                if proxied {
                    self.proxy_alive.set(Some(Instant::now()));
                    self.note_proxy(true);
                }
            };
            let r = crate::pipe::copy(&mut local, &mut FirstData { inner: &mut remote, on: Some(got) }, self.conn_idle).await;
            idle = r.idle;
            let sent = r.up + n as u64;
            bytes = (sent, r.down);
            ends = (r.eof, r.quiet, r.gap);
            let side = match r.error_side {
                Some(crate::pipe::Side::Local) => "app",
                Some(crate::pipe::Side::Remote) if is_direct && !fell_back.get() => "site",
                Some(crate::pipe::Side::Remote) => "server",
                None => "",
            };
            if is_proxy {
                let now = Instant::now();
                note_served(r.down > 0);
                let refused = server_refused(&r);
                let alive = self.proxy_alive.get().is_some_and(|t| now.saturating_duration_since(t) < PROXY_ALIVE);
                self.note_refused(&refused_key, refused);
                if r.down > 0 {
                    self.empty_clear(&refused_key);
                } else {
                    let empty = empty_close(&r, sent, relay_t0.elapsed());
                    if empty {
                        refused_log.set(Some(self.empty_hit(&app_key(), now)));
                    }
                    if !alive && sent > 0 && !r.idle && (empty || refused) {
                        self.note_proxy(false);
                    }
                }
            }
            if smart.is_some() && is_direct && !fell_back.get() && direct_silent(&r, sent, relay_t0.elapsed()) {
                tracing::debug!(target: "rv::conn", "direct {dst} host={host} silent for {}ms; via proxy for {}s", relay_t0.elapsed().as_millis(), SMART_DIRECT_TTL.as_secs());
                self.remember_direct_failure(direct_key.clone());
            }
            if proxied && let Some(app) = stall_app {
                if r.down > 0 {
                    self.stall_note(app, true, Instant::now());
                } else if stall(&r, sent) {
                    self.stall_note(app, false, Instant::now());
                }
            }
            r.error.map_or(Ok(()), |e| Err(if side.is_empty() { e } else { io::Error::new(e.kind(), format!("{side}: {e}")) }.into()))
        }
        .await;
        if is_proxy
            && setup.is_none()
            && let Err(e) = &r
            && !offline(e)
        {
            note_setup_failed();
        }
        if refused_log.get().is_none()
            && bytes.1 == 0
            && let Err(e) = &r
        {
            let key = format!("{}|{via}{}|{e}", process.as_deref().unwrap_or_default(), if fell_back.get() { ">proxy" } else { "" });
            refused_log.set(Some(streak_hit(&mut self.fail_lines.borrow_mut(), &key, Instant::now(), FAIL_LINES_FREE).1));
        }
        let repeats = match refused_log.get() {
            Some(None) => return r,
            Some(Some(n)) if n > 0 => format!(" repeats={n}"),
            _ => String::new(),
        };
        tracing::debug!(
            target: "rv::conn",
            "tcp {dst} host={host} via={via}{} app={} setup_ms={} ms={} up={} down={} idle={idle} eof={} quiet_ms={} gap_ms={}{repeats} err={}",
            if fell_back.get() { ">proxy" } else { "" },
            process.as_deref().unwrap_or_default(),
            setup.map_or(-1, |d| d.as_millis() as i64),
            t0.elapsed().as_millis(),
            bytes.0,
            bytes.1,
            match ends.0 { Some(crate::pipe::Side::Local) => "app", Some(crate::pipe::Side::Remote) => "server", None => "-" },
            ends.1.as_millis(),
            ends.2.as_millis(),
            r.as_ref().err().map(ToString::to_string).unwrap_or_default(),
        );
        r
    }

    pub fn udp_allowed(&self, dst: SocketAddr) -> bool {
        self.udp_route(None, dst).is_some()
    }

    pub fn udp_route(&self, src: Option<SocketAddr>, dst: SocketAddr) -> Option<u64> {
        if self.ipv4_only && dst.is_ipv6() {
            return None;
        }
        let now = Instant::now();
        if let Some((r, at)) = self.udp_routes.borrow_mut().get_mut(&(src, dst))
            && now - *at < UDP_ROUTE_TTL
        {
            *at = now;
            return *r;
        }
        let r = self.udp_route_uncached(src, dst);
        let mut routes = self.udp_routes.borrow_mut();
        if routes.len() >= 1024 {
            routes.retain(|_, (_, at)| now - *at < UDP_ROUTE_TTL);
            if routes.len() >= 1024 {
                routes.clear();
            }
        }
        routes.insert((src, dst), (r, now));
        r
    }

    fn udp_route_uncached(&self, src: Option<SocketAddr>, dst: SocketAddr) -> Option<u64> {
        let fake = self.is_fake(dst.ip());
        let name = fake.then(|| crate::dns::fake::name_for(dst.ip())).flatten();
        let process = self.udp_process(src, dst);
        let mut ctx = udp_ctx(dst, fake, process.as_deref());
        ctx.domain = name.as_deref();
        let class = match self.route(&ctx) {
            Out::Proxy(p) if dst.port() == 443 && self.no_udp443.contains(&Rc::as_ptr(&p)) => return None,
            Out::Proxy(p) if p.udp_allowed(dst.port()) => Rc::as_ptr(&p) as u64,
            Out::Proxy(_) | Out::Block => return None,
            Out::Direct => 1,
        };
        Some(match dst.ip() {
            std::net::IpAddr::V4(a) if fake => (1 << 63) | u64::from(u32::from(a)),
            _ => class,
        })
    }

    fn udp_session_out(&self, src: Option<SocketAddr>, dst: SocketAddr, process: Option<&str>) -> (Out, Trace) {
        let (out, mut trace) = self.route_tagged(&udp_ctx(dst, false, process));
        let Some(class) = self.udp_routes.borrow().get(&(src, dst)).and_then(|(r, _)| *r) else { return (out, trace) };
        if udp_class(&out) == Some(class) {
            return (out, trace);
        }
        match self.outs.iter().find(|(_, o)| udp_class(o) == Some(class)) {
            Some((tag, o)) => {
                trace.last = Some(tag.clone());
                (o.clone(), trace)
            }
            None => (out, trace),
        }
    }

    pub async fn udp(&self, rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>, reply: &dyn Fn(SocketAddr, &[u8])) -> Result<(), Error> {
        self.udp_from(None, rx, reply).await
    }

    pub async fn udp_from(
        &self,
        src: Option<SocketAddr>,
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        let Some(first) = rx.recv().await else {
            return Ok(());
        };
        let t0 = Instant::now();
        let dst = first.0;
        if self.ipv4_only && dst.is_ipv6() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "IPv6 is disabled").into());
        }
        let process = self.udp_process(src, dst);
        if self.is_fake(dst.ip()) {
            return self.udp_fake(first, rx, reply, process.as_deref(), src).await;
        }
        let (out, trace) = self.udp_session_out(src, dst, process.as_deref());
        let served = main_proxy(&out, trace.last.as_deref()).then(|| std::cell::Cell::new(false));
        let seen = |from: SocketAddr, data: &[u8]| {
            if let Some(s) = &served {
                s.set(true);
            }
            reply(from, data)
        };
        let via = trace.path;
        let r = match out {
            Out::Block => Ok(()),
            Out::Proxy(p) => p.udp_session_from(src, first, rx, &seen).await,
            Out::Direct => direct_udp(first, rx, reply, self.protect.as_ref()).await.map_err(Error::Io),
        };
        if r.is_err() {
            self.udp_routes.borrow_mut().remove(&(src, dst));
        }
        if let Some(s) = served {
            note_served(s.get());
        }
        tracing::debug!(
            target: "rv::conn",
            "udp {dst} via={via} ms={} err={}",
            t0.elapsed().as_millis(),
            r.as_ref().err().map(ToString::to_string).unwrap_or_default(),
        );
        r
    }

    async fn udp_fake(
        &self,
        first: (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
        process: Option<&str>,
        source: Option<SocketAddr>,
    ) -> Result<(), Error> {
        let t0 = Instant::now();
        let fake = first.0;
        let name =
            crate::dns::fake::name_for(fake.ip()).ok_or_else(|| Error::Io(io::Error::other(format!("fakedns: unknown address {fake}"))))?;
        let mut ctx = udp_ctx(fake, true, process);
        ctx.domain = Some(&name);
        let (out, trace) = Box::pin(self.route_async(&ctx)).await;
        let via = trace.path;
        if matches!(out, Out::Block) {
            return Ok(());
        }
        let real = self.resolve_real(&name).await?;
        let to_real = |d: SocketAddr| if d.ip() == fake.ip() { SocketAddr::new(real, d.port()) } else { d };
        let served = main_proxy(&out, trace.last.as_deref()).then(|| std::cell::Cell::new(false));
        let back = |from: SocketAddr, data: &[u8]| {
            if let Some(s) = &served {
                s.set(true);
            }
            reply(if from.ip() == real { SocketAddr::new(fake.ip(), from.port()) } else { from }, data)
        };
        let (tx, mut inner) = mpsc::channel(64);
        let forward = async {
            while let Some((d, data)) = rx.recv().await {
                if tx.send((to_real(d), data)).await.is_err() {
                    break;
                }
            }
        };
        let first = (to_real(first.0), first.1);
        let session = async {
            match out {
                Out::Proxy(p) => p.udp_session_from(source, first, &mut inner, &back).await,
                _ => direct_udp(first, &mut inner, &back, self.protect.as_ref()).await.map_err(Error::Io),
            }
        };
        let r = tokio::select! {
            r = session => r,
            _ = forward => Ok(()),
        };
        if let Some(s) = served {
            note_served(s.get());
        }
        tracing::debug!(
            target: "rv::conn",
            "udp {fake} host={name} real={real} via={via} ms={} err={}",
            t0.elapsed().as_millis(),
            r.as_ref().err().map(ToString::to_string).unwrap_or_default(),
        );
        r
    }

    pub fn dns_route(&self, query: &[u8]) -> DnsTarget {
        let q = crate::dns::question(query);
        let qname = q.as_ref().map(|q| q.name.as_str());
        let i = self.dns_pick(qname, true);
        if let Some(q) = &q
            && let Some(reply) = self.dns_answer(query, q, i)
        {
            return DnsTarget::Reply(reply);
        }
        if let Some(q) = &q {
            let non_ip = q.qtype != crate::dns::TYPE_A && q.qtype != crate::dns::TYPE_AAAA;
            if (non_ip && self.dns_reject_non_ip) || self.dns_block_types.contains(&q.qtype) {
                return DnsTarget::Reply(crate::dns::rcode(query, q, 5));
            }
            if let Some(Hosted::Alias(name)) = self.hosted(&q.name) {
                return DnsTarget::Lookup(name);
            }
            if !non_ip && self.dns.iter().any(|s| s.expected.is_some() || s.unexpected.is_some()) {
                return DnsTarget::Lookup(q.name.clone());
            }
        }
        let order: Vec<_> = self
            .dns_order(qname, false)
            .into_iter()
            .filter(|i| q.as_ref().is_none_or(|q| family_allowed(self.dns[*i].families, q.qtype)))
            .collect();
        if order.is_empty()
            && let Some(q) = &q
        {
            return DnsTarget::Reply(crate::dns::answer(query, q, &[], 10));
        }
        DnsTarget::Servers(order.into_iter().map(|i| (self.dns_hop(i), self.dns[i].timeout)).collect())
    }

    fn dns_hop(&self, i: usize) -> DnsHop {
        match &self.dns[i].up {
            Upstream::System => DnsHop::Direct(self.system_server()),
            Upstream::Udp(a) if matches!(self.dns_out(i), Out::Direct) => DnsHop::Direct(*a),
            _ => DnsHop::Stream(i),
        }
    }

    fn dns_pick(&self, qname: Option<&str>, fake: bool) -> usize {
        self.dns_order(qname, fake).first().copied().unwrap_or(self.dns.len() - 1)
    }

    fn tailnet_dns(&self, name: &str) -> Option<usize> {
        let name = name.trim_end_matches('.').to_ascii_lowercase();
        if name != "ts.net" && !name.ends_with(".ts.net") {
            return None;
        }
        let ctx =
            RouteCtx { domain: Some(&name), ip: None, port: 443, network: Network::Tcp, protocol: None, inbound_tag: None, process: None };
        let Decision::Outbound(tag) = self.user_router.route(&ctx) else { return None };
        self.dns.iter().position(|s| s.outbound.as_deref() == Some(tag.as_str()))
    }

    fn dns_order(&self, qname: Option<&str>, fake: bool) -> Vec<usize> {
        if let Some(i) = qname.and_then(|name| self.tailnet_dns(name)) {
            return vec![i];
        }
        let ok = |s: &DnsServer| fake || s.up != Upstream::Fake;
        let mut out = Vec::new();
        let mut taken = vec![false; self.dns.len()];
        let matched = |s: &DnsServer| qname.is_some_and(|q| s.domains.as_ref().is_some_and(|d| d.domain_goes_to(q, "dns")));
        for (i, s) in self.dns.iter().enumerate() {
            if ok(s) && matched(s) {
                out.push(i);
                taken[i] = true;
                if s.final_query {
                    return out;
                }
            }
        }
        if !(self.no_fallback || self.no_fallback_if_match && !out.is_empty()) {
            for (i, s) in self.dns.iter().enumerate() {
                if ok(s) && !s.skip_fallback && !taken[i] {
                    out.push(i);
                    if s.final_query {
                        return out;
                    }
                }
            }
        }
        if out.is_empty() {
            out.extend(self.dns.iter().position(ok).or(Some(self.dns.len() - 1)));
        }
        out
    }

    fn system_server(&self) -> SocketAddr {
        self.system_dns
            .lock()
            .ok()
            .and_then(|v| v.iter().copied().find(|a| !self.ipv4_only || a.is_ipv4()))
            .unwrap_or_else(|| DEFAULT_DNS.parse().unwrap())
    }

    fn is_fake(&self, ip: IpAddr) -> bool {
        crate::dns::fake::is_fake(ip)
            && (self.fake_all || self.dns.iter().any(|s| s.up == Upstream::Fake) || crate::dns::fake::name_for(ip).is_some())
    }

    fn fake_for(&self, name: &str, i: usize) -> bool {
        if self.tailnet_dns(name).is_some() {
            return false;
        }
        if !crate::dns::fake::valid(name) {
            return false;
        }
        if self.dns[i].up == Upstream::Fake {
            return true;
        }
        if !self.fake_all {
            return false;
        }
        let ctx =
            RouteCtx { domain: Some(name), ip: None, port: 443, network: Network::Tcp, protocol: None, inbound_tag: None, process: None };
        matches!(self.route(&ctx), Out::Proxy(_))
    }

    fn goes_direct(&self, name: &str) -> bool {
        let ctx =
            RouteCtx { domain: Some(name), ip: None, port: 443, network: Network::Tcp, protocol: None, inbound_tag: None, process: None };
        matches!(self.route(&ctx), Out::Direct)
    }

    pub async fn resolve_host(&self, name: &str) -> io::Result<IpAddr> {
        self.resolve_real(name).await
    }

    async fn resolve_real(&self, name: &str) -> io::Result<IpAddr> {
        self.resolve_addresses(name).await?.into_iter().next().ok_or_else(|| io::Error::other("dns: empty response"))
    }

    async fn route_async(&self, ctx: &RouteCtx<'_>) -> (Out, Trace) {
        let ips = if self.router.needs_ip_lookup(ctx) {
            self.resolve_addresses(ctx.domain.unwrap()).await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut trace = Trace::default();
        let out = self.resolve_with_ips(ctx, &ips, Some(&mut trace));
        (out, trace)
    }

    async fn resolve_addresses(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        if self.ipv4_only {
            return self.resolve_query(name, crate::dns::TYPE_A).await;
        }
        let mut v4 = Box::pin(self.resolve_query(name, crate::dns::TYPE_A));
        let mut v6 = Box::pin(self.resolve_query(name, crate::dns::TYPE_AAAA));
        let (v4, v6) = tokio::select! {
            r = &mut v4 => {
                let v6 = if r.as_ref().is_ok_and(|ips| !ips.is_empty()) {
                    tokio::time::timeout(AAAA_GRACE, &mut v6).await.unwrap_or_else(|_| Ok(Vec::new()))
                } else {
                    v6.await
                };
                (r, v6)
            }
            r = &mut v6 => (v4.await, r),
        };
        let ips: Vec<_> = v4.unwrap_or_default().into_iter().chain(v6.unwrap_or_default()).collect();
        if ips.is_empty() { Err(io::Error::other(format!("dns: {name} did not resolve"))) } else { Ok(ips) }
    }

    pub async fn dns_lookup(&self, query: &[u8], name: &str) -> Option<Vec<u8>> {
        let q = crate::dns::question(query)?;
        let (ips, ttl) = match self.resolve_query_ttl(name, q.qtype).await {
            Ok(v) => v,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if let Some(c) = e.get_ref().and_then(|e| e.downcast_ref::<DnsRcode>()) {
                    return Some(crate::dns::rcode(query, &q, c.0));
                }
                (Vec::new(), 0)
            }
            Err(_) => return None,
        };
        Some(crate::dns::answer(query, &q, &ips, ttl))
    }

    async fn resolve_query(&self, name: &str, qtype: u16) -> io::Result<Vec<IpAddr>> {
        self.resolve_query_ttl(name, qtype).await.map(|(ips, _)| ips)
    }
    async fn resolve_query_ttl(&self, name: &str, qtype: u16) -> io::Result<(Vec<IpAddr>, u32)> {
        if !family_allowed(self.dns_families, qtype) {
            return Ok((Vec::new(), 0));
        }
        let mut name = name.strip_suffix('.').unwrap_or(name).to_string();
        match self.hosted(&name) {
            Some(Hosted::Ips(ips)) => {
                return Ok((ips.into_iter().filter(|ip| ip.is_ipv4() == (qtype == crate::dns::TYPE_A)).collect(), 10));
            }
            Some(Hosted::Rcode(_)) => return Ok((Vec::new(), 0)),
            Some(Hosted::Alias(alias)) => name = alias,
            None => {}
        }
        let cache_key = format!("{qtype}:{name}");
        if !self.no_cache
            && let Some((ips, until)) = self.real.borrow().get(&cache_key)
            && Instant::now() < *until
        {
            return Ok((ips.clone(), until.saturating_duration_since(Instant::now()).as_secs().min(u32::MAX.into()) as u32));
        }
        if !crate::dns::fits_query(&name) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "dns: name does not fit a query"));
        }
        let query = crate::dns::query_for(&name, qtype);
        let order: Vec<usize> =
            self.dns_order(Some(&name), false).into_iter().filter(|i| family_allowed(self.dns[*i].families, qtype)).collect();
        let proxied: Vec<bool> = order.iter().map(|&i| matches!(self.dns_out(i), Out::Proxy(_))).collect();
        let query = &query;
        let attempt = |k: usize| -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<Vec<u8>>> + '_>> {
            let i = order[k];
            Box::pin(async move {
                match self.dns_hop(i) {
                    DnsHop::Direct(a) => tokio::time::timeout(self.dns[i].timeout, self.dns_direct(a, query)).await.ok().flatten(),
                    DnsHop::Stream(i) => self.dns_exchange(i, query).await,
                }
            })
        };
        let mut pending: PendingDns<'_> = Vec::new();
        let mut next = 0;
        let mut hedge_at = tokio::time::Instant::now();
        let mut fallback: Option<(usize, Vec<u8>)> = None;
        let mut fallback_at = None;
        let (mut ips, mut ttl) = (Vec::new(), 0);
        loop {
            if pending.is_empty() {
                if next >= order.len() {
                    break;
                }
                pending.push((next, attempt(next)));
                hedge_at = tokio::time::Instant::now() + DNS_HEDGE;
                next += 1;
            }
            let hedge = next < order.len() && pending.iter().all(|(k, _)| proxied[*k] == proxied[next]);
            let give_up = dns_give_up(fallback_at, next >= order.len(), pending.iter().map(|(k, _)| *k), hedge_at);
            let sleep = tokio::time::sleep_until(give_up.unwrap_or(hedge_at));
            tokio::pin!(sleep);
            let ev = std::future::poll_fn(|cx| {
                for (j, (_, f)) in pending.iter_mut().enumerate() {
                    if let std::task::Poll::Ready(r) = f.as_mut().poll(cx) {
                        return std::task::Poll::Ready(Some((j, r)));
                    }
                }
                if (hedge || give_up.is_some()) && sleep.as_mut().poll(cx).is_ready() {
                    return std::task::Poll::Ready(None);
                }
                std::task::Poll::Pending
            })
            .await;
            let Some((j, reply)) = ev else {
                if give_up.is_some() {
                    break;
                }
                pending.push((next, attempt(next)));
                hedge_at = tokio::time::Instant::now() + DNS_HEDGE;
                next += 1;
                continue;
            };
            let (k, _) = pending.swap_remove(j);
            let Some(reply) = reply else { continue };
            let i = order[k];
            let mut got: Vec<IpAddr> =
                crate::dns::addresses(&reply).into_iter().filter(|ip| ip.is_ipv4() == (qtype == crate::dns::TYPE_A)).collect();
            if let Some(f) = &self.dns[i].expected {
                f.apply(&mut got, false);
            }
            if let Some(f) = &self.dns[i].unexpected {
                f.apply(&mut got, true);
            }
            got.truncate(DNS_ADDRS_MAX);
            if !got.is_empty() {
                ttl = crate::dns::answer_ttl(&reply);
                ips = got;
                break;
            }
            if fallback.as_ref().is_none_or(|(f, _)| k < *f) {
                fallback = Some((k, reply));
            }
            fallback_at.get_or_insert_with(|| (tokio::time::Instant::now(), next));
        }
        drop(pending);
        if ips.is_empty() {
            let code = fallback.as_ref().and_then(|(_, r)| r.get(3)).map_or(0, |v| v & 15);
            if code != 0 {
                return Err(io::Error::new(io::ErrorKind::NotFound, DnsRcode(code)));
            }
            return Err(io::Error::new(
                if fallback.is_some() { io::ErrorKind::NotFound } else { io::ErrorKind::TimedOut },
                format!("dns: {name} did not resolve"),
            ));
        }
        let ttl = ttl.min(REAL_TTL_MAX);
        if !self.no_cache && ttl > 0 {
            self.cache_real(cache_key, &ips, ttl);
        }
        Ok((ips, ttl))
    }

    fn cache_real(&self, key: String, ips: &[IpAddr], ttl: u32) {
        let mut real = self.real.borrow_mut();
        let now = Instant::now();
        if real.len() >= REAL_MAX {
            real.retain(|_, (_, until)| *until > now);
            if real.len() >= REAL_MAX / 2 {
                real.clear();
            }
        }
        real.insert(key, (ips.to_vec(), now + Duration::from_secs(ttl.min(REAL_TTL_MAX).into())));
    }

    fn hosted(&self, name: &str) -> Option<Hosted> {
        let mut name = name.to_ascii_lowercase();
        for depth in 0..=5 {
            let mut ips = Vec::new();
            let mut alias = None;
            let mut found = false;
            for (_, h) in self.hosts.iter().filter(|(r, _)| r.domain_goes_to(&name, "hosts")) {
                found = true;
                match h {
                    Hosted::Ips(v) => ips.extend_from_slice(v),
                    Hosted::Rcode(c) => return Some(Hosted::Rcode(*c)),
                    Hosted::Alias(a) => alias = Some(a.clone()),
                }
            }
            if !ips.is_empty() {
                ips.sort();
                ips.dedup();
                return Some(Hosted::Ips(ips));
            }
            if let Some(a) = alias {
                if depth < 5 {
                    name = a;
                    continue;
                } else {
                    return Some(Hosted::Alias(a));
                }
            }
            return if found {
                Some(Hosted::Ips(ips))
            } else if depth > 0 {
                Some(Hosted::Alias(name))
            } else {
                None
            };
        }
        None
    }

    fn dns_answer(&self, query: &[u8], q: &crate::dns::Question, server: usize) -> Option<Vec<u8>> {
        use crate::dns::{TYPE_A, TYPE_AAAA};
        if q.qtype != TYPE_A && q.qtype != TYPE_AAAA {
            return None;
        }
        if !family_allowed(self.dns_families, q.qtype) {
            return Some(crate::dns::answer(query, q, &[], 10));
        }
        let fixed = self.hosted(&q.name);
        let want_v4 = q.qtype == TYPE_A;
        match fixed {
            Some(Hosted::Rcode(code)) => Some(crate::dns::rcode(query, q, code)),
            Some(Hosted::Ips(ips)) => {
                let ips: Vec<IpAddr> = ips.iter().copied().filter(|i| i.is_ipv4() == want_v4).collect();
                Some(crate::dns::answer(query, q, &ips, 10))
            }
            Some(Hosted::Alias(_)) => None,
            None if !want_v4 && self.fake_for(&q.name, server) => Some(crate::dns::answer(query, q, &[], 300)),
            None if !want_v4 && !rv_net::direct_v6() && self.goes_direct(&q.name) => Some(crate::dns::answer(query, q, &[], 10)),
            None if self.fake_for(&q.name, server) => {
                let ip = crate::dns::fake::ip_for(&q.name);
                tracing::debug!(target: "rv::dns", "{} fake={ip}", q.name);
                Some(crate::dns::answer(query, q, &[ip.into()], crate::dns::fake::TTL))
            }
            None => None,
        }
    }

    pub fn dns_cache(&self) -> bool {
        !self.no_cache
    }

    fn dns_out(&self, i: usize) -> Out {
        let s = &self.dns[i];
        if let Some(tag) = &s.outbound {
            return self.outs.get(tag).cloned().unwrap_or(Out::Block);
        }
        if s.local {
            return Out::Direct;
        }
        let (host, port) = s.up.endpoint();
        let ip = host.parse::<IpAddr>().ok();
        let ctx = RouteCtx {
            domain: ip.is_none().then_some(host.as_str()),
            ip,
            port,
            network: if matches!(s.up, Upstream::Udp(_)) { Network::Udp } else { Network::Tcp },
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        self.route(&ctx)
    }

    pub fn dns_pinned(&self, i: usize) -> bool {
        self.dns.get(i).is_some_and(|s| s.outbound.is_some())
    }

    pub fn dns_label(&self, i: usize) -> String {
        self.dns.get(i).map(|s| s.up.to_string()).unwrap_or_default()
    }

    pub async fn dns_stream(&self, i: usize) -> io::Result<BoxStream> {
        let s = self.dns.get(i).ok_or_else(|| io::Error::other("no such dns server"))?;
        let (host, port) = s.up.endpoint();
        if let Upstream::Quic { .. } = s.up {
            return crate::dns::doq(&host, port, self.protect.as_ref(), self.ipv4_only).await;
        }
        let io: BoxStream = match self.dns_out(i) {
            Out::Proxy(p) => {
                let dest = match host.parse::<IpAddr>() {
                    Ok(ip) => Address::from_ip(ip),
                    Err(_) if self.ipv4_only => {
                        let ips = rv_net::resolve_protected(&host, self.protect.as_ref()).await?;
                        Address::from_ip(
                            rv_net::Family::V4 { force: true }
                                .pick(ips)?
                                .into_iter()
                                .next()
                                .ok_or_else(|| io::Error::other("dns: no IPv4 address"))?,
                        )
                    }
                    Err(_) => Address::Domain(host.clone()),
                };
                self.dial(&p, dest, port, None).await.map_err(io::Error::other)?
            }
            Out::Direct => Box::new(rv_net::dial_tcp(&host, port, &self.direct_dial).await?),
            Out::Block => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "dns: route blocked")),
        };
        match &s.up {
            Upstream::Https { path, .. } => crate::dns::doh(io, &host, port, path).await,
            Upstream::Tls { .. } => crate::dns::dot(io, &host).await,
            _ => Ok(io),
        }
    }

    async fn dns_exchange(&self, i: usize, query: &[u8]) -> Option<Vec<u8>> {
        let timeout = self.dns[i].timeout;
        tokio::time::timeout(timeout, async {
            let mut s = self.dns_stream(i).await?;
            let mut frame = (query.len() as u16).to_be_bytes().to_vec();
            frame.extend_from_slice(query);
            s.write_all(&frame).await?;
            s.flush().await?;
            let mut len = [0u8; 2];
            s.read_exact(&mut len).await?;
            let mut r = vec![0u8; u16::from_be_bytes(len) as usize];
            s.read_exact(&mut r).await?;
            io::Result::Ok(r)
        })
        .await
        .ok()
        .and_then(Result::ok)
    }

    pub async fn dns_direct(&self, server: SocketAddr, msg: &[u8]) -> Option<Vec<u8>> {
        if self.ipv4_only && server.is_ipv6() {
            return None;
        }
        let sock = rv_net::udp_socket(server.is_ipv6(), self.protect.as_ref()).ok()?;
        sock.connect(server).await.ok()?;
        sock.send(msg).await.ok()?;
        let mut buf = vec![0u8; 4096];
        let n = sock.recv(&mut buf).await.ok()?;
        buf.truncate(n);
        if buf.len() > 2
            && buf[2] & 0x02 != 0
            && let Some(full) = self.dns_direct_tcp(server, msg).await
        {
            return Some(full);
        }
        Some(buf)
    }

    async fn dns_direct_tcp(&self, server: SocketAddr, msg: &[u8]) -> Option<Vec<u8>> {
        let mut s = rv_net::dial_tcp_resolved(vec![server.ip()], server.port(), &self.direct_dial).await.ok()?;
        let mut frame = (msg.len() as u16).to_be_bytes().to_vec();
        frame.extend_from_slice(msg);
        s.write_all(&frame).await.ok()?;
        let mut len = [0u8; 2];
        s.read_exact(&mut len).await.ok()?;
        let mut r = vec![0u8; u16::from_be_bytes(len) as usize];
        s.read_exact(&mut r).await.ok()?;
        Some(r)
    }
}

fn server_refused(r: &crate::pipe::Relayed) -> bool {
    r.down == 0
        && r.eof != Some(crate::pipe::Side::Local)
        && r.error_side == Some(crate::pipe::Side::Remote)
        && r.error.as_ref().is_some_and(|e| e.to_string().contains("no response header"))
}

const REFUSED_RECENTLY: &str = "server refused this host recently";
const OWN_LOOP: &str = "own connection to own server came into the tunnel, dropped";

fn dial_held(e: &Error) -> bool {
    matches!(e, Error::Io(io) if io.get_ref().is_some_and(|i| i.to_string() == DIAL_HELD))
}

fn stall(r: &crate::pipe::Relayed, sent: u64) -> bool {
    r.down == 0
        && sent > 0
        && !r.idle
        && (r.eof == Some(crate::pipe::Side::Local) || r.error_side == Some(crate::pipe::Side::Local))
        && r.quiet >= STALL_QUIET
}

struct FirstData<'a, F: FnOnce()> {
    inner: &'a mut BoxStream,
    on: Option<F>,
}

impl<F: FnOnce() + Unpin> tokio::io::AsyncRead for FirstData<'_, F> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let before = buf.filled().len();
        let r = std::pin::Pin::new(&mut *self.inner).poll_read(cx, buf);
        if buf.filled().len() > before
            && let Some(f) = self.on.take()
        {
            f();
        }
        r
    }
}

impl<F: FnOnce() + Unpin> tokio::io::AsyncWrite for FirstData<'_, F> {
    fn poll_write(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, data: &[u8]) -> std::task::Poll<io::Result<usize>> {
        std::pin::Pin::new(&mut *self.inner).poll_write(cx, data)
    }
    fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut *self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut *self.inner).poll_shutdown(cx)
    }
}

fn streak_hit(m: &mut HashMap<String, RefusedHits>, key: &str, now: Instant, free: u32) -> (u32, Option<u32>) {
    if m.len() >= 256 && !m.contains_key(key) {
        m.retain(|_, h| now.saturating_duration_since(h.last) < REFUSED_LOG_EVERY);
        if m.len() >= 256 {
            m.clear();
        }
    }
    let h = match m.entry(key.to_string()) {
        std::collections::hash_map::Entry::Occupied(o) => {
            let h = o.into_mut();
            h.streak = if now.saturating_duration_since(h.last) < REFUSED_STREAK_GAP { h.streak + 1 } else { 0 };
            h.last = now;
            h
        }
        std::collections::hash_map::Entry::Vacant(v) => v.insert(RefusedHits { streak: 0, last: now, logged: None, quiet: 0 }),
    };
    let log = match h.logged {
        Some(t) if h.streak >= free && now.saturating_duration_since(t) < REFUSED_LOG_EVERY => {
            h.quiet += 1;
            None
        }
        _ => {
            h.logged = Some(now);
            Some(std::mem::take(&mut h.quiet))
        }
    };
    (h.streak, log)
}

fn empty_close(r: &crate::pipe::Relayed, sent: u64, took: Duration) -> bool {
    r.down == 0
        && sent > 0
        && !r.idle
        && r.eof != Some(crate::pipe::Side::Local)
        && (r.eof == Some(crate::pipe::Side::Remote) || r.error_side == Some(crate::pipe::Side::Remote))
        && took < EMPTY_SHORT
}

fn direct_silent(r: &crate::pipe::Relayed, sent: u64, took: Duration) -> bool {
    r.down == 0 && sent > 0 && !r.idle && took >= SMART_DIRECT_SILENT
}

fn refused_key(last: Option<&str>, host: &str, dst: SocketAddr) -> String {
    format!("{}|{}:{}", last.unwrap_or_default(), if host.is_empty() { dst.ip().to_string() } else { host.to_string() }, dst.port())
}

fn note_setup_failed() {
    rv_stats::add(rv_stats::Slot::ProxyFail, 1);
}

fn note_served(ok: bool) {
    rv_stats::add(rv_stats::Slot::ProxyConns, 1);
    if ok {
        rv_stats::add(rv_stats::Slot::ProxyOk, 1);
    }
}

fn udp_class(out: &Out) -> Option<u64> {
    match out {
        Out::Proxy(p) => Some(Rc::as_ptr(p) as u64),
        Out::Direct => Some(1),
        Out::Block => None,
    }
}

fn main_proxy(out: &Out, last: Option<&str>) -> bool {
    matches!(out, Out::Proxy(_)) && !last.is_some_and(|t| t.starts_with("rynet-app"))
}

fn offline(e: &Error) -> bool {
    fn io_offline(e: &io::Error) -> bool {
        use io::ErrorKind::*;
        matches!(e.kind(), NetworkUnreachable | NetworkDown | AddrNotAvailable)
            || e.get_ref().is_some_and(|inner| {
                inner.downcast_ref::<io::Error>().is_some_and(io_offline) || inner.downcast_ref::<Error>().is_some_and(offline)
            })
    }
    matches!(e, Error::Io(io) if io_offline(io))
}

fn udp_ctx(dst: SocketAddr, fake: bool, process: Option<&str>) -> RouteCtx<'_> {
    RouteCtx {
        domain: None,
        ip: (!fake).then(|| dst.ip()),
        port: dst.port(),
        network: Network::Udp,
        protocol: None,
        inbound_tag: None,
        process,
    }
}

fn family_allowed(f: (bool, bool), q: u16) -> bool {
    match q {
        crate::dns::TYPE_A => f.0,
        crate::dns::TYPE_AAAA => f.1,
        _ => true,
    }
}
fn dns_families(dns: Option<&Value>) -> (bool, bool) {
    let strategy =
        dns.and_then(|d| d.get("queryStrategy")).and_then(Value::as_str).unwrap_or("").to_ascii_lowercase().replace(['_', '-'], "");
    match strategy.as_str() {
        "useipv4" | "useip4" => (true, false),
        "useipv6" | "useip6" => (false, true),
        _ => (true, true),
    }
}

const REGEX_BUDGET: usize = 8 << 20;

const DNS_SERVERS_MAX: usize = 64;

fn parse_dns(dns: Option<&Value>, geo: &GeoData, warnings: &mut Vec<String>) -> Vec<DnsServer> {
    let mut out = Vec::new();
    let Some(servers) = dns.and_then(|d| d.get("servers")).and_then(Value::as_array) else {
        return out;
    };
    for s in servers {
        if out.len() >= DNS_SERVERS_MAX {
            warnings.push(format!("dns: only the first {DNS_SERVERS_MAX} servers are used, the rest are ignored"));
            break;
        }
        let (addr, port, domains) = match s {
            Value::String(a) => (a.as_str(), None, None),
            Value::Object(o) => {
                let port = o.get("port").filter(|p| !p.is_null());
                let parsed = port.and_then(|p| p.as_u64().or_else(|| p.as_str()?.trim().parse().ok())).and_then(|p| u16::try_from(p).ok());
                if port.is_some() && parsed.is_none() {
                    warnings.push(format!(
                        "dns server {}: bad port {}",
                        o.get("address").and_then(Value::as_str).unwrap_or(""),
                        port.unwrap_or(&Value::Null)
                    ));
                    continue;
                }
                (o.get("address").and_then(Value::as_str).unwrap_or(""), parsed, o.get("domains").cloned())
            }
            _ => continue,
        };
        let flag = |k: &str| s.get(k).and_then(Value::as_bool).unwrap_or(false);
        let (skip_fallback, final_query) = (flag("skipFallback"), flag("finalQuery"));
        let timeout = s.get("timeoutMs").and_then(Value::as_u64).filter(|&ms| ms > 0).map_or(DNS_TIMEOUT, Duration::from_millis);
        let Some((up, local)) = crate::dns::parse(addr, port) else {
            warnings.push(format!("dns server {addr:?} not supported"));
            continue;
        };
        let domains = match domains {
            Some(d) => {
                let routing = serde_json::json!({ "rules": [ { "type": "field", "domain": d, "outboundTag": "dns" } ] });
                match Router::new(&routing, geo) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        warnings.push(format!("dns domains: {e}"));
                        continue;
                    }
                }
            }
            None => None,
        };
        let expected = dns_filter(
            s.get("expectedIPs").filter(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty())).or_else(|| s.get("expectIPs")),
            geo,
            warnings,
        );
        let unexpected = dns_filter(s.get("unexpectedIPs"), geo, warnings);
        out.push(DnsServer {
            definition: s.clone(),
            outbound: None,
            up,
            local,
            domains,
            skip_fallback,
            final_query,
            timeout,
            families: dns_families(Some(s)),
            expected,
            unexpected,
        });
    }
    out
}

fn dns_filter(v: Option<&Value>, geo: &GeoData, warnings: &mut Vec<String>) -> Option<DnsFilter> {
    let mut ips: Vec<_> = match v? {
        Value::String(s) => s.split(',').map(str::to_string).collect(),
        Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => return None,
    };
    let prefer = ips.iter().any(|s| s == "*");
    ips.retain(|s| s != "*");
    if ips.is_empty() {
        return None;
    }
    let config = serde_json::json!({"rules": [{"ip": ips, "outboundTag": "dns"}]});
    match Router::new(&config, geo) {
        Ok(rules) => {
            warnings.extend(rules.warnings().iter().cloned());
            Some(DnsFilter { rules, prefer })
        }
        Err(e) => {
            warnings.push(format!("dns IP filter: {e}"));
            None
        }
    }
}

enum Hosted {
    Ips(Vec<IpAddr>),
    Rcode(u8),
    Alias(String),
}

fn parse_hosts(dns: Option<&Value>, geo: &GeoData, warnings: &mut Vec<String>) -> Vec<(Router, Hosted)> {
    let Some(hosts) = dns.and_then(|d| d.get("hosts")).and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, v) in hosts {
        let values: Vec<&str> = match v {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => continue,
        };
        let ips: Vec<IpAddr> = values.iter().filter_map(|s| s.parse().ok()).collect();
        let code = values.first().and_then(|v| v.strip_prefix('#')).and_then(|c| c.parse::<u8>().ok()).filter(|c| *c < 16);
        let hosted = match code {
            Some(c) => Hosted::Rcode(c),
            None if ips.is_empty() && values.len() == 1 => Hosted::Alias(values[0].to_ascii_lowercase()),
            None => Hosted::Ips(ips),
        };
        let pattern = if key.contains(':') { key.clone() } else { format!("full:{key}") };
        let rule = serde_json::json!({ "rules": [ { "type": "field", "domain": [pattern], "outboundTag": "hosts" } ] });
        match Router::new(&rule, geo) {
            Ok(r) => out.push((r, hosted)),
            Err(e) => warnings.push(format!("dns hosts {key:?}: {e}")),
        }
    }
    out
}

async fn direct_udp(
    first: (SocketAddr, Vec<u8>),
    rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
    reply: &dyn Fn(SocketAddr, &[u8]),
    protect: Option<&Protect>,
) -> io::Result<()> {
    let sock = rv_net::udp_socket(first.0.is_ipv6(), protect)?;
    sock.send_to(&first.1, first.0).await?;
    let idle = tokio::time::sleep(crate::UDP_IDLE);
    tokio::pin!(idle);
    let mut last = tokio::time::Instant::now();
    loop {
        tokio::select! {
            p = rx.recv() => {
                let Some((dst, data)) = p else { return Ok(()) };
                let _ = sock.send_to(&data, dst).await;
                crate::udp_touch(&mut last, idle.as_mut(), crate::UDP_IDLE);
            }
            r = sock.readable() => {
                r?;
                let got = crate::with_datagram_buf(|buf| match sock.try_recv_from(buf) {
                    Ok((n, from)) => {
                        reply(from, &buf[..n]);
                        Ok(true)
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false),
                    Err(e) => Err(e),
                })?;
                if got {
                    crate::udp_touch(&mut last, idle.as_mut(), crate::UDP_IDLE);
                }
            }
            _ = &mut idle => {
                let deadline = last + crate::UDP_IDLE;
                if tokio::time::Instant::now() >= deadline {
                    return Ok(());
                }
                idle.as_mut().reset(deadline);
            }
        }
    }
}

pub(crate) async fn first_ok<T, E, F: std::future::Future<Output = Result<T, E>>>(futs: impl Iterator<Item = F>) -> Option<Result<T, E>> {
    let mut futs: Vec<_> = futs.map(|f| Some(Box::pin(f))).collect();
    if futs.is_empty() {
        return None;
    }
    let mut errs: Vec<Option<E>> = futs.iter().map(|_| None).collect();
    let r = std::future::poll_fn(|cx| {
        let mut pending = false;
        for (i, slot) in futs.iter_mut().enumerate() {
            let Some(f) = slot else { continue };
            match f.as_mut().poll(cx) {
                std::task::Poll::Ready(Ok(v)) => return std::task::Poll::Ready(Some((i, v))),
                std::task::Poll::Ready(Err(e)) => {
                    errs[i] = Some(e);
                    *slot = None;
                }
                std::task::Poll::Pending => pending = true,
            }
        }
        if pending { std::task::Poll::Pending } else { std::task::Poll::Ready(None) }
    })
    .await;
    Some(match r {
        Some((_, v)) => Ok(v),
        None => Err(errs.into_iter().flatten().next()?),
    })
}

fn dns_give_up(
    fallback: Option<(tokio::time::Instant, usize)>,
    all_asked: bool,
    mut pending: impl Iterator<Item = usize>,
    hedge_at: tokio::time::Instant,
) -> Option<tokio::time::Instant> {
    let (at, asked_before) = fallback?;
    (all_asked && pending.all(|k| k < asked_before)).then(|| (at + DNS_HEDGE).max(hedge_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ipv4_only_blocks_ipv6_routes_dns_and_dial_before_reading() {
        let mut profile = profile();
        profile.dns = Some(serde_json::json!({"hosts":{"v4.test":["192.0.2.9","2001:db8::9"]}, "servers":["[2001:db8::53]", "1.1.1.1"]}));
        let options = Options {
            ipv4_only: true,
            routing: RoutingMode::Global,
            system_dns: std::sync::Arc::new(std::sync::Mutex::new(vec![
                "[2001:db8::53]:53".parse().unwrap(),
                "192.0.2.53:53".parse().unwrap(),
            ])),
            ..Options::default()
        };
        let e = Engine::new(&profile, &GeoData::default(), None, &options).unwrap();
        let query = crate::dns::query_for("v4.test", crate::dns::TYPE_AAAA);
        let DnsTarget::Reply(reply) = e.dns_route(&query) else { panic!("AAAA must be answered locally") };
        assert!(crate::dns::addresses(&reply).is_empty());
        assert_eq!(e.resolve_host("v4.test").await.unwrap(), "192.0.2.9".parse::<IpAddr>().unwrap());
        assert_eq!(e.system_server(), "192.0.2.53:53".parse().unwrap());
        assert!(e.dns.iter().all(|s| !s.up.endpoint().0.parse::<IpAddr>().is_ok_and(|ip| ip.is_ipv6())));
        assert!(e.direct_nets().iter().all(|(ip, _)| ip.is_ipv4()));
        let destination = "[2001:db8::1]:443".parse().unwrap();
        assert!(!e.udp_allowed(destination));
        let (local, _peer) = tokio::io::duplex(32);
        assert!(matches!(e.tcp(local, destination).await, Err(Error::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied));
        assert!(matches!(e.direct_dial.family, rv_net::Family::V4 { force: true }));
        let (tx, mut rx) = mpsc::channel(1);
        tx.send((destination, vec![1])).await.unwrap();
        assert!(e.udp(&mut rx, &|_, _| panic!("IPv6 must not reply")).await.is_err());
        assert!(e.dns_direct(destination, &[]).await.is_none());
        let query = crate::dns::query_for("unseen.test", crate::dns::TYPE_AAAA);
        let DnsTarget::Reply(reply) = e.dns_route(&query) else { panic!("AAAA must not reach an upstream") };
        assert!(crate::dns::addresses(&reply).is_empty());
    }

    #[test]
    fn dns_server_port_out_of_range_is_skipped_with_a_warning() {
        let mut w = Vec::new();
        let dns = serde_json::json!({"servers": [{"address": "1.1.1.1", "port": 70000}, {"address": "8.8.8.8", "port": 5353},
            {"address": "9.9.9.9", "port": "53"}, {"address": "1.0.0.1", "port": "abc"}]});
        let s = parse_dns(Some(&dns), &GeoData::default(), &mut w);
        assert_eq!(s.len(), 2);
        assert!(w.iter().any(|w| w.contains("70000")), "{w:?}");
    }

    fn profile() -> Profile {
        let text = r#"[{"remarks": "t", "outbounds": [
            {"tag": "proxy", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.1", "port": 443,
                "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]},
             "streamSettings": {"network": "tcp", "security": "tls", "tlsSettings": {"serverName": "example.com"}}},
            {"tag": "direct", "protocol": "freedom"}
        ]}]"#;
        rv_config::parse_subscription(text).unwrap().remove(0)
    }

    fn first(t: DnsTarget) -> DnsHop {
        let DnsTarget::Servers(hops) = t else { panic!("local reply") };
        hops[0].0.clone()
    }

    fn labels(e: &Engine, name: &str) -> Vec<String> {
        let DnsTarget::Servers(hops) = e.dns_route(&crate::dns::query_for(name, crate::dns::TYPE_A)) else { panic!() };
        hops.iter()
            .map(|(h, _)| match h {
                DnsHop::Stream(i) => e.dns_label(*i),
                DnsHop::Direct(a) => a.to_string(),
            })
            .collect()
    }

    fn engine(routing: RoutingMode) -> Engine {
        let opts = Options {
            user_rules: String::new(),
            routing,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap()
    }

    #[test]
    fn udp_owner_cache_stays_cheap_and_bounded() {
        let owner: Owner = std::sync::Arc::new(|_, _, _| Some("app".into()));
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Global,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: Some(owner),
            log_apps: true,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        let dst: SocketAddr = "1.1.1.1:53".parse().unwrap();
        let t0 = Instant::now();
        for i in 0..40_000u32 {
            let src = SocketAddr::from(([10, 0, (i >> 8) as u8, i as u8], 1024 + (i % 50_000) as u16));
            assert_eq!(e.udp_process(Some(src), dst).as_deref(), Some("app"));
        }
        assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
        assert!(e.udp_owners.borrow().len() <= UDP_OWNERS_MAX);
    }

    #[test]
    fn refusals_during_an_active_block_do_not_double_it() {
        let e = Rc::new(engine(RoutingMode::Global));
        e.proxy_alive.set(Some(Instant::now()));
        let k = "proxy|burst.example:443";
        for _ in 0..5 {
            e.note_refused(k, true);
        }
        assert!(e.refused_now(k));
        assert_eq!(e.refused.borrow()[k].ttl, REFUSED_TTL);
        e.refused.borrow_mut().get_mut(k).unwrap().until = Some(Instant::now());
        e.note_refused(k, true);
        assert_eq!(e.refused.borrow()[k].ttl, REFUSED_TTL * 2);
    }

    #[test]
    fn refused_host_skipped_after_two_refusals() {
        let e = Rc::new(engine(RoutingMode::Global));
        let k = "proxy|federatedcompute-pa.googleapis.com:443";
        e.note_refused(k, true);
        e.note_refused(k, true);
        assert!(!e.refused_now(k), "без живого сервера (неверный UUID) хосты не блокируются");
        e.proxy_alive.set(Some(Instant::now()));
        e.note_refused(k, true);
        assert!(!e.refused_now(k));
        e.note_refused(k, true);
        assert!(e.refused_now(k));
        assert!(!e.refused_now("proxy|example.com:443"));
        let ttl = |e: &Engine| e.refused.borrow()[k].ttl;
        assert_eq!(ttl(&e), REFUSED_TTL);
        e.refused.borrow_mut().get_mut(k).unwrap().until = Some(Instant::now());
        assert!(!e.refused_now(k), "срок вышел: одна попытка идёт к серверу");
        e.note_refused(k, true);
        assert!(e.refused_now(k), "повторный отказ блокирует сразу");
        assert_eq!(ttl(&e), REFUSED_TTL * 2);
        e.note_refused(k, false);
        assert!(!e.refused_now(k) && e.refused.borrow().is_empty());
        e.note_refused(k, true);
        e.note_refused(k, true);
        e.reset_pools();
        assert!(!e.refused_now(k));
    }

    #[test]
    fn refused_hits_back_off_per_app_and_collapse_log_lines() {
        let e = engine(RoutingMode::Global);
        let t0 = Instant::now();
        let k = "org.telegram.messenger|proxy|149.154.167.51:443";
        let ms = |n| t0 + Duration::from_millis(n);
        assert_eq!(e.refused_hit(k, t0), (REFUSED_HOLD, Some(0)));
        assert_eq!(e.refused_hit(k, ms(1)), (REFUSED_HOLD * 2, None));
        assert_eq!(e.refused_hit(k, ms(2)), (REFUSED_HOLD_MAX, None));
        assert_eq!(e.refused_hit(k, ms(3)), (REFUSED_HOLD_MAX, None));
        assert_eq!(e.refused_hit("com.android.chrome|proxy|149.154.167.51:443", ms(4)), (REFUSED_HOLD, Some(0)));
        let mut t = 3;
        while t < 31_000 {
            t += 2_000;
            let (hold, log) = e.refused_hit(k, ms(t));
            assert_eq!(hold, REFUSED_HOLD_MAX);
            if t < REFUSED_LOG_EVERY.as_millis() as u64 {
                assert_eq!(log, None);
            } else {
                assert_eq!(log, Some(17), "3 подряд и 14 за 30 с");
                break;
            }
        }
        assert_eq!(e.refused_hit(k, ms(t + 60_000)), (REFUSED_HOLD, Some(0)));
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn refused_from_cache_is_held_and_counted_as_a_failed_attempt() {
        use tokio::io::AsyncWriteExt;
        let e = engine(RoutingMode::Global);
        let dst: SocketAddr = "149.154.167.51:443".parse().unwrap();
        let ctx = RouteCtx {
            domain: None,
            ip: Some(dst.ip()),
            port: dst.port(),
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        let (_, trace) = e.route_tagged(&ctx);
        e.proxy_alive.set(Some(Instant::now()));
        let k = refused_key(trace.last.as_deref(), "", dst);
        e.note_refused(&k, true);
        e.note_refused(&k, true);
        assert!(e.refused_now(&k));
        let attempt = |held: usize| {
            e.refused_held.set(held);
            let (mut app, local) = tokio::io::duplex(1024);
            let e = &e;
            async move {
                app.write_all(&[0xef; 64]).await.unwrap();
                let t = tokio::time::Instant::now();
                let r = e.tcp(local, dst).await;
                let err = r.expect_err("отказ");
                assert!(err.to_string().contains(REFUSED_RECENTLY), "{err}");
                t.elapsed()
            }
        };
        let fails = rv_stats::get(rv_stats::Slot::ProxyFail);
        assert!(attempt(0).await >= REFUSED_HOLD);
        assert!(attempt(0).await >= REFUSED_HOLD * 2, "подряд — дольше");
        assert!(rv_stats::get(rv_stats::Slot::ProxyFail) >= fails + 2);
        assert_eq!(e.refused_held.get(), 0, "держатель отпущен");
        assert!(attempt(REFUSED_HELD_MAX).await < Duration::from_millis(100), "держим не больше REFUSED_HELD_MAX сокетов");
        e.note_refused(&k, false);
        assert!(!e.refused_now(&k));
    }

    #[test]
    fn empty_closes_back_off_per_app_and_recover_on_data() {
        let e = engine(RoutingMode::Global);
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let k = "org.telegram.messenger|proxy|149.154.167.51:443";
        let other = "com.android.chrome|proxy|149.154.167.51:443";
        assert_eq!(e.empty_hold(k, t0), None);
        for i in 0..EMPTY_FREE as u64 {
            assert_eq!(e.empty_hold(k, ms(i)), None, "первые {EMPTY_FREE} без задержки");
            assert_eq!(e.empty_hit(k, ms(i)), Some(0), "и с полной строкой лога");
        }
        assert_eq!(e.empty_hold(k, ms(10)), Some(REFUSED_HOLD));
        assert_eq!(e.empty_hit(k, ms(10)), None, "дальше строки схлопываются");
        assert_eq!(e.empty_hold(k, ms(1_100)), Some(REFUSED_HOLD * 2));
        assert_eq!(e.empty_hit(k, ms(1_100)), None);
        assert_eq!(e.empty_hold(k, ms(3_200)), Some(REFUSED_HOLD_MAX));
        assert_eq!(e.empty_hit(k, ms(3_200)), None);
        assert_eq!(e.empty_hit(k, ms(31_000)), Some(3), "дальше строка с числом пропущенных");
        assert_eq!(e.empty_hold(other, ms(31_000)), None, "другое приложение — свой счёт");
        e.empty_hit(other, ms(31_000));
        assert_eq!(e.empty_hold(k, ms(45_000)), None);
        for i in 0..5 {
            e.empty_hit(k, ms(50_000 + i));
        }
        assert!(e.empty_hold(k, ms(50_010)).is_some());
        e.empty_clear("proxy|149.154.167.51:444");
        assert!(e.empty_hold(k, ms(50_010)).is_some(), "другой порт — другой хост");
        e.empty_clear("proxy|149.154.167.51:443");
        assert_eq!(e.empty_hold(k, ms(50_010)), None, "данные к хосту: сразу без задержки");
        assert!(e.empty_hits.borrow().is_empty());
    }

    #[test]
    fn empty_close_is_only_a_short_silent_close_by_the_server() {
        use crate::pipe::{Relayed, Side};
        let r = |down, eof, error_side, idle| Relayed {
            up: 64,
            down,
            idle,
            error: None,
            error_side,
            eof,
            quiet: Duration::ZERO,
            gap: Duration::ZERO,
        };
        let fast = Duration::from_millis(5);
        assert!(empty_close(&r(0, Some(Side::Remote), None, false), 64, fast), "hy2: поток закрыт, ни байта");
        assert!(empty_close(&r(0, None, Some(Side::Remote), false), 64, fast), "сервер оборвал");
        assert!(!empty_close(&r(10, Some(Side::Remote), None, false), 64, fast), "ответ был");
        assert!(!empty_close(&r(0, Some(Side::Local), None, false), 64, fast), "закрыло приложение");
        assert!(!empty_close(&r(0, Some(Side::Remote), None, false), 0, fast), "приложение ничего не слало");
        assert!(!empty_close(&r(0, Some(Side::Remote), None, true), 64, fast), "закрыто по простою");
        assert!(!empty_close(&r(0, Some(Side::Remote), None, false), 64, EMPTY_SHORT), "долгое соединение");
    }

    #[test]
    fn silent_direct_connection_counts_as_blocked() {
        use crate::pipe::{Relayed, Side};
        let r = |down, eof, error_side, idle| Relayed {
            up: 517,
            down,
            idle,
            error: None,
            error_side,
            eof,
            quiet: Duration::ZERO,
            gap: Duration::ZERO,
        };
        let long = Duration::from_secs(25);
        assert!(direct_silent(&r(0, Some(Side::Local), None, false), 517, long), "приложение сдалось без ответа");
        assert!(direct_silent(&r(0, None, Some(Side::Remote), false), 517, SMART_DIRECT_SILENT), "сайт оборвал после молчания");
        assert!(!direct_silent(&r(0, Some(Side::Local), None, false), 517, Duration::from_secs(2)), "приложение сразу закрыло само");
        assert!(!direct_silent(&r(1, Some(Side::Local), None, false), 517, long), "ответ был");
        assert!(!direct_silent(&r(0, Some(Side::Local), None, false), 0, long), "приложение ничего не слало");
        assert!(!direct_silent(&r(0, None, None, true), 517, long), "закрыто по простою");
    }

    #[test]
    fn owner_lookup_only_for_rules_while_quiet() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = calls.clone();
        let owner: Owner = std::sync::Arc::new(move |_, _, _| {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some("app".into())
        });
        let opts = Options { routing: RoutingMode::Global, owner: Some(owner), log_apps: true, ..Default::default() };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        let (src, dst) = ("10.0.0.2:5000".parse().unwrap(), "1.1.1.1:443".parse().unwrap());
        assert_eq!(e.process_when(false, true, Some(src), dst).as_deref(), Some("app"));
        assert_eq!(e.process_when(true, true, Some(src), dst), None, "экран выключен: владелец только ради лога не ищется");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let rules = Options { user_rules: "PROCESS-NAME,app,DIRECT".into(), routing: RoutingMode::Custom, ..opts };
        let e = Engine::new(&profile(), &GeoData::default(), None, &rules).unwrap();
        assert_eq!(e.process_when(true, true, Some(src), dst).as_deref(), Some("app"), "правилам по приложениям владелец нужен всегда");
    }

    #[test]
    fn refusal_counts_only_when_the_server_closed_first() {
        use crate::pipe::{Relayed, Side};
        let relayed = |eof, side, msg: &str| Relayed {
            up: 100,
            down: 0,
            idle: false,
            error: Some(io::Error::new(io::ErrorKind::UnexpectedEof, msg.to_string())),
            error_side: side,
            eof,
            quiet: Duration::ZERO,
            gap: Duration::ZERO,
        };
        assert!(server_refused(&relayed(None, Some(Side::Remote), "vless: no response header")));
        assert!(!server_refused(&relayed(Some(Side::Local), Some(Side::Remote), "vless: no response header")), "приложение закрыло первым");
        assert!(!server_refused(&relayed(None, Some(Side::Local), "vless: no response header")));
        assert!(!server_refused(&relayed(None, Some(Side::Remote), "connection reset")));
        assert!(!server_refused(&Relayed { down: 1, ..relayed(None, Some(Side::Remote), "vless: no response header") }));
    }

    #[test]
    fn connection_task_does_not_keep_dial_state() {
        let e = engine(RoutingMode::Global);
        let (_app, local) = tokio::io::duplex(1024);
        let size = std::mem::size_of_val(&e.tcp(local, "1.2.3.4:443".parse().unwrap()));
        assert!(size < 8 * 1024, "future tcp_from: {size} байт");
    }

    #[test]
    fn served_counted_for_main_proxy_only() {
        use rv_stats::{Slot, get};
        let e = engine(RoutingMode::Global);
        let (out, trace) = e.route_tagged(&udp_ctx("1.1.1.1:443".parse().unwrap(), false, None));
        assert!(main_proxy(&out, trace.last.as_deref()));
        assert!(!main_proxy(&out, Some("rynet-app-1")));
        assert!(!main_proxy(&Out::Direct, None));
        let (c, o) = (get(Slot::ProxyConns), get(Slot::ProxyOk));
        note_served(false);
        note_served(true);
        assert!(get(Slot::ProxyConns) >= c + 2 && get(Slot::ProxyOk) > o);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn name_too_long_for_a_dns_query_is_not_sent() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let frames = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = frames.clone();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut len = [0u8; 2];
            while s.read_exact(&mut len).await.is_ok() {
                let mut msg = vec![0u8; u16::from_be_bytes(len) as usize];
                if s.read_exact(&mut msg).await.is_err() {
                    break;
                }
                seen.lock().unwrap().push(msg.len());
            }
        });
        let config = serde_json::json!({"outbounds": [{"tag": "direct", "protocol": "freedom"}], "dns": {"servers": [
            {"address": format!("tcp://127.0.0.1:{port}"), "timeoutMs": 500}
        ]}});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let name = vec!["a".repeat(63); 1100].join(".");
        assert!(e.resolve_addresses(&name).await.is_err());
        tokio::time::sleep(Duration::from_millis(100)).await;
        let frames = frames.lock().unwrap();
        assert!(frames.is_empty(), "the server got frames of {:?} bytes", &frames[..frames.len().min(8)]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn trailing_dot_name_resolves_like_the_bare_name() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = asked.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut len = [0u8; 2];
                    while s.read_exact(&mut len).await.is_ok() {
                        let mut q = vec![0u8; u16::from_be_bytes(len) as usize];
                        if s.read_exact(&mut q).await.is_err() {
                            break;
                        }
                        let Some(qs) = crate::dns::question(&q) else { break };
                        seen.lock().unwrap().push(qs.name.clone());
                        let ips = if qs.qtype == crate::dns::TYPE_A { vec![IpAddr::from([192, 0, 2, 7])] } else { Vec::new() };
                        let r = crate::dns::answer(&q, &qs, &ips, 60);
                        let mut out = (r.len() as u16).to_be_bytes().to_vec();
                        out.extend_from_slice(&r);
                        if s.write_all(&out).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        let config = serde_json::json!({"outbounds": [{"tag": "direct", "protocol": "freedom"}], "dns": {"servers": [
            {"address": format!("tcp://127.0.0.1:{port}"), "timeoutMs": 500}
        ]}});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        assert_eq!(e.resolve_addresses("a.example.").await.unwrap(), vec![IpAddr::from([192, 0, 2, 7])]);
        let asked = asked.lock().unwrap();
        assert!(!asked.is_empty() && asked.iter().all(|n| n == "a.example"), "{asked:?}");
    }

    #[tokio::test]
    async fn truncated_direct_dns_retried_over_tcp() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let udp = tokio::net::UdpSocket::bind(addr).await.unwrap();
        let q = crate::dns::query_for("big.test", crate::dns::TYPE_A);
        let qs = crate::dns::question(&q).unwrap();
        let ips: Vec<IpAddr> = (1..=40u8).map(|i| IpAddr::from([10, 0, 0, i])).collect();
        let full = crate::dns::answer(&q, &qs, &ips, 60);
        let mut cut = crate::dns::answer(&q, &qs, &[], 60);
        cut[2] |= 0x02;
        tokio::spawn(async move {
            let mut b = [0u8; 512];
            let (_, from) = udp.recv_from(&mut b).await.unwrap();
            udp.send_to(&cut, from).await.unwrap();
        });
        let reply = full.clone();
        tokio::spawn(async move {
            let (mut s, _) = tcp.accept().await.unwrap();
            let mut len = [0u8; 2];
            s.read_exact(&mut len).await.unwrap();
            s.read_exact(&mut vec![0u8; u16::from_be_bytes(len) as usize]).await.unwrap();
            let mut out = (reply.len() as u16).to_be_bytes().to_vec();
            out.extend_from_slice(&reply);
            s.write_all(&out).await.unwrap();
        });
        let e = engine(RoutingMode::Global);
        assert_eq!(e.dns_direct(addr, &q).await.unwrap(), full);
    }

    #[test]
    fn smart_rules_json_has_ru_lists() {
        let v = smart_rules_json();
        let text = v.to_string();
        assert!(text.contains("geosite:category-ru") && text.contains("geoip:ru") && text.contains("geoip:private"));
        assert_eq!(v["private"].as_array().map(|a| a.len()), Some(LAN_RANGES.len()));
    }

    #[test]
    fn smart_mode_dns() {
        let e = engine(RoutingMode::Smart);
        for q in ["ya.ru", "mail.ru"] {
            let DnsHop::Stream(i) = first(e.dns_route(&crate::dns::query_for(q, crate::dns::TYPE_A))) else { panic!() };
            assert_eq!(e.dns_label(i), SMART_RU_DNS);
        }
        assert_eq!(
            first(e.dns_route(&crate::dns::query_for("google.com", crate::dns::TYPE_A))),
            DnsHop::Direct(DEFAULT_DNS.parse().unwrap())
        );
        let g = engine(RoutingMode::Global);
        assert_eq!(first(g.dns_route(&crate::dns::query_for("ya.ru", crate::dns::TYPE_A))), DnsHop::Direct(DEFAULT_DNS.parse().unwrap()));
    }

    #[test]
    fn direct_mode_sends_only_own_rules_through_the_proxy() {
        let opts = Options { user_rules: "DOMAIN-SUFFIX,youtube.com,PROXY".into(), routing: RoutingMode::Direct, ..Options::default() };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        let route = |domain| {
            e.route_tagged(&RouteCtx {
                domain: Some(domain),
                ip: None,
                port: 443,
                network: Network::Tcp,
                protocol: None,
                inbound_tag: None,
                process: None,
            })
            .0
        };
        assert!(matches!(route("www.youtube.com"), Out::Proxy(_)));
        assert!(matches!(route("example.com"), Out::Direct));
        assert!(e.smart_fallback.is_none());
        assert_eq!(RoutingMode::from_name("direct"), Some(RoutingMode::Direct));
    }

    #[test]
    fn smart_direct_falls_back_to_proxy() {
        let e = Rc::new(engine(RoutingMode::Smart));
        assert!(matches!(e.smart_fallback, Some(Out::Proxy(_))));
        let ctx = RouteCtx {
            domain: Some("ya.ru"),
            ip: None,
            port: 443,
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        let (out, trace) = e.route_tagged(&ctx);
        assert!(matches!(out, Out::Direct));
        assert_eq!(trace.last.as_deref(), Some(SMART_DIRECT));
        let opts = Options {
            user_rules: "DOMAIN-SUFFIX,ya.ru,DIRECT".into(),
            routing: RoutingMode::Smart,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let own = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        assert_ne!(own.route_tagged(&ctx).1.last.as_deref(), Some(SMART_DIRECT));
        assert!(engine(RoutingMode::Global).smart_fallback.is_none());

        assert!(!e.direct_failed_recently("ya.ru"));
        e.remember_direct_failure("ya.ru".into());
        assert!(e.direct_failed_recently("ya.ru"));
        e.reset_pools();
        assert!(!e.direct_failed_recently("ya.ru"));
        assert!(direct_blocked(&io::Error::new(io::ErrorKind::TimedOut, "TCP dial timed out")));
        assert!(!direct_blocked(&io::Error::new(io::ErrorKind::InvalidData, "tls")));
        assert!(direct_blocked(&io::Error::new(io::ErrorKind::ConnectionRefused, "os error 10061")), "RST на SYN бывает и блокировкой");
    }

    #[test]
    fn failing_streak_signals_once_per_series() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c = calls.clone();
        let opts = Options {
            routing: RoutingMode::Global,
            on_failing: Some(std::sync::Arc::new(move || {
                c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })),
            ..Default::default()
        };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        let n = || calls.load(std::sync::atomic::Ordering::SeqCst);
        let t0 = Instant::now();
        for i in 0..FAILING_STREAK * 10 {
            e.note_proxy_at(false, t0 + Duration::from_millis(u64::from(i) * 10));
        }
        assert_eq!(n(), 0);
        e.note_proxy_at(true, t0 + Duration::from_secs(1));
        let t1 = t0 + Duration::from_secs(2);
        for _ in 0..FAILING_STREAK {
            e.note_proxy_at(false, t1);
        }
        assert_eq!(n(), 0);
        let late = t1 + FAILING_SPAN;
        e.note_proxy_at(false, late);
        assert_eq!(n(), 1);
        for _ in 0..FAILING_STREAK * 4 - 1 {
            e.note_proxy_at(false, late);
        }
        assert_eq!(n(), 1);
        e.note_proxy_at(false, late);
        assert_eq!(n(), 2);
        e.note_proxy_at(true, late);
        for _ in 0..FAILING_STREAK * 2 {
            e.note_proxy_at(false, late + Duration::from_secs(1));
        }
        assert_eq!(n(), 2);
    }

    #[test]
    fn happ_routing_for_link_profiles() {
        let p =
            rv_config::parse_subscription("vless://9205c503-358f-4d8d-a081-20c98eddf64a@203.0.113.1:443?security=tls#t").unwrap().remove(0);
        let happ =
            serde_json::json!({ "GlobalProxy": "false", "ProxySites": ["domain:blocked.example"], "BlockSites": ["domain:ads.example"] });
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Subscription,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: Some(happ),
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        let ctx =
            |d| RouteCtx { domain: Some(d), ip: None, port: 443, network: Network::Tcp, protocol: None, inbound_tag: None, process: None };
        assert!(matches!(e.route(&ctx("www.blocked.example")), Out::Proxy(_)));
        assert!(matches!(e.route(&ctx("ads.example")), Out::Block));
        assert!(matches!(e.route(&ctx("other.example")), Out::Direct));
        assert_eq!(e.routing_summary, "source=happ global_proxy=false rules=3 last=direct default=proxy proxy=proxy");
        let g = Options { routing: RoutingMode::Global, ..opts };
        let e = Engine::new(&p, &GeoData::default(), None, &g).unwrap();
        assert!(matches!(e.route(&ctx("other.example")), Out::Proxy(_)));
    }

    #[test]
    fn broken_first_outbound_does_not_leak() {
        let text = r#"[{"remarks": "t", "outbounds": [
            {"tag": "proxy", "protocol": "wireguard", "settings": {}},
            {"tag": "direct", "protocol": "freedom"}
        ], "routing": {"rules": [{"type": "field", "port": "8080", "outboundTag": "nope"}]}}]"#;
        let p = rv_config::parse_subscription(text).unwrap().remove(0);
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Subscription,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        let ctx = |port| RouteCtx {
            domain: Some("example.com"),
            ip: None,
            port,
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        assert!(matches!(e.route(&ctx(443)), Out::Block));
        assert!(matches!(e.route(&ctx(8080)), Out::Block));
    }

    #[test]
    fn balancer_fallback_through_loopback() {
        let text = r#"[{"remarks": "t", "outbounds": [
            {"tag": "proxy", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.1", "port": 443,
                "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}},
            {"tag": "proxy-2", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.2", "port": 443,
                "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}},
            {"tag": "direct", "protocol": "freedom"},
            {"tag": "fallback-router", "protocol": "loopback", "settings": {"inboundTag": "fallback-in"}}
        ], "routing": {
            "balancers": [
                {"tag": "primary-first", "selector": ["primary"], "strategy": {"type": "leastPing"}, "fallbackTag": "fallback-router"},
                {"tag": "auto", "selector": ["proxy"], "strategy": {"type": "leastPing"}, "fallbackTag": "proxy"},
                {"tag": "empty", "selector": ["none"], "fallbackTag": "direct"},
                {"tag": "looped", "selector": ["none"], "fallbackTag": "fallback-router"}
            ],
            "rules": [
                {"type": "field", "port": "8080", "balancerTag": "empty"},
                {"type": "field", "inboundTag": ["fallback-in"], "port": "9090", "balancerTag": "looped"},
                {"type": "field", "inboundTag": ["fallback-in"], "balancerTag": "auto"},
                {"type": "field", "network": "tcp,udp", "balancerTag": "primary-first"}
            ]}}]"#;
        let p = rv_config::parse_subscription(text).unwrap().remove(0);
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Subscription,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        let ctx = |port| RouteCtx {
            domain: Some("example.com"),
            ip: None,
            port,
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        let (out, t) = e.route_tagged(&ctx(443));
        assert!(matches!(out, Out::Proxy(_)));
        assert_eq!(t.path, "balancer:primary-first>fallback-router>balancer:auto");
        let (out, t) = e.route_tagged(&ctx(8080));
        let path = t.path;
        assert!(matches!(out, Out::Direct), "{path}");
        let (out, t) = e.route_tagged(&ctx(9090));
        let path = t.path;
        assert!(matches!(out, Out::Block), "{path}");
    }

    #[test]
    fn mux_rejects_udp443() {
        let text = r#"[{"remarks": "t", "outbounds": [
            {"tag": "proxy", "protocol": "vless", "mux": {"enabled": true, "concurrency": -1, "xudpProxyUDP443": "reject"},
             "settings": {"vnext": [{"address": "203.0.113.1", "port": 443,
                "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}}
        ]}]"#;
        let p = rv_config::parse_subscription(text).unwrap().remove(0);
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Global,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        assert_eq!(e.udp_route(None, "8.8.8.8:443".parse().unwrap()), None);
        assert!(e.udp_route(None, "8.8.8.8:3478".parse().unwrap()).is_some());
        let e = engine(RoutingMode::Global);
        assert!(e.udp_route(None, "8.8.8.8:443".parse().unwrap()).is_some());
    }

    #[test]
    fn udp_flow_keeps_its_balancer_outbound_while_active() {
        let mut p = profile();
        for t in ["a", "b", "c"] {
            let mut o = p.outbounds[0].clone();
            o.tag = t.into();
            p.outbounds.push(o);
        }
        p.routing =
            Some(serde_json::json!({"balancers": [{"tag": "auto", "selector": ["a", "b", "c"], "strategy": {"type": "roundRobin"}}],
            "rules": [{"network": "tcp,udp", "balancerTag": "auto"}]}));
        let e = Engine::new(&p, &GeoData::default(), None, &Options { routing: RoutingMode::Subscription, ..Default::default() }).unwrap();
        let (src, dst): (SocketAddr, SocketAddr) = ("10.0.0.2:40000".parse().unwrap(), "1.2.3.4:3478".parse().unwrap());
        let first = e.udp_route(Some(src), dst);
        assert!(first.is_some());
        for _ in 0..30 {
            for (_, at) in e.udp_routes.borrow_mut().values_mut() {
                *at -= UDP_ROUTE_TTL * 6 / 10;
            }
            assert_eq!(e.udp_route(Some(src), dst), first);
            assert_eq!(udp_class(&e.udp_session_out(Some(src), dst, None).0), first);
        }
    }

    #[tokio::test]
    async fn failed_udp_session_reroutes_its_flow() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
                let mut p = profile();
                p.outbounds[0].stream = Default::default();
                if let OutboundKind::Vless(v) = &mut p.outbounds[0].kind {
                    v.address = "127.0.0.1".into();
                    v.port = closed;
                }
                let e =
                    Engine::new(&p, &GeoData::default(), None, &Options { routing: RoutingMode::Global, ..Default::default() }).unwrap();
                let (src, dst): (SocketAddr, SocketAddr) = ("10.0.0.2:40000".parse().unwrap(), "1.2.3.4:3478".parse().unwrap());
                assert!(e.udp_route(Some(src), dst).is_some());
                let (tx, mut rx) = mpsc::channel(4);
                tx.send((dst, b"x".to_vec())).await.unwrap();
                let r = tokio::time::timeout(Duration::from_secs(10), e.udp_from(Some(src), &mut rx, &|_, _| {})).await.unwrap();
                assert!(r.is_err());
                assert!(!e.udp_routes.borrow().contains_key(&(Some(src), dst)));
            })
            .await;
    }

    #[test]
    fn dns_outbound_rejects_non_ip() {
        use crate::dns::{TYPE_A, query_for};
        let text = r#"[{"remarks": "t", "outbounds": [
            {"tag": "proxy", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.1", "port": 443,
                "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}},
            {"tag": "dns-out", "protocol": "dns", "settings": {"nonIPQuery": "reject"}}
        ]}]"#;
        let p = rv_config::parse_subscription(text).unwrap().remove(0);
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Global,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        let DnsTarget::Reply(r) = e.dns_route(&query_for("example.com", 65)) else { panic!() };
        assert_eq!(r[3] & 0x0f, 5);
        assert!(matches!(e.dns_route(&query_for("example.com", TYPE_A)), DnsTarget::Servers(_)));
        assert!(matches!(engine(RoutingMode::Global).dns_route(&query_for("example.com", 65)), DnsTarget::Servers(_)));
    }

    #[tokio::test]
    async fn blackhole_http_answers_403() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let text = r#"[{"remarks": "t", "outbounds": [
            {"tag": "block", "protocol": "blackhole", "settings": {"response": {"type": "http"}}}
        ]}]"#;
        let p = rv_config::parse_subscription(text).unwrap().remove(0);
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Subscription,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        let (mut app, local) = tokio::io::duplex(4096);
        app.write_all(b"GET / HTTP/1.1\r\nHost: ads.test\r\n\r\n").await.unwrap();
        let _ = e.tcp(local, "203.0.113.9:80".parse().unwrap()).await;
        let mut got = Vec::new();
        app.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, HTTP_403);
    }

    #[test]
    fn subscription_dns_servers() {
        let mut p = profile();
        p.dns = Some(serde_json::json!({ "servers": [
            { "address": "https+local://77.88.8.8/dns-query", "domains": ["domain:ru"] },
            { "address": "77.88.8.1", "domains": ["domain:su"] },
            "https://8.8.8.8/dns-query",
            "localhost"
        ] }));
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Global,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        let DnsHop::Stream(i) = first(e.dns_route(&crate::dns::query_for("ya.ru", crate::dns::TYPE_A))) else { panic!() };
        assert_eq!(e.dns_label(i), "https://77.88.8.8:443/dns-query");
        assert!(matches!(e.dns_out(i), Out::Direct));
        let DnsHop::Stream(i) = first(e.dns_route(&crate::dns::query_for("x.su", crate::dns::TYPE_A))) else { panic!() };
        assert!(matches!(e.dns_out(i), Out::Proxy(_)));
        assert_eq!(
            labels(&e, "google.com"),
            ["https://77.88.8.8:443/dns-query", "77.88.8.1:53", "https://8.8.8.8:443/dns-query", "1.1.1.1:53"]
        );
        assert_eq!(labels(&e, "x.su")[..2], ["77.88.8.1:53", "https://77.88.8.8:443/dns-query"]);
    }

    #[tokio::test]
    async fn direct_udp_passes_large_datagrams_without_own_buffer() {
        let echo = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = echo.local_addr().unwrap();
        let server = async move {
            let mut b = vec![0u8; 64 * 1024];
            loop {
                let (_, from) = echo.recv_from(&mut b).await.unwrap();
                echo.send_to(&vec![b[0]; 60_000], from).await.unwrap();
                echo.send_to(&b[..1], from).await.unwrap();
            }
        };
        let got = RefCell::new(Vec::new());
        let push = |_: SocketAddr, d: &[u8]| got.borrow_mut().push((d.len(), d[0]));
        let (tx, mut rx) = mpsc::channel(4);
        let session = direct_udp((addr, vec![7]), &mut rx, &push, None);
        let check = async {
            while got.borrow().len() < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            drop(tx);
        };
        tokio::pin!(server);
        let r = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! { _ = &mut server => unreachable!(), r = async { tokio::join!(session, check).0 } => r }
        })
        .await
        .unwrap();
        r.unwrap();
        assert_eq!(*got.borrow(), vec![(60_000, 7), (1, 7)]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fake_direct_tries_all_host_addresses_and_ipv6() {
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = serde_json::json!({"outbounds": [{"tag": "direct", "protocol": "freedom"}], "dns": {"hosts": {"dual.test": ["127.0.0.2", "::1"]}}});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let (mut app, local) = tokio::io::duplex(1024);
        let dest = SocketAddr::new(crate::dns::fake::ip_for("dual.test").into(), port);
        let server = async {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut b = [0; 5];
            s.read_exact(&mut b).await.unwrap();
            s.write_all(&b).await.unwrap();
        };
        let client = async {
            app.write_all(b"hello").await.unwrap();
            app.shutdown().await.unwrap();
            let mut b = [0; 5];
            app.read_exact(&mut b).await.unwrap();
            assert_eq!(b, *b"hello");
            drop(app);
        };
        let (r, _, _) = tokio::time::timeout(Duration::from_secs(2), async { tokio::join!(Box::pin(e.tcp(local, dest)), server, client) })
            .await
            .unwrap();
        r.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn silent_dns_server_does_not_hold_the_next_one() {
        let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let good = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (sp, gp) = (silent.local_addr().unwrap().port(), good.local_addr().unwrap().port());
        tokio::spawn(async move {
            let mut b = [0u8; 512];
            loop {
                let (n, from) = good.recv_from(&mut b).await.unwrap();
                let q = &b[..n];
                let qs = crate::dns::question(q).unwrap();
                let ips: Vec<IpAddr> = if qs.qtype == crate::dns::TYPE_A { vec![IpAddr::from([10, 9, 8, 7])] } else { Vec::new() };
                good.send_to(&crate::dns::answer(q, &qs, &ips, 60), from).await.unwrap();
            }
        });
        let config = serde_json::json!({"outbounds": [{"tag": "direct", "protocol": "freedom"}], "dns": {"servers": [
            {"address": "127.0.0.1", "port": sp}, {"address": "127.0.0.1", "port": gp}
        ]}});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let t0 = Instant::now();
        let ips = e.resolve_addresses("hedge.test").await.unwrap();
        assert_eq!(ips, vec![IpAddr::from([10, 9, 8, 7])]);
        assert!(t0.elapsed() < DNS_HEDGE + Duration::from_millis(400), "{:?}", t0.elapsed());
        drop(silent);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn hedged_negative_answer_does_not_beat_the_earlier_server() {
        let slow = std::sync::Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let fast = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (sp, fp) = (slow.local_addr().unwrap().port(), fast.local_addr().unwrap().port());
        let s = slow.clone();
        tokio::spawn(async move {
            let mut b = [0u8; 512];
            loop {
                let (n, from) = s.recv_from(&mut b).await.unwrap();
                let (q, s) = (b[..n].to_vec(), s.clone());
                tokio::spawn(async move {
                    tokio::time::sleep(DNS_HEDGE + Duration::from_millis(400)).await;
                    let qs = crate::dns::question(&q).unwrap();
                    s.send_to(&crate::dns::answer(&q, &qs, &[IpAddr::from([10, 9, 8, 7])], 60), from).await.unwrap();
                });
            }
        });
        tokio::spawn(async move {
            let mut b = [0u8; 512];
            loop {
                let (n, from) = fast.recv_from(&mut b).await.unwrap();
                let qs = crate::dns::question(&b[..n]).unwrap();
                fast.send_to(&crate::dns::rcode(&b[..n], &qs, 3), from).await.unwrap();
            }
        });
        let config = serde_json::json!({"outbounds": [{"tag": "direct", "protocol": "freedom"}], "dns": {"servers": [
            {"address": "127.0.0.1", "port": sp}, {"address": "127.0.0.1", "port": fp}
        ]}});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let ips = e.resolve_query("slow-primary.test", crate::dns::TYPE_A).await.unwrap();
        assert_eq!(ips, vec![IpAddr::from([10, 9, 8, 7])], "быстрый NXDOMAIN догнавшего сервера не отменяет ответ первого");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn silent_earlier_server_is_waited_a_hedge_after_a_negative_answer() {
        let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let empty = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (sp, ep) = (silent.local_addr().unwrap().port(), empty.local_addr().unwrap().port());
        tokio::spawn(async move {
            let mut b = [0u8; 512];
            loop {
                let (n, from) = empty.recv_from(&mut b).await.unwrap();
                let qs = crate::dns::question(&b[..n]).unwrap();
                empty.send_to(&crate::dns::answer(&b[..n], &qs, &[], 60), from).await.unwrap();
            }
        });
        let config = serde_json::json!({"outbounds": [{"tag": "direct", "protocol": "freedom"}], "dns": {"servers": [
            {"address": "127.0.0.1", "port": sp}, {"address": "127.0.0.1", "port": ep}
        ]}});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let t0 = Instant::now();
        assert!(e.resolve_query("v4-only.test", crate::dns::TYPE_AAAA).await.is_err());
        let t = t0.elapsed();
        assert!(t >= DNS_HEDGE * 2 && t < DNS_HEDGE * 2 + Duration::from_millis(400), "NODATA ждал молчащий сервер до срока: {t:?}");
        drop(silent);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn server_asked_after_a_negative_answer_keeps_its_own_timeout() {
        let neg = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let slow = std::sync::Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let (np, sp) = (neg.local_addr().unwrap().port(), slow.local_addr().unwrap().port());
        tokio::spawn(async move {
            let mut b = [0u8; 512];
            loop {
                let (n, from) = neg.recv_from(&mut b).await.unwrap();
                let qs = crate::dns::question(&b[..n]).unwrap();
                neg.send_to(&crate::dns::rcode(&b[..n], &qs, 3), from).await.unwrap();
            }
        });
        let s = slow.clone();
        tokio::spawn(async move {
            let mut b = [0u8; 512];
            loop {
                let (n, from) = s.recv_from(&mut b).await.unwrap();
                let (q, s) = (b[..n].to_vec(), s.clone());
                tokio::spawn(async move {
                    tokio::time::sleep(DNS_HEDGE + Duration::from_millis(400)).await;
                    let qs = crate::dns::question(&q).unwrap();
                    let _ = s.send_to(&crate::dns::answer(&q, &qs, &[IpAddr::from([10, 9, 8, 7])], 60), from).await;
                });
            }
        });
        let config = serde_json::json!({"outbounds": [{"tag": "direct", "protocol": "freedom"}], "dns": {"servers": [
            {"address": "127.0.0.1", "port": np}, {"address": "127.0.0.1", "port": sp}
        ]}});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let ips = e.resolve_query("later.test", crate::dns::TYPE_A).await.unwrap();
        assert_eq!(ips, vec![IpAddr::from([10, 9, 8, 7])]);
    }

    #[test]
    fn server_asked_after_a_negative_answer_at_the_same_clock_reading_is_not_given_up() {
        let t = tokio::time::Instant::now();
        assert_eq!(dns_give_up(Some((t, 1)), true, [1].into_iter(), t + DNS_HEDGE), None);
        assert_eq!(dns_give_up(Some((t, 2)), true, [0].into_iter(), t), Some(t + DNS_HEDGE));
        assert_eq!(dns_give_up(Some((t, 1)), false, [0].into_iter(), t), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn huge_answers_keep_a_bounded_number_of_addresses() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut len = [0u8; 2];
                    while s.read_exact(&mut len).await.is_ok() {
                        let mut q = vec![0u8; u16::from_be_bytes(len) as usize];
                        if s.read_exact(&mut q).await.is_err() {
                            return;
                        }
                        let qs = crate::dns::question(&q).unwrap();
                        let ips: Vec<IpAddr> = if qs.qtype == crate::dns::TYPE_A {
                            (0..4000u32).map(|i| IpAddr::from((0x0a00_0000 + i).to_be_bytes())).collect()
                        } else {
                            Vec::new()
                        };
                        let r = crate::dns::answer(&q, &qs, &ips, 3600);
                        let mut out = (r.len() as u16).to_be_bytes().to_vec();
                        out.extend_from_slice(&r);
                        if s.write_all(&out).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        let config = serde_json::json!({"outbounds": [{"tag": "direct", "protocol": "freedom"}],
            "dns": {"servers": [format!("tcp://127.0.0.1:{port}")]}});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let ips = e.resolve_query("many.test", crate::dns::TYPE_A).await.unwrap();
        assert!(!ips.is_empty() && ips.len() <= DNS_ADDRS_MAX, "{} addresses", ips.len());
        assert!(e.real.borrow().values().all(|(cached, _)| cached.len() <= DNS_ADDRS_MAX));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn no_early_hedge_from_a_direct_server_to_a_proxied_one() {
        tokio::task::LocalSet::new().run_until(async {
            let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config = serde_json::json!({"outbounds": [
                {"tag": "proxy", "protocol": "vless", "settings": {"vnext": [{"address": "127.0.0.1", "port": proxy.local_addr().unwrap().port(),
                    "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}},
                {"tag": "direct", "protocol": "freedom"}
            ], "dns": {"servers": [{"address": "127.0.0.1", "port": silent.local_addr().unwrap().port(), "timeoutMs": 1500}, "tcp://1.1.1.1"]}});
            let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
            let e = Engine::new(&p, &GeoData::default(), None, &Options { routing: RoutingMode::Global, ..Default::default() }).unwrap();
            let t0 = Instant::now();
            tokio::select! {
                _ = e.resolve_query("hedge.test", crate::dns::TYPE_A) => panic!("сервер за прокси не отвечает"),
                r = proxy.accept() => { r.unwrap(); }
            }
            assert!(t0.elapsed() >= Duration::from_millis(1300), "сервер за прокси спрошен раньше срока прямого: {:?}", t0.elapsed());
            drop(silent);
        }).await;
    }

    #[test]
    fn dns_fallback_order() {
        let mut p = profile();
        p.dns = Some(serde_json::json!({ "disableFallbackIfMatch": true, "servers": [
            { "address": "77.88.8.8", "domains": ["domain:ru"], "skipFallback": true, "timeoutMs": 1500 },
            { "address": "8.8.8.8", "finalQuery": true },
            "1.1.1.1"
        ] }));
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Global,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        assert_eq!(labels(&e, "ya.ru"), ["77.88.8.8:53"]);
        assert_eq!(labels(&e, "google.com"), ["8.8.8.8:53"]);
        let DnsTarget::Servers(h) = e.dns_route(&crate::dns::query_for("ya.ru", crate::dns::TYPE_A)) else { panic!() };
        assert_eq!(h[0].1, Duration::from_millis(1500));
    }

    #[test]
    fn local_answers_and_system_dns() {
        use crate::dns::{TYPE_A, TYPE_AAAA, query_for};
        let mut p = profile();
        p.dns = Some(serde_json::json!({
            "hosts": { "router.lan": "192.168.1.1", "domain:corp.test": ["10.0.0.5", "fd00::5"], "bad": "other.name", "domain:ads.test": "#3" },
            "servers": [ { "address": "localhost", "domains": ["domain:lan"] }, "tls://1.1.1.1", "quic+local://dns.adguard.com" ]
        }));
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Global,
            system_dns: std::sync::Arc::new(std::sync::Mutex::new(vec!["192.168.1.1:53".parse().unwrap()])),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        assert!(matches!(e.dns_route(&query_for("bad", TYPE_A)), DnsTarget::Lookup(name) if name == "other.name"));
        assert!(!e.warnings.iter().any(|w| w.contains("localhost")), "{:?}", e.warnings);
        let DnsTarget::Reply(r) = e.dns_route(&query_for("router.lan", TYPE_A)) else { panic!() };
        assert_eq!(&r[r.len() - 4..], &[192, 168, 1, 1]);
        let DnsTarget::Reply(r) = e.dns_route(&query_for("t.ads.test", TYPE_A)) else { panic!() };
        assert_eq!(r[3] & 0x0f, 3);
        let DnsTarget::Reply(r) = e.dns_route(&query_for("x.corp.test", TYPE_AAAA)) else { panic!() };
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 1);
        assert!(matches!(e.dns_route(&query_for("google.com", TYPE_AAAA)), DnsTarget::Servers(_)));
        assert_eq!(first(e.dns_route(&query_for("nas.lan", TYPE_A))), DnsHop::Direct("192.168.1.1:53".parse().unwrap()));
        assert_eq!(labels(&e, "google.com"), ["192.168.1.1:53", "tls://1.1.1.1:853", "quic://dns.adguard.com:853"]);
        assert!(matches!(e.dns_out(1), Out::Proxy(_)));
        assert!(matches!(e.dns_out(2), Out::Direct));
    }

    #[test]
    fn dns_parity_families_fallback_and_block() {
        use crate::dns::{TYPE_A, TYPE_AAAA, query_for};
        let mut p = profile();
        p.dns = Some(serde_json::json!({ "servers": [{ "address": "8.8.8.8", "domains": ["domain:test"] }] }));
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        assert_eq!(e.dns.len(), 1, "Xray не добавляет сервер к заданному списку");
        assert!(matches!(e.dns_route(&query_for("a.test", TYPE_AAAA)), DnsTarget::Servers(_)));
        p.dns = Some(serde_json::json!({ "queryStrategy": "UseIPv4", "servers": ["8.8.8.8"] }));
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let DnsTarget::Reply(r) = e.dns_route(&query_for("a.test", TYPE_AAAA)) else {
            panic!("IPv6 должен быть выключен")
        };
        assert!(crate::dns::addresses(&r).is_empty());
        p.dns = Some(serde_json::json!({ "servers": [{ "address": "8.8.8.8", "queryStrategy": "UseIPv6" }] }));
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        assert!(matches!(e.dns_route(&query_for("a.test", TYPE_A)), DnsTarget::Reply(_)));
        p.routing = Some(serde_json::json!({ "rules": [{ "ip": ["8.8.8.8"], "outboundTag": "block" }] }));
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        assert!(matches!(e.dns_out(0), Out::Block), "запрет DNS не должен обходиться через proxy");
    }

    #[tokio::test]
    async fn dns_hosts_alias_and_direct_resolution() {
        use crate::dns::{TYPE_A, query_for};
        let mut p = profile();
        p.dns = Some(
            serde_json::json!({ "hosts": { "alias.test": "actual.test", "actual.test": ["192.0.2.1", "2001:db8::1"], "domain:actual.test": "192.0.2.2" } }),
        );
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let DnsTarget::Reply(r) = e.dns_route(&query_for("alias.test", TYPE_A)) else { panic!("hosts alias не разрешён") };
        assert_eq!(crate::dns::addresses(&r), vec!["192.0.2.1".parse::<IpAddr>().unwrap(), "192.0.2.2".parse().unwrap()]);
        assert_eq!(e.resolve_real("alias.test").await.unwrap(), "192.0.2.1".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    async fn dns_zero_ttl_is_not_cached_for_routing() {
        let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = sock.local_addr().unwrap();
        let count = Rc::new(std::cell::Cell::new(0));
        let received = count.clone();
        let task = async move {
            let mut b = [0; 4096];
            loop {
                let (n, from) = sock.recv_from(&mut b).await.unwrap();
                received.set(received.get() + 1);
                let q = crate::dns::question(&b[..n]).unwrap();
                let r = crate::dns::answer(&b[..n], &q, &["192.0.2.1".parse().unwrap()], 0);
                sock.send_to(&r, from).await.unwrap();
            }
        };
        let opts = Options { system_dns: std::sync::Arc::new(std::sync::Mutex::new(vec![addr])), ..Default::default() };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        let check = async {
            for _ in 0..2 {
                e.resolve_query("ttl.test", crate::dns::TYPE_A).await.unwrap();
            }
            assert_eq!(count.get(), 2);
        };
        tokio::pin!(task);
        tokio::pin!(check);
        tokio::select! { _ = &mut task => unreachable!(), _ = &mut check => {} }
    }

    #[tokio::test]
    async fn own_dns_cache_is_bounded_and_ttl_capped() {
        let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = sock.local_addr().unwrap();
        let task = async move {
            let mut b = [0; 4096];
            loop {
                let (n, from) = sock.recv_from(&mut b).await.unwrap();
                let q = crate::dns::question(&b[..n]).unwrap();
                let r = crate::dns::answer(&b[..n], &q, &["192.0.2.1".parse().unwrap()], 86400);
                sock.send_to(&r, from).await.unwrap();
            }
        };
        let opts = Options { system_dns: std::sync::Arc::new(std::sync::Mutex::new(vec![addr])), ..Default::default() };
        let e = Rc::new(Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap());
        let check = async {
            let (_, ttl) = e.resolve_query_ttl("long.test", crate::dns::TYPE_A).await.unwrap();
            assert_eq!(ttl, REAL_TTL_MAX, "срок из ответа урезан");
            for i in 0..3 * REAL_MAX {
                e.cache_real(format!("1:n{i}.test"), &["192.0.2.1".parse().unwrap()], 86400);
                assert!(e.real.borrow().len() <= REAL_MAX);
            }
            e.reset_pools();
            assert!(e.real.borrow().is_empty(), "смена сети стирает кэш");
        };
        tokio::pin!(task);
        tokio::pin!(check);
        tokio::select! { _ = &mut task => unreachable!(), _ = &mut check => {} }
    }

    #[tokio::test]
    async fn domain_strategy_respects_rule_order_and_all_resolved_addresses() {
        let mut p = profile();
        p.dns = Some(serde_json::json!({"hosts": {"a.test": ["192.0.2.1", "192.0.2.2"]}}));
        let ctx = RouteCtx {
            domain: Some("a.test"),
            ip: None,
            port: 443,
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        for strategy in ["AsIs", "IPOnDemand", "IPIfNonMatch"] {
            p.routing = Some(serde_json::json!({"domainStrategy": strategy, "rules": [
                {"ip": ["192.0.2.2"], "outboundTag": "block"}, {"domain": ["a.test"], "outboundTag": "direct"}
            ]}));
            let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
            let (out, _) = e.route_async(&ctx).await;
            assert_eq!(matches!(out, Out::Block), strategy == "IPOnDemand");
            if strategy != "IPOnDemand" {
                assert!(matches!(out, Out::Direct));
            }
        }
        p.routing = Some(serde_json::json!({"domainStrategy": "IPIfNonMatch", "rules": [{"ip": ["192.0.2.2"], "outboundTag": "block"}]}));
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        assert!(matches!(e.route_async(&ctx).await.0, Out::Block));
    }

    #[test]
    fn random_balancer_skips_dead_candidates() {
        let mut p = profile();
        let mut a = p.outbounds[0].clone();
        a.tag = "a".into();
        let mut b = p.outbounds[0].clone();
        b.tag = "b".into();
        p.outbounds.push(a);
        p.outbounds.push(b);
        p.routing = Some(serde_json::json!({"balancers": [{"tag": "auto", "selector": ["a", "b"], "strategy": {"type": "random"}}],
            "rules": [{"network": "tcp,udp", "balancerTag": "auto"}]}));
        let opts = Options { routing: RoutingMode::Subscription, ..Default::default() };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        assert!(e.has_balancers(), "random candidates must be probed too");
        e.health.borrow_mut().entry("a".into()).or_default().observe(None);
        e.health.borrow_mut().entry("b".into()).or_default().observe(Some(Duration::from_millis(80)));
        let ctx = RouteCtx {
            domain: Some("example.com"),
            ip: None,
            port: 443,
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        for _ in 0..30 {
            assert_eq!(e.route_tagged(&ctx).1.last.as_deref(), Some("b"));
        }
        e.health.borrow_mut().entry("b".into()).or_default().observe(None);
        assert!(matches!(e.route(&ctx), Out::Proxy(_)));
    }

    #[tokio::test]
    async fn measured_balancer_does_not_fall_back_while_a_candidate_may_be_alive() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut p = profile();
                p.outbounds.swap(0, 1);
                let mut deny = p.outbounds[0].clone();
                deny.tag = "deny".into();
                deny.kind = OutboundKind::Blackhole { http: false };
                let mut a = p.outbounds[1].clone();
                a.tag = "a".into();
                let mut b = p.outbounds[1].clone();
                b.tag = "b".into();
                p.outbounds.extend([deny, a, b]);
                p.routing = Some(serde_json::json!({"balancers": [
                    {"tag": "auto", "selector": ["a", "b"], "strategy": {"type": "leastPing"}},
                    {"tag": "rand", "selector": ["a", "b"], "fallbackTag": "deny"}],
                "rules": [{"port": "8443", "balancerTag": "rand"}, {"network": "tcp,udp", "balancerTag": "auto"}]}));
                let e = Rc::new(
                    Engine::new(&p, &GeoData::default(), None, &Options { routing: RoutingMode::Subscription, ..Default::default() })
                        .unwrap(),
                );
                assert!(matches!(e.default, Out::Direct));
                let ctx = |port| RouteCtx {
                    domain: Some("example.com"),
                    ip: None,
                    port,
                    network: Network::Tcp,
                    protocol: None,
                    inbound_tag: None,
                    process: None,
                };
                let last = |port| e.route_tagged(&ctx(port)).1.last;
                assert!(matches!(last(443).as_deref(), Some("a" | "b")), "замеров ещё нет: кандидат, а не default");
                e.health.borrow_mut().entry("a".into()).or_default().transport(Some(Duration::from_millis(40)));
                assert_eq!(last(443).as_deref(), Some("a"));
                e.reset_pools();
                assert!(e.health.borrow().is_empty());
                assert!(matches!(last(443).as_deref(), Some("a" | "b")), "после смены сети замеры стёрты, но не default");
                e.health.borrow_mut().entry("a".into()).or_default().transport(None);
                for _ in 0..20 {
                    assert_eq!(last(443).as_deref(), Some("b"), "мёртвый пропускается, неизвестный годен");
                }
                e.health.borrow_mut().entry("b".into()).or_default().transport(None);
                assert!(
                    matches!(last(443).as_deref(), Some("a" | "b")),
                    "мертвы все, fallbackTag нет: всё равно кандидат, а не default (direct)"
                );
                assert_eq!(last(8443).as_deref(), Some("deny"), "мертвы все — fallbackTag, а не первый кандидат");
            })
            .await;
    }

    #[tokio::test]
    async fn failover_records_health_of_tried_candidates() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let open = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
                let mut p = profile();
                p.outbounds[0].stream = Default::default();
                for (tag, port) in [("a", closed), ("b", closed), ("c", open.local_addr().unwrap().port())] {
                    let mut o = p.outbounds[0].clone();
                    o.tag = tag.into();
                    if let OutboundKind::Vless(v) = &mut o.kind {
                        v.address = "127.0.0.1".into();
                        v.port = port;
                    }
                    p.outbounds.push(o);
                }
                p.routing = Some(
                    serde_json::json!({"balancers": [{"tag": "auto", "selector": ["a", "b", "c"], "strategy": {"type": "leastPing"}}],
                "rules": [{"network": "tcp,udp", "balancerTag": "auto"}]}),
                );
                let e = Engine::new(&p, &GeoData::default(), None, &Options { routing: RoutingMode::Subscription, ..Default::default() })
                    .unwrap();
                let Some(Out::Proxy(a)) = e.outs.get("a").cloned() else { panic!() };
                let r = e
                    .failover("auto", &a, Address::from_ip(IpAddr::from([127, 0, 0, 1])), 80, Error::Config("a failed".into()), None)
                    .await;
                assert!(r.is_ok());
                let h = e.health.borrow();
                assert!(!h["b"].alive && h["b"].fresh(), "неудачная попытка запасного видна балансировщику");
                assert!(h["c"].alive, "удачная тоже");
            })
            .await;
    }

    #[test]
    fn round_robin_includes_freedom_and_blackhole_without_probes() {
        let mut p = profile();
        let mut block = p.outbounds[1].clone();
        block.tag = "block".into();
        block.kind = OutboundKind::Blackhole { http: false };
        p.outbounds.push(block);
        p.routing =
            Some(serde_json::json!({"balancers": [{"tag": "b", "selector": ["direct", "block"], "strategy": {"type": "roundRobin"}}],
            "rules": [{"network": "tcp", "balancerTag": "b"}]}));
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let ctx = RouteCtx { domain: None, ip: None, port: 443, network: Network::Tcp, protocol: None, inbound_tag: None, process: None };
        assert!(!e.has_balancers());
        assert!(matches!(e.route(&ctx), Out::Direct));
        assert!(matches!(e.route(&ctx), Out::Block));
        assert!(matches!(e.route(&ctx), Out::Direct));
    }

    #[test]
    fn probe_every_follows_the_config_within_bounds() {
        let s = |v: Value| probe_every(Some(&v)).as_secs();
        assert_eq!(probe_every(None).as_secs(), 300);
        assert_eq!(s(serde_json::json!({"probeInterval": "2m"})), 120);
        assert_eq!(s(serde_json::json!({"pingConfig": {"interval": "1s"}})), 30);
        assert_eq!(s(serde_json::json!({"pingConfig": {"interval": "1h"}})), 900);
    }

    #[test]
    fn probe_period_jitter_stays_within_15_percent() {
        let every = Duration::from_secs(100);
        assert_eq!(jittered(every, 0), Duration::from_secs(85));
        assert_eq!(jittered(every, 150), every);
        assert_eq!(jittered(every, 300), Duration::from_secs(115));
        assert_eq!(jittered(every, 9999), Duration::from_secs(115));
        for r in 0..=300 {
            let d = jittered(every, r);
            assert!(d >= Duration::from_secs(85) && d <= Duration::from_secs(115));
        }
        assert!(jittered(Duration::ZERO, 0) >= Duration::from_millis(1));
        assert!(jittered(Duration::from_nanos(1), 0) >= Duration::from_millis(1));
    }

    #[tokio::test]
    async fn observatory_uses_configured_url_get_and_any_http_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut b = Vec::new();
            let mut tmp = [0; 1024];
            while !b.windows(4).any(|s| s == b"\r\n\r\n") {
                let n = s.read(&mut tmp).await.unwrap();
                assert!(n > 0 && b.len() < 4096);
                b.extend_from_slice(&tmp[..n]);
            }
            assert!(b.windows(b"GET /selected HTTP/1.1\r\n".len()).any(|s| s == b"GET /selected HTTP/1.1\r\n"));
            s.write_all(b"\0\0HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        });
        let mut p = profile();
        p.outbounds[0].stream = Default::default();
        if let OutboundKind::Vless(v) = &mut p.outbounds[0].kind {
            v.address = "127.0.0.1".into();
            v.port = port;
        }
        p.routing =
            Some(serde_json::json!({"rules": [], "balancers": [{"tag": "b", "selector": ["proxy"], "strategy": {"type": "leastPing"}}]}));
        p.observatory = Some(serde_json::json!({"probeURL": "http://test.invalid/selected"}));
        let e = Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap();
        let e = Rc::new(e);
        e.probe_balancers().await;
        server.await.unwrap();
        assert!(e.health.borrow()["proxy"].alive);
    }

    #[tokio::test]
    async fn balancer_recheck_timer_runs_only_with_background_work() {
        let mut p = profile();
        p.outbounds[0].stream = Default::default();
        if let OutboundKind::Vless(v) = &mut p.outbounds[0].kind {
            v.address = "127.0.0.1".into();
            v.port = 9;
        }
        p.routing =
            Some(serde_json::json!({"rules": [], "balancers": [{"tag": "b", "selector": ["proxy"], "strategy": {"type": "leastPing"}}]}));
        p.observatory = Some(serde_json::json!({"probeURL": "http://127.0.0.1:9/x", "probeInterval": "1m"}));
        let off = Rc::new(Engine::new(&p, &GeoData::default(), None, &Options::default()).unwrap());
        let done = tokio::time::timeout(Duration::from_secs(20), off.watch_balancers()).await;
        assert!(done.is_ok(), "without the setting the watcher ends after the start probe");
        let on = Rc::new(Engine::new(&p, &GeoData::default(), None, &Options { background: true, ..Default::default() }).unwrap());
        assert!(
            tokio::time::timeout(Duration::from_secs(3), on.clone().watch_balancers()).await.is_err(),
            "with it the watcher keeps waiting"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn balancer_recheck_is_armed_only_by_a_balancer_choice() {
        let used = tokio::sync::Notify::new();
        let every = Duration::from_secs(60);
        assert!(
            tokio::time::timeout(Duration::from_secs(86_400), balancer_due(&used, every, || true)).await.is_err(),
            "no choice, no recheck"
        );
        used.notify_one();
        let t = tokio::time::Instant::now();
        assert!(balancer_due(&used, every, || true).await);
        let took = t.elapsed();
        assert!(took >= every.mul_f64(0.85) && took <= every.mul_f64(1.15), "{took:?}");
        used.notify_one();
        assert!(!balancer_due(&used, every, || false).await, "a dropped engine ends the wait without a timer");
    }

    #[test]
    fn own_dns_replaces_subscription_dns() {
        let mut opts = Options { routing: RoutingMode::Global, ..Default::default() };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        assert_eq!(e.dns.len(), 1);
        assert!(matches!(e.dns[0].up, Upstream::System));
        opts.dns = vec!["https://dns.example/dns-query".into(), "tls+local://9.9.9.9".into(), "bogus://x".into()];
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        assert_eq!(e.dns.len(), 2);
        assert!(matches!(&e.dns[0].up, Upstream::Https { host, .. } if host == "dns.example"));
        assert!(e.dns[1].local);
        assert!(e.warnings.iter().any(|w| w.contains("bogus")));
    }

    #[test]
    fn no_network_is_not_a_server_failure() {
        let unreachable = io::Error::from(io::ErrorKind::NetworkUnreachable);
        assert!(offline(&Error::Io(io::Error::other(Error::Io(unreachable)))));
        assert!(!offline(&Error::Io(io::Error::from(io::ErrorKind::TimedOut))));
    }

    #[test]
    fn foreign_fake_ip_without_fake_dns() {
        let ip: IpAddr = "198.19.250.7".parse().unwrap();
        let mut opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Global,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        assert!(!e.is_fake(ip));
        opts.fake_dns = true;
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        assert!(e.is_fake(ip));
    }

    #[test]
    fn lan_goes_direct_in_every_mode() {
        let at = |ip: &str| RouteCtx {
            domain: None,
            ip: Some(ip.parse().unwrap()),
            port: 445,
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        for mode in [RoutingMode::Global, RoutingMode::Subscription, RoutingMode::Smart] {
            let e = engine(mode);
            assert!(matches!(e.route(&at("192.168.1.20")), Out::Direct), "{mode:?}: LAN via proxy");
            assert!(matches!(e.route(&at("10.0.0.5")), Out::Direct), "{mode:?}: 10/8 via proxy");
        }
        let g = engine(RoutingMode::Global);
        assert!(matches!(g.route(&at("100.101.102.103")), Out::Proxy(_)));
        assert!(matches!(g.route(&at("198.18.0.9")), Out::Proxy(_)));
        assert!(matches!(g.route(&at("1.1.1.1")), Out::Proxy(_)));
        let opts = Options { user_rules: "IP-CIDR,192.168.5.0/24,PROXY".into(), routing: RoutingMode::Global, ..Default::default() };
        let own = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        assert!(matches!(own.route(&at("192.168.5.7")), Out::Proxy(_)));
    }

    #[test]
    fn fake_address_is_not_private() {
        use crate::dns::{TYPE_A, query_for};
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Smart,
            system_dns: Default::default(),
            fake_dns: true,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        let DnsTarget::Reply(r) = e.dns_route(&query_for("firestore.googleapis.com", TYPE_A)) else { panic!("no fake address") };
        let ip = crate::dns::addresses(&r)[0];
        let ctx = RouteCtx {
            domain: Some("firestore.googleapis.com"),
            ip: (!crate::dns::fake::is_fake(ip)).then_some(ip),
            port: 443,
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: None,
        };
        assert!(matches!(e.route(&ctx), Out::Proxy(_)), "fake address routed as private");
    }

    #[test]
    fn no_aaaa_for_direct_names_without_a_direct_ipv6() {
        use crate::dns::{TYPE_A, TYPE_AAAA, query_for};
        thread_local! { static V6: std::cell::Cell<bool> = const { std::cell::Cell::new(true) }; }
        rv_net::set_direct_v6(|| V6.with(|v| v.get()));
        let opts = Options {
            user_rules: "DOMAIN-SUFFIX,local.test,DIRECT".into(),
            routing: RoutingMode::Custom,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        assert!(matches!(e.dns_route(&query_for("x.local.test", TYPE_AAAA)), DnsTarget::Servers(_)));
        V6.with(|v| v.set(false));
        let DnsTarget::Reply(r) = e.dns_route(&query_for("x.local.test", TYPE_AAAA)) else { panic!() };
        assert!(crate::dns::addresses(&r).is_empty());
        assert!(matches!(e.dns_route(&query_for("x.local.test", TYPE_A)), DnsTarget::Servers(_)));
        assert!(matches!(e.dns_route(&query_for("proxied.test", TYPE_AAAA)), DnsTarget::Servers(_)));
        V6.with(|v| v.set(true));
    }

    #[test]
    fn fake_dns_for_proxied_names() {
        use crate::dns::{TYPE_A, TYPE_AAAA, fake, query_for};
        let opts = Options {
            user_rules: "DOMAIN-SUFFIX,local.test,DIRECT".into(),
            routing: RoutingMode::Custom,
            system_dns: Default::default(),
            fake_dns: true,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        let DnsTarget::Reply(r) = e.dns_route(&query_for("proxied.test", TYPE_A)) else { panic!() };
        let ip = crate::dns::addresses(&r)[0];
        assert!(fake::is_fake(ip));
        assert_eq!(fake::name_for(ip).as_deref(), Some("proxied.test"));
        let DnsTarget::Reply(r) = e.dns_route(&query_for("proxied.test", TYPE_AAAA)) else { panic!() };
        assert!(crate::dns::addresses(&r).is_empty());
        assert!(matches!(e.dns_route(&query_for("x.local.test", TYPE_A)), DnsTarget::Servers(_)));
        assert!(
            matches!(e.dns_route(&query_for("x\n7 victim.test", TYPE_A)), DnsTarget::Servers(_)),
            "строка с переводом строки не попадает в журнал"
        );
        let mut p = profile();
        p.dns = Some(serde_json::json!({ "servers": [ { "address": "fakedns", "domains": ["domain:games.test"] }, "1.1.1.1" ] }));
        let opts = Options {
            user_rules: String::new(),
            routing: RoutingMode::Global,
            system_dns: Default::default(),
            fake_dns: false,
            ipv4_only: false,
            owner: None,
            log_apps: false,
            sub_routing: None,
            on_failing: None,
            dns: Vec::new(),
            background: false,
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        assert!(matches!(e.dns_route(&query_for("a.games.test", TYPE_A)), DnsTarget::Reply(_)));
        assert!(matches!(e.dns_route(&query_for("google.com", TYPE_A)), DnsTarget::Reply(_)));
        let mut p = profile();
        p.dns = Some(
            serde_json::json!({ "servers": [ { "address": "fakedns", "domains": ["domain:games.test"], "skipFallback": true }, "1.1.1.1" ] }),
        );
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        assert!(matches!(e.dns_route(&query_for("a.games.test", TYPE_A)), DnsTarget::Reply(_)));
        assert!(matches!(first(e.dns_route(&query_for("google.com", TYPE_A))), DnsHop::Stream(_)));
    }

    fn local_vless(ports: &[u16], routing: Value) -> Profile {
        let mut outbounds: Vec<Value> = ports.iter().enumerate().map(|(i, port)| serde_json::json!({
            "tag": if i == 0 { "proxy".to_string() } else { format!("proxy-{}", i + 1) }, "protocol": "vless",
            "settings": {"vnext": [{"address": "127.0.0.1", "port": port, "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}
        })).collect();
        outbounds.push(serde_json::json!({"tag": "direct", "protocol": "freedom"}));
        let config = serde_json::json!({"outbounds": outbounds, "routing": routing});
        rv_config::parse_subscription(&config.to_string()).unwrap().remove(0)
    }

    async fn fake_vless(answer: std::rc::Rc<std::cell::Cell<bool>>) -> (u16, std::rc::Rc<std::cell::Cell<usize>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let accepted = std::rc::Rc::new(std::cell::Cell::new(0));
        let n = accepted.clone();
        tokio::task::spawn_local(async move {
            while let Ok((mut s, _)) = l.accept().await {
                n.set(n.get() + 1);
                let answer = answer.get();
                tokio::task::spawn_local(async move {
                    let mut b = [0u8; 4096];
                    let _ = s.read(&mut b).await;
                    if answer {
                        let _ = s.write_all(&[0, 0, b'o', b'k']).await;
                    }
                });
            }
        });
        (port, accepted)
    }

    fn closed_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn server_closing_every_connection_without_reply_is_failing() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let answer = std::rc::Rc::new(std::cell::Cell::new(false));
                let (port, accepted) = fake_vless(answer.clone()).await;
                let e = Engine::new(
                    &local_vless(&[port], Value::Null),
                    &GeoData::default(),
                    None,
                    &Options { routing: RoutingMode::Global, ..Default::default() },
                )
                .unwrap();
                let conn = |i: u16, app_closes: bool| {
                    let e = &e;
                    async move {
                        let (mut app, local) = tokio::io::duplex(4096);
                        app.write_all(&[0xef; 64]).await.unwrap();
                        if app_closes {
                            app.shutdown().await.unwrap();
                        }
                        let reader = async {
                            let mut got = Vec::new();
                            let _ = app.read_to_end(&mut got).await;
                            got
                        };
                        let (r, got) = tokio::join!(e.tcp(local, SocketAddr::from(([93, 184, 216, 34], 1000 + i))), reader);
                        (r, got)
                    }
                };
                for i in 0..FAILING_STREAK as u16 {
                    let (r, _) = conn(i, false).await;
                    assert!(r.unwrap_err().to_string().contains("no response header"));
                }
                assert_eq!(e.fail_streak.get(), FAILING_STREAK, "каждое закрытие без ответа — неудача сервера");
                assert_eq!(accepted.get(), FAILING_STREAK as usize, "дозвоны шли");
                for i in 0..3 {
                    let _ = conn(100 + i, true).await;
                }
                assert_eq!(e.fail_streak.get(), FAILING_STREAK, "приложение закрыло первым: не в счёт");
                answer.set(true);
                let (r, got) = conn(200, false).await;
                r.unwrap();
                assert_eq!(got, b"ok");
                assert_eq!(e.fail_streak.get(), 0, "данные от сервера — успех");
                assert!(e.proxy_alive.get().is_some());
                answer.set(false);
                let _ = conn(300, false).await;
                assert_eq!(e.fail_streak.get(), 0);
            })
            .await;
    }

    #[test]
    fn dial_breaker_opens_after_a_series_and_probes_once_per_window() {
        let e = Rc::new(engine(RoutingMode::Global));
        let p = e.default_proxy().unwrap();
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        for i in 0..DIAL_FAILS as u64 * 2 {
            e.dial_done(&p, false, at(i * 10));
        }
        assert!(e.dial_allowed(&p, at(200)), "пачка неудач за доли секунды: не повод");
        e.dial_done(&p, false, at(2_000));
        assert!(!e.dial_allowed(&p, at(2_100)), "серия дольше DIAL_FAIL_SPAN: окно");
        assert!(!e.dial_allowed(&p, at(3_900)));
        e.dial_done(&p, false, at(3_000));
        assert!(e.dial_allowed(&p, at(4_000)), "неудача начатого до окна его не продлевает: проба по концу");
        assert!(!e.dial_allowed(&p, at(4_001)), "пока проба звонит, остальные отбиваются");
        e.dial_done(&p, false, at(4_500));
        assert!(!e.dial_allowed(&p, at(8_400)), "неудачная проба: окно вдвое");
        assert!(e.dial_allowed(&p, at(8_500)));
        let mut t = 8_500;
        for _ in 0..10 {
            e.dial_done(&p, false, at(t));
            let until = e.dial_fails.borrow()[&Rc::as_ptr(&p)].until.unwrap();
            assert!(until.duration_since(at(t)) <= DIAL_BACKOFF_MAX);
            t = until.duration_since(t0).as_millis() as u64;
            assert!(e.dial_allowed(&p, at(t)));
        }
        assert_eq!(e.dial_fails.borrow()[&Rc::as_ptr(&p)].backoff, DIAL_BACKOFF_MAX);
        assert!(!e.dial_allowed(&p, at(t + 1)));
        assert!(e.dial_allowed(&p, at(t) + SETUP_TIMEOUT), "отменённая проба не держит окно вечно");
        e.dial_done(&p, true, at(t + 100));
        assert!(e.dial_allowed(&p, at(t + 101)) && e.dial_fails.borrow().is_empty(), "удачный дозвон — сразу всем");
        for i in 0..DIAL_FAILS as u64 {
            e.dial_done(&p, false, at(t + i * 1_000));
        }
        assert!(!e.dial_allowed(&p, at(t + 5_000)));
        e.reset_pools();
        assert!(e.dial_allowed(&p, at(t + 5_000)), "смена сети: сразу пробовать");
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn dial_in_a_failing_window_is_held_without_dialing() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        let e = Engine::new(
            &local_vless(&[l.local_addr().unwrap().port()], Value::Null),
            &GeoData::default(),
            None,
            &Options { routing: RoutingMode::Global, ..Default::default() },
        )
        .unwrap();
        let p = e.default_proxy().unwrap();
        let t0 = Instant::now();
        for i in 0..DIAL_FAILS {
            e.dial_done(&p, false, t0 + DIAL_FAIL_SPAN * i / (DIAL_FAILS - 1));
        }
        let (mut app, local) = tokio::io::duplex(1024);
        app.write_all(&[0xef; 64]).await.unwrap();
        let fails = rv_stats::get(rv_stats::Slot::ProxyFail);
        let t = tokio::time::Instant::now();
        let err = e.tcp(local, "93.184.216.34:443".parse().unwrap()).await.expect_err("отбит");
        assert!(err.to_string().contains(DIAL_HELD), "{err}");
        assert!(t.elapsed() >= REFUSED_HOLD, "сокет приложения держится");
        assert!(matches!(l.accept(), Err(e) if e.kind() == io::ErrorKind::WouldBlock), "дозвона не было");
        assert!(rv_stats::get(rv_stats::Slot::ProxyFail) > fails, "для проверки сервера — несостоявшаяся попытка");
        assert_eq!(e.fail_streak.get(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn balancer_skips_a_member_in_a_failing_window() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (port, accepted) = fake_vless(std::rc::Rc::new(std::cell::Cell::new(true))).await;
                let routing = serde_json::json!({
                "balancers": [{"tag": "b", "selector": ["proxy"], "strategy": {"type": "roundRobin"}}],
                "rules": [{"type": "field", "network": "tcp,udp", "balancerTag": "b"}]});
                let opts = Options { routing: RoutingMode::Subscription, ..Default::default() };
                let e = Engine::new(&local_vless(&[closed_port(), port], routing), &GeoData::default(), None, &opts).unwrap();
                let Some(Out::Proxy(dead)) = e.outs.get("proxy").cloned() else { panic!() };
                let t0 = Instant::now();
                for i in 0..DIAL_FAILS {
                    e.dial_done(&dead, false, t0 + DIAL_FAIL_SPAN * i / (DIAL_FAILS - 1));
                }
                for i in 0..4u16 {
                    let (mut app, local) = tokio::io::duplex(1024);
                    app.write_all(&[0xef; 64]).await.unwrap();
                    let reader = async {
                        let mut got = Vec::new();
                        let _ = app.read_to_end(&mut got).await;
                        got
                    };
                    let (r, got) = tokio::join!(e.tcp(local, SocketAddr::from(([93, 184, 216, 34], 2000 + i))), reader);
                    r.unwrap();
                    assert_eq!(got, b"ok");
                }
                assert_eq!(accepted.get(), 4, "все через живой выход");
            })
            .await;
    }

    #[test]
    fn stall_is_an_app_giving_up_on_a_silent_proxied_connection() {
        use crate::pipe::{Relayed, Side};
        let r =
            |down, eof, error_side, idle, quiet| Relayed { up: 68, down, idle, error: None, error_side, eof, quiet, gap: Duration::ZERO };
        let long = Duration::from_millis(10_240);
        assert!(stall(&r(0, None, Some(Side::Local), false, long), 68), "qBittorrent: сброс через 10 с тишины");
        assert!(stall(&r(0, Some(Side::Local), None, false, long), 68));
        assert!(!stall(&r(1, None, Some(Side::Local), false, long), 68), "ответ был");
        assert!(!stall(&r(0, None, Some(Side::Local), false, Duration::from_secs(1)), 68), "приложение быстро передумало");
        assert!(!stall(&r(0, Some(Side::Remote), None, false, long), 68), "закрыл сервер");
        assert!(!stall(&r(0, None, None, true, long), 68), "простой");
        assert!(!stall(&r(0, Some(Side::Local), None, false, long), 0), "ничего не слало");
    }

    #[test]
    fn stalled_app_is_rate_limited_until_it_gets_data() {
        let e = engine(RoutingMode::Global);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        for i in 0..STALL_FREE as u64 - 1 {
            e.stall_note("qbittorrent.exe", false, at(i));
        }
        assert!(!e.stalled("qbittorrent.exe", at(10)));
        e.stall_note("qbittorrent.exe", false, at(10));
        assert!(e.stalled("qbittorrent.exe", at(20)));
        assert!(!e.stalled("ayugram.exe", at(20)));
        let allowed = (0..20).filter(|_| e.stall_dial("qbittorrent.exe", at(20))).count();
        assert_eq!(allowed, 3, "пачка");
        assert!(!e.stall_dial("qbittorrent.exe", at(900)));
        assert!(e.stall_dial("qbittorrent.exe", at(1_020)), "дальше один в STALL_DIAL");
        assert!(!e.stall_dial("qbittorrent.exe", at(1_500)));
        assert!(!e.stalled("qbittorrent.exe", at(10) + STALL_GAP), "затихло — серия кончилась");
        e.stall_note("qbittorrent.exe", true, at(2_000));
        assert!(!e.stalled("qbittorrent.exe", at(2_001)) && e.stalls.borrow().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn smart_fallback_skips_stalled_apps() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (port, accepted) = fake_vless(std::rc::Rc::new(std::cell::Cell::new(true))).await;
                let owner: Owner = std::sync::Arc::new(|_, _, _| Some("torrent".into()));
                let opts = Options { routing: RoutingMode::Smart, owner: Some(owner), log_apps: true, ..Default::default() };
                let e = Engine::new(&local_vless(&[port], Value::Null), &GeoData::default(), None, &opts).unwrap();
                let dst = SocketAddr::from(([127, 0, 0, 1], closed_port()));
                let src: SocketAddr = "10.0.0.2:40000".parse().unwrap();
                let conn = || {
                    let e = &e;
                    async move {
                        let (mut app, local) = tokio::io::duplex(1024);
                        app.write_all(&[0xef; 64]).await.unwrap();
                        let reader = async {
                            let mut got = Vec::new();
                            let _ = app.read_to_end(&mut got).await;
                            got
                        };
                        tokio::join!(e.tcp_from(local, Some(src), dst), reader)
                    }
                };
                let (r, got) = conn().await;
                r.unwrap();
                assert_eq!((got.as_slice(), accepted.get()), (&b"ok"[..], 1), "отказ напрямую: через прокси");
                let now = Instant::now();
                for _ in 0..STALL_FREE {
                    e.stall_note("torrent", false, now);
                }
                let (r, _) = conn().await;
                assert!(r.is_err());
                assert_eq!(accepted.get(), 1, "приложению через сервер не отвечают: только напрямую");
            })
            .await;
    }

    #[test]
    fn own_connection_to_own_server_is_a_loop() {
        let owner: Owner = std::sync::Arc::new(|_, _, _| None);
        let opts = Options { routing: RoutingMode::Global, owner: Some(owner), log_apps: true, ..Default::default() };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts).unwrap();
        let own = std::env::current_exe().unwrap().file_name().unwrap().to_str().unwrap().to_string();
        let server: SocketAddr = "203.0.113.1:443".parse().unwrap();
        assert!(e.own_loop(Some(&own), "", server));
        assert!(e.own_loop(Some(&own[..own.len().min(15)]), "", server) || own.len() < 15);
        assert!(!e.own_loop(Some(&own), "", "203.0.113.1:80".parse().unwrap()), "другой порт");
        assert!(!e.own_loop(Some(&own), "sub.example", "198.51.100.7:443".parse().unwrap()), "подписка через туннель — можно");
        assert!(!e.own_loop(Some("chrome"), "", server), "чужое приложение к тому же адресу");
        assert!(!e.own_loop(None, "", server));
        assert!(!engine(RoutingMode::Global).own_loop(Some(&own), "", server), "без владельцев не проверить");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tailscale_dns_query_crosses_the_selected_proxy() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut io, _) = listener.accept().await.unwrap();
            let mut header = [0u8; 26];
            io.read_exact(&mut header).await.unwrap();
            assert_eq!(&header[18..], &[1, 0, 53, 1, 100, 100, 100, 100]);
            let mut len = [0u8; 2];
            io.read_exact(&mut len).await.unwrap();
            let mut query = vec![0u8; u16::from_be_bytes(len) as usize];
            io.read_exact(&mut query).await.unwrap();
            let q = crate::dns::question(&query).unwrap();
            assert_eq!(q.name, "nas.example.ts.net");
            let answer = crate::dns::answer(&query, &q, &["100.111.22.33".parse().unwrap()], 60);
            io.write_all(&[0, 0]).await.unwrap();
            io.write_all(&(answer.len() as u16).to_be_bytes()).await.unwrap();
            io.write_all(&answer).await.unwrap();
        });
        let mut p = profile();
        let mut selected = p.outbounds[0].clone();
        selected.tag = "rynet-app0".into();
        selected.stream = Default::default();
        if let OutboundKind::Vless(v) = &mut selected.kind {
            v.address = "127.0.0.1".into();
            v.port = port;
        }
        p.outbounds.push(selected);
        let e = Engine::new(
            &p,
            &GeoData::default(),
            None,
            &Options { user_rules: "DOMAIN-SUFFIX,ts.net,rynet-app0".into(), ..Default::default() },
        )
        .unwrap();
        assert_eq!(
            e.resolve_query("nas.example.ts.net", crate::dns::TYPE_A).await.unwrap(),
            vec!["100.111.22.33".parse::<IpAddr>().unwrap()]
        );
        server.await.unwrap();
    }

    #[test]
    fn tailscale_rules_win_over_private_bypass() {
        for routing in [RoutingMode::Global, RoutingMode::Smart, RoutingMode::Subscription, RoutingMode::Custom] {
            let opts = Options {
                routing,
                user_rules: "IP-CIDR,100.64.0.0/10,rynet-app0\nIP-CIDR6,fd7a:115c:a1e0::/48,rynet-app0\nDOMAIN-SUFFIX,ts.net,rynet-app0"
                    .into(),
                ..Default::default()
            };
            let mut p = profile();
            let mut selected = p.outbounds[0].clone();
            selected.tag = "rynet-app0".into();
            p.outbounds.push(selected);
            let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
            let Some(Out::Proxy(selected)) = e.outs.get("rynet-app0") else { panic!() };
            for ip in ["100.101.2.3", "fd7a:115c:a1e0::123"] {
                let ip = ip.parse().unwrap();
                let ctx = RouteCtx {
                    domain: None,
                    ip: Some(ip),
                    port: 443,
                    network: Network::Tcp,
                    protocol: None,
                    inbound_tag: None,
                    process: None,
                };
                assert!(matches!(e.route(&ctx), Out::Proxy(p) if Rc::ptr_eq(&p, selected)), "{routing:?}");
                assert!(!e.direct_nets().iter().any(|&(net, len)| in_net(net, len, ip)));
                assert!(e.tunnel_nets().iter().any(|&(net, len)| in_net(net, len, ip)));
            }
        }
    }

    #[test]
    fn tailscale_dns_uses_selected_server_without_fallback() {
        let mut p = profile();
        let mut selected = p.outbounds[0].clone();
        selected.tag = "rynet-app0".into();
        p.outbounds.push(selected);
        let opts = Options {
            routing: RoutingMode::Smart,
            fake_dns: true,
            user_rules: "DOMAIN-SUFFIX,ts.net,rynet-app0".into(),
            ..Default::default()
        };
        let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
        for name in ["nas.example.ts.net", "NAS.EXAMPLE.TS.NET.", "ts.net"] {
            let order = e.dns_order(Some(name), false);
            assert_eq!(order.len(), 1);
            let i = order[0];
            assert_eq!(e.dns_label(i), "100.100.100.100:53");
            let Some(Out::Proxy(selected)) = e.outs.get("rynet-app0") else { panic!() };
            assert!(matches!(e.dns_out(i), Out::Proxy(p) if Rc::ptr_eq(&p, selected)));
            assert!(!e.fake_for(name, i));
            assert!(
                matches!(e.dns_route(&crate::dns::query_for(name.trim_end_matches('.'), crate::dns::TYPE_A)), DnsTarget::Servers(hops) if hops.len() == 1 && hops[0].0 == DnsHop::Stream(i))
            );
        }
        assert!(e.tailnet_dns("not-ts.net").is_none());
        let e = engine(RoutingMode::Global);
        assert!(e.tailnet_dns("nas.example.ts.net").is_none());
        for action in ["DIRECT", "REJECT"] {
            let opts = Options {
                user_rules: format!("DOMAIN,nas.example.ts.net,{action}\nDOMAIN-SUFFIX,ts.net,rynet-app0"),
                ..Default::default()
            };
            let e = Engine::new(&p, &GeoData::default(), None, &opts).unwrap();
            assert!(e.tailnet_dns("nas.example.ts.net").is_none());
            assert!(e.tailnet_dns("other.example.ts.net").is_some());
        }
    }

    #[test]
    fn direct_nets_for_tun_bypass() {
        let nets = |e: &Engine| e.direct_nets().iter().map(|(ip, len)| format!("{ip}/{len}")).collect::<Vec<_>>();
        let own = "IP-CIDR,100.64.0.0/10,DIRECT\nIP-CIDR6,fd7a:115c:a1e0::/48,DIRECT\nIP-CIDR,0.0.0.0/0,DIRECT\nIP-CIDR,1.0.0.0/4,DIRECT\n\
            IP-CIDR,203.0.113.0/24,DIRECT\nIP-CIDR,198.19.4.0/24,DIRECT\nIP-CIDR6,::1/128,DIRECT\nIP-CIDR,10.20.0.0/16,PROXY\n\
            IP-CIDR,10.20.30.0/24,DIRECT\nIP-CIDR,192.0.2.0/24,DIRECT";
        let opts = |routing| Options { user_rules: own.into(), routing, ..Default::default() };
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts(RoutingMode::Global)).unwrap();
        assert_eq!(nets(&e), ["100.64.0.0/10", "fd7a:115c:a1e0::/48", "192.0.2.0/24", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16"]);
        let e = Engine::new(&profile(), &GeoData::default(), None, &opts(RoutingMode::Smart)).unwrap();
        assert_eq!(nets(&e), ["100.64.0.0/10", "fd7a:115c:a1e0::/48", "192.0.2.0/24"]);
        let text = r#"[{"remarks": "t", "outbounds": [
            {"tag": "proxy", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.1", "port": 443,
                "users": [{"id": "9205c503-358f-4d8d-a081-20c98eddf64a", "encryption": "none"}]}]}},
            {"tag": "direct", "protocol": "freedom"}
        ], "routing": {"rules": [
            {"type": "field", "ip": ["198.51.100.0/24"], "outboundTag": "direct"},
            {"type": "field", "domain": ["domain:example.com"], "outboundTag": "proxy"},
            {"type": "field", "ip": ["100.100.0.0/16"], "outboundTag": "direct"}
        ]}}]"#;
        let p = rv_config::parse_subscription(text).unwrap().remove(0);
        let e = Engine::new(&p, &GeoData::default(), None, &Options { routing: RoutingMode::Subscription, ..Default::default() }).unwrap();
        assert_eq!(nets(&e), ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16", "198.51.100.0/24"]);
        let e = Engine::new(
            &profile(),
            &GeoData::default(),
            None,
            &Options { user_rules: format!("DOMAIN-SUFFIX,example.com,PROXY\n{own}"), ..opts(RoutingMode::Global) },
        )
        .unwrap();
        assert!(nets(&e).is_empty(), "{:?}", nets(&e));
    }
}

#[cfg(test)]
mod recovery_policy_tests {
    use super::*;
    #[test]
    fn routing_snapshot_retains_conjunctive_rule_and_geo_references() {
        let cfg = serde_json::json!({"remarks":"test","outbounds":[{"tag":"proxy","protocol":"socks","settings":{"servers":[{"address":"127.0.0.1","port":1080}]}},{"tag":"direct","protocol":"freedom"}],
            "routing":{"domainStrategy":"IPIfNonMatch","rules":[{"type":"field","domain":["geosite:category-ru"],"ip":["geoip:ru"],"network":"tcp","port":"443","inboundTag":["app"],"outboundTag":"direct"}]},
            "dns":{"servers":[{"address":"8.8.8.8","domains":["geosite:category-ru"],"skipFallback":true}]}});
        let mut opts = Options { user_rules: "DOMAIN,own.test,REJECT".into(), ..Default::default() };
        let p = rv_config::parse_subscription(&cfg.to_string()).unwrap().remove(0);
        assert!(Engine::new(&p, &GeoData::default(), None, &opts).unwrap().routing_view.is_null());
        let s = Engine::inspect(&p, &GeoData::default(), &opts).unwrap();
        assert_eq!(s["rules"][0]["source"], "user");
        assert_eq!(s["rules"][1]["source"], "local");
        assert_eq!(s["rules"][2]["source"], "subscription");
        assert_eq!(s["rules"][2]["rule"], cfg["routing"]["rules"][0]);
        assert_eq!(s["dns"]["servers"][0]["definition"], cfg["dns"]["servers"][0]);
        opts.routing = RoutingMode::Smart;
        let s = Engine::inspect(&p, &GeoData::default(), &opts).unwrap();
        assert_eq!(s["rules"].as_array().unwrap().iter().filter(|r| r["source"] == "smart").count(), 5);
    }
}
