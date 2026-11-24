use rv_config::{Fragment, Freedom, Outbound, OutboundKind, Profile, Range, StreamSettings};

const VIA: &str = "chain-";
const APP: &str = "rynet-app";
pub(crate) const FRAGMENT: &str = "rynet-fragment";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frag {
    pub length: (u64, u64),
    pub interval_ms: (u64, u64),
}

#[derive(Clone, Default)]
pub struct Tune {
    pub via: Option<Profile>,
    pub servers: Vec<(Profile, String)>,
    pub missing: Vec<String>,
    pub fragment: Option<Frag>,
    pub keepalive: bool,
    pub ipv4_only: bool,
}

impl Tune {
    pub fn is_empty(&self) -> bool {
        self.via.is_none()
            && self.servers.is_empty()
            && self.missing.is_empty()
            && self.fragment.is_none()
            && !self.keepalive
            && !self.ipv4_only
    }

    pub fn from_json(v: &serde_json::Value) -> Result<Self, String> {
        let mut t = Tune::default();
        if let Some(c) = v.get("chain").filter(|c| c.is_object()) {
            let config = c.get("config").and_then(|x| x.as_str()).unwrap_or_default();
            let name = c.get("profile").and_then(|x| x.as_str()).unwrap_or_default();
            if !config.is_empty() && !name.is_empty() {
                let all = rv_config::parse_subscription(config).map_err(|e| e.to_string())?;
                let p = rv_config::find_profile(&all, name).ok_or_else(|| {
                    if crate::lang::english() {
                        format!("Chain: no server \"{name}\"")
                    } else {
                        format!("Цепочка: нет сервера «{name}»")
                    }
                })?;
                t.via = Some(p);
            }
        }
        for a in v.get("servers").and_then(|a| a.as_array()).into_iter().flatten() {
            let config = a.get("config").and_then(|x| x.as_str()).unwrap_or_default();
            let name = a.get("profile").and_then(|x| x.as_str()).unwrap_or_default();
            let tag = a.get("tag").and_then(|x| x.as_str()).unwrap_or_default();
            if config.is_empty() || name.is_empty() || !tag.starts_with(APP) || tag.contains(',') {
                continue;
            }
            let all = rv_config::parse_subscription(config).map_err(|e| e.to_string())?;
            match rv_config::find_profile(&all, name) {
                Some(p) => t.servers.push((p, tag.to_string())),
                None => t.missing.push(tag.to_string()),
            }
        }
        if let Some(f) = v.get("fragment").filter(|f| f.is_object()) {
            let range = |k: &str| f.get(k).and_then(Range::parse).map(|r| (r.from, r.to)).ok_or_else(|| format!("fragment.{k}"));
            t.fragment = Some(Frag { length: range("length")?, interval_ms: range("interval")? });
        }
        t.ipv4_only = v.get("ipv4Only").and_then(|v| v.as_bool()).unwrap_or(false);
        t.keepalive = v.get("keepalive").and_then(|k| k.as_bool()).unwrap_or(false);
        Ok(t)
    }

    pub fn apply(&self, profile: &mut Profile) -> Result<(), String> {
        if let Some(via) = &self.via {
            chain(profile, via)?;
        }
        for (server, tag) in &self.servers {
            pin(profile, server, tag);
        }
        for tag in &self.missing {
            profile.outbounds.push(Outbound {
                tag: tag.clone(),
                kind: OutboundKind::Blackhole { http: false },
                stream: StreamSettings::default(),
                reject_udp443: false,
            });
        }
        if let Some(f) = self.fragment {
            fragment(profile, f);
        }
        if self.ipv4_only {
            for ob in &mut profile.outbounds {
                ob.stream.sockopt.family = rv_config::Family::V4 { force: true };
            }
        }
        if self.keepalive {
            keepalive(profile);
        }
        Ok(())
    }
}

fn pin(profile: &mut Profile, server: &Profile, tag: &str) {
    let Some(main) = server.main_outbound().or_else(|| server.outbounds.first()).map(|o| o.tag.clone()) else { return };
    let renamed = |t: &str| if t == main { tag.to_string() } else { format!("{tag}-{t}") };
    for ob in &server.outbounds {
        let mut ob = ob.clone();
        ob.tag = renamed(&ob.tag);
        if let Some(dp) = ob.stream.sockopt.dialer_proxy.take() {
            ob.stream.sockopt.dialer_proxy = Some(renamed(&dp));
        }
        profile.outbounds.push(ob);
    }
}

const HYSTERIA_KEEPALIVE: u64 = 15;

fn keepalive(profile: &mut Profile) {
    for ob in profile.outbounds.iter_mut().filter(|o| matches!(o.kind, OutboundKind::Hysteria2(_))) {
        let q = ob.stream.finalmask.quic.get_or_insert_with(Default::default);
        if q.keep_alive_period == 0 {
            q.keep_alive_period = HYSTERIA_KEEPALIVE;
        }
    }
}

