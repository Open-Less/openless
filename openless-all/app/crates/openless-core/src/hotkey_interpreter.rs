use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::shared_types::HotkeyMode;
use crate::types::DictationPhase;

/// Auto mode treats a shorter press as a latched Toggle press and a longer
/// press as Hold-to-talk. Event timestamps are supplied by the native listener,
/// so a busy async executor cannot accidentally change the user's gesture.
const AUTO_HOLD_THRESHOLD: Duration = Duration::from_millis(350);
/// Suppress switch bounce and accidental press-release-press bursts before they
/// allocate another recorder or ASR session.
const HOTKEY_DEBOUNCE: Duration = Duration::from_millis(250);
/// A completed session keeps the next queued press from opening a surprise
/// recording while the terminal capsule is still leaving (#545 and #856).
const TERMINAL_COOLDOWN: Duration = Duration::from_millis(450);
/// Modifier-only triggers are ambiguous until the host has had enough time to
/// report a companion key. Explicit custom combinations skip this delay.
const MODIFIER_ARBITRATION_GRACE: Duration = Duration::from_millis(150);
/// Native events may overtake the serialized Pressed bridge while session start
/// awaits microphone/ASR setup. Keep a bounded set of those early Combined ids.
const MAX_PENDING_COMBINED: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyIntent {
    Noop,
    WaitForModifierGrace { press_id: u64 },
    Start { press_id: u64 },
    Stop,
    Cancel { press_id: u64 },
}

#[derive(Debug, Clone, Copy)]
struct Press {
    /// Stable id allocated by the host once per physical down/up cycle.
    id: u64,
    /// Native monotonic timestamp used for Auto mode duration classification.
    at: Instant,
    /// False means this press was rejected by debounce/cooldown and its release
    /// must remain a no-op even if a different session is active by then.
    accepted: bool,
}

/// Core-owned interpreter for the main dictation hotkey.
///
/// Hosts only allocate a `press_id` and forward timestamped edges. Keeping all
/// policy here makes the macOS/Windows hook, Tauri window fallback, and Linux
/// fcitx5 path share the same generation, arbitration, debounce, and cooldown
/// semantics.
#[derive(Debug, Default)]
pub(crate) struct HotkeyInterpreter {
    /// The latest unmatched physical press. Release must carry the same id.
    held: Option<Press>,
    /// The press generation that actually started the active Core session.
    /// Combined may cancel only this generation, never a later session.
    session_press_id: Option<u64>,
    /// Combined can arrive before Pressed because it uses a separate low-latency
    /// channel. The queue preserves multiple overtaking generations in order.
    pending_combined: VecDeque<u64>,
    /// Last accepted edge that consumed the debounce window. A Combined gesture
    /// clears its own entry because it never represented a dictation request.
    last_dispatch: Option<(u64, Instant)>,
    /// Set whenever a dictation reaches any terminal path and cleared only when
    /// the deadline expires naturally or a same-generation combo is cancelled.
    cooldown_until: Option<Instant>,
}

