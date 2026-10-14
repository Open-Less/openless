use super::*;

#[tauri::command]
pub fn get_qa_hotkey_label(core: CoreState<'_>) -> String {
    core.get_preferences()
        .qa_hotkey
        .as_ref()
        .map(|binding| binding.display_label())
        .unwrap_or_default()
}

/// Sets the QA hotkey and hot-updates the monitor.
/// `None`-form fields are not supported here — when the frontend passes
/// `binding == null`, it calls the "disable" form below (writing
/// prefs.qa_hotkey = None).
#[tauri::command]
pub fn set_qa_hotkey(
    coord: CoordinatorState<'_>,
    binding: Option<ShortcutBinding>,
) -> Result<(), String> {
    if let Some(binding) = binding.as_ref() {
        crate::shortcut_binding::validate_binding(binding).map_err(|e| e.to_string())?;
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
        reject_bare_shift_dictation_shortcut(binding)?;
    }
    let mut prefs = coord.backend().get_preferences();
    prefs.qa_hotkey = binding;
    reject_hotkey_collisions(&prefs)?;
    super::settings::persist_strict_settings(&coord, prefs)
}

/// User clicks ✕ or presses Esc to close the QA panel.
#[tauri::command]
pub async fn qa_window_dismiss(core: CoreState<'_>) -> Result<(), String> {
    core.services()
        .qa
        .dismiss()
        .await
        .map_err(|error| error.message)
}

/// Mobile QA panel record button: Idle -> begin_qa_session, Recording ->
/// end_qa_session.
#[tauri::command]
pub async fn qa_toggle_recording(core: CoreState<'_>) -> Result<(), String> {
    core.services()
        .qa
        .toggle_recording()
        .await
        .map_err(|error| error.message)
}

/// QA panel keyboard input: reuses the voice-QA LLM pipeline, replacing only
/// the question source.
#[tauri::command]
pub async fn qa_submit_text(
    core: CoreState<'_>,
    text: String,
    expected_session_id: Option<openless_core::SessionId>,
    enforce_context: Option<bool>,
) -> Result<(), String> {
    if enforce_context.unwrap_or(false) {
        core.services()
            .qa
            .submit_text_in_context(text, expected_session_id)
            .await
            .map_err(|error| error.message)
    } else {
        core.services()
            .qa
            .submit_text(text)
            .await
            .map_err(|error| error.message)
    }
}

#[tauri::command]
pub async fn qa_get_snapshot(
    window: Window,
    core: CoreState<'_>,
) -> Result<openless_core::events::QaStateEvent, String> {
    if !matches!(window.label(), "qa" | "main") {
        return Err("qa_window_required".into());
    }
    let snapshot = core
        .services()
        .qa
        .snapshot()
        .await
        .map_err(|_| "qa_snapshot_unavailable")?;
    let mut event = openless_core::events::QaStateEvent::from_snapshot(&snapshot);
    event.edit_instruction_mode = Some(snapshot.edit_instruction_mode);
    event.edit_apply_available = Some(snapshot.edit_apply_available);
    event.edit_revert_available = Some(snapshot.edit_revert_available);
    Ok(event)
}

#[tauri::command]
pub async fn qa_window_set_expanded(window: Window, expanded: bool) -> Result<(), String> {
    use tauri::Manager;
    if window.label() != "qa" {
        return Err("qa_window_required".into());
    }
    let app = window.app_handle().clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .app_handle()
        .run_on_main_thread(move || {
            let _ = sender.send(crate::set_qa_window_expanded(&app, expanded));
        })
        .map_err(|_| "qa_main_thread_unavailable".to_string())?;
    receiver
        .await
        .map_err(|_| "qa_resize_cancelled".to_string())?
}

/// Selection QA panel "edit instruction" checkbox.
#[tauri::command]
pub async fn qa_set_edit_instruction_mode(
    core: CoreState<'_>,
    enabled: bool,
) -> Result<(), String> {
    core.services()
        .qa
        .set_edit_instruction_mode(enabled)
        .await
        .map_err(|error| error.message)
}

/// User clicks ✕ or presses Esc to close the Less Computer panel.
#[tauri::command]
pub async fn less_computer_window_dismiss(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.dismiss_less_computer().await
}

/// Chat panels (qa / less-computer) request keyboard focus.
///
/// Both panels display without stealing the foreground (macOS
/// orderFrontRegardless, never makeKey), so keystrokes never reach the webview
/// while the window is not the key window — the root cause of "clicked the
/// input box but cannot type".
///
/// macOS: the window is already a non-activating NSPanel
/// (make_chat_window_panel_macos), so makeKeyAndOrderFront gives the panel
/// keyboard focus without activating the app (same as Spotlight) — the earlier
/// window.set_focus() activated the whole app, dragged the main window
/// (settings) to the front, and made OpenLess frontmost so AX could not read
/// the original app's selection. Other platforms still use set_focus. Only the
/// two chat panel windows may call this (same tightening as
/// less_computer_approve).
#[tauri::command]
pub fn chat_panel_focus_keyboard(window: Window) -> Result<(), String> {
    let label = window.label();
    if label != "qa" && label != "less-computer" {
        return Err("chat_panel_focus_keyboard is only available to chat panels".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        let label = label.to_string();
        let app = window.app_handle().clone();
        // NSWindow operations must run on the main thread (macOS 26 hard
        // assertion); the exception guard keeps an AppKit raise from punching
        // through.
        let _ = window.app_handle().run_on_main_thread(move || {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            let Some(w) = app.get_webview_window(&label) else {
                return;
            };
            let Ok(handle) = w.ns_window() else {
                log::warn!("[chat-panel] ns_window unavailable; focus skipped");
                return;
            };
            let ns = handle as *mut AnyObject;
            if ns.is_null() {
                return;
            }
            // SAFETY: the closure performs a single ObjC message send with no
            // return value and runs no Rust destructors; unwinding past the
            // closure frame preserves memory safety.
            let result = unsafe {
                objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                    let nil: *mut AnyObject = std::ptr::null_mut();
                    let _: () = msg_send![ns, makeKeyAndOrderFront: nil];
                }))
            };
            if let Err(e) = result {
                log::warn!("[chat-panel] makeKeyAndOrderFront raised (caught): {e:?}");
            }
        });
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.set_focus().map_err(|e| e.to_string())
    }
}

