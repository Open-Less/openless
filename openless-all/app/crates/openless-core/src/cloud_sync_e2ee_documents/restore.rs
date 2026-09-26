//! Crash-recoverable logical restore with authenticated, encrypted before/after images.

use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::cloud_sync_e2ee_protocol::{
    crypto::{self, DerivedKey},
    types::{DocumentSet, Revision},
};

use super::export::export_snapshot;
use super::types::*;
use super::validate::{uuid_v4, validate_scope, validate_sync_documents};

pub struct RestorePlan {
    desired: ValidatedSyncDocuments,
    context: RestoreContext,
}
impl fmt::Debug for RestorePlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RestorePlan([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreReceipt {
    pub operation_id: String,
    pub generation: Revision,
    pub committed: bool,
    pub journal_cleanup_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    NoPending,
    Committed(RestoreReceipt),
    RolledBack(RestoreReceipt),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SealedJournal {
    pub operation_id: String,
    pub ciphertext: Vec<u8>,
}
impl fmt::Debug for SealedJournal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SealedJournal([REDACTED])")
    }
}

pub trait JournalStore: Send + Sync {
    fn load(&self, scope: SyncScope) -> BoxFuture<'_, DocumentResult<Option<SealedJournal>>>;
    /// Save atomically and durably before returning. Never accept a plaintext journal DTO here.
    fn save(&self, scope: SyncScope, journal: SealedJournal) -> BoxFuture<'_, DocumentResult<()>>;
    fn clear(&self, scope: SyncScope) -> BoxFuture<'_, DocumentResult<()>>;
}

pub trait JournalProtector: Send + Sync {
    fn seal(
        &self,
        scope: &SyncScope,
        operation_id: &str,
        plaintext: &[u8],
    ) -> DocumentResult<Vec<u8>>;
    fn open(
        &self,
        scope: &SyncScope,
        operation_id: &str,
        ciphertext: &[u8],
    ) -> DocumentResult<Zeroizing<Vec<u8>>>;
}

/// The key is a separate OS-protected local key, not a password or ordinary file secret.
pub struct CryptoJournalProtector {
    key: Arc<DerivedKey>,
}
impl CryptoJournalProtector {
    pub(crate) fn new(key: Arc<DerivedKey>) -> Self {
        Self { key }
    }
}
impl fmt::Debug for CryptoJournalProtector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CryptoJournalProtector([REDACTED])")
    }
}
impl JournalProtector for CryptoJournalProtector {
    fn seal(
        &self,
        scope: &SyncScope,
        operation_id: &str,
        plaintext: &[u8],
    ) -> DocumentResult<Vec<u8>> {
        crypto::seal_local(&self.key, &journal_aad(scope, operation_id)?, plaintext)
            .map_err(|_| DocumentError::Locked)
    }
    fn open(
        &self,
        scope: &SyncScope,
        operation_id: &str,
        ciphertext: &[u8],
    ) -> DocumentResult<Zeroizing<Vec<u8>>> {
        crypto::open_local(&self.key, &journal_aad(scope, operation_id)?, ciphertext)
            .map_err(|_| DocumentError::Locked)
    }
}

fn journal_aad(scope: &SyncScope, operation_id: &str) -> DocumentResult<Vec<u8>> {
    validate_scope(scope)?;
    uuid_v4(operation_id)?;
    serde_json::to_vec(&("openless-sync-restore-journal-v1", scope, operation_id))
        .map_err(|_| DocumentError::InvalidDocument)
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    Committed,
    RolledBack,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    scope: SyncScope,
    operation_id: String,
    observed_revision: Revision,
    phase: Phase,
    before: DocumentSet,
    after: DocumentSet,
    #[serde(default)]
    local_before: SecretJson,
}

pub fn prepare_sync_restore(
    desired: ValidatedSyncDocuments,
    context: RestoreContext,
) -> DocumentResult<RestorePlan> {
    validate_scope(&context.scope)?;
    uuid_v4(&context.operation_id)?;
    if desired.observed_revision() != context.observed_revision
        || context.target_device.id != context.scope.device_id
    {
        return Err(DocumentError::StalePreview);
    }
    Ok(RestorePlan { desired, context })
}

