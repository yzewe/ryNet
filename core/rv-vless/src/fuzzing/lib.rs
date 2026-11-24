use super::*;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub use crate::vision::fuzzing as vision;

struct Server<'a> {
    data: &'a [u8],
    step: usize,
}

impl AsyncRead for Server<'_> {
    fn poll_read(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let n = self.data.len().min(buf.remaining()).min(self.step);
        buf.put_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Server<'_> {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub fn response_header(data: &[u8]) {
    let Some((&ctl, data)) = data.split_first() else { return };
    let mut done = None;
    for k in 0..=data.len() {
        let b = &data[..k];
        match parse_response_header(b) {
            Ok(Some(n)) => {
                assert!(n <= b.len() && n == 2 + usize::from(b[1]) && b[0] == 0);
                assert_eq!(*done.get_or_insert(k), n);
            }
            Ok(None) => assert!(done.is_none() && (b.len() < 2 || b.len() < 2 + usize::from(b[1]))),
            Err(_) => assert!(b.len() >= 2 && b[0] != 0),
        }
    }
    let req = Request { id: [7; 16], flow: Flow::None, command: Command::Tcp, address: Address::Ipv4([1, 2, 3, 4]), port: 443 };
    let mut s = VlessStream::new(Server { data, step: usize::from(ctl >> 2) + 1 }, &req).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let mut buf = vec![0u8; [1, 5, 64, 1000][usize::from(ctl & 3)]];
    let mut got = Vec::new();
    let r = loop {
        let mut rb = ReadBuf::new(&mut buf);
        let Poll::Ready(r) = Pin::new(&mut s).poll_read(&mut cx, &mut rb) else { panic!("сервер не отдаёт Pending") };
        match r {
            Ok(()) if rb.filled().is_empty() => break Ok(()),
            Ok(()) => got.extend_from_slice(rb.filled()),
            Err(e) => break Err(e),
        }
    };
    match done {
        Some(n) => assert!(r.is_ok() && got == data[n..]),
        None => assert!(r.is_err() && got.is_empty()),
    }
}
