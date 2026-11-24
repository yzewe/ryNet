use std::{
    cell::Cell,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Instant;

#[derive(Clone)]
pub struct Activity(Rc<Cell<Instant>>);
pub struct ActivityIo<T> {
    inner: T,
    activity: Activity,
    frames: Frames,
    bdp: Option<Rc<Bdp>>,
}

#[derive(Default)]
struct Frames {
    header: [u8; 9],
    used: usize,
    left: usize,
    payload: usize,
    padding: Option<u8>,
}
impl Frames {
    fn observe(&mut self, mut bytes: &[u8], activity: &Activity, bdp: Option<&Bdp>) {
        while !bytes.is_empty() {
            if self.used < 9 {
                let n = (9 - self.used).min(bytes.len());
                self.header[self.used..self.used + n].copy_from_slice(&bytes[..n]);
                self.used += n;
                bytes = &bytes[n..];
                if self.used < 9 {
                    break;
                }
                self.payload = u32::from_be_bytes([0, self.header[0], self.header[1], self.header[2]]) as usize;
                self.left = self.payload;
                self.padding = None;
            }
            if self.left > 0 {
                if bytes.is_empty() {
                    break;
                }
                if self.left == self.payload && self.header[3] == 0 && self.header[4] & 8 != 0 {
                    self.padding = Some(bytes[0]);
                }
                let n = self.left.min(bytes.len());
                self.left -= n;
                bytes = &bytes[n..];
            }
            if self.left == 0 {
                activity.0.set(Instant::now());
                if self.header[3] == 0 {
                    let overhead = self.padding.map_or(0, |p| p as usize + 1);
                    if let Some(bdp) = bdp {
                        bdp.observe(self.payload.saturating_sub(overhead));
                    }
                }
                self.used = 0;
            }
        }
    }
}
impl<T> ActivityIo<T> {
    pub fn new(inner: T) -> (Self, Activity) {
        Self::with_bdp(inner, None)
    }
    pub fn with_bdp(inner: T, bdp: Option<Rc<Bdp>>) -> (Self, Activity) {
        let activity = Activity(Rc::new(Cell::new(Instant::now())));
        (Self { inner, activity: activity.clone(), frames: Frames::default(), bdp }, activity)
    }
}
impl Activity {
    pub fn last_read(&self) -> Instant {
        self.0.get()
    }
}
impl<T: AsyncRead + Unpin> AsyncRead for ActivityIo<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let s = self.get_mut();
        let before = buf.filled().len();
        let result = Pin::new(&mut s.inner).poll_read(cx, buf);
        s.frames.observe(&buf.filled()[before..], &s.activity, s.bdp.as_deref());
        result
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for ActivityIo<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, b)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Records {
    GoHttp2,
    GrpcGo,
}

pub struct FrameFlush<T> {
    inner: T,
    preface_left: usize,
    grouped: usize,
    header: [u8; 9],
    header_used: usize,
    payload_left: usize,
    flush_due: bool,
    grpc: Option<u8>,
    batch: usize,
    yielded: usize,
    last_type: u8,
    server: Option<Rc<Preface>>,
}

#[derive(Default)]
pub struct Preface {
    got: Cell<usize>,
    notify: tokio::sync::Notify,
}
impl Preface {
    pub async fn wait(&self) {
        while self.got.get() < 9 {
            self.notify.notified().await;
        }
    }
}

const GRPC_BATCH: usize = 32 * 1024;

impl<T> FrameFlush<T> {
    pub fn new(inner: T, records: Records) -> Self {
        let grouped = match records {
            Records::GoHttp2 => 2,
            Records::GrpcGo => 0,
        };
        let grpc = (records == Records::GrpcGo).then_some(0);
        Self {
            inner,
            preface_left: 24,
            grouped,
            header: [0; 9],
            header_used: 0,
            payload_left: 0,
            flush_due: false,
            grpc,
            batch: 0,
            yielded: 0,
            last_type: 0,
            server: None,
        }
    }

    pub fn server_preface(&mut self) -> Rc<Preface> {
        self.server.get_or_insert_with(Default::default).clone()
    }

    fn until_boundary(&self, b: &[u8]) -> usize {
        if self.preface_left > 0 {
            return self.preface_left.min(b.len());
        }
        if self.header_used < 9 {
            let need = 9 - self.header_used;
            if b.len() < need {
                return b.len();
            }
            let mut h = self.header;
            h[self.header_used..].copy_from_slice(&b[..need]);
            let len = u32::from_be_bytes([0, h[0], h[1], h[2]]) as usize;
            return need + len.min(b.len() - need);
        }
        self.payload_left.min(b.len())
    }

