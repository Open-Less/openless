//! Mobile stub — QA hotkeys are unavailable on Android/iOS.

use std::sync::mpsc::Sender;

use crate::types::ShortcutBinding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QaHotkeyEvent {
    Pressed,
}

#[derive(Debug, thiserror::Error)]
pub enum QaHotkeyError {
    #[error("注册 QA 快捷键失败: {0}")]
    RegisterFailed(String),
}

pub struct QaHotkeyMonitor;

impl QaHotkeyMonitor {
    pub fn start(
        _binding: ShortcutBinding,
        _tx: Sender<QaHotkeyEvent>,
    ) -> Result<Self, QaHotkeyError> {
        Err(QaHotkeyError::RegisterFailed(
            "QA hotkeys are not available on mobile".into(),
        ))
    }

    pub fn update_binding(&self, _binding: ShortcutBinding) -> Result<(), QaHotkeyError> {
        Ok(())
    }
}
