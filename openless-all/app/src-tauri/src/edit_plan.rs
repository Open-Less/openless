//! Structured edit plans produced by the selection-voice LLM and applied
//! deterministically to a draft (issue #987 desktop MVP; EditPlan shape refs #900).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

use crate::correction::apply_rule;
use crate::polish::{clean_json_llm_output, clean_xml_llm_output};

const MAX_OPERATIONS: usize = 32;
const MAX_OP_STRING_LEN: usize = 8_192;
const MAX_PATTERN_LEN: usize = 512;
const REGEX_TIMEOUT_MS: u64 = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditPlan {
    pub operations: Vec<EditOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditOperation {
    LiteralReplace {
        find: String,
        replace: String,
    },
    RegexReplace {
        pattern: String,
        replace: String,
        #[serde(default)]
        flags: RegexFlags,
    },
    RangeReplace {
        start: u32,
        end: u32,
        replace: String,
    },
    FullRewrite {
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct RegexFlags {
    #[serde(default)]
    pub case_insensitive: bool,
    #[serde(default)]
    pub multiline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditApplyError {
    TooManyOperations,
    OperationTooLarge,
    PatternTooLarge,
    EmptyDraft,
    InvalidRange,
    RegexRejected(String),
    RegexTimedOut,
    NoOperations,
}

impl std::fmt::Display for EditApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyOperations => write!(f, "edit plan has too many operations"),
            Self::OperationTooLarge => write!(f, "edit operation exceeds size limit"),
            Self::PatternTooLarge => write!(f, "regex pattern exceeds size limit"),
            Self::EmptyDraft => write!(f, "draft is empty"),
            Self::InvalidRange => write!(f, "range replace indices are invalid"),
            Self::RegexRejected(reason) => write!(f, "regex rejected: {reason}"),
            Self::RegexTimedOut => write!(f, "regex execution timed out"),
            Self::NoOperations => write!(f, "edit plan has no operations"),
        }
    }
}

impl std::error::Error for EditApplyError {}

const EDIT_PLAN_ROOT_TAG: &str = "edit_plan";
const EDIT_OPERATION_TAGS: &[&str] = &[
    "literal_replace",
    "regex_replace",
    "range_replace",
    "full_rewrite",
];

/// Parse LLM edit-plan output (XML primary, JSON legacy fallback).
pub fn parse_edit_plan(raw: &str) -> Result<EditPlan, String> {
    let trimmed = raw.trim();
    if trimmed.contains('<') {
        match parse_edit_plan_xml(trimmed) {
            Ok(plan) => return Ok(plan),
            Err(xml_error) => {
                if trimmed.contains('{') {
                    return parse_edit_plan_json(trimmed).map_err(|json_error| {
                        format!(
                            "invalid EditPlan XML: {xml_error}; JSON fallback: {json_error}"
                        )
                    });
                }
                return Err(format!("invalid EditPlan XML: {xml_error}"));
            }
        }
    }
    parse_edit_plan_json(trimmed)
}

pub fn parse_edit_plan_xml(raw: &str) -> Result<EditPlan, String> {
    let cleaned = clean_xml_llm_output(raw);
    let candidate = if cleaned.is_empty() { raw.trim() } else { cleaned.trim() };
    let block = extract_edit_plan_block(candidate).unwrap_or_else(|| candidate.to_string());
    let (inner, _, _) = extract_element_block(&block, EDIT_PLAN_ROOT_TAG, 0)
        .map_err(|error| format!("missing <{EDIT_PLAN_ROOT_TAG}> root: {error}"))?;
    let summary = extract_child_text(&inner, "summary");
    let operations = parse_operations_xml(&inner)?;
    if operations.is_empty() {
        return Err("edit plan has no operations".into());
    }
    Ok(EditPlan {
        operations,
        summary,
    })
}

fn extract_edit_plan_block(raw: &str) -> Option<String> {
    let start = find_open_tag(raw, EDIT_PLAN_ROOT_TAG, 0)?;
    let close_needle = format!("</{EDIT_PLAN_ROOT_TAG}>");
    let close_start = find_ci_substr(&raw[start..], &close_needle)?;
    let end = start + close_start + close_needle.len();
    Some(raw[start..end].to_string())
}

fn parse_operations_xml(edit_plan_inner: &str) -> Result<Vec<EditOperation>, String> {
    let mut operations = Vec::new();
    let mut cursor = 0;
    while cursor < edit_plan_inner.len() {
        let mut next: Option<(usize, &'static str)> = None;
        for tag in EDIT_OPERATION_TAGS {
            if let Some(pos) = find_open_tag(edit_plan_inner, tag, cursor) {
                if next.map_or(true, |(best, _)| pos < best) {
                    next = Some((pos, tag));
                }
            }
        }
        match next {
            None => break,
            Some((pos, tag)) => {
                let (inner, opening_tag, consumed) =
                    extract_element_block(edit_plan_inner, tag, pos)?;
                operations.push(parse_operation_xml(tag, &inner, &opening_tag)?);
                cursor = pos + consumed;
            }
        }
    }
    Ok(operations)
}

fn parse_operation_xml(
    tag: &str,
    inner: &str,
    opening_tag: &str,
) -> Result<EditOperation, String> {
    match tag {
        "literal_replace" => Ok(EditOperation::LiteralReplace {
            find: extract_child_text(inner, "find").unwrap_or_default(),
            replace: extract_child_text(inner, "replace").unwrap_or_default(),
        }),
        "regex_replace" => {
            let flags = RegexFlags {
                case_insensitive: parse_bool_attr(opening_tag, "case_insensitive"),
                multiline: parse_bool_attr(opening_tag, "multiline"),
            };
            Ok(EditOperation::RegexReplace {
                pattern: extract_child_text(inner, "pattern")
                    .or_else(|| extract_child_text(inner, "regex"))
                    .unwrap_or_default(),
                replace: extract_child_text(inner, "replace").unwrap_or_default(),
                flags,
            })
        }
        "range_replace" => {
            let start = parse_u32_attr(opening_tag, "start")
                .or_else(|| extract_child_text(inner, "start").and_then(|text| parse_u32_text(&text)))
                .unwrap_or(0);
            let end = parse_u32_attr(opening_tag, "end")
                .or_else(|| extract_child_text(inner, "end").and_then(|text| parse_u32_text(&text)))
                .unwrap_or(0);
            Ok(EditOperation::RangeReplace {
                start,
                end,
                replace: extract_child_text(inner, "replace").unwrap_or_default(),
            })
        }
        "full_rewrite" => Ok(EditOperation::FullRewrite {
            text: extract_rewrite_text(inner),
        }),
        other => Err(format!("unknown edit operation tag: {other}")),
    }
}

fn extract_rewrite_text(inner: &str) -> String {
    extract_child_text(inner, "text")
        .or_else(|| extract_child_text(inner, "content"))
        .unwrap_or_else(|| decode_xml_text(inner.trim()))
}

fn extract_child_text(parent: &str, tag: &str) -> Option<String> {
    let start = find_open_tag(parent, tag, 0)?;
    let (inner, _, _) = extract_element_block(parent, tag, start).ok()?;
    Some(decode_xml_text(inner.trim()))
}

fn extract_element_block(
    content: &str,
    tag: &str,
    from: usize,
) -> Result<(String, String, usize), String> {
    let start = find_open_tag(content, tag, from)
        .ok_or_else(|| format!("<{tag}> not found"))?;
    let after_name = start + tag.len() + 1; // '<' + tag
    let open_end_rel = content[after_name..]
        .find('>')
        .ok_or_else(|| format!("<{tag}> opening tag incomplete"))?;
    let open_end = after_name + open_end_rel + 1;
    let opening_tag = content[start..open_end].to_string();
    let close_needle = format!("</{tag}>");
    let close_rel = find_ci_substr(&content[open_end..], &close_needle)
        .ok_or_else(|| format!("</{tag}> not found"))?;
    let inner = content[open_end..open_end + close_rel].to_string();
    let consumed = open_end + close_rel + close_needle.len() - from;
    Ok((inner, opening_tag, consumed))
}

fn find_open_tag(content: &str, tag: &str, from: usize) -> Option<usize> {
    let needle = format!("<{tag}");
    find_ci_substr(&content[from..], &needle).map(|rel| from + rel)
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
        if starts_with_ci(&hb[i..], needle) {
            return Some(i);
        }
    }
    None
}

fn starts_with_ci(haystack: &[u8], needle: &str) -> bool {
    let nb = needle.as_bytes();
    if haystack.len() < nb.len() {
        return false;
    }
    haystack
        .iter()
        .zip(nb.iter())
        .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

fn parse_bool_attr(opening_tag: &str, attr: &str) -> bool {
    parse_attr_value(opening_tag, attr)
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn parse_u32_attr(opening_tag: &str, attr: &str) -> Option<u32> {
    parse_attr_value(opening_tag, attr).and_then(|text| parse_u32_text(&text))
}

fn parse_u32_text(raw: &str) -> Option<u32> {
    raw.trim().parse().ok()
}

fn parse_attr_value(opening_tag: &str, attr: &str) -> Option<String> {
    let lower = opening_tag.to_lowercase();
    let attr_lower = attr.to_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find(&attr_lower) {
        let idx = search_from + rel + attr_lower.len();
        let rest = opening_tag[idx..].trim_start();
        if let Some(rest) = rest.strip_prefix('=') {
            let rest = rest.trim_start();
            if let Some(value) = read_quoted_attr_value(rest) {
                return Some(value);
            }
        }
        search_from = search_from + rel + 1;
    }
    None
}

fn read_quoted_attr_value(raw: &str) -> Option<String> {
    let first = raw.chars().next()?;
    if first != '"' && first != '\'' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in raw.chars().skip(1) {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == first {
            return Some(out);
        }
        out.push(ch);
    }
    None
}

fn decode_xml_text(raw: &str) -> String {
    let trimmed = raw.trim();
    let cdata = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|rest| rest.strip_suffix("]]>"))
        .map(str::trim);
    let source = cdata.unwrap_or(trimmed);
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            let mut entity = String::new();
            while let Some(&next) = chars.peek() {
                if next == ';' {
                    chars.next();
                    break;
                }
                if next.is_alphanumeric() || next == '#' {
                    entity.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            match entity.as_str() {
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "amp" => out.push('&'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                other if other.starts_with("#x") => {
                    if let Ok(code) = u32::from_str_radix(other[2..].trim(), 16) {
                        if let Some(decoded) = char::from_u32(code) {
                            out.push(decoded);
                        }
                    }
                }
                other if other.starts_with('#') => {
                    if let Ok(code) = other[1..].trim().parse::<u32>() {
                        if let Some(decoded) = char::from_u32(code) {
                            out.push(decoded);
                        }
                    }
                }
                _ => out.push('&'),
            }
            continue;
        }
        out.push(ch);
    }
    out
}

pub fn parse_edit_plan_json(raw: &str) -> Result<EditPlan, String> {
    let trimmed = raw.trim();
    match parse_edit_plan_json_candidate(trimmed) {
        Ok(plan) => Ok(plan),
        Err(primary) => {
            let cleaned = clean_json_llm_output(raw);
            if cleaned == trimmed {
                Err(primary)
            } else {
                parse_edit_plan_json_candidate(&cleaned).map_err(|secondary| {
                    format!("invalid EditPlan JSON: {primary}; cleaned retry: {secondary}")
                })
            }
        }
    }
}

fn parse_edit_plan_json_candidate(raw: &str) -> Result<EditPlan, String> {
    let json = extract_json_object(raw).unwrap_or(raw);
    let mut value: Value = serde_json::from_str(json)
        .map_err(|error| format!("invalid EditPlan JSON: {error}"))?;
    normalize_edit_plan_value(&mut value);
    serde_json::from_value(value).map_err(|error| format!("invalid EditPlan JSON: {error}"))
}

fn normalize_edit_plan_value(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    if !obj.contains_key("operations") {
        if let Some(ops) = obj.remove("operation") {
            obj.insert("operations".to_string(), ops);
        }
    }
    if let Some(ops) = obj.get_mut("operations").and_then(|v| v.as_array_mut()) {
        for op in ops {
            normalize_edit_operation_value(op);
        }
    }
}

fn normalize_edit_operation_value(op: &mut Value) {
    let Some(obj) = op.as_object_mut() else {
        return;
    };
    if let Some(type_value) = obj.get("type").and_then(|v| v.as_str()) {
        let normalized = normalize_operation_type(type_value);
        obj.insert("type".to_string(), Value::String(normalized));
    }
    let op_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_default();
    if op_type == "full_rewrite" {
        promote_alias_field(obj, "text", &["content", "body", "value", "replacement"]);
    }
    if op_type == "literal_replace" {
        promote_alias_field(obj, "replace", &["replacement", "with", "value"]);
        promote_alias_field(obj, "find", &["search", "match", "pattern"]);
    }
    if op_type == "regex_replace" {
        promote_alias_field(obj, "pattern", &["regex", "find", "search"]);
        promote_alias_field(obj, "replace", &["replacement", "with", "value"]);
    }
    if op_type == "range_replace" {
        promote_alias_field(obj, "replace", &["replacement", "with", "value", "text"]);
    }
}

fn normalize_operation_type(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "fullrewrite" | "full_rewrite" | "rewrite" | "translate" | "translation" => {
            "full_rewrite".into()
        }
        "literalreplace" | "literal_replace" | "replace" | "text_replace" => {
            "literal_replace".into()
        }
        "regexreplace" | "regex_replace" | "regexp_replace" => "regex_replace".into(),
        "rangereplace" | "range_replace" | "substring_replace" => "range_replace".into(),
        other => other.to_string(),
    }
}

fn promote_alias_field(
    obj: &mut serde_json::Map<String, Value>,
    canonical: &str,
    aliases: &[&str],
) {
    if obj.contains_key(canonical) {
        return;
    }
    for alias in aliases {
        let key = (*alias).to_string();
        if let Some(value) = obj.remove(&key) {
            obj.insert(canonical.to_string(), value);
            return;
        }
    }
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (start <= end).then(|| &raw[start..=end])
}

pub fn apply_edit_plan(draft: &str, plan: &EditPlan) -> Result<String, EditApplyError> {
    if draft.is_empty() {
        return Err(EditApplyError::EmptyDraft);
    }
    if plan.operations.is_empty() {
        return Err(EditApplyError::NoOperations);
    }
    if plan.operations.len() > MAX_OPERATIONS {
        return Err(EditApplyError::TooManyOperations);
    }

    let mut current = draft.to_string();
    for op in &plan.operations {
        validate_operation_size(op)?;
        current = apply_operation(&current, op)?;
    }
    Ok(current)
}

fn validate_operation_size(op: &EditOperation) -> Result<(), EditApplyError> {
    let too_large = |value: &str| value.chars().count() > MAX_OP_STRING_LEN;
    match op {
        EditOperation::LiteralReplace { find, replace } => {
            if too_large(find) || too_large(replace) {
                return Err(EditApplyError::OperationTooLarge);
            }
        }
        EditOperation::RegexReplace {
            pattern,
            replace,
            ..
        } => {
            if pattern.chars().count() > MAX_PATTERN_LEN
                || too_large(replace)
            {
                return Err(EditApplyError::PatternTooLarge);
            }
        }
        EditOperation::RangeReplace { replace, .. } => {
            if too_large(replace) {
                return Err(EditApplyError::OperationTooLarge);
            }
        }
        EditOperation::FullRewrite { text } => {
            if too_large(text) {
                return Err(EditApplyError::OperationTooLarge);
            }
        }
    }
    Ok(())
}

fn apply_operation(text: &str, op: &EditOperation) -> Result<String, EditApplyError> {
    match op {
        EditOperation::LiteralReplace { find, replace } => {
            if find.is_empty() {
                return Ok(text.to_string());
            }
            Ok(apply_rule(text, find, replace))
        }
        EditOperation::RegexReplace {
            pattern,
            replace,
            flags,
        } => apply_regex_replace(text, pattern, replace, *flags),
        EditOperation::RangeReplace {
            start,
            end,
            replace,
        } => apply_range_replace(text, *start, *end, replace),
        EditOperation::FullRewrite { text } => Ok(text.clone()),
    }
}

fn apply_range_replace(
    text: &str,
    start: u32,
    end: u32,
    replacement: &str,
) -> Result<String, EditApplyError> {
    if end < start {
        return Err(EditApplyError::InvalidRange);
    }
    let char_len = text.chars().count() as u32;
    if start > char_len || end > char_len {
        return Err(EditApplyError::InvalidRange);
    }
    let start_byte = char_index_to_byte(text, start as usize)?;
    let end_byte = char_index_to_byte(text, end as usize)?;
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..start_byte]);
    out.push_str(replacement);
    out.push_str(&text[end_byte..]);
    Ok(out)
}

