//! Settings-page state for the encrypted cloud sync card.
//!
//! Business truth stays in Core: this module only mirrors
//! `openless_core::cloud_sync_e2ee::EncryptedSyncStatus` into the shape the egui
//! settings page renders, plus the pure view decisions (which step to focus,
//! whether "sync now" is allowed, which stable error code to show). Nothing here
//! reads, stores or logs a secret beyond the password box the user is typing
//! into: the drafts are cleared as soon as an action is dispatched.

use openless_core::cloud_sync_e2ee::{
    AuthState, ConflictSide, EncryptedSyncStatus, KeyState, RestoreMode, RestorePreview, SyncState,
};
use serde::{Deserialize, Serialize};

/// Consent version the host asks Core to record. Must match Core's constant so a
/// version bump forces every client back through the consent form.
pub const CONSENT_VERSION: &str = openless_core::cloud_sync_e2ee::CONSENT_VERSION;

/// What the user asked for before the consent/protocol screens ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloudSyncIntent {
    Enable,
    Unlock,
    Restore,
}

impl CloudSyncIntent {
    pub fn ready_action(self) -> Option<CloudSyncAction> {
        match self {
            Self::Enable => Some(CloudSyncAction::SetEnabled(true)),
            Self::Restore => Some(CloudSyncAction::PreviewRestore),
            Self::Unlock => None,
        }
    }
}

/// The modal the card is currently showing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CloudSyncDialog {
    None,
    Consent {
        intent: CloudSyncIntent,
    },
    Protocol {
        intent: CloudSyncIntent,
    },
    Create {
        observed_revision: String,
    },
    Unlock {
        intent: CloudSyncIntent,
    },
    ChangePassword,
    Restore {
        preview: Box<RestorePreview>,
        enable_after: bool,
    },
    Disable,
    Delete {
        vault_id: String,
        revision: String,
    },
}

