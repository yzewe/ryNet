mod tls12;

use std::future::poll_fn;
use std::time::{SystemTime, UNIX_EPOCH};

use ring::{aead, digest, hkdf, hmac, signature};
use rustls_pki_types::{CertificateDer, ServerName, TrustAnchor, UnixTime};

use crate::der;
use crate::hello::{self, ClientHello};
use crate::keysched::{self, Direction, Secret};
use crate::record::{CT_ALERT, CT_CCS, CT_HANDSHAKE, Conn, MAX_PT};
use crate::{ClientConfig, Error, Pins, RealityConfig, Verify};

const HT_SERVER_HELLO: u8 = 2;
const HT_ENCRYPTED_EXTENSIONS: u8 = 8;
const HT_CERTIFICATE: u8 = 11;
const HT_CERTIFICATE_REQUEST: u8 = 13;
const HT_CERTIFICATE_VERIFY: u8 = 15;
const HT_FINISHED: u8 = 20;
const HT_COMPRESSED_CERTIFICATE: u8 = 25;

const HRR_RANDOM: [u8; 32] = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91, 0xc2, 0xa2, 0x11, 0x16, 0x7a, 0xbb,
    0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2, 0xc8, 0xa8, 0x33, 0x9c,
];

const REALITY_CLIENT_VERSION: [u8; 3] = [26, 7, 28];

const MAX_CERTS: usize = 10;
const MAX_CERT_LIST: usize = 1 << 17;

pub(crate) struct Outcome {
    pub alpn: Option<Vec<u8>>,
    pub reality_authenticated: bool,
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.0.len() < n {
            return Err(Error::Protocol("truncated handshake message"));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u24(&mut self) -> Result<usize, Error> {
        let b = self.take(3)?;
        Ok(u32::from_be_bytes([0, b[0], b[1], b[2]]) as usize)
    }
    fn vec8(&mut self) -> Result<&'a [u8], Error> {
        let n = self.u8()? as usize;
        self.take(n)
    }
    fn vec16(&mut self) -> Result<&'a [u8], Error> {
        let n = self.u16()? as usize;
        self.take(n)
    }
    fn vec24(&mut self) -> Result<&'a [u8], Error> {
        let n = self.u24()?;
        self.take(n)
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn extensions(&mut self) -> Result<Vec<(u16, &'a [u8])>, Error> {
        let mut r = Reader(self.vec16()?);
        let mut out = Vec::new();
        let mut seen = [0u64; 1024];
        while !r.is_empty() {
            let t = r.u16()?;
            let b = r.vec16()?;
            let (word, bit) = (usize::from(t >> 6), 1u64 << (t & 63));
            if seen[word] & bit != 0 {
                return Err(Error::Protocol("duplicate extension"));
            }
            seen[word] |= bit;
            out.push((t, b));
        }
        Ok(out)
    }
}

async fn next_msg<S>(conn: &mut Conn<S>, buf: &mut Vec<u8>) -> Result<Vec<u8>, Error>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        if buf.len() >= 4 {
            let len = u32::from_be_bytes([0, buf[1], buf[2], buf[3]]) as usize;
            if len > 1 << 17 {
                return Err(Error::Protocol("handshake message too large"));
            }
            if buf.len() >= 4 + len {
                return Ok(buf.drain(..4 + len).collect());
            }
        }
        let rec = poll_fn(|cx| conn.poll_record(cx)).await?.ok_or(Error::Protocol("connection closed during handshake"))?;
        match rec.ctype {
            CT_HANDSHAKE => buf.extend_from_slice(conn.slice(rec.start, rec.end)),
            CT_ALERT => {
                let a = conn.slice(rec.start, rec.end);
                return Err(Error::Alert(a.get(1).copied().unwrap_or(0)));
            }
            _ => return Err(Error::Protocol("unexpected record during handshake")),
        }
    }
}

