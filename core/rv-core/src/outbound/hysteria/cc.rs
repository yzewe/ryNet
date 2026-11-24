use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

use quinn::congestion::{BbrConfig, Controller, ControllerFactory};
use quinn_proto::RttEstimator;

const SLOTS: usize = 5;
const MIN_SAMPLES: u64 = 50;
const MIN_ACK_RATE: f64 = 0.8;
const CWND_MULTIPLIER: f64 = 2.0;

pub struct Factory {
    pub brutal_bps: Arc<AtomicU64>,
    pub bbr: Arc<BbrConfig>,
}

impl ControllerFactory for Factory {
    fn build(self: Arc<Self>, now: Instant, mtu: u16) -> Box<dyn Controller> {
        Box::new(Switch { bbr: self.bbr.clone().build(now, mtu), bps: self.brutal_bps.clone(), brutal: Brutal::new(now, mtu) })
    }
}

struct Switch {
    bbr: Box<dyn Controller>,
    bps: Arc<AtomicU64>,
    brutal: Brutal,
}

impl Switch {
    fn brutal(&self) -> bool {
        self.bps.load(Relaxed) != 0
    }
}

impl Controller for Switch {
    fn can_send(&self, in_flight: u64, next_datagram: u64) -> bool {
        if self.brutal() { in_flight <= self.window() } else { self.bbr.can_send(in_flight, next_datagram) }
    }

    fn on_sent(&mut self, now: Instant, bytes: u64, pn: u64) {
        if !self.brutal() {
            self.bbr.on_sent(now, bytes, pn);
        }
    }
    fn on_ack(&mut self, now: Instant, sent: Instant, bytes: u64, app_limited: bool, rtt: &RttEstimator) {
        if self.brutal() {
            self.brutal.on_ack(now, rtt.get());
        } else {
            self.bbr.on_ack(now, sent, bytes, app_limited, rtt);
        }
    }
    fn on_end_acks(&mut self, now: Instant, in_flight: u64, app_limited: bool, largest: Option<u64>) {
        if !self.brutal() {
            self.bbr.on_end_acks(now, in_flight, app_limited, largest);
        }
    }
    fn on_congestion_event(&mut self, now: Instant, sent: Instant, persistent: bool, lost_bytes: u64) {
        if !self.brutal() {
            self.bbr.on_congestion_event(now, sent, persistent, lost_bytes);
        }
    }
    fn on_packets_lost(&mut self, now: Instant, packets: u64) {
        if self.brutal() {
            self.brutal.on_loss(now, packets);
        } else {
            self.bbr.on_packets_lost(now, packets);
        }
    }
    fn pacing_rate(&self) -> Option<u64> {
        let rate = self.bps.load(Relaxed);
        if rate == 0 { self.bbr.pacing_rate() } else { Some((rate as f64 / self.brutal.ack_rate) as u64) }
    }
    fn on_mtu_update(&mut self, mtu: u16) {
        self.bbr.on_mtu_update(mtu);
        self.brutal.mtu = mtu as u64;
    }
    fn window(&self) -> u64 {
        match self.bps.load(Relaxed) {
            0 => self.bbr.window(),
            bps => self.brutal.window(bps),
        }
    }
    fn clone_box(&self) -> Box<dyn Controller> {
        Box::new(Switch { bbr: self.bbr.clone_box(), bps: self.bps.clone(), brutal: self.brutal.clone() })
    }
    fn initial_window(&self) -> u64 {
        self.bbr.initial_window()
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

#[derive(Clone, Copy, Default)]
struct Slot {
    second: u64,
    acked: u64,
    lost: u64,
}

#[derive(Clone)]
struct Brutal {
    start: Instant,
    slots: [Slot; SLOTS],
    ack_rate: f64,
    rtt: Duration,
    mtu: u64,
}

impl Brutal {
    fn new(start: Instant, mtu: u16) -> Self {
        Self { start, slots: [Slot::default(); SLOTS], ack_rate: 1.0, rtt: Duration::ZERO, mtu: mtu as u64 }
    }

    fn slot(&mut self, now: Instant) -> &mut Slot {
        let second = now.duration_since(self.start).as_secs();
        let s = &mut self.slots[(second % SLOTS as u64) as usize];
        if s.second != second {
            *s = Slot { second, acked: 0, lost: 0 };
        }
        s
    }

    fn update_rate(&mut self, now: Instant) {
        let second = now.duration_since(self.start).as_secs();
        let (mut acked, mut lost) = (0, 0);
        for s in self.slots.iter().filter(|s| s.second + SLOTS as u64 > second) {
            acked += s.acked;
            lost += s.lost;
        }
        self.ack_rate = if acked + lost < MIN_SAMPLES { 1.0 } else { (acked as f64 / (acked + lost) as f64).max(MIN_ACK_RATE) };
    }

    fn on_ack(&mut self, now: Instant, rtt: Duration) {
        self.rtt = rtt;
        self.slot(now).acked += 1;
        self.update_rate(now);
    }

    fn on_loss(&mut self, now: Instant, packets: u64) {
        self.slot(now).lost += packets;
        self.update_rate(now);
    }

    fn window(&self, bps: u64) -> u64 {
        if self.rtt.is_zero() {
            return 10240;
        }
        let cwnd = (bps as f64 * self.rtt.as_secs_f64() * CWND_MULTIPLIER / self.ack_rate) as u64;
        cwnd.max(self.mtu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loss_compensation_counts_packets_not_estimated_mtu() {
        let now = Instant::now();
        let mut b = Brutal::new(now, 1280);
        for _ in 0..90 {
            b.on_ack(now, Duration::from_millis(100));
        }
        b.on_loss(now, 10);
        assert!((b.ack_rate - 0.9).abs() < 1e-9);
        assert_eq!(b.window(1_000_000), 222222);
    }
    #[test]
    fn pacing_rate_is_independent_of_twice_bdp_window() {
        let now = Instant::now();
        let factory = Arc::new(Factory { brutal_bps: Arc::new(AtomicU64::new(1_000_000)), bbr: Arc::new(BbrConfig::default()) });
        let mut cc = factory.build(now, 1280).into_any().downcast::<Switch>().ok().unwrap();
        cc.brutal.on_ack(now, Duration::from_millis(100));
        assert_eq!(cc.window(), 200000);
        assert_eq!(cc.pacing_rate(), Some(1_000_000));
    }
}

#[cfg(test)]
mod admission_test {
    use super::*;
    #[test]
    fn low_rtt_brutal_can_send_a_full_datagram_instead_of_stalling() {
        let now = Instant::now();
        let factory = Arc::new(Factory { brutal_bps: Arc::new(AtomicU64::new(1_000_000)), bbr: Arc::new(BbrConfig::default()) });
        let mut cc = factory.build(now, 1280).into_any().downcast::<Switch>().ok().unwrap();
        cc.brutal.on_ack(now, Duration::from_micros(20));
        assert_eq!(cc.window(), 1280);
        assert!(cc.can_send(0, 1280));
        assert!(cc.can_send(1280, 1280));
        assert!(!cc.can_send(2560, 1280));
    }
}
