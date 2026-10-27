use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::icons::{self, IconName};
use super::layout;
use super::theme;
use super::view_model::{
    FrontendAction, FrontendViewModel, SettingsActionField, SettingsChannelProvider,
    SettingsComboField, SettingsField, SettingsProviderAuth, SettingsProviderEditor,
    SettingsProviderField, SettingsSection, SettingsTextField, ShortcutField, StylePack,
    StylePackHotkeyRow,
};

const RAIL_WIDTH: f32 = 214.0;
const SIDEBAR_RAIL_INPUT: f32 = 150.0;

#[derive(Clone, Copy)]
enum SettingsIcon {
    Settings,
    Mic,
    Sparkle,
    Cloud,
    Shield,
    Bolt,
    Info,
    Help,
    Document,
    External,
}

impl SettingsSection {
    fn label(self, lang: Lang) -> &'static str {
        // Keys are spelled out per arm so the i18n sync script can see them.
        match self {
            Self::General => tr_l10n(lang, "modal.sections.general"),
            Self::Shortcuts => tr_l10n(lang, "modal.sections.shortcuts"),
            Self::Appearance => tr_l10n(lang, "modal.sections.appearance"),
            Self::Services => tr_l10n(lang, "modal.sections.services"),
            Self::Privacy => tr_l10n(lang, "modal.sections.privacy"),
            Self::Advanced => tr_l10n(lang, "modal.sections.advanced"),
            Self::About => tr_l10n(lang, "modal.sections.about"),
        }
    }

    fn description(self, lang: Lang) -> &'static str {
        // Keys are spelled out per arm so the i18n sync script can see them.
        match self {
            Self::General => tr_l10n(lang, "modal.descriptions.general"),
            Self::Shortcuts => tr_l10n(lang, "modal.descriptions.shortcuts"),
            Self::Services => tr_l10n(lang, "modal.descriptions.services"),
            Self::Appearance => tr_l10n(lang, "modal.descriptions.appearance"),
            Self::Privacy => tr_l10n(lang, "settings.linux_privacy.section_desc"),
            Self::Advanced => tr_l10n(lang, "modal.descriptions.advanced"),
            Self::About => tr_l10n(lang, "modal.descriptions.about"),
        }
    }

    fn icon(self) -> SettingsIcon {
        match self {
            Self::General => SettingsIcon::Mic,
            Self::Shortcuts => SettingsIcon::Bolt,
            Self::Appearance => SettingsIcon::Settings,
            Self::Services => SettingsIcon::Cloud,
            Self::Privacy => SettingsIcon::Shield,
            Self::Advanced => SettingsIcon::Sparkle,
            Self::About => SettingsIcon::Info,
        }
    }
}

/// Paint the in-window settings modal. Actions are pushed into the provided vec.
pub fn settings_page(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    body: egui::Rect,
) {
    // 设置是一整页：不铺磨砂遮罩、不压暗、没有落影，也**不内缩**。
    // 以前内缩 8px 让圆角"露出来"，代价是上边和右边各多出一条白边（页面底色
    // 透出来）。现在贴满内容区，圆角只留在左栏的左上/左下（底色是 CONTENT_BG，
    // 内缩的那一点由它填，不会露出白色），窗口自身的圆角负责外轮廓。
    const RADIUS: u8 = 14;
    let surface = body;
    let size = surface.size();
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique("openless-settings-card-rect"), surface));
    egui::Area::new(egui::Id::unique("openless-settings-page"))
        // 仍是 Foreground：页内还有下拉菜单（`Order::Foreground`）需要压在上面。
        .order(egui::Order::Foreground)
        .fixed_pos(surface.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                egui::Frame::new()
                    // Tauri `--ol-settings-content-bg`：整页浅灰底，卡片才是白色。
                    .fill(theme::CONTENT_BG)
                    .corner_radius(egui::CornerRadius::same(RADIUS))
                    .show(ui, |ui| {
                        ui.set_min_size(size);
                        ui.set_max_size(size);
                        let body_height = size.y.max(120.0);
                        ui.horizontal(|ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(RAIL_WIDTH, body_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_min_size(egui::vec2(RAIL_WIDTH, body_height));
                                    ui.set_max_size(egui::vec2(RAIL_WIDTH, body_height));
                                    let column = ui.max_rect();
                                    // 左栏只圆左上/左下：右边缘要和内容区拼在一起。
                                    ui.painter().rect_filled(
                                        column,
                                        egui::CornerRadius {
                                            nw: RADIUS,
                                            sw: RADIUS,
                                            ne: 0,
                                            se: 0,
                                        },
                                        theme::RAIL_BG,
                                    );
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(column.right() - 0.5, column.top()),
                                            column.right_bottom(),
                                        ),
                                        egui::CornerRadius::ZERO,
                                        theme::LINE_SOFT,
                                    );
                                    let inner = column.shrink2(egui::vec2(12.0, 16.0));
                                    ui.scope_builder(
                                        egui::UiBuilder::new().max_rect(inner),
                                        |ui| {
                                            egui::ScrollArea::vertical()
                                                .id_salt("openless-settings-rail")
                                                .auto_shrink([false, false])
                                                .show(ui, |ui| {
                                                    ui.set_width(inner.width());
                                                    rail(ui, vm, actions);
                                                });
                                        },
                                    );
                                },
                            );
                            let gutters = ui.spacing().item_spacing.x;
                            ui.allocate_ui_with_layout(
                                egui::vec2((size.x - RAIL_WIDTH - gutters).max(0.0), body_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    // 内容区白底（Tauri `ol-settings-surface` 白、左栏
                                    // `--ol-settings-rail-bg` 灰）。整页统一用 CONTENT_BG
                                    // 的话，白底上的灰色选中块会变成"灰底灰块"。
                                    let column = ui.max_rect();
                                    ui.painter().rect_filled(
                                        column,
                                        egui::CornerRadius {
                                            nw: 0,
                                            sw: 0,
                                            ne: RADIUS,
                                            se: RADIUS,
                                        },
                                        theme::SURFACE,
                                    );
                                    ui.set_min_height(body_height);
                                    // 渠道编辑器 portal 进「AI 服务与模型」这一页的右栏，
                                    // 覆盖范围必须跟着分区走。
                                    let editor_here = vm.provider_editor.is_some()
                                        && vm.settings_section == SettingsSection::Services;
                                    if editor_here {
                                        channel_editor_panel(ui, vm, actions);
                                    } else {
                                        panel(ui, vm, actions);
                                    }
                                },
                            );
                        });
                    });
            });
        });
}

fn rail(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    // 底色与右侧分隔线由调用方按整栏高度铺好（见 settings_sheet）。
    ui.vertical(|ui| {
        // Tauri `ol-settings-return`：关闭设置回到应用，位置在左栏最上方
        // （搜索框之上），不再是右栏顶栏的 ✕。
        let back = tr_l10n(lang, "uiRefinement.back");
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
        let response = ui.interact(
            rect,
            egui::Id::unique("openless-settings-back"),
            egui::Sense::click(),
        );
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(7), theme::SURFACE_2);
        }
        let chevron = egui::Stroke::new(1.4, theme::INK_2);
        let c = egui::pos2(rect.left() + 9.0, rect.center().y);
        ui.painter().line_segment(
            [egui::pos2(c.x + 3.0, c.y - 5.0), egui::pos2(c.x - 2.5, c.y)],
            chevron,
        );
        ui.painter().line_segment(
            [egui::pos2(c.x - 2.5, c.y), egui::pos2(c.x + 3.0, c.y + 5.0)],
            chevron,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            back,
            egui::FontId::proportional(12.5),
            theme::INK_2,
        );
        if response.clicked() {
            actions.push(FrontendAction::CloseSettings);
        }
        ui.add_space(6.0);
        // Section search, like the Tauri rail.
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.set_width(SIDEBAR_RAIL_INPUT);
                let width = ui.available_width().max(40.0);
                ui.add_sized(
                    [width, 20.0],
                    egui::TextEdit::singleline(&mut vm.settings_query)
                        .id(egui::Id::unique("openless-settings-search"))
                        .hint_text(tr_l10n(lang, "modal.search_placeholder"))
                        // 明确文字颜色：默认的控件前景色在浅底上过淡，
                        // 看上去像「输入了但没有显示字符」。
                        .text_color(theme::INK)
                        .frame(egui::Frame::NONE)
                        .align(egui::Align2::CENTER_CENTER),
                );
            });
        ui.add_space(10.0);

        let query = vm.settings_query.trim().to_lowercase();
        // Tauri's rail order.
        for section in [
            SettingsSection::General,
            SettingsSection::Shortcuts,
            SettingsSection::Services,
            SettingsSection::Appearance,
            SettingsSection::Privacy,
            SettingsSection::Advanced,
            SettingsSection::About,
        ] {
            if !rail_section_visible(section, vm.hotkeys_supported) {
                continue;
            }
            let label = section.label(lang);
            if !query.is_empty() && !label.to_lowercase().contains(&query) {
                continue;
            }
            let response = rail_item(ui, label, section.icon(), vm.settings_section == section);
            if response.clicked() {
                actions.push(FrontendAction::SettingsSection(section));
            }
        }
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(7.0);
        for (label, icon) in [
            (
                tr_l10n(lang, "modal.sections.help_center"),
                SettingsIcon::Help,
            ),
            (
                tr_l10n(lang, "modal.sections.release_notes"),
                SettingsIcon::Document,
            ),
        ] {
            let response = rail_item(ui, label, icon, false);
            let row = response.rect;
            draw_rail_icon(
                ui,
                egui::pos2(row.right() - 14.0, row.center().y),
                SettingsIcon::External,
                theme::INK_4,
            );
            if response.clicked() {
                actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::OpenHelp,
                ));
            }
        }
    });
}

/// Tauri `visibleSettingsSections(supportsDesktopHotkey)`：没有桌面热键后端时
/// 整个「快捷键」分区不出现（其余分区与平台无关）。
fn rail_section_visible(section: SettingsSection, hotkeys_supported: bool) -> bool {
    section != SettingsSection::Shortcuts || hotkeys_supported
}

fn rail_item(ui: &mut egui::Ui, label: &str, icon: SettingsIcon, active: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    // Tauri：激活项是 --ol-blue-soft 底 + --ol-blue 字（滑动块），悬停是
    // --ol-nav-hover-bg + --ol-ink。
    if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::BLUE_SOFT);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::NAV_HOVER);
    }
    let color = if active {
        theme::BLUE
    } else if response.hovered() {
        theme::INK
    } else {
        theme::INK_2
    };
    let icon_center = egui::pos2(rect.left() + 17.0, rect.center().y);
    draw_rail_icon(ui, icon_center, icon, color);
    ui.painter().text(
        egui::pos2(rect.left() + 34.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );
    response
}

fn draw_rail_icon(ui: &egui::Ui, center: egui::Pos2, icon: SettingsIcon, color: egui::Color32) {
    let icon = match icon {
        SettingsIcon::Settings => IconName::Settings,
        SettingsIcon::Mic => IconName::Mic,
        SettingsIcon::Sparkle => IconName::Sparkle,
        SettingsIcon::Cloud => IconName::Cloud,
        SettingsIcon::Shield => IconName::Shield,
        SettingsIcon::Bolt => IconName::Bolt,
        SettingsIcon::Info => IconName::Info,
        SettingsIcon::Help => IconName::Help,
        SettingsIcon::Document => IconName::Doc,
        SettingsIcon::External => IconName::External,
    };
    icons::draw_icon(ui, center, icon, color);
}

fn panel(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(24, 16))
        .show(ui, |ui| {
            {
                // 控件（下拉/输入框/按钮）统一成 Tauri 的 SelectLite / inputStyle：
                // 白底、0.5px --ol-line-strong 描边、r8、高 30。
                let style = ui.style_mut();
                style.visuals.menu_corner_radius = egui::CornerRadius::same(10);
                style.visuals.extreme_bg_color = theme::SURFACE;
                style.spacing.interact_size.y = 30.0;
                style.spacing.button_padding = egui::vec2(9.0, 5.0);
                for widget in [
                    &mut style.visuals.widgets.inactive,
                    &mut style.visuals.widgets.hovered,
                    &mut style.visuals.widgets.active,
                    &mut style.visuals.widgets.open,
                ] {
                    widget.corner_radius = egui::CornerRadius::same(8);
                    widget.bg_stroke = egui::Stroke::new(0.5, theme::LINE_STRONG);
                    widget.bg_fill = theme::SURFACE;
                    widget.weak_bg_fill = theme::SURFACE;
                }
            }
            // 实验与扩展的下钻页把标题换成子页标题，并在左侧给出返回箭头
            // （Tauri 的 `activeAdvancedPage` 顶栏）。
            let detail = if vm.settings_section == SettingsSection::Advanced {
                advanced_page_title(vm, lang)
            } else {
                None
            };
            ui.horizontal(|ui| {
                if let Some((title, _description)) = detail {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                    if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(8),
                            theme::SURFACE_2,
                        );
                    }
                    let center = rect.center();
                    let stroke = egui::Stroke::new(1.4, theme::INK_2);
                    ui.painter().line_segment(
                        [
                            egui::pos2(center.x + 3.0, center.y - 5.0),
                            egui::pos2(center.x - 2.5, center.y),
                        ],
                        stroke,
                    );
                    ui.painter().line_segment(
                        [
                            egui::pos2(center.x - 2.5, center.y),
                            egui::pos2(center.x + 3.0, center.y + 5.0),
                        ],
                        stroke,
                    );
                    if response.clicked() {
                        vm.advanced_open = if vm.advanced_open == 0 { 3 } else { usize::MAX };
                    }
                    ui.label(
                        egui::RichText::new(title)
                            .size(20.0)
                            .strong()
                            .color(theme::INK),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(vm.settings_section.label(lang))
                            .size(21.0)
                            .strong()
                            .color(theme::INK),
                    );
                }
                // Tauri（beta 4+）：关闭设置搬到了左栏顶部的「‹ 返回应用」，
                // 右栏顶栏只剩自动保存提示。
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "modal.auto_save_hint"))
                            .size(12.0)
                            .color(theme::INK_3),
                    );
                });
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(match detail {
                    Some((_, description)) => description,
                    None => vm.settings_section.description(lang),
                })
                .size(13.0)
                .color(theme::INK_3),
            );
            save_state(ui, vm, actions);
            if let Some(notice) = &vm.settings_notice {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(notice).size(11.0).color(theme::BLUE));
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("openless-settings-content")
                .auto_shrink([false, false])
                .show(ui, |ui| match vm.settings_section {
                    SettingsSection::General => general(ui, vm, actions),
                    SettingsSection::Shortcuts => shortcuts(ui, vm, actions),
                    SettingsSection::Appearance => appearance(ui, vm, actions),
                    SettingsSection::Services => services(ui, vm, actions),
                    SettingsSection::Privacy => privacy(ui, vm, actions),
                    SettingsSection::Advanced => advanced(ui, vm, actions),
                    SettingsSection::About => about(ui, vm, actions),
                });
        });
}

pub(super) fn save_state(
    ui: &mut egui::Ui,
    vm: &FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    if vm.settings_saving {
        ui.label(tr_l10n(vm.lang, "common.saving"));
    }
    if let Some(error) = &vm.settings_save_error {
        ui.colored_label(theme::ERR, tr_l10n(vm.lang, "style.pack.unsaved"));
        ui.label(error);
        if ui.button(tr_l10n(vm.lang, "common.retry")).clicked() {
            actions.push(FrontendAction::SettingsAction(
                SettingsActionField::RetrySave,
            ));
        }
    }
}

/// Title + description of the open 实验与扩展 sub-page (`None` on the list page).
fn advanced_page_title(vm: &FrontendViewModel, lang: Lang) -> Option<(&'static str, &'static str)> {
    match vm.advanced_open {
        2 => Some((
            tr_l10n(lang, "settings.debug.title"),
            tr_l10n(lang, "modal.advanced_pages.debug"),
        )),
        _ => None,
    }
}

fn general(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;

    // 录音与输入（Tauri RecordingInputSection）
    // 行的顺序与 Tauri 保持一致：快捷键 → 录音方式 → 静音自动停止 → 麦克风 →
    // 音频增强 → 胶囊 → 胶囊样式/字幕 → 稳定模式 → 录音期间静音 → 提示音。
    card(
        ui,
        tr_l10n(lang, "settings.recording.title"),
        tr_l10n(lang, "settings.recording.desc"),
        |ui| {
            // 与快捷键页同一个录制器：键帽 + ↻ 重置 + ✕ 清空，点键帽直接录。
            shortcut_recorder_row(
                ui,
                vm,
                actions,
                &ShortcutRow {
                    field: ShortcutField::Dictation,
                    label: tr_l10n(lang, "settings.recording.hotkey_label"),
                    value: vm.dictation_hotkey.clone(),
                    can_disable: true,
                    hint: String::new(),
                },
                true,
            );
            let modes = [
                tr_l10n(lang, "settings.recording.mode_toggle"),
                tr_l10n(lang, "settings.recording.mode_hold"),
                tr_l10n(lang, "settings.recording.mode_auto"),
            ];
            segmented_row(
                ui,
                tr_l10n(lang, "settings.recording.mode_label"),
                tr_l10n(lang, "settings.recording.mode_desc"),
                &modes,
                vm.settings.recording_mode.min(2),
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::RecordingMode,
                        val,
                    ));
                },
            );
            // 「静音后自动停止」只在切换式模式下可用（Tauri 同样只在该模式渲染）。
            if vm.settings.recording_mode == 0 {
                toggle_row(
                    ui,
                    tr_l10n(lang, "settings.recording.silence_auto_stop_label"),
                    tr_l10n(lang, "settings.recording.silence_auto_stop_desc"),
                    vm.settings.silence_auto_stop,
                    || {
                        actions.push(FrontendAction::SettingsToggle(
                            SettingsField::SilenceAutoStop,
                        ));
                    },
                );
                if vm.settings.silence_auto_stop {
                    let seconds: Vec<String> = [1usize, 2, 3, 4, 5]
                        .iter()
                        .map(|value| {
                            fmt_l10n(
                                lang,
                                "settings.recording.silence_auto_stop_seconds_value",
                                &[value],
                            )
                        })
                        .collect();
                    let refs: Vec<&str> = seconds.iter().map(String::as_str).collect();
                    combo_index_row(
                        ui,
                        tr_l10n(lang, "settings.recording.silence_auto_stop_seconds_label"),
                        "",
                        vm.settings.silence_seconds.saturating_sub(1),
                        &refs,
                        |val| {
                            actions.push(FrontendAction::SettingsCombo(
                                SettingsComboField::SilenceSeconds,
                                val,
                            ));
                        },
                    );
                }
            }
            let mut microphones: Vec<String> =
                vec![tr_l10n(lang, "settings.recording.microphone_system_default").to_string()];
            microphones.extend(vm.settings.microphone_options.iter().cloned());
            let microphone_index = microphones
                .iter()
                .position(|name| name == &vm.settings.microphone_name)
                .unwrap_or(0);
            let microphone_refs: Vec<&str> = microphones.iter().map(String::as_str).collect();
            combo_index_row(
                ui,
                tr_l10n(lang, "settings.recording.microphone_label"),
                tr_l10n(lang, "settings.recording.microphone_desc"),
                microphone_index,
                &microphone_refs,
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::Microphone,
                        val,
                    ));
                },
            );
            // #3：设备列表读失败或一台设备都没有时必须说明（Tauri
            // `microphoneLoadError`），否则「只有系统默认」看起来像功能没做。
            if let Some(error) = vm.settings.microphone_error.clone() {
                hint_line(
                    ui,
                    &fmt_l10n(lang, "settings.recording.microphone_load_error", &[&error]),
                );
            } else if vm.settings.microphone_options.is_empty() {
                hint_line(ui, tr_l10n(lang, "onboarding.mic_no_device_hint"));
            }
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.audio_enhancement_label"),
                tr_l10n(lang, "settings.recording.audio_enhancement_desc"),
                vm.settings.audio_enhancement,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::AudioEnhancement,
                    ));
                },
            );
            if super::audio_dsp::settings(
                ui,
                &mut vm.settings.audio_dsp,
                lang,
                vm.settings.audio_enhancement,
            ) {
                actions.push(FrontendAction::SettingsAudioDsp(vm.settings.audio_dsp));
            }
            // AEC 需要「扬声器此刻在放什么」：让用户挑回环源（自动 / 不采集 / 具名
            // 监听设备）。没有回环源时这里只有前两项，AEC 会退化为无参考。
            if vm.settings.audio_dsp.aec_mode != openless_core::AecMode::Off {
                let mut options: Vec<&str> = vec![
                    tr_l10n(lang, "settings.audioDsp.aecReferenceAuto"),
                    tr_l10n(lang, "settings.audioDsp.aecReferenceOff"),
                ];
                options.extend(vm.settings.aec_reference_devices.iter().map(String::as_str));
                combo_index_row(
                    ui,
                    tr_l10n(lang, "settings.audioDsp.aecReferenceLabel"),
                    tr_l10n(lang, "settings.audioDsp.aecReferenceDesc"),
                    vm.settings.aec_reference_index.min(options.len().saturating_sub(1)),
                    &options,
                    |index| {
                        actions.push(FrontendAction::SettingsCombo(
                            SettingsComboField::AecReference,
                            index,
                        ));
                    },
                );
            }
            // #4：录音胶囊开关与样式（Tauri `capsuleLabel` / `capsuleStyleLabel`）。
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.capsule_label"),
                tr_l10n(lang, "settings.recording.capsule_desc"),
                vm.settings.show_capsule,
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::ShowCapsule));
                },
            );
            let capsule_styles = [
                tr_l10n(lang, "settings.recording.capsule_style_siri"),
                tr_l10n(lang, "settings.recording.capsule_style_classic"),
                tr_l10n(lang, "settings.recording.capsule_style_typeless"),
            ];
            combo_index_row(
                ui,
                tr_l10n(lang, "settings.recording.capsule_style_label"),
                "",
                vm.settings.capsule_style.min(2),
                &capsule_styles,
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::CapsuleStyle,
                        val,
                    ));
                },
            );
            capsule_style_preview(ui, vm.settings.capsule_style.min(2));
            // Tauri 这两行只受平台限制，不跟胶囊开关联动（关掉胶囊也仍可预设字幕
            // 与字号），这里保持一致。
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.capsuleTranscriptLabel"),
                tr_l10n(lang, "settings.recording.capsuleTranscriptDesc"),
                vm.settings.capsule_transcript,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::CapsuleTranscript,
                    ));
                },
            );
            if vm.settings.capsule_transcript {
                combo_index_row(
                    ui,
                    tr_l10n(lang, "settings.recording.capsuleTranscriptFontSize"),
                    "",
                    (vm.settings.capsule_transcript_font_size.clamp(12, 20) - 12) / 2,
                    &["12px", "14px", "16px", "18px", "20px"],
                    |index| {
                        actions.push(FrontendAction::SettingsCombo(
                            SettingsComboField::CapsuleTranscriptFontSize,
                            index,
                        ));
                    },
                );
            }
            // 上游 #1082（稳定模式：先录音后识别）：录音期间不连接 ASR，停止后提交整段
            // 音频；结果更晚，但录音不受建连延迟与网络抖动影响。
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.stable_transcription_label"),
                tr_l10n(lang, "settings.recording.stable_transcription_desc"),
                vm.settings.stable_transcription,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::StableTranscription,
                    ));
                },
            );
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.mute_during_recording_label"),
                tr_l10n(lang, "settings.recording.mute_during_recording_desc"),
                vm.settings.mute_while_recording,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::MuteWhileRecording,
                    ));
                },
            );
            // #5：提示音开关旁的「试听」按钮（Tauri `audioCuePreview`）。
            row_desc(
                ui,
                tr_l10n(lang, "settings.recording.audio_cue_label"),
                tr_l10n(lang, "settings.recording.audio_cue_desc"),
                |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(36.0, 20.0), egui::Sense::hover());
                    if layout::toggle(ui, rect, vm.settings.audio_cue, "audio-cue").clicked() {
                        actions.push(FrontendAction::SettingsToggle(SettingsField::AudioCue));
                    }
                    ui.add_space(10.0);
                    let preview = tr_l10n(lang, "settings.recording.audio_cue_preview");
                    let width = layout::text_width(ui, preview, 12.0) + 24.0;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
                    if layout::action_button(ui, rect, preview, None, layout::ButtonKind::Ghost)
                        .clicked()
                    {
                        actions.push(FrontendAction::SettingsAction(
                            SettingsActionField::PreviewAudioCue,
                        ));
                    }
                },
            );
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.echo_cancellation_label"),
                tr_l10n(lang, "settings.recording.echo_cancellation_desc"),
                vm.settings.echo_cancellation,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::EchoCancellation,
                    ));
                },
            );
        },
    );

    // 插入与剪贴板（Tauri：可折叠分组，含流式输入）
    card_group(
        ui,
        tr_l10n(lang, "settings.recording.insert_group_title"),
        |ui| {
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.restore_clipboard_label"),
                tr_l10n(lang, "settings.recording.restore_clipboard_desc"),
                vm.settings.restore_clipboard,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::RestoreClipboard,
                    ));
                },
            );
            toggle_row(
                ui,
                tr_l10n(lang, "settings.advanced.streaming_insert_label"),
                tr_l10n(lang, "settings.advanced.streaming_insert_desc"),
                vm.settings.streaming_insert,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::StreamingInsert,
                    ));
                },
            );
            toggle_row(
                ui,
                tr_l10n(
                    lang,
                    "settings.advanced.streaming_insert_save_clipboard_label",
                ),
                "",
                vm.settings.streaming_save_clipboard,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::StreamingSaveClipboard,
                    ));
                },
            );
        },
    );

    // 启动（Tauri `RecordingInputSection` 的 startup 折叠组，顺序一致：
    // 开机自启 → 启动时静默运行 → 自动检查更新）
    card_group(
        ui,
        tr_l10n(lang, "settings.recording.startup_group_title"),
        |ui| {
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.startup_at_boot"),
                tr_l10n(lang, "settings.recording.startup_at_boot_desc"),
                vm.settings.launch_at_login,
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::LaunchAtLogin));
                },
            );
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.start_minimized_label"),
                tr_l10n(lang, "settings.recording.start_minimized_desc"),
                vm.settings.start_minimized,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::StartMinimized,
                    ));
                },
            );
            // Tauri 也画了「自动检查更新」，但 Linux 版本没有任何更新器
            // （`supports_auto_update == false`，`auto_update_check` 存下来后没人读），
            // 画一个点了没反应的开关不算对齐。等真有更新通道再加回来。
        },
    );

    // 选区助手（Tauri `SelectionWorkspaceSection`，录音与输入标签页里
    // showShortcut=false：润色快捷键在快捷键页，这里只放行为设置）。
    //
    // Linux 上没有独立的 Voice Edit 面板（supports_voice_edit=false），所以不画
    // Tauri 的「语音编辑 + 打开面板」那一行；但语音编辑本身在 Linux 是活的
    // （core 的 selection_voice_service 按 selection_voice_enabled 决定口述改选区），
    // 所以开关、意图判定、编辑方案这些设置项都要给入口。
    card_group(
        ui,
        tr_l10n(lang, "settings.selection_workspace.title"),
        |ui| {
            hint_line(ui, tr_l10n(lang, "settings.selection_workspace.hint"));
            let recording_label = if vm.dictation_hotkey.trim().is_empty() {
                recording_mode_hint(lang, vm.settings.recording_mode)
            } else {
                vm.dictation_hotkey.clone()
            };
            toggle_row(
                ui,
                tr_l10n(lang, "settings.selection_workspace.voice_enable"),
                &fmt_l10n(
                    lang,
                    "settings.selection_workspace.voice_enable_desc",
                    &[&recording_label],
                ),
                vm.settings.selection_voice_enabled,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::SelectionVoiceEnabled,
                    ));
                },
            );
            let delivery = [
                tr_l10n(lang, "settings.selection_polish.direct_replace"),
                tr_l10n(lang, "settings.selection_polish.preview_confirm"),
            ];
            segmented_row(
                ui,
                tr_l10n(lang, "settings.selection_workspace.polish_delivery"),
                if vm.settings.selection_voice_enabled {
                    tr_l10n(lang, "settings.selection_workspace.voice_delivery_desc")
                } else {
                    ""
                },
                &delivery,
                vm.settings.selection_polish_delivery.min(1),
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::SelectionPolishDelivery,
                        val,
                    ));
                },
            );
            if vm.settings.selection_voice_enabled {
                toggle_row(
                    ui,
                    tr_l10n(lang, "settings.selection_workspace.auto_intent"),
                    tr_l10n(lang, "settings.selection_workspace.auto_intent_desc"),
                    vm.settings.selection_voice_auto_intent,
                    || {
                        actions.push(FrontendAction::SettingsToggle(
                            SettingsField::SelectionVoiceAutoIntent,
                        ));
                    },
                );
                // 关掉自动判断才用得上线索词（Tauri 同样只在此时渲染）。
                if !vm.settings.selection_voice_auto_intent {
                    text_area_row(
                        ui,
                        tr_l10n(lang, "settings.selection_workspace.edit_keywords"),
                        tr_l10n(lang, "settings.selection_workspace.edit_keywords_desc"),
                        &mut vm.settings.selection_voice_keywords,
                        "",
                        4,
                        |text| {
                            actions.push(FrontendAction::SettingsText(
                                SettingsTextField::SelectionVoiceKeywords,
                                text.to_owned(),
                            ));
                        },
                    );
                }
                let formats = [
                    tr_l10n(lang, "settings.selection_workspace.edit_plan_format_xml"),
                    tr_l10n(lang, "settings.selection_workspace.edit_plan_format_json"),
                ];
                segmented_row(
                    ui,
                    tr_l10n(lang, "settings.selection_workspace.edit_plan_format"),
                    tr_l10n(lang, "settings.selection_workspace.edit_plan_format_desc"),
                    &formats,
                    vm.settings.selection_voice_plan_format.min(1),
                    |val| {
                        actions.push(FrontendAction::SettingsCombo(
                            SettingsComboField::SelectionVoicePlanFormat,
                            val,
                        ));
                    },
                );
                text_area_row(
                    ui,
                    tr_l10n(lang, "settings.selection_workspace.edit_system_prompt"),
                    tr_l10n(lang, "settings.selection_workspace.edit_system_prompt_desc"),
                    &mut vm.settings.selection_voice_system_prompt,
                    tr_l10n(
                        lang,
                        "settings.selection_workspace.edit_system_prompt_placeholder",
                    ),
                    6,
                    |text| {
                        actions.push(FrontendAction::SettingsText(
                            SettingsTextField::SelectionVoiceSystemPrompt,
                            text.to_owned(),
                        ));
                    },
                );
                let reset = tr_l10n(
                    lang,
                    "settings.selection_workspace.edit_system_prompt_reset",
                );
                ui.horizontal(|ui| {
                    ui.add_space(0.0);
                    let width = layout::text_width(ui, reset, 12.0) + 20.0;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
                    if layout::action_button(ui, rect, reset, None, layout::ButtonKind::Ghost)
                        .clicked()
                    {
                        actions.push(FrontendAction::SettingsAction(
                            SettingsActionField::ResetSelectionVoicePrompt,
                        ));
                    }
                });
            }
        },
    );

    // 远程输入（Tauri RemoteInputSection）
    card_group(ui, tr_l10n(lang, "settings.remote_input.title"), |ui| {
        hint_line(ui, tr_l10n(lang, "settings.remote_input.security_hint"));
        toggle_row(
            ui,
            tr_l10n(lang, "settings.remote_input.enable_label"),
            tr_l10n(lang, "settings.remote_input.enable_desc"),
            vm.settings.remote_input,
            || {
                actions.push(FrontendAction::SettingsToggle(SettingsField::RemoteInput));
            },
        );
        text_edit_row(
            ui,
            tr_l10n(lang, "settings.remote_input.port_label"),
            "",
            &mut vm.settings.remote_port,
            "8765",
            |text| {
                actions.push(FrontendAction::SettingsText(
                    SettingsTextField::RemotePort,
                    text.to_owned(),
                ));
            },
        );
        remote_mode_row(ui, lang, vm.settings.remote_default_mode, |val| {
            actions.push(FrontendAction::SettingsCombo(
                SettingsComboField::RemoteDefaultMode,
                val,
            ));
        });
        // 连接细节（配对码 / 网址 / 证书指纹）只在服务真的在监听、且地址未过期时
        // 展示：过期地址可能指向别的主机，展示它等于诱导用户在错误地址上配对。
        let cert_state = remote_cert_fingerprint_state(
            vm.remote_running,
            vm.remote_urls_stale,
            vm.remote_cert_fingerprint.as_deref(),
        );
        if vm.remote_running && !vm.remote_urls_stale {
            if !vm.remote_pin.is_empty() {
                text_row(
                    ui,
                    tr_l10n(lang, "settings.remote_input.pin_label"),
                    "",
                    &vm.remote_pin,
                );
            }
            if !vm.remote_urls.is_empty() {
                text_row(
                    ui,
                    tr_l10n(lang, "settings.remote_input.url_label"),
                    tr_l10n(lang, "settings.remote_input.security_hint"),
                    &vm.remote_urls.join(" · "),
                );
            }
            if matches!(cert_state, RemoteCertFingerprintState::Available) {
                action_row(
                    ui,
                    tr_l10n(lang, "settings.remote_input.cert_fingerprint_label"),
                    tr_l10n(lang, "settings.remote_input.cert_verify_hint"),
                    tr_l10n(lang, "settings.remote_input.cert_fingerprint_copy"),
                    SettingsActionField::CopyCertFingerprint,
                    actions,
                );
                ui.label(
                    egui::RichText::new(vm.remote_cert_fingerprint.clone().unwrap_or_default())
                        .size(10.5)
                        .color(theme::INK_4),
                );
            } else {
                // 拿不到可核验的完整指纹时必须显式告警：静默省略会让人以为
                // 「不用核对证书也能连」。
                ui.colored_label(
                    theme::WARN,
                    tr_l10n(lang, "settings.remote_input.cert_fingerprint_unavailable"),
                );
            }
        }
    });
}

