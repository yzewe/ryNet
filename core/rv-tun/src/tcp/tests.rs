use super::*;
use crate::packet;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const APP: &str = "172.19.0.1:40000";
const DST: &str = "198.18.0.1:443";
const MTU: usize = 9000;

fn seg(flags: u8, seq: u32, ack: u32, wnd: u16, opts: &[u8], data: &[u8]) -> Vec<u8> {
    let (a, d): (SocketAddr, SocketAddr) = (APP.parse().unwrap(), DST.parse().unwrap());
    let (IpAddr::V4(ai), IpAddr::V4(di)) = (a.ip(), d.ip()) else { unreachable!() };
    let tl = 20 + opts.len() + data.len();
    let mut p = vec![0u8; 20 + tl];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&((20 + tl) as u16).to_be_bytes());
    p[8] = 64;
    p[9] = 6;
    p[12..16].copy_from_slice(&ai.octets());
    p[16..20].copy_from_slice(&di.octets());
    let t = &mut p[20..];
    t[0..2].copy_from_slice(&a.port().to_be_bytes());
    t[2..4].copy_from_slice(&d.port().to_be_bytes());
    t[4..8].copy_from_slice(&seq.to_be_bytes());
    t[8..12].copy_from_slice(&ack.to_be_bytes());
    t[12] = (((20 + opts.len()) / 4) as u8) << 4;
    t[13] = flags;
    t[14..16].copy_from_slice(&wnd.to_be_bytes());
    t[20..20 + opts.len()].copy_from_slice(opts);
    t[20 + opts.len()..].copy_from_slice(data);
    p
}

#[derive(Debug)]
struct Got {
    flags: u8,
    seq: u32,
    ack: u32,
    wnd: u16,
    opts: Vec<u8>,
    data: Vec<u8>,
}

fn got(parts: &[&[u8]]) -> Got {
    let pkt = parts.concat();
    let p = packet::parse(&pkt).expect("ip");
    assert_eq!(p.total_len, pkt.len());
    let mut ic = Csum::default();
    ic.add(&pkt[..20]);
    assert_eq!(ic.finish(), 0, "ip checksum");
    let mut c = Csum::default();
    c.add(&pkt[12..20]);
    c.add_u16(6);
    c.add_u16((pkt.len() - 20) as u16);
    c.add(&pkt[20..]);
    assert_eq!(c.finish(), 0, "tcp checksum");
    assert_eq!(SocketAddr::new(p.src, p.src_port), DST.parse::<SocketAddr>().unwrap());
    assert_eq!(SocketAddr::new(p.dst, p.dst_port), APP.parse::<SocketAddr>().unwrap());
    let t = &pkt[20..];
    let hl = ((t[12] >> 4) as usize) * 4;
    Got {
        flags: t[13],
        seq: u32::from_be_bytes(t[4..8].try_into().unwrap()),
        ack: u32::from_be_bytes(t[8..12].try_into().unwrap()),
        wnd: u16::from_be_bytes([t[14], t[15]]),
        opts: t[20..hl].to_vec(),
        data: t[hl..].to_vec(),
    }
}

struct Rig {
    st: Stack,
    out: Vec<Got>,
    sock: Option<Socket>,
}

