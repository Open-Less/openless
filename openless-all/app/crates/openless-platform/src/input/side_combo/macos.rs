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

pub fn dispatch_keycode(keycode: i64, flags_changed: bool, flags: u64, pressed: bool) -> bool {
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
            crate::input::hotkey::recording_companion_event(side.recording_key(), down);
            handle_side_modifier(side, down);
        } else if keycode == 63 {
            crate::input::hotkey::recording_companion_event("Fn", flags & (1 << 23) != 0);
        }
        return false;
    }

    crate::recording_companion::sync_fn(flags & (1 << 23) != 0);
    if let Some(primary) = primary_from_keycode(keycode) {
        if crate::input::hotkey::recording_companion_event(&primary, pressed) { return true; }
        handle_primary_key(&primary, pressed);
    }
    false
}

/// Modifier-class flag bit for a side-modifier keycode, or `None` if the keycode
/// is not a known side modifier.
fn class_mask_for_keycode(keycode: i64) -> Option<u64> {
    match keycode {
        55 | 54 => Some(FLAG_MASK_COMMAND),   // Cmd left / right
        59 | 62 => Some(FLAG_MASK_CONTROL),   // Ctrl left / right
        58 | 61 => Some(FLAG_MASK_ALTERNATE), // Alt/Option left / right
        56 | 60 => Some(FLAG_MASK_SHIFT),     // Shift left / right
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
pub fn resolve_side_pressed_from_flags(keycode: i64, flags: u64) -> Option<bool> {
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

pub fn side_from_keycode(keycode: i64) -> Option<SideModifier> {
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
