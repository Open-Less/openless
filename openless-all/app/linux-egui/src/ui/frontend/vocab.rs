//! Dictionary (词典) page — port of the Tauri `pages/Vocab.tsx`.
//!
//! Layout: page header with a primary "new word" action, an icon tab row
//! (all / auto-collected / manual) with a select-all checkbox and a circular
//! search control, the word list, then the quick-add row, hint and the
//! scenario presets. Correction rules live on `corrections.rs`.

use std::collections::BTreeSet;

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n};

use super::icons::{self, IconName};
use super::layout::{self, ButtonKind};
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, VocabEntry};

const GAP: f32 = 14.0;
const CARD_PADDING: f32 = 20.0;
const INPUT_ID: &str = "openless-vocab-input";
const SEARCH_ID: &str = "openless-vocab-search";
const SEARCH_INPUT_ID: &str = "openless-vocab-search-input";
const SELECTION_ID: &str = "openless-vocab-selection";
const TAB_HEIGHT: f32 = 30.0;
const SEARCH_WIDTH: f32 = 210.0;

// ── Page-local UI state ─────────────────────────────────────────────────────
//
// Search expansion and the multi-select set are view concerns only: the host
// view model has no fields for them, so they live in egui memory.

fn search_open(ctx: &egui::Context) -> bool {
    ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::new(SEARCH_ID))
            .unwrap_or(false)
    })
}

fn set_search_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(SEARCH_ID), open));
}

fn selection(ctx: &egui::Context) -> BTreeSet<usize> {
    ctx.data(|data| {
        data.get_temp::<BTreeSet<usize>>(egui::Id::new(SELECTION_ID))
            .unwrap_or_default()
    })
}

fn set_selection(ctx: &egui::Context, selection: BTreeSet<usize>) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(SELECTION_ID), selection));
}

// ── Entry point ─────────────────────────────────────────────────────────────

pub fn page(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    if vm.vocab_unsupported {
        layout::unsupported_page(ui, lang, tr_l10n(lang, "nav.vocab"));
        return;
    }

    let selected = selection(ui.ctx());
    let header = layout::page_header(
        ui,
        width,
        tr_l10n(lang, "vocab.kicker"),
        tr_l10n(lang, "vocab.title"),
        Some(tr_l10n(lang, "vocab.desc")),
    );
    let mut right = header.right();
    // Primary "new word" action (dark solid, like the Tauri `variant=primary`).
    let new_word = tr_l10n(lang, "vocab.new_word");
    let new_word_width = layout::text_width(ui, new_word, 12.5) + 42.0;
    let new_word_rect = egui::Rect::from_min_size(
        egui::pos2(right - new_word_width, header.top() + 22.0),
        egui::vec2(new_word_width, 30.0),
    );
    right = new_word_rect.left() - 8.0;
    if primary_button(ui, new_word_rect, new_word, Some(IconName::Hash)).clicked() {
        ui.memory_mut(|memory| memory.request_focus(egui::Id::new(INPUT_ID)));
    }
    // Batch delete appears only while something is selected.
    if !selected.is_empty() {
        let label = fmt_l10n(lang, "vocab.delete_selected", &[&selected.len()]);
        let label_width = layout::text_width(ui, &label, 12.5) + 40.0;
        let rect = egui::Rect::from_min_size(
            egui::pos2(right - label_width, header.top() + 22.0),
            egui::vec2(label_width, 30.0),
        );
        if layout::action_button(ui, rect, &label, Some(IconName::Trash), ButtonKind::Ghost)
            .clicked()
        {
            for index in selected.iter().rev() {
                actions.push(FrontendAction::VocabRemovePhrase(*index));
            }
            set_selection(ui.ctx(), BTreeSet::new());
        }
    }
    ui.add_space(GAP);

    // ── Tool row: tabs + select-all + expandable search ─────────────────────
    let visible: Vec<usize> = visible_indices(vm);
    tool_row(ui, width, vm, &visible, &selected, actions);
    ui.add_space(12.0);

    if let Some(error) = vm.vocab_error.clone() {
        error_banner(ui, width, &error);
        ui.add_space(10.0);
    }

    // Auto-collected group gets a stable "remove all" exit while filtering.
    if vm.vocab_filter == 1 {
        let learned: Vec<usize> = visible
            .iter()
            .copied()
            .filter(|index| vm.vocab_entries[*index].learned)
            .collect();
        if !learned.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(fmt_l10n(lang, "vocab.learned_section", &[&learned.len()]))
                        .size(12.0)
                        .color(theme::INK_3),
                );
                ui.add_space(6.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(tr_l10n(lang, "vocab.remove_all_learned"))
                                .size(11.5),
                        )
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(0.0, 28.0)),
                    )
                    .clicked()
                {
                    for index in learned.iter().rev() {
                        actions.push(FrontendAction::VocabRemovePhrase(*index));
                    }
                }
            });
            ui.add_space(10.0);
        }
    }

    // ── Word list ──────────────────────────────────────────────────────────
    if visible.is_empty() {
        ui.add_space(6.0);
        let message = if vm.vocab_query.trim().is_empty() {
            tr_l10n(lang, "vocab.empty").to_string()
        } else {
            tr_l10n(lang, "vocab.search_empty").to_string()
        };
        ui.label(egui::RichText::new(message).size(12.0).color(theme::INK_4));
    } else {
        let mut next_selection = selected.clone();
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for index in &visible {
                let entry = &vm.vocab_entries[*index];
                let is_selected = selected.contains(index);
                match word_chip(ui, entry, is_selected) {
                    ChipAction::None => {}
                    ChipAction::Toggle => {
                        actions.push(FrontendAction::VocabTogglePhrase(*index));
                    }
                    ChipAction::Select => {
                        if is_selected {
                            next_selection.remove(index);
                        } else {
                            next_selection.insert(*index);
                        }
                    }
                    ChipAction::Remove => remove = Some(*index),
                }
            }
        });
        if let Some(index) = remove {
            actions.push(FrontendAction::VocabRemovePhrase(index));
            next_selection.remove(&index);
        }
        if next_selection != selected {
            set_selection(ui.ctx(), next_selection);
        }
    }

    ui.add_space(GAP);

    // ── Quick add + presets ────────────────────────────────────────────────
    quick_add(ui, width, vm, actions);
    ui.add_space(12.0);
    presets(ui, width, vm, actions);
}

