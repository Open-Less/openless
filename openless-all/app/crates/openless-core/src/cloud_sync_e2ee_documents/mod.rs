//! Client-side logical documents for encrypted sync. No network or UI effects live here.

mod export;
mod merge;
pub mod registry;
mod restore;
mod types;
mod validate;

pub use export::{
    activity_record, export_provider_credentials_for_sync, export_snapshot, export_style_packs,
    export_sync_documents, secret_rows, ui_preferences, vocabulary_records,
};
pub use merge::{
    diff_sync_documents, record_missing_tombstones, ConflictChoice, ConflictReason, ConflictSide,
    MergePreview, RedactedConflict,
};
pub use restore::{
    apply_sync_restore, prepare_sync_restore, recover_sync_restore, CryptoJournalProtector,
    JournalProtector, JournalStore, RecoveryOutcome, RestorePlan, RestoreReceipt, SealedJournal,
};
pub use types::*;
pub(crate) use validate::validate_scope;
pub use validate::{validate_credential_set, validate_icon, validate_sync_documents};

#[cfg(test)]
mod restore_tests;
#[cfg(test)]
mod tests;
