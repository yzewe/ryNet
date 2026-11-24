use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use h2::client::{ResponseFuture, SendRequest};
use h2::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::outbound::dial::Dialer;

const IDLE_CLOSE: Duration = Duration::from_secs(300);
const PREFACE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HUNK: usize = rv_xhttp::activity::XRAY_BUFFER;
const MAX_MESSAGE: usize = 4 << 20;
const DATA_FRAME_BUDGET: usize = rv_xhttp::activity::BDP_LIMIT as usize / 2;

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

fn path_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~$&+,;=:@".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn request_path(service_name: &str, multi: bool) -> String {
    if !service_name.starts_with('/') {
        let stream = if multi { "TunMulti" } else { "Tun" };
        return format!("/{}/{stream}", path_escape(service_name));
    }
    let last = service_name.rfind('/').unwrap_or(0).max(1);
    let service = service_name[1..last].split('/').map(path_escape).collect::<Vec<_>>().join("/");
    let ending = &service_name[service_name.rfind('/').unwrap_or(0) + 1..];
    let names: Vec<&str> = ending.split('|').collect();
    let stream = if multi && names.len() > 1 { names[1] } else { names[0] };
    format!("/{service}/{}", path_escape(stream))
}

pub struct Pool {
    dialer: Dialer,
    path: String,
    authority: String,
    user_agent: Option<String>,
    keepalive: Option<(Duration, Duration, bool)>,
    initial_window: u32,
    conn: RefCell<Option<Conn>>,
    stale: RefCell<Option<Conn>>,
    dialing: tokio::sync::Mutex<()>,
}

struct Conn {
    send: SendRequest<Bytes>,
    dead: Rc<Cell<bool>>,
    active: Rc<Cell<usize>>,
    idle: Rc<Idle>,
}

#[derive(Default)]
struct Idle {
    since: Cell<Option<tokio::time::Instant>>,
    timer: Cell<bool>,
}

struct Lease {
    pool: Weak<Pool>,
    active: Rc<Cell<usize>>,
    idle: Rc<Idle>,
    dead: Rc<Cell<bool>>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        let left = self.active.get().saturating_sub(1);
        self.active.set(left);
        if left > 0 {
            return;
        }
        self.idle.since.set(Some(tokio::time::Instant::now()));
        if self.idle.timer.replace(true) {
            return;
        }
        let (pool, active, idle) = (self.pool.clone(), self.active.clone(), self.idle.clone());
        tokio::task::spawn_local(async move {
            loop {
                let Some(since) = idle.since.get() else {
                    idle.timer.set(false);
                    return;
                };
                let deadline = since + IDLE_CLOSE;
                if tokio::time::Instant::now() < deadline {
                    tokio::time::sleep_until(deadline).await;
                    continue;
                }
                idle.timer.set(false);
                if let Some(p) = pool.upgrade() {
                    let mut c = p.conn.borrow_mut();
                    if c.as_ref().is_some_and(|c| Rc::ptr_eq(&c.active, &active)) {
                        if p.keepalive.is_some_and(|k| k.2) {
                            *c = None;
                            tracing::debug!(target: "rv::pool", "grpc idle close");
                        } else {
                            *p.stale.borrow_mut() = c.take();
                            tracing::debug!(target: "rv::pool", "grpc idle: retired, closed with the next dial");
                        }
                    }
                }
                return;
            }
        });
    }
}

impl Pool {
    pub fn reset(&self) {
        self.stale.borrow_mut().take();
        if self.conn.borrow_mut().take().is_some() {
            tracing::debug!(target: "rv::pool", "grpc reset");
        }
    }

    pub fn new(cfg: &rv_config::Grpc, dialer: Dialer, authority: String) -> Rc<Self> {
        let user_agent = match cfg.user_agent.as_str() {
            "" | "chrome" => Some(rv_xhttp::browser::chrome_ua()),
            "golang" => None,
            "firefox" | "edge" | "safari" => {
                let mut h = vec![("User-Agent".to_string(), cfg.user_agent.clone())];
                rv_xhttp::browser::apply_defaults(&mut h, rv_xhttp::browser::Variant::Ws);
                rv_xhttp::browser::get(&h, "User-Agent").map(String::from)
            }
            other => Some(other.to_string()),
        };
        Rc::new(Self {
            path: request_path(&cfg.service_name, cfg.multi_mode),
            dialer,
            authority,
            user_agent,
            initial_window: cfg.initial_window_size,
            keepalive: (cfg.idle_timeout > 0).then(|| {
                (
                    Duration::from_secs(cfg.idle_timeout.max(10).into()),
                    Duration::from_secs(cfg.health_check_timeout.max(1).into()),
                    cfg.permit_without_stream,
                )
            }),
            conn: RefCell::new(None),
            stale: RefCell::new(None),
            dialing: tokio::sync::Mutex::new(()),
        })
    }

