mod range_identity_regressions {
    use super::*;
    use crate::host_document::{
        OwnedRangeChange, OwnedRangeIdentity, OwnedRangeRevision, RangeObservation,
    };

    fn identity() -> OwnedRangeIdentity {
        OwnedRangeIdentity {
            session_id: SessionId::from_uuid(uuid::Uuid::from_u128(1)),
            owner_id: uuid::Uuid::from_u128(2),
            insertion_id: uuid::Uuid::from_u128(3),
        }
    }

    fn sink(
        backend: &OpenLessBackend,
        text: &str,
        identity: OwnedRangeIdentity,
        profile: Arc<crate::expression_observation::ExpressionProfileStore>,
    ) -> CombinedEditObservationSink {
        let generation = backend.edit_observation_generation.load(Ordering::Acquire);
        CombinedEditObservationSink {
            vocabulary: Some(CoreEditObservationSink {
                session_id: identity.session_id,
                settings: backend.get_preferences().vocabulary_learning_settings,
                expected_generation: generation,
                generation: Arc::clone(&backend.edit_observation_generation),
                typed_text: text.into(),
                pending: Arc::clone(&backend.pending_corrections),
                events: Arc::clone(&backend.events),
                dictionary: Arc::clone(&backend.vocabulary),
            }),
            expression_generation: Some(profile.generation()),
            expression_profile: Some(profile),
            typed_text: text.into(),
            session_id: identity.session_id,
            clock: Arc::new(SystemClock),
            generation: Arc::clone(&backend.edit_observation_generation),
            expected_generation: generation,
            settings_gate: Arc::clone(&backend.settings_write_gate),
            duration: std::time::Duration::from_secs(60),
        }
    }

    fn utf16_slice(text: &str, start: usize, length: usize) -> String {
        let units: Vec<_> = text.encode_utf16().collect();
        String::from_utf16(&units[start..start + length]).unwrap()
    }

    fn rejects_external_shift(coalesced: bool) {
        let (backend, _) = backend();
        let mut preferences = backend.get_preferences();
        preferences.vocabulary_learning_enabled = true;
        preferences.expression_learning_enabled = true;
        backend.set_preferences(preferences).unwrap();
        let directory = TestDataDir::new("range-identity-negative");
        let path = directory.path().join("profile.json");
        let profile = Arc::new(
            crate::expression_observation::ExpressionProfileStore::open(path.clone()).unwrap(),
        );
        let original = "今天先完成这个工作然后再讨论！";
        let boundary = "！".repeat(64);
        let document = format!("{boundary}{original}{boundary}TAIL");
        let total = document.encode_utf16().count();
        let length = original.encode_utf16().count();
        let identity = identity();
        let sink = sink(&backend, original, identity, profile);
        let mut events = backend.subscribe();
        let mut range = RangeObservation::delivered(identity, 64, original.into(), total).unwrap();

        // Only edits OUTSIDE S: insert before P, then delete in far T.
        // The original S never changes; numeric [64,64+L) loses its last "！"
        // and borrows a leading "！". Both 32-character anchors still match.
        let first = format!("！{document}");
        let mut restored = first.clone();
        assert_eq!(restored.pop(), Some('L'));
        assert_eq!(restored.encode_utf16().count(), total);
        let borrowed = utf16_slice(&restored, 64, length);
        assert_eq!(borrowed, format!("！{}", original.trim_end_matches('！')));
        assert_eq!(utf16_slice(&restored, 32, 32), "！".repeat(32));
        assert_eq!(utf16_slice(&restored, 64 + length, 32), "！".repeat(32));
        let poisoned = crate::host_document::minimal_edit(original, &borrowed).unwrap();
        assert!(
            crate::expression_observation::signal_from_observed_edit(original, &poisoned).is_ok(),
            "this counterexample would be accepted by the expression classifier"
        );
        let notifications = if coalesced {
            vec![restored]
        } else {
            vec![first, restored]
        };
        let mut text_reads = 0;
        let mut sink_calls = 0;
        for document in notifications {
            // Exactly the admission used by the native AX entry: no real
            // edit-span/revision evidence, even if total/caret/anchors match.
            assert!(!range.sample_revision(
                None,
                |start, count| {
                    text_reads += 1;
                    Some(utf16_slice(&document, start, count))
                },
                |edit| {
                    sink_calls += 1;
                    sink.publish(edit)
                },
            ));
        }
        assert_eq!(text_reads, 0, "no front/back body can enter a sample");
        assert_eq!(
            sink_calls, 0,
            "the actual combined sink must not be invoked"
        );
        assert!(backend.pending_corrections().is_empty());
        assert!(backend.list_vocabulary().unwrap().is_empty());
        assert!(
            events.try_recv().is_err(),
            "no vocabulary suggestion/change event"
        );
        assert!(!path.exists(), "no expression evidence is persisted");
    }

