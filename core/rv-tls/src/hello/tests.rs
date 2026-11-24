use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn build(fp: Fingerprint, name: &str, alpn: &[Vec<u8>]) -> ClientHello {
    super::build(fp, name, alpn, true).unwrap()
}

const FIXTURES: &str = include_str!("../../tests/fixtures/utls_hellos.json");

#[derive(Debug, Clone, PartialEq, Eq)]
struct Hello {
    legacy: u16,
    compression: Vec<u8>,
    session_len: usize,
    suites: Vec<String>,
    order: Vec<String>,
    bodies: BTreeMap<String, String>,
    shares: BTreeMap<u16, Vec<u8>>,
    ech: (u16, u16, usize),
    grease_differs: bool,
}

fn u16at(b: &[u8], i: &mut usize) -> u16 {
    let v = u16::from_be_bytes([b[*i], b[*i + 1]]);
    *i += 2;
    v
}
fn vec16<'a>(b: &'a [u8], i: &mut usize) -> &'a [u8] {
    let n = u16at(b, i) as usize;
    let v = &b[*i..*i + n];
    *i += n;
    v
}
fn tag(v: u16) -> String {
    if is_grease(v) { "GREASE".into() } else { format!("{v:04x}") }
}
fn hex_bytes(b: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(b.len() * 2);
    for &v in b {
        out.push(DIGITS[(v >> 4) as usize] as char);
        out.push(DIGITS[(v & 15) as usize] as char);
    }
    out
}
fn normalized_values(b: &[u8]) -> String {
    let mut i = 0;
    let n = u16at(b, &mut i) as usize;
    let vals: Vec<_> = b[i..i + n].as_chunks::<2>().0.iter().map(|p| tag(u16::from_be_bytes([p[0], p[1]]))).collect();
    vals.join(",")
}
fn parse(raw: &[u8]) -> Hello {
    assert_eq!(raw[0], 1);
    let n = ((raw[1] as usize) << 16) | ((raw[2] as usize) << 8) | raw[3] as usize;
    assert_eq!(n, raw.len() - 4);
    let b = &raw[4..];
    let mut i = 0;
    let legacy = u16at(b, &mut i);
    i += 32;
    let session_len = b[i] as usize;
    i += 1 + session_len;
    let suite_len = u16at(b, &mut i) as usize;
    let suites = b[i..i + suite_len].as_chunks::<2>().0.iter().map(|p| tag(u16::from_be_bytes([p[0], p[1]]))).collect();
    i += suite_len;
    let comp_len = b[i] as usize;
    let compression = b[i + 1..i + 1 + comp_len].to_vec();
    i += 1 + comp_len;
    let end = i + u16at(b, &mut i) as usize;
    let mut order = Vec::new();
    let mut bodies = BTreeMap::new();
    let mut shares = BTreeMap::new();
    let mut ech = (0, 0, 0);
    let mut grease_values = Vec::new();
    while i < end {
        let typ = u16at(b, &mut i);
        let body = vec16(b, &mut i);
        let key = tag(typ);
        order.push(key.clone());
        let norm = match typ {
            t if is_grease(t) => {
                grease_values.push(body.to_vec());
                hex_bytes(body)
            }
            10 => normalized_values(body),
            43 => {
                let n = body[0] as usize;
                let vals: Vec<_> = body[1..1 + n].as_chunks::<2>().0.iter().map(|p| tag(u16::from_be_bytes([p[0], p[1]]))).collect();
                vals.join(",")
            }
            51 => {
                let mut p = 0;
                let list = vec16(body, &mut p);
                let mut q = 0;
                let mut items = Vec::new();
                while q < list.len() {
                    let group = u16at(list, &mut q);
                    let data = vec16(list, &mut q).to_vec();
                    items.push(format!("{}:{}", tag(group), data.len()));
                    shares.insert(group, data);
                }
                items.join(",")
            }
            0xfe0d => {
                let mut p = 1;
                let kdf = u16at(body, &mut p);
                let aead = u16at(body, &mut p);
                p += 1;
                let enc = vec16(body, &mut p).len();
                let payload = vec16(body, &mut p).len();
                ech = (kdf, aead, payload);
                format!("{}:{}", kdf, enc)
            }
            _ => hex_bytes(body),
        };
        bodies.insert(key, norm);
    }
    Hello {
        legacy,
        compression,
        session_len,
        suites,
        order,
        bodies,
        shares,
        ech,
        grease_differs: grease_values.len() < 2 || grease_values[0] != grease_values[1],
    }
}

