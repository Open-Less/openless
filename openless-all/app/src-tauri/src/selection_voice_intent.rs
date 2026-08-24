//! Intent routing for selection-voice sessions (issue #987 desktop MVP).

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

/// Built-in edit cues used when Auto LLM classification fails or returns prose.
pub const BUILTIN_EDIT_CUES: &[&str] = &[
    "翻译",
    "译成",
    "译为",
    "改成",
    "改写",
    "替换",
    "润色",
    "批量",
    "格式",
    "删掉",
    "删除",
    "加上",
    "改为",
    "换成",
    "translate",
    "translation",
    "rewrite",
    "replace",
    "edit",
];

pub fn resolve_selection_voice_intent_heuristic(
    instruction_polished: &str,
    keywords: &[String],
) -> SelectionVoiceIntent {
    let normalized = instruction_polished.to_lowercase();
    if keywords.iter().any(|keyword| {
        let keyword = keyword.trim();
        !keyword.is_empty() && normalized.contains(&keyword.to_lowercase())
    }) {
        return SelectionVoiceIntent::Edit;
    }
    if looks_like_edit_instruction(&normalized) {
        SelectionVoiceIntent::Edit
    } else {
        SelectionVoiceIntent::Question
    }
}

pub fn looks_like_edit_instruction(instruction: &str) -> bool {
    let normalized = instruction.to_lowercase();
    BUILTIN_EDIT_CUES
        .iter()
        .any(|cue| normalized.contains(&cue.to_lowercase()))
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
            // Prefer built-in/heuristic cues before falling back to Question.
            // LLM refinement happens in resolve_intent_with_optional_llm.
            let intent = resolve_selection_voice_intent_heuristic(
                instruction_polished,
                &prefs.selection_voice_edit_keywords,
            );
            SelectionVoiceIntentClassification {
                intent,
                source: if intent == SelectionVoiceIntent::Edit {
                    "auto_heuristic"
                } else {
                    "auto_default"
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
                "edit" | "editing" | "rewrite" | "translate" | "translation" => {
                    Some(SelectionVoiceIntent::Edit)
                }
                "question" | "ask" | "qa" | "query" => Some(SelectionVoiceIntent::Question),
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
        "edit" | "editing" | "rewrite" | "translate" => Some(SelectionVoiceIntent::Edit),
        "question" | "ask" | "qa" => Some(SelectionVoiceIntent::Question),
        _ => None,
    }
}

fn parse_intent_from_prose(raw: &str) -> Option<SelectionVoiceIntent> {
    let lower = raw.to_lowercase();
    // Models sometimes reply with a short label instead of JSON.
    let compact = lower
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == '.' || c == '。');
    match compact {
        "edit" | "editing" | "rewrite" | "translate" | "translation" | "编辑" | "修改" | "翻译" => {
            Some(SelectionVoiceIntent::Edit)
        }
        "question" | "ask" | "qa" | "query" | "提问" | "询问" | "解释" => {
            Some(SelectionVoiceIntent::Question)
        }
        _ => {
            if looks_like_edit_instruction(&lower)
                && (lower.contains("intent") || lower.starts_with("translate") || lower.contains("编辑"))
            {
                Some(SelectionVoiceIntent::Edit)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::UserPreferences;

    #[test]
    fn heuristic_routes_edit_keywords() {
        let prefs = UserPreferences {
            selection_voice_intent_mode: SelectionVoiceIntentMode::Heuristic,
            selection_voice_edit_keywords: vec!["翻译".into(), "替换".into()],
            ..UserPreferences::default()
        };
        let result = resolve_selection_voice_intent(&prefs, "请把邮箱批量替换成公司域名");
        assert_eq!(result.intent, SelectionVoiceIntent::Edit);
    }

    #[test]
    fn heuristic_defaults_to_question() {
        let prefs = UserPreferences {
            selection_voice_intent_mode: SelectionVoiceIntentMode::Heuristic,
            selection_voice_edit_keywords: vec!["翻译".into()],
            ..UserPreferences::default()
        };
        let result = resolve_selection_voice_intent(&prefs, "这段话是什么意思");
        assert_eq!(result.intent, SelectionVoiceIntent::Question);
    }

    #[test]
    fn auto_routes_translate_as_edit_without_llm() {
        let prefs = UserPreferences {
            selection_voice_intent_mode: SelectionVoiceIntentMode::Auto,
            selection_voice_edit_keywords: vec![],
            ..UserPreferences::default()
        };
        let result = resolve_selection_voice_intent(&prefs, "把上面信息翻译成英文");
        assert_eq!(result.intent, SelectionVoiceIntent::Edit);
        assert_eq!(result.source, "auto_heuristic");
    }

    #[test]
    fn parses_xml_intent_and_prose_translate() {
        assert_eq!(
            parse_intent_classification_json("<intent>edit</intent>"),
            Some(SelectionVoiceIntent::Edit)
        );
        assert_eq!(
            parse_intent_classification_json("Translate the above text to English"),
            Some(SelectionVoiceIntent::Edit)
        );
    }
}
