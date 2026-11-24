use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    CreateIpForwardEntry2, DeleteIpForwardEntry2, FreeMibTable, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    GetAdaptersAddresses, GetIpForwardTable2, GetIpInterfaceEntry, IP_ADAPTER_ADDRESSES_LH, InitializeIpForwardEntry, MIB_IPFORWARD_ROW2,
    MIB_IPFORWARD_TABLE2, MIB_IPINTERFACE_ROW,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, IP_UNICAST_IF, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, MIB_IPPROTO_NETMGMT, SOCKADDR, SOCKADDR_INET,
    setsockopt,
};
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub const V6: (Ipv6Addr, Ipv6Addr) =
    (Ipv6Addr::new(0xfd00, 0x7279, 0x6e65, 0x7400, 0, 0, 0, 1), Ipv6Addr::new(0xfd00, 0x7279, 0x6e65, 0x7400, 0, 0, 0, 2));
pub const NAME: &str = "ryNet";
pub const V4: (Ipv4Addr, Ipv4Addr) = (Ipv4Addr::new(172, 19, 0, 1), Ipv4Addr::new(172, 19, 0, 2));
const GUID: u128 = 0x6d3c_f723_5e4d_4f5f_b44d_fb1f_5669_dc87;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const FIREWALL_RULE: &str = "ryNet core";
const IPV6_BLOCK: &str = "ryNet IPv4 only";
static IPV4_ONLY: AtomicBool = AtomicBool::new(false);
static IPV6_BLOCKED: Mutex<Option<FilterSession>> = Mutex::new(None);
static DNS_GUARD: Mutex<Option<FilterSession>> = Mutex::new(None);

#[path = "dns_windows.rs"]
mod dns_guard;
use dns_guard::FilterSession;

const NRPT_COMMENT: &str = "ryNet";
static NRPT: AtomicBool = AtomicBool::new(false);

static PHYS_V4: AtomicU32 = AtomicU32::new(0);
static PHYS_V6: AtomicU32 = AtomicU32::new(0);
static OURS: AtomicU32 = AtomicU32::new(0);
static CHECKED: Mutex<Option<Instant>> = Mutex::new(None);
static DIRECT: Mutex<Vec<(IpAddr, u8)>> = Mutex::new(Vec::new());
const DIRECT_METRIC: u32 = 7590;

struct Up {
    session: Arc<wintun::Session>,
    reader: Option<std::thread::JoinHandle<()>>,
}

static STATE: Mutex<Option<Up>> = Mutex::new(None);

pub fn capable() -> bool {
    elevated() && wintun_path().is_file()
}

fn elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut e as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && e.TokenIsElevated != 0
    }
}

fn wintun_path() -> std::path::PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("wintun.dll"))).unwrap_or_else(|| "wintun.dll".into())
}

fn wintun() -> Result<&'static wintun::Wintun, String> {
    static W: OnceLock<Result<wintun::Wintun, String>> = OnceLock::new();
    W.get_or_init(|| unsafe { wintun::load_from_path(wintun_path()) }.map_err(|e| format!("wintun.dll: {e}")))
        .as_ref()
        .map_err(Clone::clone)
}

const RUN_TIMEOUT: Duration = Duration::from_secs(15);

fn system_dir() -> &'static std::path::Path {
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        use std::os::windows::ffi::OsStringExt;
        let mut buf = [0u16; 260];
        let n = unsafe { windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if n == 0 || n >= buf.len() {
            return std::path::PathBuf::from(r"C:\Windows\System32");
        }
        std::ffi::OsString::from_wide(&buf[..n]).into()
    })
}

fn system_tool(program: &str) -> std::path::PathBuf {
    match program {
        "powershell" => system_dir().join(r"WindowsPowerShell\v1.0\powershell.exe"),
        _ => system_dir().join(format!("{program}.exe")),
    }
}

fn run(program: &str, args: &[&str]) -> io::Result<()> {
    use std::io::Read;
    let mut child = Command::new(system_tool(program))
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let pipe = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut b);
            }
            b
        })
    };
    let (o, e) = (pipe(child.stdout.take().map(|r| Box::new(r) as _)), pipe(child.stderr.take().map(|r| Box::new(r) as _)));
    let until = Instant::now() + RUN_TIMEOUT;
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None if Instant::now() > until => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(io::ErrorKind::TimedOut, format!("{program} {}: timeout", args.join(" "))));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let (stdout, stderr) = (o.join().unwrap_or_default(), e.join().unwrap_or_default());
    if status.success() {
        Ok(())
    } else {
        let text = oem_text(if stdout.iter().all(u8::is_ascii_whitespace) { &stderr } else { &stdout });
        Err(io::Error::other(format!("{program} {}: {}", args.join(" "), text.trim())))
    }
}

