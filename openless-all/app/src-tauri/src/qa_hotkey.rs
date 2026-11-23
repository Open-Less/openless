//! QA 全局热键监听（宿主薄封装）。
//!
//! 监听器实现已迁到 `openless-platform::input::qa_hotkey`：解析、注册、共享 global-hotkey
//! 运行时与转发线程都在平台层。宿主只保留对它的引用。

pub use openless_platform::input::qa_hotkey::{QaHotkeyError, QaHotkeyEvent, QaHotkeyMonitor};
