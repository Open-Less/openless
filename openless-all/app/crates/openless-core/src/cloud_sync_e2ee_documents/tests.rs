use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::*;
use crate::cloud_sync_e2ee_protocol::types::{
    DocumentKind, LogicalDocument, Revision, SourceDevice, Tombstone,
};

pub(super) fn source() -> SourceDevice {
    SourceDevice {
        id: "device-a".into(),
        os: "macos".into(),
        arch: "aarch64".into(),
        app_version: "2.0.0-Beta.3".into(),
    }
}

pub(super) fn snapshot() -> ExportSnapshot {
    ExportSnapshot {
        source_device: source(),
        exported_at: "2026-09-26T00:00:00Z".into(),
        generation: Revision::new(7),
        base_revision: Revision::new(3),
        preferences: SecretJson::new(json!({"themeMode":"system","showCapsule":true})),
        ui_preferences: ui_preferences("en", "medium"),
        channels: Vec::new(),
        provider_credentials: Vec::new(),
        dictionary: Vec::new(),
        vocabulary_presets: Vec::new(),
        corrections: Vec::new(),
        style_packs: Vec::new(),
        history: Vec::new(),
        activity: Vec::new(),
        window_positions: Vec::new(),
        retained_documents: Vec::new(),
        tombstones: Vec::new(),
    }
}

pub(super) fn sample() -> ValidatedSyncDocuments {
    export_snapshot(snapshot()).unwrap().documents
}

fn change(
    set: &ValidatedSyncDocuments,
    kind: DocumentKind,
    id: &str,
    value: Value,
) -> ValidatedSyncDocuments {
    let mut set = set.documents().clone();
    if let Some(doc) = set
        .documents
        .iter_mut()
        .find(|doc| doc.kind == kind && doc.id == id)
    {
        doc.value = value;
    } else {
        set.documents.push(LogicalDocument {
            id: id.into(),
            kind,
            schema_version: 1,
            value,
        });
    }
    validate_sync_documents(set, Revision::new(4)).unwrap()
}

fn dictionary(id: &str, text: &str) -> Value {
    json!({"id":id,"phrase":text,"note":null,"enabled":true,"hits":0,"createdAt":"2026-09-26T00:00:00Z"})
}

pub(super) fn channel_snapshot() -> ExportSnapshot {
    let mut data = snapshot();
    data.channels.push(ChannelRecord {
        id: "llm-one".into(),
        namespace: SyncNamespace::Llm,
        provider_type: "requesty".into(),
        name: "original".into(),
        enabled: true,
        order: 0,
        active: true,
    });
    data.provider_credentials.push(ProviderCredentialRecord {
        channel_id: "llm-one".into(),
        namespace: SyncNamespace::Llm,
        accounts: BTreeMap::from([("ark.api_key".into(), "fixture-secret-original".into())]),
    });
    data
}

#[test]
fn all_current_preference_fields_have_an_explicit_registration() {
    let source = include_str!("../shared_types.rs");
    let fields = source
        .split("pub struct UserPreferences {")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    let names: std::collections::BTreeSet<_> = fields
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub ")
                .and_then(|s| s.split_once(':'))
                .map(|(name, _)| name)
        })
        .collect();
    let registered: std::collections::BTreeSet<_> = registry::PREFERENCE_FIELDS
        .iter()
        .map(|field| field.rust_name)
        .collect();
    assert_eq!(
        names, registered,
        "new persistent fields require a reviewed sync classification"
    );
    assert_eq!(registered.len(), registry::PREFERENCE_FIELDS.len());
}

