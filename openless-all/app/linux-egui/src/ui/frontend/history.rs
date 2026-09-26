//! History page — list + detail, ported from the Tauri `pages/History.tsx`.
//!
//! Layout:
//!
//! ```text
//! ┌ 历史记录 (kicker/title/desc)            [刷新] [清空] ┐
//! │ ┌ list ───────────┐ ┌ detail ─────────────────────┐ │
//! │ │ 🔍 search       │ │ time  pill  录音 3.1 秒  ⋯  │ │
//! │ │ row / row / …   │ │ [播放录音]                   │ │
//! │ │                 │ │ 识别  provider · model  465ms│ │
//! │ │                 │ │ 插入  App · 0 字      插入失败│ │
//! │ │                 │ │ [原文]        [样式] [复制]  │ │
//! │ └─────────────────┘ └─────────────────────────────┘ │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! The page is a single-screen layout: the list and detail cards split the
//! height the shell gives them, and each scrolls independently. Only when both
//! columns cannot fit do they stack. All strings come from the localization
//! catalog; nothing is hardcoded.

use std::sync::Arc;

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::format;
use super::icons::{self, IconName};
use super::layout::{self, ButtonKind, PillTone};
use super::settings;
use super::theme;
use super::view_model::{
    FrontendAction, FrontendViewModel, HistoryConfirm, HistoryEntry, OverviewMode, ShortcutField,
};

const GAP: f32 = 14.0;
const LIST_WIDTH: f32 = 300.0;
// Keep the 300px list and a usable 280px detail side by side at the Linux
// window minimum (960px outer, ~706px page width). The old 760px breakpoint
// stacked both cards there and hid the detail below the clipped viewport.
const STACK_WIDTH: f32 = LIST_WIDTH + GAP + 280.0;
const CARD_PADDING: f32 = 20.0;
const DETAIL_PADDING: f32 = 12.0;
const LINE_SOFT: egui::Color32 = theme::LINE_SOFT;
const MONO_SMALL: f32 = 11.0;

/// Faint hover wash for unselected rows.
fn hover_fill() -> egui::Color32 {
    theme::SURFACE_2
}

// ── Entry point ─────────────────────────────────────────────────────────────

pub fn page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    quick_notes_only: bool,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    // Tauri `QuickNote.tsx` puts the dismissible shortcut card above the list.
    if quick_notes_only {
        quick_note_shortcut_card(ui, width, vm, actions);
    }

    header(ui, width, lang, quick_notes_only, actions);
    ui.add_space(GAP);

    let body_height = ui.available_height().max(260.0);
    let (body, _) = ui.allocate_exact_size(egui::vec2(width, body_height), egui::Sense::hover());

    let (list_rect, detail_rect) = if width < STACK_WIDTH {
        let list_height = (body_height * 0.45).clamp(150.0, 300.0);
        (
            egui::Rect::from_min_size(body.min, egui::vec2(width, list_height)),
            egui::Rect::from_min_size(
                egui::pos2(body.left(), body.top() + list_height + GAP),
                egui::vec2(width, (body_height - list_height - GAP).max(140.0)),
            ),
        )
    } else {
        (
            egui::Rect::from_min_size(body.min, egui::vec2(LIST_WIDTH, body_height)),
            egui::Rect::from_min_size(
                egui::pos2(body.left() + LIST_WIDTH + GAP, body.top()),
                egui::vec2((width - LIST_WIDTH - GAP).max(280.0), body_height),
            ),
        )
    };

    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("history-column-rects"),
            (list_rect, detail_rect),
        )
    });
    let filtered = filtered_indices(vm, quick_notes_only);
    list_card(ui, list_rect, vm, &filtered, lang, actions);
    detail_card(ui, detail_rect, vm, &filtered, lang, actions);

    if vm.history_confirm.is_some() {
        confirm_overlay(ui.ctx(), body, vm, lang, actions);
    }
}

/// Dismissible shortcut card for the Quick Note page. Hidden state persists in
/// `linux-ui-state.json`; when hidden a ghost button brings it back.
fn quick_note_shortcut_card(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    if vm.quick_note_shortcut_hidden {
        let show = tr_l10n(lang, "quickNote.showShortcut");
        let button_width = layout::text_width(ui, show, 12.0) + 40.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::hover());
        let button = egui::Rect::from_min_size(
            egui::pos2(rect.right() - button_width, rect.top()),
            egui::vec2(button_width, 28.0),
        );
        if layout::action_button(ui, button, show, Some(IconName::Bolt), ButtonKind::Ghost)
            .clicked()
        {
            actions.push(FrontendAction::QuickNoteShortcutHidden(false));
        }
        ui.add_space(GAP);
        return;
    }

    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(0.5, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(18, 18))
        .show(ui, |ui| {
            ui.set_width((width - 36.0).max(1.0));
            let (row, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
            ui.painter().text(
                egui::pos2(row.left(), row.center().y),
                egui::Align2::LEFT_CENTER,
                tr_l10n(lang, "quickNote.shortcutTitle"),
                egui::FontId::proportional(13.0),
                theme::INK,
            );
            let close = egui::Rect::from_min_size(
                egui::pos2(row.right() - 20.0, row.top()),
                egui::vec2(20.0, 20.0),
            );
            icons::draw_icon(ui, close.center(), IconName::Close, theme::INK_3);
            if ui
                .interact(
                    close,
                    ui.id().with("quick-note-shortcut-hide"),
                    egui::Sense::click(),
                )
                .on_hover_text(tr_l10n(lang, "common.hide"))
                .clicked()
            {
                actions.push(FrontendAction::QuickNoteShortcutHidden(true));
            }
            ui.label(
                egui::RichText::new(tr_l10n(lang, "quickNote.shortcutDesc"))
                    .size(11.0)
                    .color(theme::INK_4),
            );
            ui.add_space(8.0);
            let row = settings::ShortcutRow::new(
                ShortcutField::QuickNote,
                "",
                vm.quick_note_hotkey.clone(),
                true,
                String::new(),
            );
            settings::shortcut_control(ui, vm, actions, &row);
            settings::shortcut_menu(ui, vm, actions, &row);
        });
    ui.add_space(GAP);
}

