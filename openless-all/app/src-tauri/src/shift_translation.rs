//! Standalone Shift taps. No timer can outlive a recording: expiry is checked on input.
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShiftTranslationContext {
    pub session_id: openless_core::SessionId,
    pub preferences_revision: u64,
    pub tap_count: u8,
    /// The native recording controller may stay held while Shift is tapped.
    pub held_recording_key: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(count: u8) -> ShiftTranslationContext {
        ShiftTranslationContext {
            session_id: openless_core::SessionId::new(),
            preferences_revision: 1,
            tap_count: count,
            held_recording_key: None,
        }
    }
    fn edge(
        g: &mut ShiftTranslationGesture,
        c: Option<ShiftTranslationContext>,
        key: u16,
        down: bool,
        ms: u64,
        epoch: Instant,
    ) -> Option<ShiftTranslationContext> {
        g.observe(
            c,
            key,
            key == 56 || key == 60,
            down,
            false,
            epoch + Duration::from_millis(ms),
        )
    }
    #[test]
    fn complete_taps_toggle_both_directions_with_the_same_count() {
        for count in [1, 2, 3, 255] {
            let c = Some(context(count));
            let mut g = ShiftTranslationGesture::default();
            let epoch = Instant::now();
            let mut toggles = 0;
            for index in 0..u64::from(count) * 2 {
                let key = if index % 2 == 0 { 56 } else { 60 };
                assert_eq!(edge(&mut g, c, key, true, index * 200, epoch), None);
                if edge(&mut g, c, key, false, index * 200 + 100, epoch).is_some() {
                    toggles += 1;
                }
                assert_eq!(toggles, (index + 1) / u64::from(count));
            }
            assert_eq!(toggles, 2);
        }
    }
    #[test]
    fn combinations_repeat_hold_mouse_and_both_sides_do_not_toggle() {
        let epoch = Instant::now();
        let c = Some(context(1));
        for companion in [56, 60, 59, 10, 500] {
            let mut g = ShiftTranslationGesture::default();
            edge(&mut g, c, 56, true, 0, epoch);
            edge(&mut g, c, companion, true, 10, epoch);
            edge(&mut g, c, companion, false, 20, epoch);
            assert_eq!(edge(&mut g, c, 56, false, 100, epoch), None);
        }
        for companion in [59, 10, 500] {
            let mut g = ShiftTranslationGesture::default();
            edge(&mut g, c, companion, true, 0, epoch);
            edge(&mut g, c, 56, true, 10, epoch);
            edge(&mut g, c, companion, false, 20, epoch);
            assert_eq!(edge(&mut g, c, 56, false, 100, epoch), None);
        }
        let mut g = ShiftTranslationGesture::default();
        edge(&mut g, c, 56, true, 0, epoch);
        assert_eq!(edge(&mut g, c, 56, false, 351, epoch), None);
        assert_eq!(g.observe(c, 56, true, true, true, epoch), None);
        assert_eq!(edge(&mut g, c, 56, false, 100, epoch), None);
    }
    #[test]
    fn releasing_the_recording_controller_discards_a_partial_shift_tap() {
        let epoch = Instant::now();
        let c = Some(ShiftTranslationContext {
            held_recording_key: Some(63),
            ..context(1)
        });
        let mut gesture = ShiftTranslationGesture::default();
        edge(&mut gesture, None, 63, true, 0, epoch);
        edge(&mut gesture, c, 56, true, 10, epoch);
        assert_eq!(edge(&mut gesture, c, 63, false, 20, epoch), None);
        assert_eq!(edge(&mut gesture, c, 56, false, 30, epoch), None);
    }

    #[test]
    fn expired_sequences_settings_changes_and_session_changes_discard_partial_input() {
        let epoch = Instant::now();
        let c = context(2);
        let mut g = ShiftTranslationGesture::default();
        edge(&mut g, Some(c), 56, true, 0, epoch);
        edge(&mut g, Some(c), 56, false, 50, epoch);
        edge(&mut g, Some(c), 56, true, 451, epoch);
        assert_eq!(edge(&mut g, Some(c), 56, false, 500, epoch), None);
        let changed = ShiftTranslationContext {
            preferences_revision: 2,
            tap_count: 1,
            ..c
        };
        edge(&mut g, Some(c), 56, true, 600, epoch);
        assert_eq!(edge(&mut g, Some(changed), 56, false, 650, epoch), None);
        let next = Some(context(1));
        edge(&mut g, Some(changed), 56, true, 700, epoch);
        assert_eq!(edge(&mut g, next, 56, false, 750, epoch), None);
        edge(&mut g, None, 56, true, 800, epoch);
        assert_eq!(edge(&mut g, next, 56, false, 850, epoch), None);
        edge(&mut g, next, 56, true, 900, epoch);
        g.reset();
        assert_eq!(edge(&mut g, next, 56, false, 950, epoch), None);
        edge(&mut g, next, 56, true, 1000, epoch);
        assert_eq!(edge(&mut g, next, 56, false, 1050, epoch), next);
    }
}

pub type ShiftTranslationSource =
    std::sync::Arc<dyn Fn() -> Option<ShiftTranslationContext> + Send + Sync>;

const TAP_MAX: Duration = Duration::from_millis(350);
const TAP_GAP: Duration = Duration::from_millis(400);

#[derive(Default)]
pub struct ShiftTranslationGesture {
    context: Option<ShiftTranslationContext>,
    held: BTreeSet<u16>,
    press: Option<(u16, Instant)>,
    taps: u8,
    last_release: Option<Instant>,
}

impl ShiftTranslationGesture {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Every keyboard/button edge is observed, including events consumed by another shortcut.
    /// `other_modifiers` also rejects modifiers held before this listener was installed.
    pub fn observe(
        &mut self,
        context: Option<ShiftTranslationContext>,
        key: u16,
        shift: bool,
        down: bool,
        other_modifiers: bool,
        now: Instant,
    ) -> Option<ShiftTranslationContext> {
        if self.context != context {
            self.context = context;
            self.invalidate();
        }
        let controller = context.and_then(|value| value.held_recording_key);
        if down {
            let alone = self.held.iter().all(|held| Some(*held) == controller);
            let fresh = self.held.insert(key);
            if !shift || !alone || !fresh || other_modifiers || context.is_none() {
                self.invalidate();
                return None;
            }
            if self
                .last_release
                .is_some_and(|at| now.saturating_duration_since(at) > TAP_GAP)
            {
                self.taps = 0;
            }
            self.press = Some((key, now));
            return None;
        }
        self.held.remove(&key);
        if Some(key) == controller {
            self.invalidate();
            return None;
        }
        let Some((pressed_key, at)) = self.press else {
            return None;
        };
        if pressed_key != key {
            return None;
        }
        self.press = None;
        if !shift
            || other_modifiers
            || self.held.iter().any(|held| Some(*held) != controller)
            || now.saturating_duration_since(at) > TAP_MAX
        {
            self.invalidate();
            return None;
        }
        let context = context?;
        self.taps += 1;
        self.last_release = Some(now);
        if self.taps == context.tap_count {
            self.invalidate();
            Some(context)
        } else {
            None
        }
    }

    pub fn invalidate(&mut self) {
        self.press = None;
        self.taps = 0;
        self.last_release = None;
    }
}
