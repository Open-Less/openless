use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::coordinator_state::request_stop_during_starting_state;
use crate::correction::apply_correction_rules;
use crate::types::HotkeyMode;

use super::qa::handle_qa_option_edge;
use super::resources::*;
use super::*;

/// 同一个 hotkey 边沿之间的最小间隔。低于此阈值的连按整体作为误触丢弃 ——
/// 避免微动开关回弹 / 用户手抖双击造成的空转写报错和 ASR session 抢资源。
const HOTKEY_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(250);
const STREAMING_INSERT_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(12);

/// Less Computer 浮窗的 Tauri 事件名（前端 LessComputerPanel 订阅）。
const LESS_COMPUTER_EVENT: &str = "less-computer:event";

/// Less Computer 内联审批：等待用户决断的 token → oneshot sender 注册表。
///
/// 无头 `claude -p` 没有 mid-run 的 `--permission-prompt-tool` 通道（v2.1.165 不支持），
/// 所以护栏拦截发生在「整轮跑完、护栏 deny 生效」之后。这个注册表是审批 UI 的实回路：
/// 后端发 `approval` 事件后把一个 oneshot 接收端挂在这里，等前端 `less_computer_approve`
/// 命令按 token 解析出用户决断（true=Approve / false=Deny）。
static LESS_COMPUTER_APPROVALS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>,
> = std::sync::OnceLock::new();

