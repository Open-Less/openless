use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openless_core::testing::{FixtureDictationEngine, FixtureTextInserter, RecordingHostActions};
use openless_core::*;
use tokio::sync::Semaphore;

type Task<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

#[derive(Default)]
struct Runtime {
    configured: AtomicUsize,
    stopped: Arc<AtomicUsize>,
    paused: Mutex<(u64, bool)>,
    fail: AtomicBool,
    delay: Mutex<Option<Arc<Semaphore>>>,
    engine_start_delay: Mutex<Option<Arc<Semaphore>>>,
    engine_finish_delay: Mutex<Option<Arc<Semaphore>>>,
    handoff_delay: Arc<Mutex<Option<Arc<Semaphore>>>>,
    handoff_started: Arc<AtomicBool>,
}
impl VoiceActivationRuntime for Runtime {
    fn set_capture_paused(&self, revision: u64, paused: bool) {
        let mut policy = self.paused.lock().unwrap();
        if revision >= policy.0 {
            *policy = (revision, paused);
        }
    }
    fn configure(
        &self,
        _: VoiceActivationConfig,
        _: Arc<dyn VoiceActivationSink>,
    ) -> Task<Result<Box<dyn VoiceActivationHandle>, VoiceActivationError>> {
        self.configured.fetch_add(1, Ordering::SeqCst);
        let stopped = Arc::clone(&self.stopped);
        let fail = self.fail.load(Ordering::SeqCst);
        let delay = self.delay.lock().unwrap().take();
        let handoff_delay = self.handoff_delay.clone();
        let handoff_started = self.handoff_started.clone();
        Box::pin(async move {
            if let Some(delay) = delay {
                delay.acquire_owned().await.unwrap().forget();
            }
            if fail {
                Err(VoiceActivationError::ModelMissing)
            } else {
                Ok(Box::new(Handle(stopped, handoff_delay, handoff_started)) as Box<dyn VoiceActivationHandle>)
            }
        })
    }
}
struct Handle(Arc<AtomicUsize>, Arc<Mutex<Option<Arc<Semaphore>>>>, Arc<AtomicBool>);
impl VoiceActivationHandle for Handle {
    fn handoff_to_dictation(
        &self,
        _: VoiceDictationHandoffRequest,
    ) -> Task<Result<VoiceDictationHandoff, VoiceActivationError>> {
        let delay = self.1.lock().unwrap().take();
        self.2.store(true, Ordering::SeqCst);
        Box::pin(async move {
            if let Some(delay) = delay { delay.acquire_owned().await.unwrap().forget(); }
            Ok(VoiceDictationHandoff { capture: Box::new(Capture) })
        })
    }
    fn stop(&self, _: VoiceActivationStopReason) -> Task<Result<(), VoiceActivationError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}
struct Capture;
impl PendingAudioCapture for Capture {
    fn commit(self: Box<Self>, _: SessionId, _: Arc<DictationContext>,
        _: Arc<dyn AudioConsumer>, _: Arc<dyn RecordingProgressSink>)
        -> Task<Result<Box<dyn ActiveRecording>, BackendError>>
    {
        Box::pin(async { Err(BackendError::new(BackendErrorCode::Unsupported, "fixture capture")) })
    }
    fn discard(self: Box<Self>) -> Task<Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}
