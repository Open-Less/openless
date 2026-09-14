use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::layout;
use super::theme;
use super::view_model::{
    FrontendAction, FrontendViewModel, SettingsActionField, SettingsComboField, SettingsField,
    SettingsSection, SettingsTextField,
};

const RAIL_WIDTH: f32 = 198.0;
const SIDEBAR_RAIL_INPUT: f32 = 150.0;

#[derive(Clone, Copy)]
enum SettingsIcon {
    Settings,
    Keyboard,
    Sun,
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
            Self::Privacy => tr_l10n(lang, "modal.descriptions.privacy"),
            Self::Advanced => tr_l10n(lang, "modal.descriptions.advanced"),
            Self::About => tr_l10n(lang, "modal.descriptions.about"),
        }
    }

    fn icon(self) -> SettingsIcon {
        match self {
            Self::General => SettingsIcon::Settings,
            Self::Shortcuts => SettingsIcon::Keyboard,
            Self::Appearance => SettingsIcon::Sun,
            Self::Services => SettingsIcon::Cloud,
            Self::Privacy => SettingsIcon::Shield,
            Self::Advanced => SettingsIcon::Bolt,
            Self::About => SettingsIcon::Info,
        }
    }
}

/// Paint the in-window settings modal. Returns true when the caller should
/// close it. Actions are pushed into the provided vec.
pub fn settings_overlay(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    body: egui::Rect,
) {
    // Mask the content area (not the sidebar/titlebar) and centre the card in
    // it — the same backdrop the marketplace detail uses.
    let size = egui::vec2(
        (body.width() - 40.0).max(320.0).min(900.0),
        (body.height() - 40.0).max(280.0).min(650.0),
    );
    let center_offset = body.center() - ctx.content_rect().center();

    let backdrop_layer = egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("openless-settings-backdrop"),
    );
    ctx.layer_painter(backdrop_layer).rect_filled(
        body,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: 14,
            se: 14,
        },
        egui::Color32::from_black_alpha(56),
    );
    // Input capture so the page behind cannot be clicked while the modal is up.
    egui::Area::new(egui::Id::new("openless-settings-backdrop-input"))
        .order(egui::Order::Foreground)
        .fixed_pos(body.min)
        .default_size(body.size())
        .constrain(false)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            ui.set_max_size(body.size());
            let _ = ui.allocate_exact_size(body.size(), egui::Sense::click());
        });

    egui::Area::new(egui::Id::new("openless-settings-modal"))
        .order(egui::Order::Tooltip)
        .anchor(egui::Align2::CENTER_CENTER, center_offset)
        .constrain_to(body)
        .show(ctx, |ui| {
            ui.set_clip_rect(body.intersect(ui.clip_rect()));
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0, theme::LINE))
                .corner_radius(egui::CornerRadius::same(14))
                .shadow(egui::Shadow {
                    offset: [0, 12],
                    blur: 28,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(42),
                })
                .show(ui, |ui| {
                    ui.set_min_size(size);
                    ui.set_max_size(size);
                    let lang = vm.lang;
                    // Modal header: title, then the auto-save hint and close on
                    // the right, above the rail / content split.
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(20, 14))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(tr_l10n(lang, "nav.settings"))
                                        .size(20.0)
                                        .strong()
                                        .color(theme::INK),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new("×")
                                                        .size(20.0)
                                                        .color(theme::INK_3),
                                                )
                                                .fill(theme::SURFACE_2)
                                                .stroke(egui::Stroke::new(0.7, theme::LINE))
                                                .corner_radius(egui::CornerRadius::same(8))
                                                .min_size(egui::vec2(28.0, 28.0)),
                                            )
                                            .clicked()
                                        {
                                            actions.push(FrontendAction::CloseSettings);
                                        }
                                        ui.add_space(10.0);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "modal.auto_save_hint",
                                            ))
                                            .size(11.0)
                                            .color(theme::INK_4),
                                        );
                                    },
                                );
                            });
                        });
                    ui.separator();
                    let body_height = (size.y - 58.0).max(120.0);
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(RAIL_WIDTH, body_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_height(body_height);
                                ui.set_max_height(body_height);
                                egui::ScrollArea::vertical()
                                    .id_salt("openless-settings-rail")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_width(RAIL_WIDTH - 20.0);
                                        rail(ui, vm, actions);
                                    });
                            },
                        );
                        ui.separator();
                        ui.allocate_ui_with_layout(
                            egui::vec2((size.x - RAIL_WIDTH - 1.0).max(0.0), body_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_height(body_height);
                                panel(ui, vm, actions);
                            },
                        );
                    });
                });
        });
}

