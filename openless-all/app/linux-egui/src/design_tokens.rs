//! 2.0 设计令牌（tokens.css 的 Rust 对照表）。
//!
//! 本模块是纯数据：不依赖 egui，任何目标都能编译与单测。egui 侧的
//! 换算（Color32 / Style / Visuals）在 `ui` 模块，仅 Linux 编译。
//!
//! 数值必须与 `src/styles/tokens.css` 一一对应；修改任何一侧都要同步
//! 另一侧并通过本模块的对照测试。CSS 十六进制按 `0xRRGGBB` 转录，
//! 省略 alpha 的透明色按实际叠加效果取实色近似并在字段注释里标明。

/// 圆角阶梯（tokens.css `--ol-r-*`）。egui 0.31 的 `CornerRadius` 以 u8 计，
/// 这里直接存 u8；需要 f32 的场合由使用方转换。
pub mod radius {
    /// `--ol-r-sm: 6px`（控件内小组件）
    pub const SM: u8 = 6;
    /// `--ol-control-radius: 8px`（按钮、导航项）
    pub const CONTROL: u8 = 8;
    /// `--ol-r-md: 10px`
    pub const MD: u8 = 10;
    /// `--ol-r-lg` / `--ol-card-radius: 14px`（卡片、设置弹窗）
    pub const CARD: u8 = 14;
    /// `--ol-panel-radius` / `--ol-r-xl: 18px`（浮层面板）
    pub const PANEL: u8 = 18;
    /// `--ol-shell-radius: 32px`（窗口外壳；Linux 由 WM 裁剪，这里供自定义绘制参考）
    pub const SHELL: u8 = 32;
}

/// 侧栏宽度：FloatingShell `SIDEBAR_WIDTH = 226`。
pub const SIDEBAR_WIDTH: f32 = 226.0;
/// 设置弹窗最大宽度：SettingsModal `maxWidth: 960`。
pub const SETTINGS_MAX_WIDTH: f32 = 960.0;
/// 设置弹窗最大高度：SettingsModal `maxHeight: 680`。
pub const SETTINGS_MAX_HEIGHT: f32 = 680.0;

/// 一个主题的全部颜色令牌。字段名与 tokens.css 的 `--ol-*` 变量对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeTokens {
    /// `--ol-canvas`
    pub canvas: u32,
    /// `--ol-surface`（内容底；2.0 为纯白）
    pub surface: u32,
    /// `--ol-surface-2`（次级底 / 导航激活底）
    pub surface_2: u32,
    /// `--ol-line`
    pub line: u32,
    /// `--ol-line-strong`
    pub line_strong: u32,
    /// `--ol-line-soft`
    pub line_soft: u32,
    /// `--ol-ink`（主文字）
    pub ink: u32,
    /// `--ol-ink-2`
    pub ink_2: u32,
    /// `--ol-ink-3`（次级文字 / 导航未选中）
    pub ink_3: u32,
    /// `--ol-ink-4`（占位 / 弱化文字）
    pub ink_4: u32,
    /// `--ol-ink-5`
    pub ink_5: u32,
    /// `--ol-blue`（强调色）
    pub blue: u32,
    /// `--ol-blue-hover`
    pub blue_hover: u32,
    /// `--ol-blue-soft`（蓝色软底；暗色主题是 alpha 色，这里取实色近似）
    pub blue_soft: u32,
    /// `--ol-on-accent`
    pub on_accent: u32,
    /// `--ol-ok`
    pub ok: u32,
    /// `--ol-ok-soft`（暗色主题为 alpha 色，取实色近似）
    pub ok_soft: u32,
    /// `--ol-warn`
    pub warn: u32,
    /// `--ol-warn-soft`（暗色主题为 alpha 色，取实色近似）
    pub warn_soft: u32,
    /// `--ol-err`
    pub err: u32,
    /// `--ol-sidebar-bg`
    pub sidebar_bg: u32,
    /// `--ol-settings-rail-bg`
    pub settings_rail_bg: u32,
    /// `--ol-settings-content-bg`
    pub settings_content_bg: u32,
    /// `--ol-pill-selected-bg`（导航 / 分段选中深色胶囊；暗色主题为蓝色渐变，取上端色）
    pub pill_selected_bg: u32,
    /// `--ol-pill-selected-ink`
    pub pill_selected_ink: u32,
    /// `--ol-segmented-bg`（alpha 色，取实色近似）
    pub segmented_bg: u32,
    /// `--ol-segmented-active-bg`
    pub segmented_active_bg: u32,
    /// `--ol-overlay-bg`（设置弹窗遮罩；alpha 在 ui 层叠加，这里存 RGB）
    pub overlay_rgb: u32,
}

