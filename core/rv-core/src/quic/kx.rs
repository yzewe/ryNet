use ml_kem::{DecapsulationKey768, KeyExport, kem::Decapsulate};
use ring::rand::{SecureRandom, SystemRandom};
use rustls::crypto::{ActiveKeyExchange, SharedSecret, SupportedKxGroup};
use rustls::pki_types::{AlgorithmIdentifier, InvalidSignature, SignatureVerificationAlgorithm, alg_id};
use rustls::{Error, NamedGroup, ProtocolVersion};
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Debug)]
pub struct Hybrid;
pub static HYBRID: Hybrid = Hybrid;

pub(crate) fn provider() -> rustls::crypto::CryptoProvider {
    use rustls::SignatureScheme as S;
    use rustls::crypto::{WebPkiSupportedAlgorithms, ring};
    use webpki::ring as a;
    static SIGNATURES: &[(S, &[&dyn SignatureVerificationAlgorithm])] = &[
        (S::RSA_PSS_SHA256, &[a::RSA_PSS_2048_8192_SHA256_LEGACY_KEY]),
        (S::ECDSA_NISTP256_SHA256, &[a::ECDSA_P256_SHA256]),
        (S::ED25519, &[a::ED25519]),
        (S::RSA_PSS_SHA384, &[a::RSA_PSS_2048_8192_SHA384_LEGACY_KEY]),
        (S::RSA_PSS_SHA512, &[a::RSA_PSS_2048_8192_SHA512_LEGACY_KEY]),
        (S::ECDSA_NISTP384_SHA384, &[a::ECDSA_P384_SHA384]),
        (S::ECDSA_NISTP521_SHA512, &[rv_tls::p521::ECDSA_P521_SHA512]),
    ];
    static ALL: &[&dyn SignatureVerificationAlgorithm] = &[
        a::ECDSA_P256_SHA256,
        a::ECDSA_P256_SHA384,
        a::ECDSA_P384_SHA256,
        a::ECDSA_P384_SHA384,
        rv_tls::p521::ECDSA_P521_SHA256,
        rv_tls::p521::ECDSA_P521_SHA384,
        rv_tls::p521::ECDSA_P521_SHA512,
        a::ED25519,
        ML_DSA_44,
        ML_DSA_65,
        ML_DSA_87,
        a::RSA_PKCS1_2048_8192_SHA256,
        a::RSA_PKCS1_2048_8192_SHA384,
        a::RSA_PKCS1_2048_8192_SHA512,
        a::RSA_PKCS1_2048_8192_SHA256_ABSENT_PARAMS,
        a::RSA_PKCS1_2048_8192_SHA384_ABSENT_PARAMS,
        a::RSA_PKCS1_2048_8192_SHA512_ABSENT_PARAMS,
        a::RSA_PKCS1_3072_8192_SHA384,
        a::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
        a::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
        a::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
    ];
    let mut p = ring::default_provider();
    p.signature_verification_algorithms = WebPkiSupportedAlgorithms { all: ALL, mapping: SIGNATURES };
    p.kx_groups =
        vec![&HYBRID, &P256_HYBRID, &P384_HYBRID, ring::kx_group::X25519, ring::kx_group::SECP256R1, ring::kx_group::SECP384R1, &P521];
    p
}

struct MlDsa<P>(AlgorithmIdentifier, std::marker::PhantomData<fn() -> P>);
impl<P> std::fmt::Debug for MlDsa<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ML-DSA")
    }
}
impl<P: ml_dsa::MlDsaParams> SignatureVerificationAlgorithm for MlDsa<P> {
    fn verify_signature(&self, key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), InvalidSignature> {
        use ml_dsa::{KeyInit, Verifier};
        let key = ml_dsa::VerifyingKey::<P>::new_from_slice(key).map_err(|_| InvalidSignature)?;
        let signature = ml_dsa::Signature::<P>::try_from(signature).map_err(|_| InvalidSignature)?;
        key.verify(message, &signature).map_err(|_| InvalidSignature)
    }
    fn public_key_alg_id(&self) -> AlgorithmIdentifier {
        self.0
    }
    fn signature_alg_id(&self) -> AlgorithmIdentifier {
        self.0
    }
}
static ML_DSA_44: &dyn SignatureVerificationAlgorithm = &MlDsa::<ml_dsa::MlDsa44>(alg_id::ML_DSA_44, std::marker::PhantomData);
static ML_DSA_65: &dyn SignatureVerificationAlgorithm = &MlDsa::<ml_dsa::MlDsa65>(alg_id::ML_DSA_65, std::marker::PhantomData);
static ML_DSA_87: &dyn SignatureVerificationAlgorithm = &MlDsa::<ml_dsa::MlDsa87>(alg_id::ML_DSA_87, std::marker::PhantomData);

