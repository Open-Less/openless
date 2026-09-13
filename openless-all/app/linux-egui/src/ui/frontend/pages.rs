use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::icons::{self, IconName};
use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel};

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut value: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        value.push('…');
    }
    value
}

// ── Style page ──────────────────────────────────────────────────────────────

pub fn style_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    if vm.style_unsupported {
        layout::unsupported_page(ui, lang, "");
        return;
    }

    let header = layout::page_header(
        ui,
        width,
        tr_l10n(lang, "style.kicker"),
        tr_l10n(lang, "style.title"),
        Some(tr_l10n(lang, "style.desc")),
    );
    let import = tr_l10n(lang, "btn.import_zip");
    let import_width = layout::text_width(ui, import, 12.5) + 46.0;
    let import_rect = egui::Rect::from_min_size(
        egui::pos2(header.right() - import_width, header.top() + 22.0),
        egui::vec2(import_width, 30.0),
    );
    if layout::action_button(
        ui,
        import_rect,
        import,
        Some(IconName::Download),
        layout::ButtonKind::Blue,
    )
    .clicked()
    {
        actions.push(FrontendAction::StyleImport);
    }
    let refresh = tr_l10n(lang, "common.refresh");
    let refresh_width = layout::text_width(ui, refresh, 12.5) + 40.0;
    let refresh_rect = egui::Rect::from_min_size(
        egui::pos2(
            import_rect.left() - 8.0 - refresh_width,
            header.top() + 22.0,
        ),
        egui::vec2(refresh_width, 30.0),
    );
    if layout::action_button(
        ui,
        refresh_rect,
        refresh,
        Some(IconName::Refresh),
        layout::ButtonKind::Ghost,
    )
    .clicked()
    {
        vm.style_notice = None;
    }
    ui.add_space(14.0);

    let mut selected_action: Option<usize> = None;
    let mut editor_prompt: Option<String> = None;
    let style_card_height = ui.available_height().max(320.0);
    layout::card_at(
        ui,
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, style_card_height)),
        |ui| {
            let raw_active = !vm.style_selection_workflow && vm.style_selected == usize::MAX;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(tr_l10n(lang, "nav.styles"))
                                .size(15.0)
                                .strong(),
                        );
                        let raw = ui.add(
                            egui::Button::new(egui::RichText::new("原文").size(11.5).color(
                                if raw_active {
                                    theme::SURFACE
                                } else {
                                    theme::INK_3
                                },
                            ))
                            .fill(if raw_active {
                                theme::BLUE
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(6))
                            .min_size(egui::vec2(52.0, 24.0)),
                        );
                        if raw.clicked() {
                            selected_action = Some(usize::MAX);
                            vm.style_selection_workflow = false;
                        }
                    });
                    ui.add_space(3.0);
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "style.desc"))
                            .size(11.5)
                            .color(theme::INK_3),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::new()
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(7, 3))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(fmt_l10n(
                                    lang,
                                    "style.pack_count",
                                    &[&vm.style_packs.len()],
                                ))
                                .size(10.5)
                                .color(theme::INK_3),
                            );
                        });
                    let tab_options = [
                        tr_l10n(lang, "style.pack.dictation_tab"),
                        tr_l10n(lang, "style.pack.selection_tab"),
                    ];
                    let tab_width = layout::segmented_width(ui, &tab_options);
                    let (tab_rect, _) =
                        ui.allocate_exact_size(egui::vec2(tab_width, 26.0), egui::Sense::hover());
                    let selected_tab = usize::from(vm.style_selection_workflow);
                    if let Some(index) = layout::segmented(ui, tab_rect, &tab_options, selected_tab)
                    {
                        vm.style_selection_workflow = index == 1;
                    }
                });
            });
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(16.0);

            egui::ScrollArea::vertical()
                .id_salt("style-packs-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let grid_width = ui.available_width();
                    let gap = 12.0;
                    let columns = if grid_width >= 820.0 {
                        3
                    } else if grid_width >= 560.0 {
                        2
                    } else {
                        1
                    };
                    let card_width =
                        ((grid_width - gap * (columns - 1) as f32) / columns as f32).max(1.0);
                    let total_tiles = vm.style_packs.len() + 1;
                    for (row_index, start) in (0..total_tiles).step_by(columns).enumerate() {
                        if row_index > 0 {
                            ui.add_space(12.0);
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for slot in start..(start + columns).min(total_tiles) {
                                if slot == vm.style_packs.len() {
                                    if new_style_pack_card(ui, lang, egui::vec2(card_width, 232.0)) {
                                        editor_prompt = Some(
                                            "# 角色\n你是 OpenLess 的润色助手。\n\n# 任务\n把输入整理成自然、清晰、可直接使用的文字。\n\n# 输出\n只输出最终文本，不添加解释。\n".into(),
                                        );
                                    }
                                    continue;
                                }
                                let pack = &vm.style_packs[slot];
                                match style_pack_card(
                                    ui,
                                    lang,
                                    egui::vec2(card_width, 232.0),
                                    slot,
                                    vm.style_selected,
                                    &pack.name,
                                    &pack.description,
                                    &pack.tags,
                                    pack.accent,
                                    pack.is_active,
                                    pack.is_builtin,
                                ) {
                                    StyleCardAction::Activate => {
                                        selected_action = Some(slot)
                                    }
                                    StyleCardAction::Export => {
                                        actions.push(FrontendAction::StyleExport(slot));
                                    }
                                    StyleCardAction::Edit => {
                                        editor_prompt = Some(format!(
                                            "# 角色\n你是 OpenLess 的{}助手。\n\n# 任务\n把输入整理成自然、清晰、可直接使用的文字。\n\n# 输出\n只输出最终文本，不添加解释。\n",
                                            pack.name
                                        ));
                                    }
                                    StyleCardAction::None => {}
                                }
                                if slot + 1 < (start + columns).min(total_tiles) {
                                    ui.add_space(gap);
                                }
                            }
                        });
                    }
                });
        },
    );

    if let Some(index) = selected_action {
        if index == usize::MAX {
            vm.style_selected = usize::MAX;
            vm.style_notice = Some(fmt_l10n(
                lang,
                "status.style_switched",
                &[&tr_l10n(lang, "overview.mode_raw")],
            ));
        } else if let Some(pack) = vm.style_packs.get(index) {
            actions.push(FrontendAction::StyleActivate(index));
            vm.style_notice = Some(fmt_l10n(lang, "status.style_switched", &[&pack.name]));
        }
    }
    if let Some(prompt) = editor_prompt {
        vm.style_prompt = prompt;
        vm.style_editor_open = true;
    }

    if let Some(notice) = vm.style_notice.clone() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("✓").color(theme::OK).strong());
            ui.label(egui::RichText::new(notice).size(11.5).color(theme::INK_2));
            if ui.small_button("×").clicked() {
                vm.style_notice = None;
            }
        });
    }
    style_editor_overlay(ui.ctx(), vm, actions);
}

