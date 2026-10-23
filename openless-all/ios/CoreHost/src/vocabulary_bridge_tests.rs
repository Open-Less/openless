use super::*;
use openless_core::{BackendConfig, BackendDependencies};

fn backend(path: &Path) -> OpenLessBackend {
    OpenLessBackend::new(
        BackendConfig {
            data_dir: path.into(),
            ..Default::default()
        },
        BackendDependencies::unsupported(),
    )
    .unwrap()
}
fn snapshot(b: &OpenLessBackend, p: &Path) -> Value {
    execute(b, p, json!({"command":"snapshot"})).unwrap()
}
fn confirm(
    b: &OpenLessBackend,
    p: &Path,
    entry: &DictionaryEntry,
    generation: u64,
) -> Result<Value, BackendError> {
    execute(
        b,
        p,
        json!({"command":"confirmEntry","id":entry.id,"phrase":entry.phrase,"expectedGeneration":generation}),
    )
}
#[test]
fn legacy_rows_require_explicit_confirmation_and_reopen_preserves_metadata_not_content() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    let manual = b.add_vocabulary("量子桥".into(), None).unwrap();
    b.add_learned_vocabulary("旧词条".into()).unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
    confirm(&b, dir.path(), &manual, 0).unwrap();
    let state = snapshot(&b, dir.path());
    assert_eq!(state["entries"][0]["id"], manual.id);
    assert_eq!(state["scope"], "localOffline");
    assert!(confirm(&b, dir.path(), &manual, 0).is_err());
    drop(b);
    let b = backend(dir.path());
    assert_eq!(snapshot(&b, dir.path()), state);
    assert_eq!(b.list_vocabulary().unwrap().len(), 2);
}
#[test]
fn renamed_removed_disabled_and_reused_identity_cannot_export() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    let entry = b.add_vocabulary("原词".into(), None).unwrap();
    confirm(&b, dir.path(), &entry, 0).unwrap();
    // Fingerprint mismatch is fail closed even if a caller missed a hook.
    b.update_vocabulary_phrase(&entry.id, "新词".into())
        .unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
    b.update_vocabulary_phrase(&entry.id, entry.phrase.clone())
        .unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
    let generation = snapshot(&b, dir.path())["generation"].as_u64().unwrap();
    confirm(&b, dir.path(), &entry, generation).unwrap();
    invalidate_entry(dir.path(), &entry.id).unwrap();
    b.set_vocabulary_enabled(&entry.id, false).unwrap();
    b.set_vocabulary_enabled(&entry.id, true).unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
    let mut reused = entry.clone();
    reused.created_at = "different-creation".into();
    assert_ne!(fingerprint(&entry), fingerprint(&reused));
    b.remove_vocabulary(&entry.id).unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
}
#[test]
fn before_mutation_hook_invalidates_even_rename_back_without_intermediate_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    let entry = b.add_vocabulary("原词".into(), None).unwrap();
    confirm(&b, dir.path(), &entry, 0).unwrap();
    invalidate_entry(dir.path(), &entry.id).unwrap();
    b.update_vocabulary_phrase(&entry.id, "中间词".into())
        .unwrap();
    invalidate_entry(dir.path(), &entry.id).unwrap();
    b.update_vocabulary_phrase(&entry.id, entry.phrase.clone())
        .unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
}
#[test]
fn proposals_are_real_core_corrections_deduped_and_acceptance_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    b.add_vocabulary("已有词".into(), None).unwrap();
    b.queue_pending_correction("已又词".into(), "已有词".into())
        .unwrap();
    let candidate = b
        .queue_pending_correction("量子乔".into(), "量子桥".into())
        .unwrap()
        .unwrap();
    let proposals = execute(&b, dir.path(), json!({"command":"proposals"})).unwrap();
    assert_eq!(proposals["kind"], "correction");
    assert_eq!(proposals["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(b.list_vocabulary().unwrap().len(), 1);
    assert!(execute(
        &b,
        dir.path(),
        json!({"command":"accept","id":"invented","expectedGeneration":0})
    )
    .is_err());
    assert!(execute(
        &b,
        dir.path(),
        json!({"command":"accept","id":candidate.id,"expectedGeneration":42})
    )
    .is_err());
    execute(
        &b,
        dir.path(),
        json!({"command":"accept","id":candidate.id,"expectedGeneration":0}),
    )
    .unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"][0]["phrase"], "量子桥");
    assert_eq!(b.list_vocabulary().unwrap().len(), 2);
    assert!(execute(
        &b,
        dir.path(),
        json!({"command":"accept","id":candidate.id,"expectedGeneration":1})
    )
    .is_err());
    let rejected = b
        .queue_pending_correction("新乔".into(), "新桥".into())
        .unwrap()
        .unwrap();
    execute(&b, dir.path(), json!({"command":"reject","id":rejected.id})).unwrap();
    execute(&b, dir.path(), json!({"command":"dismiss"})).unwrap();
    assert!(b.pending_corrections().is_empty());
    assert_eq!(b.list_vocabulary().unwrap().len(), 2);
}
#[test]
fn interrupted_or_failed_metadata_writes_never_infer_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    // A crash after Core acceptance/manual add but before metadata rename.
    let entry = b
        .add_learned_vocabulary("崩溃后词条".into())
        .unwrap()
        .unwrap();
    fs::write(
        dir.path().join(format!(".{FILE}.abandoned.tmp")),
        b"partial",
    )
    .unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
    // Existing target directory forces atomic rename failure, without relying
    // on permissions (which privileged test processes could bypass).
    fs::create_dir(dir.path().join(FILE)).unwrap();
    assert!(confirm(&b, dir.path(), &entry, 0).is_err());
    fs::remove_dir(dir.path().join(FILE)).unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
    fs::write(dir.path().join(FILE), b"partial").unwrap();
    assert!(execute(&b, dir.path(), json!({"command":"snapshot"})).is_err());
}
#[test]
fn account_scope_substitution_and_oversized_metadata_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    let entry = b.add_vocabulary("私有词条".into(), None).unwrap();
    confirm(&b, dir.path(), &entry, 0).unwrap();
    let mut ledger = Ledger::load(dir.path()).unwrap();
    ledger.scope = "github:123".into();
    ledger.save(dir.path()).unwrap();
    assert!(execute(&b, dir.path(), json!({"command":"snapshot"})).is_err());
    assert!(execute(
        &b,
        dir.path(),
        json!({"command":"snapshot","scope":"github:456"})
    )
    .is_err());
    fs::write(dir.path().join(FILE), vec![b' '; MAX_BYTES as usize + 1]).unwrap();
    assert!(execute(&b, dir.path(), json!({"command":"snapshot"})).is_err());
}

