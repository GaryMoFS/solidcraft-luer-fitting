#![no_std]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

extern crate alloc;

mod luer;
mod placement;

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use serde_json::{Value, json};

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

wit_bindgen::generate!({
    path: "../wit/plugin.wit",
    world: "command-dialog-plugin",
});

use craft::plugin::commands;
use craft::plugin::control;
pub use craft::plugin::types::{FieldFailure, PluginError};
use exports::craft::plugin::dialog_provider::Guest as DialogGuest;
use exports::craft::plugin::entry::Guest as EntryGuest;

struct LuerFittingPlugin;

export!(LuerFittingPlugin);

impl EntryGuest for LuerFittingPlugin {
    fn execute(entry: String, params: String) -> Result<String, PluginError> {
        let value: Value = serde_json::from_str(&params)
            .map_err(|e| invalid("params", format!("invalid JSON: {e}")))?;
        match entry.as_str() {
            "luer-fitting" => luer::run_luer_fitting(&value),
            _ => Err(invalid("entry", format!("unknown entry `{entry}`"))),
        }
    }
}

impl DialogGuest for LuerFittingPlugin {
    fn update_fields(entry: String, values: String) -> Result<String, PluginError> {
        if entry != "luer-fitting" {
            return Ok(json!({}).to_string());
        }
        let value: Value = serde_json::from_str(&values).unwrap_or_else(|_| json!({}));
        let clearance = value
            .get("clearance")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let max_hole = luer::max_safe_hole_diameter(clearance);
        let hole = value
            .get("hole_diameter")
            .and_then(Value::as_f64)
            .unwrap_or(2.25);

        let mut patches = serde_json::Map::new();
        if hole > max_hole {
            patches.insert("hole_diameter".to_string(), json!(luer::round3(max_hole)));
        }
        Ok(Value::Object(patches).to_string())
    }
}

pub fn report_progress(fraction: f32, msg: &str) -> Result<(), PluginError> {
    if control::cancelled() {
        return Err(PluginError::Cancelled);
    }
    control::progress(fraction.clamp(0.0, 1.0), msg);
    Ok(())
}

pub fn invalid(field: &str, reason: impl Into<String>) -> PluginError {
    PluginError::InvalidInput(FieldFailure {
        field: field.to_string(),
        reason: reason.into(),
    })
}

pub fn run_cmd(cmd: &str, args: Value) -> Result<Value, PluginError> {
    if control::cancelled() {
        return Err(PluginError::Cancelled);
    }
    let raw = commands::run(cmd, &args.to_string())?;
    serde_json::from_str(&raw)
        .map_err(|e| PluginError::Trap(format!("invalid JSON from {cmd}: {e}")))
}

pub fn list_existing_doc_names() -> Result<BTreeSet<String>, PluginError> {
    let res = run_cmd("document.inspect", json!({}))?;
    let mut set = BTreeSet::new();
    for key in ["sketches", "features", "bodies"] {
        if let Some(arr) = res.get(key).and_then(Value::as_array) {
            for item in arr {
                if let Some(name) = item.get("name").and_then(Value::as_str) {
                    set.insert(name.to_string());
                }
            }
        }
    }
    Ok(set)
}

pub fn unique_name(base: &str, existing: &mut BTreeSet<String>) -> String {
    if !existing.contains(base) {
        existing.insert(base.to_string());
        return base.to_string();
    }
    for idx in 2..=999 {
        let cand = format!("{base}_{idx}");
        if !existing.contains(&cand) {
            existing.insert(cand.clone());
            return cand;
        }
    }
    let fallback = format!("{base}_1000");
    existing.insert(fallback.clone());
    fallback
}
