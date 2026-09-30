//! Dictation coordinator.
//!
//! Mirrors the Swift `DictationCoordinator` state machine. Single owner of
//! session state. Receives hotkey edges, drives recorder + ASR + polish +
//! insertion, persists history, emits `capsule:state` events to the capsule
//! window.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

#[cfg(target_os = "windows")]
use crate::asr::local::{FoundryLocalRuntime, SherpaOnnxRuntime};
use crate::combo_hotkey::{ComboHotkeyError, ComboHotkeyEvent, ComboHotkeyMonitor};
use crate::hotkey::{HotkeyEvent, HotkeyMonitor};
use crate::insertion::TextInserter;
use crate::persistence::{
    ActivityStore, CorrectionRuleStore, DictionaryStore, HistoryStore, PreferencesStore,
    StylePackStore,
};
use crate::qa_adapter::TauriQaHostContext;

use crate::qa_hotkey::{QaHotkeyError, QaHotkeyEvent, QaHotkeyMonitor};
use crate::types::{
    CapsulePayload, CapsuleState, CapsuleStyle, HotkeyCapability, HotkeyStatus, HotkeyStatusState,
};

mod capsule_focus;
#[path = "coordinator/dictation_core.rs"]
mod dictation;
mod hotkey_loops;
mod native_dictation_key;
mod qa;
mod restore_runtime;
#[cfg(all(not(mobile), target_os = "windows"))]
pub(crate) mod selection_voice_session;
use capsule_focus::*;
pub(crate) use capsule_focus::{
    capture_external_focus_target, capture_focus_target, capture_frontmost_app,
    restore_focus_target_if_possible,
};
use hotkey_loops::*;
use native_dictation_key::try_install_mouse_dictation;

// Instance-local Less Computer replay source used by the compatibility command.
pub(crate) use dictation::{less_computer_event_replay_after, LessComputerEventReplay};

pub(super) fn qa_event_target() -> &'static str {
    #[cfg(target_os = "android")]
    {
        "main"
    }
    #[cfg(not(target_os = "android"))]
    {
        "qa"
    }
}

#[cfg(any(debug_assertions, test))]
use dictation::{handle_pressed, handle_released};
use dictation::{handle_pressed_edge, handle_released_edge, handle_trigger_combined};
use qa::handle_qa_hotkey_pressed;

/// Window size of the vocabulary suggestion card (logical points).
///
/// Showing the card must shrink the capsule window to this size — see the
/// cursor-passthrough notes in [`show_vocab_suggestion_card`].
const VOCAB_CARD_WIDTH: f64 = 320.0;
/// Height of one suggestion row: check/cross buttons 28pt + 8pt row spacing,
/// aligned with `VocabSuggestionCard.tsx`.
const VOCAB_CARD_ROW_HEIGHT: f64 = 36.0;
/// Title row + card padding + margin left for the drop shadow.
const VOCAB_CARD_CHROME_HEIGHT: f64 = 72.0;
/// Margin between the card and the right screen edge.
const VOCAB_CARD_EDGE_MARGIN: f64 = 24.0;

/// Pops the "remember this word?" card at the capsule's position.
///
/// Reuses the capsule window instead of creating a new one: multi-monitor
/// positioning, Space attachment, and the nonactivating panel were all tuned
/// the hard way; a new window would repeat those pitfalls.
///
/// One thing must change: the capsule is normally fully cursor-transparent
/// (`set_ignore_cursor_events(true)`) because it floats over other apps and
/// must not block clicks on what is underneath. The card must be clickable,
/// so passthrough is temporarily disabled — but once a transparent window
/// stops ignoring the cursor, even its transparent parts intercept the mouse.
/// So the window shrinks to the card's actual size while shown, and is
/// restored when hidden.
fn show_vocab_suggestion_card(inner: &Arc<Inner>) {
    let pending = inner.backend.pending_corrections();
    if pending.is_empty() {
        return;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return;
    };
    let height = VOCAB_CARD_CHROME_HEIGHT + VOCAB_CARD_ROW_HEIGHT * pending.len() as f64;
    let inner_for_main = Arc::clone(inner);
    let _ = capsule.run_on_main_thread(move |capsule| {
        let inner = inner_for_main;
        // Last gate: never show the card while dictation is not Idle.
        //
        // Upstream checks (observer generation, non-empty `pending_corrections`)
        // are check-then-act reads made before a cross-thread hop; a Core
        // session can start — even finish — while queued. Only this final
        // check, the last point before touching the window, tests the real
        // invariant: the card and the recording capsule share one window, so
        // showing the card mid-dictation would wipe out that session's capsule.
        //
        if inner.backend.snapshot().dictation.phase != openless_core::DictationPhase::Idle
            || inner.backend.less_computer_active_session().is_some()
        {
            log::debug!("[vocab-card] suppressed: a dictation session is in flight");
            inner.backend.dismiss_pending_corrections();
            return;
        }
        inner.vocab_card_visible.store(true, Ordering::SeqCst);
        // The card must be clickable, so cursor passthrough must be disabled.
        // Android has no capsule window and tauri's set_ignore_cursor_events
        // does not exist there (same handling as capsule_focus.rs).
        #[cfg(not(mobile))]
        if let Err(e) = capsule.set_cursor_passthrough(false) {
            log::warn!("[vocab-card] set_ignore_cursor_events(false) failed: {e}");
        }
        if let Err(e) = capsule.set_size(VOCAB_CARD_WIDTH, height) {
            log::warn!("[vocab-card] resize failed: {e}");
        }
        if let Err(e) =
            capsule.position_vocab_card(VOCAB_CARD_WIDTH, height, VOCAB_CARD_EDGE_MARGIN)
        {
            log::warn!("[vocab-card] position failed: {e}");
        }
        // Positioning ditto: the dedup cache in `maybe_position_capsule_bottom_center`
        // only tracks "monitor + translation state" and knows nothing about
        // this move. Without invalidation, the next recording would judge
        // "nothing changed" from the same monitor snapshot, skip repositioning,
        // and leave the capsule in the bottom-right corner the card moved it
        // to.
        capsule.invalidate_layout();
        capsule.show_for_recording(true);
        #[cfg(target_os = "macos")]
        capsule.restore_main_window_key_if_active();
    });
}

/// Dismisses the card and returns the window fully to the capsule.
///
/// Reached from every dismissal path — user accepts/rejects, the 10s timeout,
/// a new dictation session starting.
///
/// Must be a no-op when no card is showing: new dictation sessions call it,
/// and unconditionally hiding the window would race `emit_capsule`'s show for
/// the same window — the capsule flickers and the user thinks the hotkey is
/// broken.
fn hide_vocab_suggestion_card(inner: &Arc<Inner>) {
    inner.backend.dismiss_pending_corrections();
    if !inner.vocab_card_visible.swap(false, Ordering::SeqCst) {
        return;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return;
    };
    let _ = capsule.run_on_main_thread(move |capsule| {
        // Hide before changing geometry: restoring size and position while the
        // window is visible can composite a frame with the card stretched wide
        // and flying across half the screen.
        let _ = capsule.hide();
        // Passthrough must be restored, or the capsule keeps blocking that
        // area at the bottom of the screen.
        #[cfg(not(mobile))]
        if let Err(e) = capsule.set_cursor_passthrough(true) {
            log::warn!("[vocab-card] restoring cursor passthrough failed: {e}");
        }
        // Size must be restored too — the card shrank the window to its own
        // size, and without this the next capsule would be squeezed into a
        // 320×108 window, effectively invisible.
        let bounds = crate::capsule_window_bounds(false);
        if let Err(e) = capsule.set_size(bounds.width, bounds.height) {
            log::warn!("[vocab-card] restoring capsule size failed: {e}");
        }
        // Position must be restored too — the card moved the window to the
        // bottom-right while the capsule sits bottom-center. Restoring size
        // alone would put the next recording capsule in the bottom-right
        // corner.
        //
        // Both cache invalidation and this repositioning are needed:
        // invalidation guarantees the next emit_capsule recomputes even if this
        // repositioning fails; repositioning guarantees the window is already
        // in the right place even if some path shows it without emit_capsule.
        capsule.invalidate_layout();
        if let Err(e) = capsule.position_capsule_bottom_center(false) {
            log::warn!("[vocab-card] restoring capsule position failed: {e}");
        }
    });
}

/// Window width of the insert-fallback card (logical points). Slightly wider
/// than the vocab card — this one holds a full passage.
const FALLBACK_CARD_WIDTH: f64 = 360.0;
/// Safe height before the webview's first render. The real height is measured
/// from the card DOM and reported back over IPC.
const FALLBACK_CARD_INITIAL_HEIGHT: f64 = 260.0;
/// Native safety bounds for the size IPC; not a CSS layout rule.
const FALLBACK_CARD_MIN_HEIGHT: f64 = 96.0;
const FALLBACK_CARD_MAX_HEIGHT: f64 = 320.0;

