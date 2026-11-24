use std::cell::RefCell;
use std::io;
use std::net::SocketAddr;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use rv_config::{Outbound, Profile};
use rv_plugin_abi::{conn, ev};
use rv_vless::Address;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::instance::{Instance, drive};
use super::res::{AppConn, AppStream, AppUdp, Res, encode_target};
use super::{Ctx, Kind, Plugin};
use crate::{BoxStream, Error};

const READY_TIMEOUT: Duration = Duration::from_secs(20);

pub struct PluginOutbound {
    ctx: Rc<Ctx>,
    inst: RefCell<Weak<Instance>>,
    failed: RefCell<Option<(std::time::Instant, String)>>,
}

fn other(e: impl Into<String>) -> Error {
    Error::Io(io::Error::other(e.into()))
}

impl PluginOutbound {
    pub fn new(
        profile: &Profile,
        ob: &Outbound,
        protocol: &str,
        settings: &Value,
        protect: Option<rv_net::Protect>,
    ) -> Result<Self, Error> {
        let plugin = super::for_protocol(protocol).ok_or_else(|| {
            Error::Unsupported(format!(
                "{protocol}: {}",
                crate::lang::t("нужен плагин этого протокола", "needs a plugin for this protocol")
            ))
        })?;
        if plugin.manifest.kind == Kind::Transport {
            return Err(Error::Config(format!("{} is a transport, not a protocol", plugin.manifest.name)));
        }
        let norm = plugin.manifest.normalize(&plugin.manifest.settings, settings).map_err(Error::Config)?;
        let server = ob.kind.server().map(|(h, p)| (h.to_string(), p));
        let dgram = match (&ob.stream.network, &server) {
            (rv_config::Network::Other(name), Some((h, p)))
                if super::for_transport(name).is_some_and(|t| t.manifest.carries == super::manifest::Carries::Datagram) =>
            {
                Some(Rc::new(Self::transport(profile, ob, name, h, *p, protect.clone())?))
            }
            _ => None,
        };
        let transport = match &server {
            Some((h, p))
                if dgram.is_none()
                    && (!matches!(ob.stream.network, rv_config::Network::Tcp)
                        || !matches!(ob.stream.security, rv_config::Security::None)) =>
            {
                Some(crate::transport::Stream::new(profile, ob, h, *p, protect.clone())?)
            }
            _ => None,
        };
        let s = json!({
            "protocol": protocol,
            "tag": ob.tag,
            "settings": norm,
            "global": plugin.grants().global,
            "server": server.as_ref().map(|(h, p)| json!({ "address": h, "port": p })),
            "transport": transport.is_some(),
        });
        Ok(Self {
            ctx: Rc::new(Ctx {
                perms: plugin.permissions(),
                user_hosts: plugin.manifest.hosts_in(&norm),
                plugin,
                settings: s.to_string().into_bytes(),
                server,
                profile: Some(profile.clone()),
                stream_settings: ob.stream.clone(),
                transport,
                dgram,
                protect,
            }),
            inst: RefCell::new(Weak::new()),
            failed: RefCell::new(None),
        })
    }

    pub fn transport(
        profile: &Profile,
        ob: &Outbound,
        name: &str,
        address: &str,
        port: u16,
        protect: Option<rv_net::Protect>,
    ) -> Result<Self, Error> {
        let plugin = super::for_transport(name).ok_or_else(|| {
            Error::Unsupported(format!(
                "network {name}: {}",
                crate::lang::t("нужен плагин этого транспорта", "needs a plugin for this transport")
            ))
        })?;
        let norm = plugin.manifest.normalize(&plugin.manifest.settings, &ob.stream.plugin_settings).map_err(Error::Config)?;
        let stream = plugin.manifest.carries == super::manifest::Carries::Stream;
        let mut inner = ob.clone();
        inner.stream.network = rv_config::Network::Tcp;
        let transport = if stream { Some(crate::transport::Stream::new(profile, &inner, address, port, protect.clone())?) } else { None };
        let s = json!({
            "protocol": name,
            "tag": ob.tag,
            "settings": norm,
            "global": plugin.grants().global,
            "server": { "address": address, "port": port },
            "transport": stream,
        });
        Ok(Self {
            ctx: Rc::new(Ctx {
                perms: plugin.permissions(),
                user_hosts: plugin.manifest.hosts_in(&norm),
                plugin,
                settings: s.to_string().into_bytes(),
                server: Some((address.to_string(), port)),
                profile: Some(profile.clone()),
                stream_settings: inner.stream,
                transport,
                dgram: None,
                protect,
            }),
            inst: RefCell::new(Weak::new()),
            failed: RefCell::new(None),
        })
    }

