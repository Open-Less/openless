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
    binding: ShortcutBinding,
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
            if self.modifiers_match() && !self.combo_active {
                self.combo_active = true;
                return Some(ComboHotkeyEvent::Pressed);
            }
            return None;
        }
        if self.combo_active {
            self.combo_active = false;
            return Some(ComboHotkeyEvent::Released);
        }
        None
    }

    fn on_modifier_release(&mut self, side: SideModifier) -> Option<ComboHotkeyEvent> {
        self.set_side(side, false);
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
            binding: binding.clone(),
            tx,
            state: Mutex::new(SideAwareComboState::new(binding)),
        });
        Ok(Self)
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

    pub fn dispatch_keycode(keycode: i64, flags_changed: bool, _flags: u64, pressed: bool) {
        if flags_changed {
            if let Some(side) = side_from_keycode(keycode) {
                handle_side_modifier(side, keycode_is_down(keycode));
            }
            return;
        }

        if let Some(primary) = primary_from_keycode(keycode) {
            handle_primary_key(&primary, pressed);
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
}