fn fixture(name: &str) -> Vec<Vec<u8>> {
    let needle = format!("\"{name}\"");
    let p = FIXTURES.find(&needle).unwrap() + needle.len();
    let start = FIXTURES[p..].find('[').unwrap() + p + 1;
    let end = FIXTURES[start..].find(']').unwrap() + start;
    let array = &FIXTURES[start..end];
    let mut result = Vec::new();
    let mut rest = array;
    while let Some(q) = rest.find('"') {
        rest = &rest[q + 1..];
        let close = rest.find('"').unwrap();
        result.push(decode_hex(&rest[..close]));
        rest = &rest[close + 1..];
    }
    result
}
fn decode_hex(s: &str) -> Vec<u8> {
    s.as_bytes().as_chunks::<2>().0.iter().map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
}
fn our(fp: Fingerprint) -> Vec<Vec<u8>> {
    (0..40).map(|_| build(fp, "www.example.com", &[]).msg).collect()
}
fn shares(h: &Hello) -> (&[u8], &[u8], Option<&[u8]>) {
    (&h.shares[&GROUP_X25519MLKEM768], &h.shares[&GROUP_X25519], h.shares.get(&GROUP_SECP256R1).map(Vec::as_slice))
}
fn ech_sets(h: &[Hello]) -> (BTreeSet<(u16, u16)>, BTreeSet<usize>) {
    (h.iter().map(|x| (x.ech.0, x.ech.1)).collect(), h.iter().map(|x| x.ech.2).collect())
}
fn firefox_normalized_equal(a: &Hello, b: &Hello) -> bool {
    a.legacy == b.legacy
        && a.compression == b.compression
        && a.session_len == b.session_len
        && a.suites == b.suites
        && a.order == b.order
        && a.bodies == b.bodies
}

