use std::cell::RefCell;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::task::{Context, Poll, ready};

use bytes::Bytes;
use rv_plugin_abi::{err, open as kind};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::Ctx;
use super::res::{Dgram, Leftover, Op, Pipe, Prefixed, Res, Shared, Task, io_code};
use crate::BoxStream;

type Fail = (i32, String);

fn denied(msg: impl Into<String>) -> Fail {
    (err::DENIED, msg.into())
}

fn invalid(msg: impl Into<String>) -> Fail {
    (err::INVALID, msg.into())
}

pub(super) fn is_local(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(a) => {
            let o = a.octets();
            a.is_loopback()
                || a.is_private()
                || a.is_link_local()
                || a.is_unspecified()
                || a.is_broadcast()
                || a.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && o[1] & 0xc0 == 64)
                || (o[0] == 198 && o[1] & 0xfe == 18)
                || o[0] >= 240
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            a.is_loopback() || a.is_unspecified() || a.is_multicast() || s[0] & 0xfe00 == 0xfc00 || s[0] & 0xffc0 == 0xfe80
        }
    }
}

fn host_matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => host.len() > suffix.len() + 1 && host.ends_with(suffix) && host.as_bytes()[host.len() - suffix.len() - 1] == b'.',
        None => pattern == host,
    }
}

impl Ctx {
    fn is_server(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.');
        self.server.as_ref().is_some_and(|(h, _)| h.eq_ignore_ascii_case(host))
            || self.user_hosts.iter().any(|h| h.eq_ignore_ascii_case(host))
    }

    pub(super) fn allow_host(&self, host: &str) -> Result<bool, Fail> {
        if host.is_empty() || host.len() > 253 {
            return Err(invalid("empty or too long host"));
        }
        if self.is_server(host) {
            return Ok(true);
        }
        let p = &self.perms;
        let h = host.trim_end_matches('.').to_ascii_lowercase();
        if let Ok(ip) = h.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
            self.allow_ip(ip, false)?;
            if p.net_any || p.hosts.iter().any(|x| x == &h) {
                return Ok(false);
            }
            return Err(denied(format!("{host} is not the plugin's server and not in its permissions")));
        }
        if p.net_any || p.hosts.iter().any(|pat| host_matches(pat, &h)) {
            return Ok(false);
        }
        Err(denied(format!("{host} is not the plugin's server and not in its permissions")))
    }

    pub(super) fn allow_ip(&self, ip: IpAddr, server: bool) -> Result<(), Fail> {
        if server || self.perms.net_lan || !is_local(ip) {
            Ok(())
        } else {
            Err(denied(format!("{ip} is a local address: needs the local network permission")))
        }
    }

    fn dialer(&self, host: &str, port: u16) -> Result<crate::outbound::dial::Dialer, Fail> {
        let profile = self.profile.as_ref().ok_or_else(|| denied("no network here"))?;
        let mut st = rv_config::StreamSettings::default();
        st.sockopt = self.stream_settings.sockopt.clone();
        st.finalmask.tcp = self.stream_settings.finalmask.tcp.clone();
        crate::outbound::dial::Dialer::new(profile, &st, host, port, self.protect.clone()).map_err(|e| (err::FAILED, e.to_string()))
    }

    fn chained(&self) -> bool {
        self.stream_settings
            .sockopt
            .dialer_proxy
            .as_ref()
            .and_then(|dp| self.profile.as_ref()?.outbound(dp))
            .is_some_and(|o| o.kind.is_proxy())
    }

    async fn udp_target(&self, host: &str, port: u16) -> Result<SocketAddr, Fail> {
        if self.profile.is_none() {
            return Err(denied("no network here"));
        }
        if self.chained() {
            return Err((err::UNSUPPORTED, "UDP and QUIC can't go through dialerProxy".into()));
        }
        let server = self.allow_host(host)?;
        let h = host.trim_start_matches('[').trim_end_matches(']');
        let ips = rv_net::resolve_protected(h, self.protect.as_ref()).await.map_err(|e| (io_code(&e), e.to_string()))?;
        for ip in &ips {
            self.allow_ip(*ip, server)?;
        }
        let ip =
            crate::outbound::dial::udp_ip(&ips, self.stream_settings.sockopt.family).ok_or((err::FAILED, format!("{host}: no address")))?;
        Ok(SocketAddr::new(ip, port))
    }

    async fn tcp(&self, host: &str, port: u16) -> Result<BoxStream, Fail> {
        let server = self.allow_host(host)?;
        let h = host.trim_start_matches('[').trim_end_matches(']');
        let mut target = h.to_string();
        if !self.chained() && !server {
            let ips = rv_net::resolve_protected(h, self.protect.as_ref()).await.map_err(|e| (io_code(&e), e.to_string()))?;
            for ip in &ips {
                self.allow_ip(*ip, false)?;
            }
            target = ips.first().ok_or((err::FAILED, format!("{host}: no address")))?.to_string();
        }
        let d = self.dialer(&target, port)?;
        rv_stats::add(rv_stats::Slot::Handshakes, 1);
        d.tcp().await.map(|t| Box::new(t) as BoxStream).map_err(|e| (io_code(&e), e.to_string()))
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or_default()
}

