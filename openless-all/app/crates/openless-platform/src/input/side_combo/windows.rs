use super::*;

use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F13,
    VK_F14, VK_F15, VK_F16, VK_F17, VK_F18, VK_F19, VK_F2, VK_F20, VK_F3, VK_F4, VK_F5, VK_F6,
    VK_F7, VK_F8, VK_F9, VK_HOME, VK_INSERT, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7,
    VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS, VK_RCONTROL, VK_RETURN, VK_RIGHT,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SCROLL, VK_SPACE, VK_TAB, VK_UP,
};

pub fn dispatch_vk(vk_code: u32, pressed: bool) -> bool {
    if let Some(side) = side_from_vk(vk_code) {
        crate::input::hotkey::recording_companion_event(side.recording_key(), pressed);
        handle_side_modifier(side, pressed);
        return false;
    }
    if let Some(primary) = primary_from_vk(vk_code) {
        if crate::input::hotkey::recording_companion_event(&primary, pressed) { return true; }
        handle_primary_key(&primary, pressed);
    }
    false
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
        x if x == VK_SCROLL.0 as u32 => "ScrollLock",
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
        x if x == VK_F13.0 as u32 => "F13",
        x if x == VK_F14.0 as u32 => "F14",
        x if x == VK_F15.0 as u32 => "F15",
        x if x == VK_F16.0 as u32 => "F16",
        x if x == VK_F17.0 as u32 => "F17",
        x if x == VK_F18.0 as u32 => "F18",
        x if x == VK_F19.0 as u32 => "F19",
        x if x == VK_F20.0 as u32 => "F20",
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

#[cfg(test)]
mod tests {
    use super::*;
    use ::windows::Win32::UI::Input::KeyboardAndMouse::VK_CANCEL;

    #[test]
    fn scroll_lock_maps_to_the_canonical_primary() {
        assert_eq!(
            primary_from_vk(VK_SCROLL.0 as u32).as_deref(),
            Some("ScrollLock")
        );
    }

    #[test]
    fn scroll_lock_side_combo_pairs_edges_and_ignores_repeat() {
        let primary = primary_from_vk(VK_SCROLL.0 as u32).unwrap();
        let mut state = SideAwareComboState::new(ShortcutBinding {
            primary: "ScrollLock".into(),
            modifiers: vec!["ctrl-right".into()],
        });
        state.set_side(SideModifier::CtrlLeft, true);
        assert_eq!(state.on_primary(&primary, true), None);
        state.set_side(SideModifier::CtrlLeft, false);
        state.set_side(SideModifier::CtrlRight, true);
        let press_id = match state.on_primary(&primary, true) {
            Some(HotkeyEvent::Pressed { press_id, .. }) => press_id,
            other => panic!("expected ScrollLock Pressed, got {other:?}"),
        };
        assert_eq!(state.on_primary(&primary, true), None);
        assert!(matches!(
            state.on_primary(&primary, false),
            Some(HotkeyEvent::Released { press_id: released_id, .. }) if released_id == press_id
        ));
        assert_eq!(state.on_primary(&primary, false), None);
        assert!(matches!(
            state.on_primary(&primary, true),
            Some(HotkeyEvent::Pressed { .. })
        ));
        assert!(matches!(
            state.on_modifier_release(SideModifier::CtrlRight),
            Some(HotkeyEvent::Released { .. })
        ));
        assert_eq!(state.on_primary(&primary, false), None);
    }

    #[test]
    fn ctrl_scroll_lock_cancel_edges_keep_their_physical_primary() {
        let primary = primary_from_vk(normalize_windows_vk(VK_CANCEL.0 as u32, 0x46, false))
            .expect("physical ScrollLock primary");
        assert_eq!(primary, "ScrollLock");

        for (modifier, required, other) in [
            ("ctrl-left", SideModifier::CtrlLeft, SideModifier::CtrlRight),
            (
                "ctrl-right",
                SideModifier::CtrlRight,
                SideModifier::CtrlLeft,
            ),
        ] {
            for release_ctrl_first in [false, true] {
                let mut state = SideAwareComboState::new(ShortcutBinding {
                    primary: "ScrollLock".into(),
                    modifiers: vec![modifier.into()],
                });
                state.on_modifier_press(other);
                assert_eq!(state.on_primary(&primary, true), None);
                assert_eq!(state.on_primary(&primary, false), None);
                state.on_modifier_release(other);
                state.on_modifier_press(required);
                let press_id = match state.on_primary(&primary, true) {
                    Some(HotkeyEvent::Pressed { press_id, .. }) => press_id,
                    other => panic!("expected ScrollLock Pressed, got {other:?}"),
                };
                assert_eq!(state.on_primary(&primary, true), None);
                let released = if release_ctrl_first {
                    state.on_modifier_release(required)
                } else {
                    state.on_primary(&primary, false)
                };
                assert!(matches!(
                    released,
                    Some(HotkeyEvent::Released { press_id: released_id, .. })
                        if released_id == press_id
                ));
                // Once Ctrl is up, the same physical key can release as VK_SCROLL.
                let up = primary_from_vk(normalize_windows_vk(VK_SCROLL.0 as u32, 0x46, false))
                    .unwrap();
                assert_eq!(state.on_primary(&up, false), None);
                assert_eq!(state.on_modifier_release(required), None);
            }
        }
    }
}
