//! Exercise the production host lifecycle without native windows or microphones.
#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use openless_core::ports::{DictationEngine, EngineFailure, EngineProgressSink, EngineResult};
use openless_core::testing::{
    FixtureDictationEngine, FixtureEngineAction, FixtureTextInserter, RecordingHostActions,
};
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, DictationContext, DictationPhase,
    InMemoryCredentialStore, InsertOutcome, OpenLessBackend, SessionId, TokioTaskSpawner,
    VoiceEditPhase,
};
use tokio::sync::Semaphore;

// Only history timestamps need chrono; keep this fixture independent of native Host dependencies.
extern crate self as chrono;
pub struct Utc;
impl Utc {
    pub fn now() -> Self {
        Self
    }
    pub fn to_rfc3339(&self) -> String {
        "2026-10-07T00:00:00Z".into()
    }
}

mod types {
    pub use openless_core::shared_types::InsertStatus;
}

#[path = "../../src/insertion/clipboard_delivery.rs"]
mod clipboard_delivery;

#[derive(Default)]
struct ClipboardState {
    text: Option<String>,
    sequence: u64,
    dispatched: Vec<String>,
}

struct ClipboardAdapter(Arc<Mutex<ClipboardState>>);

impl clipboard_delivery::ClipboardDelivery for ClipboardAdapter {
    fn text(&mut self) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().text.clone())
    }
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        let mut clipboard = self.0.lock().unwrap();
        clipboard.text = Some(text.into());
        clipboard.sequence += 1;
        Ok(())
    }
    fn sequence(&self) -> Option<u64> {
        Some(self.0.lock().unwrap().sequence)
    }
    fn paste(&mut self) -> Result<(), clipboard_delivery::DispatchError> {
        let mut clipboard = self.0.lock().unwrap();
        let text = clipboard.text.clone().unwrap();
        clipboard.dispatched.push(text);
        Ok(())
    }
    fn wait_for_consumption(&mut self) {}
}

struct HostLock<T>(Mutex<T>);
impl<T> HostLock<T> {
    fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}

#[derive(Default)]
struct FakeHost(AtomicBool);
impl FakeHost {
    fn set_voice_edit_interactive(&self, interactive: bool) {
        self.0.store(interactive, Ordering::SeqCst);
    }
}
#[derive(Default)]
struct FakeInserter(Arc<Mutex<ClipboardState>>);
impl FakeInserter {
    fn insert_guarded<T>(
        &self,
        text: &str,
        restore: bool,
        _: T,
        expected: Option<u64>,
        still_current: impl FnMut() -> bool,
    ) -> types::InsertStatus {
        clipboard_delivery::insert_with(
            &mut ClipboardAdapter(Arc::clone(&self.0)),
            text,
            restore,
            expected,
            still_current,
        )
    }
}
struct Inner {
    backend: Arc<OpenLessBackend>,
    voice_edit_host: HostLock<voice_edit_session::VoiceEditHostState>,
    host: FakeHost,
    inserter: FakeInserter,
}

#[path = "../../src/coordinator/voice_edit_session.rs"]
mod voice_edit_session;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static WRITES: AtomicUsize = AtomicUsize::new(0);
static CAPTURE_GATE: Mutex<Option<Arc<CaptureGate>>> = Mutex::new(None);
static CAPTURE_SELECTION: Mutex<Option<openless_core::TextSelection>> = Mutex::new(None);
static CAPTURE_TEXT: Mutex<Option<String>> = Mutex::new(None);
static CURRENT_TARGET: Mutex<Option<host_document::NativeVoiceEditTarget>> = Mutex::new(None);
static TARGET_PREPARED: Mutex<Option<Arc<Semaphore>>> = Mutex::new(None);

