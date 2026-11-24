use super::*;
use bytes::{Buf, BytesMut};
use std::collections::VecDeque;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::time::Instant as Deadline;

struct Client {
    running: Cell<usize>,
    reuses: Cell<i64>,
    requests: Cell<i64>,
    until: Option<tokio::time::Instant>,
}
impl Client {
    fn reusable(&self) -> bool {
        self.requests.get() != 0 && self.until.is_none_or(|t| tokio::time::Instant::now() < t)
    }
}
struct Usage(Rc<Client>);
impl Drop for Usage {
    fn drop(&mut self) {
        self.0.running.set(self.0.running.get().saturating_sub(1));
    }
}
#[derive(Clone)]
struct Lease(Rc<Usage>);
impl Lease {
    fn client(&self) -> &Client {
        &self.0.0
    }
}

pub(super) struct Pool {
    cfg: request::Xmux,
    concurrency: i64,
    connections: i64,
    clients: RefCell<Vec<Rc<Client>>>,
}
impl Pool {
    pub(super) fn new(cfg: request::Xmux) -> Self {
        Self {
            cfg,
            concurrency: rand_range(cfg.max_concurrency),
            connections: rand_range(cfg.max_connections),
            clients: RefCell::new(Vec::new()),
        }
    }
    pub(super) fn reset(&self) {
        self.clients.borrow_mut().clear();
    }
    fn lease(&self) -> Lease {
        let mut clients = self.clients.borrow_mut();
        clients.retain(|c| c.reusable() && c.reuses.get() != 0);
        let wide = |n: i64| usize::try_from(n).unwrap_or(usize::MAX);
        let candidates: Vec<_> = clients.iter().filter(|c| self.concurrency <= 0 || c.running.get() < wide(self.concurrency)).collect();
        let c = if !candidates.is_empty() && !(self.connections > 0 && clients.len() < wide(self.connections)) {
            candidates[rand_range(request::Range { from: 0, to: candidates.len() as i64 - 1 }) as usize].clone()
        } else {
            let limit = |r| {
                let n = rand_range(r);
                if n > 0 { n } else { -1 }
            };
            let secs = rand_range(self.cfg.h_max_reusable_secs);
            let c = Rc::new(Client {
                running: Cell::new(0),
                reuses: Cell::new(limit(self.cfg.c_max_reuse_times)),
                requests: Cell::new(limit(self.cfg.h_max_request_times)),
                until: (secs > 0).then(|| tokio::time::Instant::now() + Duration::from_secs(secs as u64)),
            });
            clients.push(c.clone());
            c
        };
        spend(&c.reuses);
        c.running.set(c.running.get() + 1);
        Lease(Rc::new(Usage(c)))
    }
}

pub(super) struct Task(Option<tokio::task::JoinHandle<()>>);
impl Task {
    pub(super) fn new(h: tokio::task::JoinHandle<()>) -> Self {
        Self(Some(h))
    }
    pub(super) fn detach(&mut self) {
        self.0 = None;
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            h.abort();
        }
    }
}

#[derive(Default)]
struct Bufio {
    buf: Vec<u8>,
    out: VecDeque<Vec<u8>>,
}
const BUFIO: usize = 4096;
impl Bufio {
    fn write(&mut self, mut p: &[u8]) {
        while p.len() > BUFIO - self.buf.len() {
            if self.buf.is_empty() {
                self.out.push_back(p.to_vec());
                return;
            }
            let n = BUFIO - self.buf.len();
            self.buf.extend_from_slice(&p[..n]);
            p = &p[n..];
            self.out.push_back(std::mem::take(&mut self.buf));
        }
        self.buf.extend_from_slice(p);
    }
    fn flush(&mut self) {
        if !self.buf.is_empty() {
            self.out.push_back(std::mem::take(&mut self.buf));
        }
    }
}

#[derive(Clone, Copy)]
enum Framing {
    Chunked,
    Length(usize),
}

fn fixed_framing(req: &request::Request) -> Option<Framing> {
    let n = req.body.as_ref().map_or(0, Bytes::len);
    (n > 0 || ["POST", "PUT", "PATCH"].contains(&req.method.as_str())).then_some(Framing::Length(n))
}

fn head(t: &Transport, req: &request::Request, framing: Option<Framing>, close: bool, transport: bool) -> Vec<Vec<u8>> {
    let mut o = vec![format!("{} {} HTTP/1.1\r\n", req.method, req.path).into_bytes(), format!("Host: {}\r\n", t.host).into_bytes()];
    let ua = crate::browser::get(&req.headers, "user-agent").unwrap_or("Go-http-client/1.1");
    let ua = if t.cfg.golang_user_agent() && ua == "Go-http-client/2.0" { "Go-http-client/1.1" } else { ua };
    o.push(format!("User-Agent: {ua}\r\n").into_bytes());
    match framing {
        Some(Framing::Chunked) => o.push(b"Transfer-Encoding: chunked\r\n".to_vec()),
        Some(Framing::Length(n)) => {
            o.push(b"Content-Length: ".to_vec());
            o.push(format!("{n}\r\n").into_bytes());
        }
        None => {}
    }
    let field = |o: &mut Vec<Vec<u8>>, k: &str, v: &str| {
        for s in [k, ": ", v, "\r\n"] {
            o.push(s.as_bytes().to_vec());
        }
    };
    for (k, v) in &req.headers {
        if !["host", "user-agent", "content-length", "transfer-encoding"].iter().any(|n| k.eq_ignore_ascii_case(n))
            && !(close && k.eq_ignore_ascii_case("connection"))
        {
            field(&mut o, k, v);
        }
    }
    if transport && auto_gzip(req) {
        field(&mut o, "Accept-Encoding", "gzip");
    }
    if close {
        field(&mut o, "Connection", "close");
    }
    o.push(b"\r\n".to_vec());
    o
}

