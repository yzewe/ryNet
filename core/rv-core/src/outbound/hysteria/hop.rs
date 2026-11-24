use std::fmt;
use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::time::Instant;

use quinn::udp::{RecvMeta, Transmit};
use quinn::{AsyncUdpSocket, UdpPoller};

pub type Make = Box<dyn Fn() -> io::Result<Arc<dyn AsyncUdpSocket>> + Send + Sync>;

pub struct Hop {
    server: SocketAddr,
    ports: Vec<u16>,
    interval: (u32, u32),
    make: Make,
    state: Mutex<State>,
}

struct State {
    cur: Arc<dyn AsyncUdpSocket>,
    dest: SocketAddr,
    prev: Option<(Arc<dyn AsyncUdpSocket>, Instant)>,
    next: Instant,
    epoch: u64,
    recv: Option<Waker>,
    write: Option<Waker>,
}

impl fmt::Debug for Hop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hop").field("server", &self.server).finish_non_exhaustive()
    }
}

fn pick(ports: &[u16]) -> u16 {
    ports[super::proto::random_u32() as usize % ports.len()]
}

fn delay((min, max): (u32, u32)) -> Duration {
    let (min, max) = (if min == 0 { 30 } else { min }, if max == 0 { 30 } else { max });
    debug_assert!(min >= 5 && max >= min);
    let base = u64::from(min) * 1_000_000_000;
    let span = u64::from(max - min) * 1_000_000_000 + 1;
    let random = u64::from(super::proto::random_u32()) << 32 | u64::from(super::proto::random_u32());
    Duration::from_nanos(base + random % span)
}

