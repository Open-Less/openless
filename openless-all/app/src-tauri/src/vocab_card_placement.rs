//! Pure vocabulary-card geometry and observation ownership; no native handles are dereferenced here.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn valid(self) -> bool {
        self.right >= self.left
            && self.bottom > self.top
            && self.right.checked_sub(self.left).is_some()
            && self.bottom.checked_sub(self.top).is_some()
    }

    pub fn from_xywh(x: f64, y: f64, width: f64, height: f64) -> Option<Self> {
        let values = [x, y, x + width, y + height];
        if width < 0.0
            || height <= 0.0
            || !values
                .iter()
                .all(|v| v.is_finite() && *v > i32::MIN as f64 && *v < i32::MAX as f64)
        {
            return None;
        }
        let rect = Self {
            left: x.floor() as i32,
            top: y.floor() as i32,
            right: (x + width).ceil() as i32,
            bottom: (y + height).ceil() as i32,
        };
        rect.valid().then_some(rect)
    }

    fn intersects(self, other: Self) -> bool {
        self.left < other.right
            && self.right >= other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AnchorSnapshot {
    pub window: usize,
    pub process_id: u32,
    pub window_rect: Rect,
    pub anchor: Option<Rect>,
    pub screen_rect: Option<Rect>,
    pub captured_at: Instant,
}

#[derive(Default)]
pub(crate) struct AnchorRegistry {
    active: Option<u64>,
    entries: Vec<(u64, AnchorSnapshot)>,
}

impl AnchorRegistry {
    pub fn begin(&mut self, id: u64) {
        self.active = Some(id);
    }

    pub fn update(&mut self, id: u64, snapshot: AnchorSnapshot) -> bool {
        if self.active != Some(id) {
            return false;
        }
        self.entries.retain(|(key, _)| *key != id);
        self.entries.push((id, snapshot));
        if self.entries.len() > 8 {
            self.entries.remove(0);
        }
        true
    }

    pub fn invalidate(&mut self, id: u64) {
        if let Some((_, entry)) = self.entries.iter_mut().find(|(key, _)| *key == id) {
            entry.anchor = None;
        }
    }

    pub fn stop(&mut self) {
        self.active = None;
        for (_, entry) in &mut self.entries {
            entry.anchor = None;
        }
    }

    pub fn retain(&mut self, ids: &[u64]) {
        let active = self.active;
        self.entries.retain(|(id, entry)| {
            (active == Some(*id) || ids.contains(id))
                && entry.captured_at.elapsed() <= Duration::from_secs(120)
        });
    }

    pub fn get(&self, id: u64) -> Option<AnchorSnapshot> {
        self.entries
            .iter()
            .find(|(key, entry)| {
                *key == id && entry.captured_at.elapsed() <= Duration::from_secs(120)
            })
            .map(|(_, entry)| {
                let mut snapshot = entry.clone();
                if snapshot.captured_at.elapsed() > Duration::from_secs(5) {
                    snapshot.anchor = None;
                }
                snapshot
            })
    }
}

/// Capacity-bounded handoff, deliberately independent of recording/inference duration.
pub(crate) struct TargetHandoff<K, T>(Vec<(K, T)>);

impl<K, T> Default for TargetHandoff<K, T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<K: PartialEq, T> TargetHandoff<K, T> {
    pub fn remember(&mut self, key: K, target: T) {
        self.0.retain(|(id, _)| *id != key);
        self.0.push((key, target));
        if self.0.len() > 4 {
            self.0.remove(0);
        }
    }

    pub fn take(&mut self, key: K) -> Option<T> {
        let index = self.0.iter().position(|(id, _)| *id == key)?;
        Some(self.0.remove(index).1)
    }
}

/// Only vocabulary-card work uses this epoch; ordinary capsule events keep their own ownership.
#[derive(Default)]
pub(crate) struct PresentationEpoch(AtomicU64);

impl PresentationEpoch {
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
    }

    pub fn request_hide(&self, visible: &AtomicBool) -> Option<u64> {
        let epoch = self.next();
        // Keep ownership until the main-thread cleanup actually runs. A second
        // hide must replace an obsolete cleanup, not cancel it and return early.
        visible.load(Ordering::SeqCst).then_some(epoch)
    }

    pub fn is_current(&self, epoch: u64) -> bool {
        self.0.load(Ordering::SeqCst) == epoch
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Screen {
    pub bounds: Rect,
    pub work: Rect,
    pub scale: f64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Placement {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

pub(crate) fn place_card(
    screen: Screen,
    anchor: Option<Rect>,
    logical_width: f64,
    logical_height: f64,
    logical_margin: f64,
) -> Option<Placement> {
    if !screen.bounds.valid() || screen.bounds.right == screen.bounds.left {
        return None;
    }
    let work = if screen.work.valid() && screen.work.right > screen.work.left {
        screen.work
    } else {
        screen.bounds
    };
    if !screen.scale.is_finite() || screen.scale <= 0.0 || screen.scale > 16.0 {
        return None;
    }
    let width = (logical_width * screen.scale).round();
    let height = (logical_height * screen.scale).round();
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return None;
    }
    let w = (width.min((work.right - work.left) as f64)) as i32;
    let h = (height.min((work.bottom - work.top) as f64)) as i32;
    let margin = (logical_margin.max(0.0) * screen.scale).round() as i32;
    let gap = (12.0 * screen.scale).round() as i32;
    let corner = (
        work.right.saturating_sub(w).saturating_sub(margin),
        work.bottom.saturating_sub(h).saturating_sub(margin),
    );
    let (x, y) = match anchor.filter(|rect| rect.valid() && screen.bounds.intersects(*rect)) {
        Some(rect) if rect.bottom.saturating_add(gap).saturating_add(h) <= work.bottom => {
            (rect.left, rect.bottom.saturating_add(gap))
        }
        Some(rect) if rect.top.saturating_sub(gap).saturating_sub(h) >= work.top => {
            (rect.left, rect.top.saturating_sub(gap).saturating_sub(h))
        }
        _ => corner,
    };
    Some(Placement {
        width: w as u32,
        height: h as u32,
        x: x.clamp(work.left, work.right.saturating_sub(w).max(work.left)),
        y: y.clamp(work.top, work.bottom.saturating_sub(h).max(work.top)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn screen() -> Screen {
        Screen {
            bounds: rect(-1920, 0, 0, 1080),
            work: rect(-1880, 0, 0, 1040),
            scale: 1.0,
        }
    }

    fn snapshot() -> AnchorSnapshot {
        AnchorSnapshot {
            window: 1,
            process_id: 2,
            window_rect: rect(-1800, 100, -100, 900),
            anchor: Some(rect(-1000, 300, -1000, 320)),
            screen_rect: Some(screen().bounds),
            captured_at: Instant::now(),
        }
    }

    #[test]
    fn zero_width_caret_is_valid_but_empty_or_nonfinite_geometry_is_not() {
        assert!(Rect::from_xywh(-100.0, 20.0, 0.0, 20.0).is_some());
        assert!(Rect::from_xywh(0.0, 0.0, 1.0, 0.0).is_none());
        assert!(Rect::from_xywh(f64::NAN, 0.0, 1.0, 20.0).is_none());
        assert!(Rect::from_xywh(0.0, 0.0, -1.0, 20.0).is_none());
    }

    #[test]
    fn places_below_caret_on_negative_coordinate_monitor() {
        let placement = place_card(screen(), snapshot().anchor, 320.0, 108.0, 24.0).unwrap();
        assert_eq!((placement.x, placement.y), (-1000, 332));
    }

    #[test]
    fn flips_above_input_near_bottom_and_clamps_right_edge() {
        let placement = place_card(
            screen(),
            Some(rect(-100, 950, -80, 980)),
            320.0,
            108.0,
            24.0,
        )
        .unwrap();
        assert_eq!((placement.x, placement.y), (-320, 830));
    }

    #[test]
    fn fallback_corner_uses_work_area_not_monitor_bottom() {
        let placement = place_card(screen(), None, 320.0, 108.0, 24.0).unwrap();
        assert_eq!((placement.x, placement.y), (-344, 908));
    }

    #[test]
    fn scales_size_and_gap_by_the_target_monitor_only() {
        for scale in [1.25, 1.5, 2.0] {
            let monitor = Screen { scale, ..screen() };
            let placement = place_card(monitor, snapshot().anchor, 320.0, 108.0, 24.0).unwrap();
            assert_eq!(placement.width, (320.0 * scale).round() as u32);
            assert_eq!(placement.y, 320 + (12.0 * scale).round() as i32);
        }
    }

    #[test]
    fn offscreen_anchor_and_invalid_work_area_degrade_safely() {
        let monitor = Screen {
            work: rect(0, 0, 0, 0),
            ..screen()
        };
        let a = place_card(monitor, Some(rect(100, 100, 101, 120)), 320.0, 108.0, 24.0);
        let b = place_card(monitor, None, 320.0, 108.0, 24.0);
        assert_eq!(a, b);
    }

    #[test]
    fn card_larger_than_work_area_keeps_all_actions_on_screen() {
        let monitor = Screen {
            work: rect(-100, 0, 0, 100),
            ..screen()
        };
        let p = place_card(monitor, None, 320.0, 252.0, 24.0).unwrap();
        assert_eq!((p.width, p.height, p.x, p.y), (100, 100, -100, 0));
    }

    #[test]
    fn stopped_or_previous_observer_cannot_overwrite_new_anchor() {
        let mut registry = AnchorRegistry::default();
        registry.begin(1);
        assert!(registry.update(1, snapshot()));
        registry.stop();
        assert!(!registry.update(1, snapshot()));
        registry.begin(2);
        assert!(registry.update(2, snapshot()));
        assert!(!registry.update(1, snapshot()));
        registry.invalidate(1);
        assert!(registry.get(2).unwrap().anchor.is_some());
    }

    #[test]
    fn stopping_retains_original_screen_but_revokes_near_input_placement() {
        let mut registry = AnchorRegistry::default();
        registry.begin(1);
        registry.update(1, snapshot());
        registry.stop();
        let stored = registry.get(1).unwrap();
        assert!(stored.anchor.is_none());
        assert_eq!(stored.screen_rect, Some(screen().bounds));
    }

    #[test]
    fn resolving_one_item_does_not_drop_other_items_from_the_same_observation() {
        let mut registry = AnchorRegistry::default();
        registry.begin(1);
        registry.update(1, snapshot());
        registry.begin(2);
        registry.retain(&[1]);
        assert!(registry.get(1).is_some());
        registry.retain(&[]);
        assert!(registry.get(1).is_none());
    }

    #[test]
    fn old_snapshot_falls_back_to_original_screen() {
        let mut registry = AnchorRegistry::default();
        registry.begin(1);
        let mut capture = snapshot();
        capture.captured_at -= Duration::from_secs(6);
        registry.update(1, capture);
        assert!(registry.get(1).unwrap().anchor.is_none());
    }

    #[test]
    fn queued_show_and_hide_are_obsolete_even_after_a_new_session_returns_idle() {
        let epoch = PresentationEpoch::default();
        let old_show = epoch.next();
        let old_hide = epoch.next();
        let next_session = epoch.next();
        assert!(!epoch.is_current(old_show));
        assert!(!epoch.is_current(old_hide));
        assert!(epoch.is_current(next_session));
    }
    #[test]
    fn duplicate_hide_keeps_a_live_cleanup_until_the_window_is_restored() {
        let epoch = PresentationEpoch::default();
        let visible = AtomicBool::new(true);
        let first = epoch.request_hide(&visible).unwrap();
        let second = epoch.request_hide(&visible).unwrap();
        assert!(!epoch.is_current(first));
        assert!(epoch.is_current(second));
        assert!(visible.load(Ordering::SeqCst));
        visible.store(false, Ordering::SeqCst);
        assert!(epoch.request_hide(&visible).is_none());
    }

    #[test]
    fn hiding_before_show_still_cancels_the_queued_show() {
        let epoch = PresentationEpoch::default();
        let show = epoch.next();
        assert!(epoch.request_hide(&AtomicBool::new(false)).is_none());
        assert!(!epoch.is_current(show));
    }

    #[test]
    fn target_handoff_is_consumed_by_session_not_by_elapsed_time() {
        let mut targets = TargetHandoff::default();
        targets.remember(1, "original target");
        // No wall-clock expiry: long recording/polishing cannot evict the active session.
        assert_eq!(targets.take(1), Some("original target"));
        assert_eq!(targets.take(1), None);
        let mut targets = TargetHandoff::default();
        for session in 1..=5 {
            targets.remember(session, session);
        }
        assert_eq!(targets.take(1), None);
        assert_eq!(targets.take(5), Some(5));
    }
}
