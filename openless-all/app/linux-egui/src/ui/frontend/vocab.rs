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
const NEW_WORD_OPEN: &str = "openless-vocab-new-word-open";
const NEW_WORD_FOCUS: &str = "openless-vocab-new-word-focus";
const PRESETS_OPEN: &str = "openless-vocab-presets-open";
const SELECTION_ID: &str = "openless-vocab-selection";
const TAB_HEIGHT: f32 = 32.0;
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
    let mut opened_new_word_now = false;
    // Primary "new word" action (dark solid, like the Tauri `variant=primary`).
    let new_word = tr_l10n(lang, "vocab.new_word");
    let new_word_width = layout::text_width(ui, new_word, 12.5) + 42.0;
    let new_word_rect = egui::Rect::from_min_size(
        egui::pos2(right - new_word_width, header.top() + 22.0),
        egui::vec2(new_word_width, 30.0),
    );
    right = new_word_rect.left() - 8.0;
    if primary_button(ui, new_word_rect, new_word, Some(IconName::Plus)).clicked() {
        opened_new_word_now = true;
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new(NEW_WORD_OPEN), true);
            data.insert_temp(egui::Id::new(NEW_WORD_FOCUS), true);
        });
        vm.vocab_input.clear();
        vm.vocab_selected_presets.clear();
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

    // 词条独立滚动。下方整块（阴影分隔线、输入、提示、场景预设）
    // 贴住窗口底缘；面板展开时向上生长，不随词条内容高度漂浮。
    let presets_expanded = ui.ctx().data(|data| {
        data.get_temp::<bool>(egui::Id::new(PRESETS_OPEN))
            .unwrap_or(false)
    });
    let panel_id = egui::Id::new(("openless-vocab-bottom-height", presets_expanded));
    let measured = ui.ctx().data(|data| data.get_temp::<f32>(panel_id));
    let reserved = measured.unwrap_or(if presets_expanded { 290.0 } else { 190.0 });
    let viewport_bottom = ui.max_rect().bottom();
    // egui's ScrollArea adds its own spacing after the viewport. Leave room
    // for it so the fixed panel is never pushed below the content bottom.
    let list_height = (viewport_bottom - ui.cursor().min.y - reserved - 12.0).max(80.0);
    egui::ScrollArea::vertical()
        .id_salt("openless-vocab-list-scroll")
        .max_height(list_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
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
        });
    let panel_top = (viewport_bottom - reserved).max(ui.cursor().min.y + 6.0);
    let panel_rect = egui::Rect::from_min_max(
        egui::pos2(ui.cursor().min.x, panel_top),
        egui::pos2(ui.cursor().min.x + width, viewport_bottom),
    );
    let actual_height = layout::fixed_ui(ui, panel_rect, "openless-vocab-bottom", |ui| {
        let top = panel_rect.top();
        let (divider, _) = ui.allocate_exact_size(egui::vec2(width, 3.0), egui::Sense::hover());
        ui.painter().line_segment(
            [divider.left_top(), divider.right_top()],
            egui::Stroke::new(1.0, theme::LINE),
        );
        // Soft inset shadow separating the fixed entry toolbar from the scrolling list.
        for (offset, alpha) in [(1.0, 20), (2.0, 10), (3.0, 5)] {
            ui.painter().line_segment(
                [
                    divider.left_top() - egui::vec2(0.0, offset),
                    divider.right_top() - egui::vec2(0.0, offset),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_black_alpha(alpha)),
            );
        }
        quick_add(ui, width, vm, actions);
        ui.add_space(12.0);
        presets(ui, width, vm, actions);
        ui.cursor().min.y - top
    });
    if measured.is_none_or(|previous| (previous - actual_height).abs() > 0.5) {
        ui.ctx()
            .data_mut(|data| data.insert_temp(panel_id, actual_height));
        ui.ctx().request_repaint();
    }
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("openless-vocab-bottom-test-rect"),
            egui::Rect::from_min_size(panel_rect.min, egui::vec2(width, actual_height)),
        );
        data.insert_temp(
            egui::Id::new("openless-vocab-viewport-test-bottom"),
            viewport_bottom,
        );
    });
    if !opened_new_word_now
        && ui.ctx().data(|data| {
            data.get_temp::<bool>(egui::Id::new(NEW_WORD_OPEN))
                .unwrap_or(false)
        })
    {
        new_word_overlay(ui.ctx(), vm, actions);
    }
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
        (TabIcon::Feather, tr_l10n(lang, "vocab.filter_manual")),
    ];
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::hover());

    let tabs_width: f32 = tabs
        .iter()
        .map(|(icon, label)| {
            layout::text_width(ui, label, 13.0) + if *icon == TabIcon::None { 28.0 } else { 47.0 }
        })
        .sum();
    let tab_background = egui::Rect::from_min_size(
        row.min + egui::vec2(0.0, 0.5),
        egui::vec2(tabs_width + 10.0, TAB_HEIGHT + 7.0),
    );
    ui.painter().rect_filled(
        tab_background,
        egui::CornerRadius::same(20),
        theme::SURFACE_2,
    );
    ui.painter().rect_stroke(
        tab_background,
        egui::CornerRadius::same(20),
        egui::Stroke::new(0.5, theme::LINE),
        egui::StrokeKind::Inside,
    );
    // 先算出每个 tab 的矩形：活动胶囊的位置只用一个动画值插值。以前在循环里对
    // 同一个 id 反复写入「非活动 tab 就指向活动位置」的目标值，同一帧三次写不同
    // 目标，胶囊会抖动。
    let mut tab_rects = Vec::with_capacity(tabs.len());
    let mut x = row.left() + 3.0;
    for (icon, label) in &tabs {
        let text_width = layout::text_width(ui, label, 13.0);
        let tab_width = text_width + if *icon == TabIcon::None { 28.0 } else { 47.0 };
        tab_rects.push(egui::Rect::from_min_size(
            egui::pos2(x, row.top() + 4.0),
            egui::vec2(tab_width, TAB_HEIGHT),
        ));
        x += tab_width + 2.0;
    }
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("openless-vocab-tabs-test-rects"),
            (tab_background, tab_rects.clone()),
        );
    });
    let active = vm.vocab_filter.min(2);
    let pill_x = ui.ctx().animate_value_with_time(
        ui.id().with("vocab-active-pill-x"),
        tab_rects[active].left(),
        0.18,
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(pill_x, tab_rects[active].top()),
            tab_rects[active].size(),
        ),
        egui::CornerRadius::same(16),
        theme::SURFACE,
    );
    for (index, (icon, label)) in tabs.iter().enumerate() {
        let rect = tab_rects[index];
        let active = active == index;
        let response = ui.interact(
            rect,
            ui.id().with(("openless-vocab-tab", index)),
            egui::Sense::click(),
        );
        let painter = ui.painter().with_clip_rect(rect);
        if !active && response.hovered() {
            painter.rect_filled(rect, egui::CornerRadius::same(16), theme::SURFACE);
        }
        let ink = if active || response.hovered() {
            theme::INK
        } else {
            theme::INK_3
        };
        let text_left = if *icon != TabIcon::None {
            let center = egui::pos2(rect.left() + 21.0, rect.center().y);
            match icon {
                TabIcon::Feather => icons::draw_icon(ui, center, IconName::Feather, ink),
                _ => icons::draw_icon(ui, center, IconName::Sparkle, ink),
            }
            rect.left() + 33.0
        } else {
            rect.left() + 14.0
        };
        painter.text(
            egui::pos2(text_left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            *label,
            egui::FontId::proportional(13.0),
            ink,
        );
        if response.clicked() {
            _actions.push(FrontendAction::VocabFilter(index));
        }
    }
    let x = tab_rects.last().map(|rect| rect.right() + 3.0).unwrap_or(x);

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
    Feather,
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
    let open = ui.ctx().data(|data| {
        data.get_temp::<bool>(egui::Id::new(PRESETS_OPEN))
            .unwrap_or(false)
    });
    card(ui, width, |ui| {
        let (heading, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 42.0), egui::Sense::click());
        let painter = ui.painter();
        painter.text(
            egui::pos2(heading.left(), heading.top() + 11.0),
            egui::Align2::LEFT_CENTER,
            tr_l10n(lang, "vocab.presets_title"),
            egui::FontId::proportional(13.0),
            theme::INK,
        );
        painter.text(
            egui::pos2(heading.left(), heading.top() + 30.0),
            egui::Align2::LEFT_CENTER,
            tr_l10n(lang, "vocab.presets_tip"),
            egui::FontId::proportional(11.5),
            theme::INK_4,
        );
        // Tauri uses a rotated ChevronRight. Draw its two strokes directly so
        // expanded state never depends on an unavailable Unicode glyph.
        let center = egui::pos2(heading.right() - 8.0, heading.center().y);
        let points = if open {
            [
                center + egui::vec2(-3.0, -1.5),
                center + egui::vec2(0.0, 1.5),
                center + egui::vec2(3.0, -1.5),
            ]
        } else {
            [
                center + egui::vec2(-1.5, -3.0),
                center + egui::vec2(1.5, 0.0),
                center + egui::vec2(-1.5, 3.0),
            ]
        };
        for pair in points.windows(2) {
            painter.line_segment([pair[0], pair[1]], egui::Stroke::new(1.5, theme::INK_3));
        }
        if response.clicked() {
            ui.ctx()
                .data_mut(|data| data.insert_temp(egui::Id::new(PRESETS_OPEN), !open));
        }
        if !open {
            return;
        }
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

// ── New-word modal (scoped to the right content pane) ───────────────────────

fn new_word_overlay(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let body = layout::body_rect(ctx);
    let region = egui::Rect::from_min_max(
        egui::pos2(body.left() + layout::SIDEBAR_WIDTH, body.top()),
        body.max,
    );
    // Tauri ModalShell is 440px wide and grows with the actual preset cards.
    let presets = vm.vocab_saved_presets.clone();
    let modal_width = 440.0_f32.min(region.width() - 36.0);
    let modal_height = (224.0 + presets.len() as f32 * 54.0)
        .max(260.0)
        .min(region.height() - 32.0);
    let modal =
        egui::Rect::from_center_size(region.center(), egui::vec2(modal_width, modal_height));
    let mut close = ctx.input(|input| input.key_pressed(egui::Key::Escape));
    if ctx.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| body.contains(pos) && !modal.contains(pos))
    }) {
        close = true;
    }
    egui::Area::new(egui::Id::new("openless-vocab-new-word-area"))
        .order(egui::Order::Foreground)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            ui.painter().rect_filled(body, 0, theme::OVERLAY);
            let _ = ui.allocate_rect(body, egui::Sense::click());
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("new-word-dialog")
                    .max_rect(modal)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(body);
            let card = egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(0.5, theme::LINE))
                .corner_radius(egui::CornerRadius::same(16))
                .shadow(egui::Shadow {
                    offset: [0, 24],
                    blur: 32,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(72),
                })
                .inner_margin(egui::Margin::same(20))
                .show(&mut child, |ui| {
                    ui.set_width((modal_width - 44.0).max(1.0));
                    ui.set_min_height((modal_height - 44.0).max(1.0));
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(tr_l10n(lang, "vocab.newWordTitle"))
                                    .size(15.0)
                                    .strong()
                                    .color(theme::INK),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(tr_l10n(lang, "vocab.newWordDesc"))
                                    .size(12.5)
                                    .color(theme::INK_3),
                            );
                        });
                        let remaining =
                            (modal_width - 44.0 - ui.min_rect().width() - 34.0).max(0.0);
                        ui.add_space(remaining);
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(8),
                                theme::SURFACE_2,
                            );
                        }
                        icons::draw_icon(ui, rect.center(), IconName::Close, theme::INK_4);
                        #[cfg(test)]
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("openless-new-word-close-rect"), rect)
                        });
                        if response.clicked() {
                            close = true;
                        }
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        let field_width = (modal_width - 44.0 - 92.0).max(90.0);
                        let edit = egui::Frame::new()
                            .fill(theme::SURFACE_2)
                            .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                            .corner_radius(egui::CornerRadius::same(19))
                            .inner_margin(egui::Margin::symmetric(12, 9))
                            .show(ui, |ui| {
                                ui.add_sized(
                                    [field_width - 24.0, 20.0],
                                    egui::TextEdit::singleline(&mut vm.vocab_input)
                                        .hint_text(tr_l10n(lang, "vocab.newWordInputPlaceholder"))
                                        .frame(egui::Frame::NONE),
                                )
                            })
                            .inner;
                        if ui
                            .ctx()
                            .data_mut(|data| {
                                data.remove_temp::<bool>(egui::Id::new(NEW_WORD_FOCUS))
                            })
                            .unwrap_or(false)
                        {
                            edit.request_focus();
                        }
                        let add = tr_l10n(lang, "btn.add");
                        let (button, _) =
                            ui.allocate_exact_size(egui::vec2(84.0, 32.0), egui::Sense::hover());
                        if primary_button(ui, button, add, Some(IconName::Plus)).clicked()
                            || (edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            let word = vm.vocab_input.trim().to_string();
                            if !word.is_empty() {
                                actions.push(FrontendAction::VocabAddPhrase(word));
                                vm.vocab_input.clear();
                            }
                        }
                    });
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "vocab.newWordTemplates"))
                            .size(12.5)
                            .strong(),
                    );
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical()
                        .id_salt("openless-new-word-templates")
                        .max_height((modal_height - 222.0).max(52.0))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (index, preset) in presets.iter().enumerate() {
                                let selected = vm.vocab_selected_presets.contains(&index);
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 46.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(10),
                                    if selected {
                                        theme::BLUE_SOFT
                                    } else if response.hovered() {
                                        theme::SURFACE_2
                                    } else {
                                        theme::SURFACE
                                    },
                                );
                                ui.painter().rect_stroke(
                                    rect,
                                    egui::CornerRadius::same(10),
                                    egui::Stroke::new(
                                        if selected { 1.0 } else { 0.5 },
                                        if selected {
                                            theme::BLUE
                                        } else {
                                            theme::LINE_STRONG
                                        },
                                    ),
                                    egui::StrokeKind::Inside,
                                );
                                let text_clip = egui::Rect::from_min_max(
                                    rect.min,
                                    egui::pos2(rect.right() - 65.0, rect.bottom()),
                                );
                                let painter = ui.painter().with_clip_rect(text_clip);
                                painter.text(
                                    rect.left_top() + egui::vec2(12.0, 14.0),
                                    egui::Align2::LEFT_CENTER,
                                    &preset.name,
                                    egui::FontId::proportional(13.0),
                                    theme::INK,
                                );
                                painter.text(
                                    rect.left_top() + egui::vec2(12.0, 32.0),
                                    egui::Align2::LEFT_CENTER,
                                    preset.phrases.replace('、', " · "),
                                    egui::FontId::proportional(11.5),
                                    theme::INK_4,
                                );
                                let count =
                                    preset.phrases.split('、').filter(|s| !s.is_empty()).count();
                                ui.painter().text(
                                    egui::pos2(
                                        rect.right() - if selected { 30.0 } else { 12.0 },
                                        rect.center().y,
                                    ),
                                    egui::Align2::RIGHT_CENTER,
                                    fmt_l10n(lang, "vocab.newWordTemplateCount", &[&count]),
                                    egui::FontId::proportional(11.5),
                                    theme::INK_4,
                                );
                                if selected {
                                    icons::draw_icon(
                                        ui,
                                        rect.right_center() - egui::vec2(12.0, 0.0),
                                        IconName::Check,
                                        theme::INK_2,
                                    );
                                }
                                if response.clicked() {
                                    if selected {
                                        vm.vocab_selected_presets.retain(|item| *item != index);
                                    } else {
                                        vm.vocab_selected_presets.push(index);
                                    }
                                }
                                if index + 1 < presets.len() {
                                    ui.add_space(8.0);
                                }
                            }
                        });
                    ui.add_space(18.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !vm.vocab_selected_presets.is_empty(),
                                egui::Button::new(
                                    egui::RichText::new(tr_l10n(lang, "vocab.newWordAddSelected"))
                                        .size(13.0)
                                        .color(theme::SURFACE),
                                )
                                .fill(theme::INK)
                                .stroke(egui::Stroke::NONE)
                                .corner_radius(egui::CornerRadius::same(8)),
                            )
                            .clicked()
                        {
                            for index in vm.vocab_selected_presets.drain(..) {
                                actions.push(FrontendAction::VocabApplyPreset(index));
                            }
                            close = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(tr_l10n(lang, "common.close"))
                                    .fill(theme::SURFACE)
                                    .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                    .corner_radius(egui::CornerRadius::same(8)),
                            )
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("openless-new-word-card-rect"),
                    card.response.rect,
                )
            });
            #[cfg(not(test))]
            let _ = card;
        });
    if close {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(NEW_WORD_OPEN), false));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_panel_stays_at_viewport_bottom_and_grows_upward() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel::default();
        vm.vocab_unsupported = false;
        let mut actions = Vec::new();
        let mut collapsed_top = 0.0;
        for expanded in [false, true] {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new(PRESETS_OPEN), expanded));
            // The first pass measures the panel; the next pass places it at the bottom.
            for _ in 0..3 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1240.0, 800.0),
                    )),
                    ..Default::default()
                });
                layout::content_panel(&ctx, |ui| page(ui, &mut vm, &mut actions));
                let _ = crate::ui::frontend::end_pass(&ctx);
            }
            let rect: egui::Rect = ctx.data(|data| {
                data.get_temp(egui::Id::new("openless-vocab-bottom-test-rect"))
                    .unwrap()
            });
            let bottom: f32 = ctx.data(|data| {
                data.get_temp(egui::Id::new("openless-vocab-viewport-test-bottom"))
                    .unwrap()
            });
            assert!(
                (rect.bottom() - bottom).abs() < 0.5,
                "expanded={expanded}, rect={rect:?}, bottom={bottom}"
            );
            if expanded {
                assert!(
                    rect.top() < collapsed_top,
                    "expanded panel must grow upward"
                );
            } else {
                collapsed_top = rect.top();
            }
        }
    }

    #[test]
    fn filters_match_tauri_segmented_control_spacing() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel::default();
        let _ = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1240.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| tool_row(ui, 900.0, &mut vm, &[], &BTreeSet::new(), &mut Vec::new()),
        );
        let (outer, items): (egui::Rect, Vec<egui::Rect>) = ctx.data(|data| {
            data.get_temp(egui::Id::new("openless-vocab-tabs-test-rects"))
                .unwrap()
        });
        assert_eq!(outer.height(), 39.0);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].height(), 32.0);
        assert_eq!(items[0].left() - outer.left(), 3.0);
        assert_eq!(items[1].left() - items[0].right(), 2.0);
        assert_eq!(outer.right() - items[2].right(), 3.0);
    }

    #[test]
    fn new_word_dialog_uses_tauri_card_and_real_presets() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel::default();
        vm.lang = openless_linux_egui::Lang::ZhCn;
        let presets: Vec<_> = [
            ("programmer", "PR、CI、Rust"),
            ("chef", "heat、knife"),
            ("clerk", "letter、report"),
        ]
        .into_iter()
        .map(
            |(name, phrases)| super::super::view_model::SavedVocabPreset {
                name: name.into(),
                phrases: phrases.into(),
            },
        )
        .collect();
        vm.vocab_saved_presets = presets.clone();
        let mut heights = Vec::new();
        for count in [1, 3] {
            vm.vocab_saved_presets.truncate(count);
            let _ = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1240.0, 800.0),
                    )),
                    ..Default::default()
                },
                |_ui| new_word_overlay(&ctx, &mut vm, &mut Vec::new()),
            );
            let card: egui::Rect = ctx.data(|data| {
                data.get_temp(egui::Id::new("openless-new-word-card-rect"))
                    .unwrap()
            });
            let close: egui::Rect = ctx.data(|data| {
                data.get_temp(egui::Id::new("openless-new-word-close-rect"))
                    .unwrap()
            });
            assert!(card.contains_rect(close), "close={close:?}, card={card:?}");
            assert!((card.width() - 440.0).abs() < 10.0, "card={card:?}");
            assert!(card.height() < 440.0, "card={card:?}");
            heights.push(card.height());
            if count == 1 {
                vm.vocab_saved_presets = presets.clone();
            }
        }
        assert!(
            heights[1] > heights[0] + 90.0,
            "modal must grow with real presets: {heights:?}"
        );
    }
}
