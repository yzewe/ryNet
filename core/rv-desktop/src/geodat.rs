use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::store;

const KINDS: [(&str, &str); 2] = [("geosite", "geositeurl"), ("geoip", "geoipurl")];
const MAX_DAT: u64 = 256 << 20;

fn tag(url: &str) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, url.as_bytes());
    format!("d{}", d.as_ref()[..4].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn is_own(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 10 && b[0] == b'd' && b[9] == b'-' && b[1..9].iter().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn url<'a>(o: &'a Value, key: &str) -> Option<&'a str> {
    let (_, v) = o.as_object()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(key))?;
    v.as_str().map(str::trim).filter(|u| u.starts_with("https://") || u.starts_with("http://"))
}

fn refs(text: &str) -> Vec<(String, bool, String)> {
    let mut out = Vec::new();
    for (kind, _) in KINDS {
        let pat = format!("\"{kind}:");
        let mut rest = text;
        while let Some(i) = rest.to_ascii_lowercase().find(&pat) {
            rest = &rest[i + pat.len()..];
            let not = rest.starts_with('!');
            let name: String = rest[not as usize..].chars().take_while(|c| c.is_ascii_alphanumeric() || "._@!-".contains(*c)).collect();
            if !name.is_empty() && rest[not as usize + name.len()..].starts_with('"') {
                out.push((kind.to_string(), not, name.to_ascii_lowercase()));
            }
        }
    }
    out
}

fn file(dir: &Path, kind: &str, tag: &str, name: &str) -> std::path::PathBuf {
    dir.join(format!("{kind}-{tag}-{name}.list"))
}

pub fn localize(routing: &Value) -> Value {
    localize_in(routing, &store::geo_dir())
}

fn localize_in(routing: &Value, dir: &Path) -> Value {
    let tags: HashMap<&str, String> = KINDS.iter().filter_map(|(kind, key)| url(routing, key).map(|u| (*kind, tag(u)))).collect();
    if tags.is_empty() {
        return routing.clone();
    }
    fn walk(v: &Value, tags: &HashMap<&str, String>, dir: &Path) -> Value {
        match v {
            Value::String(s) => {
                for (kind, tag) in tags {
                    let Some(rest) = s
                        .get(kind.len() + 1..)
                        .filter(|_| s.get(..kind.len() + 1).is_some_and(|p| p.eq_ignore_ascii_case(&format!("{kind}:"))))
                    else {
                        continue;
                    };
                    let (not, name) = rest.strip_prefix('!').map_or(("", rest), |n| ("!", n));
                    let name = name.to_ascii_lowercase();
                    if !is_own(&name) && file(dir, kind, tag, &name).exists() {
                        return Value::String(format!("{kind}:{not}{tag}-{name}"));
                    }
                }
                v.clone()
            }
            Value::Array(a) => Value::Array(a.iter().map(|x| walk(x, tags, dir)).collect()),
            Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), walk(x, tags, dir))).collect()),
            _ => v.clone(),
        }
    }
    walk(routing, &tags, dir)
}

pub fn update(text: &str) {
    let dir = store::geo_dir();
    let all = refs(text);
    let mut seen = HashSet::new();
    for (kind, key) in KINDS {
        for u in urls(text, key) {
            if !seen.insert(u.clone()) {
                continue;
            }
            let u = u.as_str();
            let wanted: HashSet<String> = all
                .iter()
                .filter(|(k, _, n)| k == kind && !is_own(n) && !(kind == "geoip" && n == "private"))
                .map(|(_, _, n)| n.clone())
                .collect();
            if wanted.is_empty() {
                continue;
            }
            if let Err(e) = fetch(&dir, kind, u, &wanted) {
                tracing::warn!("geo {kind} from subscription: {e}");
            }
        }
    }
}

fn urls(text: &str, key: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let pat = format!("\"{key}\"");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find(&pat) {
        from += i + pat.len();
        let rest = text[from..].trim_start();
        let Some(rest) = rest.strip_prefix(':').map(str::trim_start).and_then(|r| r.strip_prefix('"')) else { continue };
        let Some(end) = rest.find('"') else { continue };
        let u = rest[..end].trim();
        if (u.starts_with("https://") || u.starts_with("http://")) && !out.iter().any(|x| x == u) {
            out.push(u.to_string());
        }
    }
    out
}

