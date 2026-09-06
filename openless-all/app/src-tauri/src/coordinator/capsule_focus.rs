//! Focus-target capture and capsule-window presentation extracted from
//! `coordinator.rs` (behavior-preserving move).
//!
//! External focus/frontmost-app capture and `emit_capsule`. Native window
//! presentation belongs to `tauri_coordinator_host`. References parent items via `use super::*;`; `pub(super)`
//! so the parent and sibling submodules reach them through `use capsule_focus::*;`.

use super::*;

/// 与 capture_focus_target 类似，但前台窗口属于本进程（即用户停在 QA / capsule / main
/// 等自家窗口）时返回 None，让 caller 区分"用户没切到别处" vs "用户切到了另一个真正的
/// 外部 app"。issue #466 多轮场景下用来刷新 qa_focus_target。
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

/// 捕获用户开始 dictation 时的前台 app 标签（"localizedName (bundle.id)"），用作 LLM
/// polish/translate 的上下文前提，让模型按 app 调风格。详见 issue #116。
///
/// macOS 走 NSWorkspace.frontmostApplication（公开 API，无需额外权限）；
/// Windows 复用前台 HWND 拿窗口标题；Linux/其他平台返回 None。
pub(crate) fn capture_frontmost_app() -> Option<String> {
    // 曾经这里有一份和 `selection.rs` 逐字重复的 NSWorkspace/Win32 实现（三个 cfg
    // 分支、连 nsstring 转换 helper 都是复制的）。收口到 selection：那边现在把取值
    // 拆成了结构化的 `current_front_app_parts`，`host_document` 的 bundle 黑名单要用。
    // 一处实现，三个消费方。
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

/// Esc 独占判定：胶囊显示「进行中」（录音/转写/润色）且确为 dictation 会话（phase 非
/// Idle）时为 true——tap/hook 吞掉 Esc 不透传宿主应用。phase 条件专门排除 QA：QA 也走
/// 胶囊，但它的 Esc 由聚焦浮窗处理（#161），全局吞键反而会把它挡掉。纯函数便于表格测试。
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

/// 选区润色复用原有无焦点 capsule 窗口，但用独立标记让前端显示一行轻量状态提示，
/// 不污染语音/QA 的光效和终态文案。
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

/// `capsule_event_lock` 已由调用方持有的内部实现。自动隐藏路径必须能在验证 epoch
/// 后、发出 Idle 前一直持锁，才能保证旧 timer 不会盖掉刚到的新 payload。
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
    emit_capsule_payload_locked(inner, payload)
}