// ── Tool row ────────────────────────────────────────────────────────────────

fn tool_row(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    visible: &[usize],
    selected: &BTreeSet<usize>,
    _actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let tabs = [
        (TabIcon::None, tr_l10n(lang, "vocab.filter_all")),
        (TabIcon::Sparkle, tr_l10n(lang, "vocab.filter_auto")),
        (TabIcon::Pencil, tr_l10n(lang, "vocab.filter_manual")),
    ];
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::hover());

    let mut x = row.left();
    for (index, (icon, label)) in tabs.iter().enumerate() {
        let text_width = layout::text_width(ui, label, 12.5);
        let has_icon = *icon != TabIcon::None;
        let tab_width = text_width + if has_icon { 38.0 } else { 22.0 };
        let rect = egui::Rect::from_min_size(
            egui::pos2(x, row.top() + 2.0),
            egui::vec2(tab_width, TAB_HEIGHT),
        );
        let active = vm.vocab_filter.min(2) == index;
        let response = ui.interact(
            rect,
            ui.id().with(("openless-vocab-tab", index)),
            egui::Sense::click(),
        );
        let painter = ui.painter().with_clip_rect(rect);
        if active {
            painter.rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
        } else if response.hovered() {
            painter.rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
        }
        let ink = if active { theme::INK } else { theme::INK_3 };
        let text_left = if has_icon {
            let center = egui::pos2(rect.left() + 13.0, rect.center().y);
            match icon {
                TabIcon::Pencil => draw_pencil(ui, center, ink),
                _ => icons::draw_icon(ui, center, IconName::Sparkle, ink),
            }
            rect.left() + 26.0
        } else {
            rect.left() + 11.0
        };
        painter.text(
            egui::pos2(text_left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            *label,
            egui::FontId::proportional(12.5),
            ink,
        );
        if response.clicked() {
            _actions.push(FrontendAction::VocabFilter(index));
        }
        x = rect.right() + 4.0;
    }

    // Select-all checkbox: label shows the selected count while non-empty.
    let all_selected = !visible.is_empty() && visible.iter().all(|index| selected.contains(index));
    let partial = !all_selected && visible.iter().any(|index| selected.contains(index));
    let checkbox_rect = egui::Rect::from_min_size(
        egui::pos2(x + 10.0, row.center().y - 8.0),
        egui::vec2(16.0, 16.0),
    );
    let checkbox = ui.interact(
        checkbox_rect,
        ui.id().with("openless-vocab-select-all"),
        egui::Sense::click(),
    );
    draw_checkbox(ui, checkbox_rect, all_selected, partial);
    let label = if selected.is_empty() {
        tr_l10n(lang, "vocab.select_all_visible").to_string()
    } else {
        fmt_l10n(lang, "vocab.selected_count", &[&selected.len()])
    };
    ui.painter().text(
        egui::pos2(checkbox_rect.right() + 7.0, row.center().y),
        egui::Align2::LEFT_CENTER,
        &label,
        egui::FontId::proportional(12.0),
        theme::INK_3,
    );
    let label_rect = egui::Rect::from_min_max(
        egui::pos2(checkbox_rect.right() + 3.0, row.top() + 4.0),
        egui::pos2(
            checkbox_rect.right() + 12.0 + layout::text_width(ui, &label, 12.0),
            row.bottom() - 4.0,
        ),
    );
    let label_response = ui.interact(
        label_rect,
        ui.id().with("openless-vocab-select-all-label"),
        egui::Sense::click(),
    );
    if checkbox.clicked() || label_response.clicked() {
        let mut next: BTreeSet<usize> = selected.clone();
        if all_selected {
            for index in visible {
                next.remove(index);
            }
        } else {
            for index in visible {
                next.insert(*index);
            }
        }
        set_selection(ui.ctx(), next);
    }

    // Search: a circular button at the right edge that expands into an input.
    let open = search_open(ui.ctx());
    let circle = egui::Rect::from_center_size(
        egui::pos2(row.right() - 15.0, row.center().y),
        egui::vec2(30.0, 30.0),
    );
    if open {
        let rect = egui::Rect::from_min_size(
            egui::pos2(circle.left() - 8.0 - SEARCH_WIDTH, row.center().y - 16.0),
            egui::vec2(SEARCH_WIDTH, 32.0),
        );
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(16), theme::SURFACE_2);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(16),
            egui::Stroke::new(0.8, theme::LINE),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink2(egui::vec2(12.0, 6.0));
        let response = ui.put(
            inner,
            egui::TextEdit::singleline(&mut vm.vocab_query)
                .id(egui::Id::new(SEARCH_INPUT_ID))
                .hint_text(tr_l10n(lang, "vocab.search_placeholder"))
                .text_color(theme::INK)
                .frame(egui::Frame::NONE),
        );
        if response.changed() {
            _actions.push(FrontendAction::VocabSearch(vm.vocab_query.clone()));
        }
    }
    let search_response = ui.interact(
        circle,
        ui.id().with("openless-vocab-search-toggle"),
        egui::Sense::click(),
    );
    ui.painter().rect_filled(
        circle,
        egui::CornerRadius::same(15),
        if search_response.hovered() || open {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        },
    );
    ui.painter().rect_stroke(
        circle,
        egui::CornerRadius::same(15),
        egui::Stroke::new(0.8, theme::LINE),
        egui::StrokeKind::Inside,
    );
    icons::draw_icon(ui, circle.center(), IconName::Search, theme::INK_3);
    if search_response.clicked() {
        if open && !vm.vocab_query.is_empty() {
            vm.vocab_query.clear();
            _actions.push(FrontendAction::VocabSearch(String::new()));
        } else {
            set_search_open(ui.ctx(), !open);
            if !open {
                ui.memory_mut(|memory| memory.request_focus(egui::Id::new(SEARCH_ID)));
            }
        }
    }
}

