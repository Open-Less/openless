//! QA / chat-answer session lifecycle extracted from `coordinator.rs`
//! (behavior-preserving move).
//!
//! The selection-ask QA panel flow: finalize-from-dictation, begin/end/cancel
//! QA session, overlay transcription, and the chat-answer dispatcher.
//! References parent items via `use super::*;`; `pub(super)` so the parent and
//! sibling submodules (e.g. `qa`) reach them through `use qa_session::*;`.

use super::*;

// ─────────────────────────── QA session lifecycle ───────────────────────────

pub(super) async fn finalize_dictation_as_qa_question(inner: &Arc<Inner>) -> Result<(), String> {
    log::info!("[coord] QA finalize from overlay: capturing selection before opening panel");
    let selection = capture_selection();
    let selection_preview_text = selection.as_ref().map(|s| s.text.clone());

    log::info!("[coord] QA finalize from overlay: opening panel and waiting for ASR result");
    open_qa_panel(inner);
    {
        let mut state = inner.qa_state.lock();
        state.phase = QaPhase::Processing;
        state.cancelled = false;
        state.session_id = new_session_id();
        state.front_app = capture_frontmost_app();
        state.selection = selection;
    }
    inner.qa_stream_cancelled.store(false, Ordering::SeqCst);

    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "loading",
                "selection_preview": selection_preview_text,
                "messages": messages,
            }),
        );
    }

    let raw = match take_current_dictation_transcript_for_qa(inner).await {
        Ok(Some(raw)) => raw,
        Ok(None) => {
            log::info!("[coord] QA finalize from overlay: no transcript produced");
            finish_qa_idle_silently(inner);
            return Ok(());
        }
        Err(error) => {
            finish_qa_with_error(inner, error.clone());
            return Err(error);
        }
    };
    log::info!(
        "[coord] QA finalize from overlay: transcript ready chars={} duration_ms={}",
        raw.text.chars().count(),
        raw.duration_ms
    );
    answer_qa_question_text(inner, raw.text.trim().to_string(), raw.duration_ms).await
}

pub(super) async fn submit_qa_text_question(inner: &Arc<Inner>, text: String) -> Result<(), String> {
    let question = text.trim().to_string();
    if question.is_empty() {
        return Ok(());
    }

    {
        let mut state = inner.qa_state.lock();
        if !state.panel_visible {
            state.panel_visible = true;
            state.messages.clear();
            state.front_app = capture_frontmost_app();
            state.qa_focus_target = capture_focus_target();
        }
        if state.phase != QaPhase::Idle {
            return Err("QA is busy".to_string());
        }
        state.phase = QaPhase::Processing;
        state.cancelled = false;
        state.session_id = new_session_id();
        if state.selection.is_none() {
            state.selection = capture_selection();
        }
    }
    inner.qa_stream_cancelled.store(false, Ordering::SeqCst);

    let selection_preview_text = inner
        .qa_state
        .lock()
        .selection
        .as_ref()
        .map(|selection| selection.text.clone());
    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "thinking",
                "selection_preview": selection_preview_text,
                "messages": messages,
            }),
        );
    }

    answer_qa_question_text(inner, question, 0).await
}

pub(super) async fn take_current_dictation_transcript_for_qa(
    inner: &Arc<Inner>,
) -> Result<Option<RawTranscript>, String> {
    wait_for_dictation_listening(inner).await?;

    let current_session_id = {
        let mut state = inner.state.lock();
        let Some(session_id) = start_processing_if_listening(&mut state) else {
            return Ok(None);
        };
        session_id
    };

    let elapsed = inner.state.lock().started_at.elapsed().as_millis() as u64;
    emit_capsule(inner, CapsuleState::Transcribing, 0.0, elapsed, None, None);

    if let Some(rec) = take_recorder_for_session(inner, current_session_id) {
        rec.stop();
        release_recording_mute(inner, "dictation");
    }

    let Some(asr) = take_asr_for_session(inner, current_session_id) else {
        restore_prepared_windows_ime_session(inner, current_session_id);
        set_phase_idle_if_session_matches(inner, current_session_id);
        return Ok(None);
    };

    let mut raw = match transcribe_overlay_dictation_asr(inner, current_session_id, asr).await {
        Ok(raw) => raw,
        Err(error) => {
            restore_prepared_windows_ime_session(inner, current_session_id);
            set_phase_idle_if_session_matches(inner, current_session_id);
            finish_qa_with_error(inner, format!("识别失败: {error}"));
            return Err(error);
        }
    };

    if inner.state.lock().cancelled {
        log::info!("[coord] overlay QA: cancel detected after ASR — discarding transcript");
        restore_prepared_windows_ime_session(inner, current_session_id);
        {
            let mut state = inner.state.lock();
            state.phase = SessionPhase::Idle;
            state.focus_target = None;
        }
        return Ok(None);
    }

    #[cfg(any(debug_assertions, test))]
    if raw.text.trim().is_empty() {
        if let Some(debug_text) = debug_transcript_override_text() {
            raw.text = debug_text;
        }
    }

    if raw.text.trim().is_empty() {
        restore_prepared_windows_ime_session(inner, current_session_id);
        set_phase_idle_if_session_matches(inner, current_session_id);
        finish_qa_idle_silently(inner);
        return Ok(None);
    }

    if let Ok(rules) = inner.correction_rules.list() {
        let corrected = apply_correction_rules(&raw.text, &rules);
        if corrected != raw.text {
            raw.text = corrected;
        }
    }

    restore_prepared_windows_ime_session(inner, current_session_id);
    {
        let mut state = inner.state.lock();
        state.phase = SessionPhase::Idle;
        state.focus_target = None;
    }
    Ok(Some(raw))
}

