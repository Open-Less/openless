use std::path::PathBuf;

use eframe::egui;

pub const BLUE: egui::Color32 = egui::Color32::from_rgb(37, 99, 235);
pub const BLUE_SOFT: egui::Color32 = egui::Color32::from_rgb(239, 245, 255);
pub const CANVAS: egui::Color32 = egui::Color32::from_rgb(250, 250, 250);
pub const SURFACE: egui::Color32 = egui::Color32::WHITE;
pub const SURFACE_2: egui::Color32 = egui::Color32::from_rgb(244, 244, 245);
/// Tauri `--ol-sidebar-bg`：左侧导航栏底色。比内容区（纯白）更灰，两者不能搞反——
/// 侧栏灰、内容白是 Tauri 的层次关系（`ol-sidebar-surface` / `ol-console-main`）。
pub const SIDEBAR: egui::Color32 = egui::Color32::from_rgb(240, 240, 241);
/// 自绘标题栏底色。Tauri 的 Linux 标题栏用 `--ol-linux-titlebar-bg`
/// （`rgba(250,250,250,0.92)`，浮在不透明桌面之上才显灰）；egui 的窗口是不透明的，
/// 所以直接取等价的浅灰，而不是纯白。
pub const TITLEBAR: egui::Color32 = SURFACE_2;
/// Tauri `--ol-pill-blue-border`：蓝色徽章的描边（BETA 标签）。
pub const BLUE_PILL_BORDER: egui::Color32 = egui::Color32::from_rgba_premultiplied(23, 40, 60, 61);
/// Tauri `--ol-segmented-bg`: the segmented-control track.
pub const SEGMENTED_TRACK: egui::Color32 = egui::Color32::from_rgba_premultiplied(10, 10, 10, 10);
/// Tauri `--ol-segmented-active-bg`: the selected chip is a plain white surface.
pub const SEGMENTED_ACTIVE_BG: egui::Color32 = SURFACE;
/// Tauri `--ol-segmented-active-shadow` 第二段 `0 0 0 0.5px rgba(0,0,0,0.06)`：
/// 选中片以细环代替描边（Tauri 的选中片 `border: 0`）。
pub const SEGMENTED_ACTIVE_RING: egui::Color32 = egui::Color32::from_black_alpha(15);
/// Tauri `--ol-segmented-active-shadow` 第一段 `0 1px 2px rgba(0,0,0,0.06)`。
/// egui 没有高斯模糊的矩形阴影，用向下偏移 1px 的淡色圆角矩形近似同一种"浮起"观感。
pub const SEGMENTED_ACTIVE_SHADOW: egui::Color32 = egui::Color32::from_black_alpha(10);
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
/// Tauri `--ol-style-card-icon-bg`: 风格包图标按钮的灰底。
pub const STYLE_CARD_ICON_BG: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(141, 141, 143, 158);
/// Tauri `--ol-style-card-icon-bg-active`: 悬停时的灰底。
pub const STYLE_CARD_ICON_BG_ACTIVE: egui::Color32 = egui::Color32::from_rgb(212, 212, 216);
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
/// Tauri `--ol-capsule-badge-bg` / `--ol-capsule-badge-border`（浅色）。
pub const CAPSULE_BADGE_BG: egui::Color32 = egui::Color32::from_rgb(250, 250, 250);
pub const CAPSULE_BADGE_BORDER: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(9, 24, 58, 64);
pub const ERR: egui::Color32 = egui::Color32::from_rgb(220, 38, 38);

/// Font key of the registered Medium face (see [`medium_font`]).
const MEDIUM_FACE: &str = "openless-medium";
/// Named font family holding the Medium face plus the regular chain as fallback.
pub const MEDIUM_FAMILY: &str = "openless-medium-family";