fn rail(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 14))
        .show(ui, |ui| {
            // Section search, like the Tauri rail.
            egui::Frame::new()
                .fill(theme::SURFACE_2)
                .stroke(egui::Stroke::new(0.8, theme::LINE))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.set_width(SIDEBAR_RAIL_INPUT);
                    let width = ui.available_width().max(40.0);
                    ui.add_sized(
                        [width, 20.0],
                        egui::TextEdit::singleline(&mut vm.settings_query)
                            .id(egui::Id::new("openless-settings-search"))
                            .hint_text(tr_l10n(lang, "modal.search_placeholder"))
                            // 明确文字颜色：默认的控件前景色在浅底上过淡，
                            // 看上去像「输入了但没有显示字符」。
                            .text_color(theme::INK)
                            .frame(false)
                            .vertical_align(egui::Align::Center),
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

fn rail_item(ui: &mut egui::Ui, label: &str, icon: SettingsIcon, active: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            egui::Color32::from_rgba_unmultiplied(244, 244, 245, 150),
        );
    }
    let color = if active { theme::INK } else { theme::INK_3 };
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
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.35, color);
    let point = |x: f32, y: f32| center + egui::vec2(x, y);
    match icon {
        SettingsIcon::Settings => {
            painter.circle_stroke(center, 4.2, stroke);
            for angle in [
                0.0,
                std::f32::consts::FRAC_PI_4,
                std::f32::consts::FRAC_PI_2,
                3.0 * std::f32::consts::FRAC_PI_4,
                std::f32::consts::PI,
                5.0 * std::f32::consts::FRAC_PI_4,
                3.0 * std::f32::consts::FRAC_PI_2,
                7.0 * std::f32::consts::FRAC_PI_4,
            ] {
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment([center + direction * 5.0, center + direction * 7.0], stroke);
            }
        }
        SettingsIcon::Keyboard => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(15.0, 11.0)),
                egui::CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment([point(-5.0, 2.5), point(5.0, 2.5)], stroke);
            for x in [-4.5, 0.0, 4.5] {
                painter.circle_filled(point(x, -2.0), 0.9, color);
            }
        }
        SettingsIcon::Sun => {
            painter.circle_stroke(center, 3.6, stroke);
            for angle in [
                0.0,
                std::f32::consts::FRAC_PI_2,
                std::f32::consts::PI,
                3.0 * std::f32::consts::FRAC_PI_2,
            ] {
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment([center + direction * 5.6, center + direction * 7.6], stroke);
            }
        }
        SettingsIcon::Cloud => {
            painter.circle_stroke(point(-2.6, 0.0), 4.2, stroke);
            painter.circle_stroke(point(2.7, -2.1), 4.0, stroke);
            painter.circle_stroke(point(5.5, 1.0), 3.4, stroke);
            painter.line_segment([point(-6.0, 4.0), point(5.7, 4.0)], stroke);
            painter.line_segment([point(-6.0, 4.0), point(-6.0, 2.0)], stroke);
            painter.line_segment([point(5.7, 4.0), point(6.8, 2.0)], stroke);
        }
        SettingsIcon::Shield => {
            painter.add(egui::Shape::line(
                [
                    point(0.0, 8.0),
                    point(6.0, 5.0),
                    point(6.0, -4.0),
                    point(0.0, -7.0),
                    point(-6.0, -4.0),
                    point(-6.0, 5.0),
                    point(0.0, 8.0),
                ]
                .to_vec(),
                stroke,
            ));
        }
        SettingsIcon::Bolt => {
            painter.add(egui::Shape::line(
                [
                    point(1.0, -8.0),
                    point(-5.0, 1.0),
                    point(1.0, 1.0),
                    point(-1.0, 8.0),
                    point(6.0, -1.0),
                    point(1.0, -1.0),
                    point(1.0, -8.0),
                ]
                .to_vec(),
                stroke,
            ));
        }
        SettingsIcon::Info => {
            painter.circle_stroke(center, 8.0, stroke);
            painter.line_segment([point(0.0, -1.0), point(0.0, 5.0)], stroke);
            painter.circle_filled(point(0.0, -4.0), 0.8, color);
        }
        SettingsIcon::Help => {
            painter.circle_stroke(center, 8.0, stroke);
            painter.add(egui::Shape::line(
                [
                    point(-2.2, -2.3),
                    point(-1.2, -4.0),
                    point(1.2, -4.0),
                    point(2.2, -2.2),
                    point(0.4, 0.0),
                    point(0.4, 2.0),
                ]
                .to_vec(),
                stroke,
            ));
            painter.circle_filled(point(0.4, 5.0), 0.75, color);
        }
        SettingsIcon::Document => {
            painter.rect_stroke(
                egui::Rect::from_center_size(
                    center + egui::vec2(-1.0, 0.0),
                    egui::vec2(12.0, 16.0),
                ),
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.add(egui::Shape::line(
                [point(1.0, -8.0), point(1.0, -3.0), point(6.0, -3.0)].to_vec(),
                stroke,
            ));
        }
        SettingsIcon::External => {
            painter.add(egui::Shape::line(
                vec![
                    point(-5.0, 4.0),
                    point(-5.0, 7.0),
                    point(4.0, 7.0),
                    point(4.0, -2.0),
                    point(1.0, -2.0),
                ],
                stroke,
            ));
            painter.line_segment([point(-1.0, 3.0), point(7.0, -5.0)], stroke);
            painter.add(egui::Shape::line(
                [point(3.0, -5.0), point(7.0, -5.0), point(7.0, -1.0)].to_vec(),
                stroke,
            ));
        }
    }
}

