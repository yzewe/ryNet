use std::rc::Rc;
use std::sync::Mutex;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::Proxy;

static LOCK: Mutex<()> = Mutex::new(());

fn manifest(extra: Value) -> Value {
    let mut m = json!({
        "id": "dev.rynet.wat", "name": "Wat", "version": "1", "api": 1, "kind": "outbound",
        "protocols": ["watproto"], "settings": [{"key": "address", "type": "host"}, {"key": "port", "type": "port"}]
    });
    for (k, v) in extra.as_object().unwrap() {
        m[k] = v.clone();
    }
    m
}

fn module(body: &str, manifest: &Value) -> Vec<u8> {
    let data: String = manifest.to_string().bytes().map(|b| format!("\\{b:02x}")).collect();
    wat::parse_str(format!(r#"(module (@custom "rynet.manifest" "{data}") {body})"#)).unwrap()
}

fn fresh_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("rynet-plugins-{}-{}", std::process::id(), N.fetch_add(1, Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn profile(protocol: &str, settings: Value) -> rv_config::Profile {
    let text = json!({ "remarks": "p", "outbounds": [{ "tag": "proxy", "protocol": protocol, "settings": settings }] }).to_string();
    rv_config::parse_subscription(&text).unwrap().remove(0)
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    tokio::task::LocalSet::new().block_on(&rt, f)
}

const EXPORTS: &str = r#"(memory (export "memory") 1)"#;

#[test]
fn modules_with_foreign_imports_or_missing_exports_are_refused() {
    let m = manifest(json!({}));
    let ok = module(
        &format!(
            r#"{EXPORTS} (func (export "rn_init") (result i32) i32.const 0) (func (export "rn_event") (param i32 i32 i32) (result i32) i32.const 0)"#
        ),
        &m,
    );
    load(&ok).unwrap();
    let wasi = module(
        &format!(
            r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))) {EXPORTS} (func (export "rn_init") (result i32) i32.const 0) (func (export "rn_event") (param i32 i32 i32) (result i32) i32.const 0)"#
        ),
        &m,
    );
    assert!(load(&wasi).unwrap_err().contains("wasi_snapshot_preview1"));
    let unknown = module(
        &format!(
            r#"(import "rynet" "exec" (func)) {EXPORTS} (func (export "rn_init") (result i32) i32.const 0) (func (export "rn_event") (param i32 i32 i32) (result i32) i32.const 0)"#
        ),
        &m,
    );
    assert!(load(&unknown).unwrap_err().contains("exec"));
    let no_event = module(&format!(r#"{EXPORTS} (func (export "rn_init") (result i32) i32.const 0)"#), &m);
    assert!(load(&no_event).unwrap_err().contains("rn_event"));
    let bare = wat::parse_str(format!(r#"(module {EXPORTS})"#)).unwrap();
    assert!(load(&bare).unwrap_err().contains("not a ryNet plugin"));
}

#[test]
fn rnp_files_install_and_foreign_or_newer_ones_are_refused() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let m = manifest(json!({ "id": "dev.rynet.rnp" }));
    let wasm = module(
        &format!(
            r#"{EXPORTS} (func (export "rn_init") (result i32) i32.const 0) (func (export "rn_event") (param i32 i32 i32) (result i32) i32.const 0)"#
        ),
        &m,
    );
    let (packed_manifest, rnp) = pack(&wasm).unwrap();
    assert_eq!(packed_manifest.id, "dev.rynet.rnp");
    assert!(rnp.starts_with(rv_plugin_abi::FILE_MAGIC) && rnp[7] == rv_plugin_abi::FILE_VERSION && rnp[8..] == wasm[..]);
    assert_eq!(pack(&rnp).unwrap().1, rnp, "packing a packed file changes nothing");

    let dir = fresh_dir();
    assert!(init(&dir).is_empty());
    assert_eq!(inspect(&rnp).unwrap()["manifest"]["id"], "dev.rynet.rnp");
    install(&rnp, Vec::new()).unwrap();
    assert_eq!(std::fs::read(dir.join("dev.rynet.rnp.wasm")).unwrap(), wasm);
    assert!(init(&dir).is_empty());
    assert!(by_id("dev.rynet.rnp").is_some());
    install(&wasm, Vec::new()).unwrap();

    let mut newer = rnp.clone();
    newer[7] = rv_plugin_abi::FILE_VERSION + 1;
    assert!(inspect(&newer).unwrap_err().contains("update ryNet"));
    assert!(inspect(b"PK\x03\x04 not a plugin").unwrap_err().contains(".rnp"));
    let mut torn = rnp.clone();
    torn.remove(4);
    assert!(inspect(&torn).unwrap_err().contains(".rnp"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn install_wat(body: &str, extra: Value) -> Arc<Plugin> {
    let dir = fresh_dir();
    assert!(init(&dir).is_empty());
    let m = manifest(extra);
    install(&module(body, &m), Vec::new()).unwrap();
    by_id(m["id"].as_str().unwrap()).unwrap()
}

#[test]
fn a_plugin_stuck_in_a_loop_runs_out_of_fuel_and_its_connections_fail() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let p = install_wat(
        &format!(
            r#"{EXPORTS} (func (export "rn_init") (result i32) i32.const 0) (func (export "rn_event") (param i32 i32 i32) (result i32) (loop $l (br $l)) i32.const 0)"#
        ),
        json!({}),
    );
    let prof = profile("watproto", json!({ "address": "203.0.113.1", "port": 443 }));
    let t0 = std::time::Instant::now();
    let e = run(async {
        let proxy = Proxy::build(&prof, "proxy", None).unwrap();
        proxy.connect(rv_vless::Address::Domain("example.com".into()), 80).await.err().unwrap().to_string()
    });
    assert!(e.contains("crashed") && e.contains("fuel"), "{e}");
    assert!(t0.elapsed() < Duration::from_secs(10), "fuel stops it quickly: {:?}", t0.elapsed());
    assert_eq!(p.diag.traps.load(Relaxed), 1);
}

use std::time::Duration;

#[test]
fn init_errors_reach_the_user_and_back_off() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let msg = "bad key length";
    let data: String = msg.bytes().map(|b| format!("\\{b:02x}")).collect();
    install_wat(
        &format!(
            r#"(import "rynet" "set_error" (func $se (param i32 i32) (result i32))) {EXPORTS} (data (i32.const 16) "{data}")
            (func (export "rn_init") (result i32) (drop (call $se (i32.const 16) (i32.const {}))) i32.const -6)
            (func (export "rn_event") (param i32 i32 i32) (result i32) i32.const 0)"#,
            msg.len()
        ),
        json!({}),
    );
    let prof = profile("watproto", json!({ "address": "203.0.113.1", "port": 443 }));
    run(async {
        let proxy = Proxy::build(&prof, "proxy", None).unwrap();
        let e = proxy.connect(rv_vless::Address::Domain("example.com".into()), 80).await.err().unwrap().to_string();
        assert!(e.contains(msg), "{e}");
        let again = proxy.connect(rv_vless::Address::Domain("example.com".into()), 80).await.err().unwrap().to_string();
        assert_eq!(e, again, "a failed start is not retried at once");
    });
}

#[test]
fn memory_is_capped_by_the_permission() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let body = format!(
        r#"{EXPORTS} (func (export "rn_init") (result i32) (if (i32.ne (memory.grow (i32.const 1000)) (i32.const -1)) (then unreachable)) (if (i32.eq (memory.grow (i32.const 100)) (i32.const -1)) (then unreachable)) i32.const 0)
        (func (export "rn_event") (param i32 i32 i32) (result i32) i32.const 0)"#
    );
    let p = install_wat(&body, json!({}));
    let ctx = Rc::new(Ctx::offline(p.clone()));
    assert!(instance::Instance::start(ctx, true).is_ok());
    let big = install_wat(&body.replace("1000", "5000"), json!({ "permissions": { "memory_mb": 128 } }));
    assert_eq!(big.permissions().memory_mb, 128);
    set_denied("dev.rynet.wat", vec!["memory".into()]).unwrap();
    assert_eq!(big.permissions().memory_mb, manifest::MEMORY_SAFE_MB, "a denied large memory falls back to the safe cap");
}

#[test]
fn protocols_need_an_enabled_plugin() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    install_wat(
        &format!(
            r#"{EXPORTS} (func (export "rn_init") (result i32) i32.const 0) (func (export "rn_event") (param i32 i32 i32) (result i32) i32.const 0)"#
        ),
        json!({}),
    );
    let prof = profile("watproto", json!({ "address": "203.0.113.1", "port": 443 }));
    assert!(Proxy::build(&prof, "proxy", None).is_ok());
    set_enabled("dev.rynet.wat", false).unwrap();
    let Err(e) = Proxy::build(&prof, "proxy", None) else { panic!("a disabled plugin is not used") };
    assert!(e.to_string().contains("watproto"), "{e}");
    let other = profile("nobodyproto", json!({ "address": "203.0.113.1", "port": 443 }));
    assert!(Proxy::build(&other, "proxy", None).is_err());
    assert!(other.main_outbound().unwrap().kind.is_proxy(), "still a server, just not usable yet");
}

fn testplug() -> Option<Vec<u8>> {
    let path = std::env::var_os("RYNET_TEST_PLUGINS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/target/wasm32-unknown-unknown/release"));
    match std::fs::read(path.join("rynet_testplug.wasm")) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("skipped: plugins are not built");
            None
        }
    }
}