fn port(v: &Value) -> Result<u16, Fail> {
    v.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).filter(|&p| p > 0).ok_or_else(|| invalid("port"))
}

fn strings(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn handle(v: &Value, k: &str) -> Option<u32> {
    v.get(k).and_then(Value::as_u64).and_then(|h| u32::try_from(h).ok())
}

fn pins(v: &Value) -> Result<rv_tls::Pins, Fail> {
    let mut sha256 = Vec::new();
    for p in strings(v, "pins") {
        let p: String = p.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if p.len() != 64 {
            return Err(invalid("pins: sha256 in hex"));
        }
        let mut b = [0u8; 32];
        for (i, out) in b.iter_mut().enumerate() {
            *out = u8::from_str_radix(&p[i * 2..i * 2 + 2], 16).map_err(|_| invalid("pins"))?;
        }
        sha256.push(b);
    }
    Ok(rv_tls::Pins { sha256, names: Vec::new() })
}

fn register(shared: &Rc<Shared>, r: Res) -> Result<u32, Fail> {
    shared.table.borrow_mut().insert(r).ok_or((err::LIMIT, "too many handles".into()))
}

fn take_stream(shared: &Rc<Shared>, h: u32) -> Result<Leftover, Fail> {
    match shared.get(h) {
        Some(Res::Stream(p)) => {
            let l = p.detach()?;
            shared.table.borrow_mut().remove(h);
            Ok(l)
        }
        _ => Err(invalid("`over` is not a stream handle")),
    }
}

pub(super) fn open(ctx: &Rc<Ctx>, shared: &Rc<Shared>, k: u32, spec: &[u8]) -> Result<u32, Fail> {
    if !ctx.online() {
        return Err(denied("no network here"));
    }
    let v: Value = if spec.is_empty() { Value::Null } else { serde_json::from_slice(spec).map_err(|e| invalid(format!("spec: {e}")))? };
    match k {
        kind::TCP | kind::TLS | kind::TRANSPORT => {
            let tls = k == kind::TLS;
            let over = if tls { handle(&v, "over") } else { None };
            let leftover = over.map(|h| take_stream(shared, h)).transpose()?;
            let (host, port) = if k == kind::TRANSPORT {
                (String::new(), 0)
            } else if leftover.is_some() {
                (s(&v, "host").to_string(), v.get("port").and_then(Value::as_u64).unwrap_or(443) as u16)
            } else {
                (s(&v, "host").to_string(), port(&v)?)
            };
            if k == kind::TRANSPORT && ctx.transport.is_none() {
                return Err(denied("no transport here"));
            }
            if leftover.is_none() && k != kind::TRANSPORT {
                ctx.allow_host(&host)?;
            }
            let cfg = if tls {
                let fp = match s(&v, "fingerprint") {
                    "" => rv_tls::Fingerprint::Chrome,
                    f => crate::outbound::dial::fingerprint(f).map_err(|e| invalid(e.to_string()))?,
                };
                let alpn: Vec<Vec<u8>> = strings(&v, "alpn").into_iter().map(String::into_bytes).collect();
                let pins = pins(&v)?;
                let sni = match s(&v, "sni") {
                    "" => host.clone(),
                    n => n.to_string(),
                };
                Some(Arc::new(rv_tls::ClientConfig {
                    server_name: sni,
                    alpn_override: !alpn.is_empty(),
                    alpn,
                    fingerprint: fp,
                    verify: if !pins.is_empty() {
                        rv_tls::Verify::Pinned(pins)
                    } else if v.get("insecure").and_then(Value::as_bool) == Some(true) {
                        rv_tls::Verify::InsecureSkipVerify
                    } else {
                        rv_tls::Verify::WebPki
                    },
                }))
            } else {
                None
            };
            let pipe = Pipe::new(shared);
            let h = register(shared, Res::Stream(pipe.clone()))?;
            pipe.id.set(h);
            let ctx = ctx.clone();
            tokio::task::spawn_local(async move {
                let io = async {
                    let io: BoxStream = match leftover {
                        Some(l) => Prefixed::boxed(l),
                        None if k == kind::TRANSPORT => {
                            ctx.transport.as_ref().unwrap().open().await.map_err(|e| (err::FAILED, e.to_string()))?
                        }
                        None => ctx.tcp(&host, port).await?,
                    };
                    match cfg {
                        None => Ok::<_, Fail>(io),
                        Some(cfg) => {
                            rv_tls::set_background(|f| {
                                tokio::task::spawn_local(f);
                            });
                            let t = rv_tls::connect(io, cfg).await.map_err(|e| (err::FAILED, format!("tls: {e}")))?;
                            Ok(Box::new(t) as BoxStream)
                        }
                    }
                };
                match tokio::time::timeout(std::time::Duration::from_secs(20), io).await {
                    Ok(Ok(io)) => pipe.ready(io),
                    Ok(Err((c, m))) => pipe.fail(c, m),
                    Err(_) => pipe.fail(err::TIMEOUT, "connect timeout".into()),
                }
            });
            Ok(h)
        }
        kind::UDP => {
            let (host, port) = (s(&v, "host").to_string(), port(&v)?);
            ctx.allow_host(&host)?;
            let d = Dgram::new(shared);
            let h = register(shared, Res::Dgram(d.clone()))?;
            d.id.set(h);
            if let Some(t) =
                ctx.dgram.as_ref().filter(|_| ctx.server.as_ref().is_some_and(|(h, p)| h.eq_ignore_ascii_case(&host) && *p == port))
            {
                let link = match t.open_dgram() {
                    Ok(l) => Rc::new(l),
                    Err(e) => {
                        shared.table.borrow_mut().remove(h);
                        return Err((err::FAILED, e.to_string()));
                    }
                };
                *d.via.borrow_mut() = Some(link.clone());
                d.op.set_ok();
                d.done();
                let weak = Rc::downgrade(&d);
                let u = link.u.clone();
                drop(link);
                let task = tokio::task::spawn_local(async move {
                    loop {
                        u.notify.notified().await;
                        let Some(d) = weak.upgrade() else { return };
                        loop {
                            let next = u.tx.borrow_mut().pop_front();
                            let Some((_, data)) = next else { break };
                            d.deliver(data);
                        }
                        if let Some(e) = u.closed.borrow().clone() {
                            return d.ended(e);
                        }
                    }
                });
                d.add_task(task.abort_handle());
                return Ok(h);
            }
            let ctx = ctx.clone();
            let weak = Rc::downgrade(&d);
            let t = tokio::task::spawn_local(async move {
                let r = async {
                    let addr = ctx.udp_target(&host, port).await?;
                    let sock = rv_net::udp_socket(addr.is_ipv6(), ctx.protect.as_ref()).map_err(|e| (io_code(&e), e.to_string()))?;
                    let r = socket2::SockRef::from(&sock);
                    let _ = r.set_recv_buffer_size(1 << 20);
                    let _ = r.set_send_buffer_size(1 << 20);
                    sock.connect(addr).await.map_err(|e| (io_code(&e), e.to_string()))?;
                    sock.writable().await.map_err(|e| (io_code(&e), e.to_string()))?;
                    Ok::<_, Fail>(Rc::new(sock))
                }
                .await;
                let Some(d) = weak.upgrade() else { return };
                let sock = match r {
                    Ok(s) => s,
                    Err((c, m)) => {
                        d.op.set_err(c, m);
                        d.done();
                        return;
                    }
                };
                *d.sock.borrow_mut() = Some(sock.clone());
                d.op.set_ok();
                d.done();
                drop(d);
                let mut buf = vec![0u8; 65536];
                loop {
                    let r = sock.recv(&mut buf).await;
                    let Some(d) = weak.upgrade() else { return };
                    match r {
                        Ok(n) => d.deliver(buf[..n].to_vec()),
                        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {}
                        Err(e) => return d.ended(e.to_string()),
                    }
                }
            });
            d.add_task(t.abort_handle());
            Ok(h)
        }
        kind::QUIC => {
            let (host, port) = (s(&v, "host").to_string(), port(&v)?);
            ctx.allow_host(&host)?;
            if ctx.dgram.is_some() {
                return Err((err::UNSUPPORTED, "QUIC over a datagram transport plugin is not supported yet".into()));
            }
            let q = QuicRes::new(shared);
            let h = register(shared, Res::Quic(q.clone()))?;
            q.dg.id.set(h);
            let ctx = ctx.clone();
            let weak = Rc::downgrade(&q);
            let t = tokio::task::spawn_local(async move {
                let r = tokio::time::timeout(std::time::Duration::from_secs(20), QuicRes::dial(&ctx, &host, port, &v)).await;
                let Some(q) = weak.upgrade() else { return };
                let (endpoint, conn) = match r {
                    Ok(Ok(c)) => c,
                    Ok(Err((c, m))) => {
                        q.dg.op.set_err(c, m);
                        return q.dg.done();
                    }
                    Err(_) => {
                        q.dg.op.set_err(err::TIMEOUT, "quic: handshake timeout");
                        return q.dg.done();
                    }
                };
                *q.endpoint.borrow_mut() = Some(endpoint);
                *q.dg.quic.borrow_mut() = Some(conn.clone());
                q.dg.op.set_ok();
                q.dg.done();
                let dg = Rc::downgrade(&q.dg);
                drop(q);
                loop {
                    let r = conn.read_datagram().await;
                    let Some(d) = dg.upgrade() else { return };
                    match r {
                        Ok(b) => d.deliver(b.to_vec()),
                        Err(e) => return d.ended(e.to_string()),
                    }
                }
            });
            q.dg.add_task(t.abort_handle());
            Ok(h)
        }
        kind::QUIC_BI | kind::QUIC_UNI | kind::QUIC_ACCEPT_BI | kind::QUIC_ACCEPT_UNI => {
            let ch = handle(&v, "conn").ok_or_else(|| invalid("conn"))?;
            let Some(Res::Quic(q)) = shared.get(ch) else { return Err(invalid("`conn` is not a QUIC handle")) };
            let conn = q.dg.quic.borrow().clone().ok_or((err::AGAIN, "quic is not connected yet".into()))?;
            let pipe = Pipe::new(shared);
            let h = register(shared, Res::Stream(pipe.clone()))?;
            pipe.id.set(h);
            tokio::task::spawn_local(async move {
                let io: Result<BoxStream, quinn::ConnectionError> = match k {
                    kind::QUIC_BI => conn.open_bi().await.map(|(s, r)| Box::new(tokio::io::join(r, s)) as BoxStream),
                    kind::QUIC_UNI => conn.open_uni().await.map(|s| Box::new(tokio::io::join(tokio::io::empty(), s)) as BoxStream),
                    kind::QUIC_ACCEPT_BI => conn.accept_bi().await.map(|(s, r)| Box::new(tokio::io::join(r, s)) as BoxStream),
                    _ => conn.accept_uni().await.map(|r| Box::new(tokio::io::join(r, tokio::io::sink())) as BoxStream),
                };
                match io {
                    Ok(io) => pipe.ready(io),
                    Err(e) => pipe.fail(err::CLOSED, e.to_string()),
                }
            });
            Ok(h)
        }
        kind::H2 => {
            let over = handle(&v, "over").ok_or_else(|| invalid("over"))?;
            let l = take_stream(shared, over)?;
            let r = H2Res::new();
            let h = register(shared, Res::H2(r.clone()))?;
            let me = Rc::downgrade(&r);
            let shared_w = Rc::downgrade(shared);
            let t = tokio::task::spawn_local(async move {
                let hs = h2::client::Builder::new()
                    .header_table_size(65536)
                    .enable_push(false)
                    .initial_window_size(6 * 1024 * 1024)
                    .initial_connection_window_size(15 * 1024 * 1024)
                    .max_header_list_size(256 * 1024)
                    .handshake::<_, Bytes>(Prefixed::boxed(l))
                    .await;
                let Some(r) = me.upgrade() else { return };
                match hs {
                    Ok((send, conn)) => {
                        *r.send.borrow_mut() = Some(send);
                        r.op.set_ok();
                        drop(r);
                        if let Some(s) = shared_w.upgrade() {
                            s.push(rv_plugin_abi::ev::DONE, h, 0);
                        }
                        let e = conn.await.err().map(|e| e.to_string()).unwrap_or_else(|| "connection closed".into());
                        if let Some(r) = me.upgrade() {
                            r.send.borrow_mut().take();
                            r.op.set_err(err::CLOSED, e);
                            if let Some(s) = shared_w.upgrade() {
                                s.push(rv_plugin_abi::ev::CLOSED, h, 0);
                            }
                        }
                    }
                    Err(e) => {
                        r.op.set_err(err::FAILED, format!("h2: {e}"));
                        if let Some(s) = shared_w.upgrade() {
                            s.push(rv_plugin_abi::ev::DONE, h, 0);
                        }
                    }
                }
            });
            *r.task.borrow_mut() = Some(Task(t.abort_handle()));
            Ok(h)
        }
        kind::H2_REQUEST => {
            let ch = handle(&v, "conn").ok_or_else(|| invalid("conn"))?;
            let Some(Res::H2(c)) = shared.get(ch) else { return Err(invalid("`conn` is not an HTTP/2 handle")) };
            let send = c.send.borrow().clone().ok_or((err::AGAIN, "http/2 is not connected yet".into()))?;
            let req = h2_request(&v)?;
            let end = v.get("end").and_then(Value::as_bool).unwrap_or(false);
            let pipe = Pipe::new(shared);
            let h = register(shared, Res::Stream(pipe.clone()))?;
            pipe.id.set(h);
            let weak = Rc::downgrade(&pipe);
            tokio::task::spawn_local(async move {
                let r = async {
                    let mut send = send.ready().await?;
                    send.send_request(req, end)
                }
                .await;
                let Some(pipe) = weak.upgrade() else { return };
                match r {
                    Ok((resp, stream)) => pipe.ready(Box::new(H2Io {
                        resp: Some(resp),
                        recv: None,
                        send: (!end).then_some(stream),
                        pending: Bytes::new(),
                        rest: Bytes::new(),
                        pipe: weak,
                    })),
                    Err(e) => pipe.fail(err::FAILED, format!("h2: {e}")),
                }
            });
            Ok(h)
        }
        kind::HTTP => http(ctx, shared, &v),
        kind::RESOLVE => {
            let host = s(&v, "host").to_string();
            let server = ctx.allow_host(&host)?;
            let op = Rc::new(Op::pending());
            let h = register(shared, Res::Info(op.clone()))?;
            let (ctx, weak, shared_w) = (ctx.clone(), Rc::downgrade(&op), Rc::downgrade(shared));
            tokio::task::spawn_local(async move {
                let r = rv_net::resolve_protected(&host, ctx.protect.as_ref()).await;
                let Some(op) = weak.upgrade() else { return };
                match r {
                    Ok(ips) => {
                        let ips: Vec<String> =
                            ips.into_iter().filter(|ip| ctx.allow_ip(*ip, server).is_ok()).map(|ip| ip.to_string()).collect();
                        *op.info.borrow_mut() = Some(serde_json::json!({ "addresses": ips }).to_string().into_bytes());
                        op.set_ok();
                    }
                    Err(e) => op.set_err(io_code(&e), e.to_string()),
                }
                if let Some(s) = shared_w.upgrade() {
                    s.push(rv_plugin_abi::ev::DONE, h, 0);
                }
            });
            Ok(h)
        }
        kind::TUN => super::tun::open(ctx, shared, &v),
        _ => Err((err::UNSUPPORTED, format!("open kind {k}"))),
    }
}

fn h2_request(v: &Value) -> Result<http::Request<()>, Fail> {
    let method = match s(v, "method") {
        "" => "GET",
        m => m,
    };
    let method = http::Method::from_bytes(method.as_bytes()).map_err(|_| invalid("method"))?;
    let authority = s(v, "authority");
    let uri = if method == http::Method::CONNECT {
        authority.to_string()
    } else {
        let scheme = match s(v, "scheme") {
            "" => "https",
            x => x,
        };
        let path = match s(v, "path") {
            "" => "/",
            p => p,
        };
        format!("{scheme}://{authority}{path}")
    };
    let mut b = http::Request::builder().method(method).uri(uri);
    if let Some(hs) = v.get("headers").and_then(Value::as_array) {
        for pair in hs {
            let (Some(k), Some(val)) = (pair.get(0).and_then(Value::as_str), pair.get(1).and_then(Value::as_str)) else {
                return Err(invalid("headers: [[name, value]]"));
            };
            b = b.header(k, val);
        }
    }
    b.body(()).map_err(|e| invalid(format!("request: {e}")))
}

struct H2Io {
    resp: Option<h2::client::ResponseFuture>,
    recv: Option<h2::RecvStream>,
    send: Option<h2::SendStream<Bytes>>,
    pending: Bytes,
    rest: Bytes,
    pipe: Weak<Pipe>,
}

fn h2err(e: h2::Error) -> io::Error {
    io::Error::other(e)
}

impl H2Io {
    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.pending.is_empty() {
            let Some(send) = self.send.as_mut() else { return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())) };
            send.reserve_capacity(self.pending.len());
            match ready!(send.poll_capacity(cx)) {
                Some(Ok(0)) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Some(Ok(n)) => {
                    let chunk = self.pending.split_to(n.min(self.pending.len()));
                    send.send_data(chunk, false).map_err(h2err)?;
                }
                Some(Err(e)) => return Poll::Ready(Err(h2err(e))),
                None => return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for H2Io {
    fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        if !me.rest.is_empty() {
            let n = me.rest.len().min(buf.remaining());
            buf.put_slice(&me.rest.split_to(n));
            return Poll::Ready(Ok(()));
        }
        if let Some(resp) = me.resp.as_mut() {
            let r = ready!(std::pin::Pin::new(resp).poll(cx)).map_err(h2err)?;
            me.resp = None;
            if let Some(p) = me.pipe.upgrade() {
                let headers: Vec<[&str; 2]> = r.headers().iter().map(|(k, v)| [k.as_str(), v.to_str().unwrap_or("")]).collect();
                *p.op.info.borrow_mut() =
                    Some(serde_json::json!({ "status": r.status().as_u16(), "headers": headers }).to_string().into_bytes());
                p.notify();
            }
            if !r.status().is_success() {
                return Poll::Ready(Err(io::Error::other(format!("http status {}", r.status().as_u16()))));
            }
            me.recv = Some(r.into_body());
        }
        let Some(recv) = me.recv.as_mut() else { return Poll::Ready(Ok(())) };
        match ready!(recv.poll_data(cx)) {
            Some(Ok(mut d)) => {
                let _ = recv.flow_control().release_capacity(d.len());
                let n = d.len().min(buf.remaining());
                buf.put_slice(&d.split_to(n));
                me.rest = d;
                Poll::Ready(Ok(()))
            }
            Some(Err(e)) => Poll::Ready(Err(h2err(e))),
            None => Poll::Ready(Ok(())),
        }
    }
}

impl AsyncWrite for H2Io {
    fn poll_write(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        ready!(self.poll_pending(cx))?;
        if data.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = data.len().min(64 * 1024);
        self.pending = Bytes::copy_from_slice(&data[..n]);
        let _ = self.poll_pending(cx)?;
        Poll::Ready(Ok(n))
    }
    fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_pending(cx)
    }
    fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_pending(cx))?;
        if let Some(mut s) = self.send.take() {
            let _ = s.send_data(Bytes::new(), true);
        }
        Poll::Ready(Ok(()))
    }
}

