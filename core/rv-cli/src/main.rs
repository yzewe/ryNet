use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::rc::Rc;

use anyhow::{Context, bail};
use rv_core::{Destination, Proxy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    match args.first().map(String::as_str) {
        Some("list") => list(args.get(1).context("usage: rv-cli list <sub.json>")?),
        Some("udp") => {
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, udp_test(&args[1..]))
        }
        Some("describe") => {
            let text = std::fs::read_to_string(args.get(1).context("usage: rv-cli describe <sub>")?)?;
            let v = rv_core::describe::describe(&text)?;
            for p in v.as_array().into_iter().flatten() {
                println!(
                    "{:<4} {:<40} {:<28} {}",
                    if p["placeholder"] == true {
                        "--"
                    } else if p["supported"] == true {
                        "ok"
                    } else {
                        "NO"
                    },
                    p["remarks"].as_str().unwrap_or(""),
                    p["kind"].as_str().unwrap_or(""),
                    p["reason"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
        Some("ping") => {
            let text = std::fs::read_to_string(args.get(1).context("usage: rv-cli ping <sub.json> [tcp|proxy]")?)?;
            let mode = match args.get(2).map(String::as_str) {
                Some("tcp") => rv_core::ping::Mode::Tcp,
                Some("icmp") => rv_core::ping::Mode::Icmp,
                Some("proxy2") => rv_core::ping::Mode::ProxyWarm,
                _ => rv_core::ping::Mode::Proxy,
            };
            let profiles = rv_config::parse_subscription(&text)?;
            let names: Vec<String> = profiles.iter().map(|p| p.remarks.clone()).collect();
            let mut opts = rv_core::ping::Opts::default();
            if let Some(u) = std::env::var("RV_PING_URL").ok().and_then(|u| rv_core::ping::ProbeUrl::parse(&u)) {
                opts.url = u;
            }
            let res = rt.block_on(rv_core::ping::ping_stream(profiles, mode, None, opts, |_, _| {}));
            for (n, ms) in names.iter().zip(res) {
                match ms {
                    Ok(m) => println!("{:>6}  {n}", format!("{m} ms")),
                    Err(e) => println!("     —  {n}  ({e})"),
                }
            }
            Ok(())
        }
        Some("speedtest") => {
            let text = std::fs::read_to_string(args.get(1).context("usage: rv-cli speedtest <sub> <name>")?)?;
            let name = args.get(2).context("profile name")?;
            let p = rv_config::find_profile(&rv_config::parse_subscription(&text)?, name).context("no such profile")?;
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let mut last = (u8::MAX, 0.0);
            let r = rt.block_on(rv_core::speedtest::run(
                &p,
                None,
                &cancel,
                |ph, v, f| {
                    if ph as u8 != last.0 || f - last.1 >= 0.2 || f >= 1.0 {
                        last = (ph as u8, f);
                        eprintln!("{ph:?} {v:.1} {:.0}%", f * 100.0);
                    }
                },
                |s, via| eprintln!("server: {} · {}, {} ({}) via {via}", s.sponsor, s.name, s.country, s.host),
            ));
            println!("{:#}", r?.to_json());
            Ok(())
        }
        Some("socks") => {
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, socks(&args[1..]))
        }
        _ => bail!("usage:\n  rv-cli list <sub.json>\n  rv-cli socks <sub.json> <profile> [--outbound TAG] [--listen ADDR]"),
    }
}

fn load(path: &str) -> anyhow::Result<Vec<rv_config::Profile>> {
    Ok(rv_config::parse_subscription(&std::fs::read_to_string(path)?)?)
}

fn list(path: &str) -> anyhow::Result<()> {
    for p in load(path)? {
        let tags: Vec<String> = p
            .outbounds
            .iter()
            .filter(|o| {
                matches!(
                    o.kind,
                    rv_config::OutboundKind::Vless(_) | rv_config::OutboundKind::Vmess(_) | rv_config::OutboundKind::Hysteria2(_)
                )
            })
            .map(|o| o.tag.clone())
            .collect();
        println!("{:<30} {}", p.remarks, tags.join(", "));
    }
    Ok(())
}

async fn socks(args: &[String]) -> anyhow::Result<()> {
    let path = args.first().context("sub.json")?;
    let name = args.get(1).context("profile name")?;
    let mut tag = None;
    let mut listen: SocketAddr = "127.0.0.1:10808".parse()?;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--outbound" => tag = args.get(i + 1).cloned(),
            "--listen" => listen = args.get(i + 1).context("--listen")?.parse()?,
            other => bail!("unknown arg {other}"),
        }
        i += 2;
    }
    let profiles = load(path)?;
    let profile = profiles
        .iter()
        .find(|p| p.remarks == *name || p.remarks.contains(name.as_str()))
        .with_context(|| format!("no profile matching `{name}`"))?;
    let tag = match tag {
        Some(t) => t,
        None => profile.main_outbound().context("profile has no proxy outbound")?.tag.clone(),
    };
    let ob = Rc::new(Proxy::build(profile, &tag, None)?);
    let listener = TcpListener::bind(listen).await?;
    tracing::info!("profile `{}`, outbound `{}`, socks5 on {}", profile.remarks, tag, listen);
    loop {
        let (sock, peer) = listener.accept().await?;
        let ob = ob.clone();
        tokio::task::spawn_local(async move {
            if let Err(e) = handle(sock, &ob).await {
                tracing::warn!("{peer}: {e:#}");
            }
        });
    }
}

