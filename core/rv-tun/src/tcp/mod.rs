mod buf;
pub(crate) mod csum;

pub(crate) use buf::trim as trim_pool;

use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::packet::{PROTO_TCP, Parsed, TCP_ACK, TCP_FIN, TCP_RST, TCP_SYN};
use buf::Chain;
use csum::Csum;

const TCP_PSH: u8 = 0x08;

pub(crate) type Key = (SocketAddr, SocketAddr);

const RTO_MIN: Duration = Duration::from_millis(200);
const RTO_SYN: Duration = Duration::from_secs(1);
const RTO_MAX: Duration = Duration::from_secs(30);
const PROBE_MAX: Duration = Duration::from_secs(120);
const DATA_RETRIES: u8 = 10;
const SYN_RETRIES: u8 = 4;
const LINGER: Duration = Duration::from_secs(60);
const SLACK: Duration = Duration::from_millis(16);
const HOLD: Duration = Duration::from_millis(10);
const HOLD_BYTES: usize = 64 * 1024;
const MAX_CONNS: usize = 8192;
const TX_CAP: usize = 256 * 1024;
const RX_MIN: usize = 256 * 1024;
const RX_MAX: usize = 1024 * 1024;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const MEM_BUDGET: usize = 640 << 20;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const MEM_FULL: usize = 256 << 20;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const MEM_MID: usize = 448 << 20;
#[cfg(any(target_os = "android", target_os = "ios"))]
const MEM_BUDGET: usize = 256 << 20;
#[cfg(any(target_os = "android", target_os = "ios"))]
const MEM_FULL: usize = 64 << 20;
#[cfg(any(target_os = "android", target_os = "ios"))]
const MEM_MID: usize = 128 << 20;
const CAP_MID: usize = 64 * 1024;
const CAP_LOW: usize = 16 * 1024;

fn seq_lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}
fn seq_le(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) <= 0
}

pub(crate) struct Seg<'a> {
    seq: u32,
    ack: u32,
    flags: u8,
    wnd: u16,
    data: &'a [u8],
    mss: Option<u16>,
    ws: Option<u8>,
    sack: bool,
}

pub(crate) fn segment<'a>(pkt: &'a [u8], p: &Parsed) -> Seg<'a> {
    let h = &pkt[p.ip_header_len..p.payload_offset];
    let be32 = |o: usize| u32::from_be_bytes([h[o], h[o + 1], h[o + 2], h[o + 3]]);
    let flags = h[13];
    let (mut mss, mut ws, mut sack) = (None, None, false);
    if flags & TCP_SYN != 0 {
        let mut o = &h[20..];
        while let [kind, rest @ ..] = o {
            match *kind {
                0 => break,
                1 => o = rest,
                _ => {
                    let Some(&len) = rest.first() else { break };
                    let len = len as usize;
                    if len < 2 || len > o.len() {
                        break;
                    }
                    match (*kind, len) {
                        (2, 4) => mss = Some(u16::from_be_bytes([o[2], o[3]])),
                        (3, 3) => ws = Some(o[2].min(14)),
                        (4, 2) => sack = true,
                        _ => {}
                    }
                    o = &o[len..];
                }
            }
        }
    }
    Seg {
        seq: be32(4),
        ack: be32(8),
        flags,
        wnd: u16::from_be_bytes([h[14], h[15]]),
        data: &pkt[p.payload_offset..p.total_len],
        mss,
        ws,
        sack,
    }
}

struct Ooo {
    seq: u32,
    data: Vec<u8>,
    fin: bool,
}

const OOO_MAX: usize = 64;

pub(crate) type Out<'a> = dyn FnMut(&[&[u8]]) + 'a;

struct Tcb {
    app: SocketAddr,
    us: SocketAddr,
    established: bool,
    irs: u32,
    rcv_nxt: u32,
    rx: Chain,
    rx_cap: usize,
    rx_fin: bool,
    adv_right: u32,
    rcv_ws: u8,
    mss_in: usize,
    ooo: Vec<Ooo>,
    ooo_bytes: usize,
    sack_ok: bool,
    dup_out: u8,
    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    snd_wnd: u32,
    snd_wl1: u32,
    snd_wl2: u32,
    max_wnd: u32,
    snd_ws: u8,
    mss: usize,
    tx: Chain,
    tx_cap: usize,
    fin_queued: bool,
    fin_sent: bool,
    dupacks: u8,
    rtx_one: bool,
    ack_now: bool,
    defer: bool,
    syn_ack: bool,
    probe: bool,
    rst: bool,
    rto: Duration,
    rto_at: Option<Instant>,
    retries: u8,
    linger_at: Option<Instant>,
    hold_at: Option<Instant>,
    reader: Option<Waker>,
    writer: Option<Waker>,
    error: Option<io::ErrorKind>,
    closed: bool,
    socket: bool,
}

