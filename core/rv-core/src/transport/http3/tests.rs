use super::*;
use rv_xhttp::{h3, request::Config, transport::Transport};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn varint(r: &mut quinn::RecvStream) -> io::Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b[..1]).await.map_err(io::Error::other)?;
    let n = 1 << (b[0] >> 6);
    r.read_exact(&mut b[1..n]).await.map_err(io::Error::other)?;
    Ok(h3::get_varint(&b[..n]).unwrap().0)
}
async fn frame(r: &mut quinn::RecvStream) -> io::Result<(u64, Vec<u8>)> {
    let t = varint(r).await?;
    let n = varint(r).await?;
    assert!(n < 10 << 20);
    let mut b = vec![0; n as usize];
    r.read_exact(&mut b).await.map_err(io::Error::other)?;
    Ok((t, b))
}
async fn response(s: &mut quinn::SendStream, fields: &[(&str, &str)]) {
    let fields = fields.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect::<Vec<_>>();
    s.write_all(&h3::frame(h3::HEADERS, &h3::encode(&fields))).await.unwrap();
}
struct Session {
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
    pending: BTreeMap<u64, Vec<u8>>,
    next: u64,
}
impl Default for Session {
    fn default() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self { tx, rx: Some(rx), pending: BTreeMap::new(), next: 0 }
    }
}

static UPLOADED: AtomicUsize = AtomicUsize::new(0);
static CLOSED_WITH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