struct Writer {
    w: WriteHalf<Box<dyn Io>>,
    pending: VecDeque<Vec<u8>>,
    at: usize,
    flushing: bool,
}
impl Writer {
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            if self.flushing {
                ready!(Pin::new(&mut self.w).poll_flush(cx))?;
                self.flushing = false;
            }
            let Some(front) = self.pending.front() else { return Poll::Ready(Ok(())) };
            while self.at < front.len() {
                let n = ready!(Pin::new(&mut self.w).poll_write(cx, &front[self.at..]))?;
                if n == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                self.at += n;
            }
            self.pending.pop_front();
            self.at = 0;
            self.flushing = true;
        }
    }
    async fn send(&mut self, writes: impl IntoIterator<Item = Vec<u8>>) -> io::Result<()> {
        self.pending.extend(writes);
        std::future::poll_fn(|cx| self.poll_drain(cx)).await
    }
}

struct Reader {
    r: ReadHalf<Box<dyn Io>>,
    buf: BytesMut,
}
struct Head {
    status: u16,
    close: bool,
    framing: BodyFraming,
    gzip: bool,
}
#[derive(Clone, Copy)]
enum BodyFraming {
    Chunked(Chunk),
    Length(u64),
    Eof,
}
#[derive(Clone, Copy)]
enum Chunk {
    Size,
    Data(u64),
    Crlf,
    Trailers,
    Done,
}

async fn connect(t: &Transport) -> io::Result<(Reader, Writer)> {
    let (r, w) = tokio::io::split((t.connect)().await?);
    Ok((Reader { r, buf: BytesMut::new() }, Writer { w, pending: VecDeque::new(), at: 0, flushing: false }))
}

impl Reader {
    fn poll_fill(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<bool>> {
        let mut tmp = [0u8; 16384];
        let mut rb = ReadBuf::new(&mut tmp);
        ready!(Pin::new(&mut self.r).poll_read(cx, &mut rb))?;
        self.buf.extend_from_slice(rb.filled());
        Poll::Ready(Ok(!rb.filled().is_empty()))
    }
    async fn head(&mut self) -> io::Result<Head> {
        loop {
            let parsed = {
                let mut fields = [httparse::EMPTY_HEADER; 64];
                let mut resp = httparse::Response::new(&mut fields);
                match resp.parse(&self.buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("xhttp h1: {e}")))? {
                    httparse::Status::Complete(n) => {
                        let has = |name: &str, value: &str| {
                            resp.headers.iter().any(|h| {
                                h.name.eq_ignore_ascii_case(name)
                                    && String::from_utf8_lossy(h.value).split(',').any(|v| v.trim().eq_ignore_ascii_case(value))
                            })
                        };
                        let status = resp.code.unwrap_or(0);
                        let length = resp.headers.iter().find(|h| h.name.eq_ignore_ascii_case("content-length")).map(|h| {
                            std::str::from_utf8(h.value)
                                .ok()
                                .and_then(|v| v.trim().parse::<u64>().ok())
                                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "xhttp h1: bad Content-Length"))
                        });
                        let framing = if has("transfer-encoding", "chunked") {
                            BodyFraming::Chunked(Chunk::Size)
                        } else if let Some(n) = length {
                            BodyFraming::Length(n?)
                        } else if status == 204 || status == 304 {
                            BodyFraming::Length(0)
                        } else {
                            BodyFraming::Eof
                        };
                        Some((n, Head { status, close: has("connection", "close"), framing, gzip: has("content-encoding", "gzip") }))
                    }
                    httparse::Status::Partial => None,
                }
            };
            match parsed {
                Some((n, head)) => {
                    self.buf.advance(n);
                    if !(100..200).contains(&head.status) || head.status == 101 {
                        return Ok(head);
                    }
                }
                None => {
                    if self.buf.len() > 65536 {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "xhttp h1: response head too large"));
                    }
                    if !std::future::poll_fn(|cx| self.poll_fill(cx)).await? {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                }
            }
        }
    }
}

