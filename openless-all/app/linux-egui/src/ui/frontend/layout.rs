use eframe::egui;

use super::icons::{self, IconName};
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, Page};

pub const SIDEBAR_WIDTH: f32 = 188.0;
pub const TITLEBAR_HEIGHT: f32 = 38.0;
const WINDOW_MARGIN: f32 = 6.0;
const WINDOW_RADIUS: u8 = 14;

// ── Window geometry helpers ─────────────────────────────────────────────────

pub fn window_rect(ctx: &egui::Context) -> egui::Rect {
    ctx.content_rect().shrink(WINDOW_MARGIN)
}

pub fn body_rect(ctx: &egui::Context) -> egui::Rect {
    let window = window_rect(ctx);
    egui::Rect::from_min_max(window.min + egui::vec2(0.0, TITLEBAR_HEIGHT), window.max)
}

// ── App icon ────────────────────────────────────────────────────────────────

pub fn load_app_icon(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("openless-frontend-app-icon");
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let image = image::load_from_memory(include_bytes!("../../../../public/AppIcon.png"))
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

pub fn paint_app_icon(ui: &egui::Ui, rect: egui::Rect, texture: &egui::TextureHandle) {
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

// ── Window background ───────────────────────────────────────────────────────

pub fn paint_window_background(ctx: &egui::Context) {
    let window = window_rect(ctx);
    let body = body_rect(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("openless-window-background"),
    ));
    painter.rect_filled(
        window,
        egui::CornerRadius::same(WINDOW_RADIUS),
        theme::SURFACE,
    );
    painter.rect_filled(
        body,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: WINDOW_RADIUS,
            se: WINDOW_RADIUS,
        },
        theme::CANVAS,
    );
    painter.rect_stroke(
        window,
        egui::CornerRadius::same(WINDOW_RADIUS),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
}

// ── Titlebar ────────────────────────────────────────────────────────────────

pub fn titlebar(ctx: &egui::Context, actions: &mut Vec<FrontendAction>) {
    let window = window_rect(ctx);
    let titlebar = egui::Rect::from_min_max(
        window.min,
        egui::pos2(window.max.x, window.min.y + TITLEBAR_HEIGHT),
    );

    egui::Area::new(egui::Id::new("openless-titlebar"))
        .order(egui::Order::Middle)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(window.width(), TITLEBAR_HEIGHT));
            let drag = ui.interact(
                titlebar,
                ui.id().with("titlebar-drag"),
                egui::Sense::click_and_drag(),
            );
            if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            let texture = load_app_icon(ctx);
            paint_app_icon(
                ui,
                egui::Rect::from_center_size(
                    window.min + egui::vec2(16.0, TITLEBAR_HEIGHT / 2.0),
                    egui::vec2(18.0, 18.0),
                ),
                &texture,
            );
            ui.painter().text(
                window.min + egui::vec2(34.0, TITLEBAR_HEIGHT / 2.0 + 0.5),
                egui::Align2::LEFT_CENTER,
                "OpenLess",
                egui::FontId::proportional(13.0),
                theme::INK_2,
            );

            let button_width = 40.0;
            let close = egui::Rect::from_min_max(
                egui::pos2(titlebar.right() - button_width, titlebar.top()),
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
                actions.push(FrontendAction::WindowClose);
            }
            if maximize_response.clicked() {
                actions.push(FrontendAction::WindowMaximize);
            }
            if minimize_response.clicked() {
                actions.push(FrontendAction::WindowMinimize);
            }
            for (rect, response) in [
                (minimize, &minimize_response),
                (maximize, &maximize_response),
                (close, &close_response),
            ] {
                if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(6), theme::SURFACE_2);
                }
            }
            let stroke = egui::Stroke::new(1.0, theme::INK_3);
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

// ── Resize handles ──────────────────────────────────────────────────────────

pub fn resize_handles(ctx: &egui::Context) {
    let window = window_rect(ctx);
    let edge = 10.0;
    let corner = 18.0;
    let left = window.left();
    let right = window.right();
    let top = window.top();
    let bottom = window.bottom();
    let zones = [
        (
            egui::Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(left + corner, top + corner),
            ),
            egui::ResizeDirection::NorthWest,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(right - corner, top),
                egui::pos2(right, top + corner),
            ),
            egui::ResizeDirection::NorthEast,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(left, bottom - corner),
                egui::pos2(left + corner, bottom),
            ),
            egui::ResizeDirection::SouthWest,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(right - corner, bottom - corner),
                egui::pos2(right, bottom),
            ),
            egui::ResizeDirection::SouthEast,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(left + corner, top),
                egui::pos2(right - corner, top + edge),
            ),
            egui::ResizeDirection::North,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(left + corner, bottom - edge),
                egui::pos2(right - corner, bottom),
            ),
            egui::ResizeDirection::South,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(left, top + corner),
                egui::pos2(left + edge, bottom - corner),
            ),
            egui::ResizeDirection::West,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(right - edge, top + corner),
                egui::pos2(right, bottom - corner),
            ),
            egui::ResizeDirection::East,
        ),
    ];

    // We need a temporary area to check for resize interactions.
    egui::Area::new(egui::Id::new("openless-resize-handles"))
        .order(egui::Order::Foreground)
        // `Area` itself registers an input region covering its complete size.
        // This layer spans the window, so leaving it interactable makes that
        // invisible region win hit testing over every button beneath it. The
        // individual edge widgets below remain interactive; only the empty
        // interior is click-through.
        .interactable(false)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(window.size());
            for (index, (rect, direction)) in zones.into_iter().enumerate() {
                let response =
                    ui.interact(rect, ui.id().with(("resize", index)), egui::Sense::drag());
                if response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
            }
        });
}