struct Engine(FixtureDictationEngine, Arc<Runtime>);
impl DictationEngine for Engine {
    fn start_with_audio_capture(&self, id: SessionId, context: Arc<DictationContext>,
        progress: Arc<dyn EngineProgressSink>, capture: Box<dyn PendingAudioCapture>)
        -> Task<Result<(), BackendError>>
    {
        let start = self.0.start(id, context, progress);
        let delay = self.1.engine_start_delay.lock().unwrap().take();
        Box::pin(async move {
            if let Some(delay) = delay { delay.acquire_owned().await.unwrap().forget(); }
            capture.discard().await?;
            start.await
        })
    }
    fn start(&self, id: SessionId, context: Arc<DictationContext>, progress: Arc<dyn EngineProgressSink>)
        -> Task<Result<(), BackendError>> { self.0.start(id, context, progress) }
    fn finish(&self, id: SessionId, progress: Arc<dyn EngineProgressSink>)
        -> Task<Result<EngineResult, EngineFailure>> {
        let finish = self.0.finish(id, progress);
        let delay = self.1.engine_finish_delay.lock().unwrap().take();
        Box::pin(async move {
            if let Some(delay) = delay { delay.acquire_owned().await.unwrap().forget(); }
            finish.await
        })
    }
    fn update_context(&self, id: SessionId, context: Arc<DictationContext>)
        -> Task<Result<(), BackendError>> { self.0.update_context(id, context) }
    fn cancel(&self, id: SessionId) -> Task<Result<(), BackendError>> { self.0.cancel(id) }
}
struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> (Directory, Arc<OpenLessBackend>, Arc<Runtime>, FixtureDictationEngine) {
    let dir = Directory(std::env::temp_dir().join(format!(
        "openless-voice-link-{}-{}",
        std::process::id(),
        SessionId::new()
    )));
    let runtime = Arc::new(Runtime::default());
    let engine = FixtureDictationEngine::successful("body", "body");
    let mut deps = BackendDependencies::unsupported();
    deps.voice_activation_runtime = Some(runtime.clone());
    deps.host_actions = Arc::new(RecordingHostActions::default());
    deps.text_inserter = Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted));
    deps.dictation_engine = Arc::new(Engine(engine.clone(), runtime.clone()));
    deps.task_spawner = Arc::new(TokioTaskSpawner);
    let backend = Arc::new(OpenLessBackend::new(BackendConfig {
        data_dir: dir.0.clone(), ..Default::default()
    }, deps).unwrap());
    (dir, backend, runtime, engine)
}
fn edges(id: u64) -> [DictationHotkeyEdge; 2] {
    let at = Instant::now() + Duration::from_secs(id);
    [
        DictationHotkeyEdge::Pressed { press_id: id, at },
        DictationHotkeyEdge::Released { press_id: id, at: at + Duration::from_millis(10) },
    ]
}
async fn click(backend: &OpenLessBackend, id: u64) {
    for edge in edges(id) {
        backend.dispatch_dictation_hotkey_edge(edge).await.unwrap();
    }
}
fn enable(backend: &OpenLessBackend, mode: HotkeyMode) {
    let mut prefs = backend.get_preferences();
    prefs.voice_activation_enabled = true;
    prefs.voice_activation_hotkey_linked = true;
    prefs.hotkey.mode = mode;
    backend.repositories().preferences.set(prefs).unwrap();
}

#[test]
fn linked_preference_is_local_only_and_old_configs_default_off() {
    use openless_core::cloud_sync_e2ee_documents::registry::{PREFERENCE_FIELDS, PreferenceClass};
    let old: UserPreferences = serde_json::from_str("{}").unwrap();
    assert!(!old.voice_activation_hotkey_linked);
    let prefs = UserPreferences { voice_activation_hotkey_linked: true, ..old };
    let json = serde_json::to_value(&prefs).unwrap();
    assert_eq!(json["voiceActivationHotkeyLinked"], true);
    let loaded: UserPreferences = serde_json::from_value(json).unwrap();
    assert!(loaded.voice_activation_hotkey_linked);
    let field = PREFERENCE_FIELDS.iter().find(|f| f.key == "voiceActivationHotkeyLinked").unwrap();
    assert_eq!(field.class, PreferenceClass::Excluded);
}

