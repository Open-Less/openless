//! Tracks active hold-to-talk trigger sources (keyboard / mouse middle / mouse side).
//!
//! Hold mode begins when the first source is pressed and ends when the last source is released.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSource {
    KeyboardDictation,
    MouseMiddle,
    MouseSide,
}

pub struct HoldSourceTracker {
    keyboard: AtomicBool,
    mouse_middle: AtomicBool,
    mouse_side: AtomicBool,
    active_count: AtomicU32,
}

impl HoldSourceTracker {
    pub fn new() -> Self {
        Self {
            keyboard: AtomicBool::new(false),
            mouse_middle: AtomicBool::new(false),
            mouse_side: AtomicBool::new(false),
            active_count: AtomicU32::new(0),
        }
    }

    pub fn reset(&self) {
        self.keyboard.store(false, Ordering::SeqCst);
        self.mouse_middle.store(false, Ordering::SeqCst);
        self.mouse_side.store(false, Ordering::SeqCst);
        self.active_count.store(0, Ordering::SeqCst);
    }

    /// Returns the active count **before** increment on a fresh press edge.
    /// Duplicate press edges for the same source return `None`.
    pub fn press(&self, source: TriggerSource) -> Option<u32> {
        let slot = self.slot(source);
        if slot.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some(self.active_count.fetch_add(1, Ordering::SeqCst))
    }

    /// Returns the remaining active source count after release.
    pub fn release(&self, source: TriggerSource) -> u32 {
        let slot = self.slot(source);
        if !slot.swap(false, Ordering::SeqCst) {
            return self.active_count.load(Ordering::SeqCst);
        }
        self.active_count.fetch_sub(1, Ordering::SeqCst);
        self.active_count.load(Ordering::SeqCst)
    }

    pub fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::SeqCst)
    }

    fn slot(&self, source: TriggerSource) -> &AtomicBool {
        match source {
            TriggerSource::KeyboardDictation => &self.keyboard,
            TriggerSource::MouseMiddle => &self.mouse_middle,
            TriggerSource::MouseSide => &self.mouse_side,
        }
    }
}

impl Default for HoldSourceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_source_count_tracks_multiple_sources() {
        let tracker = HoldSourceTracker::new();
        assert_eq!(tracker.press(TriggerSource::KeyboardDictation), Some(0));
        assert_eq!(tracker.active_count(), 1);
        assert_eq!(tracker.press(TriggerSource::KeyboardDictation), None);
        assert_eq!(tracker.active_count(), 1);

        assert_eq!(tracker.press(TriggerSource::MouseMiddle), Some(1));
        assert_eq!(tracker.active_count(), 2);

        assert_eq!(
            tracker.release(TriggerSource::KeyboardDictation),
            1
        );
        assert_eq!(tracker.release(TriggerSource::MouseMiddle), 0);
    }

    #[test]
    fn last_release_returns_zero() {
        let tracker = HoldSourceTracker::new();
        assert_eq!(tracker.press(TriggerSource::KeyboardDictation), Some(0));
        assert_eq!(tracker.release(TriggerSource::KeyboardDictation), 0);
        assert_eq!(tracker.active_count(), 0);
    }

    #[test]
    fn duplicate_release_is_no_op() {
        let tracker = HoldSourceTracker::new();
        assert_eq!(tracker.press(TriggerSource::MouseSide), Some(0));
        assert_eq!(tracker.release(TriggerSource::MouseSide), 0);
        assert_eq!(tracker.release(TriggerSource::MouseSide), 0);
        assert_eq!(tracker.active_count(), 0);
    }

    #[test]
    fn keyboard_and_mouse_hold_tracks_independently() {
        let tracker = HoldSourceTracker::new();
        assert_eq!(tracker.press(TriggerSource::KeyboardDictation), Some(0));
        assert_eq!(tracker.press(TriggerSource::MouseMiddle), Some(1));
        assert_eq!(tracker.active_count(), 2);
        assert_eq!(tracker.release(TriggerSource::KeyboardDictation), 1);
        assert_eq!(tracker.release(TriggerSource::MouseMiddle), 0);
    }
}
