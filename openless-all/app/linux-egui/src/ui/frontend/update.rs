//! The self-update dialog.
//!
//! Tauri shows this as a `Modal` (`src/components/AutoUpdate.tsx` + the status
//! machine behind it: available → downloading → installing → downloaded /
//! installError). The Linux host owns that machine and mirrors one stage into
//! the view model; this module only paints it. The AppImage is the only build
//! that can update itself, so the host hides the whole flow (`update_stage`
//! stays `None`) for deb/rpm installs.

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::layout::{self, ButtonKind};
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, UpdateStage};

/// Tauri `Modal`: a centred card, at most 560px wide.
const CARD_MAX_WIDTH: f32 = 560.0;
const CARD_PADDING: f32 = 22.0;
const FIELD_GAP: f32 = 14.0;

/// Paint the modal for `vm.update_stage`, if there is one. Drawn above the
/// settings overlay because the button that opens it lives in the About card.
pub fn update_overlay(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    body: egui::Rect,
) {
    let Some(stage) = vm.update_stage else {
        return;
    };
    let lang = vm.lang;

    // Height follows the body: the progress bar and the error text change it,
    // and a fixed box would clip the longer translations.
    let width = (body.width() - 40.0).min(CARD_MAX_WIDTH);
    let height = (body.height() - 40.0).min(card_height(vm));
    let card_rect = egui::Rect::from_center_size(body.center(), egui::vec2(width, height));
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("openless-update-card-rect"), card_rect);
    });

    // Mask + click catcher + card stay in ONE `Area` (same `LayerId`). In
    // separate areas egui raises whichever was pressed (`move_to_top`), so a
    // click on the backdrop used to lift the mask over the card.
    egui::Area::new(egui::Id::new("openless-update-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.painter().rect_filled(
                body,
                egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 14,
                    se: 14,
                },
                theme::OVERLAY,
            );
            let _ = ui.allocate_rect(body, egui::Sense::click());
            ui.scope_builder(egui::UiBuilder::new().max_rect(card_rect), |ui| {
                layout::paint_card(ui.painter(), card_rect);
                let inner = card_rect.shrink(CARD_PADDING);
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                let ui = &mut child;
                ui.set_width(inner.width());

                let busy = matches!(stage, UpdateStage::Downloading | UpdateStage::Installing);
                header(ui, lang, vm, stage, busy, actions);
                ui.add_space(FIELD_GAP);
                body_text(ui, lang, vm, stage);
                if stage == UpdateStage::Downloading {
                    ui.add_space(FIELD_GAP);
                    progress_bar(ui, lang, vm);
                }
                ui.add_space(FIELD_GAP + 4.0);
                footer(ui, lang, stage, busy, actions);
            });
        });
}

fn card_height(vm: &FrontendViewModel) -> f32 {
    let mut height = 196.0;
    if vm.update_stage == Some(UpdateStage::Downloading) {
        height += 34.0;
    }
    if vm.update_error.is_some() {
        height += 18.0;
    }
    height
}

fn header(
    ui: &mut egui::Ui,
    lang: Lang,
    vm: &FrontendViewModel,
    stage: UpdateStage,
    busy: bool,
    actions: &mut Vec<FrontendAction>,
) {
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(stage_title(lang, stage))
                    .size(16.0)
                    .strong(),
            )
            .truncate(),
        );
        if !busy {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("×").size(15.0).color(theme::INK_3))
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .min_size(egui::vec2(22.0, 22.0)),
                    )
                    .clicked()
                {
                    actions.push(FrontendAction::UpdateDismiss);
                }
            });
        }
        let _ = vm;
    });
}