fn powershell(script: &str) -> io::Result<()> {
    run("powershell", &["-NoProfile", "-NonInteractive", "-Command", script])
}

fn nrpt_add_script(server: &str, comment: &str) -> String {
    format!("$ErrorActionPreference='Stop'; Add-DnsClientNrptRule -Namespace '.' -NameServers '{server}' -Comment '{comment}'")
}

fn nrpt_remove_script(comment: &str) -> String {
    format!(
        "$ErrorActionPreference='Stop'; Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq '{comment}' }} | Remove-DnsClientNrptRule -Force"
    )
}

const NRPT_KEY: &str = "SYSTEM\\CurrentControlSet\\Services\\Dnscache\\Parameters\\DnsPolicyConfig";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn utf16_until_nul(b: &[u16]) -> String {
    String::from_utf16_lossy(b.split(|&c| c == 0).next().unwrap_or(&[]))
}

fn nrpt_stuck() -> bool {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
    };
    unsafe {
        let mut root: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(NRPT_KEY).as_ptr(), 0, KEY_READ, &mut root) != 0 {
            return false;
        }
        let comment = wide("Comment");
        let mut found = false;
        let mut i = 0;
        loop {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(
                root,
                i,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
            {
                break;
            }
            i += 1;
            let mut sub: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(root, name.as_ptr(), 0, KEY_READ, &mut sub) != 0 {
                continue;
            }
            let mut buf = [0u16; 128];
            let mut size = size_of_val(&buf) as u32;
            let mut kind = 0u32;
            if RegQueryValueExW(sub, comment.as_ptr(), std::ptr::null(), &mut kind, buf.as_mut_ptr().cast(), &mut size) == 0
                && utf16_until_nul(&buf[..(size as usize / 2).min(buf.len())]) == NRPT_COMMENT
            {
                found = true;
            }
            RegCloseKey(sub);
            if found {
                break;
            }
        }
        RegCloseKey(root);
        found
    }
}

fn nrpt_remove() -> bool {
    if NRPT.load(Ordering::Relaxed) || nrpt_stuck() {
        match powershell(&nrpt_remove_script(NRPT_COMMENT)) {
            Ok(()) => NRPT.store(false, Ordering::Relaxed),
            Err(e) => {
                tracing::warn!("dns rule: {e}");
                return false;
            }
        }
    }
    true
}

fn adapter_present() -> bool {
    unsafe {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size = 16 * 1024u32;
        let mut buf: Vec<u64> = Vec::new();
        for _ in 0..3 {
            buf = vec![0u64; size as usize / 8 + 1];
            match GetAdaptersAddresses(AF_UNSPEC as u32, flags, std::ptr::null(), buf.as_mut_ptr().cast(), &mut size) {
                0 => break,
                111 => continue,
                _ => return true,
            }
        }
        let mut a = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !a.is_null() {
            let ad = &*a;
            let name = ad.FriendlyName;
            if !name.is_null() {
                let mut len = 0;
                while *name.add(len) != 0 {
                    len += 1;
                }
                if String::from_utf16_lossy(std::slice::from_raw_parts(name, len)) == NAME {
                    return true;
                }
            }
            a = ad.Next;
        }
        false
    }
}

#[allow(dead_code)]
pub fn leftover() -> Option<bool> {
    let adapter = adapter_present();
    (!adapter && nrpt_stuck()).then_some(true)
}

#[allow(dead_code)]
pub fn clear_leftover() -> bool {
    if leftover().is_none() {
        return false;
    }
    nrpt_remove()
}

fn oem_text(b: &[u8]) -> String {
    use windows_sys::Win32::Globalization::{CP_OEMCP, MultiByteToWideChar};
    if let Ok(s) = std::str::from_utf8(b) {
        return s.to_string();
    }
    let Ok(len) = i32::try_from(b.len()) else { return String::from_utf8_lossy(b).into_owned() };
    unsafe {
        let n = MultiByteToWideChar(CP_OEMCP, 0, b.as_ptr(), len, std::ptr::null_mut(), 0);
        if n <= 0 {
            return String::from_utf8_lossy(b).into_owned();
        }
        let mut w = vec![0u16; n as usize];
        let n = MultiByteToWideChar(CP_OEMCP, 0, b.as_ptr(), len, w.as_mut_ptr(), n);
        String::from_utf16_lossy(&w[..n.max(0) as usize])
    }
}

fn netsh(args: &[&str]) -> io::Result<()> {
    run("netsh", args)
}