fn visible_indices(vm: &FrontendViewModel) -> Vec<usize> {
    let query = vm.vocab_query.trim().to_lowercase();
    vm.vocab_entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| match vm.vocab_filter {
            1 => entry.learned,
            2 => !entry.learned,
            _ => true,
        })
        .filter(|(_, entry)| query.is_empty() || entry.phrase.to_lowercase().contains(&query))
        .map(|(index, _)| index)
        .collect()
}

// ── Word chip ───────────────────────────────────────────────────────────────

enum ChipAction {
    None,
    Toggle,
    Select,
    Remove,
}

/// Tab icons: the shared icon set has a sparkle but no pencil, so the manual
/// tab draws a small pencil locally.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TabIcon {
    None,
    Sparkle,
    Pencil,
}

fn draw_pencil(ui: &egui::Ui, center: egui::Pos2, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.25, color);
    ui.painter().line_segment(
        [
            center + egui::vec2(-5.0, 5.0),
            center + egui::vec2(3.5, -3.5),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center + egui::vec2(3.5, -3.5),
            center + egui::vec2(5.0, -1.2),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center + egui::vec2(-5.0, 5.0),
            center + egui::vec2(-2.6, 4.4),
        ],
        stroke,
    );
}

fn word_chip(ui: &mut egui::Ui, entry: &VocabEntry, selected: bool) -> ChipAction {
    let fill = if !entry.enabled {
        theme::SURFACE_2
    } else if entry.hits > 0 {
        theme::BLUE_SOFT
    } else {
        theme::SURFACE
    };
    let text_color = if entry.enabled {
        theme::INK
    } else {
        theme::INK_4
    };
    let phrase = ui.painter().layout_no_wrap(
        entry.phrase.clone(),
        egui::FontId::proportional(13.0),
        text_color,
    );
    let hits_text = entry.hits.to_string();
    let hits_color = if entry.enabled && entry.hits > 0 {
        theme::SURFACE
    } else {
        theme::INK_4
    };
    let hits = ui
        .painter()
        .layout_no_wrap(hits_text, egui::FontId::proportional(11.0), hits_color);
    let hits_size = egui::vec2((hits.size().x + 12.0).max(24.0), 22.0);
    let checkbox_size = 16.0;
    let close_size = 22.0;
    let width =
        10.0 + checkbox_size + 8.0 + phrase.size().x + 8.0 + hits_size.x + 6.0 + close_size + 10.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(16), fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(16),
        egui::Stroke::new(0.6, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let checkbox_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 10.0 + checkbox_size / 2.0, rect.center().y),
        egui::vec2(checkbox_size, checkbox_size),
    );
    draw_checkbox(ui, checkbox_rect, selected, false);
    painter.galley(
        egui::pos2(
            checkbox_rect.right() + 8.0,
            rect.center().y - phrase.size().y / 2.0,
        ),
        phrase,
        text_color,
    );
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 10.0 - close_size / 2.0, rect.center().y),
        egui::vec2(close_size, close_size),
    );
    let hits_rect = egui::Rect::from_min_size(
        egui::pos2(
            close_rect.left() - 6.0 - hits_size.x,
            rect.center().y - hits_size.y / 2.0,
        ),
        hits_size,
    );
    painter.rect_filled(
        hits_rect,
        egui::CornerRadius::same(5),
        if entry.enabled && entry.hits > 0 {
            theme::BLUE
        } else {
            theme::TOGGLE_OFF
        },
    );
    painter.galley(
        egui::pos2(
            hits_rect.center().x - hits.size().x / 2.0,
            hits_rect.center().y - hits.size().y / 2.0,
        ),
        hits,
        hits_color,
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
        if let Some(pointer) = response.interact_pointer_pos() {
            if close_rect.contains(pointer) {
                return ChipAction::Remove;
            }
            if checkbox_rect.contains(pointer) {
                return ChipAction::Select;
            }
        }
        return ChipAction::Toggle;
    }
    ChipAction::None
}

