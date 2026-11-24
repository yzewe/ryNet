mod camouflage;
mod der;
mod handshake;
mod hello;
mod keysched;
pub mod p521;
mod record;

#[cfg(fuzzing)]
pub mod fuzzing {
    pub use crate::der::fuzzing as der;
    pub use crate::handshake::fuzzing as handshake;
    pub use crate::record::fuzzing as record;
}

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fingerprint {
    Chrome,
    Firefox,
    Safari,
    Ios,
    Edge,
    Qq,
    Chrome120,
    Chrome131,
    Firefox120,
    Ios13,
    Edge106,
    Qihoo11,
    Android,
    Qihoo75,
    Golang,
    UtlsGolang,
    Randomized,
    RandomizedNoAlpn,
    HelloRandomized,
    HelloRandomizedAlpn,
    HelloRandomizedNoAlpn,
    Firefox55,
    Firefox56,
    Firefox63,
    Firefox65,
    Firefox99,
    Firefox102,
    Firefox105,
    Chrome58,
    Chrome62,
    Chrome70,
    Chrome72,
    Chrome83,
    Chrome87,
    Chrome96,
    Chrome100,
    Chrome102,
    Chrome106,
    Ios11,
    Ios12,
    Safari16,
    Chrome100Psk,
    Chrome112Psk,
    Chrome114Psk,
    Chrome115Pq,
    Chrome115PqPsk,
    Chrome120Pq,
}

impl Fingerprint {
    const ALL: [Fingerprint; 11] = [
        Self::Firefox120,
        Self::Chrome120,
        Self::Chrome131,
        Self::Chrome,
        Self::Firefox,
        Self::Safari,
        Self::Ios,
        Self::Ios13,
        Self::Edge106,
        Self::Qihoo11,
        Self::Qq,
    ];

    pub fn from_xray(name: &str) -> Option<Self> {
        let n = name.to_ascii_lowercase();
        let family = match n.as_str() {
            "hellochrome_auto" | "hellochrome_133" => "chrome",
            "hellofirefox_auto" | "hellofirefox_148" => "firefox",
            "hellosafari_auto" | "hellosafari_26_3" => "safari",
            "helloios_auto" | "helloios_14" => "ios",
            "helloedge_auto" | "helloedge_85" => "edge",
            "helloqq_auto" | "helloqq_11_1" => "qq",
            _ => &n,
        };
        Some(match family {
            "" | "chrome" => Self::Chrome,
            "firefox" => Self::Firefox,
            "safari" => Self::Safari,
            "ios" => Self::Ios,
            "edge" => Self::Edge,
            "qq" => Self::Qq,
            "hellochrome_120" => Self::Chrome120,
            "hellochrome_131" => Self::Chrome131,
            "hellofirefox_120" => Self::Firefox120,
            "helloios_13" => Self::Ios13,
            "helloedge_106" => Self::Edge106,
            "hello360_11_0" => Self::Qihoo11,
            "android" => Self::Android,
            "helloandroid_11_okhttp" => Self::Android,
            "360" => Self::Qihoo75,
            "hello360_auto" => Self::Qihoo75,
            "hello360_7_5" => Self::Qihoo75,
            "unsafe" => Self::Golang,
            "hellogolang" => Self::UtlsGolang,
            "randomized" => Self::Randomized,
            "randomizednoalpn" => Self::RandomizedNoAlpn,
            "hellorandomized" => Self::HelloRandomized,
            "hellorandomizedalpn" => Self::HelloRandomizedAlpn,
            "hellorandomizednoalpn" => Self::HelloRandomizedNoAlpn,
            "helloios_11_1" => Self::Ios11,
            "helloios_12_1" => Self::Ios12,
            "hellosafari_16_0" => Self::Safari16,
            "hellochrome_106_shuffle" => Self::Chrome106,
            "hellochrome_100_psk" => Self::Chrome100Psk,
            "hellochrome_112_psk_shuf" => Self::Chrome112Psk,
            "hellochrome_114_padding_psk_shuf" => Self::Chrome114Psk,
            "hellochrome_115_pq" => Self::Chrome115Pq,
            "hellochrome_115_pq_psk" => Self::Chrome115PqPsk,
            "hellochrome_120_pq" => Self::Chrome120Pq,
            "hellofirefox_55" => Self::Firefox55,
            "hellofirefox_56" => Self::Firefox56,
            "hellofirefox_63" => Self::Firefox63,
            "hellofirefox_65" => Self::Firefox65,
            "hellofirefox_99" => Self::Firefox99,
            "hellofirefox_102" => Self::Firefox102,
            "hellofirefox_105" => Self::Firefox105,
            "hellochrome_58" => Self::Chrome58,
            "hellochrome_62" => Self::Chrome62,
            "hellochrome_70" => Self::Chrome70,
            "hellochrome_72" => Self::Chrome72,
            "hellochrome_83" => Self::Chrome83,
            "hellochrome_87" => Self::Chrome87,
            "hellochrome_96" => Self::Chrome96,
            "hellochrome_100" => Self::Chrome100,
            "hellochrome_102" => Self::Chrome102,
            "random" => Self::random(),
            _ => return None,
        })
    }

