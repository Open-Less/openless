use openless_core::{CapsuleDisplayMode, UserPreferences};

#[test]
fn legacy_preferences_default_to_cursor_display_without_extra_windows() {
    let prefs: UserPreferences = serde_json::from_str("{}").unwrap();
    assert_eq!(prefs.capsule_display_mode, CapsuleDisplayMode::CursorScreen);
    assert!(prefs.capsule_display_mode_v2.is_none());
    assert!(serde_json::to_value(&prefs)
        .unwrap()
        .get("capsuleDisplayModeV2")
        .is_none());
    assert_eq!(
        UserPreferences::default().capsule_display_mode,
        CapsuleDisplayMode::CursorScreen
    );
}

#[test]
fn all_display_modes_survive_save_reload_and_unrelated_preference_changes() {
    for (mode, wire) in [
        (CapsuleDisplayMode::CursorScreen, "cursorScreen"),
        (CapsuleDisplayMode::PrimaryScreen, "primaryScreen"),
        (
            CapsuleDisplayMode::ForegroundWindowScreen,
            "foregroundWindowScreen",
        ),
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
    assert!(
        serde_json::from_str::<UserPreferences>(r#"{"capsuleDisplayModeV2":"sometimes"}"#).is_err()
    );
}

#[test]
fn v2_is_authoritative_and_pr_legacy_new_modes_migrate_without_repositioning() {
    for mode in [
        "cursorScreen",
        "primaryScreen",
        "foregroundWindowScreen",
        "allScreens",
    ] {
        let preferences: UserPreferences = serde_json::from_value(serde_json::json!({
            "capsuleDisplayMode":"allScreens", "capsuleDisplayModeV2":mode
        }))
        .unwrap();
        let value = serde_json::to_value(&preferences).unwrap();
        assert_eq!(value["capsuleDisplayMode"], mode);
        assert_eq!(value["capsuleDisplayModeV2"], mode);
    }
    for mode in ["primaryScreen", "foregroundWindowScreen"] {
        let preferences: UserPreferences =
            serde_json::from_value(serde_json::json!({"capsuleDisplayMode":mode})).unwrap();
        assert_eq!(
            serde_json::to_value(&preferences).unwrap()["capsuleDisplayModeV2"],
            mode
        );
    }
}
