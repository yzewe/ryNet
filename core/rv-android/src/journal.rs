use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

const ROTATE_AT: u64 = 16 << 20;
const FLUSH_AFTER: Duration = Duration::from_secs(3);
static DIRTY: AtomicBool = AtomicBool::new(false);
static FLUSHER: OnceLock<std::sync::mpsc::SyncSender<()>> = OnceLock::new();

fn flush_later() {
    if DIRTY.swap(true, Relaxed) {
        return;
    }
    let tx = FLUSHER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);
        let _ = std::thread::Builder::new().name("rv-log-flush".into()).spawn(move || {
            while rx.recv().is_ok() {
                std::thread::sleep(FLUSH_AFTER);
                DIRTY.store(false, Relaxed);
                flush();
            }
        });
        tx
    });
    let _ = tx.try_send(());
}

struct Sink {
    w: BufWriter<File>,
    path: PathBuf,
    written: u64,
}

struct Journal {
    logcat: android_logger::AndroidLogger,
    file: Mutex<Option<Sink>>,
    verbose: AtomicBool,
}

static JOURNAL: OnceLock<Journal> = OnceLock::new();

fn ours(target: &str) -> bool {
    target.starts_with("rv")
}

impl Log for Journal {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= Level::Warn || (self.verbose.load(Relaxed) && (ours(m.target()) || m.level() <= Level::Info))
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        if r.level() <= Level::Warn {
            self.logcat.log(r);
        }
        if !self.verbose.load(Relaxed) {
            return;
        }
        let mut g = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sink) = g.as_mut() else { return };
        let line = format!("{} {} {}: {}\n", timestamp(SystemTime::now()), level(r.level()), r.target(), r.args());
        if sink.w.write_all(line.as_bytes()).is_err() {
            return;
        }
        sink.written += line.len() as u64;
        if r.level() <= Level::Warn {
            let _ = sink.w.flush();
        } else {
            flush_later();
        }
        if sink.written >= ROTATE_AT {
            let _ = sink.w.flush();
            let _ = std::fs::rename(&sink.path, sink.path.with_extension("1.log"));
            if let Ok(f) = open(&sink.path) {
                *sink = Sink { w: BufWriter::with_capacity(32 << 10, f), path: sink.path.clone(), written: 0 };
            }
        }
    }

    fn flush(&self) {
        if let Some(s) = self.file.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = s.w.flush();
        }
    }
}

fn level(l: Level) -> char {
    match l {
        Level::Error => 'E',
        Level::Warn => 'W',
        Level::Info => 'I',
        Level::Debug => 'D',
        Level::Trace => 'T',
    }
}

fn open(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn init() {
    let j = JOURNAL.get_or_init(|| Journal {
        logcat: android_logger::AndroidLogger::new(android_logger::Config::default().with_tag("rynet").with_max_level(LevelFilter::Warn)),
        file: Mutex::new(None),
        verbose: AtomicBool::new(false),
    });
    if log::set_logger(j).is_ok() {
        log::set_max_level(LevelFilter::Warn);
    }
}

pub fn set_file(dir: Option<&Path>) {
    let Some(j) = JOURNAL.get() else { return };
    let sink = dir.and_then(|d| {
        let _ = std::fs::create_dir_all(d);
        let path = d.join("core.log");
        let f = open(&path).ok()?;
        let written = f.metadata().map(|m| m.len()).unwrap_or(0);
        Some(Sink { w: BufWriter::with_capacity(32 << 10, f), path, written })
    });
    let on = sink.is_some();
    let old = std::mem::replace(&mut *j.file.lock().unwrap_or_else(|e| e.into_inner()), sink);
    if let Some(mut s) = old {
        let _ = s.w.flush();
    }
    j.verbose.store(on, Relaxed);
    if on {
        SAMPLER.notify_one();
    }
    log::set_max_level(if on { LevelFilter::Debug } else { LevelFilter::Warn });
}

pub fn verbose() -> bool {
    JOURNAL.get().is_some_and(|j| j.verbose.load(Relaxed))
}

pub fn flush() {
    if let Some(j) = JOURNAL.get() {
        j.flush();
    }
}

fn timestamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, d.subsec_millis())
}

static SAMPLER: tokio::sync::Notify = tokio::sync::Notify::const_new();
const MEM_EVERY: Duration = Duration::from_secs(15 * 60);
const SAMPLE_EVERY: Duration = Duration::from_secs(60);
const SAMPLE_EVERY_QUIET: Duration = Duration::from_secs(5 * 60);

fn sample_every() -> Duration {
    if rv_core::engine::quiet() { SAMPLE_EVERY_QUIET } else { SAMPLE_EVERY }
}

pub fn wake_sampler() {
    SAMPLER.notify_one();
}

