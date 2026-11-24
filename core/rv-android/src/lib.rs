mod journal;
#[path = "../../rv-desktop/src/proxy.rs"]
mod proxy;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::rc::Rc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jint, jlong};
use rv_core::engine::Engine;
use rv_host::EngineHandler;
use rv_stats::Slot;
use rv_tun::Handler;
use tokio::sync::{oneshot, watch};

pub const TUN_V4: (Ipv4Addr, Ipv4Addr) = (Ipv4Addr::new(172, 19, 0, 1), Ipv4Addr::new(172, 19, 0, 2));

struct Core {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    network: watch::Sender<u64>,
    started: Instant,
    system_dns: rv_core::engine::SystemDns,
    generation: u64,
    tid: i32,
    stopping: Arc<std::sync::atomic::AtomicBool>,
    reconf: tokio::sync::mpsc::UnboundedSender<Reconf>,
    resolve: Resolver,
}

struct Prepared {
    profile: rv_config::Profile,
    geo: rv_core::geo::GeoData,
    opts: rv_core::engine::Options,
    chained: bool,
    fragmented: bool,
}

struct Reconf {
    prepared: Prepared,
    reply: std::sync::mpsc::Sender<Result<(), String>>,
}

struct Resolve {
    name: String,
    reply: std::sync::mpsc::Sender<Option<std::net::IpAddr>>,
}

type Resolver = tokio::sync::mpsc::UnboundedSender<Resolve>;

fn parse_hosts(json: &str) -> HashMap<String, Vec<IpAddr>> {
    let hosts: HashMap<String, Vec<String>> = serde_json::from_str(json).unwrap_or_default();
    hosts.into_iter().map(|(h, ips)| (h, ips.iter().filter_map(|i| i.parse::<IpAddr>().ok()).collect())).collect()
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    env: &mut JNIEnv,
    service: &JObject,
    config: &str,
    profile: &str,
    hosts: &str,
    geo_dir: &str,
    options: &str,
    system_dns: &rv_core::engine::SystemDns,
) -> Result<Prepared, String> {
    let profiles = rv_config::parse_subscription(config).map_err(|e| e.to_string())?;
    let p = rv_config::find_profile(&profiles, profile).ok_or_else(|| format!("no profile `{profile}`"))?;
    rv_net::set_hosts(parse_hosts(hosts));
    let geo = rv_core::geo::load_dir(std::path::Path::new(geo_dir));
    let opts_json: serde_json::Value = serde_json::from_str(options).unwrap_or_default();
    rv_net::bootstrap::configure(opts_json.get("serverDns").and_then(|v| v.as_str()).unwrap_or("auto")).map_err(|e| e.to_string())?;
    let mut profile_owned = p;
    let tune = rv_core::tune::Tune::from_json(&opts_json)?;
    tune.apply(&mut profile_owned)?;
    let text = |k: &str| opts_json.get(k).and_then(|v| v.as_str()).unwrap_or_default();
    let debug = opts_json.get("debugLog").and_then(|v| v.as_bool()).unwrap_or(false);
    journal::set_file(debug.then(|| std::path::Path::new(text("logDir"))));
    if !text("fakeDnsPath").is_empty() {
        rv_core::dns::fake::persist(std::path::Path::new(text("fakeDnsPath")));
    }
    set_system_dns(system_dns, &opts_json.get("systemDns").cloned().unwrap_or_default());
    let opts = rv_core::engine::Options {
        user_rules: text("userRules").to_string(),
        routing: rv_core::engine::RoutingMode::from_name(text("routingMode")).unwrap_or_default(),
        sub_routing: opts_json.get("subRouting").filter(|v| v.is_object()).cloned(),
        system_dns: system_dns.clone(),
        fake_dns: opts_json.get("fakeDns").and_then(|v| v.as_bool()).unwrap_or(false),
        ipv4_only: opts_json.get("ipv4Only").and_then(|v| v.as_bool()).unwrap_or(false),
        owner: make_owner(env, service).ok(),
        log_apps: debug,
        on_failing: make_failing(env, service).ok(),
        dns: opts_json
            .get("dns")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        background: opts_json.get("background").and_then(|v| v.as_bool()).unwrap_or(false),
    };
    if rv_core::engine::tunnel_dns(&mut profile_owned, &opts, &[]) {
        tracing::info!(target: "rv::core", "dns: none set, names go to {} through the tunnel, local ones to the system resolver", rv_core::engine::TUNNEL_DNS);
    }
    let chained = profile_owned.outbounds.iter().any(|o| o.tag.starts_with("chain-"));
    let fragmented = profile_owned.outbound("rynet-fragment").is_some();
    Ok(Prepared { profile: profile_owned, geo, opts, chained, fragmented })
}

fn throw(env: &mut JNIEnv, msg: &str) {
    let _ = env.throw_new("java/lang/IllegalStateException", msg);
}

