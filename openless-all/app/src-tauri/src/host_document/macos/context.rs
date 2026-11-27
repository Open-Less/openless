//! Bounded reference sides and complete original selection from the P4 owner.

use super::*;
use openless_core::host_document::CursorContext;

pub(crate) fn capture(
    target: &crate::selection::SelectionInsertionTarget,
    expected: (u64, u64),
    budget: usize,
) -> Option<CursorContext> {
    with_frozen_target_read(target, |lease| unsafe {
        let (control, _) = lease.ax_objects();
        let control = control.cast();
        let start = usize::try_from(expected.0).map_err(|_| "range unavailable")?;
        let length = usize::try_from(expected.1).map_err(|_| "range unavailable")?;
        let range = copy_selected_range(control).ok_or("range unavailable")?;
        if lease.range() != Some(expected)
            || range.location != isize::try_from(start).map_err(|_| "range unavailable")?
            || range.length != isize::try_from(length).map_err(|_| "range unavailable")?
        {
            return Err("range changed".into());
        }
        let total =
            copy_index_attr(control, b"AXNumberOfCharacters\0").ok_or("length unavailable")?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= total)
            .ok_or("range unavailable")?;
        let side = budget.min(super::super::MAX_BUDGET_CHARS) / 2;
        // Native units only reach AX range APIs. Rust trimming uses characters.
        let before_start = start.saturating_sub(side.saturating_mul(2));
        let before = read_exact(control, before_start, start - before_start)?;
        let after = read_exact(control, end, (total - end).min(side.saturating_mul(2)))?;
        let selection = read_exact(control, start, length)?;
        let before: String = before
            .chars()
            .rev()
            .take(side)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let after: String = after.chars().take(side).collect();
        if copy_selected_range(control) != Some(range)
            || copy_index_attr(control, b"AXNumberOfCharacters\0") != Some(total)
            || [&before, &after, &selection]
                .iter()
                .any(|text| openless_core::expression_observation::contains_sensitive_content(text))
        {
            return Err("document changed or protected".into());
        }
        Ok(CursorContext {
            before,
            after,
            selection,
        })
    })
    .ok()
}

pub(super) unsafe fn read_exact(
    control: AxUiElementRef,
    start: usize,
    length: usize,
) -> Result<String, String> {
    start
        .checked_add(length)
        .and_then(|end| isize::try_from(end).ok())
        .ok_or("range unavailable")?;
    if length == 0 {
        return Ok(String::new());
    }
    let value = copy_string_for_range(control, start, length).ok_or("range unavailable")?;
    if value.encode_utf16().count() != length {
        return Err("incomplete range".into());
    }
    Ok(value)
}