fn chain(profile: &mut Profile, via: &Profile) -> Result<(), String> {
    let entry =
        via.main_outbound().ok_or(crate::lang::t("Цепочка: у входного сервера нет прокси", "Chain: the entry server has no proxy"))?;
    let entry_tag = format!("{VIA}{}", entry.tag);
    for ob in profile.outbounds.iter_mut().filter(|o| o.kind.is_proxy()) {
        match &ob.kind {
            OutboundKind::Hysteria2(_) => {
                ob.kind = OutboundKind::Unsupported {
                    protocol: "hysteria".into(),
                    reason: crate::lang::t("Hysteria нельзя выходом цепочки: это UDP", "Hysteria cannot be a chain exit: it is UDP").into(),
                };
            }
            OutboundKind::Unsupported { .. } => {}
            _ => {
                ob.stream.sockopt.dialer_proxy = Some(entry_tag.clone());
                ob.stream.sockopt.dialer_proxy_ip = true;
            }
        }
    }
    let main = profile.main_outbound().ok_or(crate::lang::t("Цепочка: у сервера нет прокси", "Chain: the server has no proxy"))?;
    if let OutboundKind::Unsupported { reason, .. } = &main.kind {
        return Err(reason.clone());
    }
    let renamed = |t: &str| format!("{VIA}{t}");
    for ob in &via.outbounds {
        let mut ob = ob.clone();
        ob.tag = renamed(&ob.tag);
        if let Some(dp) = ob.stream.sockopt.dialer_proxy.take() {
            ob.stream.sockopt.dialer_proxy = Some(renamed(&dp));
        }
        profile.outbounds.push(ob);
    }
    Ok(())
}