/// 证书指纹的展示决定（对齐 Tauri `RemoteInputSection`）：服务未监听或地址已过期
/// 时整块连接细节都不展示；服务在监听但没有可核验的完整指纹时必须显式告警。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RemoteCertFingerprintState {
    /// 没有可展示的连接细节。
    Hidden,
    /// 在监听但拿不到完整指纹 → 必须显式告警。
    Unavailable,
    /// 有完整指纹 → 展示并可复制。
    Available,
}

fn remote_cert_fingerprint_state(
    running: bool,
    urls_stale: bool,
    fingerprint: Option<&str>,
) -> RemoteCertFingerprintState {
    if !running || urls_stale {
        return RemoteCertFingerprintState::Hidden;
    }
    match fingerprint {
        Some(value) if is_complete_sha256(value) => RemoteCertFingerprintState::Available,
        _ => RemoteCertFingerprintState::Unavailable,
    }
}

/// 完整 SHA-256 指纹 = 64 个十六进制字符；截断/非十六进制的值不能当作可核验指纹。
fn is_complete_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn shortcuts(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    // Tauri `ShortcutsSection`：搜索 + 计数 / 分类与分配状态筛选 / 命令行。
    //
    // 搜索词与两个筛选是**纯 UI 状态**，存在 egui 自己的记忆里：以前它们走
    // `FrontendAction` 回宿主再整份快照推回来，连点筛选会明显卡顿。
    #[derive(Clone, Copy, Default)]
    struct Filters {
        category: u8,
        status: u8,
    }
    let filters_id = egui::Id::unique("openless-shortcuts-filters");
    let query_id = egui::Id::unique("openless-shortcuts-query");
    let mut filters = ui
        .data(|data| data.get_temp::<Filters>(filters_id))
        .unwrap_or_default();
    let mut query = ui
        .data(|data| data.get_temp::<String>(query_id))
        .unwrap_or_default();

    let dictation_hint = recording_mode_hint(lang, vm.settings.recording_mode);
    let rows: Vec<ShortcutRow> = vec![
        ShortcutRow {
            field: ShortcutField::Dictation,
            label: tr_l10n(lang, "settings.shortcuts.start_stop"),
            value: vm.dictation_hotkey.clone(),
            can_disable: true,
            hint: dictation_hint,
        },
        ShortcutRow {
            field: ShortcutField::Translation,
            label: tr_l10n(lang, "hotkey.translation"),
            value: vm.translation_hotkey.clone(),
            can_disable: true,
            hint: String::new(),
        },
        ShortcutRow {
            field: ShortcutField::Qa,
            label: tr_l10n(lang, "selection_ask.hotkey_title"),
            value: vm.qa_hotkey.clone(),
            can_disable: true,
            hint: String::new(),
        },
        ShortcutRow {
            field: ShortcutField::QuickNote,
            label: tr_l10n(lang, "quickNote.shortcutTitle"),
            value: vm.quick_note_hotkey.clone(),
            can_disable: true,
            hint: tr_l10n(lang, "quickNote.shortcutDesc").to_string(),
        },
        ShortcutRow {
            field: ShortcutField::SelectionPolish,
            label: tr_l10n(lang, "settings.selection_workspace.polish_hotkey"),
            value: vm.selection_polish_hotkey.clone(),
            can_disable: true,
            hint: tr_l10n(lang, "settings.selection_workspace.polish_hotkey_desc").to_string(),
        },
        ShortcutRow {
            field: ShortcutField::SwitchStyle,
            label: tr_l10n(lang, "settings.shortcuts.switch_style"),
            value: vm.switch_style_hotkey.clone(),
            can_disable: true,
            hint: String::new(),
        },
        ShortcutRow {
            field: ShortcutField::OpenApp,
            label: tr_l10n(lang, "settings.shortcuts.open_app"),
            value: vm.open_app_hotkey.clone(),
            can_disable: true,
            hint: String::new(),
        },
    ];

    let trimmed = query.trim().to_lowercase();
    let group_of = |field: ShortcutField| match field {
        ShortcutField::Dictation
        | ShortcutField::Translation
        | ShortcutField::Qa
        | ShortcutField::QuickNote
        | ShortcutField::SelectionPolish => 0,
        ShortcutField::SwitchStyle => 1,
        ShortcutField::OpenApp => 2,
        ShortcutField::StylePack(_) | ShortcutField::StyleDraft => 1,
    };
    let matches = |label: &str, value: &str| -> bool {
        let assigned = !value.trim().is_empty();
        let text = if assigned {
            format!("{label} {value}").to_lowercase()
        } else {
            format!("{label} {}", tr_l10n(lang, "uiRefinement.unassigned")).to_lowercase()
        };
        (trimmed.is_empty() || text.contains(&trimmed))
            && (filters.status == 0 || (filters.status == 1) == assigned)
    };

    let command_total = rows.len();
    let packs = vm.settings.style_pack_hotkeys.clone();
    let visible: Vec<ShortcutRow> = rows
        .into_iter()
        .filter(|row| filters.category == 0 || filters.category == group_of(row.field) + 1)
        .filter(|row| matches(row.label, &row.value))
        .collect();
    let visible_packs: Vec<(usize, StylePackHotkeyRow)> = packs
        .iter()
        .enumerate()
        .filter(|(_, _row)| filters.category == 0 || filters.category == 2)
        .filter(|(_, row)| matches(&row.name, &row.hotkey))
        .map(|(index, row)| (index, row.clone()))
        .collect();
    let readonly_visible = (filters.category == 0 || filters.category == 4)
        && filters.status != 2
        && (trimmed.is_empty()
            || format!("{} Esc", tr_l10n(lang, "settings.shortcuts.cancel"))
                .to_lowercase()
                .contains(&trimmed));
    let total = command_total + packs.len() + 1;
    let count = visible.len() + visible_packs.len() + usize::from(readonly_visible);

    // ── 工具栏 ────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if search_field(ui, &mut query, query_id).changed() {
            ui.data_mut(|data| data.insert_temp(query_id, query.clone()));
        }
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(format!("{count} / {total}"))
                .size(12.0)
                .color(theme::INK_3),
        );
    });
    ui.add_space(16.0);

    ui.horizontal_top(|ui| {
        // ── 左：筛选（140px，不参与滚动） ───────────────────────────────
        ui.allocate_ui_with_layout(
            egui::vec2(FILTER_COLUMN_WIDTH, 10.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(FILTER_COLUMN_WIDTH);
                let mut next = filters;
                filter_group(
                    ui,
                    tr_l10n(lang, "uiRefinement.category"),
                    &[
                        (tr_l10n(lang, "uiRefinement.all"), 0),
                        (tr_l10n(lang, "uiRefinement.recording"), 1),
                        (tr_l10n(lang, "uiRefinement.styles"), 2),
                        (tr_l10n(lang, "uiRefinement.system"), 3),
                    ],
                    filters.category,
                    |value| next.category = value,
                );
                ui.add_space(20.0);
                filter_group(
                    ui,
                    tr_l10n(lang, "uiRefinement.status"),
                    &[
                        (tr_l10n(lang, "uiRefinement.all"), 0),
                        (tr_l10n(lang, "uiRefinement.assigned"), 1),
                        (tr_l10n(lang, "uiRefinement.unassigned"), 2),
                    ],
                    filters.status,
                    |value| next.status = value,
                );
                if next.category != filters.category || next.status != filters.status {
                    filters = next;
                    ui.data_mut(|data| data.insert_temp(filters_id, filters));
                }
            },
        );
        ui.add_space(28.0);
        // ── 右：命令行 ──────────────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(tr_l10n(lang, "uiRefinement.shortcutHint"))
                    .size(12.0)
                    .color(theme::INK_3),
            );
            ui.add_space(8.0);
            for row in &visible {
                shortcut_recorder_row(ui, vm, actions, row, true);
            }
            for (index, row) in &visible_packs {
                style_pack_recorder_row(ui, vm, actions, *index, row);
            }
            if readonly_visible {
                readonly_keycap_row(ui, tr_l10n(lang, "settings.shortcuts.cancel"), "", "Esc");
            }
            if count == 0 {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "uiRefinement.noResults"))
                        .size(13.0)
                        .color(theme::INK_3),
                );
            }
            if (filters.category == 0 || filters.category == 2) && trimmed.is_empty() {
                ui.add_space(10.0);
                style_pack_add_row(ui, vm, actions);
            }
        });
    });
}

const FILTER_COLUMN_WIDTH: f32 = 140.0;
/// 录制器区域宽度上限（Tauri `.ol-shortcut-row` 的第二列 `minmax(150px, 260px)`）。
const RECORDER_MAX_WIDTH: f32 = 260.0;
/// 命令行最小高度（Tauri `.ol-shortcut-row` 的 `min-height: 64px`）。
const SHORTCUT_ROW_HEIGHT: f32 = 64.0;

/// 设置页的搜索框（Tauri `.ol-settings-search-field`）：36 高、1px 描边、r8、白底。
fn search_field(ui: &mut egui::Ui, value: &mut String, id: egui::Id) -> egui::Response {
    let width = ui.available_width().min(520.0);
    let (outer, _) = ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(outer, egui::CornerRadius::same(8), theme::SURFACE);
    ui.painter().rect_stroke(
        outer,
        egui::CornerRadius::same(8),
        egui::Stroke::new(1.0, theme::LINE_STRONG),
        egui::StrokeKind::Inside,
    );
    let inner = outer.shrink2(egui::vec2(11.0, 1.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add(
        egui::TextEdit::singleline(value)
            .id(id)
            .hint_text(tr_l10n(
                openless_linux_egui::Lang::ZhCn,
                "uiRefinement.shortcutsSearch",
            ))
            .font(egui::FontId::proportional(13.0))
            .text_color(theme::INK)
            .frame(egui::Frame::NONE)
            .desired_width(inner.width())
            .align(egui::Align2::CENTER_CENTER),
    )
}

/// 一组筛选按钮（标题 + 竖排选项），选中项底色与字重都跟着变。
fn filter_group(
    ui: &mut egui::Ui,
    title: &str,
    options: &[(&str, u8)],
    active: u8,
    mut on_select: impl FnMut(u8),
) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(title)
            .size(11.0)
            .color(theme::INK_3)
            .strong(),
    );
    ui.add_space(4.0);
    for (label, value) in options {
        let selected = active == *value;
        // id 由「分组标题 + 选项」经 `push_id` 派生：
        //  * 不能跟着 `ui.scope_id()` 走 —— 切换筛选会改变可见行数、连带改变 auto-id；
        //  * 也不能只用选项文字 —— 分类与分配状态两组各有一个「全部」，
        //    只按文字区分会让两个按钮抢同一个 id（日志里 `Second use of widget ID`）。
        // 交互用 `allocate_exact_size` 自己返回的 response：对同一矩形再 `interact`
        // 会出现两个控件抢同一次点击，点击就丢了。
        let (rect, response) = ui
            .push_id(("openless-shortcut-filter", title, *label), |ui| {
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click())
            })
            .inner;
        if selected {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(7), theme::SURFACE_2);
        } else if response.hovered() {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(7),
                theme::SURFACE_2.gamma_multiply(0.55),
            );
        }
        let text = if selected {
            egui::RichText::new(*label)
                .size(13.0)
                .strong()
                .color(theme::INK)
        } else {
            egui::RichText::new(*label).size(13.0).color(theme::INK_2)
        };
        ui.painter().text(
            egui::pos2(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text.text(),
            egui::FontId::proportional(13.0),
            if selected { theme::INK } else { theme::INK_2 },
        );
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            // 按「分组 + 选项」记录：两组各有一个「全部」，只按文字存会互相覆盖。
            let mut rects: std::collections::BTreeMap<(String, String), egui::Rect> = data
                .get_temp(egui::Id::unique("shortcut-filter-rects"))
                .unwrap_or_default();
            rects.insert((title.to_string(), (*label).to_string()), rect);
            data.insert_temp(egui::Id::unique("shortcut-filter-rects"), rects);
        });
        if response.clicked() {
            on_select(*value);
        }
    }
}

/// 一条命令行：左侧标签（+说明），右侧录制器（键帽 + ↻ + ✕），底部一条细线。
///
/// 布局用**显式矩形**而不是 `allocate_ui_with_layout`：后者按内容收缩
/// （`top_down` 会 shrink-wrap），标签列只占文字宽度，录制器就贴不到右边缘
/// （表现是"右对齐不对"）。这里把行切成「标签区 + 20px 间距 + 录制器区」。
fn shortcut_row_layout(
    ui: &mut egui::Ui,
    row_label: &str,
    hint: &str,
    body: impl FnOnce(&mut egui::Ui, egui::Rect),
) {
    let width = ui.available_width();
    // 没有标签时（速记页的卡片只有键帽）用紧凑行高：64px 会把那张卡片撑出可视区。
    let height = if row_label.is_empty() {
        44.0
    } else {
        SHORTCUT_ROW_HEIGHT
    };
    let recorder_width = RECORDER_MAX_WIDTH.min((width * 0.5).max(150.0));
    let (row_rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let recorder_rect = egui::Rect::from_min_size(
        egui::pos2(row_rect.right() - recorder_width, row_rect.top()),
        egui::vec2(recorder_width, height),
    );
    let label_rect = egui::Rect::from_min_max(
        row_rect.min,
        egui::pos2(
            (recorder_rect.left() - 20.0).max(row_rect.left()),
            row_rect.bottom(),
        ),
    );
    let mut label_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(label_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    label_ui.add_space(if row_label.is_empty() { 0.0 } else { 12.0 });
    label_ui.label(egui::RichText::new(row_label).size(13.5).color(theme::INK));
    if !hint.is_empty() {
        label_ui.add_space(5.0);
        label_ui.label(egui::RichText::new(hint).size(11.5).color(theme::INK_3));
    }
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::unique(("shortcut-row-rects", row_label.to_string())),
            (row_rect, recorder_rect),
        )
    });
    let mut recorder_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(recorder_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    body(&mut recorder_ui, recorder_rect);
    row_divider(ui);
}

pub(super) fn shortcut_recorder_row(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    row: &ShortcutRow,
    with_reset: bool,
) {
    let recording = vm.shortcut_recording == Some(row.field);
    shortcut_row_layout(ui, row.label, &row.hint, |ui, _rect| {
        if recording {
            recording_panel(ui, vm, actions, row.field);
            return;
        }
        // 从右往左：✕ 清空 → ↻ 重置 → 键帽组。
        if row.can_disable
            && bordered_icon_button(ui, IconName::Close, ("shortcut-clear", row.field))
        {
            actions.push(FrontendAction::ShortcutDisable(row.field));
        }
        ui.add_space(8.0);
        if with_reset && bordered_icon_button(ui, IconName::Refresh, ("shortcut-reset", row.field))
        {
            actions.push(FrontendAction::ShortcutReset(row.field));
        }
        ui.add_space(8.0);
        let assigned = !row.value.trim().is_empty();
        let width = chips_width(ui, &row.value).min(ui.available_width());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click());
        let response = ui.interact(
            rect,
            egui::Id::unique(("openless-shortcut-record", format!("{:?}", row.field))),
            egui::Sense::click(),
        );
        if assigned {
            keycaps_in_rect(ui, rect, &row.value);
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "＋",
                egui::FontId::proportional(13.0),
                theme::INK_3,
            );
        }
        if response.clicked() {
            actions.push(FrontendAction::ShortcutRecording(Some(row.field)));
        }
    });
}

fn row_divider(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 0.5), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme::LINE_SOFT);
}

fn style_pack_recorder_row(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    index: usize,
    row: &StylePackHotkeyRow,
) {
    let recording = vm.shortcut_recording == Some(ShortcutField::StylePack(index));
    // 左侧是风格包选择器（不是纯文本），所以这一行用「选择器 + 录制器」两列，
    // 录制器同样贴右边缘。
    let width = ui.available_width();
    let recorder_width = RECORDER_MAX_WIDTH.min((width * 0.5).max(150.0));
    let (row_rect, _) =
        ui.allocate_exact_size(egui::vec2(width, SHORTCUT_ROW_HEIGHT), egui::Sense::hover());
    let recorder_rect = egui::Rect::from_min_size(
        egui::pos2(row_rect.right() - recorder_width, row_rect.top()),
        egui::vec2(recorder_width, SHORTCUT_ROW_HEIGHT),
    );
    let label_rect = egui::Rect::from_min_max(
        row_rect.min,
        egui::pos2(
            (recorder_rect.left() - 20.0).max(row_rect.left()),
            row_rect.bottom(),
        ),
    );
    let mut picker_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(label_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    style_pack_picker(
        &mut picker_ui,
        &vm.style_packs,
        &row.pack_id,
        &row.name,
        Some(index),
        actions,
        vm.lang,
    );
    let mut recorder_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(recorder_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if recording {
        recording_panel(
            &mut recorder_ui,
            vm,
            actions,
            ShortcutField::StylePack(index),
        );
    } else {
        if bordered_icon_button(
            &mut recorder_ui,
            IconName::Close,
            ("style-hotkey-remove", index),
        ) {
            actions.push(FrontendAction::StyleHotkeyRemove(index));
        }
        recorder_ui.add_space(8.0);
        let chips = chips_width(&recorder_ui, &row.hotkey).min(recorder_ui.available_width());
        let (rect, _) =
            recorder_ui.allocate_exact_size(egui::vec2(chips, 26.0), egui::Sense::click());
        let response = recorder_ui.interact(
            rect,
            egui::Id::unique(("openless-style-shortcut-record", index)),
            egui::Sense::click(),
        );
        if !row.hotkey.trim().is_empty() {
            keycaps_in_rect(&mut recorder_ui, rect, &row.hotkey);
        } else {
            recorder_ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "＋",
                egui::FontId::proportional(13.0),
                theme::INK_3,
            );
        }
        if response.clicked() {
            actions.push(FrontendAction::ShortcutRecording(Some(
                ShortcutField::StylePack(index),
            )));
        }
    }
    row_divider(ui);
}

fn style_pack_add_row(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    if !vm.style_hotkey_draft_open {
        let label = format!("+ {}", tr_l10n(lang, "settings.shortcuts.style_pack_add"));
        if ui
            .add(
                egui::Label::new(egui::RichText::new(label).size(13.0).color(theme::BLUE))
                    .sense(egui::Sense::click()),
            )
            .clicked()
        {
            actions.push(FrontendAction::StyleHotkeyDraft(true));
        }
        return;
    }
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), SHORTCUT_ROW_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let pack = vm.style_packs.get(vm.style_hotkey_draft_pack);
            style_pack_picker(
                ui,
                &vm.style_packs,
                pack.map(|pack| pack.id.as_str()).unwrap_or(""),
                pack.map(|pack| pack.name.as_str()).unwrap_or(""),
                None,
                actions,
                lang,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if bordered_icon_button(ui, IconName::Close, ("style-draft-cancel", 0)) {
                    actions.push(FrontendAction::StyleHotkeyDraft(false));
                }
                ui.add_space(8.0);
                recording_panel(ui, vm, actions, ShortcutField::StyleDraft);
            });
        },
    );
    row_divider(ui);
}

