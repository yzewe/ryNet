pub mod grpc;
mod http3;
pub mod http_header;
pub mod httpupgrade;
pub mod ws;

use std::rc::Rc;

use rv_config::{Network, Outbound, Profile, Security};
use rv_net::Protect;

use crate::outbound::dial::Dialer;
use crate::{BoxStream, Error};

pub enum Kind {
    Raw,
    HttpHeader(Box<rv_config::HttpHeader>),
    Xhttp(Rc<rv_xhttp::transport::Transport>),
    Grpc(Rc<grpc::Pool>),
    Ws(Rc<ws::Settings>),
    HttpUpgrade(Rc<httpupgrade::Settings>),
    #[cfg(feature = "plugins")]
    Plugin(Rc<crate::plugin::outbound::PluginOutbound>),
}

pub struct Stream {
    pub dialer: Dialer,
    pub kind: Kind,
}

impl Stream {
    pub fn reset(&self) {
        match &self.kind {
            Kind::Xhttp(t) => t.reset(),
            Kind::Grpc(p) => p.reset(),
            #[cfg(feature = "plugins")]
            Kind::Plugin(p) => p.reset(),
            Kind::Raw | Kind::HttpHeader(_) | Kind::Ws(_) | Kind::HttpUpgrade(_) => {}
        }
        self.dialer.reset();
    }

