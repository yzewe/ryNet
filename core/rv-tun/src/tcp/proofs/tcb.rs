use super::*;
use crate::packet;
use std::task::Waker;

const DATA: usize = 24;
const WRITE: usize = 64;
static ZEROS: [u8; 64] = [0; 64];

#[allow(dead_code)]
pub(super) fn fake_instant() -> std::time::Instant {
    unsafe { std::mem::transmute::<[u64; 2], std::time::Instant>([1_000, 1_000]) }
}
#[allow(dead_code)]
pub(super) fn fake_system_time() -> std::time::SystemTime {
    unsafe { std::mem::transmute::<[u64; 2], std::time::SystemTime>([1_000, 1_000]) }
}
#[allow(dead_code)]
pub(super) fn fake_random_state() -> std::hash::RandomState {
    unsafe { std::mem::transmute::<[u64; 2], std::hash::RandomState>([1, 2]) }
}

fn any_dur(max_ms: u64) -> Duration {
    let ms: u64 = kani::any();
    kani::assume(ms <= max_ms);
    Duration::from_millis(ms)
}

fn addr(a: [u8; 4], port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::from(a), port)
}

fn check_out(parts: &[&[u8]], key: Key, mss: usize) {
    let pkt = parts.concat();
    let p = packet::parse(&pkt).unwrap();
    assert_eq!(p.total_len, pkt.len());
    assert_eq!(p.proto, PROTO_TCP);
    assert_eq!((SocketAddr::new(p.src, p.src_port), SocketAddr::new(p.dst, p.dst_port)), (key.1, key.0));
    let opts = p.payload_offset - p.ip_header_len - 20;
    let data = p.total_len - p.payload_offset;
    if data > 0 {
        assert!(data + opts <= mss, "сегмент с опциями длиннее MSS");
    }
}

fn check_inv(t: &Tcb) {
    if t.closed {
        return;
    }
    if t.established {
        assert!(t.sent_data() <= t.tx.len());
        assert!(t.in_flight() as usize <= t.tx.len() + 1);
    }
    assert!(t.rx.len() <= t.rx_cap);
    assert!(t.tx.len() <= t.tx_cap);
    assert!(t.ooo.len() <= OOO_MAX);
    assert_eq!(t.ooo_bytes, t.ooo.iter().map(|e| e.data.len()).sum::<usize>());
    assert!(t.ooo_bytes <= t.rx_cap);
    assert!(t.rto >= RTO_MIN && t.rto <= RTO_MAX);
    assert!(t.retries <= DATA_RETRIES + 1);
    assert!(t.snd_ws <= 14 && t.rcv_ws <= 14);
    let fin_pending = t.fin_queued && !t.fin_sent;
    if t.established && t.in_flight() == 0 && t.unsent() == 0 && !fin_pending {
        assert!(t.rto_at.is_none(), "таймер у тихого соединения");
    }
}

fn any_seg(rcv_nxt: u32, snd_una: u32) -> (u32, u32, u8, u16, usize) {
    let seq = if kani::any() { rcv_nxt.wrapping_add(kani::any::<i8>() as i32 as u32) } else { kani::any() };
    let ack = if kani::any() { snd_una.wrapping_add(kani::any::<i8>() as i32 as u32) } else { kani::any() };
    let flags: u8 = kani::any::<u8>() & (TCP_FIN | TCP_SYN | TCP_RST | TCP_ACK | TCP_PSH);
    let n: usize = kani::any();
    kani::assume(n <= DATA);
    (seq, ack, flags, kani::any(), n)
}