fn body_text(ui: &mut egui::Ui, lang: Lang, vm: &FrontendViewModel, stage: UpdateStage) {
    let version = vm.update_version.as_str();
    let text = match stage {
        UpdateStage::Available => fmt_l10n(
            lang,
            "settings.about.update_dialog_available_desc",
            &[&version],
        ),
        UpdateStage::Downloading => fmt_l10n(
            lang,
            "settings.about.update_dialog_downloading_desc",
            &[&version],
        ),
        UpdateStage::Installing => fmt_l10n(
            lang,
            "settings.about.update_dialog_installing_desc",
            &[&version],
        ),
        UpdateStage::Installed => fmt_l10n(
            lang,
            "settings.about.update_dialog_downloaded_desc",
            &[&version],
        ),
        UpdateStage::Failed => {
            let error = vm.update_error.as_deref().unwrap_or_default();
            fmt_l10n(
                lang,
                "settings.about.update_dialog_install_error_desc",
                &[&error],
            )
        }
    };
    ui.add(egui::Label::new(egui::RichText::new(text).size(12.5).color(theme::INK_2)).wrap());
}

/// `{{progress}}% · {{downloaded}} / {{total}}`, or just the downloaded size
/// while the server sends no `Content-Length` (Tauri `progressUnknown`).
fn progress_bar(ui: &mut egui::Ui, lang: Lang, vm: &FrontendViewModel) {
    let ratio = match vm.update_total {
        Some(total) if total > 0 => (vm.update_downloaded as f32 / total as f32).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(3), theme::SURFACE_2);
    if ratio > 0.0 {
        let filled = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.left() + rect.width() * ratio, rect.bottom()),
        );
        painter.rect_filled(filled, egui::CornerRadius::same(3), theme::BLUE);
    }
    ui.add_space(6.0);
    let downloaded = human_bytes(vm.update_downloaded);
    let label = match vm.update_total {
        Some(total) if total > 0 => {
            let percent = (ratio * 100.0).round() as u32;
            fmt_l10n(
                lang,
                "settings.about.update_dialog_progress",
                &[&percent, &downloaded, &human_bytes(total)],
            )
        }
        _ => fmt_l10n(
            lang,
            "settings.about.update_dialog_progress_unknown",
            &[&downloaded],
        ),
    };
    ui.label(
        egui::RichText::new(label)
            .size(11.5)
            .monospace()
            .color(theme::INK_3),
    );
}

fn footer(
    ui: &mut egui::Ui,
    lang: Lang,
    stage: UpdateStage,
    busy: bool,
    actions: &mut Vec<FrontendAction>,
) {
    if busy {
        // Tauri shows a bare label while it works; nothing to click.
        // Two literal call sites: the i18n sync only collects keys written
        // next to `tr_l10n(…,`.
        let label = if stage == UpdateStage::Installing {
            tr_l10n(lang, "settings.about.update_dialog_installing_label")
        } else {
            tr_l10n(lang, "settings.about.update_dialog_downloading_label")
        };
        ui.label(egui::RichText::new(label).size(12.0).color(theme::INK_3));
        return;
    }
    let label = match stage {
        UpdateStage::Available => tr_l10n(lang, "settings.about.update_dialog_install"),
        // "Restart manually later" — the Linux host never relaunches itself
        // (a second instance would just hand its intent back to this one), so
        // the only honest action here is to close and let the user relaunch.
        UpdateStage::Installed | UpdateStage::Failed => {
            tr_l10n(lang, "settings.about.update_dialog_later")
        }
        UpdateStage::Downloading | UpdateStage::Installing => {
            unreachable!("busy stages return early")
        }
    };
    let width = layout::text_width(ui, label, 12.5) + 40.0;
    let rect = egui::Rect::from_min_size(
        egui::pos2(ui.max_rect().right() - width, ui.max_rect().bottom() - 30.0),
        egui::vec2(width, 30.0),
    );
    let kind = if stage == UpdateStage::Available {
        ButtonKind::Blue
    } else {
        ButtonKind::Ghost
    };
    if layout::action_button(ui, rect, label, None, kind).clicked() {
        actions.push(if stage == UpdateStage::Available {
            FrontendAction::UpdateInstall
        } else {
            FrontendAction::UpdateDismiss
        });
    }
}

fn stage_title(lang: Lang, stage: UpdateStage) -> &'static str {
    match stage {
        UpdateStage::Available => tr_l10n(lang, "settings.about.update_dialog_available_title"),
        UpdateStage::Downloading => tr_l10n(lang, "settings.about.update_dialog_downloading_title"),
        UpdateStage::Installing => tr_l10n(lang, "settings.about.update_dialog_installing_title"),
        UpdateStage::Installed => tr_l10n(lang, "settings.about.update_dialog_downloaded_title"),
        UpdateStage::Failed => tr_l10n(lang, "settings.about.update_dialog_install_error_title"),
    }
}

