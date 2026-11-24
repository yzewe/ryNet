mod presets;
mod randomized;

use ml_kem::kem::Decapsulate;
use ml_kem::{DecapsulationKey768, DecapsulationKey1024, KeyExport};
use ring::agreement;
use ring::rand::{SecureRandom, SystemRandom};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::Fingerprint;

pub(crate) const GROUP_X25519: u16 = 0x001d;
pub(crate) const GROUP_SECP256R1: u16 = 0x0017;
pub(crate) const GROUP_SECP384R1: u16 = 0x0018;
pub(crate) const GROUP_SECP521R1: u16 = 0x0019;
pub(crate) const GROUP_X25519MLKEM768: u16 = 0x11ec;
pub(crate) const GROUP_X25519KYBER768: u16 = 0x6399;
pub(crate) const GROUP_P256MLKEM768: u16 = 0x11eb;
pub(crate) const GROUP_P384MLKEM1024: u16 = 0x11ed;
pub(crate) const UTLS_HRR_GROUP_ERROR: &str = "uTLS cannot generate this group after HelloRetryRequest";

pub(crate) const CERT_COMPRESSION_ZLIB: u16 = 1;
pub(crate) const CERT_COMPRESSION_BROTLI: u16 = 2;
pub(crate) const CERT_COMPRESSION_ZSTD: u16 = 3;

const EXT_SERVER_NAME: u16 = 0;
const EXT_STATUS_REQUEST: u16 = 5;
const EXT_SUPPORTED_GROUPS: u16 = 10;
const EXT_EC_POINT_FORMATS: u16 = 11;
const EXT_SIGNATURE_ALGORITHMS: u16 = 13;
const EXT_ALPN: u16 = 16;
const EXT_SCT: u16 = 18;
const EXT_EXTENDED_MASTER_SECRET: u16 = 23;
const EXT_COMPRESS_CERTIFICATE: u16 = 27;
const EXT_RECORD_SIZE_LIMIT: u16 = 28;
const EXT_DELEGATED_CREDENTIALS: u16 = 34;
const EXT_SESSION_TICKET: u16 = 35;
const EXT_SUPPORTED_VERSIONS: u16 = 43;
const EXT_COOKIE: u16 = 44;
const EXT_PADDING: u16 = 21;
const EXT_PSK_KEY_EXCHANGE_MODES: u16 = 45;
const EXT_KEY_SHARE: u16 = 51;
pub(crate) const EXT_APPLICATION_SETTINGS_NEW: u16 = 17613;
pub(crate) const EXT_APPLICATION_SETTINGS_OLD: u16 = 17513;
const EXT_ECH: u16 = 0xfe0d;
const EXT_RENEGOTIATION_INFO: u16 = 0xff01;

const GREASE_CIPHER: usize = 0;
const GREASE_GROUP: usize = 1;
const GREASE_EXT1: usize = 2;
const GREASE_EXT2: usize = 3;
const GREASE_VERSION: usize = 4;
const GREASE_SEED_LEN: usize = 5;

pub(crate) struct ClientHello {
    pub msg: Vec<u8>,
    pub random: [u8; 32],
    pub session_id_offset: usize,
    pub tls13_suites: Vec<u16>,
    pub cert_compression: Vec<u16>,
    pub alpn: Vec<Vec<u8>>,
    pub signature_algorithms: Vec<u16>,
    pub supported_groups: Vec<u16>,
    pub alps: Option<u16>,
    pub keys: KeyShareKeys,
    pub(crate) suites_raw: Vec<u16>,
    pub(crate) exts: Vec<(u16, Vec<u8>)>,
}

pub(crate) struct KeyShareKeys {
    x25519: Option<StaticSecret>,
    hybrid_x25519: Option<StaticSecret>,
    mlkem: Option<DecapsulationKey768>,
    mlkem1024: Option<DecapsulationKey1024>,
    p256: Option<agreement::EphemeralPrivateKey>,
    p384: Option<agreement::EphemeralPrivateKey>,
    p521: Option<crate::p521::Ephemeral>,
    groups: Vec<u16>,
}

impl KeyShareKeys {
    fn empty() -> Self {
        Self { x25519: None, hybrid_x25519: None, mlkem: None, mlkem1024: None, p256: None, p384: None, p521: None, groups: Vec::new() }
    }

