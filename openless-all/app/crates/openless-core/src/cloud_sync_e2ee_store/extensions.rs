//! Encrypted extension persistence supplied by the service's OS-key-backed LocalStorage.

use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::cloud_sync_e2ee_documents::{DocumentResult, JournalProtector, SecretJson, SyncScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceExtensionKey {
    /// Actual locale/font scale mirrored from the restricted native UI preferences bridge.
    UiPreferences,
    /// Persisted window positions, if the Host has any; never arbitrary window-state file paths.
    WindowPositions,
    /// Explicit registered restore scopes. Recovery does not scan arbitrary files.
    RecoveryPointers,
}

impl DeviceExtensionKey {
    pub fn storage_name(self) -> &'static str {
        match self {
            Self::UiPreferences => "sync-ui-preferences",
            Self::WindowPositions => "sync-window-positions",
            Self::RecoveryPointers => "sync-recovery-pointers",
        }
    }
}

pub trait ProtectedExtensionStore: Send + Sync {
    /// Scope storage is keyed by origin/owner/vault/device, not a mutable global current account.
    /// Cloud key rotation must not strand OS-key-protected local extensions.
    fn read_scope(&self, scope: SyncScope) -> BoxFuture<'_, DocumentResult<Option<SecretJson>>>;
    fn write_scope(&self, scope: SyncScope, value: SecretJson)
        -> BoxFuture<'_, DocumentResult<()>>;
    fn read_device(
        &self,
        key: DeviceExtensionKey,
    ) -> BoxFuture<'_, DocumentResult<Option<SecretJson>>>;
    fn write_device(
        &self,
        key: DeviceExtensionKey,
        value: SecretJson,
    ) -> BoxFuture<'_, DocumentResult<()>>;
    /// A local-only change token, rotated on every restore write even when values repeat.
    fn read_ui_revision(&self) -> BoxFuture<'_, DocumentResult<Option<String>>> {
        Box::pin(async { Err(crate::cloud_sync_e2ee_documents::DocumentError::Unsupported) })
    }
    /// Returns a real local AEAD protector after OS secure storage authorizes its key.
    fn journal_protector(&self) -> BoxFuture<'_, DocumentResult<Arc<dyn JournalProtector>>>;
}
