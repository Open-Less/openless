//! iOS-private confirmation metadata; Core remains the sole vocabulary owner.
//! No legacy source/note value is evidence of consent. This local scope cannot
//! be uploaded as an authenticated account or used to enable remote sync.
use openless_core::types::DictionaryEntry;
use openless_core::{BackendError, BackendErrorCode, OpenLessBackend};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: u64 = 64 * 1024;
const SCOPE: &str = "localOffline";
const FILE: &str = "ios-vocabulary-confirmations.json";

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ledger {
    schema_version: u32,
    scope: String,
    generation: u64,
    records: BTreeMap<String, Confirmation>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Confirmation {
    fingerprint: String,
    generation: u64,
}
#[derive(Deserialize)]
#[serde(
    tag = "command",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Request {
    Snapshot,
    Proposals,
    ConfirmEntry {
        id: String,
        phrase: String,
        expected_generation: u64,
    },
    Accept {
        id: String,
        expected_generation: u64,
    },
    Reject {
        id: String,
    },
    Dismiss,
}
fn failure(message: &str) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidState, message)
}
fn storage() -> BackendError {
    failure("vocabulary_confirmation_storage_unavailable")
}
fn bounded(phrase: &str) -> bool {
    !phrase.trim().is_empty()
        && phrase == phrase.trim()
        && phrase.chars().count() <= 64
        && !phrase.chars().any(char::is_control)
}
fn fingerprint(entry: &DictionaryEntry) -> String {
    // Include creation identity to reject ID reuse, but not mutable usage hits.
    let bytes =
        serde_json::to_vec(&(&entry.id, &entry.phrase, &entry.created_at, entry.enabled)).unwrap();
    format!("{:x}", Sha256::digest(bytes))
}
impl Ledger {
    fn load(directory: &Path) -> Result<Self, BackendError> {
        let path = directory.join(FILE);
        match fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    schema_version: 1,
                    scope: SCOPE.into(),
                    ..Self::default()
                })
            }
            Ok(meta) if meta.len() <= MAX_BYTES && meta.is_file() => {}
            _ => return Err(storage()),
        }
        let ledger: Self = serde_json::from_slice(&fs::read(path).map_err(|_| storage())?)
            .map_err(|_| storage())?;
        if ledger.schema_version != 1
            || ledger.scope != SCOPE
            || ledger.records.len() > MAX_ENTRIES
            || ledger
                .records
                .values()
                .any(|r| r.generation == 0 || r.generation > ledger.generation)
        {
            return Err(storage());
        }
        Ok(ledger)
    }
    fn advance(&mut self) -> Result<(), BackendError> {
        self.generation = self.generation.checked_add(1).ok_or_else(storage)?;
        Ok(())
    }
    fn save(&self, directory: &Path) -> Result<(), BackendError> {
        let bytes = serde_json::to_vec(self).map_err(|_| storage())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(storage());
        }
        let temporary = directory.join(format!(".{FILE}.{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, directory.join(FILE))?;
            File::open(directory)?.sync_all()
        })();
        let _ = fs::remove_file(temporary);
        result.map_err(|_| storage())
    }
    fn reconcile(
        &mut self,
        entries: &[DictionaryEntry],
        directory: &Path,
    ) -> Result<(), BackendError> {
        let count = self.records.len();
        self.records.retain(|id, record| {
            entries.iter().any(|entry| {
                &entry.id == id
                    && entry.enabled
                    && bounded(&entry.phrase)
                    && record.fingerprint == fingerprint(entry)
            })
        });
        if self.records.len() != count {
            self.advance()?;
            self.save(directory)?;
        }
        Ok(())
    }
    fn confirm(&mut self, entry: &DictionaryEntry, directory: &Path) -> Result<(), BackendError> {
        if !entry.enabled || !bounded(&entry.phrase) {
            return Err(failure("vocabulary_entry_not_exportable"));
        }
        if self.records.len() >= MAX_ENTRIES && !self.records.contains_key(&entry.id) {
            return Err(failure("vocabulary_confirmation_limit"));
        }
        self.advance()?;
        self.records.insert(
            entry.id.clone(),
            Confirmation {
                fingerprint: fingerprint(entry),
                generation: self.generation,
            },
        );
        self.save(directory)
    }
    fn check_generation(&self, generation: u64) -> Result<(), BackendError> {
        if generation != self.generation {
            return Err(failure("stale_vocabulary_generation"));
        }
        Ok(())
    }
}

/// Parent must call this BEFORE every rename, enable/disable or removal. A failed
/// invalidation aborts the Core mutation. Rename-back cannot revive old consent.
/// Serialize with execute and Core mutations on Host's control admission gate.
pub(crate) fn invalidate_entry(directory: &Path, id: &str) -> Result<(), BackendError> {
    let mut ledger = Ledger::load(directory)?;
    ledger.records.remove(id);
    ledger.advance()?;
    ledger.save(directory)
}

