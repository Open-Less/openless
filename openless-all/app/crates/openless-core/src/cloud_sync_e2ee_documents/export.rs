//! Explicit source-to-document registration. This module never scans user directories.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::cloud_sync_e2ee_protocol::types::{DocumentKind, DocumentSet, LogicalDocument};
use crate::credentials::{CredentialKey, CredentialNamespace, CredentialStore};

use super::registry::{
    credential_accounts, excluded_extension_key, is_device_channel, preference_field,
    PreferenceClass,
};
use super::types::*;
use super::validate::validate_sync_documents;

pub async fn export_sync_documents(
    source: &dyn SyncDocumentSource,
) -> DocumentResult<ExportedDocuments> {
    export_snapshot(source.capture().await?)
}

/// Export an already coherent capture. The caller must not substitute empty stores on errors.
pub fn export_snapshot(mut snapshot: ExportSnapshot) -> DocumentResult<ExportedDocuments> {
    let mut documents = Vec::new();
    let mut device_preferences = Map::new();
    let preferences = snapshot
        .preferences
        .expose()
        .as_object()
        .ok_or(DocumentError::InvalidDocument)?;
    for (key, value) in preferences {
        match preference_field(key).map(|field| field.class) {
            Some(PreferenceClass::Excluded) => {}
            Some(PreferenceClass::DeviceProfile) => {
                device_preferences.insert(key.clone(), value.clone());
            }
            Some(PreferenceClass::Portable) => documents.push(document(
                DocumentKind::Preferences,
                key.clone(),
                value.clone(),
            )),
            None => {
                if excluded_extension_key(key) {
                    return Err(DocumentError::ExcludedField);
                }
                documents.push(document(
                    DocumentKind::Preferences,
                    key.clone(),
                    value.clone(),
                ));
            }
        }
    }
    for (key, value) in snapshot
        .ui_preferences
        .expose()
        .as_object()
        .ok_or(DocumentError::InvalidDocument)?
    {
        if excluded_extension_key(key) {
            return Err(DocumentError::ExcludedField);
        }
        documents.push(document(
            DocumentKind::UiPreferences,
            key.clone(),
            value.clone(),
        ));
    }

    let mut credentials = BTreeMap::new();
    for record in std::mem::take(&mut snapshot.provider_credentials) {
        if credentials.insert(record.document_id(), record).is_some() {
            return Err(DocumentError::DuplicateId);
        }
    }
    let mut seen_channels = BTreeSet::new();
    let mut device_channels = Vec::new();
    let mut device_credentials = Vec::new();
    for channel in std::mem::take(&mut snapshot.channels) {
        let id = channel.document_id();
        if !seen_channels.insert(id.clone()) {
            return Err(DocumentError::DuplicateId);
        }
        let credential = credentials
            .remove(&id)
            .ok_or(DocumentError::InvalidReference)?;
        if is_device_channel(channel.namespace, &channel.provider_type) {
            device_channels.push(channel);
            device_credentials.push(credential);
        } else {
            documents.push(document(
                DocumentKind::Channels,
                id.clone(),
                to_value(&channel)?,
            ));
            documents.push(document(
                DocumentKind::ProviderCredentials,
                id,
                to_value(&credential)?,
            ));
        }
    }
    if !credentials.is_empty() {
        return Err(DocumentError::InvalidReference);
    }

    for (sort_index, mut entry) in std::mem::take(&mut snapshot.dictionary)
        .into_iter()
        .enumerate()
    {
        entry
            .expose_mut()
            .as_object_mut()
            .ok_or(DocumentError::InvalidDocument)?
            .insert("sortIndex".into(), json!(sort_index));
        let id = record_id(entry.expose())?;
        documents.push(document(DocumentKind::Dictionary, id, entry.into_value()));
    }
    for preset in std::mem::take(&mut snapshot.vocabulary_presets) {
        let prefix = match preset.origin {
            PresetOrigin::Custom => "custom",
            PresetOrigin::Override => "override",
            PresetOrigin::BuiltinState => "builtin",
        };
        documents.push(document(
            DocumentKind::VocabularyPresets,
            format!("{prefix}:{}", preset.id),
            to_value(&preset)?,
        ));
    }
    for (sort_index, mut entry) in std::mem::take(&mut snapshot.corrections)
        .into_iter()
        .enumerate()
    {
        entry
            .expose_mut()
            .as_object_mut()
            .ok_or(DocumentError::InvalidDocument)?
            .insert("sortIndex".into(), json!(sort_index));
        let id = record_id(entry.expose())?;
        documents.push(document(DocumentKind::Corrections, id, entry.into_value()));
    }
    for mut entry in std::mem::take(&mut snapshot.style_packs) {
        let id = record_id(entry.pack.expose())?;
        let pack = entry
            .pack
            .expose_mut()
            .as_object_mut()
            .ok_or(DocumentError::InvalidDocument)?;
        pack.remove("iconPath");
        pack.remove("active");
        documents.push(document(DocumentKind::StylePacks, id, to_value(&entry)?));
    }
    for (sort_index, mut entry) in std::mem::take(&mut snapshot.history)
        .into_iter()
        .enumerate()
    {
        entry
            .expose_mut()
            .as_object_mut()
            .ok_or(DocumentError::InvalidDocument)?
            .insert("sortIndex".into(), json!(sort_index));
        let id = record_id(entry.expose())?;
        // Media never travels. Availability is recomputed against this device's recordings.
        let history = entry
            .expose_mut()
            .as_object_mut()
            .ok_or(DocumentError::InvalidDocument)?;
        history.insert("hasAudioRecording".into(), Value::Bool(false));
        documents.push(document(DocumentKind::History, id, entry.into_value()));
    }
    for entry in std::mem::take(&mut snapshot.activity) {
        documents.push(document(
            DocumentKind::Activity,
            entry.document_id(),
            to_value(&entry)?,
        ));
    }
    let profile = DeviceProfileRecord {
        device: snapshot.source_device.clone(),
        preferences: SecretJson::new(Value::Object(device_preferences)),
        channels: device_channels,
        provider_credentials: device_credentials,
        window_positions: std::mem::take(&mut snapshot.window_positions),
    };
    documents.push(document(
        DocumentKind::DeviceProfile,
        snapshot.source_device.id.clone(),
        to_value(&profile)?,
    ));

    // Retained documents must be explicit source profiles or unknown preferences. Never let
    // an archive silently replace live source fields or resurrect a deleted record.
    for retained in std::mem::take(&mut snapshot.retained_documents) {
        let permitted = retained.kind == DocumentKind::DeviceProfile
            || retained.kind == DocumentKind::Preferences
                && preference_field(&retained.id).is_none()
            || retained.kind == DocumentKind::UiPreferences
                && !matches!(retained.id.as_str(), "locale" | "fontScale");
        if !permitted {
            return Err(DocumentError::InvalidDocument);
        }
        if let Some(existing) = documents
            .iter()
            .find(|doc| doc.kind == retained.kind && doc.id == retained.id)
        {
            if existing != &retained {
                return Err(DocumentError::SourceChanged);
            }
        } else {
            documents.push(retained);
        }
    }
    let set = DocumentSet {
        schema_version: DOCUMENT_SCHEMA_VERSION,
        exported_at: snapshot.exported_at,
        source_device: snapshot.source_device,
        documents,
        tombstones: snapshot.tombstones,
    };
    Ok(ExportedDocuments {
        generation: snapshot.generation,
        documents: validate_sync_documents(set, snapshot.base_revision)?,
    })
}

