use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};

#[derive(Default)]
struct Index {
    suffix: HashSet<String>,
    full: HashSet<String>,
    words: Vec<String>,
    nets: Vec<(IpAddr, u8)>,
}

static CACHE: Mutex<Vec<(String, Option<std::sync::Arc<Index>>)>> = Mutex::new(Vec::new());

fn file(dir: &Path, kind: &str, name: &str) -> Option<PathBuf> {
    let plain = dir.join(format!("{kind}-{name}.list"));
    if plain.is_file() {
        return Some(plain);
    }
    let (pre, post) = (format!("{kind}-"), format!("-{name}.list"));
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&pre) && n.ends_with(&post)))
}

fn value(line: &str) -> Option<&str> {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') {
        return None;
    }
    let mut parts = l.split(',');
    let first = parts.next().unwrap_or("");
    match parts.next() {
        Some(v) if first.chars().any(char::is_alphabetic) => Some(v.trim()),
        _ => Some(l),
    }
}

pub fn parse_net(s: &str) -> Option<(IpAddr, u8)> {
    let s = s.trim();
    let (a, m) = s.split_once('/').map_or((s, None), |(a, m)| (a, Some(m)));
    let ip: IpAddr = a.parse().ok()?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    let len = match m {
        Some(m) => m.parse::<u8>().ok().filter(|&l| l <= max)?,
        None => max,
    };
    Some((ip, len))
}

fn index(dir: &Path, kind: &str, name: &str) -> Option<std::sync::Arc<Index>> {
    let key = format!("{}|{kind}-{name}", dir.display());
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
        let hit = cache.remove(i);
        let out = hit.1.clone();
        cache.push(hit);
        return out;
    }
    let built = file(dir, kind, name).and_then(|f| std::fs::read_to_string(f).ok()).map(|text| {
        let mut idx = Index::default();
        for line in text.lines() {
            let l = line.trim();
            let Some(v) = value(l) else { continue };
            let v = v.to_ascii_lowercase();
            if kind == "geoip" {
                if let Some(n) = parse_net(&v) {
                    idx.nets.push(n);
                }
                continue;
            }
            let tag = if let Some((t, _)) = l.split_once(',') {
                format!("m:{}", t.to_ascii_uppercase())
            } else if let Some((t, _)) = l.split_once(':') {
                format!("x:{}", t.to_ascii_lowercase())
            } else {
                "x:domain".to_string()
            };
            match tag.as_str() {
                "m:DOMAIN" | "x:full" => {
                    idx.full.insert(v.trim_start_matches("full:").to_string());
                }
                "m:DOMAIN-KEYWORD" | "x:keyword" => idx.words.push(v.trim_start_matches("keyword:").to_string()),
                "m:DOMAIN-SUFFIX" | "x:domain" => {
                    idx.suffix.insert(v.trim_start_matches("domain:").to_string());
                }
                _ => {}
            }
        }
        std::sync::Arc::new(idx)
    });
    cache.push((key, built.clone()));
    if cache.len() > 6 {
        cache.remove(0);
    }
    built
}

fn geo_has(dir: &Path, kind: &str, name: &str, v: &str) -> Option<bool> {
    let idx = index(dir, kind, name)?;
    if kind == "geoip" {
        let net = parse_net(v)?;
        return Some(idx.nets.iter().any(|&n| crate::core::covers(n, net)));
    }
    let d = v.to_ascii_lowercase();
    if idx.full.contains(&d) || idx.words.iter().any(|w| d.contains(w.as_str())) {
        return Some(true);
    }
    let mut s = d.as_str();
    loop {
        if idx.suffix.contains(s) {
            return Some(true);
        }
        match s.split_once('.') {
            Some((_, rest)) => s = rest,
            None => return Some(false),
        }
    }
}

