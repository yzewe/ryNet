use std::cell::{Cell, RefCell};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};

use bytes::Bytes;
use h2::client::SendRequest;
use h2::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use crate::request::{self, Config, Mode, rand_range};

mod http1;
mod http3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpVersion {
    Http1,
    Http2,
    Http3,
}

pub trait Io: AsyncRead + AsyncWrite + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + 'static> Io for T {}

pub type LocalBoxFuture<T> = Pin<Box<dyn Future<Output = T>>>;
pub type Connector = Rc<dyn Fn() -> LocalBoxFuture<io::Result<Box<dyn Io>>>>;

pub type H3Connector = Rc<dyn Fn() -> LocalBoxFuture<io::Result<(quinn::Connection, quinn::Endpoint)>>>;
type ReadBody = Box<dyn AsyncRead + Unpin>;
type ResponseFuture = LocalBoxFuture<io::Result<http::Response<ReadBody>>>;

enum Client {
    H2(SendRequest<Bytes>),
    H3(http3::Client),
}

enum SendBody {
    H2(SendStream<Bytes>),
    H3(http3::SendBody),
}
impl AsyncWrite for SendBody {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        match &mut *self {
            Self::H3(s) => Pin::new(s).poll_write(cx, b),
            Self::H2(s) => {
                if b.is_empty() {
                    return Poll::Ready(Ok(0));
                }
                let b = &b[..b.len().min(crate::activity::XRAY_BUFFER)];
                s.reserve_capacity(b.len());
                match ready!(s.poll_capacity(cx)) {
                    Some(Ok(n)) => {
                        let n = n.min(b.len());
                        s.send_data(Bytes::copy_from_slice(&b[..n]), false).map_err(h2err)?;
                        Poll::Ready(Ok(n))
                    }
                    Some(Err(e)) => Poll::Ready(Err(h2err(e))),
                    None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
                }
            }
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::H2(_) => Poll::Ready(Ok(())),
            Self::H3(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::H2(s) => Poll::Ready(s.send_data(Bytes::new(), true).map_err(h2err)),
            Self::H3(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
impl SendBody {
    async fn send_all(&mut self, mut body: Bytes) -> io::Result<()> {
        let s = match self {
            Self::H3(s) => return s.send_all(body).await,
            Self::H2(s) => s,
        };
        while !body.is_empty() {
            s.reserve_capacity(body.len());
            let n = match tokio::time::timeout(UPLOAD_TIMEOUT, std::future::poll_fn(|cx| s.poll_capacity(cx))).await {
                Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "xhttp upload: body not sent in time")),
                Ok(Some(Ok(n))) => n,
                Ok(Some(Err(e))) => return Err(h2err(e)),
                Ok(None) => return Err(io::Error::other("xhttp upload: stream closed before the body was sent")),
            };
            let chunk = body.split_to(n.min(body.len()));
            s.send_data(chunk, body.is_empty()).map_err(h2err)?;
        }
        Ok(())
    }
    fn acked(&self) -> Option<LocalBoxFuture<()>> {
        match self {
            Self::H2(_) => None,
            Self::H3(s) => Some(Box::pin(s.acked())),
        }
    }
}

const IDLE_CLOSE: Duration = Duration::from_secs(300);
const H3_IDLE_KEEPALIVES: u32 = 3;

fn auto_gzip(req: &request::Request) -> bool {
    req.method != "HEAD"
        && crate::browser::get(&req.headers, "Accept-Encoding").is_none()
        && crate::browser::get(&req.headers, "Range").is_none()
}

fn h2err(e: h2::Error) -> io::Error {
    if e.is_io() {
        return e.into_io().unwrap();
    }
    if e.is_remote()
        && e.is_reset()
        && let Some(r) = e.reason().filter(|r| *r == h2::Reason::INTERNAL_ERROR || *r == h2::Reason::CANCEL)
    {
        return io::Error::new(io::ErrorKind::ConnectionReset, format!("stream reset by server ({r:?})"));
    }
    io::Error::other(e)
}

static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const QUIET_PERIOD: Duration = Duration::from_secs(240);

pub fn set_quiet(on: bool) {
    QUIET.store(on, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PingPayload {
    Random,
    Zero,
}

pub async fn drive<T: AsyncRead + AsyncWrite + Unpin>(
    connection: h2::client::Connection<T, Bytes>,
    keepalive: Option<(Duration, Duration)>,
    busy: impl Fn() -> bool,
) -> Result<(), h2::Error> {
    drive_observed(connection, keepalive, PingPayload::Random, busy, None, None, false).await
}

pub async fn drive_observed<T: AsyncRead + AsyncWrite + Unpin>(
    mut connection: h2::client::Connection<T, Bytes>,
    keepalive: Option<(Duration, Duration)>,
    ping: PingPayload,
    busy: impl Fn() -> bool,
    activity: Option<crate::activity::Activity>,
    bdp: Option<Rc<crate::activity::Bdp>>,
    stretch: bool,
) -> Result<(), h2::Error> {
    if keepalive.is_none() && bdp.is_none() {
        return connection.await;
    }
    let Some(mut pp) = connection.ping_pong() else {
        return connection.await;
    };
    let mut last_cycle = tokio::time::Instant::now();
    let mut estimate = crate::activity::Estimator::default();
    let mut stale = false;
    'cycle: loop {
        let quiet = stretch && QUIET.load(std::sync::atomic::Ordering::Relaxed);
        let period = keepalive.map(|k| if quiet { k.0.max(QUIET_PERIOD) } else { k.0 }).unwrap_or(Duration::from_secs(86400));
        let read_at = activity.as_ref().map(|a| a.last_read()).unwrap_or(last_cycle);
        let deadline = read_at.max(last_cycle) + period;
        let bdp_wakeup = async {
            if let Some(b) = &bdp {
                b.notified().await;
            }
        };
        let was_busy = busy();
        let until_change = std::future::poll_fn(|cx| match Pin::new(&mut connection).poll(cx) {
            Poll::Ready(r) => Poll::Ready(Some(r)),
            Poll::Pending if busy() != was_busy => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        });
        let measuring = tokio::select! {
            r=until_change => { if let Some(r) = r { return r; } last_cycle=tokio::time::Instant::now(); continue; },
            _=tokio::time::sleep_until(deadline), if keepalive.is_some() && was_busy => {
                let now=tokio::time::Instant::now();
                if !busy() || activity.as_ref().is_some_and(|a| now-a.last_read()<period) { continue; }
                false
            },
            _=bdp_wakeup, if was_busy && bdp.as_ref().is_some_and(|b| b.current()<crate::activity::BDP_LIMIT) => true,
        };
        if stale {
            match std::future::poll_fn(|cx| std::task::Poll::Ready(pp.poll_pong(cx))).await {
                std::task::Poll::Ready(Ok(_)) => stale = false,
                std::task::Poll::Ready(Err(_)) => return Ok(()),
                std::task::Poll::Pending if !measuring => return Ok(()),
                std::task::Poll::Pending => {
                    if let Some(b) = &bdp {
                        b.finish();
                    }
                    last_cycle = tokio::time::Instant::now();
                    continue 'cycle;
                }
            }
        }
        let mut payload = [0; 8];
        if measuring {
            payload = [2, 4, 16, 16, 9, 14, 7, 7];
            connection.flush_receive_window();
        } else if ping == PingPayload::Random {
            use ring::rand::SecureRandom;
            let _ = ring::rand::SystemRandom::new().fill(&mut payload);
        }
        let sent = tokio::time::Instant::now();
        if !measuring {
            rv_stats::add(rv_stats::Slot::Pings, 1);
        }
        let wait = if measuring { Duration::from_secs(20) } else { keepalive.unwrap().1 };
        let started = Cell::new(false);
        let ping = async {
            started.set(true);
            tokio::time::timeout(wait, pp.ping(h2::Ping::with_payload(payload))).await
        };
        tokio::pin!(ping);
        let until_idle = std::future::poll_fn(|cx| match Pin::new(&mut connection).poll(cx) {
            Poll::Ready(r) => Poll::Ready(Some(r)),
            Poll::Pending if !busy() => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        });
        tokio::select! {
            r=until_idle=>{
                if let Some(r)=r { return r; }
                stale=started.get();
                if measuring && let Some(b)=&bdp { b.finish(); }
                last_cycle=tokio::time::Instant::now();
                continue 'cycle;
            },
            result=&mut ping=>{
                if !matches!(result,Ok(Ok(_))) {
                    if !measuring { return Ok(()); }
                    stale = result.is_err();
                    if let Some(b)=&bdp { b.finish(); }
                    last_cycle=tokio::time::Instant::now();
                    continue 'cycle;
                }
            }
        }
        last_cycle = tokio::time::Instant::now();
        if measuring && let Some(b) = &bdp {
            let current = b.current();
            let next = estimate.update(current, b.finish(), sent.elapsed());
            if next > current {
                connection.widen_receive_window(next);
                connection.set_initial_window_size(next)?;
                b.set_current(next);
            }
        }
    }
}

struct Conn {
    send: Client,
    active: Cell<usize>,
    streams: Cell<usize>,
    reuses_left: Cell<i64>,
    requests_left: Cell<i64>,
    reusable_until: Option<Instant>,
    concurrency: i64,
    dead: Rc<Cell<bool>>,
    idle_since: Cell<Option<tokio::time::Instant>>,
    idle_timer: Cell<bool>,
    idle_close: Duration,
    waiting: Cell<usize>,
    starved_since: Cell<Option<Instant>>,
    _awake: Option<rv_stats::KeepAlive>,
}

const STARVED: Duration = Duration::from_millis(500);

impl Conn {
    fn reusable(&self, now: Instant) -> bool {
        !self.dead.get()
            && !matches!(&self.send, Client::H3(c) if !c.reusable())
            && self.requests_left.get() != 0
            && self.reusable_until.is_none_or(|t| now < t)
    }
    fn full(&self, own: usize) -> bool {
        matches!(&self.send, Client::H2(s) if self.active.get().saturating_sub(own) >= s.current_max_send_streams())
    }
}

fn h3_idle_close(keepalive: Option<Duration>) -> Duration {
    keepalive.filter(|k| !k.is_zero()).map_or(IDLE_CLOSE, |k| k.saturating_mul(H3_IDLE_KEEPALIVES).min(IDLE_CLOSE))
}

fn spend(c: &Cell<i64>) {
    if c.get() > 0 {
        c.set(c.get() - 1);
    }
}

pub struct Transport {
    cfg: Config,
    mode: Mode,
    host: String,
    connect: Connector,
    h3_connect: Option<H3Connector>,
    h3_idle: Duration,
    h3_keepalive: bool,
    download: Option<Rc<Transport>>,
    version: HttpVersion,
    h1: http1::Pool,
    conns: RefCell<Vec<Rc<Conn>>>,
    dial: tokio::sync::Mutex<()>,
}

struct Lease {
    conn: Rc<Conn>,
    transport: Weak<Transport>,
    stream: bool,
}

impl Lease {
    fn new(conn: &Rc<Conn>, transport: Weak<Transport>) -> Self {
        conn.active.set(conn.active.get() + 1);
        conn.idle_since.set(None);
        conn.streams.set(conn.streams.get() + 1);
        Lease { conn: conn.clone(), transport, stream: true }
    }

    fn again(&self) -> Self {
        self.conn.active.set(self.conn.active.get() + 1);
        self.conn.idle_since.set(None);
        Lease { conn: self.conn.clone(), transport: self.transport.clone(), stream: false }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let c = &self.conn;
        if self.stream {
            c.streams.set(c.streams.get().saturating_sub(1));
        }
        let left = c.active.get().saturating_sub(1);
        c.active.set(left);
        if left > 0 {
            return;
        }
        c.idle_since.set(Some(tokio::time::Instant::now()));
        if c.idle_timer.replace(true) {
            return;
        }
        let (conn, transport) = (Rc::downgrade(c), self.transport.clone());
        tokio::task::spawn_local(async move {
            loop {
                let Some(c) = conn.upgrade() else { return };
                let Some(since) = c.idle_since.get() else {
                    c.idle_timer.set(false);
                    return;
                };
                let deadline = since + c.idle_close;
                if tokio::time::Instant::now() >= deadline {
                    c.idle_timer.set(false);
                    if let Some(t) = transport.upgrade() {
                        t.conns.borrow_mut().retain(|x| !Rc::ptr_eq(x, &c));
                        tracing::debug!(target: "rv::pool", "xhttp idle close, pool={}", t.conns.borrow().len());
                    }
                    return;
                }
                drop(c);
                tokio::time::sleep_until(deadline).await;
            }
        });
    }
}

impl Transport {
    pub fn new(cfg: Config, reality: bool, server_name: &str, address: &str, connect: Connector) -> Rc<Transport> {
        Self::new_with_download(cfg, reality, server_name, address, connect, None)
    }

    pub fn new_with_download(
        cfg: Config,
        reality: bool,
        server_name: &str,
        address: &str,
        connect: Connector,
        download: Option<Rc<Transport>>,
    ) -> Rc<Transport> {
        Self::new_with_download_version(cfg, reality, server_name, address, connect, download, HttpVersion::Http2)
    }

    pub fn new_with_download_version(
        mut cfg: Config,
        reality: bool,
        server_name: &str,
        address: &str,
        connect: Connector,
        download: Option<Rc<Transport>>,
        version: HttpVersion,
    ) -> Rc<Transport> {
        let mode = cfg.mode(reality);
        let host = cfg.host(server_name, address);
        cfg.set_request_host(&host);
        let h1 = http1::Pool::new(cfg.xmux());
        Rc::new(Transport {
            cfg,
            mode,
            host,
            connect,
            h3_connect: None,
            h3_idle: IDLE_CLOSE,
            h3_keepalive: false,
            download,
            version,
            h1,
            conns: RefCell::new(Vec::new()),
            dial: tokio::sync::Mutex::new(()),
        })
    }

    pub fn new_h3(
        cfg: Config,
        server_name: &str,
        address: &str,
        connect: H3Connector,
        download: Option<Rc<Transport>>,
        keepalive: Option<Duration>,
    ) -> Rc<Transport> {
        let unused: Connector = Rc::new(|| Box::pin(async { Err(io::Error::other("xhttp: QUIC connector required")) }));
        let mut t = Self::new_with_download_version(cfg, false, server_name, address, unused, download, HttpVersion::Http3);
        let m = Rc::get_mut(&mut t).unwrap();
        m.h3_connect = Some(connect);
        m.h3_idle = h3_idle_close(keepalive);
        m.h3_keepalive = keepalive.is_some_and(|k| !k.is_zero());
        t
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn reset(&self) {
        self.h1.reset();
        let n = std::mem::take(&mut *self.conns.borrow_mut()).len();
        tracing::debug!(target: "rv::pool", "xhttp reset, dropped={n}");
        if let Some(d) = &self.download {
            d.reset();
        }
    }

    async fn conn(self: &Rc<Self>) -> io::Result<Lease> {
        if let Some(l) = self.pick() {
            return Ok(l);
        }
        let _dialing = self.dial.lock().await;
        if let Some(l) = self.pick() {
            return Ok(l);
        }
        self.dial_new().await
    }

    fn pick(self: &Rc<Self>) -> Option<Lease> {
        let x = self.cfg.xmux();
        {
            let mut conns = self.conns.borrow_mut();
            let now = Instant::now();
            conns.retain(|c| c.reusable(now) && c.reuses_left.get() != 0);
            let starved = |c: &Conn| c.starved_since.get().is_some_and(|t| now - t > STARVED);
            let pick = conns
                .iter()
                .find(|c| !starved(c) && !c.full(0) && (c.concurrency <= 0 || (c.streams.get() as i64) < c.concurrency))
                .or_else(|| {
                    let max_conns = rand_range(x.max_connections);
                    (max_conns > 0 && conns.len() as i64 >= max_conns)
                        .then(|| conns.iter().filter(|c| !c.full(0)).min_by_key(|c| c.streams.get()))
                        .flatten()
                });
            if let Some(c) = pick {
                spend(&c.reuses_left);
                return Some(Lease::new(c, Rc::downgrade(self)));
            }
        }
        None
    }

    async fn dial_new(self: &Rc<Self>) -> io::Result<Lease> {
        let x = self.cfg.xmux();
        let t0 = Instant::now();
        let mut h2_driver = None;
        let send = if let Some(connect) = &self.h3_connect {
            let (quic, endpoint) = connect().await?;
            Client::H3(http3::Client::new(quic, endpoint).await?)
        } else {
            let io = (self.connect)().await?;
            let io = crate::activity::FrameFlush::new(io, crate::activity::Records::GoHttp2);
            let (io, activity) = crate::activity::ActivityIo::new(io);
            let (send, connection) = h2::client::Builder::new()
                .initial_window_size(4 << 20)
                .initial_connection_window_size((1 << 30) + 65535)
                .max_frame_size(16384)
                .enable_push(false)
                .max_header_list_size(10 << 20)
                .window_refresh(h2::WindowRefresh::GoMin)
                .handshake::<_, Bytes>(io)
                .await
                .map_err(h2err)?;
            h2_driver = Some((connection, activity));
            Client::H2(send)
        };
        let dead = Rc::new(Cell::new(false));
        let limit = |r: request::Range| {
            let v = rand_range(r);
            if v > 0 { v } else { -1 }
        };
        let secs = rand_range(x.h_max_reusable_secs);
        let conn = Rc::new(Conn {
            send,
            active: Cell::new(0),
            streams: Cell::new(0),
            reuses_left: Cell::new(limit(x.c_max_reuse_times)),
            requests_left: Cell::new(limit(x.h_max_request_times)),
            reusable_until: (secs > 0).then(|| Instant::now() + Duration::from_secs(secs as u64)),
            concurrency: rand_range(x.max_concurrency),
            dead: dead.clone(),
            idle_since: Cell::new(None),
            idle_timer: Cell::new(false),
            idle_close: if self.h3_connect.is_some() { self.h3_idle } else { IDLE_CLOSE },
            waiting: Cell::new(0),
            starved_since: Cell::new(None),
            _awake: (self.h3_connect.is_some() && self.h3_keepalive).then(rv_stats::KeepAlive::new),
        });
        if let Some((connection, activity)) = h2_driver {
            let dead = dead.clone();
            let weak = Rc::downgrade(&conn);
            let period = match x.h_keep_alive_period {
                0 => 45,
                n if n > 0 => n as u64,
                _ => 0,
            };
            let default_period = x.h_keep_alive_period == 0;
            let keepalive = (period > 0).then(|| (Duration::from_secs(period), Duration::from_secs(15)));
            tokio::task::spawn_local(async move {
                let busy = move || weak.upgrade().is_some_and(|c| c.active.get() > 0);
                if let Err(e) = drive_observed(connection, keepalive, PingPayload::Random, busy, Some(activity), None, default_period).await
                {
                    tracing::debug!("xhttp h2 connection: {e}");
                }
                dead.set(true);
            });
        }
        spend(&conn.reuses_left);
        self.conns.borrow_mut().push(conn.clone());
        tracing::debug!(
            target: "rv::pool",
            "xhttp new conn ms={} pool={} mode={:?}{}",
            t0.elapsed().as_millis(),
            self.conns.borrow().len(),
            self.mode,
            if self.conns.borrow().len() == 1 { format!(" xmux={:?}", self.cfg.xmux()) } else { String::new() },
        );
        Ok(Lease::new(&conn, Rc::downgrade(self)))
    }

    async fn request_fresh(
        self: &Rc<Self>,
        mut lease: Lease,
        req: &request::Request,
        streaming_body: bool,
    ) -> io::Result<(Lease, ResponseFuture, Option<SendBody>)> {
        for attempt in 0..2 {
            let h3 = matches!(lease.conn.send, Client::H3(_));
            match self.request(&lease, req, streaming_body).await {
                Ok((resp, send)) => return Ok((lease, resp, send)),
                Err(e) if h3 => {
                    lease.conn.dead.set(true);
                    if attempt != 0 || !http3::retriable(&e) {
                        return Err(e);
                    }
                    tracing::debug!(target: "rv::pool", "xhttp h3 conn unusable ({e}), retry on a fresh one");
                    lease =
                        tokio::time::timeout(UPLOAD_TIMEOUT, self.conn()).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
                }
                Err(e)
                    if attempt == 0
                        && (matches!(
                            e.kind(),
                            io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe
                        ) || e
                            .get_ref()
                            .and_then(|e| e.downcast_ref::<h2::Error>())
                            .is_some_and(|e| e.is_go_away() || e.is_io() || e.reason() == Some(h2::Reason::REFUSED_STREAM))) =>
                {
                    lease.conn.dead.set(true);
                    tracing::debug!(target: "rv::pool", "xhttp conn closing ({e}), retry on a fresh one");
                    lease =
                        tokio::time::timeout(UPLOAD_TIMEOUT, self.conn()).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }

    async fn request(&self, lease: &Lease, req: &request::Request, streaming_body: bool) -> io::Result<(ResponseFuture, Option<SendBody>)> {
        let send = match &lease.conn.send {
            Client::H3(c) => {
                let (response, send) = c.request(&self.host, req, streaming_body).await?;
                spend(&lease.conn.requests_left);
                return Ok((response, send.map(SendBody::H3)));
            }
            Client::H2(send) => send,
        };
        let mut send = send.clone().ready().await.map_err(h2err)?;
        let mut b = http::Request::builder().method(req.method.as_str()).uri(format!("https://{}{}", self.host, req.path));
        for i in shuffled(req.headers.len()) {
            let (k, v) = &req.headers[i];
            b = b.header(k.as_str(), v.as_str());
        }
        if let Some(body) = req.body.as_ref().filter(|b| !b.is_empty()) {
            b = b.header("content-length", body.len());
        }
        if auto_gzip(req) {
            b = b.header("accept-encoding", "gzip");
        }
        let http_req = b.body(()).map_err(|e| {
            tracing::debug!("xhttp request: {e}");
            io::Error::new(io::ErrorKind::InvalidInput, e)
        })?;
        let has_body = streaming_body || req.body.as_ref().is_some_and(|b| !b.is_empty());
        let (resp, stream) = send.send_request(http_req, !has_body).map_err(h2err)?;
        spend(&lease.conn.requests_left);
        let resp: ResponseFuture = Box::pin(async move {
            let r = resp.await.map_err(h2err)?;
            Ok(r.map(|recv| Box::new(H2Body { recv, buffered: Bytes::new() }) as ReadBody))
        });
        if let Some(body) = req.body.as_ref().filter(|b| !b.is_empty()) {
            let mut send = SendBody::H2(stream);
            send.send_all(body.clone()).await?;
            return Ok((resp, Some(send)));
        }
        Ok((resp, if streaming_body { Some(SendBody::H2(stream)) } else { None }))
    }

    pub async fn open(self: &Rc<Self>) -> io::Result<Box<dyn Io>> {
        if self.version == HttpVersion::Http1 || self.download.as_ref().is_some_and(|d| d.version == HttpVersion::Http1) {
            return http1::open(self).await;
        }
        match self.mode {
            Mode::StreamOne => {
                let req = self.cfg.stream_request(None, true);
                let lease = self.conn().await?;
                let (lease, resp, send) = self.request_fresh(lease, &req, true).await?;
                Ok(Box::new(HttpStream::new(resp, send, lease, auto_gzip(&req))))
            }
            Mode::StreamUp => {
                let sid = self.cfg.generate_session_id();
                let d = self.download.as_ref().unwrap_or(self);
                let down = d.cfg.stream_request(Some(&sid), false);
                let lease = d.conn().await?;
                let (lease, resp, _) = d.request_fresh(lease, &down, false).await?;
                let up = self.cfg.stream_request(Some(&sid), true);
                let up_lease = if self.download.is_some() || lease.conn.full(0) { self.conn().await? } else { lease.again() };
                let (up_lease, up_resp, send) = self.request_fresh(up_lease, &up, true).await?;
                tokio::task::spawn_local(async move {
                    if let Ok(r) = up_resp.await {
                        let mut body = r.into_body();
                        let _ = tokio::io::copy(&mut body, &mut tokio::io::sink()).await;
                    }
                });
                let mut s = HttpStream::new(resp, send, lease, auto_gzip(&down));
                s.up_lease = Some(up_lease);
                Ok(Box::new(s))
            }
            Mode::PacketUp => {
                let sid = self.cfg.generate_session_id();
                let d = self.download.as_ref().unwrap_or(self);
                let down = d.cfg.stream_request(Some(&sid), false);
                let lease = d.conn().await?;
                let (lease, resp, _) = d.request_fresh(lease, &down, false).await?;
                let (tx, rx) = mpsc::channel::<Vec<u8>>(1);
                let up_lease = if self.download.is_some() { self.conn().await? } else { lease.again() };
                let task = tokio::task::spawn_local(uploader(self.clone(), up_lease, sid, rx));
                let monitor = tx.clone();
                let max = rand_range(self.cfg.max_each_post_bytes()).max(1) as usize;
                Ok(Box::new(PacketUp {
                    down: Box::new(HttpStream::new(resp, None, lease, auto_gzip(&down))),
                    up: tx,
                    pending: Vec::new(),
                    max,
                    sending: None,
                    reserving: None,
                    _uploader: Some(http1::Task::new(task)),
                    upload_closed: Some(Box::pin(async move {
                        monitor.closed().await;
                    })),
                }))
            }
        }
    }
}

async fn uploader(t: Rc<Transport>, mut up: Lease, sid: String, mut rx: mpsc::Receiver<Vec<u8>>) {
    let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(UPLOAD_INFLIGHT));
    let failed = Rc::new(tokio::sync::Notify::new());
    let mut seq: u64 = 0;
    let mut last: Option<Instant> = None;
    loop {
        let chunk = tokio::select! {
            c = rx.recv() => match c { Some(c) => c, None => return },
            _ = failed.notified() => return,
        };
        let min = rand_range(t.cfg.min_posts_interval_ms());
        if min > 0
            && let Some(l) = last
        {
            let wait = Duration::from_millis(min as u64).saturating_sub(l.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        let Ok(slot) = slots.clone().acquire_owned().await else { return };
        last = Some(Instant::now());
        let req = t.cfg.packet_request(&sid, seq, chunk);
        seq += 1;
        if !up.conn.reusable(Instant::now()) || up.conn.full(1) {
            up = match t.conn().await {
                Ok(l) => l,
                Err(e) => {
                    tracing::debug!("xhttp upload: {e}");
                    return;
                }
            };
        }
        match t.request_fresh(up, &req, false).await {
            Ok((lease, resp, send)) => {
                up = lease;
                let acked = send.as_ref().and_then(SendBody::acked);
                drop(send);
                let lease = up.again();
                let failed = failed.clone();
                tokio::task::spawn_local(async move {
                    let _lease = lease;
                    let _slot = slot;
                    if let Some(acked) = acked {
                        acked.await;
                    }
                    let answer = async {
                        match resp.await {
                            Ok(r) if r.status() == 200 => {
                                let mut body = r.into_body();
                                tokio::io::copy(&mut body, &mut tokio::io::sink()).await.is_ok()
                            }
                            Ok(r) => {
                                tracing::debug!("xhttp upload status {}", r.status());
                                false
                            }
                            Err(e) => {
                                tracing::debug!("xhttp upload: {e}");
                                false
                            }
                        }
                    };
                    let ok = tokio::time::timeout(UPLOAD_TIMEOUT, answer).await.unwrap_or_else(|_| {
                        tracing::debug!("xhttp upload: no response");
                        false
                    });
                    if !ok {
                        failed.notify_one();
                    }
                });
            }
            Err(e) => {
                tracing::debug!("xhttp upload: {e}");
                return;
            }
        }
    }
}

const UPLOAD_INFLIGHT: usize = 8;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);

struct HttpStream {
    resp: Option<ResponseFuture>,
    recv: Option<Body>,
    auto_gzip: bool,
    send: Option<SendBody>,
    _lease: Lease,
    up_lease: Option<Lease>,
    write_closed: bool,
    waiting: bool,
}

impl HttpStream {
    fn set_waiting(&mut self, w: bool) {
        if self.waiting == w {
            return;
        }
        self.waiting = w;
        let c = &self.up_lease.as_ref().unwrap_or(&self._lease).conn;
        if w {
            c.waiting.set(c.waiting.get() + 1);
            if c.starved_since.get().is_none() {
                c.starved_since.set(Some(Instant::now()));
            }
        } else {
            c.waiting.set(c.waiting.get().saturating_sub(1));
            c.starved_since.set((c.waiting.get() > 0).then(Instant::now));
        }
    }
}

impl Drop for HttpStream {
    fn drop(&mut self) {
        self.set_waiting(false);
    }
}

impl HttpStream {
    fn new(resp: ResponseFuture, send: Option<SendBody>, lease: Lease, auto_gzip: bool) -> Self {
        Self { resp: Some(resp), recv: None, auto_gzip, send, _lease: lease, up_lease: None, write_closed: false, waiting: false }
    }
}

struct H2Body {
    recv: RecvStream,
    buffered: Bytes,
}
impl AsyncRead for H2Body {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if !self.buffered.is_empty() {
                let n = self.buffered.len().min(buf.remaining());
                buf.put_slice(&self.buffered.split_to(n));
                return Poll::Ready(Ok(()));
            }
            match ready!(self.recv.poll_data(cx)) {
                Some(Ok(data)) => {
                    self.recv.flow_control().release_capacity(data.len()).map_err(h2err)?;
                    self.buffered = data;
                }
                Some(Err(e)) => return Poll::Ready(Err(h2err(e))),
                None => return Poll::Ready(Ok(())),
            }
        }
    }
}
enum Body {
    Plain(ReadBody),
    Gzip(Box<async_compression::tokio::bufread::GzipDecoder<tokio::io::BufReader<ReadBody>>>),
}
impl AsyncRead for Body {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(body) => Pin::new(body.as_mut()).poll_read(cx, buf),
            Self::Gzip(body) => Pin::new(body.as_mut()).poll_read(cx, buf),
        }
    }
}
impl AsyncRead for HttpStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if let Some(resp) = self.resp.as_mut() {
            let response = ready!(resp.as_mut().poll(cx))?;
            self.resp = None;
            if response.status() != 200 {
                return Poll::Ready(Err(io::Error::other(format!("xhttp: status {}", response.status()))));
            }
            let gzip =
                self.auto_gzip && response.headers().get("content-encoding").is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"gzip"));
            let body = response.into_body();
            self.recv = Some(if gzip {
                Body::Gzip(Box::new(async_compression::tokio::bufread::GzipDecoder::new(tokio::io::BufReader::new(body))))
            } else {
                Body::Plain(body)
            });
        }
        match &mut self.recv {
            Some(body) => Pin::new(body).poll_read(cx, buf),
            None => Poll::Ready(Ok(())),
        }
    }
}

