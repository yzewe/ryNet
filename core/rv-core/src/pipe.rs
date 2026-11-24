use std::cell::Cell;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
use tokio::time::Instant;

pub const CONN_IDLE: Duration = Duration::from_secs(300);
pub const HALF_IDLE: Duration = Duration::from_secs(5);

const BUF: usize = 16 * 1024;
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
const IDLE_MAX: Duration = Duration::from_secs(24 * 60 * 60);
const REARM: Duration = Duration::from_secs(1);

pub struct Relayed {
    pub up: u64,
    pub down: u64,
    pub idle: bool,
    pub error: Option<io::Error>,
    pub error_side: Option<Side>,
    pub eof: Option<Side>,
    pub quiet: Duration,
    pub gap: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Local,
    Remote,
}

struct Activity {
    last: Cell<Instant>,
    eof: Cell<bool>,
    eof_side: Cell<Option<Side>>,
    gap: Cell<Duration>,
    wake: Notify,
    failed: Cell<Option<Side>>,
}

impl Activity {
    fn progress(&self, from: Side, n: usize) {
        let now = Instant::now();
        self.gap.set(self.gap.get().max(now - self.last.get()));
        if n == 0 {
            if !self.eof.replace(true) {
                self.eof_side.set(Some(from));
                self.last.set(now);
                self.wake.notify_one();
            }
        } else if now - self.last.replace(now) >= REARM {
            self.wake.notify_one();
        }
    }

    fn fail<T>(&self, side: Side, r: Poll<io::Result<T>>) -> Poll<io::Result<T>> {
        if let Poll::Ready(Err(_)) = &r
            && self.failed.get().is_none()
        {
            self.failed.set(Some(side));
        }
        r
    }
}

pub trait Local: AsyncRead + AsyncWrite + Unpin {
    fn zero_copy(&self) -> bool {
        false
    }
    fn poll_fill(&mut self, _cx: &mut Context<'_>, _src: Pin<&mut dyn AsyncRead>) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::ErrorKind::Unsupported.into()))
    }
    fn poll_drain(&mut self, _cx: &mut Context<'_>, _dst: Pin<&mut dyn AsyncWrite>) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::ErrorKind::Unsupported.into()))
    }
}

impl Local for tokio::net::TcpStream {}
impl Local for tokio::io::DuplexStream {}

struct Remote<'a, B> {
    inner: &'a mut B,
    act: &'a Activity,
}

impl<B: AsyncRead + Unpin> AsyncRead for Remote<'_, B> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let r = Pin::new(&mut *self.inner).poll_read(cx, buf);
        self.act.fail(Side::Remote, r)
    }
}

impl<B: AsyncWrite + Unpin> AsyncWrite for Remote<'_, B> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let r = Pin::new(&mut *self.inner).poll_write(cx, data);
        self.act.fail(Side::Remote, r)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let r = Pin::new(&mut *self.inner).poll_flush(cx);
        self.act.fail(Side::Remote, r)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let r = Pin::new(&mut *self.inner).poll_shutdown(cx);
        self.act.fail(Side::Remote, r)
    }
}

struct Half {
    buf: Box<[u8]>,
    pos: usize,
    cap: usize,
    read_done: bool,
    need_flush: bool,
    done: bool,
    amt: u64,
}

impl Half {
    fn new(zero_copy: bool) -> Self {
        Self {
            buf: if zero_copy { Box::new([]) } else { vec![0u8; BUF].into_boxed_slice() },
            pos: 0,
            cap: 0,
            read_done: false,
            need_flush: false,
            done: false,
            amt: 0,
        }
    }

