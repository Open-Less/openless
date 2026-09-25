#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Tauri path adapter for the shared correction-rule repository.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use super::{data_dir, ensure_dir};
use crate::types::CorrectionRule;

pub struct CorrectionRuleStore {
    inner: Arc<openless_core::CorrectionRuleStore>,
}

impl CorrectionRuleStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            inner: Arc::new(openless_core::CorrectionRuleStore::at_data_dir(dir)),
        })
    }

    fn new_at(path: PathBuf) -> Self {
        Self {
            inner: Arc::new(openless_core::CorrectionRuleStore::at_path(path)),
        }
    }

    pub(crate) fn core(&self) -> Arc<openless_core::CorrectionRuleStore> {
        Arc::clone(&self.inner)
    }

    pub(crate) fn new_fallback() -> Self {
        Self::new_at(super::fallback_store_path(
            "openless_correction_rules_fallback.json",
        ))
    }

    pub fn list(&self) -> Result<Vec<CorrectionRule>> {
        self.inner.list().map_err(anyhow::Error::new)
    }

    pub fn add(&self, pattern: String, replacement: String) -> Result<CorrectionRule> {
        self.inner
            .add(pattern, replacement)
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
}
