use eframe::egui;

use openless_linux_egui::{tr_l10n, Lang};

use super::theme;

pub const SIDEBAR_WIDTH: f32 = 226.0;
pub const TITLEBAR_HEIGHT: f32 = 36.0;
const WINDOW_MARGIN: f32 = 0.0;
const WINDOW_RADIUS: u8 = 14;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Overview,
    History,
    Vocabulary,
    Styles,
    Marketplace,
    Providers,
    Assistant,
    Translation,
    Corrections,
}

impl Page {
    pub fn title(self, lang: Lang) -> &'static str {
        let key = match self {
            Self::Overview => "nav.overview",
            Self::History => "nav.history",
            Self::Vocabulary => "nav.vocab",
            Self::Styles => "nav.styles",
            Self::Marketplace => "nav.marketplace",
            Self::Providers => "nav.providers",
            Self::Assistant => "nav.assistant",
            Self::Translation => "nav.translation",
            Self::Corrections => "nav.corrections",
        };
        tr_l10n(lang, key)
    }

    pub fn nav_title(self, lang: Lang) -> &'static str {
        if self == Self::Providers {
            tr_l10n(lang, "nav.settings")
        } else {
            self.title(lang)
        }
    }
}

#[derive(Clone, Copy)]
enum IconName {
    Overview,
    History,
    Vocabulary,
    Style,
    Tools,
    Settings,
}

fn window_rect(ctx: &egui::Context) -> egui::Rect {
    ctx.content_rect().shrink(WINDOW_MARGIN)
}

fn body_rect(ctx: &egui::Context) -> egui::Rect {
    let window = window_rect(ctx);
    egui::Rect::from_min_max(window.min + egui::vec2(0.0, TITLEBAR_HEIGHT), window.max)
}

fn app_icon(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("openless-shell-app-icon");
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let image = image::load_from_memory(include_bytes!("../../../public/AppIcon.png"))
        .expect("OpenLess AppIcon.png must be valid")
        .into_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    let texture = ctx.load_texture("openless-app-icon", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}

pub fn titlebar(ctx: &egui::Context) {
    let window = window_rect(ctx);
    let body = body_rect(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("openless-window-background"),
    ));
    painter.rect_filled(
        window,
        egui::CornerRadius::same(WINDOW_RADIUS),
        theme::surface(),
    );
    painter.rect_filled(
        body,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: WINDOW_RADIUS,
            se: WINDOW_RADIUS,
        },
        theme::canvas(),
    );
    painter.rect_stroke(
        window,
        egui::CornerRadius::same(WINDOW_RADIUS),
        egui::Stroke::new(1.0, theme::line()),
        egui::StrokeKind::Inside,
    );

    egui::Area::new(egui::Id::new("openless-titlebar"))
        .order(egui::Order::Middle)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(window.width(), TITLEBAR_HEIGHT));
            let titlebar = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(window.width(), TITLEBAR_HEIGHT),
            );
            let drag = ui.interact(
                titlebar,
                ui.id().with("titlebar-drag"),
                egui::Sense::click_and_drag(),
            );
            if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            let texture = app_icon(ctx);
            ui.painter().image(
                texture.id(),
                egui::Rect::from_center_size(
                    egui::pos2(16.0, TITLEBAR_HEIGHT / 2.0),
                    egui::vec2(18.0, 18.0),
                ),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            ui.painter().text(
                egui::pos2(34.0, TITLEBAR_HEIGHT / 2.0 + 0.5),
                egui::Align2::LEFT_CENTER,
                "OpenLess",
                egui::FontId::proportional(13.0),
                theme::ink_2(),
            );

            let button_width = 40.0;
            let close = egui::Rect::from_min_max(
                egui::pos2(titlebar.right() - button_width, 0.0),
                titlebar.right_bottom(),
            );
            let maximize = close.translate(egui::vec2(-button_width, 0.0));
            let minimize = maximize.translate(egui::vec2(-button_width, 0.0));
            let close_response = ui.interact(close, ui.id().with("close"), egui::Sense::click());
            let maximize_response =
                ui.interact(maximize, ui.id().with("maximize"), egui::Sense::click());
            let minimize_response =
                ui.interact(minimize, ui.id().with("minimize"), egui::Sense::click());
            if close_response.clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if maximize_response.clicked() {
                let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            if minimize_response.clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            for (rect, response) in [
                (minimize, &minimize_response),
                (maximize, &maximize_response),
                (close, &close_response),
            ] {
                if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(6), theme::surface_2());
                }
            }
            let stroke = egui::Stroke::new(1.0, theme::ink_3());
            ui.painter().line_segment(
                [
                    minimize.center() - egui::vec2(5.0, 0.0),
                    minimize.center() + egui::vec2(5.0, 0.0),
                ],
                stroke,
            );
            ui.painter().rect_stroke(
                maximize.shrink(14.0),
                egui::CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Inside,
            );
            ui.painter().line_segment(
                [
                    close.center() - egui::vec2(5.0, 5.0),
                    close.center() + egui::vec2(5.0, 5.0),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    close.center() + egui::vec2(5.0, -5.0),
                    close.center() + egui::vec2(-5.0, 5.0),
                ],
                stroke,
            );
        });
}

