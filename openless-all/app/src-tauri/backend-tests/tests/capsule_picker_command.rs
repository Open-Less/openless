//! Run the command's production admission helper before the real reservation.
#![allow(dead_code)]
#[path = "../../src/capsule_style_menu.rs"]
mod capsule_style_menu;
#[path = "../../src/commands/capsule_picker.rs"]
mod capsule_picker;

use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use capsule_style_menu as menu;
use openless_core::{BackendConfig, BackendDependencies, DictationStartOptions,
    OpenLessBackend, SessionId};

struct PendingContext {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl openless_core::HostContextAdapter for PendingContext {
    fn capture(&self, _: bool) -> futures_util::future::BoxFuture<'static,
        Result<openless_core::HostContextCapture, openless_core::BackendError>> {
        let entered = self.entered.clone();
        let release = self.release.clone();
        Box::pin(async move {
            entered.notify_one();
            release.notified().await;
            Ok(openless_core::HostContextCapture::default())
        })
    }
}

fn style_open(
    core: &OpenLessBackend, session: SessionId, request: Option<&str>, geometry: &AtomicUsize,
) -> Result<menu::Owner, String> {
    capsule_picker::dispatch(core, session, true, true, || {
        let owner = menu::reservation("capsule", session, request)?;
        // Same ordering as set_style_menu_open: no native geometry before
        // successful reservation; commit only after geometry succeeds.
        geometry.fetch_add(1, Ordering::SeqCst);
        menu::commit(owner.clone(), request)?;
        Ok(owner)
    })
}

fn translation(
    core: &OpenLessBackend, session: SessionId, open: bool, geometry: &AtomicUsize,
) -> Result<(), String> {
    capsule_picker::dispatch(core, session, open, false, || {
        geometry.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
}

#[tokio::test]
async fn command_precheck_classifies_old_style_requests_before_geometry() {
    let directory = tempfile::tempdir().unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.host_actions = Arc::new(openless_core::testing::RecordingHostActions::default());
    dependencies.dictation_engine = Arc::new(openless_core::testing::FixtureDictationEngine::successful("", ""));
    dependencies.text_inserter = Arc::new(openless_core::testing::FixtureTextInserter::with_outcome(
        openless_core::InsertOutcome::Inserted,
    ));
    dependencies.services.host_context = Arc::new(PendingContext {
        entered: entered.clone(), release: release.clone(),
    });
    let backend = Arc::new(OpenLessBackend::new(BackendConfig {
        data_dir: directory.path().to_owned(), ..Default::default()
    }, dependencies).unwrap());
    backend.start().await.unwrap();
    let mut prefs = backend.get_preferences();
    prefs.skip_auto_style_picker_enabled = true;
    prefs.translation_target_language = "English".into();
    prefs.working_languages = vec!["简体中文".into()];
    backend.update_settings(prefs, openless_core::SettingsUpdateOptions::STRICT,
        &openless_core::NoopSettingsRuntime).unwrap();
    let source = Arc::downgrade(&backend);
    let fence = Arc::downgrade(&backend);
    let focus_available = Arc::new(AtomicBool::new(true));
    let source_focus = focus_available.clone();
    menu::configure_fenced(Arc::new(move || {
        let core = source.upgrade()?;
        if !matches!(core.snapshot().dictation.phase, openless_core::DictationPhase::Starting
            | openless_core::DictationPhase::Recording) { return None; }
        let (target, style_epoch, picker_enabled) = core.dictation_style_target_with_epoch()?;
        Some(menu::Context {
            session_id: target.session_id, style_epoch, mutable: target.mutable,
            skip_auto: target.skip_auto_style, picker_enabled, keyboard_available: true,
            focus: source_focus.load(Ordering::SeqCst).then_some(menu::Focus(1, 1)),
            blocked_keys: [false; 4],
        })
    }), Arc::new(|_| true), Arc::new(move || fence.upgrade().unwrap().dictation_style_epoch()));
    let geometry = AtomicUsize::new(0);
    let translation_geometry = AtomicUsize::new(0);
    let start = tokio::spawn({
        let backend = backend.clone();
        async move { backend.start_dictation_with_options(DictationStartOptions {
            translation_requested: true, ..Default::default()
        }).await }
    });
    entered.notified().await;
    let session = backend.snapshot().dictation.session_id.unwrap();
    assert_eq!(backend.snapshot().dictation.phase, openless_core::DictationPhase::Starting);
    let (target, epoch) = backend.toggle_dictation_style_with_epoch(session).unwrap();
    let request = menu::hotkey_toggled(&target, epoch).unwrap();
    assert_eq!(style_open(&backend, session, Some(&request.request_id), &geometry).unwrap_err(),
        "recording_style_required", "a current pending request is genuinely not ready");
    assert_eq!(geometry.load(Ordering::SeqCst), 0);
    assert_eq!(translation(&backend, session, true, &translation_geometry).unwrap_err(),
        "recording_translation_required");
    assert_eq!(translation_geometry.load(Ordering::SeqCst), 0);
    release.notify_one();
    assert_eq!(start.await.unwrap().unwrap(), session);
    assert!(backend.dictation_translation_target().unwrap().mutable);
    translation(&backend, session, true, &translation_geometry).unwrap();
    assert_eq!(translation_geometry.load(Ordering::SeqCst), 1);

    focus_available.store(false, Ordering::SeqCst);
    assert_eq!(style_open(&backend, session, None, &geometry).unwrap_err(),
        "capsule_style_menu_target_unavailable", "real target/permission failures stay visible");
    focus_available.store(true, Ordering::SeqCst);
    // Generate a fresh actual receipt after focus invalidated the earlier one.
    backend.toggle_dictation_style_with_epoch(session).unwrap();
    let (target, epoch) = backend.toggle_dictation_style_with_epoch(session).unwrap();
    let request = menu::hotkey_toggled(&target, epoch).unwrap();
    assert_eq!(capsule_picker::dispatch(&backend, session, true, true, || {
        menu::reservation("capsule", session, Some(&request.request_id))?;
        Err::<(), String>("native_geometry_failed".into())
    }).unwrap_err(), "native_geometry_failed");
    let owner = style_open(&backend, session, Some(&request.request_id), &geometry).unwrap();
    assert_eq!(geometry.load(Ordering::SeqCst), 1);
    backend.toggle_dictation_style_with_epoch(session).unwrap();
    let (target, epoch) = backend.toggle_dictation_style_with_epoch(session).unwrap();
    let request = menu::hotkey_toggled(&target, epoch).unwrap();
    assert_eq!(menu::pending("capsule"), Some(request.clone()));
    backend.cancel_dictation(Some(session)).await.unwrap();
    assert_eq!(style_open(&backend, session, Some(&request.request_id), &geometry).unwrap_err(),
        "capsule_style_request_stale", "ended request must reach request-aware classification");
    assert_eq!(style_open(&backend, session, None, &geometry).unwrap_err(),
        "recording_style_required", "explicit no-token errors are not silenced");
    assert_eq!(translation(&backend, session, true, &translation_geometry).unwrap_err(),
        "recording_translation_required");
    translation(&backend, session, false, &translation_geometry).unwrap();
    assert_eq!(translation_geometry.load(Ordering::SeqCst), 2, "translation close bypasses open precheck");
    assert_eq!(geometry.load(Ordering::SeqCst), 1, "stale/no-token failures never reserve geometry");

    let next_start = tokio::spawn({
        let backend = backend.clone();
        async move { backend.start_dictation().await }
    });
    entered.notified().await;
    let next = backend.snapshot().dictation.session_id.unwrap();
    release.notify_one();
    assert_eq!(next_start.await.unwrap().unwrap(), next);
    assert_ne!(next, session);
    assert_eq!(style_open(&backend, session, Some(&request.request_id), &geometry).unwrap_err(),
        "capsule_style_request_stale", "old session request stays stale after replacement");
    assert_eq!(style_open(&backend, session, None, &geometry).unwrap_err(), "recording_style_required");
    assert_eq!(translation(&backend, session, true, &translation_geometry).unwrap_err(),
        "recording_translation_required");
    assert_eq!(geometry.load(Ordering::SeqCst), 1);
    let (target, epoch) = backend.toggle_dictation_style_with_epoch(next).unwrap();
    let next_request = menu::hotkey_toggled(&target, epoch).unwrap();
    let next_owner = style_open(&backend, next, Some(&next_request.request_id), &geometry).unwrap();
    assert_eq!(geometry.load(Ordering::SeqCst), 2, "a valid new request can reserve geometry");
    assert!(!capsule_picker::dispatch(&backend, session, false, true, ||
        Ok(menu::close("capsule", session, &owner.menu_id))).unwrap());
    assert!(menu::owner_current(&next_owner), "old close cannot release replacement owner");
    assert!(capsule_picker::dispatch(&backend, next, false, true, ||
        Ok(menu::close("capsule", next, &next_owner.menu_id))).unwrap());
    backend.cancel_dictation(Some(next)).await.unwrap();
    menu::invalidate();
    menu::configure(Arc::new(|| None), Arc::new(|_| false));
    backend.shutdown().await.unwrap();
}