struct CaptureGate {
    entered: Semaphore,
    released: Mutex<bool>,
    changed: Condvar,
}
impl CaptureGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Semaphore::new(0),
            released: Mutex::new(false),
            changed: Condvar::new(),
        })
    }
    fn capture(&self) {
        self.entered.add_permits(1);
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.changed.wait(released).unwrap();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

struct ReleaseCaptureOnDrop(Arc<CaptureGate>);
impl Drop for ReleaseCaptureOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

mod host_document {
    use super::*;

    #[derive(Clone, PartialEq, Eq)]
    pub(crate) struct NativeVoiceEditTarget {
        pub control: u64,
        pub text: String,
        pub selection: (usize, usize),
    }

    pub(crate) fn capture_voice_edit_target() -> Result<
        (
            String,
            Option<openless_core::TextSelection>,
            NativeVoiceEditTarget,
        ),
        String,
    > {
        let gate = CAPTURE_GATE.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.capture();
        }
        let text = CAPTURE_TEXT
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| "original".into());
        let target = NativeVoiceEditTarget {
            control: 1,
            selection: (text.encode_utf16().count(), text.encode_utf16().count()),
            text: text.clone(),
        };
        *CURRENT_TARGET.lock().unwrap() = Some(target.clone());
        Ok((text, CAPTURE_SELECTION.lock().unwrap().take(), target))
    }

    pub(crate) fn apply_voice_edit_target(
        target: &NativeVoiceEditTarget,
        text: &str,
        insert: impl FnOnce(&str, &mut dyn FnMut() -> bool) -> Result<(), String>,
    ) -> Result<(), String> {
        {
            let mut current = CURRENT_TARGET.lock().unwrap();
            if current.as_ref() != Some(target) {
                return Err("voiceEditTargetChanged".into());
            }
            // Model the native selection setup that happens before the legacy inserter waits.
            current.as_mut().unwrap().selection = (0, target.text.encode_utf16().count());
        }
        let selected_target = CURRENT_TARGET.lock().unwrap().clone();
        WRITES.fetch_add(1, Ordering::SeqCst);
        if let Some(prepared) = TARGET_PREPARED.lock().unwrap().take() {
            prepared.add_permits(1);
        }
        insert(text, &mut || {
            *CURRENT_TARGET.lock().unwrap() == selected_target
        })
    }
}

struct AsyncGate {
    entered: Semaphore,
    resume: Semaphore,
}
impl AsyncGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Semaphore::new(0),
            resume: Semaphore::new(0),
        })
    }
    async fn block(&self) {
        self.entered.add_permits(1);
        self.resume.acquire().await.unwrap().forget();
    }
    fn release(&self) {
        self.resume.add_permits(1);
    }
}

type Task<T> = Pin<Box<dyn Future<Output = T> + Send>>;
struct GatedEngine {
    fixture: FixtureDictationEngine,
    start_gate: Mutex<Option<Arc<AsyncGate>>>,
    finish_gate: Mutex<Option<Arc<AsyncGate>>>,
}
impl DictationEngine for GatedEngine {
    fn start(
        &self,
        id: SessionId,
        context: Arc<DictationContext>,
        progress: Arc<dyn EngineProgressSink>,
    ) -> Task<Result<(), BackendError>> {
        let gate = self.start_gate.lock().unwrap().take();
        let result = self.fixture.start(id, context, progress);
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.block().await;
            }
            result.await
        })
    }
    fn finish(
        &self,
        id: SessionId,
        progress: Arc<dyn EngineProgressSink>,
    ) -> Task<Result<EngineResult, EngineFailure>> {
        let gate = self.finish_gate.lock().unwrap().take();
        let result = self.fixture.finish(id, progress);
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.block().await;
            }
            result.await
        })
    }
    fn update_context(
        &self,
        id: SessionId,
        context: Arc<DictationContext>,
    ) -> Task<Result<(), BackendError>> {
        self.fixture.update_context(id, context)
    }
    fn feed_audio(&self, id: SessionId, pcm: &[u8]) -> Result<(), BackendError> {
        self.fixture.feed_audio(id, pcm)
    }
    fn cancel(&self, id: SessionId) -> Task<Result<(), BackendError>> {
        self.fixture.cancel(id)
    }
}

fn fixture() -> (tempfile::TempDir, Arc<Inner>, Arc<GatedEngine>) {
    fixture_with_transcript("spoken")
}

fn fixture_with_transcript(transcript: &str) -> (tempfile::TempDir, Arc<Inner>, Arc<GatedEngine>) {
    let directory = tempfile::tempdir().unwrap();
    let mut preferences = openless_core::shared_types::UserPreferences::default();
    preferences.voice_edit_enabled = true;
    // This fixture supplies a fixed history timestamp; retention must not depend on today's date.
    preferences.history_retention_days = 0;
    openless_core::PreferencesStore::open(directory.path().join("preferences.json"))
        .unwrap()
        .set(preferences)
        .unwrap();
    let engine = Arc::new(GatedEngine {
        fixture: FixtureDictationEngine::successful(transcript, transcript),
        start_gate: Mutex::new(None),
        finish_gate: Mutex::new(None),
    });
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: directory.path().into(),
            ..BackendConfig::default()
        },
        BackendDependencies {
            host_actions: Arc::new(RecordingHostActions::default()),
            text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
            dictation_engine: engine.clone(),
            task_spawner: Arc::new(TokioTaskSpawner),
            credential_store: Arc::new(InMemoryCredentialStore::default()),
            voice_activation_runtime: None,
            services: openless_core::BackendServices::unsupported(),
            local_asr_runtime: None,
            selection_runtime: None,
            selection_polisher: None,
            qa_runtime: None,
            marketplace_config: None,
        },
    )
    .unwrap();
    (
        directory,
        Arc::new(Inner {
            backend: Arc::new(backend),
            voice_edit_host: HostLock(Mutex::new(Default::default())),
            host: FakeHost::default(),
            inserter: FakeInserter::default(),
        }),
        engine,
    )
}