fn validated_fallback_card_height(
    active_presentation_id: Option<u64>,
    presentation_id: u64,
    height: f64,
) -> Result<Option<f64>, String> {
    if !height.is_finite() {
        return Err("fallback card height must be finite".into());
    }
    if active_presentation_id != Some(presentation_id) {
        return Ok(None);
    }
    Ok(Some(
        height
            .ceil()
            .clamp(FALLBACK_CARD_MIN_HEIGHT, FALLBACK_CARD_MAX_HEIGHT),
    ))
}

/// Shows the text with a copy button when it could not be inserted into the
/// target app.
///
/// Needed because in these scenarios the only fallback is copying the text to
/// the clipboard, which depends on a default-off setting and gives the user no
/// hint that the text is there — the screen would show nothing or a partial
/// result.
///
/// The window mechanism mirrors [`show_vocab_suggestion_card`] (reuse the
/// capsule window, disable passthrough, shrink size, bottom-right position);
/// see there for rationale. One extra piece: this card appears exactly when
/// the session wraps up, and the wrap-up schedules `schedule_capsule_idle` →
/// `hide()`, so that hide must recognize the card and yield.
fn show_insert_fallback_card(inner: &Arc<Inner>, text: String, reason: &'static str) {
    if text.trim().is_empty() {
        return;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return;
    };
    let inner_for_main = Arc::clone(inner);
    let _ = capsule.run_on_main_thread(move |capsule| {
        let inner = inner_for_main;
        // Same gate and rationale as the vocab card: never touch this window
        // while dictation is not Idle, or the in-flight session's capsule is
        // destroyed. The wrap-up path sets the phase back to Idle before
        // reaching here.
        if inner.backend.snapshot().dictation.phase != openless_core::DictationPhase::Idle
            || inner.backend.less_computer_active_session().is_some()
        {
            log::debug!("[fallback-card] suppressed: a dictation session is in flight");
            return;
        }
        let presentation_id = inner.host.begin_insert_fallback_card();
        let payload = crate::types::InsertFallbackCardPayload {
            text,
            reason: reason.to_string(),
            presentation_id,
        };
        #[cfg(not(mobile))]
        if let Err(e) = capsule.set_cursor_passthrough(false) {
            log::warn!("[fallback-card] set_ignore_cursor_events(false) failed: {e}");
        }
        if let Err(e) = capsule.set_size(FALLBACK_CARD_WIDTH, FALLBACK_CARD_INITIAL_HEIGHT) {
            log::warn!("[fallback-card] resize failed: {e}");
        }
        if let Err(e) =
            capsule.position_fallback_card(FALLBACK_CARD_WIDTH, FALLBACK_CARD_INITIAL_HEIGHT)
        {
            log::warn!("[fallback-card] position failed: {e}");
        }
        // Positioning ditto: the dedup cache in `maybe_position_capsule_bottom_center`
        // only tracks "monitor + translation state" and knows nothing about
        // this move. Without invalidation the next recording would judge
        // "nothing changed", skip repositioning, and leave the capsule in the
        // bottom-right corner.
        capsule.invalidate_layout();
        inner.host.emit_insert_fallback(&payload);
        capsule.show_for_recording(true);
        #[cfg(target_os = "macos")]
        capsule.restore_main_window_key_if_active();
        log::info!(
            "[fallback-card] shown: reason={reason} chars={}",
            payload.text.chars().count()
        );
    });
}

fn report_insert_fallback_card_height(
    inner: &Arc<Inner>,
    presentation_id: u64,
    height: f64,
) -> Result<(), String> {
    let active_presentation_id = inner.host.active_insert_fallback_presentation_id();
    let Some(height) =
        validated_fallback_card_height(active_presentation_id, presentation_id, height)?
    else {
        return Ok(());
    };
    let Some(capsule) = inner.host.capsule_window() else {
        return Ok(());
    };
    let inner_for_main = Arc::clone(inner);
    capsule.run_on_main_thread(move |capsule| {
        if !inner_for_main
            .host
            .insert_fallback_presentation_is_current(presentation_id)
        {
            return;
        }
        if let Err(e) = capsule.set_size(FALLBACK_CARD_WIDTH, height) {
            log::warn!("[fallback-card] measured resize failed: {e}");
        }
        if let Err(e) = capsule.position_fallback_card(FALLBACK_CARD_WIDTH, height) {
            log::warn!("[fallback-card] measured position failed: {e}");
        }
    })
}

/// Dismisses the fallback card and returns the window fully to the capsule.
///
/// Same as [`hide_vocab_suggestion_card`]: must be a no-op when no card is
/// showing, or every dictation start would hide the window and race
/// `emit_capsule`'s show.
fn hide_insert_fallback_card(inner: &Arc<Inner>) {
    let _event_guard = inner.capsule_event_lock.lock();
    let (was_visible, deferred_capsule) = inner.host.dismiss_insert_fallback_card();
    if !was_visible {
        return;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return;
    };
    let host = inner.host.clone();
    let backend = Arc::clone(&inner.backend);
    let _ = capsule.run_on_main_thread(move |capsule| {
        host.clear_insert_fallback();
        // Hide before changing geometry: restoring size and position while the
        // window is visible can composite a frame with the card stretched wide
        // and flying across half the screen.
        let _ = capsule.hide();
        // Passthrough must be restored, or the capsule keeps blocking that
        // screen area.
        #[cfg(not(mobile))]
        if let Err(e) = capsule.set_cursor_passthrough(true) {
            log::warn!("[fallback-card] restoring cursor passthrough failed: {e}");
        }
        // Size must be restored too — the card shrank the window to its own
        // size, and without this the next capsule would be squeezed into a
        // card-sized window, effectively invisible.
        let bounds = crate::capsule_window_bounds(false);
        if let Err(e) = capsule.set_size(bounds.width, bounds.height) {
            log::warn!("[fallback-card] restoring capsule size failed: {e}");
        }
        // Position must be restored too — the card moved the window to the
        // bottom-right while the capsule sits bottom-center. Restoring size
        // alone would put the next recording capsule in the bottom-right
        // corner. Both cache invalidation and this repositioning are needed;
        // see `hide_vocab_suggestion_card` for rationale.
        capsule.invalidate_layout();
        if let Err(e) = capsule.position_capsule_bottom_center(false) {
            log::warn!("[fallback-card] restoring capsule position failed: {e}");
        }
        if let Some(payload) = deferred_capsule {
            // During the card, QA / Selection Polish still advance capsule
            // state but cannot touch the shared window. Re-apply the latest
            // state once the card is released; if it is Idle, the helper hides
            // normally.
            let preferences = backend.get_preferences();
            let show_capsule = payload.selection_polish || preferences.show_capsule;
            capsule.apply_capsule_payload(&payload, show_capsule, preferences.capsule_style, true);
        }
    });
}

pub struct Coordinator {
    inner: Arc<Inner>,
}

fn startup_storage_error() -> openless_core::BackendError {
    openless_core::BackendError::new(
        openless_core::BackendErrorCode::Persistence,
        "local recovery or secure storage is unavailable; restore access and restart OpenLess",
    )
    .retryable(true)
}

fn startup_sync_gate(
) -> Result<Arc<openless_core::cloud_sync_e2ee_store::SyncWriteGate>, openless_core::BackendError> {
    let directory = crate::persistence::data_dir().map_err(|_| startup_storage_error())?;
    openless_core::cloud_sync_e2ee_store::gate::open_for_data_dir(&directory)
        .map_err(|_| startup_storage_error())
}

fn startup_store<T, E>(
    startup_error: &mut Option<openless_core::BackendError>,
    open: impl FnOnce() -> Result<T, E>,
    fallback: impl FnOnce() -> T,
) -> T {
    if startup_error.is_some() {
        return fallback();
    }
    match open() {
        Ok(store) => store,
        Err(_) => {
            log::error!("[core] local store initialization failed; startup is blocked");
            *startup_error = Some(startup_storage_error());
            fallback()
        }
    }
}

fn shared_backend_from_stores(
    history: &HistoryStore,
    activity: &ActivityStore,
    prefs: &PreferencesStore,
    style_packs: &StylePackStore,
    vocab: &DictionaryStore,
    correction_rules: &CorrectionRuleStore,
    app: crate::core_adapters::AppHandleSlot,
    native_asr: crate::core_adapters::TauriNativeAsrDependencies,
    hotkey_status: Arc<Mutex<HotkeyStatus>>,
    qa_context: Arc<TauriQaHostContext>,
    startup_error: Option<openless_core::BackendError>,
) -> Arc<openless_core::OpenLessBackend> {
    let data_dir = crate::persistence::data_dir().unwrap_or_else(|error| {
        log::warn!("[core] data directory unavailable, using fallback config path: {error}");
        std::env::temp_dir().join("openless-core-fallback")
    });
    let locale = std::env::var("LANG")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "en-US".to_string());
    let config = openless_core::BackendConfig {
        cache_dir: data_dir.join("cache"),
        data_dir,
        home_dir: std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from),
        resource_dir: None,
        platform: crate::types::PlatformCapabilities::current(),
        locale,
    };
    if let Some(error) = startup_error {
        return Arc::new(openless_core::OpenLessBackend::blocked_startup(
            config, error,
        ));
    }
    let repositories = openless_core::BackendRepositories {
        preferences: prefs.core(),
        history: history.core(),
        activity: activity.core(),
        vocabulary: vocab.core(),
        correction_rules: correction_rules.core(),
        style_packs: style_packs.core(),
    };
    let backend_slot = crate::core_adapters::backend_slot();
    let mut dependencies = crate::core_adapters::backend_dependencies(
        app,
        Arc::clone(&backend_slot),
        native_asr,
        Arc::clone(&repositories.preferences),
        hotkey_status,
        qa_context,
    );
    dependencies.marketplace_config = Some(
        openless_core::MarketplaceConfig::production().with_encrypted_sync(
            openless_core::cloud_sync_e2ee::EncryptedSyncConfig {
                service_origin: openless_core::cloud_sync_e2ee::DEFAULT_SYNC_SERVICE_ORIGIN.into(),
                app_version: env!("CARGO_PKG_VERSION").into(),
            },
        ),
    );
    let backend = Arc::new(
        match openless_core::OpenLessBackend::new_with_repositories(
            config.clone(),
            dependencies,
            repositories,
        ) {
            Ok(backend) => backend,
            Err(_) => {
                log::error!(
                    "[core] shared backend initialization failed; exposing blocked startup"
                );
                openless_core::OpenLessBackend::blocked_startup(config, startup_storage_error())
            }
        },
    );
    *backend_slot.lock() = Some(Arc::downgrade(&backend));
    backend
}