pub fn sidebar(ctx: &egui::Context, active: &mut Page, status: &str, lang: Lang) {
    let body = body_rect(ctx);
    egui::Area::new(egui::Id::new("openless-sidebar"))
        .order(egui::Order::Middle)
        .fixed_pos(body.min)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(SIDEBAR_WIDTH, body.height()));
            ui.set_clip_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(SIDEBAR_WIDTH, body.height()),
            ));
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, theme::surface());
            ui.painter().line_segment(
                [
                    egui::pos2(SIDEBAR_WIDTH, 0.0),
                    egui::pos2(SIDEBAR_WIDTH, body.height()),
                ],
                egui::Stroke::new(1.0, theme::line()),
            );
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(10, 12))
                .show(ui, |ui| {
                    ui.set_width(SIDEBAR_WIDTH - 20.0);
                    nav(ui, active, Page::Overview, IconName::Overview, lang);
                    nav(ui, active, Page::History, IconName::History, lang);
                    nav(ui, active, Page::Vocabulary, IconName::Vocabulary, lang);
                    ui.add_space(5.0);
                    group_label(ui, tr_l10n(lang, "nav.styles"), IconName::Style);
                    subnav(ui, active, Page::Styles, lang);
                    subnav(ui, active, Page::Marketplace, lang);
                    ui.add_space(2.0);
                    group_label(ui, tr_l10n(lang, "nav.assistant"), IconName::Tools);
                    subnav(ui, active, Page::Assistant, lang);

                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        nav(ui, active, Page::Providers, IconName::Settings, lang);
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                egui::Frame::new()
                                    .fill(theme::blue_soft())
                                    .corner_radius(egui::CornerRadius::same(7))
                                    .inner_margin(egui::Margin::symmetric(6, 2))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new("BETA")
                                                .size(9.5)
                                                .strong()
                                                .color(theme::blue()),
                                        );
                                    });
                                ui.add_space(3.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} · {}",
                                        env!("CARGO_PKG_VERSION"),
                                        status
                                    ))
                                    .size(10.5)
                                    .color(theme::ink_4()),
                                );
                            });
                        });
                    });
                });
        });
}

fn nav(ui: &mut egui::Ui, active: &mut Page, page: Page, icon: IconName, lang: Lang) {
    let selected = *active == page;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 32.0), egui::Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::surface_2());
    }
    let color = if selected {
        theme::ink()
    } else {
        theme::ink_3()
    };
    draw_icon(ui, rect.min + egui::vec2(20.0, 16.0), icon, color);
    ui.painter().text(
        rect.min + egui::vec2(38.0, 16.0),
        egui::Align2::LEFT_CENTER,
        page.nav_title(lang),
        egui::FontId::proportional(13.0),
        color,
    );
    if response.clicked() {
        *active = page;
    }
}