#[derive(Debug)]
struct P521Group;
static P521: P521Group = P521Group;
struct P521Exchange {
    key: rv_tls::p521::Ephemeral,
    public: Vec<u8>,
}
impl SupportedKxGroup for P521Group {
    fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, Error> {
        let key = rv_tls::p521::Ephemeral::generate().ok_or(Error::FailedToGetRandomBytes)?;
        let public = key.public.clone();
        Ok(Box::new(P521Exchange { key, public }))
    }
    fn name(&self) -> NamedGroup {
        NamedGroup::secp521r1
    }
    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }
}
impl ActiveKeyExchange for P521Exchange {
    fn complete(self: Box<Self>, peer: &[u8]) -> Result<SharedSecret, Error> {
        let secret = self.key.agree(peer).ok_or_else(|| Error::General("P521 agreement".into()))?;
        Ok(SharedSecret::from(secret.as_slice()))
    }
    fn pub_key(&self) -> &[u8] {
        &self.public
    }
    fn group(&self) -> NamedGroup {
        NamedGroup::secp521r1
    }
    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }
}

#[derive(Debug)]
struct P256Hybrid;
static P256_HYBRID: P256Hybrid = P256Hybrid;
struct P256Exchange {
    ec: Box<dyn ActiveKeyExchange>,
    kem: DecapsulationKey768,
    public: Vec<u8>,
}
impl SupportedKxGroup for P256Hybrid {
    fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, Error> {
        let ec = rustls::crypto::ring::kx_group::SECP256R1.start()?;
        let mut seed = [0; 64];
        SystemRandom::new().fill(&mut seed).map_err(|_| Error::FailedToGetRandomBytes)?;
        let kem = DecapsulationKey768::from_seed(seed.into());
        let mut public = ec.pub_key().to_vec();
        public.extend_from_slice(&kem.encapsulation_key().to_bytes());
        Ok(Box::new(P256Exchange { ec, kem, public }))
    }
    fn name(&self) -> NamedGroup {
        NamedGroup::from(0x11eb)
    }
    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }
    fn usable_for_version(&self, v: ProtocolVersion) -> bool {
        v == ProtocolVersion::TLSv1_3
    }
}
impl ActiveKeyExchange for P256Exchange {
    fn complete(self: Box<Self>, peer: &[u8]) -> Result<SharedSecret, Error> {
        if peer.len() != 65 + 1088 {
            return Err(Error::General("P256+MLKEM768 share length".into()));
        }
        let mut secret = self.ec.complete(&peer[..65])?.secret_bytes().to_vec();
        let ct = ml_kem::ml_kem_768::Ciphertext::try_from(&peer[65..]).map_err(|_| Error::General("MLKEM768 ciphertext".into()))?;
        secret.extend_from_slice(&self.kem.decapsulate(&ct));
        Ok(SharedSecret::from(secret))
    }
    fn pub_key(&self) -> &[u8] {
        &self.public
    }
    fn group(&self) -> NamedGroup {
        NamedGroup::from(0x11eb)
    }
    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }
}