/// 浅色主题（tokens.css `:root`，2.0 默认）。
pub const LIGHT: ThemeTokens = ThemeTokens {
    canvas: 0xFFFFFF,
    surface: 0xFFFFFF,
    surface_2: 0xF4F4F5,
    line: 0xE4E4E7,
    line_strong: 0xD4D4D8,
    line_soft: 0xF4F4F5,
    ink: 0x09090B,
    ink_2: 0x3F3F46,
    ink_3: 0x71717A,
    ink_4: 0xA1A1AA,
    ink_5: 0xD4D4D8,
    blue: 0x2563EB,
    blue_hover: 0x1D4ED8,
    blue_soft: 0xEFF4FF,
    on_accent: 0xFFFFFF,
    ok: 0x16A34A,
    ok_soft: 0xECFDF5,
    warn: 0xD97706,
    warn_soft: 0xFFF7ED,
    err: 0xDC2626,
    sidebar_bg: 0xF0F0F1,
    settings_rail_bg: 0xF0F0F1,
    settings_content_bg: 0xF7F7F8,
    pill_selected_bg: 0x18181B,
    pill_selected_ink: 0xFFFFFF,
    segmented_bg: 0xF4F4F5,
    segmented_active_bg: 0xFFFFFF,
    overlay_rgb: 0x0F1116,
};

/// 深色主题（tokens.css `[data-ol-theme='dark']`）。
pub const DARK: ThemeTokens = ThemeTokens {
    canvas: 0x0C0C0E,
    surface: 0x1C1C1F,
    surface_2: 0x2A2A2E,
    line: 0x27272A,
    line_strong: 0x3F3F46,
    line_soft: 0x1F1F23,
    ink: 0xFAFAFA,
    ink_2: 0xD4D4D8,
    ink_3: 0xA1A1AA,
    ink_4: 0x71717A,
    ink_5: 0x3F3F46,
    blue: 0x74B7FF,
    blue_hover: 0x93C5FD,
    // rgba(116,183,255,0.16) 叠加 surface 的实色近似。
    blue_soft: 0x2A3A4E,
    on_accent: 0xF8FBFF,
    ok: 0x4ADE80,
    // rgba(74,222,128,0.14) 叠加 surface 的实色近似。
    ok_soft: 0x223529,
    warn: 0xF59E0B,
    // rgba(245,158,11,0.14) 叠加 surface 的实色近似。
    warn_soft: 0x3A2E1C,
    err: 0xF87171,
    sidebar_bg: 0x141417,
    settings_rail_bg: 0x141417,
    settings_content_bg: 0x18181B,
    // 暗色选中是蓝色渐变 3b82f6→2563eb，取上端。
    pill_selected_bg: 0x3B82F6,
    pill_selected_ink: 0xF4F7FB,
    // rgba(226,232,240,0.07) 叠加 surface 的实色近似。
    segmented_bg: 0x26262A,
    segmented_active_bg: 0x27272A,
    overlay_rgb: 0x05080D,
};

/// 按主题取令牌。
pub const fn tokens(dark: bool) -> &'static ThemeTokens {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 关键令牌必须与 tokens.css 字面值一致：这组断言是两个 UI 之间的对照合同。
    #[test]
    fn light_tokens_match_tokens_css() {
        assert_eq!(LIGHT.surface, 0xFFFFFF);
        assert_eq!(LIGHT.surface_2, 0xF4F4F5);
        assert_eq!(LIGHT.line, 0xE4E4E7);
        assert_eq!(LIGHT.ink, 0x09090B);
        assert_eq!(LIGHT.ink_3, 0x71717A);
        assert_eq!(LIGHT.blue, 0x2563EB);
        assert_eq!(LIGHT.blue_hover, 0x1D4ED8);
        assert_eq!(LIGHT.blue_soft, 0xEFF4FF);
        assert_eq!(LIGHT.ok, 0x16A34A);
        assert_eq!(LIGHT.warn, 0xD97706);
        assert_eq!(LIGHT.err, 0xDC2626);
        assert_eq!(LIGHT.sidebar_bg, 0xF0F0F1);
        assert_eq!(LIGHT.settings_content_bg, 0xF7F7F8);
        assert_eq!(LIGHT.pill_selected_bg, 0x18181B);
    }

    #[test]
    fn dark_tokens_match_tokens_css() {
        assert_eq!(DARK.canvas, 0x0C0C0E);
        assert_eq!(DARK.surface, 0x1C1C1F);
        assert_eq!(DARK.line, 0x27272A);
        assert_eq!(DARK.ink, 0xFAFAFA);
        assert_eq!(DARK.blue, 0x74B7FF);
        assert_eq!(DARK.ok, 0x4ADE80);
        assert_eq!(DARK.err, 0xF87171);
        assert_eq!(DARK.sidebar_bg, 0x141417);
        // 暗色选中胶囊是蓝色渐变（3b82f6→2563eb），ui 层取上端色。
        assert_eq!(DARK.pill_selected_bg, 0x3B82F6);
    }

    /// 明暗两套令牌都自洽：文字色和底色有足够对比（不与底色撞色）。
    #[test]
    fn themes_keep_ink_distinct_from_surfaces() {
        for theme in [&LIGHT, &DARK] {
            assert_ne!(theme.ink, theme.surface);
            assert_ne!(theme.ink, theme.surface_2);
            assert_ne!(theme.line, theme.surface);
            assert_ne!(theme.blue, theme.surface);
        }
    }

    #[test]
    fn radii_match_tokens_css_scale() {
        assert_eq!(radius::SM, 6);
        assert_eq!(radius::CONTROL, 8);
        assert_eq!(radius::CARD, 14);
        assert_eq!(radius::PANEL, 18);
        assert_eq!(radius::SHELL, 32);
        assert_eq!(SIDEBAR_WIDTH, 226.0);
    }
}
