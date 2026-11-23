//! 侧键组合监听（宿主薄封装）。
//!
//! 实现已迁到 `openless-platform::input::side_combo`：物理左右修饰键状态机、
//! 平台分发（macOS keycode / Windows 虚拟键）都在平台层。

pub use openless_platform::input::side_combo::{
    handle_companion_key_down, handle_primary_key, handle_side_modifier, platform,
    ComboHotkeyError, SideAwareComboMonitor, SideModifier,
};
#[cfg(any(target_os = "windows", test))]
pub use openless_platform::input::side_combo::normalize_windows_vk;

/// 绑定校验（宿主保留入口，便于调用点不变）。
pub fn validate_side_binding(
    binding: &openless_core::shared_types::ShortcutBinding,
) -> Result<(), ComboHotkeyError> {
    openless_platform::input::side_combo::validate_side_binding(binding)
}
