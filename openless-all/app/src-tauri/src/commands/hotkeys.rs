use super::*;

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
    let mut prefs = coord.prefs().get();
    if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
        reject_dictation_qa_hotkey_overlap(&binding, qa_hotkey)?;
    }
    reject_dictation_translation_hotkey_overlap(&binding, &prefs.translation_hotkey)?;
    if let Some(switch_style) = prefs.switch_style_hotkey.as_ref() {
        reject_dictation_switch_style_hotkey_overlap(&binding, switch_style)?;
    }
    if let Some(open_app) = prefs.open_app_hotkey.as_ref() {
        reject_dictation_open_app_hotkey_overlap(&binding, open_app)?;
    }
    if let Some(less_computer) = prefs.coding_agent_voice_hotkey.as_ref() {
        reject_dictation_less_computer_hotkey_overlap(&binding, less_computer)?;
    }
    reject_existing_selection_polish_hotkey_overlap(&binding, &prefs)?;
    prefs.dictation_hotkey = binding;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_hotkey_binding();
    coord.update_combo_hotkey_binding();
    Ok(())
}

#[tauri::command]
pub fn set_translation_hotkey(
    coord: CoordinatorState<'_>,
    binding: ShortcutBinding,
) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())?;
    crate::shortcut_binding::reject_side_specific_non_dictation(&binding)?;
    let previous = coord.prefs().get();
    reject_dictation_translation_hotkey_overlap(&previous.dictation_hotkey, &binding)?;
    if let Some(qa_hotkey) = previous.qa_hotkey.as_ref() {
        reject_qa_translation_hotkey_overlap(qa_hotkey, &binding)?;
    }
    if let Some(switch_style) = previous.switch_style_hotkey.as_ref() {
        reject_translation_switch_style_hotkey_overlap(&binding, switch_style)?;
    }
    if let Some(open_app) = previous.open_app_hotkey.as_ref() {
        reject_translation_open_app_hotkey_overlap(&binding, open_app)?;
    }
    if let Some(less_computer) = previous.coding_agent_voice_hotkey.as_ref() {
        reject_translation_less_computer_hotkey_overlap(&binding, less_computer)?;
    }
    reject_existing_selection_polish_hotkey_overlap(&binding, &previous)?;
    let mut prefs = previous.clone();
    prefs.translation_hotkey = binding;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    if let Err(e) = coord.try_update_translation_hotkey_binding() {
        if let Err(rollback_err) = coord.prefs().set(previous) {
            log::warn!("[commands] 回滚翻译快捷键失败: {rollback_err}");
        }
        coord.update_translation_hotkey_binding();
        return Err(e);
    }
    Ok(())
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
    let mut prefs = coord.prefs().get();
    if let Some(binding) = binding.as_ref() {
        reject_dictation_switch_style_hotkey_overlap(&prefs.dictation_hotkey, binding)?;
        reject_translation_switch_style_hotkey_overlap(&prefs.translation_hotkey, binding)?;
        if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
            reject_qa_switch_style_hotkey_overlap(qa_hotkey, binding)?;
        }
        if let Some(open_app) = prefs.open_app_hotkey.as_ref() {
            reject_switch_style_open_app_hotkey_overlap(binding, open_app)?;
        }
        if let Some(less_computer) = prefs.coding_agent_voice_hotkey.as_ref() {
            reject_less_computer_switch_style_hotkey_overlap(less_computer, binding)?;
        }
        reject_existing_selection_polish_hotkey_overlap(binding, &prefs)?;
    }
    prefs.switch_style_hotkey = binding;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_switch_style_hotkey_binding();
    Ok(())
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
    let mut prefs = coord.prefs().get();
    if let Some(binding) = binding.as_ref() {
        reject_dictation_open_app_hotkey_overlap(&prefs.dictation_hotkey, binding)?;
        reject_translation_open_app_hotkey_overlap(&prefs.translation_hotkey, binding)?;
        if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
            reject_qa_open_app_hotkey_overlap(qa_hotkey, binding)?;
        }
        if let Some(switch_style) = prefs.switch_style_hotkey.as_ref() {
            reject_switch_style_open_app_hotkey_overlap(switch_style, binding)?;
        }
        if let Some(less_computer) = prefs.coding_agent_voice_hotkey.as_ref() {
            reject_less_computer_open_app_hotkey_overlap(less_computer, binding)?;
        }
        reject_existing_selection_polish_hotkey_overlap(binding, &prefs)?;
    }
    prefs.open_app_hotkey = binding;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_open_app_hotkey_binding();
    Ok(())
}