// ── Header ──────────────────────────────────────────────────────────────────

fn header(
    ui: &mut egui::Ui,
    width: f32,
    lang: Lang,
    quick_note: bool,
    actions: &mut Vec<FrontendAction>,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 84.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(rect.left(), rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        tr_l10n(
            lang,
            if quick_note {
                "quickNote.kicker"
            } else {
                "history.kicker"
            },
        ),
        egui::FontId::proportional(11.0),
        theme::INK_4,
    );
    painter.text(
        egui::pos2(rect.left(), rect.top() + 18.0),
        egui::Align2::LEFT_TOP,
        tr_l10n(
            lang,
            if quick_note {
                "quickNote.title"
            } else {
                "history.title"
            },
        ),
        egui::FontId::proportional(26.0),
        theme::INK,
    );
    painter.text(
        egui::pos2(rect.left(), rect.top() + 56.0),
        egui::Align2::LEFT_TOP,
        tr_l10n(
            lang,
            if quick_note {
                "quickNote.desc"
            } else {
                "history.desc"
            },
        ),
        egui::FontId::proportional(13.0),
        theme::INK_3,
    );

    let clear = tr_l10n(lang, "common.clear");
    let refresh = tr_l10n(lang, "common.refresh");
    let clear_width = if quick_note {
        0.0
    } else {
        layout::text_width(ui, clear, 12.5) + 40.0
    };
    let refresh_width = layout::text_width(ui, refresh, 12.5) + 40.0;
    let top = rect.top() + 22.0;
    let refresh_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - refresh_width - if quick_note { 0.0 } else { clear_width + 8.0 },
            top,
        ),
        egui::vec2(refresh_width, 30.0),
    );
    if layout::action_button(
        ui,
        refresh_rect,
        refresh,
        Some(IconName::Refresh),
        ButtonKind::Ghost,
    )
    .clicked()
    {
        actions.push(FrontendAction::HistoryRefresh);
    }
    // Tauri's QuickNote history has only Refresh. Recording is controlled by
    // the configured hotkey, not a Start/Finish button in this page header.
    if !quick_note {
        let clear_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - clear_width, top),
            egui::vec2(clear_width, 30.0),
        );
        if layout::action_button(
            ui,
            clear_rect,
            clear,
            Some(IconName::Trash),
            ButtonKind::Ghost,
        )
        .clicked()
        {
            actions.push(FrontendAction::HistoryRequestClear);
        }
    }
}

// ── List ────────────────────────────────────────────────────────────────────

/// Indices into `history_entries` that match the current search query.
fn filtered_indices(vm: &FrontendViewModel, quick_notes_only: bool) -> Vec<usize> {
    let query = vm.history_query.trim().to_lowercase();
    vm.history_entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            (!quick_notes_only || entry.quick_note)
                && (query.is_empty()
                    || entry.raw_transcript.to_lowercase().contains(&query)
                    || entry.final_text.to_lowercase().contains(&query))
        })
        .map(|(index, _)| index)
        .collect()
}

fn selected_index(vm: &FrontendViewModel, filtered: &[usize]) -> Option<usize> {
    if filtered.contains(&vm.history_selected) {
        Some(vm.history_selected)
    } else {
        filtered.first().copied()
    }
}