async fn peer(behaviour: u8) -> (quinn::Endpoint, Arc<AtomicUsize>) {
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![include_bytes!("../../../tests/fixtures/h3-cert.der").to_vec().into()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(include_bytes!("../../../tests/fixtures/h3-key.der").to_vec().into()),
        )
        .unwrap();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let mut cfg = quinn::ServerConfig::with_crypto(Arc::new(quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap()));
    let transport = Arc::get_mut(&mut cfg.transport).unwrap();
    transport.stream_receive_window((65536u32).into()).receive_window((131072u32).into());
    if behaviour == 10 {
        transport.max_concurrent_bidi_streams(1u32.into());
    }
    let endpoint = quinn::Endpoint::server(cfg, "127.0.0.1:0".parse().unwrap()).unwrap();
    let e = endpoint.clone();
    let connections = Arc::new(AtomicUsize::new(0));
    let count = connections.clone();
    let sessions = Rc::new(RefCell::new(HashMap::<String, Session>::new()));
    tokio::task::spawn_local(async move {
        while let Some(incoming) = e.accept().await {
            let c = incoming.await.unwrap();
            count.fetch_add(1, Ordering::Relaxed);
            let sessions = sessions.clone();
            tokio::task::spawn_local(async move {
                let mut control = c.open_uni().await.unwrap();
                if behaviour == 9 {
                    control.write_all(&[0, 4, 0x80, 0, 0x20, 1]).await.unwrap();
                    if let quinn::ConnectionError::ApplicationClosed(a) = c.closed().await {
                        CLOSED_WITH.store(a.error_code.into_inner(), Ordering::Relaxed);
                    }
                    return;
                }
                control.write_all(&[0, 4, 0]).await.unwrap();
                let control = Rc::new(tokio::sync::Mutex::new(control));
                let cc = c.clone();
                tokio::task::spawn_local(async move {
                    let mut r = cc.accept_uni().await.unwrap();
                    assert_eq!(varint(&mut r).await.unwrap(), 0);
                    let (t, b) = frame(&mut r).await.unwrap();
                    assert_eq!(t, 4);
                    assert_eq!(b, [6, 0x80, 0xa0, 0, 0]);
                    let _ = cc.closed().await;
                });
                while let Ok((mut s, mut r)) = c.accept_bi().await {
                    let sessions = sessions.clone();
                    let control = control.clone();
                    tokio::task::spawn_local(async move {
                        let (t, b) = frame(&mut r).await.unwrap();
                        assert_eq!(t, h3::HEADERS);
                        let headers = h3::decode(&b).unwrap();
                        assert_eq!(
                            headers.iter().take(4).map(|h| h.0.as_str()).collect::<Vec<_>>(),
                            [":authority", ":method", ":path", ":scheme"]
                        );
                        let method = headers.iter().find(|h| h.0 == ":method").unwrap().1.as_str();
                        let path = headers.iter().find(|h| h.0 == ":path").unwrap().1.split('?').next().unwrap();
                        if behaviour == 1 {
                            response(&mut s, &[(":status", "103")]).await;
                            response(&mut s, &[(":status", "200"), ("content-length", "5")]).await;
                            s.write_all(&h3::frame(0x21, &[1, 2, 3])).await.unwrap();
                            s.write_all(&h3::frame(h3::DATA, b"hello")).await.unwrap();
                            response(&mut s, &[("x-trailer", "value")]).await;
                            s.finish().unwrap();
                            return;
                        }
                        if behaviour == 2 {
                            s.reset(0x10cu32.into()).unwrap();
                            return;
                        }
                        if behaviour == 3 {
                            s.write_all(&h3::frame(h3::HEADERS, &[0, 0, 0x80])).await.unwrap();
                            s.finish().unwrap();
                            return;
                        }
                        if behaviour == 4 {
                            response(&mut s, &[(":status", "200"), ("content-length", "6")]).await;
                            s.write_all(&h3::frame(h3::DATA, b"hello")).await.unwrap();
                            s.finish().unwrap();
                            return;
                        }
                        if behaviour == 5 {
                            let gz = vec![31, 139, 8, 0, 0, 0, 0, 0, 0, 3, 203, 72, 205, 201, 201, 7, 0, 134, 166, 16, 54, 5, 0, 0, 0];
                            response(&mut s, &[(":status", "200"), ("content-encoding", "gzip")]).await;
                            s.write_all(&h3::frame(h3::DATA, &gz)).await.unwrap();
                            s.finish().unwrap();
                            return;
                        }
                        if behaviour == 6 {
                            response(&mut s, &[(":status", "200")]).await;
                            control.lock().await.write_all(&[7, 1, 4]).await.unwrap();
                            s.write_all(&h3::frame(h3::DATA, b"hello")).await.unwrap();
                            s.finish().unwrap();
                            return;
                        }
                        if behaviour == 7 {
                            control.lock().await.write_all(&[4, 0]).await.unwrap();
                            let _ = r.read_to_end(1024).await;
                            return;
                        }
                        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
                        if parts.len() == 1 {
                            response(&mut s, &[(":status", "200")]).await;
                            while let Ok((typ, b)) = frame(&mut r).await {
                                assert_eq!(typ, h3::DATA);
                                if s.write_all(&h3::frame(h3::DATA, &b)).await.is_err() {
                                    break;
                                }
                            }
                            let _ = s.finish();
                        } else if method == "GET" {
                            let mut rx = sessions.borrow_mut().entry(parts[1].into()).or_default().rx.take().unwrap();
                            response(&mut s, &[(":status", "200")]).await;
                            while let Some(b) = rx.recv().await {
                                if s.write_all(&h3::frame(h3::DATA, &b)).await.is_err() {
                                    break;
                                }
                            }
                        } else {
                            let sid = parts[1].to_string();
                            let tx = sessions.borrow_mut().entry(sid.clone()).or_default().tx.clone();
                            let mut body = Vec::new();
                            while let Ok((typ, b)) = frame(&mut r).await {
                                assert_eq!(typ, h3::DATA);
                                if parts.len() == 3 {
                                    body.extend(b);
                                } else {
                                    let _ = tx.send(b);
                                }
                            }
                            if parts.len() == 3 {
                                if behaviour == 8 {
                                    UPLOADED.fetch_add(body.len(), Ordering::Relaxed);
                                }
                                let seq: u64 = parts[2].parse().unwrap();
                                let mut all = sessions.borrow_mut();
                                let session = all.get_mut(&sid).unwrap();
                                session.pending.insert(seq, body);
                                while let Some(b) = session.pending.remove(&session.next) {
                                    let _ = session.tx.send(b);
                                    session.next += 1;
                                }
                            }
                            response(&mut s, &[(":status", "200")]).await;
                            let _ = s.finish();
                        }
                    });
                }
                drop(control);
            });
        }
    });
    (endpoint, connections)
}

