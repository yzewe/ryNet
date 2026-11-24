use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

use rv_plugin_abi::{err, ev};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::BoxStream;

pub(super) const STREAM_BUF: usize = 256 * 1024;
pub(super) const DGRAM_QUEUE: usize = 512;
pub(super) const MAX_HANDLES: usize = 4096;

pub(super) struct Shared {
    events: RefCell<VecDeque<(u32, u32, u32)>>,
    pub wake: tokio::sync::Notify,
    pub table: RefCell<Table>,
    pub dead: Cell<bool>,
    pub apps: Cell<u32>,
    pub tun: RefCell<Option<Rc<super::tun::Stack>>>,
    pub tun_up: tokio::sync::Notify,
}

impl Shared {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            events: RefCell::new(VecDeque::new()),
            wake: tokio::sync::Notify::new(),
            table: RefCell::new(Table::default()),
            dead: Cell::new(false),
            apps: Cell::new(0),
            tun: RefCell::new(None),
            tun_up: tokio::sync::Notify::new(),
        })
    }

    pub fn push(&self, kind: u32, a: u32, b: u32) {
        if self.dead.get() {
            return;
        }
        self.events.borrow_mut().push_back((kind, a, b));
        self.wake.notify_one();
    }

    pub fn pop(&self) -> Option<(u32, u32, u32)> {
        self.events.borrow_mut().pop_front()
    }

    pub fn get(&self, h: u32) -> Option<Res> {
        self.table.borrow().get(h).cloned()
    }
}

fn push(shared: &Weak<Shared>, kind: u32, h: u32) {
    if let Some(s) = shared.upgrade() {
        s.push(kind, h, 0);
    }
}

#[derive(Clone)]
pub(super) enum Res {
    Stream(Rc<Pipe>),
    Dgram(Rc<Dgram>),
    Quic(Rc<super::net::QuicRes>),
    H2(Rc<super::net::H2Res>),
    Timer(Rc<Task>),
    App(Rc<AppConn>),
    AppUdp(Rc<AppUdp>),
    Info(Rc<Op>),
    Tun(Rc<super::tun::TunRes>),
}

impl Res {
    pub fn op(&self) -> Option<&Op> {
        Some(match self {
            Res::Stream(p) => &p.op,
            Res::Dgram(d) => &d.op,
            Res::Quic(q) => &q.dg.op,
            Res::H2(h) => &h.op,
            Res::Info(o) => o,
            Res::Tun(t) => &t.op,
            Res::Timer(_) | Res::App(_) | Res::AppUdp(_) => return None,
        })
    }

    pub fn close(&self) {
        match self {
            Res::Stream(p) => p.close(),
            Res::Dgram(d) => d.close(),
            Res::Quic(q) => q.close(),
            Res::H2(h) => h.close(),
            Res::Timer(t) => t.abort(),
            Res::App(c) => c.fail("closed by the plugin"),
            Res::AppUdp(u) => u.close("closed by the plugin"),
            Res::Info(_) => {}
            Res::Tun(t) => t.close(),
        }
    }
}

#[derive(Default)]
pub(super) struct Table {
    slots: Vec<(u16, Option<Res>)>,
    free: Vec<usize>,
    live: usize,
}

impl Table {
    pub fn insert(&mut self, r: Res) -> Option<u32> {
        if self.live >= MAX_HANDLES {
            return None;
        }
        let i = match self.free.pop() {
            Some(i) => i,
            None => {
                self.slots.push((0, None));
                self.slots.len() - 1
            }
        };
        let slot = &mut self.slots[i];
        slot.0 = (slot.0 + 1) & 0x7fff;
        if slot.0 == 0 {
            slot.0 = 1;
        }
        slot.1 = Some(r);
        self.live += 1;
        Some((u32::from(slot.0) << 16) | (i as u32 + 1))
    }

    fn slot(&self, h: u32) -> Option<usize> {
        let i = (h & 0xffff) as usize;
        let i = i.checked_sub(1)?;
        let (g, r) = self.slots.get(i)?;
        (u32::from(*g) == h >> 16 && r.is_some()).then_some(i)
    }

    pub fn get(&self, h: u32) -> Option<&Res> {
        self.slots[self.slot(h)?].1.as_ref()
    }

