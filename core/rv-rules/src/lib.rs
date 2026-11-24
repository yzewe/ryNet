pub mod mihomo;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, AhoCorasickKind, MatchKind};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    str::FromStr,
    sync::{Arc, Mutex, PoisonError},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("routing: {0}")]
    Invalid(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    Tcp,
    Udp,
}
#[derive(Clone, Debug)]
pub struct RouteCtx<'a> {
    pub domain: Option<&'a str>,
    pub ip: Option<IpAddr>,
    pub port: u16,
    pub network: Network,
    pub protocol: Option<&'a str>,
    pub inbound_tag: Option<&'a str>,
    pub process: Option<&'a str>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Outbound(String),
    Balancer(String),
    Default,
}
#[derive(Default, Clone, Debug)]
pub struct GeoData {
    pub geoip: HashMap<String, Vec<(IpAddr, u8)>>,
    pub geosite: HashMap<String, Vec<String>>,
    compiled: Compiled,
}
#[derive(Default)]
struct Compiled {
    ip: Mutex<HashMap<String, Arc<IpSet>>>,
    site: Mutex<HashMap<String, Arc<Site>>>,
}
impl Clone for Compiled {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl std::fmt::Debug for Compiled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Compiled")
    }
}
impl GeoData {
    fn ip_set(&self, code: &str) -> Option<Arc<IpSet>> {
        let key = code.to_ascii_lowercase();
        let mut cache = self.compiled.ip.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(set) = cache.get(&key) {
            return Some(set.clone());
        }
        let (mut v4, mut v6) = (Vec::new(), Vec::new());
        let mut add = |s: &str| {
            if let Some((four, r)) = parse_ip_range(s) {
                if four { v4.push(r) } else { v6.push(r) }
            }
        };
        if key == "private" {
            PRIVATE.iter().for_each(|s| add(s));
        } else {
            for (ip, prefix) in geo_entry(&self.geoip, code)? {
                add(&format!("{ip}/{prefix}"));
            }
        }
        let set = Arc::new(IpSet::new(v4, v6));
        cache.insert(key, set.clone());
        Some(set)
    }

    fn site(&self, cat: &str) -> Result<Option<Arc<Site>>, Error> {
        let key = cat.to_ascii_lowercase();
        let mut cache = self.compiled.site.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(site) = cache.get(&key) {
            return Ok(Some(site.clone()));
        }
        let Some(xs) = geo_entry(&self.geosite, cat) else {
            return Ok(None);
        };
        let mut domains = Domains::default();
        let mut warnings = Vec::new();
        let mut cats = HashSet::from([key.clone()]);
        for x in xs {
            domains.push(x, self, &mut cats, &mut warnings);
        }
        let site = Arc::new(Site { matcher: domains.build(&mut warnings)?, warnings });
        cache.insert(key, site.clone());
        Ok(Some(site))
    }
}
const PRIVATE: [&str; 18] = [
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.0.0.0/24",
    "192.0.2.0/24",
    "192.88.99.0/24",
    "192.168.0.0/16",
    "198.18.0.0/15",
    "198.51.100.0/24",
    "203.0.113.0/24",
    "224.0.0.0/3",
    "::/127",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];

#[derive(Clone, Copy)]
struct IpRange {
    lo: u128,
    hi: u128,
}
struct IpSet {
    v4: Box<[(u32, u32)]>,
    v6: Box<[IpRange]>,
}
impl IpSet {
    fn new(mut v4: Vec<IpRange>, mut v6: Vec<IpRange>) -> Self {
        merge(&mut v4);
        merge(&mut v6);
        let v4 = v4.into_iter().map(|r| (r.lo as u32, r.hi as u32)).collect();
        IpSet { v4, v6: v6.into_boxed_slice() }
    }

    fn is_empty(&self) -> bool {
        self.v4.is_empty() && self.v6.is_empty()
    }

    fn overlaps(&self, four: bool, r: IpRange) -> bool {
        let (lo, hi) = self.around(four, r.hi);
        lo <= hi && hi >= r.lo
    }