/// Set the Selection Polish global shortcut. The new binding is persisted first
/// so the coordinator sees it during registration; a registration failure
/// restores the exact previous preferences and listener state before returning.
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
    let previous = coord.prefs().get();
    if let Some(binding) = binding.as_ref() {
        reject_selection_polish_hotkey_collisions(binding, &previous)?;
    }
    let mut next = previous.clone();
    next.selection_polish_hotkey = binding;
    coord.prefs().set(next).map_err(|e| e.to_string())?;
    if let Err(error) = coord.try_update_selection_polish_hotkey_binding() {
        if let Err(rollback_error) = coord.prefs().set(previous) {
            return Err(format!(
                "{error}; additionally failed to restore previous Selection Polish shortcut: {rollback_error}"
            ));
        }
        coord.update_selection_polish_hotkey_binding();
        return Err(error);
    }
    Ok(())
}

fn reject_modifier_only_action_shortcut(binding: &ShortcutBinding) -> Result<(), String> {
    if binding.modifiers.is_empty()
        && (binding.primary.eq_ignore_ascii_case("shift")
            || crate::shortcut_binding::legacy_modifier_trigger(binding).is_some())
    {
        return Err("该快捷键需要使用组合键或非修饰主键".into());
    }
    Ok(())
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
    let mut prefs = coord.prefs().get();
    let shortcut = ShortcutBinding {
        primary: binding.primary.clone(),
        modifiers: binding.modifiers.clone(),
    };
    reject_bare_shift_dictation_shortcut(&shortcut)?;
    crate::combo_hotkey::validate_binding(&shortcut).map_err(|e| e.to_string())?;
    if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
        reject_dictation_qa_hotkey_overlap(&shortcut, qa_hotkey)?;
    }
    reject_dictation_translation_hotkey_overlap(&shortcut, &prefs.translation_hotkey)?;
    if let Some(switch_style) = prefs.switch_style_hotkey.as_ref() {
        reject_dictation_switch_style_hotkey_overlap(&shortcut, switch_style)?;
    }
    if let Some(open_app) = prefs.open_app_hotkey.as_ref() {
        reject_dictation_open_app_hotkey_overlap(&shortcut, open_app)?;
    }
    if let Some(less_computer) = prefs.coding_agent_voice_hotkey.as_ref() {
        reject_dictation_less_computer_hotkey_overlap(&shortcut, less_computer)?;
    }
    reject_existing_selection_polish_hotkey_overlap(&shortcut, &prefs)?;
    prefs.custom_combo_hotkey = Some(binding);
    prefs.dictation_hotkey = shortcut;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_hotkey_binding();
    coord.update_combo_hotkey_binding();
    Ok(())
}

pub(crate) fn reject_bare_shift_dictation_shortcut(
    binding: &ShortcutBinding,
) -> Result<(), String> {
    if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
        return Err("Shift 单键目前只能用于翻译快捷键".into());
    }
    Ok(())
}

pub(crate) fn sync_dictation_hotkey_legacy_fields(prefs: &mut UserPreferences) {
    if let Some(trigger) = crate::shortcut_binding::legacy_modifier_trigger(&prefs.dictation_hotkey)
    {
        prefs.hotkey.trigger = trigger;
        prefs.custom_combo_hotkey = None;
        return;
    }
    prefs.hotkey.trigger = crate::types::HotkeyTrigger::Custom;
    prefs.custom_combo_hotkey = if prefs.dictation_hotkey.primary.trim().is_empty() {
        None
    } else {
        Some(ComboBinding {
            primary: prefs.dictation_hotkey.primary.clone(),
            modifiers: prefs.dictation_hotkey.modifiers.clone(),
        })
    };
}

pub(crate) fn reject_dictation_qa_hotkey_overlap(
    dictation: &ShortcutBinding,
    qa: &ShortcutBinding,
) -> Result<(), String> {
    if shortcut_bindings_overlap(dictation, qa) {
        return Err("QA 快捷键不能和听写快捷键相同".into());
    }
    Ok(())
}

fn reject_hotkey_overlap(
    left: &ShortcutBinding,
    right: &ShortcutBinding,
    message: &'static str,
) -> Result<(), String> {
    if shortcut_bindings_overlap(left, right) {
        return Err(message.into());
    }
    Ok(())
}