fn panel(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(24, 16))
        .show(ui, |ui| {
            {
                let style = ui.style_mut();
                style.visuals.menu_corner_radius = egui::CornerRadius::same(10);
                for widget in [
                    &mut style.visuals.widgets.inactive,
                    &mut style.visuals.widgets.hovered,
                    &mut style.visuals.widgets.active,
                    &mut style.visuals.widgets.open,
                ] {
                    widget.corner_radius = egui::CornerRadius::same(8);
                    widget.bg_stroke = egui::Stroke::new(1.0, theme::LINE);
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
                if let Some((title, description)) = detail {
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
                        vm.advanced_open = usize::MAX;
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
                            .size(20.0)
                            .strong()
                            .color(theme::INK),
                    );
                }
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(match detail {
                    Some((_, description)) => description,
                    None => vm.settings_section.description(lang),
                })
                .size(12.0)
                .color(theme::INK_3),
            );
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

/// Title + description of the open 实验与扩展 sub-page (`None` on the list page).
fn advanced_page_title(vm: &FrontendViewModel, lang: Lang) -> Option<(&'static str, &'static str)> {
    match vm.advanced_open {
        0 => Some((
            tr_l10n(lang, "settings.coding_agent.title"),
            tr_l10n(lang, "modal.advanced_pages.less_computer"),
        )),
        1 => Some((
            tr_l10n(lang, "settings.advanced.multimodal_pipeline_title"),
            tr_l10n(lang, "modal.advanced_pages.multimodal"),
        )),
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
    card(
        ui,
        tr_l10n(lang, "settings.recording.title"),
        tr_l10n(lang, "settings.recording.desc"),
        |ui| {
            text_row(
                ui,
                tr_l10n(lang, "settings.recording.hotkey_label"),
                tr_l10n(lang, "settings.recording.combo_disable_hint"),
                &vm.dictation_hotkey,
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
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.audio_cue_label"),
                tr_l10n(lang, "settings.recording.audio_cue_desc"),
                vm.settings.audio_cue,
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::AudioCue));
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
            combo_index_row(
                ui,
                tr_l10n(lang, "settings.recording.paste_shortcut_label"),
                tr_l10n(lang, "settings.recording.paste_shortcut_desc"),
                vm.settings.paste_shortcut.min(1),
                &[
                    tr_l10n(lang, "settings.recording.paste_shortcut_ctrl_v"),
                    tr_l10n(lang, "settings.recording.paste_shortcut_ctrl_shift_v"),
                ],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::PasteShortcut,
                        val,
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

    // 启动（Tauri：可折叠分组）
    card_group(
        ui,
        tr_l10n(lang, "settings.recording.startup_group_title"),
        |ui| {
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.start_minimized_label"),
                "",
                vm.settings.start_minimized,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::StartMinimized,
                    ));
                },
            );
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.startup_at_boot"),
                "",
                vm.settings.launch_at_login,
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::LaunchAtLogin));
                },
            );
            toggle_row(
                ui,
                tr_l10n(lang, "settings.recording.auto_update_check_label"),
                "",
                vm.settings.auto_update,
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::AutoUpdate));
                },
            );
        },
    );

    // 远程输入（Tauri RemoteInputSection）
    card(
        ui,
        tr_l10n(lang, "settings.remote_input.title"),
        tr_l10n(lang, "settings.remote_input.security_hint"),
        |ui| {
            toggle_row(
                ui,
                tr_l10n(lang, "settings.remote_input.enable_label"),
                tr_l10n(lang, "settings.remote_input.enable_desc"),
                vm.settings.remote_input,
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::RemoteInput));
                },
            );
            let port = vm.settings.remote_port.clone();
            text_edit_row(
                ui,
                tr_l10n(lang, "settings.remote_input.port_label"),
                "",
                &mut vm.settings.remote_port,
                "8765",
                || {
                    actions.push(FrontendAction::SettingsText(
                        SettingsTextField::RemotePort,
                        port,
                    ));
                },
            );
            combo_index_row(
                ui,
                tr_l10n(lang, "settings.remote_input.default_mode_label"),
                "",
                vm.settings.remote_default_mode,
                &[
                    tr_l10n(lang, "settings.remote_input.mode_toggle"),
                    tr_l10n(lang, "settings.remote_input.mode_hold"),
                ],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::RemoteDefaultMode,
                        val,
                    ));
                },
            );
            // 只有服务在跑时才有配对码/网址/证书指纹可展示。
            if vm.remote_running {
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
                if let Some(fingerprint) = vm.remote_cert_fingerprint.clone() {
                    action_row(
                        ui,
                        tr_l10n(lang, "settings.remote_input.cert_fingerprint_label"),
                        tr_l10n(lang, "settings.remote_input.cert_verify_hint"),
                        tr_l10n(lang, "settings.remote_input.cert_fingerprint_copy"),
                        SettingsActionField::CopyCertFingerprint,
                        actions,
                    );
                    ui.label(
                        egui::RichText::new(fingerprint)
                            .size(10.5)
                            .color(theme::INK_4),
                    );
                }
            }
        },
    );
}

