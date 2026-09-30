//! Storage path resolution: models root, recordings archive (with retention
//! pruning), and the Windows
//! Foundry Local cache roots.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::{data_dir, ensure_dir, HISTORY_CAP, PREFERENCES_FILE};

/// Default models root: `<data_dir>/models/`.
pub fn default_models_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("models");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// Converts a user-selected parent directory into the actual models root.
///
/// The UI has the user pick an ordinary directory; OpenLess always creates
/// `OpenLess/models/` under it instead of scattering model files from multiple
/// engines directly in the chosen directory root.
pub fn models_root_for_base_dir(base_dir: Option<&str>) -> Result<PathBuf> {
    let trimmed = base_dir.map(str::trim).filter(|value| !value.is_empty());
    let dir = match trimmed {
        Some(base) => PathBuf::from(base).join("OpenLess").join("models"),
        None => return default_models_root(),
    };
    ensure_dir(&dir)?;
    Ok(dir)
}

fn configured_models_base_dir() -> Result<Option<String>> {
    let path = data_dir()?.join(PREFERENCES_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let value = serde_json::from_slice::<serde_json::Value>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))?;
    Ok(value
        .get("localAsrModelsBaseDir")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string))
}

/// Actual models root under the current configuration.
pub fn models_root() -> Result<PathBuf> {
    models_root_for_base_dir(configured_models_base_dir()?.as_deref())
}

/// Recording archive directory: `<data_dir>/recordings/`.
/// Populated only when `prefs.record_audio_for_debug` is on (one
/// `<session_id>.wav` per session).
/// Also subject to `history_retention_days` pruning (old files trimmed when a
/// new one is written).
pub fn recordings_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("recordings");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// Permanent quick-note archives live outside the ordinary debug-recording
/// directory so the normal WAV count/retention prune can never remove them.
pub fn quick_note_recordings_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("quick-notes").join("recordings");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// Prunes `recordings/*.wav` with a double cap:
/// - `retention_days > 0` -> delete files older than N days (same retention
///   logic as history).
/// - `max_entries == Some(n)` -> keep the newest n files by mtime descending
///   (clamped to 1..=HISTORY_CAP); with `None`, fall back to the HISTORY_CAP
///   (200) hard limit to avoid unbounded growth.
/// Called before each new recording is created. Failures only log a warn so
/// the main path is unaffected.
pub fn prune_recordings(retention_days: u32, max_entries: Option<u32>) -> Result<()> {
    let dir = match data_dir() {
        Ok(d) => d.join("recordings"),
        Err(_) => return Ok(()),
    };
    if !dir.exists() {
        return Ok(());
    }

    // Step 1: prune by age. Scan .wav only, consistent with step 2; files whose
    // metadata cannot be read count as "expired" — orphans from fs corruption
    // or future format changes should be reclaimed, not accumulated forever.
    if retention_days > 0 {
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(u64::from(retention_days) * 24 * 3600);
        for entry in fs::read_dir(&dir).context("read recordings dir")?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
                continue;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            if modified < cutoff {
                if let Err(err) = fs::remove_file(&path) {
                    log::warn!("[recordings] prune (days) remove failed for {path:?}: {err}");
                }
            }
        }
    }

    // Step 2: prune by count. Remaining wavs sorted by mtime descending; delete
    // those beyond the cap.
    let cap = max_entries
        .map(|n| (n as usize).clamp(1, HISTORY_CAP))
        .unwrap_or(HISTORY_CAP);
    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(&dir)
        .context("read recordings dir")?
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            // .wav only, so other future archive types are never deleted by
            // mistake.
            if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((path, modified))
        })
        .collect();
    if entries.len() <= cap {
        return Ok(());
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in entries.into_iter().skip(cap) {
        if let Err(err) = fs::remove_file(&path) {
            log::warn!(
                "[recordings] prune (count) remove failed for {:?}: {err}",
                path
            );
        }
    }
    Ok(())
}

/// Recording file path for one session. Does not guarantee the file exists
/// (DictationSession.has_audio_recording decides whether it was ever written).
/// The frontend reads the byte stream via the `read_audio_recording` IPC to
/// feed HTMLAudio.
pub fn recording_path_for_session(session_id: &str) -> Result<PathBuf> {
    Ok(recordings_root()?.join(format!("{session_id}.wav")))
}

pub fn quick_note_recording_path_for_session(session_id: &str) -> Result<PathBuf> {
    Ok(quick_note_recordings_root()?.join(format!("{session_id}.wav")))
}

/// Foundry Local download and cache root. Neither the DLLs nor the models ship
/// in the installer; like Qwen3-ASR they live under OpenLess's models directory
/// so uninstall/cleanup can remove them together with the user data.
#[cfg(target_os = "windows")]
pub fn foundry_local_root() -> Result<PathBuf> {
    let dir = models_root()?.join("foundry-local");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_native_runtime_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("runtime");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_model_cache_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?;
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_app_data_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("app-data");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_logs_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("logs");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::models_root_for_base_dir;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn custom_models_root_uses_openless_models_suffix() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-model-root-{}", uuid::Uuid::new_v4()));
        let root = models_root_for_base_dir(Some(tmp.to_string_lossy().as_ref()))
            .expect("build custom models root");

        assert_eq!(root, tmp.join("OpenLess").join("models"));
        assert!(root.is_dir());

        let _ = fs::remove_dir_all(&tmp);
    }
}
