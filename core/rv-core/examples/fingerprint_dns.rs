use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let proto = args.get(1).ok_or("protocol")?.clone();
    let port: u16 = args.get(2).ok_or("port")?.parse()?;
    let cert = std::fs::read_to_string(args.get(3).ok_or("test CA")?)?;
    let b64: String = cert.lines().filter(|l| !l.starts_with("---")).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(b64)?;
    rv_tls::set_roots(&[der]);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    rt.block_on(tokio::task::LocalSet::new().run_until(async move {
        let mut io = match proto.as_str() {
            "doh" => {
                let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
                rv_core::dns::doh(Box::new(tcp), "127.0.0.1", port, "/dns-query").await?
            }
            "doq" => rv_core::dns::doq("127.0.0.1", port, None, false).await?,
            _ => return Err(std::io::Error::other("only doh/doq")),
        };
        let q = [0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 2, b'f', b'p', 4, b't', b'e', b's', b't', 0, 0, 1, 0, 1];
        io.write_all(&(q.len() as u16).to_be_bytes()).await?;
        io.write_all(&q).await?;
        io.flush().await?;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), io.read_u16()).await;
        Ok::<_, std::io::Error>(())
    }))?;
    Ok(())
}