fn less_computer_approvals(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>
{
    LESS_COMPUTER_APPROVALS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 前端 `less_computer_approve` 命令调到这里：按 token 解析等待中的审批。
/// token 不存在（已超时 / 已解析）时静默忽略。
pub(super) fn resolve_less_computer_approval(token: &str, approved: bool) {
    let sender = less_computer_approvals()
        .lock()
        .ok()
        .and_then(|mut m| m.remove(token));
    if let Some(tx) = sender {
        let _ = tx.send(approved);
        log::info!("[less-computer] 审批 token={token} approved={approved}");
    } else {
        log::info!("[less-computer] 审批 token={token} 已失效（超时/重复）");
    }
}

/// 往 Less Computer 浮窗发一条事件（macOS only；前端按 `kind` 渲染聊天结构）。
fn emit_less_computer(inner: &Arc<Inner>, payload: serde_json::Value) {
    if let Some(app) = inner.app.lock().clone() {
        let _ = app.emit_to("less-computer", LESS_COMPUTER_EVENT, payload);
    }
}

/// 跑流式润色路径（opt-in，跨平台）。
///
/// 平台差异：
/// - **macOS**：`switch_to_ascii` 切到 ABC 输入源（规避 CJK / 日文 IME 拦截 Unicode 事件），
///   session 结束 `restore_input_source` 切回。`type_unicode_chunk` 走 CGEvent FFI。
/// - **Windows**：`switch_to_ascii` 是 no-op（SendInput Unicode 绕过 TSF）；
///   `type_unicode_chunk` 走 `SendInput(KEYEVENTF_UNICODE)`。
/// - **Linux（实验）**：`switch_to_ascii` 是 no-op；`type_unicode_chunk` 走 enigo
///   `Keyboard::text`。X11 / XTest 稳定。
///
/// 通用流程：
/// 1. `switch_to_ascii`（macOS）/ no-op（其他）；失败则降级回一次性 `polish_or_passthrough`。
/// 2. 起一个 `spawn_blocking` 后台任务，从 mpsc 收 SSE delta，按 12ms flush window
///    合并后调 `type_unicode_chunk` 模拟键盘事件落到光标处。串行有序，无竞态。
/// 3. 调 `polish_or_passthrough_streaming`，`on_delta` 把 chunk 塞进 mpsc。
/// 4. 流结束 / 失败 / 取消 → drop mpsc 发送端 → typer 任务 drain 完剩余 delta 退出 →
///    `restore_input_source` 恢复用户原输入源（macOS 才有意义，其他平台 no-op）。
/// 5. 返回 `(polished, polish_error, already_streamed)`：
///    - 成功：`(text, None, true)` — 字符已经在屏幕上，调用方应当跳过 `inserter.insert`
///    - 失败：`(raw_text, Some(reason), false)` — 流式过程出错，调用方走 raw 一次性兜底
///    - 不支持：`run_streaming_polish` 内部直接调 `polish_or_passthrough` 透明降级
///
/// **不在流式路径里做**：`apply_chinese_script_preference` / `apply_correction_rules`
/// 这两步在 v1 跳过 —— 字符已经一边流一边落出去了，不好回退。需要的话只能关 toggle 走
/// 一次性路径。
#[allow(clippy::too_many_arguments)]
async fn run_streaming_polish(
    inner: &Arc<Inner>,
    raw: &RawTranscript,
    mode: PolishMode,
    hotwords: &[String],
    style_system_prompt: &str,
    working_languages: &[String],
    chinese_script_preference: crate::types::ChineseScriptPreference,
    output_language_preference: crate::types::OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    prior_turns: &[(String, String)],
) -> (String, Option<String>, bool) {
    log::info!(
        "[coord] streaming_insert path ENTER (raw_chars={})",
        raw.text.chars().count()
    );

    let app = inner.app.lock().clone();
    let Some(app) = app else {
        log::warn!("[coord] streaming_insert: no AppHandle in Inner; fall back to one-shot");
        let (p, e) = polish_or_passthrough(
            raw,
            mode,
            hotwords,
            style_system_prompt,
            working_languages,
            chinese_script_preference,
            output_language_preference,
            llm_thinking_enabled,
            front_app,
            prior_turns,
        )
        .await;
        return (p, e, false);
    };

    // 1. 切到 ABC 输入源。失败则降级 —— 流式路径上 CJK IME 拦截不是可恢复错误。
    log::info!("[coord] streaming_insert: switching input source to ABC");
    let prev_ime = match crate::unicode_keystroke::switch_to_ascii(&app).await {
        Ok(prev) => {
            log::info!(
                "[coord] streaming_insert: switched to ABC (had_previous={})",
                prev.is_some()
            );
            prev
        }
        Err(e) => {
            log::warn!(
                "[coord] streaming_insert: switch_to_ascii failed: {e}; fall back to one-shot"
            );
            let (p, err) = polish_or_passthrough(
                raw,
                mode,
                hotwords,
                style_system_prompt,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                llm_thinking_enabled,
                front_app,
                prior_turns,
            )
            .await;
            return (p, err, false);
        }
    };

    // 2. 起 typer 后台任务：从 mpsc 收 delta，串行调 type_unicode_chunk。
    // 同时累积 typed_text：屏幕上真正落字的内容，用于（a）SSE 中途失败时让 history
    // 与用户实际看到的内容一致；（b）pr-agent #412 反馈 \"saved output diverges
    // from what the user actually sees\"。
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let typer_handle = tokio::task::spawn_blocking(move || {
        drain_streaming_insert_deltas(rx, STREAMING_INSERT_FLUSH_INTERVAL)
    });

    // 3. 调流式润色，on_delta 塞 mpsc；should_cancel 检查 dictation 取消旗。
    let inner_for_cancel = Arc::clone(inner);
    let should_cancel = move || inner_for_cancel.state.lock().cancelled;
    let outcome = super::polish_or_passthrough_streaming(
        raw,
        mode,
        hotwords,
        style_system_prompt,
        working_languages,
        chinese_script_preference,
        output_language_preference,
        llm_thinking_enabled,
        front_app,
        prior_turns,
        move |delta: &str| {
            let _ = tx.send(delta.to_string());
        },
        should_cancel,
    )
    .await;
    // tx 已经被 move 进 on_delta 闭包；闭包随 polish_or_passthrough_streaming 返回
    // 而 drop，typer 那侧 blocking_recv 拿到 None 自然退出。

    // 4. 等 typer 把缓冲 drain 完，拿到实际落字的全文 + 第一条失败原因。
    let (typed_text, typer_failure) = typer_handle.await.unwrap_or_else(|e| {
        log::error!("[coord] streaming_insert: typer task join failed: {e}");
        (String::new(), Some(format!("typer join: {e}")))
    });
    let typed_chars = typed_text.chars().count();
    log::info!("[coord] streaming_insert: typer drained, typed {typed_chars} chars");

    // 5. 无论流是否成功，都恢复用户原输入源。
    log::info!("[coord] streaming_insert: restoring input source");
    if let Err(e) = crate::unicode_keystroke::restore_input_source(&app, prev_ime).await {
        log::warn!("[coord] streaming_insert: restore_input_source failed: {e}");
    } else {
        log::info!("[coord] streaming_insert: input source restored");
    }

    // 6. 把 outcome 翻译成 (polished, polish_error, already_streamed)。
    match outcome {
        super::StreamingPolishOutcome::Streamed(text) => {
            log::info!(
                "[coord] streaming_insert SUCCESS: polished_chars={} typed_chars={} typer_err={:?}",
                text.chars().count(),
                typed_chars,
                typer_failure
            );
            // 边界 case：polish 成功但 typer 在第一字就失败（最常见：session 开始时
            // 已处于 Secure Input；或 SendInput / enigo 拒绝）。屏幕上一字未见，
            // already_streamed=true 会让上层跳过 inserter，最终用户看不到任何内容。
            // 这里显式回退到一次性兜底，让正常 inserter 路径写出 polish 结果。
            // pr-agent #412 反馈 \"Missing fallback\"。
            if typed_chars == 0 {
                if let Some(reason) = typer_failure {
                    log::warn!(
                        "[coord] streaming_insert: zero chars typed despite polish success ({reason}); falling back to one-shot inserter"
                    );
                    return (text, Some(reason), false);
                }
            }
            // 先确定 final_text —— typer 中途失败时屏幕只有 typed_text 这一段，
            // history 记完整 polish 反而会让用户复盘困惑。让 history / clipboard /
            // 后续逻辑统统用 final_text，三处保持一致。
            // pr-agent #412 反馈 \"Clipboard Mismatch\"：之前先写 text 到剪贴板再
            // 决定 typer 是否中途失败，导致 Cmd+V 粘出用户屏幕上没见过的内容。
            let (final_text, polish_err) = match typer_failure {
                Some(e) => (typed_text, Some(format!("typing partially failed: {e}"))),
                None => (text, None),
            };
            // 把 final_text 写回剪贴板（默认 on，macOS/Windows 适用）。
            // Linux：fcitx5 插件已直写文字到目标 app，跳过剪贴板避免破坏用户数据。
            // Android/iOS：无 arboard 剪贴板路径，v1 依赖 IME commit。
            #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "ios")))]
            if inner.prefs.get().streaming_insert_save_clipboard {
                match arboard::Clipboard::new() {
                    Ok(mut cb) => match cb.set_text(final_text.clone()) {
                        Ok(()) => log::info!(
                            "[coord] streaming_insert: final text written to clipboard ({} chars)",
                            final_text.chars().count()
                        ),
                        Err(e) => {
                            log::warn!("[coord] streaming_insert: clipboard set_text failed: {e}")
                        }
                    },
                    Err(e) => {
                        log::warn!("[coord] streaming_insert: clipboard handle init failed: {e}")
                    }
                }
            } else {
                log::info!("[coord] streaming_insert: clipboard save skipped (pref off)");
            }
            (final_text, polish_err, true)
        }
        super::StreamingPolishOutcome::UnsupportedFallback => {
            log::info!(
                "[coord] streaming_insert: dispatch reported unsupported, fall back to one-shot"
            );
            let (p, e) = polish_or_passthrough(
                raw,
                mode,
                hotwords,
                style_system_prompt,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                llm_thinking_enabled,
                front_app,
                prior_turns,
            )
            .await;
            (p, e, false)
        }
        super::StreamingPolishOutcome::Failed(reason) => {
            log::warn!(
                "[coord] streaming_insert FAILED: {reason}; typed {typed_chars} chars before failure"
            );
            // 流式失败但已经流了一部分 chars：用户屏幕上有半截 polish。history 应当
            // 跟屏幕一致 —— 记 typed_text 而不是 raw.text，否则保存内容跟用户看见的
            // 内容会分叉（pr-agent #412 \"Wrong final text\" 反馈）。
            // 一字都没流时 typed_text 是空串，回到 raw 一次性兜底。
            if typed_chars > 0 {
                (
                    typed_text,
                    Some(format!(
                        "streaming polish failed mid-stream after {typed_chars} chars: {reason}"
                    )),
                    true,
                )
            } else {
                (raw.text.clone(), Some(reason), false)
            }
        }
    }
}

fn drain_streaming_insert_deltas(
    rx: std::sync::mpsc::Receiver<String>,
    flush_interval: std::time::Duration,
) -> (String, Option<String>) {
    drain_streaming_insert_deltas_with(rx, flush_interval, flush_streaming_insert_buffer)
}

fn drain_streaming_insert_deltas_with<F>(
    rx: std::sync::mpsc::Receiver<String>,
    flush_interval: std::time::Duration,
    mut flush_pending: F,
) -> (String, Option<String>)
where
    F: FnMut(&mut String, &mut String) -> Option<String>,
{
    let mut typed_text = String::new();
    let mut first_failure: Option<String> = None;
    let mut pending = String::new();
    while let Ok(delta) = rx.recv() {
        pending.push_str(&delta);
        let flush_at = std::time::Instant::now() + flush_interval;
        loop {
            let now = std::time::Instant::now();
            if now >= flush_at {
                break;
            }
            match rx.recv_timeout(flush_at.duration_since(now)) {
                Ok(delta) => pending.push_str(&delta),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    first_failure = flush_pending(&mut pending, &mut typed_text);
                    return (typed_text, first_failure);
                }
            }
        }
        first_failure = flush_pending(&mut pending, &mut typed_text);
        if first_failure.is_some() {
            // 一旦类型链路出错（如 Secure Input 启用），后续 delta 全部丢弃，但仍
            // 把 mpsc drain 完，避免发送端阻塞。
            while rx.recv().is_ok() {}
            break;
        }
    }
    if first_failure.is_none() {
        first_failure = flush_pending(&mut pending, &mut typed_text);
    }
    (typed_text, first_failure)
}

fn flush_streaming_insert_buffer(pending: &mut String, typed_text: &mut String) -> Option<String> {
    flush_streaming_insert_buffer_with(
        pending,
        typed_text,
        crate::unicode_keystroke::type_unicode_chunk,
    )
}

fn flush_streaming_insert_buffer_with<F>(
    pending: &mut String,
    typed_text: &mut String,
    mut type_chunk: F,
) -> Option<String>
where
    F: FnMut(&str) -> Result<usize, crate::unicode_keystroke::TypeError>,
{
    if pending.is_empty() {
        return None;
    }
    let delta = std::mem::take(pending);
    let delta_chars = delta.chars().count();
    match type_chunk(&delta) {
        Ok(typed_chars) => {
            let appended = append_typed_prefix(typed_text, &delta, typed_chars);
            if appended < delta_chars {
                let reason = format!(
                    "type_unicode_chunk typed only {appended}/{delta_chars} chars without error"
                );
                log::error!(
                    "[coord] streaming_insert: {reason} at typed={} chars; \
                     dropping remaining deltas",
                    typed_text.chars().count()
                );
                Some(reason)
            } else {
                None
            }
        }
        Err(e) => {
            append_typed_prefix(typed_text, &delta, e.typed_chars());
            log::error!(
                "[coord] streaming_insert: type_unicode_chunk failed at typed={} chars: {e}; \
                 dropping remaining deltas",
                typed_text.chars().count()
            );
            Some(e.to_string())
        }
    }
}

pub(super) fn finalize_polished_text(
    polished: String,
    translation_active: bool,
    _raw_uses_llm: bool,
    mode: PolishMode,
    polish_error: &Option<String>,
    chinese_script_preference: crate::types::ChineseScriptPreference,
    correction_rules: &[crate::types::CorrectionRule],
    already_streamed: bool,
) -> String {
    if already_streamed {
        return polished;
    }
    let should_force_script = if translation_active {
        polish_error.is_some()
    } else {
        mode == PolishMode::Raw || polish_error.is_some()
    };
    let polished = if should_force_script {
        apply_chinese_script_preference(&polished, chinese_script_preference)
    } else {
        polished
    };
    if correction_rules.is_empty() {
        polished
    } else {
        let corrected = apply_correction_rules(&polished, correction_rules);
        if corrected != polished {
            log::info!(
                "[coord] correction rules adjusted final text ({} → {} chars)",
                polished.chars().count(),
                corrected.chars().count()
            );
        }
        corrected
    }
}

fn streaming_insert_eligible(
    streaming_insert_enabled: bool,
    translation_active: bool,
    mode: PolishMode,
    raw_uses_llm: bool,
) -> bool {
    streaming_insert_enabled && !translation_active && (mode != PolishMode::Raw || raw_uses_llm)
}

fn default_done_message(status: InsertStatus, polish_failed: bool) -> Option<String> {
    if polish_failed {
        // polish 失败优先告知用户，即使 insert 成功也要让用户知道这版是原文
        Some("润色失败，已插入原文".to_string())
    } else {
        match status {
            InsertStatus::Inserted => None,
            InsertStatus::PasteSent => Some("已尝试粘贴".to_string()),
            InsertStatus::CopiedFallback => Some(if cfg!(target_os = "windows") {
                "已复制，请 Ctrl+V".to_string()
            } else {
                "已复制，请粘贴".to_string()
            }),
            InsertStatus::Failed => Some("插入失败".to_string()),
        }
    }
}

pub(super) async fn handle_pressed_edge(inner: &Arc<Inner>) {
    let was_held = inner.hotkey_trigger_held.swap(true, Ordering::SeqCst);
    if !was_held {
        // 防抖：相邻 < HOTKEY_DEBOUNCE 的边沿直接丢弃，记到 log 方便排查。
        // 与 `hotkey_trigger_held` 互补：held 防 press-without-release，本检查防
        // press-release-press 三连过快。每个有效边沿都会更新时间戳。
        let now = std::time::Instant::now();
        let too_soon = {
            let mut last = inner.last_hotkey_dispatch_at.lock();
            let drop = matches!(*last, Some(t) if now.duration_since(t) < HOTKEY_DEBOUNCE);
            if !drop {
                *last = Some(now);
            }
            drop
        };
        if too_soon {
            log::info!(
                "[coord] hotkey pressed edge debounced (< {} ms since last dispatch)",
                HOTKEY_DEBOUNCE.as_millis()
            );
            return;
        }

        // 路由：QA 浮窗可见时，rightOption 边沿走 QA；否则走主听写。详见 issue #118 v2。
        // 例外：dictation session 已经在跑（Starting / Listening / Processing / Inserting），
        // 即使 QA 浮窗被打开了，这条边沿也必须先走 dictation。否则 begin_qa_session 会
        // 第二次抢同一个麦克风 device —— 在 Linux/PipeWire 上甚至会成功打开两路捕获，
        // dictation 的 recorder 没人停；在 macOS/Windows 上 cpal 会拒绝第二次 build_input_stream
        // 但 dictation session 仍在跑、用户找不到从 QA 面板停掉它的入口。审计 3.3.1。
        let dictation_active = !matches!(inner.state.lock().phase, SessionPhase::Idle);
        let panel_visible = inner.qa_state.lock().panel_visible;
        if panel_visible && !dictation_active {
            handle_qa_option_edge(inner).await;
        } else {
            handle_pressed(inner).await;
        }
    }
}

pub(super) async fn handle_pressed(inner: &Arc<Inner>) {
    let mode = inner.prefs.get().hotkey.mode;
    let phase = inner.state.lock().phase;
    log::info!("[coord] hotkey pressed (mode={mode:?}, phase={phase:?})");
    match (mode, phase) {
        (HotkeyMode::Toggle, SessionPhase::Idle) => {
            // 冷却检查：end_session 刚收尾时禁止短时间内再次激活，
            // 避免三连按第 3 次误触（此时胶囊仍在离场动画周期内，issue #545）。
            let now = std::time::Instant::now();
            let on_cooldown = inner
                .session_cooldown_until
                .lock()
                .map(|deadline| now < deadline)
                .unwrap_or(false);
            if on_cooldown {
                log::info!(
                    "[coord] toggle activation blocked by cooldown (session still winding down)"
                );
                return;
            }
            let _ = begin_session(inner).await;
        }
        (HotkeyMode::Toggle, SessionPhase::Listening) => {
            let _ = end_session(inner).await;
        }
        (HotkeyMode::Hold, SessionPhase::Idle) => {
            let _ = begin_session(inner).await;
        }
        // Toggle 模式 Starting 阶段第二次按 → 用户想停。
        // 不能直接 end_session（ASR session 还没建好），存边沿，握手完成后立即触发。
        (HotkeyMode::Toggle, SessionPhase::Starting) => {
            request_stop_during_starting(inner, "toggle stop edge");
        }
        _ => {}
    }
}

pub(super) async fn handle_released_edge(inner: &Arc<Inner>) {
    let was_held = inner.hotkey_trigger_held.swap(false, Ordering::SeqCst);
    if was_held {
        // QA 浮窗可见时，Option 行为是 press-toggle（不分 hold/release），release 边沿忽略。
        // 与 handle_pressed_edge 的路由对称：dictation session 在跑时 Pressed 已经被路由到
        // dictation，那 Released 必须也路由到 dictation —— 否则 Hold 模式松开热键时
        // end_session 不会触发，dictation 永远停不下来。审计 3.3.1。
        let dictation_active = !matches!(inner.state.lock().phase, SessionPhase::Idle);
        let panel_visible = inner.qa_state.lock().panel_visible;
        if panel_visible && !dictation_active {
            return;
        }
        handle_released(inner).await;
    }
}

pub(super) async fn handle_released(inner: &Arc<Inner>) {
    let mode = inner.prefs.get().hotkey.mode;
    let phase = inner.state.lock().phase;
    log::info!("[coord] hotkey released (mode={mode:?}, phase={phase:?})");
    if mode == HotkeyMode::Toggle {
        // Toggle 听写松手不做事（点一下停）。Less Computer 走独立专用键监听器。
        return;
    }
    if mode == HotkeyMode::Hold {
        match phase {
            SessionPhase::Listening => {
                let _ = end_session(inner).await;
            }
            // Hold 模式 Starting 阶段松开 → 用户想停。同上：握手完成后再 end。
            SessionPhase::Starting => {
                request_stop_during_starting(inner, "hold release edge");
            }
            _ => {}
        }
    }
}

/// Less Computer 收尾：把转写当作指令交给无头 Claude，结果以胶囊展示（不插入到光标）。
async fn run_voice_agent_transcript(
    inner: &Arc<Inner>,
    _session_id: SessionId,
    transcript: String,
    elapsed: u64,
) -> Result<(), String> {
    log::info!(
        "[coord] Cloud Agent 语音：指令 {} 字",
        transcript.chars().count()
    );
    // 胶囊保留「处理中」反馈（用户熟悉的小录音条状态机）；聊天浮窗承载完整对话。
    emit_capsule(
        inner,
        CapsuleState::Polishing,
        0.0,
        elapsed,
        Some("Claude 处理中…".to_string()),
        None,
    );

    // 聊天浮窗：显示窗口 + 落用户气泡（语音指令转写）。macOS only（helper 内部 gating）。
    if let Some(app) = inner.app.lock().clone() {
        crate::show_less_computer_window(&app);
        // 全屏彩虹描边已在按下键时（handle_less_computer_pressed）点亮，这里不重复。
    }
    // 连续对话：浮窗里已有进行中的会话 → 本轮 `claude --continue` 续上下文；否则是新会话（fresh）。
    // dismiss 关窗会把标志复位为 false。
    let continue_session = inner
        .less_computer_conversation
        .swap(true, Ordering::SeqCst);
    emit_less_computer(
        inner,
        serde_json::json!({ "kind": "user", "text": transcript, "fresh": !continue_session }),
    );

    let prefs = inner.prefs.get();
    // 工作目录：用户设的 workdir，否则 $HOME。--add-dir 把文件作用域限定在此。
    let cwd = prefs
        .coding_agent_workdir
        .clone()
        .filter(|d| !d.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(std::path::PathBuf::from));
    // 运行前 git 快照（cwd 是 git 仓库才有效；非仓库无副作用），便于回滚文件改动。
    if let Some(dir) = &cwd {
        if let Some(sha) = crate::coding_agent::create_git_snapshot(dir) {
            log::info!("[less-computer] 运行前 git 快照 {sha}（git stash apply 可回滚）");
        }
    }

    // 钳制：语音 → shell 这条全自动路径禁止 bypassPermissions 绕过护栏（无人审、动手即生效）。
    // 即便用户在偏好里设了 bypass，这里也降级为 acceptEdits（仍带 deny 护栏）。
    let mode = match coding_agent_mode_from_pref(&prefs.coding_agent_permission_mode) {
        crate::coding_agent::CodingAgentPermissionMode::BypassPermissions => {
            log::warn!(
                "[less-computer] 语音 Agent 路径禁止 bypassPermissions，已降级为 acceptEdits（保留护栏）"
            );
            crate::coding_agent::CodingAgentPermissionMode::AcceptEdits
        }
        other => other,
    };
    let model = prefs
        .coding_agent_model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| Some("sonnet".to_string()));
    let prompt = crate::coding_agent::autonomous_prompt(&transcript);

    // 第一轮：默认护栏（高风险全 deny）。运行后若检测到护栏拦截，弹审批卡；
    // 用户 Approve 则在第二轮把该高风险模式从 deny 移除 + 加进 allowed，重跑一次。
    let outcome = run_less_computer_once(
        inner,
        &prompt,
        cwd.as_deref(),
        mode,
        model.as_deref(),
        &[],
        continue_session,
    )
    .await;

    let final_outcome = match maybe_request_approval(inner, &outcome).await {
        Some(approved_pattern) => {
            log::info!("[less-computer] 审批通过，放行高风险模式后重跑：{approved_pattern}");
            run_less_computer_once(
                inner,
                &prompt,
                cwd.as_deref(),
                mode,
                model.as_deref(),
                &[approved_pattern],
                continue_session,
            )
            .await
        }
        None => outcome,
    };

    inner.state.lock().phase = SessionPhase::Idle;
    // 工作结束：熄灭全屏彩虹描边（聊天浮窗保留，等用户读完/关闭）。
    if let Some(app) = inner.app.lock().clone() {
        crate::hide_less_computer_glow(&app);
    }

    match final_outcome {
        LessComputerOutcome::Done { text, cost_usd } => {
            let text = text.trim().to_string();
            if text.is_empty() {
                let msg = "Claude 无结果（确认已登录 claude 且额度充足）".to_string();
                emit_less_computer(
                    inner,
                    serde_json::json!({ "kind": "error", "message": msg }),
                );
                emit_capsule(inner, CapsuleState::Error, 0.0, elapsed, Some(msg), None);
                schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                return Err("voice agent empty".to_string());
            }
            log::info!("[coord] Cloud Agent 语音：返回 {} 字", text.chars().count());
            emit_less_computer(
                inner,
                serde_json::json!({ "kind": "completed", "text": text, "costUsd": cost_usd }),
            );
            emit_capsule(inner, CapsuleState::Done, 0.0, elapsed, Some(text), None);
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            Ok(())
        }
        LessComputerOutcome::Failed { message } => {
            log::warn!("[coord] Cloud Agent 语音失败: {message}");
            emit_less_computer(
                inner,
                serde_json::json!({ "kind": "error", "message": message }),
            );
            emit_capsule(
                inner,
                CapsuleState::Error,
                0.0,
                elapsed,
                Some(message),
                None,
            );
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            Err("voice agent failed".to_string())
        }
        LessComputerOutcome::Cancelled => {
            log::info!("[coord] Cloud Agent 语音已取消");
            emit_less_computer(inner, serde_json::json!({ "kind": "cancelled" }));
            emit_capsule(inner, CapsuleState::Cancelled, 0.0, elapsed, None, None);
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            Err("voice agent cancelled".to_string())
        }
    }
}