use std::future::Future;

pub(super) struct QuicRes {
    pub dg: Rc<Dgram>,
    pub endpoint: RefCell<Option<quinn::Endpoint>>,
    _keep: RefCell<Option<rv_stats::KeepAlive>>,
}

impl QuicRes {
    fn new(shared: &Rc<Shared>) -> Rc<Self> {
        Rc::new(Self { dg: Dgram::new(shared), endpoint: RefCell::new(None), _keep: RefCell::new(None) })
    }

    async fn dial(ctx: &Ctx, host: &str, port: u16, v: &Value) -> Result<(quinn::Endpoint, quinn::Connection), Fail> {
        let server = ctx.udp_target(host, port).await?;
        let mut q = rv_config::QuicParams::default();
        q.congestion = match s(v, "cc") {
            "reno" | "newreno" => "reno".into(),
            _ => String::new(),
        };
        let ms = |k: &str, def: u64| v.get(k).and_then(Value::as_u64).unwrap_or(def);
        q.max_idle_timeout = (ms("idle_ms", 30_000) / 1000).clamp(4, 600);
        q.keep_alive_period = match ms("keepalive_ms", 0) {
            0 => 0,
            k => (k / 1000).max(10),
        };
        let alpn: Vec<Vec<u8>> = strings(v, "alpn").into_iter().map(String::into_bytes).collect();
        let sni = match s(v, "sni") {
            "" => host.to_string(),
            n => n.to_string(),
        };
        let insecure = v.get("insecure").and_then(Value::as_bool) == Some(true);
        let crypto = crate::quic::crypto(&sni, insecure, &pins(v)?, &alpn).map_err(|e| invalid(e.to_string()))?;
        let datagrams = v.get("datagrams").and_then(Value::as_bool).unwrap_or(true);
        let incoming = v.get("incoming_bi").and_then(Value::as_u64).unwrap_or(0).min(100);
        let cfg = crate::quic::client_config_announced(crypto, &q, incoming, datagrams).map_err(|e| invalid(e.to_string()))?;
        let cid = v.get("cid_len").and_then(Value::as_u64).unwrap_or(8).clamp(4, 20) as usize;
        let endpoint =
            crate::quic::socket_endpoint(server, &q, None, ctx.protect.as_ref(), cid).map_err(|e| (io_code(&e), e.to_string()))?;
        rv_stats::add(rv_stats::Slot::Handshakes, 1);
        let conn = endpoint
            .connect_with(cfg, server, &sni)
            .map_err(|e| invalid(e.to_string()))?
            .await
            .map_err(|e| (err::FAILED, format!("quic: {e}")))?;
        Ok((endpoint, conn))
    }