fn draw_checkbox(ui: &egui::Ui, rect: egui::Rect, checked: bool, partial: bool) {
    let painter = ui.painter();
    let fill = if checked || partial {
        theme::INK
    } else {
        theme::SURFACE
    };
    painter.rect_filled(rect, egui::CornerRadius::same(4), fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(
            0.8,
            if checked || partial {
                theme::INK
            } else {
                theme::LINE
            },
        ),
        egui::StrokeKind::Inside,
    );
    if checked {
        let stroke = egui::Stroke::new(1.5, theme::SURFACE);
        painter.line_segment(
            [
                rect.left_center() + egui::vec2(3.0, 0.5),
                rect.center_bottom() + egui::vec2(-1.0, -3.5),
            ],
            stroke,
        );
        painter.line_segment(
            [
                rect.center_bottom() + egui::vec2(-1.0, -3.5),
                rect.right_center() + egui::vec2(-2.5, -5.0),
            ],
            stroke,
        );
    } else if partial {
        painter.rect_filled(
            egui::Rect::from_center_size(rect.center(), egui::vec2(8.0, 2.0)),
            egui::CornerRadius::same(1),
            theme::SURFACE,
        );
    }
}

// ── Bottom: quick add + presets ─────────────────────────────────────────────

fn quick_add(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    ui.horizontal(|ui| {
        let add_width = 88.0;
        let input_width = (width - add_width - 8.0).max(80.0);
        let content_width = (input_width - 24.0).max(1.0);
        egui::Frame::new()
            .fill(theme::SURFACE_2)
            .stroke(egui::Stroke::new(0.8, theme::LINE))
            .corner_radius(egui::CornerRadius::same(18))
            .inner_margin(egui::Margin::symmetric(12, 7))
            .show(ui, |ui| {
                ui.set_width(content_width);
                let response = ui.add_sized(
                    [content_width, 20.0],
                    egui::TextEdit::singleline(&mut vm.vocab_input)
                        .id(egui::Id::new(INPUT_ID))
                        .desired_width(content_width)
                        .hint_text(tr_l10n(lang, "vocab.placeholder"))
                        .frame(egui::Frame::NONE),
                );
                if (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                    || (ui.input(|input| input.key_pressed(egui::Key::Enter))
                        && response.has_focus())
                {
                    let phrase = vm.vocab_input.trim().to_string();
                    if !phrase.is_empty() {
                        actions.push(FrontendAction::VocabAddPhrase(phrase));
                        vm.vocab_input.clear();
                    }
                }
            });
        let add = tr_l10n(lang, "btn.add");
        let rect = egui::Rect::from_min_size(
            egui::pos2(ui.cursor().min.x, ui.cursor().min.y),
            egui::vec2(add_width, 34.0),
        );
        if primary_button(ui, rect, add, Some(IconName::Hash)).clicked() {
            let phrase = vm.vocab_input.trim().to_string();
            if !phrase.is_empty() {
                actions.push(FrontendAction::VocabAddPhrase(phrase));
                vm.vocab_input.clear();
            }
        }
        ui.allocate_space(egui::vec2(add_width, 34.0));
    });
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(tr_l10n(lang, "vocab.tip"))
            .size(11.5)
            .color(theme::INK_4),
    );
}

