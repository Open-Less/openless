//! Focus-target capture and capsule-window presentation extracted from
//! `coordinator.rs` (behavior-preserving move).
//!
//! External focus/frontmost-app capture and `emit_capsule`. Native window
//! presentation belongs to `tauri_coordinator_host`. References parent items via `use super::*;`; `pub(super)`
//! so the parent and sibling submodules reach them through `use capsule_focus::*;`.

use super::*;

/// Like capture_focus_target, but returns None when the foreground window belongs to this
/// process (the user is on one of our own windows such as QA / capsule / main), letting the
/// caller distinguish "user didn't switch away" from "user switched to another real external
/// app". Used to refresh qa_focus_target in issue #466's multi-turn flow.
#[cfg(target_os = "windows")]
pub(crate) fn capture_external_focus_target() -> Option<usize> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return None;
        }
        Some(hwnd.0 as usize)
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn capture_external_focus_target() -> Option<usize> {
    None
}

#[cfg(target_os = "windows")]
pub(crate) fn capture_focus_target() -> Option<usize> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        None
    } else {
        Some(foreground.0 as usize)
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn capture_focus_target() -> Option<usize> {
    None
}

/// Capture the foreground app label at dictation start ("localizedName (bundle.id)"), used as
/// context for LLM polish/translate so the model can adapt style per app. See issue #116.
///
/// macOS uses NSWorkspace.frontmostApplication (public API, no extra permission);
/// Windows reuses the foreground HWND for the window title; Linux and other platforms return None.
pub(crate) fn capture_frontmost_app() -> Option<String> {
    // This once held an NSWorkspace/Win32 implementation duplicated verbatim from
    // `selection.rs` (three cfg branches, even the nsstring conversion helper). Consolidated
    // into selection: it now exposes the structured `current_front_app_parts`, which the
    // `host_document` bundle blocklist also needs. One implementation, three consumers.
    match crate::selection::current_front_app_parts() {
        (Some(name), Some(bundle)) => Some(format!("{name} ({bundle})")),
        (Some(name), None) => Some(name),
        (None, Some(bundle)) => Some(bundle),
        (None, None) => None,
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn restore_focus_target_if_possible(target: Option<usize>) -> bool {
    use std::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsIconic, IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let Some(raw_target) = target else {
        log::warn!("[coord] no original Windows insertion target captured");
        return false;
    };
    let hwnd = HWND(raw_target as *mut c_void);
    if hwnd.0.is_null() {
        return false;
    }
    if !unsafe { IsWindow(hwnd).as_bool() } {
        log::warn!("[coord] original Windows insertion target is no longer a valid window");
        return false;
    }

    let foreground = unsafe { GetForegroundWindow() };
    if foreground == hwnd {
        return true;
    }

    if unsafe { IsIconic(hwnd).as_bool() } {
        let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }
    let _ = unsafe { SetForegroundWindow(hwnd) };
    std::thread::sleep(std::time::Duration::from_millis(60));

    let foreground = unsafe { GetForegroundWindow() };
    if foreground != hwnd {
        log::warn!("[coord] failed to restore original Windows insertion target before paste");
        return false;
    }
    true
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn restore_focus_target_if_possible(_target: Option<usize>) -> bool {
    true
}

/// Esc exclusivity decision: true when the capsule shows an in-progress state
/// (recording/transcribing/polishing) and this is really a dictation session (phase not
/// Idle) — tap/hook swallows Esc without passing it to the host app. The phase condition
/// specifically excludes QA: QA also uses the capsule, but its Esc is handled by the focused
/// floating window (#161); global swallowing would block it instead. Pure function for table tests.
fn esc_exclusive_for_capsule(state: CapsuleState, session_active: bool) -> bool {
    matches!(
        state,
        CapsuleState::Recording | CapsuleState::Transcribing | CapsuleState::Polishing
    ) && session_active
}

pub(super) fn emit_capsule(
    inner: &Arc<Inner>,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    message: Option<String>,
    inserted_chars: Option<u32>,
) -> u64 {
    emit_capsule_with_context(
        inner,
        state,
        level,
        elapsed_ms,
        message,
        inserted_chars,
        false,
    )
}

/// Selection polish reuses the existing no-focus capsule window, with a dedicated flag so the
/// frontend shows one lightweight status line without polluting voice/QA effects and terminal text.
pub(super) fn emit_selection_polish_capsule(
    inner: &Arc<Inner>,
    state: CapsuleState,
    message: impl Into<String>,
) -> u64 {
    emit_capsule_with_context(inner, state, 0.0, 0, Some(message.into()), None, true)
}

fn emit_capsule_with_context(
    inner: &Arc<Inner>,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    message: Option<String>,
    inserted_chars: Option<u32>,
    selection_polish: bool,
) -> u64 {
    let _event_guard = inner.capsule_event_lock.lock();
    emit_capsule_with_context_locked(
        inner,
        state,
        level,
        elapsed_ms,
        message,
        inserted_chars,
        selection_polish,
    )
}

fn defer_capsule_payload_if_fallback_active(inner: &Arc<Inner>, payload: &CapsulePayload) -> bool {
    inner.host.defer_capsule_if_fallback_active(payload)
}

/// Internal implementation with `capsule_event_lock` already held by the caller. The auto-hide
/// path must hold the lock from the epoch check until Idle is emitted, guaranteeing an old
/// timer cannot overwrite a just-arrived new payload.
fn emit_capsule_with_context_locked(
    inner: &Arc<Inner>,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    message: Option<String>,
    inserted_chars: Option<u32>,
    selection_polish: bool,
) -> u64 {
    let dictation = inner.backend.snapshot().dictation;
    let payload = CapsulePayload {
        session_id: dictation.session_id.map(|session| session.to_string()),
        state,
        level,
        elapsed_ms,
        message,
        inserted_chars,
        translation: !selection_polish && dictation.translation_active,
        operating: !selection_polish && inner.backend.less_computer_active_session().is_some(),
        warming: !selection_polish
            && state == CapsuleState::Recording
            && matches!(
                dictation.phase,
                openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording
            )
            && !dictation.recording_ready,
        selection_polish,
        capsule_style: inner.host.cached_capsule_style(),
    };
    let captured_revision = inner.host.capsule_revision();
    emit_capsule_payload_locked(inner, payload, captured_revision)
}

/// Core feedback enters the same native display outlet wholesale, including warming,
/// translation and other fields; it bypasses the old "rebuild payload from Host state" path.
pub(super) fn emit_core_capsule(
    inner: &Arc<Inner>,
    payload: CapsulePayload,
    expected_epoch: Option<u64>,
    captured_revision: u64,
) -> Option<u64> {
    emit_capsule_at_epoch(
        &inner.capsule_event_lock,
        &inner.capsule_event_epoch,
        expected_epoch,
        || emit_capsule_payload_locked(inner, payload, captured_revision),
    )
}

fn emit_capsule_at_epoch(
    event_lock: &Mutex<()>,
    current_epoch: &AtomicU64,
    expected_epoch: Option<u64>,
    emit: impl FnOnce() -> u64,
) -> Option<u64> {
    let _event_guard = event_lock.lock();
    if expected_epoch.is_some_and(|expected| current_epoch.load(Ordering::SeqCst) != expected) {
        return None;
    }
    Some(emit())
}

pub(super) fn hide_core_capsule_if_current(inner: &Arc<Inner>, expected_epoch: u64) {
    let _ = emit_capsule_at_epoch(
        &inner.capsule_event_lock,
        &inner.capsule_event_epoch,
        Some(expected_epoch),
        || emit_capsule_with_context_locked(inner, CapsuleState::Idle, 0.0, 0, None, None, false),
    );
}

fn emit_capsule_payload_locked(
    inner: &Arc<Inner>,
    payload: CapsulePayload,
    captured_revision: u64,
) -> u64 {
    let state = payload.state;
    let selection_polish = payload.selection_polish;
    // Advance the epoch on every payload. That way an old selection-polish terminal timer is
    // invalidated by any later selection / voice / QA state and cannot force the new visible
    // state back to Idle.
    let event_epoch = inner
        .capsule_event_epoch
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    inner
        .selection_polish_capsule_active
        .store(selection_polish, Ordering::SeqCst);
    // Recorded before the app-handle check so headless tests can assert "hotkey pressed → which
    // capsule appeared". `replace` also fetches the previous frame's state, used to detect an
    // entrance frame (see defer_capsule_emit below).
    let prev_state = inner.last_capsule_state.lock().replace(state);
    // Esc-exclusive window: while the capsule shows an in-progress state
    // (recording/transcribing/polishing) and this is really a dictation session (phase not
    // Idle), tap/hook swallows Esc without passing it to the host app — at that moment Esc
    // means "cancel this session", and double dispatch would also hit the host app's Esc (e.g.
    // cancelling a Claude reply being generated). The phase condition excludes QA: QA sessions
    // also use the capsule, but their Esc is handled by the focused floating window; swallowing
    // the key would block it instead. Terminal frames (Done/Cancelled/Error/Idle) clear it
    // naturally. emit_capsule is the single outlet for all session state changes (including all
    // termination paths per the #77 audit), so maintaining it here misses no path.
    #[cfg(all(not(mobile), target_os = "windows"))]
    let selection_voice_active = inner.selection_voice_capture.lock().is_some();
    #[cfg(not(all(not(mobile), target_os = "windows")))]
    let selection_voice_active = false;
    let session_active = inner.backend.snapshot().dictation.phase
        != openless_core::DictationPhase::Idle
        || inner.backend.less_computer_active_session().is_some()
        || selection_voice_active;
    let esc_exclusive = esc_exclusive_for_capsule(state, session_active);
    crate::hotkey::set_esc_exclusive(esc_exclusive);
    // Keep the latest full feedback during a fallback card, even before the window handle is
    // validated; a re-show must not regress to an old preparing state.
    defer_capsule_payload_if_fallback_active(inner, &payload);
    // Commit the replay frame before queueing any native window work. The
    // snapshot lock therefore has one revision for payload and transcript even
    // when run_on_main_thread is delayed.
    if !inner
        .host
        .commit_capsule_payload(&payload, captured_revision)
    {
        log::debug!("[capsule] skipped stale payload commit at revision {captured_revision}");
        return event_epoch;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return event_epoch;
    };

    #[cfg(target_os = "android")]
    crate::android::notify_capsule_state(&payload);

    // visible / translation are part of "this frame's capsule:state event payload" — they must
    // be computed at the call site (i.e. when the audio thread triggers emit_capsule); reading
    // them inside the main-thread closure would see the next frame's state, mismatching the
    // payload actually delivered to JS.
    let visible = !matches!(state, CapsuleState::Idle);
    // 入场帧：胶囊从不可见第一次变可见。按平时的「同步 emit + 异步 show」，前端会在窗口
    // 还隐藏时就起播 capsule-in，等窗口真 show 出来动画早已播完 → 用户看到胶囊「凭空出
    // 现」而非「滑入」。修法：入场帧把发给 capsule 窗口的事件推迟到主线程闭包里、
    // window.show 之后再 emit，保证前端起播入场动画时窗口已可见、动画完整可见。
    let was_visible = matches!(prev_state, Some(s) if !matches!(s, CapsuleState::Idle));
    let defer_capsule_emit = visible && !was_visible;

    // emit_capsule is called from the cpal process_callback (audio callback thread) at ~30 Hz —
    // calling NSWindow / HWND APIs on that thread hits the macOS dispatch_assert_queue_fail
    // SIGTRAP or a Win32 SendMessage deadlock. Marshal window.show/hide + position updates to
    // the main thread; app.emit_to uses Tauri's internal event bus, which is thread-safe, so it
    // stays synchronous. See audit 3.2.2.
    //
    // show_capsule (user preference) is read on the main thread: users can change settings
    // mid-recording, and the window between queueing the closure and running it is one or two
    // frames (~16-33ms); the latest value avoids stale-pref flicker. pr_agent concern — see
    // audit follow-up.
    let host_for_main = inner.host.clone();
    let backend_for_main = Arc::clone(&inner.backend);
    // An entrance frame must re-send the state to the frontend after window.show, inside the
    // closure, so it needs its own payload clone moved in; non-entrance frames use the
    // immediate synchronous emit outside the closure (below), so this is None.
    let payload_for_deferred_emit = if defer_capsule_emit {
        Some(payload.clone())
    } else {
        None
    };
    let payload_for_window = payload.clone();
    let _ = capsule.run_on_main_thread(move |capsule| {
        if !capsule.is_available_for(state) {
            return;
        }
        let preferences = backend_for_main.get_preferences();
        let show_capsule = payload_for_window.selection_polish || preferences.show_capsule;
        capsule.apply_capsule_payload(
            &payload_for_window,
            show_capsule,
            preferences.capsule_style,
            payload_for_deferred_emit.is_some(),
        );
        // Entrance frame: the window has just shown (or the user disabled capsule display and we
        // took the hide branch); only now send the state to the capsule frontend — the
        // capsule-in animation starts while the window is visible and plays from the beginning.
        if let Some(mut payload) = payload_for_deferred_emit {
            payload.capsule_style = preferences.capsule_style;
            host_for_main.emit_capsule_state_to_capsule(&payload);
        }
    });

    // 非入场帧（录音中的 level 更新、离场/终态）保持即时同步 emit，最低延迟；
    // 入场帧已在上面的主线程闭包里、window.show 之后 emit 过，这里跳过避免重复下发。
    if !defer_capsule_emit {
        inner.host.emit_capsule_state_to_capsule(&payload);
    }
    // The main window also needs the capsule:state event: AudioCueListener uses it to trigger
    // recording cues. On Linux the cue must still work while the capsule is hidden, so it is
    // also sent to the main window. Always immediate, decoupled from capsule window show timing.
    inner.host.emit_capsule_state_to_main(&payload);
    event_epoch
}

/// Whether a selection-polish terminal timer is still eligible to collapse the capsule.
///
/// This covers two races at once: a new trigger of the same feature, and a voice/QA session
/// that started afterwards.
pub(super) fn selection_polish_capsule_epoch_is_current(
    inner: &Arc<Inner>,
    expected_epoch: u64,
) -> bool {
    inner.selection_polish_capsule_active.load(Ordering::SeqCst)
        && inner.capsule_event_epoch.load(Ordering::SeqCst) == expected_epoch
}

/// Collapse path for old dictation/QA timers. It shares one short lock with all emits: it
/// yields if Selection Polish is showing, and if a new voice/QA session emitted first, that
/// emission is ordered before Idle by the lock.
pub(super) fn hide_capsule_if_all_sessions_idle(inner: &Arc<Inner>) {
    // Read session state first, then enter the capsule lock. The event epoch cancels this Idle
    // if any new payload arrived between the two reads.
    #[cfg(all(not(mobile), target_os = "windows"))]
    let selection_voice_idle = inner.selection_voice_capture.lock().is_none();
    #[cfg(not(all(not(mobile), target_os = "windows")))]
    let selection_voice_idle = true;
    let dictation_idle = inner.backend.snapshot().dictation.phase
        == openless_core::DictationPhase::Idle
        && inner.backend.less_computer_active_session().is_none()
        && selection_voice_idle;
    let selection_polish_active = inner.selection_polish_capsule_active.load(Ordering::SeqCst);
    let observed_epoch = inner.capsule_event_epoch.load(Ordering::SeqCst);
    if !dictation_idle || selection_polish_active {
        return;
    }

    let _event_guard = inner.capsule_event_lock.lock();
    if inner.capsule_event_epoch.load(Ordering::SeqCst) == observed_epoch
        && !inner.selection_polish_capsule_active.load(Ordering::SeqCst)
    {
        emit_capsule_with_context_locked(inner, CapsuleState::Idle, 0.0, 0, None, None, false);
    }
}

/// Collapse Selection Polish only if the same generation's terminal state is still the latest
/// visible capsule. The lock makes "check + send Idle" an unpreemptable sequence, so an old
/// timer can never overwrite the UI after a new session.
pub(super) fn hide_selection_polish_capsule_if_current(inner: &Arc<Inner>, expected_epoch: u64) {
    let _event_guard = inner.capsule_event_lock.lock();
    if selection_polish_capsule_epoch_is_current(inner, expected_epoch) {
        emit_capsule_with_context_locked(inner, CapsuleState::Idle, 0.0, 0, None, None, false);
    }
}

#[cfg(test)]
mod epoch_tests {
    use super::*;

    #[test]
    fn queued_qa_terminal_and_timer_cannot_hide_direct_native_feedback() {
        for initial in [CapsuleState::Polishing, CapsuleState::Error] {
            let lock = Mutex::new(());
            let epoch = AtomicU64::new(0);
            let visible = Mutex::new(CapsuleState::Idle);
            let write = |state| {
                assert!(
                    lock.try_lock().is_none(),
                    "the native write must remain under the epoch lock"
                );
                *visible.lock() = state;
                epoch.fetch_add(1, Ordering::SeqCst) + 1
            };
            let qa_epoch = emit_capsule_at_epoch(&lock, &epoch, None, || write(initial)).unwrap();
            // Selection Voice publishes directly, then its Core phase is already
            // terminal when the queued QA Answer or error timer reaches the host.
            {
                let _guard = lock.lock();
                write(CapsuleState::Error);
            }
            assert!(
                emit_capsule_at_epoch(&lock, &epoch, Some(qa_epoch), || write(CapsuleState::Idle))
                    .is_none()
            );
            assert_eq!(*visible.lock(), CapsuleState::Error);
            assert_eq!(epoch.load(Ordering::SeqCst), 2);
        }
    }
}

#[cfg(any())]
mod tests {
    use super::*;
    use crate::types::{CapsulePayload, CapsuleState, CapsuleStyle};

    fn payload(state: CapsuleState) -> CapsulePayload {
        CapsulePayload {
            session_id: None,
            state,
            level: 0.0,
            elapsed_ms: 0,
            message: None,
            inserted_chars: None,
            translation: false,
            operating: false,
            warming: false,
            selection_polish: false,
            capsule_style: CapsuleStyle::Siri,
        }
    }

    #[test]
    fn fallback_card_keeps_only_the_latest_deferred_capsule_payload() {
        let coordinator = Coordinator::new();
        coordinator.inner.host.begin_insert_fallback_card();

        assert!(defer_capsule_payload_if_fallback_active(
            &coordinator.inner,
            &payload(CapsuleState::Recording),
        ));
        assert!(defer_capsule_payload_if_fallback_active(
            &coordinator.inner,
            &payload(CapsuleState::Idle),
        ));
        let (was_visible, deferred) = coordinator.inner.host.dismiss_insert_fallback_card();
        assert!(was_visible);
        assert_eq!(
            deferred.map(|payload| payload.state),
            Some(CapsuleState::Idle)
        );
    }

    #[test]
    fn esc_exclusive_flag_matches_capsule_and_phase() {
        // In-progress capsule + dictation phase not Idle → exclusive Esc (not passed to the host app).
        for (state, phase) in [
            (CapsuleState::Recording, SessionPhase::Listening),
            (CapsuleState::Transcribing, SessionPhase::Processing),
            (CapsuleState::Polishing, SessionPhase::Processing),
            (CapsuleState::Recording, SessionPhase::Inserting),
        ] {
            assert!(
                esc_exclusive_for_capsule(state, phase),
                "{state:?} @ {phase:?} 应独占 Esc"
            );
        }

        // Terminal frames (Done/Cancelled/Error/Idle) → clear exclusivity.
        for (state, phase) in [
            (CapsuleState::Done, SessionPhase::Idle),
            (CapsuleState::Cancelled, SessionPhase::Idle),
            (CapsuleState::Error, SessionPhase::Idle),
            (CapsuleState::Idle, SessionPhase::Idle),
        ] {
            assert!(
                !esc_exclusive_for_capsule(state, phase),
                "{state:?} @ {phase:?} 不应独占 Esc"
            );
        }

        // QA scenario: capsule in progress but dictation phase=Idle → not exclusive (Esc belongs to the floating window, #161).
        for (state, phase) in [
            (CapsuleState::Recording, SessionPhase::Idle),
            (CapsuleState::Transcribing, SessionPhase::Idle),
            (CapsuleState::Polishing, SessionPhase::Idle),
        ] {
            assert!(
                !esc_exclusive_for_capsule(state, phase),
                "{state:?} @ {phase:?}（QA）不应独占 Esc"
            );
        }
    }
}
