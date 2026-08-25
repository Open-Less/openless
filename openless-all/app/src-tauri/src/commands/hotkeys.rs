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
    prefs.dictation_hotkey = binding;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    reject_hotkey_collisions(&prefs)?;
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
    let mut prefs = previous.clone();
    prefs.translation_hotkey = binding;
    reject_hotkey_collisions(&prefs)?;
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
    prefs.switch_style_hotkey = binding;
    reject_hotkey_collisions(&prefs)?;
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
    prefs.open_app_hotkey = binding;
    reject_hotkey_collisions(&prefs)?;
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
    let mut next = previous.clone();
    next.selection_polish_hotkey = binding;
    reject_hotkey_collisions(&next)?;
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

/// 整表替换风格包直达快捷键（issue #759）。前端任何增删改都发全量列表，
/// 校验通过才落库并热更新全局键注册；失败时旧绑定原样保留。
#[tauri::command]
pub fn set_style_pack_hotkeys(
    coord: CoordinatorState<'_>,
    hotkeys: Vec<StylePackHotkey>,
) -> Result<(), String> {
    persist_style_pack_hotkeys(&**coord, hotkeys)
}

trait StylePackHotkeyWriter {
    fn read_style_pack_hotkey_preferences(&self) -> UserPreferences;
    fn write_style_pack_hotkey_preferences(&self, prefs: UserPreferences) -> Result<(), String>;
    fn try_refresh_style_pack_hotkeys(&self) -> Result<(), String>;
}

impl StylePackHotkeyWriter for Coordinator {
    fn read_style_pack_hotkey_preferences(&self) -> UserPreferences {
        self.prefs().get()
    }

    fn write_style_pack_hotkey_preferences(&self, prefs: UserPreferences) -> Result<(), String> {
        self.prefs().set(prefs).map_err(|error| error.to_string())
    }

    fn try_refresh_style_pack_hotkeys(&self) -> Result<(), String> {
        self.try_update_style_pack_hotkey_bindings()
    }
}

fn persist_style_pack_hotkeys<T: StylePackHotkeyWriter>(
    writer: &T,
    hotkeys: Vec<StylePackHotkey>,
) -> Result<(), String> {
    let previous = writer.read_style_pack_hotkey_preferences();
    reject_style_pack_hotkey_conflicts(&hotkeys, &previous)?;
    let mut next = previous.clone();
    next.style_pack_hotkeys = hotkeys;

    writer.write_style_pack_hotkey_preferences(next)?;
    if let Err(registration_error) = writer.try_refresh_style_pack_hotkeys() {
        if let Err(rollback_error) = writer.write_style_pack_hotkey_preferences(previous) {
            return Err(format!(
                "{registration_error}; additionally failed to restore previous style pack shortcut preferences: {rollback_error}"
            ));
        }
        if let Err(rollback_error) = writer.try_refresh_style_pack_hotkeys() {
            return Err(format!(
                "{registration_error}; additionally failed to restore previous style pack shortcut listeners: {rollback_error}"
            ));
        }
        return Err(registration_error);
    }
    Ok(())
}

/// 风格包快捷键集合的全量校验：逐条格式校验 + 集合内去重（同包一条、同键一条）
/// + 与其它所有快捷键互斥。
pub(crate) fn reject_style_pack_hotkey_conflicts(
    hotkeys: &[StylePackHotkey],
    prefs: &UserPreferences,
) -> Result<(), String> {
    for (index, entry) in hotkeys.iter().enumerate() {
        if entry.pack_id.trim().is_empty() {
            return Err("风格快捷键必须选择一个风格包".into());
        }
        crate::shortcut_binding::validate_binding(&entry.binding).map_err(|e| e.to_string())?;
        crate::shortcut_binding::reject_side_specific_non_dictation(&entry.binding)?;
        reject_modifier_only_action_shortcut(&entry.binding)?;
        for other in &hotkeys[..index] {
            if other.pack_id == entry.pack_id {
                return Err("同一个风格包只能绑定一个快捷键".into());
            }
            reject_hotkey_overlap(
                &other.binding,
                &entry.binding,
                "两个风格快捷键不能使用相同按键",
            )?;
        }
        reject_style_pack_hotkey_overlap_with_others(&entry.binding, prefs)?;
    }
    Ok(())
}

