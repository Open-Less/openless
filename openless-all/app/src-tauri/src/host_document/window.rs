//! 光标窗口算法 —— 纯函数，无平台依赖。
//!
//! 宿主文档可能有几万字，但送给 LLM 的预算只有几百字。「截哪一段」的答案是
//! **以光标为锚、上文 80% / 下文 20%**：用户正在写的位置，上文是已经定稿的语境
//! （人名、术语、前半句），下文往往是空的或者是待改的残句，参考价值低得多。
//!
//! 一侧吃不满预算时把余额让给另一侧 —— 光标在文档开头（上文只有 3 个字）时不该
//! 白白浪费 80% 的额度。
//!
//! **一切按 char 计数，不按字节**（对齐 `selection.rs` 的 `truncate_selection`）。
//! 按字节切会把 CJK 字符劈成半个，送进 prompt 就是乱码。

use super::DocumentWindow;

/// 上文占预算的比例（4/5 = 80%）。用整数比而非浮点，避免 `as usize` 的截断歧义。
const BEFORE_RATIO_NUM: usize = 4;
const BEFORE_RATIO_DEN: usize = 5;

/// 窗口在原文中的位置，全部以「元素个数」计（char 或 UTF-16 code unit，由调用方决定）。
///
/// 之所以把「算范围」和「切字符串」分成两步：macOS 上大文档不能整篇读回来，得先算出
/// 一个 UTF-16 范围交给 `AXStringForRange` 去取。那条路径只需要 `plan_window`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSpan {
    /// 窗口起点在原文中的下标。
    pub start: usize,
    /// 窗口长度。
    pub len: usize,
    /// 光标相对窗口起点的偏移（即窗口内的上文长度）。
    pub cursor_in_span: usize,
}

/// 给定原文长度、光标位置和预算，算出该截取的范围。
///
/// `cursor` 会先 clamp 到 `[0, len]` —— AX 返回的选区下标不保证和我们刚读到的正文
/// 同步（用户可能在两次调用之间敲了退格），越界了就贴到边上，不要 panic。
pub fn plan_window(len: usize, cursor: usize, budget: usize) -> WindowSpan {
    let cursor = cursor.min(len);
    if budget == 0 {
        return WindowSpan {
            start: cursor,
            len: 0,
            cursor_in_span: 0,
        };
    }

    // 1) 上文先按 80% 配额取，取不满就取多少算多少。
    let before = cursor.min(budget * BEFORE_RATIO_NUM / BEFORE_RATIO_DEN);
    // 2) 下文吃掉剩下的全部预算（上文没吃满的部分自动流到这里）。
    let after = (len - cursor).min(budget - before);
    // 3) 下文也没吃满的话，余额再还给上文 —— 光标在文末时上文能拿满 100%。
    let before = cursor.min(budget - after);

    WindowSpan {
        start: cursor - before,
        len: before + after,
        cursor_in_span: before,
    }
}

/// 按 char 在 `text` 上截出光标窗口。`cursor` 是 char 下标。
pub fn window_around_cursor(text: &str, cursor: usize, budget: usize) -> DocumentWindow {
    let len = text.chars().count();
    let span = plan_window(len, cursor, budget);
    let windowed: String = text.chars().skip(span.start).take(span.len).collect();
    DocumentWindow {
        text: windowed,
        cursor: span.cursor_in_span,
    }
}

