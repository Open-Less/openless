//! macOS 权限引导窗口。
//!
//! 用户在系统设置里授权辅助功能时，弹一个小气泡指向"系统设置"窗口，
//! 告诉用户该勾选哪一项。AppKit 部分在 `guide.m`（由平台层 `build.rs` 编译）。

extern "C" {
    /// 显示引导窗口。AppKit 调用会自行调度到主线程。
    fn openless_show_accessibility_guide();
}

/// 显示辅助功能授权引导。
///
/// 只做提示：授权结果仍由系统权限查询决定，这个窗口不改变任何权限状态。
pub fn show_accessibility_guide() {
    // SAFETY: C 侧只创建/复用窗口，不持有 Rust 侧的引用。
    unsafe { openless_show_accessibility_guide() }
}