impl Rig {
    fn new() -> Self {
        Rig { st: Stack::new(MTU), out: Vec::new(), sock: None }
    }
    fn feed(&mut self, pkt: Vec<u8>) -> Verdict {
        let p = packet::parse(&pkt).unwrap();
        self.st.input(&pkt, &p, Instant::now())
    }
    fn flush(&mut self) -> Vec<Got> {
        let out = &mut self.out;
        self.st.flush(Instant::now(), &mut |parts| out.push(got(parts)), true);
        self.st.flush_deferred(Instant::now(), &mut |parts| out.push(got(parts)));
        std::mem::take(&mut self.out)
    }
    fn timers(&mut self) -> Vec<Got> {
        let out = &mut self.out;
        self.st.on_timer(Instant::now(), &mut |parts| out.push(got(parts)));
        self.st.flush(Instant::now(), &mut |parts| out.push(got(parts)), false);
        std::mem::take(&mut self.out)
    }
    fn connect(&mut self) -> u32 {
        let opts = [2, 4, 0x05, 0xb4, 1, 3, 3, 7, 1, 1, 4, 2];
        assert!(matches!(self.feed(seg(TCP_SYN, 1000, 0, 64240, &opts, &[])), Verdict::None));
        let o = self.flush();
        assert_eq!(o.len(), 1);
        let sa = &o[0];
        assert_eq!(sa.flags, TCP_SYN | TCP_ACK);
        assert_eq!(sa.ack, 1001);
        assert_eq!(&sa.opts[..4], &[2, 4, ((MTU - 40) >> 8) as u8, (MTU - 40) as u8]);
        assert_eq!(sa.opts[4..7], [1, 3, 3]);
        assert_eq!(sa.opts[8..], [1, 1, 4, 2]);
        let iss = sa.seq;
        match self.feed(seg(TCP_ACK, 1001, iss.wrapping_add(1), 502, &[], &[])) {
            Verdict::Accept(s) => self.sock = Some(s),
            _ => panic!("не принято"),
        }
        assert!(self.flush().is_empty(), "на голый ACK рукопожатия не отвечаем");
        iss
    }
}

fn run<F: Future<Output = ()>>(f: F) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().start_paused(true).build().unwrap();
    tokio::task::LocalSet::new().block_on(&rt, f);
}
use std::future::Future;

#[test]
fn handshake_data_one_ack_per_batch_and_no_idle_timers() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        assert_eq!(r.st.next_timer(), None, "после рукопожатия таймеров нет");
        for i in 0..5u32 {
            r.feed(seg(TCP_ACK, 1001 + i * 100, iss + 1, 502, &[], &[i as u8; 100]));
        }
        let o = r.flush();
        assert_eq!(o.len(), 1, "пять сегментов — один ACK: {o:?}");
        assert_eq!(o[0].ack, 1501);
        let mut s = r.sock.take().unwrap();
        let mut b = vec![0u8; 500];
        s.read_exact(&mut b).await.unwrap();
        assert_eq!(&b[..100], &[0; 100]);
        assert_eq!(&b[400..], &[4; 100]);
        s.write_all(b"hello").await.unwrap();
        s.write_all(b" world").await.unwrap();
        let o = r.flush();
        assert_eq!(o.len(), 1, "две записи за такт — один сегмент: {o:?}");
        assert_eq!(o[0].data, b"hello world");
        assert_eq!(o[0].seq, iss + 1);
        assert!(r.st.next_timer().is_some());
        r.feed(seg(TCP_ACK, 1501, iss + 12, 502, &[], &[]));
        assert!(r.flush().is_empty());
        assert_eq!(r.st.next_timer(), None, "всё подтверждено — таймер снят");
        r.sock = Some(s);
    });
}

#[test]
fn deferred_ack_rides_on_response() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        let mut b = [0u8; 4];
        let rd = tokio::task::spawn_local(async move {
            s.read_exact(&mut b).await.unwrap();
            s.write_all(b"pong").await.unwrap();
            s
        });
        tokio::task::yield_now().await;
        r.feed(seg(TCP_ACK | TCP_PSH, 1001, iss + 1, 502, &[], b"ping"));
        let out = &mut r.out;
        r.st.flush(Instant::now(), &mut |p| out.push(got(p)), true);
        assert!(r.out.is_empty(), "чистый ACK придержан");
        let s = rd.await.unwrap();
        let o = r.flush();
        assert_eq!(o.len(), 1, "{o:?}");
        assert_eq!((o[0].ack, o[0].data.as_slice()), (1005, &b"pong"[..]));
        r.sock = Some(s);
    });
}

