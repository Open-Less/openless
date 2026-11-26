use super::*;
use crate::cloud_sync_e2ee::{LimitedSyncStore, TestAccount};

fn facade(f: &Fixture, account: &'static str) -> LimitedSyncStore {
    LimitedSyncStore::new(
        f.store.clone(),
        AllowedSyncScope::ConfirmedVocabularyModeStyleReferences,
        Arc::new(TestAccount(account)),
    )
    .unwrap()
}
fn selection(f: &Fixture, generation: Revision, ids: &[&str]) -> ConfirmedVocabularySelection {
    ConfirmedVocabularySelection::new(
        f.scope.clone(),
        generation,
        ids.iter().map(|id| (*id).into()).collect(),
    )
    .unwrap()
}
async fn seeded() -> Fixture {
    let f = Fixture::new();
    let before = f.store.export_scope(f.scope.clone()).await.unwrap();
    let mut set = before.documents.documents().clone();
    for (kind, id, value) in [
        (
            DocumentKind::Dictionary,
            "confirmed",
            json!({"id":"confirmed","phrase":"Approved","enabled":true,"note":"PRIVATE-NOTE","hits":17,"createdAt":"2026-10-16T00:00:00Z","sortIndex":0}),
        ),
        (
            DocumentKind::Dictionary,
            "unconfirmed",
            json!({"id":"unconfirmed","phrase":"Candidate","enabled":true,"note":crate::shared_types::LEARNED_VOCAB_NOTE,"hits":3,"createdAt":"2026-10-16T00:00:00Z","sortIndex":1}),
        ),
        (
            DocumentKind::History,
            "private-history",
            json!({"id":"private-history","createdAt":"2026-10-16T00:00:00Z","source":"quick_note","rawTranscript":"PRIVATE-HISTORY","finalText":"PRIVATE-HISTORY","mode":"raw","insertStatus":"notRequested","hasAudioRecording":false,"sortIndex":0}),
        ),
        (
            DocumentKind::Channels,
            "llm:fixture",
            json!({"id":"fixture","namespace":"llm","providerType":"requesty","name":"Private channel","enabled":true,"order":0,"active":true}),
        ),
        (
            DocumentKind::ProviderCredentials,
            "llm:fixture",
            json!({"channelId":"fixture","namespace":"llm","accounts":{"ark.api_key":"PRIVATE-API-KEY"}}),
        ),
    ] {
        set_doc(&mut set, kind, id, value);
    }
    f.store
        .restore_scope(validate(set), f.context(&before))
        .await
        .unwrap();
    f
}
fn changed(
    remote: &LimitedSyncDocuments,
    f: &Fixture,
    change: impl FnOnce(&mut Value),
) -> LimitedSyncDocuments {
    let mut value: Value = serde_json::from_slice(&remote.to_secret_json().unwrap()).unwrap();
    change(&mut value);
    LimitedSyncDocuments::from_authenticated_json(
        &serde_json::to_vec(&value).unwrap(),
        &f.scope,
        Revision::new(3),
    )
    .unwrap()
}
fn set_phrase(value: &mut Value, phrase: &str) {
    for doc in value["documents"]["documents"].as_array_mut().unwrap() {
        if doc["id"] == "confirmed" {
            doc["value"]["phrase"] = json!(phrase);
        }
    }
}

