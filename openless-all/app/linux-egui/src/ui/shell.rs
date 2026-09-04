use eframe::egui;

use super::theme;

pub const SIDEBAR_WIDTH: f32 = 188.0;
pub const TITLEBAR_HEIGHT: f32 = 38.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Overview,
    History,
    Providers,
    Models,
    Assistant,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::History => "历史",
            Self::Providers => "Provider 与设置",
            Self::Models => "本地模型",
            Self::Assistant => "Less Computer",
        }
    }
}

pub fn titlebar(ctx: &egui::Context) {
    egui::TopBottomPanel::top("openless-titlebar")
        .exact_height(TITLEBAR_HEIGHT)
        .frame(
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(14, 0)),
        )
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            let drag = ui.interact(
                rect,
                ui.id().with("window-drag"),
                egui::Sense::click_and_drag(),
            );
            if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new("●").color(theme::BLUE).size(13.0));
                ui.label(
                    egui::RichText::new("OpenLess")
                        .color(theme::INK_2)
                        .size(13.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if chrome_button(ui, "×").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if chrome_button(ui, "□").clicked() {
                        let maximized =
                            ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    if chrome_button(ui, "—").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                });
            });
            ui.painter().line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                egui::Stroke::new(1.0, theme::LINE),
            );
        });
}

fn chrome_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized(
        [36.0, 26.0],
        egui::Button::new(egui::RichText::new(text).color(theme::INK_3))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE),
    )
}

pub fn sidebar(ctx: &egui::Context, active: &mut Page, status: &str) {
    egui::SidePanel::left("openless-sidebar")
        .exact_width(SIDEBAR_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(10, 12)),
        )
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("工作台").size(10.5).color(theme::INK_4));
            ui.add_space(5.0);
            nav(ui, active, Page::Overview, "⌂", "概览");
            nav(ui, active, Page::History, "◷", "历史");
            ui.add_space(12.0);
            ui.label(egui::RichText::new("能力").size(10.5).color(theme::INK_4));
            ui.add_space(5.0);
            nav(ui, active, Page::Assistant, "✦", "Less Computer");
            nav(ui, active, Page::Models, "↓", "本地模型");
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                nav(ui, active, Page::Providers, "⚙", "设置");
                ui.separator();
                ui.label(egui::RichText::new(status).size(10.5).color(theme::INK_4));
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("CORE 2.0")
                            .size(9.5)
                            .strong()
                            .color(theme::BLUE),
                    );
                    ui.label(
                        egui::RichText::new(env!("CARGO_PKG_VERSION"))
                            .size(10.0)
                            .color(theme::INK_4),
                    );
                });
            });
        });
}

fn nav(ui: &mut egui::Ui, active: &mut Page, page: Page, icon: &str, label: &str) {
    let selected = *active == page;
    let text = egui::RichText::new(format!("{icon}   {label}"))
        .size(13.0)
        .color(if selected { theme::INK } else { theme::INK_3 });
    let response = ui.add_sized(
        [SIDEBAR_WIDTH - 20.0, 32.0],
        egui::Button::new(text)
            .selected(selected)
            .fill(if selected {
                theme::SURFACE_2
            } else {
                egui::Color32::TRANSPARENT
            })
            .stroke(egui::Stroke::NONE),
    );
    if response.clicked() {
        *active = page;
    }
}

pub fn content_panel(ctx: &egui::Context, active: Page, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::CANVAS)
                .inner_margin(egui::Margin::symmetric(28, 22)),
        )
        .show(ctx, |ui| {
            ui.heading(
                egui::RichText::new(active.title())
                    .size(23.0)
                    .color(theme::INK),
            );
            ui.add_space(12.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, add_contents);
        });
}
