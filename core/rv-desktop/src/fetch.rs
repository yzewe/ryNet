use std::io::Read;
use std::time::Duration;

use base64::Engine;

use crate::store::{self, Sub};

const HAPP_AGENT: &str = "Happ/4.3.0";

fn agent(https_only: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .https_only(https_only)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

fn cleartext(url: &str) -> bool {
    url.trim_start().get(..7).is_some_and(|p| p.eq_ignore_ascii_case("http://"))
}

fn b64(s: &str) -> Option<String> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let t = s.trim();
    [STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|e| e.decode(t.trim_end_matches('=')).ok().or_else(|| e.decode(t).ok()))
        .and_then(|b| String::from_utf8(b).ok())
}

fn text(v: Option<String>) -> Option<String> {
    let v = v?.trim().to_string();
    let v = match v.strip_prefix("base64:") {
        Some(b) => b64(b)?,
        None => v,
    };
    Some(v.trim().to_string()).filter(|s| !s.is_empty())
}

fn happ_routing(link: Option<String>) -> Option<String> {
    let link = link?;
    let text = link.trim();
    let data = if let Some(at) = text.to_ascii_lowercase().find("://routing/") {
        let rest = &text[at + 11..];
        let kind = rest.split(['/', '?']).next()?;
        if !kind.eq_ignore_ascii_case("add") && !kind.eq_ignore_ascii_case("onadd") {
            return None;
        }
        rest.split_once("?data=").map(|(_, b)| b).or_else(|| rest.split_once('/').map(|(_, b)| b))?
    } else {
        text
    };
    let bytes = data.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = ((bytes[i + 1] as char).to_digit(16), (bytes[i + 2] as char).to_digit(16))
        {
            decoded.push((h << 4 | l) as u8);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let json = b64(std::str::from_utf8(&decoded).ok()?.trim())?;
    serde_json::from_str::<serde_json::Value>(&json).ok().filter(|v| v.is_object()).map(|v| v.to_string())
}

fn routing_line(body: &str) -> Option<String> {
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    let plain = b64(&compact).unwrap_or_else(|| body.to_string());
    plain
        .lines()
        .map(str::trim)
        .find(|l| l.split_once("://").is_some_and(|(_, r)| r.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("routing/"))))
        .map(str::to_string)
}

pub struct Client {
    pub agent: Option<String>,
    pub headers: Vec<(String, String)>,
    pub hwid: Option<String>,
    pub happ_retry: bool,
}

struct Got {
    headers: Vec<(String, String)>,
    body: String,
}

fn device_os() -> (&'static str, String) {
    #[cfg(windows)]
    {
        ("Windows", windows_version())
    }
    #[cfg(target_os = "macos")]
    {
        ("macOS", macos_version())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        ("Linux", std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim().to_string())
    }
}

#[cfg(windows)]
fn windows_version() -> String {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
    let mut v: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    v.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
    if unsafe { windows_sys::Wdk::System::SystemServices::RtlGetVersion(&mut v) } != 0 {
        return String::new();
    }
    match (v.dwMajorVersion, v.dwMinorVersion) {
        (10, 0) if v.dwBuildNumber >= 22000 => "11".into(),
        (10, 0) => "10".into(),
        (6, 3) => "8.1".into(),
        (6, 2) => "8".into(),
        (6, 1) => "7".into(),
        (a, b) => format!("{a}.{b}"),
    }
}

#[cfg(target_os = "macos")]
fn macos_version() -> String {
    let mut buf = [0u8; 32];
    let mut len = buf.len();
    let r = unsafe { libc::sysctlbyname(c"kern.osproductversion".as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
    if r != 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..len]).trim_end_matches('\0').to_string()
}

