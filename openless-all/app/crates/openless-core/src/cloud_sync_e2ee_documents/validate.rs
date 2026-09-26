//! Validation after authenticated decryption and before any local side effect.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::cloud_sync_e2ee_protocol::types::{
    DocumentKind, DocumentSet, LogicalDocument, Revision, SourceDevice,
};

use super::registry::{
    credential_accounts, excluded_extension_key, is_device_channel, preference_field,
    validate_preference_value, PreferenceClass,
};
use super::types::*;

/// Input must come from the protocol's duplicate-key/UTF-8/depth checked AEAD decoder.
pub fn validate_sync_documents(
    mut set: DocumentSet,
    observed_revision: Revision,
) -> DocumentResult<ValidatedSyncDocuments> {
    if set.schema_version != DOCUMENT_SCHEMA_VERSION
        || set
            .documents
            .iter()
            .any(|doc| doc.schema_version != DOCUMENT_SCHEMA_VERSION)
    {
        return Err(DocumentError::Unsupported);
    }
    if set.documents.len().saturating_add(set.tombstones.len()) > MAX_DOCUMENTS {
        return Err(DocumentError::PayloadTooLarge);
    }
    timestamp(&set.exported_at)?;
    source_device(&set.source_device)?;
    let mut seen = BTreeSet::new();
    for doc in &set.documents {
        identifier(&doc.id)?;
        if !seen.insert((doc.kind, doc.id.clone())) {
            return Err(DocumentError::DuplicateId);
        }
        value_depth(&doc.value)?;
        validate_document(doc)?;
    }
    for tombstone in &set.tombstones {
        identifier(&tombstone.id)?;
        timestamp(&tombstone.deleted_at)?;
        if tombstone.base_revision > observed_revision {
            return Err(DocumentError::InvalidDocument);
        }
        if !seen.insert((tombstone.kind, tombstone.id.clone())) {
            return Err(DocumentError::DuplicateId);
        }
    }
    normalize_collection_order(&mut set)?;
    let mut limit = SizeLimit { bytes: 0 };
    serde_json::to_writer(&mut limit, &set).map_err(|_| DocumentError::PayloadTooLarge)?;
    validate_references(&set)?;
    set.documents
        .sort_by(|left, right| (left.kind, &left.id).cmp(&(right.kind, &right.id)));
    set.tombstones
        .sort_by(|left, right| (left.kind, &left.id).cmp(&(right.kind, &right.id)));
    Ok(ValidatedSyncDocuments {
        set,
        revision: observed_revision,
    })
}

struct SizeLimit {
    bytes: usize,
}
impl Write for SizeLimit {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(data.len())
            .ok_or_else(|| io::Error::other("limit"))?;
        if self.bytes > MAX_JSON_BYTES {
            return Err(io::Error::other("limit"));
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn value_depth(root: &Value) -> DocumentResult<()> {
    let mut stack = vec![(root, 1usize)];
    while let Some((value, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return Err(DocumentError::PayloadTooLarge);
        }
        match value {
            Value::Array(values) => stack.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => stack.extend(values.values().map(|v| (v, depth + 1))),
            Value::Number(number)
                if number.is_f64() && !number.as_f64().is_some_and(f64::is_finite) =>
            {
                return Err(DocumentError::InvalidDocument)
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn identifier(value: &str) -> DocumentResult<()> {
    if value.is_empty()
        || value.len() > 512
        || matches!(value, "." | "..")
        || value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(DocumentError::InvalidDocument);
    }
    Ok(())
}

pub(crate) fn timestamp(value: &str) -> DocumentResult<()> {
    let time =
        chrono::DateTime::parse_from_rfc3339(value).map_err(|_| DocumentError::InvalidDocument)?;
    if time.offset().local_minus_utc() != 0 {
        return Err(DocumentError::InvalidDocument);
    }
    Ok(())
}

fn source_device(device: &SourceDevice) -> DocumentResult<()> {
    identifier(&device.id)?;
    for text in [&device.os, &device.arch, &device.app_version] {
        if text.is_empty() || text.len() > 128 || text.chars().any(char::is_control) {
            return Err(DocumentError::InvalidDocument);
        }
    }
    Ok(())
}

pub(crate) fn deserialize<T: DeserializeOwned>(value: &Value) -> DocumentResult<T> {
    serde_json::from_value(value.clone()).map_err(|_| DocumentError::InvalidDocument)
}

fn object<'a>(value: &'a Value, allowed: &[&str]) -> DocumentResult<&'a Map<String, Value>> {
    let object = value.as_object().ok_or(DocumentError::InvalidDocument)?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(DocumentError::Unsupported);
    }
    Ok(object)
}

fn required_text<'a>(object: &'a Map<String, Value>, key: &str) -> DocumentResult<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(DocumentError::InvalidDocument)
}