/// 键帽组总宽度。未分配时给一个「＋」的点击区。
fn chips_width(ui: &egui::Ui, combo: &str) -> f32 {
    let mut width = 0.0;
    for part in combo
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        width += (layout::text_width(ui, part, 11.5) + 12.0).max(20.0) + 4.0;
    }
    if width <= 0.0 {
        24.0
    } else {
        width - 4.0
    }
}

/// 在给定矩形内画键帽：左→右、修饰键在前（`Alt` `Z`）。尺寸照 Tauri 的 `Kbd`：
/// 高 21、圆角 5、11.5px、底色 `SURFACE_2`、0.5px 描边 + 一条底边阴影线。
fn keycaps_in_rect(ui: &mut egui::Ui, rect: egui::Rect, combo: &str) {
    let mut x = rect.left();
    for part in combo
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let width = (layout::text_width(ui, part, 11.5) + 12.0).max(20.0);
        let key = egui::Rect::from_min_size(
            egui::pos2(x, rect.center().y - 10.5),
            egui::vec2(width, 21.0),
        );
        ui.painter()
            .rect_filled(key, egui::CornerRadius::same(5), theme::SURFACE_2);
        ui.painter().rect_stroke(
            key,
            egui::CornerRadius::same(5),
            egui::Stroke::new(0.5, theme::LINE_STRONG),
            egui::StrokeKind::Inside,
        );
        // 键帽底边那一道更深的线（Tauri `Kbd` 的 `box-shadow: inset 0 -1px`）。
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(key.left() + 1.0, key.bottom() - 1.0),
                egui::pos2(key.right() - 1.0, key.bottom()),
            ),
            egui::CornerRadius::same(3),
            theme::LINE,
        );
        ui.painter().text(
            key.center(),
            egui::Align2::CENTER_CENTER,
            part,
            egui::FontId::proportional(11.5),
            theme::INK_2,
        );
        x += width + 4.0;
    }
}

/// 带边框的小图标按钮（Tauri `menuButtonStyle`：0.5px 描边 / r6 / 透明底）。
fn bordered_icon_button(
    ui: &mut egui::Ui,
    icon: IconName,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 26.0), egui::Sense::click());
    let response = ui.interact(
        rect,
        egui::Id::unique(("openless-icon-button", format!("{salt:?}"))),
        egui::Sense::click(),
    );
    let fill = if response.hovered() {
        theme::SURFACE_2
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(6),
        egui::Stroke::new(0.5, theme::LINE_STRONG),
        egui::StrokeKind::Inside,
    );
    let color = if response.hovered() {
        theme::INK
    } else {
        theme::INK_2
    };
    crate::ui::frontend::icons::draw_icon_sized(ui, rect.center(), icon, color, 13.0);
    response.clicked()
}

pub(super) struct ShortcutRow {
    field: ShortcutField,
    label: &'static str,
    /// 已格式化的键帽文本（`Ctrl+Shift+;`），空串 = 未设置。
    value: String,
    /// 已配置的快捷键均可清空；尚未保存的草稿不提供停用操作。
    can_disable: bool,
    /// 行下方的补充说明（录音行显示当前录音方式后缀）。
    hint: String,
}

impl ShortcutRow {
    /// 供设置页之外的页面（速记页的快捷键卡片）构造同一套控件。
    pub(super) fn new(
        field: ShortcutField,
        label: &'static str,
        value: String,
        can_disable: bool,
        hint: String,
    ) -> Self {
        Self {
            field,
            label,
            value,
            can_disable,
            hint,
        }
    }
}

/// 「请按下快捷键组合…」面板：读本帧输入，Escape 取消、其它键即完成录入。
fn recording_panel(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    field: ShortcutField,
) {
    let lang = vm.lang;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(240.0, 44.0), egui::Sense::hover());
    // 录制面板的矩形交给主循环：点别处就取消录制（Tauri 的 Popover 点外部关闭）。
    // 记下"是否已经武装"——进入录制的那一帧是按下鼠标的那一帧，不能立刻取消。
    let cancel_id = egui::Id::unique("openless-shortcut-recording");
    let armed = ui
        .data(|data| data.get_temp::<(egui::Rect, bool)>(cancel_id))
        .is_some_and(|(_, armed)| armed);
    ui.data_mut(|data| data.insert_temp(cancel_id, (rect, armed)));
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), theme::BLUE_SOFT);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(37, 99, 235, 60)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y - 7.0),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "settings.recording.combo_record_hint"),
        egui::FontId::proportional(12.0),
        theme::BLUE,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y + 9.0),
        egui::Align2::LEFT_CENTER,
        format!("Esc · {}", tr_l10n(lang, "common.cancel")),
        egui::FontId::proportional(10.5),
        theme::INK_4,
    );
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        actions.push(FrontendAction::ShortcutRecording(None));
        return;
    }
    if let Some((primary, modifiers)) =
        captured_binding(ui.ctx(), &mut vm.shortcut_pending_modifier)
    {
        actions.push(FrontendAction::ShortcutCaptured(field, primary, modifiers));
    }
}

/// 读本帧按下的第一个「真键」+ 当时按住的修饰键，转成 Core 的
/// `ShortcutBinding` 形式（primary + modifiers）。
///
/// 修饰键自身在 egui 里没有 Key 事件（`Key` 枚举只有 `Colon`/`Semicolon` 这类
/// 具体键，修饰键只在 `Modifiers` 里），所以「按住某个修饰键当热键」只能跨帧判断：
/// 按住期间没有按下任何真键 → 松开时记为修饰键触发。`pending` 就是这份挂起状态。
/// egui 分不清左右修饰键，因此统一记左侧名（Core 的 legacy trigger 表接受
/// LeftControl/LeftShift/LeftAlt/LeftSuper）。
///
/// 窗口进程的本地热键匹配（`crate::local_hotkeys`）复用同一个函数：那里没有
/// `Ui`（帧由别处的面板渲染），所以这里取 `Context`。
pub(crate) fn captured_binding(
    ctx: &egui::Context,
    pending: &mut Option<String>,
) -> Option<(String, Vec<String>)> {
    // 用按键事件自带的修饰键（RawInput.modifiers 在某些输入法/后端下会滞后），
    // 并跳过 egui 合成的剪贴板命令与 Escape（后者由调用方当取消处理）。
    if let Some((key, modifiers)) = ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if !matches!(
                key,
                egui::Key::Escape | egui::Key::Copy | egui::Key::Cut | egui::Key::Paste
            ) =>
            {
                Some((*key, *modifiers))
            }
            _ => None,
        })
    }) {
        // 按下真键 = 组合键，之前挂起的修饰键作废。
        *pending = None;
        let primary = shortcut_primary(key)?;
        return Some((primary, modifier_tags(modifiers)));
    }

    let modifiers = ctx.input(|input| input.modifiers);
    match bare_modifier_name(modifiers) {
        Some(name) => {
            if pending.is_none() {
                *pending = Some(name.to_string());
            }
            None
        }
        None if modifiers.any() => {
            // 多个修饰键同按：不当作修饰键热键（松手也不触发）。
            *pending = None;
            None
        }
        None => pending.take().map(|name| (name, Vec::new())),
    }
}

pub(crate) fn modifier_tags(modifiers: egui::Modifiers) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    if modifiers.ctrl || modifiers.command {
        tags.push("ctrl".to_string());
    }
    if modifiers.alt {
        tags.push("alt".to_string());
    }
    if modifiers.shift {
        tags.push("shift".to_string());
    }
    if modifiers.mac_cmd {
        tags.push("super".to_string());
    }
    tags
}

/// 恰好按住「一个类别」的修饰键时返回它的 Core 主键名，否则 `None`。
/// 顺序 ctrl → alt → shift → super：egui 在 Linux 上把 Ctrl 同时标成
/// `command`，所以先判 ctrl。
pub(crate) fn bare_modifier_name(modifiers: egui::Modifiers) -> Option<&'static str> {
    let categories = [
        modifiers.ctrl || modifiers.command,
        modifiers.alt,
        modifiers.shift,
        modifiers.mac_cmd,
    ];
    if categories.iter().filter(|held| **held).count() != 1 {
        return None;
    }
    if categories[0] {
        Some("LeftControl")
    } else if categories[1] {
        Some("LeftAlt")
    } else if categories[2] {
        Some("LeftShift")
    } else {
        Some("LeftSuper")
    }
}

/// egui 的物理键 → Core 认可的主键名（见 `shortcut_types::validate_primary`）。
fn shortcut_primary(key: egui::Key) -> Option<String> {
    use egui::Key;
    let name = match key {
        Key::Num0 => "0".to_string(),
        Key::Num1 => "1".to_string(),
        Key::Num2 => "2".to_string(),
        Key::Num3 => "3".to_string(),
        Key::Num4 => "4".to_string(),
        Key::Num5 => "5".to_string(),
        Key::Num6 => "6".to_string(),
        Key::Num7 => "7".to_string(),
        Key::Num8 => "8".to_string(),
        Key::Num9 => "9".to_string(),
        Key::A => "A".to_string(),
        Key::B => "B".to_string(),
        Key::C => "C".to_string(),
        Key::D => "D".to_string(),
        Key::E => "E".to_string(),
        Key::F => "F".to_string(),
        Key::G => "G".to_string(),
        Key::H => "H".to_string(),
        Key::I => "I".to_string(),
        Key::J => "J".to_string(),
        Key::K => "K".to_string(),
        Key::L => "L".to_string(),
        Key::M => "M".to_string(),
        Key::N => "N".to_string(),
        Key::O => "O".to_string(),
        Key::P => "P".to_string(),
        Key::Q => "Q".to_string(),
        Key::R => "R".to_string(),
        Key::S => "S".to_string(),
        Key::T => "T".to_string(),
        Key::U => "U".to_string(),
        Key::V => "V".to_string(),
        Key::W => "W".to_string(),
        Key::X => "X".to_string(),
        Key::Y => "Y".to_string(),
        Key::Z => "Z".to_string(),
        Key::F1 => "F1".to_string(),
        Key::F2 => "F2".to_string(),
        Key::F3 => "F3".to_string(),
        Key::F4 => "F4".to_string(),
        Key::F5 => "F5".to_string(),
        Key::F6 => "F6".to_string(),
        Key::F7 => "F7".to_string(),
        Key::F8 => "F8".to_string(),
        Key::F9 => "F9".to_string(),
        Key::F10 => "F10".to_string(),
        Key::F11 => "F11".to_string(),
        Key::F12 => "F12".to_string(),
        Key::F13 => "F13".to_string(),
        Key::F14 => "F14".to_string(),
        Key::F15 => "F15".to_string(),
        Key::F16 => "F16".to_string(),
        Key::F17 => "F17".to_string(),
        Key::F18 => "F18".to_string(),
        Key::F19 => "F19".to_string(),
        Key::F20 => "F20".to_string(),
        Key::ArrowUp => "ArrowUp".to_string(),
        Key::ArrowDown => "ArrowDown".to_string(),
        Key::ArrowLeft => "ArrowLeft".to_string(),
        Key::ArrowRight => "ArrowRight".to_string(),
        Key::Space => "Space".to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::Backspace => "Backspace".to_string(),
        Key::Delete => "Delete".to_string(),
        Key::Home => "Home".to_string(),
        Key::End => "End".to_string(),
        Key::PageUp => "PageUp".to_string(),
        Key::PageDown => "PageDown".to_string(),
        Key::Semicolon => ";".to_string(),
        Key::Comma => ",".to_string(),
        Key::Period => ".".to_string(),
        Key::Slash => "/".to_string(),
        Key::Backslash => "\\".to_string(),
        Key::Minus => "-".to_string(),
        Key::Equals => "=".to_string(),
        Key::Plus => "+".to_string(),
        Key::Quote => "'".to_string(),
        Key::Backtick => "`".to_string(),
        Key::OpenBracket => "[".to_string(),
        Key::CloseBracket => "]".to_string(),
        Key::Colon => ":".to_string(),
        Key::Pipe => "|".to_string(),
        Key::Questionmark => "?".to_string(),
        Key::Exclamationmark => "!".to_string(),
        _ => return None,
    };
    Some(name)
}

/// 只读键帽行（Tauri 的 `readonlyRows`：取消本次录音 = Esc）。
fn readonly_keycap_row(ui: &mut egui::Ui, label: &str, desc: &str, combo: &str) {
    shortcut_row_layout(ui, label, desc, |ui, _rect| {
        let width = chips_width(ui, combo).min(ui.available_width());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
        keycaps_in_rect(ui, rect, combo);
    });
}

fn style_pack_pick_action(row: Option<usize>, next: usize) -> FrontendAction {
    match row {
        Some(index) => FrontendAction::StyleHotkeyRepack(index, next),
        None => FrontendAction::StyleHotkeyDraftPack(next),
    }
}

/// 风格包选择器（Tauri 的 `SelectLite`）：显示名 +「（已停用）」后缀，整表替换。
#[allow(clippy::too_many_arguments)]
fn style_pack_picker(
    ui: &mut egui::Ui,
    packs: &[StylePack],
    current_pack_id: &str,
    fallback_name: &str,
    row: Option<usize>,
    actions: &mut Vec<FrontendAction>,
    lang: Lang,
) {
    let options: Vec<String> = packs
        .iter()
        .map(|pack| {
            if pack.enabled {
                pack.name.clone()
            } else {
                format!(
                    "{}{}",
                    pack.name,
                    tr_l10n(lang, "settings.shortcuts.style_pack_disabled_suffix")
                )
            }
        })
        .collect();
    let selected = packs
        .iter()
        .position(|pack| pack.id == current_pack_id)
        .unwrap_or(0);
    let mut next = selected;
    // 草稿行的选择器也要有稳定且互不冲突的 id。
    let picker_salt = match row {
        Some(index) => format!("style-pack-hotkey-{index}"),
        None => "style-pack-hotkey-draft".to_string(),
    };
    egui::ComboBox::from_id_salt(picker_salt)
        .width(170.0)
        .selected_text(
            options
                .get(selected)
                .cloned()
                .unwrap_or_else(|| fallback_name.to_string()),
        )
        .show_ui(ui, |ui| {
            for (option_index, option) in options.iter().enumerate() {
                if ui
                    .selectable_label(option_index == selected, option)
                    .clicked()
                {
                    next = option_index;
                    ui.close();
                }
            }
        });
    if next != selected {
        actions.push(style_pack_pick_action(row, next));
    }
}

/// 设置行下方的细线（与 `row_desc` 同款）。
/// 录音方式在行下方的补充说明（与快捷键分区用同一组 `hotkey.mode*_suffix`）。
fn recording_mode_hint(lang: Lang, mode: usize) -> String {
    match mode {
        1 => tr_l10n(lang, "hotkey.mode_hold_suffix"),
        2 => tr_l10n(lang, "hotkey.mode_auto_suffix"),
        _ => tr_l10n(lang, "hotkey.mode_toggle_suffix"),
    }
    .to_string()
}

/// 行下方的小字提示（可见文字，不靠悬停）。空串不占位。
fn hint_line(ui: &mut egui::Ui, text: &str) {
    if text.is_empty() {
        return;
    }
    ui.label(egui::RichText::new(text).size(11.0).color(theme::INK_4));
}

fn separator_line(ui: &mut egui::Ui) {
    let rect = ui
        .allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover())
        .0;
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(0.5, theme::LINE_SOFT),
    );
}

fn appearance(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    card(ui, tr_l10n(lang, "settings.theme.title"), "", |ui| {
        combo_index_row(
            ui,
            tr_l10n(lang, "settings.theme.label"),
            "",
            vm.settings.theme,
            &[
                tr_l10n(lang, "settings.theme.system"),
                tr_l10n(lang, "settings.theme.light"),
                tr_l10n(lang, "settings.theme.dark"),
            ],
            |val| {
                actions.push(FrontendAction::SettingsCombo(
                    SettingsComboField::Theme,
                    val,
                ));
            },
        );
        toggle_row(
            ui,
            tr_l10n(lang, "settings.theme.activity_heatmap_label"),
            "",
            vm.settings.activity_heatmap,
            || {
                actions.push(FrontendAction::SettingsToggle(
                    SettingsField::ActivityHeatmap,
                ));
            },
        );
    });
    // 排版（Tauri `TypographySection`）：字族 / 字号 / 阅读宽度。
    let typography = vm.typography;
    card(ui, tr_l10n(lang, "uiRefinement.typography"), "", |ui| {
        use openless_linux_egui::typography::{ContentWidth, FontScale, TypographyField, UiFont};
        // 与 Tauri 的 FontCatalog 一致：只有「系统默认」是本地化文案，
        // 其余直接显示字体本名（不翻译）。这里必须写成字面量，否则
        // i18n 同步脚本扫不到 key，界面上会直接显示 `uiRefinement.native`。
        let fonts: Vec<&str> = vec![
            tr_l10n(lang, "uiRefinement.native"),
            "Avenir Next · Aptos",
            "PingFang / Microsoft YaHei",
            "New York · Georgia",
            "Songti / SimSun",
            "SF Mono · Cascadia Code",
            "Menlo · Consolas",
        ];
        let font_index = UiFont::ALL
            .iter()
            .position(|font| *font == typography.font)
            .unwrap_or(0);
        combo_index_row(
            ui,
            tr_l10n(lang, "uiRefinement.family"),
            tr_l10n(lang, "uiRefinement.fontHint"),
            font_index,
            &fonts,
            |val| {
                if let Some(font) = UiFont::ALL.get(val) {
                    actions.push(FrontendAction::SettingsTypography(TypographyField::Font(
                        *font,
                    )));
                }
            },
        );
        let scales: Vec<&str> = FontScale::ALL.iter().map(|scale| scale.label()).collect();
        let scale_index = FontScale::ALL
            .iter()
            .position(|scale| *scale == typography.scale)
            .unwrap_or(1);
        combo_index_row(
            ui,
            tr_l10n(lang, "uiRefinement.fontSize"),
            "",
            scale_index,
            &scales,
            |val| {
                if let Some(scale) = FontScale::ALL.get(val) {
                    actions.push(FrontendAction::SettingsTypography(TypographyField::Scale(
                        *scale,
                    )));
                }
            },
        );
        let widths: Vec<&str> = vec![
            tr_l10n(lang, "uiRefinement.narrow"),
            tr_l10n(lang, "uiRefinement.comfortable"),
            tr_l10n(lang, "uiRefinement.wide"),
        ];
        let width_index = ContentWidth::ALL
            .iter()
            .position(|width| *width == typography.width)
            .unwrap_or(1);
        combo_index_row(
            ui,
            tr_l10n(lang, "uiRefinement.width"),
            "",
            width_index,
            &widths,
            |val| {
                if let Some(width) = ContentWidth::ALL.get(val) {
                    actions.push(FrontendAction::SettingsTypography(TypographyField::Width(
                        *width,
                    )));
                }
            },
        );
    });
    card(
        ui,
        tr_l10n(lang, "settings.language.title"),
        tr_l10n(lang, "settings.language.desc"),
        |ui| {
            combo_index_row(
                ui,
                tr_l10n(lang, "settings.language.label"),
                tr_l10n(lang, "settings.language.label_desc"),
                vm.settings.language,
                &[
                    tr_l10n(lang, "settings.language.follow_system"),
                    tr_l10n(lang, "settings.language.zh"),
                    tr_l10n(lang, "settings.language.zh_tw"),
                    tr_l10n(lang, "settings.language.en"),
                    tr_l10n(lang, "settings.language.ja"),
                    tr_l10n(lang, "settings.language.ko"),
                ],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::Language,
                        val,
                    ));
                },
            );
            ui.label(
                egui::RichText::new(tr_l10n(lang, "settings.language.restart_hint"))
                    .size(11.0)
                    .color(theme::INK_4),
            );
        },
    );
}

/// AI-services sub-views. The list mirrors the Tauri `availableServiceViews`
/// gate: the multimodal view appears once the pipeline is enabled, the local
/// model view only when the host really has a local engine, and the tab strip
/// always ends with the connection settings.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ServiceView {
    Llm,
    Asr,
    Models,
    Connections,
    Omni,
}

impl ServiceView {
    fn id(self) -> usize {
        match self {
            Self::Llm => 0,
            Self::Asr => 1,
            Self::Models => 2,
            Self::Connections => 3,
            Self::Omni => 4,
        }
    }

    fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Llm => tr_l10n(lang, "modal.service_views.llm"),
            Self::Asr => tr_l10n(lang, "modal.service_views.asr"),
            Self::Models => tr_l10n(lang, "modal.service_views.models"),
            Self::Connections => tr_l10n(lang, "modal.service_views.connections"),
            Self::Omni => tr_l10n(lang, "modal.service_views.omni"),
        }
    }

    fn visible(vm: &FrontendViewModel) -> Vec<Self> {
        let mut views = vec![Self::Llm, Self::Asr, Self::Omni];
        if vm.supports_local_asr {
            views.push(Self::Models);
        }
        views.push(Self::Connections);
        views
    }

    fn inactive(self, multimodal: bool) -> bool {
        if multimodal {
            !matches!(self, Self::Omni | Self::Connections)
        } else {
            self == Self::Omni
        }
    }
}