    async fn conn(self: &Rc<Self>) -> io::Result<(SendRequest<Bytes>, Lease)> {
        let lease = |c: &Conn| {
            c.active.set(c.active.get() + 1);
            c.idle.since.set(None);
            Lease { pool: Rc::downgrade(self), active: c.active.clone(), idle: c.idle.clone(), dead: c.dead.clone() }
        };
        if let Some(c) = self.conn.borrow().as_ref().filter(|c| !c.dead.get()) {
            return Ok((c.send.clone(), lease(c)));
        }
        let _dialing = self.dialing.lock().await;
        if let Some(c) = self.conn.borrow().as_ref().filter(|c| !c.dead.get()) {
            return Ok((c.send.clone(), lease(c)));
        }
        let t0 = std::time::Instant::now();
        self.stale.borrow_mut().take();
        let io = self.dialer.stream().await.map_err(io::Error::other)?;
        let bdp = (self.initial_window == 0).then(rv_xhttp::activity::Bdp::new);
        let mut io = rv_xhttp::activity::FrameFlush::new(io, rv_xhttp::activity::Records::GrpcGo);
        let preface = io.server_preface();
        let (io, activity) = rv_xhttp::activity::ActivityIo::with_bdp(io, bdp.clone());
        let mut builder = h2::client::Builder::new();
        builder.window_refresh(h2::WindowRefresh::Quarter).data_frame_budget(DATA_FRAME_BUDGET);
        if self.initial_window >= 65535 {
            builder.initial_window_size(self.initial_window);
        }
        let (send, connection) = builder.handshake::<_, Bytes>(io).await.map_err(h2err)?;
        let c = self.connected(send.clone(), connection, activity, bdp);
        if tokio::time::timeout(PREFACE_TIMEOUT, preface.wait()).await.is_err() {
            c.dead.set(true);
            return Err(io::Error::new(io::ErrorKind::TimedOut, "grpc: no server preface"));
        }
        let l = lease(&c);
        *self.conn.borrow_mut() = Some(c);
        tracing::debug!(target: "rv::pool", "grpc new conn ms={}", t0.elapsed().as_millis());
        Ok((send, l))
    }

    fn connected<T: AsyncRead + AsyncWrite + Unpin + 'static>(
        &self,
        send: SendRequest<Bytes>,
        connection: h2::client::Connection<T, Bytes>,
        activity: rv_xhttp::activity::Activity,
        bdp: Option<Rc<rv_xhttp::activity::Bdp>>,
    ) -> Conn {
        let dead = Rc::new(Cell::new(false));
        let active = Rc::new(Cell::new(0));
        {
            let dead = dead.clone();
            let keepalive = self.keepalive;
            let active = active.clone();
            let bdp = bdp.clone();
            tokio::task::spawn_local(async move {
                let busy = move || active.get() > 0 || keepalive.is_some_and(|k| k.2);
                let r = rv_xhttp::transport::drive_observed(
                    connection,
                    keepalive.map(|k| (k.0, k.1)),
                    rv_xhttp::transport::PingPayload::Zero,
                    busy,
                    Some(activity),
                    bdp,
                    false,
                )
                .await;
                if let Err(e) = r {
                    tracing::debug!("grpc h2 connection: {e}");
                }
                dead.set(true);
            });
        }
        Conn { send, dead, active, idle: Rc::new(Idle::default()) }
    }

    pub async fn open(self: &Rc<Self>) -> io::Result<GrpcStream> {
        for attempt in 0..2 {
            let (send, lease) = self.conn().await?;
            let mut b = http::Request::builder()
                .extension(h2::ext::PseudoHeaderOrder::Grpc)
                .method("POST")
                .uri(format!("http://{}{}", self.authority, self.path))
                .header("content-type", "application/grpc");
            if let Some(ua) = &self.user_agent {
                b = b.header("user-agent", ua.as_str());
            }
            let req = b.header("te", "trailers").body(()).map_err(io::Error::other)?;
            let sent = match send.ready().await {
                Ok(mut send) => send.send_request(req, false),
                Err(e) => Err(e),
            };
            match sent {
                Ok((resp, stream)) => {
                    return Ok(GrpcStream {
                        resp: Some(resp),
                        recv: None,
                        send: Some(stream),
                        inbuf: BytesMut::new(),
                        out: VecDeque::new(),
                        pending: Bytes::new(),
                        header_status: false,
                        _lease: lease,
                    });
                }
                Err(e) if attempt == 0 && (e.is_go_away() || e.is_io()) => {
                    lease.dead.set(true);
                    tracing::debug!(target: "rv::pool", "grpc conn closing ({e}), retry on a fresh one");
                }
                Err(e) => return Err(h2err(e)),
            }
        }
        unreachable!()
    }
}

pub struct GrpcStream {
    resp: Option<ResponseFuture>,
    recv: Option<RecvStream>,
    send: Option<SendStream<Bytes>>,
    inbuf: BytesMut,
    out: VecDeque<Bytes>,
    pending: Bytes,
    header_status: bool,
    _lease: Lease,
}

fn put_varint(b: &mut BytesMut, mut v: u64) {
    while v >= 0x80 {
        b.put_u8(v as u8 | 0x80);
        v >>= 7;
    }
    b.put_u8(v as u8);
}

fn get_varint(b: &mut Bytes) -> io::Result<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        if !b.has_remaining() {
            break;
        }
        let x = b.get_u8();
        v |= ((x & 0x7f) as u64) << shift;
        if x & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(io::Error::other("grpc: bad varint"))
}

fn encode_hunk(data: &[u8]) -> Bytes {
    let varint_len = (64 - (data.len() as u64).leading_zeros()).max(1).div_ceil(7) as usize;
    let msg_len = 1 + varint_len + data.len();
    let mut f = BytesMut::with_capacity(5 + msg_len);
    f.put_u8(0);
    f.put_u32(msg_len as u32);
    f.put_u8(0x0a);
    put_varint(&mut f, data.len() as u64);
    f.put_slice(data);
    f.freeze()
}