impl Hop {
    pub fn new(server: SocketAddr, ports: Vec<u16>, interval: (u32, u32), first: Arc<dyn AsyncUdpSocket>, make: Make) -> Arc<Self> {
        Arc::new(Self {
            server,
            state: Mutex::new(State {
                cur: first,
                dest: server,
                prev: None,
                next: Instant::now() + delay(interval),
                epoch: 0,
                recv: None,
                write: None,
            }),
            ports,
            interval,
            make,
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn maybe_hop(&self) {
        let mut s = self.state();
        let now = Instant::now();
        if s.prev.as_ref().is_some_and(|(_, until)| now >= *until) {
            s.prev = None;
        }
        if now < s.next {
            return;
        }
        s.next = now + delay(self.interval);
        match (self.make)() {
            Ok(new) => {
                s.prev = Some((std::mem::replace(&mut s.cur, new), s.next));
                s.dest = SocketAddr::new(self.server.ip(), pick(&self.ports));
                s.epoch += 1;
                tracing::debug!("hysteria: hop to port {}", s.dest.port());
                if let Some(w) = s.recv.take() {
                    w.wake();
                }
                if let Some(w) = s.write.take() {
                    w.wake();
                }
            }
            Err(e) => tracing::debug!("hysteria: hop socket: {e}"),
        }
    }
}

impl AsyncUdpSocket for Hop {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Box::pin(Poller { hop: self, inner: None })
    }

    fn try_send(&self, t: &Transmit) -> io::Result<()> {
        self.maybe_hop();
        let (cur, dest) = {
            let s = self.state();
            (s.cur.clone(), s.dest)
        };
        cur.try_send(&Transmit { destination: dest, ecn: t.ecn, contents: t.contents, segment_size: t.segment_size, src_ip: t.src_ip })
    }

    fn poll_recv(&self, cx: &mut Context, bufs: &mut [IoSliceMut<'_>], meta: &mut [RecvMeta]) -> Poll<io::Result<usize>> {
        let (cur, prev) = {
            let mut s = self.state();
            if s.prev.as_ref().is_some_and(|(_, until)| Instant::now() >= *until) {
                s.prev = None;
            }
            s.recv = Some(cx.waker().clone());
            (s.cur.clone(), s.prev.as_ref().map(|(p, _)| p.clone()))
        };
        let n = match cur.poll_recv(cx, bufs, meta) {
            Poll::Ready(Ok(n)) => n,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => match prev.map(|p| p.poll_recv(cx, bufs, meta)) {
                Some(Poll::Ready(Ok(n))) => n,
                Some(Poll::Ready(Err(_))) => {
                    self.state().prev = None;
                    return Poll::Pending;
                }
                _ => return Poll::Pending,
            },
        };
        for m in meta.iter_mut().take(n) {
            m.addr = self.server;
        }
        Poll::Ready(Ok(n))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.state().cur.local_addr()
    }

    fn may_fragment(&self) -> bool {
        self.state().cur.may_fragment()
    }

    fn max_transmit_segments(&self) -> usize {
        self.state().cur.max_transmit_segments()
    }

    fn max_receive_segments(&self) -> usize {
        self.state().cur.max_receive_segments()
    }
}

struct Poller {
    hop: Arc<Hop>,
    inner: Option<(u64, Pin<Box<dyn UdpPoller>>)>,
}

impl fmt::Debug for Poller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HopPoller").finish_non_exhaustive()
    }
}

impl UdpPoller for Poller {
    fn poll_writable(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let (epoch, cur) = {
            let mut s = self.hop.state();
            s.write = Some(cx.waker().clone());
            (s.epoch, s.cur.clone())
        };
        if self.inner.as_ref().is_none_or(|(e, _)| *e != epoch) {
            self.inner = Some((epoch, cur.create_io_poller()));
        }
        self.inner.as_mut().unwrap().1.as_mut().poll_writable(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct Socket;
    #[derive(Debug)]
    struct PendingPoller;
    #[derive(Default)]
    struct Wakes(AtomicUsize);
    impl std::task::Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl UdpPoller for PendingPoller {
        fn poll_writable(self: Pin<&mut Self>, _: &mut Context) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }
    impl AsyncUdpSocket for Socket {
        fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
            Box::pin(PendingPoller)
        }
        fn try_send(&self, _: &Transmit) -> io::Result<()> {
            Ok(())
        }
        fn poll_recv(&self, _: &mut Context, _: &mut [IoSliceMut<'_>], _: &mut [RecvMeta]) -> Poll<io::Result<usize>> {
            Poll::Pending
        }
        fn local_addr(&self) -> io::Result<SocketAddr> {
            Ok("127.0.0.1:40000".parse().unwrap())
        }
    }
    #[tokio::test(start_paused = true)]
    async fn idle_does_not_hop_or_spawn_a_timer() {
        let made = Arc::new(AtomicUsize::new(0));
        let count = made.clone();
        let tasks = tokio::runtime::Handle::current().metrics().num_alive_tasks();
        let hop = Hop::new(
            "127.0.0.1:50000".parse().unwrap(),
            vec![50000, 50001],
            (5, 5),
            Arc::new(Socket),
            Box::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(Socket))
            }),
        );
        let weak = Arc::downgrade(&hop);
        let wakes = Arc::new(Wakes::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        assert!(hop.poll_recv(&mut cx, &mut [], &mut []).is_pending());
        assert!(hop.clone().create_io_poller().as_mut().poll_writable(&mut cx).is_pending());
        tokio::task::yield_now().await;
        assert_eq!(tokio::runtime::Handle::current().metrics().num_alive_tasks(), tasks);
        tokio::time::advance(Duration::from_secs(3600)).await;
        tokio::task::yield_now().await;
        assert_eq!(made.load(Ordering::SeqCst), 0);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
        assert!(hop.poll_recv(&mut cx, &mut [], &mut []).is_pending());
        assert!(hop.clone().create_io_poller().as_mut().poll_writable(&mut cx).is_pending());
        assert_eq!(made.load(Ordering::SeqCst), 0);
        hop.try_send(&Transmit { destination: hop.server, ecn: None, contents: b"next", segment_size: None, src_ip: None }).unwrap();
        assert_eq!(made.load(Ordering::SeqCst), 1);
        assert!(hop.state().prev.is_some());
        let sent_wakes = wakes.0.load(Ordering::SeqCst);
        assert!(sent_wakes > 0);
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert_eq!(wakes.0.load(Ordering::SeqCst), sent_wakes);
        assert_eq!(made.load(Ordering::SeqCst), 1);
        drop(hop);
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        assert!(weak.upgrade().is_none());
        assert_eq!(made.load(Ordering::SeqCst), 1);
    }

