//! 系统权限探测（宿主薄封装）。
//!
//! 全部平台实现已迁到 `openless-platform::permissions`：macOS TCC、
//! Windows 注册表探测、Android 运行时权限。宿主只转发，调用点无需知道迁移。
//!
//! 注意 `PermissionStatus::NotApplicable` 是合法值（Windows 没有"辅助功能授权"概念），
//! 不要把它当成拒绝。

pub use openless_platform::permissions::{
    check_accessibility, check_microphone, has_microphone_input_device, is_no_device_error,
    request_accessibility, request_microphone, windows_microphone_access_explicitly_denied,
    PermissionStatus,
};
