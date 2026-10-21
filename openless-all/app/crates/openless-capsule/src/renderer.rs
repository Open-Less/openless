//! Disposable desktop capsule renderer, based on the Linux frontend.
use super::{icons, layout, siri_gl, theme};
use crate::{CapsuleLabels, CapsulePopupState};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapsuleAction {
    None,
    Cancel,
    Confirm,
}

const PILL_WIDTH: f32 = 176.0;
const PILL_HEIGHT: f32 = 42.0;
/// Round icon buttons in the capsule / composer.
const ROUND_BUTTON: f32 = 28.0;
/// Tauri `getCapsuleHostMetrics(.., 'classic').bottomInset`。
const CAPSULE_BOTTOM_INSET: f32 = 16.0;
/// Typeless 胶囊（Tauri `CapsuleStyles.css`：176×64、46×46 圆形按钮、11 根波形）。
const TYPELESS_WIDTH: f32 = 176.0;
const TYPELESS_HEIGHT: f32 = 64.0;
const TYPELESS_BUTTON: f32 = 46.0;
/// `.ol-typeless-*` 调色板。
const TYPELESS_BG: egui::Color32 = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
const TYPELESS_BORDER: egui::Color32 = egui::Color32::from_rgb(0x52, 0x52, 0x5b);
const TYPELESS_INK: egui::Color32 = egui::Color32::from_rgb(0xfa, 0xfa, 0xfa);
const TYPELESS_BUTTON_BG: egui::Color32 = egui::Color32::from_rgb(0x3f, 0x3f, 0x46);
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

pub fn dictation_capsule(
    root_ui: &mut egui::Ui,
    state: &CapsulePopupState,
    labels: &CapsuleLabels,
) -> CapsuleAction {
    let mut action = CapsuleAction::None;
    let phase = state.phase.to_ascii_lowercase();
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(root_ui, |ui| {
            // 胶囊进程的首帧预热（同 QA 面板；录音环与 Siri 波都是 GPU 路径）。
            // Tauri `capsuleStyle`：siri = 流光药丸（GPU 波/环），classic = 经典药丸 +
            // 五根音量条，typeless = 176×64 深色胶囊 + 11 根波形。
            let style = state.style.as_str();
            let typeless = style == "typeless";
            // 未知/空值走 siri（默认样式），只有显式选择 classic 才关掉 GPU 光效。
            let use_gpu = !typeless && style != "classic";
            let (pill_width, pill_height, button, bar_count) = if typeless {
                (TYPELESS_WIDTH, TYPELESS_HEIGHT, TYPELESS_BUTTON, 11)
            } else {
                (PILL_WIDTH, PILL_HEIGHT, ROUND_BUTTON, 5)
            };
            let siri = use_gpu;
            let (pill_bg, pill_border, pill_ink) = if typeless {
                (TYPELESS_BG, TYPELESS_BORDER, TYPELESS_INK)
            } else if siri {
                (
                    egui::Color32::TRANSPARENT,
                    egui::Color32::TRANSPARENT,
                    theme::INK_2,
                )
            } else {
                (theme::SURFACE, theme::LINE, theme::INK_2)
            };
            // Tauri 经典药丸宿主：窗口高 100，药丸水平居中、距底 16，徽章再上移 8。
            let available = ui.available_rect_before_wrap();
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    available.center().x - pill_width / 2.0,
                    available.bottom() - CAPSULE_BOTTOM_INSET - pill_height,
                ),
                egui::vec2(pill_width, pill_height),
            );
            let _ = ui.allocate_rect(rect, egui::Sense::hover());
            if state.translation_active {
                translating_badge(ui, rect, labels);
            }
            // Tauri 的经典药丸只有「1px 中性描边」+「随音量轻微放大」两件事
            // （Capsule.tsx 的 ClassicPill：border 1px var(--ol-capsule-pill-border)、
            // transform scale(1 + ambient * 0.018)），**没有**任何外圈扫光/描边颜色变化。
            // 所以这里不再把录音相位画成红圈（那是本仓自己加的，用户报「有一个红边」）；
            // 运动感只保留药丸中心的音量波形。
            let ambient = if phase == "recording" {
                state.audio_level.unwrap_or(0.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let pill =
                egui::Rect::from_center_size(rect.center(), rect.size() * (1.0 + ambient * 0.018));
            if !siri {
                ui.painter().rect_filled(
                    pill,
                    egui::CornerRadius::same((pill_height / 2.0) as u8),
                    pill_bg,
                );
                ui.painter().rect_stroke(
                    pill,
                    egui::CornerRadius::same((pill_height / 2.0) as u8),
                    egui::Stroke::new(1.0, pill_border),
                    egui::StrokeKind::Inside,
                );
            }
            // Siri is a clean, transparent listening indicator. The cancel /
            // confirm affordances belong to the classic and Typeless capsules.
            let center = if siri {
                rect.shrink(3.0)
            } else {
                let inset = if typeless { 7.0 } else { 8.0 };
                let cancel_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + inset + button / 2.0, rect.center().y),
                    egui::vec2(button, button),
                );
                let cancel = ui.interact(
                    cancel_rect,
                    ui.scope_id().with("openless-capsule-cancel"),
                    egui::Sense::click(),
                );
                let cancel_fill = if typeless {
                    TYPELESS_BUTTON_BG
                } else {
                    theme::SURFACE_2
                };
                round_button(
                    ui,
                    cancel_rect,
                    icons::IconName::Close,
                    cancel.hovered(),
                    cancel_fill,
                    pill_ink,
                );
                if cancel.clicked() {
                    action = CapsuleAction::Cancel;
                }
                let confirm_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.right() - inset - button / 2.0, rect.center().y),
                    egui::vec2(button, button),
                );
                let confirm = ui.interact(
                    confirm_rect,
                    ui.scope_id().with("openless-capsule-confirm"),
                    egui::Sense::click(),
                );
                let (confirm_fill, confirm_ink) = if typeless {
                    (TYPELESS_INK, TYPELESS_BG)
                } else {
                    (theme::SURFACE_2, theme::INK_2)
                };
                round_button(
                    ui,
                    confirm_rect,
                    icons::IconName::Check,
                    confirm.hovered(),
                    confirm_fill,
                    confirm_ink,
                );
                if confirm.clicked() {
                    action = CapsuleAction::Confirm;
                }
                egui::Rect::from_min_max(
                    egui::pos2(cancel_rect.right() + 4.0, rect.top() + 4.0),
                    egui::pos2(confirm_rect.left() - 4.0, rect.bottom() - 4.0),
                )
            };
            let processing = matches!(
                phase.as_str(),
                "starting" | "transcribing" | "polishing" | "inserting"
            );
            if phase == "recording" {
                // Siri capsules are a transparent overlay. Draw the spectral
                // ribbons with egui primitives so they work on the Vulkan path
                // too (the legacy GL shader callback is unavailable there).
                let drive = siri_gl::SiriDrive {
                    level: state.audio_level.unwrap_or_default(),
                    resolved: 1.0,
                    speed: 1.0,
                    warming: state.audio_level.is_none(),
                };
                let dt = ui.input(|input| input.stable_dt);
                let clock = siri_gl::tick(ui.ctx(), "capsule-siri-wave", drive, dt);
                if siri {
                    let _ = siri_gl::paint(
                        ui,
                        center,
                        siri_gl::SiriGlow::wave(clock.time, clock.level),
                    );
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(16));
                } else {
                    audio_bars(
                        ui,
                        center,
                        state.audio_level.unwrap_or_default(),
                        bar_count,
                        pill_ink,
                    );
                }
            } else if processing {
                // 思考中：Siri 流体圆点（orb），从 wave 收拢的光点化开成环。
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
                if !use_gpu || !siri_gl::paint(ui, center, glow) {
                    ui.painter().text(
                        center.center(),
                        egui::Align2::CENTER_CENTER,
                        labels.thinking.as_str(),
                        egui::FontId::proportional(17.0),
                        theme::INK,
                    );
                }
            } else if state.text.is_empty() {
                let label = if processing {
                    labels.thinking.as_str()
                } else if phase == "cancelled" {
                    labels.cancelled.as_str()
                } else if phase == "failed" {
                    labels.error.as_str()
                } else {
                    labels.thinking.as_str()
                };
                let size = if processing { 17.0 } else { 11.0 };
                ui.painter().text(
                    center.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(size),
                    if phase == "failed" {
                        theme::ERR
                    } else if typeless {
                        TYPELESS_INK
                    } else {
                        theme::INK
                    },
                );
            } else {
                // 11px/500 单行居中，超长省略（Tauri `getCapsuleMessageLayout`）。
                let galley =
                    layout::text_galley(ui, &state.text, pill_ink, 11.0, center.width(), 1);
                ui.painter().galley(
                    egui::pos2(
                        center.center().x - galley.rect.width() / 2.0,
                        center.center().y - galley.rect.height() / 2.0,
                    ),
                    galley,
                    pill_ink,
                );
            }
        });
    action
}

