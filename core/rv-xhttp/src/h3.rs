use crate::hpack;

pub const MAX_FIELD_SECTION: u64 = 10 << 20;
pub const DATA: u64 = 0;
pub const HEADERS: u64 = 1;
pub const SETTINGS: u64 = 4;

pub fn put_varint(o: &mut Vec<u8>, v: u64) {
    match v {
        0..=63 => o.push(v as u8),
        64..=16383 => o.extend_from_slice(&((v as u16) | 0x4000).to_be_bytes()),
        16384..=1073741823 => o.extend_from_slice(&((v as u32) | 0x8000_0000).to_be_bytes()),
        _ => o.extend_from_slice(&(v | 0xc000_0000_0000_0000).to_be_bytes()),
    }
}

pub fn get_varint(b: &[u8]) -> Option<(u64, usize)> {
    let first = *b.first()?;
    let len = 1usize << (first >> 6);
    if b.len() < len {
        return None;
    }
    let mut v = (first & 0x3f) as u64;
    for &x in &b[1..len] {
        v = (v << 8) | x as u64;
    }
    Some((v, len))
}

fn put_int(o: &mut Vec<u8>, flags: u8, bits: u8, mut v: u64) {
    let max = (1u64 << bits) - 1;
    if v < max {
        o.push(flags | v as u8);
        return;
    }
    o.push(flags | max as u8);
    v -= max;
    while v >= 128 {
        o.push((v % 128) as u8 | 0x80);
        v /= 128;
    }
    o.push(v as u8);
}

fn get_int(b: &[u8], i: &mut usize, bits: u8) -> Option<u64> {
    let max = (1u64 << bits) - 1;
    let mut v = (*b.get(*i)? as u64) & max;
    *i += 1;
    if v < max {
        return Some(v);
    }
    let mut shift = 0;
    loop {
        let x = *b.get(*i)?;
        *i += 1;
        v = v.checked_add(((x & 0x7f) as u64).checked_shl(shift)?)?;
        if x & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 56 {
            return None;
        }
    }
}

fn put_string(o: &mut Vec<u8>, flags: u8, bits: u8, s: &[u8]) {
    let h = hpack::encode(s);
    put_int(o, flags | (1 << bits), bits, h.len() as u64);
    o.extend(h);
}

fn get_string(b: &[u8], i: &mut usize, bits: u8) -> Option<Vec<u8>> {
    let huffman = b.get(*i)? & (1 << bits) != 0;
    let n = usize::try_from(get_int(b, i, bits)?).ok()?;
    let raw = b.get(*i..i.checked_add(n)?)?;
    *i += n;
    if huffman { hpack::decode(raw) } else { Some(raw.to_vec()) }
}

const STATIC: &[(&str, &str)] = &[
    (":authority", ""),
    (":path", "/"),
    ("age", "0"),
    ("content-disposition", ""),
    ("content-length", "0"),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("referer", ""),
    ("set-cookie", ""),
    (":method", "CONNECT"),
    (":method", "DELETE"),
    (":method", "GET"),
    (":method", "HEAD"),
    (":method", "OPTIONS"),
    (":method", "POST"),
    (":method", "PUT"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "103"),
    (":status", "200"),
    (":status", "304"),
    (":status", "404"),
    (":status", "503"),
    ("accept", "*/*"),
    ("accept", "application/dns-message"),
    ("accept-encoding", "gzip, deflate, br"),
    ("accept-ranges", "bytes"),
    ("access-control-allow-headers", "cache-control"),
    ("access-control-allow-headers", "content-type"),
    ("access-control-allow-origin", "*"),
    ("cache-control", "max-age=0"),
    ("cache-control", "max-age=2592000"),
    ("cache-control", "max-age=604800"),
    ("cache-control", "no-cache"),
    ("cache-control", "no-store"),
    ("cache-control", "public, max-age=31536000"),
    ("content-encoding", "br"),
    ("content-encoding", "gzip"),
    ("content-type", "application/dns-message"),
    ("content-type", "application/javascript"),
    ("content-type", "application/json"),
    ("content-type", "application/x-www-form-urlencoded"),
    ("content-type", "image/gif"),
    ("content-type", "image/jpeg"),
    ("content-type", "image/png"),
    ("content-type", "text/css"),
    ("content-type", "text/html; charset=utf-8"),
    ("content-type", "text/plain"),
    ("content-type", "text/plain;charset=utf-8"),
    ("range", "bytes=0-"),
    ("strict-transport-security", "max-age=31536000"),
    ("strict-transport-security", "max-age=31536000; includesubdomains"),
    ("strict-transport-security", "max-age=31536000; includesubdomains; preload"),
    ("vary", "accept-encoding"),
    ("vary", "origin"),
    ("x-content-type-options", "nosniff"),
    ("x-xss-protection", "1; mode=block"),
    (":status", "100"),
    (":status", "204"),
    (":status", "206"),
    (":status", "302"),
    (":status", "400"),
    (":status", "403"),
    (":status", "421"),
    (":status", "425"),
    (":status", "500"),
    ("accept-language", ""),
    ("access-control-allow-credentials", "FALSE"),
    ("access-control-allow-credentials", "TRUE"),
    ("access-control-allow-headers", "*"),
    ("access-control-allow-methods", "get"),
    ("access-control-allow-methods", "get, post, options"),
    ("access-control-allow-methods", "options"),
    ("access-control-expose-headers", "content-length"),
    ("access-control-request-headers", "content-type"),
    ("access-control-request-method", "get"),
    ("access-control-request-method", "post"),
    ("alt-svc", "clear"),
    ("authorization", ""),
    ("content-security-policy", "script-src 'none'; object-src 'none'; base-uri 'none'"),
    ("early-data", "1"),
    ("expect-ct", ""),
    ("forwarded", ""),
    ("if-range", ""),
    ("origin", ""),
    ("purpose", "prefetch"),
    ("server", ""),
    ("timing-allow-origin", "*"),
    ("upgrade-insecure-requests", "1"),
    ("user-agent", ""),
    ("x-forwarded-for", ""),
    ("x-frame-options", "deny"),
    ("x-frame-options", "sameorigin"),
];