fn list_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vm: &mut FrontendViewModel,
    filtered: &[usize],
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    paint_card(ui.painter(), rect);
    layout::fixed_ui(ui, rect, ("openless-history-list-card",), |ui| {
        // Sticky search box.
        let padding = 14.0;
        let search_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + padding, rect.top() + 12.0),
            egui::vec2((rect.width() - padding * 2.0).max(1.0), 34.0),
        );
        let painter = ui.painter().with_clip_rect(search_rect);
        painter.rect_filled(search_rect, egui::CornerRadius::same(8), theme::SURFACE_2);
        painter.rect_stroke(
            search_rect,
            egui::CornerRadius::same(8),
            egui::Stroke::new(0.8, theme::LINE),
            egui::StrokeKind::Inside,
        );
        let search_id = egui::Id::new("openless-history-search");
        if ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::K)) {
            ui.memory_mut(|memory| memory.request_focus(search_id));
        }
        let inner = search_rect.shrink2(egui::vec2(10.0, 5.0));
        layout::fixed_ui(ui, inner, ("openless-history-search-inner",), |ui| {
            ui.horizontal(|ui| {
                let (icon_rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 24.0), egui::Sense::hover());
                icons::draw_icon(ui, icon_rect.center(), IconName::Search, theme::INK_3);
                ui.add_space(6.0);
                let hint = fmt_l10n(lang, "history.search_placeholder", &[&"Ctrl+K"]);
                ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::TextEdit::singleline(&mut vm.history_query)
                        .id(search_id)
                        .hint_text(hint)
                        .text_color(theme::INK)
                        .font(egui::FontId::proportional(12.5))
                        .vertical_align(egui::Align::Center)
                        .frame(egui::Frame::NONE),
                );
            });
        });

        // Independently scrolling list below the search box.
        let list_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 6.0, search_rect.bottom() + 6.0),
            egui::pos2(rect.right() - 6.0, rect.bottom() - 6.0),
        );
        layout::fixed_ui(ui, list_rect, ("openless-history-list-scroll",), |ui| {
            let scroll_output = egui::ScrollArea::vertical()
                .id_salt("openless-history-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width = ui.available_width();
                    if vm.history_loading {
                        hint(ui, width, tr_l10n(lang, "common.loading"));
                        return;
                    }
                    if let Some(error) = vm.history_error.as_deref() {
                        let message = fmt_l10n(lang, "history.load_failed", &[&error]);
                        if hint_with_action(ui, width, &message, tr_l10n(lang, "common.retry")) {
                            actions.push(FrontendAction::HistoryRefresh);
                        }
                        return;
                    }
                    if filtered.is_empty() {
                        let query = vm.history_query.trim();
                        let message = if query.is_empty() {
                            tr_l10n(lang, "history.empty").to_string()
                        } else {
                            fmt_l10n(lang, "history.search_no_match", &[&query])
                        };
                        hint(ui, width, &message);
                        return;
                    }
                    for &index in filtered {
                        let selected = Some(index) == selected_index(vm, filtered);
                        row(
                            ui,
                            &vm.history_entries[index],
                            index,
                            selected,
                            lang,
                            actions,
                        );
                    }
                });
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("history-list-scroll-measure"),
                    (
                        scroll_output.content_size.y,
                        scroll_output.inner_rect.height(),
                        scroll_output.state.offset.y,
                    ),
                )
            });
            #[cfg(not(test))]
            let _ = scroll_output;
        });
    });
}

fn hint(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 56.0), egui::Sense::hover());
    ui.painter().with_clip_rect(rect).text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(12.0),
        theme::INK_4,
    );
}

/// Hint with a trailing action button; returns whether the button was clicked.
fn hint_with_action(ui: &mut egui::Ui, width: f32, text: &str, action: &str) -> bool {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 96.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    let galley = layout_text(ui, text, theme::INK_4, 12.0, (width - 12.0).max(1.0), 4);
    painter.galley(
        egui::pos2(rect.left() + 6.0, rect.top() + 12.0),
        galley.clone(),
        theme::INK_4,
    );
    let button_width = layout::text_width(ui, action, 12.5) + 30.0;
    let button_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + 6.0,
            rect.top() + 12.0 + galley.size().y + 10.0,
        ),
        egui::vec2(button_width, 28.0),
    );
    layout::action_button(ui, button_rect, action, None, ButtonKind::Ghost).clicked()
}

fn row(
    ui: &mut egui::Ui,
    entry: &HistoryEntry,
    index: usize,
    selected: bool,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    let width = ui.available_width();
    let preview_text = entry.final_text.split('\n').next().unwrap_or("");
    let preview_text = if preview_text.trim().is_empty() {
        entry.raw_transcript.split('\n').next().unwrap_or("")
    } else {
        preview_text
    };
    let preview_text = if preview_text.is_empty() && entry.quick_note {
        tr_l10n(
            lang,
            if entry.error_code.as_deref() == Some("recording") {
                "quickNote.recording"
            } else {
                "quickNote.noTranscript"
            },
        )
    } else {
        preview_text
    };
    let preview = if preview_text.is_empty() {
        None
    } else {
        Some(layout_text(
            ui,
            preview_text,
            theme::INK_2,
            12.0,
            (width - 24.0).max(1.0),
            2,
        ))
    };
    let preview_height = preview.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let row_height = 10.0
        + 15.0
        + if preview_height > 0.0 {
            4.0 + preview_height
        } else {
            0.0
        }
        + 6.0
        + 18.0
        + 10.0;

    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, row_height), egui::Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    if selected {
        painter.rect_filled(rect, egui::CornerRadius::same(10), theme::SURFACE_2);
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(10),
            egui::Stroke::new(0.5, theme::LINE),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, egui::CornerRadius::same(10), hover_fill());
    }

    let header_y = rect.top() + 10.0 + 7.5;
    painter.text(
        egui::pos2(rect.left() + 12.0, header_y),
        egui::Align2::LEFT_CENTER,
        format::time_label(&entry.created_at),
        egui::FontId::monospace(MONO_SMALL),
        theme::INK_3,
    );
    painter.text(
        egui::pos2(rect.right() - 12.0, header_y - 0.5),
        egui::Align2::RIGHT_CENTER,
        format::history_duration(entry.duration_ms, lang),
        egui::FontId::monospace(10.0),
        theme::INK_4,
    );
    let mut y = rect.top() + 10.0 + 15.0;
    if let Some(preview) = preview {
        y += 4.0;
        painter.galley(egui::pos2(rect.left() + 12.0, y), preview, theme::INK_2);
        y += preview_height + 6.0;
    } else {
        y += 6.0;
    }
    let pill = layout::pill_size(ui, &entry.style_label);
    let pill_rect = egui::Rect::from_min_size(egui::pos2(rect.left() + 12.0, y), pill);
    layout::paint_pill(
        &painter,
        pill_rect,
        &entry.style_label,
        if entry.mode == OverviewMode::Raw {
            PillTone::Outline
        } else {
            PillTone::Gray
        },
    );

    if response.clicked() {
        actions.push(FrontendAction::HistorySelect(index));
    }
    ui.add_space(4.0);
}