fn reject_style_pack_hotkey_overlap_with_others(
    binding: &ShortcutBinding,
    prefs: &UserPreferences,
) -> Result<(), String> {
    reject_hotkey_overlap(
        binding,
        &prefs.dictation_hotkey,
        "风格快捷键不能和听写快捷键相同",
    )?;
    reject_hotkey_overlap(
        binding,
        &prefs.translation_hotkey,
        "风格快捷键不能和翻译快捷键相同",
    )?;
    if let Some(qa) = prefs.qa_hotkey.as_ref() {
        reject_hotkey_overlap(binding, qa, "风格快捷键不能和 QA 快捷键相同")?;
    }
    if let Some(switch_style) = prefs.switch_style_hotkey.as_ref() {
        reject_hotkey_overlap(
            binding,
            switch_style,
            "风格快捷键不能和切换风格快捷键相同",
        )?;
    }
    if let Some(open_app) = prefs.open_app_hotkey.as_ref() {
        reject_hotkey_overlap(binding, open_app, "风格快捷键不能和打开应用快捷键相同")?;
    }
    if let Some(less_computer) = prefs.coding_agent_voice_hotkey.as_ref() {
        reject_hotkey_overlap(
            binding,
            less_computer,
            "风格快捷键不能和 Less Computer 快捷键相同",
        )?;
    }
    if let Some(selection_polish) = prefs.selection_polish_hotkey.as_ref() {
        reject_hotkey_overlap(
            binding,
            selection_polish,
            "风格快捷键不能和选区润色快捷键相同",
        )?;
    }
    Ok(())
}