impl Tcb {
    fn in_flight(&self) -> u32 {
        self.snd_nxt.wrapping_sub(self.snd_una)
    }

    fn sent_data(&self) -> usize {
        let f = self.in_flight();
        (if self.fin_sent { f.saturating_sub(1) } else { f }) as usize
    }

    fn unsent(&self) -> usize {
        self.tx.len().saturating_sub(self.sent_data())
    }

    fn rcv_room(&self) -> usize {
        self.rx_cap.saturating_sub(self.rx.len())
    }

    fn promised(&self) -> usize {
        let p = self.adv_right.wrapping_sub(self.rcv_nxt);
        if (p as i32) < 0 { 0 } else { p as usize }
    }

    fn window_field(&mut self) -> u16 {
        let w = self.rcv_room().max(self.promised()) as u32;
        let f = (w >> self.rcv_ws).min(0xffff);
        let right = self.rcv_nxt.wrapping_add(f << self.rcv_ws);
        if seq_lt(self.adv_right, right) {
            self.adv_right = right;
        }
        f as u16
    }

    fn window_update_due(&self) -> bool {
        if !self.established || self.rx_fin || self.closed {
            return false;
        }
        let cur = self.promised();
        let room = self.rcv_room();
        2 * cur <= self.rx_cap && room >= 2 * cur && room >= self.mss_in.min(self.rx_cap / 2)
    }

    fn send_room(&self) -> usize {
        let room = self.snd_una.wrapping_add(self.snd_wnd).wrapping_sub(self.snd_nxt);
        if (room as i32) < 0 { 0 } else { room as usize }
    }

    fn wants_output(&self) -> bool {
        if self.closed {
            return false;
        }
        self.ack_now
            || self.rtx_one
            || self.dup_out > 0
            || self.syn_ack
            || self.rst
            || self.probe
            || (self.established
                && !self.fin_sent
                && ((self.unsent() > 0 && self.send_room() > 0) || (self.fin_queued && self.unsent() == 0)))
    }

    fn wake_reader(&mut self) {
        if let Some(w) = self.reader.take() {
            w.wake();
        }
    }

    fn wake_writer(&mut self) {
        if let Some(w) = self.writer.take() {
            w.wake();
        }
    }

    fn fail(&mut self, kind: io::ErrorKind) {
        self.error.get_or_insert(kind);
        self.closed = true;
        self.tx.clear();
        self.wake_reader();
        self.wake_writer();
    }

    fn deadline(&self) -> Option<Instant> {
        [self.rto_at, self.linger_at, self.hold_at].into_iter().flatten().min()
    }

    fn precise(&self, d: Instant) -> bool {
        self.hold_at == Some(d)
    }

    fn check_done(&mut self) {
        if !self.closed && !self.ack_now && self.established && self.rx_fin && self.fin_sent && self.snd_una == self.snd_nxt {
            self.closed = true;
            self.rto_at = None;
            self.linger_at = None;
            self.wake_reader();
            self.wake_writer();
        }
    }

    fn input(&mut self, s: &Seg, now: Instant) -> bool {
        if self.closed || self.rst {
            return false;
        }
        if s.flags & TCP_RST != 0 {
            let off = s.seq.wrapping_sub(self.rcv_nxt) as usize;
            if off == 0 || (self.established && off < self.rcv_room().max(1)) {
                self.fail(io::ErrorKind::ConnectionReset);
            }
            return false;
        }
        if s.flags & TCP_SYN != 0 {
            if !self.established && s.flags & TCP_ACK == 0 && s.seq == self.irs {
                self.syn_ack = true;
            } else {
                self.ack_now = true;
            }
            return false;
        }
        if s.flags & TCP_ACK == 0 {
            return false;
        }
        let mut accepted = false;
        if !self.established {
            if s.ack != self.iss.wrapping_add(1) {
                return false;
            }
            self.established = true;
            self.socket = true;
            accepted = true;
            self.snd_una = s.ack;
            self.snd_wnd = (s.wnd as u32) << self.snd_ws;
            self.max_wnd = self.snd_wnd;
            self.snd_wl1 = s.seq;
            self.snd_wl2 = s.ack;
            self.rto_at = None;
            self.retries = 0;
            self.rto = RTO_MIN;
            self.syn_ack = false;
        } else {
            self.on_ack(s, now);
        }
        if !s.data.is_empty() || s.flags & TCP_FIN != 0 {
            self.on_data(s);
        } else if s.seq != self.rcv_nxt {
            self.ack_now = true;
        }
        accepted
    }