#[test]
fn controlled_export_keeps_real_service_keys_but_excludes_local_secrets_and_grants() {
    let mut data = channel_snapshot();
    data.preferences = SecretJson::new(
        json!({"themeMode":"dark","remoteInputPin":"fixture-pin-never-export","codingAgentEnabled":true,"cursorContextEnabled":true,"marketplaceDevLogin":"fixture-login","splashSeenVersion":"2","activeLlmProvider":"llm-one","codingAgentExe":"/fixture/never-run","futureAppearance":{"accent":"violet"}}),
    );
    let exported = export_snapshot(data).unwrap();
    let raw = serde_json::to_string(exported.documents.documents()).unwrap();
    assert!(
        raw.contains("fixture-secret-original"),
        "ordinary service credentials must not be masked"
    );
    assert!(!raw.contains("fixture-pin-never-export"));
    assert!(!raw.contains("codingAgentEnabled"));
    assert!(!raw.contains("cursorContextEnabled"));
    assert!(!raw.contains("fixture-login"));
    assert!(!raw.contains("activeLlmProvider"));
    assert!(raw.contains("futureAppearance"));
    let profile = exported
        .documents
        .documents()
        .documents
        .iter()
        .find(|doc| doc.kind == DocumentKind::DeviceProfile)
        .unwrap();
    assert_eq!(
        profile.value["preferences"]["codingAgentExe"],
        "/fixture/never-run"
    );
    assert!(!format!("{:?}", exported).contains("fixture-secret-original"));
}

#[test]
fn local_asr_channels_are_archived_with_their_credentials_not_globally_activated() {
    let mut data = snapshot();
    data.channels.push(ChannelRecord {
        id: "local-one".into(),
        namespace: SyncNamespace::Asr,
        provider_type: "foundry-local-whisper".into(),
        name: "Windows model".into(),
        enabled: true,
        order: 0,
        active: true,
    });
    data.provider_credentials.push(ProviderCredentialRecord {
        channel_id: "local-one".into(),
        namespace: SyncNamespace::Asr,
        accounts: BTreeMap::from([("asr.model".into(), "fixture-model".into())]),
    });
    let exported = export_snapshot(data).unwrap();
    assert!(!exported
        .documents
        .documents()
        .documents
        .iter()
        .any(|doc| matches!(
            doc.kind,
            DocumentKind::Channels | DocumentKind::ProviderCredentials
        )));
    let profile = exported
        .documents
        .documents()
        .documents
        .iter()
        .find(|doc| doc.kind == DocumentKind::DeviceProfile)
        .unwrap();
    assert_eq!(profile.value["channels"][0]["id"], "local-one");
    assert_eq!(
        profile.value["providerCredentials"][0]["accounts"]["asr.model"],
        "fixture-model"
    );
}

#[test]
fn history_never_claims_that_remote_audio_exists_locally() {
    let mut data = snapshot();
    data.history.push(SecretJson::new(json!({"id":"history-one","createdAt":"2026-09-26T00:00:00Z","source":"quick_note","rawTranscript":"note","finalText":"note","mode":"raw","insertStatus":"notRequested","hasAudioRecording":true})));
    let exported = export_snapshot(data).unwrap();
    let row = exported
        .documents
        .documents()
        .documents
        .iter()
        .find(|doc| doc.kind == DocumentKind::History)
        .unwrap();
    assert_eq!(row.value["hasAudioRecording"], false);
    assert_eq!(row.value["source"], "quick_note");
}

#[test]
fn validation_rejects_duplicate_ids_versions_dangling_credentials_and_excluded_preferences() {
    let base = sample();
    let mut invalid = base.documents().clone();
    invalid.documents.push(invalid.documents[0].clone());
    assert_eq!(
        validate_sync_documents(invalid, Revision::new(3)).unwrap_err(),
        DocumentError::DuplicateId
    );
    let mut invalid = base.documents().clone();
    invalid.documents[0].schema_version = 2;
    assert_eq!(
        validate_sync_documents(invalid, Revision::new(3)).unwrap_err(),
        DocumentError::Unsupported
    );
    let mut invalid = base.documents().clone();
    invalid.documents.push(LogicalDocument {
        id: "remoteInputPin".into(),
        kind: DocumentKind::Preferences,
        schema_version: 1,
        value: json!("forbidden"),
    });
    assert_eq!(
        validate_sync_documents(invalid, Revision::new(3)).unwrap_err(),
        DocumentError::ExcludedField
    );
    let mut invalid = export_snapshot(channel_snapshot())
        .unwrap()
        .documents
        .documents()
        .clone();
    invalid
        .documents
        .retain(|doc| doc.kind != DocumentKind::Channels);
    assert_eq!(
        validate_sync_documents(invalid, Revision::new(3)).unwrap_err(),
        DocumentError::InvalidReference
    );
}

