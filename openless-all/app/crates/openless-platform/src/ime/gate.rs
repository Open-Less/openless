//! 文本通道的安全门。
//!
//! 决定"这次读取能不能做"：Secure Event Input、密码框、密码管理器/终端类应用一律拒绝。
//! 纯字符串判定，没有任何 AX/UIA 调用 —— 拒绝时一次系统调用都不发。
//!
//! 没有给用户配置界面：安全门若可配置，用户会以为"配一次就安全了"，而真实威胁面
//! 一直在变。列表只随版本更新。

use crate::common::block_reason::BlockReason;

/// Inputs to the safety gate. Modeled as a plain data structure so the decision logic is
/// unit-testable without AX — a wrong gate decision means sending passwords to the LLM, so
/// this path must have test coverage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateInputs {
    /// Result of `unicode_keystroke::is_secure_input_enabled()`.
    pub secure_input: bool,
    /// Foreground app bundle id (macOS).
    pub bundle_id: Option<String>,
    /// Focused element's `AXRole`.
    pub role: Option<String>,
    /// Focused element's `AXSubrole`.
    pub subrole: Option<String>,
}

/// AX role/subrole value denoting a secure text field.
const AX_SECURE_TEXT_FIELD: &str = "axsecuretextfield";

/// Sensitive apps that are never read (bundle id prefixes, lowercase comparison).
///
/// No UI for this — a blocklist UI creates the illusion "configure it once and you're safe",
/// while the real defense is read-off-by-default plus this hardcoded list. The list covers only
/// the two categories whose content is almost certainly sensitive:
///
/// - **Password managers / keychain**: the document body is the credential itself.
/// Prefix matching, so `com.1password` covers both `com.1password.1password` and its helper
/// processes.
const SENSITIVE_BUNDLE_PREFIXES: &[&str] = &[
    "com.1password",
    "com.agilebits.onepassword",
    "com.apple.keychainaccess",
    "com.bitwarden",
    "com.lastpass",
    "com.dashlane",
    "org.keepassxc",
    "com.kueh.keepassium",
    "in.sinew.enpass",
    "com.sinew.enpass",
    "com.apple.passwords",
];

/// Terminal app bundle id prefixes. Besides forbidding scrollback reads, used for the macOS
/// auto-wrap-mode decision: known terminals send U+000A, other apps conservatively send
/// Shift+Return.
const TERMINAL_BUNDLE_PREFIXES: &[&str] = &[
    "com.apple.terminal",
    "com.googlecode.iterm2",
    "dev.warp.warp",
    "com.github.wez.wezterm",
    "io.alacritty",
    "org.alacritty",
    "net.kovidgoyal.kitty",
    "co.zeit.hyper",
    "org.tabby",
    "com.tabby",
    "com.mitchellh.ghostty",
];

fn bundle_id_starts_with_any(bundle_id: &str, prefixes: &[&str]) -> bool {
    let lowered = bundle_id.to_ascii_lowercase();
    prefixes.iter().any(|prefix| lowered.starts_with(prefix))
}

pub(crate) fn is_terminal_bundle_id(bundle_id: &str) -> bool {
    bundle_id_starts_with_any(bundle_id, TERMINAL_BUNDLE_PREFIXES)
}

/// Gate decision. Returns `Some(reason)` to block, `None` to allow.
///
/// Checks are ordered by cost: Secure Input and bundle prefixes need no AX and are decided
/// first; role/subrole needs one AX read and is checked last.
pub fn evaluate_gate(inputs: &GateInputs) -> Option<BlockReason> {
    if inputs.secure_input {
        return Some(BlockReason::SecureInput);
    }
    if let Some(bundle) = inputs.bundle_id.as_deref() {
        if bundle_id_starts_with_any(bundle, SENSITIVE_BUNDLE_PREFIXES)
            || is_terminal_bundle_id(bundle)
        {
            return Some(BlockReason::BlockedApp);
        }
    }
    let is_secure_field = |value: &Option<String>| {
        value
            .as_deref()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case(AX_SECURE_TEXT_FIELD))
    };
    if is_secure_field(&inputs.role) || is_secure_field(&inputs.subrole) {
        return Some(BlockReason::SecureTextField);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_input_blocks_everything() {
        let inputs = GateInputs {
            secure_input: true,
            bundle_id: None,
            role: None,
            subrole: None,
        };
        assert_eq!(evaluate_gate(&inputs), Some(BlockReason::SecureInput));
    }

    #[test]
    fn secure_text_field_is_blocked_by_role_or_subrole() {
        let role = GateInputs {
            secure_input: false,
            bundle_id: None,
            role: Some("AXSecureTextField".into()),
            subrole: None,
        };
        assert_eq!(evaluate_gate(&role), Some(BlockReason::SecureTextField));
        let subrole = GateInputs {
            secure_input: false,
            bundle_id: None,
            role: Some("AXTextField".into()),
            subrole: Some("AXSecureTextField".into()),
        };
        assert_eq!(evaluate_gate(&subrole), Some(BlockReason::SecureTextField));
    }

    #[test]
    fn sensitive_and_terminal_apps_are_blocked() {
        for bundle in ["com.1password.1password", "com.apple.Terminal", "com.googlecode.iterm2"] {
            let inputs = GateInputs {
                secure_input: false,
                bundle_id: Some(bundle.into()),
                role: Some("AXTextArea".into()),
                subrole: None,
            };
            assert_eq!(evaluate_gate(&inputs), Some(BlockReason::BlockedApp), "{bundle}");
        }
    }

    #[test]
    fn ordinary_text_areas_pass() {
        let inputs = GateInputs {
            secure_input: false,
            bundle_id: Some("com.apple.TextEdit".into()),
            role: Some("AXTextArea".into()),
            subrole: None,
        };
        assert_eq!(evaluate_gate(&inputs), None);
    }
}