struct Body {
    r: Reader,
    framing: BodyFraming,
}
impl Body {
    fn find_line(&self) -> Option<usize> {
        self.r.buf.windows(2).position(|w| w == b"\r\n")
    }
}
impl AsyncRead for Body {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        macro_rules! more {
            () => {
                if !ready!(me.r.poll_fill(cx))? {
                    return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                }
            };
        }
        let bad = |why: &str| Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, format!("xhttp h1: {why}"))));
        loop {
            match me.framing {
                BodyFraming::Length(0) | BodyFraming::Chunked(Chunk::Done) => return Poll::Ready(Ok(())),
                BodyFraming::Eof => {
                    if me.r.buf.is_empty() && !ready!(me.r.poll_fill(cx))? {
                        return Poll::Ready(Ok(()));
                    }
                    let n = me.r.buf.len().min(buf.remaining());
                    buf.put_slice(&me.r.buf[..n]);
                    me.r.buf.advance(n);
                    return Poll::Ready(Ok(()));
                }
                BodyFraming::Length(left) | BodyFraming::Chunked(Chunk::Data(left)) => {
                    if me.r.buf.is_empty() {
                        more!();
                    }
                    let n = (me.r.buf.len() as u64).min(left).min(buf.remaining() as u64) as usize;
                    buf.put_slice(&me.r.buf[..n]);
                    me.r.buf.advance(n);
                    let left = left - n as u64;
                    me.framing = match me.framing {
                        BodyFraming::Length(_) => BodyFraming::Length(left),
                        _ => BodyFraming::Chunked(if left == 0 { Chunk::Crlf } else { Chunk::Data(left) }),
                    };
                    return Poll::Ready(Ok(()));
                }
                BodyFraming::Chunked(Chunk::Size) => match me.find_line() {
                    Some(i) => {
                        let line = std::str::from_utf8(&me.r.buf[..i]).unwrap_or("");
                        let Ok(n) = u64::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16) else {
                            return bad("bad chunk size");
                        };
                        me.r.buf.advance(i + 2);
                        me.framing = BodyFraming::Chunked(if n == 0 { Chunk::Trailers } else { Chunk::Data(n) });
                    }
                    None if me.r.buf.len() > 4096 => return bad("chunk size line too long"),
                    None => more!(),
                },
                BodyFraming::Chunked(Chunk::Crlf) => {
                    if me.r.buf.len() < 2 {
                        more!();
                        continue;
                    }
                    if &me.r.buf[..2] != b"\r\n" {
                        return bad("missing chunk CRLF");
                    }
                    me.r.buf.advance(2);
                    me.framing = BodyFraming::Chunked(Chunk::Size);
                }
                BodyFraming::Chunked(Chunk::Trailers) => match me.find_line() {
                    Some(i) => {
                        me.r.buf.advance(i + 2);
                        if i == 0 {
                            me.framing = BodyFraming::Chunked(Chunk::Done);
                        }
                    }
                    None if me.r.buf.len() > 65536 => return bad("trailers too large"),
                    None => more!(),
                },
            }
        }
    }
}

enum ResponseBody {
    Plain(Body),
    Gzip(Box<async_compression::tokio::bufread::GzipDecoder<tokio::io::BufReader<Body>>>),
}
impl AsyncRead for ResponseBody {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(b) => Pin::new(b).poll_read(cx, buf),
            Self::Gzip(b) => Pin::new(b.as_mut()).poll_read(cx, buf),
        }
    }
}
struct Stream {
    response: Option<LocalBoxFuture<io::Result<(Head, Reader)>>>,
    body: Option<ResponseBody>,
    gzip: bool,
    writer: Writer,
    upload: bool,
    write_closed: bool,
    _lease: Lease,
}
impl AsyncRead for Stream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if let Some(resp) = &mut self.response {
            let result = ready!(resp.as_mut().poll(cx));
            self.response = None;
            let (head, r) = result?;
            if head.status != 200 {
                return Poll::Ready(Err(io::Error::other(format!("xhttp: status {}", head.status))));
            }
            let b = Body { r, framing: head.framing };
            self.body = Some(if self.gzip && head.gzip {
                ResponseBody::Gzip(Box::new(async_compression::tokio::bufread::GzipDecoder::new(tokio::io::BufReader::new(b))))
            } else {
                ResponseBody::Plain(b)
            });
        }
        match &mut self.body {
            Some(b) => Pin::new(b).poll_read(cx, buf),
            None => Poll::Ready(Ok(())),
        }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if self.write_closed || !self.upload {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        ready!(self.writer.poll_drain(cx))?;
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = buf.len().min(crate::activity::XRAY_BUFFER);
        let mut b = Bufio::default();
        b.write(format!("{n:x}\r\n").as_bytes());
        b.write(&buf[..n]);
        b.write(b"\r\n");
        b.flush();
        self.writer.pending.extend(b.out);
        if let Poll::Ready(Err(e)) = self.writer.poll_drain(cx) {
            return Poll::Ready(Err(e));
        }
        Poll::Ready(Ok(n))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.upload {
            return Poll::Ready(Ok(()));
        }
        self.writer.poll_drain(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush(cx))?;
        self.write_closed = true;
        Poll::Ready(Ok(()))
    }
}

