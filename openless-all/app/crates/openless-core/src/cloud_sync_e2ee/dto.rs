//! Public, value-free sync status. Secret document types never cross IPC.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const CONSENT_VERSION: &str = "encrypted-full-snapshot-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    SignedOut,
    SignedIn,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    Locked,
    Unlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Disabled,
    SignInRequired,
    UnlockRequired,
    Ready,
    Pending,
    Syncing,
    Conflict,
    Failed,
    OutcomeUnknown,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncAccount {
    pub github_id: String,
    pub login: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedSyncSignIn {
    pub authorization_session_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_at: String,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EncryptedSyncSignInResult {
    Pending { slow_down: bool },
    SignedIn { account: SyncAccount },
    Denied,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFailure {
    pub code: String,
    pub retry_after_seconds: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedSyncStatus {
    pub sequence: String,
    pub enabled: bool,
    pub auth_state: AuthState,
    pub key_state: KeyState,
    pub sync_state: SyncState,
    pub account: Option<SyncAccount>,
    pub vault_id: Option<String>,
    pub key_id: Option<String>,
    pub local_generation: String,
    pub last_synced_local_generation: Option<String>,
    pub remote_revision: Option<String>,
    pub last_successful_sync_at: Option<String>,
    pub pending_operation_id: Option<String>,
    pub last_error: Option<SyncFailure>,
    pub recovery_required: bool,
    pub has_cloud_snapshot: Option<bool>,
    pub task_id: Option<String>,
    pub service_origin: String,
    pub consent_version: Option<String>,
    pub backup_retention_days: Option<u32>,
}

impl EncryptedSyncStatus {
    pub(crate) fn initial(origin: String) -> Self {
        Self {
            sequence: "0".into(),
            enabled: false,
            auth_state: AuthState::SignedOut,
            key_state: KeyState::Locked,
            sync_state: SyncState::Disabled,
            account: None,
            vault_id: None,
            key_id: None,
            local_generation: "0".into(),
            last_synced_local_generation: None,
            remote_revision: None,
            last_successful_sync_at: None,
            pending_operation_id: None,
            last_error: None,
            recovery_required: false,
            has_cloud_snapshot: None,
            task_id: None,
            service_origin: origin,
            consent_version: None,
            backup_retention_days: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnableStep {
    Create,
    Unlock,
    RestoreReview,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnablePreparation {
    pub next_step: EnableStep,
    pub status: EncryptedSyncStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreMode {
    Replace,
    Merge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncConflictChoice {
    pub id: String,
    pub side: ConflictSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSide {
    Local,
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConflictItem {
    pub id: String,
    pub kind: String,
    /// A fixed reason code, never a value, path, history excerpt, or API key.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub preview_id: String,
    pub unconfirmed_operation_id: Option<String>,
    pub observed_revision: String,
    pub local_generation: String,
    pub counts: BTreeMap<String, usize>,
    pub device_settings_to_review: Vec<String>,
    pub conflicts: Vec<SyncConflictItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedSyncEvent {
    pub sequence: String,
    pub account_id: Option<String>,
    pub vault_id: Option<String>,
    pub task_id: Option<String>,
    pub status: EncryptedSyncStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedSyncConflictEvent {
    pub sequence: String,
    pub account_id: String,
    pub vault_id: String,
    pub task_id: Option<String>,
    pub preview: RestorePreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedSyncRestoreEvent {
    pub sequence: String,
    pub account_id: String,
    pub vault_id: String,
    pub task_id: Option<String>,
    pub local_generation: String,
    pub ui_preferences: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedUiPreferencesSnapshot {
    pub preferences: Option<crate::CloudSyncUiPreferences>,
    pub revision: Option<String>,
}
