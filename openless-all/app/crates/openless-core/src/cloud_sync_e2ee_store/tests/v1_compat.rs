use super::*;
use crate::cloud_sync_e2ee_protocol::{crypto, types::*};

#[path = "../../../tests/fixtures/v1-test-device.rs"]
mod v1_test_device;
use v1_test_device::fixture_for_device;

fn doc_value<'a>(set: &'a DocumentSet, kind: DocumentKind, id: &str) -> &'a Value {
    &set.documents
        .iter()
        .find(|d| d.kind == kind && d.id == id)
        .unwrap()
        .value
}

#[tokio::test]
async fn retained_preference_is_materialized_once_and_explicit_default_syncs() {
    for explicit in [None, std::num::NonZeroU8::new(1)] {
        let fixture = Fixture::new();
        let mut state = state::ScopeState::empty(&fixture.scope).unwrap();
        state.retained_documents.push(LogicalDocument {
            id: "translationShiftTapCount".into(),
            kind: DocumentKind::Preferences,
            schema_version: 1,
            value: json!(2),
        });
        fixture
            .extensions
            .write_scope(fixture.scope.clone(), state.secret_json().unwrap())
            .await
            .unwrap();
        if let Some(value) = explicit {
            fixture
                .repo
                .preferences
                .update(|p| p.translation_shift_tap_count = Some(value))
                .unwrap();
        }
        let captured = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        assert_eq!(
            fixture.repo.preferences.get().translation_shift_tap_count(),
            if explicit.is_some() { 1 } else { 2 }
        );
        assert!(state::ScopeState::decode(
            &fixture.scope,
            fixture
                .extensions
                .read_scope(fixture.scope.clone())
                .await
                .unwrap()
        )
        .unwrap()
        .retained_documents
        .is_empty());
        let fixture = fixture.reopened();
        fixture
            .repo
            .preferences
            .update(|p| p.translation_shift_tap_count = std::num::NonZeroU8::new(1))
            .unwrap();
        let reset = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        assert_eq!(
            doc_value(
                reset.documents.documents(),
                DocumentKind::Preferences,
                "translationShiftTapCount"
            ),
            &json!(1)
        );
        assert!(reset.generation >= captured.generation);
        let receiver = Fixture::new();
        let before = receiver
            .store
            .export_scope(receiver.scope.clone())
            .await
            .unwrap();
        let mut context = receiver.context(&before);
        context.observed_revision = reset.documents.observed_revision();
        receiver
            .store
            .restore_scope(reset.documents, context)
            .await
            .unwrap();
        assert_eq!(
            receiver
                .repo
                .preferences
                .get()
                .translation_shift_tap_count(),
            1
        );
        assert_eq!(
            receiver.repo.preferences.get().translation_shift_tap_count,
            std::num::NonZeroU8::new(1)
        );
    }
}

#[tokio::test]
async fn projected_builtin_and_edited_alias_restore_without_losing_selection() {
    for alias in [false, true] {
        for edited in [false, true] {
            let fixture = Fixture::new();
            let before = fixture
                .store
                .export_scope(fixture.scope.clone())
                .await
                .unwrap();
            let mut set = before.documents.documents().clone();
            let mut record: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/intent-reconstruction-v1-projection.json"
            ))
            .unwrap();
            let id = if alias {
                "imported.saved-selection"
            } else {
                "openless.intent-reconstruction"
            };
            record["pack"]["id"] = json!(id);
            if edited {
                record["pack"]["prompt"] = json!("User edited on the released client");
            }
            set_doc(&mut set, DocumentKind::StylePacks, id, record);
            set_doc(
                &mut set,
                DocumentKind::Preferences,
                "activeStylePackId",
                json!(id),
            );
            fixture
                .store
                .restore_scope(validate(set), fixture.context(&before))
                .await
                .unwrap();
            let fixture = fixture.reopened();
            let pack = fixture
                .repo
                .style_packs
                .list()
                .unwrap()
                .into_iter()
                .find(|p| p.id == id)
                .unwrap();
            assert_eq!(fixture.repo.preferences.get().active_style_pack_id, id);
            assert_eq!(
                pack.kind,
                if edited {
                    crate::style_packs::StylePackKind::Imported
                } else {
                    crate::style_packs::StylePackKind::Builtin
                }
            );
            if edited {
                assert_eq!(pack.prompt, "User edited on the released client");
            }
            let exported = fixture
                .store
                .export_scope(fixture.scope.clone())
                .await
                .unwrap();
            assert_eq!(
                doc_value(exported.documents.documents(), DocumentKind::StylePacks, id)["pack"]
                    ["kind"],
                "imported"
            );
            let resolved = fixture
                .repo
                .style_packs
                .get_or_default_active(&fixture.repo.preferences.get().active_style_pack_id)
                .unwrap();
            assert_eq!(resolved.id, id);
        }
    }
}

