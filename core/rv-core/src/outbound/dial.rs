use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use rv_config::{Network, OutboundKind, Profile, Security, StreamSettings};
use rv_net::{Coalesced, DialOptions, Family, FragmentConfig, FragmentStream, HappyEyeballs, Protect};
use rv_tls::{ClientConfig as TlsConfig, Fingerprint, RealityConfig, Verify};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use std::rc::Rc;

use rv_vless::Address;

use crate::{BoxStream, Error, Proxy};

#[cfg(any(target_os = "android", target_os = "ios"))]
const KEEPALIVE_IDLE_MIN: u32 = 60;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const KEEPALIVE_IDLE_MIN: u32 = 0;

pub enum Transport {
    Tcp(Coalesced),
    Fragmented(FragmentStream<Coalesced>),
    Chained(BoxStream),
}

impl AsyncRead for Transport {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Tcp(s) => Pin::new(s).poll_read(cx, buf),
            Transport::Fragmented(s) => Pin::new(s).poll_read(cx, buf),
            Transport::Chained(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Transport {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Transport::Tcp(s) => Pin::new(s).poll_write(cx, buf),
            Transport::Fragmented(s) => Pin::new(s).poll_write(cx, buf),
            Transport::Chained(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Tcp(s) => Pin::new(s).poll_flush(cx),
            Transport::Fragmented(s) => Pin::new(s).poll_flush(cx),
            Transport::Chained(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Tcp(s) => Pin::new(s).poll_shutdown(cx),
            Transport::Fragmented(s) => Pin::new(s).poll_shutdown(cx),
            Transport::Chained(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

#[derive(Clone)]
pub struct Dialer {
    pub address: String,
    pub port: u16,
    dial: DialOptions,
    fragment: Option<FragmentConfig>,
    via: Option<Rc<Proxy>>,
    via_ip: bool,
    pub tls: Option<Arc<TlsConfig>>,
    pub reality: bool,
}

impl Dialer {
    pub fn new(profile: &Profile, stream: &StreamSettings, address: &str, port: u16, protect: Option<Protect>) -> Result<Self, Error> {
        let so = &stream.sockopt;
        #[allow(clippy::unnecessary_min_or_max)]
        let dial = DialOptions {
            tcp_keepalive: so
                .tcp_keepalive
                .map(|(idle, interval)| (Duration::from_secs(idle.max(KEEPALIVE_IDLE_MIN).into()), Duration::from_secs(interval.into()))),
            tcp_no_delay: true,
            tcp_max_seg: so.tcp_max_seg,
            tcp_user_timeout: so.tcp_user_timeout.map(|ms| Duration::from_millis(ms.into())),
            tcp_window_clamp: so.tcp_window_clamp,
            tcp_congestion: so.tcp_congestion.clone(),
            family: match so.family {
                rv_config::Family::Any => Family::Any,
                rv_config::Family::V4 { force } => Family::V4 { force },
                rv_config::Family::V6 { force } => Family::V6 { force },
                rv_config::Family::V4First => Family::V4First,
                rv_config::Family::V6First => Family::V6First,
            },
            happy_eyeballs: so.happy_eyeballs.map(|h| HappyEyeballs {
                try_delay: Duration::from_millis(h.try_delay_ms),
                prioritize_ipv6: h.prioritize_ipv6,
                interleave: h.interleave,
                max_concurrent_try: h.max_concurrent_try,
            }),
            protect: protect.clone(),
            timeout: None,
        };
        let (fragment, via) = match &so.dialer_proxy {
            None => (None, None),
            Some(dp) => match profile.outbound(dp).map(|o| &o.kind) {
                Some(OutboundKind::Freedom(f)) => (
                    f.fragment.as_ref().map(|f| FragmentConfig {
                        packets: f.packets,
                        lengths: vec![(f.length.from, f.length.to)],
                        delays: vec![(f.interval_ms.from, f.interval_ms.to)],
                        max_split: (f.max_split.from, f.max_split.to),
                    }),
                    None,
                ),
                Some(k) if k.is_proxy() => {
                    no_cycle(profile, dp)?;
                    (None, Some(Rc::new(Proxy::build(profile, dp, protect.clone())?)))
                }
                Some(OutboundKind::Unsupported { reason, .. }) => {
                    return Err(Error::Unsupported(format!("dialerProxy `{dp}`: {reason}")));
                }
                _ => {
                    return Err(Error::Unsupported(format!("dialerProxy `{dp}` (only freedom+fragment or a proxy)")));
                }
            },
        };
        let mut fragment = fragment;
        for m in &stream.finalmask.tcp {
            match m {
                rv_config::TcpMask::Fragment(f) => {
                    fragment = Some(FragmentConfig {
                        packets: f.packets,
                        lengths: f.lengths.clone(),
                        delays: f.delays.clone(),
                        max_split: f.max_split,
                    });
                }
                rv_config::TcpMask::Other(kind) => {
                    return Err(Error::Unsupported(format!("finalmask tcp {kind}")));
                }
            }
        }
        let (tls, reality) = match &stream.security {
            Security::None => (None, false),
            Security::Tls(t) => (
                Some(Arc::new(TlsConfig {
                    server_name: if t.server_name.is_empty() { address.to_string() } else { t.server_name.clone() },
                    alpn: fingerprint_alpn(&stream.network, &t.alpn, tls_fingerprint(&stream.network, t)?),
                    alpn_override: alpn_override(&stream.network, &t.alpn),
                    fingerprint: tls_fingerprint(&stream.network, t)?,
                    verify: if !t.pinned_sha256.is_empty() || !t.verify_names.is_empty() {
                        Verify::Pinned(rv_tls::Pins { sha256: t.pinned_sha256.clone(), names: t.verify_names.clone() })
                    } else if t.allow_insecure {
                        Verify::InsecureSkipVerify
                    } else {
                        Verify::WebPki
                    },
                })),
                false,
            ),
            Security::Reality(r) => (
                Some(Arc::new(TlsConfig {
                    server_name: if r.server_name.is_empty() { address.to_string() } else { r.server_name.clone() },
                    alpn: vec![],
                    alpn_override: false,
                    fingerprint: reality_fingerprint(&r.fingerprint)?,
                    verify: Verify::Reality(RealityConfig {
                        public_key: r.public_key,
                        mldsa65_verify: r.mldsa65_verify.clone(),
                        short_id: r.short_id,
                        spider_x: r.spider_x.clone(),
                    }),
                })),
                true,
            ),
        };
        Ok(Self { address: address.to_string(), port, dial, fragment, via, via_ip: so.dialer_proxy_ip, tls, reality })
    }

    pub fn with_alpn(&self, alpn: &[&[u8]]) -> Self {
        let mut d = self.clone();
        if let Some(t) = &d.tls {
            let mut t = (**t).clone();
            t.alpn_override = true;
            t.alpn = alpn.iter().map(|a| a.to_vec()).collect();
            d.tls = Some(Arc::new(t));
        }
        d
    }

    pub fn server_name(&self) -> &str {
        self.tls.as_ref().map(|t| t.server_name.as_str()).filter(|s| !s.is_empty()).unwrap_or(&self.address)
    }

    pub fn reset(&self) {
        if let Some(via) = &self.via {
            via.reset();
        }
    }

    pub fn chained(&self) -> bool {
        self.via.is_some()
    }

    pub async fn tcp(&self) -> io::Result<Transport> {
        if let Some(via) = &self.via {
            let dest = if matches!(self.dial.family, Family::V4 { force: true }) {
                let ips = rv_net::resolve_protected(&self.address, self.dial.protect.as_ref()).await?;
                let ip = self
                    .dial
                    .family
                    .pick(ips)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no IPv4 address"))?;
                Address::from_ip(ip)
            } else {
                match self.address.parse::<std::net::IpAddr>() {
                    Ok(ip) => Address::from_ip(ip),
                    Err(_) => match rv_net::known_hosts(&self.address).filter(|_| self.via_ip) {
                        Some(ips) => Address::from_ip(ips[0]),
                        None => Address::Domain(self.address.clone()),
                    },
                }
            };
            let s = Box::pin(via.connect(dest, self.port)).await.map_err(io::Error::other)?;
            return Ok(Transport::Chained(s));
        }
        let tcp = Coalesced::new(rv_net::dial_tcp(&self.address, self.port, &self.dial).await?);
        Ok(match &self.fragment {
            Some(f) => Transport::Fragmented(FragmentStream::new(tcp, f.clone())),
            None => Transport::Tcp(tcp),
        })
    }

    pub async fn tls(&self) -> Result<rv_tls::TlsStream<Transport>, Error> {
        let cfg = self.tls.clone().ok_or_else(|| Error::Config("tls is not configured".into()))?;
        const REALITY_TRIES: usize = 4;
        let mut attempt = 1;
        loop {
            let t0 = std::time::Instant::now();
            crate::stage::set("tcp");
            let tcp = self.tcp().await?;
            let peer = match &tcp {
                Transport::Tcp(s) => s.get_ref().peer_addr().map(|a| a.ip().to_string()).unwrap_or_default(),
                Transport::Fragmented(s) => s.get_ref().get_ref().peer_addr().map(|a| a.ip().to_string()).unwrap_or_default(),
                Transport::Chained(_) => "chain".into(),
            };
            let t1 = std::time::Instant::now();
            rv_tls::set_background(|f| {
                tokio::task::spawn_local(f);
            });
            crate::stage::set(if self.reality { "reality" } else { "tls" });
            let tls = rv_tls::connect(tcp, cfg.clone()).await;
            rv_stats::add(rv_stats::Slot::Handshakes, 1);
            crate::stage::set("handshake");
            tracing::debug!(
                target: "rv::dial",
                "{}:{} peer={peer} tcp_ms={} tls_ms={} try={attempt} err={}",
                self.address,
                self.port,
                (t1 - t0).as_millis(),
                t1.elapsed().as_millis(),
                tls.as_ref().err().map(ToString::to_string).unwrap_or_default(),
            );
            let retry = match &tls {
                Err(rv_tls::Error::RealityAuth) => true,
                Err(rv_tls::Error::Protocol(_) | rv_tls::Error::Record(_) | rv_tls::Error::Alert(_)) => self.reality,
                _ => false,
            };
            match tls {
                Err(_) if retry && attempt < REALITY_TRIES => {
                    use ring::rand::SecureRandom;
                    let mut jitter = [0; 2];
                    let _ = ring::rand::SystemRandom::new().fill(&mut jitter);
                    tokio::time::sleep(std::time::Duration::from_millis(120 * attempt as u64 + u16::from_ne_bytes(jitter) as u64 % 200))
                        .await;
                    attempt += 1;
                }
                r => return Ok(r?),
            }
        }
    }

    pub async fn stream(&self) -> Result<BoxStream, Error> {
        match &self.tls {
            Some(_) => Ok(Box::new(self.tls().await?)),
            None => {
                let t0 = std::time::Instant::now();
                crate::stage::set("tcp");
                let t = self.tcp().await;
                rv_stats::add(rv_stats::Slot::Handshakes, 1);
                crate::stage::set("handshake");
                let peer = match &t {
                    Ok(Transport::Tcp(s)) => s.get_ref().peer_addr().map(|a| a.ip().to_string()).unwrap_or_default(),
                    _ => String::new(),
                };
                tracing::debug!(
                    target: "rv::dial",
                    "{}:{} peer={peer} tcp_ms={} err={}",
                    self.address,
                    self.port,
                    t0.elapsed().as_millis(),
                    t.as_ref().err().map(ToString::to_string).unwrap_or_default(),
                );
                Ok(Box::new(t?))
            }
        }
    }
}

fn no_cycle(profile: &Profile, from: &str) -> Result<(), Error> {
    let mut seen = vec![from.to_string()];
    let mut cur = from.to_string();
    while let Some(next) = profile.outbound(&cur).and_then(|o| o.stream.sockopt.dialer_proxy.clone()) {
        if seen.contains(&next) || seen.len() > 8 {
            return Err(Error::Config(format!("dialerProxy loop at `{next}`")));
        }
        seen.push(next.clone());
        cur = next;
    }
    Ok(())
}

pub fn fingerprint(name: &str) -> Result<Fingerprint, Error> {
    Fingerprint::from_xray(name).ok_or_else(|| Error::Unsupported(format!("TLS fingerprint `{name}`")))
}

fn fingerprint_alpn(network: &Network, configured: &[String], fp: Fingerprint) -> Vec<Vec<u8>> {
    if matches!(network, Network::Ws(_) | Network::HttpUpgrade(_)) {
        if configured == ["h2", "http/1.1"] { vec![b"h2".to_vec(), b"http/1.1".to_vec()] } else { vec![b"http/1.1".to_vec()] }
    } else if matches!(network, Network::Tcp | Network::TcpHttpHeader) && configured == ["http/1.1"] {
        vec![b"http/1.1".to_vec()]
    } else if fp.uses_configured_alpn() {
        if configured.is_empty() {
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        } else {
            configured.iter().map(|s| s.as_bytes().to_vec()).collect()
        }
    } else {
        vec![]
    }
}

fn tls_fingerprint(network: &Network, t: &rv_config::Tls) -> Result<Fingerprint, Error> {
    if matches!(network, Network::Xhttp(_)) && t.alpn == ["h3"] { Ok(Fingerprint::Chrome) } else { fingerprint(&t.fingerprint) }
}

fn alpn_override(network: &Network, configured: &[String]) -> bool {
    matches!(network, Network::Ws(_) | Network::HttpUpgrade(_))
        || (matches!(network, Network::Tcp | Network::TcpHttpHeader) && configured == ["http/1.1"])
}

fn reality_fingerprint(name: &str) -> Result<Fingerprint, Error> {
    let fp = fingerprint(name)?;
    if matches!(fp, Fingerprint::Golang | Fingerprint::UtlsGolang) {
        return Err(Error::Config(format!("invalid REALITY fingerprint `{name}`")));
    }
    Ok(fp)
}

pub fn udp_ip(ips: &[std::net::IpAddr], family: rv_config::Family) -> Option<std::net::IpAddr> {
    use rv_config::Family as F;
    let v4 = ips.iter().find(|i| i.is_ipv4()).copied();
    let v6 = ips.iter().find(|i| i.is_ipv6()).copied();
    match family {
        F::V4 { force: true } => v4,
        F::V6 { force: true } => v6,
        F::V6 { force: false } | F::V6First => v6.or(v4),
        F::V4 { force: false } | F::V4First | F::Any => v4.or(v6),
    }
}

#[cfg(test)]
mod udp_ip_tests {
    use super::udp_ip;
    use rv_config::Family;

    #[test]
    fn picks_family() {
        let ips = ["203.0.113.1".parse().unwrap(), "2001:db8::1".parse().unwrap()];
        assert!(udp_ip(&ips, Family::Any).unwrap().is_ipv4());
        assert!(udp_ip(&ips, Family::V6 { force: true }).unwrap().is_ipv6());
        assert!(udp_ip(&ips, Family::V6First).unwrap().is_ipv6());
        assert_eq!(udp_ip(&ips[..1], Family::V6 { force: true }), None);
        assert!(udp_ip(&ips[..1], Family::V6 { force: false }).unwrap().is_ipv4());
    }

    #[test]
    fn explicit_cert_pins_survive_legacy_allow_insecure() {
        let config = serde_json::json!({"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "127.0.0.1", "port": 443, "password": "loopback-test"}]}, "streamSettings": {"security": "tls", "tlsSettings": {"allowInsecure": true, "verifyPeerCertByName": "cert.test"}}}]});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let out = p.outbounds.iter().find(|o| o.kind.is_proxy()).unwrap();
        let d = super::Dialer::new(&p, &out.stream, "127.0.0.1", 443, None).unwrap();
        assert!(matches!(&d.tls.unwrap().verify, rv_tls::Verify::Pinned(p) if p.names == ["cert.test"]));
    }

    #[test]
    fn fingerprint_alpn_matches_xray_transport_override() {
        for (network, configured, expected) in [
            ("raw", vec!["http/1.1"], vec!["http/1.1"]),
            ("raw", vec!["h2"], vec![]),
            ("xhttp", vec!["http/1.1"], vec![]),
            ("grpc", vec!["h2"], vec![]),
            ("ws", vec![], vec!["http/1.1"]),
            ("ws", vec!["h2", "http/1.1"], vec!["h2", "http/1.1"]),
            ("httpupgrade", vec!["h2", "http/1.1"], vec!["h2", "http/1.1"]),
        ] {
            let cfg = serde_json::json!({"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "127.0.0.1", "port": 443, "password": "loopback-test"}]}, "streamSettings": {
                "network": network, "security": "tls", "tlsSettings": {"fingerprint": "chrome", "alpn": configured}, "xhttpSettings": {"path": "/fp"}}}]});
            let p = rv_config::parse_subscription(&cfg.to_string()).unwrap().remove(0);
            let out = p.outbounds.iter().find(|o| o.kind.is_proxy()).unwrap();
            let stream = crate::transport::Stream::new(&p, out, "127.0.0.1", 443, None).unwrap();
            let actual = &stream.dialer.tls.as_ref().unwrap().alpn;
            assert_eq!(actual, &expected.iter().map(|s| s.as_bytes().to_vec()).collect::<Vec<_>>(), "{network}: {configured:?}");
        }
    }
}
