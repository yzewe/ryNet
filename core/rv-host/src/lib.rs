use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use rv_core::engine::Engine;
use rv_stats::Slot;
use rv_tun::{BoxStream, Handler, LocalBoxFuture, TcpConn};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub struct Counted<S>(pub S);

const TYPE_TXT: u16 = 16;

const SYSTEM_DNS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);

pub fn local_first(engine: &Engine, target: rv_core::engine::DnsTarget, system: SocketAddr, name: &str) -> rv_core::engine::DnsTarget {
    use rv_core::engine::{DnsHop, DnsTarget, Out};
    let first = (DnsHop::Direct(system), SYSTEM_DNS_TIMEOUT);
    if rv_core::dns::is_local_name(name) {
        return match target {
            DnsTarget::Reply(r) if !rv_core::dns::addresses(&r).into_iter().any(rv_core::dns::fake::is_fake) => DnsTarget::Reply(r),
            DnsTarget::Lookup(n) => DnsTarget::Lookup(n),
            _ => DnsTarget::Servers(vec![first]),
        };
    }
    let DnsTarget::Servers(mut hops) = target else { return target };
    if !matches!(hops.first(), Some((DnsHop::Stream(_), _))) {
        return DnsTarget::Servers(hops);
    }
    let ctx = rv_rules::RouteCtx {
        domain: Some(name),
        ip: None,
        port: 443,
        network: rv_rules::Network::Tcp,
        protocol: None,
        inbound_tag: None,
        process: None,
    };
    if !matches!(engine.route(&ctx), Out::Direct) {
        return DnsTarget::Servers(hops);
    }
    let DnsTarget::Servers(usual) = engine.dns_route(&rv_core::dns::query_for("rynet-usual.invalid", TYPE_TXT)) else {
        return DnsTarget::Servers(hops);
    };
    if usual.first().map(|h| &h.0) != hops.first().map(|h| &h.0) {
        return DnsTarget::Servers(hops);
    }
    hops.insert(0, first);
    DnsTarget::Servers(hops)
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.0).poll_read(cx, buf);
        rv_stats::add(Slot::Up, (buf.filled().len() - before) as u64);
        r
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let r = Pin::new(&mut self.0).poll_write(cx, data);
        if let Poll::Ready(Ok(n)) = r {
            rv_stats::add(Slot::Down, n as u64);
        }
        r
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl rv_core::pipe::Local for Counted<TcpConn> {
    fn zero_copy(&self) -> bool {
        self.0.zero_copy()
    }
    fn poll_fill(&mut self, cx: &mut Context<'_>, src: Pin<&mut dyn AsyncRead>) -> Poll<io::Result<usize>> {
        let r = self.0.poll_fill(cx, src);
        if let Poll::Ready(Ok(n)) = r {
            rv_stats::add(Slot::Down, n as u64);
        }
        r
    }
    fn poll_drain(&mut self, cx: &mut Context<'_>, dst: Pin<&mut dyn AsyncWrite>) -> Poll<io::Result<usize>> {
        let r = self.0.poll_drain(cx, dst);
        if let Poll::Ready(Ok(n)) = r {
            rv_stats::add(Slot::Up, n as u64);
        }
        r
    }
}

impl rv_core::pipe::Local for Counted<tokio::net::TcpStream> {}

pub struct Plain<S>(pub S);

impl<S: AsyncRead + Unpin> AsyncRead for Plain<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Plain<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> rv_core::pipe::Local for Counted<Plain<S>> {}

pub fn count_error(e: &rv_core::Error) {
    use std::io::ErrorKind::*;
    let slot = match e {
        rv_core::Error::Tls(_) => Slot::ErrTls,
        rv_core::Error::Io(io) => match io.kind() {
            TimedOut => Slot::ErrTimeout,
            ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof => Slot::ErrReset,
            ConnectionRefused | HostUnreachable | NetworkUnreachable => Slot::ErrRefused,
            _ => Slot::ErrOther,
        },
        _ => Slot::ErrOther,
    };
    rv_stats::add(slot, 1);
}