impl AsyncWrite for HttpStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if self.write_closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        let Some(send) = self.send.as_mut() else {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        };
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let polled = Pin::new(send).poll_write(cx, buf);
        self.set_waiting(polled.is_pending());
        polled
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.send.as_mut() {
            Some(s) => Pin::new(s).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.write_closed {
            return Poll::Ready(Ok(()));
        }
        if let Some(s) = self.send.as_mut() {
            if matches!(s, SendBody::H3(_)) {
                ready!(Pin::new(s).poll_flush(cx))?;
            } else {
                ready!(Pin::new(s).poll_shutdown(cx))?;
                self.send = None;
            }
        }
        self.write_closed = true;
        Poll::Ready(Ok(()))
    }
}

type Reserve = Pin<Box<dyn Future<Output = Result<mpsc::OwnedPermit<Vec<u8>>, mpsc::error::SendError<()>>>>>;

struct PacketUp {
    down: Box<dyn Io>,
    up: mpsc::Sender<Vec<u8>>,
    pending: Vec<u8>,
    max: usize,
    sending: Option<Vec<u8>>,
    reserving: Option<Reserve>,
    _uploader: Option<http1::Task>,
    upload_closed: Option<LocalBoxFuture<()>>,
}

impl PacketUp {
    fn poll_push(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.sending.is_none() && !self.pending.is_empty() {
            let take = self.pending.len().min(self.max);
            let chunk: Vec<u8> =
                if take == self.pending.len() { std::mem::take(&mut self.pending) } else { self.pending.drain(..take).collect() };
            self.sending = Some(chunk);
        }
        if self.sending.is_some() {
            if self.reserving.is_none() {
                self.reserving = Some(Box::pin(self.up.clone().reserve_owned()));
            }
            let permit = ready!(self.reserving.as_mut().unwrap().as_mut().poll(cx));
            self.reserving = None;
            match permit {
                Ok(p) => {
                    p.send(self.sending.take().unwrap());
                }
                Err(_) => return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for PacketUp {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if self.upload_closed.is_some() && (self.up.is_closed() || self.upload_closed.as_mut().unwrap().as_mut().poll(cx).is_ready()) {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "xhttp: upload failed")));
        }
        Pin::new(&mut self.down).poll_read(cx, buf)
    }
}

