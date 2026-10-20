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

#[test]
fn review_default_share_does_not_contain_history() {
    let mut set = fixture().documents().clone();
    set.documents.push(serde_json::from_value(json!({"id":"private-history","kind":"history","schemaVersion":1,"value":{"id":"private-history","createdAt":"2026-09-26T00:00:00Z","source":"quick_note","rawTranscript":"PRIVATE_DICTATION_MARKER","finalText":"PRIVATE_DICTATION_MARKER","mode":"raw","insertStatus":"notRequested","hasAudioRecording":false,"sortIndex":0}})).unwrap());
    let docs = parse_sync_documents(&serde_json::to_vec(&set).unwrap(), Revision::new(0)).unwrap();
    let artifact = export_validated_bundle(&docs, BundleExportOptions::default()).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(&artifact.bytes[..])).unwrap();
    let mut raw = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("documents.json").unwrap(), &mut raw)
        .unwrap();
    assert!(
        !raw.contains("PRIVATE_DICTATION_MARKER"),
        "default redacted share contains plaintext dictation history"
    );
}

#[test]
fn both_modes_exclude_usage_records_and_tombstones() {
    let mut set = fixture().documents().clone();
    set.documents.push(serde_json::from_value(json!({"id":"history-private","kind":"history","schemaVersion":1,"value":{"id":"history-private","createdAt":"2026-09-26T00:00:00Z","source":"quick_note","rawTranscript":"PRIVATE_HISTORY","finalText":"PRIVATE_HISTORY","mode":"raw","insertStatus":"notRequested","hasAudioRecording":false,"sortIndex":0}})).unwrap());
    set.documents.push(serde_json::from_value(json!({"id":"private-device:2026-09-26","kind":"activity","schemaVersion":1,"value":{"sourceDeviceId":"private-device","date":"2026-09-26","count":3,"chars":10,"durationMs":20}})).unwrap());
    for (kind, id) in [
        ("history", "deleted-history"),
        ("activity", "other-device:2026-09-26"),
    ] {
        set.tombstones.push(
            serde_json::from_value(
                json!({"id":id,"kind":kind,"deletedAt":"2026-09-26T00:00:00Z","baseRevision":"0"}),
            )
            .unwrap(),
        );
    }
    let docs = parse_sync_documents(&serde_json::to_vec(&set).unwrap(), Revision::new(0)).unwrap();
    for personal in [false, true] {
        let password = personal.then(|| BundlePassword::new("test-password").unwrap());
        let artifact = export_validated_bundle(
            &docs,
            BundleExportOptions {
                mode: if personal {
                    BundleMode::PersonalMigration
                } else {
                    BundleMode::ShareRedacted
                },
                credential_policy: if personal {
                    CredentialPolicy::PasswordEncrypted
                } else {
                    CredentialPolicy::Omit
                },
                password: password.clone(),
                ..BundleExportOptions::default()
            },
        )
        .unwrap();
        let decoded = decode_bundle(&artifact.bytes, password.as_ref(), Revision::new(0)).unwrap();
        assert_eq!(decoded.inspection.section_counts.history, 0);
        assert_eq!(decoded.inspection.section_counts.activity, 0);
        assert!(decoded.documents.documents().tombstones.is_empty());
        let mut archive = zip::ZipArchive::new(Cursor::new(&artifact.bytes[..])).unwrap();
        let mut raw = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("documents.json").unwrap(), &mut raw)
            .unwrap();
        assert!(!raw.contains("PRIVATE_HISTORY"));
        assert!(!raw.contains("private-device"));
    }
}

#[test]
fn malformed_android_state_is_rejected_during_inspection_and_decode() {
    let valid = json!({"schemaVersion":1,"learningPrivacyVersion":1,"preferences":{},"strokeFrequency":{},"pinyinLearnedPhrases":{}});
    let mut invalid = vec![json!({}), valid.clone()];
    invalid[1]["learningPrivacyVersion"] = json!(0);
    for (key, value) in [
        ("hapticEnabled", json!("true")),
        ("hapticAmplitude", json!(0)),
        ("hapticAmplitude", json!(1.5)),
        ("locale", json!(1)),
        ("themeMode", json!("invalid")),
    ] {
        let mut value_root = valid.clone();
        value_root["preferences"][key] = value;
        invalid.push(value_root);
    }
    for value in invalid {
        let mut set = fixture().documents().clone();
        set.documents.push(serde_json::from_value(json!({"id":"androidPortableSettings","kind":"ui_preferences","schemaVersion":1,"value":value})).unwrap());
        let docs =
            parse_sync_documents(&serde_json::to_vec(&set).unwrap(), Revision::new(0)).unwrap();
        let artifact = export_validated_bundle(&docs, BundleExportOptions::default()).unwrap();
        assert!(matches!(
            inspect_bundle(&artifact.bytes),
            Err(BundleError::InvalidSchema)
        ));
        assert!(matches!(
            decode_bundle(&artifact.bytes, None, Revision::new(0)),
            Err(BundleError::InvalidSchema)
        ));
    }
}
