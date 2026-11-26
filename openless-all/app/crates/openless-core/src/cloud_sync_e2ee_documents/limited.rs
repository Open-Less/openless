//! Opt-in domain format, not a second remote protocol. V1's singleton vault
//! cannot safely transport this alongside the desktop full snapshot.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use zeroize::Zeroizing;

use super::*;
use crate::cloud_sync_e2ee_protocol::types::{DocumentKind, DocumentSet, Revision};

pub const LIMITED_SYNC_NAMESPACE: &str = "confirmed-vocabulary-mode-style-references-v1";
const STYLE_REFERENCE: &str = "marketStyleReference";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StyleReference {
    id: String,
    version: String,
    origin_pack_id: Option<String>,
}
impl StyleReference {
    fn from_style(doc: &LogicalDocument) -> DocumentResult<Self> {
        let record: StylePackRecord = serde_json::from_value(doc.value.clone())
            .map_err(|_| DocumentError::InvalidDocument)?;
        let pack = record.pack.expose();
        let reference = Self {
            id: doc.id.clone(),
            version: pack
                .get("version")
                .and_then(Value::as_str)
                .ok_or(DocumentError::InvalidDocument)?
                .into(),
            origin_pack_id: pack
                .get("originPackId")
                .filter(|value| !value.is_null())
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or(DocumentError::InvalidDocument)
                })
                .transpose()?,
        };
        reference.validate()?;
        Ok(reference)
    }
    fn validate(&self) -> DocumentResult<()> {
        super::validate::identifier(&self.id)?;
        if self.version.is_empty()
            || self.version.len() > 128
            || self.version.chars().any(char::is_control)
        {
            return Err(DocumentError::InvalidDocument);
        }
        if let Some(origin) = &self.origin_pack_id {
            super::validate::identifier(origin)?;
        }
        Ok(())
    }
}
fn style_reference(value: &Value) -> DocumentResult<Option<StyleReference>> {
    let reference: Option<StyleReference> =
        serde_json::from_value(value.clone()).map_err(|_| DocumentError::InvalidDocument)?;
    if let Some(reference) = &reference {
        reference.validate()?;
    }
    Ok(reference)
}

/// Host permission, not a remote capability. Existing desktop behavior remains the default.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AllowedSyncScope {
    #[default]
    FullSnapshot,
    ConfirmedVocabularyModeStyleReferences,
}

