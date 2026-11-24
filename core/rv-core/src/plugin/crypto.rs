use blake2::Digest;
use rv_plugin_abi::{aead, cipher, err, hash, mac, sig};

type R = Result<usize, i32>;

fn put(out: &mut [u8], d: &[u8]) -> R {
    let o = out.get_mut(..d.len()).ok_or(err::TOO_SMALL)?;
    o.copy_from_slice(d);
    Ok(d.len())
}

pub(super) fn digest(alg: u32, data: &[u8], out: &mut [u8]) -> R {
    use ring::digest as d;
    let ring = |a: &'static d::Algorithm| d::digest(a, data);
    match alg {
        hash::SHA1 => put(out, ring(&d::SHA1_FOR_LEGACY_USE_ONLY).as_ref()),
        hash::SHA256 => put(out, ring(&d::SHA256).as_ref()),
        hash::SHA384 => put(out, ring(&d::SHA384).as_ref()),
        hash::SHA512 => put(out, ring(&d::SHA512).as_ref()),
        hash::BLAKE2S256 => put(out, &blake2::Blake2s256::digest(data)),
        hash::BLAKE2B512 => put(out, &blake2::Blake2b512::digest(data)),
        hash::BLAKE3 => put(out, blake3::hash(data).as_bytes()),
        hash::MD5 => put(out, &md5::Md5::digest(data)),
        _ => Err(err::UNSUPPORTED),
    }
}

fn hmac_blake2s(key: &[u8], data: &[&[u8]]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&blake2::Blake2s256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = blake2::Blake2s256::new();
    inner.update(k.map(|b| b ^ 0x36));
    for d in data {
        inner.update(d);
    }
    let ih = inner.finalize();
    let mut outer = blake2::Blake2s256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(ih);
    outer.finalize().into()
}

pub(super) fn mac(alg: u32, key: &[u8], data: &[u8], out: &mut [u8]) -> R {
    use ring::hmac as h;
    let ring = |a: h::Algorithm| h::sign(&h::Key::new(a, key), data);
    match alg {
        mac::HMAC_SHA1 => put(out, ring(h::HMAC_SHA1_FOR_LEGACY_USE_ONLY).as_ref()),
        mac::HMAC_SHA256 => put(out, ring(h::HMAC_SHA256).as_ref()),
        mac::HMAC_SHA384 => put(out, ring(h::HMAC_SHA384).as_ref()),
        mac::HMAC_SHA512 => put(out, ring(h::HMAC_SHA512).as_ref()),
        mac::HMAC_MD5 => {
            let mut k = [0u8; 64];
            if key.len() > 64 {
                k[..16].copy_from_slice(&md5::Md5::digest(key));
            } else {
                k[..key.len()].copy_from_slice(key);
            }
            let mut inner = md5::Md5::new();
            inner.update(k.map(|b| b ^ 0x36));
            inner.update(data);
            let mut outer = md5::Md5::new();
            outer.update(k.map(|b| b ^ 0x5c));
            outer.update(inner.finalize());
            put(out, &outer.finalize())
        }
        mac::HMAC_BLAKE2S => put(out, &hmac_blake2s(key, &[data])),
        mac::BLAKE2S_KEYED_128 | mac::BLAKE2S_KEYED_256 => {
            use blake2::digest::{KeyInit, Mac};
            if key.is_empty() || key.len() > 32 {
                return Err(err::INVALID);
            }
            if alg == mac::BLAKE2S_KEYED_128 {
                let mut m = <blake2::Blake2sMac<blake2::digest::consts::U16> as KeyInit>::new_from_slice(key).map_err(|_| err::INVALID)?;
                m.update(data);
                put(out, &m.finalize().into_bytes())
            } else {
                let mut m = <blake2::Blake2sMac256 as KeyInit>::new_from_slice(key).map_err(|_| err::INVALID)?;
                m.update(data);
                put(out, &m.finalize().into_bytes())
            }
        }
        mac::BLAKE3_KEYED => {
            let k: [u8; 32] = key.try_into().map_err(|_| err::INVALID)?;
            put(out, blake3::keyed_hash(&k, data).as_bytes())
        }
        _ => Err(err::UNSUPPORTED),
    }
}