    fn covers(&self, four: bool, r: IpRange) -> bool {
        let (lo, hi) = self.around(four, r.lo);
        lo <= r.lo && hi >= r.hi
    }

    fn around(&self, four: bool, at: u128) -> (u128, u128) {
        let found = if four {
            let i = self.v4.partition_point(|r| r.0 as u128 <= at);
            (i > 0).then(|| (self.v4[i - 1].0 as u128, self.v4[i - 1].1 as u128))
        } else {
            let i = self.v6.partition_point(|r| r.lo <= at);
            (i > 0).then(|| (self.v6[i - 1].lo, self.v6[i - 1].hi))
        };
        found.unwrap_or((1, 0))
    }

    fn contains(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(v) => {
                let value = u32::from(v);
                let i = self.v4.partition_point(|r| r.0 <= value);
                i > 0 && value <= self.v4[i - 1].1
            }
            IpAddr::V6(v) => contains(&self.v6, u128::from(v)),
        }
    }
}
struct IpMatcher {
    sets: Vec<Arc<IpSet>>,
    negate: bool,
}
impl IpMatcher {
    fn matches(&self, ip: IpAddr) -> bool {
        self.sets.iter().any(|s| s.contains(ip)) != self.negate
    }

    fn may_match(&self, four: bool, r: IpRange) -> bool {
        if self.negate { !self.sets.iter().any(|s| s.covers(four, r)) } else { self.sets.iter().any(|s| s.overlaps(four, r)) }
    }
}
#[derive(Default)]
struct IpSide {
    v4: Vec<IpRange>,
    v6: Vec<IpRange>,
    sets: Vec<Arc<IpSet>>,
}
impl IpSide {
    fn matcher(self, negate: bool) -> Option<IpMatcher> {
        let mut sets = self.sets;
        if !self.v4.is_empty() || !self.v6.is_empty() {
            sets.push(Arc::new(IpSet::new(self.v4, self.v6)));
        }
        (!sets.is_empty()).then_some(IpMatcher { sets, negate })
    }
}
fn contains(rs: &[IpRange], value: u128) -> bool {
    let i = rs.partition_point(|r| r.lo <= value);
    i > 0 && value <= rs[i - 1].hi
}
fn merge(rs: &mut Vec<IpRange>) {
    rs.sort_by_key(|r| r.lo);
    let mut out: Vec<IpRange> = Vec::with_capacity(rs.len());
    for r in rs.drain(..) {
        match out.last_mut() {
            Some(last) if r.lo <= last.hi.saturating_add(1) => {
                last.hi = last.hi.max(r.hi);
                continue;
            }
            _ => {}
        }
        out.push(r);
    }
    *rs = out;
}
fn cidr(s: &str) -> Option<(IpAddr, u8)> {
    let (four, r) = parse_ip_range(s)?;
    let len = match s.split_once('/') {
        Some((_, l)) => l.parse().ok()?,
        None if four => 32,
        None => 128,
    };
    Some((if four { IpAddr::from((r.lo as u32).to_be_bytes()) } else { IpAddr::from(r.lo.to_be_bytes()) }, len))
}
fn parse_ip_range(s: &str) -> Option<(bool, IpRange)> {
    let (ip_s, prefix_s) = s.split_once('/').unwrap_or((s, ""));
    let ip = IpAddr::from_str(ip_s).ok()?;
    let bits = if prefix_s.is_empty() {
        match ip {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        }
    } else {
        prefix_s.parse::<u32>().ok()?
    };
    match ip {
        IpAddr::V4(v) if bits <= 32 => {
            let n = u32::from(v);
            let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
            Some((true, IpRange { lo: (n & mask) as u128, hi: ((n & mask) | !mask) as u128 }))
        }
        IpAddr::V6(v) if bits <= 128 => {
            let n = u128::from(v);
            let mask = if bits == 0 { 0 } else { u128::MAX << (128 - bits) };
            Some((false, IpRange { lo: n & mask, hi: (n & mask) | !mask }))
        }
        _ => None,
    }
}

