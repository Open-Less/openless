//! 宿主文档读取被拒绝的原因。
//!
//! 平台层报告"为什么没读"，宿主把它翻译成用户可见状态与日志。

/// Reason for a hard block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// macOS Secure Event Input is on (password fields, sudo prompts, etc.).
    SecureInput,
    /// Focused control's AXRole/AXSubrole is `AXSecureTextField`.
    SecureTextField,
    /// Foreground app is on the hardcoded blocklist (password managers / keychain / terminals).
    BlockedApp,
}

impl BlockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockReason::SecureInput => "secure_input",
            BlockReason::SecureTextField => "secure_text_field",
            BlockReason::BlockedApp => "blocked_app",
        }
    }
}