pub(super) fn hkdf(alg: u32, salt: &[u8], ikm: &[u8], info: &[u8], out: &mut [u8]) -> R {
    if out.is_empty() || out.len() > 255 * 32 {
        return Err(err::INVALID);
    }
    if alg == hash::BLAKE2S256 {
        let prk = hmac_blake2s(salt, &[ikm]);
        let mut t: Vec<u8> = Vec::new();
        let mut at = 0;
        let mut i = 1u8;
        while at < out.len() {
            let block = hmac_blake2s(&prk, &[&t, info, &[i]]);
            let n = (out.len() - at).min(32);
            out[at..at + n].copy_from_slice(&block[..n]);
            at += n;
            t = block.to_vec();
            i += 1;
        }
        return Ok(out.len());
    }
    use ring::hkdf as k;
    let a = match alg {
        hash::SHA1 => k::HKDF_SHA1_FOR_LEGACY_USE_ONLY,
        hash::SHA256 => k::HKDF_SHA256,
        hash::SHA384 => k::HKDF_SHA384,
        hash::SHA512 => k::HKDF_SHA512,
        _ => return Err(err::UNSUPPORTED),
    };
    struct Len(usize);
    impl k::KeyType for Len {
        fn len(&self) -> usize {
            self.0
        }
    }
    let n = out.len();
    let infos = [info];
    k::Salt::new(a, salt).extract(ikm).expand(&infos, Len(n)).and_then(|okm| okm.fill(out)).map_err(|_| err::INVALID)?;
    Ok(n)
}

fn ring_aead(alg: u32, key: &[u8]) -> Result<ring::aead::LessSafeKey, i32> {
    let a = match alg {
        aead::AES128_GCM => &ring::aead::AES_128_GCM,
        aead::AES256_GCM => &ring::aead::AES_256_GCM,
        aead::CHACHA20_POLY1305 | aead::XCHACHA20_POLY1305 => &ring::aead::CHACHA20_POLY1305,
        _ => return Err(err::UNSUPPORTED),
    };
    Ok(ring::aead::LessSafeKey::new(ring::aead::UnboundKey::new(a, key).map_err(|_| err::INVALID)?))
}

fn xchacha_key(key: &[u8], nonce: &[u8]) -> Result<([u8; 32], [u8; 12]), i32> {
    if key.len() != 32 || nonce.len() != 24 {
        return Err(err::INVALID);
    }
    let sub = chacha20::hchacha::<chacha20::R20>(key.try_into().unwrap(), nonce[..16].try_into().unwrap());
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&nonce[16..]);
    Ok((sub.into(), n))
}

fn nonce_key(alg: u32, key: &[u8], nonce: &[u8]) -> Result<(ring::aead::LessSafeKey, ring::aead::Nonce), i32> {
    if alg == aead::XCHACHA20_POLY1305 {
        let (k, n) = xchacha_key(key, nonce)?;
        return Ok((ring_aead(alg, &k)?, ring::aead::Nonce::assume_unique_for_key(n)));
    }
    let n = ring::aead::Nonce::try_assume_unique_for_key(nonce).map_err(|_| err::INVALID)?;
    Ok((ring_aead(alg, key)?, n))
}

pub(super) fn seal(alg: u32, key: &[u8], nonce: &[u8], aad: &[u8], buf: &mut [u8], len: usize) -> R {
    let (k, n) = nonce_key(alg, key, nonce)?;
    if len + 16 > buf.len() {
        return Err(err::TOO_SMALL);
    }
    let tag = k.seal_in_place_separate_tag(n, ring::aead::Aad::from(aad), &mut buf[..len]).map_err(|_| err::FAILED)?;
    buf[len..len + 16].copy_from_slice(tag.as_ref());
    Ok(len + 16)
}

pub(super) fn open(alg: u32, key: &[u8], nonce: &[u8], aad: &[u8], buf: &mut [u8]) -> R {
    let (k, n) = nonce_key(alg, key, nonce)?;
    if buf.len() < 16 {
        return Err(err::AUTH);
    }
    k.open_in_place(n, ring::aead::Aad::from(aad), buf).map(|p| p.len()).map_err(|_| err::AUTH)
}

