use openless_core::{CapsuleDisplayMode, UserPreferences};

#[test]
fn legacy_preferences_default_to_cursor_display_without_extra_windows() {
    let prefs: UserPreferences = serde_json::from_str("{}").unwrap();
    assert_eq!(prefs.capsule_display_mode, CapsuleDisplayMode::CursorScreen);
    assert_eq!(
        UserPreferences::default().capsule_display_mode,
        CapsuleDisplayMode::CursorScreen
    );
}

#[test]
fn both_display_modes_survive_save_reload_and_unrelated_preference_changes() {
    for (mode, wire) in [
        (CapsuleDisplayMode::CursorScreen, "cursorScreen"),
        (CapsuleDisplayMode::AllScreens, "allScreens"),
    ] {
        let prefs = UserPreferences {
            capsule_display_mode: mode,
            ..Default::default()
        };
        let value = serde_json::to_value(prefs).unwrap();
        assert_eq!(value["capsuleDisplayMode"], wire);
        let mut reloaded: UserPreferences = serde_json::from_value(value).unwrap();
        reloaded.show_capsule = false;
        let saved = serde_json::to_vec(&reloaded).unwrap();
        let restored: UserPreferences = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.capsule_display_mode, mode);
        assert!(!restored.show_capsule);
    }
}

#[test]
fn unknown_display_mode_is_not_silently_accepted() {
    assert!(
        serde_json::from_str::<UserPreferences>(r#"{"capsuleDisplayMode":"sometimes"}"#).is_err()
    );
}
