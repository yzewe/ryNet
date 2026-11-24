use std::sync::Arc;

use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair};
use rustls::crypto::ring as rprov;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, TrustAnchor};
use rv_tls::{ClientConfig, Error, Fingerprint, Verify};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

struct Pki {
    ca: CertificateDer<'static>,
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
}

fn pki(alg: &'static rcgen::SignatureAlgorithm) -> Pki {
    let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.distinguished_name.push(DnType::CommonName, "rv test ca");
    let ca = CertifiedIssuer::self_signed(ca_params, ca_key).unwrap();
    let leaf_key = if alg == &rcgen::PKCS_RSA_SHA256 {
        KeyPair::from_pem(include_str!("fixtures/tls12-rsa-key.pem")).unwrap()
    } else {
        KeyPair::generate_for(alg).unwrap()
    };
    let leaf = CertificateParams::new(vec!["test.example".to_string()]).unwrap().signed_by(&leaf_key, &ca).unwrap();
    Pki {
        ca: ca.der().clone(),
        chain: vec![leaf.der().clone(), ca.der().clone()],
        key: PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into()),
    }
}

fn server_config(
    pki: &Pki,
    suite: Option<rustls::SupportedCipherSuite>,
    groups: Option<Vec<&'static dyn rustls::crypto::SupportedKxGroup>>,
) -> Arc<rustls::ServerConfig> {
    let mut provider = rprov::default_provider();
    if let Some(s) = suite {
        provider.cipher_suites = vec![s];
    }
    if let Some(g) = groups {
        provider.kx_groups = g;
    }
    let mut cfg = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(pki.chain.clone(), pki.key.clone_key())
        .unwrap();
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    cfg.send_tls13_tickets = 2;
    Arc::new(cfg)
}

fn client_config(fp: Fingerprint, name: &str, verify: Verify) -> Arc<ClientConfig> {
    Arc::new(ClientConfig { server_name: name.into(), alpn: vec![], alpn_override: true, fingerprint: fp, verify })
}

async fn pair(
    server: Arc<rustls::ServerConfig>,
    client: Arc<ClientConfig>,
    roots: Vec<TrustAnchor<'static>>,
) -> (Result<rv_tls::TlsStream<DuplexStream>, Error>, Option<tokio_rustls::server::TlsStream<DuplexStream>>) {
    let (c, s) = tokio::io::duplex(64 * 1024);
    let acceptor = tokio_rustls::TlsAcceptor::from(server);
    let srv = tokio::spawn(async move { acceptor.accept(s).await.map_err(|e| eprintln!("server: {e}")).ok() });
    let cli = rv_tls::connect_with_roots(c, client, &roots).await;
    let srv = if cli.is_ok() {
        srv.await.unwrap()
    } else {
        srv.abort();
        None
    };
    (cli, srv)
}

fn roots(p: &Pki) -> Vec<TrustAnchor<'static>> {
    vec![webpki::anchor_from_trusted_cert(&p.ca).unwrap().to_owned()]
}

async fn exchange(mut cli: rv_tls::TlsStream<DuplexStream>, mut srv: tokio_rustls::server::TlsStream<DuplexStream>) {
    let up: Vec<u8> = (0..1_048_576u32).map(|i| (i * 7 % 251) as u8).collect();
    let down: Vec<u8> = (0..1_048_576u32).map(|i| (i * 13 % 241) as u8).collect();
    let (up2, down2) = (up.clone(), down.clone());
    let server = tokio::spawn(async move {
        let mut got = vec![0u8; up2.len()];
        let (mut r, mut w) = tokio::io::split(&mut srv);
        let (a, ()) = tokio::join!(r.read_exact(&mut got), async {
            w.write_all(&down2).await.unwrap();
            w.flush().await.unwrap();
        });
        a.unwrap();
        assert_eq!(got, up2);
        srv.shutdown().await.unwrap();
    });
    let mut got = Vec::new();
    let (mut r, mut w) = tokio::io::split(&mut cli);
    let (_, rr) = tokio::join!(
        async {
            w.write_all(&up).await.unwrap();
            w.flush().await.unwrap();
        },
        r.read_to_end(&mut got)
    );
    rr.unwrap();
    assert_eq!(got.len(), down.len());
    assert!(got == down);
    server.await.unwrap();
}

