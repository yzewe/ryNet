use std::io::BufRead;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::emit;

#[derive(Clone, Default)]
pub struct Icon {
    pub w: i32,
    pub h: i32,
    pub argb: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct State {
    pub on: bool,
    pub busy: bool,
    pub title: String,
    pub server: String,
    pub servers: Vec<String>,
    pub current: String,
    pub icons_on: Vec<Icon>,
    pub icons_off: Vec<Icon>,
}

impl State {
    fn icons(&self) -> &[Icon] {
        if self.on { &self.icons_on } else { &self.icons_off }
    }
    #[cfg(windows)]
    fn tooltip(&self) -> String {
        if self.server.is_empty() { format!("ryNet — {}", self.title) } else { format!("ryNet — {}\n{}", self.title, self.server) }
    }
}

fn icons(v: Option<&Value>) -> Vec<Icon> {
    use base64::Engine;
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    let w = i.get("w")?.as_i64()? as i32;
                    let h = i.get("h")?.as_i64()? as i32;
                    let argb = base64::engine::general_purpose::STANDARD.decode(i.get("argb")?.as_str()?).ok()?;
                    (argb.len() == (w * h * 4) as usize).then_some(Icon { w, h, argb })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn apply(state: &mut State, v: &Value) -> bool {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match v.get("cmd").and_then(Value::as_str) {
        Some("icons") => {
            state.icons_on = icons(v.get("on"));
            state.icons_off = icons(v.get("off"));
            true
        }
        Some("state") => {
            state.on = v.get("on").and_then(Value::as_bool).unwrap_or(false);
            state.busy = v.get("busy").and_then(Value::as_bool).unwrap_or(false);
            state.title = s("title");
            state.server = s("server");
            state.current = s("current");
            state.servers = v
                .get("servers")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            true
        }
        _ => false,
    }
}

pub fn run() {
    let state =
        Arc::new(Mutex::new(State { title: rv_core::lang::t("Не подключено", "Not connected").into(), ..State::default() }));
    imp::run(state);
}

fn read_commands(state: &Arc<Mutex<State>>, mut changed: impl FnMut()) {
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let did = apply(&mut state.lock().unwrap_or_else(|e| e.into_inner()), &v);
        if did {
            changed();
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use ksni::menu::{MenuItem, StandardItem, SubMenu};

    struct Tray(Arc<Mutex<State>>);

    impl Tray {
        fn st(&self) -> State {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "rynet".into()
        }
        fn title(&self) -> String {
            "ryNet".into()
        }
        fn category(&self) -> ksni::Category {
            ksni::Category::Communications
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            emit(json!({ "ev": "open" }));
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.st().icons().iter().map(|i| ksni::Icon { width: i.w, height: i.h, data: i.argb.clone() }).collect()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            let s = self.st();
            ksni::ToolTip { title: format!("ryNet — {}", s.title), description: s.server.clone(), ..Default::default() }
        }
        fn menu(&self) -> Vec<MenuItem<Self>> {
            let s = self.st();
            let mut items: Vec<MenuItem<Self>> = vec![
                StandardItem {
                    label: if s.server.is_empty() { s.title.clone() } else { format!("{} · {}", s.title, s.server) },
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: if s.on || s.busy {
                        rv_core::lang::t("Отключить", "Disconnect").into()
                    } else {
                        rv_core::lang::t("Подключить", "Connect").into()
                    },
                    activate: Box::new(|_| emit(json!({ "ev": "toggle" }))),
                    ..Default::default()
                }
                .into(),
            ];
            if !s.servers.is_empty() {
                let submenu = s
                    .servers
                    .iter()
                    .map(|name| {
                        let n = name.clone();
                        ksni::menu::CheckmarkItem {
                            label: name.replace('_', "__"),
                            checked: *name == s.current,
                            activate: Box::new(move |_| emit(json!({ "ev": "server", "name": n }))),
                            ..Default::default()
                        }
                        .into()
                    })
                    .collect();
                items.push(SubMenu { label: rv_core::lang::t("Серверы", "Servers").into(), submenu, ..Default::default() }.into());
            }
            items.push(MenuItem::Separator);
            items.push(
                StandardItem {
                    label: rv_core::lang::t("Открыть ryNet", "Open ryNet").into(),
                    activate: Box::new(|_| emit(json!({ "ev": "open" }))),
                    ..Default::default()
                }
                .into(),
            );
            items.push(
                StandardItem {
                    label: rv_core::lang::t("Выход", "Quit").into(),
                    activate: Box::new(|_| emit(json!({ "ev": "quit" }))),
                    ..Default::default()
                }
                .into(),
            );
            items
        }
    }

    pub fn run(state: Arc<Mutex<State>>) {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => return emit(json!({ "ev": "ready", "ok": false, "error": e.to_string() })),
        };
        let handle = rt.block_on(async { ksni::TrayMethods::spawn(Tray(state.clone())).await });
        let handle = match handle {
            Ok(h) => h,
            Err(e) => return emit(json!({ "ev": "ready", "ok": false, "error": e.to_string() })),
        };
        emit(json!({ "ev": "ready", "ok": true }));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let st = state.clone();
        std::thread::spawn(move || {
            read_commands(&st, || {
                let _ = tx.send(());
            });
        });
        rt.block_on(async move {
            while rx.recv().await.is_some() {
                handle.update(|_| {}).await;
            }
            handle.shutdown().await;
        });
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4,
        NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const WM_TRAY: u32 = WM_APP + 1;
    const WM_REFRESH: u32 = WM_APP + 2;
    const NIN_SELECT: u32 = WM_USER;
    const NIN_KEYSELECT: u32 = WM_USER + 1;
    const ID_TOGGLE: usize = 1;
    const ID_OPEN: usize = 2;
    const ID_QUIT: usize = 3;
    const ID_SERVER: usize = 1000;

    static STATE: Mutex<Option<Arc<Mutex<State>>>> = Mutex::new(None);
    static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn state() -> State {
        STATE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).clone())
            .unwrap_or_default()
    }

    unsafe fn hicon(icons: &[Icon]) -> HICON {
        let want = unsafe { GetSystemMetrics(SM_CXSMICON) };
        let Some(i) = icons.iter().min_by_key(|i| (i.w - want).abs() * 2 + i32::from(i.w < want)) else { return std::ptr::null_mut() };
        let mut bgra = Vec::with_capacity(i.argb.len());
        for p in i.argb.chunks_exact(4) {
            bgra.extend_from_slice(&[p[3], p[2], p[1], p[0]]);
        }
        let mask = vec![0u8; ((i.w + 15) / 16 * 2 * i.h) as usize];
        unsafe { CreateIcon(GetModuleHandleW(std::ptr::null()), i.w, i.h, 1, 32, mask.as_ptr(), bgra.as_ptr()) }
    }

    unsafe fn notify(hwnd: HWND, op: u32) {
        let s = state();
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = unsafe { hicon(s.icons()) };
        let tip = wide(&s.tooltip());
        let n = tip.len().min(nid.szTip.len() - 1);
        nid.szTip[..n].copy_from_slice(&tip[..n]);
        unsafe {
            Shell_NotifyIconW(op, &nid);
            if op == NIM_ADD {
                nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                Shell_NotifyIconW(NIM_SETVERSION, &nid);
            }
            if !nid.hIcon.is_null() {
                DestroyIcon(nid.hIcon);
            }
        }
    }

    unsafe fn menu(hwnd: HWND, x: i32, y: i32) {
        let s = state();
        unsafe {
            let m = CreatePopupMenu();
            let head = wide(&if s.server.is_empty() { s.title.clone() } else { format!("{} · {}", s.title, s.server) });
            AppendMenuW(m, MF_STRING | MF_GRAYED, 0, head.as_ptr());
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            let toggle = wide(if s.on || s.busy {
                rv_core::lang::t("Отключить", "Disconnect")
            } else {
                rv_core::lang::t("Подключить", "Connect")
            });
            AppendMenuW(m, MF_STRING, ID_TOGGLE, toggle.as_ptr());
            let mut names = Vec::new();
            if !s.servers.is_empty() {
                let sub = CreatePopupMenu();
                for (i, name) in s.servers.iter().enumerate().take(200) {
                    names.push(wide(&name.replace('&', "&&")));
                    let flags = MF_STRING | if *name == s.current { MF_CHECKED } else { 0 };
                    AppendMenuW(sub, flags, ID_SERVER + i, names.last().unwrap().as_ptr());
                }
                let label = wide(rv_core::lang::t("Серверы", "Servers"));
                AppendMenuW(m, MF_POPUP, sub as usize, label.as_ptr());
            }
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            let open = wide(rv_core::lang::t("Открыть ryNet", "Open ryNet"));
            AppendMenuW(m, MF_STRING, ID_OPEN, open.as_ptr());
            let quit = wide(rv_core::lang::t("Выход", "Quit"));
            AppendMenuW(m, MF_STRING, ID_QUIT, quit.as_ptr());
            SetMenuDefaultItem(m, ID_OPEN as u32, 0);
            SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenuEx(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, x, y, hwnd, std::ptr::null()) as usize;
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(m);
            match cmd {
                ID_TOGGLE => emit(json!({ "ev": "toggle" })),
                ID_OPEN => emit(json!({ "ev": "open" })),
                ID_QUIT => emit(json!({ "ev": "quit" })),
                c if c >= ID_SERVER => {
                    if let Some(name) = s.servers.get(c - ID_SERVER) {
                        emit(json!({ "ev": "server", "name": name }));
                    }
                }
                _ => {}
            }
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_TRAY => {
                    match (lp as u32) & 0xffff {
                        WM_LBUTTONUP | NIN_SELECT | NIN_KEYSELECT => emit(json!({ "ev": "open" })),
                        WM_CONTEXTMENU | WM_RBUTTONUP => {
                            let (x, y) = ((wp & 0xffff) as i16 as i32, ((wp >> 16) & 0xffff) as i16 as i32);
                            let (x, y) = if x == 0 && y == 0 {
                                let mut p = POINT { x: 0, y: 0 };
                                GetCursorPos(&mut p);
                                (p.x, p.y)
                            } else {
                                (x, y)
                            };
                            menu(hwnd, x, y);
                        }
                        _ => {}
                    }
                    0
                }
                WM_REFRESH => {
                    notify(hwnd, NIM_MODIFY);
                    0
                }
                WM_DESTROY => {
                    notify(hwnd, NIM_DELETE);
                    PostQuitMessage(0);
                    0
                }
                m if m != 0 && m == TASKBAR_CREATED.load(std::sync::atomic::Ordering::Relaxed) => {
                    notify(hwnd, NIM_ADD);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }

    unsafe fn dark_menus() {
        unsafe {
            let lib = LoadLibraryW(wide("uxtheme.dll").as_ptr());
            if lib.is_null() {
                return;
            }
            if let Some(f) = GetProcAddress(lib, 135 as *const u8) {
                let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
                set(1);
            }
            if let Some(f) = GetProcAddress(lib, 136 as *const u8) {
                let flush: unsafe extern "system" fn() = std::mem::transmute(f);
                flush();
            }
        }
    }

    pub fn run(state: Arc<Mutex<State>>) {
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
        unsafe {
            dark_menus();
            let class = wide("ryNetTray");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: GetModuleHandleW(std::ptr::null()),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), std::sync::atomic::Ordering::Relaxed);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                wc.hInstance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return emit(json!({ "ev": "ready", "ok": false, "error": "window" }));
            }
            notify(hwnd, NIM_ADD);
            emit(json!({ "ev": "ready", "ok": true }));
            let target = hwnd as usize;
            std::thread::spawn(move || {
                read_commands(&state, || {
                    PostMessageW(target as HWND, WM_REFRESH, 0, 0);
                });
                PostMessageW(target as HWND, WM_CLOSE, 0, 0);
            });
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use super::*;
    pub fn run(_: Arc<Mutex<State>>) {
        emit(json!({ "ev": "ready", "ok": false, "error": "unsupported" }));
    }
}