/// 一轮无头 Less Computer 运行的结果。
enum LessComputerOutcome {
    Done { text: String, cost_usd: Option<f64> },
    Failed { message: String },
    Cancelled,
}

/// 跑一轮无头 Claude（「放行 + 护栏」），把 Delta/ToolUse 实时 stream 到聊天浮窗，
/// 终局收敛为 [`LessComputerOutcome`]。`extra_allow_patterns` 为审批通过后放行的
/// 高风险子串（如 "git push --force"）：从 deny 清单剔除 + 作为 `Bash(<pat>:*)` 加进 allowed。
async fn run_less_computer_once(
    inner: &Arc<Inner>,
    prompt: &str,
    cwd: Option<&std::path::Path>,
    mode: crate::coding_agent::CodingAgentPermissionMode,
    model: Option<&str>,
    extra_allow_patterns: &[String],
    continue_session: bool,
) -> LessComputerOutcome {
    // 护栏 deny：默认全量；审批放行的模式从 deny 中剔除。
    // 审批 UI 只回传命中的单个高风险子串，但同一风险有等价写法（如 --force / -f）。
    // 按「风险等价组」整组放行：只放行被点那一个会让等价写法仍卡在 deny（deny 优先级高于
    // allow）→ 命令仍被拦。见 guard::risk_equivalent_patterns。
    let mut deny = crate::coding_agent::guard::default_deny_rules();
    let approved_patterns: Vec<String> = extra_allow_patterns
        .iter()
        .flat_map(|p| {
            let group = crate::coding_agent::guard::risk_equivalent_patterns(p);
            if group.is_empty() {
                vec![p.clone()]
            } else {
                group.into_iter().map(|s| s.to_string()).collect()
            }
        })
        .collect();
    let allow_rules: Vec<String> = approved_patterns
        .iter()
        .map(|p| format!("Bash({p}:*)"))
        .collect();
    if !allow_rules.is_empty() {
        deny.retain(|d| !allow_rules.iter().any(|a| a == d));
    }
    let settings_json = serde_json::json!({
        "permissions": { "defaultMode": mode.as_cli_arg(), "deny": deny }
    });
    let settings_path = std::env::temp_dir().join(format!(
        "openless-less-computer-guard-{}.json",
        uuid::Uuid::new_v4()
    ));
    // fail-closed：序列化或写入失败时立即中止，绝不在「无护栏」下把无效路径交给
    // `claude -p --settings`（找不到文件 = 完全裸跑）。宁可不跑也不裸跑。
    let settings_bytes = match serde_json::to_vec_pretty(&settings_json) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[less-computer] 序列化护栏配置失败: {e}");
            return LessComputerOutcome::Failed {
                message: "护栏配置写入失败，已中止（拒绝在无护栏下执行）".into(),
            };
        }
    };
    if let Err(e) = std::fs::write(&settings_path, settings_bytes) {
        log::warn!("[less-computer] 写护栏配置失败: {e}");
        return LessComputerOutcome::Failed {
            message: "护栏配置写入失败，已中止（拒绝在无护栏下执行）".into(),
        };
    }

    let mut req = crate::coding_agent::CodingAgentRequest::new("less-computer", prompt.to_string());
    req.cwd = cwd.map(|p| p.to_path_buf());
    req.model = model.map(|m| m.to_string());
    req.permission_mode = mode;
    // 写护栏成功后才设置：写失败已在上面 fail-closed 返回，不会带无效路径裸跑。
    req.settings_json_path = Some(settings_path.clone());
    // 去掉 WebFetch：无出站白名单时它是 prompt 注入 SSRF 面（诱导拉取内网/元数据端点）。
    // 保留 WebSearch（走搜索引擎，不直接抓任意 URL）。
    req.allowed_tools = vec![
        "Bash".into(),
        "Read".into(),
        "Edit".into(),
        "Write".into(),
        "Glob".into(),
        "Grep".into(),
        "WebSearch".into(),
    ];
    req.allowed_tools.extend(allow_rules);
    // 真实任务（开应用、多步操作、读写文件）常超过 120s/0.5$ → 老是「运行超时」。放宽到
    // 5 分钟 / 2$，给多步任务足够空间；仍有硬上限兜底，不会无限跑/烧钱。
    req.max_budget_usd = Some(2.0);
    req.timeout_secs = 300;
    // 连续对话需要保留会话：本轮保存（供下轮 --continue），第二轮起带 --continue 续上下文。
    req.session_persistence = true;
    req.continue_session = continue_session;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_runner = Arc::clone(&cancel);
    let run = async_runtime::spawn(async move {
        crate::coding_agent::run_claude_agent("claude", req, tx, cancel_for_runner).await
    });
    let cancel_for_watcher = Arc::clone(&cancel);
    let inner_for_cancel = Arc::clone(inner);
    let cancel_watcher = async_runtime::spawn(async move {
        loop {
            if cancel_for_watcher.load(Ordering::Relaxed) {
                return;
            }
            if inner_for_cancel.state.lock().cancelled {
                cancel_for_watcher.store(true, Ordering::Relaxed);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
    });

    let mut final_text = String::new();
    let mut cost_usd: Option<f64> = None;
    let mut error_msg: Option<String> = None;
    let mut cancelled = false;
    while let Some(ev) = rx.recv().await {
        use crate::coding_agent::CodingAgentEvent as E;
        match ev {
            E::Started { .. } => {
                emit_less_computer(inner, serde_json::json!({ "kind": "started" }));
            }
            E::Delta { text, .. } => {
                emit_less_computer(inner, serde_json::json!({ "kind": "delta", "text": text }));
            }
            E::ToolUse { name, .. } => {
                emit_less_computer(inner, serde_json::json!({ "kind": "tool", "name": name }));
            }
            E::Completed {
                text, cost_usd: c, ..
            } => {
                final_text = text;
                cost_usd = c;
            }
            E::Error { message, .. } => error_msg = Some(message),
            E::Cancelled { .. } => cancelled = true,
        }
    }
    let run_result = run.await;
    cancel.store(true, Ordering::Relaxed);
    let _ = cancel_watcher.await;
    let _ = std::fs::remove_file(&settings_path);

    if cancelled
        || matches!(
            &run_result,
            Ok(Err(crate::coding_agent::CodingAgentError::Cancelled))
        )
    {
        return LessComputerOutcome::Cancelled;
    }

    let trimmed = final_text.trim().to_string();
    if !trimmed.is_empty() {
        LessComputerOutcome::Done {
            text: trimmed,
            cost_usd,
        }
    } else {
        let message = error_msg
            .or_else(|| match run_result {
                Ok(Err(e)) => Some(e.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| "Claude 无结果（确认已登录 claude 且额度充足）".to_string());
        LessComputerOutcome::Failed { message }
    }
}

/// 护栏拦截探测 + 内联审批（best-effort）。
///
/// 无头 `claude -p`（v2.1.165）没有 mid-run 的 `--permission-prompt-tool` 通道，所以
/// 我们只能在「一轮跑完」后判断护栏是否拦了高风险动作：扫描终局文本里是否提到某个
/// 高风险模式 + 权限/拒绝/blocked 关键词。命中则发 `approval` 事件、挂一个 oneshot 等
/// 用户决断（前端 Approve/Deny → `less_computer_approve` 命令解析）。
///
/// 返回 `Some(pattern)` 表示用户 Approve 了某高风险模式 → 调用方应放行该模式重跑一轮；
/// `None` 表示无需审批 / 用户 Deny / 超时。**注意**这是「重跑放行」而非真正的 mid-run
/// 续跑——headless 下没有干净的 mid-run round-trip，详见 report。
async fn maybe_request_approval(
    inner: &Arc<Inner>,
    outcome: &LessComputerOutcome,
) -> Option<String> {
    let text = match outcome {
        LessComputerOutcome::Done { text, .. } => text.as_str(),
        LessComputerOutcome::Failed { message } => message.as_str(),
        LessComputerOutcome::Cancelled => return None,
    };
    let lowered = text.to_lowercase();
    // 必须同时出现「拒绝/权限/blocked」语义 + 某个已知高风险模式，才认为是护栏拦截，
    // 避免把正常提到 "rm" 的回答误判成审批请求。
    let mentions_block = [
        "denied",
        "permission",
        "not allowed",
        "blocked",
        "拒绝",
        "权限",
        "被拦",
    ]
    .iter()
    .any(|kw| lowered.contains(kw));
    if !mentions_block {
        return None;
    }
    let hit = crate::coding_agent::guard::HIGH_RISK_PATTERNS
        .iter()
        .find(|(pat, _)| lowered.contains(*pat))?;
    let (pattern, reason) = (hit.0.to_string(), hit.1.to_string());

    // 挂 oneshot 等用户决断。
    let token = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    if let Ok(mut map) = less_computer_approvals().lock() {
        map.insert(token.clone(), tx);
    }
    emit_less_computer(
        inner,
        serde_json::json!({
            "kind": "approval",
            "token": token,
            "command": pattern,
            "reason": reason,
        }),
    );

    // 等用户点 Approve/Deny；90s 无响应按 Deny 处理并清理注册表项。
    let approved = match tokio::time::timeout(std::time::Duration::from_secs(90), rx).await {
        Ok(Ok(v)) => v,
        _ => {
            less_computer_approvals()
                .lock()
                .ok()
                .map(|mut m| m.remove(&token));
            false
        }
    };
    if approved {
        Some(pattern)
    } else {
        None
    }
}

/// 把 prefs 里的权限模式字符串映射成枚举；未知值回落到 acceptEdits（放行+护栏的默认）。
fn coding_agent_mode_from_pref(s: &str) -> crate::coding_agent::CodingAgentPermissionMode {
    use crate::coding_agent::CodingAgentPermissionMode as M;
    match s.trim() {
        "plan" => M::Plan,
        "default" => M::Default,
        "bypassPermissions" => M::BypassPermissions,
        _ => M::AcceptEdits,
    }
}

pub(super) fn request_stop_during_starting(inner: &Arc<Inner>, reason: &str) {
    {
        let mut state = inner.state.lock();
        if !request_stop_during_starting_state(&mut state) {
            return;
        }
    }
    log::info!("[coord] {reason} during Starting — queued");
    stop_recorder_if_pending_start_stop(inner);
}

pub(super) async fn begin_session(inner: &Arc<Inner>) -> Result<(), String> {
    let current_session_id = {
        let mut state = inner.state.lock();
        let Some(session_id) =
            begin_session_state(&mut state, capture_focus_target(), capture_frontmost_app())
        else {
            return Ok(());
        };
        if let Some(label) = state.front_app.as_deref() {
            log::info!("[coord] front_app captured: {label}");
        }
        session_id
    };
    #[cfg(target_os = "windows")]
    {
        let prepared = inner.windows_ime.prepare_session();
        let mut slots = inner.prepared_windows_ime_session.lock();
        store_prepared_windows_ime_session(&mut slots, current_session_id, prepared);
    }
    // 翻译模式标志重置；hotkey 监听器在 Shift down 时再 set true。
    inner
        .translation_modifier_seen
        .store(false, Ordering::SeqCst);

    #[cfg(any(debug_assertions, test))]
    if hotkey_injection_dry_run_enabled() {
        emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);
        inner.state.lock().phase = SessionPhase::Listening;
        log::info!("[coord] session started (hotkey-injection dry-run)");
        return Ok(());
    }

    if let Err(message) = ensure_asr_credentials() {
        log::warn!("[coord] ASR credential gate failed: {message}");
        emit_capsule(
            inner,
            CapsuleState::Error,
            0.0,
            0,
            Some(message.clone()),
            None,
        );
        restore_prepared_windows_ime_session(inner, current_session_id);
        inner.state.lock().phase = SessionPhase::Idle;
        return Err(message);
    }

    let active_asr = CredentialsVault::get_active_asr();

    if let Err(message) = ensure_microphone_permission(inner) {
        log::warn!("[coord] microphone permission gate failed: {message}");
        emit_capsule(
            inner,
            CapsuleState::Error,
            0.0,
            0,
            Some(message.clone()),
            None,
        );
        restore_prepared_windows_ime_session(inner, current_session_id);
        inner.state.lock().phase = SessionPhase::Idle;
        schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
        return Err(message);
    }

    // 不在这里 emit Recording capsule —— 让 start_recorder_for_starting 在
    // Recorder::start 成功后再发，确保「用户看到录音条」时 mic 已经在 capture。
    // 之前在这一行就 emit 会让用户看到录音条后立刻开口，但 mic 还在 cpal init
    // 窗口（50-200ms）内 → 开头几个字物理上录不到。详见 issue 备注。
    #[cfg(target_os = "windows")]
    if foundry::is_foundry_local_whisper(&active_asr) {
        let prefs = inner.prefs.get();
        let model_alias = if foundry::model_alias_is_known(&prefs.foundry_local_asr_model) {
            prefs.foundry_local_asr_model.clone()
        } else {
            foundry::DEFAULT_MODEL_ALIAS.to_string()
        };
        let language_hint = prefs.foundry_local_asr_language_hint.trim().to_string();
        let language_hint = if language_hint.is_empty() {
            None
        } else {
            Some(language_hint)
        };
        let local = Arc::new(FoundryLocalWhisperAsr::new(
            Arc::clone(&inner.foundry_local_runtime),
            model_alias,
            prefs.foundry_local_runtime_source.clone(),
            language_hint,
        ));
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::FoundryLocalWhisper(Arc::clone(&local)),
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
        return Ok(());
    }

    // Windows sherpa-onnx-local：与 Foundry 同形分支，复用 Recorder /
    // ActiveAsr / start_recorder_and_enter_listening。offline 模型走 batch；
    // online 模型在 provider 内部 worker 中边录边解码，并通过 local-asr-token
    // 推 partial 给前端胶囊。
    #[cfg(target_os = "windows")]
    if sherpa::is_sherpa_onnx_local(&active_asr) {
        let prefs = inner.prefs.get();
        let model_alias = if sherpa::model_alias_is_known(&prefs.sherpa_onnx_model) {
            prefs.sherpa_onnx_model.clone()
        } else {
            sherpa::DEFAULT_MODEL_ALIAS.to_string()
        };
        let language_hint = prefs.sherpa_onnx_language_hint.trim().to_string();
        let language_hint = if language_hint.is_empty() {
            None
        } else {
            Some(language_hint)
        };
        let token_handler = inner.app.lock().clone().map(|app| {
            Arc::new(move |piece: String| {
                if let Err(error) = app.emit("local-asr-token", piece) {
                    log::warn!("[sherpa-asr] emit token failed: {error}");
                }
            }) as crate::asr::local::sherpa_provider::SherpaTokenHandler
        });
        let local = match SherpaOnnxAsr::new_for_model(
            Arc::clone(&inner.sherpa_onnx_runtime),
            model_alias,
            language_hint,
            token_handler,
        )
        .await
        {
            Ok(local) => Arc::new(local),
            Err(e) => {
                log::error!("[coord] sherpa-onnx init failed: {e:#}");
                emit_capsule(
                    inner,
                    CapsuleState::Error,
                    0.0,
                    0,
                    Some(format!("本地模型初始化失败: {e}")),
                    None,
                );
                restore_prepared_windows_ime_session(inner, current_session_id);
                inner.state.lock().phase = SessionPhase::Idle;
                schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                return Err(format!("sherpa-onnx init failed: {e}"));
            }
        };
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::SherpaOnnxLocal(Arc::clone(&local)),
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    if crate::asr::local::is_local_qwen3(&active_asr) {
        let local = match build_local_qwen3(inner).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("[coord] 本地 Qwen3-ASR 初始化失败: {e:#}");
                emit_capsule(
                    inner,
                    CapsuleState::Error,
                    0.0,
                    0,
                    Some(format!("本地模型初始化失败: {e}")),
                    None,
                );
                restore_prepared_windows_ime_session(inner, current_session_id);
                inner.state.lock().phase = SessionPhase::Idle;
                schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                return Err(format!("local ASR init failed: {e}"));
            }
        };
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Local(Arc::clone(&local)),
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
        return Ok(());
    }

    if is_bailian_provider(&active_asr) {
        let asr = Arc::new(BailianRealtimeASR::new(read_bailian_credentials()));
        let bridge = Arc::new(DeferredAsrBridge::new());
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = bridge.clone();
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Bailian(Arc::clone(&asr)),
        );
        start_recorder_for_starting(inner, current_session_id, &active_asr, consumer).await?;

        if let Err(e) = asr.open_session().await {
            log::error!("[coord] open Bailian ASR session failed: {e}");
            match startup_race_status_for_starting(inner, current_session_id) {
                StartupRaceStatus::StaleContinuation => {
                    log::info!(
                        "[coord] stale Bailian ASR open_session error from session {current_session_id} — ignoring"
                    );
                    asr.cancel();
                    discard_startup_resources_for_session(inner, current_session_id);
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    return Ok(());
                }
                StartupRaceStatus::CancelRaced => {
                    asr.cancel();
                    discard_startup_resources_for_session(inner, current_session_id);
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    set_phase_idle_if_session_matches(inner, current_session_id);
                    return Ok(());
                }
                StartupRaceStatus::ActiveStarting => {
                    asr.cancel();
                }
            }
            discard_startup_resources_for_session(inner, current_session_id);
            emit_capsule(
                inner,
                CapsuleState::Error,
                0.0,
                0,
                Some(format!("ASR 连接失败: {e}")),
                None,
            );
            restore_prepared_windows_ime_session(inner, current_session_id);
            set_phase_idle_if_session_matches(inner, current_session_id);
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            return Err(e.to_string());
        }
        match startup_race_status_for_starting(inner, current_session_id) {
            StartupRaceStatus::ActiveStarting => {}
            StartupRaceStatus::CancelRaced => {
                log::info!("[coord] cancel raced during Bailian ASR open_session — aborting begin");
                asr.cancel();
                discard_startup_resources_for_session(inner, current_session_id);
                restore_prepared_windows_ime_session(inner, current_session_id);
                set_phase_idle_if_session_matches(inner, current_session_id);
                return Ok(());
            }
            StartupRaceStatus::StaleContinuation => {
                log::info!(
                    "[coord] stale Bailian ASR open_session continuation from session {current_session_id} — ignoring"
                );
                asr.cancel();
                discard_startup_resources_for_session(inner, current_session_id);
                restore_prepared_windows_ime_session(inner, current_session_id);
                return Ok(());
            }
        }
        let target: Arc<dyn crate::asr::AudioConsumer> = asr;
        let flushed_bytes = bridge.attach(target);
        log::info!("[coord] Bailian ASR connected; flushed {flushed_bytes} deferred audio bytes");
        finish_starting_session(inner, current_session_id).await;
    } else if is_mimo_provider(&active_asr) {
        let (api_key, base_url, model) = read_mimo_credentials();
        let mimo = Arc::new(MimoBatchASR::new(api_key, base_url, model));
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Mimo(Arc::clone(&mimo)),
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = mimo;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
    } else if is_whisper_compatible_provider(&active_asr) {
        let (api_key, base_url, model) = read_whisper_credentials();
        // 用户辞書の有効フレーズを Whisper の `prompt` に流し込む。固有名詞や
        // 専門用語の同音・近形誤認識を ASR 段階で抑える。Polish LLM 側には
        // 既に system prompt として注入済みだが、Whisper 出力が大きく崩れる
        // と Polish でも救えない（特に CJK で顕著）。Volcengine ASR は元々
        // hotword を受け取っており、UI 説明文も「ASR ホットワードと後処理
        // モデルのコンテキスト両方に渡される」と明示しているので、Whisper
        // 互換プロバイダにも揃えるのが筋。
        let whisper_prompt =
            crate::asr::whisper::build_prompt_from_phrases(&enabled_phrases(inner));
        let whisper = Arc::new(
            WhisperBatchASR::new(
                api_key,
                base_url,
                model,
                whisper_prompt,
                batch_asr_chunk_limit_ms(&active_asr),
                whisper_supports_verbose_json(&active_asr),
            )
            .with_request_format(whisper_request_format(&active_asr)),
        );
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Whisper(Arc::clone(&whisper)),
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = whisper;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
    } else {
        let hotwords = enabled_hotwords(inner);
        let creds = read_volc_credentials();
        let asr = Arc::new(VolcengineStreamingASR::new(creds, hotwords));
        let bridge = Arc::new(DeferredAsrBridge::new());
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = bridge.clone();
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Volcengine(Arc::clone(&asr)),
        );
        start_recorder_for_starting(inner, current_session_id, &active_asr, consumer).await?;

        if let Err(e) = asr.open_session().await {
            log::error!("[coord] open ASR session failed: {e}");
            match startup_race_status_for_starting(inner, current_session_id) {
                StartupRaceStatus::StaleContinuation => {
                    log::info!(
                        "[coord] stale ASR open_session error from session {current_session_id} — ignoring"
                    );
                    asr.cancel();
                    discard_startup_resources_for_session(inner, current_session_id);
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    return Ok(());
                }
                StartupRaceStatus::CancelRaced => {
                    asr.cancel();
                    discard_startup_resources_for_session(inner, current_session_id);
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    set_phase_idle_if_session_matches(inner, current_session_id);
                    return Ok(());
                }
                StartupRaceStatus::ActiveStarting => {}
            }
            discard_startup_resources_for_session(inner, current_session_id);
            emit_capsule(
                inner,
                CapsuleState::Error,
                0.0,
                0,
                Some(format!("ASR 连接失败: {e}")),
                None,
            );
            restore_prepared_windows_ime_session(inner, current_session_id);
            set_phase_idle_if_session_matches(inner, current_session_id);
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            return Err(e.to_string());
        }
        // open_session.await 期间用户可能按了 Esc / 改变心意。如果 cancel_session
        // 已触发（cancelled=true 或 phase 被改回 Idle），别再装 ASR，直接善后。
        // audit HIGH #1。
        match startup_race_status_for_starting(inner, current_session_id) {
            StartupRaceStatus::ActiveStarting => {}
            StartupRaceStatus::CancelRaced => {
                log::info!("[coord] cancel raced during ASR open_session — aborting begin");
                asr.cancel();
                discard_startup_resources_for_session(inner, current_session_id);
                restore_prepared_windows_ime_session(inner, current_session_id);
                set_phase_idle_if_session_matches(inner, current_session_id);
                return Ok(());
            }
            StartupRaceStatus::StaleContinuation => {
                log::info!(
                    "[coord] stale ASR open_session continuation from session {current_session_id} — ignoring"
                );
                asr.cancel();
                discard_startup_resources_for_session(inner, current_session_id);
                restore_prepared_windows_ime_session(inner, current_session_id);
                return Ok(());
            }
        }
        let target: Arc<dyn crate::asr::AudioConsumer> = asr;
        let flushed_bytes = bridge.attach(target);
        log::info!("[coord] ASR connected; flushed {flushed_bytes} deferred audio bytes");
        finish_starting_session(inner, current_session_id).await;
    }

    Ok(())
}

