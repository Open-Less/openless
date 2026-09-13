//! Mobile stub — global hotkeys are unavailable on Android/iOS.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::types::{
    HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError, HotkeyTrigger,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed { at: Instant, press_id: u64 },
    Released { at: Instant, press_id: u64 },
    // 组合键撤销与 Esc 取消在移动端无全局键盘监听，不在此枚举里（见 hotkey.rs 模块注释）。
    TranslationModifierPressed,
    QaShortcutPressed,
    // SelectionPolishShortcutPressed 为桌面（Windows-first）选区润色专属，mobile stub 不声明。
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyCombinedEdge {
    pub at: Instant,
    pub press_id: u64,
}

/// 窗口内开发注入仍需与桌面端使用同一种 press identity。移动端没有全局监听器，
/// 但生成单调 id 的纯规则不能因此缺席，否则 Android 条件编译会出现另一套事件形状。
pub fn next_press_id() -> u64 {
    static NEXT_PRESS_ID: AtomicU64 = AtomicU64::new(0);
    NEXT_PRESS_ID
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
}

/// Mobile 无全局键盘监听，Esc 独占为 no-op。
pub fn set_esc_exclusive(_active: bool) {}

pub struct HotkeyMonitor;

impl HotkeyMonitor {
    pub fn start(
        _binding: HotkeyBinding,
        _tx: Sender<HotkeyEvent>,
        _cancel_tx: Sender<()>,
        _combo_tx: Sender<HotkeyCombinedEdge>,
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

    pub fn capability() -> HotkeyCapability {
        HotkeyCapability::current()
    }
}