#[tokio::test]
async fn all_recording_modes_use_one_click_and_never_start_dictation() {
    for mode in [HotkeyMode::Toggle, HotkeyMode::Hold, HotkeyMode::Auto, HotkeyMode::DoubleClick] {
        let (_dir, backend, runtime, engine) = fixture();
        enable(&backend, mode);
        backend.start().await.unwrap();
        assert_eq!(backend.latest_voice_activation_state().unwrap().phase, VoiceActivationPhase::Paused);
        assert_eq!(runtime.configured.load(Ordering::SeqCst), 0);
        click(&backend, 1).await;
        assert_eq!(backend.latest_voice_activation_state().unwrap().phase, VoiceActivationPhase::ListeningForWake);
        assert!(!runtime.paused.lock().unwrap().1);
        click(&backend, 2).await;
        assert_eq!(backend.latest_voice_activation_state().unwrap().phase, VoiceActivationPhase::Paused);
        assert!(runtime.paused.lock().unwrap().1);
        let mut prefs = backend.get_preferences();
        prefs.low_latency_dictation_enabled = true;
        backend.repositories().preferences.set(prefs).unwrap();
        backend.configure_voice_activation().await.unwrap();
        assert_eq!(runtime.configured.load(Ordering::SeqCst), 1);
        assert!(engine.actions().is_empty());
        assert!(backend.get_preferences().voice_activation_enabled);
        assert!(backend.get_preferences().voice_activation_hotkey_linked);
        backend.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn pause_invalidates_inflight_start_without_waiting_for_the_model() {
    let (_dir, backend, runtime, _) = fixture();
    enable(&backend, HotkeyMode::Hold);
    backend.start().await.unwrap();
    let delay = Arc::new(Semaphore::new(0));
    *runtime.delay.lock().unwrap() = Some(delay.clone());
    for edge in edges(1) { backend.accept_voice_activation_hotkey_edge(edge); }
    let start = {
        let backend = backend.clone();
        tokio::spawn(async move { backend.reconcile_voice_activation_hotkey().await })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.configured.load(Ordering::SeqCst) == 0 { tokio::task::yield_now().await; }
    }).await.unwrap();
    for edge in edges(2) { backend.accept_voice_activation_hotkey_edge(edge); }
    assert!(runtime.paused.lock().unwrap().1);
    delay.add_permits(1);
    let _ = tokio::time::timeout(Duration::from_secs(5), start).await.unwrap().unwrap();
    backend.reconcile_voice_activation_hotkey().await.unwrap();
    assert_eq!(backend.latest_voice_activation_state().unwrap().phase, VoiceActivationPhase::Paused);
    assert_eq!(runtime.stopped.load(Ordering::SeqCst), 1);
    backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_start_stays_paused_and_the_next_click_retries() {
    let (_dir, backend, runtime, engine) = fixture();
    enable(&backend, HotkeyMode::Auto);
    backend.start().await.unwrap();
    runtime.fail.store(true, Ordering::SeqCst);
    backend.accept_voice_activation_hotkey_edge(edges(1)[0]);
    backend.accept_voice_activation_hotkey_edge(edges(1)[1]);
    assert!(backend.reconcile_voice_activation_hotkey().await.is_err());
    assert_eq!(backend.latest_voice_activation_state().unwrap().phase, VoiceActivationPhase::Failed);
    assert!(runtime.paused.lock().unwrap().1);
    runtime.fail.store(false, Ordering::SeqCst);
    click(&backend, 2).await;
    assert_eq!(runtime.configured.load(Ordering::SeqCst), 2);
    assert!(!runtime.paused.lock().unwrap().1);
    assert!(engine.actions().is_empty());
    backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn pausing_listener_does_not_stop_an_unrelated_explicit_recording() {
    let (_dir, backend, _, engine) = fixture();
    enable(&backend, HotkeyMode::Toggle);
    backend.start().await.unwrap();
    let id = backend.start_dictation().await.unwrap();
    click(&backend, 1).await;
    click(&backend, 2).await;
    assert_eq!(backend.snapshot().dictation.session_id, Some(id));
    assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Recording);
    assert_eq!(engine.actions().len(), 1);
    backend.stop_dictation().await.unwrap();
    backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn pause_finalizes_voice_started_body_without_cancelling_or_rearming() {
    let (_dir, backend, runtime, engine) = fixture();
    enable(&backend, HotkeyMode::Toggle);
    backend.start().await.unwrap();
    click(&backend, 1).await;
    let generation = backend.latest_voice_activation_state().unwrap().activation_generation;
    backend.handle_voice_activation_signal(VoiceActivationSignal::WakeDetected {
        activation_generation: generation,
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while backend.snapshot().dictation.phase != DictationPhase::Recording {
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    click(&backend, 2).await;
    assert_eq!(backend.latest_voice_activation_state().unwrap().phase, VoiceActivationPhase::Paused);
    assert_eq!(backend.list_history().unwrap()[0].final_text, "body");
    assert!(runtime.paused.lock().unwrap().1);
    assert_eq!(runtime.configured.load(Ordering::SeqCst), 1);
    assert!(!engine.actions().iter().any(|action|
        matches!(action, openless_core::testing::FixtureEngineAction::Cancel(_))));
    backend.shutdown().await.unwrap();
}

async fn wait_for_phase(backend: &OpenLessBackend, phase: DictationPhase) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while backend.snapshot().dictation.phase != phase { tokio::task::yield_now().await; }
    }).await.unwrap();
}

#[tokio::test]
async fn pause_then_resume_during_voice_start_preserves_and_finishes_the_body() {
    let (_dir, backend, runtime, engine) = fixture();
    enable(&backend, HotkeyMode::Hold);
    backend.start().await.unwrap();
    click(&backend, 1).await;
    let delay = Arc::new(Semaphore::new(0));
    *runtime.engine_start_delay.lock().unwrap() = Some(delay.clone());
    let generation = backend.latest_voice_activation_state().unwrap().activation_generation;
    backend.handle_voice_activation_signal(VoiceActivationSignal::WakeDetected {
        activation_generation: generation,
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while engine.actions().is_empty() { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Starting);
    for edge in edges(2) { backend.accept_voice_activation_hotkey_edge(edge); }
    assert!(runtime.paused.lock().unwrap().1);
    for edge in edges(3) { backend.accept_voice_activation_hotkey_edge(edge); }
    assert!(!runtime.paused.lock().unwrap().1);
    let reconciliation = {
        let backend = backend.clone();
        tokio::spawn(async move { backend.reconcile_voice_activation_hotkey().await })
    };
    delay.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), reconciliation).await.unwrap().unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while backend.list_history().unwrap().is_empty()
            || backend.latest_voice_activation_state().unwrap().phase != VoiceActivationPhase::ListeningForWake
        { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(backend.list_history().unwrap()[0].final_text, "body");
    assert!(!engine.actions().iter().any(|action|
        matches!(action, openless_core::testing::FixtureEngineAction::Cancel(_))));
    backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn pausing_an_already_finalizing_voice_body_restores_durable_paused_status() {
    let (_dir, backend, runtime, _) = fixture();
    enable(&backend, HotkeyMode::Toggle);
    backend.start().await.unwrap();
    click(&backend, 1).await;
    let generation = backend.latest_voice_activation_state().unwrap().activation_generation;
    backend.handle_voice_activation_signal(VoiceActivationSignal::WakeDetected {
        activation_generation: generation,
    });
    wait_for_phase(&backend, DictationPhase::Recording).await;
    let delay = Arc::new(Semaphore::new(0));
    *runtime.engine_finish_delay.lock().unwrap() = Some(delay.clone());
    let finish = {
        let backend = backend.clone();
        tokio::spawn(async move { backend.stop_dictation().await })
    };
    wait_for_phase(&backend, DictationPhase::Transcribing).await;
    click(&backend, 2).await;
    delay.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), finish).await.unwrap().unwrap().unwrap();
    assert_eq!(backend.latest_voice_activation_state().unwrap().phase, VoiceActivationPhase::Paused);
    assert_eq!(backend.list_history().unwrap()[0].final_text, "body");
    assert!(runtime.paused.lock().unwrap().1);
    backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn pause_preserves_a_staged_body_before_handoff_has_a_formal_session() {
    let (_dir, backend, runtime, engine) = fixture();
    enable(&backend, HotkeyMode::Toggle);
    backend.start().await.unwrap();
    click(&backend, 1).await;
    let delay = Arc::new(Semaphore::new(0));
    *runtime.handoff_delay.lock().unwrap() = Some(delay.clone());
    let generation = backend.latest_voice_activation_state().unwrap().activation_generation;
    backend.handle_voice_activation_signal(VoiceActivationSignal::WakeDetected {
        activation_generation: generation,
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !runtime.handoff_started.load(Ordering::SeqCst) { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert!(backend.snapshot().dictation.session_id.is_none());
    click(&backend, 2).await;
    assert!(runtime.paused.lock().unwrap().1);
    assert!(backend.voice_activation_generation_is_current(generation));
    assert_eq!(runtime.stopped.load(Ordering::SeqCst), 0);
    delay.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), async {
        while backend.list_history().unwrap().is_empty()
            || backend.latest_voice_activation_state().unwrap().phase != VoiceActivationPhase::Paused
        { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(backend.list_history().unwrap()[0].final_text, "body");
    assert!(!engine.actions().iter().any(|action|
        matches!(action, openless_core::testing::FixtureEngineAction::Cancel(_))));
    assert_eq!(runtime.configured.load(Ordering::SeqCst), 1);
    backend.shutdown().await.unwrap();
}
