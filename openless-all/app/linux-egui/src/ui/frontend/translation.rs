//! Translation page — port of the Tauri `pages/Translation.tsx`.
//!
//! Language search, a two-column working-language grid with checkboxes, the
//! target language and the inherited style on the right, then the usage guide.

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::icons::{self, IconName};
use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel};

const GAP: f32 = 12.0;
const CARD_PADDING: f32 = 18.0;

/// `(native name, language code)`. Core stores the native name, so selection
/// matching must keep using it; the code is the secondary label shown under the
/// name (the Tauri app resolves a localized name through `Intl.DisplayNames`,
/// which egui cannot use).
const SUPPORTED_LANGUAGES: [(&str, &str); 15] = [
    ("简体中文", "zh-Hans"),
    ("繁体中文", "zh-Hant"),
    ("English", "en"),
    ("日本語", "ja"),
    ("한국어", "ko"),
    ("Français", "fr"),
    ("Deutsch", "de"),
    ("Español", "es"),
    ("Italiano", "it"),
    ("Português", "pt"),
    ("Русский", "ru"),
    ("العربية", "ar"),
    ("Tiếng Việt", "vi"),
    ("ไทย", "th"),
    ("हिन्दी", "hi"),
];

pub fn page(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    // The shared page scroll area is height-bounded by the shell, so this page
    // only has to pin its own width. Pinning *both* min and max keeps the two
    // columns (and the four-column usage guide) from feeding a wider content
    // size back into the next frame's layout while a window is being resized.
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    if vm.translation_unsupported {
        layout::unsupported_page(ui, lang, tr_l10n(lang, "translation.title"));
        return;
    }

    layout::page_header(
        ui,
        width,
        tr_l10n(lang, "translation.kicker"),
        tr_l10n(lang, "translation.title"),
        Some(tr_l10n(lang, "translation.desc")),
    );
    ui.add_space(GAP);

    // Toolbar: language search on the left, selected count on the right.
    toolbar(ui, width, vm);
    ui.add_space(GAP);

    // 主窗口最低宽度依然保留左右两栏，不按内容区宽度误判为窄屏。
    {
        let column_width = (width - GAP) / 2.0;
        ui.columns(2, |columns| {
            // The left card drives the shared height so both columns end level.
            let left_height = working_languages(&mut columns[0], column_width, vm, actions);
            target_language(
                &mut columns[1],
                column_width,
                vm,
                actions,
                Some(left_height),
            );
        });
    }
    ui.add_space(GAP);
    usage(ui, width, vm);
}

fn toolbar(ui: &mut egui::Ui, width: f32, vm: &mut FrontendViewModel) {
    let lang = vm.lang;
    let count = vm.translation_working_languages.len();
    let count_text = fmt_l10n(lang, "translation.selected_languages", &[&count]);
    let count_width = layout::text_width(ui, &count_text, 11.5) + 4.0;
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::hover());
    let search = egui::Rect::from_min_max(
        row.min,
        egui::pos2(row.right() - count_width - 8.0, row.bottom()),
    );
    ui.painter()
        .rect_filled(search, egui::CornerRadius::same(17), theme::SURFACE);
    ui.painter().rect_stroke(
        search,
        egui::CornerRadius::same(17),
        egui::Stroke::new(0.8, theme::LINE),
        egui::StrokeKind::Inside,
    );
    icons::draw_icon(
        ui,
        egui::pos2(search.left() + 17.0, search.center().y),
        IconName::Search,
        theme::INK_4,
    );
    let input = egui::Rect::from_min_max(
        egui::pos2(search.left() + 33.0, search.top() + 5.0),
        egui::pos2(search.right() - 9.0, search.bottom() - 5.0),
    );
    ui.put(
        input,
        egui::TextEdit::singleline(&mut vm.translation_query)
            .id(egui::Id::new("openless-translation-search"))
            .hint_text(tr_l10n(lang, "translation.search_languages"))
            .text_color(theme::INK)
            .frame(egui::Frame::NONE)
            .vertical_align(egui::Align::Center),
    );
    ui.painter().text(
        egui::pos2(row.right(), row.center().y),
        egui::Align2::RIGHT_CENTER,
        count_text,
        egui::FontId::proportional(11.5),
        theme::INK_4,
    );
}

