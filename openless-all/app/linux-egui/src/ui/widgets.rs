//! 2.0 共享组件：卡片、状态胶囊、按钮、分段控件、开关与页头。
//!
//! 视觉规格对应 React 侧组件：`--ol-card-*`（卡片）、`--ol-pill-*`（胶囊）、
//! `--ol-primary/accent/danger-solid-*`（按钮）、`--ol-segmented-*`（分段）、
//! `--ol-toggle-*`（开关）。egui 0.31 的 `CornerRadius` 以 u8、`Margin` 以 i8 计。

use crate::design_tokens::{self as tokens};
use crate::ui::theme::{current, Palette};

use eframe::egui;

/// 页头：22px 标题 + 13px 副标题（React 页面的 `title + description` 结构）。
pub fn page_header(ui: &mut egui::Ui, title: &str, subtitle: Option<&str>) {
    let p = current(ui.ctx());
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(title)
            .size(22.0)
            .strong()
            .color(p.ink()),
    );
    if let Some(subtitle) = subtitle {
        ui.label(egui::RichText::new(subtitle).size(13.0).color(p.ink_3()));
    }
    ui.add_space(12.0);
}

/// 分区标题（卡片内 15px 半粗标题）。
pub fn section_title(ui: &mut egui::Ui, title: &str) {
    let p = current(ui.ctx());
    ui.label(
        egui::RichText::new(title)
            .size(15.0)
            .strong()
            .color(p.ink()),
    );
}

/// 白底卡片：`--ol-card-bg/-border` + 14px 圆角 + 16px 内边距。
pub fn card<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let p = current(ui.ctx());
    egui::Frame::default()
        .fill(p.surface())
        .stroke(egui::Stroke::new(0.5_f32, p.line()))
        .corner_radius(egui::CornerRadius::same(tokens::radius::CARD))
        .inner_margin(egui::Margin::same(16))
        .show(ui, add_contents)
}

/// 软底卡片：`--ol-surface-2` 底 + 14px 圆角（信息块、次级分组）。
pub fn subtle_card<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let p = current(ui.ctx());
    egui::Frame::default()
        .fill(p.surface_2())
        .corner_radius(egui::CornerRadius::same(tokens::radius::CARD))
        .inner_margin(egui::Margin::same(14))
        .show(ui, add_contents)
}

/// 蓝色软底提示块：`--ol-blue-soft` + blue 文字（引导/提示）。
pub fn info_panel<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let p = current(ui.ctx());
    egui::Frame::default()
        .fill(p.blue_soft())
        .corner_radius(egui::CornerRadius::same(tokens::radius::CARD))
        .inner_margin(egui::Margin::same(14))
        .show(ui, add_contents)
}

/// 状态胶囊的语义色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Err,
    Info,
}

impl Status {
    fn colors(self, p: &Palette) -> (egui::Color32, egui::Color32) {
        match self {
            Self::Ok => (p.ok_soft(), p.ok()),
            Self::Warn => (p.warn_soft(), p.warn()),
            Self::Err => (p.err(), egui::Color32::WHITE),
            Self::Info => (p.blue_soft(), p.blue()),
        }
    }
}

/// 状态胶囊：软底 + 语义色文字（React 的 ok/warn/err pill）。
pub fn status_pill(ui: &mut egui::Ui, text: &str, status: Status) -> egui::Response {
    let p = current(ui.ctx());
    let (bg, fg) = status.colors(&p);
    pill(ui, text, bg, fg)
}

/// 通用胶囊绘制：全圆角软底 + 居中文字。
pub fn pill(ui: &mut egui::Ui, text: &str, bg: egui::Color32, fg: egui::Color32) -> egui::Response {
    egui::Frame::default()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(12.0).strong().color(fg));
        })
        .response
}

/// 主按钮：`--ol-primary-solid`（浅色主题为深墨实底）。`enabled=false` 走
/// `add_enabled` 真禁用（egui 的 `Response::enabled` 不追溯禁用控件）。
pub fn primary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let p = current(ui.ctx());
    let button = egui::Button::new(
        egui::RichText::new(label)
            .color(p.primary_ink())
            .strong()
            .size(14.0),
    )
    .fill(p.primary_bg())
    .stroke(egui::Stroke::NONE)
    .corner_radius(egui::CornerRadius::same(tokens::radius::CONTROL));
    add_maybe_enabled(ui, button, enabled)
}

/// 强调按钮：`--ol-accent-solid`（蓝色实底）。
pub fn blue_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let p = current(ui.ctx());
    let button = egui::Button::new(
        egui::RichText::new(label)
            .color(p.on_accent())
            .strong()
            .size(14.0),
    )
    .fill(p.blue())
    .stroke(egui::Stroke::NONE)
    .corner_radius(egui::CornerRadius::same(tokens::radius::CONTROL));
    add_maybe_enabled(ui, button, enabled)
}

/// 危险按钮：`--ol-danger-solid`。
pub fn danger_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let p = current(ui.ctx());
    let button = egui::Button::new(
        egui::RichText::new(label)
            .color(egui::Color32::WHITE)
            .strong()
            .size(14.0),
    )
    .fill(p.err())
    .stroke(egui::Stroke::NONE)
    .corner_radius(egui::CornerRadius::same(tokens::radius::CONTROL));
    add_maybe_enabled(ui, button, enabled)
}