/// Install the only narrow callback the QA runtime needs from the Tauri host:
/// attaching an opaque selection insertion target to a Core-owned preview.
/// The callback only captures the shared opaque-target state; the QA adapter
/// never performs a Tauri managed-state lookup back into `Coordinator`.
#[cfg(all(not(mobile), target_os = "windows"))]
fn bind_qa_selection_voice_target(
    qa_context: &Arc<TauriQaHostContext>,
    selection_voice_host: &Arc<Mutex<selection_voice_session::SelectionVoiceHostState>>,
) {
    let selection_voice_host = Arc::clone(selection_voice_host);
    qa_context.set_selection_voice_target_binder(Arc::new(move |session_id, target| {
        selection_voice_session::bind_selection_voice_target_state(
            &selection_voice_host,
            session_id,
            target,
        )
    }));
}

struct StylePackHotkeyRegistration {
    binding: crate::types::ShortcutBinding,
    _monitor: ComboHotkeyMonitor,
}

struct Inner {
    host: crate::tauri_coordinator_host::TauriCoordinatorHost,
    backend: Arc<openless_core::OpenLessBackend>,
    less_computer_voice: Mutex<Option<LessComputerHostCapture>>,
    /// The shortcut target actually installed on the host. Settings
    /// transactions update it only via an explicit target; listener
    /// install/restore must not read back uncommitted or rolled-back
    /// preferences.
    hotkey_runtime_target: Mutex<openless_core::HotkeyRuntimeTarget>,
    /// Serializes "Core settings transaction + host effect" and style-pack
    /// removal effects on the Tauri side so two commands cannot install the
    /// explicit runtime target out of order.
    settings_host_gate: Mutex<()>,
    hotkey_resume_started: AtomicBool,
    overlay_qa_handoff: tokio::sync::Mutex<()>,
    inserter: TextInserter,
    /// Whether the suggestion card is currently occupying the capsule window.
    ///
    /// Gates `hide_vocab_suggestion_card`: with no card it must do nothing, or
    /// every dictation start would hide the capsule window and race
    /// `emit_capsule`'s show for the same window.
    vocab_card_visible: AtomicBool,
    hotkey: Mutex<Option<HotkeyMonitor>>,
    hotkey_status: Arc<Mutex<HotkeyStatus>>,
    /// Webview fallback only: pairs one raw keydown/up edge with the same Core press id.
    window_hotkey_press_id: AtomicU64,
    shortcut_recording_active: AtomicBool,
    /// Press generation and pending combo event for the Less Computer modifier
    /// hotkey.
    less_computer_press_generation: AtomicU64,
    less_computer_combo_pending_press: Mutex<Option<crate::hotkey::HotkeyCombinedEdge>>,
    /// Custom combo-key listener (global-hotkey crate). Replaces the
    /// modifier-only hotkey monitor when `prefs.hotkey.trigger == Custom`;
    /// `None` means no custom combo key is used or installation has not
    /// succeeded yet.
    combo_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    side_aware_combo: Mutex<Option<crate::side_aware_combo::SideAwareComboMonitor>>,
    /// Mouse4/Mouse5 听写监听（WH_MOUSE_LL）；与 combo / side-aware 互斥。
    mouse_dictation: Mutex<Option<crate::mouse_dictation::MouseDictationMonitor>>,
    translation_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    switch_style_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    open_app_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    quick_note_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    /// Style-pack direct shortcut listeners (issue #759): pack_id → actual
    /// binding + monitor. The binding metadata lets the supervisor distinguish
    /// "same pack_id but changed keys" and keep retrying after any
    /// non-transactional registration failure until actual state matches prefs.
    style_pack_hotkeys: Mutex<std::collections::HashMap<String, StylePackHotkeyRegistration>>,
    /// Selection-polish shortcut: modifier-only reuses `HotkeyMonitor`, other
    /// combos reuse `ComboHotkeyMonitor`. Desktop (non-mobile) only.
    #[cfg(not(mobile))]
    selection_polish_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    /// Selection-voice host resources. Business session/prompt/preview state
    /// is owned exclusively by openless-core.
    #[cfg(all(not(mobile), target_os = "windows"))]
    selection_voice_host: Arc<Mutex<selection_voice_session::SelectionVoiceHostState>>,
    #[cfg(all(not(mobile), target_os = "windows"))]
    selection_voice_capture: Mutex<Option<Arc<openless_core::VoiceTranscriptionSession>>>,
    /// Selection voice QA (issue #118): global shortcut listener parallel to
    /// the dictation hotkey (global-hotkey crate). `None` means the feature is
    /// off or installation has not succeeded yet.
    qa_hotkey: Mutex<Option<QaHotkeyMonitor>>,
    coding_agent_modifier_hotkey: Mutex<Option<HotkeyMonitor>>,
    coding_agent_combo_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    /// State from the most recent emit_capsule, for introspection/tests only
    /// (written before app-handle validation so GUI-less tests can assert
    /// which capsule a hotkey press produced). A single cheap lock per write,
    /// negligible at the ~30Hz recording cadence.
    last_capsule_state: Mutex<Option<CapsuleState>>,
    /// Increments with each capsule payload. Selection-polish final-state
    /// auto-hide carries this epoch so a stale timer cannot overwrite newer
    /// selection-polish/voice/QA visibility.
    capsule_event_epoch: AtomicU64,
    /// Linearizes capsule events against auto-hide: a stale timer either hides
    /// the old hint before the new payload or bails on a changed epoch, never
    /// emitting Idle after a new session.
    capsule_event_lock: Mutex<()>,
    /// Selection-polish lightweight hint is still showing or being processed.
    /// Older voice/QA auto-hide timers must yield during this window so they do
    /// not dismiss the selection-polish capsule early.
    selection_polish_capsule_active: AtomicBool,
    /// Tauri QA window visibility. All QA business state belongs to openless-core.
    qa_context: Arc<TauriQaHostContext>,
    /// Warming-up flag: a hotkey press "optimistically" shows the capsule (with
    /// entrance animation) while the microphone is still inside the cpal init
    /// window with no first PCM frame. When true, emit_capsule marks the
    /// Recording payload's `warming` true (the frontend renders a standby
    /// glow); the first `level_handler` fire (real PCM flow) clears it and the
    /// meter lights up into actual recording. begin_session resets it to true
    /// on each start.
    /// Coordinator shutdown signal. Each hotkey supervisor loop checks it
    /// before its retry sleep and returns immediately when set. In production,
    /// process exit reaps all supervisor threads, but integration tests and a
    /// future RunEvent::Exit hook need this explicit exit path. Audit 3.1.2.
    shutdown: AtomicBool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionHotkeyKind {
    SwitchStyle,
    OpenApp,
    QuickNote,
}

impl Coordinator {
    #[cfg(mobile)]
    pub(crate) fn bind_selection_voice_target(
        &self,
        _session_id: openless_core::SessionId,
        _insertion_target: crate::selection::SelectionInsertionTarget,
    ) -> Result<(), String> {
        Err("selectionVoiceTargetUnavailable".to_string())
    }

    pub fn new() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::new_with_local_runtimes(
                Arc::new(FoundryLocalRuntime::new()),
                Arc::new(SherpaOnnxRuntime::new()),
            )
        }

