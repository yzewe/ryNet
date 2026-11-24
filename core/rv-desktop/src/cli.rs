use rv_desktop::{core, fetch, logfile, store, tun};

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use rv_core::lang::{english, t};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::core::Cmd;
use crate::store::Sub;

const VERSION: &str = include_str!("../../../desktop-qt/version.txt");
const LOCAL_ID: &str = "local";
const LOCAL_URL: &str = "rynet:local";

macro_rules! tr {
    ($ru:literal, $en:literal $(, $a:expr)* $(,)?) => {
        if english() { format!($en $(, $a)*) } else { format!($ru $(, $a)*) }
    };
}

static EVENTS: Mutex<Option<mpsc::UnboundedSender<Value>>> = Mutex::new(None);

pub fn emit(v: Value) {
    if let Some(tx) = EVENTS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(v);
    }
}

fn events() -> mpsc::UnboundedReceiver<Value> {
    let (tx, rx) = mpsc::unbounded_channel();
    *EVENTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    rx
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    routing_mode: String,
    conn_mode: String,
    port: u16,
    kill_switch: bool,
    mtu: u32,
    dns_servers: String,
    server_dns: String,
    fake_dns: bool,
    ipv4_only: bool,
    fragment: bool,
    fragment_length: String,
    fragment_interval: String,
    chain_sub: String,
    chain_profile: String,
    ping_mode: String,
    ping_timeout: u32,
    ping_parallel: u32,
    ping_url: String,
    user_rules: String,
    processes: String,
    processes_mode: String,
    direct_ports: String,
    send_hwid: bool,
    hwid: String,
    sub_client: String,
    exit_ip_url: String,
    expiry_notifications: bool,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            routing_mode: "subscription".into(),
            conn_mode: "proxy".into(),
            port: 10808,
            kill_switch: false,
            mtu: 0,
            dns_servers: String::new(),
            server_dns: "auto".into(),
            fake_dns: false,
            ipv4_only: false,
            fragment: false,
            fragment_length: "100-200".into(),
            fragment_interval: "10-20".into(),
            chain_sub: String::new(),
            chain_profile: String::new(),
            ping_mode: "proxy".into(),
            ping_timeout: 5,
            ping_parallel: 16,
            ping_url: String::new(),
            user_rules: String::new(),
            processes: String::new(),
            processes_mode: "exclude".into(),
            direct_ports: String::new(),
            send_hwid: true,
            hwid: String::new(),
            sub_client: "rynet".into(),
            exit_ip_url: String::new(),
            expiry_notifications: false,
            extra: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct State {
    subs: Vec<Sub>,
    sub: Option<String>,
    profile: Option<String>,
    pings: BTreeMap<String, BTreeMap<String, Value>>,
    geo_pending: Vec<String>,
    settings: Settings,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn dir() -> PathBuf {
    let d = store::config_dir().join("cli");
    let _ = std::fs::create_dir_all(d.join("subs"));
    d
}

fn body_path(id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || id.len() > 128 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(t("Неверный идентификатор подписки", "Invalid subscription ID").into());
    }
    Ok(dir().join("subs").join(format!("{id}.txt")))
}

fn body(id: &str) -> Option<String> {
    std::fs::read_to_string(body_path(id).ok()?).ok()
}

fn save_body(id: &str, text: &str) -> Result<(), String> {
    let path = body_path(id)?;
    store::write_private(&path, text.as_bytes())
        .map_err(|e| format!("{}: {e}", t("Не удалось сохранить подписку", "Couldn't save the subscription")))
}

impl State {
    fn load() -> State {
        std::fs::read_to_string(dir().join("state.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            store::write(dir().join("state.json"), &s);
        }
    }
}

fn is_local(s: &Sub) -> bool {
    s.url == LOCAL_URL
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest)
}

fn sub_name(s: &Sub) -> String {
    if is_local(s) {
        return t("Свои серверы", "My servers").into();
    }
    s.title.clone().unwrap_or_else(|| host(&s.url).to_string())
}

struct Srv {
    n: usize,
    sub: usize,
    remarks: String,
    kind: String,
    ok: bool,
    stub: bool,
}

struct Catalog {
    bodies: Vec<String>,
    errors: Vec<Option<String>>,
    srvs: Vec<Srv>,
}

fn catalog(st: &State) -> Catalog {
    let mut c = Catalog { bodies: Vec::new(), errors: Vec::new(), srvs: Vec::new() };
    let mut n = 0;
    for (i, s) in st.subs.iter().enumerate() {
        let text = body(&s.id).unwrap_or_default();
        match fetch::describe(&text) {
            Ok(ps) => {
                c.errors.push(None);
                for p in ps {
                    let stub = p["placeholder"] == true;
                    if !stub {
                        n += 1;
                    }
                    c.srvs.push(Srv {
                        n: if stub { 0 } else { n },
                        sub: i,
                        remarks: p["remarks"].as_str().unwrap_or_default().to_string(),
                        kind: p["kind"].as_str().unwrap_or_default().to_string(),
                        ok: p["supported"] == true,
                        stub,
                    });
                }
            }
            Err(e) => c.errors.push(Some(e)),
        }
        c.bodies.push(text);
    }
    c
}

fn find_sub(st: &State, arg: &str, loose: bool) -> Option<usize> {
    let a = arg.trim().to_lowercase();
    let exact = st.subs.iter().position(|s| {
        sub_name(s).to_lowercase() == a || host(&s.url).to_lowercase() == a || s.url == arg.trim() || (is_local(s) && a == LOCAL_ID)
    });
    if exact.is_some() || !loose || a.is_empty() {
        return exact;
    }
    let hits: Vec<usize> = (0..st.subs.len()).filter(|&i| sub_name(&st.subs[i]).to_lowercase().contains(&a)).collect();
    (hits.len() == 1).then(|| hits[0])
}

fn find_server<'a>(st: &State, c: &'a Catalog, arg: &str) -> Result<&'a Srv, String> {
    let all = || c.srvs.iter().filter(|s| !s.stub);
    if let Ok(n) = arg.trim().parse::<usize>() {
        return all()
            .find(|s| s.n == n)
            .ok_or_else(|| tr!("Нет сервера с номером {}. Список: rynet-cli list", "No server number {}. See: rynet-cli list", n));
    }
    let a = arg.trim().to_lowercase();
    let mut hits: Vec<&Srv> = all().filter(|s| s.remarks.trim().to_lowercase() == a).collect();
    if hits.is_empty() {
        hits = all().filter(|s| s.remarks.to_lowercase().contains(&a)).collect();
    }
    match hits.len() {
        0 => Err(tr!("Нет сервера «{}». Список: rynet-cli list", "No server \"{}\". See: rynet-cli list", arg)),
        1 => Ok(hits[0]),
        _ => {
            let mut e = tr!("«{}» подходит к нескольким серверам, укажите номер:", "\"{}\" matches several servers, give a number:", arg);
            for s in hits.iter().take(12) {
                e += &format!("\n  {:>3}  {}  ({})", s.n, s.remarks, sub_name(&st.subs[s.sub]));
            }
            Err(e)
        }
    }
}

fn width(s: &str) -> usize {
    s.chars().count()
}

fn pad(s: &str, w: usize) -> String {
    let n = width(s);
    if n > w {
        let cut: String = s.chars().take(w.saturating_sub(1)).collect();
        return format!("{cut}…");
    }
    format!("{s}{}", " ".repeat(w - n))
}

fn size(b: u64) -> String {
    let units = if english() { ["B", "KB", "MB", "GB", "TB"] } else { ["Б", "КБ", "МБ", "ГБ", "ТБ"] };
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} {}", units[0]) } else { format!("{v:.1} {}", units[i]) }
}

fn date(secs: u64) -> String {
    let z = (secs / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d:02}.{m:02}.{y}")
}

fn span(secs: u64) -> String {
    let m = secs / 60;
    match m {
        0 => t("меньше минуты", "under a minute").into(),
        1..60 => tr!("{} мин", "{} min", m),
        _ if m < 1440 => tr!("{} ч {} мин", "{} h {} min", m / 60, m % 60),
        _ => tr!("{} дн {} ч", "{} d {} h", m / 1440, m % 1440 / 60),
    }
}

fn ago(secs: u64) -> String {
    let d = store::now().saturating_sub(secs);
    if d < 60 { t("только что", "just now").into() } else { tr!("{} назад", "{} ago", span(d)) }
}

fn servers_word(n: usize) -> String {
    if english() {
        return format!("{n} server{}", if n == 1 { "" } else { "s" });
    }
    let w = match (n % 10, n % 100) {
        (1, r) if r != 11 => "сервер",
        (2..=4, r) if !(12..=14).contains(&r) => "сервера",
        _ => "серверов",
    };
    format!("{n} {w}")
}

