use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

pub fn set_english(on: bool) {
    EN.store(on, Ordering::Relaxed);
}

pub fn english() -> bool {
    EN.load(Ordering::Relaxed)
}

pub fn t(ru: &'static str, en: &'static str) -> &'static str {
    if english() { en } else { ru }
}
