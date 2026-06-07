//! Mobile stub — combo hotkeys are unavailable on Android/iOS.

use std::sync::mpsc::Sender;

use crate::types::ShortcutBinding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComboHotkeyEvent {
    Pressed,
    Released,
}

#[derive(Debug, thiserror::Error)]
pub enum ComboHotkeyError {
    #[error("不支持的修饰键: {0}")]
    UnsupportedModifier(String),
    #[error("不支持的主键: {0}")]
    UnsupportedKey(String),
    #[error("注册全局快捷键失败: {0}")]
    RegisterFailed(String),
    #[error("初始化全局快捷键管理器失败: {0}")]
    ManagerInitFailed(String),
}

pub struct ComboHotkeyMonitor;

impl ComboHotkeyMonitor {
    pub fn start(
        _binding: ShortcutBinding,
        _tx: Sender<ComboHotkeyEvent>,
    ) -> Result<Self, ComboHotkeyError> {
        Err(ComboHotkeyError::RegisterFailed(
            "Combo hotkeys are not available on mobile".into(),
        ))
    }

    pub fn update_binding(&self, _binding: ShortcutBinding) -> Result<(), ComboHotkeyError> {
        Ok(())
    }
}

pub fn validate_binding(_binding: &ShortcutBinding) -> Result<(), ComboHotkeyError> {
    Err(ComboHotkeyError::RegisterFailed(
        "Combo hotkeys are not available on mobile".into(),
    ))
}