unsafe fn sockaddr_ip(p: *const u8) -> Option<IpAddr> {
    unsafe {
        let family = u16::from_ne_bytes([*p, *p.add(1)]);
        if family == AF_INET {
            let b = std::slice::from_raw_parts(p.add(4), 4);
            Some(IpAddr::from([b[0], b[1], b[2], b[3]]))
        } else if family == AF_INET6 {
            let mut a = [0u8; 16];
            a.copy_from_slice(std::slice::from_raw_parts(p.add(8), 16));
            Some(IpAddr::from(a))
        } else {
            None
        }
    }
}

fn default_interface(family: u16) -> u32 {
    let ours = OURS.load(Ordering::Relaxed);
    unsafe {
        let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        if GetIpForwardTable2(family, &mut table) != 0 || table.is_null() {
            return 0;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let mut best = (u32::MAX, 0u32);
        for r in rows {
            if r.DestinationPrefix.PrefixLength != 0 || r.InterfaceIndex == ours || r.InterfaceIndex == 0 {
                continue;
            }
            let mut row: MIB_IPINTERFACE_ROW = std::mem::zeroed();
            row.Family = family;
            row.InterfaceIndex = r.InterfaceIndex;
            if GetIpInterfaceEntry(&mut row) != 0 || !row.Connected {
                continue;
            }
            let metric = r.Metric.saturating_add(row.Metric);
            if metric < best.0 {
                best = (metric, r.InterfaceIndex);
            }
        }
        FreeMibTable(table.cast());
        best.1
    }
}

fn refresh_physical(force: bool) {
    let mut last = CHECKED.lock().unwrap_or_else(|e| e.into_inner());
    if !force && last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
        return;
    }
    *last = Some(Instant::now());
    PHYS_V4.store(default_interface(AF_INET), Ordering::Relaxed);
    PHYS_V6.store(default_interface(AF_INET6), Ordering::Relaxed);
    let want = DIRECT.lock().unwrap_or_else(|e| e.into_inner());
    if !want.is_empty() && OURS.load(Ordering::Relaxed) != 0 {
        sync_direct(&want);
    }
}

fn routes() -> Vec<MIB_IPFORWARD_ROW2> {
    unsafe {
        let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        if GetIpForwardTable2(AF_UNSPEC, &mut table) != 0 || table.is_null() {
            return Vec::new();
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize).to_vec();
        FreeMibTable(table.cast());
        rows
    }
}

fn route_net(r: &MIB_IPFORWARD_ROW2) -> Option<((IpAddr, u8), IpAddr)> {
    let p = unsafe { sockaddr_ip((&r.DestinationPrefix.Prefix as *const SOCKADDR_INET).cast())? };
    let hop = unsafe { sockaddr_ip((&r.NextHop as *const SOCKADDR_INET).cast())? };
    Some(((p, r.DestinationPrefix.PrefixLength), hop))
}

fn sockaddr_inet(ip: IpAddr) -> SOCKADDR_INET {
    let mut a = SOCKADDR_INET::default();
    match ip {
        IpAddr::V4(v) => {
            a.Ipv4.sin_family = AF_INET;
            a.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(v.octets());
        }
        IpAddr::V6(v) => {
            a.Ipv6.sin6_family = AF_INET6;
            a.Ipv6.sin6_addr.u.Byte = v.octets();
        }
    }
    a
}

fn direct_way(net: (IpAddr, u8), rows: &[MIB_IPFORWARD_ROW2], ours: u32) -> Option<(u32, IpAddr)> {
    let mut best: Option<(u8, u32, u32, IpAddr)> = None;
    for r in rows {
        if r.InterfaceIndex == ours || r.InterfaceIndex == 0 || r.Metric == DIRECT_METRIC {
            continue;
        }
        let Some((prefix, hop)) = route_net(r) else { continue };
        if !crate::core::covers(prefix, net) || best.is_some_and(|b| b.0 > prefix.1) {
            continue;
        }
        let mut row: MIB_IPINTERFACE_ROW = unsafe { std::mem::zeroed() };
        row.Family = unsafe { r.DestinationPrefix.Prefix.si_family };
        row.InterfaceIndex = r.InterfaceIndex;
        if unsafe { GetIpInterfaceEntry(&mut row) } != 0 || !row.Connected {
            continue;
        }
        let metric = r.Metric.saturating_add(row.Metric);
        if best.is_none_or(|b| prefix.1 > b.0 || metric < b.1) {
            best = Some((prefix.1, metric, r.InterfaceIndex, hop));
        }
    }
    let (len, _, index, hop) = best?;
    (len <= 1).then_some((index, hop))
}

