//! 全局热键：复用 fcitx5 插件的信号（与 Linux 宿主是同一套实现）。
//!
//! 约束：`Fcitx5HotkeyListener` 不是 `Send` —— 必须在它自己的线程里创建和使用，
//! 所以监听常驻一个专用线程，事件在该线程里直接派发。
use openless_core::{DictationHotkeyEdge, OpenLessBackend};
use openless_linux_host::{Fcitx5HotkeyListener, LinuxHotkeyEvent};
use std::sync::{Arc, Mutex};

pub type MicMonitorSlot = Arc<Mutex<Option<openless_linux_host::MicrophoneMonitor>>>;

pub fn stop_monitor(slot: &MicMonitorSlot) {
    drop(slot.lock().unwrap().take());
}

fn dispatch_edge(
    backend: &Arc<OpenLessBackend>,
    runtime: &tokio::runtime::Runtime,
    edge: DictationHotkeyEdge,
    translation_requested: bool,
) {
    let backend = backend.clone();
    let mut task = Box::pin(async move {
        if let Err(error) = backend
            .dispatch_dictation_hotkey_edge_with_options(
                edge,
                openless_core::DictationStartOptions {
                    translation_requested,
                    ..Default::default()
                },
            )
            .await
        {
            log::warn!("[hotkey] dictation edge failed: {error}");
        }
    });
    // Start each edge in listener order. Do not wait for native startup: a
    // release or combined edge must still be able to cancel it.
    if runtime
        .block_on(async { futures_util::poll!(&mut task) })
        .is_pending()
    {
        runtime.spawn(task);
    }
}