// The runner executes these phases in separate processes with the real released
// implementation in between. Ordinary unit tests never read a user's snapshots.
#[tokio::test]
async fn v1_release_exchange_current() {
    let Ok(dir) = std::env::var("OPENLESS_V1_COMPAT_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let mut fixture = fixture_for_device("new-device");
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    if std::env::var("OPENLESS_V1_COMPAT_PHASE").unwrap() == "create" {
        let mut set = before.documents.documents().clone();
        set_doc(
            &mut set,
            DocumentKind::Preferences,
            "translationShiftTapCount",
            json!(2),
        );
        set_doc(
            &mut set,
            DocumentKind::Preferences,
            "futureAppearance",
            json!({"nested":[2, "opaque"]}),
        );
        set.tombstones.push(Tombstone {
            id: "futureDeletedSetting".into(),
            kind: DocumentKind::Preferences,
            deleted_at: "2026-11-23T00:00:00Z".into(),
            base_revision: Revision::new(0),
        });
        let alias = std::env::var("OPENLESS_V1_COMPAT_ALIAS").unwrap() == "1";
        let id = if alias {
            "imported.saved-selection"
        } else {
            "openless.intent-reconstruction"
        };
        let mut record: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/intent-reconstruction-v1-projection.json"
        ))
        .unwrap();
        record["pack"]["id"] = json!(id);
        set_doc(&mut set, DocumentKind::StylePacks, id, record);
        set_doc(
            &mut set,
            DocumentKind::Preferences,
            "activeStylePackId",
            json!(id),
        );
        fixture
            .store
            .restore_scope(validate(set), fixture.context(&before))
            .await
            .unwrap();
        let exported = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        let mut template = template();
        template.vault_id = UuidV4::parse(&fixture.scope.vault_id).unwrap();
        template.key_id = UuidV4::parse(&fixture.scope.key_id).unwrap();
        template.base_revision = Revision::new(0);
        template.kind = UploadKind::Create;
        let upload = crypto::encrypt_snapshot(
            exported.documents.documents(),
            &encrypt_context(&template),
            &key(),
        )
        .unwrap();
        std::fs::write(
            dir.join("new-upload.json"),
            serde_json::to_vec(&upload).unwrap(),
        )
        .unwrap();
    } else {
        let upload: SnapshotUpload =
            serde_json::from_slice(&std::fs::read(dir.join("download.json")).unwrap()).unwrap();
        fixture.scope.vault_id = upload.vault_id.as_str().into();
        fixture.scope.key_id = upload.key_id.as_str().into();
        let docs =
            crypto::decrypt_snapshot(&upload, &metadata(&upload), &upload.owner_github_id, &key())
                .unwrap();
        let docs = validate_sync_documents(docs, upload.revision).unwrap();
        assert_eq!(
            doc_value(
                docs.documents(),
                DocumentKind::Preferences,
                "futureAppearance"
            ),
            &json!({"nested":[2, "opaque"]})
        );
        assert!(docs
            .documents()
            .tombstones
            .iter()
            .any(|mark| mark.id == "futureDeletedSetting"));
        let mut context = fixture.context(&before);
        context.observed_revision = upload.revision;
        fixture.store.restore_scope(docs, context).await.unwrap();
        let fixture = fixture.reopened();
        let prefs = fixture.repo.preferences.get();
        assert_eq!(prefs.translation_shift_tap_count(), 2);
        assert_eq!(serde_json::to_value(&prefs).unwrap()["themeMode"], "dark");
        let pack = fixture
            .repo
            .style_packs
            .list()
            .unwrap()
            .into_iter()
            .find(|p| p.id == prefs.active_style_pack_id)
            .unwrap();
        let edited = std::env::var("OPENLESS_V1_COMPAT_EDIT").unwrap() == "1";
        if edited {
            assert_eq!(pack.prompt, "Edited in actual released source");
            assert_eq!(
                pack.selection_prompt,
                "Edited selection in actual released source"
            );
            assert_eq!(
                pack.voice_edit_prompt,
                "Edited voice intent in actual released source"
            );
            assert_eq!(pack.kind, crate::style_packs::StylePackKind::Imported);
        } else {
            assert_eq!(pack.kind, crate::style_packs::StylePackKind::Builtin);
        }
        let active = fixture
            .repo
            .style_packs
            .get_or_default_active(&prefs.active_style_pack_id)
            .unwrap();
        assert_eq!(active.id, prefs.active_style_pack_id);
        // Upgrade the actual old release's durable native files and extension
        // state, rather than manufacturing an approximation with today's codec.
        let mut upgraded = fixture_for_device("old-device");
        upgraded.scope =
            serde_json::from_slice(&std::fs::read(dir.join("old-scope.json")).unwrap()).unwrap();
        for (saved, native) in [
            ("old-preferences.json", "preferences.json"),
            ("old-styles.json", "style-packs.json"),
        ] {
            std::fs::copy(dir.join(saved), upgraded._temp.0.join(native)).unwrap();
        }
        let old_state: Value =
            serde_json::from_slice(&std::fs::read(dir.join("old-scope-state.json")).unwrap())
                .unwrap();
        upgraded
            .extensions
            .write_scope(upgraded.scope.clone(), SecretJson::new(old_state))
            .await
            .unwrap();
        let upgraded = upgraded.reopened();
        assert_eq!(
            upgraded.repo.preferences.get().translation_shift_tap_count,
            None
        );
        let promoted = upgraded
            .store
            .export_scope(upgraded.scope.clone())
            .await
            .unwrap();
        assert!(promoted.materialized_preferences);
        assert_eq!(
            upgraded
                .repo
                .preferences
                .get()
                .translation_shift_tap_count(),
            2
        );
        let again = upgraded
            .store
            .export_scope(upgraded.scope.clone())
            .await
            .unwrap();
        assert!(!again.materialized_preferences);
        assert_eq!(promoted.generation, again.generation);
        upgraded
            .repo
            .preferences
            .update(|p| p.translation_shift_tap_count = std::num::NonZeroU8::new(1))
            .unwrap();
        let reset = upgraded
            .store
            .export_scope(upgraded.scope.clone())
            .await
            .unwrap();
        let receiver = Fixture::new();
        let initial = receiver
            .store
            .export_scope(receiver.scope.clone())
            .await
            .unwrap();
        let mut context = receiver.context(&initial);
        context.observed_revision = reset.documents.observed_revision();
        receiver
            .store
            .restore_scope(reset.documents, context)
            .await
            .unwrap();
        assert_eq!(
            receiver
                .repo
                .preferences
                .get()
                .translation_shift_tap_count(),
            1
        );
    }
}

