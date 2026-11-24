use super::*;

fn any_slice<const M: usize>(buf: &[u8; M]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= M);
    &buf[..len]
}

fn state_ok(s: &TrafficState) {
    assert!((-1..=5).contains(&s.remaining_command));
    assert!((-1..=0xffff).contains(&s.remaining_content));
    assert!((-1..=0xffff).contains(&s.remaining_padding));
    assert!(s.head.len() < 21);
}

const UUID: [u8; 16] = [0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe, 0x0f];

fn in_range(lo: i32, hi: i32) -> i32 {
    let v: i32 = kani::any();
    kani::assume(lo <= v && v <= hi);
    v
}

#[kani::proof]
#[kani::unwind(14)]
fn unpad_any_state_any_chunk() {
    let mut s = TrafficState::new();
    s.remaining_command = in_range(-1, 5);
    s.remaining_content = in_range(-1, 0xffff);
    s.remaining_padding = in_range(-1, 0xffff);
    s.current_command = in_range(0, 0xff);
    s.seen_uuid = kani::any();
    let buf: [u8; 12] = kani::any();
    let b = any_slice(&buf);
    let mut out = Vec::new();
    s.unpad(&UUID, b, &mut out);
    state_ok(&s);
    assert!(out.len() + s.head.len() <= b.len());
}

#[kani::proof]
#[kani::unwind(17)]
fn unpad_head_then_chunk() {
    let mut s = TrafficState::new();
    let mut out = Vec::new();
    s.unpad(&UUID, &UUID[..10], &mut out);
    assert!(out.is_empty() && s.head.len() == 10);
    let buf: [u8; 14] = kani::any();
    let b = any_slice(&buf);
    s.unpad(&UUID, b, &mut out);
    state_ok(&s);
    assert!(out.len() + s.head.len() <= 10 + b.len());
    if s.seen_uuid {
        assert!(b.len() >= 11 && b[..6] == UUID[10..] && out.len() + 21 <= 10 + b.len());
    }
}

static PAD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn rand_below_fixed(_: &SystemRandom, _: u32) -> u32 {
    PAD.load(std::sync::atomic::Ordering::Relaxed)
}

fn padded_wire(p1: &[u8], p2: &[u8], tail: &[u8], pad: u32) -> Vec<u8> {
    PAD.store(pad, std::sync::atomic::Ordering::Relaxed);
    let rng = SystemRandom::new();
    let mut once = Some(UUID);
    let mut wire = Vec::new();
    padding(&rng, p1, CMD_CONTINUE, &mut once, false, &mut wire);
    padding(&rng, p2, CMD_END, &mut once, false, &mut wire);
    wire.extend_from_slice(tail);
    wire
}

#[kani::proof]
#[kani::unwind(50)]
#[kani::stub(rand_below, rand_below_fixed)]
fn padding_unpad_roundtrip() {
    let data: [u8; 8] = kani::any();
    for a in 0..=3 {
        for c in 0..=3 {
            for t in 0..=2 {
                for pad in [0, 1, 3] {
                    let (p1, p2, tail) = (&data[..a], &data[3..3 + c], &data[6..6 + t]);
                    let wire = padded_wire(p1, p2, tail, pad);
                    let mut s = TrafficState::new();
                    let mut out = Vec::new();
                    s.unpad(&UUID, &wire, &mut out);
                    assert!(out.len() == a + c + t);
                    assert!(out[..a] == *p1 && out[a..a + c] == *p2 && out[a + c..] == *tail);
                    assert_eq!(s.remaining_command, -1);
                }
            }
        }
    }
}

#[kani::proof]
#[kani::unwind(40)]
#[kani::stub(rand_below, rand_below_fixed)]
fn padding_unpad_roundtrip_every_split() {
    let data: [u8; 4] = kani::any();
    let wire = padded_wire(&data[..2], &data[2..3], &data[3..], 1);
    assert_eq!(wire.len(), 32);
    for k in 0..=wire.len() {
        let mut s = TrafficState::new();
        let mut out = Vec::new();
        s.unpad(&UUID, &wire[..k], &mut out);
        s.unpad(&UUID, &wire[k..], &mut out);
        assert!(out == data);
        assert_eq!(s.remaining_command, -1);
    }
}

fn filter_ok(s: &TrafficState, total: usize) {
    assert!((0..=8).contains(&s.number_of_packet_to_filter));
    assert!(s.remaining_server_hello >= -1 - total as i32);
    if s.enable_xtls {
        assert!(matches!(s.cipher, 0x1301..=0x1304));
        assert!(s.is_tls12_or_above);
    }
}

#[kani::proof]
#[kani::unwind(42)]
fn filter_tls_any_input() {
    let a: [u8; 40] = kani::any();
    let b: [u8; 16] = kani::any();
    let (x, y) = (any_slice(&a), any_slice(&b));
    let mut s = TrafficState::new();
    s.filter_tls([x, y]);
    filter_ok(&s, x.len() + y.len());
}

#[kani::proof]
#[kani::unwind(82)]
fn filter_tls_long_server_hello() {
    let a: [u8; 80] = kani::any();
    let mut s = TrafficState::new();
    s.filter_tls([&a[..]]);
    filter_ok(&s, a.len());
}

#[kani::proof]
#[kani::unwind(26)]
fn complete_record_any_input() {
    let buf: [u8; 24] = kani::any();
    let b = any_slice(&buf);
    if is_complete_record(b) && !b.is_empty() {
        assert!(b.len() >= 6);
        assert!(b[..3] == TLS_APPLICATION_DATA_START);
        let len = u16::from_be_bytes([b[3], b[4]]) as usize;
        assert!(len >= 1 && 5 + len <= b.len());
    }
}