impl HotkeyInterpreter {
    pub(crate) const MODIFIER_ARBITRATION_GRACE: Duration = MODIFIER_ARBITRATION_GRACE;

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn press(
        &mut self,
        press_id: u64,
        at: Instant,
        mode: HotkeyMode,
        phase: DictationPhase,
        modifier_only: bool,
    ) -> HotkeyIntent {
        // Zero is reserved for an unmatched host edge. Repeated key-down events
        // retain their physical id, so they are ignored without touching state.
        if press_id == 0 || self.held.is_some_and(|press| press.id == press_id) {
            return HotkeyIntent::Noop;
        }
        // A fresh generation supersedes an unmatched stale press. This keeps a
        // dropped native key-up from permanently latching the interpreter.
        self.held = None;
        self.held = Some(Press {
            id: press_id,
            at,
            accepted: false,
        });
        if self.take_combined(press_id) {
            // The companion key overtook Pressed on its independent bridge. It
            // must consume this generation before any microphone work starts.
            self.held = None;
            return HotkeyIntent::Noop;
        }
        if self
            .last_dispatch
            .is_some_and(|(_, last)| at.saturating_duration_since(last) < HOTKEY_DEBOUNCE)
        {
            return HotkeyIntent::Noop;
        }
        self.last_dispatch = Some((press_id, at));
        self.held.as_mut().expect("press was just stored").accepted = true;

        let intent = match (mode, phase) {
            (_, DictationPhase::Idle)
                if self.cooldown_until.is_some_and(|deadline| at < deadline) =>
            {
                HotkeyIntent::Noop
            }
            (HotkeyMode::Hold, DictationPhase::Idle)
            | (HotkeyMode::Auto, DictationPhase::Idle)
            | (HotkeyMode::Toggle | HotkeyMode::DoubleClick, DictationPhase::Idle) => {
                HotkeyIntent::Start { press_id }
            }
            (
                HotkeyMode::Auto | HotkeyMode::Toggle | HotkeyMode::DoubleClick,
                DictationPhase::Starting | DictationPhase::Recording,
            ) => HotkeyIntent::Stop,
            _ => HotkeyIntent::Noop,
        };
        if matches!(intent, HotkeyIntent::Start { .. }) && modifier_only {
            // Option/Ctrl by itself may be speech, while Option/Ctrl plus a
            // normal key is ordinary typing. The caller waits once and asks us
            // to resolve the same generation through `after_modifier_grace`.
            HotkeyIntent::WaitForModifierGrace { press_id }
        } else {
            if matches!(intent, HotkeyIntent::Start { .. }) {
                self.session_press_id = Some(press_id);
            }
            intent
        }
    }

    pub(crate) fn after_modifier_grace(
        &mut self,
        press_id: u64,
        phase: DictationPhase,
    ) -> HotkeyIntent {
        if self.take_combined(press_id) {
            // Cancelling during grace must not consume the debounce window;
            // the user's next intentional press should work immediately.
            self.clear_press(press_id);
            return HotkeyIntent::Noop;
        }
        if !self
            .held
            .is_some_and(|press| press.id == press_id && press.accepted)
            || phase != DictationPhase::Idle
        {
            return HotkeyIntent::Noop;
        }
        self.session_press_id = Some(press_id);
        HotkeyIntent::Start { press_id }
    }

    pub(crate) fn release(
        &mut self,
        press_id: u64,
        at: Instant,
        mode: HotkeyMode,
        phase: DictationPhase,
    ) -> HotkeyIntent {
        // Generation matching is the release-side stale-event guard. An old
        // key-up can never stop a session opened by a newer press.
        let Some(press) = self.held.filter(|press| press.id == press_id) else {
            return HotkeyIntent::Noop;
        };
        self.held = None;
        if !press.accepted || self.take_combined(press_id) {
            return HotkeyIntent::Noop;
        }
        let active = matches!(phase, DictationPhase::Starting | DictationPhase::Recording)
            && self.session_press_id == Some(press_id);
        match mode {
            HotkeyMode::Hold if active => HotkeyIntent::Stop,
            HotkeyMode::Auto
                if active && at.saturating_duration_since(press.at) >= AUTO_HOLD_THRESHOLD =>
            {
                HotkeyIntent::Stop
            }
            HotkeyMode::Toggle | HotkeyMode::DoubleClick | HotkeyMode::Auto | HotkeyMode::Hold => {
                HotkeyIntent::Noop
            }
        }
    }

    pub(crate) fn combined(&mut self, press_id: u64) -> HotkeyIntent {
        if press_id == 0 {
            return HotkeyIntent::Noop;
        }
        if !self.pending_combined.contains(&press_id) {
            self.pending_combined.push_back(press_id);
            if self.pending_combined.len() > MAX_PENDING_COMBINED {
                self.pending_combined.pop_front();
            }
        }
        self.clear_press(press_id);
        // A companion key cancels only when this exact press created the active
        // session. Otherwise the queued marker is consumed by Pressed/grace or
        // eventually evicted as a harmless stale generation.
        if self.session_press_id == Some(press_id) {
            HotkeyIntent::Cancel { press_id }
        } else {
            HotkeyIntent::Noop
        }
    }