fn sync_direct(want: &[(IpAddr, u8)]) {
    let ours = OURS.load(Ordering::Relaxed);
    let rows = routes();
    let mut kept = Vec::new();
    for r in rows.iter().filter(|r| r.Metric == DIRECT_METRIC) {
        let Some((net, hop)) = route_net(r) else { continue };
        if want.contains(&net) && !kept.contains(&net) && direct_way(net, &rows, ours) == Some((r.InterfaceIndex, hop)) {
            kept.push(net);
            continue;
        }
        let e = unsafe { DeleteIpForwardEntry2(r) };
        if e != 0 {
            tracing::warn!("tun direct: delete route {}/{}: error {e}", net.0, net.1);
        }
    }
    for &net in want.iter().filter(|n| !kept.contains(n)) {
        let Some((index, hop)) = direct_way(net, &rows, ours) else { continue };
        let mut row: MIB_IPFORWARD_ROW2 = unsafe { std::mem::zeroed() };
        unsafe { InitializeIpForwardEntry(&mut row) };
        row.InterfaceIndex = index;
        row.DestinationPrefix.Prefix = sockaddr_inet(net.0);
        row.DestinationPrefix.PrefixLength = net.1;
        row.NextHop = sockaddr_inet(hop);
        row.Metric = DIRECT_METRIC;
        row.Protocol = MIB_IPPROTO_NETMGMT;
        match unsafe { CreateIpForwardEntry2(&row) } {
            0 => tracing::info!("tun direct {}/{} via {hop} (if {index})", net.0, net.1),
            e => tracing::warn!("tun direct: route {}/{} via {hop} (if {index}): error {e}", net.0, net.1),
        }
    }
}

pub fn direct(nets: &[(IpAddr, u8)]) {
    let mut want = DIRECT.lock().unwrap_or_else(|e| e.into_inner());
    *want = nets.to_vec();
    if OURS.load(Ordering::Relaxed) != 0 {
        sync_direct(&want);
    }
}

pub fn capture(nets: &[(IpAddr, u8)]) {
    static CAPTURE: Mutex<(u32, Vec<(IpAddr, u8)>)> = Mutex::new((0, Vec::new()));
    let mut state = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    let ours = OURS.load(Ordering::Relaxed);
    if state.0 != ours {
        state.0 = ours;
        state.1.clear();
    }
    let have = &mut state.1;
    if ours == 0 {
        have.clear();
        return;
    }
    let idx = ours.to_string();
    for &n in have.iter().filter(|n| !nets.contains(n)) {
        let _ = netsh(&[
            "interface",
            if n.0.is_ipv4() { "ipv4" } else { "ipv6" },
            "delete",
            "route",
            &format!("{}/{}", n.0, n.1),
            &idx,
            "store=active",
        ]);
    }
    have.retain(|n| nets.contains(n));
    for &n in nets {
        if have.contains(&n) {
            continue;
        }
        match netsh(&[
            "interface",
            if n.0.is_ipv4() { "ipv4" } else { "ipv6" },
            "add",
            "route",
            &format!("{}/{}", n.0, n.1),
            &idx,
            "metric=0",
            "store=active",
        ]) {
            Ok(()) => have.push(n),
            Err(e) => tracing::warn!("tun capture: {e}"),
        }
    }
}

pub fn direct_v6() -> bool {
    if OURS.load(Ordering::Relaxed) == 0 {
        return true;
    }
    refresh_physical(false);
    PHYS_V6.load(Ordering::Relaxed) != 0
}

