use std::ops::Range;

use rv_plugin_abi::{err, ev, log as lvl};
use wasmi::{Caller, Engine, Linker};

use super::crypto;
use super::instance::Host;
use super::res::{AppConn, Res, Task};

pub(super) const NAMES: &[&str] = &[
    "log",
    "now_ms",
    "unix_ms",
    "random",
    "last_error",
    "set_error",
    "settings",
    "arg",
    "ret",
    "timer",
    "open",
    "status",
    "info",
    "read",
    "write",
    "shutdown",
    "close",
    "recv",
    "send",
    "accept",
    "reject",
    "splice",
    "target",
    "export_keying",
    "hash",
    "mac",
    "hkdf",
    "seal",
    "unseal",
    "cipher",
    "x25519",
    "verify",
    "kv_get",
    "kv_set",
];

const MAX_COPY: u32 = 16 << 20;

fn range(len: usize, ptr: u32, n: u32) -> Option<Range<usize>> {
    let (p, n) = (ptr as usize, n as usize);
    (n <= MAX_COPY as usize && p.checked_add(n)? <= len).then_some(p..p + n)
}

fn mem<'a>(c: &'a mut Caller<'_, Host>) -> (&'a mut [u8], &'a mut Host) {
    let m = c.data().memory.expect("memory is set before any call");
    m.data_and_store_mut(c)
}

fn input(c: &mut Caller<'_, Host>, ptr: u32, len: u32) -> Result<Vec<u8>, i32> {
    let (m, _) = mem(c);
    let r = range(m.len(), ptr, len).ok_or(err::INVALID)?;
    Ok(m[r].to_vec())
}

fn fail(c: &mut Caller<'_, Host>, code: i32, msg: impl Into<String>) -> i32 {
    c.data_mut().error = msg.into();
    code
}

fn give(c: &mut Caller<'_, Host>, ptr: u32, cap: u32, data: &[u8]) -> i32 {
    let Ok(n) = i32::try_from(data.len()) else { return err::LIMIT };
    if data.len() > cap as usize {
        return n;
    }
    let (m, _) = mem(c);
    match range(m.len(), ptr, data.len() as u32) {
        Some(r) => {
            m[r].copy_from_slice(data);
            n
        }
        None => err::INVALID,
    }
}

fn get(c: &Caller<'_, Host>, h: u32) -> Option<Res> {
    c.data().shared.get(h)
}

fn result(c: &mut Caller<'_, Host>, r: Result<usize, (i32, String)>) -> i32 {
    match r {
        Ok(n) => n as i32,
        Err((code, msg)) => fail(c, code, msg),
    }
}

fn with_buf(c: &mut Caller<'_, Host>, ptr: u32, len: u32, f: impl FnOnce(&mut [u8]) -> Result<usize, (i32, String)>) -> i32 {
    let r = {
        let (m, _) = mem(c);
        match range(m.len(), ptr, len) {
            Some(r) => f(&mut m[r]),
            None => Err((err::INVALID, "buffer outside memory".into())),
        }
    };
    result(c, r)
}

fn crypto_out(c: &mut Caller<'_, Host>, out: u32, cap: u32, f: impl FnOnce(&mut [u8]) -> Result<usize, i32>) -> i32 {
    let (m, _) = mem(c);
    let Some(r) = range(m.len(), out, cap) else { return err::INVALID };
    match f(&mut m[r]) {
        Ok(n) => n as i32,
        Err(e) => e,
    }
}

