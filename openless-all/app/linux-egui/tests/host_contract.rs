use std::sync::{Arc, Mutex};

use openless_linux_egui::{
    drain_events, BackendConfig, BackendDependencies, BackendErrorCode, BackendEventKind,
    BackendServices, CliDispatchOutcome, CliIntent, DictationSession, EventDrainOutcome,
    EventRecvError, FixtureDictationEngine, FixtureSelectionRuntime, FixtureTextInserter,
    FixtureTextPolisher, HistoryInsertStatus, HistorySource, HotkeyRuntimeTarget, HotkeyTrigger,
    InMemoryCredentialStore, InsertOutcome, LinuxHost, LinuxHotkeyEvent, LinuxLaunchIntent,
    LinuxSettingsEffects, LinuxSettingsRuntime, OpenLessBackend, PolishMode, RecordingHostActions,
    SelectionCapture, SelectionPolishOutputMode, SelectionPolishRequest,
    SelectionVoiceApplyOutcome, SelectionVoicePhase, SelectionVoicePreviewUpdate, ShortcutBinding,
    StylePack, TokioTaskSpawner,
};

fn history_session(id: &str) -> DictationSession {
    DictationSession {
        id: id.to_string(),
        created_at: "2026-08-27T00:00:00Z".to_string(),
        source: HistorySource::Voice,
        raw_transcript: "raw".to_string(),
        asr_transcript: None,
        final_text: "final".to_string(),
        mode: PolishMode::Light,
        style_pack_id: None,
        translation_active: false,
        polish_source: None,
        app_bundle_id: None,
        app_name: None,
        insert_status: HistoryInsertStatus::Inserted,
        error_code: None,
        duration_ms: Some(1000),
        dictionary_entry_count: None,
        has_audio_recording: None,
        asr_provider: None,
        asr_model: None,
        llm_provider: None,
        llm_model: None,
        pipeline_mode: None,
        asr_ms: None,
        polish_ms: None,
    }
}