#[test]
fn out_of_order_is_sacked_and_reassembled() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        r.feed(seg(TCP_ACK, 1101, iss + 1, 502, &[], &[2; 100]));
        r.feed(seg(TCP_ACK, 1301, iss + 1, 502, &[], &[4; 100]));
        let o = r.flush();
        assert_eq!(o.len(), 2, "повторный ACK на каждый сегмент вне порядка");
        for g in &o {
            assert_eq!(g.ack, 1001);
            assert_eq!(g.opts[..4], [1, 1, 5, 18]);
            assert_eq!(u32::from_be_bytes(g.opts[4..8].try_into().unwrap()), 1101);
            assert_eq!(u32::from_be_bytes(g.opts[8..12].try_into().unwrap()), 1201);
            assert_eq!(u32::from_be_bytes(g.opts[12..16].try_into().unwrap()), 1301);
        }
        r.feed(seg(TCP_ACK, 1201, iss + 1, 502, &[], &[3; 100]));
        r.feed(seg(TCP_ACK, 1001, iss + 1, 502, &[], &[1; 100]));
        let o = r.flush();
        assert_eq!(o.last().unwrap().ack, 1401);
        assert!(o.last().unwrap().opts.is_empty(), "дыр нет — SACK нет");
        let mut b = vec![0u8; 400];
        r.sock.as_mut().unwrap().read_exact(&mut b).await.unwrap();
        for (i, c) in b.chunks(100).enumerate() {
            assert!(c.iter().all(|&x| x == i as u8 + 1));
        }
    });
}

#[test]
fn retransmit_on_rto_then_reset_after_retries() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        s.write_all(&[7; 3000]).await.unwrap();
        let o = r.flush();
        assert_eq!(o.len(), 3, "MSS приложения 1460");
        assert!(o.iter().all(|g| g.data.len() <= 1460));
        let t = r.st.next_timer().unwrap();
        tokio::time::sleep_until(t).await;
        let o = r.timers();
        assert_eq!(o[0].seq, iss + 1, "go-back-N с snd_una");
        r.feed(seg(TCP_ACK, 1001, iss + 1 + 1460, 502, &[], &[]));
        r.flush();
        let mut resets = 0;
        for _ in 0..12 {
            let Some(t) = r.st.next_timer() else { break };
            tokio::time::sleep_until(t).await;
            for g in r.timers() {
                assert_ne!(g.seq, iss + 1, "подтверждённое не шлём");
                if g.flags & TCP_RST != 0 {
                    resets += 1;
                }
            }
        }
        assert_eq!(resets, 1, "после исчерпания повторов — RST");
        let mut b = [0u8; 1];
        assert_eq!(s.read(&mut b).await.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(r.st.conns.len(), 0);
    });
}

#[test]
fn zero_window_probe_and_reopen() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        r.feed(seg(TCP_ACK, 1001, iss + 1, 0, &[], &[]));
        r.flush();
        s.write_all(&[1; 100]).await.unwrap();
        assert!(r.flush().is_empty(), "в нулевое окно не шлём");
        let t = r.st.next_timer().expect("persist");
        tokio::time::sleep_until(t).await;
        let o = r.timers();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].seq, iss, "зонд старым номером");
        assert!(o[0].data.is_empty());
        r.feed(seg(TCP_ACK, 1001, iss + 1, 502, &[], &[]));
        let o = r.flush();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].data.len(), 100);
        r.sock = Some(s);
    });
}

#[test]
fn receive_window_update_only_when_needed() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let cap = r.st.conns.values().next().unwrap().tcb.borrow().rx_cap;
        let mut seq = 1001u32;
        while seq - 1001 + 1460 <= cap as u32 {
            r.feed(seg(TCP_ACK, seq, iss + 1, 502, &[], &[0; 1460]));
            seq += 1460;
        }
        let o = r.flush();
        let last = o.last().unwrap();
        let ws = 3;
        assert!(((last.wnd as usize) << ws) < 1460 * 2, "окно почти закрыто");
        let mut b = vec![0u8; 512];
        r.sock.as_mut().unwrap().read_exact(&mut b).await.unwrap();
        assert!(r.flush().is_empty());
        let mut big = vec![0u8; cap / 2];
        r.sock.as_mut().unwrap().read_exact(&mut big).await.unwrap();
        let o = r.flush();
        assert_eq!(o.len(), 1);
        assert!(((o[0].wnd as usize) << ws) >= cap / 2);
    });
}