pub(super) fn apply(alg: u32, key: &[u8], iv: &[u8], buf: &mut [u8]) -> R {
    use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit, KeyIvInit, StreamCipher, StreamCipherSeek};
    match alg {
        cipher::AES128_CTR => {
            let mut c = ctr::Ctr128BE::<aes::Aes128>::new_from_slices(key, iv).map_err(|_| err::INVALID)?;
            c.apply_keystream(buf);
        }
        cipher::AES256_CTR => {
            let mut c = ctr::Ctr128BE::<aes::Aes256>::new_from_slices(key, iv).map_err(|_| err::INVALID)?;
            c.apply_keystream(buf);
        }
        cipher::CHACHA20 => {
            if iv.len() != 16 {
                return Err(err::INVALID);
            }
            let counter = u32::from_le_bytes(iv[..4].try_into().unwrap());
            let mut c = chacha20::ChaCha20::new_from_slices(key, &iv[4..]).map_err(|_| err::INVALID)?;
            c.try_seek(u64::from(counter) * 64).map_err(|_| err::INVALID)?;
            c.try_apply_keystream(buf).map_err(|_| err::INVALID)?;
        }
        cipher::XCHACHA20 => {
            let mut c = chacha20::XChaCha20::new_from_slices(key, iv).map_err(|_| err::INVALID)?;
            c.try_apply_keystream(buf).map_err(|_| err::INVALID)?;
        }
        cipher::AES128_ECB | cipher::AES256_ECB | cipher::AES128_ECB_DECRYPT | cipher::AES256_ECB_DECRYPT => {
            if buf.len() % 16 != 0 {
                return Err(err::INVALID);
            }
            let enc = matches!(alg, cipher::AES128_ECB | cipher::AES256_ECB);
            let wide = matches!(alg, cipher::AES256_ECB | cipher::AES256_ECB_DECRYPT);
            if key.len() != if wide { 32 } else { 16 } {
                return Err(err::INVALID);
            }
            for b in buf.chunks_exact_mut(16) {
                let mut blk = aes::Block::try_from(&*b).map_err(|_| err::INVALID)?;
                match (wide, enc) {
                    (false, true) => aes::Aes128::new_from_slice(key).map_err(|_| err::INVALID)?.encrypt_block(&mut blk),
                    (false, false) => aes::Aes128::new_from_slice(key).map_err(|_| err::INVALID)?.decrypt_block(&mut blk),
                    (true, true) => aes::Aes256::new_from_slice(key).map_err(|_| err::INVALID)?.encrypt_block(&mut blk),
                    (true, false) => aes::Aes256::new_from_slice(key).map_err(|_| err::INVALID)?.decrypt_block(&mut blk),
                }
                b.copy_from_slice(&blk);
            }
        }
        _ => return Err(err::UNSUPPORTED),
    }
    Ok(buf.len())
}

pub(super) fn x25519(scalar: &[u8], point: Option<&[u8]>, out: &mut [u8]) -> R {
    let k: [u8; 32] = scalar.try_into().map_err(|_| err::INVALID)?;
    let u: [u8; 32] = match point {
        None => x25519_dalek::X25519_BASEPOINT_BYTES,
        Some(p) => p.try_into().map_err(|_| err::INVALID)?,
    };
    let r = x25519_dalek::x25519(k, u);
    if point.is_some() && r == [0; 32] {
        return Err(err::AUTH);
    }
    put(out, &r)
}

