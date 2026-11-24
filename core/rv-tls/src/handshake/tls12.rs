use super::*;
use crate::keysched::{HashAlg, Suite};

fn suite(id: u16) -> Option<Suite> {
    Some(match id {
        0xc02b | 0xc02f => Suite { aead: &aead::AES_128_GCM, hash: HashAlg::Sha256 },
        0xc02c | 0xc030 => Suite { aead: &aead::AES_256_GCM, hash: HashAlg::Sha384 },
        0xcca8 | 0xcca9 => Suite { aead: &aead::CHACHA20_POLY1305, hash: HashAlg::Sha256 },
        _ => return None,
    })
}

fn cbc(id: u16) -> Option<(usize, hmac::Algorithm)> {
    Some(match id {
        0xc009 | 0xc013 => (16, hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY),
        0xc00a | 0xc014 => (32, hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY),
        0xc023 | 0xc027 => (16, hmac::HMAC_SHA256),
        _ => return None,
    })
}

fn prf(hash: HashAlg, secret: &[u8], label: &[u8], seed: &[u8], len: usize) -> Secret {
    let mut out = prf_bytes(hash, secret, label, seed, len);
    let result = Secret::from(out.as_slice());
    out.fill(0);
    result
}

fn message(typ: u8, body: &[u8]) -> Vec<u8> {
    let mut msg = vec![typ];
    msg.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    msg.extend_from_slice(body);
    msg
}

