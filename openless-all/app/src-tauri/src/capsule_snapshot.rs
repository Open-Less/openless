//! Replay state for the native transcript rail, independent of Tauri window handles.

use openless_core::{
    BackendEvent, BackendEventKind, CapsulePayload, CapsuleState, DictationPhase,
    LessComputerEvent, LessComputerEventKind, SelectionVoicePhase,
};

#[derive(Clone, Debug, Default)]
pub(super) struct CapsuleSnapshotState {
    pub(super) payload: Option<CapsulePayload>,
    pub(super) payload_revision: u64,
    pub(super) pending_payload_revision: Option<u64>,
    // QA and Less Computer claim the transcript only after their existing
    // business ownership checks actually present the matching capsule frame.
    pending_session_id: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) sequence: u64,
    pub(super) revision: u64,
    pub(super) text: String,
}

fn can_commit_capsule_payload(
    current_revision: u64,
    pending_revision: Option<u64>,
    captured_revision: u64,
) -> bool {
    captured_revision == current_revision
        && pending_revision.map_or(true, |pending| pending == current_revision)
}

fn payload_matches_backend_event(payload: &CapsulePayload, event: &BackendEventKind) -> bool {
    match event {
        BackendEventKind::DictationStateChanged(dictation) => {
            let expected = match dictation.phase {
                DictationPhase::Idle => CapsuleState::Idle,
                DictationPhase::Starting | DictationPhase::Recording => CapsuleState::Recording,
                DictationPhase::Transcribing => CapsuleState::Transcribing,
                DictationPhase::Polishing | DictationPhase::Inserting => CapsuleState::Polishing,
                DictationPhase::Completed => CapsuleState::Done,
                DictationPhase::Cancelled => CapsuleState::Cancelled,
                DictationPhase::Failed => CapsuleState::Error,
            };
            payload.state == expected
        }
        BackendEventKind::DictationCompleted(_) => payload.state == CapsuleState::Done,
        BackendEventKind::BackendStopping => payload.state == CapsuleState::Idle,
        BackendEventKind::SelectionVoiceLevel(_) => payload.state == CapsuleState::Recording,
        BackendEventKind::SelectionVoiceStateChanged(selection) => {
            let expected = match selection.phase {
                SelectionVoicePhase::Idle
                | SelectionVoicePhase::Completed
                | SelectionVoicePhase::Cancelled
                | SelectionVoicePhase::Failed => CapsuleState::Idle,
                SelectionVoicePhase::Recording => CapsuleState::Recording,
                SelectionVoicePhase::Processing
                | SelectionVoicePhase::AwaitingIntent
                | SelectionVoicePhase::Preview
                | SelectionVoicePhase::Applying => CapsuleState::Polishing,
            };
            payload.state == expected
        }
        // QA projections do not own the transcript session. They still use
        // the same revision path, but their detailed phase is resolved by
        // the async QA snapshot before presentation.
        BackendEventKind::QaLevel(_) | BackendEventKind::QaState(_) => true,
        _ => false,
    }
}

impl CapsuleSnapshotState {
    pub(super) fn commit_capsule_payload(
        &mut self,
        payload: &CapsulePayload,
        captured_revision: u64,
    ) -> bool {
        let snapshot = self;
        let current_revision = snapshot.revision;
        if !can_commit_capsule_payload(
            current_revision,
            snapshot.pending_payload_revision,
            captured_revision,
        ) {
            return false;
        }
        let mut committed = payload.clone();
        if snapshot.pending_session_id.is_some()
            && committed.session_id != snapshot.pending_session_id
        {
            return false;
        }
        if let Some(session_id) = snapshot
            .pending_session_id
            .as_ref()
            .or(snapshot.session_id.as_ref())
        {
            if committed
                .session_id
                .as_ref()
                .is_some_and(|payload_session| payload_session != session_id)
            {
                return false;
            }
            committed.session_id = Some(session_id.clone());
        }
        if snapshot.session_id != committed.session_id {
            snapshot.text.clear();
            snapshot.session_id = committed.session_id.clone();
        }
        snapshot.payload = Some(committed);
        snapshot.payload_revision = current_revision;
        snapshot.pending_payload_revision = None;
        snapshot.pending_session_id = None;
        true
    }

