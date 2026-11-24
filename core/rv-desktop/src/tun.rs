use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::RawFd;
use std::process::Command;
use std::sync::{Arc, Mutex};

pub const MARK: u32 = 0x2d0;
const TABLE: &str = "7590";
pub const IFACE: &str = "rynet0";
pub const V4: (Ipv4Addr, Ipv4Addr) = (Ipv4Addr::new(172, 19, 0, 1), Ipv4Addr::new(172, 19, 0, 2));
pub const V6: (Ipv6Addr, Ipv6Addr) =
    (Ipv6Addr::new(0xfd00, 0x7279, 0x6e65, 0x7400, 0, 0, 0, 1), Ipv6Addr::new(0xfd00, 0x7279, 0x6e65, 0x7400, 0, 0, 0, 2));
const LAN: [&str; 5] = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16", "224.0.0.0/4"];
const PREFS: std::ops::RangeInclusive<u32> = 7590..=7610;
const PREF_MARK: u32 = 7590;
const PREF_DNS_LAST: u32 = 7600;
const PREF_MAIN: u32 = 7601;
const PREF_LAN: u32 = 7602;
const PREF_DIRECT: u32 = 7609;
const PREF_TUN: u32 = 7610;
const PREF_GUARD: u32 = 7611;

const CAP_NET_ADMIN: u32 = 12;

const NFT_TABLE: &str = "rynet";

#[derive(Default)]
struct Nft {
    on: Option<bool>,
    dns_guard: bool,
    direct: Vec<(IpAddr, u8)>,
    capture: Vec<(IpAddr, u8)>,
}
static NFT: Mutex<Nft> = Mutex::new(Nft { on: None, dns_guard: true, direct: Vec::new(), capture: Vec::new() });

static CAPTURE: Mutex<Vec<(IpAddr, u8)>> = Mutex::new(Vec::new());
static DIRECT: Mutex<Vec<(IpAddr, u8)>> = Mutex::new(Vec::new());

pub fn capable() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("CapEff:").map(|v| v.trim().to_string())))
        .and_then(|hex| u64::from_str_radix(&hex, 16).ok())
        .is_some_and(|caps| caps & (1 << CAP_NET_ADMIN) != 0)
}

#[repr(C)]
struct CapHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn raise_ambient() -> io::Result<()> {
    const V3: u32 = 0x2008_0522;
    let mut hdr = CapHeader { version: V3, pid: 0 };
    let mut data = [CapData::default(); 2];
    unsafe {
        if libc::syscall(libc::SYS_capget, &mut hdr as *mut CapHeader, data.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        data[0].inheritable |= 1 << CAP_NET_ADMIN;
        if libc::syscall(libc::SYS_capset, &mut hdr as *mut CapHeader, data.as_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_RAISE, CAP_NET_ADMIN as libc::c_ulong, 0, 0) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

const TOOL_DIRS: [&str; 5] = ["/usr/sbin", "/usr/bin", "/sbin", "/bin", "/run/current-system/sw/bin"];

fn tool(name: &str) -> Command {
    let path = TOOL_DIRS
        .iter()
        .map(|d| std::path::Path::new(d).join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| std::path::Path::new(TOOL_DIRS[0]).join(name));
    let mut c = Command::new(path);
    c.env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin").env("LC_ALL", "C");
    c
}

fn ip(args: &[&str]) -> io::Result<()> {
    let out = tool("ip").args(args).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("ip {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())))
    }
}

fn resolvectl(args: &[&str]) -> io::Result<()> {
    use std::process::Stdio;
    let mut child =
        tool("resolvectl").arg("--no-ask-password").args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while child.try_wait()?.is_none() {
        if std::time::Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(format!("resolvectl {}: timeout", args.join(" "))));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("resolvectl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())))
    }
}

fn resolved_running(runtime_dir: bool, resolv_conf: &str) -> bool {
    runtime_dir
        || resolv_conf.lines().any(|l| {
            l.trim().strip_prefix("nameserver").and_then(|a| a.trim().parse::<IpAddr>().ok())
                == Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 53)))
        })
}

fn resolved() -> bool {
    resolved_running(
        std::path::Path::new("/run/systemd/resolve").is_dir(),
        &std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default(),
    )
}

fn resolved_args(iface: &str, dns: Ipv4Addr) -> [Vec<String>; 3] {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
    [v(&["dns", iface, &dns.to_string()]), v(&["domain", iface, "~."]), v(&["default-route", iface, "yes"])]
}