    pub fn new(profile: &Profile, ob: &Outbound, address: &str, port: u16, protect: Option<Protect>) -> Result<Self, Error> {
        let stream = &ob.stream;
        let dialer = Dialer::new(profile, stream, address, port, protect.clone())?;
        let kind = match &stream.network {
            Network::Tcp => Kind::Raw,
            Network::Xhttp(v) => {
                let cfg = rv_xhttp::request::Config::from_json(v).map_err(Error::Config)?;
                use rv_xhttp::transport::HttpVersion;
                let version = match &stream.security {
                    Security::None => HttpVersion::Http1,
                    Security::Tls(t) if t.alpn == ["http/1.1"] => HttpVersion::Http1,
                    Security::Tls(t) if t.alpn == ["h3"] => HttpVersion::Http3,
                    _ => HttpVersion::Http2,
                };
                let server_name = dialer.tls.as_ref().map(|t| t.server_name.clone()).unwrap_or_default();
                let d = dialer.clone();
                let connect: rv_xhttp::transport::Connector = Rc::new(move || {
                    let d = d.clone();
                    Box::pin(async move {
                        if d.tls.is_none() {
                            return Ok(Box::new(d.tcp().await?) as Box<dyn rv_xhttp::transport::Io>);
                        }
                        let s = d.tls().await.map_err(std::io::Error::other)?;
                        let expected: &[u8] = match version {
                            HttpVersion::Http1 => b"http/1.1",
                            HttpVersion::Http2 => b"h2",
                            HttpVersion::Http3 => b"h3",
                        };
                        if s.alpn().is_some_and(|a| a != expected) {
                            return Err(std::io::Error::other(format!(
                                "xhttp: server did not select {}",
                                String::from_utf8_lossy(expected)
                            )));
                        }
                        Ok(Box::new(s) as Box<dyn rv_xhttp::transport::Io>)
                    })
                });
                let download = if let Some(settings) = cfg.download_settings() {
                    let host = settings
                        .get("address")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| Error::Config("xhttp downloadSettings.address is required".into()))?;
                    let port = settings
                        .get("port")
                        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                        .and_then(|p| u16::try_from(p).ok())
                        .filter(|p| *p > 0)
                        .ok_or_else(|| Error::Config("xhttp downloadSettings.port is required".into()))?;
                    let mut s = settings.clone();
                    for key in ["xhttpSettings", "splithttpSettings"] {
                        if let Some(x) = s.get_mut(key).filter(|x| x.is_object() || x.is_null()) {
                            x["downloadSettings"] = serde_json::Value::Null;
                            if let Some(e) = x.get_mut("extra").filter(|v| v.is_object()) {
                                e["downloadSettings"] = serde_json::Value::Null;
                            }
                        }
                    }
                    let raw = serde_json::json!({"outbounds": [{"protocol": "freedom", "tag": "download", "streamSettings": s}]});
                    let parsed = rv_config::parse_subscription(&raw.to_string()).map_err(|e| Error::Config(e.to_string()))?;
                    if let rv_config::OutboundKind::Unsupported { reason, .. } = &parsed[0].outbounds[0].kind {
                        return Err(Error::Config(format!("xhttp downloadSettings: {reason}")));
                    }
                    let mut down_ob = ob.clone();
                    down_ob.stream = parsed[0].outbounds[0].stream.clone();
                    if !matches!(down_ob.stream.network, Network::Xhttp(_)) {
                        return Err(Error::Config("xhttp downloadSettings.network must be xhttp".into()));
                    }
                    match Self::new(profile, &down_ob, host, port, protect.clone())?.kind {
                        Kind::Xhttp(t) => Some(t),
                        _ => unreachable!(),
                    }
                } else {
                    None
                };
                let t = if version == HttpVersion::Http3 {
                    if dialer.chained() {
                        return Err(Error::Unsupported("xhttp h3 through dialerProxy requires a UDP tunnel".into()));
                    }
                    let (connect, keepalive) = http3::connector(ob, address, port, protect, cfg.xmux().h_keep_alive_period)?;
                    rv_xhttp::transport::Transport::new_h3(cfg, &server_name, address, connect, download, keepalive)
                } else {
                    rv_xhttp::transport::Transport::new_with_download_version(
                        cfg,
                        dialer.reality,
                        &server_name,
                        address,
                        connect,
                        download,
                        version,
                    )
                };
                Kind::Xhttp(t)
            }
            Network::Grpc(g) => {
                let authority = if !g.authority.is_empty() {
                    g.authority.clone()
                } else if let Security::Tls(t) = &stream.security
                    && !t.server_name.is_empty()
                {
                    t.server_name.clone()
                } else if !dialer.reality && address.parse::<std::net::IpAddr>().is_err() {
                    address.to_string()
                } else if address.contains(':') {
                    format!("[{address}]:{port}")
                } else {
                    format!("{address}:{port}")
                };
                Kind::Grpc(grpc::Pool::new(g, dialer.clone(), authority))
            }
            Network::Ws(w) => {
                if dialer.reality {
                    return Err(Error::Unsupported("websocket over reality".into()));
                }
                let host = dialer.server_name().to_string();
                return Ok(Self { dialer, kind: Kind::Ws(Rc::new(ws::Settings::new(w, &host))) });
            }
            Network::HttpUpgrade(h) => {
                if dialer.reality {
                    return Err(Error::Unsupported("httpupgrade over reality".into()));
                }
                let host = dialer.server_name().to_string();
                return Ok(Self { dialer, kind: Kind::HttpUpgrade(Rc::new(httpupgrade::Settings::new(h, &host))) });
            }
            Network::TcpHttpHeader => {
                Kind::HttpHeader(stream.http_header.clone().ok_or_else(|| Error::Config("tcp http header settings are missing".into()))?)
            }
            #[cfg(feature = "plugins")]
            Network::Other(name) if crate::plugin::for_transport(name).is_some() => {
                if crate::plugin::for_transport(name).is_some_and(|p| p.manifest.carries == crate::plugin::manifest::Carries::Datagram) {
                    return Err(Error::Unsupported(format!(
                        "network {name}: a datagram transport works only under plugin protocols (WireGuard and others)"
                    )));
                }
                Kind::Plugin(Rc::new(crate::plugin::outbound::PluginOutbound::transport(profile, ob, name, address, port, protect)?))
            }
            Network::Other(name) => {
                return Err(Error::Unsupported(format!(
                    "network {name}: {}",
                    crate::lang::t("нужен плагин этого транспорта", "needs a plugin for this transport")
                )));
            }
            other => return Err(Error::Unsupported(format!("network {}", other.name()))),
        };
        Ok(Self { dialer, kind })
    }

    pub async fn open(&self) -> Result<BoxStream, Error> {
        Ok(match &self.kind {
            Kind::Raw => self.go_writes(self.dialer.stream().await?),
            Kind::HttpHeader(h) => Box::new(http_header::HeaderStream::new(self.dialer.stream().await?, h)),
            Kind::Xhttp(x) => Box::new(x.open().await?),
            Kind::Grpc(g) => Box::new(g.open().await?),
            Kind::Ws(s) => Box::new(ws::open(s.clone(), self.go_writes(self.dialer.stream().await?)).await?),
            Kind::HttpUpgrade(s) => self.go_writes(Box::new(httpupgrade::open(s.clone(), self.dialer.stream().await?).await?)),
            #[cfg(feature = "plugins")]
            Kind::Plugin(p) => p.open_stream().await?,
        })
    }

    fn go_writes(&self, s: BoxStream) -> BoxStream {
        if self.dialer.tls.is_some() { Box::new(rv_xhttp::activity::GoWrites::new(s, rv_xhttp::activity::XRAY_BUFFER)) } else { s }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn download_settings_with_odd_xhttp_settings_do_not_panic() {
        for odd in [json!("x"), json!(1), json!([1]), json!(true)] {
            let stream = json!({"network": "xhttp", "security": "tls", "tlsSettings": {"serverName": "a.example"},
                "xhttpSettings": {"downloadSettings": {"address": "b.example", "port": 443, "xhttpSettings": odd, "splithttpSettings": odd}}});
            let text = json!({"outbounds": [{"tag": "proxy", "protocol": "vless", "streamSettings": stream,
                "settings": {"vnext": [{"address": "a.example", "port": 443, "users": [{"id": "11111111-2222-3333-4444-555555555555"}]}]}}]}).to_string();
            let p = rv_config::parse_subscription(&text).unwrap().remove(0);
            let _ = crate::Proxy::build(&p, "proxy", None);
        }
    }
}
