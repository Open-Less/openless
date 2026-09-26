//! Three-way logical merge. Timestamps never choose a winning value.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cloud_sync_e2ee_protocol::types::{
    DocumentKind, DocumentSet, LogicalDocument, Revision, Tombstone,
};

use super::types::*;
use super::validate::{timestamp, validate_sync_documents};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    BothModified,
    DeleteModify,
    NoCommonBaseline,
}

/// No local/cloud values, value hashes, excerpts, channel names, or secret lengths reach UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RedactedConflict {
    pub conflict_id: String,
    pub kind: DocumentKind,
    pub reason: ConflictReason,
    pub is_credential_unit: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSide {
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConflictChoice {
    pub conflict_id: String,
    pub side: ConflictSide,
}

#[derive(Clone, PartialEq)]
enum Entry {
    Live(LogicalDocument),
    Deleted(Tombstone),
}

type Unit = BTreeMap<DocumentKey, Entry>;
type UnitMap = BTreeMap<DocumentKey, Unit>;

pub struct MergePreview {
    source: DocumentSet,
    revision: Revision,
    accepted: UnitMap,
    unresolved: BTreeMap<String, (DocumentKey, Option<Unit>, Option<Unit>)>,
    conflicts: Vec<RedactedConflict>,
    active_choices: BTreeMap<SyncNamespace, Option<String>>,
    active_conflicts: BTreeMap<String, (SyncNamespace, Option<String>, Option<String>)>,
}

impl fmt::Debug for MergePreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MergePreview([REDACTED])")
    }
}

impl MergePreview {
    pub fn conflicts(&self) -> &[RedactedConflict] {
        &self.conflicts
    }

    pub fn resolve(mut self, choices: &[ConflictChoice]) -> DocumentResult<ValidatedSyncDocuments> {
        let mut selected = BTreeMap::new();
        for choice in choices {
            if selected
                .insert(choice.conflict_id.clone(), choice.side)
                .is_some()
            {
                return Err(DocumentError::ConflictChoiceRequired);
            }
        }
        if selected.keys().collect::<BTreeSet<_>>()
            != self
                .unresolved
                .keys()
                .chain(self.active_conflicts.keys())
                .collect::<BTreeSet<_>>()
        {
            return Err(DocumentError::ConflictChoiceRequired);
        }
        for (id, (key, local, remote)) in std::mem::take(&mut self.unresolved) {
            let chosen = match selected[&id] {
                ConflictSide::Local => local,
                ConflictSide::Remote => remote,
            };
            if let Some(unit) = chosen {
                self.accepted.insert(key, unit);
            }
        }
        for (id, (namespace, local, remote)) in std::mem::take(&mut self.active_conflicts) {
            self.active_choices.insert(
                namespace,
                match selected[&id] {
                    ConflictSide::Local => local,
                    ConflictSide::Remote => remote,
                },
            );
        }
        self.source.documents.clear();
        self.source.tombstones.clear();
        for unit in self.accepted.values() {
            for entry in unit.values() {
                match entry {
                    Entry::Live(doc) => self.source.documents.push(doc.clone()),
                    Entry::Deleted(tombstone) => self.source.tombstones.push(tombstone.clone()),
                }
            }
        }
        for (namespace, choice) in &self.active_choices {
            let mut found = choice.is_none();
            for doc in self
                .source
                .documents
                .iter_mut()
                .filter(|doc| doc.kind == DocumentKind::Channels)
            {
                let mut channel: ChannelRecord = serde_json::from_value(doc.value.clone())
                    .map_err(|_| DocumentError::InvalidDocument)?;
                if channel.namespace == *namespace {
                    channel.active = choice.as_deref() == Some(channel.id.as_str());
                    if channel.active {
                        channel.enabled = true;
                        found = true;
                    }
                    doc.value = serde_json::to_value(channel)
                        .map_err(|_| DocumentError::InvalidDocument)?;
                }
            }
            if !found {
                return Err(DocumentError::ConflictChoiceRequired);
            }
        }
        validate_sync_documents(self.source, self.revision)
    }
}

