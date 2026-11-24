use std::path::Path;

use serde_json::{Value, json};

pub fn init(_dir: &Path) -> Vec<String> {
    Vec::new()
}

pub fn list() -> Value {
    json!([])
}

pub fn call(op: &str, _arg: &Value, _bytes: Option<&[u8]>) -> Result<Value, String> {
    match op {
        "init" => Ok(json!({ "errors": [] })),
        "list" | "logs" => Ok(list()),
        _ => Err(crate::lang::t("Плагины в этой сборке не поддерживаются", "Plugins are not supported in this build").into()),
    }
}
