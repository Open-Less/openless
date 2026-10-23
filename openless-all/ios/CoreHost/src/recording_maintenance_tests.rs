use super::*;
use crate::control_tests::open_at;
use crate::recordings::{asset_path, header, WavArchive, HEADER_LEN, MAX_BYTES};
use std::io::Write;

fn row(id: &str, code: Option<&str>) -> DictationSession {
    serde_json::from_value(
        json!({"id":id,"createdAt":"2099-01-01T00:00:00Z", "rawTranscript":"",
        "finalText":"", "mode":"raw", "insertStatus":"notRequested", "errorCode":code,
        "hasAudioRecording":true}),
    )
    .unwrap()
}
fn open(dir: &Path) -> (u64, Arc<Host>) {
    let (handle, _) = open_at(dir, false, Arc::new(credentials::MemoryVault::default()));
    (handle, host(handle).unwrap())
}
fn pcm(directory: &Path, id: &str) -> PathBuf {
    let archive = WavArchive::create(
        directory,
        SessionId::from_uuid(Uuid::parse_str(id).unwrap()),
    )
    .unwrap();
    archive.append(&[1, 2, 3, 4]);
    // Dropping without finish models a process exit, not Core cancellation.
    drop(archive);
    directory.join(format!("{id}.wav"))
}
fn append(owner: &Host, entry: DictationSession) {
    owner.backend.append_history(entry, 0, None).unwrap();
}
fn management<T>(owner: &Host, f: impl FnOnce() -> T) -> T {
    assert!(!owner.control_busy.swap(true, Ordering::AcqRel));
    let result = f();
    owner.control_busy.store(false, Ordering::Release);
    result
}

#[test]
fn startup_repairs_stale_lengths_and_recovers_real_draft_without_transcription() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let id = Uuid::new_v4().to_string();
    append(&owner, row(&id, Some("recording")));
    let path = pcm(&owner.recordings_directory, &id);
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.write_all(&header(0)).unwrap(); // PCM committed before length flush
    drop(file);
    ol_ios_destroy(handle);
    let (handle, owner) = open(dir.path());
    let entry = owner.history_entry(&id).unwrap();
    assert_eq!(entry.error_code.as_deref(), Some("interrupted"));
    assert_eq!(entry.has_audio_recording, Some(true));
    assert!(entry.raw_transcript.is_empty() && entry.final_text.is_empty());
    assert_eq!(entry.duration_ms, Some(0)); // four real PCM bytes, not guessed seconds
    let library = owner.library_snapshot().unwrap();
    assert_eq!(library["history"][0]["hasAudioRecording"], true);
    assert_eq!(library["history"][0]["errorCode"], "interrupted");
    let asset = owner
        .executor
        .block_on(owner.control(ControlAction::AudioAsset { id: id.clone() }))
        .unwrap();
    assert_eq!(asset["path"].as_str().unwrap(), path.to_str().unwrap());
    assert_eq!(
        std::fs::read(asset_path(&owner.recordings_directory, &id).unwrap()).unwrap()[44..],
        [1, 2, 3, 4]
    );
    let before = std::fs::read(&path).unwrap();
    let report = owner.maintain_recordings_if_idle().unwrap().unwrap();
    assert!(report.recovered.is_empty() && report.removed.is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    ol_ios_destroy(handle);
}

#[test]
fn crash_before_first_chunk_or_header_never_claims_playable_audio() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let missing = Uuid::new_v4().to_string();
    let empty = Uuid::new_v4().to_string();
    let torn = Uuid::new_v4().to_string();
    for id in [&missing, &empty, &torn] {
        append(&owner, row(id, Some("recording")));
    }
    let archive = WavArchive::create(
        &owner.recordings_directory,
        SessionId::from_uuid(Uuid::parse_str(&empty).unwrap()),
    )
    .unwrap();
    drop(archive);
    let torn_path = owner.recordings_directory.join(format!("{torn}.wav"));
    std::fs::write(&torn_path, b"RI").unwrap();
    let report = owner.maintain_recordings_if_idle().unwrap().unwrap();
    assert!(report.rejected.contains(&torn));
    for id in [&missing, &empty, &torn] {
        let entry = owner.history_entry(id).unwrap();
        assert_eq!(entry.error_code.as_deref(), Some("interrupted"));
        assert_eq!(entry.has_audio_recording, Some(false));
        assert_eq!(entry.duration_ms, None);
        assert!(asset_path(&owner.recordings_directory, id).is_err());
    }
    assert!(!owner
        .recordings_directory
        .join(format!("{empty}.wav"))
        .exists());
    assert_eq!(std::fs::read(torn_path).unwrap(), b"RI");
    ol_ios_destroy(handle);
}