    pub fn shared_secret(&mut self, group: u16, server_share: &[u8]) -> Result<Vec<u8>, &'static str> {
        if !self.groups.contains(&group) {
            return Err("server selected a group we did not send a share for");
        }
        match group {
            GROUP_X25519 => {
                let pk: [u8; 32] = server_share.try_into().map_err(|_| "bad X25519 share")?;
                let k = self.x25519.as_ref().ok_or("no X25519 key")?;
                let ss = k.diffie_hellman(&PublicKey::from(pk));
                if !ss.was_contributory() {
                    return Err("X25519 low-order point");
                }
                Ok(ss.to_bytes().to_vec())
            }
            GROUP_X25519MLKEM768 | GROUP_X25519KYBER768 => {
                if server_share.len() != 1088 + 32 {
                    return Err("bad X25519MLKEM768 share");
                }
                let dk = self.mlkem.as_ref().ok_or("no ML-KEM key")?;
                let (ct_bytes, x_bytes) = if group == GROUP_X25519KYBER768 {
                    (&server_share[32..], &server_share[..32])
                } else {
                    (&server_share[..1088], &server_share[1088..])
                };
                let ct = ml_kem::ml_kem_768::Ciphertext::try_from(ct_bytes).map_err(|_| "bad ML-KEM ciphertext")?;
                let mut kem_ss: [u8; 32] = dk.decapsulate(&ct).into();
                if group == GROUP_X25519KYBER768 {
                    use sha3::digest::{ExtendableOutput, Update, XofReader};
                    use sha3::{Digest, Sha3_256, Shake256};
                    let mut h = Shake256::default();
                    Update::update(&mut h, &kem_ss);
                    Update::update(&mut h, &Sha3_256::digest(ct_bytes));
                    h.finalize_xof().read(&mut kem_ss);
                }
                let pk: [u8; 32] = x_bytes.try_into().unwrap();
                let k = self.hybrid_x25519.as_ref().ok_or("no hybrid X25519 key")?;
                let ss = k.diffie_hellman(&PublicKey::from(pk));
                if !ss.was_contributory() {
                    return Err("X25519 low-order point");
                }
                let out = if group == GROUP_X25519KYBER768 {
                    [ss.as_bytes().as_slice(), kem_ss.as_slice()].concat()
                } else {
                    [kem_ss.as_slice(), ss.as_bytes().as_slice()].concat()
                };
                Ok(out)
            }
            GROUP_P256MLKEM768 | GROUP_P384MLKEM1024 => {
                let (ec_len, ct_len) = if group == GROUP_P256MLKEM768 { (65, 1088) } else { (97, 1568) };
                if server_share.len() != ec_len + ct_len {
                    return Err("bad NIST ML-KEM share");
                }
                let (ec, ct) = server_share.split_at(ec_len);
                let (key, alg, kem_ss) = if group == GROUP_P256MLKEM768 {
                    let ct = ml_kem::ml_kem_768::Ciphertext::try_from(ct).map_err(|_| "bad ML-KEM ciphertext")?;
                    (
                        self.p256.take().ok_or("no P-256 key")?,
                        &agreement::ECDH_P256,
                        self.mlkem.as_ref().ok_or("no ML-KEM key")?.decapsulate(&ct),
                    )
                } else {
                    let ct = ml_kem::ml_kem_1024::Ciphertext::try_from(ct).map_err(|_| "bad ML-KEM ciphertext")?;
                    (
                        self.p384.take().ok_or("no P-384 key")?,
                        &agreement::ECDH_P384,
                        self.mlkem1024.as_ref().ok_or("no ML-KEM-1024 key")?.decapsulate(&ct),
                    )
                };
                let peer = agreement::UnparsedPublicKey::new(alg, ec);
                let mut out = agreement::agree_ephemeral(key, &peer, |s| s.to_vec()).map_err(|_| "NIST hybrid agreement failed")?;
                out.extend_from_slice(&kem_ss);
                Ok(out)
            }
            GROUP_SECP256R1 => {
                let k = self.p256.take().ok_or("no P-256 key")?;
                let peer = agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, server_share);
                agreement::agree_ephemeral(k, &peer, |s| s.to_vec()).map_err(|_| "P-256 agreement failed")
            }
            GROUP_SECP384R1 => {
                let k = self.p384.take().ok_or("no P-384 key")?;
                let peer = agreement::UnparsedPublicKey::new(&agreement::ECDH_P384, server_share);
                agreement::agree_ephemeral(k, &peer, |s| s.to_vec()).map_err(|_| "P-384 agreement failed")
            }
            GROUP_SECP521R1 => {
                let k = self.p521.take().ok_or("no P-521 key")?;
                k.agree(server_share).ok_or("P-521 agreement failed")
            }
            _ => Err("unsupported group"),
        }
    }

    pub fn reality_ecdh(&self, server_public: &[u8; 32]) -> Option<[u8; 32]> {
        let k = self.x25519.as_ref().or(self.hybrid_x25519.as_ref())?;
        Some(k.diffie_hellman(&PublicKey::from(*server_public)).to_bytes())
    }
}

fn rng_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    SystemRandom::new().fill(&mut b).expect("system RNG");
    b
}

fn rand_below(n: u64) -> u64 {
    let threshold = n.wrapping_neg() % n;
    loop {
        let value = u64::from_le_bytes(rng_bytes::<8>());
        if value >= threshold {
            return value % n;
        }
    }
}

fn grease_value(seed: &[u16; GREASE_SEED_LEN], idx: usize) -> u16 {
    let v = (seed[idx] & 0xf0) | 0x0a;
    v | (v << 8)
}

fn grease_seed() -> [u16; GREASE_SEED_LEN] {
    let b = rng_bytes::<{ 2 * GREASE_SEED_LEN }>();
    let mut s = [0u16; GREASE_SEED_LEN];
    for (i, v) in s.iter_mut().enumerate() {
        *v = u16::from_le_bytes([b[2 * i], b[2 * i + 1]]);
    }
    if grease_value(&s, GREASE_EXT1) == grease_value(&s, GREASE_EXT2) {
        s[GREASE_EXT2] ^= 0x1010;
    }
    s
}

pub(crate) fn is_grease(v: u16) -> bool {
    (v >> 8) == (v & 0xff) && v & 0xf == 0xa
}

fn put_u16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_be_bytes());
}
fn put_vec8(o: &mut Vec<u8>, b: &[u8]) {
    o.push(b.len() as u8);
    o.extend_from_slice(b);
}
fn put_vec16(o: &mut Vec<u8>, b: &[u8]) {
    put_u16(o, b.len() as u16);
    o.extend_from_slice(b);
}
fn exts_fit(exts: &[(u16, Vec<u8>)]) -> bool {
    exts.iter().map(|(_, b)| 4 + b.len()).sum::<usize>() <= 0xffff
}
pub(crate) const INVALID_ALPN: &str = "invalid NextProtos value";
fn u16_list(v: &[u16]) -> Vec<u8> {
    let mut inner = Vec::with_capacity(v.len() * 2);
    for &x in v {
        put_u16(&mut inner, x);
    }
    let mut o = Vec::new();
    put_vec16(&mut o, &inner);
    o
}
fn proto_list(v: &[Vec<u8>]) -> Vec<u8> {
    let mut inner = Vec::new();
    for p in v {
        put_vec8(&mut inner, p);
    }
    let mut o = Vec::new();
    put_vec16(&mut o, &inner);
    o
}