#[test]
fn close_both_ways_removes_connection() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        r.feed(seg(TCP_ACK | TCP_FIN, 1001, iss + 1, 502, &[], b"bye"));
        r.flush();
        let mut v = Vec::new();
        s.read_to_end(&mut v).await.unwrap();
        assert_eq!(v, b"bye");
        s.write_all(b"ok").await.unwrap();
        s.shutdown().await.unwrap();
        let o = r.flush();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].flags, TCP_ACK | TCP_PSH | TCP_FIN);
        assert_eq!(o[0].ack, 1005);
        r.feed(seg(TCP_ACK, 1005, iss + 4, 502, &[], &[]));
        r.flush();
        assert_eq!(r.st.conns.len(), 0);
        assert_eq!(r.st.next_timer(), None);
        drop(s);
        assert!(r.flush().is_empty(), "закрытое соединение молчит");
    });
}

#[test]
fn proxy_close_first_then_linger_limit() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        drop(r.sock.take());
        let o = r.flush();
        assert_eq!(o[0].flags, TCP_FIN | TCP_ACK);
        r.feed(seg(TCP_ACK, 1001, iss + 2, 502, &[], &[]));
        r.flush();
        assert_eq!(r.st.conns.len(), 1, "FIN_WAIT_2: ждём FIN приложения");
        tokio::time::sleep(LINGER + Duration::from_secs(1)).await;
        let o = r.timers();
        assert_eq!(o.len(), 1);
        assert!(o[0].flags & TCP_RST != 0);
        assert_eq!(r.st.conns.len(), 0);
    });
}

#[test]
fn reset_from_app_and_from_closer() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        r.feed(seg(TCP_RST, 1001, 0, 0, &[], &[]));
        let mut b = [0u8; 1];
        assert_eq!(s.read(&mut b).await.unwrap_err().kind(), io::ErrorKind::ConnectionReset);
        assert_eq!(r.st.conns.len(), 0);
        let mut r = Rig::new();
        let iss2 = r.connect();
        r.feed(seg(TCP_RST, 900_000, 0, 0, &[], &[]));
        assert_eq!(r.st.conns.len(), 1);
        r.sock.as_ref().unwrap().handle().reset();
        let o = r.flush();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].flags, TCP_RST | TCP_ACK);
        assert_eq!(o[0].seq, iss2 + 1);
        assert_eq!(r.st.conns.len(), 0);
        let _ = iss;
    });
}

#[test]
fn unknown_flow_and_syn_retransmit() {
    run(async {
        let mut r = Rig::new();
        assert!(matches!(r.feed(seg(TCP_ACK, 5, 5, 502, &[], b"x")), Verdict::Reject));
        assert!(matches!(r.feed(seg(TCP_RST, 5, 5, 502, &[], &[])), Verdict::None));
        r.feed(seg(TCP_SYN, 1000, 0, 64240, &[2, 4, 5, 180], &[]));
        let a = r.flush();
        r.feed(seg(TCP_SYN, 1000, 0, 64240, &[2, 4, 5, 180], &[]));
        let b = r.flush();
        assert_eq!(a[0].seq, b[0].seq, "повтор SYN — тот же SYN-ACK");
        assert_eq!(b[0].opts.len(), 4, "без WS и SACK у приложения — только MSS");
        let mut synacks = 0;
        while let Some(t) = r.st.next_timer() {
            tokio::time::sleep_until(t).await;
            synacks += r.timers().len();
        }
        assert_eq!(synacks, SYN_RETRIES as usize);
        assert_eq!(r.st.conns.len(), 0);
    });
}

#[test]
fn unread_data_on_close_resets_like_linux() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        r.feed(seg(TCP_ACK, 1001, iss + 1, 502, &[], b"unread"));
        r.flush();
        drop(r.sock.take());
        let o = r.flush();
        assert_eq!(o.len(), 1);
        assert!(o[0].flags & TCP_RST != 0);
    });
}