async fn echo_server() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::task::spawn_local(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            tokio::task::spawn_local(async move {
                let mut b = vec![0u8; 65536];
                loop {
                    match s.read(&mut b).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            if s.write_all(&b[..n]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });
    port
}

async fn roundtrip(proxy: &Proxy, port: u16, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut s = proxy.connect(rv_vless::Address::Ipv4([127, 0, 0, 1]), port).await.map_err(|e| e.to_string())?;
    s.write_all(data).await.unwrap();
    let mut got = vec![0u8; data.len()];
    s.read_exact(&mut got).await.map_err(|e| e.to_string())?;
    Ok(got)
}

fn with_testplug(denied: Vec<String>) -> Option<Arc<Plugin>> {
    let bytes = testplug()?;
    let dir = fresh_dir();
    assert!(init(&dir).is_empty());
    let info = inspect(&bytes).unwrap();
    assert_eq!(info["dangerous"], json!(["net_any", "net_lan"]));
    install(&bytes, denied).unwrap();
    by_id("dev.rynet.test")
}

#[test]
fn sdk_plugin_relays_tcp_by_splice_and_by_pumping() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = with_testplug(Vec::new()) else { return };
    run(async {
        let port = echo_server().await;
        let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8 + b'a' % 3).collect();
        for mode in ["splice", "pump"] {
            let prof = profile("rntest", json!({ "address": "127.0.0.1", "port": port, "mode": mode }));
            let proxy = Proxy::build(&prof, "proxy", None).unwrap();
            assert_eq!(roundtrip(&proxy, port, b"hello").await.unwrap(), b"hello", "{mode}");
            assert_eq!(roundtrip(&proxy, port, &big).await.unwrap(), big, "{mode}: 3 MB both ways");
        }
        let prof = profile("rntest", json!({ "address": "127.0.0.1", "port": port, "mode": "upper" }));
        let proxy = Proxy::build(&prof, "proxy", None).unwrap();
        assert_eq!(roundtrip(&proxy, port, b"shout").await.unwrap(), b"SHOUT", "the plugin rewrites data on the way");
        let prof = profile("rntest", json!({ "address": "127.0.0.1", "port": port, "mode": "reject" }));
        let proxy = Proxy::build(&prof, "proxy", None).unwrap();
        assert!(roundtrip(&proxy, port, b"x").await.unwrap_err().contains("rejected on purpose"));
    });
    assert_eq!(p.diag.traps.load(Relaxed), 0);
    assert_eq!(p.diag.conns.load(Relaxed), 6);
    assert_eq!(p.kv_get("started").as_deref(), Some(&b"yes"[..]), "storage works with the permission");
}