fn udp_dns(engine: Rc<Engine>, upstream: usize, server: SocketAddr, failed: impl FnOnce() + 'static) -> BoxStream {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    tokio::task::spawn_local(async move {
        let (mut rd, mut wr) = tokio::io::split(ours);
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(SocketAddr, Vec<u8>)>(64);
        let (got_tx, mut got) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let asked = std::cell::RefCell::new(std::collections::HashMap::<u16, Vec<u8>>::new());
        let answered = std::cell::Cell::new(false);
        let session = async {
            if let Err(e) = engine
                .udp_from(None, &mut rx, &|_, data| {
                    let _ = got_tx.send(data.to_vec());
                })
                .await
            {
                tracing::debug!(target: "rv::dns", "upstream {server} udp: {e}");
            }
        };
        let reader = async {
            let mut len = [0u8; 2];
            while rd.read_exact(&mut len).await.is_ok() {
                let mut msg = vec![0u8; u16::from_be_bytes(len) as usize];
                if rd.read_exact(&mut msg).await.is_err() || msg.len() < 12 {
                    return;
                }
                {
                    let mut asked = asked.borrow_mut();
                    if asked.len() >= 256 {
                        asked.clear();
                    }
                    asked.insert(u16::from_be_bytes([msg[0], msg[1]]), msg.clone());
                }
                if tx.send((server, msg)).await.is_err() {
                    return;
                }
            }
        };
        let writer = async {
            while let Some(mut reply) = got.recv().await {
                if reply.len() < 12 {
                    continue;
                }
                answered.set(true);
                let query = asked.borrow_mut().remove(&u16::from_be_bytes([reply[0], reply[1]]));
                if reply[2] & 0x02 != 0
                    && let Some(q) = query
                    && let Some(full) = tcp_exchange(&engine, upstream, &q).await
                {
                    reply = full;
                }
                let mut frame = (reply.len() as u16).to_be_bytes().to_vec();
                frame.extend_from_slice(&reply);
                if wr.write_all(&frame).await.is_err() {
                    return;
                }
            }
        };
        tokio::select! {
            _ = session => {}
            _ = reader => {}
            _ = writer => {}
        }
        if !answered.get() {
            failed();
        }
    });
    Box::new(theirs)
}

async fn tcp_exchange(engine: &Engine, upstream: usize, query: &[u8]) -> Option<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut s = engine.dns_stream(upstream).await.ok()?;
        let mut frame = (query.len() as u16).to_be_bytes().to_vec();
        frame.extend_from_slice(query);
        s.write_all(&frame).await.ok()?;
        s.flush().await.ok()?;
        let mut len = [0u8; 2];
        s.read_exact(&mut len).await.ok()?;
        let mut r = vec![0u8; u16::from_be_bytes(len) as usize];
        s.read_exact(&mut r).await.ok()?;
        Some(r)
    })
    .await
    .ok()
    .flatten()
}

pub struct EngineHandler {
    engine: std::cell::RefCell<Rc<Engine>>,
    generation: tokio::sync::watch::Sender<u64>,
    system_dns: Option<rv_core::engine::SystemDns>,
    udp_dns_failed: Rc<std::cell::RefCell<std::collections::HashSet<(u64, usize)>>>,
}

impl EngineHandler {
    pub fn new(engine: Rc<Engine>) -> Self {
        Self {
            engine: std::cell::RefCell::new(engine),
            generation: tokio::sync::watch::channel(0).0,
            system_dns: None,
            udp_dns_failed: Default::default(),
        }
    }

    pub fn with_system_dns(mut self, dns: rv_core::engine::SystemDns) -> Self {
        self.system_dns = Some(dns);
        self
    }

    fn system_dns(&self) -> Option<SocketAddr> {
        self.system_dns.as_ref()?.lock().ok()?.first().copied()
    }

    pub fn current(&self) -> Rc<Engine> {
        self.engine.borrow().clone()
    }

    pub fn swap(&self, engine: Rc<Engine>) {
        *self.engine.borrow_mut() = engine;
        self.generation.send_modify(|g| *g += 1);
    }
}

impl Handler for EngineHandler {
    fn generation(&self) -> u64 {
        *self.generation.borrow()
    }

