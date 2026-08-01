#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! User preferences store: a single JSON document held in memory behind a lock,
//! with a one-time `streamingInsert` default migration on load.

use std::fs::{self, OpenOptions};
use std::io::Write;
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
    let prefs = match serde_json::from_slice::<UserPreferences>(&bytes) {
        Ok(prefs) => prefs,
        Err(err) => {
            // 严格解析失败绝不能静默回落到 default——那样应用一启动就“忘光”所有设置，
            // 用户随手改一项就把整份 preferences.json 覆盖成默认，历史设置永久丢失
            // （用户反馈：每次重装 app 后热键等设置读不到的根因路径）。
            // 改为：① 原样备份坏文件，永不销毁；② 逐字段抢救所有仍合法的设置；
            // ③ 把抢救结果写回，得到一份干净可解析的文件，后续走正常路径。
            log::error!(
                "[prefs] strict decode of {} failed: {err:#}; backing up original and salvaging valid fields",
                path.display()
            );
            let backup = backup_unparseable_preferences(path, &bytes)
                .with_context(|| format!("backup failed: {}", path.display()))?;
            log::info!(
                "[prefs] original unparseable preferences backed up to {}",
                backup.display()
            );
            let salvaged = UserPreferences::salvage_from_json_bytes(&bytes);
            match serde_json::to_vec_pretty(&salvaged)
                .context("encode salvaged prefs failed")
                .and_then(|json| atomic_write(path, &json))
            {
                Ok(()) => log::info!(
                    "[prefs] salvaged preferences written back to {}",
                    path.display()
                ),
                Err(err) => log::warn!(
                    "[prefs] failed to persist salvaged preferences to {}: {err}",
                    path.display()
                ),
            }
            return Ok(salvaged);
        }
    };

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

/// 把无法解析的 preferences.json 原样备份为唯一文件。
///
/// 使用 `create_new` 保证不会覆盖已有备份；备份失败时返回错误，调用方必须保留原文件。
fn backup_unparseable_preferences(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let backup = path.with_file_name(format!(
        "preferences.corrupt-{ts}-{}.json",
        uuid::Uuid::new_v4().simple()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
        .with_context(|| format!("create backup failed: {}", backup.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write backup failed: {}", backup.display()))?;
    file.sync_all()
        .with_context(|| format!("flush backup failed: {}", backup.display()))?;
    Ok(backup)
}

pub struct PreferencesStore {
    path: PathBuf,
    state: Mutex<UserPreferences>,
}

impl PreferencesStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Self::from_path(dir.join(PREFERENCES_FILE))
    }

    fn from_path(path: PathBuf) -> Result<Self> {
        let prefs = read_preferences(&path)?;
        Ok(Self {
            path,
            state: Mutex::new(prefs),
        })
    }

    /// 降级实例：data_dir 不可用时使用默认配置。
    /// Android 使用空 path（内存态，写盘明确失败），禁止落 `/data/local/tmp`。
    pub(crate) fn new_fallback() -> Self {
        Self {
            path: super::fallback_store_path("openless_prefs_fallback.json"),
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

    pub fn set_preserving_current_style_preferences(
        &self,
        mut prefs: UserPreferences,
    ) -> Result<()> {
        let mut guard = self.state.lock();
        prefs.preserve_style_preferences_from(&guard);
        let json = serde_json::to_vec_pretty(&prefs).context("encode prefs failed")?;
        atomic_write(&self.path, &json)?;
        *guard = prefs;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{backup_unparseable_preferences, read_preferences, PreferencesStore};
    use crate::types::{builtin_style_pack_id, PolishMode, UserPreferences};
    use parking_lot::Mutex;
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

    #[test]
    fn corrupt_preferences_are_backed_up_before_salvage() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-prefs-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        let path = tmp.join("preferences.json");
        let original = br#"{
            "defaultMode": "totally-removed-mode",
            "activeAsrProvider": "preserved-provider"
        }"#;
        fs::write(&path, original).expect("write corrupt prefs");

        let prefs = read_preferences(&path).expect("salvage prefs");
        assert_eq!(prefs.active_asr_provider, "preserved-provider");

        let mut backups = fs::read_dir(&tmp)
            .expect("read temp dir")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("preferences.corrupt-"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        let backup = backups.pop().expect("backup path");
        assert_eq!(fs::read(&backup).expect("read backup"), original);
        assert!(serde_json::from_slice::<UserPreferences>(
            &fs::read(&path).expect("read salvaged prefs")
        )
        .is_ok());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn corrupt_preference_backups_are_unique_and_preserve_each_snapshot() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-prefs-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        let path = tmp.join("preferences.json");

        let first = backup_unparseable_preferences(&path, b"first").expect("first backup");
        let second = backup_unparseable_preferences(&path, b"second").expect("second backup");

        assert_ne!(first, second);
        assert_eq!(fs::read(first).expect("read first backup"), b"first");
        assert_eq!(fs::read(second).expect("read second backup"), b"second");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preferences_store_init_propagates_load_failures() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-prefs-test-{}", uuid::Uuid::new_v4()));
        let path = tmp.join("preferences.json");
        fs::create_dir_all(&path).expect("create directory at preferences path");

        let result = PreferencesStore::from_path(path);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn set_preserving_current_style_preferences_keeps_store_style_fields() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-prefs-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        let path = tmp.join("preferences.json");
        let current = UserPreferences {
            default_mode: PolishMode::Light,
            active_style_pack_id: "local.light-cleanup".to_string(),
            ..UserPreferences::default()
        };
        let store = PreferencesStore {
            path,
            state: Mutex::new(current),
        };
        let incoming = UserPreferences {
            default_mode: PolishMode::Formal,
            active_style_pack_id: builtin_style_pack_id(PolishMode::Formal).to_string(),
            microphone_device_name: "External Mic".to_string(),
            ..UserPreferences::default()
        };

        store
            .set_preserving_current_style_preferences(incoming)
            .expect("save prefs");

        let saved = store.get();
        assert_eq!(saved.default_mode, PolishMode::Light);
        assert_eq!(saved.active_style_pack_id, "local.light-cleanup");
        assert_eq!(saved.microphone_device_name, "External Mic");
        let saved_on_disk = read_preferences(&store.path).expect("read saved prefs");
        assert_eq!(saved_on_disk.default_mode, PolishMode::Light);
        assert_eq!(saved_on_disk.active_style_pack_id, "local.light-cleanup");
        assert_eq!(saved_on_disk.microphone_device_name, "External Mic");

        let _ = fs::remove_dir_all(&tmp);
    }
}