#[test]
fn utls_hello_structural_fingerprints() {
    let cases = [("chrome", Fingerprint::Chrome), ("firefox", Fingerprint::Firefox)];
    for (name, fp) in cases {
        let expected_raw = fixture(name);
        assert_eq!(expected_raw.len(), 40, "{name} fixtures");
        let ours_raw = our(fp);
        let expected: Vec<_> = expected_raw.iter().map(|x| parse(x)).collect();
        let ours: Vec<_> = ours_raw.iter().map(|x| parse(x)).collect();
        for (label, set) in [("ours", &ours), ("uTLS", &expected)] {
            for h in set {
                assert_eq!(h.legacy, 0x0303, "{name} {label} legacy_version");
                assert_eq!(h.session_len, 32, "{name} {label} session id");
                assert_eq!(h.compression, [0], "{name} {label} compression methods");
                assert!(h.grease_differs, "{name} {label} GREASE extension bodies");
            }
        }
        let (eh, ex, _) = shares(&expected[0]);
        let (oh, ox, _) = shares(&ours[0]);
        assert_eq!(eh.len(), 1216, "{name} uTLS hybrid key_share length");
        assert_eq!(oh.len(), 1216, "{name} our hybrid key_share length");
        assert_eq!(ex.len(), 32);
        assert_eq!(ox.len(), 32);
        for (label, samples) in [("uTLS", &expected), ("ours", &ours)] {
            for (idx, h) in samples.iter().enumerate() {
                let (hybrid, classical, p256) = shares(h);
                assert_eq!(hybrid.len(), 1216, "{name} {label} sample {idx} hybrid key share");
                assert_eq!(classical.len(), 32, "{name} {label} sample {idx} X25519 key share");
                if fp == Fingerprint::Firefox {
                    assert_eq!(p256.unwrap().len(), 65, "Firefox {label} sample {idx} P-256 key share");
                    assert_eq!(&hybrid[1184..], classical, "Firefox {label} sample {idx} hybrid key reuse");
                } else {
                    assert_ne!(&hybrid[1184..], classical, "Chrome {label} sample {idx} hybrid/classical X25519 shares");
                }
            }
        }
        if fp == Fingerprint::Firefox {
            assert_eq!(shares(&expected[0]).2.unwrap().len(), 65);
            assert_eq!(shares(&ours[0]).2.unwrap().len(), 65);
            assert_eq!(&eh[1184..], ex, "Firefox uTLS hybrid tail reuse");
            assert_eq!(&oh[1184..], ox, "Firefox our hybrid tail reuse");
            for idx in 0..40 {
                assert!(
                    firefox_normalized_equal(&ours[idx], &expected[idx]),
                    "Firefox normalized sample {idx} differs: ours order={:?} bodies={:?}, uTLS order={:?} bodies={:?}",
                    ours[idx].order,
                    ours[idx].bodies,
                    expected[idx].order,
                    expected[idx].bodies
                );
            }
            let (ours_k, ours_p) = ech_sets(&ours);
            let (utls_k, utls_p) = ech_sets(&expected);
            assert_eq!(ours_k, utls_k, "Firefox ECH kdf/aead set");
            assert_eq!(ours_p, utls_p, "Firefox ECH payload length set");
        } else {
            let first = &ours[0];
            let expected_first = &expected[0];
            assert_eq!(first.suites, expected_first.suites, "Chrome cipher suites");
            assert_eq!(first.bodies, expected_first.bodies, "Chrome normalized extension bodies");
            let mut a = first.order.clone();
            a.sort();
            let mut b = expected_first.order.clone();
            b.sort();
            assert_eq!(a, b, "Chrome extension multiset");
            for (label, hs) in [("ours", &ours), ("uTLS", &expected)] {
                assert!(
                    hs.iter().all(|h| h.order.first().is_some_and(|x| x == "GREASE") && h.order.last().is_some_and(|x| x == "GREASE")),
                    "Chrome {label} GREASE endpoints"
                );
                for h in hs.iter().skip(1) {
                    assert_eq!(h.suites, first.suites, "Chrome {label} cipher list");
                    assert_eq!(h.bodies, hs[0].bodies, "Chrome {label} extension bodies");
                }
            }
            let (ours_k, ours_p) = ech_sets(&ours);
            let (utls_k, utls_p) = ech_sets(&expected);
            assert_eq!(ours_k, utls_k, "Chrome ECH kdf/aead set");
            assert_eq!(ours_p, utls_p, "Chrome ECH payload lengths");
            assert_eq!(ours_p, [144, 176, 208, 240].into_iter().collect());
            let orders: BTreeSet<_> = ours.iter().map(|h| h.order[1..h.order.len() - 1].to_vec()).collect();
            assert!(orders.len() >= 2, "Chrome extension shuffle produced {} middle orders", orders.len());
        }
        println!("{name} {}", hex_bytes(&ours_raw[0]));
    }
}

#[test]
fn utls_static_fingerprints() {
    for (name, fp) in [("safari", Fingerprint::Safari), ("ios", Fingerprint::Ios), ("edge", Fingerprint::Edge), ("qq", Fingerprint::Qq)] {
        let expected_raw = fixture(name);
        assert_eq!(expected_raw.len(), 40, "{name} fixtures");
        let ours_raw = our(fp);
        for (i, (o, e)) in ours_raw.iter().zip(&expected_raw).enumerate() {
            assert_eq!(o.len(), e.len(), "{name} sample {i}: length");
            let (o, e) = (parse(o), parse(e));
            assert_eq!(o.legacy, e.legacy, "{name} legacy_version");
            assert_eq!(o.compression, e.compression, "{name} compression");
            assert_eq!(o.session_len, e.session_len, "{name} session id");
            assert_eq!(o.suites, e.suites, "{name} cipher suites");
            assert_eq!(o.order, e.order, "{name} extension order");
            assert_eq!(o.bodies, e.bodies, "{name} extension bodies");
            assert!(o.grease_differs, "{name} GREASE extension bodies");
        }
        println!("{name} {}", hex_bytes(&ours_raw[0]));
    }
}