// ── Vocab helpers ───────────────────────────────────────────────────────────

pub fn correction_chip(ui: &mut egui::Ui, label: &str, enabled: bool) -> (bool, bool) {
    let fill = if enabled {
        theme::SURFACE
    } else {
        theme::SURFACE_2
    };
    let text_color = if enabled { theme::INK } else { theme::INK_4 };
    let text_galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.5),
        text_color,
    );
    let close_size = 22.0;
    let width = 12.0 + text_galley.size().x + 8.0 + close_size + 10.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(16), fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(16),
        egui::Stroke::new(0.6, theme::LINE),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        egui::pos2(
            rect.left() + 12.0,
            rect.center().y - text_galley.size().y / 2.0,
        ),
        text_galley,
        text_color,
    );

    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 10.0 - close_size / 2.0, rect.center().y),
        egui::vec2(close_size, close_size),
    );
    painter.circle_filled(close_rect.center(), close_size / 2.0, theme::SURFACE_2);
    painter.circle_stroke(
        close_rect.center(),
        close_size / 2.0,
        egui::Stroke::new(0.5, theme::LINE),
    );
    let center = close_rect.center();
    let x_stroke = egui::Stroke::new(1.1, theme::INK_4);
    painter.line_segment(
        [
            center + egui::vec2(-3.0, -3.0),
            center + egui::vec2(3.0, 3.0),
        ],
        x_stroke,
    );
    painter.line_segment(
        [
            center + egui::vec2(3.0, -3.0),
            center + egui::vec2(-3.0, 3.0),
        ],
        x_stroke,
    );

    if response.clicked() {
        if response
            .interact_pointer_pos()
            .is_some_and(|pointer| close_rect.contains(pointer))
        {
            return (false, true);
        }
        return (true, false);
    }
    (false, false)
}

// ── Style helpers ───────────────────────────────────────────────────────────

enum StyleCardAction {
    None,
    Activate,
    Export,
    Edit,
}