#[tokio::test]
async fn handshake_matrix() {
    let suites = [
        rprov::cipher_suite::TLS13_AES_128_GCM_SHA256,
        rprov::cipher_suite::TLS13_AES_256_GCM_SHA384,
        rprov::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
    ];
    for alg in [&rcgen::PKCS_ECDSA_P256_SHA256, &rcgen::PKCS_ECDSA_P384_SHA384] {
        let p = pki(alg);
        for fp in [Fingerprint::Chrome, Fingerprint::Firefox] {
            for s in suites {
                let (cli, srv) = pair(server_config(&p, Some(s), None), client_config(fp, "test.example", Verify::WebPki), roots(&p)).await;
                let cli = cli.unwrap_or_else(|e| panic!("{fp:?} {:?}: {e}", s.suite()));
                assert_eq!(cli.alpn(), Some(&b"h2"[..]));
                exchange(cli, srv.unwrap()).await;
            }
        }
    }
}

#[tokio::test]
async fn tls12_aead_matrix_authenticates_and_transfers_both_directions() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    for fp in [
        Fingerprint::Chrome,
        Fingerprint::Firefox,
        Fingerprint::Safari,
        Fingerprint::Ios,
        Fingerprint::Edge,
        Fingerprint::Qq,
        Fingerprint::Android,
        Fingerprint::Firefox55,
        Fingerprint::Firefox56,
        Fingerprint::Firefox63,
        Fingerprint::Firefox65,
        Fingerprint::Firefox99,
        Fingerprint::Firefox102,
        Fingerprint::Firefox105,
        Fingerprint::Chrome58,
        Fingerprint::Chrome62,
        Fingerprint::Chrome70,
        Fingerprint::Chrome72,
        Fingerprint::Chrome83,
        Fingerprint::Chrome87,
        Fingerprint::Chrome96,
        Fingerprint::Chrome100,
        Fingerprint::Chrome102,
        Fingerprint::Chrome106,
        Fingerprint::Ios11,
        Fingerprint::Ios12,
        Fingerprint::Safari16,
        Fingerprint::Chrome115Pq,
        Fingerprint::Chrome120Pq,
    ] {
        for suite in [
            rprov::cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
            rprov::cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            rprov::cipher_suite::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
        ] {
            let mut provider = rprov::default_provider();
            provider.cipher_suites = vec![suite];
            let mut server = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
                .with_protocol_versions(&[&rustls::version::TLS12])
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(p.chain.clone(), p.key.clone_key())
                .unwrap();
            server.alpn_protocols = vec![b"h2".to_vec()];
            let (client, server) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                pair(Arc::new(server), client_config(fp, "test.example", Verify::WebPki), roots(&p)),
            )
            .await
            .unwrap();
            let client = client.unwrap_or_else(|e| panic!("TLS 1.2 {fp:?} {:?}: {e}", suite.suite()));
            assert_eq!(client.alpn(), if fp == Fingerprint::Android { None } else { Some(b"h2".as_slice()) });
            exchange(client, server.unwrap()).await;
        }
    }
}

