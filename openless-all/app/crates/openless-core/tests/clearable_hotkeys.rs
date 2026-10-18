use openless_core::shared_types::ShortcutBinding;
use openless_core::{
    reconcile_hotkey_collisions, reject_hotkey_collisions, sync_dictation_hotkey_legacy_fields,
    UserPreferences,
};

#[test]
fn clearing_required_legacy_shortcuts_survives_save_and_reload() {
    let mut prefs = UserPreferences {
        dictation_hotkey: ShortcutBinding::unassigned(),
        translation_hotkey: ShortcutBinding::unassigned(),
        ..Default::default()
    };
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    assert!(reject_hotkey_collisions(&prefs).is_ok());
    assert!(prefs.custom_combo_hotkey.is_none());
    let bytes = serde_json::to_vec(&prefs).unwrap();
    let loaded: UserPreferences = serde_json::from_slice(&bytes).unwrap();
    assert!(loaded.dictation_hotkey.is_unassigned());
    assert!(loaded.translation_hotkey.is_unassigned());
    assert!(loaded.custom_combo_hotkey.is_none());
}

#[test]
fn unrelated_preference_updates_do_not_restore_cleared_shortcuts() {
    let previous = UserPreferences::default();
    let mut next = previous.clone();
    next.dictation_hotkey = ShortcutBinding::unassigned();
    next.translation_hotkey = ShortcutBinding::unassigned();
    next.show_capsule = !next.show_capsule;
    sync_dictation_hotkey_legacy_fields(&mut next);
    reconcile_hotkey_collisions(&mut next, &previous);
    assert!(next.dictation_hotkey.is_unassigned());
    assert!(next.translation_hotkey.is_unassigned());
    assert!(reject_hotkey_collisions(&next).is_ok());
}

#[test]
fn missing_preferences_keep_first_install_defaults() {
    let loaded: UserPreferences = serde_json::from_str("{}").unwrap();
    assert!(!loaded.dictation_hotkey.is_unassigned());
    assert!(!loaded.translation_hotkey.is_unassigned());
}

#[test]
fn clearing_a_key_does_not_relax_validation_of_new_recorded_bindings() {
    assert!(openless_core::validate_shortcut_binding(&ShortcutBinding::unassigned()).is_err());
    let assigned = ShortcutBinding {
        primary: "F8".into(),
        modifiers: vec![],
    };
    assert!(!openless_core::bindings_overlap(
        &assigned,
        &ShortcutBinding::unassigned()
    ));
    assert!(!openless_core::bindings_overlap(
        &ShortcutBinding::unassigned(),
        &ShortcutBinding::unassigned()
    ));
}