#[tokio::test]
async fn limited_restore_changes_only_confirmed_vocabulary_and_reference_preferences() {
    let f = seeded().await;
    let full = f.store.export_scope(f.scope.clone()).await.unwrap();
    let api = facade(&f, "42");
    assert!(LimitedSyncStore::new(
        f.store.clone(),
        AllowedSyncScope::default(),
        Arc::new(TestAccount("42"))
    )
    .is_err());
    let limited = api
        .export(
            f.scope.clone(),
            selection(&f, full.generation, &["confirmed"]),
        )
        .await
        .unwrap();
    let raw = String::from_utf8(limited.documents.to_secret_json().unwrap().to_vec()).unwrap();
    for marker in [
        "PRIVATE-HISTORY",
        "PRIVATE-API-KEY",
        "PRIVATE-NOTE",
        "Candidate",
    ] {
        assert!(!raw.contains(marker));
    }
    let remote = changed(&limited.documents, &f, |value| {
        set_phrase(value, "Remote approved");
        for doc in value["documents"]["documents"].as_array_mut().unwrap() {
            if doc["id"] == "defaultMode" {
                doc["value"] = json!("formal");
            }
            if doc["id"] == "marketStyleReference" {
                doc["value"] = json!({"id":"builtin.light","version":"2.0.0","originPackId":null});
            }
        }
    });
    api.restore(
        remote,
        f.context(&full),
        selection(&f, full.generation, &["confirmed"]),
    )
    .await
    .unwrap();
    let rows = f.repo.vocabulary.list().unwrap();
    assert_eq!(
        rows.iter()
            .find(|row| row.id == "confirmed")
            .unwrap()
            .phrase,
        "Remote approved"
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.id == "confirmed")
            .unwrap()
            .note
            .as_deref(),
        Some("PRIVATE-NOTE")
    );
    assert_eq!(
        rows.iter().find(|row| row.id == "confirmed").unwrap().hits,
        17
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.id == "unconfirmed")
            .unwrap()
            .phrase,
        "Candidate"
    );
    let after = f.store.export_scope(f.scope.clone()).await.unwrap();
    let untouched = |set: &DocumentSet| {
        set.documents
            .iter()
            .filter(|doc| {
                doc.kind != DocumentKind::Dictionary
                    && !(doc.kind == DocumentKind::Preferences
                        && matches!(doc.id.as_str(), "defaultMode" | "activeStylePackId"))
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        untouched(full.documents.documents()),
        untouched(after.documents.documents())
    );
}

#[tokio::test]
async fn limited_wrong_account_key_generation_missing_style_and_unconfirmed_collision_do_not_write()
{
    let f = seeded().await;
    let full = f.store.export_scope(f.scope.clone()).await.unwrap();
    let api = facade(&f, "42");
    let limited = api
        .export(
            f.scope.clone(),
            selection(&f, full.generation, &["confirmed"]),
        )
        .await
        .unwrap();
    let remote = changed(&limited.documents, &f, |value| {
        set_phrase(value, "Must not write")
    });
    assert_eq!(
        facade(&f, "999")
            .restore(
                remote.clone(),
                f.context(&full),
                selection(&f, full.generation, &["confirmed"])
            )
            .await
            .unwrap_err(),
        DocumentError::InvalidReference
    );
    let mut context = f.context(&full);
    context.scope.key_id = uuid::Uuid::new_v4().to_string();
    assert!(api
        .restore(
            remote.clone(),
            context,
            selection(&f, full.generation, &["confirmed"])
        )
        .await
        .is_err());
    let stale = selection(&f, Revision::new(full.generation.get() - 1), &["confirmed"]);
    assert_eq!(
        api.restore(remote.clone(), f.context(&full), stale)
            .await
            .unwrap_err(),
        DocumentError::StalePreview
    );
    let missing_style = changed(&limited.documents, &f, |value| {
        for doc in value["documents"]["documents"].as_array_mut().unwrap() {
            if doc["id"] == "marketStyleReference" {
                doc["value"] = json!({"id":"not-installed","version":"1.0.0","originPackId":null});
            }
        }
    });
    assert_eq!(
        api.restore(
            missing_style,
            f.context(&full),
            selection(&f, full.generation, &["confirmed"])
        )
        .await
        .unwrap_err(),
        DocumentError::InvalidReference
    );
    let omitted = changed(&limited.documents, &f, |value| {
        value["documents"]["documents"]
            .as_array_mut()
            .unwrap()
            .retain(|doc| doc["id"] != "confirmed");
    });
    assert_eq!(
        api.restore(
            omitted,
            f.context(&full),
            selection(&f, full.generation, &["confirmed"])
        )
        .await
        .unwrap_err(),
        DocumentError::MissingTombstone
    );
    for tombstone in [false, true] {
        let collision = changed(&limited.documents, &f, |value| {
            if tombstone {
                value["documents"]["tombstones"] = json!([{"id":"unconfirmed","kind":"dictionary","deletedAt":"2026-10-16T00:00:00Z","baseRevision":"3"}]);
            } else {
                value["documents"]["documents"].as_array_mut().unwrap().push(json!({"id":"unconfirmed","kind":"dictionary","schemaVersion":1,"value":{"id":"unconfirmed","phrase":"bad overwrite","enabled":true,"createdAt":"2026-10-16T00:00:00Z","sortIndex":1}}));
            }
        });
        assert_eq!(
            api.restore(
                collision,
                f.context(&full),
                selection(&f, full.generation, &["confirmed"])
            )
            .await
            .unwrap_err(),
            DocumentError::InvalidReference
        );
    }
    let after = f.store.export_scope(f.scope.clone()).await.unwrap();
    assert_eq!(
        after.documents.documents().documents,
        full.documents.documents().documents
    );
    assert_eq!(after.generation, full.generation);
    // A real ordinary write after preview is retained, never overwritten by stale restore.
    f.repo
        .preferences
        .update(|prefs| prefs.show_capsule = !prefs.show_capsule)
        .unwrap();
    assert_eq!(
        api.restore(
            remote,
            f.context(&full),
            selection(&f, full.generation, &["confirmed"])
        )
        .await
        .unwrap_err(),
        DocumentError::StalePreview
    );
}

#[tokio::test]
async fn limited_deletion_removes_only_confirmed_rows_and_keeps_unconfirmed_rows() {
    let f = seeded().await;
    let full = f.store.export_scope(f.scope.clone()).await.unwrap();
    let api = facade(&f, "42");
    let limited = api
        .export(
            f.scope.clone(),
            selection(&f, full.generation, &["confirmed"]),
        )
        .await
        .unwrap();
    let remote = changed(&limited.documents, &f, |value| {
        value["documents"]["documents"]
            .as_array_mut()
            .unwrap()
            .retain(|doc| doc["id"] != "confirmed");
        value["documents"]["tombstones"] = json!([{"id":"confirmed","kind":"dictionary","deletedAt":"2026-10-16T00:00:00Z","baseRevision":"3"}]);
    });
    let restored = api
        .restore(
            remote,
            f.context(&full),
            selection(&f, full.generation, &["confirmed"]),
        )
        .await
        .unwrap();
    let returned: Value =
        serde_json::from_slice(&restored.documents.to_secret_json().unwrap()).unwrap();
    assert_eq!(returned["documents"]["tombstones"][0]["id"], "confirmed");
    let rows = f.repo.vocabulary.list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "unconfirmed");
}