fn expect(msg: &[u8], typ: u8, what: &'static str) -> Result<(), Error> {
    if msg[0] == typ { Ok(()) } else { Err(Error::Protocol(what)) }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

struct Len32;
impl hkdf::KeyType for Len32 {
    fn len(&self) -> usize {
        32
    }
}

fn reality_seal(hello: &mut ClientHello, rc: &RealityConfig) -> Result<[u8; 32], Error> {
    let shared = hello.keys.reality_ecdh(&rc.public_key).ok_or(Error::Unsupported("fingerprint without X25519 key share"))?;
    let random: [u8; 32] = hello.msg[6..38].try_into().unwrap();
    let mut auth_key = [0u8; 32];
    hkdf::Salt::new(hkdf::HKDF_SHA256, &random[..20])
        .extract(&shared)
        .expand(&[b"REALITY"], Len32)
        .and_then(|okm| okm.fill(&mut auth_key))
        .map_err(|_| Error::Protocol("hkdf"))?;

    let mut sid = Vec::with_capacity(32);
    sid.extend_from_slice(&REALITY_CLIENT_VERSION);
    sid.push(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as u32;
    sid.extend_from_slice(&now.to_be_bytes());
    sid.extend_from_slice(&rc.short_id);

    let off = hello.session_id_offset;
    hello.msg[off..off + 32].fill(0);
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &auth_key).unwrap());
    let nonce = aead::Nonce::try_assume_unique_for_key(&random[20..]).unwrap();
    key.seal_in_place_append_tag(nonce, aead::Aad::from(&hello.msg[..]), &mut sid).map_err(|_| Error::Protocol("reality seal"))?;
    hello.msg[off..off + 32].copy_from_slice(&sid);
    Ok(auth_key)
}

fn reality_verify_cert(
    leaf: &[u8],
    auth_key: &[u8; 32],
    verify: &[u8],
    client_hello: &[u8],
    server_hello: &[u8],
) -> Result<[u8; 32], Error> {
    let cert = der::parse(leaf).ok_or(Error::RealityAuth)?;
    if cert.key_alg != [0x2b, 0x65, 0x70] {
        return Err(Error::RealityAuth);
    }
    let pubkey: [u8; 32] = cert.key.try_into().map_err(|_| Error::RealityAuth)?;
    let mac = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA512, auth_key), &pubkey);
    if !ct_eq(mac.as_ref(), cert.sig) {
        return Err(Error::RealityAuth);
    }
    if !verify.is_empty() {
        use ml_dsa::{KeyInit, MlDsa65, Signature, Verifier, VerifyingKey};
        let bad = Error::RealityMldsa65;
        let key = VerifyingKey::<MlDsa65>::new_from_slice(verify).map_err(|_| bad("invalid public key"))?;
        let sig = cert.first_extension().ok_or_else(|| bad("missing certificate extension"))?;
        let sig = Signature::<MlDsa65>::try_from(sig).map_err(|_| bad("invalid certificate signature"))?;
        let mut h = hmac::Context::with_key(&hmac::Key::new(hmac::HMAC_SHA512, auth_key));
        h.update(&pubkey);
        h.update(client_hello);
        h.update(server_hello);
        key.verify(h.sign().as_ref(), &sig).map_err(|_| bad("signature mismatch (wrong mldsa65Verify key or modified handshake)"))?;
    }
    Ok(pubkey)
}

fn decompress_cert(alg: u16, data: &[u8], expected: usize) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    if expected > MAX_CERT_LIST {
        return Err(Error::Protocol("compressed certificate too large"));
    }
    let bad = |_| Error::Protocol("bad compressed certificate");
    let out = match alg {
        hello::CERT_COMPRESSION_ZLIB => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, expected)
            .map_err(|_| Error::Protocol("bad compressed certificate"))?,
        hello::CERT_COMPRESSION_BROTLI => {
            let mut out = Vec::new();
            brotli_decompressor::Decompressor::new(data, 4096).take(expected as u64 + 1).read_to_end(&mut out).map_err(bad)?;
            out
        }
        hello::CERT_COMPRESSION_ZSTD => {
            let mut out = Vec::new();
            ruzstd::decoding::StreamingDecoder::new_with_max_window_size(data, 1 << 20)
                .map_err(|_| Error::Protocol("bad compressed certificate"))?
                .take(expected as u64 + 1)
                .read_to_end(&mut out)
                .map_err(bad)?;
            out
        }
        _ => {
            return Err(Error::Protocol("unexpected certificate compression algorithm"));
        }
    };
    if out.len() != expected {
        return Err(Error::Protocol("compressed certificate length mismatch"));
    }
    Ok(out)
}