#[derive(Debug)]
struct P384Hybrid;
static P384_HYBRID: P384Hybrid = P384Hybrid;
struct P384Exchange {
    ec: Box<dyn ActiveKeyExchange>,
    kem: ml_kem::DecapsulationKey1024,
    public: Vec<u8>,
}
impl SupportedKxGroup for P384Hybrid {
    fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, Error> {
        let ec = rustls::crypto::ring::kx_group::SECP384R1.start()?;
        let mut seed = [0; 64];
        SystemRandom::new().fill(&mut seed).map_err(|_| Error::FailedToGetRandomBytes)?;
        let kem = ml_kem::DecapsulationKey1024::from_seed(seed.into());
        let mut public = ec.pub_key().to_vec();
        public.extend_from_slice(&kem.encapsulation_key().to_bytes());
        Ok(Box::new(P384Exchange { ec, kem, public }))
    }
    fn name(&self) -> NamedGroup {
        NamedGroup::from(0x11ed)
    }
    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }
    fn usable_for_version(&self, v: ProtocolVersion) -> bool {
        v == ProtocolVersion::TLSv1_3
    }
}
impl ActiveKeyExchange for P384Exchange {
    fn complete(self: Box<Self>, peer: &[u8]) -> Result<SharedSecret, Error> {
        if peer.len() != 97 + 1568 {
            return Err(Error::General("P384+MLKEM1024 share length".into()));
        }
        let mut secret = self.ec.complete(&peer[..97])?.secret_bytes().to_vec();
        let ct = ml_kem::ml_kem_1024::Ciphertext::try_from(&peer[97..]).map_err(|_| Error::General("MLKEM1024 ciphertext".into()))?;
        secret.extend_from_slice(&self.kem.decapsulate(&ct));
        Ok(SharedSecret::from(secret))
    }
    fn pub_key(&self) -> &[u8] {
        &self.public
    }
    fn group(&self) -> NamedGroup {
        NamedGroup::from(0x11ed)
    }
    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }
}

struct Exchange {
    x: StaticSecret,
    kem: DecapsulationKey768,
    public: Vec<u8>,
}

impl SupportedKxGroup for Hybrid {
    fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, Error> {
        let mut seed = [0; 64];
        let mut xseed = [0; 32];
        let rng = SystemRandom::new();
        rng.fill(&mut seed).map_err(|_| Error::FailedToGetRandomBytes)?;
        rng.fill(&mut xseed).map_err(|_| Error::FailedToGetRandomBytes)?;
        let x = StaticSecret::from(xseed);
        let kem = DecapsulationKey768::from_seed(seed.into());
        let mut public = kem.encapsulation_key().to_bytes().to_vec();
        public.extend_from_slice(PublicKey::from(&x).as_bytes());
        Ok(Box::new(Exchange { x, kem, public }))
    }
    fn name(&self) -> NamedGroup {
        NamedGroup::X25519MLKEM768
    }
    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }
    fn usable_for_version(&self, v: ProtocolVersion) -> bool {
        v == ProtocolVersion::TLSv1_3
    }
}

impl Exchange {
    fn classical(&self, peer: &[u8]) -> Result<[u8; 32], Error> {
        let peer: [u8; 32] = peer.try_into().map_err(|_| Error::General("bad X25519 share".into()))?;
        let ss = self.x.diffie_hellman(&PublicKey::from(peer));
        if !ss.was_contributory() {
            return Err(Error::General("X25519 low-order point".into()));
        }
        Ok(ss.to_bytes())
    }
}

impl ActiveKeyExchange for Exchange {
    fn complete(self: Box<Self>, peer: &[u8]) -> Result<SharedSecret, Error> {
        if peer.len() != 1120 {
            return Err(Error::General("bad X25519MLKEM768 share".into()));
        }
        let ct = ml_kem::ml_kem_768::Ciphertext::try_from(&peer[..1088]).map_err(|_| Error::General("bad ML-KEM ciphertext".into()))?;
        let mut secret = self.kem.decapsulate(&ct).to_vec();
        secret.extend_from_slice(&self.classical(&peer[1088..])?);
        Ok(SharedSecret::from(secret.as_slice()))
    }
    fn hybrid_component(&self) -> Option<(NamedGroup, &[u8])> {
        Some((NamedGroup::X25519, &self.public[1184..]))
    }
    fn complete_hybrid_component(self: Box<Self>, peer: &[u8]) -> Result<SharedSecret, Error> {
        Ok(SharedSecret::from(self.classical(peer)?.as_slice()))
    }
    fn pub_key(&self) -> &[u8] {
        &self.public
    }
    fn group(&self) -> NamedGroup {
        NamedGroup::X25519MLKEM768
    }
    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ml_kem::TryKeyInit;
    #[test]
    fn hybrid_secret_matches_independent_server() {
        let client = HYBRID.start().unwrap();
        let pk = client.pub_key();
        assert_eq!(pk.len(), 1216);
        let ek = ml_kem::EncapsulationKey768::new_from_slice(&pk[..1184]).unwrap();
        let mut randomness = [0; 32];
        SystemRandom::new().fill(&mut randomness).unwrap();
        let (ct, kem_secret) = ek.encapsulate_deterministic(&randomness.into());
        let server = StaticSecret::from([91; 32]);
        let peer = PublicKey::from(<[u8; 32]>::try_from(&pk[1184..]).unwrap());
        let x_secret = server.diffie_hellman(&peer);
        let mut response = ct.to_vec();
        response.extend_from_slice(PublicKey::from(&server).as_bytes());
        let actual = client.complete(&response).unwrap();
        let mut expected = kem_secret.to_vec();
        expected.extend_from_slice(x_secret.as_bytes());
        assert_eq!(actual.secret_bytes(), expected);
    }
    #[test]
    fn rejects_zero_classical_share() {
        let client = HYBRID.start().unwrap();
        assert!(client.complete_hybrid_component(&[0; 32]).is_err());
    }