fn optional_text(object: &Map<String, Value>, keys: &[&str]) -> DocumentResult<()> {
    for key in keys {
        if object
            .get(*key)
            .is_some_and(|value| !value.is_null() && !value.is_string())
        {
            return Err(DocumentError::InvalidDocument);
        }
    }
    Ok(())
}

fn optional_unsigned(object: &Map<String, Value>, keys: &[&str]) -> DocumentResult<()> {
    for key in keys {
        if object
            .get(*key)
            .is_some_and(|value| !value.is_null() && value.as_u64().is_none())
        {
            return Err(DocumentError::InvalidDocument);
        }
    }
    Ok(())
}

fn optional_bool(object: &Map<String, Value>, keys: &[&str]) -> DocumentResult<()> {
    for key in keys {
        if object
            .get(*key)
            .is_some_and(|value| !value.is_null() && !value.is_boolean())
        {
            return Err(DocumentError::InvalidDocument);
        }
    }
    Ok(())
}

fn choice(value: &str, choices: &[&str]) -> DocumentResult<()> {
    if choices.contains(&value) {
        Ok(())
    } else {
        Err(DocumentError::InvalidDocument)
    }
}

fn validate_document(doc: &LogicalDocument) -> DocumentResult<()> {
    match doc.kind {
        DocumentKind::Preferences => {
            if let Some(field) = preference_field(&doc.id) {
                if field.class != PreferenceClass::Portable {
                    return Err(DocumentError::ExcludedField);
                }
                validate_preference_value(field, &doc.value)?;
            } else if excluded_extension_key(&doc.id) {
                return Err(DocumentError::ExcludedField);
            }
        }
        DocumentKind::UiPreferences => match doc.id.as_str() {
            "locale" => choice(
                doc.value.as_str().ok_or(DocumentError::InvalidDocument)?,
                &[
                    "system", "zh-CN", "zh-TW", "en", "ja", "ko", "es", "fr", "de",
                ],
            )?,
            "fontScale" => choice(
                doc.value.as_str().ok_or(DocumentError::InvalidDocument)?,
                &["small", "medium", "large"],
            )?,
            _ if excluded_extension_key(&doc.id) => return Err(DocumentError::ExcludedField),
            _ => {}
        },
        DocumentKind::Channels => {
            let channel: ChannelRecord = deserialize(&doc.value)?;
            validate_channel(&channel)?;
            if channel.document_id() != doc.id
                || is_device_channel(channel.namespace, &channel.provider_type)
            {
                return Err(DocumentError::InvalidDocument);
            }
        }
        DocumentKind::ProviderCredentials => {
            let credentials: ProviderCredentialRecord = deserialize(&doc.value)?;
            validate_credentials(&credentials)?;
            if credentials.document_id() != doc.id {
                return Err(DocumentError::InvalidReference);
            }
        }
        DocumentKind::Dictionary => {
            let row = object(
                &doc.value,
                &[
                    "id",
                    "phrase",
                    "note",
                    "enabled",
                    "hits",
                    "createdAt",
                    "sortIndex",
                ],
            )?;
            if required_text(row, "id")? != doc.id {
                return Err(DocumentError::InvalidReference);
            }
            required_text(row, "phrase")?;
            optional_text(row, &["note", "createdAt"])?;
            optional_bool(row, &["enabled"])?;
            optional_unsigned(row, &["hits"])?;
        }
        DocumentKind::VocabularyPresets => {
            let preset: VocabularyPresetRecord = deserialize(&doc.value)?;
            identifier(&preset.id)?;
            let prefix = match preset.origin {
                PresetOrigin::Custom => "custom",
                PresetOrigin::Override => "override",
                PresetOrigin::BuiltinState => "builtin",
            };
            if doc.id != format!("{prefix}:{}", preset.id) {
                return Err(DocumentError::InvalidReference);
            }
            if preset.origin == PresetOrigin::BuiltinState {
                if preset.name.is_some() || !preset.phrases.is_empty() || preset.order != 0 {
                    return Err(DocumentError::InvalidDocument);
                }
            } else if preset.name.as_deref().is_none_or(str::is_empty) {
                return Err(DocumentError::InvalidDocument);
            }
        }
        DocumentKind::Corrections => {
            let row = object(
                &doc.value,
                &[
                    "id",
                    "pattern",
                    "replacement",
                    "enabled",
                    "createdAt",
                    "source",
                    "sortIndex",
                ],
            )?;
            if required_text(row, "id")? != doc.id {
                return Err(DocumentError::InvalidReference);
            }
            required_text(row, "pattern")?;
            required_text(row, "replacement")?;
            optional_text(row, &["createdAt"])?;
            optional_bool(row, &["enabled"])?;
            if let Some(value) = row.get("source") {
                choice(
                    value.as_str().ok_or(DocumentError::InvalidDocument)?,
                    &["manual", "learned"],
                )?;
            }
        }
        DocumentKind::StylePacks => validate_style(doc)?,
        DocumentKind::History => validate_history(doc)?,
        DocumentKind::Activity => {
            let day: ActivityRecord = deserialize(&doc.value)?;
            identifier(&day.source_device_id)?;
            let parsed = chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
                .map_err(|_| DocumentError::InvalidDocument)?;
            if parsed.format("%Y-%m-%d").to_string() != day.date || day.document_id() != doc.id {
                return Err(DocumentError::InvalidDocument);
            }
        }
        DocumentKind::DeviceProfile => validate_profile(doc)?,
    }
    Ok(())
}

