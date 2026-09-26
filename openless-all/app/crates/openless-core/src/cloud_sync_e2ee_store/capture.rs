use std::collections::BTreeSet;

use crate::cloud_sync_e2ee_documents::*;
use crate::cloud_sync_e2ee_protocol::types::{DocumentSet, Revision, Tombstone};

use super::{state::ScopeState, CoreSyncStore, DeviceExtensionKey, ExclusivePermit};

impl CoreSyncStore {
    pub(super) async fn capture_locked(
        &self,
        scope: &SyncScope,
        permit: &ExclusivePermit,
    ) -> DocumentResult<ExportedDocuments> {
        let state = ScopeState::decode(
            scope,
            self.inner.extensions.read_scope(scope.clone()).await?,
        )?;
        let snapshot = self.capture_native(scope, permit, &state).await?;
        self.finish_capture(scope, state, snapshot).await
    }

    pub(super) async fn capture_readonly(
        &self,
        scope: &SyncScope,
    ) -> DocumentResult<ExportedDocuments> {
        let token = self.inner.gate.coherent_generation()?;
        let state = ScopeState::decode(
            scope,
            self.inner.extensions.read_scope(scope.clone()).await?,
        )?;
        let captured = self.capture_native_inner(None, &state, token.0).await;
        if self.inner.gate.coherent_generation()? != token {
            return Err(DocumentError::SourceChanged);
        }
        self.finish_capture(scope, state, captured?).await
    }

    async fn finish_capture(
        &self,
        scope: &SyncScope,
        mut state: ScopeState,
        mut snapshot: ExportSnapshot,
    ) -> DocumentResult<ExportedDocuments> {
        let old_tombstones = std::mem::take(&mut snapshot.tombstones);
        let mut exported = export_snapshot(snapshot)?;
        let live: BTreeSet<_> = exported
            .documents
            .documents()
            .documents
            .iter()
            .map(|doc| (doc.kind, doc.id.clone()))
            .collect();
        let mut set = exported.documents.documents().clone();
        set.tombstones = old_tombstones
            .into_iter()
            .filter(|tombstone| !live.contains(&(tombstone.kind, tombstone.id.clone())))
            .collect();
        exported.documents = validate_sync_documents(set, state.observed_revision)?;
        if let Some(baseline) = &state.baseline {
            let mut current = exported.documents.documents().clone();
            let mut present: BTreeSet<_> = current
                .documents
                .iter()
                .map(|doc| (doc.kind, doc.id.clone()))
                .chain(
                    current
                        .tombstones
                        .iter()
                        .map(|doc| (doc.kind, doc.id.clone())),
                )
                .collect();
            for tombstone in &baseline.tombstones {
                if present.insert((tombstone.kind, tombstone.id.clone())) {
                    current.tombstones.push(tombstone.clone());
                }
            }
            for old in &baseline.documents {
                if present.insert((old.kind, old.id.clone())) {
                    current.tombstones.push(Tombstone {
                        id: old.id.clone(),
                        kind: old.kind,
                        deleted_at: chrono::Utc::now().to_rfc3339(),
                        base_revision: state.baseline_revision,
                    });
                }
            }
            exported.documents = validate_sync_documents(
                current,
                state.observed_revision.max(state.baseline_revision),
            )?;
        }
        if exported.documents.documents().tombstones != state.tombstones {
            state.tombstones = exported.documents.documents().tombstones.clone();
            self.inner
                .extensions
                .write_scope(scope.clone(), state.secret_json()?)
                .await?;
        }
        Ok(exported)
    }

    pub(super) async fn capture_native(
        &self,
        _scope: &SyncScope,
        permit: &ExclusivePermit,
        state: &ScopeState,
    ) -> DocumentResult<ExportSnapshot> {
        self.capture_native_inner(Some(permit), state, permit.generation()?)
            .await
    }