#[test]
fn utls_websocket_alpn() {
    let expected: Vec<_> = fixture("chrome_ws").iter().map(|x| parse(x)).collect();
    let ours: Vec<_> = (0..40).map(|_| parse(&build(Fingerprint::Chrome, "www.example.com", &[b"http/1.1".to_vec()]).msg)).collect();
    let alpn = "000908687474702f312e31";
    for h in expected.iter().chain(&ours) {
        assert_eq!(h.bodies["0010"], alpn);
    }
    assert_eq!(ours[0].suites, expected[0].suites);
    assert_eq!(ours[0].bodies, expected[0].bodies);
}

#[test]
fn xray_fingerprint_names() {
    use Fingerprint::*;
    for (n, f) in [
        ("chrome", Chrome),
        ("", Chrome),
        ("firefox", Firefox),
        ("safari", Safari),
        ("ios", Ios),
        ("edge", Edge),
        ("qq", Qq),
        ("HelloChrome_133", Chrome),
        ("HelloIOS_14", Ios),
        ("HelloChrome_Auto", Chrome),
        ("HelloFirefox_148", Firefox),
        ("HelloIOS_Auto", Ios),
        ("hellosafari_26_3", Safari),
        ("helloqq_11_1", Qq),
    ] {
        assert_eq!(Fingerprint::from_xray(n), Some(f), "{n}");
    }
    assert!(Fingerprint::from_xray("random").is_some());
    assert_eq!(Fingerprint::from_xray("random"), Fingerprint::from_xray("random"));
    assert_eq!(Fingerprint::from_xray("nope"), None);
}

#[test]
fn unsupported_versioned_fingerprints_are_not_silently_upgraded() {
    for n in ["HelloChrome_119", "HelloFirefox_54", "HelloIOS_11", "golang", "android_12", "randomizedh3"] {
        assert_eq!(Fingerprint::from_xray(n), None, "{n}");
    }
    assert_eq!(Fingerprint::from_xray("HelloChrome_133"), Some(Fingerprint::Chrome));
}

#[test]
fn modern_xray_templates_match_utls() {
    for (name, alias, shuffled) in [
        ("chrome120", "HelloChrome_120", true),
        ("chrome131", "HelloChrome_131", true),
        ("firefox120", "HelloFirefox_120", false),
        ("ios13", "HelloIOS_13", false),
        ("edge106", "HelloEdge_106", false),
        ("360_11", "Hello360_11_0", false),
    ] {
        let fp = Fingerprint::from_xray(alias).expect(alias);
        let expected_raw = fixture(name);
        let ours_raw = our(fp);
        let expected: Vec<_> = expected_raw.iter().map(|b| parse(b)).collect();
        let ours: Vec<_> = ours_raw.iter().map(|b| parse(b)).collect();
        for (raw, h) in ours_raw.iter().zip(&ours) {
            let idx = expected.iter().position(|e| e.ech.2 == h.ech.2).unwrap();
            let e = &expected[idx];
            assert_eq!(raw.len(), expected_raw[idx].len(), "{name} length");
            assert_eq!(h.legacy, e.legacy, "{name}");
            assert_eq!(h.compression, e.compression, "{name}");
            assert_eq!(h.session_len, e.session_len, "{name}");
            assert_eq!(h.suites, e.suites, "{name}");
            assert_eq!(h.bodies, e.bodies, "{name}");
            let mut order = h.order.clone();
            let mut expected_order = e.order.clone();
            order.sort();
            expected_order.sort();
            assert_eq!(order, expected_order, "{name} extension multiset");
            assert!(h.grease_differs, "{name} GREASE bodies");
            if !shuffled {
                assert_eq!(h.order, e.order, "{name}");
            }
        }
        if shuffled {
            assert!(ours.iter().map(|h| &h.order).collect::<BTreeSet<_>>().len() > 1);
            assert!(ours.iter().all(|h| h.order.first().unwrap() == "GREASE"));
            for h in &ours {
                let last = h.order.len() - 1;
                let grease = if h.order[last] == "0015" { last - 1 } else { last };
                assert_eq!(h.order[grease], "GREASE", "{name} fixed suffix");
                assert!([144, 176, 208, 240].contains(&h.ech.2));
                assert!(expected.iter().any(|e| (e.ech.0, e.ech.1) == (h.ech.0, h.ech.1)));
            }
        }
    }
}

