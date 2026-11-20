use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AskEntryIntentMode {
    SemanticHybrid,
    #[default]
    SeparateModel,
    MainModelTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AskEntrySettings {
    pub(crate) auto_voice_on_open: bool,
    pub(crate) intent_mode: AskEntryIntentMode,
}

impl Default for AskEntrySettings {
    fn default() -> Self {
        Self {
            auto_voice_on_open: true,
            intent_mode: AskEntryIntentMode::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(remote = "AskEntrySettings", rename_all = "camelCase")]
struct StoredSettings {
    #[serde(default = "default_auto_voice_on_open")]
    auto_voice_on_open: bool,
    #[serde(default)]
    intent_mode: AskEntryIntentMode,
}

fn default_auto_voice_on_open() -> bool {
    AskEntrySettings::default().auto_voice_on_open
}

fn decode_stored(bytes: &[u8]) -> serde_json::Result<AskEntrySettings> {
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let settings = StoredSettings::deserialize(&mut decoder)?;
    decoder.end()?;
    Ok(settings)
}

fn persist(path: &Path, settings: AskEntrySettings) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent =
        parent.ok_or_else(|| anyhow::anyhow!("Ask entry settings directory is missing"))?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec(&settings)?)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

pub(crate) struct AskEntrySettingsStore {
    path: Option<PathBuf>,
    committed: RwLock<Result<AskEntrySettings, String>>,
    writes: Mutex<()>,
}

impl AskEntrySettingsStore {
    pub(crate) const FILE_NAME: &str = "ask-entry-settings.json";

    pub(crate) fn load(path: Option<PathBuf>) -> Self {
        let committed = match path.as_deref() {
            Some(path) => match fs::read(path) {
                Ok(bytes) => decode_stored(&bytes)
                    .map_err(|error| format!("Invalid Ask entry settings: {error}")),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    Ok(AskEntrySettings::default())
                }
                Err(error) => Err(format!("Could not read Ask entry settings: {error}")),
            },
            None => Err("Ask entry settings storage is unavailable.".into()),
        };

        Self {
            path,
            committed: RwLock::new(committed),
            writes: Mutex::new(()),
        }
    }

    pub(crate) fn read(&self) -> Result<AskEntrySettings, String> {
        self.committed.read().clone()
    }

    pub(crate) fn write(&self, settings: AskEntrySettings) -> Result<AskEntrySettings, String> {
        let _writer = self.writes.lock();
        let path = self
            .path
            .as_deref()
            .ok_or("Ask entry settings storage is unavailable.")?;
        persist(path, settings)
            .map_err(|error| format!("Could not save Ask entry settings: {error}"))?;
        *self.committed.write() = Ok(settings);
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_and_missing_keys_use_the_canonical_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(AskEntrySettingsStore::FILE_NAME);
        assert_eq!(
            AskEntrySettingsStore::load(Some(path.clone()))
                .read()
                .unwrap(),
            AskEntrySettings {
                auto_voice_on_open: true,
                intent_mode: AskEntryIntentMode::SeparateModel,
            }
        );
        fs::write(&path, br#"{"autoVoiceOnOpen":false}"#).unwrap();
        assert_eq!(
            AskEntrySettingsStore::load(Some(path.clone()))
                .read()
                .unwrap(),
            AskEntrySettings {
                auto_voice_on_open: false,
                intent_mode: AskEntryIntentMode::SeparateModel,
            }
        );
        fs::write(&path, br#"{"intentMode":"mainModelTool"}"#).unwrap();
        assert_eq!(
            AskEntrySettingsStore::load(Some(path)).read().unwrap(),
            AskEntrySettings {
                auto_voice_on_open: true,
                intent_mode: AskEntryIntentMode::MainModelTool,
            }
        );
        assert!(serde_json::from_str::<AskEntrySettings>(r#"{"autoVoiceOnOpen":false}"#).is_err());
    }

    #[test]
    fn writes_replace_the_full_value_and_reload_with_the_canonical_wire_names() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(AskEntrySettingsStore::FILE_NAME);
        let store = AskEntrySettingsStore::load(Some(path.clone()));
        for (settings, mode) in [
            (
                AskEntrySettings {
                    intent_mode: AskEntryIntentMode::SemanticHybrid,
                    ..AskEntrySettings::default()
                },
                "semanticHybrid",
            ),
            (
                AskEntrySettings {
                    auto_voice_on_open: false,
                    intent_mode: AskEntryIntentMode::SeparateModel,
                },
                "separateModel",
            ),
            (
                AskEntrySettings {
                    auto_voice_on_open: true,
                    intent_mode: AskEntryIntentMode::MainModelTool,
                },
                "mainModelTool",
            ),
        ] {
            assert_eq!(store.write(settings).unwrap(), settings);
            assert_eq!(store.read().unwrap(), settings);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
                serde_json::json!({"autoVoiceOnOpen": settings.auto_voice_on_open, "intentMode": mode})
            );
            assert_eq!(
                AskEntrySettingsStore::load(Some(path.clone()))
                    .read()
                    .unwrap(),
                settings
            );
        }
    }

    #[test]
    fn failures_do_not_publish_uncommitted_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(AskEntrySettingsStore::FILE_NAME);
        fs::write(&path, br#"{"intentMode":"unknown"}"#).unwrap();
        assert!(AskEntrySettingsStore::load(Some(path.clone()))
            .read()
            .is_err());
        fs::remove_file(&path).unwrap();
        let store = AskEntrySettingsStore::load(Some(path.clone()));
        fs::create_dir(&path).unwrap();
        assert!(store
            .write(AskEntrySettings {
                auto_voice_on_open: false,
                ..AskEntrySettings::default()
            })
            .is_err());
        assert_eq!(store.read().unwrap(), AskEntrySettings::default());
    }

    #[test]
    fn overlapping_writes_leave_disk_and_memory_at_the_same_commit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(AskEntrySettingsStore::FILE_NAME);
        let store = AskEntrySettingsStore::load(Some(path.clone()));
        let barrier = std::sync::Barrier::new(2);
        let settings = [
            AskEntrySettings {
                auto_voice_on_open: false,
                intent_mode: AskEntryIntentMode::SeparateModel,
            },
            AskEntrySettings {
                auto_voice_on_open: true,
                intent_mode: AskEntryIntentMode::MainModelTool,
            },
        ];
        std::thread::scope(|scope| {
            for value in settings {
                let (store, barrier) = (&store, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    assert_eq!(store.write(value).unwrap(), value);
                });
            }
        });
        let committed = store.read().unwrap();
        assert!(settings.contains(&committed));
        assert_eq!(
            AskEntrySettingsStore::load(Some(path)).read().unwrap(),
            committed
        );
    }
}