impl AsyncWrite for PacketUp {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if self.upload_closed.is_some() && self.up.is_closed() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        while self.pending.len() >= self.max {
            ready!(self.poll_push(cx))?;
        }
        let room = self.max - self.pending.len();
        let n = room.min(buf.len());
        self.pending.extend_from_slice(&buf[..n]);
        Poll::Ready(Ok(n))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.upload_closed.is_some() && self.up.is_closed() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        while !self.pending.is_empty() || self.sending.is_some() {
            ready!(self.poll_push(cx))?;
        }
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush(cx))?;
        if let Some(t) = self._uploader.as_mut() {
            t.detach();
        }
        Poll::Ready(Ok(()))
    }
}

pub fn shuffled(n: usize) -> Vec<usize> {
    use ring::rand::SecureRandom;
    let mut order: Vec<usize> = (0..n).collect();
    let mut r = vec![0u8; 4 * n];
    let _ = ring::rand::SystemRandom::new().fill(&mut r);
    for i in (1..n).rev() {
        let x = u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap());
        order.swap(i, x as usize % (i + 1));
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hpack_matches_go_for_path_and_short_strings() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (client, mut peer) = tokio::io::duplex(65536);
        let capture = tokio::spawn(async move {
            let mut preface = [0; 24];
            peer.read_exact(&mut preface).await.unwrap();
            peer.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0]).await.unwrap();
            loop {
                let mut frame = [0; 9];
                peer.read_exact(&mut frame).await.unwrap();
                let len = u32::from_be_bytes([0, frame[0], frame[1], frame[2]]) as usize;
                let mut payload = vec![0; len];
                peer.read_exact(&mut payload).await.unwrap();
                if frame[3] == 1 {
                    return payload;
                }
            }
        });
        let (mut send, conn) = h2::client::handshake(client).await.unwrap();
        let driver = tokio::spawn(async move {
            let _ = conn.await;
        });
        let req = http::Request::post("http://test.local/fp/Tun")
            .extension(h2::ext::PseudoHeaderOrder::Grpc)
            .header("content-type", "application/grpc")
            .header("te", "trailers")
            .header("dnt", "1")
            .header("accept", "*/*")
            .body(())
            .unwrap();
        let _response = send.send_request(req, true).unwrap();
        let block = tokio::time::timeout(Duration::from_secs(2), capture).await.unwrap().unwrap();
        let expected = "83864586625ad8df6d5f4187495095e83907475f8b1d75d0620d263d4c4d656440027465864d833505b11f4003646e74013153032a2f2a";
        let hex: String = block.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, expected);
        driver.abort();
    }

    #[tokio::test]
    async fn explicit_grpc_order_survives_h2_extension_cleanup() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for grpc in [false, true] {
            let (client, mut peer) = tokio::io::duplex(65536);
            let capture = tokio::spawn(async move {
                let mut preface = [0; 24];
                peer.read_exact(&mut preface).await.unwrap();
                assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
                peer.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0]).await.unwrap();
                loop {
                    let mut frame = [0; 9];
                    peer.read_exact(&mut frame).await.unwrap();
                    let len = u32::from_be_bytes([0, frame[0], frame[1], frame[2]]) as usize;
                    let mut payload = vec![0; len];
                    peer.read_exact(&mut payload).await.unwrap();
                    if frame[3] == 1 {
                        return payload;
                    }
                }
            });
            let (mut send, conn) = h2::client::handshake(client).await.unwrap();
            let driver = tokio::spawn(async move {
                let _ = conn.await;
            });
            let mut request = http::Request::post("http://test.local/path").header("content-type", "application/grpc");
            if grpc {
                request = request.extension(h2::ext::PseudoHeaderOrder::Grpc);
            }
            let _request = send.send_request(request.body(()).unwrap(), true).unwrap();
            let payload = tokio::time::timeout(Duration::from_secs(2), capture).await.unwrap().unwrap();
            if grpc {
                assert_eq!(&payload[..2], &[0x83, 0x86], "POST, http must be the first indexed fields");
            } else {
                assert_eq!(payload[0], 0x41, "Go HTTP starts with literal indexed-name :authority");
            }
            driver.abort();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn auto_gzip_decodes_a_streaming_response_and_explicit_encoding_is_preserved() {
        use std::io::Write;
        use tokio::io::AsyncReadExt;
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut state = 17u64;
                let expected: Vec<u8> = (0..1_048_576)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        state as u8
                    })
                    .collect();
                let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                gzip.write_all(&expected).unwrap();
                let compressed = gzip.finish().unwrap();
                for explicit in [false, true] {
                    let data = compressed.clone();
                    let connect: Connector = Rc::new(move || {
                        let data = data.clone();
                        Box::pin(async move {
                            let (client, server) = tokio::io::duplex(65536);
                            tokio::task::spawn_local(async move {
                                let mut conn = h2::server::handshake(server).await.unwrap();
                                while let Some(Ok((req, mut reply))) = conn.accept().await {
                                    assert_eq!(req.headers()["accept-encoding"], "gzip");
                                    assert_eq!(req.headers()["user-agent"], "Go-http-client/2.0");
                                    if !explicit {
                                        assert_eq!(req.headers().keys().last().unwrap(), "accept-encoding");
                                    }
                                    let mut send = reply
                                        .send_response(
                                            http::Response::builder().header("content-encoding", "gzip").body(()).unwrap(),
                                            false,
                                        )
                                        .unwrap();
                                    let data = data.clone();
                                    tokio::task::spawn_local(async move {
                                        let mut at = 0;
                                        while at < data.len() {
                                            send.reserve_capacity((data.len() - at).min(16384));
                                            let capacity = std::future::poll_fn(|cx| send.poll_capacity(cx)).await.unwrap().unwrap();
                                            let n = capacity.min(data.len() - at).min(16384);
                                            if n == 0 {
                                                continue;
                                            }
                                            send.send_data(Bytes::copy_from_slice(&data[at..at + n]), at + n == data.len()).unwrap();
                                            at += n;
                                        }
                                    });
                                }
                            });
                            Ok(Box::new(client) as Box<dyn Io>)
                        })
                    });
                    let mut headers = serde_json::json!({"User-Agent":"golang"});
                    if explicit {
                        headers["Accept-Encoding"] = "gzip".into();
                    }
                    let cfg = Config::from_json(&serde_json::json!({"mode":"stream-one","path":"/x","headers":headers})).unwrap();
                    let t = Transport::new(cfg, false, "example.com", "127.0.0.1", connect);
                    let mut stream = t.open().await.unwrap();
                    let mut received = Vec::new();
                    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut received)).await.unwrap().unwrap();
                    assert_eq!(received, if explicit { compressed.clone() } else { expected.clone() });
                }
            })
            .await;
    }

    fn connector(dials: Rc<Cell<u32>>) -> Connector {
        Rc::new(move || {
            dials.set(dials.get() + 1);
            Box::pin(async move {
                let (client, server) = tokio::io::duplex(1 << 16);
                tokio::task::spawn_local(async move {
                    let mut h2 = h2::server::handshake(server).await.unwrap();
                    while let Some(Ok((_req, mut respond))) = h2.accept().await {
                        let _ = respond.send_response(http::Response::new(()), false);
                    }
                });
                Ok(Box::new(client) as Box<dyn Io>)
            })
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn keepalive_ping() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let ka = Some((Duration::from_millis(50), Duration::from_millis(100)));
                let (client, server) = tokio::io::duplex(1 << 16);
                tokio::task::spawn_local(async move {
                    let mut h2 = h2::server::handshake(server).await.unwrap();
                    while h2.accept().await.is_some() {}
                });
                let (_send, conn) = h2::client::handshake(client).await.unwrap();
                let r = tokio::time::timeout(Duration::from_millis(400), drive(conn, ka, || true)).await;
                assert!(r.is_err(), "alive connection must not end");
                let (client, mut server) = tokio::io::duplex(1 << 16);
                tokio::task::spawn_local(async move {
                    let mut b = [0u8; 4096];
                    while tokio::io::AsyncReadExt::read(&mut server, &mut b).await.is_ok_and(|n| n > 0) {}
                });
                let (_send, conn) = h2::client::handshake(client).await.unwrap();
                let r = tokio::time::timeout(Duration::from_millis(400), drive(conn, ka, || true)).await;
                assert!(r.is_ok(), "silent server must be dropped");
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn keepalive_deadline_follows_last_received_frame() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::task::LocalSet::new()
            .run_until(async {
                let (client, peer) = tokio::io::duplex(65536);
                let (io, activity) = crate::activity::ActivityIo::new(client);
                let (_send, conn) = h2::client::handshake(io).await.unwrap();
                let driver = tokio::task::spawn_local(drive_observed(
                    conn,
                    Some((Duration::from_secs(1), Duration::from_secs(1))),
                    PingPayload::Random,
                    || true,
                    Some(activity),
                    None,
                    false,
                ));
                let (mut reader, mut writer) = tokio::io::split(peer);
                let mut preface = [0; 24];
                reader.read_exact(&mut preface).await.unwrap();
                let first_ping = tokio::task::spawn_local(async move {
                    loop {
                        let mut h = [0; 9];
                        reader.read_exact(&mut h).await.unwrap();
                        let n = u32::from_be_bytes([0, h[0], h[1], h[2]]) as usize;
                        let mut data = vec![0; n];
                        reader.read_exact(&mut data).await.unwrap();
                        if h[3] == 6 && h[4] == 0 {
                            assert_eq!(n, 8);
                            return;
                        }
                    }
                });
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                tokio::time::advance(Duration::from_millis(100)).await;
                writer.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0]).await.unwrap();
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                tokio::time::advance(Duration::from_millis(900)).await;
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                assert!(!first_ping.is_finished(), "PING before the idle period elapsed");
                tokio::time::advance(Duration::from_millis(101)).await;
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                assert!(first_ping.is_finished(), "received frame delayed PING by a whole extra period");
                first_ping.await.unwrap();
                driver.abort();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn quiet_screen_stretches_only_the_default_keepalive() {
        use tokio::io::AsyncReadExt;
        set_quiet(true);
        tokio::task::LocalSet::new()
            .run_until(async {
                let (client, peer) = tokio::io::duplex(65536);
                let (io, activity) = crate::activity::ActivityIo::new(client);
                let (_send, conn) = h2::client::handshake(io).await.unwrap();
                let driver = tokio::task::spawn_local(drive_observed(
                    conn,
                    Some((Duration::from_secs(45), Duration::from_secs(15))),
                    PingPayload::Random,
                    || true,
                    Some(activity),
                    None,
                    true,
                ));
                let (mut reader, _writer) = tokio::io::split(peer);
                let mut preface = [0; 24];
                reader.read_exact(&mut preface).await.unwrap();
                let first_ping = tokio::task::spawn_local(async move {
                    loop {
                        let mut h = [0; 9];
                        reader.read_exact(&mut h).await.unwrap();
                        let n = u32::from_be_bytes([0, h[0], h[1], h[2]]) as usize;
                        let mut data = vec![0; n];
                        reader.read_exact(&mut data).await.unwrap();
                        if h[3] == 6 && h[4] == 0 {
                            return;
                        }
                    }
                });
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                tokio::time::advance(Duration::from_secs(230)).await;
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                assert!(!first_ping.is_finished(), "default keepalive pinged with the screen off before 4 minutes");
                tokio::time::advance(Duration::from_secs(11)).await;
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                assert!(first_ping.is_finished(), "no PING after 4 minutes with the screen off");
                driver.abort();
            })
            .await;
        set_quiet(false);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn idle_pool_has_no_keepalive_wakeups_and_resumes_on_new_stream() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (client, server) = tokio::io::duplex(1 << 16);
                let peer = tokio::task::spawn_local(async move {
                    let mut h2 = h2::server::handshake(server).await.unwrap();
                    let mut replies = Vec::new();
                    while let Some(Ok((_req, mut reply))) = h2.accept().await {
                        replies.push(reply.send_response(http::Response::new(()), false).unwrap());
                    }
                });
                let (send, conn) = h2::client::handshake(client).await.unwrap();
                let active = Rc::new(Cell::new(false));
                let polls = Rc::new(Cell::new(0));
                let (busy, count) = (active.clone(), polls.clone());
                let driver = tokio::task::spawn_local(async move {
                    let mut f = Box::pin(drive(conn, Some((Duration::from_secs(1), Duration::from_secs(1))), move || busy.get()));
                    std::future::poll_fn(|cx| {
                        count.set(count.get() + 1);
                        f.as_mut().poll(cx)
                    })
                    .await
                });
                for _ in 0..50 {
                    tokio::task::yield_now().await;
                }
                let before = polls.get();
                tokio::time::advance(Duration::from_secs(60)).await;
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                assert_eq!(polls.get(), before, "idle keepalive timer woke the executor");
                active.set(true);
                let mut send = send.ready().await.unwrap();
                let (response, request) =
                    send.send_request(http::Request::builder().uri("http://test.local/").body(()).unwrap(), false).unwrap();
                let _response = response.await.unwrap();
                for _ in 0..50 {
                    tokio::task::yield_now().await;
                }
                let before = polls.get();
                tokio::time::advance(Duration::from_secs(1)).await;
                for _ in 0..50 {
                    tokio::task::yield_now().await;
                }
                assert!(polls.get() > before, "keepalive did not resume on a new stream");
                active.set(false);
                drop(request);
                drop(_response);
                for _ in 0..50 {
                    tokio::task::yield_now().await;
                }
                let before = polls.get();
                tokio::time::advance(Duration::from_secs(60)).await;
                for _ in 0..10 {
                    tokio::task::yield_now().await;
                }
                assert_eq!(polls.get(), before, "keepalive timer survived the last stream");
                driver.abort();
                peer.abort();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn last_stream_cancels_pending_ping_timeout() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::task::LocalSet::new()
            .run_until(async {
                for measuring in [false, true] {
                    let (client, mut server) = tokio::io::duplex(1 << 16);
                    let pings = Rc::new(Cell::new(0));
                    let seen = pings.clone();
                    let answer = Rc::new(Cell::new(false));
                    let replying = answer.clone();
                    let peer = tokio::task::spawn_local(async move {
                        let mut preface = [0; 24];
                        server.read_exact(&mut preface).await.unwrap();
                        server.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0]).await.unwrap();
                        let mut delayed = None;
                        loop {
                            let mut h = [0; 9];
                            if server.read_exact(&mut h).await.is_err() {
                                return;
                            }
                            let n = u32::from_be_bytes([0, h[0], h[1], h[2]]) as usize;
                            let mut payload = vec![0; n];
                            server.read_exact(&mut payload).await.unwrap();
                            if h[3] == 4 && h[4] == 0 {
                                server.write_all(&[0, 0, 0, 4, 1, 0, 0, 0, 0]).await.unwrap();
                            }
                            if h[3] == 6 && h[4] == 0 {
                                seen.set(seen.get() + 1);
                                if replying.get() {
                                    server.write_all(&[0, 0, 8, 6, 1, 0, 0, 0, 0]).await.unwrap();
                                    server.write_all(&payload).await.unwrap();
                                } else {
                                    delayed = Some(payload);
                                }
                            }
                            if h[3] == 1
                                && replying.get()
                                && let Some(payload) = delayed.take()
                            {
                                server.write_all(&[0, 0, 8, 6, 1, 0, 0, 0, 0]).await.unwrap();
                                server.write_all(&payload).await.unwrap();
                            }
                        }
                    });
                    let (send, conn) = h2::client::handshake(client).await.unwrap();
                    let active = Rc::new(Cell::new(true));
                    let polls = Rc::new(Cell::new(0));
                    let bdp = measuring.then(crate::activity::Bdp::new);
                    let (busy, count, sampler) = (active.clone(), polls.clone(), bdp.clone());
                    let driver = tokio::task::spawn_local(async move {
                        let keepalive = (!measuring).then_some((Duration::from_secs(1), Duration::from_secs(5)));
                        let mut f =
                            Box::pin(drive_observed(conn, keepalive, PingPayload::Random, move || busy.get(), None, sampler, false));
                        std::future::poll_fn(|cx| {
                            count.set(count.get() + 1);
                            f.as_mut().poll(cx)
                        })
                        .await
                    });
                    let mut send = send.ready().await.unwrap();
                    let (response, request) =
                        send.send_request(http::Request::builder().uri("http://test.local/").body(()).unwrap(), false).unwrap();
                    for _ in 0..50 {
                        tokio::task::yield_now().await;
                    }
                    if let Some(b) = &bdp {
                        b.observe(1024);
                    } else {
                        tokio::time::advance(Duration::from_secs(1)).await;
                    }
                    for _ in 0..50 {
                        tokio::task::yield_now().await;
                    }
                    assert_eq!(pings.get(), 1);
                    active.set(false);
                    drop(request);
                    drop(response);
                    for _ in 0..50 {
                        tokio::task::yield_now().await;
                    }
                    let before = polls.get();
                    tokio::time::advance(Duration::from_secs(60)).await;
                    for _ in 0..50 {
                        tokio::task::yield_now().await;
                    }
                    assert_eq!(polls.get(), before, "PING timeout woke an empty pool, BDP={measuring}");
                    assert!(!driver.is_finished());
                    answer.set(true);
                    active.set(true);
                    send = send.ready().await.unwrap();
                    let (response, request) =
                        send.send_request(http::Request::builder().uri("http://test.local/again").body(()).unwrap(), false).unwrap();
                    for _ in 0..50 {
                        tokio::task::yield_now().await;
                    }
                    if let Some(b) = &bdp {
                        b.observe(1024);
                    } else {
                        tokio::time::advance(Duration::from_secs(1)).await;
                    }
                    for _ in 0..50 {
                        tokio::task::yield_now().await;
                    }
                    assert_eq!(pings.get(), 2, "late PONG blocked the next PING, BDP={measuring}");
                    assert!(!driver.is_finished());
                    active.set(false);
                    drop(request);
                    drop(response);
                    for _ in 0..50 {
                        tokio::task::yield_now().await;
                    }
                    let before = polls.get();
                    tokio::time::advance(Duration::from_secs(60)).await;
                    for _ in 0..50 {
                        tokio::task::yield_now().await;
                    }
                    assert_eq!(polls.get(), before, "idle timer returned after a late PONG");
                    driver.abort();
                    peer.abort();
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn requests_in_flight_do_not_count_as_streams() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let dials = Rc::new(Cell::new(0));
                let cfg = Config::from_json(&serde_json::json!({
                    "mode": "packet-up", "path": "/x", "xmux": {"maxConcurrency": 2}
                }))
                .unwrap();
                let t = Transport::new(cfg, false, "example.com", "1.2.3.4", connector(dials.clone()));
                let first = t.conn().await.unwrap();
                let _post = first.again();
                let _second = t.conn().await.unwrap();
                assert_eq!(dials.get(), 1, "второй поток помещается в то же соединение");
                let _third = t.conn().await.unwrap();
                assert_eq!(dials.get(), 2, "третий — уже в новое");
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_opens_share_one_dial() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let dials = Rc::new(Cell::new(0));
                let connect: Connector = {
                    let dials = dials.clone();
                    Rc::new(move || {
                        dials.set(dials.get() + 1);
                        Box::pin(async move {
                            let (client, server) = tokio::io::duplex(1 << 16);
                            tokio::task::spawn_local(async move {
                                let mut h2 = h2::server::handshake(server).await.unwrap();
                                while let Some(Ok((_req, mut respond))) = h2.accept().await {
                                    let _ = respond.send_response(http::Response::new(()), false);
                                }
                            });
                            Ok(Box::new(client) as Box<dyn Io>)
                        })
                    })
                };
                let cfg = Config::from_json(&serde_json::json!({"mode": "stream-one", "path": "/x"})).unwrap();
                let t = Transport::new(cfg, false, "example.com", "1.2.3.4", connect);
                let (a, b, c) = tokio::join!(t.open(), t.open(), t.open());
                assert!(a.is_ok() && b.is_ok() && c.is_ok());
                assert_eq!(dials.get(), 1);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn server_stream_limit_opens_another_connection() {
        use tokio::io::AsyncReadExt;
        tokio::task::LocalSet::new()
            .run_until(async {
                let dials = Rc::new(Cell::new(0));
                let count = dials.clone();
                let connect: Connector = Rc::new(move || {
                    count.set(count.get() + 1);
                    Box::pin(async move {
                        let (client, server) = tokio::io::duplex(1 << 16);
                        tokio::task::spawn_local(async move {
                            let mut b = h2::server::Builder::new();
                            b.max_concurrent_streams(2);
                            let mut h2 = b.handshake::<_, Bytes>(server).await.unwrap();
                            let mut open = Vec::new();
                            while let Some(Ok((_req, mut respond))) = h2.accept().await {
                                let mut s = respond.send_response(http::Response::new(()), false).unwrap();
                                s.send_data(Bytes::from_static(b"hello"), false).unwrap();
                                open.push(s);
                            }
                        });
                        Ok(Box::new(client) as Box<dyn Io>)
                    })
                });
                let cfg = Config::from_json(&serde_json::json!({"mode": "stream-one", "path": "/x"})).unwrap();
                let t = Transport::new(cfg, false, "example.com", "1.2.3.4", connect);
                let mut streams = Vec::new();
                for i in 0..4 {
                    let mut s = t.open().await.unwrap();
                    let mut b = [0; 5];
                    tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut b))
                        .await
                        .unwrap_or_else(|_| panic!("stream {i} queued behind the server limit"))
                        .unwrap();
                    streams.push(s);
                }
                assert_eq!(dials.get(), 2);
            })
            .await;
    }

    #[test]
    fn h3_idle_close_is_three_keepalives_but_not_past_the_usual_close() {
        assert_eq!(h3_idle_close(None), IDLE_CLOSE);
        assert_eq!(h3_idle_close(Some(Duration::ZERO)), IDLE_CLOSE);
        assert_eq!(h3_idle_close(Some(Duration::from_secs(10))), Duration::from_secs(30));
        assert_eq!(h3_idle_close(Some(Duration::from_secs(u64::MAX))), IDLE_CLOSE);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn huge_xmux_times_from_a_profile_do_not_panic() {
        use tokio::io::AsyncReadExt;
        tokio::task::LocalSet::new()
            .run_until(async {
                let connect: Connector = Rc::new(move || {
                    Box::pin(async move {
                        let (client, server) = tokio::io::duplex(1 << 16);
                        tokio::task::spawn_local(async move {
                            let mut h2 = h2::server::Builder::new().handshake::<_, Bytes>(server).await.unwrap();
                            let mut open = Vec::new();
                            while let Some(Ok((_req, mut respond))) = h2.accept().await {
                                let mut s = respond.send_response(http::Response::new(()), false).unwrap();
                                s.send_data(Bytes::from_static(b"hello"), false).unwrap();
                                open.push(s);
                            }
                        });
                        Ok(Box::new(client) as Box<dyn Io>)
                    })
                });
                let cfg = Config::from_json(&serde_json::json!({"mode": "stream-one", "path": "/x",
                "xmux": {"hMaxReusableSecs": i64::MAX, "hKeepAlivePeriod": i64::MAX}}))
                .unwrap();
                let t = Transport::new(cfg, false, "example.com", "1.2.3.4", connect);
                for _ in 0..2 {
                    let mut s = t.open().await.unwrap();
                    let mut b = [0; 5];
                    tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut b)).await.unwrap().unwrap();
                    tokio::task::yield_now().await;
                }
            })
            .await;
    }

    struct SlowReads<T> {
        inner: T,
        budget: usize,
        tick: Pin<Box<tokio::time::Sleep>>,
    }
    impl<T: AsyncRead + Unpin> AsyncRead for SlowReads<T> {
        fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            if self.budget == 0 {
                ready!(self.tick.as_mut().poll(cx));
                let next = tokio::time::Instant::now() + Duration::from_millis(100);
                self.tick.as_mut().reset(next);
                self.budget = 16 << 10;
            }
            let mut b = vec![0; self.budget.min(buf.remaining())];
            let mut rb = ReadBuf::new(&mut b);
            ready!(Pin::new(&mut self.inner).poll_read(cx, &mut rb))?;
            self.budget -= rb.filled().len();
            buf.put_slice(rb.filled());
            Poll::Ready(Ok(()))
        }
    }
    impl<T: AsyncWrite + Unpin> AsyncWrite for SlowReads<T> {
        fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.inner).poll_write(cx, b)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn packet_up_posts_wait_for_the_body_on_a_slow_uplink() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::task::LocalSet::new()
            .run_until(async {
                let received = Rc::new(RefCell::new((0usize, Vec::new())));
                let seen = received.clone();
                let connect: Connector = Rc::new(move || {
                    let seen = seen.clone();
                    Box::pin(async move {
                        let (client, server) = tokio::io::duplex(64 << 10);
                        let server = SlowReads { inner: server, budget: 0, tick: Box::pin(tokio::time::sleep(Duration::ZERO)) };
                        tokio::task::spawn_local(async move {
                            let mut b = h2::server::Builder::new();
                            b.initial_window_size(1 << 20).initial_connection_window_size(1 << 20);
                            let mut h2 = b.handshake::<_, Bytes>(server).await.unwrap();
                            while let Some(Ok((req, mut respond))) = h2.accept().await {
                                let seen = seen.clone();
                                if req.method() == http::Method::GET {
                                    let s = respond.send_response(http::Response::new(()), false).unwrap();
                                    tokio::task::spawn_local(async move {
                                        let _s = s;
                                        std::future::pending::<()>().await
                                    });
                                    continue;
                                }
                                tokio::task::spawn_local(async move {
                                    let seq: usize = req.uri().path().rsplit('/').next().unwrap().parse().unwrap();
                                    let mut body = req.into_body();
                                    while let Some(Ok(d)) = body.data().await {
                                        let _ = body.flow_control().release_capacity(d.len());
                                        seen.borrow_mut().0 += d.len();
                                    }
                                    seen.borrow_mut().1.push(seq);
                                    let _ = respond.send_response(http::Response::new(()), true);
                                });
                            }
                        });
                        Ok(Box::new(client) as Box<dyn Io>)
                    })
                });
                let cfg = Config::from_json(&serde_json::json!({"mode": "packet-up", "path": "/x"})).unwrap();
                let t = Transport::new(cfg, false, "example.com", "1.2.3.4", connect);
                let mut s = t.open().await.unwrap();
                let t0 = tokio::time::Instant::now();
                for _ in 0..256 {
                    s.write_all(&[0x41; 16 << 10]).await.unwrap();
                }
                s.flush().await.unwrap();
                assert!(t0.elapsed() > Duration::from_secs(5), "4 MiB taken in {:?} at 160 KiB/s", t0.elapsed());
                while received.borrow().0 < 4 << 20 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    assert!(t0.elapsed() < Duration::from_secs(120));
                }
                let read = tokio::time::timeout(Duration::from_secs(60), s.read(&mut [0; 1])).await;
                assert!(read.is_err(), "session ended: {read:?}");
                let mut seqs = received.borrow().1.clone();
                seqs.sort();
                assert_eq!(seqs, (0..seqs.len()).collect::<Vec<_>>());
            })
            .await;
    }
}
