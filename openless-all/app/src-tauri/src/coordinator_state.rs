//! Pure state-transition layer of the Coordinator.
//!
//! This module has no dependency on Tauri / audio / the system clipboard; it only describes the
//! Rust state machine of a dictation session. That way Windows CI can run real backend unit tests
//! without booting the full Tauri test harness.

use std::time::Instant;

use uuid::Uuid;

pub type SessionId = Uuid;

pub fn new_session_id() -> SessionId {
    Uuid::new_v4()
}

pub fn initial_session_id() -> SessionId {
    Uuid::nil()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionPhase {
    Idle,
    Starting,
    Listening,
    Processing,
    /// Window after the last cancel check, about to invoke / already invoking inserter.insert.
    /// cancel_session refuses to intervene in this phase: the simulated Cmd+V has started or has
    /// been sent and cannot be undone; forcing cancelled=true cannot rescue it and would only
    /// leave the UI showing "cancelled" while the text was still inserted. See the PR fixing
    /// Codex audit HIGH #2.
    Inserting,
}

pub(crate) struct SessionState {
    pub(crate) phase: SessionPhase,
    pub(crate) started_at: Instant,
    /// A stop edge pressed during Starting (ASR handshake; second toggle press / hold release) →
    /// end_session fires immediately once the handshake completes and phase=Listening, so the
    /// edge is not lost. issue #51.
    pub(crate) pending_stop: bool,
    /// Esc pressed during Processing: end_session skips insert + history.append at the
    /// polish/insert checkpoints. issue #52.
    pub(crate) cancelled: bool,
    pub(crate) focus_target: Option<usize>,
    /// Fresh UUID session id generated on each begin_session.
    /// The recorder error monitor holds the captured id; if it differs from the current one at
    /// handling time, the error is a late one from the previous session and must be dropped, not
    /// abort the current active session.
    pub(crate) session_id: SessionId,
    /// Foreground app label when the user started dictation
    /// ("Mail (com.apple.mail)" / Windows window title).
    /// Used as context precondition for LLM polish/translate so the model adapts style per app.
    /// See issue #116.
    pub(crate) front_app: Option<String>,
    /// Less Computer voice mode: set true when the dedicated Agent key is pressed. On getting the
    /// transcript, end_session branches on it — no polish-and-insert; instead the transcript goes
    /// to Claude to run a task and the result pops the capsule. Defaults to false.
    pub(crate) voice_agent: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            phase: SessionPhase::Idle,
            started_at: Instant::now(),
            pending_stop: false,
            cancelled: false,
            focus_target: None,
            session_id: initial_session_id(),
            front_app: None,
            voice_agent: false,
        }
    }
}

/// In-lock transition of begin_session: only Idle may enter Starting, and a new session id is
/// generated.
pub(crate) fn begin_session_state(
    state: &mut SessionState,
    focus_target: Option<usize>,
    front_app: Option<String>,
) -> Option<SessionId> {
    begin_session_state_with_id(state, focus_target, front_app, new_session_id())
}

/// Same as [`begin_session_state`], but lets the host generate the session id before entering the
/// Coordinator state machine. Less Computer needs to hand the same id to both the Core capture
/// lease and the host recording resources, so two states do not generate separate UUIDs that
/// cannot reliably cancel the same session.
pub(crate) fn begin_session_state_with_id(
    state: &mut SessionState,
    focus_target: Option<usize>,
    front_app: Option<String>,
    session_id: SessionId,
) -> Option<SessionId> {
    if state.phase != SessionPhase::Idle {
        return None;
    }
    state.phase = SessionPhase::Starting;
    state.started_at = Instant::now();
    state.pending_stop = false;
    state.cancelled = false;
    state.focus_target = focus_target;
    state.session_id = session_id;
    state.front_app = front_app;
    // Every new session defaults to plain dictation; the Less Computer entry point explicitly
    // marks it as a voice Agent.
    state.voice_agent = false;
    Some(state.session_id)
}

/// stop_dictation / hold release during Starting only records pending_stop, processed after
/// startup completes.
pub(crate) fn request_stop_during_starting_state(state: &mut SessionState) -> bool {
    if state.phase != SessionPhase::Starting {
        return false;
    }
    state.pending_stop = true;
    true
}

/// Result of the cancel race check between awaits inside begin_session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BeginOutcome {
    /// The startup continuation belongs to an old session; must not touch current session state.
    StaleContinuation,
    /// Entered Listening normally.
    Started,
    /// A pending_stop edge accumulated during Starting; end_session must run immediately (fast
    /// hold release / rapid toggle double press).
    PendingStop,
    /// cancel_session fired meanwhile (cancelled=true or phase externally reset to Idle).
    /// Must roll back recorder + ASR resources; do not enter Listening.
    CancelRaced,
}