fn get(url: &str, ua: &str, client: &Client, retry_after: &mut Option<String>) -> Result<Got, String> {
    let mut req = agent(!cleartext(url)).get(url).header("User-Agent", ua);
    if let Some(hwid) = &client.hwid
        && !cleartext(url)
    {
        let (os, version) = device_os();
        req = req.header("x-hwid", hwid).header("x-device-os", os).header("x-ver-os", version).header("x-device-model", "PC");
    }
    for (k, v) in &client.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let mut resp =
        req.call().map_err(|e| format!("{}: {e}", rv_core::lang::t("Не удалось загрузить подписку", "Couldn't load the subscription")))?;
    let headers =
        resp.headers().iter().map(|(k, v)| (k.as_str().to_ascii_lowercase(), v.to_str().unwrap_or("").to_string())).collect::<Vec<_>>();
    let status = resp.status().as_u16();
    if matches!(status, 429 | 503) {
        *retry_after = headers.iter().find(|(k, _)| k == "retry-after").map(|(_, v)| v.chars().take(128).collect());
    }
    let mut body = String::new();
    resp.body_mut()
        .as_reader()
        .take(32 << 20)
        .read_to_string(&mut body)
        .map_err(|e| format!("{}: {e}", rv_core::lang::t("Подписка", "Subscription")))?;
    if headers.iter().any(|(k, _)| k == "x-hwid-max-devices-reached") {
        return Err(rv_core::lang::t("Достигнут лимит устройств в подписке", "The subscription's device limit is reached").into());
    }
    if !(200..300).contains(&status) {
        return Err(format!("{} {status}", rv_core::lang::t("Панель ответила кодом", "The panel responded with code")));
    }
    Ok(Got { headers, body })
}

pub fn need_hwid() -> String {
    rv_core::lang::t(
        "Провайдер подписки требует HWID: включите «Отправлять HWID»",
        "The subscription provider requires HWID: turn on \"Send HWID\"",
    )
    .into()
}

fn hwid_cleartext() -> String {
    rv_core::lang::t(
        "Провайдер подписки требует HWID, а ссылка без шифрования (http://): по такой ссылке HWID не отправляется. Нужна ссылка https://",
        "The subscription provider requires HWID, but the link is unencrypted (http://): HWID is not sent over such a link. An https:// link is needed",
    )
    .into()
}

pub fn describe(text: &str) -> Result<Vec<serde_json::Value>, String> {
    let v = rv_core::describe::describe(text)
        .map_err(|e| format!("{}: {e}", rv_core::lang::t("Не похоже на подписку", "This doesn't look like a subscription")))?;
    Ok(v.as_array().cloned().unwrap_or_default())
}

pub fn download(url: &str, id: Option<&str>, client: &Client) -> Result<(Sub, String), String> {
    download_metadata(url, id, client, &mut None)
}

pub fn download_metadata(url: &str, id: Option<&str>, client: &Client, retry_after: &mut Option<String>) -> Result<(Sub, String), String> {
    *retry_after = None;
    let version = env!("CARGO_PKG_VERSION");
    let ua = client.agent.clone().unwrap_or_else(|| {
        let (os, os_version) = device_os();
        format!("ryNet/{version} ({os} {os_version})").replace(" )", ")")
    });
    let mut got = get(url, &ua, client, retry_after)?;
    let only_stubs = |t: &str| describe(t).map(|p| !p.is_empty() && p.iter().all(|x| x["placeholder"] == true)).unwrap_or(true);
    if client.happ_retry && only_stubs(&got.body) {
        got = get(url, HAPP_AGENT, client, retry_after)?;
    }
    let sent_hwid = client.hwid.is_some() && !cleartext(url);
    let wants_hwid = !sent_hwid && got.headers.iter().any(|(k, _)| k == "x-hwid-not-supported");
    let hwid_error = || if client.hwid.is_some() { hwid_cleartext() } else { need_hwid() };
    let profiles = describe(&got.body).map_err(|e| if wants_hwid { hwid_error() } else { e })?;
    if profiles.iter().all(|p| p["placeholder"] == true) {
        if wants_hwid {
            return Err(hwid_error());
        }
        return Err(rv_core::lang::t(
            "Подписка без серверов (панель требует HWID или не поддерживает клиент)",
            "The subscription has no servers (the panel requires HWID or doesn't support the client)",
        )
        .into());
    }
    let h = |name: &str| -> Option<String> {
        got.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).or_else(|| {
            got.body
                .lines()
                .map(str::trim)
                .take_while(|l| l.is_empty() || l.starts_with('#'))
                .filter_map(|l| l.trim_start_matches('#').split_once(':'))
                .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_string())
        })
    };
    let ui: Vec<(String, u64)> = h("subscription-userinfo")
        .unwrap_or_default()
        .split(';')
        .filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().parse().unwrap_or(0))))
        .collect();
    let u = |k: &str| ui.iter().find(|(n, _)| n == k).map_or(0, |(_, v)| *v);
    let link = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| s.starts_with("https://") || s.starts_with("http://"));
    let sub = Sub {
        id: id.map(str::to_string).unwrap_or_else(store::new_id),
        url: migrated(url, h("new-url"), h("new-domain")),
        title: text(h("profile-title")),
        upload: u("upload"),
        download: u("download"),
        total: u("total"),
        expire: u("expire"),
        updated: store::now(),
        interval_hours: h("profile-update-interval").and_then(|v| v.trim().parse().ok()),
        announce: text(h("announce")),
        support_url: link(h("support-url")),
        web_page: link(h("profile-web-page-url")),
        client_override: None,
        headers_override: None,
        update_interval_minutes: None,
        extra: Default::default(),
        routing: happ_routing(h("routing").or_else(|| routing_line(&got.body))),
    };
    let geo_text = format!("{}\n{}", got.body, sub.routing.clone().unwrap_or_default());
    std::thread::spawn(move || update_geo(&geo_text));
    Ok((sub, got.body))
}