async fn entered(semaphore: &Semaphore) {
    tokio::time::timeout(Duration::from_secs(5), semaphore.acquire())
        .await
        .expect("production operation should reach the gate")
        .unwrap()
        .forget();
}

#[tokio::test]
async fn invalid_commit_never_writes_and_completed_commit_only_writes_once() {
    let _serial = SERIAL.lock().await;
    WRITES.store(0, Ordering::SeqCst);
    let (_directory, inner, _) = fixture();
    let id = voice_edit_session::start(&inner).await.unwrap().session_id;
    assert!(voice_edit_session::commit(&inner, id).await.is_err());
    assert_eq!(WRITES.load(Ordering::SeqCst), 0);
    voice_edit_session::finish_dictation(&inner, id)
        .await
        .unwrap();
    assert_eq!(
        voice_edit_session::commit(&inner, id).await.unwrap().phase,
        VoiceEditPhase::Completed
    );
    assert!(voice_edit_session::commit(&inner, id).await.is_err());
    assert_eq!(WRITES.load(Ordering::SeqCst), 1);
    inner.backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn commit_revalidates_prepared_native_target_after_waiting_for_clipboard_delivery() {
    let _serial = SERIAL.lock().await;
    for change in ["control", "selection", "text", "unchanged"] {
        let (_directory, inner, _) = fixture();
        let id = voice_edit_session::start(&inner).await.unwrap().session_id;
        voice_edit_session::finish_dictation(&inner, id)
            .await
            .unwrap();
        let before = voice_edit_session::snapshot(&inner).unwrap();
        let history_before = inner.backend.list_history().unwrap().len();
        let prepared = Arc::new(Semaphore::new(0));
        *TARGET_PREPARED.lock().unwrap() = Some(Arc::clone(&prepared));
        let gate_entered = Arc::new(Semaphore::new(0));
        let notify = Arc::clone(&gate_entered);
        let (release, wait) = std::sync::mpsc::channel();
        // Another host delivery owns the shared gate while voice-edit prepares its target.
        let holder = std::thread::spawn(move || {
            let _gate = clipboard_delivery::lock();
            notify.add_permits(1);
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        entered(&gate_entered).await;
        let writer = Arc::clone(&inner);
        let commit = tokio::spawn(async move { voice_edit_session::commit(&writer, id).await });
        entered(&prepared).await;
        let changed_target = {
            let mut current = CURRENT_TARGET.lock().unwrap();
            let current = current.as_mut().unwrap();
            match change {
                "control" => current.control += 1,
                "selection" => current.selection = (1, 1),
                "text" => current.text.push_str(" user edit"),
                "unchanged" => {}
                _ => unreachable!(),
            }
            current.clone()
        };
        // Commit reservation continues to reject cancellation, as before this fix.
        assert!(voice_edit_session::cancel(&inner, Some(id)).await.is_err());
        release.send(()).unwrap();
        holder.join().unwrap();
        let outcome = commit.await.unwrap();
        let snapshot = voice_edit_session::snapshot(&inner).unwrap();
        {
            let clipboard = inner.inserter.0.lock().unwrap();
            if change == "unchanged" {
                assert_eq!(outcome.unwrap().phase, VoiceEditPhase::Completed);
                assert_eq!(clipboard.dispatched, [before.context.unwrap().preview]);
                assert_eq!(
                    inner.backend.list_history().unwrap().len(),
                    history_before
                );
            } else {
                assert!(
                    outcome.is_err(),
                    "{change} while waiting must reject the commit"
                );
                assert!(clipboard.dispatched.is_empty());
                assert_eq!(
                    clipboard.sequence, 0,
                    "rejection must not even borrow the clipboard"
                );
                assert_eq!(snapshot, before, "keep the complete draft retryable");
                assert_eq!(inner.backend.list_history().unwrap().len(), history_before);
                assert!(
                    CURRENT_TARGET.lock().unwrap().as_ref() == Some(&changed_target),
                    "never reset the user's changed control, text or range"
                );
            }
        }
        inner.backend.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn empty_initial_draft_releases_the_host_for_another_overlay_recording() {
    let _serial = SERIAL.lock().await;
    let (_directory, inner, _) = fixture_with_transcript("");
    inner.backend.start().await.unwrap();
    *CAPTURE_TEXT.lock().unwrap() = Some(String::new());
    inner
        .backend
        .start_dictation_with_options(openless_core::DictationStartOptions {
            output_target: openless_core::DictationOutputTarget::Undecided,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(voice_edit_session::from_overlay(&inner).await.is_err());
    assert_eq!(
        voice_edit_session::snapshot(&inner).unwrap().phase,
        VoiceEditPhase::Cancelled
    );
    assert!(inner.voice_edit_host.lock().dictation_session_id.is_none());
    let next = voice_edit_session::start(&inner).await.unwrap();
    voice_edit_session::cancel(&inner, Some(next.session_id))
        .await
        .unwrap();
    inner.backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn overlay_handoff_reuses_recording_and_preserves_audio_without_native_insertion() {
    let _serial = SERIAL.lock().await;
    WRITES.store(0, Ordering::SeqCst);
    let (_directory, inner, engine) = fixture();
    inner.backend.start().await.unwrap();
    let recording = inner
        .backend
        .start_dictation_with_options(openless_core::DictationStartOptions {
            output_target: openless_core::DictationOutputTarget::Undecided,
            ..Default::default()
        })
        .await
        .unwrap();
    let draft = voice_edit_session::from_overlay(&inner).await.unwrap();
    assert_eq!(draft.phase, VoiceEditPhase::DraftReady);
    assert_eq!(draft.context.unwrap().preview, "originalspoken");
    assert_eq!(
        engine
            .fixture
            .actions()
            .iter()
            .filter(|action| { matches!(action, FixtureEngineAction::Start(_)) })
            .count(),
        1
    );
    assert!(engine
        .fixture
        .actions()
        .contains(&FixtureEngineAction::Finish(recording)));
    let history = inner.backend.list_history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].raw_transcript, "originalspoken");
    assert_eq!(history[0].final_text, "originalspoken");
    assert_eq!(history[0].insert_status, openless_core::HistoryInsertStatus::NotRequested);
    assert_eq!(WRITES.load(Ordering::SeqCst), 0);
    assert!(inner.voice_edit_host.lock().dictation_session_id.is_none());
    voice_edit_session::cancel(&inner, Some(draft.session_id))
        .await
        .unwrap();
    inner.backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelled_capture_cannot_install_recording_over_the_next_session() {
    let _serial = SERIAL.lock().await;
    let (_directory, inner, engine) = fixture();
    let gate = CaptureGate::new();
    let _release = ReleaseCaptureOnDrop(gate.clone());
    *CAPTURE_GATE.lock().unwrap() = Some(gate.clone());
    let first = tokio::spawn({
        let inner = inner.clone();
        async move { voice_edit_session::start(&inner).await }
    });
    entered(&gate.entered).await;
    voice_edit_session::cancel(&inner, None).await.unwrap();
    let current = voice_edit_session::start(&inner).await.unwrap();
    let recording = inner.voice_edit_host.lock().dictation_session_id;
    gate.release();
    assert!(first.await.unwrap().is_err());
    assert_eq!(voice_edit_session::snapshot(&inner).unwrap(), current);
    assert_eq!(inner.voice_edit_host.lock().dictation_session_id, recording);
    assert_eq!(engine.fixture.actions().len(), 1);
    voice_edit_session::cancel(&inner, Some(current.session_id))
        .await
        .unwrap();
    inner.backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn late_recorder_start_is_cancelled_without_attaching_to_a_newer_session() {
    let _serial = SERIAL.lock().await;
    let (_directory, inner, engine) = fixture();
    let gate = AsyncGate::new();
    *engine.start_gate.lock().unwrap() = Some(gate.clone());
    let first = tokio::spawn({
        let inner = inner.clone();
        async move { voice_edit_session::start(&inner).await }
    });
    entered(&gate.entered).await;
    let original = voice_edit_session::snapshot(&inner).unwrap().session_id;
    voice_edit_session::cancel(&inner, Some(original))
        .await
        .unwrap();
    assert!(voice_edit_session::start(&inner).await.is_err());
    let newer = voice_edit_session::snapshot(&inner).unwrap();
    assert_ne!(newer.session_id, original);
    gate.release();
    assert!(first.await.unwrap().is_err());
    assert_eq!(voice_edit_session::snapshot(&inner).unwrap(), newer);
    assert!(inner.voice_edit_host.lock().dictation_session_id.is_none());
    assert!(engine
        .fixture
        .actions()
        .iter()
        .any(|action| matches!(action, FixtureEngineAction::Cancel(_))));
    assert_eq!(
        inner.backend.snapshot().dictation.phase,
        DictationPhase::Idle
    );
    let next = voice_edit_session::start(&inner).await.unwrap();
    voice_edit_session::cancel(&inner, Some(next.session_id))
        .await
        .unwrap();
    inner.backend.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelling_during_initial_or_instruction_asr_releases_the_backend() {
    let _serial = SERIAL.lock().await;
    for instruction in [false, true] {
        let (_directory, inner, engine) = fixture();
        let id = voice_edit_session::start(&inner).await.unwrap().session_id;
        if instruction {
            voice_edit_session::finish_dictation(&inner, id)
                .await
                .unwrap();
            voice_edit_session::start_instruction(&inner, id)
                .await
                .unwrap();
        }
        let recording = inner.voice_edit_host.lock().dictation_session_id.unwrap();
        let gate = AsyncGate::new();
        *engine.finish_gate.lock().unwrap() = Some(gate.clone());
        let finishing = tokio::spawn({
            let inner = inner.clone();
            async move {
                if instruction {
                    voice_edit_session::finish_instruction(&inner, id).await
                } else {
                    voice_edit_session::finish_dictation(&inner, id).await
                }
            }
        });
        entered(&gate.entered).await;
        assert_eq!(
            inner.backend.snapshot().dictation.phase,
            DictationPhase::Transcribing
        );
        voice_edit_session::cancel(&inner, Some(id)).await.unwrap();
        assert!(engine
            .fixture
            .actions()
            .contains(&FixtureEngineAction::Cancel(recording)));
        assert_eq!(
            inner.backend.snapshot().dictation.phase,
            DictationPhase::Idle
        );
        gate.release();
        assert!(finishing.await.unwrap().is_err());
        assert_eq!(
            voice_edit_session::snapshot(&inner).unwrap().phase,
            VoiceEditPhase::Cancelled
        );
        assert!(voice_edit_session::can_close(&inner, Some(id)).is_ok());
        let next = voice_edit_session::start(&inner).await.unwrap();
        assert_ne!(next.session_id, id);
        assert!(voice_edit_session::start_instruction(&inner, id)
            .await
            .is_err());
        assert!(voice_edit_session::commit(&inner, id).await.is_err());
        assert!(voice_edit_session::cancel(&inner, Some(id)).await.is_err());
        assert_eq!(voice_edit_session::snapshot(&inner).unwrap(), next);
        voice_edit_session::cancel(&inner, Some(next.session_id))
            .await
            .unwrap();
        inner.backend.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn frozen_selection_history_survives_cancel_and_commit_without_instruction_text() {
    let _serial = SERIAL.lock().await;
    for commit in [false, true] {
        let original = format!(" \t{} UNIQUE_MIDDLE {}\n ", "😀".repeat(9000), "中".repeat(9000));
        *CAPTURE_TEXT.lock().unwrap() = Some(format!("prefix{original}suffix"));
        *CAPTURE_SELECTION.lock().unwrap() = Some(openless_core::TextSelection {
            start: 6, end: 6 + original.chars().count() as u32,
        });
        let (_directory, inner, _) = fixture();
        let id = voice_edit_session::start(&inner).await.unwrap().session_id;
        let before = inner.backend.list_history().unwrap().into_iter().filter(|entry| entry.id == id.to_string()).collect::<Vec<_>>();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].raw_transcript, original);
        assert!(before[0].final_text.is_empty());
        voice_edit_session::finish_dictation(&inner, id).await.unwrap();
        let draft = voice_edit_session::snapshot(&inner).unwrap().context.unwrap().preview;
        assert_eq!(draft, original);
        // Model instructions are stored in turns, never in rawTranscript.
        if commit {
            voice_edit_session::commit(&inner, id).await.unwrap();
        } else {
            voice_edit_session::cancel(&inner, Some(id)).await.unwrap();
        }
        let history = inner.backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, id.to_string());
        assert_eq!(history[0].raw_transcript, original);
        assert_eq!(history[0].final_text, draft);
        assert_eq!(history[0].insert_status, if commit { openless_core::HistoryInsertStatus::PasteSent } else { openless_core::HistoryInsertStatus::NotRequested });
        inner.backend.shutdown().await.unwrap();
    }
}
