use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(from = "TextRaw")]
pub struct Text {
    pub ru: String,
    pub en: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TextRaw {
    One(String),
    Both {
        #[serde(default)]
        ru: String,
        #[serde(default)]
        en: String,
    },
}

impl From<TextRaw> for Text {
    fn from(t: TextRaw) -> Self {
        match t {
            TextRaw::One(s) => Text { ru: s.clone(), en: s },
            TextRaw::Both { ru, en } => {
                Text { ru: if ru.is_empty() { en.clone() } else { ru }, en: if en.is_empty() { String::new() } else { en } }
            }
        }
    }
}

impl Text {
    pub fn get(&self) -> &str {
        if !crate::lang::english() && !self.ru.is_empty() || self.en.is_empty() { &self.ru } else { &self.en }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Outbound,
    Tunnel,
    Transport,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Carries {
    #[default]
    Stream,
    Datagram,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Text,
    Secret,
    Int,
    Bool,
    Choice,
    List,
    Host,
    Port,
    Multiline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub value: String,
    #[serde(default)]
    pub label: Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    #[serde(rename = "type")]
    pub ty: FieldType,
    #[serde(default)]
    pub label: Text,
    #[serde(default)]
    pub help: Text,
    #[serde(default)]
    pub placeholder: Text,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub advanced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Permissions {
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub net_any: bool,
    #[serde(default)]
    pub net_lan: bool,
    #[serde(default)]
    pub storage: bool,
    #[serde(default = "default_memory")]
    pub memory_mb: u32,
    #[serde(default)]
    pub keepalive: bool,
}

fn default_memory() -> u32 {
    16
}

impl Default for Permissions {
    fn default() -> Self {
        Self { hosts: Vec::new(), net_any: false, net_lan: false, storage: false, memory_mb: default_memory(), keepalive: false }
    }
}

pub const MEMORY_MAX_MB: u32 = 256;
pub const MEMORY_SAFE_MB: u32 = 64;

impl Permissions {
    pub fn dangerous(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.net_any {
            out.push("net_any");
        }
        if self.net_lan {
            out.push("net_lan");
        }
        if self.memory_mb > MEMORY_SAFE_MB {
            out.push("memory");
        }
        if self.keepalive {
            out.push("keepalive");
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u32,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub description: Text,
    pub kind: Kind,
    #[serde(default)]
    pub carries: Carries,
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default)]
    pub schemes: Vec<String>,
    #[serde(default)]
    pub udp: bool,
    #[serde(default)]
    pub settings: Vec<Field>,
    #[serde(default)]
    pub global: Vec<Field>,
    #[serde(default)]
    pub permissions: Permissions,
}

const BUILTIN: [&str; 15] = [
    "vless",
    "vmess",
    "trojan",
    "shadowsocks",
    "ss",
    "socks",
    "http",
    "hysteria",
    "hysteria2",
    "hy2",
    "freedom",
    "blackhole",
    "dns",
    "loopback",
    "direct",
];

fn ident(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_' || b == b'.')
}

impl Manifest {
    pub fn parse(json: &[u8]) -> Result<Self, String> {
        let m: Manifest = serde_json::from_slice(json).map_err(|e| format!("manifest: {e}"))?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), String> {
        let parts: Vec<&str> = self.id.split('.').collect();
        if self.id.len() > 128 || parts.len() < 2 || !parts.iter().all(|p| ident(p, 63) && !p.contains('.')) {
            return Err(format!("manifest: id `{}` (need reverse domain like dev.example.tuic)", self.id));
        }
        if self.api != rv_plugin_abi::API_VERSION {
            return Err(format!("manifest: api {} (this app has {})", self.api, rv_plugin_abi::API_VERSION));
        }
        if self.name.trim().is_empty() || self.name.len() > 64 || self.version.is_empty() || self.version.len() > 32 {
            return Err("manifest: name or version".into());
        }
        for p in &self.protocols {
            if !ident(p, 32) {
                return Err(format!("manifest: protocol `{p}`"));
            }
            if BUILTIN.contains(&p.as_str()) {
                return Err(format!("manifest: protocol `{p}` is built in"));
            }
        }
        for s in &self.schemes {
            if !ident(&s.replace('+', "."), 32)
                || BUILTIN.contains(&s.as_str())
                || ["vless", "vmess", "trojan", "ss", "hy2", "hysteria2", "socks", "https", "http"].contains(&s.as_str())
            {
                return Err(format!("manifest: scheme `{s}`"));
            }
        }
        if self.kind != Kind::Transport && self.protocols.is_empty() {
            return Err("manifest: an outbound or tunnel plugin names at least one protocol".into());
        }
        if self.settings.len() > 64 || self.global.len() > 64 {
            return Err("manifest: too many settings".into());
        }
        let mut keys = std::collections::HashSet::new();
        for f in self.settings.iter().chain(&self.global) {
            if !ident(&f.key, 48) && !f.key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err(format!("manifest: setting key `{}`", f.key));
            }
            if f.ty == FieldType::Choice && f.choices.is_empty() {
                return Err(format!("manifest: choice `{}` without choices", f.key));
            }
            let _ = keys.insert(f.key.clone());
        }
        for h in &self.permissions.hosts {
            let bare = h.strip_prefix("*.").unwrap_or(h);
            if bare.is_empty() || bare.contains('*') || bare.contains('/') || bare.contains(':') || !bare.contains('.') {
                return Err(format!("manifest: host `{h}`"));
            }
        }
        if self.permissions.memory_mb == 0 || self.permissions.memory_mb > MEMORY_MAX_MB {
            return Err(format!("manifest: memory_mb (1..={MEMORY_MAX_MB})"));
        }
        Ok(())
    }

    pub fn hosts_in(&self, settings: &serde_json::Map<String, Value>) -> Vec<String> {
        self.settings
            .iter()
            .filter(|f| f.ty == FieldType::Host)
            .filter_map(|f| settings.get(&f.key)?.as_str())
            .map(|h| h.trim().trim_end_matches('.').to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect()
    }

    pub fn normalize(&self, fields: &[Field], v: &Value) -> Result<serde_json::Map<String, Value>, String> {
        let mut out = match v {
            Value::Object(o) => o.clone(),
            Value::Null => serde_json::Map::new(),
            _ => return Err("settings: not an object".into()),
        };
        for f in fields {
            let got = out.get(&f.key).cloned().filter(|v| !v.is_null());
            let val = match got {
                None if !f.default.is_null() => f.default.clone(),
                None if f.required => return Err(format!("settings: `{}` is required", f.key)),
                None => continue,
                Some(v) => v,
            };
            let val = match f.ty {
                FieldType::Int | FieldType::Port => {
                    let n = match &val {
                        Value::Number(n) => n.as_i64(),
                        Value::String(s) => s.trim().parse::<i64>().ok(),
                        _ => None,
                    }
                    .ok_or_else(|| format!("settings: `{}` is not a number", f.key))?;
                    let (lo, hi) = if f.ty == FieldType::Port {
                        (f.min.unwrap_or(1), f.max.unwrap_or(65535))
                    } else {
                        (f.min.unwrap_or(i64::MIN), f.max.unwrap_or(i64::MAX))
                    };
                    if n < lo || n > hi {
                        return Err(format!("settings: `{}` out of {lo}..={hi}", f.key));
                    }
                    Value::from(n)
                }
                FieldType::Bool => match &val {
                    Value::Bool(b) => Value::Bool(*b),
                    Value::String(s) => Value::Bool(matches!(s.as_str(), "1" | "true" | "yes" | "on")),
                    Value::Number(n) => Value::Bool(n.as_i64().unwrap_or(0) != 0),
                    _ => return Err(format!("settings: `{}` is not a switch", f.key)),
                },
                FieldType::List => match val {
                    Value::Array(a) => Value::Array(
                        a.into_iter()
                            .map(|v| match v {
                                Value::String(s) => Value::String(s),
                                v => Value::String(v.to_string()),
                            })
                            .collect(),
                    ),
                    Value::String(s) => Value::Array(
                        s.split([',', '\n']).map(str::trim).filter(|s| !s.is_empty()).map(|s| Value::String(s.into())).collect(),
                    ),
                    _ => return Err(format!("settings: `{}` is not a list", f.key)),
                },
                FieldType::Choice => {
                    let s = match &val {
                        Value::String(s) => s.clone(),
                        v => v.to_string(),
                    };
                    if !f.choices.iter().any(|c| c.value == s) {
                        return Err(format!("settings: `{}` = `{s}` is not one of the choices", f.key));
                    }
                    Value::String(s)
                }
                _ => match val {
                    Value::String(s) => {
                        if s.len() > 64 * 1024 {
                            return Err(format!("settings: `{}` is too long", f.key));
                        }
                        Value::String(s)
                    }
                    Value::Number(n) => Value::String(n.to_string()),
                    Value::Bool(b) => Value::String(b.to_string()),
                    _ => return Err(format!("settings: `{}` is not text", f.key)),
                },
            };
            out.insert(f.key.clone(), val);
        }
        Ok(out)
    }
}

pub fn extract(wasm: &[u8]) -> Result<Vec<u8>, String> {
    if wasm.len() < 8 || &wasm[..4] != b"\0asm" || wasm[4..8] != [1, 0, 0, 0] {
        return Err("not a WebAssembly module".into());
    }
    let mut at = 8;
    let mut found = None;
    while at < wasm.len() {
        let id = wasm[at];
        at += 1;
        let (size, n) = leb(&wasm[at..]).ok_or("bad section size")?;
        at += n;
        let end = at.checked_add(size as usize).filter(|&e| e <= wasm.len()).ok_or("section past the end")?;
        if id == 0 {
            let body = &wasm[at..end];
            let (len, n) = leb(body).ok_or("bad section name")?;
            let name = body.get(n..n + len as usize).ok_or("bad section name")?;
            if name == rv_plugin_abi::MANIFEST_SECTION.as_bytes() {
                if found.is_some() {
                    return Err("two manifests".into());
                }
                found = Some(body[n + len as usize..].to_vec());
            }
        }
        at = end;
    }
    found.ok_or_else(|| format!("no `{}` section: not a ryNet plugin", rv_plugin_abi::MANIFEST_SECTION))
}

fn leb(b: &[u8]) -> Option<(u32, usize)> {
    let mut v = 0u32;
    for (i, &x) in b.iter().enumerate().take(5) {
        v |= u32::from(x & 0x7f) << (7 * i);
        if x & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        serde_json::json!({
            "id": "dev.example.tuic", "name": "TUIC", "version": "1.0.0", "api": 1, "kind": "outbound",
            "protocols": ["tuic"], "schemes": ["tuic"], "udp": true,
            "settings": [
                {"key": "address", "type": "host", "required": true, "label": {"ru": "Сервер", "en": "Server"}},
                {"key": "port", "type": "port", "default": 443},
                {"key": "cc", "type": "choice", "default": "bbr", "choices": [{"value": "bbr"}, {"value": "cubic"}]},
                {"key": "zero_rtt", "type": "bool"},
                {"key": "alpn", "type": "list"}
            ],
            "permissions": {"hosts": ["*.example.com"]}
        })
    }

    #[test]
    fn parses_and_normalizes_settings() {
        let m = Manifest::parse(sample().to_string().as_bytes()).unwrap();
        assert_eq!(m.settings[0].label.ru, "Сервер");
        assert!(m.permissions.dangerous().is_empty());
        let s = m
            .normalize(
                &m.settings,
                &serde_json::json!({"address": "a.example", "port": "8443", "zero_rtt": "true", "alpn": "h3, h2", "extra": 1}),
            )
            .unwrap();
        assert_eq!(s["port"], 8443);
        assert_eq!(s["cc"], "bbr");
        assert_eq!(s["zero_rtt"], true);
        assert_eq!(s["alpn"], serde_json::json!(["h3", "h2"]));
        assert_eq!(s["extra"], 1);
        assert!(m.normalize(&m.settings, &serde_json::json!({"port": 1})).is_err(), "address is required");
        assert!(m.normalize(&m.settings, &serde_json::json!({"address": "a", "port": 70000})).is_err());
        assert!(m.normalize(&m.settings, &serde_json::json!({"address": "a", "cc": "reno"})).is_err());
    }

    #[test]
    fn rejects_builtin_protocols_bad_ids_and_hosts() {
        for (k, v) in [
            ("protocols", serde_json::json!(["vless"])),
            ("id", serde_json::json!("tuic")),
            ("id", serde_json::json!("Dev.Example")),
            ("api", serde_json::json!(2)),
            ("schemes", serde_json::json!(["https"])),
        ] {
            let mut j = sample();
            j[k] = v;
            assert!(Manifest::parse(j.to_string().as_bytes()).is_err(), "{k}");
        }
        let mut j = sample();
        j["permissions"]["hosts"] = serde_json::json!(["*"]);
        assert!(Manifest::parse(j.to_string().as_bytes()).is_err());
        j["permissions"] = serde_json::json!({"net_any": true, "memory_mb": 128, "keepalive": true});
        let m = Manifest::parse(j.to_string().as_bytes()).unwrap();
        assert_eq!(m.permissions.dangerous(), ["net_any", "memory", "keepalive"]);
    }

    #[test]
    fn manifest_is_read_from_its_custom_section() {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        let json = sample().to_string();
        let name = rv_plugin_abi::MANIFEST_SECTION.as_bytes();
        let body_len = 1 + name.len() + json.len();
        wasm.push(0);
        let mut n = body_len;
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                wasm.push(b);
                break;
            }
            wasm.push(b | 0x80);
        }
        wasm.push(name.len() as u8);
        wasm.extend_from_slice(name);
        wasm.extend_from_slice(json.as_bytes());
        assert_eq!(extract(&wasm).unwrap(), json.as_bytes());
        assert!(extract(b"\0asm\x01\0\0\0").is_err());
        assert!(extract(b"PK\x03\x04").is_err());
    }
}