pub fn control_stream() -> Vec<u8> {
    let mut o = vec![0, 4, 5, 6];
    put_varint(&mut o, MAX_FIELD_SECTION);
    o
}

pub fn frame(typ: u64, body: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(body.len() + 16);
    put_varint(&mut o, typ);
    put_varint(&mut o, body.len() as u64);
    o.extend_from_slice(body);
    o
}

pub fn encode(headers: &[(String, String)]) -> Vec<u8> {
    let mut o = vec![0, 0];
    for (name, value) in headers {
        if let Some(i) = STATIC.iter().position(|&(n, v)| n == name && v == value) {
            put_int(&mut o, 0xc0, 6, i as u64);
        } else if let Some(i) = STATIC.iter().position(|&(n, _)| n == name) {
            put_int(&mut o, 0x50, 4, i as u64);
            put_string(&mut o, 0, 7, value.as_bytes());
        } else {
            put_string(&mut o, 0x20, 3, name.as_bytes());
            put_string(&mut o, 0, 7, value.as_bytes());
        }
    }
    o
}

pub fn decode(block: &[u8]) -> Option<Vec<(String, String)>> {
    let mut i = 0;
    if get_int(block, &mut i, 8)? != 0 || block.get(i)? & 0x80 != 0 || get_int(block, &mut i, 7)? != 0 {
        return None;
    }
    let mut headers = Vec::new();
    let mut left = MAX_FIELD_SECTION as usize;
    while i < block.len() {
        let b = block[i];
        let (name, value) = if b & 0xc0 == 0xc0 {
            let (n, v) = *STATIC.get(usize::try_from(get_int(block, &mut i, 6)?).ok()?)?;
            (n.into(), v.into())
        } else if b & 0xc0 == 0x40 && b & 0x10 != 0 {
            let (n, _) = *STATIC.get(usize::try_from(get_int(block, &mut i, 4)?).ok()?)?;
            (n.into(), String::from_utf8(get_string(block, &mut i, 7)?).ok()?)
        } else if b & 0xe0 == 0x20 {
            let n = String::from_utf8(get_string(block, &mut i, 3)?).ok()?;
            (n, String::from_utf8(get_string(block, &mut i, 7)?).ok()?)
        } else {
            return None;
        };
        left = left.checked_sub(name.len() + value.len() + 32)?;
        headers.push((name, value));
    }
    Some(headers)
}

pub fn small_map_order(n: usize, r: u32) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let slot = r as usize % 8;
    let start = if slot < n { slot } else { 0 };
    (0..n).map(|i| (start + i) % n).collect()
}

#[cfg(kani)]
#[path = "proofs/h3.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_match_quic_go() {
        assert_eq!(control_stream(), [0, 4, 5, 6, 0x80, 0xa0, 0, 0]);
    }
    #[test]
    fn qpack_rejects_dynamic_and_overflowing_references() {
        for block in [vec![], vec![1, 0], vec![0, 0, 0x80], vec![0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]] {
            assert!(decode(&block).is_none());
        }
        let headers: Vec<_> = STATIC.iter().map(|&(n, v)| (n.into(), v.into())).collect();
        assert_eq!(decode(&encode(&headers)).unwrap(), headers);
        let h = vec![("x-custom".into(), "literal".into()), ("content-type".into(), "application/grpc".into())];
        assert_eq!(decode(&encode(&h)).unwrap(), h);
    }
    #[test]
    fn matches_quic_go_qpack_060_bytes() {
        let fields = [
            (":authority", "test.local"),
            (":method", "POST"),
            (":path", "/fp/01234567-89ab-cdef-0123-456789abcdef/0"),
            (":scheme", "https"),
            ("accept", "*/*"),
            ("x-custom", "literal"),
            ("content-type", "application/grpc"),
            ("content-length", "73"),
            ("accept-encoding", "gzip"),
            ("user-agent", "test"),
        ];
        let headers = fields.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect::<Vec<_>>();
        let actual: String = encode(&headers).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            actual,
            "00005087495095e8390747d4519e625ad80044cb4db8eacf3e38d624859560044cacd36e3af3e38c92165601d7dd2ef2b12d424f4f85a0c92d83a35f1d8b1d75d0620d263d4c4d65645482759f5f10839bd9ab5f508349509f"
        );
    }
    #[test]
    fn go_map_rotation_distribution() {
        let mut counts = [0; 3];
        for i in 0..8 {
            counts[small_map_order(3, i)[0]] += 1;
        }
        assert_eq!(counts, [6, 1, 1]);
    }
}
