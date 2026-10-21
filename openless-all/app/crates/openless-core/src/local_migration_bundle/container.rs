use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::cloud_sync_e2ee_documents::{
    export_sync_documents, parse_redacted_sync_documents, parse_sync_documents,
    revalidate_sync_documents, serialize_sync_documents, DocumentKind, Revision,
    SyncDocumentSource, ValidatedSyncDocuments,
};
use crate::cloud_sync_e2ee_protocol::types::LogicalDocument;

use super::crypto;
use super::types::*;

const MANIFEST_ENTRY: &str = "manifest.json";
const DOCUMENTS_ENTRY: &str = "documents.json";
const CREDENTIALS_ENTRY: &str = "credentials.enc";
const PLATFORM: &str = "android";
const MAX_COMPRESSION_RATIO: u64 = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EntryDigest {
    entry: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CredentialsManifest {
    entry: String,
    bytes: usize,
    sha256: String,
    encryption: String,
    kdf: String,
    aead: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    format_version: u32,
    document_schema_version: u32,
    bundle_id: String,
    created_at: String,
    producer: Producer,
    platform: String,
    mode: BundleMode,
    documents: EntryDigest,
    credentials: Option<CredentialsManifest>,
    sections: SectionCounts,
    permissions_migrated: bool,
    device_profile_migrated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Producer {
    app_version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialPayload<'a> {
    schema_version: u32,
    documents: &'a [Value],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DecodedCredentialPayload {
    schema_version: u32,
    documents: Vec<Value>,
}

pub async fn export_bundle(
    source: &dyn SyncDocumentSource,
    options: BundleExportOptions,
) -> BundleResult<BundleArtifact> {
    options.validate()?;
    let exported = export_sync_documents(source)
        .await
        .map_err(BundleError::from)?;
    assemble_bundle(exported.documents, exported.generation, &options)
}

/// Build a Bundle from an already validated coherent logical snapshot.
/// This is the seam used by CoreSyncStore adapters, so hosts cannot combine
/// independent repository reads into a false snapshot.
pub fn export_validated_bundle(
    documents: &ValidatedSyncDocuments,
    options: BundleExportOptions,
) -> BundleResult<BundleArtifact> {
    options.validate()?;
    let documents = if let Some(value) = &options.portable_android_preferences {
        let mut set = documents.documents().clone();
        set.documents.retain(|document| {
            !(document.kind == DocumentKind::UiPreferences
                && document.id == "androidPortableSettings")
        });
        set.documents.push(LogicalDocument {
            id: "androidPortableSettings".into(),
            kind: DocumentKind::UiPreferences,
            schema_version: BUNDLE_DOCUMENT_SCHEMA_VERSION,
            value: value.clone(),
        });
        revalidate_sync_documents(set, documents.observed_revision()).map_err(BundleError::from)?
    } else {
        documents.clone()
    };
    let revision = documents.observed_revision();
    assemble_bundle(documents, revision, &options)
}

pub fn inspect_bundle(bytes: &[u8]) -> BundleResult<BundleInspection> {
    let entries = read_entries(bytes)?;
    let manifest = decode_manifest(&entries)?;
    let documents = validate_document_entry(&manifest, &entries)?;
    Ok(inspection(&manifest, &documents, false))
}

pub fn decode_bundle(
    bytes: &[u8],
    password: Option<&BundlePassword>,
    observed_revision: Revision,
) -> BundleResult<DecodedBundle> {
    let entries = read_entries(bytes)?;
    let manifest = decode_manifest(&entries)?;
    let mut root = validate_document_entry(&manifest, &entries)?;
    let mut credentials_unlocked = false;
    if let Some(credentials) = &manifest.credentials {
        let password = password.ok_or(BundleError::PasswordRequired)?;
        let ciphertext = entries
            .get(&credentials.entry)
            .ok_or(BundleError::MalformedContainer)?;
        let aad = crypto::aad(
            &manifest.bundle_id,
            &manifest.documents.sha256,
            &credentials.entry,
        );
        let plaintext = crypto::decrypt(password, ciphertext, &aad)?;
        let payload: DecodedCredentialPayload =
            serde_json::from_slice(&plaintext).map_err(|_| BundleError::InvalidPassword)?;
        if payload.schema_version != BUNDLE_FORMAT_VERSION {
            return Err(BundleError::UnsupportedVersion);
        }
        let documents = root
            .get_mut("documents")
            .and_then(Value::as_array_mut)
            .ok_or(BundleError::InvalidSchema)?;
        for document in payload.documents {
            if document.get("kind").and_then(Value::as_str) != Some("provider_credentials") {
                return Err(BundleError::InvalidSchema);
            }
            documents.push(document);
        }
        credentials_unlocked = true;
    }
    let bytes = serde_json::to_vec(&root).map_err(|_| BundleError::InvalidSchema)?;
    let mut documents = if manifest.credentials.is_none() {
        parse_redacted_sync_documents(&bytes, observed_revision)
    } else {
        parse_sync_documents(&bytes, observed_revision)
    }
    .map_err(BundleError::from)?;
    let inspection = inspection(&manifest, &root, credentials_unlocked);
    for document in &mut documents.set.documents {
        if document.kind == DocumentKind::UiPreferences && document.id == super::ANDROID_SETTINGS_ID
        {
            let preferences = document.value["preferences"]
                .as_object_mut()
                .ok_or(BundleError::InvalidSchema)?;
            for (key, value) in [
                ("hapticEnabled", json!(true)),
                ("hapticAmplitude", json!(55)),
                ("hapticDurationMs", json!(12)),
                ("englishSuggestionsEnabled", json!(true)),
                ("debugFeaturesEnabled", json!(false)),
            ] {
                preferences.entry(key).or_insert(value);
            }
        }
    }
    // Validate old bundles before dropping usage data; local migration never restores it.
    documents.set.documents.retain(|document| {
        !matches!(
            document.kind,
            DocumentKind::History | DocumentKind::Activity
        )
    });
    documents.set.tombstones.retain(|document| {
        !matches!(
            document.kind,
            DocumentKind::History | DocumentKind::Activity
        )
    });
    Ok(DecodedBundle {
        inspection,
        documents,
    })
}

fn assemble_bundle(
    documents: ValidatedSyncDocuments,
    _generation: Revision,
    options: &BundleExportOptions,
) -> BundleResult<BundleArtifact> {
    let bytes = serialize_sync_documents(&documents).map_err(BundleError::from)?;
    let mut root: Value = serde_json::from_slice(&bytes).map_err(|_| BundleError::InvalidSchema)?;
    let root_object = root.as_object_mut().ok_or(BundleError::InvalidSchema)?;
    // A bundle never carries the source device identity or device profile. The
    // target restore replaces this metadata with its own device profile.
    root_object.insert(
        "sourceDevice".into(),
        json!({"id":"imported","os":"android","arch":"unknown","appVersion":options.app_version}),
    );
    let source_documents = root_object
        .get_mut("documents")
        .and_then(Value::as_array_mut)
        .ok_or(BundleError::InvalidSchema)?;
    let mut credential_documents = Vec::new();
    source_documents.retain(|document| {
        let kind = document.get("kind").and_then(Value::as_str);
        if matches!(kind, Some("device_profile" | "history" | "activity")) {
            return false;
        }
        if kind == Some("provider_credentials") {
            credential_documents.push(document.clone());
            return false;
        }
        true
    });
    for channel in source_documents
        .iter_mut()
        .filter(|document| document.get("kind").and_then(Value::as_str) == Some("channels"))
    {
        let Some(credential) = credential_documents
            .iter()
            .find(|document| document.get("id") == channel.get("id"))
        else {
            continue;
        };
        let namespace = credential
            .get("value")
            .and_then(|value| value.get("namespace"))
            .cloned()
            .ok_or(BundleError::InvalidSchema)?;
        let namespace =
            serde_json::from_value::<crate::cloud_sync_e2ee_documents::SyncNamespace>(namespace)
                .map_err(|_| BundleError::InvalidSchema)?;
        let mut configuration = serde_json::Map::new();
        if let Some(accounts) = credential
            .get("value")
            .and_then(|value| value.get("accounts"))
            .and_then(Value::as_object)
        {
            for (account, value) in accounts {
                if crate::cloud_sync_e2ee_documents::registry::public_credential_account(
                    namespace, account,
                ) && crate::cloud_sync_e2ee_documents::registry::public_credential_value_safe(
                    account,
                    value.as_str().unwrap_or_default(),
                ) {
                    configuration.insert(account.clone(), value.clone());
                }
            }
        }
        channel
            .get_mut("value")
            .and_then(Value::as_object_mut)
            .ok_or(BundleError::InvalidSchema)?
            .insert("configuration".into(), Value::Object(configuration));
    }
    if let Some(tombstones) = root_object
        .get_mut("tombstones")
        .and_then(Value::as_array_mut)
    {
        tombstones.retain(|tombstone| {
            !matches!(
                tombstone.get("kind").and_then(Value::as_str),
                Some("history" | "activity" | "device_profile")
            ) && !(matches!(options.mode, BundleMode::ShareRedacted)
                && matches!(
                    tombstone.get("kind").and_then(Value::as_str),
                    Some("provider_credentials" | "device_profile")
                ))
        });
        for tombstone in tombstones {
            if let Some(object) = tombstone.as_object_mut() {
                // Bundle tombstones are portable deletion markers, not cloud
                // revision assertions from the source device.
                object.insert("baseRevision".into(), json!(0));
            }
        }
    }
    let documents_bytes = serde_json::to_vec(&root).map_err(|_| BundleError::InvalidSchema)?;
    let documents_digest = crypto::sha256_hex(&documents_bytes);
    let bundle_id = Uuid::new_v4().to_string();
    let mode = options.mode;
    let encrypted_credentials = matches!(
        options.credential_policy,
        CredentialPolicy::PasswordEncrypted
    );
    let credentials_bytes = if encrypted_credentials {
        let password = options
            .password
            .as_ref()
            .ok_or(BundleError::PasswordRequired)?;
        let payload = serde_json::to_vec(&CredentialPayload {
            schema_version: BUNDLE_FORMAT_VERSION,
            documents: &credential_documents,
        })
        .map_err(|_| BundleError::InvalidSchema)?;
        let aad = crypto::aad(&bundle_id, &documents_digest, CREDENTIALS_ENTRY);
        Some(crypto::encrypt(password, &payload, &aad)?)
    } else {
        None
    };
    let manifest = Manifest {
        format: BUNDLE_FORMAT.into(),
        format_version: BUNDLE_FORMAT_VERSION,
        document_schema_version: BUNDLE_DOCUMENT_SCHEMA_VERSION,
        bundle_id,
        created_at: chrono::Utc::now().to_rfc3339(),
        producer: Producer {
            app_version: options.app_version.clone(),
        },
        platform: PLATFORM.into(),
        mode,
        documents: EntryDigest {
            entry: DOCUMENTS_ENTRY.into(),
            bytes: documents_bytes.len(),
            sha256: documents_digest,
        },
        credentials: credentials_bytes.as_ref().map(|value| CredentialsManifest {
            entry: CREDENTIALS_ENTRY.into(),
            bytes: value.len(),
            sha256: crypto::sha256_hex(value),
            encryption: "password_argon2id_xchacha20poly1305_v1".into(),
            kdf: "argon2id".into(),
            aead: "xchacha20poly1305".into(),
        }),
        sections: section_counts(&root, false)?,
        permissions_migrated: false,
        device_profile_migrated: false,
    };
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(|_| BundleError::InvalidSchema)?;
    let archive = write_entries(BundleEntries::new(
        manifest_bytes,
        documents_bytes,
        credentials_bytes,
    ))?;
    let inspection = inspection(&manifest, &root, false);
    Ok(BundleArtifact {
        bytes: Zeroizing::new(archive),
        inspection,
    })
}

fn inspection(manifest: &Manifest, root: &Value, credentials_unlocked: bool) -> BundleInspection {
    let skipped_history = root["documents"].as_array().map_or(0, |documents| {
        documents
            .iter()
            .filter(|doc| doc["kind"] == "history")
            .count()
    });
    let skipped_activity = root["documents"].as_array().map_or(0, |documents| {
        documents
            .iter()
            .filter(|doc| doc["kind"] == "activity")
            .count()
    });
    let skipped_tombstones = root["tombstones"].as_array().map_or(0, |documents| {
        documents
            .iter()
            .filter(|doc| matches!(doc["kind"].as_str(), Some("history" | "activity")))
            .count()
    });
    let mut section_counts =
        section_counts(root, credentials_unlocked).expect("validated document kinds");
    section_counts.history = 0;
    section_counts.activity = 0;
    let (document_count, tombstone_count) = root
        .as_object()
        .map(|object| {
            (
                object
                    .get("documents")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
                object
                    .get("tombstones")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            )
        })
        .unwrap_or_default();
    BundleInspection {
        format: manifest.format.clone(),
        format_version: manifest.format_version,
        document_schema_version: manifest.document_schema_version,
        bundle_id: manifest.bundle_id.clone(),
        created_at: manifest.created_at.clone(),
        platform: manifest.platform.clone(),
        mode: manifest.mode,
        document_count: document_count - skipped_history - skipped_activity,
        tombstone_count: tombstone_count - skipped_tombstones,
        section_counts,
        skipped_history,
        skipped_activity,
        skipped_tombstones,
        credentials_present: manifest.credentials.is_some(),
        credentials_encrypted: manifest.credentials.is_some(),
        credentials_unlocked,
        permissions_migrated: manifest.permissions_migrated,
        device_profile_migrated: manifest.device_profile_migrated,
    }
}

fn decode_manifest(entries: &BTreeMap<String, Vec<u8>>) -> BundleResult<Manifest> {
    let bytes = entries
        .get(MANIFEST_ENTRY)
        .ok_or(BundleError::MalformedContainer)?;
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| BundleError::InvalidSchema)?;
    if manifest.format != BUNDLE_FORMAT
        || manifest.format_version != BUNDLE_FORMAT_VERSION
        || manifest.document_schema_version != BUNDLE_DOCUMENT_SCHEMA_VERSION
        || manifest.platform != PLATFORM
        || manifest.permissions_migrated
        || manifest.device_profile_migrated
        || manifest.documents.entry != DOCUMENTS_ENTRY
    {
        return Err(BundleError::UnsupportedVersion);
    }
    if manifest
        .credentials
        .as_ref()
        .is_some_and(|_| matches!(manifest.mode, BundleMode::ShareRedacted))
    {
        return Err(BundleError::InvalidSchema);
    }
    if manifest.credentials.as_ref().is_some_and(|credential| {
        credential.entry != CREDENTIALS_ENTRY
            || credential.encryption != "password_argon2id_xchacha20poly1305_v1"
            || credential.kdf != "argon2id"
            || credential.aead != "xchacha20poly1305"
    }) {
        return Err(BundleError::UnsupportedVersion);
    }
    Ok(manifest)
}

fn validate_document_entry(
    manifest: &Manifest,
    entries: &BTreeMap<String, Vec<u8>>,
) -> BundleResult<Value> {
    let documents = entries
        .get(&manifest.documents.entry)
        .ok_or(BundleError::MalformedContainer)?;
    if documents.len() != manifest.documents.bytes
        || crypto::sha256_hex(documents) != manifest.documents.sha256
    {
        return Err(BundleError::DigestMismatch);
    }
    let root: Value = serde_json::from_slice(documents).map_err(|_| BundleError::InvalidSchema)?;
    if root.get("schemaVersion").and_then(Value::as_u64)
        != Some(BUNDLE_DOCUMENT_SCHEMA_VERSION as u64)
    {
        return Err(BundleError::UnsupportedVersion);
    }
    if root.get("sourceDevice").is_none()
        || !root.get("documents").is_some_and(Value::is_array)
        || !root.get("tombstones").is_some_and(Value::is_array)
    {
        return Err(BundleError::InvalidSchema);
    }
    section_counts(&root, false)?;
    for document in root["documents"]
        .as_array()
        .ok_or(BundleError::InvalidSchema)?
    {
        if document["kind"] == "ui_preferences" && document["id"] == "androidPortableSettings" {
            validate_portable_android_preferences(&document["value"])?;
        }
    }
    if let Some(credentials) = &manifest.credentials {
        let bytes = entries
            .get(&credentials.entry)
            .ok_or(BundleError::MalformedContainer)?;
        if bytes.len() != credentials.bytes || crypto::sha256_hex(bytes) != credentials.sha256 {
            return Err(BundleError::DigestMismatch);
        }
    } else if entries.contains_key(CREDENTIALS_ENTRY) {
        return Err(BundleError::MalformedContainer);
    }
    Ok(root)
}

fn section_counts(root: &Value, credentials_unlocked: bool) -> BundleResult<SectionCounts> {
    let mut counts = SectionCounts::default();
    for document in root
        .get("documents")
        .and_then(Value::as_array)
        .ok_or(BundleError::InvalidSchema)?
    {
        match document.get("kind").and_then(Value::as_str) {
            Some("preferences") => counts.preferences += 1,
            Some("ui_preferences") => counts.ui_preferences += 1,
            Some("channels") => counts.channels += 1,
            Some("dictionary") => counts.dictionary += 1,
            Some("vocabulary_presets") => counts.vocabulary_presets += 1,
            Some("corrections") => counts.corrections += 1,
            Some("style_packs") => counts.style_packs += 1,
            Some("history") => counts.history += 1,
            Some("activity") => counts.activity += 1,
            Some("provider_credentials") if credentials_unlocked => {}
            Some("provider_credentials") | Some("device_profile") => {
                return Err(BundleError::InvalidSchema)
            }
            None => return Err(BundleError::InvalidSchema),
            Some(_) => return Err(BundleError::UnsupportedVersion),
        }
    }
    Ok(counts)
}

fn read_entries(bytes: &[u8]) -> BundleResult<BTreeMap<String, Vec<u8>>> {
    if bytes.is_empty() || bytes.len() > MAX_BUNDLE_COMPRESSED_BYTES {
        return Err(BundleError::TooLarge);
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| BundleError::MalformedContainer)?;
    if archive.is_empty() || archive.len() > MAX_BUNDLE_ENTRIES {
        return Err(BundleError::TooLarge);
    }
    let mut entries = BTreeMap::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| BundleError::MalformedContainer)?;
        // `zip` 9（本仓）返回 `Result<Cow<str>, ZipError>`，2.x 直接返回 `&str`。
        let name = file
            .name()
            .map_err(|_| BundleError::MalformedContainer)?
            .into_owned();
        if file.is_dir()
            || file.encrypted()
            || !safe_entry_name(&name)
            || entries.contains_key(&name)
            || !matches!(
                name.as_str(),
                MANIFEST_ENTRY | DOCUMENTS_ENTRY | CREDENTIALS_ENTRY
            )
        {
            return Err(BundleError::MalformedContainer);
        }
        let uncompressed = usize::try_from(file.size()).map_err(|_| BundleError::TooLarge)?;
        let compressed = file.compressed_size();
        if uncompressed > MAX_BUNDLE_ENTRY_BYTES
            || compressed == 0 && uncompressed > 0
            || compressed > 0
                && (uncompressed as u64) > compressed.saturating_mul(MAX_COMPRESSION_RATIO)
        {
            return Err(BundleError::TooLarge);
        }
        total = total
            .checked_add(uncompressed)
            .ok_or(BundleError::TooLarge)?;
        if total > MAX_BUNDLE_UNCOMPRESSED_BYTES {
            return Err(BundleError::TooLarge);
        }
        let mut content = Vec::with_capacity(uncompressed.min(64 * 1024));
        file.take((MAX_BUNDLE_ENTRY_BYTES + 1) as u64)
            .read_to_end(&mut content)
            .map_err(|_| BundleError::MalformedContainer)?;
        if content.len() != uncompressed {
            return Err(BundleError::MalformedContainer);
        }
        entries.insert(name, content);
    }
    if !entries.contains_key(MANIFEST_ENTRY) || !entries.contains_key(DOCUMENTS_ENTRY) {
        return Err(BundleError::MalformedContainer);
    }
    Ok(entries)
}

