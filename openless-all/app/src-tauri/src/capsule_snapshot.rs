//! Replay state for the native transcript rail, independent of Tauri window handles.

use openless_core::{
    BackendEvent, BackendEventKind, CapsulePayload, CapsuleState, DictationPhase,
    SelectionVoicePhase,
};

#[derive(Clone, Debug, Default)]
pub(super) struct CapsuleSnapshotState {
    pub(super) payload: Option<CapsulePayload>,
    pub(super) payload_revision: u64,
    pub(super) pending_payload_revision: Option<u64>,
    pub(super) session_id: Option<String>,
    pub(super) sequence: u64,
    pub(super) revision: u64,
    pub(super) text: String,
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
    pub(super) fn record_backend_event(&mut self, event: &BackendEvent) {
        let snapshot = self;
        if event.sequence <= snapshot.sequence {
            return;
        }
        let event_session = event.session_id.map(|session| session.to_string());
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
            } else {
                snapshot.pending_payload_revision = Some(event.sequence);
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
