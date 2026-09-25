//! Selection-ask (划词追问) page — port of the Tauri `pages/SelectionAsk.tsx`.
//!
//! The guide is a plain section with three columns (01 / 02 / 03) followed by a
//! footer row, then the save-history card with its switch. The shortcut-settings
//! entry sits under the title on the right.

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n};

use super::icons::{self, IconName};
use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, SettingsSection};

const GAP: f32 = 16.0;
const CARD_PADDING: f32 = 20.0;
const COLUMN_GAP: f32 = 18.0;
const GUIDE_HEIGHT: f32 = 96.0;

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

    // Saved toast, right aligned above the shortcut-settings entry.
    if let Some(notice) = vm.settings_notice.clone() {
        let toast = format!("✓  {notice}");
        let size = layout::pill_size(ui, &toast);
        let rect = egui::Rect::from_min_size(
            egui::pos2(header.right() - size.x, header.top() + 4.0),
            size,
        );
        layout::paint_pill(ui.painter(), rect, &toast, layout::PillTone::Blue);
    }

    let settings_label = tr_l10n(lang, "selection_ask.shortcut_settings");
    let settings_width = layout::text_width(ui, settings_label, 12.5) + 60.0;
    let settings_rect = egui::Rect::from_min_size(
        egui::pos2(header.right() - settings_width, header.top() + 28.0),
        egui::vec2(settings_width, 30.0),
    );
    let settings_clicked = layout::action_button(
        ui,
        settings_rect,
        settings_label,
        Some(IconName::Settings),
        layout::ButtonKind::Ghost,
    )
    .clicked();
    icons::draw_icon(
        ui,
        egui::pos2(settings_rect.right() - 13.0, settings_rect.center().y),
        IconName::ChevronRight,
        theme::INK_3,
    );
    if settings_clicked {
        actions.push(FrontendAction::ToggleSettings);
        actions.push(FrontendAction::SettingsSection(SettingsSection::Shortcuts));
    }
    ui.add_space(GAP);

    guide(ui, width, vm);
    ui.add_space(GAP);
    save_history_card(ui, width, vm, actions);
}

/// Three-column usage guide plus its footer row.
fn guide(ui: &mut egui::Ui, width: f32, vm: &FrontendViewModel) {
    let lang = vm.lang;
    layout::section_title(ui, width, tr_l10n(lang, "selection_ask.howto_title"), None);
    ui.add_space(8.0);

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
        (tr_l10n(lang, "selection_ask.guide_open_title"), open_desc),
        (
            tr_l10n(lang, "selection_ask.guide_select_title"),
            tr_l10n(lang, "selection_ask.howto_step2").to_string(),
        ),
        (tr_l10n(lang, "selection_ask.guide_ask_title"), ask_desc),
    ];

    let (row, _) = ui.allocate_exact_size(egui::vec2(width, GUIDE_HEIGHT), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(row);
    let column_width = ((width - COLUMN_GAP * 2.0) / 3.0).max(80.0);
    for (index, (title, desc)) in steps.iter().enumerate() {
        let x = row.left() + index as f32 * (column_width + COLUMN_GAP);
        painter.text(
            egui::pos2(x, row.top()),
            egui::Align2::LEFT_TOP,
            format!("{:02}", index + 1),
            egui::FontId::monospace(11.0),
            theme::BLUE,
        );
        painter.text(
            egui::pos2(x + 22.0, row.top() - 1.0),
            egui::Align2::LEFT_TOP,
            title,
            egui::FontId::proportional(13.0),
            theme::INK,
        );
        let galley = layout::text_galley(
            ui,
            desc,
            theme::INK_3,
            11.5,
            (column_width - 22.0).max(1.0),
            3,
        );
        painter.galley(egui::pos2(x + 22.0, row.top() + 20.0), galley, theme::INK_3);
    }

    ui.add_space(10.0);
    let (footer, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(footer);
    icons::draw_icon(
        ui,
        egui::pos2(footer.left() + 7.0, footer.center().y),
        IconName::Refresh,
        theme::INK_4,
    );
    painter.text(
        egui::pos2(footer.left() + 22.0, footer.center().y),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "selection_ask.guide_followup"),
        egui::FontId::proportional(11.5),
        theme::INK_3,
    );

    // Dismissal hint on the right: an Esc key chip plus its description.
    let dismiss = tr_l10n(lang, "selection_ask.guide_dismiss");
    let dismiss_width = layout::text_width(ui, dismiss, 11.5);
    let dismiss_left = footer.right() - dismiss_width;
    painter.text(
        egui::pos2(dismiss_left, footer.center().y),
        egui::Align2::LEFT_CENTER,
        dismiss,
        egui::FontId::proportional(11.5),
        theme::INK_3,
    );
    let chip_text = "Esc";
    let chip_width = layout::text_width(ui, chip_text, 10.5) + 14.0;
    let chip = egui::Rect::from_min_size(
        egui::pos2(dismiss_left - 8.0 - chip_width, footer.center().y - 9.0),
        egui::vec2(chip_width, 18.0),
    );
    painter.rect_filled(
        chip.translate(egui::vec2(0.0, 2.0)),
        egui::CornerRadius::same(5),
        egui::Color32::from_black_alpha(14),
    );
    painter.rect_filled(chip, egui::CornerRadius::same(5), theme::SURFACE_2);
    painter.rect_stroke(
        chip,
        egui::CornerRadius::same(5),
        egui::Stroke::new(0.5, theme::LINE),
        egui::StrokeKind::Inside,
    );
    painter.text(
        chip.center(),
        egui::Align2::CENTER_CENTER,
        chip_text,
        egui::FontId::proportional(10.5),
        theme::INK_3,
    );
}

/// Save-history card: icon, title, description and the switch on the right.
fn save_history_card(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 92.0), egui::Sense::hover());
    layout::card(ui, rect, CARD_PADDING, |ui, inner| {
        let painter = ui.painter().with_clip_rect(inner);
        // Tauri `.ol-selection-ask-history-icon`：40×40 圆角方块 + 居中的 20px 图标。
        let icon_box = egui::Rect::from_min_size(
            egui::pos2(inner.left(), inner.center().y - 20.0),
            egui::vec2(40.0, 40.0),
        );
        painter.rect_filled(icon_box, egui::CornerRadius::same(10), theme::SURFACE_2);
        icons::draw_icon(ui, icon_box.center(), IconName::History, theme::INK_3);
        // gap 14（`.ol-selection-ask-history` 的 `gap`）。
        let text_left = icon_box.right() + 14.0;
        let toggle_rect = egui::Rect::from_min_size(
            egui::pos2(inner.right() - 40.0, inner.center().y - 12.0),
            egui::vec2(40.0, 24.0),
        );
        let text_width = (toggle_rect.left() - 14.0 - text_left).max(40.0);
        let title = layout::text_galley(
            ui,
            tr_l10n(lang, "selection_ask.history_title"),
            theme::INK,
            13.0,
            text_width,
            2,
        );
        let desc = layout::text_galley(
            ui,
            tr_l10n(lang, "selection_ask.history_desc"),
            theme::INK_3,
            12.0,
            text_width,
            3,
        );
        // 标题在上、说明在下，整块在卡片内垂直居中（`align-items: center`）。
        let title_height = title.size().y;
        let block_height = title_height + 5.0 + desc.size().y;
        let block_top = inner.center().y - block_height / 2.0;
        painter.galley(egui::pos2(text_left, block_top), title, theme::INK);
        painter.galley(
            egui::pos2(text_left, block_top + title_height + 5.0),
            desc,
            theme::INK_3,
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
