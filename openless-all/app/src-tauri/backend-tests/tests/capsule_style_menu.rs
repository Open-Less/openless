//! Production picker protocol plus a real Core session, without native UI/audio.
#![allow(dead_code)]
#[path = "../../src/capsule_style_menu.rs"]
mod capsule_style_menu;

#[tokio::test]
async fn ordinary_skip_start_and_programmatic_switch_never_create_hotkey_open_intent() {
    use std::sync::Arc;
    use capsule_style_menu as menu;
    use openless_core::{BackendConfig, BackendDependencies, DictationStartOptions,
        DictationStyleSelection as Selection, OpenLessBackend};
    let directory = tempfile::tempdir().unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.host_actions = Arc::new(openless_core::testing::RecordingHostActions::default());
    dependencies.dictation_engine = Arc::new(openless_core::testing::FixtureDictationEngine::successful("", ""));
    dependencies.text_inserter = Arc::new(openless_core::testing::FixtureTextInserter::with_outcome(
        openless_core::InsertOutcome::Inserted,
    ));
    let backend = Arc::new(OpenLessBackend::new(BackendConfig {
        data_dir: directory.path().to_owned(), ..Default::default()
    }, dependencies).unwrap());
    backend.start().await.unwrap();
    let mut prefs = backend.get_preferences();
    prefs.skip_auto_style_picker_enabled = true;
    backend.update_settings(prefs, openless_core::SettingsUpdateOptions::STRICT,
        &openless_core::NoopSettingsRuntime).unwrap();
    let source = Arc::downgrade(&backend);
    menu::configure(Arc::new(move || {
        let core = source.upgrade()?;
        if !matches!(core.snapshot().dictation.phase, openless_core::DictationPhase::Starting
            | openless_core::DictationPhase::Recording) { return None; }
        let target = core.dictation_style_target()?;
        Some(menu::Context { session_id: target.session_id, mutable: target.mutable,
            skip_auto: target.skip_auto_style, picker_enabled: true, keyboard_available: true,
            focus: Some(menu::Focus(1, 1)), blocked_keys: [false; 4] })
    }), Arc::new(|_| true));
    let session = backend.start_dictation_with_options(DictationStartOptions {
        skip_auto_style: true, ..Default::default()
    }).await.unwrap();
    menu::refresh();
    assert!(backend.dictation_style_target().unwrap().skip_auto_style);
    assert!(menu::pending("capsule").is_none(), "ordinary skipAuto start has no hotkey provenance");
    backend.update_dictation_style(session, Selection::Automatic).unwrap();
    backend.update_dictation_style(session, Selection::SkipAuto).unwrap();
    menu::refresh();
    assert!(menu::pending("capsule").is_none(), "programmatic switches have no hotkey provenance");
    let automatic = backend.toggle_dictation_style_for_session(session).unwrap();
    assert!(menu::hotkey_toggled(&automatic).is_none());
    let skipped = backend.toggle_dictation_style_for_session(session).unwrap();
    let request = menu::hotkey_toggled(&skipped).unwrap();
    let owner = menu::reservation("capsule", session, Some(&request.request_id)).unwrap();
    menu::commit(owner.clone(), Some(&request.request_id)).unwrap();
    assert!(menu::owner_current(&owner));
    backend.cancel_dictation(Some(session)).await.unwrap();
    menu::refresh();
    assert!(!menu::owner_current(&owner));
    assert!(menu::pending("capsule").is_none());
    menu::invalidate();
    menu::configure(Arc::new(|| None), Arc::new(|_| false));
    backend.shutdown().await.unwrap();
}