#[test]
fn orphan_capture_and_legacy_wav_are_durably_quarantined_not_invented_or_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let captured = Uuid::new_v4().to_string();
    let legacy = Uuid::new_v4().to_string();
    let captured_path = pcm(&owner.recordings_directory, &captured);
    let legacy_path = owner.recordings_directory.join(format!("{legacy}.wav"));
    std::fs::copy(&captured_path, &legacy_path).unwrap();
    for _ in 0..2 {
        let report = owner.maintain_recordings_if_idle().unwrap().unwrap();
        assert_eq!(report.quarantined.len(), 2);
        for id in [&captured, &legacy] {
            assert!(report.quarantined.contains(id));
            assert!(
                read_journal(&owner.recordings_directory, id).unwrap()
                    == Some(Journal::Quarantined)
            );
            assert!(asset_path(&owner.recordings_directory, id).is_ok());
        }
        assert!(owner.backend.list_history().unwrap().is_empty());
    }
    ol_ios_destroy(handle);
    let (handle, owner) = open(dir.path());
    assert_eq!(
        owner
            .maintain_recordings_if_idle()
            .unwrap()
            .unwrap()
            .quarantined
            .len(),
        2
    );
    assert!(captured_path.exists() && legacy_path.exists());
    ol_ios_destroy(handle);
}

#[test]
fn only_known_transcription_success_is_ephemeral_and_quick_notes_survive() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let mut retained = Vec::new();
    let mut removed = Vec::new();
    for (code, text, note) in [
        (None, "recognized", false),
        (Some("polishFailed"), "recognized", false),
        (Some("insertFailed"), "recognized", false),
        (None, "", false),
        (Some("futureVoiceState"), "future metadata", false),
        (Some("transcribeFailed"), "", false),
        (None, "recognized", true),
    ] {
        let id = Uuid::new_v4().to_string();
        let mut entry = row(&id, code);
        entry.raw_transcript = text.into();
        entry.final_text = text.into();
        if note {
            entry.source = HistorySource::QuickNote;
        }
        append(&owner, entry);
        let path = pcm(&owner.recordings_directory, &id);
        if text == "recognized" && !note {
            removed.push((id, path));
        } else {
            retained.push((id, path));
        }
    }
    owner.maintain_recordings_if_idle().unwrap();
    for (id, path) in removed {
        assert!(!path.exists());
        assert_eq!(
            owner.history_entry(&id).unwrap().has_audio_recording,
            Some(false)
        );
    }
    for (id, path) in retained {
        assert!(path.exists());
        assert_eq!(
            owner.history_entry(&id).unwrap().has_audio_recording,
            Some(true)
        );
    }
    ol_ios_destroy(handle);
}

#[test]
fn idle_gate_never_scans_starting_capture_or_concurrent_management() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let id = Uuid::new_v4().to_string();
    let path = pcm(&owner.recordings_directory, &id);
    *owner.admission.lock().unwrap() = Some(Admission {
        client: Uuid::new_v4(),
        finishing: false,
        cancelled: false,
        execution: Arc::new(voice_intent::Execution::default()),
    });
    assert!(owner.maintain_recordings_if_idle().unwrap().is_none());
    assert!(read_journal(&owner.recordings_directory, &id).unwrap() == Some(Journal::Capturing));
    assert!(owner.dispatch(Command::ClearHistory).is_err());
    owner.control_busy.store(true, Ordering::Release);
    assert!(owner.clear_history_with_recordings().is_err());
    owner.control_busy.store(false, Ordering::Release);
    *owner.admission.lock().unwrap() = None;
    owner.control_busy.store(true, Ordering::Release);
    assert!(owner.maintain_recordings_if_idle().unwrap().is_none());
    assert!(path.exists());
    assert!(read_journal(&owner.recordings_directory, &id).unwrap() == Some(Journal::Capturing));
    owner.control_busy.store(false, Ordering::Release);
    struct Sink;
    impl AudioConsumer for Sink {
        fn consume_pcm_chunk(&self, _: &[u8]) {}
    }
    impl RecordingProgressSink for Sink {
        fn publish_level(&self, _: u64, _: f32) -> Result<(), BackendError> {
            Ok(())
        }
    }
    let session = SessionId::new();
    owner.broker.captures.lock().unwrap().insert(
        session,
        Arc::new(Capture {
            processing: None,
            consumer: Arc::new(Sink),
            progress: Arc::new(Sink),
            bytes: AtomicU64::new(0),
            archive: None,
        }),
    );
    assert!(owner.maintain_recordings_if_idle().unwrap().is_none());
    assert!(read_journal(&owner.recordings_directory, &id).unwrap() == Some(Journal::Capturing));
    owner.broker.captures.lock().unwrap().clear();
    ol_ios_destroy(handle);
}