// ── Sidebar ─────────────────────────────────────────────────────────────────

pub fn sidebar(ctx: &egui::Context, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let body = body_rect(ctx);
    egui::Area::new(egui::Id::new("openless-sidebar"))
        .order(egui::Order::Middle)
        .fixed_pos(body.min)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(SIDEBAR_WIDTH, body.height()));
            ui.set_clip_rect(egui::Rect::from_min_size(
                body.min,
                egui::vec2(SIDEBAR_WIDTH, body.height()),
            ));
            ui.painter().rect_filled(ui.max_rect(), 0.0, theme::SURFACE);
            ui.painter().line_segment(
                [
                    egui::pos2(SIDEBAR_WIDTH, 0.0),
                    egui::pos2(SIDEBAR_WIDTH, body.height()),
                ],
                egui::Stroke::new(1.0, theme::LINE),
            );
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(10, 12))
                .show(ui, |ui| {
                    ui.set_width(SIDEBAR_WIDTH - 20.0);
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(20.0, 22.0), egui::Sense::hover());
                        let texture = load_app_icon(ctx);
                        paint_app_icon(ui, rect, &texture);
                        ui.label(egui::RichText::new("OpenLess").strong().size(14.0));
                    });
                    ui.add_space(16.0);
                    nav(ui, vm, "概览", Page::Overview, IconName::Overview, actions);
                    nav(ui, vm, "历史", Page::History, IconName::History, actions);
                    nav(ui, vm, "词汇表", Page::Vocab, IconName::Vocab, actions);
                    ui.add_space(4.0);
                    group(ui, vm, "风格", IconName::Style, actions);
                    if vm.style_open {
                        subnav(ui, vm, "润色模式", Page::Style, actions);
                        subnav(ui, vm, "风格市场", Page::Marketplace, actions);
                    }
                    group(ui, vm, "工具", IconName::SelectionAsk, actions);
                    if vm.tools_open {
                        subnav(ui, vm, "划词追问", Page::SelectionAsk, actions);
                        subnav(ui, vm, "翻译", Page::Translation, actions);
                    }
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        nav_with_icon(ui, vm, "设置", Page::Settings, IconName::Settings, actions);
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                egui::Frame::new()
                                    .fill(theme::BLUE_SOFT)
                                    .corner_radius(egui::CornerRadius::same(7))
                                    .inner_margin(egui::Margin::symmetric(6, 2))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new("BETA")
                                                .size(9.5)
                                                .strong()
                                                .color(theme::BLUE),
                                        );
                                    });
                                ui.add_space(3.0);
                                ui.label(
                                    egui::RichText::new(format!("版本 {}", vm.version))
                                        .size(10.5)
                                        .color(theme::INK_4),
                                );
                            });
                        });
                    });
                });
        });
}

fn nav(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    label: &str,
    page: Page,
    icon: IconName,
    actions: &mut Vec<FrontendAction>,
) {
    nav_with_icon(ui, vm, label, page, icon, actions);
}

fn nav_with_icon(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    label: &str,
    page: Page,
    icon: IconName,
    actions: &mut Vec<FrontendAction>,
) {
    let active = vm.active_page == page;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 32.0), egui::Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
    }
    let color = if active { theme::INK } else { theme::INK_3 };
    icons::draw_icon(ui, rect.min + egui::vec2(20.0, 16.0), icon, color);
    ui.painter().text(
        rect.min + egui::vec2(38.0, 16.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );
    if response.clicked() {
        actions.push(FrontendAction::Navigate(page));
        if page == Page::Settings {
            actions.push(FrontendAction::ToggleSettings);
        }
    }
}

fn subnav(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    label: &str,
    page: Page,
    actions: &mut Vec<FrontendAction>,
) {
    let active = vm.active_page == page;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 30.0), egui::Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
    }
    ui.painter().text(
        rect.min + egui::vec2(30.0, 15.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        if active { theme::INK } else { theme::INK_3 },
    );
    if response.clicked() {
        actions.push(FrontendAction::Navigate(page));
    }
}

