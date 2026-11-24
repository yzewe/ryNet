#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod journal;

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rv_core::engine::Engine;
use rv_host::EngineHandler;
use rv_stats::Slot;
use rv_tun::Handler;
use tokio::sync::{oneshot, watch};

pub const TUN_V4: (Ipv4Addr, Ipv4Addr) = (Ipv4Addr::new(172, 19, 0, 1), Ipv4Addr::new(172, 19, 0, 2));

pub type ExitCb = extern "C" fn(ctx: *mut c_void, why: *const c_char);
pub type PingCb = extern "C" fn(ctx: *mut c_void, name: *const c_char, ms: i32, err: *const c_char);
pub type ProgressCb = extern "C" fn(ctx: *mut c_void, phase: i32, value: f64, fraction: f64);
pub type ServerCb = extern "C" fn(ctx: *mut c_void, json: *const c_char);

struct Ctx(*mut c_void);
unsafe impl Send for Ctx {}

struct Core {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    network: watch::Sender<u64>,
    started: Instant,
    system_dns: rv_core::engine::SystemDns,
    generation: u64,
    stopping: Arc<AtomicBool>,
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
    reply: std::sync::mpsc::Sender<Option<IpAddr>>,
}

type Resolver = tokio::sync::mpsc::UnboundedSender<Resolve>;