fn decode_hunks(mut m: Bytes, out: &mut VecDeque<Bytes>) -> io::Result<()> {
    let mut first: Option<Bytes> = None;
    let mut joined: Option<BytesMut> = None;
    while m.has_remaining() {
        let key = get_varint(&mut m)?;
        match key & 7 {
            0 => {
                get_varint(&mut m)?;
            }
            1 if m.remaining() >= 8 => m.advance(8),
            5 if m.remaining() >= 4 => m.advance(4),
            2 => {
                let n = usize::try_from(get_varint(&mut m)?).unwrap_or(usize::MAX);
                if m.remaining() < n {
                    return Err(io::Error::other("grpc: truncated field"));
                }
                let v = m.split_to(n);
                if key >> 3 == 1 && !v.is_empty() {
                    match (&first, &mut joined) {
                        (None, _) => first = Some(v),
                        (Some(f), None) => {
                            let mut j = BytesMut::with_capacity(f.len() + v.len());
                            j.extend_from_slice(f);
                            j.extend_from_slice(&v);
                            joined = Some(j);
                        }
                        (Some(_), Some(j)) => j.extend_from_slice(&v),
                    }
                }
            }
            _ => return Err(io::Error::other("grpc: bad protobuf")),
        }
    }
    if let Some(data) = joined.map(BytesMut::freeze).or(first) {
        out.push_back(data);
    }
    Ok(())
}

impl GrpcStream {
    fn parse(&mut self) -> io::Result<()> {
        while self.out.is_empty() && self.inbuf.len() >= 5 {
            if self.inbuf[0] != 0 {
                return Err(io::Error::other("grpc: compressed messages are not supported"));
            }
            let len = u32::from_be_bytes(self.inbuf[1..5].try_into().unwrap()) as usize;
            if len > MAX_MESSAGE {
                return Err(io::Error::other("grpc: message too large"));
            }
            if self.inbuf.len() < 5 + len {
                break;
            }
            self.inbuf.advance(5);
            let msg = self.inbuf.split_to(len).freeze();
            decode_hunks(msg, &mut self.out)?;
        }
        Ok(())
    }

    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.pending.is_empty() {
            let Some(send) = self.send.as_mut() else {
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            };
            send.reserve_capacity(self.pending.len());
            match ready!(send.poll_capacity(cx)) {
                Some(Ok(n)) if n > 0 => {
                    let chunk = self.pending.split_to(n.min(self.pending.len()));
                    send.send_data(chunk, false).map_err(h2err)?;
                }
                Some(Ok(_)) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Some(Err(e)) => return Poll::Ready(Err(h2err(e))),
                None => return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for GrpcStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if let Some(front) = me.out.front_mut() {
                let n = front.len().min(buf.remaining());
                buf.put_slice(&front.split_to(n));
                if front.is_empty() {
                    me.out.pop_front();
                }
                return Poll::Ready(Ok(()));
            }
            if !me.inbuf.is_empty() {
                me.parse()?;
                if !me.out.is_empty() {
                    continue;
                }
            }
            if let Some(resp) = me.resp.as_mut() {
                let r = ready!(Pin::new(resp).poll(cx)).map_err(h2err)?;
                me.resp = None;
                if r.status() != 200 {
                    return Poll::Ready(Err(io::Error::other(format!("grpc: status {}", r.status()))));
                }
                if let Some(status) = r.headers().get("grpc-status") {
                    if status != "0" {
                        return Poll::Ready(Err(io::Error::other(format!("grpc-status {}", status.to_str().unwrap_or("invalid")))));
                    }
                    me.header_status = true;
                }
                me.recv = Some(r.into_body());
            }
            let Some(recv) = me.recv.as_mut() else {
                return Poll::Ready(Ok(()));
            };
            match ready!(recv.poll_data(cx)) {
                Some(Ok(d)) => {
                    let _ = recv.flow_control().release_capacity(d.len());
                    me.inbuf.extend_from_slice(&d);
                    me.parse()?;
                }
                Some(Err(e)) => return Poll::Ready(Err(h2err(e))),
                None => {
                    if !me.inbuf.is_empty() {
                        return Poll::Ready(Err(io::Error::new(io::ErrorKind::UnexpectedEof, "grpc: truncated frame")));
                    }
                    if let Some(t) = ready!(recv.poll_trailers(cx)).map_err(h2err)?
                        && let Some(status) = t.get("grpc-status")
                    {
                        if status != "0" {
                            return Poll::Ready(Err(io::Error::other(format!("grpc-status {}", status.to_str().unwrap_or("invalid")))));
                        }
                        me.header_status = true;
                    }
                    if !me.header_status {
                        return Poll::Ready(Err(io::Error::other("grpc: missing grpc-status")));
                    }
                    return Poll::Ready(Ok(()));
                }
            }
        }
    }
}

impl AsyncWrite for GrpcStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let me = &mut *self;
        ready!(me.poll_pending(cx))?;
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = buf.len().min(MAX_HUNK);
        me.pending = encode_hunk(&buf[..n]);
        let _ = me.poll_pending(cx)?;
        Poll::Ready(Ok(n))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_pending(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_pending(cx))?;
        if let Some(mut s) = self.send.take() {
            let _ = s.send_data(Bytes::new(), true);
        }
        Poll::Ready(Ok(()))
    }
}