fn validate_channel(channel: &ChannelRecord) -> DocumentResult<()> {
    identifier(&channel.id)?;
    identifier(&channel.provider_type)?;
    if channel.active && !channel.enabled {
        return Err(DocumentError::InvalidDocument);
    }
    if channel.namespace == SyncNamespace::Omni && channel.id != channel.provider_type {
        return Err(DocumentError::InvalidReference);
    }
    Ok(())
}

/// Common native-vault batch contract, including device-local ASR entries.
/// Provider-specific headers/temperature/protocol representability is checked by the Host.
pub fn validate_credential_set(
    channels: &[ChannelRecord],
    credentials: &[ProviderCredentialRecord],
) -> DocumentResult<()> {
    let mut channel_ids = BTreeSet::new();
    let mut credential_ids = BTreeSet::new();
    let mut active = BTreeSet::new();
    for channel in channels {
        validate_channel(channel)?;
        if !channel_ids.insert(channel.document_id()) {
            return Err(DocumentError::DuplicateId);
        }
        if channel.active && !active.insert(channel.namespace) {
            return Err(DocumentError::InvalidReference);
        }
    }
    for record in credentials {
        identifier(&record.channel_id)?;
        if !credential_ids.insert(record.document_id()) {
            return Err(DocumentError::DuplicateId);
        }
        if record
            .accounts
            .keys()
            .any(|account| !credential_accounts(record.namespace).contains(&account.as_str()))
        {
            return Err(DocumentError::ExcludedField);
        }
    }
    if channel_ids != credential_ids {
        return Err(DocumentError::InvalidReference);
    }
    Ok(())
}

fn validate_credentials(credentials: &ProviderCredentialRecord) -> DocumentResult<()> {
    identifier(&credentials.channel_id)?;
    for (account, value) in &credentials.accounts {
        if !credential_accounts(credentials.namespace).contains(&account.as_str()) {
            return Err(DocumentError::ExcludedField);
        }
        if account.ends_with(".extra_headers") && !value.trim().is_empty() {
            let headers: BTreeMap<String, String> =
                serde_json::from_str(value).map_err(|_| DocumentError::InvalidDocument)?;
            if headers.keys().any(|key| {
                key.eq_ignore_ascii_case("cookie") || key.eq_ignore_ascii_case("set-cookie")
            }) {
                return Err(DocumentError::ExcludedField);
            }
        }
    }
    Ok(())
}