fn shortcuts(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    card(
        ui,
        tr_l10n(lang, "settings.shortcuts.title"),
        tr_l10n(lang, "settings.shortcuts.desc_no_acc"),
        |ui| {
            keycap_row(
                ui,
                tr_l10n(lang, "settings.shortcuts.start_stop"),
                "",
                &vm.dictation_hotkey,
            );
            keycap_row(
                ui,
                tr_l10n(lang, "hotkey.translation"),
                "",
                &vm.translation_hotkey,
            );
            keycap_row(
                ui,
                tr_l10n(lang, "selection_ask.hotkey_title"),
                "",
                &vm.qa_hotkey,
            );
            keycap_row(
                ui,
                tr_l10n(lang, "settings.shortcuts.switch_style"),
                "",
                &vm.switch_style_hotkey,
            );
            keycap_row(
                ui,
                tr_l10n(lang, "settings.shortcuts.open_app"),
                "",
                &vm.open_app_hotkey,
            );
            keycap_row(
                ui,
                tr_l10n(lang, "settings.shortcuts.agent_voice"),
                "",
                &vm.coding_agent_hotkey,
            );
            keycap_row(ui, tr_l10n(lang, "settings.shortcuts.cancel"), "", "Esc");
        },
    );
    // 风格包直选（Tauri：标题 + 说明 + 每个风格包一枚快捷键）。
    card(
        ui,
        tr_l10n(lang, "settings.shortcuts.style_pack_title"),
        tr_l10n(lang, "settings.shortcuts.style_pack_desc"),
        |ui| {
            for row in vm.settings.style_pack_hotkeys.clone() {
                keycap_row(ui, &row.name, "", &row.hotkey);
            }
        },
    );
    // 选区工作区（Tauri：划词润色快捷键 + 交付方式）
    card(
        ui,
        tr_l10n(lang, "settings.selection_workspace.title"),
        tr_l10n(lang, "settings.selection_workspace.hint"),
        |ui| {
            keycap_row(
                ui,
                tr_l10n(lang, "settings.selection_workspace.polish_hotkey"),
                tr_l10n(lang, "settings.selection_workspace.polish_hotkey_desc"),
                &vm.selection_polish_hotkey,
            );
            segmented_row(
                ui,
                tr_l10n(lang, "settings.selection_workspace.polish_delivery"),
                "",
                &[
                    tr_l10n(lang, "settings.selection_polish.direct_replace"),
                    tr_l10n(lang, "settings.selection_polish.preview_confirm"),
                ],
                vm.settings.selection_polish_delivery.min(1),
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::SelectionPolishDelivery,
                        val,
                    ));
                },
            );
        },
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
        let mut views = Vec::new();
        if vm.multimodal_view {
            views.push(Self::Omni);
        }
        if !vm.pipeline_multimodal {
            views.push(Self::Llm);
            views.push(Self::Asr);
        }
        if vm.supports_local_asr {
            views.push(Self::Models);
        }
        views.push(Self::Connections);
        views
    }
}

