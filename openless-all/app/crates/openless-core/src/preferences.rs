//! Framework-independent user preferences persistence.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::atomic_write;
use crate::shared_types::UserPreferences;

fn persistence_error(operation: impl Into<String>) -> BackendError {
    BackendError::new(BackendErrorCode::Persistence, operation)
}

fn read_preferences(path: &Path) -> Result<UserPreferences, BackendError> {
    if !path.exists() {
        return Ok(UserPreferences::default());
    }
    let bytes = fs::read(path).map_err(|_| persistence_error("read preferences"))?;
    if bytes.is_empty() {
        return Ok(UserPreferences::default());
    }

    let preferences = match serde_json::from_slice::<UserPreferences>(&bytes) {
        Ok(preferences) => preferences,
        Err(error) => {
            log::error!(
                "[prefs] strict decode of {} failed: {error}; backing up and salvaging",
                path.display()
            );
            let backup = backup_unparseable_preferences(path, &bytes)?;
            log::info!(
                "[prefs] original unparseable preferences backed up to {}",
                backup.display()
            );
            let salvaged = UserPreferences::salvage_from_json_bytes(&bytes);
            match serde_json::to_vec_pretty(&salvaged)
                .map_err(|_| persistence_error("encode salvaged preferences"))
                .and_then(|json| atomic_write(path, &json))
            {
                Ok(()) => log::info!(
                    "[prefs] salvaged preferences written back to {}",
                    path.display()
                ),
                Err(error) => log::warn!(
                    "[prefs] failed to persist salvaged preferences to {}: {error}",
                    path.display()
                ),
            }
            return Ok(salvaged);
        }
    };

    let streaming_default_migrated = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("streamingInsertDefaultMigrated")
                .and_then(|flag| flag.as_bool())
        })
        .unwrap_or(false);
    if !streaming_default_migrated {
        match serde_json::to_vec_pretty(&preferences)
            .map_err(|_| persistence_error("encode migrated preferences"))
            .and_then(|json| atomic_write(path, &json))
        {
            Ok(()) => log::info!("[prefs] migrated streamingInsert default marker"),
            Err(error) => log::warn!(
                "[prefs] failed to persist streamingInsert migration marker for {}: {error}",
                path.display()
            ),
        }
    }

    Ok(preferences)
}

