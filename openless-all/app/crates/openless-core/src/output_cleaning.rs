//! LLM polish output sanitization extracted from `polish.rs`
//! (behavior-preserving move).
//!
//! Strips model `<think>` blocks, markdown fences, and known boilerplate
//! prefixes. `clean_polish_output` stays `pub(crate)` (also used by `llm_gemini`)
//! and is re-exported from `polish`.

use std::borrow::Cow;

pub fn clean_polish_output(content: &str) -> String {
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

/// XML structured-output cleaning: strips thinking blocks, keeps the edit_plan
/// envelope.
pub fn clean_xml_llm_output(content: &str) -> String {
    let without_thinking = strip_thinking_blocks(content);
    let trimmed = without_thinking.trim();
    if let Some(start) = find_ci_tag_open(trimmed, "edit_plan") {
        let close = "</edit_plan>";
        if let Some(close_rel) = find_ci_substr(&trimmed[start..], close) {
            let end = start + close_rel + close.len();
            return trimmed[start..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn find_ci_tag_open(content: &str, tag: &str) -> Option<usize> {
    find_ci_substr(content, &format!("<{tag}"))
}

fn find_ci_substr(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    if hb.len() < nb.len() {
        return None;
    }
    for i in 0..=hb.len() - nb.len() {
        if hb[i..]
            .iter()
            .zip(nb.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
        {
            return Some(i);
        }
    }
    None
}

/// JSON structured-output cleaning: strips only thinking blocks and markdown fences,
/// leaving boilerplate prefixes alone.
pub fn clean_json_llm_output(content: &str) -> String {
    let without_thinking = strip_thinking_blocks(content);
    let trimmed = without_thinking.trim();
    strip_markdown_fence(trimmed).trim().to_string()
}

/// Strip model reasoning blocks so only the final polished text is inserted.
///
/// Thinking-capable OpenAI-compatible models commonly return their reasoning in
/// `<think>...</think>` before the final answer. Match only explicit `think`
/// tags, with optional attributes and ASCII casing variants, so normal prose is
/// left untouched.
fn strip_thinking_blocks(text: &str) -> Cow<'_, str> {
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

fn find_think_open(text: &str) -> Option<(usize, usize)> {
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

fn find_think_close(text: &str) -> Option<(usize, usize)> {
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

fn parse_think_open_at(text: &str, start: usize) -> Option<usize> {
    let tag_start = start + '<'.len_utf8();
    if text.as_bytes().get(tag_start) == Some(&b'/') {
        return None;
    }
    parse_think_tag_end(text, tag_start, true)
}

fn parse_think_close_at(text: &str, start: usize) -> Option<usize> {
    let slash = start + '<'.len_utf8();
    if text.as_bytes().get(slash) != Some(&b'/') {
        return None;
    }
    parse_think_tag_end(text, slash + '/'.len_utf8(), false)
}

fn parse_think_tag_end(text: &str, tag_start: usize, allow_attributes: bool) -> Option<usize> {
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

fn strip_markdown_fence(text: &str) -> &str {
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
const LEADING_BOILERPLATE_PREFIXES: &[&str] = &[
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

const BOILERPLATE_END_CHARS: &[char] = &['。', '：', ':', '，', ',', '\n'];

fn strip_leading_boilerplate(text: &str) -> &str {
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

/// Normalize only ordinary Markdown ordered-list runs in the built-in structured
/// style. Code, quotes, dates, versions and serialized Markdown stay literal.
pub(crate) fn normalize_structured_numbering(text: &str) -> String {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut fence: Option<char> = None;
    let mut completed = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some('`')
        } else if trimmed.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        if let Some(marker) = marker {
            if fence == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker);
            }
            completed.extend(groups.drain(..));
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some((indent, _, _, _)) = ordered_marker(line) {
            // An indented code block outside a list is literal Markdown.
            if indent >= 4 && groups.is_empty() {
                continue;
            }
            while groups.last().is_some_and(|(level, _)| *level > indent) {
                completed.push(groups.pop().unwrap());
            }
            if let Some((level, indices)) = groups.last_mut().filter(|(level, _)| *level == indent)
            {
                let _ = level;
                indices.push(index);
            } else {
                groups.push((indent, vec![index]));
            }
        } else if !trimmed.is_empty() {
            let indent = line.len() - trimmed.len();
            while groups.last().is_some_and(|(level, _)| *level >= indent) {
                completed.push(groups.pop().unwrap());
            }
        }
    }
    completed.extend(groups);
    for (_, indices) in completed {
        if indices.len() < 2 {
            continue;
        }
        for (number, index) in indices.into_iter().enumerate() {
            let line = &lines[index];
            let (indent, old, delimiter, body) = ordered_marker(line).unwrap();
            let mut body = &line[body..];
            // Strip only a duplicated identical marker, not arbitrary nested text.
            if let Some((0, repeated, repeat_delimiter, repeated_body)) = ordered_marker(body) {
                if repeated == old && repeat_delimiter == delimiter {
                    body = &body[repeated_body..];
                }
            }
            lines[index] = format!("{}{}{delimiter} {body}", &line[..indent], number + 1);
        }
    }
    lines.join("\n")
}

fn ordered_marker(line: &str) -> Option<(usize, usize, char, usize)> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let bytes = line.as_bytes();
    let mut end = indent;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == indent || end - indent > 2 {
        return None;
    }
    let number = line[indent..end].parse().ok()?;
    let delimiter = char::from(*bytes.get(end)?);
    if !matches!(delimiter, '.' | ')') || !bytes.get(end + 1)?.is_ascii_whitespace() {
        return None;
    }
    let body =
        end + 1 + line[end + 1..].len() - line[end + 1..].trim_start_matches([' ', '\t']).len();
    Some((indent, number, delimiter, body))
}

#[cfg(test)]
mod numbering_tests {
    use super::*;
    #[test]
    fn duplicate_numbers_and_nested_runs_are_normalized() {
        assert_eq!(
            normalize_structured_numbering(
                "1. A\n2. 2. B\n  1. child\n  1. child2\n2. C\n3. D\n3. E"
            ),
            "1. A\n2. B\n  1. child\n  2. child2\n3. C\n4. D\n5. E"
        );
    }
    #[test]
    fn separate_lists_and_parent_items_reset_children() {
        assert_eq!(
            normalize_structured_numbering(
                "1. A\n  1. a\n  1. b\n2. B\n  1. c\n  1. d\n\nHeading\n1. E\n1. F"
            ),
            "1. A\n  1. a\n  2. b\n2. B\n  1. c\n  2. d\n\nHeading\n1. E\n2. F"
        );
    }
    #[test]
    fn code_versions_dates_quotes_entities_and_alphabetic_children_remain_literal() {
        assert_eq!(
            normalize_structured_numbering("    1. command\n    1. command"),
            "    1. command\n    1. command"
        );
        let text = "2026. year\n2.1 version\n> 1. quote\n> 1. quote\n```\n1. code\n1. code\n```\n1. parent\n  (a) child\n2. parent\n2\\. escaped &#x20;\nhttps://example.com/1.2";
        assert_eq!(normalize_structured_numbering(text), text);
    }
}