/// UTF-16 下标 → char 下标。
///
/// AX 的所有下标（`AXSelectedTextRange` / `AXStringForRange` / `AXNumberOfCharacters`）
/// 都是 UTF-16 code unit 计数，而我们的窗口算法按 char 走。中文在 UTF-16 里是 1 个
/// 单元、emoji 是 2 个，两套坐标对不上，必须显式换算。
///
/// 越界时返回末尾 —— 同样是「AX 下标可能比正文新」的防御。
pub fn utf16_offset_to_char_offset(text: &str, utf16_offset: usize) -> usize {
    let mut seen = 0usize;
    for (char_idx, ch) in text.chars().enumerate() {
        if seen >= utf16_offset {
            return char_idx;
        }
        seen += ch.len_utf16();
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: usize = 100;

    #[test]
    fn cursor_in_the_middle_splits_80_20() {
        let span = plan_window(1000, 500, BUDGET);
        assert_eq!(
            span,
            WindowSpan {
                start: 420,
                len: 100,
                cursor_in_span: 80,
            }
        );
    }

    #[test]
    fn cursor_at_start_gives_all_budget_to_the_tail() {
        let span = plan_window(1000, 0, BUDGET);
        assert_eq!(
            span,
            WindowSpan {
                start: 0,
                len: 100,
                cursor_in_span: 0,
            }
        );
    }

    #[test]
    fn cursor_at_end_gives_all_budget_to_the_head() {
        let span = plan_window(1000, 1000, BUDGET);
        assert_eq!(
            span,
            WindowSpan {
                start: 900,
                len: 100,
                cursor_in_span: 100,
            }
        );
    }

    #[test]
    fn short_head_donates_its_leftover_to_the_tail() {
        // 上文只有 10 个字，80 的配额用不掉 70 —— 那 70 应该流给下文，总量仍是 100。
        let span = plan_window(1000, 10, BUDGET);
        assert_eq!(
            span,
            WindowSpan {
                start: 0,
                len: 100,
                cursor_in_span: 10,
            }
        );
    }

    #[test]
    fn short_tail_donates_its_leftover_back_to_the_head() {
        // 下文只有 5 个字，20 的配额用不掉 15 —— 上文应该拿到 95 而不是死守 80。
        let span = plan_window(1000, 995, BUDGET);
        assert_eq!(
            span,
            WindowSpan {
                start: 900,
                len: 100,
                cursor_in_span: 95,
            }
        );
    }

    #[test]
    fn whole_document_shorter_than_budget_is_taken_verbatim() {
        let span = plan_window(50, 25, BUDGET);
        assert_eq!(
            span,
            WindowSpan {
                start: 0,
                len: 50,
                cursor_in_span: 25,
            }
        );
    }

    #[test]
    fn empty_document_yields_empty_span() {
        assert_eq!(
            plan_window(0, 0, BUDGET),
            WindowSpan {
                start: 0,
                len: 0,
                cursor_in_span: 0,
            }
        );
    }

    #[test]
    fn zero_budget_yields_empty_span_anchored_at_the_cursor() {
        assert_eq!(
            plan_window(1000, 500, 0),
            WindowSpan {
                start: 500,
                len: 0,
                cursor_in_span: 0,
            }
        );
    }

    #[test]
    fn cursor_past_the_end_is_clamped_instead_of_panicking() {
        // AX 给的下标可能比我们读到的正文新一步，越界不能 panic。
        let span = plan_window(10, 999, BUDGET);
        assert_eq!(
            span,
            WindowSpan {
                start: 0,
                len: 10,
                cursor_in_span: 10,
            }
        );
    }

    #[test]
    fn windowing_slices_cjk_on_char_boundaries() {
        // 每个汉字 3 字节 —— 按字节切会切出无效 UTF-8，这里必须按 char。
        let text: String = "上下文测试".repeat(100); // 500 个汉字
        let win = window_around_cursor(&text, 250, 10);
        assert_eq!(win.text.chars().count(), 10);
        assert_eq!(win.cursor, 8);
        // 窗口正文必须能在原文里原样找到（证明没有切坏字符）。
        assert!(text.contains(&win.text));
    }

    #[test]
    fn windowing_keeps_the_cursor_pointing_at_the_same_spot() {
        let text = "abcdefghij";
        let win = window_around_cursor(text, 5, 4);
        // 预算 4：上文 3（80% 向下取整）、下文 1。
        assert_eq!(win.text, "cdef");
        assert_eq!(win.cursor, 3);
        // 窗口内 cursor 之前的内容 == 原文 cursor 之前的内容的尾巴。
        assert!(text[..5].ends_with(&win.text[..win.cursor]));
    }

    #[test]
    fn windowing_a_short_document_returns_it_whole() {
        let win = window_around_cursor("hi", 1, BUDGET);
        assert_eq!(win.text, "hi");
        assert_eq!(win.cursor, 1);
    }

    #[test]
    fn windowing_empty_text_is_empty() {
        let win = window_around_cursor("", 0, BUDGET);
        assert_eq!(win.text, "");
        assert_eq!(win.cursor, 0);
    }

    #[test]
    fn utf16_offset_maps_to_char_offset_for_ascii() {
        assert_eq!(utf16_offset_to_char_offset("hello", 0), 0);
        assert_eq!(utf16_offset_to_char_offset("hello", 3), 3);
        assert_eq!(utf16_offset_to_char_offset("hello", 5), 5);
    }

    #[test]
    fn utf16_offset_maps_to_char_offset_for_cjk() {
        // CJK 在 UTF-16 里是 1 个单元，和 char 一一对应。
        assert_eq!(utf16_offset_to_char_offset("你好世界", 2), 2);
    }

    #[test]
    fn utf16_offset_accounts_for_surrogate_pairs() {
        // emoji 占 2 个 UTF-16 单元：UTF-16 下标 2 对应 char 下标 1。
        let text = "🍎🍊ab";
        assert_eq!(utf16_offset_to_char_offset(text, 0), 0);
        assert_eq!(utf16_offset_to_char_offset(text, 2), 1);
        assert_eq!(utf16_offset_to_char_offset(text, 4), 2);
        assert_eq!(utf16_offset_to_char_offset(text, 5), 3);
    }

    #[test]
    fn utf16_offset_past_the_end_clamps_to_the_last_char() {
        assert_eq!(utf16_offset_to_char_offset("abc", 99), 3);
    }

    #[test]
    fn utf16_offset_landing_inside_a_surrogate_pair_rounds_up_to_a_boundary() {
        // 下标 1 落在 🍎 的低位代理上 —— 没有对应的 char 边界，向后取整到下一个，
        // 绝不返回「半个字符」的位置。
        assert_eq!(utf16_offset_to_char_offset("🍎b", 1), 1);
    }
}