fn backup_unparseable_preferences(path: &Path, bytes: &[u8]) -> Result<PathBuf, BackendError> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let backup = path.with_file_name(format!(
        "preferences.corrupt-{timestamp}-{}.json",
        uuid::Uuid::new_v4().simple()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
        .map_err(|_| persistence_error("create corrupt preferences backup"))?;
    file.write_all(bytes)
        .map_err(|_| persistence_error("write corrupt preferences backup"))?;
    file.sync_all()
        .map_err(|_| persistence_error("flush corrupt preferences backup"))?;
    Ok(backup)
}

pub struct PreferencesStore {
    path: PathBuf,
    state: Mutex<UserPreferences>,
}

impl PreferencesStore {
    /// Opens a preferences document at a host-selected path.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, BackendError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(persistence_error("preferences path is empty"));
        }
        let preferences = read_preferences(&path)?;
        Ok(Self {
            path,
            state: Mutex::new(preferences),
        })
    }

    /// Creates an in-memory fallback. Mutating calls deliberately fail instead
    /// of writing to an implicit temporary or platform directory.
    pub fn in_memory() -> Self {
        Self {
            path: PathBuf::new(),
            state: Mutex::new(UserPreferences::default()),
        }
    }

    /// Creates a default-valued fallback at a path selected by the host.
    /// An empty path retains the memory-only, fail-on-write behavior.
    pub fn fallback(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            state: Mutex::new(UserPreferences::default()),
        }
    }

    pub fn get(&self) -> UserPreferences {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn set(&self, preferences: UserPreferences) -> Result<(), BackendError> {
        let json = serde_json::to_vec_pretty(&preferences)
            .map_err(|_| persistence_error("encode preferences"))?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        atomic_write(&self.path, &json)?;
        *state = preferences;
        Ok(())
    }

    /// Change only a domain's owned fields while holding the persistence lock.
    /// Long-running model preparation must not write back the whole snapshot
    /// taken before an await: another settings request may have committed since.
    /// Work on a clone so a failed disk write leaves the live state unchanged.
    pub(crate) fn update<R>(
        &self,
        update: impl FnOnce(&mut UserPreferences) -> R,
    ) -> Result<R, BackendError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut next = state.clone();
        let result = update(&mut next);
        let json = serde_json::to_vec_pretty(&next)
            .map_err(|_| persistence_error("encode preferences"))?;
        atomic_write(&self.path, &json)?;
        *state = next;
        Ok(result)
    }

    pub fn set_preserving_current_style_preferences(
        &self,
        mut preferences: UserPreferences,
    ) -> Result<(), BackendError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        preferences.preserve_style_preferences_from(&state);
        let json = serde_json::to_vec_pretty(&preferences)
            .map_err(|_| persistence_error("encode preferences"))?;
        atomic_write(&self.path, &json)?;
        *state = preferences;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_types::{builtin_style_pack_id, PolishMode};

    fn temporary_preferences_path() -> (PathBuf, PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("openless-core-prefs-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create temp dir");
        let path = directory.join("preferences.json");
        (directory, path)
    }

    #[test]
    fn legacy_streaming_insert_false_is_migrated_and_marker_is_persisted() {
        let (directory, path) = temporary_preferences_path();
        fs::write(
            &path,
            r#"{
                "streamingInsert": false,
                "streamingInsertSaveClipboard": true
            }"#,
        )
        .expect("write legacy preferences");

        let preferences = read_preferences(&path).expect("read preferences");
        assert!(preferences.streaming_insert);
        assert!(preferences.streaming_insert_default_migrated);

        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("read saved preferences"))
                .expect("decode saved preferences");
        assert_eq!(saved["streamingInsert"], true);
        assert_eq!(saved["streamingInsertDefaultMigrated"], true);

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn corrupt_preferences_are_backed_up_before_salvage() {
        let (directory, path) = temporary_preferences_path();
        let original = br#"{
            "defaultMode": "totally-removed-mode",
            "activeAsrProvider": "preserved-provider"
        }"#;
        fs::write(&path, original).expect("write corrupt preferences");

        let preferences = read_preferences(&path).expect("salvage preferences");
        assert_eq!(preferences.active_asr_provider, "preserved-provider");

        let backups = fs::read_dir(&directory)
            .expect("read temp dir")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("preferences.corrupt-"))
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0]).expect("read backup"), original);
        assert!(serde_json::from_slice::<UserPreferences>(
            &fs::read(&path).expect("read salvaged preferences")
        )
        .is_ok());

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn store_preserves_style_fields_during_settings_updates() {
        let (directory, path) = temporary_preferences_path();
        let store = PreferencesStore::open(&path).expect("open preferences");
        store
            .set(UserPreferences {
                default_mode: PolishMode::Light,
                active_style_pack_id: "local.light-cleanup".to_string(),
                ..UserPreferences::default()
            })
            .expect("seed preferences");

        store
            .set_preserving_current_style_preferences(UserPreferences {
                default_mode: PolishMode::Formal,
                active_style_pack_id: builtin_style_pack_id(PolishMode::Formal).to_string(),
                microphone_device_name: "External Mic".to_string(),
                ..UserPreferences::default()
            })
            .expect("update preferences");

        let saved = store.get();
        assert_eq!(saved.default_mode, PolishMode::Light);
        assert_eq!(saved.active_style_pack_id, "local.light-cleanup");
        assert_eq!(saved.microphone_device_name, "External Mic");

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn in_memory_store_refuses_implicit_persistence() {
        let store = PreferencesStore::in_memory();
        let error = store.set(UserPreferences::default()).unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Persistence);
    }
}
