#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Tauri path adapter for the shared vocabulary repository.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use super::{data_dir, ensure_dir};
use crate::types::{DictionaryEntry, VocabPresetStore};

pub struct DictionaryStore {
    inner: Arc<openless_core::DictionaryStore>,
}

impl DictionaryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            inner: Arc::new(openless_core::DictionaryStore::at_data_dir(dir)),
        })
    }

    fn new_at(path: PathBuf) -> Self {
        Self {
            inner: Arc::new(openless_core::DictionaryStore::at_path(path)),
        }
    }

    pub(crate) fn core(&self) -> Arc<openless_core::DictionaryStore> {
        Arc::clone(&self.inner)
    }

    pub(crate) fn new_fallback() -> Self {
        Self::new_at(super::fallback_store_path("openless_vocab_fallback.json"))
    }

    pub fn list(&self) -> Result<Vec<DictionaryEntry>> {
        self.inner.list().map_err(anyhow::Error::new)
    }

    pub fn add(&self, phrase: String, note: Option<String>) -> Result<DictionaryEntry> {
        self.inner.add(phrase, note).map_err(anyhow::Error::new)
    }

    pub fn add_if_absent(
        &self,
        phrase: String,
        note: Option<String>,
    ) -> Result<Option<DictionaryEntry>> {
        self.inner
            .add_if_absent(phrase, note)
            .map_err(anyhow::Error::new)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        self.inner.remove(id).map_err(anyhow::Error::new)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        self.inner
            .set_enabled(id, enabled)
            .map_err(anyhow::Error::new)
    }

    pub fn record_hits(&self, text: &str) -> Result<u64> {
        self.inner.record_hits(text).map_err(anyhow::Error::new)
    }
}

pub fn list_vocab_presets() -> Result<VocabPresetStore> {
    let dir = data_dir()?;
    ensure_dir(&dir)?;
    openless_core::list_vocab_presets(&dir).map_err(anyhow::Error::new)
}

pub fn save_vocab_presets(store: &VocabPresetStore) -> Result<()> {
    let dir = data_dir()?;
    ensure_dir(&dir)?;
    openless_core::save_vocab_presets(&dir, store).map_err(anyhow::Error::new)
}
