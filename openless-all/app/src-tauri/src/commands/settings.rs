use super::*;

#[tauri::command]
pub fn get_settings(coord: CoordinatorState<'_>) -> UserPreferences {
    coord.prefs().get()
}

#[tauri::command]
pub fn get_default_style_system_prompts() -> StyleSystemPrompts {
    StyleSystemPrompts::default()
}

pub(crate) trait SettingsWriter {
    fn read_settings(&self) -> UserPreferences;
    fn write_settings(&self, prefs: UserPreferences) -> Result<(), String>;
    fn write_settings_preserving_current_style_preferences(
        &self,
        mut prefs: UserPreferences,
    ) -> Result<(), String> {
        let current = self.read_settings();
        prefs.preserve_style_preferences_from(&current);
        self.write_settings(prefs)
    }
    fn sync_active_asr_provider(&self, provider: &str) -> Result<(), String>;
    fn refresh_dictation_hotkey(&self);
    fn refresh_qa_hotkey(&self);
    fn refresh_combo_hotkey(&self);
    fn refresh_translation_hotkey(&self);
    fn refresh_switch_style_hotkey(&self);
    fn refresh_open_app_hotkey(&self);
    fn refresh_selection_polish_hotkey(&self);
    fn refresh_coding_agent_hotkey(&self);
    // 默认 no-op：测试 mock 不关心风格快捷键；真实实现（Coordinator / Arc<T>）覆写。
    fn refresh_style_pack_hotkeys(&self) {}
}

impl SettingsWriter for Coordinator {
    fn read_settings(&self) -> UserPreferences {
        self.prefs().get()
    }

    fn write_settings(&self, prefs: UserPreferences) -> Result<(), String> {
        self.prefs().set(prefs).map_err(|e| e.to_string())
    }

    fn write_settings_preserving_current_style_preferences(
        &self,
        prefs: UserPreferences,
    ) -> Result<(), String> {
        self.prefs()
            .set_preserving_current_style_preferences(prefs)
            .map_err(|e| e.to_string())
    }

    fn sync_active_asr_provider(&self, provider: &str) -> Result<(), String> {
        self.sync_active_asr_provider_to_vault(provider)
    }

    fn refresh_dictation_hotkey(&self) {
        self.update_hotkey_binding();
    }

    fn refresh_qa_hotkey(&self) {
        self.update_qa_hotkey_binding();
    }

    fn refresh_combo_hotkey(&self) {
        self.update_combo_hotkey_binding();
    }

    fn refresh_translation_hotkey(&self) {
        self.update_translation_hotkey_binding();
    }

    fn refresh_switch_style_hotkey(&self) {
        self.update_switch_style_hotkey_binding();
    }

    fn refresh_open_app_hotkey(&self) {
        self.update_open_app_hotkey_binding();
    }

    #[cfg(not(mobile))]
    fn refresh_selection_polish_hotkey(&self) {
        self.update_selection_polish_hotkey_binding();
    }

    #[cfg(mobile)]
    fn refresh_selection_polish_hotkey(&self) {}

    fn refresh_coding_agent_hotkey(&self) {
        self.update_coding_agent_hotkey_binding();
    }

    fn refresh_style_pack_hotkeys(&self) {
        self.update_style_pack_hotkey_bindings();
    }
}

impl<T: SettingsWriter + ?Sized> SettingsWriter for Arc<T> {
    fn read_settings(&self) -> UserPreferences {
        (**self).read_settings()
    }

    fn write_settings(&self, prefs: UserPreferences) -> Result<(), String> {
        (**self).write_settings(prefs)
    }

    fn write_settings_preserving_current_style_preferences(
        &self,
        prefs: UserPreferences,
    ) -> Result<(), String> {
        (**self).write_settings_preserving_current_style_preferences(prefs)
    }

    fn sync_active_asr_provider(&self, provider: &str) -> Result<(), String> {
        (**self).sync_active_asr_provider(provider)
    }

    fn refresh_dictation_hotkey(&self) {
        (**self).refresh_dictation_hotkey();
    }

    fn refresh_qa_hotkey(&self) {
        (**self).refresh_qa_hotkey();
    }

    fn refresh_combo_hotkey(&self) {
        (**self).refresh_combo_hotkey();
    }

    fn refresh_translation_hotkey(&self) {
        (**self).refresh_translation_hotkey();
    }

    fn refresh_switch_style_hotkey(&self) {
        (**self).refresh_switch_style_hotkey();
    }

    fn refresh_open_app_hotkey(&self) {
        (**self).refresh_open_app_hotkey();
    }

    fn refresh_selection_polish_hotkey(&self) {
        (**self).refresh_selection_polish_hotkey();
    }

    fn refresh_coding_agent_hotkey(&self) {
        (**self).refresh_coding_agent_hotkey();
    }

    fn refresh_style_pack_hotkeys(&self) {
        (**self).refresh_style_pack_hotkeys();
    }
}

/// 非核心热键，用于保存兜底的冲突化解。dictation 是核心热键，永不参与调整。
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
    fn get(&self, prefs: &UserPreferences) -> Option<ShortcutBinding> {
        match self {
            Self::Translation => Some(prefs.translation_hotkey.clone()),
            Self::Qa => prefs.qa_hotkey.clone(),
            Self::SwitchStyle => prefs.switch_style_hotkey.clone(),
            Self::OpenApp => prefs.open_app_hotkey.clone(),
            Self::SelectionPolish => prefs.selection_polish_hotkey.clone(),
            Self::LessComputer => prefs.coding_agent_voice_hotkey.clone(),
        }
    }

    fn set(&self, prefs: &mut UserPreferences, value: Option<ShortcutBinding>) {
        match self {
            // translation 是必填键，None 表示恢复失败时保持旧值不动。
            Self::Translation => {
                if let Some(value) = value {
                    prefs.translation_hotkey = value;
                }
            }
            Self::Qa => prefs.qa_hotkey = value,
            Self::SwitchStyle => prefs.switch_style_hotkey = value,
            Self::OpenApp => prefs.open_app_hotkey = value,
            Self::SelectionPolish => prefs.selection_polish_hotkey = value,
            Self::LessComputer => prefs.coding_agent_voice_hotkey = value,
        }
    }
}

