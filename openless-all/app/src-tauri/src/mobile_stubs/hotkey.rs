//! Mobile stub — global hotkeys are unavailable on Android/iOS.

use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::types::{
    HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError, HotkeyTrigger,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed { at: Instant, press_id: u64 },
    Released { at: Instant },
    // 组合键撤销与 Esc 取消在移动端无全局键盘监听，不在此枚举里（见 hotkey.rs 模块注释）。

    TranslationModifierPressed,
    QaShortcutPressed,
    // SelectionPolishShortcutPressed 为桌面（Windows-first）选区润色专属，mobile stub 不声明。
}

/// Mobile 无全局键盘监听，Esc 独占为 no-op。
pub fn set_esc_exclusive(_active: bool) {}

pub struct HotkeyMonitor;

impl HotkeyMonitor {
    pub fn start(
        _binding: HotkeyBinding,
        _tx: Sender<HotkeyEvent>,
        _cancel_tx: Sender<()>,
        _combo_tx: Sender<u64>,
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
        _selection_polish_trigger: Option<HotkeyTrigger>,
        _translation_trigger: Option<HotkeyTrigger>,
    ) {
    }

    pub fn kind(&self) -> HotkeyAdapterKind {
        HotkeyAdapterKind::Unavailable
    }

    pub fn reset_held_state(&self) {}

    /// 移动端没有键盘监听器，永远「没看到叠加的普通键」——组合键仲裁窗口在这里
    /// 恒等于放行（`press_resolves_to_combo` 也拿不到 monitor，双保险）。
    pub fn trigger_combined_since_press(&self, _press_id: u64) -> bool {
        false
    }

    pub fn capability() -> HotkeyCapability {
        HotkeyCapability::current()
    }
}