fn validate_style(doc: &LogicalDocument) -> DocumentResult<()> {
    let style: StylePackRecord = deserialize(&doc.value)?;
    let row = object(
        style.pack.expose(),
        &[
            "id",
            "name",
            "description",
            "author",
            "version",
            "kind",
            "baseMode",
            "selectionPrompt",
            "voiceEditPrompt",
            "prompt",
            "examples",
            "tags",
            "createdAt",
            "updatedAt",
            "enabled",
            "recommendedModel",
            "compatibleAppVersion",
            "originPackId",
            "originAuthorLogin",
        ],
    )?;
    if required_text(row, "id")? != doc.id {
        return Err(DocumentError::InvalidReference);
    }
    for key in [
        "name",
        "description",
        "version",
        "kind",
        "selectionPrompt",
        "voiceEditPrompt",
        "prompt",
    ] {
        required_text(row, key)?;
    }
    choice(
        required_text(row, "baseMode")?,
        &["raw", "light", "structured", "formal"],
    )?;
    optional_text(
        row,
        &[
            "author",
            "createdAt",
            "updatedAt",
            "recommendedModel",
            "compatibleAppVersion",
            "originPackId",
            "originAuthorLogin",
        ],
    )?;
    optional_bool(row, &["enabled"])?;
    if !row
        .get("tags")
        .and_then(Value::as_array)
        .is_some_and(|tags| tags.iter().all(Value::is_string))
    {
        return Err(DocumentError::InvalidDocument);
    }
    let examples = row
        .get("examples")
        .and_then(Value::as_array)
        .ok_or(DocumentError::InvalidDocument)?;
    for example in examples {
        let fields = object(example, &["title", "input", "output"])?;
        required_text(fields, "input")?;
        required_text(fields, "output")?;
        optional_text(fields, &["title"])?;
    }
    if let Some(icon) = &style.icon {
        validate_icon(icon)?;
    }
    Ok(())
}

pub fn validate_icon(icon: &IconAsset) -> DocumentResult<()> {
    if icon.base64.len() > MAX_ICON_BYTES.div_ceil(3) * 4 {
        return Err(DocumentError::PayloadTooLarge);
    }
    let bytes = STANDARD
        .decode(&icon.base64)
        .map_err(|_| DocumentError::InvalidDocument)?;
    if bytes.len() > MAX_ICON_BYTES || STANDARD.encode(&bytes) != icon.base64 {
        return Err(DocumentError::InvalidDocument);
    }
    let extension = match icon.mime.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => return Err(DocumentError::InvalidDocument),
    };
    crate::style_pack_archive::validate_icon_content(extension, &bytes)
        .map_err(|_| DocumentError::InvalidDocument)
}