/// 单个非核心热键是否非法。与 `reject_non_dictation_side_specific_shortcuts`
/// 的逐键校验保持精确一致，避免把非冲突键一并停用。
fn non_core_hotkey_invalid(key: NonCoreHotkey, binding: &ShortcutBinding) -> bool {
    if crate::shortcut_binding::reject_side_specific_non_dictation(binding).is_err() {
        return true;
    }
    match key {
        NonCoreHotkey::SelectionPolish => {
            crate::shortcut_binding::validate_binding(binding).is_err()
                || reject_bare_shift_dictation_shortcut(binding).is_err()
        }
        _ => false,
    }
}

/// 保存兜底（#904）：热键冲突不能把整份设置挡在保存之外。
///
/// 按核心度从高到低处理每个非核心热键：凡与更高优先级键重叠、或本身非法
/// （侧特定修饰键等）的，恢复为旧值；旧值仍冲突/非法（历史遗留，例如 1.3.15
/// 升级注入的选区润色默认键与录音键重复）时停用（translation 回退默认 Shift）。
/// 返回被调整的键数量。dictation 永远保留，不参与调整。
pub(crate) fn reconcile_hotkey_collisions(
    prefs: &mut UserPreferences,
    previous: &UserPreferences,
) -> usize {
    // 处理顺序 = 核心度从高到低：处理某项时，更高优先级的键已定稿。
    const ORDER: [NonCoreHotkey; 6] = [
        NonCoreHotkey::Translation,
        NonCoreHotkey::Qa,
        NonCoreHotkey::SwitchStyle,
        NonCoreHotkey::OpenApp,
        NonCoreHotkey::SelectionPolish,
        NonCoreHotkey::LessComputer,
    ];
    let mut higher: Vec<ShortcutBinding> = vec![prefs.dictation_hotkey.clone()];
    let mut adjusted = 0;
    for key in ORDER {
        let Some(current) = key.get(prefs) else {
            continue;
        };
        let collides = higher
            .iter()
            .any(|held| crate::shortcut_binding::bindings_overlap(held, &current));
        if !collides && !non_core_hotkey_invalid(key, &current) {
            higher.push(current);
            continue;
        }
        let fallback = key.get(previous).filter(|candidate| {
            !higher
                .iter()
                .any(|held| crate::shortcut_binding::bindings_overlap(held, candidate))
                && !non_core_hotkey_invalid(key, candidate)
        });
        // translation 不能停用：旧值仍冲突/非法时回退到默认 Shift（不会与任何键重叠）。
        let resolved = if key == NonCoreHotkey::Translation && fallback.is_none() {
            Some(UserPreferences::default().translation_hotkey.clone())
        } else {
            fallback
        };
        key.set(prefs, resolved.clone());
        adjusted += 1;
        if let Some(value) = resolved {
            higher.push(value);
        }
    }
    // 风格包直达快捷键是最低优先级：与更高优先级键重叠、非法或集合内重复的条目，
    // 先尝试恢复该风格包的旧绑定，仍不行则整条移除（不影响其余设置落盘）。
    let mut kept: Vec<StylePackHotkey> = Vec::new();
    for entry in &prefs.style_pack_hotkeys {
        let candidate_ok = |candidate: &StylePackHotkey| {
            !candidate.pack_id.trim().is_empty()
                && crate::shortcut_binding::validate_binding(&candidate.binding).is_ok()
                && crate::shortcut_binding::reject_side_specific_non_dictation(&candidate.binding)
                    .is_ok()
                && reject_modifier_only_action_shortcut(&candidate.binding).is_ok()
                && !kept.iter().any(|held: &StylePackHotkey| {
                    held.pack_id == candidate.pack_id
                        || crate::shortcut_binding::bindings_overlap(
                            &held.binding,
                            &candidate.binding,
                        )
                })
                && !higher.iter().any(|held| {
                    crate::shortcut_binding::bindings_overlap(held, &candidate.binding)
                })
        };
        if candidate_ok(entry) {
            kept.push(entry.clone());
            continue;
        }
        adjusted += 1;
        if let Some(fallback) = previous
            .style_pack_hotkeys
            .iter()
            .find(|old| old.pack_id == entry.pack_id)
            .filter(|old| candidate_ok(old))
        {
            kept.push(fallback.clone());
        }
    }
    if kept != prefs.style_pack_hotkeys {
        prefs.style_pack_hotkeys = kept;
    }
    adjusted
}

pub(crate) fn persist_settings<T: SettingsWriter>(
    coord: &T,
    prefs: UserPreferences,
) -> Result<(), String> {
    persist_settings_with_keyboard_apply(
        coord,
        prefs,
        crate::windows_ime_profile::apply_windows_openless_keyboard_list_pref,
    )
}

