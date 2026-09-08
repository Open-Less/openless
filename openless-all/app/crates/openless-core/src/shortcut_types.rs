//! Pure shortcut compatibility helpers shared by host adapters.

use std::collections::BTreeSet;

use crate::shared_types::{
    ComboBinding, HotkeyTrigger, ShortcutBinding, StylePackHotkey, UserPreferences,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ShortcutBindingError {
    #[error("不支持的修饰键: {0}")]
    UnsupportedModifier(String),
    #[error("不支持的主键: {0}")]
    UnsupportedKey(String),
}

const SIDE_MODIFIER_TAGS: &[&str] = &[
    "cmd-left",
    "cmd-right",
    "ctrl-left",
    "ctrl-right",
    "alt-left",
    "alt-right",
    "shift-left",
    "shift-right",
    "super-left",
    "super-right",
];

pub fn normalize_side_modifier_tag(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "super-left" => "cmd-left".into(),
        "super-right" => "cmd-right".into(),
        tag => tag.to_string(),
    }
}

pub fn is_side_specific_modifier_tag(raw: &str) -> bool {
    SIDE_MODIFIER_TAGS.contains(&normalize_side_modifier_tag(raw).as_str())
}

pub fn binding_requires_side_aware_hook(binding: &ShortcutBinding) -> bool {
    !binding.modifiers.is_empty()
        && binding
            .modifiers
            .iter()
            .any(|tag| is_side_specific_modifier_tag(tag))
}

pub const SIDE_SPECIFIC_NON_DICTATION_MSG: &str =
    "Side-specific modifier shortcuts are only supported for dictation start/stop.";

pub fn reject_side_specific_non_dictation(binding: &ShortcutBinding) -> Result<(), String> {
    if binding_requires_side_aware_hook(binding) {
        return Err(SIDE_SPECIFIC_NON_DICTATION_MSG.to_string());
    }
    Ok(())
}

fn normalize_modifier_tag(raw: &str) -> String {
    let tag = raw.trim().to_ascii_lowercase();
    if is_side_specific_modifier_tag(&tag) {
        return tag;
    }
    #[cfg(target_os = "windows")]
    {
        if matches!(tag.as_str(), "cmd" | "command") {
            return "ctrl".to_string();
        }
    }
    tag
}

fn physical_class_from_generic_tag(tag: &str) -> String {
    match tag {
        "ctrl" | "control" => "Control".to_string(),
        "alt" | "option" | "opt" => "Alt".to_string(),
        "shift" => "Shift".to_string(),
        #[cfg(target_os = "windows")]
        "cmd" | "command" => "Control".to_string(),
        #[cfg(target_os = "windows")]
        "super" | "meta" | "win" => "Super".to_string(),
        #[cfg(not(target_os = "windows"))]
        "cmd" | "command" | "super" | "meta" | "win" => "Super".to_string(),
        other => other.to_string(),
    }
}

fn physical_modifier_class(raw: &str) -> String {
    let tag = normalize_side_modifier_tag(raw);
    if is_side_specific_modifier_tag(&tag) {
        if tag.starts_with("cmd-") || tag.starts_with("super-") {
            return "Super".to_string();
        }
        if tag.starts_with("ctrl-") {
            return "Control".to_string();
        }
        if tag.starts_with("alt-") {
            return "Alt".to_string();
        }
        if tag.starts_with("shift-") {
            return "Shift".to_string();
        }
    }
    physical_class_from_generic_tag(&normalize_modifier_tag(raw))
}

fn physical_modifier_set(binding: &ShortcutBinding) -> BTreeSet<String> {
    binding
        .modifiers
        .iter()
        .map(|raw| physical_modifier_class(raw))
        .collect()
}