    pub async fn open_stream(&self) -> Result<BoxStream, Error> {
        let (h, p) = self.ctx.server.clone().ok_or_else(|| other("transport without a server"))?;
        let dest = match h.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(a)) => Address::Ipv4(a.octets()),
            Ok(std::net::IpAddr::V6(a)) => Address::Ipv6(a.octets()),
            Err(_) => Address::Domain(h),
        };
        self.open_conn(dest, p, conn::STREAM_TRANSPORT).await
    }

    pub(super) fn open_dgram(&self) -> Result<DgramLink, Error> {
        let (h, p) = self.ctx.server.clone().ok_or_else(|| other("transport without a server"))?;
        let inst = self.instance()?;
        let u = AppUdp::new(&inst.shared);
        let dest = match h.parse::<std::net::IpAddr>() {
            Ok(ip) => SocketAddr::new(ip, p),
            Err(_) => SocketAddr::from(([0, 0, 0, 0], p)),
        };
        *u.target.borrow_mut() = encode_target(
            &match h.parse::<std::net::IpAddr>() {
                Ok(std::net::IpAddr::V4(a)) => Address::Ipv4(a.octets()),
                Ok(std::net::IpAddr::V6(a)) => Address::Ipv6(a.octets()),
                Err(_) => Address::Domain(h),
            },
            p,
        );
        let id = inst.shared.table.borrow_mut().insert(Res::AppUdp(u.clone())).ok_or_else(|| other("plugin: too many sessions"))?;
        u.id.set(id);
        inst.shared.apps.set(inst.shared.apps.get() + 1);
        inst.shared.push(ev::OPEN, id, conn::DATAGRAM_TRANSPORT);
        self.ctx.plugin.diag.conns.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(DgramLink { shared: inst.shared.clone(), id, u, dest })
    }

    pub fn plugin(&self) -> &Arc<Plugin> {
        &self.ctx.plugin
    }

    fn instance(&self) -> Result<Rc<Instance>, Error> {
        if let Some(i) = self.inst.borrow().upgrade().filter(|i| !i.dead()) {
            return Ok(i);
        }
        let p = &self.ctx.plugin;
        if !p.enabled() {
            return Err(Error::Unsupported(format!(
                "{}: {}",
                p.manifest.name,
                crate::lang::t("плагин выключен", "the plugin is turned off")
            )));
        }
        if p.diag.resting() {
            return Err(other(format!(
                "{}: {}",
                p.manifest.name,
                crate::lang::t("плагин падал, пауза на минуту", "the plugin keeps crashing, paused for a minute")
            )));
        }
        if let Some((t, e)) = self.failed.borrow().as_ref()
            && t.elapsed() < Duration::from_secs(5)
        {
            return Err(other(e.clone()));
        }
        match Instance::start(self.ctx.clone(), true) {
            Ok(i) => {
                tokio::task::spawn_local(drive(i.clone()));
                *self.inst.borrow_mut() = Rc::downgrade(&i);
                self.failed.borrow_mut().take();
                Ok(i)
            }
            Err(e) => {
                let e = format!("{}: {e}", p.manifest.name);
                p.diag.error(&e);
                *self.failed.borrow_mut() = Some((std::time::Instant::now(), e.clone()));
                Err(other(e))
            }
        }
    }

    pub async fn connect(&self, dest: Address, port: u16) -> Result<BoxStream, Error> {
        if self.ctx.plugin.manifest.kind == Kind::Tunnel {
            return super::tun::connect(self, dest, port).await;
        }
        self.open_conn(dest, port, conn::TCP).await
    }

    async fn open_conn(&self, dest: Address, port: u16, kind: u32) -> Result<BoxStream, Error> {
        let inst = self.instance()?;
        let c = AppConn::new(&inst.shared, encode_target(&dest, port));
        let h = inst.shared.table.borrow_mut().insert(Res::App(c.clone())).ok_or_else(|| other("plugin: too many connections"))?;
        c.id.set(h);
        inst.shared.apps.set(inst.shared.apps.get() + 1);
        let stream = AppStream { conn: c.clone() };
        inst.shared.push(ev::OPEN, h, kind);
        self.ctx.plugin.diag.conns.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match tokio::time::timeout(READY_TIMEOUT, c.wait_ready()).await {
            Ok(Ok(())) => Ok(Box::new(stream)),
            Ok(Err(e)) => Err(Error::Io(io::Error::new(io::ErrorKind::ConnectionRefused, e))),
            Err(_) => Err(Error::Io(io::Error::new(io::ErrorKind::TimedOut, "plugin did not connect in 20 s"))),
        }
    }

    pub fn udp_allowed(&self) -> bool {
        self.ctx.plugin.manifest.udp
    }

    pub fn reset(&self) {
        if let Some(i) = self.inst.borrow().upgrade() {
            i.kill("network changed");
        }
        if let Some(t) = &self.ctx.transport {
            t.reset();
        }
        if let Some(d) = &self.ctx.dgram {
            d.reset();
        }
    }

    pub(super) fn instance_for_tun(&self) -> Result<Rc<Instance>, Error> {
        self.instance()
    }

    pub async fn udp_session(
        &self,
        first: (SocketAddr, Vec<u8>),
        rx: &mut mpsc::Receiver<(SocketAddr, Vec<u8>)>,
        reply: &dyn Fn(SocketAddr, &[u8]),
    ) -> Result<(), Error> {
        if self.ctx.plugin.manifest.kind == Kind::Tunnel {
            return super::tun::udp_session(self, first, rx, reply).await;
        }
        let inst = self.instance()?;
        let u = AppUdp::new(&inst.shared);
        let h = inst.shared.table.borrow_mut().insert(Res::AppUdp(u.clone())).ok_or_else(|| other("plugin: too many sessions"))?;
        u.id.set(h);
        struct Session(Rc<super::res::Shared>, u32);
        impl Drop for Session {
            fn drop(&mut self) {
                self.0.apps.set(self.0.apps.get().saturating_sub(1));
                if self.0.table.borrow_mut().remove(self.1).is_some() {
                    self.0.push(ev::CLOSED, self.1, 0);
                }
            }
        }
        inst.shared.apps.set(inst.shared.apps.get() + 1);
        let _session = Session(inst.shared.clone(), h);
        inst.shared.push(ev::OPEN, h, conn::UDP);
        u.deliver(first.0, &first.1);
        drop(inst);
        let idle = tokio::time::sleep(crate::UDP_IDLE);
        tokio::pin!(idle);
        let mut last = tokio::time::Instant::now();
        loop {
            tokio::select! {
                p = rx.recv() => {
                    let Some((dst, data)) = p else { return Ok(()) };
                    u.deliver(dst, &data);
                    crate::udp_touch(&mut last, idle.as_mut(), crate::UDP_IDLE);
                }
                _ = u.notify.notified() => {
                    loop {
                        let next = u.tx.borrow_mut().pop_front();
                        let Some((src, d)) = next else { break };
                        reply(src, &d);
                    }
                    if let Some(e) = u.closed.borrow().clone() {
                        return Err(other(e));
                    }
                    crate::udp_touch(&mut last, idle.as_mut(), crate::UDP_IDLE);
                }
                _ = &mut idle => return Ok(()),
            }
        }
    }
}