    pub fn remove(&mut self, h: u32) -> Option<Res> {
        let i = self.slot(h)?;
        self.live -= 1;
        self.free.push(i);
        self.slots[i].1.take()
    }

    pub fn drain(&mut self) -> Vec<Res> {
        self.free.clear();
        self.live = 0;
        self.slots.drain(..).filter_map(|(_, r)| r).collect()
    }
}

#[derive(Default)]
pub(super) struct Op {
    status: Cell<i32>,
    error: RefCell<String>,
    pub info: RefCell<Option<Vec<u8>>>,
}

impl Op {
    pub fn pending() -> Self {
        Self { status: Cell::new(err::PENDING), ..Default::default() }
    }

    pub fn status(&self) -> i32 {
        self.status.get()
    }

    pub fn error(&self) -> String {
        self.error.borrow().clone()
    }

    pub fn set_ok(&self) {
        self.status.set(0);
    }

    pub fn set_err(&self, code: i32, msg: impl Into<String>) {
        self.status.set(code);
        *self.error.borrow_mut() = msg.into();
    }
}

pub(super) fn io_code(e: &io::Error) -> i32 {
    match e.kind() {
        io::ErrorKind::TimedOut => err::TIMEOUT,
        io::ErrorKind::PermissionDenied => err::DENIED,
        _ => err::FAILED,
    }
}

pub(super) struct Pipe {
    pub id: Cell<u32>,
    pub op: Op,
    st: RefCell<PipeState>,
    waker: Cell<Option<Waker>>,
    shared: Weak<Shared>,
}

#[derive(Default)]
struct PipeState {
    io: Option<BoxStream>,
    rx: Vec<u8>,
    rx_at: usize,
    eof: bool,
    err: Option<String>,
    tx: Vec<u8>,
    tx_at: usize,
    dirty: bool,
    shut: bool,
    shut_done: bool,
    closed: bool,
    detached: bool,
    want_read: bool,
    want_write: bool,
    full: bool,
}

pub(super) struct Leftover {
    pub io: BoxStream,
    pub read: Vec<u8>,
    pub write: Vec<u8>,
    pub shut: bool,
}

impl Pipe {
    pub fn new(shared: &Rc<Shared>) -> Rc<Self> {
        let p = Rc::new(Self {
            id: Cell::new(0),
            op: Op::pending(),
            st: RefCell::new(PipeState::default()),
            waker: Cell::new(None),
            shared: Rc::downgrade(shared),
        });
        tokio::task::spawn_local(Pump { p: p.clone(), linger: None });
        p
    }

    fn wake(&self) {
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }

    pub fn ready(&self, io: BoxStream) {
        {
            let mut s = self.st.borrow_mut();
            if s.closed {
                return;
            }
            s.io = Some(io);
        }
        self.op.set_ok();
        push(&self.shared, ev::DONE, self.id.get());
        self.wake();
    }

    pub fn notify(&self) {
        push(&self.shared, ev::READABLE, self.id.get());
    }

    pub fn fail(&self, code: i32, msg: String) {
        if self.st.borrow().closed {
            return;
        }
        self.op.set_err(code, msg);
        push(&self.shared, ev::DONE, self.id.get());
    }

    fn not_ready(&self) -> Option<i32> {
        match self.op.status() {
            0 => None,
            err::PENDING => Some(err::AGAIN),
            e => Some(e),
        }
    }

    pub fn read(&self, out: &mut [u8]) -> Result<usize, (i32, String)> {
        if let Some(e) = self.not_ready() {
            return Err((e, self.op.error()));
        }
        let mut s = self.st.borrow_mut();
        let avail = s.rx.len() - s.rx_at;
        if avail > 0 {
            let n = avail.min(out.len());
            out[..n].copy_from_slice(&s.rx[s.rx_at..s.rx_at + n]);
            s.rx_at += n;
            if s.rx_at == s.rx.len() {
                s.rx.clear();
                s.rx_at = 0;
            }
            if std::mem::take(&mut s.full) {
                drop(s);
                self.wake();
            }
            return Ok(n);
        }
        if let Some(e) = &s.err {
            return Err((err::FAILED, e.clone()));
        }
        if s.eof || s.closed {
            return Ok(0);
        }
        s.want_read = true;
        Err((err::AGAIN, String::new()))
    }