fn char_index_to_byte(text: &str, char_index: usize) -> Result<usize, EditApplyError> {
    if char_index == 0 {
        return Ok(0);
    }
    let mut count = 0usize;
    for (byte_index, _) in text.char_indices() {
        if count == char_index {
            return Ok(byte_index);
        }
        count += 1;
    }
    if count == char_index {
        return Ok(text.len());
    }
    Err(EditApplyError::InvalidRange)
}

fn apply_regex_replace(
    text: &str,
    pattern: &str,
    replacement: &str,
    flags: RegexFlags,
) -> Result<String, EditApplyError> {
    if pattern.trim().is_empty() {
        return Ok(text.to_string());
    }
    if contains_nested_quantifiers(pattern) {
        return Err(EditApplyError::RegexRejected(
            "nested quantifiers are not allowed".into(),
        ));
    }

    let mut builder = regex::RegexBuilder::new(pattern);
    builder.case_insensitive(flags.case_insensitive);
    if flags.multiline {
        builder.multi_line(true);
    }
    let regex = builder
        .size_limit(1 << 20)
        .build()
        .map_err(|error| EditApplyError::RegexRejected(error.to_string()))?;

    let started = Instant::now();
    let haystack = text.to_string();
    let pattern_owned = pattern.to_string();
    let replacement_owned = replacement.to_string();
    let regex_owned = regex;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = regex_owned.replace_all(&haystack, replacement_owned.as_str());
        let _ = tx.send(result.into_owned());
    });

    match rx.recv_timeout(Duration::from_millis(REGEX_TIMEOUT_MS)) {
        Ok(replaced) => {
            if started.elapsed() > Duration::from_millis(REGEX_TIMEOUT_MS) {
                return Err(EditApplyError::RegexTimedOut);
            }
            Ok(replaced)
        }
        Err(_) => {
            log::warn!(
                "[edit-plan] regex timed out after {REGEX_TIMEOUT_MS}ms (pattern={pattern_owned:?})"
            );
            Err(EditApplyError::RegexTimedOut)
        }
    }
}

