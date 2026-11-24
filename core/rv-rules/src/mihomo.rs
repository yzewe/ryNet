use ring::digest::{SHA256, digest};
use serde_json::{Map, Value, json};
use std::{collections::HashSet, net::IpAddr, str::FromStr};

#[derive(Clone, Debug)]
pub struct Targets {
    pub direct: String,
    pub reject: String,
    pub proxy: String,
    pub known: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSetRef {
    pub url: String,
    pub behavior: Option<String>,
    pub key: String,
}

#[derive(Clone, Debug, Default)]
pub struct Translated {
    pub rules: Vec<Value>,
    pub bypass_apps: Vec<String>,
    pub rule_sets: Vec<RuleSetRef>,
    pub warnings: Vec<String>,
}

pub fn rule_set_key(url: &str) -> String {
    let d = digest(&SHA256, url.as_bytes());
    let hex: String = d.as_ref()[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("ruleset-{hex}")
}

type Cond = (&'static str, Value);

struct Ctx<'a> {
    targets: &'a Targets,
    unknown: HashSet<String>,
    out: Translated,
}

impl Ctx<'_> {
    fn tag(&mut self, target: &str) -> String {
        match target.to_ascii_uppercase().as_str() {
            "DIRECT" => self.targets.direct.clone(),
            "REJECT" | "REJECT-DROP" => self.targets.reject.clone(),
            "PROXY" | "GLOBAL" => self.targets.proxy.clone(),
            _ if self.targets.known.iter().any(|t| t == target) => target.to_string(),
            _ => {
                if self.unknown.insert(target.to_string()) {
                    self.out.warnings.push(format!("unknown target {target:?} → proxy"));
                }
                self.targets.proxy.clone()
            }
        }
    }
}

fn condition(kind: &str, value: &str, line: usize, cx: &mut Ctx<'_>) -> Result<Vec<Cond>, String> {
    let list = |s: String| json!([s]);
    Ok(match kind {
        "DOMAIN" => vec![("domain", list(format!("full:{}", value.to_ascii_lowercase())))],
        "DOMAIN-SUFFIX" => vec![("domain", list(format!("domain:{}", value.trim_start_matches('.').to_ascii_lowercase())))],
        "DOMAIN-KEYWORD" => vec![("domain", list(format!("keyword:{}", value.to_ascii_lowercase())))],
        "DOMAIN-REGEX" => vec![("domain", list(format!("regexp:{value}")))],
        "GEOSITE" => vec![("domain", list(format!("geosite:{}", value.to_ascii_lowercase())))],
        "IP-SUFFIX" => return Err("IP-SUFFIX is not supported".into()),
        "IP-CIDR" | "IP-CIDR6" => {
            validate_cidr(value)?;
            vec![("ip", list(value.to_string()))]
        }
        "GEOIP" => vec![("ip", list(format!("geoip:{}", value.to_ascii_lowercase())))],
        "DST-PORT" => vec![("port", json!(value.replace('/', ",")))],
        "SRC-PORT" => {
            cx.out.warnings.push(format!("line {line}: SRC-PORT → sourcePort, which our core cannot match"));
            vec![("sourcePort", json!(value.replace('/', ",")))]
        }
        "NETWORK" => {
            let v = value.to_ascii_lowercase();
            if v != "tcp" && v != "udp" {
                return Err(format!("NETWORK must be tcp or udp, got {value:?}"));
            }
            vec![("network", json!(v))]
        }
        "IP-ASN" => return Err("IP-ASN is not supported".into()),
        other => return Err(format!("unsupported rule type {other}")),
    })
}

fn validate_cidr(s: &str) -> Result<(), String> {
    let (ip, len) = s.split_once('/').unwrap_or((s, ""));
    let ip = IpAddr::from_str(ip).map_err(|_| format!("bad IP {s:?}"))?;
    if !len.is_empty() {
        let n: u8 = len.parse().map_err(|_| format!("bad prefix {s:?}"))?;
        if n > if ip.is_ipv4() { 32 } else { 128 } {
            return Err(format!("bad prefix {s:?}"));
        }
    }
    Ok(())
}

fn rule(conds: Vec<Cond>, tag: &str) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!("field"));
    for (k, v) in conds {
        m.insert(k.into(), v);
    }
    m.insert("outboundTag".into(), json!(tag));
    Value::Object(m)
}