fn verify_ske(cert: &[u8], suite: u16, scheme: u16, signed: &[u8], sig: &[u8]) -> Result<(), Error> {
    use ring::signature as a;
    let cert = der::parse(cert).ok_or_else(|| Error::BadCertificate("TLS 1.2: bad DER".into()))?;
    let ec_auth = matches!(suite, 0xc02b | 0xc02c | 0xcca9 | 0xc009 | 0xc00a | 0xc023);
    let ec_key = cert.key_alg == [0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
    let ed_key = cert.key_alg == [0x2b, 0x65, 0x70];
    let rsa_key = cert.key_alg == [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
    if (ec_auth && !(ec_key || ed_key)) || (!ec_auth && !rsa_key) {
        return Err(Error::BadCertificate("TLS 1.2: certificate key does not match cipher suite".into()));
    }
    let bad = || Error::BadCertificate("TLS 1.2: bad ServerKeyExchange signature".into());
    let unsupported = || Error::BadCertificate("TLS 1.2: unsupported EC key".into());
    if matches!(scheme, 0x0403 | 0x0503 | 0x0603) && cert.key.len() == 133 {
        let hash = match scheme {
            0x0403 => &digest::SHA256,
            0x0503 => &digest::SHA384,
            _ => &digest::SHA512,
        };
        return crate::p521::verify(cert.key, hash, signed, sig).then_some(()).ok_or_else(bad);
    }
    let alg: &dyn a::VerificationAlgorithm = match scheme {
        0x0403 => match cert.key.len() {
            65 => &a::ECDSA_P256_SHA256_ASN1,
            97 => &a::ECDSA_P384_SHA256_ASN1,
            _ => return Err(unsupported()),
        },
        0x0503 => match cert.key.len() {
            65 => &a::ECDSA_P256_SHA384_ASN1,
            97 => &a::ECDSA_P384_SHA384_ASN1,
            _ => return Err(unsupported()),
        },
        0x0401 => &a::RSA_PKCS1_2048_8192_SHA256,
        0x0501 => &a::RSA_PKCS1_2048_8192_SHA384,
        0x0601 => &a::RSA_PKCS1_2048_8192_SHA512,
        0x0804 => &a::RSA_PSS_2048_8192_SHA256,
        0x0805 => &a::RSA_PSS_2048_8192_SHA384,
        0x0806 => &a::RSA_PSS_2048_8192_SHA512,
        0x0807 => &a::ED25519,
        _ => return Err(Error::Unsupported("TLS 1.2 ServerKeyExchange signature")),
    };
    a::UnparsedPublicKey::new(alg, cert.key).verify(signed, sig).map_err(|_| bad())
}

fn exchange(group: u16, peer: &[u8]) -> Result<(Vec<u8>, Secret), Error> {
    use ring::agreement as a;
    let alg = match group {
        hello::GROUP_X25519 => &a::X25519,
        hello::GROUP_SECP256R1 => &a::ECDH_P256,
        hello::GROUP_SECP384R1 => &a::ECDH_P384,
        hello::GROUP_SECP521R1 => {
            let key = crate::p521::Ephemeral::generate().ok_or(Error::Protocol("TLS 1.2 ECDHE keygen"))?;
            let public = key.public.clone();
            let secret = key.agree(peer).ok_or(Error::Protocol("TLS 1.2 ECDHE agreement"))?;
            return Ok((public, Secret::from(secret.as_slice())));
        }
        _ => return Err(Error::Unsupported("TLS 1.2 ECDHE group")),
    };
    let key =
        a::EphemeralPrivateKey::generate(alg, &ring::rand::SystemRandom::new()).map_err(|_| Error::Protocol("TLS 1.2 ECDHE keygen"))?;
    let public = key.compute_public_key().map_err(|_| Error::Protocol("TLS 1.2 ECDHE key"))?.as_ref().to_vec();
    let peer = a::UnparsedPublicKey::new(alg, peer);
    let secret = a::agree_ephemeral(key, &peer, |s| Secret::from(s)).map_err(|_| Error::Protocol("TLS 1.2 ECDHE agreement"))?;
    Ok((public, secret))
}

pub(super) async fn run<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    conn: &mut Conn<S>,
    cfg: &ClientConfig,
    roots: &[TrustAnchor<'_>],
    hello: &ClientHello,
    sh: &[u8],
    hs_buf: &mut Vec<u8>,
) -> Result<Outcome, Error> {
    if matches!(cfg.verify, Verify::Reality(_)) {
        return Err(Error::Unsupported("REALITY requires TLS 1.3"));
    }
    conn.ccs_left = 0;
    let mut r = Reader(&sh[4..]);
    if r.u16()? != 0x0303 {
        return Err(Error::Unsupported("TLS version"));
    }
    let random = r.take(32)?;
    let offered_tls13 =
        hello.exts.iter().any(|(kind, body)| *kind == 43 && body.get(1..).is_some_and(|b| b.as_chunks::<2>().0.contains(&[3, 4])));
    if offered_tls13 && (random[24..] == *b"DOWNGRD\x01" || random[24..] == *b"DOWNGRD\0") {
        return Err(Error::Protocol("TLS downgrade sentinel"));
    }
    let _sid = r.vec8()?;
    let suite_id = r.u16()?;
    if !hello.suites_raw.contains(&suite_id) {
        return Err(Error::Protocol("TLS 1.2 unoffered cipher suite"));
    }
    let cbc = cbc(suite_id);
    let suite = match cbc {
        Some(_) => Suite { aead: &aead::AES_128_GCM, hash: HashAlg::Sha256 },
        None => suite(suite_id).ok_or(Error::Unsupported(
            "TLS 1.2 cipher suite: only ECDHE with AES-GCM, ChaCha20 or AES-CBC; DHE, RSA key exchange, RC4 and 3DES are not implemented",
        ))?,
    };
    if r.u8()? != 0 {
        return Err(Error::Protocol("TLS compression"));
    }
    let extensions = if r.is_empty() { Vec::new() } else { r.extensions()? };
    if !r.is_empty() {
        return Err(Error::Protocol("TLS 1.2 trailing ServerHello"));
    }
    let mut ems = false;
    let mut alpn = None;
    let mut status_ack = false;
    let mut ticket_ack = false;
    for (kind, body) in extensions {
        if !hello.exts.iter().any(|e| e.0 == kind) {
            return Err(Error::Protocol("TLS 1.2 unsolicited extension"));
        }
        match kind {
            23 if body.is_empty() => ems = true,
            16 => {
                let mut protocols = Reader(Reader(body).vec16()?);
                let selected = protocols.vec8()?;
                if !protocols.is_empty() || !hello.alpn.iter().any(|p| p == selected) {
                    return Err(Error::Protocol("TLS 1.2 unoffered ALPN"));
                }
                alpn = Some(selected.to_vec());
            }
            0 if body.is_empty() => {}
            5 if body.is_empty() => status_ack = true,
            35 if body.is_empty() => ticket_ack = true,
            11 if body.first().is_some_and(|n| usize::from(*n) + 1 == body.len()) && body[1..].contains(&0) => {}
            0xff01 if body == [0] => {}
            18 => {}
            28 => conn.record_limit = record_size_limit(body, false)?,
            _ => return Err(Error::Protocol("TLS 1.2 invalid ServerHello extension")),
        }
    }
    let mut transcript = digest::Context::new(suite.hash.digest());
    transcript.update(&hello.msg);
    transcript.update(sh);
    let cert_msg = next_msg(conn, hs_buf).await?;
    expect(&cert_msg, HT_CERTIFICATE, "TLS 1.2 expected Certificate")?;
    let mut cert_body = Reader(&cert_msg[4..]);
    let mut list = Reader(cert_body.vec24()?);
    if !cert_body.is_empty() {
        return Err(Error::Protocol("TLS 1.2 trailing Certificate"));
    }
    let mut certs = Vec::new();
    while !list.is_empty() {
        push_cert(&mut certs, list.vec24()?)?;
    }
    if certs.is_empty() {
        return Err(Error::BadCertificate("TLS 1.2 empty chain".into()));
    }
    match &cfg.verify {
        Verify::WebPki => verify_chain(&certs, &cfg.server_name, roots)?,
        Verify::Pinned(pins) => verify_pinned(&certs, pins, &cfg.server_name)?,
        Verify::InsecureSkipVerify => {}
        Verify::Reality(_) => return Err(Error::Unsupported("REALITY requires TLS 1.3")),
    }
    transcript.update(&cert_msg);
    let mut ske = next_msg(conn, hs_buf).await?;
    if ske[0] == 22 {
        let mut status = Reader(&ske[4..]);
        if !status_ack || status.u8()? != 1 || status.vec24()?.is_empty() || !status.is_empty() {
            return Err(Error::Protocol("TLS 1.2 invalid CertificateStatus"));
        }
        transcript.update(&ske);
        ske = next_msg(conn, hs_buf).await?;
    }
    expect(&ske, 12, "TLS 1.2 expected ServerKeyExchange")?;
    let mut r = Reader(&ske[4..]);
    if r.u8()? != 3 {
        return Err(Error::Protocol("TLS 1.2 ECDHE curve type"));
    }
    let group = r.u16()?;
    if !hello.supported_groups.contains(&group) {
        return Err(Error::Protocol("TLS 1.2 unoffered group"));
    }
    let public = r.vec8()?;
    let params_end = ske.len() - r.0.len();
    let scheme = r.u16()?;
    let sig = r.vec16()?;
    if !r.is_empty() || !hello.signature_algorithms.contains(&scheme) {
        return Err(Error::Protocol("TLS 1.2 unoffered signature or trailing data"));
    }
    let signed = [hello.random.as_slice(), random, &ske[4..params_end]].concat();
    verify_ske(&certs[0], suite_id, scheme, &signed, sig)?;
    let (public, premaster) = exchange(group, public)?;
    transcript.update(&ske);
    let mut done = next_msg(conn, hs_buf).await?;
    let mut client_auth = false;
    if done[0] == HT_CERTIFICATE_REQUEST {
        transcript.update(&done);
        client_auth = true;
        done = next_msg(conn, hs_buf).await?;
    }
    expect(&done, 14, "TLS 1.2 expected ServerHelloDone")?;
    if done.len() != 4 || !hs_buf.is_empty() {
        return Err(Error::Protocol("TLS 1.2 ServerHelloDone data"));
    }
    transcript.update(&done);
    if client_auth {
        let empty_cert = message(HT_CERTIFICATE, &[0, 0, 0]);
        transcript.update(&empty_cert);
        conn.queue(CT_HANDSHAKE, 0x0303, &empty_cert);
    }
    let mut cke_body = vec![public.len() as u8];
    cke_body.extend_from_slice(&public);
    let cke = message(16, &cke_body);
    transcript.update(&cke);
    conn.queue(CT_HANDSHAKE, 0x0303, &cke);
    let master = if ems {
        prf(suite.hash, premaster.as_slice(), b"extended master secret", transcript.clone().finish().as_ref(), 48)
    } else {
        prf(suite.hash, premaster.as_slice(), b"master secret", &[hello.random.as_slice(), random].concat(), 48)
    };
    let (client, server, mut key_bytes) = if let Some((key_len, mac)) = cbc {
        let mac_len = mac.digest_algorithm().output_len();
        let key_bytes = prf_bytes(
            suite.hash,
            master.as_slice(),
            b"key expansion",
            &[random, hello.random.as_slice()].concat(),
            2 * (mac_len + key_len),
        );
        let (client_mac, rest) = key_bytes.split_at(mac_len);
        let (server_mac, rest) = rest.split_at(mac_len);
        let (client_key, server_key) = rest.split_at(key_len);
        let client = Direction::new_tls12_cbc(suite, client_key, mac, client_mac);
        let server = Direction::new_tls12_cbc(suite, server_key, mac, server_mac);
        (client, server, key_bytes)
    } else {
        let key_len = suite.aead.key_len();
        let iv_len = if suite.aead == &aead::CHACHA20_POLY1305 { 12 } else { 4 };
        let key_bytes =
            prf_bytes(suite.hash, master.as_slice(), b"key expansion", &[random, hello.random.as_slice()].concat(), 2 * (key_len + iv_len));
        let (client_key, rest) = key_bytes.split_at(key_len);
        let (server_key, rest) = rest.split_at(key_len);
        let (client_iv, server_iv) = rest.split_at(iv_len);
        let client = Direction::new_tls12(suite, client_key, client_iv);
        let server = Direction::new_tls12(suite, server_key, server_iv);
        (client, server, key_bytes)
    };
    key_bytes.fill(0);
    conn.queue(CT_CCS, 0x0303, &[1]);
    conn.tx = Some(client);
    let finished = prf(suite.hash, master.as_slice(), b"client finished", transcript.clone().finish().as_ref(), 12);
    let finished = message(HT_FINISHED, finished.as_slice());
    conn.queue(CT_HANDSHAKE, 0x0303, &finished);
    transcript.update(&finished);
    conn.flush_out().await?;
    conn.want_ccs = true;
    loop {
        let record = poll_fn(|cx| conn.poll_record(cx)).await?.ok_or(Error::Protocol("TLS 1.2 missing server CCS"))?;
        let body = conn.slice(record.start, record.end);
        match record.ctype {
            CT_CCS if body == [1] && hs_buf.is_empty() => break,
            CT_HANDSHAKE => {
                hs_buf.extend_from_slice(body);
                while hs_buf.len() >= 4 {
                    let len = u32::from_be_bytes([0, hs_buf[1], hs_buf[2], hs_buf[3]]) as usize;
                    if len > 1 << 17 {
                        return Err(Error::Protocol("TLS 1.2 ticket too large"));
                    }
                    if hs_buf.len() < len + 4 {
                        break;
                    }
                    if hs_buf[0] != 4 || !ticket_ack {
                        return Err(Error::Protocol("TLS 1.2 expected acknowledged NewSessionTicket or CCS"));
                    }
                    let mut ticket = Reader(&hs_buf[4..len + 4]);
                    ticket.take(4)?;
                    ticket.vec16()?;
                    if !ticket.is_empty() {
                        return Err(Error::Protocol("TLS 1.2 trailing NewSessionTicket"));
                    }
                    transcript.update(&hs_buf[..len + 4]);
                    hs_buf.drain(..len + 4);
                }
            }
            CT_ALERT => return Err(Error::Alert(body.get(1).copied().unwrap_or(0))),
            _ => return Err(Error::Protocol("TLS 1.2 invalid CCS flight")),
        }
    }
    conn.want_ccs = false;
    conn.rx = Some(server);
    let server_finished = next_msg(conn, hs_buf).await?;
    expect(&server_finished, HT_FINISHED, "TLS 1.2 expected Finished")?;
    let expected = prf(suite.hash, master.as_slice(), b"server finished", transcript.clone().finish().as_ref(), 12);
    if !ct_eq(&server_finished[4..], expected.as_slice()) || !hs_buf.is_empty() {
        return Err(Error::Protocol("TLS 1.2 Finished mismatch"));
    }
    Ok(Outcome { alpn, reality_authenticated: false })
}

fn prf_bytes(hash: HashAlg, secret: &[u8], label: &[u8], seed: &[u8], len: usize) -> Vec<u8> {
    let alg = match hash {
        HashAlg::Sha256 => hmac::HMAC_SHA256,
        HashAlg::Sha384 => hmac::HMAC_SHA384,
    };
    let key = hmac::Key::new(alg, secret);
    let seed = [label, seed].concat();
    let mut a = hmac::sign(&key, &seed).as_ref().to_vec();
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        let block = hmac::sign(&key, &[a.as_slice(), seed.as_slice()].concat());
        out.extend_from_slice(&block.as_ref()[..(len - out.len()).min(block.as_ref().len())]);
        a = hmac::sign(&key, &a).as_ref().to_vec();
    }
    a.fill(0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p521_premaster_fits_and_agrees_with_the_peer() {
        let peer = crate::p521::Ephemeral::generate().unwrap();
        let (public, secret) = exchange(hello::GROUP_SECP521R1, &peer.public).unwrap();
        assert_eq!(secret.as_slice().len(), 66);
        assert_eq!(secret.as_slice(), peer.agree(&public).unwrap());
    }

    #[test]
    fn rsa_kex_dhe_rc4_3des_fail_explicitly_and_ecdhe_cbc_is_supported() {
        for id in [0x0035, 0x003d, 0x0039, 0x006b, 0xc007, 0xc011, 0x000a, 0x0004, 0xc012] {
            assert!(suite(id).is_none() && cbc(id).is_none(), "unsupported TLS 1.2 suite {id:04x}");
        }
        for id in [0xc009, 0xc00a, 0xc013, 0xc014, 0xc023, 0xc027] {
            assert!(cbc(id).is_some(), "ECDHE AES-CBC {id:04x}");
        }
    }
}