fn template() -> SnapshotUpload {
    let v: Value = serde_json::from_str(include_str!(
        "../../cloud_sync_e2ee_protocol/fixtures/v1.json"
    ))
    .unwrap();
    let mut value = v["vectors"][0]["snapshot"].clone();
    value["ownerGithubId"] = json!("11");
    serde_json::from_value(value).unwrap()
}
fn key() -> DerivedKey {
    DerivedKey::from_secret_bytes(Zeroizing::new([37; 32]))
}
fn encrypt_context(s: &SnapshotUpload) -> crypto::EncryptContext {
    crypto::EncryptContext {
        owner_github_id: s.owner_github_id.clone(),
        vault_id: s.vault_id.clone(),
        key_id: s.key_id.clone(),
        base_revision: s.base_revision,
        operation_id: UuidV4::random().unwrap(),
        kind: s.kind,
        kdf: s.kdf.clone(),
    }
}
fn metadata(s: &SnapshotUpload) -> VaultMetadata {
    VaultMetadata {
        protocol_version: 1,
        state: VaultState::Active,
        owner_github_id: s.owner_github_id.clone(),
        revision: s.revision,
        vault_id: Some(s.vault_id.clone()),
        key_id: Some(s.key_id.clone()),
        updated_at: Some("2026-11-23T00:00:00Z".into()),
        payload_schema_version: Some(1),
        ciphertext_bytes: s.decoded_ciphertext().unwrap().len() as u64,
        ciphertext_sha256: Some(s.ciphertext_sha256.clone()),
        last_operation_id: Some(s.operation_id.clone()),
    }
}