fn group(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    label: &str,
    icon: IconName,
    actions: &mut Vec<FrontendAction>,
) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 32.0), egui::Sense::click());
    let color = if response.hovered() {
        theme::INK_2
    } else {
        theme::INK_3
    };
    icons::draw_icon(ui, rect.min + egui::vec2(20.0, 16.0), icon, color);
    ui.painter().text(
        rect.min + egui::vec2(38.0, 16.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );
    let x = rect.max.x - 18.0;
    let y = rect.center().y;
    let is_open = match icon {
        IconName::Style => vm.style_open,
        _ => vm.tools_open,
    };
    if is_open {
        ui.painter().line_segment(
            [egui::pos2(x - 3.0, y - 1.0), egui::pos2(x, y + 2.0)],
            egui::Stroke::new(1.2, color),
        );
        ui.painter().line_segment(
            [egui::pos2(x, y + 2.0), egui::pos2(x + 3.0, y - 1.0)],
            egui::Stroke::new(1.2, color),
        );
    } else {
        ui.painter().line_segment(
            [egui::pos2(x - 1.0, y - 3.0), egui::pos2(x + 2.0, y)],
            egui::Stroke::new(1.2, color),
        );
        ui.painter().line_segment(
            [egui::pos2(x + 2.0, y), egui::pos2(x - 1.0, y + 3.0)],
            egui::Stroke::new(1.2, color),
        );
    }
    if response.clicked() {
        match icon {
            IconName::Style => actions.push(FrontendAction::SidebarToggleStyle),
            _ => actions.push(FrontendAction::SidebarToggleTools),
        }
    }
}

// ── Content panel ───────────────────────────────────────────────────────────

pub fn content_panel(ctx: &egui::Context, add_contents: impl FnOnce(&mut egui::Ui)) {
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
            ui.set_max_size(content.size());
            ui.set_clip_rect(content);
            let scroll = &mut ui.style_mut().spacing.scroll;
            scroll.floating = true;
            scroll.bar_width = 8.0;
            scroll.handle_min_length = 24.0;
            scroll.bar_inner_margin = 0.0;
            scroll.bar_outer_margin = 0.0;
            scroll.foreground_color = false;
            scroll.floating_width = 6.0;
            scroll.floating_allocated_width = 0.0;
            let visuals = &mut ui.style_mut().visuals.widgets;
            visuals.inactive.corner_radius = egui::CornerRadius::same(6);
            visuals.hovered.corner_radius = egui::CornerRadius::same(6);
            visuals.active.corner_radius = egui::CornerRadius::same(6);
            add_contents(ui);
        });
}

// ── Shared helpers ──────────────────────────────────────────────────────────

pub fn icon_text_button(
    ui: &mut egui::Ui,
    label: &str,
    icon: IconName,
    width: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::click());
    let hovered = response.hovered();
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(8),
        if hovered {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(0.8, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let icon_center = egui::pos2(rect.left() + 16.0, rect.center().y);
    icons::draw_icon(ui, icon_center, icon, theme::INK_3);
    ui.painter().text(
        egui::pos2(rect.left() + 28.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.5),
        theme::INK_2,
    );
    response
}

pub fn text_chevron_button(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(8),
        if response.hovered() {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(0.8, theme::LINE),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        theme::INK_2,
    );
    icons::draw_icon(
        ui,
        egui::pos2(rect.right() - 14.0, rect.center().y),
        IconName::ChevronDown,
        theme::INK_3,
    );
    response
}

pub fn small_pill(
    ui: &mut egui::Ui,
    text: &str,
    fill: egui::Color32,
    border: egui::Color32,
    color: egui::Color32,
) -> egui::Response {
    let width = (text.chars().count() as f32 * 10.0 + 16.0).max(42.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(9), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(9),
        egui::Stroke::new(0.7, border),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(10.5),
        color,
    );
    response
}

pub fn card_at(ui: &mut egui::Ui, rect: egui::Rect, contents: impl FnOnce(&mut egui::Ui)) {
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(14), theme::SURFACE);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(17.0);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            contents(ui);
        },
    );
}

pub fn tag(ui: &egui::Ui, pos: egui::Pos2, text: &str, blue: bool) {
    let width = (text.chars().count() as f32 * 10.0 + 16.0).max(48.0);
    let rect = egui::Rect::from_min_size(pos, egui::vec2(width, 20.0));
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(9),
        if blue {
            theme::BLUE_SOFT
        } else {
            theme::SURFACE_2
        },
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(10.0),
        if blue { theme::BLUE } else { theme::INK_3 },
    );
}

pub fn soft_separator(ui: &mut egui::Ui) {
    let rect = ui
        .allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover())
        .0;
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(0.5, egui::Color32::from_rgb(242, 242, 244)),
    );
}

pub fn unsupported_page(ui: &mut egui::Ui, title: &str) {
    ui.add_space(28.0);
    ui.label(
        egui::RichText::new(title)
            .size(28.0)
            .strong()
            .color(theme::INK),
    );
    ui.add_space(22.0);
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(28))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("此页面暂未接线")
                        .size(13.0)
                        .color(theme::INK_3),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("数据桥接将在后续阶段完成")
                        .size(11.0)
                        .color(theme::INK_4),
                );
            });
        });
}