#[test]
fn sdk_plugin_relays_udp() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_p) = with_testplug(Vec::new()) else { return };
    run(async {
        let echo = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let eport = echo.local_addr().unwrap().port();
        tokio::task::spawn_local(async move {
            let mut b = [0u8; 2048];
            while let Ok((n, from)) = echo.recv_from(&mut b).await {
                let _ = echo.send_to(&b[..n], from).await;
            }
        });
        let prof = profile("rntest", json!({ "address": "127.0.0.1", "port": eport }));
        let proxy = Proxy::build(&prof, "proxy", None).unwrap();
        assert!(proxy.udp_allowed(53));
        let dst: std::net::SocketAddr = format!("127.0.0.1:{eport}").parse().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let got = std::cell::RefCell::new(Vec::new());
        let reply = |from: std::net::SocketAddr, d: &[u8]| got.borrow_mut().push((from, d.to_vec()));
        let session = proxy.udp_session((dst, b"one".to_vec()), &mut rx, &reply);
        let feed = async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            tx.send((dst, b"two".to_vec())).await.unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            drop(tx);
        };
        let (r, ()) = tokio::join!(session, feed);
        r.unwrap();
        let got = got.into_inner();
        assert_eq!(got, vec![(dst, b"one".to_vec()), (dst, b"two".to_vec())]);
    });
}

