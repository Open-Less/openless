//! 侧键组合监听（物理左右修饰键 + 伴随键）。
//!
//! 与 [`super::hotkey`] 同属输入能力，但职责独立：热键判断"主触发键"，
//! 这里判断"哪一侧的修饰键被按下"，以及按下修饰键后再按伴随键时上报组合边沿。
//!
//! 平台后端与热键共用同一条事件流：macOS 的 CGEventTap 与 Windows 的低级钩子
//! 都会把按键转交到这里。

//! Side-specific combo hotkey matching (e.g. Left Cmd + D).
//!
//! `global-hotkey` cannot distinguish left/right modifiers. This module maintains
//! physical modifier state and matches side-aware bindings registered via [`SideAwareComboMonitor`].

use std::sync::mpsc::Sender;
use std::sync::{OnceLock, RwLock};
use std::time::Instant;

use parking_lot::Mutex;

use crate::input::hotkey::{HotkeyCombinedEdge, HotkeyEvent};

pub mod keys;

/// 组合键校验/注册失败。
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
use crate::shortcut_binding::{is_side_specific_modifier_tag, normalize_side_modifier_tag};
use crate::types::ShortcutBinding;
use openless_core::is_modifier_chord_binding;

static ACTIVE_MONITOR: OnceLock<RwLock<Option<ActiveSideCombo>>> = OnceLock::new();

struct ActiveSideCombo {
    tx: Sender<HotkeyEvent>,
    combo_tx: Sender<HotkeyCombinedEdge>,
    state: Mutex<SideAwareComboState>,
}

#[derive(Default, Clone, Copy, Debug)]
struct ModifierSideState {
    cmd_left: bool,
    cmd_right: bool,
    ctrl_left: bool,
    ctrl_right: bool,
    alt_left: bool,
    alt_right: bool,
    shift_left: bool,
    shift_right: bool,
}

#[derive(Debug)]
struct SideAwareComboState {
    binding: ShortcutBinding,
    modifiers: ModifierSideState,
    active_press_id: Option<u64>,
    companion_seen: bool,
}

impl SideAwareComboState {
    fn new(binding: ShortcutBinding) -> Self {
        Self {
            binding,
            modifiers: ModifierSideState::default(),
            active_press_id: None,
            companion_seen: false,
        }
    }

    fn set_side(&mut self, side: SideModifier, pressed: bool) {
        match side {
            SideModifier::CmdLeft => self.modifiers.cmd_left = pressed,
            SideModifier::CmdRight => self.modifiers.cmd_right = pressed,
            SideModifier::CtrlLeft => self.modifiers.ctrl_left = pressed,
            SideModifier::CtrlRight => self.modifiers.ctrl_right = pressed,
            SideModifier::AltLeft => self.modifiers.alt_left = pressed,
            SideModifier::AltRight => self.modifiers.alt_right = pressed,
            SideModifier::ShiftLeft => self.modifiers.shift_left = pressed,
            SideModifier::ShiftRight => self.modifiers.shift_right = pressed,
        }
    }