pub(super) async fn start_recorder_for_starting(
    inner: &Arc<Inner>,
    session_id: SessionId,
    active_asr: &str,
    consumer: Arc<dyn crate::recorder::AudioConsumer>,
) -> Result<(), String> {
    let inner_for_level = Arc::clone(inner);
    // 节流：电平回调本身约 185 Hz（cpal 默认音频块），全部转发到前端会让 CSS
    // transition 互相覆盖、视觉上"被平均"成静止。限制为 ~30 Hz（33ms 最少间隔），
    // 配合 CSS 短 transition 让每次 emit 完整可见。
    let last_emit_at = Arc::new(Mutex::new(None::<Instant>));
    const LEVEL_EMIT_MIN_INTERVAL_MS: u64 = 33;
    let level_handler: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level| {
        let phase = inner_for_level.state.lock().phase;
        if phase != SessionPhase::Listening && phase != SessionPhase::Starting {
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
        let elapsed = inner_for_level
            .state
            .lock()
            .started_at
            .elapsed()
            .as_millis() as u64;
        emit_capsule(
            &inner_for_level,
            CapsuleState::Recording,
            level,
            elapsed,
            None,
            None,
        );
    });

    let microphone_device_name = selected_microphone_device_name(inner);
    stop_microphone_preview_monitor(inner, "dictation recorder");
    acquire_recording_mute(inner, "dictation").await;
    let audio_archive_path = if inner.prefs.get().record_audio_for_debug {
        // 用 coordinator 的 SessionId 作为文件名，跟 history 那条记录 id 对齐（见
        // 下游 polish 收尾时 `history_session_id = current_session_id.to_string()`）。
        // 顺手把超龄 / 超量录音清理一下，避免 debug 开关常开时磁盘膨胀。
        let prefs = inner.prefs.get();
        let _ = crate::persistence::prune_recordings(
            prefs.history_retention_days,
            prefs.audio_recording_max_entries,
        );
        crate::persistence::recording_path_for_session(&session_id.to_string()).ok()
    } else {
        None
    };
    match Recorder::start(
        microphone_device_name,
        consumer,
        level_handler,
        audio_archive_path,
    ) {
        Ok((rec, runtime_errors, archive_active)) => {
            // 把 archive 实际创建状态存到 Inner，让 history 写入路径（含 empty-transcript
            // 失败分支）读真实情况，而不是 prefs 开关。修 pr_agent "Wrong Flag" 反馈。
            inner
                .audio_archive_active
                .store(archive_active, std::sync::atomic::Ordering::Relaxed);
            store_recorder_for_session(inner, session_id, rec);
            spawn_recorder_error_monitor(inner, runtime_errors);
            // 不在这里 emit Recording capsule。
            // Recorder::start Ok 仅代表 cpal Stream::play 完成，不代表 audio
            // 线程已经在向 consumer 推 PCM —— macOS CoreAudio AudioUnit 启动到
            // 第一帧 process_callback 中间有 50–200 ms 间隙（Windows 类似）。
            // 之前在这里立即 emit Recording 会让用户「看到录音条」就开口，但前几个
            // 字落在 cpal init 窗口里被吞，反映为短录音漏首字（用户报告）。
            //
            // 现改为：level_handler 第一次被触发时才 emit Recording capsule。
            // recorder.rs::process_callback 的顺序是 consume_pcm_chunk → level_handler，
            // 所以 level_handler 第一次执行 == PCM 已经真实流到 consumer。从这一刻
            // 起用户说什么都被录到。capsule 自然就晚 50–200 ms 出现，但出现 ==
            // mic 真的在录，匹配「麦先录、UI 再弹」的预期。
            //
            // 原本的竞态保护交还给两条已有路径：
            //   - stop_recorder_if_pending_start_stop：短按时把 capsule 切到
            //     Transcribing；recorder 已 stop，level_handler 不会再发火。
            //   - level_handler 内部 phase 检查：cancel / 错误使 phase 不在
            //     {Starting, Listening} 时直接 return，不会在错误状态上盖
            //     Recording。
            stop_recorder_if_pending_start_stop(inner);
            log::info!("[coord] recorder started (asr={active_asr}, phase=Starting)");
        }
        Err(e) => {
            log::error!("[coord] recorder start failed: {e}");
            cancel_asr_for_session(inner, session_id);
            emit_capsule(
                inner,
                CapsuleState::Error,
                0.0,
                0,
                Some(format!("录音启动失败: {e}")),
                None,
            );
            restore_prepared_windows_ime_session(inner, session_id);
            release_recording_mute(inner, "dictation");
            inner.state.lock().phase = SessionPhase::Idle;
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            return Err(e.to_string());
        }
    }

    Ok(())
}