fn omni_settings(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    use openless_linux_egui::omni_settings::{OmniAction, OmniField};
    let lang = vm.lang;
    let mut state = vm.omni.clone();
    let timer = ui.scope_id().with("omni-autosave");
    let mut changed = false;
    card(
        ui,
        tr_l10n(lang, "settings.providers.omniTitle"),
        "",
        |ui| {
            if state.busy {
                ui.label(tr_l10n(lang, "common.loading"));
            }
            ui.add_enabled_ui(state.loaded && !state.switching, |ui| {
                ui.add_enabled_ui(!state.busy, |ui| {
                    modal_form_row(
                        ui,
                        tr_l10n(lang, "settings.providers.providerLabel"),
                        |ui| {
                            let selected = state
                                .presets
                                .iter()
                                .position(|p| p.id == state.provider)
                                .unwrap_or(0);
                            let labels: Vec<String> = state
                                .presets
                                .iter()
                                .map(|preset| {
                                    let key =
                                        format!("settings.providers.presets.{}", preset.label_key);
                                    let label = fmt_l10n(lang, &key, &[]);
                                    if label == key {
                                        preset.label_key.clone()
                                    } else {
                                        label
                                    }
                                })
                                .collect();
                            egui::ComboBox::from_id_salt("omni-provider")
                                .width(ui.available_width())
                                .selected_text(
                                    labels.get(selected).map(String::as_str).unwrap_or_default(),
                                )
                                .show_ui(ui, |ui| {
                                    for (index, label) in labels.iter().enumerate() {
                                        if ui
                                            .selectable_label(index == selected, label.as_str())
                                            .clicked()
                                            && index != selected
                                            && state.validate().is_ok()
                                        {
                                            actions.push(FrontendAction::SettingsOmni(
                                                OmniAction::Provider(
                                                    state.presets[index].id.clone(),
                                                ),
                                            ));
                                            ui.data_mut(|data| data.remove_temp::<f64>(timer));
                                        }
                                    }
                                });
                        },
                    );
                });
                let preset = state.presets.iter().find(|p| p.id == state.provider);
                let model_hint = preset.map(|p| p.model.clone()).unwrap_or_default();
                let endpoint_hint = preset.map(|p| p.endpoint.clone()).unwrap_or_default();
                changed |= omni_text_field(
                    ui,
                    tr_l10n(lang, "settings.providers.modelLabel"),
                    &model_hint,
                    &mut state.model,
                    OmniField::Model,
                    false,
                    actions,
                );
                changed |= omni_text_field(
                    ui,
                    tr_l10n(lang, "settings.providers.apiKeyLabel"),
                    tr_l10n(lang, "providers.api_key_hint"),
                    &mut state.api_key,
                    OmniField::ApiKey,
                    true,
                    actions,
                );
                changed |= omni_text_field(
                    ui,
                    tr_l10n(lang, "settings.providers.baseUrlLabel"),
                    &endpoint_hint,
                    &mut state.endpoint,
                    OmniField::Endpoint,
                    false,
                    actions,
                );
                if state.provider == "custom" {
                    changed |= omni_text_field(
                        ui,
                        tr_l10n(lang, "settings.providers.temperatureLabel"),
                        tr_l10n(lang, "settings.providers.temperaturePlaceholder"),
                        &mut state.temperature,
                        OmniField::Temperature,
                        false,
                        actions,
                    );
                    changed |= omni_text_field(
                        ui,
                        tr_l10n(lang, "settings.providers.extraHeadersLabel"),
                        tr_l10n(lang, "settings.providers.extraHeadersPlaceholder"),
                        &mut state.extra_headers,
                        OmniField::ExtraHeaders,
                        true,
                        actions,
                    );
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "providers.extra_headers_hint"))
                            .size(11.5)
                            .color(theme::INK_4),
                    );
                }
                if let Err(error) = state.validate() {
                    ui.colored_label(theme::ERR, tr_l10n(lang, "common.operationFailed"));
                    ui.label(error.to_string());
                }
                ui.add_space(10.0);
                ui.add_enabled_ui(state.validate().is_ok() && !state.busy, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(tr_l10n(lang, "settings.providers.fetchModels"))
                            .clicked()
                        {
                            actions.push(FrontendAction::SettingsOmni(OmniAction::Models));
                        }
                        if !state.models.is_empty() {
                            egui::ComboBox::from_id_salt("omni-models")
                                .selected_text(tr_l10n(lang, "settings.providers.selectModel"))
                                .show_ui(ui, |ui| {
                                    for model in &state.models {
                                        if ui
                                            .selectable_label(*model == state.model, model)
                                            .clicked()
                                            && *model != state.model
                                        {
                                            state.model = model.clone();
                                            actions.push(FrontendAction::SettingsOmni(
                                                OmniAction::Field(OmniField::Model, model.clone()),
                                            ));
                                            changed = true;
                                        }
                                    }
                                });
                        }
                        if ui
                            .button(tr_l10n(lang, "settings.channels.verify"))
                            .clicked()
                        {
                            actions.push(FrontendAction::SettingsOmni(OmniAction::Validate));
                        }
                    });
                });
                if ui
                    .add_enabled(
                        !state.busy,
                        egui::Button::new(tr_l10n(lang, "btn.clear_secret")),
                    )
                    .clicked()
                {
                    actions.push(FrontendAction::SettingsOmni(OmniAction::ClearSecrets));
                    ui.data_mut(|data| data.remove_temp::<f64>(timer));
                }
            });
            if let Some(notice) = &state.notice {
                ui.label(notice);
            }
            if !state.busy
                && (!state.loaded || (state.dirty && state.notice.is_some()))
                && ui.button(tr_l10n(lang, "common.retry")).clicked()
            {
                actions.push(FrontendAction::SettingsOmni(if state.loaded {
                    OmniAction::Save
                } else {
                    OmniAction::Reload
                }));
            }
        },
    );
    let now = ui.input(|i| i.time);
    if changed {
        state.dirty = true;
        state.notice = None;
        ui.data_mut(|data| data.insert_temp(timer, now));
    }
    if !state.dirty {
        ui.data_mut(|data| data.remove_temp::<f64>(timer));
    }
    if let Some(at) = ui.data(|data| data.get_temp::<f64>(timer)) {
        if now - at >= 0.3 && !state.busy && state.loaded && state.validate().is_ok() {
            ui.data_mut(|data| data.remove_temp::<f64>(timer));
            actions.push(FrontendAction::SettingsOmni(OmniAction::Save));
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
    vm.omni = state;
}

fn omni_text_field(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    value: &mut String,
    field: openless_linux_egui::omni_settings::OmniField,
    secret: bool,
    actions: &mut Vec<FrontendAction>,
) -> bool {
    let mut changed = false;
    modal_form_row(ui, label, |ui| {
        changed = ui
            .add(
                egui::TextEdit::singleline(value)
                    .id(ui.scope_id().with(label))
                    .password(secret)
                    .hint_text(hint)
                    .desired_width(ui.available_width()),
            )
            .changed();
        if changed {
            actions.push(FrontendAction::SettingsOmni(
                openless_linux_egui::omni_settings::OmniAction::Field(field, value.clone()),
            ));
        }
    });
    changed
}

fn services(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(12)
        .inner_margin(16)
        .show(ui, |ui| {
            row_desc(
                ui,
                tr_l10n(lang, "settings.providers.pipeline_mode_label"),
                tr_l10n(lang, "settings.providers.pipeline_mode_hint"),
                |ui| {
                    let options = [
                        tr_l10n(lang, "settings.providers.pipeline_mode_traditional"),
                        tr_l10n(lang, "settings.providers.pipeline_mode_multimodal"),
                    ];
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 38.0),
                        egui::Sense::hover(),
                    );
                    if let Some(index) =
                        layout::segmented(ui, rect, &options, usize::from(vm.pipeline_multimodal))
                    {
                        actions.push(FrontendAction::SettingsCombo(
                            SettingsComboField::PipelineMode,
                            index,
                        ));
                    }
                },
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(tr_l10n(
                    lang,
                    "settings.providers.pipeline_isolation_notice",
                ))
                .size(11.0)
                .color(theme::INK_4),
            );
        });
    ui.add_space(14.0);
    let views = ServiceView::visible(vm);
    let active = views
        .iter()
        .position(|view| view.id() == vm.services_view && !view.inactive(vm.pipeline_multimodal))
        .unwrap_or_else(|| {
            views
                .iter()
                .position(|view| !view.inactive(vm.pipeline_multimodal))
                .unwrap_or(0)
        });
    let items: Vec<(&str, Option<egui::Color32>, bool)> = views
        .iter()
        .map(|view| {
            let dot = match view {
                ServiceView::Llm | ServiceView::Asr if !view.inactive(vm.pipeline_multimodal) => {
                    let configured = vm.service_configured[usize::from(*view == ServiceView::Asr)];
                    Some(if configured { theme::OK } else { theme::ERR })
                }
                _ => None,
            };
            (view.label(lang), dot, view.inactive(vm.pipeline_multimodal))
        })
        .collect();
    if let Some(index) = service_tabs(ui, &items, active, lang) {
        if let Some(view) = views.get(index) {
            actions.push(FrontendAction::SettingsServicesView(view.id()));
        }
    }
    ui.add_space(10.0);

    match views.get(active).copied().unwrap_or(ServiceView::Llm) {
        ServiceView::Omni => omni_settings(ui, vm, actions),
        ServiceView::Models => {
            card(
                ui,
                tr_l10n(lang, "modal.service_views.models"),
                tr_l10n(lang, "settings.advanced.local_asr_desc"),
                |ui| {
                    ui.label(
                        egui::RichText::new(tr_l10n(
                            lang,
                            "settings.advanced.platform_not_supported",
                        ))
                        .size(11.5)
                        .color(theme::INK_4),
                    );
                },
            );
        }
        ServiceView::Connections => {
            card(ui, tr_l10n(lang, "settings.network.title"), "", |ui| {
                toggle_row(
                    ui,
                    tr_l10n(lang, "settings.network.use_system_proxy_label"),
                    tr_l10n(lang, "settings.network.use_system_proxy_desc"),
                    vm.settings.system_proxy,
                    || {
                        actions.push(FrontendAction::SettingsToggle(SettingsField::SystemProxy));
                    },
                );
            });
            card(
                ui,
                tr_l10n(lang, "settings.marketplace.title"),
                tr_l10n(lang, "settings.marketplace.desc"),
                |ui| {
                    action_row(
                        ui,
                        tr_l10n(lang, "settings.marketplace.github.sign_in"),
                        "",
                        tr_l10n(lang, "settings.marketplace.github.open_github"),
                        SettingsActionField::OpenGitHub,
                        actions,
                    );
                },
            );
        }
        view => {
            // 视图 → 渠道类型（0 = 语言模型，1 = 语音识别）。宿主按该类型取数。
            let kinds: &[usize] = match view {
                ServiceView::Asr => &[1],
                _ => &[0],
            };
            for kind in kinds {
                let asr = *kind == 1;
                let title = if asr {
                    tr_l10n(lang, "settings.channels.asr_title")
                } else {
                    tr_l10n(lang, "settings.channels.llm_title")
                };
                let add = tr_l10n(lang, "settings.channels.add");
                let mut add_clicked = false;
                // 卡片头：标题在左、＋添加渠道在右（Tauri 的 ProvidersSection），
                // 标题下方一行说明，再下面是渠道行。
                card_header(
                    ui,
                    title,
                    "",
                    |ui| {
                        let add_width = layout::text_width(ui, add, 12.0) + 30.0;
                        let (add_rect, _) = ui
                            .allocate_exact_size(egui::vec2(add_width, 26.0), egui::Sense::hover());
                        add_clicked = layout::action_button(
                            ui,
                            add_rect,
                            add,
                            None,
                            layout::ButtonKind::Blue,
                        )
                        .clicked();
                    },
                    |ui| {
                        ui.label(
                            egui::RichText::new(tr_l10n(lang, "settings.channels.order_hint"))
                                .size(11.0)
                                .color(theme::INK_4),
                        );
                        ui.add_space(6.0);
                        if vm.channels_loading {
                            ui.label(
                                egui::RichText::new(tr_l10n(lang, "common.loading"))
                                    .size(11.5)
                                    .color(theme::INK_4),
                            );
                        } else if vm.channels.is_empty() {
                            ui.label(
                                egui::RichText::new(tr_l10n(lang, "settings.channels.empty"))
                                    .size(11.5)
                                    .color(theme::INK_4),
                            );
                        } else {
                            let held = ui.input(|input| {
                                input.focused
                                    && input.pointer.primary_down()
                                    && !input.key_pressed(egui::Key::Escape)
                            });
                            if !held || vm.settings_notice.is_some() {
                                ui.data_mut(|data| {
                                    data.remove::<ChannelDrag>(egui::Id::unique(
                                        "openless-settings-channel-drag",
                                    ))
                                });
                            }
                            for (index, channel) in vm.channels.iter().enumerate() {
                                ui.push_id(&channel.id, |ui| {
                                    channel_row(ui, channel, index, &vm.channels, lang, actions)
                                });
                            }
                        }
                    },
                );
                if add_clicked {
                    // Tauri `startCreate`：先建空渠道再开同一个弹窗（草稿模式）。
                    actions.push(FrontendAction::SettingsChannelDraft);
                }
            }
            ui.label(
                egui::RichText::new(tr_l10n(
                    lang,
                    "settings.providers.credential_storage_notice",
                ))
                .size(11.0)
                .color(theme::INK_4),
            );
        }
    }
}

fn privacy(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        // 与 Tauri 的隐私页同序：权限 → 数据与存储 → 云端同步。
        // 权限卡先给结论（麦克风 / 快捷键 / 网络 / 文字插入 / 保险库），
        // 再让用户改留存与同步设置。
        privacy_access(ui, vm, actions);
        privacy_data(ui, vm, actions);
        super::cloud_sync::cloud_sync_card(ui, vm, actions);
    });
}

fn privacy_note(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).size(11.5).color(theme::INK_3))
            .wrap()
            .halign(egui::Align::Min),
    );
}

fn privacy_access(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    use openless_linux_egui::linux_access::{CheckState, KeyringState};
    let lang = vm.lang;
    let checking = vm.linux_access.checking;
    let mut refresh = false;
    card_header(
        ui,
        tr_l10n(lang, "settings.permissions.title"),
        "", // 说明放在卡片正文里，标题行只留结论与动作
        |ui| {
            // 标题行右侧：重新检查（Tauri 的 Retry 按钮等价物）
            refresh = ui
                .add_enabled(
                    !checking,
                    egui::Button::new(if checking {
                        tr_l10n(lang, "settings.permissions.checking")
                    } else {
                        tr_l10n(lang, "settings.linux_privacy.refresh")
                    }),
                )
                .clicked();
        },
        |ui| {
            privacy_note(ui, tr_l10n(lang, "settings.linux_privacy.desc"));
            ui.add_space(6.0);
            let microphone_detected =
                vm.permissions.microphone == super::view_model::PermissionState::Granted;
            status_row(
                ui,
                tr_l10n(lang, "settings.permissions.mic_label"),
                tr_l10n(lang, "settings.linux_privacy.mic_desc"),
                if microphone_detected {
                    tr_l10n(lang, "settings.linux_privacy.mic_detected")
                } else {
                    tr_l10n(lang, "settings.permissions.indeterminate")
                },
                if microphone_detected {
                    theme::OK
                } else {
                    theme::INK_4
                },
            );
            let hotkey_ready = vm.permissions.hotkey == super::view_model::PermissionState::Granted
                && vm.linux_access.input == CheckState::Ready;
            status_row(
                ui,
                tr_l10n(lang, "settings.permissions.hotkey_label"),
                tr_l10n(lang, "settings.linux_privacy.hotkey_desc"),
                if checking {
                    tr_l10n(lang, "settings.permissions.checking")
                } else if hotkey_ready {
                    tr_l10n(lang, "settings.linux_privacy.hotkey_ready")
                } else if vm.linux_access.input == CheckState::Unavailable {
                    tr_l10n(lang, "settings.permissions.hotkey_failed")
                } else {
                    tr_l10n(lang, "settings.linux_privacy.not_checked")
                },
                if hotkey_ready && !checking {
                    theme::OK
                } else {
                    theme::INK_4
                },
            );
            let (network_text, network_color) = if checking {
                (
                    tr_l10n(lang, "settings.permissions.checking").to_string(),
                    theme::INK_4,
                )
            } else {
                match vm.linux_access.network {
                    CheckState::Ready => (
                        fmt_l10n(
                            lang,
                            "settings.linux_privacy.network_online",
                            &[&vm.linux_access.network_latency_ms.unwrap_or(0)],
                        ),
                        theme::OK,
                    ),
                    CheckState::Unavailable => (
                        tr_l10n(lang, "settings.linux_privacy.network_unavailable").to_string(),
                        theme::WARN,
                    ),
                    CheckState::Unknown => (
                        tr_l10n(lang, "settings.linux_privacy.not_checked").to_string(),
                        theme::INK_4,
                    ),
                }
            };
            status_row(
                ui,
                tr_l10n(lang, "settings.permissions.network_label"),
                tr_l10n(lang, "settings.linux_privacy.network_probe_desc"),
                &network_text,
                network_color,
            );
            let (input_text, input_color) = if checking {
                (tr_l10n(lang, "settings.permissions.checking"), theme::INK_4)
            } else {
                match vm.linux_access.input {
                    CheckState::Ready => (
                        tr_l10n(lang, "settings.linux_privacy.input_ready"),
                        theme::OK,
                    ),
                    CheckState::Unavailable => (
                        tr_l10n(lang, "settings.linux_privacy.input_unavailable"),
                        theme::WARN,
                    ),
                    CheckState::Unknown => (
                        tr_l10n(lang, "settings.linux_privacy.not_checked"),
                        theme::INK_4,
                    ),
                }
            };
            status_row(
                ui,
                tr_l10n(lang, "settings.linux_privacy.input_label"),
                tr_l10n(lang, "settings.linux_privacy.input_desc"),
                input_text,
                input_color,
            );
        },
    );
    if refresh {
        actions.push(FrontendAction::SettingsAction(
            SettingsActionField::RefreshLinuxAccess,
        ));
    }
    card(
        ui,
        tr_l10n(lang, "settings.linux_privacy.keyring_title"),
        "",
        |ui| {
            privacy_note(ui, tr_l10n(lang, "settings.linux_privacy.keyring_desc"));
            ui.add_space(6.0);
            let (text, color) = if checking {
                (tr_l10n(lang, "settings.permissions.checking"), theme::INK_4)
            } else {
                match vm.linux_access.keyring {
                    KeyringState::Ready => (
                        tr_l10n(lang, "settings.linux_privacy.keyring_ready"),
                        theme::OK,
                    ),
                    KeyringState::Locked => (
                        tr_l10n(lang, "settings.linux_privacy.keyring_locked"),
                        theme::WARN,
                    ),
                    KeyringState::MissingService => (
                        tr_l10n(lang, "settings.linux_privacy.keyring_missing_service"),
                        theme::WARN,
                    ),
                    KeyringState::MissingCollection => (
                        tr_l10n(lang, "settings.linux_privacy.keyring_missing_collection"),
                        theme::WARN,
                    ),
                    KeyringState::TemporaryVault => (
                        tr_l10n(lang, "settings.linux_privacy.keyring_temporary"),
                        theme::WARN,
                    ),
                    KeyringState::Unavailable => (
                        tr_l10n(lang, "settings.linux_privacy.keyring_unavailable"),
                        theme::WARN,
                    ),
                    KeyringState::RestartRequired => (
                        tr_l10n(lang, "settings.linux_privacy.keyring_restart"),
                        theme::WARN,
                    ),
                    KeyringState::Unknown => (
                        tr_l10n(lang, "settings.linux_privacy.not_checked"),
                        theme::INK_4,
                    ),
                }
            };
            status_row(ui, "Secret Service", "", text, color);
            privacy_note(
                ui,
                tr_l10n(lang, "settings.linux_privacy.keyring_probe_desc"),
            );
            if !checking
                && !matches!(
                    vm.linux_access.keyring,
                    KeyringState::Ready | KeyringState::Unknown
                )
            {
                privacy_note(ui, tr_l10n(lang, "settings.linux_privacy.keyring_recovery"));
            }
        },
    );
}

fn privacy_data(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    card(ui, tr_l10n(lang, "settings.data_storage.title"), "", |ui| {
        privacy_note(ui, tr_l10n(lang, "settings.linux_privacy.intro_desc"));
        ui.add_space(6.0);
        privacy_note(ui, tr_l10n(lang, "settings.linux_privacy.storage_desc"));
        ui.add_space(6.0);
        row_desc(
            ui,
            tr_l10n(lang, "settings.linux_privacy.data_directory"),
            "",
            |ui| {
                let directory = if vm.application_data_directory.is_empty() {
                    tr_l10n(lang, "settings.permissions.indeterminate")
                } else {
                    &vm.application_data_directory
                };
                ui.add(
                    egui::Label::new(egui::RichText::new(directory).monospace())
                        .wrap()
                        .halign(egui::Align::Min),
                );
            },
        );
        text_edit_row(
            ui,
            tr_l10n(lang, "settings.recording.history_retention_label"),
            "",
            &mut vm.settings.retention_days,
            "0",
            |text| {
                actions.push(FrontendAction::SettingsText(
                    SettingsTextField::RetentionDays,
                    text.to_owned(),
                ));
            },
        );
        text_edit_row(
            ui,
            tr_l10n(lang, "settings.recording.history_max_entries_label"),
            "",
            &mut vm.settings.history_max_entries,
            "200",
            |text| {
                actions.push(FrontendAction::SettingsText(
                    SettingsTextField::HistoryMaxEntries,
                    text.to_owned(),
                ));
            },
        );
        text_edit_row(
            ui,
            tr_l10n(lang, "settings.recording.polish_context_window_label"),
            tr_l10n(lang, "settings.recording.polish_context_window_desc"),
            &mut vm.settings.polish_context_window,
            "0",
            |text| {
                actions.push(FrontendAction::SettingsText(
                    SettingsTextField::PolishContextWindow,
                    text.to_owned(),
                ));
            },
        );
        text_edit_row(
            ui,
            tr_l10n(lang, "settings.recording.polish_skip_max_chars_label"),
            tr_l10n(lang, "settings.recording.polish_skip_max_chars_desc"),
            &mut vm.settings.polish_skip_max_chars,
            "0",
            |text| {
                actions.push(FrontendAction::SettingsText(
                    SettingsTextField::PolishSkipMaxChars,
                    text.to_owned(),
                ));
            },
        );
    });
}

fn advanced(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    // The Tauri page is a list of drill-in rows; the panel swaps to the detail
    // page (title + back button) while `advanced_open` is set.
    // Keep existing drill-in IDs, but the pipeline now lives only on Services.
    let rows = [
        (
            2,
            SettingsIcon::Bolt,
            tr_l10n(lang, "settings.debug.title"),
            tr_l10n(lang, "modal.advanced_pages.debug"),
        ),
    ];
    if rows.iter().any(|(id, _, _, _)| *id == vm.advanced_open) {
        card(ui, "", "", |ui| {
                toggle_row(
                    ui,
                    tr_l10n(lang, "settings.recording.record_audio_for_debug_label"),
                    "",
                    vm.settings.record_audio_for_debug,
                    || {
                        actions.push(FrontendAction::SettingsToggle(
                            SettingsField::RecordAudioForDebug,
                        ));
                    },
                );
                // 调试录音保存哪一份：两个独立开关，可以只留处理前、只留处理后，
                // 或者两份都留来做 A/B 对照。主开关关掉时这两项无意义。
                if vm.settings.record_audio_for_debug {
                    ui.add_enabled_ui(vm.settings.record_audio_for_debug, |ui| {
                        toggle_row(
                            ui,
                            tr_l10n(lang, "settings.recording.archive_processed_label"),
                            tr_l10n(lang, "settings.recording.archive_processed_desc"),
                            vm.settings.debug_archive_processed,
                            || {
                                actions.push(FrontendAction::SettingsToggle(
                                    SettingsField::DebugArchiveProcessed,
                                ));
                            },
                        );
                        toggle_row(
                            ui,
                            tr_l10n(lang, "settings.recording.archive_raw_label"),
                            tr_l10n(lang, "settings.recording.archive_raw_desc"),
                            vm.settings.debug_archive_raw,
                            || {
                                actions.push(FrontendAction::SettingsToggle(
                                    SettingsField::DebugArchiveRaw,
                                ));
                            },
                        );
                    });
                }
                text_edit_row(
                    ui,
                    tr_l10n(lang, "settings.recording.audio_recording_max_entries_label"),
                    tr_l10n(lang, "settings.recording.audio_recording_max_entries_desc"),
                    &mut vm.settings.audio_recording_max_entries,
                    "50",
                    |text| {
                        actions.push(FrontendAction::SettingsText(
                            SettingsTextField::AudioRecordingMaxEntries,
                            text.to_owned(),
                        ));
                    },
                );
                combo_index_row(
                    ui,
                    tr_l10n(lang, "settings.debug.log_level_label"),
                    tr_l10n(lang, "settings.debug.log_level_desc"),
                    vm.settings.log_level.min(2),
                    &["info", "debug", "trace"],
                    |index| {
                        actions.push(FrontendAction::SettingsCombo(
                            SettingsComboField::LogLevel,
                            index,
                        ));
                    },
                );
                action_row(
                    ui,
                    tr_l10n(lang, "settings.debug.title"),
                    "",
                    tr_l10n(lang, "btn.export_error_log"),
                    SettingsActionField::ExportDiagnostics,
                    actions,
                );
        });
        return;
    }
    card(ui, "", "", |ui| {
        for (id, icon, title, description) in &rows {
            if drill_row(ui, *icon, title, description) {
                vm.advanced_open = *id;
            }
        }
    });
}

/// A drill-in row: icon + bold title + description + chevron. Returns true when
/// the row was clicked (the panel then swaps to that detail page).
fn drill_row(ui: &mut egui::Ui, icon: SettingsIcon, title: &str, description: &str) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.0), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            egui::Color32::from_rgba_unmultiplied(244, 244, 245, 140),
        );
    }
    draw_rail_icon(
        ui,
        egui::pos2(rect.left() + 18.0, rect.center().y),
        icon,
        theme::INK_2,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 40.0, rect.center().y - 9.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(13.5),
        theme::INK,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 40.0, rect.center().y + 9.0),
        egui::Align2::LEFT_CENTER,
        description,
        egui::FontId::proportional(11.5),
        theme::INK_4,
    );
    let chevron = egui::pos2(rect.right() - 12.0, rect.center().y);
    let stroke = egui::Stroke::new(1.2, theme::INK_4);
    ui.painter().line_segment(
        [
            egui::pos2(chevron.x - 2.5, chevron.y - 4.5),
            egui::pos2(chevron.x + 2.0, chevron.y),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(chevron.x + 2.0, chevron.y),
            egui::pos2(chevron.x - 2.5, chevron.y + 4.5),
        ],
        stroke,
    );
    response.clicked()
}

