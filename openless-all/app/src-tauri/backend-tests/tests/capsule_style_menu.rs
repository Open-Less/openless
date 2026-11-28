//! Production picker protocol plus a real Core session, without native UI/audio.
#![allow(dead_code)]
#[path = "../../src/capsule_style_menu.rs"]
mod capsule_style_menu;

use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};
use capsule_style_menu as menu;
use openless_core::{BackendConfig, BackendDependencies, DictationStartOptions,
    DictationStyleSelection as Selection, OpenLessBackend, SessionId};

fn actual_toggle(core: &OpenLessBackend, session: SessionId) -> (openless_core::DictationStyleTarget, u64) {
    // Exactly the atomic Core mutation/receipt used by the native coordinator.
    core.toggle_dictation_style_with_epoch(session).unwrap()
}
fn request(core: &OpenLessBackend, session: SessionId) -> (menu::Request, openless_core::DictationStyleTarget, u64) {
    core.update_dictation_style(session, Selection::Automatic).unwrap();
    let (target, epoch) = actual_toggle(core, session);
    (menu::hotkey_toggled(&target, epoch).unwrap(), target, epoch)
}
fn open(request: &menu::Request) -> menu::Owner {
    let owner = menu::reservation("capsule", request.session_id, Some(&request.request_id)).unwrap();
    menu::commit(owner.clone(), Some(&request.request_id)).unwrap();
    owner
}
fn automatic_skip(core: &OpenLessBackend, session: SessionId) {
    // No refresh, menu callback, or broadcast consumption between these calls.
    core.update_dictation_style(session, Selection::Automatic).unwrap();
    core.update_dictation_style(session, Selection::SkipAuto).unwrap();
}
fn delayed_events(events: &mut openless_core::EventSubscription) -> Vec<openless_core::BackendEvent> {
    let mut delayed = Vec::new();
    loop {
        match events.try_recv() {
            Ok(event) => delayed.push(event),
            Err(openless_core::EventRecvError::Empty) => break,
            Err(error) => panic!("unexpected event replay failure: {error}"),
        }
    }
    delayed
}
fn deliver(events: &[openless_core::BackendEvent]) {
    for event in events {
        if matches!(&event.kind, openless_core::BackendEventKind::DictationStateChanged(_)
            | openless_core::BackendEventKind::PreferencesChanged(_)) {
            // Production event handling refreshes from Core, never the payload.
            menu::refresh();
        }
    }
}
fn set_picker(core: &OpenLessBackend, enabled: bool) {
    let mut prefs = core.get_preferences();
    prefs.skip_auto_style_picker_enabled = enabled;
    core.update_settings(prefs, openless_core::SettingsUpdateOptions::STRICT,
        &openless_core::NoopSettingsRuntime).unwrap();
}

