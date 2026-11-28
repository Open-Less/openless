//! Validate an existing native anchor without treating another capture as invalidation.
pub(crate) fn unchanged(captured: Option<(u64, u64)>, current: Option<(u64, u64)>) -> bool {
    captured.is_some() && captured == current
}
/// HWND equality is insufficient for two web/editor fields in one native window.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn same_control(captured: Option<&[i32]>, current: Option<&[i32]>) -> bool {
    captured.is_some_and(|identity| !identity.is_empty() && Some(identity) == current)
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
    fn same_app_and_window_still_require_the_original_uia_control() {
        let original = [42, 7, 101];
        let other_control_in_same_window = [42, 7, 102];
        assert!(same_control(Some(&original), Some(&original)));
        assert!(!same_control(
            Some(&original),
            Some(&other_control_in_same_window)
        ));
        assert!(!same_control(Some(&original), None));
        assert!(!same_control(None, None));
        assert!(!same_control(Some(&[]), Some(&[])));
    }
    #[test]
    fn a_new_capture_is_not_a_position_change() {
        assert!(unchanged(Some((4, 0)), Some((4, 0))));
        assert!(!unchanged(Some((4, 0)), Some((5, 0))));
        assert!(!unchanged(Some((4, 0)), Some((4, 2))));
        assert!(
            !unchanged(Some((4, 7)), Some((30, 7))),
            "an identical selected string at another occurrence is a different range"
        );
        assert!(!unchanged(None, None));
    }
}
