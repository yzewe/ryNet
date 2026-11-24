use super::trojan::{fits_socks_addr, put_addr};
use crate::{BoxStream, Error, UDP_IDLE, transport::Stream};
use base64::Engine;
use rv_config::{Outbound, OutboundKind, Profile, ProxyServer, Security, SocksVersion};
use rv_net::Protect;
use rv_vless::Address;
use std::{
    io,
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
};

pub struct BasicProxy {
    pub stream: Stream,
    server: ProxyServer,
    version: Option<SocksVersion>,
    protect: Option<Protect>,
    family: rv_config::Family,
}

fn invalid(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}

impl BasicProxy {
    pub fn new(profile: &Profile, ob: &Outbound, protect: Option<Protect>) -> Result<Self, Error> {
        let (server, version) = match &ob.kind {
            OutboundKind::Socks(s) => (s.server.clone(), Some(s.version)),
            OutboundKind::Http(h) => (h.server.clone(), None),
            _ => return Err(Error::Config("not a basic proxy".into())),
        };
        let mut ob = ob.clone();
        if version.is_none()
            && let Security::Tls(t) = &mut ob.stream.security
        {
            if t.alpn.is_empty() {
                t.alpn = vec!["http/1.1".into()];
            } else if t.alpn != ["http/1.1"] {
                return Err(Error::Unsupported("HTTP proxy requires HTTP/1.1 ALPN".into()));
            }
        }
        let stream = Stream::new(profile, &ob, &server.address, server.port, protect.clone())?;
        Ok(Self { stream, server, version, protect, family: ob.stream.sockopt.family })
    }

    pub fn udp_allowed(&self) -> bool {
        self.version == Some(SocksVersion::V5) && !self.stream.dialer.chained()
    }

    pub async fn connect(&self, dest: Address, port: u16) -> Result<BoxStream, Error> {
        let work = async {
            let mut stream = self.stream.open().await?;
            match self.version {
                Some(SocksVersion::V5) => {
                    socks5(&mut stream, self.server.auth.as_ref(), 1, &dest, port).await?;
                }
                Some(v) => {
                    socks4(&mut stream, self.server.auth.as_ref(), v, &dest, port, self.protect.as_ref()).await?;
                }
                None => {
                    return http_connect(stream, self.server.auth.as_ref(), &dest, port).await.map_err(Error::from);
                }
            }
            Ok(stream)
        };
        tokio::time::timeout(Duration::from_secs(10), work)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "proxy handshake timed out"))?
    }

    pub async fn udp_session(
        &self,
        first: (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        if !self.udp_allowed() {
            return Err(Error::Unsupported("UDP requires unchained SOCKS5".into()));
        }
        let (mut control, bound, port) = tokio::time::timeout(Duration::from_secs(10), async {
            let mut control = self.stream.open().await?;
            let (bound, port) = socks5(&mut control, self.server.auth.as_ref(), 3, &Address::Ipv4([0; 4]), 0).await?;
            Ok::<_, Error>((control, bound, port))
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "SOCKS UDP handshake timed out"))??;
        if port == 0 {
            return Err(invalid("SOCKS UDP relay has no port").into());
        }
        let host = match bound {
            Address::Ipv4(a) if a != [0; 4] => IpAddr::from(a).to_string(),
            Address::Ipv6(a) if a != [0; 16] => IpAddr::from(a).to_string(),
            Address::Domain(d) => d,
            _ => self.server.address.clone(),
        };
        let ips = rv_net::resolve_protected(&host, self.protect.as_ref()).await?;
        let ip = super::dial::udp_ip(&ips, self.family).ok_or_else(|| invalid("SOCKS UDP relay has no address"))?;
        let socket = rv_net::udp_socket(ip.is_ipv6(), self.protect.as_ref())?;
        socket.connect(SocketAddr::new(ip, port)).await?;
        if let Some(packet) = udp_packet(first.0, &first.1) {
            socket.send(&packet).await?;
        }
        let mut buffer = vec![0u8; 65536];
        let mut closed = [0u8; 1];
        let idle = tokio::time::sleep(UDP_IDLE);
        tokio::pin!(idle);
        let mut last = tokio::time::Instant::now();
        loop {
            tokio::select! {
                packet = rx.recv() => {
                    let Some((dest,data)) = packet else { return Ok(()) };
                    let Some(packet) = udp_packet(dest,&data) else { continue };
                    socket.send(&packet).await?;
                    crate::udp_touch(&mut last,idle.as_mut(),UDP_IDLE);
                }
                n = socket.recv(&mut buffer) => {
                    let n = n?;
                    if let Some((addr,port,data)) = udp_decode(&buffer[..n]) {
                        let ip = match addr {
                            Address::Ipv4(a) => Some(IpAddr::from(a)),
                            Address::Ipv6(a) => Some(IpAddr::from(a)),
                            Address::Domain(d) => rv_net::resolve_protected(&d,self.protect.as_ref()).await.ok().and_then(|ips| super::dial::udp_ip(&ips,self.family)),
                        };
                        if let Some(ip) = ip { reply(SocketAddr::new(ip,port),data); }
                        crate::udp_touch(&mut last,idle.as_mut(),UDP_IDLE);
                    }
                }
                _ = control.read(&mut closed) => return Ok(()),
                _ = &mut idle => return Ok(()),
            }
        }
    }
}

