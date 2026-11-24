mod kx;
mod moderate;
pub(crate) use moderate::{Rx, Writable, bytes as recv_bytes};

use crate::Error;
use quinn::udp::{RecvMeta, Transmit};
use quinn::{AsyncUdpSocket, Runtime, UdpPoller};
use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Poll};
use std::time::Duration;

pub(crate) fn initial_destination_cid() -> quinn_proto::ConnectionId {
    use quinn_proto::ConnectionIdGenerator;
    use ring::rand::SecureRandom;
    let mut r = [0; 4];
    let _ = ring::rand::SystemRandom::new().fill(&mut r);
    let len = 8 + (u32::from_le_bytes(r) % 13) as usize;
    quinn_proto::RandomConnectionIdGenerator::new(len).generate_cid()
}

pub(crate) fn endpoint(socket: Arc<dyn AsyncUdpSocket>, runtime: Arc<dyn Runtime>, cid_len: usize) -> io::Result<quinn::Endpoint> {
    let mut cfg = quinn::EndpointConfig::default();
    cfg.grease_quic_bit(false).quic_go_compat(true);
    cfg.max_udp_payload_size(1452).map_err(io::Error::other)?;
    cfg.cid_generator(move || Box::new(quinn_proto::RandomConnectionIdGenerator::new(cid_len)));
    quinn::Endpoint::new_with_abstract_socket(cfg, None, socket, runtime)
}

pub(crate) fn crypto(
    server_name: &str,
    insecure: bool,
    pins: &rv_tls::Pins,
    alpn: &[Vec<u8>],
) -> Result<quinn::crypto::rustls::QuicClientConfig, Error> {
    let mut provider = kx::provider();
    provider.cipher_suites = vec![
        rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256,
        rustls::crypto::ring::cipher_suite::TLS13_AES_256_GCM_SHA384,
        rustls::crypto::ring::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
    ];
    let provider = Arc::new(provider);
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| Error::Config(e.to_string()))?;
    let mut tls = if !pins.is_empty() {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinVerify { provider, pins: pins.clone(), server_name: server_name.into() }))
            .with_no_client_auth()
    } else if insecure {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(NoVerify(provider))).with_no_client_auth()
    } else {
        builder.with_root_certificates(crate::roots()).with_no_client_auth()
    };
    tls.alpn_protocols = alpn.to_vec();
    tls.resumption = rustls::client::Resumption::disabled();
    tls.go_quic_compat = true;
    quinn::crypto::rustls::QuicClientConfig::try_from(tls).map_err(|e| Error::Config(e.to_string()))
}

fn omit_datagram_param(now: std::time::SystemTime) -> bool {
    const FROM: u64 = 1_788_220_800;
    now.duration_since(std::time::UNIX_EPOCH).is_ok_and(|d| d.as_secs() >= FROM)
}

pub(crate) fn client_config(
    crypto: quinn::crypto::rustls::QuicClientConfig,
    q: &rv_config::QuicParams,
    incoming: u64,
    datagrams: bool,
    bps: Arc<std::sync::atomic::AtomicU64>,
) -> Result<quinn::ClientConfig, Error> {
    client_config_at(crypto, q, incoming, datagrams, bps, std::time::SystemTime::now())
}

#[cfg(feature = "plugins")]
pub(crate) fn client_config_announced(
    crypto: quinn::crypto::rustls::QuicClientConfig,
    q: &rv_config::QuicParams,
    incoming: u64,
    datagrams: bool,
) -> Result<quinn::ClientConfig, Error> {
    client_config_at(crypto, q, incoming, datagrams, Arc::new(std::sync::atomic::AtomicU64::new(0)), std::time::UNIX_EPOCH)
}