const CHROME_SUITES: [u16; 16] =
    [0x0a0a, 0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035];
const CHROME_GROUPS: [u16; 5] = [0x0a0a, GROUP_X25519MLKEM768, GROUP_X25519, GROUP_SECP256R1, GROUP_SECP384R1];
const CHROME_SIGALGS: [u16; 8] = [0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601];

const FIREFOX_SUITES: [u16; 17] = [
    0x1301, 0x1303, 0x1302, 0xc02b, 0xc02f, 0xcca9, 0xcca8, 0xc02c, 0xc030, 0xc00a, 0xc009, 0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035,
];
const FIREFOX_GROUPS: [u16; 7] = [GROUP_X25519MLKEM768, GROUP_X25519, GROUP_SECP256R1, GROUP_SECP384R1, GROUP_SECP521R1, 0x0100, 0x0101];
const FIREFOX_SIGALGS: [u16; 11] = [0x0403, 0x0503, 0x0603, 0x0804, 0x0805, 0x0806, 0x0401, 0x0501, 0x0601, 0x0203, 0x0201];
const FIREFOX_DC_SIGALGS: [u16; 4] = [0x0403, 0x0503, 0x0603, 0x0203];

enum Ext {
    Raw(u16, Vec<u8>),
    Grease1,
    Grease2,
    KeyShare,
    Padding,
}

#[derive(Clone, PartialEq, Eq)]
enum Shares {
    GreaseHybrid,
    FirefoxReuse,
    GreaseX25519,
    X25519,
    FirefoxClassical,
    List(Vec<u16>),
    GoHybridReuse,
}

struct Template {
    suites: Vec<u16>,
    exts: Vec<Ext>,
    alpn: Vec<Vec<u8>>,
    sigalgs: Vec<u16>,
    groups: Vec<u16>,
    cert_compression: Vec<u16>,
    shares: Shares,
    versions: Vec<u16>,
    shuffle: bool,
}

fn key_share_body(shares: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut inner = Vec::new();
    for (g, d) in shares {
        put_u16(&mut inner, *g);
        put_vec16(&mut inner, d);
    }
    let mut o = Vec::new();
    put_vec16(&mut o, &inner);
    o
}

fn sni_body(name: &str) -> Vec<u8> {
    if name.is_empty() || name.parse::<std::net::IpAddr>().is_ok() {
        return vec![];
    }
    let mut entry = vec![0u8];
    put_vec16(&mut entry, name.as_bytes());
    let mut o = Vec::new();
    put_vec16(&mut o, &entry);
    o
}

fn ech_grease_body(suites: &[(u16, u16)], payload_lens: &[u16]) -> Vec<u8> {
    let (kdf, aead) = suites[rand_below(suites.len() as u64) as usize];
    let config_id = rng_bytes::<1>()[0];
    let enc = PublicKey::from(&StaticSecret::random()).to_bytes();
    let payload_len = payload_lens[rand_below(payload_lens.len() as u64) as usize] as usize + 16;
    let mut payload = vec![0u8; payload_len];
    SystemRandom::new().fill(&mut payload).expect("system RNG");
    let mut o = vec![0u8];
    put_u16(&mut o, kdf);
    put_u16(&mut o, aead);
    o.push(config_id);
    put_vec16(&mut o, &enc);
    put_vec16(&mut o, &payload);
    o
}

fn chrome_shuffle(exts: &mut [Ext]) {
    let fixed = |e: &Ext| matches!(e, Ext::Grease1 | Ext::Grease2 | Ext::Padding);
    for i in (1..exts.len()).rev() {
        let j = rand_below(i as u64 + 1) as usize;
        if fixed(&exts[i]) || fixed(&exts[j]) {
            continue;
        }
        exts.swap(i, j);
    }
}

const GREASE: u16 = 0x0a0a;
const TLS13_12: [u16; 3] = [GREASE, 0x0304, 0x0303];
const TLS13_TO_10: [u16; 5] = [GREASE, 0x0304, 0x0303, 0x0302, 0x0301];

const SAFARI_SUITES: [u16; 21] = [
    GREASE, 0x1302, 0x1303, 0x1301, 0xc02c, 0xc02b, 0xcca9, 0xc030, 0xc02f, 0xcca8, 0xc00a, 0xc009, 0xc014, 0xc013, 0x009d, 0x009c, 0x0035,
    0x002f, 0xc008, 0xc012, 0x000a,
];
const SAFARI_SIGALGS: [u16; 10] = [0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0805, 0x0501, 0x0806, 0x0601, 0x0201];
const IOS_SUITES: [u16; 27] = [
    GREASE, 0x1301, 0x1302, 0x1303, 0xc02c, 0xc02b, 0xcca9, 0xc030, 0xc02f, 0xcca8, 0xc024, 0xc023, 0xc00a, 0xc009, 0xc028, 0xc027, 0xc014,
    0xc013, 0x009d, 0x009c, 0x003d, 0x003c, 0x0035, 0x002f, 0xc008, 0xc012, 0x000a,
];
const IOS_SIGALGS: [u16; 11] = [0x0403, 0x0804, 0x0401, 0x0503, 0x0203, 0x0805, 0x0805, 0x0501, 0x0806, 0x0601, 0x0201];
const EDGE_SUITES: [u16; 16] =
    [GREASE, 0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035];
const EDGE_SIGALGS: [u16; 8] = CHROME_SIGALGS;