    fn on_ack(&mut self, s: &Seg, now: Instant) {
        let acked = s.ack.wrapping_sub(self.snd_una);
        if (acked as i32) < 0 {
            return;
        }
        if acked > self.in_flight() {
            self.ack_now = true;
            return;
        }
        let wnd = (s.wnd as u32) << self.snd_ws;
        let wnd_changed = wnd != self.snd_wnd;
        if seq_lt(self.snd_wl1, s.seq) || (self.snd_wl1 == s.seq && seq_le(self.snd_wl2, s.ack)) {
            self.snd_wnd = wnd;
            self.max_wnd = self.max_wnd.max(wnd);
            self.snd_wl1 = s.seq;
            self.snd_wl2 = s.ack;
        }
        if acked > 0 {
            let data = (acked as usize).min(self.sent_data());
            self.tx.consume(data);
            self.snd_una = s.ack;
            self.dupacks = 0;
            self.rtx_one = false;
            self.retries = 0;
            self.rto = RTO_MIN;
            self.probe = false;
            self.rto_at = (self.in_flight() > 0).then(|| now + self.rto);
            if data > 0 && self.tx.len() <= self.tx_cap / 2 {
                self.wake_writer();
            }
        } else if self.in_flight() > 0 && s.data.is_empty() && s.flags & TCP_FIN == 0 && !wnd_changed {
            self.dupacks = self.dupacks.saturating_add(1);
            if self.dupacks == 3 {
                self.rtx_one = true;
            }
        }
        if self.in_flight() == 0 && self.snd_wnd > 0 && self.unsent() > 0 {
            self.rto_at = None;
            self.probe = false;
            self.rto = RTO_MIN;
        }
    }

    fn retransmit(&mut self) {
        self.snd_nxt = self.snd_una;
        self.fin_sent = false;
        self.rtx_one = false;
    }

    fn on_data(&mut self, s: &Seg) {
        self.ack_now = true;
        let fin = s.flags & TCP_FIN != 0;
        let off = self.rcv_nxt.wrapping_sub(s.seq);
        if (off as i32) < 0 {
            if !self.rx_fin {
                self.ooo_insert(s.seq, s.data, fin);
            }
            self.dup_out = (self.dup_out + 1).min(3);
            return;
        }
        let off = off as usize;
        if off <= s.data.len() && !self.rx_fin {
            let data = &s.data[off..];
            if !self.socket && !data.is_empty() {
                self.rst = true;
                return;
            }
            if self.accept(data, fin) && !self.ooo.is_empty() {
                self.ooo_drain();
            }
        }
        if off > 0 {
            self.defer = false;
        }
    }

    fn accept(&mut self, data: &[u8], fin: bool) -> bool {
        let take = data.len().min(self.rcv_room());
        if take > 0 {
            self.rx.push(&data[..take]);
            self.rcv_nxt = self.rcv_nxt.wrapping_add(take as u32);
            if self.reader.is_some() {
                self.defer = true;
            }
            self.wake_reader();
        }
        if take < data.len() {
            return false;
        }
        if fin {
            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
            self.rx_fin = true;
            self.ooo.clear();
            self.ooo_bytes = 0;
            self.wake_reader();
            return false;
        }
        true
    }

    fn ooo_insert(&mut self, seq: u32, data: &[u8], fin: bool) {
        let rel = seq.wrapping_sub(self.rcv_nxt) as usize;
        let room = self.rcv_room().saturating_sub(self.ooo_bytes);
        if rel >= self.rcv_room() || self.ooo.len() >= OOO_MAX || (data.is_empty() && !fin) {
            return;
        }
        let fits = data.len().min(self.rcv_room() - rel).min(room);
        let fin = fin && fits == data.len();
        if fits == 0 && !fin {
            return;
        }
        let at = self.ooo.partition_point(|e| seq_lt(e.seq, seq));
        if self.ooo.get(at).is_some_and(|e| e.seq == seq && e.data.len() >= fits) {
            return;
        }
        self.ooo_bytes += fits;
        self.ooo.insert(at, Ooo { seq, data: data[..fits].to_vec(), fin });
    }

    fn ooo_drain(&mut self) {
        while let Some(first) = self.ooo.first() {
            let off = self.rcv_nxt.wrapping_sub(first.seq);
            if (off as i32) < 0 {
                break;
            }
            let e = self.ooo.remove(0);
            self.ooo_bytes -= e.data.len();
            let off = off as usize;
            if off > e.data.len() {
                continue;
            }
            if !self.accept(&e.data[off..], e.fin) {
                break;
            }
        }
    }