async fn stream(t: &Rc<Transport>, sid: Option<&str>, upload: bool, lease: Option<Lease>) -> io::Result<Box<dyn Io>> {
    let req = t.cfg.stream_request(sid, upload);
    if t.version != HttpVersion::Http1 {
        let lease = t.conn().await?;
        let (lease, resp, send) = t.request_fresh(lease, &req, upload).await?;
        return Ok(Box::new(HttpStream::new(resp, send, lease, auto_gzip(&req))));
    }
    let lease = lease.unwrap_or_else(|| t.h1.lease());
    let (mut reader, mut writer) = connect(t).await?;
    let mut b = Bufio::default();
    for p in head(t, &req, if upload { Some(Framing::Chunked) } else { fixed_framing(&req) }, true, true) {
        b.write(&p);
    }
    if !upload && let Some(body) = &req.body {
        b.write(body);
    }
    b.flush();
    writer.send(b.out).await?;
    spend(&lease.client().requests);
    let response = Box::pin(async move {
        let head = reader.head().await?;
        Ok((head, reader))
    });
    Ok(Box::new(Stream { response: Some(response), body: None, gzip: auto_gzip(&req), writer, upload, write_closed: false, _lease: lease }))
}

struct Split {
    down: Box<dyn Io>,
    up: tokio::io::WriteHalf<Box<dyn Io>>,
    _reader: Task,
}
impl AsyncRead for Split {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.down).poll_read(cx, buf)
    }
}
impl AsyncWrite for Split {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.up).poll_write(cx, b)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.up).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.up).poll_shutdown(cx)
    }
}

pub(super) async fn open(t: &Rc<Transport>) -> io::Result<Box<dyn Io>> {
    let lease = (t.version == HttpVersion::Http1).then(|| t.h1.lease());
    if t.mode == Mode::StreamOne {
        return stream(t, None, true, lease).await;
    }
    let sid = t.cfg.generate_session_id();
    let d = t.download.as_ref().unwrap_or(t);
    let down = stream(d, Some(&sid), false, if Rc::ptr_eq(t, d) { lease.clone() } else { None }).await?;
    if t.mode == Mode::StreamUp {
        let up = stream(t, Some(&sid), true, lease).await?;
        let (mut r, w) = tokio::io::split(up);
        let reader = tokio::task::spawn_local(async move {
            let _ = tokio::io::copy(&mut r, &mut tokio::io::sink()).await;
        });
        return Ok(Box::new(Split { down, up: w, _reader: Task::new(reader) }));
    }
    let (tx, rx) = mpsc::channel(1);
    let task = if let Some(lease) = lease {
        tokio::task::spawn_local(uploader(t.clone(), lease, sid, rx))
    } else {
        let up = t.conn().await?;
        tokio::task::spawn_local(super::uploader(t.clone(), up, sid, rx))
    };
    let monitor = tx.clone();
    Ok(Box::new(PacketUp {
        down,
        up: tx,
        pending: Vec::new(),
        max: rand_range(t.cfg.max_each_post_bytes()).max(1) as usize,
        sending: None,
        reserving: None,
        _uploader: Some(Task::new(task)),
        upload_closed: Some(Box::pin(async move {
            monitor.closed().await;
        })),
    }))
}

struct Pipe {
    writer: Writer,
    state: Rc<PipeState>,
    used: bool,
    _reader: Task,
}
#[derive(Default)]
struct PipeState {
    waiting: RefCell<VecDeque<Waiting>>,
    dead: Cell<bool>,
    failed: RefCell<Option<io::Error>>,
    progress: tokio::sync::Notify,
}
struct Waiting {
    req: Rc<Vec<u8>>,
    at: Deadline,
    resent: bool,
}

impl Pipe {
    async fn new(t: &Transport) -> io::Result<Self> {
        let (reader, writer) = connect(t).await?;
        let state = Rc::new(PipeState::default());
        let task = tokio::task::spawn_local(answers(reader, state.clone()));
        Ok(Self { writer, state, used: false, _reader: Task::new(task) })
    }
    async fn write(&mut self, req: Rc<Vec<u8>>, resent: bool) -> io::Result<()> {
        tokio::time::timeout(UPLOAD_TIMEOUT, self.writer.send([req.to_vec()]))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "xhttp upload: request not sent in time"))??;
        self.used = true;
        self.state.waiting.borrow_mut().push_back(Waiting { req, at: Deadline::now(), resent });
        Ok(())
    }
    fn oldest(&self) -> Option<Deadline> {
        self.state.waiting.borrow().front().map(|w| w.at + UPLOAD_TIMEOUT)
    }
}

async fn answers(mut r: Reader, s: Rc<PipeState>) {
    let result = async {
        loop {
            let head = match r.head().await {
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof && s.waiting.borrow().is_empty() => return Ok(()),
                h => h?,
            };
            if head.status != 200 {
                return Err(io::Error::other(format!("xhttp upload: status {}", head.status)));
            }
            let mut body = Body { r, framing: head.framing };
            tokio::io::copy(&mut body, &mut tokio::io::sink()).await.map_err(|e| io::Error::new(io::ErrorKind::UnexpectedEof, e))?;
            r = body.r;
            s.waiting.borrow_mut().pop_front();
            s.progress.notify_one();
            if head.close || matches!(head.framing, BodyFraming::Eof) {
                return Ok(());
            }
        }
    }
    .await;
    match result {
        Err(e) if e.kind() != io::ErrorKind::UnexpectedEof => *s.failed.borrow_mut() = Some(e),
        Err(e) => tracing::debug!("xhttp h1 upload connection: {e}"),
        Ok(()) => {}
    }
    s.dead.set(true);
    s.progress.notify_one();
}