    pub(super) fn record_backend_event(&mut self, event: &BackendEvent) {
        let snapshot = self;
        if event.sequence <= snapshot.sequence {
            return;
        }
        let event_session = event.session_id.map(|session| session.to_string());
        let pending_session_id = match &event.kind {
            BackendEventKind::QaLevel(_)
            | BackendEventKind::QaState(_)
            | BackendEventKind::LessComputerEvent(LessComputerEvent {
                kind: LessComputerEventKind::VoiceState { .. },
                ..
            }) => event_session.clone(),
            _ => None,
        };
        let payload_projection_pending = matches!(
            &event.kind,
            BackendEventKind::DictationStateChanged(_)
                | BackendEventKind::TranscriptDelta(_)
                | BackendEventKind::DictationCompleted(_)
                | BackendEventKind::QaLevel(_)
                | BackendEventKind::QaState(_)
                | BackendEventKind::SelectionVoiceLevel(_)
                | BackendEventKind::SelectionVoiceStateChanged(_)
                | BackendEventKind::BackendStopping
                | BackendEventKind::LessComputerEvent(LessComputerEvent {
                    kind: LessComputerEventKind::VoiceState { .. },
                    ..
                })
        );
        match &event.kind {
            BackendEventKind::DictationStateChanged(dictation)
                if dictation.phase == DictationPhase::Starting =>
            {
                let session_id = dictation
                    .session_id
                    .map(|session| session.to_string())
                    .or(event_session.clone());
                if session_id.is_some() && snapshot.session_id != session_id {
                    snapshot.session_id = session_id;
                    snapshot.text.clear();
                } else if snapshot.session_id.is_none() {
                    snapshot.session_id = session_id;
                }
            }
            BackendEventKind::DictationStateChanged(dictation)
                if dictation.phase == DictationPhase::Recording =>
            {
                let session_id = dictation
                    .session_id
                    .map(|session| session.to_string())
                    .or(event_session.clone());
                if session_id.is_some() && snapshot.session_id != session_id {
                    snapshot.session_id = session_id;
                    snapshot.text.clear();
                } else if snapshot.session_id.is_none() {
                    snapshot.session_id = session_id;
                }
            }
            BackendEventKind::SelectionVoiceStateChanged(selection)
                if selection.phase == SelectionVoicePhase::Recording =>
            {
                let session_id = selection
                    .session_id
                    .map(|session| session.to_string())
                    .or(event_session.clone());
                if session_id.is_some() && snapshot.session_id != session_id {
                    snapshot.session_id = session_id;
                    snapshot.text.clear();
                } else if snapshot.session_id.is_none() {
                    snapshot.session_id = session_id;
                }
            }
            BackendEventKind::SelectionVoiceStateChanged(selection) => {
                let session_id = selection
                    .session_id
                    .map(|session| session.to_string())
                    .or(event_session.clone());
                if session_id.is_some() && snapshot.session_id != session_id {
                    snapshot.session_id = session_id;
                    snapshot.text.clear();
                }
            }
            BackendEventKind::TranscriptDelta(delta)
                if event_session.is_none() || snapshot.session_id == event_session =>
            {
                let offset = usize::try_from(delta.offset).ok();
                if let Some(offset) = offset {
                    let mut chars: Vec<char> = snapshot.text.chars().collect();
                    if offset <= chars.len() {
                        chars.truncate(offset);
                        chars.extend(delta.text.chars());
                        snapshot.text = chars.into_iter().collect();
                        if event_session.is_some() {
                            snapshot.session_id = event_session.clone();
                        }
                    }
                }
            }
            _ => {}
        }
        snapshot.sequence = event.sequence;
        if payload_projection_pending {
            snapshot.revision = event.sequence;
            // Only an actual precommit carrying this exact event revision is
            // coherent. In particular, an old Recording payload must not be
            // promoted when the next Recording event is just a PCM update.
            if snapshot.payload_revision == event.sequence
                && snapshot
                    .payload
                    .as_ref()
                    .is_some_and(|payload| payload_matches_backend_event(payload, &event.kind))
            {
                snapshot.pending_payload_revision = None;
                snapshot.pending_session_id = None;
            } else {
                snapshot.pending_payload_revision = Some(event.sequence);
                snapshot.pending_session_id = pending_session_id;
            }
        } else if snapshot.pending_payload_revision.is_none() {
            // Unrelated events keep the committed payload but advance both
            // watermarks, matching the frontend's global event revision.
            snapshot.revision = event.sequence;
            snapshot.payload_revision = event.sequence;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_core::{
        CapsuleStyle, DictationStateSnapshot, LessComputerEvent, LessComputerEventKind,
        LessComputerVoicePhase, QaRecordingLevel, QaStateEvent, QaStateKind, SessionId,
    };

    fn payload(session_id: SessionId, state: CapsuleState) -> CapsulePayload {
        CapsulePayload {
            session_id: Some(session_id.to_string()),
            state,
            level: 0.0,
            elapsed_ms: 0,
            message: None,
            inserted_chars: None,
            translation: false,
            operating: false,
            warming: false,
            capsule_style: CapsuleStyle::Classic,
            selection_polish: false,
        }
    }

    fn completed_dictation() -> (CapsuleSnapshotState, SessionId) {
        let mut snapshot = CapsuleSnapshotState::default();
        let session_id = SessionId::new();
        for (sequence, phase, state) in [
            (1, DictationPhase::Starting, CapsuleState::Recording),
            (2, DictationPhase::Idle, CapsuleState::Idle),
        ] {
            snapshot.record_backend_event(&BackendEvent {
                sequence,
                session_id: Some(session_id),
                kind: BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                    phase,
                    session_id: (phase != DictationPhase::Idle).then_some(session_id),
                    ..Default::default()
                }),
            });
            assert!(snapshot.commit_capsule_payload(&payload(session_id, state), sequence));
        }
        snapshot.text = "previous transcript".into();
        (snapshot, session_id)
    }

