//! Android app-private storage roots.
//!
//! Honor / Huawei devices reject writes under `/data/local/tmp`. Persistence and
//! file logging must use `Context.getFilesDir()` (or Tauri's
//! `TAURI_ANDROID_APP_DATA_DIR`) and never fall back to `std::env::temp_dir()`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};

static ANDROID_APP_FILES_DIR: OnceLock<PathBuf> = OnceLock::new();
static ANDROID_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static ANDROID_LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

const INIT_ATTEMPTS: usize = 5;
const INIT_RETRY_DELAY: Duration = Duration::from_millis(40);

/// Pure resolution used by runtime and unit tests. Prefers JNI `filesDir`, then env.
/// Never returns a temp-dir path.
pub(crate) fn resolve_android_app_files_dir(
    jni_path: Option<&str>,
    env_path: Option<&str>,
) -> Result<PathBuf> {
    if let Some(path) = jni_path.map(str::trim).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env_path.map(str::trim).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    bail!("Android app files dir unavailable (JNI getFilesDir + TAURI_ANDROID_APP_DATA_DIR)")
}

#[cfg(target_os = "android")]
fn probe_jni_files_dir() -> Option<String> {
    crate::android::jni::android::app_files_dir().ok()
}

#[cfg(not(target_os = "android"))]
fn probe_jni_files_dir() -> Option<String> {
    None
}

fn env_app_data_dir() -> Option<String> {
    std::env::var("TAURI_ANDROID_APP_DATA_DIR").ok()
}

/// Resolve and cache `filesDir`, `{filesDir}/OpenLess`, and `{filesDir}/logs`.
/// Safe to call multiple times; subsequent calls are no-ops once initialized.
pub fn init_android_storage_roots() -> Result<()> {
    if ANDROID_DATA_DIR.get().is_some() {
        return Ok(());
    }

    let mut last_err = None;
    let mut files_dir = None;
    for attempt in 0..INIT_ATTEMPTS {
        match resolve_android_app_files_dir(
            probe_jni_files_dir().as_deref(),
            env_app_data_dir().as_deref(),
        ) {
            Ok(path) => {
                files_dir = Some(path);
                break;
            }
            Err(error) => {
                last_err = Some(error);
                if attempt + 1 < INIT_ATTEMPTS {
                    thread::sleep(INIT_RETRY_DELAY);
                }
            }
        }
    }

    let files_dir = files_dir.ok_or_else(|| {
        last_err.unwrap_or_else(|| {
            anyhow::anyhow!("Android app files dir unavailable after {INIT_ATTEMPTS} attempts")
        })
    })?;

    let data_dir = files_dir.join("OpenLess");
    let log_dir = files_dir.join("logs");
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("create Android data dir {}", data_dir.display()))?;
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("create Android log dir {}", log_dir.display()))?;

    let _ = ANDROID_APP_FILES_DIR.set(files_dir.clone());
    let _ = ANDROID_DATA_DIR.set(data_dir.clone());
    let _ = ANDROID_LOG_DIR.set(log_dir.clone());

    eprintln!(
        "[android-storage] roots ready filesDir={} dataDir={} logDir={}",
        files_dir.display(),
        data_dir.display(),
        log_dir.display()
    );
    Ok(())
}

pub(crate) fn android_app_files_dir() -> Result<PathBuf> {
    if let Some(dir) = ANDROID_APP_FILES_DIR.get() {
        return Ok(dir.clone());
    }
    let path = resolve_android_app_files_dir(
        probe_jni_files_dir().as_deref(),
        env_app_data_dir().as_deref(),
    )?;
    let _ = ANDROID_APP_FILES_DIR.set(path.clone());
    Ok(path)
}

pub(crate) fn android_data_dir() -> Result<PathBuf> {
    if let Some(dir) = ANDROID_DATA_DIR.get() {
        return Ok(dir.clone());
    }
    let files = android_app_files_dir()?;
    let data = files.join("OpenLess");
    std::fs::create_dir_all(&data)
        .with_context(|| format!("create Android data dir {}", data.display()))?;
    let _ = ANDROID_DATA_DIR.set(data.clone());
    Ok(data)
}

pub(crate) fn android_log_dir() -> Result<PathBuf> {
    if let Some(dir) = ANDROID_LOG_DIR.get() {
        return Ok(dir.clone());
    }
    let files = android_app_files_dir()?;
    let logs = files.join("logs");
    std::fs::create_dir_all(&logs)
        .with_context(|| format!("create Android log dir {}", logs.display()))?;
    let _ = ANDROID_LOG_DIR.set(logs.clone());
    Ok(logs)
}

/// Candidate paths for `openless.log` (export + ADB dump).
pub(crate) fn android_openless_log_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(files) = android_app_files_dir() {
        candidates.push(files.join("logs").join("openless.log"));
        candidates.push(files.join("openless.log"));
        if let Some(parent) = files.parent() {
            candidates.push(parent.join("logs").join("openless.log"));
        }
    }
    if let Ok(logs) = android_log_dir() {
        let path = logs.join("openless.log");
        if !candidates.iter().any(|c| c == &path) {
            candidates.push(path);
        }
    }
    candidates
}

pub(crate) fn is_memory_only_path(path: &Path) -> bool {
    path.as_os_str().is_empty()
}

#[cfg(test)]
mod tests {
    use super::resolve_android_app_files_dir;
    use std::path::PathBuf;

    #[test]
    fn prefers_jni_files_dir_over_env() {
        let path = resolve_android_app_files_dir(
            Some("/data/user/0/com.openless.app/files"),
            Some("/data/local/tmp/wrong"),
        )
        .expect("resolve");
        assert_eq!(
            path,
            PathBuf::from("/data/user/0/com.openless.app/files")
        );
    }

    #[test]
    fn falls_back_to_env_when_jni_missing() {
        let path =
            resolve_android_app_files_dir(None, Some("/data/user/0/com.openless.app/files"))
                .expect("resolve");
        assert_eq!(
            path,
            PathBuf::from("/data/user/0/com.openless.app/files")
        );
    }

    #[test]
    fn rejects_empty_sources_without_temp_fallback() {
        let err = resolve_android_app_files_dir(Some(""), Some("   ")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unavailable"), "{msg}");
        assert!(!msg.to_ascii_lowercase().contains("temp"), "{msg}");
    }
}
