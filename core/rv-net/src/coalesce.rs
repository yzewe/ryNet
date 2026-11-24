use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod imp {
    use std::os::fd::AsRawFd;
    use std::time::Duration;
    use tokio::time::{Instant, Sleep};

    const LOWAT: libc::c_int = 64 * 1024;
    pub(super) const WAIT: Duration = Duration::from_millis(5);
    const WINDOW: Duration = Duration::from_millis(20);
    const BULK: usize = 16 * 1024;
    const SLOW: usize = 4 * 1024;

    pub(super) struct State {
        pub(super) on: bool,
        pub(super) sleep: Option<std::pin::Pin<Box<Sleep>>>,
        pub(super) drain: bool,
        win_at: Instant,
        win_bytes: usize,
    }

    impl State {
        pub(super) fn new() -> Self {
            State { on: false, sleep: None, drain: false, win_at: Instant::now(), win_bytes: 0 }
        }

        pub(super) fn set(&mut self, s: &tokio::net::TcpStream, on: bool) {
            if self.on == on {
                return;
            }
            let v: libc::c_int = if on { LOWAT } else { 1 };
            let r = unsafe {
                libc::setsockopt(
                    s.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_RCVLOWAT,
                    (&v as *const libc::c_int).cast(),
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                )
            };
            if r == 0 {
                self.on = on;
            }
            if !self.on {
                self.sleep = None;
            }
        }

        pub(super) fn account(&mut self, s: &tokio::net::TcpStream, n: usize) {
            let now = Instant::now();
            if now.duration_since(self.win_at) > WINDOW {
                self.win_at = now;
                self.win_bytes = 0;
            }
            self.win_bytes += n;
            if !self.on && self.win_bytes >= BULK {
                self.set(s, true);
            }
        }

        pub(super) fn recv(&mut self, s: &tokio::net::TcpStream, buf: &mut tokio::io::ReadBuf<'_>) -> Option<std::io::Result<usize>> {
            let room = buf.remaining();
            if room == 0 {
                return Some(Ok(0));
            }
            let mut r = raw(s, buf);
            if r.as_ref().is_err_and(|e| e.kind() == std::io::ErrorKind::WouldBlock) && self.on {
                self.set(s, false);
                if !self.on {
                    r = raw(s, buf);
                }
            }
            let n = match r {
                Ok(n) => n,
                Err(e) => {
                    self.drain = false;
                    return match e.kind() {
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted => None,
                        _ => Some(Err(e)),
                    };
                }
            };
            self.drain = n == room;
            if n > 0 && n < SLOW && !self.drain && self.on {
                self.set(s, false);
                self.drain = !self.on;
            }
            Some(Ok(n))
        }
    }

    fn raw(s: &tokio::net::TcpStream, buf: &mut tokio::io::ReadBuf<'_>) -> std::io::Result<usize> {
        let room = buf.remaining();
        let dst = unsafe { buf.unfilled_mut() };
        let n = unsafe { libc::recv(s.as_raw_fd(), dst.as_mut_ptr().cast(), room, libc::MSG_DONTWAIT) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let n = n as usize;
        unsafe { buf.assume_init(n) };
        buf.advance(n);
        Ok(n)
    }
}

pub struct Coalesced {
    inner: TcpStream,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    st: imp::State,
}

impl Coalesced {
    pub fn new(inner: TcpStream) -> Self {
        Coalesced {
            inner,
            #[cfg(any(target_os = "linux", target_os = "android"))]
            st: imp::State::new(),
        }
    }

    pub fn get_ref(&self) -> &TcpStream {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut TcpStream {
        &mut self.inner
    }
}

impl AsyncRead for Coalesced {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if me.st.drain {
            match me.st.recv(&me.inner, buf) {
                Some(Ok(_)) => return Poll::Ready(Ok(())),
                Some(Err(e)) => return Poll::Ready(Err(e)),
                None => {}
            }
        }
        let before = buf.filled().len();
        match Pin::new(&mut me.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                #[cfg(any(target_os = "linux", target_os = "android"))]
                {
                    me.st.sleep = None;
                    let n = buf.filled().len() - before;
                    if n > 0 {
                        me.st.account(&me.inner, n);
                    }
                }
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => {}
        }
        let _ = before;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if me.st.on {
            let sleep = me.st.sleep.get_or_insert_with(|| Box::pin(tokio::time::sleep(imp::WAIT)));
            if sleep.as_mut().poll(cx).is_ready() {
                me.st.sleep = None;
                match me.st.recv(&me.inner, buf) {
                    Some(Ok(_)) => return Poll::Ready(Ok(())),
                    Some(Err(e)) => return Poll::Ready(Err(e)),
                    None => {}
                }
            }
        }
        Poll::Pending
    }
}

impl AsyncWrite for Coalesced {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }
    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[io::IoSlice<'_>]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn pair() -> (tokio::net::TcpStream, Coalesced) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let c = tokio::net::TcpStream::connect(l.local_addr().unwrap()).await.unwrap();
        let (s, _) = l.accept().await.unwrap();
        (s, Coalesced::new(c))
    }

    #[tokio::test]
    async fn bulk_turns_threshold_on_and_tail_still_arrives() {
        let (mut s, mut c) = pair().await;
        let total = 1 << 20;
        let writer = tokio::spawn(async move {
            let b = vec![7u8; 4096];
            for _ in 0..total / 4096 {
                s.write_all(&b).await.unwrap();
            }
            s.write_all(&[9u8; 100]).await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(s);
        });
        let mut buf = vec![0u8; 16 * 1024];
        let mut got = 0;
        let mut on = false;
        while got < total + 100 {
            let n = tokio::time::timeout(Duration::from_secs(1), c.read(&mut buf)).await.expect("хвост не застрял").unwrap();
            assert!(n > 0);
            got += n;
            on |= c.st.on;
        }
        assert!(on, "на мегабайте подряд порог включился");
        assert!(tokio::time::timeout(Duration::from_millis(50), c.read(&mut buf)).await.is_err());
        assert!(!c.st.on, "поток встал — порог снят");
        writer.abort();
    }

    #[tokio::test]
    async fn small_exchange_is_not_delayed() {
        let (mut s, mut c) = pair().await;
        let mut b = [0u8; 16];
        for _ in 0..50 {
            s.write_all(b"ping").await.unwrap();
            let t = std::time::Instant::now();
            let n = c.read(&mut b).await.unwrap();
            assert_eq!(n, 4);
            assert!(t.elapsed() < Duration::from_millis(3));
        }
        assert!(!c.st.on);
    }
}
