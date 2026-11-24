use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub outbounds: Vec<Value>,
    pub routing: Option<Value>,
    pub dns: Option<Value>,
    pub burst_observatory: Option<Value>,
    pub observatory: Option<Value>,
    pub policy: Option<Value>,
    pub meta: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outbound {
    pub tag: Option<String>,
    pub protocol: String,
    pub settings: Option<Value>,
    pub stream_settings: Option<StreamSettings>,
    pub mux: Option<Mux>,
    pub proxy_settings: Option<ProxySettings>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub transport_layer: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Mux {
    #[serde(default, deserialize_with = "null_as_default")]
    pub enabled: bool,
    #[serde(rename = "xudpProxyUDP443", default, deserialize_with = "null_as_default")]
    pub xudp_proxy_udp443: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StreamSettings {
    pub method: Option<String>,
    pub network: Option<String>,
    pub security: Option<String>,
    pub tls_settings: Option<TlsSettings>,
    pub reality_settings: Option<RealitySettings>,
    pub tcp_settings: Option<TcpSettings>,
    pub raw_settings: Option<TcpSettings>,
    #[serde(alias = "splithttpSettings")]
    pub xhttp_settings: Option<Value>,
    pub grpc_settings: Option<GrpcSettings>,
    pub ws_settings: Option<WsSettings>,
    pub httpupgrade_settings: Option<HttpUpgradeSettings>,
    pub hysteria_settings: Option<HysteriaStream>,
    pub finalmask: Option<FinalMask>,
    pub sockopt: Option<SockOpt>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TlsSettings {
    pub server_name: Option<String>,
    #[serde(default, deserialize_with = "string_list")]
    pub alpn: Option<Vec<String>>,
    pub fingerprint: Option<String>,
    pub allow_insecure: Option<bool>,
    pub pinned_peer_cert_sha256: Option<String>,
    pub verify_peer_cert_by_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealitySettings {
    pub spider_x: Option<String>,
    pub server_name: Option<String>,
    #[serde(default)]
    pub public_key: String,
    pub password: Option<String>,
    pub mldsa65_verify: Option<String>,
    pub short_id: Option<String>,
    pub fingerprint: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct TcpSettings {
    pub header: Option<Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GrpcSettings {
    #[serde(rename = "initial_windows_size", alias = "initialWindowsSize")]
    pub initial_windows_size: Option<i64>,
    pub service_name: Option<String>,
    pub authority: Option<String>,
    pub multi_mode: Option<bool>,
    #[serde(alias = "user_agent")]
    pub user_agent: Option<String>,
    #[serde(rename = "idle_timeout")]
    pub idle_timeout: Option<i64>,
    #[serde(rename = "health_check_timeout")]
    pub health_check_timeout: Option<i64>,
    #[serde(rename = "permit_without_stream")]
    pub permit_without_stream: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WsSettings {
    pub path: Option<String>,
    pub host: Option<String>,
    pub headers: Option<serde_json::Map<String, Value>>,
    pub heartbeat_period: Option<u32>,
}

#[derive(Deserialize, Default)]
pub struct HttpUpgradeSettings {
    pub path: Option<String>,
    pub host: Option<String>,
    pub headers: Option<serde_json::Map<String, Value>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HysteriaStream {
    pub auth: Option<String>,
    pub udp_idle_timeout: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FinalMask {
    #[serde(default, deserialize_with = "null_as_default")]
    pub tcp: Vec<Mask>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub udp: Vec<Mask>,
    pub quic_params: Option<QuicParams>,
}

#[derive(Deserialize)]
pub struct Mask {
    #[serde(rename = "type")]
    pub kind: String,
    pub settings: Option<Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuicParams {
    pub congestion: Option<String>,
    pub bbr_profile: Option<String>,
    pub brutal_up: Option<Value>,
    pub brutal_down: Option<Value>,
    pub udp_hop: Option<Value>,
    pub init_stream_receive_window: Option<u64>,
    pub max_stream_receive_window: Option<u64>,
    pub init_connection_receive_window: Option<u64>,
    pub max_connection_receive_window: Option<u64>,
    pub max_idle_timeout: Option<u64>,
    pub keep_alive_period: Option<u64>,
    pub max_incoming_streams: Option<i64>,
    #[serde(rename = "disablePathMTUDiscovery")]
    pub disable_path_mtu_discovery: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SockOpt {
    pub tcp_keep_alive_idle: Option<i64>,
    pub tcp_keep_alive_interval: Option<i64>,
    pub tcp_max_seg: Option<u32>,
    pub tcp_user_timeout: Option<u32>,
    pub tcp_window_clamp: Option<u32>,
    #[serde(rename = "tcpCongestion", alias = "tcpcongestion")]
    pub tcp_congestion: Option<String>,
    pub domain_strategy: Option<String>,
    pub dialer_proxy: Option<String>,
    pub happy_eyeballs: Option<HappyEyeballs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HappyEyeballs {
    pub try_delay_ms: Option<u64>,
    #[serde(rename = "prioritizeIPv6")]
    pub prioritize_ipv6: Option<bool>,
    pub interleave: Option<u32>,
    pub max_concurrent_try: Option<u32>,
}

#[derive(Deserialize)]
pub struct VlessSettings {
    pub vnext: Vec<VlessServer>,
}

#[derive(Deserialize)]
pub struct VlessServer {
    pub address: String,
    pub port: u16,
    pub users: Vec<VlessUser>,
}

#[derive(Deserialize)]
pub struct VlessUser {
    pub id: String,
    pub encryption: Option<String>,
    pub flow: Option<String>,
}

#[derive(Deserialize)]
pub struct VmessSettings {
    pub vnext: Vec<VmessServer>,
}

#[derive(Deserialize)]
pub struct VmessServer {
    pub address: String,
    pub port: u16,
    pub users: Vec<VmessUser>,
}

#[derive(Deserialize)]
pub struct VmessUser {
    pub id: String,
    pub security: Option<String>,
}

#[derive(Deserialize)]
pub struct HysteriaSettings {
    pub address: String,
    pub port: u16,
    pub version: Option<u32>,
    pub auth: Option<String>,
    pub password: Option<String>,
}

#[derive(Deserialize)]
pub struct ServerList<T> {
    pub servers: Vec<T>,
}

#[derive(Deserialize)]
pub struct TrojanServer {
    pub address: String,
    pub port: u16,
    pub password: String,
}

#[derive(Deserialize)]
pub struct ShadowsocksServer {
    pub address: String,
    pub port: u16,
    pub method: String,
    pub password: String,
}

#[derive(Deserialize, Default)]
pub struct FreedomSettings {
    pub fragment: Option<FragmentSettings>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FragmentSettings {
    pub packets: Option<String>,
    pub length: Value,
    pub interval: Value,
    pub max_split: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopbackSettings {
    pub inbound_tag: String,
}

fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

fn string_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    match Option::<Value>::deserialize(d)? {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.split(',').map(String::from).collect())),
        Some(v) => serde_json::from_value(v).map(Some).map_err(serde::de::Error::custom),
    }
}

const FIELDS: &str = "remarks outbounds routing dns burstObservatory observatory policy meta serverDescription tag protocol settings streamSettings mux proxySettings transportLayer enabled xudpProxyUDP443 method network security tlsSettings realitySettings tcpSettings rawSettings xhttpSettings splithttpSettings grpcSettings wsSettings httpupgradeSettings hysteriaSettings finalmask sockopt serverName alpn fingerprint allowInsecure pinnedPeerCertSha256 verifyPeerCertByName spiderX publicKey password mldsa65Verify shortId header initial_windows_size initialWindowsSize serviceName authority multiMode userAgent user_agent idle_timeout health_check_timeout permit_without_stream path host headers heartbeatPeriod auth udpIdleTimeout tcp udp quicParams type congestion bbrProfile brutalUp brutalDown udpHop ports interval initStreamReceiveWindow maxStreamReceiveWindow initConnectionReceiveWindow maxConnectionReceiveWindow maxIdleTimeout keepAlivePeriod disablePathMTUDiscovery tcpKeepAliveIdle tcpKeepAliveInterval tcpMaxSeg tcpUserTimeout tcpWindowClamp tcpCongestion tcpcongestion domainStrategy dialerProxy happyEyeballs tryDelayMs prioritizeIPv6 interleave maxConcurrentTry vnext address port users id encryption flow version servers fragment packets length maxSplit inboundTag response nonIPQuery blockTypes rules domain domains ip sourcePort outboundTag balancerTag balancers selector strategy fallbackTag hosts queryStrategy skipFallback finalQuery timeoutMs disableFallback disableFallbackIfMatch disableCache levels connIdle maxRTT baselines expected tolerance costs match regexp value";
const EXTRA: &str = "probeURL subjectSelector pingConfig destination sampleCount expectedIPs expectIPs unexpectedIPs serveStale serveExpiredTTL enableParallelQuery clientIp useSystemHosts";

fn canonical(k: &str) -> Option<&'static str> {
    type Names = (HashMap<&'static str, &'static str>, HashMap<String, &'static str>);
    static NAMES: std::sync::OnceLock<Names> = std::sync::OnceLock::new();
    let (exact, lower) = NAMES.get_or_init(|| {
        let mut lower = HashMap::new();
        for f in FIELDS.split_whitespace().chain(EXTRA.split_whitespace()) {
            lower.entry(f.to_ascii_lowercase()).or_insert(f);
        }
        let exact = FIELDS.split_whitespace().chain(EXTRA.split_whitespace()).map(|f| (f, lower[&f.to_ascii_lowercase()])).collect();
        (exact, lower)
    });
    exact.get(k).or_else(|| lower.get(&k.to_ascii_lowercase())).copied()
}

pub fn normalize(v: &mut Value) {
    match v {
        Value::Array(a) => {
            for v in a {
                normalize(v);
            }
        }
        Value::Object(o) => {
            if o.keys().all(|k| canonical(k).is_none_or(|c| c == k)) {
                for (k, val) in o.iter_mut() {
                    if k != "headers" && k != "hosts" {
                        normalize(val);
                    }
                }
                return;
            }
            let old = std::mem::take(o);
            for (k, mut val) in old {
                let key = canonical(&k).unwrap_or(&k);
                if key != "headers" && key != "hosts" {
                    normalize(&mut val);
                }
                o.insert(key.to_string(), val);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod normalize_tests {
    use super::*;

    fn reference(v: &mut Value) {
        match v {
            Value::Array(a) => {
                for v in a {
                    reference(v);
                }
            }
            Value::Object(o) => {
                let old = std::mem::take(o);
                for (k, mut val) in old {
                    let key = FIELDS.split_whitespace().chain(EXTRA.split_whitespace()).find(|f| f.eq_ignore_ascii_case(&k)).unwrap_or(&k);
                    if key != "headers" && key != "hosts" {
                        reference(&mut val);
                    }
                    o.insert(key.to_string(), val);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn same_result_as_the_linear_scan() {
        let mut v: Value = serde_json::json!([
            {"Remarks": "a", "OUTBOUNDS": [{"Tag": "proxy", "protocol": "vless", "StreamSettings": {"Headers": {"X-Key": 1}}}],
             "routing": {"rules": [{"Domain": ["x", "y"], "outboundTag": "proxy"}], "Balancers": [{"Selector": ["p"]}]},
             "dns": {"Hosts": {"A.B": "1.2.3.4"}, "useSystemhosts": true}, "unknownKey": {"InnerUnknown": 1}},
            {"remarks": "b", "outbounds": [], "routing": {"rules": [{"domain": ["z"], "outboundTag": "direct"}]}, "TcpCongestion": "bbr", "tcpcongestion": "x", "ProbeUrl": "u"}
        ]);
        let mut want = v.clone();
        reference(&mut want);
        let keep = |v: &Value| v.to_string();
        normalize(&mut v);
        assert_eq!(keep(&v), keep(&want));
    }
}