pub(super) struct DgramLink {
    shared: Rc<super::res::Shared>,
    id: u32,
    pub u: Rc<AppUdp>,
    dest: SocketAddr,
}

impl DgramLink {
    pub fn send(&self, data: &[u8]) {
        self.u.deliver(self.dest, data);
    }
}

impl Drop for DgramLink {
    fn drop(&mut self) {
        self.shared.apps.set(self.shared.apps.get().saturating_sub(1));
        if self.shared.table.borrow_mut().remove(self.id).is_some() {
            self.shared.push(ev::CLOSED, self.id, 0);
        }
    }
}

pub fn parse_link(line: &str) -> Option<(String, Value)> {
    let (scheme, _) = line.split_once("://")?;
    let p = super::for_scheme(scheme)?;
    let r = (|| {
        let inst = Instance::start(Rc::new(Ctx::offline(p.clone())), false)?;
        let out = inst.call("rn_parse_link", line.as_bytes().to_vec())?.ok_or("no result")?;
        let v: Value = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
        let mut ob = v.get("outbound").filter(|o| o.is_object()).cloned().ok_or("no outbound")?;
        let proto = ob.get("protocol").and_then(Value::as_str).unwrap_or_default().to_string();
        if !p.manifest.protocols.contains(&proto) {
            return Err(format!("returned protocol `{proto}`, not its own"));
        }
        ob["tag"] = json!("proxy");
        let name = v.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()).map(String::from);
        let name = name.or_else(|| {
            rv_config::OutboundKind::Plugin { protocol: proto.clone(), settings: ob["settings"].clone() }
                .server()
                .map(|(h, p)| format!("{h}:{p}"))
        });
        Ok::<_, String>((name.unwrap_or_else(|| p.manifest.name.clone()), ob))
    })();
    match r {
        Ok(x) => Some(x),
        Err(e) => {
            p.diag.error(&format!("link: {e}"));
            None
        }
    }
}