    fn tcp(&self, local: TcpConn, src: SocketAddr, dst: SocketAddr) -> LocalBoxFuture<()> {
        let engine = self.current();
        let mut switched = self.generation.subscribe();
        Box::pin(async move {
            local.set_nodelay(true);
            rv_stats::add(Slot::TcpTotal, 1);
            let _active = rv_stats::Active::new(Slot::TcpActive);
            let closer = local.closer();
            let relay = engine.tcp_from(Counted(local), Some(src), dst);
            tokio::pin!(relay);
            tokio::select! {
                r = &mut relay => {
                    if let Err(e) = r {
                        count_error(&e);
                    }
                }
                Ok(()) = switched.changed() => {
                    closer.reset();
                    tracing::debug!(target: "rv::conn", "tcp {dst} reset: server switched");
                }
            }
        })
    }

    fn dns_stream(&self, upstream: usize) -> LocalBoxFuture<std::io::Result<BoxStream>> {
        let engine = self.current();
        let key = (*self.generation.borrow(), upstream);
        let udp = engine.dns_label(upstream).parse::<SocketAddr>().ok().filter(|&server| {
            !engine.dns_pinned(upstream)
                && !self.udp_dns_failed.borrow().contains(&key)
                && engine.udp_route(None, server).is_some_and(|class| class != 1)
        });
        let failed = self.udp_dns_failed.clone();
        Box::pin(async move {
            match udp {
                Some(server) => Ok(udp_dns(engine, upstream, server, move || {
                    tracing::debug!(target: "rv::dns", "upstream {server}: no answer over UDP through the proxy, TCP from now on");
                    failed.borrow_mut().insert(key);
                })),
                None => engine.dns_stream(upstream).await.map(|s| Box::new(s) as BoxStream),
            }
        })
    }

    fn dns_cache(&self) -> bool {
        self.current().dns_cache()
    }

    fn dns_label(&self, upstream: usize) -> String {
        self.current().dns_label(upstream)
    }

    fn dns_route(&self, query: &[u8]) -> rv_tun::DnsRoute {
        use rv_core::engine::{DnsHop, DnsTarget};
        let engine = self.current();
        let mut target = engine.dns_route(query);
        if let (Some(system), Some(q)) = (self.system_dns(), rv_core::dns::question(query)) {
            target = local_first(&engine, target, system, &q.name);
        }
        match target {
            DnsTarget::Servers(hops) => rv_tun::DnsRoute::Servers(
                hops.into_iter()
                    .map(|(hop, t)| match hop {
                        DnsHop::Direct(a) => (rv_tun::DnsHop::Direct(a), t),
                        DnsHop::Stream(i) => (rv_tun::DnsHop::Stream(i), t),
                    })
                    .collect(),
            ),
            DnsTarget::Reply(r) => rv_tun::DnsRoute::Reply(r),
            DnsTarget::Lookup(name) => rv_tun::DnsRoute::Lookup(name),
        }
    }

    fn dns_direct(&self, server: SocketAddr, msg: Vec<u8>) -> LocalBoxFuture<Option<Vec<u8>>> {
        let engine = self.current();
        Box::pin(async move { engine.dns_direct(server, &msg).await })
    }

    fn dns_lookup(&self, query: Vec<u8>, name: String) -> LocalBoxFuture<Option<Vec<u8>>> {
        let engine = self.current();
        Box::pin(async move { engine.dns_lookup(&query, &name).await })
    }

    fn udp_route(&self, src: SocketAddr, dst: SocketAddr) -> Option<u64> {
        self.current().udp_route(Some(src), dst)
    }