pub(super) fn spawn_recorder_error_monitor(inner: &Arc<Inner>, rx: mpsc::Receiver<RecorderError>) {
    // 捕获当前 session_id：err 来时若 id 已经不一致说明是上一 session 的迟到事件，
    // 不能去 abort 当前 active 的新 session（它录得好好的）。
    let captured_session_id = inner.state.lock().session_id;
    let inner = Arc::clone(inner);
    std::thread::Builder::new()
        .name("openless-recorder-error-monitor".into())
        .spawn(move || {
            if let Ok(err) = rx.recv() {
                let current_session_id = inner.state.lock().session_id;
                if captured_session_id != current_session_id {
                    log::warn!(
                        "[coord] recorder error from stale session {} dropped (current={}, err={})",
                        captured_session_id,
                        current_session_id,
                        err
                    );
                    return;
                }
                log::error!("[coord] recorder runtime error: {err}");
                abort_recording_with_error(&inner, format!("录音中断: {err}"));
            }
        })
        .ok();
}

pub(super) fn abort_recording_with_error(inner: &Arc<Inner>, message: String) {
    let Some(abort) = ({
        let mut state = inner.state.lock();
        begin_recording_abort_before_restore(&mut state)
    }) else {
        return;
    };

    discard_startup_resources_for_session(inner, abort.session_id);
    restore_prepared_windows_ime_session(inner, abort.session_id);
    {
        let mut state = inner.state.lock();
        publish_abort_idle_after_restore(&mut state, abort.session_id);
    }

    emit_capsule(
        inner,
        CapsuleState::Error,
        0.0,
        abort.elapsed,
        Some(message),
        None,
    );
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
}

pub(super) async fn start_recorder_and_enter_listening(
    inner: &Arc<Inner>,
    session_id: SessionId,
    active_asr: &str,
    consumer: Arc<dyn crate::recorder::AudioConsumer>,
) -> Result<(), String> {
    start_recorder_for_starting(inner, session_id, active_asr, consumer).await?;
    finish_starting_session(inner, session_id).await;
    Ok(())
}

pub(super) async fn finish_starting_session(inner: &Arc<Inner>, session_id: SessionId) {
    // audit HIGH #1：转 Listening 之前在同一 lock 内检查 cancel race。
    // 之前是无条件 phase=Listening，会把 cancel_session 在 await 期间设的 Idle
    // 反向覆盖回 Listening → 用户的 cancel 边沿被吞掉。
    let outcome = {
        let mut state = inner.state.lock();
        finish_starting_session_state(&mut state, session_id)
    };
    match outcome {
        BeginOutcome::StaleContinuation => {
            log::info!(
                "[coord] stale recorder/ASR startup continuation from session {session_id} — ignoring"
            );
            discard_startup_resources_for_session(inner, session_id);
            restore_prepared_windows_ime_session(inner, session_id);
        }
        BeginOutcome::CancelRaced => {
            log::info!("[coord] cancel raced during recorder/ASR startup — aborting begin");
            discard_startup_resources_for_session(inner, session_id);
            restore_prepared_windows_ime_session(inner, session_id);
            set_phase_idle_if_session_matches(inner, session_id);
        }
        BeginOutcome::Started | BeginOutcome::PendingStop => {
            log::info!("[coord] session started");
            if matches!(outcome, BeginOutcome::PendingStop) {
                log::info!("[coord] applying pending_stop edge → end_session immediately");
                let _ = end_session(inner).await;
            }
        }
    }
}

