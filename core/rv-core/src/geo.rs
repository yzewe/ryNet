use std::net::IpAddr;
use std::path::Path;

pub use rv_rules::GeoData;

pub fn load_dir(dir: &Path) -> GeoData {
    let mut geo = GeoData::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return geo;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".list") else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        if let Some(code) = stem.strip_prefix("geoip-") {
            geo.geoip.insert(code.to_ascii_lowercase(), parse_cidrs(&text));
        } else if let Some(cat) = stem.strip_prefix("geosite-") {
            geo.geosite.insert(cat.to_ascii_lowercase(), parse_domains(&text));
        }
    }
    geo
}

pub fn parse_cidrs(text: &str) -> Vec<(IpAddr, u8)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (ip, len) = l.split_once('/').unwrap_or((l, if l.contains(':') { "128" } else { "32" }));
            Some((ip.parse().ok()?, len.parse().ok()?))
        })
        .collect()
}

pub fn parse_domains(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            if ["domain:", "full:", "keyword:", "regexp:"].iter().any(|p| l.starts_with(p)) {
                l.to_string()
            } else {
                match l.strip_prefix("+.") {
                    Some(d) => format!("domain:{d}"),
                    None => match l.strip_prefix("*.") {
                        Some(d) => format!("domain:{d}"),
                        None => format!("full:{l}"),
                    },
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(
            parse_cidrs("2.16.20.0/23\n# c\n\n2a00::/16\n1.2.3.4"),
            vec![("2.16.20.0".parse().unwrap(), 23), ("2a00::".parse().unwrap(), 16), ("1.2.3.4".parse().unwrap(), 32)]
        );
        assert_eq!(parse_domains("+.yandex\nst-ok.cdn-vk.ru\n"), vec!["domain:yandex", "full:st-ok.cdn-vk.ru"]);
        assert_eq!(
            parse_domains("keyword:tiktok\nregexp:^a\\.b$\ndomain:x.ru\nfull:y.ru"),
            vec!["keyword:tiktok", "regexp:^a\\.b$", "domain:x.ru", "full:y.ru"]
        );
    }
}