fn about(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    let icon = layout::load_app_icon(ui.ctx());
    card(ui, "", "", |ui| {
        let header_width = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(header_width, 64.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |row| {
                let (icon_rect, _) =
                    row.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
                row.painter().rect_filled(
                    icon_rect.translate(egui::vec2(0.0, 2.0)),
                    egui::CornerRadius::same(13),
                    egui::Color32::from_black_alpha(28),
                );
                row.painter()
                    .rect_filled(icon_rect, egui::CornerRadius::same(13), theme::SURFACE);
                row.painter().rect_stroke(
                    icon_rect,
                    egui::CornerRadius::same(13),
                    egui::Stroke::new(0.5, theme::LINE),
                    egui::StrokeKind::Inside,
                );
                row.painter().image(
                    icon.id(),
                    icon_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                row.add_space(12.0);
                row.vertical(|ui| {
                    ui.label(egui::RichText::new("OpenLess").size(17.0).strong());
                    ui.label(
                        egui::RichText::new(format!(
                            "{} · v{}",
                            tr_l10n(lang, "settings.about.tagline"),
                            vm.version
                        ))
                        .size(12.0)
                        .color(theme::INK_3),
                    );
                });
            },
        );
        if let Some(notice) = &vm.settings_notice {
            ui.label(egui::RichText::new(notice).size(11.0).color(theme::BLUE));
        }
    });
    // The Tauri link card groups documentation links above a divider.
    ui.add_space(-8.0);
    card(ui, tr_l10n(lang, "settings.about.links_title"), "", |ui| {
        link_row(
            ui,
            tr_l10n(lang, "settings.about.source"),
            "GitHub",
            SettingsActionField::OpenGitHub,
            actions,
        );
        link_row(
            ui,
            tr_l10n(lang, "settings.about.docs"),
            tr_l10n(lang, "modal.about.docs_btn"),
            SettingsActionField::OpenHelp,
            actions,
        );
        separator_line(ui);
        link_row(
            ui,
            tr_l10n(lang, "modal.sections.help_center"),
            tr_l10n(lang, "modal.sections.help_center"),
            SettingsActionField::OpenHelp,
            actions,
        );
        link_row(
            ui,
            tr_l10n(lang, "modal.sections.release_notes"),
            tr_l10n(lang, "modal.sections.release_notes"),
            SettingsActionField::OpenReleaseNotes,
            actions,
        );
        link_row(
            ui,
            tr_l10n(lang, "settings.about.feedback"),
            tr_l10n(lang, "modal.about.feedback_btn"),
            SettingsActionField::OpenFeedback,
            actions,
        );
        link_row(
            ui,
            tr_l10n(lang, "settings.about.qq"),
            "1078960553",
            SettingsActionField::CopyQQ,
            actions,
        );
    });
}

#[derive(Clone)]
struct ChannelDrag {
    channel_id: String,
    pending: Option<(String, isize)>,
}

/// 一个渠道卡片（Tauri `ChannelList` 的行）。
///
/// 结构照抄 Tauri：左侧拖拽手柄 → 名称 + 徽章 → `provider · model`（模型用等宽）
/// → 「上次验证 …」状态块；右侧「重新验证 / 启用 / 编辑 ›」。
/// 换顺序靠手柄拖动 —— Tauri 那边同样是拖动重排，没有上下箭头按钮，
/// 渠道类型与删除都在编辑页里。
fn channel_row(
    ui: &mut egui::Ui,
    channel: &super::view_model::SettingsChannel,
    index: usize,
    channels: &[super::view_model::SettingsChannel],
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    // 拖动身份不随排序变化；同一次交换等宿主确认后再提交下一次。
    let drag_id = egui::Id::unique("openless-settings-channel-drag");
    let dragging = ui.data(|data| data.get_temp::<ChannelDrag>(drag_id));

    let card = egui::Frame::new()
        // 当前使用中的渠道：中性灰底 + 细描边。Tauri 特意不用蓝底竖条 ——
        //「当前使用」徽章已经说明问题，整行染色太花哨。
        .fill(if channel.is_active {
            theme::SURFACE_2
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(if channel.is_active {
            egui::Stroke::new(0.5, theme::LINE)
        } else {
            egui::Stroke::NONE
        })
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(12, 14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;

                // ── 拖拽手柄（Tauri 的 `⠿`）
                let (handle_rect, handle) =
                    ui.allocate_exact_size(egui::vec2(16.0, 24.0), egui::Sense::drag());
                let lifted = dragging
                    .as_ref()
                    .is_some_and(|drag| drag.channel_id == channel.id);
                icons::draw_icon_sized(
                    ui,
                    handle_rect.center(),
                    IconName::Grip,
                    if lifted || handle.hovered() {
                        theme::INK
                    } else {
                        theme::INK_4
                    },
                    16.0,
                );
                if handle.hovered() || lifted {
                    handle
                        .clone()
                        .on_hover_text(tr_l10n(lang, "settings.channels.drag_hint"));
                }
                if handle.drag_started() {
                    ui.data_mut(|data| {
                        data.insert_temp(
                            drag_id,
                            ChannelDrag {
                                channel_id: channel.id.clone(),
                                pending: None,
                            },
                        )
                    });
                }
                if handle.drag_stopped() {
                    ui.data_mut(|data| data.remove::<ChannelDrag>(drag_id));
                }

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        // Tauri：`const label = channel.name.trim() || providerLabel`
                        // —— 用户没给渠道起名时，标题就是供应商名。
                        let title = if channel.name.trim().is_empty() {
                            channel.provider.as_str()
                        } else {
                            channel.name.trim()
                        };
                        ui.label(
                            egui::RichText::new(title)
                                .size(14.0)
                                .strong()
                                .color(theme::INK),
                        );
                        if channel.is_active {
                            egui::Frame::new()
                                .fill(theme::BLUE_SOFT)
                                .corner_radius(egui::CornerRadius::same(9))
                                .inner_margin(egui::Margin::symmetric(7, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(tr_l10n(
                                            lang,
                                            "settings.channels.current",
                                        ))
                                        .size(10.0)
                                        .color(theme::BLUE),
                                    );
                                });
                        }
                        if !channel.enabled {
                            egui::Frame::new()
                                .stroke(egui::Stroke::new(0.5, theme::LINE))
                                .corner_radius(egui::CornerRadius::same(9))
                                .inner_margin(egui::Margin::symmetric(7, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(tr_l10n(
                                            lang,
                                            "settings.channels.disabled",
                                        ))
                                        .size(10.0)
                                        .color(theme::INK_4),
                                    );
                                });
                        }
                    });
                    // 第二行：`provider · model`，模型用等宽（Tauri 的 ol-font-mono）。
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // 第二行的供应商前缀只在标题用了自定义名字时才加，否则
                        // 会和标题重复（Tauri 的 `{channel.name.trim() && …}`）。
                        if !channel.name.trim().is_empty() {
                            ui.label(
                                egui::RichText::new(format!("{} ·", channel.provider))
                                    .size(12.0)
                                    .color(theme::INK_3),
                            );
                        }
                        let model = if channel.model.trim().is_empty() {
                            egui::RichText::new(tr_l10n(lang, "settings.channels.model_not_set"))
                                .size(12.0)
                                .color(theme::INK_3)
                        } else {
                            egui::RichText::new(channel.model.trim())
                                .size(12.0)
                                .monospace()
                                .color(theme::INK_3)
                        };
                        ui.label(model);
                    });
                    channel_status_line(ui, channel, lang);
                });

                // ── 右侧动作：与 Tauri 同序（右到左压栈 → 屏幕顺序：验证、启用、编辑）
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    channel_edit_button(ui, lang, || {
                        actions.push(FrontendAction::SettingsChannelSelect(index));
                    });
                    ui.add_space(8.0);
                    let (switch, _) =
                        ui.allocate_exact_size(egui::vec2(36.0, 20.0), egui::Sense::hover());
                    if layout::toggle(ui, switch, channel.enabled, ("settings-channel", index))
                        .clicked()
                    {
                        actions.push(FrontendAction::SettingsChannelToggle(index));
                    }
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "settings.channels.enabled"))
                            .size(11.0)
                            .color(theme::INK_3),
                    );
                    ui.add_space(8.0);
                    let testing = channel_testing(ui, channel);
                    let verify = if testing {
                        tr_l10n(lang, "settings.channels.verifying")
                    } else if channel.last_ok.is_some() {
                        tr_l10n(lang, "settings.channels.reverify")
                    } else {
                        tr_l10n(lang, "settings.channels.verify")
                    };
                    let response = ui.add_enabled(
                        !testing,
                        egui::Button::new(egui::RichText::new(verify).size(11.0))
                            .fill(theme::SURFACE_2)
                            .stroke(egui::Stroke::new(0.8, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(0.0, 24.0)),
                    );
                    if response.clicked() {
                        mark_channel_testing(ui, channel);
                        actions.push(FrontendAction::SettingsChannelValidate(index));
                    }
                });
            });
        })
        .response;

    if let Some(mut drag) = dragging.filter(|drag| drag.channel_id == channel.id) {
        if let Some((neighbor, delta)) = &drag.pending {
            if index
                .checked_add_signed(*delta)
                .and_then(|target| channels.get(target))
                .is_some_and(|channel| channel.id == *neighbor)
            {
                return;
            }
            drag.pending = None;
        }
        if let Some(pointer) = ui.ctx().pointer_latest_pos() {
            let delta = if pointer.y > card.rect.bottom() + 2.0 {
                1
            } else if pointer.y < card.rect.top() - 2.0 {
                -1
            } else {
                0
            };
            if delta != 0 {
                if let Some(neighbor) = index
                    .checked_add_signed(delta)
                    .and_then(|target| channels.get(target))
                {
                    actions.push(FrontendAction::SettingsChannelMove {
                        channel_id: channel.id.clone(),
                        neighbor_id: neighbor.id.clone(),
                        delta,
                    });
                    drag.pending = Some((neighbor.id.clone(), delta));
                }
            }
        }
        if ui.input(|input| {
            input.focused && input.pointer.primary_down() && !input.key_pressed(egui::Key::Escape)
        }) {
            ui.data_mut(|data| data.insert_temp(drag_id, drag));
        }
    }
}

/// 验证请求在飞的那几秒：按钮变「正在验证…」并禁用。
///
/// Tauri 的 `testingIds` 同样是**界面本地**状态（promise 落地即清除），所以这里
/// 也放在 egui 的临时数据里：结果回包（`last_check_age_seconds` 变新）或超时即恢复。
fn channel_testing(ui: &egui::Ui, channel: &super::view_model::SettingsChannel) -> bool {
    /// 快照里出现这么新的结果就算回包了。
    const FRESH_RESULT_SECONDS: i64 = 30;

    let id = egui::Id::unique(("settings-channel-testing", &channel.id));
    let Some(deadline) = ui.data(|data| data.get_temp::<f64>(id)) else {
        return false;
    };
    let expired = ui.input(|input| input.time) >= deadline;
    let answered = channel
        .last_check_age_seconds
        .is_some_and(|age| age <= FRESH_RESULT_SECONDS);
    if expired || answered {
        ui.data_mut(|data| data.remove_temp::<f64>(id));
        return false;
    }
    true
}

fn mark_channel_testing(ui: &egui::Ui, channel: &super::view_model::SettingsChannel) {
    /// 与 `channel_testing` 的上限一致。
    const MAX_TESTING_SECONDS: f64 = 30.0;
    let id = egui::Id::unique(("settings-channel-testing", &channel.id));
    let now = ui.input(|input| input.time);
    ui.data_mut(|data| data.insert_temp(id, now + MAX_TESTING_SECONDS));
}

/// Tauri 的 `.ol-channel-edit-button`：铅笔 + 「编辑」+ 右尖括号。
fn channel_edit_button(ui: &mut egui::Ui, lang: Lang, mut on_click: impl FnMut()) {
    let label = tr_l10n(lang, "settings.channels.edit");
    let width = 14.0 + 6.0 + layout::text_width(ui, label, 11.0) + 6.0 + 14.0 + 12.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
    }
    let icon_size = 14.0;
    let mut cursor = rect.left() + 6.0;
    icons::draw_icon_sized(
        ui,
        egui::pos2(cursor + icon_size / 2.0, rect.center().y),
        IconName::Pencil,
        theme::INK_2,
        icon_size,
    );
    cursor += icon_size + 6.0;
    ui.painter().text(
        egui::pos2(cursor, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.0),
        theme::INK_2,
    );
    cursor += layout::text_width(ui, label, 11.0) + 6.0;
    icons::draw_icon_sized(
        ui,
        egui::pos2(cursor + icon_size / 2.0, rect.center().y),
        IconName::ChevronRight,
        theme::INK_4,
        icon_size,
    );
    if response.clicked() {
        on_click();
    }
}

/// 「上次验证 …」状态块（Tauri `ChannelList` 的 `ChannelTestStatus`）。
///
/// 结构：`上次验证` + 结果（通过 + 耗时，或失败 + 错误 + 「验证失败不会自动停用」
/// 说明）+ 多久以前；结果超过 24 小时再加一条过期提示。
fn channel_status_line(
    ui: &mut egui::Ui,
    channel: &super::view_model::SettingsChannel,
    lang: Lang,
) {
    /// 与 Tauri `STALE_TEST_SECONDS` 一致：一天前的验证结果不可信。
    const STALE_AFTER_SECONDS: i64 = 24 * 60 * 60;

    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            egui::RichText::new(tr_l10n(lang, "settings.channels.last_check"))
                .size(11.0)
                .color(theme::INK_3),
        );
        let Some(passed) = channel.last_ok else {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "settings.channels.not_verified"))
                    .size(11.0)
                    .color(theme::INK_3),
            );
            return;
        };
        let stale = channel
            .last_check_age_seconds
            .is_some_and(|age| age > STALE_AFTER_SECONDS);
        if passed {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "settings.channels.passed"))
                    .size(11.0)
                    .color(if stale { theme::INK_2 } else { theme::OK }),
            );
            if let Some(ms) = channel.last_latency_ms {
                ui.label(
                    egui::RichText::new(fmt_l10n(lang, "settings.channels.elapsed", &[&ms]))
                        .size(11.0)
                        .color(theme::INK_3),
                );
            }
        } else {
            let error = channel.last_error.as_deref().unwrap_or_default();
            let text = if error.trim().is_empty() {
                tr_l10n(lang, "settings.channels.failed_plain").to_string()
            } else {
                fmt_l10n(lang, "settings.channels.failed", &[&error])
            };
            ui.label(egui::RichText::new(text).size(11.0).color(theme::ERR));
            ui.label(
                egui::RichText::new(tr_l10n(lang, "settings.channels.failure_keeps_enabled"))
                    .size(11.0)
                    .color(theme::INK_2),
            );
        }
        if let Some(age) = channel.last_check_age_seconds {
            ui.label(
                egui::RichText::new(relative_check_age(lang, age))
                    .size(11.0)
                    .color(theme::INK_3),
            );
            if stale {
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "settings.channels.stale_result"))
                        .size(11.0)
                        .color(theme::INK_2),
                );
            }
        }
    });
}

/// 「刚刚 / N 分钟前 / N 小时前 / N 天前」，与 Tauri `relativeTime` 同规则。
fn relative_check_age(lang: Lang, seconds: i64) -> String {
    if seconds < 60 {
        return tr_l10n(lang, "settings.channels.just_now").to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return fmt_l10n(lang, "settings.channels.minutes_ago", &[&minutes]);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return fmt_l10n(lang, "settings.channels.hours_ago", &[&hours]);
    }
    fmt_l10n(lang, "settings.channels.days_ago", &[&(hours / 24)])
}

/// Provider + name form used by "add channel".
/// One editor text row. The pushed value is the post-edit text: pushing the
/// pre-edit copy would make the host write the old value straight back into the
/// field on every keystroke.
/// 模型设置块：对应 Tauri `ProvidersSection` 里的「模型设置」分区。
///
/// - 模型字段（`ChannelModelField`）：descriptor 带 `staticModels` 时是一个预设下拉，
///   末尾附「自定义模型…」逃生口切到手输；没预设时是普通输入框。值为空且有
///   `defaultModel` 时给一个「填入默认」。
/// - 可用模型（`ProviderTools`）：当前 endpoint 命中带文档页的预设时按钮是
///   「查看支持的模型」（打开文档）；否则是「拉取模型」。拉到后下拉选择，
///   **选中即落地凭据**（Tauri `applyModel`）。
fn provider_model_block(
    ui: &mut egui::Ui,
    editor: &SettingsProviderEditor,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(tr_l10n(lang, "settings.channels.modelTitle"))
            .font(theme::medium_font(12.0))
            .color(theme::INK),
    );
    ui.label(
        egui::RichText::new(if editor.has_models_url {
            tr_l10n(lang, "settings.providers.planModelsHint")
        } else {
            tr_l10n(lang, "settings.channels.modelHint")
        })
        .size(11.0)
        .color(theme::INK_4),
    );
    ui.add_space(4.0);

    // 模型字段：预设下拉 ⇄ 自定义输入。
    modal_form_row(ui, tr_l10n(lang, "settings.providers.modelLabel"), |ui| {
        let show_presets = !editor.static_models.is_empty() && !editor.custom_model;
        if show_presets {
            // Tauri 会把当前值（不在清单里时）也塞进选项，否则下拉会显示空白。
            let mut options: Vec<String> = editor.static_models.clone();
            let current = editor.model.trim();
            if !current.is_empty() && !options.iter().any(|option| option == current) {
                options.push(current.to_string());
            }
            let mut pick = editor.model.clone();
            egui::ComboBox::from_id_salt("settings-provider-model-preset")
                .selected_text(if current.is_empty() {
                    &editor.default_model
                } else {
                    current
                })
                .show_ui(ui, |ui| {
                    for option in &options {
                        if ui.selectable_label(current == option, option).clicked() {
                            pick = option.clone();
                            ui.close();
                        }
                    }
                    if ui
                        .selectable_label(
                            false,
                            tr_l10n(lang, "settings.providers.customModelLabel"),
                        )
                        .clicked()
                    {
                        actions.push(FrontendAction::SettingsProviderModelCustom(true));
                        ui.close();
                    }
                });
            if pick != editor.model {
                editor_mark_dirty(ui);
                actions.push(FrontendAction::SettingsProviderField(
                    SettingsProviderField::Model,
                    pick,
                ));
            }
        } else {
            // 手输框与「回到预设列表」是上下两行（Tauri 的模型工具都在字段下方），
            // 同一行会让按钮被挤出右边界、根本画不出来。
            ui.vertical(|ui| {
                let mut draft = editor.model.clone();
                let id = egui::Id::unique("openless-settings-provider-model");
                let width = ui.available_width().min(420.0);
                if layout::text_input_sized(
                    ui,
                    &mut draft,
                    id,
                    &editor.default_model,
                    width,
                    38.0,
                    false,
                )
                .changed()
                {
                    editor_mark_dirty(ui);
                    actions.push(FrontendAction::SettingsProviderField(
                        SettingsProviderField::Model,
                        draft,
                    ));
                }
                if !editor.static_models.is_empty()
                    && provider_small_button(ui, lang, "settings.providers.presetListLabel", false)
                {
                    actions.push(FrontendAction::SettingsProviderModelCustom(false));
                }
            });
        }
        if editor.model.trim().is_empty()
            && !editor.default_model.is_empty()
            && provider_small_button(ui, lang, "settings.providers.fillDefault", false)
        {
            actions.push(FrontendAction::SettingsProviderModelSelected(
                editor.default_model.clone(),
            ));
        }
    });

    if editor.auth == SettingsProviderAuth::TencentCloud {
        ui.label(
            egui::RichText::new(tr_l10n(lang, "settings.providers.tencentCloudNote"))
                .size(11.5)
                .color(theme::INK_4),
        );
    }
    // Tauri disables catalog fetching for these ASR credential shapes.
    if matches!(
        editor.auth,
        SettingsProviderAuth::Volcengine
            | SettingsProviderAuth::Xfyun
            | SettingsProviderAuth::TencentCloud
    ) {
        return;
    }
    // 可用模型：拉取（或看文档）→ 选择 → 写入模型字段并保存。
    ui.add_space(2.0);
    if editor.has_models_url {
        if provider_small_button(ui, lang, "settings.providers.viewModels", false) {
            actions.push(FrontendAction::SettingsProviderModelsUrl);
        }
        return;
    }
    let fetch = if editor.models_loading {
        tr_l10n(lang, "settings.providers.loadingModels")
    } else {
        tr_l10n(lang, "settings.providers.fetchModels")
    };
    let fetch_width = layout::text_width(ui, fetch, 11.5) + 24.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(fetch_width, 26.0), egui::Sense::hover());
    if layout::action_button(ui, rect, fetch, None, layout::ButtonKind::Ghost).clicked() {
        actions.push(FrontendAction::SettingsProviderModels);
    }
    if editor.models.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(tr_l10n(lang, "settings.channels.availableModels"))
            .size(11.0)
            .color(theme::INK_3),
    );
    // Tauri 的「可用模型」是可搜索下拉（OrcaRouter 这类目录很大）；这里同样在下拉里
    // 带一个搜索框，并把过滤词存在 egui 自己的临时存储里。
    let filter_id = egui::Id::unique("openless-settings-provider-model-filter");
    let mut filter: String = ui.data(|data| data.get_temp(filter_id).unwrap_or_default());
    egui::ComboBox::from_id_salt("settings-provider-model-catalog")
        .selected_text(tr_l10n(lang, "settings.providers.selectModel"))
        .show_ui(ui, |ui| {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut filter)
                        .hint_text(tr_l10n(lang, "settings.providers.searchModels")),
                )
                .changed()
            {
                ui.data_mut(|data| data.insert_temp(filter_id, filter.clone()));
            }
            let needle = filter.trim().to_lowercase();
            let mut shown = 0;
            for model in &editor.models {
                if !needle.is_empty() && !model.to_lowercase().contains(&needle) {
                    continue;
                }
                shown += 1;
                if ui.selectable_label(editor.model == *model, model).clicked() {
                    actions.push(FrontendAction::SettingsProviderModelSelected(model.clone()));
                    ui.close();
                }
            }
            if shown == 0 {
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "settings.providers.noMatchingModels"))
                        .size(11.0)
                        .color(theme::INK_4),
                );
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn provider_field(
    ui: &mut egui::Ui,
    channel_id: &str,
    label: &str,
    value: &str,
    field: SettingsProviderField,
    password: bool,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    provider_field_hint(
        ui, channel_id, label, value, "", field, password, lang, actions,
    );
}

#[allow(clippy::too_many_arguments)]
fn provider_field_hint(
    ui: &mut egui::Ui,
    channel_id: &str,
    label: &str,
    value: &str,
    hint: &str,
    field: SettingsProviderField,
    password: bool,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    /// Tauri 渠道弹窗里的输入框：整列宽、38px 高。
    const MODAL_INPUT_HEIGHT: f32 = 38.0;
    let mut row = |ui: &mut egui::Ui| {
        // 草稿按「渠道 + 字段」保存：光标与编辑状态由同一个缓冲维护，不再每帧从
        // 宿主回显的值重建（那会让光标错位、吞字）。
        //
        // 早期只按字段类型做 key，没有渠道维度，于是：
        //   * 第一次渲染时若编辑器还没加载完（value 为空），这个空缓冲会被一直写回，
        //     之后加载出来的 endpoint/model 再也进不了输入框——「编辑已有渠道读不到配置」；
        //   * 编辑完 A 再编辑 B，B 显示的是 A 的文字。
        // 现在额外记一份「播种时的宿主值」：宿主值变了就重新播种，没变就保留用户输入。
        let id = egui::Id::unique((
            "openless-settings-provider-field",
            channel_id,
            format!("{field:?}"),
        ));
        let draft_id = id.with("draft");
        let seed_id = id.with("seed");
        let seed = ui.data(|data| data.get_temp::<String>(seed_id));
        let mut draft = match (ui.data(|data| data.get_temp::<String>(draft_id)), seed) {
            (Some(draft), Some(seed)) if seed == value => draft,
            _ => value.to_string(),
        };
        let width = ui.available_width().min(420.0);
        // 密钥默认打码，但可以点一下看明文（Tauri `CredentialField` 的眼睛按钮）。
        let reveal_id = id.with("reveal");
        let mut revealed = ui
            .data(|data| data.get_temp::<bool>(reveal_id))
            .unwrap_or(false);
        let response = if password {
            let mut render = |ui: &mut egui::Ui, password: bool| {
                layout::text_input_sized(
                    ui,
                    &mut draft,
                    id,
                    hint,
                    width - 44.0,
                    MODAL_INPUT_HEIGHT,
                    password,
                )
            };
            let response = render(ui, !revealed);
            ui.horizontal(|ui| {
                let label = if revealed {
                    tr_l10n(lang, "settings.providers.hideSecret")
                } else {
                    tr_l10n(lang, "settings.providers.showSecret")
                };
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new(label).size(11.0).color(theme::INK_2),
                    ))
                    .clicked()
                {
                    revealed = !revealed;
                    ui.data_mut(|data| data.insert_temp(reveal_id, revealed));
                }
            });
            response
        } else {
            layout::text_input_sized(ui, &mut draft, id, hint, width, MODAL_INPUT_HEIGHT, false)
        };
        if response.changed() {
            editor_mark_dirty(ui);
            actions.push(FrontendAction::SettingsProviderField(field, draft.clone()));
        }
        ui.data_mut(|data| {
            data.insert_temp(draft_id, draft);
            data.insert_temp(seed_id, value.to_string());
        });
    };
    if label.is_empty() {
        row(ui);
    } else {
        modal_form_row(ui, label, row);
    }
}

fn provider_small_button(ui: &mut egui::Ui, lang: Lang, key: &'static str, primary: bool) -> bool {
    let text = egui::RichText::new(tr_l10n(lang, key)).size(11.5);
    let button = if primary {
        egui::Button::new(text.color(theme::SURFACE)).fill(theme::INK)
    } else {
        egui::Button::new(text).fill(theme::SURFACE_2)
    };
    ui.add(
        button
            .stroke(if primary {
                egui::Stroke::NONE
            } else {
                egui::Stroke::new(0.8, theme::LINE)
            })
            .corner_radius(egui::CornerRadius::same(8))
            .min_size(egui::vec2(0.0, 26.0)),
    )
    .clicked()
}

/// 自动保存的临时状态：存在即「有未落盘的改动」，值是防抖截止时刻（egui 时间轴）。
fn editor_autosave_id() -> egui::Id {
    egui::Id::unique("openless-channel-editor-autosave")
}

/// Match Tauri's debounce: persist 300ms after the last edit.
fn editor_mark_dirty(ui: &egui::Ui) {
    const AUTO_SAVE_DELAY: f64 = 0.3;
    let deadline = ui.input(|input| input.time) + AUTO_SAVE_DELAY;
    ui.data_mut(|data| data.insert_temp(editor_autosave_id(), deadline));
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(300));
}

/// 到点就落盘。返回是否推了保存。
fn editor_auto_save(ui: &egui::Ui, actions: &mut Vec<FrontendAction>) -> bool {
    let Some(deadline) = ui.data(|data| data.get_temp::<f64>(editor_autosave_id())) else {
        return false;
    };
    let now = ui.input(|input| input.time);
    if now < deadline {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(
                ((deadline - now) * 1000.0).clamp(16.0, 300.0) as u64,
            ));
        return false;
    }
    ui.data_mut(|data| data.remove_temp::<f64>(editor_autosave_id()));
    actions.push(FrontendAction::SettingsProviderSave);
    true
}

/// 关闭编辑器：先把还没落盘的改动冲掉（Tauri 的 flush-on-leave），再关。
fn editor_close(ui: &egui::Ui, actions: &mut Vec<FrontendAction>) {
    if ui
        .data(|data| data.get_temp::<f64>(editor_autosave_id()))
        .is_some()
    {
        ui.data_mut(|data| data.remove_temp::<f64>(editor_autosave_id()));
        actions.push(FrontendAction::SettingsProviderSave);
    }
    actions.push(FrontendAction::SettingsProviderClose);
}

