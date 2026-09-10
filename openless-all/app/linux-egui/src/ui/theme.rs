//! 把 [`crate::design_tokens`] 的令牌应用到 egui 上下文，并提供组件层使用的
//! [`Palette`] 视图。2.0 的白底 zinc 体系在这里落成 egui 的 Style/Visuals。

use crate::design_tokens::{self as tokens, ThemeTokens};

use eframe::egui;

/// 主题调色板视图：包一层令牌 + 明暗标记，组件层从这里取 `Color32`。
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    tokens: &'static ThemeTokens,
    dark: bool,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        Self {
            tokens: tokens::tokens(dark),
            dark,
        }
    }

    pub fn is_dark(&self) -> bool {
        self.dark
    }

    fn c(value: u32) -> egui::Color32 {
        egui::Color32::from_rgb(
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        )
    }

    /// `--ol-canvas`
    pub fn canvas(&self) -> egui::Color32 {
        Self::c(self.tokens.canvas)
    }
    /// `--ol-surface`：内容区底色（2.0 纯白 / 暗色 #1c1c1f）
    pub fn surface(&self) -> egui::Color32 {
        Self::c(self.tokens.surface)
    }
    /// `--ol-surface-2`：次级底、卡片软底、导航激活底
    pub fn surface_2(&self) -> egui::Color32 {
        Self::c(self.tokens.surface_2)
    }
    /// `--ol-line`
    pub fn line(&self) -> egui::Color32 {
        Self::c(self.tokens.line)
    }
    /// `--ol-line-strong`
    pub fn line_strong(&self) -> egui::Color32 {
        Self::c(self.tokens.line_strong)
    }
    /// `--ol-ink`
    pub fn ink(&self) -> egui::Color32 {
        Self::c(self.tokens.ink)
    }
    /// `--ol-ink-2`
    pub fn ink_2(&self) -> egui::Color32 {
        Self::c(self.tokens.ink_2)
    }
    /// `--ol-ink-3`
    pub fn ink_3(&self) -> egui::Color32 {
        Self::c(self.tokens.ink_3)
    }
    /// `--ol-ink-4`
    pub fn ink_4(&self) -> egui::Color32 {
        Self::c(self.tokens.ink_4)
    }
    /// `--ol-ink-5`（分隔线级弱色，兼作开关关闭轨道）
    pub fn ink_5(&self) -> egui::Color32 {
        Self::c(self.tokens.ink_5)
    }
    /// `--ol-blue`
    pub fn blue(&self) -> egui::Color32 {
        Self::c(self.tokens.blue)
    }
    /// `--ol-blue-hover`
    pub fn blue_hover(&self) -> egui::Color32 {
        Self::c(self.tokens.blue_hover)
    }
    /// `--ol-blue-soft`
    pub fn blue_soft(&self) -> egui::Color32 {
        Self::c(self.tokens.blue_soft)
    }
    /// `--ol-on-accent`
    pub fn on_accent(&self) -> egui::Color32 {
        Self::c(self.tokens.on_accent)
    }
    /// `--ol-ok`
    pub fn ok(&self) -> egui::Color32 {
        Self::c(self.tokens.ok)
    }
    /// `--ol-ok-soft`
    pub fn ok_soft(&self) -> egui::Color32 {
        Self::c(self.tokens.ok_soft)
    }
    /// `--ol-warn`
    pub fn warn(&self) -> egui::Color32 {
        Self::c(self.tokens.warn)
    }
    /// `--ol-warn-soft`
    pub fn warn_soft(&self) -> egui::Color32 {
        Self::c(self.tokens.warn_soft)
    }
    /// `--ol-err`
    pub fn err(&self) -> egui::Color32 {
        Self::c(self.tokens.err)
    }
    /// `--ol-sidebar-bg`
    pub fn sidebar_bg(&self) -> egui::Color32 {
        Self::c(self.tokens.sidebar_bg)
    }
    /// `--ol-settings-rail-bg`
    pub fn settings_rail_bg(&self) -> egui::Color32 {
        Self::c(self.tokens.settings_rail_bg)
    }
    /// `--ol-settings-content-bg`
    pub fn settings_content_bg(&self) -> egui::Color32 {
        Self::c(self.tokens.settings_content_bg)
    }
    /// `--ol-pill-selected-bg`：主按钮（浅色主题的深色实心按钮）
    pub fn primary_bg(&self) -> egui::Color32 {
        Self::c(self.tokens.pill_selected_bg)
    }
    /// `--ol-pill-selected-ink`
    pub fn primary_ink(&self) -> egui::Color32 {
        Self::c(self.tokens.pill_selected_ink)
    }
    /// `--ol-segmented-bg`
    pub fn segmented_bg(&self) -> egui::Color32 {
        Self::c(self.tokens.segmented_bg)
    }
    /// `--ol-segmented-active-bg`
    pub fn segmented_active_bg(&self) -> egui::Color32 {
        Self::c(self.tokens.segmented_active_bg)
    }
    /// `--ol-overlay-bg`（alpha 固定 0.32/0.64，见 tokens.css）
    pub fn overlay(&self) -> egui::Color32 {
        let alpha: f32 = if self.dark { 0.64 } else { 0.32 };
        egui::Color32::from_rgba_premultiplied(
            ((self.tokens.overlay_rgb >> 16) & 0xFF) as u8,
            ((self.tokens.overlay_rgb >> 8) & 0xFF) as u8,
            (self.tokens.overlay_rgb & 0xFF) as u8,
            (alpha * 255.0).round() as u8,
        )
    }
}

/// egui 内存里记录当前明暗主题，让任意 `&mut Ui` 都能取到调色板。
fn store_dark(ctx: &egui::Context, dark: bool) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("openless-ui-dark"), dark));
}

