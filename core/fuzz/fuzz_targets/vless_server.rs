#![no_main]

use libfuzzer_sys::fuzz_target;
use rv_vless::fuzzing::{self, vision};
use rv_vless::xudp;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, data)) = data.split_first() else { return };
    match pick % 9 {
        0 => fuzzing::response_header(data),
        1 => vision::unpad(data),
        2 => vision::roundtrip(data),
        3 => vision::filter(data),
        4 => vision::complete_record(data),
        5 => vision::downlink(data),
        6 => vision::uplink(data),
        7 => xudp::fuzzing::packets(data),
        _ => xudp::fuzzing::addr(data),
    }
});
