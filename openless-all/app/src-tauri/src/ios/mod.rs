//! iOS platform integration (clipboard, keyboard extension status, shared config).
//!
//! 结构对齐 `android/`：Rust 运行时模块挂在这里，iOS 专属 Swift 模板放在
//! 仓库 `ios/`（由 scripts/copy-ios-scaffolding.mjs 复制到 src-tauri/gen/apple）。
//! 麦克风权限不走这里——它经 permissions.rs 的 iOS platform 模块进入现有
//! check/request 命令链，与桌面端共用 IPC 面。

/// 键盘扩展共享配置的数据类型。纯数据结构跨平台编译（命令签名在
/// 非 iOS 上也要成立），文件读写等平台行为在 app_group 模块内 cfg 门控。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct KeyboardConfig {
    /// OpenAI 兼容转写端点基础地址（不含 /audio/transcriptions）。
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    /// 转写 prompt（可选，用于引导专有名词）。
    #[serde(default)]
    pub prompt: String,
}

impl KeyboardConfig {
    pub fn is_configured(&self) -> bool {
        !self.endpoint.is_empty() && !self.api_key.is_empty() && !self.model.is_empty()
    }
}

#[cfg(target_os = "ios")]
pub mod app_group;
#[cfg(target_os = "ios")]
pub mod clipboard;
#[cfg(target_os = "ios")]
pub mod keyboard_status;

#[cfg(target_os = "ios")]
pub use app_group::{app_group_available, load_keyboard_config, store_keyboard_config};
/// 键盘扩展启用状态（纯数据，跨平台编译以保持命令签名稳定）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct KeyboardExtensionStatus {
    /// 键盘扩展是否已在系统设置中启用（设置 → 通用 → 键盘 → 键盘）。
    pub enabled: bool,
    /// 找到的 OpenLess 输入模式 identifier（诊断用）。
    pub identifier: Option<String>,
}
