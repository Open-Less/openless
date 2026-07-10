//! LLM polish output sanitization extracted from `polish.rs`
//! (behavior-preserving move).
//!
//! Strips model `<think>` blocks, markdown fences, and known boilerplate
//! prefixes. `clean_polish_output` stays `pub(crate)` (also used by `llm_gemini`)
//! and is re-exported from `polish`.

use std::borrow::Cow;

/// Best-effort cleanup of common LLM "introduction" prefixes and markdown fences.
///
/// Matches a small set of known leading phrases (`根据您给的内容...`, `整理如下...`, etc.)
/// and strips them. We don't have the `regex` crate, so we use prefix checks plus
/// an iterative trim — if the model stacks two boilerplate sentences we'll still
/// strip both.
///
/// `pub(crate)` because `llm_gemini` 也要在它自己的解析路径上跑同一套清洗，
/// 否则 polish prompt 已经禁用的"以下是整理后的内容"前缀只在 OpenAI 兼容路径生效。
pub(crate) fn clean_polish_output(content: &str) -> String {
    let without_thinking = strip_thinking_blocks(content);
    let trimmed = without_thinking.trim();
    let stripped = strip_markdown_fence(trimmed);
    let mut output = stripped.to_string();

    loop {
        let before_len = output.len();
        output = strip_leading_boilerplate(&output).to_string();
        output = output.trim_start().to_string();
        if output.len() == before_len {
            break;
        }
    }

    output.trim().to_string()
}

/// Strip model reasoning blocks so only the final polished text is inserted.
///
/// Thinking-capable OpenAI-compatible models commonly return their reasoning in
/// `<think>...</think>` before the final answer. Match only explicit `think`
/// tags, with optional attributes and ASCII casing variants, so normal prose is
/// left untouched.
pub(super) fn strip_thinking_blocks(text: &str) -> Cow<'_, str> {
    let mut cursor = 0;
    let mut output: Option<String> = None;

    while let Some((open_start, open_end)) = find_think_open(&text[cursor..]) {
        let open_start = cursor + open_start;
        let open_end = cursor + open_end;
        let Some((_, close_end)) = find_think_close(&text[open_end..]) else {
            break;
        };
        let close_end = open_end + close_end;

        output
            .get_or_insert_with(|| String::with_capacity(text.len()))
            .push_str(&text[cursor..open_start]);
        cursor = close_end;
    }

    match output {
        Some(mut output) => {
            output.push_str(&text[cursor..]);
            Cow::Owned(output)
        }
        None => Cow::Borrowed(text),
    }
}

pub(super) fn find_think_open(text: &str) -> Option<(usize, usize)> {
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find('<') {
        let start = cursor + offset;
        if let Some(end) = parse_think_open_at(text, start) {
            return Some((start, end));
        }
        cursor = start + '<'.len_utf8();
    }
    None
}

pub(super) fn find_think_close(text: &str) -> Option<(usize, usize)> {
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find('<') {
        let start = cursor + offset;
        if let Some(end) = parse_think_close_at(text, start) {
            return Some((start, end));
        }
        cursor = start + '<'.len_utf8();
    }
    None
}

pub(super) fn parse_think_open_at(text: &str, start: usize) -> Option<usize> {
    let tag_start = start + '<'.len_utf8();
    if text.as_bytes().get(tag_start) == Some(&b'/') {
        return None;
    }
    parse_think_tag_end(text, tag_start, true)
}

pub(super) fn parse_think_close_at(text: &str, start: usize) -> Option<usize> {
    let slash = start + '<'.len_utf8();
    if text.as_bytes().get(slash) != Some(&b'/') {
        return None;
    }
    parse_think_tag_end(text, slash + '/'.len_utf8(), false)
}

