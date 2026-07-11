//! Side-specific combo hotkey matching (e.g. Left Cmd + D).
//!
//! `global-hotkey` cannot distinguish left/right modifiers. This module maintains
//! physical modifier state and matches combos registered via [`SideAwareComboMonitor`].

use std::sync::mpsc::Sender;
use std::sync::{OnceLock, RwLock};

use parking_lot::Mutex;

use crate::combo_hotkey::ComboHotkeyEvent;
use crate::shortcut_binding::{is_side_specific_modifier_tag, normalize_side_modifier_tag};
use crate::types::ShortcutBinding;

static ACTIVE_MONITOR: OnceLock<RwLock<Option<ActiveSideCombo>>> = OnceLock::new();

struct ActiveSideCombo {
    tx: Sender<ComboHotkeyEvent>,
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
    combo_active: bool,
}

impl SideAwareComboState {
    fn new(binding: ShortcutBinding) -> Self {
        Self {
            binding,
            modifiers: ModifierSideState::default(),
            combo_active: false,
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

    fn on_primary(&mut self, primary: &str, pressed: bool) -> Option<ComboHotkeyEvent> {
        if !primary_eq(&self.binding.primary, primary) {
            return None;
        }
        if pressed {
            if self.modifiers_match() {
                // `modifiers_match()` is the authoritative activation gate. If
                // `combo_active` is still true here, the previous `Released` was
                // dropped by the OS: we must NOT emit a second `Pressed` (that
                // would break the pairing invariant), so treat the flag as
                // already reflecting an active combo and swallow this edge.
                if !self.combo_active {
                    self.combo_active = true;
                    return Some(ComboHotkeyEvent::Pressed);
                }
                return None;
            }
            // Modifiers no longer match — this is the single authoritative reset
            // condition. If `combo_active` is stuck true (a modifier release was
            // dropped so no `Released` was ever emitted), self-heal by emitting
            // the terminal `Released` now so the recording latch cannot stick.
            if self.combo_active {
                self.combo_active = false;
                return Some(ComboHotkeyEvent::Released);
            }
            return None;
        }
        // Primary key up is the absolute termination signal for the combo.
        if self.combo_active {
            self.combo_active = false;
            return Some(ComboHotkeyEvent::Released);
        }
        None
    }

    fn on_modifier_release(&mut self, side: SideModifier) -> Option<ComboHotkeyEvent> {
        self.set_side(side, false);
        // Modifiers no longer matching is the authoritative reset condition:
        // once the required side-modifier set is broken, the combo is over.
        if self.combo_active && !self.modifiers_match() {
            self.combo_active = false;
            return Some(ComboHotkeyEvent::Released);
        }
        None
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

pub struct SideAwareComboMonitor;

impl SideAwareComboMonitor {
    pub fn start(
        binding: ShortcutBinding,
        tx: Sender<ComboHotkeyEvent>,
    ) -> Result<Self, crate::combo_hotkey::ComboHotkeyError> {
        // Linux has no side-aware platform dispatch (no CGEventTap / WH_KEYBOARD_LL
        // equivalent wired here). Accepting the binding would leave the user with a
        // "successfully configured" combo that silently never fires. Reject up front
        // so the caller can surface an actionable error instead.
        #[cfg(target_os = "linux")]
        {
            let _ = (&binding, &tx);
            return Err(crate::combo_hotkey::ComboHotkeyError::UnsupportedModifier(
                "侧向修饰键组合键在 Linux 暂不支持".into(),
            ));
        }

        #[cfg(not(target_os = "linux"))]
        {
        if binding.modifiers.is_empty()
            || binding
                .modifiers
                .iter()
                .any(|tag| !is_side_specific_modifier_tag(tag))
        {
            return Err(crate::combo_hotkey::ComboHotkeyError::UnsupportedModifier(
                "binding is not side-specific".into(),
            ));
        }
        crate::shortcut_binding::parse_primary(&binding.primary).map_err(|e| {
            crate::combo_hotkey::ComboHotkeyError::UnsupportedKey(e.to_string())
        })?;

        let slot = ACTIVE_MONITOR.get_or_init(|| RwLock::new(None));
        let mut guard = slot.write().expect("side combo monitor lock poisoned");
        *guard = Some(ActiveSideCombo {
            tx,
            state: Mutex::new(SideAwareComboState::new(binding)),
        });
        Ok(Self)
        }
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

fn send_event(tx: &Sender<ComboHotkeyEvent>, evt: ComboHotkeyEvent) {
    if let Err(err) = tx.send(evt) {
        log::warn!("[side-aware-combo] event send failed: {err}");
    }
}

pub fn handle_side_modifier(side: SideModifier, pressed: bool) {
    if let Some(evt) = with_active(|active| {
        let mut state = active.state.lock();
        if pressed {
            state.set_side(side, true);
            None
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

fn primary_eq(expected: &str, actual: &str) -> bool {
    expected.trim().eq_ignore_ascii_case(actual.trim())
}

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2,
        VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_HOME, VK_INSERT, VK_LCONTROL,
        VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5,
        VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS, VK_RETURN,
        VK_RIGHT, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP,
    };

    pub fn dispatch_vk(vk_code: u32, pressed: bool) {
        if let Some(side) = side_from_vk(vk_code) {
            handle_side_modifier(side, pressed);
            return;
        }
        if let Some(primary) = primary_from_vk(vk_code) {
            handle_primary_key(&primary, pressed);
        }
    }

    fn side_from_vk(vk_code: u32) -> Option<SideModifier> {
        match vk_code {
            x if x == VK_LWIN.0 as u32 => Some(SideModifier::CmdLeft),
            x if x == VK_RWIN.0 as u32 => Some(SideModifier::CmdRight),
            x if x == VK_LCONTROL.0 as u32 => Some(SideModifier::CtrlLeft),
            x if x == VK_RCONTROL.0 as u32 => Some(SideModifier::CtrlRight),
            x if x == VK_LMENU.0 as u32 => Some(SideModifier::AltLeft),
            x if x == VK_RMENU.0 as u32 => Some(SideModifier::AltRight),
            x if x == VK_LSHIFT.0 as u32 => Some(SideModifier::ShiftLeft),
            x if x == VK_RSHIFT.0 as u32 => Some(SideModifier::ShiftRight),
            _ => None,
        }
    }

    fn primary_from_vk(vk_code: u32) -> Option<String> {
        if (0x41..=0x5A).contains(&vk_code) {
            return Some(((vk_code as u8) as char).to_string());
        }
        if (0x30..=0x39).contains(&vk_code) {
            return Some(((vk_code as u8) as char).to_string());
        }
        let named = match vk_code {
            x if x == VK_SPACE.0 as u32 => "Space",
            x if x == VK_RETURN.0 as u32 => "Enter",
            x if x == VK_TAB.0 as u32 => "Tab",
            x if x == VK_BACK.0 as u32 => "Backspace",
            x if x == VK_DELETE.0 as u32 => "Delete",
            x if x == VK_ESCAPE.0 as u32 => "Escape",
            x if x == VK_HOME.0 as u32 => "Home",
            x if x == VK_END.0 as u32 => "End",
            x if x == VK_INSERT.0 as u32 => "Insert",
            x if x == VK_UP.0 as u32 => "ArrowUp",
            x if x == VK_DOWN.0 as u32 => "ArrowDown",
            x if x == VK_LEFT.0 as u32 => "ArrowLeft",
            x if x == VK_RIGHT.0 as u32 => "ArrowRight",
            x if x == VK_F1.0 as u32 => "F1",
            x if x == VK_F2.0 as u32 => "F2",
            x if x == VK_F3.0 as u32 => "F3",
            x if x == VK_F4.0 as u32 => "F4",
            x if x == VK_F5.0 as u32 => "F5",
            x if x == VK_F6.0 as u32 => "F6",
            x if x == VK_F7.0 as u32 => "F7",
            x if x == VK_F8.0 as u32 => "F8",
            x if x == VK_F9.0 as u32 => "F9",
            x if x == VK_F10.0 as u32 => "F10",
            x if x == VK_F11.0 as u32 => "F11",
            x if x == VK_F12.0 as u32 => "F12",
            x if x == VK_OEM_1.0 as u32 => ";",
            x if x == VK_OEM_PLUS.0 as u32 => "=",
            x if x == VK_OEM_COMMA.0 as u32 => ",",
            x if x == VK_OEM_MINUS.0 as u32 => "-",
            x if x == VK_OEM_PERIOD.0 as u32 => ".",
            x if x == VK_OEM_2.0 as u32 => "/",
            x if x == VK_OEM_3.0 as u32 => "`",
            x if x == VK_OEM_4.0 as u32 => "[",
            x if x == VK_OEM_5.0 as u32 => "\\",
            x if x == VK_OEM_6.0 as u32 => "]",
            x if x == VK_OEM_7.0 as u32 => "'",
            _ => return None,
        };
        Some(named.to_string())
    }
}

/// macOS virtual keycode → ShortcutBinding.primary (US ANSI layout).
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
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub mod platform {
    use super::*;

    use std::os::raw::c_int;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceKeyState(state: c_int, key: u16) -> bool;
    }

    const COMBINED_SESSION: c_int = 1;

    // NSEventModifierFlags device-independent class bits (also match the CGEventFlags
    // masks used by hotkey.rs `handle_flags_changed`). One bit per modifier *class* —
    // it does NOT encode which side (left/right) is held.
    const FLAG_MASK_SHIFT: u64 = 1 << 17; // 0x0002_0000
    const FLAG_MASK_CONTROL: u64 = 1 << 18; // 0x0004_0000
    const FLAG_MASK_ALTERNATE: u64 = 1 << 19; // 0x0008_0000 (Option/Alt)
    const FLAG_MASK_COMMAND: u64 = 1 << 20; // 0x0010_0000

    pub fn dispatch_keycode(keycode: i64, flags_changed: bool, flags: u64, pressed: bool) {
        if flags_changed {
            if let Some(side) = side_from_keycode(keycode) {
                // The event-carried `flags` are race-free (they describe the state
                // *at* this event); `CGEventSourceKeyState` is a separate query that
                // may observe a later state. Prefer flags where they are decisive.
                let down = match resolve_side_pressed_from_flags(keycode, flags) {
                    Some(down) => down,
                    // Class bit is set: some key in this modifier class is down, but a
                    // single flags bit cannot tell left from right. Fall back to
                    // `CGEventSourceKeyState` (Apple's standard way to resolve side).
                    None => keycode_is_down(keycode),
                };
                handle_side_modifier(side, down);
            }
            return;
        }

        if let Some(primary) = primary_from_keycode(keycode) {
            handle_primary_key(&primary, pressed);
        }
    }

    /// Modifier-class flag bit for a side-modifier keycode, or `None` if the keycode
    /// is not a known side modifier.
    fn class_mask_for_keycode(keycode: i64) -> Option<u64> {
        match keycode {
            55 | 54 => Some(FLAG_MASK_COMMAND),     // Cmd left / right
            59 | 62 => Some(FLAG_MASK_CONTROL),     // Ctrl left / right
            58 | 61 => Some(FLAG_MASK_ALTERNATE),   // Alt/Option left / right
            56 | 60 => Some(FLAG_MASK_SHIFT),       // Shift left / right
            _ => None,
        }
    }

    /// Pure race-free classifier for a FLAGS_CHANGED side-modifier event.
    ///
    /// Returns:
    /// - `Some(false)` when the event's `flags` show the modifier *class* fully
    ///   released (class bit clear). This is the dangerous case to get wrong —
    ///   a stuck-down misread leaves the combo latched — and the flags bit makes
    ///   it unambiguous with no query race. Force the side up.
    /// - `None` when the class bit is set: at least one side of this class is held,
    ///   but a single class bit cannot distinguish left vs right, so the caller
    ///   must query `CGEventSourceKeyState` to resolve the specific side.
    /// - `None` for non-side keycodes (caller will have already filtered these).
    pub(crate) fn resolve_side_pressed_from_flags(keycode: i64, flags: u64) -> Option<bool> {
        let mask = class_mask_for_keycode(keycode)?;
        if flags & mask == 0 {
            Some(false)
        } else {
            None
        }
    }

    fn keycode_is_down(keycode: i64) -> bool {
        unsafe { CGEventSourceKeyState(COMBINED_SESSION, keycode as u16) }
    }

    pub(crate) fn side_from_keycode(keycode: i64) -> Option<SideModifier> {
        match keycode {
            55 => Some(SideModifier::CmdLeft),
            54 => Some(SideModifier::CmdRight),
            59 => Some(SideModifier::CtrlLeft),
            62 => Some(SideModifier::CtrlRight),
            58 => Some(SideModifier::AltLeft),
            61 => Some(SideModifier::AltRight),
            56 => Some(SideModifier::ShiftLeft),
            60 => Some(SideModifier::ShiftRight),
            _ => None,
        }
    }

    fn primary_from_keycode(keycode: i64) -> Option<String> {
        super::macos_keycode_to_primary(keycode).map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(evt, Some(ComboHotkeyEvent::Pressed));
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
        assert_eq!(
            state.on_primary("D", true),
            Some(ComboHotkeyEvent::Pressed)
        );
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
        assert_eq!(
            state.on_primary("D", true),
            Some(ComboHotkeyEvent::Pressed)
        );
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

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_side_keycodes_are_distinct() {
        use crate::side_aware_combo::platform::side_from_keycode;
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
        assert_eq!(state.on_primary("D", true), Some(ComboHotkeyEvent::Pressed));
        // Primary key up terminates the combo with exactly one Released.
        assert_eq!(
            state.on_primary("D", false),
            Some(ComboHotkeyEvent::Released)
        );
        // No trailing events; a second key-up must not emit anything.
        assert_eq!(state.on_primary("D", false), None);
        assert!(!state.combo_active);
    }

    #[test]
    fn modifier_release_after_press_emits_paired_released() {
        let mut state = cmd_left_d_state();
        state.set_side(SideModifier::CmdLeft, true);
        assert_eq!(state.on_primary("D", true), Some(ComboHotkeyEvent::Pressed));
        // Modifier lifts while primary is still down -> combo terminates once.
        assert_eq!(
            state.on_modifier_release(SideModifier::CmdLeft),
            Some(ComboHotkeyEvent::Released)
        );
        assert!(!state.combo_active);
        // A now-orphaned primary key-up must NOT emit a second Released.
        assert_eq!(state.on_primary("D", false), None);
    }

    #[test]
    fn dropped_modifier_release_is_recovered_on_primary_up() {
        // Simulate the OS dropping the modifier-up event: combo_active stays true
        // and modifiers still "match" from the state's perspective. The primary
        // key-up (absolute termination) must still emit the paired Released.
        let mut state = cmd_left_d_state();
        state.set_side(SideModifier::CmdLeft, true);
        assert_eq!(state.on_primary("D", true), Some(ComboHotkeyEvent::Pressed));
        // Modifier physically released but the release event never arrived, so the
        // side flag is still set here. Primary up is the fallback terminator.
        assert_eq!(
            state.on_primary("D", false),
            Some(ComboHotkeyEvent::Released)
        );
        assert!(!state.combo_active);
    }

    #[test]
    fn stale_combo_active_reset_when_modifiers_stop_matching() {
        // Reproduce a stuck latch: a prior Released was lost so combo_active is true,
        // yet the required modifier is no longer held (modifiers_match() == false).
        // The next primary-down must self-heal by emitting the terminal Released
        // (NOT swallow it, and NOT emit a second Pressed) so recording can't stick.
        let mut state = cmd_left_d_state();
        state.combo_active = true; // stale flag from a dropped Released
        assert!(!state.modifiers_match()); // cmd-left is not held
        let evt = state.on_primary("D", true);
        assert_eq!(evt, Some(ComboHotkeyEvent::Released));
        assert!(!state.combo_active);
    }

    #[test]
    fn dropped_release_then_fresh_full_press_triggers_again() {
        // End-to-end: after a lost Released, a subsequent *complete* press cycle
        // must be able to fire a fresh Pressed. Guards against the combo becoming
        // permanently unrepeatable (the historical #545/#468 stuck-latch class).
        let mut state = cmd_left_d_state();
        state.combo_active = true; // leftover from a dropped Released

        // Releasing the required side-modifier breaks the match, so the stale latch
        // self-heals by emitting the terminal Released here (pairing the Pressed whose
        // Released was dropped). Either way combo_active must end up cleared.
        assert_eq!(
            state.on_modifier_release(SideModifier::CmdLeft),
            Some(ComboHotkeyEvent::Released)
        );
        assert!(!state.combo_active);

        // Fresh, clean press cycle now behaves normally.
        state.set_side(SideModifier::CmdLeft, true);
        assert_eq!(state.on_primary("D", true), Some(ComboHotkeyEvent::Pressed));
        assert_eq!(
            state.on_primary("D", false),
            Some(ComboHotkeyEvent::Released)
        );
    }

    #[test]
    fn stale_combo_active_still_matching_does_not_double_press() {
        // If the flag is stale-true but modifiers currently match (edge case where a
        // Released was lost but the modifier happens to be held again), a primary-down
        // must NOT emit a second Pressed — that would break the pairing invariant.
        let mut state = cmd_left_d_state();
        state.set_side(SideModifier::CmdLeft, true);
        state.combo_active = true; // pretend previous Released was dropped
        assert!(state.modifiers_match());
        assert_eq!(state.on_primary("D", true), None);
        // The real terminator (primary up) still yields exactly one Released.
        assert_eq!(
            state.on_primary("D", false),
            Some(ComboHotkeyEvent::Released)
        );
    }

    // ---- Fix 2: macOS race-free FLAGS_CHANGED side classification ----

    #[cfg(target_os = "macos")]
    #[test]
    fn flags_class_bit_clear_forces_side_released() {
        use crate::side_aware_combo::platform::resolve_side_pressed_from_flags;
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
        use crate::side_aware_combo::platform::resolve_side_pressed_from_flags;
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
        use crate::side_aware_combo::platform::resolve_side_pressed_from_flags;
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
