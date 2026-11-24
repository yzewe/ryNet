use super::*;
use crate::h3::{self, DATA, HEADERS, MAX_FIELD_SECTION, SETTINGS, get_varint};
use bytes::{Buf, BytesMut};
use tokio::io::AsyncWriteExt;

const NO_ERROR: u32 = 0x100;
const STREAM_CREATION_ERROR: u32 = 0x103;
const CLOSED_CRITICAL_STREAM: u32 = 0x104;
const FRAME_UNEXPECTED: u32 = 0x105;
const FRAME_ERROR: u32 = 0x106;
const SETTINGS_ERROR: u32 = 0x109;
const MISSING_SETTINGS: u32 = 0x10a;
const SETTINGS_MAX: u64 = 8 << 10;
const OPEN_WAIT: Duration = Duration::from_secs(2);
const REQUEST_REJECTED: u32 = 0x10b;
const REQUEST_CANCELLED: u32 = 0x10c;
const MESSAGE_ERROR: u32 = 0x10e;
const QPACK_ERROR: u32 = 0x200;

fn protocol(c: &quinn::Connection, code: u32, reason: &str) -> io::Error {
    c.close(code.into(), b"");
    io::Error::new(io::ErrorKind::InvalidData, format!("http3: {reason} (0x{code:x})"))
}

fn read_error(e: quinn::ReadError) -> io::Error {
    match e {
        quinn::ReadError::Reset(code) => {
            io::Error::new(io::ErrorKind::ConnectionReset, format!("http3: stream reset (0x{:x})", code.into_inner()))
        }
        quinn::ReadError::ConnectionLost(e) => connection_error(e),
        e => io::Error::other(e),
    }
}
fn write_error(e: quinn::WriteError) -> io::Error {
    match e {
        quinn::WriteError::Stopped(code) => io::Error::new(
            if code.into_inner() == REQUEST_REJECTED as u64 { io::ErrorKind::ConnectionAborted } else { io::ErrorKind::ConnectionReset },
            format!("http3: stream stopped (0x{:x})", code.into_inner()),
        ),
        quinn::WriteError::ConnectionLost(e) => connection_error(e),
        e => io::Error::other(e),
    }
}
fn connection_error(e: quinn::ConnectionError) -> io::Error {
    let kind = match e {
        quinn::ConnectionError::TimedOut => io::ErrorKind::TimedOut,
        _ => io::ErrorKind::ConnectionAborted,
    };
    io::Error::new(kind, e)
}

#[derive(Debug)]
struct Unprocessed(io::Error);
impl std::fmt::Display for Unprocessed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Unprocessed {}
fn unprocessed(e: io::Error) -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, Unprocessed(e))
}
pub(super) fn retriable(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|e| e.is::<Unprocessed>())
}
fn rejected(e: io::Error) -> io::Error {
    let code = e.get_ref().and_then(|i| match i.downcast_ref::<quinn::WriteError>() {
        Some(quinn::WriteError::Stopped(c)) => Some(c.into_inner()),
        _ => None,
    });
    if code == Some(REQUEST_REJECTED as u64) { unprocessed(e) } else { e }
}

fn exact_error(e: quinn::ReadExactError) -> io::Error {
    match e {
        quinn::ReadExactError::FinishedEarly(_) => io::ErrorKind::UnexpectedEof.into(),
        quinn::ReadExactError::ReadError(e) => read_error(e),
    }
}

async fn varint(r: &mut quinn::RecvStream) -> io::Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b[..1]).await.map_err(exact_error)?;
    let n = 1 << (b[0] >> 6);
    r.read_exact(&mut b[1..n]).await.map_err(exact_error)?;
    Ok(get_varint(&b[..n]).unwrap().0)
}
async fn frame_head(r: &mut quinn::RecvStream) -> io::Result<(u64, u64)> {
    Ok((varint(r).await?, varint(r).await?))
}
async fn payload(r: &mut quinn::RecvStream, n: u64) -> io::Result<Vec<u8>> {
    if n > MAX_FIELD_SECTION {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "http3: frame too large"));
    }
    let mut b = vec![0; n as usize];
    r.read_exact(&mut b).await.map_err(exact_error)?;
    Ok(b)
}
async fn skip(r: &mut quinn::RecvStream, mut n: u64) -> io::Result<()> {
    let mut b = [0; 8192];
    while n > 0 {
        let take = n.min(b.len() as u64) as usize;
        r.read_exact(&mut b[..take]).await.map_err(exact_error)?;
        n -= take as u64;
    }
    Ok(())
}
fn forbidden(typ: u64) -> bool {
    matches!(typ, 2..=9 | 13)
}

