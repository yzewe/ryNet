mod crypto;
mod host;
mod instance;
pub mod manifest;
mod net;
pub mod outbound;
mod res;
mod tun;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use manifest::{Kind, Manifest, Permissions};
pub use rv_plugin_abi::{FILE_EXTENSION, FILE_MIME};

pub const MAX_MODULE: usize = 16 << 20;
const LOG_LINES: usize = 300;
const LOG_FILE_MAX: u64 = 128 << 10;
const KV_MAX: usize = 1 << 20;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Grants {
    pub enabled: bool,
    #[serde(default)]
    pub denied: Vec<String>,
    #[serde(default)]
    pub global: Map<String, Value>,
}

#[derive(Default)]
pub struct Diag {
    logs: Mutex<VecDeque<(u64, u32, String)>>,
    file: Option<PathBuf>,
    pub traps: AtomicU32,
    pub starts: AtomicU32,
    pub conns: AtomicU64,
    pub failures: AtomicU64,
    last_error: Mutex<String>,
    recent_traps: Mutex<VecDeque<std::time::Instant>>,
}

fn append_log(path: &Path, line: &str) {
    use std::io::Write;
    let big = std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_FILE_MAX);
    if big && let Ok(old) = std::fs::read(path) {
        let half = old.len() / 2;
        let cut = old[half..].iter().position(|&b| b == b'\n').map_or(old.len(), |i| half + i + 1);
        let _ = write_private(path, &old[cut..]);
    }
    let mut o = std::fs::OpenOptions::new();
    o.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    if let Ok(mut f) = o.open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Diag {
    pub fn log(&self, level: u32, msg: &str) {
        let mut l = self.logs.lock().unwrap_or_else(|e| e.into_inner());
        if l.len() >= LOG_LINES {
            l.pop_front();
        }
        let mut m = msg.to_string();
        if m.len() > 1024 {
            let mut cut = 1024;
            while !m.is_char_boundary(cut) {
                cut -= 1;
            }
            m.truncate(cut);
            m.push('…');
        }
        let at = unix_ms();
        if level <= rv_plugin_abi::log::INFO
            && let Some(f) = &self.file
        {
            append_log(f, &json!({ "time": at, "level": level, "message": m }).to_string());
        }
        l.push_back((at, level, m));
    }

    pub fn error(&self, msg: &str) {
        self.log(rv_plugin_abi::log::ERROR, msg);
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = msg.to_string();
    }

    pub fn trapped(&self, msg: &str) {
        self.traps.fetch_add(1, Relaxed);
        self.error(msg);
        let mut r = self.recent_traps.lock().unwrap_or_else(|e| e.into_inner());
        let now = std::time::Instant::now();
        r.push_back(now);
        while r.front().is_some_and(|t| now.duration_since(*t) > std::time::Duration::from_secs(60)) {
            r.pop_front();
        }
    }

    pub fn resting(&self) -> bool {
        let r = self.recent_traps.lock().unwrap_or_else(|e| e.into_inner());
        r.len() >= 3 && r.back().is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(60))
    }

    fn json(&self) -> Value {
        json!({
            "traps": self.traps.load(Relaxed),
            "starts": self.starts.load(Relaxed),
            "connections": self.conns.load(Relaxed),
            "failures": self.failures.load(Relaxed),
            "last_error": *self.last_error.lock().unwrap_or_else(|e| e.into_inner()),
            "resting": self.resting(),
        })
    }
}

pub struct Plugin {
    pub manifest: Manifest,
    pub sha256: String,
    pub size: usize,
    module: wasmi::Module,
    grants: RwLock<Grants>,
    pub diag: Diag,
    kv: Mutex<Option<Map<String, Value>>>,
    dir: Option<PathBuf>,
}

