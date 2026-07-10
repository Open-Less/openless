#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! User preferences store: a single JSON document held in memory behind a lock,
//! with a one-time `streamingInsert` default migration on load.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use parking_lot::Mutex;

use super::{atomic_write, data_dir, ensure_dir, PREFERENCES_FILE};
use crate::types::UserPreferences;

fn read_preferences(path: &Path) -> Result<UserPreferences> {
    if !path.exists() {
        return Ok(UserPreferences::default());
    }
    let bytes = fs::read(path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(UserPreferences::default());
    }
    let prefs = serde_json::from_slice::<UserPreferences>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))?;

    // issue #440：老版本可能已把旧默认 `streamingInsert:false` 写进 preferences.json。
    // 反序列化会在内存里迁到 true，但还必须把迁移标记落盘，否则每次启动都停留在
    // “旧文件”状态，无法表达用户后续手动关闭后的 durable opt-out。
    let streaming_default_migrated = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("streamingInsertDefaultMigrated")
                .and_then(|flag| flag.as_bool())
        })
        .unwrap_or(false);
    if !streaming_default_migrated {
        match serde_json::to_vec_pretty(&prefs)
            .context("encode prefs failed")
            .and_then(|json| atomic_write(path, &json))
        {
            Ok(()) => log::info!("[prefs] migrated streamingInsert default marker"),
            Err(err) => log::warn!(
                "[prefs] failed to persist streamingInsert migration marker for {}: {}",
                path.display(),
                err
            ),
        }
    }

    Ok(prefs)
}

pub struct PreferencesStore {
    path: PathBuf,
    state: Mutex<UserPreferences>,
}

impl PreferencesStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        let path = dir.join(PREFERENCES_FILE);
        let prefs = if path.exists() {
            read_preferences(&path).unwrap_or_else(|e| {
                log::warn!(
                    "[prefs] load {} failed, using defaults: {}",
                    path.display(),
                    e
                );
                UserPreferences::default()
            })
        } else {
            UserPreferences::default()
        };
        Ok(Self {
            path,
            state: Mutex::new(prefs),
        })
    }

    /// 降级实例：data_dir 不可用时使用默认配置，写操作会安静地失败。
    pub(crate) fn new_fallback() -> Self {
        Self {
            path: std::env::temp_dir().join("openless_prefs_fallback.json"),
            state: Mutex::new(UserPreferences::default()),
        }
    }

    pub fn get(&self) -> UserPreferences {
        self.state.lock().clone()
    }

    pub fn set(&self, prefs: UserPreferences) -> Result<()> {
        let json = serde_json::to_vec_pretty(&prefs).context("encode prefs failed")?;
        let mut guard = self.state.lock();
        atomic_write(&self.path, &json)?;
        *guard = prefs;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::read_preferences;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn legacy_streaming_insert_false_is_migrated_and_marker_is_persisted() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-prefs-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        let path = tmp.join("preferences.json");
        fs::write(
            &path,
            r#"{
                "streamingInsert": false,
                "streamingInsertSaveClipboard": true
            }"#,
        )
        .expect("write legacy prefs");

        let prefs = read_preferences(&path).expect("read prefs");
        assert!(prefs.streaming_insert);
        assert!(prefs.streaming_insert_default_migrated);

        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("read saved prefs"))
                .expect("decode saved prefs");
        assert_eq!(
            saved
                .get("streamingInsert")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            saved
                .get("streamingInsertDefaultMigrated")
                .and_then(|value| value.as_bool()),
            Some(true)
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}