fn normalize_primary(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|character| !matches!(character, ' ' | '-' | '_'))
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn legacy_modifier_trigger(binding: &ShortcutBinding) -> Option<HotkeyTrigger> {
    if !binding.modifiers.is_empty() {
        return None;
    }
    match normalize_primary(&binding.primary).as_str() {
        "rightoption" | "rightalt" => Some(HotkeyTrigger::RightOption),
        "leftoption" | "leftalt" => Some(HotkeyTrigger::LeftOption),
        "rightcontrol" | "rightctrl" => Some(HotkeyTrigger::RightControl),
        "leftcontrol" | "leftctrl" => Some(HotkeyTrigger::LeftControl),
        "rightcommand" | "rightcmd" | "rightsuper" | "rightmeta" => {
            Some(HotkeyTrigger::RightCommand)
        }
        "leftcommand" | "leftcmd" | "leftsuper" | "leftmeta" => Some(HotkeyTrigger::LeftCommand),
        "leftshift" | "shiftleft" => Some(HotkeyTrigger::LeftShift),
        "rightshift" | "shiftright" => Some(HotkeyTrigger::RightShift),
        "fn" | "function" => Some(HotkeyTrigger::Fn),
        "mediaplaypause" | "mediaplay" | "playpause" => Some(HotkeyTrigger::MediaPlayPause),
        _ => None,
    }
}

pub fn bindings_overlap(left: &ShortcutBinding, right: &ShortcutBinding) -> bool {
    let left_legacy = legacy_modifier_trigger(left);
    let right_legacy = legacy_modifier_trigger(right);
    match (left_legacy, right_legacy) {
        (Some(left), Some(right)) => left == right,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => {
            if normalize_primary(&left.primary) != normalize_primary(&right.primary) {
                return false;
            }
            let left_side = binding_requires_side_aware_hook(left);
            let right_side = binding_requires_side_aware_hook(right);
            if left_side && right_side {
                let left_modifiers: BTreeSet<String> = left
                    .modifiers
                    .iter()
                    .map(|raw| normalize_side_modifier_tag(raw))
                    .collect();
                let right_modifiers: BTreeSet<String> = right
                    .modifiers
                    .iter()
                    .map(|raw| normalize_side_modifier_tag(raw))
                    .collect();
                return left_modifiers == right_modifiers;
            }
            physical_modifier_set(left) == physical_modifier_set(right)
        }
    }
}

pub fn binding_from_legacy_trigger(trigger: HotkeyTrigger) -> ShortcutBinding {
    let primary = match trigger {
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => "RightOption",
        HotkeyTrigger::LeftOption => "LeftOption",
        HotkeyTrigger::RightControl => "RightControl",
        HotkeyTrigger::LeftControl => "LeftControl",
        HotkeyTrigger::RightCommand => "RightCommand",
        HotkeyTrigger::LeftCommand => "LeftCommand",
        HotkeyTrigger::LeftShift => "LeftShift",
        HotkeyTrigger::RightShift => "RightShift",
        HotkeyTrigger::Fn => "Fn",
        HotkeyTrigger::MediaPlayPause => "MediaPlayPause",
        HotkeyTrigger::Custom => "RightOption",
    };
    ShortcutBinding {
        primary: primary.into(),
        modifiers: Vec::new(),
    }
}

pub fn validate_shortcut_binding(binding: &ShortcutBinding) -> Result<(), ShortcutBindingError> {
    if legacy_modifier_trigger(binding).is_some() {
        return Ok(());
    }
    if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
        return Ok(());
    }

    validate_primary(&binding.primary)?;
    for raw in &binding.modifiers {
        if binding_requires_side_aware_hook(binding) {
            if !is_side_specific_modifier_tag(raw) {
                return Err(ShortcutBindingError::UnsupportedModifier(raw.clone()));
            }
            continue;
        }
        let normalized = normalize_modifier_tag(raw);
        if !matches!(
            normalized.as_str(),
            "cmd"
                | "command"
                | "super"
                | "meta"
                | "win"
                | "ctrl"
                | "control"
                | "alt"
                | "option"
                | "opt"
                | "shift"
        ) {
            return Err(ShortcutBindingError::UnsupportedModifier(normalized));
        }
    }
    Ok(())
}