fn sub_header(s: &Sub) -> String {
    let mut parts = vec![sub_name(s)];
    if !is_local(s) {
        if s.title.is_some() {
            parts.push(host(&s.url).to_string());
        }
        let used = s.upload + s.download;
        if s.total > 0 {
            parts.push(tr!("{} из {}", "{} of {}", size(used), size(s.total)));
        } else if used > 0 {
            parts.push(size(used));
        }
        if s.expire > 0 {
            parts.push(tr!("до {}", "until {}", date(s.expire)));
        }
        if s.updated > 0 {
            parts.push(tr!("обновлена {}", "updated {}", ago(s.updated)));
        }
    }
    parts.join(" · ")
}

fn ping_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::Number(ms)) => tr!("{} мс", "{} ms", ms),
        Some(_) => "—".into(),
        None => String::new(),
    }
}

fn print_subs(st: &State, c: &Catalog, only: Option<usize>) {
    let w = c.srvs.iter().filter(|s| only.is_none_or(|o| o == s.sub)).map(|s| width(&s.remarks)).max().unwrap_or(0).min(40);
    let kw = c.srvs.iter().filter(|s| s.ok).map(|s| width(&s.kind)).max().unwrap_or(0).min(32);
    for (i, s) in st.subs.iter().enumerate() {
        if only.is_some_and(|o| o != i) {
            continue;
        }
        println!("{}", sub_header(s));
        if let Some(Some(e)) = c.errors.get(i) {
            println!("    {e}");
        }
        if let Some(a) = s.announce.as_deref() {
            println!("    {}", a.lines().next().unwrap_or_default());
        }
        let pings = st.pings.get(&s.id);
        for v in c.srvs.iter().filter(|v| v.sub == i) {
            if v.stub {
                println!("         {}", v.remarks);
                continue;
            }
            let mark =
                if st.sub.as_deref() == Some(s.id.as_str()) && st.profile.as_deref() == Some(v.remarks.as_str()) { "*" } else { " " };
            let tail = if v.ok {
                format!("{}  {}", pad(&v.kind, kw), ping_text(pings.and_then(|p| p.get(&v.remarks))))
            } else {
                t("не поддерживается", "not supported").to_string()
            };
            println!(" {mark} {:>3}  {}  {}", v.n, pad(&v.remarks, w), tail.trim_end());
        }
    }
}

const KEYS: &[(&str, &str, &str)] = &[
    (
        "routing",
        "маршруты: subscription — как в подписке, smart — российское напрямую, global — всё через сервер",
        "routing: subscription — as the subscription says, smart — Russian sites direct, global — everything through the server",
    ),
    (
        "mode",
        "подключение: proxy — локальный прокси SOCKS5/HTTP, tun — весь трафик системы",
        "connection: proxy — local SOCKS5/HTTP proxy, tun — all system traffic",
    ),
    ("port", "порт локального прокси", "local proxy port"),
    (
        "kill-switch",
        "защита от утечки трафика в режиме tun: без сервера интернет блокируется",
        "leak protection in tun mode: no server, no internet",
    ),
    ("mtu", "MTU для tun, 0 — автоматически", "tun MTU, 0 — automatic"),
    ("dns", "свои DNS-серверы через запятую; пусто — из подписки", "own DNS servers, comma separated; empty — from the subscription"),
    (
        "server-dns",
        "как узнавать адрес сервера: auto | cloudflare | google | system",
        "how to resolve the server address: auto | cloudflare | google | system",
    ),
    ("ipv4-only", "использовать только IPv4", "use IPv4 only"),
    ("fake-dns", "подменные адреса для имён в режиме tun", "fake addresses for names in tun mode"),
    (
        "fragment",
        "дробить TLS ClientHello, чтобы провайдер не видел имя сайта",
        "split TLS ClientHello so the provider can't read the site name",
    ),
    ("fragment-length", "размер кусков, байт (от-до)", "piece size, bytes (from-to)"),
    ("fragment-interval", "пауза между кусками, мс (от-до)", "pause between pieces, ms (from-to)"),
    (
        "chain",
        "цепочка: сначала через этот сервер (номер или имя), off — без цепочки",
        "chain: through this server first (number or name), off — no chain",
    ),
    (
        "ping-mode",
        "пинг: proxy — через сервер, proxy2 — повторный через сервер, tcp — до сервера, icmp — ICMP",
        "ping: proxy — through the server, proxy2 — repeated through the server, tcp — to the server, icmp — ICMP",
    ),
    ("ping-timeout", "таймаут пинга, с", "ping timeout, s"),
    ("ping-parallel", "сколько серверов пинговать одновременно", "how many servers to ping at once"),
    ("ping-url", "страница для пинга через сервер; пусто — по умолчанию", "page to ping through the server; empty — default"),
    (
        "rules",
        "свои правила (синтаксис mihomo) из файла или - (stdin); off — без правил",
        "own rules (mihomo syntax) from a file or - (stdin); off — none",
    ),
    (
        "processes",
        "Windows, tun: имена процессов через запятую (chrome.exe, steam.exe); off — пусто",
        "Windows, tun: process names, comma separated (chrome.exe, steam.exe); off — none",
    ),
    (
        "direct-ports",
        "tun: порты и диапазоны напрямую мимо VPN, TCP и UDP (443, 8000-8100); off — пусто",
        "tun: ports and ranges that go direct, bypassing the VPN, TCP and UDP (443, 8000-8100); off — none",
    ),
    (
        "processes-mode",
        "для списка процессов: exclude — они идут напрямую, include — только они через сервер, остальное напрямую",
        "for the process list: exclude — they go direct, include — only they go through the server, the rest direct",
    ),
    ("send-hwid", "отправлять провайдеру подписки идентификатор устройства", "send the device ID to the subscription provider"),
    ("hwid", "свой идентификатор устройства; default — сгенерированный", "own device ID; default — the generated one"),
    ("sub-client", "каким приложением представляться провайдеру: rynet | happ", "which app to introduce as to the provider: rynet | happ"),
];

fn on_off(b: bool) -> String {
    if b { "on".into() } else { "off".into() }
}

fn chain_label(st: &State) -> String {
    let s = &st.settings;
    if s.chain_sub.is_empty() || s.chain_profile.is_empty() {
        return "off".into();
    }
    match st.subs.iter().find(|x| x.id == s.chain_sub) {
        Some(x) => format!("{} ({})", s.chain_profile, sub_name(x)),
        None => s.chain_profile.clone(),
    }
}

fn value_of(st: &State, key: &str) -> String {
    let s = &st.settings;
    match key {
        "routing" => s.routing_mode.clone(),
        "mode" => s.conn_mode.clone(),
        "port" => s.port.to_string(),
        "kill-switch" => on_off(s.kill_switch),
        "mtu" => s.mtu.to_string(),
        "dns" => s.dns_servers.clone(),
        "server-dns" => s.server_dns.clone(),
        "fake-dns" => on_off(s.fake_dns),
        "ipv4-only" => on_off(s.ipv4_only),
        "fragment" => on_off(s.fragment),
        "fragment-length" => s.fragment_length.clone(),
        "fragment-interval" => s.fragment_interval.clone(),
        "chain" => chain_label(st),
        "ping-mode" => s.ping_mode.clone(),
        "ping-timeout" => s.ping_timeout.to_string(),
        "ping-parallel" => s.ping_parallel.to_string(),
        "ping-url" => s.ping_url.clone(),
        "rules" => match s.user_rules.lines().filter(|l| !l.trim().is_empty()).count() {
            0 => "off".into(),
            n => tr!("{} строк", "{} lines", n),
        },
        "processes" => {
            if s.processes.is_empty() {
                "off".into()
            } else {
                s.processes.clone()
            }
        }
        "processes-mode" => s.processes_mode.clone(),
        "direct-ports" => {
            if s.direct_ports.is_empty() {
                "off".into()
            } else {
                s.direct_ports.clone()
            }
        }
        "send-hwid" => on_off(s.send_hwid),
        "hwid" => {
            if s.hwid.is_empty() {
                store::hwid()
            } else {
                s.hwid.clone()
            }
        }
        "sub-client" => s.sub_client.clone(),
        _ => String::new(),
    }
}