#[test]
fn ipv6_handshake_checksums_and_mss() {
    run(async {
        let a: SocketAddr = "[fd00::1]:40000".parse().unwrap();
        let d: SocketAddr = "[2001:db8::5]:443".parse().unwrap();
        let (IpAddr::V6(ai), IpAddr::V6(di)) = (a.ip(), d.ip()) else { unreachable!() };
        let mut p = vec![0u8; 40 + 24];
        p[0] = 0x60;
        p[4..6].copy_from_slice(&24u16.to_be_bytes());
        p[6] = 6;
        p[7] = 64;
        p[8..24].copy_from_slice(&ai.octets());
        p[24..40].copy_from_slice(&di.octets());
        let t = &mut p[40..];
        t[0..2].copy_from_slice(&a.port().to_be_bytes());
        t[2..4].copy_from_slice(&d.port().to_be_bytes());
        t[4..8].copy_from_slice(&7u32.to_be_bytes());
        t[12] = 6 << 4;
        t[13] = TCP_SYN;
        t[14..16].copy_from_slice(&65535u16.to_be_bytes());
        t[20..24].copy_from_slice(&[2, 4, 0x05, 0xa0]);
        let mut st = Stack::new(MTU);
        let parsed = packet::parse(&p).unwrap();
        assert!(matches!(st.input(&p, &parsed, Instant::now()), Verdict::None));
        let mut out = Vec::new();
        st.flush(Instant::now(), &mut |parts| out.push(parts.concat()), false);
        assert_eq!(out.len(), 1);
        let o = &out[0];
        let q = packet::parse(o).unwrap();
        assert_eq!((q.src, q.dst, q.src_port, q.dst_port), (d.ip(), a.ip(), 443, 40000));
        let mut c = Csum::default();
        c.add(&o[8..40]);
        c.add_u16(6);
        c.add_u16((o.len() - 40) as u16);
        c.add(&o[40..]);
        assert_eq!(c.finish(), 0, "tcp checksum v6");
        assert_eq!(o[40 + 13], TCP_SYN | TCP_ACK);
        assert_eq!(&o[60..64], &[2, 4, ((MTU - 60) >> 8) as u8, (MTU - 60) as u8], "MSS для IPv6 = MTU-60");
    });
}

#[test]
fn trickle_to_app_waits_briefly_while_previous_data_is_unacked() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        s.write_all(&[1; 100]).await.unwrap();
        assert_eq!(r.flush().iter().filter(|g| !g.data.is_empty()).count(), 1, "первое уходит сразу");
        s.write_all(&[2; 100]).await.unwrap();
        assert!(r.flush().iter().all(|g| g.data.is_empty()), "пока прежнее в пути, капля ждёт продолжения");
        let t = r.st.next_timer().unwrap();
        assert!(t <= tokio::time::Instant::now() + HOLD, "срок придержки не округляется до 16 мс");
        tokio::time::sleep_until(t).await;
        let o = r.timers();
        assert_eq!(o.iter().map(|g| g.data.len()).sum::<usize>(), 100, "по сроку уходит");
        r.feed(seg(TCP_ACK, 1001, iss + 201, 502, &[], &[]));
        r.flush();
        s.write_all(&[3; 100]).await.unwrap();
        assert_eq!(r.flush().iter().map(|g| g.data.len()).sum::<usize>(), 100);
    });
}

#[test]
fn held_tail_against_full_window_does_not_rearm_timer_in_the_past() {
    run(async {
        let mut r = Rig::new();
        let _iss = r.connect();
        let mut s = r.sock.take().unwrap();
        let w = 502usize << 7;
        s.write_all(&vec![1u8; w - 100]).await.unwrap();
        assert_eq!(r.flush().iter().map(|g| g.data.len()).sum::<usize>(), w - 100);
        s.write_all(&[2u8; 50]).await.unwrap();
        assert!(r.flush().iter().all(|g| g.data.is_empty()));
        s.write_all(&[3u8; 1000]).await.unwrap();
        assert!(r.flush().iter().all(|g| g.data.is_empty()));
        let t = r.st.next_timer().unwrap();
        tokio::time::sleep_until(t).await;
        r.timers();
        let now = Instant::now();
        assert!(r.st.next_timer().is_none_or(|t| t > now), "срок не перевзводится в прошлое");
        assert!(r.st.conns.values().all(|c| c.tcb.borrow().hold_at.is_none()), "придержка снята");
        r.sock = Some(s);
    });
}