// ── Detail ──────────────────────────────────────────────────────────────────

fn detail_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vm: &FrontendViewModel,
    filtered: &[usize],
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    paint_card(ui.painter(), rect);
    let selected = selected_index(vm, filtered);
    layout::fixed_ui(
        ui,
        rect.shrink(CARD_PADDING),
        ("openless-history-detail",),
        |ui| {
            let scroll_output = egui::ScrollArea::vertical()
                .id_salt("openless-history-detail-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width = ui.available_width();
                    if vm.history_loading && vm.history_entries.is_empty() {
                        hint(ui, width, tr_l10n(lang, "common.loading"));
                        return;
                    }
                    let Some(index) = selected else {
                        let message = if let Some(error) = vm.history_error.as_deref() {
                            fmt_l10n(lang, "history.load_failed", &[&error])
                        } else {
                            tr_l10n(lang, "history.select_hint").to_string()
                        };
                        hint(ui, width, &message);
                        return;
                    };
                    detail_body(
                        ui,
                        &vm.history_entries[index],
                        index,
                        vm.history_playback.as_ref(),
                        lang,
                        actions,
                    );
                    if vm.history_repolish_open {
                        ui.add_space(12.0);
                        ui.separator();
                        ui.label(
                            egui::RichText::new(tr_l10n(lang, "history.repolish.title")).strong(),
                        );
                        ui.label(
                            egui::RichText::new(tr_l10n(lang, "history.repolish.hint"))
                                .size(11.5)
                                .color(theme::INK_4),
                        );
                        if let Some(error) = &vm.history_repolish_error {
                            ui.colored_label(theme::ERR, error);
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(
                                    !vm.history_repolish_running,
                                    egui::Button::new(tr_l10n(lang, "history.repolish.retry")),
                                )
                                .clicked()
                            {
                                actions.push(FrontendAction::HistoryRepolish(index, None));
                            }
                            for (pack_index, pack) in vm.style_packs.iter().enumerate() {
                                if pack.enabled
                                    && pack.id != "builtin.raw"
                                    && ui
                                        .add_enabled(
                                            !vm.history_repolish_running,
                                            egui::Button::new(&pack.name),
                                        )
                                        .clicked()
                                {
                                    actions.push(FrontendAction::HistoryRepolish(
                                        index,
                                        Some(pack_index),
                                    ));
                                }
                            }
                            if ui.button(tr_l10n(lang, "common.close")).clicked() {
                                actions.push(FrontendAction::HistoryRepolishClose);
                            }
                        });
                        if vm.history_repolish_running {
                            ui.spinner();
                        }
                        if let Some((id, text)) = &vm.history_repolish_result {
                            if id == &vm.history_entries[index].id {
                                ui.label(
                                    egui::RichText::new(tr_l10n(
                                        lang,
                                        "history.repolish.retryResultTitle",
                                    ))
                                    .strong(),
                                );
                                ui.label(if text.trim().is_empty() {
                                    tr_l10n(lang, "history.repolish.empty")
                                } else {
                                    text
                                });
                            }
                        }
                    }
                });
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("history-detail-scroll-measure"),
                    (
                        scroll_output.content_size.y,
                        scroll_output.inner_rect.height(),
                        scroll_output.state.offset.y,
                    ),
                )
            });
            #[cfg(not(test))]
            let _ = scroll_output;
        },
    );
}

