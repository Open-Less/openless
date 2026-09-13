#![cfg_attr(target_os = "linux", allow(dead_code))]
//! Tauri path adapter for the framework-independent preferences store.

use anyhow::{Context, Result};
use std::sync::Arc;

use super::{data_dir, ensure_dir, fallback_store_path, PREFERENCES_FILE};
use crate::types::UserPreferences;

pub struct PreferencesStore {
    inner: Arc<openless_core::PreferencesStore>,
}

impl PreferencesStore {
    pub fn new() -> Result<Self> {
        let directory = data_dir()?;
        ensure_dir(&directory)?;
        let inner = openless_core::PreferencesStore::open(directory.join(PREFERENCES_FILE))
            .context("open preferences store")?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub(crate) fn new_fallback() -> Self {
        Self {
            inner: Arc::new(openless_core::PreferencesStore::fallback(
                fallback_store_path("openless_prefs_fallback.json"),
            )),
        }
    }

    pub(crate) fn core(&self) -> Arc<openless_core::PreferencesStore> {
        Arc::clone(&self.inner)
    }

    pub fn get(&self) -> UserPreferences {
        self.inner.get()
    }

    pub fn set(&self, preferences: UserPreferences) -> Result<()> {
        self.inner.set(preferences).context("save preferences")
    }

    pub fn set_preserving_current_style_preferences(
        &self,
        preferences: UserPreferences,
    ) -> Result<()> {
        self.inner
            .set_preserving_current_style_preferences(preferences)
            .context("save preferences while preserving style fields")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tauri_wrapper_and_backend_share_the_same_preferences_repository() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-shared-preferences-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let repositories = openless_core::BackendRepositories::open(&data_dir).unwrap();
        let wrapper = PreferencesStore {
            inner: Arc::clone(&repositories.preferences),
        };
        let backend = openless_core::OpenLessBackend::new_with_repositories(
            openless_core::BackendConfig {
                data_dir: data_dir.clone(),
                ..openless_core::BackendConfig::default()
            },
            openless_core::BackendDependencies::unsupported(),
            repositories,
        )
        .unwrap();

        let mut preferences = backend.get_preferences();
        preferences.microphone_device_name = "shared repository".to_string();
        crate::set_backend_preferences_for_test(&backend, preferences);

        assert_eq!(
            wrapper.get().microphone_device_name,
            "shared repository",
            "the compatibility wrapper must observe core writes without reopening the JSON store"
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