async fn read_addr(stream: &mut BoxStream) -> io::Result<(Address, u16)> {
    let addr = match stream.read_u8().await? {
        1 => {
            let mut a = [0; 4];
            stream.read_exact(&mut a).await?;
            Address::Ipv4(a)
        }
        4 => {
            let mut a = [0; 16];
            stream.read_exact(&mut a).await?;
            Address::Ipv6(a)
        }
        3 => {
            let n = stream.read_u8().await? as usize;
            if n == 0 {
                return Err(invalid("empty SOCKS address"));
            }
            let mut a = vec![0; n];
            stream.read_exact(&mut a).await?;
            Address::Domain(String::from_utf8(a).map_err(|_| invalid("invalid SOCKS address"))?)
        }
        _ => return Err(invalid("invalid SOCKS address type")),
    };
    Ok((addr, stream.read_u16().await?))
}

async fn socks5(
    stream: &mut BoxStream,
    auth: Option<&(String, String)>,
    command: u8,
    dest: &Address,
    port: u16,
) -> io::Result<(Address, u16)> {
    fits_socks_addr(dest)?;
    let method = if auth.is_some() { 2 } else { 0 };
    stream.write_all(&[5, 1, method]).await?;
    stream.flush().await?;
    let mut response = [0; 2];
    stream.read_exact(&mut response).await?;
    if response != [5, method] {
        return Err(invalid("SOCKS5 authentication method rejected"));
    }
    if let Some((user, pass)) = auth {
        let mut request = vec![1, user.len() as u8];
        request.extend_from_slice(user.as_bytes());
        request.push(pass.len() as u8);
        request.extend_from_slice(pass.as_bytes());
        stream.write_all(&request).await?;
        stream.flush().await?;
        stream.read_exact(&mut response).await?;
        if response != [1, 0] {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "SOCKS5 authentication rejected"));
        }
    }
    let mut request = vec![5, command, 0];
    put_addr(&mut request, dest, port);
    stream.write_all(&request).await?;
    stream.flush().await?;
    let mut response = [0; 3];
    stream.read_exact(&mut response).await?;
    if response[0] != 5 || response[2] != 0 || response[1] != 0 {
        return Err(invalid(&format!("SOCKS5 connect rejected ({})", response[1])));
    }
    read_addr(stream).await
}

async fn socks4(
    stream: &mut BoxStream,
    auth: Option<&(String, String)>,
    version: SocksVersion,
    dest: &Address,
    port: u16,
    protect: Option<&Protect>,
) -> io::Result<()> {
    fits_socks_addr(dest)?;
    let mut domain = None;
    let ip = match dest {
        Address::Ipv4(a) => *a,
        Address::Ipv6(_) => {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "SOCKS4 does not support IPv6"));
        }
        Address::Domain(d) if version == SocksVersion::V4a => {
            if d.contains('\0') {
                return Err(invalid("NUL in SOCKS4a domain"));
            }
            domain = Some(d);
            [0, 0, 0, 1]
        }
        Address::Domain(d) => rv_net::resolve_protected(d, protect)
            .await?
            .into_iter()
            .find_map(|a| if let IpAddr::V4(v) = a { Some(v.octets()) } else { None })
            .ok_or_else(|| invalid("SOCKS4 requires IPv4"))?,
    };
    let mut request = vec![4, 1];
    request.extend_from_slice(&port.to_be_bytes());
    request.extend_from_slice(&ip);
    if let Some((user, _)) = auth {
        request.extend_from_slice(user.as_bytes());
    }
    request.push(0);
    if let Some(d) = domain {
        request.extend_from_slice(d.as_bytes());
        request.push(0);
    }
    stream.write_all(&request).await?;
    stream.flush().await?;
    let mut response = [0; 8];
    stream.read_exact(&mut response).await?;
    if response[0] != 0 || response[1] != 90 {
        return Err(invalid("SOCKS4 connect rejected"));
    }
    Ok(())
}