#[cfg(kani)]
#[path = "../proofs/grpc.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "../fuzzing/grpc.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_hunks_cost_one_queue_entry_per_message() {
        let mut pb = Vec::new();
        for i in 0..100_000u32 {
            pb.extend_from_slice(&[0x0a, 0x01, i as u8]);
        }
        let mut out = VecDeque::new();
        decode_hunks(Bytes::from(pb), &mut out).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 100_000);
        assert!(out[0].iter().enumerate().all(|(i, &b)| b == i as u8));
    }

    #[test]
    fn field_length_past_usize_is_an_error() {
        let mut pb = BytesMut::new();
        pb.put_u8(0x0a);
        put_varint(&mut pb, (1 << 32) + 1);
        pb.put_u8(0x41);
        let mut out = VecDeque::new();
        assert!(decode_hunks(pb.freeze(), &mut out).is_err());
        assert!(out.is_empty());
    }

    #[test]
    fn small_messages_are_decoded_one_at_a_time() {
        let mut inbuf = BytesMut::new();
        for i in 0..1000u32 {
            inbuf.extend_from_slice(&encode_hunk(&[i as u8]));
        }
        let mut s = std::mem::ManuallyDrop::new(GrpcStream {
            resp: None,
            recv: None,
            send: None,
            inbuf,
            out: VecDeque::new(),
            pending: Bytes::new(),
            header_status: false,
            _lease: Lease {
                pool: Weak::new(),
                active: Rc::new(Cell::new(2)),
                idle: Rc::new(Idle::default()),
                dead: Rc::new(Cell::new(false)),
            },
        });
        s.parse().unwrap();
        assert_eq!(s.out.len(), 1);
        let mut got = Vec::new();
        while let Some(h) = s.out.pop_front() {
            got.extend_from_slice(&h);
            s.parse().unwrap();
            assert!(s.out.len() <= 1);
        }
        assert_eq!(got, (0..1000u32).map(|i| i as u8).collect::<Vec<_>>());
        assert!(s.inbuf.is_empty());
    }

    struct ObservedReads<T> {
        inner: T,
        bytes: Rc<Cell<usize>>,
    }
    impl<T: AsyncRead + Unpin> AsyncRead for ObservedReads<T> {
        fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            let before = b.filled().len();
            let result = Pin::new(&mut self.inner).poll_read(cx, b);
            self.bytes.set(self.bytes.get() + b.filled().len() - before);
            result
        }
    }
    impl<T: AsyncWrite + Unpin> AsyncWrite for ObservedReads<T> {
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

    struct RecordedReads<T> {
        inner: T,
        bytes: Rc<RefCell<Vec<u8>>>,
    }
    impl<T: AsyncRead + Unpin> AsyncRead for RecordedReads<T> {
        fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            let before = b.filled().len();
            let result = Pin::new(&mut self.inner).poll_read(cx, b);
            self.bytes.borrow_mut().extend_from_slice(&b.filled()[before..]);
            result
        }
    }
    impl<T: AsyncWrite + Unpin> AsyncWrite for RecordedReads<T> {
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

    fn client_frames(bytes: &[u8]) -> Vec<(u8, u8, u32, Vec<u8>)> {
        let (mut b, mut out) = (&bytes[24.min(bytes.len())..], Vec::new());
        while b.len() >= 9 {
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]) as usize;
            if b.len() < 9 + n {
                break;
            }
            out.push((b[3], b[4], u32::from_be_bytes([b[5], b[6], b[7], b[8]]) & 0x7fff_ffff, b[9..9 + n].to_vec()));
            b = &b[9 + n..];
        }
        out
    }

    fn grpc_pool(settings: serde_json::Value, port: u16) -> Rc<Pool> {
        let config = serde_json::json!({"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "127.0.0.1", "port": port, "password": "loopback-test"}]},
            "streamSettings": {"network": "grpc", "grpcSettings": settings}}]});
        let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0);
        let out = p.outbounds.iter().find(|o| o.kind.is_proxy()).unwrap();
        let rv_config::Network::Grpc(g) = &out.stream.network else { panic!() };
        Pool::new(g, Dialer::new(&p, &out.stream, "127.0.0.1", port, None).unwrap(), "test.local".into())
    }

    async fn recording_server(body: usize) -> (u16, Rc<RefCell<Vec<u8>>>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let written = Rc::new(RefCell::new(Vec::new()));
        let bytes = written.clone();
        let server = tokio::task::spawn_local(async move {
            let (io, _) = listener.accept().await.unwrap();
            let mut c = h2::server::handshake(RecordedReads { inner: io, bytes }).await.unwrap();
            while let Some(Ok((_req, mut reply))) = c.accept().await {
                let headers = http::Response::builder().header("content-type", "application/grpc").body(()).unwrap();
                let mut send = reply.send_response(headers, false).unwrap();
                tokio::task::spawn_local(async move {
                    let hunk = encode_hunk(&[0x5a; 8000]);
                    let mut sent = 0;
                    while sent < body {
                        let mut data = hunk.clone();
                        while !data.is_empty() {
                            send.reserve_capacity(data.len());
                            let Some(Ok(n)) = std::future::poll_fn(|cx| send.poll_capacity(cx)).await else { return };
                            if send.send_data(data.split_to(n.min(data.len())), false).is_err() {
                                return;
                            }
                        }
                        sent += 8000;
                    }
                    std::future::pending::<()>().await;
                    drop(send);
                });
            }
        });
        (port, written, server)
    }

    async fn burst_server(frames: usize, payload: usize) -> (u16, Rc<RefCell<Vec<u8>>>, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let frame = |t: u8, flags: u8, id: u32, body: &[u8]| {
            let mut f = (body.len() as u32).to_be_bytes()[1..].to_vec();
            f.extend([t, flags]);
            f.extend(id.to_be_bytes());
            f.extend_from_slice(body);
            f
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let written = Rc::new(RefCell::new(Vec::new()));
        let bytes = written.clone();
        let server = tokio::task::spawn_local(async move {
            let (mut io, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 1 << 16];
            io.write_all(&frame(4, 0, 0, &[])).await.unwrap();
            while !client_frames(&bytes.borrow()).iter().any(|f| f.0 == 1) {
                let n = io.read(&mut buf).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.borrow_mut().extend_from_slice(&buf[..n]);
            }
            let mut out = frame(4, 1, 0, &[]);
            let mut headers = vec![0x88, 0x0f, 0x10, 16];
            headers.extend_from_slice(b"application/grpc");
            out.extend(frame(1, 4, 1, &headers));
            let hunk = encode_hunk(&vec![0x5a; payload]);
            for _ in 0..frames {
                out.extend(frame(0, 0, 1, &hunk));
            }
            io.write_all(&out).await.unwrap();
            loop {
                let n = io.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                bytes.borrow_mut().extend_from_slice(&buf[..n]);
            }
        });
        (port, written, server)
    }

    fn connection_updates(frames: &[(u8, u8, u32, Vec<u8>)]) -> Vec<u32> {
        frames.iter().filter(|f| f.0 == 8 && f.2 == 0).map(|f| u32::from_be_bytes(f.3[..4].try_into().unwrap())).collect()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connection_window_update_per_frame_like_grpc_go() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (port, written, server) = burst_server(8, 8000).await;
                let pool = grpc_pool(serde_json::json!({"initial_windows_size": 1 << 20}), port);
                let s = pool.open().await.unwrap();
                tokio::time::sleep(Duration::from_millis(300)).await;
                assert_eq!(connection_updates(&client_frames(&written.borrow())), [24024, 24024]);
                drop(s);
                pool.reset();
                server.abort();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn small_data_frames_of_an_unread_stream_keep_the_connection() {
        use tokio::io::AsyncReadExt;
        tokio::task::LocalSet::new()
            .run_until(async {
                let (port, written, server) = burst_server(1300, 40).await;
                let pool = grpc_pool(serde_json::json!({}), port);
                let mut s = pool.open().await.unwrap();
                tokio::time::sleep(Duration::from_millis(300)).await;
                let mut b = vec![0; 1300 * 40];
                tokio::time::timeout(Duration::from_secs(5), s.read_exact(&mut b)).await.unwrap().unwrap();
                assert!(b.iter().all(|&x| x == 0x5a));
                assert!(!client_frames(&written.borrow()).iter().any(|f| f.0 == 7), "client sent GOAWAY");
                drop(s);
                pool.reset();
                server.abort();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configured_window_keeps_connection_window_like_grpc_go() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (port, written, server) = recording_server(0).await;
                let pool = grpc_pool(serde_json::json!({"initial_windows_size": 1 << 20}), port);
                let s = pool.open().await.unwrap();
                tokio::time::sleep(Duration::from_millis(200)).await;
                let frames = client_frames(&written.borrow());
                let settings = frames.iter().find(|f| f.0 == 4 && f.1 & 1 == 0).expect("SETTINGS");
                assert!(settings.3.chunks(6).any(|s| s == [0, 4, 0, 0x10, 0, 0]), "INITIAL_WINDOW_SIZE 1 MiB: {settings:?}");
                assert!(!frames.iter().any(|f| f.0 == 8 && f.2 == 0), "grpc-go keeps the 65535 connection window: {frames:?}");
                drop(s);
                pool.reset();
                server.abort();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bdp_ping_follows_connection_window_update_like_grpc_go() {
        use tokio::io::AsyncReadExt;
        tokio::task::LocalSet::new()
            .run_until(async {
                let (port, written, server) = recording_server(8000).await;
                let pool = grpc_pool(serde_json::json!({}), port);
                let mut s = pool.open().await.unwrap();
                let mut b = vec![0; 8000];
                tokio::time::timeout(Duration::from_secs(5), s.read_exact(&mut b)).await.unwrap().unwrap();
                tokio::time::sleep(Duration::from_millis(200)).await;
                let frames = client_frames(&written.borrow());
                let i = frames.iter().position(|f| f.0 == 6 && f.1 & 1 == 0 && f.3 == [2, 4, 16, 16, 9, 14, 7, 7]).expect("BDP ping");
                let before = &frames[i - 1];
                assert!(before.0 == 8 && before.2 == 0, "connection WINDOW_UPDATE right before the BDP ping: {frames:?}");
                assert_eq!(u32::from_be_bytes(before.3[..4].try_into().unwrap()), 8008, "the whole received DATA is returned");
                drop(s);
                pool.reset();
                server.abort();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bdp_growth_keeps_connection_window_at_stream_window_like_grpc_go() {
        use tokio::io::AsyncReadExt;
        tokio::task::LocalSet::new()
            .run_until(async {
                let total = 8000 * 1024;
                let (port, written, server) = recording_server(total).await;
                let pool = grpc_pool(serde_json::json!({}), port);
                let mut s = pool.open().await.unwrap();
                let mut b = vec![0; total];
                tokio::time::timeout(Duration::from_secs(20), s.read_exact(&mut b)).await.unwrap().unwrap();
                tokio::time::sleep(Duration::from_millis(200)).await;
                let frames = client_frames(&written.borrow());
                let windows: Vec<u32> = frames
                    .iter()
                    .filter(|f| f.0 == 4 && f.1 & 1 == 0)
                    .flat_map(|f| {
                        f.3.chunks(6)
                            .filter(|s| s[..2] == [0, 4])
                            .map(|s| u32::from_be_bytes(s[2..6].try_into().unwrap()))
                            .collect::<Vec<_>>()
                    })
                    .collect();
                let largest = windows.iter().copied().max().unwrap_or(65535).max(65535) as u64;
                assert!(largest > 65535, "BDP never widened the window: {windows:?}");
                let granted: u64 = 65535
                    + frames
                        .iter()
                        .filter(|f| f.0 == 8 && f.2 == 0)
                        .map(|f| u32::from_be_bytes(f.3[..4].try_into().unwrap()) as u64)
                        .sum::<u64>();
                let received = (total / 8000 * 8008) as u64;
                assert!(granted <= received + largest, "connection credit {} past the stream window {largest}", granted - received);
                let mut limit = 65535;
                for (i, f) in frames.iter().enumerate().skip(1).filter(|(_, f)| f.0 == 4 && f.1 & 1 == 0) {
                    let Some(n) = f.3.chunks(6).find(|s| s[..2] == [0, 4]).map(|s| u32::from_be_bytes(s[2..6].try_into().unwrap())) else {
                        continue;
                    };
                    assert_eq!(connection_updates(&frames[i - 1..i]), [n - limit], "growth to {n}: {:?}", frames[i - 1]);
                    limit = n;
                }
                drop(s);
                pool.reset();
                server.abort();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn keepalive_ping_is_zero_like_grpc_go() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (client, server_io) = tokio::io::duplex(1 << 16);
                let written = Rc::new(RefCell::new(Vec::new()));
                let server_io = RecordedReads { inner: server_io, bytes: written.clone() };
                let server = tokio::task::spawn_local(async move {
                    let mut c = h2::server::handshake(server_io).await.unwrap();
                    let mut responses = Vec::new();
                    while let Some(Ok((_req, mut reply))) = c.accept().await {
                        responses.push(reply.send_response(http::Response::new(()), false).unwrap());
                    }
                });
                let pool = grpc_pool(serde_json::json!({"idle_timeout": 10, "health_check_timeout": 5}), 1);
                let (client, activity) = rv_xhttp::activity::ActivityIo::with_bdp(client, None);
                let (send, conn) = h2::client::handshake(client).await.unwrap();
                *pool.conn.borrow_mut() = Some(pool.connected(send, conn, activity, None));
                let stream = pool.open().await.unwrap();
                for _ in 0..50 {
                    tokio::task::yield_now().await;
                }
                tokio::time::advance(Duration::from_secs(11)).await;
                for _ in 0..50 {
                    tokio::task::yield_now().await;
                }
                let pings: Vec<_> = client_frames(&written.borrow()).into_iter().filter(|f| f.0 == 6 && f.1 & 1 == 0).collect();
                assert!(!pings.is_empty(), "no keepalive PING");
                assert!(pings.iter().all(|p| p.3 == [0; 8]), "grpc-go keepalive PING carries zeros: {pings:?}");
                drop(stream);
                server.abort();
            })
            .await;
    }

    #[test]
    fn service_paths_like_xray() {
        assert_eq!(request_path("rkn-test", false), "/rkn-test/Tun");
        assert_eq!(request_path("svc/grpc", true), "/svc%2Fgrpc/TunMulti");
        assert_eq!(request_path("/my/sample/path1|path2", false), "/my/sample/path1");
        assert_eq!(request_path("/my/sample/path1|path2", true), "/my/sample/path2");
        assert_eq!(request_path("/a/b", true), "/a/b");
    }

    #[test]
    fn hunk_roundtrip() {
        let f = encode_hunk(b"hello");
        assert_eq!(&f[..], &[0, 0, 0, 0, 7, 0x0a, 5, b'h', b'e', b'l', b'l', b'o']);
        let mut out = VecDeque::new();
        decode_hunks(Bytes::copy_from_slice(&f[5..]), &mut out).unwrap();
        assert_eq!(out.pop_front().unwrap(), Bytes::from_static(b"hello"));
    }

    #[test]
    fn hunk_varint_boundaries_keep_wire_bytes() {
        for (len, varint) in [
            (0, vec![0]),
            (1, vec![1]),
            (127, vec![127]),
            (128, vec![128, 1]),
            (16383, vec![255, 127]),
            (16384, vec![128, 128, 1]),
            (MAX_HUNK, vec![128, 64]),
            (32768, vec![128, 128, 2]),
            (MAX_MESSAGE, vec![128, 128, 128, 2]),
        ] {
            let data = vec![0x5a; len];
            let f = encode_hunk(&data);
            let mut expected = vec![0];
            expected.extend_from_slice(&((1 + varint.len() + len) as u32).to_be_bytes());
            expected.push(0x0a);
            expected.extend_from_slice(&varint);
            expected.extend_from_slice(&data);
            assert_eq!(&f[..], expected, "length {len}");
            let mut out = VecDeque::new();
            decode_hunks(f.slice(5..), &mut out).unwrap();
            if len > 0 {
                assert_eq!(&out.pop_front().unwrap()[..], data);
            }
            assert!(out.is_empty());
        }
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn empty_pool_keepalive_permission_and_close() {
        tokio::task::LocalSet::new().run_until(async {
            for permit in [false, true] {
                let (client, server_io) = tokio::io::duplex(1 << 16); let observed = Rc::new(Cell::new(0));
                let server_io = ObservedReads { inner: server_io, bytes: observed.clone() };
                let server = tokio::task::spawn_local(async move {
                    let mut c = h2::server::handshake(server_io).await.unwrap(); let mut responses = Vec::new();
                    while let Some(Ok((_req, mut reply))) = c.accept().await { responses.push(reply.send_response(http::Response::new(()), false).unwrap()); }
                });
                let config = serde_json::json!({"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "127.0.0.1", "port": 1, "password": "loopback-test"}]},
                    "streamSettings": {"network": "grpc", "grpcSettings": {"idle_timeout": 10, "health_check_timeout": 1, "permit_without_stream": permit}}}]});
                let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0); let out = p.outbounds.iter().find(|o| o.kind.is_proxy()).unwrap();
                let rv_config::Network::Grpc(g) = &out.stream.network else { panic!() }; let dialer = Dialer::new(&p, &out.stream, "127.0.0.1", 1, None).unwrap();
                assert_eq!(g.idle_timeout, 10); assert_eq!(g.permit_without_stream, permit);
                let pool = Pool::new(g, dialer, "test.local".into()); let bdp = rv_xhttp::activity::Bdp::new();
                let (client, activity) = rv_xhttp::activity::ActivityIo::with_bdp(client, Some(bdp.clone()));
                let (send, conn) = h2::client::handshake(client).await.unwrap(); *pool.conn.borrow_mut() = Some(pool.connected(send, conn, activity, Some(bdp)));
                let stream = pool.open().await.unwrap();
                for _ in 0..50 { tokio::task::yield_now().await; }
                let dead = pool.conn.borrow().as_ref().unwrap().dead.clone(); drop(stream);
                for _ in 0..50 { tokio::task::yield_now().await; }
                let before = observed.get();
                tokio::time::advance(Duration::from_secs(20)).await; for _ in 0..50 { tokio::task::yield_now().await; }
                let after = observed.get();
                if permit { assert!(after > before, "explicit idle keepalive did not send PING"); }
                else { assert_eq!(after, before, "empty pool sent traffic without permission"); }
                assert!(!dead.get()); assert!(pool.conn.borrow().is_some());
                tokio::time::advance(Duration::from_secs(280)).await; for _ in 0..50 { tokio::task::yield_now().await; }
                assert!(pool.conn.borrow().is_none(), "empty gRPC pool survived the XHTTP hold");
                if permit { assert!(dead.get(), "idle close left the HTTP/2 driver alive"); }
                else {
                    let quiet = observed.get();
                    tokio::time::advance(Duration::from_secs(600)).await; for _ in 0..50 { tokio::task::yield_now().await; }
                    assert_eq!(observed.get(), quiet, "retired connection sent traffic");
                    assert!(pool.stale.borrow().is_some());
                    pool.reset();
                    for _ in 0..50 { tokio::task::yield_now().await; }
                    assert!(dead.get(), "reset left the retired HTTP/2 driver alive");
                }
                server.abort();
            }
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn goaway_moves_new_streams_to_a_fresh_connection() {
        tokio::task::LocalSet::new().run_until(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(); let port = listener.local_addr().unwrap().port();
            let accepted = Rc::new(Cell::new(0)); let count = accepted.clone();
            let server = tokio::task::spawn_local(async move {
                loop {
                    let (io, _) = listener.accept().await.unwrap(); count.set(count.get() + 1); let first = count.get() == 1;
                    tokio::task::spawn_local(async move {
                        let mut c = h2::server::handshake(io).await.unwrap(); let mut replies = Vec::new();
                        while let Some(Ok((_req, mut reply))) = c.accept().await {
                            let headers = http::Response::builder().header("content-type", "application/grpc").body(()).unwrap();
                            replies.push(reply.send_response(headers, false).unwrap());
                            if first { c.graceful_shutdown(); }
                        }
                    });
                }
            });
            let config = serde_json::json!({"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "127.0.0.1", "port": port, "password": "loopback-test"}]}, "streamSettings": {"network": "grpc"}}]});
            let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0); let out = p.outbounds.iter().find(|o| o.kind.is_proxy()).unwrap();
            let rv_config::Network::Grpc(g) = &out.stream.network else { panic!() }; let dialer = Dialer::new(&p, &out.stream, "127.0.0.1", port, None).unwrap();
            let pool = Pool::new(g, dialer, "test.local".into());
            let first = pool.open().await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            let second = pool.open().await.expect("GOAWAY must move new streams to another connection");
            assert_eq!(accepted.get(), 2);
            drop((first, second)); pool.reset(); server.abort();
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unread_stream_does_not_freeze_the_connection() {
        tokio::task::LocalSet::new().run_until(async {
            for settings in [serde_json::json!({}), serde_json::json!({"initial_windows_size": 1 << 20})] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(); let port = listener.local_addr().unwrap().port();
                let server = tokio::task::spawn_local(async move {
                    let (io, _) = listener.accept().await.unwrap(); let mut c = h2::server::handshake(io).await.unwrap(); let mut first = true;
                    while let Some(Ok((_req, mut reply))) = c.accept().await {
                        let headers = http::Response::builder().header("content-type", "application/grpc").body(()).unwrap();
                        let mut send = reply.send_response(headers, false).unwrap();
                        let total = if std::mem::take(&mut first) { 64 << 20 } else { 1 << 20 };
                        tokio::task::spawn_local(async move {
                            let hunk = encode_hunk(&[0x5a; 8000]); let mut sent = 0;
                            while sent < total {
                                let mut data = hunk.clone();
                                while !data.is_empty() {
                                    send.reserve_capacity(data.len());
                                    let Some(Ok(n)) = std::future::poll_fn(|cx| send.poll_capacity(cx)).await else { return };
                                    if send.send_data(data.split_to(n.min(data.len())), false).is_err() { return; }
                                }
                                sent += 8000;
                            }
                            let mut trailers = http::HeaderMap::new(); trailers.insert("grpc-status", "0".parse().unwrap()); let _ = send.send_trailers(trailers);
                        });
                    }
                });
                let config = serde_json::json!({"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "127.0.0.1", "port": port, "password": "loopback-test"}]}, "streamSettings": {"network": "grpc", "grpcSettings": settings}}]});
                let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0); let out = p.outbounds.iter().find(|o| o.kind.is_proxy()).unwrap();
                let rv_config::Network::Grpc(g) = &out.stream.network else { panic!() }; let dialer = Dialer::new(&p, &out.stream, "127.0.0.1", port, None).unwrap();
                let pool = Pool::new(g, dialer, "test.local".into());
                let unread = pool.open().await.unwrap();
                tokio::time::sleep(Duration::from_millis(300)).await;
                let mut s = pool.open().await.unwrap(); let mut b = vec![0; 1 << 20];
                tokio::time::timeout(Duration::from_secs(5), tokio::io::AsyncReadExt::read_exact(&mut s, &mut b)).await
                    .unwrap_or_else(|_| panic!("unread stream froze the connection, {settings}")).unwrap();
                drop((unread, s)); pool.reset(); server.abort();
            }
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn authority_headers_trailers_and_truncated_frames() {
        use tokio::io::AsyncReadExt;
        tokio::task::LocalSet::new().run_until(async {
            for (truncated, status, headers_only) in [(false, "0", false), (false, "14", false), (true, "0", false), (false, "14", true), (false, "", false)] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(); let port = listener.local_addr().unwrap().port();
                let server = tokio::spawn(async move {
                    let (io, _) = listener.accept().await.unwrap(); let mut c = h2::server::handshake(io).await.unwrap();
                    while let Some(request) = c.accept().await {
                        let (request, mut reply) = request.unwrap();
                        assert_eq!(request.uri().authority().unwrap().as_str(), "authority.test:12345");
                        assert_eq!(request.uri().path(), "/my%20service/tun%20name"); assert_eq!(request.headers()["user-agent"], "parity-agent");
                        let mut headers = http::Response::builder().header("content-type", "application/grpc"); if headers_only { headers = headers.header("grpc-status", status); }
                        let mut stream = reply.send_response(headers.body(()).unwrap(), headers_only).unwrap();
                        if !headers_only {
                            let b = encode_hunk(b"hello"); stream.send_data(if truncated { b.slice(..4) } else { b }, status.is_empty()).unwrap();
                            if !status.is_empty() { let mut trailers = http::HeaderMap::new(); trailers.insert("grpc-status", status.parse().unwrap()); stream.send_trailers(trailers).unwrap(); }
                        }
                    }
                });
                let config = serde_json::json!({"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "127.0.0.1", "port": port, "password": "loopback-test"}]}, "streamSettings": {"network": "grpc", "grpcSettings": {"serviceName": "/my service/tun name|multi", "userAgent": "parity-agent"}}}]});
                let p = rv_config::parse_subscription(&config.to_string()).unwrap().remove(0); let out = p.outbounds.iter().find(|o| o.kind.is_proxy()).unwrap();
                let rv_config::Network::Grpc(g) = &out.stream.network else { panic!() }; let dialer = Dialer::new(&p, &out.stream, "127.0.0.1", port, None).unwrap();
                let pool = Pool::new(g, dialer, "authority.test:12345".into()); let mut s = pool.open().await.unwrap(); let mut data = Vec::new();
                let result = tokio::time::timeout(Duration::from_secs(2), s.read_to_end(&mut data)).await.unwrap();
                if truncated { assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof); }
                else if status.is_empty() { assert!(result.unwrap_err().to_string().contains("missing grpc-status")); }
                else if status != "0" { assert!(result.unwrap_err().to_string().contains("grpc-status 14")); }
                else { result.unwrap(); assert_eq!(data, b"hello"); }
                drop(s); pool.reset(); server.abort();
            }
        }).await;
    }
}