    pub(crate) fn start_finished(&mut self, press_id: u64, started: bool) -> bool {
        // Start awaits platform work. Re-check the queue afterwards to close the
        // race where Combined arrived after Start intent but before setup ended.
        let combined = self.take_combined(press_id);
        if (!started || combined) && self.session_press_id == Some(press_id) {
            self.session_press_id = None;
        }
        combined
    }

    pub(crate) fn terminal(&mut self, at: Instant) {
        // Clear generation state on every success/failure/cancel terminal path;
        // the cooldown is deliberately shared across all those outcomes.
        self.held = None;
        self.session_press_id = None;
        self.cooldown_until = Some(at + TERMINAL_COOLDOWN);
    }

    pub(crate) fn combo_cancelled(&mut self, press_id: u64) {
        self.take_combined(press_id);
        self.clear_press(press_id);
        if self.session_press_id == Some(press_id) {
            self.session_press_id = None;
        }
        // A combined keyboard gesture never requested dictation, so it should
        // not penalize the next real press with session cooldown.
        self.cooldown_until = None;
    }

    fn clear_press(&mut self, press_id: u64) {
        if self.held.is_some_and(|press| press.id == press_id) {
            self.held = None;
        }
        if self
            .last_dispatch
            .is_some_and(|(last_press_id, _)| last_press_id == press_id)
        {
            self.last_dispatch = None;
        }
    }

    fn take_combined(&mut self, press_id: u64) -> bool {
        self.pending_combined
            .iter()
            .position(|pending| *pending == press_id)
            .and_then(|index| self.pending_combined.remove(index))
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_before_pressed_cancels_only_that_generation() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(
            interpreter.combined(1),
            HotkeyIntent::Noop,
            "Combined may arrive before Pressed"
        );
        assert_eq!(
            interpreter.press(1, start, HotkeyMode::Auto, DictationPhase::Idle, true),
            HotkeyIntent::Noop
        );
        assert_eq!(
            interpreter.press(
                2,
                start + Duration::from_millis(10),
                HotkeyMode::Auto,
                DictationPhase::Idle,
                false,
            ),
            HotkeyIntent::Start { press_id: 2 }
        );
    }