    pub fn export(&self, label: &[u8], context: &[u8], out: &mut [u8]) -> Result<(), Fail> {
        let conn = self.dg.quic.borrow().clone().ok_or((err::AGAIN, "quic is not connected yet".into()))?;
        conn.export_keying_material(out, label, context).map_err(|_| (err::FAILED, "keying material export failed".into()))
    }

    pub fn close(&self) {
        if let Some(c) = self.dg.quic.borrow().as_ref() {
            c.close(0u32.into(), b"");
        }
        self.dg.close();
        self.endpoint.borrow_mut().take();
    }
}

pub(super) struct H2Res {
    pub op: Op,
    pub send: RefCell<Option<h2::client::SendRequest<Bytes>>>,
    task: RefCell<Option<Task>>,
}

impl H2Res {
    fn new() -> Rc<Self> {
        Rc::new(Self { op: Op::pending(), send: RefCell::new(None), task: RefCell::new(None) })
    }

    pub fn close(&self) {
        self.send.borrow_mut().take();
        self.task.borrow_mut().take();
    }
}

fn http(ctx: &Rc<Ctx>, shared: &Rc<Shared>, v: &Value) -> Result<u32, Fail> {
    let url = s(v, "url");
    let rest = url.strip_prefix("https://").ok_or_else(|| invalid("url: https only"))?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h.to_string(), p.parse::<u16>().map_err(|_| invalid("url port"))?),
        _ => (hostport.to_string(), 443),
    };
    if host.is_empty() || path.contains(['\r', '\n', ' ']) {
        return Err(invalid("url"));
    }
    ctx.allow_host(&host)?;
    let method = match s(v, "method") {
        "" => "GET",
        m if m.bytes().all(|b| b.is_ascii_uppercase()) && m.len() <= 16 => m,
        _ => return Err(invalid("method")),
    };
    let body = match v.get("body") {
        Some(Value::String(t)) => t.clone().into_bytes(),
        None | Some(Value::Null) => Vec::new(),
        Some(_) => return Err(invalid("body: string")),
    };
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nAccept-Encoding: identity\r\n");
    let mut has_ua = false;
    if let Some(hs) = v.get("headers").and_then(Value::as_array) {
        for pair in hs {
            let (Some(k), Some(val)) = (pair.get(0).and_then(Value::as_str), pair.get(1).and_then(Value::as_str)) else {
                return Err(invalid("headers: [[name, value]]"));
            };
            if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || val.contains(['\r', '\n']) {
                return Err(invalid("header"));
            }
            if matches!(k.to_ascii_lowercase().as_str(), "host" | "content-length" | "connection" | "transfer-encoding") {
                continue;
            }
            has_ua |= k.eq_ignore_ascii_case("user-agent");
            req.push_str(&format!("{k}: {val}\r\n"));
        }
    }
    if !has_ua {
        req.push_str("User-Agent: Mozilla/5.0\r\n");
    }
    if !body.is_empty() || method == "POST" {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    let mut req = req.into_bytes();
    req.extend_from_slice(&body);
    let timeout = std::time::Duration::from_millis(v.get("timeout_ms").and_then(Value::as_u64).unwrap_or(15_000).clamp(1000, 60_000));
    let pipe = Pipe::new(shared);
    let h = register(shared, Res::Stream(pipe.clone()))?;
    pipe.id.set(h);
    let (ctx, weak) = (ctx.clone(), Rc::downgrade(&pipe));
    tokio::task::spawn_local(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let r = tokio::time::timeout(timeout, async {
            let io = ctx.tcp(&host, port).await?;
            rv_tls::set_background(|f| {
                tokio::task::spawn_local(f);
            });
            let cfg = Arc::new(rv_tls::ClientConfig {
                server_name: host.trim_start_matches('[').trim_end_matches(']').to_string(),
                alpn: vec![b"http/1.1".to_vec()],
                alpn_override: true,
                fingerprint: rv_tls::Fingerprint::Chrome,
                verify: rv_tls::Verify::WebPki,
            });
            let mut t = rv_tls::connect(io, cfg).await.map_err(|e| (err::FAILED, format!("tls: {e}")))?;
            let e = |e: io::Error| (io_code(&e), e.to_string());
            t.write_all(&req).await.map_err(e)?;
            t.flush().await.map_err(e)?;
            let mut resp = Vec::new();
            (&mut t).take(1 << 20 | 1).read_to_end(&mut resp).await.map_err(e)?;
            if resp.len() > 1 << 20 {
                return Err((err::LIMIT, "response over 1 MB".into()));
            }
            parse_http1(&resp).ok_or((err::FAILED, "bad http response".into()))
        })
        .await;
        let Some(pipe) = weak.upgrade() else { return };
        match r {
            Ok(Ok((info, body))) => {
                *pipe.op.info.borrow_mut() = Some(info);
                pipe.ready(Box::new(tokio::io::join(std::io::Cursor::new(body), tokio::io::sink())));
            }
            Ok(Err((c, m))) => pipe.fail(c, m),
            Err(_) => pipe.fail(err::TIMEOUT, "http timeout".into()),
        }
    });
    Ok(h)
}