    fn expected_modifier_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .binding
            .modifiers
            .iter()
            .map(|raw| normalize_side_modifier_tag(raw))
            .collect();
        tags.sort();
        tags
    }

    fn pressed_modifier_tags(&self) -> Vec<String> {
        let mut tags = Vec::new();
        if self.modifiers.cmd_left {
            tags.push("cmd-left".into());
        }
        if self.modifiers.cmd_right {
            tags.push("cmd-right".into());
        }
        if self.modifiers.ctrl_left {
            tags.push("ctrl-left".into());
        }
        if self.modifiers.ctrl_right {
            tags.push("ctrl-right".into());
        }
        if self.modifiers.alt_left {
            tags.push("alt-left".into());
        }
        if self.modifiers.alt_right {
            tags.push("alt-right".into());
        }
        if self.modifiers.shift_left {
            tags.push("shift-left".into());
        }
        if self.modifiers.shift_right {
            tags.push("shift-right".into());
        }
        tags.sort();
        tags
    }

    fn modifiers_match(&self) -> bool {
        self.expected_modifier_tags() == self.pressed_modifier_tags()
    }

    fn activate(&mut self) -> Option<HotkeyEvent> {
        if self.active_press_id.is_some() {
            return None;
        }
        let press_id = crate::input::hotkey::next_press_id();
        self.active_press_id = Some(press_id);
        self.companion_seen = false;
        Some(HotkeyEvent::Pressed {
            at: Instant::now(),
            press_id,
        })
    }

    fn release_active(&mut self) -> Option<HotkeyEvent> {
        let press_id = self.active_press_id.take()?;
        let combined = std::mem::take(&mut self.companion_seen);
        Some(HotkeyEvent::Released {
            at: Instant::now(),
            press_id,
            combined,
        })
    }

    fn on_modifier_press(&mut self, side: SideModifier) -> Option<HotkeyEvent> {
        self.set_side(side, true);
        if is_modifier_chord_binding(&self.binding) && self.modifiers_match() {
            return self.activate();
        }
        None
    }

    fn on_primary(&mut self, primary: &str, pressed: bool) -> Option<HotkeyEvent> {
        if is_modifier_chord_binding(&self.binding) {
            return None;
        }
        if !primary_eq(&self.binding.primary, primary) {
            return None;
        }
        if pressed {
            if self.modifiers_match() {
                return self.activate();
            }
            if self.active_press_id.is_some() {
                return self.release_active();
            }
            return None;
        }
        self.release_active()
    }

    fn on_modifier_release(&mut self, side: SideModifier) -> Option<HotkeyEvent> {
        self.set_side(side, false);
        if self.active_press_id.is_some() && !self.modifiers_match() {
            return self.release_active();
        }
        None
    }

    fn on_companion_key_down(&mut self) -> Option<HotkeyCombinedEdge> {
        if !is_modifier_chord_binding(&self.binding) || self.companion_seen {
            return None;
        }
        let press_id = self.active_press_id?;
        self.companion_seen = true;
        Some(HotkeyCombinedEdge {
            at: Instant::now(),
            press_id,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideModifier {
    CmdLeft,
    CmdRight,
    CtrlLeft,
    CtrlRight,
    AltLeft,
    AltRight,
    ShiftLeft,
    ShiftRight,
}

impl SideModifier {
    fn recording_key(self) -> &'static str {
        match self {
            Self::CmdLeft => "LeftCommand", Self::CmdRight => "RightCommand",
            Self::CtrlLeft => "LeftControl", Self::CtrlRight => "RightControl",
            Self::AltLeft => "LeftAlt", Self::AltRight => "RightAlt",
            Self::ShiftLeft => "LeftShift", Self::ShiftRight => "RightShift",
        }
    }
}

pub struct SideAwareComboMonitor;

impl SideAwareComboMonitor {
    pub fn start(
        binding: ShortcutBinding,
        tx: Sender<HotkeyEvent>,
        combo_tx: Sender<HotkeyCombinedEdge>,
    ) -> Result<Self, ComboHotkeyError> {
        // Linux accepts the binding too: the egui host owns its own side-aware
        // modifier layer, so the platform dispatch this file wraps is not the
        // only source of those edges. Rejecting here would hide a working
        // configuration behind a bogus error.
        validate_side_binding(&binding)?;

        let slot = ACTIVE_MONITOR.get_or_init(|| RwLock::new(None));
        let mut guard = slot.write().expect("side combo monitor lock poisoned");
        *guard = Some(ActiveSideCombo {
            tx,
            combo_tx,
            state: Mutex::new(SideAwareComboState::new(binding)),
        });
        Ok(Self)
    }
}

pub fn validate_side_binding(
    binding: &ShortcutBinding,
) -> Result<(), ComboHotkeyError> {
    if binding.modifiers.is_empty()
        || binding
            .modifiers
            .iter()
            .any(|tag| !is_side_specific_modifier_tag(tag))
    {
        return Err(ComboHotkeyError::UnsupportedModifier(
            "binding is not side-specific".into(),
        ));
    }
    if is_modifier_chord_binding(binding) {
        return openless_core::validate_shortcut_binding(binding).map_err(|error| match error {
            openless_core::ShortcutBindingError::UnsupportedModifier(value) => {
                ComboHotkeyError::UnsupportedModifier(value)
            }
            openless_core::ShortcutBindingError::UnsupportedKey(value) => {
                ComboHotkeyError::UnsupportedKey(value)
            }
        });
    }

    keys::parse_primary(&binding.primary)
        .map_err(|e| ComboHotkeyError::UnsupportedKey(e.to_string()))?;

    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SideAwareComboMonitor {
    /// Update the existing route without replacing its handle or event sender.
    /// Used when a failed native-key switch restores an already-live shortcut.
    pub fn update_binding(
        &self,
        binding: ShortcutBinding,
    ) -> Result<(), ComboHotkeyError> {
        validate_side_binding(&binding)?;
        let slot = ACTIVE_MONITOR.get().ok_or_else(|| {
            ComboHotkeyError::RegisterFailed(
                "side-aware listener is unavailable".into(),
            )
        })?;
        let active = slot.read().expect("side combo monitor lock poisoned");
        let active = active.as_ref().ok_or_else(|| {
            ComboHotkeyError::RegisterFailed(
                "side-aware listener is unavailable".into(),
            )
        })?;
        *active.state.lock() = SideAwareComboState::new(binding);
        Ok(())
    }
}

impl Drop for SideAwareComboMonitor {
    fn drop(&mut self) {
        if let Some(slot) = ACTIVE_MONITOR.get() {
            *slot.write().expect("side combo monitor lock poisoned") = None;
        }
    }
}

fn with_active<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&ActiveSideCombo) -> R,
{
    let slot = ACTIVE_MONITOR.get()?;
    let guard = slot.read().ok()?;
    guard.as_ref().map(f)
}

fn send_event(tx: &Sender<HotkeyEvent>, evt: HotkeyEvent) {
    if let Err(err) = tx.send(evt) {
        log::warn!("[side-aware-combo] event send failed: {err}");
    }
}

fn send_combo_abort(tx: &Sender<HotkeyCombinedEdge>, edge: HotkeyCombinedEdge) {
    if let Err(err) = tx.send(edge) {
        log::warn!("[side-aware-combo] abort send failed: {err}");
    }
}

pub fn handle_side_modifier(side: SideModifier, pressed: bool) {
    if let Some(evt) = with_active(|active| {
        let mut state = active.state.lock();
        if pressed {
            state.on_modifier_press(side)
        } else {
            state.on_modifier_release(side)
        }
    })
    .flatten()
    {
        with_active(|active| send_event(&active.tx, evt));
    }
}

pub fn handle_primary_key(primary: &str, pressed: bool) {
    if let Some(evt) = with_active(|active| {
        let mut state = active.state.lock();
        state.on_primary(primary, pressed)
    })
    .flatten()
    {
        with_active(|active| send_event(&active.tx, evt));
    }
}

pub fn handle_companion_key_down() {
    if let Some(edge) = with_active(|active| active.state.lock().on_companion_key_down()).flatten()
    {
        with_active(|active| send_combo_abort(&active.combo_tx, edge));
    }
}

fn primary_eq(expected: &str, actual: &str) -> bool {
    expected.trim().eq_ignore_ascii_case(actual.trim())
}

/// Windows can translate Ctrl+ScrollLock to VK_CANCEL before WH_KEYBOARD_LL.
/// Keep its physical identity, including a release that arrives as VK_SCROLL
/// after Ctrl was released. Extended E0-46 is the separate Ctrl+Break key.
#[cfg(any(target_os = "windows", test))]
pub fn normalize_windows_vk(vk_code: u32, scan_code: u32, extended: bool) -> u32 {
    const VK_CANCEL: u32 = 0x03;
    const VK_SCROLL: u32 = 0x91;
    const SCROLL_LOCK_SCAN_CODE: u32 = 0x46;

    if vk_code == VK_CANCEL && scan_code == SCROLL_LOCK_SCAN_CODE && !extended {
        VK_SCROLL
    } else {
        vk_code
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_keycode_to_primary(keycode: i64) -> Option<&'static str> {
    match keycode {
        // A-Z (non-contiguous on macOS)
        0 => Some("A"),
        1 => Some("S"),
        2 => Some("D"),
        3 => Some("F"),
        4 => Some("H"),
        5 => Some("G"),
        6 => Some("Z"),
        7 => Some("X"),
        8 => Some("C"),
        9 => Some("V"),
        11 => Some("B"),
        12 => Some("Q"),
        13 => Some("W"),
        14 => Some("E"),
        15 => Some("R"),
        16 => Some("Y"),
        17 => Some("T"),
        31 => Some("O"),
        32 => Some("U"),
        34 => Some("I"),
        35 => Some("P"),
        37 => Some("L"),
        38 => Some("J"),
        40 => Some("K"),
        45 => Some("N"),
        46 => Some("M"),
        // 0-9 top row
        18 => Some("1"),
        19 => Some("2"),
        20 => Some("3"),
        21 => Some("4"),
        22 => Some("6"),
        23 => Some("5"),
        25 => Some("9"),
        26 => Some("7"),
        28 => Some("8"),
        29 => Some("0"),
        // Punctuation
        24 => Some("="),
        27 => Some("-"),
        30 => Some("]"),
        33 => Some("["),
        39 => Some("'"),
        41 => Some(";"),
        42 => Some("\\"),
        43 => Some(","),
        44 => Some("/"),
        47 => Some("."),
        50 => Some("`"),
        // Special keys
        36 => Some("Enter"),
        48 => Some("Tab"),
        49 => Some("Space"),
        51 => Some("Backspace"),
        53 => Some("Escape"),
        117 => Some("Delete"),
        123 => Some("ArrowLeft"),
        124 => Some("ArrowRight"),
        125 => Some("ArrowDown"),
        126 => Some("ArrowUp"),
        // Function keys
        122 => Some("F1"),
        120 => Some("F2"),
        99 => Some("F3"),
        118 => Some("F4"),
        96 => Some("F5"),
        97 => Some("F6"),
        98 => Some("F7"),
        100 => Some("F8"),
        101 => Some("F9"),
        109 => Some("F10"),
        103 => Some("F11"),
        111 => Some("F12"),
        105 => Some("F13"),
        107 => Some("F14"),
        113 => Some("F15"),
        106 => Some("F16"),
        64 => Some("F17"),
        79 => Some("F18"),
        80 => Some("F19"),
        90 => Some("F20"),
        _ => None,
    }
}


#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "macos")]
pub mod macos;

/// 当前平台的侧键分发后端。
#[cfg(target_os = "windows")]
pub use windows as platform;
#[cfg(target_os = "macos")]
pub use macos as platform;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_ctrl_scroll_lock_normalizes_cancel_but_preserves_other_keys() {
        assert_eq!(normalize_windows_vk(0x03, 0x46, false), 0x91);
        assert_eq!(normalize_windows_vk(0x91, 0x46, false), 0x91);
        // Do not turn a dedicated Ctrl+Break, a scan-less injected Cancel,
        // or another key at the same scan code into ScrollLock.
        assert_eq!(normalize_windows_vk(0x03, 0x46, true), 0x03);
        assert_eq!(normalize_windows_vk(0x03, 0, false), 0x03);
        assert_eq!(normalize_windows_vk(0x03, 0x45, false), 0x03);
        assert_eq!(normalize_windows_vk(0x41, 0x46, false), 0x41);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn restoring_live_side_binding_preserves_its_event_route() {
        use std::sync::mpsc;
        let binding = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["ctrl-right".into()],
        };
        let (tx, rx) = mpsc::channel();
        let (combo_tx, _combo_rx) = mpsc::channel();
        let monitor = SideAwareComboMonitor::start(binding.clone(), tx, combo_tx).unwrap();
        let press_and_release = |primary: &str| {
            handle_side_modifier(SideModifier::CtrlRight, true);
            handle_primary_key(primary, true);
            handle_primary_key(primary, false);
            handle_side_modifier(SideModifier::CtrlRight, false);
            assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Pressed { .. })));
            assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Released { .. })));
            assert!(rx.try_recv().is_err());
        };
        press_and_release("D");
        // Native registration failed before removing this monitor. The reverse
        // transaction must reuse the live route, even on repeated restoration.
        monitor.update_binding(binding.clone()).unwrap();
        monitor.update_binding(binding).unwrap();
        press_and_release("D");
        assert!(monitor
            .update_binding(ShortcutBinding {
                primary: "F25".into(),
                modifiers: vec!["ctrl-right".into()]
            })
            .is_err());
        press_and_release("D");
        monitor
            .update_binding(ShortcutBinding {
                primary: "F20".into(),
                modifiers: vec!["ctrl-right".into()],
            })
            .unwrap();
        press_and_release("F20");
        drop(monitor);
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn extended_macos_function_keys_use_carbon_keycodes() {
        for (keycode, primary) in [
            (105, "F13"),
            (107, "F14"),
            (113, "F15"),
            (106, "F16"),
            (64, "F17"),
            (79, "F18"),
            (80, "F19"),
            (90, "F20"),
        ] {
            assert_eq!(macos_keycode_to_primary(keycode), Some(primary));
        }
    }

    #[test]
    fn macos_keycode_2_is_d() {
        assert_eq!(macos_keycode_to_primary(2), Some("D"));
    }

    #[test]
    fn macos_keycode_us_ansi_samples() {
        assert_eq!(macos_keycode_to_primary(17), Some("T"));
        assert_eq!(macos_keycode_to_primary(11), Some("B"));
        assert_eq!(macos_keycode_to_primary(24), Some("="));
        assert_eq!(macos_keycode_to_primary(29), Some("0"));
        assert_eq!(macos_keycode_to_primary(44), Some("/"));
    }

    #[test]
    fn macos_letter_keycodes_are_non_contiguous() {
        assert_eq!(macos_keycode_to_primary(0), Some("A"));
        assert_eq!(macos_keycode_to_primary(1), Some("S"));
        assert_eq!(macos_keycode_to_primary(8), Some("C"));
        assert_eq!(macos_keycode_to_primary(9), Some("V"));
        assert_eq!(macos_keycode_to_primary(12), Some("Q"));
        assert_eq!(macos_keycode_to_primary(17), Some("T"));
    }

    #[test]
    fn side_specific_combo_matches_required_modifiers() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd-left".into()],
        });
        state.set_side(SideModifier::CmdLeft, true);
        assert!(state.modifiers_match());
        let evt = state.on_primary("D", true);
        assert!(matches!(evt, Some(HotkeyEvent::Pressed { .. })));
    }

    #[test]
    fn shift_right_combo_requires_right_shift() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["shift-right".into()],
        });
        state.set_side(SideModifier::ShiftLeft, true);
        assert!(!state.modifiers_match());
        state.set_side(SideModifier::ShiftLeft, false);
        state.set_side(SideModifier::ShiftRight, true);
        assert!(state.modifiers_match());
        assert!(matches!(
            state.on_primary("D", true),
            Some(HotkeyEvent::Pressed { .. })
        ));
    }

    #[test]
    fn extra_modifier_blocks_exact_match() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd-left".into()],
        });
        state.set_side(SideModifier::CmdLeft, true);
        state.set_side(SideModifier::ShiftLeft, true);
        assert!(!state.modifiers_match());
        assert_eq!(state.on_primary("D", true), None);
    }

    #[test]
    fn releasing_extra_modifier_allows_combo() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd-left".into()],
        });
        state.set_side(SideModifier::CmdLeft, true);
        state.set_side(SideModifier::CmdRight, true);
        assert!(!state.modifiers_match());
        state.set_side(SideModifier::CmdRight, false);
        assert!(state.modifiers_match());
    }

    #[test]
    fn super_left_alias_matches_physical_cmd_left() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["super-left".into()],
        });
        state.set_side(SideModifier::CmdLeft, true);
        assert!(state.modifiers_match());
        assert!(matches!(
            state.on_primary("D", true),
            Some(HotkeyEvent::Pressed { .. })
        ));
    }

    #[test]
    fn super_left_alias_rejects_extra_modifier() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["super-left".into()],
        });
        state.set_side(SideModifier::CmdLeft, true);
        state.set_side(SideModifier::ShiftLeft, true);
        assert!(!state.modifiers_match());
        assert_eq!(state.on_primary("D", true), None);
    }

    #[test]
    fn modifier_chord_activates_on_final_modifier_and_releases_with_same_generation() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "ModifierChord".into(),
            modifiers: vec!["ctrl-left".into(), "cmd-left".into()],
        });

        assert_eq!(state.on_modifier_press(SideModifier::CtrlLeft), None);
        let press_id = match state.on_modifier_press(SideModifier::CmdLeft) {
            Some(HotkeyEvent::Pressed { press_id, .. }) => press_id,
            other => panic!("expected modifier chord Pressed, got {other:?}"),
        };
        assert_ne!(press_id, 0);
        assert_eq!(state.on_modifier_press(SideModifier::CmdLeft), None);

        assert!(matches!(
            state.on_modifier_release(SideModifier::CtrlLeft),
            Some(HotkeyEvent::Released { press_id: released_id, .. }) if released_id == press_id
        ));
        assert!(state.active_press_id.is_none());
    }

    #[test]
    fn modifier_chord_companion_aborts_once_but_still_pairs_release() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "ModifierChord".into(),
            modifiers: vec!["ctrl-left".into(), "cmd-left".into()],
        });
        state.on_modifier_press(SideModifier::CtrlLeft);
        let press_id = match state.on_modifier_press(SideModifier::CmdLeft) {
            Some(HotkeyEvent::Pressed { press_id, .. }) => press_id,
            other => panic!("expected modifier chord Pressed, got {other:?}"),
        };

        assert!(matches!(
            state.on_companion_key_down(),
            Some(HotkeyCombinedEdge { press_id: combined_id, .. }) if combined_id == press_id
        ));
        assert_eq!(state.on_companion_key_down(), None);
        assert!(matches!(
            state.on_modifier_release(SideModifier::CmdLeft),
            Some(HotkeyEvent::Released { press_id: released_id, .. }) if released_id == press_id
        ));
    }

    #[test]
    fn extra_modifier_does_not_activate_modifier_chord_when_it_is_released() {
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "ModifierChord".into(),
            modifiers: vec!["ctrl-left".into(), "cmd-left".into()],
        });
        assert_eq!(state.on_modifier_press(SideModifier::CtrlLeft), None);
        assert_eq!(state.on_modifier_press(SideModifier::ShiftLeft), None);
        assert_eq!(state.on_modifier_press(SideModifier::CmdLeft), None);
        assert_eq!(state.on_modifier_release(SideModifier::ShiftLeft), None);
        assert!(state.active_press_id.is_none());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_side_keycodes_are_distinct() {
        use crate::input::side_combo::platform::side_from_keycode;
        assert_eq!(side_from_keycode(55), Some(SideModifier::CmdLeft));
        assert_eq!(side_from_keycode(54), Some(SideModifier::CmdRight));
        assert_eq!(side_from_keycode(56), Some(SideModifier::ShiftLeft));
        assert_eq!(side_from_keycode(60), Some(SideModifier::ShiftRight));
    }

    // ---- Fix 1: event-pairing self-healing (Pressed always paired with Released) ----

    fn cmd_left_d_state() -> SideAwareComboState {
        SideAwareComboState::new(ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd-left".into()],
        })
    }

    #[test]
    fn normal_press_then_release_is_paired() {
        let mut state = cmd_left_d_state();
        state.set_side(SideModifier::CmdLeft, true);
        assert!(matches!(
            state.on_primary("D", true),
            Some(HotkeyEvent::Pressed { .. })
        ));
        // Primary key up terminates the combo with exactly one Released.
        assert!(matches!(
            state.on_primary("D", false),
            Some(HotkeyEvent::Released { .. })
        ));
        // No trailing events; a second key-up must not emit anything.
        assert_eq!(state.on_primary("D", false), None);
        assert!(state.active_press_id.is_none());
    }

    #[test]
    fn modifier_release_after_press_emits_paired_released() {
        let mut state = cmd_left_d_state();
        state.set_side(SideModifier::CmdLeft, true);
        assert!(matches!(
            state.on_primary("D", true),
            Some(HotkeyEvent::Pressed { .. })
        ));
        // Modifier lifts while primary is still down -> combo terminates once.
        assert!(matches!(
            state.on_modifier_release(SideModifier::CmdLeft),
            Some(HotkeyEvent::Released { .. })
        ));
        assert!(state.active_press_id.is_none());
        // A now-orphaned primary key-up must NOT emit a second Released.
        assert_eq!(state.on_primary("D", false), None);
    }

    #[test]
    fn dropped_modifier_release_is_recovered_on_primary_up() {
        // Simulate the OS dropping the modifier-up event: the active press stays latched
        // and modifiers still "match" from the state's perspective. The primary
        // key-up (absolute termination) must still emit the paired Released.
        let mut state = cmd_left_d_state();
        state.set_side(SideModifier::CmdLeft, true);
        assert!(matches!(
            state.on_primary("D", true),
            Some(HotkeyEvent::Pressed { .. })
        ));
        // Modifier physically released but the release event never arrived, so the
        // side flag is still set here. Primary up is the fallback terminator.
        assert!(matches!(
            state.on_primary("D", false),
            Some(HotkeyEvent::Released { .. })
        ));
        assert!(state.active_press_id.is_none());
    }

    #[test]
    fn stale_combo_active_reset_when_modifiers_stop_matching() {
        // Reproduce a stuck latch: a prior Released was lost so an active press is still latched,
        // yet the required modifier is no longer held (modifiers_match() == false).
        // The next primary-down must self-heal by emitting the terminal Released
        // (NOT swallow it, and NOT emit a second Pressed) so recording can't stick.
        let mut state = cmd_left_d_state();
        state.active_press_id = Some(crate::input::hotkey::next_press_id()); // stale flag from a dropped Released
        assert!(!state.modifiers_match()); // cmd-left is not held
        let evt = state.on_primary("D", true);
        assert!(matches!(evt, Some(HotkeyEvent::Released { .. })));
        assert!(state.active_press_id.is_none());
    }

    #[test]
    fn dropped_release_then_fresh_full_press_triggers_again() {
        // End-to-end: after a lost Released, a subsequent *complete* press cycle
        // must be able to fire a fresh Pressed. Guards against the combo becoming
        // permanently unrepeatable (the historical #545/#468 stuck-latch class).
        let mut state = cmd_left_d_state();
        state.active_press_id = Some(crate::input::hotkey::next_press_id()); // leftover from a dropped Released

        // Releasing the required side-modifier breaks the match, so the stale latch
        // self-heals by emitting the terminal Released here (pairing the Pressed whose
        // Released was dropped). Either way the active press must end up cleared.
        assert!(matches!(
            state.on_modifier_release(SideModifier::CmdLeft),
            Some(HotkeyEvent::Released { .. })
        ));
        assert!(state.active_press_id.is_none());

        // Fresh, clean press cycle now behaves normally.
        state.set_side(SideModifier::CmdLeft, true);
        assert!(matches!(
            state.on_primary("D", true),
            Some(HotkeyEvent::Pressed { .. })
        ));
        assert!(matches!(
            state.on_primary("D", false),
            Some(HotkeyEvent::Released { .. })
        ));
    }

    #[test]
    fn stale_combo_active_still_matching_does_not_double_press() {
        // If the flag is stale-true but modifiers currently match (edge case where a
        // Released was lost but the modifier happens to be held again), a primary-down
        // must NOT emit a second Pressed — that would break the pairing invariant.
        let mut state = cmd_left_d_state();
        state.set_side(SideModifier::CmdLeft, true);
        state.active_press_id = Some(crate::input::hotkey::next_press_id()); // pretend previous Released was dropped
        assert!(state.modifiers_match());
        assert_eq!(state.on_primary("D", true), None);
        // The real terminator (primary up) still yields exactly one Released.
        assert!(matches!(
            state.on_primary("D", false),
            Some(HotkeyEvent::Released { .. })
        ));
    }

    // ---- Fix 2: macOS race-free FLAGS_CHANGED side classification ----

    #[cfg(target_os = "macos")]
    #[test]
    fn flags_class_bit_clear_forces_side_released() {
        use crate::input::side_combo::platform::resolve_side_pressed_from_flags;
        // Command class bit clear -> Cmd keys (55 left / 54 right) are decisively up.
        assert_eq!(resolve_side_pressed_from_flags(55, 0), Some(false));
        assert_eq!(resolve_side_pressed_from_flags(54, 0), Some(false));
        // Shift bit set but Command bit clear -> a Cmd keycode is still decisively up.
        let shift_only: u64 = 1 << 17;
        assert_eq!(resolve_side_pressed_from_flags(55, shift_only), Some(false));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn flags_class_bit_set_defers_to_query() {
        use crate::input::side_combo::platform::resolve_side_pressed_from_flags;
        // Command class bit set -> cannot tell left from right; defer (None) so the
        // caller resolves the specific side via CGEventSourceKeyState.
        let cmd_bit: u64 = 1 << 20;
        assert_eq!(resolve_side_pressed_from_flags(55, cmd_bit), None);
        assert_eq!(resolve_side_pressed_from_flags(54, cmd_bit), None);
        // Shift keycodes with the Shift bit set likewise defer.
        let shift_bit: u64 = 1 << 17;
        assert_eq!(resolve_side_pressed_from_flags(56, shift_bit), None);
        assert_eq!(resolve_side_pressed_from_flags(60, shift_bit), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn flags_classifier_maps_each_modifier_class() {
        use crate::input::side_combo::platform::resolve_side_pressed_from_flags;
        // Each side keycode keys off its own class bit; an unrelated set bit does not
        // count as "held" for that class, so the side is forced released.
        let ctrl_bit: u64 = 1 << 18;
        let alt_bit: u64 = 1 << 19;
        // Ctrl (59/62) held -> defer; Alt (58/61) with only Ctrl set -> forced up.
        assert_eq!(resolve_side_pressed_from_flags(59, ctrl_bit), None);
        assert_eq!(resolve_side_pressed_from_flags(58, ctrl_bit), Some(false));
        assert_eq!(resolve_side_pressed_from_flags(58, alt_bit), None);
        // Non-side keycode (e.g. 'D' == 2) is never classified here.
        assert_eq!(resolve_side_pressed_from_flags(2, alt_bit), None);
        assert_eq!(resolve_side_pressed_from_flags(2, 0), None);
    }
}