fn detail_body(
    ui: &mut egui::Ui,
    entry: &HistoryEntry,
    index: usize,
    playback: Option<&super::view_model::HistoryPlayback>,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    let width = ui.available_width();

    // Header: time · style pill · recording length, actions on the right.
    let (top, _) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(top);
    let time = format::time_label(&entry.created_at);
    let mut x = top.left();
    painter.text(
        egui::pos2(x, top.center().y),
        egui::Align2::LEFT_CENTER,
        &time,
        egui::FontId::monospace(13.0),
        theme::INK_3,
    );
    x += layout::text_width(ui, &time, 13.0) + 10.0;
    let pill = layout::pill_size(ui, &entry.style_label);
    let pill_rect = egui::Rect::from_min_size(egui::pos2(x, top.center().y - pill.y / 2.0), pill);
    layout::paint_pill(&painter, pill_rect, &entry.style_label, PillTone::Gray);
    x += pill.x + 10.0;
    let recorded = fmt_l10n(
        lang,
        "history.recorded",
        &[&format::history_duration(entry.duration_ms, lang)],
    );
    painter.text(
        egui::pos2(x, top.center().y),
        egui::Align2::LEFT_CENTER,
        &recorded,
        egui::FontId::proportional(11.0),
        theme::INK_4,
    );

    // Right-aligned: 「…」录音操作菜单。Tauri 把删除/重新转录/导出/播放收进一个
    // `HistoryActionMenu`（右对齐浮层、向下展开），不再平铺一排按钮。
    let more_size = 30.0;
    let more_rect = egui::Rect::from_min_size(
        egui::pos2(top.right() - more_size, top.center().y - more_size / 2.0),
        egui::vec2(more_size, more_size),
    );
    let menu_id = egui::Id::new(("openless-history-menu", entry.id.as_str()));
    let mut menu_open = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(menu_id).unwrap_or(false));
    if layout::action_button(ui, more_rect, "", Some(IconName::More), ButtonKind::Ghost).clicked() {
        menu_open = !menu_open;
        ui.ctx()
            .data_mut(|data| data.insert_temp(menu_id, menu_open));
    }

    // Start/stop remains in the action menu. Once started, both History and Quick Note
    // share this pause/resume button, draggable seek bar and live elapsed clock.
    if entry.has_audio && playback.is_some_and(|clip| clip.id == entry.id) {
        ui.add_space(10.0);
        let (play_row, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::hover());
        if let Some(playback) = playback.filter(|clip| clip.id == entry.id) {
            let button = egui::Rect::from_min_size(
                egui::pos2(play_row.left(), play_row.center().y - 14.0),
                egui::vec2(28.0, 28.0),
            );
            let toggle = ui.interact(
                button,
                ui.id().with(("playback-toggle", &entry.id)),
                egui::Sense::click(),
            );
            if toggle.hovered() {
                ui.painter()
                    .rect_filled(button, egui::CornerRadius::same(6), theme::BLUE_SOFT);
            }
            if playback.paused {
                icons::draw_icon(ui, button.center(), IconName::Play, theme::BLUE);
            } else {
                // Two filled bars are more legible than a 14px outline icon.
                for dx in [-4.0, 2.0] {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            button.center() + egui::vec2(dx, -6.0),
                            egui::vec2(3.0, 12.0),
                        ),
                        egui::CornerRadius::same(1),
                        theme::BLUE,
                    );
                }
            }
            if toggle
                .on_hover_text(tr_l10n(
                    lang,
                    if playback.paused {
                        "history.resume"
                    } else {
                        "history.pause"
                    },
                ))
                .clicked()
            {
                actions.push(FrontendAction::HistoryPauseToggle);
            }
            let clock = format!(
                "{} / {}",
                playback_clock(playback.position_ms),
                playback_clock(playback.total_ms),
            );
            let clock_width = layout::text_width(ui, &clock, 11.0) + 6.0;
            let track = egui::Rect::from_min_max(
                egui::pos2(play_row.left() + 34.0, play_row.center().y - 3.0),
                egui::pos2(
                    play_row.right() - clock_width - 8.0,
                    play_row.center().y + 3.0,
                ),
            );
            ui.painter().text(
                egui::pos2(play_row.right(), play_row.center().y),
                egui::Align2::RIGHT_CENTER,
                clock,
                egui::FontId::monospace(11.0),
                theme::INK_4,
            );
            if track.width() > 20.0 {
                let hit = track.expand2(egui::vec2(0.0, 10.0));
                let response = ui.interact(
                    hit,
                    ui.id().with(("playback-seek", &entry.id)),
                    egui::Sense::click_and_drag(),
                );
                let seek = if response.dragged() || response.clicked() {
                    response
                        .interact_pointer_pos()
                        .map(|pos| seek_position_ms(pos.x, track, playback.total_ms))
                } else {
                    None
                };
                if let Some(ms) = seek {
                    actions.push(FrontendAction::HistorySeek(ms));
                }
                let displayed_ms = seek.unwrap_or(playback.position_ms);
                let ratio = if playback.total_ms == 0 {
                    0.0
                } else {
                    (displayed_ms as f32 / playback.total_ms as f32).clamp(0.0, 1.0)
                };
                ui.painter()
                    .rect_filled(track, egui::CornerRadius::same(3), theme::SURFACE_2);
                let filled = egui::Rect::from_min_max(
                    track.min,
                    egui::pos2(track.left() + track.width() * ratio, track.bottom()),
                );
                ui.painter()
                    .rect_filled(filled, egui::CornerRadius::same(3), theme::BLUE);
                ui.painter().circle_filled(
                    egui::pos2(track.left() + track.width() * ratio, track.center().y),
                    5.0,
                    theme::BLUE,
                );
                response.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
        }
    }

    ui.add_space(if playback.is_some_and(|clip| clip.id == entry.id) {
        12.0
    } else {
        4.0
    });
    separator(ui, width);
    ui.add_space(12.0);

    // 流水线明细：默认只留「识别」一行（用来语音识别的模型）。润色/插入属于前台
    // 投递细节，不在这块展示；润色信息由下方润色卡片的「润色 · 风格」胶囊承担。
    let label_column = layout::text_width(ui, tr_l10n(lang, "history.step_asr"), 11.0) + 14.0;
    let asr_detail = join_provider(&entry.asr_provider, &entry.asr_model);
    if !asr_detail.is_empty() || entry.asr_ms.is_some() {
        pipeline_row(
            ui,
            width,
            label_column,
            tr_l10n(lang, "history.step_asr"),
            &asr_detail,
            entry.asr_ms.map(|ms| format::step_duration(ms, lang)),
        );
    }

    // 结果卡片：默认只显示**润色结果**，原文按需展开（Tauri：「默认只显示润色
    // 结果；原文仍可按需展开，避免用户每次都面对两栏重复内容」）。
    ui.add_space(16.0);
    let styled_source = if entry.final_text.trim().is_empty() {
        entry.raw_transcript.as_str()
    } else {
        entry.final_text.as_str()
    };
    let styled_text = if styled_source.trim().is_empty() {
        tr_l10n(
            lang,
            if entry.quick_note {
                "quickNote.noTranscript"
            } else {
                "history.raw_empty"
            },
        )
    } else {
        styled_source
    };
    // 复制按钮始终在：润色失败时回退到原文（Tauri 的 `onCopy` 也是这个回退）。
    let styled_copy = Some(("openless-history-copy-styled", styled_source.to_string()));
    let raw_text = entry.raw_transcript.as_str();
    let raw_is_empty = raw_text.trim().is_empty();
    let raw_copy = (!raw_is_empty).then(|| ("openless-history-copy-raw", raw_text.to_string()));
    let raw_override = raw_is_empty.then(|| tr_l10n(lang, "history.raw_empty").to_string());

    // 「查看原文 / 隐藏原文」开关：状态挂在条目 id 上（Tauri 是组件 state，
    // 切条目就重置；这里按条目记，行为等价且省一次跳变）。
    let raw_id = egui::Id::new(("openless-history-raw", entry.id.as_str()));
    let mut raw_open = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(raw_id).unwrap_or(false));

    let text_width = (width - 12.0 - DETAIL_PADDING * 2.0).max(60.0);
    let styled_galley = layout_text(ui, styled_text, theme::INK, 13.0, text_width, 60);
    // 润色胶囊带上步骤名：Tauri 是「润色 · 风格名」。
    let styled_pill = format!(
        "{} · {}",
        tr_l10n(lang, "history.step_polish"),
        entry.style_label
    );
    let styled_card_height = 52.0 + styled_galley.size().y;
    let (styled_card, _) =
        ui.allocate_exact_size(egui::vec2(width, styled_card_height), egui::Sense::hover());
    let toggled = text_card_with_action(
        ui,
        styled_card,
        &styled_pill,
        PillTone::Blue,
        styled_text,
        None,
        styled_copy,
        lang,
        (!raw_is_empty).then(|| {
            if raw_open {
                tr_l10n(lang, "history.hide_raw")
            } else {
                tr_l10n(lang, "history.show_raw")
            }
        }),
    );
    if toggled {
        raw_open = !raw_open;
        ui.ctx().data_mut(|data| data.insert_temp(raw_id, raw_open));
    }

    if raw_open {
        ui.add_space(12.0);
        let raw_galley = layout_text(
            ui,
            raw_override.as_deref().unwrap_or(raw_text),
            theme::INK_2,
            13.0,
            text_width,
            60,
        );
        let card_height = 52.0 + raw_galley.size().y;
        let (raw_card, _) =
            ui.allocate_exact_size(egui::vec2(width, card_height), egui::Sense::hover());
        text_card_with_action(
            ui,
            raw_card,
            tr_l10n(lang, "history.step_asr"),
            PillTone::Outline,
            raw_text,
            raw_override,
            raw_copy,
            lang,
            None,
        );
    }

    // 「…」菜单浮层：右对齐、向下展开（Tauri 的 `HistoryActionMenu`：
    // `top: calc(100% + 6px); right: 0`）。
    if menu_open {
        let menu_rect = history_action_menu(ui, more_rect, entry, index, lang, actions);
        let clicked_outside = ui.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| !menu_rect.contains(pos) && !more_rect.contains(pos))
        });
        if clicked_outside || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            ui.ctx().data_mut(|data| data.insert_temp(menu_id, false));
        }
    }
}