/// Tauri 的渠道编辑器在桌面端是 **embedded**：`ChannelModal` 被 portal 进右侧内容栏
/// （`.ol-settings-content-pane`），靠 `.ol-channel-dialog-embedded { position: absolute;
/// inset: 0; background: var(--ol-sidebar-bg) }` 整栏盖住——不是全窗口遮罩加居中卡片。
///
/// 所以这里是「在右栏里覆盖一层」：灰底（`--ol-sidebar-bg`）+ 头部（返回按钮 + 标题 +
/// 渠道种类徽章 + 自动保存说明）+ 中部滚动区（白底圆角表单卡：服务连接 → 供应商 →
/// 渠道名称 → 凭据 → 模型）+ 底部（删除渠道 / 完成）。
fn channel_editor_panel(
    ui: &mut egui::Ui,
    vm: &FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    /// Tauri `.ol-channel-dialog-embedded .ol-channel-dialog-header { padding: 18px 24px }`
    /// + 标题行与说明行。
    const HEADER_HEIGHT: f32 = 92.0;
    /// Tauri `.ol-channel-dialog-embedded .ol-channel-dialog-footer { padding: 16px 24px }`
    /// + 34px 按钮 + 1px 顶线。
    const FOOTER_HEIGHT: f32 = 67.0;
    /// 左右内边距（Tauri 的 `24px`）。
    const SIDE_PADDING: f32 = 24.0;
    /// `.ol-channel-dialog-embedded .ol-channel-form-sheet { padding: 18px }`。
    const SHEET_PADDING: f32 = 18.0;

    let Some(editor) = vm.provider_editor.as_ref() else {
        return;
    };
    let lang = vm.lang;
    if editor.busy {
        ui.disable();
    }
    // Tauri：字段修改后自动保存（300ms 防抖 + 失焦/离开前 flush）。这里到点就推一次
    // `SettingsProviderSave`；没到点则请求下一帧再看。
    let protocol_valid = editor.protocol.validate().is_ok();
    if protocol_valid && !editor.busy {
        editor_auto_save(ui, actions);
    }
    let edited = vm
        .channels
        .iter()
        .position(|channel| channel.id == editor.channel_id);
    let column = ui.max_rect();
    ui.painter().rect_filled(
        column,
        egui::CornerRadius {
            nw: 0,
            sw: 0,
            ne: 14,
            se: 14,
        },
        theme::RAIL_BG,
    );

    // ── 头部：返回按钮 + 标题/徽章 + 说明 ────────────────────────────────────
    let header = egui::Rect::from_min_size(
        column.min,
        egui::vec2(column.width(), HEADER_HEIGHT.min(column.height())),
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(header.left(), header.bottom() - 1.0),
            header.right_bottom(),
        ),
        egui::CornerRadius::ZERO,
        theme::LINE,
    );
    // Tauri `.ol-settings-back`：32×30、白底、0.5px 强描边、圆角 8。
    let back = egui::Rect::from_min_size(
        egui::pos2(column.left() + SIDE_PADDING, column.top() + 18.0),
        egui::vec2(32.0, 30.0),
    );
    let back_response = ui.interact(
        back,
        egui::Id::unique("openless-channel-editor-back"),
        egui::Sense::click(),
    );
    ui.painter().rect_filled(
        back,
        egui::CornerRadius::same(8),
        if back_response.hovered() {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        },
    );
    ui.painter().rect_stroke(
        back,
        egui::CornerRadius::same(8),
        egui::Stroke::new(0.8, theme::LINE_STRONG),
        egui::StrokeKind::Inside,
    );
    icons::draw_icon_sized(
        ui,
        back.center(),
        IconName::ChevronLeft,
        if back_response.hovered() {
            theme::INK
        } else {
            theme::INK_2
        },
        18.0,
    );
    if back_response.clicked() && protocol_valid {
        editor_close(ui, actions);
    }
    let heading_left = back.right() + 14.0;
    let title_painter = ui.painter().clone();
    let title_y = column.top() + 18.0;
    let title_key = if editor.is_draft {
        "settings.channels.create_title"
    } else {
        "settings.channels.edit_title"
    };
    title_painter.text(
        egui::pos2(heading_left, title_y),
        egui::Align2::LEFT_TOP,
        tr_l10n(lang, title_key),
        egui::FontId::proportional(21.0),
        theme::INK,
    );
    let title_width = layout::text_width(ui, tr_l10n(lang, title_key), 21.0);
    let kind_title = tr_l10n(
        lang,
        if editor.is_asr {
            "settings.channels.asr_title"
        } else {
            "settings.channels.llm_title"
        },
    );
    let badge_galley = title_painter.layout_no_wrap(
        kind_title.to_owned(),
        egui::FontId::proportional(10.5),
        theme::INK_3,
    );
    let badge_padding = egui::vec2(7.0, 2.0);
    let badge = egui::Rect::from_min_size(
        egui::pos2(heading_left + title_width + 12.0, title_y + 6.0),
        badge_galley.size() + badge_padding * 2.0,
    );
    title_painter.rect_stroke(
        badge,
        egui::CornerRadius::same(5),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    title_painter.galley(badge.min + badge_padding, badge_galley, theme::INK_3);
    title_painter.text(
        egui::pos2(heading_left, title_y + 32.0),
        egui::Align2::LEFT_TOP,
        tr_l10n(lang, "settings.channels.auto_save_hint"),
        egui::FontId::proportional(12.0),
        theme::INK_3,
    );

    // ── 底部：删除渠道（两步确认）+ 完成 ────────────────────────────────────
    let footer = egui::Rect::from_min_size(
        egui::pos2(
            column.left(),
            column.bottom() - FOOTER_HEIGHT.min(column.height()),
        ),
        egui::vec2(column.width(), FOOTER_HEIGHT.min(column.height())),
    );
    ui.painter()
        .rect_filled(footer, egui::CornerRadius::ZERO, theme::RAIL_BG);
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            footer.left_top(),
            egui::pos2(footer.right(), footer.top() + 1.0),
        ),
        egui::CornerRadius::ZERO,
        theme::LINE,
    );

    // ── 中部：滚动区里的白色表单卡 ─────────────────────────────────────────
    let body = egui::Rect::from_min_max(
        egui::pos2(column.left(), header.bottom()),
        egui::pos2(column.right(), footer.top()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("openless-channel-editor-body")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                let inner = egui::Rect::from_min_max(
                    egui::pos2(body.left() + SIDE_PADDING, ui.cursor().top()),
                    egui::pos2(
                        body.right() - SIDE_PADDING,
                        body.bottom().max(ui.cursor().top() + 40.0),
                    ),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                    egui::Frame::new()
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(1.0, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(14))
                        .inner_margin(egui::Margin::same(SHEET_PADDING as i8))
                        .show(ui, |ui| {
                            ui.set_width((inner.width() - SHEET_PADDING * 2.0).max(80.0));
                            provider_editor_body(
                                ui,
                                editor,
                                &vm.channel_providers,
                                edited,
                                lang,
                                vm.settings.llm_thinking_enabled,
                                actions,
                            );
                        });
                });
                ui.add_space(22.0);
            });
    });

    // ── 底部按钮 ───────────────────────────────────────────────────────────
    let confirm_id = egui::Id::unique(("settings-channel-delete", &editor.channel_id));
    let confirming = ui.data(|data| data.get_temp::<bool>(confirm_id).unwrap_or(false));
    let mut footer_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(footer.shrink2(egui::vec2(SIDE_PADDING, 16.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if let Some(index) = edited {
        if confirming {
            footer_ui.label(
                egui::RichText::new(tr_l10n(lang, "settings.channels.delete_confirm"))
                    .size(12.0)
                    .color(theme::INK_2),
            );
            footer_ui.add_space(12.0);
            if danger_button(
                &mut footer_ui,
                tr_l10n(lang, "settings.channels.confirm_delete"),
                Some(IconName::Trash),
            ) {
                actions.push(FrontendAction::SettingsChannelDelete(index));
                ui.data_mut(|data| data.remove_temp::<bool>(confirm_id));
            }
            footer_ui.add_space(8.0);
            if provider_small_button(&mut footer_ui, lang, "common.cancel", false) {
                footer_ui.data_mut(|data| data.remove_temp::<bool>(confirm_id));
            }
        } else if danger_button(
            &mut footer_ui,
            tr_l10n(lang, "settings.channels.delete"),
            Some(IconName::Trash),
        ) {
            footer_ui.data_mut(|data| data.insert_temp(confirm_id, true));
        }
    }
    if editor.busy {
        footer_ui.add_space(10.0);
        footer_ui.label(
            egui::RichText::new(tr_l10n(lang, "common.loading"))
                .size(11.5)
                .color(theme::INK_4),
        );
    }
    // 右侧组（Tauri `.ol-channel-dialog-footer-end`）：自动保存提示 + 保存/清除 + 完成。
    let hint = footer_ui.painter().layout_no_wrap(
        tr_l10n(lang, "modal.auto_save_hint").to_owned(),
        egui::FontId::proportional(11.5),
        theme::INK_3,
    );
    let done_width =
        layout::text_width(&footer_ui, tr_l10n(lang, "settings.channels.done"), 12.0) + 30.0;
    let clear_width =
        layout::text_width(&footer_ui, tr_l10n(lang, "btn.clear_secret"), 11.5) + 22.0;
    let mut cursor = footer.right() - SIDE_PADDING - done_width;
    let done_rect = egui::Rect::from_min_size(
        egui::pos2(cursor, footer.center().y - 17.0),
        egui::vec2(done_width, 34.0),
    );
    cursor -= 16.0 + clear_width;
    let clear_rect = egui::Rect::from_min_size(
        egui::pos2(cursor, footer.center().y - 14.0),
        egui::vec2(clear_width, 28.0),
    );
    cursor -= 16.0 + hint.size().x;
    if cursor > footer.left() + SIDE_PADDING + 200.0 {
        footer_ui.painter().galley(
            egui::pos2(cursor, footer.center().y - hint.size().y / 2.0),
            hint,
            theme::INK_3,
        );
    }
    let done = footer_ui.interact(
        done_rect,
        egui::Id::unique("openless-channel-editor-done"),
        egui::Sense::click(),
    );
    footer_ui.painter().rect_filled(
        done_rect,
        egui::CornerRadius::same(10),
        if done.hovered() {
            theme::BLUE_HOVER
        } else {
            theme::BLUE
        },
    );
    footer_ui.painter().text(
        done_rect.center(),
        egui::Align2::CENTER_CENTER,
        tr_l10n(lang, "settings.channels.done"),
        egui::FontId::proportional(12.0),
        theme::SURFACE,
    );
    if done.clicked() && protocol_valid {
        editor_close(ui, actions);
    }
    let clear = footer_ui.interact(
        clear_rect,
        egui::Id::unique("openless-channel-editor-clear"),
        egui::Sense::click(),
    );
    paint_ghost_small_button(
        &footer_ui,
        clear_rect,
        clear.hovered(),
        tr_l10n(lang, "btn.clear_secret"),
        11.5,
    );
    if clear.clicked() {
        actions.push(FrontendAction::SettingsProviderClearSecrets);
    }
}

/// 次要按钮（白底 + 0.8px 描边 + 圆角 8），用于弹窗底部的保存/清除。
fn paint_ghost_small_button(
    ui: &egui::Ui,
    rect: egui::Rect,
    hovered: bool,
    label: &str,
    size: f32,
) {
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(8),
        if hovered {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(0.8, theme::LINE),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(size),
        theme::INK_2,
    );
}

/// 危险按钮（Tauri `.ol-channel-delete-button`：淡红底、红描边、红字 + 垃圾桶图标）。
fn danger_button(ui: &mut egui::Ui, label: &str, icon: Option<IconName>) -> bool {
    let text_width = layout::text_width(ui, label, 12.5);
    let icon_space = if icon.is_some() { 21.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(text_width + icon_space + 24.0, 34.0),
        egui::Sense::click(),
    );
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(8),
        if response.hovered() {
            theme::DANGER_FILL_HOVER
        } else {
            theme::DANGER_FILL
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(1.0, theme::DANGER_BORDER),
        egui::StrokeKind::Inside,
    );
    let mut text_left = rect.left() + 12.0;
    if let Some(icon) = icon {
        icons::draw_icon_sized(
            ui,
            egui::pos2(text_left + 7.5, rect.center().y),
            icon,
            theme::ERR,
            15.0,
        );
        text_left += 21.0;
    }
    ui.painter().text(
        egui::pos2(text_left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        theme::ERR,
    );
    response.clicked()
}

/// 弹窗正文：Tauri 的 `.ol-channel-form-sheet`——`服务连接` 分区 + 供应商 + 名称，
/// 之后是 Core 决定形状的凭据字段与模型块。
fn provider_editor_body(
    ui: &mut egui::Ui,
    editor: &SettingsProviderEditor,
    providers: &[SettingsChannelProvider],
    _channel_index: Option<usize>,
    lang: Lang,
    llm_thinking_enabled: bool,
    actions: &mut Vec<FrontendAction>,
) {
    // 分区标题：云图标 + 「服务连接」（Tauri `ChannelSectionHeading`）。
    ui.horizontal(|ui| {
        icons::draw_icon_sized(
            ui,
            ui.cursor().min + egui::vec2(7.0, 8.0),
            IconName::Cloud,
            theme::INK_3,
            14.0,
        );
        ui.add_space(22.0);
        ui.label(
            egui::RichText::new(tr_l10n(lang, "settings.channels.connection_title"))
                .size(13.0)
                .color(theme::INK)
                .strong(),
        );
    });
    ui.add_space(6.0);
    if !providers.is_empty() {
        modal_form_row(
            ui,
            tr_l10n(lang, "settings.channels.provider_label"),
            |ui| {
                let selected = providers
                    .iter()
                    .position(|provider| provider.provider_type == editor.provider_type)
                    .unwrap_or(0);
                let mut picked = selected;
                egui::ComboBox::from_id_salt(("settings-editor-provider", &editor.channel_id))
                    .selected_text(&editor.provider)
                    .width(ui.available_width().min(420.0))
                    .show_ui(ui, |ui| {
                        for (option_index, provider) in providers.iter().enumerate() {
                            if ui
                                .selectable_label(option_index == selected, &provider.label)
                                .clicked()
                            {
                                picked = option_index;
                                ui.close();
                            }
                        }
                    });
                if picked != selected {
                    actions.push(FrontendAction::SettingsChannelProviderType {
                        channel_id: editor.channel_id.clone(),
                        provider_type: providers[picked].provider_type.clone(),
                    });
                }
            },
        );
    }
    modal_form_row(ui, tr_l10n(lang, "settings.channels.name_label"), |ui| {
        provider_field(
            ui,
            &editor.channel_id,
            "",
            &editor.name,
            SettingsProviderField::Name,
            false,
            lang,
            actions,
        );
    });
    ui.label(
        egui::RichText::new(tr_l10n(lang, "settings.channels.name_hint"))
            .size(11.5)
            .color(theme::INK_3),
    );
    ui.add_space(4.0);
    if editor.auth == SettingsProviderAuth::Siwc {
        siwc_editor(ui, editor, lang, actions);
        return;
    }
    provider_protocol_block(ui, editor, lang, actions);
    ui.label(
        egui::RichText::new(tr_l10n(lang, "providers.credentials"))
            .size(11.5)
            .color(theme::INK_3),
    );
    match editor.auth {
        SettingsProviderAuth::None => {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "providers.no_cloud_note"))
                    .size(11.5)
                    .color(theme::INK_4),
            );
        }
        SettingsProviderAuth::Siwc => unreachable!("SIWC has its own account form"),
        SettingsProviderAuth::OAuth => {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "providers.oauth_note"))
                    .size(11.5)
                    .color(theme::INK_4),
            );
        }
        SettingsProviderAuth::Volcengine => {
            let mut mode = editor.auth_mode.clone();
            modal_form_row(
                ui,
                tr_l10n(lang, "settings.providers.volcengineAuthModeLabel"),
                |ui| {
                    egui::ComboBox::from_id_salt("settings-provider-auth-mode")
                        .selected_text(&mode)
                        .show_ui(ui, |ui| {
                            for option in ["app_id_token", "api_key"] {
                                if ui.selectable_label(mode == option, option).clicked() {
                                    mode = option.to_string();
                                    ui.close();
                                }
                            }
                        });
                },
            );
            if mode != editor.auth_mode {
                editor_mark_dirty(ui);
                actions.push(FrontendAction::SettingsProviderField(
                    SettingsProviderField::AuthMode,
                    mode.clone(),
                ));
            }
            if mode == "api_key" {
                provider_field(
                    ui,
                    &editor.channel_id,
                    tr_l10n(lang, "settings.providers.volcengineApiKeyLabel"),
                    &editor.primary_secret,
                    SettingsProviderField::PrimarySecret,
                    true,
                    lang,
                    actions,
                );
            } else {
                provider_field(
                    ui,
                    &editor.channel_id,
                    tr_l10n(lang, "settings.providers.volcengineAppKeyLabel"),
                    &editor.primary_secret,
                    SettingsProviderField::PrimarySecret,
                    true,
                    lang,
                    actions,
                );
                provider_field(
                    ui,
                    &editor.channel_id,
                    tr_l10n(lang, "settings.providers.volcengineAccessKeyLabel"),
                    &editor.secondary_secret,
                    SettingsProviderField::SecondarySecret,
                    true,
                    lang,
                    actions,
                );
            }
            provider_field(
                ui,
                &editor.channel_id,
                tr_l10n(lang, "settings.providers.volcengineResourceIdLabel"),
                &editor.resource_id,
                SettingsProviderField::ResourceId,
                false,
                lang,
                actions,
            );
        }
        SettingsProviderAuth::Xfyun => {
            provider_field(
                ui,
                &editor.channel_id,
                tr_l10n(lang, "settings.providers.xfyunAppIdLabel"),
                &editor.primary_secret,
                SettingsProviderField::PrimarySecret,
                true,
                lang,
                actions,
            );
            provider_field(
                ui,
                &editor.channel_id,
                tr_l10n(lang, "settings.providers.xfyunApiKeyLabel"),
                &editor.secondary_secret,
                SettingsProviderField::SecondarySecret,
                true,
                lang,
                actions,
            );
        }
        SettingsProviderAuth::TencentCloud => {
            provider_field(
                ui,
                &editor.channel_id,
                tr_l10n(lang, "settings.providers.tencentCloudAppIdLabel"),
                &editor.resource_id,
                SettingsProviderField::ResourceId,
                false,
                lang,
                actions,
            );
            provider_field(
                ui,
                &editor.channel_id,
                tr_l10n(lang, "settings.providers.tencentCloudSecretIdLabel"),
                &editor.primary_secret,
                SettingsProviderField::PrimarySecret,
                true,
                lang,
                actions,
            );
            provider_field(
                ui,
                &editor.channel_id,
                tr_l10n(lang, "settings.providers.tencentCloudSecretKeyLabel"),
                &editor.secondary_secret,
                SettingsProviderField::SecondarySecret,
                true,
                lang,
                actions,
            );
        }
        SettingsProviderAuth::Other => {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "providers.core_note"))
                    .size(11.5)
                    .color(theme::INK_4),
            );
        }
        SettingsProviderAuth::ApiKey => {
            provider_field(
                ui,
                &editor.channel_id,
                if editor.api_key_optional {
                    tr_l10n(lang, "settings.providers.apiKeyOptionalLabel")
                } else {
                    tr_l10n(lang, "settings.providers.apiKeyLabel")
                },
                &editor.primary_secret,
                SettingsProviderField::PrimarySecret,
                true,
                lang,
                actions,
            );
            if !editor.endpoint_presets.is_empty() {
                modal_form_row(
                    ui,
                    tr_l10n(lang, "settings.providers.volcengineServiceLabel"),
                    |ui| {
                        let selected = editor.endpoint_presets.iter().position(|preset| {
                            openless_core::provider_rules::matches_endpoint_preset(
                                if editor.endpoint.trim().is_empty() {
                                    &editor.default_endpoint
                                } else {
                                    &editor.endpoint
                                },
                                &preset.endpoint,
                            )
                        });
                        egui::ComboBox::from_id_salt((
                            "provider-endpoint-preset",
                            &editor.channel_id,
                        ))
                        .selected_text(
                            selected
                                .map(|index| editor.endpoint_presets[index].name.as_str())
                                .unwrap_or(&editor.endpoint),
                        )
                        .show_ui(ui, |ui| {
                            for (index, preset) in editor.endpoint_presets.iter().enumerate() {
                                if ui
                                    .selectable_label(selected == Some(index), &preset.name)
                                    .clicked()
                                {
                                    editor_mark_dirty(ui);
                                    actions.push(FrontendAction::SettingsProviderField(
                                        SettingsProviderField::Endpoint,
                                        preset.endpoint.clone(),
                                    ));
                                    ui.close();
                                }
                            }
                        });
                    },
                );
            }
            provider_field_hint(
                ui,
                &editor.channel_id,
                tr_l10n(lang, "settings.providers.baseUrlLabel"),
                &editor.endpoint,
                &editor.default_endpoint,
                SettingsProviderField::Endpoint,
                false,
                lang,
                actions,
            );
            if editor.endpoint.trim().is_empty()
                && !editor.default_endpoint.is_empty()
                && provider_small_button(ui, lang, "settings.providers.fillDefault", false)
            {
                editor_mark_dirty(ui);
                actions.push(FrontendAction::SettingsProviderField(
                    SettingsProviderField::Endpoint,
                    editor.default_endpoint.clone(),
                ));
            }
        }
    }
    ui.add_space(6.0);
    if !editor.is_asr
        && matches!(
            editor.provider_type.as_str(),
            "custom" | "custom_responses" | "custom_messages"
        )
    {
        modal_form_row(
            ui,
            tr_l10n(lang, "settings.providers.extraHeadersLabel"),
            |ui| {
                let mut headers = editor.protocol.extra_headers.clone();
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut headers)
                            .id(egui::Id::unique("settings-provider-extra-headers"))
                            .desired_width(ui.available_width().min(420.0))
                            .desired_rows(3)
                            .hint_text(tr_l10n(lang, "settings.providers.extraHeadersPlaceholder")),
                    )
                    .changed()
                {
                    editor_mark_dirty(ui);
                    actions.push(FrontendAction::SettingsProviderField(
                        SettingsProviderField::ExtraHeaders,
                        headers,
                    ));
                }
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "providers.extra_headers_hint"))
                        .size(11.5)
                        .color(theme::INK_4),
                );
            },
        );
    }
    provider_model_block(ui, editor, lang, actions);
    if !editor.is_asr {
        let mut changed = false;
        toggle_row(
            ui,
            tr_l10n(lang, "settings.providers.thinkingModeLabel"),
            tr_l10n(lang, "settings.providers.thinkingModeHint"),
            llm_thinking_enabled,
            || changed = true,
        );
        if changed {
            actions.push(FrontendAction::SettingsToggle(SettingsField::LlmThinking));
        }
        if matches!(
            editor.provider_type.as_str(),
            "custom" | "custom_responses" | "custom_messages"
        ) {
            modal_form_row(
                ui,
                tr_l10n(lang, "settings.providers.temperatureLabel"),
                |ui| {
                    provider_field(
                        ui,
                        &editor.channel_id,
                        "",
                        &editor.protocol.temperature,
                        SettingsProviderField::Temperature,
                        false,
                        lang,
                        actions,
                    );
                    ui.label(
                        egui::RichText::new(tr_l10n(
                            lang,
                            "settings.providers.temperaturePlaceholder",
                        ))
                        .size(11.5)
                        .color(theme::INK_4),
                    );
                },
            );
        }
    }
}

