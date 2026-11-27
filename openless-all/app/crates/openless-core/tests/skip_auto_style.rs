use openless_core::shared_types::ShortcutBinding;
use openless_core::{bindings_overlap, reconcile_hotkey_collisions, reject_hotkey_collisions};
use openless_core::{HotkeyRuntimeTarget, UserPreferences};

#[test]
fn skip_auto_style_preferences_upgrade_clear_patch_and_roundtrip() {
    let defaults = UserPreferences::default();
    let mut old = serde_json::to_value(&defaults).unwrap();
    old.as_object_mut().unwrap().remove("skipAutoStyleHotkey");
    old.as_object_mut()
        .unwrap()
        .remove("skipAutoStylePickerEnabled");
    let upgraded: UserPreferences = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(
        upgraded.skip_auto_style_hotkey,
        defaults.skip_auto_style_hotkey
    );
    assert!(!upgraded.skip_auto_style_picker_enabled);
    // Exercise collision migration even where the new default is unassigned.
    let proposed = if cfg!(target_os = "windows") {
        ShortcutBinding { primary: "LeftAlt".into(), modifiers: vec![] }
    } else {
        defaults.skip_auto_style_hotkey.clone()
    };
    old["dictationHotkey"] = serde_json::to_value(&proposed).unwrap();
    let occupied: UserPreferences = serde_json::from_value(old).unwrap();
    assert!(
        occupied.skip_auto_style_hotkey.is_unassigned(),
        "upgrade must not steal a custom recording key"
    );
    let mut configured = defaults.clone();
    configured.skip_auto_style_hotkey = ShortcutBinding {
        primary: "K".into(), modifiers: vec!["ctrl".into(), "shift".into()],
    };
    let edits = std::collections::BTreeMap::from([
        (
            "/skipAutoStyleHotkey".into(),
            serde_json::json!({"primary":"","modifiers":[]}),
        ),
        (
            "/skipAutoStylePickerEnabled".into(),
            serde_json::json!(true),
        ),
    ]);
    let patched = openless_core::preference_patch::patch_preferences(&configured, &edits).unwrap();
    let restored: UserPreferences =
        serde_json::from_value(serde_json::to_value(&patched).unwrap()).unwrap();
    assert!(restored.skip_auto_style_hotkey.is_unassigned());
    assert!(restored.skip_auto_style_picker_enabled);
    assert_ne!(
        HotkeyRuntimeTarget::from(&restored),
        HotkeyRuntimeTarget::from(&configured)
    );
    let change = openless_core::SettingsEffectPlan::between(&configured, &restored)
        .hotkeys
        .unwrap();
    assert_eq!(
        change.previous.skip_auto_style,
        configured.skip_auto_style_hotkey
    );
    assert!(change.next.skip_auto_style.is_unassigned());
}

#[test]
fn skip_auto_style_defaults_and_conflicts_cover_every_owner() {
    let defaults = UserPreferences::default();
    assert_eq!(
        defaults.skip_auto_style_hotkey.primary,
        if cfg!(target_os = "macos") {
            "LeftCommand"
        } else if cfg!(target_os = "windows") {
            ""
        } else {
            "LeftAlt"
        }
    );
    reject_hotkey_collisions(&defaults).unwrap();
    for owner in 0..10 {
        let mut prefs = defaults.clone();
        // Use a supported, assigned combination on every platform.
        prefs.skip_auto_style_hotkey = ShortcutBinding {
            primary: "K".into(), modifiers: vec!["ctrl".into(), "shift".into()],
        };
        let key = prefs.skip_auto_style_hotkey.clone();
        match owner {
            0 => prefs.dictation_hotkey = key,
            1 => prefs.translation_hotkey = key,
            2 => prefs.qa_hotkey = Some(key),
            3 => prefs.switch_style_hotkey = Some(key),
            4 => prefs.open_app_hotkey = Some(key),
            5 => prefs.coding_agent_voice_hotkey = Some(key),
            6 => prefs.selection_polish_hotkey = Some(key),
            7 => prefs.quick_note_hotkey = Some(key),
            8 => prefs.coding_agent_panel_hotkey = Some(key),
            _ => prefs.coding_agent_quick_hotkey = Some(key),
        }
        assert!(reject_hotkey_collisions(&prefs).is_err(), "owner {owner}");
    }
    let mut conflicting = defaults.clone();
    conflicting.skip_auto_style_hotkey = defaults.dictation_hotkey.clone();
    assert_eq!(reconcile_hotkey_collisions(&mut conflicting, &defaults), 1);
    assert_eq!(
        conflicting.skip_auto_style_hotkey,
        defaults.skip_auto_style_hotkey
    );
    let mut disabled = defaults.clone();
    disabled.skip_auto_style_hotkey = ShortcutBinding::unassigned();
    assert_eq!(reconcile_hotkey_collisions(&mut disabled, &defaults), 0);
    let left_alt = ShortcutBinding {
        primary: "LeftAlt".into(),
        modifiers: vec![],
    };
    let right_alt = ShortcutBinding {
        primary: "RightAlt".into(),
        modifiers: vec![],
    };
    assert!(
        !bindings_overlap(&left_alt, &right_alt),
        "Windows menu key and selection polish must use distinct physical sides"
    );
    let mut shift = defaults;
    shift.skip_auto_style_hotkey = ShortcutBinding {
        primary: "LeftShift".into(),
        modifiers: vec![],
    };
    assert!(reject_hotkey_collisions(&shift).is_err());
    let mut style_conflict = UserPreferences {
        skip_auto_style_hotkey: ShortcutBinding {
            primary: "K".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
        },
        ..UserPreferences::default()
    };
    style_conflict
        .style_pack_hotkeys
        .push(openless_core::shared_types::StylePackHotkey {
            pack_id: "builtin.raw".into(),
            binding: style_conflict.skip_auto_style_hotkey.clone(),
        });
    assert!(reject_hotkey_collisions(&style_conflict).is_err());
}

#[cfg(target_os = "windows")]
#[test]
fn skip_auto_style_windows_migrates_and_rejects_unverified_modifier_taps() {
    for primary in ["LeftAlt", "RightAlt", "LeftOption", "LeftCommand", "LeftControl"] {
        let binding = ShortcutBinding { primary: primary.into(), modifiers: vec![] };
        let prefs = UserPreferences {
            skip_auto_style_hotkey: binding.clone(),
            ..UserPreferences::default()
        };
        assert!(reject_hotkey_collisions(&prefs).is_err(), "{primary}");
        let migrated: UserPreferences =
            serde_json::from_value(serde_json::to_value(&prefs).unwrap()).unwrap();
        assert!(migrated.skip_auto_style_hotkey.is_unassigned(), "{primary}");
    }
    let prefs = UserPreferences {
        skip_auto_style_hotkey: ShortcutBinding {
            primary: "K".into(), modifiers: vec!["ctrl".into(), "shift".into()],
        },
        ..UserPreferences::default()
    };
    reject_hotkey_collisions(&prefs).unwrap();
    let roundtrip: UserPreferences =
        serde_json::from_value(serde_json::to_value(&prefs).unwrap()).unwrap();
    assert_eq!(roundtrip.skip_auto_style_hotkey, prefs.skip_auto_style_hotkey);
}
