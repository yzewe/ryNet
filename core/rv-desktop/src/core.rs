use std::net::{IpAddr, SocketAddr};
use std::rc::Rc;
use std::time::Duration;

use rv_core::engine::{Engine, Options, RoutingMode, TUNNEL_DNS, tunnel_dns};
use rv_host::EngineHandler;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use crate::{emit, owner, proxy, tun};

pub enum Cmd {
    Start { text: String, profile: String, options: Value, tun: bool, port: u16, kill_switch: bool, mtu: usize },
    Stop,
    Ping { generation: u64, text: String, names: Vec<String>, mode: i32, parallel: usize, timeout_ms: u64, url: String, tune: Value },
    Probe { text: String, profile: String, options: Value, req: Value },
    Countries { text: String, names: Vec<String>, parallel: usize, timeout_ms: u64, tune: Value, req: Value },
    Direct { url: String, timeout_ms: u64, req: Value },
    Block,
    Stats(bool),
    Network,
}

fn stats_interval() -> tokio::time::Interval {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tick
}

pub static SPEED_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static PING_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn speedtest(text: String, profile: String, options: Value) {
    SPEED_CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
    std::thread::spawn(move || {
        let r = (|| -> Result<rv_core::speedtest::Report, String> {
            let p = prepared(&text, &profile, &options)?;
            let protect = tun::capable().then(tun::protect);
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
            let r = rt.block_on(rv_core::speedtest::run(
                &p,
                protect,
                &SPEED_CANCEL,
                |phase, v, f| emit(json!({ "ev": "speed", "phase": phase as i32, "value": v, "fraction": f })),
                |server, via| emit(json!({ "ev": "speedServer", "server": server.to_json(), "via": via })),
            ));
            rt.shutdown_background();
            r.map_err(|e| rv_core::explain::explain(&e))
        })();
        emit(match r {
            Ok(rep) => json!({ "ev": "speedDone", "report": rep.to_json() }),
            Err(e) => json!({ "ev": "speedDone", "error": e }),
        });
    });
}

struct Session {
    task: tokio::task::JoinHandle<()>,
    stop: Option<oneshot::Sender<()>>,
    tun: bool,
    handler: Option<Rc<EngineHandler>>,
    engine: Option<Rc<Engine>>,
    network: Option<tokio::sync::watch::Sender<u64>>,
    mtu: usize,
    kill_switch: bool,
    ipv4_only: bool,
}

impl Session {
    fn network_changed(&self) {
        tracing::info!(target: "rv::core", "network changed: pools reset");
        match (&self.handler, &self.engine) {
            (Some(h), _) => h.current().reset_pools(),
            (None, Some(e)) => e.reset_pools(),
            (None, None) => {}
        }
        if let Some(n) = &self.network {
            n.send_modify(|n| *n += 1);
        }
    }
}

async fn end(s: Session, down: bool) {
    let Session { task, stop, tun, .. } = s;
    if let Some(stop) = stop {
        let _ = stop.send(());
    }
    task.abort();
    let _ = task.await;
    if tun && down {
        tun::down();
    }
}

fn fail(what: &str, message: &str, blocked: bool, kept: bool) {
    if kept {
        tracing::warn!(target: "rv::core", "{what}: {message}");
    } else {
        tracing::error!(target: "rv::core", "{what}: {message}");
    }
    emit(json!({ "ev": "error", "message": message, "blocked": blocked, "kept": kept }));
}

fn block_after_failure() -> bool {
    match tun::block() {
        Ok(()) => true,
        Err(e) => {
            tracing::error!(target: "rv::core", "kill switch: {e}");
            false
        }
    }
}