impl CloudSyncDialog {
    pub fn is_open(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// One user gesture on the cloud sync card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CloudSyncAction {
    /// Re-read the status from Core.
    Reload,
    /// Flip the master toggle: enabling opens the consent flow, disabling asks
    /// for confirmation.
    Toggle(bool),
    /// Run Core's enable preparation after the consent + protocol screens.
    Prepare(CloudSyncIntent),
    /// Submit the password form for whatever dialog is open.
    SubmitPassword(CloudSyncIntent),
    /// Turn sync on or off once the user confirmed.
    SetEnabled(bool),
    Lock,
    SyncNow,
    Cancel(String),
    PreviewRestore,
    ApplyRestore(RestoreMode),
    DeleteRemote,
    SignOut,
    ConfigureServer,
    OpenDialog(CloudSyncDialog),
    CloseDialog,
    /// Restore-review conflict choice: `None` keeps the item unresolved.
    ResolveConflict(String, Option<ConflictSide>),
    Password(String),
    Confirmation(String),
    CurrentPassword(String),
    RememberKey(bool),
    ConsentChecked(bool),
    CustomOrigin(String),
    CustomToken(String),
}

/// One Core call the host should perform. The host turns this into a single
/// async task so every arm shares one `await` point.
#[derive(Clone, Debug)]
pub enum CloudSyncOp {
    Prepare {
        consent_version: String,
    },
    Create {
        password: String,
        confirmation: String,
        remember_key: bool,
        consent_version: String,
        observed_revision: String,
    },
    Unlock {
        password: String,
        remember_key: bool,
    },
    ChangePassword {
        current_password: String,
        password: String,
        confirmation: String,
        remember_key: bool,
    },
    SetEnabled(bool),
    Lock,
    SyncNow,
    Cancel(String),
    PreviewRestore {
        observed: String,
    },
    ApplyRestore {
        preview_id: String,
        mode: RestoreMode,
        conflicts: Vec<openless_core::cloud_sync_e2ee::SyncConflictChoice>,
    },
    DeleteRemote {
        vault_id: String,
        revision: String,
    },
    SignOut,
    ConfigureServer {
        origin: Option<String>,
        token: String,
    },
}

impl CloudSyncOp {
    /// Run the operation against Core. Passwords are moved in and dropped with
    /// the task; nothing is retained on the UI side.
    pub async fn run(
        backend: std::sync::Arc<openless_core::OpenLessBackend>,
        operation: CloudSyncOp,
    ) -> Result<SyncOutcome, String> {
        use openless_core::cloud_sync_e2ee::AuthState;
        let status = |status: EncryptedSyncStatus| SyncOutcome::Status(Box::new(status));
        match operation {
            Self::Prepare { consent_version } => backend
                .cloud_sync_e2ee_prepare_enable(consent_version)
                .await
                .map(|prepared| SyncOutcome::Prepared(Box::new(prepared)))
                .map_err(|error| error.to_string()),
            Self::Create {
                password,
                confirmation,
                remember_key,
                consent_version,
                observed_revision,
            } => backend
                .cloud_sync_e2ee_create(
                    password,
                    confirmation,
                    remember_key,
                    consent_version,
                    observed_revision,
                )
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            Self::Unlock {
                password,
                remember_key,
            } => backend
                .cloud_sync_e2ee_unlock(password, remember_key)
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            Self::ChangePassword {
                current_password,
                password,
                confirmation,
                remember_key,
            } => backend
                .cloud_sync_e2ee_change_password(
                    current_password,
                    password,
                    confirmation,
                    remember_key,
                )
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            Self::SetEnabled(enabled) => backend
                .cloud_sync_e2ee_set_enabled(enabled)
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            Self::Lock => backend
                .cloud_sync_e2ee_lock()
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            Self::SyncNow => backend
                .cloud_sync_e2ee_sync_now()
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            // `cancel` is synchronous in Core: it only marks the task.
            Self::Cancel(task) => backend
                .cloud_sync_e2ee_cancel(task)
                .map(status)
                .map_err(|error| error.to_string()),
            Self::PreviewRestore { observed } => backend
                .cloud_sync_e2ee_preview_restore(observed)
                .await
                .map(|preview| SyncOutcome::Preview(Box::new(preview)))
                .map_err(|error| error.to_string()),
            Self::ApplyRestore {
                preview_id,
                mode,
                conflicts,
            } => backend
                .cloud_sync_e2ee_apply_restore(preview_id, mode, conflicts)
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            Self::DeleteRemote { vault_id, revision } => backend
                .cloud_sync_e2ee_delete_remote(vault_id, revision, true)
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            Self::SignOut => backend
                .cloud_sync_e2ee_sign_out()
                .await
                .map(status)
                .map_err(|error| error.to_string()),
            Self::ConfigureServer { origin, token } => {
                let mut status = backend
                    .cloud_sync_e2ee_configure_server(origin, token)
                    .await
                    .map_err(|error| error.to_string())?;
                // 自定义服务器自带会话：配完立刻用刚存的 token 登录。
                if status.auth_state == AuthState::SignedOut {
                    if let Ok(signed_in) = backend.cloud_sync_e2ee_sign_in_with_token().await {
                        status = signed_in;
                    }
                }
                Ok(SyncOutcome::Status(Box::new(status)))
            }
        }
    }
}

/// What a finished Core call hands back to the host.
#[derive(Clone, Debug)]
pub enum SyncOutcome {
    Status(Box<EncryptedSyncStatus>),
    Prepared(Box<openless_core::cloud_sync_e2ee::EnablePreparation>),
    Preview(Box<RestorePreview>),
    /// `sign_out_account` returns nothing; the host reloads the status itself.
    Done,
}

/// Which control the card highlights when nothing is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudSyncFocus {
    Enable,
    Unlock,
    Restore,
    State,
}

/// Stable, allow-listed failure codes the card may render. Everything else
/// collapses to `unknown` so a Core message (which may quote a path) never
/// reaches the screen.
pub const KNOWN_ERROR_CODES: &[&str] = &[
    "busy",
    "conflict",
    "denied",
    "expired",
    "invalid_response",
    "invalidPassword",
    "locked",
    "network",
    "not_signed_in",
    "outcomeUnknown",
    "rate_limited",
    "recovery_required",
    "secureStorage",
    "server",
    "service_unavailable",
    "stale_preview",
    "unlock",
    "unknown",
    "unsupported",
];