async fn handle(mut s: TcpStream, ob: &Proxy) -> anyhow::Result<()> {
    s.set_nodelay(true)?;
    let mut h = [0u8; 2];
    s.read_exact(&mut h).await?;
    if h[0] != 5 {
        bail!("not socks5");
    }
    let mut methods = vec![0u8; h[1] as usize];
    s.read_exact(&mut methods).await?;
    s.write_all(&[5, 0]).await?;
    let mut r = [0u8; 4];
    s.read_exact(&mut r).await?;
    if r[1] != 1 {
        s.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        bail!("socks command {} not supported", r[1]);
    }
    let dest = match r[3] {
        1 => {
            let mut a = [0u8; 4];
            s.read_exact(&mut a).await?;
            Destination::Ipv4(a)
        }
        3 => {
            let n = s.read_u8().await? as usize;
            let mut d = vec![0u8; n];
            s.read_exact(&mut d).await?;
            Destination::Domain(String::from_utf8(d)?)
        }
        4 => {
            let mut a = [0u8; 16];
            s.read_exact(&mut a).await?;
            Destination::Ipv6(a)
        }
        t => bail!("bad atyp {t}"),
    };
    let port = s.read_u16().await?;
    let shown = match &dest {
        Destination::Ipv4(a) => Ipv4Addr::from(*a).to_string(),
        Destination::Ipv6(a) => Ipv6Addr::from(*a).to_string(),
        Destination::Domain(d) => d.clone(),
    };
    let t0 = std::time::Instant::now();
    let remote = match ob.connect(dest, port).await {
        Ok(r) => r,
        Err(e) => {
            s.write_all(&[5, 1, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            return Err(e).with_context(|| format!("connect {shown}:{port}"));
        }
    };
    tracing::info!("{shown}:{port} connected in {:?}", t0.elapsed());
    s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    let (up, down) = rv_core::relay(s, remote).await?;
    tracing::info!("{shown}:{port} closed, up {up} B, down {down} B, {:?}", t0.elapsed());
    Ok(())
}

async fn udp_test(args: &[String]) -> anyhow::Result<()> {
    let [path, name, server, qname, ..] = args else { bail!("rv-cli udp <sub.json> <profile> <dns-ip:port> <name>") };
    let profiles = load(path)?;
    let profile = profiles.iter().find(|p| p.remarks.contains(name.as_str())).context("profile")?;
    let tag = profile.main_outbound().context("no outbound")?.tag.clone();
    let ob = Proxy::build(profile, &tag, None)?;
    let server: std::net::SocketAddr = server.parse()?;
    let mut q = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in qname.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    tx.send((server, q.clone())).await?;
    q[1] = 0x35;
    tx.send((server, q)).await?;
    let t0 = std::time::Instant::now();
    let got = std::cell::RefCell::new(0);
    let reply = |from: std::net::SocketAddr, data: &[u8]| {
        let an = u16::from_be_bytes([data[6], data[7]]);
        println!("{:?}: reply from {from}, id {:02x}{:02x}, {} bytes, {an} answers", t0.elapsed(), data[0], data[1], data.len());
        *got.borrow_mut() += 1;
    };
    let first = rx.recv().await.context("no packet")?;
    let fut = ob.udp_session(first, &mut rx, &reply);
    tokio::select! {
        r = fut => r?,
        _ = async { while *got.borrow() < 2 { tokio::time::sleep(std::time::Duration::from_millis(20)).await } } => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => bail!("timeout"),
    }
    drop(tx);
    Ok(())
}