fn template(fp: Fingerprint, server_name: &str) -> Template {
    let alpn = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let sni = Some(Ext::Raw(EXT_SERVER_NAME, sni_body(server_name)));
    let boring_head = |exts: &mut Vec<Ext>, session_ticket: bool| {
        exts.push(Ext::Grease1);
        exts.extend(sni.as_ref().map(|e| match e {
            Ext::Raw(t, b) => Ext::Raw(*t, b.clone()),
            _ => unreachable!(),
        }));
        exts.extend([
            Ext::Raw(EXT_EXTENDED_MASTER_SECRET, vec![]),
            Ext::Raw(EXT_RENEGOTIATION_INFO, vec![0]),
            Ext::Raw(EXT_SUPPORTED_GROUPS, vec![]),
            Ext::Raw(EXT_EC_POINT_FORMATS, vec![1, 0]),
        ]);
        if session_ticket {
            exts.push(Ext::Raw(EXT_SESSION_TICKET, vec![]));
        }
        exts.push(Ext::Raw(EXT_ALPN, proto_list(&alpn)));
        exts.push(Ext::Raw(EXT_STATUS_REQUEST, vec![1, 0, 0, 0, 0]));
    };
    match fp {
        Fingerprint::Randomized
        | Fingerprint::RandomizedNoAlpn
        | Fingerprint::HelloRandomized
        | Fingerprint::HelloRandomizedAlpn
        | Fingerprint::HelloRandomizedNoAlpn => randomized::template(fp, server_name, &[]),
        Fingerprint::Chrome120 | Fingerprint::Chrome131 => {
            let mut t = template(Fingerprint::Chrome, server_name);
            for ext in &mut t.exts {
                if let Ext::Raw(id, _) = ext
                    && *id == EXT_APPLICATION_SETTINGS_NEW
                {
                    *id = EXT_APPLICATION_SETTINGS_OLD;
                }
            }
            if fp == Fingerprint::Chrome120 {
                t.groups.retain(|g| *g != GROUP_X25519MLKEM768);
                t.shares = Shares::GreaseX25519;
                t.exts.push(Ext::Padding);
            }
            t
        }
        Fingerprint::Firefox120 => {
            let mut t = template(Fingerprint::Firefox, server_name);
            t.groups.retain(|g| *g != GROUP_X25519MLKEM768);
            t.shares = Shares::FirefoxClassical;
            t.cert_compression.clear();
            t.exts.retain(|e| !matches!(e, Ext::Raw(EXT_SCT | EXT_COMPRESS_CERTIFICATE, _)));
            for e in &mut t.exts {
                if let Ext::Raw(EXT_SUPPORTED_GROUPS, b) = e {
                    *b = u16_list(&t.groups);
                }
            }
            let at = t.exts.iter().position(|e| matches!(e, Ext::Raw(EXT_ALPN, _))).unwrap();
            t.exts.insert(at, Ext::Raw(EXT_SESSION_TICKET, vec![]));
            let at = t.exts.iter().position(|e| matches!(e, Ext::Raw(EXT_RECORD_SIZE_LIMIT, _))).unwrap();
            t.exts.insert(at, Ext::Raw(EXT_PSK_KEY_EXCHANGE_MODES, vec![1, 1]));
            t
        }
        Fingerprint::Ios13 => Template {
            suites: vec![
                0x1301, 0x1302, 0x1303, 0xc02c, 0xc02b, 0xc024, 0xc023, 0xc00a, 0xc009, 0xcca9, 0xc030, 0xc02f, 0xc028, 0xc027, 0xc014,
                0xc013, 0xcca8, 0x009d, 0x009c, 0x003d, 0x003c, 0x0035, 0x002f, 0xc008, 0xc012, 0x000a,
            ],
            exts: {
                let mut exts = vec![Ext::Raw(EXT_RENEGOTIATION_INFO, vec![0])];
                exts.extend(sni);
                exts.extend([
                    Ext::Raw(EXT_EXTENDED_MASTER_SECRET, vec![]),
                    Ext::Raw(EXT_SIGNATURE_ALGORITHMS, u16_list(&IOS_SIGALGS)),
                    Ext::Raw(EXT_STATUS_REQUEST, vec![1, 0, 0, 0, 0]),
                    Ext::Raw(EXT_SCT, vec![]),
                    Ext::Raw(EXT_ALPN, proto_list(&alpn)),
                    Ext::Raw(EXT_EC_POINT_FORMATS, vec![1, 0]),
                    Ext::KeyShare,
                    Ext::Raw(EXT_PSK_KEY_EXCHANGE_MODES, vec![1, 1]),
                    Ext::Raw(EXT_SUPPORTED_VERSIONS, vec![8, 3, 4, 3, 3, 3, 2, 3, 1]),
                    Ext::Raw(EXT_SUPPORTED_GROUPS, vec![]),
                    Ext::Padding,
                ]);
                exts
            },
            alpn,
            sigalgs: IOS_SIGALGS.to_vec(),
            groups: vec![GROUP_X25519, GROUP_SECP256R1, GROUP_SECP384R1, GROUP_SECP521R1],
            cert_compression: vec![],
            shares: Shares::X25519,
            versions: vec![],
            shuffle: false,
        },
        Fingerprint::Edge106 => {
            let mut t = template(Fingerprint::Qq, server_name);
            t.versions = TLS13_12.to_vec();
            t
        }
        Fingerprint::Qihoo11 => {
            let mut t = template(Fingerprint::Edge, server_name);
            t.suites.push(0x000a);
            t.sigalgs.push(0x0201);
            for e in &mut t.exts {
                if let Ext::Raw(EXT_SIGNATURE_ALGORITHMS, b) = e {
                    *b = u16_list(&t.sigalgs);
                }
            }
            let at = t.exts.iter().position(|e| matches!(e, Ext::KeyShare)).unwrap();
            t.exts.insert(at, Ext::Raw(30032, vec![]));
            t
        }
        Fingerprint::Chrome => {
            let mut exts = vec![Ext::Grease1];
            exts.extend(sni);
            exts.extend([
                Ext::Raw(EXT_EXTENDED_MASTER_SECRET, vec![]),
                Ext::Raw(EXT_RENEGOTIATION_INFO, vec![0]),
                Ext::Raw(EXT_SUPPORTED_GROUPS, vec![]),
                Ext::Raw(EXT_EC_POINT_FORMATS, vec![1, 0]),
                Ext::Raw(EXT_SESSION_TICKET, vec![]),
                Ext::Raw(EXT_ALPN, proto_list(&alpn)),
                Ext::Raw(EXT_STATUS_REQUEST, vec![1, 0, 0, 0, 0]),
                Ext::Raw(EXT_SIGNATURE_ALGORITHMS, u16_list(&CHROME_SIGALGS)),
                Ext::Raw(EXT_SCT, vec![]),
                Ext::KeyShare,
                Ext::Raw(EXT_PSK_KEY_EXCHANGE_MODES, vec![1, 1]),
                Ext::Raw(EXT_SUPPORTED_VERSIONS, vec![]),
                Ext::Raw(EXT_COMPRESS_CERTIFICATE, vec![2, 0, 2]),
                Ext::Raw(EXT_APPLICATION_SETTINGS_NEW, proto_list(&[b"h2".to_vec()])),
                Ext::Raw(EXT_ECH, ech_grease_body(&[(0x0001, 0x0001)], &[128, 160, 192, 224])),
                Ext::Grease2,
            ]);
            Template {
                suites: CHROME_SUITES.to_vec(),
                exts,
                alpn,
                sigalgs: CHROME_SIGALGS.to_vec(),
                groups: CHROME_GROUPS.to_vec(),
                cert_compression: vec![CERT_COMPRESSION_BROTLI],
                shares: Shares::GreaseHybrid,
                versions: TLS13_12.to_vec(),
                shuffle: true,
            }
        }
        Fingerprint::Firefox => {
            let mut exts = Vec::new();
            exts.extend(sni);
            exts.extend([
                Ext::Raw(EXT_EXTENDED_MASTER_SECRET, vec![]),
                Ext::Raw(EXT_RENEGOTIATION_INFO, vec![0]),
                Ext::Raw(EXT_SUPPORTED_GROUPS, u16_list(&FIREFOX_GROUPS)),
                Ext::Raw(EXT_EC_POINT_FORMATS, vec![1, 0]),
                Ext::Raw(EXT_ALPN, proto_list(&alpn)),
                Ext::Raw(EXT_STATUS_REQUEST, vec![1, 0, 0, 0, 0]),
                Ext::Raw(EXT_DELEGATED_CREDENTIALS, u16_list(&FIREFOX_DC_SIGALGS)),
                Ext::Raw(EXT_SCT, vec![]),
                Ext::KeyShare,
                Ext::Raw(EXT_SUPPORTED_VERSIONS, vec![4, 0x03, 0x04, 0x03, 0x03]),
                Ext::Raw(EXT_SIGNATURE_ALGORITHMS, u16_list(&FIREFOX_SIGALGS)),
                Ext::Raw(EXT_RECORD_SIZE_LIMIT, vec![0x40, 0x01]),
                Ext::Raw(EXT_COMPRESS_CERTIFICATE, vec![6, 0, 1, 0, 2, 0, 3]),
                Ext::Raw(EXT_ECH, ech_grease_body(&[(0x0001, 0x0001), (0x0001, 0x0003)], &[223])),
            ]);
            Template {
                suites: FIREFOX_SUITES.to_vec(),
                exts,
                alpn,
                sigalgs: FIREFOX_SIGALGS.to_vec(),
                groups: FIREFOX_GROUPS.to_vec(),
                cert_compression: vec![CERT_COMPRESSION_ZLIB, CERT_COMPRESSION_BROTLI, CERT_COMPRESSION_ZSTD],
                shares: Shares::FirefoxReuse,
                versions: vec![],
                shuffle: false,
            }
        }
        Fingerprint::Safari => {
            let mut exts = Vec::new();
            boring_head(&mut exts, false);
            exts.extend([
                Ext::Raw(EXT_SIGNATURE_ALGORITHMS, u16_list(&SAFARI_SIGALGS)),
                Ext::Raw(EXT_SCT, vec![]),
                Ext::KeyShare,
                Ext::Raw(EXT_PSK_KEY_EXCHANGE_MODES, vec![1, 1]),
                Ext::Raw(EXT_SUPPORTED_VERSIONS, vec![]),
                Ext::Raw(EXT_COMPRESS_CERTIFICATE, vec![2, 0, 1]),
                Ext::Grease2,
            ]);
            Template {
                suites: SAFARI_SUITES.to_vec(),
                exts,
                alpn,
                sigalgs: SAFARI_SIGALGS.to_vec(),
                groups: vec![GREASE, GROUP_X25519MLKEM768, GROUP_X25519, GROUP_SECP256R1, GROUP_SECP384R1, GROUP_SECP521R1],
                cert_compression: vec![CERT_COMPRESSION_ZLIB],
                shares: Shares::GreaseHybrid,
                versions: TLS13_12.to_vec(),
                shuffle: false,
            }
        }
        Fingerprint::Ios => {
            let mut exts = Vec::new();
            boring_head(&mut exts, false);
            exts.extend([
                Ext::Raw(EXT_SIGNATURE_ALGORITHMS, u16_list(&IOS_SIGALGS)),
                Ext::Raw(EXT_SCT, vec![]),
                Ext::KeyShare,
                Ext::Raw(EXT_PSK_KEY_EXCHANGE_MODES, vec![1, 1]),
                Ext::Raw(EXT_SUPPORTED_VERSIONS, vec![]),
                Ext::Grease2,
                Ext::Padding,
            ]);
            Template {
                suites: IOS_SUITES.to_vec(),
                exts,
                alpn,
                sigalgs: IOS_SIGALGS.to_vec(),
                groups: vec![GREASE, GROUP_X25519, GROUP_SECP256R1, GROUP_SECP384R1, GROUP_SECP521R1],
                cert_compression: vec![],
                shares: Shares::GreaseX25519,
                versions: TLS13_TO_10.to_vec(),
                shuffle: false,
            }
        }
        Fingerprint::Edge | Fingerprint::Qq => {
            let mut exts = Vec::new();
            boring_head(&mut exts, true);
            exts.extend([
                Ext::Raw(EXT_SIGNATURE_ALGORITHMS, u16_list(&EDGE_SIGALGS)),
                Ext::Raw(EXT_SCT, vec![]),
                Ext::KeyShare,
                Ext::Raw(EXT_PSK_KEY_EXCHANGE_MODES, vec![1, 1]),
                Ext::Raw(EXT_SUPPORTED_VERSIONS, vec![]),
                Ext::Raw(EXT_COMPRESS_CERTIFICATE, vec![2, 0, 2]),
            ]);
            if fp == Fingerprint::Qq {
                exts.push(Ext::Raw(EXT_APPLICATION_SETTINGS_OLD, proto_list(&[b"h2".to_vec()])));
            }
            exts.extend([Ext::Grease2, Ext::Padding]);
            Template {
                suites: EDGE_SUITES.to_vec(),
                exts,
                alpn,
                sigalgs: EDGE_SIGALGS.to_vec(),
                groups: vec![GREASE, GROUP_X25519, GROUP_SECP256R1, GROUP_SECP384R1],
                cert_compression: vec![CERT_COMPRESSION_BROTLI],
                shares: Shares::GreaseX25519,
                versions: TLS13_TO_10.to_vec(),
                shuffle: false,
            }
        }
        _ => presets::template(fp, server_name),
    }
}