#[test]
fn permissions_keep_plugins_off_the_local_network() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_p) = with_testplug(vec!["net_lan".into()]) else { return };
    run(async {
        let port = echo_server().await;
        let prof = profile("rntest", json!({ "address": "localhost", "port": port }));
        let proxy = Proxy::build(&prof, "proxy", None).unwrap();
        let e = roundtrip(&proxy, port, b"x").await.unwrap_err();
        assert!(e.contains("local address"), "{e}");
        let prof = profile("rntest", json!({ "address": "127.0.0.1", "port": port }));
        let proxy = Proxy::build(&prof, "proxy", None).unwrap();
        assert_eq!(roundtrip(&proxy, port, b"ok").await.unwrap(), b"ok");
    });
}

#[test]
fn plugin_links_become_servers() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_p) = with_testplug(Vec::new()) else { return };
    let ps = rv_config::parse_subscription(
        "rntest://srv.example:8443?mode=pump#Mine\nvless://11111111-2222-3333-4444-555555555555@v.example:443#V",
    )
    .unwrap();
    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].remarks, "Mine");
    let ob = ps[0].main_outbound().unwrap();
    assert_eq!(ob.kind.server(), Some(("srv.example", 8443)));
    let rv_config::OutboundKind::Plugin { protocol, settings } = &ob.kind else { panic!("{:?}", ob.kind) };
    assert_eq!((protocol.as_str(), settings["mode"].as_str()), ("rntest", Some("pump")));
    assert!(matches!(ps[1].main_outbound().unwrap().kind, rv_config::OutboundKind::Vless(_)));
}

#[test]
fn a_spinning_sdk_plugin_is_stopped() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = with_testplug(Vec::new()) else { return };
    run(async {
        let prof = profile("rntest", json!({ "address": "127.0.0.1", "port": 9, "mode": "spin" }));
        let proxy = Proxy::build(&prof, "proxy", None).unwrap();
        let e = proxy.connect(rv_vless::Address::Ipv4([127, 0, 0, 1]), 9).await.err().unwrap().to_string();
        assert!(e.contains("fuel"), "{e}");
    });
    assert_eq!(p.diag.traps.load(Relaxed), 1);
    assert!(logs("dev.rynet.test").as_array().unwrap().iter().any(|l| l["message"].as_str().unwrap().contains("fuel")));
}

fn plugin_file(name: &str) -> Option<Vec<u8>> {
    let dir = std::env::var_os("RYNET_TEST_PLUGINS")?;
    std::fs::read(Path::new(&dir).join(format!("rynet_{name}.wasm"))).ok()
}

fn interop(server_env: &str, plugins: &[&str]) -> Option<(String, u16)> {
    let Some(addr) = std::env::var(server_env).ok() else {
        eprintln!("skipped: {server_env} is not set");
        return None;
    };
    let dir = fresh_dir();
    assert!(init(&dir).is_empty());
    for p in plugins {
        install(&plugin_file(p).unwrap_or_else(|| panic!("plugin {p} is not built")), Vec::new()).unwrap();
    }
    let (h, p) = addr.rsplit_once(':').unwrap();
    Some((h.to_string(), p.parse().unwrap()))
}

async fn udp_echo_server() -> u16 {
    let echo = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = echo.local_addr().unwrap().port();
    tokio::task::spawn_local(async move {
        let mut b = vec![0u8; 65536];
        while let Ok((n, from)) = echo.recv_from(&mut b).await {
            let _ = echo.send_to(&b[..n], from).await;
        }
    });
    port
}