fn route_resolved() -> io::Result<bool> {
    if !resolved() {
        return Ok(false);
    }
    for a in resolved_args(IFACE, V4.1) {
        resolvectl(&a.iter().map(String::as_str).collect::<Vec<_>>())?;
    }
    Ok(true)
}

pub fn protect() -> rv_net::Protect {
    Arc::new(|fd: RawFd| {
        let mark = MARK;
        let r = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_MARK,
                (&mark as *const u32).cast(),
                std::mem::size_of::<u32>() as libc::socklen_t,
            )
        };
        if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    })
}

pub fn system_dns() -> Vec<IpAddr> {
    let mut out = Vec::new();
    for f in ["/etc/resolv.conf", "/run/systemd/resolve/resolv.conf"] {
        for l in std::fs::read_to_string(f).unwrap_or_default().lines() {
            if let Some(ip) = l.trim().strip_prefix("nameserver").and_then(|a| a.trim().parse::<IpAddr>().ok())
                && !ip.is_loopback()
                && !out.contains(&ip)
            {
                out.push(ip);
            }
        }
    }
    out
}

pub fn search_domains() -> Vec<String> {
    let mut out = Vec::new();
    for f in ["/etc/resolv.conf", "/run/systemd/resolve/resolv.conf"] {
        for l in std::fs::read_to_string(f).unwrap_or_default().lines() {
            let mut words = l.split_whitespace();
            if !matches!(words.next(), Some("search" | "domain")) {
                continue;
            }
            for d in words.map(|d| d.trim_matches('.').to_ascii_lowercase()) {
                if !d.is_empty() && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') && !out.contains(&d) {
                    out.push(d);
                }
            }
        }
    }
    out
}