/// 起热键监听：快捷键 → 听写 / 语音提问。
pub fn spawn_headless(
    backend: Arc<OpenLessBackend>,
    runtime: Arc<tokio::runtime::Runtime>,
    voice_edit: Arc<crate::voice_edit::VoiceEdit>,
    host_events: tokio::sync::broadcast::Sender<(String, String)>,
    mic_monitor: MicMonitorSlot,
) {
    std::thread::Builder::new()
        .name("openless-hotkeys".into())
        .spawn(move || match Fcitx5HotkeyListener::start() {
            Ok(listener) => {
                listener.set_waker(std::thread::current());
                println!("[linux-web] 热键监听已启动（听写 / QA）");
                let mut translation_at: Option<std::time::Instant> = None;
                let quick_note_gate = Arc::new(tokio::sync::Mutex::new(()));
                loop {
                    listener.drain(|event| match event {
                        LinuxHotkeyEvent::DictationPressed { press_id, at, .. } => {
                            stop_monitor(&mic_monitor);
                            dispatch_edge(
                                &backend,
                                &runtime,
                                DictationHotkeyEdge::Pressed { press_id, at },
                                translation_at.take().is_some_and(|armed| {
                                    at.saturating_duration_since(armed)
                                        < std::time::Duration::from_secs(1)
                                }),
                            );
                        }
                        LinuxHotkeyEvent::DictationReleased { press_id, at, .. } => dispatch_edge(
                            &backend,
                            &runtime,
                            DictationHotkeyEdge::Released { press_id, at },
                            false,
                        ),
                        LinuxHotkeyEvent::DictationCombined { press_id, at, .. } => dispatch_edge(
                            &backend,
                            &runtime,
                            DictationHotkeyEdge::Combined { press_id, at },
                            false,
                        ),
                        LinuxHotkeyEvent::SelectionPolishPressed => {
                            if voice_edit.take_arm() && backend.get_preferences().voice_edit_enabled
                            {
                                let event = match runtime.block_on(voice_edit.capture()) {
                                    Ok(()) => ("voice-edit:show".into(), "{}".into()),
                                    Err(error) => (
                                        "voice-edit:error".into(),
                                        serde_json::json!(error).to_string(),
                                    ),
                                };
                                let _ = host_events.send(event);
                                return;
                            }
                            // 选区助手：读当前选区（fcitx5 surrounding/selection）→
                            // 润色 → 用 SelectionRuntimeAdapter 替换回原处。
                            // previewConfirm 需要预览确认 UI，Linux 还没有，先按直替处理。
                            let backend = Arc::clone(&backend);
                            runtime.spawn(async move {
                                let mode = backend.get_preferences().selection_polish_output_mode;
                                if mode != openless_core::SelectionPolishOutputMode::DirectReplace {
                                    log::warn!(
                                        "[selection] previewConfirm is not available on Linux yet; \
                                         applying the polished text directly"
                                    );
                                }
                                match backend
                                    .services()
                                    .selection
                                    .begin_polish(openless_core::SelectionPolishRequest {
                                        selected_text: None,
                                        mode: openless_core::PolishMode::Raw,
                                        instruction: None,
                                    })
                                    .await
                                {
                                    Ok(session) => {
                                        log::info!(
                                            "[selection] polished selection session {session}"
                                        )
                                    }
                                    Err(error) => {
                                        log::warn!("[selection] polish failed: {error}");
                                    }
                                }
                            });
                        }
                        LinuxHotkeyEvent::QaPressed => {
                            stop_monitor(&mic_monitor);
                            // 问答热键：开/关一次语音提问，状态通过 qa:state 推给宿主。
                            let backend = Arc::clone(&backend);
                            runtime.spawn(async move {
                                if let Err(error) = backend.services().qa.toggle_recording().await {
                                    eprintln!("[linux-web] QA 切换失败：{error}");
                                }
                            });
                        }
                        LinuxHotkeyEvent::OpenAppPressed => {
                            let _ =
                                backend.request_host_action(openless_core::HostAction::ShowMain);
                        }
                        LinuxHotkeyEvent::SwitchStylePressed => {
                            if let Err(error) = backend.activate_previous_style_pack() {
                                log::warn!("[hotkey] {error}");
                            }
                        }
                        LinuxHotkeyEvent::StylePackPressed { symbol, states } => {
                            let prefs = backend.get_preferences();
                            if let Some(binding) = prefs.style_pack_hotkeys.iter().find(|entry| {
                                openless_linux_host::shortcut_to_raw(&entry.binding).ok()
                                    == Some((symbol, states))
                            }) {
                                if let Err(error) = backend.activate_style_pack(&binding.pack_id) {
                                    log::warn!("[hotkey] {error}");
                                }
                            }
                        }
                        LinuxHotkeyEvent::QuickNotePressed => {
                            stop_monitor(&mic_monitor);
                            let backend = backend.clone();
                            let gate = quick_note_gate.clone();
                            runtime.spawn(async move {
                                let Ok(_guard) = gate.try_lock() else { return };
                                // Match the desktop quick-note shortcut: it can
                                // redirect an in-progress capture (including a
                                // still-Starting capture) into a note, not just
                                // stop an already-labelled QuickNote session.
                                let snapshot = backend.snapshot().dictation;
                                let target = backend.dictation_output_target();
                                let result = match snapshot.session_id {
                                    Some(_) if matches!(snapshot.phase, openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording)
                                        && matches!(target, Some(openless_core::DictationOutputTarget::QuickNote | openless_core::DictationOutputTarget::Undecided)) =>
                                        backend.stop_dictation_with_options(openless_core::DictationStopOptions {
                                            quick_note: Some(true),
                                            ..Default::default()
                                        }).await.map(|_| ()),
                                    Some(_) => return,
                                    None => backend.start_dictation_with_options(openless_core::DictationStartOptions {
                                        output_target: openless_core::DictationOutputTarget::QuickNote,
                                        ..Default::default()
                                    }).await.map(|_| ()),
                                };
                                if let Err(error) = result {
                                    log::warn!("[quick note] {error}");
                                }
                            });
                        }
                        LinuxHotkeyEvent::TranslationPressed { pressed: true, .. } => {
                            if backend.snapshot().dictation.session_id.is_some() {
                                let backend = backend.clone();
                                runtime.spawn(async move {
                                    let _ =
                                        backend.update_dictation_translation_requested(true).await;
                                });
                            } else {
                                translation_at = Some(std::time::Instant::now());
                            }
                        }
                        LinuxHotkeyEvent::TranslationPressed { pressed: false, .. } => {}
                    });
                    if listener.take_error().is_some() {
                        eprintln!("[linux-web] fcitx5 热键监听断开");
                        break;
                    }
                    std::thread::park();
                }
            }
            Err(error) => eprintln!("[linux-web] fcitx5 热键监听启动失败：{error}"),
        })
        .ok();
}