fn working_languages(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) -> f32 {
    let lang = vm.lang;
    let query = vm.translation_query.trim().to_lowercase();
    let visible: Vec<(&str, &str)> = SUPPORTED_LANGUAGES
        .iter()
        .copied()
        .filter(|(native, code)| {
            query.is_empty()
                || native.to_lowercase().contains(&query)
                || code.to_lowercase().contains(&query)
        })
        .collect();

    card(ui, width, |ui| {
        ui.label(
            egui::RichText::new(tr_l10n(lang, "translation.working_title"))
                .size(13.5)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(tr_l10n(lang, "translation.working_desc"))
                .size(11.5)
                .color(theme::INK_4),
        );
        ui.add_space(10.0);

        if visible.is_empty() {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "translation.no_matching_languages"))
                    .size(11.5)
                    .color(theme::INK_4),
            );
        } else {
            const ROW_HEIGHT: f32 = 46.0;
            const ROW_GAP: f32 = 8.0;
            let available = ui.available_width();
            let columns: usize = if available >= 560.0 { 3 } else { 2 };

            let cell_width = (available - ROW_GAP * (columns as f32 - 1.0)) / columns as f32;
            let mut index = 0;
            while index < visible.len() {
                let count = columns.min(visible.len() - index);
                let (row, _) =
                    ui.allocate_exact_size(egui::vec2(available, ROW_HEIGHT), egui::Sense::hover());
                for slot in 0..count {
                    let (native, code) = visible[index + slot];
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(row.left() + slot as f32 * (cell_width + ROW_GAP), row.top()),
                        egui::vec2(cell_width, ROW_HEIGHT),
                    );
                    let selected = vm
                        .translation_working_languages
                        .iter()
                        .any(|value| value == native);
                    if language_row(ui, rect, native, code, selected) {
                        actions.push(FrontendAction::TranslationToggleLanguage(
                            native.to_string(),
                        ));
                    }
                }
                index += count;
                if index < visible.len() {
                    ui.add_space(ROW_GAP);
                }
            }
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(tr_l10n(lang, "translation.language_support_hint"))
                .size(11.0)
                .color(theme::INK_4),
        );
    })
}

/// One selectable language row: name (bold) over its code, checkbox on the right.
fn language_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    native: &str,
    code: &str,
    selected: bool,
) -> bool {
    let response = ui.interact(
        rect,
        ui.id().with(("openless-language-row", native)),
        egui::Sense::click(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    let fill = if selected || response.hovered() {
        theme::SURFACE_2
    } else {
        theme::SURFACE
    };
    painter.rect_filled(rect, egui::CornerRadius::same(10), fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(0.8, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let checkbox = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 18.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    draw_check(ui, checkbox, selected);
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.center().y - 8.0),
        egui::Align2::LEFT_CENTER,
        native,
        egui::FontId::proportional(12.5),
        theme::INK,
    );
    // Only show the code when it adds information next to the native name.
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.center().y + 9.0),
        egui::Align2::LEFT_CENTER,
        code,
        egui::FontId::proportional(10.5),
        theme::INK_4,
    );
    response.clicked()
}

fn draw_check(ui: &egui::Ui, rect: egui::Rect, checked: bool) {
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(4),
        if checked { theme::INK } else { theme::SURFACE },
    );
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0, if checked { theme::INK } else { theme::LINE }),
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
    }
}