pub fn spawn() -> mpsc::UnboundedSender<Cmd> {
    rv_net::set_system_servers(network_dns);
    #[cfg(windows)]
    rv_net::set_direct_v6(tun::direct_v6);
    let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
    std::thread::Builder::new()
        .name("rv-core".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                let mut running: Option<Session> = None;
                let mut stats = true;
                let mut tick = stats_interval();
                loop {
                    let cmd = tokio::select! {
                        c = rx.recv() => match c { Some(c) => c, None => break },
                        _ = tick.tick(), if stats && running.is_some() => {
                            use rv_stats::{Slot, get};
                            emit(json!({ "ev": "stats", "up": get(Slot::Up), "down": get(Slot::Down), "tcp": get(Slot::TcpActive) }));
                            continue;
                        }
                    };
                    match cmd {
                        Cmd::Stats(on) => stats = on,
                        Cmd::Network => {
                            rv_net::forget_lookups();
                            if let Some(s) = &running {
                                s.network_changed();
                            }
                        }
                        Cmd::Start { text, profile, options, tun, port, kill_switch, mtu } => {
                            let protect = (tun || tun::capable()).then(tun::protect);
                            if let Err(e) = check(&text, &profile, &options, protect).await {
                                let kept = running.is_some();
                                let blocked = !kept && tun && kill_switch && block_after_failure();
                                fail("connect", &e, blocked, kept);
                                continue;
                            }
                            if let Some(s) = running.as_ref().filter(|s| {
                                s.tun
                                    && tun
                                    && s.mtu == mtu
                                    && s.kill_switch == kill_switch
                                    && s.ipv4_only == options.get("ipv4Only").and_then(Value::as_bool).unwrap_or(false)
                            }) && let Some(h) = s.handler.clone()
                            {
                                match engine(&text, &profile, &options, true) {
                                    Ok(e) => {
                                        let nets = direct_nets(&e);
                                        let capture = e.tunnel_nets();
                                        h.swap(e);
                                        tun::capture(&capture);
                                        tun::direct(&nets);
                                        emit(json!({ "ev": "started", "mode": "tun", "port": port }));
                                    }
                                    Err(e) => fail("switch", &e, false, true),
                                }
                                continue;
                            }
                            if let Some(s) = running.take() {
                                end(s, !(tun && kill_switch)).await;
                            }
                            match start(&text, &profile, &options, tun, port, kill_switch, mtu).await {
                                Ok(s) => {
                                    running = Some(s);
                                    emit(json!({ "ev": "started", "mode": if tun { "tun" } else { "proxy" }, "port": port }));
                                }
                                Err(e) => {
                                    let blocked = tun && kill_switch && block_after_failure();
                                    fail("start", &e, blocked, false);
                                }
                            }
                        }
                        Cmd::Stop => {
                            if let Some(s) = running.take() {
                                end(s, true).await;
                            }
                            tun::down();
                            emit(json!({ "ev": "stopped" }));
                        }
                        Cmd::Block => {
                            if let Some(s) = running.take() {
                                end(s, false).await;
                            }
                            let blocked = block_after_failure();
                            fail("health", rv_core::lang::t("Сервер перестал отвечать", "Server stopped responding"), blocked, false);
                        }
                        Cmd::Ping { generation, text, names, mode, parallel, timeout_ms, url, tune } => {
                            let protect = tun::capable().then(tun::protect);
                            tokio::task::spawn_local(ping(generation, text, names, mode, parallel, timeout_ms, url, tune, protect));
                        }
                        Cmd::Countries { text, names, parallel, timeout_ms, tune, req } => {
                            let protect = tun::capable().then(tun::protect);
                            tokio::task::spawn_local(countries(text, names, parallel, timeout_ms, tune, req, protect));
                        }
                        Cmd::Probe { text, profile, options, req } => {
                            let protect = tun::capable().then(tun::protect);
                            tokio::task::spawn_local(async move {
                                emit(match probe(&text, &profile, &options, protect).await {
                                    Ok(ms) => json!({ "ev": "probe", "req": req, "ms": ms, "activeFlows": active_flows() }),
                                    Err(e) => json!({ "ev": "probe", "req": req, "error": e, "activeFlows": active_flows() }),
                                });
                            });
                        }
                        Cmd::Direct { url, timeout_ms, req } => {
                            let protect = tun::capable().then(tun::protect);
                            tokio::task::spawn_local(async move {
                                let r = match rv_core::ping::ProbeUrl::parse(&url) {
                                    Some(u) => rv_core::ping::direct(&u, protect, Duration::from_millis(timeout_ms.clamp(1000, 30_000)))
                                        .await
                                        .map_err(|e| rv_core::explain::explain(&e)),
                                    None => Err(format!("bad url: {url}")),
                                };
                                emit(match r {
                                    Ok(ms) => json!({ "ev": "direct", "req": req, "ms": ms }),
                                    Err(e) => json!({ "ev": "direct", "req": req, "error": e }),
                                });
                            });
                        }
                    }
                }
                if let Some(s) = running.take() {
                    end(s, true).await;
                }
            });
        })
        .expect("core thread");
    tx
}