    async fn capture_native_inner(
        &self,
        permit: Option<&ExclusivePermit>,
        state: &ScopeState,
        generation: Revision,
    ) -> DocumentResult<ExportSnapshot> {
        let repositories = &self.inner.repositories;
        let preferences = SecretJson::new(
            match permit {
                Some(permit) => repositories.preferences.sync_snapshot(permit),
                None => repositories.preferences.sync_snapshot_readonly(),
            }
            .map_err(|error| capture_backend_error("preferences", error))?,
        );
        let dictionary = secret_rows(
            &match permit {
                Some(permit) => repositories.vocabulary.sync_snapshot(permit),
                None => repositories.vocabulary.list(),
            }
            .map_err(|error| capture_backend_error("dictionary", error))?,
        )?;
        let corrections = secret_rows(
            &match permit {
                Some(permit) => repositories.correction_rules.sync_snapshot(permit),
                None => repositories.correction_rules.list(),
            }
            .map_err(|error| capture_backend_error("corrections", error))?,
        )?;
        let history = secret_rows(
            &match permit {
                Some(permit) => repositories.history.sync_snapshot(permit),
                None => repositories.history.list(),
            }
            .map_err(|error| capture_backend_error("history", error))?,
        )?;
        let style_packs = match permit {
            Some(permit) => repositories.style_packs.sync_snapshot(permit),
            None => repositories.style_packs.sync_snapshot_readonly(),
        }
        .map_err(|error| capture_backend_error("style_packs", error))?;
        let activity = match permit {
            Some(permit) => repositories.activity.sync_records(permit),
            None => repositories.activity.sync_records_readonly(),
        }
        .map_err(|error| capture_backend_error("activity", error))?;
        let presets = crate::vocabulary::list_vocab_presets(&self.inner.data_dir)
            .map_err(|error| capture_backend_error("vocabulary_presets", error))?;
        let builtin_ids: Vec<_> = crate::vocabulary::builtin_vocab_presets()
            .into_iter()
            .map(|preset| preset.id)
            .collect();
        let vocabulary_presets = vocabulary_records(
            &presets.custom,
            &presets.overrides,
            &presets.disabled_builtin_preset_ids,
            &builtin_ids,
        )?;
        let ui_preferences = self
            .inner
            .extensions
            .read_device(DeviceExtensionKey::UiPreferences)
            .await?
            .ok_or(DocumentError::Unsupported)?;
        let window_positions = match self
            .inner
            .extensions
            .read_device(DeviceExtensionKey::WindowPositions)
            .await?
        {
            Some(value) => serde_json::from_value(value.expose().clone())
                .map_err(|_| DocumentError::InvalidDocument)?,
            None => Vec::new(),
        };
        let credentials = match permit {
            Some(permit) => self.inner.credentials.export_sync_credentials(permit).await,
            None => {
                self.inner
                    .credentials
                    .export_sync_credentials_readonly()
                    .await
            }
        }
        .map_err(|error| capture_backend_error("credentials", error))?;
        validate_credential_set(&credentials.channels, &credentials.credentials)?;
        Ok(ExportSnapshot {
            source_device: self.inner.device.clone(),
            exported_at: chrono::Utc::now().to_rfc3339(),
            generation,
            base_revision: state.observed_revision,
            preferences,
            ui_preferences,
            channels: credentials.channels,
            provider_credentials: credentials.credentials,
            dictionary,
            vocabulary_presets,
            corrections,
            style_packs,
            history,
            activity,
            window_positions,
            retained_documents: state.retained_documents.clone(),
            tombstones: state.tombstones.clone(),
        })
    }

    pub(super) async fn baseline_locked(
        &self,
        scope: &SyncScope,
        documents: DocumentSet,
        revision: Revision,
    ) -> DocumentResult<()> {
        let validated = validate_sync_documents(documents, revision)?;
        let mut state = ScopeState::decode(
            scope,
            self.inner.extensions.read_scope(scope.clone()).await?,
        )?;
        if revision < state.baseline_revision {
            return Err(DocumentError::StalePreview);
        }
        state.baseline_revision = revision;
        state.observed_revision = state.observed_revision.max(revision);
        state.baseline = Some(validated.documents().clone());
        // Do not replace retained documents or local tombstones: later local changes may exist.
        self.inner
            .extensions
            .write_scope(scope.clone(), state.secret_json()?)
            .await
    }
}

/// Values, paths, record IDs and error bodies may contain private user data.
/// A capture diagnostic therefore records only an audited stage and enum code.
fn capture_backend_error(stage: &'static str, error: crate::BackendError) -> DocumentError {
    log::warn!("[e2ee-capture] stage={stage} code={:?}", error.code);
    DocumentError::CaptureFailed
}