/// Only the parent AddVocabulary/UpdateVocabulary control branches may use
/// this after their explicit user mutation. Do not call from list/refresh/sync.
/// Core may already be changed if this returns an error; retry requires explicit
/// confirmation rather than rollback or inferred consent. False means saved
/// Core content is outside this bounded enabled projection (not a CRUD error).
pub(crate) fn confirm_explicit_edit(
    backend: &OpenLessBackend,
    directory: &Path,
    id: &str,
    phrase: &str,
) -> Result<bool, BackendError> {
    let mut ledger = Ledger::load(directory)?;
    let entries = backend.list_vocabulary()?;
    ledger.reconcile(&entries, directory)?;
    let entry = entries
        .iter()
        .find(|entry| entry.id == id && entry.phrase == phrase)
        .ok_or_else(|| failure("vocabulary_entry_changed"))?;
    if !entry.enabled || !bounded(&entry.phrase) {
        return Ok(false);
    }
    ledger.confirm(entry, directory)?;
    Ok(true)
}

/// Main-App only, under Host control admission. No keyboard may call Core or
/// directly write the private metadata file. Missing/corrupt metadata fails closed.
pub(crate) fn execute(
    backend: &OpenLessBackend,
    directory: &Path,
    request: Value,
) -> Result<Value, BackendError> {
    let request: Request = serde_json::from_value(request)
        .map_err(|_| failure("invalid_vocabulary_bridge_request"))?;
    let mut ledger = Ledger::load(directory)?;
    let entries = backend.list_vocabulary()?;
    ledger.reconcile(&entries, directory)?;
    match request {
        Request::Snapshot => {
            let mut seen = BTreeSet::new();
            let entries: Vec<_> = entries.iter().filter_map(|entry| ledger.records.get(&entry.id).map(|record|
                (entry, record))).filter(|(entry, _)| seen.insert(entry.phrase.clone())).map(|(entry, record)|
                json!({"id":entry.id,"phrase":entry.phrase,"confirmationGeneration":record.generation})).collect();
            let result = json!({"schemaVersion":1,"scope":SCOPE,"generation":ledger.generation,"entries":entries});
            if serde_json::to_vec(&result).map_err(|_| storage())?.len() as u64 > MAX_BYTES {
                return Err(storage());
            }
            Ok(result)
        }
        Request::Proposals => {
            let mut seen = BTreeSet::new();
            let proposals: Vec<_> = backend
                .pending_corrections()
                .into_iter()
                .filter(|p| {
                    bounded(&p.replacement)
                        && !entries.iter().any(|e| e.phrase == p.replacement)
                        && seen.insert(p.replacement.clone())
                })
                .take(16)
                .map(|p| json!({"id":p.id,"phrase":p.replacement,"expiresAtMs":p.expires_at_ms}))
                .collect();
            Ok(
                json!({"schemaVersion":1,"scope":SCOPE,"generation":ledger.generation,"kind":"correction","proposals":proposals}),
            )
        }
        Request::ConfirmEntry {
            id,
            phrase,
            expected_generation,
        } => {
            ledger.check_generation(expected_generation)?;
            let entry = entries
                .iter()
                .find(|e| e.id == id && e.phrase == phrase)
                .ok_or_else(|| failure("vocabulary_entry_changed"))?;
            ledger.confirm(entry, directory)?;
            Ok(json!({"confirmed":true,"generation":ledger.generation}))
        }
        Request::Accept {
            id,
            expected_generation,
        } => {
            ledger.check_generation(expected_generation)?;
            let proposal = backend
                .pending_corrections()
                .into_iter()
                .find(|p| p.id == id)
                .ok_or_else(|| failure("vocabulary_proposal_expired"))?;
            if !bounded(&proposal.replacement)
                || entries.iter().any(|e| e.phrase == proposal.replacement)
            {
                return Err(failure("vocabulary_proposal_duplicate_or_invalid"));
            }
            // Core owns expiry checks and acceptance. If metadata persistence
            // fails after this mutation, the new row stays UNCONFIRMED. Recovery
            // is an explicit confirmEntry, never inference from the learned note.
            backend
                .accept_pending_correction(&id)?
                .ok_or_else(|| failure("vocabulary_proposal_expired"))?;
            let entry = backend
                .list_vocabulary()?
                .into_iter()
                .find(|e| e.phrase == proposal.replacement)
                .ok_or_else(|| failure("vocabulary_entry_missing"))?;
            ledger.confirm(&entry, directory)?;
            Ok(json!({"confirmed":true,"generation":ledger.generation}))
        }
        Request::Reject { id } => Ok(json!({"rejected":backend.reject_pending_correction(&id)})),
        Request::Dismiss => {
            backend.dismiss_pending_corrections();
            Ok(json!({}))
        }
    }
}

#[cfg(test)]
#[path = "vocabulary_bridge_tests.rs"]
mod tests;