enum DomainMatcher {
    Sub(AhoCorasick),
    Suffix(HashSet<String>),
    Regex(regex_lite::Regex),
    Any(Vec<DomainMatcher>),
    Site(Arc<Site>),
}
struct Site {
    matcher: Option<DomainMatcher>,
    warnings: Vec<String>,
}
impl DomainMatcher {
    fn is_match(&self, domain: &str) -> bool {
        match self {
            Self::Sub(ac) => ac.is_match(domain),
            Self::Regex(re) => re.is_match(&domain.to_ascii_lowercase()),
            Self::Suffix(set) => {
                let d = domain.to_ascii_lowercase();
                if set.contains(&format!("={d}")) {
                    return true;
                }
                let mut rest = d.as_str();
                loop {
                    if set.contains(rest) {
                        return true;
                    }
                    match rest.find('.') {
                        Some(n) => rest = &rest[n + 1..],
                        None => return false,
                    }
                }
            }
            Self::Any(ms) => ms.iter().any(|m| m.is_match(domain)),
            Self::Site(site) => site.matcher.as_ref().is_some_and(|m| m.is_match(domain)),
        }
    }

    fn has_regex(&self) -> bool {
        match self {
            Self::Regex(_) => true,
            Self::Any(ms) => ms.iter().any(Self::has_regex),
            Self::Site(site) => site.matcher.as_ref().is_some_and(Self::has_regex),
            Self::Sub(_) | Self::Suffix(_) => false,
        }
    }
}

const MEMO_MAX: usize = 256;
#[derive(Default)]
struct Memo(Mutex<HashMap<Box<str>, bool>>);
impl Memo {
    fn get(&self, d: &str, f: impl FnOnce() -> bool) -> bool {
        if let Some(&hit) = self.0.lock().unwrap_or_else(PoisonError::into_inner).get(d) {
            return hit;
        }
        let hit = f();
        let mut m = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if m.len() >= MEMO_MAX {
            m.clear();
        }
        m.insert(d.into(), hit);
        hit
    }
}
struct Rule {
    domain: Option<DomainMatcher>,
    memo: Option<Memo>,
    domain_present: bool,
    ip_present: bool,
    ips: Vec<IpMatcher>,
    ports: Vec<(u16, u16)>,
    source_ports: Vec<(u16, u16)>,
    networks: Vec<Network>,
    protocols: Vec<String>,
    inbounds: Vec<String>,
    processes: Vec<String>,
    tag: Option<String>,
    balancer: Option<String>,
    nets: Vec<(IpAddr, u8)>,
}
impl Rule {
    fn may_catch(&self, four: bool, r: IpRange) -> bool {
        if !self.source_ports.is_empty() || (self.ip_present && self.ips.is_empty()) {
            return false;
        }
        self.ips.is_empty() || self.ips.iter().any(|m| m.may_match(four, r))
    }