fn sub_rules(s: &str) -> Result<Vec<String>, String> {
    let inner = s.trim().strip_prefix('(').and_then(|x| x.strip_suffix(')')).ok_or("expected ((…),(…))")?;
    let parts = split_top(inner);
    parts
        .into_iter()
        .map(|p| {
            let p = p.trim();
            p.strip_prefix('(').and_then(|x| x.strip_suffix(')')).map(str::to_string).ok_or_else(|| format!("bad sub-rule {p:?}"))
        })
        .collect()
}

fn translate_line(line_no: usize, line: &str, cx: &mut Ctx<'_>) -> Result<(), String> {
    let parts = split_top(line);
    let parts: Vec<&str> = parts.iter().map(|p| p.trim()).collect();
    let kind = parts.first().map(|k| k.to_ascii_uppercase()).unwrap_or_default();
    match kind.as_str() {
        "MATCH" | "FINAL" => {
            let target = parts.get(1).ok_or("MATCH needs a target")?;
            let tag = cx.tag(target);
            cx.out.rules.push(rule(vec![("network", json!("tcp,udp"))], &tag));
        }
        "PROCESS-NAME" | "PROCESS-NAME-REGEX" | "PROCESS-PATH" => {
            let (pkg, target) = (parts.get(1).ok_or("no package")?, parts.get(2).ok_or("no target")?);
            if kind != "PROCESS-NAME" {
                return Err(format!("{kind} is not supported, use PROCESS-NAME with a package name"));
            }
            if target.eq_ignore_ascii_case("DIRECT") {
                cx.out.bypass_apps.push(pkg.to_string());
            }
            let tag = cx.tag(target);
            cx.out.rules.push(rule(vec![("processName", json!([pkg]))], &tag));
        }
        "RULE-SET" => {
            let (url, target) = (parts.get(1).ok_or("no url")?, parts.get(2).ok_or("no target")?);
            let behavior = parts.get(3).map(|b| b.to_ascii_lowercase()).filter(|b| b != "no-resolve");
            let key = rule_set_key(url);
            let tag = cx.tag(target);
            if behavior.as_deref() != Some("ipcidr") {
                cx.out.rules.push(rule(vec![("domain", json!([format!("geosite:{key}")]))], &tag));
            }
            if behavior.as_deref() != Some("domain") {
                cx.out.rules.push(rule(vec![("ip", json!([format!("geoip:{key}")]))], &tag));
            }
            cx.out.rule_sets.push(RuleSetRef { url: url.to_string(), behavior, key });
        }
        "AND" | "OR" => {
            let (body, target) = (parts.get(1).ok_or("no sub-rules")?, parts.get(2).ok_or("no target")?);
            let subs = sub_rules(body)?;
            let mut all: Vec<Vec<Cond>> = Vec::new();
            for sub in &subs {
                let sp = split_top(sub);
                let (k, v) = (
                    sp.first().map(|k| k.trim().to_ascii_uppercase()).unwrap_or_default(),
                    sp.get(1).map(|v| v.trim().to_string()).unwrap_or_default(),
                );
                all.push(condition(&k, &v, line_no, cx)?);
            }
            let tag = cx.tag(target);
            if kind == "OR" {
                for conds in all {
                    cx.out.rules.push(rule(conds, &tag));
                }
            } else {
                let mut merged: Vec<Cond> = Vec::new();
                for (k, v) in all.into_iter().flatten() {
                    if merged.iter().any(|(mk, _)| *mk == k) {
                        return Err(format!("AND with two {k} conditions (same field) cannot be expressed in xray"));
                    }
                    merged.push((k, v));
                }
                cx.out.rules.push(rule(merged, &tag));
            }
        }
        "NOT" => return Err("NOT is not supported".into()),
        _ => {
            let (value, target) = (parts.get(1).ok_or("no value")?, parts.get(2).ok_or("no target")?);
            let conds = condition(&kind, value, line_no, cx)?;
            let tag = cx.tag(target);
            cx.out.rules.push(rule(conds, &tag));
        }
    }
    Ok(())
}

