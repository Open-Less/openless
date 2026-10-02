use super::DocumentWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSpan {
    pub start: usize,
    pub len: usize,
    pub cursor_in_span: usize,
}

pub fn plan_window(len: usize, cursor: usize, budget: usize) -> WindowSpan {
    let cursor = cursor.min(len);
    if budget == 0 {
        return WindowSpan {
            start: cursor,
            len: 0,
            cursor_in_span: 0,
        };
    }
    let before = cursor.min(budget * 4 / 5);
    let after = (len - cursor).min(budget - before);
    let before = cursor.min(budget - after);
    WindowSpan {
        start: cursor - before,
        len: before + after,
        cursor_in_span: before,
    }
}

pub fn window_around_cursor(text: &str, cursor: usize, budget: usize) -> DocumentWindow {
    let span = plan_window(text.chars().count(), cursor, budget);
    DocumentWindow {
        text: text.chars().skip(span.start).take(span.len).collect(),
        cursor: span.cursor_in_span,
    }
}

/// Window for hosts that hand over the text already split at the caret (Android's
/// `InputConnection`), trimmed with the same budget rule as [`window_around_cursor`].
/// `None` when both sides are blank, so callers send no context at all.
pub fn window_from_split(before: &str, after: &str, budget: usize) -> Option<DocumentWindow> {
    if before.trim().is_empty() && after.trim().is_empty() {
        return None;
    }
    Some(window_around_cursor(
        &format!("{before}{after}"),
        before.chars().count(),
        budget,
    ))
}

pub fn utf16_offset_to_char_offset(text: &str, utf16_offset: usize) -> usize {
    let mut units = 0;
    for (index, character) in text.chars().enumerate() {
        if units >= utf16_offset {
            return index;
        }
        units += character.len_utf16();
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_text_keeps_the_caret_between_both_sides_and_respects_the_budget() {
        let window = window_from_split("前文🙂", "😀后文", 600).unwrap();
        assert_eq!(window.before(), "前文🙂");
        assert_eq!(window.after(), "😀后文");

        // Android reads up to 600 before + 200 after; the window still ends at 480/120.
        let window = window_from_split(&"前".repeat(600), &"后".repeat(200), 600).unwrap();
        assert_eq!(window.before().chars().count(), 480);
        assert_eq!(window.after().chars().count(), 120);

        assert_eq!(window_from_split("只有前文", "", 600).unwrap().after(), "");
        assert_eq!(window_from_split("", "只有后文", 600).unwrap().before(), "");
        assert!(window_from_split("", "", 600).is_none());
        assert!(window_from_split(" \n", "\t", 600).is_none());
    }
}
