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
//! height the shell gives them, and each scrolls independently. Below
//! `STACK_WIDTH` the two cards stack vertically. All strings come from the
//! localization catalog; nothing is hardcoded.

use std::sync::Arc;

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::format;
use super::icons::{self, IconName};
use super::layout::{self, ButtonKind, PillTone};
use super::theme;
use super::view_model::{
    FrontendAction, FrontendViewModel, HistoryConfirm, HistoryEntry, HistoryInsertStatus,
    OverviewMode,
};

const GAP: f32 = 14.0;
const LIST_WIDTH: f32 = 300.0;
const STACK_WIDTH: f32 = 760.0;
const CARD_PADDING: f32 = 20.0;
const DETAIL_PADDING: f32 = 12.0;
const LINE_SOFT: egui::Color32 = theme::LINE_SOFT;
const MONO_SMALL: f32 = 11.0;

/// Faint hover wash for unselected rows.
fn hover_fill() -> egui::Color32 {
    theme::SURFACE_2
}

// ── Entry point ─────────────────────────────────────────────────────────────

pub fn page(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    header(ui, width, lang, actions);
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

    let filtered = filtered_indices(vm);
    list_card(ui, list_rect, vm, &filtered, lang, actions);
    detail_card(ui, detail_rect, vm, &filtered, lang, actions);

    if vm.history_confirm.is_some() {
        confirm_overlay(ui.ctx(), body, vm, lang, actions);
    }
}

// ── Header ──────────────────────────────────────────────────────────────────

fn header(ui: &mut egui::Ui, width: f32, lang: Lang, actions: &mut Vec<FrontendAction>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 84.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(rect.left(), rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        tr_l10n(lang, "history.kicker"),
        egui::FontId::proportional(11.0),
        theme::INK_4,
    );
    painter.text(
        egui::pos2(rect.left(), rect.top() + 18.0),
        egui::Align2::LEFT_TOP,
        tr_l10n(lang, "history.title"),
        egui::FontId::proportional(26.0),
        theme::INK,
    );
    painter.text(
        egui::pos2(rect.left(), rect.top() + 56.0),
        egui::Align2::LEFT_TOP,
        tr_l10n(lang, "history.desc"),
        egui::FontId::proportional(13.0),
        theme::INK_3,
    );

    let clear = tr_l10n(lang, "common.clear");
    let refresh = tr_l10n(lang, "common.refresh");
    let clear_width = layout::text_width(ui, clear, 12.5) + 40.0;
    let refresh_width = layout::text_width(ui, refresh, 12.5) + 40.0;
    let top = rect.top() + 22.0;
    let refresh_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - clear_width - 8.0 - refresh_width, top),
        egui::vec2(refresh_width, 30.0),
    );
    let clear_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - clear_width, top),
        egui::vec2(clear_width, 30.0),
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

// ── List ────────────────────────────────────────────────────────────────────