pub fn translate(text: &str, targets: &Targets) -> Translated {
    let mut cx = Ctx { targets, unknown: HashSet::new(), out: Translated::default() };
    for (i, line) in text.lines().enumerate() {
        let mut s = line.trim();
        if let Some(x) = s.strip_prefix("- ") {
            s = x.trim();
        }
        let s = s.split(" #").next().unwrap_or("").trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        if let Err(e) = translate_line(i + 1, s, &mut cx) {
            cx.out.warnings.push(format!("line {}: {e}", i + 1));
        }
    }
    cx.out
}

fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[start..i].trim().to_owned());
                start = i + 1
            }
            _ => {}
        }
    }
    out.push(s[start..].trim().to_owned());
    out
}

pub fn parse_rule_set(text: &str, behavior: Option<&str>) -> (Vec<String>, Vec<(IpAddr, u8)>) {
    let mut lines = Vec::new();
    for l in text.lines() {
        let mut s = l.trim();
        if s.starts_with("payload:") {
            continue;
        }
        if let Some(x) = s.strip_prefix("- ") {
            s = x.trim()
        }
        s = s.trim_matches(['\'', '\"']);
        if !s.is_empty() && !s.starts_with('#') {
            lines.push(s.to_owned())
        }
    }
    let beh = behavior.map(str::to_ascii_lowercase).unwrap_or_else(|| {
        let classical = lines
            .iter()
            .filter(|l| {
                let p = split_top(l);
                p.len() >= 2
                    && matches!(
                        p[0].to_ascii_uppercase().as_str(),
                        "DOMAIN" | "DOMAIN-SUFFIX" | "DOMAIN-KEYWORD" | "DOMAIN-REGEX" | "IP-CIDR" | "IP-CIDR6"
                    )
            })
            .count();
        if classical * 2 >= lines.len().max(1) {
            "classical".into()
        } else if lines.iter().all(|l| validate_cidr(l).is_ok()) {
            "ipcidr".into()
        } else {
            "domain".into()
        }
    });
    let (mut domains, mut ips) = (Vec::new(), Vec::new());
    for s in lines {
        match beh.as_str() {
            "ipcidr" => {
                if validate_cidr(&s).is_ok() {
                    let (ip, p) = s.split_once('/').unwrap_or((&s, ""));
                    let addr = IpAddr::from_str(ip).ok();
                    let max = if addr.is_some_and(|x| x.is_ipv4()) { 32 } else { 128 };
                    if let Some(a) = addr {
                        ips.push((a, p.parse().unwrap_or(max)))
                    }
                }
            }
            "classical" => {
                let p = split_top(&s);
                if p.len() < 2 {
                    continue;
                }
                let x = p[1].clone();
                match p[0].to_ascii_uppercase().as_str() {
                    "DOMAIN" => domains.push(format!("full:{x}")),
                    "DOMAIN-SUFFIX" => domains.push(format!("domain:{x}")),
                    "DOMAIN-KEYWORD" => domains.push(format!("keyword:{x}")),
                    "DOMAIN-REGEX" => domains.push(format!("regexp:{x}")),
                    "IP-CIDR" | "IP-CIDR6" if validate_cidr(&x).is_ok() => {
                        let (a, b) = x.split_once('/').unwrap_or((&x, ""));
                        if let Ok(addr) = IpAddr::from_str(a) {
                            let max = if addr.is_ipv4() { 32 } else { 128 };
                            ips.push((addr, b.parse().unwrap_or(max)))
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                if let Some(x) = s.strip_prefix("+.").or_else(|| s.strip_prefix("*.")).or_else(|| s.strip_prefix('.')) {
                    domains.push(format!("domain:{x}"))
                } else {
                    domains.push(format!("full:{s}"))
                }
            }
        }
    }
    (domains, ips)
}

#[cfg(kani)]
#[path = "proofs/mihomo.rs"]
mod proofs;