    fn udp(&self, mut rx: tokio::sync::mpsc::Receiver<(SocketAddr, Vec<u8>)>, reply: rv_tun::UdpReply) -> LocalBoxFuture<()> {
        let engine = self.current();
        Box::pin(async move {
            let app = reply.app();
            if let Err(e) = engine.udp_from(Some(app), &mut rx, &|from, data| reply.send(from, data)).await {
                tracing::debug!("udp session: {e}");
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rv_core::dns::{TYPE_A, query_for};
    use rv_core::engine::{DnsHop, DnsTarget, Options, RoutingMode};

    fn engine(routing: RoutingMode, fake_dns: bool) -> Engine {
        let p = rv_config::parse_subscription("vless://9205c503-358f-4d8d-a081-20c98eddf64a@192.0.2.1:443?type=tcp&security=none#t")
            .unwrap()
            .remove(0);
        let opts = Options {
            user_rules: "DOMAIN-SUFFIX,direct.example,DIRECT\nIP-CIDR,192.168.1.0/24,DIRECT".into(),
            routing,
            fake_dns,
            dns: vec!["1.1.1.1".into()],
            ..Options::default()
        };
        Engine::new(&p, &Default::default(), None, &opts).unwrap()
    }

    fn hops(e: &Engine, name: &str) -> Vec<DnsHop> {
        match local_first(e, e.dns_route(&query_for(name, TYPE_A)), SYSTEM, name) {
            DnsTarget::Servers(h) => h.into_iter().map(|(h, _)| h).collect(),
            t => panic!("{name}: {t:?}"),
        }
    }

    const SYSTEM: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 1)), 53);

    #[test]
    fn local_and_direct_names_ask_the_system_resolver() {
        let system = DnsHop::Direct(SYSTEM);
        for routing in [RoutingMode::Custom, RoutingMode::Subscription, RoutingMode::Global] {
            let e = engine(routing, false);
            for local in ["router.lan", "nas", "my.keenetic.net", "1.1.168.192.in-addr.arpa"] {
                assert_eq!(hops(&e, local), std::slice::from_ref(&system), "{routing:?} {local}");
            }
            for proxied in ["example.com", "direct.example.org"] {
                let h = hops(&e, proxied);
                assert!(!h.is_empty() && !h.contains(&system), "{routing:?} {proxied}: {h:?}");
            }
        }
        let e = engine(RoutingMode::Custom, false);
        let h = hops(&e, "panel.direct.example");
        assert_eq!(h[0], system);
        assert!(matches!(h[1], DnsHop::Stream(_)), "the tunnel's server stays behind it: {h:?}");
    }

    #[test]
    fn fake_dns_leaves_local_names_real() {
        let e = engine(RoutingMode::Custom, true);
        let fake = |name: &str| match local_first(&e, e.dns_route(&query_for(name, TYPE_A)), SYSTEM, name) {
            DnsTarget::Reply(r) => rv_core::dns::addresses(&r).into_iter().any(rv_core::dns::fake::is_fake),
            _ => false,
        };
        assert!(fake("example.com"));
        assert!(!fake("router.lan") && hops(&e, "router.lan") == [DnsHop::Direct(SYSTEM)]);
        assert!(!fake("nas"));
        assert_eq!(hops(&e, "panel.direct.example")[0], DnsHop::Direct(SYSTEM));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn plain_dns_goes_as_udp_then_falls_back_to_tcp() {
        use rv_tun::Handler;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::task::LocalSet::new()
            .run_until(async {
                let p = rv_config::parse_subscription("vless://9205c503-358f-4d8d-a081-20c98eddf64a@127.0.0.1:1?type=tcp&security=none#t")
                    .unwrap()
                    .remove(0);
                let opts = Options { routing: RoutingMode::Global, dns: vec!["1.1.1.1".into()], ..Options::default() };
                let h = EngineHandler::new(Rc::new(Engine::new(&p, &Default::default(), None, &opts).unwrap()));
                assert_eq!(h.dns_label(0), "1.1.1.1:53");
                let mut s = h.dns_stream(0).await.expect("the UDP stream opens without dialing");
                let q = query_for("a.example", TYPE_A);
                let mut frame = (q.len() as u16).to_be_bytes().to_vec();
                frame.extend_from_slice(&q);
                s.write_all(&frame).await.unwrap();
                let mut b = [0u8; 2];
                assert!(s.read_exact(&mut b).await.is_err(), "no proxy: the stream ends without an answer");
                tokio::task::yield_now().await;
                assert!(h.udp_dns_failed.borrow().contains(&(0, 0)));
                assert!(h.dns_stream(0).await.is_err(), "next time the engine's TCP stream, which fails to dial here");
            })
            .await;
    }

    #[test]
    fn a_server_set_for_the_name_still_decides() {
        let e = engine(RoutingMode::Smart, false);
        let h = hops(&e, "ya.ru");
        assert_ne!(h[0], DnsHop::Direct(SYSTEM), "{h:?}");
    }
}
