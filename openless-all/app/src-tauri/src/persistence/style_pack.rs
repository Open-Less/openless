#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Tauri path adapter for the framework-independent style-pack store.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};

use super::{data_dir, ensure_dir, PreferencesStore};
use crate::types::StylePack;

pub use openless_core::style_pack_store::{
    enabled_modes_from_style_packs, sync_style_pack_preferences,
};

/// Preserves the existing Tauri-facing API while delegating style-pack
/// lifecycle, migration, validation, and persistence to `openless-core`.
pub struct StylePackStore {
    inner: Arc<openless_core::StylePackStore>,
}

impl StylePackStore {
    pub fn new(preferences: &PreferencesStore) -> Result<Self> {
        let directory = data_dir()?;
        ensure_dir(&directory)?;

        let mut preference_snapshot = preferences.get();
        let inner = openless_core::StylePackStore::at_data_dir_with_preferences(
            &directory,
            &preference_snapshot,
        )
        .context("open style pack store")?;
        let packs = inner
            .list()
            .context("list style packs after opening store")?;
        if sync_style_pack_preferences(&mut preference_snapshot, &packs) {
            preferences.set(preference_snapshot)?;
        }

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Memory-only fallback used when the platform data directory is unavailable.
    pub(crate) fn new_fallback() -> Self {
        Self {
            inner: Arc::new(openless_core::StylePackStore::in_memory()),
        }
    }

    pub(crate) fn core(&self) -> Arc<openless_core::StylePackStore> {
        Arc::clone(&self.inner)
    }

    pub fn list(&self) -> Result<Vec<StylePack>> {
        self.inner.list().context("list style packs")
    }

    pub fn list_with_active(&self, active_style_pack_id: &str) -> Result<Vec<StylePack>> {
        self.inner
            .list_with_active(active_style_pack_id)
            .context("list style packs with active marker")
    }

    pub fn get(&self, id: &str) -> Result<StylePack> {
        self.inner.get(id).context("get style pack")
    }

    pub fn get_or_default_active(&self, active_style_pack_id: &str) -> Result<StylePack> {
        self.inner
            .get_or_default_active(active_style_pack_id)
            .context("get active style pack")
    }

    pub fn create_from_template(&self, template: StylePack) -> Result<StylePack> {
        self.inner.create(template).context("create style pack")
    }

    pub fn upsert(&self, style_pack: StylePack) -> Result<StylePack> {
        self.inner.update(style_pack).context("update style pack")
    }

    pub fn set_origin(
        &self,
        id: &str,
        origin_pack_id: Option<String>,
        origin_author_login: Option<String>,
    ) -> Result<StylePack> {
        self.inner
            .set_origin(id, origin_pack_id, origin_author_login)
            .context("set style pack origin")
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<StylePack> {
        self.inner
            .set_enabled(id, enabled)
            .context("set style pack enabled state")
    }

    pub fn reset_builtin(&self, id: &str) -> Result<StylePack> {
        self.inner
            .reset_builtin(id)
            .context("reset builtin style pack")
    }

    pub fn remove_imported(&self, id: &str) -> Result<()> {
        self.inner
            .remove_imported(id)
            .context("remove imported style pack")
    }

    pub fn import_from_zip(&self, zip_path: &Path) -> Result<StylePack> {
        self.inner
            .import_from_zip(zip_path)
            .context("import style pack archive")
    }

    pub fn import_from_zip_bytes(&self, bytes: &[u8], source: &str) -> Result<StylePack> {
        self.inner
            .import_from_zip_bytes(bytes)
            .with_context(|| format!("import style pack archive from {source}"))
    }

    pub fn export_zip_bytes(&self, id: &str) -> Result<Vec<u8>> {
        self.inner
            .export_zip_bytes(id)
            .context("export style pack archive bytes")
    }

    pub fn export_to_zip(&self, id: &str, target_path: &Path) -> Result<()> {
        self.inner
            .export_to_zip(id, target_path)
            .context("export style pack archive")
    }
}
