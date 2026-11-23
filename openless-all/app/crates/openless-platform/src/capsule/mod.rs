//! 胶囊浮层的窗口控制能力。
//!
//! 拆成两层：
//! - [`geometry`]：显示器矩形、放置、镜像选择、命中区域计算 —— 纯逻辑，与框架无关；
//! - 平台后端（`windows/capsule_window.rs`、`macos/capsule_window.rs`）：
//!   以原生窗口句柄为参数做"不抢焦点 / 点击穿透 / 命中测试 / 圆角区域"。
//!
//! 宿主保留窗口对象本身（tauri `WebviewWindow`、Android overlay Service），
//! 只把句柄传进来。这样平台层不依赖任何窗口框架。

pub mod geometry;

pub use geometry::{
    capsule_placement, clamp_to_monitor, display_targets, mirror_targets, monitor_at_point,
    monitor_identity, MonitorRect, MAX_CAPSULE_MIRRORS,
};