    fn domain_match(&self, d: &str) -> bool {
        let Some(m) = &self.domain else { return false };
        match &self.memo {
            Some(memo) => memo.get(d, || m.is_match(d)),
            None => m.is_match(d),
        }
    }
    fn matches(&self, c: &RouteCtx<'_>) -> bool {
        if self.domain_present && !c.domain.is_some_and(|d| self.domain_match(d)) {
            return false;
        }
        if self.ip_present && self.ips.is_empty() {
            return false;
        }
        if !self.ips.is_empty() && !c.ip.is_some_and(|ip| self.ips.iter().any(|m| m.matches(ip))) {
            return false;
        }
        if !self.ports.is_empty() && !port_contains(&self.ports, c.port) {
            return false;
        }
        if !self.source_ports.is_empty() {
            return false;
        }
        if !self.networks.is_empty() && !self.networks.contains(&c.network) {
            return false;
        }
        if !self.protocols.is_empty() && !c.protocol.is_some_and(|p| self.protocols.iter().any(|x| p.starts_with(x))) {
            return false;
        }
        if !self.inbounds.is_empty() && !c.inbound_tag.is_some_and(|p| self.inbounds.iter().any(|x| x == p)) {
            return false;
        }
        if !self.processes.is_empty() && !c.process.is_some_and(|p| self.processes.iter().any(|x| x.eq_ignore_ascii_case(p))) {
            return false;
        }
        true
    }
}
fn port_contains(rs: &[(u16, u16)], p: u16) -> bool {
    let i = rs.partition_point(|r| r.0 <= p);
    i > 0 && p <= rs[i - 1].1
}
fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}
fn field(v: &Value, name: &str) -> Vec<String> {
    v.get(name).map(strings).unwrap_or_default()
}
fn port_ranges(v: &Value) -> Vec<(u16, u16)> {
    let items: Vec<String> = match v {
        Value::Number(n) => vec![n.to_string()],
        Value::Array(a) => a
            .iter()
            .flat_map(|x| match x {
                Value::Number(n) => vec![n.to_string()],
                other => strings(other),
            })
            .collect(),
        other => strings(other),
    };
    let mut out = Vec::new();
    for s in items {
        let (a, b) = s.split_once('-').unwrap_or((&s, &s));
        match (a.trim().parse::<u16>(), b.trim().parse::<u16>()) {
            (Ok(a), Ok(b)) if a <= b => out.push((a, b)),
            _ => {}
        }
    }
    out.sort();
    let mut merged: Vec<(u16, u16)> = Vec::with_capacity(out.len());
    for (a, b) in out {
        match merged.last_mut() {
            Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}
fn add_range(s: &str, geo: &GeoData, codes: &mut HashSet<(bool, String)>, pos: &mut IpSide, neg: &mut IpSide, warnings: &mut Vec<String>) {
    let (negate, src) = s.strip_prefix('!').map_or((false, s), |x| (true, x));
    let side = if negate { neg } else { pos };
    if let Some(code) = src.strip_prefix("geoip:") {
        if !codes.insert((negate, code.to_ascii_lowercase())) {
            return;
        }
        match geo.ip_set(code) {
            Some(set) if set.is_empty() => {}
            Some(set) => side.sets.push(set),
            None => warnings.push(format!("missing GeoIP data for {src}; value skipped")),
        }
    } else if let Some((four, r)) = parse_ip_range(src) {
        if four { side.v4.push(r) } else { side.v6.push(r) }
    } else {
        warnings.push(format!("unsupported IP rule {s:?}; value skipped"));
    }
}
fn geo_entry<'a, T>(map: &'a HashMap<String, T>, code: &str) -> Option<&'a T> {
    map.get(code).or_else(|| map.get(&code.to_ascii_uppercase())).or_else(|| map.get(&code.to_ascii_lowercase()))
}
#[derive(Default)]
struct Domains {
    subs: Vec<String>,
    suffix: HashSet<String>,
    regexes: Vec<String>,
}
impl Domains {
    fn push(&mut self, e: &str, geo: &GeoData, cats: &mut HashSet<String>, warnings: &mut Vec<String>) {
        if let Some(cat) = e.strip_prefix("geosite:") {
            if !cats.insert(cat.to_ascii_lowercase()) {
                return;
            }
            if let Some(xs) = geo_entry(&geo.geosite, cat) {
                for x in xs {
                    self.push(x, geo, cats, warnings);
                }
            } else {
                warnings.push(format!("missing GeoSite data for {e}; value skipped"));
            }
            return;
        }
        if e.starts_with("ext:") || e.starts_with("ext-domain:") || e.starts_with("ext-site:") {
            warnings.push(format!("unsupported domain rule {e:?}; value skipped"));
            return;
        }
        if e.starts_with("*.") {
            warnings.push(format!("domain entry {e:?} is a literal substring in Xray; likely panel template bug"));
        }
        if let Some(x) = e.strip_prefix("regexp:") {
            self.regexes.push(x.to_owned());
        } else if let Some(x) = e.strip_prefix("domain:") {
            self.suffix.insert(x.to_ascii_lowercase());
        } else if let Some(x) = e.strip_prefix("full:") {
            self.suffix.insert(format!("={}", x.to_ascii_lowercase()));
        } else {
            let x = e.strip_prefix("keyword:").unwrap_or(e);
            if !x.is_empty() {
                self.subs.push(x.to_owned());
            }
        }
    }