fn presets(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    card(ui, width, |ui| {
        layout::section_title(
            ui,
            ui.available_width(),
            tr_l10n(lang, "vocab.presets_title"),
            Some(tr_l10n(lang, "vocab.presets_tip")),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            let names = [
                tr_l10n(lang, "vocab.presets_dev_tools"),
                tr_l10n(lang, "vocab.presets_products"),
                tr_l10n(lang, "vocab.presets_terms"),
                tr_l10n(lang, "vocab.presets_english"),
            ];
            for (index, name) in names.iter().enumerate() {
                let selected = vm.vocab_selected_presets.contains(&index);
                let response = ui.add(
                    egui::Button::new(egui::RichText::new(*name).size(12.5).color(if selected {
                        theme::BLUE
                    } else {
                        theme::INK_2
                    }))
                    .fill(if selected {
                        theme::BLUE_SOFT
                    } else {
                        theme::SURFACE_2
                    })
                    .stroke(egui::Stroke::new(0.5, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .min_size(egui::vec2(88.0, 30.0)),
                );
                if response.clicked() {
                    actions.push(FrontendAction::VocabApplyPreset(index));
                }
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(tr_l10n(lang, "vocab.presets_create")).size(12.5),
                    )
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(0.5, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(96.0, 32.0)),
                )
                .clicked()
            {
                vm.vocab_editing_preset = Some(usize::MAX);
                vm.vocab_preset_name = tr_l10n(lang, "vocab.presets_new_preset").into();
                vm.vocab_preset_phrases.clear();
            }
            if !vm.vocab_selected_presets.is_empty() {
                let apply = tr_l10n(lang, "vocab.presets_apply");
                let rect = egui::Rect::from_min_size(
                    egui::pos2(ui.cursor().min.x, ui.cursor().min.y),
                    egui::vec2(88.0, 32.0),
                );
                if primary_button(ui, rect, apply, None).clicked() {
                    actions.push(FrontendAction::VocabApplyPreset(usize::MAX));
                }
                ui.allocate_space(egui::vec2(88.0, 32.0));
            }
        });

        if vm.vocab_editing_preset.is_some() {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                egui::Frame::new()
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.8, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.add_sized(
                            [180.0, 20.0],
                            egui::TextEdit::singleline(&mut vm.vocab_preset_name)
                                .hint_text(tr_l10n(lang, "vocab.presets_name_placeholder"))
                                .frame(egui::Frame::NONE),
                        );
                    });
                let save = tr_l10n(lang, "vocab.presets_save");
                if primary_button(
                    ui,
                    egui::Rect::from_min_size(ui.cursor().min, egui::vec2(84.0, 32.0)),
                    save,
                    None,
                )
                .clicked()
                {
                    let name = vm.vocab_preset_name.trim().to_owned();
                    if !name.is_empty() {
                        actions.push(FrontendAction::VocabCreatePreset {
                            name,
                            phrases: vm.vocab_preset_phrases.clone(),
                        });
                    }
                    vm.vocab_editing_preset = None;
                }
                ui.allocate_space(egui::vec2(84.0, 32.0));
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(tr_l10n(lang, "common.cancel")).size(12.0),
                        )
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(72.0, 32.0)),
                    )
                    .clicked()
                {
                    vm.vocab_editing_preset = None;
                }
            });
            ui.add_space(8.0);
            ui.add_sized(
                [ui.available_width(), 64.0],
                egui::TextEdit::multiline(&mut vm.vocab_preset_phrases)
                    .desired_rows(3)
                    .hint_text(tr_l10n(lang, "vocab.presets_words_placeholder")),
            );
        }

        if vm.vocab_editing_preset.is_none() && !vm.vocab_saved_presets.is_empty() {
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                let saved = vm.vocab_saved_presets.clone();
                for (index, preset) in saved.iter().enumerate() {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(fmt_l10n(
                                    lang,
                                    "vocab.presets_edit",
                                    &[&preset.name],
                                ))
                                .size(12.5),
                            )
                            .fill(theme::SURFACE_2)
                            .stroke(egui::Stroke::new(0.6, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(14))
                            .min_size(egui::vec2(0.0, 30.0)),
                        )
                        .clicked()
                    {
                        vm.vocab_preset_name = preset.name.clone();
                        vm.vocab_preset_phrases = preset.phrases.clone();
                        vm.vocab_editing_preset = Some(index);
                    }
                }
            });
        }
    });
}