fn settings_cmd(st: &State) {
    let w = KEYS.iter().map(|k| k.0.len()).max().unwrap_or(0);
    let vw = KEYS.iter().map(|k| width(&value_of(st, k.0))).max().unwrap_or(0).clamp(6, 24);
    for (k, ru, en) in KEYS {
        let v = value_of(st, k);
        println!("{}  {}  {}", pad(k, w), pad(if v.is_empty() { "—" } else { &v }, vw), if english() { en } else { ru });
    }
    println!(
        "\n{}",
        t(
            "Изменить: rynet-cli set <ключ> <значение>, вернуть по умолчанию: rynet-cli set <ключ> default",
            "Change: rynet-cli set <key> <value>, back to default: rynet-cli set <key> default"
        )
    );
}

fn flag(v: &str) -> Result<bool, String> {
    match v.to_lowercase().as_str() {
        "on" | "yes" | "true" | "1" | "да" | "вкл" => Ok(true),
        "off" | "no" | "false" | "0" | "нет" | "выкл" => Ok(false),
        _ => Err(tr!("Ожидается on или off, а не «{}»", "Expected on or off, not \"{}\"", v)),
    }
}

fn one_of(v: &str, all: &[&str]) -> Result<String, String> {
    let l = v.to_lowercase();
    all.iter().find(|a| **a == l).map(|a| a.to_string()).ok_or_else(|| tr!("Допустимо: {}", "Allowed: {}", all.join(" | ")))
}

fn number(v: &str, lo: u32, hi: u32) -> Result<u32, String> {
    v.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| (lo..=hi).contains(n))
        .ok_or_else(|| tr!("Нужно число от {} до {}", "Expected a number from {} to {}", lo, hi))
}

fn split_dns(s: &str) -> Vec<String> {
    s.split([',', ' ', '\n', '\t']).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect()
}

fn valid_dns(a: &str) -> bool {
    let l = a.to_lowercase();
    if l == "localhost" || l == "fakedns" || (!l.is_empty() && l.chars().all(|c| c.is_ascii_hexdigit() || ".:[]".contains(c))) {
        return true;
    }
    let Some((scheme, rest)) = l.split_once("://") else { return false };
    let scheme = scheme.strip_suffix("+local").unwrap_or(scheme);
    ["udp", "tcp", "tls", "https", "quic"].contains(&scheme)
        && !rest.is_empty()
        && !rest.starts_with('/')
        && !rest.chars().any(char::is_whitespace)
}

fn read_input(arg: &str) -> Result<String, String> {
    if arg == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
        return Ok(s);
    }
    let p = std::path::Path::new(arg);
    if p.is_file() {
        return std::fs::read_to_string(p).map_err(|e| format!("{arg}: {e}"));
    }
    Ok(arg.to_string())
}

fn set_cmd(st: &mut State, key: &str, value: &str) -> Result<(), String> {
    let d = Settings::default();
    let reset = value == "default";
    let s = &mut st.settings;
    match key {
        "routing" => s.routing_mode = if reset { d.routing_mode } else { one_of(value, &["subscription", "smart", "global", "direct"])? },
        "mode" => s.conn_mode = if reset { d.conn_mode } else { one_of(value, &["proxy", "tun"])? },
        "port" => s.port = if reset { d.port } else { number(value, 1, 65535)? as u16 },
        "kill-switch" => s.kill_switch = if reset { d.kill_switch } else { flag(value)? },
        "mtu" => {
            s.mtu = if reset {
                d.mtu
            } else {
                let n = number(value, 0, 9000)?;
                if n != 0 && n < 1280 {
                    return Err(t("MTU: 0 или от 1280 до 9000", "MTU: 0 or 1280 to 9000").into());
                }
                n
            }
        }
        "dns" => {
            let list = if reset { Vec::new() } else { split_dns(value) };
            if let Some(bad) = list.iter().find(|a| !valid_dns(a)) {
                return Err(tr!("Неверный адрес DNS: {}", "Invalid DNS address: {}", bad));
            }
            s.dns_servers = list.join(", ");
        }
        "server-dns" => s.server_dns = if reset { d.server_dns } else { one_of(value, &["auto", "cloudflare", "google", "system"])? },
        "fake-dns" => s.fake_dns = if reset { d.fake_dns } else { flag(value)? },
        "ipv4-only" => s.ipv4_only = if reset { d.ipv4_only } else { flag(value)? },
        "fragment" => s.fragment = if reset { d.fragment } else { flag(value)? },
        "fragment-length" | "fragment-interval" => {
            let v = if reset {
                if key == "fragment-length" { d.fragment_length } else { d.fragment_interval }
            } else {
                value.trim().to_string()
            };
            let (length, interval) = if key == "fragment-length" { (&v, &s.fragment_interval) } else { (&s.fragment_length, &v) };
            rv_core::tune::Tune::from_json(&json!({ "fragment": { "length": length, "interval": interval } }))
                .map_err(|_| t("Нужен диапазон вида 100-200", "Expected a range like 100-200").to_string())?;
            if key == "fragment-length" { s.fragment_length = v } else { s.fragment_interval = v }
        }
        "chain" => {
            if reset || value.eq_ignore_ascii_case("off") {
                s.chain_sub.clear();
                s.chain_profile.clear();
            } else {
                let c = catalog(st);
                let v = find_server(st, &c, value)?;
                if !v.ok {
                    return Err(t("Этот сервер не поддерживается", "This server isn't supported").into());
                }
                let (id, remarks) = (st.subs[v.sub].id.clone(), v.remarks.clone());
                st.settings.chain_sub = id;
                st.settings.chain_profile = remarks;
            }
        }
        "ping-mode" => s.ping_mode = if reset { d.ping_mode } else { one_of(value, &["proxy", "proxy2", "tcp", "icmp"])? },
        "ping-timeout" => s.ping_timeout = if reset { d.ping_timeout } else { number(value, 1, 60)? },
        "ping-parallel" => s.ping_parallel = if reset { d.ping_parallel } else { number(value, 1, 64)? },
        "ping-url" => {
            let v = if reset { String::new() } else { value.trim().to_string() };
            if !v.is_empty() && rv_core::ping::ProbeUrl::parse(&v).is_none() {
                return Err(tr!("Неверный адрес страницы: {}", "Invalid page address: {}", v));
            }
            s.ping_url = v;
        }
        "rules" => s.user_rules = if reset || value.eq_ignore_ascii_case("off") { String::new() } else { read_input(value)? },
        "processes" => {
            let list = if reset || value.eq_ignore_ascii_case("off") { Vec::new() } else { rv_core::procroute::parse_list(value)? };
            s.processes = list.join(", ");
        }
        "direct-ports" => {
            let list = if reset || value.eq_ignore_ascii_case("off") { Vec::new() } else { rv_core::portroute::parse_list(value)? };
            s.direct_ports = list.join(", ");
        }
        "processes-mode" => s.processes_mode = if reset { d.processes_mode } else { one_of(value, &["exclude", "include"])? },
        "send-hwid" => s.send_hwid = if reset { d.send_hwid } else { flag(value)? },
        "hwid" => s.hwid = if reset { String::new() } else { value.trim().to_string() },
        "sub-client" => s.sub_client = if reset { d.sub_client } else { one_of(value, &["rynet", "happ"])? },
        _ => return Err(tr!("Нет настройки «{}». Список: rynet-cli settings", "No setting \"{}\". See: rynet-cli settings", key)),
    }
    if matches!(key, "kill-switch" | "mode") && st.settings.kill_switch && st.settings.conn_mode != "tun" {
        eprintln!("{}", kill_switch_proxy());
    }
    Ok(())
}

fn kill_switch_proxy() -> &'static str {
    t(
        "Защита от утечки работает только в режиме tun (rynet-cli set mode tun или connect --tun): в режиме прокси трафик мимо прокси не блокируется",
        "Leak protection works only in tun mode (rynet-cli set mode tun or connect --tun): in proxy mode traffic outside the proxy is not blocked",
    )
}

fn options(st: &State, sub: &Sub) -> Result<Value, String> {
    let s = &st.settings;
    let mut o = json!({
        "routingMode": s.routing_mode,
        "pingUrl": s.ping_url,
        "pingTimeout": s.ping_timeout,
        "fakeDns": s.fake_dns,
        "ipv4Only": s.ipv4_only,
        "serverDns": s.server_dns,
        "userRules": s.user_rules,
        "processes": s.processes,
        "processesMode": s.processes_mode,
        "directPorts": s.direct_ports,
        "dns": split_dns(&s.dns_servers),
    });
    if let Some(r) = sub.routing.as_deref().and_then(|r| serde_json::from_str::<Value>(r).ok()).filter(Value::is_object) {
        o["subRouting"] = r;
    }
    if s.fragment {
        o["fragment"] = json!({ "length": s.fragment_length, "interval": s.fragment_interval });
    }
    if !s.chain_sub.is_empty() && !s.chain_profile.is_empty() {
        let config = body(&s.chain_sub).ok_or_else(|| t("Нет подписки для цепочки", "No subscription for the chain").to_string())?;
        o["chain"] = json!({ "config": config, "profile": s.chain_profile });
    }
    Ok(o)
}