#[test]
fn random_uses_xray_modern_pool() {
    let expected: BTreeSet<_> = [
        "HelloFirefox_120",
        "HelloFirefox_148",
        "HelloChrome_120",
        "HelloChrome_131",
        "HelloChrome_133",
        "HelloIOS_13",
        "HelloIOS_14",
        "HelloEdge_106",
        "HelloSafari_26_3",
        "Hello360_11_0",
        "HelloQQ_11_1",
    ]
    .into_iter()
    .map(|s| format!("{:?}", Fingerprint::from_xray(s).expect(s)))
    .collect();
    let actual: BTreeSet<_> = Fingerprint::ALL.iter().map(|f| format!("{f:?}")).collect();
    assert_eq!(actual, expected);
}

#[test]
fn cookie_only_hrr_retains_the_original_key_share() {
    let mut first = build(Fingerprint::Chrome, "test.example", &[]);
    let parsed = parse(&first.msg);
    let client_pk = PublicKey::from(<[u8; 32]>::try_from(parsed.shares[&GROUP_X25519].as_slice()).unwrap());
    let server = StaticSecret::from([41; 32]);
    let expected = server.diffie_hellman(&client_pk);
    let mut retry = rebuild_after_hrr(&mut first, Fingerprint::Chrome, 0, Some(b"test-cookie")).unwrap();
    assert_eq!(parse(&retry.msg).shares, parsed.shares);
    let shared = retry.keys.shared_secret(GROUP_X25519, PublicKey::from(&server).as_bytes()).unwrap();
    assert_eq!(shared, expected.as_bytes());
}

#[test]
fn all_additional_templates_match_utls() {
    for name in [
        "android",
        "hello360_7_5",
        "helloios_11_1",
        "helloios_12_1",
        "hellosafari_16_0",
        "hellofirefox_55",
        "hellofirefox_56",
        "hellofirefox_63",
        "hellofirefox_65",
        "hellofirefox_99",
        "hellofirefox_102",
        "hellofirefox_105",
        "hellochrome_58",
        "hellochrome_62",
        "hellochrome_70",
        "hellochrome_72",
        "hellochrome_83",
        "hellochrome_87",
        "hellochrome_96",
        "hellochrome_100",
        "hellochrome_102",
        "hellochrome_106_shuffle",
        "hellochrome_115_pq",
        "hellochrome_120_pq",
        "unsafe",
        "hellogolang",
    ] {
        let fp = Fingerprint::from_xray(name).unwrap();
        let shuffled = matches!(fp, Fingerprint::Chrome106 | Fingerprint::Chrome115Pq | Fingerprint::Chrome120Pq);
        let expected = fixture(name);
        for raw in &expected {
            let configured = if matches!(fp, Fingerprint::Golang | Fingerprint::UtlsGolang) {
                vec![b"h2".to_vec(), b"http/1.1".to_vec()]
            } else {
                vec![]
            };
            let ours = build(fp, "www.example.com", &configured).msg;
            let (o, e) = (parse(&ours), parse(raw));
            assert_eq!(o.legacy, e.legacy, "{name}: version");
            assert_eq!(o.compression, e.compression, "{name}: compression");
            assert_eq!(o.session_len, e.session_len, "{name}: session ID");
            assert_eq!(o.suites, e.suites, "{name}: suites");
            assert_eq!(o.bodies, e.bodies, "{name}: extension bodies");
            if !shuffled {
                assert_eq!(o.order, e.order, "{name}: order");
            }
            if fp != Fingerprint::Chrome120Pq {
                assert_eq!(ours.len(), raw.len(), "{name}: length/padding");
            }
        }
    }
}

