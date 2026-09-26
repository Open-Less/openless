//! Shared page helpers that are still consumed by other page modules.
//!
//! The style page moved to `style.rs`; the correction chip is shared with the
//! corrections page.

use eframe::egui;

use super::theme;

pub fn correction_chip(ui: &mut egui::Ui, label: &str, enabled: bool) -> (bool, bool) {
    let fill = if enabled {
        theme::SURFACE
    } else {
        theme::SURFACE_2
    };
    let text_color = if enabled { theme::INK } else { theme::INK_4 };
    let text_galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.5),
        text_color,
    );
    let close_size = 22.0;
    let width = 12.0 + text_galley.size().x + 8.0 + close_size + 10.0;
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
        egui::pos2(
            rect.left() + 12.0,
            rect.center().y - text_galley.size().y / 2.0,
        ),
        text_galley,
        text_color,
    );

    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 10.0 - close_size / 2.0, rect.center().y),
        egui::vec2(close_size, close_size),
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
