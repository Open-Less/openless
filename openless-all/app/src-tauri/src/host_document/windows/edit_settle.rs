use std::time::{Duration, Instant};

// UIA hosts without a TextEdit pattern only expose changes, not IME state.
// Give users a longer pause to finish their edits in those hosts too.
const QUIET_PERIOD: Duration = Duration::from_secs(2);

pub(crate) struct EditSettle {
    changed_at: Option<Instant>,
    composing: bool,
}

impl EditSettle {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            changed_at: Some(now),
            composing: false,
        }
    }

    pub(crate) fn mark_changed(&mut self, now: Instant) {
        self.changed_at = Some(now);
    }

    pub(crate) fn ready(&self, now: Instant) -> bool {
        self.changed_at
            .is_some_and(|at| now.duration_since(at) >= QUIET_PERIOD)
    }

    pub(crate) fn should_probe(&self) -> bool {
        self.changed_at.is_some()
    }

    pub(crate) fn observe_composition(&mut self, active: bool, now: Instant) {
        if active || self.composing {
            self.mark_changed(now);
        }
        self.composing = active;
    }

    pub(crate) fn confirm_snapshot(&mut self, snapshot: &str, latest: &str, now: Instant) -> bool {
        if snapshot != latest {
            self.mark_changed(now);
            return false;
        }
        true
    }

    pub(crate) fn clear(&mut self) {
        self.changed_at = None;
    }
}

pub(crate) fn has_edit_events(text: bool, value: bool, composition_finalized: bool) -> bool {
    text || value || composition_finalized
}

/// Finish focus checks before sampling composition: reconversion can begin
/// without changing the text. Sample local vetoes after both provider calls.
pub(crate) fn can_publish(
    editor_is_current: impl FnOnce() -> bool,
    is_composing: impl FnOnce() -> bool,
    locally_eligible: impl FnOnce() -> bool,
) -> bool {
    editor_is_current() && !is_composing() && locally_eligible()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_finalization_alone_keeps_the_watcher_armed() {
        assert!(has_edit_events(false, false, true));
        assert!(!has_edit_events(false, false, false));
    }

    #[test]
    fn short_pause_and_new_edit_do_not_release_the_old_candidate() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut settle = EditSettle::new(start);
        assert!(settle.should_probe());
        assert!(!settle.ready(at(700)));
        settle.mark_changed(at(1400));
        assert!(!settle.ready(at(3399)));
        assert!(settle.ready(at(3400)));
        settle.observe_composition(false, at(3400));
        assert!(settle.ready(at(3400)));
        settle.clear();
        assert!(!settle.should_probe());
    }

    #[test]
    fn composition_holds_the_candidate_even_without_text_change_events() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut settle = EditSettle::new(start);
        assert!(settle.should_probe());
        settle.observe_composition(true, at(2000));
        assert!(!settle.ready(at(2000)));
        assert!(settle.should_probe());
        settle.observe_composition(true, at(8000));
        assert!(!settle.ready(at(8000)));
        // The user confirms the candidate, but the editor sends no text event.
        settle.observe_composition(false, at(8100));
        assert!(!settle.ready(at(10099)));
        assert!(settle.ready(at(10100)));
    }

    #[test]
    fn finalization_event_restarts_quiet_period_even_after_cancellation() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut settle = EditSettle::new(start);
        settle.mark_changed(at(1999));
        settle.observe_composition(false, at(2000));
        assert!(!settle.ready(at(2000)));
        assert!(settle.ready(at(3999)));
    }

    #[test]
    fn ime_commit_between_text_and_composition_reads_restarts_quiet_period() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut settle = EditSettle::new(start);
        assert!(settle.ready(at(2000)));

        let snapshot = "我要bu'ji";
        // The first read captured preedit text. The IME has since committed,
        // but the watcher has received no finalization event.
        settle.observe_composition(false, at(2001));
        assert!(settle.ready(at(2001)));
        assert!(!settle.confirm_snapshot(snapshot, "我要补给", at(2002)));

        assert!(!settle.ready(at(4001)));
        assert!(settle.ready(at(4002)));
        assert!(settle.confirm_snapshot("我要补给", "我要补给", at(4002)));
        assert!(settle.ready(at(4002)));
    }

    #[test]
    fn change_arriving_during_the_final_provider_check_vetoes_publish() {
        let dirty = std::cell::Cell::new(false);
        assert!(!can_publish(
            || {
                dirty.set(true);
                true
            },
            || false,
            || !dirty.get(),
        ));
        dirty.set(false);
        assert!(can_publish(|| true, || false, || !dirty.get()));
        assert!(!can_publish(|| false, || false, || !dirty.get()));
    }

    #[test]
    fn reconversion_starting_during_focus_check_vetoes_publish_without_a_text_change() {
        let composing = std::cell::Cell::new(false);
        assert!(!can_publish(
            || {
                composing.set(true);
                true
            },
            || composing.get(),
            || true,
        ));
    }
}