fn tcb_run<const STEPS: usize>() {
    let mut st = Stack::new(kani::any::<u16>() as usize);
    st.rng = kani::any::<u64>() | 1;
    let key: Key = (addr([10, 0, 0, 2], 40000), addr([1, 1, 1, 1], 443));
    let ws: Option<u8> = kani::any();
    kani::assume(ws.is_none_or(|w| w <= 14));
    let syn =
        Seg { seq: kani::any(), ack: kani::any(), flags: TCP_SYN, wnd: kani::any(), data: &[], mss: kani::any(), ws, sack: kani::any() };
    let conn = st.open(key, &syn);
    let shared = st.shared.clone();
    let mut now = st.epoch;
    let mut sock: Option<Socket> = None;
    let mut cx = Context::from_waker(Waker::noop());
    {
        let t = conn.tcb.borrow();
        assert!(t.mss >= 64 && t.mss <= t.mss_in);
        assert!(t.rx_cap >= RX_MIN && t.rx_cap <= RX_MAX);
        assert!(t.tx_cap == TX_CAP);
        assert!((0xffffusize << t.rcv_ws) >= t.rx_cap || t.rcv_ws == 0);
    }
    let mss = conn.tcb.borrow().mss;
    for _ in 0..STEPS {
        now += any_dur(120_000);
        let mut t = conn.tcb.borrow_mut();
        if t.closed {
            break;
        }
        match kani::any::<u8>() % 8 {
            0 => {
                let (seq, ack, flags, wnd, n) = any_seg(t.rcv_nxt, t.snd_una);
                let s = Seg { seq, ack, flags, wnd, data: &ZEROS[..n], mss: None, ws: None, sack: false };
                if t.input(&s, now) {
                    drop(t);
                    sock = Some(Socket { conn: conn.clone(), shared: shared.clone() });
                    t = conn.tcb.borrow_mut();
                }
                let _ = t.wants_output();
            }
            1 => {
                t.output(now, &mut |parts| check_out(parts, key, mss), kani::any(), usize::MAX);
            }
            2 => {
                t.on_timeout(now);
                t.output(now, &mut |parts| check_out(parts, key, mss), false, usize::MAX);
                t.check_done();
                if !t.closed {
                    assert!(t.deadline().is_none_or(|d| d > now), "таймер без паузы");
                }
            }
            op => {
                drop(t);
                if let Some(s) = sock.as_mut() {
                    let pin = Pin::new(&mut *s);
                    match op {
                        3 => {
                            let n: usize = kani::any();
                            kani::assume(n <= WRITE);
                            let _ = pin.poll_write(&mut cx, &ZEROS[..n]);
                        }
                        4 => {
                            let mut b = [0u8; 32];
                            let n: usize = kani::any();
                            kani::assume(n <= 32);
                            let mut rb = ReadBuf::new(&mut b[..n]);
                            let _ = pin.poll_read(&mut cx, &mut rb);
                        }
                        5 => {
                            let _ = pin.poll_shutdown(&mut cx);
                        }
                        6 => {
                            s.handle().reset();
                        }
                        _ => {
                            sock = None;
                        }
                    }
                }
                t = conn.tcb.borrow_mut();
            }
        }
        t.check_done();
        check_inv(&t);
    }
}

#[kani::proof]
#[kani::stub(std::time::Instant::now, fake_instant)]
#[kani::stub(std::time::SystemTime::now, fake_system_time)]
#[kani::stub(std::hash::RandomState::new, fake_random_state)]
#[kani::unwind(10)]
fn tcb_any_3_events() {
    tcb_run::<3>();
}

#[kani::proof]
#[kani::stub(std::time::Instant::now, fake_instant)]
#[kani::stub(std::time::SystemTime::now, fake_system_time)]
#[kani::stub(std::hash::RandomState::new, fake_random_state)]
#[kani::unwind(10)]
fn tcb_any_5_events() {
    tcb_run::<5>();
}