fn client(port: u16, mode: &str, protects: Arc<AtomicUsize>) -> Rc<Transport> {
    let settings = serde_json::json!({"mode":mode,"path":"/test","scMinPostsIntervalMs":0,"scMaxEachPostBytes":4096,"xPaddingBytes":0,
        "headers":{"User-Agent":"test"},"xmux":{"cMaxReuseTimes":3,"hMaxRequestTimes":4,"hKeepAlivePeriod":-1}});
    let profile=rv_config::parse_subscription(&serde_json::json!({"outbounds":[{"protocol":"vless","tag":"proxy",
        "settings":{"address":"127.0.0.1","port":port,"id":"9205c503-358f-4d8d-a081-20c98eddf64a","encryption":"none"},
        "streamSettings":{"network":"xhttp","security":"tls","tlsSettings":{"serverName":"test.local","alpn":["h3"],"allowInsecure":true},"xhttpSettings":settings}}]}).to_string()).unwrap().remove(0);
    let cfg = Config::from_json(&settings).unwrap();
    let protect: Protect = Arc::new(move |_| {
        protects.fetch_add(1, Ordering::Relaxed);
        Ok(())
    });
    let (dial, keepalive) = connector(profile.main_outbound().unwrap(), "127.0.0.1", port, Some(protect), -1).unwrap();
    assert_eq!(keepalive, None);
    Transport::new_h3(cfg, "test.local", "127.0.0.1", dial, None, keepalive)
}

