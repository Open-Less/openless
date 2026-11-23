// Injected only as a test module into the immutable release's existing store tests.
// All codec, registry, persistence, merge and restore code is from that Git commit.
use super::*;
use crate::cloud_sync_e2ee_protocol::{crypto, types::*};

#[path = "v1-test-device.rs"]
mod v1_test_device;
use v1_test_device::fixture_for_device;

#[tokio::test]
async fn v1_release_exchange_old() {
    let dir = PathBuf::from(std::env::var("OPENLESS_V1_COMPAT_DIR").unwrap());
    let upload: SnapshotUpload =
        serde_json::from_slice(&std::fs::read(dir.join("download.json")).unwrap()).unwrap();
    let key = DerivedKey::from_secret_bytes(Zeroizing::new([37; 32]));
    let metadata = VaultMetadata {
        protocol_version: 1,
        state: VaultState::Active,
        owner_github_id: upload.owner_github_id.clone(),
        revision: upload.revision,
        vault_id: Some(upload.vault_id.clone()),
        key_id: Some(upload.key_id.clone()),
        updated_at: Some("2026-11-23T00:00:00Z".into()),
        payload_schema_version: Some(1),
        ciphertext_bytes: upload.decoded_ciphertext().unwrap().len() as u64,
        ciphertext_sha256: Some(upload.ciphertext_sha256.clone()),
        last_operation_id: Some(upload.operation_id.clone()),
    };
    let plaintext =
        crypto::decrypt_snapshot(&upload, &metadata, &upload.owner_github_id, &key).unwrap();
    if std::env::var("OPENLESS_V1_COMPAT_REJECT").unwrap() == "1" {
        assert_eq!(
            crate::cloud_sync_e2ee_documents::registry::preference_field("cursorContextEnabled")
                .unwrap()
                .class,
            crate::cloud_sync_e2ee_documents::registry::PreferenceClass::Excluded
        );
        assert_eq!(
            validate_sync_documents(plaintext, upload.revision).unwrap_err(),
            DocumentError::ExcludedField
        );
        return;
    }
    let docs = validate_sync_documents(plaintext, upload.revision).unwrap();
    assert!(
        crate::cloud_sync_e2ee_documents::registry::preference_field("translationShiftTapCount")
            .is_none()
    );
    let mut fixture = fixture_for_device("old-device");
    fixture.scope.vault_id = upload.vault_id.as_str().into();
    fixture.scope.key_id = upload.key_id.as_str().into();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut context = fixture.context(&before);
    context.observed_revision = upload.revision;
    fixture
        .store
        .restore_scope(docs.clone(), context)
        .await
        .unwrap();
    let fixture = fixture.reopened();
    let raw: Value =
        serde_json::from_slice(&std::fs::read(fixture._temp.0.join("preferences.json")).unwrap())
            .unwrap();
    assert!(
        raw.get("translationShiftTapCount").is_none(),
        "old release must not apply unknown data"
    );
    let prefs = fixture.repo.preferences.get();
    let mut pack = fixture
        .repo
        .style_packs
        .get_or_default_active(&prefs.active_style_pack_id)
        .unwrap();
    assert_eq!(pack.id, prefs.active_style_pack_id);
    assert_eq!(pack.kind, crate::style_packs::StylePackKind::Imported);
    assert!(!pack.prompt.is_empty());
    // Actual old style selection is usable, including its dictation prompt slot.
    assert_eq!(
        crate::style_packs::style_pack_prompt(
            &pack,
            crate::style_packs::StylePromptKind::DictationAsr
        ),
        pack.prompt
    );
    if std::env::var("OPENLESS_V1_COMPAT_EDIT").unwrap() == "1" {
        pack.prompt = "Edited in actual released source".into();
        pack.selection_prompt = "Edited selection in actual released source".into();
        pack.voice_edit_prompt = "Edited voice intent in actual released source".into();
        fixture.repo.style_packs.update(pack).unwrap();
    }
    fixture
        .repo
        .preferences
        .update(|p| p.theme_mode = crate::shared_types::ThemeMode::Dark)
        .unwrap();
    let fixture = fixture.reopened();
    let current = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    assert!(current
        .documents
        .documents()
        .documents
        .iter()
        .any(|d| d.kind == DocumentKind::Preferences
            && d.id == "translationShiftTapCount"
            && d.value == json!(2)));
    let merged = diff_sync_documents(Some(&docs), &current.documents, &docs)
        .unwrap()
        .resolve(&[])
        .unwrap();
    let context = crypto::EncryptContext {
        owner_github_id: upload.owner_github_id.clone(),
        vault_id: upload.vault_id.clone(),
        key_id: upload.key_id.clone(),
        base_revision: upload.revision,
        operation_id: UuidV4::random().unwrap(),
        kind: UploadKind::Snapshot,
        kdf: upload.kdf.clone(),
    };
    let outgoing = crypto::encrypt_snapshot(merged.documents(), &context, &key).unwrap();
    std::fs::write(
        dir.join("old-upload.json"),
        serde_json::to_vec(&outgoing).unwrap(),
    )
    .unwrap();
    for (native, saved) in [
        ("preferences.json", "old-preferences.json"),
        ("style-packs.json", "old-styles.json"),
    ] {
        std::fs::copy(fixture._temp.0.join(native), dir.join(saved)).unwrap();
    }
    std::fs::write(
        dir.join("old-scope.json"),
        serde_json::to_vec(&fixture.scope).unwrap(),
    )
    .unwrap();
    let state = fixture
        .extensions
        .read_scope(fixture.scope.clone())
        .await
        .unwrap()
        .unwrap();
    std::fs::write(
        dir.join("old-scope-state.json"),
        serde_json::to_vec(state.expose()).unwrap(),
    )
    .unwrap();

    // Counterexamples: changing an existing strict value/kind does not gain
    // compatibility merely because the server stores opaque ciphertext.
    let mut bad_builtin = docs.documents().clone();
    bad_builtin
        .documents
        .iter_mut()
        .find(|d| d.kind == DocumentKind::StylePacks && d.id == prefs.active_style_pack_id)
        .unwrap()
        .value["pack"]["kind"] = json!("builtin");
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    assert!(fixture
        .store
        .restore_scope(validate(bad_builtin), fixture.context(&before))
        .await
        .is_err());
    let mut unknown_field = docs.documents().clone();
    unknown_field
        .documents
        .iter_mut()
        .find(|d| d.kind == DocumentKind::StylePacks)
        .unwrap()
        .value["pack"]["futureSetting"] = json!(2);
    assert!(validate_sync_documents(unknown_field, upload.revision).is_err());
    let mut unknown_kind = serde_json::to_value(docs.documents()).unwrap();
    unknown_kind["documents"][0]["kind"] = json!("future_settings");
    assert!(DocumentSet::from_json_bytes(&serde_json::to_vec(&unknown_kind).unwrap()).is_err());
}