pub(crate) fn persist_settings_with_keyboard_apply<T: SettingsWriter>(
    coord: &T,
    mut prefs: UserPreferences,
    apply_keyboard_list: impl Fn(&UserPreferences) -> Result<(), String>,
) -> Result<(), String> {
    let mut previous = coord.read_settings();
    sync_dictation_hotkey_legacy_fields(&mut previous);
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    if let Err(collision_error) = reject_hotkey_collisions(&prefs) {
        // 兜底（#904）：热键冲突（含历史遗留的重复键）不能拒绝整份设置保存。
        // 自动把冲突/非法的非核心热键恢复旧值或停用，其余设置照常落盘。
        let adjusted = reconcile_hotkey_collisions(&mut prefs, &previous);
        reject_hotkey_collisions(&prefs).map_err(|leftover| {
            format!("{collision_error}; 自动化解 {adjusted} 项后仍无法通过校验: {leftover}")
        })?;
        log::warn!(
            "[settings] 热键冲突已自动化解（调整 {adjusted} 项）后保存: {collision_error}"
        );
    }
    let dictation_shortcut_changed = previous.dictation_hotkey != prefs.dictation_hotkey;
    let dictation_mode_changed = previous.hotkey.mode != prefs.hotkey.mode;
    let qa_changed = previous.qa_hotkey != prefs.qa_hotkey;
    let translation_changed = previous.translation_hotkey != prefs.translation_hotkey;
    let switch_style_changed = previous.switch_style_hotkey != prefs.switch_style_hotkey;
    let open_app_changed = previous.open_app_hotkey != prefs.open_app_hotkey;
    let style_pack_hotkeys_changed = previous.style_pack_hotkeys != prefs.style_pack_hotkeys;
    let selection_polish_changed =
        previous.selection_polish_hotkey != prefs.selection_polish_hotkey;
    let coding_agent_changed = previous.coding_agent_enabled != prefs.coding_agent_enabled
        || previous.coding_agent_voice_hotkey != prefs.coding_agent_voice_hotkey;
    let windows_keyboard_list_changed = previous.windows_sendinput_insertion_only
        != prefs.windows_sendinput_insertion_only
        || previous.windows_show_openless_in_keyboard_list
            != prefs.windows_show_openless_in_keyboard_list;
    let active_asr_provider_changed = previous.active_asr_provider != prefs.active_asr_provider;
    let active_asr_provider = prefs.active_asr_provider.clone();

    if windows_keyboard_list_changed {
        apply_keyboard_list(&prefs)?;
    }

    if active_asr_provider_changed {
        if let Err(asr_err) = coord.sync_active_asr_provider(&active_asr_provider) {
            if windows_keyboard_list_changed {
                if let Err(kb_rollback_err) = apply_keyboard_list(&previous) {
                    return Err(format!(
                        "{asr_err}; additionally failed to rollback keyboard list visibility: {kb_rollback_err}"
                    ));
                }
                log::warn!(
                    "[windows-ime] rolled back keyboard list visibility after ASR provider sync failure"
                );
            }
            return Err(asr_err);
        }
    }

    if let Err(error) = coord.write_settings_preserving_current_style_preferences(prefs.clone()) {
        if active_asr_provider_changed {
            match coord.sync_active_asr_provider(&previous.active_asr_provider) {
                Ok(()) => {
                    if windows_keyboard_list_changed {
                        if let Err(rollback_err) = apply_keyboard_list(&previous) {
                            return Err(format!(
                                "{error}; additionally failed to rollback keyboard list visibility: {rollback_err}"
                            ));
                        }
                        log::warn!(
                            "[windows-ime] rolled back keyboard list visibility after settings write failure"
                        );
                    }
                    return Err(error);
                }
                Err(rollback_error) => {
                    // ASR vault 无法回滚时 roll-forward prefs；键盘列表保持新状态，避免三者分叉。
                    coord
                        .write_settings_preserving_current_style_preferences(prefs)
                        .map_err(|roll_forward_error| {
                            format!(
                                "{error}; additionally failed to restore active ASR provider: {rollback_error}; additionally failed to preserve active ASR provider consistency: {roll_forward_error}"
                            )
                        })?;
                }
            }
        } else if windows_keyboard_list_changed {
            if let Err(rollback_err) = apply_keyboard_list(&previous) {
                return Err(format!(
                    "{error}; additionally failed to rollback keyboard list visibility: {rollback_err}"
                ));
            }
            log::warn!(
                "[windows-ime] rolled back keyboard list visibility after settings write failure"
            );
            return Err(error);
        } else {
            return Err(error);
        }
    }
    if dictation_shortcut_changed || dictation_mode_changed {
        coord.refresh_dictation_hotkey();
    }
    if dictation_shortcut_changed {
        coord.refresh_combo_hotkey();
    }
    if qa_changed {
        coord.refresh_qa_hotkey();
    }
    if translation_changed {
        coord.refresh_translation_hotkey();
    }
    if switch_style_changed {
        coord.refresh_switch_style_hotkey();
    }
    if open_app_changed {
        coord.refresh_open_app_hotkey();
    }
    if style_pack_hotkeys_changed {
        coord.refresh_style_pack_hotkeys();
    }
    if selection_polish_changed {
        coord.refresh_selection_polish_hotkey();
    }
    if coding_agent_changed {
        coord.refresh_coding_agent_hotkey();
    }
    Ok(())
}