/// Map a Core/local failure string onto the allow-listed code set.
pub fn error_code(raw: &str) -> &'static str {
    let trimmed = raw.trim();
    for candidate in KNOWN_ERROR_CODES {
        if candidate.eq_ignore_ascii_case(trimmed) {
            return candidate;
        }
    }
    // Core 用 snake_case，catalog 里有几个用 camelCase。
    match trimmed {
        "invalid_password" | "wrong_password" => "invalidPassword",
        "password_mismatch" => "passwordMismatch",
        "weak_password" => "weakPassword",
        "too_large" => "tooLarge",
        "invalid_data" => "invalidData",
        "review_required" => "reviewRequired",
        "choices_required" => "choicesRequired",
        "not_signed_in" => "not_signed_in",
        "recovery_required" => "recovery_required",
        "outcome_unknown" => "outcomeUnknown",
        "rate_limited" => "rate_limited",
        "service_unavailable" => "service_unavailable",
        "invalid_response" => "invalid_response",
        "stale_preview" => "stale_preview",
        "secure_storage" => "secureStorage",
        _ => "unknown",
    }
}

/// 错误代码 → catalog 键。
///
/// 之前界面用 `format!("cloudSyncE2ee.errors.{code}")` 拼键，而 catalog 里根本
/// 没有这批条目，用户看到的是裸键（`cloudSyncE2ee.errors.service_unavailable`）。
/// 这里把**每个代码的完整键**写成字面量：既能被 i18n 校对脚本看到，也不会再拼
/// 出一个不存在的键。Core 侧特有的代码映射到语义最接近的文案。
pub fn error_key(raw: &str) -> &'static str {
    // 直接按原始代码匹配：`error_code()` 会把「不在 allow-list 里」的 Core 代码
    // 折叠成 `unknown`，而这些代码恰恰最需要自己的文案。
    match raw.trim() {
        // ── Core（snake_case）─────────────────────────────────────────────
        "account_changed" => "cloudSyncE2ee.errors.accountChanged",
        "busy" => "cloudSyncE2ee.errors.busy",
        "cancelled" => "cloudSyncE2ee.errors.cancelled",
        "choices_required" => "cloudSyncE2ee.errors.choicesRequired",
        "cloud_deleted" => "cloudSyncE2ee.errors.cloudDeleted",
        "conflict" | "revision_conflict" => "cloudSyncE2ee.errors.conflict",
        "consent_required" => "cloudSyncE2ee.errors.consentRequired",
        "crypto_worker_failed" => "cloudSyncE2ee.errors.cryptoWorkerFailed",
        "delete_confirmation_required" => "cloudSyncE2ee.errors.deleteConfirmationRequired",
        "denied" => "cloudSyncE2ee.errors.denied",
        "expired" => "cloudSyncE2ee.errors.expired",
        "invalid_data" => "cloudSyncE2ee.errors.invalidData",
        "invalid_password" | "wrong_password" => "cloudSyncE2ee.errors.invalidPassword",
        "invalid_response" => "cloudSyncE2ee.errors.invalidResponse",
        "local_storage_unavailable" => "cloudSyncE2ee.errors.localData",
        "locked" => "cloudSyncE2ee.errors.locked",
        "metadata_required" => "cloudSyncE2ee.errors.metadataRequired",
        "network" => "cloudSyncE2ee.errors.network",
        "not_signed_in" | "sign_in_required" => "cloudSyncE2ee.errors.signIn",
        "outcome_unknown" => "cloudSyncE2ee.errors.outcomeUnknown",
        "password_mismatch" => "cloudSyncE2ee.errors.passwordMismatch",
        "rate_limited" => "cloudSyncE2ee.errors.rateLimited",
        "recovery_required" => "cloudSyncE2ee.errors.recovery",
        "restore_review_required" | "review_required" => "cloudSyncE2ee.errors.reviewRequired",
        "revision_rollback" => "cloudSyncE2ee.errors.rolledBack",
        "secure_random_unavailable" => "cloudSyncE2ee.errors.secureRandomUnavailable",
        "secure_storage" | "secure_storage_denied" => "cloudSyncE2ee.errors.secureStorage",
        "server" => "cloudSyncE2ee.errors.server",
        "service_unavailable" => "cloudSyncE2ee.errors.serviceUnavailable",
        "stale_preview" => "cloudSyncE2ee.errors.changed",
        "stale_task" => "cloudSyncE2ee.errors.staleTask",
        "too_large" => "cloudSyncE2ee.errors.tooLarge",
        "unlock_required" => "cloudSyncE2ee.errors.unlock",
        "unsupported" | "unsupported_protocol" => "cloudSyncE2ee.errors.unsupported",
        "weak_password" => "cloudSyncE2ee.errors.weakPassword",
        // ── Tauri 的 camelCase 键名 ──────────────────────────────────────
        "invalidPassword" => "cloudSyncE2ee.errors.invalidPassword",
        "outcomeUnknown" => "cloudSyncE2ee.errors.outcomeUnknown",
        "rateLimited" => "cloudSyncE2ee.errors.rateLimited",
        "secureStorage" => "cloudSyncE2ee.errors.secureStorage",
        "stalePreview" => "cloudSyncE2ee.errors.changed",
        "unavailable" => "cloudSyncE2ee.errors.serviceUnavailable",
        "unlock" => "cloudSyncE2ee.errors.unlock",
        _ => "cloudSyncE2ee.errors.unknown",
    }
}

