// Included inside linux_app so native editors can use the same session owners
// and result queue as the main window. No second backend or credential cache.
impl OpenLessEguiApp {
    fn save_field_edits(&mut self, edits: std::collections::BTreeMap<String, serde_json::Value>) {
        if edits.is_empty() {
            return;
        }
        let Some(native) = &self.native else {
            return;
        };
        if let Some(preferences) = &self.preferences {
            match openless_linux_egui::patch_preferences(preferences, &edits) {
                Ok(draft) => self.preferences = Some(draft),
                Err(error) => {
                    self.status = error.to_string();
                    return;
                }
            }
        }
        let host = native.host_arc();
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result = tokio::task::spawn_blocking(move || host.update_preference_fields(&edits))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            let _ = tx.send(UiResult::SettingsSaved(Box::new(result)));
        });
    }

    fn settings_v2(&mut self, ctx: &egui::Context) {
        use crate::ui::settings::{self, SettingsState};
        let id = egui::Id::new("openless-settings-state");
        let mut state = ctx
            .data(|d| d.get_temp::<SettingsState>(id))
            .unwrap_or_default();
        if settings::modal(ctx, &mut state, |ui, state| {
            self.settings_section_v2(ui, state)
        }) {
            self.frontend_vm.settings_open = false;
        }
        ctx.data_mut(|d| d.insert_temp(id, state));
    }

    fn settings_section_v2(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut crate::ui::settings::SettingsState,
    ) {
        use crate::ui::settings::{card, Section};
        use serde_json::{json, Value};
        let mut edits = std::collections::BTreeMap::new();
        let document = self
            .preferences
            .as_ref()
            .and_then(|p| serde_json::to_value(p).ok())
            .unwrap_or(Value::Null);
        macro_rules! fields {
            ($ui:expr,$title:expr,$rows:expr) => {
                card($ui, $title, |ui| {
                    for (pointer, label, kind) in $rows {
                        preference_field(ui, &document, &mut edits, pointer, label, *kind);
                    }
                })
            };
        }
        match state.section {
            Section::General => {
                fields!(
                    ui,
                    "录音",
                    &[
                        (
                            "/hotkey/mode",
                            "录音方式",
                            FieldKind::Choice(&[
                                ("hold", "按住说话"),
                                ("toggle", "切换录音"),
                                ("auto", "智能模式")
                            ])
                        ),
                        ("/silenceAutoStopEnabled", "静音时自动停止", FieldKind::Bool),
                        (
                            "/silenceAutoStopSeconds",
                            "静音等待时间（秒）",
                            FieldKind::Number(0.5, 10.0)
                        ),
                        (
                            "/muteDuringRecording",
                            "录音期间静音系统声音",
                            FieldKind::Bool
                        ),
                        (
                            "/audioCueOnRecord",
                            "开始和停止时播放提示音",
                            FieldKind::Bool
                        ),
                        ("/showCapsule", "显示录音胶囊", FieldKind::Bool),
                    ]
                );
                card(ui, "麦克风", |ui| {
                    let selected = document["microphoneDeviceName"]
                        .as_str()
                        .unwrap_or_default();
                    let mut value = selected.to_string();
                    egui::ComboBox::from_id_salt("microphone-v2")
                        .selected_text(if selected.is_empty() {
                            "跟随系统"
                        } else {
                            selected
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut value, String::new(), "跟随系统");
                            for device in &self.microphones {
                                ui.selectable_value(&mut value, device.name.clone(), &device.name);
                            }
                        });
                    if value != selected {
                        edits.insert("/microphoneDeviceName".into(), json!(value));
                    }
                });
                card(ui, "文本输入", |ui| {
                    for (pointer, label) in [
                        ("/streamingInsert", "流式插入"),
                        ("/restoreClipboardAfterPaste", "粘贴后恢复剪贴板"),
                        ("/streamingInsertSaveClipboard", "保留流式文本到剪贴板"),
                    ] {
                        preference_field(
                            ui,
                            &document,
                            &mut edits,
                            pointer,
                            label,
                            FieldKind::Bool,
                        );
                    }
                });
                card(ui, "手机输入", |ui| {
                    preference_field(
                        ui,
                        &document,
                        &mut edits,
                        "/remoteInputEnabled",
                        "启用手机输入",
                        FieldKind::Bool,
                    );
                    preference_field(
                        ui,
                        &document,
                        &mut edits,
                        "/remoteInputPort",
                        "服务端口",
                        FieldKind::Number(1024.0, 65535.0),
                    );
                    if let Some((remote, pin)) = &self.remote_access {
                        ui.label(if remote.running {
                            "服务运行中"
                        } else if remote.starting {
                            "正在启动…"
                        } else {
                            "服务已停止"
                        });
                        if remote.running {
                            ui.label(format!("已连接 {} 台设备", remote.connection_count));
                            ui.monospace(format!("PIN  {pin}"));
                            for url in &remote.urls {
                                ui.hyperlink(url);
                            }
                        }
                    }
                    if ui.button(theme::text("重置配对码")).clicked() {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend
                                    .services()
                                    .remote_input
                                    .regenerate_pairing_pin()
                                    .await?;
                                Ok("配对码已重置".into())
                            });
                        }
                    }
                });
            }
            Section::Shortcuts => {
                card(ui, "全局快捷键", |ui| {
                    if let Some(prefs) = self.preferences.as_mut() {
                        let mut changed = false;
                        changed |= shortcut_editor(ui, "听写", &mut prefs.dictation_hotkey);
                        changed |= shortcut_editor(ui, "翻译", &mut prefs.translation_hotkey);
                        changed |= optional_shortcut_editor(
                            ui,
                            self.lang,
                            "划词追问",
                            &mut prefs.qa_hotkey,
                            ";",
                        );
                        changed |= optional_shortcut_editor(
                            ui,
                            self.lang,
                            "划词润色",
                            &mut prefs.selection_polish_hotkey,
                            "P",
                        );
                        changed |= optional_shortcut_editor(
                            ui,
                            self.lang,
                            "切换风格",
                            &mut prefs.switch_style_hotkey,
                            "S",
                        );
                        changed |= optional_shortcut_editor(
                            ui,
                            self.lang,
                            "显示主窗口",
                            &mut prefs.open_app_hotkey,
                            "O",
                        );
                        changed |= optional_shortcut_editor(
                            ui,
                            self.lang,
                            "Less Computer 语音",
                            &mut prefs.coding_agent_voice_hotkey,
                            "L",
                        );
                        changed |= optional_shortcut_editor(
                            ui,
                            self.lang,
                            "Less Computer 面板",
                            &mut prefs.coding_agent_panel_hotkey,
                            "K",
                        );
                        changed |= optional_shortcut_editor(ui,self.lang,"Less Computer 快速输入",&mut prefs.coding_agent_quick_hotkey,"J");
                        for pack in &self.style_packs {
                            let mut binding=prefs.style_pack_hotkeys.iter().find(|h|h.pack_id==pack.id).map(|h|h.binding.clone());
                            if optional_shortcut_editor(ui,self.lang,&pack.name,&mut binding,"1") {
                                set_style_pack_hotkey(prefs,&pack.id,binding);changed=true;
                            }
                        }
                        if changed {
                            self.settings_dirty.hotkeys = true;
                        }
                    }
                });
                fields!(
                    ui,
                    "划词语音",
                    &[
                        ("/selectionVoiceEnabled", "启用划词语音", FieldKind::Bool),
                        (
                            "/selectionVoiceIntentMode",
                            "意图识别",
                            FieldKind::Choice(&[("auto", "自动识别"), ("manual", "手动选择")])
                        ),
                        (
                            "/selectionVoiceManualIntent",
                            "默认意图",
                            FieldKind::Choice(&[("question", "追问"), ("edit", "修改")])
                        ),
                        ("/qaSaveHistory", "保存追问历史", FieldKind::Bool)
                    ]
                );
                self.save_settings_if_dirty();
            }
            Section::Services => {
                let omni_enabled = self.preferences.as_ref().is_some_and(|p| p.multimodal_pipeline_enabled);
                let omni = self.preferences.as_ref().is_some_and(|p| {
                    p.multimodal_pipeline_enabled
                        && p.pipeline_mode == openless_core::shared_types::PipelineMode::Multimodal
                });
                ui.horizontal_wrapped(|ui| {
                    let mut tabs = if omni {
                        vec![(2, "多模态"), (3, "本地模型"), (4, "网络与连接")]
                    } else {
                        vec![
                            (0, "语言模型"),
                            (1, "语音识别"),
                            (3, "本地模型"),
                            (4, "网络与连接"),
                        ]
                    };
                    if omni_enabled && !omni {tabs.insert(2,(2,"多模态"));}
                    if !tabs.iter().any(|(index, _)| *index == state.service) {
                        state.service = tabs[0].0;
                    }
                    for (index, label) in tabs {
                        if ui.selectable_label(state.service == index, label).clicked() {
                            state.service = index;
                        }
                    }
                });
                ui.add_space(12.0);
                match state.service {
                    2 => self.omni_v2(ui),
                    0..=1 => {
                        let kind = match state.service {
                            1 => openless_core::ChannelKind::Asr,
                            _ => openless_core::ChannelKind::Llm,
                        };
                        if self.provider_kind != kind {
                            self.load_providers(kind);
                        }
                        self.provider_management_ui(ui);
                    }
                    3 => self.local_models_v2(ui, state),
                    _ => {
                        fields!(
                            ui,
                            "网络",
                            &[("/useSystemProxy", "使用系统代理", FieldKind::Bool)]
                        );
                        card(ui, "GitHub 账户与风格市场", |ui| {
                            self.marketplace_account_v2(ui)
                        });
                    }
                }
            }
            Section::Appearance => {
                fields!(
                    ui,
                    "外观",
                    &[
                        (
                            "/themeMode",
                            "主题",
                            FieldKind::Choice(&[
                                ("system", "跟随系统"),
                                ("light", "浅色"),
                                ("dark", "深色")
                            ])
                        ),
                        ("/stackedRowLayout", "纵向排列设置项", FieldKind::Bool),
                        ("/conservativeLayout", "简洁布局", FieldKind::Bool),
                        (
                            "/showOverviewActivityHeatmap",
                            "显示活动热力图",
                            FieldKind::Bool
                        )
                    ]
                );
                card(ui, "语言与字体", |ui| {
                    self.language_selector_ui(ui);
                    let id = egui::Id::new("openless-font-scale");
                    let mut size = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or_else(|| openless_linux_egui::load_ui_value("fontScale").and_then(|v|v.as_f64()).unwrap_or(1.0) as f32);
                    if ui
                        .add(egui::Slider::new(&mut size, 0.85..=1.35).text("字体大小"))
                        .changed()
                    {
                        ui.ctx().data_mut(|d| d.insert_temp(id, size));
                        ui.ctx().set_zoom_factor(size);
                        if let Err(error) = openless_linux_egui::save_ui_value("fontScale", serde_json::json!(size)) {self.status=error.to_string();}
                    }
                });
            }
            Section::Privacy => {
                fields!(
                    ui,
                    "隐私",
                    &[
                        ("/cursorContextEnabled", "读取光标上下文", FieldKind::Bool),
                        ("/qaSaveHistory", "保存划词追问历史", FieldKind::Bool)
                    ]
                );
                fields!(
                    ui,
                    "历史与录音",
                    &[
                        (
                            "/historyRetentionDays",
                            "历史保留天数（0 为永久）",
                            FieldKind::Number(0.0, 3650.0)
                        ),
                        (
                            "/historyMaxEntries",
                            "历史条目上限",
                            FieldKind::OptionalNumber
                        ),
                        ("/recordAudioForDebug", "保存录音", FieldKind::Bool),
                        (
                            "/audioRecordingMaxEntries",
                            "录音条目上限",
                            FieldKind::OptionalNumber
                        )
                    ]
                );
                card(ui, "云同步", |ui| {
                    if let Some(status) = &self.cloud_sync_status {
                        ui.label(if status.has_snapshot {
                            "已有云端备份"
                        } else {
                            "尚无云端备份"
                        });
                        if let Some(updated) = &status.updated_at {
                            ui.label(updated);
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        for (action, label) in [
                            ("status", "刷新状态"),
                            ("upload", "备份到云端"),
                            ("restore", "从云端恢复"),
                            ("delete", "删除云端备份"),
                        ] {
                            if ui.button(label).clicked() {
                                if matches!(action, "restore" | "delete") {
                                    state.confirmation = Some(action.into());
                                } else {
                                    self.cloud_sync_action(action);
                                }
                            }
                        }
                    });
                    if let Some(action) = state
                        .confirmation
                        .clone()
                        .filter(|s| s == "restore" || s == "delete")
                    {
                        ui.colored_label(
                            egui::Color32::from_rgb(217, 119, 6),
                            if action == "restore" {
                                "将用云端快照替换本地词典、纠错与风格，确认继续？"
                            } else {
                                "确认删除云端备份？"
                            },
                        );
                        ui.horizontal(|ui| {
                            if ui.button(theme::text("确认")).clicked() {
                                state.confirmation = None;
                                self.cloud_sync_action(&action);
                            }
                            if ui.button(theme::text("取消")).clicked() {
                                state.confirmation = None;
                            }
                        });
                    }
                });
                card(ui, "权限与数据目录", |ui| {
                    if let Some(backend) = self.backend() {
                        ui.label(backend.config().data_dir.display().to_string());
                        if ui.button(theme::text("打开数据目录")).clicked() {
                            let path = backend.config().data_dir.clone();
                            std::thread::spawn(move || {
                                let _ = openless_linux_egui::open_local_file(&path);
                            });
                        }
                    }
                    ui.label(format!(
                        "桌面会话：{:?}",
                        std::env::var("XDG_SESSION_TYPE").unwrap_or_default()
                    ));
                });
            }
            Section::Advanced => {
                ui.horizontal_wrapped(|ui| {
                    for (index, label) in ["Less Computer", "多模态管线", "调试工具"]
                        .iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut state.advanced, index, *label);
                    }
                });
                match state.advanced {
                    0 => {
                        fields!(
                            ui,
                            "Less Computer",
                            &[
                                ("/codingAgentEnabled", "启用 Less Computer", FieldKind::Bool),
                                ("/codingAgentProvider", "后端", FieldKind::Text),
                                ("/codingAgentExe", "可执行文件", FieldKind::OptionalText),
                                ("/codingAgentWorkdir", "工作目录", FieldKind::OptionalText),
                                ("/codingAgentModel", "模型", FieldKind::OptionalText),
                                (
                                    "/codingAgentPermissionMode",
                                    "权限模式",
                                    FieldKind::Choice(&[
                                        ("default", "默认"),
                                        ("plan", "规划"),
                                        ("acceptEdits", "允许编辑")
                                    ])
                                )
                            ]
                        );
                        self.less_computer_ui(ui);
                    }
                    1 => {
                        fields!(
                            ui,
                            "多模态管线",
                            &[
                                (
                                    "/multimodalPipelineEnabled",
                                    "启用多模态管线",
                                    FieldKind::Bool
                                ),
                                (
                                    "/pipelineMode",
                                    "管线模式",
                                    FieldKind::Choice(&[
                                        ("traditional", "语音识别 + 语言模型"),
                                        ("multimodal", "多模态")
                                    ])
                                ),
                                ("/llmThinkingEnabled", "启用模型思考", FieldKind::Bool)
                            ]
                        );
                    }
                    _ => {
                        card(ui, "诊断", |ui| {
                            ui.label(&self.status);
                            if ui.button(theme::text("导出日志")).clicked() {
                                self.apply_settings_action(
                                    frontend::view_model::SettingsActionField::ExportDiagnostics,
                                );
                            }
                            if ui.button(theme::text("重新加载 fcitx5 插件")).clicked() {
                                self.status = if reload_running_fcitx5() {
                                    "已请求重新加载"
                                } else {
                                    "未能重新加载 fcitx5"
                                }
                                .into();
                            }
                        });
                    }
                }
            }
            Section::About => {
                card(ui,"Linux 桌面集成",|ui| {
                    for (mode,label) in [("install","安装桌面组件"),("enable","启用桌面组件"),("uninstall","卸载桌面组件")] {
                        if ui.button(theme::text(label)).clicked() {self.desktop_component(mode);}
                    }
                });
                card(ui, "OpenLess", |ui| {
                    ui.heading(format!("OpenLess {}", self.app_version()));
                    ui.label(theme::text("开源语音输入 · Linux"));
                    ui.hyperlink_to("GitHub", "https://github.com/Open-Less/openless");
                });
                fields!(
                    ui,
                    "启动与更新",
                    &[
                        ("/launchAtLogin", "登录时启动", FieldKind::Bool),
                        ("/startMinimized", "启动时最小化", FieldKind::Bool),
                        ("/autoUpdateCheck", "自动检查更新", FieldKind::Bool),
                        (
                            "/updateChannel",
                            "更新频道",
                            FieldKind::Choice(&[("stable", "稳定版"), ("beta", "Beta")])
                        )
                    ]
                );
                self.update_controls_v2(ui);
            }
        }
        self.save_field_edits(edits);
        if !self.status.is_empty() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(&self.status)
                    .size(11.0)
                    .color(theme::ink_3()),
            );
        }
    }

    fn app_version(&self) -> String {
        serde_json::from_str::<serde_json::Value>(include_str!("../../package.json"))
            .ok()
            .and_then(|v| v["version"].as_str().map(str::to_owned))
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into())
    }

    fn update_controls_v2(&mut self, ui: &mut egui::Ui) {
        let channel = self
            .preferences
            .as_ref()
            .map(|p| p.update_channel)
            .unwrap_or_default();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.update_busy, egui::Button::new("检查更新"))
                .clicked()
            {
                self.request_update_check(channel);
            }
            if self.update_manifest.is_some()
                && ui
                    .add_enabled(!self.update_busy, egui::Button::new("下载并安装"))
                    .clicked()
            {
                self.install_update();
            }
        });
        if self.update_busy && self.update_cancellation.is_some() && ui.button(theme::text("取消下载")).clicked() {
            if self.update_cancellation.as_ref().is_some_and(|c|c.cancel()) {self.status="正在取消更新…".into();}
        }
        if let Some(progress) = self.update_progress {
            if let Some(total) = progress.content_length.filter(|n| *n > 0) {
                ui.add(egui::ProgressBar::new(
                    progress.downloaded as f32 / total as f32,
                ));
            }
        }
    }

    fn cloud_sync_action(&self, action: &str) {
        let Some(backend) = self.backend() else {
            return;
        };
        let action = action.to_string();
        let scale = openless_linux_egui::load_ui_value("fontScale").and_then(|v|v.as_f64()).unwrap_or(1.0);
        let ui_preferences = openless_core::CloudSyncUiPreferences {
            locale: serde_json::from_value(serde_json::json!(self.locale_pref.to_tag())).ok(),
            font_scale: Some(if scale < 0.95 {openless_core::SyncFontScale::Small} else if scale > 1.05 {openless_core::SyncFontScale::Large} else {openless_core::SyncFontScale::Medium}),
        };
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result = async {
                let latest = backend.cloud_sync_status().await?;
                match action.as_str() {
                    "upload" => {
                        backend
                            .cloud_sync_upload(
                                latest.revision,
                                ui_preferences,
                            )
                            .await
                    }
                    "restore" => {let restored=backend.cloud_sync_restore().await?; let _=tx.send(UiResult::CloudUi(restored.ui_preferences));Ok(restored.status)},
                    "delete" => backend.cloud_sync_delete(latest.revision).await,
                    _ => Ok(latest),
                }
            }
            .await
            .map_err(|e: BackendError| e.to_string());
            let _ = tx.send(UiResult::CloudSync(result));
        });
    }
}