#[test]
fn app_fin_after_our_fin_is_acked() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        drop(r.sock.take());
        let o = r.flush();
        assert_eq!(o[0].flags, TCP_FIN | TCP_ACK);
        r.feed(seg(TCP_ACK, 1001, iss + 2, 502, &[], &[]));
        assert!(r.flush().is_empty());
        r.feed(seg(TCP_ACK | TCP_FIN, 1001, iss + 2, 502, &[], &[]));
        let o = r.flush();
        assert_eq!(o.len(), 1, "{o:?}");
        assert_eq!((o[0].flags, o[0].ack), (TCP_ACK, 1002));
        assert_eq!(r.st.conns.len(), 0);

        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        s.shutdown().await.unwrap();
        assert_eq!(r.flush()[0].flags & TCP_FIN, TCP_FIN);
        r.feed(seg(TCP_ACK | TCP_FIN, 1001, iss + 2, 502, &[], &[]));
        let o = r.flush();
        assert_eq!(o.len(), 1, "{o:?}");
        assert_eq!(o[0].ack, 1002);
        assert_eq!(r.st.conns.len(), 0);
        r.sock = Some(s);
    });
}

#[test]
fn reset_from_app_with_data_in_flight() {
    run(async {
        let mut r = Rig::new();
        let _iss = r.connect();
        let mut s = r.sock.take().unwrap();
        s.write_all(&[1; 100]).await.unwrap();
        r.flush();
        r.feed(seg(TCP_RST, 1001, 0, 0, &[], &[]));
        assert!(r.flush().is_empty());
        assert_eq!(r.st.conns.len(), 0);
        r.sock = Some(s);
    });
}

#[test]
fn three_dup_acks_resend_only_the_hole() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        s.write_all(&vec![5u8; 8 * 1460]).await.unwrap();
        let o = r.flush();
        assert_eq!(o.len(), 8);
        let hole = o[1].seq;
        for _ in 2..8 {
            r.feed(seg(TCP_ACK, 1001, hole, 502, &[], &[]));
        }
        let o = r.flush();
        let data: Vec<_> = o.iter().filter(|g| !g.data.is_empty()).collect();
        assert_eq!(data.len(), 1, "только дыра, без go-back-N: {o:?}");
        assert_eq!((data[0].seq, data[0].data.len()), (hole, 1460));
        r.feed(seg(TCP_ACK, 1001, iss + 1 + 8 * 1460, 502, &[], &[]));
        assert!(r.flush().iter().all(|g| g.data.is_empty()));
        assert_eq!(r.st.next_timer(), None);
        r.sock = Some(s);
    });
}

#[test]
fn sack_blocks_fit_into_mtu_65535() {
    run(async {
        let mut st = Stack::new(65535);
        let opts = [2, 4, 0xff, 0xd7, 1, 3, 3, 7, 1, 1, 4, 2];
        let p = seg(TCP_SYN, 1000, 0, 65535, &opts, &[]);
        st.input(&p, &packet::parse(&p).unwrap(), Instant::now());
        let mut raw: Vec<Vec<u8>> = Vec::new();
        st.flush(Instant::now(), &mut |parts| raw.push(parts.concat()), false);
        let iss = u32::from_be_bytes(raw[0][24..28].try_into().unwrap());
        let p = seg(TCP_ACK, 1001, iss + 1, 0xffff, &[], &[]);
        let Verdict::Accept(mut s) = st.input(&p, &packet::parse(&p).unwrap(), Instant::now()) else { panic!() };
        let p = seg(TCP_ACK, 1101, iss + 1, 0xffff, &[], &[5; 100]);
        st.input(&p, &packet::parse(&p).unwrap(), Instant::now());
        st.flush(Instant::now(), &mut |_| {}, false);
        s.write_all(&vec![7u8; 65495]).await.unwrap();
        raw.clear();
        st.flush(Instant::now(), &mut |parts| raw.push(parts.concat()), false);
        assert!(raw.len() >= 2, "полный сегмент делится: место под SACK");
        for pk in &raw {
            assert!(pk.len() <= 65535);
            assert_eq!(u16::from_be_bytes([pk[2], pk[3]]) as usize, pk.len(), "длина IPv4");
        }
        drop(s);
    });
}