fn parse_certificate_body(body: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
    let mut r = Reader(body);
    let _ctx = r.vec8()?;
    let mut list = Reader(r.vec24()?);
    let mut certs = Vec::new();
    while !list.is_empty() {
        push_cert(&mut certs, list.vec24()?)?;
        let _exts = list.vec16()?;
    }
    if certs.is_empty() {
        return Err(Error::BadCertificate("empty certificate list".into()));
    }
    Ok(certs)
}

fn record_size_limit(body: &[u8], tls13: bool) -> Result<usize, Error> {
    let mut r = Reader(body);
    let limit = usize::from(r.u16()?);
    if !r.is_empty() || limit < 64 {
        return Err(Error::Protocol("bad record_size_limit"));
    }
    Ok((limit - usize::from(tls13)).min(MAX_PT))
}

fn push_cert(certs: &mut Vec<Vec<u8>>, der: &[u8]) -> Result<(), Error> {
    if der.is_empty() {
        return Err(Error::Protocol("empty certificate"));
    }
    if certs.len() == MAX_CERTS {
        return Err(Error::BadCertificate(format!("more than {MAX_CERTS} certificates")));
    }
    certs.push(der.to_vec());
    Ok(())
}

fn webpki_alg(scheme: u16) -> Option<&'static dyn rustls_pki_types::SignatureVerificationAlgorithm> {
    use webpki::ring as a;
    Some(match scheme {
        0x0403 => a::ECDSA_P256_SHA256,
        0x0503 => a::ECDSA_P384_SHA384,
        0x0603 => crate::p521::ECDSA_P521_SHA512,
        0x0804 => a::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
        0x0805 => a::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
        0x0806 => a::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
        0x0807 => a::ED25519,
        _ => return None,
    })
}

fn cv_content(th: &[u8]) -> Vec<u8> {
    let mut m = vec![0x20u8; 64];
    m.extend_from_slice(b"TLS 1.3, server CertificateVerify\0");
    m.extend_from_slice(th);
    m
}

enum Peer {
    WebPki(Vec<Vec<u8>>),
    Reality([u8; 32]),
    Pinned(Vec<Vec<u8>>),
    Insecure(Vec<Vec<u8>>),
}

fn verify_chain(certs: &[Vec<u8>], server_name: &str, roots: &[TrustAnchor<'_>]) -> Result<(), Error> {
    let bad = |e: webpki::Error| Error::BadCertificate(format!("{e:?}"));
    let ders: Vec<CertificateDer<'_>> = certs.iter().map(|c| CertificateDer::from(c.as_slice())).collect();
    let (leaf, intermediates) = ders.split_first().ok_or(Error::Protocol("empty Certificate"))?;
    let ee = webpki::EndEntityCert::try_from(leaf).map_err(bad)?;
    ee.verify_for_usage(
        crate::p521::ALL_VERIFICATION_ALGS,
        roots,
        intermediates,
        UnixTime::now(),
        webpki::KeyUsage::server_auth(),
        None,
        None,
    )
    .map_err(bad)?;
    let name = ServerName::try_from(server_name).map_err(|_| Error::BadCertificate("bad server name".into()))?;
    ee.verify_is_valid_for_subject_name(&name).map_err(bad)
}

pub fn verify_pinned(certs: &[Vec<u8>], pins: &Pins, server_name: &str) -> Result<(), Error> {
    let leaf = certs.first().ok_or(Error::Protocol("empty Certificate"))?;
    let hash = |c: &[u8]| -> [u8; 32] { digest::digest(&digest::SHA256, c).as_ref().try_into().unwrap() };
    let mut ca: Option<&[u8]> = None;
    if !pins.sha256.is_empty() {
        if pins.sha256.contains(&hash(leaf)) {
            return Ok(());
        }
        ca = certs[1..].iter().find(|c| pins.sha256.contains(&hash(c)) && is_ca(c)).map(Vec::as_slice);
        if ca.is_none() {
            return Err(Error::BadCertificate("peer cert is unrecognized (against pinnedPeerCertSha256)".into()));
        }
    }
    let own;
    let system;
    let roots: &[TrustAnchor<'_>] = match ca {
        Some(c) => {
            let der = CertificateDer::from(c);
            own = [webpki::anchor_from_trusted_cert(&der).map_err(|e| Error::BadCertificate(format!("pinned CA: {e:?}")))?.to_owned()];
            &own
        }
        None => {
            system = crate::roots();
            &system
        }
    };
    let names: Vec<&str> = if !pins.names.is_empty() {
        pins.names.iter().map(String::as_str).collect()
    } else if server_name.is_empty() {
        return Err(Error::BadCertificate("pinning a CA needs serverName".into()));
    } else {
        vec![server_name]
    };
    let mut last = None;
    for n in names {
        match verify_chain(certs, n, roots) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| Error::BadCertificate("no names to verify".into())))
}