pub(super) fn parse_think_tag_end(text: &str, tag_start: usize, allow_attributes: bool) -> Option<usize> {
    let tag_end = tag_start.checked_add("think".len())?;
    if tag_end > text.len() || !text[tag_start..tag_end].eq_ignore_ascii_case("think") {
        return None;
    }

    let next = text.as_bytes().get(tag_end).copied()?;
    if next == b'>' {
        return Some(tag_end + 1);
    }
    if !next.is_ascii_whitespace() {
        return None;
    }

    if allow_attributes {
        return text[tag_end..].find('>').map(|offset| tag_end + offset + 1);
    }

    let suffix = &text[tag_end..];
    let trimmed = suffix.trim_start_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.starts_with('>') {
        Some(text.len() - trimmed.len() + 1)
    } else {
        None
    }
}

pub(super) fn strip_markdown_fence(text: &str) -> &str {
    if !(text.starts_with("```") && text.ends_with("```")) {
        return text;
    }
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.len() < 2 {
        return text;
    }
    lines.remove(0);
    lines.pop();
    // Re-borrow as &str by stitching is impossible without alloc; fallback to
    // returning the original slice if the cheap path can't strip.
    // Find the byte offsets of the first newline and the last fence to slice in place.
    let after_first_line = match text.find('\n') {
        Some(i) => i + 1,
        None => return text,
    };
    let before_last_fence = match text.rfind("```") {
        Some(i) => i,
        None => return text,
    };
    if before_last_fence <= after_first_line {
        return text;
    }
    text[after_first_line..before_last_fence].trim_matches(['\n', ' ', '\t', '\r'].as_ref())
}

/// Known introduction phrases that some models prepend even when prompted not to.
pub(super) const LEADING_BOILERPLATE_PREFIXES: &[&str] = &[
    "根据您给的内容",
    "根据您提供的内容",
    "根据你给的内容",
    "根据你提供的内容",
    "以下是整理后的内容",
    "以下是优化后的内容",
    "以下为整理后的内容",
    "以下是结构化整理后的内容",
    "我整理如下",
    "我已整理如下",
    "整理如下",
    "优化如下",
    "结构化整理如下",
];

pub(super) const BOILERPLATE_END_CHARS: &[char] = &['。', '：', ':', '，', ',', '\n'];

pub(super) fn strip_leading_boilerplate(text: &str) -> &str {
    for prefix in LEADING_BOILERPLATE_PREFIXES {
        if let Some(after_prefix) = text.strip_prefix(prefix) {
            // Trim characters after the prefix up to (and including) the first
            // sentence-ending punctuation or newline.
            for (idx, c) in after_prefix.char_indices() {
                if BOILERPLATE_END_CHARS.contains(&c) {
                    let cut = prefix.len() + idx + c.len_utf8();
                    return &text[cut..];
                }
            }
            // No terminator: drop the prefix only.
            return after_prefix;
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_polish_output_strips_think_tag_block() {
        let content =
            "<think>先分析用户意图。\n这里可能很长。</think>\n\n请明天上午十点提醒我开会。";

        assert_eq!(clean_polish_output(content), "请明天上午十点提醒我开会。");
    }

    #[test]
    fn clean_polish_output_strips_think_tag_with_attributes_and_case() {
        let content = r#"<THINK reason="true">hidden</THINK>
最终文本。"#;

        assert_eq!(clean_polish_output(content), "最终文本。");
    }

    #[test]
    fn clean_polish_output_strips_multiple_think_blocks() {
        let content = "<think>one</think>第一句。<think>two</think>第二句。";

        assert_eq!(clean_polish_output(content), "第一句。第二句。");
    }

    #[test]
    fn strip_thinking_blocks_ignores_non_think_and_unclosed_tags() {
        assert!(matches!(
            strip_thinking_blocks("普通文本"),
            Cow::Borrowed(_)
        ));
        assert_eq!(
            strip_thinking_blocks("<thinking>保留</thinking>正文"),
            "<thinking>保留</thinking>正文"
        );
        assert_eq!(
            strip_thinking_blocks("<think>未闭合正文"),
            "<think>未闭合正文"
        );
    }
}
