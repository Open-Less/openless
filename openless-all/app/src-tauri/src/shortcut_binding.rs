//! Shared parsing/validation for user-configurable shortcut bindings.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

#[cfg(test)]
use crate::types::HotkeyTrigger;
use crate::types::ShortcutBinding;

// 主键/组合键解析已迁到平台层，宿主只转发，避免多份实现。
pub use openless_platform::input::side_combo::keys::{parse_global_hotkey, parse_primary};

pub use openless_core::{
    binding_requires_mouse_hook, binding_requires_side_aware_hook, bindings_overlap,
    is_side_specific_modifier_tag, legacy_modifier_trigger, normalize_side_modifier_tag,
    reject_side_specific_non_dictation, ShortcutBindingError, SIDE_SPECIFIC_NON_DICTATION_MSG,
};

pub fn validate_binding(binding: &ShortcutBinding) -> Result<(), ShortcutBindingError> {
    openless_core::validate_shortcut_binding(binding)?;
    if binding_requires_mouse_hook(binding) {
        return if cfg!(target_os = "windows") {
            Ok(())
        } else {
            Err(ShortcutBindingError::UnsupportedKey(
                "Mouse4/Mouse5 global shortcuts require Windows".into(),
            ))
        };
    }
    #[cfg(target_os = "macos")]
    if binding.primary == crate::macos_dictation_key::PRIMARY {
        return Ok(());
    }
    if legacy_modifier_trigger(binding).is_some()
        || (binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift"))
        || binding_requires_side_aware_hook(binding)
    {
        return Ok(());
    }
    parse_global_hotkey(binding)?;
    Ok(())
}









#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_function_keys_parse_without_modifiers() {
        for (name, code) in [
            ("F13", Code::F13),
            ("F14", Code::F14),
            ("F15", Code::F15),
            ("F16", Code::F16),
            ("F17", Code::F17),
            ("F18", Code::F18),
            ("F19", Code::F19),
            ("F20", Code::F20),
        ] {
            let binding = ShortcutBinding {
                primary: name.into(),
                modifiers: vec![],
            };
            assert!(validate_binding(&binding).is_ok());
            let parsed = parse_global_hotkey(&binding).unwrap();
            assert_eq!(parsed.key, code);
            assert!(parsed.mods.is_empty());
            assert!(legacy_modifier_trigger(&binding).is_none());
        }
        assert_eq!(parse_primary(" f20 ").unwrap(), Code::F20);
        assert!(parse_primary("F21").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_dictation_key_is_bare_and_dictation_only() {
        let native = ShortcutBinding {
            primary: crate::macos_dictation_key::PRIMARY.into(),
            modifiers: vec![],
        };
        assert!(validate_binding(&native).is_ok());
        assert!(parse_global_hotkey(&native).is_err());
        assert!(reject_side_specific_non_dictation(&native).is_err());
        let modified = ShortcutBinding {
            modifiers: vec!["shift".into()],
            ..native
        };
        assert!(validate_binding(&modified).is_err());
    }

    #[test]
    fn parses_combo_and_single_key() {
        let combo = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        let parsed = parse_global_hotkey(&combo).expect("combo parses");
        #[cfg(target_os = "windows")]
        assert!(parsed.mods.contains(Modifiers::CONTROL));
        #[cfg(not(target_os = "windows"))]
        assert!(parsed.mods.contains(Modifiers::SUPER));
        assert!(parsed.mods.contains(Modifiers::SHIFT));
        assert_eq!(parsed.key, Code::KeyD);

        let single = ShortcutBinding {
            primary: "F8".into(),
            modifiers: vec![],
        };
        let parsed = parse_global_hotkey(&single).expect("single key parses");
        assert!(parsed.mods.is_empty());
        assert_eq!(parsed.key, Code::F8);
    }

    #[test]
    fn scroll_lock_uses_the_existing_custom_hotkey_path() {
        for primary in ["ScrollLock", " scrolllock ", "SCROLLLOCK"] {
            for modifiers in [vec![], vec!["ctrl".into()]] {
                let binding = ShortcutBinding {
                    primary: primary.into(),
                    modifiers,
                };
                assert!(validate_binding(&binding).is_ok());
                assert!(legacy_modifier_trigger(&binding).is_none());
                assert!(!binding_requires_side_aware_hook(&binding));
                let parsed = parse_global_hotkey(&binding).unwrap();
                assert_eq!(parsed.key, Code::ScrollLock);
                assert_eq!(
                    parsed.mods.contains(Modifiers::CONTROL),
                    !binding.modifiers.is_empty()
                );
            }
        }
        let binding = combo("ScrollLock", vec!["ctrl-right"]);
        assert!(validate_binding(&binding).is_ok());
        assert!(binding_requires_side_aware_hook(&binding));
    }

    #[test]
    fn detects_legacy_modifier_only() {
        let binding = ShortcutBinding {
            primary: "RightControl".into(),
            modifiers: vec![],
        };
        assert_eq!(
            legacy_modifier_trigger(&binding),
            Some(HotkeyTrigger::RightControl)
        );
        let left_cmd = ShortcutBinding {
            primary: "LeftCommand".into(),
            modifiers: vec![],
        };
        assert_eq!(
            legacy_modifier_trigger(&left_cmd),
            Some(HotkeyTrigger::LeftCommand)
        );
    }

    #[test]
    fn side_specific_combo_requires_side_aware_hook() {
        let binding = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd-left".into(), "shift-right".into()],
        };
        assert!(binding_requires_side_aware_hook(&binding));
        assert!(validate_binding(&binding).is_ok());
    }

    #[test]
    fn mouse_button_skips_global_hotkey_parse() {
        let binding = ShortcutBinding {
            primary: "Mouse4".into(),
            modifiers: vec![],
        };
        assert!(binding_requires_mouse_hook(&binding));
        assert_eq!(
            validate_binding(&binding).is_ok(),
            cfg!(target_os = "windows")
        );
        assert!(parse_global_hotkey(&binding).is_err());
    }

    #[test]
    fn mouse_with_ctrl_validates_without_registerhotkey() {
        let binding = ShortcutBinding {
            primary: "Mouse5".into(),
            modifiers: vec!["ctrl".into()],
        };
        assert!(binding_requires_mouse_hook(&binding));
        assert_eq!(
            validate_binding(&binding).is_ok(),
            cfg!(target_os = "windows")
        );
    }

    #[test]
    fn literal_space_primary_normalizes() {
        let binding = ShortcutBinding {
            primary: " ".into(),
            modifiers: vec!["ctrl".into()],
        };
        assert!(validate_binding(&binding).is_ok());
        let parsed = parse_global_hotkey(&binding).expect("space parses");
        assert_eq!(parsed.key, Code::Space);
    }

    #[test]
    fn generic_combo_uses_global_hotkey_path() {
        let binding = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        assert!(!binding_requires_side_aware_hook(&binding));
        assert!(parse_global_hotkey(&binding).is_ok());
    }

    #[test]
    fn super_side_aliases_normalize_to_cmd() {
        assert_eq!(normalize_side_modifier_tag("super-left"), "cmd-left");
        assert_eq!(normalize_side_modifier_tag("Super-Right"), "cmd-right");
        assert!(is_side_specific_modifier_tag("super-left"));
    }

    #[test]
    fn accepts_shifted_printable_aliases() {
        let cases = [
            ("?", Code::Slash),
            ("!", Code::Digit1),
            (":", Code::Semicolon),
            ("+", Code::Equal),
            ("_", Code::Minus),
            ("{", Code::BracketLeft),
            ("|", Code::Backslash),
        ];
        for (primary, expected) in cases {
            let binding = ShortcutBinding {
                primary: primary.into(),
                modifiers: vec!["shift".into()],
            };
            let parsed = parse_global_hotkey(&binding).expect("shifted printable parses");
            assert_eq!(parsed.key, expected);
            assert!(parsed.mods.contains(Modifiers::SHIFT));
        }
    }

    fn combo(primary: &str, modifiers: Vec<&str>) -> ShortcutBinding {
        ShortcutBinding {
            primary: primary.into(),
            modifiers: modifiers.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn side_combo_overlaps_generic_combo_with_same_physical_modifiers() {
        #[cfg(target_os = "windows")]
        {
            assert!(!bindings_overlap(
                &combo("D", vec!["cmd-left"]),
                &combo("D", vec!["cmd"]),
            ));
            assert!(bindings_overlap(
                &combo("D", vec!["cmd-left"]),
                &combo("D", vec!["super"]),
            ));
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(bindings_overlap(
                &combo("D", vec!["cmd-left"]),
                &combo("D", vec!["cmd"]),
            ));
        }
    }

    #[test]
    fn side_combo_does_not_overlap_generic_combo_with_extra_modifier() {
        assert!(!bindings_overlap(
            &combo("D", vec!["cmd-left"]),
            &combo("D", vec!["cmd", "shift"]),
        ));
    }

    #[test]
    fn side_combos_with_different_sides_do_not_overlap() {
        assert!(!bindings_overlap(
            &combo("D", vec!["cmd-left"]),
            &combo("D", vec!["cmd-right"]),
        ));
    }

    #[test]
    fn rejects_side_specific_non_dictation_shortcut() {
        let binding = combo("D", vec!["cmd-left"]);
        assert!(binding_requires_side_aware_hook(&binding));
        assert_eq!(
            reject_side_specific_non_dictation(&binding).unwrap_err(),
            SIDE_SPECIFIC_NON_DICTATION_MSG,
        );
        assert!(reject_side_specific_non_dictation(&combo("D", vec!["cmd"])).is_ok());
    }
}
