use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use ring::rand::{SecureRandom, SystemRandom};

pub type Headers = Vec<(String, String)>;

struct Versions {
    chrome: i64,
    firefox: i64,
    safari: String,
    curl: String,
}

fn rand_f64() -> f64 {
    let mut b = [0u8; 8];
    let _ = SystemRandom::new().fill(&mut b);
    (u64::from_le_bytes(b) >> 11) as f64 / (1u64 << 53) as f64
}

fn days_since(y: i32, m: u32, d: u32) -> i64 {
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m as i32 - 3) + 2) / 5 + d as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let then = (era * 146097 + doe - 719468) as i64;
    now_days() - then
}

fn now_days() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64 / 86400).unwrap_or(0)
}

fn versions() -> &'static Versions {
    static V: OnceLock<Versions> = OnceLock::new();
    V.get_or_init(|| {
        let chrome = {
            let diff = days_since(2026, 1, 13) - 35 - (rand_f64().powi(2) * 105.0).floor() as i64;
            144 + diff.div_euclid(35)
        };
        let firefox = {
            let diff = days_since(2024, 7, 29) - 25 - (rand_f64().powi(2) * 50.0).floor() as i64;
            128 + diff.div_euclid(30)
        };
        let curl = {
            let diff = days_since(2023, 3, 20) - 60 - (rand_f64().powi(2) * 165.0).floor() as i64;
            format!("8.{}.0", diff.div_euclid(57))
        };
        let safari = safari_version();
        Versions { chrome, firefox, safari, curl }
    })
}

fn safari_version() -> String {
    const MINOR: [i64; 25] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 5, 5, 6, 6, 6, 6];
    let delay = (rand_f64().powi(3) * 75.0).floor() as i64;
    let today = now_days();
    let mut year = 1970 + (today as f64 / 365.2425) as i32;
    let split = |y: i32| now_days() - days_since(y, 9, 23) + delay;
    if split(year) > today {
        year -= 1;
    }
    if split(year) > today {
        year -= 1;
    }
    let idx = ((today - split(year)) * 86400 / 1_296_000).clamp(0, 24) as usize;
    format!("{}.{}", year - 1999, MINOR[idx])
}

pub fn chrome_version() -> i64 {
    versions().chrome
}

pub fn chrome_ua() -> String {
    format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
        chrome_version()
    )
}

fn firefox_ua() -> String {
    let v = versions().firefox;
    format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:{v}.0) Gecko/20100101 Firefox/{v}.0")
}

fn safari_ua() -> String {
    format!(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{} Safari/605.1.15",
        versions().safari
    )
}

pub fn chrome_ch_ua(version: i64, fork: &str) -> String {
    let grease = [" ", "(", ":", "-", ".", "/", ")", ";", "=", "?", "_"];
    let na = ["8", "99", "24"];
    let seed = version as usize;
    let mut brands = vec![
        format!("\"Not{}A{}Brand\";v=\"{}\"", grease[seed % 11], grease[(seed + 1) % 11], na[seed % 3]),
        format!("\"Chromium\";v=\"{version}\""),
    ];
    match fork {
        "chrome" => brands.push(format!("\"Google Chrome\";v=\"{version}\"")),
        "edge" => brands.push(format!("\"Microsoft Edge\";v=\"{version}\"")),
        _ => {}
    }
    const S3: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    let mut out = vec![String::new(); brands.len()];
    match brands.len() {
        3 => {
            for (i, &e) in S3[seed % 6].iter().enumerate() {
                out[e] = brands[i].clone();
            }
        }
        2 => {
            out[seed % 2] = brands[0].clone();
            out[(seed + 1) % 2] = brands[1].clone();
        }
        _ => out = brands,
    }
    out.join(", ")
}

pub fn get<'a>(h: &'a Headers, k: &str) -> Option<&'a str> {
    h.iter().find(|x| x.0.eq_ignore_ascii_case(k)).map(|x| x.1.as_str())
}

pub fn set(h: &mut Headers, k: &str, v: &str) {
    match h.iter_mut().find(|x| x.0.eq_ignore_ascii_case(k)) {
        Some(x) => {
            x.0 = k.into();
            x.1 = v.into();
        }
        None => h.push((k.into(), v.into())),
    }
}