/// No caller cancellation branch is allowed after the durable journal exists.
/// If the task/process nevertheless dies, recovery chooses the durable decision below.
pub async fn apply_sync_restore(
    plan: RestorePlan,
    backend: &dyn RestoreBackend,
    storage: &dyn JournalStore,
    protector: &dyn JournalProtector,
) -> DocumentResult<RestoreReceipt> {
    if storage.load(plan.context.scope.clone()).await?.is_some() {
        return Err(DocumentError::RecoveryRequired);
    }
    let mut lease = backend
        .acquire(
            plan.context.scope.clone(),
            Some(plan.context.local_generation),
        )
        .await?;
    let before = export_snapshot(lease.capture().await?)?;
    if before.generation != plan.context.local_generation {
        return Err(DocumentError::StalePreview);
    }
    let mut after = plan.desired.set.clone();
    // Foreign device profiles are preserved, but importing them cannot erase the receiving
    // device's own profile or make its paths/programs active.
    let local_profile = before.documents.set.documents.iter().find(|doc| {
        doc.kind == crate::cloud_sync_e2ee_protocol::types::DocumentKind::DeviceProfile
            && doc.id == plan.context.target_device.id
    });
    if let Some(profile) = local_profile {
        if !after
            .documents
            .iter()
            .any(|doc| doc.kind == profile.kind && doc.id == profile.id)
        {
            after.documents.push(profile.clone());
        }
    }
    materialize_receiver_fields(
        &mut after,
        before.documents.documents(),
        &plan.context.target_device,
    )?;
    preserve_applicable_asr_selection(
        &mut after,
        before.documents.documents(),
        &plan.context.target_device.id,
    )?;
    after.source_device = plan.context.target_device.clone();
    let after = validate_sync_documents(after, plan.context.observed_revision)?;
    lease.preflight(&after).await?;
    let mut journal = Journal {
        schema_version: 1,
        scope: plan.context.scope.clone(),
        operation_id: plan.context.operation_id.clone(),
        observed_revision: plan.context.observed_revision,
        phase: Phase::Prepared,
        before: before.documents.set.clone(),
        after: after.set.clone(),
        local_before: lease.capture_rollback_state().await?,
    };
    lease.mark_recovery_pending(&journal.operation_id)?;
    save_journal(&journal, storage, protector).await?;
    lease.mark_journal_ready(&journal.operation_id)?;
    if replace_and_verify(lease.as_mut(), after).await.is_err() {
        let before = validate_sync_documents(journal.before.clone(), journal.observed_revision)?;
        if replace_and_verify(lease.as_mut(), before).await.is_err() {
            return Err(DocumentError::RecoveryRequired);
        }
        restore_local_rollback_state(lease.as_mut(), &journal.local_before)
            .await
            .map_err(|_| DocumentError::RecoveryRequired)?;
        journal.phase = Phase::RolledBack;
        save_journal(&journal, storage, protector)
            .await
            .map_err(|_| DocumentError::RecoveryRequired)?;
        let _ = finish(&journal, lease.as_mut(), storage).await?;
        return Err(DocumentError::RestoreRolledBack);
    }
    journal.phase = Phase::Committed;
    // A failed/unknown commit-decision write cannot safely be reinterpreted as rollback.
    save_journal(&journal, storage, protector)
        .await
        .map_err(|_| DocumentError::RecoveryRequired)?;
    finish(&journal, lease.as_mut(), storage).await
}

fn materialize_receiver_fields(
    after: &mut DocumentSet,
    before: &DocumentSet,
    target: &crate::cloud_sync_e2ee_protocol::types::SourceDevice,
) -> DocumentResult<()> {
    use crate::cloud_sync_e2ee_protocol::types::DocumentKind;
    for old in &before.documents {
        let required = matches!(
            old.kind,
            DocumentKind::Preferences | DocumentKind::UiPreferences
        ) || old.kind == DocumentKind::VocabularyPresets
            && old.id.starts_with("builtin:")
            || old.kind == DocumentKind::StylePacks && old.value["pack"]["kind"] == "builtin";
        if !required
            || after
                .documents
                .iter()
                .any(|doc| doc.kind == old.kind && doc.id == old.id)
        {
            continue;
        }
        let deleted = after
            .tombstones
            .iter()
            .any(|mark| mark.kind == old.kind && mark.id == old.id);
        if deleted {
            let native_required = old.kind == DocumentKind::Preferences
                && super::registry::preference_field(&old.id).is_some()
                || old.kind == DocumentKind::UiPreferences
                    && matches!(old.id.as_str(), "locale" | "fontScale")
                || old.kind == DocumentKind::VocabularyPresets
                || old.kind == DocumentKind::StylePacks;
            if native_required {
                return Err(DocumentError::Unsupported);
            }
        } else {
            after.documents.push(old.clone());
        }
    }
    if let Some(profile) = after
        .documents
        .iter_mut()
        .find(|doc| doc.kind == DocumentKind::DeviceProfile && doc.id == target.id)
    {
        let mut value: DeviceProfileRecord = serde_json::from_value(profile.value.clone())
            .map_err(|_| DocumentError::InvalidDocument)?;
        if value.device.os != target.os || value.device.arch != target.arch {
            return Err(DocumentError::Unsupported);
        }
        if let Some(previous) = before
            .documents
            .iter()
            .find(|doc| doc.kind == DocumentKind::DeviceProfile && doc.id == target.id)
        {
            let previous: DeviceProfileRecord = serde_json::from_value(previous.value.clone())
                .map_err(|_| DocumentError::InvalidDocument)?;
            let preferences = value
                .preferences
                .expose_mut()
                .as_object_mut()
                .ok_or(DocumentError::InvalidDocument)?;
            for (key, old) in previous
                .preferences
                .expose()
                .as_object()
                .ok_or(DocumentError::InvalidDocument)?
            {
                preferences
                    .entry(key.clone())
                    .or_insert_with(|| old.clone());
            }
        }
        value.device = target.clone();
        profile.value = serde_json::to_value(value).map_err(|_| DocumentError::InvalidDocument)?;
    }
    Ok(())
}