    pub fn write(&self, data: &[u8]) -> Result<usize, (i32, String)> {
        if let Some(e) = self.not_ready() {
            return Err((e, self.op.error()));
        }
        let mut s = self.st.borrow_mut();
        if let Some(e) = &s.err {
            return Err((err::FAILED, e.clone()));
        }
        if s.shut || s.closed {
            return Err((err::CLOSED, "write after shutdown".into()));
        }
        let queued = s.tx.len() - s.tx_at;
        let room = STREAM_BUF.saturating_sub(queued);
        if room == 0 {
            s.want_write = true;
            return Err((err::AGAIN, String::new()));
        }
        let n = room.min(data.len());
        s.tx.extend_from_slice(&data[..n]);
        drop(s);
        if queued == 0 {
            self.wake();
        }
        Ok(n)
    }

    pub fn shutdown(&self) {
        self.st.borrow_mut().shut = true;
        self.wake();
    }

    pub fn close(&self) {
        let mut s = self.st.borrow_mut();
        s.closed = true;
        s.rx = Vec::new();
        s.rx_at = 0;
        if s.err.is_some() {
            s.io = None;
        }
        drop(s);
        self.wake();
    }

    pub fn detach(&self) -> Result<Leftover, (i32, String)> {
        if let Some(e) = self.not_ready() {
            return Err((if e == err::AGAIN { err::PENDING } else { e }, self.op.error()));
        }
        let mut s = self.st.borrow_mut();
        if let Some(e) = &s.err {
            return Err((err::FAILED, e.clone()));
        }
        let Some(io) = s.io.take() else { return Err((err::CLOSED, "stream is closed".into())) };
        s.detached = true;
        let (ra, ta) = (s.rx_at, s.tx_at);
        let read = s.rx.split_off(ra);
        let write = s.tx.split_off(ta);
        let shut = s.shut && !s.shut_done;
        drop(s);
        self.wake();
        Ok(Leftover { io, read, write, shut })
    }
}

struct Pump {
    p: Rc<Pipe>,
    linger: Option<Pin<Box<tokio::time::Sleep>>>,
}

fn write_side(s: &mut PipeState, io: &mut BoxStream, cx: &mut Context<'_>) {
    while s.tx_at < s.tx.len() && s.err.is_none() {
        match Pin::new(&mut *io).poll_write(cx, &s.tx[s.tx_at..]) {
            Poll::Ready(Ok(0)) => s.err = Some("connection closed while writing".into()),
            Poll::Ready(Ok(n)) => {
                s.tx_at += n;
                s.dirty = true;
            }
            Poll::Ready(Err(e)) => s.err = Some(e.to_string()),
            Poll::Pending => break,
        }
    }
    if s.tx_at == s.tx.len() {
        s.tx.clear();
        s.tx_at = 0;
    } else if s.tx_at > STREAM_BUF / 2 {
        s.tx.drain(..s.tx_at);
        s.tx_at = 0;
    }
    if s.dirty && s.tx.is_empty() && s.err.is_none() {
        match Pin::new(&mut *io).poll_flush(cx) {
            Poll::Ready(Ok(())) => s.dirty = false,
            Poll::Ready(Err(e)) => s.err = Some(e.to_string()),
            Poll::Pending => {}
        }
    }
    if s.shut && !s.shut_done && s.tx.is_empty() && !s.dirty && s.err.is_none() && Pin::new(&mut *io).poll_shutdown(cx).is_ready() {
        s.shut_done = true;
    }
}

