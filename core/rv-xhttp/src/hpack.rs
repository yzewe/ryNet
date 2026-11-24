use std::collections::HashMap;
use std::sync::OnceLock;

use crate::request::HPACK_BITS;

struct Tables {
    codes: [u32; 257],
    decode: HashMap<(u8, u32), u16>,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut order: Vec<u16> = (0..257).collect();
        order.sort_by_key(|&s| (HPACK_BITS[s as usize], s));
        let mut codes = [0u32; 257];
        let mut decode = HashMap::with_capacity(257);
        let (mut code, mut len) = (0u32, HPACK_BITS[order[0] as usize]);
        for (i, &s) in order.iter().enumerate() {
            let l = HPACK_BITS[s as usize];
            if i > 0 {
                code = (code + 1) << (l - len);
            }
            len = l;
            codes[s as usize] = code;
            decode.insert((l, code), s);
        }
        Tables { codes, decode }
    })
}

fn symbol(t: &Tables, len: u8, code: u32) -> Option<u16> {
    t.decode.get(&(len, code)).copied()
}

pub fn encode(s: &[u8]) -> Vec<u8> {
    let t = tables();
    let mut out = Vec::with_capacity(s.len());
    let (mut acc, mut bits) = (0u64, 0u32);
    for &b in s {
        let len = HPACK_BITS[b as usize] as u32;
        acc = (acc << len) | t.codes[b as usize] as u64;
        bits += len;
        while bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    if bits > 0 {
        out.push(((acc << (8 - bits)) as u8) | (0xff >> bits));
    }
    out
}

pub fn decode(b: &[u8]) -> Option<Vec<u8>> {
    let t = tables();
    let mut out = Vec::with_capacity(b.len() * 8 / 5);
    let (mut code, mut len) = (0u32, 0u8);
    for &byte in b {
        for i in (0..8).rev() {
            code = (code << 1) | ((byte >> i) & 1) as u32;
            len += 1;
            if len >= 5
                && let Some(s) = symbol(t, len, code)
            {
                if s == 256 {
                    return None;
                }
                out.push(s as u8);
                code = 0;
                len = 0;
            }
            if len > 30 {
                return None;
            }
        }
    }
    (len <= 7 && code == (1u32 << len) - 1).then_some(out)
}

#[cfg(kani)]
#[path = "proofs/hpack.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    #[test]
    fn rfc7541_examples() {
        let e = super::encode(b"www.example.com");
        assert_eq!(e, [0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab, 0x90, 0xf4, 0xff]);
        assert_eq!(super::decode(&e).unwrap(), b"www.example.com");
        assert_eq!(super::encode(b"no-cache"), [0xa8, 0xeb, 0x10, 0x64, 0x9c, 0xbf]);
        for s in [&b"hysteria"[..], b"/auth", b"233", b"Hysteria-Padding", &[0u8, 255, 10, 13]] {
            assert_eq!(super::decode(&super::encode(s)).unwrap(), s);
        }
    }
}