fn gen_x25519() -> (StaticSecret, [u8; 32]) {
    let k = StaticSecret::random();
    let p = PublicKey::from(&k).to_bytes();
    (k, p)
}

fn gen_hybrid(keys: &mut KeyShareKeys) -> Vec<u8> {
    let (xk, xp) = gen_x25519();
    let seed = rng_bytes::<64>();
    let dk = DecapsulationKey768::from_seed(seed.into());
    let mut data = dk.encapsulation_key().to_bytes().to_vec();
    data.extend_from_slice(&xp);
    keys.mlkem = Some(dk);
    keys.hybrid_x25519 = Some(xk);
    keys.groups.push(GROUP_X25519MLKEM768);
    data
}

fn gen_nist_hybrid(group: u16, keys: &mut KeyShareKeys) -> Vec<u8> {
    let alg = if group == GROUP_P256MLKEM768 { &agreement::ECDH_P256 } else { &agreement::ECDH_P384 };
    let k = agreement::EphemeralPrivateKey::generate(alg, &SystemRandom::new()).expect("NIST keygen");
    let mut out = k.compute_public_key().expect("NIST pub").as_ref().to_vec();
    if group == GROUP_P256MLKEM768 {
        let dk = DecapsulationKey768::from_seed(rng_bytes::<64>().into());
        out.extend_from_slice(&dk.encapsulation_key().to_bytes());
        keys.mlkem = Some(dk);
        keys.p256 = Some(k);
    } else {
        let dk = DecapsulationKey1024::from_seed(rng_bytes::<64>().into());
        out.extend_from_slice(&dk.encapsulation_key().to_bytes());
        keys.mlkem1024 = Some(dk);
        keys.p384 = Some(k);
    }
    keys.groups.push(group);
    out
}

