#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sniffed {
    pub protocol: Option<&'static str>,
    pub domain: Option<String>,
}

pub fn sniff(b: &[u8]) -> Sniffed {
    if let Some(d) = tls_sni(b) {
        return Sniffed { protocol: Some("tls"), domain: d };
    }
    if let Some(d) = http_host(b) {
        return Sniffed { protocol: Some("http"), domain: d };
    }
    if b.starts_with(b"\x13BitTorrent protocol") {
        return Sniffed { protocol: Some("bittorrent"), domain: None };
    }
    Sniffed::default()
}

pub fn need_more(b: &[u8]) -> bool {
    if b.len() >= 5 && b[0] == 0x16 && b[1] == 3 {
        let rec = u16::from_be_bytes([b[3], b[4]]) as usize;
        return rec <= 16384 && b.len() < 5 + rec;
    }
    const METHODS: [&[u8]; 4] = [b"GET ", b"POST ", b"HEAD ", b"PUT "];
    METHODS.iter().any(|m| b.starts_with(m)) && b.len() < 4096 && !b.windows(4).any(|w| w == b"\r\n\r\n")
}

fn tls_sni(b: &[u8]) -> Option<Option<String>> {
    if b.len() < 6 || b[0] != 0x16 || b[1] != 3 || b[5] != 1 {
        return None;
    }
    Some(client_hello_sni(b))
}

fn client_hello_sni(b: &[u8]) -> Option<String> {
    if b.len() < 5 + 4 {
        return None;
    }
    let mut r = &b[5 + 4..];
    let skip = |r: &mut &[u8], n: usize| -> Option<()> {
        if r.len() < n {
            return None;
        }
        *r = &r[n..];
        Some(())
    };
    let u8v = |r: &&[u8]| r.first().copied().map(usize::from);
    let u16v = |r: &&[u8]| (r.len() >= 2).then(|| u16::from_be_bytes([r[0], r[1]]) as usize);
    skip(&mut r, 2 + 32)?;
    let n = u8v(&r)?;
    skip(&mut r, 1 + n)?;
    let n = u16v(&r)?;
    skip(&mut r, 2 + n)?;
    let n = u8v(&r)?;
    skip(&mut r, 1 + n)?;
    let ext_len = u16v(&r)?;
    skip(&mut r, 2)?;
    let mut e = &r[..ext_len.min(r.len())];
    while e.len() >= 4 {
        let t = u16::from_be_bytes([e[0], e[1]]);
        let l = u16::from_be_bytes([e[2], e[3]]) as usize;
        e = &e[4..];
        if e.len() < l {
            break;
        }
        if t == 0 {
            let s = &e[..l];
            if s.len() >= 5 && s[2] == 0 {
                let nl = u16::from_be_bytes([s[3], s[4]]) as usize;
                if let Some(name) = s.get(5..5 + nl)
                    && let Ok(name) = std::str::from_utf8(name)
                    && valid_domain(name)
                {
                    return Some(name.to_ascii_lowercase());
                }
            }
            return None;
        }
        e = &e[l..];
    }
    None
}

fn http_host(b: &[u8]) -> Option<Option<String>> {
    const METHODS: [&[u8]; 9] = [b"GET ", b"POST ", b"HEAD ", b"PUT ", b"DELETE ", b"OPTIONS ", b"PATCH ", b"CONNECT ", b"TRACE "];
    if !METHODS.iter().any(|m| b.starts_with(m)) {
        return None;
    }
    let head = &b[..b.len().min(4096)];
    let head = &head[..head.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1)];
    for line in head.split(|&c| c == b'\n').skip(1) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break;
        }
        if line.len() > 5 && line[..5].eq_ignore_ascii_case(b"host:") {
            let v = std::str::from_utf8(&line[5..]).ok()?.trim();
            let host = if v.starts_with('[') {
                return Some(None);
            } else {
                v.rsplit_once(':').map_or(v, |(h, _)| h)
            };
            return Some(valid_domain(host).then(|| host.to_ascii_lowercase()));
        }
    }
    Some(None)
}

fn valid_domain(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.parse::<std::net::IpAddr>().is_err()
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.' || c == b'_')
}

#[cfg(kani)]
#[path = "proofs/sniff.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/sniff.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    #[test]
    fn http_host_needs_whole_line() {
        assert_eq!(super::http_host(b"GET / HTTP/1.1\r\nHost: exam"), Some(None));
        assert_eq!(super::http_host(b"GET / HTTP/1.1\r\nHost: example.com\r\n"), Some(Some("example.com".into())));
    }

    use super::*;

    fn client_hello(sni: &str) -> Vec<u8> {
        let mut ext = Vec::new();
        let name = sni.as_bytes();
        let mut sn = vec![0u8];
        sn.extend_from_slice(&(name.len() as u16).to_be_bytes());
        sn.extend_from_slice(name);
        let mut list = (sn.len() as u16).to_be_bytes().to_vec();
        list.extend(sn);
        ext.extend_from_slice(&[0, 23, 0, 0]);
        ext.extend_from_slice(&[0, 0]);
        ext.extend_from_slice(&(list.len() as u16).to_be_bytes());
        ext.extend(list);
        let mut body = vec![3, 3];
        body.extend([7u8; 32]);
        body.push(32);
        body.extend([1u8; 32]);
        body.extend([0, 2, 0x13, 0x01, 1, 0]);
        body.extend((ext.len() as u16).to_be_bytes());
        body.extend(ext);
        let mut hs = vec![1, 0, (body.len() >> 8) as u8, body.len() as u8];
        hs.extend(body);
        let mut rec = vec![0x16, 3, 1, (hs.len() >> 8) as u8, hs.len() as u8];
        rec.extend(hs);
        rec
    }

    #[test]
    fn tls_sni_found() {
        let s = sniff(&client_hello("WWW.Example.COM"));
        assert_eq!(s, Sniffed { protocol: Some("tls"), domain: Some("www.example.com".into()) });
    }

    #[test]
    fn http_host_found() {
        let s = sniff(b"GET / HTTP/1.1\r\nUser-Agent: x\r\nHost: ya.ru:8080\r\n\r\n");
        assert_eq!(s, Sniffed { protocol: Some("http"), domain: Some("ya.ru".into()) });
        let s = sniff(b"GET / HTTP/1.1\r\nHost: 1.2.3.4\r\n\r\n");
        assert_eq!(s.domain, None);
    }

    #[test]
    fn bittorrent_and_garbage() {
        assert_eq!(sniff(b"\x13BitTorrent protocol....").protocol, Some("bittorrent"));
        assert_eq!(sniff(b"\x00\x01garbage"), Sniffed::default());
        assert_eq!(sniff(&client_hello("x.com")[..20]).protocol, Some("tls"));
    }
}