/// Indices into `history_entries` that match the current search query.
fn filtered_indices(vm: &FrontendViewModel) -> Vec<usize> {
    let query = vm.history_query.trim().to_lowercase();
    vm.history_entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            query.is_empty()
                || entry.raw_transcript.to_lowercase().contains(&query)
                || entry.final_text.to_lowercase().contains(&query)
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
            egui::ScrollArea::vertical()
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
            egui::ScrollArea::vertical()
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
                });
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

    // Right-aligned actions: 删除 / 重新转写 / 导出录音 (audio-gated).
    let mut right = top.right();
    let delete_label = tr_l10n(lang, "common.delete");
    let delete_width = layout::text_width(ui, delete_label, 12.5) + 38.0;
    let delete_rect = egui::Rect::from_min_size(
        egui::pos2(right - delete_width, top.center().y - 15.0),
        egui::vec2(delete_width, 30.0),
    );
    right = delete_rect.left() - 6.0;
    if layout::action_button(
        ui,
        delete_rect,
        delete_label,
        Some(IconName::Trash),
        ButtonKind::Ghost,
    )
    .clicked()
    {
        actions.push(FrontendAction::HistoryRequestDelete(index));
    }
    if entry.has_audio {
        let retranscribe = tr_l10n(lang, "history.retranscribe");
        let retranscribe_width = layout::text_width(ui, retranscribe, 12.5) + 38.0;
        let retranscribe_rect = egui::Rect::from_min_size(
            egui::pos2(right - retranscribe_width, top.center().y - 15.0),
            egui::vec2(retranscribe_width, 30.0),
        );
        right = retranscribe_rect.left() - 6.0;
        if layout::action_button(
            ui,
            retranscribe_rect,
            retranscribe,
            Some(IconName::Refresh),
            ButtonKind::Ghost,
        )
        .clicked()
        {
            actions.push(FrontendAction::HistoryRetranscribe(index));
        }
        let export = tr_l10n(lang, "history.export");
        let export_width = layout::text_width(ui, export, 12.5) + 38.0;
        let export_rect = egui::Rect::from_min_size(
            egui::pos2(right - export_width, top.center().y - 15.0),
            egui::vec2(export_width, 30.0),
        );
        if layout::action_button(
            ui,
            export_rect,
            export,
            Some(IconName::Download),
            ButtonKind::Ghost,
        )
        .clicked()
        {
            actions.push(FrontendAction::HistoryExport(index));
        }
    }

    // In-app playback: a player bar with the elapsed time and a progress track.
    if entry.has_audio {
        ui.add_space(10.0);
        let (play_row, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::hover());
        let playing = playback.filter(|playback| playback.id == entry.id);
        let label = if playing.is_some() {
            tr_l10n(lang, "history.stop_playback")
        } else {
            tr_l10n(lang, "history.play")
        };
        let icon = if playing.is_some() {
            IconName::Stop
        } else {
            IconName::Play
        };
        let button_width = layout::text_width(ui, label, 12.5) + 42.0;
        let button_rect = egui::Rect::from_min_size(play_row.min, egui::vec2(button_width, 32.0));
        if layout::action_button(ui, button_rect, label, Some(icon), ButtonKind::Ghost).clicked() {
            actions.push(FrontendAction::HistoryPlay(index));
        }
        if let Some(playback) = playing {
            // Progress track to the right of the button.
            let track = egui::Rect::from_min_max(
                egui::pos2(button_rect.right() + 12.0, play_row.center().y - 3.0),
                egui::pos2(play_row.right() - 96.0, play_row.center().y + 3.0),
            );
            if track.width() > 20.0 {
                let ratio = if playback.total_ms == 0 {
                    0.0
                } else {
                    (playback.position_ms as f32 / playback.total_ms as f32).clamp(0.0, 1.0)
                };
                ui.painter()
                    .rect_filled(track, egui::CornerRadius::same(3), theme::SURFACE_2);
                let filled = egui::Rect::from_min_max(
                    track.min,
                    egui::pos2(track.left() + track.width() * ratio, track.bottom()),
                );
                ui.painter()
                    .rect_filled(filled, egui::CornerRadius::same(3), theme::BLUE);
                ui.painter().text(
                    egui::pos2(play_row.right(), play_row.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!(
                        "{} / {}",
                        playback_clock(playback.position_ms),
                        playback_clock(playback.total_ms)
                    ),
                    egui::FontId::monospace(11.0),
                    theme::INK_4,
                );
            }
        }
    }

    ui.add_space(if entry.has_audio { 12.0 } else { 4.0 });
    separator(ui, width);
    ui.add_space(12.0);

    // Pipeline rows: 识别 / 润色 / 插入.
    let step_labels = [
        tr_l10n(lang, "history.step_asr"),
        tr_l10n(lang, "history.step_polish"),
        tr_l10n(lang, "history.step_insert"),
    ];
    let label_column = step_labels
        .iter()
        .map(|label| layout::text_width(ui, label, 11.0))
        .fold(0.0_f32, f32::max)
        + 14.0;

    let asr_detail = join_provider(&entry.asr_provider, &entry.asr_model);
    if !asr_detail.is_empty() || entry.asr_ms.is_some() {
        pipeline_row(
            ui,
            width,
            label_column,
            step_labels[0],
            &asr_detail,
            entry.asr_ms.map(|ms| format::step_duration(ms, lang)),
        );
    }
    let llm_detail = join_provider(&entry.llm_provider, &entry.llm_model);
    if !llm_detail.is_empty() || entry.polish_ms.is_some() {
        pipeline_row(
            ui,
            width,
            label_column,
            step_labels[1],
            &llm_detail,
            entry.polish_ms.map(|ms| format::step_duration(ms, lang)),
        );
    }
    let mut insert_detail = match &entry.app_name {
        Some(app) if !app.trim().is_empty() => format!("{app} · "),
        _ => String::new(),
    };
    insert_detail.push_str(&fmt_l10n(
        lang,
        "history.chars",
        &[&format::code_points(&entry.final_text)],
    ));
    if let Some(count) = entry.dictionary_count.filter(|count| *count > 0) {
        insert_detail.push_str(" · ");
        insert_detail.push_str(&fmt_l10n(lang, "history.vocab_hits", &[&count]));
    }
    pipeline_row(
        ui,
        width,
        label_column,
        step_labels[2],
        &insert_detail,
        Some(insert_status_label(lang, entry.insert_status)),
    );

    // 原文 / 润色结果 cards.
    ui.add_space(16.0);
    let remaining = ui.available_height().max(120.0);
    let raw_text = entry.raw_transcript.as_str();
    let styled_text = entry.final_text.as_str();
    let raw_empty = tr_l10n(lang, "history.raw_empty");
    let raw_body = if raw_text.trim().is_empty() {
        raw_empty
    } else {
        raw_text
    };
    let raw_galley = layout_text(
        ui,
        raw_body,
        theme::INK_2,
        13.0,
        (width / 2.0 - 60.0).max(60.0),
        60,
    );
    let styled_galley = layout_text(
        ui,
        styled_text,
        theme::INK,
        13.0,
        (width / 2.0 - 60.0).max(60.0),
        60,
    );
    let content_height = raw_galley.size().y.max(styled_galley.size().y);
    let card_height = remaining.max(52.0 + content_height);

    let (cards, _) = ui.allocate_exact_size(egui::vec2(width, card_height), egui::Sense::hover());
    let raw_label = tr_l10n(lang, "history.raw_label");
    let raw_is_empty = raw_text.trim().is_empty();
    let raw_override = if raw_is_empty {
        Some(tr_l10n(lang, "history.raw_empty").to_string())
    } else {
        None
    };
    let raw_copy = (!raw_is_empty).then(|| ("openless-history-copy-raw", raw_text.to_string()));
    let styled_is_empty = styled_text.trim().is_empty();
    let styled_copy =
        (!styled_is_empty).then(|| ("openless-history-copy-styled", styled_text.to_string()));

    if width >= 560.0 {
        let column_width = (width - 12.0) / 2.0;
        let left_rect = egui::Rect::from_min_size(cards.min, egui::vec2(column_width, card_height));
        let right_rect = egui::Rect::from_min_size(
            egui::pos2(cards.left() + column_width + 12.0, cards.top()),
            egui::vec2(column_width, card_height),
        );
        text_card(
            ui,
            left_rect,
            raw_label,
            PillTone::Outline,
            raw_text,
            raw_override,
            raw_copy,
            lang,
        );
        text_card(
            ui,
            right_rect,
            &entry.style_label,
            PillTone::Blue,
            styled_text,
            None,
            styled_copy,
            lang,
        );
    } else {
        text_card(
            ui,
            cards,
            raw_label,
            PillTone::Outline,
            raw_text,
            raw_override,
            raw_copy,
            lang,
        );
        ui.add_space(12.0);
        let (second, _) =
            ui.allocate_exact_size(egui::vec2(width, card_height), egui::Sense::hover());
        text_card(
            ui,
            second,
            &entry.style_label,
            PillTone::Blue,
            styled_text,
            None,
            styled_copy,
            lang,
        );
    }
}