pub(crate) fn reject_hotkey_collisions(prefs: &UserPreferences) -> Result<(), String> {
    reject_non_dictation_side_specific_shortcuts(prefs)?;
    // 停用（None）的 action 快捷键不参与任何冲突检测。
    let switch_style = prefs.switch_style_hotkey.as_ref();
    let open_app = prefs.open_app_hotkey.as_ref();
    let less_computer = prefs.coding_agent_voice_hotkey.as_ref();
    if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
        reject_dictation_qa_hotkey_overlap(&prefs.dictation_hotkey, qa_hotkey)?;
        reject_qa_translation_hotkey_overlap(qa_hotkey, &prefs.translation_hotkey)?;
        if let Some(less_computer) = less_computer {
            reject_qa_less_computer_hotkey_overlap(qa_hotkey, less_computer)?;
        }
        if let Some(switch_style) = switch_style {
            reject_qa_switch_style_hotkey_overlap(qa_hotkey, switch_style)?;
        }
        if let Some(open_app) = open_app {
            reject_qa_open_app_hotkey_overlap(qa_hotkey, open_app)?;
        }
    }
    reject_dictation_translation_hotkey_overlap(
        &prefs.dictation_hotkey,
        &prefs.translation_hotkey,
    )?;
    if let Some(less_computer) = less_computer {
        reject_dictation_less_computer_hotkey_overlap(&prefs.dictation_hotkey, less_computer)?;
        reject_translation_less_computer_hotkey_overlap(&prefs.translation_hotkey, less_computer)?;
    }
    if let Some(switch_style) = switch_style {
        reject_dictation_switch_style_hotkey_overlap(&prefs.dictation_hotkey, switch_style)?;
        reject_translation_switch_style_hotkey_overlap(&prefs.translation_hotkey, switch_style)?;
        if let Some(less_computer) = less_computer {
            reject_less_computer_switch_style_hotkey_overlap(less_computer, switch_style)?;
        }
    }
    if let Some(open_app) = open_app {
        reject_dictation_open_app_hotkey_overlap(&prefs.dictation_hotkey, open_app)?;
        reject_translation_open_app_hotkey_overlap(&prefs.translation_hotkey, open_app)?;
        if let Some(less_computer) = less_computer {
            reject_less_computer_open_app_hotkey_overlap(less_computer, open_app)?;
        }
    }
    if let (Some(switch_style), Some(open_app)) = (switch_style, open_app) {
        reject_switch_style_open_app_hotkey_overlap(switch_style, open_app)?;
    }
    if let Some(selection_polish) = prefs.selection_polish_hotkey.as_ref() {
        reject_selection_polish_hotkey_collisions(selection_polish, prefs)?;
    }
    Ok(())
}

pub(crate) fn reject_selection_polish_hotkey_collisions(
    selection_polish: &ShortcutBinding,
    prefs: &UserPreferences,
) -> Result<(), String> {
    reject_hotkey_overlap(
        selection_polish,
        &prefs.dictation_hotkey,
        "选区润色快捷键不能和听写快捷键相同",
    )?;
    reject_hotkey_overlap(
        selection_polish,
        &prefs.translation_hotkey,
        "选区润色快捷键不能和翻译快捷键相同",
    )?;
    if let Some(qa) = prefs.qa_hotkey.as_ref() {
        reject_hotkey_overlap(selection_polish, qa, "选区润色快捷键不能和 QA 快捷键相同")?;
    }
    if let Some(switch_style) = prefs.switch_style_hotkey.as_ref() {
        reject_hotkey_overlap(
            selection_polish,
            switch_style,
            "选区润色快捷键不能和切换风格快捷键相同",
        )?;
    }
    if let Some(open_app) = prefs.open_app_hotkey.as_ref() {
        reject_hotkey_overlap(
            selection_polish,
            open_app,
            "选区润色快捷键不能和打开应用快捷键相同",
        )?;
    }
    if let Some(less_computer) = prefs.coding_agent_voice_hotkey.as_ref() {
        reject_hotkey_overlap(
            selection_polish,
            less_computer,
            "选区润色快捷键不能和 Less Computer 快捷键相同",
        )?;
    }
    Ok(())
}

pub(crate) fn reject_existing_selection_polish_hotkey_overlap(
    binding: &ShortcutBinding,
    prefs: &UserPreferences,
) -> Result<(), String> {
    if let Some(selection_polish) = prefs.selection_polish_hotkey.as_ref() {
        reject_hotkey_overlap(
            binding,
            selection_polish,
            "该快捷键不能和选区润色快捷键相同",
        )?;
    }
    Ok(())
}