fn services(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    let views = ServiceView::visible(vm);
    let active = views
        .iter()
        .position(|view| view.id() == vm.services_view)
        .unwrap_or(0);
    let items: Vec<(&str, Option<egui::Color32>)> = views
        .iter()
        .map(|view| {
            let dot = match view {
                ServiceView::Llm | ServiceView::Asr => {
                    let configured = vm.service_configured[usize::from(*view == ServiceView::Asr)];
                    Some(if configured { theme::WARN } else { theme::ERR })
                }
                _ => None,
            };
            (view.label(lang), dot)
        })
        .collect();
    if let Some(index) = service_tabs(ui, &items, active) {
        if let Some(view) = views.get(index) {
            actions.push(FrontendAction::SettingsServicesView(view.id()));
        }
    }
    ui.add_space(10.0);

    match views.get(active).copied().unwrap_or(ServiceView::Llm) {
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
                ServiceView::Omni => &[0, 1],
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
                            for (index, channel) in vm.channels.iter().enumerate() {
                                channel_row(ui, channel, index, lang, actions);
                            }
                        }
                        if vm.channel_form_open {
                            ui.add_space(8.0);
                            add_channel_form(ui, vm, actions);
                        }
                    },
                );
                if add_clicked {
                    actions.push(FrontendAction::SettingsChannelFormOpen(true));
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
    let lang = vm.lang;
    egui::Frame::new()
        .fill(theme::BLUE_SOFT)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "settings.about.local_first"))
                        .strong()
                        .color(theme::BLUE),
                );
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "settings.about.privacy_desc"))
                        .size(11.5)
                        .color(theme::INK_3),
                );
            });
        });
    ui.add_space(10.0);

    // 权限：状态全部来自宿主快照（Linux 没有系统级授权弹窗，标为「不适用」）。
    card(
        ui,
        tr_l10n(lang, "settings.permissions.title"),
        tr_l10n(lang, "settings.permissions.desc_no_acc"),
        |ui| {
            permission_row(
                ui,
                tr_l10n(lang, "settings.permissions.mic_label"),
                "",
                vm.permissions.microphone,
                lang,
            );
            permission_row(
                ui,
                tr_l10n(lang, "settings.permissions.acc_label"),
                "",
                vm.permissions.accessibility,
                lang,
            );
            permission_row(
                ui,
                tr_l10n(lang, "settings.permissions.hotkey_label"),
                "",
                vm.permissions.hotkey,
                lang,
            );
            permission_row(
                ui,
                tr_l10n(lang, "settings.permissions.network_label"),
                "",
                vm.permissions.network,
                lang,
            );
        },
    );

    // 数据存储（Tauri DataStorageSection：保留时长 / 上限 / 润色上下文 / 光标上下文）。
    card(
        ui,
        tr_l10n(lang, "settings.data_storage.title"),
        tr_l10n(lang, "settings.data_storage.desc"),
        |ui| {
            let retention = vm.settings.retention_days.clone();
            text_edit_row(
                ui,
                tr_l10n(lang, "settings.recording.history_retention_label"),
                "",
                &mut vm.settings.retention_days,
                "0",
                || {
                    actions.push(FrontendAction::SettingsText(
                        SettingsTextField::RetentionDays,
                        retention,
                    ));
                },
            );
            let entries = vm.settings.history_max_entries.clone();
            text_edit_row(
                ui,
                tr_l10n(lang, "settings.recording.history_max_entries_label"),
                "",
                &mut vm.settings.history_max_entries,
                "200",
                || {
                    actions.push(FrontendAction::SettingsText(
                        SettingsTextField::HistoryMaxEntries,
                        entries,
                    ));
                },
            );
            let window = vm.settings.polish_context_window.clone();
            text_edit_row(
                ui,
                tr_l10n(lang, "settings.recording.polish_context_window_label"),
                tr_l10n(lang, "settings.recording.polish_context_window_desc"),
                &mut vm.settings.polish_context_window,
                "0",
                || {
                    actions.push(FrontendAction::SettingsText(
                        SettingsTextField::PolishContextWindow,
                        window,
                    ));
                },
            );
        },
    );
}