#[test]
fn credential_batch_validation_rejects_oauth_cookie_accounts_and_omni_id_aliases() {
    let mut data = channel_snapshot();
    data.provider_credentials[0]
        .accounts
        .insert("github.access_token".into(), "fixture-oauth".into());
    assert_eq!(
        validate_credential_set(&data.channels, &data.provider_credentials).unwrap_err(),
        DocumentError::ExcludedField
    );
    data.provider_credentials[0]
        .accounts
        .remove("github.access_token");
    data.channels[0].namespace = SyncNamespace::Omni;
    data.provider_credentials[0].namespace = SyncNamespace::Omni;
    data.provider_credentials[0].accounts.clear();
    assert_eq!(
        validate_credential_set(&data.channels, &data.provider_credentials).unwrap_err(),
        DocumentError::InvalidReference
    );
    data.channels[0].id = "requesty".into();
    data.provider_credentials[0].channel_id = "requesty".into();
    validate_credential_set(&data.channels, &data.provider_credentials).unwrap();
    let mut cookie = channel_snapshot();
    cookie.provider_credentials[0].accounts.insert(
        "ark.extra_headers".into(),
        r#"{"Cookie":"fixture-session"}"#.into(),
    );
    assert_eq!(
        export_snapshot(cookie).unwrap_err(),
        DocumentError::ExcludedField
    );
}

#[test]
fn unknown_preference_values_roundtrip_and_merge_without_application() {
    let base = sample();
    let local = change(
        &base,
        DocumentKind::Preferences,
        "futureAppearance",
        json!({"value":"preserve-me"}),
    );
    let remote = change(&base, DocumentKind::Preferences, "themeMode", json!("dark"));
    let merged = diff_sync_documents(Some(&base), &local, &remote)
        .unwrap()
        .resolve(&[])
        .unwrap();
    assert!(merged
        .documents()
        .documents
        .iter()
        .any(|doc| doc.id == "futureAppearance" && doc.value["value"] == "preserve-me"));
}

#[test]
fn independent_stable_ids_merge_but_simultaneous_same_key_changes_do_not() {
    let base = sample();
    let local = change(
        &base,
        DocumentKind::Dictionary,
        "local-id",
        dictionary("local-id", "same name"),
    );
    let remote = change(
        &base,
        DocumentKind::Dictionary,
        "remote-id",
        dictionary("remote-id", "same name"),
    );
    let result = diff_sync_documents(Some(&base), &local, &remote)
        .unwrap()
        .resolve(&[])
        .unwrap();
    assert_eq!(
        result
            .documents()
            .documents
            .iter()
            .filter(|doc| doc.kind == DocumentKind::Dictionary)
            .count(),
        2
    );
    let local = change(&base, DocumentKind::Preferences, "themeMode", json!("dark"));
    let remote = change(
        &base,
        DocumentKind::Preferences,
        "themeMode",
        json!("light"),
    );
    let preview = diff_sync_documents(Some(&base), &local, &remote).unwrap();
    assert_eq!(preview.conflicts().len(), 1);
    assert_eq!(
        preview.resolve(&[]).unwrap_err(),
        DocumentError::ConflictChoiceRequired
    );
}

