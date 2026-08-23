//! Agent debug NDJSON logging (session ddfc8d).

use serde_json::{json, Value};
use std::io::Write;

fn debug_log_path() -> Option<std::path::PathBuf> {
    if let Ok(root) = std::env::var("OPENLESS_REPO_ROOT") {
        return Some(std::path::PathBuf::from(root).join("debug-ddfc8d.log"));
    }
    Some(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../debug-ddfc8d.log"),
    )
}

pub fn agent_debug_log(hypothesis_id: &str, location: &str, message: &str, data: Value) {
    let payload = json!({
        "sessionId": "ddfc8d",
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": chrono::Utc::now().timestamp_millis(),
    });
    log::debug!("[agent-debug] {}", payload);
    if let Some(path) = debug_log_path() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{}", payload);
        }
    }
}