    fn sack_opts(&self, buf: &mut [u8; 28]) -> usize {
        if !self.sack_ok || self.ooo.is_empty() {
            return 0;
        }
        let mut blocks: [(u32, u32); 3] = [(0, 0); 3];
        let mut n = 0;
        for e in &self.ooo {
            let (l, r) = (e.seq, e.seq.wrapping_add(e.data.len() as u32 + e.fin as u32));
            if n > 0 && seq_le(l, blocks[n - 1].1) {
                if seq_lt(blocks[n - 1].1, r) {
                    blocks[n - 1].1 = r;
                }
                continue;
            }
            if n == 3 {
                break;
            }
            blocks[n] = (l, r);
            n += 1;
        }
        buf[0] = 1;
        buf[1] = 1;
        buf[2] = 5;
        buf[3] = (2 + 8 * n) as u8;
        for (i, (l, r)) in blocks[..n].iter().enumerate() {
            buf[4 + 8 * i..8 + 8 * i].copy_from_slice(&l.to_be_bytes());
            buf[8 + 8 * i..12 + 8 * i].copy_from_slice(&r.to_be_bytes());
        }
        4 + 8 * n
    }

    fn on_timeout(&mut self, now: Instant) {
        if self.linger_at.is_some_and(|t| t <= now) {
            self.linger_at = None;
            self.rst = true;
            return;
        }
        if !self.rto_at.is_some_and(|t| t <= now) {
            return;
        }
        self.rto_at = None;
        let max = if self.established && self.in_flight() == 0 && self.unsent() > 0 { PROBE_MAX } else { RTO_MAX };
        self.rto = (self.rto * 2).min(max);
        if !self.established {
            self.retries += 1;
            if self.retries > SYN_RETRIES {
                self.closed = true;
            } else {
                self.syn_ack = true;
            }
        } else if self.in_flight() > 0 {
            self.retries += 1;
            if self.retries > DATA_RETRIES {
                self.error.get_or_insert(io::ErrorKind::TimedOut);
                self.rst = true;
            } else {
                self.retransmit();
            }
        } else if self.unsent() > 0 {
            self.probe = true;
        }
    }

