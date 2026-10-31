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

const NEAR_DUPLICATE_MIN_CHARS: usize = 12;
const NEAR_DUPLICATE_MIN_LENGTH_RATIO: f64 = 0.65;
const NEAR_DUPLICATE_MIN_ADJUSTED_SIMILARITY: f64 = 0.90;
const NEAR_DUPLICATE_MAX_SEGMENT_CHARS: usize = 512;

/// Privacy-safe metadata for two adjacent polished segments that look unusually
/// similar. The finding deliberately does not retain either segment's contents.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjacentNearDuplicate {
    pub left_chars: usize,
    pub right_chars: usize,
    pub similarity: f64,
    pub length_adjusted_similarity: f64,
}

/// Finds adjacent sentence-like segments that may be an accidental repetition.
///
/// This is an observation-only detector. It never changes the text because an
/// SSE delta and an intentional repeated sentence are indistinguishable without
/// provider metadata or user intent. Punctuation and whitespace define the
/// candidate boundaries; the comparison itself is Unicode scalar-safe.
pub fn find_adjacent_near_duplicates(content: &str) -> Vec<AdjacentNearDuplicate> {
    let segments = sentence_like_segments(content);
    segments
        .windows(2)
        .filter_map(|pair| near_duplicate_finding(pair[0], pair[1]))
        .collect()
}

