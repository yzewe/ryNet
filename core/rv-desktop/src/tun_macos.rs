use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::RawFd;
use std::sync::Arc;

pub const V4: (Ipv4Addr, Ipv4Addr) = (Ipv4Addr::new(172, 19, 0, 1), Ipv4Addr::new(172, 19, 0, 2));
#[allow(dead_code)]
pub const V6: (Ipv6Addr, Ipv6Addr) =
    (Ipv6Addr::new(0xfd00, 0x7279, 0x6e65, 0x7400, 0, 0, 0, 1), Ipv6Addr::new(0xfd00, 0x7279, 0x6e65, 0x7400, 0, 0, 0, 2));

fn unsupported() -> String {
    if rv_core::lang::english() {
        "TUN mode isn't available on macOS yet: use proxy mode".into()
    } else {
        "Режим TUN на macOS пока недоступен: включите режим прокси".into()
    }
}

pub fn capable() -> bool {
    false
}

pub fn protect() -> rv_net::Protect {
    Arc::new(|_fd: RawFd| -> io::Result<()> { Ok(()) })
}

fn resolv(mut each: impl FnMut(&str)) {
    for l in std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default().lines() {
        each(l);
    }
}

pub fn system_dns() -> Vec<IpAddr> {
    let mut out = Vec::new();
    resolv(|l| {
        if let Some(ip) = l.trim().strip_prefix("nameserver").and_then(|a| a.trim().parse::<IpAddr>().ok())
            && !ip.is_loopback()
            && !out.contains(&ip)
        {
            out.push(ip);
        }
    });
    out
}

pub fn search_domains() -> Vec<String> {
    let mut out = Vec::new();
    resolv(|l| {
        let mut words = l.split_whitespace();
        if !matches!(words.next(), Some("search" | "domain")) {
            return;
        }
        for d in words.map(|d| d.trim_matches('.').to_ascii_lowercase()) {
            if !d.is_empty() && !out.contains(&d) {
                out.push(d);
            }
        }
    });
    out
}

pub async fn config(
    _mtu: usize,
    _kill_switch: bool,
    _ipv4_only: bool,
    _network: tokio::sync::watch::Receiver<u64>,
) -> Result<rv_tun::Config, String> {
    Err(unsupported())
}

pub fn capture(_nets: &[(IpAddr, u8)]) {}

pub fn direct(_nets: &[(IpAddr, u8)]) {}

pub fn block() -> Result<(), String> {
    Err(unsupported())
}

pub fn leftover() -> Option<bool> {
    None
}

pub fn down() {}

pub struct FdGuard(Option<std::os::fd::OwnedFd>);

impl FdGuard {
    pub fn new(fd: RawFd) -> Self {
        Self(Some(unsafe { std::os::fd::FromRawFd::from_raw_fd(fd) }))
    }

    pub fn release(mut self) {
        if let Some(fd) = self.0.take() {
            let _ = std::os::fd::IntoRawFd::into_raw_fd(fd);
        }
    }
}