#[tokio::test(flavor = "current_thread")]
async fn packet_up_queue_survives_drop_after_shutdown() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (endpoint, _) = peer(8).await;
            let t = client(endpoint.local_addr().unwrap().port(), "packet-up", Arc::new(AtomicUsize::new(0)));
            let mut s = t.open().await.unwrap();
            let data = vec![7u8; 65536];
            s.write_all(&data).await.unwrap();
            s.shutdown().await.unwrap();
            drop(s);
            let sent = tokio::time::timeout(Duration::from_secs(5), async {
                while UPLOADED.load(Ordering::Relaxed) < data.len() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
            assert!(sent.is_ok(), "uploaded {} of {}", UPLOADED.load(Ordering::Relaxed), data.len());
            endpoint.close(0u32.into(), b"");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn local_modes_stream_both_directions_rotate_and_protect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for mode in ["packet-up", "stream-up", "stream-one", "auto"] {
                let (endpoint, connections) = peer(0).await;
                let protects = Arc::new(AtomicUsize::new(0));
                let t = client(endpoint.local_addr().unwrap().port(), mode, protects.clone());
                for _ in 0..3 {
                    let mut s = t.open().await.unwrap();
                    let data: Vec<_> = (0..262144).map(|i| (i % 251) as u8).collect();
                    let mut back = vec![0; data.len()];
                    tokio::time::timeout(Duration::from_secs(8), async {
                        let (mut r, mut w) = tokio::io::split(&mut s);
                        let (a, b) = tokio::join!(
                            async {
                                w.write_all(&data).await?;
                                w.flush().await
                            },
                            r.read_exact(&mut back)
                        );
                        a?;
                        b?;
                        io::Result::Ok(())
                    })
                    .await
                    .unwrap()
                    .unwrap();
                    assert_eq!(back, data, "{mode}");
                    t.reset();
                    s.write_all(b"still alive").await.unwrap();
                    s.flush().await.unwrap();
                    let mut b = [0; 11];
                    s.read_exact(&mut b).await.unwrap();
                    assert_eq!(&b, b"still alive");
                }
                assert!(connections.load(Ordering::Relaxed) >= 3);
                assert_eq!(protects.load(Ordering::Relaxed), connections.load(Ordering::Relaxed));
                endpoint.close(0u32.into(), b"");
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn informational_trailers_gzip_and_errors() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for behaviour in 1..=7 {
                let (endpoint, connections) = peer(behaviour).await;
                let t = client(endpoint.local_addr().unwrap().port(), "stream-one", Arc::new(AtomicUsize::new(0)));
                let mut s = t.open().await.unwrap();
                let mut back = Vec::new();
                let r = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut back)).await.unwrap();
                match behaviour {
                    1 | 5 | 6 => {
                        r.unwrap();
                        assert_eq!(back, b"hello");
                    }
                    7 => assert_eq!(r.unwrap_err().kind(), io::ErrorKind::ConnectionAborted),
                    2 => assert_eq!(r.unwrap_err().kind(), io::ErrorKind::ConnectionReset),
                    3 => assert!(r.unwrap_err().to_string().contains("QPACK")),
                    4 => assert_eq!(r.unwrap_err().kind(), io::ErrorKind::InvalidData),
                    _ => unreachable!(),
                }
                if behaviour == 6 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    let mut next = t.open().await.unwrap();
                    let mut data = Vec::new();
                    next.read_to_end(&mut data).await.unwrap();
                    assert_eq!(data, b"hello");
                    assert_eq!(connections.load(Ordering::Relaxed), 2, "GOAWAY must rotate the connection");
                }
                endpoint.close(0u32.into(), b"");
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_settings_close_the_connection() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (endpoint, _) = peer(9).await;
            let t = client(endpoint.local_addr().unwrap().port(), "stream-one", Arc::new(AtomicUsize::new(0)));
            let _s = t.open().await;
            let closed = tokio::time::timeout(Duration::from_secs(5), async {
                while CLOSED_WITH.load(Ordering::Relaxed) == 0 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
            assert!(closed.is_ok(), "клиент ждёт тело SETTINGS на 8193 байта");
            assert_eq!(CLOSED_WITH.load(Ordering::Relaxed), 0x109);
            endpoint.close(0u32.into(), b"");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn server_stream_limit_sends_the_next_stream_to_a_new_connection() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (endpoint, connections) = peer(10).await;
            let t = client(endpoint.local_addr().unwrap().port(), "stream-one", Arc::new(AtomicUsize::new(0)));
            let mut first = t.open().await.unwrap();
            first.write_all(b"one").await.unwrap();
            first.flush().await.unwrap();
            let mut b = [0; 3];
            first.read_exact(&mut b).await.unwrap();
            let second = tokio::time::timeout(Duration::from_secs(10), async {
                let mut s = t.open().await.unwrap();
                s.write_all(b"two").await.unwrap();
                s.flush().await.unwrap();
                let mut b = [0; 3];
                s.read_exact(&mut b).await.unwrap();
                b
            })
            .await;
            assert_eq!(second.expect("второй поток ждёт кредита сервера без срока"), *b"two");
            assert_eq!(connections.load(Ordering::Relaxed), 2);
            drop(first);
            endpoint.close(0u32.into(), b"");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn idle_h3_connection_with_keepalive_closes_after_three_periods() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (endpoint, connections) = peer(0).await;
            let port = endpoint.local_addr().unwrap().port();
            let settings = serde_json::json!({"mode":"stream-one","path":"/test","xPaddingBytes":0,
                "headers":{"User-Agent":"test"},"xmux":{"hKeepAlivePeriod":1}});
            let profile=rv_config::parse_subscription(&serde_json::json!({"outbounds":[{"protocol":"vless","tag":"proxy",
                "settings":{"address":"127.0.0.1","port":port,"id":"9205c503-358f-4d8d-a081-20c98eddf64a","encryption":"none"},
                "streamSettings":{"network":"xhttp","security":"tls","tlsSettings":{"serverName":"test.local","alpn":["h3"],"allowInsecure":true},"xhttpSettings":settings}}]}).to_string()).unwrap().remove(0);
            let cfg = Config::from_json(&settings).unwrap();
            let (dial, keepalive) = connector(profile.main_outbound().unwrap(), "127.0.0.1", port, None, 1).unwrap();
            assert_eq!(keepalive, Some(Duration::from_secs(1)));
            let t = Transport::new_h3(cfg, "test.local", "127.0.0.1", dial, None, keepalive);
            for _ in 0..2 {
                let mut s = t.open().await.unwrap();
                s.write_all(b"ping").await.unwrap();
                s.flush().await.unwrap();
                let mut b = [0; 4];
                tokio::time::timeout(Duration::from_secs(5), s.read_exact(&mut b)).await.unwrap().unwrap();
                assert_eq!(&b, b"ping");
                drop(s);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            assert_eq!(connections.load(Ordering::Relaxed), 1, "внутри окна соединение переиспользуется");
            let closed = tokio::time::timeout(Duration::from_secs(8), async {
                while endpoint.open_connections() > 0 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await;
            assert!(closed.is_ok(), "пустое h3-соединение с keepalive всё ещё открыто");
            endpoint.close(0u32.into(), b"");
        })
        .await;
}