fn contains_nested_quantifiers(pattern: &str) -> bool {
    let quantifiers = ['*', '+', '?', '{'];
    let mut prev_was_quantifier = false;
    for ch in pattern.chars() {
        let is_quantifier = quantifiers.contains(&ch);
        if is_quantifier && prev_was_quantifier {
            return true;
        }
        prev_was_quantifier = is_quantifier;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_replace_masks_credentials() {
        let draft = "账号: old@mail.com\n密码: secret123";
        let plan = EditPlan {
            operations: vec![EditOperation::LiteralReplace {
                find: "old@mail.com".into(),
                replace: "user@example.com".into(),
            }],
            summary: None,
        };
        assert_eq!(
            apply_edit_plan(draft, &plan).unwrap(),
            "账号: user@example.com\n密码: secret123"
        );
    }

    #[test]
    fn regex_replace_batch_email_format() {
        let draft = "邮箱1: a@b.com\n邮箱2: c@d.com";
        let plan = EditPlan {
            operations: vec![EditOperation::RegexReplace {
                pattern: r"([a-z]+)@([a-z]+\.com)".into(),
                replace: r"$1@company.com".into(),
                flags: RegexFlags::default(),
            }],
            summary: Some("normalize email domains".into()),
        };
        let out = apply_edit_plan(draft, &plan).unwrap();
        assert!(out.contains("a@company.com"));
        assert!(out.contains("c@company.com"));
    }

    #[test]
    fn range_replace_is_char_safe() {
        let draft = "你好世界";
        let plan = EditPlan {
            operations: vec![EditOperation::RangeReplace {
                start: 2,
                end: 4,
                replace: "Rust".into(),
            }],
            summary: None,
        };
        assert_eq!(apply_edit_plan(draft, &plan).unwrap(), "你好Rust");
    }

    #[test]
    fn full_rewrite_replaces_entire_draft() {
        let draft = "旧内容";
        let plan = EditPlan {
            operations: vec![EditOperation::FullRewrite {
                text: "新内容".into(),
            }],
            summary: None,
        };
        assert_eq!(apply_edit_plan(draft, &plan).unwrap(), "新内容");
    }

    #[test]
    fn rejects_empty_operations() {
        let plan = EditPlan {
            operations: vec![],
            summary: None,
        };
        assert_eq!(
            apply_edit_plan("text", &plan),
            Err(EditApplyError::NoOperations)
        );
    }

    #[test]
    fn rejects_invalid_range() {
        let plan = EditPlan {
            operations: vec![EditOperation::RangeReplace {
                start: 5,
                end: 2,
                replace: "x".into(),
            }],
            summary: None,
        };
        assert_eq!(
            apply_edit_plan("abc", &plan),
            Err(EditApplyError::InvalidRange)
        );
    }

    #[test]
    fn parses_xml_literal_replace() {
        let raw = r#"<edit_plan>
  <summary>replace email</summary>
  <literal_replace>
    <find>old@mail.com</find>
    <replace>user@company.com</replace>
  </literal_replace>
</edit_plan>"#;
        let plan = parse_edit_plan_xml(raw).unwrap();
        assert_eq!(plan.summary.as_deref(), Some("replace email"));
        assert_eq!(plan.operations.len(), 1);
        assert_eq!(
            plan.operations[0],
            EditOperation::LiteralReplace {
                find: "old@mail.com".into(),
                replace: "user@company.com".into(),
            }
        );
    }

    #[test]
    fn parses_xml_full_rewrite_multiline() {
        let raw = r#"<edit_plan>
  <full_rewrite>
    <text>Line one
Line two</text>
  </full_rewrite>
</edit_plan>"#;
        let plan = parse_edit_plan_xml(raw).unwrap();
        assert_eq!(
            plan.operations[0],
            EditOperation::FullRewrite {
                text: "Line one\nLine two".into()
            }
        );
    }

    #[test]
    fn parses_operation_alias_and_translate_type() {
        let raw = r#"{"operation":[{"type":"translate","content":"Hello"}]}"#;
        let plan = parse_edit_plan_json(raw).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert_eq!(
            plan.operations[0],
            EditOperation::FullRewrite {
                text: "Hello".into()
            }
        );
    }

    #[test]
    fn parses_json_with_surrounding_markdown() {
        let raw = r#"Here is the plan:
```json
{"operations":[{"type":"literal_replace","find":"a","replace":"b"}],"summary":"ok"}
```"#;
        let plan = parse_edit_plan_json(raw).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert_eq!(plan.summary.as_deref(), Some("ok"));
    }
}