fn network_dns() -> Vec<std::net::IpAddr> {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<std::net::IpAddr>)>> = std::sync::Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, ips)) = c.as_ref()
        && at.elapsed() < Duration::from_secs(5)
    {
        return ips.clone();
    }
    let ips = tun::system_dns();
    *c = Some((std::time::Instant::now(), ips.clone()));
    ips
}

fn system_dns() -> Vec<SocketAddr> {
    tun::system_dns().into_iter().map(|ip| SocketAddr::new(ip, 53)).collect()
}

fn text<'a>(o: &'a Value, k: &str) -> &'a str {
    o.get(k).and_then(Value::as_str).unwrap_or_default()
}

fn server_dns(options: &Value) {
    let _ = rv_net::bootstrap::configure(options.get("serverDns").and_then(Value::as_str).unwrap_or("auto"));
}

fn prepared(config: &str, profile: &str, options: &Value) -> Result<rv_config::Profile, String> {
    server_dns(options);
    let profiles = rv_config::parse_subscription(config).map_err(|e| e.to_string())?;
    let mut p = rv_config::find_profile(&profiles, profile).ok_or_else(|| {
        if rv_core::lang::english() { format!("No server \"{profile}\"") } else { format!("Нет сервера «{profile}»") }
    })?;
    rv_core::tune::Tune::from_json(options)?.apply(&mut p)?;
    Ok(p)
}

async fn probe(config: &str, profile: &str, options: &Value, protect: Option<rv_net::Protect>) -> Result<u32, String> {
    let p = prepared(config, profile, options)?;
    let url = rv_core::ping::ProbeUrl::parse(text(options, "pingUrl")).unwrap_or_default();
    let timeout = Duration::from_secs(options.get("pingTimeout").and_then(Value::as_u64).filter(|&t| t > 0).unwrap_or(5).max(5));
    rv_core::ping::ping_url(&p, rv_core::ping::Mode::Proxy, protect, timeout, &url).await.map_err(|e| rv_core::explain::explain(&e))
}

async fn check(config: &str, profile: &str, options: &Value, protect: Option<rv_net::Protect>) -> Result<(), String> {
    let mut why = String::new();
    for attempt in 0..2 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        match probe(config, profile, options, protect.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) => why = e,
        }
    }
    Err(why)
}

fn connection_options(options: &Value, use_tun: bool) -> Result<Options, String> {
    let debug = options.get("debugLog").and_then(Value::as_bool).unwrap_or(false);
    let user_rules = rv_core::procroute::wrap(text(options, "userRules"), text(options, "processes"), text(options, "processesMode"))?;
    let user_rules = rv_core::portroute::wrap(&user_rules, text(options, "directPorts"))?;
    Ok(Options {
        user_rules,
        routing: RoutingMode::from_name(text(options, "routingMode")).unwrap_or_default(),
        system_dns: std::sync::Arc::new(std::sync::Mutex::new(system_dns())),
        fake_dns: !use_tun || options.get("fakeDns").and_then(Value::as_bool).unwrap_or(false),
        ipv4_only: options.get("ipv4Only").and_then(Value::as_bool).unwrap_or(false),
        owner: Some(owner::owner()),
        log_apps: debug,
        sub_routing: options.get("subRouting").filter(|v| v.is_object()).map(crate::geodat::localize),
        on_failing: Some(std::sync::Arc::new(|| emit(json!({ "ev": "failing" })))),
        dns: options
            .get("dns")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        background: false,
    })
}

pub fn routing(config: &str, profile: &str, options: &Value, use_tun: bool) -> Result<Value, String> {
    let profiles = rv_config::parse_subscription(config).map_err(|e| e.to_string())?;
    let mut p = rv_config::find_profile(&profiles, profile).ok_or_else(|| "no such profile".to_string())?;
    rv_core::tune::Tune::from_json(options)?.apply(&mut p)?;
    let opts = connection_options(options, use_tun)?;
    if use_tun {
        #[cfg(unix)]
        let search = tun::search_domains();
        #[cfg(windows)]
        let search = Vec::new();
        tunnel_dns(&mut p, &opts, &search);
    }
    let geo = rv_core::geo::load_dir(&crate::store::geo_dir());
    Engine::inspect(&p, &geo, &opts).map_err(|e| e.to_string())
}