fn preserve_applicable_asr_selection(
    after: &mut DocumentSet,
    before: &DocumentSet,
    target_device_id: &str,
) -> DocumentResult<()> {
    use crate::cloud_sync_e2ee_protocol::types::DocumentKind;
    let foreign_local_active = after
        .documents
        .iter()
        .filter(|doc| doc.kind == DocumentKind::DeviceProfile && doc.id != target_device_id)
        .try_fold(false, |found, doc| {
            let profile: DeviceProfileRecord = serde_json::from_value(doc.value.clone())
                .map_err(|_| DocumentError::InvalidDocument)?;
            Ok::<_, DocumentError>(
                found
                    || profile
                        .channels
                        .iter()
                        .any(|channel| channel.namespace == SyncNamespace::Asr && channel.active),
            )
        })?;
    let mut active = std::collections::BTreeSet::new();
    for doc in after
        .documents
        .iter()
        .filter(|doc| doc.kind == DocumentKind::Channels)
    {
        let channel: ChannelRecord = serde_json::from_value(doc.value.clone())
            .map_err(|_| DocumentError::InvalidDocument)?;
        if channel.active {
            active.insert(channel.namespace);
        }
    }
    if foreign_local_active && !active.contains(&SyncNamespace::Asr) {
        if let Some(current) = before.documents.iter().find(|doc| {
            doc.kind == DocumentKind::Channels
                && doc.value["namespace"] == "asr"
                && doc.value["active"] == true
                && !after.tombstones.iter().any(|mark| {
                    mark.id == doc.id
                        && matches!(
                            mark.kind,
                            DocumentKind::Channels | DocumentKind::ProviderCredentials
                        )
                })
        }) {
            if let Some(existing) = after
                .documents
                .iter_mut()
                .find(|doc| doc.kind == DocumentKind::Channels && doc.id == current.id)
            {
                existing.value["active"] = serde_json::Value::Bool(true);
                existing.value["enabled"] = serde_json::Value::Bool(true);
            } else {
                after.documents.push(current.clone());
                if let Some(credential) = before.documents.iter().find(|doc| {
                    doc.kind == DocumentKind::ProviderCredentials && doc.id == current.id
                }) {
                    after.documents.push(credential.clone());
                }
            }
            active.insert(SyncNamespace::Asr);
        }
    }
    if let Some(profile) = after
        .documents
        .iter_mut()
        .find(|doc| doc.kind == DocumentKind::DeviceProfile && doc.id == target_device_id)
    {
        let mut value: DeviceProfileRecord = serde_json::from_value(profile.value.clone())
            .map_err(|_| DocumentError::InvalidDocument)?;
        for channel in &mut value.channels {
            if active.contains(&channel.namespace) {
                channel.active = false;
            }
        }
        profile.value = serde_json::to_value(value).map_err(|_| DocumentError::InvalidDocument)?;
    }
    Ok(())
}