#[test]
fn randomized_generator_matches_utls_for_public_seeds() {
    for name in ["randomized", "randomizednoalpn", "hellorandomized", "hellorandomizedalpn", "hellorandomizednoalpn"] {
        let fp = Fingerprint::from_xray(name).unwrap();
        for seed in 0..32 {
            let t = randomized::seeded(fp, "www.example.com", &[], &[seed; 32]);
            let ours = build_template(t, fp, &[], false).unwrap().msg;
            let expected = fixture(&format!("{name}_seed{seed}"));
            let (o, e) = (parse(&ours), parse(&expected[0]));
            assert!(firefox_normalized_equal(&o, &e), "{name} seed {seed}\nours: {o:?}\nuTLS: {e:?}");
            assert_eq!(ours.len(), expected[0].len(), "{name} seed {seed}: padding");
        }
    }
}

#[test]
fn randomized_preset_is_fixed_for_process_but_hello_randomized_is_fresh() {
    for fp in [Fingerprint::Randomized, Fingerprint::RandomizedNoAlpn] {
        let a = parse(&super::build(fp, "www.example.com", &[], false).unwrap().msg);
        for _ in 0..8 {
            let b = parse(&super::build(fp, "www.example.com", &[], false).unwrap().msg);
            assert!(firefox_normalized_equal(&a, &b));
            assert!(a.suites.iter().any(|s| s.starts_with("130")));
        }
    }
    let set: BTreeSet<_> =
        (0..8).map(|_| parse(&super::build(Fingerprint::HelloRandomized, "www.example.com", &[], false).unwrap().msg).suites).collect();
    assert!(set.len() > 1);
}

#[test]
fn xray_full_name_table_is_accepted() {
    for name in [
        "chrome",
        "firefox",
        "safari",
        "ios",
        "android",
        "edge",
        "360",
        "qq",
        "random",
        "randomized",
        "randomizednoalpn",
        "unsafe",
        "hellogolang",
        "hellorandomized",
        "hellorandomizedalpn",
        "hellorandomizednoalpn",
        "hellofirefox_auto",
        "hellofirefox_55",
        "hellofirefox_56",
        "hellofirefox_63",
        "hellofirefox_65",
        "hellofirefox_99",
        "hellofirefox_102",
        "hellofirefox_105",
        "hellofirefox_120",
        "hellofirefox_148",
        "hellochrome_auto",
        "hellochrome_58",
        "hellochrome_62",
        "hellochrome_70",
        "hellochrome_72",
        "hellochrome_83",
        "hellochrome_87",
        "hellochrome_96",
        "hellochrome_100",
        "hellochrome_102",
        "hellochrome_106_shuffle",
        "hellochrome_120",
        "hellochrome_131",
        "hellochrome_133",
        "helloios_auto",
        "helloios_11_1",
        "helloios_12_1",
        "helloios_13",
        "helloios_14",
        "helloandroid_11_okhttp",
        "helloedge_auto",
        "helloedge_85",
        "helloedge_106",
        "hellosafari_auto",
        "hellosafari_16_0",
        "hellosafari_26_3",
        "hello360_auto",
        "hello360_7_5",
        "hello360_11_0",
        "helloqq_auto",
        "helloqq_11_1",
        "hellochrome_100_psk",
        "hellochrome_112_psk_shuf",
        "hellochrome_114_padding_psk_shuf",
        "hellochrome_115_pq",
        "hellochrome_115_pq_psk",
        "hellochrome_120_pq",
    ] {
        assert!(Fingerprint::from_xray(name).is_some(), "{name}");
    }
    assert_eq!(Fingerprint::from_xray("golang"), None);
    for name in ["hellochrome_100_psk", "hellochrome_112_psk_shuf", "hellochrome_114_padding_psk_shuf", "hellochrome_115_pq_psk"] {
        assert!(fixture(name).is_empty(), "uTLS emits no ClientHello without a session: {name}");
        assert!(Fingerprint::from_xray(name).unwrap().empty_psk());
    }
}