fn client(st: &State, chosen: &str) -> fetch::Client {
    let s = &st.settings;
    #[cfg(unix)]
    let os = format!("Linux {}", std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim());
    #[cfg(windows)]
    let os = "Windows".to_string();
    fetch::Client {
        agent: Some(if chosen == "happ" { "Happ/4.3.0".into() } else { format!("ryNet/{} ({os})", VERSION.trim()) }),
        headers: Vec::new(),
        hwid: s.send_hwid.then(|| Some(s.hwid.clone()).filter(|h| !h.trim().is_empty()).unwrap_or_else(store::hwid)),
        happ_retry: chosen == "rynet",
    }
}

fn subscription_headers(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for line in text.lines().filter(|s| !s.trim().is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err("Headers: Name: value".into());
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || value.chars().any(|c| c.is_control() && c != '\t')
        {
            return Err("Invalid subscription header".into());
        }
        out.push((name.into(), value.into()));
    }
    Ok(out)
}

fn fetch_sub(st: &mut State, url: &str, id: Option<&str>) -> Result<usize, String> {
    let previous = id.and_then(|id| st.subs.iter().find(|s| s.id == id));
    let chosen = previous.and_then(|s| s.client_override.as_deref()).unwrap_or(st.settings.sub_client.as_str());
    let mut client = client(st, chosen);
    let headers = previous
        .and_then(|s| s.headers_override.as_deref())
        .or_else(|| st.settings.extra.get("subHeaders").and_then(Value::as_str))
        .unwrap_or("");
    client.headers = subscription_headers(headers)?;
    let (mut sub, text) = fetch::download(url, id, &client)?;
    if let Some(old) = previous {
        sub.client_override = old.client_override.clone();
        sub.headers_override = old.headers_override.clone();
        sub.update_interval_minutes = old.update_interval_minutes;
        sub.extra = old.extra.clone();
    }
    save_body(&sub.id, &text)?;
    if !st.geo_pending.contains(&sub.id) {
        st.geo_pending.push(sub.id.clone());
    }
    Ok(match st.subs.iter().position(|s| s.id == sub.id) {
        Some(i) => {
            st.subs[i] = sub;
            i
        }
        None => {
            st.subs.push(sub);
            st.subs.len() - 1
        }
    })
}

fn add_cmd(st: &mut State, arg: &str) -> Result<(), String> {
    let text = read_input(arg)?;
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    if (lower.starts_with("https://") || lower.starts_with("http://")) && !trimmed.contains(char::is_whitespace) {
        let known = st.subs.iter().find(|s| s.url == trimmed).map(|s| s.id.clone());
        let i = fetch_sub(st, trimmed, known.as_deref())?;
        st.save();
        let c = catalog(st);
        let n = c.srvs.iter().filter(|s| s.sub == i && !s.stub).count();
        let name = sub_name(&st.subs[i]);
        println!(
            "{}",
            if known.is_some() {
                tr!("Подписка «{}» уже была, обновлена: {}", "Subscription \"{}\" was already here, updated: {}", name, servers_word(n))
            } else {
                tr!("Добавлена подписка «{}»: {}", "Added subscription \"{}\": {}", name, servers_word(n))
            }
        );
        print_subs(st, &c, Some(i));
        hint_use(st);
        return Ok(());
    }
    let found = rv_config::subscription_values(&text)
        .map_err(|e| format!("{}: {e}", t("Не похоже на ссылку на сервер или конфиг", "This doesn't look like a server link or config")))?;
    let key = |v: &Value| {
        let mut v = v.clone();
        if let Some(o) = v.as_object_mut() {
            o.remove("remarks");
        }
        v
    };
    let mut all = body(LOCAL_ID).and_then(|b| rv_config::subscription_values(&b).ok()).unwrap_or_default();
    for v in found {
        match all.iter().position(|x| key(x) == key(&v)) {
            Some(i) => all[i] = v,
            None => all.push(v),
        }
    }
    let joined = Value::Array(all).to_string();
    fetch::describe(&joined)?;
    save_body(LOCAL_ID, &joined)?;
    if !st.subs.iter().any(is_local) {
        st.subs.push(Sub { id: LOCAL_ID.into(), url: LOCAL_URL.into(), updated: store::now(), ..Default::default() });
    }
    st.save();
    let c = catalog(st);
    let i = st.subs.iter().position(is_local).unwrap_or_default();
    print_subs(st, &c, Some(i));
    hint_use(st);
    Ok(())
}

fn hint_use(st: &State) {
    if st.profile.is_none() {
        println!("\n{}", t("Выберите сервер: rynet-cli use <номер>", "Pick a server: rynet-cli use <number>"));
    }
}

fn update_cmd(st: &mut State, arg: Option<&str>) -> Result<i32, String> {
    let which: Vec<usize> = match arg {
        Some(a) => vec![find_sub(st, a, true).ok_or_else(|| tr!("Нет подписки «{}»", "No subscription \"{}\"", a))?],
        None => (0..st.subs.len()).filter(|&i| !is_local(&st.subs[i])).collect(),
    };
    if which.is_empty() {
        println!("{}", t("Нет подписок. Добавьте: rynet-cli add <ссылка>", "No subscriptions. Add one: rynet-cli add <link>"));
        return Ok(0);
    }
    let mut code = 0;
    for i in which {
        let (url, id, name) = (st.subs[i].url.clone(), st.subs[i].id.clone(), sub_name(&st.subs[i]));
        if is_local(&st.subs[i]) {
            continue;
        }
        match fetch_sub(st, &url, Some(&id)) {
            Ok(i) => {
                let n = fetch::describe(&body(&st.subs[i].id).unwrap_or_default())
                    .map(|p| p.iter().filter(|x| x["placeholder"] != true).count())
                    .unwrap_or(0);
                println!("{}: {}", sub_name(&st.subs[i]), servers_word(n));
            }
            Err(e) => {
                eprintln!("{name}: {e}");
                code = 1;
            }
        }
    }
    st.save();
    Ok(code)
}

fn remove_cmd(st: &mut State, arg: &str) -> Result<(), String> {
    if let Some(i) = find_sub(st, arg, false) {
        let s = st.subs.remove(i);
        let _ = std::fs::remove_file(body_path(&s.id)?);
        st.pings.remove(&s.id);
        st.geo_pending.retain(|g| *g != s.id);
        if st.sub.as_deref() == Some(s.id.as_str()) {
            st.sub = None;
            st.profile = None;
        }
        st.save();
        println!("{}", tr!("Подписка «{}» удалена", "Subscription \"{}\" removed", sub_name(&s)));
        return Ok(());
    }
    let c = catalog(st);
    let v = find_server(st, &c, arg)?;
    if !is_local(&st.subs[v.sub]) {
        return Err(t(
            "Сервер из подписки не удалить отдельно — удалите подписку целиком",
            "A server from a subscription can't be removed on its own — remove the whole subscription",
        )
        .into());
    }
    let at = c.srvs.iter().filter(|x| x.sub == v.sub).position(|x| std::ptr::eq(x, v)).unwrap_or_default();
    let mut all = rv_config::subscription_values(&c.bodies[v.sub]).map_err(|e| e.to_string())?;
    if at < all.len() {
        all.remove(at);
    }
    let remarks = v.remarks.clone();
    if all.is_empty() {
        st.subs.retain(|s| !is_local(s));
        let _ = std::fs::remove_file(body_path(LOCAL_ID)?);
    } else {
        save_body(LOCAL_ID, &Value::Array(all).to_string())?;
    }
    if let Some(p) = st.pings.get_mut(LOCAL_ID) {
        p.remove(&remarks);
    }
    if st.sub.as_deref() == Some(LOCAL_ID) && st.profile.as_deref() == Some(remarks.as_str()) {
        st.sub = None;
        st.profile = None;
    }
    st.save();
    println!("{}", tr!("Сервер «{}» удалён", "Server \"{}\" removed", remarks));
    Ok(())
}

fn use_cmd(st: &mut State, arg: &str) -> Result<(), String> {
    let c = catalog(st);
    let v = find_server(st, &c, arg)?;
    if !v.ok {
        return Err(t("Этот сервер не поддерживается", "This server isn't supported").into());
    }
    st.sub = Some(st.subs[v.sub].id.clone());
    st.profile = Some(v.remarks.clone());
    println!("{}", tr!("Выбран: {} {} ({})", "Selected: {} {} ({})", v.n, v.remarks, sub_name(&st.subs[v.sub])));
    st.save();
    Ok(())
}