async fn check(t: &Transport, pipe: &mut Option<Pipe>) -> io::Result<()> {
    let Some(p) = pipe else { return Ok(()) };
    if let Some(e) = p.state.failed.borrow_mut().take() {
        return Err(e);
    }
    if p.oldest().is_some_and(|d| d <= Deadline::now()) {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "xhttp upload: no response"));
    }
    if !p.state.dead.get() {
        return Ok(());
    }
    let again: Vec<_> = p.state.waiting.take().into();
    *pipe = None;
    if again.is_empty() {
        return Ok(());
    }
    if again.iter().any(|w| w.resent) {
        return Err(io::Error::other("xhttp upload: connection lost again before the response"));
    }
    let mut fresh = Pipe::new(t).await?;
    for w in again {
        fresh.write(w.req, true).await?;
    }
    *pipe = Some(fresh);
    Ok(())
}

async fn progress(pipe: &Option<Pipe>) {
    match pipe {
        Some(p) => match p.oldest() {
            Some(d) => {
                let _ = tokio::time::timeout_at(d, p.state.progress.notified()).await;
            }
            None => p.state.progress.notified().await,
        },
        None => std::future::pending().await,
    }
}

async fn send(t: &Transport, pipe: &mut Option<Pipe>, req: Rc<Vec<u8>>) -> io::Result<()> {
    loop {
        check(t, pipe).await?;
        if pipe.is_none() {
            *pipe = Some(Pipe::new(t).await?);
        }
        let p = pipe.as_mut().unwrap();
        if p.state.waiting.borrow().len() >= UPLOAD_INFLIGHT {
            progress(pipe).await;
            continue;
        }
        match p.write(req.clone(), false).await {
            Ok(()) => return Ok(()),
            Err(e) if p.used && e.kind() != io::ErrorKind::TimedOut => {
                tracing::debug!(target: "rv::pool", "xhttp h1 upload ({e}), retry on a fresh connection");
                p.state.dead.set(true);
            }
            Err(e) => return Err(e),
        }
    }
}

async fn settle(t: &Transport, pipe: &mut Option<Pipe>) -> io::Result<()> {
    loop {
        check(t, pipe).await?;
        match pipe {
            Some(p) if !p.state.waiting.borrow().is_empty() => progress(pipe).await,
            _ => return Ok(()),
        }
    }
}