/// Reads only registered logical accounts. Each error aborts the entire export.
/// Omni IDs must enumerate every stored provider entry; None would read only the active one.
pub async fn export_provider_credentials_for_sync(
    store: &dyn CredentialStore,
    channels: &[ChannelRecord],
) -> DocumentResult<Vec<ProviderCredentialRecord>> {
    let mut records = Vec::with_capacity(channels.len());
    let mut seen = BTreeSet::new();
    for channel in channels {
        if !seen.insert(channel.document_id()) {
            return Err(DocumentError::DuplicateId);
        }
        let namespace = match channel.namespace {
            SyncNamespace::Asr => CredentialNamespace::Asr,
            SyncNamespace::Llm => CredentialNamespace::Llm,
            SyncNamespace::Omni => CredentialNamespace::Omni,
        };
        let mut accounts = BTreeMap::new();
        for account in credential_accounts(channel.namespace) {
            let key = CredentialKey::new(namespace, Some(channel.id.clone()), *account)
                .map_err(|_| DocumentError::InvalidDocument)?;
            if let Some(secret) = store
                .read(key)
                .await
                .map_err(|_| DocumentError::CaptureFailed)?
            {
                accounts.insert((*account).to_string(), secret.expose_secret().to_string());
            }
        }
        records.push(ProviderCredentialRecord {
            channel_id: channel.id.clone(),
            namespace: channel.namespace,
            accounts,
        });
    }
    Ok(records)
}