#[allow(clippy::too_many_arguments)]
fn start(
    env: &mut JNIEnv,
    service: &JObject,
    fd: jint,
    config: &str,
    profile: &str,
    hosts: &str,
    mtu: jint,
    geo_dir: &str,
    options: &str,
) -> Result<Box<Core>, String> {
    let protect = make_protect(env, service)?;
    rv_stats::on_keepalive(make_keepalive(env, service).ok());
    let opts_json: serde_json::Value = serde_json::from_str(options).unwrap_or_default();
    rv_net::bootstrap::configure(opts_json.get("serverDns").and_then(|v| v.as_str()).unwrap_or("auto")).map_err(|e| e.to_string())?;
    let stats_path = opts_json.get("statsPath").and_then(|v| v.as_str()).unwrap_or_default();
    if let Err(e) = rv_stats::map(std::path::Path::new(stats_path)) {
        tracing::warn!("stats: {e}");
    }
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    *PREV.lock().unwrap_or_else(|e| e.into_inner()) = Some(Totals::now());
    rv_stats::reset(now_ms);
    let generation = rv_stats::get(Slot::Generation);
    let system_dns: rv_core::engine::SystemDns = Default::default();
    let Prepared { profile: profile_owned, geo, opts, chained, fragmented } =
        prepare(env, service, config, profile, hosts, geo_dir, options, &system_dns)?;

    let (stop_tx, stop_rx) = oneshot::channel();
    let (net_tx, net_rx) = watch::channel(0u64);
    let mut engine_net = net_rx.clone();
    let cfg = rv_tun::Config {
        fd,
        mtu: mtu as usize,
        v4: Some(TUN_V4),
        v6: Some(("fdfe:dcba:9876::1".parse().unwrap(), "fdfe:dcba:9876::2".parse().unwrap())),
        network: net_rx,
        stack: match opts_json.get("tcpStack").and_then(|v| v.as_str()) {
            Some("system") => rv_tun::Stack::System,
            _ => rv_tun::Stack::User,
        },
    };
    let tcp_stack = cfg.stack;
    let proxy_port = opts_json.get("proxyPort").and_then(|v| v.as_u64()).and_then(|p| u16::try_from(p).ok()).unwrap_or(10808);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<i32, String>>();
    let stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let exit_watch =
        (env.get_java_vm().map_err(|e| e.to_string())?, env.new_global_ref(service).map_err(|e| e.to_string())?, stopping.clone());
    let (reconf_tx, mut reconf_rx) = tokio::sync::mpsc::unbounded_channel::<Reconf>();
    let (resolve_tx, mut resolve_rx) = tokio::sync::mpsc::unbounded_channel::<Resolve>();
    let thread = std::thread::Builder::new()
        .name("rv-core".into())
        .spawn(move || {
            let _ = exit_watch.0.attach_current_thread_permanently();
            let engine = match Engine::new(&profile_owned, &geo, Some(protect.clone()), &opts) {
                Ok(e) => e,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            for w in &engine.warnings {
                tracing::warn!("{w}");
            }
            tracing::info!(target: "rv::core", "start profile={:?} routing={:?} mtu={mtu} stack={tcp_stack:?} chain={} fragment={}", profile_owned.remarks, opts.routing, chained, fragmented);
            tracing::info!(target: "rv::core", "routing {}", engine.routing_summary);
            let system_dns = opts.system_dns.clone();
            drop(geo);
            drop(profile_owned);
            drop(opts);
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("runtime: {e}")));
                    return;
                }
            };
            let tid = unsafe { libc::gettid() };
            let local = tokio::task::LocalSet::new();
            let engine = Rc::new(engine);
            if engine.has_balancers() {
                let e = engine.clone();
                local.spawn_local(e.watch_balancers());
            }
            let eh = Rc::new(EngineHandler::new(engine).with_system_dns(system_dns));
            {
                let h = eh.clone();
                local.spawn_local(async move {
                    while engine_net.changed().await.is_ok() {
                        tracing::info!(target: "rv::core", "network changed: pools reset");
                        h.current().reset_pools();
                    }
                });
            }
            {
                let h = eh.clone();
                local.spawn_local(async move {
                    while let Some(Reconf { prepared: p, reply }) = reconf_rx.recv().await {
                        let r = Engine::new(&p.profile, &p.geo, Some(protect.clone()), &p.opts).map(|e| {
                            for w in &e.warnings {
                                tracing::warn!("{w}");
                            }
                            tracing::info!(
                                target: "rv::core",
                                "reconfigure profile={:?} routing={:?} chain={} fragment={}",
                                p.profile.remarks, p.opts.routing, p.chained, p.fragmented,
                            );
                            tracing::info!(target: "rv::core", "routing {}", e.routing_summary);
                            let e = Rc::new(e);
                            if e.has_balancers() {
                                let b = e.clone();
                                tokio::task::spawn_local(b.watch_balancers());
                            }
                            h.swap(e);
                        });
                        drop(p);
                        let _ = reply.send(r.map_err(|e| e.to_string()));
                    }
                });
            }
            {
                let h = eh.clone();
                local.spawn_local(async move {
                    while let Some(Resolve { name, reply }) = resolve_rx.recv().await {
                        let e = h.current();
                        tokio::task::spawn_local(async move {
                            let r = tokio::time::timeout(std::time::Duration::from_secs(8), e.resolve_host(&name)).await;
                            let _ = reply.send(r.ok().and_then(Result::ok));
                        });
                    }
                });
            }
            local.spawn_local(journal::sampler());
            let proxied = eh.clone();
            let handler: Rc<dyn Handler> = eh;
            let r = if fd < 0 {
                let ready = move |r: std::io::Result<()>| {
                    let _ = ready_tx.send(r.map(|_| tid).map_err(|e| format!("proxy 127.0.0.1:{proxy_port}: {e}")));
                };
                local.block_on(&rt, serve_proxy(proxied, proxy_port, stop_rx, ready))
            } else {
                let ready = move |r: std::io::Result<()>| {
                    let _ = ready_tx.send(r.map(|_| tid).map_err(|e| format!("tun: {e}")));
                };
                local.block_on(&rt, rv_tun::run(cfg, handler, stop_rx, ready))
            };
            if let Err(e) = &r {
                tracing::error!("tun stack: {e}");
            }
            drop(local);
            rt.shutdown_background();
            let (vm, svc, stopping) = exit_watch;
            let attached = vm.attach_current_thread_permanently();
            if !stopping.load(std::sync::atomic::Ordering::SeqCst)
                && let Ok(mut env) = attached
            {
                let why = r.err().map_or_else(|| rv_core::lang::t("TUN закрыт", "TUN closed").to_string(), |e| e.to_string());
                if let Ok(why) = env.new_string(why) {
                    let _ = env.call_method(&svc, "onCoreExit", "(ILjava/lang/String;)V", &[tid.into(), (&why).into()]);
                }
                if env.exception_check().unwrap_or(false) {
                    let _ = env.exception_clear();
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let tid = ready_rx.recv().map_err(|_| "core thread died".to_string())??;
    Ok(Box::new(Core {
        stop: Some(stop_tx),
        thread: Some(thread),
        network: net_tx,
        started: Instant::now(),
        system_dns,
        tid,
        generation,
        stopping,
        reconf: reconf_tx,
        resolve: resolve_tx,
    }))
}

async fn serve_proxy(
    h: Rc<EngineHandler>,
    port: u16,
    mut stop: oneshot::Receiver<()>,
    ready: impl FnOnce(std::io::Result<()>),
) -> std::io::Result<()> {
    let listener = match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Ok(l) => {
            ready(Ok(()));
            l
        }
        Err(e) => {
            let kind = e.kind();
            ready(Err(std::io::Error::new(kind, e.to_string())));
            return Err(e);
        }
    };
    tracing::info!(target: "rv::core", "proxy listening on 127.0.0.1:{port}");
    let mut conns = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut stop => return Ok(()),
            a = listener.accept() => {
                let Ok((s, src)) = a else { continue };
                let engine = h.current();
                conns.spawn_local(async move {
                    if let Err(e) = proxy::handle(&engine, s, src).await {
                        tracing::debug!(target: "rv::core", "proxy conn: {e}");
                    }
                });
            }
            Some(_) = conns.join_next(), if !conns.is_empty() => {}
        }
    }
}

