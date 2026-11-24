use quinn::udp::{RecvMeta, Transmit, UdpSocketState};
use quinn::{AsyncUdpSocket, UdpPoller};
use socket2::SockRef;
use std::fmt;
use std::future::Future;
use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::{Instant, Sleep};

const WAIT: Duration = Duration::from_millis(4);
const WINDOW: Duration = Duration::from_millis(20);
const BULK_RATE: f64 = 1.6e6;
const FAST_RATE: f64 = 12e6;
const SLOW: usize = 8 * 1024;
const FAST: usize = 48 * 1024;
const RCVBUF: usize = 1 << 20;

enum Io {
    Polled(tokio::net::UdpSocket),
    Timed(std::net::UdpSocket),
    Gone,
}

struct State {
    io: Io,
    sleep: Option<Pin<Box<Sleep>>>,
    got: usize,
    win_at: Instant,
    win_bytes: usize,
}

pub(crate) struct Rx {
    st: Mutex<State>,
}

impl Rx {
    pub(crate) fn new(sock: std::net::UdpSocket) -> io::Result<Self> {
        let _ = socket2::SockRef::from(&sock).set_recv_buffer_size(RCVBUF);
        Ok(Rx {
            st: Mutex::new(State {
                io: Io::Polled(tokio::net::UdpSocket::from_std(sock)?),
                sleep: None,
                got: 0,
                win_at: Instant::now(),
                win_bytes: 0,
            }),
        })
    }

    pub(crate) fn try_send(&self, f: impl FnOnce(SockRef<'_>) -> io::Result<()>) -> io::Result<()> {
        let st = self.st.lock().unwrap();
        match &st.io {
            Io::Polled(s) => s.try_io(tokio::io::Interest::WRITABLE, || f(SockRef::from(s))),
            Io::Timed(s) => f(SockRef::from(s)),
            Io::Gone => Err(io::ErrorKind::NotConnected.into()),
        }
    }

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        let st = self.st.lock().unwrap();
        match &st.io {
            Io::Polled(s) => s.local_addr(),
            Io::Timed(s) => s.local_addr(),
            Io::Gone => Err(io::ErrorKind::NotConnected.into()),
        }
    }