    pub fn uses_configured_alpn(self) -> bool {
        matches!(
            self,
            Self::Golang
                | Self::UtlsGolang
                | Self::Randomized
                | Self::RandomizedNoAlpn
                | Self::HelloRandomized
                | Self::HelloRandomizedAlpn
                | Self::HelloRandomizedNoAlpn
        )
    }

    pub(crate) fn empty_psk(self) -> bool {
        matches!(self, Self::Chrome100Psk | Self::Chrome112Psk | Self::Chrome114Psk | Self::Chrome115PqPsk)
    }

    fn random() -> Self {
        static PICK: std::sync::OnceLock<Fingerprint> = std::sync::OnceLock::new();
        *PICK.get_or_init(|| {
            let mut b = [0u8; 1];
            loop {
                if ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut b).is_err() {
                    return Self::Chrome;
                }
                if (b[0] as usize) < 256 - 256 % Self::ALL.len() {
                    return Self::ALL[b[0] as usize % Self::ALL.len()];
                }
            }
        })
    }
}

#[derive(Clone, Debug)]
pub struct RealityConfig {
    pub public_key: [u8; 32],
    pub mldsa65_verify: Vec<u8>,
    pub short_id: [u8; 8],
    pub spider_x: String,
}

#[derive(Clone, Debug)]
pub enum Verify {
    WebPki,
    Reality(RealityConfig),
    InsecureSkipVerify,
    Pinned(Pins),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pins {
    pub sha256: Vec<[u8; 32]>,
    pub names: Vec<String>,
}

impl Pins {
    pub fn is_empty(&self) -> bool {
        self.sha256.is_empty() && self.names.is_empty()
    }
}

pub use handshake::verify_pinned;

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub server_name: String,
    pub alpn: Vec<Vec<u8>>,
    pub alpn_override: bool,
    pub fingerprint: Fingerprint,
    pub verify: Verify,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("tls alert from server: {0}")]
    Alert(u8),
    #[error("protocol violation: {0}")]
    Protocol(&'static str),
    #[error("protocol violation: {0}")]
    Record(String),
    #[error("server negotiated unsupported parameters: {0}")]
    Unsupported(&'static str),
    #[error("tls: CurvePreferences includes unsupported curve (uTLS HelloRetryRequest)")]
    UtlsHrrGroup,
    #[error("tls: empty psk detected (Xray supplies no session cache for this template)")]
    EmptyPsk,
    #[error("tls: {0}")]
    Config(&'static str),
    #[error("certificate verification failed: {0}")]
    BadCertificate(String),
    #[error("REALITY authentication failed (server is not our xray, or wrong keys)")]
    RealityAuth,
    #[error("REALITY ML-DSA-65 verification failed: {0}")]
    RealityMldsa65(&'static str),
}

pub type Background = fn(Pin<Box<dyn std::future::Future<Output = ()>>>);
pub fn set_background(spawn: Background) {
    let _ = camouflage::BACKGROUND.set(spawn);
}

pub async fn connect<S>(io: S, config: Arc<ClientConfig>) -> Result<TlsStream<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    connect_with_roots(io, config, &roots()).await
}

pub type Roots = Arc<[rustls_pki_types::TrustAnchor<'static>]>;

static ROOTS: std::sync::Mutex<Option<Roots>> = std::sync::Mutex::new(None);

pub fn set_roots(ders: &[Vec<u8>]) -> usize {
    let roots: Vec<_> = ders
        .iter()
        .filter_map(|d| {
            let der = rustls_pki_types::CertificateDer::from(d.as_slice());
            webpki::anchor_from_trusted_cert(&der).ok().map(|a| a.to_owned())
        })
        .collect();
    let n = roots.len();
    if n > 0 {
        *ROOTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(roots.into());
    }
    n
}

pub fn roots() -> Roots {
    let mut g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = g.as_ref() {
        return r.clone();
    }
    let found = load_roots();
    if found.is_empty() {
        tracing::warn!("no CA certificates");
    } else {
        *g = Some(found.clone());
    }
    found
}

#[cfg(target_os = "android")]
fn load_roots() -> Roots {
    use rustls_pki_types::CertificateDer;
    use rustls_pki_types::pem::PemObject;
    for dir in ["/apex/com.android.conscrypt/cacerts", "/system/etc/security/cacerts"] {
        let Ok(files) = std::fs::read_dir(dir) else { continue };
        let roots: Vec<_> = files
            .flatten()
            .filter_map(|f| CertificateDer::from_pem_file(f.path()).ok())
            .filter_map(|c| webpki::anchor_from_trusted_cert(&c).ok().map(|a| a.to_owned()))
            .collect();
        if !roots.is_empty() {
            return roots.into();
        }
    }
    Vec::new().into()
}

#[cfg(not(target_os = "android"))]
fn load_roots() -> Roots {
    webpki_roots::TLS_SERVER_ROOTS.to_vec().into()
}

#[doc(hidden)]
pub async fn connect_with_roots<S>(
    io: S,
    config: Arc<ClientConfig>,
    roots: &[rustls_pki_types::TrustAnchor<'_>],
) -> Result<TlsStream<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let mut conn = record::Conn::new(io);
    let outcome = match handshake::run(&mut conn, &config, roots).await {
        Ok(outcome) => outcome,
        Err(error) => {
            let code = match &error {
                Error::BadCertificate(_) => Some(42),
                Error::RealityAuth | Error::RealityMldsa65(_) => Some(51),
                Error::Unsupported(_) => Some(70),
                Error::UtlsHrrGroup => Some(80),
                Error::Protocol(msg) if msg.contains("expected") => Some(10),
                Error::Protocol(_) => Some(47),
                Error::Record(_) => Some(20),
                Error::Alert(_) | Error::Io(_) | Error::EmptyPsk | Error::Config(_) => None,
            };
            if let Some(code) = code.filter(|_| conn.hello_sent) {
                conn.queue(record::CT_ALERT, 0x0303, &[2, code]);
                let _ = tokio::time::timeout(std::time::Duration::from_millis(250), conn.flush_out()).await;
            }
            return Err(error);
        }
    };
    let tls = TlsStream { conn, alpn: outcome.alpn };
    if matches!(config.verify, Verify::Reality(_)) && !outcome.reality_authenticated {
        camouflage::start(tls, config).await;
        return Err(Error::RealityAuth);
    }
    Ok(tls)
}

pub struct TlsStream<S> {
    conn: record::Conn<S>,
    alpn: Option<Vec<u8>>,
}

impl<S> TlsStream<S> {
    pub fn alpn(&self) -> Option<&[u8]> {
        self.alpn.as_deref()
    }

    pub fn get_ref(&self) -> &S {
        &self.conn.io
    }
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.conn.io
    }

    pub fn switch_read_to_direct(&mut self) -> Vec<u8> {
        self.conn.switch_read_to_direct()
    }

    pub fn switch_write_to_direct(&mut self) {
        self.conn.switch_write_to_direct()
    }

    pub fn is_read_direct(&self) -> bool {
        self.conn.is_read_direct()
    }
    pub fn is_write_direct(&self) -> bool {
        self.conn.is_write_direct()
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for TlsStream<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        self.get_mut().conn.poll_read_app(cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for TlsStream<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.get_mut().conn.poll_write_app(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().conn.poll_flush_app(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().conn.poll_shutdown_app(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn roots_verify_real_server() {
        assert!(roots().len() > 50, "{} roots", roots().len());
        let host = std::env::var("RV_TLS_HOST").unwrap_or_else(|_| "www.google.com".into());
        let tcp = tokio::net::TcpStream::connect((host.as_str(), 443)).await.unwrap();
        let cfg = ClientConfig {
            server_name: host,
            alpn: Vec::new(),
            alpn_override: true,
            fingerprint: Fingerprint::Chrome,
            verify: Verify::WebPki,
        };
        connect(tcp, Arc::new(cfg)).await.unwrap();
    }
}