impl Future for Pump {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let me = &mut *self;
        let p = &me.p;
        let (mut readable, mut writable) = (false, false);
        {
            let mut g = p.st.borrow_mut();
            let s = &mut *g;
            if s.detached {
                return Poll::Ready(());
            }
            if s.closed {
                let Some(mut io) = s.io.take() else { return Poll::Ready(()) };
                let linger = me.linger.get_or_insert_with(|| Box::pin(tokio::time::sleep(std::time::Duration::from_secs(10))));
                s.shut = true;
                write_side(s, &mut io, cx);
                if s.shut_done || s.err.is_some() || linger.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(());
                }
                s.io = Some(io);
                return Poll::Pending;
            }
            if let Some(mut io) = s.io.take() {
                write_side(s, &mut io, cx);
                if s.want_write && (s.err.is_some() || s.tx.len() - s.tx_at < STREAM_BUF / 2) {
                    s.want_write = false;
                    writable = true;
                }
                while !s.eof && s.err.is_none() {
                    let avail = s.rx.len() - s.rx_at;
                    if avail >= STREAM_BUF {
                        s.full = true;
                        break;
                    }
                    if s.rx_at > 0 && s.rx_at * 2 >= s.rx.len() {
                        s.rx.drain(..s.rx_at);
                        s.rx_at = 0;
                    }
                    let len = s.rx.len();
                    let want = (STREAM_BUF - avail).min(32 * 1024);
                    s.rx.resize(len + want, 0);
                    let mut rb = ReadBuf::new(&mut s.rx[len..]);
                    let r = Pin::new(&mut io).poll_read(cx, &mut rb);
                    let n = rb.filled().len();
                    s.rx.truncate(len + n);
                    match r {
                        Poll::Ready(Ok(())) if n == 0 => s.eof = true,
                        Poll::Ready(Ok(())) => readable = true,
                        Poll::Ready(Err(e)) => s.err = Some(e.to_string()),
                        Poll::Pending => break,
                    }
                }
                if s.eof || s.err.is_some() {
                    readable = true;
                }
                readable = readable && std::mem::take(&mut s.want_read);
                s.io = Some(io);
            }
            p.waker.set(Some(cx.waker().clone()));
        }
        if readable {
            push(&p.shared, ev::READABLE, p.id.get());
        }
        if writable {
            push(&p.shared, ev::WRITABLE, p.id.get());
        }
        Poll::Pending
    }
}

pub(crate) struct Prefixed {
    io: BoxStream,
    read: Vec<u8>,
    read_at: usize,
    write: Vec<u8>,
    write_at: usize,
    shut: bool,
}

impl Prefixed {
    pub fn new(l: Leftover) -> Self {
        Self { io: l.io, read: l.read, read_at: 0, write: l.write, write_at: 0, shut: l.shut }
    }

    pub fn boxed(l: Leftover) -> BoxStream {
        if l.read.is_empty() && l.write.is_empty() && !l.shut { l.io } else { Box::new(Self::new(l)) }
    }