#[tokio::test]
async fn tls12_rejects_corrupted_server_signature_and_finished_with_insecure_enabled() {
    for corrupt_signature in [true, false] {
        let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
        let mut provider = rprov::default_provider();
        provider.cipher_suites = vec![rprov::cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256];
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(p.chain, p.key)
            .unwrap();
        let (client, bridge_client) = tokio::io::duplex(65536);
        let (bridge_server, server) = tokio::io::duplex(65536);
        let peer = tokio::spawn(async move {
            match tokio_rustls::TlsAcceptor::from(Arc::new(config)).accept(server).await {
                Ok(mut tls) => tls.read(&mut [0; 1]).await.err(),
                Err(e) => Some(e),
            }
        });
        let bridge = tokio::spawn(async move {
            let (mut client_read, mut client_write) = tokio::io::split(bridge_client);
            let (mut server_read, mut server_write) = tokio::io::split(bridge_server);
            let to_server = async {
                let _ = tokio::io::copy(&mut client_read, &mut server_write).await;
            };
            let to_client = async {
                let (mut changed, mut encrypted) = (false, false);
                loop {
                    let mut header = [0; 5];
                    if server_read.read_exact(&mut header).await.is_err() {
                        break;
                    }
                    let mut body = vec![0; u16::from_be_bytes([header[3], header[4]]) as usize];
                    if server_read.read_exact(&mut body).await.is_err() {
                        break;
                    }
                    if header[0] == 20 {
                        encrypted = true;
                    }
                    if !changed && corrupt_signature && !encrypted && header[0] == 22 {
                        let mut at = 0;
                        while at + 4 <= body.len() {
                            let len = u32::from_be_bytes([0, body[at + 1], body[at + 2], body[at + 3]]) as usize;
                            if at + 4 + len > body.len() {
                                break;
                            }
                            if body[at] == 12 {
                                body[at + 3 + len] ^= 1;
                                changed = true;
                                break;
                            }
                            at += 4 + len;
                        }
                    } else if !changed && !corrupt_signature && encrypted && header[0] == 22 && !body.is_empty() {
                        let end = body.len() - 1;
                        body[end] ^= 1;
                        changed = true;
                    }
                    if client_write.write_all(&header).await.is_err() || client_write.write_all(&body).await.is_err() {
                        break;
                    }
                    let _ = client_write.flush().await;
                }
                let _ = client_write.shutdown().await;
                changed
            };
            let (_, changed) = tokio::join!(to_server, to_client);
            changed
        });
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            rv_tls::connect_with_roots(client, client_config(Fingerprint::Firefox, "test.example", Verify::InsecureSkipVerify), &[]),
        )
        .await
        .unwrap();
        assert!(if corrupt_signature { matches!(result, Err(Error::BadCertificate(_))) } else { matches!(result, Err(Error::Record(_))) });
        assert!(tokio::time::timeout(std::time::Duration::from_secs(5), bridge).await.unwrap().unwrap());
        assert!(peer.await.unwrap().is_some());
    }
}

#[tokio::test]
async fn ed25519_certificate_is_not_offered() {
    let p = pki(&rcgen::PKCS_ED25519);
    let (cli, _) = pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "test.example", Verify::WebPki), roots(&p)).await;
    assert!(matches!(cli, Err(Error::Alert(40))), "{:?}", cli.err());
}

#[tokio::test]
async fn hello_retry_request_to_p256() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let groups: Vec<&'static dyn rustls::crypto::SupportedKxGroup> = vec![rprov::kx_group::SECP256R1];
    let (cli, srv) =
        pair(server_config(&p, None, Some(groups)), client_config(Fingerprint::Chrome, "test.example", Verify::WebPki), roots(&p)).await;
    exchange(cli.unwrap(), srv.unwrap()).await;
}

#[tokio::test]
async fn rejects_wrong_name_and_untrusted_root() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let (cli, _) = pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "evil.example", Verify::WebPki), roots(&p)).await;
    assert!(matches!(cli, Err(Error::BadCertificate(_))), "{:?}", cli.err());
    let (cli, _) = pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "test.example", Verify::WebPki), vec![]).await;
    assert!(matches!(cli, Err(Error::BadCertificate(_))), "{:?}", cli.err());
    let (cli, srv) =
        pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "evil.example", Verify::InsecureSkipVerify), vec![]).await;
    exchange(cli.unwrap(), srv.unwrap()).await;
}