#[test]
fn channel_and_credential_changes_are_one_redacted_conflict_unit() {
    let base = export_snapshot(channel_snapshot()).unwrap().documents;
    let mut channel = base
        .documents()
        .documents
        .iter()
        .find(|doc| doc.kind == DocumentKind::Channels)
        .unwrap()
        .value
        .clone();
    channel["name"] = json!("private-local-name");
    let local = change(&base, DocumentKind::Channels, "llm:llm-one", channel);
    let mut credentials = base
        .documents()
        .documents
        .iter()
        .find(|doc| doc.kind == DocumentKind::ProviderCredentials)
        .unwrap()
        .value
        .clone();
    credentials["accounts"]["ark.api_key"] = json!("fixture-secret-remote");
    let remote = change(
        &base,
        DocumentKind::ProviderCredentials,
        "llm:llm-one",
        credentials,
    );
    let preview = diff_sync_documents(Some(&base), &local, &remote).unwrap();
    assert_eq!(preview.conflicts().len(), 1);
    assert!(preview.conflicts()[0].is_credential_unit);
    let ui = serde_json::to_string(preview.conflicts()).unwrap();
    assert!(!ui.contains("fixture-secret"));
    assert!(!ui.contains("private-local-name"));
    let choice = ConflictChoice {
        conflict_id: preview.conflicts()[0].conflict_id.clone(),
        side: ConflictSide::Remote,
    };
    let merged = preview.resolve(&[choice]).unwrap();
    let channel = merged
        .documents()
        .documents
        .iter()
        .find(|doc| doc.kind == DocumentKind::Channels)
        .unwrap();
    assert_eq!(
        channel.value["name"], "original",
        "must not combine half of each channel unit"
    );
}

#[test]
fn deletion_marks_prevent_offline_resurrection_and_conflict_with_edits() {
    let base = change(
        &sample(),
        DocumentKind::Dictionary,
        "entry",
        dictionary("entry", "original"),
    );
    let mut deleted = base.documents().clone();
    deleted.documents.retain(|doc| doc.id != "entry");
    deleted.tombstones.push(Tombstone {
        id: "entry".into(),
        kind: DocumentKind::Dictionary,
        deleted_at: "2026-09-26T01:00:00Z".into(),
        base_revision: Revision::new(4),
    });
    let remote = validate_sync_documents(deleted, Revision::new(5)).unwrap();
    let merged = diff_sync_documents(Some(&base), &base, &remote)
        .unwrap()
        .resolve(&[])
        .unwrap();
    assert!(merged
        .documents()
        .tombstones
        .iter()
        .any(|t| t.id == "entry"));
    assert!(!merged
        .documents()
        .documents
        .iter()
        .any(|doc| doc.id == "entry"));
    let edited = change(
        &base,
        DocumentKind::Dictionary,
        "entry",
        dictionary("entry", "edited"),
    );
    let preview = diff_sync_documents(Some(&base), &edited, &remote).unwrap();
    assert_eq!(preview.conflicts()[0].reason, ConflictReason::DeleteModify);
    let mut missing = base.documents().clone();
    missing.documents.retain(|doc| doc.id != "entry");
    let missing = validate_sync_documents(missing, Revision::new(4)).unwrap();
    assert_eq!(
        diff_sync_documents(Some(&base), &missing, &base).unwrap_err(),
        DocumentError::MissingTombstone
    );
}

#[test]
fn no_common_baseline_never_treats_a_missing_local_id_as_a_delete() {
    let local = sample();
    let remote = change(
        &local,
        DocumentKind::Dictionary,
        "cloud",
        dictionary("cloud", "remote"),
    );
    let result = diff_sync_documents(None, &local, &remote)
        .unwrap()
        .resolve(&[])
        .unwrap();
    assert!(result
        .documents()
        .documents
        .iter()
        .any(|doc| doc.id == "cloud"));
}

#[test]
fn limits_reject_deep_values_and_oversized_full_snapshots_without_truncating() {
    let mut value = json!(true);
    for _ in 0..65 {
        value = json!([value]);
    }
    let mut set = sample().documents().clone();
    set.documents.push(LogicalDocument {
        id: "futureNested".into(),
        kind: DocumentKind::Preferences,
        schema_version: 1,
        value,
    });
    assert_eq!(
        validate_sync_documents(set, Revision::new(3)).unwrap_err(),
        DocumentError::PayloadTooLarge
    );
    let mut set = sample().documents().clone();
    set.documents.push(LogicalDocument {
        id: "futureText".into(),
        kind: DocumentKind::Preferences,
        schema_version: 1,
        value: json!("x".repeat(MAX_JSON_BYTES)),
    });
    assert_eq!(
        validate_sync_documents(set, Revision::new(3)).unwrap_err(),
        DocumentError::PayloadTooLarge
    );
}