/// Explicit Host confirmation decision, bound to the exact local capture. Empty means
/// no confirmed local rows, NEVER all rows. The Host owns durable confirmation metadata.
#[derive(Clone)]
pub struct ConfirmedVocabularySelection {
    scope: SyncScope,
    generation: Revision,
    ids: BTreeSet<String>,
}
impl ConfirmedVocabularySelection {
    pub fn new(scope: SyncScope, generation: Revision, ids: Vec<String>) -> DocumentResult<Self> {
        super::validate::validate_scope(&scope)?;
        if ids.len() > MAX_DOCUMENTS {
            return Err(DocumentError::PayloadTooLarge);
        }
        let mut unique = BTreeSet::new();
        for id in ids {
            super::validate::identifier(&id)?;
            if !unique.insert(id) {
                return Err(DocumentError::DuplicateId);
            }
        }
        Ok(Self {
            scope,
            generation,
            ids: unique,
        })
    }
    pub(crate) fn check(&self, full: &ExportedDocuments, scope: &SyncScope) -> DocumentResult<()> {
        if self.scope != *scope || self.generation != full.generation {
            return Err(DocumentError::StalePreview);
        }
        if self.ids.iter().any(|id| {
            !full
                .documents
                .documents()
                .documents
                .iter()
                .any(|doc| doc.kind == DocumentKind::Dictionary && doc.id == *id)
        }) {
            return Err(DocumentError::SourceChanged);
        }
        Ok(())
    }
}
impl std::fmt::Debug for ConfirmedVocabularySelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConfirmedVocabularySelection([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    namespace: String,
    service_origin: String,
    owner_github_id: String,
    vault_id: String,
    key_id: String,
}
impl Manifest {
    fn for_scope(scope: &SyncScope) -> DocumentResult<Self> {
        super::validate::validate_scope(scope)?;
        Ok(Self {
            schema_version: 1,
            namespace: LIMITED_SYNC_NAMESPACE.into(),
            service_origin: scope.service_origin.clone(),
            owner_github_id: scope.owner_github_id.clone(),
            vault_id: scope.vault_id.clone(),
            key_id: scope.key_id.clone(),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    manifest: Manifest,
    documents: DocumentSet,
}

/// Only validated limited data can be serialized; no full snapshot accessor is exposed.
#[derive(Clone)]
pub struct LimitedSyncDocuments {
    manifest: Manifest,
    documents: ValidatedSyncDocuments,
}
impl std::fmt::Debug for LimitedSyncDocuments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LimitedSyncDocuments([REDACTED])")
    }
}

fn allowed(kind: DocumentKind, id: &str) -> bool {
    kind == DocumentKind::Dictionary
        || kind == DocumentKind::Preferences
            && matches!(id, "defaultMode" | "enabledModes" | STYLE_REFERENCE)
}

fn validate_limited(
    set: DocumentSet,
    revision: Revision,
) -> DocumentResult<ValidatedSyncDocuments> {
    for doc in &set.documents {
        if !allowed(doc.kind, &doc.id) {
            return Err(DocumentError::ExcludedField);
        }
        if doc.kind == DocumentKind::Dictionary {
            let row = doc
                .value
                .as_object()
                .ok_or(DocumentError::InvalidDocument)?;
            if row.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "id" | "phrase" | "enabled" | "createdAt" | "sortIndex"
                )
            }) {
                return Err(DocumentError::ExcludedField);
            }
            if row.get("enabled").and_then(Value::as_bool).is_none()
                || row.get("createdAt").and_then(Value::as_str).is_none()
            {
                return Err(DocumentError::InvalidDocument);
            }
        } else if doc.id == STYLE_REFERENCE {
            style_reference(&doc.value)?;
        } else if doc.id == "defaultMode" {
            serde_json::from_value::<crate::types::PolishMode>(doc.value.clone())
                .map_err(|_| DocumentError::InvalidDocument)?;
        } else if doc.id == "enabledModes" {
            let modes = serde_json::from_value::<Vec<crate::types::PolishMode>>(doc.value.clone())
                .map_err(|_| DocumentError::InvalidDocument)?;
            if modes
                .iter()
                .enumerate()
                .any(|(index, mode)| modes[..index].contains(mode))
            {
                return Err(DocumentError::DuplicateId);
            }
        }
    }
    for mark in &set.tombstones {
        // These native settings cannot be deleted; selecting an empty style is explicit.
        if mark.kind != DocumentKind::Dictionary {
            return Err(DocumentError::ExcludedField);
        }
    }
    for id in ["defaultMode", "enabledModes", STYLE_REFERENCE] {
        if !set
            .documents
            .iter()
            .any(|doc| doc.kind == DocumentKind::Preferences && doc.id == id)
        {
            return Err(DocumentError::InvalidDocument);
        }
    }
    validate_sync_documents(set, revision)
}

impl LimitedSyncDocuments {
    /// Project only committed vocabulary rows, never unconfirmed candidate stores.
    /// Notes and usage counters are receiver-local; prompts and style contents are absent.
    pub(crate) fn project(
        full: &ExportedDocuments,
        scope: &SyncScope,
        confirmed: &ConfirmedVocabularySelection,
    ) -> DocumentResult<Self> {
        confirmed.check(full, scope)?;
        let mut set = full.documents.documents().clone();
        let selected_id = set
            .documents
            .iter()
            .find(|doc| doc.kind == DocumentKind::Preferences && doc.id == "activeStylePackId")
            .and_then(|doc| doc.value.as_str())
            .ok_or(DocumentError::InvalidDocument)?;
        let reference = if selected_id.is_empty() {
            None
        } else {
            Some(StyleReference::from_style(
                set.documents
                    .iter()
                    .find(|doc| doc.kind == DocumentKind::StylePacks && doc.id == selected_id)
                    .ok_or(DocumentError::InvalidReference)?,
            )?)
        };
        let reference =
            serde_json::to_value(reference).map_err(|_| DocumentError::InvalidDocument)?;
        set.documents.retain(|doc| {
            doc.kind == DocumentKind::Dictionary && confirmed.ids.contains(&doc.id)
                || doc.kind == DocumentKind::Preferences
                    && matches!(
                        doc.id.as_str(),
                        "defaultMode" | "enabledModes" | "activeStylePackId"
                    )
        });
        for doc in &mut set.documents {
            if doc.kind == DocumentKind::Dictionary {
                let row = doc
                    .value
                    .as_object_mut()
                    .ok_or(DocumentError::InvalidDocument)?;
                row.retain(|key, _| {
                    matches!(
                        key.as_str(),
                        "id" | "phrase" | "enabled" | "createdAt" | "sortIndex"
                    )
                });
            } else if doc.id == "activeStylePackId" {
                doc.id = STYLE_REFERENCE.into();
                doc.value = reference.clone();
            }
        }
        // Only the separately protected limited baseline may establish confirmed deletions.
        // Full-snapshot tombstones do not prove that an entry was ever confirmed.
        set.tombstones.clear();
        Ok(Self {
            manifest: Manifest::for_scope(scope)?,
            documents: validate_limited(set, full.documents.observed_revision())?,
        })
    }