async fn udp_roundtrip(proxy: &Proxy, port: u16, sizes: &[usize]) -> Vec<Vec<u8>> {
    udp_roundtrip_to(proxy, format!("127.0.0.1:{port}").parse().unwrap(), sizes).await
}

async fn udp_roundtrip_to(proxy: &Proxy, dst: std::net::SocketAddr, sizes: &[usize]) -> Vec<Vec<u8>> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let got = std::cell::RefCell::new(Vec::new());
    let reply = |_: std::net::SocketAddr, d: &[u8]| got.borrow_mut().push(d.to_vec());
    let payload = |n: usize| (0..n).map(|i| (i * 7 + n) as u8).collect::<Vec<u8>>();
    let session = proxy.udp_session((dst, payload(sizes[0])), &mut rx, &reply);
    let feed = async {
        for &n in &sizes[1..] {
            tokio::time::sleep(Duration::from_millis(100)).await;
            tx.send((dst, payload(n))).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
        drop(tx);
    };
    let (r, ()) = tokio::join!(session, feed);
    r.unwrap();
    let mut got = got.into_inner();
    got.sort_by_key(Vec::len);
    let mut want: Vec<Vec<u8>> = sizes.iter().map(|&n| payload(n)).collect();
    want.sort_by_key(Vec::len);
    assert_eq!(got.len(), want.len(), "every datagram came back");
    assert!(got == want, "datagrams came back intact");
    got
}

#[test]
fn tuic_talks_to_sing_box() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((host, port)) = interop("RYNET_TUIC", &["tuic"]) else { return };
    run(async {
        let echo = echo_server().await;
        let uecho = udp_echo_server().await;
        for mode in ["native", "quic"] {
            let prof = profile(
                "tuic",
                json!({ "address": host, "port": port, "uuid": "8b775467-5bc8-4344-a79e-90d795f324de", "password": "tuic-pass",
                        "sni": "test.local", "insecure": true, "udp_relay_mode": mode }),
            );
            let proxy = Proxy::build(&prof, "proxy", None).unwrap();
            let big: Vec<u8> = (0..4_000_000u32).map(|i| (i % 253) as u8).collect();
            assert_eq!(roundtrip(&proxy, echo, &big).await.unwrap(), big, "{mode}: tcp 4 MB");
            udp_roundtrip(&proxy, uecho, &[10, 1200, 3000, 1]).await;
        }
        let bad = profile(
            "tuic",
            json!({ "address": host, "port": port, "uuid": "8b775467-5bc8-4344-a79e-90d795f324de", "password": "wrong", "insecure": true }),
        );
        let proxy = Proxy::build(&bad, "proxy", None).unwrap();
        assert!(roundtrip(&proxy, echo, b"x").await.is_err(), "a wrong password does not get through");
    });
    let p = by_id("dev.rynet.tuic").unwrap();
    assert_eq!(p.diag.traps.load(Relaxed), 0, "{}", logs("dev.rynet.tuic"));
}

#[test]
fn naive_talks_to_sing_box() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((host, port)) = interop("RYNET_NAIVE", &["naive"]) else { return };
    run(async {
        let echo = echo_server().await;
        for round in 0..2 {
            let prof = profile(
                "naive",
                json!({ "address": host, "port": port, "username": "user", "password": "naive-pass", "sni": "test.local", "insecure": true }),
            );
            let proxy = Proxy::build(&prof, "proxy", None).unwrap();
            let mut s = proxy.connect(rv_vless::Address::Ipv4([127, 0, 0, 1]), echo).await.unwrap();
            for i in 0..20u32 {
                let msg = format!("message {i} {}", "x".repeat(i as usize * 50));
                s.write_all(msg.as_bytes()).await.unwrap();
                let mut got = vec![0u8; msg.len()];
                s.read_exact(&mut got).await.unwrap();
                assert_eq!(got, msg.as_bytes(), "round {round} #{i}");
            }
            drop(s);
            let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 249) as u8).collect();
            assert_eq!(roundtrip(&proxy, echo, &big).await.unwrap(), big, "round {round}: 3 MB");
        }
        let bad = profile("naive", json!({ "address": host, "port": port, "username": "user", "password": "wrong", "insecure": true }));
        let proxy = Proxy::build(&bad, "proxy", None).unwrap();
        let e = roundtrip(&proxy, echo, b"x").await.unwrap_err();
        assert!(e.contains("407") || e.contains("password"), "{e}");
    });
    let p = by_id("dev.rynet.naive").unwrap();
    assert_eq!(p.diag.traps.load(Relaxed), 0, "{}", logs("dev.rynet.naive"));
}