    fn output(&mut self, now: Instant, out: &mut Out, defer: bool, seg_cap: usize) -> bool {
        if self.closed {
            return false;
        }
        if self.rst {
            let wnd = self.window_field();
            self.emit(TCP_RST | TCP_ACK, self.snd_nxt, wnd, &[], 0, 0, out);
            self.rst = false;
            self.fail(io::ErrorKind::ConnectionAborted);
            return false;
        }
        if !self.established {
            if self.syn_ack {
                self.syn_ack = false;
                self.ack_now = false;
                let wnd = self.rcv_room().min(0xffff) as u16;
                self.adv_right = self.rcv_nxt.wrapping_add(wnd as u32);
                let mut opts = [0u8; 12];
                opts[0] = 2;
                opts[1] = 4;
                opts[2..4].copy_from_slice(&(self.mss_in.min(0xffff) as u16).to_be_bytes());
                let mut n = 4;
                if self.rcv_ws > 0 || self.snd_ws > 0 {
                    opts[n..n + 4].copy_from_slice(&[1, 3, 3, self.rcv_ws]);
                    n += 4;
                }
                if self.sack_ok {
                    opts[n..n + 4].copy_from_slice(&[1, 1, 4, 2]);
                    n += 4;
                }
                self.emit(TCP_SYN | TCP_ACK, self.iss, wnd, &opts[..n], 0, 0, out);
                if self.rto_at.is_none() {
                    self.rto_at = Some(now + self.rto);
                }
            }
            return false;
        }
        let mut sb = [0u8; 28];
        let k = self.sack_opts(&mut sb);
        let mss = self.mss.min(seg_cap) - k;
        if self.rtx_one {
            self.rtx_one = false;
            let wnd = self.window_field();
            let data = self.sent_data();
            if data > 0 {
                let len = data.min(mss);
                let flags = if len == data { TCP_ACK | TCP_PSH } else { TCP_ACK };
                self.emit(flags, self.snd_una, wnd, &sb[..k], 0, len, out);
                self.ack_now = false;
            } else if self.fin_sent {
                self.emit(TCP_FIN | TCP_ACK, self.snd_una, wnd, &[], 0, 0, out);
                self.ack_now = false;
            }
        }
        let mut sent_any = false;
        let mut held = false;
        while !self.fin_sent {
            let unsent = self.unsent();
            let room = self.send_room();
            if unsent > 0 && room > 0 {
                let len = unsent.min(room).min(mss);
                if len < mss && len < unsent && self.in_flight() > 0 && (len as u32) < self.max_wnd / 2 {
                    break;
                }
                if !sent_any && len == unsent && len < HOLD_BYTES && !self.fin_queued && self.in_flight() > 0 {
                    let at = *self.hold_at.get_or_insert(now + HOLD);
                    if now < at {
                        held = true;
                        break;
                    }
                }
                let fin = self.fin_queued && len == unsent;
                let mut flags = TCP_ACK;
                if len == unsent {
                    flags |= TCP_PSH;
                }
                if fin {
                    flags |= TCP_FIN;
                }
                let wnd = self.window_field();
                let off = self.sent_data();
                self.emit(flags, self.snd_nxt, wnd, &sb[..k], off, len, out);
                self.snd_nxt = self.snd_nxt.wrapping_add(len as u32 + fin as u32);
                self.fin_sent = fin;
                sent_any = true;
                continue;
            }
            if unsent == 0 && self.fin_queued {
                let wnd = self.window_field();
                self.emit(TCP_FIN | TCP_ACK, self.snd_nxt, wnd, &[], 0, 0, out);
                self.snd_nxt = self.snd_nxt.wrapping_add(1);
                self.fin_sent = true;
                sent_any = true;
            }
            break;
        }
        if !held {
            self.hold_at = None;
        }
        if sent_any {
            self.ack_now = false;
            self.probe = false;
            if self.rto_at.is_none() {
                self.rto_at = Some(now + self.rto);
            }
        } else if self.unsent() > 0 && self.in_flight() == 0 {
            if self.probe {
                self.probe = false;
                let wnd = self.window_field();
                self.emit(TCP_ACK, self.snd_una.wrapping_sub(1), wnd, &[], 0, 0, out);
            }
            if self.rto_at.is_none() {
                self.rto_at = Some(now + self.rto);
            }
        }
        if sent_any {
            self.defer = false;
        }
        if self.dup_out > 0 {
            let wnd = self.window_field();
            for _ in 0..self.dup_out {
                self.emit(TCP_ACK, self.snd_nxt, wnd, &sb[..k], 0, 0, out);
            }
            self.dup_out = 0;
            self.ack_now = false;
        }
        if self.ack_now {
            if defer && self.defer && !self.rx_fin && self.ooo.is_empty() {
                return true;
            }
            self.ack_now = false;
            self.defer = false;
            let wnd = self.window_field();
            self.emit(TCP_ACK, self.snd_nxt, wnd, &sb[..k], 0, 0, out);
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(&self, flags: u8, seq: u32, wnd: u16, opts: &[u8], off: usize, len: usize, out: &mut Out) {
        let mut hdr = [0u8; 40 + 20 + 40];
        let ip = if self.us.is_ipv4() { 20 } else { 40 };
        let tcp_len = 20 + opts.len() + len;
        let mut c = Csum::default();
        match (self.us.ip(), self.app.ip()) {
            (IpAddr::V4(s), IpAddr::V4(d)) => {
                hdr[0] = 0x45;
                hdr[2..4].copy_from_slice(&((20 + tcp_len) as u16).to_be_bytes());
                hdr[6] = 0x40;
                hdr[8] = 64;
                hdr[9] = PROTO_TCP;
                hdr[12..16].copy_from_slice(&s.octets());
                hdr[16..20].copy_from_slice(&d.octets());
                let mut ic = Csum::default();
                ic.add(&hdr[..20]);
                hdr[10..12].copy_from_slice(&ic.finish().to_be_bytes());
                c.add(&hdr[12..20]);
            }
            (IpAddr::V6(s), IpAddr::V6(d)) => {
                hdr[0] = 0x60;
                hdr[4..6].copy_from_slice(&(tcp_len as u16).to_be_bytes());
                hdr[6] = PROTO_TCP;
                hdr[7] = 64;
                hdr[8..24].copy_from_slice(&s.octets());
                hdr[24..40].copy_from_slice(&d.octets());
                c.add(&hdr[8..40]);
            }
            _ => return,
        }
        c.add_u16(PROTO_TCP as u16);
        c.add_u16(tcp_len as u16);
        let t = &mut hdr[ip..];
        t[0..2].copy_from_slice(&self.us.port().to_be_bytes());
        t[2..4].copy_from_slice(&self.app.port().to_be_bytes());
        t[4..8].copy_from_slice(&seq.to_be_bytes());
        t[8..12].copy_from_slice(&self.rcv_nxt.to_be_bytes());
        t[12] = (((20 + opts.len()) / 4) as u8) << 4;
        t[13] = flags;
        t[14..16].copy_from_slice(&wnd.to_be_bytes());
        t[20..20 + opts.len()].copy_from_slice(opts);
        let hl = ip + 20 + opts.len();
        let mut parts: [&[u8]; 8] = [&[]; 8];
        let mut n = 1;
        if len > 0 {
            self.tx.slices(off, len, |s| {
                parts[n] = s;
                n += 1;
            });
        }
        c.add(&hdr[ip..hl]);
        for p in &parts[1..n] {
            c.add(p);
        }
        let sum = c.finish();
        hdr[ip + 16..ip + 18].copy_from_slice(&sum.to_be_bytes());
        parts[0] = &hdr[..hl];
        out(&parts[..n]);
    }
}

pub(crate) struct Conn {
    key: Key,
    tcb: RefCell<Tcb>,
    queued: Cell<bool>,
    timer: Cell<Option<Instant>>,
    live: Rc<Cell<usize>>,
    committed: Rc<Cell<usize>>,
    reserved: usize,
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.live.set(self.live.get().saturating_sub(1));
        self.committed.set(self.committed.get().saturating_sub(self.reserved));
    }
}

#[derive(Default)]
pub(crate) struct Shared {
    dirty: RefCell<Vec<Rc<Conn>>>,
    notify: Notify,
}

impl Shared {
    pub(crate) async fn notified(&self) {
        self.notify.notified().await
    }

