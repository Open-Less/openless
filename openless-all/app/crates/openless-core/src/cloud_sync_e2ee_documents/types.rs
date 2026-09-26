//! Logical, secret-bearing data boundaries. None of these snapshots is a UI DTO.

use std::collections::BTreeMap;
use std::fmt;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroize;

use crate::cloud_sync_e2ee_protocol::types::{
    DocumentKind, DocumentSet, LogicalDocument, Revision, SourceDevice, Tombstone,
};

pub const DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_JSON_BYTES: usize = 15 * 1024 * 1024;
pub const MAX_DOCUMENTS: usize = 100_000;
pub const MAX_DEPTH: usize = 64;
pub const MAX_ICON_BYTES: usize = 64 * 1024;
pub const MAX_JOURNAL_BYTES: usize = 33 * 1024 * 1024;

/// Stable, value-free errors. Never include serde, provider, path, or secret error bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum DocumentError {
    #[error("sync_documents_unsupported")]
    Unsupported,
    #[error("sync_documents_invalid")]
    InvalidDocument,
    #[error("sync_documents_invalid_reference")]
    InvalidReference,
    #[error("payload_too_large")]
    PayloadTooLarge,
    #[error("sync_documents_duplicate_id")]
    DuplicateId,
    #[error("sync_documents_excluded_field")]
    ExcludedField,
    #[error("sync_documents_capture_failed")]
    CaptureFailed,
    #[error("sync_documents_source_changed")]
    SourceChanged,
    #[error("runtime_busy")]
    RuntimeBusy,
    #[error("sync_documents_missing_tombstone")]
    MissingTombstone,
    #[error("sync_conflict_choice_required")]
    ConflictChoiceRequired,
    #[error("stale_preview")]
    StalePreview,
    #[error("sync_journal_unavailable")]
    JournalUnavailable,
    #[error("sync_documents_locked")]
    Locked,
    #[error("sync_restore_rolled_back")]
    RestoreRolledBack,
    #[error("recovery_required")]
    RecoveryRequired,
}

pub type DocumentResult<T> = Result<T, DocumentError>;

/// JSON may contain API keys or private history. Diagnostics expose neither keys nor values.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretJson(Value);

impl SecretJson {
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &Value {
        &self.0
    }

    pub fn expose_mut(&mut self) -> &mut Value {
        &mut self.0
    }

    pub fn into_value(mut self) -> Value {
        std::mem::replace(&mut self.0, Value::Null)
    }

    pub fn from_serializable(value: &impl Serialize) -> DocumentResult<Self> {
        serde_json::to_value(value)
            .map(Self)
            .map_err(|_| DocumentError::InvalidDocument)
    }
}

impl Default for SecretJson {
    fn default() -> Self {
        Self(Value::Object(Default::default()))
    }
}

impl fmt::Debug for SecretJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretJson([REDACTED])")
    }
}

impl Drop for SecretJson {
    fn drop(&mut self) {
        erase_json(&mut self.0);
    }
}

pub(crate) fn erase_json(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(values) => values.iter_mut().for_each(erase_json),
        Value::Object(values) => values.values_mut().for_each(erase_json),
        _ => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncNamespace {
    Asr,
    Llm,
    Omni,
}

impl SyncNamespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asr => "asr",
            Self::Llm => "llm",
            Self::Omni => "omni",
        }
    }
}

/// Local readiness/test results and OAuth login state are intentionally absent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelRecord {
    pub id: String,
    pub namespace: SyncNamespace,
    pub provider_type: String,
    pub name: String,
    pub enabled: bool,
    pub order: u32,
    pub active: bool,
}

impl ChannelRecord {
    pub fn document_id(&self) -> String {
        format!("{}:{}", self.namespace.as_str(), self.id)
    }
}

/// Exactly one controlled account map per channel, including an empty map.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCredentialRecord {
    pub channel_id: String,
    pub namespace: SyncNamespace,
    pub accounts: BTreeMap<String, String>,
}

impl ProviderCredentialRecord {
    pub fn document_id(&self) -> String {
        format!("{}:{}", self.namespace.as_str(), self.channel_id)
    }
}