/// Different settings/record IDs merge independently; a channel and all its credentials do not.
pub fn diff_sync_documents(
    baseline: Option<&ValidatedSyncDocuments>,
    local: &ValidatedSyncDocuments,
    remote: &ValidatedSyncDocuments,
) -> DocumentResult<MergePreview> {
    let base = baseline.map(|set| units(&set.set));
    let left = units(&local.set);
    let right = units(&remote.set);
    let keys: BTreeSet<_> = base
        .iter()
        .flat_map(|map| map.keys())
        .chain(left.keys())
        .chain(right.keys())
        .cloned()
        .collect();
    let mut preview = MergePreview {
        source: local.set.clone(),
        revision: local.revision.max(remote.revision),
        accepted: BTreeMap::new(),
        unresolved: BTreeMap::new(),
        conflicts: Vec::new(),
        active_choices: BTreeMap::new(),
        active_conflicts: BTreeMap::new(),
    };
    for key in keys {
        let ancestor = base.as_ref().and_then(|map| map.get(&key));
        let local_unit = left.get(&key);
        let remote_unit = right.get(&key);
        // v1 never garbage-collects tombstones. A missing baseline identity is not a delete.
        if ancestor.is_some() && (local_unit.is_none() || remote_unit.is_none()) {
            return Err(DocumentError::MissingTombstone);
        }
        let chosen = if equivalent(local_unit, remote_unit) {
            Some(prefer_tombstone_revision(local_unit, remote_unit))
        } else if baseline.is_none() {
            if local_unit.is_none() {
                Some(remote_unit.cloned())
            } else if remote_unit.is_none() {
                Some(local_unit.cloned())
            } else {
                None
            }
        } else if is_deleted(ancestor) && (has_live(local_unit) || has_live(remote_unit)) {
            // A stale device cannot revive a deletion already present in the common baseline.
            None
        } else if equivalent(local_unit, ancestor) {
            Some(remote_unit.cloned())
        } else if equivalent(remote_unit, ancestor) {
            Some(local_unit.cloned())
        } else {
            None
        };
        if let Some(chosen) = chosen {
            if let Some(unit) = chosen {
                preview.accepted.insert(key, unit);
            }
            continue;
        }
        let conflict_id = conflict_id(&key);
        let reason = if is_deleted(local_unit) || is_deleted(remote_unit) || is_deleted(ancestor) {
            ConflictReason::DeleteModify
        } else if baseline.is_none() {
            ConflictReason::NoCommonBaseline
        } else {
            ConflictReason::BothModified
        };
        preview.conflicts.push(RedactedConflict {
            conflict_id: conflict_id.clone(),
            kind: key.kind,
            reason,
            is_credential_unit: key.kind == DocumentKind::Channels,
        });
        preview.unresolved.insert(
            conflict_id,
            (key, local_unit.cloned(), remote_unit.cloned()),
        );
    }
    for namespace in [SyncNamespace::Asr, SyncNamespace::Llm, SyncNamespace::Omni] {
        let local_active = active_channel(&local.set, namespace)?;
        let remote_active = active_channel(&remote.set, namespace)?;
        let base_active = baseline
            .map(|set| active_channel(&set.set, namespace))
            .transpose()?
            .flatten();
        let chosen = if local_active == remote_active {
            Some(local_active.clone())
        } else if baseline.is_some() && local_active == base_active {
            Some(remote_active.clone())
        } else if baseline.is_some() && remote_active == base_active {
            Some(local_active.clone())
        } else if baseline.is_none() && local_active.is_none() {
            Some(remote_active.clone())
        } else if baseline.is_none() && remote_active.is_none() {
            Some(local_active.clone())
        } else {
            None
        };
        if let Some(chosen) = chosen {
            preview.active_choices.insert(namespace, chosen);
        } else {
            let id = format!(
                "active-selection-{}",
                match namespace {
                    SyncNamespace::Asr => "asr",
                    SyncNamespace::Llm => "llm",
                    SyncNamespace::Omni => "omni",
                }
            );
            let conflict_id = conflict_id(&DocumentKey {
                kind: DocumentKind::Channels,
                id,
            });
            preview.conflicts.push(RedactedConflict {
                conflict_id: conflict_id.clone(),
                kind: DocumentKind::Channels,
                reason: if baseline.is_some() {
                    ConflictReason::BothModified
                } else {
                    ConflictReason::NoCommonBaseline
                },
                is_credential_unit: true,
            });
            preview
                .active_conflicts
                .insert(conflict_id, (namespace, local_active, remote_active));
        }
    }
    Ok(preview)
}