fn gen_p256(keys: &mut KeyShareKeys) -> Vec<u8> {
    let k = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &SystemRandom::new()).expect("P-256 keygen");
    let p = k.compute_public_key().expect("P-256 pub").as_ref().to_vec();
    keys.p256 = Some(k);
    keys.groups.push(GROUP_SECP256R1);
    p
}

fn key_shares(kind: Shares, grease_group: u16, keys: &mut KeyShareKeys) -> Vec<(u16, Vec<u8>)> {
    match kind {
        Shares::List(groups) => groups
            .into_iter()
            .map(|group| {
                let data = match group {
                    GREASE => return (grease_group, vec![0]),
                    GROUP_X25519 => {
                        let (k, p) = gen_x25519();
                        keys.x25519 = Some(k);
                        keys.groups.push(group);
                        p.to_vec()
                    }
                    GROUP_SECP256R1 => gen_p256(keys),
                    GROUP_X25519MLKEM768 => gen_hybrid(keys),
                    GROUP_X25519KYBER768 => {
                        let data = gen_hybrid(keys);
                        *keys.groups.last_mut().unwrap() = group;
                        [data[1184..].as_ref(), &data[..1184]].concat()
                    }
                    _ => unreachable!("unsupported template share"),
                };
                (group, data)
            })
            .collect(),
        Shares::GoHybridReuse => {
            let hybrid = gen_hybrid(keys);
            let k = keys.hybrid_x25519.clone().unwrap();
            let p = PublicKey::from(&k).to_bytes();
            keys.x25519 = Some(k);
            keys.groups.push(GROUP_X25519);
            vec![(GROUP_X25519MLKEM768, hybrid), (GROUP_X25519, p.to_vec())]
        }
        Shares::GreaseHybrid => {
            let hybrid = gen_hybrid(keys);
            let (xk, xp) = gen_x25519();
            keys.x25519 = Some(xk);
            keys.groups.push(GROUP_X25519);
            vec![(grease_group, vec![0]), (GROUP_X25519MLKEM768, hybrid), (GROUP_X25519, xp.to_vec())]
        }
        Shares::FirefoxReuse => {
            let hybrid = gen_hybrid(keys);
            let xk = keys.hybrid_x25519.clone().unwrap();
            let xp = PublicKey::from(&xk).to_bytes();
            keys.x25519 = Some(xk);
            keys.groups.push(GROUP_X25519);
            let p = gen_p256(keys);
            vec![(GROUP_X25519MLKEM768, hybrid), (GROUP_X25519, xp.to_vec()), (GROUP_SECP256R1, p)]
        }
        Shares::GreaseX25519 => {
            let (xk, xp) = gen_x25519();
            keys.x25519 = Some(xk);
            keys.groups.push(GROUP_X25519);
            vec![(grease_group, vec![0]), (GROUP_X25519, xp.to_vec())]
        }
        Shares::X25519 | Shares::FirefoxClassical => {
            let (xk, xp) = gen_x25519();
            keys.x25519 = Some(xk);
            keys.groups.push(GROUP_X25519);
            let mut out = vec![(GROUP_X25519, xp.to_vec())];
            if kind == Shares::FirefoxClassical {
                out.push((GROUP_SECP256R1, gen_p256(keys)));
            }
            out
        }
    }
}