pub(super) async fn end_session(inner: &Arc<Inner>) -> Result<(), String> {
    let current_session_id = {
        let mut state = inner.state.lock();
        let Some(session_id) = start_processing_if_listening(&mut state) else {
            return Ok(());
        };
        session_id
    };

    let elapsed = inner.state.lock().started_at.elapsed().as_millis() as u64;
    emit_capsule(inner, CapsuleState::Transcribing, 0.0, elapsed, None, None);

    if let Some(rec) = take_recorder_for_session(inner, current_session_id) {
        rec.stop();
        release_recording_mute(inner, "dictation");
    }

    let asr_opt = take_asr_for_session(inner, current_session_id);
    let asr = match asr_opt {
        Some(a) => a,
        None => {
            restore_prepared_windows_ime_session(inner, current_session_id);
            inner.state.lock().phase = SessionPhase::Idle;
            return Ok(());
        }
    };

    let uses_global_timeout = asr_transcribe_uses_global_timeout(&asr);
    let raw = match asr {
        ActiveAsr::Volcengine(asr) => {
            debug_assert!(uses_global_timeout);
            if let Err(e) = asr.send_last_frame().await {
                log::error!("[coord] send last frame failed: {e}");
            }
            // 添加全局超时保护：防止 await_final_result() 永远挂起
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] await final failed: {e}");
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some(format!("识别失败: {e}")),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err(e.to_string());
                }
                Err(_) => {
                    // 全局超时：最后的防线
                    log::error!(
                        "[coord] 全局超时 {} 秒 - 强制恢复",
                        COORDINATOR_GLOBAL_TIMEOUT_SECS
                    );
                    // 清理 ASR session，避免资源泄漏
                    asr.cancel();
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some("识别超时".to_string()),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err("global timeout".to_string());
                }
            }
        }
        ActiveAsr::Whisper(w) => {
            debug_assert!(uses_global_timeout);
            // Whisper 也添加类似的超时保护
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, w.transcribe()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] whisper transcribe failed: {e}");
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some(format!("识别失败: {e}")),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] whisper 全局超时 {} 秒",
                        COORDINATOR_GLOBAL_TIMEOUT_SECS
                    );
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some("识别超时".to_string()),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
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
                    log::error!("[coord] MiMo ASR transcribe failed: {e}");
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some(format!("识别失败: {e}")),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] MiMo ASR 全局超时 {} 秒",
                        COORDINATOR_GLOBAL_TIMEOUT_SECS
                    );
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some("识别超时".to_string()),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err("mimo global timeout".to_string());
                }
            }
        }
        ActiveAsr::Bailian(asr) => {
            debug_assert!(uses_global_timeout);
            if let Err(e) = asr.send_last_frame().await {
                log::error!("[coord] Bailian send last frame failed: {e}");
            }
            let timeout_duration = std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
            match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] Bailian await final failed: {e}");
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some(format!("识别失败: {e}")),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] Bailian 全局超时 {} 秒",
                        COORDINATOR_GLOBAL_TIMEOUT_SECS
                    );
                    asr.cancel();
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some("识别超时".to_string()),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err("bailian global timeout".to_string());
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
                    schedule_foundry_local_asr_release(
                        inner,
                        AsrReleaseSession::Dictation(current_session_id),
                    );
                    r
                }
                Err(e) => {
                    if inner.state.lock().cancelled {
                        log::info!(
                            "[coord] Foundry Local Whisper transcribe cancelled — discarding transcript"
                        );
                        schedule_foundry_local_asr_release(
                            inner,
                            AsrReleaseSession::Dictation(current_session_id),
                        );
                        restore_prepared_windows_ime_session(inner, current_session_id);
                        set_phase_idle_if_session_matches(inner, current_session_id);
                        return Ok(());
                    }
                    log::error!("[coord] Foundry Local Whisper transcribe failed: {e:#}");
                    schedule_foundry_local_asr_release(
                        inner,
                        AsrReleaseSession::Dictation(current_session_id),
                    );
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some(format!("本地识别失败: {e}")),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err(e.to_string());
                }
            }
        }
        // Windows sherpa-onnx offline batch：停止录音后整段转写，再复用现有
        // polish / insert / history 收尾路径。
        #[cfg(target_os = "windows")]
        ActiveAsr::SherpaOnnxLocal(local) => {
            debug_assert!(!uses_global_timeout);
            match local
                .transcribe(sherpa_audio_transcribe_timeout_duration())
                .await
            {
                Ok(r) => {
                    schedule_sherpa_onnx_release(
                        inner,
                        AsrReleaseSession::Dictation(current_session_id),
                    );
                    r
                }
                Err(e) => {
                    if inner.state.lock().cancelled {
                        log::info!(
                            "[coord] sherpa-onnx transcribe cancelled — discarding transcript"
                        );
                        schedule_sherpa_onnx_release(
                            inner,
                            AsrReleaseSession::Dictation(current_session_id),
                        );
                        restore_prepared_windows_ime_session(inner, current_session_id);
                        set_phase_idle_if_session_matches(inner, current_session_id);
                        return Ok(());
                    }
                    log::error!("[coord] sherpa-onnx transcribe failed: {e:#}");
                    schedule_sherpa_onnx_release(
                        inner,
                        AsrReleaseSession::Dictation(current_session_id),
                    );
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some(format!("本地识别失败: {e}")),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err(e.to_string());
                }
            }
        }
        #[cfg(target_os = "macos")]
        ActiveAsr::Local(local) => {
            debug_assert!(uses_global_timeout);
            // 缓存命中时 transcribe 不含 load 时间；冷启动 load 已在 build_local_qwen3
            // 提前完成。但 transcribe 本身受音频长度影响：用户实测 RTF ≈ 0.3，慢机
            // 可达 0.5；15s 固定超时在 ≥ 30s 录音上会把整段结果丢掉。改用动态
            // 超时 max(15, ceil(audio_s × 0.6) + 10)，公式与单测见
            // `local_qwen_transcribe_timeout`。
            let audio_secs = (local.buffer_duration_ms() as f64) / 1000.0;
            let timeout_duration = local_qwen_transcribe_timeout(audio_secs);
            log::info!(
                "[coord] local Qwen3-ASR transcribe: audio={:.2}s timeout={}s",
                audio_secs,
                timeout_duration.as_secs()
            );
            let result = tokio::time::timeout(timeout_duration, local.transcribe()).await;
            inner.local_asr_cache.touch();
            schedule_local_asr_release(inner);
            match result {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log::error!("[coord] local Qwen3-ASR transcribe failed: {e:#}");
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some(format!("本地识别失败: {e}")),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] local Qwen3-ASR 动态超时 {}s（音频 {:.2}s）",
                        timeout_duration.as_secs(),
                        audio_secs
                    );
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some("识别超时".to_string()),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err("local global timeout".to_string());
                }
            }
        }
        // Apple Speech：系统语音识别，无模型加载耗时。批处理 transcribe 受音频
        // 长度影响，沿用 local_qwen_transcribe_timeout 的动态超时公式。
        #[cfg(target_os = "macos")]
        ActiveAsr::AppleSpeech(local) => {
            debug_assert!(uses_global_timeout);
            let audio_secs = (local.buffer_duration_ms() as f64) / 1000.0;
            let timeout_duration = local_qwen_transcribe_timeout(audio_secs);
            log::info!(
                "[coord] Apple Speech transcribe: audio={:.2}s timeout={}s",
                audio_secs,
                timeout_duration.as_secs()
            );
            match tokio::time::timeout(timeout_duration, local.transcribe()).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    if inner.state.lock().cancelled {
                        log::info!(
                            "[coord] Apple Speech transcribe cancelled - discarding transcript"
                        );
                        restore_prepared_windows_ime_session(inner, current_session_id);
                        set_phase_idle_if_session_matches(inner, current_session_id);
                        return Ok(());
                    }
                    log::error!("[coord] Apple Speech transcribe failed: {e:#}");
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some(format!("本地识别失败: {e}")),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err(e.to_string());
                }
                Err(_) => {
                    log::error!(
                        "[coord] Apple Speech 动态超时 {}s（音频 {:.2}s）",
                        timeout_duration.as_secs(),
                        audio_secs
                    );
                    emit_capsule(
                        inner,
                        CapsuleState::Error,
                        0.0,
                        elapsed,
                        Some("识别超时".to_string()),
                        None,
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    inner.state.lock().phase = SessionPhase::Idle;
                    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                    return Err("apple-speech global timeout".to_string());
                }
            }
        }
    };

    // ASR 完成后 cancel 检查：用户在 transcribe 进行中按 Esc 时，这里就会命中。
    // 优先级高于 empty 检查 — 用户取消 → 静默丢弃，不写失败历史也不弹错误胶囊。
    if inner.state.lock().cancelled {
        log::info!("[coord] cancel detected after ASR — discarding transcript");
        restore_prepared_windows_ime_session(inner, current_session_id);
        // PR #387 的「cancel 后清 focus_target」契约要在 Processing 路径上也成立。
        // cancel_session 在 Processing 阶段故意跳过 finish_cancel_session_state（让
        // 这里收尾），但此前的 end_session 没把 focus_target 清掉。logic-review
        // 2026-05-10 P3 (🚩) 把这条补完。
        {
            let mut state = inner.state.lock();
            state.phase = SessionPhase::Idle;
            state.focus_target = None;
        }
        return Ok(());
    }

    // ASR 返回空转写护栏（来自 PR #66）：写一条 emptyTranscript 失败历史 + 错误胶囊，
    // 与 main 上其它 error 路径保持一致（带 schedule_capsule_idle 让胶囊自动消失）。
    let mut raw = raw;

    #[cfg(any(debug_assertions, test))]
    if raw.text.trim().is_empty() {
        if let Some(debug_text) = debug_transcript_override_text() {
            log::info!(
                "[coord] using debug transcript override (chars={})",
                debug_text.chars().count()
            );
            raw.text = debug_text;
        }
    }

    if raw.text.trim().is_empty() {
        let session = DictationSession {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now().to_rfc3339(),
            raw_transcript: raw.text.clone(),
            final_text: String::new(),
            mode: inner.prefs.get().default_mode,
            style_pack_id: None,
            translation_active: false,
            polish_source: None,
            app_bundle_id: None,
            app_name: None,
            insert_status: InsertStatus::Failed,
            error_code: Some("emptyTranscript".to_string()),
            duration_ms: Some(raw.duration_ms),
            dictionary_entry_count: Some(enabled_phrases(inner).len() as u32),
            // empty-transcript（ASR 没识别到任何文字）也保留 wav 标记——这是用户最想
            // 通过原始录音定位"是不是麦克风太小声 / ASR 模型问题"的场景。修 pr_agent
            // "Missing Audio" 反馈。
            has_audio_recording: Some(inner.audio_archive_active.load(Ordering::Relaxed)),
        };
        let prefs_snapshot = inner.prefs.get();
        if let Err(e) = inner.history.append_with_retention(
            session,
            prefs_snapshot.history_retention_days,
            prefs_snapshot.history_max_entries,
        ) {
            log::error!("[coord] history append failed: {e}");
        }
        emit_capsule(
            inner,
            CapsuleState::Error,
            0.0,
            elapsed,
            Some("没有识别到语音".to_string()),
            None,
        );
        restore_prepared_windows_ime_session(inner, current_session_id);
        inner.state.lock().phase = SessionPhase::Idle;
        schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
        return Err("ASR returned empty transcript".to_string());
    }

    let correction_rules = match inner.correction_rules.list() {
        Ok(rules) => rules,
        Err(e) => {
            log::warn!("[coord] load correction rules failed: {e}; continue without correction");
            Vec::new()
        }
    };
    let front_app = inner.state.lock().front_app.clone();
    if !correction_rules.is_empty() {
        let corrected = apply_correction_rules(&raw.text, &correction_rules);
        if corrected != raw.text {
            log::info!(
                "[coord] correction rules adjusted raw transcript ({} → {} chars)",
                raw.text.chars().count(),
                corrected.chars().count()
            );
            raw.text = corrected;
        }
    }

    // Cloud Agent 语音分流：长按升级的会话不走润色/插入，转写交给 Claude 跑任务、结果弹胶囊。
    if inner.state.lock().voice_agent {
        return run_voice_agent_transcript(inner, current_session_id, raw.text.clone(), elapsed)
            .await;
    }

    emit_capsule(inner, CapsuleState::Polishing, 0.0, elapsed, None, None);

    let prefs = inner.prefs.get();
    let pack = match inner
        .style_packs
        .get_or_default_active(&prefs.active_style_pack_id)
    {
        Ok(pack) => pack,
        Err(error) => {
            log::warn!(
                "[coord] active style pack unavailable, falling back to builtin light: {error}"
            );
            crate::types::builtin_style_pack_for_mode(PolishMode::Light)
        }
    };
    let mode = pack.base_mode;
    let hotword_strs = enabled_phrases(inner);
    let working_languages = prefs.working_languages.clone();
    let chinese_script_preference = prefs.chinese_script_preference;
    let output_language_preference = prefs.output_language_preference;
    let llm_thinking_enabled = prefs.llm_thinking_enabled;
    let style_system_prompt = pack.prompt.clone();
    let raw_uses_llm = mode == PolishMode::Raw && super::raw_style_pack_uses_llm(&pack);
    let translation_target = prefs.translation_target_language.trim().to_string();
    let translation_active =
        inner.translation_modifier_seen.load(Ordering::SeqCst) && !translation_target.is_empty();
    log::info!(
        "[style-pack] runtime dispatch session_id={} active_pack={} kind={:?} mode={:?} raw_chars={} prompt_chars={} raw_uses_llm={} translation_active={} hotwords={} working_languages={:?}",
        current_session_id,
        pack.id,
        pack.kind,
        mode,
        raw.text.chars().count(),
        style_system_prompt.chars().count(),
        raw_uses_llm,
        translation_active,
        hotword_strs.len(),
        working_languages
    );
    // 对话感知 polish：拉最近 N 分钟的会话作为 LLM 上下文。翻译现在也走"润色+翻译"单次
    // LLM 调用，所以翻译路径同样需要上下文；只有 Raw 且不走 LLM 才没意义。窗口=0 时为空 Vec。
    // 只复用同一 active style pack 的历史；翻译历史按当前是否翻译决定喂译文还是润色后源文
    // （见 eligible_polish_context_turns）。
    let polish_context_window_minutes = prefs.polish_context_window_minutes;
    let prior_turns: Vec<(String, String)> = if (translation_active
        || mode != PolishMode::Raw
        || raw_uses_llm)
        && polish_context_window_minutes > 0
    {
        match inner
            .history
            .recent_within_minutes(polish_context_window_minutes)
        {
            Ok(sessions) => eligible_polish_context_turns(sessions, &pack.id, translation_active),
            Err(e) => {
                log::warn!("[coord] fetch polish context failed: {e}; fall back to single-turn");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    // 流式插入 opt-in 路径：开关打开 + 非翻译 + 非 Raw 模式 → 进入流式分支。
    // 任何不满足都走原一次性 polish_or_passthrough 路径，行为跟历史完全一致。
    let streaming_eligible = streaming_insert_eligible(
        prefs.streaming_insert,
        translation_active,
        mode,
        raw_uses_llm,
    );
    log::info!(
        "[coord] polish dispatch: translation={translation_active} mode={mode:?} streaming_eligible={streaming_eligible}"
    );

    // Linux: emit_capsule(Polishing) 已通过 fcitx5 auxDown 显示 "✨ 润色中..."，
    // 无需在此重复调用。

    // 翻译会话润色后的源语言文本（译文前的中间产物），仅翻译路径解析成功时有值，
    // 写进 history 供后续普通润色轮复用（剔除译文、避免外语污染）。
    let mut polish_source: Option<String> = None;
    let (polished, polish_error, already_streamed) = if translation_active {
        log::info!(
            "[coord] translation mode → target=\u{300C}{}\u{300D} working={:?} front_app={:?}",
            translation_target,
            working_languages,
            front_app
        );
        let (p, src, e) = polish_and_translate_or_passthrough(
            &raw,
            &translation_target,
            mode,
            &hotword_strs,
            &working_languages,
            chinese_script_preference,
            output_language_preference,
            llm_thinking_enabled,
            front_app.as_deref(),
            &prior_turns,
        )
        .await;
        polish_source = src;
        (p, e, false)
    } else if streaming_eligible {
        run_streaming_polish(
            inner,
            &raw,
            mode,
            &hotword_strs,
            &style_system_prompt,
            &working_languages,
            chinese_script_preference,
            output_language_preference,
            llm_thinking_enabled,
            front_app.as_deref(),
            &prior_turns,
        )
        .await
    } else {
        let (p, e) = polish_or_passthrough(
            &raw,
            mode,
            &hotword_strs,
            &style_system_prompt,
            &working_languages,
            chinese_script_preference,
            output_language_preference,
            llm_thinking_enabled,
            front_app.as_deref(),
            &prior_turns,
        )
        .await;
        (p, e, false)
    };

    let polished = finalize_polished_text(
        polished,
        translation_active,
        raw_uses_llm,
        mode,
        &polish_error,
        chinese_script_preference,
        &correction_rules,
        already_streamed,
    );
    // 原子化最后一次 cancel 检查 + 转 Inserting：
    // 在同一 lock 内决定「丢弃」还是「进入 Inserting」。一旦设到 Inserting，
    // cancel_session 就拒绝介入（Cmd+V 已发出，撤销不掉）。这是 audit HIGH #2 的修复，
    // 之前 check 与 inserter.insert 之间有窗口期。
    //
    // 流式路径例外：`already_streamed = true` 表示字符已经一边流一边落到光标了，
    // 撤销不掉。即使 cancel 旗在中途被立起来，也只能尊重「已经发生」的事实，进入
    // Inserting 状态完成 history / vocab 等收尾工作。
    let proceed_to_insert = {
        let mut state = inner.state.lock();
        if state.cancelled && !already_streamed {
            state.phase = SessionPhase::Idle;
            false
        } else {
            state.phase = SessionPhase::Inserting;
            true
        }
    };
    if !proceed_to_insert {
        log::info!(
            "[coord] cancel detected before insert — discarding output (chars={})",
            polished.chars().count()
        );
        restore_prepared_windows_ime_session(inner, current_session_id);
        return Ok(());
    }

    let focus_target = inner.state.lock().focus_target;
    let focus_ready_for_paste = restore_focus_target_if_possible(focus_target);
    let prefs = inner.prefs.get();
    let restore_clipboard = prefs.restore_clipboard_after_paste;
    let allow_non_tsf_insertion_fallback = prefs.allow_non_tsf_insertion_fallback;
    let paste_shortcut = prefs.paste_shortcut;
    // 流式路径下，字符已经通过 Unicode keystroke 落到光标处，跳过 inserter.insert。
    let status = if already_streamed {
        log::info!(
            "[coord] insertion skipped: {} chars already streamed via unicode_keystroke (polish_error={:?})",
            polished.chars().count(),
            polish_error
        );
        InsertStatus::Inserted
    } else {
        #[cfg(target_os = "android")]
        {
            crate::android::android_insert_with_strategy(
                &inner.inserter,
                &polished,
                inner.prefs.get().android_insert_strategy,
            )
        }
        #[cfg(not(target_os = "android"))]
        if focus_ready_for_paste {
            #[cfg(target_os = "windows")]
            {
                let ime_target = capture_ime_submit_target();
                insert_with_windows_ime_first(
                    inner,
                    current_session_id,
                    &polished,
                    restore_clipboard,
                    allow_non_tsf_insertion_fallback,
                    paste_shortcut,
                    ime_target,
                )
                .await
            }
            #[cfg(not(target_os = "windows"))]
            {
                inner
                    .inserter
                    .insert(&polished, restore_clipboard, paste_shortcut)
            }
        } else {
            #[cfg(target_os = "linux")]
            {
                // Linux: fcitx5 commitString 无需窗口焦点，始终尝试插入。
                inner
                    .inserter
                    .insert(&polished, restore_clipboard, paste_shortcut)
            }
            #[cfg(not(target_os = "linux"))]
            {
                log::warn!(
                    "[coord] original insertion target is not foreground; copied output without paste"
                );
                if allow_non_tsf_insertion_fallback {
                    inner.inserter.copy_fallback(&polished)
                } else {
                    InsertStatus::Failed
                }
            }
        }
    };
    restore_prepared_windows_ime_session(inner, current_session_id);
    let inserted_chars = polished.chars().count() as u32;

    // 累计每条 enabled 词条在最终文本中的命中次数。
    // 用 polished（最终插入的文本）扫描，与用户实际看到的输出一致。
    let total_hits: u64 = match inner.vocab.record_hits(&polished) {
        Ok(n) => n,
        Err(e) => {
            log::error!("[coord] record_hits failed: {e}");
            0
        }
    };
    // 词汇本页面在打开时通常需要立即看到 hits 增长，否则用户得手动切走再切回来才刷新。
    // 命中数 > 0 时通知前端：Vocab 页面订阅 vocab:updated 即时 listVocab() 重新加载。
    if total_hits > 0 {
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit("vocab:updated", total_hits);
        }
    }

    // polish 失败时在 history 里标记 polishFailed，让用户能在历史详情看到为什么这次输出
    // 不是预期的 mode 风格。即使失败也不丢词 — final_text 仍是原文（保留"用户的话不丢"语义）。
    let mut error_code = dictation_error_code(
        status,
        polish_error.is_some(),
        focus_ready_for_paste,
        allow_non_tsf_insertion_fallback,
    )
    .map(str::to_string);
    let prefs_snapshot = inner.prefs.get();
    error_code = resolve_polish_unchanged_error_code(
        prefs_snapshot.polish_unchanged_enabled,
        error_code,
        mode,
        translation_active,
        &polish_error,
        &raw.text,
        &polished,
    );
    let tsf_required_insert_failed = error_code.as_deref() == Some("windowsImeTsfRequired");

    // 与 coordinator 内部 SessionId 对齐：方便 recorder 旁路写盘的 `<session_id>.wav`
    // 跟 history 这条 DictationSession.id 同名，前端凭 id 就能找到对应录音文件。
    let history_session_id = current_session_id.to_string();
    let history_created_at = Utc::now().to_rfc3339();
    let session = DictationSession {
        id: history_session_id.clone(),
        created_at: history_created_at.clone(),
        raw_transcript: raw.text.clone(),
        final_text: polished.clone(),
        mode,
        style_pack_id: Some(pack.id.clone()),
        translation_active,
        polish_source,
        app_bundle_id: None,
        app_name: None,
        insert_status: status,
        error_code,
        duration_ms: Some(raw.duration_ms),
        // 历史详情页的"X 个热词"显示：用本次实际命中次数（每个匹配实例算一次），
        // 比"启用词条总数"更能反映本段口述命中了多少。u64 → u32 截断对单段听写足够。
        dictionary_entry_count: Some(total_hits.min(u32::MAX as u64) as u32),
        // 用 begin_session 时 Recorder::start 返回的实际写盘状态，而不是 prefs 开关——
        // 开关打开但路径创建失败时这里是 false，避免前端渲染播放按钮后端 404。
        has_audio_recording: Some(inner.audio_archive_active.load(Ordering::Relaxed)),
    };
    if let Err(e) = inner.history.append_with_retention(
        session,
        prefs_snapshot.history_retention_days,
        prefs_snapshot.history_max_entries,
    ) {
        log::error!("[coord] history append failed: {e}");
    }
    let done_message = if tsf_required_insert_failed {
        Some("TSF 未上屏，已禁止非 TSF 兜底".to_string())
    } else if error_code.as_deref() == Some(POLISH_UNCHANGED_ERROR_CODE) {
        Some("本次润色未产生变化，可在历史中重新润色".to_string())
    } else {
        default_done_message(status, polish_error.is_some())
    };

    emit_capsule(
        inner,
        CapsuleState::Done,
        0.0,
        elapsed,
        done_message,
        Some(inserted_chars),
    );

    {
        let mut state = inner.state.lock();
        state.phase = SessionPhase::Idle;
        state.focus_target = None;
    }
    // Toggle 模式冷却：设冷却时间戳，POST_SESSION_COOLDOWN_MS 内禁止新的 activate。
    // 覆盖胶囊离场动画周期，避免三连按第 3 次误激活（issue #545）。
    {
        let now = std::time::Instant::now();
        *inner.session_cooldown_until.lock() =
            Some(now + std::time::Duration::from_millis(POST_SESSION_COOLDOWN_MS));
    }
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);

    Ok(())
}