#[tokio::test]
async fn direct_mode_keeps_stream_order() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let (cli, srv) =
        pair(server_config(&p, None, None), client_config(Fingerprint::Firefox, "test.example", Verify::WebPki), roots(&p)).await;
    let (mut cli, mut srv) = (cli.unwrap(), srv.unwrap());

    srv.write_all(b"hello").await.unwrap();
    srv.flush().await.unwrap();
    srv.get_mut().0.write_all(b"RAW-DOWN").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let mut two = [0u8; 2];
    cli.read_exact(&mut two).await.unwrap();
    assert_eq!(&two, b"he");
    let pending = cli.switch_read_to_direct();
    assert_eq!(pending, b"llo");
    let mut raw = [0u8; 8];
    cli.read_exact(&mut raw).await.unwrap();
    assert_eq!(&raw, b"RAW-DOWN");

    cli.write_all(b"abc").await.unwrap();
    cli.flush().await.unwrap();
    let mut abc = [0u8; 3];
    srv.read_exact(&mut abc).await.unwrap();
    assert_eq!(&abc, b"abc");
    cli.switch_write_to_direct();
    cli.write_all(b"RAW-UP").await.unwrap();
    cli.flush().await.unwrap();
    let mut up = [0u8; 6];
    srv.get_mut().0.read_exact(&mut up).await.unwrap();
    assert_eq!(&up, b"RAW-UP");
}

#[tokio::test]
async fn write_after_shutdown_is_error() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let (cli, _srv) =
        pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "test.example", Verify::WebPki), roots(&p)).await;
    let mut cli = cli.unwrap();
    cli.shutdown().await.unwrap();
    assert_eq!(cli.write(b"pong").await.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
    cli.flush().await.unwrap();
}

#[tokio::test]
async fn change_cipher_spec_after_server_finished_is_rejected() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let (cli, srv) =
        pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "test.example", Verify::WebPki), roots(&p)).await;
    let (mut cli, mut srv) = (cli.unwrap(), srv.unwrap());
    srv.get_mut().0.write_all(&[20, 3, 3, 0, 1, 1]).await.unwrap();
    srv.write_all(b"ok").await.unwrap();
    srv.flush().await.unwrap();
    let mut b = [0; 2];
    let r = tokio::time::timeout(std::time::Duration::from_secs(2), cli.read_exact(&mut b)).await.unwrap();
    assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn tls12_half_close_keeps_delayed_response_readable() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(rprov::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(p.chain.clone(), p.key.clone_key())
        .unwrap();
    let (cli, srv) = pair(Arc::new(server), client_config(Fingerprint::Chrome, "test.example", Verify::WebPki), roots(&p)).await;
    let (mut cli, mut srv) = (cli.unwrap(), srv.unwrap());
    let peer = tokio::spawn(async move {
        let mut b = [0; 7];
        srv.read_exact(&mut b).await.unwrap();
        assert_eq!(&b, b"request");
        let mut tail = [0; 1];
        tokio::select! { _ = srv.read(&mut tail) => panic!("TLS 1.2 close_notify before response"), _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {} }
        srv.write_all(b"reply").await.unwrap();
        srv.flush().await.unwrap();
    });
    cli.write_all(b"request").await.unwrap();
    cli.shutdown().await.unwrap();
    let mut b = [0; 5];
    tokio::time::timeout(std::time::Duration::from_secs(2), cli.read_exact(&mut b)).await.unwrap().unwrap();
    assert_eq!(&b, b"reply");
    assert_eq!(cli.write(b"x").await.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
    peer.await.unwrap();
}

fn sha256(der: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, der).as_ref().try_into().unwrap()
}

#[tokio::test]
async fn pinned_self_signed_ca_leaf() {
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::new(vec!["lk.example".to_string()]).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let cert = params.self_signed(&key).unwrap();
    let p = Pki { ca: cert.der().clone(), chain: vec![cert.der().clone()], key: PrivateKeyDer::Pkcs8(key.serialize_der().into()) };
    let pins = |h: [u8; 32]| Verify::Pinned(rv_tls::Pins { sha256: vec![h], names: vec![] });

    let (cli, _) = pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "lk.example", Verify::WebPki), vec![]).await;
    assert!(matches!(cli, Err(Error::BadCertificate(_))), "{:?}", cli.err());

    let (cli, _) = pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "lk.example", pins([7; 32])), vec![]).await;
    assert!(matches!(cli, Err(Error::BadCertificate(_))), "{:?}", cli.err());

    let (cli, srv) =
        pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "lk.example", pins(sha256(cert.der()))), vec![]).await;
    exchange(cli.unwrap(), srv.unwrap()).await;
}

