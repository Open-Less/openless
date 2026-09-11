//! 2.0 界面的 egui 承接层：主题、组件与外观偏好。
//!
//! 只在 Linux 编译（egui/eframe 是 Linux 目标依赖）；颜色数值来自
//! [`crate::design_tokens`]（tokens.css 的对照表），本模块只做换算与布局。

pub mod prefs;
pub mod theme;
pub mod widgets;

pub use prefs::UiPrefs;
pub use theme::{apply_theme, current, install_cjk_fonts, Palette};
