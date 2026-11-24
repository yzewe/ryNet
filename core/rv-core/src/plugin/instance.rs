use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use rv_plugin_abi::ev;
use wasmi::{Memory, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};

use super::Ctx;
use super::res::{Res, Shared};

pub(super) const EVENT_FUEL: u64 = 50_000_000;
const INIT_FUEL: u64 = 300_000_000;
pub(super) const IDLE: Duration = Duration::from_secs(60);

pub(super) struct Host {
    pub ctx: Rc<Ctx>,
    pub shared: Rc<Shared>,
    limits: StoreLimits,
    pub memory: Option<Memory>,
    pub error: String,
    pub arg: Vec<u8>,
    pub ret: Option<Vec<u8>>,
    pub calls: u64,
}

pub(super) struct Instance {
    pub ctx: Rc<Ctx>,
    pub shared: Rc<Shared>,
    store: RefCell<Store<Host>>,
    event: Option<TypedFunc<(u32, u32, u32), i32>>,
    instance: wasmi::Instance,
    _keep: Option<rv_stats::KeepAlive>,
}

fn trap_text(e: &wasmi::Error) -> String {
    match e.as_trap_code() {
        Some(wasmi::TrapCode::OutOfFuel) => "ran out of fuel (stuck in a loop?)".into(),
        Some(c) => format!("trap: {c}"),
        None => e.to_string(),
    }
}

impl Instance {
    pub fn start(ctx: Rc<Ctx>, events: bool) -> Result<Rc<Self>, String> {
        let p = &ctx.plugin;
        let shared = Shared::new();
        let limits = StoreLimitsBuilder::new()
            .memory_size(ctx.perms.memory_mb as usize * 1024 * 1024)
            .instances(1)
            .memories(1)
            .tables(4)
            .table_elements(65536)
            .build();
        let host = Host {
            ctx: ctx.clone(),
            shared: shared.clone(),
            limits,
            memory: None,
            error: String::new(),
            arg: Vec::new(),
            ret: None,
            calls: 0,
        };
        let mut store = Store::new(super::engine(), host);
        store.limiter(|h| &mut h.limits);
        store.call_hook(|h, hook| {
            if matches!(hook, wasmi::CallHook::CallingHost) {
                h.calls += 1;
            }
            Ok(())
        });
        store.set_fuel(INIT_FUEL).map_err(|e| e.to_string())?;
        let linker = super::host::linker(super::engine()).map_err(|e| e.to_string())?;
        let instance = linker.instantiate_and_start(&mut store, &p.module).map_err(|e| format!("start: {}", trap_text(&e)))?;
        let memory = instance.get_memory(&store, "memory").ok_or("no memory export")?;
        store.data_mut().memory = Some(memory);
        let event = if events {
            Some(instance.get_typed_func::<(u32, u32, u32), i32>(&store, "rn_event").map_err(|e| format!("rn_event: {e}"))?)
        } else {
            None
        };
        if events {
            let init = instance.get_typed_func::<(), i32>(&store, "rn_init").map_err(|e| format!("rn_init: {e}"))?;
            let r = init.call(&mut store, ()).map_err(|e| trap_text(&e))?;
            if r < 0 {
                let e = std::mem::take(&mut store.data_mut().error);
                return Err(if e.is_empty() { format!("init failed: {}", rv_plugin_abi::err_name(r)) } else { e });
            }
        }
        p.diag.starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let keep = (events && ctx.perms.keepalive).then(rv_stats::KeepAlive::new);
        Ok(Rc::new(Self { ctx, shared, store: RefCell::new(store), event, instance, _keep: keep }))
    }

    pub fn call(&self, name: &str, arg: Vec<u8>) -> Result<Option<Vec<u8>>, String> {
        let mut store = self.store.borrow_mut();
        let f = self.instance.get_typed_func::<(), i32>(&*store, name).map_err(|e| format!("{name}: {e}"))?;
        store.data_mut().arg = arg;
        store.data_mut().ret = None;
        store.set_fuel(INIT_FUEL).map_err(|e| e.to_string())?;
        let r = f.call(&mut *store, ()).map_err(|e| trap_text(&e))?;
        if r < 0 {
            let e = std::mem::take(&mut store.data_mut().error);
            return Err(if e.is_empty() { rv_plugin_abi::err_name(r).to_string() } else { e });
        }
        Ok(store.data_mut().ret.take())
    }

    fn dispatch(&self, kind: u32, a: u32, b: u32) -> Result<(), String> {
        let Some(f) = &self.event else { return Ok(()) };
        let mut store = self.store.borrow_mut();
        store.set_fuel(EVENT_FUEL).map_err(|e| e.to_string())?;
        store.data_mut().calls = 0;
        f.call(&mut *store, (kind, a, b)).map(|_| ()).map_err(|e| {
            let n = store.data().calls;
            format!("{} (event {kind} on {a:#x}, {n} host calls)", trap_text(&e))
        })
    }

    pub fn kill(&self, why: &str) {
        if self.shared.dead.replace(true) {
            return;
        }
        let all = self.shared.table.borrow_mut().drain();
        for r in all {
            match &r {
                Res::App(c) => c.fail(why),
                Res::AppUdp(u) => u.close(why),
                _ => r.close(),
            }
        }
        self.shared.wake.notify_one();
        self.shared.tun_up.notify_waiters();
    }

    pub fn dead(&self) -> bool {
        self.shared.dead.get()
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.kill("plugin stopped");
    }
}

pub(super) async fn drive(inst: Rc<Instance>) {
    let diag = &inst.ctx.plugin.diag;
    loop {
        while let Some((k, a, b)) = inst.shared.pop() {
            if inst.dead() {
                return;
            }
            if let Err(e) = inst.dispatch(k, a, b) {
                diag.trapped(&format!("crashed: {e}"));
                tracing::warn!(target: "rv::plugin", "{}: {e}", inst.ctx.plugin.manifest.id);
                inst.kill(&format!("plugin {} crashed: {e}", inst.ctx.plugin.manifest.name));
                return;
            }
        }
        if inst.dead() {
            return;
        }
        let idle = inst.shared.apps.get() == 0 && !inst.ctx.perms.keepalive;
        tokio::select! {
            _ = inst.shared.wake.notified() => {}
            _ = tokio::time::sleep(IDLE), if idle => {
                if inst.shared.apps.get() == 0 {
                    let _ = inst.dispatch(ev::STOP, 0, 0);
                    inst.kill("plugin idle");
                    return;
                }
            }
        }
    }
}
