pub mod core;
pub mod cover;
pub mod fetch;
pub mod geodat;
pub mod logfile;
#[cfg(unix)]
pub mod owner;
#[cfg(windows)]
#[path = "owner_windows.rs"]
pub mod owner;
pub mod proxy;
pub mod store;
pub mod tray;
#[cfg(target_os = "linux")]
pub mod tun;
#[cfg(target_os = "macos")]
#[path = "tun_macos.rs"]
pub mod tun;
#[cfg(windows)]
#[path = "tun_windows.rs"]
pub mod tun;
#[cfg(any(windows, test))]
pub mod tun_setup;

use std::sync::OnceLock;

use serde_json::Value;

static EMIT: OnceLock<fn(Value)> = OnceLock::new();

pub fn set_emit(f: fn(Value)) {
    let _ = EMIT.set(f);
}

pub fn emit(v: Value) {
    match EMIT.get() {
        Some(f) => f(v),
        None => println!("{v}"),
    }
}