/// Core 反馈完整进入同一个原生显示出口；包括 warming、translation 等字段，
/// 不经过旧的“按 Host 状态重建 payload”路径。
pub(super) fn emit_core_capsule(
    inner: &Arc<Inner>,
    payload: CapsulePayload,
    expected_epoch: Option<u64>,
) -> Option<u64> {
    emit_capsule_at_epoch(
        &inner.capsule_event_lock,
        &inner.capsule_event_epoch,
        expected_epoch,
        || emit_capsule_payload_locked(inner, payload),
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

fn emit_capsule_payload_locked(inner: &Arc<Inner>, payload: CapsulePayload) -> u64 {
    let state = payload.state;
    let selection_polish = payload.selection_polish;
    // 每次 payload 都推进代数。这样一个选区润色终态的旧 timer 在之后出现任何
    // selection / voice / QA 状态时都失效，不会把新的可见状态强行收回 Idle。
    let event_epoch = inner
        .capsule_event_epoch
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    inner
        .selection_polish_capsule_active
        .store(selection_polish, Ordering::SeqCst);
    // 在 app 句柄校验之前记录，便于无 GUI 的测试断言「按下热键 → 弹了哪种胶囊」。
    // replace 顺带取回上一帧 state，用于判断本次是不是「入场帧」（见下方 defer_capsule_emit）。
    let prev_state = inner.last_capsule_state.lock().replace(state);
    // Esc 独占窗口：胶囊显示进行中（录音/转写/润色）且确为 dictation 会话（phase 非
    // Idle）时，tap/hook 吞掉 Esc 不透传宿主应用——此刻 Esc 的语义是「取消这个会话」，
    // 双重派发会顺带触发宿主应用的 Esc（如取消 Claude 正在生成的回复）。phase 条件排除
    // QA：QA 会话也走胶囊，但它的 Esc 由聚焦的浮窗窗口处理，吞键反而会把它挡掉。
    // 终止帧（Done/Cancelled/Error/Idle）自然清除。emit_capsule 是所有会话状态变化的
    // 单一出口（含 #77 审计保证的全部终止路径），在此维护不会漏路径。
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
    // 即使窗口尚未绑定，也保留卡片期间最新的完整反馈，重显时不能倒退到旧准备态。
    defer_capsule_payload_if_fallback_active(inner, &payload);
    let Some(capsule) = inner.host.capsule_window() else {
        return event_epoch;
    };

    #[cfg(target_os = "android")]
    crate::android::notify_capsule_state(&payload);

    // visible / translation 是「这一帧 capsule:state event 的 payload」内容 ——
    // 必须在 call-site（即音频线程触发 emit_capsule 时）就算定，否则 main thread
    // 闭包里读到的将是「下一帧」的 state，跟实际下发给 JS 的 payload 不一致。
    let visible = !matches!(state, CapsuleState::Idle);
    // 入场帧：胶囊从不可见第一次变可见。按平时的「同步 emit + 异步 show」，前端会在窗口
    // 还隐藏时就起播 capsule-in，等窗口真 show 出来动画早已播完 → 用户看到胶囊「凭空出
    // 现」而非「滑入」。修法：入场帧把发给 capsule 窗口的事件推迟到主线程闭包里、
    // window.show 之后再 emit，保证前端起播入场动画时窗口已可见、动画完整可见。Linux 不
    // 走胶囊窗口（文字经 fcitx5 直接 commit），保持原同步 emit 不变。
    let was_visible = matches!(prev_state, Some(s) if !matches!(s, CapsuleState::Idle));
    let defer_capsule_emit = visible && !was_visible && cfg!(not(target_os = "linux"));

    // Linux: 通过 fcitx5 插件在候选词列表下方显示听写状态，不干扰输入法预编辑。
    // 只在文本变化时调用 DBus，避免录音中 ~30Hz 的音频电平回调重复调用。
    #[cfg(target_os = "linux")]
    {
        use std::sync::Mutex;
        static LAST_AUX: Mutex<Option<String>> = Mutex::new(None);

        let aux = match state {
            CapsuleState::Idle => None,
            CapsuleState::Recording => Some("🎤 收音中..."),
            CapsuleState::Transcribing => Some("🔄 识别中..."),
            CapsuleState::Polishing => Some("✨ 润色中..."),
            CapsuleState::Done => Some("✅ 已插入"),
            CapsuleState::Cancelled => Some("— 已取消"),
            CapsuleState::Error => Some("❌ 出错"),
        };

        let mut last = LAST_AUX.lock().unwrap();
        if aux != last.as_deref() {
            *last = aux.map(String::from);
            // 代数计数器：每次状态变化 +1，retry 线程只在自己代数仍为最新时生效。
            // 避免 Recording→Idle→Recording 快速切换时多个 retry 重复触发。
            static RETRY_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            // fetch_add 返回旧值，所以 latest_gen > gen+1 才表示"在我之后又发生了变更"。
            let gen = RETRY_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match aux {
                Some(t) => {
                    log::info!("[capsule] set_aux_down: {t} gen={gen}");
                    let text = t.to_string();
                    std::thread::spawn(move || {
                        let current = LAST_AUX.lock().unwrap().clone();
                        if current.as_deref() != Some(&text) {
                            log::info!(
                                "[capsule] set_aux_down skipped: state changed to {current:?}"
                            );
                            return;
                        }
                        if let Err(e) = crate::linux_fcitx::set_aux_down(&text) {
                            log::warn!("[capsule] set_aux_down failed: {e}");
                        }
                    });
                    // 终态（Done/Cancelled/Error）3 秒后自动清除，避免一直跟随焦点。
                    if matches!(
                        state,
                        CapsuleState::Done | CapsuleState::Cancelled | CapsuleState::Error
                    ) {
                        let text = t.to_string();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            let latest_gen = RETRY_GEN.load(std::sync::atomic::Ordering::SeqCst);
                            if latest_gen > gen + 1 {
                                return;
                            }
                            let current = LAST_AUX.lock().unwrap().clone();
                            if current.as_deref() != Some(&text) {
                                return;
                            }
                            log::info!("[capsule] auto-clear terminal state: {text}");
                            let _ = crate::linux_fcitx::set_aux_down("");
                            *LAST_AUX.lock().unwrap() = None;
                        });
                    }
                }
                None => {
                    log::info!("[capsule] clear_aux_down gen={gen}");
                    std::thread::spawn(move || {
                        let latest_gen = RETRY_GEN.load(std::sync::atomic::Ordering::SeqCst);
                        if latest_gen > gen + 1 {
                            log::info!(
                                "[capsule] clear_aux_down skipped: gen {gen}, latest {latest_gen}"
                            );
                            return;
                        }
                        let current = LAST_AUX.lock().unwrap().clone();
                        if current.is_some() {
                            log::info!(
                                "[capsule] clear_aux_down skipped: state changed to {current:?}"
                            );
                            return;
                        }
                        if let Err(e) = crate::linux_fcitx::clear_aux_down() {
                            log::warn!("[capsule] clear_aux_down failed: {e}");
                        }
                    });
                }
            }
        }
    }

    // emit_capsule 会被 cpal process_callback（音频回调线程）调用 ~30 Hz —— 在该
    // 线程上调用 NSWindow / HWND API 会撞 macOS dispatch_assert_queue_fail SIGTRAP
    // 或者 Win32 SendMessage 死锁。把 window.show/hide + 位置调整 marshal 到主线程；
    // app.emit_to 走 Tauri 内部事件总线，本身线程安全，保留同步调用。详见 audit 3.2.2。
    //
    // show_capsule（用户偏好）在主线程执行时再读 —— 用户可以在录音过程中改设置，
    // 闭包入队到真正跑之间窗口上限是一两帧（~16-33ms），用最新值消除 stale-pref
    // 闪烁。pr_agent 关注点 — 见 audit follow-up。
    let host_for_main = inner.host.clone();
    let backend_for_main = Arc::clone(&inner.backend);
    // 入场帧要在 window.show 之后、闭包内部把 state 回发给前端，需要 payload 的独立副本
    // move 进闭包；非入场帧走闭包外的即时同步 emit（下方），这里就是 None。
    // 注意：入场帧的 payload 在闭包同步 capsule_style 原子之前克隆，最多带一帧旧样式
    //（设置里刚切换后的首次录音，第 2 帧 ~33ms 即纠正）。这是刻意取舍——不要在音频
    // 线程改回直接读 prefs。前端第 1 帧处于 capsule-in 动画期间（380ms），无感知。
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
        let classic_style = matches!(preferences.capsule_style, CapsuleStyle::Classic);
        capsule.apply_capsule_payload(
            &payload_for_window,
            show_capsule,
            classic_style,
            payload_for_deferred_emit.is_some(),
        );
        // 入场帧：窗口刚 show（或本次用户关了胶囊显示走了 hide 分支），此刻再把 state 发给
        // capsule 前端 —— 前端起播 capsule-in 时窗口已可见，入场动画从头完整播放。
        if let Some(payload) = payload_for_deferred_emit.as_ref() {
            host_for_main.emit_capsule_state_to_capsule(payload);
        }
    });

    // 非入场帧（含 Linux、录音中的 level 更新、离场/终态）保持即时同步 emit，最低延迟；
    // 入场帧已在上面的主线程闭包里、window.show 之后 emit 过，这里跳过避免重复下发。
    if !defer_capsule_emit {
        inner.host.emit_capsule_state_to_capsule(&payload);
    }
    // 主窗口也需要 capsule:state 事件：AudioCueListener 用它触发录音提示音。
    // Linux 上胶囊隐藏时提示音仍应工作，所以同时发给 main 窗口。始终即时，与胶囊窗口
    // 显示时机解耦。
    inner.host.emit_capsule_state_to_main(&payload);
    event_epoch
}