#[tokio::test]
async fn linux_host_exposes_snapshot_and_non_blocking_event_subscription() {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-linux-host-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(RecordingHostActions::default()),
                text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
                dictation_engine: Arc::new(FixtureDictationEngine::successful("raw", "polished")),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(InMemoryCredentialStore::default()),
                services: BackendServices::unsupported(),
                local_asr_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap(),
    );
    let host = LinuxHost::new(Arc::clone(&backend));
    let mut events = host.subscribe();

    assert!(!host.snapshot().running);
    assert!(matches!(events.try_recv(), Err(EventRecvError::Empty)));

    backend.start().await.unwrap();
    assert!(host.snapshot().running);
    let mut received = Vec::new();
    assert_eq!(
        drain_events(&mut events, |event| received.push(event)),
        EventDrainOutcome::Idle { processed: 1 }
    );
    assert!(matches!(received[0].kind, BackendEventKind::BackendStarted));

    let entry = backend.add_vocabulary("OpenLess".into(), None).unwrap();
    assert_eq!(backend.list_vocabulary().unwrap(), vec![entry]);
    assert_eq!(host.snapshot().vocabulary_revision, 1);
    received.clear();
    assert_eq!(
        drain_events(&mut events, |event| received.push(event)),
        EventDrainOutcome::Idle { processed: 1 }
    );
    assert!(matches!(
        received[0].kind,
        BackendEventKind::VocabularyChanged(_)
    ));

    let history = history_session("linux-contract");
    backend.append_history(history.clone(), 30, None).unwrap();
    assert_eq!(backend.list_history().unwrap(), vec![history]);
    assert_eq!(host.snapshot().history_revision, 1);
    received.clear();
    assert_eq!(
        drain_events(&mut events, |event| received.push(event)),
        EventDrainOutcome::Idle { processed: 1 }
    );
    assert!(matches!(
        received[0].kind,
        BackendEventKind::HistoryChanged(_)
    ));

    let style_pack = backend
        .create_style_pack(StylePack {
            name: "Linux contract".to_string(),
            prompt: "prompt".to_string(),
            ..StylePack::default()
        })
        .unwrap();
    assert!(backend
        .list_style_packs(&style_pack.id)
        .unwrap()
        .iter()
        .any(|pack| pack.id == style_pack.id && pack.active));
    assert_eq!(host.snapshot().style_pack_revision, 1);
    received.clear();
    assert_eq!(
        drain_events(&mut events, |event| received.push(event)),
        EventDrainOutcome::Idle { processed: 1 }
    );
    assert!(matches!(
        received[0].kind,
        BackendEventKind::StylePacksChanged(_)
    ));

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn forwarded_launch_intents_use_core_state_and_semantic_host_actions() {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-linux-launch-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let actions = RecordingHostActions::default();
    let engine = FixtureDictationEngine::successful("raw", "polished");
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(actions.clone()),
                text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
                dictation_engine: Arc::new(engine.clone()),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(InMemoryCredentialStore::default()),
                services: BackendServices::unsupported(),
                local_asr_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap(),
    );
    backend.start().await.unwrap();
    let host = LinuxHost::new(Arc::clone(&backend));

    assert_eq!(
        host.dispatch_launch_intent(LinuxLaunchIntent::ShowMain)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        actions.actions(),
        vec![
            openless_linux_egui::HostAction::ShowMain,
            openless_linux_egui::HostAction::FocusMain,
        ]
    );

    assert!(matches!(
        host.dispatch_launch_intent(LinuxLaunchIntent::Cli(CliIntent::ToggleDictation,))
            .await
            .unwrap(),
        Some(CliDispatchOutcome::DictationStarted(_))
    ));
    assert!(matches!(
        host.dispatch_launch_intent(LinuxLaunchIntent::Cli(CliIntent::ToggleDictation,))
            .await
            .unwrap(),
        Some(CliDispatchOutcome::DictationCompleted(_))
    ));

    let pressed_at = std::time::Instant::now() + std::time::Duration::from_secs(1);
    assert!(matches!(
        host.dispatch_hotkey_event(LinuxHotkeyEvent::DictationPressed {
            symbol: 1,
            states: 0,
            press_id: 1,
            at: pressed_at,
        })
        .await
        .unwrap(),
        Some(CliDispatchOutcome::DictationStarted(_))
    ));
    assert_eq!(
        host.dispatch_hotkey_event(LinuxHotkeyEvent::DictationCombined {
            symbol: 2,
            states: 0,
            press_id: 1,
            at: pressed_at,
        })
        .await
        .unwrap(),
        Some(CliDispatchOutcome::DictationCancelled)
    );

    let mut preferences = backend.get_preferences();
    preferences.translation_target_language = "English".to_string();
    preferences.working_languages = vec!["简体中文".to_string()];
    host.update_settings_strict(preferences, host.snapshot().preferences_revision)
        .unwrap();
    assert_eq!(
        host.dispatch_hotkey_event(LinuxHotkeyEvent::TranslationPressed)
            .await
            .unwrap(),
        None
    );
    assert!(matches!(
        host.dispatch_hotkey_event(LinuxHotkeyEvent::DictationPressed {
            symbol: 1,
            states: 0,
            press_id: 2,
            at: pressed_at + std::time::Duration::from_secs(1),
        })
        .await
        .unwrap(),
        Some(CliDispatchOutcome::DictationStarted(_))
    ));
    assert!(
        engine
            .contexts()
            .last()
            .expect("translation dictation context")
            .polish
            .translation_active
    );
    host.dispatch_hotkey_event(LinuxHotkeyEvent::DictationCombined {
        symbol: 2,
        states: 0,
        press_id: 2,
        at: pressed_at + std::time::Duration::from_secs(1),
    })
    .await
    .unwrap();
    let selection_error = host
        .dispatch_hotkey_event(LinuxHotkeyEvent::SelectionPolishPressed)
        .await
        .expect_err("unconfigured selection adapter must fail explicitly");
    assert_eq!(selection_error.code, BackendErrorCode::Unsupported);

    backend.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[derive(Default)]
struct RecordingSettingsEffects {
    hotkeys: Mutex<Vec<HotkeyRuntimeTarget>>,
    active_asr_providers: Mutex<Vec<String>>,
    fail_next_hotkey: std::sync::atomic::AtomicBool,
}

impl LinuxSettingsEffects for RecordingSettingsEffects {
    fn apply_hotkeys(
        &self,
        target: &HotkeyRuntimeTarget,
    ) -> Result<(), openless_linux_egui::BackendError> {
        self.hotkeys.lock().unwrap().push(target.clone());
        if self
            .fail_next_hotkey
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return Err(openless_linux_egui::BackendError::new(
                BackendErrorCode::Platform,
                "fixture fcitx5 apply failed",
            ));
        }
        Ok(())
    }

    fn set_active_asr_provider(
        &self,
        provider_id: &str,
    ) -> Result<(), openless_linux_egui::BackendError> {
        self.active_asr_providers
            .lock()
            .unwrap()
            .push(provider_id.to_string());
        Ok(())
    }
}