#[test]
fn amneziawg_talks_to_amneziawg_go() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(cfg) = std::env::var("RYNET_AWG") else {
        eprintln!("skipped: RYNET_AWG is not set");
        return;
    };
    let cfg: Value = serde_json::from_str(&cfg).unwrap();
    let dir = fresh_dir();
    assert!(init(&dir).is_empty());
    install(&plugin_file("awg").expect("plugin awg is not built"), Vec::new()).unwrap();
    let amnezia = json!({
        "jc": 4, "jmin": 40, "jmax": 70, "s1": 15, "s2": 18, "s3": 20, "s4": 23,
        "h1": "123456-123500", "h2": "223456-223500", "h3": "323456-323500", "h4": "423456-423500",
        "i1": "<b 0xc0ffee><r 10><t>", "header_protection_key": cfg["hp_key"],
    });
    run(async {
        for (proto, port, extra) in [("amneziawg", &cfg["awg_port"], amnezia.clone()), ("wireguard", &cfg["wg_port"], json!({}))] {
            let mut settings = json!({
                "server": cfg["server"], "server_port": port, "private_key": cfg["client_private"], "peer_public_key": cfg["server_public"],
                "pre_shared_key": cfg["psk"], "local_address": ["10.7.0.2/32"], "dns": ["10.7.0.1"],
            });
            for (k, v) in extra.as_object().unwrap() {
                settings[k] = v.clone();
            }
            let proxy = Proxy::build(&profile(proto, settings), "proxy", None).unwrap();
            let t0 = std::time::Instant::now();
            let s = proxy
                .connect(rv_vless::Address::Domain("echo.test".into()), 7)
                .await
                .unwrap_or_else(|e| panic!("{proto}: {e}\n{}", logs("dev.rynet.amneziawg")));
            eprintln!("{proto}: connected in {:?}", t0.elapsed());
            let t0 = std::time::Instant::now();
            let (mut r, mut w) = tokio::io::split(s);
            let big: Vec<u8> = (0..2_000_000u32).map(|i| (i % 241) as u8).collect();
            let wr = async {
                w.write_all(&big).await.unwrap();
            };
            let rd = async {
                let mut got = vec![0u8; big.len()];
                r.read_exact(&mut got).await.unwrap();
                got
            };
            let ((), got) = tokio::join!(wr, rd);
            assert!(got == big, "{proto}: tcp 2 MB came back intact");
            eprintln!("{proto}: 2 MB echo in {:?}", t0.elapsed());
            udp_roundtrip_to(&proxy, "10.7.0.1:9".parse().unwrap(), &[10, 1200, 1, 300]).await;
        }
    });
    let p = by_id("dev.rynet.amneziawg").unwrap();
    assert_eq!(p.diag.traps.load(Relaxed), 0, "{}", logs("dev.rynet.amneziawg"));
}

