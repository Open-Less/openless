#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Tauri path adapter for the shared dictation history repository.

use anyhow::Result;
use std::sync::Arc;

use super::{data_dir, ensure_dir};
use crate::types::DictationSession;

pub struct HistoryStore {
    inner: Arc<openless_core::HistoryStore>,
}

impl HistoryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            inner: Arc::new(openless_core::HistoryStore::at_data_dir(dir)),
        })
    }

    pub(crate) fn new_fallback() -> Self {
        Self {
            inner: Arc::new(openless_core::HistoryStore::at_path(
                super::fallback_store_path("openless_history_fallback.json"),
            )),
        }
    }

    pub(crate) fn core(&self) -> Arc<openless_core::HistoryStore> {
        Arc::clone(&self.inner)
    }

    pub fn list(&self) -> Result<Vec<DictationSession>> {
        self.inner.list().map_err(anyhow::Error::new)
    }

    pub fn append_with_retention(
        &self,
        session: DictationSession,
        retention_days: u32,
        max_entries: Option<u32>,
    ) -> Result<()> {
        self.inner
            .append_with_retention(session, retention_days, max_entries)
            .map_err(anyhow::Error::new)
    }

    pub fn recent_within_minutes(&self, minutes: u32) -> Result<Vec<DictationSession>> {
        self.inner
            .recent_within_minutes(minutes)
            .map_err(anyhow::Error::new)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.inner.delete(id).map_err(anyhow::Error::new)
    }

    pub fn update_entry(&self, updated: DictationSession) -> Result<bool> {
        self.inner.update_entry(updated).map_err(anyhow::Error::new)
    }

    pub fn clear(&self) -> Result<()> {
        self.inner.clear().map_err(anyhow::Error::new)
    }
}