fn validate_primary(raw: &str) -> Result<(), ShortcutBindingError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ShortcutBindingError::UnsupportedKey("(空)".into()));
    }
    if trimmed.chars().count() == 1
        && trimmed
            .chars()
            .next()
            .is_some_and(is_supported_shortcut_character)
    {
        return Ok(());
    }
    let upper = trimmed.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "ENTER"
            | "RETURN"
            | "TAB"
            | "ESC"
            | "ESCAPE"
            | "SPACE"
            | "BACKSPACE"
            | "DELETE"
            | "DEL"
            | "HOME"
            | "END"
            | "PAGEUP"
            | "PAGEDOWN"
            | "ARROWUP"
            | "UP"
            | "ARROWDOWN"
            | "DOWN"
            | "ARROWLEFT"
            | "LEFT"
            | "ARROWRIGHT"
            | "RIGHT"
            | "F1"
            | "F2"
            | "F3"
            | "F4"
            | "F5"
            | "F6"
            | "F7"
            | "F8"
            | "F9"
            | "F10"
            | "F11"
            | "F12"
    ) {
        return Ok(());
    }
    Err(ShortcutBindingError::UnsupportedKey(trimmed.to_string()))
}

fn is_supported_shortcut_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            ';' | ':'
                | ','
                | '<'
                | '.'
                | '>'
                | '/'
                | '?'
                | '\\'
                | '|'
                | '['
                | '{'
                | ']'
                | '}'
                | '\''
                | '"'
                | '`'
                | '~'
                | '-'
                | '_'
                | '='
                | '+'
                | ' '
                | '!'
                | '@'
                | '#'
                | '$'
                | '%'
                | '^'
                | '&'
                | '*'
                | '('
                | ')'
        )
}

pub fn reject_modifier_only_action_shortcut(binding: &ShortcutBinding) -> Result<(), String> {
    if binding.modifiers.is_empty()
        && (binding.primary.eq_ignore_ascii_case("shift")
            || legacy_modifier_trigger(binding).is_some())
    {
        return Err("该快捷键需要使用组合键或非修饰主键".into());
    }
    Ok(())
}

pub fn reject_bare_shift_dictation_shortcut(binding: &ShortcutBinding) -> Result<(), String> {
    if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
        return Err("Shift 单键目前只能用于翻译快捷键".into());
    }
    Ok(())
}

pub fn sync_dictation_hotkey_legacy_fields(preferences: &mut UserPreferences) {
    if let Some(trigger) = legacy_modifier_trigger(&preferences.dictation_hotkey) {
        preferences.hotkey.trigger = trigger;
        preferences.custom_combo_hotkey = None;
        return;
    }
    preferences.hotkey.trigger = HotkeyTrigger::Custom;
    preferences.custom_combo_hotkey = if preferences.dictation_hotkey.primary.trim().is_empty() {
        None
    } else {
        Some(ComboBinding {
            primary: preferences.dictation_hotkey.primary.clone(),
            modifiers: preferences.dictation_hotkey.modifiers.clone(),
        })
    };
}

fn reject_overlap(
    left: &ShortcutBinding,
    right: &ShortcutBinding,
    message: &'static str,
) -> Result<(), String> {
    if bindings_overlap(left, right) {
        return Err(message.into());
    }
    Ok(())
}

pub fn reject_dictation_qa_hotkey_overlap(
    dictation: &ShortcutBinding,
    qa: &ShortcutBinding,
) -> Result<(), String> {
    reject_overlap(dictation, qa, "QA 快捷键不能和听写快捷键相同")
}

pub fn reject_dictation_translation_hotkey_overlap(
    dictation: &ShortcutBinding,
    translation: &ShortcutBinding,
) -> Result<(), String> {
    reject_overlap(dictation, translation, "翻译快捷键不能和听写快捷键相同")
}

pub fn reject_qa_translation_hotkey_overlap(
    qa: &ShortcutBinding,
    translation: &ShortcutBinding,
) -> Result<(), String> {
    reject_overlap(qa, translation, "翻译快捷键不能和 QA 快捷键相同")
}

pub fn reject_qa_switch_style_hotkey_overlap(
    qa: &ShortcutBinding,
    switch_style: &ShortcutBinding,
) -> Result<(), String> {
    reject_overlap(qa, switch_style, "切换风格快捷键不能和 QA 快捷键相同")
}

pub fn reject_qa_open_app_hotkey_overlap(
    qa: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_overlap(qa, open_app, "打开应用快捷键不能和 QA 快捷键相同")
}

pub fn reject_qa_less_computer_hotkey_overlap(
    qa: &ShortcutBinding,
    less_computer: &ShortcutBinding,
) -> Result<(), String> {
    reject_overlap(
        qa,
        less_computer,
        "Less Computer 快捷键不能和 QA 快捷键相同",
    )
}

