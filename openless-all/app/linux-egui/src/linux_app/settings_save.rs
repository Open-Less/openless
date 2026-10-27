use super::*;
use serde_json::Value;
use std::collections::BTreeMap;

/// 设置页里的外链目标，与 Tauri 的 `AboutSection` / `SettingsModal` 一字不差
/// （`HELP_URL` / `RELEASE_NOTES_URL` 及仓库、issue 页）。
pub(crate) const REPOSITORY_URL: &str = "https://github.com/Open-Less/openless";
pub(crate) const HELP_URL: &str = "https://github.com/Open-Less/openless#readme";
pub(crate) const RELEASE_NOTES_URL: &str = "https://github.com/Open-Less/openless/releases";
pub(crate) const ISSUES_URL: &str = "https://github.com/Open-Less/openless/issues";

type Patch = BTreeMap<String, Value>;

#[derive(Default)]
pub(super) struct SettingsSave {
    observed: Option<Value>,
    pending: Patch,
    pub(super) in_flight: Option<(u64, Patch)>,
    next_request: u64,
    pub(super) error: Option<String>,
}

impl SettingsSave {
    pub(super) fn new(preferences: &UserPreferences) -> Self {
        Self {
            observed: Some(serde_json::to_value(preferences).expect("serializable preferences")),
            ..Self::default()
        }
    }

    fn capture(&mut self, draft: &UserPreferences) {
        let next = serde_json::to_value(draft).expect("serializable preferences");
        if let Some(previous) = &self.observed {
            for (key, value) in next.as_object().expect("preference document") {
                if previous[key] == *value {
                    continue;
                }
                // Mode and trigger are separate edits; shortcut bindings and
                // style-hotkey collections are atomic preference fields.
                if key == "hotkey" {
                    for (field, value) in value.as_object().expect("hotkey document") {
                        if previous[key][field] != *value {
                            self.pending
                                .insert(format!("/hotkey/{field}"), value.clone());
                        }
                    }
                } else {
                    self.pending.insert(format!("/{key}"), value.clone());
                }
            }
        }
        self.observed = Some(next);
    }

    pub(super) fn rebase(&mut self, latest: &UserPreferences) -> UserPreferences {
        let mut edits = self
            .in_flight
            .as_ref()
            .map(|(_, edits)| edits.clone())
            .unwrap_or_default();
        edits.extend(self.pending.clone());
        let draft = openless_linux_egui::patch_preferences(latest, &edits)
            .expect("patches generated from typed preferences");
        self.observed = Some(serde_json::to_value(&draft).expect("serializable preferences"));
        draft
    }

    fn begin(&mut self) -> Option<(u64, Patch)> {
        if self.in_flight.is_some() || self.error.is_some() || self.pending.is_empty() {
            return None;
        }
        self.next_request += 1;
        let request = (self.next_request, std::mem::take(&mut self.pending));
        self.in_flight = Some(request.clone());
        Some(request)
    }

    fn finish(&mut self, request: u64, result: &Result<(), String>) -> bool {
        if self.in_flight.as_ref().map(|(id, _)| *id) != Some(request) {
            return false;
        }
        let (_, mut submitted) = self.in_flight.take().expect("matched request");
        if let Err(error) = result {
            submitted.extend(std::mem::take(&mut self.pending));
            self.pending = submitted;
            self.error = Some(error.clone());
        } else {
            self.error = None;
        }
        true
    }
}