fn ping_cmd(st: &mut State, args: &[String]) -> Result<i32, String> {
    let mut mode = st.settings.ping_mode.clone();
    let mut picks: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                i += 1;
                mode = one_of(args.get(i).map_or("", String::as_str), &["proxy", "proxy2", "tcp", "icmp"])?;
            }
            a => picks.push(a),
        }
        i += 1;
    }
    #[cfg(windows)]
    tun::clear_leftover();
    #[cfg(target_os = "linux")]
    if ask("status", Duration::from_secs(2)).is_none() {
        clear_dead_rules()?;
    }
    let c = catalog(st);
    let mut chosen: Vec<&Srv> = Vec::new();
    for a in &picks {
        let server = find_server(st, &c, a);
        match find_sub(st, a, false).or_else(|| server.is_err().then(|| find_sub(st, a, true)).flatten()) {
            Some(s) => chosen.extend(c.srvs.iter().filter(|v| v.sub == s)),
            None => chosen.push(server?),
        }
    }
    if picks.is_empty() {
        chosen = c.srvs.iter().collect();
    }
    chosen.retain(|v| v.ok && !v.stub);
    if chosen.is_empty() {
        println!(
            "{}",
            t("Нечего пинговать. Добавьте подписку: rynet-cli add <ссылка>", "Nothing to ping. Add a subscription: rynet-cli add <link>")
        );
        return Ok(0);
    }
    let code = match mode.as_str() {
        "tcp" => 0,
        "icmp" => 2,
        "proxy2" => 3,
        _ => 1,
    };
    let rt = runtime()?;
    let mut rx = events();
    let tx = core::spawn();
    let w = chosen.iter().map(|v| width(&v.remarks)).max().unwrap_or(0).min(40);
    let mut results: Vec<(usize, String, Value)> = Vec::new();
    for sub in 0..st.subs.len() {
        let names: Vec<String> = chosen.iter().filter(|v| v.sub == sub).map(|v| v.remarks.clone()).collect();
        if names.is_empty() {
            continue;
        }
        let tune = options(st, &st.subs[sub])?;
        let parallel = (st.settings.ping_parallel as usize).min(names.len()).max(1);
        let _ = tx.send(Cmd::Ping {
            generation: 0,
            text: c.bodies[sub].clone(),
            names,
            mode: code,
            parallel,
            timeout_ms: u64::from(st.settings.ping_timeout) * 1000,
            url: st.settings.ping_url.clone(),
            tune,
        });
        rt.block_on(async {
            while let Some(ev) = rx.recv().await {
                match ev["ev"].as_str() {
                    Some("ping") => {
                        let name = ev["name"].as_str().unwrap_or_default().to_string();
                        let n = chosen.iter().find(|v| v.sub == sub && v.remarks == name).map_or(0, |v| v.n);
                        let r = if let Some(ms) = ev["ms"].as_u64() {
                            println!("{:>8}  {n:>3}  {name}", tr!("{} мс", "{} ms", ms));
                            json!(ms)
                        } else {
                            let e = ev["error"].as_str().unwrap_or_default().to_string();
                            println!("{:>8}  {n:>3}  {}  ({e})", "—", pad(&name, w).trim_end());
                            json!(e)
                        };
                        results.push((sub, name, r));
                    }
                    Some("pingDone") => break,
                    _ => {}
                }
            }
        });
    }
    drop(tx);
    for (sub, name, r) in results {
        st.pings.entry(st.subs[sub].id.clone()).or_default().insert(name, r);
    }
    st.save();
    Ok(0)
}

#[derive(Serialize, Deserialize)]
struct Live {
    pid: u32,
    port: u16,
    token: String,
    server: String,
    sub: String,
    tun: bool,
    listen: u16,
}

fn live_path() -> PathBuf {
    dir().join("connect.json")
}

fn write_live(l: &Live) -> Result<(), String> {
    let path = live_path();
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(&path).map_err(|e| e.to_string())?;
    f.write_all(serde_json::to_string(l).unwrap_or_default().as_bytes()).map_err(|e| e.to_string())
}

fn ask(cmd: &str, wait: Duration) -> Option<(Live, String)> {
    let l: Live = serde_json::from_str(&std::fs::read_to_string(live_path()).ok()?).ok()?;
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], l.port).into(), Duration::from_secs(2)) else {
        let _ = std::fs::remove_file(live_path());
        return None;
    };
    let _ = s.set_read_timeout(Some(wait));
    writeln!(s, "{} {cmd}", l.token).ok()?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    Some((l, out))
}

struct Shared {
    connected: AtomicBool,
    since: AtomicU64,
}

fn control(listener: TcpListener, token: String, tx: mpsc::UnboundedSender<Cmd>, shared: Arc<Shared>) {
    for s in listener.incoming() {
        let Ok(mut s) = s else { continue };
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        let _ = BufReader::new((&s).take(256)).read_line(&mut line);
        let Some((tok, cmd)) = line.trim().split_once(' ') else { continue };
        if tok != token {
            continue;
        }
        match cmd {
            "status" => {
                use rv_stats::{Slot, get};
                let _ = writeln!(
                    s,
                    "{}",
                    json!({ "connected": shared.connected.load(Ordering::Relaxed), "since": shared.since.load(Ordering::Relaxed), "up": get(Slot::Up), "down": get(Slot::Down) })
                );
            }
            "stop" => {
                let _ = tx.send(Cmd::Stop);
                std::mem::forget(s);
            }
            _ => {}
        }
    }
}

fn tun_hint() -> String {
    #[cfg(unix)]
    {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "rynet-cli".into());
        tr!(
            "Для режима tun нужно право CAP_NET_ADMIN: sudo setcap cap_net_admin,cap_net_raw+ep {}",
            "tun mode needs CAP_NET_ADMIN: sudo setcap cap_net_admin,cap_net_raw+ep {}",
            exe
        )
    }
    #[cfg(windows)]
    {
        t("Для режима tun запустите терминал от имени администратора", "For tun mode, run the terminal as administrator").into()
    }
}

