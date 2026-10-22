//! Disposable desktop capsule renderer for the optional spectral style. The
//! OpenLess default capsule lives in [`crate::pill`].
use super::{layout, siri_gl, theme};
use crate::{CapsuleLabels, CapsulePopupState};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapsuleAction {
    None,
    Cancel,
    Confirm,
}

const PILL_WIDTH: f32 = 176.0;
const PILL_HEIGHT: f32 = 42.0;
/// Tauri `getCapsuleHostMetrics(.., 'classic').bottomInset`。
const CAPSULE_BOTTOM_INSET: f32 = 16.0;
/// 徽章与药丸之间的间距（Tauri `badgeGap`）。
const CAPSULE_BADGE_GAP: f32 = 8.0;

fn translating_badge(ui: &mut egui::Ui, pill: egui::Rect, labels: &CapsuleLabels) {
    let label = labels.translating.as_str();
    let text_width = layout::text_width(ui, label, 10.5);
    let width = text_width + 5.0 + 5.0 + 20.0;
    let height = 19.0;
    let rect = egui::Rect::from_center_size(
        egui::pos2(
            pill.center().x,
            pill.top() - CAPSULE_BADGE_GAP - height / 2.0,
        ),
        egui::vec2(width, height),
    );
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        egui::CornerRadius::same((height / 2.0) as u8),
        theme::CAPSULE_BADGE_BG,
    );
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same((height / 2.0) as u8),
        egui::Stroke::new(0.5, theme::CAPSULE_BADGE_BORDER),
        egui::StrokeKind::Inside,
    );
    let dot = egui::pos2(rect.left() + 10.0, rect.center().y);
    painter.circle_filled(dot, 2.5, theme::BLUE);
    painter.text(
        egui::pos2(dot.x + 5.0 + 2.5, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(10.5),
        theme::BLUE,
    );
}

/// A transparent spectral listening indicator without interactive controls.
pub fn dictation_capsule(
    root_ui: &mut egui::Ui,
    state: &CapsulePopupState,
    labels: &CapsuleLabels,
) -> CapsuleAction {
    let phase = state.phase.to_ascii_lowercase();
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(root_ui, |ui| {
            // Tauri 经典药丸宿主：窗口高 100，药丸水平居中、距底 16，徽章再上移 8。
            let available = ui.available_rect_before_wrap();
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    available.center().x - PILL_WIDTH / 2.0,
                    available.bottom() - CAPSULE_BOTTOM_INSET - PILL_HEIGHT,
                ),
                egui::vec2(PILL_WIDTH, PILL_HEIGHT),
            );
            let _ = ui.allocate_rect(rect, egui::Sense::hover());
            if state.translation_active {
                translating_badge(ui, rect, labels);
            }
            let center = rect.shrink(3.0);
            let processing = matches!(
                phase.as_str(),
                "starting" | "transcribing" | "polishing" | "inserting"
            );
            if phase == "recording" {
                // Draw the spectral ribbons with egui primitives so they work on
                // the Vulkan path too (the legacy GL shader callback is unavailable there).
                let drive = siri_gl::SiriDrive {
                    level: state.audio_level.unwrap_or_default(),
                    resolved: 1.0,
                    speed: 1.0,
                    warming: state.audio_level.is_none(),
                };
                let dt = ui.input(|input| input.stable_dt);
                let clock = siri_gl::tick(ui.ctx(), "capsule-siri-wave", drive, dt);
                let _ = siri_gl::paint(
                    ui,
                    center,
                    siri_gl::SiriGlow::wave(clock.time, clock.level),
                );
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(16));
            } else if processing {
                // Collapse the listening waveform into a processing orb.
                let drive = siri_gl::SiriDrive {
                    level: 0.0,
                    resolved: 0.0,
                    speed: 1.3,
                    warming: false,
                };
                let dt = ui.input(|input| input.stable_dt);
                let clock = siri_gl::tick(ui.ctx(), "capsule-siri-orb", drive, dt);
                // 0.3s 全聚圆心接住 wave 收拢的光点，再缓缓散开成环。
                let gather = (1.0 - (clock.time / 0.9).clamp(0.0, 1.0)).clamp(0.0, 1.0);
                let glow = siri_gl::SiriGlow::orb(clock.time, gather);
                if !siri_gl::paint(ui, center, glow) {
                    ui.painter().text(
                        center.center(),
                        egui::Align2::CENTER_CENTER,
                        labels.thinking.as_str(),
                        egui::FontId::proportional(17.0),
                        theme::INK,
                    );
                }
            } else if state.text.is_empty() {
                let label = if phase == "cancelled" {
                    labels.cancelled.as_str()
                } else if phase == "failed" {
                    labels.error.as_str()
                } else {
                    labels.thinking.as_str()
                };
                ui.painter().text(
                    center.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(11.0),
                    if phase == "failed" {
                        theme::ERR
                    } else {
                        theme::INK
                    },
                );
            } else {
                // 11px/500 单行居中，超长省略（Tauri `getCapsuleMessageLayout`）。
                let galley =
                    layout::text_galley(ui, &state.text, theme::INK_2, 11.0, center.width(), 1);
                ui.painter().galley(
                    egui::pos2(
                        center.center().x - galley.rect.width() / 2.0,
                        center.center().y - galley.rect.height() / 2.0,
                    ),
                    galley,
                    theme::INK_2,
                );
            }
        });
    CapsuleAction::None
}