        #[cfg(not(target_os = "windows"))]
        {
            let gate = startup_sync_gate();
            let mut startup_error = gate.as_ref().err().cloned();
            // Keep the registered barrier alive until Core adopts it. Once any store
            // fails, later constructors use fallbacks and no real repository is touched.
            let _sync_gate = gate.ok();
            let history = startup_store(
                &mut startup_error,
                HistoryStore::new,
                HistoryStore::new_fallback,
            );
            let prefs = startup_store(
                &mut startup_error,
                PreferencesStore::new,
                PreferencesStore::new_fallback,
            );
            if startup_error.is_none()
                && _sync_gate
                    .as_ref()
                    .is_some_and(|gate| !gate.recovery_required().unwrap_or(true))
            {
                crate::net::set_use_system_proxy(prefs.get().use_system_proxy);
            }
            let style_packs = startup_store(
                &mut startup_error,
                || StylePackStore::new(&prefs),
                StylePackStore::new_fallback,
            );
            let vocab = startup_store(
                &mut startup_error,
                DictionaryStore::new,
                DictionaryStore::new_fallback,
            );
            let correction_rules = startup_store(
                &mut startup_error,
                CorrectionRuleStore::new,
                CorrectionRuleStore::new_fallback,
            );
            let activity = startup_store(
                &mut startup_error,
                ActivityStore::load,
                ActivityStore::new_fallback,
            );

            let app = crate::core_adapters::app_handle_slot();
            let native_asr = crate::core_adapters::TauriNativeAsrDependencies::new();
            let hotkey_status = Arc::new(Mutex::new(HotkeyStatus::default()));
            let qa_context = Arc::new(TauriQaHostContext::default());
            let backend = shared_backend_from_stores(
                &history,
                &activity,
                &prefs,
                &style_packs,
                &vocab,
                &correction_rules,
                Arc::clone(&app),
                native_asr.clone(),
                Arc::clone(&hotkey_status),
                Arc::clone(&qa_context),
                startup_error,
            );

            let host = crate::tauri_coordinator_host::TauriCoordinatorHost::new(Arc::clone(&app));
            let hotkey_runtime_target = (&backend.get_preferences()).into();
            let inner = Arc::new(Inner {
                host,
                backend,
                less_computer_voice: Mutex::new(None),
                hotkey_runtime_target: Mutex::new(hotkey_runtime_target),
                settings_host_gate: Mutex::new(()),
                hotkey_resume_started: AtomicBool::new(false),
                overlay_qa_handoff: tokio::sync::Mutex::new(()),
                inserter: TextInserter::new(),
                vocab_card_visible: AtomicBool::new(false),
                hotkey: Mutex::new(None),
                hotkey_status,
                window_hotkey_press_id: AtomicU64::new(0),
                shortcut_recording_active: AtomicBool::new(false),
                less_computer_press_generation: AtomicU64::new(0),
                less_computer_combo_pending_press: Mutex::new(None),
                combo_hotkey: Mutex::new(None),
                side_aware_combo: Mutex::new(None),
                mouse_dictation: Mutex::new(None),
                translation_hotkey: Mutex::new(None),
                switch_style_hotkey: Mutex::new(None),
                open_app_hotkey: Mutex::new(None),
                quick_note_hotkey: Mutex::new(None),
                style_pack_hotkeys: Mutex::new(std::collections::HashMap::new()),
                #[cfg(not(mobile))]
                selection_polish_hotkey: Mutex::new(None),
                #[cfg(all(not(mobile), target_os = "windows"))]
                selection_voice_host: Arc::new(Mutex::new(
                    selection_voice_session::SelectionVoiceHostState::default(),
                )),
                #[cfg(all(not(mobile), target_os = "windows"))]
                selection_voice_capture: Mutex::new(None),
                qa_hotkey: Mutex::new(None),
                coding_agent_modifier_hotkey: Mutex::new(None),
                coding_agent_combo_hotkey: Mutex::new(None),
                last_capsule_state: Mutex::new(None),
                capsule_event_epoch: AtomicU64::new(0),
                capsule_event_lock: Mutex::new(()),
                selection_polish_capsule_active: AtomicBool::new(false),
                qa_context: Arc::clone(&qa_context),
                shutdown: AtomicBool::new(false),
            });
            #[cfg(all(not(mobile), target_os = "windows"))]
            bind_qa_selection_voice_target(&qa_context, &inner.selection_voice_host);
            Self { inner }
        }
    }

    /// Legacy constructor kept for existing call sites (including unit tests)
    /// that only pass a Foundry runtime. The sherpa-onnx runtime is created
    /// here as a default offline batch instance; once inside the app (lib.rs),
    /// use `new_with_local_runtimes` so Tauri State shares the same Arc.
    #[cfg(target_os = "windows")]
    pub fn new_with_foundry_runtime(foundry_local_runtime: Arc<FoundryLocalRuntime>) -> Self {
        Self::new_with_local_runtimes(foundry_local_runtime, Arc::new(SherpaOnnxRuntime::new()))
    }

    #[cfg(target_os = "windows")]
    pub fn new_with_local_runtimes(
        foundry_local_runtime: Arc<FoundryLocalRuntime>,
        sherpa_onnx_runtime: Arc<SherpaOnnxRuntime>,
    ) -> Self {
        let gate = startup_sync_gate();
        let mut startup_error = gate.as_ref().err().cloned();
        // Keep the registered barrier alive until Core adopts it. Once any store
        // fails, later constructors use fallbacks and no real repository is touched.
        let _sync_gate = gate.ok();
        let history = startup_store(
            &mut startup_error,
            HistoryStore::new,
            HistoryStore::new_fallback,
        );
        let prefs = startup_store(
            &mut startup_error,
            PreferencesStore::new,
            PreferencesStore::new_fallback,
        );
        if startup_error.is_none()
            && _sync_gate
                .as_ref()
                .is_some_and(|gate| !gate.recovery_required().unwrap_or(true))
        {
            crate::net::set_use_system_proxy(prefs.get().use_system_proxy);
        }
        let style_packs = startup_store(
            &mut startup_error,
            || StylePackStore::new(&prefs),
            StylePackStore::new_fallback,
        );
        let vocab = startup_store(
            &mut startup_error,
            DictionaryStore::new,
            DictionaryStore::new_fallback,
        );
        let correction_rules = startup_store(
            &mut startup_error,
            CorrectionRuleStore::new,
            CorrectionRuleStore::new_fallback,
        );
        let activity = startup_store(
            &mut startup_error,
            ActivityStore::load,
            ActivityStore::new_fallback,
        );

        let app = crate::core_adapters::app_handle_slot();
        let hotkey_status = Arc::new(Mutex::new(HotkeyStatus::default()));
        let selection_voice_host = Arc::new(Mutex::new(
            selection_voice_session::SelectionVoiceHostState::default(),
        ));
        let qa_context = Arc::new(TauriQaHostContext::default());
        let backend = shared_backend_from_stores(
            &history,
            &activity,
            &prefs,
            &style_packs,
            &vocab,
            &correction_rules,
            Arc::clone(&app),
            crate::core_adapters::TauriNativeAsrDependencies::new(
                Arc::clone(&foundry_local_runtime),
                Arc::clone(&sherpa_onnx_runtime),
            ),
            Arc::clone(&hotkey_status),
            Arc::clone(&qa_context),
            startup_error,
        );

        let host = crate::tauri_coordinator_host::TauriCoordinatorHost::new(Arc::clone(&app));
        let hotkey_runtime_target = (&backend.get_preferences()).into();
        let inner = Arc::new(Inner {
            host,
            backend,
            less_computer_voice: Mutex::new(None),
            hotkey_runtime_target: Mutex::new(hotkey_runtime_target),
            settings_host_gate: Mutex::new(()),
            hotkey_resume_started: AtomicBool::new(false),
            overlay_qa_handoff: tokio::sync::Mutex::new(()),
            inserter: TextInserter::new(),
            vocab_card_visible: AtomicBool::new(false),
            hotkey: Mutex::new(None),
            hotkey_status,
            window_hotkey_press_id: AtomicU64::new(0),
            shortcut_recording_active: AtomicBool::new(false),
            less_computer_press_generation: AtomicU64::new(0),
            less_computer_combo_pending_press: Mutex::new(None),
            combo_hotkey: Mutex::new(None),
            side_aware_combo: Mutex::new(None),
            mouse_dictation: Mutex::new(None),
            translation_hotkey: Mutex::new(None),
            switch_style_hotkey: Mutex::new(None),
            open_app_hotkey: Mutex::new(None),
            quick_note_hotkey: Mutex::new(None),
            style_pack_hotkeys: Mutex::new(std::collections::HashMap::new()),
            #[cfg(not(mobile))]
            selection_polish_hotkey: Mutex::new(None),
            #[cfg(all(not(mobile), target_os = "windows"))]
            selection_voice_host: Arc::clone(&selection_voice_host),
            #[cfg(all(not(mobile), target_os = "windows"))]
            selection_voice_capture: Mutex::new(None),
            qa_hotkey: Mutex::new(None),
            coding_agent_modifier_hotkey: Mutex::new(None),
            coding_agent_combo_hotkey: Mutex::new(None),
            last_capsule_state: Mutex::new(None),
            capsule_event_epoch: AtomicU64::new(0),
            capsule_event_lock: Mutex::new(()),
            selection_polish_capsule_active: AtomicBool::new(false),
            qa_context: Arc::clone(&qa_context),
            shutdown: AtomicBool::new(false),
        });
        bind_qa_selection_voice_target(&qa_context, &selection_voice_host);
        Self { inner }
    }

    pub fn startup_error(&self) -> Option<openless_core::BackendError> {
        self.inner.backend.startup_error()
    }

    pub fn backend(&self) -> Arc<openless_core::OpenLessBackend> {
        Arc::clone(&self.inner.backend)
    }

    pub fn show_core_insert_fallback(&self, text: String, reason: &str) {
        let reason = match reason {
            "partial_stream" => crate::types::INSERT_FALLBACK_REASON_PARTIAL_STREAM,
            _ => crate::types::INSERT_FALLBACK_REASON_INSERT_FAILED,
        };
        show_insert_fallback_card(&self.inner, text, reason);
    }

    pub fn present_core_capsule(&self, payload: CapsulePayload) {
        let _ = self.present_core_capsule_if_current(payload, None);
    }

    /// Selection-voice claims the shared capsule from hotkey Start through
    /// session teardown so late dictation Done cannot overwrite Recording.
    pub(crate) fn selection_voice_owns_capsule(&self) -> bool {
        #[cfg(all(not(mobile), target_os = "windows"))]
        {
            selection_voice_session::selection_voice_owns_capsule(&self.inner)
        }
        #[cfg(not(all(not(mobile), target_os = "windows")))]
        {
            false
        }
    }

    pub(crate) fn selection_voice_accepts_level(&self, session_id: &str) -> bool {
        #[cfg(all(not(mobile), target_os = "windows"))]
        {
            selection_voice_session::selection_voice_accepts_level(&self.inner, session_id)
        }
        #[cfg(not(all(not(mobile), target_os = "windows")))]
        {
            let _ = session_id;
            false
        }
    }

    pub(crate) fn present_core_capsule_if_current(
        &self,
        payload: CapsulePayload,
        expected_epoch: Option<u64>,
    ) -> Option<u64> {
        let state = payload.state;
        // Core already owns this frame's translation, readiness, and session
        // ownership; the window layer must not rebuild it from stale late
        // state, or cold starts and fast switches lose real feedback.
        let epoch = emit_core_capsule(&self.inner, payload, expected_epoch)?;
        if let Some(delay_ms) = core_capsule_hide_delay(state) {
            let inner = Arc::clone(&self.inner);
            self.inner.host.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                hide_core_capsule_if_current(&inner, epoch);
            });
        }
        Some(epoch)
    }

    pub(crate) fn tauri_host(&self) -> crate::tauri_coordinator_host::TauriCoordinatorHost {
        self.inner.host.clone()
    }

    pub fn android_insert_strategy(&self) -> crate::types::AndroidInsertStrategy {
        self.inner.backend.get_preferences().android_insert_strategy
    }

    pub fn android_overlay_trigger(&self) -> crate::types::AndroidOverlayTrigger {
        self.inner
            .backend
            .get_preferences()
            .android_overlay_trigger
            .normalized()
    }

    pub fn apply_android_overlay_settings_change(
        &self,
        previous: &crate::types::UserPreferences,
        next: &crate::types::UserPreferences,
    ) {
        #[cfg(target_os = "android")]
        {
            use crate::types::android_types::{
                classify_android_overlay_settings_change, AndroidOverlaySettingsAction,
            };
            match classify_android_overlay_settings_change(previous, next) {
                AndroidOverlaySettingsAction::None => {}
                AndroidOverlaySettingsAction::RefreshLayout => {
                    self.refresh_android_overlay_layout();
                }
                AndroidOverlaySettingsAction::Transition { from, to } => {
                    self.transition_android_overlay_trigger(from, to);
                }
            }
        }
        let _ = (previous, next);
    }

    pub fn transition_android_overlay_trigger(
        &self,
        from: crate::types::AndroidOverlayTrigger,
        to: crate::types::AndroidOverlayTrigger,
    ) {
        #[cfg(target_os = "android")]
        {
            use crate::types::AndroidOverlayTrigger;
            fn overlay_trigger_log_name(trigger: AndroidOverlayTrigger) -> &'static str {
                match trigger.normalized() {
                    AndroidOverlayTrigger::Background => "background",
                    AndroidOverlayTrigger::Keyboard => "keyboard",
                    AndroidOverlayTrigger::Always => "always",
                }
            }
            if from == to {
                return;
            }
            log::info!(
                "[coord] overlay transition from={} to={}",
                overlay_trigger_log_name(from),
                overlay_trigger_log_name(to),
            );
            match (from, to) {
                (
                    AndroidOverlayTrigger::Background | AndroidOverlayTrigger::Keyboard,
                    AndroidOverlayTrigger::Always,
                ) => {
                    let _ = crate::android::replace_android_overlay();
                }
                (
                    AndroidOverlayTrigger::Always,
                    AndroidOverlayTrigger::Background | AndroidOverlayTrigger::Keyboard,
                ) => {
                    let _ = crate::android::hide_android_overlay();
                }
                _ => {}
            }
        }
        let _ = (from, to);
    }

    pub fn apply_android_overlay_on_startup(&self) {
        #[cfg(target_os = "android")]
        {
            use crate::types::AndroidOverlayTrigger;
            match self.android_overlay_trigger() {
                AndroidOverlayTrigger::Always => {
                    let _ = crate::android::replace_android_overlay();
                }
                AndroidOverlayTrigger::Background | AndroidOverlayTrigger::Keyboard => {
                    let _ = crate::android::hide_android_overlay();
                }
            }
        }
    }

    pub fn refresh_android_overlay_layout(&self) {
        #[cfg(target_os = "android")]
        {
            let _ = crate::android::refresh_android_overlay_layout();
        }
    }

    /// Makes all hotkey supervisor loops (dictation / qa / combo / translation
    /// / switch_style / open_app / style_pack / selection_polish) exit after
    /// their next sleep / poll. In production, process exit reaps all threads,
    /// but integration tests and a future RunEvent::Exit hook need this
    /// explicit exit path. Audit 3.1.2.
    #[allow(dead_code)]
    pub fn request_shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        self.inner.mouse_dictation.lock().take();
    }

    /// Call once from RunEvent::Ready, even when recovery is still pending.
    /// Installation waits for the fence; sleeping never owns the settings gate.
    pub fn start_hotkey_supervisors_when_ready(&self) {
        if self
            .inner
            .hotkey_resume_started
            .swap(true, Ordering::AcqRel)
        {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let fallback = weak.clone();
        if std::thread::Builder::new()
            .name("openless-hotkey-resume".into())
            .spawn(move || loop {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if inner.shutdown.load(Ordering::SeqCst) || inner.backend.startup_error().is_some()
                {
                    return;
                }
                if inner.backend.ensure_runtime_ready().is_ok() {
                    let coord = Coordinator { inner };
                    coord.start_hotkey_listener();
                    coord.start_qa_hotkey_listener();
                    #[cfg(not(mobile))]
                    coord.start_selection_polish_hotkey_listener();
                    coord.start_coding_agent_hotkey_listener();
                    coord.start_combo_hotkey_listener();
                    coord.start_translation_hotkey_listener();
                    coord.start_switch_style_hotkey_listener();
                    coord.start_open_app_hotkey_listener();
                    coord.start_quick_note_hotkey_listener();
                    coord.start_style_pack_hotkey_listeners();
                    return;
                }
                drop(inner);
                std::thread::sleep(std::time::Duration::from_secs(1));
            })
            .is_err()
        {
            if let Some(inner) = fallback.upgrade() {
                inner.hotkey_resume_started.store(false, Ordering::Release);
            }
            log::error!("[coord] hotkey resume supervisor could not start");
        }
    }

    pub fn start_hotkey_listener(&self) {
        if self.inner.backend.ensure_runtime_ready().is_err() {
            log::info!("[coord] hotkey startup waits for local recovery");
            return;
        }
        // Spawn a daemon thread that retries installing the hotkey hook, so it
        // takes effect as soon as Accessibility is granted without requiring
        // the user to restart OpenLess.
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-hotkey-supervisor".into())
            .spawn(move || hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_hotkey_listener(&self) {
        self.inner.hotkey.lock().take();
    }

    /// Starts the QA hotkey supervisor (issue #118), parallel to
    /// `start_hotkey_listener`: a daemon thread retries registration every 3s
    /// on failure (the user may have changed the combo).
    pub fn start_qa_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-qa-hotkey-supervisor".into())
            .spawn(move || qa_hotkey_supervisor_loop(inner))
            .ok();
    }

    /// Starts the "quick Agent" dual-hotkey supervisor, parallel to the QA
    /// hotkey; the feature is off by default and registered only when
    /// `coding_agent_enabled`.
    pub fn start_coding_agent_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-coding-agent-hotkey-supervisor".into())
            .spawn(move || coding_agent_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_coding_agent_hotkey_listener(&self) {
        take_coding_agent_hotkeys_on_main_thread(&self.inner);
    }

    pub(crate) fn update_coding_agent_hotkey_binding(&self) -> Result<(), String> {
        update_coding_agent_hotkey_binding_now(&self.inner)
    }

    pub fn stop_qa_hotkey_listener(&self) {
        // QaHotkeyMonitor::drop bottoms out in Carbon RemoveEventHotKey on
        // macOS, which requires the main thread. The RunEvent::Exit callback is
        // not guaranteed to run on the AppKit main thread; a drop that lands on
        // a tokio worker triggers macOS dispatch_assert_queue_fail SIGTRAP.
        // Wrap it in run_on_main_thread so the drop happens on the main
        // thread; when the AppHandle is already None, drop directly (worst case
        // is a crash at exit time anyway). See issue #169.
        let inner = Arc::clone(&self.inner);
        if self
            .inner
            .host
            .run_on_main_thread(move || {
                inner.qa_hotkey.lock().take();
            })
            .is_err()
        {
            self.inner.qa_hotkey.lock().take();
        }
    }

    #[cfg(not(mobile))]
    pub fn start_selection_polish_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-selection-polish-hotkey-supervisor".into())
            .spawn(move || selection_polish_hotkey_supervisor_loop(inner))
            .ok();
    }

    #[cfg(not(mobile))]
    pub fn stop_selection_polish_hotkey_listener(&self) {
        take_selection_polish_hotkey_on_main_thread(&self.inner);
    }

    #[cfg(not(mobile))]
    pub(crate) fn try_update_selection_polish_hotkey_binding(&self) -> Result<(), String> {
        try_update_selection_polish_hotkey_binding(&self.inner)
    }

    #[cfg(not(mobile))]
    pub(crate) fn update_selection_polish_hotkey_binding(&self) {
        if let Err(error) = self.try_update_selection_polish_hotkey_binding() {
            log::warn!("[coord] update selection polish hotkey binding failed: {error}");
        }
    }

    /// Starts the custom combo-key listener. Replaces the modifier-only hotkey
    /// monitor when `prefs.hotkey.trigger == Custom`.
    pub fn start_combo_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-combo-hotkey-supervisor".into())
            .spawn(move || combo_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_combo_hotkey_listener(&self) {
        take_combo_hotkey_on_main_thread(&self.inner);
    }

    pub fn start_translation_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-translation-hotkey-supervisor".into())
            .spawn(move || translation_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_translation_hotkey_listener(&self) {
        take_translation_hotkey_on_main_thread(&self.inner);
    }

    pub fn start_switch_style_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-switch-style-hotkey-supervisor".into())
            .spawn(move || action_hotkey_supervisor_loop(inner, ActionHotkeyKind::SwitchStyle))
            .ok();
    }

    pub fn stop_switch_style_hotkey_listener(&self) {
        take_action_hotkey_on_main_thread(&self.inner, ActionHotkeyKind::SwitchStyle);
    }

    pub fn start_open_app_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-open-app-hotkey-supervisor".into())
            .spawn(move || action_hotkey_supervisor_loop(inner, ActionHotkeyKind::OpenApp))
            .ok();
    }

    pub fn stop_open_app_hotkey_listener(&self) {
        take_action_hotkey_on_main_thread(&self.inner, ActionHotkeyKind::OpenApp);
    }

    pub fn start_quick_note_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-quick-note-hotkey-supervisor".into())
            .spawn(move || action_hotkey_supervisor_loop(inner, ActionHotkeyKind::QuickNote))
            .ok();
    }

    pub fn stop_quick_note_hotkey_listener(&self) {
        take_action_hotkey_on_main_thread(&self.inner, ActionHotkeyKind::QuickNote);
    }

    /// Starts style-pack direct shortcut listening (issue #759). The
    /// supervisor thread waits for the AppHandle, registers everything from
    /// prefs, and retries individual failures on the action-hotkey cadence.
    pub fn start_style_pack_hotkey_listeners(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-style-pack-hotkey-supervisor".into())
            .spawn(move || style_pack_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_style_pack_hotkey_listeners(&self) {
        clear_style_pack_hotkeys_on_main_thread(&self.inner);
    }

    /// Called when the user edits the style shortcut list in settings:
    /// realigns registration state fully against the latest prefs.
    pub(crate) fn update_style_pack_hotkey_bindings(&self) {
        sync_style_pack_hotkeys_on_main_thread(&self.inner);
    }

    /// Used by the transactional settings path: waits for the main thread to
    /// finish full registration and returns the precise failure reason.
    pub(crate) fn try_update_style_pack_hotkey_bindings(&self) -> Result<(), String> {
        try_sync_style_pack_hotkeys_on_main_thread(&self.inner)
    }

    /// 用户在设置里改了 QA 组合键时调用。先持久化（由 prefs.set 完成），
    /// 然后通知活着的 monitor 重新注册；monitor 不存在时 supervisor 会自然
    /// 在下一次循环里读到新的 prefs。
    pub(crate) fn update_qa_hotkey_binding(&self) {
        let target = hotkey_runtime_target(&self.inner);
        let Some(binding) = target.qa else {
            // The user disabled the feature -> drop the monitor directly. The
            // drop must happen on the main thread, or Carbon unregister fails
            // / is UB.
            let inner_clone = Arc::clone(&self.inner);
            if self
                .inner
                .host
                .run_on_main_thread(move || {
                    inner_clone.qa_hotkey.lock().take();
                })
                .is_err()
            {
                self.inner.qa_hotkey.lock().take();
            }
            log::info!("[coord] QA hotkey 已关闭");
            self.update_modifier_shortcut_bindings();
            return;
        };
        if crate::shortcut_binding::legacy_modifier_trigger(&binding).is_some() {
            let inner_clone = Arc::clone(&self.inner);
            if self
                .inner
                .host
                .run_on_main_thread(move || {
                    inner_clone.qa_hotkey.lock().take();
                })
                .is_err()
            {
                self.inner.qa_hotkey.lock().take();
            }
            self.update_modifier_shortcut_bindings();
            log::info!("[coord] QA hotkey uses modifier-only listener");
            return;
        }
        self.update_modifier_shortcut_bindings();
        // The global-hotkey crate's manager.register/unregister must run on
        // the main thread; off-thread, Carbon registration appears to succeed
        // but events are never dispatched.
        let inner_clone = Arc::clone(&self.inner);
        let binding_for_main = binding.clone();
        if self
            .inner
            .host
            .run_on_main_thread(move || {
                // Path 1: a monitor already exists -> swap the binding on the
                // main thread.
                if let Some(monitor) = inner_clone.qa_hotkey.lock().as_ref() {
                    if let Err(e) = monitor.update_binding(binding_for_main.clone()) {
                        log::warn!("[coord] update QA hotkey binding 失败: {e}");
                    }
                    return;
                }
                // Path 2: not installed yet -> reinstall on the main thread
                // (the supervisor also retries, but this way the hotkey takes
                // effect as soon as the set_qa_hotkey command returns).
                let (tx, rx) = mpsc::channel::<QaHotkeyEvent>();
                match QaHotkeyMonitor::start(binding_for_main, tx) {
                    Ok(monitor) => {
                        *inner_clone.qa_hotkey.lock() = Some(monitor);
                        log::info!(
                            "[coord] QA hotkey listener installed on main thread (via update)"
                        );
                        let bridge_inner = Arc::clone(&inner_clone);
                        std::thread::Builder::new()
                            .name("openless-qa-hotkey-bridge".into())
                            .spawn(move || qa_hotkey_bridge_loop(bridge_inner, rx))
                            .ok();
                    }
                    Err(e) => {
                        log::warn!("[coord] update QA hotkey binding 失败: {e}");
                    }
                }
            })
            .is_err()
        {
            log::warn!("[coord] update QA hotkey binding: AppHandle 未 bind，跳过");
        }
    }

    pub(crate) fn update_translation_hotkey_binding(&self) {
        if let Err(e) = self.try_update_translation_hotkey_binding() {
            log::warn!("[coord] update translation hotkey binding 失败: {e}");
        }
    }

    pub(crate) fn try_update_translation_hotkey_binding(&self) -> Result<(), String> {
        let target = hotkey_runtime_target(&self.inner);
        if is_builtin_translation_shift(&target.translation)
            || crate::shortcut_binding::legacy_modifier_trigger(&target.translation).is_some()
        {
            take_translation_hotkey_on_main_thread(&self.inner);
            self.update_modifier_shortcut_bindings();
            log::info!("[coord] translation hotkey uses modifier-only listener");
            return Ok(());
        }
        self.update_modifier_shortcut_bindings();
        let inner_clone = Arc::clone(&self.inner);
        let binding_for_main = target.translation;
        let (result_tx, result_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        self.inner.host.run_on_main_thread(move || {
            let result = update_translation_hotkey_on_main_thread(inner_clone, binding_for_main);
            let _ = result_tx.send(result.map_err(|e| e.to_string()));
        })?;
        match result_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(result) => result,
            Err(_) => Err("注册翻译快捷键超时".into()),
        }
    }

    pub(crate) fn update_switch_style_hotkey_binding(&self) {
        self.update_action_hotkey_binding(ActionHotkeyKind::SwitchStyle);
    }

    pub(crate) fn update_open_app_hotkey_binding(&self) {
        self.update_action_hotkey_binding(ActionHotkeyKind::OpenApp);
    }

    pub(crate) fn update_quick_note_hotkey_binding(&self) {
        self.update_action_hotkey_binding(ActionHotkeyKind::QuickNote);
    }

    fn update_action_hotkey_binding(&self, kind: ActionHotkeyKind) {
        // None = the user disabled it: unregister the global key immediately.
        let Some(binding) = action_hotkey_binding(&self.inner, kind) else {
            take_action_hotkey_on_main_thread(&self.inner, kind);
            log::info!("[coord] action hotkey {kind:?} 已停用（用户清空）");
            return;
        };
        if is_modifier_only_shortcut(&binding) {
            take_action_hotkey_on_main_thread(&self.inner, kind);
            log::warn!("[coord] action hotkey {kind:?} 使用了不支持的 modifier-only 绑定，已关闭");
            return;
        }

        let inner_clone = Arc::clone(&self.inner);
        if self
            .inner
            .host
            .run_on_main_thread(move || {
                if let Some(monitor) = action_hotkey_slot(&inner_clone, kind).lock().as_ref() {
                    if let Err(e) = monitor.update_binding(binding.clone()) {
                        log::warn!("[coord] update action hotkey {kind:?} binding 失败: {e}");
                    }
                    return;
                }
                let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
                match ComboHotkeyMonitor::start(binding, tx) {
                    Ok(monitor) => {
                        *action_hotkey_slot(&inner_clone, kind).lock() = Some(monitor);
                        let bridge_inner = Arc::clone(&inner_clone);
                        std::thread::Builder::new()
                            .name(action_hotkey_bridge_thread_name(kind).into())
                            .spawn(move || action_hotkey_bridge_loop(bridge_inner, rx, kind))
                            .ok();
                    }
                    Err(e) => log::warn!("[coord] update action hotkey {kind:?} binding 失败: {e}"),
                }
            })
            .is_err()
        {
            log::warn!("[coord] update action hotkey binding: AppHandle 未 bind，跳过");
        }
    }

    /// Label of the current QA shortcut for the frontend Settings (e.g.
    /// "Cmd+Shift+;"). Returns an empty string when `qa_hotkey == None`; the UI
    /// shows "not enabled" for it.
    pub fn qa_hotkey_label(&self) -> String {
        self.inner
            .backend
            .get_preferences()
            .qa_hotkey
            .as_ref()
            .map(|b| b.display_label())
            .unwrap_or_default()
    }

    /// After saving, syncs style, window size, and click region without
    /// waiting for the next recording state event.
    pub fn sync_capsule_style_from_preferences(&self) {
        self.inner
            .host
            .cache_capsule_style(self.inner.backend.get_preferences().capsule_style);
    }
    /// Apply only the Tauri presentation side of the Core-owned vocabulary
    /// suggestion state. Commands mutate the Core collection first, then pass
    /// this narrow boolean to the host so Coordinator never owns or rewrites
    /// the suggestion business state.
    pub(crate) fn refresh_vocab_suggestion_presentation(&self, has_pending: bool) {
        if has_pending {
            show_vocab_suggestion_card(&self.inner);
        } else {
            hide_vocab_suggestion_card(&self.inner);
        }
    }

    /// The insert-fallback card dismissed itself (user closed it or TTL
    /// expired).
    pub fn dismiss_insert_fallback_card(&self) {
        hide_insert_fallback_card(&self.inner);
    }

    pub fn report_insert_fallback_card_height(
        &self,
        presentation_id: u64,
        height: f64,
    ) -> Result<(), String> {
        report_insert_fallback_card_height(&self.inner, presentation_id, height)
    }

    fn try_ensure_modifier_hotkey_monitor(
        &self,
        binding: crate::types::HotkeyBinding,
    ) -> Result<(), String> {
        if let Some(monitor) = self.inner.hotkey.lock().as_ref() {
            monitor.update_binding(binding);
            return Ok(());
        }
        let (tx, rx) = mpsc::channel::<HotkeyEvent>();
        let cancel_tx = spawn_esc_cancel_bridge(&self.inner);
        let combo_tx = spawn_combo_abort_bridge(&self.inner, handle_trigger_combined);
        match HotkeyMonitor::start(binding, tx, cancel_tx, combo_tx) {
            Ok(monitor) => {
                let adapter = monitor.kind();
                let inner_clone = Arc::clone(&self.inner);
                std::thread::Builder::new()
                    .name("openless-hotkey-bridge".into())
                    .spawn(move || hotkey_bridge_loop(inner_clone, rx))
                    .map_err(|error| error.to_string())?;
                // Publish only after the bridge exists. A failed thread spawn
                // must drop this monitor, not leave a registered dead sender.
                *self.inner.hotkey.lock() = Some(monitor);
                *self.inner.hotkey_status.lock() = HotkeyStatus {
                    adapter,
                    state: HotkeyStatusState::Installed,
                    message: Some(format!("{} 已安装", adapter.display_name())),
                    last_error: None,
                };
            }
            Err(e) => {
                *self.inner.hotkey_status.lock() = HotkeyStatus {
                    adapter: HotkeyMonitor::capability().adapter,
                    state: HotkeyStatusState::Failed,
                    message: Some(e.message.clone()),
                    last_error: Some(e.clone()),
                };
                return Err(e.message);
            }
        }
        Ok(())
    }

    pub fn update_modifier_shortcut_bindings(&self) {
        if let Some(monitor) = self.inner.hotkey.lock().as_ref() {
            let (qa_trigger, selection_polish_trigger, translation_trigger) =
                modifier_shortcut_triggers(&self.inner);
            monitor.update_modifier_shortcuts(
                qa_trigger,
                selection_polish_trigger,
                translation_trigger,
            );
        }
    }

    /// Applies a Core-validated, conflict-resolved explicit target to the host
    /// listeners.
    ///
    /// This method never reads preferences; on failure the target stays `next`
    /// and Core restores it via the inverse change based on the receipt, so
    /// partial installation still converges back to the old state.
    pub(crate) fn apply_hotkey_runtime_change(
        &self,
        change: &openless_core::SettingsValueChange<openless_core::HotkeyRuntimeTarget>,
    ) -> Result<(), String> {
        let previous = &change.previous;
        let next = &change.next;
        *self.inner.hotkey_runtime_target.lock() = next.clone();

        if previous.translation != next.translation {
            self.try_update_translation_hotkey_binding()?;
        }
        #[cfg(not(mobile))]
        if previous.selection_polish != next.selection_polish {
            self.try_update_selection_polish_hotkey_binding()?;
        }
        if previous.style_packs != next.style_packs {
            self.try_update_style_pack_hotkey_bindings()?;
        }
        if previous.dictation != next.dictation || previous.dictation_mode != next.dictation_mode {
            self.try_update_native_dictation_binding()?;
            self.update_modifier_shortcut_bindings();
        }
        if previous.qa != next.qa {
            self.update_qa_hotkey_binding();
        }
        if previous.switch_style != next.switch_style {
            self.update_switch_style_hotkey_binding();
        }
        if previous.open_app != next.open_app {
            self.update_open_app_hotkey_binding();
        }
        if previous.quick_note != next.quick_note {
            self.update_quick_note_hotkey_binding();
        }
        if previous.coding_agent_enabled != next.coding_agent_enabled
            || previous.coding_agent_voice != next.coding_agent_voice
        {
            // After the old key is unregistered no Released edge will arrive;
            // cancel only its still-capturing Less session. Tasks the Agent
            // already handled took the capture and are not in this slot.
            cancel_less_computer_capture(&self.inner, None);
            self.update_coding_agent_hotkey_binding()?;
        }
        Ok(())
    }

    pub(crate) fn lock_settings_host(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.inner.settings_host_gate.lock()
    }

    pub(crate) async fn dismiss_less_computer(&self) -> Result<(), String> {
        // End the current conversation identity first, then release the Host
        // capture / Core run precisely; hiding the window does not stop the
        // mic.
        self.inner.backend.services().less_computer.dismiss();
        // Hide immediately. If the user reopens the window during cleanup, its
        // show epoch supersedes this exit animation.
        self.inner.host.hide_less_computer();
        let result = cancel_active_less_computer(&self.inner).await;
        result.map(|_| ()).map_err(|error| error.to_string())
    }

    pub(crate) async fn stop_less_computer_recording(&self) -> Result<bool, String> {
        let starting = {
            let slot = self.inner.less_computer_voice.lock();
            match slot.as_ref() {
                Some(LessComputerHostCapture::Starting(id, control)) => {
                    Some((*id, control.clone()))
                }
                _ => None,
            }
        };
        if let Some((id, control)) = starting {
            // Capsule stop and mute stop share the handoff queue, so clicks
            // during cold start cannot be swallowed.
            return control
                .request(id, openless_core::RecordingControlAction::Stop)
                .map(|_| true)
                .map_err(|error| error.to_string());
        }
        finish_less_computer_voice_session(&self.inner, None)
            .await
            .map_err(|error| error.to_string())
    }

    /// Composer microphone / voice-mode button. Start errors are returned to the
    /// panel instead of being posted into the conversation stream.
    pub(crate) async fn start_less_computer_voice_from_panel(
        &self,
        mode: openless_core::LessComputerVoiceMode,
    ) -> Result<(), String> {
        start_less_computer_capture(
            &self.inner,
            openless_core::LessComputerVoiceOptions {
                mode,
                publish_start_error: false,
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| error.message)
    }

    pub(crate) fn stop_less_computer_voice_from_panel(
        &self,
        session_id: openless_core::SessionId,
    ) -> Result<(), String> {
        request_less_computer_voice_stop(&self.inner, session_id)
            .map(|_| ())
            .map_err(|error| error.message)
    }

    pub(crate) async fn cancel_less_computer_voice_from_panel(
        &self,
        session_id: openless_core::SessionId,
    ) -> Result<(), String> {
        cancel_less_computer_voice_request(&self.inner, session_id)
            .await
            .map(|_| ())
            .map_err(|error| error.message)
    }

    /// Stop button while an Agent turn runs; the window stays open.
    pub(crate) async fn cancel_less_computer_task(&self) -> Result<(), String> {
        cancel_active_less_computer(&self.inner)
            .await
            .map(|_| ())
            .map_err(|error| error.message)
    }

    pub(crate) async fn cancel_active_voice(&self) {
        dictation::cancel_active_session(&self.inner).await;
    }

    pub(crate) async fn cancel_dictation_from_cli(
        &self,
    ) -> Result<openless_core::CliDispatchOutcome, openless_core::BackendError> {
        // CLI cancel matches the 1.x main-session scope: release the Less Host
        // slot first, then delegate only to main dictation cancellation. The
        // global Esc QA/Selection branches are not reused, keeping the
        // existing CLI command scope unchanged.
        if cancel_active_less_computer(&self.inner).await? {
            return Ok(openless_core::CliDispatchOutcome::DictationCancelled);
        }
        self.inner
            .backend
            .dispatch_cli_intent(crate::cli::CliIntent::CancelDictation)
            .await
    }

    pub fn hotkey_capability(&self) -> HotkeyCapability {
        HotkeyMonitor::capability()
    }

    pub fn switch_to_previous_style_pack(&self) {
        switch_to_previous_style(&self.inner);
    }

    pub async fn open_qa_from_overlay(&self) -> Result<(), String> {
        log::info!("[coord] overlay QA open requested");
        self.inner
            .backend
            .services()
            .qa
            .show()
            .await
            .map_err(|error| error.message)?;
        self.inner
            .backend
            .services()
            .qa
            .toggle_recording()
            .await
            .map_err(|error| error.message)
    }

    pub async fn finalize_qa_from_overlay(&self) -> Result<(), String> {
        let Ok(_handoff) = self.inner.overlay_qa_handoff.try_lock() else {
            return Ok(());
        };
        let backend = &self.inner.backend;
        let snapshot = backend.snapshot().dictation;
        if let Some(session_id) = snapshot.session_id {
            if !matches!(
                snapshot.phase,
                openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording
            ) {
                return Err("当前语音仍在处理中，请稍后再试".into());
            }
            // Capture the source before either transcription or the QA panel
            // can change focus. Only owned text/app metadata crosses threads.
            let capture = tauri::async_runtime::spawn_blocking(|| {
                let (capture, _) = crate::selection::resolve_selection_workspace_capture();
                capture.map(|value| (value.text, value.source_app))
            })
            .await
            .map_err(|error| format!("capture QA selection: {error}"))?;
            let result = backend
                .stop_dictation_for_qa(session_id)
                .await
                .map_err(|error| error.message)?;
            let (selection_text, selection_source_app) = match capture {
                Some((text, app)) => (Some(text), app),
                None => (None, None),
            };
            return backend
                .services()
                .qa
                .submit_captured_text(openless_core::QaInput {
                    text: result.raw_text,
                    selection_text,
                    selection_source_app,
                })
                .await
                .map_err(|error| error.message);
        }
        let qa = backend
            .services()
            .qa
            .snapshot()
            .await
            .map_err(|error| error.message)?;
        if let (openless_core::QaPhase::Recording, Some(session_id)) = (qa.phase, qa.session_id) {
            backend
                .services()
                .qa
                .stop_recording(session_id)
                .await
                .map_err(|error| error.message)
        } else {
            self.open_qa_from_overlay().await
        }
    }

    /// QA toggle for the CLI entry point: reuses the modifier-only QA hotkey
    /// edge handler. Same semantics as `handle_qa_hotkey_pressed` — Idle →
    /// open the panel / Recording → finalize / Processing → ignore. A fallback
    /// entry point when the desktop shortcut forwards to CLI.
    pub async fn cli_toggle_qa_panel(&self) {
        handle_qa_hotkey_pressed(&self.inner).await;
    }

    pub fn set_shortcut_recording_active(&self, active: bool) {
        self.inner
            .shortcut_recording_active
            .store(active, Ordering::SeqCst);
        // Sync to the hotkey listeners: while recording mode is active, the
        // CGEventTap reports Fn press edges so the frontend ShortcutRecorder
        // can commit Fn bindings (browsers never deliver Fn keydown to web
        // pages).
        #[cfg(not(mobile))]
        let sync_ok = self.inner.hotkey.lock().as_ref().map(|m| {
            m.set_recording_active(active);
            true
        });
        #[cfg(mobile)]
        let sync_ok = None;
        #[cfg(not(mobile))]
        if let Some(monitor) = self.inner.coding_agent_modifier_hotkey.lock().as_ref() {
            monitor.set_recording_active(active);
            if active {
                monitor.reset_held_state();
            }
        }
        if active {
            reset_shortcut_held_state(&self.inner);
        }
        log::info!(
            "[coord] shortcut recording active={active} (synced_to_hotkey={})",
            sync_ok.unwrap_or(false)
        );
    }

    pub async fn handle_window_hotkey_event(
        &self,
        event_type: String,
        key: String,
        code: String,
        repeat: bool,
    ) -> Result<(), String> {
        handle_window_hotkey_event(&self.inner, event_type, key, code, repeat).await
    }

    #[cfg(any(debug_assertions, test))]
    pub async fn inject_hotkey_click_for_dev(&self) -> Result<(), String> {
        log::info!("[coord] dev hotkey injection started");
        let press_id = crate::hotkey::next_press_id();
        handle_pressed(&self.inner, std::time::Instant::now(), press_id).await;
        handle_released(&self.inner, std::time::Instant::now(), press_id).await;
        let _ = self.inner.backend.cancel_dictation(None).await;
        Ok(())
    }
}

const CAPSULE_AUTO_HIDE_DELAY_MS: u64 = 2000;

/// Core terminal events describe semantics only; how long the native capsule
/// stays is the Tauri Host's job. Centralizing the mapping in a pure function
/// pins the done/error/cancel timings and lets tests run without real windows
/// and timers.
fn core_capsule_hide_delay(state: CapsuleState) -> Option<u64> {
    match state {
        CapsuleState::Done | CapsuleState::Error => Some(CAPSULE_AUTO_HIDE_DELAY_MS),
        CapsuleState::Cancelled => Some(0),
        _ => None,
    }
}

fn schedule_capsule_idle(inner: &Arc<Inner>, delay_ms: u64) {
    let expected = inner.last_capsule_state.lock().as_ref().copied();
    let inner = Arc::clone(inner);
    let spawner = inner.host.clone();
    spawner.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        if inner.last_capsule_state.lock().as_ref().copied() == expected {
            hide_capsule_if_all_sessions_idle(&inner);
        }
    });
}

#[cfg(test)]
mod core_capsule_tests {
    use super::*;

    #[test]
    fn core_terminal_capsule_timing_preserves_the_legacy_contract() {
        assert_eq!(core_capsule_hide_delay(CapsuleState::Done), Some(2000));
        assert_eq!(core_capsule_hide_delay(CapsuleState::Error), Some(2000));
        assert_eq!(core_capsule_hide_delay(CapsuleState::Cancelled), Some(0));
        assert_eq!(core_capsule_hide_delay(CapsuleState::Recording), None);
    }
}

#[cfg(not(mobile))]
fn schedule_selection_polish_capsule_idle(inner: &Arc<Inner>, epoch: u64, delay_ms: u64) {
    let inner = Arc::clone(inner);
    let spawner = inner.host.clone();
    spawner.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        hide_selection_polish_capsule_if_current(&inner, epoch);
    });
}

#[cfg(test)]
mod startup_restore_tests {
    use super::*;

    #[test]
    fn restore_host_denied_store_blocks_later_real_constructors() {
        let mut failure = None;
        let first = startup_store(&mut failure, || Err::<u8, _>("denied"), || 7);
        assert_eq!(first, 7);
        assert!(failure.is_some());
        let second = startup_store(
            &mut failure,
            || -> Result<u8, ()> { panic!("must not open another real store after failure") },
            || 9,
        );
        assert_eq!(second, 9);
        let backend = openless_core::OpenLessBackend::blocked_startup(
            openless_core::BackendConfig::default(),
            failure.unwrap(),
        );
        assert!(backend.startup_error().is_some());
        assert!(!backend.snapshot().running);
        assert!(backend.ensure_runtime_ready().is_err());
    }

    #[test]
    fn restore_host_healthy_store_does_not_use_fallback() {
        let mut failure = None;
        assert_eq!(
            startup_store(
                &mut failure,
                || Ok::<_, ()>(11),
                || panic!("unexpected fallback")
            ),
            11
        );
        assert!(failure.is_none());
    }
}