#[test]
fn go_nist_hybrid_hrr_agrees_with_an_independent_peer() {
    for group in [GROUP_P256MLKEM768, GROUP_P384MLKEM1024] {
        let mut first = build(Fingerprint::Golang, "www.example.com", &[]);
        let mut retry = rebuild_after_hrr(&mut first, Fingerprint::Golang, group, None).unwrap();
        let parsed = parse(&retry.msg);
        let share = &parsed.shares[&group];
        let (ec_len, alg) = if group == GROUP_P256MLKEM768 { (65, &agreement::ECDH_P256) } else { (97, &agreement::ECDH_P384) };
        let peer = agreement::EphemeralPrivateKey::generate(alg, &SystemRandom::new()).unwrap();
        let public = peer.compute_public_key().unwrap().as_ref().to_vec();
        let public_peer = agreement::UnparsedPublicKey::new(alg, &share[..ec_len]);
        let mut expected = agreement::agree_ephemeral(peer, &public_peer, |s| s.to_vec()).unwrap();
        let (ct, kem_ss) = if group == GROUP_P256MLKEM768 {
            let ek = ml_kem::EncapsulationKey768::new(&share[ec_len..].try_into().unwrap()).unwrap();
            let (ct, ss) = ek.encapsulate_deterministic(&[7; 32].into());
            (ct.to_vec(), ss)
        } else {
            let ek = ml_kem::EncapsulationKey1024::new(&share[ec_len..].try_into().unwrap()).unwrap();
            let (ct, ss) = ek.encapsulate_deterministic(&[7; 32].into());
            (ct.to_vec(), ss)
        };
        expected.extend_from_slice(&kem_ss);
        assert_eq!(retry.keys.shared_secret(group, &[public, ct].concat()).unwrap(), expected);
    }
}

#[test]
fn kyber_draft_share_and_secret_use_x25519_first_and_round3_kdf() {
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    use sha3::{Digest, Sha3_256, Shake256};
    let mut hello = build(Fingerprint::Chrome115Pq, "www.example.com", &[]);
    let parsed = parse(&hello.msg);
    let share = &parsed.shares[&GROUP_X25519KYBER768];
    assert_eq!(share.len(), 1216);
    let peer = StaticSecret::random();
    let public = PublicKey::from(&peer);
    let classical: [u8; 32] = share[..32].try_into().unwrap();
    let expected_x = peer.diffie_hellman(&PublicKey::from(classical));
    let ek = ml_kem::EncapsulationKey768::new(&share[32..].try_into().unwrap()).unwrap();
    let (ct, k) = ek.encapsulate_deterministic(&[8; 32].into());
    let mut shake = Shake256::default();
    Update::update(&mut shake, &k);
    Update::update(&mut shake, &Sha3_256::digest(ct));
    let mut round3 = [0; 32];
    shake.finalize_xof().read(&mut round3);
    let response = [public.as_bytes().as_slice(), ct.as_slice()].concat();
    let expected = [expected_x.as_bytes().as_slice(), round3.as_slice()].concat();
    assert_eq!(hello.keys.shared_secret(GROUP_X25519KYBER768, &response).unwrap(), expected);
}