pub(super) struct Client {
    conn: quinn::Connection,
    _endpoint: quinn::Endpoint,
    _control: quinn::SendStream,
    goaway: Rc<Cell<Option<u64>>>,
}
impl Drop for Client {
    fn drop(&mut self) {
        self.conn.close(NO_ERROR.into(), b"");
    }
}
impl Client {
    pub async fn new(conn: quinn::Connection, endpoint: quinn::Endpoint) -> io::Result<Self> {
        let mut control = conn.open_uni().await.map_err(connection_error)?;
        control.write_all(&h3::control_stream()).await.map_err(write_error)?;
        let goaway = Rc::new(Cell::new(None));
        let c = conn.clone();
        let g = goaway.clone();
        tokio::task::spawn_local(async move {
            let seen = Rc::new(Cell::new(0u8));
            loop {
                let mut r = tokio::select! {
                    r=c.accept_uni() => match r { Ok(r)=>r,Err(_)=>return },
                    r=c.accept_bi() => { if r.is_ok() { let _=protocol(&c,FRAME_UNEXPECTED,"server bidirectional stream"); } return; },
                };
                let (c, g, seen) = (c.clone(), g.clone(), seen.clone());
                tokio::task::spawn_local(async move {
                    if let Err(e) = incoming(&c, &g, &seen, &mut r).await {
                        if c.close_reason().is_none() {
                            let _ = protocol(&c, CLOSED_CRITICAL_STREAM, "critical stream closed");
                        }
                        tracing::debug!("xhttp h3 control: {e}");
                    }
                });
            }
        });
        Ok(Self { conn, _endpoint: endpoint, _control: control, goaway })
    }
    pub fn reusable(&self) -> bool {
        self.conn.close_reason().is_none() && self.goaway.get().is_none()
    }
    pub async fn request(&self, host: &str, req: &request::Request, streaming: bool) -> io::Result<(ResponseFuture, Option<SendBody>)> {
        if !self.reusable() {
            return Err(unprocessed(io::Error::new(io::ErrorKind::ConnectionAborted, "http3: GOAWAY/closed")));
        }
        let headers = request_headers(host, req, streaming)?;
        let (mut send, recv) = tokio::time::timeout(OPEN_WAIT, self.conn.open_bi())
            .await
            .map_err(|_| unprocessed(io::Error::new(io::ErrorKind::TimedOut, "http3: no stream credit from the server")))?
            .map_err(|e| unprocessed(connection_error(e)))?;
        if self.goaway.get().is_some_and(|id| send.id().index() * 4 >= id) {
            let _ = send.reset(REQUEST_REJECTED.into());
            return Err(unprocessed(io::Error::new(io::ErrorKind::ConnectionAborted, "http3: GOAWAY")));
        }
        send.write_all(&h3::frame(HEADERS, &h3::encode(&headers))).await.map_err(|e| match e {
            quinn::WriteError::Stopped(c) if c.into_inner() == REQUEST_REJECTED as u64 => unprocessed(write_error(e)),
            e => write_error(e),
        })?;
        let mut send = SendBody { send, pending: Vec::new(), at: 0, accepted: 0, finished: false };
        let sent = req.body.as_ref().filter(|b| !b.is_empty());
        if let Some(b) = sent {
            send.send_all(b.clone()).await.map_err(rejected)?;
        }
        let body = RecvBody {
            recv,
            conn: self.conn.clone(),
            buffer: BytesMut::new(),
            remaining: 0,
            typ: None,
            trailers: false,
            ended: false,
            length: None,
            received: 0,
        };
        let response = Box::pin(body.response()) as ResponseFuture;
        if streaming || sent.is_some() {
            Ok((response, Some(send)))
        } else {
            send.shutdown().await.map_err(rejected)?;
            Ok((response, None))
        }
    }
}