async fn http_connect(mut stream: BoxStream, auth: Option<&(String, String)>, dest: &Address, port: u16) -> io::Result<BoxStream> {
    let host = match dest {
        Address::Ipv4(a) => IpAddr::from(*a).to_string(),
        Address::Ipv6(a) => format!("[{}]", IpAddr::from(*a)),
        Address::Domain(d) => {
            if d.is_empty() || d.len() > 255 || d.bytes().any(|b| b <= 32 || b >= 127 || b"/\\?#@[]:".contains(&b)) {
                return Err(invalid("invalid CONNECT host"));
            }
            d.clone()
        }
    };
    let target = format!("{host}:{port}");
    let mut headers = Vec::new();
    if let Some((u, p)) = auth {
        headers.push((
            "Proxy-Authorization".into(),
            format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"))),
        ));
    }
    rv_xhttp::browser::apply_defaults(&mut headers, rv_xhttp::browser::Variant::Nav);
    headers.push(("Proxy-Connection".into(), "Keep-Alive".into()));
    let request = rv_xhttp::browser::http1_request("CONNECT", &target, &target, &headers);
    stream.write_all(&request).await?;
    stream.flush().await?;
    let mut reader = BufReader::new(stream);
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= 16384 {
            return Err(invalid("CONNECT response headers too large"));
        }
        header.push(reader.read_u8().await?);
    }
    let line = header.split(|b| *b == b'\n').next().unwrap_or_default();
    let mut fields = line.split(|b| *b == b' ');
    if !matches!(fields.next(), Some(b"HTTP/1.0" | b"HTTP/1.1")) || fields.next() != Some(b"200") {
        return Err(invalid("HTTP CONNECT rejected"));
    }
    Ok(Box::new(reader))
}

