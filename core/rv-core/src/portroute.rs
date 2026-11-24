pub fn parse_list(s: &str) -> Result<Vec<String>, String> {
    let port = |t: &str, whole: &str| -> Result<u16, String> {
        match t.trim().parse::<u16>() {
            Ok(n) if n >= 1 => Ok(n),
            _ => Err(format!("{whole}: порт — число от 1 до 65535")),
        }
    };
    let mut out: Vec<String> = Vec::new();
    for n in s.split([',', ';', '\n', '\r']).map(str::trim).filter(|n| !n.is_empty()) {
        let item = match n.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (port(a, n)?, port(b, n)?);
                if a >= b {
                    return Err(format!("{n}: в диапазоне первый порт должен быть меньше второго"));
                }
                format!("{a}-{b}")
            }
            None => port(n, n)?.to_string(),
        };
        if !out.contains(&item) {
            out.push(item);
        }
    }
    Ok(out)
}

fn rules(ports: &[String]) -> String {
    ports.iter().map(|p| format!("DST-PORT,{p},DIRECT\n")).collect()
}

pub fn wrap(rules_text: &str, list: &str) -> Result<String, String> {
    let ports = parse_list(list)?;
    Ok(format!("{}{rules_text}", rules(&ports)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_parsing() {
        assert_eq!(parse_list("443, 8000-8100; 5060\n 443,\r\n 80 - 90").unwrap(), ["443", "8000-8100", "5060", "80-90"]);
        assert!(parse_list("").unwrap().is_empty());
        assert!(parse_list(" , ;").unwrap().is_empty());
    }

    #[test]
    fn garbage_is_rejected() {
        for bad in ["0", "65536", "abc", "-5", "80-", "90-80", "80-80", "1-2-3", "443 80", "1.5"] {
            assert!(parse_list(bad).is_err(), "{bad}");
        }
        assert!(parse_list("65535").is_ok());
        assert!(parse_list("1-65535").is_ok());
    }

    #[test]
    fn empty_list_changes_nothing() {
        assert_eq!(wrap("DOMAIN,a.com,REJECT", "").unwrap(), "DOMAIN,a.com,REJECT");
    }

    #[test]
    fn ports_come_first() {
        assert_eq!(
            wrap("DOMAIN,a.com,REJECT", "443,8000-8100").unwrap(),
            "DST-PORT,443,DIRECT\nDST-PORT,8000-8100,DIRECT\nDOMAIN,a.com,REJECT"
        );
    }

    fn route(text: &str, port: u16, network: rv_rules::Network, process: &str) -> String {
        use rv_rules::{RouteCtx, Router};
        let t = rv_rules::mihomo::Targets { direct: "direct".into(), reject: "block".into(), proxy: "proxy".into(), known: Vec::new() };
        let tr = rv_rules::mihomo::translate(text, &t);
        assert!(tr.warnings.is_empty(), "{:?}", tr.warnings);
        let r = Router::new(&serde_json::json!({ "rules": tr.rules }), &Default::default()).unwrap();
        let ctx =
            RouteCtx { domain: Some("example.com"), ip: None, port, network, protocol: None, inbound_tag: None, process: Some(process) };
        match r.route(&ctx) {
            rv_rules::Decision::Outbound(t) => t,
            _ => "none".into(),
        }
    }

    #[test]
    fn matching_tcp_udp_and_ranges() {
        use rv_rules::Network::{Tcp, Udp};
        let t = wrap("DOMAIN,example.com,REJECT", "443, 8000-8100").unwrap();
        for net in [Tcp, Udp] {
            assert_eq!(route(&t, 443, net, "a.exe"), "direct");
            assert_eq!(route(&t, 8000, net, "a.exe"), "direct");
            assert_eq!(route(&t, 8100, net, "a.exe"), "direct");
            assert_eq!(route(&t, 8101, net, "a.exe"), "block");
            assert_eq!(route(&t, 442, net, "a.exe"), "block");
        }
    }

    #[test]
    fn direct_port_wins_in_process_include_mode() {
        use rv_rules::Network::Tcp;
        let inner = crate::procroute::wrap("", "chrome.exe", "include").unwrap();
        let t = wrap(&inner, "443").unwrap();
        assert_eq!(route(&t, 443, Tcp, "chrome.exe"), "direct");
        assert_eq!(route(&t, 80, Tcp, "chrome.exe"), "proxy");
        assert_eq!(route(&t, 80, Tcp, "curl.exe"), "direct");
    }
}