async fn incoming(c: &quinn::Connection, goaway: &Cell<Option<u64>>, seen: &Cell<u8>, r: &mut quinn::RecvStream) -> io::Result<()> {
    let typ = varint(r).await?;
    if !matches!(typ, 0 | 2 | 3) {
        if typ == 1 {
            return Err(protocol(c, STREAM_CREATION_ERROR, "unsolicited push"));
        }
        let _ = r.stop(STREAM_CREATION_ERROR.into());
        return Ok(());
    }
    let bit = 1 << typ;
    if seen.get() & bit != 0 {
        return Err(protocol(c, STREAM_CREATION_ERROR, "duplicate critical stream"));
    }
    seen.set(seen.get() | bit);
    if typ != 0 {
        loop {
            let mut b = [0];
            r.read_exact(&mut b).await.map_err(exact_error)?;
            if typ != 2 || b[0] != 0x20 {
                return Err(protocol(c, if typ == 2 { 0x201 } else { 0x202 }, "unexpected QPACK instruction"));
            }
        }
    }
    let (frame, n) = frame_head(r).await?;
    if frame != SETTINGS {
        return Err(protocol(c, MISSING_SETTINGS, "SETTINGS must be first"));
    }
    if n > SETTINGS_MAX {
        return Err(protocol(c, SETTINGS_ERROR, "SETTINGS too large"));
    }
    let settings = payload(r, n).await.map_err(|_| protocol(c, SETTINGS_ERROR, "bad SETTINGS"))?;
    let mut i = 0;
    let mut keys = Vec::new();
    while i < settings.len() {
        let (key, len) = get_varint(&settings[i..]).ok_or_else(|| protocol(c, SETTINGS_ERROR, "truncated SETTINGS"))?;
        i += len;
        let (value, len) = get_varint(&settings[i..]).ok_or_else(|| protocol(c, SETTINGS_ERROR, "truncated SETTINGS"))?;
        i += len;
        if keys.contains(&key) || matches!(key, 2..=5) || (matches!(key, 8 | 0x33) && value > 1) {
            return Err(protocol(c, SETTINGS_ERROR, "invalid SETTINGS"));
        }
        keys.push(key);
    }
    loop {
        let (typ, n) = frame_head(r).await?;
        if typ == 7 {
            let b = payload(r, n).await?;
            let (id, len) = get_varint(&b).ok_or_else(|| protocol(c, FRAME_ERROR, "bad GOAWAY"))?;
            if len != b.len() || id % 4 != 0 || goaway.get().is_some_and(|old| id > old) {
                return Err(protocol(c, 0x108, "invalid GOAWAY stream id"));
            }
            goaway.set(Some(id));
        } else if matches!(typ, DATA | HEADERS | SETTINGS | 2 | 3 | 6 | 8 | 9 | 13) {
            return Err(protocol(c, FRAME_UNEXPECTED, "frame on control stream"));
        } else {
            skip(r, n).await?;
        }
    }
}

fn request_headers(host: &str, req: &request::Request, streaming: bool) -> io::Result<Vec<(String, String)>> {
    let _: http::Uri = format!("https://{host}{}", req.path).parse().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let mut fields = vec![
        (":authority".into(), host.into()),
        (":method".into(), req.method.clone()),
        (":path".into(), req.path.clone()),
        (":scheme".into(), "https".into()),
    ];
    let mut random = [0; 4];
    use ring::rand::SecureRandom;
    let _ = ring::rand::SystemRandom::new().fill(&mut random);
    let order = if req.headers.len() <= 8 {
        h3::small_map_order(req.headers.len(), u32::from_le_bytes(random))
    } else {
        shuffled(req.headers.len())
    };
    for i in order {
        let (n, v) = &req.headers[i];
        let n = n.to_ascii_lowercase();
        if matches!(
            n.as_str(),
            "host" | "content-length" | "connection" | "proxy-connection" | "transfer-encoding" | "upgrade" | "keep-alive"
        ) {
            continue;
        }
        http::header::HeaderName::from_bytes(n.as_bytes()).map_err(io::Error::other)?;
        http::header::HeaderValue::from_str(v).map_err(io::Error::other)?;
        if n == "user-agent" && v.is_empty() {
            continue;
        }
        fields.push((n, v.clone()));
    }
    let len = req.body.as_ref().map_or(0, Bytes::len);
    if !streaming && (len > 0 || matches!(req.method.as_str(), "POST" | "PUT" | "PATCH")) {
        fields.push(("content-length".into(), len.to_string()));
    }
    if auto_gzip(req) {
        fields.push(("accept-encoding".into(), "gzip".into()));
    }
    if crate::browser::get(&req.headers, "User-Agent").is_none() {
        fields.push(("user-agent".into(), "quic-go HTTP/3".into()));
    }
    Ok(fields)
}

