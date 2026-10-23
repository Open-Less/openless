use super::*;
use serde_json::{json, Value};

fn scope() -> SyncScope {
    SyncScope {
        service_origin: "https://sync.example.test".into(),
        owner_github_id: "42".into(),
        vault_id: uuid::Uuid::new_v4().to_string(),
        key_id: uuid::Uuid::new_v4().to_string(),
        device_id: "device-a".into(),
    }
}
fn full() -> ExportedDocuments {
    let mut snapshot = super::tests::channel_snapshot();
    snapshot.preferences = SecretJson::new(
        json!({"defaultMode":"raw", "enabledModes":["raw","light"], "activeStylePackId":"", "styleSystemPrompts":{"light":"PRIVATE-PROMPT"}, "themeMode":"dark"}),
    );
    snapshot.dictionary = vec![
        SecretJson::new(
            json!({"id":"confirmed", "phrase":"approved-word", "enabled":true, "note":"PRIVATE-NOTE", "hits":999, "createdAt":"2026-10-16T00:00:00Z"}),
        ),
        SecretJson::new(
            json!({"id":"candidate", "phrase":"UNCONFIRMED-WORD", "enabled":true, "note":crate::shared_types::LEARNED_VOCAB_NOTE, "createdAt":"2026-10-16T00:00:00Z"}),
        ),
    ];
    snapshot.history = vec![SecretJson::new(
        json!({"id":"history", "rawTranscript":"PRIVATE-HISTORY", "finalText":"PRIVATE-HISTORY", "mode":"raw", "insertStatus":"notRequested", "createdAt":"2026-10-16T00:00:00Z"}),
    )];
    export_snapshot(snapshot).unwrap()
}
fn limited(full: &ExportedDocuments, scope: &SyncScope) -> LimitedSyncDocuments {
    let selection =
        ConfirmedVocabularySelection::new(scope.clone(), full.generation, vec!["confirmed".into()])
            .unwrap();
    LimitedSyncDocuments::project(full, scope, &selection).unwrap()
}
fn value(documents: &LimitedSyncDocuments) -> Value {
    serde_json::from_slice(&documents.to_secret_json().unwrap()).unwrap()
}
fn parse(value: Value, scope: &SyncScope) -> DocumentResult<LimitedSyncDocuments> {
    LimitedSyncDocuments::from_authenticated_json(
        &serde_json::to_vec(&value).unwrap(),
        scope,
        Revision::new(3),
    )
}

#[test]
fn limited_export_never_contains_full_snapshot_secrets_or_unconfirmed_vocabulary() {
    assert_eq!(AllowedSyncScope::default(), AllowedSyncScope::FullSnapshot);
    let full = full();
    let scope = scope();
    let raw_full = serde_json::to_string(full.documents.documents()).unwrap();
    for marker in [
        "fixture-secret-original",
        "PRIVATE-HISTORY",
        "PRIVATE-PROMPT",
        "PRIVATE-NOTE",
        "UNCONFIRMED-WORD",
    ] {
        assert!(
            raw_full.contains(marker),
            "full default must remain unchanged: {marker}"
        );
    }
    let raw_limited =
        String::from_utf8(limited(&full, &scope).to_secret_json().unwrap().to_vec()).unwrap();
    for marker in [
        "fixture-secret-original",
        "PRIVATE-HISTORY",
        "PRIVATE-PROMPT",
        "PRIVATE-NOTE",
        "UNCONFIRMED-WORD",
        "hits",
        "channels",
        "styleSystemPrompts",
        "device_profile",
    ] {
        assert!(!raw_limited.contains(marker), "excluded: {marker}");
    }
    assert!(raw_limited.contains("approved-word"));
    let none = ConfirmedVocabularySelection::new(scope.clone(), full.generation, vec![]).unwrap();
    assert!(!String::from_utf8(
        LimitedSyncDocuments::project(&full, &scope, &none)
            .unwrap()
            .to_secret_json()
            .unwrap()
            .to_vec()
    )
    .unwrap()
    .contains("approved-word"));
}