pub fn reject_non_dictation_side_specific_shortcuts(
    preferences: &UserPreferences,
) -> Result<(), String> {
    reject_side_specific_non_dictation(&preferences.translation_hotkey)?;
    for binding in [
        preferences.qa_hotkey.as_ref(),
        preferences.switch_style_hotkey.as_ref(),
        preferences.open_app_hotkey.as_ref(),
        preferences.coding_agent_voice_hotkey.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        reject_side_specific_non_dictation(binding)?;
    }
    if let Some(binding) = preferences.selection_polish_hotkey.as_ref() {
        validate_shortcut_binding(binding).map_err(|error| error.to_string())?;
        reject_side_specific_non_dictation(binding)?;
        reject_bare_shift_dictation_shortcut(binding)?;
    }
    Ok(())
}

pub fn reject_selection_polish_hotkey_collisions(
    selection_polish: &ShortcutBinding,
    preferences: &UserPreferences,
) -> Result<(), String> {
    reject_overlap(
        selection_polish,
        &preferences.dictation_hotkey,
        "选区润色快捷键不能和听写快捷键相同",
    )?;
    reject_overlap(
        selection_polish,
        &preferences.translation_hotkey,
        "选区润色快捷键不能和翻译快捷键相同",
    )?;
    if let Some(binding) = preferences.qa_hotkey.as_ref() {
        reject_overlap(
            selection_polish,
            binding,
            "选区润色快捷键不能和 QA 快捷键相同",
        )?;
    }
    if let Some(binding) = preferences.switch_style_hotkey.as_ref() {
        reject_overlap(
            selection_polish,
            binding,
            "选区润色快捷键不能和切换风格快捷键相同",
        )?;
    }
    if let Some(binding) = preferences.open_app_hotkey.as_ref() {
        reject_overlap(
            selection_polish,
            binding,
            "选区润色快捷键不能和打开应用快捷键相同",
        )?;
    }
    if let Some(binding) = preferences.coding_agent_voice_hotkey.as_ref() {
        reject_overlap(
            selection_polish,
            binding,
            "选区润色快捷键不能和 Less Computer 快捷键相同",
        )?;
    }
    Ok(())
}

fn reject_style_pack_hotkey_overlap_with_others(
    binding: &ShortcutBinding,
    preferences: &UserPreferences,
) -> Result<(), String> {
    reject_overlap(
        binding,
        &preferences.dictation_hotkey,
        "风格快捷键不能和听写快捷键相同",
    )?;
    reject_overlap(
        binding,
        &preferences.translation_hotkey,
        "风格快捷键不能和翻译快捷键相同",
    )?;
    let optional_bindings = [
        (
            preferences.qa_hotkey.as_ref(),
            "风格快捷键不能和 QA 快捷键相同",
        ),
        (
            preferences.switch_style_hotkey.as_ref(),
            "风格快捷键不能和切换风格快捷键相同",
        ),
        (
            preferences.open_app_hotkey.as_ref(),
            "风格快捷键不能和打开应用快捷键相同",
        ),
        (
            preferences.coding_agent_voice_hotkey.as_ref(),
            "风格快捷键不能和 Less Computer 快捷键相同",
        ),
        (
            preferences.selection_polish_hotkey.as_ref(),
            "风格快捷键不能和选区润色快捷键相同",
        ),
    ];
    for (other, message) in optional_bindings {
        if let Some(other) = other {
            reject_overlap(binding, other, message)?;
        }
    }
    Ok(())
}

pub fn reject_style_pack_hotkey_conflicts(
    hotkeys: &[StylePackHotkey],
    preferences: &UserPreferences,
) -> Result<(), String> {
    for (index, entry) in hotkeys.iter().enumerate() {
        if entry.pack_id.trim().is_empty() {
            return Err("风格快捷键必须选择一个风格包".into());
        }
        validate_shortcut_binding(&entry.binding).map_err(|error| error.to_string())?;
        reject_side_specific_non_dictation(&entry.binding)?;
        reject_modifier_only_action_shortcut(&entry.binding)?;
        for other in &hotkeys[..index] {
            if other.pack_id == entry.pack_id {
                return Err("同一个风格包只能绑定一个快捷键".into());
            }
            reject_overlap(
                &other.binding,
                &entry.binding,
                "两个风格快捷键不能使用相同按键",
            )?;
        }
        reject_style_pack_hotkey_overlap_with_others(&entry.binding, preferences)?;
    }
    Ok(())
}