    fn build(self, warnings: &mut Vec<String>) -> Result<Option<DomainMatcher>, Error> {
        let mut matchers = Vec::new();
        if !self.subs.is_empty() {
            match AhoCorasickBuilder::new()
                .ascii_case_insensitive(true)
                .match_kind(MatchKind::Standard)
                .kind(Some(AhoCorasickKind::ContiguousNFA))
                .build(&self.subs)
            {
                Ok(m) => matchers.push(DomainMatcher::Sub(m)),
                Err(err) => warnings.push(format!("could not compile domain substrings: {err}")),
            }
        }
        if !self.suffix.is_empty() {
            matchers.push(DomainMatcher::Suffix(self.suffix));
        }
        if !self.regexes.is_empty() {
            let mut valid = Vec::new();
            for pattern in self.regexes {
                if let Err(err) = regex_lite::RegexBuilder::new(&pattern).size_limit(REGEX_SIZE_MAX).build() {
                    return Err(Error::Invalid(format!("invalid domain regex {pattern:?}: {err}")));
                } else {
                    valid.push(pattern);
                }
            }
            if !valid.is_empty() {
                let joined = valid.iter().map(|p| format!("(?:{p})")).collect::<Vec<_>>().join("|");
                matchers.push(DomainMatcher::Regex(compile_budgeted(&joined)?));
            }
        }
        Ok((!matchers.is_empty()).then_some(DomainMatcher::Any(matchers)))
    }
}
const REGEX_SIZE_MAX: usize = 1 << 20;