#[test]
fn retransmitted_data_is_acked_at_once() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        let rd = tokio::task::spawn_local(async move {
            let mut b = [0u8; 4];
            s.read_exact(&mut b).await.unwrap();
            s
        });
        tokio::task::yield_now().await;
        r.feed(seg(TCP_ACK | TCP_PSH, 1001, iss + 1, 502, &[], b"ping"));
        let out = &mut r.out;
        r.st.flush(Instant::now(), &mut |p| out.push(got(p)), true);
        assert!(r.out.is_empty(), "чистый ACK придержан");
        let s = rd.await.unwrap();
        r.feed(seg(TCP_ACK | TCP_PSH, 1001, iss + 1, 502, &[], b"ping"));
        let out = &mut r.out;
        r.st.flush(Instant::now(), &mut |p| out.push(got(p)), true);
        let o = std::mem::take(&mut r.out);
        assert_eq!(o.len(), 1, "{o:?}");
        assert_eq!(o[0].ack, 1005);
        r.sock = Some(s);
    });
}

#[test]
fn reopened_window_resets_probe_backoff() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        r.feed(seg(TCP_ACK, 1001, iss + 1, 0, &[], &[]));
        r.flush();
        s.write_all(&[1; 100]).await.unwrap();
        r.flush();
        for _ in 0..4 {
            let t = r.st.next_timer().unwrap();
            tokio::time::sleep_until(t).await;
            r.timers();
        }
        r.feed(seg(TCP_ACK, 1001, iss + 1, 502, &[], &[]));
        assert_eq!(r.flush().iter().map(|g| g.data.len()).sum::<usize>(), 100);
        let t = r.st.next_timer().unwrap();
        assert!(t <= Instant::now() + RTO_MIN + SLACK, "RTO данных не наследует отсрочку зондов");
        r.sock = Some(s);
    });
}

#[test]
fn rst_with_data_in_flight_does_not_underflow() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        let mut s = r.sock.take().unwrap();
        s.write_all(&[7u8; 300]).await.unwrap();
        let o = r.flush();
        assert_eq!(o.iter().map(|g| g.data.len()).sum::<usize>(), 300);
        assert!(matches!(r.feed(seg(TCP_RST, 1001, iss + 1, 0, &[], &[])), Verdict::None));
        assert!(r.flush().is_empty());
        let mut b = [0u8; 8];
        assert_eq!(s.read(&mut b).await.unwrap_err().kind(), io::ErrorKind::ConnectionReset);
    });
}

#[test]
fn data_with_sack_blocks_fits_mss() {
    run(async {
        let mut r = Rig::new();
        let iss = r.connect();
        r.feed(seg(TCP_ACK, 1101, iss + 1, 502, &[], &[1; 100]));
        let mut s = r.sock.take().unwrap();
        s.write_all(&[7u8; 4000]).await.unwrap();
        let o = r.flush();
        let data: Vec<&Got> = o.iter().filter(|g| !g.data.is_empty()).collect();
        assert!(!data.is_empty());
        assert!(data.iter().any(|g| !g.opts.is_empty()), "SACK-блоки должны быть в сегментах: {o:?}");
        for g in data {
            assert!(g.data.len() + g.opts.len() <= 1460, "данные {} + опции {} > MSS 1460", g.data.len(), g.opts.len());
        }
    });
}

#[test]
fn buffer_caps_shrink_as_the_stack_commits_memory() {
    run(async {
        let mut st = Stack::new(MTU);
        let open = |st: &mut Stack, i: usize| {
            let mut p = seg(TCP_SYN, 1000, 0, 64240, &[], &[]);
            p[20..22].copy_from_slice(&((1024 + i) as u16).to_be_bytes());
            let parsed = packet::parse(&p).unwrap();
            assert!(!matches!(st.input(&p, &parsed, Instant::now()), Verdict::Reject), "соединение {i}");
        };
        let caps = |st: &Stack, i: usize| {
            let key = st.conns.keys().find(|k| k.0.port() == (1024 + i) as u16).copied().unwrap();
            let t = st.conns[&key].tcb.borrow();
            (t.rx_cap, t.tx_cap)
        };
        let full = MEM_FULL / (RX_MIN + TX_CAP);
        let mid = (MEM_MID - MEM_FULL) / (2 * CAP_MID);
        for i in 0..full + mid + 1 {
            open(&mut st, i);
        }
        assert_eq!(caps(&st, 0), (RX_MIN, TX_CAP));
        assert_eq!(caps(&st, full - 1), (RX_MIN, TX_CAP));
        assert_eq!(caps(&st, full), (CAP_MID, CAP_MID));
        assert_eq!(caps(&st, full + mid - 1), (CAP_MID, CAP_MID));
        assert_eq!(caps(&st, full + mid), (CAP_LOW, CAP_LOW));
        let sum: usize = st
            .conns
            .values()
            .map(|c| {
                let t = c.tcb.borrow();
                t.rx_cap + t.tx_cap
            })
            .sum();
        assert_eq!(st.committed.get(), sum);
        assert!(sum <= MEM_BUDGET);
        st.conns.clear();
        st.timers.clear();
        st.flush(Instant::now(), &mut |_| {}, true);
        assert_eq!(st.committed.get(), 0);
        open(&mut st, 0);
        assert_eq!(caps(&st, 0), (RX_MIN, TX_CAP));
    });
}