#[tokio::test]
async fn limited_partial_failure_uses_existing_encrypted_rollback_and_recovery() {
    for failures in [vec![true, false], vec![true, true, false]] {
        let f = seeded().await;
        let full = f.store.export_scope(f.scope.clone()).await.unwrap();
        let api = facade(&f, "42");
        let limited = api
            .export(
                f.scope.clone(),
                selection(&f, full.generation, &["confirmed"]),
            )
            .await
            .unwrap();
        let remote = changed(&limited.documents, &f, |value| {
            set_phrase(value, "Roll back me")
        });
        f.credentials
            .failures
            .lock()
            .unwrap()
            .extend(failures.clone());
        let error = api
            .restore(
                remote,
                f.context(&full),
                selection(&f, full.generation, &["confirmed"]),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            if failures.len() == 2 {
                DocumentError::RestoreRolledBack
            } else {
                DocumentError::RecoveryRequired
            }
        );
        if failures.len() == 3 {
            f.store.recover_registered().await.unwrap();
        }
        let after = f.store.export_scope(f.scope.clone()).await.unwrap();
        assert_eq!(
            after.documents.documents().documents,
            full.documents.documents().documents
        );
        assert_eq!(after.generation, full.generation);
    }
}

#[tokio::test]
async fn limited_restore_blocks_concurrent_repository_and_credential_mutations() {
    let f = seeded().await;
    let full = f.store.export_scope(f.scope.clone()).await.unwrap();
    let api = facade(&f, "42");
    let limited = api
        .export(
            f.scope.clone(),
            selection(&f, full.generation, &["confirmed"]),
        )
        .await
        .unwrap();
    let remote = changed(&limited.documents, &f, |value| {
        set_phrase(value, "Gated update")
    });
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    *f.effects.wait.lock().unwrap() = Some(barrier.clone());
    let context = f.context(&full);
    let confirmed = selection(&f, full.generation, &["confirmed"]);
    let restore = tokio::spawn(async move { api.restore(remote, context, confirmed).await });
    barrier.wait().await;
    assert!(!restore.is_finished());
    assert!(f.repo.vocabulary.add("Concurrent".into(), None).is_err());
    assert!(f.store.inner.gate.begin_mutation().is_err());
    // Fixture effects also assert that the credential store's exact shared gate is held.
    barrier.wait().await;
    restore.await.unwrap().unwrap();
}