fn client_config_at(
    crypto: quinn::crypto::rustls::QuicClientConfig,
    q: &rv_config::QuicParams,
    incoming: u64,
    datagrams: bool,
    bps: Arc<std::sync::atomic::AtomicU64>,
    now: std::time::SystemTime,
) -> Result<quinn::ClientConfig, Error> {
    let mut t = quinn::TransportConfig::default();
    t.max_idle_timeout(Some(Duration::from_secs(q.max_idle_timeout).try_into().map_err(|_| Error::Config("maxIdleTimeout".into()))?));
    t.keep_alive_interval((q.keep_alive_period > 0).then(|| Duration::from_secs(q.keep_alive_period)));
    let var = |v: u64| quinn::VarInt::from_u64(v).map_err(|_| Error::Config("window".into()));
    t.stream_receive_window(var(q.max_stream_window)?);
    t.receive_window(var(q.max_conn_window)?);
    t.initial_receive_windows(var(q.init_stream_window)?, var(q.init_conn_window)?);
    t.datagram_receive_buffer_size(datagrams.then_some(1 << 20));
    t.omit_datagram_param(omit_datagram_param(now));
    t.max_concurrent_bidi_streams(var(incoming)?);
    t.max_concurrent_uni_streams(100u8.into());
    t.allow_spin(false);
    t.initial_mtu(1280);
    if q.disable_path_mtu_discovery {
        t.mtu_discovery_config(None);
    }
    if q.congestion == "reno" {
        t.congestion_controller_factory(Arc::new(quinn::congestion::NewRenoConfig::default()));
    } else {
        let mut bbr = quinn::congestion::BbrConfig::default();
        bbr.go_profile(&q.bbr_profile.to_ascii_lowercase()).map_err(|e| Error::Config(e.into()))?;
        bbr.initial_window(32 * 1280);
        t.congestion_controller_factory(Arc::new(crate::outbound::hysteria::cc::Factory { brutal_bps: bps, bbr: Arc::new(bbr) }));
    }
    let mut cfg = quinn::ClientConfig::new(Arc::new(crypto));
    cfg.transport_config(Arc::new(t));
    cfg.initial_dst_cid_provider(Arc::new(crate::quic::initial_destination_cid));
    Ok(cfg)
}

pub(crate) fn socket_endpoint(
    server: SocketAddr,
    q: &rv_config::QuicParams,
    mask: Option<&(String, Option<(u32, u32)>)>,
    protect: Option<&rv_net::Protect>,
    cid_len: usize,
) -> io::Result<quinn::Endpoint> {
    let runtime: Arc<dyn Runtime> = Arc::new(quinn::TokioRuntime);
    let (protect, mask, v6) = (protect.cloned(), mask.cloned(), server.is_ipv6());
    let make = {
        #[cfg_attr(any(target_os = "linux", target_os = "android"), allow(unused_variables))]
        let runtime = runtime.clone();
        move || -> io::Result<Arc<dyn AsyncUdpSocket>> {
            let udp = rv_net::udp_socket(v6, protect.as_ref())?;
            let inner: Arc<dyn AsyncUdpSocket> = match &mask {
                Some((pw, gecko)) => Arc::new(crate::outbound::hysteria::socket::Salamander::new(udp, pw, *gecko)?),
                #[cfg(any(target_os = "linux", target_os = "android"))]
                None => Arc::new(moderate::Moderated::new(udp.into_std()?)?),
                #[cfg(not(any(target_os = "linux", target_os = "android")))]
                None => runtime.wrap_udp_socket(udp.into_std()?)?,
            };
            Ok(Arc::new(GsoGuard(inner)))
        }
    };
    let first = make()?;
    let socket = if q.udp_hop_ports.is_empty() {
        first
    } else {
        crate::outbound::hysteria::hop::Hop::new(server, q.udp_hop_ports.clone(), q.udp_hop_interval, first, Box::new(make))
    };
    endpoint(socket, runtime, cid_len)
}

#[derive(Debug)]
struct NoVerify(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not used by QUIC".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct PinVerify {
    provider: Arc<rustls::crypto::CryptoProvider>,
    pins: rv_tls::Pins,
    server_name: String,
}

impl rustls::client::danger::ServerCertVerifier for PinVerify {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let certs: Vec<Vec<u8>> = std::iter::once(end_entity).chain(intermediates).map(|c| c.to_vec()).collect();
        rv_tls::verify_pinned(&certs, &self.pins, &self.server_name)
            .map(|_| rustls::client::danger::ServerCertVerified::assertion())
            .map_err(|e| rustls::Error::General(e.to_string()))
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not used by QUIC".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

pub(super) static GSO_BROKEN: AtomicBool = AtomicBool::new(false);
const RECV_BUDGET: usize = 256 * 1024;

#[derive(Debug)]
pub struct GsoGuard(pub Arc<dyn AsyncUdpSocket>);

impl GsoGuard {
    fn send_each(&self, t: &Transmit, seg: usize) -> io::Result<()> {
        for d in t.contents.chunks(seg.max(1)) {
            self.0.try_send(&Transmit { destination: t.destination, ecn: t.ecn, contents: d, segment_size: None, src_ip: t.src_ip })?;
        }
        Ok(())
    }
}

impl AsyncUdpSocket for GsoGuard {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        self.0.clone().create_io_poller()
    }