impl Plugin {
    pub fn grants(&self) -> Grants {
        self.grants.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn enabled(&self) -> bool {
        self.grants.read().unwrap_or_else(|e| e.into_inner()).enabled
    }

    pub fn permissions(&self) -> Permissions {
        let g = self.grants.read().unwrap_or_else(|e| e.into_inner());
        let mut p = self.manifest.permissions.clone();
        let no = |k: &str| g.denied.iter().any(|d| d == k);
        p.net_any &= !no("net_any");
        p.net_lan &= !no("net_lan");
        p.keepalive &= !no("keepalive");
        if no("memory") {
            p.memory_mb = p.memory_mb.min(manifest::MEMORY_SAFE_MB);
        }
        if no("storage") {
            p.storage = false;
        }
        p
    }

    fn kv_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{}.kv.json", self.manifest.id)))
    }

    fn kv<R>(&self, f: impl FnOnce(&mut Map<String, Value>) -> R) -> R {
        let mut g = self.kv.lock().unwrap_or_else(|e| e.into_inner());
        let m = g.get_or_insert_with(|| {
            self.kv_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
        });
        f(m)
    }

    pub(crate) fn kv_get(&self, key: &str) -> Option<Vec<u8>> {
        use base64::Engine;
        self.kv(|m| m.get(key)?.as_str().and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok()))
    }

    pub(crate) fn kv_set(&self, key: &str, value: Option<&[u8]>) -> Result<(), String> {
        use base64::Engine;
        let text = self.kv(|m| {
            match value {
                Some(v) => {
                    m.insert(key.to_string(), Value::String(base64::engine::general_purpose::STANDARD.encode(v)));
                }
                None => {
                    m.remove(key);
                }
            }
            let t = serde_json::to_vec(m).unwrap_or_default();
            if t.len() > KV_MAX {
                m.remove(key);
                return Err("storage is full (1 MB)".to_string());
            }
            Ok(t)
        })?;
        if let Some(p) = self.kv_path() {
            write_private(&p, &text).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn summary(&self) -> Value {
        let g = self.grants();
        json!({
            "manifest": self.manifest,
            "sha256": self.sha256,
            "size": self.size,
            "enabled": g.enabled,
            "denied": g.denied,
            "global": g.global,
            "dangerous": self.manifest.permissions.dangerous(),
            "diag": self.diag.json(),
        })
    }
}

fn engine() -> &'static wasmi::Engine {
    static E: std::sync::OnceLock<wasmi::Engine> = std::sync::OnceLock::new();
    E.get_or_init(|| {
        let mut c = wasmi::Config::default();
        c.consume_fuel(true)
            .compilation_mode(wasmi::CompilationMode::Lazy)
            .enforced_limits(wasmi::EnforcedLimits::strict())
            .set_max_recursion_depth(4096)
            .set_max_cached_stacks(4)
            .wasm_multi_memory(false)
            .wasm_custom_page_sizes(false);
        wasmi::Engine::new(&c)
    })
}

fn sha256_hex(b: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, b).as_ref().iter().map(|x| format!("{x:02x}")).collect()
}

fn load(bytes: &[u8]) -> Result<(Manifest, wasmi::Module), String> {
    if bytes.len() > MAX_MODULE {
        return Err(format!("plugin is over {} MB", MAX_MODULE >> 20));
    }
    let m = Manifest::parse(&manifest::extract(bytes)?)?;
    let module = wasmi::Module::new(engine(), bytes).map_err(|e| format!("bad module: {e}"))?;
    for i in module.imports() {
        if i.module() != rv_plugin_abi::IMPORT_MODULE {
            return Err(format!("imports `{}.{}`: only `{}` is available", i.module(), i.name(), rv_plugin_abi::IMPORT_MODULE));
        }
        if !host::NAMES.contains(&i.name()) {
            return Err(format!("imports `{}`, which this app doesn't have (newer plugin?)", i.name()));
        }
    }
    let exports: Vec<&str> = module.exports().map(|e| e.name()).collect();
    for need in ["memory", "rn_init", "rn_event"] {
        if !exports.contains(&need) {
            return Err(format!("no `{need}` export"));
        }
    }
    if !m.schemes.is_empty() && !exports.contains(&"rn_parse_link") {
        return Err("declares link schemes but has no `rn_parse_link`".into());
    }
    Ok((m, module))
}