fn make_protect(env: &mut JNIEnv, service: &JObject) -> Result<rv_net::Protect, String> {
    let vm = env.get_java_vm().map_err(|e| e.to_string())?;
    let svc = env.new_global_ref(service).map_err(|e| e.to_string())?;
    Ok(Arc::new(move |fd| {
        let mut env = vm.attach_current_thread_permanently().map_err(std::io::Error::other)?;
        let r = env.call_method(&svc, "protect", "(I)Z", &[fd.into()]).and_then(|v| v.z());
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        if r.map_err(std::io::Error::other)? { Ok(()) } else { Err(std::io::Error::other("VpnService.protect failed")) }
    }))
}

fn call_service(vm: &jni::JavaVM, svc: &jni::objects::GlobalRef, name: &str, sig: &str, args: &[jni::objects::JValue]) {
    let mut env = match vm.attach_current_thread_permanently() {
        Ok(env) => env,
        Err(e) => {
            tracing::warn!(target: "rv::core", "jni {name}: attach: {e}");
            return;
        }
    };
    let r = env.call_method(svc, name, sig, args);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    if let Err(e) = r {
        tracing::warn!(target: "rv::core", "jni {name}{sig}: {e}");
    }
}

fn make_failing(env: &mut JNIEnv, service: &JObject) -> Result<rv_core::engine::Failing, String> {
    let vm = env.get_java_vm().map_err(|e| e.to_string())?;
    let svc = env.new_global_ref(service).map_err(|e| e.to_string())?;
    Ok(Arc::new(move || call_service(&vm, &svc, "onServerFailing", "()V", &[])))
}

