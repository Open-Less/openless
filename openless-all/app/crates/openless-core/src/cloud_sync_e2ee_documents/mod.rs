//! Client-side logical documents for encrypted sync. No network or UI effects live here.

mod export;
mod merge;
pub mod registry;
mod restore;
mod types;
mod validate;

pub use crate::cloud_sync_e2ee_protocol::types::{DocumentKind, Revision};
pub use export::{
    activity_record, export_provider_credentials_for_sync, export_snapshot, export_style_packs,
    export_sync_documents, secret_rows, ui_preferences, vocabulary_records,
};
pub use merge::{
    diff_local_replace_documents, diff_sync_documents, record_missing_tombstones, ConflictChoice,
    ConflictReason, ConflictSide, MergePreview, RedactedConflict,
};
pub use restore::{
    apply_sync_restore, prepare_sync_restore, recover_sync_restore, CryptoJournalProtector,
    JournalProtector, JournalStore, RecoveryOutcome, RestorePlan, RestoreReceipt, SealedJournal,
};
pub use types::*;
pub(crate) use validate::validate_scope;
pub use validate::{validate_credential_set, validate_icon, validate_sync_documents};

pub fn parse_redacted_sync_documents(
    bytes: &[u8],
    observed_revision: crate::cloud_sync_e2ee_protocol::types::Revision,
) -> DocumentResult<ValidatedSyncDocuments> {
    let mut set = crate::cloud_sync_e2ee_protocol::types::DocumentSet::from_json_bytes(bytes)
        .map_err(|_| DocumentError::InvalidDocument)?;
    let channel_ids: Vec<_> = set
        .documents
        .iter()
        .filter(|document| document.kind == DocumentKind::Channels)
        .map(|document| document.id.clone())
        .collect();
    for channel_id in channel_ids {
        let (namespace, id) = channel_id
            .split_once(':')
            .ok_or(DocumentError::InvalidDocument)?;
        if !matches!(namespace, "asr" | "llm" | "omni") {
            return Err(DocumentError::InvalidDocument);
        }
        set.documents
            .push(crate::cloud_sync_e2ee_protocol::types::LogicalDocument {
                id: channel_id.clone(),
                kind: DocumentKind::ProviderCredentials,
                schema_version: DOCUMENT_SCHEMA_VERSION,
                value: serde_json::json!({
                    "channelId": id,
                    "namespace": namespace,
                    "accounts": {}
                }),
            });
    }
    let mut validated = validate_sync_documents(set, observed_revision)?;
    validated
        .set
        .documents
        .retain(|document| document.kind != DocumentKind::ProviderCredentials);
    Ok(validated)
}

/// Revalidate a logical set after a bundle adapter restores target-device-only fields.
pub fn revalidate_sync_documents(
    documents: crate::cloud_sync_e2ee_protocol::types::DocumentSet,
    observed_revision: crate::cloud_sync_e2ee_protocol::types::Revision,
) -> DocumentResult<ValidatedSyncDocuments> {
    validate_sync_documents(documents, observed_revision)
}

/// Parse a bounded logical document set before any local side effect.
///
/// The wire parser remains owned by the document module so other import
/// formats cannot accidentally bypass duplicate-key, depth, and schema checks.
pub fn parse_sync_documents(
    bytes: &[u8],
    observed_revision: crate::cloud_sync_e2ee_protocol::types::Revision,
) -> DocumentResult<ValidatedSyncDocuments> {
    let set = crate::cloud_sync_e2ee_protocol::types::DocumentSet::from_json_bytes(bytes)
        .map_err(|_| DocumentError::InvalidDocument)?;
    validate_sync_documents(set, observed_revision)
}

/// Serialize a validated logical document set without exposing its internal
/// representation to host adapters.
pub fn serialize_sync_documents(documents: &ValidatedSyncDocuments) -> DocumentResult<Vec<u8>> {
    serde_json::to_vec(documents.documents()).map_err(|_| DocumentError::InvalidDocument)
}

#[cfg(test)]
mod restore_tests;
#[cfg(test)]
mod tests;