    /// Plaintext domain bytes for a future namespaced encrypted transport, NEVER an HTTP body.
    pub fn to_secret_json(&self) -> DocumentResult<Zeroizing<Vec<u8>>> {
        let bytes = serde_json::to_vec(&Envelope {
            manifest: self.manifest.clone(),
            documents: self.documents.documents().clone(),
        })
        .map(Zeroizing::new)
        .map_err(|_| DocumentError::InvalidDocument)?;
        if bytes.len() > MAX_JSON_BYTES {
            return Err(DocumentError::PayloadTooLarge);
        }
        Ok(bytes)
    }

    /// Call only after authenticated decryption. Scope/revision come from authenticated metadata,
    /// not fields supplied by the remote plaintext. Strict decoding rejects duplicate keys.
    pub fn from_authenticated_json(
        bytes: &[u8],
        scope: &SyncScope,
        revision: Revision,
    ) -> DocumentResult<Self> {
        crate::cloud_sync_e2ee_protocol::types::validate_secret_json(bytes, MAX_JSON_BYTES)
            .map_err(|_| DocumentError::InvalidDocument)?;
        let envelope: Envelope =
            serde_json::from_slice(bytes).map_err(|_| DocumentError::InvalidDocument)?;
        if envelope.manifest != Manifest::for_scope(scope)? {
            return Err(DocumentError::InvalidReference);
        }
        Ok(Self {
            manifest: envelope.manifest,
            documents: validate_limited(envelope.documents, revision)?,
        })
    }

    pub(crate) fn project_restored(
        &self,
        full: &ExportedDocuments,
        scope: &SyncScope,
    ) -> DocumentResult<Self> {
        let ids = self
            .documents
            .documents()
            .documents
            .iter()
            .filter(|doc| doc.kind == DocumentKind::Dictionary)
            .map(|doc| doc.id.clone())
            .collect();
        let confirmed = ConfirmedVocabularySelection::new(scope.clone(), full.generation, ids)?;
        let mut projected = Self::project(full, scope, &confirmed)?;
        let mut set = projected.documents.documents().clone();
        set.tombstones = self.documents.documents().tombstones.clone();
        projected.documents = validate_limited(set, self.documents.observed_revision())?;
        Ok(projected)
    }

    pub fn diff(
        &self,
        baseline: Option<&Self>,
        remote: &Self,
    ) -> DocumentResult<LimitedMergePreview> {
        if self.manifest != remote.manifest
            || baseline.is_some_and(|base| base.manifest != self.manifest)
        {
            return Err(DocumentError::InvalidReference);
        }
        Ok(LimitedMergePreview {
            manifest: self.manifest.clone(),
            preview: diff_sync_documents(
                baseline.map(|base| &base.documents),
                &self.documents,
                &remote.documents,
            )?,
        })
    }

    /// Keep a limited baseline separately from the desktop full baseline. Missing IDs require
    /// explicit tombstones; timestamps never decide which device wins.
    pub fn record_local_deletions(
        &self,
        baseline: &Self,
        deleted_at: &str,
    ) -> DocumentResult<Self> {
        if self.manifest != baseline.manifest {
            return Err(DocumentError::InvalidReference);
        }
        let documents = record_missing_tombstones(
            self.documents.documents().clone(),
            &baseline.documents,
            deleted_at,
        )?;
        Ok(Self {
            manifest: self.manifest.clone(),
            documents: validate_limited(
                documents.documents().clone(),
                documents.observed_revision(),
            )?,
        })
    }