#[test]
fn clear_preserves_core_retained_notes_and_delete_retry_never_resurrects_history() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let voice = Uuid::new_v4().to_string();
    let note = Uuid::new_v4().to_string();
    append(&owner, row(&voice, Some("transcribeFailed")));
    let mut entry = row(&note, Some("interrupted"));
    entry.source = HistorySource::QuickNote;
    append(&owner, entry);
    let voice_path = pcm(&owner.recordings_directory, &voice);
    let note_path = pcm(&owner.recordings_directory, &note);
    management(&owner, || owner.clear_history_with_recordings()).unwrap();
    assert!(!voice_path.exists() && note_path.exists());
    assert_eq!(owner.backend.list_history().unwrap().len(), 1);
    management(&owner, || owner.clear_history_with_recordings()).unwrap();
    management(&owner, || owner.delete_history_with_recordings(&note)).unwrap();
    management(&owner, || owner.delete_history_with_recordings(&note)).unwrap();
    assert!(!note_path.exists());
    ol_ios_destroy(handle);
    let (handle, owner) = open(dir.path());
    assert!(owner.backend.list_history().unwrap().is_empty());
    assert!(!voice_path.exists() && !note_path.exists());
    ol_ios_destroy(handle);
}

#[test]
fn crash_after_core_clear_or_partial_unlink_retries_from_durable_links() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let mut paths = Vec::new();
    for _ in 0..2 {
        let id = Uuid::new_v4().to_string();
        append(&owner, row(&id, Some("transcribeFailed")));
        paths.push((id.clone(), pcm(&owner.recordings_directory, &id)));
    }
    before_history_removal(
        &owner.recordings_directory,
        &owner.backend.list_history().unwrap(),
    )
    .unwrap();
    owner.backend.clear_history().unwrap();
    // One durable unlink succeeds; the next cannot be parsed (simulated disk
    // damage after the transaction's first half), so cleanup reports failure.
    recordings::remove(&owner.recordings_directory, &paths[0].0).unwrap();
    let bytes = std::fs::read(&paths[1].1).unwrap();
    std::fs::write(&paths[1].1, b"bad").unwrap();
    assert!(after_history_removal(&owner.backend, &owner.recordings_directory).is_err());
    assert!(
        read_journal(&owner.recordings_directory, &paths[1].0).unwrap() == Some(Journal::Linked)
    );
    assert!(owner.backend.list_history().unwrap().is_empty());
    std::fs::write(&paths[1].1, bytes).unwrap();
    ol_ios_destroy(handle);
    let (handle, owner) = open(dir.path());
    assert!(owner.backend.list_history().unwrap().is_empty());
    assert!(paths.iter().all(|(_, path)| !path.exists()));
    management(&owner, || owner.clear_history_with_recordings()).unwrap();
    assert!(std::fs::read_dir(&owner.recordings_directory)
        .unwrap()
        .next()
        .is_none());
    ol_ios_destroy(handle);
}

#[test]
fn pre_mutation_failure_or_crash_retains_history_and_audio_until_idempotent_retry() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let id = Uuid::new_v4().to_string();
    append(&owner, row(&id, Some("transcribeFailed")));
    let path = pcm(&owner.recordings_directory, &id);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"invalid").unwrap();
    assert!(management(&owner, || owner.delete_history_with_recordings(&id)).is_err());
    assert!(owner.history_entry(&id).is_ok());
    std::fs::write(&path, bytes).unwrap();
    before_history_removal(
        &owner.recordings_directory,
        &owner.backend.list_history().unwrap(),
    )
    .unwrap();
    // Process exits after journalling, before Core mutation (or Core rejects it).
    ol_ios_destroy(handle);
    let (handle, owner) = open(dir.path());
    assert!(owner.history_entry(&id).is_ok() && path.exists());
    management(&owner, || owner.delete_history_with_recordings(&id)).unwrap();
    management(&owner, || owner.delete_history_with_recordings(&id)).unwrap();
    assert!(owner.backend.list_history().unwrap().is_empty() && !path.exists());
    ol_ios_destroy(handle);
}

#[test]
fn core_retention_trim_cleans_linked_expired_audio_but_not_current_failures_or_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let expired = Uuid::new_v4().to_string();
    let retained = Uuid::new_v4().to_string();
    let orphan = Uuid::new_v4().to_string();
    let mut old = row(&expired, Some("transcribeFailed"));
    old.created_at = "2000-01-01T00:00:00Z".into();
    append(&owner, old);
    append(&owner, row(&retained, Some("transcribeFailed")));
    let expired_path = pcm(&owner.recordings_directory, &expired);
    let retained_path = pcm(&owner.recordings_directory, &retained);
    let orphan_path = pcm(&owner.recordings_directory, &orphan);
    owner.maintain_recordings_if_idle().unwrap();
    owner
        .backend
        .append_history(
            row(&Uuid::new_v4().to_string(), Some("transcribeFailed")),
            7,
            Some(5),
        )
        .unwrap();
    owner.maintain_recordings_if_idle().unwrap();
    assert!(!expired_path.exists() && retained_path.exists() && orphan_path.exists());
    assert!(owner.history_entry(&retained).is_ok());
    ol_ios_destroy(handle);
}