#[tokio::test]
async fn pinned_ca_checks_name() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let ca = sha256(&p.ca);
    let v = |names: Vec<&str>| Verify::Pinned(rv_tls::Pins { sha256: vec![ca], names: names.into_iter().map(String::from).collect() });

    let (cli, srv) = pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "test.example", v(vec![])), vec![]).await;
    exchange(cli.unwrap(), srv.unwrap()).await;

    let (cli, _) = pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "evil.example", v(vec![])), vec![]).await;
    assert!(matches!(cli, Err(Error::BadCertificate(_))), "{:?}", cli.err());

    let (cli, srv) =
        pair(server_config(&p, None, None), client_config(Fingerprint::Chrome, "cdn.example", v(vec!["test.example"])), vec![]).await;
    exchange(cli.unwrap(), srv.unwrap()).await;
}

#[tokio::test]
async fn reality_fallback_crawls_compressed_same_origin_pages_on_h1_and_h2() {
    for h2 in [false, true] {
        let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
        let anchors = roots(&p);
        let mut config = (*server_config(&p, None, None)).clone();
        config.alpn_protocols = vec![if h2 { b"h2".to_vec() } else { b"http/1.1".to_vec() }];
        let html = br#"<a href="/next">next</a><a href="https://evil.invalid/steal">external</a>"#;
        let gzip = {
            use std::io::Write;
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(html).unwrap();
            e.finish().unwrap()
        };
        let (client, server) = tokio::io::duplex(65536);
        let peer = tokio::spawn(async move {
            let mut tls = tokio_rustls::TlsAcceptor::from(Arc::new(config)).accept(server).await.unwrap();
            let mut requests = Vec::new();
            if h2 {
                let mut conn = h2::server::handshake(tls).await.unwrap();
                while let Some(Ok((req, mut reply))) = conn.accept().await {
                    assert!(req.headers()["user-agent"].to_str().unwrap().contains("Firefox/148.0"));
                    if !requests.is_empty() {
                        assert_eq!(req.headers()["referer"], "https://test.example/start");
                    }
                    requests.push(req.uri().path().to_owned());
                    let mut body =
                        reply.send_response(http::Response::builder().header("content-encoding", "gzip").body(()).unwrap(), false).unwrap();
                    body.send_data(bytes::Bytes::copy_from_slice(&gzip), true).unwrap();
                }
            } else {
                for n in 0..2 {
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(tls.read_u8().await.unwrap());
                    }
                    let head = String::from_utf8(head).unwrap();
                    assert!(head.contains("Firefox/148.0"));
                    assert!(head.contains(&format!("Cookie: padding={}\r\n", "0".repeat(50))));
                    if n > 0 {
                        assert!(head.contains("Referer: https://test.example/start\r\n"));
                    }
                    requests.push(head.split_whitespace().nth(1).unwrap().to_owned());
                    tls.write_all(
                        format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", gzip.len()).as_bytes(),
                    )
                    .await
                    .unwrap();
                    tls.write_all(&gzip).await.unwrap();
                    tls.flush().await.unwrap();
                }
            }
            requests
        });
        let config = Arc::new(ClientConfig {
            server_name: "test.example".into(),
            alpn_override: true,
            fingerprint: Fingerprint::Firefox,
            alpn: vec![if h2 { b"h2".to_vec() } else { b"http/1.1".to_vec() }],
            verify: Verify::Reality(rv_tls::RealityConfig {
                public_key: [7; 32],
                mldsa65_verify: vec![],
                short_id: [0; 8],
                spider_x: "/start?p=50&c=1&t=1&i=0&r=500".into(),
            }),
        });
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(3), rv_tls::connect_with_roots(client, config, &anchors)).await.unwrap();
        assert!(matches!(result, Err(Error::RealityAuth)));
        let requests = tokio::time::timeout(std::time::Duration::from_secs(3), peer).await.unwrap().unwrap();
        assert_eq!(requests, vec!["/start", "/next"]);
    }
}

