use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

fn parse_addr(s: &str) -> Option<SocketAddr> {
    let (a, p) = s.split_once(':')?;
    let port = u16::from_str_radix(p, 16).ok()?;
    let ip = match a.len() {
        8 => IpAddr::V4(Ipv4Addr::from(u32::from_str_radix(a, 16).ok()?.swap_bytes())),
        32 => {
            let mut b = [0u8; 16];
            for i in 0..4 {
                let w = u32::from_str_radix(&a[i * 8..i * 8 + 8], 16).ok()?;
                b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
            }
            let v6 = Ipv6Addr::from(b);
            v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4)
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

fn inode_of(tcp: bool, src: SocketAddr) -> Option<u64> {
    let files: [&str; 2] = if tcp { ["/proc/net/tcp", "/proc/net/tcp6"] } else { ["/proc/net/udp", "/proc/net/udp6"] };
    for f in files {
        let text = std::fs::read_to_string(f).ok()?;
        for l in text.lines().skip(1) {
            let cols: Vec<&str> = l.split_whitespace().collect();
            if cols.len() > 9 && parse_addr(cols[1]) == Some(src) {
                return cols[9].parse().ok().filter(|&i| i != 0);
            }
        }
    }
    None
}

fn process_of(inode: u64) -> Option<String> {
    let want = format!("socket:[{inode}]");
    for p in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = p.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else { continue };
        if fds.flatten().any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l.as_os_str() == want.as_str())) {
            return std::fs::read_to_string(p.path().join("comm")).ok().map(|c| c.trim().to_string());
        }
    }
    None
}

pub fn owner() -> rv_core::engine::Owner {
    std::sync::Arc::new(|tcp, src, _dst| inode_of(tcp, src).and_then(process_of))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(parse_addr("0100007F:1F90"), Some("127.0.0.1:8080".parse().unwrap()));
        assert_eq!(parse_addr("0000000000000000FFFF00000100007F:0050"), Some("127.0.0.1:80".parse().unwrap()));
        assert_eq!(parse_addr("00000000000000000000000001000000:0016"), Some("[::1]:22".parse().unwrap()));
    }

    #[test]
    fn finds_self() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let name = inode_of(true, l.local_addr().unwrap()).and_then(process_of);
        let me = std::fs::read_to_string("/proc/self/comm").unwrap();
        assert_eq!(name.as_deref(), Some(me.trim()));
    }
}