/// Panel typed input: a text instruction goes straight into the Less Computer
/// execution chain (skipping recording and ASR).
#[tauri::command]
pub fn less_computer_submit_text(core: CoreState<'_>, coord: CoordinatorState<'_>, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let host = coord.tauri_host();
    let backend = Arc::clone(&*core);
    host.spawn(async move {
        if let Err(error) = backend.submit_less_computer(text).await {
            log::warn!("[less-computer] text submit run failed: {error}");
        }
    });
}

fn require_less_computer_window(window: &Window) -> Result<(), String> {
    if window.label() != "less-computer" {
        return Err("voice input can only be controlled from the Less Computer window".to_string());
    }
    Ok(())
}

/// Input-box microphone (dictate: transcription only fills the input box) /
/// voice-mode button (submit: same as the hotkey). Start failures return
/// directly to the panel as an inline hint, not into the conversation stream.
#[tauri::command]
pub async fn less_computer_voice_start(
    window: Window,
    coord: CoordinatorState<'_>,
    mode: openless_core::LessComputerVoiceMode,
) -> Result<(), String> {
    require_less_computer_window(&window)?;
    coord.start_less_computer_voice_from_panel(mode).await
}

/// Ends the current recording and finalizes per the session's own mode; the
/// finalize runs in the background without waiting for the Agent to finish.
#[tauri::command]
pub fn less_computer_voice_stop(
    window: Window,
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
) -> Result<(), String> {
    require_less_computer_window(&window)?;
    coord.stop_less_computer_voice_from_panel(session_id)
}

/// Cancels only the recording the panel names.
#[tauri::command]
pub async fn less_computer_voice_cancel(
    window: Window,
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
) -> Result<(), String> {
    require_less_computer_window(&window)?;
    coord
        .cancel_less_computer_voice_from_panel(session_id)
        .await
}

/// Stops the running Agent task; the panel stays open.
#[tauri::command]
pub async fn less_computer_task_cancel(
    window: Window,
    coord: CoordinatorState<'_>,
) -> Result<(), String> {
    require_less_computer_window(&window)?;
    coord.cancel_less_computer_task().await
}

/// Text testing entry from the main settings page. The panel itself neither
/// needs nor is allowed to call this command in reverse.
#[tauri::command]
pub async fn less_computer_window_open(
    window: Window,
    coord: CoordinatorState<'_>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Less Computer can only be opened from the main window".to_string());
    }
    let host = coord.tauri_host();
    // Windows WebView creation deadlocks inside a synchronous IPC handler.
    tauri::async_runtime::spawn_blocking(move || host.show_less_computer())
        .await
        .map_err(|error| format!("Less Computer window task failed: {error}"))?
}

/// Pulls the current session's event buffer (seq ascending) when the panel
/// mounts.
///
/// The panel's webview cold-loads on first creation, so backend events
/// (especially the first `user` — what the user said) are dropped before the
/// frontend registers its listener, showing "the AI is working but the panel
/// lacks what I said". After mounting, the frontend registers the listener
/// first and then calls this command to replay the backlog, deduplicating by
/// seq to join the live stream.
/// Session content is sensitive; only the less-computer window may call this
/// (same tightening as less_computer_approve).
#[tauri::command]
pub fn less_computer_sync(
    window: Window,
    core: CoreState<'_>,
    after_sequence: Option<u64>,
) -> Result<crate::coordinator::LessComputerEventReplay, String> {
    if window.label() != "less-computer" {
        return Err("sync can only be requested from the Less Computer window".to_string());
    }
    Ok(crate::coordinator::less_computer_event_replay_after(
        &core,
        after_sequence.unwrap_or(0),
    ))
}

/// Approve / Deny receipt of the inline approval card. The token is bound to
/// the pending interception action.
///
/// Security: the approval UI renders in the less-computer window
/// (LessComputerPanel), so only that window may submit, blocking main /
/// capsule / qa / glow and other windows from forging approvals — narrowing
/// the callable windows from 5 to 1.
#[tauri::command]
pub async fn less_computer_approve(
    window: Window,
    core: CoreState<'_>,
    token: String,
    approved: bool,
) -> Result<(), String> {
    if window.label() != "less-computer" {
        return Err("approval can only be submitted from the Less Computer window".to_string());
    }
    core.services()
        .less_computer
        .approve(token, approved)
        .await
        .map_err(|error| error.to_string())
}
