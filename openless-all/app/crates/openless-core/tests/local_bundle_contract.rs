use std::io::{Cursor, Write};

use openless_core::cloud_sync_e2ee_documents::{parse_sync_documents, Revision};
use openless_core::local_migration_bundle::{
    decode_bundle, export_validated_bundle, inspect_bundle, BundleError, BundleExportOptions,
    BundleMode, BundlePassword, CredentialPolicy,
};
use serde_json::json;

fn fixture() -> openless_core::cloud_sync_e2ee_documents::ValidatedSyncDocuments {
    let value = json!({
        "schemaVersion": 1,
        "exportedAt": "2026-10-14T00:00:00Z",
        "sourceDevice": {"id":"source","os":"android","arch":"arm64","appVersion":"1"},
        "documents": [
            {"id":"themeMode","kind":"preferences","schemaVersion":1,"value":"dark"},
            {"id":"llm:main","kind":"channels","schemaVersion":1,"value":{"id":"main","namespace":"llm","providerType":"requesty","name":"Main","enabled":true,"order":0,"active":true}},
            {"id":"asr:asr-main","kind":"channels","schemaVersion":1,"value":{"id":"asr-main","namespace":"asr","providerType":"requesty","name":"Speech","enabled":true,"order":1,"active":false}},
            {"id":"omni:omni-provider","kind":"channels","schemaVersion":1,"value":{"id":"omni-provider","namespace":"omni","providerType":"omni-provider","name":"Omni","enabled":true,"order":2,"active":false}},
            {"id":"llm:main","kind":"provider_credentials","schemaVersion":1,"value":{"channelId":"main","namespace":"llm","accounts":{"ark.api_key":"secret","ark.endpoint":"https://example.test","ark.model_id":"model"}}},
            {"id":"asr:asr-main","kind":"provider_credentials","schemaVersion":1,"value":{"channelId":"asr-main","namespace":"asr","accounts":{"asr.api_key":"asr-secret","asr.endpoint":"https://asr.example.test","asr.model":"small"}}},
            {"id":"omni:omni-provider","kind":"provider_credentials","schemaVersion":1,"value":{"channelId":"omni-provider","namespace":"omni","accounts":{"omni.api_key":"omni-secret","omni.endpoint":"https://omni.example.test","omni.model":"vision"}}}
        ],
        "tombstones": []
    });
    parse_sync_documents(&serde_json::to_vec(&value).unwrap(), Revision::new(0)).unwrap()
}

#[test]
fn share_contract_excludes_credentials_and_device_profile() {
    let artifact = export_validated_bundle(&fixture(), BundleExportOptions::default()).unwrap();
    let decoded = decode_bundle(&artifact.bytes, None, Revision::new(0)).unwrap();
    assert!(!decoded.inspection.credentials_present);
    assert!(decoded
        .documents
        .documents()
        .documents
        .iter()
        .all(|document| {
            document.kind
                != openless_core::cloud_sync_e2ee_documents::DocumentKind::ProviderCredentials
        }));
    assert!(decoded
        .documents
        .documents()
        .documents
        .iter()
        .all(|document| {
            document.kind != openless_core::cloud_sync_e2ee_documents::DocumentKind::DeviceProfile
        }));
    let channel = decoded
        .documents
        .documents()
        .documents
        .iter()
        .find(|document| {
            document.kind == openless_core::cloud_sync_e2ee_documents::DocumentKind::Channels
                && document.value["namespace"] == "llm"
        })
        .unwrap();
    assert_eq!(
        channel.value["configuration"]["ark.endpoint"],
        "https://example.test"
    );
    assert!(channel.value["configuration"].get("ark.api_key").is_none());
}

#[test]
fn personal_contract_requires_password_and_rejects_wrong_password() {
    let password = BundlePassword::new("correct horse battery staple").unwrap();
    let artifact = export_validated_bundle(
        &fixture(),
        BundleExportOptions {
            mode: BundleMode::PersonalMigration,
            credential_policy: CredentialPolicy::PasswordEncrypted,
            password: Some(password),
            app_version: "test".into(),
            portable_android_preferences: None,
        },
    )
    .unwrap();
    assert!(matches!(
        decode_bundle(
            &artifact.bytes,
            Some(&BundlePassword::new("wrong").unwrap()),
            Revision::new(0)
        ),
        Err(BundleError::InvalidPassword)
    ));
    let decoded = decode_bundle(
        &artifact.bytes,
        Some(&BundlePassword::new("correct horse battery staple").unwrap()),
        Revision::new(0),
    )
    .unwrap();
    assert!(decoded.inspection.credentials_unlocked);
    assert_eq!(decoded.documents.documents().documents.len(), 7);
}

#[test]
fn personal_without_credentials_is_importable_as_redacted_documents() {
    let artifact = export_validated_bundle(
        &fixture(),
        BundleExportOptions {
            mode: BundleMode::PersonalMigration,
            credential_policy: CredentialPolicy::Omit,
            password: None,
            app_version: "test".into(),
            portable_android_preferences: None,
        },
    )
    .unwrap();
    let decoded = decode_bundle(&artifact.bytes, None, Revision::new(0)).unwrap();
    assert!(!decoded.inspection.credentials_present);
    assert!(decoded
        .documents
        .documents()
        .documents
        .iter()
        .all(|document| {
            document.kind
                != openless_core::cloud_sync_e2ee_documents::DocumentKind::ProviderCredentials
        }));
}

#[test]
fn portable_preferences_require_all_sections() {
    let result = export_validated_bundle(
        &fixture(),
        BundleExportOptions {
            portable_android_preferences: Some(json!({
                "schemaVersion": 1,
                "learningPrivacyVersion": 1,
            })),
            ..BundleExportOptions::default()
        },
    );
    assert!(matches!(result, Err(BundleError::InvalidSchema)));
}

#[test]
fn container_rejects_unknown_entries() {
    let options = zip::write::SimpleFileOptions::default();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("manifest.json", options).unwrap();
    writer.write_all(b"{}").unwrap();
    writer.start_file("documents.json", options).unwrap();
    writer.write_all(b"{}").unwrap();
    writer.start_file("hidden.bin", options).unwrap();
    writer.write_all(b"unexpected").unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert!(matches!(
        inspect_bundle(&bytes),
        Err(BundleError::MalformedContainer)
    ));
}