fn engine(config: &str, profile: &str, options: &Value, use_tun: bool) -> Result<Rc<Engine>, String> {
    let mut p = prepared(config, profile, options)?;
    let geo = rv_core::geo::load_dir(&crate::store::geo_dir());
    let opts = connection_options(options, use_tun)?;
    if use_tun {
        #[cfg(unix)]
        let search = tun::search_domains();
        #[cfg(windows)]
        let search = Vec::new();
        if tunnel_dns(&mut p, &opts, &search) {
            tracing::info!(target: "rv::core", "dns: none set, names go to {TUNNEL_DNS} through the tunnel, local ones to the system resolver");
        }
    }
    let engine = Rc::new(Engine::new(&p, &geo, use_tun.then(tun::protect), &opts).map_err(|e| e.to_string())?);
    for w in &engine.warnings {
        tracing::warn!("{w}");
    }
    if engine.has_balancers() {
        let e = engine.clone();
        tokio::task::spawn_local(e.watch_balancers());
    }
    tracing::info!(target: "rv::core", "engine profile={profile:?} routing={:?} tun={use_tun}", opts.routing);
    tracing::info!(target: "rv::core", "routing {}", engine.routing_summary);
    Ok(engine)
}

async fn start(
    config: &str,
    profile: &str,
    options: &Value,
    use_tun: bool,
    port: u16,
    kill_switch: bool,
    mtu: usize,
) -> Result<Session, String> {
    let engine = engine(config, profile, options, use_tun)?;
    let ipv4_only = options.get("ipv4Only").and_then(Value::as_bool).unwrap_or(false);
    rv_stats::reset(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64));
    if use_tun {
        let (stop_tx, stop_rx) = oneshot::channel();
        let (net_tx, net_rx) = tokio::sync::watch::channel(0u64);
        let nets = direct_nets(&engine);
        let cfg = tun::config(mtu, kill_switch, ipv4_only, net_rx).await?;
        tun::capture(&engine.tunnel_nets());
        tun::direct(&nets);
        #[cfg(unix)]
        let fd = tun::FdGuard::new(cfg.fd);
        let handler = Rc::new(EngineHandler::new(engine));
        let h = handler.clone();
        let task = tokio::task::spawn_local(async move {
            #[cfg(unix)]
            fd.release();
            if let Err(e) = rv_tun::run(cfg, h, stop_rx, |_| {}).await {
                fail("tun", &format!("TUN: {e}"), kill_switch, false);
            }
        });
        return Ok(Session {
            task,
            stop: Some(stop_tx),
            tun: true,
            handler: Some(handler),
            engine: None,
            network: Some(net_tx),
            mtu,
            kill_switch,
            ipv4_only,
        });
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        let busy = if rv_core::lang::english() { format!("Port {port} is busy") } else { format!("Порт {port} занят") };
        if e.kind() == std::io::ErrorKind::AddrInUse { busy } else { format!("{busy}: {e}") }
    })?;
    let task = tokio::task::spawn_local(serve(engine.clone(), listener));
    Ok(Session { task, stop: None, tun: false, handler: None, engine: Some(engine), network: None, mtu, kill_switch, ipv4_only })
}

const DIRECT_NETS_MAX: usize = 64;

fn direct_nets(e: &Engine) -> Vec<(IpAddr, u8)> {
    let own = (IpAddr::V4(tun::V4.0), 30);
    let mut nets: Vec<(IpAddr, u8)> = e.direct_nets().iter().copied().filter(|&n| !overlaps(n, own)).collect();
    if nets.len() > DIRECT_NETS_MAX {
        tracing::warn!(target: "rv::core", "tun: {} direct nets, only the first {DIRECT_NETS_MAX} bypass the tunnel", nets.len());
        nets.truncate(DIRECT_NETS_MAX);
    }
    nets
}

pub fn overlaps((a, la): (IpAddr, u8), (b, lb): (IpAddr, u8)) -> bool {
    let len = u32::from(la.min(lb));
    match (a, b) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let m = u32::MAX.checked_shl(32 - len.min(32)).unwrap_or(0);
            u32::from(a) & m == u32::from(b) & m
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let m = u128::MAX.checked_shl(128 - len.min(128)).unwrap_or(0);
            u128::from(a) & m == u128::from(b) & m
        }
        _ => false,
    }
}

pub fn covers(outer: (IpAddr, u8), inner: (IpAddr, u8)) -> bool {
    outer.1 <= inner.1 && overlaps(outer, inner)
}

