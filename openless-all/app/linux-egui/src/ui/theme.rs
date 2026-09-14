use std::path::PathBuf;

use eframe::egui;

pub const BLUE: egui::Color32 = egui::Color32::from_rgb(37, 99, 235);
pub const BLUE_SOFT: egui::Color32 = egui::Color32::from_rgb(239, 245, 255);
pub const CANVAS: egui::Color32 = egui::Color32::from_rgb(250, 250, 250);
pub const SURFACE: egui::Color32 = egui::Color32::WHITE;
pub const SURFACE_2: egui::Color32 = egui::Color32::from_rgb(244, 244, 245);
/// Tauri `--ol-segmented-bg`: the segmented-control track.
pub const SEGMENTED_TRACK: egui::Color32 = egui::Color32::from_rgba_premultiplied(10, 10, 10, 10);
pub const LINE: egui::Color32 = egui::Color32::from_rgb(228, 228, 231);
pub const INK: egui::Color32 = egui::Color32::from_rgb(9, 9, 11);
pub const INK_2: egui::Color32 = egui::Color32::from_rgb(63, 63, 70);
pub const INK_3: egui::Color32 = egui::Color32::from_rgb(113, 113, 122);
pub const INK_4: egui::Color32 = egui::Color32::from_rgb(161, 161, 170);
pub const OK: egui::Color32 = egui::Color32::from_rgb(22, 163, 74);
/// Tauri `--ol-line-soft`: 设置行之间的分隔线（比 `--ol-line` 更淡）。
pub const LINE_SOFT: egui::Color32 = egui::Color32::from_rgb(244, 244, 245);
/// Tauri `--ol-line-strong`: 输入框 / 次级按钮的描边。
pub const LINE_STRONG: egui::Color32 = egui::Color32::from_rgb(212, 212, 216);
/// Tauri `--ol-settings-rail-bg`: 设置弹窗左侧导航底色。
pub const RAIL_BG: egui::Color32 = egui::Color32::from_rgb(240, 240, 241);
/// Tauri `--ol-settings-content-bg`: 设置弹窗内容区底色（卡片是白色的）。
pub const CONTENT_BG: egui::Color32 = egui::Color32::from_rgb(247, 247, 248);
/// Tauri `--ol-nav-hover-bg`: 侧栏/导航项悬停底色。
pub const NAV_HOVER: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 10);
/// Tauri `--ol-toggle-off-bg`: 关闭态开关轨道。
pub const TOGGLE_OFF: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 38);
/// Tauri `--ol-overlay-bg`: 设置/市场遮罩。
pub const OVERLAY: egui::Color32 = egui::Color32::from_rgba_premultiplied(5, 5, 7, 82);
/// Tauri `--ol-err` 的淡底：红框提示卡。
pub const DANGER_SOFT: egui::Color32 = egui::Color32::from_rgba_premultiplied(37, 11, 11, 18);
/// Tauri `--ol-warn-soft`: 警告卡底色。
pub const WARN_SOFT: egui::Color32 = egui::Color32::from_rgb(255, 247, 237);
/// Tauri `--ol-warn`: 已配置但非必选的提示色。
pub const WARN: egui::Color32 = egui::Color32::from_rgb(217, 119, 6);
/// Tauri `--ol-err`: used by the denied permission state.
pub const ERR: egui::Color32 = egui::Color32::from_rgb(220, 38, 38);

/// Resolve the file + face index fontconfig would pick for `query`.
///
/// This is the same face the Tauri/WebKit app gets through `system-ui`. It
/// matters for `.ttc` collections: loading one without an index silently picks
/// face 0 (Noto Sans CJK **JP**), which is why Simplified Chinese used to render
/// with Japanese glyph variants.
fn fontconfig_match(query: &str) -> Option<(PathBuf, u32)> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}|%{index}", query])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (file, index) = text.split_once('|')?;
    let file = file.trim();
    if file.is_empty() {
        return None;
    }
    Some((PathBuf::from(file), index.trim().parse().unwrap_or(0)))
}

/// Install the Linux font stack.
///
/// Faces are resolved through fontconfig (so HarmonyOS Sans, Noto or whatever
/// the desktop prefers is used, exactly like the Tauri build) and CJK is loaded
/// with its proper face index. The egui defaults stay as the last fallback.
pub fn install(ctx: &egui::Context) {
    // (font key, file, face index, proportional?, monospace?)
    let mut candidates: Vec<(String, PathBuf, u32, bool, bool)> = Vec::new();
    if let Some(path) = std::env::var_os("OPENLESS_IME_FONT").map(PathBuf::from) {
        candidates.push(("openless-primary".to_owned(), path, 0, true, true));
    }
    if let Some((path, index)) = fontconfig_match("sans-serif") {
        candidates.push(("openless-ui-sans".to_owned(), path, index, true, false));
    }
    if let Some((path, index)) = fontconfig_match("monospace") {
        candidates.push(("openless-ui-mono".to_owned(), path, index, false, true));
    }
    for (name, query) in [
        ("openless-cjk", ":lang=zh-cn"),
        ("openless-arabic", ":lang=ar"),
        ("openless-thai", ":lang=th"),
        ("openless-devanagari", ":lang=hi"),
    ] {
        if let Some((path, index)) = fontconfig_match(query) {
            candidates.push((name.to_owned(), path, index, true, true));
        }
    }
    // Last-resort fallbacks for minimal systems without fontconfig entries.
    for (name, path) in [
        (
            "openless-legacy-cjk",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        ),
        (
            "openless-legacy-latin",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        ),
    ] {
        candidates.push((name.to_owned(), PathBuf::from(path), 0, true, true));
    }

    let mut fonts = egui::FontDefinitions::default();
    let mut proportional: Vec<String> = Vec::new();
    let mut monospace: Vec<String> = Vec::new();
    let mut seen: Vec<(PathBuf, u32)> = Vec::new();
    for (name, path, index, is_proportional, is_monospace) in candidates {
        if seen
            .iter()
            .any(|(seen_path, seen_index)| seen_path == &path && *seen_index == index)
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        seen.push((path, index));
        fonts.font_data.insert(
            name.clone(),
            egui::FontData {
                font: bytes.into(),
                index,
                tweak: Default::default(),
            }
            .into(),
        );
        if is_proportional {
            proportional.push(name.clone());
        }
        if is_monospace {
            monospace.push(name);
        }
    }
    for name in proportional.iter().rev() {
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, name.clone());
    }
    for name in monospace.iter().rev() {
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, name.clone());
    }
    ctx.set_fonts(fonts);

    apply_visuals(ctx, openless_core::shared_types::ThemeMode::System);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

/// Apply the light/dark visual theme.
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