    fn mark(&self, c: &Rc<Conn>) {
        if !c.queued.replace(true) {
            let mut d = self.dirty.borrow_mut();
            let wake = d.is_empty();
            d.push(c.clone());
            drop(d);
            if wake {
                self.notify.notify_one();
            }
        }
    }
}

pub(crate) enum Verdict {
    None,
    Accept(Socket),
    Reject,
}

pub(crate) struct Stack {
    conns: HashMap<Key, Rc<Conn>>,
    live: Rc<Cell<usize>>,
    committed: Rc<Cell<usize>>,
    shared: Rc<Shared>,
    timers: BinaryHeap<Reverse<(Instant, Key)>>,
    due: Vec<(Instant, Key)>,
    deferred: Vec<Rc<Conn>>,
    epoch: Instant,
    mss4: usize,
    mss6: usize,
    rng: u64,
    seg_cap: usize,
}

impl Stack {
    pub(crate) fn new(mtu: usize) -> Self {
        let mtu = mtu.clamp(1280, 65535);
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Self {
            conns: HashMap::new(),
            live: Rc::new(Cell::new(0)),
            committed: Rc::new(Cell::new(0)),
            shared: Rc::new(Shared::default()),
            timers: BinaryHeap::new(),
            due: Vec::new(),
            deferred: Vec::new(),
            epoch: Instant::now(),
            mss4: mtu - 40,
            mss6: mtu - 60,
            rng: seed | 1,
            seg_cap: usize::MAX,
        }
    }

    pub(crate) fn set_seg_cap(&mut self, cap: usize) {
        self.seg_cap = cap.max(536);
    }

    fn random(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 16) as u32
    }

    pub(crate) fn input(&mut self, pkt: &[u8], p: &Parsed, now: Instant) -> Verdict {
        let s = segment(pkt, p);
        let key = (SocketAddr::new(p.src, p.src_port), SocketAddr::new(p.dst, p.dst_port));
        let Some(conn) = self.conns.get(&key).cloned() else {
            if s.flags & TCP_RST != 0 {
                return Verdict::None;
            }
            if s.flags & (TCP_SYN | TCP_ACK) != TCP_SYN || self.live.get() >= MAX_CONNS || self.committed.get() + 2 * CAP_LOW > MEM_BUDGET {
                return Verdict::Reject;
            }
            let c = self.open(key, &s);
            self.conns.insert(key, c.clone());
            self.shared.mark(&c);
            return Verdict::None;
        };
        let accepted = {
            let mut t = conn.tcb.borrow_mut();
            let a = t.input(&s, now);
            if t.wants_output() {
                drop(t);
                self.shared.mark(&conn);
            }
            a
        };
        self.settle(&conn);
        if accepted { Verdict::Accept(Socket { conn, shared: self.shared.clone() }) } else { Verdict::None }
    }

    fn open(&mut self, key: Key, s: &Seg) -> Rc<Conn> {
        let v4 = key.0.is_ipv4();
        let mss_in = if v4 { self.mss4 } else { self.mss6 };
        let mss = (s.mss.map_or(if v4 { 536 } else { 1220 }, |m| m as usize)).clamp(64, mss_in);
        let used = self.committed.get();
        let (rx_cap, tx_cap) = if used < MEM_FULL {
            ((16 * mss_in).clamp(RX_MIN, RX_MAX), TX_CAP)
        } else if used < MEM_MID {
            (CAP_MID, CAP_MID)
        } else {
            (CAP_LOW, CAP_LOW)
        };
        let reserved = rx_cap + tx_cap;
        self.committed.set(used + reserved);
        let (rcv_ws, snd_ws) = match s.ws {
            Some(ws) => ((rx_cap >> 16).max(1).ilog2() as u8 + 1, ws),
            None => (0, 0),
        };
        let iss = self.random();
        let rcv_nxt = s.seq.wrapping_add(1);
        self.live.set(self.live.get() + 1);
        Rc::new(Conn {
            key,
            queued: Cell::new(false),
            timer: Cell::new(None),
            live: self.live.clone(),
            committed: self.committed.clone(),
            reserved,
            tcb: RefCell::new(Tcb {
                app: key.0,
                us: key.1,
                established: false,
                irs: s.seq,
                rcv_nxt,
                rx: Chain::default(),
                rx_cap,
                rx_fin: false,
                adv_right: rcv_nxt,
                rcv_ws,
                mss_in,
                ooo: Vec::new(),
                ooo_bytes: 0,
                sack_ok: s.sack,
                dup_out: 0,
                iss,
                snd_una: iss,
                snd_nxt: iss.wrapping_add(1),
                snd_wnd: s.wnd as u32,
                snd_wl1: s.seq,
                snd_wl2: iss,
                max_wnd: s.wnd as u32,
                snd_ws,
                mss,
                tx: Chain::default(),
                tx_cap,
                fin_queued: false,
                fin_sent: false,
                dupacks: 0,
                rtx_one: false,
                ack_now: false,
                defer: false,
                syn_ack: true,
                probe: false,
                rst: false,
                rto: RTO_SYN,
                rto_at: None,
                retries: 0,
                linger_at: None,
                hold_at: None,
                reader: None,
                writer: None,
                error: None,
                closed: false,
                socket: false,
            }),
        })
    }