pub(super) async fn wait_for_dictation_listening(inner: &Arc<Inner>) -> Result<(), String> {
    const MAX_WAIT_MS: u64 = 3_000;
    const STEP_MS: u64 = 20;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(MAX_WAIT_MS);

    loop {
        let phase = { inner.state.lock().phase };
        match phase {
            SessionPhase::Starting if std::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(STEP_MS)).await;
            }
            SessionPhase::Starting => {
                return Err("dictation startup timed out before QA finalize".to_string());
            }
            _ => return Ok(()),
        }
    }
}

pub(super) async fn transcribe_overlay_dictation_asr(
    _inner: &Arc<Inner>,
    _current_session_id: SessionId,
    asr: ActiveAsr,
) -> Result<RawTranscript, String> {
    let uses_global_timeout = asr_transcribe_uses_global_timeout(&asr);
    match asr {
        ActiveAsr::Volcengine(asr) => {
            debug_assert!(uses_global_timeout);
            if let Err(error) = asr.send_last_frame().await {
                log::error!("[coord] overlay QA: send last frame failed: {error}");
            }
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                Ok(Ok(raw)) => Ok(raw),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => {
                    asr.cancel();
                    Err("global timeout".to_string())
                }
            }
        }
        ActiveAsr::Bailian(asr) => {
            debug_assert!(uses_global_timeout);
            if let Err(error) = asr.send_last_frame().await {
                log::error!("[coord] overlay QA: Bailian send last frame failed: {error}");
            }
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                Ok(Ok(raw)) => Ok(raw),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => {
                    asr.cancel();
                    Err("bailian global timeout".to_string())
                }
            }
        }
        ActiveAsr::Whisper(whisper) => {
            debug_assert!(uses_global_timeout);
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, whisper.transcribe()).await {
                Ok(Ok(raw)) => Ok(raw),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("whisper global timeout".to_string()),
            }
        }
        ActiveAsr::Mimo(mimo) => {
            debug_assert!(uses_global_timeout);
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, mimo.transcribe()).await {
                Ok(Ok(raw)) => Ok(raw),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("mimo global timeout".to_string()),
            }
        }
        #[cfg(target_os = "windows")]
        ActiveAsr::FoundryLocalWhisper(local) => {
            debug_assert!(!uses_global_timeout);
            match local
                .transcribe(foundry_audio_transcribe_timeout_duration())
                .await
            {
                Ok(raw) => {
                    schedule_foundry_local_asr_release(
                        _inner,
                        AsrReleaseSession::Dictation(_current_session_id),
                    );
                    Ok(raw)
                }
                Err(error) => {
                    schedule_foundry_local_asr_release(
                        _inner,
                        AsrReleaseSession::Dictation(_current_session_id),
                    );
                    Err(error.to_string())
                }
            }
        }
        #[cfg(target_os = "windows")]
        ActiveAsr::SherpaOnnxLocal(local) => {
            debug_assert!(!uses_global_timeout);
            match local
                .transcribe(sherpa_audio_transcribe_timeout_duration())
                .await
            {
                Ok(raw) => {
                    schedule_sherpa_onnx_release(
                        _inner,
                        AsrReleaseSession::Dictation(_current_session_id),
                    );
                    Ok(raw)
                }
                Err(error) => {
                    schedule_sherpa_onnx_release(
                        _inner,
                        AsrReleaseSession::Dictation(_current_session_id),
                    );
                    Err(error.to_string())
                }
            }
        }
        #[cfg(target_os = "macos")]
        ActiveAsr::Local(local) => {
            debug_assert!(uses_global_timeout);
            let audio_secs = (local.buffer_duration_ms() as f64) / 1000.0;
            let timeout_duration = local_qwen_transcribe_timeout(audio_secs);
            let result = tokio::time::timeout(timeout_duration, local.transcribe()).await;
            _inner.local_asr_cache.touch();
            schedule_local_asr_release(_inner);
            match result {
                Ok(Ok(raw)) => Ok(raw),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("local qwen transcribe timeout".to_string()),
            }
        }
        #[cfg(target_os = "macos")]
        ActiveAsr::AppleSpeech(local) => {
            debug_assert!(uses_global_timeout);
            match tokio::time::timeout(
                std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS),
                local.transcribe(),
            )
            .await
            {
                Ok(Ok(raw)) => Ok(raw),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("apple speech transcribe timeout".to_string()),
            }
        }
    }
}

