use std::path::PathBuf;

use eframe::egui;

pub const BLUE: egui::Color32 = egui::Color32::from_rgb(37, 99, 235);
pub const BLUE_SOFT: egui::Color32 = egui::Color32::from_rgb(239, 245, 255);
pub const CANVAS: egui::Color32 = egui::Color32::from_rgb(250, 250, 250);
pub const SURFACE: egui::Color32 = egui::Color32::WHITE;
pub const SURFACE_2: egui::Color32 = egui::Color32::from_rgb(244, 244, 245);
pub const LINE: egui::Color32 = egui::Color32::from_rgb(228, 228, 231);
pub const INK: egui::Color32 = egui::Color32::from_rgb(9, 9, 11);
pub const INK_2: egui::Color32 = egui::Color32::from_rgb(63, 63, 70);
pub const INK_3: egui::Color32 = egui::Color32::from_rgb(113, 113, 122);
pub const INK_4: egui::Color32 = egui::Color32::from_rgb(161, 161, 170);

/// Install the same Linux font fallback strategy as the redesigned prototype,
/// without shipping its large duplicate font bundle.
pub fn install(ctx: &egui::Context) {
    let mut candidates: Vec<(&str, PathBuf)> = Vec::new();
    if let Some(path) = std::env::var_os("OPENLESS_IME_FONT").map(PathBuf::from) {
        candidates.push(("openless-primary", path));
    }
    candidates.extend([
        (
            "openless-cjk",
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        ),
        (
            "openless-cjk-fallback",
            PathBuf::from("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf"),
        ),
        (
            "openless-latin",
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf"),
        ),
        (
            "openless-arabic",
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf"),
        ),
        (
            "openless-thai",
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"),
        ),
        (
            "openless-devanagari",
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf"),
        ),
    ]);

    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = Vec::new();
    for (name, path) in candidates {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_owned(bytes).into());
        loaded.push(name);
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let family_fonts = fonts.families.entry(family).or_default();
        for name in loaded.iter().rev() {
            family_fonts.insert(0, (*name).into());
        }
    }
    ctx.set_fonts(fonts);

    apply_visuals(ctx, openless_core::shared_types::ThemeMode::System);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

pub fn apply_visuals(ctx: &egui::Context, mode: openless_core::shared_types::ThemeMode) {
    let dark = match mode {
        openless_core::shared_types::ThemeMode::System => {
            ctx.system_theme() == Some(egui::Theme::Dark)
        }
        openless_core::shared_types::ThemeMode::Light => false,
        openless_core::shared_types::ThemeMode::Dark => true,
    };
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    if !dark {
        visuals.panel_fill = CANVAS;
        visuals.window_fill = SURFACE;
        visuals.faint_bg_color = SURFACE_2;
        visuals.selection.bg_fill = BLUE_SOFT;
    }
    visuals.selection.stroke = egui::Stroke::new(1.0, BLUE);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(7);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(7);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(7);
    ctx.set_visuals(visuals);
}