/// Resolve shortcut conflicts in a full settings document without rejecting
/// unrelated preference changes.
///
/// Dictation is the highest-priority binding and is never changed. Other
/// bindings are considered in product-priority order: an invalid or colliding
/// value first falls back to its previous value, then is disabled when no safe
/// fallback exists. Translation is required, so it falls back to its default.
/// Style-pack shortcuts are lowest priority and are reconciled last.
pub fn reconcile_hotkey_collisions(
    preferences: &mut UserPreferences,
    previous: &UserPreferences,
) -> usize {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum NonCoreHotkey {
        Translation,
        Qa,
        SwitchStyle,
        OpenApp,
        SelectionPolish,
        LessComputer,
    }

    impl NonCoreHotkey {
        fn get(self, preferences: &UserPreferences) -> Option<ShortcutBinding> {
            match self {
                Self::Translation => Some(preferences.translation_hotkey.clone()),
                Self::Qa => preferences.qa_hotkey.clone(),
                Self::SwitchStyle => preferences.switch_style_hotkey.clone(),
                Self::OpenApp => preferences.open_app_hotkey.clone(),
                Self::SelectionPolish => preferences.selection_polish_hotkey.clone(),
                Self::LessComputer => preferences.coding_agent_voice_hotkey.clone(),
            }
        }

        fn set(self, preferences: &mut UserPreferences, value: Option<ShortcutBinding>) {
            match self {
                Self::Translation => {
                    if let Some(value) = value {
                        preferences.translation_hotkey = value;
                    }
                }
                Self::Qa => preferences.qa_hotkey = value,
                Self::SwitchStyle => preferences.switch_style_hotkey = value,
                Self::OpenApp => preferences.open_app_hotkey = value,
                Self::SelectionPolish => preferences.selection_polish_hotkey = value,
                Self::LessComputer => preferences.coding_agent_voice_hotkey = value,
            }
        }

        fn binding_is_valid(self, binding: &ShortcutBinding) -> bool {
            if reject_side_specific_non_dictation(binding).is_err() {
                return false;
            }
            match self {
                Self::SelectionPolish => {
                    validate_shortcut_binding(binding).is_ok()
                        && reject_bare_shift_dictation_shortcut(binding).is_ok()
                }
                _ => true,
            }
        }
    }

    const ORDER: [NonCoreHotkey; 6] = [
        NonCoreHotkey::Translation,
        NonCoreHotkey::Qa,
        NonCoreHotkey::SwitchStyle,
        NonCoreHotkey::OpenApp,
        NonCoreHotkey::SelectionPolish,
        NonCoreHotkey::LessComputer,
    ];
    let mut higher = vec![preferences.dictation_hotkey.clone()];
    let mut adjusted = 0;
    for key in ORDER {
        let Some(current) = key.get(preferences) else {
            continue;
        };
        let collides = higher.iter().any(|held| bindings_overlap(held, &current));
        if !collides && key.binding_is_valid(&current) {
            higher.push(current);
            continue;
        }
        let fallback = key.get(previous).filter(|candidate| {
            !higher.iter().any(|held| bindings_overlap(held, candidate))
                && key.binding_is_valid(candidate)
        });
        let resolved = if key == NonCoreHotkey::Translation && fallback.is_none() {
            Some(UserPreferences::default().translation_hotkey)
        } else {
            fallback
        };
        key.set(preferences, resolved.clone());
        adjusted += 1;
        if let Some(value) = resolved {
            higher.push(value);
        }
    }

    let mut kept = Vec::<StylePackHotkey>::new();
    for entry in &preferences.style_pack_hotkeys {
        let candidate_is_valid = |candidate: &StylePackHotkey| {
            !candidate.pack_id.trim().is_empty()
                && validate_shortcut_binding(&candidate.binding).is_ok()
                && reject_side_specific_non_dictation(&candidate.binding).is_ok()
                && reject_modifier_only_action_shortcut(&candidate.binding).is_ok()
                && !kept.iter().any(|held| {
                    held.pack_id == candidate.pack_id
                        || bindings_overlap(&held.binding, &candidate.binding)
                })
                && !higher
                    .iter()
                    .any(|held| bindings_overlap(held, &candidate.binding))
        };
        if candidate_is_valid(entry) {
            kept.push(entry.clone());
            continue;
        }
        adjusted += 1;
        if let Some(fallback) = previous
            .style_pack_hotkeys
            .iter()
            .find(|old| old.pack_id == entry.pack_id)
            .filter(|old| candidate_is_valid(old))
        {
            kept.push(fallback.clone());
        }
    }
    if kept != preferences.style_pack_hotkeys {
        preferences.style_pack_hotkeys = kept;
    }
    adjusted
}

