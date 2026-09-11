use std::path::PathBuf;

use eframe::egui;
pub use openless_linux_egui::ui_catalog::{key as text_key, source as text};

use openless_linux_egui::design_tokens::{self, ThemeTokens};
use std::cell::Cell;

thread_local! { static TOKENS: Cell<&'static ThemeTokens> = const { Cell::new(&design_tokens::LIGHT) }; }
fn color(value: u32) -> egui::Color32 {
    egui::Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}
macro_rules! token {
    ($($name:ident),* $(,)?) => { $(pub fn $name() -> egui::Color32 { TOKENS.with(|t| color(t.get().$name)) })* };
}
token!(
    blue,
    blue_soft,
    canvas,
    surface,
    surface_2,
    line,
    ink,
    ink_2,
    ink_3,
    ink_4,
    ok,
    sidebar_bg,
    settings_rail_bg,
    settings_content_bg
);

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
    let scale = openless_linux_egui::load_ui_value("fontScale")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0)
        .clamp(0.85, 1.35);
    ctx.set_zoom_factor(scale as f32);
}

pub fn apply_visuals(ctx: &egui::Context, mode: openless_core::shared_types::ThemeMode) {
    let dark = match mode {
        openless_core::shared_types::ThemeMode::System => {
            ctx.system_theme() == Some(egui::Theme::Dark)
        }
        openless_core::shared_types::ThemeMode::Light => false,
        openless_core::shared_types::ThemeMode::Dark => true,
    };
    TOKENS.with(|t| t.set(design_tokens::tokens(dark)));
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = canvas();
    visuals.window_fill = surface();
    visuals.faint_bg_color = surface_2();
    visuals.override_text_color = Some(ink());
    visuals.selection.bg_fill = blue_soft();
    visuals.selection.stroke = egui::Stroke::new(1.0, blue());
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    ctx.set_visuals(visuals);
}