async fn uploader(t: Rc<Transport>, mut lease: Lease, sid: String, mut rx: mpsc::Receiver<Vec<u8>>) {
    let mut pipe = None;
    let result = async {
        let mut seq = 0;
        let mut last: Option<Instant> = None;
        loop {
            let chunk = tokio::select! {
                c = rx.recv() => match c { Some(c) => c, None => break },
                _ = progress(&pipe) => { check(&t, &mut pipe).await?; continue; }
            };
            let min = rand_range(t.cfg.min_posts_interval_ms());
            if min > 0
                && let Some(last) = last
            {
                let delay = Duration::from_millis(min as u64).saturating_sub(last.elapsed());
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
            }
            if !lease.client().reusable() {
                settle(&t, &mut pipe).await?;
                pipe = None;
                lease = t.h1.lease();
            }
            let req = t.cfg.packet_request(&sid, seq, chunk);
            seq += 1;
            let mut wire: Vec<u8> = head(&t, &req, fixed_framing(&req), false, false).concat();
            if let Some(b) = &req.body {
                wire.extend_from_slice(b);
            }
            spend(&lease.client().requests);
            last = Some(Instant::now());
            send(&t, &mut pipe, Rc::new(wire)).await?;
        }
        settle(&t, &mut pipe).await
    }
    .await;
    if let Err(e) = result {
        tracing::debug!("xhttp h1 upload: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, DuplexStream};

    async fn head(io: &mut tokio::io::BufReader<DuplexStream>) -> String {
        let mut head = String::new();
        loop {
            let mut line = String::new();
            io.read_line(&mut line).await.unwrap();
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        head
    }
    async fn request(io: &mut tokio::io::BufReader<DuplexStream>) -> (String, Vec<u8>) {
        request_n(io, 0).await
    }
    async fn request_n(io: &mut tokio::io::BufReader<DuplexStream>, limit: usize) -> (String, Vec<u8>) {
        let head = head(io).await;
        let mut body = Vec::new();
        if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
            loop {
                let mut line = String::new();
                io.read_line(&mut line).await.unwrap();
                let n = usize::from_str_radix(line.trim(), 16).unwrap();
                if n == 0 {
                    let mut crlf = [0; 2];
                    io.read_exact(&mut crlf).await.unwrap();
                    break;
                }
                let mut chunk = vec![0; n];
                io.read_exact(&mut chunk).await.unwrap();
                body.extend_from_slice(&chunk);
                let mut crlf = [0; 2];
                io.read_exact(&mut crlf).await.unwrap();
                assert_eq!(&crlf, b"\r\n");
                if limit > 0 && body.len() >= limit {
                    break;
                }
            }
        } else if let Some(n) =
            head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse::<usize>().unwrap()))
        {
            body.resize(n, 0);
            io.read_exact(&mut body).await.unwrap();
        }
        (head, body)
    }

    fn transport(cfg: Config, connect: Connector) -> Rc<Transport> {
        Transport::new_with_download_version(cfg, false, "test.local", "127.0.0.1", connect, None, HttpVersion::Http1)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn chunked_stream_half_close_and_gzip_response_survive_fragmented_io() {
        use std::io::Write;
        tokio::task::LocalSet::new()
            .run_until(async {
                for (gzip, size) in [(false, 7), (true, 131072)] {
                    let (client, peer) = tokio::io::duplex(127);
                    let slot = Rc::new(RefCell::new(Some(client)));
                    let connect: Connector = Rc::new(move || {
                        let io = slot.borrow_mut().take().unwrap();
                        Box::pin(async { Ok(Box::new(io) as Box<dyn Io>) })
                    });
                    let server = tokio::task::spawn_local(async move {
                        let mut peer = tokio::io::BufReader::new(peer);
                        let (head, body) = request_n(&mut peer, size).await;
                        assert!(head.starts_with("POST /test/ HTTP/1.1\r\n"));
                        assert!(head.contains("Host: test.local\r\n"));
                        let names: Vec<_> = head.lines().skip(1).filter_map(|l| l.split_once(':').map(|(k, _)| k)).collect();
                        assert_eq!(&names[..3], &["Host", "User-Agent", "Transfer-Encoding"]);
                        assert_eq!(&names[names.len() - 2..], &["Accept-Encoding", "Connection"]);
                        assert!(head.contains("Connection: close\r\n"));
                        assert!(head.contains("User-Agent: Go-http-client/1.1\r\n"));
                        assert_eq!(body, vec![b'x'; size]);
                        assert!(
                            tokio::time::timeout(Duration::from_millis(5), peer.fill_buf()).await.is_err(),
                            "logical half-close must leave the H1 upload open until the response"
                        );
                        let mut encoded = b"after EOF, read half remains open".to_vec();
                        if gzip {
                            let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                            z.write_all(&encoded).unwrap();
                            encoded = z.finish().unwrap();
                        }
                        let headers = format!(
                            "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n{}\r\n",
                            if gzip { "Content-Encoding: gzip\r\n" } else { "" }
                        );
                        for b in headers.bytes() {
                            peer.get_mut().write_all(&[b]).await.unwrap();
                        }
                        for data in encoded.chunks(7) {
                            peer.get_mut().write_all(format!("{:x};extension=yes\r\n", data.len()).as_bytes()).await.unwrap();
                            peer.get_mut().write_all(data).await.unwrap();
                            peer.get_mut().write_all(b"\r\n").await.unwrap();
                        }
                        peer.get_mut().write_all(b"0\r\nX-Trailer: ignored\r\n\r\n").await.unwrap();
                        peer.get_mut().shutdown().await.unwrap();
                    });
                    let t = transport(
                        Config::from_json(&serde_json::json!({"mode": "stream-one", "path": "/test", "headers": {"User-Agent": "golang"}}))
                            .unwrap(),
                        connect,
                    );
                    let mut s = t.open().await.unwrap();
                    s.write_all(&vec![b'x'; size]).await.unwrap();
                    s.shutdown().await.unwrap();
                    assert_eq!(s.write(b"after shutdown").await.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
                    let mut back = Vec::new();
                    s.read_to_end(&mut back).await.unwrap();
                    assert_eq!(back, b"after EOF, read half remains open");
                    server.await.unwrap();
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn error_status_and_truncated_chunk_are_errors() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for response in [
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".as_slice(),
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nab",
                    b"not HTTP\r\n\r\n",
                ] {
                    let (client, peer) = tokio::io::duplex(4096);
                    let response = response.to_vec();
                    let slot = Rc::new(RefCell::new(Some(client)));
                    let connect: Connector = Rc::new(move || {
                        let io = slot.borrow_mut().take().unwrap();
                        Box::pin(async { Ok(Box::new(io) as Box<dyn Io>) })
                    });
                    tokio::task::spawn_local(async move {
                        let mut peer = tokio::io::BufReader::new(peer);
                        head(&mut peer).await;
                        peer.get_mut().write_all(&response).await.unwrap();
                    });
                    let t = transport(Config::from_json(&serde_json::json!({"mode": "stream-one"})).unwrap(), connect);
                    let mut s = t.open().await.unwrap();
                    s.shutdown().await.unwrap();
                    assert!(s.read_to_end(&mut Vec::new()).await.is_err());
                    assert!(s.read_u8().await.is_err());
                }
            })
            .await;
    }

    #[test]
    fn xmux_limits_past_usize_keep_their_meaning() {
        let pool = Pool::new(Config::from_json(&serde_json::json!({"xmux": {"maxConcurrency": 4294967296u64}})).unwrap().xmux());
        let a = pool.lease();
        let b = pool.lease();
        assert!(Rc::ptr_eq(&a.0.0, &b.0.0), "лимит 2³² потоков не исчерпан двумя");
        let pool = Pool::new(Config::from_json(&serde_json::json!({"xmux": {"maxConnections": 4294967296u64}})).unwrap().xmux());
        let a = pool.lease();
        let b = pool.lease();
        assert!(!Rc::ptr_eq(&a.0.0, &b.0.0), "до 2³² соединений пул открывает новые");
    }

    #[tokio::test(start_paused = true)]
    async fn xmux_limits_count_sessions_requests_and_age_without_maintenance_timer() {
        let limited =
            Pool::new(Config::from_json(&serde_json::json!({"xmux": {"maxConnections": 1, "cMaxReuseTimes": 2}})).unwrap().xmux());
        let first = limited.lease();
        let old = first.0.0.clone();
        drop(first);
        let second = limited.lease();
        assert!(Rc::ptr_eq(&old, &second.0.0));
        assert_eq!(second.client().reuses.get(), 0);
        drop(second);
        assert!(!Rc::ptr_eq(&old, &limited.lease().0.0));
        let cfg = Config::from_json(
            &serde_json::json!({"xmux": {"maxConcurrency": 1, "cMaxReuseTimes": 2, "hMaxRequestTimes": 3, "hMaxReusableSecs": 10}}),
        )
        .unwrap();
        let pool = Pool::new(cfg.xmux());
        let a = pool.lease();
        let b = pool.lease();
        assert!(!Rc::ptr_eq(&a.0.0, &b.0.0));
        drop(b);
        let busy = a.clone();
        drop(a);
        assert_eq!(busy.client().running.get(), 1);
        let client = busy.0.0.clone();
        drop(busy);
        let a = pool.lease();
        assert_eq!(a.client().running.get(), 1);
        if Rc::ptr_eq(&client, &a.0.0) {
            assert_eq!(a.client().reuses.get(), 0);
        }
        for _ in 0..3 {
            spend(&a.client().requests);
        }
        assert!(!a.client().reusable());
        let fresh = pool.lease();
        assert!(!Rc::ptr_eq(&a.0.0, &fresh.0.0));
        tokio::time::advance(Duration::from_secs(11)).await;
        assert!(!fresh.client().reusable());
        assert!(pool.lease().client().reusable());
        pool.reset();
        assert!(pool.clients.borrow().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn packet_up_keeps_connection_across_idle_and_rotates_request_limit() {
        tokio::task::LocalSet::new().run_until(async {
            let dials = Rc::new(Cell::new(0)); let copies = dials.clone();
            let requests = Rc::new(RefCell::new(Vec::new())); let records = requests.clone();
            let closed = Rc::new(Cell::new(0)); let closes = closed.clone();
            let connect: Connector = Rc::new(move || {
                let id = copies.get(); copies.set(id + 1); let records = records.clone();
                let closes = closes.clone();
                Box::pin(async move {
                    let (client, peer) = tokio::io::duplex(4096);
                    tokio::task::spawn_local(async move {
                        let mut peer = tokio::io::BufReader::new(peer);
                        loop {
                            if peer.fill_buf().await.unwrap().is_empty() { break; }
                            let (head, body) = request(&mut peer).await; assert!(!head.contains("Connection: close"));
                            records.borrow_mut().push((id, body)); peer.get_mut().write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await.unwrap();
                        }
                        closes.set(closes.get() + 1);
                    });
                    Ok(Box::new(client) as Box<dyn Io>)
                })
            });
            let t = transport(Config::from_json(&serde_json::json!({"mode": "packet-up", "scMinPostsIntervalMs": 0, "xmux": {"maxConnections": 1, "hMaxRequestTimes": 2}})).unwrap(), connect);
            let lease = t.h1.lease(); let (tx, rx) = mpsc::channel(8);
            for data in [b"one", b"two", b"tri"] { tx.send(data.to_vec()).await.unwrap(); }
            let task = tokio::task::spawn_local(uploader(t.clone(), lease, "session".into(), rx));
            while requests.borrow().len() < 3 { tokio::task::yield_now().await; }
            while closed.get() < 1 { tokio::task::yield_now().await; }
            assert_eq!(requests.borrow().iter().map(|r| r.0).collect::<Vec<_>>(), [0, 0, 1]);
            tx.send(b"new".to_vec()).await.unwrap(); drop(tx); task.await.unwrap();
            assert_eq!(requests.borrow().iter().map(|r| r.0).collect::<Vec<_>>(), [0, 0, 1, 1]);
            assert_eq!(dials.get(), 2);
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_packet_response_wakes_a_reader_waiting_on_download() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for reply in
                    [b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".as_slice(), b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nab"]
                {
                    let dials = Rc::new(Cell::new(0));
                    let reply = reply.to_vec();
                    let connect: Connector = Rc::new(move || {
                        let down = dials.get() == 0;
                        dials.set(dials.get() + 1);
                        let reply = reply.clone();
                        Box::pin(async move {
                            let (client, peer) = tokio::io::duplex(4096);
                            tokio::task::spawn_local(async move {
                                let mut peer = tokio::io::BufReader::new(peer);
                                request(&mut peer).await;
                                if down {
                                    peer.get_mut().write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
                                    let _ = peer.read_to_end(&mut Vec::new()).await;
                                } else {
                                    peer.get_mut().write_all(&reply).await.unwrap();
                                }
                            });
                            Ok(Box::new(client) as Box<dyn Io>)
                        })
                    });
                    let t = transport(
                        Config::from_json(&serde_json::json!({"mode": "packet-up", "scMinPostsIntervalMs": 0})).unwrap(),
                        connect,
                    );
                    let mut s = t.open().await.unwrap();
                    s.write_all(b"first").await.unwrap();
                    s.flush().await.unwrap();
                    let e = tokio::time::timeout(Duration::from_secs(2), s.read_u8()).await.unwrap().unwrap_err();
                    assert_eq!(e.kind(), io::ErrorKind::BrokenPipe);
                    assert_eq!(s.read_u8().await.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
                    assert_eq!(s.write(b"after failure").await.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
                    assert_eq!(s.flush().await.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
                    assert_eq!(s.shutdown().await.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
                }
            })
            .await;
    }

    #[test]
    fn chunks_split_into_conn_writes_like_go_bufio() {
        let sizes = |n: usize| {
            let mut b = Bufio::default();
            b.write(format!("{n:x}\r\n").as_bytes());
            b.write(&vec![0; n]);
            b.write(b"\r\n");
            b.flush();
            b.out.iter().map(Vec::len).collect::<Vec<_>>()
        };
        assert_eq!(sizes(8192), [4096, 4102, 2]);
        assert_eq!(sizes(100), [106]);
        assert_eq!(sizes(4093), [4096, 4]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn packet_up_writes_requests_back_to_back_with_header_case_and_without_gzip() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let heads = Rc::new(RefCell::new(Vec::new()));
                let seen = heads.clone();
                let connect: Connector = Rc::new(move || {
                    let seen = seen.clone();
                    Box::pin(async move {
                        let (client, peer) = tokio::io::duplex(65536);
                        tokio::task::spawn_local(async move {
                            let mut peer = tokio::io::BufReader::new(peer);
                            for _ in 0..2 {
                                let (head, _) = request(&mut peer).await;
                                seen.borrow_mut().push(head);
                            }
                            peer.get_mut()
                                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                                .await
                                .unwrap();
                            let _ = peer.read_to_end(&mut Vec::new()).await;
                        });
                        Ok(Box::new(client) as Box<dyn Io>)
                    })
                });
                let t = transport(
                    Config::from_json(&serde_json::json!({"mode": "packet-up", "scMinPostsIntervalMs": 0, "headers": {"DNT": "1"}}))
                        .unwrap(),
                    connect,
                );
                let (tx, rx) = mpsc::channel(8);
                tx.send(b"one".to_vec()).await.unwrap();
                tx.send(b"two".to_vec()).await.unwrap();
                drop(tx);
                tokio::time::timeout(Duration::from_secs(2), uploader(t.clone(), t.h1.lease(), "sid".into(), rx)).await.unwrap();
                let heads = heads.borrow();
                assert_eq!(heads.len(), 2);
                for h in heads.iter() {
                    assert!(h.contains("\r\nDNT: 1\r\n"), "{h}");
                    assert!(!h.contains("Accept-Encoding"), "{h}");
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn closed_keep_alive_retries_the_same_packet_on_a_fresh_connection() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let seen = Rc::new(RefCell::new(Vec::new()));
                let records = seen.clone();
                let connect: Connector = Rc::new(move || {
                    let records = records.clone();
                    Box::pin(async move {
                        let (client, peer) = tokio::io::duplex(4096);
                        tokio::task::spawn_local(async move {
                            let mut peer = tokio::io::BufReader::new(peer);
                            let (head, body) = request(&mut peer).await;
                            records.borrow_mut().push((head.split_whitespace().nth(1).unwrap().to_string(), body));
                            peer.get_mut().write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await.unwrap();
                        });
                        Ok(Box::new(client) as Box<dyn Io>)
                    })
                });
                let t =
                    transport(Config::from_json(&serde_json::json!({"mode": "packet-up", "scMinPostsIntervalMs": 0})).unwrap(), connect);
                let (tx, rx) = mpsc::channel(8);
                tx.send(b"first".to_vec()).await.unwrap();
                tx.send(b"second".to_vec()).await.unwrap();
                drop(tx);
                uploader(t.clone(), t.h1.lease(), "sid".into(), rx).await;
                assert_eq!(*seen.borrow(), [("/sid/0".into(), b"first".to_vec()), ("/sid/1".into(), b"second".to_vec())]);
            })
            .await;
    }
}