fn target_language(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    fill_height: Option<f32>,
) {
    let lang = vm.lang;
    let target = vm.translation_target_language.clone();
    let redundant = !target.is_empty()
        && vm.translation_working_languages.len() == 1
        && vm.translation_working_languages[0] == target;
    let enabled = !target.is_empty() && !redundant;
    let disabled_label = tr_l10n(lang, "translation.target_disabled").to_string();

    card(ui, width, |ui| {
        if let Some(height) = fill_height {
            // Match the left column so the two cards end on the same line.
            ui.set_min_height((height - CARD_PADDING * 2.0).max(0.0));
        }
        // The status pill is `flex: 0 0 auto` upstream, so the text block has to
        // wrap instead of pushing the pill past the card edge on a narrow column.
        let pill_width =
            layout::text_width(ui, tr_l10n(lang, "translation.status_disabled"), 10.5) + 24.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - pill_width - 8.0).max(80.0));
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "translation.target_title"))
                        .size(13.5)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "translation.target_desc"))
                        .size(11.5)
                        .color(theme::INK_4),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let label = if enabled {
                    tr_l10n(lang, "translation.status_enabled")
                } else {
                    tr_l10n(lang, "translation.status_disabled")
                };
                egui::Frame::new()
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(0.7, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(9))
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(label).size(10.5).color(if enabled {
                            theme::BLUE
                        } else {
                            theme::INK_4
                        }));
                    });
            });
        });
        ui.add_space(10.0);

        let mut selected_target = target.clone();
        // `ComboBox::width` is the *minimum* width of the selected text, and the
        // final button is that text plus the drop-down icon and the button
        // padding. Asking for `available - 4` therefore overflowed the card by
        // that chrome, which made egui widen the whole page (and with it the two
        // columns) again on the next frame.
        let combo_width = (ui.available_width() - 28.0).clamp(60.0, 360.0);
        egui::ComboBox::from_id_salt("translation-target-language")
            .width(combo_width)
            .height(32.0)
            .truncate()
            .selected_text(if target.is_empty() {
                egui::RichText::new(&disabled_label).color(theme::INK_4)
            } else {
                egui::RichText::new(target.as_str()).color(theme::INK)
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(selected_target.is_empty(), &disabled_label)
                    .clicked()
                {
                    selected_target = String::new();
                    ui.close();
                }
                for (native, _) in SUPPORTED_LANGUAGES {
                    if ui
                        .selectable_label(selected_target == native, native)
                        .clicked()
                    {
                        selected_target = native.to_string();
                        ui.close();
                    }
                }
            });
        if selected_target != target {
            actions.push(FrontendAction::TranslationSetTarget(selected_target));
        }

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(12.0);
        // The host marks the persisted dictation pack with `is_active`. The
        // page-local `style_selected` index is only a transient style-page
        // cursor (and is not hydrated by the host), so using it here could
        // leave Translation stuck on the first pack after another pack was
        // activated in 润色模式.
        let style_name = translation_style_name(vm, lang);
        let row_width = ui.available_width();
        // CSS uses flex: 0 0 auto, max-width: 180px and a single ellipsized
        // line. An egui Frame inside right_to_left instead inherited all the
        // remaining width and wrapped the style name into a tall blue block.
        let badge_width = (layout::text_width(ui, &style_name, 11.5) + 18.0)
            .min(180.0)
            .min((row_width * 0.48).max(18.0));
        let label_width = (row_width - badge_width - ui.spacing().item_spacing.x).max(1.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(label_width);
                ui.set_max_width(label_width);
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "translation.style_title"))
                        .size(12.0)
                        .strong(),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "translation.style_desc"))
                        .size(11.5)
                        .color(theme::INK_4),
                );
            });
            let (badge, response) =
                ui.allocate_exact_size(egui::vec2(badge_width, 24.0), egui::Sense::hover());
            paint_style_badge(ui, badge, &style_name);
            response.on_hover_text(style_name);
        });

        if redundant {
            ui.add_space(10.0);
            egui::Frame::new()
                .fill(theme::WARN_SOFT)
                .stroke(egui::Stroke::new(0.5, theme::WARN))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "translation.target_same_as_working"))
                            .size(11.5)
                            .color(theme::WARN),
                    );
                });
        }
    });
}

fn translation_style_name(vm: &FrontendViewModel, lang: Lang) -> String {
    vm.style_packs
        .iter()
        .find(|pack| pack.is_active && pack.enabled)
        .or_else(|| vm.style_packs.iter().find(|pack| pack.is_active))
        .map(|pack| pack.name.clone())
        .unwrap_or_else(|| {
            if vm.style_selected == usize::MAX {
                tr_l10n(lang, "overview.mode_raw").to_string()
            } else {
                tr_l10n(lang, "overview.mode_light").to_string()
            }
        })
}

fn paint_style_badge(ui: &egui::Ui, rect: egui::Rect, name: &str) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(12), theme::BLUE_SOFT);
    let galley = layout::text_galley(ui, name, theme::BLUE, 11.5, rect.width() - 18.0, 1);
    painter.galley(
        egui::pos2(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::BLUE,
    );
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("translation-style-badge"), rect));
}

fn usage_columns(width: f32) -> usize {
    ((width + 14.0) / (150.0 + 14.0)).floor().clamp(1.0, 5.0) as usize
}