#[tokio::test]
async fn reality_valid_tls_fallback_completes_without_proxy_data() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let mut server = (*server_config(&p, None, None)).clone();
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    let (c, s) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move {
        let Ok(mut tls) = tokio_rustls::TlsAcceptor::from(Arc::new(server)).accept(s).await else {
            return None;
        };
        let mut request = Vec::new();
        while request.len() < 16384 && !request.ends_with(b"\r\n\r\n") {
            let Ok(byte) = tls.read_u8().await else {
                return None;
            };
            request.push(byte);
        }
        tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK").await.unwrap();
        tls.flush().await.unwrap();
        Some(request)
    });
    let config = Arc::new(ClientConfig {
        server_name: "test.example".into(),
        alpn: vec![b"http/1.1".to_vec()],
        alpn_override: true,
        fingerprint: Fingerprint::Chrome,
        verify: Verify::Reality(rv_tls::RealityConfig {
            public_key: [7; 32],
            mldsa65_verify: vec![],
            short_id: [0; 8],
            spider_x: "/".into(),
        }),
    });
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), rv_tls::connect_with_roots(c, config, &roots(&p))).await.unwrap();
    assert!(matches!(result, Err(Error::RealityAuth)));
    let request = server.await.unwrap().expect("ordinary TLS handshake did not finish");
    assert!(request.starts_with(b"GET / HTTP/1.1\r\n"));
    assert!(request.windows(b"Host: test.example".len()).any(|w| w == b"Host: test.example"));
}

#[tokio::test]
async fn hello_retry_request_to_p384() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let groups = vec![rprov::kx_group::SECP384R1];
    let (cli, srv) =
        pair(server_config(&p, None, Some(groups)), client_config(Fingerprint::Chrome, "test.example", Verify::WebPki), roots(&p)).await;
    exchange(cli.unwrap(), srv.unwrap()).await;
}