/// 「…」菜单宽度 / 行高（Tauri：`width: min(260px, ...)`，行内边距 8×9）。
const MENU_WIDTH: f32 = 208.0;
const MENU_ITEM_HEIGHT: f32 = 31.0;
const MENU_PADDING: f32 = 6.0;

enum HistoryMenuItem {
    Separator,
    Action {
        icon: IconName,
        label: String,
        danger: bool,
        action: FrontendAction,
    },
}

/// 详情头部右侧的「…」菜单：播放、导出、重新转录、重新润色、删除。
fn history_action_menu(
    ui: &mut egui::Ui,
    button: egui::Rect,
    entry: &HistoryEntry,
    index: usize,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) -> egui::Rect {
    let mut items: Vec<HistoryMenuItem> = Vec::new();
    if entry.has_audio {
        items.push(HistoryMenuItem::Action {
            icon: IconName::Play,
            label: tr_l10n(lang, "history.play_recording").to_string(),
            danger: false,
            action: FrontendAction::HistoryPlay(index),
        });
        items.push(HistoryMenuItem::Action {
            icon: IconName::Download,
            label: tr_l10n(lang, "history.export").to_string(),
            danger: false,
            action: FrontendAction::HistoryExport(index),
        });
        items.push(HistoryMenuItem::Separator);
        items.push(HistoryMenuItem::Action {
            icon: IconName::Refresh,
            label: tr_l10n(lang, "history.retranscribe").to_string(),
            danger: false,
            action: FrontendAction::HistoryRetranscribe(index),
        });
    }
    if !entry.raw_transcript.trim().is_empty() {
        items.push(HistoryMenuItem::Action {
            icon: IconName::Sparkle,
            label: tr_l10n(lang, "history.repolish.title").to_string(),
            danger: false,
            action: FrontendAction::HistoryRepolishOpen(index),
        });
        items.push(HistoryMenuItem::Separator);
    }
    items.push(HistoryMenuItem::Action {
        icon: IconName::Trash,
        label: tr_l10n(lang, "common.delete").to_string(),
        danger: true,
        action: FrontendAction::HistoryRequestDelete(index),
    });

    let height = MENU_PADDING * 2.0
        + items
            .iter()
            .map(|item| match item {
                HistoryMenuItem::Separator => 13.0,
                HistoryMenuItem::Action { .. } => MENU_ITEM_HEIGHT,
            })
            .sum::<f32>();
    let rect = egui::Rect::from_min_size(
        egui::pos2(button.right() - MENU_WIDTH, button.bottom() + 6.0),
        egui::vec2(MENU_WIDTH, height),
    );

    egui::Area::new(egui::Id::new((
        "openless-history-menu-area",
        entry.id.as_str(),
    )))
    .order(egui::Order::Foreground)
    .fixed_pos(rect.min)
    .show(ui.ctx(), |ui| {
        ui.set_min_size(rect.size());
        ui.set_max_size(rect.size());
        let painter = ui.painter().clone();
        painter.rect_filled(rect, egui::CornerRadius::same(10), theme::SURFACE);
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(10),
            egui::Stroke::new(0.8, theme::LINE_STRONG),
            egui::StrokeKind::Inside,
        );
        let mut y = rect.top() + MENU_PADDING;
        for item in items {
            match item {
                HistoryMenuItem::Separator => {
                    painter.line_segment(
                        [
                            egui::pos2(rect.left() + 10.0, y + 6.0),
                            egui::pos2(rect.right() - 10.0, y + 6.0),
                        ],
                        egui::Stroke::new(1.0, theme::LINE_SOFT),
                    );
                    y += 13.0;
                }
                HistoryMenuItem::Action {
                    icon,
                    label,
                    danger,
                    action,
                } => {
                    let item_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + MENU_PADDING, y),
                        egui::vec2(MENU_WIDTH - MENU_PADDING * 2.0, MENU_ITEM_HEIGHT - 2.0),
                    );
                    let response = ui.interact(
                        item_rect,
                        ui.id().with(label.as_str()),
                        egui::Sense::click(),
                    );
                    if response.hovered() {
                        painter.rect_filled(
                            item_rect,
                            egui::CornerRadius::same(7),
                            theme::SURFACE_2,
                        );
                    }
                    let color = if danger { theme::ERR } else { theme::INK_2 };
                    icons::draw_icon(
                        ui,
                        egui::pos2(item_rect.left() + 15.0, item_rect.center().y),
                        icon,
                        color,
                    );
                    painter.text(
                        egui::pos2(item_rect.left() + 30.0, item_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &label,
                        egui::FontId::proportional(12.5),
                        color,
                    );
                    if response.clicked() {
                        actions.push(action);
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                egui::Id::new(("openless-history-menu", entry.id.as_str())),
                                false,
                            )
                        });
                    }
                    y += MENU_ITEM_HEIGHT;
                }
            }
        }
    });
    rect
}