impl Drop for ProviderCredentialRecord {
    fn drop(&mut self) {
        self.accounts.values_mut().for_each(Zeroize::zeroize);
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IconAsset {
    pub mime: String,
    pub base64: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StylePackRecord {
    /// Full StylePack serialization; export removes iconPath and derived active.
    pub pack: SecretJson,
    pub icon: Option<IconAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetOrigin {
    Custom,
    Override,
    BuiltinState,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VocabularyPresetRecord {
    pub id: String,
    pub origin: PresetOrigin,
    pub name: Option<String>,
    pub phrases: Vec<String>,
    pub enabled: bool,
    #[serde(default)]
    pub order: u32,
}

/// One source device/day contribution. Never export an imported aggregate as a new local one.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityRecord {
    pub source_device_id: String,
    pub date: String,
    pub count: u64,
    pub chars: u64,
    pub duration_ms: u64,
}

impl ActivityRecord {
    pub fn document_id(&self) -> String {
        format!("{}:{}", self.source_device_id, self.date)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowPosition {
    pub window_id: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// These are data to preserve/review, never executable paths or permission grants.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceProfileRecord {
    pub device: SourceDevice,
    pub preferences: SecretJson,
    pub channels: Vec<ChannelRecord>,
    pub provider_credentials: Vec<ProviderCredentialRecord>,
    pub window_positions: Vec<WindowPosition>,
}

/// A coherent capture under the Host's write barrier. No directory/keystore enumeration.
#[derive(Clone)]
pub struct ExportSnapshot {
    pub source_device: SourceDevice,
    pub exported_at: String,
    pub generation: Revision,
    pub base_revision: Revision,
    /// Raw preferences plus preserved unknown keys, before UserPreferences drops unknown data.
    pub preferences: SecretJson,
    pub ui_preferences: SecretJson,
    pub channels: Vec<ChannelRecord>,
    pub provider_credentials: Vec<ProviderCredentialRecord>,
    pub dictionary: Vec<SecretJson>,
    pub vocabulary_presets: Vec<VocabularyPresetRecord>,
    pub corrections: Vec<SecretJson>,
    pub style_packs: Vec<StylePackRecord>,
    pub history: Vec<SecretJson>,
    pub activity: Vec<ActivityRecord>,
    pub window_positions: Vec<WindowPosition>,
    /// Other-device profiles and unknown preference keys from protected local extension storage.
    pub retained_documents: Vec<LogicalDocument>,
    pub tombstones: Vec<Tombstone>,
}

pub trait SyncDocumentSource: Send + Sync {
    /// Capture all registered stores/credentials at one persisted generation.
    /// Missing capabilities and failed credential reads must return an error, never an empty set.
    fn capture(&self) -> BoxFuture<'_, DocumentResult<ExportSnapshot>>;
}

#[derive(Clone)]
pub struct ValidatedSyncDocuments {
    pub(crate) set: DocumentSet,
    pub(crate) revision: Revision,
}

impl ValidatedSyncDocuments {
    pub fn documents(&self) -> &DocumentSet {
        &self.set
    }

    pub fn observed_revision(&self) -> Revision {
        self.revision
    }
}

impl Drop for ValidatedSyncDocuments {
    fn drop(&mut self) {
        for document in &mut self.set.documents {
            erase_json(&mut document.value);
        }
    }
}

#[derive(Clone)]
pub struct ExportedDocuments {
    pub generation: Revision,
    pub documents: ValidatedSyncDocuments,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentKey {
    pub kind: DocumentKind,
    pub id: String,
}

/// Origin/account/vault/key/device binding for protected local state and restore ownership.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncScope {
    pub service_origin: String,
    pub owner_github_id: String,
    pub vault_id: String,
    pub key_id: String,
    pub device_id: String,
}

#[derive(Clone)]
pub struct RestoreContext {
    pub scope: SyncScope,
    pub operation_id: String,
    pub observed_revision: Revision,
    pub local_generation: Revision,
    pub target_device: SourceDevice,
}

/// Exclusive lease blocks every normal repository/credential mutation until released.
/// After a crash, startup must recover a pending journal before admitting ordinary writes.
pub trait RestoreBackend: Send + Sync {
    fn acquire(
        &self,
        scope: SyncScope,
        expected_generation: Option<Revision>,
    ) -> BoxFuture<'_, DocumentResult<Box<dyn RestoreLease>>>;
}

pub trait RestoreLease: Send {
    fn capture(&mut self) -> BoxFuture<'_, DocumentResult<ExportSnapshot>>;

    /// Local-only side effects never enter cloud documents. The encrypted journal may retain
    /// the minimal receiving-device state needed to roll them back exactly.
    fn capture_rollback_state(&mut self) -> BoxFuture<'_, DocumentResult<SecretJson>> {
        Box::pin(async { Ok(SecretJson::default()) })
    }
    fn restore_rollback_state(&mut self, state: &SecretJson) -> BoxFuture<'_, DocumentResult<()>> {
        let empty = state
            .expose()
            .as_object()
            .is_some_and(|value| value.is_empty());
        Box::pin(async move {
            if empty {
                Ok(())
            } else {
                Err(DocumentError::Unsupported)
            }
        })
    }

    /// Validate native representations before journalling or mutating any local data.
    fn preflight(
        &mut self,
        _documents: &ValidatedSyncDocuments,
    ) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async { Ok(()) })
    }

    /// Replace registered logical data, preserving stable IDs and all history.
    /// Do not import OAuth/permissions, execute device paths, or run retention trimming.
    fn replace(&mut self, documents: ValidatedSyncDocuments) -> BoxFuture<'_, DocumentResult<()>>;

    /// Reload file-backed caches and invalidate credential/test readiness before verification.
    fn reload(&mut self) -> BoxFuture<'_, DocumentResult<()>>;

    /// Must persist the pending marker; dropping a cancelled future cannot admit ordinary writes.
    fn mark_recovery_pending(&mut self, operation_id: &str) -> DocumentResult<()>;

    /// Persist this only after the encrypted journal is durable, before the first data write.
    fn mark_journal_ready(&mut self, operation_id: &str) -> DocumentResult<()>;

    /// Missing journal is safe only for a pending Preparing marker; Applying must fail closed.
    fn recover_without_journal(&mut self) -> DocumentResult<()>;

    /// A durable receipt prevents replaying an old committed journal over newer user changes.
    fn completed_restore(
        &self,
        operation_id: &str,
    ) -> DocumentResult<Option<super::restore::RestoreReceipt>>;

    /// Idempotent for operation_id. Commit advances one Restore-origin generation; rollback does not.
    fn finish_restore(
        &mut self,
        operation_id: &str,
        committed: bool,
    ) -> DocumentResult<super::restore::RestoreReceipt>;
}

macro_rules! redact_debug {
    ($($name:ty),+ $(,)?) => {
        $(impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "([REDACTED])"))
            }
        })+
    };
}

redact_debug!(
    ChannelRecord,
    ProviderCredentialRecord,
    IconAsset,
    StylePackRecord,
    VocabularyPresetRecord,
    ActivityRecord,
    WindowPosition,
    DeviceProfileRecord,
    ExportSnapshot,
    ValidatedSyncDocuments,
    ExportedDocuments,
    SyncScope,
    RestoreContext,
);