#[test]
fn reset_connections_held_by_handlers_still_count_against_the_limit() {
    run(async {
        let mut st = Stack::new(MTU);
        let mut held = Vec::new();
        let mut rejected = None;
        for i in 0..=MAX_CONNS {
            let port = (1024 + i) as u16;
            let feed = |st: &mut Stack, mut p: Vec<u8>| {
                p[20..22].copy_from_slice(&port.to_be_bytes());
                let parsed = packet::parse(&p).unwrap();
                st.input(&p, &parsed, Instant::now())
            };
            if matches!(feed(&mut st, seg(TCP_SYN, 1000, 0, 64240, &[], &[])), Verdict::Reject) {
                rejected = Some(i);
                break;
            }
            let mut iss = 0;
            st.flush(Instant::now(), &mut |parts| iss = u32::from_be_bytes(parts.concat()[24..28].try_into().unwrap()), true);
            match feed(&mut st, seg(TCP_ACK, 1001, iss.wrapping_add(1), 502, &[], b"x")) {
                Verdict::Accept(s) => held.push(s),
                _ => panic!("не принято"),
            }
            feed(&mut st, seg(TCP_RST, 1002, 0, 0, &[], &[]));
            st.flush(Instant::now(), &mut |_| {}, true);
        }
        assert_eq!(st.conns.len(), 0);
        assert_eq!(rejected, Some(MAX_CONNS));
        held.clear();
        let mut p = seg(TCP_SYN, 1000, 0, 64240, &[], &[]);
        p[20..22].copy_from_slice(&999u16.to_be_bytes());
        let parsed = packet::parse(&p).unwrap();
        assert!(!matches!(st.input(&p, &parsed, Instant::now()), Verdict::Reject));
    });
}

#[test]
fn segment_cap_below_mss_after_nobufs() {
    run(async {
        let mut st = Stack::new(65535);
        let opts = [2, 4, 0xff, 0xd7, 1, 3, 3, 7, 1, 1, 4, 2];
        let p = seg(TCP_SYN, 1000, 0, 65535, &opts, &[]);
        st.input(&p, &packet::parse(&p).unwrap(), Instant::now());
        let mut raw: Vec<Vec<u8>> = Vec::new();
        st.flush(Instant::now(), &mut |parts| raw.push(parts.concat()), false);
        let iss = u32::from_be_bytes(raw[0][24..28].try_into().unwrap());
        let p = seg(TCP_ACK, 1001, iss + 1, 0xffff, &[], &[]);
        let Verdict::Accept(mut s) = st.input(&p, &packet::parse(&p).unwrap(), Instant::now()) else { panic!() };
        st.set_seg_cap(16 * 1024 - 612);
        s.write_all(&vec![7u8; 60_000]).await.unwrap();
        raw.clear();
        st.flush(Instant::now(), &mut |parts| raw.push(parts.concat()), false);
        assert!(raw.len() >= 4, "{} сегментов", raw.len());
        assert!(raw.iter().all(|pk| pk.len() <= 16 * 1024 - 512), "{:?}", raw.iter().map(Vec::len).collect::<Vec<_>>());
        assert_eq!(raw.iter().map(|pk| pk.len() - 40).sum::<usize>(), 60_000);
        drop(s);
    });
}
