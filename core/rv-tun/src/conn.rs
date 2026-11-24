use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::tcp;

pub struct TcpConn(Inner);

enum Inner {
    Kernel(TcpStream),
    User(tcp::Socket),
}

impl TcpConn {
    pub(crate) fn kernel(s: TcpStream) -> Self {
        Self(Inner::Kernel(s))
    }

    pub(crate) fn user(s: tcp::Socket) -> Self {
        Self(Inner::User(s))
    }

    pub fn set_nodelay(&self, on: bool) {
        if let Inner::Kernel(s) = &self.0 {
            let _ = s.set_nodelay(on);
        }
    }

    pub fn zero_copy(&self) -> bool {
        matches!(self.0, Inner::User(_))
    }

    pub fn poll_fill(&mut self, cx: &mut Context<'_>, src: Pin<&mut dyn AsyncRead>) -> Poll<io::Result<usize>> {
        match &mut self.0 {
            Inner::User(s) => s.poll_fill(cx, src),
            Inner::Kernel(_) => Poll::Ready(Err(io::ErrorKind::Unsupported.into())),
        }
    }

    pub fn poll_drain(&mut self, cx: &mut Context<'_>, dst: Pin<&mut dyn AsyncWrite>) -> Poll<io::Result<usize>> {
        match &mut self.0 {
            Inner::User(s) => s.poll_drain(cx, dst),
            Inner::Kernel(_) => Poll::Ready(Err(io::ErrorKind::Unsupported.into())),
        }
    }

    pub fn closer(&self) -> Closer {
        match &self.0 {
            Inner::Kernel(s) => Closer(CloserInner::Kernel(raw(s))),
            Inner::User(s) => Closer(CloserInner::User(s.handle())),
        }
    }
}

pub struct Closer(CloserInner);

enum CloserInner {
    #[cfg(unix)]
    Kernel(std::os::fd::RawFd),
    #[cfg(windows)]
    Kernel(std::os::windows::io::RawSocket),
    User(tcp::Handle),
}

impl Closer {
    pub fn reset(&self) {
        match &self.0 {
            CloserInner::Kernel(fd) => reset_on_close(*fd),
            CloserInner::User(h) => h.reset(),
        }
    }
}

#[cfg(unix)]
fn raw(s: &TcpStream) -> std::os::fd::RawFd {
    use std::os::fd::AsRawFd;
    s.as_raw_fd()
}
#[cfg(windows)]
fn raw(s: &TcpStream) -> std::os::windows::io::RawSocket {
    use std::os::windows::io::AsRawSocket;
    s.as_raw_socket()
}

#[cfg(unix)]
fn reset_on_close(fd: std::os::fd::RawFd) {
    let linger = libc::linger { l_onoff: 1, l_linger: 0 };
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            (&linger as *const libc::linger).cast(),
            std::mem::size_of::<libc::linger>() as libc::socklen_t,
        );
    }
}

#[cfg(windows)]
fn reset_on_close(sock: std::os::windows::io::RawSocket) {
    use windows_sys::Win32::Networking::WinSock::{LINGER, SO_LINGER, SOL_SOCKET, setsockopt};
    let linger = LINGER { l_onoff: 1, l_linger: 0 };
    unsafe {
        setsockopt(sock as usize, SOL_SOCKET, SO_LINGER, (&linger as *const LINGER).cast(), size_of::<LINGER>() as i32);
    }
}

impl AsyncRead for TcpConn {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().0 {
            Inner::Kernel(s) => Pin::new(s).poll_read(cx, buf),
            Inner::User(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TcpConn {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().0 {
            Inner::Kernel(s) => Pin::new(s).poll_write(cx, data),
            Inner::User(s) => Pin::new(s).poll_write(cx, data),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().0 {
            Inner::Kernel(s) => Pin::new(s).poll_flush(cx),
            Inner::User(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().0 {
            Inner::Kernel(s) => Pin::new(s).poll_shutdown(cx),
            Inner::User(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