/// Status mirror plus the card's transient UI state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudSyncUi {
    pub status: Option<EncryptedSyncStatus>,
    pub loading: bool,
    pub busy: bool,
    pub signed_in: bool,
    pub dialog: CloudSyncDialog,
    /// What the running enable/unlock/restore flow was started for.
    pub intent: Option<CloudSyncIntent>,
    /// 完整 catalog 键（例如 `cloudSyncE2ee.noticeDone`），渲染时直接查表。
    pub notice: Option<String>,
    /// Stable error code while the last gesture failed.
    pub error: Option<String>,
    pub password: String,
    pub confirmation: String,
    pub current_password: String,
    pub remember_key: bool,
    pub consent_checked: bool,
    pub conflicts: Vec<(String, Option<ConflictSide>)>,
    pub custom_origin: String,
    pub custom_token: String,
    /// Origin Core currently syncs against (empty when unset).
    pub saved_origin: String,
}

impl Default for CloudSyncUi {
    fn default() -> Self {
        Self {
            status: None,
            loading: true,
            busy: false,
            signed_in: false,
            dialog: CloudSyncDialog::None,
            intent: None,
            notice: None,
            error: None,
            password: String::new(),
            confirmation: String::new(),
            current_password: String::new(),
            remember_key: true,
            consent_checked: false,
            conflicts: Vec::new(),
            custom_origin: String::new(),
            custom_token: String::new(),
            saved_origin: String::new(),
        }
    }
}

impl CloudSyncUi {
    /// Accept a status snapshot only when it is at least as new as the current
    /// one. Core's `sequence` is a decimal counter, so a lexicographic compare is
    /// wrong the moment it crosses a digit boundary.
    pub fn accept_status(&mut self, next: EncryptedSyncStatus) -> bool {
        if let Some(current) = &self.status {
            match (
                sequence_value(&current.sequence),
                sequence_value(&next.sequence),
            ) {
                (Some(previous), Some(incoming)) if incoming < previous => return false,
                _ => {}
            }
        }
        let scope_changed = self
            .status
            .as_ref()
            .is_some_and(|current| scope_key(current) != scope_key(&next));
        if scope_changed {
            // A different account/vault invalidates whatever dialog was open.
            self.dialog = CloudSyncDialog::None;
            self.conflicts.clear();
            clear_secrets(self);
        }
        // `saved_origin` mirrors the *user-configured* custom server, not the
        // effective origin Core reports, so it is only set by the configure
        // action. Overwriting it here would make every vault look custom.
        self.status = Some(next);
        true
    }

    pub fn apply_error(&mut self, raw: &str) {
        self.error = Some(error_code(raw).to_string());
        self.busy = false;
    }

    /// Drop typed secrets. Called after every dispatched action and whenever the
    /// dialog closes so a password never outlives the form that asked for it.
    pub fn forget_inputs(&mut self) {
        clear_secrets(self);
    }

    /// Clear the password boxes after dispatching, but keep what the user typed
    /// into the custom-server row (it is not a secret and re-typing it is rude).
    pub fn forget_secrets_keep_origin(&mut self) {
        self.password.clear();
        self.confirmation.clear();
        self.current_password.clear();
        self.custom_token.clear();
        self.remember_key = true;
    }

    pub fn open(&mut self, dialog: CloudSyncDialog) {
        clear_secrets(self);
        self.notice = None;
        self.error = None;
        if let CloudSyncDialog::Restore { preview, .. } = &dialog {
            self.conflicts = preview
                .conflicts
                .iter()
                .map(|item| (item.id.clone(), None))
                .collect();
        }
        self.dialog = dialog;
    }