/// Converts a registered store's typed vector to secret rows, without dropping any row.
pub fn secret_rows<T: serde::Serialize>(rows: &[T]) -> DocumentResult<Vec<SecretJson>> {
    rows.iter().map(SecretJson::from_serializable).collect()
}

/// Convert all preset state, including enabled builtins, to independently mergeable IDs.
pub fn vocabulary_records(
    custom: &[crate::types::VocabPreset],
    overrides: &[crate::types::VocabPreset],
    disabled_builtin_ids: &[String],
    builtin_ids: &[String],
) -> DocumentResult<Vec<VocabularyPresetRecord>> {
    if disabled_builtin_ids
        .iter()
        .any(|id| !builtin_ids.contains(id))
    {
        return Err(DocumentError::InvalidReference);
    }
    let mut result = Vec::new();
    for (origin, entries) in [
        (PresetOrigin::Custom, custom),
        (PresetOrigin::Override, overrides),
    ] {
        for (order, entry) in entries.iter().enumerate() {
            result.push(VocabularyPresetRecord {
                id: entry.id.clone(),
                origin,
                name: Some(entry.name.clone()),
                phrases: entry.phrases.clone(),
                enabled: true,
                order: u32::try_from(order).map_err(|_| DocumentError::PayloadTooLarge)?,
            });
        }
    }
    for id in builtin_ids {
        result.push(VocabularyPresetRecord {
            id: id.clone(),
            origin: PresetOrigin::BuiltinState,
            name: None,
            phrases: Vec::new(),
            enabled: !disabled_builtin_ids.contains(id),
            order: 0,
        });
    }
    Ok(result)
}

/// Read icons through the repository's bounded ID-based resource reader, not iconPath.
pub fn export_style_packs(
    store: &crate::style_pack_store::StylePackStore,
) -> DocumentResult<Vec<StylePackRecord>> {
    store
        .list()
        .map_err(|_| DocumentError::CaptureFailed)?
        .iter()
        .map(|pack| {
            let icon = match store
                .icon_data_url(&pack.id)
                .map_err(|_| DocumentError::CaptureFailed)?
            {
                Some(data) => {
                    let (mime, encoded) = data
                        .strip_prefix("data:")
                        .and_then(|s| s.split_once(";base64,"))
                        .ok_or(DocumentError::InvalidDocument)?;
                    Some(IconAsset {
                        mime: mime.to_string(),
                        base64: encoded.to_string(),
                    })
                }
                None if pack.icon_path.is_some() => return Err(DocumentError::CaptureFailed),
                None => None,
            };
            Ok(StylePackRecord {
                pack: SecretJson::from_serializable(pack)?,
                icon,
            })
        })
        .collect()
}

pub(crate) fn document(kind: DocumentKind, id: String, value: Value) -> LogicalDocument {
    LogicalDocument {
        id,
        kind,
        schema_version: DOCUMENT_SCHEMA_VERSION,
        value,
    }
}

pub(crate) fn to_value(value: &impl serde::Serialize) -> DocumentResult<Value> {
    serde_json::to_value(value).map_err(|_| DocumentError::InvalidDocument)
}

pub(crate) fn record_id(value: &Value) -> DocumentResult<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or(DocumentError::InvalidDocument)
}

/// A stable per-source aggregate, not a sum of two already merged device totals.
pub fn activity_record(
    source_device_id: &str,
    day: &crate::activity::ActivityDay,
) -> ActivityRecord {
    ActivityRecord {
        source_device_id: source_device_id.into(),
        date: day.date.clone(),
        count: day.count.into(),
        chars: day.chars,
        duration_ms: day.duration_ms,
    }
}

/// Convenience for UI bridges; unrelated localStorage keys must never be passed here.
pub fn ui_preferences(locale: &str, font_scale: &str) -> SecretJson {
    SecretJson::new(json!({"locale": locale, "fontScale": font_scale}))
}