fn join_provider(provider: &Option<String>, model: &Option<String>) -> String {
    [provider.as_deref(), model.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// One `label | detail | status` row of the pipeline breakdown.
fn pipeline_row(
    ui: &mut egui::Ui,
    width: f32,
    label_column: f32,
    label: &str,
    detail: &str,
    status: Option<String>,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.0),
        theme::INK_4,
    );
    painter.text(
        egui::pos2(rect.left() + label_column, rect.center().y),
        egui::Align2::LEFT_CENTER,
        detail,
        egui::FontId::monospace(11.0),
        theme::INK_2,
    );
    if let Some(status) = status {
        painter.text(
            egui::pos2(rect.right(), rect.center().y),
            egui::Align2::RIGHT_CENTER,
            status,
            egui::FontId::monospace(11.0),
            theme::INK_4,
        );
    }
    ui.add_space(4.0);
}

/// A `原文` / polished text card with an optional pill, copy button and an
/// optional secondary toggle (used for 查看原文 / 隐藏原文). Returns whether the
/// toggle was clicked.
#[allow(clippy::too_many_arguments)]
fn text_card_with_action(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    pill_text: &str,
    pill_tone: PillTone,
    body: &str,
    empty_override: Option<String>,
    copy: Option<(&'static str, String)>,
    lang: Lang,
    toggle: Option<&str>,
) -> bool {
    if rect == egui::Rect::NOTHING {
        return false;
    }
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), theme::SURFACE_2);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(0.5, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let painter = ui.painter().with_clip_rect(rect);
    let pill = layout::pill_size(ui, pill_text);
    let pill_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + DETAIL_PADDING, rect.top() + DETAIL_PADDING),
        pill,
    );
    layout::paint_pill(&painter, pill_rect, pill_text, pill_tone);

    let mut toggled = false;
    let mut right = rect.right() - DETAIL_PADDING;
    if let Some((salt, text)) = copy {
        let id = egui::Id::new(salt);
        let now = ui.input(|input| input.time);
        let copied = ui
            .ctx()
            .data(|data| data.get_temp::<f64>(id))
            .is_some_and(|at| now - at < 1.5);
        let label = if copied {
            tr_l10n(lang, "common.copied")
        } else {
            tr_l10n(lang, "common.copy")
        };
        let button_width = layout::text_width(ui, label, 12.5) + 34.0;
        let button_rect = egui::Rect::from_min_size(
            egui::pos2(right - button_width, rect.top() + DETAIL_PADDING - 3.0),
            egui::vec2(button_width, 26.0),
        );
        right = button_rect.left() - 6.0;
        if layout::action_button(
            ui,
            button_rect,
            label,
            Some(IconName::Copy),
            ButtonKind::Ghost,
        )
        .clicked()
        {
            ui.ctx().copy_text(text);
            ui.ctx().data_mut(|data| data.insert_temp(id, now));
        }
    }
    if let Some(label) = toggle {
        let button_width = layout::text_width(ui, label, 12.5) + 20.0;
        let button_rect = egui::Rect::from_min_size(
            egui::pos2(right - button_width, rect.top() + DETAIL_PADDING - 3.0),
            egui::vec2(button_width, 26.0),
        );
        toggled = layout::action_button(ui, button_rect, label, None, ButtonKind::Ghost).clicked();
    }

    let text = empty_override.as_deref().unwrap_or(body);
    let color = if empty_override.is_some() {
        theme::INK_4
    } else {
        theme::INK_2
    };
    let galley = layout_text(
        ui,
        text,
        color,
        13.0,
        (rect.width() - DETAIL_PADDING * 2.0).max(1.0),
        60,
    );
    painter.galley(
        egui::pos2(rect.left() + DETAIL_PADDING, pill_rect.bottom() + 10.0),
        galley,
        color,
    );
    toggled
}