fn active_channel(set: &DocumentSet, namespace: SyncNamespace) -> DocumentResult<Option<String>> {
    for doc in set
        .documents
        .iter()
        .filter(|doc| doc.kind == DocumentKind::Channels)
    {
        let channel: ChannelRecord = serde_json::from_value(doc.value.clone())
            .map_err(|_| DocumentError::InvalidDocument)?;
        if channel.namespace == namespace && channel.active {
            return Ok(Some(channel.id));
        }
    }
    Ok(None)
}

fn units(set: &DocumentSet) -> UnitMap {
    let mut result: UnitMap = BTreeMap::new();
    for doc in &set.documents {
        let key = DocumentKey {
            kind: doc.kind,
            id: doc.id.clone(),
        };
        result
            .entry(unit_key(&key))
            .or_default()
            .insert(key, Entry::Live(doc.clone()));
    }
    for tombstone in &set.tombstones {
        let key = DocumentKey {
            kind: tombstone.kind,
            id: tombstone.id.clone(),
        };
        result
            .entry(unit_key(&key))
            .or_default()
            .insert(key, Entry::Deleted(tombstone.clone()));
    }
    result
}

fn unit_key(key: &DocumentKey) -> DocumentKey {
    DocumentKey {
        kind: if key.kind == DocumentKind::ProviderCredentials {
            DocumentKind::Channels
        } else {
            key.kind
        },
        id: key.id.clone(),
    }
}

fn equivalent(left: Option<&Unit>, right: Option<&Unit>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) if left.len() == right.len() => {
            left.iter()
                .all(|(key, value)| match (value, right.get(key)) {
                    (Entry::Deleted(_), Some(Entry::Deleted(_))) => true,
                    (_, Some(other)) => value == other,
                    _ => false,
                })
        }
        _ => false,
    }
}

fn prefer_tombstone_revision(left: Option<&Unit>, right: Option<&Unit>) -> Option<Unit> {
    let mut selected = left?.clone();
    if let Some(right) = right {
        for (key, entry) in right {
            if let (Entry::Deleted(candidate), Some(Entry::Deleted(current))) =
                (entry, selected.get(key))
            {
                if candidate.base_revision > current.base_revision {
                    selected.insert(key.clone(), entry.clone());
                }
            }
        }
    }
    Some(selected)
}

fn is_deleted(unit: Option<&Unit>) -> bool {
    unit.is_some_and(|unit| {
        unit.values()
            .any(|entry| matches!(entry, Entry::Deleted(_)))
    })
}
fn has_live(unit: Option<&Unit>) -> bool {
    unit.is_some_and(|unit| unit.values().any(|entry| matches!(entry, Entry::Live(_))))
}

fn conflict_id(key: &DocumentKey) -> String {
    let identity = serde_json::to_vec(key).expect("fixed conflict identity serializes");
    format!("{:x}", Sha256::digest(identity))
}

/// Materialize explicit local deletes relative to the last accepted baseline; keep all old marks.
/// Call after a coherent capture, never use a remote snapshot as the local deletion baseline.
pub fn record_missing_tombstones(
    mut current: DocumentSet,
    baseline: &ValidatedSyncDocuments,
    deleted_at: &str,
) -> DocumentResult<ValidatedSyncDocuments> {
    timestamp(deleted_at)?;
    let mut live: BTreeSet<_> = current
        .documents
        .iter()
        .map(|doc| (doc.kind, doc.id.clone()))
        .collect();
    live.extend(
        current
            .tombstones
            .iter()
            .map(|doc| (doc.kind, doc.id.clone())),
    );
    for old in &baseline.set.tombstones {
        if live.insert((old.kind, old.id.clone())) {
            current.tombstones.push(old.clone());
        }
    }
    for old in &baseline.set.documents {
        if live.insert((old.kind, old.id.clone())) {
            current.tombstones.push(Tombstone {
                id: old.id.clone(),
                kind: old.kind,
                deleted_at: deleted_at.into(),
                base_revision: baseline.revision,
            });
        }
    }
    validate_sync_documents(current, baseline.revision)
}
