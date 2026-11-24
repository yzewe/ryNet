use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID,
    TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

fn port(v: u32) -> u16 {
    u16::from_be(v as u16)
}

fn table<T: Copy>(tcp: bool, family: u16) -> Vec<T> {
    let mut size = 0u32;
    for _ in 0..4 {
        let mut buf = vec![0u64; size as usize / 8 + 1];
        let r = unsafe {
            if tcp {
                GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut size, 0, family as u32, TCP_TABLE_OWNER_PID_ALL, 0)
            } else {
                GetExtendedUdpTable(buf.as_mut_ptr().cast(), &mut size, 0, family as u32, UDP_TABLE_OWNER_PID, 0)
            }
        };
        match r {
            0 => unsafe {
                let base = buf.as_ptr() as *const u8;
                let n = *(base as *const u32) as usize;
                let off = std::mem::align_of::<T>().max(4);
                let rows = base.add(off) as *const T;
                let max = (size as usize).saturating_sub(off) / size_of::<T>();
                return std::slice::from_raw_parts(rows, n.min(max)).to_vec();
            },
            122 => continue,
            _ => break,
        }
    }
    Vec::new()
}

fn v6(b: [u8; 16]) -> IpAddr {
    let a = Ipv6Addr::from(b);
    a.to_ipv4_mapped().map_or(IpAddr::V6(a), IpAddr::V4)
}

fn same(local: IpAddr, lport: u16, src: SocketAddr, any_ok: bool) -> bool {
    lport == src.port() && (local == src.ip() || (any_ok && local.is_unspecified()))
}

fn pid_of(tcp: bool, src: SocketAddr) -> Option<u32> {
    if tcp {
        let four = table::<MIB_TCPROW_OWNER_PID>(true, AF_INET)
            .into_iter()
            .find(|r| same(IpAddr::V4(Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes())), port(r.dwLocalPort), src, false))
            .map(|r| r.dwOwningPid);
        four.or_else(|| {
            table::<MIB_TCP6ROW_OWNER_PID>(true, AF_INET6)
                .into_iter()
                .find(|r| same(v6(r.ucLocalAddr), port(r.dwLocalPort), src, false))
                .map(|r| r.dwOwningPid)
        })
    } else {
        let four = table::<MIB_UDPROW_OWNER_PID>(false, AF_INET)
            .into_iter()
            .find(|r| same(IpAddr::V4(Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes())), port(r.dwLocalPort), src, true))
            .map(|r| r.dwOwningPid);
        four.or_else(|| {
            table::<MIB_UDP6ROW_OWNER_PID>(false, AF_INET6)
                .into_iter()
                .find(|r| same(v6(r.ucLocalAddr), port(r.dwLocalPort), src, true))
                .map(|r| r.dwOwningPid)
        })
    }
    .filter(|&p| p != 0)
}

fn process_name(pid: u32) -> Option<String> {
    process_path(pid)?.rsplit(['\\', '/']).next().map(str::to_string)
}

#[allow(dead_code)]
pub fn granted_client(peer: SocketAddr) -> bool {
    let Some(want) = std::env::current_exe().ok().and_then(|p| std::fs::read_to_string(p.with_file_name("client.txt")).ok()) else {
        return true;
    };
    let want = want.trim_start_matches('\u{feff}').trim();
    if want.is_empty() {
        return true;
    }
    let got = pid_of(true, peer).and_then(process_path);
    let ok = got.as_deref().is_some_and(|g| g.to_lowercase() == want.to_lowercase());
    if !ok {
        tracing::warn!("listen: client {} is not the app the access was granted to", got.as_deref().unwrap_or("?"));
    }
    ok
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn cached_name(pid: u32) -> Option<String> {
    use std::time::{Duration, Instant};
    static NAMES: std::sync::Mutex<Option<std::collections::HashMap<u32, (String, Instant)>>> = std::sync::Mutex::new(None);
    const TTL: Duration = Duration::from_secs(10);
    let now = Instant::now();
    let mut g = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let names = g.get_or_insert_with(Default::default);
    if let Some((n, at)) = names.get(&pid)
        && now - *at < TTL
    {
        return Some(n.clone());
    }
    let n = process_name(pid)?;
    if names.len() >= 512 {
        names.retain(|_, (_, at)| now - *at < TTL);
    }
    names.insert(pid, (n.clone(), now));
    Some(n)
}

pub fn owner() -> rv_core::engine::Owner {
    std::sync::Arc::new(|tcp, src, _dst| pid_of(tcp, src).and_then(cached_name))
}