#[cfg(not(mobile))]
#[tauri::command]
pub fn set_settings(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    tray_microphones: State<'_, TrayMicrophoneMenuState>,
    mut prefs: UserPreferences,
) -> Result<(), String> {
    // 捕获旧值用于远程输入服务的 diff（persist 后端口/开关变化时启停/重启）。
    let remote_prev = coord.prefs().get();
    let packs = coord.style_packs().list().map_err(|e| e.to_string())?;
    sync_style_pack_preferences(&mut prefs, &packs);
    prefs.android_overlay_trigger = prefs.android_overlay_trigger.normalized();
    // 广播给所有 webview。issue #205：QaPanel 跑在独立 webview，
    // 没有 HotkeySettingsContext，必须靠事件感知录音键变化，否则面板可见时
    // 用户改键会让浮窗里的 "{recordHotkey}" 文案一直停留在旧值。
    persist_settings(&*coord, prefs)?;
    let prefs = coord.prefs().get();
    // 保存即同步胶囊样式原子：下一次录音的入场帧就携带新样式，不依赖 emit_capsule
    // 主线程闭包的 ~30Hz 同步（Windows 主线程拥塞时闭包延迟 → 整场显示旧样式）。
    // 前端也会通过 prefs:changed 广播收到新样式，录音中切换即时换肤。
    coord.sync_capsule_style_from_preferences();
    // 用户关掉 Esc 录音恢复时，已经显示或正等待显示的恢复入口也必须立即收起。
    // 历史/WAV 已经落盘，不在设置切换时删除；开关只控制后续 Esc 是否创建恢复记录。
    if remote_prev.esc_recording_recovery_enabled && !prefs.esc_recording_recovery_enabled {
        coord.dismiss_cancelled_recording_recovery(None);
    }
    // 系统代理开关变化时立即重建客户端连接池（issue #869）。
    if remote_prev.use_system_proxy != prefs.use_system_proxy {
        crate::net::set_use_system_proxy(prefs.use_system_proxy);
    }
    // 关掉「光标上下文」时立刻解除已经武装的手改观察器。
    //
    // 不这么做的话，上一次听写留下的观察器会一直活到它自己的 60 秒硬超时（或前台 app
    // 切换）为止 —— 也就是用户明确关掉开关之后，我们还在读他正在写的那个文档，最长
    // 一分钟。功能本身是否还有用不重要：**开关关掉的那一刻就该停**，这是这个功能敢
    // 默认存在的全部前提。
    if remote_prev.cursor_context_enabled && !prefs.cursor_context_enabled {
        coord.disarm_edit_watch();
    }
    #[cfg(target_os = "android")]
    coord.apply_android_overlay_settings_change(&remote_prev, &prefs);
    // refresh_tray_microphone_menu 内部会调用 NSStatusItem.set_menu，必须在主线程上跑。
    // set_settings 本身是同步 Tauri command，在 IPC handler 线程上执行；从这里直接调
    // 会触发 macOS 主线程断言或在 dispatch 队列上死锁，导致整个 UI 无响应（用户改
    // 偏好后所有按键都没反应即此根因）。dispatch 到主线程后立即返回，IPC 线程不阻塞。
    let app_for_main = app.clone();
    let prefs_for_main = prefs.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(err) = crate::refresh_tray_microphone_menu(&app_for_main) {
            log::warn!("[tray] refresh microphone menu after settings save failed: {err}");
            let tray_state = app_for_main.state::<TrayMicrophoneMenuState>();
            sync_tray_microphone_selection(
                &tray_state.lock(),
                &prefs_for_main.microphone_device_name,
            );
        }
    });
    // 抑制 unused 警告：tray_microphones 现在改在闭包里通过 app.state 取，
    // 但函数签名保留 State 入参，以便 Tauri 在调用前注入。
    let _ = tray_microphones;
    let _ = app.emit("prefs:changed", &prefs);
    // 远程输入：开关 / 端口变化时启停或重启服务（PIN 变化走 regenerate_remote_pin 命令）。
    if remote_prev.remote_input_enabled != prefs.remote_input_enabled
        || remote_prev.remote_input_port != prefs.remote_input_port
    {
        coord.refresh_remote_server();
    }
    Ok(())
}