fn safe_entry_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.starts_with('\\')
        && !name.contains('\\')
        && !name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && !name.chars().any(|character| character.is_control())
}

fn write_entries(entries: BundleEntries) -> BundleResult<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [
        (MANIFEST_ENTRY, entries.manifest),
        (DOCUMENTS_ENTRY, entries.documents),
    ] {
        writer
            .start_file(name, options)
            .map_err(|_| BundleError::MalformedContainer)?;
        writer
            .write_all(&bytes)
            .map_err(|_| BundleError::MalformedContainer)?;
    }
    if let Some(bytes) = entries.credentials {
        writer
            .start_file(CREDENTIALS_ENTRY, options)
            .map_err(|_| BundleError::MalformedContainer)?;
        writer
            .write_all(&bytes)
            .map_err(|_| BundleError::MalformedContainer)?;
    }
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|_| BundleError::MalformedContainer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_sync_e2ee_documents::parse_sync_documents;

    fn documents() -> ValidatedSyncDocuments {
        let value = json!({
            "schemaVersion": 1,
            "exportedAt": "2026-10-14T00:00:00Z",
            "sourceDevice": {"id":"device","os":"android","arch":"arm64","appVersion":"1"},
            "documents": [
                {"id":"themeMode","kind":"preferences","schemaVersion":1,"value":"dark"},
                {"id":"llm:main","kind":"channels","schemaVersion":1,"value":{"id":"main","namespace":"llm","providerType":"requesty","name":"Main","enabled":true,"order":0,"active":true}},
                {"id":"llm:main","kind":"provider_credentials","schemaVersion":1,"value":{"channelId":"main","namespace":"llm","accounts":{"ark.api_key":"secret"}}}
            ],
            "tombstones": []
        });
        let bytes = serde_json::to_vec(&value).unwrap();
        parse_sync_documents(&bytes, Revision::new(0)).unwrap()
    }

    #[test]
    fn share_bundle_omits_device_and_credentials() {
        let artifact = export_validated_bundle(
            &documents(),
            BundleExportOptions {
                app_version: "2".into(),
                ..BundleExportOptions::default()
            },
        )
        .unwrap();
        let inspection = inspect_bundle(&artifact.bytes).unwrap();
        assert!(!inspection.credentials_present);
        assert!(!inspection.device_profile_migrated);
        assert_eq!(inspection.section_counts.channels, 1);
    }

    #[test]
    fn password_bundle_round_trips_credentials() {
        let password = BundlePassword::new("Strong migration password 9").unwrap();
        let artifact = export_validated_bundle(
            &documents(),
            BundleExportOptions {
                mode: BundleMode::PersonalMigration,
                credential_policy: CredentialPolicy::PasswordEncrypted,
                password: Some(password),
                app_version: "2".into(),
                portable_android_preferences: None,
            },
        )
        .unwrap();
        assert!(inspect_bundle(&artifact.bytes).unwrap().credentials_present);
        let wrong = BundlePassword::new("wrong password").unwrap();
        assert!(matches!(
            decode_bundle(&artifact.bytes, Some(&wrong), Revision::new(0)),
            Err(BundleError::InvalidPassword)
        ));
        let right = BundlePassword::new("Strong migration password 9").unwrap();
        let decoded = decode_bundle(&artifact.bytes, Some(&right), Revision::new(0)).unwrap();
        assert!(decoded.inspection.credentials_unlocked);
        assert_eq!(decoded.documents.documents().documents.len(), 3);
    }

    #[test]
    fn rejects_path_traversal_entry() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file("../manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"{}").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(matches!(
            read_entries(&bytes),
            Err(BundleError::MalformedContainer)
        ));
    }
}
