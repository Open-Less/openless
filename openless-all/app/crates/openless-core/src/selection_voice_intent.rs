//! Intent routing for selection-voice sessions shared by all UI hosts.

use crate::shared_types::{OutputLanguagePreference, UserPreferences};
use crate::types::{SelectionVoiceIntentMode, SelectionVoiceManualIntent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionVoiceIntent {
    Question,
    Edit,
    Compose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionVoiceIntentClassification {
    pub intent: SelectionVoiceIntent,
    pub source: &'static str,
}

pub const LEGACY_EDIT_KEYWORD_DEFAULTS: &[&str] = &["翻译", "改成", "替换", "批量", "格式"];

pub const BUILTIN_QUESTION_CUES: &[&str] = &[
    "吗",
    "呢",
    "么",
    "什么",
    "怎么",
    "怎样",
    "为何",
    "为什么",
    "是否",
    "是不是",
    "有没有",
    "哪",
    "几",
    "多少",
    "谁",
    "何时",
    "何处",
    "如何",
    "能否",
    "可以吗",
    "对吗",
    "好吗",
    "how",
    "what",
    "why",
    "when",
    "where",
    "which",
    "who",
    "whose",
    "is it",
    "are you",
    "do you",
    "does ",
    "did ",
    "can you",
    "could you",
];

pub const BUILTIN_COMPOSE_CUES: &[&str] = &[
    "帮我写",
    "写一封",
    "写一条",
    "写一个",
    "起草",
    "拟一封",
    "help me write",
    "write an email",
    "write a message",
    "draft a",
    "write a group",
    "write a post",
    "i want to post",
    "i need a",
];

pub fn looks_like_question_instruction(instruction: &str) -> bool {
    let trimmed = instruction.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.to_lowercase();
    let without_trail = normalized.trim_end_matches(|c: char| {
        c == '.' || c == '。' || c == '!' || c == '！' || c.is_whitespace()
    });
    if without_trail.ends_with('?') || without_trail.ends_with('？') {
        return true;
    }
    BUILTIN_QUESTION_CUES
        .iter()
        .any(|cue| normalized.contains(&cue.to_lowercase()))
}

pub fn looks_like_compose_instruction(instruction: &str) -> bool {
    let trimmed = instruction.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.to_lowercase();
    BUILTIN_COMPOSE_CUES
        .iter()
        .any(|cue| normalized.contains(&cue.to_lowercase()))
}

pub fn intent_heuristic_is_ambiguous(instruction: &str) -> bool {
    let trimmed = instruction.trim();
    if trimmed.is_empty() {
        return true;
    }
    if looks_like_compose_instruction(trimmed) || looks_like_question_instruction(trimmed) {
        return false;
    }
    trimmed.chars().count() < 4
}

fn is_legacy_edit_keyword_default(keyword: &str) -> bool {
    let trimmed = keyword.trim();
    LEGACY_EDIT_KEYWORD_DEFAULTS
        .iter()
        .any(|legacy| legacy.eq_ignore_ascii_case(trimmed))
}

pub fn effective_question_keywords(keywords: &[String]) -> Vec<&str> {
    keywords
        .iter()
        .filter_map(|keyword| {
            let trimmed = keyword.trim();
            if trimmed.is_empty() || is_legacy_edit_keyword_default(trimmed) {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect()
}

pub fn resolve_selection_voice_intent_heuristic(
    instruction_polished: &str,
    question_keywords: &[String],
    selection_empty: bool,
) -> SelectionVoiceIntent {
    // Strong compose cues win even when the instruction also looks like a question
    // (e.g. 「能帮我写一封邮件吗？」) and even with a non-empty selection.
    if looks_like_compose_instruction(instruction_polished) {
        return SelectionVoiceIntent::Compose;
    }
    let normalized = instruction_polished.to_lowercase();
    for keyword in effective_question_keywords(question_keywords) {
        if normalized.contains(&keyword.to_lowercase()) {
            return SelectionVoiceIntent::Question;
        }
    }
    if looks_like_question_instruction(instruction_polished) {
        return SelectionVoiceIntent::Question;
    }
    if selection_empty {
        SelectionVoiceIntent::Compose
    } else {
        SelectionVoiceIntent::Edit
    }
}

pub fn looks_like_edit_instruction(instruction: &str) -> bool {
    !instruction.trim().is_empty()
        && !looks_like_question_instruction(instruction)
        && !looks_like_compose_instruction(instruction)
}

/// Resolve the selection-voice intent without accepting a host-specific
/// preferences object.
pub fn classify_selection_voice_intent(
    mode: SelectionVoiceIntentMode,
    manual_intent: SelectionVoiceManualIntent,
    question_keywords: &[String],
    instruction_polished: &str,
    selection_empty: bool,
) -> SelectionVoiceIntentClassification {
    classify_selection_voice_intent_with_provider_result(
        mode,
        manual_intent,
        question_keywords,
        instruction_polished,
        selection_empty,
        None,
    )
}

pub fn classify_selection_voice_intent_with_provider_result(
    mode: SelectionVoiceIntentMode,
    manual_intent: SelectionVoiceManualIntent,
    question_keywords: &[String],
    instruction_polished: &str,
    selection_empty: bool,
    auto_classification: Option<&str>,
) -> SelectionVoiceIntentClassification {
    match mode {
        SelectionVoiceIntentMode::Prompt => SelectionVoiceIntentClassification {
            intent: SelectionVoiceIntent::Question,
            source: "prompt_pending",
        },
        SelectionVoiceIntentMode::Manual => SelectionVoiceIntentClassification {
            intent: match manual_intent {
                SelectionVoiceManualIntent::Question => SelectionVoiceIntent::Question,
                SelectionVoiceManualIntent::Edit => SelectionVoiceIntent::Edit,
                SelectionVoiceManualIntent::Compose => SelectionVoiceIntent::Compose,
            },
            source: "manual",
        },
        SelectionVoiceIntentMode::Heuristic => SelectionVoiceIntentClassification {
            intent: resolve_selection_voice_intent_heuristic(
                instruction_polished,
                question_keywords,
                selection_empty,
            ),
            source: "heuristic",
        },
        SelectionVoiceIntentMode::Auto => {
            if let Some(intent) = auto_classification.and_then(parse_intent_classification_json) {
                return SelectionVoiceIntentClassification {
                    intent,
                    source: "auto_llm",
                };
            }
            let intent = resolve_selection_voice_intent_heuristic(
                instruction_polished,
                question_keywords,
                selection_empty,
            );
            SelectionVoiceIntentClassification {
                intent,
                source: if auto_classification.is_some() {
                    "auto_heuristic_fallback"
                } else {
                    match intent {
                        SelectionVoiceIntent::Question => "auto_question",
                        SelectionVoiceIntent::Edit => "auto_edit",
                        SelectionVoiceIntent::Compose => "auto_compose",
                    }
                },
            }
        }
    }
}

pub fn parse_intent_classification_json(raw: &str) -> Option<SelectionVoiceIntent> {
    let trimmed = raw.trim();
    if let Some(intent) = parse_intent_from_xml(trimmed) {
        return Some(intent);
    }
    let json = trimmed
        .find('{')
        .and_then(|start| trimmed.rfind('}').map(|end| &trimmed[start..=end]))
        .unwrap_or(trimmed);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
        if let Some(intent) = value.get("intent").and_then(|value| value.as_str()) {
            return parse_intent_word(intent);
        }
    }
    parse_intent_from_prose(trimmed)
}

pub fn selection_voice_instruction_looks_like_translation(instruction: &str) -> bool {
    let lower = instruction.to_lowercase();
    lower.contains("翻译")
        || lower.contains("译成")
        || lower.contains("译为")
        || lower.contains("translate")
        || lower.contains("translation")
}

fn language_label_from_fragment(fragment: &str) -> Option<String> {
    let token = fragment
        .trim()
        .split(['，', ',', '。', '.', ' ', '；', ';'])
        .next()
        .unwrap_or(fragment)
        .trim()
        .to_lowercase();
    if token.is_empty() {
        return None;
    }
    if token.contains("英文") || token.contains("英语") || token.contains("english") {
        return Some("English".to_string());
    }
    if token.contains("繁体") || token.contains("繁體") {
        return Some("繁體中文".to_string());
    }
    if token.contains("简体") || token.contains("簡體") || token.contains("中文") {
        return Some("简体中文".to_string());
    }
    if token.contains("日文") || token.contains("日语") || token.contains("japanese") {
        return Some("日本語".to_string());
    }
    if token.contains("韩文") || token.contains("韩语") || token.contains("korean") {
        return Some("한국어".to_string());
    }
    None
}

fn extract_translation_target_after_cue(instruction: &str) -> Option<String> {
    let lower = instruction.to_lowercase();
    for cue in [
        "翻译成",
        "译成",
        "译为",
        "翻译为",
        "翻譯成",
        "譯成",
        "translate to",
        "translate into",
        "translated to",
    ] {
        if let Some(index) = lower.find(cue) {
            let after = instruction[index + cue.len()..].trim();
            if let Some(language) = language_label_from_fragment(after) {
                return Some(language);
            }
        }
    }
    None
}

pub fn infer_selection_voice_translation_target(
    instruction: &str,
    preferences: &UserPreferences,
) -> String {
    if let Some(target) = extract_translation_target_after_cue(instruction) {
        return target;
    }
    let lower = instruction.to_lowercase();
    if lower.contains("日文") || lower.contains("日语") || lower.contains("japanese") {
        return "日本語".to_string();
    }
    if lower.contains("韩文") || lower.contains("韩语") || lower.contains("korean") {
        return "한국어".to_string();
    }
    if lower.contains("繁体") || lower.contains("繁體") {
        return "繁體中文".to_string();
    }
    if lower.contains("简体") || lower.contains("簡體") || lower.contains("中文") {
        return "简体中文".to_string();
    }
    if lower.contains("英文") || lower.contains("英语") || lower.contains("english") {
        return "English".to_string();
    }
    let configured = preferences.translation_target_language.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    match preferences.output_language_preference {
        OutputLanguagePreference::En => "English".to_string(),
        OutputLanguagePreference::Ja => "日本語".to_string(),
        OutputLanguagePreference::Ko => "한국어".to_string(),
        OutputLanguagePreference::ZhCn => "简体中文".to_string(),
        OutputLanguagePreference::ZhTw => "繁體中文".to_string(),
        OutputLanguagePreference::Auto => String::new(),
    }
}

pub fn clean_selection_voice_translation_output(raw: &str) -> String {
    let mut text = crate::output_cleaning::clean_json_llm_output(raw);
    loop {
        let trimmed = text.trim_start();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            if let Some((_, after)) = rest.split_once('\n') {
                text = after.to_string();
                continue;
            }
            if rest.starts_with("Processing") || rest.starts_with("处理") {
                text.clear();
                break;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            if let Some((_, after)) = rest.split_once('\n') {
                text = after.to_string();
                continue;
            }
        }
        break;
    }
    text.trim().to_string()
}

/// Strip common LLM preamble / fencing from compose drafts.
pub fn clean_selection_voice_compose_output(raw: &str) -> String {
    clean_selection_voice_translation_output(raw)
}

fn parse_intent_from_xml(raw: &str) -> Option<SelectionVoiceIntent> {
    let lower = raw.to_lowercase();
    let start = lower.find("<intent>")? + "<intent>".len();
    let end = lower[start..].find("</intent>")? + start;
    parse_intent_word(&raw[start..end])
}

fn parse_intent_word(raw: &str) -> Option<SelectionVoiceIntent> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "edit" | "editing" | "rewrite" | "imperative" | "command" => {
            Some(SelectionVoiceIntent::Edit)
        }
        "question" | "ask" | "qa" | "query" | "interrogative" => {
            Some(SelectionVoiceIntent::Question)
        }
        "compose" | "draft" | "write" | "help_me_write" | "help-me-write" => {
            Some(SelectionVoiceIntent::Compose)
        }
        _ => None,
    }
}

fn parse_intent_from_prose(raw: &str) -> Option<SelectionVoiceIntent> {
    let lower = raw.to_lowercase();
    let compact = lower
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == '.' || c == '。');
    match compact {
        "edit" | "editing" | "rewrite" | "imperative" | "command" | "编辑" | "执行" => {
            Some(SelectionVoiceIntent::Edit)
        }
        "question" | "ask" | "qa" | "query" | "interrogative" | "提问" | "询问" | "问句" => {
            Some(SelectionVoiceIntent::Question)
        }
        "compose" | "draft" | "write" | "帮我写" | "写作" | "成稿" => {
            Some(SelectionVoiceIntent::Compose)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(
        mode: SelectionVoiceIntentMode,
        instruction: &str,
    ) -> SelectionVoiceIntentClassification {
        classify_selection_voice_intent(
            mode,
            SelectionVoiceManualIntent::Question,
            &[],
            instruction,
            false,
        )
    }

    fn classify_empty(
        mode: SelectionVoiceIntentMode,
        instruction: &str,
    ) -> SelectionVoiceIntentClassification {
        classify_selection_voice_intent(
            mode,
            SelectionVoiceManualIntent::Question,
            &[],
            instruction,
            true,
        )
    }

    #[test]
    fn summary_and_translation_commands_are_edits() {
        assert_eq!(
            classify(SelectionVoiceIntentMode::Auto, "总结这段").intent,
            SelectionVoiceIntent::Edit
        );
        let result = classify(SelectionVoiceIntentMode::Auto, "把上面信息翻译成英文");
        assert_eq!(result.intent, SelectionVoiceIntent::Edit);
        assert_eq!(result.source, "auto_edit");
    }

    #[test]
    fn interrogatives_and_custom_cues_are_questions() {
        assert_eq!(
            classify(SelectionVoiceIntentMode::Heuristic, "这段话是什么意思？").intent,
            SelectionVoiceIntent::Question
        );
        let keywords = vec!["解读".to_string()];
        assert_eq!(
            classify_selection_voice_intent(
                SelectionVoiceIntentMode::Heuristic,
                SelectionVoiceManualIntent::Question,
                &keywords,
                "请解读这段文字",
                false,
            )
            .intent,
            SelectionVoiceIntent::Question
        );
    }

    #[test]
    fn legacy_edit_keywords_do_not_force_question() {
        let keywords = LEGACY_EDIT_KEYWORD_DEFAULTS
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            classify_selection_voice_intent(
                SelectionVoiceIntentMode::Heuristic,
                SelectionVoiceManualIntent::Question,
                &keywords,
                "把牵引改成迁移",
                false,
            )
            .intent,
            SelectionVoiceIntent::Edit
        );
    }

    #[test]
    fn empty_selection_affirmative_defaults_to_compose() {
        assert_eq!(
            classify_empty(SelectionVoiceIntentMode::Heuristic, "帮我写一封短邮件").intent,
            SelectionVoiceIntent::Compose
        );
        assert_eq!(
            classify_empty(SelectionVoiceIntentMode::Heuristic, "写一段会议纪要").intent,
            SelectionVoiceIntent::Compose
        );
    }

    #[test]
    fn empty_selection_questions_stay_questions() {
        assert_eq!(
            classify_empty(SelectionVoiceIntentMode::Heuristic, "今天是什么日子？").intent,
            SelectionVoiceIntent::Question
        );
    }

    #[test]
    fn non_empty_edit_and_question_keep_existing_routes() {
        assert_eq!(
            classify(SelectionVoiceIntentMode::Heuristic, "翻译成英文").intent,
            SelectionVoiceIntent::Edit
        );
        assert_eq!(
            classify(SelectionVoiceIntentMode::Heuristic, "什么意思").intent,
            SelectionVoiceIntent::Question
        );
    }

    #[test]
    fn strong_compose_cue_wins_over_question_shape() {
        assert_eq!(
            classify_empty(SelectionVoiceIntentMode::Heuristic, "能帮我写一封邮件吗？").intent,
            SelectionVoiceIntent::Compose
        );
        assert_eq!(
            classify(SelectionVoiceIntentMode::Heuristic, "能帮我写一封邮件吗？").intent,
            SelectionVoiceIntent::Compose
        );
    }

    #[test]
    fn parses_xml_json_and_prose_intents() {
        assert_eq!(
            parse_intent_classification_json("<intent>edit</intent>"),
            Some(SelectionVoiceIntent::Edit)
        );
        assert_eq!(
            parse_intent_classification_json(r#"{"intent":"question"}"#),
            Some(SelectionVoiceIntent::Question)
        );
        assert_eq!(
            parse_intent_classification_json("<intent>compose</intent>"),
            Some(SelectionVoiceIntent::Compose)
        );
        assert_eq!(
            parse_intent_classification_json("编辑"),
            Some(SelectionVoiceIntent::Edit)
        );
        assert_eq!(
            parse_intent_classification_json("帮我写"),
            Some(SelectionVoiceIntent::Compose)
        );
    }

    #[test]
    fn translation_target_is_taken_after_the_cue_not_from_the_source_language() {
        let preferences = UserPreferences::default();
        assert_eq!(
            infer_selection_voice_translation_target("把上面的英文翻译成中文。", &preferences),
            "简体中文"
        );
        assert_eq!(
            infer_selection_voice_translation_target("将上面的中文翻译成英文。", &preferences),
            "English"
        );
    }

    #[test]
    fn translation_output_removes_model_headings_without_touching_body_text() {
        assert_eq!(
            clean_selection_voice_translation_output("## Translation\nHello world"),
            "Hello world"
        );
        assert_eq!(
            clean_selection_voice_translation_output("# 结果\n正文"),
            "正文"
        );
    }
}