pub(super) fn linker(engine: &Engine) -> Result<Linker<Host>, wasmi::Error> {
    let mut l = Linker::<Host>::new(engine);
    let m = rv_plugin_abi::IMPORT_MODULE;

    l.func_wrap(m, "log", |mut c: Caller<'_, Host>, level: u32, ptr: u32, len: u32| {
        let Ok(b) = input(&mut c, ptr, len.min(4096)) else { return };
        let text = String::from_utf8_lossy(&b);
        let p = &c.data().ctx.plugin;
        match level {
            lvl::ERROR => p.diag.error(&text),
            l => p.diag.log(l.clamp(lvl::ERROR, lvl::DEBUG), &text),
        }
        tracing::debug!(target: "rv::plugin", "{}: {text}", p.manifest.id);
    })?;
    l.func_wrap(m, "now_ms", |_: Caller<'_, Host>| -> i64 {
        static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as i64
    })?;
    l.func_wrap(m, "unix_ms", |_: Caller<'_, Host>| -> i64 { super::unix_ms() as i64 })?;
    l.func_wrap(m, "random", |mut c: Caller<'_, Host>, ptr: u32, len: u32| -> i32 {
        use ring::rand::SecureRandom;
        with_buf(&mut c, ptr, len.min(65536), |b| {
            ring::rand::SystemRandom::new().fill(b).map_err(|_| (err::FAILED, "no randomness".into()))?;
            Ok(b.len())
        })
    })?;
    l.func_wrap(m, "last_error", |mut c: Caller<'_, Host>, ptr: u32, cap: u32| -> i32 {
        let e = c.data().error.clone();
        give(&mut c, ptr, cap, e.as_bytes())
    })?;
    l.func_wrap(m, "set_error", |mut c: Caller<'_, Host>, ptr: u32, len: u32| -> i32 {
        match input(&mut c, ptr, len.min(4096)) {
            Ok(b) => {
                c.data_mut().error = String::from_utf8_lossy(&b).into_owned();
                0
            }
            Err(e) => e,
        }
    })?;
    l.func_wrap(m, "settings", |mut c: Caller<'_, Host>, ptr: u32, cap: u32| -> i32 {
        let s = c.data().ctx.settings.clone();
        give(&mut c, ptr, cap, &s)
    })?;
    l.func_wrap(m, "arg", |mut c: Caller<'_, Host>, ptr: u32, cap: u32| -> i32 {
        let a = c.data().arg.clone();
        give(&mut c, ptr, cap, &a)
    })?;
    l.func_wrap(m, "ret", |mut c: Caller<'_, Host>, ptr: u32, len: u32| -> i32 {
        match input(&mut c, ptr, len.min(1 << 20)) {
            Ok(b) => {
                c.data_mut().ret = Some(b);
                0
            }
            Err(e) => e,
        }
    })?;
    l.func_wrap(m, "timer", |mut c: Caller<'_, Host>, ms: u32| -> i32 {
        if !c.data().ctx.online() {
            return fail(&mut c, err::DENIED, "no timers here");
        }
        let shared = c.data().shared.clone();
        let ms = u64::from(ms.clamp(1, 86_400_000));
        let slot = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let (weak, id) = (std::rc::Rc::downgrade(&shared), slot.clone());
        let t = tokio::task::spawn_local(async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            if let Some(s) = weak.upgrade() {
                let h = id.get();
                s.table.borrow_mut().remove(h);
                s.push(ev::TIMER, h, 0);
            }
        });
        match shared.table.borrow_mut().insert(Res::Timer(std::rc::Rc::new(Task(t.abort_handle())))) {
            Some(h) => {
                slot.set(h);
                h as i32
            }
            None => {
                t.abort();
                err::LIMIT
            }
        }
    })?;
    l.func_wrap(m, "open", |mut c: Caller<'_, Host>, kind: u32, ptr: u32, len: u32| -> i32 {
        let spec = match input(&mut c, ptr, len.min(64 * 1024)) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let (ctx, shared) = (c.data().ctx.clone(), c.data().shared.clone());
        match super::net::open(&ctx, &shared, kind, &spec) {
            Ok(h) => h as i32,
            Err((code, msg)) => fail(&mut c, code, msg),
        }
    })?;
    l.func_wrap(m, "status", |mut c: Caller<'_, Host>, h: u32| -> i32 {
        match get(&c, h) {
            Some(r) => match r.op() {
                Some(op) => {
                    let s = op.status();
                    if s < 0 && s != err::PENDING {
                        let e = op.error();
                        c.data_mut().error = e;
                    }
                    s
                }
                None => 0,
            },
            None => fail(&mut c, err::INVALID, "unknown handle"),
        }
    })?;
    l.func_wrap(m, "info", |mut c: Caller<'_, Host>, h: u32, ptr: u32, cap: u32| -> i32 {
        let Some(r) = get(&c, h) else { return fail(&mut c, err::INVALID, "unknown handle") };
        let data = match &r {
            Res::App(a) => Some(a.target.clone()),
            _ => r.op().and_then(|o| o.info.borrow().clone()),
        };
        match data {
            Some(d) => give(&mut c, ptr, cap, &d),
            None => err::AGAIN,
        }
    })?;
    l.func_wrap(m, "target", |mut c: Caller<'_, Host>, h: u32, ptr: u32, cap: u32| -> i32 {
        match get(&c, h) {
            Some(Res::App(a)) => give(&mut c, ptr, cap, &a.target),
            Some(Res::AppUdp(u)) if !u.target.borrow().is_empty() => {
                let t = u.target.borrow().clone();
                give(&mut c, ptr, cap, &t)
            }
            _ => fail(&mut c, err::INVALID, "not a connection handle"),
        }
    })?;
    l.func_wrap(m, "read", |mut c: Caller<'_, Host>, h: u32, ptr: u32, cap: u32| -> i32 {
        match get(&c, h) {
            Some(Res::Stream(p)) => with_buf(&mut c, ptr, cap, |b| p.read(b)),
            Some(Res::App(a)) => with_buf(&mut c, ptr, cap, |b| a.read(b)),
            Some(_) => fail(&mut c, err::INVALID, "not a stream handle (use recv for datagrams)"),
            None => fail(&mut c, err::INVALID, "unknown handle"),
        }
    })?;
    l.func_wrap(m, "write", |mut c: Caller<'_, Host>, h: u32, ptr: u32, len: u32| -> i32 {
        match get(&c, h) {
            Some(Res::Stream(p)) => with_buf(&mut c, ptr, len, |b| p.write(b)),
            Some(Res::App(a)) => with_buf(&mut c, ptr, len, |b| a.write(b)),
            Some(_) => fail(&mut c, err::INVALID, "not a stream handle (use send for datagrams)"),
            None => fail(&mut c, err::INVALID, "unknown handle"),
        }
    })?;
    l.func_wrap(m, "recv", |mut c: Caller<'_, Host>, h: u32, ptr: u32, cap: u32| -> i32 {
        match get(&c, h) {
            Some(Res::Dgram(d)) => with_buf(&mut c, ptr, cap, |b| d.recv(b)),
            Some(Res::Quic(q)) => with_buf(&mut c, ptr, cap, |b| q.dg.recv(b)),
            Some(Res::AppUdp(u)) => with_buf(&mut c, ptr, cap, |b| u.recv(b)),
            Some(Res::Tun(t)) => with_buf(&mut c, ptr, cap, |b| t.recv(b)),
            Some(_) => fail(&mut c, err::INVALID, "not a datagram handle"),
            None => fail(&mut c, err::INVALID, "unknown handle"),
        }
    })?;
    l.func_wrap(m, "send", |mut c: Caller<'_, Host>, h: u32, ptr: u32, len: u32| -> i32 {
        match get(&c, h) {
            Some(Res::Dgram(d)) => with_buf(&mut c, ptr, len, |b| d.send(b)),
            Some(Res::Quic(q)) => with_buf(&mut c, ptr, len, |b| q.dg.send(b)),
            Some(Res::AppUdp(u)) => with_buf(&mut c, ptr, len, |b| u.send(b)),
            Some(Res::Tun(t)) => with_buf(&mut c, ptr, len, |b| t.send(b)),
            Some(_) => fail(&mut c, err::INVALID, "not a datagram handle"),
            None => fail(&mut c, err::INVALID, "unknown handle"),
        }
    })?;
    l.func_wrap(m, "shutdown", |mut c: Caller<'_, Host>, h: u32| -> i32 {
        match get(&c, h) {
            Some(Res::Stream(p)) => {
                p.shutdown();
                0
            }
            Some(Res::App(a)) => {
                a.shutdown();
                0
            }
            Some(_) => fail(&mut c, err::INVALID, "not a stream handle"),
            None => fail(&mut c, err::INVALID, "unknown handle"),
        }
    })?;
    l.func_wrap(m, "close", |mut c: Caller<'_, Host>, h: u32| -> i32 {
        let r = c.data().shared.table.borrow_mut().remove(h);
        match r {
            Some(r) => {
                r.close();
                0
            }
            None => fail(&mut c, err::INVALID, "unknown handle"),
        }
    })?;
    l.func_wrap(m, "accept", |mut c: Caller<'_, Host>, h: u32| -> i32 {
        match get(&c, h) {
            Some(Res::App(a)) => {
                a.accept();
                0
            }
            Some(Res::AppUdp(_)) => 0,
            _ => fail(&mut c, err::INVALID, "not a connection handle"),
        }
    })?;
    l.func_wrap(m, "reject", |mut c: Caller<'_, Host>, h: u32, ptr: u32, len: u32| -> i32 {
        let why = input(&mut c, ptr, len.min(1024)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let why = if why.is_empty() { "rejected by the plugin".to_string() } else { why };
        match get(&c, h) {
            Some(Res::App(a)) => {
                c.data().shared.table.borrow_mut().remove(h);
                c.data().ctx.plugin.diag.failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                a.fail(&why);
                0
            }
            Some(Res::AppUdp(u)) => {
                c.data().shared.table.borrow_mut().remove(h);
                u.close(&why);
                0
            }
            Some(_) => fail(&mut c, err::INVALID, "not a connection handle"),
            None => fail(&mut c, err::INVALID, "unknown handle"),
        }
    })?;
    l.func_wrap(m, "splice", |mut c: Caller<'_, Host>, conn: u32, stream: u32| -> i32 {
        let shared = c.data().shared.clone();
        let (Some(Res::App(a)), Some(Res::Stream(p))) = (shared.get(conn), shared.get(stream)) else {
            return fail(&mut c, err::INVALID, "splice(connection, stream)");
        };
        match p.detach() {
            Ok(l) => {
                let mut t = shared.table.borrow_mut();
                t.remove(conn);
                t.remove(stream);
                drop(t);
                AppConn::splice(&a, l);
                0
            }
            Err((code, msg)) => fail(&mut c, code, msg),
        }
    })?;
    l.func_wrap(m, "export_keying", |mut c: Caller<'_, Host>, h: u32, lp: u32, ll: u32, cp: u32, cl: u32, out: u32, olen: u32| -> i32 {
        let (Ok(label), Ok(context)) = (input(&mut c, lp, ll.min(255)), input(&mut c, cp, cl.min(65535))) else { return err::INVALID };
        let Some(Res::Quic(q)) = get(&c, h) else { return fail(&mut c, err::UNSUPPORTED, "keying material export needs a QUIC handle") };
        with_buf(&mut c, out, olen.min(1024), |b| q.export(&label, &context, b).map(|_| b.len()))
    })?;
    l.func_wrap(m, "hash", |mut c: Caller<'_, Host>, alg: u32, ptr: u32, len: u32, out: u32, cap: u32| -> i32 {
        let Ok(d) = input(&mut c, ptr, len) else { return err::INVALID };
        crypto_out(&mut c, out, cap.min(64), |o| crypto::digest(alg, &d, o))
    })?;
    l.func_wrap(m, "mac", |mut c: Caller<'_, Host>, alg: u32, kp: u32, kl: u32, ptr: u32, len: u32, out: u32, cap: u32| -> i32 {
        let (Ok(k), Ok(d)) = (input(&mut c, kp, kl.min(1024)), input(&mut c, ptr, len)) else { return err::INVALID };
        crypto_out(&mut c, out, cap.min(64), |o| crypto::mac(alg, &k, &d, o))
    })?;
    l.func_wrap(
        m,
        "hkdf",
        |mut c: Caller<'_, Host>, alg: u32, sp: u32, sl: u32, ip: u32, il: u32, fp: u32, fl: u32, out: u32, olen: u32| -> i32 {
            let (Ok(salt), Ok(ikm), Ok(info)) =
                (input(&mut c, sp, sl.min(1024)), input(&mut c, ip, il.min(4096)), input(&mut c, fp, fl.min(4096)))
            else {
                return err::INVALID;
            };
            crypto_out(&mut c, out, olen.min(255 * 32), |o| crypto::hkdf(alg, &salt, &ikm, &info, o))
        },
    )?;
    l.func_wrap(
        m,
        "seal",
        |mut c: Caller<'_, Host>, alg: u32, kp: u32, kl: u32, np: u32, nl: u32, ap: u32, al: u32, buf: u32, len: u32, cap: u32| -> i32 {
            let (Ok(k), Ok(n), Ok(a)) = (input(&mut c, kp, kl.min(64)), input(&mut c, np, nl.min(64)), input(&mut c, ap, al.min(65536)))
            else {
                return err::INVALID;
            };
            if len > cap {
                return err::INVALID;
            }
            crypto_out(&mut c, buf, cap, |b| crypto::seal(alg, &k, &n, &a, b, len as usize))
        },
    )?;
    l.func_wrap(
        m,
        "unseal",
        |mut c: Caller<'_, Host>, alg: u32, kp: u32, kl: u32, np: u32, nl: u32, ap: u32, al: u32, buf: u32, len: u32| -> i32 {
            let (Ok(k), Ok(n), Ok(a)) = (input(&mut c, kp, kl.min(64)), input(&mut c, np, nl.min(64)), input(&mut c, ap, al.min(65536)))
            else {
                return err::INVALID;
            };
            crypto_out(&mut c, buf, len, |b| crypto::open(alg, &k, &n, &a, b))
        },
    )?;
    l.func_wrap(m, "cipher", |mut c: Caller<'_, Host>, alg: u32, kp: u32, kl: u32, ip: u32, il: u32, buf: u32, len: u32| -> i32 {
        let (Ok(k), Ok(iv)) = (input(&mut c, kp, kl.min(64)), input(&mut c, ip, il.min(64))) else { return err::INVALID };
        crypto_out(&mut c, buf, len, |b| crypto::apply(alg, &k, &iv, b))
    })?;
    l.func_wrap(m, "x25519", |mut c: Caller<'_, Host>, sp: u32, pp: u32, pl: u32, out: u32| -> i32 {
        let Ok(s) = input(&mut c, sp, 32) else { return err::INVALID };
        let p = if pl == 0 {
            None
        } else {
            match input(&mut c, pp, pl.min(32)) {
                Ok(p) => Some(p),
                Err(e) => return e,
            }
        };
        crypto_out(&mut c, out, 32, |o| crypto::x25519(&s, p.as_deref(), o))
    })?;
    l.func_wrap(m, "verify", |mut c: Caller<'_, Host>, alg: u32, kp: u32, kl: u32, mp: u32, ml: u32, sp: u32, sl: u32| -> i32 {
        let (Ok(k), Ok(msg), Ok(s)) = (input(&mut c, kp, kl.min(4096)), input(&mut c, mp, ml), input(&mut c, sp, sl.min(1024))) else {
            return err::INVALID;
        };
        match crypto::verify(alg, &k, &msg, &s) {
            Ok(_) => 0,
            Err(e) => e,
        }
    })?;
    l.func_wrap(m, "kv_get", |mut c: Caller<'_, Host>, kp: u32, kl: u32, out: u32, cap: u32| -> i32 {
        if !c.data().ctx.perms.storage {
            return fail(&mut c, err::DENIED, "no storage permission");
        }
        let Ok(k) = input(&mut c, kp, kl.min(256)) else { return err::INVALID };
        let v = c.data().ctx.plugin.kv_get(&String::from_utf8_lossy(&k));
        match v {
            Some(v) => give(&mut c, out, cap, &v),
            None => fail(&mut c, err::CLOSED, "no such key"),
        }
    })?;
    l.func_wrap(m, "kv_set", |mut c: Caller<'_, Host>, kp: u32, kl: u32, vp: u32, vl: u32| -> i32 {
        if !c.data().ctx.perms.storage {
            return fail(&mut c, err::DENIED, "no storage permission");
        }
        let Ok(k) = input(&mut c, kp, kl.min(256)) else { return err::INVALID };
        let v = if vl == u32::MAX {
            None
        } else {
            match input(&mut c, vp, vl.min(256 * 1024)) {
                Ok(v) => Some(v),
                Err(e) => return e,
            }
        };
        let r = c.data().ctx.plugin.kv_set(&String::from_utf8_lossy(&k), v.as_deref());
        match r {
            Ok(()) => 0,
            Err(e) => fail(&mut c, err::LIMIT, e),
        }
    })?;
    Ok(l)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ranges_stay_inside_memory() {
        assert_eq!(super::range(100, 10, 20), Some(10..30));
        assert_eq!(super::range(100, 90, 10), Some(90..100));
        assert_eq!(super::range(100, 90, 11), None);
        assert_eq!(super::range(100, u32::MAX, 2), None);
        assert_eq!(super::range(usize::MAX, 0, u32::MAX), None, "copies are bounded");
    }
}
