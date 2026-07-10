//! Mobile stub — global hotkeys are unavailable on Android/iOS.

use std::sync::mpsc::Sender;

use crate::types::{
    HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError, HotkeyTrigger,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
    Cancelled,
    TranslationModifierPressed,
    QaShortcutPressed,
}

pub struct HotkeyMonitor;

impl HotkeyMonitor {
    pub fn start(
        _binding: HotkeyBinding,
        _tx: Sender<HotkeyEvent>,
    ) -> Result<Self, HotkeyInstallError> {
        Err(HotkeyInstallError {
            code: "unavailable".into(),
            message: "Global hotkeys are not available on mobile".into(),
        })
    }

    pub fn update_binding(&self, _binding: HotkeyBinding) {}

    pub fn update_modifier_shortcuts(
        &self,
        _qa_trigger: Option<HotkeyTrigger>,
        _translation_trigger: Option<HotkeyTrigger>,
    ) {
    }

    pub fn kind(&self) -> HotkeyAdapterKind {
        HotkeyAdapterKind::Unavailable
    }

    pub fn reset_held_state(&self) {}

    pub fn capability() -> HotkeyCapability {
        HotkeyCapability::current()
    }
}
