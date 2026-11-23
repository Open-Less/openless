//! 胶囊窗口命中测试（宿主薄封装）。
//!
//! Win32 子类化、点击穿透、不抢焦点已迁到 `openless-platform::windows::capsule_window`，
//! 集成测试也一并迁到那里（创建真实窗口验证子类链与消息转发）。
//! 宿主只转交 `HWND` 与命中区域。

pub(crate) use openless_platform::windows::capsule_window::{
    install_card_hit_test as install_capsule_card_hit_test_proc,
    install_hit_test as install_capsule_hit_test_proc, set_no_activate as set_capsule_no_activate,
};

/// 命中区域（相对窗口左上角）。
pub(crate) type CapsuleHitTestRect = openless_platform::windows::capsule_window::HitRect;