fn make_keepalive(env: &mut JNIEnv, service: &JObject) -> Result<Arc<dyn Fn(bool) + Send + Sync>, String> {
    let vm = env.get_java_vm().map_err(|e| e.to_string())?;
    let svc = env.new_global_ref(service).map_err(|e| e.to_string())?;
    Ok(Arc::new(move |on| call_service(&vm, &svc, "onKeepAlive", "(Z)V", &[on.into()])))
}

fn make_owner(env: &mut JNIEnv, service: &JObject) -> Result<rv_core::engine::Owner, String> {
    let vm = env.get_java_vm().map_err(|e| e.to_string())?;
    let svc = env.new_global_ref(service).map_err(|e| e.to_string())?;
    Ok(Arc::new(move |tcp, src, dst| {
        let mut env = vm.attach_current_thread_permanently().ok()?;
        let s = env.new_string(src.ip().to_string()).ok()?;
        let d = env.new_string(dst.ip().to_string()).ok()?;
        let r = env
            .call_method(
                &svc,
                "ownerPackage",
                "(ZLjava/lang/String;ILjava/lang/String;I)Ljava/lang/String;",
                &[tcp.into(), (&s).into(), (src.port() as i32).into(), (&d).into(), (dst.port() as i32).into()],
            )
            .and_then(|v| v.l());
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        let _ = env.delete_local_ref(s);
        let _ = env.delete_local_ref(d);
        let obj = r.ok()?;
        if obj.is_null() {
            return None;
        }
        let js = jni::objects::JString::from(obj);
        let out: Option<String> = env.get_string(&js).ok().map(Into::into);
        let _ = env.delete_local_ref(js);
        out
    }))
}

#[allow(clippy::too_many_arguments)]
fn ping(
    env: &mut JNIEnv,
    service: &JObject,
    config: &str,
    remarks: &str,
    mode: jint,
    parallel: jint,
    timeout_ms: jint,
    sink: &JObject,
    ping_url: &str,
    tune: &str,
) -> Result<String, String> {
    let since = PING_GEN.load(std::sync::atomic::Ordering::SeqCst);
    let wanted: Vec<String> = serde_json::from_str(remarks).map_err(|e| e.to_string())?;
    let all = rv_config::parse_subscription_dial(config).map_err(|e| e.to_string())?;
    let tune_json: serde_json::Value = serde_json::from_str(tune).unwrap_or_default();
    rv_net::bootstrap::configure(tune_json.get("serverDns").and_then(|v| v.as_str()).unwrap_or("auto")).map_err(|e| e.to_string())?;
    let tune = rv_core::tune::Tune::from_json(&tune_json)?;
    let mut names: Vec<String> = Vec::new();
    let mut profiles: Vec<rv_config::Profile> = Vec::new();
    let mut broken: Vec<(String, String)> = Vec::new();
    for r in &wanted {
        let Some(mut p) = rv_config::find_profile(&all, r) else { continue };
        match tune.apply(&mut p) {
            Ok(()) => {
                names.push(p.remarks.clone());
                profiles.push(p);
            }
            Err(e) => broken.push((p.remarks.clone(), e)),
        }
    }
    drop(all);
    let protect = if service.is_null() { None } else { Some(make_protect(env, service)?) };
    let mode = rv_core::ping::Mode::from_code(mode);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    for (name, e) in &broken {
        if !sink.is_null()
            && let (Ok(n), Ok(e)) = (env.new_string(name), env.new_string(e))
        {
            let _ = env.call_method(sink, "result", "(Ljava/lang/String;ILjava/lang/String;)V", &[(&n).into(), (-1).into(), (&e).into()]);
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            let _ = env.delete_local_ref(n);
            let _ = env.delete_local_ref(e);
        }
    }
    let opts = rv_core::ping::Opts {
        parallel: parallel.max(0) as usize,
        timeout: std::time::Duration::from_millis(if timeout_ms > 0 { timeout_ms as u64 } else { 5000 }),
        url: rv_core::ping::ProbeUrl::parse(ping_url).unwrap_or_default(),
        cancel: None,
    };
    let mut done: Vec<Option<Result<u32, String>>> = vec![None; names.len()];
    let finished = rt.block_on(async {
        let stream = rv_core::ping::ping_stream(profiles, mode, protect, opts, |i, r| {
            done[i] = Some(r.clone());
            if sink.is_null() {
                return;
            }
            let (ms, err) = match r {
                Ok(ms) => (*ms as i32, None),
                Err(e) => (-1, Some(e.as_str())),
            };
            let Ok(name) = env.new_string(&names[i]) else { return };
            let err = match err.map(|e| env.new_string(e)) {
                Some(Ok(s)) => JObject::from(s),
                _ => JObject::null(),
            };
            let _ =
                env.call_method(sink, "result", "(Ljava/lang/String;ILjava/lang/String;)V", &[(&name).into(), ms.into(), (&err).into()]);
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            let _ = env.delete_local_ref(name);
            let _ = env.delete_local_ref(err);
        });
        tokio::select! {
            out = stream => Some(out),
            () = ping_cancelled(since) => None,
        }
    });
    rt.shutdown_background();
    let res = finished.unwrap_or_else(|| {
        tracing::debug!("ping cancelled");
        done.into_iter().map(|r| r.unwrap_or_else(|| Err(rv_core::ping::CANCELLED.to_string()))).collect()
    });
    let map: serde_json::Map<String, serde_json::Value> = names
        .into_iter()
        .zip(res)
        .filter(|(_, r)| r.as_ref().err().is_none_or(|e| e != rv_core::ping::CANCELLED))
        .map(|(n, r)| (n, r.map_or_else(|e| serde_json::json!(e), |ms| serde_json::json!(ms))))
        .chain(broken.into_iter().map(|(n, e)| (n, serde_json::json!(e))))
        .collect();
    Ok(serde_json::Value::Object(map).to_string())
}

