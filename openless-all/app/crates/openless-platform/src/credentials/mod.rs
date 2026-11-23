//! 凭据存储能力。
//!
//! 平台差异在**原语层**：怎么把一段不透明的秘文安全读写进系统保险库。
//! 上层的数据模型（信封编码、同步密钥分片、迁移）不属于平台代码，留在内核与宿主。
//!
//! - macOS：[`keychain`] —— `SecKeychain` 通用密码项，复用已授权的 item 引用，
//!   避免每次写入都弹授权框；
//! - Windows / 其余桌面：系统 `Credential Manager`（由宿主经 `keyring` crate 使用，
//!   本层暂无额外原语）；
//! - Android：Keystore + 信封文件，需要 Android Context，暂留宿主。

#[cfg(target_os = "macos")]
pub mod keychain;

/// 系统凭据保险库的统一读写原语（按平台选后端）。
pub mod store;