fn fetch(dir: &Path, kind: &str, url: &str, wanted: &HashSet<String>) -> Result<(), String> {
    let tag = tag(url);
    let etag = dir.join(format!("{kind}-{tag}.etag"));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into();
    let mut req = agent.get(url);
    if wanted.iter().all(|n| file(dir, kind, &tag, n).exists())
        && let Ok(e) = std::fs::read_to_string(&etag)
    {
        req = req.header("If-None-Match", e.trim());
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        200 => {}
        304 => return Ok(()),
        s => return Err(format!("http {s}")),
    }
    let new_etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
    let mut body = Vec::new();
    resp.body_mut().with_config().limit(MAX_DAT).reader().read_to_end(&mut body).map_err(|e| e.to_string())?;
    let lists = if kind == "geosite" { sites(&body, wanted)? } else { ips(&body, wanted)? };
    for (name, lines) in &lists {
        let path = file(dir, kind, &tag, name);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, lines.join("\n") + "\n").and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| e.to_string())?;
    }
    if let Some(e) = new_etag {
        let _ = std::fs::write(&etag, e);
    }
    tracing::info!("geo {kind} from subscription: {} of {} lists", lists.len(), wanted.len());
    Ok(())
}

struct Proto<'a> {
    b: &'a [u8],
    p: usize,
}

enum Field<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

impl<'a> Proto<'a> {
    fn new(b: &'a [u8]) -> Self {
        Proto { b, p: 0 }
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut r = 0u64;
        for shift in (0..64).step_by(7) {
            let x = *self.b.get(self.p).ok_or("bad .dat")?;
            self.p += 1;
            r |= u64::from(x & 0x7f) << shift;
            if x & 0x80 == 0 {
                return Ok(r);
            }
        }
        Err("bad varint".into())
    }

    fn next(&mut self) -> Result<Option<(u64, Field<'a>)>, String> {
        if self.p >= self.b.len() {
            return Ok(None);
        }
        let key = self.varint()?;
        let skip = |p: &mut Self, n: usize| -> Result<(), String> {
            p.p = p.p.checked_add(n).filter(|&e| e <= p.b.len()).ok_or("bad .dat")?;
            Ok(())
        };
        let f = match key & 7 {
            0 => Field::Int(self.varint()?),
            2 => {
                let n = usize::try_from(self.varint()?).map_err(|_| "bad .dat")?;
                let start = self.p;
                skip(self, n)?;
                Field::Bytes(&self.b[start..self.p])
            }
            1 => {
                skip(self, 8)?;
                return self.next();
            }
            5 => {
                skip(self, 4)?;
                return self.next();
            }
            _ => return Err("bad .dat".into()),
        };
        Ok(Some((key >> 3, f)))
    }

    fn fields(mut self) -> impl Iterator<Item = Result<(u64, Field<'a>), String>> {
        std::iter::from_fn(move || self.next().transpose())
    }
}

fn spec(r: &str) -> (&str, Option<&str>, bool) {
    match r.split_once('@') {
        Some((base, a)) if !a.is_empty() => (base, Some(a.trim_start_matches('!')), a.starts_with('!')),
        Some((base, _)) => (base, None, false),
        None => (r, None, false),
    }
}

