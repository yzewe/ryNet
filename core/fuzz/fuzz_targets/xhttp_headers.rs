#![no_main]

use libfuzzer_sys::fuzz_target;
use rv_xhttp::{h3, hpack};

fuzz_target!(|data: &[u8]| {
    assert_eq!(hpack::decode(&hpack::encode(data)).as_deref(), Some(data));
    let _ = hpack::decode(data);
    if let Some(h) = h3::decode(data) {
        assert_eq!(h3::decode(&h3::encode(&h)), Some(h));
    }
    if let Some((v, n)) = h3::get_varint(data) {
        assert!(n <= data.len());
        let mut o = Vec::new();
        h3::put_varint(&mut o, v);
        assert_eq!(h3::get_varint(&o), Some((v, o.len())));
    }
});
