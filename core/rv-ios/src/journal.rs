use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

const ROTATE_AT: u64 = 16 << 20;

struct Sink {
    w: BufWriter<File>,
    path: PathBuf,
    written: u64,
}

struct Journal {
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
    let j = JOURNAL.get_or_init(|| Journal { file: Mutex::new(None), verbose: AtomicBool::new(false) });
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
    log::set_max_level(if on { LevelFilter::Debug } else { LevelFilter::Warn });
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