// ── Small shared pieces ─────────────────────────────────────────────────────

fn error_banner(ui: &mut egui::Ui, width: f32, message: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), theme::DANGER_SOFT);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(0.5, theme::ERR),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        message,
        egui::FontId::proportional(12.0),
        theme::ERR,
    );
}

/// Dark solid button (the Tauri `variant=primary`).
fn primary_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    label: &str,
    icon: Option<IconName>,
) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.id().with(("openless-vocab-primary", label)),
        egui::Sense::click(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    let fill = if response.hovered() {
        theme::INK_2
    } else {
        theme::INK
    };
    painter.rect_filled(rect, egui::CornerRadius::same(8), fill);
    let label_width = layout::text_width(ui, label, 12.5);
    let icon_space = if icon.is_some() { 18.0 } else { 0.0 };
    let mut x = rect.center().x - (label_width + icon_space) / 2.0;
    if let Some(icon) = icon {
        icons::draw_icon(
            ui,
            egui::pos2(x + 6.0, rect.center().y),
            icon,
            theme::SURFACE,
        );
        x += icon_space;
    }
    painter.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        theme::SURFACE,
    );
    response
}

/// Full-width card that sizes itself to its contents.
fn card(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(CARD_PADDING as i8))
        .show(ui, |ui| {
            ui.set_width((width - CARD_PADDING * 2.0).max(1.0));
            contents(ui);
        });
}