    #[test]
    fn p384_hybrid_matches_independent_kem_and_ecdh() {
        use ring::agreement as a;
        let client = P384_HYBRID.start().unwrap();
        let pk = client.pub_key();
        assert_eq!(pk.len(), 97 + 1568);
        let ek = ml_kem::EncapsulationKey1024::new_from_slice(&pk[97..]).unwrap();
        let (ct, kem_secret) = ek.encapsulate_deterministic(&[19; 32].into());
        let server = a::EphemeralPrivateKey::generate(&a::ECDH_P384, &SystemRandom::new()).unwrap();
        let public = server.compute_public_key().unwrap().as_ref().to_vec();
        let peer = a::UnparsedPublicKey::new(&a::ECDH_P384, &pk[..97]);
        let mut expected = a::agree_ephemeral(server, &peer, |s| s.to_vec()).unwrap();
        expected.extend_from_slice(&kem_secret);
        let mut reply = public;
        reply.extend_from_slice(&ct);
        assert_eq!(client.complete(&reply).unwrap().secret_bytes(), expected);
    }

    #[test]
    fn p256_hybrid_matches_independent_kem_and_ecdh() {
        use ring::agreement as a;
        let client = P256_HYBRID.start().unwrap();
        let pk = client.pub_key();
        assert_eq!(pk.len(), 65 + 1184);
        let ek = ml_kem::EncapsulationKey768::new_from_slice(&pk[65..]).unwrap();
        let (ct, kem_secret) = ek.encapsulate_deterministic(&[23; 32].into());
        let server = a::EphemeralPrivateKey::generate(&a::ECDH_P256, &SystemRandom::new()).unwrap();
        let public = server.compute_public_key().unwrap().as_ref().to_vec();
        let peer = a::UnparsedPublicKey::new(&a::ECDH_P256, &pk[..65]);
        let mut expected = a::agree_ephemeral(server, &peer, |s| s.to_vec()).unwrap();
        expected.extend_from_slice(&kem_secret);
        let mut reply = public;
        reply.extend_from_slice(&ct);
        assert_eq!(client.complete(&reply).unwrap().secret_bytes(), expected);
        assert!(P256_HYBRID.start().unwrap().complete(&[0; 65 + 1088]).is_err());
    }

    #[test]
    fn p521_matches_independent_ecdh_and_rejects_invalid_points() {
        let client = P521.start().unwrap();
        let server = rv_tls::p521::Ephemeral::generate().unwrap();
        let public = server.public.clone();
        let expected = server.agree(client.pub_key()).unwrap();
        assert_eq!(client.complete(&public).unwrap().secret_bytes(), expected);
        assert!(P521.start().unwrap().complete(&[0; 133]).is_err());
        assert!(P384_HYBRID.start().unwrap().complete(&[0; 1665]).is_err());
    }

    #[test]
    fn ml_dsa_verifies_and_rejects() {
        use ml_dsa::{Keypair, Signer};
        let key = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&[5; 32].into());
        let sig = key.sign(b"hello");
        let pk = key.verifying_key().encode();
        assert!(ML_DSA_65.verify_signature(&pk, b"hello", &sig.encode()).is_ok());
        assert!(ML_DSA_65.verify_signature(&pk, b"hellp", &sig.encode()).is_err());
        assert!(ML_DSA_44.verify_signature(&pk, b"hello", &sig.encode()).is_err());
    }
}