pub(super) struct SendBody {
    send: quinn::SendStream,
    pending: Vec<u8>,
    at: usize,
    accepted: usize,
    finished: bool,
}
impl SendBody {
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.at < self.pending.len() {
            let n = ready!(Pin::new(&mut self.send).poll_write(cx, &self.pending[self.at..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.at += n;
        }
        self.pending.clear();
        self.at = 0;
        Poll::Ready(Ok(()))
    }
}
impl SendBody {
    pub(super) async fn send_all(&mut self, body: Bytes) -> io::Result<()> {
        let mut head = Vec::with_capacity(16);
        h3::put_varint(&mut head, DATA);
        h3::put_varint(&mut head, body.len() as u64);
        let mut chunks = [Bytes::from(head), body];
        let mut done = 0;
        while done < chunks.len() {
            let written = tokio::time::timeout(UPLOAD_TIMEOUT, self.send.write_chunks(&mut chunks[done..]))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "http3: body not sent in time"))?
                .map_err(io::Error::from)?;
            done += written.chunks;
        }
        self.send.finish().map_err(io::Error::other)?;
        self.finished = true;
        Ok(())
    }
    pub(super) fn acked(&self) -> impl Future<Output = ()> + 'static {
        let stopped = self.send.stopped();
        async move {
            let _ = stopped.await;
        }
    }
}
impl Drop for SendBody {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.send.reset(REQUEST_CANCELLED.into());
        }
    }
}
impl AsyncWrite for SendBody {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        if self.finished {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if self.pending.is_empty() {
            if b.is_empty() {
                return Poll::Ready(Ok(0));
            }
            self.accepted = b.len().min(8192);
            self.pending = h3::frame(DATA, &b[..self.accepted]);
        }
        ready!(self.drain(cx))?;
        Poll::Ready(Ok(self.accepted))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.drain(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.drain(cx))?;
        if !self.finished {
            self.send.finish().map_err(io::Error::other)?;
            self.finished = true;
        }
        Poll::Ready(Ok(()))
    }
}

