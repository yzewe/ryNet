mod balance;
pub mod describe;
pub mod dns;
pub mod engine;
pub mod explain;
pub mod geo;
pub mod happ;
pub mod lang;
pub mod link;
pub mod outbound;
pub mod ping;
pub mod pipe;
#[cfg(feature = "plugins")]
pub mod plugin;
#[cfg(not(feature = "plugins"))]
#[path = "plugin_off.rs"]
pub mod plugin;
pub mod portroute;
pub mod procroute;
mod quic;
pub mod sniff;
pub mod speedtest;
mod stage;
pub mod transport;
pub mod tune;

pub use outbound::Proxy;

use std::io;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub use rv_vless::Address as Destination;

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin {}
impl<T: AsyncRead + AsyncWrite + Unpin> AsyncStream for T {}

pub type BoxStream = Box<dyn AsyncStream>;

fn roots() -> rustls::RootCertStore {
    rustls::RootCertStore { roots: rv_tls::roots().to_vec() }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("tls: {0}")]
    Tls(#[from] rv_tls::Error),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("config: {0}")]
    Config(String),
}

pub async fn relay<L>(mut local: L, mut remote: BoxStream) -> io::Result<(u64, u64)>
where
    L: pipe::Local,
{
    let mut first = vec![0u8; 16 * 1024];
    let mut up = 0u64;
    match tokio::time::timeout(Duration::from_millis(500), local.read(&mut first)).await {
        Ok(Ok(0)) => return Ok((0, 0)),
        Ok(Ok(n)) => {
            remote.write_all(&first[..n]).await?;
            up += n as u64;
        }
        Ok(Err(e)) => return Err(e),
        Err(_) => {}
    }
    remote.flush().await?;
    drop(first);
    let r = pipe::copy(&mut local, &mut remote, pipe::CONN_IDLE).await;
    match r.error {
        Some(e) => Err(e),
        None => Ok((up + r.up, r.down)),
    }
}

pub const UDP_IDLE: Duration = Duration::from_secs(60);

pub(crate) fn udp_touch(last: &mut tokio::time::Instant, idle: std::pin::Pin<&mut tokio::time::Sleep>, limit: Duration) {
    let now = tokio::time::Instant::now();
    if now - *last >= Duration::from_secs(1) {
        idle.reset(now + limit);
    }
    *last = now;
}

pub(crate) async fn udp_write<S: tokio::io::AsyncWrite + Unpin + ?Sized>(s: &mut S, data: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    tokio::time::timeout(UDP_WRITE, async {
        s.write_all(data).await?;
        s.flush().await
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "udp: server does not read"))?
}
const UDP_WRITE: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) const UDP_BATCH: usize = 15 * 1024;

pub(crate) fn udp_drain(
    rx: &mut tokio::sync::mpsc::Receiver<(std::net::SocketAddr, Vec<u8>)>,
    w: &mut Vec<u8>,
    mut put: impl FnMut(&mut Vec<u8>, std::net::SocketAddr, &[u8]),
) {
    while w.len() < UDP_BATCH {
        match rx.try_recv() {
            Ok((dst, data)) => put(w, dst, &data),
            Err(_) => break,
        }
    }
}

pub(crate) async fn udp_setup<T, E: From<io::Error>>(f: impl std::future::Future<Output = Result<T, E>>) -> Result<T, E> {
    tokio::time::timeout(Duration::from_secs(20), f)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "udp: proxy setup timeout"))?
}

thread_local! {
    static DATAGRAM: std::cell::Cell<Option<Box<[u8]>>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn with_datagram_buf<R>(f: impl FnOnce(&mut [u8]) -> R) -> R {
    let mut buf = DATAGRAM.take().unwrap_or_else(|| vec![0u8; 64 * 1024].into_boxed_slice());
    let r = f(&mut buf);
    DATAGRAM.set(Some(buf));
    r
}