fn advanced(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    // The Tauri page is a list of drill-in rows; the panel swaps to the detail
    // page (title + back button) while `advanced_open` is set.
    let rows = [
        (
            SettingsIcon::Settings,
            tr_l10n(lang, "settings.coding_agent.title"),
            tr_l10n(lang, "modal.advanced_pages.less_computer"),
        ),
        (
            SettingsIcon::Bolt,
            tr_l10n(lang, "settings.advanced.multimodal_pipeline_title"),
            tr_l10n(lang, "modal.advanced_pages.multimodal"),
        ),
        (
            SettingsIcon::Document,
            tr_l10n(lang, "settings.debug.title"),
            tr_l10n(lang, "modal.advanced_pages.debug"),
        ),
    ];
    if vm.advanced_open < rows.len() {
        let (icon, title, description) = rows[vm.advanced_open];
        let _ = icon;
        card(ui, title, description, |ui| match vm.advanced_open {
            0 => {
                toggle_row(
                    ui,
                    tr_l10n(lang, "settings.coding_agent.enable"),
                    tr_l10n(lang, "settings.coding_agent.hotkey_hint"),
                    vm.settings.less_computer,
                    || {
                        actions.push(FrontendAction::SettingsToggle(SettingsField::LessComputer));
                    },
                );
                combo_index_row(
                    ui,
                    tr_l10n(lang, "settings.coding_agent.provider"),
                    tr_l10n(lang, "settings.coding_agent.coming_soon_note"),
                    vm.settings.coding_agent_provider.min(3),
                    &["Claude Code", "OpenCode", "Codex", "dsh"],
                    |val| {
                        actions.push(FrontendAction::SettingsCombo(
                            SettingsComboField::CodingAgentProvider,
                            val,
                        ));
                    },
                );
                combo_index_row(
                    ui,
                    tr_l10n(lang, "settings.coding_console.permission_mode"),
                    "",
                    vm.settings.coding_agent_permission.min(3),
                    &[
                        tr_l10n(lang, "settings.coding_console.mode.accept_edits"),
                        tr_l10n(lang, "settings.coding_console.mode.plan"),
                        tr_l10n(lang, "settings.coding_console.mode.default"),
                        tr_l10n(lang, "settings.coding_console.mode.bypass_permissions"),
                    ],
                    |val| {
                        actions.push(FrontendAction::SettingsCombo(
                            SettingsComboField::CodingAgentPermission,
                            val,
                        ));
                    },
                );
                let model = vm.settings.coding_agent_model.clone();
                text_edit_row(
                    ui,
                    tr_l10n(lang, "settings.coding_agent.model"),
                    tr_l10n(lang, "settings.coding_agent.model_hint"),
                    &mut vm.settings.coding_agent_model,
                    tr_l10n(lang, "settings.coding_agent.model_placeholder"),
                    || {
                        actions.push(FrontendAction::SettingsText(
                            SettingsTextField::CodingAgentModel,
                            model,
                        ));
                    },
                );
                let workdir = vm.settings.coding_agent_workdir.clone();
                text_edit_row(
                    ui,
                    tr_l10n(lang, "settings.coding_console.workdir"),
                    tr_l10n(lang, "settings.coding_console.workdir_desc"),
                    &mut vm.settings.coding_agent_workdir,
                    tr_l10n(lang, "settings.coding_console.workdir_placeholder"),
                    || {
                        actions.push(FrontendAction::SettingsText(
                            SettingsTextField::CodingAgentWorkdir,
                            workdir,
                        ));
                    },
                );
                let exe = vm.settings.coding_agent_exe.clone();
                text_edit_row(
                    ui,
                    tr_l10n(lang, "settings.coding_agent.exe"),
                    "",
                    &mut vm.settings.coding_agent_exe,
                    "claude",
                    || {
                        actions.push(FrontendAction::SettingsText(
                            SettingsTextField::CodingAgentExe,
                            exe,
                        ));
                    },
                );
            }
            1 => {
                toggle_row(
                    ui,
                    tr_l10n(lang, "settings.advanced.multimodal_pipeline_label"),
                    tr_l10n(lang, "settings.advanced.multimodal_pipeline_hint"),
                    vm.settings.multimodal,
                    || {
                        actions.push(FrontendAction::SettingsToggle(SettingsField::Multimodal));
                    },
                );
            }
            _ => {
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
                let entries = vm.settings.audio_recording_max_entries.clone();
                text_edit_row(
                    ui,
                    tr_l10n(lang, "settings.recording.audio_recording_max_entries_label"),
                    tr_l10n(lang, "settings.recording.audio_recording_max_entries_desc"),
                    &mut vm.settings.audio_recording_max_entries,
                    "50",
                    || {
                        actions.push(FrontendAction::SettingsText(
                            SettingsTextField::AudioRecordingMaxEntries,
                            entries,
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
            }
        });
        return;
    }
    card(ui, "", "", |ui| {
        for (index, (icon, title, description)) in rows.iter().enumerate() {
            if drill_row(ui, *icon, title, description) {
                vm.advanced_open = index;
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
    card(ui, "", "", |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("OpenLess").size(17.0).strong());
            if vm.auto_update_capable {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(tr_l10n(
                                    lang,
                                    "settings.about.check_stable_update_btn",
                                ))
                                .size(11.5),
                            )
                            .fill(theme::SURFACE_2)
                            .stroke(egui::Stroke::new(0.8, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(0.0, 26.0)),
                        )
                        .clicked()
                    {
                        actions.push(FrontendAction::SettingsAction(
                            SettingsActionField::CheckUpdate,
                        ));
                    }
                });
            }
        });
        ui.label(
            egui::RichText::new(format!(
                "{} · v{}",
                tr_l10n(lang, "settings.about.tagline"),
                vm.version
            ))
            .size(12.0)
            .color(theme::INK_3),
        );
        if let Some(notice) = &vm.settings_notice {
            ui.label(egui::RichText::new(notice).size(11.0).color(theme::BLUE));
        }
    });
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
    // Beta 渠道（Tauri BetaChannelSection）：只在宿主支持自更新时出现。
    if vm.auto_update_capable {
        card(
            ui,
            tr_l10n(lang, "settings.about.beta_channel_label"),
            tr_l10n(lang, "settings.about.beta_channel_desc"),
            |ui| {
                toggle_row(
                    ui,
                    tr_l10n(lang, "settings.about.beta_channel_toggle_label"),
                    "",
                    vm.settings.beta_channel,
                    || {
                        actions.push(FrontendAction::SettingsToggle(SettingsField::BetaChannel));
                    },
                );
                action_row(
                    ui,
                    "",
                    "",
                    tr_l10n(lang, "settings.about.check_beta_update_btn"),
                    SettingsActionField::CheckBetaUpdate,
                    actions,
                );
            },
        );
    }
}

/// One credential channel row: name + current marker, provider/model, actions.
fn channel_row(
    ui: &mut egui::Ui,
    channel: &super::view_model::SettingsChannel,
    index: usize,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&channel.name)
                        .size(12.5)
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
                                egui::RichText::new(tr_l10n(lang, "settings.channels.current"))
                                    .size(10.0)
                                    .color(theme::BLUE),
                            );
                        });
                }
                if !channel.enabled {
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "settings.channels.disabled"))
                            .size(10.5)
                            .color(theme::INK_4),
                    );
                }
            });
            let detail = if channel.model.trim().is_empty() {
                channel.provider.clone()
            } else {
                format!("{} · {}", channel.provider, channel.model)
            };
            ui.label(egui::RichText::new(detail).size(11.0).color(theme::INK_3));
            let last_check = channel
                .last_check
                .clone()
                .unwrap_or_else(|| tr_l10n(lang, "settings.channels.not_verified").to_string());
            ui.label(
                egui::RichText::new(last_check)
                    .size(10.5)
                    .color(theme::INK_4),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(tr_l10n(lang, "settings.channels.delete")).size(11.0),
                    )
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.8, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(0.0, 24.0)),
                )
                .clicked()
            {
                actions.push(FrontendAction::SettingsChannelDelete(index));
            }
            let (switch, _) = ui.allocate_exact_size(egui::vec2(36.0, 20.0), egui::Sense::hover());
            if layout::toggle(ui, switch, channel.enabled, ("settings-channel", index)).clicked() {
                actions.push(FrontendAction::SettingsChannelToggle(index));
            }
            ui.label(
                egui::RichText::new(tr_l10n(lang, "settings.channels.enabled"))
                    .size(11.0)
                    .color(theme::INK_3),
            );
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(tr_l10n(lang, "settings.channels.verify")).size(11.0),
                    )
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.8, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(0.0, 24.0)),
                )
                .clicked()
            {
                actions.push(FrontendAction::SettingsChannelValidate(index));
            }
            ui.add_space(6.0);
        });
    });
    ui.separator();
}