pub(super) fn verify(alg: u32, key: &[u8], msg: &[u8], signature: &[u8]) -> R {
    use ring::signature as s;
    let a: &dyn s::VerificationAlgorithm = match alg {
        sig::ECDSA_P256_SHA256 => &s::ECDSA_P256_SHA256_ASN1,
        sig::ECDSA_P384_SHA384 => &s::ECDSA_P384_SHA384_ASN1,
        sig::ED25519 => &s::ED25519,
        sig::RSA_PKCS1_SHA256 => &s::RSA_PKCS1_2048_8192_SHA256,
        sig::RSA_PSS_SHA256 => &s::RSA_PSS_2048_8192_SHA256,
        _ => return Err(err::UNSUPPORTED),
    };
    s::UnparsedPublicKey::new(a, key).verify(msg, signature).map(|_| 0).map_err(|_| err::AUTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn hashes_and_macs_match_known_answers() {
        let mut out = [0u8; 64];
        let n = digest(hash::SHA256, b"abc", &mut out).unwrap();
        assert_eq!(&out[..n], hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        let n = digest(hash::BLAKE2S256, b"abc", &mut out).unwrap();
        assert_eq!(&out[..n], hex("508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982"));
        let n = digest(hash::MD5, b"abc", &mut out).unwrap();
        assert_eq!(&out[..n], hex("900150983cd24fb0d6963f7d28e17f72"));
        let n = mac(mac::HMAC_SHA256, b"Jefe", b"what do ya want for nothing?", &mut out).unwrap();
        assert_eq!(&out[..n], hex("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"));
        let n = mac(mac::HMAC_MD5, b"Jefe", b"what do ya want for nothing?", &mut out).unwrap();
        assert_eq!(&out[..n], hex("750c783e6ab0b503eaa86e310a5db738"));
        assert_eq!(digest(hash::SHA512, b"x", &mut [0u8; 10]), Err(err::TOO_SMALL));
        assert_eq!(digest(99, b"x", &mut out), Err(err::UNSUPPORTED));
    }

    #[test]
    fn hkdf_matches_rfc5869_and_blake2s_matches_hmac() {
        let mut okm = [0u8; 42];
        hkdf(
            hash::SHA256,
            &hex("000102030405060708090a0b0c"),
            &hex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"),
            &hex("f0f1f2f3f4f5f6f7f8f9"),
            &mut okm,
        )
        .unwrap();
        assert_eq!(okm.to_vec(), hex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"));
        let (k, x) = (b"chaining key 32 bytes long......", b"input");
        let prk = hmac_blake2s(k, &[x]);
        let t1 = hmac_blake2s(&prk, &[&[1]]);
        let t2 = hmac_blake2s(&prk, &[&t1, &[2]]);
        let mut out = [0u8; 64];
        hkdf(hash::BLAKE2S256, k, x, b"", &mut out).unwrap();
        assert_eq!(&out[..32], t1);
        assert_eq!(&out[32..], t2);
    }

    #[test]
    fn aead_round_trips_and_rejects_tampering() {
        for (alg, klen, nlen) in
            [(aead::AES128_GCM, 16, 12), (aead::AES256_GCM, 32, 12), (aead::CHACHA20_POLY1305, 32, 12), (aead::XCHACHA20_POLY1305, 32, 24)]
        {
            let key = vec![7u8; klen];
            let nonce = vec![9u8; nlen];
            let mut buf = [0u8; 64];
            buf[..5].copy_from_slice(b"hello");
            let n = seal(alg, &key, &nonce, b"aad", &mut buf, 5).unwrap();
            assert_eq!(n, 21);
            let mut c = buf[..n].to_vec();
            assert_eq!(open(alg, &key, &nonce, b"aad", &mut c).unwrap(), 5);
            assert_eq!(&c[..5], b"hello");
            let mut bad = buf[..n].to_vec();
            bad[0] ^= 1;
            assert_eq!(open(alg, &key, &nonce, b"aad", &mut bad), Err(err::AUTH), "{alg}");
            assert_eq!(seal(alg, &key, &nonce, b"", &mut [0u8; 10], 5), Err(err::TOO_SMALL));
        }
        let key = hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let nonce = hex("404142434445464748494a4b4c4d4e4f5051525354555657");
        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let mut buf = vec![0u8; pt.len() + 16];
        buf[..pt.len()].copy_from_slice(pt);
        seal(aead::XCHACHA20_POLY1305, &key, &nonce, &aad, &mut buf, pt.len()).unwrap();
        assert_eq!(&buf[pt.len()..], hex("c0875924c1c7987947deafd8780acf49"));
    }

    #[test]
    fn stream_ciphers_and_ecb() {
        let key: Vec<u8> = (0..32).collect();
        let mut iv = vec![1, 0, 0, 0];
        iv.extend(hex("000000000000004a00000000"));
        let mut buf = b"Ladies and Gentlemen of the class of '99".to_vec();
        apply(cipher::CHACHA20, &key, &iv, &mut buf).unwrap();
        assert_eq!(&buf[..8], hex("6e2e359a2568f980"));
        let mut blk = hex("00112233445566778899aabbccddeeff");
        let k = hex("000102030405060708090a0b0c0d0e0f");
        apply(cipher::AES128_ECB, &k, &[], &mut blk).unwrap();
        assert_eq!(blk, hex("69c4e0d86a7b0430d8cdb78070b4c55a"));
        apply(cipher::AES128_ECB_DECRYPT, &k, &[], &mut blk).unwrap();
        assert_eq!(blk, hex("00112233445566778899aabbccddeeff"));
        assert_eq!(apply(cipher::AES128_ECB, &k, &[], &mut [0u8; 15]), Err(err::INVALID));
        let mut a = [0u8; 40];
        apply(cipher::AES128_CTR, &k, &[0u8; 16], &mut a).unwrap();
        apply(cipher::AES128_CTR, &k, &[0u8; 16], &mut a).unwrap();
        assert_eq!(a, [0u8; 40]);
    }

    #[test]
    fn x25519_matches_rfc7748_and_refuses_low_order_points() {
        let a = hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let mut pa = [0u8; 32];
        x25519(&a, None, &mut pa).unwrap();
        assert_eq!(pa.to_vec(), hex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"));
        let bp = hex("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f");
        let mut shared = [0u8; 32];
        x25519(&a, Some(&bp), &mut shared).unwrap();
        assert_eq!(shared.to_vec(), hex("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742"));
        assert_eq!(x25519(&a, Some(&[0u8; 32]), &mut shared), Err(err::AUTH));
    }
}