#[test]
fn randomized_mlkem_hrr_rejects_the_same_path_as_utls() {
    let mut checked = 0;
    for seed in 0..32 {
        let t = randomized::seeded(Fingerprint::Randomized, "www.example.com", &[], &[seed; 32]);
        let mut first = build_template(t, Fingerprint::Randomized, &[], false).unwrap();
        if !first.supported_groups.contains(&GROUP_X25519MLKEM768) || first.keys.groups.contains(&GROUP_X25519MLKEM768) {
            continue;
        }
        assert_eq!(rebuild_after_hrr(&mut first, Fingerprint::Randomized, GROUP_X25519MLKEM768, None).err(), Some(UTLS_HRR_GROUP_ERROR));
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
#[ignore]
fn keyshare_cost() {
    let n = 2000;
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        let mut keys = KeyShareKeys::empty();
        let share = gen_hybrid(&mut keys);
        let (_x, _xp) = gen_x25519();
        let ek = keys.mlkem.as_ref().unwrap().encapsulation_key().clone();
        let (ct, _) = ek.encapsulate_deterministic(&[7u8; 32].into());
        let _ = keys.mlkem.as_ref().unwrap().decapsulate(&ct);
        std::hint::black_box(share);
    }
    println!("на ClientHello: {:.1} мкс", t0.elapsed().as_secs_f64() * 1e6 / n as f64);
    let us = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1e6 / n as f64;
    let t = std::time::Instant::now();
    for _ in 0..n {
        std::hint::black_box(gen_x25519());
    }
    println!("  x25519 ключ: {:.1} мкс", us(t));
    let t = std::time::Instant::now();
    let mut last = None;
    for _ in 0..n {
        last = Some(DecapsulationKey768::from_seed([3u8; 64].into()));
    }
    println!("  ML-KEM from_seed: {:.1} мкс", us(t));
    let dk = last.unwrap();
    let t = std::time::Instant::now();
    for _ in 0..n {
        std::hint::black_box(dk.encapsulation_key().to_bytes());
    }
    println!("  ML-KEM ek.to_bytes: {:.1} мкс", us(t));
    let (ct, _) = dk.encapsulation_key().encapsulate_deterministic(&[7u8; 32].into());
    let t = std::time::Instant::now();
    for _ in 0..n {
        std::hint::black_box(dk.decapsulate(&ct));
    }
    println!("  ML-KEM decapsulate: {:.1} мкс", us(t));
}

fn ext_lengths(raw: &[u8]) -> (usize, usize) {
    let b = &raw[4..];
    let mut i = 2 + 32;
    i += 1 + b[i] as usize;
    let suites = u16at(b, &mut i) as usize;
    i += suites;
    i += 1 + b[i] as usize;
    let declared = u16at(b, &mut i) as usize;
    (declared, b.len() - i)
}

fn alpn_names(raw: &[u8]) -> Option<Vec<Vec<u8>>> {
    let b = &raw[4..];
    let mut i = 2 + 32;
    i += 1 + b[i] as usize;
    let suites = u16at(b, &mut i) as usize;
    i += suites;
    i += 1 + b[i] as usize;
    let end = i + 2 + u16at(b, &mut i) as usize;
    while i < end {
        let t = u16at(b, &mut i);
        let body = vec16(b, &mut i);
        if t == EXT_ALPN {
            let mut j = 0;
            let list = vec16(body, &mut j);
            let mut names = Vec::new();
            let mut k = 0;
            while k < list.len() {
                let n = list[k] as usize;
                names.push(list.get(k + 1..k + 1 + n)?.to_vec());
                k += 1 + n;
            }
            return (k == list.len() && j == body.len()).then_some(names);
        }
    }
    None
}

#[test]
fn alpn_names_outside_1_to_255_bytes_are_refused_not_wrapped() {
    for bad in [vec![b'a'; 300], vec![]] {
        let r = super::build(Fingerprint::Golang, "www.example.com", &[bad], true);
        assert_eq!(r.err(), Some(INVALID_ALPN));
    }
    let max = vec![b'a'; 255];
    let h = build(Fingerprint::Golang, "www.example.com", std::slice::from_ref(&max));
    assert_eq!(alpn_names(&h.msg), Some(vec![max]));
}

#[test]
fn server_name_past_the_extension_block_is_refused_not_wrapped() {
    assert!(super::build(Fingerprint::Golang, &"a".repeat(70_000), &[], true).is_err());
    let h = build(Fingerprint::Golang, &"a".repeat(60_000), &[]);
    let (declared, actual) = ext_lengths(&h.msg);
    assert_eq!(declared, actual);
}

#[test]
fn hrr_cookie_past_the_extension_block_is_refused_not_wrapped() {
    let mut first = build(Fingerprint::Chrome, "test.example", &[]);
    assert!(rebuild_after_hrr(&mut first, Fingerprint::Chrome, 0, Some(&vec![7u8; 65_000])).is_err());
    let mut first = build(Fingerprint::Chrome, "test.example", &[]);
    let retry = rebuild_after_hrr(&mut first, Fingerprint::Chrome, 0, Some(&vec![7u8; 60_000])).unwrap();
    let (declared, actual) = ext_lengths(&retry.msg);
    assert_eq!(declared, actual);
}