struct RecvBody {
    recv: quinn::RecvStream,
    conn: quinn::Connection,
    buffer: BytesMut,
    remaining: u64,
    typ: Option<u64>,
    trailers: bool,
    ended: bool,
    length: Option<u64>,
    received: u64,
}
impl Drop for RecvBody {
    fn drop(&mut self) {
        if !self.ended {
            let _ = self.recv.stop(REQUEST_CANCELLED.into());
        }
    }
}
impl RecvBody {
    async fn response(mut self) -> io::Result<http::Response<ReadBody>> {
        loop {
            let (typ, n) = frame_head(&mut self.recv).await?;
            if typ != HEADERS {
                if typ == DATA || forbidden(typ) {
                    return Err(protocol(&self.conn, FRAME_UNEXPECTED, "expected response HEADERS"));
                }
                skip(&mut self.recv, n).await?;
                continue;
            }
            let block = payload(&mut self.recv, n).await?;
            let headers = h3::decode(&block).ok_or_else(|| protocol(&self.conn, QPACK_ERROR, "bad response QPACK"))?;
            let mut response = http::Response::builder().version(http::Version::HTTP_3);
            let mut status = None;
            let mut regular = false;
            for (n, v) in headers {
                if n.starts_with(':') {
                    if n != ":status" || regular || status.is_some() {
                        return Err(self.message("invalid response pseudo-header"));
                    }
                    status = Some(v.parse::<u16>().map_err(|_| self.message("invalid status"))?);
                } else {
                    regular = true;
                    self.validate(&n, &v)?;
                    if n == "content-length" {
                        let len = v.parse::<u64>().map_err(|_| self.message("invalid content-length"))?;
                        if self.length.is_some_and(|old| old != len) {
                            return Err(self.message("conflicting content-length"));
                        }
                        self.length = Some(len);
                    }
                    response = response.header(n, v);
                }
            }
            let status = status.ok_or_else(|| self.message("missing status"))?;
            if !(100..=999).contains(&status) {
                return Err(self.message("invalid status"));
            }
            if (100..200).contains(&status) {
                if status == 101 {
                    return Err(self.message("HTTP/3 status 101"));
                }
                self.length = None;
                continue;
            }
            return response.status(status).body(Box::new(self) as ReadBody).map_err(io::Error::other);
        }
    }
    fn message(&mut self, why: &str) -> io::Error {
        let _ = self.recv.stop(MESSAGE_ERROR.into());
        io::Error::new(io::ErrorKind::InvalidData, format!("http3: {why}"))
    }
    fn validate(&mut self, n: &str, v: &str) -> io::Result<()> {
        if n != n.to_ascii_lowercase()
            || matches!(n, "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade")
            || (n == "te" && v != "trailers")
            || http::header::HeaderName::from_bytes(n.as_bytes()).is_err()
            || http::header::HeaderValue::from_str(v).is_err()
        {
            return Err(self.message("invalid response header"));
        }
        Ok(())
    }
    fn fill(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<bool>> {
        let mut b = [0; 8192];
        match ready!(self.recv.poll_read(cx, &mut b)) {
            Ok(0) => Poll::Ready(Ok(false)),
            Ok(n) => {
                self.buffer.extend_from_slice(&b[..n]);
                Poll::Ready(Ok(true))
            }
            Err(e) => Poll::Ready(Err(read_error(e))),
        }
    }
}
impl AsyncRead for RecvBody {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if out.remaining() == 0 || self.ended {
            return Poll::Ready(Ok(()));
        }
        loop {
            if self.typ.is_none() {
                let head = get_varint(&self.buffer).and_then(|(t, k)| get_varint(&self.buffer[k..]).map(|(n, l)| (t, n, k + l)));
                if let Some((typ, n, k)) = head {
                    if forbidden(typ) || (typ == DATA && self.trailers) || (typ == HEADERS && self.trailers) {
                        return Poll::Ready(Err(protocol(&self.conn, FRAME_UNEXPECTED, "invalid body frame")));
                    }
                    if typ == HEADERS && n > MAX_FIELD_SECTION {
                        return Poll::Ready(Err(self.message("trailers too large")));
                    }
                    self.buffer.advance(k);
                    self.typ = Some(typ);
                    self.remaining = n;
                } else if !ready!(self.fill(cx))? {
                    if !self.buffer.is_empty() {
                        return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                    }
                    if self.length.is_some_and(|l| l != self.received) {
                        return Poll::Ready(Err(self.message("truncated content-length")));
                    }
                    self.ended = true;
                    return Poll::Ready(Ok(()));
                } else {
                    continue;
                }
            }
            if self.typ == Some(HEADERS) {
                if self.buffer.len() < self.remaining as usize {
                    if !ready!(self.fill(cx))? {
                        return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                    }
                    continue;
                }
                let n = self.remaining as usize;
                let block = self.buffer.split_to(n);
                let headers = h3::decode(&block).ok_or_else(|| protocol(&self.conn, QPACK_ERROR, "bad trailer QPACK"))?;
                for (n, v) in headers {
                    if n.starts_with(':') {
                        return Poll::Ready(Err(self.message("pseudo-header in trailers")));
                    }
                    self.validate(&n, &v)?;
                }
                self.trailers = true;
                self.remaining = 0;
            }
            if self.remaining == 0 {
                self.typ = None;
                continue;
            }
            if self.buffer.is_empty() && !ready!(self.fill(cx))? {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
            let n = self.remaining.min(self.buffer.len() as u64).min(if self.typ == Some(DATA) { out.remaining() as u64 } else { u64::MAX })
                as usize;
            let b = self.buffer.split_to(n);
            self.remaining -= n as u64;
            if self.typ == Some(DATA) {
                self.received += n as u64;
                if self.length.is_some_and(|l| self.received > l) {
                    return Poll::Ready(Err(self.message("exceeded content-length")));
                }
                out.put_slice(&b);
                return Poll::Ready(Ok(()));
            }
        }
    }
}