#[derive(Clone, Copy)]
enum FieldKind {
    Bool,
    Text,
    OptionalText,
    OptionalNumber,
    Number(f64, f64),
    Choice(&'static [(&'static str, &'static str)]),
}

fn preference_field(
    ui: &mut egui::Ui,
    document: &serde_json::Value,
    edits: &mut std::collections::BTreeMap<String, serde_json::Value>,
    pointer: &str,
    label: &str,
    kind: FieldKind,
) {
    use serde_json::{json, Value};
    let Some(value) = document.pointer(pointer) else {
        return;
    };
    ui.push_id(pointer, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.set_min_height(32.0);
            ui.label(theme::text(label));
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match kind {
                    FieldKind::Bool => {
                        let mut v = value.as_bool().unwrap_or(false);
                        if ui.checkbox(&mut v, "").changed() {
                            edits.insert(pointer.into(), json!(v));
                        }
                    }
                    FieldKind::Choice(options) => {
                        let mut v = value.as_str().unwrap_or_default().to_string();
                        let title = options
                            .iter()
                            .find(|(id, _)| *id == v)
                            .map(|(_, label)| *label)
                            .unwrap_or(&v);
                        egui::ComboBox::from_id_salt(pointer)
                            .selected_text(title)
                            .show_ui(ui, |ui| {
                                for (id, label) in options {
                                    ui.selectable_value(&mut v, id.to_string(), theme::text(label));
                                }
                            });
                        if value.as_str() != Some(v.as_str()) {
                            edits.insert(pointer.into(), json!(v));
                        }
                    }
                    FieldKind::Number(min, max) => {
                        let mut v = value.as_f64().unwrap_or(min);
                        if ui
                            .add(egui::DragValue::new(&mut v).range(min..=max))
                            .changed()
                        {
                            edits.insert(
                                pointer.into(),
                                if value.is_u64() {
                                    json!(v.round() as u64)
                                } else {
                                    json!(v)
                                },
                            );
                        }
                    }
                    FieldKind::OptionalNumber => {
                        let mut enabled = !value.is_null();
                        if ui.checkbox(&mut enabled, "限制数量").changed() {
                            edits.insert(
                                pointer.into(),
                                if enabled { json!(500) } else { Value::Null },
                            );
                        }
                        if enabled {
                            let mut count = value.as_u64().unwrap_or(500);
                            if ui
                                .add(egui::DragValue::new(&mut count).range(1..=100_000))
                                .changed()
                            {
                                edits.insert(pointer.into(), json!(count));
                            }
                        }
                    }
                    FieldKind::Text | FieldKind::OptionalText => {
                        let id = ui.make_persistent_id(("draft", pointer));
                        let mut text = ui
                            .ctx()
                            .data(|d| d.get_temp::<String>(id))
                            .unwrap_or_else(|| value.as_str().unwrap_or_default().into());
                        let response =
                            ui.add(egui::TextEdit::singleline(&mut text).desired_width(230.0));
                        if response.lost_focus()
                            || (response.has_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            if text != value.as_str().unwrap_or_default() {
                                edits.insert(
                                    pointer.into(),
                                    if matches!(kind, FieldKind::OptionalText)
                                        && text.trim().is_empty()
                                    {
                                        Value::Null
                                    } else {
                                        json!(text)
                                    },
                                );
                            }
                            ui.ctx().data_mut(|d| d.remove::<String>(id));
                        } else if response.has_focus() {
                            ui.ctx().data_mut(|d| d.insert_temp(id, text));
                        }
                    }
                },
            );
        });
        ui.separator();
    });
}
