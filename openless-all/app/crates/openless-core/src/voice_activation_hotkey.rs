//! Runtime-only intent and physical-click arbitration for linked voice activation.
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::api::DictationHotkeyEdge;

#[derive(Default)]
pub(crate) struct VoiceActivationLink {
    linked: bool,
    pub(crate) requested: bool,
    pub(crate) revision: u64,
    pub(crate) finalize_session: Option<crate::types::SessionId>,
    pub(crate) finalize_generation: Option<u64>,
    pressed: Option<(u64, Instant)>,
    consumed: VecDeque<u64>,
    combined: HashSet<u64>,
    last_click: Option<Instant>,
}

impl VoiceActivationLink {
    pub(crate) fn configure(&mut self, linked: bool) {
        if self.linked != linked {
            self.linked = linked;
            self.requested = false;
            self.revision += 1;
            self.pressed = None;
            self.consumed.clear();
            self.combined.clear();
            self.last_click = None;
        }
    }

    pub(crate) fn pause(&mut self) {
        self.requested = false;
        self.revision += 1;
    }

    pub(crate) fn edge(&mut self, edge: DictationHotkeyEdge) -> bool {
        match edge {
            DictationHotkeyEdge::Pressed { press_id, at } => {
                if !self.consumed.contains(&press_id) {
                    // A fresh physical ID supersedes a release lost while the
                    // native shortcut recorder or registration was suspended.
                    if self.pressed.map(|press| press.0) != Some(press_id) {
                        self.pressed = Some((press_id, at));
                    }
                }
            }
            DictationHotkeyEdge::Combined { press_id, .. } => {
                if self.combined.len() >= 64 {
                    self.combined.clear();
                }
                self.combined.insert(press_id);
            }
            DictationHotkeyEdge::Released { press_id, at } => {
                if self.pressed.map(|press| press.0) != Some(press_id) {
                    return false;
                }
                let (_, pressed_at) = self.pressed.take().expect("matched physical press");
                self.consumed.push_back(press_id);
                if self.consumed.len() > 64 {
                    self.consumed.pop_front();
                }
                if self.combined.remove(&press_id) || at < pressed_at {
                    return false;
                }
                if self.last_click.is_some_and(|last| {
                    at.saturating_duration_since(last) < Duration::from_millis(250)
                }) {
                    return false;
                }
                self.last_click = Some(at);
                self.requested = !self.requested;
                self.revision += 1;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_press_recovers_after_a_suppressed_release() {
        let mut state = VoiceActivationLink::default();
        state.configure(true);
        let at = Instant::now();
        state.edge(DictationHotkeyEdge::Pressed { press_id: 1, at });
        state.edge(DictationHotkeyEdge::Pressed { press_id: 2, at });
        assert!(state.edge(DictationHotkeyEdge::Released { press_id: 2, at }));
        assert!(state.requested);
        assert!(!state.edge(DictationHotkeyEdge::Released { press_id: 1, at }));
    }

    #[test]
    fn linked_clicks_are_paired_debounced_and_combination_safe() {
        let mut state = VoiceActivationLink::default();
        state.configure(true);
        let now = Instant::now();
        assert!(!state.requested);
        assert!(!state.edge(DictationHotkeyEdge::Released { press_id: 1, at: now }));
        assert!(!state.edge(DictationHotkeyEdge::Pressed { press_id: 1, at: now }));
        assert!(!state.edge(DictationHotkeyEdge::Pressed { press_id: 1, at: now }));
        assert!(state.edge(DictationHotkeyEdge::Released { press_id: 1, at: now }));
        assert!(state.requested);
        assert!(!state.edge(DictationHotkeyEdge::Released { press_id: 1, at: now }));
        state.edge(DictationHotkeyEdge::Pressed { press_id: 2, at: now });
        assert!(!state.edge(DictationHotkeyEdge::Released { press_id: 2, at: now }));
        let later = now + Duration::from_secs(1);
        state.edge(DictationHotkeyEdge::Combined { press_id: 3, at: later });
        state.edge(DictationHotkeyEdge::Pressed { press_id: 3, at: later });
        assert!(!state.edge(DictationHotkeyEdge::Released { press_id: 3, at: later }));
        state.edge(DictationHotkeyEdge::Pressed { press_id: 4, at: later });
        assert!(state.edge(DictationHotkeyEdge::Released { press_id: 4, at: later }));
        assert!(!state.requested);
        state.configure(false);
        state.configure(true);
        assert!(!state.requested);
    }
}
