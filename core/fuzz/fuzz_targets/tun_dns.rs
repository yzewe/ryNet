#![no_main]

use libfuzzer_sys::fuzz_target;
use rv_tun::fuzzing::dns;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, data)) = data.split_first() else { return };
    match pick % 3 {
        0 => dns::message(data),
        1 => dns::pair(data),
        _ => dns::cache(data),
    }
});
