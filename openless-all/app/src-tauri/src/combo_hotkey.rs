//! 自定义组合键听写（宿主薄封装）。
//!
//! 实现已迁到 `openless-platform::input::combo`。

pub use openless_platform::input::combo::{
    validate_binding, ComboHotkeyError, ComboHotkeyEvent, ComboHotkeyMonitor,
};