    pub fn close(&mut self) {
        if self.busy {
            return;
        }
        self.dialog = CloudSyncDialog::None;
        self.notice = None;
        self.error = None;
        self.conflicts.clear();
        clear_secrets(self);
    }

    pub fn unlocked(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(|status| status.key_state == KeyState::Unlocked)
    }

    pub fn signed_in_to_sync(&self) -> bool {
        self.status.as_ref().is_some_and(|status| {
            status.auth_state == AuthState::SignedIn
                && status.sync_state != SyncState::SignInRequired
        })
    }

    /// Identities that can start the enable flow: Core's own sync session or the
    /// marketplace login when the user has not pointed sync at a custom server.
    pub fn identity_available(&self) -> bool {
        let custom = !self.saved_origin.trim().is_empty();
        self.status.as_ref().is_some_and(|status| {
            status.auth_state != AuthState::Expired
                && status.sync_state != SyncState::SignInRequired
                && (self.signed_in_to_sync() || (!custom && self.signed_in))
        })
    }

    pub fn working(&self) -> bool {
        self.busy
            || self
                .status
                .as_ref()
                .is_some_and(|status| status.sync_state == SyncState::Syncing)
    }

    /// "Sync now" needs an unlocked key and no pending recovery/conflict.
    pub fn can_sync(&self) -> bool {
        let Some(status) = &self.status else {
            return false;
        };
        !self.working()
            && !status.recovery_required
            && (status.pending_operation_id.is_some()
                || (status.enabled
                    && status.key_state == KeyState::Unlocked
                    && status.sync_state != SyncState::Conflict))
    }

    pub fn focus(&self) -> CloudSyncFocus {
        let Some(status) = &self.status else {
            return CloudSyncFocus::Enable;
        };
        if status.recovery_required
            || matches!(
                status.sync_state,
                SyncState::Conflict
                    | SyncState::Failed
                    | SyncState::OutcomeUnknown
                    | SyncState::RecoveryRequired
                    | SyncState::Pending
                    | SyncState::Syncing
            )
        {
            return CloudSyncFocus::State;
        }
        if !self.identity_available() {
            return CloudSyncFocus::Enable;
        }
        if !status.enabled {
            return CloudSyncFocus::Enable;
        }
        if status.key_state == KeyState::Locked {
            return CloudSyncFocus::Unlock;
        }
        if status.has_cloud_snapshot == Some(false) {
            return CloudSyncFocus::Restore;
        }
        CloudSyncFocus::State
    }

    /// Whether the enable toggle may be flipped right now.
    pub fn toggle_enabled(&self) -> bool {
        !self.loading && !self.working() && self.identity_available()
    }

    /// Title/detail keys for the focus block, resolved against the catalog.
    pub fn focus_keys(&self) -> (&'static str, &'static str) {
        const STEP_ENABLE: (&str, &str) = (
            "cloudSyncE2ee.stepEnableTitle",
            "cloudSyncE2ee.stepEnableDetail",
        );
        const STEP_UNLOCK: (&str, &str) = (
            "cloudSyncE2ee.stepUnlockTitle",
            "cloudSyncE2ee.stepUnlockDetail",
        );
        const STEP_RESTORE: (&str, &str) = (
            "cloudSyncE2ee.stepRestoreTitle",
            "cloudSyncE2ee.stepRestoreDetail",
        );
        match self.focus() {
            CloudSyncFocus::Enable => STEP_ENABLE,
            CloudSyncFocus::Unlock => STEP_UNLOCK,
            CloudSyncFocus::Restore => STEP_RESTORE,
            CloudSyncFocus::State => ("cloudSyncE2ee.stepFollowDetail", ""),
        }
    }