pub(super) async fn answer_qa_question_text(
    inner: &Arc<Inner>,
    question: String,
    duration_ms: u64,
) -> Result<(), String> {
    if question.trim().is_empty() {
        finish_qa_idle_silently(inner);
        return Ok(());
    }

    let user_content = {
        let st = inner.qa_state.lock();
        let is_first_turn = st.messages.is_empty();
        let sel_text = st
            .selection
            .as_ref()
            .map(|s| s.text.clone())
            .unwrap_or_default();
        if is_first_turn && !sel_text.trim().is_empty() {
            format!(
                "# 选区原文\n{}\n\n# 我的问题\n{}",
                sel_text.trim(),
                question
            )
        } else {
            question.clone()
        }
    };

    inner
        .qa_state
        .lock()
        .messages
        .push(crate::types::QaChatMessage {
            role: "user".to_string(),
            content: user_content,
        });

    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "thinking",
                "messages": messages,
            }),
        );
    }

    emit_capsule(inner, CapsuleState::Polishing, 0.0, 0, None, None);

    let prefs = inner.prefs.get();
    let working_languages = prefs.working_languages.clone();
    let chinese_script_preference = prefs.chinese_script_preference;
    let output_language_preference = prefs.output_language_preference;
    let llm_thinking_enabled = prefs.llm_thinking_enabled;
    let (messages_for_llm, front_app) = {
        let st = inner.qa_state.lock();
        (st.messages.clone(), st.front_app.clone())
    };

    let captured_session_id = inner.qa_state.lock().session_id;
    let inner_for_delta = Arc::clone(inner);
    let on_delta = move |chunk: &str| {
        let cur_id = inner_for_delta.qa_state.lock().session_id;
        if cur_id != captured_session_id {
            return;
        }
        if let Some(app) = inner_for_delta.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "answer_delta",
                    "chunk": chunk,
                }),
            );
        }
    };

    let cancel_flag = Arc::clone(&inner.qa_stream_cancelled);
    let should_cancel = move || cancel_flag.load(Ordering::Relaxed);

    let answer = match answer_chat_dispatch(
        &messages_for_llm,
        &working_languages,
        chinese_script_preference,
        output_language_preference,
        llm_thinking_enabled,
        front_app.as_deref(),
        on_delta,
        should_cancel,
    )
    .await
    {
        Ok(answer) => answer,
        Err(error) => {
            inner.qa_state.lock().messages.pop();
            finish_qa_with_error(inner, format!("回答失败: {error}"));
            return Err(error.to_string());
        }
    };

    if inner.qa_state.lock().cancelled {
        inner.qa_state.lock().messages.pop();
        finish_qa_idle_silently(inner);
        return Ok(());
    }

    inner
        .qa_state
        .lock()
        .messages
        .push(crate::types::QaChatMessage {
            role: "assistant".to_string(),
            content: answer.clone(),
        });

    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "answer",
                "messages": messages,
            }),
        );
    }

    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);

    if prefs.qa_save_history {
        let session = DictationSession {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now().to_rfc3339(),
            raw_transcript: question.clone(),
            final_text: answer,
            mode: PolishMode::Raw,
            style_pack_id: None,
            translation_active: false,
            polish_source: None,
            app_bundle_id: None,
            app_name: front_app,
            insert_status: InsertStatus::CopiedFallback,
            error_code: Some("qaSession".to_string()),
            duration_ms: Some(duration_ms),
            dictionary_entry_count: None,
            has_audio_recording: None,
        };
        let prefs_snapshot = inner.prefs.get();
        if let Err(error) = inner.history.append_with_retention(
            session,
            prefs_snapshot.history_retention_days,
            prefs_snapshot.history_max_entries,
        ) {
            log::error!("[coord] overlay QA history append failed: {error}");
        }
    }

    inner.qa_state.lock().phase = QaPhase::Idle;
    Ok(())
}

