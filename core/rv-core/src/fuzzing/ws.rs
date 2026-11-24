use super::*;

pub fn frames(data: &[u8]) {
    use std::task::{Context, Poll, Waker};
    let mut cx = Context::from_waker(Waker::noop());
    let (client, mut server) = tokio::io::duplex(data.len() + MAX_WRITE + (64 << 10));
    match Pin::new(&mut server).poll_write(&mut cx, data) {
        Poll::Ready(Ok(n)) if n == data.len() => {}
        _ => return,
    }
    if !matches!(Pin::new(&mut server).poll_shutdown(&mut cx), Poll::Ready(Ok(()))) {
        return;
    }
    let mut ws = WsStream {
        phase: Phase::Open(Box::new(client)),
        settings: Rc::new(Settings::new(&rv_config::Ws::default(), "fuzz.test")),
        rbuf: ReadBuffer::default(),
        flush_pending: false,
        data_left: 0,
        fragmented: false,
        out: Out::default(),
        read_closed: false,
        close_sent: false,
        write_closed: false,
        heartbeat: None,
        read_error: None,
        text: false,
        text_final: false,
        utf8: Utf8::default(),
    };
    let mut buf = vec![0u8; 4096];
    let mut total = 0;
    for _ in 0..data.len() + 16 {
        let mut rb = ReadBuf::new(&mut buf);
        match Pin::new(&mut ws).poll_read(&mut cx, &mut rb) {
            Poll::Ready(Ok(())) if !rb.filled().is_empty() => total += rb.filled().len(),
            _ => break,
        }
        assert!(total <= data.len());
    }
}