struct Registry {
    dir: Option<PathBuf>,
    plugins: Vec<Arc<Plugin>>,
}

static REGISTRY: RwLock<Registry> = RwLock::new(Registry { dir: None, plugins: Vec::new() });

fn registry() -> std::sync::RwLockReadGuard<'static, Registry> {
    REGISTRY.read().unwrap_or_else(|e| e.into_inner())
}

fn registry_mut() -> std::sync::RwLockWriteGuard<'static, Registry> {
    REGISTRY.write().unwrap_or_else(|e| e.into_inner())
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

fn grants_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

pub fn init(dir: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    let mut plugins = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("wasm") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            match load(&bytes) {
                Ok((m, module)) => {
                    if p.file_stem().and_then(|s| s.to_str()) != Some(m.id.as_str()) {
                        errors.push(format!("{}: file name doesn't match id {}", p.display(), m.id));
                        continue;
                    }
                    let grants =
                        std::fs::read(grants_path(dir, &m.id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                    let log = dir.join(format!("{}.log", m.id));
                    plugins.push(Arc::new(Plugin {
                        sha256: sha256_hex(&bytes),
                        size: bytes.len(),
                        manifest: m,
                        module,
                        grants: RwLock::new(grants),
                        diag: Diag { file: Some(log), ..Default::default() },
                        kv: Mutex::new(None),
                        dir: Some(dir.to_path_buf()),
                    }));
                }
                Err(e) => errors.push(format!("{}: {e}", p.display())),
            }
        }
    }
    plugins.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    let mut r = registry_mut();
    r.dir = Some(dir.to_path_buf());
    r.plugins = plugins;
    drop(r);
    rv_config::links::set_link_hook(outbound::parse_link);
    errors
}

pub fn for_protocol(protocol: &str) -> Option<Arc<Plugin>> {
    registry()
        .plugins
        .iter()
        .find(|p| p.enabled() && p.manifest.kind != Kind::Transport && p.manifest.protocols.iter().any(|x| x == protocol))
        .cloned()
}

pub fn for_transport(name: &str) -> Option<Arc<Plugin>> {
    registry()
        .plugins
        .iter()
        .find(|p| p.enabled() && p.manifest.kind == Kind::Transport && p.manifest.protocols.iter().any(|x| x == name))
        .cloned()
}

pub fn by_id(id: &str) -> Option<Arc<Plugin>> {
    registry().plugins.iter().find(|p| p.manifest.id == id).cloned()
}

pub(crate) fn for_scheme(scheme: &str) -> Option<Arc<Plugin>> {
    registry().plugins.iter().find(|p| p.enabled() && p.manifest.schemes.iter().any(|s| s.eq_ignore_ascii_case(scheme))).cloned()
}

fn module_of(file: &[u8]) -> Result<&[u8], String> {
    match rv_plugin_abi::open_file(file) {
        rv_plugin_abi::File::Module(m) => Ok(m),
        rv_plugin_abi::File::Newer(v) => {
            Err(format!("the plugin file is format {v}, this app reads up to {}: update ryNet", rv_plugin_abi::FILE_VERSION))
        }
        rv_plugin_abi::File::Unknown => Err("not a ryNet plugin file (.rnp)".into()),
    }
}

pub fn pack(module: &[u8]) -> Result<(Manifest, Vec<u8>), String> {
    let module = module_of(module)?;
    let (m, _) = load(module)?;
    let mut out = Vec::with_capacity(rv_plugin_abi::FILE_MAGIC.len() + 1 + module.len());
    out.extend_from_slice(rv_plugin_abi::FILE_MAGIC);
    out.push(rv_plugin_abi::FILE_VERSION);
    out.extend_from_slice(module);
    Ok((m, out))
}