#[test]
fn wrong_manifest_scope_account_key_version_and_forbidden_documents_fail_closed() {
    let full = full();
    let scope = scope();
    let original = limited(&full, &scope);
    for (key, wrong) in [
        ("namespace", json!("encrypted-full-snapshot-v1")),
        ("schemaVersion", json!(2)),
        ("ownerGithubId", json!("999")),
        ("keyId", json!(uuid::Uuid::new_v4().to_string())),
        ("vaultId", json!(uuid::Uuid::new_v4().to_string())),
        ("serviceOrigin", json!("https://other.example.test")),
    ] {
        let mut changed = value(&original);
        changed["manifest"][key] = wrong;
        assert!(parse(changed, &scope).is_err(), "{key}");
    }
    let mut changed = value(&original);
    changed["documents"]["documents"].as_array_mut().unwrap().push(json!({"id":"styleSystemPrompts", "kind":"preferences", "schemaVersion":1, "value":{"light":"private"}}));
    assert_eq!(
        parse(changed, &scope).unwrap_err(),
        DocumentError::ExcludedField
    );
    for kind in [
        DocumentKind::ProviderCredentials,
        DocumentKind::Channels,
        DocumentKind::History,
        DocumentKind::Activity,
        DocumentKind::StylePacks,
        DocumentKind::VocabularyPresets,
        DocumentKind::DeviceProfile,
        DocumentKind::UiPreferences,
        DocumentKind::Corrections,
    ] {
        let mut changed = value(&original);
        changed["documents"]["documents"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"forbidden", "kind":kind, "schemaVersion":1, "value":null}));
        assert_eq!(
            parse(changed, &scope).unwrap_err(),
            DocumentError::ExcludedField
        );
    }
    for key in ["note", "hits", "rawAudio", "apiKey"] {
        let mut changed = value(&original);
        let dictionary = changed["documents"]["documents"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|doc| doc["id"] == "confirmed")
            .unwrap();
        dictionary["value"][key] = json!("private");
        assert_eq!(
            parse(changed, &scope).unwrap_err(),
            DocumentError::ExcludedField
        );
    }
    assert!(
        DocumentSet::from_json_bytes(&original.to_secret_json().unwrap()).is_err(),
        "the full protocol must not silently accept the limited envelope"
    );
    let raw_full = serde_json::to_vec(full.documents.documents()).unwrap();
    assert!(
        LimitedSyncDocuments::from_authenticated_json(&raw_full, &scope, Revision::new(3)).is_err()
    );
    let bytes = original.to_secret_json().unwrap();
    let duplicate = String::from_utf8(bytes.to_vec()).unwrap().replacen(
        "\"schemaVersion\":1",
        "\"schemaVersion\":1,\"schemaVersion\":1",
        1,
    );
    assert!(LimitedSyncDocuments::from_authenticated_json(
        duplicate.as_bytes(),
        &scope,
        Revision::new(3)
    )
    .is_err());
}

#[test]
fn confirmation_selection_requires_exact_scope_generation_and_existing_ids() {
    let full = full();
    let scope = scope();
    for selection in [
        ConfirmedVocabularySelection::new(
            scope.clone(),
            Revision::new(0),
            vec!["confirmed".into()],
        )
        .unwrap(),
        ConfirmedVocabularySelection::new(
            super::limited_tests::scope(),
            full.generation,
            vec!["confirmed".into()],
        )
        .unwrap(),
    ] {
        assert_eq!(
            LimitedSyncDocuments::project(&full, &scope, &selection).unwrap_err(),
            DocumentError::StalePreview
        );
    }
    let missing =
        ConfirmedVocabularySelection::new(scope.clone(), full.generation, vec!["missing".into()])
            .unwrap();
    assert_eq!(
        LimitedSyncDocuments::project(&full, &scope, &missing).unwrap_err(),
        DocumentError::SourceChanged
    );
}

#[test]
fn limited_conflicts_and_deletions_reuse_core_three_way_rules() {
    let full = full();
    let scope = scope();
    let baseline = limited(&full, &scope);
    let mut left = value(&baseline);
    let mut right = value(&baseline);
    for (set, phrase) in [(&mut left, "left"), (&mut right, "right")] {
        for doc in set["documents"]["documents"].as_array_mut().unwrap() {
            if doc["id"] == "confirmed" {
                doc["value"]["phrase"] = json!(phrase);
            }
        }
    }
    let left = parse(left, &scope).unwrap();
    let right = parse(right, &scope).unwrap();
    let preview = left.diff(Some(&baseline), &right).unwrap();
    assert_eq!(preview.conflicts().len(), 1);
    let choice = ConflictChoice {
        conflict_id: preview.conflicts()[0].conflict_id.clone(),
        side: ConflictSide::Remote,
    };
    let merged = preview.resolve(&[choice]).unwrap();
    assert!(String::from_utf8(merged.to_secret_json().unwrap().to_vec())
        .unwrap()
        .contains("right"));
    let selection =
        ConfirmedVocabularySelection::new(scope.clone(), full.generation, vec![]).unwrap();
    let deleted = LimitedSyncDocuments::project(&full, &scope, &selection)
        .unwrap()
        .record_local_deletions(&baseline, "2026-10-16T00:00:00Z")
        .unwrap();
    let json = value(&deleted);
    assert_eq!(json["documents"]["tombstones"].as_array().unwrap().len(), 1);
    assert_eq!(json["documents"]["tombstones"][0]["id"], "confirmed");
    assert_eq!(
        deleted.diff(Some(&baseline), &right).unwrap().conflicts()[0].reason,
        ConflictReason::DeleteModify
    );
}