fn round_button(
    ui: &egui::Ui,
    rect: egui::Rect,
    icon: icons::IconName,
    hovered: bool,
    fill: egui::Color32,
    ink: egui::Color32,
) {
    ui.painter().circle_filled(
        rect.center(),
        rect.width() / 2.0,
        if hovered {
            fill.gamma_multiply(1.06)
        } else {
            fill
        },
    );
    ui.painter().circle_stroke(
        rect.center(),
        rect.width() / 2.0,
        egui::Stroke::new(0.8, theme::LINE),
    );
    icons::draw_icon(ui, rect.center(), icon, ink);
}

fn audio_bars(ui: &egui::Ui, rect: egui::Rect, level: f32, bar_count: usize, ink: egui::Color32) {
    const CLASSIC_ENVELOPE: [f32; 5] = [0.55, 0.85, 1.0, 0.85, 0.55];
    // Tauri `WAVE_ENVELOPE`（Typeless 的 11 根）。
    const TYPELESS_ENVELOPE: [f32; 11] = [
        0.28, 0.44, 0.63, 0.82, 0.96, 1.0, 0.96, 0.82, 0.63, 0.44, 0.28,
    ];
    let envelope: &[f32] = if bar_count > CLASSIC_ENVELOPE.len() {
        &TYPELESS_ENVELOPE
    } else {
        &CLASSIC_ENVELOPE
    };
    const BASE: f32 = 2.0;
    let max = if bar_count > CLASSIC_ENVELOPE.len() {
        26.0
    } else {
        24.0
    };
    let voice = level.clamp(0.0, 1.0);
    let gated = ((voice - 0.012) / (0.34 - 0.012)).clamp(0.0, 1.0);
    let eased = gated * gated * (3.0 - 2.0 * gated);
    let visual = eased.powf(0.42);
    let bar_width = 3.0;
    let gap = 3.0;
    let total = envelope.len() as f32 * bar_width + (envelope.len() - 1) as f32 * gap;
    let mut x = rect.center().x - total / 2.0;
    for envelope in envelope {
        let height = BASE + (max - BASE) * visual * envelope;
        ui.painter().rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(x + bar_width / 2.0, rect.center().y),
                egui::vec2(bar_width, height),
            ),
            egui::CornerRadius::same(2),
            ink,
        );
        x += bar_width + gap;
    }
}
