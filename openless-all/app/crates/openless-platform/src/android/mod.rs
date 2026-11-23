//! Android 平台后端。
//!
//! Android 的绝大多数系统能力都要 Java 环境（`JNIEnv` + `Application` context），而持有它们的
//! 只能是宿主。所以这里按**接缝**设计：
//!
//! - 平台层定义 [`AndroidEnv`] 接口与能力实现；
//! - 宿主在启动时用 [`install_env`] 注册一个基于自身 JNI 代码的实现；
//! - 平台层不直接链接 `jni` crate，也不假设宿主用哪种方式（JNI / 无障碍服务 / Shizuku）。
//!
//! 这样 Android 与 Windows / macOS 平级：同一套能力门面，后端换成宿主注入的实现。

use std::sync::OnceLock;

/// 宿主必须提供的 Java 侧能力。
///
/// 每个方法对应一次 Java 调用；失败一律返回 `Err(String)`，由能力层决定降级方式。
pub trait AndroidEnv: Send + Sync {
    /// 检查某个运行时权限是否已授予（`android.permission.RECORD_AUDIO` 等）。
    fn check_permission(&self, permission: &str) -> Result<bool, String>;

    /// 请求某个运行时权限，返回请求后的授予状态。
    fn request_permission(&self, permission: &str) -> Result<bool, String>;

    /// 读剪贴板纯文本；没有文本时返回 `Ok(None)`。
    fn clipboard_read_text(&self) -> Result<Option<String>, String>;

    /// 写剪贴板纯文本。
    fn clipboard_write_text(&self, text: &str) -> Result<(), String>;

    /// 用无障碍服务把文本写入当前焦点输入框；返回是否成功。
    fn insert_via_accessibility(&self, text: &str) -> Result<bool, String>;

    /// 用无障碍服务读取焦点输入框的选中文本。
    fn read_selection_via_accessibility(&self) -> Result<Option<String>, String>;
}

static ENV: OnceLock<Box<dyn AndroidEnv>> = OnceLock::new();

/// 注册 Android 环境。宿主启动时调用一次；重复调用只有第一次生效。
pub fn install_env(env: Box<dyn AndroidEnv>) {
    let _ = ENV.set(env);
}

/// 取已注册的环境。未注册时返回 `None`，调用方应返回明确的"不可用"。
pub(crate) fn env() -> Option<&'static dyn AndroidEnv> {
    ENV.get().map(|env| env.as_ref())
}

#[cfg(target_os = "android")]
pub mod clipboard;
#[cfg(target_os = "android")]
pub mod permissions;
#[cfg(target_os = "android")]
pub mod selection;
#[cfg(target_os = "android")]
pub mod text;

// ── JNI 基础与基于无障碍服务的插入通道 ─────────────────────────────
//
// 这些模块需要 `jni` crate 与宿主的 Java 类名约定；它们仍是"平台实现"，
// 只是比纯 Rust 逻辑多了一层 JNI 依赖。宿主只在启动时注入 context。
pub mod accessibility;
pub mod context;
#[cfg(target_os = "android")]
pub mod input;
pub mod jni;
#[cfg(target_os = "android")]
pub mod overlay_control;
#[cfg(target_os = "android")]
#[allow(dead_code, unused_imports, unused_variables)]
pub mod overlay;
#[cfg(target_os = "android")]
pub mod local_migration;
#[cfg(target_os = "android")]
pub mod insert_tiers;
#[cfg(target_os = "android")]
pub mod edit_observation;
#[cfg(target_os = "android")]
#[allow(dead_code, unused_imports, unused_variables)]
pub mod shizuku;