pub fn protect() -> rv_net::Protect {
    Arc::new(|sock| {
        refresh_physical(false);
        let (v4, v6) = (PHYS_V4.load(Ordering::Relaxed), PHYS_V6.load(Ordering::Relaxed));
        let v4 = v4.to_be();
        let ok4 = v4 != 0 && unsafe { setsockopt(sock as usize, IPPROTO_IP, IP_UNICAST_IF, (&v4 as *const u32).cast(), 4) } == 0;
        let ok6 = v6 != 0 && unsafe { setsockopt(sock as usize, IPPROTO_IPV6, IPV6_UNICAST_IF, (&v6 as *const u32).cast(), 4) } == 0;
        if v4 == 0 && v6 == 0 && OURS.load(Ordering::Relaxed) != 0 {
            Err(io::ErrorKind::NetworkUnreachable.into())
        } else if ok4 || ok6 || (v4 == 0 && v6 == 0) {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    })
}

pub fn system_dns() -> Vec<IpAddr> {
    let ours = OURS.load(Ordering::Relaxed);
    let mut out = Vec::new();
    unsafe {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
        let mut size = 16 * 1024u32;
        let mut buf: Vec<u64> = Vec::new();
        let mut loaded = false;
        for _ in 0..3 {
            buf = vec![0u64; size as usize / 8 + 1];
            match GetAdaptersAddresses(AF_UNSPEC as u32, flags, std::ptr::null(), buf.as_mut_ptr().cast(), &mut size) {
                0 => {
                    loaded = true;
                    break;
                }
                111 => continue,
                _ => return out,
            }
        }
        if !loaded {
            return out;
        }
        let mut a = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !a.is_null() {
            let ad = &*a;
            let index = ad.Anonymous1.Anonymous.IfIndex;
            if ad.OperStatus == 1 && ad.IfType != 24 && (ours == 0 || (index != ours && ad.Ipv6IfIndex != ours)) {
                let mut d = ad.FirstDnsServerAddress;
                while !d.is_null() {
                    let sa = (*d).Address.lpSockaddr as *const SOCKADDR;
                    if !sa.is_null()
                        && let Some(ip) = sockaddr_ip(sa.cast())
                        && !ip.is_loopback()
                        && !ip.is_unspecified()
                        && ip != IpAddr::V4(V4.1)
                        && ip != IpAddr::V6(V6.1)
                        && !out.contains(&ip)
                    {
                        out.push(ip);
                    }
                    d = (*d).Next;
                }
            }
            a = ad.Next;
        }
    }
    out
}

pub fn single_instance() {
    use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
    let name: Vec<u16> = "Global\\ryNetCore".encode_utf16().chain([0]).collect();
    let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if h.is_null() {
        return;
    }
    let r = unsafe { WaitForSingleObject(h, 10_000) };
    if r != 0 && r != 0x80 {
        tracing::warn!("previous core still running, starting anyway");
    }
}

fn firewall() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        let Ok(exe) = std::env::current_exe() else { return };
        let _ = netsh(&["advfirewall", "firewall", "delete", "rule", &format!("name={FIREWALL_RULE}")]);
        if let Err(e) = netsh(&[
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={FIREWALL_RULE}"),
            "dir=in",
            "action=allow",
            &format!("program={}", exe.display()),
            "enable=yes",
            "profile=any",
        ]) {
            tracing::warn!("firewall rule: {e}");
        }
    });
}

fn release_ipv6_block() {
    IPV6_BLOCKED.lock().unwrap_or_else(|e| e.into_inner()).take();
}

fn block_ipv6() -> io::Result<()> {
    use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::*;
    let mut held = IPV6_BLOCKED.lock().unwrap_or_else(|e| e.into_inner());
    if held.is_some() {
        return Ok(());
    }
    unsafe {
        let mut session: FWPM_SESSION0 = std::mem::zeroed();
        session.flags = FWPM_SESSION_FLAG_DYNAMIC;
        let mut handle: HANDLE = std::ptr::null_mut();
        let status = FwpmEngineOpen0(std::ptr::null(), 10, std::ptr::null(), &session, &mut handle);
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let block = FilterSession(handle as usize);
        let mut name: Vec<u16> = IPV6_BLOCK.encode_utf16().chain(Some(0)).collect();
        let mut weight = u64::MAX;
        let mut filter: FWPM_FILTER0 = std::mem::zeroed();
        filter.displayData.name = name.as_mut_ptr();
        filter.layerKey = FWPM_LAYER_OUTBOUND_IPPACKET_V6;
        filter.subLayerKey = FWPM_SUBLAYER_UNIVERSAL;
        filter.action.r#type = FWP_ACTION_BLOCK;
        filter.weight.r#type = FWP_UINT64;
        filter.weight.Anonymous.uint64 = &mut weight;
        let status = FwpmFilterAdd0(handle, &filter, std::ptr::null_mut(), std::ptr::null_mut());
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        *held = Some(block);
    }
    Ok(())
}

fn guard_dns(luid: u64) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let exe: Vec<u16> = std::env::current_exe()?.as_os_str().encode_wide().chain(Some(0)).collect();
    let guard = dns_guard::install(&exe, luid)?;
    *DNS_GUARD.lock().unwrap_or_else(|e| e.into_inner()) = Some(guard);
    Ok(())
}

fn release_dns_guard() {
    DNS_GUARD.lock().unwrap_or_else(|e| e.into_inner()).take();
}