    #[test]
    fn capsule_payload_commit_requires_the_current_pending_revision() {
        assert!(can_commit_capsule_payload(7, None, 7));
        assert!(can_commit_capsule_payload(7, Some(7), 7));
        assert!(!can_commit_capsule_payload(7, Some(6), 7));
        assert!(!can_commit_capsule_payload(7, Some(7), 6));
        assert!(!can_commit_capsule_payload(7, None, 8));
    }

    #[test]
    fn qa_and_less_computer_can_take_over_after_dictation() {
        let next = SessionId::new();
        for kind in [
            BackendEventKind::QaState(QaStateEvent::simple(QaStateKind::Recording)),
            BackendEventKind::QaLevel(QaRecordingLevel {
                session_id: next.to_string(),
                level: 0.2,
            }),
            BackendEventKind::LessComputerEvent(LessComputerEvent {
                seq: None,
                kind: LessComputerEventKind::VoiceState {
                    session_id: next,
                    phase: LessComputerVoicePhase::Recording,
                    level: 0.2,
                    elapsed_ms: 1,
                    mode: Default::default(),
                    transcript: String::new(),
                    outcome: None,
                },
            }),
        ] {
            let (mut snapshot, previous) = completed_dictation();
            snapshot.record_backend_event(&BackendEvent {
                sequence: 3,
                session_id: Some(next),
                kind,
            });
            // Merely observing another domain's event must not claim the rail:
            // forward_legacy_event can still reject a superseded/background owner.
            assert_eq!(snapshot.session_id, Some(previous.to_string()));
            assert_eq!(snapshot.text, "previous transcript");
            let mut anonymous = payload(next, CapsuleState::Idle);
            anonymous.session_id = None;
            assert!(!snapshot.commit_capsule_payload(&anonymous, 3));
            assert!(
                !snapshot.commit_capsule_payload(&payload(previous, CapsuleState::Recording), 3)
            );
            assert!(snapshot.commit_capsule_payload(&payload(next, CapsuleState::Recording), 3));
            assert_eq!(snapshot.session_id, Some(next.to_string()));
            assert!(snapshot.text.is_empty());
            assert_eq!(snapshot.payload_revision, snapshot.revision);
            assert!(snapshot.pending_payload_revision.is_none());
            assert!(
                !snapshot.commit_capsule_payload(&payload(previous, CapsuleState::Recording), 3)
            );
            assert!(!snapshot.commit_capsule_payload(&payload(next, CapsuleState::Idle), 2));
        }
    }

    #[test]
    fn unrelated_events_keep_the_committed_replay_frame_coherent() {
        let mut state = CapsuleSnapshotState {
            sequence: 1,
            revision: 1,
            payload_revision: 1,
            text: "已有原文".into(),
            ..Default::default()
        };
        state.record_backend_event(&BackendEvent {
            sequence: 2,
            session_id: None,
            kind: BackendEventKind::BackendStarted,
        });
        assert_eq!(state.sequence, 2);
        assert_eq!(
            state.revision, state.payload_revision,
            "ready replay must not reject an unrelated event as a torn frame"
        );
        assert_eq!(state.revision, 2);
        assert_eq!(state.text, "已有原文");
    }

    #[test]
    fn unrelated_events_do_not_commit_a_pending_payload() {
        let mut state = CapsuleSnapshotState {
            sequence: 1,
            revision: 1,
            payload_revision: 0,
            pending_payload_revision: Some(1),
            ..Default::default()
        };
        state.record_backend_event(&BackendEvent {
            sequence: 2,
            session_id: None,
            kind: BackendEventKind::BackendStarted,
        });
        assert_eq!(state.pending_payload_revision, Some(1));
        assert_eq!(state.revision, 1);
        assert_eq!(state.payload_revision, 0);
    }
}
