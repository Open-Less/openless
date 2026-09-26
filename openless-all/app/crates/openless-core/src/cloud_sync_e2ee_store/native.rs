//! Materialization is preflighted completely before the first repository write.
use super::{state::ScopeState, CoreSyncStore, DeviceExtensionKey, ExclusivePermit};
use crate::cloud_sync_e2ee_documents::registry::{preference_field, PreferenceClass};
use crate::cloud_sync_e2ee_documents::*;
use crate::cloud_sync_e2ee_protocol::types::{DocumentKind, LogicalDocument};
use crate::credentials::SyncCredentials;
use crate::types::{
    CorrectionRule, DictationSession, DictionaryEntry, VocabPreset, VocabPresetStore,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

struct NativeRestore {
    preferences: Value,
    ui: SecretJson,
    windows: SecretJson,
    credentials: SyncCredentials,
    dictionary: Vec<DictionaryEntry>,
    corrections: Vec<CorrectionRule>,
    history: Vec<DictationSession>,
    presets: VocabPresetStore,
    styles: Vec<StylePackRecord>,
    activity: Vec<ActivityRecord>,
    state: ScopeState,
}

fn typed<T: DeserializeOwned>(value: &Value) -> DocumentResult<T> {
    serde_json::from_value(value.clone()).map_err(|_| DocumentError::Unsupported)
}
fn ordered<T: DeserializeOwned>(
    documents: &[LogicalDocument],
    kind: DocumentKind,
) -> DocumentResult<Vec<T>> {
    let mut values: Vec<_> = documents.iter().filter(|doc| doc.kind == kind).collect();
    values.sort_by_key(|doc| {
        doc.value
            .get("sortIndex")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    });
    values.into_iter().map(|doc| typed(&doc.value)).collect()
}

pub(super) fn canonicalize_native_documents(
    desired: ValidatedSyncDocuments,
) -> DocumentResult<ValidatedSyncDocuments> {
    let mut set = desired.documents().clone();
    for doc in &mut set.documents {
        let original = doc.value.clone();
        let mut canonical = match doc.kind {
            DocumentKind::Dictionary => serde_json::to_value(typed::<DictionaryEntry>(&original)?),
            DocumentKind::Corrections => serde_json::to_value(typed::<CorrectionRule>(&original)?),
            DocumentKind::History => serde_json::to_value(typed::<DictationSession>(&original)?),
            DocumentKind::StylePacks => {
                let mut record: StylePackRecord = typed(&original)?;
                let pack: crate::style_packs::StylePack = typed(record.pack.expose())?;
                let mut canonical =
                    serde_json::to_value(pack).map_err(|_| DocumentError::InvalidDocument)?;
                crate::persistence::ensure_lossless_value(record.pack.expose(), &canonical, &[])
                    .map_err(|_| DocumentError::Unsupported)?;
                canonical
                    .as_object_mut()
                    .ok_or(DocumentError::InvalidDocument)?
                    .remove("iconPath");
                canonical
                    .as_object_mut()
                    .ok_or(DocumentError::InvalidDocument)?
                    .remove("active");
                record.pack = SecretJson::new(canonical);
                serde_json::to_value(record)
            }
            _ => continue,
        }
        .map_err(|_| DocumentError::InvalidDocument)?;
        crate::persistence::ensure_lossless_value(&original, &canonical, &["sortIndex"])
            .map_err(|_| DocumentError::Unsupported)?;
        if let Some(index) = original.get("sortIndex") {
            canonical
                .as_object_mut()
                .ok_or(DocumentError::InvalidDocument)?
                .insert("sortIndex".into(), index.clone());
        }
        doc.value = canonical;
    }
    validate_sync_documents(set, desired.observed_revision())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalRollbackState {
    schema_version: u32,
    coding_agent_enabled: bool,
    active_asr_provider: String,
    active_llm_provider: String,
    active_omni_provider: String,
    /// Receiver-only availability; no audio bytes or paths ever enter the journal.
    history_audio_markers: std::collections::BTreeMap<String, Option<bool>>,
}

impl CoreSyncStore {
    pub(super) fn capture_local_rollback_state(
        &self,
        permit: &ExclusivePermit,
    ) -> DocumentResult<SecretJson> {
        let value = self
            .inner
            .repositories
            .preferences
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        let prefs: crate::shared_types::UserPreferences = typed(&value)?;
        let history = self
            .inner
            .repositories
            .history
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        let mut history_audio_markers = std::collections::BTreeMap::new();
        for record in history {
            if history_audio_markers
                .insert(record.id, record.has_audio_recording)
                .is_some()
            {
                return Err(DocumentError::DuplicateId);
            }
        }
        SecretJson::from_serializable(&LocalRollbackState {
            schema_version: 1,
            coding_agent_enabled: prefs.coding_agent_enabled,
            active_asr_provider: prefs.active_asr_provider,
            active_llm_provider: prefs.active_llm_provider,
            active_omni_provider: prefs.active_omni_provider,
            history_audio_markers,
        })
    }
    pub(super) fn restore_local_rollback_state(
        &self,
        value: &SecretJson,
        permit: &ExclusivePermit,
    ) -> DocumentResult<()> {
        let local: LocalRollbackState = typed(value.expose())?;
        if local.schema_version != 1 {
            return Err(DocumentError::Unsupported);
        }
        // Logical rollback restores the rows first. Require the same complete identity set,
        // then restore only this receiver's original metadata, never a cloud media flag.
        let mut history = self
            .inner
            .repositories
            .history
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        let ids: std::collections::BTreeSet<_> =
            history.iter().map(|record| record.id.as_str()).collect();
        if ids.len() != history.len()
            || history.len() != local.history_audio_markers.len()
            || !ids
                .iter()
                .all(|id| local.history_audio_markers.contains_key(*id))
        {
            return Err(DocumentError::RecoveryRequired);
        }
        for record in &mut history {
            record.has_audio_recording = local.history_audio_markers[&record.id];
        }
        self.inner
            .repositories
            .history
            .sync_replace_all(&history, permit)
            .map_err(|_| DocumentError::RecoveryRequired)?;
        let mut current = self
            .inner
            .repositories
            .preferences
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        let object = current
            .as_object_mut()
            .ok_or(DocumentError::InvalidDocument)?;
        object.insert(
            "codingAgentEnabled".into(),
            Value::Bool(local.coding_agent_enabled),
        );
        object.insert(
            "activeAsrProvider".into(),
            Value::String(local.active_asr_provider),
        );
        object.insert(
            "activeLlmProvider".into(),
            Value::String(local.active_llm_provider),
        );
        object.insert(
            "activeOmniProvider".into(),
            Value::String(local.active_omni_provider),
        );
        self.inner
            .repositories
            .preferences
            .sync_replace_raw(current, permit)
            .map_err(|_| DocumentError::RecoveryRequired)
    }

    async fn preflight_native(
        &self,
        scope: &SyncScope,
        desired: &ValidatedSyncDocuments,
        permit: &ExclusivePermit,
    ) -> DocumentResult<NativeRestore> {
        let mut state = ScopeState::decode(
            scope,
            self.inner.extensions.read_scope(scope.clone()).await?,
        )?;
        let current = self
            .inner
            .repositories
            .preferences
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        let mut preferences = current.clone();
        let map = preferences
            .as_object_mut()
            .ok_or(DocumentError::InvalidDocument)?;
        // Unknown preferences have an encrypted home; never spread future keys into native files.
        map.retain(|key, _| preference_field(key).is_some());
        let mut retained = Vec::new();
        let mut ui = serde_json::Map::new();
        let mut channels = Vec::new();
        let mut credentials = Vec::new();
        let mut windows = Vec::<WindowPosition>::new();
        let mut styles = Vec::<StylePackRecord>::new();
        let mut activity = Vec::<ActivityRecord>::new();
        let mut preset_records = Vec::<VocabularyPresetRecord>::new();
        let docs = &desired.documents().documents;
        for doc in docs {
            match doc.kind {
                DocumentKind::Preferences => match preference_field(&doc.id) {
                    Some(field) if field.class == PreferenceClass::Portable => {
                        map.insert(doc.id.clone(), doc.value.clone());
                    }
                    None => retained.push(doc.clone()),
                    _ => return Err(DocumentError::ExcludedField),
                },
                DocumentKind::UiPreferences
                    if matches!(doc.id.as_str(), "locale" | "fontScale") =>
                {
                    ui.insert(doc.id.clone(), doc.value.clone());
                }
                DocumentKind::UiPreferences => retained.push(doc.clone()),
                DocumentKind::Channels => channels.push(typed(&doc.value)?),
                DocumentKind::ProviderCredentials => credentials.push(typed(&doc.value)?),
                DocumentKind::DeviceProfile if doc.id == self.inner.device.id => {
                    let profile: DeviceProfileRecord = typed(&doc.value)?;
                    if profile.device.os != self.inner.device.os
                        || profile.device.arch != self.inner.device.arch
                    {
                        return Err(DocumentError::Unsupported);
                    }
                    for (key, value) in profile
                        .preferences
                        .expose()
                        .as_object()
                        .ok_or(DocumentError::InvalidDocument)?
                    {
                        if preference_field(key)
                            .is_none_or(|field| field.class != PreferenceClass::DeviceProfile)
                        {
                            return Err(DocumentError::Unsupported);
                        }
                        map.insert(key.clone(), value.clone());
                    }
                    channels.extend(profile.channels);
                    credentials.extend(profile.provider_credentials);
                    windows = profile.window_positions;
                }
                DocumentKind::DeviceProfile => retained.push(doc.clone()),
                DocumentKind::StylePacks => {
                    let record: StylePackRecord = typed(&doc.value)?;
                    let pack: crate::style_packs::StylePack = typed(record.pack.expose())?;
                    let known_builtin = crate::style_packs::builtin_style_packs()
                        .iter()
                        .any(|builtin| builtin.id == pack.id);
                    if known_builtin != (pack.kind == crate::style_packs::StylePackKind::Builtin) {
                        return Err(DocumentError::Unsupported);
                    }
                    if let Some(icon) = &record.icon {
                        validate_icon(icon)?;
                    }
                    styles.push(record);
                }
                DocumentKind::Activity => {
                    let record: ActivityRecord = typed(&doc.value)?;
                    u32::try_from(record.count).map_err(|_| DocumentError::Unsupported)?;
                    activity.push(record);
                }
                DocumentKind::VocabularyPresets => preset_records.push(typed(&doc.value)?),
                DocumentKind::Dictionary | DocumentKind::Corrections | DocumentKind::History => {}
            }
        }
        if styles.is_empty()
            || !styles.iter().any(|style| {
                style.pack.expose().get("enabled").and_then(Value::as_bool) == Some(true)
            })
        {
            return Err(DocumentError::InvalidReference);
        }
        // Restored executable/workspace data never silently activates an existing permission.
        if [
            "codingAgentProvider",
            "codingAgentModel",
            "codingAgentPermissionMode",
            "codingAgentExe",
            "codingAgentWorkdir",
        ]
        .iter()
        .any(|key| current.get(key) != map.get(*key))
        {
            map.insert("codingAgentEnabled".into(), Value::Bool(false));
        }
        validate_credential_set(&channels, &credentials)?;
        for (namespace, key) in [
            (SyncNamespace::Asr, "activeAsrProvider"),
            (SyncNamespace::Llm, "activeLlmProvider"),
            (SyncNamespace::Omni, "activeOmniProvider"),
        ] {
            let active = channels
                .iter()
                .find(|channel| channel.namespace == namespace && channel.active)
                .map(|channel| channel.id.clone())
                .unwrap_or_default();
            map.insert(key.into(), Value::String(active));
        }
        let _: crate::shared_types::UserPreferences = typed(&preferences)?;
        if !ui.contains_key("locale") || !ui.contains_key("fontScale") {
            return Err(DocumentError::Unsupported);
        }
        preset_records.sort_by_key(|record| (record.order, record.id.clone()));
        let builtin_ids: Vec<_> = crate::vocabulary::builtin_vocab_presets()
            .into_iter()
            .map(|item| item.id)
            .collect();
        let mut presets = VocabPresetStore::default();
        for record in preset_records {
            match record.origin {
                PresetOrigin::BuiltinState => {
                    if !builtin_ids.contains(&record.id) {
                        return Err(DocumentError::Unsupported);
                    }
                    if !record.enabled {
                        presets.disabled_builtin_preset_ids.push(record.id);
                    }
                }
                origin => {
                    if !record.enabled {
                        return Err(DocumentError::Unsupported);
                    }
                    let preset = VocabPreset {
                        id: record.id,
                        name: record.name.ok_or(DocumentError::InvalidDocument)?,
                        phrases: record.phrases,
                    };
                    if origin == PresetOrigin::Custom {
                        presets.custom.push(preset);
                    } else {
                        presets.overrides.push(preset);
                    }
                }
            }
        }
        // Availability is local metadata, never imported from the cloud. Existing records
        // keep the receiver's own marker; newly restored records never claim remote media.
        let local_history = self
            .inner
            .repositories
            .history
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        let local_media: std::collections::BTreeMap<_, _> = local_history
            .iter()
            .map(|record| (record.id.as_str(), record.has_audio_recording))
            .collect();
        let mut history: Vec<DictationSession> = ordered(docs, DocumentKind::History)?;
        for record in &mut history {
            record.has_audio_recording = local_media
                .get(record.id.as_str())
                .copied()
                .unwrap_or(Some(false));
        }
        state.retained_documents = retained;
        state.tombstones = desired.documents().tombstones.clone();
        state.observed_revision = desired.observed_revision();
        Ok(NativeRestore {
            preferences,
            ui: SecretJson::new(Value::Object(ui)),
            windows: SecretJson::from_serializable(&windows)?,
            credentials: SyncCredentials {
                channels,
                credentials,
            },
            dictionary: ordered(docs, DocumentKind::Dictionary)?,
            corrections: ordered(docs, DocumentKind::Corrections)?,
            history,
            presets,
            styles,
            activity,
            state,
        })
    }

    pub(super) async fn validate_native(
        &self,
        scope: &SyncScope,
        desired: &ValidatedSyncDocuments,
        permit: &ExclusivePermit,
    ) -> DocumentResult<()> {
        self.preflight_native(scope, desired, permit)
            .await
            .map(|_| ())
    }

    pub(super) async fn replace_native(
        &self,
        scope: &SyncScope,
        desired: &ValidatedSyncDocuments,
        permit: &ExclusivePermit,
    ) -> DocumentResult<()> {
        let prepared = self.preflight_native(scope, desired, permit).await?;
        let repositories = &self.inner.repositories;
        // The durable before/after journal and recovery marker already exist here.
        repositories
            .vocabulary
            .sync_replace_all(&prepared.dictionary, permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        repositories
            .correction_rules
            .sync_replace_all(&prepared.corrections, permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        repositories
            .history
            .sync_replace_all(&prepared.history, permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        repositories
            .activity
            .sync_replace_records(&prepared.activity, permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        crate::vocabulary::save_vocab_presets_for_sync(
            &self.inner.data_dir,
            &prepared.presets,
            permit,
        )
        .map_err(|_| DocumentError::CaptureFailed)?;
        repositories
            .style_packs
            .sync_replace_all(&prepared.styles, permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        repositories
            .preferences
            .sync_replace_raw(prepared.preferences, permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        self.inner
            .credentials
            .replace_sync_credentials(prepared.credentials, permit)
            .await
            .map_err(|_| DocumentError::CaptureFailed)?;
        self.inner
            .extensions
            .write_device(DeviceExtensionKey::UiPreferences, prepared.ui)
            .await?;
        self.inner
            .extensions
            .write_device(DeviceExtensionKey::WindowPositions, prepared.windows)
            .await?;
        self.inner
            .extensions
            .write_scope(scope.clone(), prepared.state.secret_json()?)
            .await
    }
}