    fn poll_copy(
        &mut self,
        cx: &mut Context<'_>,
        act: &Activity,
        (mut r, rs): (Pin<&mut dyn AsyncRead>, Side),
        (mut w, ws): (Pin<&mut dyn AsyncWrite>, Side),
    ) -> Poll<io::Result<()>> {
        loop {
            if self.done {
                return Poll::Ready(Ok(()));
            }
            if self.pos == self.cap && !self.read_done {
                let mut rb = ReadBuf::new(&mut self.buf);
                match act.fail(rs, r.as_mut().poll_read(cx, &mut rb)) {
                    Poll::Pending => {
                        if self.need_flush {
                            ready!(act.fail(ws, w.as_mut().poll_flush(cx)))?;
                            self.need_flush = false;
                        }
                        return Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Ready(Ok(())) => {
                        let n = rb.filled().len();
                        act.progress(rs, n);
                        if n == 0 {
                            self.read_done = true;
                        } else {
                            self.pos = 0;
                            self.cap = n;
                            self.amt += n as u64;
                        }
                    }
                }
            }
            while self.pos < self.cap {
                let n = ready!(act.fail(ws, w.as_mut().poll_write(cx, &self.buf[self.pos..self.cap])))?;
                if n == 0 {
                    act.failed.set(act.failed.get().or(Some(ws)));
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                self.pos += n;
                self.need_flush = true;
            }
            if self.pos == self.cap && self.read_done {
                ready!(act.fail(ws, w.as_mut().poll_flush(cx)))?;
                ready!(act.fail(ws, w.as_mut().poll_shutdown(cx)))?;
                self.done = true;
            }
        }
    }
}

fn poll_down_zc<A: Local, B: AsyncRead + AsyncWrite + Unpin>(
    h: &mut Half,
    cx: &mut Context<'_>,
    act: &Activity,
    local: &mut A,
    remote: &mut B,
) -> Poll<io::Result<()>> {
    loop {
        if h.done {
            return Poll::Ready(Ok(()));
        }
        if h.read_done {
            ready!(act.fail(Side::Local, Pin::new(&mut *local).poll_shutdown(cx)))?;
            h.done = true;
            continue;
        }
        let mut src = Remote { inner: &mut *remote, act };
        let n = ready!(act.fail(Side::Local, local.poll_fill(cx, Pin::<&mut Remote<'_, B>>::new(&mut src))))?;
        act.progress(Side::Remote, n);
        if n == 0 {
            h.read_done = true;
        } else {
            h.amt += n as u64;
        }
    }
}

fn poll_up_zc<A: Local, B: AsyncRead + AsyncWrite + Unpin>(
    h: &mut Half,
    cx: &mut Context<'_>,
    act: &Activity,
    local: &mut A,
    remote: &mut B,
) -> Poll<io::Result<()>> {
    loop {
        if h.done {
            return Poll::Ready(Ok(()));
        }
        let mut dst = Remote { inner: &mut *remote, act };
        if h.read_done {
            ready!(Pin::new(&mut dst).poll_flush(cx))?;
            ready!(Pin::new(&mut dst).poll_shutdown(cx))?;
            h.done = true;
            continue;
        }
        match act.fail(Side::Local, local.poll_drain(cx, Pin::<&mut Remote<'_, B>>::new(&mut dst))) {
            Poll::Pending => {
                if h.need_flush {
                    ready!(Pin::new(&mut dst).poll_flush(cx))?;
                    h.need_flush = false;
                }
                return Poll::Pending;
            }
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(n)) => {
                act.progress(Side::Local, n);
                if n == 0 {
                    h.read_done = true;
                } else {
                    h.amt += n as u64;
                    h.need_flush = true;
                }
            }
        }
    }
}

async fn watchdog(act: &Activity, idle: Duration) {
    loop {
        let limit = if act.eof.get() { HALF_IDLE } else { idle };
        let deadline = act.last.get() + limit;
        if Instant::now() >= deadline {
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {}
            _ = act.wake.notified() => {}
        }
    }
}

pub async fn copy<A, B>(local: &mut A, remote: &mut B, idle: Duration) -> Relayed
where
    A: Local,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let idle = idle.min(IDLE_MAX);
    let act = Activity {
        last: Cell::new(Instant::now()),
        eof: Cell::new(false),
        eof_side: Cell::new(None),
        gap: Cell::new(Duration::ZERO),
        wake: Notify::new(),
        failed: Cell::new(None),
    };
    let zc = local.zero_copy();
    let (mut up, mut down) = (Half::new(zc), Half::new(zc));
    let relay = std::future::poll_fn(|cx| {
        let u = if zc {
            poll_up_zc(&mut up, cx, &act, local, remote)
        } else {
            up.poll_copy(cx, &act, (Pin::<&mut A>::new(&mut *local), Side::Local), (Pin::<&mut B>::new(&mut *remote), Side::Remote))
        };
        if let Poll::Ready(Err(e)) = u {
            return Poll::Ready(Err(e));
        }
        let d = if zc {
            poll_down_zc(&mut down, cx, &act, local, remote)
        } else {
            down.poll_copy(cx, &act, (Pin::<&mut B>::new(&mut *remote), Side::Remote), (Pin::<&mut A>::new(&mut *local), Side::Local))
        };
        match (u, d) {
            (_, Poll::Ready(Err(e))) => Poll::Ready(Err(e)),
            (Poll::Ready(Ok(())), Poll::Ready(Ok(()))) => Poll::Ready(Ok(())),
            _ => Poll::Pending,
        }
    });
    let (idle, error) = tokio::select! {
        r = relay => (false, r.err()),
        _ = watchdog(&act, idle) => (true, None),
    };
    let error_side = error.as_ref().and(act.failed.get());
    let quiet = Instant::now() - act.last.get();
    for _ in 0..2 {
        if !matches!(tokio::time::timeout(CLOSE_TIMEOUT, tokio::io::AsyncWriteExt::shutdown(&mut *remote)).await, Ok(Ok(()))) {
            break;
        }
    }
    Relayed { up: up.amt, down: down.amt, idle, error, error_side, eof: act.eof_side.get(), quiet, gap: act.gap.get() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test(start_paused = true)]
    async fn silent_connection_is_closed() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut app, mut local) = tokio::io::duplex(1024);
                let (mut remote, mut server) = tokio::io::duplex(1024);
                let t = tokio::task::spawn_local(async move { copy(&mut local, &mut remote, CONN_IDLE).await });
                app.write_all(b"hi").await.unwrap();
                let mut b = [0u8; 2];
                server.read_exact(&mut b).await.unwrap();
                tokio::time::sleep(CONN_IDLE + Duration::from_secs(1)).await;
                let r = t.await.unwrap();
                assert!(r.idle);
                assert_eq!((r.up, r.down), (2, 0));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn close_side_and_pauses_reported() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut app, mut local) = tokio::io::duplex(1024);
                let (mut remote, mut server) = tokio::io::duplex(1024);
                let t = tokio::task::spawn_local(async move { copy(&mut local, &mut remote, CONN_IDLE).await });
                let mut b = [0u8; 1];
                tokio::time::sleep(Duration::from_secs(40)).await;
                server.write_all(b"x").await.unwrap();
                app.read_exact(&mut b).await.unwrap();
                tokio::time::sleep(Duration::from_secs(10)).await;
                server.shutdown().await.unwrap();
                let r = t.await.unwrap();
                assert_eq!(r.eof, Some(Side::Remote));
                assert!(r.idle, "after the server's EOF the app stays silent: half-idle close");
                assert_eq!(r.gap.as_secs(), 40);
                assert_eq!(r.quiet.as_secs(), HALF_IDLE.as_secs());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn active_connection_survives() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut app, mut local) = tokio::io::duplex(1024);
                let (mut remote, mut server) = tokio::io::duplex(1024);
                let t = tokio::task::spawn_local(async move { copy(&mut local, &mut remote, CONN_IDLE).await });
                let mut b = [0u8; 1];
                for _ in 0..10 {
                    tokio::time::sleep(CONN_IDLE / 2).await;
                    server.write_all(b"x").await.unwrap();
                    app.read_exact(&mut b).await.unwrap();
                }
                assert!(!t.is_finished());
                drop(app);
                drop(server);
                let r = t.await.unwrap();
                assert!(!r.idle);
                assert_eq!(r.down, 10);
            })
            .await;
    }

    #[tokio::test]
    async fn remote_failure_side() {
        struct Broken;
        impl AsyncRead for Broken {
            fn poll_read(self: Pin<&mut Self>, _: &mut Context<'_>, _: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()))
            }
        }
        impl AsyncWrite for Broken {
            fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, d: &[u8]) -> Poll<io::Result<usize>> {
                Poll::Ready(Ok(d.len()))
            }
            fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Ok(()))
            }
            fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Ok(()))
            }
        }
        let (_app, mut local) = tokio::io::duplex(1024);
        let r = copy(&mut local, &mut Broken, CONN_IDLE).await;
        assert_eq!(r.error.map(|e| e.kind()), Some(io::ErrorKind::ConnectionReset));
        assert_eq!(r.error_side, Some(Side::Remote));
    }

    #[tokio::test]
    async fn huge_idle_from_config_does_not_overflow() {
        let (mut app, mut local) = tokio::io::duplex(1024);
        let (mut remote, mut server) = tokio::io::duplex(1024);
        let close = async {
            tokio::task::yield_now().await;
            app.shutdown().await.unwrap();
            server.shutdown().await.unwrap();
        };
        let (r, ()) = tokio::join!(copy(&mut local, &mut remote, Duration::from_secs(u64::MAX)), close);
        assert!(!r.idle && r.error.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn half_closed_is_closed_soon() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut app, mut local) = tokio::io::duplex(1024);
                let (mut remote, _server) = tokio::io::duplex(1024);
                let t = tokio::task::spawn_local(async move { copy(&mut local, &mut remote, CONN_IDLE).await });
                app.shutdown().await.unwrap();
                tokio::time::sleep(HALF_IDLE + Duration::from_secs(1)).await;
                assert!(t.await.unwrap().idle);
            })
            .await;
    }
}