    #[test]
    fn external_front_insert_far_delete_split_notifications_never_read_or_reach_either_sink() {
        rejects_external_shift(false);
    }

    #[test]
    fn external_front_insert_far_delete_single_equal_length_batch_never_read_or_reach_either_sink()
    {
        rejects_external_shift(true);
    }

    #[test]
    fn synthetic_trusted_edit_span_chain_reaches_real_sinks_only_with_explicit_word_confirmation() {
        // Contract fixture only: no native AX/UIA or real editor callback is
        // represented by these hand-authored owner/revision records.
        let (backend, _) = backend();
        let mut preferences = backend.get_preferences();
        preferences.vocabulary_learning_enabled = true;
        preferences.expression_learning_enabled = true;
        backend.set_preferences(preferences).unwrap();
        let directory = TestDataDir::new("range-identity-positive");
        let path = directory.path().join("profile.json");
        let profile = Arc::new(
            crate::expression_observation::ExpressionProfileStore::open(path.clone()).unwrap(),
        );
        let identity = identity();
        let original = "请使用 codx 完成任务";
        let edited = "请使用 codex 完成任务";
        let original_length = original.encode_utf16().count();
        let total = 10 + original_length + 4;
        let sink = sink(&backend, original, identity, Arc::clone(&profile));
        let mut range = RangeObservation::delivered(identity, 10, original.into(), total).unwrap();
        let mut read_bounds = Vec::new();
        // A trusted owner reports delete, type, and another local correction,
        // each in the SAME successful insertion; no full-field diff is used.
        for (revision, removed, inserted, before, after, text) in [
            (1, 4, 0, total, total - 4, "请使用  完成任务"),
            (2, 0, 5, total - 4, total + 1, edited),
            (3, 1, 1, total + 1, total + 1, "请使用 Codex 完成任务"),
        ] {
            assert!(range.record_local_change(OwnedRangeChange {
                identity,
                revision,
                start: 14,
                removed_utf16: removed,
                inserted_utf16: inserted,
                total_before: before,
                total_after: after,
            }));
            assert!(range.sample_revision(
                Some(OwnedRangeRevision {
                    identity,
                    revision,
                    total: after,
                    selection: (14, 0),
                }),
                |start, length| {
                    read_bounds.push((start, length));
                    assert_eq!(length, text.encode_utf16().count());
                    Some(text.into())
                },
                |edit| sink.publish(edit),
            ));
        }
        assert!(read_bounds.iter().all(|(start, _)| *start == 10));
        assert!(backend.list_vocabulary().unwrap().is_empty());
        let pending = backend.pending_corrections();
        assert!(pending.iter().any(|entry| entry.replacement == "codex"));
        let confirmed = pending
            .iter()
            .find(|entry| entry.replacement == "codex")
            .unwrap();
        backend.accept_pending_correction(&confirmed.id).unwrap();
        assert!(backend
            .asr_vocabulary_phrases()
            .contains(&"codex".to_owned()));
        assert!(backend
            .enabled_vocabulary_phrases()
            .contains(&"codex".to_owned()));
        assert!(
            !path.exists(),
            "lexical correction is not expression evidence"
        );

        let original = "今天先完成这个工作然后再讨论！";
        let edited = original.trim_end_matches('！');
        let original_length = original.encode_utf16().count();
        let identity = OwnedRangeIdentity {
            session_id: SessionId::from_uuid(uuid::Uuid::from_u128(4)),
            insertion_id: uuid::Uuid::from_u128(5),
            ..identity
        };
        let sink = self::sink(&backend, original, identity, profile);
        let mut range =
            RangeObservation::delivered(identity, 10, original.into(), 10 + original_length)
                .unwrap();
        assert!(range.record_local_change(OwnedRangeChange {
            identity,
            revision: 1,
            start: 10 + original_length - 1,
            removed_utf16: 1,
            inserted_utf16: 0,
            total_before: 10 + original_length,
            total_after: 9 + original_length,
        }));
        assert!(range.sample_revision(
            Some(OwnedRangeRevision {
                identity,
                revision: 1,
                total: 9 + original_length,
                selection: (9 + original_length, 0),
            }),
            |start, length| {
                assert_eq!((start, length), (10, original_length - 1));
                Some(edited.into())
            },
            |edit| sink.publish(edit),
        ));
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(
            !saved.contains(original),
            "only expression counts may persist"
        );
        assert_eq!(backend.list_vocabulary().unwrap().len(), 1);
    }
}