    #[test]
    fn modifier_only_waits_for_grace_but_custom_combo_starts_immediately() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(
            interpreter.press(1, start, HotkeyMode::Auto, DictationPhase::Idle, true,),
            HotkeyIntent::WaitForModifierGrace { press_id: 1 }
        );
        assert_eq!(interpreter.combined(1), HotkeyIntent::Noop);
        assert_eq!(
            interpreter.after_modifier_grace(1, DictationPhase::Idle),
            HotkeyIntent::Noop
        );
        assert_eq!(
            interpreter.press(
                2,
                start + Duration::from_millis(10),
                HotkeyMode::Auto,
                DictationPhase::Idle,
                false,
            ),
            HotkeyIntent::Start { press_id: 2 },
            "an explicit custom combo has no modifier-only ambiguity"
        );
    }

    #[test]
    fn auto_uses_native_press_duration_and_release_generation() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(
            interpreter.press(1, start, HotkeyMode::Auto, DictationPhase::Idle, false),
            HotkeyIntent::Start { press_id: 1 }
        );
        assert_eq!(
            interpreter.release(
                99,
                start + Duration::from_millis(500),
                HotkeyMode::Auto,
                DictationPhase::Recording,
            ),
            HotkeyIntent::Noop,
            "a stale release must not stop the current generation"
        );
        assert_eq!(
            interpreter.release(
                1,
                start + Duration::from_millis(500),
                HotkeyMode::Auto,
                DictationPhase::Recording,
            ),
            HotkeyIntent::Stop
        );
    }

    #[test]
    fn debounce_suppresses_a_press_release_press_burst() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(
            interpreter.press(1, start, HotkeyMode::Hold, DictationPhase::Idle, false),
            HotkeyIntent::Start { press_id: 1 }
        );
        assert_eq!(
            interpreter.release(
                1,
                start + Duration::from_millis(20),
                HotkeyMode::Hold,
                DictationPhase::Recording,
            ),
            HotkeyIntent::Stop
        );
        assert_eq!(
            interpreter.press(
                2,
                start + Duration::from_millis(100),
                HotkeyMode::Hold,
                DictationPhase::Idle,
                false,
            ),
            HotkeyIntent::Noop
        );
    }

    #[test]
    fn terminal_cooldown_drops_issue_545_third_press() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(
            interpreter.press(1, start, HotkeyMode::Toggle, DictationPhase::Idle, false),
            HotkeyIntent::Start { press_id: 1 }
        );
        assert_eq!(
            interpreter.press(
                2,
                start + Duration::from_millis(300),
                HotkeyMode::Toggle,
                DictationPhase::Recording,
                false,
            ),
            HotkeyIntent::Stop
        );
        interpreter.terminal(start + Duration::from_millis(310));
        assert_eq!(
            interpreter.press(
                3,
                start + Duration::from_millis(600),
                HotkeyMode::Toggle,
                DictationPhase::Idle,
                false,
            ),
            HotkeyIntent::Noop,
            "#545: the third press must not restart during the terminal animation"
        );
        assert_eq!(
            interpreter.release(
                3,
                start + Duration::from_millis(610),
                HotkeyMode::Toggle,
                DictationPhase::Idle,
            ),
            HotkeyIntent::Noop
        );
    }

    #[test]
    fn issue_856_processing_press_stays_discarded_after_terminal() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(
            interpreter.press(
                1,
                start,
                HotkeyMode::Toggle,
                DictationPhase::Transcribing,
                false,
            ),
            HotkeyIntent::Noop,
            "#856: a press dequeued during processing is not a future start request"
        );
        interpreter.terminal(start + Duration::from_millis(10));
        assert_eq!(
            interpreter.release(
                1,
                start + Duration::from_millis(20),
                HotkeyMode::Toggle,
                DictationPhase::Idle,
            ),
            HotkeyIntent::Noop
        );
        assert_eq!(
            interpreter.press(
                2,
                start + Duration::from_millis(100),
                HotkeyMode::Toggle,
                DictationPhase::Idle,
                false,
            ),
            HotkeyIntent::Noop,
            "a queued follow-up edge inside terminal cooldown must also be dropped"
        );
    }

    #[test]
    fn combined_during_start_cancels_the_same_press_and_clears_debounce() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(
            interpreter.press(1, start, HotkeyMode::Hold, DictationPhase::Idle, false),
            HotkeyIntent::Start { press_id: 1 }
        );
        assert_eq!(
            interpreter.combined(1),
            HotkeyIntent::Cancel { press_id: 1 }
        );
        interpreter.combo_cancelled(1);
        assert_eq!(
            interpreter.press(
                2,
                start + Duration::from_millis(1),
                HotkeyMode::Hold,
                DictationPhase::Idle,
                false,
            ),
            HotkeyIntent::Start { press_id: 2 },
            "a cancelled keyboard combo must not consume the next debounce window"
        );
    }

    #[test]
    fn combined_during_start_await_is_rechecked_after_setup() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(
            interpreter.press(7, start, HotkeyMode::Hold, DictationPhase::Idle, false),
            HotkeyIntent::Start { press_id: 7 }
        );
        assert_eq!(
            interpreter.combined(7),
            HotkeyIntent::Cancel { press_id: 7 }
        );
        assert!(
            interpreter.start_finished(7, true),
            "the start task must observe a Combined edge that arrived while it awaited setup"
        );
    }

    #[test]
    fn pending_combined_queue_preserves_overtaking_press_ids() {
        let start = Instant::now();
        let mut interpreter = HotkeyInterpreter::default();

        assert_eq!(interpreter.combined(11), HotkeyIntent::Noop);
        assert_eq!(interpreter.combined(12), HotkeyIntent::Noop);
        assert_eq!(
            interpreter.press(11, start, HotkeyMode::Hold, DictationPhase::Idle, true),
            HotkeyIntent::Noop
        );
        assert_eq!(
            interpreter.press(
                12,
                start + Duration::from_millis(1),
                HotkeyMode::Hold,
                DictationPhase::Idle,
                true,
            ),
            HotkeyIntent::Noop
        );
        assert_eq!(
            interpreter.press(
                13,
                start + Duration::from_millis(2),
                HotkeyMode::Hold,
                DictationPhase::Idle,
                false,
            ),
            HotkeyIntent::Start { press_id: 13 }
        );
    }
}