    /// `cloudSyncE2ee.states.*` key for the current sync state (or recovery).
    pub fn state_key(&self) -> &'static str {
        let Some(status) = &self.status else {
            return "cloudSyncE2ee.states.disabled";
        };
        if status.recovery_required {
            return "cloudSyncE2ee.states.recovery_required";
        }
        match status.sync_state {
            SyncState::Disabled => "cloudSyncE2ee.states.disabled",
            SyncState::SignInRequired => "cloudSyncE2ee.states.sign_in_required",
            SyncState::UnlockRequired => "cloudSyncE2ee.states.unlock_required",
            SyncState::Ready => "cloudSyncE2ee.states.ready",
            SyncState::Pending => "cloudSyncE2ee.states.pending",
            SyncState::Syncing => "cloudSyncE2ee.states.syncing",
            SyncState::Conflict => "cloudSyncE2ee.states.conflict",
            SyncState::Failed => "cloudSyncE2ee.states.failed",
            SyncState::OutcomeUnknown => "cloudSyncE2ee.states.outcome_unknown",
            SyncState::RecoveryRequired => "cloudSyncE2ee.states.recovery_required",
        }
    }

    /// Choices handed to Core, in the order the review listed the conflicts.
    pub fn conflict_choices(&self) -> Vec<openless_core::cloud_sync_e2ee::SyncConflictChoice> {
        self.conflicts
            .iter()
            .filter_map(|(id, side)| {
                side.map(|side| openless_core::cloud_sync_e2ee::SyncConflictChoice {
                    id: id.clone(),
                    side,
                })
            })
            .collect()
    }

    pub fn resolve_conflict(&mut self, id: &str, side: Option<ConflictSide>) {
        if let Some(entry) = self.conflicts.iter_mut().find(|(key, _)| key == id) {
            entry.1 = side;
        }
    }

    /// Every conflict needs a side before Core will apply the restore.
    pub fn conflicts_resolved(&self) -> bool {
        self.conflicts.iter().all(|(_, side)| side.is_some())
    }

    /// Custom server origin must be a bare `https://host[:port]` (or plain http
    /// for a local dev server) with no path: Core refuses anything else.
    pub fn normalized_origin(&self) -> Result<Option<String>, &'static str> {
        normalize_origin(&self.custom_origin)
    }
}

fn clear_secrets(state: &mut CloudSyncUi) {
    state.password.clear();
    state.confirmation.clear();
    state.current_password.clear();
    state.custom_token.clear();
    state.remember_key = true;
    state.consent_checked = false;
}

/// Parse a Core sequence counter. Values Core wrote are plain decimal strings;
/// anything else is treated as absent so the caller does not drop a snapshot on a
/// parse failure.
fn sequence_value(raw: &str) -> Option<u128> {
    raw.trim().parse::<u128>().ok()
}

fn scope_key(status: &EncryptedSyncStatus) -> (Option<String>, Option<String>, String) {
    (
        status
            .account
            .as_ref()
            .map(|account| account.github_id.clone()),
        status.vault_id.clone(),
        status.service_origin.clone(),
    )
}

/// `https://host[:port]` with no path, query or credentials. Returns `Ok(None)`
/// for an empty input (meaning "use the built-in server").
pub fn normalize_origin(value: &str) -> Result<Option<String>, &'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let (scheme, rest) = match trimmed.split_once("://") {
        Some(parts) => parts,
        None => return Err("invalid_response"),
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return Err("invalid_response");
    }
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() || rest.contains('/') || rest.contains('?') || rest.contains('#') {
        return Err("invalid_response");
    }
    if rest.contains('@') {
        return Err("invalid_response");
    }
    if scheme == "http" && !is_loopback_host(rest) {
        // Cleartext to a remote host would leak the vault key material.
        return Err("invalid_response");
    }
    Ok(Some(format!("{scheme}://{rest}")))
}

