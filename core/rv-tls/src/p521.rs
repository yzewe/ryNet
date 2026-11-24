use p521::ecdsa::signature::hazmat::PrehashVerifier;
use ring::digest;
use rustls_pki_types::{AlgorithmIdentifier, InvalidSignature, SignatureVerificationAlgorithm, alg_id};

pub struct Ephemeral {
    secret: p521::SecretKey,
    pub public: Vec<u8>,
}

impl Ephemeral {
    pub fn generate() -> Option<Self> {
        use ring::rand::SecureRandom;
        let rng = ring::rand::SystemRandom::new();
        for _ in 0..64 {
            let mut bytes = [0u8; 66];
            rng.fill(&mut bytes).ok()?;
            bytes[0] &= 1;
            if let Ok(secret) = p521::SecretKey::from_slice(&bytes) {
                bytes.fill(0);
                let public = secret.public_key().to_sec1_bytes().into_vec();
                return Some(Self { secret, public });
            }
        }
        None
    }

    pub fn agree(self, peer: &[u8]) -> Option<Vec<u8>> {
        if peer.first() != Some(&4) {
            return None;
        }
        let peer = p521::PublicKey::from_sec1_bytes(peer).ok()?;
        let shared = p521::elliptic_curve::ecdh::diffie_hellman(self.secret.to_nonzero_scalar(), peer.as_affine());
        Some(shared.raw_secret_bytes().to_vec())
    }
}

pub fn verify(key: &[u8], hash: &'static digest::Algorithm, message: &[u8], sig: &[u8]) -> bool {
    let (Ok(key), Ok(sig)) = (p521::ecdsa::VerifyingKey::from_sec1_bytes(key), p521::ecdsa::DerSignature::try_from(sig)) else {
        return false;
    };
    key.verify_prehash(digest::digest(hash, message).as_ref(), &sig).is_ok()
}

#[derive(Debug)]
struct EcdsaP521(&'static digest::Algorithm, AlgorithmIdentifier);

impl SignatureVerificationAlgorithm for EcdsaP521 {
    fn verify_signature(&self, public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), InvalidSignature> {
        verify(public_key, self.0, message, signature).then_some(()).ok_or(InvalidSignature)
    }
    fn public_key_alg_id(&self) -> AlgorithmIdentifier {
        alg_id::ECDSA_P521
    }
    fn signature_alg_id(&self) -> AlgorithmIdentifier {
        self.1
    }
}

pub static ECDSA_P521_SHA512: &dyn SignatureVerificationAlgorithm = &EcdsaP521(&digest::SHA512, alg_id::ECDSA_SHA512);
pub static ECDSA_P521_SHA384: &dyn SignatureVerificationAlgorithm = &EcdsaP521(&digest::SHA384, alg_id::ECDSA_SHA384);
pub static ECDSA_P521_SHA256: &dyn SignatureVerificationAlgorithm = &EcdsaP521(&digest::SHA256, alg_id::ECDSA_SHA256);

pub static ALL_VERIFICATION_ALGS: &[&dyn SignatureVerificationAlgorithm] = &[
    webpki::ring::ECDSA_P256_SHA256,
    webpki::ring::ECDSA_P256_SHA384,
    webpki::ring::ECDSA_P384_SHA256,
    webpki::ring::ECDSA_P384_SHA384,
    ECDSA_P521_SHA256,
    ECDSA_P521_SHA384,
    ECDSA_P521_SHA512,
    webpki::ring::ED25519,
    webpki::ring::RSA_PKCS1_2048_8192_SHA256,
    webpki::ring::RSA_PKCS1_2048_8192_SHA384,
    webpki::ring::RSA_PKCS1_2048_8192_SHA512,
    webpki::ring::RSA_PKCS1_2048_8192_SHA256_ABSENT_PARAMS,
    webpki::ring::RSA_PKCS1_2048_8192_SHA384_ABSENT_PARAMS,
    webpki::ring::RSA_PKCS1_2048_8192_SHA512_ABSENT_PARAMS,
    webpki::ring::RSA_PKCS1_3072_8192_SHA384,
    webpki::ring::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
    webpki::ring::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
    webpki::ring::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecdh_agrees_and_rejects_bad_points() {
        let (a, b) = (Ephemeral::generate().unwrap(), Ephemeral::generate().unwrap());
        assert_eq!(a.public.len(), 133);
        let (pa, pb) = (a.public.clone(), b.public.clone());
        let s1 = a.agree(&pb).unwrap();
        assert_eq!(s1.len(), 66);
        assert_eq!(s1, b.agree(&pa).unwrap());
        assert!(Ephemeral::generate().unwrap().agree(&[0; 133]).is_none());
        let mut off = pa.clone();
        off[100] ^= 1;
        assert!(Ephemeral::generate().unwrap().agree(&off).is_none());
    }

    #[test]
    fn ecdsa_verifies_all_hashes() {
        use p521::ecdsa::signature::hazmat::PrehashSigner;
        let sk = p521::ecdsa::SigningKey::from_slice(
            &[7; 66][..].iter().enumerate().map(|(i, b)| if i == 0 { 1 } else { *b }).collect::<Vec<_>>(),
        )
        .unwrap();
        let pk = p521::ecdsa::VerifyingKey::from(&sk).to_sec1_bytes();
        for (hash, alg) in
            [(&digest::SHA512, ECDSA_P521_SHA512), (&digest::SHA384, ECDSA_P521_SHA384), (&digest::SHA256, ECDSA_P521_SHA256)]
        {
            let sig: p521::ecdsa::Signature = sk.sign_prehash(digest::digest(hash, b"hello").as_ref()).unwrap();
            let der = sig.to_der();
            assert!(alg.verify_signature(&pk, b"hello", der.as_bytes()).is_ok());
            assert!(alg.verify_signature(&pk, b"hellp", der.as_bytes()).is_err());
        }
    }
}