fn style_pack_card(
    ui: &mut egui::Ui,
    lang: Lang,
    size: egui::Vec2,
    index: usize,
    selected: usize,
    name: &str,
    description: &str,
    tags: &[String],
    accent: egui::Color32,
    is_active: bool,
    is_builtin: bool,
) -> StyleCardAction {
    let active = is_active || selected == index;
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(14),
        if active || response.hovered() {
            theme::BLUE_SOFT
        } else {
            theme::SURFACE
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(
            if active { 1.5 } else { 1.0 },
            if active { theme::BLUE } else { theme::LINE },
        ),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(16.0);
    let mut action = StyleCardAction::None;
    let mut card_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let ui = &mut card_ui;
    ui.style_mut().interaction.selectable_labels = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(name)
                .size(14.0)
                .strong()
                .color(theme::INK),
        );
        ui.add_space(8.0);
        if is_builtin {
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(0.7, accent))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "style.pack.builtin"))
                            .size(10.5)
                            .color(accent),
                    );
                });
        }
        if active {
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(theme::INK)
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "style.pack.current"))
                            .size(10.5)
                            .strong()
                            .color(theme::SURFACE),
                    );
                });
        }
    });
    ui.add_space(9.0);
    let description_width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(description_width, 48.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(truncate_text(description, 96))
                        .size(11.5)
                        .color(theme::INK_3),
                )
                .wrap(),
            );
        },
    );
    ui.add_space(9.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (tag_index, tag) in tags.iter().enumerate() {
            egui::Frame::new()
                .fill(if tag_index == 0 {
                    theme::BLUE_SOFT
                } else {
                    theme::SURFACE_2
                })
                .stroke(egui::Stroke::new(
                    0.5,
                    if tag_index == 0 { accent } else { theme::LINE },
                ))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(tag.as_str())
                            .size(10.5)
                            .color(if tag_index == 0 { accent } else { theme::INK_3 }),
                    );
                });
        }
    });
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let activate = ui.add_enabled(
                !active,
                egui::Button::new(egui::RichText::new(tr_l10n(lang, "btn.activate")).size(10.5))
                    .fill(if active { theme::INK } else { theme::BLUE })
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(7))
                    .min_size(egui::vec2(64.0, 24.0)),
            );
            if activate.clicked() {
                action = StyleCardAction::Activate;
            }
            let export = ui.add(
                egui::Button::new(egui::RichText::new(tr_l10n(lang, "btn.export_zip")).size(10.5))
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.7, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(7))
                    .min_size(egui::vec2(64.0, 24.0)),
            );
            if export.clicked() {
                action = StyleCardAction::Export;
            }
            let edit = ui.add_enabled(
                false,
                egui::Button::new(egui::RichText::new(tr_l10n(lang, "btn.edit")).size(10.5))
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.7, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(7))
                    .min_size(egui::vec2(64.0, 24.0)),
            );
            if edit.clicked() {
                action = StyleCardAction::Edit;
            }
        });
    });
    action
}

fn new_style_pack_card(ui: &mut egui::Ui, lang: Lang, size: egui::Vec2) -> bool {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(14),
        if response.hovered() {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let center = rect.center() - egui::vec2(0.0, 20.0);
    ui.painter().circle_filled(center, 22.0, theme::SURFACE_2);
    let stroke = egui::Stroke::new(1.5, theme::BLUE);
    ui.painter().line_segment(
        [center - egui::vec2(8.0, 0.0), center + egui::vec2(8.0, 0.0)],
        stroke,
    );
    ui.painter().line_segment(
        [center - egui::vec2(0.0, 8.0), center + egui::vec2(0.0, 8.0)],
        stroke,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 22.0),
        egui::Align2::CENTER_CENTER,
        tr_l10n(lang, "btn.new_style"),
        egui::FontId::proportional(14.0),
        theme::INK_2,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 45.0),
        egui::Align2::CENTER_CENTER,
        tr_l10n(lang, "style.new_pack_hint"),
        egui::FontId::proportional(11.0),
        theme::INK_4,
    );
    response.clicked()
}

pub fn style_editor_overlay(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    if !vm.style_editor_open {
        return;
    }
    let lang = vm.lang;
    let mut open = true;
    egui::Window::new(tr_l10n(lang, "head.style_pack_editor"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(620.0)
        .default_height(520.0)
        .show(ctx, |ui| {
            let style_name = if let Some(pack) = vm.style_packs.get(vm.style_selected) {
                pack.name.clone()
            } else {
                tr_l10n(lang, "btn.new_style").into()
            };
            ui.label(egui::RichText::new(style_name).size(18.0).strong());
            ui.label(
                egui::RichText::new(tr_l10n(lang, "lbl.style_note"))
                    .size(11.5)
                    .color(theme::INK_3),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "lbl.description"))
                        .size(12.0)
                        .strong(),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut vm.style_prompt)
                        .hint_text(tr_l10n(lang, "style.pack.new_description"))
                        .desired_width(390.0),
                );
            });
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(tr_l10n(lang, "style.pack.dictation_prompt_title"))
                    .size(12.0)
                    .strong(),
            );
            ui.add(
                egui::TextEdit::multiline(&mut vm.style_prompt)
                    .desired_rows(14)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(tr_l10n(lang, "style.custom_prompt_save"))
                            .fill(theme::BLUE)
                            .corner_radius(egui::CornerRadius::same(7)),
                    )
                    .clicked()
                {
                    let prompt = vm.style_prompt.clone();
                    actions.push(FrontendAction::StyleSaveEditor(prompt));
                    vm.style_editor_open = false;
                }
                if ui.button(tr_l10n(lang, "btn.reset_builtin")).clicked() {
                    vm.style_notice = None;
                }
                if ui.button(tr_l10n(lang, "common.cancel")).clicked() {
                    vm.style_editor_open = false;
                }
            });
        });
    if !open {
        vm.style_editor_open = false;
    }
}
