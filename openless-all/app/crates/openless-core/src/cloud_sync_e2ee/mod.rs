//! Encrypted sync orchestration. All private data stays in Core; the UI receives
//! only status, category counts and opaque conflict identifiers.

mod adapter;
mod dto;
mod local;
mod service;
#[cfg(test)]
mod tests;

pub(crate) use adapter::build;
pub use dto::*;
pub(crate) use service::{EncryptedSyncService, SyncServiceConfig};

pub const DEFAULT_SYNC_SERVICE_ORIGIN: &str = "https://apic.openless.top:9443";

#[derive(Debug, Clone)]
pub struct EncryptedSyncConfig {
    pub service_origin: String,
    pub app_version: String,
}

use crate::cloud_sync_e2ee_protocol as protocol;
use crate::{BackendError, BackendErrorCode};

pub(crate) type SyncResult<T> = Result<T, BackendError>;

pub(crate) fn error(reason: &'static str) -> BackendError {
    let code = match reason {
        "busy" | "stale_preview" | "revision_conflict" => BackendErrorCode::Busy,
        "cancelled" | "account_changed" => BackendErrorCode::Cancelled,
        "sign_in_required" | "unlock_required" | "secure_storage_denied" => {
            BackendErrorCode::PermissionDenied
        }
        "service_unavailable" | "unsupported_protocol" => BackendErrorCode::Unsupported,
        "outcome_unknown" => BackendErrorCode::OutcomeUnknown,
        "recovery_required" | "local_storage_unavailable" => BackendErrorCode::Persistence,
        "transport_failed" | "rate_limited" => BackendErrorCode::Provider,
        _ => BackendErrorCode::InvalidState,
    };
    let mut error = BackendError::new(code, reason);
    error.details = Some(serde_json::json!({"reason": reason}));
    error
}

pub(crate) fn protocol_error(value: protocol::Error) -> BackendError {
    use protocol::Error as E;
    let reason = match &value {
        E::WeakPassword => "weak_password",
        E::PasswordConfirmationMismatch => "password_confirmation_mismatch",
        E::InvalidPasswordOrCiphertext => "invalid_password_or_ciphertext",
        E::Transport => "transport_failed",
        E::PayloadTooLarge => "payload_too_large",
        E::AccountMismatch => "account_changed",
        E::ContextMismatch => "revision_conflict",
        E::UnsupportedProtocol | E::UnsupportedDocumentVersion => "unsupported_protocol",
        E::RandomUnavailable => "secure_random_unavailable",
        E::Api {
            status: 401 | 403, ..
        } => "sign_in_required",
        E::Api {
            status: 409 | 412, ..
        } => "revision_conflict",
        E::Api {
            status: 404 | 405 | 501,
            ..
        } => "service_unavailable",
        E::Api { status: 429, .. } => "rate_limited",
        E::Api { status: 413, .. } => "payload_too_large",
        E::Api { .. } => "service_failed",
        _ => "invalid_response",
    };
    let mut result = error(reason);
    if let E::Api {
        retry_after_seconds: Some(seconds),
        ..
    } = value
    {
        result.details = Some(serde_json::json!({"reason": reason, "retryAfterSeconds": seconds}));
    }
    result
}

pub(crate) fn document_error(
    value: crate::cloud_sync_e2ee_documents::DocumentError,
) -> BackendError {
    // DocumentError's Display is a closed set of value-free protocol codes.
    let reason = value.to_string();
    let code = if matches!(
        value,
        crate::cloud_sync_e2ee_documents::DocumentError::RuntimeBusy
            | crate::cloud_sync_e2ee_documents::DocumentError::SourceChanged
            | crate::cloud_sync_e2ee_documents::DocumentError::StalePreview
    ) {
        BackendErrorCode::Busy
    } else {
        BackendErrorCode::InvalidState
    };
    let mut result = BackendError::new(code, reason.clone());
    result.details = Some(serde_json::json!({"reason": reason}));
    result
}