fn countries(
    env: &mut JNIEnv,
    service: &JObject,
    config: &str,
    remarks: &str,
    parallel: jint,
    timeout_ms: jint,
    tune: &str,
) -> Result<String, String> {
    let wanted: Vec<String> = serde_json::from_str(remarks).map_err(|e| e.to_string())?;
    let all = rv_config::parse_subscription_dial(config).map_err(|e| e.to_string())?;
    let tune_json: serde_json::Value = serde_json::from_str(tune).unwrap_or_default();
    rv_net::bootstrap::configure(tune_json.get("serverDns").and_then(|v| v.as_str()).unwrap_or("auto")).map_err(|e| e.to_string())?;
    let tune = rv_core::tune::Tune::from_json(&tune_json)?;
    let profiles: Vec<rv_config::Profile> =
        wanted.iter().filter_map(|r| rv_config::find_profile(&all, r)).filter_map(|mut p| tune.apply(&mut p).ok().map(|()| p)).collect();
    let names: Vec<String> = profiles.iter().map(|p| p.remarks.clone()).collect();
    let protect = if service.is_null() { None } else { Some(make_protect(env, service)?) };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let timeout = std::time::Duration::from_millis(if timeout_ms > 0 { timeout_ms as u64 } else { 5000 });
    let since = PING_GEN.load(std::sync::atomic::Ordering::SeqCst);
    let res = rt.block_on(async {
        tokio::select! {
            out = rv_core::ping::countries(profiles, protect, parallel.max(0) as usize, timeout) => out,
            () = ping_cancelled(since) => Vec::new(),
        }
    });
    rt.shutdown_background();
    let map: serde_json::Map<String, serde_json::Value> =
        names.into_iter().zip(res).filter_map(|(n, c)| Some((n, serde_json::json!(c?)))).collect();
    Ok(serde_json::Value::Object(map).to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_countries<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    service: JObject<'l>,
    config: JString<'l>,
    remarks: JString<'l>,
    parallel: jint,
    timeout_ms: jint,
    tune: JString<'l>,
) -> jni::sys::jstring {
    journal::init();
    let config = jstr(&mut env, &config);
    let remarks = jstr(&mut env, &remarks);
    let tune = jstr(&mut env, &tune);
    let r = countries(&mut env, &service, &config, &remarks, parallel, timeout_ms, &tune);
    drop(config);
    purge();
    match r {
        Ok(s) => env.new_string(s).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_details<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    text: JString<'l>,
    remarks: JString<'l>,
) -> jni::sys::jstring {
    let text = jstr(&mut env, &text);
    let remarks = jstr(&mut env, &remarks);
    match rv_core::describe::details(&text, &remarks) {
        Ok(v) => env.new_string(v.to_string()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_importRules<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    text: JString<'l>,
) -> jni::sys::jstring {
    let text = jstr(&mut env, &text);
    match rv_core::happ::import_rules(&text) {
        Ok((rules, skipped)) => env
            .new_string(serde_json::json!({ "rules": rules, "skipped": skipped }).to_string())
            .map(|s| s.into_raw())
            .unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_describe<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    text: JString<'l>,
) -> jni::sys::jstring {
    let text = jstr(&mut env, &text);
    let r = rv_core::describe::describe(&text);
    drop(text);
    purge();
    match r {
        Ok(v) => env.new_string(v.to_string()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            throw(&mut env, &e.to_string());
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_plugins<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    op: JString<'l>,
    arg: JString<'l>,
    bytes: jni::objects::JByteArray<'l>,
) -> jni::sys::jstring {
    let op = jstr(&mut env, &op);
    let arg: serde_json::Value = serde_json::from_str(&jstr(&mut env, &arg)).unwrap_or_default();
    let bytes = if bytes.is_null() { None } else { env.convert_byte_array(&bytes).ok() };
    match rv_core::plugin::call(&op, &arg, bytes.as_deref()) {
        Ok(v) => env.new_string(v.to_string()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_smartRules<'l>(env: JNIEnv<'l>, _cls: JClass<'l>) -> jni::sys::jstring {
    env.new_string(rv_core::engine::smart_rules_json().to_string()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_ping<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    service: JObject<'l>,
    config: JString<'l>,
    remarks: JString<'l>,
    mode: jint,
    parallel: jint,
    timeout_ms: jint,
    sink: JObject<'l>,
    ping_url: JString<'l>,
    tune: JString<'l>,
) -> jni::sys::jstring {
    journal::init();
    let config = jstr(&mut env, &config);
    let remarks = jstr(&mut env, &remarks);
    let ping_url = jstr(&mut env, &ping_url);
    let tune = jstr(&mut env, &tune);
    let r = ping(&mut env, &service, &config, &remarks, mode, parallel, timeout_ms, &sink, &ping_url, &tune);
    drop(config);
    purge();
    let out = match r {
        Ok(s) => s,
        Err(e) => {
            throw(&mut env, &e);
            return std::ptr::null_mut();
        }
    };
    env.new_string(out).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

static SPEEDTEST_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_speedtest<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    service: JObject<'l>,
    config: JString<'l>,
    remarks: JString<'l>,
    tune: JString<'l>,
    sink: JObject<'l>,
) -> jni::sys::jstring {
    journal::init();
    let config = jstr(&mut env, &config);
    let remarks = jstr(&mut env, &remarks);
    let tune = jstr(&mut env, &tune);
    match speedtest(&mut env, &service, &config, &remarks, &tune, &sink) {
        Ok(s) => env.new_string(s).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

static PING_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PING_CANCELLED: tokio::sync::Notify = tokio::sync::Notify::const_new();

fn ping_cancel() {
    PING_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    PING_CANCELLED.notify_waiters();
}

async fn ping_cancelled(since: u64) {
    loop {
        let notified = PING_CANCELLED.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if PING_GEN.load(std::sync::atomic::Ordering::SeqCst) != since {
            return;
        }
        notified.await;
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_pingCancel(_env: JNIEnv, _cls: JClass) {
    ping_cancel();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_speedtestCancel(_env: JNIEnv, _cls: JClass) {
    SPEEDTEST_CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_speedtestReset(_env: JNIEnv, _cls: JClass) {
    SPEEDTEST_CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
}

fn speedtest(env: &mut JNIEnv, service: &JObject, config: &str, remarks: &str, tune: &str, sink: &JObject) -> Result<String, String> {
    let all = rv_config::parse_subscription_dial(config).map_err(|e| e.to_string())?;
    let mut p = rv_config::find_profile(&all, remarks).ok_or_else(|| format!("no profile `{remarks}`"))?;
    let tune_json: serde_json::Value = serde_json::from_str(tune).unwrap_or_default();
    rv_net::bootstrap::configure(tune_json.get("serverDns").and_then(|v| v.as_str()).unwrap_or("auto")).map_err(|e| e.to_string())?;
    rv_core::tune::Tune::from_json(&tune_json)?.apply(&mut p)?;
    let protect = if service.is_null() { None } else { Some(make_protect(env, service)?) };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let env = std::cell::RefCell::new(env);
    let call = |method: &str, sig: &str, args: &[jni::objects::JValue]| {
        let mut env = env.borrow_mut();
        let _ = env.call_method(sink, method, sig, args);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
    };
    let r = rt.block_on(rv_core::speedtest::run(
        &p,
        protect,
        &SPEEDTEST_CANCEL,
        |phase, v, f| call("progress", "(IDD)V", &[(phase as i32).into(), v.into(), f.into()]),
        |server, via| {
            let json = serde_json::json!({ "server": server.to_json(), "via": via }).to_string();
            let Ok(js) = env.borrow_mut().new_string(json) else { return };
            call("server", "(Ljava/lang/String;)V", &[(&js).into()]);
            let _ = env.borrow_mut().delete_local_ref(js);
        },
    ));
    rt.shutdown_background();
    match r {
        Ok(rep) => Ok(rep.to_json().to_string()),
        Err(e) => Err(rv_core::explain::explain(&e)),
    }
}

fn purge() {
    #[cfg(target_os = "android")]
    {
        static MALLOPT: std::sync::OnceLock<Option<unsafe extern "C" fn(libc::c_int, libc::c_int) -> libc::c_int>> =
            std::sync::OnceLock::new();
        let f = MALLOPT.get_or_init(|| {
            let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"mallopt".as_ptr()) };
            (!p.is_null()).then(|| unsafe {
                std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn(libc::c_int, libc::c_int) -> libc::c_int>(p)
            })
        });
        const M_PURGE: libc::c_int = -101;
        if let Some(f) = f {
            unsafe { f(M_PURGE, 0) };
        }
    }
}

fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(Into::into).unwrap_or_default()
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_start<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    service: JObject<'l>,
    fd: jint,
    config: JString<'l>,
    profile: JString<'l>,
    hosts: JString<'l>,
    mtu: jint,
    geo_dir: JString<'l>,
    options: JString<'l>,
) -> jlong {
    journal::init();
    let (config, profile, hosts) = (jstr(&mut env, &config), jstr(&mut env, &profile), jstr(&mut env, &hosts));
    let geo_dir = jstr(&mut env, &geo_dir);
    let options = jstr(&mut env, &options);
    let r = start(&mut env, &service, fd, &config, &profile, &hosts, mtu, &geo_dir, &options);
    drop(config);
    purge();
    match r {
        Ok(core) => Box::into_raw(core) as jlong,
        Err(e) => {
            throw(&mut env, &e);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_setQuiet(_env: JNIEnv, _cls: JClass, quiet: jni::sys::jboolean) {
    let quiet = quiet != 0;
    if rv_core::engine::set_quiet(quiet) && !quiet {
        journal::wake_sampler();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_setEnglish(_env: JNIEnv, _cls: JClass, en: jni::sys::jboolean) {
    rv_core::lang::set_english(en != 0);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_stop(_env: JNIEnv, _cls: JClass, handle: jlong) {
    if handle == 0 {
        return;
    }
    let mut core = unsafe { Box::from_raw(handle as *mut Core) };
    core.stopping.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(s) = core.stop.take() {
        let _ = s.send(());
    }
    if let Some(t) = core.thread.take() {
        let _ = t.join();
    }
    rv_stats::on_keepalive(None);
    let t = if rv_stats::get(Slot::Generation) == core.generation {
        Totals::now()
    } else {
        PREV.lock().unwrap_or_else(|e| e.into_inner()).filter(|p| p.generation == core.generation).unwrap_or_default()
    };
    tracing::info!(
        target: "rv::core",
        "stop uptime_s={} up={} down={} tcp_total={} dns={}",
        core.started.elapsed().as_secs(),
        t.up,
        t.down,
        t.tcp,
        t.dns,
    );
    journal::flush();
}

#[derive(Clone, Copy, Default)]
struct Totals {
    generation: u64,
    up: u64,
    down: u64,
    tcp: u64,
    dns: u64,
}

impl Totals {
    fn now() -> Self {
        Totals {
            generation: rv_stats::get(Slot::Generation),
            up: rv_stats::get(Slot::Up),
            down: rv_stats::get(Slot::Down),
            tcp: rv_stats::get(Slot::TcpTotal),
            dns: rv_stats::get(Slot::DnsQueries),
        }
    }
}

static PREV: std::sync::Mutex<Option<Totals>> = std::sync::Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_coreTid(_env: JNIEnv, _cls: JClass, handle: jlong) -> jint {
    if handle == 0 {
        return 0;
    }
    unsafe { &*(handle as *const Core) }.tid
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_networkChanged<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    handle: jlong,
    dns: JString<'l>,
) {
    if handle == 0 {
        return;
    }
    let core = unsafe { &*(handle as *const Core) };
    let dns = jstr(&mut env, &dns);
    set_system_dns(&core.system_dns, &serde_json::from_str(&dns).unwrap_or_default());
    rv_net::forget_lookups();
    core.network.send_modify(|n| *n += 1);
}

fn set_system_dns(to: &rv_core::engine::SystemDns, list: &serde_json::Value) {
    let mut v: Vec<SocketAddr> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a.as_str()?.parse::<IpAddr>().ok())
        .map(|ip| SocketAddr::new(ip, 53))
        .collect();
    v.sort_by_key(|a| !a.is_ipv4());
    if let Ok(mut s) = to.lock() {
        *s = v;
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_flushLog(_env: JNIEnv, _cls: JClass) {
    journal::flush();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_setRoots<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    certs: jni::objects::JObjectArray<'l>,
) -> jint {
    journal::init();
    let n = env.get_array_length(&certs).unwrap_or(0);
    let mut ders = Vec::with_capacity(n.max(0) as usize);
    for i in 0..n {
        let Ok(obj) = env.get_object_array_element(&certs, i) else { break };
        let arr = jni::objects::JByteArray::from(obj);
        if let Ok(der) = env.convert_byte_array(&arr) {
            ders.push(der);
        }
        let _ = env.delete_local_ref(arr);
    }
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    let accepted = rv_tls::set_roots(&ders);
    tracing::info!(target: "rv::core", "CA roots from Android: {accepted} of {}", ders.len());
    accepted as jint
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_reconfigure<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    service: JObject<'l>,
    handle: jlong,
    config: JString<'l>,
    profile: JString<'l>,
    hosts: JString<'l>,
    geo_dir: JString<'l>,
    options: JString<'l>,
) {
    if handle == 0 {
        throw(&mut env, "core is not running");
        return;
    }
    let core = unsafe { &*(handle as *const Core) };
    let (config, profile, hosts) = (jstr(&mut env, &config), jstr(&mut env, &profile), jstr(&mut env, &hosts));
    let (geo_dir, options) = (jstr(&mut env, &geo_dir), jstr(&mut env, &options));
    let r = prepare(&mut env, &service, &config, &profile, &hosts, &geo_dir, &options, &core.system_dns).and_then(|prepared| {
        let (tx, rx) = std::sync::mpsc::channel();
        core.reconf.send(Reconf { prepared, reply: tx }).map_err(|_| "core stopped".to_string())?;
        rx.recv().map_err(|_| "core stopped".to_string())?
    });
    drop(config);
    purge();
    if let Err(e) = r {
        throw(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_resolver(_env: JNIEnv, _cls: JClass, handle: jlong) -> jlong {
    if handle == 0 {
        return 0;
    }
    let core = unsafe { &*(handle as *const Core) };
    Box::into_raw(Box::new(core.resolve.clone())) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_resolve<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    resolver: jlong,
    name: JString<'l>,
) -> jni::sys::jstring {
    if resolver == 0 {
        return std::ptr::null_mut();
    }
    let tx = unsafe { &*(resolver as *const Resolver) };
    let name = jstr(&mut env, &name);
    let (reply, rx) = std::sync::mpsc::channel();
    let ip = tx.send(Resolve { name, reply }).ok().and_then(|_| rx.recv_timeout(std::time::Duration::from_secs(10)).ok().flatten());
    match ip.map(|ip| env.new_string(ip.to_string())) {
        Some(Ok(s)) => s.into_raw(),
        _ => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_freeResolver(_env: JNIEnv, _cls: JClass, resolver: jlong) {
    if resolver != 0 {
        drop(unsafe { Box::from_raw(resolver as *mut Resolver) });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_addHosts<'l>(mut env: JNIEnv<'l>, _cls: JClass<'l>, hosts: JString<'l>) {
    let hosts = jstr(&mut env, &hosts);
    rv_net::add_hosts(parse_hosts(&hosts));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_bootstrapHosts<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    service: JObject<'l>,
    names: JString<'l>,
    mode: JString<'l>,
) -> jni::sys::jstring {
    let names = jstr(&mut env, &names);
    let mode = jstr(&mut env, &mode);
    let run = (|| -> Result<String, String> {
        rv_net::bootstrap::configure(&mode).map_err(|e| e.to_string())?;
        let protect = if service.is_null() { None } else { Some(make_protect(&mut env, &service)?) };
        let names: Vec<String> = serde_json::from_str(&names).map_err(|e| e.to_string())?;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        let map = rt.block_on(async {
            let mut out = serde_json::Map::new();
            for chunk in names.chunks(6) {
                let mut pending: Vec<_> = chunk
                    .iter()
                    .map(|h| (h, Box::pin(rv_net::bootstrap::resolve(h, protect.as_ref(), rv_net::bootstrap::mode()))))
                    .collect();
                while !pending.is_empty() {
                    let (i, result) = std::future::poll_fn(|cx| {
                        use std::future::Future;
                        for (i, (_, f)) in pending.iter_mut().enumerate() {
                            if let std::task::Poll::Ready(v) = f.as_mut().poll(cx) {
                                return std::task::Poll::Ready((i, v));
                            }
                        }
                        std::task::Poll::Pending
                    })
                    .await;
                    let (h, _) = pending.remove(i);
                    if let Ok(ips) = result {
                        out.insert(h.clone(), serde_json::json!(ips.iter().map(ToString::to_string).collect::<Vec<_>>()));
                    }
                }
            }
            out
        });
        rt.shutdown_background();
        Ok(serde_json::Value::Object(map).to_string())
    })();
    match run {
        Ok(s) => env.new_string(s).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_yzewe_rynet_core_Native_routing<'l>(
    mut env: JNIEnv<'l>,
    _cls: JClass<'l>,
    config: JString<'l>,
    profile: JString<'l>,
    options: JString<'l>,
) -> jni::sys::jstring {
    let (config, profile, options) = (jstr(&mut env, &config), jstr(&mut env, &profile), jstr(&mut env, &options));
    let result = serde_json::from_str::<serde_json::Value>(&options)
        .map_err(|e| e.to_string())
        .and_then(|opts| rv_core::describe::routing(&config, &profile, &opts));
    let value = result.unwrap_or_else(|e| serde_json::json!({"error":e}));
    env.new_string(value.to_string()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ping_cancel_hits_only_pings_started_before_it() {
        let ms = std::time::Duration::from_millis;
        let early = PING_GEN.load(std::sync::atomic::Ordering::SeqCst);
        let running = tokio::spawn(ping_cancelled(early));
        tokio::time::sleep(ms(20)).await;
        assert!(!running.is_finished());
        ping_cancel();
        tokio::time::timeout(ms(500), running).await.expect("cancel reaches a running ping").unwrap();
        let late = PING_GEN.load(std::sync::atomic::Ordering::SeqCst);
        assert!(tokio::time::timeout(ms(50), ping_cancelled(late)).await.is_err(), "a ping started after the cancel keeps going");
        assert!(tokio::time::timeout(ms(50), ping_cancelled(early)).await.is_ok(), "a later ping does not undo the earlier cancel");
    }
}