// ── Confirmation dialog ─────────────────────────────────────────────────────

fn confirm_overlay(
    ctx: &egui::Context,
    body: egui::Rect,
    vm: &FrontendViewModel,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    egui::Area::new(egui::Id::new("openless-history-confirm"))
        .order(egui::Order::Foreground)
        // `click` 而不是 `hover`：遮罩必须**吃掉**点击，否则点遮罩会漏到下方页面（列表行
        // 会在确认框打开时被选中）。Tauri 这里用的是原生 `window.confirm()`——点外面
        // 不会关闭，所以只拦截、不关闭。
        .sense(egui::Sense::click())
        .fixed_pos(body.min)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            ui.set_clip_rect(body);
            ui.painter().rect_filled(
                body,
                egui::CornerRadius::ZERO,
                egui::Color32::from_black_alpha(36),
            );

            let dialog = egui::Rect::from_center_size(
                body.center(),
                egui::vec2(body.width().min(400.0), 152.0),
            );
            // 对话框矩形写进 memory 供测试查询（Area 覆盖整个 body，面积已不等于卡片）。
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("openless-history-confirm-card-rect"), dialog)
            });
            paint_card(ui.painter(), dialog);
            let painter = ui.painter().with_clip_rect(dialog);
            let message = match vm.history_confirm {
                Some(HistoryConfirm::Clear) => {
                    fmt_l10n(lang, "history.confirm_clear", &[&vm.history_entries.len()])
                }
                Some(HistoryConfirm::Delete(_)) => {
                    tr_l10n(lang, "history.confirm_delete").to_string()
                }
                None => return,
            };
            let galley = layout_text(
                ui,
                &message,
                theme::INK_2,
                13.0,
                (dialog.width() - 40.0).max(1.0),
                4,
            );
            painter.galley(
                egui::pos2(dialog.left() + 20.0, dialog.top() + 22.0),
                galley,
                theme::INK_2,
            );

            let confirm = tr_l10n(lang, "common.confirm");
            let cancel = tr_l10n(lang, "common.cancel");
            let confirm_width = layout::text_width(ui, confirm, 12.5) + 34.0;
            let cancel_width = layout::text_width(ui, cancel, 12.5) + 34.0;
            let button_y = dialog.bottom() - 20.0 - 30.0;
            let confirm_rect = egui::Rect::from_min_size(
                egui::pos2(dialog.right() - 20.0 - confirm_width, button_y),
                egui::vec2(confirm_width, 30.0),
            );
            let cancel_rect = egui::Rect::from_min_size(
                egui::pos2(confirm_rect.left() - 8.0 - cancel_width, button_y),
                egui::vec2(cancel_width, 30.0),
            );
            if layout::action_button(ui, cancel_rect, cancel, None, ButtonKind::Ghost).clicked() {
                actions.push(FrontendAction::HistoryCancelConfirm);
            }
            if layout::action_button(ui, confirm_rect, confirm, None, ButtonKind::Blue).clicked() {
                actions.push(FrontendAction::HistoryConfirmAction);
            }
        });
}

// ── Painting helpers ────────────────────────────────────────────────────────

/// Convert a pointer position (including drags beyond either end) to a clip timestamp.
fn seek_position_ms(pointer_x: f32, track: egui::Rect, total_ms: u64) -> u64 {
    if track.width() <= 0.0 {
        return 0;
    }
    let fraction = ((pointer_x - track.left()) / track.width()).clamp(0.0, 1.0);
    (fraction as f64 * total_ms as f64).round() as u64
}

/// `m:ss` clock used by the in-app player bar.
fn playback_clock(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod playback_tests {
    use super::*;

    #[test]
    fn seek_uses_track_coordinates_and_clamps_drag_outside_it() {
        let track = egui::Rect::from_min_max(egui::pos2(100.0, 10.0), egui::pos2(300.0, 16.0));
        assert_eq!(seek_position_ms(100.0, track, 90_000), 0);
        assert_eq!(seek_position_ms(200.0, track, 90_000), 45_000);
        assert_eq!(seek_position_ms(400.0, track, 90_000), 90_000);
        assert_eq!(seek_position_ms(-50.0, track, 90_000), 0);
        assert_eq!(playback_clock(45_900), "0:45");
    }
}

fn paint_card(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, egui::CornerRadius::same(14), theme::SURFACE);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
}

fn separator(ui: &mut egui::Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(0.5, LINE_SOFT),
    );
}

fn layout_text(
    ui: &egui::Ui,
    text: &str,
    color: egui::Color32,
    size: f32,
    max_width: f32,
    max_rows: usize,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = max_rows;
    job.append(
        text,
        0.0,
        egui::text::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}