fn boring_padding(unpadded: usize) -> Option<usize> {
    if unpadded > 0xff && unpadded < 0x200 {
        let len = 0x200 - unpadded;
        Some(if len >= 5 { len - 4 } else { 1 })
    } else {
        None
    }
}

fn encode(random: &[u8; 32], session_id: &[u8; 32], suites: &[u16], exts: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let ext_len: usize = exts.iter().map(|(_, b)| 4 + b.len()).sum();
    let mut body = Vec::with_capacity(2 + 32 + 33 + 2 + suites.len() * 2 + 2 + 2 + ext_len);
    body.extend_from_slice(&[3, 3]);
    body.extend_from_slice(random);
    put_vec8(&mut body, session_id);
    body.extend(u16_list(suites));
    body.extend_from_slice(&[1, 0]);
    put_u16(&mut body, ext_len as u16);
    for (t, b) in exts {
        put_u16(&mut body, *t);
        put_vec16(&mut body, b);
    }
    let mut msg = Vec::with_capacity(4 + body.len());
    msg.push(1);
    msg.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    msg.extend(body);
    msg
}

pub(crate) fn build(fp: Fingerprint, server_name: &str, alpn_override: &[Vec<u8>], force_alpn: bool) -> Result<ClientHello, &'static str> {
    let t = if fp.uses_configured_alpn() && !matches!(fp, Fingerprint::Golang | Fingerprint::UtlsGolang) {
        randomized::template(fp, server_name, alpn_override)
    } else {
        template(fp, server_name)
    };
    build_template(t, fp, alpn_override, force_alpn)
}

fn build_template(t: Template, fp: Fingerprint, alpn_override: &[Vec<u8>], force_alpn: bool) -> Result<ClientHello, &'static str> {
    let seed = grease_seed();
    let Template { mut suites, exts: mut tmpl, mut alpn, sigalgs, mut groups, cert_compression, shares: share_kind, versions, shuffle } = t;
    if matches!(fp, Fingerprint::Golang | Fingerprint::UtlsGolang) || (force_alpn && !alpn_override.is_empty()) {
        alpn = alpn_override.to_vec();
        let at = tmpl.iter().position(|e| matches!(e, Ext::Raw(EXT_ALPN, _)));
        match at {
            Some(i) if alpn.is_empty() => {
                tmpl.remove(i);
            }
            Some(i) => tmpl[i] = Ext::Raw(EXT_ALPN, proto_list(&alpn)),
            None if !alpn.is_empty() => tmpl.push(Ext::Raw(EXT_ALPN, proto_list(&alpn))),
            None => {}
        }
    }
    for s in suites.iter_mut().filter(|s| is_grease(**s)) {
        *s = grease_value(&seed, GREASE_CIPHER);
    }
    let grease_group = grease_value(&seed, GREASE_GROUP);
    for g in groups.iter_mut().filter(|g| is_grease(**g)) {
        *g = grease_group;
    }
    if shuffle {
        chrome_shuffle(&mut tmpl);
    }

    let mut keys = KeyShareKeys::empty();
    let shares = key_shares(share_kind, grease_group, &mut keys);
    let mut alps = None;
    let mut exts: Vec<(u16, Vec<u8>)> = Vec::with_capacity(tmpl.len());
    let mut padding_at = None;
    for e in tmpl {
        exts.push(match e {
            Ext::Grease1 => (grease_value(&seed, GREASE_EXT1), vec![]),
            Ext::Grease2 => (grease_value(&seed, GREASE_EXT2), vec![0]),
            Ext::KeyShare => (EXT_KEY_SHARE, key_share_body(&shares)),
            Ext::Padding => {
                padding_at = Some(exts.len());
                continue;
            }
            Ext::Raw(EXT_SUPPORTED_GROUPS, b) if b.is_empty() => (EXT_SUPPORTED_GROUPS, u16_list(&groups)),
            Ext::Raw(EXT_SUPPORTED_VERSIONS, b) if b.is_empty() => {
                let gv = grease_value(&seed, GREASE_VERSION);
                let mut o = vec![(versions.len() * 2) as u8];
                for &v in &versions {
                    put_u16(&mut o, if is_grease(v) { gv } else { v });
                }
                (EXT_SUPPORTED_VERSIONS, o)
            }
            Ext::Raw(EXT_SERVER_NAME, b) if b.is_empty() => continue,
            Ext::Raw(t, b) => {
                if t == EXT_APPLICATION_SETTINGS_NEW || t == EXT_APPLICATION_SETTINGS_OLD {
                    alps = Some(t);
                }
                (t, b)
            }
        });
    }
    if let Some(at) = padding_at {
        let head = 2 + 32 + 1 + 32 + 2 + suites.len() * 2 + 2;
        let ext_len: usize = exts.iter().map(|(_, b)| 4 + b.len()).sum();
        if let Some(n) = boring_padding(4 + head + 2 + ext_len) {
            exts.insert(at, (EXT_PADDING, vec![0; n]));
        }
    }

    if alpn.iter().any(|p| p.is_empty() || p.len() > 0xff) {
        return Err(INVALID_ALPN);
    }
    if !exts_fit(&exts) {
        return Err("ClientHello extensions exceed 65535 bytes");
    }
    let random = rng_bytes::<32>();
    let session_id = rng_bytes::<32>();
    let msg = encode(&random, &session_id, &suites, &exts);
    Ok(ClientHello {
        msg,
        random,
        session_id_offset: 39,
        tls13_suites: suites.iter().copied().filter(|s| (0x1301..=0x1303).contains(s)).collect(),
        cert_compression,
        alpn,
        signature_algorithms: sigalgs,
        supported_groups: groups.into_iter().filter(|g| !is_grease(*g)).collect(),
        alps,
        keys,
        suites_raw: suites,
        exts,
    })
}