    pub(crate) fn flush(&mut self, now: Instant, out: &mut Out, defer: bool) {
        loop {
            let list = std::mem::take(&mut *self.shared.dirty.borrow_mut());
            if list.is_empty() {
                return;
            }
            for c in list {
                c.queued.set(false);
                if c.tcb.borrow_mut().output(now, out, defer, self.seg_cap) {
                    self.deferred.push(c.clone());
                }
                self.settle(&c);
            }
        }
    }

    pub(crate) fn has_deferred(&self) -> bool {
        !self.deferred.is_empty()
    }

    pub(crate) fn flush_deferred(&mut self, now: Instant, out: &mut Out) {
        for c in std::mem::take(&mut self.deferred) {
            self.shared.mark(&c);
        }
        self.flush(now, out, false);
    }

    pub(crate) fn has_dirty(&self) -> bool {
        !self.shared.dirty.borrow().is_empty()
    }

    pub(crate) fn shared(&self) -> Rc<Shared> {
        self.shared.clone()
    }

    pub(crate) fn next_timer(&mut self) -> Option<Instant> {
        while let Some(&Reverse((t, key))) = self.timers.peek() {
            let Some(c) = self.conns.get(&key) else {
                self.timers.pop();
                continue;
            };
            if c.timer.get() != Some(t) {
                self.timers.pop();
                continue;
            }
            let due = {
                let t = c.tcb.borrow();
                t.deadline().map(|d| if t.precise(d) { d } else { self.round(d) })
            };
            match due {
                Some(d) if d <= t => return Some(t),
                Some(d) => {
                    self.timers.pop();
                    self.timers.push(Reverse((d, key)));
                    c.timer.set(Some(d));
                }
                None => {
                    self.timers.pop();
                    c.timer.set(None);
                }
            }
        }
        None
    }

    pub(crate) fn on_timer(&mut self, now: Instant, out: &mut Out) {
        let mut due = std::mem::take(&mut self.due);
        while let Some(&Reverse((t, key))) = self.timers.peek() {
            if t > now {
                break;
            }
            self.timers.pop();
            due.push((t, key));
        }
        for &(t, key) in &due {
            let Some(c) = self.conns.get(&key).cloned() else { continue };
            if c.timer.get() != Some(t) {
                continue;
            }
            c.timer.set(None);
            {
                let mut tcb = c.tcb.borrow_mut();
                tcb.on_timeout(now);
                tcb.output(now, out, false, self.seg_cap);
            }
            self.settle(&c);
        }
        due.clear();
        if due.capacity() > 1024 {
            due.shrink_to(64);
        }
        self.due = due;
        if self.timers.is_empty() && self.timers.capacity() > 1024 {
            self.timers.shrink_to(64);
        }
    }

    fn round(&self, d: Instant) -> Instant {
        let since = d.saturating_duration_since(self.epoch).as_nanos();
        let slack = SLACK.as_nanos();
        self.epoch + Duration::from_nanos((since.div_ceil(slack) * slack) as u64)
    }

    fn settle(&mut self, c: &Rc<Conn>) {
        let mut t = c.tcb.borrow_mut();
        t.check_done();
        if t.closed {
            drop(t);
            if self.conns.get(&c.key).is_some_and(|x| Rc::ptr_eq(x, c)) {
                self.conns.remove(&c.key);
                if self.conns.is_empty() && self.conns.capacity() > 1024 {
                    self.conns.shrink_to(64);
                }
            }
            return;
        }
        if let Some(d) = t.deadline() {
            let d = if t.precise(d) { d } else { self.round(d) };
            if c.timer.get().is_none_or(|q| d < q) {
                self.timers.push(Reverse((d, c.key)));
                c.timer.set(Some(d));
            }
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        for c in self.conns.values() {
            c.tcb.borrow_mut().fail(io::ErrorKind::ConnectionAborted);
        }
    }
}

pub struct Socket {
    conn: Rc<Conn>,
    shared: Rc<Shared>,
}

pub struct Handle {
    conn: Rc<Conn>,
    shared: Rc<Shared>,
}

impl Handle {
    pub fn reset(&self) {
        let mut t = self.conn.tcb.borrow_mut();
        if !t.closed {
            t.rst = true;
            drop(t);
            self.shared.mark(&self.conn);
        }
    }
}

impl Socket {
    pub(crate) fn handle(&self) -> Handle {
        Handle { conn: self.conn.clone(), shared: self.shared.clone() }
    }

