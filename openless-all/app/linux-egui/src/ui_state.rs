//! Persistence of pure Linux-UI state (view-model preferences that are *not*
//! business truth). Business truth lives in Core's `UserPreferences`; this
//! module keeps only UI-surface state such as the chosen display language,
//! stored on disk beside Core but never inside a Core-owned document.

use std::path::PathBuf;

use crate::desktop::atomic_save;
use crate::i18n::{LocalePref, FOLLOW_SYSTEM};

const STATE_FILE: &str = "linux-ui-state.json";

#[derive(Debug)]
pub enum UiStateError {
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Json(String),
}

impl std::fmt::Display for UiStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::Json(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for UiStateError {}

fn io_error(operation: &'static str, source: std::io::Error) -> UiStateError {
    UiStateError::Io { operation, source }
}

/// The application data directory, mirroring the runtime's `backend_config`
/// derivation (XDG_DATA_HOME, falling back to `~/.local/share`). Kept here so
/// the main window *and* the separate popup window resolve the exact same
/// state file without sharing a process-local handle.
pub fn ui_state_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    Some(base.join("OpenLess"))
}

/// Absolute path to the persisted Linux-UI state document.
pub fn ui_state_path() -> Option<PathBuf> {
    ui_state_dir().map(|dir| dir.join(STATE_FILE))
}

/// Read the persisted locale preference. A missing, empty or unreadable file
/// (plus any unrecognised value) degrades to `LocalePref::System` — following
/// the OS locale — so a corrupt state can never wedge the UI on the wrong
/// language.
pub fn load_locale_pref() -> LocalePref {
    let Some(path) = ui_state_path() else {
        return LocalePref::System;
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return LocalePref::System,
    };
    let value = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(value) => value,
        Err(_) => return LocalePref::System,
    };
    match value.get("locale").and_then(serde_json::Value::as_str) {
        Some(tag) => LocalePref::from_tag(tag),
        None => LocalePref::System,
    }
}

/// Persist the locale preference atomically. Returns an error only when the
/// state cannot be written at all; unknown environments (no HOME) simply leave
/// the preference unsaved and are reported as a recoverable failure.
pub fn save_locale_pref(pref: LocalePref) -> Result<(), UiStateError> {
    let tag = match pref {
        LocalePref::System => FOLLOW_SYSTEM.to_string(),
        LocalePref::Lang(lang) => lang.tag().to_string(),
    };
    write_state_value("locale", serde_json::Value::String(tag))
}

/// The state document as a JSON object. Unreadable or non-object content
/// degrades to an empty map so one bad field can never wedge the whole file.
fn read_state_object() -> serde_json::Map<String, serde_json::Value> {
    let Some(path) = ui_state_path() else {
        return serde_json::Map::new();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return serde_json::Map::new();
    };
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// Merge one key into the state document, leaving every other key intact.
fn write_state_value(key: &str, value: serde_json::Value) -> Result<(), UiStateError> {
    let Some(dir) = ui_state_dir() else {
        return Err(io_error(
            "resolve ui-state directory",
            std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is unavailable"),
        ));
    };
    let Some(path) = ui_state_path() else {
        return Err(io_error(
            "resolve ui-state path",
            std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is unavailable"),
        ));
    };
    std::fs::create_dir_all(&dir).map_err(|error| io_error("create ui-state directory", error))?;
    let mut document = read_state_object();
    document.insert(key.to_string(), value);
    let bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(document))
        .map_err(|error| UiStateError::Json(error.to_string()))?;
    atomic_save(&path, &bytes)
        .map(|_| ())
        .map_err(|error| match error {
            crate::desktop::DesktopError::InvalidInput(message) => UiStateError::Json(message),
            crate::desktop::DesktopError::Io { operation, source } => {
                UiStateError::Io { operation, source }
            }
            other => UiStateError::Json(other.to_string()),
        })
}

/// Whether the user dismissed the shortcut card on the Quick Note page.
/// A missing key keeps the card visible, matching the upstream default.
pub fn load_quick_note_shortcut_hidden() -> bool {
    read_state_object()
        .get("quick_note_shortcut_hidden")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

pub fn save_quick_note_shortcut_hidden(hidden: bool) -> Result<(), UiStateError> {
    write_state_value(
        "quick_note_shortcut_hidden",
        serde_json::Value::Bool(hidden),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;
    use std::sync::{Mutex, OnceLock};

    /// Env vars are process-global, so these tests must not interleave.
    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_tmp_state(run: impl FnOnce(PathBuf)) {
        // Recover a poisoned lock (from an earlier assertion) rather than
        // failing the whole module: env vars must stay consistent per test.
        let _guard = test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir =
            std::env::temp_dir().join(format!("openless-ui-state-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // Point every data-dir resolver at the temp dir via XDG_DATA_HOME.
        let previous = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", &dir);
        run(dir.clone());
        if let Some(value) = previous {
            std::env::set_var("XDG_DATA_HOME", value);
        } else {
            std::env::remove_var("XDG_DATA_HOME");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn absent_state_resolves_to_follow_system() {
        with_tmp_state(|dir| {
            // No file has been written in this fresh dir.
            assert_eq!(load_locale_pref(), LocalePref::System);
            std::fs::remove_dir_all(&dir).ok();
        });
    }

    #[test]
    fn locale_preference_persists_and_roundtrips_across_reload() {
        with_tmp_state(|_dir| {
            save_locale_pref(LocalePref::Lang(Lang::ZhTw)).unwrap();
            assert_eq!(load_locale_pref(), LocalePref::Lang(Lang::ZhTw));
            save_locale_pref(LocalePref::System).unwrap();
            assert_eq!(load_locale_pref(), LocalePref::System);
            save_locale_pref(LocalePref::Lang(Lang::Ko)).unwrap();
            assert_eq!(load_locale_pref(), LocalePref::Lang(Lang::Ko));
        });
    }

    #[test]
    fn quick_note_shortcut_visibility_roundtrips_without_clobbering_locale() {
        with_tmp_state(|_dir| {
            // Absent key = the card is visible, matching the upstream default.
            assert!(!load_quick_note_shortcut_hidden());
            save_locale_pref(LocalePref::Lang(Lang::Ja)).unwrap();
            save_quick_note_shortcut_hidden(true).unwrap();
            assert!(load_quick_note_shortcut_hidden());
            // Writing one key must not drop the other.
            assert_eq!(load_locale_pref(), LocalePref::Lang(Lang::Ja));
            save_quick_note_shortcut_hidden(false).unwrap();
            assert!(!load_quick_note_shortcut_hidden());
            assert_eq!(load_locale_pref(), LocalePref::Lang(Lang::Ja));
        });
    }

    #[test]
    fn corrupt_state_file_degrades_to_follow_system() {
        with_tmp_state(|_dir| {
            let path = ui_state_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "{ not json").unwrap();
            assert_eq!(load_locale_pref(), LocalePref::System);
            // A later valid write repairs the state.
            save_locale_pref(LocalePref::Lang(Lang::Ja)).unwrap();
            assert_eq!(load_locale_pref(), LocalePref::Lang(Lang::Ja));
        });
    }
}