/// 划词语音问答会话（issue #118）。
///
/// 与 dictation 完全分离：
/// - 不进 SessionPhase（互不抢锁）
/// - 不写 history.json（除非 prefs.qa_save_history=true 才旁路写一条 placeholder）
/// - 用独立的 qa_recorder + qa_asr，复用现有 Volcengine ASR 通路
pub(super) async fn begin_qa_session(inner: &Arc<Inner>) -> Result<(), String> {
    {
        let mut state = inner.qa_state.lock();
        if !state.panel_visible {
            // 防御：浮窗没开就被叫到这里说明路由错了，直接退出。
            return Ok(());
        }
        if state.phase != QaPhase::Idle {
            return Ok(());
        }
        state.phase = QaPhase::Recording;
        state.cancelled = false;
        state.session_id = new_session_id();
        state.front_app = capture_frontmost_app();
        state.selection = None;
    }
    // 重置 SSE 取消标志：上一轮可能 set 过的 true 留着会让本轮流式立即 break。
    inner.qa_stream_cancelled.store(false, Ordering::SeqCst);

    // 抓选区。每轮按 Option 都重新抓一次：用户多轮提问中可以重新选别处文字。
    //
    // - macOS：浮窗走 orderFrontRegardless，不成为 key window，原 app 仍是 frontmost，
    //   AX/Cmd+C fallback 都能拿到。
    // - Windows：#466 修复后 show_qa_window_no_activate 主动抓焦点，QA 此刻已是前台，
    //   simulate_copy 会跑在 QA 自己 webview 上 → 抓不到。focus-dance 上半场：把焦点临时
    //   还给"用户原 app 的 HWND"。
    //
    //   多轮场景的目标刷新：用户开 QA 后可能 Alt+Tab 切到别的 app 选新文字。如果还死认
    //   open_qa_panel 时记下的初始 HWND，会把焦点抢回错的 app（pr_agent stale-focus 关注点）。
    //   策略：每轮先看当前前台是不是本进程的窗口（QA / capsule / main）—— 是 → 用户没切
    //   走，沿用 saved；不是 → 用户切到了真正的外部 app，刷新 saved 为当前 HWND。
    //   抓完选区后下半场再把焦点交还 QA，让 ESC/X 继续可用。
    #[cfg(target_os = "windows")]
    {
        // 合并两次 lock：原来分 lock #1 写 + lock #2 读，两者之间 close_qa_panel 在别的
        // 线程把 qa_focus_target 清成 None 会被覆盖回旧 HWND。Cloud 评审指出的 TOCTOU。
        // 单次加锁里既写最新外部前台、再读出来交给后面的 restore_focus_target_if_possible
        // —— capture_external_focus_target() 内部只调 GetForegroundWindow / pid 查询，
        // 不会反向取 qa_state 锁，持锁期间调用安全。
        let saved_target = {
            let mut state = inner.qa_state.lock();
            if let Some(current_external) = capture_external_focus_target() {
                state.qa_focus_target = Some(current_external);
            }
            state.qa_focus_target
        };
        let _ = restore_focus_target_if_possible(saved_target);
    }
    let selection = capture_selection();
    #[cfg(target_os = "windows")]
    if let Some(app) = inner.app.lock().clone() {
        crate::refocus_qa_window(&app);
    }
    let selection_preview_text = selection.as_ref().map(|s| s.text.clone());
    inner.qa_state.lock().selection = selection.clone();

    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "recording",
                "selection_preview": selection_preview_text,
                "messages": messages,
            }),
        );
    }

    // 2. QA 与 dictation 使用同一个 active ASR 入口。不要回退火山，否则用户配置
    // 百炼 / Whisper / 本地 ASR 后，浮窗仍会偷偷走另一套凭据。
    let active_asr = CredentialsVault::get_active_asr();
    if let Err(message) = ensure_asr_credentials() {
        log::warn!("[coord] QA: active ASR credentials missing: {message}");
        finish_qa_with_error(inner, format!("缺少 ASR 凭据：{message}"));
        return Err(message);
    }

    if let Err(message) = ensure_microphone_permission(inner) {
        log::warn!("[coord] QA: microphone permission gate failed: {message}");
        finish_qa_with_error(inner, message.clone());
        return Err(message);
    }

    let qa_asr = match build_qa_asr_start(inner, &active_asr).await {
        Ok(qa_asr) => qa_asr,
        Err(message) => {
            log::error!("[coord] QA active ASR init failed: {message}");
            finish_qa_with_error(inner, format!("ASR 初始化失败: {message}"));
            return Err(message);
        }
    };
    let consumer = qa_asr.recorder_consumer();
    *inner.qa_asr.lock() = Some(qa_asr.active_asr());

    // QA recorder 不需要 RMS 节流到胶囊；前端 QA 浮窗有自己的电平视图，
    // Android 的 QA 面板嵌在 main WebView；桌面端仍发给独立 qa 窗口。
    let inner_for_level = Arc::clone(inner);
    let last_emit_at = Arc::new(Mutex::new(None::<Instant>));
    const LEVEL_EMIT_MIN_INTERVAL_MS: u64 = 33;
    let level_handler: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level| {
        let phase = inner_for_level.qa_state.lock().phase;
        if phase != QaPhase::Recording {
            return;
        }
        let now = Instant::now();
        {
            let mut last = last_emit_at.lock();
            if let Some(prev) = *last {
                if now.duration_since(prev).as_millis() < LEVEL_EMIT_MIN_INTERVAL_MS as u128 {
                    return;
                }
            }
            *last = Some(now);
        }
        if let Some(app) = inner_for_level.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:level",
                serde_json::json!({ "level": level }),
            );
        }
        // 同步把电平推给底部胶囊，让 QA 录音也有跟主听写一致的可视反馈。
        emit_capsule(
            &inner_for_level,
            CapsuleState::Recording,
            level,
            0,
            None,
            None,
        );
    });

    let microphone_device_name = selected_microphone_device_name(inner);
    stop_microphone_preview_monitor(inner, "QA recorder");
    acquire_recording_mute(inner, "qa").await;
    // QA 默认不留痕（qa_save_history 默认 false），录音文件归档也跟着不开。
    // 调试 QA 麦克风请用主听写路径。
    match Recorder::start(microphone_device_name, consumer, level_handler, None) {
        Ok((rec, runtime_errors, archive_active)) => {
            // QA 路径不写 dictation 的 history，但仍把 archive 状态归零，避免 dictation
            // 接力时读到上一个 QA session 的过期值。
            inner
                .audio_archive_active
                .store(archive_active, std::sync::atomic::Ordering::Relaxed);
            *inner.qa_recorder.lock() = Some(rec);
            // QA 也跟主听写一样监听 cpal runtime error。设备中途消失 / panic 时
            // 不能让 QA 永远卡在 Recording 没反馈。详见 issue #168。
            spawn_qa_recorder_error_monitor(inner, runtime_errors);
        }
        Err(e) => {
            log::error!("[coord] QA recorder start failed: {e}");
            if let Some(asr) = inner.qa_asr.lock().take() {
                cancel_active_asr(asr);
            }
            release_recording_mute(inner, "qa");
            finish_qa_with_error(inner, format!("录音启动失败: {e}"));
            return Err(e.to_string());
        }
    }

    if let Err(e) = qa_asr.open_streaming_session().await {
        log::error!("[coord] QA: open ASR session failed: {e}");
        stop_qa_recorder(inner);
        if let Some(asr) = inner.qa_asr.lock().take() {
            cancel_active_asr(asr);
        }
        finish_qa_with_error(inner, format!("ASR 连接失败: {e}"));
        return Err(e);
    }

    // cancel race：在 await 期间用户可能 dismiss 了浮窗。
    if inner.qa_state.lock().cancelled {
        log::info!("[coord] QA cancel raced during open_session — aborting begin");
        if let Some(asr) = inner.qa_asr.lock().take() {
            cancel_active_asr(asr);
        }
        stop_qa_recorder(inner);
        inner.qa_state.lock().phase = QaPhase::Idle;
        return Ok(());
    }

    // QA 无「预备态」语义（不走等麦克风预热的乐观显示），显式清掉 capsule_warming ——
    // 否则若上一次听写在拿到首帧 PCM 前异常早退、warming 停在 true，这里的 QA 录音胶囊会
    // 读到陈旧 true 卡在「待命」收拢态（QA 的 level_handler 不翻这个标志）。审核 follow-up。
    inner.capsule_warming.store(false, Ordering::SeqCst);
    // 显式弹胶囊到 Recording。level_handler 后续会持续推电平，胶囊里"录音中…"
    // 的视觉反馈跟主听写完全一致。
    emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);

    Ok(())
}