/// Bytes → a short human string. The sizes here are AppImages (tens of MB), so
/// one decimal of MB is enough and keeps the progress line from jittering.
fn human_bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= MB {
        format!("{:.1} MB", value / MB)
    } else {
        format!("{:.0} KB", (value / 1024.0).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::frontend::run_pass;

    fn painted_text(output: &egui::FullOutput) -> String {
        fn collect(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push_str(text.galley.text());
                    out.push('\n');
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = String::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut out);
        }
        out
    }

    /// Renders the dialog twice (egui needs one pass to settle sizes) and
    /// returns everything it painted plus the card the host can query.
    fn painted_lines(vm: &mut FrontendViewModel) -> (String, egui::Rect) {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let mut painted = String::new();
        for _ in 0..2 {
            let mut output = run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let body = ui.max_rect();
                    update_overlay(ui.ctx(), vm, &mut actions, body);
                },
            );
            painted = painted_text(&output);
            output.textures_delta.clear();
        }
        let card = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("openless-update-card-rect")))
            .unwrap_or(egui::Rect::NOTHING);
        (painted, card)
    }

    fn vm_with(stage: UpdateStage) -> FrontendViewModel {
        let mut vm = FrontendViewModel::default();
        vm.update_stage = Some(stage);
        vm.update_version = "2.0.0-Beta.2".into();
        vm
    }

    #[test]
    fn every_stage_paints_its_own_wording() {
        let zh = Lang::ZhCn;
        let mut vm = vm_with(UpdateStage::Available);
        let (painted, _) = painted_lines(&mut vm);
        assert!(painted.contains(tr_l10n(zh, "settings.about.update_dialog_available_title")));
        assert!(painted.contains("2.0.0-Beta.2"), "{painted}");
        assert!(painted.contains(tr_l10n(zh, "settings.about.update_dialog_install")));

        let mut vm = vm_with(UpdateStage::Failed);
        vm.update_error = Some("signature rejected".into());
        let (painted, _) = painted_lines(&mut vm);
        assert!(painted.contains(tr_l10n(
            zh,
            "settings.about.update_dialog_install_error_title"
        )));
        assert!(painted.contains("signature rejected"), "{painted}");
    }

    /// The dialog must show real byte progress, not a stuck empty bar.
    #[test]
    fn downloading_shows_the_byte_progress() {
        let mut vm = vm_with(UpdateStage::Downloading);
        vm.update_downloaded = 1024 * 1024;
        vm.update_total = Some(4 * 1024 * 1024);
        let (painted, _) = painted_lines(&mut vm);
        assert!(painted.contains("25%"), "{painted}");
        assert!(painted.contains("1.0 MB"), "{painted}");
        assert!(painted.contains("4.0 MB"), "{painted}");

        // No Content-Length: the line falls back to the downloaded size only.
        let mut vm = vm_with(UpdateStage::Downloading);
        vm.update_downloaded = 512 * 1024;
        vm.update_total = None;
        let (painted, _) = painted_lines(&mut vm);
        assert!(painted.contains("512 KB"), "{painted}");
        assert!(!painted.contains('%'), "{painted}");
    }

    /// No stage → nothing painted (deb/rpm installs never see the dialog).
    #[test]
    fn without_a_stage_the_dialog_stays_away() {
        let mut vm = FrontendViewModel::default();
        let (painted, _) = painted_lines(&mut vm);
        assert!(!painted.contains(tr_l10n(
            Lang::ZhCn,
            "settings.about.update_dialog_available_title"
        )));
    }

    #[test]
    fn human_bytes_switches_at_a_megabyte() {
        assert_eq!(human_bytes(0), "0 KB");
        assert_eq!(human_bytes(1536), "2 KB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(human_bytes(1024 * 1024 * 3 / 2), "1.5 MB");
    }
}