#[test]
fn reused_core_id_with_different_creation_invalidates_persisted_consent() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    let entry = b.add_vocabulary("同名词".into(), None).unwrap();
    confirm_explicit_edit(&b, dir.path(), &entry.id, &entry.phrase).unwrap();
    let mut rows = b.list_vocabulary().unwrap();
    rows[0].created_at = "different-creation".into();
    fs::write(
        dir.path().join("dictionary.json"),
        serde_json::to_vec(&rows).unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
    assert_eq!(Ledger::load(dir.path()).unwrap().records.len(), 0);
}
#[test]
fn confirmation_capacity_is_bounded_and_excess_core_rows_stay_unconfirmed() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    for index in 0..MAX_ENTRIES {
        let entry = b
            .add_vocabulary(format!("fixture_{index:03}"), None)
            .unwrap();
        confirm_explicit_edit(&b, dir.path(), &entry.id, &entry.phrase).unwrap();
    }
    let extra = b.add_vocabulary("unconfirmed_extra".into(), None).unwrap();
    assert!(confirm_explicit_edit(&b, dir.path(), &extra.id, &extra.phrase).is_err());
    let state = snapshot(&b, dir.path());
    assert_eq!(state["entries"].as_array().unwrap().len(), MAX_ENTRIES);
    assert_eq!(b.list_vocabulary().unwrap().len(), MAX_ENTRIES + 1);
    assert!(fs::metadata(dir.path().join(FILE)).unwrap().len() <= MAX_BYTES);
}

#[test]
fn explicit_edit_hook_preserves_core_long_or_disabled_rows_without_claiming_export() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    let long = b.add_vocabulary("长".repeat(65), None).unwrap();
    assert!(!confirm_explicit_edit(&b, dir.path(), &long.id, &long.phrase).unwrap());
    let disabled = b.add_vocabulary("停用词".into(), None).unwrap();
    b.set_vocabulary_enabled(&disabled.id, false).unwrap();
    assert!(!confirm_explicit_edit(&b, dir.path(), &disabled.id, &disabled.phrase).unwrap());
    assert_eq!(snapshot(&b, dir.path())["entries"], json!([]));
    assert_eq!(b.list_vocabulary().unwrap().len(), 2);
}
