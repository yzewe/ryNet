//! Алгоритм uTLS: Copyright (c) 2019 Psiphon Inc.; условия — LICENSE-utls.txt.
use super::*;
use sha3::digest::{ExtendableOutput, Update, XofReader};
use sha3::{Digest, Sha3_256, Shake256, Shake256Reader};
use std::sync::OnceLock;

struct Prng(Shake256Reader);

impl Prng {
    fn new(seed: &[u8; 32]) -> Self {
        let mut shake = Shake256::default();
        Update::update(&mut shake, seed);
        Self(shake.finalize_xof())
    }
    fn int63(&mut self) -> u64 {
        let mut b = [0; 8];
        self.0.read(&mut b);
        u64::from_be_bytes(b) & i64::MAX as u64
    }
    fn coin(&mut self, weight: f64) -> bool {
        self.int63() as f64 / i64::MAX as f64 > 1.0 - weight.min(1.0)
    }
    fn intn(&mut self, n: u32) -> usize {
        if n.is_power_of_two() {
            return ((self.int63() >> 32) as u32 & (n - 1)) as usize;
        }
        let max = i32::MAX as u32 - (1u32 << 31) % n;
        loop {
            let v = (self.int63() >> 32) as u32;
            if v <= max {
                return (v % n) as usize;
            }
        }
    }
    fn perm(&mut self, n: usize) -> Vec<usize> {
        let mut out = vec![0; n];
        for i in 0..n {
            let j = self.intn(i as u32 + 1);
            out[i] = out[j];
            out[j] = i;
        }
        out
    }
    fn shuffle<T>(&mut self, values: &mut [T]) {
        for i in (1..values.len()).rev() {
            let n = i as u32 + 1;
            let threshold = n.wrapping_neg() % n;
            let j = loop {
                let v = (self.int63() >> 31) as u32;
                let product = u64::from(v) * u64::from(n);
                if product as u32 >= threshold {
                    break (product >> 32) as usize;
                }
            };
            values.swap(i, j);
        }
    }
}

fn hmac_sha3(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut block = [0x36; 136];
    for (dst, src) in block.iter_mut().zip(key) {
        *dst ^= src;
    }
    let mut hash = Sha3_256::new();
    Digest::update(&mut hash, block);
    Digest::update(&mut hash, data);
    let inner = hash.finalize();
    for b in &mut block {
        *b ^= 0x36 ^ 0x5c;
    }
    let mut hash = Sha3_256::new();
    Digest::update(&mut hash, block);
    Digest::update(&mut hash, inner);
    hash.finalize().into()
}

fn alps_seed(seed: &[u8; 32]) -> [u8; 32] {
    let extract = hmac_sha3(b"ALPS", seed);
    hmac_sha3(&extract, &[1])
}

pub(super) fn template(fp: Fingerprint, name: &str, next_protos: &[Vec<u8>]) -> Template {
    static ALPN_SEED: OnceLock<[u8; 32]> = OnceLock::new();
    static NOALPN_SEED: OnceLock<[u8; 32]> = OnceLock::new();
    let seed = match fp {
        Fingerprint::Randomized => *ALPN_SEED.get_or_init(rng_bytes),
        Fingerprint::RandomizedNoAlpn => *NOALPN_SEED.get_or_init(rng_bytes),
        _ => rng_bytes(),
    };
    seeded(fp, name, next_protos, &seed)
}