fn join_provider(provider: &Option<String>, model: &Option<String>) -> String {
    [provider.as_deref(), model.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn insert_status_label(lang: Lang, status: HistoryInsertStatus) -> String {
    match status {
        HistoryInsertStatus::Inserted => tr_l10n(lang, "history.inserted").to_string(),
        HistoryInsertStatus::PasteSent => tr_l10n(lang, "history.paste_sent").to_string(),
        HistoryInsertStatus::CopiedFallback => {
            fmt_l10n(lang, "history.copied_fallback", &[&"Ctrl+V"])
        }
        HistoryInsertStatus::Failed => tr_l10n(lang, "history.insert_failed").to_string(),
        HistoryInsertStatus::NotRequested => tr_l10n(lang, "history.not_requested").to_string(),
    }
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

/// A `原文` / polished text card with an optional pill and copy button.
#[allow(clippy::too_many_arguments)]
fn text_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    pill_text: &str,
    pill_tone: PillTone,
    body: &str,
    empty_override: Option<String>,
    copy: Option<(&'static str, String)>,
    lang: Lang,
) {
    if rect == egui::Rect::NOTHING {
        return;
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
            egui::pos2(
                rect.right() - DETAIL_PADDING - button_width,
                rect.top() + DETAIL_PADDING - 3.0,
            ),
            egui::vec2(button_width, 26.0),
        );
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

/// `m:ss` clock used by the in-app player bar.
fn playback_clock(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
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