fn migrated(url: &str, new_url: Option<String>, new_domain: Option<String>) -> String {
    if let Some(u) =
        new_url.map(|u| u.trim().to_string()).filter(|u| u.starts_with("https://") || (u.starts_with("http://") && cleartext(url)))
    {
        return u;
    }
    let Some(d) =
        new_domain.map(|d| d.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/').to_string())
    else {
        return url.to_string();
    };
    if d.is_empty() || !d.chars().all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c)) {
        return url.to_string();
    }
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let path = rest.find('/').map_or("", |i| &rest[i..]);
            format!("{scheme}://{d}{path}")
        }
        None => url.to_string(),
    }
}

const GEO_LISTS_MAX: usize = 256;

fn geo_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "!@._-".contains(c))
}

fn referenced(text: &str) -> Vec<(String, String)> {
    let mut out = vec![("geosite".to_string(), "category-ru".to_string()), ("geoip".to_string(), "ru".to_string())];
    for l in text.lines() {
        let parts: Vec<&str> = l.trim().trim_start_matches("- ").split(',').map(str::trim).collect();
        if parts.len() >= 2 && (parts[0].eq_ignore_ascii_case("GEOSITE") || parts[0].eq_ignore_ascii_case("GEOIP")) {
            let item = (parts[0].to_ascii_lowercase(), parts[1].to_ascii_lowercase());
            if geo_name(&item.1) && !out.contains(&item) && out.len() < GEO_LISTS_MAX {
                out.push(item);
            }
        }
    }
    for kind in ["geosite", "geoip"] {
        let pat = format!("\"{kind}:");
        let mut rest = text;
        while let Some(i) = rest.find(&pat) {
            rest = &rest[i + pat.len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "!@._-".contains(*c))
                .collect::<String>()
                .trim_start_matches('!')
                .to_ascii_lowercase();
            if geo_name(&name)
                && !(kind == "geoip" && name == "private")
                && !out.contains(&(kind.to_string(), name.clone()))
                && out.len() < GEO_LISTS_MAX
            {
                out.push((kind.to_string(), name));
            }
        }
    }
    out
}

pub fn update_geo(text: &str) {
    crate::geodat::update(text);
    let dir = store::geo_dir();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into();
    for (kind, name) in referenced(text) {
        let urls = [
            format!("https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/meta/geo/{kind}/{name}.list"),
            format!("https://cdn.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/{kind}/{name}.list"),
        ];
        for url in &urls {
            let Ok(mut resp) = agent.get(url).call() else { continue };
            if resp.status().as_u16() == 404 {
                break;
            }
            if resp.status().as_u16() != 200 {
                continue;
            }
            let mut body = String::new();
            if resp.body_mut().as_reader().take(64 << 20).read_to_string(&mut body).is_ok() && !body.trim().is_empty() {
                let path = dir.join(format!("{kind}-{name}.list"));
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, body).is_ok() && std::fs::rename(&tmp, &path).is_ok() {
                    tracing::info!("geo {kind}:{name} updated");
                    break;
                }
            }
        }
    }
}