pub(super) async fn end_qa_session(inner: &Arc<Inner>) -> Result<(), String> {
    {
        let mut state = inner.qa_state.lock();
        if state.phase != QaPhase::Recording {
            return Ok(());
        }
        state.phase = QaPhase::Processing;
    }

    // 胶囊进入 Transcribing：用户视觉上看到"识别中"。
    emit_capsule(inner, CapsuleState::Transcribing, 0.0, 0, None, None);

    if let Some(app) = inner.app.lock().clone() {
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({ "kind": "loading" }),
        );
    }

    stop_qa_recorder(inner);

    let asr = match inner.qa_asr.lock().take() {
        Some(a) => a,
        None => {
            inner.qa_state.lock().phase = QaPhase::Idle;
            return Ok(());
        }
    };

    #[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
    let qa_session_id = inner.qa_state.lock().session_id;
    let uses_global_timeout = asr_transcribe_uses_global_timeout(&asr);
    let raw = match asr {
        ActiveAsr::Volcengine(asr) => {
            debug_assert!(uses_global_timeout);
            if let Err(e) = asr.send_last_frame().await {
                log::error!("[coord] QA: send last frame failed: {e}");
            }
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] QA: await final failed: {e}");
                    finish_qa_with_error(inner, format!("识别失败: {e}"));
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] QA: 全局超时 {} 秒 - 强制恢复",
                        COORDINATOR_GLOBAL_TIMEOUT_SECS
                    );
                    asr.cancel();
                    finish_qa_with_error(inner, "识别超时".to_string());
                    return Err("global timeout".to_string());
                }
            }
        }
        ActiveAsr::Bailian(asr) => {
            debug_assert!(uses_global_timeout);
            if let Err(e) = asr.send_last_frame().await {
                log::error!("[coord] QA: Bailian send last frame failed: {e}");
            }
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] QA: Bailian await final failed: {e}");
                    finish_qa_with_error(inner, format!("识别失败: {e}"));
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] QA: Bailian 全局超时 {} 秒",
                        COORDINATOR_GLOBAL_TIMEOUT_SECS
                    );
                    asr.cancel();
                    finish_qa_with_error(inner, "识别超时".to_string());
                    return Err("bailian global timeout".to_string());
                }
            }
        }
        ActiveAsr::Whisper(w) => {
            debug_assert!(uses_global_timeout);
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, w.transcribe()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] QA: whisper transcribe failed: {e}");
                    finish_qa_with_error(inner, format!("识别失败: {e}"));
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] QA: whisper 全局超时 {} 秒",
                        COORDINATOR_GLOBAL_TIMEOUT_SECS
                    );
                    finish_qa_with_error(inner, "识别超时".to_string());
                    return Err("whisper global timeout".to_string());
                }
            }
        }
        ActiveAsr::Mimo(m) => {
            debug_assert!(uses_global_timeout);
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, m.transcribe()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] QA: MiMo ASR transcribe failed: {e}");
                    finish_qa_with_error(inner, format!("识别失败: {e}"));
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] QA: MiMo ASR 全局超时 {} 秒",
                        COORDINATOR_GLOBAL_TIMEOUT_SECS
                    );
                    finish_qa_with_error(inner, "识别超时".to_string());
                    return Err("mimo global timeout".to_string());
                }
            }
        }
        #[cfg(target_os = "windows")]
        ActiveAsr::FoundryLocalWhisper(local) => {
            debug_assert!(!uses_global_timeout);
            match local
                .transcribe(foundry_audio_transcribe_timeout_duration())
                .await
            {
                Ok(r) => {
                    schedule_foundry_local_asr_release(inner, AsrReleaseSession::Qa(qa_session_id));
                    r
                }
                Err(e) => {
                    schedule_foundry_local_asr_release(inner, AsrReleaseSession::Qa(qa_session_id));
                    if inner.qa_state.lock().cancelled {
                        log::info!(
                            "[coord] QA Foundry Local Whisper transcribe cancelled — discarding transcript"
                        );
                        finish_qa_idle_silently(inner);
                        return Ok(());
                    }
                    log::error!("[coord] QA Foundry Local Whisper transcribe failed: {e:#}");
                    finish_qa_with_error(inner, format!("本地识别失败: {e}"));
                    return Err(e.to_string());
                }
            }
        }
        #[cfg(target_os = "windows")]
        ActiveAsr::SherpaOnnxLocal(local) => {
            debug_assert!(!uses_global_timeout);
            match local
                .transcribe(sherpa_audio_transcribe_timeout_duration())
                .await
            {
                Ok(r) => {
                    schedule_sherpa_onnx_release(inner, AsrReleaseSession::Qa(qa_session_id));
                    r
                }
                Err(e) => {
                    schedule_sherpa_onnx_release(inner, AsrReleaseSession::Qa(qa_session_id));
                    if inner.qa_state.lock().cancelled {
                        log::info!(
                            "[coord] QA sherpa-onnx transcribe cancelled — discarding transcript"
                        );
                        finish_qa_idle_silently(inner);
                        return Ok(());
                    }
                    log::error!("[coord] QA sherpa-onnx transcribe failed: {e:#}");
                    finish_qa_with_error(inner, format!("本地识别失败: {e}"));
                    return Err(e.to_string());
                }
            }
        }
        #[cfg(target_os = "macos")]
        ActiveAsr::Local(local) => {
            debug_assert!(uses_global_timeout);
            let audio_secs = (local.buffer_duration_ms() as f64) / 1000.0;
            let timeout_duration = local_qwen_transcribe_timeout(audio_secs);
            log::info!(
                "[coord] QA local Qwen3-ASR transcribe: audio={:.2}s timeout={}s",
                audio_secs,
                timeout_duration.as_secs()
            );
            let result = tokio::time::timeout(timeout_duration, local.transcribe()).await;
            inner.local_asr_cache.touch();
            schedule_local_asr_release(inner);
            match result {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] QA local Qwen3-ASR transcribe failed: {e:#}");
                    finish_qa_with_error(inner, format!("本地识别失败: {e}"));
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] QA local Qwen3-ASR transcribe timeout after {}s",
                        timeout_duration.as_secs()
                    );
                    finish_qa_with_error(inner, "本地识别超时".to_string());
                    return Err("local qwen transcribe timeout".to_string());
                }
            }
        }
        #[cfg(target_os = "macos")]
        ActiveAsr::AppleSpeech(local) => {
            debug_assert!(uses_global_timeout);
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, local.transcribe()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] QA Apple Speech transcribe failed: {e:#}");
                    finish_qa_with_error(inner, format!("本地识别失败: {e}"));
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!("[coord] QA Apple Speech transcribe timeout");
                    finish_qa_with_error(inner, "本地识别超时".to_string());
                    return Err("apple speech transcribe timeout".to_string());
                }
            }
        }
    };

    // cancel race：用户在 transcribe 中按 Esc / dismiss → 静默退出。
    if inner.qa_state.lock().cancelled {
        log::info!("[coord] QA cancel detected after ASR — discarding transcript");
        finish_qa_idle_silently(inner);
        return Ok(());
    }

    let question = raw.text.trim().to_string();
    if question.is_empty() {
        // 静默录音：不调 LLM，不弹错误，直接关浮窗。
        log::info!("[coord] QA: empty transcript → silent dismiss");
        finish_qa_idle_silently(inner);
        return Ok(());
    }

    // 拼这一轮的 user 消息：第一轮（messages 还空）把选区原文嵌进去；
    // 之后的轮次只送提问，让 LLM 顺着上下文回答。详见 issue #118 v2。
    let user_content = {
        let st = inner.qa_state.lock();
        let is_first_turn = st.messages.is_empty();
        let sel_text = st
            .selection
            .as_ref()
            .map(|s| s.text.clone())
            .unwrap_or_default();
        if is_first_turn && !sel_text.trim().is_empty() {
            format!(
                "# 选区原文\n{}\n\n# 我的问题\n{}",
                sel_text.trim(),
                question
            )
        } else {
            question.clone()
        }
    };

    inner
        .qa_state
        .lock()
        .messages
        .push(crate::types::QaChatMessage {
            role: "user".to_string(),
            content: user_content,
        });

    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "thinking",
                "messages": messages,
            }),
        );
    }

    // 胶囊：思考阶段（复用 dictation 的 Polishing 状态——视觉上是"润色中"，QA 借用一下）。
    emit_capsule(inner, CapsuleState::Polishing, 0.0, 0, None, None);

    let prefs = inner.prefs.get();
    let working_languages = prefs.working_languages.clone();
    let chinese_script_preference = prefs.chinese_script_preference;
    let output_language_preference = prefs.output_language_preference;
    let llm_thinking_enabled = prefs.llm_thinking_enabled;
    let (messages_for_llm, front_app) = {
        let st = inner.qa_state.lock();
        (st.messages.clone(), st.front_app.clone())
    };

    // 流式回调：每个 SSE delta 立刻推一帧 qa:state{kind:"answer_delta"} 给前端，
    // 浮窗里气泡边收边长。最终的 messages 由 answer 事件统一下发（保证一致性）。
    //
    // session_id 守卫（issue #161）：闭包捕获本会话 id；用户取消 → 关浮窗 → 开新浮窗
    // 开新一轮时，旧的 in-flight LLM 流仍可能 emit chunk，必须在 emit 前比对当前
    // qa_state.session_id == 捕获 id，否则跳过——避免旧会话的字漏进新气泡。
    let captured_session_id = inner.qa_state.lock().session_id;
    let inner_for_delta = Arc::clone(inner);
    let on_delta = move |chunk: &str| {
        let cur_id = inner_for_delta.qa_state.lock().session_id;
        if cur_id != captured_session_id {
            return; // 旧 session 漏来的 chunk，丢弃
        }
        if let Some(app) = inner_for_delta.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "answer_delta",
                    "chunk": chunk,
                }),
            );
        }
    };

    // SSE 流取消旗标：cancel_qa_session / close_qa_panel 会 set true，
    // polish 的 SSE loop 每帧检查 → break，释放 HTTP body。详见 issue #161。
    let cancel_flag = Arc::clone(&inner.qa_stream_cancelled);
    let should_cancel = move || cancel_flag.load(Ordering::Relaxed);

    let answer = match answer_chat_dispatch(
        &messages_for_llm,
        &working_languages,
        chinese_script_preference,
        output_language_preference,
        llm_thinking_enabled,
        front_app.as_deref(),
        on_delta,
        should_cancel,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            log::error!("[coord] QA: LLM answer failed: {e}");
            // 把刚 push 的 user 消息回滚，避免 retry 重复
            inner.qa_state.lock().messages.pop();
            finish_qa_with_error(inner, format!("回答失败: {e}"));
            return Err(e.to_string());
        }
    };

    if inner.qa_state.lock().cancelled {
        log::info!("[coord] QA cancel detected before answer — discarding");
        // 同样回滚未配对的 user 消息
        inner.qa_state.lock().messages.pop();
        finish_qa_idle_silently(inner);
        return Ok(());
    }

    inner
        .qa_state
        .lock()
        .messages
        .push(crate::types::QaChatMessage {
            role: "assistant".to_string(),
            content: answer.clone(),
        });

    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "answer",
                "messages": messages,
            }),
        );
    }

    // 胶囊直接收掉。QA 不走 insertion，没"已粘贴 N 字"语义；浮窗里答案就是用户的反馈。
    // （之前用 Done 状态会被 capsule UI 错误地渲染上一次 dictation 残留的 message/insertedChars。）
    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);

    // 可选：写一条 history（QA 类型）。当前 DictationSession schema 不能直接表达
    // "QuestionAnswer" 类型，因此简单做法：勾选 qa_save_history 时写一条
    // mode=Raw、error_code=Some("qaSession") 的 placeholder，避免污染 schema 同时
    // 让用户能在历史里翻到这次问答的字面值。详见 issue #118。
    if prefs.qa_save_history {
        let session = DictationSession {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now().to_rfc3339(),
            raw_transcript: question.clone(),
            final_text: answer.clone(),
            mode: PolishMode::Raw,
            style_pack_id: None,
            translation_active: false,
            polish_source: None,
            app_bundle_id: None,
            app_name: front_app.clone(),
            insert_status: InsertStatus::CopiedFallback,
            error_code: Some("qaSession".to_string()),
            duration_ms: Some(raw.duration_ms),
            dictionary_entry_count: None,
            has_audio_recording: None,
        };
        let prefs_snapshot = inner.prefs.get();
        if let Err(e) = inner.history.append_with_retention(
            session,
            prefs_snapshot.history_retention_days,
            prefs_snapshot.history_max_entries,
        ) {
            log::error!("[coord] QA history append failed: {e}");
        }
    }

    inner.qa_state.lock().phase = QaPhase::Idle;
    Ok(())
}