pub(super) const POLISH_UNCHANGED_ERROR_CODE: &str = "polishUnchanged";

fn is_zero_width_char(ch: char) -> bool {
    matches!(ch, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{2060}')
}

pub(super) fn normalize_for_polish_compare(text: &str) -> String {
    let mut out = String::new();
    let mut last_was_space = false;
    for ch in text.trim().chars() {
        if is_zero_width_char(ch) {
            continue;
        }
        if ch.is_whitespace() {
            if !last_was_space && !out.is_empty() {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    out
}

pub(super) fn is_polish_unchanged(raw: &str, final_text: &str) -> bool {
    normalize_for_polish_compare(raw) == normalize_for_polish_compare(final_text)
}

pub(super) fn resolve_polish_unchanged_error_code(
    feature_enabled: bool,
    existing: Option<String>,
    mode: PolishMode,
    translation_active: bool,
    polish_error: &Option<String>,
    raw: &str,
    final_text: &str,
) -> Option<String> {
    if existing.is_some() {
        return existing;
    }
    if !feature_enabled
        || mode == PolishMode::Raw
        || translation_active
        || polish_error.is_some()
        || !is_polish_unchanged(raw, final_text)
    {
        return None;
    }
    Some(POLISH_UNCHANGED_ERROR_CODE.to_string())
}

/// 历史重新润色写回 `error_code` 时：插入失败等非润色错误保留；`None` /
/// `polishUnchanged` / `polishFailed` 由本次重试结果重新计算。
pub(super) fn is_repolish_mutable_error_code(code: Option<&str>) -> bool {
    matches!(code, None | Some(POLISH_UNCHANGED_ERROR_CODE) | Some("polishFailed"))
}

pub(super) fn resolve_repolish_history_error_code(
    feature_enabled: bool,
    existing: Option<String>,
    mode: PolishMode,
    translation_active: bool,
    polish_error: &Option<String>,
    raw: &str,
    final_text: &str,
) -> Option<String> {
    if let Some(code) = existing.as_deref().filter(|code| !is_repolish_mutable_error_code(Some(code)))
    {
        return Some(code.to_string());
    }
    resolve_polish_unchanged_error_code(
        feature_enabled,
        None,
        mode,
        translation_active,
        polish_error,
        raw,
        final_text,
    )
}

pub(super) fn dictation_error_code(
    status: InsertStatus,
    polish_failed: bool,
    focus_ready_for_paste: bool,
    allow_non_tsf_insertion_fallback: bool,
) -> Option<&'static str> {
    if !focus_ready_for_paste && status == InsertStatus::Failed {
        Some("focusRestoreFailed")
    } else if cfg!(target_os = "windows")
        && focus_ready_for_paste
        && !allow_non_tsf_insertion_fallback
        && status == InsertStatus::Failed
    {
        Some("windowsImeTsfRequired")
    } else if polish_failed {
        Some("polishFailed")
    } else {
        None
    }
}

pub(super) fn cancel_session(inner: &Arc<Inner>) {
    let Some(decision) = ({
        let mut state = inner.state.lock();
        let phase = state.phase;
        let decision = begin_cancel_session_state(&mut state);
        if phase == SessionPhase::Inserting {
            log::info!("[coord] cancel ignored — already in Inserting phase, can't undo paste");
        }
        decision
    }) else {
        return;
    };

    stop_recorder_for_session(inner, decision.session_id);
    cancel_asr_for_session(inner, decision.session_id);
    restore_prepared_windows_ime_session(inner, decision.session_id);
    // Processing 阶段保持 phase=Processing 让 end_session 自己走完检查 + 收尾；
    // 其他阶段直接转 Idle。
    if decision.phase != SessionPhase::Processing {
        let mut state = inner.state.lock();
        finish_cancel_session_state(&mut state, decision);
        // 只有真正把 phase 设为 Idle 时才设冷却（避免离场动画期间误激活）。
        let now = std::time::Instant::now();
        *inner.session_cooldown_until.lock() =
            Some(now + std::time::Duration::from_millis(POST_SESSION_COOLDOWN_MS));
    }
    emit_capsule(inner, CapsuleState::Cancelled, 0.0, 0, None, None);
    log::info!("[coord] session cancelled (was {:?})", decision.phase);
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
    // 取消时也熄灭整屏彩虹描边（dictation session 没开描边，hide 是无害 no-op）。
    if let Some(app) = inner.app.lock().clone() {
        crate::hide_less_computer_glow(&app);
    }
}

fn append_typed_prefix(target: &mut String, delta: &str, typed_chars: usize) -> usize {
    let mut end = 0;
    let mut appended = 0;
    for (idx, ch) in delta.char_indices().take(typed_chars) {
        end = idx + ch.len_utf8();
        appended += 1;
    }
    target.push_str(&delta[..end]);
    appended
}

fn eligible_polish_context_turns(
    sessions: Vec<DictationSession>,
    active_style_pack_id: &str,
    current_translation_active: bool,
) -> Vec<(String, String)> {
    sessions
        .into_iter()
        // 只取实际成功润色过的会话作为上下文：失败的会话 final_text 是 raw 兜底，
        // 喂回 LLM 会让模型以为"上一轮我什么都没做"——没意义且占 token。
        // 这条同时保证下面 filter_map 里翻译历史的 final_text 一定是真译文（而非 passthrough
        // 原文）——失败 / 兜底的翻译会话 error_code 非空，已在此被滤掉。
        .filter(|s| s.error_code.is_none() && !s.final_text.trim().is_empty())
        // 风格包切换 = 上下文边界。旧历史没有 style_pack_id，无法证明同源，保守排除。
        .filter(|s| s.style_pack_id.as_deref() == Some(active_style_pack_id))
        // 翻译历史按"下一轮是否也翻译"决定喂哪一段，既保留对话连续性又不让译文串味：
        //   - 当前是翻译轮 → 喂译文(final_text)，保持目标语言一致；
        //   - 当前是普通轮 → 喂润色后的源文(polish_source)，把译文剔除掉；源文缺失（解析
        //     失败 / 旧历史）则整条跳过——宁可少一条上下文，也不让外语译文混进普通润色。
        //   - 普通历史无论当前轮是什么，都喂 final_text（本就是源语言润色结果）。
        .filter_map(|s| {
            if s.translation_active && !current_translation_active {
                s.polish_source
                    .filter(|src| !src.trim().is_empty())
                    .map(|src| (s.raw_transcript, src))
            } else {
                Some((s.raw_transcript, s.final_text))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        append_typed_prefix, batch_asr_chunk_limit_ms, default_done_message,
        drain_streaming_insert_deltas_with, eligible_polish_context_turns, finalize_polished_text,
        flush_streaming_insert_buffer_with, is_polish_unchanged, normalize_for_polish_compare,
        resolve_polish_unchanged_error_code, resolve_repolish_history_error_code,
        streaming_insert_eligible, POLISH_UNCHANGED_ERROR_CODE,
    };
    use crate::types::{
        ChineseScriptPreference, CorrectionRule, DictationSession, InsertStatus, PolishMode,
    };

    fn correction_rule(pattern: &str, replacement: &str) -> CorrectionRule {
        CorrectionRule {
            id: "test".into(),
            pattern: pattern.into(),
            replacement: replacement.into(),
            enabled: true,
            created_at: String::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn history_session(
        id: &str,
        raw: &str,
        final_text: &str,
        style_pack_id: Option<&str>,
        translation_active: bool,
        polish_source: Option<&str>,
    ) -> DictationSession {
        DictationSession {
            id: id.into(),
            created_at: "2026-06-03T00:00:00Z".into(),
            raw_transcript: raw.into(),
            final_text: final_text.into(),
            mode: PolishMode::Structured,
            app_bundle_id: None,
            app_name: None,
            insert_status: InsertStatus::Inserted,
            error_code: None,
            duration_ms: Some(1000),
            dictionary_entry_count: None,
            has_audio_recording: None,
            style_pack_id: style_pack_id.map(str::to_string),
            translation_active,
            polish_source: polish_source.map(str::to_string),
        }
    }

    #[test]
    fn polish_context_resets_when_active_style_pack_changes() {
        let sessions = vec![
            history_session("new", "raw new", "final new", Some("pack.new"), false, None),
            history_session("old", "raw old", "final old", Some("pack.old"), false, None),
        ];

        let turns = eligible_polish_context_turns(sessions, "pack.new", false);

        assert_eq!(
            turns,
            vec![("raw new".to_string(), "final new".to_string())]
        );
    }

    #[test]
    fn normal_turn_uses_polished_source_of_translation_history_not_the_translation() {
        // 当前是普通润色轮：翻译历史喂"润色后的源文"，把译文剔除，避免外语污染。
        let sessions = vec![
            history_session(
                "translation",
                "你好",
                "Hello",
                Some("pack.new"),
                true,
                Some("你好。"),
            ),
            history_session("dictation", "继续", "继续。", Some("pack.new"), false, None),
        ];

        let turns = eligible_polish_context_turns(sessions, "pack.new", false);

        assert_eq!(
            turns,
            vec![
                ("你好".to_string(), "你好。".to_string()),
                ("继续".to_string(), "继续。".to_string()),
            ]
        );
    }

    #[test]
    fn normal_turn_skips_translation_history_without_polished_source() {
        // 译文历史没有 polish_source（解析失败 / 旧历史）→ 普通轮整条跳过，宁缺毋滥。
        let sessions = vec![
            history_session("translation", "你好", "Hello", Some("pack.new"), true, None),
            history_session("dictation", "继续", "继续。", Some("pack.new"), false, None),
        ];

        let turns = eligible_polish_context_turns(sessions, "pack.new", false);

        assert_eq!(turns, vec![("继续".to_string(), "继续。".to_string())]);
    }

    #[test]
    fn translation_turn_keeps_translation_text_of_translation_history() {
        // 当前还是翻译轮：翻译历史喂译文(final_text)，保持目标语言一致。
        let sessions = vec![history_session(
            "translation",
            "你好",
            "Hello",
            Some("pack.new"),
            true,
            Some("你好。"),
        )];

        let turns = eligible_polish_context_turns(sessions, "pack.new", true);

        assert_eq!(turns, vec![("你好".to_string(), "Hello".to_string())]);
    }

    #[test]
    fn translation_turn_uses_normal_history_final_text() {
        // 当前是翻译轮，普通历史照常喂 final_text（本就是源语言润色结果，不需要剔除）。
        let sessions = vec![history_session(
            "dictation",
            "继续",
            "继续。",
            Some("pack.new"),
            false,
            None,
        )];

        let turns = eligible_polish_context_turns(sessions, "pack.new", true);

        assert_eq!(turns, vec![("继续".to_string(), "继续。".to_string())]);
    }

    #[test]
    fn streamed_output_skips_postprocessing_mutations() {
        let rules = vec![correction_rule("Open AI", "OpenAI")];

        let result = finalize_polished_text(
            "Open AI".into(),
            false,
            false,
            PolishMode::Raw,
            &None,
            ChineseScriptPreference::Auto,
            &rules,
            true,
        );

        assert_eq!(result, "Open AI");
    }

    #[test]
    fn raw_llm_output_still_applies_script_preference() {
        let result = finalize_polished_text(
            "繁體".into(),
            false,
            true,
            PolishMode::Raw,
            &None,
            ChineseScriptPreference::Simplified,
            &[],
            false,
        );

        assert_eq!(result, "繁体");
    }

    #[test]
    fn non_streamed_output_still_applies_correction_rules() {
        let rules = vec![correction_rule("Open AI", "OpenAI")];

        let result = finalize_polished_text(
            "Open AI".into(),
            false,
            false,
            PolishMode::Raw,
            &None,
            ChineseScriptPreference::Auto,
            &rules,
            false,
        );

        assert_eq!(result, "OpenAI");
    }

    #[test]
    fn append_typed_prefix_keeps_unicode_char_boundaries() {
        let mut typed = String::from("前");

        let appended = append_typed_prefix(&mut typed, "a你🙂b", 3);

        assert_eq!(appended, 3);
        assert_eq!(typed, "前a你🙂");
    }

    #[test]
    fn append_typed_prefix_caps_at_delta_length() {
        let mut typed = String::new();

        let appended = append_typed_prefix(&mut typed, "好", 10);

        assert_eq!(appended, 1);
        assert_eq!(typed, "好");
    }

    #[test]
    fn streaming_insert_eligible_when_gates_allow() {
        assert!(streaming_insert_eligible(
            true,
            false,
            PolishMode::Light,
            false,
        ));
    }

    #[test]
    fn batch_asr_chunk_limit_applies_only_to_zhipu() {
        assert_eq!(batch_asr_chunk_limit_ms("zhipu"), Some(30_000));
        assert_eq!(batch_asr_chunk_limit_ms("openrouter"), Some(30_000));
        assert_eq!(batch_asr_chunk_limit_ms("whisper"), None);
        assert_eq!(batch_asr_chunk_limit_ms("siliconflow"), None);
        assert_eq!(batch_asr_chunk_limit_ms("groq"), None);
        assert_eq!(batch_asr_chunk_limit_ms("volcengine"), None);
    }

    #[test]
    fn default_done_message_works_correctly() {
        assert_eq!(
            default_done_message(InsertStatus::PasteSent, false),
            Some("已尝试粘贴".to_string())
        );
        assert_eq!(
            default_done_message(InsertStatus::Inserted, true),
            Some("润色失败，已插入原文".to_string())
        );
    }

    #[test]
    fn streaming_insert_batches_queued_deltas_before_flush() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send("你".to_string()).unwrap();
        tx.send("好".to_string()).unwrap();
        tx.send("🙂".to_string()).unwrap();
        drop(tx);

        let mut flushed = Vec::new();
        let (typed, failure) = drain_streaming_insert_deltas_with(
            rx,
            std::time::Duration::from_millis(50),
            |pending, typed_text| {
                flushed.push(pending.clone());
                typed_text.push_str(pending);
                pending.clear();
                None
            },
        );

        assert_eq!(flushed, vec!["你好🙂".to_string()]);
        assert_eq!(typed, "你好🙂");
        assert_eq!(failure, None);
    }

    #[test]
    fn flush_streaming_insert_buffer_keeps_partial_unicode_prefix() {
        let mut pending = "a你🙂b".to_string();
        let mut typed = String::new();

        let failure = flush_streaming_insert_buffer_with(&mut pending, &mut typed, |_| {
            Err(crate::unicode_keystroke::TypeError::Partial {
                typed_chars: 3,
                source: Box::new(platform_type_error()),
            })
        });

        assert_eq!(typed, "a你🙂");
        assert!(pending.is_empty());
        assert!(failure.is_some());
    }

    #[test]
    fn normalize_for_polish_compare_collapses_whitespace_and_zero_width() {
        assert_eq!(
            normalize_for_polish_compare("  hello\u{200B}  world \n"),
            "hello world"
        );
    }

    #[test]
    fn is_polish_unchanged_detects_equivalent_text() {
        assert!(is_polish_unchanged("你好世界", "你好世界"));
        assert!(is_polish_unchanged("  hello ", "hello"));
        assert!(!is_polish_unchanged("你好", "您好"));
    }

    #[test]
    fn resolve_polish_unchanged_respects_feature_toggle_and_priority() {
        assert_eq!(
            resolve_polish_unchanged_error_code(
                false,
                None,
                PolishMode::Structured,
                false,
                &None,
                "same",
                "same",
            ),
            None
        );
        assert_eq!(
            resolve_polish_unchanged_error_code(
                true,
                Some("polishFailed".into()),
                PolishMode::Structured,
                false,
                &None,
                "same",
                "same",
            ),
            Some("polishFailed".into())
        );
        assert_eq!(
            resolve_polish_unchanged_error_code(
                true,
                None,
                PolishMode::Raw,
                false,
                &None,
                "same",
                "same",
            ),
            None
        );
        assert_eq!(
            resolve_polish_unchanged_error_code(
                true,
                None,
                PolishMode::Structured,
                false,
                &None,
                "hello",
                "world",
            ),
            None
        );
        assert_eq!(
            resolve_polish_unchanged_error_code(
                true,
                None,
                PolishMode::Structured,
                false,
                &None,
                "same text",
                "same text",
            )
            .as_deref(),
            Some(POLISH_UNCHANGED_ERROR_CODE)
        );
    }

    #[test]
    fn resolve_repolish_history_preserves_insert_errors() {
        assert_eq!(
            resolve_repolish_history_error_code(
                true,
                Some("focusRestoreFailed".into()),
                PolishMode::Structured,
                false,
                &None,
                "same",
                "changed output",
            )
            .as_deref(),
            Some("focusRestoreFailed")
        );
        assert_eq!(
            resolve_repolish_history_error_code(
                true,
                Some("windowsImeTsfRequired".into()),
                PolishMode::Light,
                false,
                &None,
                "same",
                "changed output",
            )
            .as_deref(),
            Some("windowsImeTsfRequired")
        );
    }

    #[test]
    fn resolve_repolish_history_recalculates_polish_related_codes() {
        assert_eq!(
            resolve_repolish_history_error_code(
                true,
                Some(POLISH_UNCHANGED_ERROR_CODE.into()),
                PolishMode::Structured,
                false,
                &None,
                "same",
                "changed output",
            ),
            None
        );
        assert_eq!(
            resolve_repolish_history_error_code(
                true,
                Some("polishFailed".into()),
                PolishMode::Structured,
                false,
                &None,
                "same",
                "changed output",
            ),
            None
        );
        assert_eq!(
            resolve_repolish_history_error_code(
                true,
                Some(POLISH_UNCHANGED_ERROR_CODE.into()),
                PolishMode::Structured,
                false,
                &None,
                "same text",
                "same text",
            )
            .as_deref(),
            Some(POLISH_UNCHANGED_ERROR_CODE)
        );
    }

    #[test]
    fn resolve_repolish_history_rejects_translation_via_mutable_path() {
        assert_eq!(
            resolve_repolish_history_error_code(
                true,
                None,
                PolishMode::Structured,
                true,
                &None,
                "same",
                "same",
            ),
            None
        );
    }

    #[cfg(target_os = "macos")]
    fn platform_type_error() -> crate::unicode_keystroke::TypeError {
        crate::unicode_keystroke::TypeError::EventAllocFailed
    }

    #[cfg(target_os = "windows")]
    fn platform_type_error() -> crate::unicode_keystroke::TypeError {
        crate::unicode_keystroke::TypeError::SendInputFailed("fail".into())
    }

    #[cfg(target_os = "linux")]
    fn platform_type_error() -> crate::unicode_keystroke::TypeError {
        crate::unicode_keystroke::TypeError::EnigoText("fail".into())
    }

    #[cfg(target_os = "android")]
    fn platform_type_error() -> crate::unicode_keystroke::TypeError {
        crate::unicode_keystroke::TypeError::Unavailable
    }
}