pub(crate) fn finish_starting_session_state(
    state: &mut SessionState,
    session_id: SessionId,
) -> BeginOutcome {
    if state.session_id != session_id {
        BeginOutcome::StaleContinuation
    } else if state.cancelled || state.phase != SessionPhase::Starting {
        BeginOutcome::CancelRaced
    } else {
        state.phase = SessionPhase::Listening;
        let pending = std::mem::replace(&mut state.pending_stop, false);
        if pending {
            BeginOutcome::PendingStop
        } else {
            BeginOutcome::Started
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupRaceStatus {
    ActiveStarting,
    CancelRaced,
    StaleContinuation,
}

pub(crate) fn startup_race_status(
    state: &SessionState,
    captured_session_id: SessionId,
) -> StartupRaceStatus {
    if state.session_id != captured_session_id {
        StartupRaceStatus::StaleContinuation
    } else if state.cancelled || state.phase != SessionPhase::Starting {
        StartupRaceStatus::CancelRaced
    } else {
        StartupRaceStatus::ActiveStarting
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CancelDecision {
    pub(crate) phase: SessionPhase,
    pub(crate) session_id: SessionId,
}

/// In-lock first half of cancel_session. Idle / Inserting cannot be cancelled; other phases set
/// cancelled.
pub(crate) fn begin_cancel_session_state(state: &mut SessionState) -> Option<CancelDecision> {
    let phase = state.phase;
    if matches!(phase, SessionPhase::Idle | SessionPhase::Inserting) {
        return None;
    }
    state.cancelled = true;
    Some(CancelDecision {
        phase,
        session_id: state.session_id,
    })
}

/// In-lock wrap-up of cancel_session after external resource cleanup. In Processing, the phase is
/// left for end_session to finish itself (avoiding a race with the polish/insert path), but
/// focus_target is the current session's window resource handle and must be released on cancel
/// regardless of phase, so the gap before the next session is not polluted by a stale value. See
/// audit 3.3.5.
pub(crate) fn finish_cancel_session_state(state: &mut SessionState, decision: CancelDecision) {
    state.focus_target = None;
    if decision.phase != SessionPhase::Processing {
        state.phase = SessionPhase::Idle;
    }
}

/// Completes the cancel wrap-up for a session that already entered Processing.
///
/// cancel_session must not move Processing straight to Idle, or it would race with end_session's
/// polish/insert wrap-up; end_session therefore calls this at its cancel early-exit points. The
/// session id check keeps a late continuation from an old session from mutating the new one.
pub(crate) fn finish_cancelled_processing_state(
    state: &mut SessionState,
    session_id: SessionId,
) -> bool {
    if state.session_id != session_id || !state.cancelled {
        return false;
    }
    if state.phase == SessionPhase::Processing {
        state.phase = SessionPhase::Idle;
    }
    if state.phase != SessionPhase::Idle {
        return false;
    }
    state.focus_target = None;
    true
}

pub(crate) fn start_processing_if_listening(state: &mut SessionState) -> Option<SessionId> {
    if state.phase != SessionPhase::Listening {
        return None;
    }
    state.phase = SessionPhase::Processing;
    Some(state.session_id)
}

pub(crate) struct RecordingAbort {
    pub(crate) elapsed: u64,
    pub(crate) session_id: SessionId,
}

pub(crate) fn begin_recording_abort_before_restore(
    state: &mut SessionState,
) -> Option<RecordingAbort> {
    if state.cancelled
        || !matches!(
            state.phase,
            SessionPhase::Starting | SessionPhase::Listening
        )
    {
        return None;
    }
    state.cancelled = true;
    Some(RecordingAbort {
        elapsed: state.started_at.elapsed().as_millis() as u64,
        session_id: state.session_id,
    })
}

pub(crate) fn publish_abort_idle_after_restore(state: &mut SessionState, session_id: SessionId) {
    if state.session_id == session_id {
        state.phase = SessionPhase::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_id(n: u128) -> SessionId {
        Uuid::from_u128(n)
    }

    #[test]
    fn begin_session_enters_starting_and_clears_stale_edges() {
        let mut state = SessionState {
            pending_stop: true,
            cancelled: true,
            ..Default::default()
        };

        let id = begin_session_state(
            &mut state,
            Some(7),
            Some("Terminal (com.apple.Terminal)".into()),
        )
        .unwrap();

        assert_eq!(state.phase, SessionPhase::Starting);
        assert!(!state.pending_stop);
        assert!(!state.cancelled);
        assert_eq!(state.focus_target, Some(7));
        assert_eq!(
            state.front_app.as_deref(),
            Some("Terminal (com.apple.Terminal)")
        );
        assert_eq!(state.session_id, id);
        assert_ne!(id, initial_session_id());
    }

    #[test]
    fn begin_session_resets_voice_agent_flag() {
        // Safety guard: a stale voice_agent=true from the previous session must never let the
        // next plain dictation be misjudged as Cloud Agent (otherwise the dictation text would be
        // sent to run Claude instead of being inserted at the cursor).
        let mut state = SessionState {
            voice_agent: true,
            ..Default::default()
        };
        begin_session_state(&mut state, None, None).unwrap();
        assert!(!state.voice_agent, "新会话必须从普通听写开始");
    }

    #[test]
    fn begin_session_with_id_preserves_host_core_session_identity() {
        let mut state = SessionState::default();
        let expected = session_id(42);

        let actual = begin_session_state_with_id(&mut state, None, None, expected).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(state.session_id, expected);
        assert_eq!(state.phase, SessionPhase::Starting);
    }

    #[test]
    fn begin_session_ignores_non_idle_phase() {
        let mut state = SessionState {
            phase: SessionPhase::Processing,
            session_id: session_id(99),
            ..Default::default()
        };

        assert!(begin_session_state(&mut state, Some(1), Some("Mail".into())).is_none());

        assert_eq!(state.phase, SessionPhase::Processing);
        assert_eq!(state.session_id, session_id(99));
        assert!(state.focus_target.is_none());
        assert!(state.front_app.is_none());
    }

    #[test]
    fn stop_during_starting_sets_pending_stop_only_for_starting() {
        let mut state = SessionState {
            phase: SessionPhase::Starting,
            ..Default::default()
        };

        assert!(request_stop_during_starting_state(&mut state));
        assert!(state.pending_stop);

        state.phase = SessionPhase::Listening;
        state.pending_stop = false;
        assert!(!request_stop_during_starting_state(&mut state));
        assert!(!state.pending_stop);
    }

    #[test]
    fn finish_starting_is_table_driven_for_pending_cancel_and_stale_edges() {
        let cases = [
            (
                SessionPhase::Starting,
                false,
                false,
                session_id(7),
                BeginOutcome::Started,
                SessionPhase::Listening,
            ),
            (
                SessionPhase::Starting,
                false,
                true,
                session_id(7),
                BeginOutcome::PendingStop,
                SessionPhase::Listening,
            ),
            (
                SessionPhase::Starting,
                true,
                false,
                session_id(7),
                BeginOutcome::CancelRaced,
                SessionPhase::Starting,
            ),
            (
                SessionPhase::Idle,
                false,
                false,
                session_id(7),
                BeginOutcome::CancelRaced,
                SessionPhase::Idle,
            ),
            (
                SessionPhase::Starting,
                false,
                false,
                session_id(8),
                BeginOutcome::StaleContinuation,
                SessionPhase::Starting,
            ),
        ];

        for (phase, cancelled, pending_stop, actual_id, expected, expected_phase) in cases {
            let mut state = SessionState {
                phase,
                cancelled,
                pending_stop,
                session_id: actual_id,
                ..Default::default()
            };

            assert_eq!(
                finish_starting_session_state(&mut state, session_id(7)),
                expected,
                "phase={phase:?} cancelled={cancelled} pending_stop={pending_stop} actual_id={actual_id}"
            );
            assert_eq!(state.phase, expected_phase);
            if expected == BeginOutcome::PendingStop {
                assert!(!state.pending_stop);
            }
        }
    }

    #[test]
    fn cancel_session_state_machine_is_table_driven() {
        let cases = [
            (SessionPhase::Idle, SessionPhase::Idle, false),
            (SessionPhase::Starting, SessionPhase::Idle, true),
            (SessionPhase::Listening, SessionPhase::Idle, true),
            (SessionPhase::Processing, SessionPhase::Processing, true),
            (SessionPhase::Inserting, SessionPhase::Inserting, false),
        ];

        for (initial, expected_phase, expected_cancelled) in cases {
            let mut state = SessionState {
                phase: initial,
                cancelled: false,
                focus_target: Some(1),
                session_id: session_id(42),
                ..Default::default()
            };

            if let Some(decision) = begin_cancel_session_state(&mut state) {
                finish_cancel_session_state(&mut state, decision);
            }

            assert_eq!(state.phase, expected_phase, "initial={initial:?}");
            assert_eq!(state.cancelled, expected_cancelled, "initial={initial:?}");
            // Any phase accepted by begin_cancel_session_state (i.e. not Idle/Inserting) must
            // clear focus_target, including Processing — this is the regression card for audit
            // 3.3.5.
            if expected_cancelled {
                assert!(
                    state.focus_target.is_none(),
                    "focus_target should clear after cancel, initial={initial:?}"
                );
            } else {
                assert_eq!(
                    state.focus_target,
                    Some(1),
                    "rejected cancel must not touch focus_target, initial={initial:?}"
                );
            }
        }
    }

    #[test]
    fn finish_cancelled_processing_state_returns_idle_for_matching_session() {
        let mut state = SessionState {
            phase: SessionPhase::Processing,
            cancelled: true,
            focus_target: Some(1),
            session_id: session_id(42),
            ..Default::default()
        };

        assert!(finish_cancelled_processing_state(
            &mut state,
            session_id(42)
        ));
        assert_eq!(state.phase, SessionPhase::Idle);
        assert!(state.focus_target.is_none());
    }

    #[test]
    fn finish_cancelled_processing_state_rejects_stale_session() {
        let mut state = SessionState {
            phase: SessionPhase::Processing,
            cancelled: true,
            focus_target: Some(1),
            session_id: session_id(42),
            ..Default::default()
        };

        assert!(!finish_cancelled_processing_state(
            &mut state,
            session_id(41)
        ));
        assert_eq!(state.phase, SessionPhase::Processing);
        assert_eq!(state.focus_target, Some(1));
    }

    #[test]
    fn stop_dictation_from_listening_enters_processing_once() {
        let mut state = SessionState {
            phase: SessionPhase::Listening,
            session_id: session_id(123),
            ..Default::default()
        };

        assert_eq!(
            start_processing_if_listening(&mut state),
            Some(session_id(123))
        );
        assert_eq!(state.phase, SessionPhase::Processing);
        assert_eq!(start_processing_if_listening(&mut state), None);
        assert_eq!(state.phase, SessionPhase::Processing);
    }

    #[test]
    fn startup_race_check_is_table_driven_for_begin_session_edges() {
        let cases = [
            (
                SessionPhase::Starting,
                false,
                session_id(7),
                StartupRaceStatus::ActiveStarting,
            ),
            (
                SessionPhase::Starting,
                true,
                session_id(7),
                StartupRaceStatus::CancelRaced,
            ),
            (
                SessionPhase::Idle,
                false,
                session_id(7),
                StartupRaceStatus::CancelRaced,
            ),
            (
                SessionPhase::Listening,
                false,
                session_id(7),
                StartupRaceStatus::CancelRaced,
            ),
            (
                SessionPhase::Starting,
                false,
                session_id(8),
                StartupRaceStatus::StaleContinuation,
            ),
        ];

        for (phase, cancelled, actual_session_id, expected) in cases {
            let state = SessionState {
                phase,
                cancelled,
                session_id: actual_session_id,
                ..Default::default()
            };

            assert_eq!(
                startup_race_status(&state, session_id(7)),
                expected,
                "phase={phase:?} cancelled={cancelled} actual_session={actual_session_id}"
            );
        }
    }

    #[test]
    fn recording_abort_keeps_session_non_idle_until_restore_can_run() {
        let mut state = SessionState {
            phase: SessionPhase::Listening,
            cancelled: false,
            session_id: session_id(7),
            ..Default::default()
        };

        let abort = begin_recording_abort_before_restore(&mut state).unwrap();

        assert_eq!(abort.session_id, session_id(7));
        assert!(state.cancelled);
        assert_eq!(state.phase, SessionPhase::Listening);

        publish_abort_idle_after_restore(&mut state, abort.session_id);

        assert_eq!(state.phase, SessionPhase::Idle);
    }

    #[test]
    fn recording_abort_is_noop_after_prior_cancel_or_idle() {
        let cases = [
            (SessionPhase::Idle, false),
            (SessionPhase::Processing, false),
            (SessionPhase::Listening, true),
        ];

        for (phase, cancelled) in cases {
            let mut state = SessionState {
                phase,
                cancelled,
                ..Default::default()
            };

            assert!(begin_recording_abort_before_restore(&mut state).is_none());
            assert_eq!(state.phase, phase);
            assert_eq!(state.cancelled, cancelled);
        }
    }
}