fn setup_ipv6(index: u32) -> io::Result<()> {
    crate::tun_setup::ready(
        || {
            let mut row: MIB_IPINTERFACE_ROW = unsafe { std::mem::zeroed() };
            row.Family = AF_INET6;
            row.InterfaceIndex = index;
            let status = unsafe { GetIpInterfaceEntry(&mut row) };
            if status == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(status as i32)) }
        },
        || std::thread::sleep(Duration::from_millis(100)),
    )?;
    let idx = index.to_string();
    netsh(&["interface", "ipv6", "add", "address", &idx, &V6.0.to_string(), "store=active"])?;
    netsh(&["interface", "ipv6", "set", "interface", &idx, "metric=1"])?;
    for net in ["::/1", "8000::/1"] {
        netsh(&["interface", "ipv6", "add", "route", net, &idx, "metric=0", "store=active"])?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AdapterError {
    Exists,
    QueryTimeout,
    Setup,
    Other,
}

static WINTUN_LOG: Mutex<String> = Mutex::new(String::new());

unsafe extern "stdcall" fn wintun_logger(level: i32, _timestamp: u64, message: *const u16) {
    if message.is_null() {
        return;
    }
    let mut len = 0;
    while unsafe { *message.add(len) } != 0 {
        len += 1;
    }
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(message, len) });
    match level {
        0 => tracing::info!(target: "wintun::log", "WinTun: {text}"),
        1 => tracing::warn!(target: "wintun::log", "WinTun: {text}"),
        _ => tracing::error!(target: "wintun::log", "WinTun: {text}"),
    }
    let mut log = WINTUN_LOG.lock().unwrap_or_else(|e| e.into_inner());
    if log.len() < 4096 {
        log.push_str(&text);
        log.push('\n');
    }
}

fn hook_wintun_logger(w: &wintun::Wintun) {
    static HOOKED: std::sync::Once = std::sync::Once::new();
    HOOKED.call_once(|| {
        drop(wintun::Adapter::open(w, NAME));
        wintun::set_logger(w, Some(wintun_logger));
    });
}

fn classify(why: &str) -> AdapterError {
    let w = why.to_lowercase();
    let has = |codes: &[&str]| codes.iter().any(|c| w.contains(c));
    if has(&["0xb7", "already exists", "уже существует"]) {
        AdapterError::Exists
    } else if has(&["0x102", "timed out", "истекло"]) {
        AdapterError::QueryTimeout
    } else if has(&["c00002f0", "c0000035", "c0000490", "failed to setup adapter"]) {
        AdapterError::Setup
    } else {
        AdapterError::Other
    }
}

fn remove_ghost() {
    let id = format!(
        "SWD\\Wintun\\{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
        (GUID >> 96) as u32,
        (GUID >> 80) as u16,
        (GUID >> 64) as u16,
        (GUID >> 48) as u16,
        (GUID as u64) & 0xFFFF_FFFF_FFFF
    );
    match run("pnputil", &["/remove-device", &id]) {
        Ok(()) => tracing::info!("tun: removed the leftover {id} device"),
        Err(e) => tracing::debug!("tun: no leftover device: {e}"),
    }
}

fn remove_stale(w: &wintun::Wintun) -> bool {
    match wintun::Adapter::open(w, NAME) {
        Ok(a) => {
            drop(a);
            true
        }
        Err(_) => false,
    }
}

fn create_adapter(w: &wintun::Wintun) -> Result<Arc<wintun::Adapter>, String> {
    if remove_stale(w) {
        tracing::info!("tun: removed a stale {NAME} adapter");
    }
    remove_ghost();
    hook_wintun_logger(w);
    let until = Instant::now() + Duration::from_secs(45);
    let mut last = (AdapterError::Other, String::new());
    for attempt in 0..crate::tun_setup::ADAPTER_TRIES {
        WINTUN_LOG.lock().unwrap_or_else(|e| e.into_inner()).clear();
        match wintun::Adapter::create(w, NAME, NAME, Some(GUID)) {
            Ok(a) => return Ok(a),
            Err(e) => {
                let logged = std::mem::take(&mut *WINTUN_LOG.lock().unwrap_or_else(|e| e.into_inner()));
                last = (classify(&format!("{e}\n{logged}")), e.to_string());
            }
        }
        let (kind, why) = &last;
        let limit = match kind {
            AdapterError::Exists | AdapterError::QueryTimeout => crate::tun_setup::ADAPTER_TRIES,
            AdapterError::Setup | AdapterError::Other => 2,
        };
        tracing::warn!("tun: adapter attempt {} failed ({kind:?}): {why}", attempt + 1);
        if attempt + 1 >= limit || Instant::now() > until {
            break;
        }
        if *kind == AdapterError::Exists {
            if remove_stale(w) {
                tracing::info!("tun: removed the {NAME} adapter that was in the way");
            }
            remove_ghost();
        }
        std::thread::sleep(crate::tun_setup::retry_pause(attempt));
    }
    let (kind, why) = last;
    let hint = match kind {
        AdapterError::Exists => rv_core::lang::t(
            "адаптер ryNet уже существует и не удаляется: перезапустите приложение или удалите его в диспетчере устройств",
            "the ryNet adapter already exists and can't be removed: restart the app or delete it in Device Manager",
        ),
        AdapterError::QueryTimeout => rv_core::lang::t(
            "драйвер не ответил вовремя: подождите минуту и повторите, при повторе перезагрузите компьютер",
            "the driver did not respond in time: wait a minute and retry, reboot if it repeats",
        ),
        AdapterError::Setup => rv_core::lang::t(
            "система не смогла установить адаптер: проверьте, что антивирус не блокирует wintun.dll, и перезагрузите компьютер",
            "Windows could not set the adapter up: check that an antivirus isn't blocking wintun.dll, and reboot",
        ),
        AdapterError::Other => rv_core::lang::t("не удалось создать адаптер", "couldn't create the adapter"),
    };
    Err(format!("Wintun: {hint} ({why})"))
}