fn usage(ui: &mut egui::Ui, width: f32, vm: &FrontendViewModel) {
    let lang = vm.lang;
    card(ui, width, |ui| {
        ui.label(
            egui::RichText::new(tr_l10n(lang, "translation.howto_title"))
                .size(13.0)
                .strong(),
        );
        ui.add_space(10.0);

        let steps = [
            tr_l10n(lang, "translation.howto_step1").to_string(),
            fmt_l10n(lang, "translation.howto_step2", &[&vm.dictation_hotkey]),
            fmt_l10n(lang, "translation.howto_step3", &[&vm.translation_hotkey]),
            fmt_l10n(lang, "translation.howto_step4", &[&vm.dictation_hotkey]),
            tr_l10n(lang, "translation.howto_step5").to_string(),
        ];
        // Match CSS grid repeat(auto-fit, minmax(150px, 1fr)) with a 14px
        // column gap: five steps fit in one row only when the viewport allows.
        let columns = usage_columns(ui.available_width());
        let gap = 14.0;
        let cell_width = (ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32;
        let mut index = 0;
        while index < steps.len() {
            let count = columns.min(steps.len() - index);
            let mut row_height: f32 = 20.0;
            let mut galleys = Vec::new();
            for slot in 0..count {
                let galley = layout::text_galley(
                    ui,
                    &steps[index + slot],
                    theme::INK_2,
                    12.0,
                    (cell_width - 26.0).max(1.0),
                    usize::MAX,
                );
                row_height = row_height.max(galley.size().y + 2.0);
                galleys.push(galley);
            }
            let (row, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), row_height),
                egui::Sense::hover(),
            );
            for (slot, galley) in galleys.into_iter().enumerate() {
                let left = row.left() + slot as f32 * (cell_width + gap);
                let painter = ui.painter().with_clip_rect(egui::Rect::from_min_size(
                    egui::pos2(left, row.top()),
                    egui::vec2(cell_width, row_height),
                ));
                let number = egui::pos2(left + 9.0, row.top() + 10.0);
                painter.circle_filled(number, 9.0, theme::BLUE_SOFT);
                painter.text(
                    number,
                    egui::Align2::CENTER_CENTER,
                    (index + slot + 1).to_string(),
                    egui::FontId::proportional(11.0),
                    theme::BLUE,
                );
                painter.galley(egui::pos2(left + 26.0, row.top()), galley, theme::INK_2);
            }
            index += count;
            if index < steps.len() {
                ui.add_space(10.0);
            }
        }
    });
}

/// Full-width card that sizes itself to its contents. Returns its height.
fn card(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui)) -> f32 {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(CARD_PADDING as i8))
        .show(ui, |ui| {
            ui.set_width((width - CARD_PADDING * 2.0).max(1.0));
            contents(ui);
        })
        .response
        .rect
        .height()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::frontend::view_model::StylePack;

    #[test]
    fn translation_style_name_follows_the_host_active_pack() {
        let pack = |id: &str, name: &str, is_active: bool| StylePack {
            id: id.to_string(),
            icon_path: None,
            icon_data_url: None,
            base_mode: "light".to_string(),
            name: name.to_string(),
            description: String::new(),
            tags: Vec::new(),
            is_builtin: true,
            enabled: true,
            is_active,
            selection_active: false,
        };
        let vm = FrontendViewModel {
            style_selected: 0,
            style_packs: vec![
                pack("builtin.light", "轻度润色", false),
                pack("custom", "My translation style", true),
            ],
            ..Default::default()
        };

        assert_eq!(
            translation_style_name(&vm, Lang::ZhCn),
            "My translation style",
            "translation must follow Core's active pack, not the stale page index"
        );
    }

    #[test]
    fn usage_steps_auto_fit_into_one_through_five_columns() {
        for (width, expected) in [
            (140.0, 1),
            (314.0, 2),
            (478.0, 3),
            (642.0, 4),
            (806.0, 5),
            (1600.0, 5),
        ] {
            assert_eq!(usage_columns(width), expected, "width={width}");
        }
    }

    #[test]
    fn long_style_name_is_ellipsized_inside_a_one_line_badge() {
        let ctx = egui::Context::default();
        let _ = super::super::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 300.0),
                )),
                ..Default::default()
            },
            |ui| {
                let name = "An exceptionally long translation style name that must not wrap";
                let badge =
                    egui::Rect::from_min_size(egui::pos2(50.0, 50.0), egui::vec2(180.0, 24.0));
                paint_style_badge(ui, badge, name);
                let galley =
                    layout::text_galley(ui, name, theme::BLUE, 11.5, badge.width() - 18.0, 1);
                assert_eq!(galley.rows.len(), 1);
                assert!(galley.elided);
            },
        );
    }

    #[test]
    fn narrow_step_text_is_wrapped_without_three_line_truncation() {
        let ctx = egui::Context::default();
        let _ = super::super::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(480.0, 300.0),
                )),
                ..Default::default()
            },
            |ui| {
                let text = "A long instruction must be fully readable even when the window is narrowed and the grid reflows";
                let galley = layout::text_galley(ui, text, theme::INK_2, 12.0, 90.0, usize::MAX);
                assert!(galley.rows.len() > 3);
                assert!(!galley.elided);
            },
        );
    }
}
