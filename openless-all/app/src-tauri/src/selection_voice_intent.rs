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

pub fn resolve_selection_voice_intent_heuristic(
    instruction_polished: &str,
    keywords: &[String],
) -> SelectionVoiceIntent {
    let normalized = instruction_polished.to_lowercase();
    if keywords.iter().any(|keyword| {
        let keyword = keyword.trim();
        !keyword.is_empty() && normalized.contains(&keyword.to_lowercase())
    }) {
        SelectionVoiceIntent::Edit
    } else {
        SelectionVoiceIntent::Question
    }
}

pub fn resolve_selection_voice_intent(
    prefs: &UserPreferences,
    instruction_polished: &str,
) -> SelectionVoiceIntentClassification {
    match prefs.selection_voice_intent_mode {
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
        SelectionVoiceIntentMode::Auto => SelectionVoiceIntentClassification {
            intent: resolve_selection_voice_intent_heuristic(
                instruction_polished,
                &prefs.selection_voice_edit_keywords,
            ),
            source: "auto_heuristic_fallback",
        },
    }
}

pub fn parse_intent_classification_json(raw: &str) -> Option<SelectionVoiceIntent> {
    let trimmed = raw.trim();
    let json = trimmed
        .find('{')
        .and_then(|start| trimmed.rfind('}').map(|end| &trimmed[start..=end]))
        .unwrap_or(trimmed);
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let intent = value.get("intent")?.as_str()?;
    match intent {
        "edit" => Some(SelectionVoiceIntent::Edit),
        "question" => Some(SelectionVoiceIntent::Question),
        _ => None,
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
}