#[test]
fn linux_public_settings_contract_is_validated_transactional_and_runtime_backed() {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-linux-preferences-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies::unsupported(),
        )
        .unwrap(),
    );
    let effects = Arc::new(RecordingSettingsEffects::default());
    let settings_runtime = Arc::new(LinuxSettingsRuntime::with_effects(effects.clone()));
    let host = LinuxHost::with_settings_runtime(Arc::clone(&backend), settings_runtime);
    let mut events = host.subscribe();
    let mut valid = backend.get_preferences();
    valid.dictation_hotkey = ShortcutBinding {
        primary: "LeftShift".to_string(),
        modifiers: Vec::new(),
    };

    host.update_settings_strict(valid, 0).unwrap();

    let saved = backend.get_preferences();
    assert_eq!(saved.hotkey.trigger, HotkeyTrigger::LeftShift);
    assert!(saved.custom_combo_hotkey.is_none());
    assert_eq!(effects.hotkeys.lock().unwrap().len(), 1);
    assert!(matches!(
        events.try_recv().unwrap().kind,
        BackendEventKind::PreferencesChanged(_)
    ));
    let revision = backend.snapshot().preferences_revision;
    let saved_json = serde_json::to_value(&saved).unwrap();
    let mut conflicting = saved.clone();
    conflicting.translation_hotkey = conflicting.dictation_hotkey.clone();

    let error = host
        .update_settings_strict(conflicting, revision)
        .expect_err("Linux host must receive the shared shortcut conflict");

    assert_eq!(error.code, BackendErrorCode::InvalidArgument);
    assert_eq!(backend.snapshot().preferences_revision, revision);
    assert_eq!(
        serde_json::to_value(backend.get_preferences()).unwrap(),
        saved_json
    );
    assert_eq!(events.try_recv(), Err(EventRecvError::Empty));

    effects
        .fail_next_hotkey
        .store(true, std::sync::atomic::Ordering::Release);
    let mut runtime_failure = backend.get_preferences();
    runtime_failure.dictation_hotkey = ShortcutBinding {
        primary: "F9".to_string(),
        modifiers: vec!["ctrl".to_string()],
    };
    let error = host
        .update_settings_strict(runtime_failure, revision)
        .expect_err("Linux runtime failure must fail the settings transaction");
    assert_eq!(error.code, BackendErrorCode::Platform);
    assert_eq!(backend.snapshot().preferences_revision, revision);
    assert_eq!(
        serde_json::to_value(backend.get_preferences()).unwrap(),
        saved_json
    );
    assert_eq!(events.try_recv(), Err(EventRecvError::Empty));
    let applied = effects.hotkeys.lock().unwrap();
    assert_eq!(applied.len(), 3, "next apply plus previous-target restore");
    assert_eq!(applied.last().unwrap().dictation, saved.dictation_hotkey);
    drop(applied);

    let mut provider_change = backend.get_preferences();
    provider_change.active_asr_provider = "linux-fixture-asr".to_string();
    host.update_settings_strict(provider_change, revision)
        .unwrap();
    assert_eq!(
        effects.active_asr_providers.lock().unwrap().as_slice(),
        ["linux-fixture-asr"]
    );
    assert!(matches!(
        events.try_recv().unwrap().kind,
        BackendEventKind::PreferencesChanged(_)
    ));

    let stale_error = host
        .update_settings_strict(saved, revision)
        .expect_err("stale egui settings documents must not overwrite a newer save");
    assert_eq!(stale_error.code, BackendErrorCode::Busy);
    assert!(stale_error.retryable);
    assert_eq!(backend.snapshot().preferences_revision, revision + 1);
    assert_eq!(events.try_recv(), Err(EventRecvError::Empty));

    let mut windows_only = backend.get_preferences();
    windows_only.windows_show_openless_in_keyboard_list =
        !windows_only.windows_show_openless_in_keyboard_list;
    let unsupported = host
        .update_settings_strict(windows_only, revision + 1)
        .expect_err("Windows-only effects must be explicit on the Linux host");
    assert_eq!(unsupported.code, BackendErrorCode::Unsupported);
    assert_eq!(backend.snapshot().preferences_revision, revision + 1);
    assert_eq!(events.try_recv(), Err(EventRecvError::Empty));
    for enabled in [true, false] {
        let mut preferences = backend.get_preferences();
        preferences.coding_agent_enabled = enabled;
        preferences.coding_agent_voice_hotkey = Some(ShortcutBinding {
            primary: "F10".into(),
            modifiers: vec!["ctrl".into()],
        });
        host.update_settings_strict(preferences, backend.snapshot().preferences_revision)
            .expect("Linux must allow enabling and disabling its wired Less Computer action");
        assert_eq!(backend.get_preferences().coding_agent_enabled, enabled);
        assert_eq!(
            effects
                .hotkeys
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .coding_agent_enabled,
            enabled
        );
    }
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn linux_headless_selection_contract_covers_capability_and_session_edges() {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-linux-selection-host-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.selection_runtime = Some(Arc::new(FixtureSelectionRuntime::successful(
        SelectionCapture {
            text: "fixture selection".into(),
            source_app: None,
        },
        InsertOutcome::Inserted,
    )));
    dependencies.selection_polisher = Some(Arc::new(FixtureTextPolisher::successful(
        "fixture selection polished",
    )));
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap(),
    );
    let host = LinuxHost::new(Arc::clone(&backend));
    let selection = &backend.services().selection;

    let direct_session = selection
        .begin_polish(SelectionPolishRequest {
            selected_text: Some("fixture selection".into()),
            mode: PolishMode::Raw,
            instruction: None,
        })
        .await
        .unwrap();
    selection.revert(direct_session).await.unwrap();

    let mut preferences = backend.get_preferences();
    preferences.selection_polish_output_mode = SelectionPolishOutputMode::PreviewConfirm;
    host.update_settings_strict(preferences, host.snapshot().preferences_revision)
        .unwrap();
    let preview_session = selection
        .begin_polish(SelectionPolishRequest {
            selected_text: Some("fixture selection".into()),
            mode: PolishMode::Raw,
            instruction: None,
        })
        .await
        .unwrap();
    assert_eq!(
        selection.snapshot().await.unwrap().phase,
        openless_linux_egui::SelectionPhase::Preview
    );
    selection.confirm(preview_session, None).await.unwrap();
    selection.revert(preview_session).await.unwrap();

    let voice = &backend.services().selection_voice;
    let confirmed = voice
        .begin(SelectionCapture {
            text: "source".into(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(confirmed).await.unwrap();
    voice
        .set_preview(SelectionVoicePreviewUpdate {
            session_id: confirmed,
            owner_session_id: Some(confirmed),
            text: "preview".into(),
            summary: None,
        })
        .await
        .unwrap();
    let ticket = voice
        .begin_preview_apply(Some(confirmed), "confirmed".into())
        .unwrap();
    voice
        .finish_preview_apply(ticket.ticket_id, SelectionVoiceApplyOutcome::Inserted)
        .await
        .unwrap();
    assert_eq!(
        voice.snapshot().await.unwrap().phase,
        SelectionVoicePhase::Completed
    );

    let unknown = voice
        .begin(SelectionCapture {
            text: "unknown".into(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(unknown).await.unwrap();
    voice
        .set_preview(SelectionVoicePreviewUpdate {
            session_id: unknown,
            owner_session_id: Some(unknown),
            text: "unknown preview".into(),
            summary: None,
        })
        .await
        .unwrap();
    let ticket = voice
        .begin_preview_apply(Some(unknown), "unknown preview".into())
        .unwrap();
    voice
        .finish_preview_apply(ticket.ticket_id, SelectionVoiceApplyOutcome::CopiedFallback)
        .await
        .unwrap();
    assert_eq!(
        voice.snapshot().await.unwrap().apply_outcome,
        Some(SelectionVoiceApplyOutcome::CopiedFallback)
    );

    let cancelled = voice
        .begin(SelectionCapture {
            text: "cancelled".into(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.cancel(Some(cancelled)).await.unwrap();
    let current = voice
        .begin(SelectionCapture {
            text: "current".into(),
            source_app: None,
        })
        .await
        .unwrap();
    assert_eq!(
        voice
            .cancel(Some(cancelled))
            .await
            .expect_err("stale cancel must preserve the current session")
            .code,
        BackendErrorCode::Cancelled
    );
    assert_eq!(voice.snapshot().await.unwrap().session_id, Some(current));
    voice.cancel(Some(current)).await.unwrap();

    let _ = std::fs::remove_dir_all(data_dir);
}