async fn serve(engine: Rc<Engine>, listener: TcpListener) {
    let mut conns = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            a = listener.accept() => {
                let Ok((s, src)) = a else { continue };
                let engine = engine.clone();
                conns.spawn_local(async move {
                    if let Err(e) = proxy::handle(&engine, s, src).await {
                        tracing::debug!("proxy conn: {e}");
                    }
                });
            }
            Some(_) = conns.join_next(), if !conns.is_empty() => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn ping(
    generation: u64,
    text: String,
    names: Vec<String>,
    mode: i32,
    parallel: usize,
    timeout_ms: u64,
    url: String,
    tune: Value,
    protect: Option<rv_net::Protect>,
) {
    let Ok(all) = rv_config::parse_subscription_dial(&text) else {
        emit(json!({ "ev": "pingDone", "generation": generation }));
        return;
    };
    server_dns(&tune);
    let tune = rv_core::tune::Tune::from_json(&tune).unwrap_or_default();
    let (mut profiles, mut order) = (Vec::new(), Vec::new());
    for name in names {
        let Some(mut p) = rv_config::find_profile(&all, &name) else { continue };
        match tune.apply(&mut p) {
            Ok(()) => {
                profiles.push(p);
                order.push(name);
            }
            Err(e) => emit(json!({ "ev": "ping", "generation": generation, "name": name, "error": e })),
        }
    }
    PING_CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
    let opts = rv_core::ping::Opts {
        parallel,
        timeout: Duration::from_millis(if timeout_ms > 0 { timeout_ms } else { 5000 }),
        url: rv_core::ping::ProbeUrl::parse(&url).unwrap_or_default(),
        cancel: Some(&PING_CANCEL),
    };
    rv_core::ping::ping_stream(profiles, rv_core::ping::Mode::from_code(mode), protect, opts, |i, r| {
        emit(match r {
            Ok(ms) => json!({ "ev": "ping", "generation": generation, "name": order[i], "ms": ms }),
            Err(e) => json!({ "ev": "ping", "generation": generation, "name": order[i], "error": e }),
        });
    })
    .await;
    emit(json!({ "ev": "pingDone", "generation": generation }));
}

async fn countries(
    text: String,
    names: Vec<String>,
    parallel: usize,
    timeout_ms: u64,
    tune: Value,
    req: Value,
    protect: Option<rv_net::Protect>,
) {
    let all = rv_config::parse_subscription_dial(&text).unwrap_or_default();
    server_dns(&tune);
    let tune = rv_core::tune::Tune::from_json(&tune).unwrap_or_default();
    let (order, profiles): (Vec<String>, Vec<rv_config::Profile>) = names
        .into_iter()
        .filter_map(|n| {
            let mut p = rv_config::find_profile(&all, &n)?;
            tune.apply(&mut p).ok().map(|()| (n, p))
        })
        .unzip();
    let timeout = Duration::from_millis(if timeout_ms > 0 { timeout_ms } else { 5000 });
    let found = rv_core::ping::countries(profiles, protect, parallel, timeout).await;
    let map: serde_json::Map<String, Value> = order.into_iter().zip(found).filter_map(|(n, c)| Some((n, json!(c?)))).collect();
    emit(json!({ "ev": "countries", "req": req, "countries": map }));
}

pub fn active_flows() -> u64 {
    rv_stats::get(rv_stats::Slot::TcpActive).saturating_add(rv_stats::get(rv_stats::Slot::UdpActive))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn stats_tick_does_not_burst_after_idle() {
        let mut tick = super::stats_interval();
        tick.tick().await;
        tokio::time::advance(Duration::from_secs(3600)).await;
        let mut immediate = 0;
        while immediate < 10 && tokio::time::timeout(Duration::ZERO, tick.tick()).await.is_ok() {
            immediate += 1;
        }
        assert!(immediate <= 1, "{immediate} stats ticks fired back to back after an hour without a session");
    }

    use rv_core::dns::{TYPE_A, query_for};
    use rv_core::engine::{DnsHop, DnsTarget, Engine, Options, RoutingMode};
    use serde_json::json;

    fn profile() -> rv_config::Profile {
        rv_config::parse_subscription("vless://9205c503-358f-4d8d-a081-20c98eddf64a@192.0.2.1:443?type=tcp&security=none#t")
            .unwrap()
            .remove(0)
    }

    fn opts(routing: RoutingMode) -> Options {
        Options {
            routing,
            system_dns: std::sync::Arc::new(std::sync::Mutex::new(vec!["192.168.1.1:53".parse().unwrap()])),
            ..Options::default()
        }
    }

    fn hops(e: &Engine, name: &str) -> Vec<DnsHop> {
        match e.dns_route(&query_for(name, TYPE_A)) {
            DnsTarget::Servers(h) => h.into_iter().map(|(h, _)| h).collect(),
            t => panic!("{name}: {t:?}"),
        }
    }

    #[test]
    fn tun_names_skip_the_system_resolver_unless_local() {
        let system = DnsHop::Direct("192.168.1.1:53".parse().unwrap());
        for routing in [RoutingMode::Subscription, RoutingMode::Smart, RoutingMode::Global] {
            let o = opts(routing);
            let mut p = profile();
            assert!(rv_core::engine::tunnel_dns(&mut p, &o, &["fritz.box".into(), "corp.example".into()]));
            let e = Engine::new(&p, &Default::default(), None, &o).unwrap();
            let public = hops(&e, "example.com");
            assert!(!public.is_empty() && !public.contains(&system), "{routing:?}: {public:?}");
            let DnsHop::Stream(i) = public[0] else { panic!("{routing:?}: {public:?}") };
            assert_eq!(e.dns_label(i), "1.1.1.1:53");
            for local in [
                "nas",
                "nas.lan",
                "router.home.arpa",
                "box.fritz.box",
                "wiki.corp.example",
                "5.1.168.192.in-addr.arpa",
                "1.0.20.172.in-addr.arpa",
            ] {
                assert_eq!(hops(&e, local)[0], system, "{routing:?}: {local}");
            }
            assert!(!hops(&e, "1.0.40.172.in-addr.arpa").contains(&system));
        }
        let o = opts(RoutingMode::Smart);
        let mut p = profile();
        rv_core::engine::tunnel_dns(&mut p, &o, &[]);
        let e = Engine::new(&p, &Default::default(), None, &o).unwrap();
        let DnsHop::Stream(i) = hops(&e, "ya.ru")[0] else { panic!() };
        assert_eq!(e.dns_label(i), rv_core::engine::SMART_RU_DNS);
    }

    #[test]
    fn tun_direct_nets_skip_own_and_are_capped() {
        let own: String = ["IP-CIDR,172.19.0.0/16,DIRECT".to_string(), "IP-CIDR6,fd7a:115c:a1e0::/48,DIRECT".into()]
            .into_iter()
            .chain((0..80).map(|i| format!("IP-CIDR,100.{}.0.0/16,DIRECT", 64 + i)))
            .collect::<Vec<_>>()
            .join("\n");
        let o = Options { user_rules: own, ..opts(RoutingMode::Global) };
        let e = Engine::new(&profile(), &Default::default(), None, &o).unwrap();
        let nets = super::direct_nets(&e);
        assert_eq!(nets.len(), super::DIRECT_NETS_MAX);
        assert_eq!(nets[0], ("fd7a:115c:a1e0::".parse().unwrap(), 48));
        assert_eq!(nets[1], ("100.64.0.0".parse().unwrap(), 16));
        assert!(
            super::covers(("100.64.0.0".parse().unwrap(), 10), nets[1]) && !super::covers(nets[1], ("100.64.0.0".parse().unwrap(), 10))
        );
        assert!(super::overlaps(nets[1], ("100.0.0.0".parse().unwrap(), 8)) && !super::overlaps(nets[0], nets[1]));
    }

    #[test]
    fn tun_dns_set_elsewhere_stays() {
        let mut o = opts(RoutingMode::Subscription);
        o.dns = vec!["8.8.8.8".into()];
        let mut p = profile();
        assert!(!rv_core::engine::tunnel_dns(&mut p, &o, &[]));
        assert!(p.dns.is_none());
        let mut p = profile();
        p.dns = Some(json!({ "servers": ["8.8.8.8"] }));
        assert!(!rv_core::engine::tunnel_dns(&mut p, &opts(RoutingMode::Global), &[]));
        assert_eq!(p.dns, Some(json!({ "servers": ["8.8.8.8"] })));
        let mut o = opts(RoutingMode::Subscription);
        o.sub_routing = Some(json!({ "RemoteDNSType": "DoU", "RemoteDNSIP": "8.8.4.4" }));
        let mut p = profile();
        assert!(!rv_core::engine::tunnel_dns(&mut p, &o, &[]));
        assert!(p.dns.is_none());
        let mut p = profile();
        p.dns = Some(json!({ "hosts": { "a.test": "192.0.2.7" } }));
        assert!(rv_core::engine::tunnel_dns(&mut p, &opts(RoutingMode::Global), &[]));
        assert_eq!(p.dns.as_ref().unwrap()["hosts"]["a.test"], "192.0.2.7");
    }
}
