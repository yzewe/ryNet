use ring::{aead, digest, hkdf, hmac};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HashAlg {
    Sha256,
    Sha384,
}

impl HashAlg {
    pub fn len(self) -> usize {
        match self {
            Self::Sha256 => 32,
            Self::Sha384 => 48,
        }
    }
    pub fn digest(self) -> &'static digest::Algorithm {
        match self {
            Self::Sha256 => &digest::SHA256,
            Self::Sha384 => &digest::SHA384,
        }
    }
    fn hkdf(self) -> hkdf::Algorithm {
        match self {
            Self::Sha256 => hkdf::HKDF_SHA256,
            Self::Sha384 => hkdf::HKDF_SHA384,
        }
    }
    fn hmac(self) -> hmac::Algorithm {
        match self {
            Self::Sha256 => hmac::HMAC_SHA256,
            Self::Sha384 => hmac::HMAC_SHA384,
        }
    }
    pub fn hash(self, data: &[u8]) -> Secret {
        Secret::from(digest::digest(self.digest(), data).as_ref())
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Suite {
    pub aead: &'static aead::Algorithm,
    pub hash: HashAlg,
}

pub(crate) fn suite_by_id(id: u16) -> Option<Suite> {
    Some(match id {
        0x1301 => Suite { aead: &aead::AES_128_GCM, hash: HashAlg::Sha256 },
        0x1302 => Suite { aead: &aead::AES_256_GCM, hash: HashAlg::Sha384 },
        0x1303 => Suite { aead: &aead::CHACHA20_POLY1305, hash: HashAlg::Sha256 },
        _ => return None,
    })
}

#[derive(Clone)]
pub(crate) struct Secret {
    buf: [u8; 80],
    len: usize,
}

impl Secret {
    pub fn zeros(len: usize) -> Self {
        Self { buf: [0; 80], len }
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl From<&[u8]> for Secret {
    fn from(s: &[u8]) -> Self {
        let mut buf = [0; 80];
        buf[..s.len()].copy_from_slice(s);
        Self { buf, len: s.len() }
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.buf.fill(0);
    }
}

struct Len(usize);
impl hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

pub(crate) fn extract(hash: HashAlg, salt: &[u8], ikm: &[u8]) -> Secret {
    let key = hmac::Key::new(hash.hmac(), salt);
    Secret::from(hmac::sign(&key, ikm).as_ref())
}

pub(crate) fn expand_label(hash: HashAlg, secret: &[u8], label: &[u8], context: &[u8], out: &mut [u8]) {
    let prk = hkdf::Prk::new_less_safe(hash.hkdf(), secret);
    let len = (out.len() as u16).to_be_bytes();
    let label_len = [(6 + label.len()) as u8];
    let ctx_len = [context.len() as u8];
    let info: [&[u8]; 6] = [&len, &label_len, b"tls13 ", label, &ctx_len, context];
    prk.expand(&info, Len(out.len())).and_then(|okm| okm.fill(out)).expect("hkdf output length is always valid here");
}

pub(crate) fn derive_secret(hash: HashAlg, secret: &[u8], label: &[u8], transcript_hash: &[u8]) -> Secret {
    let mut s = Secret::zeros(hash.len());
    expand_label(hash, secret, label, transcript_hash, &mut s.buf[..hash.len()]);
    s
}

pub(crate) fn finished_verify_data(hash: HashAlg, base_secret: &[u8], transcript_hash: &[u8]) -> Secret {
    let mut fk = Secret::zeros(hash.len());
    expand_label(hash, base_secret, b"finished", b"", &mut fk.buf[..hash.len()]);
    let key = hmac::Key::new(hash.hmac(), fk.as_slice());
    Secret::from(hmac::sign(&key, transcript_hash).as_ref())
}

pub(crate) struct Direction {
    suite: Suite,
    secret: Secret,
    key: aead::LessSafeKey,
    iv: [u8; 12],
    seq: u64,
    tls12: bool,
    cbc: Option<Box<Cbc>>,
}

pub(crate) struct Cbc {
    aes: CbcAes,
    mac: hmac::Key,
    mac_len: usize,
}

enum CbcAes {
    A128(Box<aes::Aes128>),
    A256(Box<aes::Aes256>),
}

impl CbcAes {
    fn encrypt(&self, b: &mut [u8]) {
        use aes::cipher::BlockCipherEncrypt;
        let b = <&mut aes::cipher::Array<u8, _>>::try_from(b).expect("16-byte block");
        match self {
            Self::A128(k) => k.encrypt_block(b),
            Self::A256(k) => k.encrypt_block(b),
        }
    }
    fn decrypt(&self, b: &mut [u8]) {
        use aes::cipher::BlockCipherDecrypt;
        let b = <&mut aes::cipher::Array<u8, _>>::try_from(b).expect("16-byte block");
        match self {
            Self::A128(k) => k.decrypt_block(b),
            Self::A256(k) => k.decrypt_block(b),
        }
    }
}

impl Direction {
    pub fn new(suite: Suite, secret: Secret) -> Self {
        let mut key = [0u8; 32];
        let klen = suite.aead.key_len();
        expand_label(suite.hash, secret.as_slice(), b"key", b"", &mut key[..klen]);
        let mut iv = [0u8; 12];
        expand_label(suite.hash, secret.as_slice(), b"iv", b"", &mut iv);
        let key = aead::LessSafeKey::new(aead::UnboundKey::new(suite.aead, &key[..klen]).expect("key length"));
        Self { suite, secret, key, iv, seq: 0, tls12: false, cbc: None }
    }

    pub fn new_tls12(suite: Suite, key: &[u8], iv: &[u8]) -> Self {
        let key = aead::LessSafeKey::new(aead::UnboundKey::new(suite.aead, key).expect("TLS 1.2 key length"));
        let mut nonce = [0; 12];
        nonce[..iv.len()].copy_from_slice(iv);
        Self { suite, secret: Secret::zeros(0), key, iv: nonce, seq: 0, tls12: true, cbc: None }
    }

    pub fn new_tls12_cbc(suite: Suite, enc_key: &[u8], mac: hmac::Algorithm, mac_key: &[u8]) -> Self {
        use aes::cipher::KeyInit;
        let aes = match enc_key.len() {
            16 => CbcAes::A128(Box::new(aes::Aes128::new_from_slice(enc_key).expect("AES-128 key"))),
            _ => CbcAes::A256(Box::new(aes::Aes256::new_from_slice(enc_key).expect("AES-256 key"))),
        };
        let mac_len = mac.digest_algorithm().output_len();
        let cbc = Cbc { aes, mac: hmac::Key::new(mac, mac_key), mac_len };
        let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_128_GCM, &[0; 16]).expect("key"));
        Self { suite, secret: Secret::zeros(0), key, iv: [0; 12], seq: 0, tls12: true, cbc: Some(Box::new(cbc)) }
    }

    pub fn is_tls12(&self) -> bool {
        self.tls12
    }
    pub fn is_cbc(&self) -> bool {
        self.cbc.is_some()
    }
    pub fn explicit_nonce_len(&self) -> usize {
        if self.cbc.is_some() {
            16
        } else if self.tls12 && self.suite.aead != &aead::CHACHA20_POLY1305 {
            8
        } else {
            0
        }
    }
    pub fn overhead(&self) -> usize {
        match &self.cbc {
            Some(c) => 16 + c.mac_len + 8,
            None => TAG_LEN + self.explicit_nonce_len() + usize::from(!self.tls12),
        }
    }

    pub fn go_payload(&self, mss: usize) -> usize {
        match &self.cbc {
            Some(c) => ((mss - 5 - 16) & !15) - 1 - c.mac_len,
            None => mss - 5 - self.overhead(),
        }
    }

    pub fn seal_cbc(&mut self, buf: &mut Vec<u8>, start: usize, ctype: u8) {
        use ring::rand::SecureRandom;
        let cbc = self.cbc.as_ref().expect("CBC direction");
        let pt = start + 5 + 16;
        let mut m = hmac::Context::with_key(&cbc.mac);
        m.update(&self.seq.to_be_bytes());
        m.update(&[ctype, 3, 3]);
        m.update(&((buf.len() - pt) as u16).to_be_bytes());
        m.update(&buf[pt..]);
        buf.extend_from_slice(m.sign().as_ref());
        let pad = 15 - (buf.len() - pt) % 16;
        buf.resize(buf.len() + pad + 1, pad as u8);
        let _ = ring::rand::SystemRandom::new().fill(&mut buf[start + 5..pt]);
        for i in (pt..buf.len()).step_by(16) {
            let (prev, block) = buf.split_at_mut(i);
            for (b, p) in block[..16].iter_mut().zip(&prev[i - 16..]) {
                *b ^= p;
            }
            cbc.aes.encrypt(&mut block[..16]);
        }
        let len = (buf.len() - start - 5) as u16;
        buf[start..start + 5].copy_from_slice(&[ctype, 3, 3, (len >> 8) as u8, len as u8]);
        self.seq += 1;
    }

    fn open_cbc(&mut self, header: &[u8; 5], in_out: &mut [u8]) -> Result<usize, ()> {
        let cbc = self.cbc.as_ref().expect("CBC direction");
        let ct = in_out.len().checked_sub(16).ok_or(())?;
        if ct == 0 || ct % 16 != 0 || ct < cbc.mac_len + 1 {
            return Err(());
        }
        let mut prev = [0u8; 16];
        prev.copy_from_slice(&in_out[..16]);
        for i in (16..in_out.len()).step_by(16) {
            let mut cur = [0u8; 16];
            cur.copy_from_slice(&in_out[i..i + 16]);
            cbc.aes.decrypt(&mut in_out[i..i + 16]);
            for (b, p) in in_out[i..i + 16].iter_mut().zip(&prev) {
                *b ^= p;
            }
            prev = cur;
        }
        let body = &in_out[16..];
        let pad = *body.last().unwrap() as usize;
        let mut good = pad + 1 + cbc.mac_len <= body.len();
        let pad_len = if good { pad + 1 } else { 1 };
        let scan = body.len().min(256);
        let mut diff = 0u8;
        for (i, &b) in body[body.len() - scan..].iter().rev().enumerate() {
            let inside = ((i < pad_len) as u8).wrapping_neg();
            diff |= (b ^ pad as u8) & inside;
        }
        good &= diff == 0;
        let n = body.len() - pad_len - cbc.mac_len;
        let mut m = hmac::Context::with_key(&cbc.mac);
        m.update(&self.seq.to_be_bytes());
        m.update(&header[..3]);
        m.update(&(n as u16).to_be_bytes());
        m.update(&body[..n]);
        let expected = m.sign();
        let alg = cbc.mac.algorithm().digest_algorithm();
        let block = alg.block_len();
        let tail = if block > 64 { 17 } else { 9 };
        let blocks = |data: usize| (13 + data + tail).div_ceil(block);
        let spare = blocks(body.len() - cbc.mac_len - 1) - blocks(n);
        let mut idle = ring::digest::Context::new(alg);
        idle.update(&[0u8; 128 * 4][..(spare * block).min(128 * 4)]);
        let _ = idle.finish();
        let mac_ok = expected.as_ref().iter().zip(&body[n..n + cbc.mac_len]).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
        self.seq += 1;
        if good && mac_ok { Ok(n) } else { Err(()) }
    }

    fn tls12_aad(&self, header: &[u8; 5], pt_len: usize) -> [u8; 13] {
        let mut aad = [0; 13];
        aad[..8].copy_from_slice(&self.seq.to_be_bytes());
        aad[8..11].copy_from_slice(&header[..3]);
        aad[11..].copy_from_slice(&(pt_len as u16).to_be_bytes());
        aad
    }

    fn next_nonce(&mut self) -> aead::Nonce {
        let mut n = self.iv;
        for (a, b) in n[4..].iter_mut().zip(self.seq.to_be_bytes()) {
            *a ^= b;
        }
        self.seq += 1;
        aead::Nonce::assume_unique_for_key(n)
    }

    pub fn seal(&mut self, header: &[u8; 5], in_out: &mut [u8]) -> aead::Tag {
        if self.tls12 {
            let explicit = self.explicit_nonce_len();
            let aad = self.tls12_aad(header, in_out.len() - explicit);
            let nonce = if explicit > 0 {
                in_out[..8].copy_from_slice(&self.seq.to_be_bytes());
                let mut value = self.iv;
                value[4..].copy_from_slice(&in_out[..8]);
                self.seq += 1;
                aead::Nonce::assume_unique_for_key(value)
            } else {
                self.next_nonce()
            };
            return self
                .key
                .seal_in_place_separate_tag(nonce, aead::Aad::from(&aad), &mut in_out[explicit..])
                .expect("in-range TLS 1.2 record");
        }
        let nonce = self.next_nonce();
        self.key.seal_in_place_separate_tag(nonce, aead::Aad::from(header), in_out).expect("seal cannot fail for in-range record sizes")
    }

    pub fn open(&mut self, header: &[u8; 5], in_out: &mut [u8]) -> Result<usize, ()> {
        if self.cbc.is_some() {
            return self.open_cbc(header, in_out);
        }
        if self.tls12 {
            let explicit = self.explicit_nonce_len();
            if in_out.len() < explicit + TAG_LEN {
                return Err(());
            }
            let aad = self.tls12_aad(header, in_out.len() - explicit - TAG_LEN);
            let nonce = if explicit > 0 {
                let mut value = self.iv;
                value[4..].copy_from_slice(&in_out[..8]);
                self.seq += 1;
                aead::Nonce::assume_unique_for_key(value)
            } else {
                self.next_nonce()
            };
            return self.key.open_in_place(nonce, aead::Aad::from(&aad), &mut in_out[explicit..]).map(|pt| pt.len()).map_err(|_| ());
        }
        let nonce = self.next_nonce();
        self.key.open_in_place(nonce, aead::Aad::from(header), in_out).map(|pt| pt.len()).map_err(|_| ())
    }

    pub fn update(&mut self) {
        let h = self.suite.hash;
        let next = derive_secret_raw(h, self.secret.as_slice(), b"traffic upd");
        *self = Direction::new(self.suite, next);
    }
}

fn derive_secret_raw(hash: HashAlg, secret: &[u8], label: &[u8]) -> Secret {
    let mut s = Secret::zeros(hash.len());
    expand_label(hash, secret, label, b"", &mut s.buf[..hash.len()]);
    s
}

pub(crate) const TAG_LEN: usize = 16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls12_cbc_roundtrip_and_tamper_detection() {
        let suite = Suite { aead: &aead::AES_128_GCM, hash: HashAlg::Sha256 };
        for (key_len, mac) in
            [(16, hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY), (32, hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY), (16, hmac::HMAC_SHA256)]
        {
            let (k, m) = (vec![7u8; key_len], vec![9u8; 32]);
            let go = Direction::new_tls12_cbc(suite, &k, mac, &m[..mac.digest_algorithm().output_len()]).go_payload(1208);
            assert_eq!(go, if mac == hmac::HMAC_SHA256 { 1151 } else { 1163 });
            for n in [0usize, 1, 15, 16, 17, 1000] {
                let mut tx = Direction::new_tls12_cbc(suite, &k, mac, &m[..mac.digest_algorithm().output_len()]);
                let mut rx = Direction::new_tls12_cbc(suite, &k, mac, &m[..mac.digest_algorithm().output_len()]);
                let data: Vec<u8> = (0..n).map(|i| i as u8).collect();
                let mut buf = vec![0u8; 5 + 16];
                buf.extend_from_slice(&data);
                tx.seal_cbc(&mut buf, 0, 23);
                let header: [u8; 5] = buf[..5].try_into().unwrap();
                assert_eq!(u16::from_be_bytes([header[3], header[4]]) as usize, buf.len() - 5);
                assert_eq!((buf.len() - 5 - 16) % 16, 0);
                let mut tampered = buf.clone();
                let last = tampered.len() - 20;
                tampered[last] ^= 1;
                let mut rx2 = Direction::new_tls12_cbc(suite, &k, mac, &m[..mac.digest_algorithm().output_len()]);
                assert!(rx2.open(&header, &mut tampered[5..]).is_err(), "tampered {n}");
                let ciphertext = buf.clone();
                let got = rx.open(&header, &mut buf[5..]).expect("open");
                assert_eq!(&buf[5 + 16..5 + 16 + got], &data[..]);
                let mut again = ciphertext;
                assert!(rx.open(&header, &mut again[5..]).is_err());
            }
        }
    }

    #[test]
    fn tls12_aead_authenticates_payload_type_version_and_sequence() {
        for id in [0x1301, 0x1302, 0x1303] {
            let suite = suite_by_id(id).unwrap();
            let key = vec![7; suite.aead.key_len()];
            let iv = vec![9; if id == 0x1303 { 12 } else { 4 }];
            let mut tx = Direction::new_tls12(suite, &key, &iv);
            let explicit = tx.explicit_nonce_len();
            let data = b"authenticated TLS 1.2 application bytes";
            let mut body = vec![0; explicit];
            body.extend_from_slice(data);
            let n = (body.len() + TAG_LEN) as u16;
            let header = [23, 3, 3, (n >> 8) as u8, n as u8];
            let tag = tx.seal(&header, &mut body);
            body.extend_from_slice(tag.as_ref());
            let mut rx = Direction::new_tls12(suite, &key, &iv);
            let mut valid = body.clone();
            assert_eq!(rx.open(&header, &mut valid).unwrap(), data.len());
            assert_eq!(&valid[explicit..explicit + data.len()], data);
            assert!(rx.open(&header, &mut body.clone()).is_err(), "replayed sequence");
            for index in [explicit, body.len() - 1] {
                let mut bad = body.clone();
                bad[index] ^= 1;
                assert!(Direction::new_tls12(suite, &key, &iv).open(&header, &mut bad).is_err());
            }
            for index in [0, 2] {
                let mut bad = header;
                bad[index] ^= 1;
                assert!(Direction::new_tls12(suite, &key, &iv).open(&bad, &mut body.clone()).is_err());
            }
        }
    }

    #[test]
    fn rfc8448_key_schedule() {
        let h = HashAlg::Sha256;
        let early = extract(h, &[], &[0u8; 32]);
        assert_eq!(hex(early.as_slice()), "33ad0a1c607ec03b09e6cd9893680ce210adf300aa1f2660e1b22e10f170f92a");
        let empty = h.hash(b"");
        let derived = derive_secret(h, early.as_slice(), b"derived", empty.as_slice());
        assert_eq!(hex(derived.as_slice()), "6f2615a108c702c5678f54fc9dbab69716c076189c48250cebeac3576c3611ba");
        let ecdhe = unhex("8bd4054fb55b9d63fdfbacf9f04b9f0d35e6d63f537563efd46272900f89492d");
        let hs = extract(h, derived.as_slice(), &ecdhe);
        assert_eq!(hex(hs.as_slice()), "1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac");
        let th = unhex("860c06edc07858ee8e78f0e7428c58edd6b43f2ca3e6e95f02ed063cf0e1cad8");
        let c_hs = derive_secret(h, hs.as_slice(), b"c hs traffic", &th);
        assert_eq!(hex(c_hs.as_slice()), "b3eddb126e067f35a780b3abf45e2d8f3b1a950738f52e9600746a0e27a55a21");
        let s_hs = derive_secret(h, hs.as_slice(), b"s hs traffic", &th);
        assert_eq!(hex(s_hs.as_slice()), "b67b7d690cc16c4e75e54213cb2d37b4e9c912bcded9105d42befd59d391ad38");
        let mut key = [0u8; 16];
        expand_label(h, s_hs.as_slice(), b"key", b"", &mut key);
        assert_eq!(hex(&key), "3fce516009c21727d0f2e4e86ee403bc");
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
}
