#![no_main]

use libfuzzer_sys::fuzz_target;
use rv_core::outbound::{hysteria, shadowsocks, trojan};
use rv_core::{dns, sniff, transport};

fuzz_target!(|data: &[u8]| {
    let Some((&pick, data)) = data.split_first() else { return };
    match pick % 9 {
        0 => dns::fuzzing::message(data),
        1 => transport::grpc::fuzzing::frames(data),
        2 => transport::ws::fuzzing::frames(data),
        3 => hysteria::proto_fuzzing::server(data),
        4 => hysteria::h3_fuzzing::headers(data),
        5 => trojan::fuzzing::udp(data),
        6 => shadowsocks::fuzzing::server(data),
        _ => sniff::fuzzing::first_bytes(data),
    }
});