/// Provider + name form used by "add channel".
fn add_channel_form(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    ui.horizontal(|ui| {
        let options: Vec<String> = vm
            .channel_providers
            .iter()
            .map(|provider| provider.label.clone())
            .collect();
        let selected = vm
            .channel_provider_index
            .min(options.len().saturating_sub(1));
        let mut new_selection = selected;
        egui::ComboBox::from_id_salt("settings-new-channel-provider")
            .selected_text(options.get(selected).cloned().unwrap_or_default())
            .show_ui(ui, |ui| {
                for (index, option) in options.iter().enumerate() {
                    if ui.selectable_label(index == selected, option).clicked() {
                        new_selection = index;
                        ui.close();
                    }
                }
            });
        if new_selection != selected {
            actions.push(FrontendAction::SettingsChannelProvider(new_selection));
        }
        let name = vm.channel_form_name.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut vm.channel_form_name)
                .id(egui::Id::new("openless-settings-channel-name"))
                .hint_text(tr_l10n(lang, "settings.channels.name_placeholder"))
                .text_color(theme::INK)
                .desired_width(200.0),
        );
        if response.changed() {
            actions.push(FrontendAction::SettingsChannelName(name));
        }
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(tr_l10n(lang, "settings.channels.create"))
                        .color(theme::SURFACE)
                        .size(11.5),
                )
                .fill(theme::INK)
                .stroke(egui::Stroke::NONE)
                .corner_radius(egui::CornerRadius::same(8))
                .min_size(egui::vec2(0.0, 26.0)),
            )
            .clicked()
        {
            actions.push(FrontendAction::SettingsChannelCreate);
        }
        if ui
            .add(
                egui::Button::new(egui::RichText::new(tr_l10n(lang, "common.cancel")).size(11.5))
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.8, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(0.0, 26.0)),
            )
            .clicked()
        {
            actions.push(FrontendAction::SettingsChannelFormOpen(false));
        }
    });
    ui.label(
        egui::RichText::new(tr_l10n(lang, "settings.channels.name_hint"))
            .size(11.0)
            .color(theme::INK_4),
    );
}

/// A row whose value is a button that opens a link / performs an action.
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

// ── Card & row helpers ──────────────────────────────────────────────────────

/// A settings card. Tauri hides every `SectionDesc` (the only visible
/// description is the one under the pane title), so the second argument is a
/// hover hint attached to a 「?」 next to the card title.
fn card(ui: &mut egui::Ui, title: &str, hint: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(18, 16))
        .show(ui, |ui| {
            if !title.is_empty() {
                card_title(ui, title, hint);
            }
            contents(ui);
        });
    ui.add_space(10.0);
}

/// A collapsible card group (Tauri wraps 插入与剪贴板 / 启动 in a `Collapsible`).
/// The open state lives in egui memory, keyed by the group title.
fn card_group(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    let id = egui::Id::new(("openless-settings-group", title));
    let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(true));
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(18, 16))
        .show(ui, |ui| {
            let (rect, response) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::click());
            ui.painter().text(
                egui::pos2(rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(13.5),
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
    ui.add_space(10.0);
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
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(13.5)
                        .strong()
                        .color(theme::INK),
                );
                if !hint.is_empty() {
                    help_dot(ui, hint);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), action);
            });
            ui.add_space(4.0);
            contents(ui);
        });
    ui.add_space(10.0);
}