fn connect_cmd(st: &mut State, args: &[String]) -> Result<i32, String> {
    let mut i = 0;
    let mut server = None;
    while i < args.len() {
        let a = args[i].as_str();
        let (name, inline) = a.split_once('=').map_or((a, None), |(n, v)| (n, Some(v.to_string())));
        let mut value = || -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            i += 1;
            args.get(i).cloned().ok_or_else(|| tr!("Нет значения для {}", "No value for {}", name))
        };
        match name {
            "--tun" => st.settings.conn_mode = "tun".into(),
            "--proxy" => st.settings.conn_mode = "proxy".into(),
            "--port" => {
                let v = value()?;
                set_cmd(st, "port", &v)?;
            }
            "--routing" => {
                let v = value()?;
                set_cmd(st, "routing", &v)?;
            }
            _ if name.starts_with("--") => return Err(tr!("Неизвестный параметр {}", "Unknown option {}", name)),
            _ => server = Some(a.to_string()),
        }
        i += 1;
    }
    if ask("status", Duration::from_secs(2)).is_some() {
        return Err(t(
            "Уже подключено. Состояние: rynet-cli status, отключиться: rynet-cli disconnect",
            "Already connected. See rynet-cli status, or rynet-cli disconnect",
        )
        .into());
    }
    #[cfg(windows)]
    tun::clear_leftover();
    #[cfg(target_os = "linux")]
    clear_dead_rules()?;
    let mut saved = State::load();
    if let Some(a) = server {
        use_cmd(&mut saved, &a)?;
        st.sub = saved.sub.clone();
        st.profile = saved.profile.clone();
    }
    let (Some(id), Some(profile)) = (st.sub.clone(), st.profile.clone()) else {
        return Err(t(
            "Выберите сервер: rynet-cli use <номер> или rynet-cli connect <номер>",
            "Pick a server: rynet-cli use <number> or rynet-cli connect <number>",
        )
        .into());
    };
    let sub = st
        .subs
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .ok_or_else(|| t("Выбранной подписки больше нет", "The selected subscription is gone").to_string())?;
    let text = body(&id).ok_or_else(|| {
        t("Нет данных подписки, обновите её: rynet-cli update", "No subscription data, update it: rynet-cli update").to_string()
    })?;
    let use_tun = st.settings.conn_mode == "tun";
    if use_tun && !tun::capable() {
        return Err(tun_hint());
    }
    let opts = options(st, &sub)?;

    let path = dir().join("core.log");
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
        tracing_subscriber::fmt().with_writer(Mutex::new(logfile::LogFile::new(f, path))).with_ansi(false).with_env_filter(filter).init();
    }
    if saved.geo_pending.contains(&id) {
        println!("{}", t("Загружаю списки для маршрутов…", "Downloading routing lists…"));
        fetch::update_geo(&format!("{text}\n{}", sub.routing.clone().unwrap_or_default()));
        saved.geo_pending.retain(|g| *g != id);
        saved.save();
    }
    fetch::ensure_geo();

    let rt = runtime()?;
    let mut rx = events();
    let tx = core::spawn();
    let shared = Arc::new(Shared { connected: AtomicBool::new(false), since: AtomicU64::new(0) });
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let token = store::new_id();
    let port = st.settings.port;
    write_live(&Live {
        pid: std::process::id(),
        port: listener.local_addr().map_err(|e| e.to_string())?.port(),
        token: token.clone(),
        server: profile.clone(),
        sub: sub_name(&sub),
        tun: use_tun,
        listen: port,
    })?;
    {
        let (tx, shared) = (tx.clone(), shared.clone());
        std::thread::spawn(move || control(listener, token, tx, shared));
    }
    if !use_tun && st.settings.kill_switch {
        eprintln!("{}", kill_switch_proxy());
    }
    println!("{}", tr!("Подключаюсь: {} ({})…", "Connecting: {} ({})…", profile, sub_name(&sub)));
    let _ = tx.send(Cmd::Stats(false));
    let _ = tx.send(Cmd::Start {
        text,
        profile: profile.clone(),
        options: opts,
        tun: use_tun,
        port,
        kill_switch: use_tun && st.settings.kill_switch,
        mtu: if (1280..=9000).contains(&st.settings.mtu) { st.settings.mtu as usize } else { 9000 },
    });
    let code = rt.block_on(async {
        #[cfg(unix)]
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        let mut stopping = false;
        loop {
            #[cfg(unix)]
            let terminate = async {
                match term.as_mut() {
                    Some(s) => s.recv().await,
                    None => std::future::pending().await,
                }
            };
            #[cfg(windows)]
            let terminate = std::future::pending::<Option<()>>();
            tokio::select! {
                ev = rx.recv() => {
                    let Some(ev) = ev else { return 1 };
                    match ev["ev"].as_str() {
                        Some("started") => {
                            shared.since.store(store::now(), Ordering::Relaxed);
                            shared.connected.store(true, Ordering::Relaxed);
                            let how = if use_tun { t("весь трафик (tun)", "all traffic (tun)").to_string() } else { tr!("прокси SOCKS5/HTTP 127.0.0.1:{}", "SOCKS5/HTTP proxy 127.0.0.1:{}", port) };
                            println!("{}", tr!("Подключено: {} · {}. Ctrl+C — отключиться.", "Connected: {} · {}. Ctrl+C to disconnect.", profile, how));
                        }
                        Some("error") => {
                            eprintln!("{}", ev["message"].as_str().unwrap_or_default());
                            if ev["kept"] != true {
                                if ev["blocked"] == true {
                                    eprintln!("{}", t("Интернет заблокирован защитой от утечки. Снять: rynet-cli disconnect", "Internet is blocked by leak protection. Lift it: rynet-cli disconnect"));
                                }
                                return 1;
                            }
                        }
                        Some("failing") => eprintln!("{}", t("Соединения через сервер не проходят", "Connections through the server are failing")),
                        Some("stopped") => {
                            println!("{}", t("Отключено", "Disconnected"));
                            return 0;
                        }
                        _ => {}
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    if stopping { return 130 }
                    stopping = true;
                    let _ = tx.send(Cmd::Stop);
                }
                _ = terminate => {
                    if stopping { return 143 }
                    stopping = true;
                    let _ = tx.send(Cmd::Stop);
                }
            }
        }
    });
    let _ = std::fs::remove_file(live_path());
    Ok(code)
}

