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