#[cfg(mobile)]
#[tauri::command]
pub fn set_settings(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    mut prefs: UserPreferences,
) -> Result<(), String> {
    let previous = coord.prefs().get();
    let packs = coord.style_packs().list().map_err(|e| e.to_string())?;
    sync_style_pack_preferences(&mut prefs, &packs);
    prefs.android_overlay_trigger = prefs.android_overlay_trigger.normalized();
    persist_settings(&*coord, prefs)?;
    let prefs = coord.prefs().get();
    // 保存即同步胶囊样式原子（Android 通知胶囊 payload 同源，见 emit_capsule）。
    coord.sync_capsule_style_from_preferences();
    // 系统代理开关变化时立即重建客户端连接池（issue #869）。
    if previous.use_system_proxy != prefs.use_system_proxy {
        crate::net::set_use_system_proxy(prefs.use_system_proxy);
    }
    #[cfg(target_os = "android")]
    coord.apply_android_overlay_settings_change(&previous, &prefs);
    let _ = app.emit("prefs:changed", &prefs);
    let _ = app.emit_to("main", "prefs:changed", &prefs);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RaceSettingsWriter {
        reads: Mutex<Vec<UserPreferences>>,
        saved: Mutex<Option<UserPreferences>>,
    }

    impl SettingsWriter for RaceSettingsWriter {
        fn read_settings(&self) -> UserPreferences {
            let mut reads = self.reads.lock().unwrap();
            if reads.is_empty() {
                return self.saved.lock().unwrap().clone().unwrap_or_default();
            }
            reads.remove(0)
        }

        fn write_settings(&self, prefs: UserPreferences) -> Result<(), String> {
            *self.saved.lock().unwrap() = Some(prefs);
            Ok(())
        }

        fn sync_active_asr_provider(&self, _provider: &str) -> Result<(), String> {
            Ok(())
        }

        fn refresh_dictation_hotkey(&self) {}

        fn refresh_qa_hotkey(&self) {}

        fn refresh_combo_hotkey(&self) {}

        fn refresh_translation_hotkey(&self) {}

        fn refresh_switch_style_hotkey(&self) {}

        fn refresh_open_app_hotkey(&self) {}
        fn refresh_selection_polish_hotkey(&self) {}

        fn refresh_coding_agent_hotkey(&self) {}
    }

    #[test]
    fn settings_save_preserves_current_style_preferences_before_write() {
        let packs = crate::types::builtin_style_packs();
        let current = UserPreferences {
            default_mode: PolishMode::Light,
            active_style_pack_id: builtin_style_pack_id(PolishMode::Light).to_string(),
            ..UserPreferences::default()
        };
        let mut stale_settings_payload = UserPreferences {
            default_mode: PolishMode::Formal,
            active_style_pack_id: builtin_style_pack_id(PolishMode::Formal).to_string(),
            ..UserPreferences::default()
        };

        stale_settings_payload.preserve_style_preferences_from(&current);
        sync_style_pack_preferences(&mut stale_settings_payload, &packs);

        assert_eq!(
            stale_settings_payload.active_style_pack_id,
            builtin_style_pack_id(PolishMode::Light)
        );
        assert_eq!(stale_settings_payload.default_mode, PolishMode::Light);
    }

    #[test]
    fn persist_settings_keeps_style_change_that_lands_before_write() {
        let active_before_request = UserPreferences {
            default_mode: PolishMode::Formal,
            active_style_pack_id: builtin_style_pack_id(PolishMode::Formal).to_string(),
            ..UserPreferences::default()
        };
        let active_before_write = UserPreferences {
            default_mode: PolishMode::Light,
            active_style_pack_id: builtin_style_pack_id(PolishMode::Light).to_string(),
            ..UserPreferences::default()
        };
        let stale_payload = UserPreferences {
            default_mode: PolishMode::Formal,
            active_style_pack_id: builtin_style_pack_id(PolishMode::Formal).to_string(),
            microphone_device_name: "External Mic".to_string(),
            ..UserPreferences::default()
        };
        let writer = RaceSettingsWriter {
            reads: Mutex::new(vec![active_before_request, active_before_write]),
            saved: Mutex::new(None),
        };

        persist_settings(&writer, stale_payload).unwrap();

        let saved = writer.saved.lock().unwrap().clone().expect("prefs saved");
        assert_eq!(
            saved.active_style_pack_id,
            builtin_style_pack_id(PolishMode::Light)
        );
        assert_eq!(saved.default_mode, PolishMode::Light);
        assert_eq!(saved.microphone_device_name, "External Mic");
    }

    #[test]
    fn reconcile_clears_legacy_dictation_selection_polish_duplication() {
        // #904 历史遗留：1.3.15 升级注入的选区润色默认键（右 Alt）与录音键相同。
        let prefs = UserPreferences {
            hotkey: crate::types::HotkeyBinding {
                trigger: crate::types::HotkeyTrigger::RightAlt,
                mode: crate::types::HotkeyMode::Hold,
                keys: None,
            },
            dictation_hotkey: ShortcutBinding {
                primary: "RightAlt".into(),
                modifiers: vec![],
            },
            selection_polish_hotkey: Some(ShortcutBinding {
                primary: "RightAlt".into(),
                modifiers: vec![],
            }),
            ..Default::default()
        };
        let mut next = prefs.clone();

        let adjusted = reconcile_hotkey_collisions(&mut next, &prefs);

        assert!(adjusted >= 1);
        assert!(next.selection_polish_hotkey.is_none());
        assert!(reject_hotkey_collisions(&next).is_ok());
    }

    #[test]
    fn persist_settings_reconciles_legacy_collision_and_still_saves_mode() {
        // #904 复现：历史冲突存在时，用户切「自动」必须能保存成功，
        // 冲突的选区润色键被停用，而不是整份设置被拒。
        let collision = UserPreferences {
            hotkey: crate::types::HotkeyBinding {
                trigger: crate::types::HotkeyTrigger::RightAlt,
                mode: crate::types::HotkeyMode::Hold,
                keys: None,
            },
            dictation_hotkey: ShortcutBinding {
                primary: "RightAlt".into(),
                modifiers: vec![],
            },
            selection_polish_hotkey: Some(ShortcutBinding {
                primary: "RightAlt".into(),
                modifiers: vec![],
            }),
            ..Default::default()
        };
        let mut next = collision.clone();
        next.hotkey.mode = crate::types::HotkeyMode::Auto;
        let writer = RaceSettingsWriter {
            reads: Mutex::new(vec![collision]),
            saved: Mutex::new(None),
        };

        persist_settings_with_keyboard_apply(&writer, next, |_| Ok(())).unwrap();

        let saved = writer.saved.lock().unwrap().clone().expect("prefs saved");
        assert_eq!(saved.hotkey.mode, crate::types::HotkeyMode::Auto);
        assert!(saved.selection_polish_hotkey.is_none());
    }

    #[test]
    fn reconcile_resolves_non_core_overlap_and_invalid_side_specific_hotkey() {
        // QA 与翻译键相同：较低优先级的 QA 恢复旧值，旧值仍冲突则停用。
        let previous = UserPreferences {
            qa_hotkey: Some(ShortcutBinding {
                primary: "E".into(),
                modifiers: vec!["ctrl".into(), "shift".into()],
            }),
            ..Default::default()
        };
        let mut next = previous.clone();
        next.qa_hotkey = Some(ShortcutBinding {
            primary: "Shift".into(),
            modifiers: vec![],
        });
        // 侧特定修饰键对非 dictation 非法（SIDE_SPECIFIC_NON_DICTATION_MSG）。
        next.translation_hotkey = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd-left".into()],
        };

        let adjusted = reconcile_hotkey_collisions(&mut next, &previous);

        assert!(adjusted >= 2);
        assert_eq!(next.qa_hotkey, previous.qa_hotkey);
        assert_eq!(next.translation_hotkey, previous.translation_hotkey);
        assert!(reject_hotkey_collisions(&next).is_ok());
    }
}

// ─────────────────────────── release channel (Beta opt-in) ───────────────────────────
//
// 渠道偏好的写入路径跟 set_settings 复用 persist_settings：保持热键兜底归一化
// 跟其他 prefs 写入一致，且写完后 emit "prefs:changed"，让前端跨 webview 同步。
//
// 更新：plugin-updater 2.10.1 的 Builder 现在暴露 .endpoints() runtime API（CLAUDE.md
// 当年记的"不支持"已不成立）。本节配合 `app_check_update_with_channel` 命令实现
// Beta auto-update：Stable 渠道 → 走 tauri.conf 的默认 endpoints；Beta 渠道 →
// fetch_latest_beta_release 拿最新 prerelease tag → 拼成 -beta manifest URL →
// builder.endpoints(vec![url]).build().check()。Stable 用户绝对不会撞到 Beta 包
// （Beta tag 的 manifest 文件名带 `-beta` 后缀，跟 Stable manifest 在 GitHub
// Release assets 里物理分离）。

#[tauri::command]
pub fn get_update_channel(coord: CoordinatorState<'_>) -> UpdateChannel {
    coord.prefs().get().update_channel
}

