#![no_main]

use libfuzzer_sys::fuzz_target;
use rv_tls::fuzzing::{der, handshake, record};

fuzz_target!(|data: &[u8]| {
    let Some((&pick, data)) = data.split_first() else { return };
    match pick % 13 {
        0 => record::records(data),
        1 => record::read_app(data),
        2 => record::keyed(data),
        3 => record::post_handshake(data),
        4 => handshake::reader(data),
        5 => handshake::extensions(data),
        6 => handshake::certificate(data),
        7 => handshake::compressed_certificate(data),
        8 => handshake::certificate_verify(data),
        9 => handshake::reality(data),
        10 => handshake::server_flight(data),
        11 => der::tlv(data),
        _ => der::cert(data),
    }
});