fn sentence_like_segments(content: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    for (index, character) in content.char_indices() {
        let period_boundary = character == '.'
            && content[index + 1..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
            && !is_abbreviation(&content[start..index]);
        if matches!(
            character,
            '\n' | '。' | '！' | '？' | '!' | '?' | ';' | '；'
        ) || period_boundary
        {
            let end = index + character.len_utf8();
            let segment = content[start..end].trim();
            if !segment.is_empty() {
                segments.push(segment);
            }
            start = end;
        }
    }
    let tail = content[start..].trim();
    if !tail.is_empty() {
        segments.push(tail);
    }
    segments
}

fn is_abbreviation(before_period: &str) -> bool {
    let word = before_period.split_whitespace().next_back().unwrap_or("");
    // shortcut: recognize common English titles and initials; extend for observed false boundaries.
    (word.len() == 1 && word.bytes().all(|byte| byte.is_ascii_alphabetic()))
        || (word.contains('.')
            && word
                .split('.')
                .all(|part| part.len() == 1 && part.bytes().all(|byte| byte.is_ascii_alphabetic())))
        || matches!(
            word.to_ascii_lowercase().as_str(),
            "mr" | "mrs" | "ms" | "dr" | "prof" | "sr" | "jr" | "st" | "vs" | "etc"
        )
}

fn near_duplicate_finding(left: &str, right: &str) -> Option<AdjacentNearDuplicate> {
    let left = normalized_segment(left);
    let right = normalized_segment(right);
    if left.len() < NEAR_DUPLICATE_MIN_CHARS
        || right.len() < NEAR_DUPLICATE_MIN_CHARS
        || (left.len().min(right.len()) as f64 / left.len().max(right.len()) as f64)
            < NEAR_DUPLICATE_MIN_LENGTH_RATIO
        || left.len() > NEAR_DUPLICATE_MAX_SEGMENT_CHARS
        || right.len() > NEAR_DUPLICATE_MAX_SEGMENT_CHARS
    {
        return None;
    }

    let distance = levenshtein_distance(&left, &right);
    let longest = left.len().max(right.len());
    let shortest = left.len().min(right.len());
    let length_difference = left.len().abs_diff(right.len());
    let similarity = 1.0 - distance as f64 / longest as f64;
    let adjusted_distance = distance.saturating_sub(length_difference);
    let length_adjusted_similarity = 1.0 - adjusted_distance as f64 / shortest as f64;
    (length_adjusted_similarity >= NEAR_DUPLICATE_MIN_ADJUSTED_SIMILARITY).then_some(
        AdjacentNearDuplicate {
            left_chars: left.len(),
            right_chars: right.len(),
            similarity,
            length_adjusted_similarity,
        },
    )
}

fn normalized_segment(segment: &str) -> Vec<char> {
    use unicode_normalization::UnicodeNormalization;

    segment
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn levenshtein_distance(left: &[char], right: &[char]) -> usize {
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_character) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_character) in right.iter().enumerate() {
            let substitution =
                previous[right_index] + usize::from(left_character != right_character);
            current[right_index + 1] =
                (substitution.min(previous[right_index + 1] + 1)).min(current[right_index] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// Hashes text without exposing it in logs. The short digest is only intended
/// to correlate stages within a diagnostic record, not to authenticate content.
pub(crate) fn privacy_text_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(text.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

/// JSON keys audio models used for the polished text and the translation when they
/// wrapped a plain-text reply (issue #1304), in priority order. Compared
/// case-insensitively after trimming `[`/`]`, so the reply markers count as keys,
/// and also matched as a `_` suffix (`final_translation`).
const ENVELOPE_TRANSLATION_KEYS: &[&str] = &[
    "translated_text",
    "translation",
    "translated",
    "openless_translation",
];
const ENVELOPE_TEXT_KEYS: &[&str] = &[
    "final_text",
    "final",
    "polished_text",
    "openless_text",
    "text",
    "raw_transcript",
];

/// Plain text that a model returned as a JSON object (`{"final_text": "…"}`)
/// although it was asked for plain text (issue #1304). Returns `None` unless the
/// whole content is such an envelope, so ordinary text, including text that merely
/// contains braces, is left alone; `Some("")` means an envelope without text.
///
/// Only the known keys above are taken, translation keys first when
/// `prefer_translation`. `lenient` also takes an object whose only field is a
/// string and one quoted string in braces (`{"…"}`, not valid JSON): shapes seen
/// when the whole reply is the wrapper, but also what dictated JSON looks like.
pub(crate) fn unwrap_json_text_envelope(
    content: &str,
    prefer_translation: bool,
    lenient: bool,
) -> Option<String> {
    let trimmed = content.trim();
    let inner = trimmed.strip_prefix('{')?.strip_suffix('}')?;
    let text = match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
        &escape_json_string_controls(trimmed),
    ) {
        Ok(object) => {
            let field = |keys: &[&str]| {
                keys.iter().find_map(|wanted| {
                    object
                        .iter()
                        .find(|(key, _)| {
                            let key = key.trim_matches(['[', ']']).to_ascii_lowercase();
                            key == *wanted || key.ends_with(&format!("_{wanted}"))
                        })
                        .and_then(|(_, value)| value.as_str())
                })
            };
            let only = match object.values().collect::<Vec<_>>().as_slice() {
                [value] if lenient => value.as_str(),
                _ => None,
            };
            prefer_translation
                .then(|| field(ENVELOPE_TRANSLATION_KEYS))
                .flatten()
                .or_else(|| field(ENVELOPE_TEXT_KEYS))
                .or(only)?
                .to_string()
        }
        Err(_) if lenient => {
            serde_json::from_str::<String>(&escape_json_string_controls(inner.trim())).ok()?
        }
        Err(_) => return None,
    };
    Some(text.trim().to_string())
}

/// Models write real line breaks and tabs inside JSON strings, which JSON forbids.
fn escape_json_string_controls(json: &str) -> Cow<'_, str> {
    if !json.contains(['\n', '\r', '\t']) {
        return Cow::Borrowed(json);
    }
    let mut escaped = String::with_capacity(json.len());
    let (mut in_string, mut after_backslash) = (false, false);
    for character in json.chars() {
        if in_string {
            match character {
                _ if after_backslash => after_backslash = false,
                '\\' => after_backslash = true,
                '"' => in_string = false,
                '\n' => {
                    escaped.push_str("\\n");
                    continue;
                }
                '\r' => {
                    escaped.push_str("\\r");
                    continue;
                }
                '\t' => {
                    escaped.push_str("\\t");
                    continue;
                }
                _ => {}
            }
        } else if character == '"' {
            in_string = true;
        }
        escaped.push(character);
    }
    Cow::Owned(escaped)
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

    #[test]
    fn adjacent_near_duplicate_detector_reports_only_metadata() {
        let findings = find_adjacent_near_duplicates(
            "这个问题需要及时处理，避免重复内容。这个问题需要及时处理，避免出现重复内容。",
        );

        assert_eq!(findings.len(), 1);
        assert!(findings[0].left_chars >= NEAR_DUPLICATE_MIN_CHARS);
        assert!(findings[0].right_chars > findings[0].left_chars);
        assert!(findings[0].length_adjusted_similarity >= 0.9);
    }

    #[test]
    fn adjacent_near_duplicate_detector_does_not_treat_short_or_distinct_text_as_duplicate() {
        assert!(find_adjacent_near_duplicates("非常非常重要。").is_empty());
        assert!(
            find_adjacent_near_duplicates("第一段内容完全不同。第二段也完全不一样。").is_empty()
        );
    }

    #[test]
    fn near_duplicate_observation_never_changes_cleaned_text() {
        let content = "这个问题需要及时处理，避免重复内容。这个问题需要及时处理，避免重复内容。";
        assert_eq!(find_adjacent_near_duplicates(content).len(), 1);
        assert_eq!(clean_polish_output(content), content);
    }

    #[test]
    fn adjacent_near_duplicates_support_english_sentence_endings() {
        for separator in [". ", "! ", "? ", "\n"] {
            let text = format!(
                "Please submit the final report today{separator}Please submit the final report today."
            );
            assert_eq!(
                find_adjacent_near_duplicates(&text).len(),
                1,
                "{separator:?}"
            );
        }
        assert_eq!(
            find_adjacent_near_duplicates(
                "Please submit the final report today. Please submit the revised final report today."
            ).len(),
            1
        );
    }

    #[test]
    fn sentence_boundaries_preserve_decimals_and_abbreviations() {
        assert_eq!(
            sentence_like_segments("Dr. A. Smith paid 3.14 dollars, e.g. in cash. Next sentence."),
            [
                "Dr. A. Smith paid 3.14 dollars, e.g. in cash.",
                "Next sentence."
            ]
        );
        assert_eq!(
            sentence_like_segments("The price is 3.14. Next sentence."),
            ["The price is 3.14.", "Next sentence."]
        );
    }

    #[test]
    fn json_text_envelope_unwraps_shapes_returned_by_audio_models() {
        let unwrap = |content: &str| unwrap_json_text_envelope(content, false, true);
        assert_eq!(
            unwrap(r#"{"final_text": "你好。"}"#).as_deref(),
            Some("你好。")
        );
        assert_eq!(
            unwrap("{\"final_text\":\"标题：\\n\\n1. 第一项\"}").as_deref(),
            Some("标题：\n\n1. 第一项")
        );
        // Real line breaks inside the string are not valid JSON but still a wrapper.
        assert_eq!(
            unwrap("{\n  \"final_text\": \"第一行\n第二行\"\n}").as_deref(),
            Some("第一行\n第二行")
        );
        assert_eq!(
            unwrap(r#" {"raw_transcript": "原话"} "#).as_deref(),
            Some("原话")
        );
        assert_eq!(
            unwrap(r#"{"original_text": "原文", "final_text": "整理后"}"#).as_deref(),
            Some("整理后")
        );
        // Known keys win regardless of key order or other metadata keys.
        assert_eq!(
            unwrap(r#"{"final_output_language": "简体中文", "final_text": "你好"}"#).as_deref(),
            Some("你好")
        );
        assert_eq!(
            unwrap(r#"{"final": "整理后", "confidence": 0.55}"#).as_deref(),
            Some("整理后")
        );
        assert_eq!(
            unwrap(r#"{"项目启动请求": "请再次启动项目。"}"#).as_deref(),
            Some("请再次启动项目。")
        );
        // Braces around one quoted string are not valid JSON but are still a wrapper.
        assert_eq!(
            unwrap(r#"{"现在 .env 文件已经完整了。"}"#).as_deref(),
            Some("现在 .env 文件已经完整了。")
        );
        // A recognized envelope without text is reported as empty, not left as JSON.
        assert_eq!(unwrap(r#"{"final_text": "  "}"#).as_deref(), Some(""));
        let both = r#"{"final_text": "中文", "translated_text": "English"}"#;
        assert_eq!(unwrap(both).as_deref(), Some("中文"));
        assert_eq!(
            unwrap_json_text_envelope(both, true, false).as_deref(),
            Some("English")
        );
        assert_eq!(
            unwrap_json_text_envelope(
                r#"{"final_source": "你好", "final_translation": "Hello"}"#,
                true,
                false
            )
            .as_deref(),
            Some("Hello")
        );
        assert_eq!(
            unwrap_json_text_envelope(
                r#"{"translated_from": "zh", "translation": "Hello"}"#,
                true,
                false
            )
            .as_deref(),
            Some("Hello")
        );
        assert_eq!(
            unwrap_json_text_envelope(
                r#"{"[[OPENLESS_POLISHED_SOURCE]]": "你好", "[[OPENLESS_TRANSLATION]]": "Hello"}"#,
                true,
                false
            )
            .as_deref(),
            Some("Hello")
        );
    }

    #[test]
    fn json_text_envelope_leaves_other_text_alone() {
        for content in [
            "普通文本",
            r#"{"env"} 文件就是环境配置文件。"#,
            "{\"这里面\"这部分不太通顺，整理如下：}\n\n正文",
            r#"{"steps": ["a", "b"]}"#,
            r#"{"final_text": 1}"#,
            r#"{"a": "一", "b": "二"}"#,
            "{}",
        ] {
            assert_eq!(
                unwrap_json_text_envelope(content, false, true),
                None,
                "{content}"
            );
        }
        // Strict mode keeps dictated JSON that does not use a known key.
        for content in [r#"{"name": "Alice"}"#, r#"{"apple"}"#] {
            assert_eq!(
                unwrap_json_text_envelope(content, false, false),
                None,
                "{content}"
            );
        }
    }
}