pub(super) fn seeded(fp: Fingerprint, name: &str, next_protos: &[Vec<u8>], seed: &[u8; 32]) -> Template {
    let mut r = Prng::new(seed);
    let with_alpn = match fp {
        Fingerprint::Randomized | Fingerprint::HelloRandomizedAlpn => true,
        Fingerprint::RandomizedNoAlpn | Fingerprint::HelloRandomizedNoAlpn => false,
        Fingerprint::HelloRandomized => r.coin(0.7),
        _ => unreachable!("not randomized"),
    };
    let ciphers: [(u16, bool); 22] = [
        (0xcca8, true),
        (0xcca9, true),
        (0xc02f, true),
        (0xc02b, true),
        (0xc030, true),
        (0xc02c, true),
        (0xc027, true),
        (0xc013, false),
        (0xc023, true),
        (0xc009, false),
        (0xc014, false),
        (0xc00a, false),
        (0x009c, true),
        (0x009d, true),
        (0x003c, true),
        (0x002f, false),
        (0x0035, false),
        (0xc012, false),
        (0x000a, false),
        (0x0005, false),
        (0xc011, false),
        (0xc007, false),
    ];
    let permutation = r.perm(ciphers.len());
    let mut tagged: Vec<_> = ciphers.iter().zip(permutation).collect();
    tagged.sort_by_key(|((_, tls12), tag)| (!tls12, *tag));
    let mut suites: Vec<_> = tagged.iter().map(|((id, _), _)| *id).collect();
    let xray_weights = matches!(fp, Fingerprint::Randomized | Fingerprint::RandomizedNoAlpn);
    let tls13 = r.coin(if xray_weights { 1.0 } else { 0.4 });
    let min = if tls13 { [0x0301, 0x0303][r.intn(2)] } else { 0x0301 };
    if tls13 {
        let mut tls13_suites = vec![0x1301, 0x1302, 0x1303];
        r.shuffle(&mut tls13_suites);
        tls13_suites.extend(suites);
        suites = tls13_suites;
        suites.retain(|s| !matches!(s, 0xc007 | 0xc011 | 0x0005));
    }
    let float_len = suites.len() as f64;
    let mut i = 1;
    while i < suites.len() {
        if r.coin(0.4 * i as f64 / float_len) {
            suites.remove(i);
        } else {
            i += 1;
        }
    }
    let mut sigalgs = vec![0x0403, 0x0401, 0x0503, 0x0501, 0x0201, 0x0601];
    if r.coin(0.63) {
        sigalgs.push(0x0203);
    }
    if r.coin(0.59) {
        sigalgs.push(0x0603);
    }
    if r.coin(0.51) || tls13 {
        sigalgs.push(0x0804);
        if r.coin(0.9) {
            sigalgs.extend([0x0805, 0x0806]);
        }
    }
    r.shuffle(&mut sigalgs);
    let mut groups = vec![];
    if r.coin(0.71) && tls13 {
        groups.push(GROUP_X25519MLKEM768);
    }
    if r.coin(0.71) || tls13 {
        groups.push(GROUP_X25519);
    }
    groups.extend([GROUP_SECP256R1, GROUP_SECP384R1]);
    if r.coin(0.46) {
        groups.push(GROUP_SECP521R1);
    }
    let mut exts = vec![
        Ext::Raw(EXT_SERVER_NAME, sni_body(name)),
        Ext::Raw(EXT_SESSION_TICKET, vec![]),
        Ext::Raw(EXT_SIGNATURE_ALGORITHMS, u16_list(&sigalgs)),
        Ext::Raw(EXT_EC_POINT_FORMATS, vec![1, 0]),
        Ext::Raw(EXT_SUPPORTED_GROUPS, vec![]),
    ];
    let alpn = if with_alpn {
        if next_protos.is_empty() { vec![b"h2".to_vec(), b"http/1.1".to_vec()] } else { next_protos.to_vec() }
    } else {
        vec![]
    };
    if with_alpn {
        exts.push(Ext::Raw(EXT_ALPN, proto_list(&alpn)));
    }
    if r.coin(0.62) || tls13 {
        exts.push(Ext::Padding);
    }
    if r.coin(0.74) {
        exts.push(Ext::Raw(EXT_STATUS_REQUEST, vec![1, 0, 0, 0, 0]));
    }
    if r.coin(0.46) {
        exts.push(Ext::Raw(EXT_SCT, vec![]));
    }
    if r.coin(0.75) {
        exts.push(Ext::Raw(EXT_RENEGOTIATION_INFO, vec![0]));
    }
    if r.coin(0.77) {
        exts.push(Ext::Raw(EXT_EXTENDED_MASTER_SECRET, vec![]));
    }
    let mut shares = vec![];
    let mut versions = vec![];
    if tls13 {
        shares.push(GROUP_X25519);
        if r.coin(0.0) {
            shares[0] = GROUP_SECP256R1;
        } else {
            if r.coin(0.5) {
                shares.push(GROUP_SECP256R1);
            }
            if r.coin(0.5) {
                shares.insert(0, GROUP_X25519MLKEM768);
            }
        }
        versions.extend((min..=0x0304).rev());
        exts.extend([Ext::KeyShare, Ext::Raw(EXT_PSK_KEY_EXCHANGE_MODES, vec![1, 1]), Ext::Raw(EXT_SUPPORTED_VERSIONS, vec![])]);
        if with_alpn && Prng::new(&alps_seed(seed)).coin(0.33) {
            exts.push(Ext::Raw(EXT_APPLICATION_SETTINGS_OLD, proto_list(&[b"h2".to_vec()])));
        }
    }
    r.shuffle(&mut exts);
    exts.retain(|e| !matches!(e, Ext::Raw(EXT_SERVER_NAME, _) if name.is_empty() || name.parse::<std::net::IpAddr>().is_ok()));
    Template { suites, exts, alpn, sigalgs, groups, cert_compression: vec![], shares: Shares::List(shares), versions, shuffle: false }
}
