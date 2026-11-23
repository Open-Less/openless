//! 全局热键（宿主薄封装）。
//!
//! 全部实现已迁到 `openless-platform`：macOS CGEventTap、Windows `WH_KEYBOARD_LL`、
//! 共享状态与监听线程脚手架都在平台层。宿主只保留两件事：
//!
//! 1. 把热键实现原样转发出去，调用点无需知道迁移；
//! 2. 注册宿主行为钩子（预取选区、清理录音伴生键），平台层在按键回调里向下调用。

pub use openless_platform::input::hotkey::{
    install_host_hooks, next_press_id, set_capsule_menu_open, set_esc_exclusive, HostHooks,
    HotkeyAdapter, HotkeyAdapterKind, HotkeyCombinedEdge, HotkeyEvent, HotkeyMonitor,
};

/// 注册宿主的两个热键行为钩子。应用启动时调用一次。
pub fn install_dictation_hooks() {
    install_host_hooks(HostHooks {
        prefetch_selection: crate::selection::prefetch_selection_workspace_capture,
        reset_companion_held: crate::recording_companion::reset_held,
        recording_companion_event: crate::recording_companion::event,
        #[cfg(target_os = "macos")]
        secure_input_enabled: || crate::unicode_keystroke::is_secure_input_enabled(),
        #[cfg(not(target_os = "macos"))]
        secure_input_enabled: || false,
    });
}
