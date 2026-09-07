pub mod icons;
pub mod layout;
pub mod marketplace;
pub mod pages;
pub mod settings;
pub mod view_model;

use eframe::egui;
use view_model::{FrontendAction, FrontendViewModel, Page};

/// Re-export the theme module from the parent ui module.
pub use super::theme;

/// Render the complete egui frontend for one frame. This is the single entry
/// point called from `OpenLessEguiApp::update`. It replaces the old
/// `shell::titlebar` + `shell::sidebar` + `shell::content_panel` calls.
///
/// The frontend is a pure function of `ctx` and `vm` — it reads display state
/// from the view model and pushes user actions into the `actions` vec. The host
/// drains actions after this call and dispatches them to existing Core/backend
/// methods.
pub fn render(ctx: &egui::Context, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    // Paint the rounded window surface and body canvas.
    layout::paint_window_background(ctx);

    // Titlebar with window controls.
    layout::titlebar(ctx, actions);

    // Sidebar with navigation.
    layout::sidebar(ctx, vm, actions);

    // Resize handles for borderless window.
    layout::resize_handles(ctx);

    // Content area.
    layout::content_panel(ctx, |ui| {
        let body = layout::body_rect(ctx);

        // Style page owns its own scroll viewport.
        if vm.active_page == Page::Style {
            let width = (ui.available_width() - 24.0).max(1.0);
            ui.set_min_width(width);
            ui.set_max_width(width);
            ui.add_space(28.0);
            ui.label(
                egui::RichText::new("润色模式")
                    .size(28.0)
                    .strong()
                    .color(theme::INK),
            );
            ui.add_space(22.0);
            pages::style_page(ui, vm, actions);
            ui.add_space(32.0);
            return;
        }

        // History owns two independent scroll regions (list and detail).
        // Do not wrap in another ScrollArea.
        if vm.active_page == Page::History {
            pages::history_page(ui, vm, actions);
            ui.add_space(32.0);
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("openless-main-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = (ui.available_width() - 24.0).max(1.0);
                ui.set_min_width(width);
                ui.set_max_width(width);

                match vm.active_page {
                    Page::Overview => {
                        pages::overview_page(ui, vm, actions);
                    }
                    Page::Vocab => {
                        ui.add_space(28.0);
                        ui.label(
                            egui::RichText::new("词汇表")
                                .size(28.0)
                                .strong()
                                .color(theme::INK),
                        );
                        ui.add_space(22.0);
                        pages::vocab_page(ui, vm, actions);
                    }
                    Page::Marketplace => {
                        ui.add_space(28.0);
                        ui.label(
                            egui::RichText::new("风格市场")
                                .size(28.0)
                                .strong()
                                .color(theme::INK),
                        );
                        ui.add_space(22.0);
                        marketplace::marketplace_page(ui, vm, actions, body);
                    }
                    Page::SelectionAsk => {
                        ui.add_space(28.0);
                        ui.label(
                            egui::RichText::new("划词追问")
                                .size(28.0)
                                .strong()
                                .color(theme::INK),
                        );
                        ui.add_space(22.0);
                        pages::selection_ask_page(ui, vm, actions);
                    }
                    Page::Translation => {
                        ui.add_space(28.0);
                        ui.label(
                            egui::RichText::new("翻译")
                                .size(28.0)
                                .strong()
                                .color(theme::INK),
                        );
                        ui.add_space(22.0);
                        pages::translation_page(ui, vm, actions);
                    }
                    Page::History | Page::Style | Page::Settings => {
                        // Handled above or via overlay.
                    }
                }
                ui.add_space(32.0);
            });

        // Settings overlay (rendered on top of everything).
        if vm.settings_open {
            settings::settings_overlay(ctx, vm, actions, body);
        }
    });
}
