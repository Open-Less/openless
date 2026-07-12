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
            backup_unparseable_preferences(path, &bytes);
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

/// 把无法解析的 preferences.json 原样备份为 `preferences.corrupt-<unix>.json`，
/// 保证抢救/写回之前用户的原始设置永远有一份可人工核对的副本，绝不静默销毁。
fn backup_unparseable_preferences(path: &Path, bytes: &[u8]) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = path.with_file_name(format!("preferences.corrupt-{ts}.json"));
    match fs::write(&backup, bytes) {
        Ok(()) => log::error!(
            "[prefs] original unparseable preferences backed up to {}",
            backup.display()
        ),
        Err(err) => log::warn!(
            "[prefs] failed to back up unparseable preferences to {}: {err}",
            backup.display()
        ),
    }
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
    use super::{read_preferences, PreferencesStore};
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
