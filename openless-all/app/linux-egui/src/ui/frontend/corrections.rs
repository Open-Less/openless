//! Correction rules (纠错规则) page — port of the Tauri `pages/Corrections.tsx`.
//!
//! Fixes common ASR misrecognitions: `pattern → replacement`, with one `{num}`
//! digit wildcard. Rules collected automatically from user edits are badged and
//! can be reviewed or removed like any other rule.

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::layout;
use super::pages::correction_chip;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel};

const GAP: f32 = 14.0;
const CARD_PADDING: f32 = 20.0;

pub fn page(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    if vm.vocab_unsupported {
        layout::unsupported_page(ui, lang, tr_l10n(lang, "nav.corrections"));
        return;
    }

    layout::page_header(
        ui,
        width,
        tr_l10n(lang, "nav.corrections"),
        tr_l10n(lang, "vocab.corrections_title"),
        Some(tr_l10n(lang, "vocab.corrections_tip")),
    );
    ui.add_space(GAP);

    // ── Add a rule ──────────────────────────────────────────────────────────
    card(ui, width, |ui| {
        ui.horizontal(|ui| {
            let add_width = 72.0;
            let arrow_width = 24.0;
            let spacing = ui.spacing().item_spacing.x;
            let input_width =
                ((ui.available_width() - add_width - arrow_width - spacing * 3.0) / 2.0).max(60.0);
            input(
                ui,
                input_width,
                &mut vm.vocab_pattern,
                tr_l10n(lang, "vocab.corrections_pattern_placeholder"),
            );
            ui.add_sized(
                [arrow_width, 32.0],
                egui::Label::new(egui::RichText::new("→").color(theme::INK_4))
                    .wrap_mode(egui::TextWrapMode::Extend),
            );
            input(
                ui,
                input_width,
                &mut vm.vocab_replacement,
                tr_l10n(lang, "vocab.corrections_replacement_placeholder"),
            );
            let pattern = vm.vocab_pattern.trim().to_string();
            let replacement = vm.vocab_replacement.trim().to_string();
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
                && !pattern.is_empty()
            {
                actions.push(FrontendAction::VocabAddRule {
                    pattern,
                    replacement,
                });
                vm.vocab_pattern.clear();
                vm.vocab_replacement.clear();
            }
        });
        if let Some(error) = vm.vocab_error.clone() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(error)
                    .size(11.5)
                    .color(egui::Color32::from_rgb(185, 28, 28)),
            );
        }
    });

    ui.add_space(GAP);

    // ── Rules ───────────────────────────────────────────────────────────────
    let learned = vm.vocab_rules.iter().filter(|rule| rule.learned).count();
    card(ui, width, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "vocab.corrections_title"))
                    .size(13.0)
                    .strong(),
            );
            if learned > 0 {
                ui.add_space(8.0);
                egui::Frame::new()
                    .fill(theme::SURFACE_2)
                    .corner_radius(egui::CornerRadius::same(9))
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(fmt_l10n(
                                lang,
                                "vocab.corrections_only_learned",
                                &[&learned],
                            ))
                            .size(10.5)
                            .color(theme::INK_3),
                        );
                    });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if learned > 0
                    && ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(tr_l10n(
                                    lang,
                                    "vocab.corrections_remove_all_learned",
                                ))
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
                        .vocab_rules
                        .iter()
                        .enumerate()
                        .filter(|(_, rule)| rule.learned)
                        .map(|(index, _)| index)
                        .rev()
                        .collect();
                    for index in indices {
                        actions.push(FrontendAction::VocabRemoveRule(index));
                    }
                }
            });
        });
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            let mut remove_index = None;
            for (index, rule) in vm.vocab_rules.iter().enumerate() {
                let label = if rule.learned {
                    format!(
                        "{} → {}   {}",
                        rule.pattern,
                        rule.replacement,
                        tr_l10n(lang, "vocab.corrections_learned_badge")
                    )
                } else {
                    format!("{} → {}", rule.pattern, rule.replacement)
                };
                let (toggle, remove) = correction_chip(ui, &label, rule.enabled);
                if remove {
                    remove_index = Some(index);
                    break;
                }
                if toggle {
                    actions.push(FrontendAction::VocabToggleRule(index));
                }
            }
            if let Some(index) = remove_index {
                actions.push(FrontendAction::VocabRemoveRule(index));
            }
            if vm.vocab_rules.is_empty() {
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "vocab.corrections_empty"))
                        .size(12.0)
                        .color(theme::INK_4),
                );
            }
        });
    });
}

/// A single-line input with the shared rounded surface style.
fn input(ui: &mut egui::Ui, width: f32, value: &mut String, hint: &str) {
    let content_width = (width - 20.0).max(1.0);
    egui::Frame::new()
        .fill(theme::SURFACE_2)
        .stroke(egui::Stroke::new(0.8, theme::LINE))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(content_width);
            ui.add_sized(
                [content_width, 20.0],
                egui::TextEdit::singleline(value)
                    .desired_width(content_width)
                    .hint_text(hint)
                    .frame(false),
            );
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