// One fixture owns the process-global production Runtime throughout these real
// Core sequences; the module's other tests use independent, local State values.
#[tokio::test]
async fn real_core_style_mutations_fence_pending_prepared_active_and_late_host_delivery() {
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
    set_picker(&backend, true);
    let source = Arc::downgrade(&backend);
    let fence = Arc::downgrade(&backend);
    let disturb = Arc::new(AtomicUsize::new(0));
    let source_disturb = disturb.clone();
    let output = Arc::new(Mutex::new(Vec::new()));
    let sink = output.clone();
    menu::configure_fenced(Arc::new(move || {
        let core = source.upgrade()?;
        if !matches!(core.snapshot().dictation.phase, openless_core::DictationPhase::Starting
            | openless_core::DictationPhase::Recording) { return None; }
        let (target, style_epoch, picker_enabled) = core.dictation_style_target_with_epoch()?;
        let disturbance = source_disturb.load(Ordering::SeqCst);
        if disturbance != 0 {
            if disturbance == 1 { source_disturb.store(0, Ordering::SeqCst); }
            // A real Core transition occurs after taking a view, just like an
            // asynchronous native focus query crossing an API transaction.
            automatic_skip(&core, target.session_id);
        }
        Some(menu::Context { session_id: target.session_id, style_epoch, mutable: target.mutable,
            skip_auto: target.skip_auto_style, picker_enabled, keyboard_available: true,
            focus: Some(menu::Focus(1, 1)), blocked_keys: [false; 4] })
    }), Arc::new(move |event| { sink.lock().unwrap().push(event); true }),
        Arc::new(move || fence.upgrade().unwrap().dictation_style_epoch()));
    let session = backend.start_dictation_with_options(DictationStartOptions {
        skip_auto_style: true, ..Default::default()
    }).await.unwrap();
    menu::refresh();
    assert!(menu::pending("capsule").is_none(), "ordinary skipAuto start has no hotkey provenance");
    automatic_skip(&backend, session);
    assert!(menu::pending("capsule").is_none(), "programmatic changes alone cannot open a menu");
    let unchanged = backend.dictation_style_epoch();
    backend.update_dictation_style(session, Selection::SkipAuto).unwrap();
    assert_eq!(unchanged, backend.dictation_style_epoch(), "an idempotent write is not a style transition");

    // R1: r1 exists, but Host observes neither intermediate Core frame. Reading
    // latest SkipAuto=true must not resurrect the previous request or token.
    let (r1, old_target, old_epoch) = request(&backend, session);
    let mut events = backend.subscribe();
    automatic_skip(&backend, session);
    assert!(backend.dictation_style_epoch() > old_epoch);
    assert!(menu::pending("capsule").is_none());
    assert_eq!(menu::reservation("capsule", session, Some(&r1.request_id)).unwrap_err(),
        "capsule_style_request_stale");
    assert!(!menu::key(menu::Key::ArrowDown, true, false, false));
    assert!(!menu::key(menu::Key::ArrowDown, false, false, false));
    assert!(menu::hotkey_toggled(&old_target, old_epoch).is_none());
    let delayed = delayed_events(&mut events);
    assert!(delayed.iter().any(|event| matches!(&event.kind,
        openless_core::BackendEventKind::DictationStateChanged(state) if !state.skip_auto_style)));
    assert!(delayed.iter().any(|event| matches!(&event.kind,
        openless_core::BackendEventKind::DictationStateChanged(state) if state.skip_auto_style)));
    deliver(&delayed);
    assert!(menu::pending("capsule").is_none());
    // Also reject directly at prepare, before any getter/refresh cleans up r1.
    let (stale_prepare, _, _) = request(&backend, session);
    automatic_skip(&backend, session);
    assert_eq!(menu::reservation("capsule", session, Some(&stale_prepare.request_id)).unwrap_err(),
        "capsule_style_request_stale");

    // Geometry reservation completed, but native commit is delayed across the
    // same real Core transitions. The old reservation must not gain ownership.
    let (prepared_request, _, _) = request(&backend, session);
    let prepared = menu::reservation("capsule", session, Some(&prepared_request.request_id)).unwrap();
    automatic_skip(&backend, session);
    assert_eq!(menu::commit(prepared, Some(&prepared_request.request_id)).unwrap_err(),
        "capsule_style_menu_reservation_stale");
    assert!(menu::pending("capsule").is_none());
    assert!(!menu::key(menu::Key::Enter, true, false, false));
    assert!(!menu::key(menu::Key::Enter, false, false, false));

    // An active owner is fenced on the very next key/owner check, without first
    // consuming a state event. Retain captured releases, never steal passed ups.
    let (active_request, _, _) = request(&backend, session);
    let active = open(&active_request);
    assert!(menu::key(menu::Key::Enter, true, false, false));
    assert!(!menu::key(menu::Key::Escape, true, false, true));
    let old_key = output.lock().unwrap().iter().find_map(|event| match event {
        menu::Event::Key(key) if key.menu_id == active.menu_id => Some(key.clone()), _ => None,
    }).unwrap();
    automatic_skip(&backend, session);
    assert!(!menu::key(menu::Key::ArrowUp, true, false, false));
    assert!(!menu::key(menu::Key::ArrowUp, false, false, false));
    assert!(!menu::owner_current(&active));
    assert!(!menu::is_owner(&old_key));
    assert!(menu::key(menu::Key::Enter, false, false, false));
    assert!(!menu::key(menu::Key::Escape, false, false, false));

    let (automatic_request, _, _) = request(&backend, session);
    let automatic_owner = open(&automatic_request);
    backend.update_dictation_style(session, Selection::Automatic).unwrap();
    assert!(!menu::owner_current(&automatic_owner));
    assert!(!menu::key(menu::Key::ArrowUp, true, false, false));
    assert!(!menu::key(menu::Key::ArrowUp, false, false, false));
    backend.update_dictation_style(session, Selection::SkipAuto).unwrap();

    // A new *real mutation receipt* can open again; old broadcasts and both
    // kinds of old hotkey receipt must not cancel or reopen that newer owner.
    let (old_auto, old_auto_epoch) = actual_toggle(&backend, session);
    let (new_skip, new_epoch) = actual_toggle(&backend, session);
    let current_request = menu::hotkey_toggled(&new_skip, new_epoch).unwrap();
    let current = open(&current_request);
    set_picker(&backend, true);
    assert_eq!(new_epoch, backend.dictation_style_epoch(), "unchanged picker settings retain the current epoch");
    deliver(&delayed);
    let mut old_automatic_frame = delayed.iter().find_map(|event| match &event.kind {
        openless_core::BackendEventKind::DictationStateChanged(state) if !state.skip_auto_style =>
            Some(state.clone()),
        _ => None,
    }).unwrap();
    let old_phase = old_automatic_frame.phase;
    let old_translation = old_automatic_frame.translation_active;
    menu::project_current_style(&mut old_automatic_frame, &backend.snapshot().dictation);
    assert!(old_automatic_frame.skip_auto_style, "a late app frame cannot hide the current style badge");
    assert_eq!(old_automatic_frame.phase, old_phase);
    assert_eq!(old_automatic_frame.translation_active, old_translation);
    assert!(menu::hotkey_toggled(&old_auto, old_auto_epoch).is_none());
    assert!(menu::hotkey_toggled(&old_target, old_epoch).is_none());
    assert!(menu::owner_current(&current));
    assert!(!menu::close("capsule", session, &active.menu_id));
    assert!(menu::owner_current(&current));
    assert!(menu::key(menu::Key::ArrowDown, true, false, false));
    assert!(menu::key(menu::Key::ArrowDown, false, false, false));
    assert!(menu::close("capsule", session, &current.menu_id));
    assert!(menu::hotkey_toggled(&new_skip, new_epoch).is_none(), "closed receipt cannot reopen");
    assert!(menu::pending("capsule").is_none());

    // A stale native-context snapshot is rejected against the cheap Core fence;
    // continuous changes decline fresh keys but still drain existing pairs.
    let (racing_request, _, _) = request(&backend, session);
    disturb.store(1, Ordering::SeqCst);
    assert!(menu::pending("capsule").is_none());
    assert_eq!(menu::reservation("capsule", session, Some(&racing_request.request_id)).unwrap_err(),
        "capsule_style_request_stale");
    let (unstable_request, _, _) = request(&backend, session);
    let unstable_owner = open(&unstable_request);
    assert!(menu::key(menu::Key::Enter, true, false, false));
    assert!(!menu::key(menu::Key::Escape, true, false, true));
    disturb.store(2, Ordering::SeqCst);
    assert!(!menu::key(menu::Key::ArrowUp, true, false, false));
    assert!(menu::key(menu::Key::Enter, false, false, false));
    assert!(!menu::key(menu::Key::Escape, false, false, false));
    disturb.store(0, Ordering::SeqCst);
    assert!(!menu::owner_current(&unstable_owner));
    assert!(!menu::key(menu::Key::ArrowUp, false, false, false));

    // Turning the picker off/on is also remembered even with delayed settings
    // events, in pending, prepared and active stages of the real protocol.
    for stage in 0..4 {
        let (r, _, _) = request(&backend, session);
        let owner = (stage != 0).then(|| menu::reservation("capsule", session, Some(&r.request_id)).unwrap());
        if stage >= 2 {
            menu::commit(owner.clone().unwrap(), Some(&r.request_id)).unwrap();
            assert!(menu::key(menu::Key::Enter, true, false, false));
            assert!(!menu::key(menu::Key::Escape, true, false, true));
        }
        set_picker(&backend, false);
        if stage == 3 {
            assert!(!menu::key(menu::Key::ArrowUp, true, false, false));
            assert!(!menu::key(menu::Key::ArrowUp, false, false, false));
        }
        set_picker(&backend, true);
        assert!(menu::pending("capsule").is_none());
        assert!(!menu::key(menu::Key::ArrowUp, true, false, false));
        assert!(!menu::key(menu::Key::ArrowUp, false, false, false));
        assert_eq!(menu::reservation("capsule", session, Some(&r.request_id)).unwrap_err(),
            "capsule_style_request_stale");
        if let Some(owner) = owner {
            assert!(!menu::owner_current(&owner));
            assert_eq!(menu::commit(owner, Some(&r.request_id)).unwrap_err(),
                "capsule_style_menu_reservation_stale");
        }
        if stage >= 2 {
            assert!(menu::key(menu::Key::Enter, false, false, false));
            assert!(!menu::key(menu::Key::Escape, false, false, false));
        }
    }

    // Cancellation removes qualification, with the same retained pair rules.
    let (last, last_target, last_epoch) = request(&backend, session);
    let last_owner = open(&last);
    assert!(menu::key(menu::Key::Enter, true, false, false));
    assert!(!menu::key(menu::Key::Escape, true, false, true));
    let before_cancel = backend.dictation_style_epoch();
    backend.cancel_dictation(Some(session)).await.unwrap();
    assert!(backend.dictation_style_epoch() > before_cancel, "session retirement fences in-flight native views");
    assert_eq!(menu::reservation("capsule", session, Some(&last.request_id)).unwrap_err(),
        "capsule_style_request_stale");
    assert!(!menu::owner_current(&last_owner));
    assert!(menu::key(menu::Key::Enter, false, false, false));
    assert!(!menu::key(menu::Key::Escape, false, false, false));
    assert!(!menu::key(menu::Key::ArrowUp, true, false, false));
    assert!(!menu::key(menu::Key::ArrowUp, false, false, false));
    let next = backend.start_dictation_with_options(DictationStartOptions::default()).await.unwrap();
    let (next_request, _, _) = request(&backend, next);
    let next_owner = open(&next_request);
    let ended_session_events = delayed_events(&mut events);
    deliver(&ended_session_events);
    assert!(menu::hotkey_toggled(&last_target, last_epoch).is_none());
    assert!(!menu::close("capsule", session, &last_owner.menu_id));
    assert!(menu::owner_current(&next_owner));
    backend.cancel_dictation(Some(next)).await.unwrap();
    menu::refresh();
    assert!(!menu::owner_current(&next_owner));
    menu::invalidate();
    menu::configure(Arc::new(|| None), Arc::new(|_| false));
    backend.shutdown().await.unwrap();
}
