//! Dictionary (词典) page — port of the Tauri `pages/Vocab.tsx`.
//!
//! Layout: page header, filter tabs (all / auto / manual) with a search box,
//! then the word list, the auto-collected group and the scenario presets.
//! Correction rules live on their own page now (`corrections.rs`).

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, VocabEntry};

const GAP: f32 = 14.0;
const CARD_PADDING: f32 = 20.0;

pub fn page(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    if vm.vocab_unsupported {
        layout::unsupported_page(ui, lang, tr_l10n(lang, "nav.vocab"));
        return;
    }

    let header = layout::page_header(
        ui,
        width,
        tr_l10n(lang, "vocab.kicker"),
        tr_l10n(lang, "vocab.title"),
        Some(tr_l10n(lang, "vocab.desc")),
    );
    // "New word" jumps to the input at the top of the list card.
    let new_word = tr_l10n(lang, "vocab.new_word");
    let new_word_width = layout::text_width(ui, new_word, 12.5) + 40.0;
    let new_word_rect = egui::Rect::from_min_size(
        egui::pos2(header.right() - new_word_width, header.top() + 22.0),
        egui::vec2(new_word_width, 30.0),
    );
    if layout::action_button(ui, new_word_rect, new_word, None, layout::ButtonKind::Ghost).clicked()
    {
        ui.memory_mut(|memory| memory.request_focus(egui::Id::new("openless-vocab-input")));
    }
    ui.add_space(GAP);

    // ── Word list ───────────────────────────────────────────────────────────
    card(ui, width, |ui| {
        ui.horizontal(|ui| {
            let filters = [
                tr_l10n(lang, "vocab.filter_all"),
                tr_l10n(lang, "vocab.filter_auto"),
                tr_l10n(lang, "vocab.filter_manual"),
            ];
            let filter_width = layout::segmented_width(ui, &filters);
            let (filter_rect, _) =
                ui.allocate_exact_size(egui::vec2(filter_width, 26.0), egui::Sense::hover());
            if let Some(index) =
                layout::segmented(ui, filter_rect, &filters, vm.vocab_filter.min(2))
            {
                actions.push(FrontendAction::VocabFilter(index));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(1.0);
                let search_width = (ui.available_width() * 0.5).clamp(140.0, 260.0);
                egui::Frame::new()
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.8, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.set_width((search_width - 20.0).max(1.0));
                        ui.add_sized(
                            [(search_width - 20.0).max(1.0), 20.0],
                            egui::TextEdit::singleline(&mut vm.vocab_query)
                                .hint_text(tr_l10n(lang, "vocab.search_placeholder"))
                                .frame(false),
                        );
                    });
            });
        });
        ui.add_space(12.0);

        // New word input.
        ui.horizontal(|ui| {
            let add_width = 78.0;
            let input_width = (ui.available_width() - add_width - 8.0).max(80.0);
            let content_width = (input_width - 20.0).max(1.0);
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(0.8, theme::LINE))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(content_width);
                    let response = ui.add_sized(
                        [content_width, 20.0],
                        egui::TextEdit::singleline(&mut vm.vocab_input)
                            .id(egui::Id::new("openless-vocab-input"))
                            .desired_width(content_width)
                            .hint_text(tr_l10n(lang, "vocab.placeholder"))
                            .frame(false),
                    );
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        && !vm.vocab_input.trim().is_empty()
                    {
                        let phrase = vm.vocab_input.trim().to_string();
                        actions.push(FrontendAction::VocabAddPhrase(phrase));
                        vm.vocab_input.clear();
                    }
                });
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(tr_l10n(lang, "btn.add"))
                            .color(theme::SURFACE)
                            .size(12.0),
                    )
                    .fill(theme::INK)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(add_width, 32.0)),
                )
                .clicked()
                && !vm.vocab_input.trim().is_empty()
            {
                let phrase = vm.vocab_input.trim().to_string();
                actions.push(FrontendAction::VocabAddPhrase(phrase));
                vm.vocab_input.clear();
            }
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(tr_l10n(lang, "vocab.tip"))
                .size(11.0)
                .color(theme::INK_4),
        );

        ui.add_space(12.0);
        if let Some(error) = vm.vocab_error.clone() {
            ui.label(
                egui::RichText::new(error)
                    .size(11.5)
                    .color(egui::Color32::from_rgb(185, 28, 28)),
            );
            ui.add_space(8.0);
        }

        // Filtered entries.
        let query = vm.vocab_query.trim().to_lowercase();
        let visible: Vec<usize> = vm
            .vocab_entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| match vm.vocab_filter {
                1 => entry.learned,
                2 => !entry.learned,
                _ => true,
            })
            .filter(|(_, entry)| query.is_empty() || entry.phrase.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect();

        if visible.is_empty() {
            ui.add_space(6.0);
            let message = if query.is_empty() {
                tr_l10n(lang, "vocab.empty").to_string()
            } else {
                tr_l10n(lang, "vocab.search_empty").to_string()
            };
            ui.label(egui::RichText::new(message).size(12.0).color(theme::INK_4));
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                let mut remove_index = None;
                for index in visible {
                    let entry = &vm.vocab_entries[index];
                    let (toggle, remove) = word_chip(ui, entry);
                    if remove {
                        remove_index = Some(index);
                        break;
                    }
                    if toggle {
                        actions.push(FrontendAction::VocabTogglePhrase(index));
                    }
                }
                if let Some(index) = remove_index {
                    actions.push(FrontendAction::VocabRemovePhrase(index));
                }
            });
        }

        // Auto-collected group.
        let learned_total = vm
            .vocab_entries
            .iter()
            .filter(|entry| entry.learned)
            .count();
        if learned_total > 0 && vm.vocab_filter == 0 {
            ui.add_space(14.0);
            layout::soft_separator(ui);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(fmt_l10n(lang, "vocab.learned_section", &[&learned_total]))
                        .size(12.0)
                        .color(theme::INK_3),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                        let indices: Vec<usize> = vm
                            .vocab_entries
                            .iter()
                            .enumerate()
                            .filter(|(_, entry)| entry.learned)
                            .map(|(index, _)| index)
                            .rev()
                            .collect();
                        for index in indices {
                            actions.push(FrontendAction::VocabRemovePhrase(index));
                        }
                    }
                });
            });
        }
    });

    ui.add_space(GAP);

    // ── Scenario presets ────────────────────────────────────────────────────
    card(ui, width, |ui| {
        layout::section_title(
            ui,
            ui.available_width(),
            tr_l10n(lang, "vocab.presets_title"),
            Some(tr_l10n(lang, "vocab.presets_tip")),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            let preset_names = [
                tr_l10n(lang, "vocab.presets_dev_tools"),
                tr_l10n(lang, "vocab.presets_products"),
                tr_l10n(lang, "vocab.presets_terms"),
                tr_l10n(lang, "vocab.presets_english"),
            ];
            for (index, name) in preset_names.iter().enumerate() {
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
                    .corner_radius(egui::CornerRadius::same(12))
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
            if !vm.vocab_selected_presets.is_empty()
                && ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(tr_l10n(lang, "vocab.presets_apply"))
                                .color(theme::SURFACE)
                                .size(12.0),
                        )
                        .fill(theme::INK)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(72.0, 30.0)),
                    )
                    .clicked()
            {
                actions.push(FrontendAction::VocabApplyPreset(usize::MAX));
            }
        });

        // Editor for a new / existing preset.
        if vm.vocab_editing_preset.is_some() {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [200.0, 28.0],
                    egui::TextEdit::singleline(&mut vm.vocab_preset_name)
                        .hint_text(tr_l10n(lang, "vocab.presets_name_placeholder")),
                );
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(tr_l10n(lang, "vocab.presets_save"))
                                .color(theme::SURFACE)
                                .size(12.0),
                        )
                        .fill(theme::INK)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(72.0, 28.0)),
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
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(tr_l10n(lang, "common.cancel")).size(12.0),
                        )
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(72.0, 28.0)),
                    )
                    .clicked()
                {
                    vm.vocab_editing_preset = None;
                }
            });
            ui.add_space(6.0);
            ui.add_sized(
                [ui.available_width(), 64.0],
                egui::TextEdit::multiline(&mut vm.vocab_preset_phrases)
                    .desired_rows(3)
                    .hint_text(tr_l10n(lang, "vocab.presets_words_placeholder")),
            );
        }

        // Saved presets.
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

/// A word pill: phrase, hit counter, enable toggle and delete.
fn word_chip(ui: &mut egui::Ui, entry: &VocabEntry) -> (bool, bool) {
    let fill = if entry.enabled && entry.hits > 0 {
        theme::BLUE_SOFT
    } else if entry.enabled {
        theme::SURFACE
    } else {
        theme::SURFACE_2
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
    let close_size = 22.0;
    let width = 12.0 + phrase.size().x + 8.0 + hits_size.x + 6.0 + close_size + 10.0;
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
        egui::pos2(rect.left() + 12.0, rect.center().y - phrase.size().y / 2.0),
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
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 15)
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
