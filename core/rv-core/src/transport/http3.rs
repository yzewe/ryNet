use crate::Error;
use rv_config::{Outbound, QuicParams, Security, UdpMask};
use rv_net::Protect;
use rv_xhttp::transport::H3Connector;
use std::{
    io,
    net::SocketAddr,
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

pub(super) fn connector(
    ob: &Outbound,
    address: &str,
    port: u16,
    protect: Option<Protect>,
    keepalive: i64,
) -> Result<(H3Connector, Option<Duration>), Error> {
    let Security::Tls(tls) = &ob.stream.security else {
        return Err(Error::Config("xhttp h3 requires TLS".into()));
    };
    let name = if tls.server_name.is_empty() { address } else { &tls.server_name }.to_string();
    let pins = rv_tls::Pins { sha256: tls.pinned_sha256.clone(), names: tls.verify_names.clone() };
    let crypto = crate::quic::crypto(&name, tls.allow_insecure, &pins, &[b"h3".to_vec()])?;
    let mut q = ob.stream.finalmask.quic.clone().unwrap_or_else(QuicParams::xhttp);
    q.max_stream_window = q.max_stream_window.max(q.init_stream_window);
    q.max_conn_window = q.max_conn_window.max(q.init_conn_window);
    if (1..8).contains(&q.max_incoming_streams) {
        return Err(Error::Config("maxIncomingStreams must be at least 8".into()));
    }
    if q.keep_alive_period == 0 {
        q.keep_alive_period = match keepalive {
            0 => 10,
            n if n > 0 => n as u64,
            _ => 0,
        };
    }
    if !cfg!(any(target_os = "linux", target_os = "android", target_os = "windows", target_os = "macos", target_os = "ios")) {
        q.disable_path_mtu_discovery = true;
    }
    if !matches!(q.congestion.as_str(), "" | "bbr" | "reno" | "force-brutal") {
        return Err(Error::Config("xhttp h3: invalid QUIC congestion".into()));
    }
    let mut mask = None;
    for m in &ob.stream.finalmask.udp {
        match m {
            UdpMask::Salamander { password, packet_size } if mask.is_none() => mask = Some((password.clone(), *packet_size)),
            _ => return Err(Error::Unsupported("xhttp h3: unsupported UDP mask".into())),
        }
    }
    let bps = Arc::new(AtomicU64::new(if q.congestion == "force-brutal" { q.brutal_up } else { 0 }));
    let cfg = crate::quic::client_config(crypto, &q, q.max_incoming_streams.max(0) as u64, false, bps)?;
    let address = address.to_string();
    let family = ob.stream.sockopt.family;
    let keepalive = (q.keep_alive_period > 0).then(|| Duration::from_secs(q.keep_alive_period));
    let connect: H3Connector = Rc::new(move || {
        let (address, name, protect, q, mask, cfg) = (address.clone(), name.clone(), protect.clone(), q.clone(), mask.clone(), cfg.clone());
        Box::pin(async move {
            let dial = async {
                let ips = rv_net::resolve_protected(&address, protect.as_ref()).await?;
                let ip = crate::outbound::dial::udp_ip(&ips, family)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "xhttp h3: no address"))?;
                let port = if q.udp_hop_ports.is_empty() {
                    port
                } else {
                    use ring::rand::SecureRandom;
                    let mut r = [0; 4];
                    let _ = ring::rand::SystemRandom::new().fill(&mut r);
                    q.udp_hop_ports[u32::from_le_bytes(r) as usize % q.udp_hop_ports.len()]
                };
                let endpoint = crate::quic::socket_endpoint(SocketAddr::new(ip, port), &q, mask.as_ref(), protect.as_ref(), 0)?;
                let t0 = std::time::Instant::now();
                rv_stats::add(rv_stats::Slot::Handshakes, 1);
                let conn = endpoint.connect_with(cfg, SocketAddr::new(ip, port), &name).map_err(io::Error::other)?.await;
                tracing::debug!(target: "rv::dial", "{ip}:{port} quic_ms={} keepalive_s={} idle_s={} err={}", t0.elapsed().as_millis(), q.keep_alive_period, q.max_idle_timeout, conn.as_ref().err().map(ToString::to_string).unwrap_or_default());
                let conn = conn.map_err(io::Error::other)?;
                Ok((conn, endpoint))
            };
            tokio::time::timeout(Duration::from_secs(10), dial).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
        })
    });
    Ok((connect, keepalive))
}

#[cfg(test)]
mod tests;