/// 读取当前主题；尚未调用 [`apply_theme`] 时按浅色处理。
pub fn current(ctx: &egui::Context) -> Palette {
    let dark = ctx
        .data_mut(|data| data.get_temp::<bool>(egui::Id::new("openless-ui-dark")))
        .unwrap_or(false);
    Palette::new(dark)
}

/// 应用 2.0 主题到 egui 上下文。`dark=false` 是 2.0 默认的白底体系。
pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    let p = Palette::new(dark);
    store_dark(ctx, dark);

    let mut style = egui::Style::default();
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // 面板与浮层底色：内容区白底（暗色 surface）。
    visuals.panel_fill = p.surface();
    visuals.window_fill = p.surface();
    visuals.window_stroke = egui::Stroke::new(0.5_f32, p.line());
    visuals.faint_bg_color = p.surface_2();
    visuals.extreme_bg_color = if dark { p.canvas() } else { p.surface() };
    visuals.code_bg_color = p.surface_2();
    visuals.hyperlink_color = p.blue();
    // egui 0.31 起 warn/error 文字色字段更名 *_fg_color。
    visuals.warn_fg_color = p.warn();
    visuals.error_fg_color = p.err();
    visuals.selection.bg_fill = p.blue_soft();
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, p.ink());

    let control_rounding = egui::CornerRadius::same(tokens::radius::CONTROL);
    // 控件观感对齐 React：默认按钮 = surface 底 + 0.5px line 边 + ink-2 文字；
    // 悬停 = surface-2；激活（按下）= blue 实底白字；危险文字直接用 err 色。
    visuals.widgets.noninteractive = widget_visuals(
        p.surface_2(),
        p.surface_2(),
        egui::Stroke::new(0.5_f32, p.line()),
        p.ink_3(),
        control_rounding,
    );
    visuals.widgets.inactive = widget_visuals(
        p.surface(),
        p.surface(),
        egui::Stroke::new(1.0_f32, p.line()),
        p.ink_2(),
        control_rounding,
    );
    visuals.widgets.hovered = widget_visuals(
        p.surface_2(),
        p.surface_2(),
        egui::Stroke::new(1.0_f32, p.line_strong()),
        p.ink(),
        control_rounding,
    );
    visuals.widgets.active = widget_visuals(
        p.blue(),
        p.blue(),
        egui::Stroke::new(1.0_f32, p.blue()),
        p.on_accent(),
        control_rounding,
    );
    visuals.widgets.open = widget_visuals(
        p.blue_soft(),
        p.blue_soft(),
        egui::Stroke::new(1.0_f32, p.line()),
        p.ink(),
        control_rounding,
    );

    style.visuals = visuals;

    // 排版：正文 15px（导航/正文），页标题 22px，小字 12.5px，等宽 13px。
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::proportional(22.0)),
        (egui::TextStyle::Body, egui::FontId::proportional(15.0)),
        (egui::TextStyle::Small, egui::FontId::proportional(12.5)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(13.0)),
    ]
    .into_iter()
    .collect();

    // 间距：横向 10px（React gap:10）、按钮内边距 12x6（padding: 6px 12px 的镜像）。
    // egui 0.31 的 Margin 以 i8 计。
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.menu_spacing = 4.0;

    ctx.set_style(style);
}

fn widget_visuals(
    bg_fill: egui::Color32,
    weak_bg_fill: egui::Color32,
    bg_stroke: egui::Stroke,
    ink: egui::Color32,
    corner_radius: egui::CornerRadius,
) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill,
        weak_bg_fill,
        bg_stroke,
        corner_radius,
        fg_stroke: egui::Stroke::new(1.0_f32, ink),
        expansion: 0.0,
    }
}

/// 安装 CJK 字体（egui 默认字体不含中文字形，不装会显示成 □）。
///
/// 移植自 egui 弹窗实验（PR #997 `egui_host/fonts.rs`），修正了原实现
/// 每轮循环都用全新 `FontDefinitions` 覆盖前一轮的问题：这里只构建一次、
/// 最多装两个候选并一次 `set_fonts`。候选优先级：
/// `OPENLESS_IME_FONT` > 常见系统中文字体 > HOME 字体目录扫描。
pub fn install_cjk_fonts(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        // Linux
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        // 开发宿主（macOS/Windows）上跑测试时的回退。
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "C:\\Windows\\Fonts\\msyh.ttc",
    ];

    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(explicit) = std::env::var("OPENLESS_IME_FONT") {
        paths.push(std::path::PathBuf::from(explicit));
    }
    paths.extend(CANDIDATES.iter().map(std::path::PathBuf::from));
    if let Ok(home) = std::env::var("HOME") {
        for sub in [".fonts", ".local/share/fonts"] {
            let dir = std::path::PathBuf::from(&home).join(sub);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if name.contains("cjk") || name.contains("wqy") {
                        paths.push(path);
                    }
                }
            }
        }
    }

    let mut fonts = egui::FontDefinitions::default();
    let mut inserted = 0usize;
    let mut first_source: Option<std::path::PathBuf> = None;
    for path in paths {
        if inserted >= 2 {
            break;
        }
        if !path.exists() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let name = format!("openless-cjk-{inserted}");
        fonts.font_data.insert(
            name.clone(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.insert(0, name.clone());
            }
        }
        if first_source.is_none() {
            first_source = Some(path.clone());
        }
        log::info!("[ui] CJK 字体: {}", path.display());
        inserted += 1;
    }
    if inserted == 0 {
        log::warn!("[ui] 未找到 CJK 字体，中文将显示为 □（OPENLESS_IME_FONT 可指定）");
        return;
    }
    ctx.set_fonts(fonts);
    let _ = first_source;
}