pub fn ensure_geo() {
    let dir = store::geo_dir();
    let week = Duration::from_secs(7 * 24 * 3600);
    let fresh = |f: &str| {
        std::fs::metadata(dir.join(f)).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < week)
    };
    if !fresh("geosite-category-ru.list") || !fresh("geoip-ru.list") {
        std::thread::spawn(|| update_geo(""));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_links_match_android() {
        let json = base64::engine::general_purpose::STANDARD.encode(r#"{"Name":"RU","DirectSites":["geosite:ru"]}"#);
        let expected = happ_routing(Some(json.clone())).unwrap();
        for link in [
            format!("happ://routing/onadd/{json}"),
            format!("incy://routing/onadd/{json}"),
            format!("INCY://ROUTING/ADD/{json}"),
            format!("://routing/add?data={}", json.replace('=', "%3D")),
        ] {
            assert_eq!(happ_routing(Some(link)).as_deref(), Some(expected.as_str()));
        }
        let body = format!("incy://routing/onadd/{json}\ntrojan://pw@example.org:443#Test");
        assert_eq!(happ_routing(routing_line(&body)).as_deref(), Some(expected.as_str()));
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        assert_eq!(happ_routing(routing_line(&format!("{}\n{}", &encoded[..20], &encoded[20..]))).as_deref(), Some(expected.as_str()));
        assert!(happ_routing(Some("incy://routing/off".into())).is_none());
        assert!(happ_routing(Some(base64::engine::general_purpose::STANDARD.encode("[]"))).is_none());
    }

    #[test]
    fn headers_and_geo() {
        assert_eq!(text(Some("base64:0J/RgNC40LLQtdGC".into())).as_deref(), Some("Привет"));
        assert_eq!(text(Some(" plain ".into())).as_deref(), Some("plain"));
        let r = referenced(r#"{"domain":["geosite:youtube","geosite:!cn"],"ip":["geoip:private","geoip:NL"]}"#);
        assert!(r.contains(&("geosite".into(), "youtube".into())));
        assert!(r.contains(&("geosite".into(), "cn".into())));
        assert!(r.contains(&("geoip".into(), "nl".into())));
        assert!(!r.contains(&("geoip".into(), "private".into())));
        let json = base64::engine::general_purpose::STANDARD.encode(r#"{"GlobalProxy":"true"}"#);
        assert!(happ_routing(Some(format!("happ://routing/onadd/{json}"))).is_some());
        assert!(happ_routing(Some("happ://routing/off".into())).is_none());
        assert_eq!(migrated("https://a.example/sub/x?y=1", None, Some("b.example".into())), "https://b.example/sub/x?y=1");
        assert_eq!(migrated("https://a.example/s", Some("https://c.example/t".into()), None), "https://c.example/t");
    }

    #[test]
    fn no_downgrade_to_http_and_geo_names_are_plain() {
        assert_eq!(migrated("https://a.example/s", Some("http://c.example/t".into()), None), "https://a.example/s");
        assert_eq!(migrated("http://a.example/s", Some("http://c.example/t".into()), None), "http://c.example/t");
        assert_eq!(migrated("http://a.example/s", Some("https://c.example/t".into()), None), "https://c.example/t");
        assert_eq!(migrated("https://a.example/s", None, Some("http://b.example/".into())), "https://b.example/s");
        assert!(cleartext("http://a.example") && cleartext(" HTTP://a.example") && !cleartext("https://a.example"));

        let r = referenced(
            "GEOSITE,../../../.config/autostart/x,PROXY\nGEOIP,a/b,DIRECT\n- GEOSITE,Google,PROXY\nGEOSITE,x?y#z,PROXY\nGEOIP,..,DIRECT",
        );
        assert_eq!(r, vec![("geosite".into(), "category-ru".into()), ("geoip".into(), "ru".into()), ("geosite".into(), "google".into())]);
        assert!(geo_name("category-ads-all") && geo_name("geolocation-!cn") && geo_name("cn@ads"));
        assert!(
            !geo_name("")
                && !geo_name(".hidden")
                && !geo_name("a..b")
                && !geo_name("a/b")
                && !geo_name("a\\b")
                && !geo_name(&"a".repeat(65))
        );
        let many: String = (0..1000).map(|i| format!("GEOSITE,list{i},PROXY\n")).collect();
        assert_eq!(referenced(&many).len(), GEO_LISTS_MAX);
    }
}

#[cfg(test)]
mod retry_metadata_tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn retry_after_survives_a_rejected_fetch_without_retrying() {
        for header in ["7200", "Wed, 21 Oct 2037 07:28:00 GMT"] {
            let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = server.local_addr().unwrap();
            let header = header.to_string();
            let expected = header.clone();
            let thread = std::thread::spawn(move || {
                let (mut s, _) = server.accept().unwrap();
                let mut buf = [0; 4096];
                let _ = s.read(&mut buf);
                s.write_all(
                    format!("HTTP/1.1 429 Too Many Requests\r\nRetry-After: {header}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .unwrap();
            });
            let client = Client { agent: Some("ryNet/test".into()), headers: vec![], hwid: None, happ_retry: true };
            let mut retry = None;
            let result = download_metadata(&format!("http://{addr}/sub"), None, &client, &mut retry);
            assert!(result.unwrap_err().contains("429"));
            assert_eq!(retry.as_deref(), Some(expected.as_str()));
            thread.join().unwrap();
        }
    }
}