/// The AI-services tab strip: Tauri uses underline tabs (active = blue label +
/// blue underline, required services carry a red/yellow status dot).
fn service_tabs(
    ui: &mut egui::Ui,
    items: &[(&str, Option<egui::Color32>)],
    active: usize,
) -> Option<usize> {
    let height = 38.0;
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
    let mut x = rect.left();
    let mut clicked = None;
    for (index, (label, dot)) in items.iter().enumerate() {
        let width =
            layout::text_width(ui, label, 13.0) + 28.0 + if dot.is_some() { 13.0 } else { 0.0 };
        let tab = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, height));
        let response = ui.interact(
            tab,
            ui.id().with(("openless-service-tab", index)),
            egui::Sense::click(),
        );
        let selected = index == active;
        let mut text_x = tab.left() + 14.0;
        if let Some(color) = dot {
            ui.painter()
                .circle_filled(egui::pos2(text_x + 3.5, tab.center().y), 3.5, *color);
            text_x += 13.0;
        }
        ui.painter().text(
            egui::pos2(text_x, tab.center().y),
            egui::Align2::LEFT_CENTER,
            *label,
            egui::FontId::proportional(13.0),
            if selected {
                theme::BLUE
            } else if response.hovered() {
                theme::INK
            } else {
                theme::INK_3
            },
        );
        if selected {
            ui.painter().line_segment(
                [
                    egui::pos2(tab.left(), tab.bottom() - 1.0),
                    egui::pos2(tab.right(), tab.bottom() - 1.0),
                ],
                egui::Stroke::new(2.0, theme::BLUE),
            );
        }
        if response.clicked() {
            clicked = Some(index);
        }
        x += width + 6.0;
    }
    clicked
}

fn card_title(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .size(13.5)
                .strong()
                .color(theme::INK),
        );
        if !hint.is_empty() {
            help_dot(ui, hint);
        }
    });
}

/// The small 「?」 Tauri renders next to a setting label: hover for the full
/// explanation instead of spending a permanent paragraph on it.
fn help_dot(ui: &mut egui::Ui, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), egui::Sense::hover());
    ui.painter()
        .circle_stroke(rect.center(), 6.5, egui::Stroke::new(0.7, theme::LINE));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "?",
        egui::FontId::proportional(9.5),
        theme::INK_4,
    );
    response.on_hover_text(hint)
}

fn toggle_row(ui: &mut egui::Ui, label: &str, desc: &str, value: bool, on_toggle: impl FnOnce()) {
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

/// A shortcut row: label on the left, key caps on the right (like the Tauri
/// settings rows, which show one bordered chip per key).
fn keycap_row(ui: &mut egui::Ui, label: &str, desc: &str, combo: &str) {
    row_desc(ui, label, desc, |ui| {
        let parts: Vec<&str> = combo
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        // right_to_left layout: draw the caps in reverse so they read left→right.
        for part in parts.iter().rev() {
            let width = layout::text_width(ui, part, 11.0) + 16.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(6), theme::SURFACE_2);
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(6),
                egui::Stroke::new(0.6, theme::LINE),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                *part,
                egui::FontId::proportional(11.0),
                theme::INK_2,
            );
        }
    });
}

fn text_row(ui: &mut egui::Ui, label: &str, desc: &str, value: &str) {
    row_desc(ui, label, desc, |ui| {
        ui.label(egui::RichText::new(value).size(12.0).color(theme::INK_2));
    });
}

fn text_edit_row(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    value: &mut String,
    hint: &str,
    on_change: impl FnOnce(),
) {
    row_desc(ui, label, desc, |ui| {
        let response = ui.add(
            egui::TextEdit::singleline(value)
                .id(egui::Id::new(("openless-settings-text", label)))
                .hint_text(hint)
                .text_color(theme::INK)
                .desired_width(ui.available_width().min(300.0)),
        );
        if response.changed() {
            on_change();
        }
    });
}

fn status_row(ui: &mut egui::Ui, label: &str, desc: &str, status: &str, color: egui::Color32) {
    row_desc(ui, label, desc, |ui| {
        ui.label(egui::RichText::new(status).size(12.0).color(color));
    });
}

/// A permission row: the host state is rendered as text, colored by severity.
fn permission_row(
    ui: &mut egui::Ui,
    label: &str,
    desc: &str,
    state: super::view_model::PermissionState,
    lang: Lang,
) {
    use super::view_model::PermissionState;
    let (text, color) = match state {
        PermissionState::Granted => (tr_l10n(lang, "settings.permissions.granted"), theme::OK),
        PermissionState::Unsupported => (
            tr_l10n(lang, "settings.permissions.not_applicable"),
            theme::INK_4,
        ),
        PermissionState::Unknown => (
            tr_l10n(lang, "settings.permissions.indeterminate"),
            theme::INK_4,
        ),
    };
    status_row(ui, label, desc, text, color);
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

fn row(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    row_desc(ui, label, "", control);
}

fn row_desc(ui: &mut egui::Ui, label: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(38.0);
        if !label.is_empty() {
            ui.label(egui::RichText::new(label).size(13.5).color(theme::INK));
        }
        if !desc.is_empty() {
            help_dot(ui, desc);
        }
        // Controls hug the right edge, like the Tauri settings rows.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
    let rect = ui
        .allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover())
        .0;
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(0.5, theme::LINE),
    );
}