/// 把出错状态送到前端浮窗 + 胶囊错误闪一下 + 复位 phase。
/// 浮窗保持可见（v2：错误后用户可以再按 Option 重试）；messages 一并送过去
/// 让前端继续渲染历史对话。
pub(super) fn finish_qa_with_error(inner: &Arc<Inner>, message: String) {
    stop_qa_recorder(inner);
    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "error",
                "error": message,
                "messages": messages,
            }),
        );
    }
    emit_capsule(inner, CapsuleState::Error, 0.0, 0, Some(message), None);
    schedule_capsule_idle(inner, 1500);
    let mut state = inner.qa_state.lock();
    state.phase = QaPhase::Idle;
    state.cancelled = false;
}

/// 静默收尾：发 idle 事件给前端，phase 复位。**不关浮窗**（v2：浮窗只在用户
/// Esc/X 或再按 QA hotkey 时才关）；多轮对话历史保留。胶囊也即刻收掉。
pub(super) fn finish_qa_idle_silently(inner: &Arc<Inner>) {
    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "idle",
                "messages": messages,
            }),
        );
    }
    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
    let mut state = inner.qa_state.lock();
    state.phase = QaPhase::Idle;
    state.cancelled = false;
    state.selection = None;
}

pub(super) fn cancel_qa_session(inner: &Arc<Inner>) {
    let phase = inner.qa_state.lock().phase;
    if phase == QaPhase::Idle {
        return;
    }
    inner.qa_state.lock().cancelled = true;
    // SSE 流取消旗标——polish::chat_completion_history_streaming 的 loop 每帧检查
    // 这个 flag，true 时立即 break 不再 drain HTTP body，避免取消后 LLM 仍烧 token。
    // 详见 issue #161。
    inner.qa_stream_cancelled.store(true, Ordering::SeqCst);
    stop_qa_recorder(inner);
    if let Some(asr) = inner.qa_asr.lock().take() {
        cancel_active_asr(asr);
    }
    // Processing 阶段保持 phase 让 end_qa_session 自然走完 cancel 检查；
    // 否则直接复位。
    if phase != QaPhase::Processing {
        inner.qa_state.lock().phase = QaPhase::Idle;
    }
    log::info!("[coord] QA session cancelled (was {phase:?})");
}

pub(super) async fn answer_chat_dispatch<F, C>(
    messages: &[crate::types::QaChatMessage],
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    on_delta: F,
    should_cancel: C,
) -> anyhow::Result<String>
where
    F: Fn(&str) + Send + Sync,
    C: Fn() -> bool + Send + Sync,
{
    // 见 polish_text 顶部注释——同样的 Gemini / OpenAI-compatible 路由逻辑，
    // QA 流式回答走 Gemini 原生 :streamGenerateContent?alt=sse。
    let active_llm = CredentialsVault::get_active_llm();
    if active_llm == "gemini" {
        let (api_key, model, base_url) = read_gemini_credentials()?;
        let provider = GeminiProvider::new(
            GeminiConfig::new(api_key, model, base_url).with_thinking_enabled(llm_thinking_enabled),
        );
        return Ok(provider
            .answer_chat_streaming(
                messages,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
                on_delta,
                should_cancel,
            )
            .await?);
    }

    let provider = build_active_llm_provider(llm_thinking_enabled)?;
    Ok(provider
        .answer_chat_streaming(
            messages,
            working_languages,
            chinese_script_preference,
            output_language_preference,
            front_app,
            on_delta,
            should_cancel,
        )
        .await?)
}