/// 返回一个选区润色终态 timer 是否仍有资格收起 capsule。
///
/// 该判断同时覆盖两类竞态：同一功能的新一轮触发，以及随后开始的语音/QA 会话。
pub(super) fn selection_polish_capsule_epoch_is_current(
    inner: &Arc<Inner>,
    expected_epoch: u64,
) -> bool {
    inner.selection_polish_capsule_active.load(Ordering::SeqCst)
        && inner.capsule_event_epoch.load(Ordering::SeqCst) == expected_epoch
}

/// 旧 dictation/QA timer 的收起路径。它与所有 emit 共享一把短锁：如果 Selection
/// Polish 已经显示，就让路；如果新语音/QA 先一步发了状态，也会在锁序上排在 Idle 前。
pub(super) fn hide_capsule_if_all_sessions_idle(inner: &Arc<Inner>) {
    // 先读 session state，再进 capsule lock。event epoch 负责在两次读取之间
    // 有任何新 payload 时取消本次 Idle。
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

/// 只在同一代 Selection Polish 终态仍是最新可见 capsule 时收起它。锁会让“检查 +
/// 发送 Idle”成为一个不可插队的顺序点，因此旧 timer 不可能在新会话之后覆盖 UI。
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
        // 进行中胶囊 + dictation phase 非 Idle → 独占 Esc（不透传宿主应用）。
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

        // 终止帧（Done/Cancelled/Error/Idle）→ 清除独占。
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

        // QA 场景：胶囊显示进行中但 dictation phase=Idle → 不独占（Esc 归浮窗，#161）。
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
