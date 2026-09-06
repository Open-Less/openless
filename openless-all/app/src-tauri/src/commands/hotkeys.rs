use super::*;

pub(crate) use openless_core::{
    reconcile_hotkey_collisions, reject_bare_shift_dictation_shortcut,
    reject_dictation_qa_hotkey_overlap, reject_dictation_translation_hotkey_overlap,
    reject_hotkey_collisions, reject_modifier_only_action_shortcut,
    reject_non_dictation_side_specific_shortcuts, reject_style_pack_hotkey_conflicts,
    sync_dictation_hotkey_legacy_fields,
};

#[tauri::command]
pub fn validate_shortcut_binding(binding: ShortcutBinding) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_dictation_hotkey(
    coord: CoordinatorState<'_>,
    binding: ShortcutBinding,
) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())?;
    reject_bare_shift_dictation_shortcut(&binding)?;
    let mut prefs = coord.backend().get_preferences();
    prefs.dictation_hotkey = binding;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    reject_hotkey_collisions(&prefs)?;
    super::settings::persist_strict_settings(&coord, prefs)
}

#[tauri::command]
pub fn set_translation_hotkey(
    coord: CoordinatorState<'_>,
    binding: ShortcutBinding,
) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())?;
    crate::shortcut_binding::reject_side_specific_non_dictation(&binding)?;
    let mut prefs = coord.backend().get_preferences();
    prefs.translation_hotkey = binding;
    reject_hotkey_collisions(&prefs)?;
    super::settings::persist_strict_settings(&coord, prefs)
}

/// 设置「切换风格」全局快捷键。`binding == None`（前端传 null）= 停用：清空绑定并
/// 反注册全局键。镜像 `set_qa_hotkey` 的 `Option=None` 停用模式（issue #576）。
#[tauri::command]
pub fn set_switch_style_hotkey(
    coord: CoordinatorState<'_>,
    binding: Option<ShortcutBinding>,
) -> Result<(), String> {
    if let Some(binding) = binding.as_ref() {
        crate::shortcut_binding::validate_binding(binding).map_err(|e| e.to_string())?;
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
        reject_modifier_only_action_shortcut(binding)?;
    }
    let mut prefs = coord.backend().get_preferences();
    prefs.switch_style_hotkey = binding;
    reject_hotkey_collisions(&prefs)?;
    super::settings::persist_strict_settings(&coord, prefs)
}

/// 设置「唤起 App」全局快捷键。`binding == None`（前端传 null）= 停用（同上）。
#[tauri::command]
pub fn set_open_app_hotkey(
    coord: CoordinatorState<'_>,
    binding: Option<ShortcutBinding>,
) -> Result<(), String> {
    if let Some(binding) = binding.as_ref() {
        crate::shortcut_binding::validate_binding(binding).map_err(|e| e.to_string())?;
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
        reject_modifier_only_action_shortcut(binding)?;
    }
    let mut prefs = coord.backend().get_preferences();
    prefs.open_app_hotkey = binding;
    reject_hotkey_collisions(&prefs)?;
    super::settings::persist_strict_settings(&coord, prefs)
}

/// 设置 Selection Polish 全局快捷键。Core 先产生显式 effect target；Tauri
/// 注册成功后才持久化，失败则按 receipt 恢复旧监听器且不写偏好。
/// 选区润色为桌面（Windows-first）工作流，mobile 不注册。
#[cfg(not(mobile))]
#[tauri::command]
pub fn set_selection_polish_hotkey(
    coord: CoordinatorState<'_>,
    binding: Option<ShortcutBinding>,
) -> Result<(), String> {
    if let Some(binding) = binding.as_ref() {
        crate::shortcut_binding::validate_binding(binding).map_err(|e| e.to_string())?;
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
        reject_bare_shift_dictation_shortcut(binding)?;
    }
    let mut next = coord.backend().get_preferences();
    next.selection_polish_hotkey = binding;
    reject_hotkey_collisions(&next)?;
    super::settings::persist_strict_settings(&coord, next)
}