fn bring_up(mtu: usize, block: bool, ipv4_only: bool) -> Result<Arc<wintun::Session>, String> {
    if !elevated() {
        return Err(rv_core::lang::t(
            "Нет прав на режим «Весь трафик»: разрешите его в настройках",
            "No permission for \"All traffic\" mode: allow it in settings",
        )
        .into());
    }
    let w = wintun()?;
    reset();
    if ipv4_only {
        block_ipv6().map_err(|e| format!("IPv6 block: {e}"))?;
    }
    refresh_physical(true);
    if !block && PHYS_V4.load(Ordering::Relaxed) == 0 {
        return Err(rv_core::lang::t("Нет подключения к сети", "No network connection").into());
    }
    let dns = system_dns();
    firewall();
    let adapter = create_adapter(w)?;
    let index = adapter.get_adapter_index().map_err(|e| format!("Wintun: {e}"))?;
    OURS.store(index, Ordering::Relaxed);
    let session = Arc::new(adapter.start_session(0x80_0000).map_err(|e| format!("Wintun: {e}"))?);
    let idx = index.to_string();
    let gw = V4.1.to_string();
    guard_dns(unsafe { adapter.get_luid().Value }).map_err(|e| format!("DNS guard: {e}"))?;
    let nrpt = (!block).then(|| {
        NRPT.store(true, Ordering::Relaxed);
        let script = nrpt_add_script(&gw, NRPT_COMMENT);
        std::thread::spawn(move || powershell(&script))
    });
    let r = (|| -> io::Result<()> {
        netsh(&[
            "interface",
            "ipv4",
            "set",
            "address",
            &format!("name={idx}"),
            "source=static",
            &format!("address={}", V4.0),
            "mask=255.255.255.252",
        ])?;
        match crate::tun_setup::ipv6(ipv4_only, block_ipv6, || setup_ipv6(index)) {
            Err(e) if block && !ipv4_only => tracing::warn!("kill switch: {e}"),
            r => {
                r?;
            }
        }
        if !ipv4_only
            && let Err(e) = netsh(&[
                "interface",
                "ipv6",
                "set",
                "dnsservers",
                &format!("name={idx}"),
                "source=static",
                &format!("address={}", V6.1),
                "register=none",
                "validate=no",
            ])
        {
            tracing::warn!("tun dns: {e}");
        }
        if let Err(e) = netsh(&["interface", "ipv4", "set", "subinterface", &idx, &format!("mtu={mtu}"), "store=active"]) {
            tracing::warn!("tun mtu {mtu}: {e}");
            if mtu != 1500
                && let Err(e) = netsh(&["interface", "ipv4", "set", "subinterface", &idx, "mtu=1500", "store=active"])
            {
                tracing::warn!("tun mtu 1500: {e}");
            }
        }
        netsh(&["interface", "ipv4", "set", "interface", &idx, "metric=1"])?;
        if let Err(e) = netsh(&[
            "interface",
            "ipv4",
            "set",
            "dnsservers",
            &format!("name={idx}"),
            "source=static",
            &format!("address={gw}"),
            "register=none",
            "validate=no",
        ]) {
            tracing::warn!("tun dns: {e}");
        }
        for net in ["0.0.0.0/1", "128.0.0.0/1"] {
            netsh(&["interface", "ipv4", "add", "route", net, &idx, &gw, "metric=0", "store=active"])?;
        }
        for net in ["224.0.0.0/4", "255.255.255.255/32"] {
            let _ = netsh(&["interface", "ipv4", "delete", "route", &format!("prefix={net}"), &format!("interface={idx}"), "store=active"]);
        }
        for ns in dns.iter().filter(|ip| ip.is_ipv4()) {
            let _ = netsh(&["interface", "ipv4", "add", "route", &format!("{ns}/32"), &idx, &gw, "metric=0", "store=active"]);
        }
        Ok(())
    })();
    let r = r.and_then(|_| {
        let until = Instant::now() + Duration::from_secs(8);
        loop {
            match std::net::TcpListener::bind((V4.0, 0))
                .and_then(|_| if block || ipv4_only { Ok(()) } else { std::net::TcpListener::bind((V6.0, 0)).map(drop) })
            {
                Ok(_) => return Ok(()),
                Err(e) if Instant::now() > until => {
                    return Err(io::Error::other(format!("{} {}: {e}", rv_core::lang::t("адрес", "address"), V4.0)));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    let nrpt = nrpt.map(|t| t.join().unwrap_or_else(|_| Err(io::Error::other("powershell thread panicked"))));
    if let Err(e) = r {
        if nrpt.is_some() {
            nrpt_remove();
        }
        let _ = session.shutdown();
        OURS.store(0, Ordering::Relaxed);
        return Err(e.to_string());
    }
    if !ipv4_only {
        release_ipv6_block();
    }
    match &nrpt {
        Some(Ok(())) => tracing::info!("dns rule: all names go to {gw}"),
        Some(Err(e)) => tracing::warn!("dns rule: {e}"),
        None => {}
    }
    tracing::info!(
        "tun {NAME} up (if {index}), mtu {mtu}, dns {dns:?}, phys v4 {} v6 {}",
        PHYS_V4.load(Ordering::Relaxed),
        PHYS_V6.load(Ordering::Relaxed)
    );
    Ok(session)
}

pub async fn config(
    mtu: usize,
    kill_switch: bool,
    ipv4_only: bool,
    network: tokio::sync::watch::Receiver<u64>,
) -> Result<rv_tun::Config, String> {
    IPV4_ONLY.store(ipv4_only, Ordering::Relaxed);
    let session =
        match tokio::task::spawn_blocking(move || bring_up(mtu, false, ipv4_only)).await.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(session) => session,
            Err(error) => {
                reset();
                if !kill_switch {
                    release_dns_guard();
                    release_ipv6_block();
                }
                return Err(error);
            }
        };
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1024);
    let s = session.clone();
    let reader = std::thread::Builder::new()
        .name("wintun-rx".into())
        .spawn(move || {
            while let Ok(p) = s.receive_blocking() {
                if tx.blocking_send(p.bytes().to_vec()).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| {
            let _ = session.shutdown();
            reset();
            if !kill_switch {
                release_dns_guard();
                release_ipv6_block();
            }
            e.to_string()
        })?;
    let s = session.clone();
    let send = Box::new(move |pkt: &[u8]| {
        if let Ok(len) = u16::try_from(pkt.len())
            && let Ok(mut p) = s.allocate_send_packet(len)
        {
            p.bytes_mut().copy_from_slice(pkt);
            s.send_packet(p);
        }
    });
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Up { session, reader: Some(reader) });
    Ok(rv_tun::Config {
        device: rv_tun::Device { rx, send },
        mtu,
        v4: Some(V4),
        v6: (!ipv4_only).then_some(V6),
        network,
        stack: rv_tun::Stack::System,
    })
}

pub fn block() -> Result<(), String> {
    let session = bring_up(1500, true, IPV4_ONLY.load(Ordering::Relaxed))?;
    let s = session.clone();
    let reader = std::thread::Builder::new()
        .name("wintun-block".into())
        .spawn(move || while s.receive_blocking().is_ok() {})
        .map_err(|e| e.to_string())?;
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Up { session, reader: Some(reader) });
    Ok(())
}

pub fn down() {
    reset();
    release_dns_guard();
    release_ipv6_block();
}

fn reset() {
    nrpt_remove();
    capture(&[]);
    let mut want = DIRECT.lock().unwrap_or_else(|e| e.into_inner());
    want.clear();
    sync_direct(&want);
    drop(want);
    let up = STATE.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(mut up) = up {
        let _ = up.session.shutdown();
        if let Some(r) = up.reader.take() {
            let _ = r.join();
        }
        drop(up);
        if let Ok(w) = wintun() {
            let gone = |secs| {
                let until = Instant::now() + Duration::from_secs(secs);
                while remove_stale(w) {
                    if Instant::now() > until {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                true
            };
            let closed = gone(3);
            remove_ghost();
            if !closed {
                gone(2);
            }
        }
    }
    OURS.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nrpt_scripts() {
        assert_eq!(
            nrpt_add_script("172.19.0.2", "ryNet"),
            "$ErrorActionPreference='Stop'; Add-DnsClientNrptRule -Namespace '.' -NameServers '172.19.0.2' -Comment 'ryNet'"
        );
        assert_eq!(
            nrpt_remove_script("ryNet"),
            "$ErrorActionPreference='Stop'; Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'ryNet' } | Remove-DnsClientNrptRule -Force"
        );
    }

    #[test]
    fn utf16_cut_at_nul() {
        let b: Vec<u16> = "ryNet\0junk".encode_utf16().collect();
        assert_eq!(utf16_until_nul(&b), "ryNet");
        assert_eq!(utf16_until_nul(&[]), "");
    }
}