/// The same non-secret accounts and conditional controls as Tauri LlmProtocolFields.
fn provider_protocol_block(
    ui: &mut egui::Ui,
    editor: &SettingsProviderEditor,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    use openless_core::llm_protocol::LlmRequestFormat;
    let draft = &editor.protocol;
    if editor.is_asr || draft.formats.is_empty() {
        return;
    }
    let format = draft.format();
    modal_form_row(
        ui,
        tr_l10n(lang, "settings.providers.requestFormatLabel"),
        |ui| {
            egui::ComboBox::from_id_salt("provider-request-format")
                .selected_text(match format {
                    LlmRequestFormat::ChatCompletions => "Chat Completions",
                    LlmRequestFormat::Responses => "Responses",
                    LlmRequestFormat::Messages => "Messages",
                })
                .show_ui(ui, |ui| {
                    for option in &draft.formats {
                        let (label, value) = match option {
                            LlmRequestFormat::ChatCompletions => {
                                ("Chat Completions", "chat_completions")
                            }
                            LlmRequestFormat::Responses => ("Responses", "responses"),
                            LlmRequestFormat::Messages => ("Messages", "messages"),
                        };
                        if ui.selectable_label(format == *option, label).clicked() {
                            editor_mark_dirty(ui);
                            actions.push(FrontendAction::SettingsProviderField(
                                SettingsProviderField::RequestFormat,
                                value.into(),
                            ));
                            ui.close();
                        }
                    }
                });
        },
    );
    if format == LlmRequestFormat::Responses {
        ui.label(
            egui::RichText::new(tr_l10n(lang, "settings.providers.responsesThinkingHint"))
                .size(11.5)
                .color(theme::INK_4),
        );
    }
    if editor.provider_type == "ark" {
        let mut changed = false;
        toggle_row(
            ui,
            tr_l10n(lang, "settings.providers.serviceTierLabel"),
            tr_l10n(lang, "settings.providers.serviceTierHint"),
            draft.values[4] == "fast",
            || changed = true,
        );
        if changed {
            editor_mark_dirty(ui);
            actions.push(FrontendAction::SettingsProviderField(
                SettingsProviderField::ServiceTier,
                if draft.values[4] == "fast" {
                    String::new()
                } else {
                    "fast".into()
                },
            ));
        }
    }
    if format == LlmRequestFormat::Messages {
        let budget = draft.values[1] == "budget";
        modal_form_row(
            ui,
            tr_l10n(lang, "settings.providers.messagesThinkingLabel"),
            |ui| {
                egui::ComboBox::from_id_salt("provider-messages-thinking")
                    .selected_text(tr_l10n(
                        lang,
                        if budget {
                            "settings.providers.thinkingBudget"
                        } else {
                            "settings.providers.thinkingAdaptive"
                        },
                    ))
                    .show_ui(ui, |ui| {
                        for (value, key) in [
                            ("adaptive", "settings.providers.thinkingAdaptive"),
                            ("budget", "settings.providers.thinkingBudget"),
                        ] {
                            if ui
                                .selectable_label(budget == (value == "budget"), tr_l10n(lang, key))
                                .clicked()
                            {
                                editor_mark_dirty(ui);
                                actions.push(FrontendAction::SettingsProviderField(
                                    SettingsProviderField::MessagesThinking,
                                    value.into(),
                                ));
                                ui.close();
                            }
                        }
                    });
            },
        );
        for (index, field, key, placeholder) in [
            (
                2,
                SettingsProviderField::MaxTokens,
                "settings.providers.maxTokensLabel",
                "8192",
            ),
            (
                3,
                SettingsProviderField::ThinkingBudget,
                "settings.providers.thinkingBudgetLabel",
                "1024",
            ),
        ] {
            if index == 3 && !budget {
                continue;
            }
            modal_form_row(ui, tr_l10n(lang, key), |ui| {
                let mut value = draft.values[index].clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut value).hint_text(placeholder))
                    .changed()
                {
                    editor_mark_dirty(ui);
                    actions.push(FrontendAction::SettingsProviderField(field, value));
                }
            });
        }
        ui.label(
            egui::RichText::new(tr_l10n(lang, "settings.providers.messagesThinkingHint"))
                .size(11.5)
                .color(theme::INK_4),
        );
    }
    if let Err(error) = draft.validate() {
        let key = match error.message.as_str() {
            "llmRequestFormatInvalid" => "settings.providers.llmRequestFormatInvalid",
            "llmThinkingModeInvalid" => "settings.providers.llmThinkingModeInvalid",
            "llmTokenLimitInvalid" => "settings.providers.llmTokenLimitInvalid",
            "llmThinkingBudgetInvalid" => "settings.providers.llmThinkingBudgetInvalid",
            "llmServiceTierInvalid" => "settings.providers.llmServiceTierInvalid",
            "llmProtocolHeaderConflict" => "settings.providers.llmProtocolHeaderConflict",
            _ => "common.operationFailed",
        };
        ui.label(
            egui::RichText::new(tr_l10n(lang, key))
                .size(12.0)
                .color(theme::ERR),
        );
    }
}

/// Tauri `.ol-channel-form-row`：`grid-template-columns: 140px minmax(0, 1fr)`，
/// 标签与控件同一行、标签顶部对齐（label `padding-top: 9px`）。
fn modal_form_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    /// Tauri 的标签列宽。
    const LABEL_WIDTH: f32 = 140.0;
    /// Tauri `gap: 8px 20px`。
    const COLUMN_GAP: f32 = 20.0;
    /// Tauri 渠道弹窗里的控件高 38，标签用 `padding-top: 9px` 与它对齐；标签格就取
    /// 同一行高，不要用 `available_height()`——在纵向布局里那可能是整屏高度，会把
    /// 这一行撑开、后面的控件被挤出可见区。
    const ROW_HEIGHT: f32 = 38.0;
    ui.horizontal(|ui| {
        let (label_rect, _) =
            ui.allocate_exact_size(egui::vec2(LABEL_WIDTH, ROW_HEIGHT), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(label_rect.left(), label_rect.top() + 9.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(13.0),
            theme::INK_2,
        );
        ui.add_space(COLUMN_GAP);
        control(ui)
    })
    .inner
}

fn link_row(
    ui: &mut egui::Ui,
    label: &str,
    button: &str,
    field: SettingsActionField,
    actions: &mut Vec<FrontendAction>,
) {
    row(ui, label, |ui| {
        if ui
            .add(
                egui::Button::new(egui::RichText::new(button).size(12.0).color(theme::INK_2))
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                    .corner_radius(egui::CornerRadius::same(6))
                    .min_size(egui::vec2(0.0, 28.0)),
            )
            .clicked()
        {
            actions.push(FrontendAction::SettingsAction(field));
        }
    });
}

// ── Card & row helpers ──────────────────────────────────────────────────────

/// A settings card. Tauri hides every `SectionDesc` (the only visible
/// description is the one under the pane title), so the second argument is a
/// hover hint attached to a 「?」 next to the card title.
pub(super) fn card(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(0.5, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(18, 18))
        .show(ui, |ui| {
            if !title.is_empty() {
                card_title(ui, title, hint);
            }
            contents(ui);
        });
    ui.add_space(16.0);
}

/// A collapsible card group (Tauri wraps 插入与剪贴板 / 启动 in a `Collapsible`).
/// The open state lives in egui memory, keyed by the group title.
pub(super) fn card_group(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    let id = egui::Id::unique(("openless-settings-group", title));
    let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(true));
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(0.5, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(18, 18))
        .show(ui, |ui| {
            let (rect, response) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::click());
            ui.painter().text(
                egui::pos2(rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(13.0),
                theme::INK,
            );
            let chevron = egui::pos2(rect.right() - 8.0, rect.center().y);
            let stroke = egui::Stroke::new(1.2, theme::INK_4);
            let dy = if open { -2.0 } else { 2.0 };
            ui.painter().line_segment(
                [egui::pos2(chevron.x - 4.0, chevron.y - dy), chevron],
                stroke,
            );
            ui.painter().line_segment(
                [chevron, egui::pos2(chevron.x + 4.0, chevron.y - dy)],
                stroke,
            );
            if response.clicked() {
                open = !open;
            }
            if open {
                ui.add_space(4.0);
                contents(ui);
            }
        });
    ui.data_mut(|data| data.insert_temp(id, open));
    ui.add_space(16.0);
}

/// A card whose header carries an action on the right (AI-services lists).
fn card_header(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    action: impl FnOnce(&mut egui::Ui),
    contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(0.5, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(18, 18))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(13.0)
                        .strong()
                        .color(theme::INK),
                );
                if !hint.is_empty() {
                    help_dot(ui, hint);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), action);
            });
            ui.add_space(6.0);
            contents(ui);
        });
    ui.add_space(16.0);
}

/// The AI-services tab strip: Tauri uses underline tabs (active = blue label +
/// blue underline, required services carry a red/yellow status dot).
fn service_tabs(
    ui: &mut egui::Ui,
    items: &[(&str, Option<egui::Color32>, bool)],
    active: usize,
    lang: Lang,
) -> Option<usize> {
    let height = 62.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.bottom() - 0.5),
            egui::pos2(rect.right(), rect.bottom() - 0.5),
        ],
        egui::Stroke::new(1.0, theme::LINE),
    );
    let width =
        (rect.width() - 4.0 * items.len().saturating_sub(1) as f32) / items.len().max(1) as f32;
    let mut x = rect.left();
    let mut clicked = None;
    for (index, (label, dot, inactive)) in items.iter().enumerate() {
        let tab = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, height));
        let response = ui.interact(
            tab,
            ui.scope_id().with(("openless-service-tab", index)),
            if *inactive {
                egui::Sense::hover()
            } else {
                egui::Sense::click()
            },
        );
        let selected = index == active;
        let color = if *inactive {
            theme::INK_4
        } else if selected {
            theme::BLUE
        } else if response.hovered() {
            theme::INK
        } else {
            theme::INK_3
        };
        let dot_width = if dot.is_some() { 12.0 } else { 0.0 };
        let galley = ui.painter().layout(
            (*label).to_owned(),
            egui::FontId::proportional(13.0),
            color,
            (width - 8.0 - dot_width).max(1.0),
        );
        let left = tab.center().x - (galley.size().x + dot_width) / 2.0;
        if let Some(color) = dot {
            ui.painter()
                .circle_filled(egui::pos2(left + 3.5, tab.top() + 19.0), 3.5, *color);
        }
        ui.painter().galley(
            egui::pos2(left + dot_width, tab.top() + 10.0),
            galley,
            color,
        );
        if *inactive {
            let label = tr_l10n(lang, "modal.service_views.inactive");
            let size = egui::vec2(
                (layout::text_width(ui, label, 10.0) + 8.0).min(width - 4.0),
                17.0,
            );
            let badge =
                egui::Rect::from_center_size(egui::pos2(tab.center().x, tab.bottom() - 15.0), size);
            ui.painter().rect_filled(badge, 4, theme::WARN_SOFT);
            ui.painter().text(
                badge.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(10.0),
                theme::WARN,
            );
            response.clone().on_hover_text(label);
        }
        if selected {
            ui.painter().line_segment(
                [
                    egui::pos2(tab.left(), tab.bottom() - 1.0),
                    egui::pos2(tab.right(), tab.bottom() - 1.0),
                ],
                egui::Stroke::new(2.0, theme::BLUE),
            );
        }
        if !*inactive && response.clicked() {
            clicked = Some(index);
        }
        x += width + 4.0;
    }
    clicked
}

fn card_title(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.horizontal(|ui| {
        let title_response = ui.label(
            // Tauri `SectionTitle`: font-size 14 / font-weight 600。
            egui::RichText::new(title)
                .size(14.0)
                .strong()
                .color(theme::INK),
        );
        if !hint.is_empty() {
            let _ = title_response.on_hover_text(hint);
        }
    });
    ui.add_space(6.0);
}

/// The small 「?」 Tauri renders next to a setting label: hover for the full
/// explanation instead of spending a permanent paragraph on it.
fn help_dot(ui: &mut egui::Ui, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    ui.painter().circle_stroke(
        rect.center(),
        7.5,
        egui::Stroke::new(0.5, theme::LINE_STRONG),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "?",
        egui::FontId::proportional(10.0),
        theme::INK_4,
    );
    response.on_hover_text(hint)
}

pub(super) fn toggle_row(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    value: bool,
    on_toggle: impl FnOnce(),
) {
    row_desc(ui, label, desc, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 20.0), egui::Sense::hover());
        if layout::toggle(ui, rect, value, label).clicked() {
            on_toggle();
        }
    });
}

fn combo_index_row(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    value: usize,
    options: &[&str],
    on_change: impl FnOnce(usize),
) {
    row_desc(ui, label, desc, |ui| {
        let mut selected = value;
        egui::ComboBox::from_id_salt(("settings", label))
            .selected_text(options.get(value).copied().unwrap_or(""))
            .show_ui(ui, |ui| {
                for (index, option) in options.iter().enumerate() {
                    let response = ui.selectable_label(selected == index, *option);
                    if response.clicked() {
                        selected = index;
                        ui.close();
                    }
                }
            });
        if selected != value {
            on_change(selected);
        }
    });
}

/// A row whose control is a segmented picker (Tauri uses one for the recording
/// mode and the selection-polish delivery).
fn segmented_row(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    options: &[&str],
    selected: usize,
    on_select: impl FnOnce(usize),
) {
    row_desc(ui, label, desc, |ui| {
        let width = layout::segmented_width(ui, options);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
        if let Some(index) = layout::segmented(ui, rect, options, selected) {
            on_select(index);
        }
    });
}

fn remote_mode_row(ui: &mut egui::Ui, lang: Lang, selected: usize, on_select: impl FnOnce(usize)) {
    row_desc(
        ui,
        tr_l10n(lang, "settings.remote_input.default_mode_label"),
        "",
        |ui| {
            let mut changed = None;
            for (index, key) in [
                "settings.remote_input.mode_toggle",
                "settings.remote_input.mode_hold",
            ]
            .iter()
            .enumerate()
            {
                let text = tr_l10n(lang, key);
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(layout::text_width(ui, text, 12.0) + 26.0, 30.0),
                    egui::Sense::click(),
                );
                let fill = if selected == index {
                    theme::BLUE
                } else {
                    theme::SURFACE_2
                };
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(8), fill);
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(8),
                    egui::Stroke::new(0.7, theme::LINE_STRONG),
                    egui::StrokeKind::Inside,
                );
                let center = rect.left_center() + egui::vec2(13.0, 0.0);
                if index == 0 {
                    ui.painter().circle_stroke(
                        center,
                        4.0,
                        egui::Stroke::new(
                            1.2,
                            if selected == index {
                                theme::SURFACE
                            } else {
                                theme::INK_2
                            },
                        ),
                    );
                    ui.painter().circle_filled(
                        center,
                        1.3,
                        if selected == index {
                            theme::SURFACE
                        } else {
                            theme::INK_2
                        },
                    );
                } else {
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(0.0, -4.0),
                            center + egui::vec2(0.0, 3.0),
                        ],
                        egui::Stroke::new(
                            1.2,
                            if selected == index {
                                theme::SURFACE
                            } else {
                                theme::INK_2
                            },
                        ),
                    );
                    ui.painter().line_segment(
                        [center, center + egui::vec2(-2.0, 2.0)],
                        egui::Stroke::new(
                            1.2,
                            if selected == index {
                                theme::SURFACE
                            } else {
                                theme::INK_2
                            },
                        ),
                    );
                    ui.painter().line_segment(
                        [center, center + egui::vec2(2.0, 2.0)],
                        egui::Stroke::new(
                            1.2,
                            if selected == index {
                                theme::SURFACE
                            } else {
                                theme::INK_2
                            },
                        ),
                    );
                }
                ui.painter().text(
                    rect.left_center() + egui::vec2(24.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    text,
                    theme::medium_font(12.0),
                    if selected == index {
                        theme::SURFACE
                    } else {
                        theme::INK
                    },
                );
                if response.clicked() {
                    changed = Some(index);
                }
            }
            if let Some(index) = changed {
                on_select(index);
            }
        },
    );
}

fn capsule_style_preview(ui: &mut egui::Ui, style: usize) {
    ui.horizontal(|ui| {
        ui.add_space(216.0);
        // Half-size Siri stage, matching Tauri's 230×90 preview. Keep its
        // aspect ratio instead of squeezing the ribbons into a pill.
        let size = if style == 0 {
            egui::vec2(230.0, 90.0)
        } else {
            egui::vec2(164.0, 42.0)
        };
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        match style {
            0 => {
                // Match the transparent, drifting spectral ribbons used by the
                // live Siri capsule instead of showing unrelated loading dots.
                let dt = ui.input(|input| input.stable_dt);
                let clock = super::siri_wgpu::tick(
                    ui.ctx(),
                    "settings-siri-preview",
                    super::siri_wgpu::SiriDrive {
                        level: 0.18,
                        ..Default::default()
                    },
                    dt,
                );
                let _ = super::siri_wgpu::paint(
                    ui,
                    rect,
                    super::siri_wgpu::SiriEffect::wave(clock.time, clock.level),
                );
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(16));
            }
            1 => {
                let pill = egui::Rect::from_center_size(rect.center(), egui::vec2(152.0, 34.0));
                ui.painter()
                    .rect_filled(pill, egui::CornerRadius::same(17), theme::SURFACE);
                ui.painter().rect_stroke(
                    pill,
                    egui::CornerRadius::same(17),
                    egui::Stroke::new(1.0, theme::LINE),
                    egui::StrokeKind::Inside,
                );
                for i in 0..5 {
                    let h = [8.0, 16.0, 22.0, 12.0, 7.0][i];
                    let x = pill.center().x + (i as f32 - 2.0) * 7.0;
                    ui.painter().line_segment(
                        [
                            egui::pos2(x, pill.center().y - h / 2.0),
                            egui::pos2(x, pill.center().y + h / 2.0),
                        ],
                        egui::Stroke::new(2.0, theme::BLUE),
                    );
                }
            }
            _ => {
                let pill = egui::Rect::from_center_size(rect.center(), egui::vec2(152.0, 34.0));
                ui.painter()
                    .rect_filled(pill, egui::CornerRadius::same(17), theme::INK);
                for i in 0..7 {
                    let h = [5.0, 10.0, 17.0, 22.0, 15.0, 8.0, 5.0][i];
                    let x = pill.center().x + (i as f32 - 3.0) * 7.0;
                    ui.painter().line_segment(
                        [
                            egui::pos2(x, pill.center().y - h / 2.0),
                            egui::pos2(x, pill.center().y + h / 2.0),
                        ],
                        egui::Stroke::new(2.0, theme::SURFACE),
                    );
                }
            }
        }
    });
}

fn text_row(ui: &mut egui::Ui, label: &str, desc: &str, value: &str) {
    row_desc(ui, label, desc, |ui| {
        ui.label(egui::RichText::new(value).size(12.0).color(theme::INK_2));
    });
}

pub(super) fn text_edit_row(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    value: &mut String,
    hint: &str,
    on_change: impl FnOnce(&str),
) {
    row_desc(ui, label, desc, |ui| {
        let response = layout::text_input(
            ui,
            value,
            egui::Id::unique(("openless-settings-text", label)),
            hint,
            ui.available_width().min(300.0),
            false,
        );
        if response.gained_focus() {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        if response.changed() {
            on_change(value);
        }
    });
}

/// 多行文本行（Tauri 的 textarea 设置项：线索词、系统提示词）。
fn text_area_row(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    value: &mut String,
    hint: &str,
    rows: usize,
    on_change: impl FnOnce(&str),
) {
    row_desc(ui, label, desc, |ui| {
        let width = ui.available_width().clamp(220.0, 520.0);
        let response = ui.add(
            egui::TextEdit::multiline(value)
                .id(egui::Id::unique(("openless-settings-textarea", label)))
                .desired_rows(rows)
                .desired_width(width)
                .hint_text(egui::RichText::new(hint).size(12.0).color(theme::INK_4))
                .font(egui::TextStyle::Body),
        );
        if response.gained_focus() {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        if response.changed() {
            on_change(value);
        }
    });
}

fn status_row(ui: &mut egui::Ui, label: &str, desc: &str, status: &str, color: egui::Color32) {
    row_desc(ui, label, desc, |ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(status).size(12.0).color(color))
                .wrap()
                .halign(egui::Align::Min),
        );
    });
}

/// A row whose right-hand control is an action button.
fn action_row(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    button: &str,
    field: SettingsActionField,
    actions: &mut Vec<FrontendAction>,
) {
    row_desc(ui, label, desc, |ui| {
        if ui
            .add(
                egui::Button::new(egui::RichText::new(button).size(11.5))
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.8, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(0.0, 26.0)),
            )
            .clicked()
        {
            actions.push(FrontendAction::SettingsAction(field));
        }
    });
}

pub(super) fn row(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    row_desc(ui, label, "", control);
}

pub(super) fn row_desc(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    // Tauri `SettingRow` 是 grid `minmax(0,200px) minmax(0,1fr)` + gap 16，控件在第二列里
    // **左对齐**（`justify-content: flex-start`），所以每一行的控件起点都固定在同一 x，
    // 而不是各自贴右边缘 —— 贴右会让不同宽度的控件彼此错开。
    const LABEL_COLUMN: f32 = 200.0;
    const COLUMN_GAP: f32 = 16.0;
    let row_width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(row_width, 46.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |row| {
            // 窄窗口下标签列最多占一半宽度，避免控件列被挤没。
            let label_width = LABEL_COLUMN.min(row_width * 0.5);
            row.allocate_ui_with_layout(
                egui::vec2(label_width, 46.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // `allocate_ui_with_layout` 只按内容实际宽度推进光标：不显式撑开
                    // 最小宽度，标签列就会塌缩成文字宽度，控件紧贴在标签后面（用户报
                    // 「都被向左对齐了」）。Tauri 的 `minmax(0,200px)` 是硬列宽。
                    ui.set_min_width(label_width);
                    if !label.is_empty() {
                        // Reserve both the help glyph and egui's inter-item gap;
                        // otherwise described rows grow the label column by 4px.
                        let help_width = if desc.is_empty() {
                            0.0
                        } else {
                            16.0 + ui.spacing().item_spacing.x
                        };
                        let text_width = (label_width - help_width).max(1.0);
                        ui.add_sized(
                            egui::vec2(text_width, 46.0),
                            egui::Label::new(
                                egui::RichText::new(label)
                                    .font(theme::medium_font(14.0))
                                    .color(theme::INK),
                            )
                            .wrap()
                            .halign(egui::Align::Min),
                        );
                    }
                    if !desc.is_empty() {
                        help_dot(ui, desc);
                    }
                },
            );
            row.add_space(COLUMN_GAP);
            control(row);
        },
    );
    let rect = ui
        .allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover())
        .0;
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(0.5, theme::LINE_SOFT),
    );
}

/// 仅供测试：直接渲染一个可折叠分组，用来验证「点标题行切换开合」。
#[cfg(test)]
pub(crate) fn test_group_toggle(ui: &mut egui::Ui) {
    card_group(ui, "group-toggle-probe", |ui| {
        ui.label("GROUPCONTENT");
    });
}

#[cfg(test)]
pub(crate) fn test_render_shortcuts(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    shortcuts(ui, vm, actions);
}

fn siwc_editor(
    ui: &mut egui::Ui,
    editor: &SettingsProviderEditor,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    let emit = |actions: &mut Vec<FrontendAction>, action: &str, id: Option<String>| {
        actions.push(FrontendAction::SettingsSiwc {
            action: action.into(),
            registration_id: id,
        })
    };
    ui.label(tr_l10n(lang, "siwc.description"));
    let Some(state) = editor.siwc.as_ref() else {
        ui.label(tr_l10n(lang, "common.loading"));
        if ui.button(tr_l10n(lang, "siwc.refreshModels")).clicked() {
            emit(actions, "refresh", None);
        }
        return;
    };
    let status = &state.status;
    let label = match status.state.as_str() {
        "connected" => "siwc.states.connected",
        "authorizing" => "siwc.states.authorizing",
        "usage_disabled" => "siwc.states.usage_disabled",
        "usage_limited" => "siwc.states.usage_limited",
        "reauthorization_required" => "siwc.states.reauthorization_required",
        _ => "siwc.states.signed_out",
    };
    ui.strong(tr_l10n(lang, label));
    egui::ComboBox::from_id_salt(("siwc-account", &editor.channel_id))
        .selected_text(
            status
                .account
                .as_ref()
                .map(|a| a.label.as_str())
                .unwrap_or(tr_l10n(lang, "siwc.account")),
        )
        .show_ui(ui, |ui| {
            for account in &status.accounts {
                if ui
                    .selectable_label(
                        status.account.as_ref().is_some_and(|a| a.id == account.id),
                        &account.label,
                    )
                    .clicked()
                {
                    emit(actions, "select", Some(account.id.clone()));
                }
            }
        });
    ui.horizontal_wrapped(|ui| {
        if status.state == "authorizing" {
            if ui.button(tr_l10n(lang, "siwc.cancel")).clicked() {
                emit(actions, "cancel", None);
            }
        } else {
            let (action, label) = if status.state == "usage_disabled" {
                ("enable_usage", tr_l10n(lang, "siwc.enableUsage"))
            } else if status.account.is_some() {
                ("sign_in", tr_l10n(lang, "siwc.reauthorize"))
            } else {
                ("sign_in", "Continue with ChatGPT")
            };
            if ui.button(label).clicked() {
                emit(actions, action, None);
            }
            if !status.accounts.is_empty() && ui.button(tr_l10n(lang, "siwc.addAccount")).clicked()
            {
                emit(actions, "add_account", None);
            }
            if status.account.is_some() && ui.button(tr_l10n(lang, "siwc.disconnect")).clicked() {
                emit(actions, "disconnect", None);
            }
        }
        if provider_small_button(
            ui,
            lang,
            "siwc.manageUsage",
            status.state == "usage_limited",
        ) {
            ui.ctx()
                .open_url(egui::OpenUrl::new_tab(openless_core::siwc::USAGE_URL));
        }
        if status.state == "usage_limited" && ui.button(tr_l10n(lang, "siwc.retry")).clicked() {
            emit(actions, "retry", None);
        }
    });
    if let Some(error) = &status.error {
        if error.code == "remote_revocation_unconfirmed" {
            ui.label(tr_l10n(lang, "siwc.remoteUnconfirmed"));
        } else {
            ui.colored_label(
                theme::INK_3,
                format!("{} ({})", tr_l10n(lang, "siwc.failed"), error.code),
            );
        }
    }
    if let Some(message) = &state.message {
        if message != "siwc.remoteUnconfirmed"
            || !status
                .error
                .as_ref()
                .is_some_and(|e| e.code == "remote_revocation_unconfirmed")
        {
            ui.label(match message.as_str() {
                "siwc.remoteUnconfirmed" => tr_l10n(lang, "siwc.remoteUnconfirmed"),
                "siwc.testSuccess" => tr_l10n(lang, "siwc.testSuccess"),
                _ => message,
            });
        }
    }
    if status.welcome_pending {
        ui.group(|ui| {
            ui.strong(tr_l10n(lang, "siwc.welcome"));
            if ui.button(tr_l10n(lang, "siwc.gotIt")).clicked() {
                emit(actions, "acknowledge", None);
            }
        });
    }
    if status.state == "connected" {
        let selected = state.models.iter().find(|m| m.slug == editor.model);
        if !editor.model.is_empty() && selected.is_none() {
            ui.label(fmt_l10n(lang, "siwc.modelUnavailable", &[&editor.model]));
        }
        if state.models.is_empty() {
            ui.label(tr_l10n(lang, "siwc.noModels"));
        }
        egui::ComboBox::from_id_salt(("siwc-model", &editor.channel_id))
            .selected_text(
                selected
                    .map(|m| m.display_name.as_str())
                    .unwrap_or(tr_l10n(lang, "siwc.selectModel")),
            )
            .show_ui(ui, |ui| {
                for model in &state.models {
                    if ui
                        .selectable_label(editor.model == model.slug, &model.display_name)
                        .clicked()
                    {
                        actions.push(FrontendAction::SettingsProviderModelSelected(
                            model.slug.clone(),
                        ));
                    }
                }
            });
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr_l10n(lang, "siwc.refreshModels")).clicked() {
                emit(actions, "refresh", None);
            }
            if ui
                .add_enabled(
                    selected.is_some(),
                    egui::Button::new(tr_l10n(lang, "siwc.test")),
                )
                .clicked()
            {
                emit(actions, "test", None);
            }
        });
        ui.label(tr_l10n(lang, "siwc.testHint"));
    }
}