fn open_tun() -> io::Result<RawFd> {
    unsafe {
        let fd = libc::open(c"/dev/net/tun".as_ptr(), libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut ifr = [0u8; 40];
        ifr[..IFACE.len()].copy_from_slice(IFACE.as_bytes());
        let flags: i16 = 0x0001 | 0x1000;
        ifr[16..18].copy_from_slice(&flags.to_ne_bytes());
        const TUNSETIFF: libc::c_ulong = 0x4004_54ca;
        if libc::ioctl(fd, TUNSETIFF as _, ifr.as_mut_ptr()) < 0 {
            let e = io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        Ok(fd)
    }
}

pub fn up(mtu: usize, kill_switch: bool, ipv4_only: bool) -> Result<RawFd, String> {
    if !capable() {
        return Err(rv_core::lang::t(
            "Нет прав на режим TUN: разрешите его в настройках",
            "No permission for TUN mode: allow it in settings",
        )
        .into());
    }
    raise_ambient().map_err(|e| format!("{}: {e}", rv_core::lang::t("Права для ip", "Permissions for ip")))?;
    if kill_switch {
        guard();
    }
    clear(kill_switch);
    let fd = open_tun().map_err(|e| match e.raw_os_error() {
        Some(libc::EBUSY) if rv_core::lang::english() => format!("TUN: {IFACE} is used by another program"),
        Some(libc::EBUSY) => format!("TUN: {IFACE} занят другой программой"),
        _ => format!("TUN: {e}"),
    })?;
    let mtu = mtu.to_string();
    let addr = format!("{}/30", V4.0);
    let mark = format!("{MARK:#x}");
    let r = (|| -> io::Result<()> {
        ip(&["addr", "add", &addr, "dev", IFACE])?;
        if !ipv4_only {
            ip(&["-6", "addr", "add", &format!("{}/126", V6.0), "dev", IFACE, "nodad"])?;
        }
        ip(&["link", "set", "dev", IFACE, "mtu", &mtu, "multicast", "off", "up"])?;
        ip(&["route", "replace", "default", "dev", IFACE, "table", TABLE])?;
        if kill_switch {
            ip(&["route", "add", "blackhole", "default", "table", TABLE, "metric", "1000"])?;
        }
        ip(&["-6", "route", "replace", "unreachable", "default", "table", TABLE])?;
        rules(&mark)?;
        if kill_switch {
            nft_up(false, true);
        }
        let mut pref = PREF_MARK + 1;
        for ns in system_dns().into_iter().filter(IpAddr::is_ipv4) {
            for proto in ["udp", "tcp"] {
                if pref <= PREF_DNS_LAST {
                    ip(&[
                        "rule",
                        "add",
                        "pref",
                        &pref.to_string(),
                        "to",
                        &ns.to_string(),
                        "ipproto",
                        proto,
                        "dport",
                        "53",
                        "lookup",
                        TABLE,
                    ])?;
                    pref += 1;
                }
            }
        }
        for (i, net) in LAN.iter().enumerate() {
            ip(&["rule", "add", "pref", &(PREF_LAN + i as u32).to_string(), "to", net, "lookup", "main"])?;
        }
        Ok(())
    })();
    if let Err(e) = r {
        unsafe { libc::close(fd) };
        clear(kill_switch);
        return Err(e.to_string());
    }
    unguard();
    let dns = match route_resolved() {
        Ok(true) => ", resolved dns via tun",
        Ok(false) => "",
        Err(e) => {
            nft_dns_open();
            tracing::warn!(
                "{}: {e}. {} {}",
                rv_core::lang::t(
                    "systemd-resolved не принял DNS для rynet0, имена могут уходить мимо VPN",
                    "systemd-resolved did not take the DNS for rynet0, names may leave outside the VPN"
                ),
                rv_core::lang::t("Kill switch в этом случае имена не блокирует.", "The kill switch does not block names in this case."),
                rv_core::lang::t(
                    "Выдайте права заново в приложении: Настройки → Сеть → «Разрешить режим «Весь трафик»» (ставит правило polkit для resolvectl). Или выполните от root: resolvectl dns rynet0 172.19.0.2; resolvectl domain rynet0 '~.'; resolvectl default-route rynet0 yes",
                    "Grant the permission again in the app: Settings → Network → \"Allow \"All traffic\" mode\" (installs a polkit rule for resolvectl). Or run as root: resolvectl dns rynet0 172.19.0.2; resolvectl domain rynet0 '~.'; resolvectl default-route rynet0 yes"
                )
            );
            ""
        }
    };
    tracing::info!("tun {IFACE} up, mtu {mtu}, kill switch {kill_switch}{dns}");
    check_bypass(&mark);
    Ok(fd)
}

pub async fn config(
    mtu: usize,
    kill_switch: bool,
    ipv4_only: bool,
    network: tokio::sync::watch::Receiver<u64>,
) -> Result<rv_tun::Config, String> {
    let fd = up(mtu, kill_switch, ipv4_only)?;
    Ok(rv_tun::Config { fd, mtu, v4: Some(V4), v6: (!ipv4_only).then_some(V6), network, stack: rv_tun::Stack::User })
}

pub fn capture(nets: &[(IpAddr, u8)]) {
    let mut have = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    if *have == nets {
        return;
    }
    if capable() {
        let _ = raise_ambient();
    }
    let rule = |op: &str, (a, l): (IpAddr, u8)| {
        ip(&[if a.is_ipv4() { "-4" } else { "-6" }, "rule", op, "pref", "7591", "to", &format!("{a}/{l}"), "lookup", TABLE])
    };
    for &n in have.iter().filter(|n| !nets.contains(n)) {
        if let Err(e) = rule("del", n) {
            tracing::warn!("tun capture: {e}");
        }
        if n.0.is_ipv6() {
            let _ = ip(&["-6", "route", "del", &format!("{}/{}", n.0, n.1), "dev", IFACE, "table", TABLE]);
        }
    }
    have.retain(|n| nets.contains(n));
    for &n in nets {
        if have.contains(&n) {
            continue;
        }
        if n.0.is_ipv6()
            && let Err(e) = ip(&["-6", "route", "replace", &format!("{}/{}", n.0, n.1), "dev", IFACE, "table", TABLE])
        {
            tracing::warn!("tun capture: {e}");
            continue;
        }
        match rule("add", n) {
            Ok(()) => have.push(n),
            Err(e) => tracing::warn!("tun capture: {e}"),
        }
    }
    nft_nets(None, Some(&have));
}

pub fn direct(nets: &[(IpAddr, u8)]) {
    let mut have = DIRECT.lock().unwrap_or_else(|e| e.into_inner());
    let lan = lan_nets();
    let mut want: Vec<(IpAddr, u8)> = Vec::new();
    for &n in nets {
        if !want.contains(&n) && !lan.iter().any(|&l| crate::core::covers(l, n)) {
            want.push(n);
        }
    }
    if *have == want {
        return;
    }
    if capable() {
        let _ = raise_ambient();
    }
    let pref = PREF_DIRECT.to_string();
    let rule = |op: &str, (a, l): (IpAddr, u8)| {
        ip(&[if a.is_ipv4() { "-4" } else { "-6" }, "rule", op, "pref", &pref, "to", &format!("{a}/{l}"), "lookup", "main"])
    };
    for &n in have.iter().filter(|n| !want.contains(n)) {
        if let Err(e) = rule("del", n) {
            tracing::warn!("tun direct: {e}");
        }
    }
    have.retain(|n| want.contains(n));
    for &n in &want {
        if have.contains(&n) {
            continue;
        }
        match rule("add", n) {
            Ok(()) => have.push(n),
            Err(e) => tracing::warn!("tun direct: {e}"),
        }
    }
    nft_nets(Some(&have), None);
    let list: Vec<String> = have.iter().map(|(a, l)| format!("{a}/{l}")).collect();
    tracing::info!("tun direct nets bypass {IFACE}: {}", if list.is_empty() { "none".into() } else { list.join(" ") });
}

fn lan_nets() -> Vec<(IpAddr, u8)> {
    LAN.iter()
        .filter_map(|n| {
            let (a, l) = n.split_once('/')?;
            Some((a.parse().ok()?, l.parse().ok()?))
        })
        .collect()
}

fn masked((a, l): (IpAddr, u8)) -> (IpAddr, u8) {
    match a {
        IpAddr::V4(a) => {
            let m = u32::MAX.checked_shl(32 - u32::from(l.min(32))).unwrap_or(0);
            (IpAddr::V4(Ipv4Addr::from(u32::from(a) & m)), l.min(32))
        }
        IpAddr::V6(a) => {
            let m = u128::MAX.checked_shl(128 - u32::from(l.min(128))).unwrap_or(0);
            (IpAddr::V6(Ipv6Addr::from(u128::from(a) & m)), l.min(128))
        }
    }
}

fn set_elements(nets: &[(IpAddr, u8)], v6: bool) -> String {
    let mut own: Vec<(IpAddr, u8)> = Vec::new();
    for n in nets.iter().map(|&n| masked(n)).filter(|n| n.0.is_ipv6() == v6) {
        if !own.contains(&n) {
            own.push(n);
        }
    }
    let keep: Vec<String> =
        own.iter().filter(|&&n| !own.iter().any(|&o| o != n && crate::core::covers(o, n))).map(|(a, l)| format!("{a}/{l}")).collect();
    keep.join(", ")
}

fn nft_script(lan: &[(IpAddr, u8)], direct: &[(IpAddr, u8)], capture: &[(IpAddr, u8)], block: bool, dns_guard: bool) -> String {
    let (direct, capture) = if block { (&[][..], &[][..]) } else { (direct, capture) };
    let set = |name: &str, nets: &[(IpAddr, u8)], v6: bool| {
        let ty = if v6 { "ipv6_addr" } else { "ipv4_addr" };
        let el = set_elements(nets, v6);
        let el = if el.is_empty() { String::new() } else { format!(" elements = {{ {el} }}") };
        format!("\tset {name} {{ type {ty}; flags interval;{el} }}\n")
    };
    let lan4 = set_elements(lan, false);
    let mut s = format!("add table inet {NFT_TABLE}\ndelete table inet {NFT_TABLE}\ntable inet {NFT_TABLE} {{\n");
    s += &set("direct4", direct, false);
    s += &set("direct6", direct, true);
    s += &set("capture4", capture, false);
    s += &set("capture6", capture, true);
    s += "\tchain output {\n\t\ttype filter hook output priority 0; policy accept;\n";
    s += &format!(
        "\t\tmeta mark {MARK:#x} accept\n\t\toifname \"lo\" accept\n\t\toifname \"{IFACE}\" accept\n\t\tmeta oiftype 65534 accept\n"
    );
    s += "\t\tudp sport 68 udp dport 67 accept\n\t\tudp sport 546 udp dport 547 accept\n";
    s += "\t\ticmpv6 type { nd-router-solicit, nd-router-advert, nd-neighbor-solicit, nd-neighbor-advert } accept\n";
    if dns_guard {
        s += "\t\tmeta l4proto { tcp, udp } th dport 53 reject\n";
    }
    s += "\t\tip daddr @capture4 reject\n\t\tip6 daddr @capture6 reject\n";
    if !lan4.is_empty() {
        s += &format!("\t\tip daddr {{ {lan4} }} accept\n");
    }
    s += "\t\tip6 daddr { fe80::/10, ff00::/8 } accept\n";
    s += "\t\tip daddr @direct4 accept\n\t\tip6 daddr @direct6 accept\n";
    s += "\t\treject\n\t}\n}\n";
    s
}

fn nft(script: &str) -> io::Result<()> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = tool("nft").args(["-f", "-"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
    child.stdin.take().ok_or_else(|| io::Error::other("nft: no stdin"))?.write_all(script.as_bytes())?;
    let out = child.wait_with_output()?;
    if out.status.success() { Ok(()) } else { Err(io::Error::other(format!("nft: {}", String::from_utf8_lossy(&out.stderr).trim()))) }
}

fn nft_up(block: bool, dns_guard: bool) {
    let mut st = NFT.lock().unwrap_or_else(|e| e.into_inner());
    match nft(&nft_script(&lan_nets(), &st.direct, &st.capture, block, dns_guard)) {
        Ok(()) => {
            st.on = Some(block);
            st.dns_guard = dns_guard;
        }
        Err(e) => {
            st.on = None;
            tracing::warn!(
                "{}: {e}",
                rv_core::lang::t(
                    "nftables не принял правила kill switch: сокеты, привязанные к физическому интерфейсу, не блокируются (маршрутизацией kill switch держится)",
                    "nftables did not take the kill switch rules: sockets bound to the physical interface are not blocked (the kill switch still holds by routing)"
                )
            );
        }
    }
}

fn nft_nets(direct: Option<&[(IpAddr, u8)]>, capture: Option<&[(IpAddr, u8)]>) {
    let mut st = NFT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = direct {
        st.direct = d.to_vec();
    }
    if let Some(c) = capture {
        st.capture = c.to_vec();
    }
    let Some(block) = st.on else { return };
    if let Err(e) = nft(&nft_script(&lan_nets(), &st.direct, &st.capture, block, st.dns_guard)) {
        tracing::warn!("tun nft: {e}");
    }
}

fn nft_dns_open() {
    let (on, direct, capture) = {
        let st = NFT.lock().unwrap_or_else(|e| e.into_inner());
        (st.on, st.direct.clone(), st.capture.clone())
    };
    if let Some(block) = on {
        let mut st = NFT.lock().unwrap_or_else(|e| e.into_inner());
        st.direct = direct;
        st.capture = capture;
        match nft(&nft_script(&lan_nets(), &st.direct, &st.capture, block, false)) {
            Ok(()) => st.dns_guard = false,
            Err(e) => tracing::warn!("tun nft: {e}"),
        }
    }
}

fn nft_exists() -> bool {
    tool("nft").args(["list", "table", "inet", NFT_TABLE]).output().is_ok_and(|o| o.status.success())
}

fn nft_del() {
    let mut st = NFT.lock().unwrap_or_else(|e| e.into_inner());
    *st = Nft::default();
    if nft_exists() {
        let _ = tool("nft").args(["delete", "table", "inet", NFT_TABLE]).output();
    }
}

pub fn block() -> Result<(), String> {
    if !capable() {
        return Err(rv_core::lang::t("нет прав", "no permission").into());
    }
    raise_ambient().map_err(|e| e.to_string())?;
    guard();
    clear(true);
    let mark = format!("{MARK:#x}");
    let r = (|| -> io::Result<()> {
        ip(&["route", "replace", "blackhole", "default", "table", TABLE])?;
        ip(&["-6", "route", "replace", "unreachable", "default", "table", TABLE])?;
        rules(&mark)
    })();
    if r.is_ok() {
        nft_up(true, true);
    }
    match r {
        Ok(()) => {
            unguard();
            Ok(())
        }
        Err(e) => {
            clear(false);
            Err(e.to_string())
        }
    }
}

fn guard() {
    for v in ["-4", "-6"] {
        let _ = ip(&[v, "rule", "add", "pref", &PREF_GUARD.to_string(), "blackhole"]);
    }
}

fn unguard() {
    for v in ["-4", "-6"] {
        while ip(&[v, "rule", "del", "pref", &PREF_GUARD.to_string()]).is_ok() {}
    }
}

pub fn leftover() -> Option<bool> {
    let show = |args: &[&str]| tool("ip").args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let prefs: Vec<u32> = [show(&["-4", "rule", "show"]), show(&["-6", "rule", "show"])]
        .iter()
        .flat_map(|t| t.lines().filter_map(|l| l.split(':').next()?.trim().parse().ok()).collect::<Vec<u32>>())
        .collect();
    if !prefs.iter().any(|p| PREFS.contains(p) || *p == PREF_GUARD) {
        return nft_exists().then_some(true);
    }
    let blackhole = show(&["-4", "route", "show", "table", TABLE]).lines().any(|l| l.starts_with("blackhole"));
    Some(blackhole || prefs.contains(&PREF_GUARD) || nft_exists())
}

fn rules(mark: &str) -> io::Result<()> {
    for v in [&[][..], &["-6"][..]] {
        let rule = |args: &[&str]| ip(&[v, &["rule", "add"][..], args].concat());
        rule(&["pref", &PREF_MARK.to_string(), "fwmark", mark, "lookup", "main"])?;
        rule(&["pref", &PREF_MAIN.to_string(), "lookup", "main", "suppress_prefixlength", "0"])?;
        rule(&["pref", &PREF_TUN.to_string(), "lookup", TABLE])?;
    }
    Ok(())
}

fn check_bypass(mark: &str) {
    let Ok(out) = tool("ip").args(["-4", "route", "get", "1.1.1.1", "mark", mark]).output() else { return };
    let route = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || route.contains(&format!("dev {IFACE} ")) {
        let defaults = tool("ip")
            .args(["-4", "route", "show", "table", "all", "default"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.contains(IFACE)).map(str::trim).collect::<Vec<_>>().join("; ")
            })
            .unwrap_or_default();
        tracing::warn!("own traffic loops into {IFACE}: no default route in table main (defaults: {defaults})");
    }
}

pub fn down() {
    if capable() {
        let _ = raise_ambient();
    }
    if resolved() {
        let _ = resolvectl(&["revert", IFACE]);
    }
    clear(false);
}

fn clear(keep_guard: bool) {
    if keep_guard {
        *NFT.lock().unwrap_or_else(|e| e.into_inner()) = Nft::default();
    } else {
        nft_del();
    }
    CAPTURE.lock().unwrap_or_else(|e| e.into_inner()).clear();
    DIRECT.lock().unwrap_or_else(|e| e.into_inner()).clear();
    for v in ["-4", "-6"] {
        for pref in PREFS.chain((!keep_guard).then_some(PREF_GUARD)) {
            while tool("ip").args([v, "rule", "del", "pref", &pref.to_string()]).output().is_ok_and(|o| o.status.success()) {}
        }
        let _ = tool("ip").args([v, "route", "flush", "table", TABLE]).output();
    }
}

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

#[cfg(test)]
mod tests {
    #[test]
    fn resolved_detection() {
        use super::resolved_running as r;
        assert!(r(true, ""));
        assert!(r(false, "# stub\nnameserver 127.0.0.53\noptions edns0\n"));
        assert!(r(false, "  nameserver   127.0.0.53  "));
        assert!(!r(false, "nameserver 192.168.1.1\nnameserver 127.0.0.1\n"));
        assert!(!r(false, "# nameserver 127.0.0.53\n"));
        assert!(!r(false, ""));
    }

    #[test]
    fn resolved_args_route_all_names_to_the_peer() {
        let a = super::resolved_args("rynet0", super::V4.1);
        assert_eq!(a[0], ["dns", "rynet0", "172.19.0.2"]);
        assert_eq!(a[1], ["domain", "rynet0", "~."]);
        assert_eq!(a[2], ["default-route", "rynet0", "yes"]);
    }

    use super::*;

    fn n(s: &str) -> (IpAddr, u8) {
        let (a, l) = s.split_once('/').unwrap();
        (a.parse().unwrap(), l.parse().unwrap())
    }

    #[test]
    fn nft_script_holds_direct_and_capture_nets_in_sets() {
        let t = nft_script(
            &lan_nets(),
            &[n("100.64.0.0/10"), n("fd7a:115c:a1e0::/48"), n("198.51.100.7/24")],
            &[n("203.0.113.0/24")],
            false,
            true,
        );
        assert!(t.starts_with("add table inet rynet\ndelete table inet rynet\ntable inet rynet {"));
        assert!(t.contains("set direct4 { type ipv4_addr; flags interval; elements = { 100.64.0.0/10, 198.51.100.0/24 } }"), "{t}");
        assert!(t.contains("set direct6 { type ipv6_addr; flags interval; elements = { fd7a:115c:a1e0::/48 } }"));
        assert!(t.contains("set capture4 { type ipv4_addr; flags interval; elements = { 203.0.113.0/24 } }"));
        assert!(t.contains("set capture6 { type ipv6_addr; flags interval; }"));
        assert!(t.contains("meta mark 0x2d0 accept") && t.contains("oifname \"rynet0\" accept"));
        assert!(t.contains("udp sport 68 udp dport 67 accept") && t.contains("nd-neighbor-advert"));
        assert!(t.contains("ip daddr { 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, 224.0.0.0/4 } accept"));
        assert!(t.trim_end().ends_with("\t\treject\n\t}\n}"));
        assert!(t.find("@capture4 reject").unwrap() < t.find("ip daddr { 10.0.0.0/8").unwrap());
        assert!(t.find("th dport 53 reject").unwrap() < t.find("ip daddr { 10.0.0.0/8").unwrap());
    }

    #[test]
    fn nft_script_dns_guard_is_optional() {
        let on = nft_script(&lan_nets(), &[], &[], false, true);
        let off = nft_script(&lan_nets(), &[], &[], false, false);
        assert!(on.contains("th dport 53 reject") && !off.contains("th dport 53"));
        assert_eq!(on.replace("\t\tmeta l4proto { tcp, udp } th dport 53 reject\n", ""), off);
    }

    #[test]
    fn nft_script_block_mode_has_no_direct_or_capture() {
        let t = nft_script(&lan_nets(), &[n("100.64.0.0/10")], &[n("203.0.113.0/24")], true, true);
        assert!(!t.contains("100.64.0.0") && !t.contains("203.0.113"));
        assert!(t.contains("meta mark 0x2d0 accept"));
    }

    #[test]
    fn nft_set_elements_drop_overlaps_and_host_bits() {
        let all = [n("10.1.2.3/8"), n("10.0.0.0/8"), n("10.5.0.0/16"), n("192.0.2.1/32"), n("2001:db8::1/32")];
        assert_eq!(set_elements(&all, false), "10.0.0.0/8, 192.0.2.1/32");
        assert_eq!(set_elements(&all, true), "2001:db8::/32");
        assert_eq!(set_elements(&[n("0.0.0.0/0")], false), "0.0.0.0/0");
    }

    fn nft_table() -> std::process::Output {
        tool("nft").args(["list", "table", "inet", NFT_TABLE]).output().unwrap()
    }

    fn show(args: &[&str]) -> String {
        tool("ip").args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
    }

    #[test]
    #[ignore = "run in an isolated fresh user/network namespace with CAP_NET_ADMIN"]
    fn ipv4_only_tun_with_ipv6_disabled() {
        assert!(capable());
        assert!(show(&["route", "show", "table", "all"]).trim().is_empty(), "fresh network namespace required");
        std::fs::write("/proc/sys/net/ipv6/conf/default/disable_ipv6", "1").unwrap();
        std::fs::write("/proc/sys/net/ipv6/conf/all/disable_ipv6", "1").unwrap();
        let fd = up(1500, false, true).unwrap();
        assert!(show(&["-6", "addr", "show", "dev", IFACE]).trim().is_empty());
        unsafe { libc::close(fd) };
        down();
    }

    fn dev(a: &str) -> String {
        show(&["route", "get", a]).split_whitespace().skip_while(|w| *w != "dev").nth(1).unwrap_or("unreachable").to_string()
    }

    fn ours(v: &str) -> Vec<String> {
        show(&[v, "rule", "show"])
            .lines()
            .filter(|l| l.split(':').next().and_then(|p| p.trim().parse().ok()).is_some_and(|p: u32| PREFS.contains(&p) || p == PREF_GUARD))
            .map(str::to_string)
            .collect()
    }

    fn direct_rules(v: &str) -> usize {
        ours(v).iter().filter(|l| l.starts_with(&format!("{PREF_DIRECT}:"))).count()
    }

    #[test]
    #[ignore = "нужен unshare -rn"]
    fn direct_rules_in_netns() {
        let fresh = !show(&["link", "show", "lo"]).contains("UP") && show(&["route", "show", "table", "all"]).trim().is_empty();
        assert!(capable() && fresh, "запускать под unshare -rn: {}", show(&["route", "show", "table", "all"]));
        for args in [
            &["link", "set", "lo", "up"][..],
            &["link", "add", "phys0", "type", "dummy"],
            &["link", "set", "phys0", "up"],
            &["addr", "add", "192.0.2.2/24", "dev", "phys0"],
            &["route", "add", "default", "via", "192.0.2.1"],
            &["-6", "addr", "add", "2001:db8::2/64", "dev", "phys0", "nodad"],
            &["-6", "route", "add", "default", "via", "2001:db8::1"],
        ] {
            ip(args).unwrap();
        }
        let net = |s: &str| -> (IpAddr, u8) {
            let (a, l) = s.split_once('/').unwrap();
            (a.parse().unwrap(), l.parse().unwrap())
        };
        for kill_switch in [false, true] {
            let fd = up(1500, kill_switch, false).unwrap();
            assert_eq!(dev("100.64.1.1"), IFACE);
            assert_eq!(dev("fd7a:115c:a1e0::1"), "unreachable");
            direct(&[net("100.64.0.0/10"), net("fd7a:115c:a1e0::/48"), net("10.1.0.0/16"), net("198.51.100.0/24"), net("100.64.0.0/10")]);
            assert_eq!((direct_rules("-4"), direct_rules("-6")), (2, 1), "{:?}", ours("-4"));
            for (a, want) in [
                ("100.64.1.1", "phys0"),
                ("fd7a:115c:a1e0::1", "phys0"),
                ("198.51.100.9", "phys0"),
                ("10.1.2.3", "phys0"),
                ("1.1.1.1", IFACE),
                ("2606:4700::1", "unreachable"),
            ] {
                assert_eq!(dev(a), want, "{a}, kill switch {kill_switch}");
            }
            assert_eq!(nft_table().status.success(), kill_switch);
            if kill_switch {
                let text = String::from_utf8_lossy(&nft_table().stdout).into_owned();
                assert!(
                    text.contains("100.64.0.0/10") && text.contains("198.51.100.0/24") && text.contains("fd7a:115c:a1e0::/48"),
                    "{text}"
                );
                assert!(text.contains("oifname \"rynet0\" accept"), "{text}");
            }
            direct(&[net("100.64.0.0/10"), net("203.0.113.0/24")]);
            if kill_switch {
                let text = String::from_utf8_lossy(&nft_table().stdout).into_owned();
                assert!(text.contains("203.0.113.0/24") && !text.contains("198.51.100.0/24"), "{text}");
            }
            assert_eq!((direct_rules("-4"), direct_rules("-6")), (2, 0), "{:?}", ours("-4"));
            for (a, want) in
                [("100.64.1.1", "phys0"), ("203.0.113.9", "phys0"), ("198.51.100.9", IFACE), ("fd7a:115c:a1e0::1", "unreachable")]
            {
                assert_eq!(dev(a), want, "{a}, kill switch {kill_switch}");
            }
            for n in ["100.64.0.0/10", "fd7a:115c:a1e0::/48"] {
                ip(&[if n.contains(':') { "-6" } else { "-4" }, "route", "add", n, "dev", "phys0"]).unwrap();
            }
            capture(&[net("100.64.0.0/10"), net("fd7a:115c:a1e0::/48"), net("10.1.0.0/16")]);
            for a in ["100.64.1.1", "fd7a:115c:a1e0::1", "10.1.2.3"] {
                assert_eq!(dev(a), IFACE, "capture {a}");
                assert!(show(&["route", "get", a, "mark", &format!("{MARK:#x}")]).contains("dev phys0"));
            }
            assert_eq!(dev("2606:4700::1"), "unreachable");
            capture(&[]);
            for a in ["100.64.1.1", "fd7a:115c:a1e0::1", "10.1.2.3"] {
                assert_eq!(dev(a), "phys0");
            }
            for n in ["100.64.0.0/10", "fd7a:115c:a1e0::/48"] {
                ip(&[if n.contains(':') { "-6" } else { "-4" }, "route", "del", n, "dev", "phys0"]).unwrap();
            }
            unsafe { libc::close(fd) };
            assert_eq!(leftover(), Some(kill_switch));
            down();
            assert_eq!(leftover(), None);
            assert!(!nft_table().status.success(), "таблица nftables снята");
            assert!(ours("-4").is_empty() && ours("-6").is_empty(), "{:?} {:?}", ours("-4"), ours("-6"));
            assert_eq!(dev("1.1.1.1"), "phys0");
        }
    }
}