pub fn reject_hotkey_collisions(preferences: &UserPreferences) -> Result<(), String> {
    reject_non_dictation_side_specific_shortcuts(preferences)?;
    let switch_style = preferences.switch_style_hotkey.as_ref();
    let open_app = preferences.open_app_hotkey.as_ref();
    let less_computer = preferences.coding_agent_voice_hotkey.as_ref();
    if let Some(qa) = preferences.qa_hotkey.as_ref() {
        reject_dictation_qa_hotkey_overlap(&preferences.dictation_hotkey, qa)?;
        reject_qa_translation_hotkey_overlap(qa, &preferences.translation_hotkey)?;
        if let Some(binding) = less_computer {
            reject_qa_less_computer_hotkey_overlap(qa, binding)?;
        }
        if let Some(binding) = switch_style {
            reject_qa_switch_style_hotkey_overlap(qa, binding)?;
        }
        if let Some(binding) = open_app {
            reject_qa_open_app_hotkey_overlap(qa, binding)?;
        }
    }
    reject_dictation_translation_hotkey_overlap(
        &preferences.dictation_hotkey,
        &preferences.translation_hotkey,
    )?;
    if let Some(binding) = less_computer {
        reject_overlap(
            &preferences.dictation_hotkey,
            binding,
            "Less Computer 快捷键不能和听写快捷键相同",
        )?;
        reject_overlap(
            &preferences.translation_hotkey,
            binding,
            "Less Computer 快捷键不能和翻译快捷键相同",
        )?;
    }
    if let Some(binding) = switch_style {
        reject_overlap(
            &preferences.dictation_hotkey,
            binding,
            "切换风格快捷键不能和听写快捷键相同",
        )?;
        reject_overlap(
            &preferences.translation_hotkey,
            binding,
            "切换风格快捷键不能和翻译快捷键相同",
        )?;
        if let Some(less_computer) = less_computer {
            reject_overlap(
                less_computer,
                binding,
                "Less Computer 快捷键不能和切换风格快捷键相同",
            )?;
        }
    }
    if let Some(binding) = open_app {
        reject_overlap(
            &preferences.dictation_hotkey,
            binding,
            "打开应用快捷键不能和听写快捷键相同",
        )?;
        reject_overlap(
            &preferences.translation_hotkey,
            binding,
            "打开应用快捷键不能和翻译快捷键相同",
        )?;
        if let Some(less_computer) = less_computer {
            reject_overlap(
                less_computer,
                binding,
                "Less Computer 快捷键不能和打开应用快捷键相同",
            )?;
        }
    }
    if let (Some(switch_style), Some(open_app)) = (switch_style, open_app) {
        reject_overlap(
            switch_style,
            open_app,
            "打开应用快捷键不能和切换风格快捷键相同",
        )?;
    }
    if let Some(binding) = preferences.selection_polish_hotkey.as_ref() {
        reject_selection_polish_hotkey_collisions(binding, preferences)?;
    }
    reject_style_pack_hotkey_conflicts(&preferences.style_pack_hotkeys, preferences)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(primary: &str, modifiers: &[&str]) -> ShortcutBinding {
        ShortcutBinding {
            primary: primary.to_string(),
            modifiers: modifiers.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    #[test]
    fn validates_shared_shortcut_grammar_without_a_native_hotkey_crate() {
        assert!(validate_shortcut_binding(&combo("D", &["cmd", "shift"])).is_ok());
        assert!(validate_shortcut_binding(&combo("?", &["shift"])).is_ok());
        assert!(validate_shortcut_binding(&combo("F12", &[])).is_ok());
        assert_eq!(
            validate_shortcut_binding(&combo("D", &["hyper"])),
            Err(ShortcutBindingError::UnsupportedModifier("hyper".into()))
        );
        assert_eq!(
            validate_shortcut_binding(&combo("VolumeUp", &[])),
            Err(ShortcutBindingError::UnsupportedKey("VolumeUp".into()))
        );
    }

    #[test]
    fn side_specific_rules_are_shared_by_all_hosts() {
        let side_specific = combo("D", &["cmd-left", "shift-right"]);
        assert!(validate_shortcut_binding(&side_specific).is_ok());
        assert!(binding_requires_side_aware_hook(&side_specific));
        assert_eq!(
            reject_side_specific_non_dictation(&side_specific).unwrap_err(),
            SIDE_SPECIFIC_NON_DICTATION_MSG
        );
        assert!(validate_shortcut_binding(&combo("D", &["cmd-left", "shift"])).is_err());
    }

    #[test]
    fn overlap_and_legacy_conversion_have_one_shared_implementation() {
        assert!(bindings_overlap(
            &combo("D", &["ctrl-left"]),
            &combo("D", &["ctrl"])
        ));
        assert!(!bindings_overlap(
            &combo("D", &["ctrl-left"]),
            &combo("D", &["ctrl", "shift"])
        ));
        let binding = binding_from_legacy_trigger(HotkeyTrigger::RightControl);
        assert_eq!(
            legacy_modifier_trigger(&binding),
            Some(HotkeyTrigger::RightControl)
        );
    }

    #[test]
    fn settings_reconciliation_disables_a_legacy_selection_collision() {
        let previous = UserPreferences {
            dictation_hotkey: ShortcutBinding {
                primary: "RightAlt".into(),
                modifiers: vec![],
            },
            selection_polish_hotkey: Some(ShortcutBinding {
                primary: "RightAlt".into(),
                modifiers: vec![],
            }),
            ..UserPreferences::default()
        };
        let mut next = previous.clone();

        let adjusted = reconcile_hotkey_collisions(&mut next, &previous);

        assert_eq!(adjusted, 1);
        assert!(next.selection_polish_hotkey.is_none());
        assert!(reject_hotkey_collisions(&next).is_ok());
    }

    #[test]
    fn settings_reconciliation_restores_non_core_conflicts_and_invalid_bindings() {
        let previous = UserPreferences {
            qa_hotkey: Some(combo("E", &["ctrl", "shift"])),
            ..UserPreferences::default()
        };
        let mut next = previous.clone();
        next.qa_hotkey = Some(combo("Shift", &[]));
        next.translation_hotkey = combo("D", &["cmd-left"]);

        let adjusted = reconcile_hotkey_collisions(&mut next, &previous);

        assert_eq!(adjusted, 2);
        assert_eq!(next.qa_hotkey, previous.qa_hotkey);
        assert_eq!(next.translation_hotkey, previous.translation_hotkey);
        assert!(reject_hotkey_collisions(&next).is_ok());
    }

    #[test]
    fn settings_reconciliation_treats_style_pack_shortcuts_as_lowest_priority() {
        let previous = UserPreferences::default();
        let mut next = previous.clone();
        next.style_pack_hotkeys = vec![
            StylePackHotkey {
                pack_id: "custom.one".into(),
                binding: next.dictation_hotkey.clone(),
            },
            StylePackHotkey {
                pack_id: "custom.two".into(),
                binding: combo("K", &["ctrl", "shift"]),
            },
            StylePackHotkey {
                pack_id: "custom.three".into(),
                binding: combo("K", &["ctrl", "shift"]),
            },
        ];

        let adjusted = reconcile_hotkey_collisions(&mut next, &previous);

        assert_eq!(adjusted, 2);
        assert_eq!(next.style_pack_hotkeys.len(), 1);
        assert_eq!(next.style_pack_hotkeys[0].pack_id, "custom.two");
        assert!(reject_hotkey_collisions(&next).is_ok());
    }
}