pub(crate) fn rebuild_after_hrr(
    first: &mut ClientHello,
    fp: Fingerprint,
    selected_group: u16,
    cookie: Option<&[u8]>,
) -> Result<ClientHello, &'static str> {
    let mut keys = KeyShareKeys::empty();
    let mut exts = first.exts.clone();
    if selected_group != 0 {
        if !first.supported_groups.contains(&selected_group) {
            return Err("HRR selected a group we did not offer");
        }
        if first.keys.groups.contains(&selected_group) {
            return Err("HRR asked for a group we already sent");
        }
        if fp != Fingerprint::Golang
            && matches!(selected_group, GROUP_X25519MLKEM768 | GROUP_X25519KYBER768 | GROUP_P256MLKEM768 | GROUP_P384MLKEM1024)
        {
            return Err(UTLS_HRR_GROUP_ERROR);
        }
        let data = match selected_group {
            GROUP_X25519 => {
                let (k, p) = gen_x25519();
                keys.x25519 = Some(k);
                keys.groups.push(GROUP_X25519);
                p.to_vec()
            }
            GROUP_SECP256R1 => gen_p256(&mut keys),
            GROUP_SECP384R1 => {
                let k = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P384, &SystemRandom::new()).expect("P-384 keygen");
                let p = k.compute_public_key().expect("P-384 pub").as_ref().to_vec();
                keys.p384 = Some(k);
                keys.groups.push(GROUP_SECP384R1);
                p
            }
            GROUP_X25519MLKEM768 => gen_hybrid(&mut keys),
            GROUP_P256MLKEM768 | GROUP_P384MLKEM1024 => gen_nist_hybrid(selected_group, &mut keys),
            GROUP_X25519KYBER768 => {
                let data = gen_hybrid(&mut keys);
                *keys.groups.last_mut().unwrap() = selected_group;
                [data[1184..].as_ref(), &data[..1184]].concat()
            }
            GROUP_SECP521R1 => {
                let k = crate::p521::Ephemeral::generate().ok_or("P-521 keygen")?;
                let p = k.public.clone();
                keys.p521 = Some(k);
                keys.groups.push(GROUP_SECP521R1);
                p
            }
            _ => return Err("HRR selected an unsupported group"),
        };
        let body = key_share_body(&[(selected_group, data)]);
        for e in exts.iter_mut().filter(|e| e.0 == EXT_KEY_SHARE) {
            e.1 = body.clone();
        }
    } else {
        if cookie.is_none_or(|c| c.is_empty()) {
            return Err("HRR has neither a selected group nor a cookie");
        }
        keys = std::mem::replace(&mut first.keys, KeyShareKeys::empty());
    }
    if let Some(c) = cookie {
        let mut b = Vec::new();
        put_vec16(&mut b, c);
        let idx = rand_below(exts.len().saturating_sub(2).max(1) as u64) as usize;
        exts.insert(idx, (EXT_COOKIE, b));
    }
    if let Some(at) = exts.iter().position(|e| e.0 == EXT_PADDING) {
        exts.remove(at);
        let head = 2 + 32 + 1 + 32 + 2 + first.suites_raw.len() * 2 + 2;
        let ext_len: usize = exts.iter().map(|(_, b)| 4 + b.len()).sum();
        if let Some(n) = boring_padding(4 + head + 2 + ext_len) {
            exts.insert(at, (EXT_PADDING, vec![0; n]));
        }
    }
    if !exts_fit(&exts) {
        return Err("HRR cookie does not fit in ClientHello");
    }
    let sid: [u8; 32] = first.msg[39..71].try_into().unwrap();
    let msg = encode(&first.random, &sid, &first.suites_raw, &exts);
    Ok(ClientHello {
        msg,
        random: first.random,
        session_id_offset: 39,
        tls13_suites: first.tls13_suites.clone(),
        cert_compression: first.cert_compression.clone(),
        alpn: first.alpn.clone(),
        signature_algorithms: first.signature_algorithms.clone(),
        supported_groups: first.supported_groups.clone(),
        alps: first.alps,
        keys,
        suites_raw: first.suites_raw.clone(),
        exts,
    })
}

#[cfg(test)]
mod tests;
