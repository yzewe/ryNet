#![cfg_attr(windows, windows_subsystem = "windows")]

use rv_desktop::{core, cover, fetch, logfile, store, tray, tun};

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

static OUT: Mutex<Option<Box<dyn Write + Send>>> = Mutex::new(None);

pub fn emit(v: Value) {
    let mut g = OUT.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_mut() {
        Some(w) => {
            let _ = writeln!(w, "{v}");
            let _ = w.flush();
        }
        None => {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{v}");
            let _ = out.flush();
        }
    }
}

fn listen() -> std::io::Result<Box<dyn BufRead>> {
    use std::net::{TcpListener, TcpStream};
    let dir = store::config_dir();
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let mut raw = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut raw).map_err(|_| std::io::Error::other("rand"))?;
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let file = dir.join("core.port");
    store::write_private(&file, format!("{} {token}", listener.local_addr()?.port()).as_bytes())?;
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let accepted = loop {
        match listener.accept() {
            Ok((s, _peer)) => {
                #[cfg(windows)]
                if !rv_desktop::owner::granted_client(_peer) {
                    continue;
                }
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut line = Vec::new();
                let mut r = (&s).take(128);
                let mut b = [0u8; 1];
                while r.read(&mut b).unwrap_or(0) == 1 && b[0] != b'\n' {
                    line.push(b[0]);
                }
                if line.strip_suffix(b"\r").unwrap_or(&line) == token.as_bytes() {
                    let _ = s.set_read_timeout(None);
                    break s;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    let _ = std::fs::remove_file(&file);
                    return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "no client"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    };
    let _ = std::fs::remove_file(&file);
    let _ = accepted.set_nodelay(true);
    let writer: TcpStream = accepted.try_clone()?;
    *OUT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(writer));
    Ok(Box::new(BufReader::new(accepted)))
}

fn load_plugins() {
    for e in rv_core::plugin::init(&store::plugins_dir()) {
        tracing::warn!(target: "rv::core", "plugin: {e}");
    }
}

fn main() {
    rv_desktop::set_emit(emit);
    rv_core::lang::set_english(std::env::var("RYNET_LANG").is_ok_and(|v| v == "en"));
    if std::env::args().any(|a| a == "--tray") {
        tray::run();
        return;
    }
    let listening = std::env::args().any(|a| a == "--listen");
    if listening && std::fs::read_to_string(store::config_dir().join("core.lang")).is_ok_and(|v| v.trim() == "en") {
        rv_core::lang::set_english(true);
    }
    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            let debug = listening && store::config_dir().join("core.debug").exists();
            (if debug { "info,rv=debug" } else { "info" }).into()
        })
    };
    if listening {
        let path = store::config_dir().join("core.log");
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&path);
        match log {
            Ok(f) => tracing_subscriber::fmt()
                .with_writer(Mutex::new(logfile::LogFile::new(f, path)))
                .with_ansi(false)
                .with_env_filter(filter())
                .init(),
            Err(_) => tracing_subscriber::fmt().with_writer(std::io::sink).with_env_filter(filter()).init(),
        }
    } else if let Some(log) = logfile::LogFile::stderr() {
        tracing_subscriber::fmt().with_writer(Mutex::new(log)).with_ansi(false).with_env_filter(filter()).init();
    } else {
        tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false).with_env_filter(filter()).init();
    }
    #[cfg(windows)]
    if listening {
        tun::single_instance();
    }
    let input: Box<dyn BufRead> = if listening {
        match listen() {
            Ok(i) => i,
            Err(e) => {
                tracing::error!("listen: {e}");
                return;
            }
        }
    } else {
        Box::new(BufReader::new(std::io::stdin()))
    };
    let core = core::spawn();
    load_plugins();
    fetch::ensure_geo();
    #[cfg(unix)]
    let blocked = match tun::leftover() {
        Some(true) => {
            tracing::warn!(target: "rv::core", "a kill switch left by a stopped core still blocks traffic");
            true
        }
        Some(false) => {
            tracing::info!(target: "rv::core", "cleared routes left by a stopped core");
            tun::down();
            false
        }
        None => false,
    };
    #[cfg(windows)]
    let blocked = {
        tun::down();
        false
    };
    emit(
        json!({ "ev": "ready", "version": env!("CARGO_PKG_VERSION"), "tunCapable": tun::capable(), "elevated": listening, "blocked": blocked }),
    );
    for line in input.lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let n = |k: &str, d: u64| v.get(k).and_then(Value::as_u64).unwrap_or(d);
        let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        let req = v.get("req").cloned().unwrap_or(Value::Null);
        match s("cmd").as_str() {
            "start" => {
                load_plugins();
                let _ = core.send(core::Cmd::Start {
                    text: s("text"),
                    profile: s("profile"),
                    options: v.get("options").cloned().unwrap_or_else(|| json!({})),
                    tun: b("tun"),
                    port: u16::try_from(n("port", 10808)).unwrap_or(10808),
                    kill_switch: b("killSwitch"),
                    mtu: (n("mtu", 9000) as usize).clamp(1280, 65535),
                });
            }
            "stop" => {
                let _ = core.send(core::Cmd::Stop);
            }
            "block" => {
                let _ = core.send(core::Cmd::Block);
            }
            "network" => {
                let _ = core.send(core::Cmd::Network);
            }
            "stats" => {
                let _ = core.send(core::Cmd::Stats(v.get("on").and_then(Value::as_bool).unwrap_or(true)));
            }
            "ping" => {
                let names = v
                    .get("names")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                let _ = core.send(core::Cmd::Ping {
                    generation: n("generation", 0),
                    text: s("text"),
                    names,
                    mode: n("mode", 1) as i32,
                    parallel: n("parallel", 16) as usize,
                    timeout_ms: n("timeoutMs", 5000),
                    url: s("url"),
                    tune: v.get("tune").cloned().unwrap_or_else(|| json!({})),
                });
            }
            "countries" => {
                let names = v
                    .get("names")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                let _ = core.send(core::Cmd::Countries {
                    text: s("text"),
                    names,
                    parallel: n("parallel", 16) as usize,
                    timeout_ms: n("timeoutMs", 5000),
                    tune: v.get("tune").cloned().unwrap_or_else(|| json!({})),
                    req: req.clone(),
                });
            }
            "speed" => core::speedtest(s("text"), s("profile"), v.get("options").cloned().unwrap_or_else(|| json!({}))),
            "speedCancel" => core::SPEED_CANCEL.store(true, std::sync::atomic::Ordering::Relaxed),
            "pingCancel" => core::PING_CANCEL.store(true, std::sync::atomic::Ordering::Relaxed),
            "probe" => {
                let _ = core.send(core::Cmd::Probe {
                    text: s("text"),
                    profile: s("profile"),
                    options: v.get("options").cloned().unwrap_or_else(|| json!({})),
                    req,
                });
            }
            "direct" => {
                let _ = core.send(core::Cmd::Direct { url: s("url"), timeout_ms: n("timeoutMs", 5000), req });
            }
            "fetch" => {
                let (url, sub) = (s("url"), v.get("sub").and_then(Value::as_str).map(str::to_string));
                let client = fetch::Client {
                    agent: Some(s("agent")).filter(|a| !a.is_empty()),
                    headers: v
                        .get("headers")
                        .and_then(Value::as_object)
                        .map(|h| h.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect())
                        .unwrap_or_default(),
                    hwid: (v.get("sendHwid").and_then(Value::as_bool) != Some(false))
                        .then(|| Some(s("hwid")).filter(|h| !h.trim().is_empty()).unwrap_or_else(store::hwid)),
                    happ_retry: v.get("happRetry").and_then(Value::as_bool).unwrap_or(true),
                };
                std::thread::spawn(move || {
                    let mut retry_after = None;
                    emit(match fetch::download_metadata(&url, sub.as_deref(), &client, &mut retry_after) {
                        Ok((sub, body)) => json!({ "ev": "fetched", "req": req, "sub": sub, "body": body }),
                        Err(e) => {
                            json!({ "ev": "fetched", "req": req, "needHwid": e == fetch::need_hwid(), "error": e, "retryAfter": retry_after })
                        }
                    })
                });
            }
            "describe" => emit(match fetch::describe(&s("text")) {
                Ok(p) => json!({ "ev": "described", "req": req, "profiles": p }),
                Err(e) => json!({ "ev": "described", "req": req, "error": e }),
            }),
            "routing" => {
                emit(match core::routing(&s("text"), &s("profile"), &v.get("options").cloned().unwrap_or_else(|| json!({})), b("tun")) {
                    Ok(routing) => json!({"ev":"routing","req":req,"routing":routing}),
                    Err(e) => json!({"ev":"routing","req":req,"error":e}),
                })
            }
            "details" => emit(match rv_core::describe::details(&s("text"), &s("profile")) {
                Ok(d) => json!({ "ev": "details", "req": req, "details": d }),
                Err(e) => json!({ "ev": "details", "req": req, "error": e }),
            }),
            "smartRules" => emit(json!({ "ev": "smartRules", "req": req, "rules": rv_core::engine::smart_rules_json() })),
            "importRules" => emit(match rv_core::happ::import_rules(&s("text")) {
                Ok((rules, skipped)) => json!({ "ev": "importRules", "req": req, "rules": rules, "skipped": skipped }),
                Err(e) => json!({ "ev": "importRules", "req": req, "error": e }),
            }),
            "cover" => {
                std::thread::spawn(move || emit(json!({ "ev": "cover", "req": req, "found": cover::find(&store::geo_dir(), &v) })));
            }
            "geoCounts" => {
                std::thread::spawn(move || {
                    emit(
                        json!({ "ev": "geoCounts", "req": req, "counts": cover::counts(&store::geo_dir(), v.get("items").unwrap_or(&Value::Null)) }),
                    )
                });
            }
            "caps" => emit(json!({ "ev": "caps", "req": req, "tun": tun::capable(), "hwid": store::hwid() })),
            "plugins" => {
                use base64::Engine;
                let op = s("op");
                let data = v.get("data").and_then(Value::as_str).map(|d| base64::engine::general_purpose::STANDARD.decode(d));
                let r = match data {
                    Some(Err(_)) => Err("bad plugin file".to_string()),
                    Some(Ok(b)) => rv_core::plugin::call(&op, v.get("arg").unwrap_or(&Value::Null), Some(&b)),
                    None => rv_core::plugin::call(&op, v.get("arg").unwrap_or(&Value::Null), None),
                };
                emit(match r {
                    Ok(result) => json!({ "ev": "plugins", "req": req, "op": op, "result": result }),
                    Err(e) => json!({ "ev": "plugins", "req": req, "op": op, "error": e }),
                });
            }
            #[cfg(windows)]
            "titlebar" => titlebar(n("hwnd", 0), b("dark"), v.get("caption").and_then(Value::as_u64)),
            "geo" => {
                let t = s("text");
                std::thread::spawn(move || fetch::update_geo(&t));
            }
            _ => {}
        }
    }
    let _ = core.send(core::Cmd::Stop);
    std::thread::sleep(std::time::Duration::from_millis(300));
    tun::down();
}

#[cfg(windows)]
fn titlebar(hwnd: u64, dark: bool, caption: Option<u64>) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    if hwnd == 0 {
        return;
    }
    let h = hwnd as usize as windows_sys::Win32::Foundation::HWND;
    let on: i32 = dark.into();
    unsafe {
        DwmSetWindowAttribute(h, 20, (&on as *const i32).cast(), 4);
        if let Some(rgb) = caption {
            let bgr = (((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)) as u32;
            DwmSetWindowAttribute(h, 35, (&bgr as *const u32).cast(), 4);
            let text: u32 = if dark { 0x00ff_ffff } else { 0 };
            DwmSetWindowAttribute(h, 36, (&text as *const u32).cast(), 4);
        }
    }
}