    pub(crate) fn overlay(
        &self,
        full: &ExportedDocuments,
        scope: &SyncScope,
        revision: Revision,
        confirmed: &ConfirmedVocabularySelection,
    ) -> DocumentResult<ValidatedSyncDocuments> {
        confirmed.check(full, scope)?;
        if self.manifest != Manifest::for_scope(scope)?
            || self.documents.observed_revision() != revision
        {
            return Err(DocumentError::StalePreview);
        }
        let mut set = full.documents.documents().clone();
        let old_rows = set
            .documents
            .iter()
            .filter(|doc| doc.kind == DocumentKind::Dictionary)
            .map(|doc| (doc.id.clone(), doc.value.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        for id in self
            .documents
            .documents()
            .documents
            .iter()
            .filter(|doc| doc.kind == DocumentKind::Dictionary)
            .map(|doc| &doc.id)
            .chain(
                self.documents
                    .documents()
                    .tombstones
                    .iter()
                    .map(|mark| &mark.id),
            )
        {
            if old_rows.contains_key(id) && !confirmed.ids.contains(id) {
                return Err(DocumentError::InvalidReference);
            }
        }
        set.documents.retain(|doc| {
            !(doc.kind == DocumentKind::Dictionary && confirmed.ids.contains(&doc.id))
                && !(doc.kind == DocumentKind::Preferences
                    && matches!(
                        doc.id.as_str(),
                        "defaultMode" | "enabledModes" | "activeStylePackId"
                    ))
        });
        let remote_ids: BTreeSet<_> = self
            .documents
            .documents()
            .documents
            .iter()
            .filter(|doc| doc.kind == DocumentKind::Dictionary)
            .map(|doc| &doc.id)
            .chain(
                self.documents
                    .documents()
                    .tombstones
                    .iter()
                    .map(|mark| &mark.id),
            )
            .collect();
        if confirmed.ids.iter().any(|id| !remote_ids.contains(id)) {
            return Err(DocumentError::MissingTombstone);
        }
        set.tombstones.retain(|mark| {
            mark.kind != DocumentKind::Dictionary
                || !confirmed.ids.contains(&mark.id) && !remote_ids.contains(&mark.id)
        });
        for source in &self.documents.documents().documents {
            let mut doc = source.clone();
            if doc.kind == DocumentKind::Dictionary {
                let row = doc
                    .value
                    .as_object_mut()
                    .ok_or(DocumentError::InvalidDocument)?;
                row.insert(
                    "note".into(),
                    old_rows
                        .get(&doc.id)
                        .and_then(|old| old.get("note"))
                        .cloned()
                        .unwrap_or(Value::Null),
                );
                row.insert(
                    "hits".into(),
                    old_rows
                        .get(&doc.id)
                        .and_then(|old| old.get("hits"))
                        .cloned()
                        .unwrap_or(Value::from(0)),
                );
            } else if doc.id == STYLE_REFERENCE {
                let reference = style_reference(&doc.value)?;
                if let Some(reference) = &reference {
                    let installed = set
                        .documents
                        .iter()
                        .find(|style| {
                            style.kind == DocumentKind::StylePacks && style.id == reference.id
                        })
                        .ok_or(DocumentError::InvalidReference)?;
                    if StyleReference::from_style(installed)? != *reference
                        || installed.value["pack"]["enabled"] != true
                    {
                        return Err(DocumentError::InvalidReference);
                    }
                }
                doc.id = "activeStylePackId".into();
                doc.value =
                    Value::String(reference.map(|reference| reference.id).unwrap_or_default());
            }
            set.documents.push(doc);
        }
        set.tombstones
            .extend(self.documents.documents().tombstones.clone());
        // Full validation requires the referenced style to be installed. Missing styles fail
        // without changing settings; Host can download/verify via the existing marketplace.
        validate_sync_documents(set, revision)
    }
}

pub struct LimitedMergePreview {
    manifest: Manifest,
    preview: MergePreview,
}
impl LimitedMergePreview {
    pub fn conflicts(&self) -> &[RedactedConflict] {
        self.preview.conflicts()
    }
    pub fn resolve(self, choices: &[ConflictChoice]) -> DocumentResult<LimitedSyncDocuments> {
        let documents = self.preview.resolve(choices)?;
        Ok(LimitedSyncDocuments {
            manifest: self.manifest,
            documents: validate_limited(
                documents.documents().clone(),
                documents.observed_revision(),
            )?,
        })
    }
}