fn parse_http1(resp: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let end = resp.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&resp[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next()?.split(' ').nth(1)?.parse().ok()?;
    let mut headers = Vec::new();
    let mut chunked = false;
    let mut length = None;
    for l in lines {
        let (k, v) = l.split_once(':')?;
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        chunked |= k == "transfer-encoding" && v.eq_ignore_ascii_case("chunked");
        if k == "content-length" {
            length = v.parse::<usize>().ok();
        }
        headers.push([k, v.to_string()]);
    }
    let raw = &resp[end + 4..];
    let body = if chunked {
        let mut out = Vec::new();
        let mut at = 0;
        loop {
            let nl = raw[at..].windows(2).position(|w| w == b"\r\n")? + at;
            let size = usize::from_str_radix(std::str::from_utf8(&raw[at..nl]).ok()?.split(';').next()?.trim(), 16).ok()?;
            at = nl + 2;
            if size == 0 {
                break;
            }
            out.extend_from_slice(raw.get(at..at + size)?);
            at += size + 2;
        }
        out
    } else {
        match length {
            Some(n) => raw.get(..n)?.to_vec(),
            None => raw.to_vec(),
        }
    };
    Some((serde_json::json!({ "status": status, "headers": headers }).to_string().into_bytes(), body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_addresses_need_the_lan_permission() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "172.20.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "198.18.0.5",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
            "224.0.0.1",
        ] {
            assert!(is_local(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "203.0.113.5", "2001:db8::1", "100.128.0.1"] {
            assert!(!is_local(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn host_patterns_match_subdomains_only() {
        assert!(host_matches("*.vk.com", "api.vk.com"));
        assert!(host_matches("*.vk.com", "a.b.vk.com"));
        assert!(!host_matches("*.vk.com", "vk.com"));
        assert!(!host_matches("*.vk.com", "evilvk.com"));
        assert!(host_matches("login.vk.ru", "login.vk.ru"));
        assert!(!host_matches("login.vk.ru", "xlogin.vk.ru"));
    }

    #[test]
    fn http1_responses_are_parsed() {
        let (info, body) = parse_http1(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-A: b\r\n\r\nhelloEXTRA").unwrap();
        let info: Value = serde_json::from_slice(&info).unwrap();
        assert_eq!(info["status"], 200);
        assert_eq!(body, b"hello");
        let (_, body) = parse_http1(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!(body, b"abcde");
        assert!(parse_http1(b"garbage").is_none());
    }

    #[test]
    fn h2_requests_are_built_from_specs() {
        let r = h2_request(
            &serde_json::json!({"method": "CONNECT", "authority": "example.com:443", "headers": [["proxy-authorization", "Basic eA=="]]}),
        )
        .unwrap();
        assert_eq!(r.method(), http::Method::CONNECT);
        assert_eq!(r.uri().authority().unwrap().as_str(), "example.com:443");
        assert_eq!(r.headers()["proxy-authorization"], "Basic eA==");
        let r = h2_request(&serde_json::json!({"authority": "a.example", "path": "/x?y"})).unwrap();
        assert_eq!(r.uri().to_string(), "https://a.example/x?y");
        assert!(h2_request(&serde_json::json!({"headers": [["bad header", "v"]]})).is_err());
    }
}
