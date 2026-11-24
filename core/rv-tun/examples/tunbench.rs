#[cfg(any(target_os = "linux", target_os = "android"))]
fn main() {
    linux::main();
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn main() {
    eprintln!("tunbench: только Linux");
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod linux {
    use std::io::{self, Read, Write};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
    use std::process::Command;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use rv_tun::{BoxStream, DnsRoute, Handler, LocalBoxFuture, TcpConn, UdpReply};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{mpsc, oneshot, watch};

    const LOCAL: Ipv4Addr = Ipv4Addr::new(172, 19, 0, 1);
    const PEER: Ipv4Addr = Ipv4Addr::new(172, 19, 0, 2);
    const REMOTE: Ipv4Addr = Ipv4Addr::new(198, 18, 0, 1);

    const SINK: u16 = 9001;
    const SOURCE: u16 = 9002;
    const RR: u16 = 9003;
    const ECHO: u16 = 9004;
    const PACED: u16 = 9005;
    const UDP_ECHO: u16 = 9006;
    const SMALL: u16 = 9007;
    const SMALL_PACE: Duration = Duration::from_micros(800);
    const PACE: Duration = Duration::from_micros(12_500);

    #[derive(Clone, Copy, Default, Debug)]
    struct Usage {
        user: f64,
        sys: f64,
        vcsw: i64,
        ivcsw: i64,
    }

    impl Usage {
        fn thread() -> Self {
            let mut u: libc::rusage = unsafe { std::mem::zeroed() };
            unsafe { libc::getrusage(1, &mut u) };
            let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
            Usage { user: t(u.ru_utime), sys: t(u.ru_stime), vcsw: u.ru_nvcsw, ivcsw: u.ru_nivcsw }
        }
        fn since(self, b: Usage) -> Usage {
            Usage { user: self.user - b.user, sys: self.sys - b.sys, vcsw: self.vcsw - b.vcsw, ivcsw: self.ivcsw - b.ivcsw }
        }
        fn cpu(&self) -> f64 {
            self.user + self.sys
        }
    }

    struct Bench;

    fn pat(i: u64) -> u8 {
        (i ^ (i >> 11) ^ (i >> 19)).wrapping_mul(31) as u8
    }

    thread_local! {
        static VERIFY: bool = std::env::var_os("VERIFY").is_some();
    }

    fn fill(buf: &mut [u8], at: u64) {
        if !VERIFY.with(|v| *v) {
            return;
        }
        for (k, b) in buf.iter_mut().enumerate() {
            *b = pat(at + k as u64);
        }
    }

    fn check(buf: &[u8], at: u64) -> bool {
        !VERIFY.with(|v| *v) || buf.iter().enumerate().all(|(k, &b)| b == pat(at + k as u64))
    }

    async fn serve(mut s: TcpConn, dst: SocketAddr) {
        let mut buf = vec![0u8; 64 * 1024];
        match dst.port() {
            SINK => {
                let mut total = 0u64;
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if !check(&buf[..n], total) {
                                eprintln!("sink: искажены данные на {total}");
                                return;
                            }
                            total += n as u64
                        }
                    }
                }
                let _ = s.write_all(&total.to_be_bytes()).await;
                let _ = s.shutdown().await;
            }
            SOURCE => {
                let mut want = [0u8; 8];
                if s.read_exact(&mut want).await.is_err() {
                    return;
                }
                let total = u64::from_be_bytes(want);
                let mut at = 0u64;
                while at < total {
                    let n = (total - at).min(16 * 1024) as usize;
                    fill(&mut buf[..n], at);
                    if s.write_all(&buf[..n]).await.is_err() {
                        return;
                    }
                    at += n as u64;
                }
                let _ = s.shutdown().await;
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                }
            }
            PACED => {
                let mut want = [0u8; 8];
                if s.read_exact(&mut want).await.is_err() {
                    return;
                }
                let mut left = u64::from_be_bytes(want);
                buf.fill(0x5a);
                let mut tick = tokio::time::interval(PACE);
                while left > 0 {
                    tick.tick().await;
                    let mut chunk = left.min(64 * 1024) as usize;
                    left -= chunk as u64;
                    while chunk > 0 {
                        let n = chunk.min(16 * 1024);
                        if s.write_all(&buf[..n]).await.is_err() {
                            return;
                        }
                        chunk -= n;
                    }
                }
                let _ = s.shutdown().await;
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                }
            }
            SMALL => {
                let mut want = [0u8; 8];
                if s.read_exact(&mut want).await.is_err() {
                    return;
                }
                let mut left = u64::from_be_bytes(want);
                buf.fill(0x5a);
                let mut tick = tokio::time::interval(SMALL_PACE);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                while left > 0 {
                    tick.tick().await;
                    let n = left.min(4096) as usize;
                    left -= n as u64;
                    if s.write_all(&buf[..n]).await.is_err() {
                        return;
                    }
                }
                let _ = s.shutdown().await;
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                }
            }
            RR => {
                let mut req = [0u8; 300];
                if s.read_exact(&mut req).await.is_err() {
                    return;
                }
                let _ = s.write_all(&[0x42; 4096]).await;
                let _ = s.shutdown().await;
                let _ = s.read(&mut buf).await;
            }
            _ => loop {
                match s.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if s.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                }
            },
        }
    }

    impl Handler for Bench {
        fn tcp(&self, local: TcpConn, _src: SocketAddr, dst: SocketAddr) -> LocalBoxFuture<()> {
            Box::pin(serve(local, dst))
        }
        fn dns_stream(&self, _: usize) -> LocalBoxFuture<io::Result<BoxStream>> {
            Box::pin(async { Err(io::ErrorKind::Unsupported.into()) })
        }
        fn dns_label(&self, _: usize) -> String {
            String::new()
        }
        fn dns_route(&self, _: &[u8]) -> DnsRoute {
            DnsRoute::Servers(Vec::new())
        }
        fn dns_direct(&self, _: SocketAddr, _: Vec<u8>) -> LocalBoxFuture<Option<Vec<u8>>> {
            Box::pin(async { None })
        }
        fn udp_route(&self, _: SocketAddr, dst: SocketAddr) -> Option<u64> {
            (dst.port() == UDP_ECHO).then_some(0)
        }
        fn udp(&self, mut rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>, reply: UdpReply) -> LocalBoxFuture<()> {
            Box::pin(async move {
                while let Some((dst, data)) = rx.recv().await {
                    reply.send(dst, &data);
                }
            })
        }
    }

    fn open_tun(name: &str) -> i32 {
        let fd = unsafe { libc::open(c"/dev/net/tun".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        assert!(fd >= 0, "open /dev/net/tun: {}", io::Error::last_os_error());
        let mut ifr = [0u8; 40];
        ifr[..name.len()].copy_from_slice(name.as_bytes());
        let flags: i16 = 0x0001 | 0x1000;
        ifr[16..18].copy_from_slice(&flags.to_ne_bytes());
        let r = unsafe { libc::ioctl(fd, 0x400454ca, ifr.as_mut_ptr()) };
        assert!(r == 0, "TUNSETIFF: {} (запускать под unshare -rn)", io::Error::last_os_error());
        fd
    }

    fn ip(args: &str) {
        let ok = Command::new("ip").args(args.split_whitespace()).status().map(|s| s.success()).unwrap_or(false);
        assert!(ok, "ip {args}");
    }

    enum Cmd {
        Usage(std::sync::mpsc::Sender<Usage>),
    }

    fn pin(cpu: usize) {
        if std::env::var_os("PIN").is_none() {
            return;
        }
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_SET(cpu, &mut set);
            libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
        }
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let stack = args.first().cloned().unwrap_or_else(|| "user".into());
        let mtu: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(9000);
        let tests: Vec<String> =
            if args.len() > 2 { args[2..].to_vec() } else { ["up", "down", "rr", "chat", "idle"].map(String::from).to_vec() };
        let mb: u64 = std::env::var("MB").ok().and_then(|s| s.parse().ok()).unwrap_or(512);

        let fd = open_tun("rb0");
        ip("link set lo up");
        ip(&format!("link set rb0 up mtu {mtu}"));
        ip(&format!("addr add {LOCAL}/30 dev rb0"));
        ip("route add 198.18.0.0/15 dev rb0");
        let android = cfg!(target_os = "android");
        if android {
            for net in ["198.18.0.0/15", "172.19.0.0/30"] {
                let _ = Command::new("ip").args(format!("rule del to {net} lookup main pref 5000").split(' ')).status();
                ip(&format!("rule add to {net} lookup main pref 5000"));
            }
        }
        if let Ok(netem) = std::env::var("NETEM") {
            let ok = Command::new("tc")
                .args(["qdisc", "add", "dev", "rb0", "root", "netem"])
                .args(netem.split_whitespace())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            assert!(ok, "tc netem {netem}");
        }

        let kind = match stack.as_str() {
            "system" => rv_tun::Stack::System,
            _ => rv_tun::Stack::User,
        };
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Cmd>();
        let (stop_tx, stop_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            eprintln!("stack tid {}", unsafe { libc::gettid() });
            pin(2);
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let local = tokio::task::LocalSet::new();
            let (_net_tx, net_rx) = watch::channel(0u64);
            let cfg = rv_tun::Config { fd, mtu, v4: Some((LOCAL, PEER)), v6: None, network: net_rx, stack: kind };
            local.block_on(&rt, async move {
                tokio::task::spawn_local(async move {
                    while let Some(Cmd::Usage(tx)) = cmd_rx.recv().await {
                        let _ = tx.send(Usage::thread());
                    }
                });
                let run = rv_tun::run(cfg, Rc::new(Bench), stop_rx, move |r| {
                    let _ = ready_tx.send(r.is_ok());
                });
                let r = if std::env::var_os("TUN_TASK").is_some() { tokio::task::spawn_local(run).await.unwrap() } else { run.await };
                if let Err(e) = r {
                    eprintln!("stack: {e}");
                }
            });
        });
        assert!(ready_rx.recv().unwrap());
        let usage = || {
            let (tx, rx) = std::sync::mpsc::channel();
            cmd_tx.send(Cmd::Usage(tx)).ok().unwrap();
            rx.recv().unwrap()
        };

        pin(6);
        println!("stack={stack} mtu={mtu}");
        for t in &tests {
            if t == "idle" {
                let n = 200;
                let d = Duration::from_secs(20);
                let conns = idle_setup(n);
                let s0 = usage();
                std::thread::sleep(d);
                let s = usage().since(s0);
                println!("idle  {n} соединений {d:?}: stack cpu {:.6}s wakeups {}", s.cpu(), s.vcsw - 1);
                drop(conns);
                std::thread::sleep(Duration::from_millis(300));
                continue;
            }
            let prof = std::env::var("PROF_TEST").is_ok_and(|p| p == *t);
            let count = std::env::var_os("SYSCOUNT").is_some();
            if count {
                unsafe { libc::raise(libc::SIGUSR1) };
            }
            let s0 = usage();
            let c0 = Usage::thread();
            if prof {
                unsafe { libc::raise(libc::SIGUSR2) };
            }
            let p0 = tun_packets();
            let t0 = Instant::now();
            let note = match t.as_str() {
                "up" => up(mb),
                "down" => down(mb),
                "rr" => rr(2000),
                "pdown" => paced_down(50),
                "sdown" => small_down(20),
                "pup" => paced_up(50),
                "udp" => udp_echo(100_000),
                "chat" => chat(5000),
                _ => continue,
            };
            let wall = t0.elapsed().as_secs_f64();
            if count {
                unsafe { libc::raise(libc::SIGUSR1) };
            }
            if prof {
                unsafe { libc::raise(libc::SIGUSR2) };
            }
            let c = Usage::thread().since(c0);
            let s = usage().since(s0);
            let p1 = tun_packets();
            let note = format!("{note}; пакетов от приложения {} к приложению {}", p1.0 - p0.0, p1.1 - p0.1);
            println!(
                "{t:5} wall {wall:7.3}s  stack cpu {:7.3}s (u {:.3} s {:.3}) wakeups {:7} preempt {:5}  app cpu {:7.3}s  {note}",
                s.cpu(),
                s.user,
                s.sys,
                s.vcsw - 1,
                s.ivcsw,
                c.cpu()
            );
        }
        let _ = stop_tx.send(());
        thread.join().unwrap();
        if android {
            for net in ["198.18.0.0/15", "172.19.0.0/30"] {
                ip(&format!("rule del to {net} lookup main pref 5000"));
            }
        }
    }

    fn tun_packets() -> (u64, u64) {
        let dev = std::fs::read_to_string("/proc/net/dev").unwrap_or_default();
        let Some(l) = dev.lines().find(|l| l.trim_start().starts_with("rb0:")) else { return (0, 0) };
        let f: Vec<u64> = l.split_once(':').unwrap().1.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        (f.get(9).copied().unwrap_or(0), f.get(1).copied().unwrap_or(0))
    }

    fn connect(port: u16) -> TcpStream {
        let s = TcpStream::connect_timeout(&SocketAddr::new(IpAddr::V4(REMOTE), port), Duration::from_secs(5)).expect("connect");
        s.set_nodelay(true).unwrap();
        s
    }

    fn up(mb: u64) -> String {
        let t0 = Instant::now();
        let mut s = connect(SINK);
        let mut buf = vec![0u8; 256 * 1024];
        let total = mb << 20;
        let mut sent = 0;
        while sent < total {
            let n = buf.len().min((total - sent) as usize);
            fill(&mut buf[..n], sent);
            s.write_all(&buf[..n]).unwrap();
            sent += n as u64;
        }
        s.shutdown(std::net::Shutdown::Write).unwrap();
        let mut got = [0u8; 8];
        s.read_exact(&mut got).unwrap();
        assert_eq!(u64::from_be_bytes(got), total, "sink получил не всё");
        format!("{:.0} MB/s", mb as f64 / t0.elapsed().as_secs_f64())
    }

    fn down(mb: u64) -> String {
        let t0 = Instant::now();
        let mut s = connect(SOURCE);
        let total = mb << 20;
        s.write_all(&total.to_be_bytes()).unwrap();
        let mut buf = vec![0u8; 256 * 1024];
        let mut got = 0u64;
        loop {
            let n = s.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            assert!(check(&buf[..n], got), "искажённые данные на {got}");
            got += n as u64;
        }
        assert_eq!(got, total, "source отдал не всё");
        format!("{:.0} MB/s", mb as f64 / t0.elapsed().as_secs_f64())
    }

    fn paced_down(mb: u64) -> String {
        let mut s = connect(PACED);
        let total = mb << 20;
        s.write_all(&total.to_be_bytes()).unwrap();
        let mut buf = vec![0u8; 256 * 1024];
        let mut got = 0u64;
        loop {
            let n = s.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            got += n as u64;
        }
        assert_eq!(got, total);
        format!("{mb} MB")
    }

    fn small_down(mb: u64) -> String {
        let mut s = connect(SMALL);
        let total = mb << 20;
        s.write_all(&total.to_be_bytes()).unwrap();
        let mut buf = vec![0u8; 256 * 1024];
        let mut got = 0u64;
        let mut reads = 0u64;
        loop {
            let n = s.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            got += n as u64;
            reads += 1;
        }
        assert_eq!(got, total);
        format!("{mb} MB, {reads} чтений приложения")
    }

    fn paced_up(mb: u64) -> String {
        let mut s = connect(SINK);
        let mut buf = vec![0u8; 64 * 1024];
        let start = Instant::now();
        let n = (mb << 20) as usize / buf.len();
        for i in 0..n {
            let at = start + PACE * i as u32;
            if let Some(d) = at.checked_duration_since(Instant::now()) {
                std::thread::sleep(d);
            }
            let at = (i * buf.len()) as u64;
            fill(&mut buf, at);
            s.write_all(&buf).unwrap();
        }
        s.shutdown(std::net::Shutdown::Write).unwrap();
        let mut got = [0u8; 8];
        s.read_exact(&mut got).unwrap();
        assert_eq!(u64::from_be_bytes(got), mb << 20);
        format!("{mb} MB")
    }

    fn udp_echo(n: usize) -> String {
        let s = std::net::UdpSocket::bind("172.19.0.1:0").unwrap();
        s.connect(SocketAddr::new(IpAddr::V4(REMOTE), UDP_ECHO)).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        let mut msg = [0u8; 1200];
        let mut back = [0u8; 1500];
        let (mut sent, mut got) = (0, 0);
        let t0 = Instant::now();
        while sent < n {
            for _ in 0..32 {
                msg[..8].copy_from_slice(&(sent as u64).to_be_bytes());
                s.send(&msg).unwrap();
                sent += 1;
            }
            while got < sent {
                match s.recv(&mut back) {
                    Ok(1200) => got += 1,
                    Ok(_) => panic!("размер"),
                    Err(_) => break,
                }
            }
        }
        format!("{:.0} kpps, lost {}", n as f64 / t0.elapsed().as_secs_f64() / 1000.0, sent - got)
    }

    fn rr(n: usize) -> String {
        let t0 = Instant::now();
        for _ in 0..n {
            let mut s = connect(RR);
            s.write_all(&[7u8; 300]).unwrap();
            let mut resp = Vec::new();
            s.read_to_end(&mut resp).unwrap();
            assert_eq!(resp.len(), 4096);
        }
        format!("{:.0} conn/s", n as f64 / t0.elapsed().as_secs_f64())
    }

    fn chat(n: usize) -> String {
        let mut s = connect(ECHO);
        let msg = [9u8; 100];
        let mut back = [0u8; 100];
        let t0 = Instant::now();
        for _ in 0..n {
            s.write_all(&msg).unwrap();
            s.read_exact(&mut back).unwrap();
        }
        format!("{:.1} us/rtt", t0.elapsed().as_secs_f64() * 1e6 / n as f64)
    }

    fn idle_setup(n: usize) -> Vec<TcpStream> {
        let mut conns: Vec<TcpStream> = (0..n).map(|_| connect(ECHO)).collect();
        for c in &mut conns {
            c.write_all(b"x").unwrap();
            let mut b = [0u8; 1];
            c.read_exact(&mut b).unwrap();
        }
        std::thread::sleep(Duration::from_millis(500));
        conns
    }
}