fn status_cmd(st: &State) -> Result<(), String> {
    if let Some((l, out)) = ask("status", Duration::from_secs(3)) {
        let v: Value = serde_json::from_str(out.trim()).unwrap_or_default();
        let how = if l.tun {
            t("весь трафик (tun)", "all traffic (tun)").to_string()
        } else {
            tr!("прокси 127.0.0.1:{}", "proxy 127.0.0.1:{}", l.listen)
        };
        if v["connected"] == true {
            let up = v["up"].as_u64().unwrap_or_default();
            let down = v["down"].as_u64().unwrap_or_default();
            let since = store::now().saturating_sub(v["since"].as_u64().unwrap_or_default());
            println!(
                "{}",
                tr!(
                    "Подключено: {} ({}) · {} · {} · ↑ {} ↓ {}",
                    "Connected: {} ({}) · {} · {} · ↑ {} ↓ {}",
                    l.server,
                    l.sub,
                    how,
                    span(since),
                    size(up),
                    size(down)
                )
            );
        } else {
            println!("{}", tr!("Подключение: {} ({}) · {}…", "Connecting: {} ({}) · {}…", l.server, l.sub, how));
        }
        return Ok(());
    }
    println!("{}", t("Не подключено", "Not connected"));
    #[cfg(windows)]
    if tun::leftover() == Some(true) {
        println!(
            "{}",
            t(
                "DNS системы заблокирован правилом, оставшимся от прошлого подключения. Снять: rynet-cli disconnect (от администратора)",
                "System DNS is blocked by a rule left by the last connection. Lift it: rynet-cli disconnect (as administrator)"
            )
        );
    }
    #[cfg(unix)]
    if tun::leftover() == Some(true) {
        println!(
            "{}",
            t(
                "Интернет заблокирован защитой от утечки. Снять: rynet-cli disconnect",
                "Internet is blocked by leak protection. Lift it: rynet-cli disconnect"
            )
        );
    }
    if let (Some(id), Some(p)) = (&st.sub, &st.profile) {
        let c = catalog(st);
        let n = c.srvs.iter().find(|v| st.subs.get(v.sub).is_some_and(|s| &s.id == id) && &v.remarks == p).map_or(0, |v| v.n);
        let sub = st.subs.iter().find(|s| &s.id == id).map(sub_name).unwrap_or_default();
        println!("{}", tr!("Выбран: {} {} ({})", "Selected: {} {} ({})", n, p, sub));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn clear_dead_rules() -> Result<(), String> {
    if tun::leftover().is_none() {
        return Ok(());
    }
    if !tun::capable() {
        return Err(tun_hint());
    }
    tun::down();
    println!("{}", t("Сняты правила, оставшиеся от прошлого подключения", "Removed the rules left by the last connection"));
    Ok(())
}

fn disconnect_cmd() -> Result<(), String> {
    if ask("stop", Duration::from_secs(15)).is_some() {
        println!("{}", t("Отключено", "Disconnected"));
        return Ok(());
    }
    #[cfg(unix)]
    if tun::leftover().is_some() {
        if !tun::capable() {
            return Err(tun_hint());
        }
        tun::down();
        println!("{}", t("Сняты правила, оставшиеся от прошлого подключения", "Removed the rules left by the last connection"));
        return Ok(());
    }
    #[cfg(windows)]
    if tun::leftover().is_some() {
        if !tun::capable() {
            return Err(tun_hint());
        }
        if tun::clear_leftover() {
            println!("{}", t("Снято правило DNS, оставшееся от прошлого подключения", "Removed the DNS rule left by the last connection"));
            return Ok(());
        }
        return Err(t(
            "Не удалось снять правило DNS, оставшееся от прошлого подключения (подробности в core.log)",
            "Could not remove the DNS rule left by the last connection (see core.log)",
        )
        .into());
    }
    println!("{}", t("Не подключено", "Not connected"));
    Ok(())
}

const HELP_RU: &str = "\
ryNet {v} — клиент ryNet для терминала

Использование: rynet-cli <команда> [аргументы]

Подписки и серверы
  add <ссылка|файл|->        добавить подписку (https://…) или серверы (vless://…, xray JSON)
  update [подписка]          обновить подписки
  remove <подписка|сервер>   удалить подписку или свой сервер
  list                       подписки и серверы с номерами; * — выбранный
  ping [подписка|сервер …] [--mode proxy|proxy2|tcp|icmp]
                             проверить задержку; без аргументов — все серверы
  use <номер|имя>            выбрать сервер

Подключение
  connect [номер|имя] [--tun|--proxy] [--port N] [--routing subscription|smart|global]
                             подключиться; работает до Ctrl+C или disconnect
  status                     состояние подключения и трафик
  disconnect                 отключиться

Настройки
  settings                   показать настройки и что они значат
  set <ключ> <значение>      изменить; set <ключ> default — вернуть как было

Плагины протоколов и транспортов
  plugin                     список, разрешения и ошибки
  plugin install <файл.rnp> [--grant net_any,net_lan,memory,keepalive]
                             поставить; опасные разрешения — только перечисленные
  plugin enable|disable <плагин>
  plugin grant|deny <плагин> <разрешение>
  plugin set <плагин> <ключ> <значение>   общая настройка плагина
  plugin logs <плагин> [--clear]
  plugin remove <плагин>
  plugin pack <модуль.wasm> [файл.rnp]   упаковать собранный плагин для раздачи

Данные: {dir}
Язык: RYNET_LANG=ru|en. Режим tun в Linux требует права CAP_NET_ADMIN
(sudo setcap cap_net_admin,cap_net_raw+ep rynet-cli), в Windows — администратора.
";

const HELP_EN: &str = "\
ryNet {v} — the ryNet client for the terminal

Usage: rynet-cli <command> [arguments]

Subscriptions and servers
  add <link|file|->          add a subscription (https://…) or servers (vless://…, xray JSON)
  update [subscription]      refresh subscriptions
  remove <subscription|server>  remove a subscription or one of your own servers
  list                       subscriptions and numbered servers; * — selected
  ping [subscription|server …] [--mode proxy|proxy2|tcp|icmp]
                             measure latency; no arguments — all servers
  use <number|name>          select a server

Connection
  connect [number|name] [--tun|--proxy] [--port N] [--routing subscription|smart|global]
                             connect; runs until Ctrl+C or disconnect
  status                     connection state and traffic
  disconnect                 disconnect

Settings
  settings                   show settings and what they mean
  set <key> <value>          change; set <key> default — back to default

Protocol and transport plugins
  plugin                     list, permissions and errors
  plugin install <file.rnp> [--grant net_any,net_lan,memory,keepalive]
                             install; dangerous permissions only as listed
  plugin enable|disable <plugin>
  plugin grant|deny <plugin> <permission>
  plugin set <plugin> <key> <value>   a plugin-wide setting
  plugin logs <plugin> [--clear]
  plugin remove <plugin>
  plugin pack <module.wasm> [file.rnp]   package a built plugin for sharing

Data: {dir}
Language: RYNET_LANG=ru|en. tun mode on Linux needs CAP_NET_ADMIN
(sudo setcap cap_net_admin,cap_net_raw+ep rynet-cli), on Windows — administrator.
";

fn help() {
    let text = if english() { HELP_EN } else { HELP_RU };
    print!("{}", text.replace("{v}", VERSION.trim()).replace("{dir}", &dir().display().to_string()));
}

#[cfg(unix)]
fn system_russian() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .is_some_and(|v| v.starts_with("ru"))
}

#[cfg(windows)]
fn system_russian() -> bool {
    unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() & 0x3ff == 0x19 }
}

#[cfg(feature = "plugins")]
fn find_plugin(q: &str) -> Result<Value, String> {
    let all = rv_core::plugin::list();
    let q = q.trim().to_ascii_lowercase();
    all.as_array()
        .into_iter()
        .flatten()
        .find(|p| {
            let m = &p["manifest"];
            m["id"].as_str().is_some_and(|s| s.eq_ignore_ascii_case(&q))
                || m["name"].as_str().is_some_and(|s| s.eq_ignore_ascii_case(&q))
                || m["protocols"].as_array().into_iter().flatten().any(|x| x.as_str().is_some_and(|s| s.eq_ignore_ascii_case(&q)))
        })
        .cloned()
        .ok_or_else(|| tr!("Нет плагина «{}». Список: rynet-cli plugin", "No plugin \"{}\". List: rynet-cli plugin", q))
}

#[cfg(feature = "plugins")]
fn danger_label(k: &str) -> &'static str {
    match k {
        "net_any" => t("любые адреса в интернете", "any internet address"),
        "net_lan" => t("локальная сеть", "local network"),
        "memory" => t("больше 64 МБ памяти", "over 64 MB of memory"),
        "keepalive" => t("таймеры в простое (батарея)", "idle timers (battery)"),
        _ => "?",
    }
}

#[cfg(feature = "plugins")]
fn print_plugin(p: &Value) {
    let m = &p["manifest"];
    let strs = |v: &Value| v.as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
    let on = if p["enabled"].as_bool() == Some(true) { t("вкл", "on") } else { t("выкл", "off") };
    println!(
        "{} {} [{}] {} — {}: {}",
        m["name"].as_str().unwrap_or("?"),
        m["version"].as_str().unwrap_or(""),
        on,
        m["id"].as_str().unwrap_or(""),
        m["kind"].as_str().unwrap_or(""),
        strs(&m["protocols"])
    );
    let denied: Vec<&str> = p["denied"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    for d in p["dangerous"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let state = if denied.contains(&d) { t("не дано", "denied") } else { t("дано", "granted") };
        println!("    {d}: {} — {state}", danger_label(d));
    }
    let diag = &p["diag"];
    if let Some(e) = diag["last_error"].as_str().filter(|e| !e.is_empty()) {
        println!("    {}: {e}", t("последняя ошибка", "last error"));
    }
}

#[cfg(feature = "plugins")]
fn plugin_cmd(rest: &[String]) -> Result<(), String> {
    let sub = rest.first().map_or("list", String::as_str);
    let arg = |i: usize, usage: &str| {
        rest.get(i).cloned().ok_or_else(|| tr!("Использование: rynet-cli plugin {}", "Usage: rynet-cli plugin {}", usage))
    };
    match sub {
        "list" | "ls" => {
            let all = rv_core::plugin::list();
            let all = all.as_array().cloned().unwrap_or_default();
            if all.is_empty() {
                println!(
                    "{}",
                    t(
                        "Плагинов нет. Поставить: rynet-cli plugin install <файл.rnp>",
                        "No plugins. Install one: rynet-cli plugin install <file.rnp>"
                    )
                );
            }
            all.iter().for_each(print_plugin);
        }
        "install" | "add" => {
            let file = arg(1, "install <file.rnp> [--grant net_any,net_lan,memory,keepalive]")?;
            let bytes = std::fs::read(&file).map_err(|e| format!("{file}: {e}"))?;
            let grant: Vec<String> = rest
                .iter()
                .position(|a| a == "--grant")
                .and_then(|i| rest.get(i + 1))
                .map(|g| g.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();
            let ins = rv_core::plugin::inspect(&bytes)?;
            let asked: Vec<String> = ins[if ins["installed"].is_null() { "dangerous" } else { "new_dangerous" }]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            let denied: Vec<String> = asked.iter().filter(|d| !grant.contains(d)).cloned().collect();
            let mut keep: Vec<String> = match ins["manifest"]["id"].as_str().map(find_plugin) {
                Some(Ok(old)) => old["denied"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect(),
                _ => Vec::new(),
            };
            keep.retain(|d| !grant.contains(d));
            keep.extend(denied.iter().cloned());
            keep.sort();
            keep.dedup();
            let p = rv_core::plugin::install(&bytes, keep)?;
            print_plugin(&p);
            if !denied.is_empty() {
                println!(
                    "{}",
                    tr!(
                        "Опасные разрешения не даны: {}. Дать: rynet-cli plugin grant <плагин> <разрешение>",
                        "Dangerous permissions not granted: {}. Grant: rynet-cli plugin grant <plugin> <permission>",
                        denied.join(", ")
                    )
                );
            }
            if let Some(c) = ins["conflicts"].as_array().filter(|c| !c.is_empty()) {
                println!(
                    "{}",
                    tr!(
                        "Те же протоколы уже у: {}; новый плагин выключен",
                        "The same protocols are already handled by: {}; the new plugin is off",
                        c.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
                    )
                );
            }
        }
        "pack" => {
            let input = arg(1, "pack <plugin.wasm> [out.rnp]")?;
            let bytes = std::fs::read(&input).map_err(|e| format!("{input}: {e}"))?;
            let (m, rnp) = rv_core::plugin::pack(&bytes)?;
            let out = rest.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| {
                std::path::Path::new(&input).with_file_name(format!("{}-{}.{}", m.id, m.version, rv_core::plugin::FILE_EXTENSION))
            });
            std::fs::write(&out, &rnp).map_err(|e| format!("{}: {e}", out.display()))?;
            println!("{} {} {} -> {}", m.name, m.version, t("упакован", "packed"), out.display());
        }
        "remove" | "rm" => {
            let p = find_plugin(&arg(1, "remove <plugin>")?)?;
            rv_core::plugin::remove(p["manifest"]["id"].as_str().unwrap_or_default())?;
            println!("{}", t("Удалён", "Removed"));
        }
        "enable" | "disable" => {
            let p = find_plugin(&arg(1, "enable|disable <plugin>")?)?;
            print_plugin(&rv_core::plugin::set_enabled(p["manifest"]["id"].as_str().unwrap_or_default(), sub == "enable")?);
        }
        "grant" | "deny" => {
            let p = find_plugin(&arg(1, "grant|deny <plugin> <permission>")?)?;
            let perm = arg(2, "grant|deny <plugin> <permission>")?;
            if !p["dangerous"].as_array().into_iter().flatten().any(|d| d.as_str() == Some(&perm)) {
                return Err(tr!("Плагин не просит «{}»", "The plugin doesn't ask for \"{}\"", perm));
            }
            let mut denied: Vec<String> =
                p["denied"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).filter(|d| *d != perm).collect();
            if sub == "deny" {
                denied.push(perm);
            }
            print_plugin(&rv_core::plugin::set_denied(p["manifest"]["id"].as_str().unwrap_or_default(), denied)?);
        }
        "set" => {
            let usage = "set <plugin> <key> <value>";
            let p = find_plugin(&arg(1, usage)?)?;
            let key = arg(2, usage)?;
            let value = rest
                .get(3..)
                .map(|v| v.join(" "))
                .ok_or_else(|| tr!("Использование: rynet-cli plugin {}", "Usage: rynet-cli plugin {}", usage))?;
            let field = p["manifest"]["global"].as_array().into_iter().flatten().find(|f| f["key"].as_str() == Some(&key)).cloned();
            let Some(field) = field else {
                return Err(tr!("У плагина нет общей настройки «{}»", "The plugin has no global setting \"{}\"", key));
            };
            let v = match field["type"].as_str().unwrap_or("text") {
                "bool" => Value::Bool(matches!(value.as_str(), "on" | "true" | "1" | "yes")),
                "int" | "port" => value.parse::<i64>().map(Value::from).map_err(|_| t("Нужно число", "A number is needed").to_string())?,
                "list" => {
                    Value::Array(value.split(',').map(|s| Value::String(s.trim().to_string())).filter(|s| s.as_str() != Some("")).collect())
                }
                _ => Value::String(value),
            };
            let mut g = p["global"].as_object().cloned().unwrap_or_default();
            g.insert(key, v);
            print_plugin(&rv_core::plugin::set_global(p["manifest"]["id"].as_str().unwrap_or_default(), &Value::Object(g))?);
        }
        "logs" | "log" => {
            let p = find_plugin(&arg(1, "logs <plugin> [--clear]")?)?;
            let id = p["manifest"]["id"].as_str().unwrap_or_default();
            if rest.iter().any(|a| a == "--clear") {
                rv_core::plugin::clear_logs(id);
                return Ok(());
            }
            for l in rv_core::plugin::logs(id).as_array().into_iter().flatten() {
                let lvl = match l["level"].as_u64() {
                    Some(1) => "E",
                    Some(2) => "W",
                    Some(3) => "I",
                    _ => "D",
                };
                println!("{} {lvl} {}", l["time"].as_u64().unwrap_or(0), l["message"].as_str().unwrap_or(""));
            }
        }
        other => {
            return Err(tr!(
                "Неизвестная команда plugin «{}». Справка: rynet-cli --help",
                "Unknown plugin command \"{}\". Help: rynet-cli --help",
                other
            ));
        }
    }
    Ok(())
}

fn run(args: &[String]) -> Result<i32, String> {
    let cmd = args.first().map_or("", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        help();
        return Ok(0);
    }
    let need = |n: usize, usage: &str| -> Result<(), String> {
        if rest.len() < n { Err(tr!("Использование: rynet-cli {}", "Usage: rynet-cli {}", usage)) } else { Ok(()) }
    };
    for e in rv_core::plugin::init(&store::plugins_dir()) {
        eprintln!("plugin: {e}");
    }
    let mut st = State::load();
    match cmd {
        "" | "help" | "--help" | "-h" => help(),
        #[cfg(feature = "plugins")]
        "plugin" | "plugins" => plugin_cmd(rest)?,
        #[cfg(not(feature = "plugins"))]
        "plugin" | "plugins" => return Err(rv_core::plugin::call(cmd, &Value::Null, None).unwrap_err()),
        "--version" | "-V" | "version" => println!("rynet-cli {}", VERSION.trim()),
        "add" => {
            need(1, t("add <ссылка|файл|->", "add <link|file|->"))?;
            add_cmd(&mut st, &rest[0])?;
        }
        "update" => return update_cmd(&mut st, rest.first().map(String::as_str)),
        "remove" | "rm" => {
            need(1, t("remove <подписка|сервер>", "remove <subscription|server>"))?;
            remove_cmd(&mut st, &rest.join(" "))?;
        }
        "list" | "ls" => {
            if st.subs.is_empty() {
                println!("{}", t("Нет подписок. Добавьте: rynet-cli add <ссылка>", "No subscriptions. Add one: rynet-cli add <link>"));
            } else {
                print_subs(&st, &catalog(&st), None);
            }
        }
        "ping" => return ping_cmd(&mut st, rest),
        "use" => {
            need(1, t("use <номер|имя>", "use <number|name>"))?;
            use_cmd(&mut st, &rest.join(" "))?;
        }
        "connect" | "up" => return connect_cmd(&mut st, rest),
        "status" => status_cmd(&st)?,
        "disconnect" | "down" => disconnect_cmd()?,
        "settings" | "config" => settings_cmd(&st),
        "set" => {
            need(2, t("set <ключ> <значение>", "set <key> <value>"))?;
            set_cmd(&mut st, &rest[0], &rest[1..].join(" "))?;
            st.save();
            println!("{} = {}", rest[0], value_of(&st, &rest[0]));
        }
        other => {
            return Err(tr!(
                "Неизвестная команда «{}». Справка: rynet-cli --help",
                "Unknown command \"{}\". Help: rynet-cli --help",
                other
            ));
        }
    }
    Ok(0)
}

fn main() {
    rv_desktop::set_emit(emit);
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let en = match std::env::var("RYNET_LANG").ok().as_deref() {
        Some("en") => true,
        Some("ru") => false,
        _ => !system_russian(),
    };
    rv_core::lang::set_english(en);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        1
    });
    std::process::exit(code);
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    #[test]
    fn ipv4_only_setting_reaches_connection_and_ping_options() {
        let mut state = State::default();
        let sub = Sub { id: "test".into(), ..Sub::default() };
        assert_eq!(options(&state, &sub).unwrap()["ipv4Only"], false);
        set_cmd(&mut state, "ipv4-only", "on").unwrap();
        let restored: Settings = serde_json::from_value(serde_json::to_value(&state.settings).unwrap()).unwrap();
        assert!(restored.ipv4_only);
        assert_eq!(options(&state, &sub).unwrap()["ipv4Only"], true);
        set_cmd(&mut state, "ipv4-only", "default").unwrap();
        assert_eq!(options(&state, &sub).unwrap()["ipv4Only"], false);
    }
}

#[cfg(test)]
mod accepted_settings_tests {
    use super::*;
    #[test]
    fn cli_roundtrip_preserves_platform_settings_and_subscription_overrides() {
        let input = serde_json::json!({"settings":{"exitIpUrl":"https://ip.example/address","expiryNotifications":true,"futureSetting":{"enabled":true},"subHeaders":"X-Test: global"},"subs":[{"id":"s","url":"https://example/sub","clientOverride":"happ","headersOverride":"","updateIntervalMinutes":45,"futureSub":{"v":1}}],"futureState":"keep"});
        let state: State = serde_json::from_value(input).unwrap();
        let output = serde_json::to_value(&state).unwrap();
        assert_eq!(output["settings"]["futureSetting"]["enabled"], true);
        assert_eq!(output["settings"]["exitIpUrl"], "https://ip.example/address");
        assert_eq!(output["settings"]["expiryNotifications"], true);
        assert_eq!(output["subs"][0]["headersOverride"], "");
        assert_eq!(output["subs"][0]["clientOverride"], "happ");
        assert_eq!(output["subs"][0]["updateIntervalMinutes"], 45);
        assert_eq!(output["subs"][0]["futureSub"]["v"], 1);
        assert_eq!(output["futureState"], "keep");
        let mut state = state;
        state.settings.sub_client = "happ".into();
        assert!(client(&state, "rynet").agent.unwrap().starts_with("ryNet/"));
        assert_eq!(client(&state, "happ").agent.unwrap(), "Happ/4.3.0");
        assert!(subscription_headers("User-Agent: test\nX-Token: yes").is_ok());
        assert!(subscription_headers("Bad Header: value").is_err());
        assert!(subscription_headers("X: bad\0value").is_err());
        assert!(subscription_headers("").unwrap().is_empty());
    }
}