fn udp_packet(dest: SocketAddr, data: &[u8]) -> Option<Vec<u8>> {
    let overhead = if dest.is_ipv4() { 10 } else { 22 };
    if data.len() > 65507 - overhead {
        return None;
    }
    let mut out = vec![0, 0, 0];
    let addr = match dest.ip() {
        IpAddr::V4(a) => Address::Ipv4(a.octets()),
        IpAddr::V6(a) => Address::Ipv6(a.octets()),
    };
    put_addr(&mut out, &addr, dest.port());
    out.extend_from_slice(data);
    Some(out)
}
fn udp_decode(data: &[u8]) -> Option<(Address, u16, &[u8])> {
    if data.get(..3)? != [0, 0, 0] {
        return None;
    }
    let (addr, end) = match *data.get(3)? {
        1 => (Address::Ipv4(data.get(4..8)?.try_into().ok()?), 8),
        4 => (Address::Ipv6(data.get(4..20)?.try_into().ok()?), 20),
        3 => {
            let n = *data.get(4)? as usize;
            if n == 0 {
                return None;
            }
            (Address::Domain(std::str::from_utf8(data.get(5..5 + n)?).ok()?.into()), 5 + n)
        }
        _ => return None,
    };
    let port = u16::from_be_bytes(data.get(end..end + 2)?.try_into().ok()?);
    Some((addr, port, data.get(end + 2..)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn request(s: &mut tokio::io::DuplexStream) -> Vec<u8> {
        let mut out = Vec::new();
        while !out.ends_with(b"\r\n\r\n") {
            out.push(s.read_u8().await.unwrap());
        }
        out
    }

    #[tokio::test]
    async fn connect_headers_auth_ipv6_and_server_first_bytes() {
        let (client, mut server) = tokio::io::duplex(65536);
        let peer = async {
            let req = String::from_utf8(request(&mut server).await).unwrap();
            assert!(req.starts_with("CONNECT [2001:db8::1]:443 HTTP/1.1\r\nHost: [2001:db8::1]:443\r\nUser-Agent: Mozilla/5.0"));
            assert!(req.contains("Proxy-Authorization: Basic dXNlcjpwYXNz\r\n"));
            assert!(req.contains("Proxy-Connection: Keep-Alive\r\n"));
            assert!(req.contains("Sec-Fetch-Mode: navigate\r\n"));
            server.write_all(b"HTTP/1.1 200 Connection established\r\nX-Server: test\r\n\r\nfirst").await.unwrap();
            let mut b = [0; 4];
            server.read_exact(&mut b).await.unwrap();
            assert_eq!(&b, b"ping");
        };
        let user = async {
            let mut stream = http_connect(
                Box::new(client),
                Some(&("user".into(), "pass".into())),
                &Address::Ipv6("2001:db8::1".parse::<std::net::Ipv6Addr>().unwrap().octets()),
                443,
            )
            .await
            .unwrap();
            let mut b = [0; 5];
            stream.read_exact(&mut b).await.unwrap();
            assert_eq!(&b, b"first");
            stream.write_all(b"ping").await.unwrap();
            stream.flush().await.unwrap();
        };
        tokio::join!(peer, user);
    }

    #[tokio::test]
    async fn connect_rejects_status_truncation_oversized_headers_and_injection() {
        for response in [
            b"HTTP/1.1 407 Authentication required\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\n".to_vec(),
            [b"HTTP/1.1 200 OK\r\nX: ".as_slice(), &vec![b'x'; 16384], b"\r\n\r\n"].concat(),
        ] {
            let (client, mut server) = tokio::io::duplex(65536);
            let peer = async move {
                request(&mut server).await;
                let _ = server.write_all(&response).await;
                drop(server);
            };
            let user = async {
                assert!(http_connect(Box::new(client), None, &Address::Domain("target.example".into()), 80).await.is_err());
            };
            tokio::join!(peer, user);
        }
        let (client, _) = tokio::io::duplex(64);
        assert!(http_connect(Box::new(client), None, &Address::Domain("x\r\nX:injected".into()), 80).await.is_err());
    }

    #[tokio::test]
    async fn socks5_auth_exact_wire_and_reply_validation() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut client: BoxStream = Box::new(client);
        let peer = async {
            let mut b = [0; 3];
            server.read_exact(&mut b).await.unwrap();
            assert_eq!(b, [5, 1, 2]);
            server.write_all(&[5, 2]).await.unwrap();
            let mut b = [0; 5];
            server.read_exact(&mut b).await.unwrap();
            assert_eq!(&b, &[1, 1, b'u', 1, b'p']);
            server.write_all(&[1, 0]).await.unwrap();
            let mut b = [0; 10];
            server.read_exact(&mut b).await.unwrap();
            assert_eq!(&b, &[5, 1, 0, 1, 1, 2, 3, 4, 0, 80]);
            server.write_all(&[5, 0, 0, 4]).await.unwrap();
            server.write_all(&[0; 16]).await.unwrap();
            server.write_all(&[0, 1]).await.unwrap();
        };
        let user = async {
            let (_, p) = socks5(&mut client, Some(&("u".into(), "p".into())), 1, &Address::Ipv4([1, 2, 3, 4]), 80).await.unwrap();
            assert_eq!(p, 1);
        };
        tokio::join!(peer, user);
        for response in [[5, 255], [4, 0], [5, 2]] {
            let (client, mut server) = tokio::io::duplex(64);
            let mut client: BoxStream = Box::new(client);
            let peer = async {
                let mut b = [0; 3];
                server.read_exact(&mut b).await.unwrap();
                server.write_all(&response).await.unwrap();
            };
            let user = async {
                assert!(socks5(&mut client, None, 1, &Address::Ipv4([1; 4]), 80).await.is_err());
            };
            tokio::join!(peer, user);
        }
    }

    #[tokio::test]
    async fn socks4a_remote_domain_wire_and_ipv6_refusal() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut client: BoxStream = Box::new(client);
        let peer = async {
            let mut b = [0; 23];
            server.read_exact(&mut b).await.unwrap();
            assert_eq!(&b, b"\x04\x01\x00\x50\x00\x00\x00\x01id\0remote.test\0");
            server.write_all(&[0, 90, 0, 80, 127, 0, 0, 1]).await.unwrap();
        };
        let user = async {
            socks4(&mut client, Some(&("id".into(), String::new())), SocksVersion::V4a, &Address::Domain("remote.test".into()), 80, None)
                .await
                .unwrap();
        };
        tokio::join!(peer, user);
        assert!(socks4(&mut client, None, SocksVersion::V4, &Address::Ipv6([0; 16]), 80, None).await.is_err());
    }

    #[test]
    fn udp_codec_ipv4_ipv6_fragments_and_truncation() {
        for dest in ["1.2.3.4:53".parse::<SocketAddr>().unwrap(), "[2001:db8::2]:123".parse().unwrap()] {
            let p = udp_packet(dest, b"payload").unwrap();
            assert!(udp_packet(dest, &vec![0; 65507]).is_none());
            let (_, port, data) = udp_decode(&p).unwrap();
            assert_eq!(port, dest.port());
            assert_eq!(data, b"payload");
            for n in 0..p.len() - 7 {
                assert!(udp_decode(&p[..n]).is_none());
            }
            let mut p = p;
            p[2] = 1;
            assert!(udp_decode(&p).is_none());
        }
    }
}