/// 次级按钮（主题默认按钮：白底 + line 边框）。
pub fn secondary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(label).size(14.0));
    add_maybe_enabled(ui, button, enabled)
}

fn add_maybe_enabled(ui: &mut egui::Ui, button: egui::Button<'_>, enabled: bool) -> egui::Response {
    if enabled {
        ui.add(button)
    } else {
        ui.add_enabled(false, button)
    }
}

/// 弱化文字提示（`--ol-ink-4` 小字）。
pub fn hint(ui: &mut egui::Ui, text: &str) {
    let p = current(ui.ctx());
    ui.label(egui::RichText::new(text).size(12.5).color(p.ink_4()));
}

/// 键值行：固定键列（`--ol-ink-3`）+ 值（`--ol-ink-2`），值可换行。
pub fn kv_row(ui: &mut egui::Ui, key: &str, value: &str) {
    let p = current(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_sized(
            [200.0, 18.0],
            egui::Label::new(egui::RichText::new(key).size(13.0).color(p.ink_3())),
        );
        ui.add(egui::Label::new(egui::RichText::new(value).size(13.0).color(p.ink_2())).wrap());
    });
}

/// 1px 分隔线（`--ol-line`）。
pub fn divider(ui: &mut egui::Ui) {
    let p = current(ui.ctx());
    ui.add_space(4.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(1), p.line());
    ui.add_space(4.0);
}

/// 分段控件：`--ol-segmented-bg` 容器 + 选中白底片（`--ol-segmented-active-bg`）。
/// 返回被点中的选项下标。
pub fn segmented(
    ui: &mut egui::Ui,
    _id_salt: &str,
    options: &[&str],
    selected: usize,
) -> Option<usize> {
    let p = current(ui.ctx());
    let mut picked = None;
    egui::Frame::default()
        .fill(p.segmented_bg())
        .corner_radius(egui::CornerRadius::same(tokens::radius::CONTROL))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.style_mut().interaction.selectable_labels = false;
                for (index, option) in options.iter().enumerate() {
                    let active = index == selected;
                    let response = if active {
                        egui::Frame::default()
                            .fill(p.segmented_active_bg())
                            .corner_radius(egui::CornerRadius::same(tokens::radius::SM))
                            .inner_margin(egui::Margin::symmetric(12, 5))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(*option)
                                        .size(13.0)
                                        .strong()
                                        .color(p.ink()),
                                )
                            })
                            .response
                    } else {
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new(*option).size(13.0).color(p.ink_3()),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(tokens::radius::SM)),
                        )
                    };
                    if response.clicked() && !active {
                        picked = Some(index);
                    }
                }
            });
        });
    picked
}

/// 开关（React 的 toggle switch）：34x20 轨道 + 白色圆钮，点按切换。
/// 返回是否发生了切换；`on` 由调用方持有。关闭轨道色取 `--ol-toggle-off-bg`
/// 的近似（浅色 ink-5 / 深色 ink-3，保证暗底可见）。
pub fn switch(ui: &mut egui::Ui, on: bool) -> egui::Response {
    let p = current(ui.ctx());
    let size = egui::vec2(34.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let track = if on {
        p.blue()
    } else if p.is_dark() {
        p.ink_3()
    } else {
        p.ink_5()
    };
    painter.rect_filled(
        rect,
        egui::CornerRadius::same((rect.height() / 2.0) as u8),
        track,
    );
    let knob_d = 14.0;
    let knob_y = rect.center().y;
    let knob_center_x = if on {
        rect.right() - knob_d / 2.0 - 3.0
    } else {
        rect.left() + knob_d / 2.0 + 3.0
    };
    painter.circle_filled(
        egui::pos2(knob_center_x, knob_y),
        knob_d / 2.0,
        egui::Color32::WHITE,
    );
    response
}

/// 带标签的开关行：左标签右开关（React 设置行的排布）。
pub fn toggle_row(ui: &mut egui::Ui, on: &mut bool, label: &str, hint_text: Option<&str>) {
    let p = current(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).size(14.0).color(p.ink_2()));
            if let Some(hint_text) = hint_text {
                ui.label(egui::RichText::new(hint_text).size(12.0).color(p.ink_4()));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if switch(ui, *on).clicked() {
                changed = true;
            }
        });
    });
    if changed {
        *on = !*on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 状态胶囊与开关在无 CJK 字体的 headless 环境也能完成布局。
    #[test]
    fn widgets_render_headless() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply_theme(&ctx, false);
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    page_header(ui, "开始", Some("副标题"));
                    card(ui, |ui| {
                        section_title(ui, "卡片标题");
                        hint(ui, "弱化提示");
                    });
                    subtle_card(ui, |ui| {
                        status_pill(ui, "运行中", Status::Ok);
                        status_pill(ui, "待确认", Status::Warn);
                        status_pill(ui, "失败", Status::Err);
                    });
                    let mut selected = 0;
                    if let Some(picked) = segmented(ui, "t", &["听写", "润色"], selected) {
                        selected = picked;
                    }
                    let mut on = false;
                    toggle_row(ui, &mut on, "保持亮屏", Some("录手机输入时"));
                    assert_eq!(selected, 0);
                    assert!(!on);
                });
            },
        );
    }
}
