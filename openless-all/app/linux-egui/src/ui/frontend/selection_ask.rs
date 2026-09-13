//! Selection-ask (划词追问) page — port of the Tauri `pages/SelectionAsk.tsx`.
//!
//! Guide first, then the save-history switch, plus a shortcut-settings entry.

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::icons::{self, IconName};
use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, SettingsSection};

const GAP: f32 = 14.0;
const CARD_PADDING: f32 = 20.0;

pub fn page(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    if vm.selection_unsupported {
        layout::unsupported_page(ui, lang, tr_l10n(lang, "selection_ask.title"));
        return;
    }

    let header = layout::page_header(
        ui,
        width,
        tr_l10n(lang, "nav.selection_ask"),
        tr_l10n(lang, "selection_ask.title"),
        Some(tr_l10n(lang, "selection_ask.desc")),
    );
    // "Shortcut settings" entry, right-aligned on the header row.
    let settings_label = tr_l10n(lang, "selection_ask.shortcut_settings");
    let settings_width = layout::text_width(ui, settings_label, 12.5) + 46.0;
    let settings_rect = egui::Rect::from_min_size(
        egui::pos2(header.right() - settings_width, header.top() + 22.0),
        egui::vec2(settings_width, 30.0),
    );
    if layout::action_button(
        ui,
        settings_rect,
        settings_label,
        Some(IconName::Settings),
        layout::ButtonKind::Ghost,
    )
    .clicked()
    {
        actions.push(FrontendAction::ToggleSettings);
        actions.push(FrontendAction::SettingsSection(SettingsSection::Shortcuts));
    }
    ui.add_space(GAP);

    // ── Guide ───────────────────────────────────────────────────────────────
    let open_desc = if vm.qa_hotkey.trim().is_empty() {
        tr_l10n(lang, "selection_ask.guide_unset_desc").to_string()
    } else {
        fmt_l10n(lang, "selection_ask.guide_open_desc", &[&vm.qa_hotkey])
    };
    let ask_desc = fmt_l10n(
        lang,
        "selection_ask.guide_ask_desc",
        &[&vm.dictation_hotkey],
    );
    let steps = [
        (
            tr_l10n(lang, "selection_ask.guide_open_title").to_string(),
            open_desc,
        ),
        (
            tr_l10n(lang, "selection_ask.guide_select_title").to_string(),
            tr_l10n(lang, "selection_ask.howto_step2").to_string(),
        ),
        (
            tr_l10n(lang, "selection_ask.guide_ask_title").to_string(),
            ask_desc,
        ),
    ];
    // Height: padding + title + 3 steps (number column height) + footer.
    let step_height = 46.0;
    let guide_height = CARD_PADDING * 2.0 + 24.0 + steps.len() as f32 * step_height + 12.0 + 20.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, guide_height), egui::Sense::hover());
    layout::card(ui, rect, CARD_PADDING, |ui, inner| {
        let painter = ui.painter().with_clip_rect(inner);
        painter.text(
            inner.left_top(),
            egui::Align2::LEFT_TOP,
            tr_l10n(lang, "selection_ask.howto_title"),
            egui::FontId::proportional(15.0),
            theme::INK,
        );
        let mut y = inner.top() + 30.0;
        for (index, (title, desc)) in steps.iter().enumerate() {
            painter.text(
                egui::pos2(inner.left(), y + 1.0),
                egui::Align2::LEFT_TOP,
                format!("{:02}", index + 1),
                egui::FontId::monospace(12.0),
                theme::BLUE,
            );
            painter.text(
                egui::pos2(inner.left() + 34.0, y),
                egui::Align2::LEFT_TOP,
                title,
                egui::FontId::proportional(13.0),
                theme::INK,
            );
            let galley = layout::text_galley(
                ui,
                desc,
                theme::INK_3,
                12.0,
                (inner.width() - 34.0).max(1.0),
                3,
            );
            painter.galley(
                egui::pos2(inner.left() + 34.0, y + 18.0),
                galley,
                theme::INK_3,
            );
            y += step_height;
        }
        // Footer: follow-up hint and the Esc dismissal.
        let footer = format!(
            "{}    ·    Esc  {}",
            tr_l10n(lang, "selection_ask.guide_followup"),
            tr_l10n(lang, "selection_ask.guide_dismiss"),
        );
        painter.text(
            egui::pos2(inner.left(), inner.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            footer,
            egui::FontId::proportional(11.5),
            theme::INK_4,
        );
    });

    ui.add_space(GAP);

    // ── Save history ────────────────────────────────────────────────────────
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 76.0), egui::Sense::hover());
    layout::card(ui, rect, CARD_PADDING, |ui, inner| {
        let painter = ui.painter().with_clip_rect(inner);
        let icon_rect = egui::Rect::from_min_size(inner.left_top(), egui::vec2(24.0, 24.0));
        icons::draw_icon(
            ui,
            egui::pos2(icon_rect.center().x, icon_rect.center().y + 2.0),
            IconName::History,
            theme::INK_3,
        );
        painter.text(
            egui::pos2(inner.left() + 34.0, inner.top()),
            egui::Align2::LEFT_TOP,
            tr_l10n(lang, "selection_ask.history_title"),
            egui::FontId::proportional(13.0),
            theme::INK,
        );
        painter.text(
            egui::pos2(inner.left() + 34.0, inner.top() + 22.0),
            egui::Align2::LEFT_TOP,
            tr_l10n(lang, "selection_ask.history_desc"),
            egui::FontId::proportional(11.5),
            theme::INK_4,
        );
        let toggle_rect = egui::Rect::from_min_size(
            egui::pos2(inner.right() - 36.0, inner.center().y - 10.0),
            egui::vec2(36.0, 20.0),
        );
        if layout::toggle(
            ui,
            toggle_rect,
            vm.qa_save_history,
            "selection-ask-history-toggle",
        )
        .clicked()
        {
            actions.push(FrontendAction::SelectionAskToggleHistory);
        }
    });
}