fn subnav(ui: &mut egui::Ui, active: &mut Page, page: Page, lang: Lang) {
    let selected = *active == page;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 30.0), egui::Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::surface_2());
    }
    ui.painter().text(
        rect.min + egui::vec2(30.0, 15.0),
        egui::Align2::LEFT_CENTER,
        page.nav_title(lang),
        egui::FontId::proportional(12.5),
        if selected {
            theme::ink()
        } else {
            theme::ink_3()
        },
    );
    if response.clicked() {
        *active = page;
    }
}

fn group_label(ui: &mut egui::Ui, label: &str, icon: IconName) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 32.0), egui::Sense::hover());
    draw_icon(ui, rect.min + egui::vec2(20.0, 16.0), icon, theme::ink_3());
    ui.painter().text(
        rect.min + egui::vec2(38.0, 16.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        theme::ink_3(),
    );
}

fn draw_icon(ui: &egui::Ui, center: egui::Pos2, icon: IconName, color: egui::Color32) {
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.25, color);
    match icon {
        IconName::Overview => {
            painter.line_segment(
                [
                    center + egui::vec2(-6.0, -6.0),
                    center + egui::vec2(-6.0, 6.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(-6.0, 6.0),
                    center + egui::vec2(6.0, 6.0),
                ],
                stroke,
            );
            for (x, top) in [(-2.5, 1.0), (1.5, -4.0), (5.5, -1.5)] {
                painter.line_segment(
                    [center + egui::vec2(x, 5.0), center + egui::vec2(x, top)],
                    stroke,
                );
            }
        }
        IconName::History => {
            painter.circle_stroke(center, 6.0, stroke);
            painter.line_segment([center, center + egui::vec2(0.0, -3.5)], stroke);
            painter.line_segment([center, center + egui::vec2(3.0, 2.0)], stroke);
        }
        IconName::Vocabulary => {
            for y in [-4.0, 0.0, 4.0] {
                painter.circle_filled(center + egui::vec2(-5.0, y), 1.0, color);
                painter.line_segment(
                    [center + egui::vec2(-2.0, y), center + egui::vec2(6.0, y)],
                    stroke,
                );
            }
        }
        IconName::Style => {
            painter.line_segment(
                [
                    center + egui::vec2(-5.0, 5.0),
                    center + egui::vec2(4.0, -4.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(2.0, -5.0),
                    center + egui::vec2(5.0, -2.0),
                ],
                stroke,
            );
        }
        IconName::Tools => {
            painter.circle_stroke(center, 5.5, stroke);
            painter.line_segment(
                [
                    center + egui::vec2(-3.5, 3.5),
                    center + egui::vec2(3.5, -3.5),
                ],
                stroke,
            );
        }
        IconName::Settings => {
            painter.circle_stroke(center, 5.5, stroke);
            painter.circle_stroke(center, 2.0, stroke);
            for angle in [
                0.0_f32,
                std::f32::consts::FRAC_PI_2,
                std::f32::consts::PI,
                3.0 * std::f32::consts::FRAC_PI_2,
            ] {
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment([center + direction * 5.5, center + direction * 7.0], stroke);
            }
        }
    }
}

pub fn content_panel(
    ctx: &egui::Context,
    _active: Page,
    _lang: Lang,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let body = body_rect(ctx);
    let content = egui::Rect::from_min_max(
        egui::pos2(body.left() + SIDEBAR_WIDTH + 28.0, body.top()),
        egui::pos2(body.right() - 2.0, body.bottom() - 8.0),
    );
    egui::Area::new(egui::Id::new("openless-content"))
        .order(egui::Order::Middle)
        .fixed_pos(content.min)
        .show(ctx, |ui| {
            ui.set_min_size(content.size());
            ui.set_clip_rect(egui::Rect::from_min_size(egui::Pos2::ZERO, content.size()));
            let scroll = &mut ui.style_mut().spacing.scroll;
            scroll.floating = true;
            scroll.bar_width = 8.0;
            scroll.handle_min_length = 24.0;
            scroll.bar_inner_margin = 0.0;
            scroll.bar_outer_margin = 0.0;
            scroll.foreground_color = false;
            scroll.floating_width = 6.0;
            scroll.floating_allocated_width = 0.0;
            ui.add_space(22.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, add_contents);
        });
}