fn set_default(h: &mut Headers, k: &str, v: &str) {
    if get(h, k).is_none_or(str::is_empty) {
        set(h, k, v);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Ws,
    Fetch,
    Nav,
}

pub fn apply_defaults(h: &mut Headers, variant: Variant) {
    let browser = match get(h, "User-Agent") {
        None => "chrome".to_string(),
        Some(ua) => ua.to_string(),
    };
    let v = versions().chrome;
    match browser.as_str() {
        "chrome" | "edge" => {
            let fork = browser.as_str();
            set(h, "Sec-CH-UA", &chrome_ch_ua(v, fork));
            set(h, "Sec-CH-UA-Mobile", "?0");
            set(h, "Sec-CH-UA-Platform", "\"Windows\"");
            set(h, "DNT", "1");
            let ua = if fork == "edge" { format!("{}Edg/{v}.0.0.0", chrome_ua()) } else { chrome_ua() };
            set(h, "User-Agent", &ua);
            set(h, "Accept-Language", "en-US,en;q=0.9");
        }
        "firefox" => {
            set(h, "User-Agent", &firefox_ua());
            set(h, "DNT", "1");
            set(h, "Accept-Language", "en-US,en;q=0.5");
        }
        "safari" => {
            set(h, "User-Agent", &safari_ua());
            set(h, "Accept-Language", "en-US,en;q=0.9");
        }
        "golang" => {
            set(
                h,
                "User-Agent",
                match variant {
                    Variant::Fetch => "Go-http-client/2.0",
                    Variant::Ws | Variant::Nav => "Go-http-client/1.1",
                },
            );
            return;
        }
        "curl" => {
            set(h, "User-Agent", &format!("curl/{}", versions().curl));
            return;
        }
        _ => return,
    }
    match variant {
        Variant::Nav => {
            if browser == "chrome" || browser == "edge" {
                set_default(h, "Cache-Control", "max-age=0");
            }
            set(h, "Upgrade-Insecure-Requests", "1");
            set_default(
                h,
                "Accept",
                if browser == "chrome" || browser == "edge" {
                    "text/html,application/xhtml+xml,application/xml;q=0.9,image/jxl,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"
                } else {
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
                },
            );
            set(h, "Sec-Fetch-Site", "none");
            set(h, "Sec-Fetch-Mode", "navigate");
            if browser != "safari" {
                set(h, "Sec-Fetch-User", "?1");
            }
            set(h, "Sec-Fetch-Dest", "document");
            set(h, "Priority", "u=0, i");
        }
        Variant::Ws => {
            set(h, "Sec-Fetch-Mode", "websocket");
            set(h, "Sec-Fetch-Dest", if browser == "safari" { "websocket" } else { "empty" });
            set(h, "Sec-Fetch-Site", "same-origin");
            set_default(h, "Cache-Control", "no-cache");
            set_default(h, "Pragma", "no-cache");
            set_default(h, "Accept", "*/*");
        }
        Variant::Fetch => {
            set(h, "Sec-Fetch-Mode", "cors");
            set(h, "Sec-Fetch-Dest", "empty");
            set(h, "Sec-Fetch-Site", "same-origin");
            let priority = match browser.as_str() {
                "chrome" | "edge" => Some("u=1, i"),
                "firefox" => Some("u=4"),
                "safari" => Some("u=3, i"),
                _ => None,
            };
            if let Some(p) = priority {
                set_default(h, "Priority", p);
            }
            set_default(h, "Cache-Control", "no-cache");
            set_default(h, "Pragma", "no-cache");
            set_default(h, "Accept", "*/*");
        }
    }
}

pub fn http1_request(method: &str, path: &str, host: &str, headers: &Headers) -> Vec<u8> {
    let mut out = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n");
    if let Some(ua) = get(headers, "User-Agent").filter(|u| !u.is_empty()) {
        out.push_str(&format!("User-Agent: {ua}\r\n"));
    }
    let mut rest: Vec<&(String, String)> = headers
        .iter()
        .filter(|(k, _)| {
            !["host", "user-agent", "content-length", "transfer-encoding", "trailer"].contains(&k.to_ascii_lowercase().as_str())
        })
        .collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in rest {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_defaults_like_xray() {
        let mut h = Headers::new();
        apply_defaults(&mut h, Variant::Ws);
        assert!(get(&h, "User-Agent").unwrap().contains("Chrome/"));
        assert_eq!(get(&h, "Sec-Fetch-Mode"), Some("websocket"));
        assert_eq!(get(&h, "Sec-Fetch-Dest"), Some("empty"));
        let v = chrome_version();
        assert!((145..=160).contains(&v), "chrome {v}");
        let req = String::from_utf8(http1_request("GET", "/p", "h", &h)).unwrap();
        let lines: Vec<&str> = req.lines().collect();
        assert_eq!(lines[1], "Host: h");
        assert!(lines[2].starts_with("User-Agent: "));
        assert!(lines[3].starts_with("Accept: "));
    }

    #[test]
    fn custom_user_agent_keeps_user_headers_unchanged() {
        let mut h = vec![("User-Agent".into(), "custom/1.0".into()), ("Accept".into(), "application/test".into())];
        let before = h.clone();
        for variant in [Variant::Ws, Variant::Fetch, Variant::Nav] {
            apply_defaults(&mut h, variant);
            assert_eq!(h, before);
        }
    }

    #[test]
    fn ch_ua_matches_xray_shape() {
        assert_eq!(chrome_ch_ua(150, "chrome"), "\"Not;A=Brand\";v=\"8\", \"Chromium\";v=\"150\", \"Google Chrome\";v=\"150\"");
    }
}