#[cfg(test)]
mod tests {
    /// 点「验证」后按钮要立刻变成「正在验证…」并禁用，直到结果回包或超时。
    /// Tauri 用 `testingIds` 做同一件事（组件本地状态）。
    #[test]
    fn a_verify_click_marks_the_channel_as_testing_until_the_result_arrives() {
        let ctx = eframe::egui::Context::default();
        let channel = super::super::view_model::SettingsChannel {
            id: "channel-1".into(),
            name: "DeepSeek".into(),
            model: "deepseek-v4-flash".into(),
            provider: "DeepSeek".into(),
            provider_type: "deepseek".into(),
            is_active: true,
            enabled: true,
            last_ok: None,
            last_error: None,
            last_latency_ms: None,
            last_check_age_seconds: None,
        };
        let mut states = Vec::new();
        let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
            states.push(super::channel_testing(ui, &channel));
            super::mark_channel_testing(ui, &channel);
            states.push(super::channel_testing(ui, &channel));
        });
        output.textures_delta.clear();
        assert_eq!(
            states,
            vec![false, true],
            "the button flips to the checking state right away"
        );

        // 快照里出现新结果（或超时）就不再自称「正在验证」。
        let answered = super::super::view_model::SettingsChannel {
            last_check_age_seconds: Some(3),
            last_ok: Some(true),
            ..channel.clone()
        };
        let mut cleared = Vec::new();
        let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
            cleared.push(super::channel_testing(ui, &answered));
        });
        output.textures_delta.clear();
        assert_eq!(
            cleared,
            vec![false],
            "a fresh result restores the normal button"
        );
    }

    #[test]
    fn siri_preview_advances_and_requests_animation_frames() {
        let ctx = eframe::egui::Context::default();
        let mut previous = 0.0;
        for frame in 0..4 {
            let mut output = ctx.run_ui(
                eframe::egui::RawInput {
                    screen_rect: Some(eframe::egui::Rect::from_min_size(
                        eframe::egui::Pos2::ZERO,
                        eframe::egui::vec2(800.0, 600.0),
                    )),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ui| super::capsule_style_preview(ui, 0),
            );
            let clock = ctx
                .data(|data| {
                    data.get_temp::<super::super::siri_wgpu::SiriClock>(eframe::egui::Id::unique((
                        "openless-siri-clock",
                        "settings-siri-preview",
                    )))
                })
                .expect("preview animation clock");
            assert!(clock.time > previous);
            previous = clock.time;
            assert!(
                output.viewport_output[&eframe::egui::ViewportId::ROOT].repaint_delay
                    <= std::time::Duration::from_millis(16)
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn siri_preview_uses_half_of_the_full_stage() {
        let ctx = eframe::egui::Context::default();
        let mut output = ctx.run_ui(
            eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| super::capsule_style_preview(ui, 0),
        );
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape, eframe::egui::Shape::Callback(callback)
                if callback.rect.size() == eframe::egui::vec2(230.0, 90.0)
        )));
        output.textures_delta.clear();
    }

    use super::*;

    #[test]
    fn short_polish_threshold_row_emits_the_edited_value_only_when_changed() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: Lang::ZhCn,
            ..Default::default()
        };
        vm.settings.polish_skip_max_chars = "0".to_string();
        let mut actions = Vec::new();
        let label = tr_l10n(vm.lang, "settings.recording.polish_skip_max_chars_label");
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        for (events, expected) in [
            (Vec::new(), None),
            (
                vec![
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                    egui::Event::Text("8".to_string()),
                ],
                Some("8"),
            ),
            (Vec::new(), None),
        ] {
            actions.clear();
            let _ = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 2200.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| privacy(ui, &mut vm, &mut actions),
            );
            if let Some(expected) = expected {
                assert_eq!(vm.settings.polish_skip_max_chars, expected);
                assert_eq!(actions.len(), 1);
                assert!(
                    matches!(&actions[0], FrontendAction::SettingsText(SettingsTextField::PolishSkipMaxChars, text) if text == expected)
                );
            } else {
                assert!(actions.is_empty());
            }
            ctx.memory_mut(|memory| {
                memory.request_focus(egui::Id::unique(("openless-settings-text", label)))
            });
        }
    }

    #[test]
    fn linux_privacy_describes_session_services_without_claiming_authorization() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: Lang::En,
            application_data_directory: "/tmp/openless-user-data".into(),
            linux_access: openless_linux_egui::linux_access::LinuxAccessSnapshot {
                keyring: openless_linux_egui::linux_access::KeyringState::Ready,
                input: openless_linux_egui::linux_access::CheckState::Ready,
                network: openless_linux_egui::linux_access::CheckState::Ready,
                network_latency_ms: Some(12),
                ..Default::default()
            },
            permissions: super::super::view_model::SettingsPermissions {
                microphone: super::super::view_model::PermissionState::Granted,
                hotkey: super::super::view_model::PermissionState::Granted,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut actions = Vec::new();
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1800.0),
                )),
                ..Default::default()
            },
            |ui| privacy(ui, &mut vm, &mut actions),
        );
        let mut text = String::new();
        for clipped in &output.shapes {
            collect_shape_text(&clipped.shape, &mut text);
        }
        let text = text.replace('\n', "");
        for expected in [
            tr_l10n(Lang::En, "settings.linux_privacy.keyring_title"),
            tr_l10n(Lang::En, "settings.linux_privacy.keyring_ready"),
            tr_l10n(Lang::En, "settings.linux_privacy.mic_detected"),
            tr_l10n(Lang::En, "settings.linux_privacy.hotkey_ready"),
            "org.freedesktop.secrets",
            "/tmp/openless-user-data",
        ] {
            assert!(text.contains(expected), "missing {expected:?}: {text}");
        }
        assert!(!text.contains(tr_l10n(Lang::En, "settings.permissions.granted")));
        assert!(!text.contains(tr_l10n(Lang::En, "settings.permissions.acc_label")));
        assert!(
            actions.is_empty(),
            "rendering must not access or mutate credentials"
        );
    }

    #[test]
    fn unavailable_or_unchecked_keyrings_are_never_painted_as_ready() {
        use openless_linux_egui::linux_access::{KeyringState, LinuxAccessSnapshot};
        for (state, checking, expected) in [
            (
                KeyringState::Unknown,
                false,
                "settings.linux_privacy.not_checked",
            ),
            (
                KeyringState::Locked,
                false,
                "settings.linux_privacy.keyring_locked",
            ),
            (
                KeyringState::MissingService,
                false,
                "settings.linux_privacy.keyring_missing_service",
            ),
            (
                KeyringState::MissingCollection,
                false,
                "settings.linux_privacy.keyring_missing_collection",
            ),
            (
                KeyringState::RestartRequired,
                false,
                "settings.linux_privacy.keyring_restart",
            ),
            (
                KeyringState::TemporaryVault,
                false,
                "settings.linux_privacy.keyring_temporary",
            ),
            (KeyringState::Ready, true, "settings.permissions.checking"),
        ] {
            let ctx = egui::Context::default();
            let mut vm = FrontendViewModel {
                lang: Lang::En,
                linux_access: LinuxAccessSnapshot {
                    keyring: state,
                    checking,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut actions = Vec::new();
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 1800.0),
                    )),
                    ..Default::default()
                },
                |ui| privacy_access(ui, &mut vm, &mut actions),
            );
            let mut text = String::new();
            for clipped in &output.shapes {
                collect_shape_text(&clipped.shape, &mut text);
            }
            let text = text.replace('\n', "");
            assert!(
                text.contains(tr_l10n(Lang::En, expected)),
                "{state:?}: {text}"
            );
            assert!(
                !text.contains(tr_l10n(Lang::En, "settings.linux_privacy.keyring_ready")),
                "{state:?}: {text}"
            );
            assert!(actions.is_empty());
        }
    }

    #[test]
    fn privacy_paragraphs_keep_wrapped_lines_left_aligned() {
        let ctx = egui::Context::default();
        let output = crate::ui::frontend::run_pass(&ctx, egui::RawInput::default(), |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                privacy_note(ui, "short\na substantially longer second line");
            });
        });
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .expect("privacy paragraph must be painted");
        assert_eq!(text.galley.job.halign, egui::Align::Min);
        assert!(text.galley.rows.len() >= 2);
        assert!(text.galley.rows.iter().all(|row| row.pos.x.abs() < 0.5));
    }

    /// 速记页的快捷键卡片现在复用设置页的内联录制器：键帽在右、↻/✕ 有描边，
    /// 不再有 chevron 菜单。
    #[test]
    fn quick_note_shortcut_uses_the_inline_recorder() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel::default();
        let row = ShortcutRow::new(
            ShortcutField::QuickNote,
            "",
            "Ctrl+Shift+S".into(),
            true,
            String::new(),
        );
        let mut actions = Vec::new();
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(520.0, 180.0),
                )),
                ..Default::default()
            },
            |ui| {
                ui.set_width(480.0);
                shortcut_recorder_row(ui, &mut vm, &mut actions, &row, true);
            },
        );
        // 收集画出来的文字（`painted_text_of` 在 linux_app 的测试模块里，不可见）。
        let painted = {
            fn collect(shape: &egui::Shape, out: &mut String) {
                match shape {
                    egui::Shape::Text(text) => {
                        out.push_str(text.galley.text());
                        out.push('\n');
                    }
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            collect(shape, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut text = String::new();
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut text);
            }
            text
        };
        // 三个键帽都画出来了，顺序是 Ctrl → Shift → S。
        // 注意 `S` 也是 `Shift` 的首字母，所以按"行"比较而不是按字符查找。
        let order: Vec<&str> = painted.lines().map(str::trim).collect();
        let ctrl = order
            .iter()
            .position(|line| *line == "Ctrl")
            .expect("Ctrl chip");
        let shift = order
            .iter()
            .position(|line| *line == "Shift")
            .expect("Shift chip");
        let key = order.iter().rposition(|line| *line == "S").expect("S chip");
        assert!(
            ctrl < shift && shift < key,
            "keycaps must read left to right: {painted}"
        );
    }

    #[test]
    fn the_shortcut_section_hides_without_a_hotkey_backend() {
        // Tauri 的 `visibleSettingsSections` 只在 supportsDesktopHotkey 为真时才列出
        // 「快捷键」；没有 fcitx5 监听器时要跟着隐藏，否则用户会进到一个改不动任何
        // 东西的分区。其余分区的可见性不受该能力影响。
        for section in [
            SettingsSection::General,
            SettingsSection::Services,
            SettingsSection::Appearance,
            SettingsSection::Privacy,
            SettingsSection::Advanced,
            SettingsSection::About,
        ] {
            assert!(rail_section_visible(section, false));
            assert!(rail_section_visible(section, true));
        }
        assert!(rail_section_visible(SettingsSection::Shortcuts, true));
        assert!(!rail_section_visible(SettingsSection::Shortcuts, false));
    }

    /// Tauri 的 `SettingRow` 是 grid `minmax(0,200px) minmax(0,1fr)`：每一行的控件
    /// 都从同一条竖直线开始（第一列固定 200px，第二列里左对齐）。egui 的
    /// `allocate_ui_with_layout` 只按内容实际宽度推进光标，标签列会塌缩成文字宽度，
    /// 控件就紧贴在标签后面——用户看到的就是「都被向左对齐了」。
    #[test]
    fn setting_rows_put_every_control_on_one_column() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: Lang::ZhCn,
            active_page: crate::ui::frontend::view_model::Page::Settings,
            settings_open: true,
            settings_section: SettingsSection::General,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 1600.0),
                )),
                ..Default::default()
            },
            |ui| general(ui, &mut vm, &mut actions),
        );
        let mut columns = Vec::new();
        for clipped in &output.shapes {
            collect_toggle_columns(&clipped.shape, &mut columns);
        }
        assert!(
            columns.len() >= 3,
            "expected several toggles in the recording pane, got {columns:?}"
        );
        let first = columns[0];
        assert!(
            columns.iter().all(|x| (x - first).abs() < 0.5),
            "every toggle must start on the same column: {columns:?}"
        );
        // 固定 200px 标签列 + 16px 间距 ⇒ 控件不可能落在 200px 以内。
        assert!(
            first > 200.0,
            "the label column must be reserved: toggle at x={first}"
        );
    }

    /// 「录音与输入」页上的开关都是 36×20 的圆角矩形，把它们的左边缘收出来。
    fn collect_toggle_columns(shape: &egui::Shape, out: &mut Vec<f32>) {
        match shape {
            egui::Shape::Rect(rect)
                if (rect.rect.width() - 36.0).abs() < 0.5
                    && (rect.rect.height() - 20.0).abs() < 0.5 =>
            {
                out.push(rect.rect.left());
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_toggle_columns(shape, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn provider_editor_renders_every_auth_shape() {
        // 描述符决定渲染哪组字段：六种形态都必须能画出来，且静态渲染不得产生任何
        // 动作（否则每帧都会往宿主推重复的写入）。
        let shapes = [
            SettingsProviderAuth::None,
            SettingsProviderAuth::OAuth,
            SettingsProviderAuth::Siwc,
            SettingsProviderAuth::Volcengine,
            SettingsProviderAuth::Xfyun,
            SettingsProviderAuth::Other,
            SettingsProviderAuth::ApiKey,
        ];
        for auth in shapes {
            let editor = SettingsProviderEditor {
                siwc: None,
                channel_id: "channel".to_string(),
                is_asr: false,
                is_draft: false,
                provider: "volcengine".to_string(),
                provider_type: "volcengine".to_string(),
                endpoint_presets: Vec::new(),
                api_key_optional: false,
                default_endpoint: String::new(),
                name: "main".to_string(),
                endpoint: "https://example.invalid".to_string(),
                model: "model".to_string(),
                resource_id: "resource".to_string(),
                auth_mode: "app_id_token".to_string(),
                auth,
                primary_secret: String::new(),
                secondary_secret: String::new(),
                models: vec!["m1".to_string()],
                models_loading: false,
                static_models: Vec::new(),
                default_model: String::new(),
                has_models_url: false,
                custom_model: false,
                protocol: Default::default(),
                busy: false,
            };
            let mut actions = Vec::new();
            let ctx = egui::Context::default();
            let _ = crate::ui::frontend::run_pass(&ctx, egui::RawInput::default(), |ui| {
                provider_editor_body(ui, &editor, &[], None, Lang::ZhCn, false, &mut actions);
            });
            assert!(
                actions.is_empty(),
                "渲染 {auth:?} 形态的编辑器时不应产生动作"
            );
        }
    }

    /// 模型块照 Core descriptor 走：有 `staticModels` 时是一个带当前值的预设下拉，
    /// 有文档页时按钮变成「查看支持的模型」而不是拉取，自定义模式下能切回预设列表。
    #[test]
    fn provider_model_block_follows_the_descriptor() {
        let editor = |static_models: Vec<String>, has_models_url: bool, custom_model: bool| {
            SettingsProviderEditor {
                siwc: None,
                channel_id: "channel".to_string(),
                is_asr: false,
                is_draft: false,
                provider: "ark".to_string(),
                provider_type: "ark".to_string(),
                endpoint_presets: Vec::new(),
                api_key_optional: false,
                default_endpoint: String::new(),
                name: "main".to_string(),
                endpoint: "https://example.invalid".to_string(),
                model: "doubao-pro".to_string(),
                resource_id: String::new(),
                auth_mode: "api_key".to_string(),
                auth: SettingsProviderAuth::ApiKey,
                primary_secret: String::new(),
                secondary_secret: String::new(),
                models: vec!["doubao-lite".to_string()],
                models_loading: false,
                static_models,
                default_model: "doubao-lite".to_string(),
                has_models_url,
                custom_model,
                protocol: Default::default(),
                busy: false,
            }
        };
        let presets = vec!["doubao-lite".to_string(), "doubao-pro".to_string()];

        // 预设模式：分区标题、提示、当前值、可用模型的选择入口都在。
        let painted = painted_provider_editor(&editor(presets.clone(), false, false));
        for expected in [
            tr_l10n(Lang::ZhCn, "settings.channels.modelTitle"),
            tr_l10n(Lang::ZhCn, "settings.channels.modelHint"),
            tr_l10n(Lang::ZhCn, "settings.providers.fetchModels"),
            "doubao-pro",
        ] {
            assert!(
                painted.contains(expected),
                "missing {expected:?}: {painted}"
            );
        }

        // 服务商只提供文档页（例如火山方舟）：改成引导去看文档，说明也换成套餐提示。
        let painted = painted_provider_editor(&editor(Vec::new(), true, false));
        assert!(
            painted.contains(tr_l10n(Lang::ZhCn, "settings.providers.viewModels")),
            "{painted}"
        );
        assert!(
            painted.contains(tr_l10n(Lang::ZhCn, "settings.providers.planModelsHint")),
            "{painted}"
        );

        // 「自定义模型…」模式：手输框旁边给出回到预设列表的入口。
        let painted = painted_provider_editor(&editor(presets, false, true));
        assert!(
            painted.contains(tr_l10n(Lang::ZhCn, "settings.providers.presetListLabel")),
            "{painted}"
        );
    }

    /// Tauri 的自动保存：字段改动后防抖落盘；关闭前先把未落盘的改动冲掉。
    #[test]
    fn editor_auto_save_debounces_and_flushes_before_closing() {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let raw = |time: f64| egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 680.0),
            )),
            ..Default::default()
        };

        // 第一帧：标脏，未到防抖截止 → 不保存。
        crate::ui::frontend::run_pass(&ctx, raw(10.0), |ui| {
            editor_mark_dirty(ui);
            assert!(
                !editor_auto_save(ui, &mut actions),
                "the debounce window must not save yet"
            );
        });
        assert!(
            actions.is_empty(),
            "nothing may be saved during the debounce"
        );

        // 时间推进到截止之后 → 推一次保存，并且只推一次。
        crate::ui::frontend::run_pass(&ctx, raw(11.0), |ui| {
            assert!(
                editor_auto_save(ui, &mut actions),
                "the settled edit must be saved"
            );
            assert!(
                !editor_auto_save(ui, &mut actions),
                "an already saved edit must not save twice"
            );
        });
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], FrontendAction::SettingsProviderSave));

        // 关闭时：脏则先保存再关；干净则只关。
        actions.clear();
        crate::ui::frontend::run_pass(&ctx, raw(12.0), |ui| {
            editor_mark_dirty(ui);
        });
        crate::ui::frontend::run_pass(&ctx, raw(12.1), |ui| {
            editor_close(ui, &mut actions);
        });
        assert_eq!(
            actions.len(),
            2,
            "closing must flush the pending edit first"
        );
        assert!(matches!(actions[0], FrontendAction::SettingsProviderSave));
        assert!(matches!(actions[1], FrontendAction::SettingsProviderClose));

        actions.clear();
        crate::ui::frontend::run_pass(&ctx, raw(13.0), |ui| {
            editor_close(ui, &mut actions);
        });
        assert_eq!(actions.len(), 1, "a clean editor closes without saving");
        assert!(matches!(actions[0], FrontendAction::SettingsProviderClose));
    }

    /// 渠道编辑器在桌面端是**右栏覆盖**（Tauri 的 embedded 形态）：整栏画灰底、
    /// 头部是返回按钮而不是关闭按钮，正文是白底圆角表单卡，底部有删除与完成。
    #[test]
    fn the_channel_editor_covers_the_content_column_with_tauri_chrome() {
        use super::super::view_model::{SettingsChannel, SettingsChannelProvider};

        let ctx = egui::Context::default();
        let vm = crate::ui::frontend::FrontendViewModel {
            lang: Lang::ZhCn,
            provider_editor: Some(SettingsProviderEditor {
                siwc: None,
                channel_id: "channel".to_string(),
                is_asr: false,
                is_draft: false,
                provider: "DeepSeek".to_string(),
                provider_type: "deepseek".to_string(),
                endpoint_presets: Vec::new(),
                api_key_optional: false,
                default_endpoint: String::new(),
                name: "DeepSeek".to_string(),
                endpoint: "https://api.deepseek.com/v1".to_string(),
                model: "deepseek-v4-flash".to_string(),
                resource_id: String::new(),
                auth_mode: "api_key".to_string(),
                auth: SettingsProviderAuth::ApiKey,
                primary_secret: String::new(),
                secondary_secret: String::new(),
                models: Vec::new(),
                models_loading: false,
                static_models: Vec::new(),
                default_model: String::new(),
                has_models_url: true,
                custom_model: false,
                protocol: Default::default(),
                busy: false,
            }),
            channel_providers: vec![SettingsChannelProvider {
                provider_type: "deepseek".to_string(),
                label: "DeepSeek".to_string(),
            }],
            channels: vec![SettingsChannel {
                id: "channel".to_string(),
                name: "DeepSeek".to_string(),
                provider: "DeepSeek".to_string(),
                provider_type: "deepseek".to_string(),
                model: "deepseek-v4-flash".to_string(),
                is_active: true,
                enabled: true,
                last_ok: None,
                last_error: None,
                last_latency_ms: None,
                last_check_age_seconds: None,
            }],
            ..Default::default()
        };
        let mut actions = Vec::new();
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 680.0),
                )),
                ..Default::default()
            },
            |ui| channel_editor_panel(ui, &vm, &mut actions),
        );
        let mut text = String::new();
        for clipped in &output.shapes {
            collect_shape_text(&clipped.shape, &mut text);
        }
        let painted = |needle: &str| text.contains(needle);
        for key in [
            "settings.channels.edit_title",
            "settings.channels.llm_title",
            "settings.channels.auto_save_hint",
            "settings.channels.connection_title",
            "settings.channels.provider_label",
            "settings.channels.name_label",
            "settings.channels.name_hint",
            "settings.channels.delete",
            "settings.channels.done",
        ] {
            let expected = tr_l10n(Lang::ZhCn, key);
            assert!(
                painted(expected),
                "{key} must be painted inside the channel editor, got {text:?}"
            );
        }
        // 右栏覆盖不该出现设置页自己的标题（编辑器整栏盖住内容区）。
        let section_title = tr_l10n(Lang::ZhCn, "modal.service_views.llm");
        assert!(
            !painted(&format!("{section_title}服务")),
            "the settings section body must not show through the editor"
        );

        // 草稿模式（Tauri `isDraft`）：标题换成「添加渠道」。
        let mut draft_vm = vm;
        if let Some(editor) = draft_vm.provider_editor.as_mut() {
            editor.is_draft = true;
        }
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 680.0),
                )),
                ..Default::default()
            },
            |ui| channel_editor_panel(ui, &draft_vm, &mut actions),
        );
        let mut draft_text = String::new();
        for clipped in &output.shapes {
            collect_shape_text(&clipped.shape, &mut draft_text);
        }
        assert!(
            draft_text.contains(tr_l10n(Lang::ZhCn, "settings.channels.create_title")),
            "a draft editor is titled as a new channel, got {draft_text:?}"
        );
        assert!(
            !draft_text.contains(tr_l10n(Lang::ZhCn, "settings.channels.edit_title")),
            "a draft editor must not say it is editing an existing channel"
        );
    }

    fn painted_provider_editor(editor: &SettingsProviderEditor) -> String {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(560.0, 1200.0),
                )),
                ..Default::default()
            },
            |ui| provider_editor_body(ui, editor, &[], None, Lang::ZhCn, false, &mut actions),
        );
        let mut text = String::new();
        for clipped in &output.shapes {
            collect_shape_text(&clipped.shape, &mut text);
        }
        text
    }

    fn collect_shape_text(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                for row in &text.galley.rows {
                    for glyph in &row.glyphs {
                        if glyph.chr != '\0' {
                            out.push(glyph.chr);
                        }
                    }
                    out.push('\n');
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_shape_text(shape, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn style_pack_draft_selection_only_touches_the_draft() {
        // 草稿行的选择器必须走 StyleHotkeyDraftPack：以前传的是 draft_pack 下标，
        // 于是「＋ 添加风格快捷键 → 选另一个包」会把**已有行**的包换掉，而
        // StyleHotkeyDraftPack 从未被构造（编译告警）。
        assert!(matches!(
            style_pack_pick_action(None, 3),
            FrontendAction::StyleHotkeyDraftPack(3)
        ));
        assert!(matches!(
            style_pack_pick_action(Some(2), 3),
            FrontendAction::StyleHotkeyRepack(2, 3)
        ));
    }

    #[test]
    fn remote_certificate_details_hide_when_stopped_or_stale_and_warn_without_a_full_fingerprint() {
        let fingerprint = "ab".repeat(32);
        assert_eq!(
            remote_cert_fingerprint_state(true, false, Some(&fingerprint)),
            RemoteCertFingerprintState::Available
        );
        // 服务没在跑 / 地址已过期 → 配对码与旧地址都不能展示。
        assert_eq!(
            remote_cert_fingerprint_state(false, false, Some(&fingerprint)),
            RemoteCertFingerprintState::Hidden
        );
        assert_eq!(
            remote_cert_fingerprint_state(true, true, Some(&fingerprint)),
            RemoteCertFingerprintState::Hidden
        );
        // 在监听但指纹缺失 / 被截断 / 不是十六进制 → 必须走「不可用」告警。
        assert_eq!(
            remote_cert_fingerprint_state(true, false, None),
            RemoteCertFingerprintState::Unavailable
        );
        assert_eq!(
            remote_cert_fingerprint_state(true, false, Some("ab")),
            RemoteCertFingerprintState::Unavailable
        );
        assert_eq!(
            remote_cert_fingerprint_state(true, false, Some(&"zz".repeat(32))),
            RemoteCertFingerprintState::Unavailable
        );
    }
}