fn is_ca(der: &[u8]) -> bool {
    der::parse(der).is_some_and(|c| c.is_ca())
}

fn verify_cv(peer: &Peer, scheme: u16, sig: &[u8], th: &[u8]) -> Result<(), Error> {
    let content = cv_content(th);
    match peer {
        Peer::Reality(pubkey) => {
            if scheme != 0x0807 {
                return Err(Error::RealityAuth);
            }
            signature::UnparsedPublicKey::new(&signature::ED25519, pubkey).verify(&content, sig).map_err(|_| Error::RealityAuth)
        }
        Peer::WebPki(certs) | Peer::Pinned(certs) | Peer::Insecure(certs) => {
            let alg = webpki_alg(scheme).ok_or(Error::Unsupported("CertificateVerify signature scheme"))?;
            let der = CertificateDer::from(certs[0].as_slice());
            let ee = match webpki::EndEntityCert::try_from(&der) {
                Ok(ee) => ee,
                Err(_) if matches!(peer, Peer::Pinned(_)) => return verify_raw(&certs[0], scheme, &content, sig),
                Err(_) if matches!(peer, Peer::Insecure(_)) => return verify_raw(&certs[0], scheme, &content, sig),
                Err(e) => return Err(Error::BadCertificate(format!("{e:?}"))),
            };
            ee.verify_signature(alg, &content, sig).map_err(|e| Error::BadCertificate(format!("CertificateVerify: {e:?}")))
        }
    }
}

fn verify_raw(cert: &[u8], scheme: u16, content: &[u8], sig: &[u8]) -> Result<(), Error> {
    let x = der::parse(cert).ok_or_else(|| Error::BadCertificate("pinned cert: bad DER".into()))?;
    let key = x.key;
    if scheme == 0x0603 {
        return crate::p521::verify(key, &ring::digest::SHA512, content, sig)
            .then_some(())
            .ok_or_else(|| Error::BadCertificate("CertificateVerify: bad P-521 signature".into()));
    }
    let alg: &'static dyn signature::VerificationAlgorithm = match scheme {
        0x0403 => &signature::ECDSA_P256_SHA256_ASN1,
        0x0503 => &signature::ECDSA_P384_SHA384_ASN1,
        0x0804 => &signature::RSA_PSS_2048_8192_SHA256,
        0x0805 => &signature::RSA_PSS_2048_8192_SHA384,
        0x0806 => &signature::RSA_PSS_2048_8192_SHA512,
        0x0807 => &signature::ED25519,
        _ => return Err(Error::Unsupported("CertificateVerify signature scheme")),
    };
    signature::UnparsedPublicKey::new(alg, key)
        .verify(content, sig)
        .map_err(|_| Error::BadCertificate("CertificateVerify: bad signature".into()))
}