async fn obfs_echo_server(tls: bool) -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::task::spawn_local(async move {
        while let Ok((s, _)) = l.accept().await {
            tokio::task::spawn_local(async move {
                let (mut r, mut w) = s.into_split();
                let first = if tls {
                    let mut h = [0u8; 5];
                    r.read_exact(&mut h).await.unwrap();
                    assert_eq!(&h[..3], &[22, 3, 1]);
                    let mut hello = vec![0u8; usize::from(u16::from_be_bytes([h[3], h[4]]))];
                    r.read_exact(&mut hello).await.unwrap();
                    let mut at = 4 + 2 + 32;
                    at += 1 + usize::from(hello[at]);
                    at += 2 + usize::from(u16::from_be_bytes([hello[at], hello[at + 1]]));
                    at += 1 + usize::from(hello[at]);
                    let end = at + 2 + usize::from(u16::from_be_bytes([hello[at], hello[at + 1]]));
                    at += 2;
                    let mut ticket = None;
                    while at < end {
                        let ty = u16::from_be_bytes([hello[at], hello[at + 1]]);
                        let len = usize::from(u16::from_be_bytes([hello[at + 2], hello[at + 3]]));
                        if ty == 0x23 {
                            ticket = Some(hello[at + 4..at + 4 + len].to_vec());
                        }
                        at += 4 + len;
                    }
                    let first = ticket.expect("data in the session ticket");
                    let mut resp = vec![0x16, 3, 3, 0, 91];
                    resp.extend_from_slice(&[0x02; 91]);
                    resp.extend_from_slice(&[0x14, 3, 3, 0, 1, 1]);
                    resp.extend_from_slice(&[0x16, 3, 3]);
                    resp.extend_from_slice(&(first.len() as u16).to_be_bytes());
                    resp.extend_from_slice(&first);
                    w.write_all(&resp).await.unwrap();
                    first
                } else {
                    let mut head = Vec::new();
                    let mut b = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        r.read_exact(&mut b).await.unwrap();
                        head.push(b[0]);
                    }
                    let head = String::from_utf8(head).unwrap();
                    assert!(head.starts_with("GET / HTTP/1.1\r\nHost: www.bing.com:") && head.contains("Upgrade: websocket"), "{head}");
                    let len: usize = head.lines().find_map(|l| l.strip_prefix("Content-Length: ")).unwrap().trim().parse().unwrap();
                    let mut body = vec![0u8; len];
                    r.read_exact(&mut body).await.unwrap();
                    let mut resp =
                        b"HTTP/1.1 101 Switching Protocols\r\nServer: nginx/1.18.0\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n"
                            .to_vec();
                    resp.extend_from_slice(&body);
                    w.write_all(&resp).await.unwrap();
                    body
                };
                drop(first);
                let mut buf = vec![0u8; 65536];
                loop {
                    if tls {
                        let mut h = [0u8; 5];
                        if r.read_exact(&mut h).await.is_err() {
                            return;
                        }
                        assert_eq!(&h[..3], &[0x17, 3, 3]);
                        let mut d = vec![0u8; usize::from(u16::from_be_bytes([h[3], h[4]]))];
                        r.read_exact(&mut d).await.unwrap();
                        let mut o = vec![0x17, 3, 3];
                        o.extend_from_slice(&(d.len() as u16).to_be_bytes());
                        o.extend_from_slice(&d);
                        if w.write_all(&o).await.is_err() {
                            return;
                        }
                    } else {
                        match r.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => {
                                if w.write_all(&buf[..n]).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            });
        }
    });
    port
}

#[test]
fn simple_obfs_is_a_stream_transport_for_any_protocol() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(wasm) = plugin_file("obfs") else { return };
    let dir = fresh_dir();
    assert!(init(&dir).is_empty());
    install(&wasm, Vec::new()).unwrap();
    run(async {
        for mode in ["http", "tls"] {
            let port = obfs_echo_server(mode == "tls").await;
            let text = json!({ "outbounds": [{ "tag": "proxy", "protocol": "shadowsocks",
                "settings": { "servers": [{ "address": "127.0.0.1", "port": port, "method": "aes-128-gcm", "password": "x" }] },
                "streamSettings": { "network": "obfs-local", "obfs-localSettings": { "obfs": mode, "obfs-host": "www.bing.com" } } }] })
            .to_string();
            let prof = rv_config::parse_subscription(&text).unwrap().remove(0);
            let ob = prof.outbounds[0].clone();
            let t = crate::transport::Stream::new(&prof, &ob, "127.0.0.1", port, None).unwrap();
            let s = t.open().await.unwrap_or_else(|e| panic!("{mode}: {e}"));
            let (mut r, mut w) = tokio::io::split(s);
            let big: Vec<u8> = (0..1_000_000u32).map(|i| (i % 239) as u8).collect();
            let wr = async { w.write_all(&big).await.unwrap() };
            let rd = async {
                let mut got = vec![0u8; big.len()];
                r.read_exact(&mut got).await.unwrap();
                got
            };
            let ((), got) = tokio::join!(wr, rd);
            assert!(got == big, "{mode}: 1 MB came back intact");
        }
        let text = json!({ "outbounds": [{ "tag": "proxy", "protocol": "shadowsocks",
            "settings": { "servers": [{ "address": "127.0.0.1", "port": 1, "method": "aes-128-gcm", "password": "x" }] },
            "streamSettings": { "network": "kcp" } }] })
        .to_string();
        let prof = rv_config::parse_subscription(&text).unwrap().remove(0);
        let e = crate::transport::Stream::new(&prof, &prof.outbounds[0], "127.0.0.1", 1, None).err().unwrap().to_string();
        assert!(e.contains("kcp"), "{e}");
    });
    let p = by_id("dev.rynet.simple-obfs").unwrap();
    assert_eq!(p.diag.traps.load(Relaxed), 0, "{}", logs("dev.rynet.simple-obfs"));
}