fn sites(dat: &[u8], wanted: &HashSet<String>) -> Result<HashMap<String, Vec<String>>, String> {
    let mut by_base: HashMap<&str, Vec<&str>> = HashMap::new();
    for w in wanted {
        by_base.entry(spec(w).0).or_default().push(w);
    }
    let mut out = HashMap::new();
    for e in Proto::new(dat).fields() {
        let (1, Field::Bytes(e)) = e? else { continue };
        let mut code = None;
        let mut domains = Vec::new();
        for f in Proto::new(e).fields() {
            match f? {
                (1, Field::Bytes(v)) => {
                    let c = String::from_utf8_lossy(v).to_ascii_lowercase();
                    if !by_base.contains_key(c.as_str()) {
                        break;
                    }
                    code = Some(c);
                }
                (2, Field::Bytes(v)) => {
                    let (mut ty, mut value, mut attrs) = (0, String::new(), HashSet::new());
                    for g in Proto::new(v).fields() {
                        match g? {
                            (1, Field::Int(t)) => ty = t,
                            (2, Field::Bytes(s)) => value = String::from_utf8_lossy(s).into_owned(),
                            (3, Field::Bytes(a)) => {
                                if let Some(Ok((1, Field::Bytes(k)))) = Proto::new(a).fields().next() {
                                    attrs.insert(String::from_utf8_lossy(k).to_ascii_lowercase());
                                }
                            }
                            _ => {}
                        }
                    }
                    domains.push((ty, value, attrs));
                }
                _ => {}
            }
        }
        let Some(code) = code else { continue };
        for r in &by_base[code.as_str()] {
            let (_, attr, not) = spec(r);
            let lines = domains
                .iter()
                .filter(|(_, _, a)| attr.is_none_or(|x| a.contains(x) != not))
                .filter_map(|(t, v, _)| Some(format!("{}:{v}", ["keyword", "regexp", "domain", "full"].get(*t as usize)?)))
                .collect();
            out.insert(r.to_string(), lines);
        }
    }
    Ok(out)
}