/// 整表替换风格包直达快捷键（issue #759）。前端任何增删改都发全量列表，
/// 校验通过才落库并热更新全局键注册；失败时旧绑定原样保留。
#[tauri::command]
pub fn set_style_pack_hotkeys(
    coord: CoordinatorState<'_>,
    hotkeys: Vec<StylePackHotkey>,
) -> Result<(), String> {
    let mut preferences = coord.backend().get_preferences();
    reject_style_pack_hotkey_conflicts(&hotkeys, &preferences)?;
    preferences.style_pack_hotkeys = hotkeys;
    super::settings::persist_strict_settings(&coord, preferences)
}

#[tauri::command]
pub fn validate_combo_hotkey(binding: ComboBinding) -> Result<(), String> {
    let shortcut = ShortcutBinding {
        primary: binding.primary,
        modifiers: binding.modifiers,
    };
    reject_bare_shift_dictation_shortcut(&shortcut)?;
    crate::combo_hotkey::validate_binding(&shortcut).map_err(|e| e.to_string())
}

/// 设置自定义录音组合键并热更新 monitor。
#[tauri::command]
pub fn set_combo_hotkey(coord: CoordinatorState<'_>, binding: ComboBinding) -> Result<(), String> {
    let mut prefs = coord.backend().get_preferences();
    let shortcut = ShortcutBinding {
        primary: binding.primary.clone(),
        modifiers: binding.modifiers.clone(),
    };
    reject_bare_shift_dictation_shortcut(&shortcut)?;
    crate::combo_hotkey::validate_binding(&shortcut).map_err(|e| e.to_string())?;
    prefs.custom_combo_hotkey = Some(binding);
    prefs.dictation_hotkey = shortcut;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    reject_hotkey_collisions(&prefs)?;
    super::settings::persist_strict_settings(&coord, prefs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(primary: &str) -> ShortcutBinding {
        ShortcutBinding {
            primary: primary.into(),
            modifiers: vec![],
        }
    }

    #[test]
    fn each_action_hotkey_collides_with_less_computer() {
        let lc = key("LeftControl");
        let mut prefs = UserPreferences {
            dictation_hotkey: key("A"),
            translation_hotkey: key("B"),
            qa_hotkey: Some(key("C")),
            switch_style_hotkey: Some(key("D")),
            open_app_hotkey: Some(key("E")),
            coding_agent_voice_hotkey: Some(lc.clone()),
            ..Default::default()
        };
        // 基线全不同 → 通过。
        assert!(reject_hotkey_collisions(&prefs).is_ok());

        prefs.dictation_hotkey = lc.clone();
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.dictation_hotkey = key("A");

        prefs.translation_hotkey = lc.clone();
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.translation_hotkey = key("B");

        prefs.qa_hotkey = Some(lc.clone());
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.qa_hotkey = Some(key("C"));

        prefs.switch_style_hotkey = Some(lc.clone());
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.switch_style_hotkey = Some(key("D"));

        prefs.open_app_hotkey = Some(lc.clone());
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.open_app_hotkey = Some(key("E"));

        // 复位后再次全不同 → 通过。
        assert!(reject_hotkey_collisions(&prefs).is_ok());
    }

    fn style_hotkey(pack_id: &str, primary: &str) -> StylePackHotkey {
        StylePackHotkey {
            pack_id: pack_id.into(),
            binding: ShortcutBinding {
                primary: primary.into(),
                modifiers: vec!["alt".into()],
            },
        }
    }

    #[test]
    fn style_pack_hotkeys_reject_duplicates_and_overlaps() {
        let prefs = UserPreferences {
            dictation_hotkey: key("A"),
            ..Default::default()
        };
        // 基线：两条不同包、不同键 → 通过。
        assert!(reject_style_pack_hotkey_conflicts(
            &[
                style_hotkey("builtin.raw", "1"),
                style_hotkey("imported.x", "2")
            ],
            &prefs,
        )
        .is_ok());
        // 同一个包绑两条 → 拒绝。
        assert!(reject_style_pack_hotkey_conflicts(
            &[
                style_hotkey("builtin.raw", "1"),
                style_hotkey("builtin.raw", "2")
            ],
            &prefs,
        )
        .is_err());
        // 两条绑同一个键 → 拒绝。
        assert!(reject_style_pack_hotkey_conflicts(
            &[
                style_hotkey("builtin.raw", "1"),
                style_hotkey("imported.x", "1")
            ],
            &prefs,
        )
        .is_err());
        // 空 pack_id → 拒绝。
        assert!(reject_style_pack_hotkey_conflicts(&[style_hotkey("", "1")], &prefs).is_err());
        // 与听写键重叠 → 拒绝。
        let clash = StylePackHotkey {
            pack_id: "builtin.raw".into(),
            binding: key("A"),
        };
        assert!(reject_style_pack_hotkey_conflicts(&[clash], &prefs).is_err());
    }

    #[test]
    fn reject_hotkey_collisions_covers_style_pack_hotkeys_against_every_owner() {
        let style_binding = style_hotkey("builtin.raw", "1").binding;
        let mut prefs = UserPreferences {
            dictation_hotkey: key("A"),
            translation_hotkey: key("B"),
            qa_hotkey: Some(key("C")),
            switch_style_hotkey: Some(key("D")),
            open_app_hotkey: Some(key("E")),
            coding_agent_voice_hotkey: Some(key("F")),
            selection_polish_hotkey: Some(key("G")),
            style_pack_hotkeys: vec![StylePackHotkey {
                pack_id: "builtin.raw".into(),
                binding: style_binding.clone(),
            }],
            ..Default::default()
        };
        assert!(reject_hotkey_collisions(&prefs).is_ok());

        prefs.dictation_hotkey = style_binding.clone();
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.dictation_hotkey = key("A");

        prefs.translation_hotkey = style_binding.clone();
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.translation_hotkey = key("B");

        prefs.qa_hotkey = Some(style_binding.clone());
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.qa_hotkey = Some(key("C"));

        prefs.switch_style_hotkey = Some(style_binding.clone());
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.switch_style_hotkey = Some(key("D"));

        prefs.open_app_hotkey = Some(style_binding.clone());
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.open_app_hotkey = Some(key("E"));

        prefs.coding_agent_voice_hotkey = Some(style_binding.clone());
        assert!(reject_hotkey_collisions(&prefs).is_err());
        prefs.coding_agent_voice_hotkey = Some(key("F"));

        prefs.selection_polish_hotkey = Some(style_binding);
        assert!(reject_hotkey_collisions(&prefs).is_err());
    }

    #[test]
    fn selection_polish_hotkey_collides_with_existing_shortcuts() {
        let binding = key("RightControl");
        let prefs = UserPreferences {
            dictation_hotkey: binding.clone(),
            selection_polish_hotkey: Some(binding),
            ..Default::default()
        };
        assert!(reject_hotkey_collisions(&prefs).is_err());
    }

    #[test]
    fn side_specific_dictation_overlaps_generic_qa_hotkey() {
        let mut prefs = UserPreferences {
            dictation_hotkey: ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd-left".into()],
            },
            qa_hotkey: Some(ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd".into()],
            }),
            ..Default::default()
        };
        #[cfg(target_os = "windows")]
        {
            assert!(reject_hotkey_collisions(&prefs).is_ok());
            prefs.qa_hotkey = Some(ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["super".into()],
            });
            assert!(reject_hotkey_collisions(&prefs).is_err());
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(reject_hotkey_collisions(&prefs).is_err());
        }
        prefs.qa_hotkey = Some(ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        });
        assert!(reject_hotkey_collisions(&prefs).is_ok());
    }

    #[test]
    fn rejects_side_specific_qa_hotkey_on_save() {
        let prefs = UserPreferences {
            qa_hotkey: Some(ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd-left".into()],
            }),
            ..Default::default()
        };
        assert!(reject_non_dictation_side_specific_shortcuts(&prefs).is_err());
    }

    #[test]
    fn rejects_side_specific_selection_polish_hotkey_on_save() {
        let prefs = UserPreferences {
            selection_polish_hotkey: Some(ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd-right".into()],
            }),
            ..Default::default()
        };
        assert!(reject_non_dictation_side_specific_shortcuts(&prefs).is_err());
    }

    #[test]
    fn accepts_side_specific_dictation_hotkey_on_save() {
        let prefs = UserPreferences {
            dictation_hotkey: ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd-left".into()],
            },
            ..Default::default()
        };
        assert!(reject_non_dictation_side_specific_shortcuts(&prefs).is_ok());
    }
}