fn validate_history(doc: &LogicalDocument) -> DocumentResult<()> {
    let row = object(
        &doc.value,
        &[
            "id",
            "createdAt",
            "source",
            "rawTranscript",
            "asrTranscript",
            "finalText",
            "mode",
            "stylePackId",
            "translationActive",
            "polishSource",
            "appBundleId",
            "appName",
            "insertStatus",
            "errorCode",
            "durationMs",
            "dictionaryEntryCount",
            "hasAudioRecording",
            "asrProvider",
            "asrModel",
            "llmProvider",
            "llmModel",
            "pipelineMode",
            "asrMs",
            "polishMs",
            "sortIndex",
        ],
    )?;
    if required_text(row, "id")? != doc.id {
        return Err(DocumentError::InvalidReference);
    }
    for key in ["createdAt", "rawTranscript", "finalText"] {
        required_text(row, key)?;
    }
    choice(
        required_text(row, "mode")?,
        &["raw", "light", "structured", "formal"],
    )?;
    choice(
        required_text(row, "insertStatus")?,
        &[
            "inserted",
            "pasteSent",
            "copiedFallback",
            "failed",
            "notRequested",
        ],
    )?;
    if let Some(source) = row.get("source") {
        choice(
            source.as_str().ok_or(DocumentError::InvalidDocument)?,
            &[
                "voice",
                "quick_note",
                "selection_polish",
                "selection_voice_edit",
            ],
        )?;
    }
    optional_text(
        row,
        &[
            "asrTranscript",
            "stylePackId",
            "polishSource",
            "appBundleId",
            "appName",
            "errorCode",
            "asrProvider",
            "asrModel",
            "llmProvider",
            "llmModel",
            "pipelineMode",
        ],
    )?;
    optional_unsigned(
        row,
        &["durationMs", "dictionaryEntryCount", "asrMs", "polishMs"],
    )?;
    optional_bool(row, &["translationActive", "hasAudioRecording"])?;
    if row.get("hasAudioRecording") == Some(&Value::Bool(true)) {
        return Err(DocumentError::ExcludedField);
    }
    Ok(())
}

fn validate_profile(doc: &LogicalDocument) -> DocumentResult<()> {
    let profile: DeviceProfileRecord = deserialize(&doc.value)?;
    source_device(&profile.device)?;
    if profile.device.id != doc.id {
        return Err(DocumentError::InvalidReference);
    }
    for (key, value) in profile
        .preferences
        .expose()
        .as_object()
        .ok_or(DocumentError::InvalidDocument)?
    {
        let field = preference_field(key).ok_or(DocumentError::Unsupported)?;
        if field.class != PreferenceClass::DeviceProfile {
            return Err(DocumentError::ExcludedField);
        }
        validate_preference_value(field, value)?;
    }
    let mut channels = BTreeMap::new();
    let mut active = BTreeSet::new();
    for channel in &profile.channels {
        validate_channel(channel)?;
        if !is_device_channel(channel.namespace, &channel.provider_type) {
            return Err(DocumentError::InvalidDocument);
        }
        if channel.active && !active.insert(channel.namespace) {
            return Err(DocumentError::InvalidReference);
        }
        if channels.insert(channel.document_id(), channel).is_some() {
            return Err(DocumentError::DuplicateId);
        }
    }
    let mut credential_ids = BTreeSet::new();
    for credential in &profile.provider_credentials {
        validate_credentials(credential)?;
        if !credential_ids.insert(credential.document_id()) {
            return Err(DocumentError::DuplicateId);
        }
    }
    if credential_ids != channels.keys().cloned().collect() {
        return Err(DocumentError::InvalidReference);
    }
    let mut windows = BTreeSet::new();
    for window in &profile.window_positions {
        identifier(&window.window_id)?;
        if !windows.insert(&window.window_id)
            || window.width == 0
            || window.height == 0
            || window.width > 100_000
            || window.height > 100_000
        {
            return Err(DocumentError::InvalidDocument);
        }
    }
    Ok(())
}

fn validate_references(set: &DocumentSet) -> DocumentResult<()> {
    let mut channels = BTreeMap::new();
    let mut credentials = BTreeSet::new();
    let mut active = BTreeSet::new();
    let styles: BTreeSet<&str> = set
        .documents
        .iter()
        .filter(|doc| doc.kind == DocumentKind::StylePacks)
        .map(|doc| doc.id.as_str())
        .collect();
    for doc in &set.documents {
        match doc.kind {
            DocumentKind::Channels => {
                let channel: ChannelRecord = deserialize(&doc.value)?;
                if channel.active && !active.insert(channel.namespace) {
                    return Err(DocumentError::InvalidReference);
                }
                channels.insert(doc.id.clone(), channel);
            }
            DocumentKind::ProviderCredentials => {
                credentials.insert(doc.id.clone());
            }
            DocumentKind::Preferences
                if matches!(
                    doc.id.as_str(),
                    "activeStylePackId" | "selectionPolishStylePackId"
                ) =>
            {
                let id = doc.value.as_str().ok_or(DocumentError::InvalidReference)?;
                if !id.is_empty() && !styles.contains(id) {
                    return Err(DocumentError::InvalidReference);
                }
            }
            _ => {}
        }
    }
    if credentials != channels.keys().cloned().collect() {
        return Err(DocumentError::InvalidReference);
    }
    // A channel cannot be deleted while a separate credential half remains live.
    for tombstone in &set.tombstones {
        if tombstone.kind == DocumentKind::Channels && credentials.contains(&tombstone.id)
            || tombstone.kind == DocumentKind::ProviderCredentials
                && channels.contains_key(&tombstone.id)
        {
            return Err(DocumentError::InvalidReference);
        }
    }
    Ok(())
}