pub fn inspect(file: &[u8]) -> Result<Value, String> {
    let bytes = module_of(file)?;
    let (m, _) = load(bytes)?;
    let old = by_id(&m.id);
    let dangerous = m.permissions.dangerous();
    let new_dangerous: Vec<&str> = match &old {
        Some(o) => {
            let had = o.manifest.permissions.dangerous();
            dangerous.iter().copied().filter(|d| !had.contains(d)).collect()
        }
        None => dangerous.clone(),
    };
    let conflicts: Vec<String> = registry()
        .plugins
        .iter()
        .filter(|p| p.manifest.id != m.id)
        .flat_map(|p| p.manifest.protocols.iter().filter(|x| m.protocols.contains(x)).map(|x| format!("{x} ({})", p.manifest.name)))
        .collect();
    Ok(json!({
        "manifest": m,
        "sha256": sha256_hex(bytes),
        "size": bytes.len(),
        "dangerous": dangerous,
        "new_dangerous": new_dangerous,
        "installed": old.as_ref().map(|o| o.manifest.version.clone()),
        "conflicts": conflicts,
    }))
}

pub fn install(file: &[u8], denied: Vec<String>) -> Result<Value, String> {
    let bytes = module_of(file)?;
    let (m, module) = load(bytes)?;
    let dir = registry().dir.clone().ok_or("plugins are not set up")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let clash = registry()
        .plugins
        .iter()
        .find(|p| p.manifest.id != m.id && p.manifest.protocols.iter().any(|x| m.protocols.contains(x)))
        .map(|p| p.manifest.name.clone());
    let old = by_id(&m.id);
    let mut grants = old.as_ref().map(|o| o.grants()).unwrap_or(Grants { enabled: clash.is_none(), ..Default::default() });
    grants.denied = denied;
    write_private(&dir.join(format!("{}.wasm", m.id)), bytes).map_err(|e| e.to_string())?;
    let log = dir.join(format!("{}.log", m.id));
    write_private(&grants_path(&dir, &m.id), &serde_json::to_vec(&grants).unwrap_or_default()).map_err(|e| e.to_string())?;
    let p = Arc::new(Plugin {
        sha256: sha256_hex(bytes),
        size: bytes.len(),
        manifest: m,
        module,
        grants: RwLock::new(grants),
        diag: Diag { file: Some(log), ..Default::default() },
        kv: Mutex::new(None),
        dir: Some(dir),
    });
    let mut r = registry_mut();
    r.plugins.retain(|x| x.manifest.id != p.manifest.id);
    r.plugins.push(p.clone());
    r.plugins.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    Ok(p.summary())
}

pub fn remove(id: &str) -> Result<(), String> {
    let dir = registry().dir.clone().ok_or("plugins are not set up")?;
    let mut r = registry_mut();
    let before = r.plugins.len();
    r.plugins.retain(|p| p.manifest.id != id);
    if r.plugins.len() == before {
        return Err(format!("no plugin {id}"));
    }
    drop(r);
    for ext in ["wasm", "json", "kv.json", "log"] {
        let _ = std::fs::remove_file(dir.join(format!("{id}.{ext}")));
    }
    Ok(())
}