#[tauri::command]
pub fn set_update_channel(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    channel: UpdateChannel,
) -> Result<(), String> {
    let mut prefs = coord.prefs().get();
    if prefs.update_channel == channel {
        return Ok(());
    }
    prefs.update_channel = channel;
    persist_settings(&*coord, prefs)?;
    let _ = app.emit("prefs:changed", &coord.prefs().get());
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestBetaRelease {
    pub tag_name: String,
    pub html_url: String,
    pub published_at: String,
}

/// 拉 GitHub Releases atom feed 找最新 Beta release。
///
/// 历史：之前用 `api.github.com/repos/.../releases` REST 端点，**未认证 60 req/h/IP**，
/// 多人多次切 Beta toggle 很容易撞 403 rate limit（用户报"获取 Beta 版本信息失败"
/// 即是这个）。换成 `releases.atom` 后是公开页面 + CDN cache，没有同等 rate 限制。
/// Atom feed 不显式标 prerelease，所以按当前 `-Beta.N-tauri` 约定过滤，同时兼容
/// 历史 `-beta-tauri` 后缀。
///
/// 返回 `Ok(None)` = 当前没发过 Beta 版；`Err(String)` = 网络/解析故障。
#[tauri::command]
pub async fn fetch_latest_beta_release() -> Result<Option<LatestBetaRelease>, String> {
    let resp = net::send_with_retry(|| {
        net::http()
            .get("https://github.com/Open-Less/openless/releases.atom")
            .timeout(std::time::Duration::from_secs(15))
    })
    .await
    .map_err(|e| format!("fetch releases.atom: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("releases.atom status {}", resp.status()));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("read atom body: {e}"))?;
    Ok(parse_latest_beta_from_atom(&body))
}

/// 简单字符串解析 atom feed，避免引 XML 库。每个 `<entry>...</entry>` 内含一行
/// `<link rel="alternate" type="text/html" href=".../releases/tag/<tag>"/>`，
/// 用 `/releases/tag/` 这个唯一锚点抓 tag。
pub(crate) fn parse_latest_beta_from_atom(body: &str) -> Option<LatestBetaRelease> {
    for entry in body.split("<entry>").skip(1) {
        let entry_body = entry
            .split_once("</entry>")
            .map(|(b, _)| b)
            .unwrap_or(entry);
        let needle = "/releases/tag/";
        let tag_start = match entry_body.find(needle) {
            Some(i) => i + needle.len(),
            None => continue,
        };
        let tag_after = &entry_body[tag_start..];
        let tag_end = tag_after
            .find(|c: char| c == '"' || c == '<' || c == ' ' || c == '/')
            .unwrap_or(tag_after.len());
        let tag_name = tag_after[..tag_end].to_string();
        if !is_beta_release_tag(&tag_name) {
            continue;
        }
        let html_url = format!("https://github.com/Open-Less/openless/releases/tag/{tag_name}");
        let published_at =
            extract_between(entry_body, "<updated>", "</updated>").unwrap_or_default();
        return Some(LatestBetaRelease {
            tag_name,
            html_url,
            published_at,
        });
    }
    None
}

fn is_beta_release_tag(tag_name: &str) -> bool {
    if tag_name.ends_with("-beta-tauri") {
        return true;
    }

    let Some((version, beta_number)) = tag_name
        .strip_prefix('v')
        .and_then(|tag| tag.strip_suffix("-tauri"))
        .and_then(|tag| tag.split_once("-Beta."))
    else {
        return false;
    };

    if beta_number.is_empty() || !beta_number.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let mut version_parts = version.split('.');
    (0..3).all(|_| {
        version_parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    }) && version_parts.next().is_none()
}

fn extract_between(haystack: &str, open: &str, close: &str) -> Option<String> {
    let start = haystack.find(open)? + open.len();
    let end = haystack[start..].find(close)?;
    Some(haystack[start..start + end].to_string())
}

// ─────────────────────── Channel-aware updater check ────────────────────────
//
// 替换前端原来直接 import('@tauri-apps/plugin-updater').check() 的路径：
// - Stable 渠道：builder 不动 endpoints，沿用 tauri.conf 配的 stable manifest URL。
// - Beta 渠道：先 fetch_latest_beta_release 拿最新 prerelease tag，拼成 -beta manifest
//   URL（同时给一对 mirror + direct），再 builder.endpoints(vec![url])?.build()?.check()。
//
// 返回的 Metadata 形状与 plugin-updater 的 JS UpdateMetadata 完全一致（rid +
// currentVersion 等驼峰字段），前端可以直接 `new Update(metadata)` 复用 plugin
// 的 download / install / close 实现，无需我们自己写下载和签名校验。
//
// 物理隔离：Beta tag 推出来的 manifest 文件名带 `-beta` 后缀（参见 release-tauri.yml
// 第 382 行注释），跟 Stable 的 `latest-{tgt}-{arch}.json` 在 GitHub Release assets
// 里是分开的两份文件 —— 即使代码逻辑写错把 Beta URL 传给 Stable 用户，HTTP 也是
// 直接 404，绝不会拿到错档。

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateMetadata {
    #[cfg(not(mobile))]
    pub rid: tauri::ResourceId,
    #[cfg(mobile)]
    pub rid: u32,
    pub current_version: String,
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
    /// 原始 manifest JSON——桌面 `new Update(metadata)` / Android 自定义安装路径共用。
    pub raw_json: serde_json::Value,
}

/// 决定 manifest 来源后走 plugin-updater 的标准 check 流程。
/// 渠道：显式传入 `channel` 时用它（关于页固定查 Stable、高级页 Beta 区查 Beta）；
/// 不传则回落到 `prefs.update_channel`（后台 AutoUpdateGate 自动检查走这条）。
/// 返回 None = 当前是最新；Some(metadata) = 有新版可装。
#[tauri::command]
#[cfg(not(mobile))]
pub async fn app_check_update_with_channel<R: tauri::Runtime>(
    coord: CoordinatorState<'_>,
    webview: tauri::Webview<R>,
    timeout_ms: Option<u64>,
    channel: Option<UpdateChannel>,
) -> Result<Option<AppUpdateMetadata>, String> {
    use tauri_plugin_updater::UpdaterExt;

    let channel = channel.unwrap_or_else(|| coord.prefs().get().update_channel);
    let mut builder = webview.updater_builder();
    if let Some(ms) = timeout_ms {
        builder = builder.timeout(std::time::Duration::from_millis(ms));
    }
    if matches!(channel, UpdateChannel::Beta) {
        let urls = resolve_beta_manifest_endpoints().await?;
        builder = builder
            .endpoints(urls)
            .map_err(|e| format!("set beta endpoints: {e}"))?;
    }
    let updater = builder.build().map_err(|e| format!("build updater: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("check update failed: {e}"))?;

    let Some(update) = update else {
        return Ok(None);
    };
    // date 字段透传需要引 time crate；前端 AutoUpdate.tsx 实际并不用 date，所以这里
    // 直接置 None，避免拉一个新 dep 进 src-tauri/Cargo.toml。
    let metadata = AppUpdateMetadata {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        date: None,
        body: update.body.clone(),
        raw_json: update.raw_json.clone(),
        rid: webview.resources_table().add(update),
    };
    Ok(Some(metadata))
}

/// 把 fetch_latest_beta_release 找到的最新 prerelease tag 拼成 -beta manifest URL 对。
/// 顺序：先镜像（fastgit.cc 代理 GitHub），后直连 —— 跟 tauri.conf 现有 Stable
/// endpoints 一致，让国内访问优先打到 CDN。
#[cfg(not(mobile))]
async fn resolve_beta_manifest_endpoints() -> Result<Vec<url::Url>, String> {
    let Some(latest) = fetch_latest_beta_release().await? else {
        return Err("尚未发布过 Beta 版本".to_string());
    };
    let tag = latest.tag_name;
    // {{target}} / {{arch}} 占位符由 plugin 在 check 时替换。Rust raw string 用 r#""#
    // 不需要转义双花括号，比 format! 干净。
    let mirror = format!(
        "https://fastgit.cc/https://github.com/Open-Less/openless/releases/download/{tag}/latest-{{{{target}}}}-{{{{arch}}}}-beta-mirror.json"
    );
    let direct = format!(
        "https://github.com/Open-Less/openless/releases/download/{tag}/latest-{{{{target}}}}-{{{{arch}}}}-beta.json"
    );
    let mirror_url = url::Url::parse(&mirror).map_err(|e| format!("parse beta mirror url: {e}"))?;
    let direct_url = url::Url::parse(&direct).map_err(|e| format!("parse beta direct url: {e}"))?;
    Ok(vec![mirror_url, direct_url])
}

#[cfg(mobile)]
#[tauri::command]
pub async fn app_check_update_with_channel(
    coord: CoordinatorState<'_>,
    _timeout_ms: Option<u64>,
    channel: Option<UpdateChannel>,
) -> Result<Option<AppUpdateMetadata>, String> {
    #[cfg(target_os = "android")]
    {
        let channel = channel.unwrap_or_else(|| coord.prefs().get().update_channel);
        return crate::android::updater::check_update(channel).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (coord, channel);
        Err("应用内更新仅支持 Android".to_string())
    }
}

#[cfg(test)]
mod persist_settings_tests {
    use super::*;
    use std::cell::RefCell;

    struct MockWriter {
        prefs: RefCell<UserPreferences>,
        write_calls: RefCell<u32>,
        asr_sync_calls: RefCell<Vec<String>>,
        /// 前 N 次 write_settings 调用返回失败；0 = 从不失败。
        write_fail_count: u32,
        fail_forward_asr_sync: bool,
        fail_rollback_asr_sync: bool,
    }

    impl MockWriter {
        fn new(prefs: UserPreferences) -> Self {
            Self {
                prefs: RefCell::new(prefs),
                write_calls: RefCell::new(0),
                asr_sync_calls: RefCell::new(Vec::new()),
                write_fail_count: 0,
                fail_forward_asr_sync: false,
                fail_rollback_asr_sync: false,
            }
        }
    }

    impl SettingsWriter for MockWriter {
        fn read_settings(&self) -> UserPreferences {
            self.prefs.borrow().clone()
        }

        fn write_settings(&self, prefs: UserPreferences) -> Result<(), String> {
            let mut calls = self.write_calls.borrow_mut();
            *calls += 1;
            if *calls <= self.write_fail_count {
                return Err("write failed".into());
            }
            *self.prefs.borrow_mut() = prefs;
            Ok(())
        }

        fn sync_active_asr_provider(&self, provider: &str) -> Result<(), String> {
            self.asr_sync_calls.borrow_mut().push(provider.to_string());
            let stored = self.prefs.borrow().active_asr_provider.clone();
            if self.fail_forward_asr_sync && provider != stored {
                return Err("asr forward sync failed".into());
            }
            if self.fail_rollback_asr_sync && provider == stored {
                return Err("asr rollback sync failed".into());
            }
            Ok(())
        }

        fn refresh_dictation_hotkey(&self) {}
        fn refresh_qa_hotkey(&self) {}
        fn refresh_combo_hotkey(&self) {}
        fn refresh_translation_hotkey(&self) {}
        fn refresh_switch_style_hotkey(&self) {}
        fn refresh_open_app_hotkey(&self) {}
        fn refresh_selection_polish_hotkey(&self) {}
        fn refresh_coding_agent_hotkey(&self) {}
    }

    #[test]
    fn keyboard_apply_failure_does_not_sync_asr_or_write_prefs() {
        let writer = MockWriter::new(UserPreferences::default());
        let mut next = writer.read_settings();
        next.windows_sendinput_insertion_only = true;
        next.windows_show_openless_in_keyboard_list = false;
        next.active_asr_provider = "other-asr".into();

        let result = persist_settings_with_keyboard_apply(&writer, next, |_| {
            Err("apply failed".into())
        });

        assert!(result.is_err());
        assert_eq!(*writer.write_calls.borrow(), 0);
        assert!(writer.asr_sync_calls.borrow().is_empty());
        assert!(writer.read_settings().windows_show_openless_in_keyboard_list);
    }

    #[test]
    fn keyboard_apply_success_writes_prefs() {
        let writer = MockWriter::new(UserPreferences::default());
        let mut next = writer.read_settings();
        next.windows_sendinput_insertion_only = true;
        next.windows_show_openless_in_keyboard_list = false;

        let result = persist_settings_with_keyboard_apply(&writer, next.clone(), |_| Ok(()));

        assert!(result.is_ok());
        assert_eq!(*writer.write_calls.borrow(), 1);
        assert!(!writer.read_settings().windows_show_openless_in_keyboard_list);
    }

    #[test]
    fn asr_sync_failure_rolls_back_keyboard_list() {
        let writer = MockWriter {
            prefs: RefCell::new(UserPreferences::default()),
            write_calls: RefCell::new(0),
            asr_sync_calls: RefCell::new(Vec::new()),
            write_fail_count: 0,
            fail_forward_asr_sync: true,
            fail_rollback_asr_sync: false,
        };
        let mut next = writer.read_settings();
        next.windows_sendinput_insertion_only = true;
        next.windows_show_openless_in_keyboard_list = false;
        next.active_asr_provider = "other-asr".into();

        let apply_calls = RefCell::new(0);
        let result = persist_settings_with_keyboard_apply(&writer, next, |_| {
            *apply_calls.borrow_mut() += 1;
            Ok(())
        });

        assert!(result.is_err());
        assert_eq!(*writer.write_calls.borrow(), 0);
        assert_eq!(*apply_calls.borrow(), 2);
        assert!(writer.read_settings().windows_show_openless_in_keyboard_list);
    }

    #[test]
    fn keyboard_write_failure_rolls_back_profile_without_asr_change() {
        let writer = MockWriter {
            prefs: RefCell::new(UserPreferences::default()),
            write_calls: RefCell::new(0),
            asr_sync_calls: RefCell::new(Vec::new()),
            write_fail_count: 1,
            fail_forward_asr_sync: false,
            fail_rollback_asr_sync: false,
        };
        let mut next = writer.read_settings();
        next.windows_sendinput_insertion_only = true;
        next.windows_show_openless_in_keyboard_list = false;

        let rollback_calls = RefCell::new(0);
        let result = persist_settings_with_keyboard_apply(&writer, next, |prefs| {
            if prefs.windows_show_openless_in_keyboard_list {
                *rollback_calls.borrow_mut() += 1;
            }
            Ok(())
        });

        assert!(result.is_err());
        assert_eq!(*writer.write_calls.borrow(), 1);
        assert_eq!(*rollback_calls.borrow(), 1);
        assert!(writer.asr_sync_calls.borrow().is_empty());
    }

    #[test]
    fn keyboard_write_failure_rolls_back_profile_when_asr_rollback_succeeds() {
        let writer = MockWriter {
            prefs: RefCell::new(UserPreferences::default()),
            write_calls: RefCell::new(0),
            asr_sync_calls: RefCell::new(Vec::new()),
            write_fail_count: 1,
            fail_forward_asr_sync: false,
            fail_rollback_asr_sync: false,
        };
        let mut next = writer.read_settings();
        next.windows_sendinput_insertion_only = true;
        next.windows_show_openless_in_keyboard_list = false;
        next.active_asr_provider = "other-asr".into();

        let rollback_calls = RefCell::new(0);
        let result = persist_settings_with_keyboard_apply(&writer, next, |prefs| {
            if prefs.windows_show_openless_in_keyboard_list {
                *rollback_calls.borrow_mut() += 1;
            }
            Ok(())
        });

        assert!(result.is_err());
        assert_eq!(*writer.write_calls.borrow(), 1);
        assert_eq!(*rollback_calls.borrow(), 1);
    }

    #[test]
    fn keyboard_write_failure_keeps_new_keyboard_when_asr_roll_forward_succeeds() {
        let writer = MockWriter {
            prefs: RefCell::new(UserPreferences::default()),
            write_calls: RefCell::new(0),
            asr_sync_calls: RefCell::new(Vec::new()),
            write_fail_count: 1,
            fail_forward_asr_sync: false,
            fail_rollback_asr_sync: true,
        };
        let mut next = writer.read_settings();
        next.windows_sendinput_insertion_only = true;
        next.windows_show_openless_in_keyboard_list = false;
        next.active_asr_provider = "other-asr".into();

        let apply_calls = RefCell::new(0);
        let result = persist_settings_with_keyboard_apply(&writer, next.clone(), |_| {
            *apply_calls.borrow_mut() += 1;
            Ok(())
        });

        assert!(result.is_ok());
        assert_eq!(*writer.write_calls.borrow(), 2);
        assert_eq!(*apply_calls.borrow(), 1);
        assert!(!writer.read_settings().windows_show_openless_in_keyboard_list);
    }
}

#[cfg(mobile)]
#[tauri::command]
pub async fn app_download_and_install_android_update(
    app: AppHandle,
    url: String,
    signature: String,
    version: String,
) -> Result<(), String> {
    // 安全：下载前校验 URL，防止 SSRF（如内网元数据接口、localhost 服务）。
    // 只允许已知的 GitHub 直链和 fastgit 镜像前缀。
    const DIRECT_BASE: &str = "https://github.com/Open-Less/openless";
    const MIRROR_BASE: &str = "https://fastgit.cc/https://github.com/Open-Less/openless";
    if !url.starts_with(DIRECT_BASE) && !url.starts_with(MIRROR_BASE) {
        return Err(format!("不信任的更新 URL，拒绝下载: {url}"));
    }
    #[cfg(target_os = "android")]
    {
        return crate::android::updater::download_and_install(app, url, signature, version).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, url, signature, version);
        Err("应用内更新仅支持 Android".to_string())
    }
}