fn app_packet(key: Key, flags: u8, seq: u32, ack: u32, wnd: u16, opts: &[u8], data: usize) -> Vec<u8> {
    let (IpAddr::V4(a), IpAddr::V4(d)) = (key.0.ip(), key.1.ip()) else { unreachable!() };
    let tl = 20 + opts.len() + data;
    let mut p = vec![0u8; 20 + tl];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&((20 + tl) as u16).to_be_bytes());
    p[9] = PROTO_TCP;
    p[12..16].copy_from_slice(&a.octets());
    p[16..20].copy_from_slice(&d.octets());
    p[20..22].copy_from_slice(&key.0.port().to_be_bytes());
    p[22..24].copy_from_slice(&key.1.port().to_be_bytes());
    p[24..28].copy_from_slice(&seq.to_be_bytes());
    p[28..32].copy_from_slice(&ack.to_be_bytes());
    p[32] = (((20 + opts.len()) / 4) as u8) << 4;
    p[33] = flags;
    p[34..36].copy_from_slice(&wnd.to_be_bytes());
    p[40..40 + opts.len()].copy_from_slice(opts);
    p
}

fn earliest(st: &Stack) -> Option<Instant> {
    st.conns.values().filter_map(|c| c.tcb.borrow().deadline().map(|d| st.round(d))).min()
}

#[kani::proof]
#[kani::stub(std::time::Instant::now, fake_instant)]
#[kani::stub(std::time::SystemTime::now, fake_system_time)]
#[kani::stub(std::hash::RandomState::new, fake_random_state)]
#[kani::unwind(8)]
fn stack_timer_is_exact() {
    let mut st = Stack::new(1500);
    st.rng = kani::any::<u64>() | 1;
    let key: Key = (addr([10, 0, 0, 2], 40000), addr([1, 1, 1, 1], 443));
    let mut now = st.epoch;
    let irs: u32 = kani::any();
    let syn = app_packet(key, TCP_SYN, irs, 0, kani::any(), &[2, 4, 5, 0xb4], 0);
    let p = packet::parse(&syn).unwrap();
    let _ = st.input(&syn, &p, now);
    let mut sent = 0usize;
    for _ in 0..3 {
        now += any_dur(70_000);
        match kani::any::<u8>() % 4 {
            0 => {
                let iss = st.conns.get(&key).map(|c| c.tcb.borrow().iss).unwrap_or(0);
                let ack = if kani::any() { iss.wrapping_add(1) } else { kani::any() };
                let flags = kani::any::<u8>() & (TCP_ACK | TCP_FIN | TCP_RST);
                let pkt = app_packet(key, flags, irs.wrapping_add(1), ack, kani::any(), &[], 0);
                let p = packet::parse(&pkt).unwrap();
                let _ = st.input(&pkt, &p, now);
            }
            1 => st.flush(now, &mut |_| sent += 1, kani::any()),
            2 => {
                if let Some(t) = st.next_timer()
                    && t <= now
                {
                    st.on_timer(now, &mut |_| sent += 1);
                    assert!(st.next_timer().is_none_or(|n| n > now), "срок в прошлом после on_timer");
                }
            }
            _ => {
                if let Some(c) = st.conns.get(&key).cloned() {
                    let mut t = c.tcb.borrow_mut();
                    if t.established && !t.closed && !t.fin_queued {
                        t.tx.push(&ZEROS[..kani::any::<u8>() as usize % 33]);
                        drop(t);
                        st.shared.mark(&c);
                    }
                }
            }
        }
        assert_eq!(st.next_timer(), earliest(&st), "next_timer не совпал с ближайшим сроком");
    }
    let d = st.epoch + any_dur(10_000_000);
    let r = st.round(d);
    assert!(r >= d && r - d < SLACK);
}

#[kani::proof]
#[kani::stub(std::time::Instant::now, fake_instant)]
#[kani::stub(std::time::SystemTime::now, fake_system_time)]
#[kani::stub(std::hash::RandomState::new, fake_random_state)]
fn deadlines_coalesce_on_16ms_grid() {
    let st = Stack::new(1500);
    let a = st.epoch + Duration::from_micros(kani::any::<u32>() as u64);
    let b = st.epoch + Duration::from_micros(kani::any::<u32>() as u64);
    let (ra, rb) = (st.round(a), st.round(b));
    assert!(ra >= a && ra - a < SLACK);
    if a <= b {
        assert!(ra <= rb);
    }
    if a <= b && b <= ra {
        assert_eq!(ra, rb);
    }
    std::mem::forget(st);
}