    fn try_send(&self, t: &Transmit) -> io::Result<()> {
        let Some(seg) = t.segment_size.filter(|&s| s < t.contents.len()) else {
            return self.0.try_send(t);
        };
        if GSO_BROKEN.load(Ordering::Relaxed) {
            return self.send_each(t, seg);
        }
        self.0.try_send(t)?;
        if self.0.max_transmit_segments() == 1 {
            GSO_BROKEN.store(true, Ordering::Relaxed);
            tracing::info!(target: "rv::quic", "GSO not supported: off for all QUIC sockets");
            return self.send_each(t, seg);
        }
        Ok(())
    }

    fn poll_recv(&self, cx: &mut Context, bufs: &mut [IoSliceMut<'_>], meta: &mut [RecvMeta]) -> Poll<io::Result<usize>> {
        let slot = bufs.first().map_or(1, |b| b.len().max(1));
        let n = (RECV_BUDGET / slot).clamp(1, bufs.len().max(1)).min(bufs.len());
        self.0.poll_recv(cx, &mut bufs[..n], &mut meta[..n])
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }

    fn may_fragment(&self) -> bool {
        self.0.may_fragment()
    }

    fn max_transmit_segments(&self) -> usize {
        if GSO_BROKEN.load(Ordering::Relaxed) { 1 } else { self.0.max_transmit_segments() }
    }

    fn max_receive_segments(&self) -> usize {
        self.0.max_receive_segments()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(idle: Duration) -> quinn::Endpoint {
        let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![include_bytes!("../tests/fixtures/h3-cert.der").to_vec().into()],
                rustls::pki_types::PrivateKeyDer::Pkcs8(include_bytes!("../tests/fixtures/h3-key.der").to_vec().into()),
            )
            .unwrap();
        tls.alpn_protocols = vec![b"h3".to_vec()];
        let mut cfg = quinn::ServerConfig::with_crypto(Arc::new(quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap()));
        Arc::get_mut(&mut cfg.transport).unwrap().max_idle_timeout(Some(idle.try_into().unwrap()));
        quinn::Endpoint::server(cfg, "127.0.0.1:0".parse().unwrap()).unwrap()
    }

    fn client(peer: SocketAddr, q: &rv_config::QuicParams) -> (quinn::Endpoint, quinn::ClientConfig) {
        let endpoint = socket_endpoint(peer, q, None, None, 0).unwrap();
        let crypto = crypto("test.local", true, &rv_tls::Pins::default(), &[b"h3".to_vec()]).unwrap();
        let cfg = client_config(crypto, q, 0, false, Arc::new(std::sync::atomic::AtomicU64::new(0))).unwrap();
        (endpoint, cfg)
    }

    #[tokio::test]
    async fn go_keepalive_uses_negotiated_peer_idle() {
        let server = server(Duration::from_secs(4));
        let peer = server.local_addr().unwrap();
        let s = server.clone();
        let task = tokio::spawn(async move {
            let c = s.accept().await.unwrap().await.unwrap();
            c.closed().await
        });
        let q = rv_config::QuicParams {
            keep_alive_period: 60,
            max_idle_timeout: 120,
            disable_path_mtu_discovery: true,
            ..rv_config::QuicParams::xhttp()
        };
        let (endpoint, cfg) = client(peer, &q);
        let conn = endpoint.connect_with(cfg, peer, "test.local").unwrap().await.unwrap();
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert!(conn.close_reason().is_none(), "PING must use peer idle/2, not the configured 60s");
        let stats = conn.stats();
        let keepalive = stats.frame_tx.ping - stats.path.sent_plpmtud_probes;
        assert!(
            (1..=4).contains(&keepalive) && stats.frame_tx.ping <= 8,
            "no busy keepalive timer: {keepalive} keepalive, {} PING",
            stats.frame_tx.ping
        );
        conn.close(0x100u32.into(), b"");
        task.await.unwrap();
    }

    struct Pair {
        now: std::time::Instant,
        client: (quinn_proto::Endpoint, quinn_proto::ConnectionHandle, quinn_proto::Connection),
        server: (quinn_proto::Endpoint, Option<(quinn_proto::ConnectionHandle, quinn_proto::Connection)>),
        last: std::time::Instant,
    }

    const CLIENT: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 1)), 40000);
    const SERVER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 2)), 443);

    impl Pair {
        fn new(server_idle: Duration, q: &rv_config::QuicParams) -> Pair {
            Self::with(server_idle, q, false)
        }

        fn with(server_idle: Duration, q: &rv_config::QuicParams, datagrams: bool) -> Pair {
            Self::with_at(server_idle, q, datagrams, std::time::SystemTime::now())
        }

        fn with_at(server_idle: Duration, q: &rv_config::QuicParams, datagrams: bool, at: std::time::SystemTime) -> Pair {
            let now = std::time::Instant::now();
            let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![include_bytes!("../tests/fixtures/h3-cert.der").to_vec().into()],
                    rustls::pki_types::PrivateKeyDer::Pkcs8(include_bytes!("../tests/fixtures/h3-key.der").to_vec().into()),
                )
                .unwrap();
            tls.alpn_protocols = vec![b"h3".to_vec()];
            let mut cfg = quinn_proto::ServerConfig::with_crypto(Arc::new(quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap()));
            let t = Arc::get_mut(&mut cfg.transport).unwrap();
            t.max_idle_timeout(Some(server_idle.try_into().unwrap()));
            t.initial_mtu(1452).mtu_discovery_config(None).datagram_receive_buffer_size(Some(1 << 20));
            let server =
                quinn_proto::Endpoint::new(Arc::new(quinn_proto::EndpointConfig::default()), Some(Arc::new(cfg)), true, Some([1; 32]));
            let mut ecfg = quinn_proto::EndpointConfig::default();
            ecfg.grease_quic_bit(false).quic_go_compat(true);
            let mut client = quinn_proto::Endpoint::new(Arc::new(ecfg), None, true, Some([2; 32]));
            let crypto = crypto("test.local", true, &rv_tls::Pins::default(), &[b"h3".to_vec()]).unwrap();
            let cfg = client_config_at(crypto, q, 0, datagrams, Arc::new(std::sync::atomic::AtomicU64::new(0)), at).unwrap();
            let (ch, conn) = client.connect(now, cfg, SERVER, "test.local").unwrap();
            Pair { now, client: (client, ch, conn), server: (server, None), last: now }
        }

        fn run(&mut self, rounds: usize, ms: u64) {
            let mut buf = Vec::new();
            for _ in 0..rounds {
                self.now += Duration::from_millis(ms);
                let now = self.now;
                if self.client.2.poll_timeout().is_some_and(|t| t <= now) {
                    self.client.2.handle_timeout(now);
                }
                if let Some((_, c)) = &mut self.server.1
                    && c.poll_timeout().is_some_and(|t| t <= now)
                {
                    c.handle_timeout(now);
                }
                buf.clear();
                while let Some(t) = self.client.2.poll_transmit(now, 1, &mut buf) {
                    self.last = now;
                    let data = bytes::BytesMut::from(&buf[..t.size]);
                    buf.clear();
                    let mut resp = Vec::new();
                    match self.server.0.handle(now, CLIENT, None, None, data, &mut resp) {
                        Some(quinn_proto::DatagramEvent::NewConnection(incoming)) => {
                            self.server.1 = Some(self.server.0.accept(incoming, now, &mut resp, None).unwrap());
                        }
                        Some(quinn_proto::DatagramEvent::ConnectionEvent(_, ev)) => {
                            if let Some((_, c)) = &mut self.server.1 {
                                c.handle_event(ev);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some((sh, c)) = &mut self.server.1 {
                    while let Some(ev) = c.poll_endpoint_events() {
                        if let Some(ev) = self.server.0.handle_event(*sh, ev) {
                            c.handle_event(ev);
                        }
                    }
                    while c.poll().is_some() {}
                    while let Some(t) = c.poll_transmit(now, 1, &mut buf) {
                        let data = bytes::BytesMut::from(&buf[..t.size]);
                        buf.clear();
                        let mut resp = Vec::new();
                        if let Some(quinn_proto::DatagramEvent::ConnectionEvent(_, ev)) =
                            self.client.0.handle(now, SERVER, None, None, data, &mut resp)
                        {
                            self.client.2.handle_event(ev);
                            self.last = now;
                        }
                    }
                }
                while let Some(ev) = self.client.2.poll_endpoint_events() {
                    if let Some(ev) = self.client.0.handle_event(self.client.1, ev) {
                        self.client.2.handle_event(ev);
                    }
                }
                while self.client.2.poll().is_some() {}
            }
        }
    }

    #[test]
    fn hysteria_datagram_limit_is_1200_like_xray() {
        let before = std::time::UNIX_EPOCH + Duration::from_secs(1_788_220_800 - 1);
        let mut pair = Pair::with_at(Duration::from_secs(30), &rv_config::QuicParams::default(), true, before);
        pair.run(100, 5);
        let (_, server) = pair.server.1.as_mut().expect("server connection");
        let max = server.datagrams().max_size().expect("client advertises datagrams");
        assert!(max <= 1200, "server may send {max}-byte datagrams to the client");
    }

    #[test]
    fn hysteria_omits_datagram_param_since_september_2026_like_xray() {
        let from = std::time::UNIX_EPOCH + Duration::from_secs(1_788_220_800);
        let mut pair = Pair::with_at(Duration::from_secs(30), &rv_config::QuicParams::default(), true, from);
        pair.run(100, 5);
        let (_, server) = pair.server.1.as_mut().expect("server connection");
        assert!(server.datagrams().max_size().is_none(), "parameter must be absent");
        assert!(!pair.client.2.is_handshaking());
    }

    #[test]
    fn write_at_idle_deadline_is_not_sent_like_quic_go() {
        let mut pair = Pair::new(Duration::from_secs(2), &rv_config::QuicParams::default());
        pair.run(400, 5);
        assert!(!pair.client.2.is_handshaking() && !pair.client.2.is_closed(), "handshake done");
        let deadline = pair.last + Duration::from_millis(2100);
        assert!(pair.client.2.poll_timeout().is_some_and(|t| t <= deadline));
        let id = pair.client.2.streams().open(quinn_proto::Dir::Bi).unwrap();
        pair.client.2.send_stream(id).write(b"late").unwrap();
        let mut buf = Vec::new();
        let sent = pair.client.2.poll_transmit(deadline, 1, &mut buf).map(|t| t.size);
        pair.client.2.handle_timeout(deadline);
        assert_eq!(sent, None, "nothing goes out after the idle deadline");
        assert!(pair.client.2.is_closed(), "the idle timer closes the connection");
    }

    #[tokio::test]
    async fn idle_timeout_closes_without_mtu_probe_like_quic_go() {
        let server = server(Duration::from_secs(2));
        let target = server.local_addr().unwrap();
        let s = server.clone();
        let task = tokio::spawn(async move {
            let c = s.accept().await.unwrap().await.unwrap();
            c.closed().await
        });
        let near = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let far = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let peer = near.local_addr().unwrap();
        let client_addr = Arc::new(std::sync::Mutex::new(None));
        let relay = |from: Arc<tokio::net::UdpSocket>, to: Arc<tokio::net::UdpSocket>, upstream: bool| {
            let client_addr = client_addr.clone();
            tokio::spawn(async move {
                let mut buf = vec![0; 2048];
                loop {
                    let Ok((n, src)) = from.recv_from(&mut buf).await else { return };
                    let dst = if upstream {
                        *client_addr.lock().unwrap() = Some(src);
                        target
                    } else {
                        let Some(a) = *client_addr.lock().unwrap() else { continue };
                        a
                    };
                    let (to, pkt) = (to.clone(), buf[..n].to_vec());
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        let _ = to.send_to(&pkt, dst).await;
                    });
                }
            })
        };
        let up = relay(near.clone(), far.clone(), true);
        let down = relay(far.clone(), near.clone(), false);
        let q = rv_config::QuicParams::default();
        let (endpoint, cfg) = client(peer, &q);
        let conn = endpoint.connect_with(cfg, peer, "test.local").unwrap().await.unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let quiet = conn.stats().path.sent_plpmtud_probes;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let probes = conn.stats().path.sent_plpmtud_probes;
        assert!(
            matches!(conn.close_reason(), Some(quinn::ConnectionError::TimedOut)),
            "the connection must die at the negotiated 2 s idle timeout, not live on: {:?}, {} MTU probes at the deadline",
            conn.close_reason(),
            probes - quiet
        );
        assert_eq!(probes, quiet, "no MTU probe at the idle deadline");
        up.abort();
        down.abort();
        task.abort();
    }
}
