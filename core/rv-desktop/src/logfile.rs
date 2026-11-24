use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

const LOG_MAX: u64 = 8 << 20;
const LOG_KEEP: u64 = 5 << 20;
const CHECK_EVERY: u64 = 256 << 10;

pub struct LogFile {
    file: File,
    path: PathBuf,
    written: u64,
}

impl LogFile {
    pub fn new(file: File, path: PathBuf) -> Self {
        let mut log = Self { file, path, written: 0 };
        let _ = log.trim();
        log
    }

    #[cfg(target_os = "linux")]
    pub fn stderr() -> Option<Self> {
        use std::os::fd::AsFd;
        let path = std::fs::read_link("/proc/self/fd/2").ok()?;
        if !std::fs::metadata(&path).ok()?.is_file() {
            return None;
        }
        let file = File::from(io::stderr().as_fd().try_clone_to_owned().ok()?);
        Some(Self::new(file, path))
    }

    #[cfg(not(target_os = "linux"))]
    pub fn stderr() -> Option<Self> {
        None
    }

    fn trim(&mut self) -> io::Result<()> {
        let len = self.file.metadata()?.len();
        if len <= LOG_MAX {
            return Ok(());
        }
        let mut tail = Vec::with_capacity(LOG_KEEP as usize);
        let mut src = File::open(&self.path)?;
        src.seek(SeekFrom::Start(len - LOG_KEEP))?;
        src.take(LOG_KEEP).read_to_end(&mut tail)?;
        let start = tail.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
        std::fs::OpenOptions::new().write(true).open(&self.path)?.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&tail[start..])
    }
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.file.write(buf)?;
        self.written += n as u64;
        if self.written >= CHECK_EVERY {
            self.written = 0;
            let _ = self.trim();
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_tail_from_a_line_start() {
        let dir = std::env::temp_dir().join(format!("rynet-logfile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("core.log");
        let line = "x".repeat(99) + "\n";
        let file = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&path).unwrap();
        let mut log = LogFile::new(file, path.clone());
        let mut total = 0u64;
        while total < LOG_MAX + 2 * CHECK_EVERY {
            log.write_all(line.as_bytes()).unwrap();
            total += line.len() as u64;
        }
        log.write_all(b"last\n").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!((text.len() as u64) < LOG_MAX, "{} bytes left", text.len());
        assert!(text.len() as u64 >= LOG_KEEP - 200);
        assert!(text.starts_with(&line), "cut mid-line");
        assert!(text.ends_with("last\n"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