fn is_loopback_host(rest: &str) -> bool {
    let host = rest.split(':').next().unwrap_or_default();
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a status through serde so the fixture cannot drift from Core's DTO.
    fn status(sequence: &str) -> EncryptedSyncStatus {
        serde_json::from_value(serde_json::json!({
            "sequence": sequence,
            "enabled": true,
            "authState": "signed_in",
            "keyState": "unlocked",
            "syncState": "ready",
            "account": { "githubId": "1", "login": "octocat" },
            "vaultId": "vault-1",
            "keyId": "key-1",
            "localGeneration": "3",
            "lastSyncedLocalGeneration": "3",
            "remoteRevision": "3",
            "lastSuccessfulSyncAt": null,
            "pendingOperationId": null,
            "lastError": null,
            "recoveryRequired": false,
            "hasCloudSnapshot": true,
            "taskId": null,
            "serviceOrigin": "https://sync.example.com",
            "consentVersion": CONSENT_VERSION,
            "backupRetentionDays": null
        }))
        .expect("fixture is a valid EncryptedSyncStatus")
    }

    #[test]
    fn a_ready_device_keeps_the_requested_enable_restore_or_unlock_intent() {
        assert_eq!(
            CloudSyncIntent::Enable.ready_action(),
            Some(CloudSyncAction::SetEnabled(true))
        );
        assert_eq!(
            CloudSyncIntent::Restore.ready_action(),
            Some(CloudSyncAction::PreviewRestore)
        );
        assert_eq!(CloudSyncIntent::Unlock.ready_action(), None);
    }

    #[test]
    fn sequences_compare_numerically_not_lexicographically() {
        let mut ui = CloudSyncUi::default();
        assert!(ui.accept_status(status("9")));
        // "10" sorts before "9" as text: a lexicographic compare would drop it.
        assert!(ui.accept_status(status("10")));
        assert!(
            !ui.accept_status(status("9")),
            "an older sequence is dropped"
        );
        assert_eq!(ui.status.as_ref().unwrap().sequence, "10");
    }

    #[test]
    fn a_different_vault_closes_the_open_dialog_and_drops_secrets() {
        let mut ui = CloudSyncUi::default();
        ui.accept_status(status("1"));
        ui.open(CloudSyncDialog::Unlock {
            intent: CloudSyncIntent::Unlock,
        });
        ui.password = "hunter2".into();
        let mut other = status("2");
        other.vault_id = Some("vault-2".into());
        assert!(ui.accept_status(other));
        assert_eq!(ui.dialog, CloudSyncDialog::None);
        assert!(ui.password.is_empty(), "a stale password must not survive");
    }

    #[test]
    fn retry_only_needs_an_unlocked_enabled_vault() {
        let mut ui = CloudSyncUi::default();
        assert!(!ui.can_sync(), "no status yet");
        ui.accept_status(status("1"));
        assert!(ui.can_sync());
        let mut locked = status("2");
        locked.key_state = KeyState::Locked;
        ui.accept_status(locked);
        assert!(!ui.can_sync());
        let mut pending = status("3");
        pending.key_state = KeyState::Locked;
        pending.pending_operation_id = Some("op-1".into());
        ui.accept_status(pending);
        assert!(ui.can_sync(), "a pending operation can always be checked");
    }

    #[test]
    fn recovery_and_conflict_focus_the_state_block() {
        let mut ui = CloudSyncUi::default();
        let mut conflict = status("1");
        conflict.sync_state = SyncState::Conflict;
        ui.accept_status(conflict);
        assert_eq!(ui.focus(), CloudSyncFocus::State);
        assert_eq!(ui.state_key(), "cloudSyncE2ee.states.conflict");

        let mut recovery = status("2");
        recovery.recovery_required = true;
        ui.accept_status(recovery);
        assert_eq!(ui.state_key(), "cloudSyncE2ee.states.recovery_required");
        assert!(!ui.can_sync(), "recovery blocks every sync gesture");
    }

    #[test]
    fn an_unsigned_in_vault_focuses_the_enable_step() {
        let mut ui = CloudSyncUi::default();
        let mut signed_out = status("1");
        signed_out.auth_state = AuthState::SignedOut;
        signed_out.sync_state = SyncState::SignInRequired;
        ui.accept_status(signed_out);
        assert_eq!(ui.focus(), CloudSyncFocus::Enable);
        assert!(!ui.identity_available());
    }

    #[test]
    fn a_custom_server_removes_the_marketplace_identity_fallback() {
        let mut ui = CloudSyncUi::default();
        let mut signed_out = status("1");
        signed_out.auth_state = AuthState::SignedOut;
        signed_out.sync_state = SyncState::Disabled;
        ui.accept_status(signed_out);
        ui.signed_in = true;
        assert!(ui.identity_available(), "marketplace login is enough");
        ui.saved_origin = "https://sync.example.com".into();
        assert!(
            !ui.identity_available(),
            "a custom server needs its own token sign-in"
        );
    }

    #[test]
    fn only_allow_listed_error_codes_reach_the_card() {
        assert_eq!(error_code("invalid_password"), "invalidPassword");
        assert_eq!(error_code("outcome_unknown"), "outcomeUnknown");
        assert_eq!(error_code("  network  "), "network");

        // Anything else (including a path Core quoted) collapses to `unknown`.
        assert_eq!(error_code("/home/user/.local/share/OpenLess"), "unknown");
        assert_eq!(
            error_code("failed to read vault: permission denied"),
            "unknown"
        );
    }

    #[test]
    fn custom_server_origin_must_be_a_bare_https_host() {
        assert_eq!(normalize_origin(""), Ok(None));
        assert_eq!(
            normalize_origin("  https://sync.example.com/  "),
            Ok(Some("https://sync.example.com".into()))
        );
        assert_eq!(
            normalize_origin("https://sync.example.com:8443"),
            Ok(Some("https://sync.example.com:8443".into()))
        );
        // Cleartext to a remote host would expose the vault key over the wire.
        assert!(normalize_origin("http://sync.example.com").is_err());
        assert_eq!(
            normalize_origin("http://127.0.0.1:8787"),
            Ok(Some("http://127.0.0.1:8787".into()))
        );
        assert!(normalize_origin("https://sync.example.com/api").is_err());
        assert!(normalize_origin("ftp://sync.example.com").is_err());
        assert!(normalize_origin("sync.example.com").is_err());
    }

    #[test]
    fn a_restore_cannot_be_applied_until_every_conflict_has_a_side() {
        let mut ui = CloudSyncUi::default();
        let preview: RestorePreview = serde_json::from_value(serde_json::json!({
            "previewId": "preview-1",
            "unconfirmedOperationId": null,
            "observedRevision": "3",
            "localGeneration": "3",
            "counts": { "dictionary": 2 },
            "deviceSettingsToReview": ["locale"],
            "conflicts": [
                { "id": "a", "kind": "style_pack", "reason": "both_changed" },
                { "id": "b", "kind": "dictionary", "reason": "both_changed" }
            ]
        }))
        .unwrap();
        ui.open(CloudSyncDialog::Restore {
            preview: Box::new(preview),
            enable_after: false,
        });
        assert!(!ui.conflicts_resolved());
        ui.resolve_conflict("a", Some(ConflictSide::Local));
        assert!(!ui.conflicts_resolved(), "one conflict is still open");
        ui.resolve_conflict("b", Some(ConflictSide::Cloud));
        assert!(ui.conflicts_resolved());
        let choices = ui.conflict_choices();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].side, ConflictSide::Local);
        assert_eq!(choices[1].side, ConflictSide::Cloud);
    }

    #[test]
    fn closing_a_dialog_forgets_the_typed_password() {
        let mut ui = CloudSyncUi::default();
        ui.accept_status(status("1"));
        ui.open(CloudSyncDialog::ChangePassword);
        ui.password = "new".into();
        ui.confirmation = "new".into();
        ui.current_password = "old".into();
        ui.close();
        assert!(ui.password.is_empty());
        assert!(ui.confirmation.is_empty());
        assert!(ui.current_password.is_empty());
    }

    /// 界面上不允许出现裸键：每个可能的失败代码都必须映射到 catalog 里存在的
    /// 文案。之前用 `format!("cloudSyncE2ee.errors.{code}")` 拼键，catalog 里没有
    /// 这批条目，用户看到的就是 `cloudSyncE2ee.errors.service_unavailable`。
    #[test]
    fn every_failure_code_resolves_to_a_real_message() {
        let codes = [
            // Core 侧会发出的代码（cloud_sync_e2ee 全部 `error(...)` 字面量）
            "account_changed",
            "busy",
            "cancelled",
            "choices_required",
            "cloud_deleted",
            "conflict",
            "consent_required",
            "crypto_worker_failed",
            "delete_confirmation_required",
            "denied",
            "expired",
            "invalid_data",
            "invalid_password",
            "invalid_response",
            "local_storage_unavailable",
            "locked",
            "metadata_required",
            "network",
            "not_signed_in",
            "outcome_unknown",
            "password_mismatch",
            "rate_limited",
            "recovery_required",
            "restore_review_required",
            "review_required",
            "revision_conflict",
            "revision_rollback",
            "secure_random_unavailable",
            "secure_storage",
            "secure_storage_denied",
            "server",
            "service_unavailable",
            "sign_in_required",
            "stale_preview",
            "stale_task",
            "too_large",
            "unlock_required",
            "unsupported_protocol",
            "weak_password",
            // 本地生成的代码
            "unknown",
            "unsupported",
        ];
        for code in codes {
            let key = error_key(code);
            let text = crate::tr_l10n(crate::Lang::ZhCn, key);
            assert_ne!(text, key, "{code} → {key} is missing from the catalog");
            assert!(
                !text.starts_with("cloudSyncE2ee."),
                "{code} rendered a raw key: {text}"
            );
        }
    }
}