impl OpenLessEguiApp {
    pub(super) fn save_settings_if_dirty(&mut self) {
        if let Some(draft) = &self.preferences {
            self.settings_save.capture(draft);
        }
        let Some(native) = &self.native else {
            return;
        };
        let Some((request, edits)) = self.settings_save.begin() else {
            return;
        };
        let host = native.host_arc();
        let tx = self.tx.clone();
        self.status = tr_l10n(self.lang, "common.saving").to_string();
        self.tokio.spawn(async move {
            let backend = host.backend().clone();
            let result = async {
                tokio::task::spawn_blocking(move || host.update_preference_fields(&edits))
                    .await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())?;
                let preferences = backend.get_preferences();
                backend
                    .services()
                    .remote_input
                    .configure(openless_core::RemoteInputConfig {
                        enabled: preferences.remote_input_enabled,
                        port: preferences.remote_input_port,
                    })
                    .await
                    .map_err(|error| error.to_string())
            }
            .await;
            let _ = tx.send(UiResult::SettingsSaved { request, result });
        });
    }

    pub(super) fn finish_settings_save(&mut self, request: u64, result: Result<(), String>) {
        if !self.settings_save.finish(request, &result) {
            return;
        }
        if let Some(backend) = self.backend() {
            self.preferences = Some(self.settings_save.rebase(&backend.get_preferences()));
            self.snapshot = Some(backend.snapshot());
        }
        self.hydrate_text_fields = true;
        // 设置写入是「用户改了东西但看起来没保存」类问题的分界线：
        // 成功一条 debug，失败一条 warn（带字段数），不问就不知道卡在哪一段。
        match &result {
            Ok(()) => log::debug!("[settings] saved request={request}"),
            Err(error) => log::warn!("[settings] save request={request} failed: {error}"),
        }
        match result {
            Ok(()) => {
                self.status = tr_l10n(self.lang, "status.settings_saved").to_string();
                self.load_overview();
                self.save_settings_if_dirty();
            }
            Err(error) => self.status = error,
        }
    }
    pub(super) fn apply_settings_toggle(&mut self, field: frontend::view_model::SettingsField) {
        let Some(preferences) = self.preferences.as_mut() else {
            return;
        };
        match field {
            frontend::view_model::SettingsField::StreamingInsert => {
                preferences.streaming_insert = !preferences.streaming_insert;
            }
            frontend::view_model::SettingsField::StableTranscription => {
                preferences.stable_transcription_enabled =
                    !preferences.stable_transcription_enabled;
            }
            frontend::view_model::SettingsField::StartMinimized => {
                preferences.start_minimized = !preferences.start_minimized;
            }
            frontend::view_model::SettingsField::AutoUpdate => {
                preferences.auto_update_check = !preferences.auto_update_check;
            }
            frontend::view_model::SettingsField::RemoteInput => {
                preferences.remote_input_enabled = !preferences.remote_input_enabled;
            }
            frontend::view_model::SettingsField::ActivityHeatmap => {
                preferences.show_overview_activity_heatmap =
                    !preferences.show_overview_activity_heatmap;
            }
            frontend::view_model::SettingsField::RestoreClipboard => {
                preferences.restore_clipboard_after_paste =
                    !preferences.restore_clipboard_after_paste;
            }
            frontend::view_model::SettingsField::SystemProxy => {
                preferences.use_system_proxy = !preferences.use_system_proxy;
            }
            frontend::view_model::SettingsField::Multimodal => {
                preferences.multimodal_pipeline_enabled = !preferences.multimodal_pipeline_enabled;
            }
            frontend::view_model::SettingsField::SelectionVoiceEnabled => {
                preferences.selection_voice_enabled = !preferences.selection_voice_enabled;
            }
            frontend::view_model::SettingsField::SelectionVoiceAutoIntent => {
                // Tauri 的开关只有两态：当前是 Auto 就关掉（退回启发式），否则打开。
                preferences.selection_voice_intent_mode = if matches!(
                    preferences.selection_voice_intent_mode,
                    openless_core::shared_types::SelectionVoiceIntentMode::Auto
                ) {
                    openless_core::shared_types::SelectionVoiceIntentMode::Heuristic
                } else {
                    openless_core::shared_types::SelectionVoiceIntentMode::Auto
                };
            }
            frontend::view_model::SettingsField::SilenceAutoStop => {
                preferences.silence_auto_stop_enabled = !preferences.silence_auto_stop_enabled;
            }
            frontend::view_model::SettingsField::AudioCue => {
                preferences.audio_cue_on_record = !preferences.audio_cue_on_record;
            }
            frontend::view_model::SettingsField::LlmThinking => {
                preferences.llm_thinking_enabled = !preferences.llm_thinking_enabled;
            }
            frontend::view_model::SettingsField::CapsuleTranscript => {
                preferences.capsule_transcript_enabled = !preferences.capsule_transcript_enabled;
            }
            frontend::view_model::SettingsField::ShowCapsule => {
                preferences.show_capsule = !preferences.show_capsule;

                // 关掉就要立刻收起，否则药丸会留在屏幕上直到下一次录音。
                if !preferences.show_capsule {
                    self.dismiss_capsule();
                }
            }
            frontend::view_model::SettingsField::MuteWhileRecording => {
                preferences.mute_during_recording = !preferences.mute_during_recording;
            }
            frontend::view_model::SettingsField::AudioEnhancement => {
                preferences.audio_enhancement_enabled = !preferences.audio_enhancement_enabled;
            }
            frontend::view_model::SettingsField::EchoCancellation => {
                preferences.echo_cancellation_enabled = !preferences.echo_cancellation_enabled;
            }
            frontend::view_model::SettingsField::RecordAudioForDebug => {
                preferences.record_audio_for_debug = !preferences.record_audio_for_debug;
                // 打开主开关时默认选「处理后」，否则主开关打开了却什么都不落盘、
                // 用户以为坏了；「处理前」保持用户上一次的选择。
                if preferences.record_audio_for_debug
                    && !preferences.debug_archive_processed
                    && !preferences.debug_archive_raw
                {
                    preferences.debug_archive_processed = true;
                }
            }
            frontend::view_model::SettingsField::DebugArchiveProcessed => {
                preferences.debug_archive_processed = !preferences.debug_archive_processed;
            }
            frontend::view_model::SettingsField::DebugArchiveRaw => {
                preferences.debug_archive_raw = !preferences.debug_archive_raw;
            }
            frontend::view_model::SettingsField::StreamingSaveClipboard => {
                preferences.streaming_insert_save_clipboard =
                    !preferences.streaming_insert_save_clipboard;
            }
            frontend::view_model::SettingsField::LaunchAtLogin => {
                preferences.launch_at_login = !preferences.launch_at_login;
            }
        }
        self.save_settings_if_dirty();
    }

    /// Apply a settings combo change from the frontend.
    pub(super) fn apply_settings_combo(
        &mut self,
        field: frontend::view_model::SettingsComboField,
        index: usize,
    ) {
        // AEC 参考源是 Core 偏好里的字符串（空 = 自动，"off" = 不采集），
        // 界面用下标表达，这里换算回去。
        if matches!(field, frontend::view_model::SettingsComboField::AecReference) {
            let value = match index {
                0 => String::new(),
                1 => "off".to_string(),
                other => self
                    .frontend_vm
                    .settings
                    .aec_reference_devices
                    .get(other - 2)
                    .cloned()
                    .unwrap_or_default(),
            };
            self.frontend_vm.settings.aec_reference_index = index;
            if let Some(preferences) = self.preferences.as_mut() {
                preferences.aec_reference_device_name = value;
                self.save_settings_if_dirty();
            }
            return;
        }

        // 日志级别是宿主本地设置（`<data_dir>/logs/level`），不经过 Core 偏好，
        // 因此要放在下面 `self.preferences` 的守卫之前 —— 否则偏好还没加载时
        // 连日志级别都改不了。
        if matches!(field, frontend::view_model::SettingsComboField::LogLevel) {
            let Some(backend) = self.backend() else {
                return;
            };
            let level = ["info", "debug", "trace"]
                .get(index)
                .copied()
                .unwrap_or("info");
            let data_dir = backend.config().data_dir.clone();
            match openless_linux_egui::write_log_level(&data_dir, level) {
                Ok(parsed) => {
                    self.frontend_vm.settings.log_level = index;
                    log::info!(
                        "[logging] level set to {} (takes effect on the next start)",
                        openless_linux_egui::level_name(parsed)
                    );
                    self.status = tr_l10n(self.lang, "status.log_level_saved").to_string();
                }
                Err(error) => {
                    log::warn!("[logging] failed to persist the level: {error}");
                    self.status = error.to_string();
                }
            }
            return;
        }
        let Some(preferences) = self.preferences.as_mut() else {
            return;
        };
        match field {
            // 已在函数开头处理（不走 Core 偏好）。
            frontend::view_model::SettingsComboField::LogLevel => {}
            // 已在函数开头处理（换算成偏好里的设备名字符串）。
            frontend::view_model::SettingsComboField::AecReference => {}
            frontend::view_model::SettingsComboField::Theme => {
                preferences.theme_mode = match index {
                    0 => openless_core::shared_types::ThemeMode::System,
                    1 => openless_core::shared_types::ThemeMode::Light,
                    2 => openless_core::shared_types::ThemeMode::Dark,
                    _ => return,
                };

                self.frontend_vm.settings.theme = index;
            }
            frontend::view_model::SettingsComboField::Language => {
                let pref = match index {
                    0 => LocalePref::System,
                    1 => LocalePref::Lang(Lang::ZhCn),
                    2 => LocalePref::Lang(Lang::ZhTw),
                    3 => LocalePref::Lang(Lang::En),
                    4 => LocalePref::Lang(Lang::Ja),
                    5 => LocalePref::Lang(Lang::Ko),
                    _ => return,
                };
                self.apply_locale_pref(pref);
                self.frontend_vm.settings.language = index;
            }
            frontend::view_model::SettingsComboField::RecordingMode => {
                // Tauri 的三档：切换式 / 按住说话 / 自动识别。
                preferences.hotkey.mode = match index {
                    1 => openless_core::shared_types::HotkeyMode::Hold,
                    2 => openless_core::shared_types::HotkeyMode::Auto,
                    _ => openless_core::shared_types::HotkeyMode::Toggle,
                };
            }
            frontend::view_model::SettingsComboField::SelectionPolishDelivery => {
                preferences.selection_polish_output_mode = match index {
                    1 => openless_core::shared_types::SelectionPolishOutputMode::PreviewConfirm,
                    _ => openless_core::shared_types::SelectionPolishOutputMode::DirectReplace,
                };
            }
            frontend::view_model::SettingsComboField::SelectionVoicePlanFormat => {
                preferences.selection_voice_edit_plan_format = if index == 1 {
                    openless_core::edit_plan::EditPlanFormat::Json
                } else {
                    openless_core::edit_plan::EditPlanFormat::Xml
                };
            }
            frontend::view_model::SettingsComboField::PipelineMode => {
                // Tauri's selector enables the pipeline flag in both modes;
                // only the selected mode is consumed, and both namespaces survive.
                preferences.multimodal_pipeline_enabled = true;
                preferences.pipeline_mode = if index == 1 {
                    openless_core::shared_types::PipelineMode::Multimodal
                } else {
                    openless_core::shared_types::PipelineMode::Traditional
                };
            }
            frontend::view_model::SettingsComboField::SilenceSeconds => {
                preferences.silence_auto_stop_seconds = index as f32 + 1.0;
            }
            frontend::view_model::SettingsComboField::CapsuleTranscriptFontSize => {
                preferences.capsule_transcript_font_size = (12 + 2 * index.min(4)) as u8;
            }
            frontend::view_model::SettingsComboField::CapsuleStyle => {
                preferences.capsule_style = match index {
                    1 => openless_core::shared_types::CapsuleStyle::Classic,
                    2 => openless_core::shared_types::CapsuleStyle::Typeless,
                    _ => openless_core::shared_types::CapsuleStyle::Siri,
                };
            }
            frontend::view_model::SettingsComboField::Microphone => {
                preferences.microphone_device_name = if index == 0 {
                    String::new()
                } else {
                    self.frontend_vm
                        .settings
                        .microphone_options
                        .get(index - 1)
                        .cloned()
                        .unwrap_or_default()
                };
            }
            frontend::view_model::SettingsComboField::RemoteDefaultMode => {
                preferences.remote_input_default_mode = if index == 1 {
                    "hold".to_string()
                } else {
                    "toggle".to_string()
                };
            }
        }
        self.save_settings_if_dirty();
    }

    /// 快捷键录入完成：写入对应偏好，并以 strict 模式保存以便立即应用热键副作用。
    pub(super) fn apply_shortcut_captured(
        &mut self,
        field: frontend::view_model::ShortcutField,
        primary: String,
        modifiers: Vec<String>,
    ) {
        let binding = openless_core::shared_types::ShortcutBinding { primary, modifiers };
        if let Err(error) = openless_core::validate_shortcut_binding(&binding) {
            self.frontend_vm.settings_notice = Some(fmt_l10n(
                self.lang,
                "settings.recording.combo_conflict",
                &[&error.to_string()],
            ));
            self.frontend_vm.shortcut_recording = None;
            return;
        }
        // 修饰键触发（按住说话）照常保存：插件只观察不吞修饰键，
        // 按住期间若又按了别的键就判定为组合键、放弃触发。
        let draft_pack_id = self
            .frontend_vm
            .style_packs
            .get(self.frontend_vm.style_hotkey_draft_pack)
            .map(|pack| pack.id.clone());
        let Some(preferences) = self.preferences.as_mut() else {
            self.frontend_vm.shortcut_recording = None;
            self.frontend_vm.shortcut_pending_modifier = None;
            return;
        };
        use frontend::view_model::ShortcutField;
        match field {
            ShortcutField::Dictation => preferences.dictation_hotkey = binding,
            ShortcutField::Translation => preferences.translation_hotkey = binding,
            ShortcutField::Qa => preferences.qa_hotkey = Some(binding),
            ShortcutField::QuickNote => preferences.quick_note_hotkey = Some(binding),
            ShortcutField::SwitchStyle => preferences.switch_style_hotkey = Some(binding),
            ShortcutField::OpenApp => preferences.open_app_hotkey = Some(binding),
            ShortcutField::SelectionPolish => preferences.selection_polish_hotkey = Some(binding),
            ShortcutField::StylePack(index) => {
                if let Some(row) = preferences.style_pack_hotkeys.get_mut(index) {
                    row.binding = binding;
                }
            }
            ShortcutField::StyleDraft => {
                if let Some(pack_id) = draft_pack_id {
                    preferences
                        .style_pack_hotkeys
                        .retain(|entry| entry.pack_id != pack_id);
                    preferences
                        .style_pack_hotkeys
                        .push(openless_core::shared_types::StylePackHotkey { pack_id, binding });
                    self.frontend_vm.style_hotkey_draft_open = false;
                }
            }
        }

        self.frontend_vm.shortcut_recording = None;
        self.save_settings_if_dirty();
    }

    /// 清空快捷键；录音和翻译使用显式空绑定，避免重新加载时恢复默认键。
    pub(super) fn apply_shortcut_disable(&mut self, field: frontend::view_model::ShortcutField) {
        let Some(preferences) = self.preferences.as_mut() else {
            return;
        };
        use frontend::view_model::ShortcutField;
        match field {
            ShortcutField::Dictation => {
                preferences.dictation_hotkey =
                    openless_core::shared_types::ShortcutBinding::unassigned()
            }
            ShortcutField::Translation => {
                preferences.translation_hotkey =
                    openless_core::shared_types::ShortcutBinding::unassigned()
            }
            ShortcutField::Qa => preferences.qa_hotkey = None,
            ShortcutField::QuickNote => preferences.quick_note_hotkey = None,
            ShortcutField::SwitchStyle => preferences.switch_style_hotkey = None,
            ShortcutField::OpenApp => preferences.open_app_hotkey = None,
            ShortcutField::SelectionPolish => preferences.selection_polish_hotkey = None,
            ShortcutField::StylePack(index) => {
                if let Some(row) = preferences.style_pack_hotkeys.get(index) {
                    let pack_id = row.pack_id.clone();
                    preferences
                        .style_pack_hotkeys
                        .retain(|entry| entry.pack_id != pack_id);
                }
            }
            // 草稿行还没有已保存的绑定。
            ShortcutField::StyleDraft => {
                return;
            }
        }

        self.save_settings_if_dirty();
    }

    /// 恢复出厂默认绑定：默认值来自 `UserPreferences::default()`，与 Core 的
    /// 序列化默认保持一致（不在这里另抄一份键位）。
    pub(super) fn apply_shortcut_reset(&mut self, field: frontend::view_model::ShortcutField) {
        let defaults = openless_core::shared_types::UserPreferences::default();
        let Some(preferences) = self.preferences.as_mut() else {
            return;
        };
        use frontend::view_model::ShortcutField;
        match field {
            ShortcutField::Dictation => preferences.dictation_hotkey = defaults.dictation_hotkey,
            ShortcutField::Translation => {
                preferences.translation_hotkey = defaults.translation_hotkey
            }
            ShortcutField::Qa => preferences.qa_hotkey = defaults.qa_hotkey,
            ShortcutField::QuickNote => preferences.quick_note_hotkey = defaults.quick_note_hotkey,
            ShortcutField::SwitchStyle => {
                preferences.switch_style_hotkey = defaults.switch_style_hotkey
            }
            ShortcutField::OpenApp => preferences.open_app_hotkey = defaults.open_app_hotkey,
            ShortcutField::SelectionPolish => {
                preferences.selection_polish_hotkey = defaults.selection_polish_hotkey
            }
            // 风格直达键与草稿行没有出厂默认。
            ShortcutField::StylePack(_) | ShortcutField::StyleDraft => {
                return;
            }
        }
        self.frontend_vm.shortcut_recording = None;
        self.save_settings_if_dirty();
    }

    pub(super) fn apply_style_hotkey_remove(&mut self, index: usize) {
        let pack_id = self
            .frontend_vm
            .settings
            .style_pack_hotkeys
            .get(index)
            .map(|row| row.pack_id.clone());
        let Some(pack_id) = pack_id else {
            return;
        };
        if let Some(preferences) = self.preferences.as_mut() {
            preferences
                .style_pack_hotkeys
                .retain(|entry| entry.pack_id != pack_id);
        }

        self.save_settings_if_dirty();
    }

    /// 换绑到另一个风格包（目标包已有绑定时忽略，与 Tauri 的下拉置灰同义）。
    pub(super) fn apply_style_hotkey_repack(&mut self, index: usize, pack_index: usize) {
        let pack_id = self
            .frontend_vm
            .settings
            .style_pack_hotkeys
            .get(index)
            .map(|row| row.pack_id.clone());
        let target = self
            .frontend_vm
            .style_packs
            .get(pack_index)
            .map(|pack| pack.id.clone());
        let (Some(current), Some(target)) = (pack_id, target) else {
            return;
        };
        if current == target {
            return;
        }
        if let Some(preferences) = self.preferences.as_mut() {
            if preferences
                .style_pack_hotkeys
                .iter()
                .any(|entry| entry.pack_id == target)
            {
                return;
            }
            if let Some(entry) = preferences
                .style_pack_hotkeys
                .iter_mut()
                .find(|entry| entry.pack_id == current)
            {
                entry.pack_id = target;
            }
        }

        self.save_settings_if_dirty();
    }

    /// Apply a settings text field change from the frontend.
    pub(super) fn apply_settings_text(
        &mut self,
        field: frontend::view_model::SettingsTextField,
        text: String,
    ) {
        let Some(preferences) = self.preferences.as_mut() else {
            return;
        };
        match field {
            frontend::view_model::SettingsTextField::RemotePort => {
                if let Ok(port) = text.parse::<u16>() {
                    preferences.remote_input_port = port;

                    self.frontend_vm.settings.remote_port = text;
                }
            }
            frontend::view_model::SettingsTextField::RetentionDays => {
                let parsed = text.trim().parse::<u32>().unwrap_or(0).min(365);
                preferences.history_retention_days = parsed;

                self.frontend_vm.settings.retention_days = parsed.to_string();
            }
            frontend::view_model::SettingsTextField::PolishContextWindow => {
                let parsed = text.trim().parse::<u32>().unwrap_or(0).min(60);
                preferences.polish_context_window_minutes = parsed;

                self.frontend_vm.settings.polish_context_window = parsed.to_string();
            }
            frontend::view_model::SettingsTextField::PolishSkipMaxChars => {
                // Issue #1032: 0 = off. Cap at 50 to keep the skip limited to short fragments.
                let parsed = text.trim().parse::<u32>().unwrap_or(0).min(50);
                preferences.polish_skip_max_chars = parsed;

                self.frontend_vm.settings.polish_skip_max_chars = parsed.to_string();
            }
            frontend::view_model::SettingsTextField::AudioRecordingMaxEntries => {
                preferences.audio_recording_max_entries = text
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .map(|value| value.clamp(1, 200));

                self.frontend_vm.settings.audio_recording_max_entries = text;
            }
            frontend::view_model::SettingsTextField::SelectionVoiceKeywords => {
                // Tauri：一行一条，空行丢弃。
                preferences.selection_voice_edit_keywords = text
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect();

                self.frontend_vm.settings.selection_voice_keywords = text;
            }
            frontend::view_model::SettingsTextField::SelectionVoiceSystemPrompt => {
                preferences.selection_voice_edit_system_prompt = text.clone();

                self.frontend_vm.settings.selection_voice_system_prompt = text;
            }
            frontend::view_model::SettingsTextField::HistoryMaxEntries => {
                preferences.history_max_entries = text
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .map(|value| value.clamp(5, 200));

                self.frontend_vm.settings.history_max_entries = text;
            }
        }
        self.save_settings_if_dirty();
    }

    /// Apply a settings action button from the frontend.
    pub(super) fn apply_settings_action(
        &mut self,
        field: frontend::view_model::SettingsActionField,
    ) {
        match field {
            frontend::view_model::SettingsActionField::RefreshLinuxAccess => {
                if !self.frontend_vm.linux_access.checking {
                    self.refresh_linux_access();
                    self.load_microphones();
                }
            }
            frontend::view_model::SettingsActionField::RetrySave => {
                self.settings_save.error = None;
                self.save_settings_if_dirty();
            }
            frontend::view_model::SettingsActionField::ExportDiagnostics => {
                if let Some(backend) = self.backend() {
                    let data_dir = backend.config().data_dir.clone();
                    let lang = self.lang;
                    self.spawn(async move {
                        let destination = tokio::task::spawn_blocking(|| {
                            rfd::FileDialog::new()
                                // 导出的是「当前 + 全部轮转历史」，默认名字别再叫 openless.log，
                                // 否则用户会以为是那个单文件。
                                .add_filter("Log", &["log", "txt"])
                                .set_file_name("openless-diagnostics.log")
                                .save_file()
                        })
                        .await
                        .map_err(|error| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Internal,
                                error.to_string(),
                            )
                        })?
                        .ok_or_else(|| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Cancelled,
                                tr_l10n(lang, "dialog.export_log_cancelled"),
                            )
                        })?;
                        tokio::task::spawn_blocking(move || {
                            openless_linux_egui::export_error_logs(&data_dir, &destination)
                        })
                        .await
                        .map_err(|error| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Internal,
                                error.to_string(),
                            )
                        })?
                        .map_err(|error| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Platform,
                                error.to_string(),
                            )
                        })?;
                        Ok(tr_l10n(lang, "status.export_log_done").to_string())
                    });
                }
            }
            frontend::view_model::SettingsActionField::CopyCertFingerprint => {
                let fingerprint = self
                    .remote_access
                    .as_ref()
                    .and_then(|(status, _)| status.ca_fingerprint_sha256.clone());
                match fingerprint {
                    Some(fingerprint) => match fcitx5_copy_to_clipboard(&fingerprint) {
                        Ok(()) => {
                            self.frontend_vm.settings_notice =
                                Some(tr_l10n(self.lang, "status.copied").to_string());
                        }
                        Err(error) => {
                            self.frontend_vm.settings_notice =
                                Some(fmt_l10n(self.lang, "status.copy_failed", &[&error]));
                        }
                    },
                    None => {
                        self.frontend_vm.settings_notice = Some(
                            tr_l10n(
                                self.lang,
                                "settings.remote_input.cert_fingerprint_unavailable",
                            )
                            .to_string(),
                        );
                    }
                }
            }
            frontend::view_model::SettingsActionField::ResetSelectionVoicePrompt => {
                // 留空 = 按「自定义 → 风格包 → 内置默认」回退。
                if let Some(preferences) = self.preferences.as_mut() {
                    preferences.selection_voice_edit_system_prompt = String::new();
                }
                self.frontend_vm.settings.selection_voice_system_prompt = String::new();
            }
            frontend::view_model::SettingsActionField::PreviewAudioCue => {
                // 与真实录音开始时同一段合成提示音（Tauri `playRecordStartCue`）。
                openless_linux_egui::play_cue_start();
            }
            frontend::view_model::SettingsActionField::OpenGitHub => {
                let _ = open_external(REPOSITORY_URL);
            }
            frontend::view_model::SettingsActionField::OpenHelp => {
                let _ = open_external(HELP_URL);
            }
            frontend::view_model::SettingsActionField::OpenReleaseNotes => {
                let _ = open_external(RELEASE_NOTES_URL);
            }
            frontend::view_model::SettingsActionField::OpenFeedback => {
                let _ = open_external(ISSUES_URL);
            }
            frontend::view_model::SettingsActionField::CopyQQ => {
                match fcitx5_copy_to_clipboard("1078960553") {
                    Ok(()) => {
                        self.frontend_vm.settings_notice =
                            Some(tr_l10n(self.lang, "status.copied").to_string());
                    }
                    Err(error) => {
                        self.frontend_vm.settings_notice = Some(fmt_l10n(
                            self.lang,
                            "status.copy_failed",
                            &[&error.to_string()],
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_dictation_and_translation_survives_the_settings_patch() {
        let mut app = OpenLessEguiApp::new(
            Arc::new(tokio::runtime::Runtime::new().unwrap()),
            Err("fixture".into()),
            None,
            false,
        );
        let initial = UserPreferences::default();
        app.preferences = Some(initial.clone());
        app.settings_save = SettingsSave::new(&initial);
        app.apply_shortcut_disable(frontend::view_model::ShortcutField::Dictation);
        app.apply_shortcut_disable(frontend::view_model::ShortcutField::Translation);
        let current = app.preferences.as_ref().unwrap();
        assert!(current.dictation_hotkey.is_unassigned());
        assert!(current.translation_hotkey.is_unassigned());
        let (_, edits) = app.settings_save.begin().unwrap();
        let persisted = openless_linux_egui::patch_preferences(&initial, &edits).unwrap();
        let loaded: UserPreferences =
            serde_json::from_value(serde_json::to_value(persisted).unwrap()).unwrap();
        assert!(loaded.dictation_hotkey.is_unassigned());
        assert!(loaded.translation_hotkey.is_unassigned());
    }

    #[test]
    fn short_polish_threshold_saves_and_hydrates_the_current_text() {
        let mut app = OpenLessEguiApp::new(
            Arc::new(tokio::runtime::Runtime::new().unwrap()),
            Err("fixture".into()),
            None,
            false,
        );
        let mut persisted = UserPreferences::default();
        app.preferences = Some(persisted.clone());
        app.settings_save = SettingsSave::new(&persisted);
        for (text, expected) in [("8", 8), ("", 0), ("99", 50), ("invalid", 0)] {
            app.apply_settings_text(
                frontend::view_model::SettingsTextField::PolishSkipMaxChars,
                text.to_string(),
            );
            assert_eq!(
                app.preferences.as_ref().unwrap().polish_skip_max_chars,
                expected
            );
            assert_eq!(
                app.frontend_vm.settings.polish_skip_max_chars,
                expected.to_string()
            );
            let (request, edits) = app.settings_save.begin().unwrap();
            persisted = openless_linux_egui::patch_preferences(&persisted, &edits).unwrap();
            persisted = serde_json::from_value(serde_json::to_value(&persisted).unwrap()).unwrap();
            assert_eq!(persisted.polish_skip_max_chars, expected);
            assert!(app.settings_save.finish(request, &Ok(())));
        }
    }

    #[test]
    fn queue_keeps_later_edits_external_changes_and_failed_drafts() {
        let mut draft = UserPreferences::default();
        let mut queue = SettingsSave::new(&draft);
        draft.remote_input_port = 9001;
        queue.capture(&draft);
        let (first, _) = queue.begin().unwrap();
        draft.remote_input_port = 9002;
        draft.show_capsule = !draft.show_capsule;
        queue.capture(&draft);
        assert!(queue.begin().is_none());
        let latest = UserPreferences {
            microphone_device_name: "external".into(),
            ..Default::default()
        };
        let merged = queue.rebase(&latest);
        assert_eq!(merged.remote_input_port, 9002);
        assert_eq!(merged.show_capsule, draft.show_capsule);
        assert_eq!(merged.microphone_device_name, "external");
        assert!(!queue.finish(first + 1, &Ok(())));
        assert!(queue.finish(first, &Ok(())));
        let (second, edits) = queue.begin().unwrap();
        assert_eq!(edits["/remoteInputPort"], 9002);
        assert!(!queue.finish(first, &Ok(())));
        assert!(queue.finish(second, &Err("registration failed".into())));
        assert!(queue.begin().is_none());
        assert_eq!(queue.rebase(&latest).remote_input_port, 9002);
        queue.error = None;
        let (retry, _) = queue.begin().unwrap();
        assert!(queue.finish(retry, &Ok(())));
        assert!(queue.pending.is_empty());
        assert!(queue.error.is_none());
    }

    #[test]
    fn every_previously_omitted_field_becomes_an_independent_patch() {
        let initial = UserPreferences::default();
        let mut draft = initial.clone();
        draft.streaming_insert_save_clipboard = !initial.streaming_insert_save_clipboard;
        draft.show_capsule = !initial.show_capsule;
        draft.capsule_style = openless_core::shared_types::CapsuleStyle::Classic;
        draft.selection_polish_output_mode =
            openless_core::shared_types::SelectionPolishOutputMode::PreviewConfirm;
        draft.polish_context_window_minutes = 17;
        draft.polish_skip_max_chars = 8;
        draft.audio_recording_max_entries = Some(19);
        draft.coding_agent_enabled = !initial.coding_agent_enabled;
        draft.coding_agent_provider = "codex-cli".into();
        draft.coding_agent_permission_mode = "plan".into();
        draft.coding_agent_model = Some("test-model".into());
        draft.coding_agent_workdir = Some("/tmp/work".into());
        draft.coding_agent_exe = Some("/bin/codex".into());
        draft.remote_input_default_mode = "hold".into();
        draft.working_languages = vec!["ja".into()];
        draft.translation_target_language = "ko".into();
        draft.selection_polish_style_pack_id = "selection-style".into();
        draft.style_pack_hotkeys = vec![openless_core::shared_types::StylePackHotkey {
            pack_id: "style".into(),
            binding: openless_core::shared_types::ShortcutBinding {
                primary: "1".into(),
                modifiers: vec!["alt".into()],
            },
        }];
        draft.hotkey.mode = openless_core::shared_types::HotkeyMode::Hold;
        let mut queue = SettingsSave::new(&initial);
        queue.capture(&draft);
        let (_, edits) = queue.begin().unwrap();
        let mut external = initial.clone();
        external.microphone_device_name = "external microphone".into();
        let merged = openless_linux_egui::patch_preferences(&external, &edits).unwrap();
        draft.microphone_device_name = external.microphone_device_name;
        assert_eq!(
            serde_json::to_value(merged).unwrap(),
            serde_json::to_value(draft).unwrap()
        );
        assert!(edits.contains_key("/hotkey/mode"));
    }

    /// 外链必须指向与 Tauri 相同的仓库/页面（回归：曾经指向 earendil-works）。
    #[test]
    fn settings_links_match_the_tauri_targets() {
        assert_eq!(REPOSITORY_URL, "https://github.com/Open-Less/openless");
        assert_eq!(HELP_URL, "https://github.com/Open-Less/openless#readme");
        assert_eq!(
            RELEASE_NOTES_URL,
            "https://github.com/Open-Less/openless/releases"
        );
        assert_eq!(ISSUES_URL, "https://github.com/Open-Less/openless/issues");
    }
}