fn fragment(profile: &mut Profile, f: Frag) {
    let mut any = false;
    for ob in profile.outbounds.iter_mut() {
        let tcp = matches!(
            ob.kind,
            OutboundKind::Vless(_)
                | OutboundKind::Vmess(_)
                | OutboundKind::Trojan(_)
                | OutboundKind::Shadowsocks(_)
                | OutboundKind::Socks(_)
                | OutboundKind::Http(_)
        );
        let reality = matches!(ob.stream.security, rv_config::Security::Reality(_));
        if tcp && !reality && ob.stream.sockopt.dialer_proxy.is_none() {
            ob.stream.sockopt.dialer_proxy = Some(FRAGMENT.into());
            any = true;
        }
    }
    if any {
        profile.outbounds.push(Outbound {
            tag: FRAGMENT.into(),
            kind: OutboundKind::Freedom(Freedom {
                fragment: Some(Fragment {
                    packets: (0, 1),
                    length: Range { from: f.length.0, to: f.length.1 },
                    interval_ms: Range { from: f.interval_ms.0, to: f.interval_ms.1 },
                    max_split: Range { from: 0, to: 0 },
                }),
            }),
            stream: StreamSettings::default(),
            reject_udp443: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(link: &str) -> Profile {
        rv_config::parse_subscription(link).unwrap().remove(0)
    }

    const EXIT: &str =
        "vless://9205c503-358f-4d8d-a081-20c98eddf64a@exit.example:443?encryption=none&type=tcp&security=tls&sni=exit.example#exit";
    const ENTRY: &str = "trojan://pw@entry.example:443?type=tcp&security=tls&sni=entry.example#entry";

    #[test]
    fn ipv4_only_forces_every_outbound_after_chain_and_keeps_default() {
        let mut p = profile(ENTRY);
        let default = Tune::from_json(&serde_json::json!({})).unwrap();
        assert!(!default.ipv4_only);
        let tune = Tune::from_json(&serde_json::json!({"ipv4Only":true})).unwrap();
        assert!(!tune.is_empty());
        tune.apply(&mut p).unwrap();
        assert!(p.outbounds.iter().all(|ob| matches!(ob.stream.sockopt.family, rv_config::Family::V4 { force: true })));
    }

    #[test]
    fn chain_routes_exit_through_entry_and_fragments_entry_only() {
        let mut p = profile(EXIT);
        let tune = Tune { via: Some(profile(ENTRY)), fragment: Some(Frag { length: (10, 20), interval_ms: (1, 2) }), ..Default::default() };
        tune.apply(&mut p).unwrap();
        let main = p.main_outbound().unwrap();
        let via = main.stream.sockopt.dialer_proxy.clone().unwrap();
        assert!(via.starts_with(VIA));
        assert!(main.stream.sockopt.dialer_proxy_ip);
        let entry = p.outbound(&via).unwrap();
        assert!(matches!(entry.kind, OutboundKind::Trojan(_)));
        assert_eq!(entry.stream.sockopt.dialer_proxy.as_deref(), Some(FRAGMENT));
        assert!(!entry.stream.sockopt.dialer_proxy_ip);
        assert!(matches!(&p.outbound(FRAGMENT).unwrap().kind, OutboundKind::Freedom(f) if f.fragment.is_some()));
        assert!(matches!(main.kind, OutboundKind::Vless(_)));
    }

    #[test]
    fn rule_servers_get_their_tags_and_missing_ones_block() {
        let v = serde_json::json!({"servers": [
            {"config": ENTRY, "profile": "entry", "tag": "rynet-app0"},
            {"config": ENTRY, "profile": "gone", "tag": "rynet-app1"},
            {"config": ENTRY, "profile": "entry", "tag": "proxy"},
        ]});
        let t = Tune::from_json(&v).unwrap();
        let mut p = profile(EXIT);
        t.apply(&mut p).unwrap();
        assert!(matches!(p.outbound("rynet-app0").unwrap().kind, OutboundKind::Trojan(_)));
        assert!(matches!(p.outbound("rynet-app1").unwrap().kind, OutboundKind::Blackhole { .. }));
        assert!(matches!(p.main_outbound().unwrap().kind, OutboundKind::Vless(_)));
        assert!(!p.outbounds.iter().any(|o| o.tag.starts_with("proxy-")));
        assert_eq!(t.servers.len(), 1);
        let targets = rv_rules::mihomo::Targets {
            direct: "direct".into(),
            reject: "block".into(),
            proxy: "proxy".into(),
            known: vec!["rynet-app0".into()],
        };
        let tr =
            rv_rules::mihomo::translate("PROCESS-NAME,org.telegram.messenger,rynet-app0\nDOMAIN-SUFFIX,youtube.com,rynet-app0", &targets);
        assert!(tr.warnings.is_empty(), "{:?}", tr.warnings);
        assert_eq!(tr.rules[0]["outboundTag"], "rynet-app0");
        assert_eq!(tr.rules[1]["outboundTag"], "rynet-app0");
    }

    #[test]
    fn reality_is_not_fragmented() {
        let reality = "vless://9205c503-358f-4d8d-a081-20c98eddf64a@r.example:443?encryption=none&flow=xtls-rprx-vision&type=tcp&security=reality&sni=www.example.com&fp=chrome&pbk=D-nWk6c566_IKoExMW7koFwczBHF4nIaRGQo93HDNRE&sid=a1b2#r";
        let mut p = profile(reality);
        Tune { fragment: Some(Frag { length: (10, 20), interval_ms: (1, 2) }), ..Default::default() }.apply(&mut p).unwrap();
        assert!(p.main_outbound().unwrap().stream.sockopt.dialer_proxy.is_none());
        assert!(p.outbound(FRAGMENT).is_none());
    }

    #[test]
    fn hysteria_cannot_be_exit() {
        let mut p = profile("hysteria2://a@h.example:443?sni=h.example#h");
        let tune = Tune { via: Some(profile(ENTRY)), ..Default::default() };
        assert!(tune.apply(&mut p).unwrap_err().contains("Hysteria"));
    }

    #[test]
    fn from_json_reads_both() {
        let v = serde_json::json!({
            "chain": {"config": ENTRY, "profile": "entry"},
            "fragment": {"length": "100-200", "interval": "10-20"},
        });
        let t = Tune::from_json(&v).unwrap();
        assert_eq!(t.via.unwrap().remarks, "entry");
        assert_eq!(t.fragment, Some(Frag { length: (100, 200), interval_ms: (10, 20) }));
        assert!(Tune::from_json(&serde_json::json!({})).unwrap().is_empty());
        assert!(Tune::from_json(&serde_json::json!({"chain": false, "fragment": true})).unwrap().is_empty());
        assert!(Tune::from_json(&serde_json::json!({"keepalive": true})).unwrap().keepalive);
    }

    #[test]
    fn keepalive_only_for_hysteria_without_own_period() {
        let sub = "hysteria2://pw@h.example:443?sni=h.example#h\nhysteria2://pw@k.example:443?sni=k.example#k\ntrojan://pw@t.example:443?security=tls&sni=t.example#t";
        let all = rv_config::parse_subscription(sub).unwrap();
        let t = Tune { keepalive: true, ..Default::default() };
        let period = |p: &Profile| p.main_outbound().unwrap().stream.finalmask.quic.as_ref().map(|q| q.keep_alive_period);
        let mut h = rv_config::find_profile(&all, "h").unwrap();
        t.apply(&mut h).unwrap();
        assert_eq!(period(&h), Some(HYSTERIA_KEEPALIVE));
        let mut k = rv_config::find_profile(&all, "k").unwrap();
        k.outbounds[0].stream.finalmask.quic.get_or_insert_with(Default::default).keep_alive_period = 25;
        t.apply(&mut k).unwrap();
        assert_eq!(period(&k), Some(25));
        let mut tr = rv_config::find_profile(&all, "t").unwrap();
        t.apply(&mut tr).unwrap();
        assert_eq!(period(&tr), None);
    }
}