    #[derive(Debug, Default)]
    struct PacketSocket {
        packet: AtomicUsize,
        pollers: AtomicUsize,
        sent: Mutex<Vec<SocketAddr>>,
    }
    impl AsyncUdpSocket for PacketSocket {
        fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
            self.pollers.fetch_add(1, Ordering::SeqCst);
            Box::pin(PendingPoller)
        }
        fn try_send(&self, t: &Transmit) -> io::Result<()> {
            self.sent.lock().unwrap().push(t.destination);
            Ok(())
        }
        fn poll_recv(&self, _: &mut Context, bufs: &mut [IoSliceMut<'_>], meta: &mut [RecvMeta]) -> Poll<io::Result<usize>> {
            if self.packet.swap(0, Ordering::SeqCst) == 0 {
                return Poll::Pending;
            }
            bufs[0][0] = 42;
            meta[0] = RecvMeta { addr: "127.0.0.1:50001".parse().unwrap(), len: 1, stride: 1, ..RecvMeta::default() };
            Poll::Ready(Ok(1))
        }
        fn local_addr(&self) -> io::Result<SocketAddr> {
            Ok("127.0.0.1:40000".parse().unwrap())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn late_packets_grace_and_write_poller_survive_hop() {
        let first = Arc::new(PacketSocket::default());
        let next = Arc::new(PacketSocket::default());
        let socket = next.clone();
        let hop = Hop::new("127.0.0.1:50000".parse().unwrap(), vec![50001], (5, 5), first.clone(), Box::new(move || Ok(socket.clone())));
        let mut cx = Context::from_waker(Waker::noop());
        let mut poller = hop.clone().create_io_poller();
        assert!(poller.as_mut().poll_writable(&mut cx).is_pending());
        assert_eq!(first.pollers.load(Ordering::SeqCst), 1);
        tokio::time::advance(Duration::from_secs(5)).await;
        hop.try_send(&Transmit { destination: hop.server, ecn: None, contents: b"hop", segment_size: None, src_ip: None }).unwrap();
        assert!(poller.as_mut().poll_writable(&mut cx).is_pending());
        assert_eq!(next.pollers.load(Ordering::SeqCst), 1);
        assert_eq!(*next.sent.lock().unwrap(), vec!["127.0.0.1:50001".parse::<SocketAddr>().unwrap()]);
        let mut data = [0; 16];
        let mut meta = [RecvMeta::default()];
        first.packet.store(1, Ordering::SeqCst);
        assert!(matches!(hop.poll_recv(&mut cx, &mut [IoSliceMut::new(&mut data)], &mut meta), Poll::Ready(Ok(1))));
        assert_eq!(data[0], 42);
        assert_eq!(meta[0].addr, hop.server);
        tokio::time::advance(Duration::from_secs(5)).await;
        first.packet.store(1, Ordering::SeqCst);
        let old = Arc::downgrade(&first);
        drop(first);
        assert!(hop.poll_recv(&mut cx, &mut [IoSliceMut::new(&mut data)], &mut meta).is_pending());
        assert!(hop.state().prev.is_none());
        assert_eq!(hop.state().epoch, 1);
        assert!(old.upgrade().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn failed_rebind_keeps_socket_and_retries_only_on_due_send() {
        let first = Arc::new(PacketSocket::default());
        let attempts = Arc::new(AtomicUsize::new(0));
        let made = attempts.clone();
        let hop = Hop::new(
            "127.0.0.1:50000".parse().unwrap(),
            vec![50000],
            (5, 5),
            first.clone(),
            Box::new(move || {
                made.fetch_add(1, Ordering::SeqCst);
                Err(io::Error::other("test bind failure"))
            }),
        );
        let send =
            || hop.try_send(&Transmit { destination: hop.server, ecn: None, contents: b"data", segment_size: None, src_ip: None }).unwrap();
        tokio::time::advance(Duration::from_secs(5)).await;
        send();
        send();
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(first.sent.lock().unwrap().len(), 2);
        assert_eq!(hop.state().epoch, 0);
        tokio::time::advance(Duration::from_secs(5)).await;
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        send();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn default_and_ranged_delay() {
        assert_eq!(delay((0, 0)), Duration::from_secs(30));
        assert_eq!(delay((5, 5)), Duration::from_secs(5));
        for _ in 0..32 {
            assert!((Duration::from_secs(5)..=Duration::from_secs(10)).contains(&delay((5, 10))));
        }
    }
}
