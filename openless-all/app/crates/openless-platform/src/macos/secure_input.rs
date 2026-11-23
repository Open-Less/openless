//! macOS Secure Event Input 状态。
//!
//! 打开时（密码框、sudo 提示、部分终端/密码管理器）系统会拦截按键注入与事件读取，
//! 此时文本通道必须直接拒绝，而不是发一串注定失败的 AX/按键调用。

extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

/// Secure Event Input 是否开启。
pub fn is_secure_input_enabled() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn query_does_not_panic() {
        // 仅验证调用路径可用；具体值取决于当前系统状态。
        let _ = super::is_secure_input_enabled();
    }
}