fn s(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn out(v: impl Into<Vec<u8>>) -> *mut c_char {
    let mut v = v.into();
    v.retain(|&b| b != 0);
    CString::new(v).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

fn set_err(err: *mut *mut c_char, msg: &str) {
    if !err.is_null() {
        unsafe { *err = out(msg) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_free(p: *mut c_char) {
    if !p.is_null() {
        drop(unsafe { CString::from_raw(p) });
    }
}

fn parse_hosts(json: &str) -> HashMap<String, Vec<IpAddr>> {
    let hosts: HashMap<String, Vec<String>> = serde_json::from_str(json).unwrap_or_default();
    hosts.into_iter().map(|(h, ips)| (h, ips.iter().filter_map(|i| i.parse::<IpAddr>().ok()).collect())).collect()
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

fn prepare(
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
        owner: None,
        log_apps: false,
        on_failing: None,
        dns: Vec::new(),
        background: false,
    };
    if rv_core::engine::tunnel_dns(&mut profile_owned, &opts, &[]) {
        tracing::info!(target: "rv::core", "dns: none set, names go to {} through the tunnel, local ones to the system resolver", rv_core::engine::TUNNEL_DNS);
    }
    let chained = profile_owned.outbounds.iter().any(|o| o.tag.starts_with("chain-"));
    let fragmented = profile_owned.outbound("rynet-fragment").is_some();
    Ok(Prepared { profile: profile_owned, geo, opts, chained, fragmented })
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_tun_fd() -> i32 {
    #[cfg(target_vendor = "apple")]
    {
        const SYSPROTO_CONTROL: libc::c_int = 2;
        const UTUN_OPT_IFNAME: libc::c_int = 2;
        for fd in 0..1024 {
            let mut name = [0u8; 32];
            let mut len = name.len() as libc::socklen_t;
            let r = unsafe { libc::getsockopt(fd, SYSPROTO_CONTROL, UTUN_OPT_IFNAME, name.as_mut_ptr().cast(), &mut len) };
            if r == 0 && name.starts_with(b"utun") {
                return fd;
            }
        }
    }
    -1
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn rv_start(
    fd: i32,
    config: *const c_char,
    profile: *const c_char,
    hosts: *const c_char,
    mtu: i32,
    geo_dir: *const c_char,
    options: *const c_char,
    on_exit: Option<ExitCb>,
    ctx: *mut c_void,
    err: *mut *mut c_char,
) -> *mut c_void {
    journal::init();
    let dup = unsafe { libc::dup(fd) };
    if dup < 0 {
        set_err(err, &format!("utun: {}", std::io::Error::last_os_error()));
        return std::ptr::null_mut();
    }
    match start(dup, &s(config), &s(profile), &s(hosts), mtu, &s(geo_dir), &s(options), on_exit, Ctx(ctx)) {
        Ok(core) => Box::into_raw(core).cast(),
        Err(e) => {
            unsafe { libc::close(dup) };
            set_err(err, &e);
            std::ptr::null_mut()
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn start(
    fd: i32,
    config: &str,
    profile: &str,
    hosts: &str,
    mtu: i32,
    geo_dir: &str,
    options: &str,
    on_exit: Option<ExitCb>,
    ctx: Ctx,
) -> Result<Box<Core>, String> {
    let opts_json: serde_json::Value = serde_json::from_str(options).unwrap_or_default();
    rv_net::bootstrap::configure(opts_json.get("serverDns").and_then(|v| v.as_str()).unwrap_or("auto")).map_err(|e| e.to_string())?;
    let stats_path = opts_json.get("statsPath").and_then(|v| v.as_str()).unwrap_or_default();
    if !stats_path.is_empty()
        && let Err(e) = rv_stats::map(std::path::Path::new(stats_path))
    {
        tracing::warn!("stats: {e}");
    }
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    rv_stats::reset(now_ms);
    let generation = rv_stats::get(Slot::Generation);
    let system_dns: rv_core::engine::SystemDns = Default::default();
    let Prepared { profile: profile_owned, geo, opts, chained, fragmented } =
        prepare(config, profile, hosts, geo_dir, options, &system_dns)?;

    let (stop_tx, stop_rx) = oneshot::channel();
    let (net_tx, net_rx) = watch::channel(0u64);
    let mut engine_net = net_rx.clone();
    let cfg = rv_tun::Config { fd, mtu: mtu as usize, v4: Some(TUN_V4), v6: None, network: net_rx, stack: rv_tun::Stack::System };
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let stopping = Arc::new(AtomicBool::new(false));
    let exit_watch = (on_exit, ctx, stopping.clone());
    let (reconf_tx, mut reconf_rx) = tokio::sync::mpsc::unbounded_channel::<Reconf>();
    let (resolve_tx, mut resolve_rx) = tokio::sync::mpsc::unbounded_channel::<Resolve>();
    let thread = std::thread::Builder::new()
        .name("rv-core".into())
        .spawn(move || {
            let engine = match Engine::new(&profile_owned, &geo, None, &opts) {
                Ok(e) => e,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            for w in &engine.warnings {
                tracing::warn!("{w}");
            }
            tracing::info!(target: "rv::core", "start profile={:?} routing={:?} mtu={mtu} chain={} fragment={}", profile_owned.remarks, opts.routing, chained, fragmented);
            drop(geo);
            drop(profile_owned);
            drop(opts);
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("runtime: {e}")));
                    return;
                }
            };
            let local = tokio::task::LocalSet::new();
            let engine = Rc::new(engine);
            if engine.has_balancers() {
                let e = engine.clone();
                local.spawn_local(e.watch_balancers());
            }
            let eh = Rc::new(EngineHandler::new(engine));
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
                        let r = Engine::new(&p.profile, &p.geo, None, &p.opts).map(|e| {
                            for w in &e.warnings {
                                tracing::warn!("{w}");
                            }
                            tracing::info!(target: "rv::core", "reconfigure profile={:?} routing={:?} chain={} fragment={}", p.profile.remarks, p.opts.routing, p.chained, p.fragmented);
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
            let handler: Rc<dyn Handler> = eh;
            let ready = move |r: std::io::Result<()>| {
                let _ = ready_tx.send(r.map_err(|e| format!("tun: {e}")));
            };
            let r = local.block_on(&rt, rv_tun::run(cfg, handler, stop_rx, ready));
            if let Err(e) = &r {
                tracing::error!("tun stack: {e}");
            }
            drop(local);
            rt.shutdown_background();
            let (on_exit, ctx, stopping) = exit_watch;
            if !stopping.load(Ordering::SeqCst)
                && let Some(cb) = on_exit
            {
                let why = r.err().map_or_else(|| "TUN закрыт".to_string(), |e| e.to_string());
                let why = out(why);
                cb(ctx.0, why);
                rv_free(why);
            }
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv().map_err(|_| "core thread died".to_string())??;
    Ok(Box::new(Core {
        stop: Some(stop_tx),
        thread: Some(thread),
        network: net_tx,
        started: Instant::now(),
        system_dns,
        generation,
        stopping,
        reconf: reconf_tx,
        resolve: resolve_tx,
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_stop(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    let mut core = unsafe { Box::from_raw(handle.cast::<Core>()) };
    core.stopping.store(true, Ordering::SeqCst);
    if let Some(s) = core.stop.take() {
        let _ = s.send(());
    }
    if let Some(t) = core.thread.take() {
        let _ = t.join();
    }
    let same = rv_stats::get(Slot::Generation) == core.generation;
    tracing::info!(
        target: "rv::core",
        "stop uptime_s={} up={} down={}",
        core.started.elapsed().as_secs(),
        if same { rv_stats::get(Slot::Up) } else { 0 },
        if same { rv_stats::get(Slot::Down) } else { 0 },
    );
    journal::flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_network_changed(handle: *mut c_void, dns: *const c_char) {
    if handle.is_null() {
        return;
    }
    let core = unsafe { &*handle.cast::<Core>() };
    set_system_dns(&core.system_dns, &serde_json::from_str(&s(dns)).unwrap_or_default());
    rv_net::forget_lookups();
    core.network.send_modify(|n| *n += 1);
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_reconfigure(
    handle: *mut c_void,
    config: *const c_char,
    profile: *const c_char,
    hosts: *const c_char,
    geo_dir: *const c_char,
    options: *const c_char,
    err: *mut *mut c_char,
) -> i32 {
    if handle.is_null() {
        set_err(err, "core is not running");
        return 0;
    }
    let core = unsafe { &*handle.cast::<Core>() };
    let r = prepare(&s(config), &s(profile), &s(hosts), &s(geo_dir), &s(options), &core.system_dns).and_then(|prepared| {
        let (tx, rx) = std::sync::mpsc::channel();
        core.reconf.send(Reconf { prepared, reply: tx }).map_err(|_| "core stopped".to_string())?;
        rx.recv().map_err(|_| "core stopped".to_string())?
    });
    match r {
        Ok(()) => 1,
        Err(e) => {
            set_err(err, &e);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_resolver(handle: *mut c_void) -> *mut c_void {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    let core = unsafe { &*handle.cast::<Core>() };
    Box::into_raw(Box::new(core.resolve.clone())).cast()
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_resolver_free(resolver: *mut c_void) {
    if !resolver.is_null() {
        drop(unsafe { Box::from_raw(resolver.cast::<Resolver>()) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_resolver_resolve(resolver: *mut c_void, name: *const c_char) -> *mut c_char {
    if resolver.is_null() {
        return std::ptr::null_mut();
    }
    let tx = unsafe { &*resolver.cast::<Resolver>() };
    let (reply, rx) = std::sync::mpsc::channel();
    let ip =
        tx.send(Resolve { name: s(name), reply }).ok().and_then(|_| rx.recv_timeout(std::time::Duration::from_secs(10)).ok().flatten());
    ip.map_or(std::ptr::null_mut(), |ip| out(ip.to_string()))
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_add_hosts(hosts: *const c_char) {
    rv_net::add_hosts(parse_hosts(&s(hosts)));
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_flush_log() {
    journal::flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_describe(text: *const c_char, err: *mut *mut c_char) -> *mut c_char {
    match rv_core::describe::describe(&s(text)) {
        Ok(v) => out(v.to_string()),
        Err(e) => {
            set_err(err, &e.to_string());
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn rv_ping(
    config: *const c_char,
    remarks: *const c_char,
    mode: i32,
    parallel: i32,
    timeout_ms: i32,
    ping_url: *const c_char,
    tune: *const c_char,
    cb: Option<PingCb>,
    ctx: *mut c_void,
    err: *mut *mut c_char,
) -> *mut c_char {
    journal::init();
    match ping(&s(config), &s(remarks), mode, parallel, timeout_ms, &s(ping_url), &s(tune), cb, ctx) {
        Ok(r) => out(r),
        Err(e) => {
            set_err(err, &e);
            std::ptr::null_mut()
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn ping(
    config: &str,
    remarks: &str,
    mode: i32,
    parallel: i32,
    timeout_ms: i32,
    ping_url: &str,
    tune: &str,
    cb: Option<PingCb>,
    ctx: *mut c_void,
) -> Result<String, String> {
    let wanted: Vec<String> = serde_json::from_str(remarks).map_err(|e| e.to_string())?;
    let all = rv_config::parse_subscription(config).map_err(|e| e.to_string())?;
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
    let send = |name: &str, ms: i32, e: Option<&str>| {
        let Some(cb) = cb else { return };
        let n = out(name);
        let e = e.map_or(std::ptr::null_mut(), out);
        cb(ctx, n, ms, e);
        rv_free(n);
        rv_free(e);
    };
    for (name, e) in &broken {
        send(name, -1, Some(e));
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let opts = rv_core::ping::Opts {
        parallel: parallel.max(0) as usize,
        timeout: std::time::Duration::from_millis(if timeout_ms > 0 { timeout_ms as u64 } else { 5000 }),
        url: rv_core::ping::ProbeUrl::parse(ping_url).unwrap_or_default(),
        cancel: None,
    };
    let res = rt.block_on(rv_core::ping::ping_stream(profiles, rv_core::ping::Mode::from_code(mode), None, opts, |i, r| match r {
        Ok(ms) => send(&names[i], *ms as i32, None),
        Err(e) => send(&names[i], -1, Some(e.as_str())),
    }));
    rt.shutdown_background();
    let map: serde_json::Map<String, serde_json::Value> = names
        .into_iter()
        .zip(res)
        .map(|(n, r)| (n, r.map_or_else(|e| serde_json::json!(e), |ms| serde_json::json!(ms))))
        .chain(broken.into_iter().map(|(n, e)| (n, serde_json::json!(e))))
        .collect();
    Ok(serde_json::Value::Object(map).to_string())
}

static SPEEDTEST_CANCEL: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
pub extern "C" fn rv_speedtest_cancel() {
    SPEEDTEST_CANCEL.store(true, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn rv_speedtest(
    config: *const c_char,
    remarks: *const c_char,
    tune: *const c_char,
    progress: Option<ProgressCb>,
    server: Option<ServerCb>,
    ctx: *mut c_void,
    err: *mut *mut c_char,
) -> *mut c_char {
    journal::init();
    match speedtest(&s(config), &s(remarks), &s(tune), progress, server, ctx) {
        Ok(r) => out(r),
        Err(e) => {
            set_err(err, &e);
            std::ptr::null_mut()
        }
    }
}

fn speedtest(
    config: &str,
    remarks: &str,
    tune: &str,
    progress: Option<ProgressCb>,
    server: Option<ServerCb>,
    ctx: *mut c_void,
) -> Result<String, String> {
    SPEEDTEST_CANCEL.store(false, Ordering::Relaxed);
    let all = rv_config::parse_subscription(config).map_err(|e| e.to_string())?;
    let mut p = rv_config::find_profile(&all, remarks).ok_or_else(|| format!("no profile `{remarks}`"))?;
    let tune_json: serde_json::Value = serde_json::from_str(tune).unwrap_or_default();
    rv_net::bootstrap::configure(tune_json.get("serverDns").and_then(|v| v.as_str()).unwrap_or("auto")).map_err(|e| e.to_string())?;
    rv_core::tune::Tune::from_json(&tune_json)?.apply(&mut p)?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let r = rt.block_on(rv_core::speedtest::run(
        &p,
        None,
        &SPEEDTEST_CANCEL,
        |phase, v, f| {
            if let Some(cb) = progress {
                cb(ctx, phase as i32, v, f);
            }
        },
        |srv, via| {
            if let Some(cb) = server {
                let json = out(serde_json::json!({ "server": srv.to_json(), "via": via }).to_string());
                cb(ctx, json);
                rv_free(json);
            }
        },
    ));
    rt.shutdown_background();
    match r {
        Ok(rep) => Ok(rep.to_json().to_string()),
        Err(e) => Err(rv_core::explain::explain(&e)),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_bootstrap_hosts(names: *const c_char, mode: *const c_char, err: *mut *mut c_char) -> *mut c_char {
    let run = || -> Result<String, String> {
        rv_net::bootstrap::configure(&s(mode)).map_err(|e| e.to_string())?;
        let names: Vec<String> = serde_json::from_str(&s(names)).map_err(|e| e.to_string())?;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        let map = rt.block_on(async {
            let mut out = serde_json::Map::new();
            for chunk in names.chunks(6) {
                let rs = futures_join(chunk.iter().cloned().map(|h| async move {
                    let r = rv_net::bootstrap::resolve(&h, None, rv_net::bootstrap::mode()).await;
                    (h, r)
                }))
                .await;
                for (h, r) in rs {
                    if let Ok(ips) = r {
                        out.insert(h, serde_json::json!(ips.iter().map(ToString::to_string).collect::<Vec<_>>()));
                    }
                }
            }
            out
        });
        rt.shutdown_background();
        Ok(serde_json::Value::Object(map).to_string())
    };
    match run() {
        Ok(r) => out(r),
        Err(e) => {
            set_err(err, &e);
            std::ptr::null_mut()
        }
    }
}

async fn futures_join<F: std::future::Future + 'static>(fs: impl Iterator<Item = F>) -> Vec<F::Output>
where
    F::Output: 'static,
{
    let local = tokio::task::LocalSet::new();
    let handles: Vec<_> = fs.map(|f| local.spawn_local(f)).collect();
    local
        .run_until(async move {
            let mut v = Vec::with_capacity(handles.len());
            for h in handles {
                if let Ok(r) = h.await {
                    v.push(r);
                }
            }
            v
        })
        .await
}

#[unsafe(no_mangle)]
pub extern "C" fn rv_share_link(config: *const c_char, remarks: *const c_char) -> *mut c_char {
    let Ok(all) = rv_config::parse_subscription(&s(config)) else { return std::ptr::null_mut() };
    let name = s(remarks);
    rv_config::find_profile(&all, &name)
        .and_then(|p| p.main_outbound().and_then(|o| rv_core::link::share_link(o, &p.remarks)))
        .map_or(std::ptr::null_mut(), out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn network_change_forgets_lookups_like_android() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        rv_net::bootstrap::configure("auto").unwrap();
        let tries = Arc::new(AtomicUsize::new(0));
        let t = tries.clone();
        let protect: rv_net::Protect = Arc::new(move |_| {
            t.fetch_add(1, SeqCst);
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "DoH blocked in test"))
        });
        rv_net::resolve_protected("localhost", Some(&protect)).await.expect("system answer");
        let before = tries.load(SeqCst);
        assert!(before > 0, "DoH is tried first");
        rv_net::resolve_protected("localhost", Some(&protect)).await.unwrap();
        assert_eq!(tries.load(SeqCst), before, "DoH rests after the failure");
        let (stop, _) = oneshot::channel();
        let (reconf, _) = tokio::sync::mpsc::unbounded_channel();
        let (resolve, _) = tokio::sync::mpsc::unbounded_channel();
        let mut core = Core {
            stop: Some(stop),
            thread: None,
            network: watch::channel(0).0,
            started: Instant::now(),
            system_dns: Default::default(),
            generation: 0,
            stopping: Arc::new(AtomicBool::new(false)),
            reconf,
            resolve,
        };
        let dns = CString::new("[\"192.0.2.53\"]").unwrap();
        rv_network_changed((&mut core as *mut Core).cast(), dns.as_ptr());
        assert_eq!(*core.system_dns.lock().unwrap(), vec!["192.0.2.53:53".parse::<SocketAddr>().unwrap()]);
        rv_net::resolve_protected("localhost", Some(&protect)).await.unwrap();
        assert!(tries.load(SeqCst) > before, "в новой сети DoH пробуется снова");
    }
}