#[tokio::test]
async fn invalid_certificate_sends_encrypted_fatal_alert() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let (client, server) = tokio::io::duplex(65536);
    let acceptor = tokio_rustls::TlsAcceptor::from(server_config(&p, None, None));
    let server = tokio::spawn(async move { acceptor.accept(server).await.err().unwrap() });
    let result = rv_tls::connect_with_roots(client, client_config(Fingerprint::Chrome, "wrong.example", Verify::WebPki), &roots(&p)).await;
    assert!(matches!(result, Err(Error::BadCertificate(_))));
    let error = server.await.unwrap();
    assert!(
        matches!(
            error.get_ref().and_then(|e| e.downcast_ref::<rustls::Error>()),
            Some(rustls::Error::AlertReceived(rustls::AlertDescription::BadCertificate))
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn tls12_only_templates_negotiate_tls12_with_tls13_capable_server() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let mut server = rustls::ServerConfig::builder_with_provider(Arc::new(rprov::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(p.chain.clone(), p.key.clone_key())
        .unwrap();
    server.alpn_protocols = vec![b"h2".to_vec()];
    for name in ["android", "hellofirefox_55", "hellofirefox_56", "hellochrome_58", "hellochrome_62", "helloios_11_1", "helloios_12_1"] {
        let fp = Fingerprint::from_xray(name).unwrap();
        let (client, peer) = pair(Arc::new(server.clone()), client_config(fp, "test.example", Verify::WebPki), roots(&p)).await;
        let client = client.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(peer.as_ref().unwrap().get_ref().1.protocol_version(), Some(rustls::ProtocolVersion::TLSv1_2));
        exchange(client, peer.unwrap()).await;
    }
}

#[tokio::test]
async fn tls12_only_templates_verify_webpki_and_pins() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(rprov::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(p.chain.clone(), p.key.clone_key())
        .unwrap();
    let leaf: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, p.chain[0].as_ref()).as_ref().try_into().unwrap();
    let ca: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, p.ca.as_ref()).as_ref().try_into().unwrap();
    for fp in [Fingerprint::Android, Fingerprint::Firefox55, Fingerprint::Chrome58, Fingerprint::Ios11] {
        for (name, verify, anchors, ok) in [
            ("test.example", Verify::WebPki, roots(&p), true),
            ("evil.example", Verify::WebPki, roots(&p), false),
            ("test.example", Verify::WebPki, vec![], false),
            ("test.example", Verify::Pinned(rv_tls::Pins { sha256: vec![leaf], names: vec![] }), vec![], true),
            ("test.example", Verify::Pinned(rv_tls::Pins { sha256: vec![[7; 32]], names: vec![] }), roots(&p), false),
            ("test.example", Verify::Pinned(rv_tls::Pins { sha256: vec![ca], names: vec![] }), vec![], true),
            ("evil.example", Verify::Pinned(rv_tls::Pins { sha256: vec![ca], names: vec![] }), vec![], false),
        ] {
            let (client, peer) = pair(Arc::new(server.clone()), client_config(fp, name, verify), anchors).await;
            if ok {
                exchange(client.unwrap(), peer.unwrap()).await;
            } else {
                assert!(matches!(client, Err(Error::BadCertificate(_))), "{fp:?} {name}");
            }
        }
    }
}

#[tokio::test]
async fn psk_templates_reject_empty_session_before_clienthello() {
    for name in ["hellochrome_100_psk", "hellochrome_112_psk_shuf", "hellochrome_114_padding_psk_shuf", "hellochrome_115_pq_psk"] {
        let (c, mut s) = tokio::io::duplex(65536);
        let result =
            rv_tls::connect_with_roots(c, client_config(Fingerprint::from_xray(name).unwrap(), "test.example", Verify::WebPki), &[]).await;
        assert!(matches!(result, Err(Error::EmptyPsk)), "{name}");
        assert_eq!(s.read(&mut [0; 5]).await.unwrap(), 0, "no ClientHello/alert for empty PSK");
    }
}

#[tokio::test]
async fn reality_rejects_tls12_only_fingerprints_before_clienthello() {
    for name in
        ["android", "360", "hellofirefox_55", "hellofirefox_56", "hellochrome_58", "hellochrome_62", "helloios_11_1", "helloios_12_1"]
    {
        let (c, mut s) = tokio::io::duplex(65536);
        let verify =
            Verify::Reality(rv_tls::RealityConfig { public_key: [7; 32], short_id: [0; 8], mldsa65_verify: vec![], spider_x: "/".into() });
        let result = rv_tls::connect_with_roots(c, client_config(Fingerprint::from_xray(name).unwrap(), "test.example", verify), &[]).await;
        assert!(matches!(result, Err(Error::Unsupported(_))), "{name}");
        assert_eq!(s.read(&mut [0; 5]).await.unwrap(), 0, "{name}: no ClientHello or alert");
    }
}

#[tokio::test]
async fn tls12_legacy_templates_support_rsa_aes128_aes256_and_chacha20() {
    let p = pki(&rcgen::PKCS_RSA_SHA256);
    for fp in [Fingerprint::Android, Fingerprint::Firefox55, Fingerprint::Chrome58, Fingerprint::Ios11] {
        for suite in [
            rprov::cipher_suite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            rprov::cipher_suite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
            rprov::cipher_suite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
        ] {
            let mut provider = rprov::default_provider();
            provider.cipher_suites = vec![suite];
            let server = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
                .with_protocol_versions(&[&rustls::version::TLS12])
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(p.chain.clone(), p.key.clone_key())
                .unwrap();
            let (client, peer) = pair(Arc::new(server), client_config(fp, "test.example", Verify::WebPki), roots(&p)).await;
            exchange(client.unwrap(), peer.unwrap()).await;
        }
    }
}

#[tokio::test]
async fn tls12_cbc_selection_fails_explicitly_without_establishing_a_connection() {
    let (c, mut s) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move {
        let mut header = [0; 5];
        s.read_exact(&mut header).await.unwrap();
        let mut ch = vec![0; u16::from_be_bytes([header[3], header[4]]) as usize];
        s.read_exact(&mut ch).await.unwrap();
        let mut body = vec![3, 3];
        body.extend([0; 32]);
        body.push(0);
        body.extend([0x00, 0x35, 0, 0, 0]);
        let mut message = vec![2];
        message.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        message.extend(body);
        s.write_all(&[22, 3, 3, 0, message.len() as u8]).await.unwrap();
        s.write_all(&message).await.unwrap();
        let mut alert = [0; 7];
        let _ = s.read_exact(&mut alert).await;
    });
    let result = rv_tls::connect_with_roots(c, client_config(Fingerprint::Android, "test.example", Verify::WebPki), &[]).await;
    assert!(matches!(result,Err(Error::Unsupported(m)) if m.contains("RSA key exchange")));
    server.await.unwrap();
}