fn update(id: &str, f: impl FnOnce(&Plugin, &mut Grants) -> Result<(), String>) -> Result<Value, String> {
    let p = by_id(id).ok_or_else(|| format!("no plugin {id}"))?;
    let dir = registry().dir.clone().ok_or("plugins are not set up")?;
    let g = {
        let mut g = p.grants.write().unwrap_or_else(|e| e.into_inner());
        f(&p, &mut g)?;
        g.clone()
    };
    write_private(&grants_path(&dir, id), &serde_json::to_vec(&g).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(p.summary())
}

pub fn set_enabled(id: &str, on: bool) -> Result<Value, String> {
    if on {
        let me = by_id(id).ok_or_else(|| format!("no plugin {id}"))?;
        if let Some(o) = registry()
            .plugins
            .iter()
            .find(|p| p.manifest.id != id && p.enabled() && p.manifest.protocols.iter().any(|x| me.manifest.protocols.contains(x)))
        {
            return Err(format!("{} already handles the same protocol", o.manifest.name));
        }
    }
    update(id, |_, g| {
        g.enabled = on;
        Ok(())
    })
}

pub fn set_denied(id: &str, denied: Vec<String>) -> Result<Value, String> {
    update(id, |_, g| {
        g.denied = denied;
        Ok(())
    })
}

pub fn set_global(id: &str, values: &Value) -> Result<Value, String> {
    update(id, |p, g| {
        g.global = p.manifest.normalize(&p.manifest.global, values)?;
        Ok(())
    })
}

pub fn list() -> Value {
    Value::Array(registry().plugins.iter().map(|p| p.summary()).collect())
}

pub fn logs(id: &str) -> Value {
    let Some(p) = by_id(id) else { return Value::Array(Vec::new()) };
    let mut all: Vec<(u64, u32, String)> = p
        .diag
        .file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| Some((v["time"].as_u64()?, v["level"].as_u64()? as u32, v["message"].as_str()?.to_string())))
        .collect();
    let mem = p.diag.logs.lock().unwrap_or_else(|e| e.into_inner());
    let on_disk = p.diag.file.is_some();
    all.extend(mem.iter().filter(|(_, lvl, _)| !on_disk || *lvl > rv_plugin_abi::log::INFO).cloned());
    drop(mem);
    all.sort_by_key(|(t, _, _)| *t);
    let skip = all.len().saturating_sub(LOG_LINES);
    Value::Array(all[skip..].iter().map(|(t, lvl, m)| json!({ "time": t, "level": lvl, "message": m })).collect())
}

pub fn clear_logs(id: &str) {
    if let Some(p) = by_id(id) {
        p.diag.logs.lock().unwrap_or_else(|e| e.into_inner()).clear();
        if let Some(f) = &p.diag.file {
            let _ = std::fs::remove_file(f);
        }
    }
}

pub fn call(op: &str, arg: &Value, bytes: Option<&[u8]>) -> Result<Value, String> {
    let id = || arg["id"].as_str().ok_or_else(|| "no plugin id".to_string());
    let strings = |v: &Value| -> Vec<String> { v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect() };
    match op {
        "init" => {
            let dir = arg["dir"].as_str().ok_or("no plugin dir")?;
            Ok(json!({ "errors": init(Path::new(dir)) }))
        }
        "list" => Ok(list()),
        "inspect" => inspect(bytes.ok_or("no plugin file")?),
        "install" => install(bytes.ok_or("no plugin file")?, strings(&arg["denied"])),
        "remove" => remove(id()?).map(|()| Value::Null),
        "enable" => set_enabled(id()?, arg["on"].as_bool().unwrap_or(true)),
        "deny" => set_denied(id()?, strings(&arg["denied"])),
        "global" => set_global(id()?, &arg["values"]),
        "logs" => Ok(logs(id()?)),
        "clear_logs" => {
            clear_logs(id()?);
            Ok(Value::Null)
        }
        _ => Err(format!("unknown plugin operation {op}")),
    }
}

pub(crate) struct Ctx {
    pub plugin: Arc<Plugin>,
    pub perms: Permissions,
    pub settings: Vec<u8>,
    pub server: Option<(String, u16)>,
    pub user_hosts: Vec<String>,
    pub profile: Option<rv_config::Profile>,
    pub stream_settings: rv_config::StreamSettings,
    pub transport: Option<crate::transport::Stream>,
    pub dgram: Option<std::rc::Rc<outbound::PluginOutbound>>,
    pub protect: Option<rv_net::Protect>,
}

impl Ctx {
    fn offline(plugin: Arc<Plugin>) -> Self {
        Self {
            perms: plugin.permissions(),
            plugin,
            settings: b"{}".to_vec(),
            server: None,
            user_hosts: Vec::new(),
            profile: None,
            stream_settings: rv_config::StreamSettings::default(),
            transport: None,
            dgram: None,
            protect: None,
        }
    }

    fn online(&self) -> bool {
        self.profile.is_some()
    }
}

#[cfg(test)]
mod tests;
