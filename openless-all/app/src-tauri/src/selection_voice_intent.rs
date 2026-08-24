//! Intent routing for selection-voice sessions (issue #987 desktop MVP).
//!
//! Auto / Heuristic: interrogative → Question; otherwise → Edit (imperative /
//! affirmative / execution). Custom keywords are optional extra question cues.

use crate::types::{
    SelectionVoiceIntentMode, SelectionVoiceManualIntent, UserPreferences,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionVoiceIntent {
    Question,
    Edit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionVoiceIntentClassification {
    pub intent: SelectionVoiceIntent,
    pub source: &'static str,
}

/// Built-in question cues (substring match after lowercasing).
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

/// True when the instruction looks like a question (not an edit command).
pub fn looks_like_question_instruction(instruction: &str) -> bool {
    let trimmed = instruction.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.to_lowercase();
    let without_trail = normalized
        .trim_end_matches(|c: char| c == '.' || c == '。' || c == '!' || c == '！' || c.is_whitespace());
    if without_trail.ends_with('?') || without_trail.ends_with('？') {
        return true;
    }
    BUILTIN_QUESTION_CUES
        .iter()
        .any(|cue| normalized.contains(&cue.to_lowercase()))
}

/// Ambiguous short utterances with no question punctuation/cues — LLM may help in Auto.
pub fn intent_heuristic_is_ambiguous(instruction: &str) -> bool {
    let trimmed = instruction.trim();
    if trimmed.is_empty() {
        return true;
    }
    if looks_like_question_instruction(trimmed) {
        return false;
    }
    // Clear non-question with enough content → Edit without LLM.
    let chars = trimmed.chars().count();
    chars < 4
}

pub fn resolve_selection_voice_intent_heuristic(
    instruction_polished: &str,
    question_keywords: &[String],
) -> SelectionVoiceIntent {
    let normalized = instruction_polished.to_lowercase();
    if question_keywords.iter().any(|keyword| {
        let keyword = keyword.trim();
        !keyword.is_empty() && normalized.contains(&keyword.to_lowercase())
    }) {
        return SelectionVoiceIntent::Question;
    }
    if looks_like_question_instruction(&normalized) {
        SelectionVoiceIntent::Question
    } else {
        SelectionVoiceIntent::Edit
    }
}

/// Kept for callers that still check edit-like phrases (translation path, etc.).
pub fn looks_like_edit_instruction(instruction: &str) -> bool {
    !looks_like_question_instruction(instruction) && !instruction.trim().is_empty()
}

pub fn resolve_selection_voice_intent(
    prefs: &UserPreferences,
    instruction_polished: &str,
) -> SelectionVoiceIntentClassification {
    match prefs.selection_voice_intent_mode {
        SelectionVoiceIntentMode::Prompt => SelectionVoiceIntentClassification {
            intent: SelectionVoiceIntent::Question,
            source: "prompt_pending",
        },
        SelectionVoiceIntentMode::Manual => SelectionVoiceIntentClassification {
            intent: match prefs.selection_voice_manual_intent {
                SelectionVoiceManualIntent::Question => SelectionVoiceIntent::Question,
                SelectionVoiceManualIntent::Edit => SelectionVoiceIntent::Edit,
            },
            source: "manual",
        },
        SelectionVoiceIntentMode::Heuristic => SelectionVoiceIntentClassification {
            intent: resolve_selection_voice_intent_heuristic(
                instruction_polished,
                &prefs.selection_voice_edit_keywords,
            ),
            source: "heuristic",
        },
        SelectionVoiceIntentMode::Auto => {
            let intent = resolve_selection_voice_intent_heuristic(
                instruction_polished,
                &prefs.selection_voice_edit_keywords,
            );
            SelectionVoiceIntentClassification {
                intent,
                source: if intent == SelectionVoiceIntent::Question {
                    "auto_question"
                } else {
                    "auto_edit"
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
        if let Some(intent) = value.get("intent").and_then(|v| v.as_str()) {
            return match intent.trim().to_ascii_lowercase().as_str() {
                "edit" | "editing" | "rewrite" | "imperative" | "command" => {
                    Some(SelectionVoiceIntent::Edit)
                }
                "question" | "ask" | "qa" | "query" | "interrogative" => {
                    Some(SelectionVoiceIntent::Question)
                }
                _ => None,
            };
        }
    }
    parse_intent_from_prose(trimmed)
}

fn parse_intent_from_xml(raw: &str) -> Option<SelectionVoiceIntent> {
    let lower = raw.to_lowercase();
    let start = lower.find("<intent>")? + "<intent>".len();
    let end = lower[start..].find("</intent>")? + start;
    let intent = raw[start..end].trim().to_ascii_lowercase();
    match intent.as_str() {
        "edit" | "editing" | "rewrite" | "imperative" | "command" => {
            Some(SelectionVoiceIntent::Edit)
        }
        "question" | "ask" | "qa" | "interrogative" => Some(SelectionVoiceIntent::Question),
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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::UserPreferences;

    #[test]
    fn summary_is_edit_not_question() {
        let prefs = UserPreferences {
            selection_voice_intent_mode: SelectionVoiceIntentMode::Auto,
            selection_voice_edit_keywords: vec![],
            ..UserPreferences::default()
        };
        let result = resolve_selection_voice_intent(&prefs, "总结这段");
        assert_eq!(result.intent, SelectionVoiceIntent::Edit);
    }

    #[test]
    fn interrogative_routes_to_question() {
        let prefs = UserPreferences {
            selection_voice_intent_mode: SelectionVoiceIntentMode::Heuristic,
            selection_voice_edit_keywords: vec![],
            ..UserPreferences::default()
        };
        let result = resolve_selection_voice_intent(&prefs, "这段话是什么意思？");
        assert_eq!(result.intent, SelectionVoiceIntent::Question);
    }

    #[test]
    fn translate_imperative_is_edit() {
        let prefs = UserPreferences {
            selection_voice_intent_mode: SelectionVoiceIntentMode::Auto,
            selection_voice_edit_keywords: vec![],
            ..UserPreferences::default()
        };
        let result = resolve_selection_voice_intent(&prefs, "把上面信息翻译成英文");
        assert_eq!(result.intent, SelectionVoiceIntent::Edit);
        assert_eq!(result.source, "auto_edit");
    }

    #[test]
    fn custom_keywords_force_question() {
        let prefs = UserPreferences {
            selection_voice_intent_mode: SelectionVoiceIntentMode::Heuristic,
            selection_voice_edit_keywords: vec!["解读".into()],
            ..UserPreferences::default()
        };
        let result = resolve_selection_voice_intent(&prefs, "请解读这段文字");
        assert_eq!(result.intent, SelectionVoiceIntent::Question);
    }

    #[test]
    fn parses_xml_intent() {
        assert_eq!(
            parse_intent_classification_json("<intent>edit</intent>"),
            Some(SelectionVoiceIntent::Edit)
        );
        assert_eq!(
            parse_intent_classification_json("<intent>question</intent>"),
            Some(SelectionVoiceIntent::Question)
        );
    }
}
