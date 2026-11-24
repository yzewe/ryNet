#[cfg(unix)]
use std::fs::OpenOptions;
use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering::Relaxed};

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum Slot {
    Generation = 1,
    Started = 2,
    Up = 3,
    Down = 4,
    TcpActive = 5,
    UdpActive = 6,
    TcpTotal = 7,
    DnsQueries = 8,
    ErrTimeout = 9,
    ErrTls = 10,
    ErrReset = 11,
    ErrRefused = 12,
    ErrOther = 13,
    TunDropped = 14,
    Handshakes = 15,
    Pings = 16,
    ProxyConns = 17,
    ProxyOk = 18,
    ProxyFail = 19,
    KeepAlive = 20,
}

const SLOTS: usize = 32;
#[cfg(unix)]
const SIZE: usize = 4096;
#[cfg(unix)]
const MAGIC: u64 = u64::from_le_bytes(*b"RYNETST1");

static FALLBACK: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static SHARED: AtomicPtr<AtomicU64> = AtomicPtr::new(ptr::null_mut());

fn slot(s: Slot) -> &'static AtomicU64 {
    let p = SHARED.load(Relaxed);
    if p.is_null() { &FALLBACK[s as usize] } else { unsafe { &*p.add(s as usize) } }
}

pub fn add(s: Slot, n: u64) {
    slot(s).fetch_add(n, Relaxed);
}

pub fn sub(s: Slot, n: u64) {
    slot(s).fetch_sub(n, Relaxed);
}

pub fn get(s: Slot) -> u64 {
    slot(s).load(Relaxed)
}

#[cfg(unix)]
pub fn map(path: &Path) -> io::Result<()> {
    if !SHARED.load(Relaxed).is_null() {
        return Ok(());
    }
    let f = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    f.set_len(SIZE as u64)?;
    let p = unsafe { libc::mmap(ptr::null_mut(), SIZE, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0) };
    if p == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }
    let base = p.cast::<AtomicU64>();
    unsafe { (*base).store(MAGIC, Relaxed) };
    if SHARED.compare_exchange(ptr::null_mut(), base, Relaxed, Relaxed).is_err() {
        unsafe { libc::munmap(p, SIZE) };
    } else {
        for s in [Slot::TcpActive, Slot::UdpActive, Slot::KeepAlive] {
            slot(s).store(0, Relaxed);
        }
    }
    Ok(())
}

pub fn reset(started_ms: u64) {
    for s in [
        Slot::Up,
        Slot::Down,
        Slot::TcpTotal,
        Slot::DnsQueries,
        Slot::TunDropped,
        Slot::Handshakes,
        Slot::Pings,
        Slot::ProxyConns,
        Slot::ProxyOk,
        Slot::ProxyFail,
    ] {
        slot(s).store(0, Relaxed);
    }
    slot(Slot::Started).store(started_ms, Relaxed);
    add(Slot::Generation, 1);
}

pub struct Active(Slot);

impl Active {
    pub fn new(s: Slot) -> Self {
        add(s, 1);
        Active(s)
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        sub(self.0, 1);
    }
}

pub struct KeepAlive(());

static KEEPALIVE_LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
type Hook = std::sync::Arc<dyn Fn(bool) + Send + Sync>;
static KEEPALIVE_HOOK: std::sync::Mutex<Option<Hook>> = std::sync::Mutex::new(None);

fn keepalive_changed(live: bool) {
    let hook = KEEPALIVE_HOOK.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(f) = hook {
        f(live);
    }
}

impl KeepAlive {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        add(Slot::KeepAlive, 1);
        if KEEPALIVE_LIVE.fetch_add(1, Relaxed) == 0 {
            keepalive_changed(true);
        }
        KeepAlive(())
    }
}

impl Drop for KeepAlive {
    fn drop(&mut self) {
        let s = slot(Slot::KeepAlive);
        let _ = s.fetch_update(Relaxed, Relaxed, |v| Some(v.saturating_sub(1)));
        if KEEPALIVE_LIVE.fetch_sub(1, Relaxed) == 1 {
            keepalive_changed(false);
        }
    }
}

pub fn keepalive_live() -> usize {
    KEEPALIVE_LIVE.load(Relaxed)
}

pub fn on_keepalive(hook: Option<Hook>) {
    *KEEPALIVE_HOOK.lock().unwrap_or_else(|e| e.into_inner()) = hook;
}

#[cfg(windows)]
pub fn map(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "shared-file counters are not used by the Windows desktop core"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn shared_file_layout() {
        let path = std::env::temp_dir().join(format!("rv-stats-{}", std::process::id()));
        map(&path).unwrap();
        reset(1234);
        add(Slot::Up, 10);
        add(Slot::Down, 7);
        add(Slot::TunDropped, 2);
        {
            let _a = Active::new(Slot::TcpActive);
            assert_eq!(get(Slot::TcpActive), 1);
        }
        assert_eq!(get(Slot::TcpActive), 0);
        let bytes = std::fs::read(&path).unwrap();
        let at = |i: usize| u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
        assert_eq!(bytes.len(), SIZE);
        assert_eq!(at(0), MAGIC);
        assert_eq!(at(Slot::Started as usize), 1234);
        assert_eq!(at(Slot::Up as usize), 10);
        assert_eq!(at(Slot::Down as usize), 7);
        assert_eq!(at(Slot::TunDropped as usize), 2);
        let _ = std::fs::remove_file(path);

        let a = Active::new(Slot::UdpActive);
        reset(1);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        on_keepalive(Some(std::sync::Arc::new(move |on| s2.lock().unwrap().push(on))));
        let k1 = KeepAlive::new();
        let k2 = KeepAlive::new();
        assert_eq!((keepalive_live(), get(Slot::KeepAlive)), (2, 2));
        drop(k1);
        drop(k2);
        let k3 = KeepAlive::new();
        drop(k3);
        on_keepalive(None);
        assert_eq!(*seen.lock().unwrap(), [true, false, true, false], "только переходы 0 ↔ 1");
        assert_eq!(get(Slot::KeepAlive), 0);
        drop(a);
        assert_eq!(get(Slot::UdpActive), 0);
        assert_eq!(get(Slot::TunDropped), 0);
    }
}