#[test]
fn wav_input_is_bounded_canonical_and_rejects_malformed_codecs_paths_and_lengths() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let id = Uuid::new_v4().to_string();
    let path = pcm(&owner.recordings_directory, &id);
    let valid = std::fs::read(&path).unwrap();
    for bad in [
        "../outside",
        "00000000000000000000000000000000",
        "00000000-0000-0000-0000-00000000000A",
    ] {
        assert!(asset_path(&owner.recordings_directory, bad).is_err());
    }
    for offset in [0, 8, 12, 20, 22, 24, 28, 32, 34, 36] {
        let mut bad = valid.clone();
        bad[offset] ^= 0xff;
        std::fs::write(&path, bad).unwrap();
        assert!(asset_path(&owner.recordings_directory, &id).is_err());
        assert!(owner
            .maintain_recordings_if_idle()
            .unwrap()
            .unwrap()
            .rejected
            .contains(&id));
    }
    let mut overclaimed = valid.clone();
    overclaimed[..44].copy_from_slice(&header(6));
    std::fs::write(&path, overclaimed).unwrap();
    assert!(open_wav(&path, true).is_err()); // never repair a truncated/overclaimed payload
    std::fs::write(&path, &valid).unwrap();
    let file = OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(u64::from(MAX_BYTES) + HEADER_LEN + 2).unwrap();
    assert!(asset_path(&owner.recordings_directory, &id).is_err());
    file.set_len(45).unwrap();
    assert!(asset_path(&owner.recordings_directory, &id).is_err());
    std::fs::write(&path, valid).unwrap();
    ol_ios_destroy(handle);
}

#[test]
#[cfg(unix)]
fn symlinks_hardlinks_and_directory_aliases_are_never_opened_exported_or_removed() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let outside = dir.path().join("model-resource.bin");
    std::fs::write(&outside, [header(4), vec![1, 2, 3, 4]].concat()).unwrap();
    for hard in [false, true] {
        let id = Uuid::new_v4().to_string();
        let path = owner.recordings_directory.join(format!("{id}.wav"));
        if hard {
            std::fs::hard_link(&outside, &path).unwrap();
        } else {
            symlink(&outside, &path).unwrap();
        }
        assert!(asset_path(&owner.recordings_directory, &id).is_err());
        assert!(recordings::remove(&owner.recordings_directory, &id).is_err());
        assert!(owner
            .maintain_recordings_if_idle()
            .unwrap()
            .unwrap()
            .rejected
            .contains(&id));
        assert_eq!(std::fs::read(&outside).unwrap().len(), 48);
        std::fs::remove_file(path).unwrap();
    }
    let alias = dir.path().join("aliased-recordings");
    symlink(&owner.recordings_directory, &alias).unwrap();
    assert!(prepare_directory(&alias).is_err());
    assert!(prepare_directory(&owner.recordings_directory.join("../escape")).is_err());
    assert_eq!(std::fs::read(&outside).unwrap().len(), 48);
    ol_ios_destroy(handle);
}

#[test]
fn stale_atomic_journal_temp_is_idempotent_and_invalid_journal_cannot_authorize_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let id = Uuid::new_v4().to_string();
    let path = pcm(&owner.recordings_directory, &id);
    let journal = owner.recordings_directory.join(format!("{id}.lifecycle"));
    let temp = owner
        .recordings_directory
        .join(format!("{id}.lifecycle.tmp"));
    std::fs::write(&temp, Journal::Linked.bytes()).unwrap();
    owner.maintain_recordings_if_idle().unwrap();
    assert!(!temp.exists() && path.exists());
    std::fs::write(&journal, b"v99 linked\n").unwrap();
    let report = owner.maintain_recordings_if_idle().unwrap().unwrap();
    assert!(report.rejected.contains(&id) && path.exists());
    assert!(owner.backend.list_history().unwrap().is_empty());
    ol_ios_destroy(handle);
}

#[test]
fn maintenance_bounds_directory_scan_before_any_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, owner) = open(dir.path());
    let id = Uuid::new_v4().to_string();
    let path = pcm(&owner.recordings_directory, &id);
    for n in 0..MAX_DIRECTORY_ENTRIES {
        std::fs::write(
            owner.recordings_directory.join(format!("unrelated-{n}")),
            b"",
        )
        .unwrap();
    }
    assert!(owner.maintain_recordings_if_idle().is_err());
    assert!(path.exists());
    assert!(read_journal(&owner.recordings_directory, &id).unwrap() == Some(Journal::Capturing));
    ol_ios_destroy(handle);
}