#[test]
fn wireguard_rides_a_turn_relay() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (Ok(awg), Ok(turn)) = (std::env::var("RYNET_AWG"), std::env::var("RYNET_TURN")) else {
        eprintln!("skipped: RYNET_AWG and RYNET_TURN are needed");
        return;
    };
    let awg: Value = serde_json::from_str(&awg).unwrap();
    let turn: Value = serde_json::from_str(&turn).unwrap();
    let dir = fresh_dir();
    assert!(init(&dir).is_empty());
    for p in ["awg", "turn"] {
        install(&plugin_file(p).unwrap_or_else(|| panic!("plugin {p} is not built")), Vec::new()).unwrap();
    }
    let (turn_host, turn_port) = turn["turn"].as_str().unwrap().rsplit_once(':').unwrap();
    let (dtls_host, dtls_port) = turn["peer"].as_str().unwrap().rsplit_once(':').unwrap();
    run(async {
        for (dtls, server, port) in [
            (true, dtls_host.to_string(), dtls_port.parse::<u16>().unwrap()),
            (false, awg["server"].as_str().unwrap().to_string(), awg["wg_port"].as_u64().unwrap() as u16),
        ] {
            for over in ["udp", "tcp"] {
                let text = json!({ "outbounds": [{ "tag": "proxy", "protocol": "wireguard",
                    "settings": { "server": server, "server_port": port, "private_key": awg["client_private"], "peer_public_key": awg["server_public"],
                                  "pre_shared_key": awg["psk"], "local_address": ["10.7.0.2/32"], "dns": ["10.7.0.1"] },
                    "streamSettings": { "network": "turn", "turnSettings": { "turn_host": turn_host, "turn_port": turn_port, "username": turn["user"],
                                        "password": turn["pass"], "turn_transport": over, "dtls": dtls } } }] })
                .to_string();
                let prof = rv_config::parse_subscription(&text).unwrap().remove(0);
                let proxy = Proxy::build(&prof, "proxy", None).unwrap();
                let t0 = std::time::Instant::now();
                let what = format!("dtls={dtls} over {over}");
                let s = tokio::time::timeout(Duration::from_secs(40), proxy.connect(rv_vless::Address::Domain("echo.test".into()), 7))
                    .await
                    .unwrap_or_else(|_| panic!("{what}: connect hangs\n{}\n{}", logs("dev.rynet.turn"), logs("dev.rynet.amneziawg")))
                    .unwrap_or_else(|e| panic!("{what}: {e}\n{}\n{}", logs("dev.rynet.turn"), logs("dev.rynet.amneziawg")));
                let (mut r, mut w) = tokio::io::split(s);
                let big: Vec<u8> = (0..1_000_000u32).map(|i| (i % 233) as u8).collect();
                let wr = async { w.write_all(&big).await.unwrap() };
                let rd = async {
                    let mut got = vec![0u8; big.len()];
                    r.read_exact(&mut got).await.unwrap();
                    got
                };
                let ((), got) = tokio::time::timeout(Duration::from_secs(40), async { tokio::join!(wr, rd) })
                    .await
                    .unwrap_or_else(|_| panic!("{what}: echo hangs\n{}\n{}", logs("dev.rynet.turn"), logs("dev.rynet.amneziawg")));
                assert!(got == big, "{what}: 1 MB came back intact");
                eprintln!("{what}: connect + 1 MB echo in {:?}", t0.elapsed());
            }
        }
    });
    for id in ["dev.rynet.turn", "dev.rynet.amneziawg"] {
        assert_eq!(by_id(id).unwrap().diag.traps.load(Relaxed), 0, "{}", logs(id));
    }
}