pub async fn sampler() {
    let tid = unsafe { libc::gettid() };
    let tick = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
    let mut last = tokio::time::Instant::now();
    let mut mem_at: Option<tokio::time::Instant> = None;
    loop {
        if !verbose() {
            SAMPLER.notified().await;
            last = tokio::time::Instant::now();
            continue;
        }
        tokio::select! {
            _ = tokio::time::sleep_until(last + sample_every()) => {}
            _ = SAMPLER.notified() => continue,
        }
        if !verbose() {
            continue;
        }
        let now = tokio::time::Instant::now();
        last = now;
        let mem = mem_at.is_none_or(|t| now - t >= MEM_EVERY);
        if mem {
            mem_at = Some(now);
        }
        sample(tid, tick, mem);
        flush();
    }
}

pub fn sample(tid: i32, tick: u64, mem: bool) {
    use rv_stats::{Slot, get};
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit_once(')').map(|(_, r)| r.split_whitespace().collect()).unwrap_or_default();
    let ticks = |i: usize| fields.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1000 / tick;
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |text: &str, key: &str| {
        text.lines().find_map(|l| l.strip_prefix(key)).map(|v| v.trim().trim_end_matches(" kB").to_string()).unwrap_or_default()
    };
    let thread = std::fs::read_to_string(format!("/proc/self/task/{tid}/status")).unwrap_or_default();
    let tstat = std::fs::read_to_string(format!("/proc/self/task/{tid}/stat")).unwrap_or_default();
    let tfields: Vec<&str> = tstat.rsplit_once(')').map(|(_, r)| r.split_whitespace().collect()).unwrap_or_default();
    let tticks = |i: usize| tfields.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1000 / tick;
    let fds = std::fs::read_dir("/proc/self/fd").map(|d| d.count()).unwrap_or(0);
    if mem {
        let rollup = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
        let native_kb = unsafe { libc::mallinfo() }.uordblks as u64 / 1024;
        log::debug!(
            target: "rv::sample",
            "mem pss_kb={} private_dirty_kb={} private_clean_kb={} anon_kb={} native_kb={native_kb} err_timeout={} err_tls={} err_reset={} err_refused={} err_other={}",
            field(&rollup, "Pss:"),
            field(&rollup, "Private_Dirty:"),
            field(&rollup, "Private_Clean:"),
            field(&rollup, "Anonymous:"),
            get(Slot::ErrTimeout),
            get(Slot::ErrTls),
            get(Slot::ErrReset),
            get(Slot::ErrRefused),
            get(Slot::ErrOther),
        );
    }
    log::debug!(
        target: "rv::sample",
        "cpu_user_ms={} cpu_sys_ms={} rss_kb={} threads={} fds={} core_wakeups={} core_preempt={} tcp={} udp={} tcp_total={} dns={} up={} down={} core_user_ms={} core_sys_ms={} handshakes={} pings={} proxy_conns={} proxy_ok={} proxy_fail={} keepalive={} quiet={}",
        ticks(11),
        ticks(12),
        field(&status, "VmRSS:"),
        field(&status, "Threads:"),
        fds,
        field(&thread, "voluntary_ctxt_switches:"),
        field(&thread, "nonvoluntary_ctxt_switches:"),
        get(Slot::TcpActive),
        get(Slot::UdpActive),
        get(Slot::TcpTotal),
        get(Slot::DnsQueries),
        get(Slot::Up),
        get(Slot::Down),
        tticks(11),
        tticks(12),
        get(Slot::Handshakes),
        get(Slot::Pings),
        get(Slot::ProxyConns),
        get(Slot::ProxyOk),
        get(Slot::ProxyFail),
        get(Slot::KeepAlive),
        u8::from(rv_core::engine::quiet()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    static FILE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn file_levels() {
        let _g = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("rv-journal-{}", std::process::id()));
        init();
        set_file(Some(&dir));
        log::debug!(target: "rv::conn", "tcp one");
        log::debug!(target: "h2::codec", "noise");
        log::info!(target: "quinn", "quic info");
        flush();
        set_file(None);
        log::warn!(target: "rv::core", "after");
        let text = std::fs::read_to_string(dir.join("core.log")).unwrap();
        assert!(text.contains(" D rv::conn: tcp one\n"), "{text}");
        assert!(!text.contains("noise"));
        assert!(text.contains(" I quinn: quic info"));
        assert!(!text.contains("after"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn quiet_tail_is_flushed_without_explicit_flush() {
        let _g = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("rv-journal-tail-{}", std::process::id()));
        init();
        set_file(Some(&dir));
        log::debug!(target: "rv::conn", "last line before a kill");
        let text = || std::fs::read_to_string(dir.join("core.log")).unwrap_or_default();
        assert!(!text().contains("last line before a kill"), "buffered, not written per line");
        std::thread::sleep(FLUSH_AFTER + Duration::from_millis(700));
        assert!(text().contains("last line before a kill"), "{}", text());
        set_file(None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn utc_timestamp() {
        let t = UNIX_EPOCH + Duration::from_millis(1_758_708_672_345);
        assert_eq!(timestamp(t), "2025-09-24T10:11:12.345Z");
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(timestamp(leap), "2000-02-29T00:00:00.000Z");
    }
}