    pub fn peer(&self) -> (SocketAddr, SocketAddr) {
        self.conn.key
    }
}

impl Socket {
    pub fn poll_fill(&mut self, cx: &mut Context<'_>, src: Pin<&mut dyn AsyncRead>) -> Poll<io::Result<usize>> {
        let mut t = self.conn.tcb.borrow_mut();
        if let Some(e) = t.error {
            return Poll::Ready(Err(e.into()));
        }
        if t.closed || t.fin_queued || t.rst {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        let room = t.tx_cap.saturating_sub(t.tx.len());
        if room == 0 {
            store(&mut t.writer, cx);
            return Poll::Pending;
        }
        let space = t.tx.tail_space(room);
        let mut rb = ReadBuf::new(space);
        let r = src.poll_read(cx, &mut rb);
        let n = rb.filled().len();
        t.tx.commit(n);
        match r {
            Poll::Ready(Ok(())) => {
                drop(t);
                if n > 0 {
                    self.shared.mark(&self.conn);
                }
                Poll::Ready(Ok(n))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    pub fn poll_drain(&mut self, cx: &mut Context<'_>, dst: Pin<&mut dyn AsyncWrite>) -> Poll<io::Result<usize>> {
        let mut t = self.conn.tcb.borrow_mut();
        if t.rx.is_empty() {
            if t.rx_fin || (t.closed && t.error.is_none()) {
                return Poll::Ready(Ok(0));
            }
            if let Some(e) = t.error {
                return Poll::Ready(Err(e.into()));
            }
            store(&mut t.reader, cx);
            return Poll::Pending;
        }
        let n = match dst.poll_write(cx, t.rx.front()) {
            Poll::Ready(Ok(0)) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
            Poll::Ready(Ok(n)) => n,
            other => return other,
        };
        t.rx.consume(n);
        if t.window_update_due() {
            t.ack_now = true;
            drop(t);
            self.shared.mark(&self.conn);
        }
        Poll::Ready(Ok(n))
    }
}

fn store(slot: &mut Option<Waker>, cx: &Context<'_>) {
    match slot {
        Some(w) if w.will_wake(cx.waker()) => {}
        _ => *slot = Some(cx.waker().clone()),
    }
}

impl AsyncRead for Socket {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let mut t = this.conn.tcb.borrow_mut();
        if !t.rx.is_empty() {
            let n = t.rx.read(buf.initialize_unfilled());
            buf.advance(n);
            if t.window_update_due() {
                t.ack_now = true;
                drop(t);
                this.shared.mark(&this.conn);
            }
            return Poll::Ready(Ok(()));
        }
        if t.rx_fin {
            return Poll::Ready(Ok(()));
        }
        if let Some(e) = t.error {
            return Poll::Ready(Err(e.into()));
        }
        if t.closed {
            return Poll::Ready(Ok(()));
        }
        store(&mut t.reader, cx);
        Poll::Pending
    }
}

impl AsyncWrite for Socket {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let mut t = this.conn.tcb.borrow_mut();
        if let Some(e) = t.error {
            return Poll::Ready(Err(e.into()));
        }
        if t.closed || t.fin_queued || t.rst {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        let room = t.tx_cap.saturating_sub(t.tx.len());
        if room == 0 {
            store(&mut t.writer, cx);
            return Poll::Pending;
        }
        let n = room.min(data.len());
        t.tx.push(&data[..n]);
        drop(t);
        this.shared.mark(&this.conn);
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let mut t = this.conn.tcb.borrow_mut();
        if !t.closed && !t.fin_queued {
            t.fin_queued = true;
            drop(t);
            this.shared.mark(&this.conn);
        }
        Poll::Ready(Ok(()))
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let mut t = self.conn.tcb.borrow_mut();
        t.socket = false;
        t.reader = None;
        t.writer = None;
        if t.closed {
            return;
        }
        if !t.rx.is_empty() {
            t.rx.clear();
            t.rst = true;
        } else {
            t.fin_queued = true;
        }
        t.linger_at = Some(Instant::now() + LINGER);
        drop(t);
        self.shared.mark(&self.conn);
    }
}

#[cfg(kani)]
#[path = "proofs/tcb.rs"]
mod proofs;

#[cfg(test)]
mod tests;