#[test]
fn tombstone_materialization_preserves_all_old_deletions() {
    let base = change(
        &sample(),
        DocumentKind::Dictionary,
        "remove",
        dictionary("remove", "old"),
    );
    let mut current = base.documents().clone();
    current.documents.retain(|doc| doc.id != "remove");
    let updated = record_missing_tombstones(current, &base, "2026-09-26T02:00:00Z").unwrap();
    assert_eq!(updated.documents().tombstones.len(), 1);
    let mut next = updated.documents().clone();
    next.tombstones.clear();
    let preserved = record_missing_tombstones(next, &updated, "2026-09-26T03:00:00Z").unwrap();
    assert_eq!(
        preserved.documents().tombstones,
        updated.documents().tombstones
    );
}

#[test]
fn scope_and_restore_preview_require_the_exact_revision_and_device() {
    let scope = SyncScope {
        service_origin: "https://sync.example.test".into(),
        owner_github_id: "42".into(),
        vault_id: "12345678-1234-4234-8234-123456789abc".into(),
        key_id: "22345678-1234-4234-8234-123456789abc".into(),
        device_id: "device-a".into(),
    };
    let context = RestoreContext {
        scope,
        operation_id: "32345678-1234-4234-8234-123456789abc".into(),
        observed_revision: Revision::new(99),
        local_generation: Revision::new(7),
        target_device: source(),
    };
    assert_eq!(
        prepare_sync_restore(sample(), context).unwrap_err(),
        DocumentError::StalePreview
    );
}

#[test]
fn simultaneous_active_channel_additions_are_a_resolvable_redacted_selection() {
    let base = sample();
    let mut local = channel_snapshot();
    let mut remote = channel_snapshot();
    local.channels[0].id = "local-new".into();
    local.provider_credentials[0].channel_id = "local-new".into();
    remote.channels[0].id = "remote-new".into();
    remote.provider_credentials[0].channel_id = "remote-new".into();
    let local = export_snapshot(local).unwrap().documents;
    let remote = export_snapshot(remote).unwrap().documents;
    let preview = diff_sync_documents(Some(&base), &local, &remote).unwrap();
    assert_eq!(preview.conflicts().len(), 1);
    let choices = preview
        .conflicts()
        .iter()
        .map(|conflict| ConflictChoice {
            conflict_id: conflict.conflict_id.clone(),
            side: ConflictSide::Remote,
        })
        .collect::<Vec<_>>();
    let merged = preview.resolve(&choices).unwrap();
    let channels = merged
        .documents()
        .documents
        .iter()
        .filter(|doc| doc.kind == DocumentKind::Channels)
        .collect::<Vec<_>>();
    assert_eq!(channels.len(), 2);
    assert_eq!(
        channels
            .iter()
            .filter(|doc| doc.value["active"] == true)
            .count(),
        1
    );
    assert!(channels
        .iter()
        .any(|doc| doc.id == "llm:remote-new" && doc.value["active"] == true));
}

#[test]
fn collection_order_is_preserved_independently_of_stable_id_sorting() {
    let mut source = snapshot();
    source.dictionary = vec![
        SecretJson::new(dictionary("z", "newest")),
        SecretJson::new(dictionary("a", "oldest")),
    ];
    let exported = export_snapshot(source).unwrap();
    let docs = &exported.documents.documents().documents;
    assert_eq!(
        docs.iter()
            .find(|doc| doc.kind == DocumentKind::Dictionary && doc.id == "z")
            .unwrap()
            .value["sortIndex"],
        0
    );
    assert_eq!(
        docs.iter()
            .find(|doc| doc.kind == DocumentKind::Dictionary && doc.id == "a")
            .unwrap()
            .value["sortIndex"],
        1
    );
}