static MEDIUM_AVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Font for Tauri's `font-weight: 500` labels (setting rows).
///
/// Uses the fontconfig-resolved Medium face when the desktop ships one; otherwise
/// falls back to the regular proportional face. The fallback is deliberate: egui
/// cannot synthesise a weight, and faking it would misreport the alignment.
pub fn medium_font(size: f32) -> egui::FontId {
    if MEDIUM_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed) {
        egui::FontId::new(size, egui::FontFamily::Name(MEDIUM_FAMILY.into()))
    } else {
        egui::FontId::proportional(size)
    }
}

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
    enable_antialiasing(ctx);
    // (font key, file, face index, proportional?, monospace?)
    let mut candidates: Vec<(String, PathBuf, u32, bool, bool)> = Vec::new();
    if let Some(path) = std::env::var_os("OPENLESS_IME_FONT").map(PathBuf::from) {
        candidates.push(("openless-primary".to_owned(), path, 0, true, true));
    }
    // 拉丁字体排在 CJK 前面：`fc-match sans-serif` 在中文桌面（例如 Noto Sans CJK SC）
    // 返回的是 CJK 面，而 CJK 面的 `…`（U+2026）是**居中**的点（全角省略号），
    // 于是「搜索转写内容…」的省略号看着像「···」；macOS 的 `system-ui` 是拉丁优先、
    // CJK 回退，所以我们要同样的顺序。缺字形时照样回退到下面的 CJK 面。
    if let Some((path, index)) = fontconfig_match("sans-serif:lang=en") {
        candidates.push(("openless-ui-latin".to_owned(), path, index, true, false));
    }
    if let Some((path, index)) = fontconfig_match("sans-serif") {
        candidates.push(("openless-ui-sans".to_owned(), path, index, true, false));
    }
    if let Some((path, index)) = fontconfig_match("monospace") {
        candidates.push(("openless-ui-mono".to_owned(), path, index, false, true));
    }
    for (name, query) in [
        ("openless-cjk", ":lang=zh-cn"),
        // 本机 fc-match ':lang=ar/th/hi' 错误地总返回 Noto Sans CJK SC，
        // 这些脚本的字形根本不存在；显式请求对应的 Noto 家族。
        ("openless-arabic", "Noto Sans Arabic"),
        ("openless-thai", "Noto Looped Thai"),
        ("openless-devanagari", "Noto Sans Devanagari UI"),
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

    // Tauri 的 `SettingRow` 标签是 `font-weight: 500`。egui 不能选可变字体的字重轴，
    // 所以去 fontconfig 要一个真正的 Medium 面（例如 Noto Sans CJK Medium）单独注册
    // 成一个命名族；桌面没有 500 面时退回 Proportional（见 `medium_font`）。
    let regular_face = fontconfig_match("sans-serif");
    let medium_face = fontconfig_match("sans-serif:weight=medium");
    if let Some((path, index)) = medium_face {
        if regular_face.as_ref() != Some(&(path.clone(), index)) {
            if let Ok(bytes) = std::fs::read(&path) {
                fonts.font_data.insert(
                    MEDIUM_FACE.to_owned(),
                    egui::FontData {
                        font: bytes.into(),
                        index,
                        tweak: Default::default(),
                    }
                    .into(),
                );
                let mut chain = vec![MEDIUM_FACE.to_owned()];
                chain.extend(
                    fonts
                        .families
                        .get(&egui::FontFamily::Proportional)
                        .cloned()
                        .unwrap_or_default(),
                );
                fonts
                    .families
                    .insert(egui::FontFamily::Name(MEDIUM_FAMILY.into()), chain);
                MEDIUM_AVAILABLE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    ctx.set_fonts(fonts);

    apply_visuals(ctx, openless_core::shared_types::ThemeMode::System);

    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
        });
    }
}

/// Feather shape edges in physical pixels on both the WGPU windows (which also
/// use 4x MSAA) and the single-sample EGL layer surface.
pub fn enable_antialiasing(ctx: &egui::Context) {
    ctx.tessellation_options_mut(|options| {
        options.feathering = true;
        options.feathering_size_in_pixels = 1.0;
    });
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
    // egui derives a widget's padding from
    // `button_padding + visuals.expansion - visuals.bg_stroke.width`, and its
    // stock styles have `inactive.bg_stroke.width = 0.0` against
    // `hovered.bg_stroke.width = 1.0`. A plain `egui::Button` therefore shrinks
    // by 2×2px the moment the pointer enters it, and every sibling in the row
    // reflows with it — the "buttons wobble while I hover" report. Pin the
    // stroke width and expansion across all interactive states so a control's
    // *size* no longer depends on the pointer, while the fills still light the
    // hovered/pressed state up.
    let hairline = egui::Stroke::new(0.5, if dark { LINE } else { LINE_STRONG });
    for state in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        state.bg_stroke = hairline;
        state.expansion = 0.0;
    }
    ctx.set_visuals(visuals);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antialiasing_is_enabled_even_if_tessellation_was_disabled() {
        let ctx = egui::Context::default();
        ctx.tessellation_options_mut(|options| {
            options.feathering = false;
            options.feathering_size_in_pixels = 0.0;
        });
        enable_antialiasing(&ctx);
        ctx.tessellation_options(|options| {
            assert!(options.feathering);
            assert_eq!(options.feathering_size_in_pixels, 1.0);
        });
    }
}