pub(crate) async fn run<S>(conn: &mut Conn<S>, cfg: &ClientConfig, roots: &[TrustAnchor<'_>]) -> Result<Outcome, Error>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    if cfg.fingerprint.empty_psk() {
        return Err(Error::EmptyPsk);
    }
    if matches!(cfg.verify, Verify::Reality(_)) && matches!(cfg.fingerprint, crate::Fingerprint::Golang | crate::Fingerprint::UtlsGolang) {
        return Err(Error::Unsupported("invalid REALITY fingerprint: Go TLS"));
    }
    let mut hello = hello::build(cfg.fingerprint, &cfg.server_name, &cfg.alpn, cfg.alpn_override).map_err(Error::Config)?;
    let auth_key = match &cfg.verify {
        Verify::Reality(rc) => Some(reality_seal(&mut hello, rc)?),
        _ => None,
    };
    let session_id: [u8; 32] = hello.msg[39..71].try_into().unwrap();

    conn.queue(CT_HANDSHAKE, 0x0301, &hello.msg);
    conn.flush_out().await?;
    conn.hello_sent = true;

    let mut hs_buf = Vec::new();
    let mut sent_ccs = false;
    let mut sh = next_msg(conn, &mut hs_buf).await?;
    let mut transcript: Option<digest::Context> = None;
    let mut hrr_suite: Option<u16> = None;

    let (suite, group, server_share) = loop {
        expect(&sh, HT_SERVER_HELLO, "expected ServerHello")?;
        let mut r = Reader(&sh[4..]);
        let _legacy_version = r.u16()?;
        let random = r.take(32)?;
        let sid_echo = r.vec8()?;
        let suite_id = r.u16()?;
        let _compression = r.u8()?;
        let exts = if r.is_empty() { Vec::new() } else { r.extensions()? };
        let version = exts.iter().find(|e| e.0 == 43).map(|e| Reader(e.1).u16()).transpose()?;
        if version != Some(0x0304) {
            if _legacy_version == 0x0303 && matches!(version, None | Some(0x0303)) && hrr_suite.is_none() && random != HRR_RANDOM {
                if matches!(cfg.verify, Verify::Reality(_)) {
                    return Err(Error::Unsupported("REALITY requires TLS 1.3"));
                }
                return tls12::run(conn, cfg, roots, &hello, &sh, &mut hs_buf).await;
            }
            return Err(Error::Unsupported("server did not negotiate TLS 1.3"));
        }
        if !ct_eq(sid_echo, &session_id) {
            return Err(Error::Protocol("legacy_session_id_echo mismatch"));
        }
        if !hello.tls13_suites.contains(&suite_id) || hrr_suite.is_some_and(|s| s != suite_id) {
            return Err(Error::Protocol("server selected a cipher suite we did not offer"));
        }
        let suite = keysched::suite_by_id(suite_id).ok_or(Error::Unsupported("cipher suite"))?;
        let key_share = exts.iter().find(|e| e.0 == 51).map(|e| e.1);

        if random == HRR_RANDOM {
            if hrr_suite.is_some() {
                return Err(Error::Protocol("second HelloRetryRequest"));
            }
            hrr_suite = Some(suite_id);
            let selected = key_share.map(|b| Reader(b).u16()).transpose()?.unwrap_or(0);
            let cookie = exts.iter().find(|e| e.0 == 44).map(|e| Reader(e.1).vec16()).transpose()?;
            conn.queue(CT_CCS, 0x0303, &[1]);
            sent_ccs = true;
            let h = suite.hash;
            let ch_hash = h.hash(&hello.msg);
            let mut ctx = digest::Context::new(h.digest());
            ctx.update(&[254, 0, 0, h.len() as u8]);
            ctx.update(ch_hash.as_slice());
            ctx.update(&sh);
            let hello2 = hello::rebuild_after_hrr(&mut hello, cfg.fingerprint, selected, cookie)
                .map_err(|m| if m == hello::UTLS_HRR_GROUP_ERROR { Error::UtlsHrrGroup } else { Error::Protocol(m) })?;
            hello = hello2;
            ctx.update(&hello.msg);
            transcript = Some(ctx);
            conn.queue(CT_HANDSHAKE, 0x0303, &hello.msg);
            conn.flush_out().await?;
            sh = next_msg(conn, &mut hs_buf).await?;
            continue;
        }

        let ks = key_share.ok_or(Error::Protocol("ServerHello without key_share"))?;
        let mut kr = Reader(ks);
        let group = kr.u16()?;
        let share = kr.vec16()?.to_vec();
        break (suite, group, share);
    };

    let h = suite.hash;
    let mut ctx = transcript.unwrap_or_else(|| {
        let mut c = digest::Context::new(h.digest());
        c.update(&hello.msg);
        c
    });
    ctx.update(&sh);

    let ecdhe = hello.keys.shared_secret(group, &server_share).map_err(Error::Protocol)?;
    let early = keysched::extract(h, &[], Secret::zeros(h.len()).as_slice());
    let empty_hash = h.hash(b"");
    let derived = keysched::derive_secret(h, early.as_slice(), b"derived", empty_hash.as_slice());
    let hs_secret = keysched::extract(h, derived.as_slice(), &ecdhe);
    let th = ctx.clone().finish();
    let c_hs = keysched::derive_secret(h, hs_secret.as_slice(), b"c hs traffic", th.as_ref());
    let s_hs = keysched::derive_secret(h, hs_secret.as_slice(), b"s hs traffic", th.as_ref());
    if !hs_buf.is_empty() {
        return Err(Error::Protocol("data after ServerHello in the same record"));
    }
    conn.rx = Some(Direction::new(suite, s_hs.clone()));
    conn.tx = Some(Direction::new(suite, c_hs.clone()));
    if !sent_ccs {
        conn.queue(CT_CCS, 0x0303, &[1]);
    }

    let ee = next_msg(conn, &mut hs_buf).await?;
    expect(&ee, HT_ENCRYPTED_EXTENSIONS, "expected EncryptedExtensions")?;
    ctx.update(&ee);
    let mut alpn = None;
    let mut alps = None;
    for (t, b) in Reader(&ee[4..]).extensions()? {
        match t {
            16 => {
                let mut r = Reader(Reader(b).vec16()?);
                let p = r.vec8()?.to_vec();
                if !r.is_empty() || !hello.alpn.contains(&p) {
                    return Err(Error::Protocol("server selected ALPN we did not offer"));
                }
                alpn = Some(p);
            }
            hello::EXT_APPLICATION_SETTINGS_NEW | hello::EXT_APPLICATION_SETTINGS_OLD => {
                if hello.alps != Some(t) {
                    return Err(Error::Protocol("unexpected application_settings"));
                }
                alps = Some(t);
            }
            28 => conn.record_limit = record_size_limit(b, true)?,
            _ => {}
        }
    }

    let mut msg = next_msg(conn, &mut hs_buf).await?;
    let mut cert_requested = false;
    if msg[0] == HT_CERTIFICATE_REQUEST {
        ctx.update(&msg);
        cert_requested = true;
        msg = next_msg(conn, &mut hs_buf).await?;
    }
    let certs = match msg[0] {
        HT_CERTIFICATE => parse_certificate_body(&msg[4..])?,
        HT_COMPRESSED_CERTIFICATE => {
            let mut r = Reader(&msg[4..]);
            let alg = r.u16()?;
            if !hello.cert_compression.contains(&alg) {
                return Err(Error::Protocol("certificate compressed with an algorithm we did not offer"));
            }
            let len = r.u24()?;
            let body = decompress_cert(alg, r.vec24()?, len)?;
            parse_certificate_body(&body)?
        }
        _ => return Err(Error::Protocol("expected Certificate")),
    };
    ctx.update(&msg);

    let peer = match (&cfg.verify, auth_key) {
        (Verify::Reality(rc), Some(k)) => match reality_verify_cert(&certs[0], &k, &rc.mldsa65_verify, &hello.msg, &sh) {
            Ok(key) => Peer::Reality(key),
            Err(e @ Error::RealityMldsa65(_)) => return Err(e),
            Err(_) => {
                verify_chain(&certs, &cfg.server_name, roots)?;
                Peer::WebPki(certs)
            }
        },
        (Verify::WebPki, _) => {
            verify_chain(&certs, &cfg.server_name, roots)?;
            Peer::WebPki(certs)
        }
        (Verify::Pinned(pins), _) => {
            verify_pinned(&certs, pins, &cfg.server_name)?;
            Peer::Pinned(certs)
        }
        _ => Peer::Insecure(certs),
    };

    let cv = next_msg(conn, &mut hs_buf).await?;
    expect(&cv, HT_CERTIFICATE_VERIFY, "expected CertificateVerify")?;
    let mut r = Reader(&cv[4..]);
    let scheme = r.u16()?;
    let sig = r.vec16()?;
    if !matches!(peer, Peer::Reality(_)) && !hello.signature_algorithms.contains(&scheme) {
        return Err(Error::Protocol("CertificateVerify with a scheme we did not offer"));
    }
    verify_cv(&peer, scheme, sig, ctx.clone().finish().as_ref())?;
    ctx.update(&cv);

    let fin = next_msg(conn, &mut hs_buf).await?;
    expect(&fin, HT_FINISHED, "expected Finished")?;
    let expected = keysched::finished_verify_data(h, s_hs.as_slice(), ctx.clone().finish().as_ref());
    if !ct_eq(&fin[4..], expected.as_slice()) {
        return Err(Error::Protocol("bad server Finished"));
    }
    ctx.update(&fin);
    if !hs_buf.is_empty() {
        return Err(Error::Protocol("handshake data after server Finished"));
    }

    let th_sf = ctx.clone().finish();
    let derived2 = keysched::derive_secret(h, hs_secret.as_slice(), b"derived", empty_hash.as_slice());
    let master = keysched::extract(h, derived2.as_slice(), Secret::zeros(h.len()).as_slice());
    let c_ap = keysched::derive_secret(h, master.as_slice(), b"c ap traffic", th_sf.as_ref());
    let s_ap = keysched::derive_secret(h, master.as_slice(), b"s ap traffic", th_sf.as_ref());
    conn.rx = Some(Direction::new(suite, s_ap));
    conn.ccs_left = 0;

    let mut flight = Vec::new();
    if let Some(t) = alps {
        let ee = [HT_ENCRYPTED_EXTENSIONS, 0, 0, 6, 0, 4, (t >> 8) as u8, t as u8, 0, 0];
        ctx.update(&ee);
        flight.extend_from_slice(&ee);
    }
    if cert_requested {
        let c = [HT_CERTIFICATE, 0, 0, 4, 0, 0, 0, 0];
        ctx.update(&c);
        flight.extend_from_slice(&c);
    }
    let vd = keysched::finished_verify_data(h, c_hs.as_slice(), ctx.clone().finish().as_ref());
    flight.extend_from_slice(&[HT_FINISHED, 0, 0, h.len() as u8]);
    flight.extend_from_slice(vd.as_slice());
    conn.queue(CT_HANDSHAKE, 0x0303, &flight);
    conn.tx = Some(Direction::new(suite, c_ap));
    conn.flush_out().await?;

    Ok(Outcome { alpn, reality_authenticated: matches!(peer, Peer::Reality(_)) })
}

