#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Local persistence: history JSON, user preferences JSON, vocab JSON, and
//! platform-backed credentials vault.
//!
//! Storage roots:
//! - macOS:   `~/Library/Application Support/OpenLess`
//! - Windows: `%APPDATA%\OpenLess`
//! - Linux:   `$XDG_DATA_HOME/OpenLess` or `~/.local/share/OpenLess`
//! - Android: `{Context.getFilesDir()}/OpenLess` (never `/data/local/tmp`)
//!
//! Credential storage policy: provider credentials are stored in the OS
//! credential vault (macOS Keychain, Windows Credential Manager, Linux keyring).
//! A legacy plaintext JSON file is read once as a migration source and removed
//! after a successful vault write; new writes never persist plaintext secrets.
//!
//! This module is split into focused submodules; everything that was previously
//! reachable as `crate::persistence::*` stays reachable via the glob re-exports
//! below. The shared filesystem helpers and the two cross-cutting constants live
//! here so every submodule can reach them through `super::`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use uuid::Uuid;

mod activity;
#[cfg(any(target_os = "android", test))]
mod android_credentials;
#[cfg(any(target_os = "android", test))]
mod android_storage;
mod correction;
mod credentials;
mod dictionary;
mod history;
mod paths;
mod preferences;
mod style_pack;
mod style_pack_archive;

pub use activity::*;
pub use correction::*;
pub use credentials::*;
pub use dictionary::*;
pub use history::*;
pub use paths::*;
pub use preferences::*;
pub use style_pack::*;
pub(crate) use style_pack_archive::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES;

#[cfg(target_os = "android")]
pub use android_storage::init_android_storage_roots;
#[cfg(target_os = "android")]
pub(crate) use android_storage::{android_log_dir, android_openless_log_candidates};
#[cfg(any(target_os = "android", test))]
use android_storage::is_memory_only_path;

const HISTORY_CAP: usize = 200;
const PREFERENCES_FILE: &str = "preferences.json";

fn data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("OpenLess"))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").context("APPDATA not set")?;
        Ok(PathBuf::from(appdata).join("OpenLess"))
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return Ok(PathBuf::from(xdg).join("OpenLess"));
            }
        }
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("OpenLess"))
    }

    #[cfg(target_os = "android")]
    {
        // Never use std::env::temp_dir() (/data/local/tmp) — apps cannot write there.
        android_storage::android_data_dir()
    }
}

/// Fallback store path when `data_dir()` is unavailable.
/// Android: empty path → memory-only (writes refused); never `/data/local/tmp`.
fn fallback_store_path(file_name: &str) -> PathBuf {
    #[cfg(target_os = "android")]
    {
        let _ = file_name;
        PathBuf::new()
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::temp_dir().join(file_name)
    }
}

fn ensure_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create dir failed: {}", dir.display()))?;
    Ok(())
}

/// Atomic write: write to a unique `*.tmp-<uuid>` first, then rename onto the
/// target path. The unique suffix lets concurrent writers each own their own
/// tmp file, so a parallel rename never finds its source already taken.
fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    #[cfg(any(target_os = "android", test))]
    if is_memory_only_path(path) {
        bail!("atomic write refused: empty path (memory-only store; Android data_dir unavailable)");
    }
    #[cfg(not(any(target_os = "android", test)))]
    if path.as_os_str().is_empty() {
        bail!("atomic write refused: empty path (memory-only store)");
    }
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}", Uuid::new_v4().simple()));
    fs::write(&tmp_path, contents)
        .with_context(|| format!("write tmp failed: {}", tmp_path.display()))?;
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err).with_context(|| format!("rename failed: {}", path.display()));
    }
    Ok(())
}

fn read_or_default<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice::<T>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))
}