fn ips(dat: &[u8], wanted: &HashSet<String>) -> Result<HashMap<String, Vec<String>>, String> {
    let mut out = HashMap::new();
    for e in Proto::new(dat).fields() {
        let (1, Field::Bytes(e)) = e? else { continue };
        let (mut code, mut cidrs, mut inverse) = (None, Vec::new(), false);
        for f in Proto::new(e).fields() {
            match f? {
                (1, Field::Bytes(v)) => {
                    let c = String::from_utf8_lossy(v).to_ascii_lowercase();
                    if !wanted.contains(&c) {
                        break;
                    }
                    code = Some(c);
                }
                (2, Field::Bytes(v)) => {
                    let (mut ip, mut prefix) = (None, None);
                    for g in Proto::new(v).fields() {
                        match g? {
                            (1, Field::Bytes(a)) => {
                                ip = <[u8; 4]>::try_from(a).map(IpAddr::from).or_else(|_| <[u8; 16]>::try_from(a).map(IpAddr::from)).ok()
                            }
                            (2, Field::Int(p)) => prefix = Some(p),
                            _ => {}
                        }
                    }
                    if let Some(ip) = ip {
                        cidrs.push(prefix.map_or(ip.to_string(), |p| format!("{ip}/{p}")));
                    }
                }
                (3, Field::Int(v)) => inverse = v != 0,
                _ => {}
            }
        }
        if let Some(c) = code.filter(|_| !inverse) {
            out.insert(c, cidrs);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(field: u64, v: &[u8]) -> Vec<u8> {
        let mut out = varint((field << 3) | 2);
        out.extend(varint(v.len() as u64));
        out.extend_from_slice(v);
        out
    }

    fn int(field: u64, v: u64) -> Vec<u8> {
        let mut out = varint(field << 3);
        out.extend(varint(v));
        out
    }

    fn varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return out;
            }
            out.push(b | 0x80);
        }
    }

    fn domain(ty: u64, value: &str, attr: Option<&str>) -> Vec<u8> {
        let mut d = int(1, ty);
        d.extend(bytes(2, value.as_bytes()));
        if let Some(a) = attr {
            d.extend(bytes(3, &bytes(1, a.as_bytes())));
        }
        bytes(2, &d)
    }

    fn site(code: &str, domains: &[Vec<u8>]) -> Vec<u8> {
        let mut s = bytes(1, code.as_bytes());
        for d in domains {
            s.extend(d);
        }
        bytes(1, &s)
    }

    #[test]
    fn sites_take_only_wanted_categories_with_attributes() {
        let mut dat =
            site("CHATGPT", &[domain(2, "openai.com", None), domain(3, "chatgpt.com", Some("ads")), domain(0, "oaistatic", None)]);
        dat.extend(site("other", &[domain(2, "example.com", None)]));
        let wanted: HashSet<String> = ["chatgpt", "chatgpt@ads", "chatgpt@!ads", "missing"].map(String::from).into();
        let got = sites(&dat, &wanted).unwrap();
        assert_eq!(got["chatgpt"], ["domain:openai.com", "full:chatgpt.com", "keyword:oaistatic"]);
        assert_eq!(got["chatgpt@ads"], ["full:chatgpt.com"]);
        assert_eq!(got["chatgpt@!ads"], ["domain:openai.com", "keyword:oaistatic"]);
        assert!(!got.contains_key("other") && !got.contains_key("missing"));
    }

    #[test]
    fn ips_skip_inverse_and_keep_prefixes() {
        let cidr = |ip: &[u8], p: u64| {
            let mut c = bytes(1, ip);
            c.extend(int(2, p));
            bytes(2, &c)
        };
        let mut ru = bytes(1, b"RU");
        ru.extend(cidr(&[10, 0, 0, 0], 8));
        ru.extend(cidr(&[0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 32));
        let mut not = bytes(1, b"notru");
        not.extend(cidr(&[1, 1, 1, 1], 32));
        not.extend(int(3, 1));
        let mut dat = bytes(1, &ru);
        dat.extend(bytes(1, &not));
        let got = ips(&dat, &["ru", "notru"].map(String::from).into()).unwrap();
        assert_eq!(got["ru"], ["10.0.0.0/8", "2001:db8::/32"]);
        assert!(!got.contains_key("notru"));
    }

    #[test]
    fn broken_dat_is_an_error_not_a_panic() {
        assert!(sites(&[0x0a, 0x7f, 1, 2], &["x".to_string()].into()).is_err());
        assert!(sites(&[0x0a, 0xff], &["x".to_string()].into()).is_err());
    }

    #[test]
    fn refs_and_own_names() {
        let r = refs(
            r#"{"Geositeurl":"https://x/geosite.dat","DirectSites":["geosite:chatgpt","geosite:!cn","GEOSITE:Ads@ads","geosite:x y"],"DirectIp":["geoip:ru"]}"#,
        );
        assert!(r.contains(&("geosite".into(), false, "chatgpt".into())));
        assert!(r.contains(&("geosite".into(), true, "cn".into())));
        assert!(r.contains(&("geosite".into(), false, "ads@ads".into())));
        assert!(r.contains(&("geoip".into(), false, "ru".into())));
        assert!(!r.iter().any(|(_, _, n)| n == "x"));
        assert!(is_own(&format!("{}-chatgpt", tag("https://x/geosite.dat"))));
        assert!(!is_own("category-ru"));
    }

    #[test]
    fn urls_are_found_in_multiline_routing_and_subscription_body() {
        let text = "vless://a@b:1#x\n{\n  \"Name\": \"r\",\n  \"GeoSiteUrl\" : \"https://a/geosite.dat\",\n  \"geoipurl\":\"ftp://no\"\n}\n{\"geositeurl\":\"https://a/geosite.dat\"}";
        assert_eq!(urls(text, "geositeurl"), ["https://a/geosite.dat"]);
        assert!(urls(text, "geoipurl").is_empty());
    }

    #[test]
    fn localize_points_only_at_downloaded_lists() {
        let dir = std::env::temp_dir().join(format!("rynet-geodat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let u = "https://example.com/geosite.dat";
        let t = tag(u);
        std::fs::write(file(&dir, "geosite", &t, "chatgpt"), "domain:openai.com\n").unwrap();
        let routing = serde_json::json!({ "geositeurl": u, "ProxySites": ["geosite:ChatGPT", "geosite:!chatgpt", "geosite:youtube", "domain:x"], "DirectIp": ["geoip:ru"] });
        let got = localize_in(&routing, &dir);
        assert_eq!(
            got["ProxySites"],
            serde_json::json!([format!("geosite:{t}-chatgpt"), format!("geosite:!{t}-chatgpt"), "geosite:youtube", "domain:x"])
        );
        assert_eq!(got["DirectIp"], serde_json::json!(["geoip:ru"]));
        assert_eq!(localize_in(&serde_json::json!({ "ProxySites": ["geosite:chatgpt"] }), &dir)["ProxySites"][0], "geosite:chatgpt");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