pub(crate) fn validate_scope(scope: &SyncScope) -> DocumentResult<()> {
    let origin =
        url::Url::parse(&scope.service_origin).map_err(|_| DocumentError::InvalidDocument)?;
    if origin.scheme() != "https"
        || origin.origin().ascii_serialization() != scope.service_origin
        || !origin.username().is_empty()
        || origin.password().is_some()
    {
        return Err(DocumentError::InvalidDocument);
    }
    let github_id =
        Revision::parse(&scope.owner_github_id).map_err(|_| DocumentError::InvalidDocument)?;
    if github_id.get() == 0 {
        return Err(DocumentError::InvalidDocument);
    }
    for id in [&scope.vault_id, &scope.key_id] {
        uuid_v4(id)?;
    }
    identifier(&scope.device_id)
}

pub(crate) fn uuid_v4(value: &str) -> DocumentResult<()> {
    let id = uuid::Uuid::parse_str(value).map_err(|_| DocumentError::InvalidDocument)?;
    if id.get_version() != Some(uuid::Version::Random)
        || id.get_variant() != uuid::Variant::RFC4122
        || id.to_string() != value
    {
        return Err(DocumentError::InvalidDocument);
    }
    Ok(())
}

fn normalize_collection_order(set: &mut DocumentSet) -> DocumentResult<()> {
    for kind in [
        DocumentKind::Dictionary,
        DocumentKind::Corrections,
        DocumentKind::History,
    ] {
        let mut indices: Vec<_> = set
            .documents
            .iter()
            .enumerate()
            .filter(|(_, doc)| doc.kind == kind)
            .map(|(index, _)| index)
            .collect();
        for index in &indices {
            if set.documents[*index]
                .value
                .get("sortIndex")
                .is_some_and(|value| value.as_u64().is_none())
            {
                return Err(DocumentError::InvalidDocument);
            }
        }
        indices.sort_by(|left, right| {
            let left = &set.documents[*left];
            let right = &set.documents[*right];
            (
                left.value
                    .get("sortIndex")
                    .and_then(Value::as_u64)
                    .unwrap_or(u64::MAX),
                &left.id,
            )
                .cmp(&(
                    right
                        .value
                        .get("sortIndex")
                        .and_then(Value::as_u64)
                        .unwrap_or(u64::MAX),
                    &right.id,
                ))
        });
        for (order, index) in indices.into_iter().enumerate() {
            set.documents[index]
                .value
                .as_object_mut()
                .ok_or(DocumentError::InvalidDocument)?
                .insert("sortIndex".into(), Value::from(order));
        }
    }
    for origin in ["custom", "override"] {
        let mut indices: Vec<_> = set
            .documents
            .iter()
            .enumerate()
            .filter(|(_, doc)| {
                doc.kind == DocumentKind::VocabularyPresets
                    && doc.value.get("origin").and_then(Value::as_str) == Some(origin)
            })
            .map(|(index, _)| index)
            .collect();
        indices.sort_by(|left, right| {
            let left = &set.documents[*left];
            let right = &set.documents[*right];
            (
                left.value.get("order").and_then(Value::as_u64).unwrap_or(0),
                &left.id,
            )
                .cmp(&(
                    right
                        .value
                        .get("order")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    &right.id,
                ))
        });
        for (order, index) in indices.into_iter().enumerate() {
            set.documents[index]
                .value
                .as_object_mut()
                .ok_or(DocumentError::InvalidDocument)?
                .insert("order".into(), Value::from(order));
        }
    }
    Ok(())
}