thread_local! {
    static REGEX_BUDGET: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

pub struct RegexBudget(Option<usize>);

impl RegexBudget {
    pub fn new(bytes: usize) -> Self {
        Self(REGEX_BUDGET.with(|b| b.replace(Some(bytes))))
    }
}

impl Drop for RegexBudget {
    fn drop(&mut self) {
        REGEX_BUDGET.with(|b| b.set(self.0));
    }
}

const REGEX_TIERS: [usize; 4] = [16 << 10, 64 << 10, 256 << 10, REGEX_SIZE_MAX];

fn compile_budgeted(joined: &str) -> Result<regex_lite::Regex, Error> {
    let left = REGEX_BUDGET.with(|b| b.get());
    let mut last = None;
    for tier in REGEX_TIERS {
        if left.is_some_and(|l| tier > l) {
            return Err(Error::Invalid("domain regexes of the config take too much memory".into()));
        }
        match regex_lite::RegexBuilder::new(joined).size_limit(tier).build() {
            Ok(r) => {
                if let Some(l) = left {
                    REGEX_BUDGET.with(|b| b.set(Some(l - tier)));
                }
                return Ok(r);
            }
            Err(err) => last = Some(err),
        }
    }
    Err(Error::Invalid(format!("could not compile domain regex set: {}", last.map(|e| e.to_string()).unwrap_or_default())))
}

fn compile_domains(entries: &[String], geo: &GeoData, warnings: &mut Vec<String>) -> Result<Option<DomainMatcher>, Error> {
    let mut own = Domains::default();
    let mut matchers = Vec::new();
    let mut cats = HashSet::new();
    for e in entries {
        match e.strip_prefix("geosite:") {
            Some(cat) if !cats.insert(cat.to_ascii_lowercase()) => {}
            Some(cat) => match geo.site(cat)? {
                Some(site) => {
                    warnings.extend(site.warnings.iter().cloned());
                    if site.matcher.is_some() {
                        matchers.push(DomainMatcher::Site(site));
                    }
                }
                None => warnings.push(format!("missing GeoSite data for {e}; value skipped")),
            },
            None => own.push(e, geo, &mut cats, warnings),
        }
    }
    matchers.extend(own.build(warnings)?);
    Ok((!matchers.is_empty()).then_some(DomainMatcher::Any(matchers)))
}

pub struct Router {
    rules: Vec<Rule>,
    warnings: Vec<String>,
    strategy: DomainStrategy,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum DomainStrategy {
    AsIs,
    IpIfNonMatch,
    IpOnDemand,
}
impl Router {
    pub fn new(routing: &Value, geo: &GeoData) -> Result<Router, Error> {
        let list = routing.get("rules").and_then(Value::as_array).ok_or_else(|| Error::Invalid("routing.rules must be an array".into()))?;
        let mut warnings = Vec::new();
        let mut rules = Vec::new();
        for (idx, v) in list.iter().enumerate() {
            if !v.get("type").and_then(Value::as_str).is_none_or(|t| t.eq_ignore_ascii_case("field")) {
                warnings.push(format!("rule {idx}: only type=field is supported; rule ignored"));
                continue;
            }
            let mut dom = Vec::new();
            let mut dom_present = false;
            if v.get("domain").is_some() {
                dom_present = true;
                dom.extend(field(v, "domain"));
            }
            if v.get("domains").is_some() {
                dom_present = true;
                dom = field(v, "domains");
            }
            dom_present &= !dom.is_empty();
            let domain = if dom_present { compile_domains(&dom, geo, &mut warnings)? } else { None };
            let mut pos = IpSide::default();
            let mut neg = IpSide::default();
            let mut codes = HashSet::new();
            let ip_values = field(v, "ip");
            let ip_present = !ip_values.is_empty();
            let nets: Option<Vec<(IpAddr, u8)>> = ip_values.iter().map(|s| cidr(s)).collect();
            for ip in ip_values {
                add_range(&ip, geo, &mut codes, &mut pos, &mut neg, &mut warnings);
            }
            let ips: Vec<IpMatcher> = [pos.matcher(false), neg.matcher(true)].into_iter().flatten().collect();
            let ports = v.get("port").map(port_ranges).unwrap_or_default();
            if v.get("port").is_some_and(|p| !p.is_null()) && ports.is_empty() {
                warnings.push(format!("rule {idx}: unreadable port {}; rule ignored", v["port"]));
                continue;
            }
            if let Some(k) = ["sourceIP", "source", "user", "vlessRoute", "attrs", "localIP", "localPort"]
                .iter()
                .find(|k| v.get(**k).is_some_and(|x| !x.is_null()))
            {
                warnings.push(format!("rule {idx}: condition `{k}` cannot be evaluated on the client; rule ignored"));
                continue;
            }
            let os = field(v, "localOS");
            if !os.is_empty() && !os.iter().any(|o| o.eq_ignore_ascii_case("android")) {
                continue;
            }
            let source_ports = v.get("sourcePort").map(port_ranges).unwrap_or_default();
            if !source_ports.is_empty() {
                warnings.push(format!(
                    "rule {idx}: sourcePort cannot be evaluated because RouteCtx has no source port; such rules will not match"
                ));
            }
            let networks: Vec<Network> = field(v, "network")
                .iter()
                .filter_map(|s| match s.to_ascii_lowercase().as_str() {
                    "tcp" => Some(Network::Tcp),
                    "udp" => Some(Network::Udp),
                    _ => None,
                })
                .collect();
            let target = v.get("outboundTag").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
            let balancer = v.get("balancerTag").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
            if target.is_none() && balancer.is_none() {
                warnings.push(format!("rule {idx}: missing outboundTag/balancerTag; rule ignored"));
                continue;
            }
            let protocols = field(v, "protocol");
            let inbounds = field(v, "inboundTag");
            let processes = [field(v, "processName"), field(v, "process")].concat();
            let plain = ip_present
                && !dom_present
                && ports.is_empty()
                && source_ports.is_empty()
                && (networks.is_empty() || (networks.contains(&Network::Tcp) && networks.contains(&Network::Udp)))
                && protocols.is_empty()
                && inbounds.is_empty()
                && processes.is_empty();
            rules.push(Rule {
                nets: nets.filter(|_| plain).unwrap_or_default(),
                memo: domain.as_ref().is_some_and(DomainMatcher::has_regex).then(Memo::default),
                domain,
                domain_present: dom_present,
                ip_present,
                ips,
                ports,
                source_ports,
                networks,
                protocols,
                inbounds,
                processes,
                tag: target,
                balancer,
            });
        }
        let strategy = match routing.get("domainStrategy").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase().as_str() {
            "ipifnonmatch" => DomainStrategy::IpIfNonMatch,
            "ipondemand" => DomainStrategy::IpOnDemand,
            _ => DomainStrategy::AsIs,
        };
        Ok(Router { rules, warnings, strategy })
    }
    pub fn uses_process(&self) -> bool {
        self.rules.iter().any(|r| !r.processes.is_empty())
    }

    pub fn route(&self, ctx: &RouteCtx<'_>) -> Decision {
        self.route_resolved(ctx, &[])
    }
    pub fn route_resolved(&self, ctx: &RouteCtx<'_>, ips: &[IpAddr]) -> Decision {
        for r in &self.rules {
            let mut c = ctx.clone();
            let matched = if !r.ip_present || ips.is_empty() {
                r.matches(ctx)
            } else {
                ips.iter().any(|ip| {
                    c.ip = Some(*ip);
                    r.matches(&c)
                })
            };
            if matched {
                if let Some(t) = &r.tag {
                    return Decision::Outbound(t.clone());
                }
                if let Some(t) = &r.balancer {
                    return Decision::Balancer(t.clone());
                }
            }
        }
        Decision::Default
    }
    pub fn needs_ip_lookup(&self, ctx: &RouteCtx<'_>) -> bool {
        if ctx.domain.is_none() || ctx.ip.is_some() {
            return false;
        }
        match self.strategy {
            DomainStrategy::AsIs => false,
            DomainStrategy::IpIfNonMatch => self.route(ctx) == Decision::Default,
            DomainStrategy::IpOnDemand => {
                for r in &self.rules {
                    if r.domain_present && !ctx.domain.is_some_and(|d| r.domain_match(d)) {
                        continue;
                    }
                    if r.ip_present {
                        return true;
                    }
                    if r.matches(ctx) {
                        return false;
                    }
                }
                false
            }
        }
    }
    pub fn needs_domain(&self) -> bool {
        self.rules.iter().any(|r| r.domain_present)
    }
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
    pub fn direct_nets(&self, direct: impl Fn(&str) -> bool) -> Vec<(IpAddr, u8)> {
        let goes_direct = |r: &Rule| r.tag.as_deref().is_some_and(&direct);
        let mut out = Vec::new();
        for (i, r) in self.rules.iter().enumerate() {
            if !goes_direct(r) {
                continue;
            }
            for &(ip, len) in &r.nets {
                let Some((four, range)) = parse_ip_range(&format!("{ip}/{len}")) else { continue };
                if !out.contains(&(ip, len)) && !self.rules[..i].iter().any(|e| !goes_direct(e) && e.may_catch(four, range)) {
                    out.push((ip, len));
                }
            }
        }
        out
    }
    pub fn explicit_nets(&self, target: impl Fn(&str) -> bool) -> Vec<(IpAddr, u8)> {
        let mut out = Vec::new();
        for r in &self.rules {
            if r.tag.as_deref().is_some_and(&target) {
                for &n in &r.nets {
                    if !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
        out
    }

    pub fn domain_goes_to(&self, domain: &str, outbound: &str) -> bool {
        self.rules.iter().any(|r| r.tag.as_deref() == Some(outbound) && r.domain_present && r.domain_match(domain))
    }
}

#[cfg(kani)]
#[path = "proofs/lib.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_build_a_linear_nfa_not_a_dfa() {
        let mut d = Domains::default();
        d.subs.push("keyword".into());
        let Some(DomainMatcher::Any(m)) = d.build(&mut Vec::new()).unwrap() else { panic!() };
        let DomainMatcher::Sub(ac) = &m[0] else { panic!() };
        assert_eq!(ac.kind(), aho_corasick::AhoCorasickKind::ContiguousNFA);
    }

    #[test]
    fn regexes_of_all_rules_share_one_budget() {
        let rules = |n: usize| {
            let list: Vec<Value> = (0..n)
                .map(|i| serde_json::json!({ "domain": [format!("regexp:(?:[a-z]{{40}}){{20}}x{i}")], "outboundTag": "proxy" }))
                .collect();
            Router::new(&serde_json::json!({ "rules": list }), &GeoData::default())
        };
        assert!(rules(64).is_ok());
        {
            let _b = RegexBudget::new(1 << 20);
            assert!(rules(2).is_ok());
            let err = rules(64).err().expect("over the budget").to_string();
            assert!(err.contains("too much memory"), "{err}");
        }
        assert!(rules(64).is_ok());
        let _b = RegexBudget::new(8 << 20);
        let plain: Vec<Value> = (0..400)
            .map(|i| serde_json::json!({ "domain": [format!("regexp:^ads{i}\\.example\\.com$")], "outboundTag": "block" }))
            .collect();
        assert!(Router::new(&serde_json::json!({ "rules": plain }), &GeoData::default()).is_ok());
    }

    fn direct(rules: Value) -> Vec<String> {
        let r = Router::new(&serde_json::json!({ "rules": rules }), &GeoData::default()).unwrap();
        r.direct_nets(|t| t == "direct" || t == "direct2").iter().map(|(ip, len)| format!("{ip}/{len}")).collect()
    }

    #[test]
    fn direct_nets_follow_rule_order() {
        use serde_json::json;
        assert_eq!(
            direct(json!([
                { "ip": ["100.64.0.0/10", "fd7a:115c:a1e0::/48", "192.0.2.7"], "outboundTag": "direct" },
                { "ip": "100.100.1.2/10", "outboundTag": "direct2" },
                { "network": "tcp,udp", "outboundTag": "proxy" },
                { "ip": ["198.51.100.0/24"], "outboundTag": "direct" },
            ])),
            ["100.64.0.0/10", "fd7a:115c:a1e0::/48", "192.0.2.7/32"]
        );
        assert_eq!(
            direct(json!([
                { "ip": ["10.1.0.0/16"], "outboundTag": "proxy" },
                { "ip": ["10.0.0.0/8", "10.1.2.0/24", "10.2.0.0/16", "fd00::/8"], "outboundTag": "direct" },
            ])),
            ["10.2.0.0/16", "fd00::/8"]
        );
        for before in [
            json!({ "domain": ["domain:example.com"], "outboundTag": "proxy" }),
            json!({ "port": "443", "outboundTag": "block" }),
            json!({ "processName": ["curl"], "outboundTag": "proxy" }),
            json!({ "ip": ["10.0.0.0/8"], "port": "53", "balancerTag": "b" }),
            json!({ "ip": ["!192.168.0.0/16"], "outboundTag": "proxy" }),
        ] {
            assert_eq!(direct(json!([before, { "ip": ["10.9.0.0/16"], "outboundTag": "direct" }])), Vec::<String>::new(), "{before}");
        }
        assert_eq!(
            direct(json!([
                { "ip": ["!10.0.0.0/8"], "outboundTag": "proxy" },
                { "ip": ["10.9.0.0/16"], "sourcePort": "1000", "outboundTag": "proxy" },
                { "domain": ["domain:example.com"], "outboundTag": "direct" },
                { "ip": ["10.9.0.0/16"], "outboundTag": "direct" },
            ])),
            ["10.9.0.0/16"]
        );
        for rule in [
            json!({ "ip": ["10.9.0.0/16"], "port": "80", "outboundTag": "direct" }),
            json!({ "ip": ["10.9.0.0/16"], "network": "udp", "outboundTag": "direct" }),
            json!({ "ip": ["10.9.0.0/16"], "domain": ["domain:lan"], "outboundTag": "direct" }),
            json!({ "ip": ["10.9.0.0/16", "geoip:private"], "outboundTag": "direct" }),
            json!({ "ip": ["!10.9.0.0/16"], "outboundTag": "direct" }),
            json!({ "ip": ["10.9.0.0/16"], "outboundTag": "proxy" }),
        ] {
            assert_eq!(direct(json!([rule])), Vec::<String>::new(), "{rule}");
        }
        assert_eq!(direct(json!([{ "ip": ["10.9.0.0/16"], "network": "tcp,udp", "outboundTag": "direct" }])), ["10.9.0.0/16"]);
    }
}
