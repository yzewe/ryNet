use std::collections::HashMap;

use base64::Engine;
use serde_json::Value;

mod http_header;
pub mod links;
pub use http_header::HttpHeader;
mod raw;
mod singbox;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Format(String),
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub remarks: String,
    pub description: Option<String>,
    pub outbounds: Vec<Outbound>,
    pub routing: Option<Value>,
    pub dns: Option<Value>,
    pub observatory: Option<Value>,
    pub conn_idle: Option<u64>,
}

impl Profile {
    pub fn outbound(&self, tag: &str) -> Option<&Outbound> {
        self.outbounds.iter().find(|o| o.tag == tag)
    }

    pub fn main_outbound(&self) -> Option<&Outbound> {
        self.outbound("proxy").or_else(|| self.outbounds.iter().find(|o| o.kind.is_proxy()))
    }

    pub fn balancer_members(&self) -> Vec<&Outbound> {
        let selectors: Vec<&str> = self
            .routing
            .as_ref()
            .and_then(|r| r.get("balancers"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|b| b.get("selector").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        self.outbounds.iter().filter(|o| o.kind.is_proxy() && selectors.iter().any(|s| o.tag.starts_with(s))).collect()
    }

    pub fn member(&self, tag: &str) -> Option<Profile> {
        let i = self.outbounds.iter().position(|o| o.tag == tag && o.kind.is_proxy())?;
        let mut p = self.clone();
        p.remarks = format!("{}{MEMBER_SEP}{tag}", self.remarks);
        p.description = None;
        let ob = p.outbounds.remove(i);
        p.outbounds.insert(0, ob);
        let renamed = format!("proxy{MEMBER_SEP}");
        if tag != "proxy"
            && let Some(o) = p.outbounds.iter_mut().find(|o| o.tag == "proxy")
        {
            o.tag = renamed.clone();
        }
        if let Some(r) = p.routing.as_mut().and_then(Value::as_object_mut) {
            r.remove("balancers");
            for rule in r.get_mut("rules").and_then(Value::as_array_mut).into_iter().flatten() {
                let Some(rule) = rule.as_object_mut() else { continue };
                if rule.remove("balancerTag").is_some() {
                    rule.insert("outboundTag".into(), Value::String(tag.to_string()));
                } else if tag != "proxy" && rule.get("outboundTag").and_then(Value::as_str) == Some("proxy") {
                    rule.insert("outboundTag".into(), Value::String(renamed.clone()));
                }
            }
        }
        Some(p)
    }
}

pub const MEMBER_SEP: char = '\u{1f}';

pub fn find_profile(all: &[Profile], name: &str) -> Option<Profile> {
    if let Some(p) = all.iter().find(|p| p.remarks == name) {
        return Some(p.clone());
    }
    let (r, tag) = name.split_once(MEMBER_SEP)?;
    all.iter().find(|p| p.remarks == r)?.member(tag)
}

#[derive(Debug, Clone)]
pub struct Outbound {
    pub tag: String,
    pub kind: OutboundKind,
    pub stream: StreamSettings,
    pub reject_udp443: bool,
}

#[derive(Debug, Clone)]
pub enum OutboundKind {
    Vless(Vless),
    Vmess(Vmess),
    Hysteria2(Hysteria2),
    Trojan(Trojan),
    Shadowsocks(Shadowsocks),
    Socks(Socks),
    Http(HttpProxy),
    Freedom(Freedom),
    Blackhole { http: bool },
    Dns { reject_non_ip: bool, block_types: Vec<u16> },
    Loopback { inbound_tag: String },
    Plugin { protocol: String, settings: Value },
    Unsupported { protocol: String, reason: String },
}

impl OutboundKind {
    pub fn is_proxy(&self) -> bool {
        match self {
            Self::Vless(_)
            | Self::Vmess(_)
            | Self::Hysteria2(_)
            | Self::Trojan(_)
            | Self::Shadowsocks(_)
            | Self::Socks(_)
            | Self::Http(_)
            | Self::Plugin { .. } => true,
            Self::Unsupported { protocol, .. } => !matches!(protocol.as_str(), "freedom" | "blackhole" | "dns" | "loopback"),
            _ => false,
        }
    }

    pub fn server(&self) -> Option<(&str, u16)> {
        match self {
            Self::Vless(v) => Some((&v.address, v.port)),
            Self::Vmess(v) => Some((&v.address, v.port)),
            Self::Hysteria2(h) => Some((&h.address, h.port)),
            Self::Trojan(t) => Some((&t.address, t.port)),
            Self::Shadowsocks(s) => Some((&s.address, s.port)),
            Self::Socks(s) => Some((&s.server.address, s.server.port)),
            Self::Http(h) => Some((&h.server.address, h.server.port)),
            Self::Plugin { settings, .. } => plugin_server(settings),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyServer {
    pub address: String,
    pub port: u16,
    pub auth: Option<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocksVersion {
    V4,
    V4a,
    V5,
}

#[derive(Debug, Clone)]
pub struct Socks {
    pub server: ProxyServer,
    pub version: SocksVersion,
}

#[derive(Debug, Clone)]
pub struct HttpProxy {
    pub server: ProxyServer,
}

#[derive(Debug, Clone)]
pub struct Vless {
    pub address: String,
    pub port: u16,
    pub id: [u8; 16],
    pub flow: Flow,
    pub encryption: Option<VlessEncryption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlessEncryption {
    pub xor_mode: u8,
    pub zero_rtt: bool,
    pub padding_lens: Vec<[u32; 3]>,
    pub padding_gaps: Vec<[u32; 3]>,
    pub keys: Vec<Vec<u8>>,
    pub raw: String,
}

impl VlessEncryption {
    pub fn parse(s: &str) -> Result<Self, String> {
        use base64::Engine;
        let bad = || format!("vless: unsupported encryption `{}`", s.chars().take(40).collect::<String>());
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() < 4 || parts[0] != "mlkem768x25519plus" {
            return Err(bad());
        }
        let xor_mode = match parts[1] {
            "native" => 0,
            "xorpub" => 1,
            "random" => 2,
            _ => return Err(bad()),
        };
        let zero_rtt = match parts[2] {
            "1rtt" => false,
            "0rtt" => true,
            _ => return Err(bad()),
        };
        let (mut padding_lens, mut padding_gaps, mut keys) = (Vec::new(), Vec::new(), Vec::new());
        let mut paddings = 0;
        for r in &parts[3..] {
            if r.len() < 20 {
                let x: Vec<u32> = r.split('-').map(|n| n.parse::<u32>()).collect::<Result<_, _>>().map_err(|_| bad())?;
                let y: [u32; 3] = x.get(..3).and_then(|y| y.try_into().ok()).ok_or_else(bad)?;
                if paddings == 0 && (y[0] < 100 || y[1] < 18 + 17 || y[2] < 18 + 17) {
                    return Err("vless: first encryption padding length must not be smaller than 35".into());
                }
                if paddings % 2 == 0 {
                    padding_lens.push(y)
                } else {
                    padding_gaps.push(y)
                }
                paddings += 1;
                continue;
            }
            let k = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(r).map_err(|_| bad())?;
            if k.len() != 32 && k.len() != 1184 {
                return Err(bad());
            }
            keys.push(k);
        }
        if keys.is_empty() {
            return Err(bad());
        }
        if padding_lens.iter().map(|y| u64::from(y[1].max(y[2]))).sum::<u64>() > 18 + 65535 {
            return Err("vless: total encryption padding length must not be larger than 65553".into());
        }
        Ok(Self { xor_mode, zero_rtt, padding_lens, padding_gaps, keys, raw: s.to_string() })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    None,
    Vision,
    VisionUdp443,
}

#[derive(Debug, Clone)]
pub struct Hysteria2 {
    pub address: String,
    pub port: u16,
    pub auth: String,
}

#[derive(Debug, Clone)]
pub struct Trojan {
    pub address: String,
    pub port: u16,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct Vmess {
    pub address: String,
    pub port: u16,
    pub id: [u8; 16],
    pub security: String,
}

#[derive(Debug, Clone)]
pub struct Shadowsocks {
    pub address: String,
    pub port: u16,
    pub method: String,
    pub password: String,
}

#[derive(Debug, Clone, Default)]
pub struct Freedom {
    pub fragment: Option<Fragment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub packets: (u64, u64),
    pub length: Range,
    pub interval_ms: Range,
    pub max_split: Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub from: u64,
    pub to: u64,
}

impl Range {
    pub fn parse(v: &Value) -> Option<Self> {
        match v {
            Value::Number(n) => n.as_u64().map(|x| Range { from: x, to: x }),
            Value::String(s) => {
                let s = s.trim();
                match s.split_once('-') {
                    Some((a, b)) => {
                        let (from, to) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
                        (from <= to).then_some(Range { from, to })
                    }
                    None => s.parse().ok().map(|x| Range { from: x, to: x }),
                }
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamSettings {
    pub network: Network,
    pub http_header: Option<Box<HttpHeader>>,
    pub security: Security,
    pub sockopt: SockOpt,
    pub finalmask: FinalMask,
    pub plugin_settings: Value,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            network: Network::Tcp,
            http_header: None,
            security: Security::None,
            sockopt: SockOpt::default(),
            finalmask: FinalMask::default(),
            plugin_settings: Value::Null,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Network {
    Tcp,
    TcpHttpHeader,
    Xhttp(Value),
    Grpc(Grpc),
    Ws(Ws),
    HttpUpgrade(HttpUpgrade),
    Hysteria(HysteriaStream),
    Other(String),
}

impl Network {
    pub fn name(&self) -> &str {
        match self {
            Self::Tcp | Self::TcpHttpHeader => "tcp",
            Self::Xhttp(_) => "xhttp",
            Self::Grpc(_) => "grpc",
            Self::Ws(_) => "ws",
            Self::HttpUpgrade(_) => "httpupgrade",
            Self::Hysteria(_) => "hysteria",
            Self::Other(n) => n,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Grpc {
    pub initial_window_size: u32,
    pub service_name: String,
    pub authority: String,
    pub multi_mode: bool,
    pub user_agent: String,
    pub idle_timeout: u32,
    pub health_check_timeout: u32,
    pub permit_without_stream: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Ws {
    pub path: String,
    pub host: String,
    pub headers: Vec<(String, String)>,
    pub early_data: usize,
    pub heartbeat_period: u32,
}

#[derive(Debug, Clone, Default)]
pub struct HttpUpgrade {
    pub path: String,
    pub host: String,
    pub headers: Vec<(String, String)>,
    pub early_data: usize,
}

#[derive(Debug, Clone, Default)]
pub struct HysteriaStream {
    pub auth: Option<String>,
    pub udp_idle_timeout: u32,
}

#[derive(Debug, Clone, Default)]
pub struct FinalMask {
    pub udp: Vec<UdpMask>,
    pub tcp: Vec<TcpMask>,
    pub quic: Option<QuicParams>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpMask {
    Fragment(FragmentMask),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentMask {
    pub packets: (u64, u64),
    pub lengths: Vec<(u64, u64)>,
    pub delays: Vec<(u64, u64)>,
    pub max_split: (u64, u64),
}

impl FragmentMask {
    pub fn parse(s: &Value) -> Result<Self, String> {
        let range = |v: Option<&Value>| -> Result<(u64, u64), String> {
            let r = match v {
                None | Some(Value::Null) => return Ok((0, 0)),
                Some(Value::Number(n)) => {
                    let n = n.as_u64().ok_or("fragment: bad number")?;
                    (n, n)
                }
                Some(Value::String(t)) if t.is_empty() => (0, 0),
                Some(Value::String(t)) => match t.split_once('-') {
                    Some((a, b)) => (
                        a.trim().parse().map_err(|_| format!("fragment: bad range {t}"))?,
                        b.trim().parse().map_err(|_| format!("fragment: bad range {t}"))?,
                    ),
                    None => {
                        let n = t.trim().parse().map_err(|_| format!("fragment: bad range {t}"))?;
                        (n, n)
                    }
                },
                Some(v) => return Err(format!("fragment: bad range {v}")),
            };
            Ok((r.0.min(r.1), r.0.max(r.1)))
        };
        let list = |key: &str, one: &str| -> Result<Vec<(u64, u64)>, String> {
            match s.get(key).and_then(Value::as_array).filter(|a| !a.is_empty()) {
                Some(a) => a.iter().map(|v| range(Some(v))).collect(),
                None => Ok(vec![range(s.get(one))?]),
            }
        };
        let packets = match s.get("packets").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase().as_str() {
            "tlshello" => (0, 1),
            "" => (0, 0),
            other => {
                let r = range(Some(&Value::String(other.to_string())))?;
                if r.0 == 0 {
                    return Err("fragment: packets can't start from 0".into());
                }
                r
            }
        };
        let lengths = list("lengths", "length")?;
        if lengths.last().is_some_and(|l| l.0 == 0) {
            return Err("fragment: last length min can't be 0".into());
        }
        Ok(Self { packets, lengths, delays: list("delays", "delay")?, max_split: range(s.get("maxSplit"))? })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UdpMask {
    Salamander { password: String, packet_size: Option<(u32, u32)> },
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuicParams {
    pub congestion: String,
    pub bbr_profile: String,
    pub brutal_up: u64,
    pub brutal_down: u64,
    pub udp_hop_ports: Vec<u16>,
    pub udp_hop_interval: (u32, u32),
    pub init_stream_window: u64,
    pub max_stream_window: u64,
    pub init_conn_window: u64,
    pub max_conn_window: u64,
    pub max_idle_timeout: u64,
    pub keep_alive_period: u64,
    pub max_incoming_streams: i64,
    pub disable_path_mtu_discovery: bool,
}

impl Default for QuicParams {
    fn default() -> Self {
        Self {
            congestion: String::new(),
            bbr_profile: "standard".into(),
            brutal_up: 0,
            brutal_down: 0,
            udp_hop_ports: Vec::new(),
            udp_hop_interval: (30, 30),
            init_stream_window: 8 << 20,
            max_stream_window: 8 << 20,
            init_conn_window: (8 << 20) * 5 / 2,
            max_conn_window: (8 << 20) * 5 / 2,
            max_idle_timeout: 30,
            keep_alive_period: 0,
            max_incoming_streams: -1,
            disable_path_mtu_discovery: false,
        }
    }
}

impl QuicParams {
    pub fn xhttp() -> Self {
        Self {
            init_stream_window: 2 << 20,
            max_stream_window: 6 << 20,
            init_conn_window: 3 << 20,
            max_conn_window: 15 << 20,
            max_idle_timeout: 300,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
pub enum Security {
    None,
    Tls(Tls),
    Reality(Reality),
}

#[derive(Debug, Clone)]
pub struct Tls {
    pub server_name: String,
    pub alpn: Vec<String>,
    pub fingerprint: String,
    pub allow_insecure: bool,
    pub pinned_sha256: Vec<[u8; 32]>,
    pub verify_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Reality {
    pub spider_x: String,
    pub server_name: String,
    pub public_key: [u8; 32],
    pub mldsa65_verify: Vec<u8>,
    pub short_id: [u8; 8],
    pub fingerprint: String,
}

#[derive(Debug, Clone, Default)]
pub struct SockOpt {
    pub tcp_keepalive: Option<(u32, u32)>,
    pub tcp_max_seg: Option<u32>,
    pub tcp_user_timeout: Option<u32>,
    pub tcp_window_clamp: Option<u32>,
    pub tcp_congestion: Option<String>,
    pub family: Family,
    pub dialer_proxy: Option<String>,
    pub dialer_proxy_ip: bool,
    pub happy_eyeballs: Option<HappyEyeballs>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Family {
    #[default]
    Any,
    V4 {
        force: bool,
    },
    V6 {
        force: bool,
    },
    V4First,
    V6First,
}

impl Family {
    pub fn parse(s: &str) -> Self {
        let l = s.to_ascii_lowercase();
        let force = l.starts_with("force");
        match l.trim_start_matches("force").trim_start_matches("use") {
            "ipv4" => Self::V4 { force },
            "ipv6" => Self::V6 { force },
            "ipv4v6" => Self::V4First,
            "ipv6v4" => Self::V6First,
            _ => Self::Any,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HappyEyeballs {
    pub try_delay_ms: u64,
    pub prioritize_ipv6: bool,
    pub interleave: u32,
    pub max_concurrent_try: u32,
}

impl Default for HappyEyeballs {
    fn default() -> Self {
        Self { try_delay_ms: 250, prioritize_ipv6: false, interleave: 1, max_concurrent_try: 4 }
    }
}

pub fn parse_subscription(text: &str) -> Result<Vec<Profile>, Error> {
    Ok(unique_remarks(subscription_values(text)?.into_iter().map(parse_profile_value).collect()))
}

pub fn parse_subscription_dial(text: &str) -> Result<Vec<Profile>, Error> {
    let mut values = subscription_values(text)?;
    for v in &mut values {
        for (k, r) in v.as_object_mut().into_iter().flatten() {
            if k.eq_ignore_ascii_case("routing")
                && let Some(r) = r.as_object_mut()
            {
                r.retain(|k, _| !k.eq_ignore_ascii_case("rules"));
            }
        }
    }
    Ok(unique_remarks(values.into_iter().map(parse_profile_value).collect()))
}

pub fn subscription_values(text: &str) -> Result<Vec<Value>, Error> {
    let mut t = text.trim_start_matches('\u{feff}').trim();
    let decoded;
    if !t.starts_with(['[', '{'])
        && !t.contains("://")
        && let Some(d) = links::b64(&t.split_whitespace().collect::<String>()).and_then(|b| String::from_utf8(b).ok())
        && d.trim_start().starts_with(['[', '{'])
    {
        decoded = d;
        t = decoded.trim();
    }
    if t.starts_with('[') || t.starts_with('{') {
        let v = serde_json::from_str::<Value>(t)?;
        if is_sing_box(&v) {
            let profiles = singbox::parse(&v);
            if profiles.is_empty() {
                return Err(Error::Format("no servers in sing-box config".into()));
            }
            return Ok(profiles);
        }
        return match v {
            Value::Array(items) => Ok(items
                .into_iter()
                .flat_map(|item| match item {
                    Value::String(link) => links::parse(&link),
                    other => vec![other],
                })
                .collect()),
            other => Ok(vec![other]),
        };
    }
    if t.lines().any(|l| l.starts_with("proxies:") || l.starts_with("proxy-groups:")) {
        return Err(Error::Format("clash config is not supported".into()));
    }
    let profiles = links::parse(t);
    if profiles.is_empty() {
        return Err(Error::Format("no xray config or share links".into()));
    }
    Ok(profiles)
}

fn unique_remarks(mut profiles: Vec<Profile>) -> Vec<Profile> {
    let mut seen = std::collections::HashSet::new();
    let mut next = std::collections::HashMap::new();
    for p in &mut profiles {
        if !seen.insert(p.remarks.clone()) {
            let n = next.entry(p.remarks.trim_start().to_string()).or_insert(2usize);
            let name = loop {
                let name = format!("{} ({n})", p.remarks).trim().to_string();
                *n += 1;
                if !seen.contains(&name) {
                    break name;
                }
            };
            seen.insert(name.clone());
            p.remarks = name;
        }
    }
    profiles
}

fn is_sing_box(v: &Value) -> bool {
    let first = match v {
        Value::Array(a) => a.first(),
        o => Some(o),
    };
    first
        .and_then(|c| c.get("outbounds"))
        .and_then(Value::as_array)
        .is_some_and(|obs| !obs.is_empty() && obs.iter().all(|o| o.get("type").is_some() && o.get("protocol").is_none()))
}

fn parse_profile_value(mut v: Value) -> Profile {
    raw::normalize(&mut v);
    let remarks = v.get("remarks").and_then(Value::as_str).unwrap_or_default().to_string();
    let raw: raw::Config = match serde_json::from_value(v) {
        Ok(r) => r,
        Err(e) => {
            return Profile {
                remarks,
                description: None,
                outbounds: vec![Outbound {
                    tag: "proxy".into(),
                    kind: OutboundKind::Unsupported { protocol: String::new(), reason: e.to_string() },
                    stream: StreamSettings::default(),
                    reject_udp443: false,
                }],
                routing: None,
                dns: None,
                observatory: None,
                conn_idle: None,
            };
        }
    };
    let description = raw
        .meta
        .as_ref()
        .and_then(|m| m.get("serverDescription"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    Profile {
        remarks,
        description,
        outbounds: raw.outbounds.into_iter().map(convert_outbound).collect(),
        routing: raw.routing,
        dns: raw.dns,
        observatory: raw.burst_observatory.or(raw.observatory),
        conn_idle: raw.policy.as_ref().and_then(|p| p.get("levels")?.get("0")?.get("connIdle")?.as_u64()).filter(|&s| s > 0),
    }
}

pub fn plugin_protocol(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.'))
        && !matches!(
            name,
            "vless"
                | "vmess"
                | "trojan"
                | "shadowsocks"
                | "ss"
                | "socks"
                | "hysteria"
                | "hysteria2"
                | "hy2"
                | "freedom"
                | "blackhole"
                | "dns"
                | "loopback"
                | "direct"
                | "block"
        )
}

fn plugin_server(s: &Value) -> Option<(&str, u16)> {
    let port = |v: Option<&Value>| match v? {
        Value::Number(n) => n.as_u64().and_then(|p| u16::try_from(p).ok()),
        Value::String(t) => t.trim().parse().ok(),
        _ => None,
    };
    for (a, p) in
        [("address", "port"), ("server", "server_port"), ("/peers/0/address", "/peers/0/port"), ("/peers/0/server", "/peers/0/server_port")]
    {
        let get = |k: &str| if k.starts_with('/') { s.pointer(k) } else { s.get(k) };
        if let Some(host) = get(a).and_then(Value::as_str).map(str::trim).filter(|h| !h.is_empty()) {
            return Some((host, port(get(p)).filter(|&p| p > 0)?));
        }
    }
    let ep = s.get("endpoint").or_else(|| s.pointer("/peers/0/endpoint")).and_then(Value::as_str)?.trim();
    let (host, p) = ep.rsplit_once(':')?;
    let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    Some((host, p.parse().ok().filter(|&p| p > 0)?))
}

fn convert_outbound(v: Value) -> Outbound {
    let tag = v.get("tag").and_then(Value::as_str).unwrap_or_default().to_string();
    let protocol = v.get("protocol").and_then(Value::as_str).unwrap_or_default().to_string();
    match try_outbound(v) {
        Ok(o) => o,
        Err(reason) => {
            Outbound { tag, kind: OutboundKind::Unsupported { protocol, reason }, stream: StreamSettings::default(), reject_udp443: false }
        }
    }
}

fn try_outbound(v: Value) -> Result<Outbound, String> {
    let mut o: raw::Outbound = serde_json::from_value(v).map_err(|e| e.to_string())?;
    o.protocol.make_ascii_lowercase();
    let tag = o.tag.unwrap_or_default();
    let mut raw_stream = o.stream_settings.unwrap_or_default();
    let hysteria_stream = raw_stream.hysteria_settings.take();
    let mut stream = convert_stream(raw_stream, hysteria_stream.as_ref())?;
    if let Some(ps) = o.proxy_settings.filter(|p| !p.tag.is_empty()) {
        if !ps.transport_layer {
            stream = StreamSettings::default();
        }
        stream.sockopt.dialer_proxy.get_or_insert(ps.tag);
    }
    let settings = flat_settings(&o.protocol, o.settings.unwrap_or(Value::Null));
    let unsupported = |reason: &str| OutboundKind::Unsupported { protocol: o.protocol.clone(), reason: reason.into() };
    let kind = match o.protocol.as_str() {
        "vless" => OutboundKind::Vless(convert_vless(settings)?),
        "vmess" => OutboundKind::Vmess(convert_vmess(settings)?),
        "hysteria" | "hysteria2" | "hy2" => {
            let s: raw::HysteriaSettings = serde_json::from_value(settings).map_err(|e| e.to_string())?;
            let v2 = o.protocol != "hysteria" || s.version.unwrap_or(2) == 2;
            if !v2 {
                unsupported("hysteria v1")
            } else {
                let auth = hysteria_stream.and_then(|h| h.auth).or(s.auth).or(s.password).ok_or("hysteria: no auth")?;
                OutboundKind::Hysteria2(Hysteria2 { address: s.address, port: s.port, auth })
            }
        }
        "trojan" => {
            let s: raw::ServerList<raw::TrojanServer> = serde_json::from_value(settings).map_err(|e| e.to_string())?;
            let t = s.servers.into_iter().next().ok_or("trojan: no servers")?;
            OutboundKind::Trojan(Trojan { address: t.address, port: t.port, password: t.password })
        }
        "shadowsocks" => {
            let s: raw::ServerList<raw::ShadowsocksServer> = serde_json::from_value(settings).map_err(|e| e.to_string())?;
            let t = s.servers.into_iter().next().ok_or("shadowsocks: no servers")?;
            OutboundKind::Shadowsocks(Shadowsocks {
                address: t.address,
                port: t.port,
                method: t.method.to_ascii_lowercase(),
                password: t.password,
            })
        }
        "socks" | "socks5" | "socks4" | "socks4a" => {
            let version = settings.get("version").map(|v| v.as_str().ok_or("socks: version must be a string")).transpose()?.unwrap_or(
                match o.protocol.as_str() {
                    "socks4" => "4",
                    "socks4a" => "4a",
                    _ => "5",
                },
            );
            let version = match version {
                "4" => SocksVersion::V4,
                "4a" => SocksVersion::V4a,
                "5" => SocksVersion::V5,
                _ => return Err("socks: unsupported version".into()),
            };
            let server = convert_proxy_server(&settings)?;
            if let Some((u, p)) = &server.auth {
                if version == SocksVersion::V5 && (u.is_empty() || p.is_empty() || u.len() > 255 || p.len() > 255) {
                    return Err("socks5: username/password must be 1..255 bytes".into());
                }
                if version != SocksVersion::V5 && (u.contains('\0') || !p.is_empty()) {
                    return Err("socks4: only a NUL-free user ID is supported".into());
                }
            }
            OutboundKind::Socks(Socks { server, version })
        }
        "http" | "https" => {
            if settings.get("headers").is_some_and(|h| h.as_object().is_none_or(|o| !o.is_empty())) {
                return Err("http proxy: custom headers are not supported".into());
            }
            if o.protocol == "https" && matches!(stream.security, Security::None) {
                return Err("https proxy: TLS settings are required".into());
            }
            let server = convert_proxy_server(&settings)?;
            if server.auth.as_ref().is_some_and(|(user, _)| user.contains(':')) {
                return Err("http proxy: username cannot contain a colon".into());
            }
            OutboundKind::Http(HttpProxy { server })
        }
        "freedom" => {
            let s: raw::FreedomSettings = match settings {
                Value::Null => raw::FreedomSettings::default(),
                v => serde_json::from_value(v).map_err(|e| format!("freedom: {e}"))?,
            };
            let fragment = match s.fragment {
                None => None,
                Some(f) => Some(Fragment {
                    packets: match f.packets.as_deref().map(str::trim).unwrap_or("").to_ascii_lowercase().as_str() {
                        "tlshello" => (0, 1),
                        "" => (0, 0),
                        p => {
                            let r = Range::parse(&Value::String(p.into())).ok_or("fragment.packets")?;
                            if r.from == 0 {
                                return Err("fragment.packets can't start at 0".into());
                            }
                            (r.from, r.to)
                        }
                    },
                    length: Range::parse(&f.length).ok_or("fragment.length")?,
                    interval_ms: Range::parse(&f.interval).ok_or("fragment.interval")?,
                    max_split: match &f.max_split {
                        Some(v) => Range::parse(v).ok_or("fragment.maxSplit")?,
                        None => Range { from: 0, to: 0 },
                    },
                }),
            };
            OutboundKind::Freedom(Freedom { fragment })
        }
        "blackhole" => OutboundKind::Blackhole {
            http: settings.pointer("/response/type").and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case("http")),
        },
        "dns" => {
            let q = settings.get("nonIPQuery").and_then(Value::as_str).unwrap_or("");
            OutboundKind::Dns {
                reject_non_ip: q.eq_ignore_ascii_case("reject") || q.eq_ignore_ascii_case("drop"),
                block_types: settings
                    .get("blockTypes")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|t| t.as_u64()?.try_into().ok()).collect())
                    .unwrap_or_default(),
            }
        }
        "loopback" => {
            let s: raw::LoopbackSettings = serde_json::from_value(settings).map_err(|e| e.to_string())?;
            OutboundKind::Loopback { inbound_tag: s.inbound_tag }
        }
        other if plugin_protocol(other) => OutboundKind::Plugin { protocol: other.to_string(), settings },
        other => unsupported(&format!("protocol {other}")),
    };
    let reject_udp443 =
        o.mux.is_some_and(|m| m.enabled && (m.xudp_proxy_udp443.is_empty() || m.xudp_proxy_udp443.eq_ignore_ascii_case("reject")));
    Ok(Outbound { tag, kind, stream, reject_udp443 })
}

fn convert_proxy_server(settings: &Value) -> Result<ProxyServer, String> {
    let server = if let Some(servers) = settings.get("servers") {
        let servers = servers.as_array().ok_or("proxy: servers must be an array")?;
        if servers.len() != 1 {
            return Err("proxy: exactly one server is required".into());
        }
        &servers[0]
    } else {
        settings
    };
    let address = server
        .get("address")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .ok_or("proxy: invalid address")?
        .to_string();
    let port =
        server.get("port").and_then(Value::as_u64).and_then(|n| u16::try_from(n).ok()).filter(|p| *p != 0).ok_or("proxy: invalid port")?;
    let account = if let Some(users) = server.get("users") {
        let users = users.as_array().ok_or("proxy: invalid users")?;
        if users.len() > 1 {
            return Err("proxy: at most one account is supported".into());
        }
        users.first()
    } else if server.get("user").is_some_and(|u| u.as_str().is_none_or(|u| !u.is_empty())) {
        Some(server)
    } else {
        None
    };
    let auth = account
        .map(|a| {
            let user = a.get("user").and_then(Value::as_str).ok_or("proxy: invalid username")?;
            let pass = a.get("pass").map(|p| p.as_str().ok_or("proxy: invalid password")).transpose()?.unwrap_or("");
            if user.len() + pass.len() > 8192 {
                return Err("proxy: credentials too long");
            }
            Ok((user.to_string(), pass.to_string()))
        })
        .transpose()?;
    Ok(ProxyServer { address, port, auth })
}

fn flat_settings(protocol: &str, settings: Value) -> Value {
    if settings.get("address").is_none() {
        return settings;
    }
    match protocol {
        "vless" | "vmess" => {
            let mut user = serde_json::Map::new();
            for k in ["id", "flow", "encryption", "security"] {
                if let Some(v) = settings.get(k) {
                    user.insert(k.into(), v.clone());
                }
            }
            serde_json::json!({ "vnext": [{ "address": settings["address"], "port": settings.get("port").cloned().unwrap_or(serde_json::json!(0)), "users": [user] }] })
        }
        "trojan" | "shadowsocks" | "socks" => serde_json::json!({ "servers": [settings] }),
        _ => settings,
    }
}

fn convert_vmess(settings: Value) -> Result<Vmess, String> {
    let s: raw::VmessSettings = serde_json::from_value(settings).map_err(|e| e.to_string())?;
    let server = s.vnext.into_iter().next().ok_or("vmess: empty vnext")?;
    let user = server.users.into_iter().next().ok_or("vmess: no users")?;
    if server.address.trim().is_empty() || server.port == 0 || user.id.trim().is_empty() {
        return Err("vmess: empty server, port or id".into());
    }
    let security = user.security.unwrap_or_default().trim().to_ascii_lowercase();
    let security = match security.as_str() {
        "aes-128-gcm" | "chacha20-poly1305" => security,
        _ => "auto".to_string(),
    };
    Ok(Vmess { address: server.address, port: server.port, id: parse_uuid(&user.id)?, security })
}

fn convert_vless(settings: Value) -> Result<Vless, String> {
    let s: raw::VlessSettings = serde_json::from_value(settings).map_err(|e| e.to_string())?;
    let server = s.vnext.into_iter().next().ok_or("vless: empty vnext")?;
    let user = server.users.into_iter().next().ok_or("vless: no users")?;
    let encryption = match user.encryption.as_deref().unwrap_or("none") {
        "none" | "" => None,
        e => Some(VlessEncryption::parse(e)?),
    };
    let id = parse_uuid(&user.id)?;
    let flow = match user.flow.as_deref().unwrap_or("") {
        "" => Flow::None,
        "xtls-rprx-vision" => Flow::Vision,
        "xtls-rprx-vision-udp443" => Flow::VisionUdp443,
        other => return Err(format!("vless: unknown flow `{other}`")),
    };
    Ok(Vless { address: server.address, port: server.port, id, flow, encryption })
}

fn parse_uuid(s: &str) -> Result<[u8; 16], String> {
    if s.trim().is_empty() {
        return Ok([0; 16]);
    }
    if let Ok(u) = uuid::Uuid::parse_str(s) {
        return Ok(*u.as_bytes());
    }
    if !s.is_empty() && s.len() <= 30 {
        return Ok(*uuid::Uuid::new_v5(&uuid::Uuid::nil(), s.as_bytes()).as_bytes());
    }
    Err(format!("bad uuid `{s}`"))
}

fn headers(m: Option<serde_json::Map<String, Value>>) -> Vec<(String, String)> {
    m.unwrap_or_default()
        .into_iter()
        .map(|(k, v)| {
            let v = match v {
                Value::String(s) => s,
                other => other.to_string(),
            };
            (k, v)
        })
        .collect()
}

fn split_early_data(path: &str) -> (String, usize) {
    let Some((base, query)) = path.split_once('?') else {
        return (path.to_string(), 0);
    };
    let mut ed = 0;
    let mut rest: Vec<(&str, &str)> = Vec::new();
    for kv in query.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        if k == "ed" {
            ed = v.parse().unwrap_or(0);
        } else {
            rest.push((k, v));
        }
    }
    if ed == 0 && rest.len() == query.split('&').filter(|s| !s.is_empty()).count() {
        return (path.to_string(), 0);
    }
    rest.sort_by(|a, b| a.0.cmp(b.0));
    let q: Vec<String> = rest.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let path = if q.is_empty() { base.to_string() } else { format!("{base}?{}", q.join("&")) };
    (path, ed)
}

pub fn parse_bandwidth(v: &Value) -> Result<u64, String> {
    let s = match v {
        Value::Null => return Ok(0),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.trim().to_ascii_lowercase(),
        other => return Err(format!("bad bandwidth {other}")),
    };
    if s.is_empty() {
        return Ok(0);
    }
    let idx = s.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(s.len());
    let num: f64 = s[..idx].parse().map_err(|_| format!("bad bandwidth `{s}`"))?;
    let mul: u64 = match s[idx..].trim() {
        "" | "b" | "bps" => 1,
        "k" | "kb" | "kbps" => 1 << 10,
        "m" | "mb" | "mbps" => 1 << 20,
        "g" | "gb" | "gbps" => 1 << 30,
        "t" | "tb" | "tbps" => 1 << 40,
        u => return Err(format!("unsupported unit `{u}`")),
    };
    Ok((num * mul as f64) as u64 / 8)
}

fn parse_ports(v: &Value) -> Result<Vec<u16>, String> {
    let mut out = Vec::new();
    let mut seen = vec![false; 1 << 16];
    let mut parts = 0;
    let mut add = |s: &str| -> Result<(), String> {
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            parts += 1;
            if parts > 1024 {
                return Err("udpHop: too many port ranges".into());
            }
            let mut push = |p: u16| {
                if !std::mem::replace(&mut seen[p as usize], true) {
                    out.push(p);
                }
            };
            match part.split_once('-') {
                Some((a, b)) => {
                    let a = a.trim().parse::<u16>().map_err(|_| "udpHop: invalid port range")?;
                    let b = b.trim().parse::<u16>().map_err(|_| "udpHop: invalid port range")?;
                    (a..=b).for_each(&mut push);
                }
                None => push(part.parse::<u16>().map_err(|_| "udpHop: invalid port")?),
            }
        }
        Ok(())
    };
    match v {
        Value::Number(n) if n.as_u64() == Some(0) => {}
        Value::Number(n) => add(&n.to_string())?,
        Value::String(s) => add(s)?,
        Value::Array(a) => {
            for x in a {
                match x {
                    Value::Number(n) => add(&n.to_string())?,
                    Value::String(s) => add(s)?,
                    _ => return Err("udpHop: invalid port list".into()),
                }
            }
        }
        Value::Null => {}
        _ => return Err("udpHop: invalid port list".into()),
    }
    Ok(out)
}

fn hop_interval(v: Option<&Value>) -> Result<(u32, u32), String> {
    let bad = || "udpHop: invalid interval (need seconds >= 5, or 0 for 30)".to_string();
    let (a, b) = match v {
        None | Some(Value::Null) => (0, 0),
        Some(Value::Number(n)) => {
            let n = n.as_i64().and_then(|n| i32::try_from(n).ok()).ok_or_else(bad)?;
            (n, n)
        }
        Some(Value::String(s)) if s.is_empty() => (0, 0),
        Some(Value::String(s)) => match s.parse::<i32>() {
            Ok(n) => (n, n),
            Err(_) => {
                let at = s.get(1..).and_then(|s| s.find('-')).map(|at| at + 1).ok_or_else(bad)?;
                (s[..at].parse::<i32>().map_err(|_| bad())?, s[at + 1..].parse::<i32>().map_err(|_| bad())?)
            }
        },
        _ => return Err(bad()),
    };
    let (a, b) = (a.min(b), a.max(b));
    let (a, b) = (if a == 0 { 30 } else { a }, if b == 0 { 30 } else { b });
    if a < 5 || b < a {
        return Err(bad());
    }
    Ok((a as u32, b as u32))
}

fn convert_finalmask(f: raw::FinalMask, xhttp: bool) -> Result<FinalMask, String> {
    let udp = f
        .udp
        .into_iter()
        .map(|m| match m.kind.as_str() {
            "salamander" => {
                let s = m.settings.unwrap_or(Value::Null);
                let packet_size = match s.get("packetSize").and_then(Range::parse) {
                    Some(r) if r.to > 0 => {
                        if r.from == 0 || r.from > r.to || r.to > 2048 {
                            return UdpMask::Other("salamander: invalid packetSize".into());
                        }
                        Some((r.from as u32, r.to as u32))
                    }
                    _ => None,
                };
                let password = s.get("password").and_then(Value::as_str).unwrap_or_default().to_string();
                UdpMask::Salamander { password, packet_size }
            }
            other => UdpMask::Other(other.to_string()),
        })
        .collect();
    let quic = match f.quic_params {
        None => None,
        Some(q) => {
            let d = if xhttp { QuicParams::xhttp() } else { QuicParams::default() };
            let (hop_ports, hop_interval) = match &q.udp_hop {
                Some(h) => (h.get("ports").map(parse_ports).transpose()?.unwrap_or_default(), hop_interval(h.get("interval"))?),
                None => (Vec::new(), d.udp_hop_interval),
            };
            let nz = |v: Option<u64>, d: u64| v.filter(|&x| x > 0).unwrap_or(d);
            Some(QuicParams {
                congestion: q.congestion.unwrap_or_default().to_ascii_lowercase(),
                bbr_profile: q.bbr_profile.filter(|p| !p.is_empty()).map(|p| p.to_ascii_lowercase()).unwrap_or(d.bbr_profile),
                brutal_up: parse_bandwidth(q.brutal_up.as_ref().unwrap_or(&Value::Null))?,
                brutal_down: parse_bandwidth(q.brutal_down.as_ref().unwrap_or(&Value::Null))?,
                udp_hop_ports: hop_ports,
                udp_hop_interval: hop_interval,
                init_stream_window: nz(q.init_stream_receive_window, d.init_stream_window),
                max_stream_window: nz(q.max_stream_receive_window, d.max_stream_window),
                init_conn_window: nz(q.init_connection_receive_window, d.init_conn_window),
                max_conn_window: nz(q.max_connection_receive_window, d.max_conn_window),
                max_idle_timeout: nz(q.max_idle_timeout, d.max_idle_timeout),
                keep_alive_period: q.keep_alive_period.unwrap_or(0),
                max_incoming_streams: q.max_incoming_streams.filter(|&n| n != 0).unwrap_or(d.max_incoming_streams),
                disable_path_mtu_discovery: q.disable_path_mtu_discovery.unwrap_or(false),
            })
        }
    };
    Ok(FinalMask {
        udp,
        tcp: f
            .tcp
            .into_iter()
            .map(|m| match m.kind.as_str() {
                "fragment" => FragmentMask::parse(m.settings.as_ref().unwrap_or(&Value::Null)).map(TcpMask::Fragment),
                other => Ok(TcpMask::Other(other.to_string())),
            })
            .collect::<Result<_, _>>()?,
        quic,
    })
}

fn convert_stream(mut s: raw::StreamSettings, hysteria: Option<&raw::HysteriaStream>) -> Result<StreamSettings, String> {
    let network_name = s.method.as_deref().or(s.network.as_deref()).unwrap_or("tcp").to_ascii_lowercase();
    let extra = std::mem::take(&mut s.extra);
    let mut plugin_settings = Value::Null;
    let mut http_header = None;
    let network = match network_name.as_str() {
        "tcp" | "raw" => {
            let header = s.raw_settings.as_ref().or(s.tcp_settings.as_ref()).and_then(|t| t.header.as_ref());
            if let Some(h) = header.filter(|h| h["type"].as_str() == Some("http")) {
                http_header = Some(Box::new(HttpHeader::from_json(h)?));
                Network::TcpHttpHeader
            } else {
                Network::Tcp
            }
        }
        "xhttp" | "splithttp" => Network::Xhttp(s.xhttp_settings.unwrap_or(Value::Null)),
        "grpc" | "gun" => {
            let g = s.grpc_settings.unwrap_or_default();
            Network::Grpc(Grpc {
                initial_window_size: g.initial_windows_size.unwrap_or(0).clamp(0, 0x7fff_ffff) as u32,
                service_name: g.service_name.unwrap_or_default(),
                authority: g.authority.unwrap_or_default(),
                multi_mode: g.multi_mode.unwrap_or(false),
                user_agent: g.user_agent.unwrap_or_default(),
                idle_timeout: g.idle_timeout.unwrap_or(0).clamp(0, u32::MAX as i64) as u32,
                health_check_timeout: g.health_check_timeout.filter(|&t| t > 0).unwrap_or(20).min(u32::MAX as i64) as u32,
                permit_without_stream: g.permit_without_stream.unwrap_or(false),
            })
        }
        "ws" | "websocket" => {
            let w = s.ws_settings.unwrap_or_default();
            let (path, early_data) = split_early_data(w.path.as_deref().unwrap_or(""));
            let mut host = w.host.unwrap_or_default();
            let headers: Vec<(String, String)> = headers(w.headers)
                .into_iter()
                .filter(|(k, v)| {
                    if k.eq_ignore_ascii_case("host") {
                        if host.is_empty() {
                            host = v.clone();
                        }
                        false
                    } else {
                        true
                    }
                })
                .collect();
            Network::Ws(Ws { path, host, headers, early_data, heartbeat_period: w.heartbeat_period.unwrap_or(0) })
        }
        "httpupgrade" => {
            let h = s.httpupgrade_settings.unwrap_or_default();
            let (path, early_data) = split_early_data(h.path.as_deref().unwrap_or(""));
            Network::HttpUpgrade(HttpUpgrade {
                path,
                host: h.host.unwrap_or_default(),
                headers: headers(h.headers).into_iter().filter(|(k, _)| !k.eq_ignore_ascii_case("host")).collect(),
                early_data,
            })
        }
        "hysteria" => Network::Hysteria(HysteriaStream {
            auth: hysteria.and_then(|h| h.auth.clone()),
            udp_idle_timeout: hysteria.and_then(|h| h.udp_idle_timeout).filter(|&t| t > 0).unwrap_or(60),
        }),
        other => {
            let key = format!("{other}settings");
            plugin_settings = extra.into_iter().find(|(k, _)| k.to_ascii_lowercase() == key).map(|(_, v)| v).unwrap_or(Value::Null);
            Network::Other(other.to_string())
        }
    };
    let security = match s.security.as_deref().unwrap_or("none").to_ascii_lowercase().as_str() {
        "none" | "" => Security::None,
        "tls" => {
            let t = s.tls_settings.unwrap_or_default();
            Security::Tls(Tls {
                server_name: t.server_name.unwrap_or_default(),
                alpn: t.alpn.unwrap_or_default(),
                fingerprint: t.fingerprint.unwrap_or_default(),
                allow_insecure: t.allow_insecure.unwrap_or(false),
                pinned_sha256: pinned_hashes(t.pinned_peer_cert_sha256.as_deref().unwrap_or_default())?,
                verify_names: t
                    .verify_peer_cert_by_name
                    .as_deref()
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(String::from)
                    .collect(),
            })
        }
        "reality" => {
            let r = s.reality_settings.ok_or("reality: no realitySettings")?;
            let mldsa65_verify = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(r.mldsa65_verify.as_deref().unwrap_or_default())
                .map_err(|_| "reality.mldsa65Verify: invalid base64url")?;
            if !mldsa65_verify.is_empty() && mldsa65_verify.len() != 1952 {
                return Err("reality.mldsa65Verify: need 1952 bytes".into());
            }
            let key = r.password.as_deref().filter(|p| !p.is_empty()).unwrap_or(&r.public_key);
            let pk = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(key.trim_end_matches('='))
                .map_err(|e| format!("reality.publicKey: {e}"))?;
            let public_key: [u8; 32] = pk.try_into().map_err(|_| "reality.publicKey: need 32 bytes")?;
            let sid = r.short_id.unwrap_or_default();
            if sid.len() > 16 || sid.len() % 2 != 0 {
                return Err(format!("reality.shortId `{sid}`"));
            }
            let mut short_id = [0u8; 8];
            hex::decode_to_slice(&sid, &mut short_id[..sid.len() / 2]).map_err(|e| format!("reality.shortId: {e}"))?;
            Security::Reality(Reality {
                server_name: r.server_name.unwrap_or_default(),
                public_key,
                mldsa65_verify,
                short_id,
                fingerprint: r.fingerprint.unwrap_or_else(|| "chrome".into()),
                spider_x: r.spider_x.filter(|s| s.starts_with('/') && !s.contains(['\r', '\n'])).unwrap_or_else(|| "/".into()),
            })
        }
        other => return Err(format!("security {other}")),
    };
    let so = s.sockopt.unwrap_or_default();
    let (idle, interval) = (so.tcp_keep_alive_idle.unwrap_or(0), so.tcp_keep_alive_interval.unwrap_or(0));
    if (idle < 0 && interval > 0) || (interval < 0 && idle > 0) {
        return Err("invalid TCP keepalive idle/interval combination".into());
    }
    let keepalive = (idle >= 0 && interval >= 0 && (idle > 0 || interval > 0)).then(|| {
        (
            (if idle > 0 { idle } else { 45 }).min(u32::MAX as i64) as u32,
            (if interval > 0 { interval } else { 45 }).min(u32::MAX as i64) as u32,
        )
    });
    let sockopt = SockOpt {
        tcp_keepalive: keepalive,
        tcp_max_seg: so.tcp_max_seg.filter(|&m| m > 0),
        tcp_user_timeout: so.tcp_user_timeout.filter(|&t| t > 0),
        tcp_window_clamp: so.tcp_window_clamp.filter(|&t| t > 0),
        tcp_congestion: so.tcp_congestion.filter(|c| !c.is_empty()),
        family: so.domain_strategy.as_deref().map(Family::parse).unwrap_or_default(),
        dialer_proxy: so.dialer_proxy.filter(|d| !d.is_empty()),
        dialer_proxy_ip: false,
        happy_eyeballs: so.happy_eyeballs.map(|h| {
            let d = HappyEyeballs::default();
            HappyEyeballs {
                try_delay_ms: h.try_delay_ms.unwrap_or(d.try_delay_ms),
                prioritize_ipv6: h.prioritize_ipv6.unwrap_or(d.prioritize_ipv6),
                interleave: h.interleave.unwrap_or(d.interleave).max(1),
                max_concurrent_try: h.max_concurrent_try.unwrap_or(d.max_concurrent_try).max(1),
            }
        }),
    };
    let finalmask = match s.finalmask {
        Some(f) => convert_finalmask(f, matches!(network, Network::Xhttp(_)))?,
        None => FinalMask::default(),
    };
    Ok(StreamSettings { network, http_header, security, sockopt, finalmask, plugin_settings })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionInfo {
    pub title: Option<String>,
    pub upload: Option<u64>,
    pub download: Option<u64>,
    pub total: Option<u64>,
    pub expire: Option<u64>,
    pub update_interval_hours: Option<u32>,
}

impl SubscriptionInfo {
    pub fn from_headers(headers: &HashMap<String, String>) -> Self {
        let mut info = SubscriptionInfo::default();
        if let Some(t) = headers.get("profile-title") {
            info.title = Some(decode_b64_prefixed(t));
        }
        if let Some(u) = headers.get("subscription-userinfo") {
            for kv in u.split(';') {
                if let Some((k, v)) = kv.split_once('=') {
                    let v = v.trim().parse().ok();
                    match k.trim() {
                        "upload" => info.upload = v,
                        "download" => info.download = v,
                        "total" => info.total = v,
                        "expire" => info.expire = v,
                        _ => {}
                    }
                }
            }
        }
        info.update_interval_hours = headers.get("profile-update-interval").and_then(|v| v.trim().parse().ok());
        info
    }
}

fn pinned_hashes(v: &str) -> Result<Vec<[u8; 32]>, String> {
    v.split(',')
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(|h| {
            let hex: String = h.chars().filter(|c| *c != ':').collect();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
                .collect::<Option<Vec<u8>>>()
                .filter(|b| hex.len().is_multiple_of(2) && b.len() == 32)
                .ok_or_else(|| format!("pinnedPeerCertSha256: bad hash `{h}`"))?;
            Ok(bytes.try_into().unwrap())
        })
        .collect()
}

pub fn decode_b64_prefixed(t: &str) -> String {
    match t.strip_prefix("base64:") {
        Some(b) => base64::engine::general_purpose::STANDARD
            .decode(b.trim())
            .ok()
            .and_then(|v| String::from_utf8(v).ok())
            .unwrap_or_else(|| t.to_string()),
        None => t.to_string(),
    }
}

#[cfg(kani)]
#[path = "proofs/lib.rs"]
mod proofs;

#[cfg(test)]
mod tests;
