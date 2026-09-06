//! Minimal non-UI host example for the Linux egui team.
//!
//! This example deliberately does not create a window or draw a frame.  It
//! demonstrates the lifecycle and event seam that an `eframe::App` can use.

use std::sync::Arc;

use openless_core::testing::{
    FixtureDictationEngine, FixtureSelectionRuntime, FixtureTextInserter, FixtureTextPolisher,
    RecordingHostActions,
};
use openless_core::{
    BackendErrorCode, LocalAsrRuntime, PolishMode, RepolishRequest, SelectionCapture,
    SelectionPolishOutputMode, SelectionPolishRequest, SelectionVoiceApplyOutcome,
    SelectionVoicePhase, SelectionVoicePreviewUpdate, SessionId,
};
use openless_linux_egui::{
    drain_events, BackendConfig, BackendDependencies, BackendError, BackendServices,
    EventDrainOutcome, InMemoryCredentialStore, InsertOutcome, LinuxHost, MarketplaceQuery,
    OpenLessBackend, SettingsEffectFailure, SettingsEffectPlan, SettingsEffectReceipt,
    SettingsRuntime, TokioTaskSpawner,
};

struct HeadlessSettingsRuntime;

impl SettingsRuntime for HeadlessSettingsRuntime {
    fn prepare(
        &self,
        _plan: &SettingsEffectPlan,
    ) -> Result<SettingsEffectReceipt, SettingsEffectFailure> {
        Ok(SettingsEffectReceipt::default())
    }

    fn commit(
        &self,
        _plan: &SettingsEffectPlan,
        _receipt: &mut SettingsEffectReceipt,
    ) -> Result<(), SettingsEffectFailure> {
        Ok(())
    }

    fn restore(
        &self,
        _plan: &SettingsEffectPlan,
        _receipt: &SettingsEffectReceipt,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), BackendError> {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-headless-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let host = RecordingHostActions::default();
    let selection_runtime = FixtureSelectionRuntime::linux_preview_unsupported(SelectionCapture {
        text: "fixture selection".into(),
        source_app: Some("Headless fixture".into()),
    });
    let backend = Arc::new(OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        BackendDependencies {
            host_actions: Arc::new(host),
            text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
            dictation_engine: Arc::new(FixtureDictationEngine::successful(
                "fixture raw",
                "fixture polished",
            )),
            task_spawner: Arc::new(TokioTaskSpawner),
            credential_store: Arc::new(InMemoryCredentialStore::default()),
            services: BackendServices::unsupported(),
            local_asr_runtime: None,
            marketplace_config: None,
            selection_runtime: Some(Arc::new(selection_runtime)),
            selection_polisher: Some(Arc::new(FixtureTextPolisher::successful(
                "fixture selection polished",
            ))),
            qa_runtime: None,
        },
    )?);
    let linux_host =
        LinuxHost::with_settings_runtime(Arc::clone(&backend), Arc::new(HeadlessSettingsRuntime));

    let mut events = linux_host.subscribe();
    backend.start().await?;

    // Data-only use-cases are immediately available through the same facade.
    // A real view model can load these before it starts drawing frames.
    let preferences = backend.get_preferences();
    let _history = backend.list_history()?;
    let _style_packs = backend.list_style_packs(&preferences.active_style_pack_id)?;

    // Unconfigured platform/provider domains fail explicitly.  The egui UI
    // should map these errors to unavailable capabilities instead of rendering
    // controls that appear to work.  Replace each unsupported adapter with a
    // real Linux adapter or a deterministic fixture without changing callers.
    let services = backend.services();
    let unsupported = [
        services
            .local_asr
            .runtime_status(LocalAsrRuntime::Generic)
            .await
            .expect_err("headless Local ASR must be unsupported"),
        services
            .marketplace
            .list(MarketplaceQuery {
                query: None,
                sort: None,
                limit: None,
            })
            .await
            .expect_err("headless Marketplace must be unsupported"),
        services
            .qa
            .snapshot()
            .await
            .expect_err("headless QA must be unsupported"),
        services
            .remote_input
            .status()
            .expect_err("headless Remote Input must be unsupported"),
    ];
    assert!(unsupported
        .iter()
        .all(|error| error.code == BackendErrorCode::Unsupported));
    let auxiliary_error = services
        .auxiliary
        .repolish(RepolishRequest {
            raw_text: "fixture raw".into(),
            style_pack_id: None,
            front_app: None,
        })
        .await
        .expect_err("headless auxiliary processing must be unsupported");
    assert_eq!(auxiliary_error.code, BackendErrorCode::Unsupported);
    let retranscription_error = services
        .auxiliary
        .retranscribe_pcm(vec![0, 0])
        .await
        .expect_err("headless retranscription must be unsupported");
    assert_eq!(
        retranscription_error.error.code,
        BackendErrorCode::Unsupported
    );

