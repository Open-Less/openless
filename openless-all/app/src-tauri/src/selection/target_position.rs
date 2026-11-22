//! Numeric target-range validation; no text reads or anchor reconstruction.

pub(crate) fn unchanged(
    captured_generation: u64,
    captured_range: Option<(u64, u64)>,
    current_generation: u64,
    current_range: Option<(u64, u64)>,
) -> bool {
    captured_generation == current_generation
        && captured_range.is_some()
        && captured_range == current_range
}

#[cfg(test)]
mod tests {
    use super::unchanged;

    #[test]
    fn unchanged_control_is_insufficient_after_caret_selection_or_capture_changes() {
        assert!(unchanged(1, Some((4, 0)), 1, Some((4, 0))));
        assert!(!unchanged(1, Some((4, 0)), 1, Some((12, 0))));
        assert!(!unchanged(1, Some((4, 0)), 1, Some((4, 6))));
        assert!(!unchanged(1, Some((4, 0)), 2, Some((4, 0))));
        assert!(!unchanged(1, None, 1, None));
        assert!(!unchanged(1, Some((4, 0)), 1, None));
    }
}