#[tokio::test]
async fn limited_wrong_journal_key_cannot_replay_or_clear_pending_local_recovery() {
    let f = seeded().await;
    let full = f.store.export_scope(f.scope.clone()).await.unwrap();
    let api = facade(&f, "42");
    let limited = api
        .export(
            f.scope.clone(),
            selection(&f, full.generation, &["confirmed"]),
        )
        .await
        .unwrap();
    let remote = changed(&limited.documents, &f, |value| {
        set_phrase(value, "Pending image")
    });
    f.credentials.failures.lock().unwrap().extend([true, true]);
    assert_eq!(
        api.restore(
            remote,
            f.context(&full),
            selection(&f, full.generation, &["confirmed"])
        )
        .await
        .unwrap_err(),
        DocumentError::RecoveryRequired
    );
    let dictionary_path = f._temp.0.join("dictionary.json");
    // Snapshot real local file bytes even when the normal API is recovery-fenced.
    let dictionary_before = std::fs::read(&dictionary_path).unwrap();
    let preferences_before = std::fs::read(f._temp.0.join("preferences.json")).unwrap();
    let pending_before = f.store.inner.gate.registered_recovery_scopes().unwrap();
    let wrong_extensions = Arc::new(Extensions {
        values: Mutex::new(f.extensions.values.lock().unwrap().clone()),
        protector: Arc::new(CryptoJournalProtector::new(Arc::new(
            DerivedKey::from_secret_bytes(Zeroizing::new([99; 32])),
        ))),
        key_requested: AtomicBool::new(false),
        ui_revision: Mutex::new(f.extensions.ui_revision.lock().unwrap().clone()),
    });
    let wrong_store = CoreSyncStore::new(
        f.repo.clone(),
        Arc::new(f.credentials.clone()),
        f._temp.0.clone(),
        f.store.device(),
        f.store.inner.gate.clone(),
        wrong_extensions,
        Arc::new(crate::TokioTaskSpawner),
    )
    .unwrap();
    wrong_store
        .bind_runtime_idle_probe(Arc::new(|| true))
        .unwrap();
    wrong_store
        .bind_runtime_effects(Arc::new(f.effects.clone()))
        .unwrap();
    assert!(wrong_store.recover_registered().await.is_err());
    assert_eq!(std::fs::read(&dictionary_path).unwrap(), dictionary_before);
    assert_eq!(
        std::fs::read(f._temp.0.join("preferences.json")).unwrap(),
        preferences_before
    );
    assert_eq!(
        f.store.inner.gate.registered_recovery_scopes().unwrap(),
        pending_before
    );
    assert!(f.store.is_recovering());
    f.store.recover_registered().await.unwrap();
    let recovered = f.store.export_scope(f.scope.clone()).await.unwrap();
    assert_eq!(
        recovered.documents.documents().documents,
        full.documents.documents().documents
    );
}

#[tokio::test]
async fn limited_public_factory_uses_core_local_storage_without_full_service_or_remote_requests() {
    let source = Fixture::new();
    let temp = Temp::new();
    let credentials = TestCredentials {
        bound_gate: Arc::new(Mutex::new(None)),
        ..source.credentials.clone()
    };
    let store = LimitedSyncStore::build(
        crate::cloud_sync_e2ee::EncryptedSyncConfig {
            service_origin: "https://sync.example.test".into(),
            app_version: "test".into(),
        },
        &temp.0,
        BackendRepositories::open(&temp.0).unwrap(),
        Arc::new(credentials),
        Arc::new(crate::TokioTaskSpawner),
        AllowedSyncScope::ConfirmedVocabularyModeStyleReferences,
        Arc::new(TestAccount("42")),
    )
    .unwrap();
    assert!(temp.0.join("encrypted-sync-limited-v1/device-id").is_file());
    assert!(!temp.0.join("encrypted-sync/device-id").exists());
    let scope = SyncScope {
        device_id: store.device().id.clone(),
        ..source.scope.clone()
    };
    let confirmed =
        ConfirmedVocabularySelection::new(scope.clone(), store.generation().unwrap(), vec![])
            .unwrap();
    store.export(scope, confirmed).await.unwrap();
    assert!(!store.is_recovering());
    // Production export above needed no crypto key yet. Restore still requires the real
    // platform effects + secure vault hooks; this fixture cannot silently substitute them.
}