fn covers(dir: &Path, kind: &str, src: &Value, v: &str) -> bool {
    let k = src.get("k").and_then(Value::as_str).unwrap_or("");
    let sv = src.get("v").and_then(Value::as_str).unwrap_or("").trim();
    let low = sv.to_ascii_lowercase();
    match kind {
        "domain" => match k {
            "suffix" => v == low || v.ends_with(&format!(".{low}")),
            "full" => v == low,
            "keyword" => !low.is_empty() && v.contains(&low),
            "geosite" => geo_has(dir, "geosite", &low, v) == Some(true),
            _ => false,
        },
        "ip" => match (k, parse_net(v)) {
            ("cidr", Some(net)) => parse_net(sv).is_some_and(|o| crate::core::covers(o, net)),
            ("geoip", Some(_)) => geo_has(dir, "geoip", &low, v) == Some(true),
            _ => false,
        },
        "app" => k == "app" && sv.eq_ignore_ascii_case(v),
        _ => false,
    }
}

pub fn find(dir: &Path, req: &Value) -> Value {
    let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
    let sources = req.get("sources").and_then(Value::as_array).cloned().unwrap_or_default();
    let values = req.get("values").and_then(Value::as_array).cloned().unwrap_or_default();
    let out: Vec<i64> = values
        .iter()
        .map(|v| {
            let raw = v.as_str().unwrap_or("").trim();
            let v = if kind == "app" { raw.to_string() } else { raw.to_ascii_lowercase() };
            if v.is_empty() {
                return -1;
            }
            sources.iter().position(|s| covers(dir, kind, s, &v)).map_or(-1, |i| i as i64)
        })
        .collect();
    json!(out)
}

pub fn counts(dir: &Path, items: &Value) -> Value {
    let mut seen: HashMap<String, i64> = HashMap::new();
    let out: Vec<i64> = items
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|it| {
            let kind = it.get("kind").and_then(Value::as_str).unwrap_or("");
            let name = it.get("name").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
            if (kind != "geosite" && kind != "geoip") || name.is_empty() || name.contains(['/', '\\']) {
                return -1;
            }
            *seen.entry(format!("{kind}-{name}")).or_insert_with(|| {
                file(dir, kind, &name)
                    .and_then(|f| std::fs::read_to_string(f).ok())
                    .map_or(-1, |t| t.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count() as i64)
            })
        })
        .collect();
    json!(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rynet-cover-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("geosite-test.list"), "# c\nDOMAIN-SUFFIX,example.org\nDOMAIN,exact.net\nDOMAIN-KEYWORD,tube\n").unwrap();
        std::fs::write(d.join("geoip-dabcdef01-nets.list"), "IP-CIDR,10.1.0.0/16,no-resolve\n2001:db8::/32\n").unwrap();
        d
    }

    #[test]
    fn finds_first_source_in_order() {
        let d = dir();
        let req = json!({"kind": "domain", "values": ["a.example.org", "exact.net", "x.exact.net", "youtube.com", "vk.com", "other.io"],
            "sources": [{"k": "geosite", "v": "test"}, {"k": "suffix", "v": "vk.com"}, {"k": "keyword", "v": "you"}]});
        assert_eq!(find(&d, &req), json!([0, 0, -1, 0, 1, -1]));
        let req = json!({"kind": "ip", "values": ["10.1.2.0/24", "10.0.0.0/8", "192.168.1.1", "2001:db8:1::1"],
            "sources": [{"k": "geoip", "v": "nets"}, {"k": "cidr", "v": "192.168.0.0/16"}]});
        assert_eq!(find(&d, &req), json!([0, -1, 1, 0]));
        let req = json!({"kind": "app", "values": ["Firefox.exe"], "sources": [{"k": "suffix", "v": "firefox.exe"}, {"k": "app", "v": "firefox.exe"}]});
        assert_eq!(find(&d, &req), json!([1]));
        assert_eq!(
            counts(
                &d,
                &json!([{"kind": "geosite", "name": "test"}, {"kind": "geoip", "name": "nets"}, {"kind": "geosite", "name": "none"}])
            ),
            json!([3, 2, -1])
        );
    }
}