pub(crate) fn reject_non_dictation_side_specific_shortcuts(
    prefs: &UserPreferences,
) -> Result<(), String> {
    crate::shortcut_binding::reject_side_specific_non_dictation(&prefs.translation_hotkey)?;
    if let Some(binding) = prefs.qa_hotkey.as_ref() {
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
    }
    if let Some(binding) = prefs.switch_style_hotkey.as_ref() {
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
    }
    if let Some(binding) = prefs.open_app_hotkey.as_ref() {
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
    }
    if let Some(binding) = prefs.selection_polish_hotkey.as_ref() {
        crate::shortcut_binding::validate_binding(binding).map_err(|e| e.to_string())?;
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
        reject_bare_shift_dictation_shortcut(binding)?;
    }
    if let Some(binding) = prefs.coding_agent_voice_hotkey.as_ref() {
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
    }
    Ok(())
}

pub(crate) fn reject_dictation_translation_hotkey_overlap(
    dictation: &ShortcutBinding,
    translation: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(dictation, translation, "翻译快捷键不能和听写快捷键相同")
}

fn reject_dictation_switch_style_hotkey_overlap(
    dictation: &ShortcutBinding,
    switch_style: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        dictation,
        switch_style,
        "切换风格快捷键不能和听写快捷键相同",
    )
}

fn reject_dictation_open_app_hotkey_overlap(
    dictation: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(dictation, open_app, "打开应用快捷键不能和听写快捷键相同")
}

fn reject_dictation_less_computer_hotkey_overlap(
    dictation: &ShortcutBinding,
    less_computer: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        dictation,
        less_computer,
        "Less Computer 快捷键不能和听写快捷键相同",
    )
}

pub(crate) fn reject_qa_translation_hotkey_overlap(
    qa: &ShortcutBinding,
    translation: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(qa, translation, "翻译快捷键不能和 QA 快捷键相同")
}

pub(crate) fn reject_qa_switch_style_hotkey_overlap(
    qa: &ShortcutBinding,
    switch_style: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(qa, switch_style, "切换风格快捷键不能和 QA 快捷键相同")
}

pub(crate) fn reject_qa_open_app_hotkey_overlap(
    qa: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(qa, open_app, "打开应用快捷键不能和 QA 快捷键相同")
}

pub(crate) fn reject_qa_less_computer_hotkey_overlap(
    qa: &ShortcutBinding,
    less_computer: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        qa,
        less_computer,
        "Less Computer 快捷键不能和 QA 快捷键相同",
    )
}

fn reject_translation_switch_style_hotkey_overlap(
    translation: &ShortcutBinding,
    switch_style: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        translation,
        switch_style,
        "切换风格快捷键不能和翻译快捷键相同",
    )
}

fn reject_translation_open_app_hotkey_overlap(
    translation: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(translation, open_app, "打开应用快捷键不能和翻译快捷键相同")
}

fn reject_translation_less_computer_hotkey_overlap(
    translation: &ShortcutBinding,
    less_computer: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        translation,
        less_computer,
        "Less Computer 快捷键不能和翻译快捷键相同",
    )
}

fn reject_switch_style_open_app_hotkey_overlap(
    switch_style: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        switch_style,
        open_app,
        "打开应用快捷键不能和切换风格快捷键相同",
    )
}

fn reject_less_computer_switch_style_hotkey_overlap(
    less_computer: &ShortcutBinding,
    switch_style: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        less_computer,
        switch_style,
        "Less Computer 快捷键不能和切换风格快捷键相同",
    )
}

fn reject_less_computer_open_app_hotkey_overlap(
    less_computer: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        less_computer,
        open_app,
        "Less Computer 快捷键不能和打开应用快捷键相同",
    )
}

fn shortcut_bindings_overlap(left: &ShortcutBinding, right: &ShortcutBinding) -> bool {
    crate::shortcut_binding::bindings_overlap(left, right)
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
    fn existing_selection_polish_hotkey_rejects_another_action_binding() {
        let selection = key("RightControl");
        let prefs = UserPreferences {
            selection_polish_hotkey: Some(selection.clone()),
            ..Default::default()
        };

        assert!(reject_existing_selection_polish_hotkey_overlap(&selection, &prefs).is_err());
        assert!(reject_existing_selection_polish_hotkey_overlap(&key("P"), &prefs).is_ok());
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