    // Less Computer voice hosts reserve a Core capture lease before touching
    // recorder/native ASR resources and release it explicitly on cancellation
    // or startup failure. The headless example exercises that seam without
    // requiring a coding-agent process.
    let mut less_computer_preferences = backend.get_preferences();
    less_computer_preferences.coding_agent_enabled = true;
    linux_host.update_settings_strict(
        less_computer_preferences,
        linux_host.snapshot().preferences_revision,
    )?;
    let less_computer_session = SessionId::new();
    backend.begin_less_computer_capture(less_computer_session)?;
    assert_eq!(
        backend.less_computer_active_session(),
        Some(less_computer_session)
    );
    backend
        .cancel_less_computer(Some(less_computer_session))
        .await?;
    assert!(backend.less_computer_capture_cancelled(less_computer_session));
    backend.abort_less_computer_capture(less_computer_session)?;
    assert_eq!(backend.less_computer_active_session(), None);

    // The deterministic Linux selection fixture supports direct replacement,
    // but retained preview targets and platform revert are intentionally
    // unsupported. A view model can therefore exercise the exact capability
    // branches without fcitx5 or a window.
    let direct_session = services
        .selection
        .begin_polish(SelectionPolishRequest {
            selected_text: Some("fixture selection".into()),
            mode: PolishMode::Raw,
            instruction: None,
        })
        .await?;
    let revert_error = services
        .selection
        .revert(direct_session)
        .await
        .expect_err("headless Linux revert must be unsupported");
    assert_eq!(revert_error.code, BackendErrorCode::Unsupported);

    let mut preview_preferences = backend.get_preferences();
    preview_preferences.selection_polish_output_mode = SelectionPolishOutputMode::PreviewConfirm;
    linux_host.update_settings_strict(
        preview_preferences,
        linux_host.snapshot().preferences_revision,
    )?;
    let preview_error = services
        .selection
        .begin_polish(SelectionPolishRequest {
            selected_text: Some("fixture selection".into()),
            mode: PolishMode::Raw,
            instruction: None,
        })
        .await
        .expect_err("headless Linux preview must be unsupported");
    assert_eq!(preview_error.code, BackendErrorCode::Unsupported);

    exercise_selection_voice(services).await?;

    let _session = backend.start_dictation().await?;
    let result = backend.stop_dictation().await?;
    println!("{}", result.polished_text);
    backend.shutdown().await?;

    // A real egui host drains this subscription without blocking its frame and
    // requests a repaint after each event.  The example only proves that the
    // subscription can be created and consumed by a host runtime.
    match drain_events(&mut events, |event| println!("event #{}", event.sequence)) {
        EventDrainOutcome::Idle { .. } => {}
        EventDrainOutcome::Lagged { dropped, .. } => {
            eprintln!("event subscription lagged by {dropped}; resync from snapshot");
            let _snapshot = linux_host.snapshot();
        }
        EventDrainOutcome::Closed { .. } => eprintln!("event subscription closed"),
    }
    let _ = std::fs::remove_dir_all(data_dir);
    Ok(())
}

async fn exercise_selection_voice(services: &BackendServices) -> Result<(), BackendError> {
    let voice = &services.selection_voice;

    let confirmed = voice
        .begin(SelectionCapture {
            text: "source".into(),
            source_app: None,
        })
        .await?;
    voice.mark_processing(confirmed).await?;
    voice
        .set_preview(SelectionVoicePreviewUpdate {
            session_id: confirmed,
            owner_session_id: Some(confirmed),
            text: "preview".into(),
            summary: Some("fixture summary".into()),
        })
        .await?;
    assert!(voice.preview(Some(confirmed)).await?.is_some());
    let ticket = voice.begin_preview_apply(Some(confirmed), "confirmed preview".into())?;
    voice
        .finish_preview_apply(ticket.ticket_id, SelectionVoiceApplyOutcome::Inserted)
        .await?;
    assert_eq!(
        voice.snapshot().await?.phase,
        SelectionVoicePhase::Completed
    );

    let unknown = voice
        .begin(SelectionCapture {
            text: "unknown source".into(),
            source_app: None,
        })
        .await?;
    voice.mark_processing(unknown).await?;
    voice
        .set_preview(SelectionVoicePreviewUpdate {
            session_id: unknown,
            owner_session_id: Some(unknown),
            text: "unknown preview".into(),
            summary: None,
        })
        .await?;
    let ticket = voice.begin_preview_apply(Some(unknown), "unknown preview".into())?;
    voice
        .finish_preview_apply(ticket.ticket_id, SelectionVoiceApplyOutcome::CopiedFallback)
        .await?;
    assert_eq!(
        voice.snapshot().await?.apply_outcome,
        Some(SelectionVoiceApplyOutcome::CopiedFallback)
    );

    let cancelled = voice
        .begin(SelectionCapture {
            text: "cancel source".into(),
            source_app: None,
        })
        .await?;
    voice.cancel(Some(cancelled)).await?;
    assert_eq!(
        voice.snapshot().await?.phase,
        SelectionVoicePhase::Cancelled
    );

    let current = voice
        .begin(SelectionCapture {
            text: "current source".into(),
            source_app: None,
        })
        .await?;
    let stale = voice
        .cancel(Some(cancelled))
        .await
        .expect_err("a stale selection voice session must not cancel the current one");
    assert_eq!(stale.code, BackendErrorCode::Cancelled);
    voice.cancel(Some(current)).await?;
    Ok(())
}
