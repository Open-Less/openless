use openless_core::shared_types::{HotkeyMode, HotkeyTrigger, ShortcutBinding};
use openless_core::{
    binding_requires_mouse_hook, binding_requires_side_aware_hook, legacy_modifier_trigger,
    reconcile_hotkey_collisions, reject_hotkey_collisions, sync_dictation_hotkey_legacy_fields,
    validate_shortcut_binding, ShortcutBindingError, UserPreferences,
};

fn binding(primary: &str, modifiers: &[&str]) -> ShortcutBinding {
    ShortcutBinding {
        primary: primary.into(),
        modifiers: modifiers.iter().map(|modifier| (*modifier).into()).collect(),
    }
}

#[test]
fn scroll_lock_accepts_bare_generic_and_side_specific_bindings() {
    for primary in ["ScrollLock", " scrolllock ", "SCROLLLOCK"] {
        for modifiers in [vec![], vec!["ctrl"], vec!["ctrl-left"], vec!["ctrl-right"]] {
            let shortcut = binding(primary, &modifiers);
            assert!(validate_shortcut_binding(&shortcut).is_ok());
            assert!(legacy_modifier_trigger(&shortcut).is_none());
            assert!(!binding_requires_mouse_hook(&shortcut));
            assert_eq!(
                binding_requires_side_aware_hook(&shortcut),
                modifiers.iter().any(|modifier| modifier.contains('-'))
            );
        }
    }
    assert_eq!(
        validate_shortcut_binding(&binding("ScrollLock", &["hyper"])),
        Err(ShortcutBindingError::UnsupportedModifier("hyper".into()))
    );
    assert!(validate_shortcut_binding(&binding("ScrllLock", &[])).is_err());
}

#[test]
fn scroll_lock_preferences_round_trip_without_a_new_trigger_or_schema() {
    for mode in [HotkeyMode::Toggle, HotkeyMode::Hold, HotkeyMode::Auto] {
        let mut prefs = UserPreferences {
            dictation_hotkey: binding("ScrollLock", &[]),
            ..UserPreferences::default()
        };
        prefs.hotkey.mode = mode;
        sync_dictation_hotkey_legacy_fields(&mut prefs);
        assert_eq!(prefs.hotkey.trigger, HotkeyTrigger::Custom);
        let combo = prefs.custom_combo_hotkey.as_ref().unwrap();
        assert_eq!(combo.primary, "ScrollLock");
        assert!(combo.modifiers.is_empty());
        assert!(reject_hotkey_collisions(&prefs).is_ok());

        let bytes = serde_json::to_vec(&prefs).unwrap();
        let loaded: UserPreferences = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(loaded.dictation_hotkey, prefs.dictation_hotkey);
        assert_eq!(loaded.hotkey.mode, mode);
        assert_eq!(loaded.hotkey.trigger, HotkeyTrigger::Custom);
        assert_eq!(loaded.custom_combo_hotkey.unwrap().primary, "ScrollLock");

        let previous = prefs.clone();
        prefs.show_capsule = !prefs.show_capsule;
        reconcile_hotkey_collisions(&mut prefs, &previous);
        assert_eq!(prefs.dictation_hotkey.primary, "ScrollLock");
    }
}

#[test]
fn scroll_lock_keeps_existing_collision_checks() {
    let mut prefs = UserPreferences {
        dictation_hotkey: binding("ScrollLock", &[]),
        translation_hotkey: binding("ScrollLock", &[]),
        ..UserPreferences::default()
    };
    assert!(reject_hotkey_collisions(&prefs).is_err());
    prefs.translation_hotkey = binding("ScrollLock", &["ctrl"]);
    assert!(reject_hotkey_collisions(&prefs).is_ok());
}