    pub(crate) fn poll_recv(
        &self,
        cx: &mut Context,
        mut recv: impl FnMut(SockRef<'_>) -> io::Result<(usize, usize)>,
    ) -> Poll<io::Result<usize>> {
        let mut guard = self.st.lock().unwrap();
        let st: &mut State = &mut guard;
        loop {
            match &st.io {
                Io::Gone => return Poll::Ready(Err(io::ErrorKind::NotConnected.into())),
                Io::Polled(s) => {
                    std::task::ready!(s.poll_recv_ready(cx))?;
                    let (n, bytes) = match s.try_io(tokio::io::Interest::READABLE, || recv(SockRef::from(s))) {
                        Ok(r) => r,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                        Err(e) => return Poll::Ready(Err(e)),
                    };
                    let now = Instant::now();
                    if now.duration_since(st.win_at) > WINDOW {
                        st.win_at = now;
                        st.win_bytes = 0;
                    }
                    st.win_bytes += bytes;
                    let el = now.duration_since(st.win_at);
                    if cfg!(any(target_os = "linux", target_os = "android")) && el >= WINDOW / 2 {
                        let rate = st.win_bytes as f64 / el.as_secs_f64();
                        if (BULK_RATE..FAST_RATE).contains(&rate) {
                            st.enter_timed();
                        }
                    }
                    return Poll::Ready(Ok(n));
                }
                Io::Timed(s) => {
                    if let Some(sl) = st.sleep.as_mut() {
                        if sl.as_mut().poll(cx).is_pending() {
                            return Poll::Pending;
                        }
                        st.sleep = None;
                        st.got = 0;
                    }
                    match recv(SockRef::from(s)) {
                        Ok((n, bytes)) => {
                            st.got += bytes;
                            return Poll::Ready(Ok(n));
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            if st.got < SLOW || st.got > FAST {
                                st.enter_polled()?;
                                continue;
                            }
                            st.got = 0;
                            st.sleep = Some(Box::pin(tokio::time::sleep(WAIT)));
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(e) => return Poll::Ready(Err(e)),
                    }
                }
            }
        }
    }

    fn poll_writable(&self, cx: &mut Context, sleep: &mut Option<Pin<Box<Sleep>>>) -> Poll<io::Result<()>> {
        {
            let st = self.st.lock().unwrap();
            if let Io::Polled(s) = &st.io {
                *sleep = None;
                return s.poll_send_ready(cx);
            }
        }
        let sl = sleep.get_or_insert_with(|| Box::pin(tokio::time::sleep(Duration::from_millis(1))));
        std::task::ready!(sl.as_mut().poll(cx));
        *sleep = None;
        Poll::Ready(Ok(()))
    }
}

impl State {
    fn enter_timed(&mut self) {
        if let Io::Polled(s) = std::mem::replace(&mut self.io, Io::Gone) {
            match s.into_std() {
                Ok(s) => self.io = Io::Timed(s),
                Err(e) => tracing::debug!(target: "rv::quic", "udp moderate off: {e}"),
            }
        }
        self.sleep = Some(Box::pin(tokio::time::sleep(WAIT)));
        self.got = 0;
    }

    fn enter_polled(&mut self) -> io::Result<()> {
        if let Io::Timed(s) = std::mem::replace(&mut self.io, Io::Gone) {
            self.io = Io::Polled(tokio::net::UdpSocket::from_std(s)?);
        }
        self.sleep = None;
        self.win_at = Instant::now();
        self.win_bytes = 0;
        Ok(())
    }
}

pub(crate) fn bytes(meta: &[RecvMeta], n: usize) -> usize {
    meta[..n].iter().map(|m| m.len).sum()
}

#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
pub struct Moderated {
    udp: UdpSocketState,
    rx: Arc<Rx>,
}

impl fmt::Debug for Moderated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Moderated").finish_non_exhaustive()
    }
}

#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
impl Moderated {
    pub fn new(sock: std::net::UdpSocket) -> io::Result<Self> {
        let udp = UdpSocketState::new((&sock).into())?;
        Ok(Moderated { udp, rx: Arc::new(Rx::new(sock)?) })
    }
}

impl AsyncUdpSocket for Moderated {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Box::pin(Writable::new(self.rx.clone()))
    }

    fn try_send(&self, t: &Transmit) -> io::Result<()> {
        self.rx.try_send(|s| self.udp.send((&*s).into(), t))
    }

    fn poll_recv(&self, cx: &mut Context, bufs: &mut [IoSliceMut<'_>], meta: &mut [RecvMeta]) -> Poll<io::Result<usize>> {
        self.rx.poll_recv(cx, |s| {
            let n = self.udp.recv((&*s).into(), bufs, meta)?;
            Ok((n, bytes(meta, n)))
        })
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.rx.local_addr()
    }

    fn may_fragment(&self) -> bool {
        self.udp.may_fragment()
    }

    fn max_transmit_segments(&self) -> usize {
        self.udp.max_gso_segments()
    }

    fn max_receive_segments(&self) -> usize {
        self.udp.gro_segments()
    }
}

pub(crate) struct Writable {
    rx: Arc<Rx>,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl Writable {
    pub(crate) fn new(rx: Arc<Rx>) -> Self {
        Writable { rx, sleep: None }
    }

    #[cfg(test)]
    pub(crate) fn poll_writable_pinned(mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        self.rx.poll_writable(cx, &mut self.sleep)
    }
}

impl fmt::Debug for Writable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writable").finish_non_exhaustive()
    }
}

impl UdpPoller for Writable {
    fn poll_writable(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let me = &mut *self;
        me.rx.poll_writable(cx, &mut me.sleep)
    }
}