#[cfg(kani)]
#[path = "proofs/handshake.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/handshake.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;
    use ml_dsa::{KeyInit, Keypair, MlDsa65, Signer, SigningKey};

    #[test]
    fn reality_mldsa65_binds_key_and_raw_hellos() {
        let auth = [7; 32];
        let (ch, sh) = (b"\x01\0\0\x05hello".as_slice(), b"\x02\0\0\x05world".as_slice());
        let ed = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap();
        let public = ed.public_key_raw();
        let sk = SigningKey::<MlDsa65>::new_from_slice(&[42; 32]).unwrap();
        let pk = sk.verifying_key().encode();
        let hkey = hmac::Key::new(hmac::HMAC_SHA512, &auth);
        let sig = sk.sign(hmac::sign(&hkey, &[public, ch, sh].concat()).as_ref()).encode();
        let leaf = |extensions: Vec<Vec<u8>>| {
            let mut p = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
            p.custom_extensions = extensions
                .into_iter()
                .enumerate()
                .map(|(i, v)| rcgen::CustomExtension::from_oid_content(&[1, 2, 3, i as u64], v))
                .collect();
            let mut cert = p.self_signed(&ed).unwrap().der().to_vec();
            let at = cert.len() - 64;
            cert[at..].copy_from_slice(hmac::sign(&hkey, public).as_ref());
            cert
        };
        let cert = leaf(vec![sig.to_vec(), vec![0]]);
        assert_eq!(reality_verify_cert(&cert, &auth, &pk, ch, sh).unwrap().as_slice(), public);
        for (ch, sh) in [(&ch[4..], sh), (ch, &sh[4..]), (sh, ch)] {
            assert!(matches!(reality_verify_cert(&cert, &auth, &pk, ch, sh), Err(Error::RealityMldsa65(_))));
        }
        let wrong = SigningKey::<MlDsa65>::new_from_slice(&[43; 32]).unwrap().verifying_key().encode();
        assert!(matches!(reality_verify_cert(&cert, &auth, &wrong, ch, sh), Err(Error::RealityMldsa65(_))));
        assert!(matches!(reality_verify_cert(&cert, &auth, &pk[..1951], ch, sh), Err(Error::RealityMldsa65(_))));
        let mut corrupt = sig.to_vec();
        corrupt[0] ^= 1;
        for cert in [leaf(vec![]), leaf(vec![vec![0]]), leaf(vec![corrupt]), leaf(vec![vec![0], sig.to_vec()])] {
            assert!(matches!(reality_verify_cert(&cert, &auth, &pk, ch, sh), Err(Error::RealityMldsa65(_))));
            assert!(reality_verify_cert(&cert, &auth, &[], ch, sh).is_ok());
        }
        assert!(matches!(reality_verify_cert(&cert, &[8; 32], &pk, ch, sh), Err(Error::RealityAuth)));
    }

    fn cert_body(entries: &[&[u8]]) -> Vec<u8> {
        let mut list = Vec::new();
        for e in entries {
            list.extend_from_slice(&(e.len() as u32).to_be_bytes()[1..]);
            list.extend_from_slice(e);
            list.extend_from_slice(&[0, 0]);
        }
        let mut body = vec![0];
        body.extend_from_slice(&(list.len() as u32).to_be_bytes()[1..]);
        body.extend_from_slice(&list);
        body
    }

    #[test]
    fn certificate_list_and_decompression_are_bounded() {
        let der = [0x30u8, 0];
        assert_eq!(parse_certificate_body(&cert_body(&[&der[..]; 10])).unwrap().len(), 10);
        assert!(matches!(parse_certificate_body(&cert_body(&[&der[..]; 11])), Err(Error::BadCertificate(_))));
        assert!(matches!(parse_certificate_body(&cert_body(&[&der[..], &[]])), Err(Error::Protocol("empty certificate"))));
        let bomb = cert_body(&vec![&[][..]; 200_000]);
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(&bomb, 9);
        assert!(packed.len() < 4096);
        assert!(decompress_cert(hello::CERT_COMPRESSION_ZLIB, &packed, bomb.len()).is_err());
        for alg in [hello::CERT_COMPRESSION_ZLIB, hello::CERT_COMPRESSION_BROTLI, hello::CERT_COMPRESSION_ZSTD] {
            assert!(matches!(decompress_cert(alg, &packed, MAX_CERT_LIST + 1), Err(Error::Protocol("compressed certificate too large"))));
        }
        let small = cert_body(&vec![&[][..]; 20_000]);
        let out =
            decompress_cert(hello::CERT_COMPRESSION_ZLIB, &miniz_oxide::deflate::compress_to_vec_zlib(&small, 9), small.len()).unwrap();
        assert!(matches!(parse_certificate_body(&out), Err(Error::Protocol("empty certificate"))));
    }

    #[test]
    fn largest_extension_block_parses_in_linear_time() {
        let block = |types: &mut dyn Iterator<Item = u16>| {
            let inner: Vec<u8> = types.flat_map(|t| [(t >> 8) as u8, t as u8, 0, 0]).collect();
            [(inner.len() as u16).to_be_bytes().as_slice(), &inner].concat()
        };
        let all = block(&mut (0..16383u16).map(|i| i.wrapping_mul(4099)));
        let t0 = std::time::Instant::now();
        assert_eq!(Reader(&all).extensions().unwrap().len(), 16383);
        assert!(t0.elapsed() < std::time::Duration::from_millis(100), "{:?}", t0.elapsed());
        for dup in [[0, 0], [65535, 65535], [64, 64], [7, 16382]] {
            let mut types = (0..16381u16).map(|i| i + 1).chain(dup);
            assert!(matches!(Reader(&block(&mut types)).extensions(), Err(Error::Protocol("duplicate extension"))));
        }
    }

    #[test]
    fn record_size_limit_counts_the_tls13_content_type() {
        assert_eq!(record_size_limit(&[0x40, 0x01], true).unwrap(), MAX_PT);
        assert_eq!(record_size_limit(&[0xff, 0xff], false).unwrap(), MAX_PT);
        assert_eq!(record_size_limit(&[0, 64], true).unwrap(), 63);
        assert_eq!(record_size_limit(&[0, 64], false).unwrap(), 64);
        for bad in [&[0, 63][..], &[0, 64, 0], &[1]] {
            assert!(record_size_limit(bad, true).is_err());
        }
    }
}