    fn poll_owed(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.write_at < self.write.len() {
            let n = std::task::ready!(Pin::new(&mut self.io).poll_write(cx, &self.write[self.write_at..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.write_at += n;
            if self.write_at == self.write.len() {
                self.write = Vec::new();
                self.write_at = 0;
                std::task::ready!(Pin::new(&mut self.io).poll_flush(cx))?;
            }
        }
        if self.shut {
            std::task::ready!(Pin::new(&mut self.io).poll_shutdown(cx))?;
            self.shut = false;
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for Prefixed {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        if me.read_at < me.read.len() {
            let n = (me.read.len() - me.read_at).min(buf.remaining());
            buf.put_slice(&me.read[me.read_at..me.read_at + n]);
            me.read_at += n;
            if me.read_at == me.read.len() {
                me.read = Vec::new();
                me.read_at = 0;
            }
            return Poll::Ready(Ok(()));
        }
        if let Poll::Ready(Err(e)) = me.poll_owed(cx) {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut me.io).poll_read(cx, buf)
    }
}

impl AsyncWrite for Prefixed {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        std::task::ready!(self.poll_owed(cx))?;
        Pin::new(&mut self.io).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        std::task::ready!(self.poll_owed(cx))?;
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        std::task::ready!(self.poll_owed(cx))?;
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

pub(super) struct Dgram {
    pub id: Cell<u32>,
    pub op: Op,
    pub rx: RefCell<VecDeque<Vec<u8>>>,
    pub want_read: Cell<bool>,
    pub want_write: Cell<bool>,
    pub sock: RefCell<Option<Rc<tokio::net::UdpSocket>>>,
    pub quic: RefCell<Option<quinn::Connection>>,
    pub via: RefCell<Option<Rc<super::outbound::DgramLink>>>,
    tasks: RefCell<Vec<tokio::task::AbortHandle>>,
    shared: Weak<Shared>,
}

impl Dgram {
    pub fn new(shared: &Rc<Shared>) -> Rc<Self> {
        Rc::new(Self {
            id: Cell::new(0),
            op: Op::pending(),
            rx: RefCell::new(VecDeque::new()),
            want_read: Cell::new(false),
            want_write: Cell::new(false),
            sock: RefCell::new(None),
            quic: RefCell::new(None),
            via: RefCell::new(None),
            tasks: RefCell::new(Vec::new()),
            shared: Rc::downgrade(shared),
        })
    }

    pub fn add_task(&self, t: tokio::task::AbortHandle) {
        self.tasks.borrow_mut().push(t);
    }

    pub fn done(&self) {
        push(&self.shared, ev::DONE, self.id.get());
    }

    pub fn deliver(&self, d: Vec<u8>) {
        let mut rx = self.rx.borrow_mut();
        if rx.len() >= DGRAM_QUEUE {
            return;
        }
        rx.push_back(d);
        drop(rx);
        if self.want_read.replace(false) {
            push(&self.shared, ev::READABLE, self.id.get());
        }
    }

    pub fn ended(&self, msg: String) {
        if self.op.status() == 0 {
            self.op.set_err(err::CLOSED, msg);
        }
        if self.want_read.replace(false) {
            push(&self.shared, ev::READABLE, self.id.get());
        }
        push(&self.shared, ev::CLOSED, self.id.get());
    }

    pub fn recv(&self, out: &mut [u8]) -> Result<usize, (i32, String)> {
        match self.op.status() {
            0 => {}
            err::PENDING => return Err((err::AGAIN, String::new())),
            e => {
                if self.rx.borrow().is_empty() {
                    return Err((e, self.op.error()));
                }
            }
        }
        let mut rx = self.rx.borrow_mut();
        let Some(d) = rx.front() else {
            self.want_read.set(true);
            return Err((err::AGAIN, String::new()));
        };
        if d.len() > out.len() {
            return Err((err::TOO_SMALL, format!("datagram of {} bytes", d.len())));
        }
        let d = rx.pop_front().unwrap();
        out[..d.len()].copy_from_slice(&d);
        Ok(d.len())
    }

    pub fn send(&self, data: &[u8]) -> Result<usize, (i32, String)> {
        match self.op.status() {
            0 => {}
            err::PENDING => return Err((err::AGAIN, String::new())),
            e => return Err((e, self.op.error())),
        }
        if let Some(s) = self.sock.borrow().as_ref() {
            return match s.try_send(data) {
                Ok(n) => Ok(n),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.want_write.set(true);
                    let (s, me) = (s.clone(), self.shared.clone());
                    let id = self.id.get();
                    tokio::task::spawn_local(async move {
                        let _ = s.writable().await;
                        push(&me, ev::WRITABLE, id);
                    });
                    Err((err::AGAIN, String::new()))
                }
                Err(e) => Err((io_code(&e), e.to_string())),
            };
        }
        if let Some(l) = self.via.borrow().as_ref() {
            l.send(data);
            return Ok(data.len());
        }
        if let Some(q) = self.quic.borrow().as_ref() {
            return match q.send_datagram(bytes::Bytes::copy_from_slice(data)) {
                Ok(()) => Ok(data.len()),
                Err(quinn::SendDatagramError::TooLarge) => {
                    Err((err::TOO_SMALL, format!("datagram over {} bytes", q.max_datagram_size().unwrap_or(0))))
                }
                Err(e) => Err((err::FAILED, e.to_string())),
            };
        }
        Err((err::INVALID, "not a datagram handle".into()))
    }

    pub fn close(&self) {
        for t in self.tasks.borrow_mut().drain(..) {
            t.abort();
        }
        self.sock.borrow_mut().take();
        self.quic.borrow_mut().take();
        self.via.borrow_mut().take();
    }
}

pub(super) struct Task(pub tokio::task::AbortHandle);

impl Task {
    pub fn abort(&self) {
        self.0.abort();
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) struct AppConn {
    pub id: Cell<u32>,
    pub target: Vec<u8>,
    st: RefCell<AppState>,
    shared: Weak<Shared>,
}

#[derive(Default)]
struct AppState {
    up: Vec<u8>,
    up_at: usize,
    up_eof: bool,
    down: Vec<u8>,
    down_at: usize,
    down_eof: bool,
    err: Option<String>,
    ready: Option<Result<(), String>>,
    direct: Option<BoxStream>,
    want_read: bool,
    want_write: bool,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    ready_waker: Option<Waker>,
}

impl AppState {
    fn wake_all(&mut self) {
        for w in [self.read_waker.take(), self.write_waker.take(), self.ready_waker.take()].into_iter().flatten() {
            w.wake();
        }
    }
}

impl AppConn {
    pub fn new(shared: &Rc<Shared>, target: Vec<u8>) -> Rc<Self> {
        Rc::new(Self { id: Cell::new(0), target, st: RefCell::new(AppState::default()), shared: Rc::downgrade(shared) })
    }

    pub fn accept(&self) {
        let mut s = self.st.borrow_mut();
        if s.ready.is_none() {
            s.ready = Some(Ok(()));
        }
        s.wake_all();
    }

    pub fn fail(&self, msg: &str) {
        let mut s = self.st.borrow_mut();
        if s.direct.is_some() {
            return;
        }
        if s.ready.is_none() {
            s.ready = Some(Err(msg.to_string()));
        }
        if s.err.is_none() && !s.down_eof {
            s.err = Some(msg.to_string());
        }
        s.wake_all();
    }

    pub fn splice(&self, mut l: Leftover) {
        let mut s = self.st.borrow_mut();
        let at = s.up_at;
        l.write.extend_from_slice(&s.up[at..]);
        s.up = Vec::new();
        s.up_at = 0;
        l.shut |= s.up_eof;
        s.direct = Some(Prefixed::boxed(l));
        if s.ready.is_none() {
            s.ready = Some(Ok(()));
        }
        s.wake_all();
    }

    pub fn read(&self, out: &mut [u8]) -> Result<usize, (i32, String)> {
        let mut s = self.st.borrow_mut();
        let avail = s.up.len() - s.up_at;
        if avail > 0 {
            let n = avail.min(out.len());
            out[..n].copy_from_slice(&s.up[s.up_at..s.up_at + n]);
            s.up_at += n;
            if s.up_at == s.up.len() {
                s.up.clear();
                s.up_at = 0;
            }
            if let Some(w) = s.write_waker.take() {
                w.wake();
            }
            return Ok(n);
        }
        if s.up_eof {
            return Ok(0);
        }
        if let Some(e) = &s.err {
            return Err((err::CLOSED, e.clone()));
        }
        s.want_read = true;
        Err((err::AGAIN, String::new()))
    }

    pub fn write(&self, data: &[u8]) -> Result<usize, (i32, String)> {
        let mut s = self.st.borrow_mut();
        if s.err.is_some() || s.down_eof {
            return Err((err::CLOSED, "the application closed the connection".into()));
        }
        let queued = s.down.len() - s.down_at;
        let room = STREAM_BUF.saturating_sub(queued);
        if room == 0 {
            s.want_write = true;
            return Err((err::AGAIN, String::new()));
        }
        let n = room.min(data.len());
        s.down.extend_from_slice(&data[..n]);
        if let Some(w) = s.read_waker.take() {
            w.wake();
        }
        Ok(n)
    }

    pub fn shutdown(&self) {
        let mut s = self.st.borrow_mut();
        s.down_eof = true;
        if let Some(w) = s.read_waker.take() {
            w.wake();
        }
    }

    pub async fn wait_ready(&self) -> Result<(), String> {
        std::future::poll_fn(|cx| {
            let mut s = self.st.borrow_mut();
            match &s.ready {
                Some(r) => Poll::Ready(r.clone()),
                None => {
                    s.ready_waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await
    }
}

pub(super) struct AppStream {
    pub conn: Rc<AppConn>,
}

impl AppStream {
    fn signal(&self, kind: u32) {
        push(&self.conn.shared, kind, self.conn.id.get());
    }
}

impl Drop for AppStream {
    fn drop(&mut self) {
        let direct = self.conn.st.borrow_mut().direct.take();
        drop(direct);
        if let Some(s) = self.conn.shared.upgrade() {
            s.apps.set(s.apps.get().saturating_sub(1));
            let id = self.conn.id.get();
            let gone = s.table.borrow_mut().remove(id).is_some();
            if gone {
                s.push(ev::CLOSED, id, 0);
            }
        }
    }
}

impl AsyncRead for AppStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let mut writable = false;
        let r = {
            let mut g = self.conn.st.borrow_mut();
            let s = &mut *g;
            if s.down_at < s.down.len() {
                let n = (s.down.len() - s.down_at).min(buf.remaining());
                buf.put_slice(&s.down[s.down_at..s.down_at + n]);
                s.down_at += n;
                if s.down_at == s.down.len() {
                    s.down.clear();
                    s.down_at = 0;
                }
                writable = std::mem::take(&mut s.want_write);
                Poll::Ready(Ok(()))
            } else if let Some(d) = s.direct.as_mut() {
                Pin::new(d).poll_read(cx, buf)
            } else if s.down_eof {
                Poll::Ready(Ok(()))
            } else if let Some(e) = &s.err {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, e.clone())))
            } else {
                s.read_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        };
        if writable {
            self.signal(ev::WRITABLE);
        }
        r
    }
}

impl AsyncWrite for AppStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let mut readable = false;
        let r = {
            let mut g = self.conn.st.borrow_mut();
            let s = &mut *g;
            if let Some(d) = s.direct.as_mut() {
                Pin::new(d).poll_write(cx, data)
            } else if let Some(e) = &s.err {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, e.clone())))
            } else if s.up.len() - s.up_at >= STREAM_BUF {
                s.write_waker = Some(cx.waker().clone());
                Poll::Pending
            } else {
                let n = (STREAM_BUF - (s.up.len() - s.up_at)).min(data.len());
                s.up.extend_from_slice(&data[..n]);
                readable = std::mem::take(&mut s.want_read);
                Poll::Ready(Ok(n))
            }
        };
        if readable {
            self.signal(ev::READABLE);
        }
        r
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.conn.st.borrow_mut().direct.as_mut() {
            Some(d) => Pin::new(d).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let readable = {
            let mut s = self.conn.st.borrow_mut();
            if let Some(d) = s.direct.as_mut() {
                return Pin::new(d).poll_shutdown(cx);
            }
            s.up_eof = true;
            std::mem::take(&mut s.want_read)
        };
        if readable {
            self.signal(ev::READABLE);
        }
        Poll::Ready(Ok(()))
    }
}

impl crate::pipe::Local for AppStream {}

pub(super) struct AppUdp {
    pub id: Cell<u32>,
    pub target: RefCell<Vec<u8>>,
    pub rx: RefCell<VecDeque<Vec<u8>>>,
    pub tx: RefCell<VecDeque<(SocketAddr, Vec<u8>)>>,
    pub closed: RefCell<Option<String>>,
    pub want_read: Cell<bool>,
    pub notify: tokio::sync::Notify,
    shared: Weak<Shared>,
}

impl AppUdp {
    pub fn new(shared: &Rc<Shared>) -> Rc<Self> {
        Rc::new(Self {
            id: Cell::new(0),
            target: RefCell::new(Vec::new()),
            rx: RefCell::new(VecDeque::new()),
            tx: RefCell::new(VecDeque::new()),
            closed: RefCell::new(None),
            want_read: Cell::new(false),
            notify: tokio::sync::Notify::new(),
            shared: Rc::downgrade(shared),
        })
    }

    pub fn deliver(&self, dst: SocketAddr, data: &[u8]) {
        let mut rx = self.rx.borrow_mut();
        if rx.len() >= DGRAM_QUEUE {
            return;
        }
        let mut d = Vec::with_capacity(19 + data.len());
        put_socket_addr(&mut d, dst);
        d.extend_from_slice(data);
        rx.push_back(d);
        drop(rx);
        if self.want_read.replace(false) {
            push(&self.shared, ev::READABLE, self.id.get());
        }
    }

    pub fn recv(&self, out: &mut [u8]) -> Result<usize, (i32, String)> {
        let mut rx = self.rx.borrow_mut();
        let Some(d) = rx.front() else {
            if let Some(e) = self.closed.borrow().as_ref() {
                return Err((err::CLOSED, e.clone()));
            }
            self.want_read.set(true);
            return Err((err::AGAIN, String::new()));
        };
        if d.len() > out.len() {
            return Err((err::TOO_SMALL, format!("datagram of {} bytes", d.len())));
        }
        let d = rx.pop_front().unwrap();
        out[..d.len()].copy_from_slice(&d);
        Ok(d.len())
    }

    pub fn send(&self, data: &[u8]) -> Result<usize, (i32, String)> {
        if let Some(e) = self.closed.borrow().as_ref() {
            return Err((err::CLOSED, e.clone()));
        }
        let (src, n) =
            take_socket_addr(data).ok_or((err::INVALID, "datagram needs a source address (type 1 or 4) in front".to_string()))?;
        let mut tx = self.tx.borrow_mut();
        if tx.len() >= DGRAM_QUEUE {
            return Err((err::AGAIN, String::new()));
        }
        tx.push_back((src, data[n..].to_vec()));
        drop(tx);
        self.notify.notify_one();
        Ok(data.len())
    }

    pub fn close(&self, why: &str) {
        self.closed.borrow_mut().get_or_insert_with(|| why.to_string());
        self.notify.notify_one();
    }
}

pub(super) fn put_socket_addr(o: &mut Vec<u8>, a: SocketAddr) {
    match a {
        SocketAddr::V4(v) => {
            o.push(1);
            o.extend_from_slice(&v.ip().octets());
        }
        SocketAddr::V6(v) => match v.ip().to_ipv4_mapped() {
            Some(m) => {
                o.push(1);
                o.extend_from_slice(&m.octets());
            }
            None => {
                o.push(4);
                o.extend_from_slice(&v.ip().octets());
            }
        },
    }
    o.extend_from_slice(&a.port().to_be_bytes());
}

pub(super) fn take_socket_addr(d: &[u8]) -> Option<(SocketAddr, usize)> {
    match *d.first()? {
        1 if d.len() >= 7 => {
            let ip: [u8; 4] = d[1..5].try_into().ok()?;
            Some((SocketAddr::from((ip, u16::from_be_bytes([d[5], d[6]]))), 7))
        }
        4 if d.len() >= 19 => {
            let ip: [u8; 16] = d[1..17].try_into().ok()?;
            Some((SocketAddr::from((ip, u16::from_be_bytes([d[17], d[18]]))), 19))
        }
        _ => None,
    }
}

pub(super) fn encode_target(dest: &rv_vless::Address, port: u16) -> Vec<u8> {
    let mut o = Vec::with_capacity(20);
    crate::outbound::trojan::put_addr(&mut o, dest, port);
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_not_reused_with_the_same_number() {
        let mut t = Table::default();
        let a = t.insert(Res::Info(Rc::new(Op::default()))).unwrap();
        assert!(t.remove(a).is_some());
        let b = t.insert(Res::Info(Rc::new(Op::default()))).unwrap();
        assert_ne!(a, b, "same slot, new generation");
        assert_eq!(a & 0xffff, b & 0xffff);
        assert!(t.get(a).is_none(), "an old handle does not reach the new resource");
        assert!(t.get(b).is_some());
        assert!(t.remove(0).is_none() && t.remove(u32::MAX).is_none());
        assert!(i32::try_from(b).is_ok(), "handles stay positive i32");
    }

    #[test]
    fn handle_table_is_bounded() {
        let mut t = Table::default();
        for _ in 0..MAX_HANDLES {
            t.insert(Res::Info(Rc::new(Op::default()))).unwrap();
        }
        assert!(t.insert(Res::Info(Rc::new(Op::default()))).is_none());
    }

    #[test]
    fn datagram_addresses_round_trip() {
        for a in ["1.2.3.4:53", "[2001:db8::1]:443", "[::ffff:5.6.7.8]:80"] {
            let a: SocketAddr = a.parse().unwrap();
            let mut o = Vec::new();
            put_socket_addr(&mut o, a);
            o.extend_from_slice(b"x");
            let (b, n) = take_socket_addr(&o).unwrap();
            assert_eq!(&o[n..], b"x");
            assert_eq!(b.port(), a.port());
            assert_eq!(rv_net_canon(b.ip()), rv_net_canon(a.ip()));
        }
        assert!(take_socket_addr(&[3, 1, b'a', 0, 80]).is_none(), "domains are not datagram sources");
    }

    fn rv_net_canon(ip: std::net::IpAddr) -> std::net::IpAddr {
        ip.to_canonical()
    }

    #[tokio::test]
    async fn prefixed_stream_replays_leftovers_in_order() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (a, mut b) = tokio::io::duplex(1024);
        let mut p = Prefixed::new(Leftover { io: Box::new(a), read: b"early".to_vec(), write: b"head".to_vec(), shut: false });
        p.write_all(b"+body").await.unwrap();
        p.flush().await.unwrap();
        let mut got = [0u8; 9];
        b.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"head+body");
        b.write_all(b"-late").await.unwrap();
        let mut got = [0u8; 10];
        p.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"early-late");
    }
}
