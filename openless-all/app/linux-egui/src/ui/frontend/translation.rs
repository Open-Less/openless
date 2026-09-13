//! Translation page — port of the Tauri `pages/Translation.tsx`.
//!
//! Language search + working-language grid on the left, target language and the
//! inherited style on the right, then the usage guide.

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel};

const GAP: f32 = 12.0;
const CARD_PADDING: f32 = 18.0;
const TWO_COLUMN_MIN_WIDTH: f32 = 860.0;

/// Language names are endonyms: they read the same in every UI locale.
const SUPPORTED_LANGUAGES: [&str; 15] = [
    "简体中文",
    "繁体中文",
    "English",
    "日本語",
    "한국어",
    "Français",
    "Deutsch",
    "Español",
    "Italiano",
    "Português",
    "Русский",
    "العربية",
    "Tiếng Việt",
    "ไทย",
    "हिन्दी",
];

pub fn page(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
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

    if width >= TWO_COLUMN_MIN_WIDTH {
        let column_width = (width - GAP) / 2.0;
        ui.columns(2, |columns| {
            working_languages(&mut columns[0], column_width, vm, actions);
            target_language(&mut columns[1], column_width, vm, actions);
        });
    } else {
        working_languages(ui, width, vm, actions);
        ui.add_space(GAP);
        target_language(ui, width, vm, actions);
    }
    ui.add_space(GAP);
    usage(ui, width, vm);
}

fn working_languages(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
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

        // Search + selected count.
        ui.horizontal(|ui| {
            let count_text = fmt_l10n(
                lang,
                "translation.selected_languages",
                &[&vm.translation_working_languages.len()],
            );
            let count_width = layout::text_width(ui, &count_text, 11.0) + 4.0;
            let search_width = (ui.available_width() - count_width - 8.0).max(120.0);
            egui::Frame::new()
                .fill(theme::SURFACE_2)
                .stroke(egui::Stroke::new(0.8, theme::LINE))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.set_width((search_width - 20.0).max(1.0));
                    ui.add_sized(
                        [(search_width - 20.0).max(1.0), 20.0],
                        egui::TextEdit::singleline(&mut vm.translation_query)
                            .hint_text(tr_l10n(lang, "translation.search_languages"))
                            .frame(false),
                    );
                });
            ui.label(
                egui::RichText::new(count_text)
                    .size(11.0)
                    .color(theme::INK_4),
            );
        });
        ui.add_space(10.0);

        let query = vm.translation_query.trim().to_lowercase();
        let visible: Vec<&str> = SUPPORTED_LANGUAGES
            .iter()
            .copied()
            .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
            .collect();
        if visible.is_empty() {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "translation.no_matching_languages"))
                    .size(11.5)
                    .color(theme::INK_4),
            );
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for name in visible {
                    let selected = vm
                        .translation_working_languages
                        .iter()
                        .any(|value| value == name);
                    let response = ui.add(
                        egui::Button::new(egui::RichText::new(name).size(12.5).color(
                            if selected {
                                egui::Color32::WHITE
                            } else {
                                theme::INK_2
                            },
                        ))
                        .fill(if selected {
                            theme::BLUE
                        } else {
                            theme::SURFACE_2
                        })
                        .stroke(egui::Stroke::new(0.5, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(255))
                        .min_size(egui::vec2(0.0, 28.0)),
                    );
                    if response.clicked() {
                        actions.push(FrontendAction::TranslationToggleLanguage(name.to_string()));
                    }
                }
            });
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(tr_l10n(lang, "translation.language_support_hint"))
                .size(11.0)
                .color(theme::INK_4),
        );
    });
}

fn target_language(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let target = vm.translation_target_language.clone();
    let redundant = !target.is_empty()
        && vm.translation_working_languages.len() == 1
        && vm.translation_working_languages[0] == target;
    let enabled = !target.is_empty() && !redundant;
    let disabled_label = tr_l10n(lang, "translation.target_disabled").to_string();

    card(ui, width, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
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
                ui.label(
                    egui::RichText::new(if enabled {
                        tr_l10n(lang, "translation.status_enabled")
                    } else {
                        tr_l10n(lang, "translation.status_disabled")
                    })
                    .size(10.5)
                    .strong()
                    .color(if enabled { theme::BLUE } else { theme::INK_4 }),
                );
            });
        });
        ui.add_space(10.0);

        let mut selected_target = target.clone();
        egui::ComboBox::from_id_salt("translation-target-language")
            .width((width - CARD_PADDING * 2.0).min(360.0))
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
                for language in SUPPORTED_LANGUAGES {
                    if ui
                        .selectable_label(selected_target == language, language)
                        .clicked()
                    {
                        selected_target = language.to_string();
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
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let style_name = if let Some(pack) = vm.style_packs.get(vm.style_selected) {
                    pack.name.clone()
                } else if vm.style_selected == usize::MAX {
                    tr_l10n(lang, "overview.mode_raw").to_string()
                } else {
                    tr_l10n(lang, "overview.mode_light").to_string()
                };
                egui::Frame::new()
                    .fill(theme::BLUE_SOFT)
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(9, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(style_name)
                                .size(11.0)
                                .strong()
                                .color(theme::BLUE),
                        );
                    });
            });
        });

        if redundant {
            ui.add_space(10.0);
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(217, 119, 6, 20))
                .stroke(egui::Stroke::new(
                    0.5,
                    egui::Color32::from_rgba_unmultiplied(217, 119, 6, 62),
                ))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "translation.target_same_as_working"))
                            .size(11.5)
                            .color(egui::Color32::from_rgb(180, 103, 10)),
                    );
                });
        }
    });
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
        for (index, text) in steps.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{}.", index + 1))
                        .size(12.5)
                        .color(theme::INK_3),
                );
                ui.label(egui::RichText::new(text).size(12.5).color(theme::INK_2));
            });
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
        layout::soft_separator(ui);
        ui.add_space(10.0);
        note(
            ui,
            tr_l10n(lang, "translation.howto_indicator_title"),
            tr_l10n(lang, "translation.howto_indicator_desc"),
            theme::BLUE,
        );
        ui.add_space(8.0);
        note(
            ui,
            tr_l10n(lang, "translation.howto_fallback_title"),
            tr_l10n(lang, "translation.howto_fallback_desc"),
            theme::OK,
        );
    });
}

fn note(ui: &mut egui::Ui, title: &str, desc: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 18.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.0, color);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(12.0)
                    .strong()
                    .color(theme::INK_2),
            );
            ui.label(egui::RichText::new(desc).size(11.5).color(theme::INK_4));
        });
    });
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