pub async fn recover_sync_restore(
    scope: SyncScope,
    backend: &dyn RestoreBackend,
    storage: &dyn JournalStore,
    protector: &dyn JournalProtector,
) -> DocumentResult<RecoveryOutcome> {
    validate_scope(&scope)?;
    let mut lease = backend.acquire(scope.clone(), None).await?;
    let Some(sealed) = storage.load(scope.clone()).await? else {
        lease.recover_without_journal()?;
        return Ok(RecoveryOutcome::NoPending);
    };
    uuid_v4(&sealed.operation_id)?;
    if sealed.ciphertext.len() > MAX_JOURNAL_BYTES + 128 {
        return Err(DocumentError::RecoveryRequired);
    }
    let plaintext = protector.open(&scope, &sealed.operation_id, &sealed.ciphertext)?;
    if plaintext.len() > MAX_JOURNAL_BYTES {
        return Err(DocumentError::RecoveryRequired);
    }
    let mut journal: Journal =
        serde_json::from_slice(&plaintext).map_err(|_| DocumentError::RecoveryRequired)?;
    if journal.schema_version != 1
        || journal.scope != scope
        || journal.operation_id != sealed.operation_id
    {
        return Err(DocumentError::RecoveryRequired);
    }
    let before = validate_sync_documents(journal.before.clone(), journal.observed_revision)
        .map_err(|_| DocumentError::RecoveryRequired)?;
    let after = validate_sync_documents(journal.after.clone(), journal.observed_revision)
        .map_err(|_| DocumentError::RecoveryRequired)?;
    if let Some(mut receipt) = lease.completed_restore(&journal.operation_id)? {
        // New user mutations may exist after this receipt. Never replay an obsolete image.
        receipt.journal_cleanup_pending = storage.clear(scope).await.is_err();
        return Ok(if receipt.committed {
            RecoveryOutcome::Committed(receipt)
        } else {
            RecoveryOutcome::RolledBack(receipt)
        });
    }
    lease.mark_recovery_pending(&journal.operation_id)?;
    lease.mark_journal_ready(&journal.operation_id)?;
    let committed = journal.phase == Phase::Committed;
    replace_and_verify(lease.as_mut(), if committed { after } else { before })
        .await
        .map_err(|_| DocumentError::RecoveryRequired)?;
    if !committed {
        restore_local_rollback_state(lease.as_mut(), &journal.local_before)
            .await
            .map_err(|_| DocumentError::RecoveryRequired)?;
        journal.phase = Phase::RolledBack;
        save_journal(&journal, storage, protector)
            .await
            .map_err(|_| DocumentError::RecoveryRequired)?;
    }
    let receipt = finish(&journal, lease.as_mut(), storage).await?;
    Ok(if committed {
        RecoveryOutcome::Committed(receipt)
    } else {
        RecoveryOutcome::RolledBack(receipt)
    })
}

async fn save_journal(
    journal: &Journal,
    storage: &dyn JournalStore,
    protector: &dyn JournalProtector,
) -> DocumentResult<()> {
    let plaintext =
        Zeroizing::new(serde_json::to_vec(journal).map_err(|_| DocumentError::InvalidDocument)?);
    if plaintext.len() > MAX_JOURNAL_BYTES {
        return Err(DocumentError::PayloadTooLarge);
    }
    let ciphertext = protector.seal(&journal.scope, &journal.operation_id, &plaintext)?;
    if ciphertext.is_empty() || ciphertext.len() > MAX_JOURNAL_BYTES + 128 {
        return Err(DocumentError::JournalUnavailable);
    }
    storage
        .save(
            journal.scope.clone(),
            SealedJournal {
                operation_id: journal.operation_id.clone(),
                ciphertext,
            },
        )
        .await
}

async fn restore_local_rollback_state(
    lease: &mut dyn RestoreLease,
    expected: &SecretJson,
) -> DocumentResult<()> {
    lease.restore_rollback_state(expected).await?;
    if lease.capture_rollback_state().await? != *expected {
        return Err(DocumentError::RecoveryRequired);
    }
    Ok(())
}

async fn replace_and_verify(
    lease: &mut dyn RestoreLease,
    expected: ValidatedSyncDocuments,
) -> DocumentResult<()> {
    lease.replace(expected.clone()).await?;
    lease.reload().await?;
    let actual = export_snapshot(lease.capture().await?)?.documents;
    // Source metadata and capture time differ; logical records and deletions must not.
    if actual.set.documents != expected.set.documents
        || actual.set.tombstones != expected.set.tombstones
    {
        return Err(DocumentError::RecoveryRequired);
    }
    Ok(())
}

async fn finish(
    journal: &Journal,
    lease: &mut dyn RestoreLease,
    storage: &dyn JournalStore,
) -> DocumentResult<RestoreReceipt> {
    let mut receipt =
        lease.finish_restore(&journal.operation_id, journal.phase == Phase::Committed)?;
    receipt.journal_cleanup_pending = storage.clear(journal.scope.clone()).await.is_err();
    Ok(receipt)
}
