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

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows UIA 光标上下文方案（`windows_cursor_context开发方案.md` §6）的验收用例:
    /// budget=600 时 before<=480、after<=120，且 before+after<=600。
    #[test]
    fn budget_splits_roughly_eighty_twenty() {
        let long = "字".repeat(2000);
        let span = plan_window(long.chars().count(), 1000, 600);
        assert!(span.cursor_in_span <= 480, "before 不应超过预算的 80%");
        assert!(span.len - span.cursor_in_span <= 120, "after 不应超过预算的 20%");
        assert!(span.len <= 600);
    }

    /// 左侧文本不足时，剩余预算应让给右侧（方案 §6 "如果左侧不足 480 字，可以把剩余预算让给右侧"）。
    #[test]
    fn insufficient_left_text_gives_remaining_budget_to_the_right() {
        let text = "字".repeat(1000);
        // cursor 只有 10 个字在左边，右边还有 990 个字可读。
        let span = plan_window(text.chars().count(), 10, 600);
        assert_eq!(span.cursor_in_span, 10, "左边全部给出，不应凭空截断");
        assert_eq!(span.len, 600, "右侧应吃满剩余预算以补满 600");
    }

    /// 右侧文本不足时，剩余预算应让给左侧。
    #[test]
    fn insufficient_right_text_gives_remaining_budget_to_the_left() {
        let text = "字".repeat(1000);
        // cursor 在第 990 个字，右边只剩 10 个字。
        let span = plan_window(text.chars().count(), 990, 600);
        assert_eq!(span.len - span.cursor_in_span, 10, "右边全部给出");
        assert_eq!(span.len, 600, "左侧应补满剩余预算到 600");
    }

    /// emoji / surrogate pair 不应被按 UTF-16 长度误判为 2 个 char 而切坏字符。
    #[test]
    fn window_around_cursor_does_not_split_emoji_or_surrogate_pairs() {
        let text = "前文🙂😀后文";
        // 光标紧跟在两个 emoji 之后（按 char 计数，不按 UTF-16 code unit 计数）。
        let cursor_char = "前文🙂😀".chars().count();
        let window = window_around_cursor(text, cursor_char, 600);
        assert_eq!(window.before(), "前文🙂😀");
        assert_eq!(window.after(), "后文");
        // 两个 emoji 字符仍然完整，没有产生孤立的 surrogate / 替换字符。
        assert!(window.text.chars().all(|c| c != '\u{FFFD}'));
    }

    #[test]
    fn zero_budget_yields_an_empty_window_at_the_cursor() {
        let span = plan_window(100, 50, 0);
        assert_eq!(span.len, 0);
        assert_eq!(span.cursor_in_span, 0);
    }
}
