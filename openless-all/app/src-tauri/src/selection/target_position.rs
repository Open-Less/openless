//! Validate an existing native anchor without treating another capture as invalidation.
pub(crate) fn unchanged(captured: Option<(u64, u64)>, current: Option<(u64, u64)>) -> bool {
    captured.is_some() && captured == current
}
pub(crate) fn advanced(
    captured: Option<(u64, u64)>,
    utf16_units: u64,
    observed: Option<(u64, u64)>,
) -> Option<(u64, u64)> {
    let expected = (captured?.0.checked_add(utf16_units)?, 0);
    (observed == Some(expected)).then_some(expected)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_new_capture_is_not_a_position_change() {
        assert!(unchanged(Some((4, 0)), Some((4, 0))));
        assert!(!unchanged(Some((4, 0)), Some((5, 0))));
        assert!(!unchanged(Some((4, 0)), Some((4, 2))));
        assert!(!unchanged(Some((4, 7)), Some((30, 7))), "an identical selected string at another occurrence is a different range");
        assert!(!unchanged(None, None));
    }
}