    fn frame_done(&mut self) {
        if let Some(g) = self.grpc {
            match g {
                0 => self.grpc = Some(1),
                1 => {
                    self.grpc = Some(2);
                    self.flush_due = true;
                }
                _ => {}
            }
            return;
        }
        if self.grouped > 0 {
            self.grouped -= 1;
            self.flush_due = self.grouped == 0;
        } else {
            self.flush_due = true;
        }
    }

    fn advance(&mut self, mut b: &[u8]) {
        while !b.is_empty() {
            if self.preface_left > 0 {
                let n = self.preface_left.min(b.len());
                self.preface_left -= n;
                b = &b[n..];
                if self.preface_left == 0 && self.grouped == 0 {
                    self.flush_due = true;
                }
            } else if self.header_used < 9 {
                let n = (9 - self.header_used).min(b.len());
                self.header[self.header_used..self.header_used + n].copy_from_slice(&b[..n]);
                self.header_used += n;
                b = &b[n..];
                if self.header_used == 9 {
                    self.last_type = self.header[3];
                    self.payload_left = u32::from_be_bytes([0, self.header[0], self.header[1], self.header[2]]) as usize;
                    if self.payload_left == 0 {
                        self.header_used = 0;
                        self.frame_done();
                    }
                }
            } else {
                let n = self.payload_left.min(b.len());
                self.payload_left -= n;
                b = &b[n..];
                if self.payload_left == 0 {
                    self.header_used = 0;
                    self.frame_done();
                }
            }
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for FrameFlush<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
        if let Some(p) = &this.server
            && p.got.get() < 9
        {
            p.got.set(p.got.get() + buf.filled().len() - before);
            if p.got.get() >= 9 {
                p.notify.notify_one();
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for FrameFlush<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.grpc == Some(1) && this.header_used == 0 && b.get(3) != Some(&8) {
            this.grpc = Some(2);
            this.flush_due = true;
        }
        if this.flush_due {
            ready!(Pin::new(&mut this.inner).poll_flush(cx))?;
            this.flush_due = false;
            this.batch = 0;
            this.yielded = 0;
        }
        let batching = this.grpc == Some(2);
        let n = if batching { b.len().min(GRPC_BATCH - this.batch) } else { this.until_boundary(b) };
        let n = ready!(Pin::new(&mut this.inner).poll_write(cx, &b[..n]))?;
        this.advance(&b[..n]);
        if batching {
            this.batch += n;
            this.flush_due = this.batch == GRPC_BATCH;
        }
        Poll::Ready(Ok(n))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.grpc == Some(2) && this.last_type == 0 && this.batch > this.yielded && this.batch < GRPC_BATCH {
            this.yielded = this.batch;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        ready!(Pin::new(&mut this.inner).poll_flush(cx))?;
        this.flush_due = false;
        this.batch = 0;
        this.yielded = 0;
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

pub struct GoWrites<T> {
    inner: T,
    chunk: usize,
    written: usize,
    flush_due: bool,
}

pub const XRAY_BUFFER: usize = 8192;

impl<T> GoWrites<T> {
    pub fn new(inner: T, chunk: usize) -> Self {
        Self { inner, chunk, written: 0, flush_due: false }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for GoWrites<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for GoWrites<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.flush_due {
            ready!(Pin::new(&mut this.inner).poll_flush(cx))?;
            this.flush_due = false;
        }
        if b.is_empty() {
            return Pin::new(&mut this.inner).poll_write(cx, b);
        }
        let take = b.len().min(this.chunk - this.written);
        let n = ready!(Pin::new(&mut this.inner).poll_write(cx, &b[..take]))?;
        this.written += n;
        if n == b.len() || this.written == this.chunk {
            this.written = 0;
            this.flush_due = Pin::new(&mut this.inner).poll_flush(cx)?.is_pending();
        }
        Poll::Ready(Ok(n))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(Pin::new(&mut this.inner).poll_flush(cx))?;
        this.flush_due = false;
        this.written = 0;
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

pub const BDP_LIMIT: u32 = 16 << 20;
pub struct Bdp {
    sample: Cell<u64>,
    started: Cell<bool>,
    current: Cell<u32>,
    wakeup: tokio::sync::Notify,
}
impl Bdp {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { sample: Cell::new(0), started: Cell::new(false), current: Cell::new(65535), wakeup: tokio::sync::Notify::new() })
    }
    pub fn observe(&self, n: usize) {
        if n == 0 || self.current.get() == BDP_LIMIT {
            return;
        }
        self.sample.set(self.sample.get().saturating_add(n as u64));
        if !self.started.replace(true) {
            self.wakeup.notify_one();
        }
    }
    pub(crate) async fn notified(&self) {
        self.wakeup.notified().await
    }
    pub(crate) fn finish(&self) -> u64 {
        self.started.set(false);
        self.sample.replace(0)
    }
    pub(crate) fn current(&self) -> u32 {
        self.current.get()
    }
    pub(crate) fn set_current(&self, value: u32) {
        self.current.set(value)
    }
}

#[derive(Default)]
pub(crate) struct Estimator {
    samples: u64,
    rtt: f64,
    max_bandwidth: f64,
}
impl Estimator {
    pub fn update(&mut self, current: u32, sample: u64, rtt: Duration) -> u32 {
        self.samples += 1;
        let rtt = rtt.as_secs_f64().max(1e-9);
        if self.samples < 10 {
            self.rtt += (rtt - self.rtt) / self.samples as f64;
        } else {
            self.rtt += (rtt - self.rtt) * 0.9;
        }
        let bandwidth = sample as f64 / (self.rtt * 1.5);
        self.max_bandwidth = self.max_bandwidth.max(bandwidth);
        if sample as f64 >= 0.66 * current as f64 && bandwidth == self.max_bandwidth {
            current.max(sample.saturating_mul(2).min(BDP_LIMIT as u64) as u32)
        } else {
            current
        }
    }
}

#[cfg(kani)]
#[path = "proofs/activity.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        open: usize,
        records: Vec<usize>,
        accept: Option<usize>,
    }
    impl AsyncWrite for Recorder {
        fn poll_write(mut self: Pin<&mut Self>, _: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
            let n = b.len().min(self.accept.unwrap_or(usize::MAX));
            self.open += n;
            Poll::Ready(Ok(n))
        }
        fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            if self.open > 0 {
                let n = std::mem::take(&mut self.open);
                self.records.push(n);
            }
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.poll_flush(cx)
        }
    }

    #[tokio::test]
    async fn frames_end_tls_records_where_go_flushes() {
        use tokio::io::AsyncWriteExt;
        let frame = |len: usize, typ: u8| {
            let mut f = vec![(len >> 16) as u8, (len >> 8) as u8, len as u8, typ, 0, 0, 0, 0, 1];
            f.resize(9 + len, 0);
            f
        };
        let mut wire = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
        for f in [frame(24, 4), frame(4, 8), frame(474, 1), frame(73, 0), frame(0, 4)] {
            wire.extend(f);
        }
        for (records, expected) in [(Records::GoHttp2, vec![24 + 33 + 13, 483, 82, 9]), (Records::GrpcGo, vec![24, 33 + 13, 483 + 82 + 9])]
        {
            let mut io = FrameFlush::new(Recorder::default(), records);
            io.write_all(&wire).await.unwrap();
            io.flush().await.unwrap();
            assert_eq!(io.inner.records, expected, "{records:?}");
        }
        let mut io = FrameFlush::new(Recorder::default(), Records::GrpcGo);
        let mut wire = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
        for f in [frame(0, 4), frame(0, 4), frame(8200, 0), frame(8200, 0), frame(8200, 0), frame(8200, 0), frame(8200, 0)] {
            wire.extend(f);
        }
        io.write_all(&wire).await.unwrap();
        io.flush().await.unwrap();
        assert_eq!(io.inner.records, [24, 9, 32768, 5 * 8209 + 9 - 32768]);
    }
    #[tokio::test]
    async fn go_writes_end_a_record_per_caller_write_and_xray_buffer() {
        use tokio::io::AsyncWriteExt;
        let mut io = GoWrites::new(Recorder { accept: Some(5000), ..Default::default() }, XRAY_BUFFER);
        io.write_all(&[0; 20000]).await.unwrap();
        io.write_all(&[0; 100]).await.unwrap();
        io.write_all(&[0; 50]).await.unwrap();
        io.flush().await.unwrap();
        assert_eq!(io.inner.records, [8192, 8192, 3616, 100, 50]);
    }
    #[test]
    fn bdp_grows_only_on_a_saturated_sample_and_stops_at_limit() {
        let mut e = Estimator::default();
        assert_eq!(e.update(65535, 1000, Duration::from_millis(100)), 65535);
        assert_eq!(e.update(65535, 65535, Duration::from_millis(100)), 131070);
        assert_eq!(e.update(131070, 32 << 20, Duration::from_millis(100)), BDP_LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn partial_frames_do_not_reset_idle_and_data_is_sampled_before_body_read() {
        let activity = Activity(Rc::new(Cell::new(Instant::now())));
        let started = activity.last_read();
        let bdp = Bdp::new();
        let mut frames = Frames::default();
        tokio::time::advance(Duration::from_secs(10)).await;
        frames.observe(&[0, 0, 5, 0, 8], &activity, Some(&bdp));
        assert_eq!(activity.last_read(), started);
        frames.observe(&[0, 0, 0, 1, 2, b'a', b'b'], &activity, Some(&bdp));
        assert_eq!(activity.last_read(), started);
        frames.observe(&[0, 0], &activity, Some(&bdp));
        assert_eq!(activity.last_read(), Instant::now());
        assert_eq!(bdp.finish(), 2, "padding must not enter the DATA sample");
    }
}
