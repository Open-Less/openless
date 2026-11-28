//! Reading typography: font family, scale and content width.
//!
//! The Tauri build writes CSS custom properties (`--ol-font-ui`,
//! `--ol-reading-width`) and a `localStorage` entry. egui has no CSS, so the same
//! three choices are stored in the Linux UI state document and applied to
//! fontconfig resolution, `Context::set_zoom_factor` and the main content width.

use serde::{Deserialize, Serialize};

/// Font choices offered by the appearance page, in the same order as Tauri's
/// `FONT_CATALOG`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFont {
    /// Whatever the desktop resolves for `sans-serif`.
    #[default]
    System,
    /// Humanist sans (Avenir Next / Aptos class).
    Humanist,
    /// CJK-first sans (PingFang SC / 微软雅黑 class).
    Cjk,
    /// Serif (New York / Georgia / 宋体 class).
    Serif,
    /// CJK serif.
    Song,
    /// Monospace (SF Mono / Cascadia class).
    Mono,
    /// Menlo / Consolas class.
    Menlo,
}

impl UiFont {
    pub const ALL: [Self; 7] = [
        Self::System,
        Self::Humanist,
        Self::Cjk,
        Self::Serif,
        Self::Song,
        Self::Mono,
        Self::Menlo,
    ];

    /// State-document tag. Kept stable so an existing file keeps working.
    pub fn tag(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Humanist => "humanist",
            Self::Cjk => "cjk",
            Self::Serif => "serif",
            Self::Song => "song",
            Self::Mono => "mono",
            Self::Menlo => "menlo",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|font| font.tag() == tag)
    }

    /// fontconfig query used to resolve the actual face file.
    pub fn fontconfig_query(self) -> &'static str {
        match self {
            Self::System | Self::Humanist => "sans-serif",
            Self::Cjk => "sans-serif:lang=zh-cn",
            Self::Serif => "serif",
            Self::Song => "serif:lang=zh-cn",
            Self::Mono | Self::Menlo => "monospace",
        }
    }
}

/// 90% / 100% / 110%, matching Tauri's `fontScale` ids.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontScale {
    Small,
    #[default]
    Medium,
    Large,
}

impl FontScale {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn tag(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scale| scale.tag() == tag)
    }

    pub fn factor(self) -> f32 {
        match self {
            Self::Small => 0.9,
            Self::Medium => 1.0,
            Self::Large => 1.1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "90%",
            Self::Medium => "100%",
            Self::Large => "110%",
        }
    }
}

/// Reading width: Tauri maps these onto 56/72/88 `ch`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentWidth {
    Narrow,
    #[default]
    Comfortable,
    Wide,
}

impl ContentWidth {
    pub const ALL: [Self; 3] = [Self::Narrow, Self::Comfortable, Self::Wide];

    pub fn tag(self) -> &'static str {
        match self {
            Self::Narrow => "narrow",
            Self::Comfortable => "comfortable",
            Self::Wide => "wide",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|width| width.tag() == tag)
    }

    /// Approximate Tauri's `ch` budget in logical pixels at the default text size
    /// (1ch ≈ 0.5em for the UI stack).
    pub fn max_width(self) -> f32 {
        match self {
            Self::Narrow => 620.0,
            Self::Comfortable => 800.0,
            Self::Wide => 980.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Typography {
    pub font: UiFont,
    pub scale: FontScale,
    pub width: ContentWidth,
}

/// Which control the appearance page changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypographyField {
    Font(UiFont),
    Scale(FontScale),
    Width(ContentWidth),
}

impl Typography {
    pub fn apply(&mut self, field: TypographyField) {
        match field {
            TypographyField::Font(font) => self.font = font,
            TypographyField::Scale(scale) => self.scale = scale,
            TypographyField::Width(width) => self.width = width,
        }
    }
}

/// Read the persisted typography (Linux UI state document).
pub fn load() -> Typography {
    crate::ui_state::load_typography()
}

/// Persist the typography document.
pub fn save(typography: Typography) -> Result<(), crate::ui_state::UiStateError> {
    crate::ui_state::save_typography(typography)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_round_trip_so_a_saved_state_file_keeps_working() {
        for font in UiFont::ALL {
            assert_eq!(UiFont::from_tag(font.tag()), Some(font));
        }
        for scale in FontScale::ALL {
            assert_eq!(FontScale::from_tag(scale.tag()), Some(scale));
        }
        for width in ContentWidth::ALL {
            assert_eq!(ContentWidth::from_tag(width.tag()), Some(width));
        }
        assert_eq!(UiFont::from_tag("nope"), None);
    }

    #[test]
    fn the_scale_factors_match_tauri_percentages() {
        assert_eq!(FontScale::Small.factor(), 0.9);
        assert_eq!(FontScale::Medium.factor(), 1.0);
        assert_eq!(FontScale::Large.factor(), 1.1);
    }

    #[test]
    fn reading_widths_grow_monotonically() {
        assert!(ContentWidth::Narrow.max_width() < ContentWidth::Comfortable.max_width());
        assert!(ContentWidth::Comfortable.max_width() < ContentWidth::Wide.max_width());
    }

    #[test]
    fn applying_a_field_only_touches_that_field() {
        let mut typography = Typography::default();
        typography.apply(TypographyField::Font(UiFont::Serif));
        assert_eq!(typography.font, UiFont::Serif);
        assert_eq!(typography.scale, FontScale::Medium);
        assert_eq!(typography.width, ContentWidth::Comfortable);
    }
}
