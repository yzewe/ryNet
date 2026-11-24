use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn portable() -> Option<PathBuf> {
    std::env::var_os("RYNET_HOME").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

pub fn config_dir() -> PathBuf {
    let d = match portable() {
        Some(d) => d,
        None => std::env::var_os(if cfg!(windows) { "APPDATA" } else { "XDG_CONFIG_HOME" })
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
            .join("rynet"),
    };
    let _ = std::fs::create_dir_all(&d);
    #[cfg(unix)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = std::fs::set_permissions(&d, std::os::unix::fs::PermissionsExt::from_mode(0o700));
        });
    }
    d
}

pub fn plugins_dir() -> PathBuf {
    config_dir().join("plugins")
}

pub fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(&tmp)?;
    #[cfg(unix)]
    f.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    f.write_all(data)?;
    drop(f);
    std::fs::rename(&tmp, path)
}

pub fn geo_dir() -> PathBuf {
    if let Some(d) = portable() {
        let d = d.join("geo");
        let _ = std::fs::create_dir_all(&d);
        return d;
    }
    let base = std::env::var_os(if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_CACHE_HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"));
    let d = base.join("rynet").join("geo");
    let _ = std::fs::create_dir_all(&d);
    d
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sub {
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: u64,
    pub updated: u64,
    pub interval_hours: Option<u32>,
    pub announce: Option<String>,
    pub routing: Option<String>,
    pub support_url: Option<String>,
    pub web_page: Option<String>,
    pub client_override: Option<String>,
    pub headers_override: Option<String>,
    pub update_interval_minutes: Option<u32>,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

pub fn write(path: PathBuf, text: &str) {
    let _ = write_private(&path, text.as_bytes());
}

pub fn hwid() -> String {
    let path = config_dir().join("hwid");
    if let Ok(h) = std::fs::read_to_string(&path)
        && h.trim().len() == 16
    {
        return h.trim().to_string();
    }
    let h: String = new_id().chars().take(16).collect();
    write(path, &h);
    h
}

pub fn new_id() -> String {
    let mut bytes = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).expect("OS random source");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