pub(crate) fn reject_modifier_only_action_shortcut(binding: &ShortcutBinding) -> Result<(), String> {
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
    prefs.custom_combo_hotkey = Some(binding);
    prefs.dictation_hotkey = shortcut;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    reject_hotkey_collisions(&prefs)?;
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
    reject_style_pack_hotkey_conflicts(&prefs.style_pack_hotkeys, prefs)?;
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

    struct MockStylePackHotkeyWriter {
        prefs: Mutex<UserPreferences>,
        write_results: Mutex<std::collections::VecDeque<Result<(), String>>>,
        refresh_results: Mutex<std::collections::VecDeque<Result<(), String>>>,
        write_count: Mutex<usize>,
        refresh_count: Mutex<usize>,
    }

    impl MockStylePackHotkeyWriter {
        fn new(
            prefs: UserPreferences,
            write_results: impl IntoIterator<Item = Result<(), String>>,
            refresh_results: impl IntoIterator<Item = Result<(), String>>,
        ) -> Self {
            Self {
                prefs: Mutex::new(prefs),
                write_results: Mutex::new(write_results.into_iter().collect()),
                refresh_results: Mutex::new(refresh_results.into_iter().collect()),
                write_count: Mutex::new(0),
                refresh_count: Mutex::new(0),
            }
        }
    }

    impl StylePackHotkeyWriter for MockStylePackHotkeyWriter {
        fn read_style_pack_hotkey_preferences(&self) -> UserPreferences {
            self.prefs.lock().clone()
        }

        fn write_style_pack_hotkey_preferences(
            &self,
            prefs: UserPreferences,
        ) -> Result<(), String> {
            *self.write_count.lock() += 1;
            let result = self.write_results.lock().pop_front().unwrap_or(Ok(()));
            if result.is_ok() {
                *self.prefs.lock() = prefs;
            }
            result
        }

        fn try_refresh_style_pack_hotkeys(&self) -> Result<(), String> {
            *self.refresh_count.lock() += 1;
            self.refresh_results.lock().pop_front().unwrap_or(Ok(()))
        }
    }

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
    fn style_pack_hotkey_transaction_restores_previous_state_when_registration_fails() {
        let previous_hotkey = style_hotkey("builtin.raw", "1");
        let previous = UserPreferences {
            style_pack_hotkeys: vec![previous_hotkey.clone()],
            ..Default::default()
        };
        let writer = MockStylePackHotkeyWriter::new(
            previous.clone(),
            [Ok(()), Ok(())],
            [Err("new registration failed".into()), Ok(())],
        );

        let error = persist_style_pack_hotkeys(&writer, vec![style_hotkey("builtin.raw", "2")])
            .unwrap_err();

        assert_eq!(error, "new registration failed");
        assert_eq!(
            writer.prefs.lock().style_pack_hotkeys,
            vec![previous_hotkey]
        );
        assert_eq!(*writer.write_count.lock(), 2);
        assert_eq!(*writer.refresh_count.lock(), 2);
    }

    #[test]
    fn style_pack_hotkey_transaction_persists_and_registers_valid_candidate() {
        let writer = MockStylePackHotkeyWriter::new(UserPreferences::default(), [Ok(())], [Ok(())]);
        let expected = vec![style_hotkey("builtin.raw", "1")];

        persist_style_pack_hotkeys(&writer, expected.clone()).unwrap();

        assert_eq!(writer.prefs.lock().style_pack_hotkeys, expected);
        assert_eq!(*writer.write_count.lock(), 1);
        assert_eq!(*writer.refresh_count.lock(), 1);
    }

    #[test]
    fn style_pack_hotkey_transaction_ignores_unrelated_existing_collision() {
        let existing_binding = key("RightAlt");
        let previous = UserPreferences {
            dictation_hotkey: existing_binding.clone(),
            selection_polish_hotkey: Some(existing_binding),
            ..Default::default()
        };
        let writer = MockStylePackHotkeyWriter::new(previous, [Ok(())], [Ok(())]);
        let expected = vec![style_hotkey("builtin.raw", "1")];

        persist_style_pack_hotkeys(&writer, expected.clone()).unwrap();

        assert_eq!(writer.prefs.lock().style_pack_hotkeys, expected);
        assert_eq!(*writer.write_count.lock(), 1);
        assert_eq!(*writer.refresh_count.lock(), 1);
    }

    #[test]
    fn style_pack_hotkey_transaction_rejects_invalid_candidate_without_side_effects() {
        let previous = UserPreferences::default();
        let writer = MockStylePackHotkeyWriter::new(
            previous.clone(),
            std::iter::empty(),
            std::iter::empty(),
        );

        let error = persist_style_pack_hotkeys(&writer, vec![style_hotkey("", "1")]).unwrap_err();

        assert!(error.contains("必须选择一个风格包"));
        assert_eq!(
            writer.prefs.lock().style_pack_hotkeys,
            previous.style_pack_hotkeys
        );
        assert_eq!(*writer.write_count.lock(), 0);
        assert_eq!(*writer.refresh_count.lock(), 0);
    }

    #[test]
    fn style_pack_hotkey_transaction_reports_listener_restore_failure() {
        let previous = UserPreferences {
            style_pack_hotkeys: vec![style_hotkey("builtin.raw", "1")],
            ..Default::default()
        };
        let writer = MockStylePackHotkeyWriter::new(
            previous.clone(),
            [Ok(()), Ok(())],
            [
                Err("new registration failed".into()),
                Err("old registration failed".into()),
            ],
        );

        let error = persist_style_pack_hotkeys(&writer, vec![style_hotkey("builtin.raw", "2")])
            .unwrap_err();

        assert!(error.contains("new registration failed"));
        assert!(error.contains("old registration failed"));
        assert_eq!(
            writer.prefs.lock().style_pack_hotkeys,
            previous.style_pack_hotkeys
        );
    }

    #[test]
    fn style_pack_hotkey_transaction_reports_preferences_restore_failure() {
        let writer = MockStylePackHotkeyWriter::new(
            UserPreferences::default(),
            [Ok(()), Err("preferences rollback failed".into())],
            [Err("new registration failed".into())],
        );

        let error = persist_style_pack_hotkeys(&writer, vec![style_hotkey("builtin.raw", "1")])
            .unwrap_err();

        assert!(error.contains("new registration failed"));
        assert!(error.contains("preferences rollback failed"));
        assert_eq!(*writer.write_count.lock(), 2);
        assert_eq!(*writer.refresh_count.lock(), 1);
    }

    #[test]
    fn style_pack_hotkeys_reject_duplicates_and_overlaps() {
        let prefs = UserPreferences {
            dictation_hotkey: key("A"),
            ..Default::default()
        };
        // 基线：两条不同包、不同键 → 通过。
        assert!(reject_style_pack_hotkey_conflicts(
            &[style_hotkey("builtin.raw", "1"), style_hotkey("imported.x", "2")],
            &prefs,
        )
        .is_ok());
        // 同一个包绑两条 → 拒绝。
        assert!(reject_style_pack_hotkey_conflicts(
            &[style_hotkey("builtin.raw", "1"), style_hotkey("builtin.raw", "2")],
            &prefs,
        )
        .is_err());
        // 两条绑同一个键 → 拒绝。
        assert!(reject_style_pack_hotkey_conflicts(
            &[style_hotkey("builtin.raw", "1"), style_hotkey("imported.x", "1")],
            &prefs,
        )
        .is_err());
        // 空 pack_id → 拒绝。
        assert!(
            reject_style_pack_hotkey_conflicts(&[style_hotkey("", "1")], &prefs).is_err()
        );
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
