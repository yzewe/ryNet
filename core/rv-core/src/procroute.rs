pub fn parse_list(s: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for n in s.split([',', ';', '\n', '\r']).map(str::trim).filter(|n| !n.is_empty()) {
        if n.contains(['\\', '/']) {
            return Err(format!("{n}: нужно имя файла (chrome.exe), не путь"));
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(n)) {
            out.push(n.to_string());
        }
    }
    Ok(out)
}

fn rules(names: &[String], include: bool) -> (String, String) {
    if names.is_empty() {
        return Default::default();
    }
    let target = if include { "PROXY" } else { "DIRECT" };
    let head: String = names.iter().map(|n| format!("PROCESS-NAME,{n},{target}\n")).collect();
    (head, if include { "\nMATCH,DIRECT\n".into() } else { String::new() })
}

pub fn wrap(user_rules: &str, list: &str, mode: &str) -> Result<String, String> {
    let names = parse_list(list)?;
    let (head, tail) = rules(&names, mode.eq_ignore_ascii_case("include"));
    Ok(format!("{head}{user_rules}{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_parsing() {
        assert_eq!(parse_list(" chrome.exe, Steam.exe;\nfirefox.exe\r\n,,CHROME.EXE").unwrap(), ["chrome.exe", "Steam.exe", "firefox.exe"]);
        assert_eq!(parse_list("My App.exe").unwrap(), ["My App.exe"]);
        assert!(parse_list("").unwrap().is_empty());
        assert!(parse_list("C:\\Program Files\\a.exe").is_err());
    }

    #[test]
    fn empty_list_changes_nothing() {
        assert_eq!(wrap("DOMAIN,a.com,DIRECT", "", "include").unwrap(), "DOMAIN,a.com,DIRECT");
    }

    #[test]
    fn exclude_goes_direct_before_user_rules() {
        let t = wrap("DOMAIN,a.com,REJECT", "chrome.exe", "exclude").unwrap();
        assert_eq!(t, "PROCESS-NAME,chrome.exe,DIRECT\nDOMAIN,a.com,REJECT");
    }

    #[test]
    fn include_proxies_listed_and_closes_with_direct() {
        let t = wrap("DOMAIN,a.com,REJECT", "chrome.exe,steam.exe", "include").unwrap();
        assert_eq!(t, "PROCESS-NAME,chrome.exe,PROXY\nPROCESS-NAME,steam.exe,PROXY\nDOMAIN,a.com,REJECT\nMATCH,DIRECT\n");
    }

    fn route(text: &str, process: &str) -> String {
        use rv_rules::{Network, RouteCtx, Router};
        let t = rv_rules::mihomo::Targets { direct: "direct".into(), reject: "block".into(), proxy: "proxy".into(), known: Vec::new() };
        let tr = rv_rules::mihomo::translate(text, &t);
        assert!(tr.warnings.is_empty(), "{:?}", tr.warnings);
        let r = Router::new(&serde_json::json!({ "rules": tr.rules }), &Default::default()).unwrap();
        let ctx = RouteCtx {
            domain: Some("example.com"),
            ip: None,
            port: 443,
            network: Network::Tcp,
            protocol: None,
            inbound_tag: None,
            process: Some(process),
        };
        match r.route(&ctx) {
            rv_rules::Decision::Outbound(t) => t,
            _ => "none".into(),
        }
    }

    #[test]
    fn matching_through_the_rule_engine() {
        let inc = wrap("", "Chrome.exe", "include").unwrap();
        assert_eq!(route(&inc, "chrome.exe"), "proxy");
        assert_eq!(route(&inc, "curl.exe"), "direct");
        let exc = wrap("", "chrome.exe", "exclude").unwrap();
        assert_eq!(route(&exc, "CHROME.EXE"), "direct");
        assert_eq!(route(&exc, "curl.exe"), "none");
    }
}