#[tokio::test]
async fn tls12_ecdhe_cbc_against_openssl() {
    if std::process::Command::new("openssl").arg("version").output().is_err() {
        return;
    }
    for (alg, ciphers) in [
        (&rcgen::PKCS_RSA_SHA256, ["ECDHE-RSA-AES128-SHA", "ECDHE-RSA-AES256-SHA", "ECDHE-RSA-AES128-SHA256"]),
        (&rcgen::PKCS_ECDSA_P256_SHA256, ["ECDHE-ECDSA-AES128-SHA", "ECDHE-ECDSA-AES256-SHA", "ECDHE-ECDSA-AES128-SHA256"]),
    ] {
        let p = pki(alg);
        let dir = std::env::temp_dir().join(format!("rv-tls-cbc-{}-{}", std::process::id(), ciphers[0]));
        std::fs::create_dir_all(&dir).unwrap();
        let (cert, key) = (dir.join("cert.der"), dir.join("key.der"));
        std::fs::write(&cert, &p.chain[0]).unwrap();
        std::fs::write(&key, p.key.secret_der()).unwrap();
        for cipher in ciphers {
            let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            let mut server = std::process::Command::new("openssl")
                .args(["s_server", "-quiet", "-naccept", "1", "-www", "-tls1_2"])
                .args(["-accept", &format!("127.0.0.1:{port}"), "-cipher", &format!("{cipher}:@SECLEVEL=0")])
                .arg("-cert")
                .arg(&cert)
                .args(["-certform", "DER"])
                .arg("-key")
                .arg(&key)
                .args(["-keyform", "DER"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let tcp = loop {
                if let Ok(t) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                    break t;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            };
            let wait = std::time::Duration::from_secs(5);
            let mut tls = tokio::time::timeout(
                wait,
                rv_tls::connect_with_roots(tcp, client_config(Fingerprint::Qihoo75, "test.example", Verify::WebPki), &roots(&p)),
            )
            .await
            .unwrap_or_else(|_| panic!("{cipher}: handshake timeout"))
            .unwrap_or_else(|e| panic!("{cipher}: {e}"));
            tls.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
            tls.flush().await.unwrap();
            let mut page = Vec::new();
            let _ = tokio::time::timeout(wait, tls.read_to_end(&mut page)).await;
            let page = String::from_utf8_lossy(&page);
            assert!(page.contains("200 ok") && page.contains(cipher), "{cipher}: {page}");
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[tokio::test]
async fn reality_tls13_fingerprint_rejects_tls12_server() {
    let p = pki(&rcgen::PKCS_ECDSA_P256_SHA256);
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(rprov::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(p.chain.clone(), p.key.clone_key())
        .unwrap();
    let verify =
        Verify::Reality(rv_tls::RealityConfig { public_key: [7; 32], short_id: [0; 8], mldsa65_verify: vec![], spider_x: "/".into() });
    let (client, _) = pair(Arc::new(server), client_config(Fingerprint::Chrome, "test.example", verify), roots(&p)).await;
    assert!(matches!(client, Err(Error::Unsupported("REALITY requires TLS 1.3"))));
}
