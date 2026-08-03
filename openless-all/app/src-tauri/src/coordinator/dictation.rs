use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::coordinator_state::{
    finish_cancelled_processing_state, request_stop_during_starting_state,
};
use crate::correction::apply_correction_rules;
use crate::types::HotkeyMode;

use super::qa::handle_qa_option_edge;
use super::resources::*;
use super::*;

/// 同一个 hotkey 边沿之间的最小间隔。低于此阈值的连按整体作为误触丢弃 ——
/// 避免微动开关回弹 / 用户手抖双击造成的空转写报错和 ASR session 抢资源。
const HOTKEY_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(250);
const MAX_PENDING_COMBO_PRESSES: usize = 64;
/// Auto 模式下区分「短按 = 切换式」与「长按 = 按住说话」的按住时长阈值。
/// 松手时若按住 < 此值判为短按（锁存，保持录音），>= 此值判为长按（松手即停）。
/// 时长以热键事件产生时携带的时间戳计算，避免串行 bridge 的排队延迟改变用户的物理按住时长。
/// 350ms 是「点一下 vs 明显按住」的自然分界。
const AUTO_HOLD_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(350);
/// modifier-only 触发键（Option / 右 Ctrl…）按下后的「组合键仲裁窗口」。
///
/// 按下这一刻还分不清用户是想说话，还是要打 Option+任意字母/数字键：修饰键的按下边沿两者完全一样。
/// 所以先等这么久再开会话——期间监听器若报告叠加了普通键，这次按下整条作废，麦克风
/// 不开、胶囊不闪、也不烧一次 ASR 建连。代价是听写起录晚这么多，取 150ms：足以覆盖
/// 绝大多数组合键的「修饰键→普通键」间隔，又低于人从按键到开口的反应时间（>250ms），
/// 不会吃掉首字。窗口没盖住的慢速组合键（按住 Option 半秒再按 Tab）由组合键撤销
/// 事后撤销兜底，见 handle_trigger_combined。
pub(super) const COMBO_ARBITRATION_GRACE: std::time::Duration =
    std::time::Duration::from_millis(150);
const STREAMING_INSERT_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(12);

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MacosKeylessDictationProvider {
    LocalQwen3,
    AppleSpeech,
}

#[cfg(target_os = "macos")]
fn macos_keyless_dictation_provider(active_asr: &str) -> Option<MacosKeylessDictationProvider> {
    if crate::asr::local::is_local_qwen3(active_asr) {
        Some(MacosKeylessDictationProvider::LocalQwen3)
    } else if crate::asr::local::is_apple_speech(active_asr) {
        Some(MacosKeylessDictationProvider::AppleSpeech)
    } else {
        None
    }
}

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
        log::info!("[less-computer] 审批已解析 approved={approved}");
    } else {
        log::info!("[less-computer] 审批请求已失效（超时/重复）");
    }
}

/// Less Computer 事件缓冲：浮窗首次创建时 webview 冷加载需要数百毫秒，此时后端
/// emit 的事件（尤其第一条 `user` —— 用户说出的那句话）会先于前端 listener 注册
/// 被丢弃，表现为「AI 在干活、但面板上没有我说的话」。这里按单调 seq 缓存当前
/// 会话的全部事件，前端 mount 后调 `less_computer_sync` 全量重放，实时流按 seq
/// 去重衔接。fresh=true 的 user 事件 = 新会话，清空重来（seq 不回卷，去重不混淆）。
/// 容量上限防极端长会话无界增长（超限丢最旧 —— 重放的意义在冷启动窗口，尾部足够）。
const LESS_COMPUTER_EVENT_LOG_CAP: usize = 2048;

struct LessComputerEventLog {
    next_seq: u64,
    events: std::collections::VecDeque<serde_json::Value>,
}

static LESS_COMPUTER_EVENT_LOG: std::sync::OnceLock<std::sync::Mutex<LessComputerEventLog>> =
    std::sync::OnceLock::new();

fn less_computer_event_log() -> &'static std::sync::Mutex<LessComputerEventLog> {
    LESS_COMPUTER_EVENT_LOG.get_or_init(|| {
        std::sync::Mutex::new(LessComputerEventLog {
            next_seq: 0,
            events: std::collections::VecDeque::new(),
        })
    })
}

/// 纯逻辑：给 payload 编 seq 并写入缓冲（fresh user 先清空，超限丢最旧）。
fn log_less_computer_event(log: &mut LessComputerEventLog, payload: &mut serde_json::Value) {
    let fresh_user = payload.get("kind").and_then(|k| k.as_str()) == Some("user")
        && payload.get("fresh").and_then(|f| f.as_bool()) == Some(true);
    if fresh_user {
        log.events.clear();
    }
    log.next_seq += 1;
    payload["seq"] = serde_json::json!(log.next_seq);
    log.events.push_back(payload.clone());
    while log.events.len() > LESS_COMPUTER_EVENT_LOG_CAP {
        log.events.pop_front();
    }
}

/// `less_computer_sync` 命令的数据源：当前会话已发生的事件（seq 升序）。
pub(crate) fn less_computer_event_backlog() -> Vec<serde_json::Value> {
    less_computer_event_log()
        .lock()
        .map(|log| log.events.iter().cloned().collect())
        .unwrap_or_default()
}

/// 往 Less Computer 浮窗发一条事件（macOS only；前端按 `kind` 渲染聊天结构）。
/// 每条事件先记入缓冲并带上 seq，再实时 emit —— 锁中毒时跳过缓冲照常 emit
/// （无 seq 事件前端无条件应用，退化为修复前行为而不是丢事件）。
fn emit_less_computer(inner: &Arc<Inner>, mut payload: serde_json::Value) {
    if let Ok(mut log) = less_computer_event_log().lock() {
        log_less_computer_event(&mut log, &mut payload);
    }
    if let Some(app) = inner.app.lock().clone() {
        let _ = app.emit_to("less-computer", LESS_COMPUTER_EVENT, payload);
    }
}

#[cfg(test)]
mod less_computer_event_log_tests {
    use super::{log_less_computer_event, LessComputerEventLog, LESS_COMPUTER_EVENT_LOG_CAP};

    fn new_log() -> LessComputerEventLog {
        LessComputerEventLog {
            next_seq: 0,
            events: std::collections::VecDeque::new(),
        }
    }

    #[test]
    fn assigns_monotonic_seq_and_clears_on_fresh_user() {
        let mut log = new_log();
        let mut e1 = serde_json::json!({"kind":"user","text":"第一句","fresh":true});
        let mut e2 = serde_json::json!({"kind":"delta","text":"好的"});
        log_less_computer_event(&mut log, &mut e1);
        log_less_computer_event(&mut log, &mut e2);
        assert_eq!(e1["seq"], 1);
        assert_eq!(e2["seq"], 2);
        assert_eq!(log.events.len(), 2);

        // fresh=true 开新会话：缓冲清空，seq 继续单调（前端按 seq 去重不回卷）。
        let mut e3 = serde_json::json!({"kind":"user","text":"新会话","fresh":true});
        log_less_computer_event(&mut log, &mut e3);
        assert_eq!(log.events.len(), 1);
        assert_eq!(e3["seq"], 3);

        // 追加轮次（fresh=false / 缺省）不清空。
        let mut e4 = serde_json::json!({"kind":"user","text":"追加","fresh":false});
        log_less_computer_event(&mut log, &mut e4);
        assert_eq!(log.events.len(), 2);
        assert_eq!(log.events.front().unwrap()["seq"], 3);
    }

    #[test]
    fn caps_backlog_dropping_oldest() {
        let mut log = new_log();
        for i in 0..(LESS_COMPUTER_EVENT_LOG_CAP + 5) {
            let mut e = serde_json::json!({"kind":"delta","text":i.to_string()});
            log_less_computer_event(&mut log, &mut e);
        }
        assert_eq!(log.events.len(), LESS_COMPUTER_EVENT_LOG_CAP);
        // 丢最旧：队首是第 6 条（seq 从 1 起）。
        assert_eq!(log.events.front().unwrap()["seq"], 6);
    }
}

#[cfg(test)]
mod less_computer_approval_log_tests {
    #[test]
    fn approval_capability_is_never_interpolated_into_logs() {
        let sources = [
            include_str!("dictation.rs"),
            include_str!("hotkey_loops.rs"),
            include_str!("../coordinator.rs"),
            include_str!("../commands/qa.rs"),
            include_str!("../lib.rs"),
        ];

        for source in sources {
            for statement in source.split("log::").skip(1) {
                let statement = statement
                    .split_once(");")
                    .map_or(statement, |(head, _)| head);
                if statement.contains("[less-computer]") {
                    assert!(
                        !statement.contains("token"),
                        "Less Computer approval logs must not contain the capability token: {statement}"
                    );
                }
            }
        }
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
/// **流式路径里的字形转换**：Simplified（t2s）在 `on_delta` 对每个 delta 就地转换
/// （近乎逐字映射，跨 delta 拆散词条也几乎总是正确）；Traditional（s2t）有真歧义，
/// `streaming_insert_eligible` 仍把它挡在一次性路径。`apply_correction_rules` 依旧
/// 不在流式路径里做 —— 字符已经落出去，不好回退。
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
    llm_call: &mut Option<crate::polish::LlmCallLabel>,
    llm_elapsed_ms: &mut Option<u64>,
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
            llm_call,
            llm_elapsed_ms,
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
                llm_call,
                llm_elapsed_ms,
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
    #[cfg(target_os = "windows")]
    let sendinput_options =
        windows_sendinput_options_from_prefs(&inner.prefs.get());
    let typer_handle = tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            drain_streaming_insert_deltas_with_sendinput_options(
                rx,
                STREAMING_INSERT_FLUSH_INTERVAL,
                sendinput_options,
            )
        }
        #[cfg(not(target_os = "windows"))]
        {
            drain_streaming_insert_deltas(rx, STREAMING_INSERT_FLUSH_INTERVAL)
        }
    });

    // 3. 调流式润色，on_delta 塞 mpsc；should_cancel 检查 dictation 取消旗。
    let inner_for_cancel = Arc::clone(inner);
    let should_cancel = move || inner_for_cancel.state.lock().cancelled;
    // Simplified 目标：对每个 delta 就地 t2s（转换器建一次，避免每个 delta 重新加载
    // 词典）。Traditional 不会走到这里（eligibility 已降级），Auto 无需转换。
    let delta_converter = (chinese_script_preference
        == crate::types::ChineseScriptPreference::Simplified)
        .then(|| {
            ferrous_opencc::OpenCC::from_config(ferrous_opencc::config::BuiltinConfig::T2s)
                .map_err(|e| {
                    log::warn!("[coord] streaming_insert: OpenCC t2s init failed, deltas stay unconverted: {e}");
                })
                .ok()
        })
        .flatten();
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
        llm_call,
        llm_elapsed_ms,
        move |delta: &str| {
            let converted = match delta_converter.as_ref() {
                Some(converter) => converter.convert(delta),
                None => delta.to_string(),
            };
            let _ = tx.send(converted);
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
                llm_call,
                llm_elapsed_ms,
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

#[cfg(target_os = "windows")]
pub(super) fn windows_sendinput_options_from_prefs(
    prefs: &crate::types::UserPreferences,
) -> crate::unicode_keystroke::WindowsSendInputOptions {
    crate::unicode_keystroke::WindowsSendInputOptions {
        newline_mode: prefs.windows_sendinput_newline_mode,
    }
}

#[cfg(target_os = "windows")]
fn windows_insertion_allows_streaming(mode: crate::types::WindowsInsertionMode) -> bool {
    mode == crate::types::WindowsInsertionMode::SendInput
}

#[cfg(not(target_os = "windows"))]
fn windows_insertion_allows_streaming(_mode: crate::types::WindowsInsertionMode) -> bool {
    true
}

fn drain_streaming_insert_deltas(
    rx: std::sync::mpsc::Receiver<String>,
    flush_interval: std::time::Duration,
) -> (String, Option<String>) {
    drain_streaming_insert_deltas_with(rx, flush_interval, flush_streaming_insert_buffer)
}

#[cfg(target_os = "windows")]
fn drain_streaming_insert_deltas_with_sendinput_options(
    rx: std::sync::mpsc::Receiver<String>,
    flush_interval: std::time::Duration,
    options: crate::unicode_keystroke::WindowsSendInputOptions,
) -> (String, Option<String>) {
    drain_streaming_insert_deltas_with(rx, flush_interval, move |pending, typed| {
        flush_streaming_insert_buffer_with_options(pending, typed, options)
    })
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

#[cfg(target_os = "windows")]
fn flush_streaming_insert_buffer_with_options(
    pending: &mut String,
    typed_text: &mut String,
    options: crate::unicode_keystroke::WindowsSendInputOptions,
) -> Option<String> {
    flush_streaming_insert_buffer_with(pending, typed_text, move |text| {
        crate::unicode_keystroke::type_unicode_chunk_with_options(text, options)
    })
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

fn finalize_polished_text(
    polished: String,
    translation_active: bool,
    _raw_uses_llm: bool,
    _mode: PolishMode,
    polish_error: &Option<String>,
    chinese_script_preference: crate::types::ChineseScriptPreference,
    correction_rules: &[crate::types::CorrectionRule],
    already_streamed: bool,
) -> String {
    if already_streamed {
        return polished;
    }
    let should_force_script = if translation_active {
        // 翻译路径目标可能是非中文（英/日/韩），OpenCC 会破坏它，故只在 polish 失败、
        // 回退到中文原文时才做字形转换。
        polish_error.is_some()
    } else {
        // 普通听写：始终按用户所选字形（简/繁）做确定性 OpenCC 转换。Auto 时
        // apply_chinese_script_preference 内部是 no-op，对默认用户零影响。
        // 不再只在 Raw / polish 失败时转——polish 模式靠 LLM 提示输出繁体并不可靠
        // （模型默认简体），导致繁中用户每次都拿到简体输出（issue #643）。
        true
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
    chinese_script_preference: crate::types::ChineseScriptPreference,
    windows_insertion_mode: crate::types::WindowsInsertionMode,
) -> bool {
    streaming_insert_enabled
        && !translation_active
        && (mode != PolishMode::Raw || raw_uses_llm)
        // 固定字形的 OpenCC 转换与流式的兼容性按方向区分：
        //   - Simplified（t2s）：近乎逐字映射，对每个 delta 就地转换即可（跨 delta
        //     边界拆散的词级条目退化为逐字转换，t2s 方向仍几乎总是正确），流式放行
        //     —— 否则固定简体的用户流式静默失效且无从得知原因。
        //   - Traditional（s2t）：一简对多繁有真歧义（发→發/髮），需要全文上下文，
        //     仍走一次性路径确保转换准确（issue #643）。
        && chinese_script_preference != crate::types::ChineseScriptPreference::Traditional
        && windows_insertion_allows_streaming(windows_insertion_mode)
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

pub(super) async fn handle_pressed_edge(
    inner: &Arc<Inner>,
    pressed_at: std::time::Instant,
    press_id: u64,
) {
    let was_held = inner.hotkey_trigger_held.swap(true, Ordering::SeqCst);
    if !was_held {
        // 先切换代次并清掉上一轮的会话标记，再做防抖。被防抖丢弃的按下也必须
        // 让后续组合键撤销事件归属于自己，不能继承上一轮的 true。
        inner
            .hotkey_press_generation
            .store(press_id, Ordering::SeqCst);
        inner
            .hotkey_press_began_session
            .store(0, Ordering::SeqCst);

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
            handle_pressed(inner, pressed_at, press_id).await;
        }
    }
}

/// 「排队接力」放行窗口（ms）。识别中按下热键想录下一条时,那个 Pressed 在处理期间就被缓进
/// hotkey channel,bridge 串行阻塞到本条会话收尾(Idle)才取出 —— 取出几乎在 Idle 后 0ms
/// (bridge 立即 recv)。它和 #545「会话结束后胶囊离场动画期间误触」的区别在于物理按下时刻:
/// 误触是会话结束后才按,处理时刻离 Idle 已隔人类反应时间(>150ms)。所以冷却期内、但距 Idle
/// < 该窗口的按下 = 排队接力,放行开录下一条;其余冷却期按下仍按 #545 拦截。120ms 远低于
/// 人类反应、又足够覆盖 end_session 收尾尾巴 + bridge recv 延迟。
const HOTKEY_QUEUE_GRACE_MS: u64 = 120;

/// 判断一次落在冷却期内的 Toggle 激活是否为「排队接力」按下（见 HOTKEY_QUEUE_GRACE_MS）。
/// since_idle = POST_SESSION_COOLDOWN_MS − 剩余冷却；落在 grace 窗口内即认为是处理期间缓进、
/// 收尾后立刻取出的接力按下。剩余冷却异常(> 全程,理论不会发生)时返回 false,从严不放行。
fn is_queued_chain_press(now: std::time::Instant, cooldown_until: std::time::Instant) -> bool {
    let cooldown = std::time::Duration::from_millis(POST_SESSION_COOLDOWN_MS);
    let remaining = cooldown_until.saturating_duration_since(now);
    cooldown
        .checked_sub(remaining)
        .map(|since_idle| since_idle < std::time::Duration::from_millis(HOTKEY_QUEUE_GRACE_MS))
        .unwrap_or(false)
}

pub(super) async fn handle_pressed(
    inner: &Arc<Inner>,
    pressed_at: std::time::Instant,
    press_id: u64,
) {
    let mode = inner.prefs.get().hotkey.mode;
    let phase = inner.state.lock().phase;
    log::info!("[coord] hotkey pressed (mode={mode:?}, phase={phase:?})");
    match (mode, phase) {
        (HotkeyMode::Toggle, SessionPhase::Idle) => {
            // 冷却检查：end_session 刚收尾时禁止短时间内再次激活，避免三连按第 3 次误触
            // （此时胶囊仍在离场动画周期内，issue #545）。例外「排队接力」：识别中按下想录
            // 下一条的 Pressed 被缓在 channel 里、会话收尾后立刻取出（距 Idle < grace），放行
            // 直接开录下一条（用户选的「安全版排队接力」）。
            let now = std::time::Instant::now();
            let cooldown_until = *inner.session_cooldown_until.lock();
            if let Some(deadline) = cooldown_until {
                if now < deadline {
                    if is_queued_chain_press(now, deadline) {
                        log::info!(
                            "[coord] queued-chain activation: 识别中按下，会话收尾后接力开录下一条"
                        );
                    } else {
                        log::info!(
                            "[coord] toggle activation blocked by cooldown (session still winding down)"
                        );
                        return;
                    }
                }
            }
            begin_session_from_press(inner, press_id).await;
        }
        (HotkeyMode::Toggle, SessionPhase::Listening) => {
            let _ = end_session(inner).await;
        }
        (HotkeyMode::Hold, SessionPhase::Idle) => {
            begin_session_from_press(inner, press_id).await;
        }
        // Toggle 模式 Starting 阶段第二次按 → 用户想停。
        // 不能直接 end_session（ASR session 还没建好），存边沿，握手完成后立即触发。
        (HotkeyMode::Toggle, SessionPhase::Starting) => {
            request_stop_during_starting(inner, "toggle stop edge");
        }
        // Auto 模式：按下即开录（与 Hold 一样不丢首字）。是短按还是长按要到松手时才知道，
        // 所以这里只负责「开始」并记下按下时刻，语义交给 handle_released 判定。
        (HotkeyMode::Auto, SessionPhase::Idle) => {
            // 复用 Toggle 的冷却 / 排队接力检查：#545 离场动画期间误触保护。
            let now = std::time::Instant::now();
            let cooldown_until = *inner.session_cooldown_until.lock();
            if let Some(deadline) = cooldown_until {
                if now < deadline {
                    if is_queued_chain_press(now, deadline) {
                        log::info!(
                            "[coord] queued-chain activation (auto): 识别中按下，会话收尾后接力开录下一条"
                        );
                    } else {
                        log::info!(
                            "[coord] auto activation blocked by cooldown (session still winding down)"
                        );
                        return;
                    }
                }
            }
            *inner.hotkey_press_at.lock() = Some(pressed_at);
            begin_session_from_press(inner, press_id).await;
        }
        // Auto 模式已因上一次「短按」锁存为切换态，再次按下 → 用户想停。
        (HotkeyMode::Auto, SessionPhase::Listening) => {
            let _ = end_session(inner).await;
        }
        // Auto 模式锁存后仍在 Starting 时第二次按 → 想停，同 Toggle 存边沿。
        (HotkeyMode::Auto, SessionPhase::Starting) => {
            request_stop_during_starting(inner, "auto stop edge");
        }
        _ => {}
    }
}

/// 由「这一次热键按下」开一条会话，并记下这个事实。组合键撤销只撤销带着这个
/// 标记的会话（见 handle_trigger_combined）。
///
/// 开录之前先过一遍组合键仲裁窗口：命中就当这次按下没发生过——不开麦、不弹胶囊。
async fn begin_session_from_press(inner: &Arc<Inner>, press_id: u64) {
    if press_resolves_to_combo(inner, press_id).await {
        // 按住态一并清掉：随后必然到来的 Released 会被 handle_released_edge 的
        // was_held 检查吞掉，不会走 Auto 短按锁存。
        inner.hotkey_trigger_held.store(false, Ordering::SeqCst);
        *inner.hotkey_press_at.lock() = None;
        *inner.last_hotkey_dispatch_at.lock() = None;
        return;
    }
    inner
        .hotkey_press_began_session
        .store(press_id, Ordering::SeqCst);
    // 组合键事件可能刚好在仲裁窗口结束、但在上面的标记写入前抵达；再检查一次，
    // 避免这种窄竞态把已判定为组合键的按下开成会话。
    if combo_seen_for_press(inner, press_id) {
        inner
            .hotkey_press_began_session
            .compare_exchange(press_id, 0, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
        inner.hotkey_trigger_held.store(false, Ordering::SeqCst);
        *inner.hotkey_press_at.lock() = None;
        *inner.last_hotkey_dispatch_at.lock() = None;
        return;
    }
    let _ = begin_session(inner).await;
    // 组合键撤销走独立通道，可能恰好在上面的仲裁检查之后、会话启动之前抵达。
    // 这种情况下撤销线程会留下 pending 标记，但在 phase=Idle 时无法取消；启动完成后
    // 必须再消费一次，否则这次组合键会把会话误启动出来。
    if inner.hotkey_press_generation.load(Ordering::SeqCst) == press_id
        && combo_seen_for_press(inner, press_id)
    {
        inner.hotkey_trigger_held.store(false, Ordering::SeqCst);
        *inner.hotkey_press_at.lock() = None;
        *inner.last_hotkey_dispatch_at.lock() = None;
        inner
            .hotkey_press_began_session
            .compare_exchange(press_id, 0, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
        cancel_combined_session_if_active(inner);
        return;
    }
    if inner.hotkey_press_generation.load(Ordering::SeqCst) == press_id
        && inner.state.lock().phase == SessionPhase::Idle
    {
        inner
            .hotkey_press_began_session
            .compare_exchange(press_id, 0, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
    }
}

/// 组合键仲裁：等 COMBO_ARBITRATION_GRACE，再问监听器这次按住有没有叠加普通键。
///
/// 只对 modifier-only 触发键等待 —— 自定义组合键（Cmd+Shift+D 之类）本身就没有歧义，
/// 让它白等这一下纯粹是掉延迟。等待放在防抖 / 冷却判定之后，那些判定用的仍是未被本
/// 窗口推迟的时刻（尤其别把「排队接力」窗口挤掉，见 is_queued_chain_press）。
async fn press_resolves_to_combo(inner: &Arc<Inner>, press_id: u64) -> bool {
    let binding = inner.prefs.get().dictation_hotkey;
    if crate::shortcut_binding::legacy_modifier_trigger(&binding).is_none() {
        return false;
    }
    tokio::time::sleep(COMBO_ARBITRATION_GRACE).await;
    let combined = combo_seen_for_press(inner, press_id);
    if combined {
        log::info!(
            "[coord] 触发键在 {}ms 仲裁窗口内叠加了其他键 —— 本次按下作废，不开录音",
            COMBO_ARBITRATION_GRACE.as_millis()
        );
    }
    combined
}

/// 触发键（modifier-only 热键）按住期间又按了普通键 —— 用户在打 Option+任意字母/数字键这类组合键，
/// 不是想说话。撤销这次按下：
///
/// 1. 清掉按住态。后面必然到来的 Released 会被 handle_released_edge 的 `was_held`
///    检查吞掉，不会再走 Hold 松手结束 / Auto 短按锁存那套判定 —— 否则 Auto 模式下
///    「Option+组合键快速松手」正是被判成短按锁存，录音一直开着停不下来。
/// 2. 只有这次按下真的开出了会话才取消它。按下时是 toggle 停止 / 被冷却拦下 /
///    路由给 QA 的，什么都不动（尤其不能取消正在转写的上一条）。
///
/// 组合键误触不算「刚用完一次听写」，所以顺带清掉冷却与防抖时间戳：否则紧接着那次
/// 真想说话的按下会被 #545 冷却 / 250ms 防抖静默吞掉，用户以为热键坏了。
///
/// 本函数跑在 `combo_abort_bridge_loop` 的独立线程上，与 Pressed/Released 那条串行
/// bridge 并发 —— 这正是它能在按下 Q 的那一帧就撤掉胶囊的原因，但也意味着不能再假定
/// 「Released 一定排在自己后面」。所以撤不撤销只看 `hotkey_press_began_session`
/// （每个 Pressed 边沿都会重置它，见 handle_pressed_edge），不看 `hotkey_trigger_held`：
/// 万一 Released 抢先跑完把按住态清了，撤销仍然认得出这条会话是自己那次按下开的。
/// 清 `hotkey_trigger_held` 只为吞掉后面的 Released，与撤销与否无关。
///
/// 另一个并发面是撤销落在 `begin_session` 还在 await 的中途 —— 由 begin_session 里
/// 既有的 `startup_race_status_for_starting` / `CancelRaced` 检查点接住（audit HIGH #1），
/// 与 Esc 取消同一条路径。
fn combo_seen_for_press(inner: &Arc<Inner>, press_id: u64) -> bool {
    // 自定义组合键和窗口回退路径没有 modifier-only 监听器，使用 0 表示没有代次。
    // pending 的初始值也是 0，不能让 compare_exchange(0, 0) 把每次自定义组合键误判为
    // 已发生组合撤销。
    if press_id == 0 {
        return false;
    }
    let pending = {
        let mut pending_presses = inner.hotkey_combo_pending_presses.lock();
        pending_presses
            .iter()
            .position(|pending_press| *pending_press == press_id)
            .and_then(|index| pending_presses.remove(index))
            .is_some()
    };
    let monitor_seen = inner
        .hotkey
        .lock()
        .as_ref()
        .is_some_and(|monitor| monitor.trigger_combined_since_press(press_id));
    pending || monitor_seen
}

pub(super) fn handle_trigger_combined(inner: &Arc<Inner>, press_id: u64) {
    if press_id == 0 {
        return;
    }
    // 先记下代次：combo 事件可能早于 Pressed 事件被协调器线程取出，仲裁窗口会
    // 在稍后消费这个待处理标记。若当前已进入下一代，则只记录旧事件，不能清掉
    // 新按下的 held 状态。
    {
        let mut pending_presses = inner.hotkey_combo_pending_presses.lock();
        if !pending_presses.contains(&press_id) {
            pending_presses.push_back(press_id);
            if pending_presses.len() > MAX_PENDING_COMBO_PRESSES {
                pending_presses.pop_front();
            }
        }
    }
    if inner.hotkey_press_generation.load(Ordering::SeqCst) != press_id {
        log::debug!("[coord] ignore stale combined hotkey press_id={press_id}");
        return;
    }
    inner.hotkey_trigger_held.store(false, Ordering::SeqCst);
    *inner.hotkey_press_at.lock() = None;
    let began_session = inner
        .hotkey_press_began_session
        .compare_exchange(press_id, 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok();
    if !began_session {
        log::info!("[coord] hotkey combined with another key (本次按下没开出会话，无需撤销)");
        return;
    }
    log::info!("[coord] hotkey combined with another key —— 取消本次按下开出的会话");
    cancel_combined_session_if_active(inner);
}

/// 只取消仍处于可取消阶段的本次会话。
///
/// 组合键通道独立于 Pressed/Released，事件可能在正常松手收尾、phase 已回到 Idle 后才被
/// 消费。此时不能清掉正常会话留下的冷却和防抖时间戳，否则会重新打开 #545 的三连按窗口。
/// 若会话尚未进入可取消阶段，pending 标记由 `begin_session_from_press` 的收尾检查消费，
/// 防止「撤销先到、开录后到」的竞态。
fn cancel_combined_session_if_active(inner: &Arc<Inner>) {
    if !cancel_session(inner) {
        return;
    }
    *inner.session_cooldown_until.lock() = None;
    *inner.last_hotkey_dispatch_at.lock() = None;
}

pub(super) async fn handle_released_edge(inner: &Arc<Inner>, released_at: std::time::Instant) {
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
        handle_released(inner, released_at).await;
    }
}

pub(super) async fn handle_released(inner: &Arc<Inner>, released_at: std::time::Instant) {
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
    if mode == HotkeyMode::Auto {
        // 使用物理按下/松开的事件时刻，避免 bridge 排队时把处理延迟误算为按住时长。
        let held_long = inner.hotkey_press_at.lock().take()
            .map(|pressed_at| released_at.saturating_duration_since(pressed_at) >= AUTO_HOLD_THRESHOLD)
            .unwrap_or(false);
        match phase {
            // 长按松手 = 按住说话，松手即停；短按 = 切换式，锁存保持录音，下次按下再停。
            SessionPhase::Listening if held_long => {
                let _ = end_session(inner).await;
            }
            // 仍在握手就松手，且判为长按 → 用户按住说话想停，存边沿握手完成后再 end。
            SessionPhase::Starting if held_long => {
                request_stop_during_starting(inner, "auto hold release edge");
            }
            SessionPhase::Listening | SessionPhase::Starting => {
                log::info!(
                    "[coord] auto short-tap latched (toggle semantics); next press stops"
                );
            }
            _ => {}
        }
    }
}

/// Less Computer 收尾：把转写当作指令交给无头 Claude，结果以胶囊展示（不插入到光标）。
/// pub(super)：除语音路径外，浮窗的打字输入（less_computer_submit_text 命令）
/// 也以文字直接进入同一条执行链（同样的护栏 / 审批 / 连续会话语义）。
pub(super) async fn run_voice_agent_transcript(
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
        Some("Agent 处理中…".to_string()),
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
    let provider =
        crate::coding_agent::CodingAgentProvider::from_pref(&prefs.coding_agent_provider);
    let model =
        crate::coding_agent::resolve_coding_agent_model(provider, prefs.coding_agent_model.clone());
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
    // 审批等待期间会话被取消（Esc）：把结果强制为 Cancelled，避免把第一轮拦截文本
    // 当 Done 收尾（cancelled 旗标已置、插入被跳过；胶囊/浮窗语义应一致显示「已取消」）。
    let final_outcome = if inner.state.lock().cancelled {
        LessComputerOutcome::Cancelled
    } else {
        final_outcome
    };

    {
        let mut state = inner.state.lock();
        state.phase = SessionPhase::Idle;
        state.focus_target = None; // 清除过期焦点目标，避免影响下次会话
    }
    // 工作结束：熄灭全屏彩虹描边（聊天浮窗保留，等用户读完/关闭）。
    if let Some(app) = inner.app.lock().clone() {
        crate::hide_less_computer_glow(&app);
    }

    match final_outcome {
        LessComputerOutcome::Done { text, cost_usd } => {
            let text = text.trim().to_string();
            if text.is_empty() {
                let msg = "Agent 无结果（确认已登录且额度充足）".to_string();
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
            schedule_capsule_idle(inner, CAPSULE_CANCEL_HIDE_DELAY_MS);
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
    use crate::coding_agent::CodingAgentProvider;

    let provider = CodingAgentProvider::from_pref(&inner.prefs.get().coding_agent_provider);
    // 可配置可执行文件：用户在「高级 → Less Computer」填了路径就用它，留空/空白按后端取默认
    // （claude / opencode）。trim 后为空视作未配置。
    let configured_exe: Option<String> = inner
        .prefs
        .get()
        .coding_agent_exe
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // 审批放行的高风险子串按「风险等价组」整组放行（如 --force / -f）：只放行被点那一个会让
    // 等价写法仍被拦。Claude / OpenCode 共用这组前缀。见 guard::risk_equivalent_patterns。
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
        // 不可安全批准的模式（提权/毁盘/系统级如 "sudo "、"dd if=" 等，deny_rule_for_pattern
        // 返回 None）在审批阶段保持拦截，不注入 allow 列表也不生成 OpenCode allow glob。
        .filter(|p| crate::coding_agent::guard::deny_rule_for_pattern(p).is_some())
        .collect();

    let mut req = crate::coding_agent::CodingAgentRequest::new("less-computer", prompt.to_string());
    req.cwd = cwd.map(|p| p.to_path_buf());
    req.model = model.map(|m| m.to_string());
    req.permission_mode = mode;
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

    // 护栏 + 运行器按 provider 分派。两条路径都 fail-closed：护栏配置生成失败一律中止，
    // 绝不在无护栏下裸跑。`settings_path` 仅 Claude 路径用临时文件（OpenCode 走 env 注入，
    // 无临时文件需清理）。
    let settings_path: Option<std::path::PathBuf>;
    let run = match provider {
        CodingAgentProvider::ClaudeCodeCli => {
            // 护栏 deny：默认全量；审批放行的模式从 deny 中剔除。
            let mut deny = crate::coding_agent::guard::default_deny_rules();
            // 只放行「可批准」的命令：deny_rule_for_pattern 返回该 pattern 在 default_deny_rules
            // 里的精确 deny 规则；提权/毁盘/系统级等不可安全表达的命令返回 None → 即使被批准也
            // 保持拦截（fail-closed），且不向 allow 注入畸形规则。
            let allow_rules: Vec<String> = approved_patterns
                .iter()
                .filter_map(|p| crate::coding_agent::guard::deny_rule_for_pattern(p))
                .map(|rule| rule.to_string())
                .collect();
            if !allow_rules.is_empty() {
                deny.retain(|d| !allow_rules.iter().any(|a| a == d));
            }
            let settings_json = serde_json::json!({
                "permissions": { "defaultMode": mode.as_cli_arg(), "deny": deny }
            });
            let path = std::env::temp_dir().join(format!(
                "openless-less-computer-guard-{}.json",
                uuid::Uuid::new_v4()
            ));
            // fail-closed：序列化或写入失败时立即中止，绝不把无效路径交给 `claude -p --settings`
            //（找不到文件 = 完全裸跑）。宁可不跑也不裸跑。
            let settings_bytes = match serde_json::to_vec_pretty(&settings_json) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("[less-computer] 序列化护栏配置失败: {e}");
                    return LessComputerOutcome::Failed {
                        message: "护栏配置写入失败，已中止（拒绝在无护栏下执行）".into(),
                    };
                }
            };
            if let Err(e) = std::fs::write(&path, settings_bytes) {
                log::warn!("[less-computer] 写护栏配置失败: {e}");
                return LessComputerOutcome::Failed {
                    message: "护栏配置写入失败，已中止（拒绝在无护栏下执行）".into(),
                };
            }
            settings_path = Some(path.clone());
            req.settings_json_path = Some(path);
            // 去掉 WebFetch：无出站白名单时它是 prompt 注入 SSRF 面。保留 WebSearch（走搜索引擎）。
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
            let exe = configured_exe.unwrap_or_else(|| "claude".to_string());
            async_runtime::spawn(async move {
                crate::coding_agent::run_claude_agent(&exe, req, tx, cancel_for_runner).await
            })
        }
        CodingAgentProvider::OpenCodeCli => {
            // OpenCode 无 `--settings`，护栏走 `permission` 配置经 OPENCODE_CONFIG_CONTENT 注入。
            // build_opencode_guard_config 默认 bash deny 高风险前缀、webfetch deny，审批放行的
            // 前缀显式 allow。fail-closed：序列化失败立即中止，绝不无护栏裸跑。
            let guard =
                crate::coding_agent::guard::build_opencode_guard_config(&approved_patterns);
            let guard_str = match serde_json::to_string(&guard) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("[less-computer] 序列化 OpenCode 护栏配置失败: {e}");
                    return LessComputerOutcome::Failed {
                        message: "护栏配置写入失败，已中止（拒绝在无护栏下执行）".into(),
                    };
                }
            };
            settings_path = None;
            let exe = configured_exe.unwrap_or_else(|| "opencode".to_string());
            async_runtime::spawn(async move {
                crate::coding_agent::run_opencode_agent(
                    &exe,
                    req,
                    Some(guard_str),
                    tx,
                    cancel_for_runner,
                )
                .await
            })
        }
    };
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
            E::Compaction { .. } => {
                emit_less_computer(inner, serde_json::json!({ "kind": "compaction" }));
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
    // 仅 Claude 路径有临时护栏文件需清理；OpenCode 走 env 注入无文件。
    if let Some(path) = &settings_path {
        let _ = std::fs::remove_file(path);
    }

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
            .unwrap_or_else(|| "Agent 无结果（确认已登录且额度充足）".to_string());
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

    // 等用户点 Approve/Deny；90s 无响应按 Deny 处理并清理注册表项。会话被取消
    // （Esc → esc-cancel-bridge → cancel_session 置 cancelled，PR #855 场景）时
    // 同样按 Deny 处理并清理——否则审批挂起期间 Esc 被独占吞掉却毫无效果。
    let approved = tokio::select! {
        v = rx => v.unwrap_or(false),
        _ = tokio::time::sleep(std::time::Duration::from_secs(90)) => {
            less_computer_approvals()
                .lock()
                .ok()
                .map(|mut m| m.remove(&token));
            false
        }
        _ = wait_for_processing_cancel(inner) => {
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
    begin_session_as(inner, false).await
}

/// begin_session 的带参版本，voice_agent=true 时在 Starting 阶段就标记好，
/// 防止 finish_starting_session 处理 pending_stop 时丢失标志。
pub(super) async fn begin_session_as(
    inner: &Arc<Inner>,
    voice_agent: bool,
) -> Result<(), String> {
    let current_session_id = {
        let mut state = inner.state.lock();
        let Some(session_id) =
            begin_session_state(&mut state, capture_focus_target(), capture_frontmost_app())
        else {
            return Ok(());
        };
        if voice_agent {
            state.voice_agent = true;
        }
        if let Some(label) = state.front_app.as_deref() {
            log::info!("[coord] front_app captured: {label}");
        }
        session_id
    };
    #[cfg(target_os = "windows")]
    {
        if inner.prefs.get().windows_insertion_mode == crate::types::WindowsInsertionMode::Tsf {
            let prepared = inner.windows_ime.prepare_session();
            let mut slots = inner.prepared_windows_ime_session.lock();
            store_prepared_windows_ime_session(&mut slots, current_session_id, prepared);
        }
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

    // 乐观显示：按下热键即弹出胶囊并播入场动画，不等麦克风/ASR。此刻麦克风还在 cpal
    // init 窗口内、没有第一帧 PCM，先进「预备态」（warming=true → 前端渲染待命光效，引导
    // 用户稍候再开口）；level_handler 首次触发（PCM 真的流入）后翻成正式录音态、光条点亮。
    // 这样把「视觉反馈」与「麦克风就绪」解耦：即时反馈 + 完整入场动画，同时用预备→点亮的
    // 过渡守住「不漏首字」。若随后凭证/权限校验失败，下面分支会用 Error 覆盖这一帧。
    inner.capsule_warming.store(true, Ordering::SeqCst);
    emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);

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
    let asr_model = CredentialsVault::get(CredentialAccount::AsrModel)
        .ok()
        .flatten()
        .unwrap_or_default();
    let effective_asr = match resolve_effective_asr_provider(&active_asr, &asr_model) {
        Ok(provider) => provider,
        Err(message) => {
            log::warn!("[coord] ASR model routing rejected: {message}");
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
    };

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
            model_alias.clone(),
            prefs.foundry_local_runtime_source.clone(),
            language_hint,
        ));
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::FoundryLocalWhisper(Arc::clone(&local)),
            AsrCallLabel::new(foundry::PROVIDER_ID, Some(model_alias)),
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
            model_alias.clone(),
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
            AsrCallLabel::new(sherpa::PROVIDER_ID, Some(model_alias)),
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    if let Some(provider) = macos_keyless_dictation_provider(&active_asr) {
        match provider {
            MacosKeylessDictationProvider::LocalQwen3 => {
                let (local, local_model) = match build_local_qwen3(inner).await {
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
                    AsrCallLabel::new(crate::asr::local::PROVIDER_ID, Some(local_model)),
                );
                let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
                start_recorder_and_enter_listening(
                    inner,
                    current_session_id,
                    &active_asr,
                    consumer,
                )
                .await?;
            }
            MacosKeylessDictationProvider::AppleSpeech => {
                let local = build_apple_speech(&inner.prefs.get());
                store_asr_for_session(
                    inner,
                    current_session_id,
                    ActiveAsr::AppleSpeech(Arc::clone(&local)),
                    // 系统语音识别没有用户可见的模型 id。
                    AsrCallLabel::new(crate::asr::local::APPLE_SPEECH_PROVIDER_ID, None),
                );
                let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
                start_recorder_and_enter_listening(
                    inner,
                    current_session_id,
                    &active_asr,
                    consumer,
                )
                .await?;
            }
        }
        return Ok(());
    }

    // 统一百炼:按所选模型把 build 分发重定向到具体协议 id（凭据仍读真实 active
    // `bailian` 的那把 key；endpoint 由前端按模型同步）。别名 id 原样返回,走旧路径。
    // 编译期护栏（exhaustiveness tripwire）：下面这条云端构建 if-else 链最后是
    // `else` 静默落到火山。这个穷尽的空 match 本身不做事，但新增
    // ActiveAsrProviderKind 时会在此编译失败，逼作者回来给新 kind 补一条构建分支
    // ——把「装完才发现漏了」的运行期坑变成编译期错误。QA 侧的 build_qa_asr_start
    // 已是穷尽 match，两条构建路径都受编译器保护。
    match active_asr_provider_kind(&effective_asr) {
        ActiveAsrProviderKind::Bailian
        | ActiveAsrProviderKind::Qwen3Realtime
        | ActiveAsrProviderKind::StepfunRealtime
        | ActiveAsrProviderKind::Mimo
        | ActiveAsrProviderKind::DashScopeMultimodal
        | ActiveAsrProviderKind::ElevenLabs
        | ActiveAsrProviderKind::WhisperCompatible
        | ActiveAsrProviderKind::Volcengine => {}
    }

    if is_bailian_provider(&effective_asr) {
        let creds = read_bailian_credentials();
        let asr_call_label = AsrCallLabel::new(effective_asr.clone(), Some(creds.model.clone()));
        let asr = Arc::new(BailianRealtimeASR::new(creds));
        let bridge = Arc::new(DeferredAsrBridge::new());
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = bridge.clone();
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Bailian(Arc::clone(&asr)),
            asr_call_label,
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
    } else if is_qwen3_realtime_provider(&effective_asr) {
        // 与 Bailian 分支同构：流式 WS 会话 + DeferredAsrBridge 缓冲开链前音频。
        let creds = read_qwen3_realtime_credentials();
        let asr_call_label = AsrCallLabel::new(effective_asr.clone(), Some(creds.model.clone()));
        let asr = Arc::new(Qwen3RealtimeASR::new(creds));
        let bridge = Arc::new(DeferredAsrBridge::new());
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = bridge.clone();
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Qwen3Realtime(Arc::clone(&asr)),
            asr_call_label,
        );
        start_recorder_for_starting(inner, current_session_id, &active_asr, consumer).await?;

        if let Err(e) = asr.open_session().await {
            log::error!("[coord] open Qwen3 realtime ASR session failed: {e}");
            match startup_race_status_for_starting(inner, current_session_id) {
                StartupRaceStatus::StaleContinuation => {
                    log::info!(
                        "[coord] stale Qwen3 realtime ASR open_session error from session {current_session_id} — ignoring"
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
                log::info!(
                    "[coord] cancel raced during Qwen3 realtime ASR open_session — aborting begin"
                );
                asr.cancel();
                discard_startup_resources_for_session(inner, current_session_id);
                restore_prepared_windows_ime_session(inner, current_session_id);
                set_phase_idle_if_session_matches(inner, current_session_id);
                return Ok(());
            }
            StartupRaceStatus::StaleContinuation => {
                log::info!(
                    "[coord] stale Qwen3 realtime ASR open_session continuation from session {current_session_id} — ignoring"
                );
                asr.cancel();
                discard_startup_resources_for_session(inner, current_session_id);
                restore_prepared_windows_ime_session(inner, current_session_id);
                return Ok(());
            }
        }
        let target: Arc<dyn crate::asr::AudioConsumer> = asr;
        let flushed_bytes = bridge.attach(target);
        log::info!(
            "[coord] Qwen3 realtime ASR connected; flushed {flushed_bytes} deferred audio bytes"
        );
        finish_starting_session(inner, current_session_id).await;
    } else if is_stepfun_realtime_provider(&effective_asr) {
        // 与 Qwen3 realtime 分支同构：流式 WS 会话 + DeferredAsrBridge 缓冲开链前音频。
        // 实时协议的词汇偏置走 transcription.prompt（批式 stepfun 则相反走 hotwords）。
        let prompt = crate::asr::whisper::build_prompt_from_phrases(&enabled_phrases(inner));
        let creds = read_stepfun_realtime_credentials(prompt);
        let asr_call_label = AsrCallLabel::new(effective_asr.clone(), Some(creds.model.clone()));
        let asr = Arc::new(crate::asr::StepfunRealtimeASR::new(creds));
        let bridge = Arc::new(DeferredAsrBridge::new());
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = bridge.clone();
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::StepfunRealtime(Arc::clone(&asr)),
            asr_call_label,
        );
        start_recorder_for_starting(inner, current_session_id, &active_asr, consumer).await?;

        if let Err(e) = asr.open_session().await {
            log::error!("[coord] open StepFun realtime ASR session failed: {e}");
            match startup_race_status_for_starting(inner, current_session_id) {
                StartupRaceStatus::StaleContinuation => {
                    log::info!(
                        "[coord] stale StepFun realtime ASR open_session error from session {current_session_id} — ignoring"
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
                log::info!(
                    "[coord] cancel raced during StepFun realtime ASR open_session — aborting begin"
                );
                asr.cancel();
                discard_startup_resources_for_session(inner, current_session_id);
                restore_prepared_windows_ime_session(inner, current_session_id);
                set_phase_idle_if_session_matches(inner, current_session_id);
                return Ok(());
            }
            StartupRaceStatus::StaleContinuation => {
                log::info!(
                    "[coord] stale StepFun realtime ASR open_session continuation from session {current_session_id} — ignoring"
                );
                asr.cancel();
                discard_startup_resources_for_session(inner, current_session_id);
                restore_prepared_windows_ime_session(inner, current_session_id);
                return Ok(());
            }
        }
        let target: Arc<dyn crate::asr::AudioConsumer> = asr;
        let flushed_bytes = bridge.attach(target);
        log::info!(
            "[coord] StepFun realtime ASR connected; flushed {flushed_bytes} deferred audio bytes"
        );
        finish_starting_session(inner, current_session_id).await;
    } else if is_mimo_provider(&effective_asr) {
        let (api_key, base_url, model) = read_mimo_credentials();
        let asr_call_label = AsrCallLabel::new(effective_asr.clone(), Some(model.clone()));
        let mimo = Arc::new(MimoBatchASR::new(api_key, base_url, model));
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Mimo(Arc::clone(&mimo)),
            asr_call_label,
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = mimo;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
    } else if is_dashscope_multimodal_provider(&effective_asr) {
        let (api_key, base_url, model) = read_dashscope_multimodal_credentials();
        let asr_call_label = AsrCallLabel::new(effective_asr.clone(), Some(model.clone()));
        let asr = Arc::new(DashScopeMultimodalASR::new(api_key, base_url, model));
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::DashScopeMultimodal(Arc::clone(&asr)),
            asr_call_label,
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = asr;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
    } else if is_elevenlabs_provider(&effective_asr) {
        let (api_key, base_url, model) = read_elevenlabs_credentials();
        let asr_call_label = AsrCallLabel::new(effective_asr.clone(), Some(model.clone()));
        let asr = Arc::new(ElevenLabsBatchASR::new(api_key, base_url, model));
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::ElevenLabs(Arc::clone(&asr)),
            asr_call_label,
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = asr;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
    } else if is_whisper_compatible_provider(&effective_asr) {
        let (api_key, base_url, model) = read_whisper_credentials();
        // 用户辞書の有効フレーズを Whisper の `prompt` に流し込む。固有名詞や
        // 専門用語の同音・近形誤認識を ASR 段階で抑える。Polish LLM 側には
        // 既に system prompt として注入済みだが、Whisper 出力が大きく崩れる
        // と Polish でも救えない（特に CJK で顕著）。Volcengine ASR は元々
        // hotword を受け取っており、UI 説明文も「ASR ホットワードと後処理
        // モデルのコンテキスト両方に渡される」と明示しているので、Whisper
        // 互換プロバイダにも揃えるのが筋。
        let (whisper_prompt, hotwords) =
            whisper_vocab_for_provider(&active_asr, enabled_phrases(inner));
        let asr_call_label = AsrCallLabel::new(effective_asr.clone(), Some(model.clone()));
        let whisper = Arc::new(
            WhisperBatchASR::new(
                api_key,
                base_url,
                model,
                whisper_prompt,
                batch_asr_chunk_limit_ms(&active_asr),
                whisper_supports_verbose_json(&active_asr),
            )
            .with_request_format(whisper_request_format(&active_asr))
            .with_hotwords(hotwords),
        );
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Whisper(Arc::clone(&whisper)),
            asr_call_label,
        );
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = whisper;
        start_recorder_and_enter_listening(inner, current_session_id, &active_asr, consumer)
            .await?;
    } else {
        let hotwords = enabled_hotwords(inner);
        let creds = read_volc_credentials();
        // Volcengine 没有模型 id，但 resource id（volc.seedasr.* / volc.bigasr.*）承担同样的
        // 「用的哪个引擎」角色；经 allowlist 脱敏后当 model 落历史（issue #373 排障场景）。
        let asr_call_label = AsrCallLabel::new(
            effective_asr.clone(),
            volc_resource_history_label(&creds.resource_id),
        );
        let asr = Arc::new(VolcengineStreamingASR::new(creds, hotwords));
        let bridge = Arc::new(DeferredAsrBridge::new());
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = bridge.clone();
        store_asr_for_session(
            inner,
            current_session_id,
            ActiveAsr::Volcengine(Arc::clone(&asr)),
            asr_call_label,
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
        // 第一帧 PCM 真的流到 consumer 了（recorder.rs::process_callback 的顺序保证
        // consume_pcm_chunk 先于 level_handler）——关掉预备态，让这一帧起 payload.warming
        // 翻 false，前端把「待命」光条点亮成正式录音态。之后每帧都是 false（幂等）。
        inner_for_level.capsule_warming.store(false, Ordering::SeqCst);
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
    // 总是把这次口述归档成 `recordings/<session_id>.wav`，不再只在 record_audio_for_debug
    // 下归档。原因：失败保留 + 自动重试需要原始音频，而该开关默认 false——之前转录失败时音频
    // 直接丢失（用户反馈「识别失败，之前的语音也都丢失了」）。归档是临时的：拿到非空转写后，
    // 若用户没开 record_audio_for_debug 就立刻删掉（隐私——成功的口述不留痕），只有「转录失败」
    // 的录音会留下，供历史里手动「重新转录」或自动静默重试复用。prune_recordings 兜底总量。
    // 文件名用 coordinator 的 SessionId，跟 history 那条记录 id 对齐（见下游 polish 收尾
    // `history_session_id = current_session_id.to_string()`），前端凭 id 就能找到录音。
    let audio_archive_path = {
        let prefs = inner.prefs.get();
        let _ = crate::persistence::prune_recordings(
            prefs.history_retention_days,
            prefs.audio_recording_max_entries,
        );
        crate::persistence::recording_path_for_session(&session_id.to_string()).ok()
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

/// 转录失败时落一条「转录失败」历史，并保留这次的原始录音，让用户能在历史里看到失败、
/// 手动「重新转录」。复活并修好 issue #613：之前失败的录音被孤立——历史里看不到这条、
/// 音频也找不回（孤儿 wav 最终被 prune 清掉，语音彻底丢失）。
///
/// session_id 与归档 wav 同名（`recordings/<session_id>.wav`），保证 read_audio_recording /
/// retranscribe_recording 凭 id 能定位文件。has_audio_recording 读 Recorder::start 的实际
/// 写盘状态（不是 prefs 开关）：开关想录但路径创建失败时为 false，避免前端渲染播放/重转
/// 按钮而后端 404。
fn build_transcribe_failed_session(
    session_id: SessionId,
    duration_ms: u64,
    asr_ms: u64,
    mode: PolishMode,
    has_audio_recording: bool,
) -> DictationSession {
    DictationSession {
        id: session_id.to_string(),
        created_at: Utc::now().to_rfc3339(),
        source: crate::types::HistorySource::Voice,
        raw_transcript: String::new(),
        final_text: String::new(),
        mode,
        style_pack_id: None,
        translation_active: false,
        polish_source: None,
        app_bundle_id: None,
        app_name: None,
        insert_status: InsertStatus::Failed,
        error_code: Some("transcribeFailed".to_string()),
        duration_ms: Some(duration_ms),
        dictionary_entry_count: None,
        has_audio_recording: Some(has_audio_recording),
        asr_provider: None,
        asr_model: None,
        llm_provider: None,
        llm_model: None,
        asr_ms: Some(asr_ms),
        polish_ms: None,
    }
}

fn write_transcribe_failed_history(
    inner: &Arc<Inner>,
    session_id: SessionId,
    duration_ms: u64,
    asr_ms: u64,
    asr_call_label: Option<&AsrCallLabel>,
) {
    let prefs = inner.prefs.get();
    let mut session = build_transcribe_failed_session(
        session_id,
        duration_ms,
        asr_ms,
        prefs.default_mode,
        inner.audio_archive_active.load(Ordering::Relaxed),
    );
    // 失败条目也记下是哪个 ASR 出的错——「哪个模型转不出来」正是模型对比要看的信息。
    // 用 begin_session 的构建时快照，而不是此刻重读设置（PR #826 review）。
    if let Some(label) = asr_call_label {
        session.asr_provider = Some(label.provider.clone());
        session.asr_model = label.model.clone();
    }
    if let Err(e) = inner.history.append_with_retention(
        session,
        prefs.history_retention_days,
        prefs.history_max_entries,
    ) {
        log::error!("[coord] transcribeFailed history append failed: {e}");
    }
}

/// ASR 转录失败 / 超时的统一收尾，替代之前散落在每个引擎分支里重复 5 行的失败尾巴：
/// 保留录音 + 落失败历史 → 错误胶囊 → 恢复窗口/IME → 回 Idle → 定时隐藏胶囊。
/// 永远返回 `Err(err)`，调用方写 `return fail_dictation(...)`。集中一处既保证没有任何引擎
/// 分支漏掉「失败保留」，也是自动静默重试彻底失败后的唯一收尾点。
fn fail_dictation(
    inner: &Arc<Inner>,
    session_id: SessionId,
    elapsed: u64,
    asr_ms: u64,
    user_msg: String,
    err: String,
    asr_call_label: Option<&AsrCallLabel>,
) -> Result<(), String> {
    write_transcribe_failed_history(inner, session_id, elapsed, asr_ms, asr_call_label);
    emit_capsule(
        inner,
        CapsuleState::Error,
        0.0,
        elapsed,
        Some(user_msg),
        None,
    );
    restore_prepared_windows_ime_session(inner, session_id);
    inner.state.lock().phase = SessionPhase::Idle;
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
    Err(err)
}

/// ASR 失败/超时分支从引擎 match 里产出的「失败」值：带用户提示文案 + 内部错误串，交给
/// match 之后的统一处理（先自动重试，彻底失败再 fail_dictation 收尾）。
struct TranscribeFail {
    user_msg: String,
    err: String,
}

impl TranscribeFail {
    fn new(user_msg: String, err: String) -> Self {
        Self { user_msg, err }
    }
}

/// 自动静默重试的最大次数（不含首次转写）。失败/超时多为网络或服务端瞬时抖动，重试几次
/// 往往就能拿回这段语音；上限避免在永久性故障（如鉴权失败）上空耗太久。
const SILENT_RETRY_MAX: u32 = 2;
/// 每次重试前的线性退避基数：第 N 次重试前等 `SILENT_RETRY_BACKOFF_MS * N` 毫秒，给抖动的
/// 网络/服务端一点缓冲再打。
const SILENT_RETRY_BACKOFF_MS: u64 = 500;

enum SilentRetryOutcome {
    Transcript {
        raw: RawTranscript,
        asr_call_label: AsrCallLabel,
    },
    Exhausted(Option<AsrCallLabel>),
    Cancelled,
}

fn accept_silent_retry_transcript(
    raw: RawTranscript,
    retry_label: AsrCallLabel,
    asr_call_label: &mut Option<AsrCallLabel>,
) -> RawTranscript {
    *asr_call_label = Some(retry_label);
    raw
}

/// 归档 wav 是 16k/mono/16-bit、固定 44 字节标准头（asr::wav::encode_wav_16k_mono）；取出
/// PCM 负载。长度 <= 44（空/损坏）返回 None。
fn pcm_from_wav_bytes(wav: &[u8]) -> Option<Vec<u8>> {
    if wav.len() <= 44 {
        return None;
    }
    Some(wav[44..].to_vec())
}

/// 16k/mono/16-bit PCM：每毫秒 32 字节（16000 * 2 / 1000）。用 PCM 长度反推时长，给重试成功
/// 后的 RawTranscript.duration_ms（写历史 / 胶囊用）。
fn pcm_duration_ms(pcm_len: usize) -> u64 {
    (pcm_len as u64) / 32
}

/// 用「当前」provider 把一段 PCM 重新转录（建一条全新 ASR 会话——原会话失败/断开后不可
/// 复用）。复用 Coordinator::retranscribe_pcm（历史「重新转录」同款逻辑）；Coordinator 只持有
/// `inner`，这里用 inner 重建一个轻量句柄，零副作用。
async fn retranscribe_pcm_via_inner(
    inner: &Arc<Inner>,
    pcm: Vec<u8>,
) -> (Result<String, String>, Option<AsrCallLabel>) {
    Coordinator {
        inner: Arc::clone(inner),
    }
    .retranscribe_pcm_until_cancelled(pcm)
    .await
}

/// 自动静默重试：从刚归档的 wav 读 PCM，用当前 provider 重转最多 SILENT_RETRY_MAX 次（线性
/// 退避）。任一次拿到非空文本立即返回 Transcript（当作正常转写继续走润色/插入）；没有归档
/// 音频、读不到或全部失败返回 Exhausted（交回 fail_dictation 做「失败保留 + 报错」）。如果
/// 用户在退避或重试请求期间按 Esc，则返回 Cancelled，直接完成取消收尾。全程不改胶囊文案——
/// 对用户静默，只是「转写中」多停留一会儿。
async fn try_silent_retranscribe(inner: &Arc<Inner>, session_id: SessionId) -> SilentRetryOutcome {
    if inner.state.lock().cancelled {
        return SilentRetryOutcome::Cancelled;
    }
    if !inner.audio_archive_active.load(Ordering::Relaxed) {
        return SilentRetryOutcome::Exhausted(None); // 没归档音频，无从重试
    }
    let Some(path) = crate::persistence::recording_path_for_session(&session_id.to_string()).ok()
    else {
        return SilentRetryOutcome::Exhausted(None);
    };
    let wav = tokio::select! {
        biased;
        _ = wait_for_processing_cancel(inner) => return SilentRetryOutcome::Cancelled,
        result = tokio::fs::read(&path) => match result {
            Ok(wav) => wav,
            Err(_) => return SilentRetryOutcome::Exhausted(None),
        },
    };
    let Some(pcm) = pcm_from_wav_bytes(&wav) else {
        return SilentRetryOutcome::Exhausted(None);
    };
    let duration_ms = pcm_duration_ms(pcm.len());
    let mut last_attempted_label = None;
    for attempt in 1..=SILENT_RETRY_MAX {
        tokio::select! {
            biased;
            _ = wait_for_processing_cancel(inner) => return SilentRetryOutcome::Cancelled,
            _ = tokio::time::sleep(std::time::Duration::from_millis(
                SILENT_RETRY_BACKOFF_MS * attempt as u64,
            )) => {}
        }
        let (result, attempted_label) = tokio::select! {
            biased;
            _ = wait_for_processing_cancel(inner) => return SilentRetryOutcome::Cancelled,
            result = retranscribe_pcm_via_inner(inner, pcm.clone()) => result,
        };
        if attempted_label.is_some() {
            last_attempted_label = attempted_label.clone();
        }
        match result {
            Ok(text) if !text.trim().is_empty() => {
                log::info!(
                    "[coord] 自动静默重试第 {attempt}/{SILENT_RETRY_MAX} 次成功（{} 字）",
                    text.chars().count()
                );
                return SilentRetryOutcome::Transcript {
                    raw: RawTranscript { text, duration_ms },
                    asr_call_label: attempted_label
                        .expect("successful retranscription must have a build-time ASR label"),
                };
            }
            Ok(_) => {
                // 重试得到空转写——多半真没说话，再重试无意义，省流量直接放弃。
                log::info!("[coord] 自动静默重试得到空转写，停止重试");
                return SilentRetryOutcome::Exhausted(last_attempted_label);
            }
            Err(e) => {
                log::warn!("[coord] 自动静默重试第 {attempt}/{SILENT_RETRY_MAX} 次失败: {e}");
            }
        }
    }
    SilentRetryOutcome::Exhausted(last_attempted_label)
}

fn finish_cancelled_processing(inner: &Arc<Inner>, session_id: SessionId) -> bool {
    let finished = {
        let mut state = inner.state.lock();
        finish_cancelled_processing_state(&mut state, session_id)
    };
    if finished {
        schedule_capsule_idle(inner, CAPSULE_CANCEL_HIDE_DELAY_MS);
    }
    finished
}

pub(super) fn schedule_cancelled_asr_release(
    inner: &Arc<Inner>,
    asr: &ActiveAsr,
    session_id: SessionId,
) {
    match asr {
        #[cfg(target_os = "windows")]
        ActiveAsr::FoundryLocalWhisper(_) => {
            schedule_foundry_local_asr_release(inner, AsrReleaseSession::Dictation(session_id));
        }
        #[cfg(target_os = "windows")]
        ActiveAsr::SherpaOnnxLocal(_) => {
            schedule_sherpa_onnx_release(inner, AsrReleaseSession::Dictation(session_id));
        }
        #[cfg(target_os = "macos")]
        ActiveAsr::Local(_) => {
            inner.local_asr_cache.touch();
            schedule_local_asr_release(inner);
        }
        _ => {}
    }
}

/// end_session 转写阶段与「用户取消」赛跑的结果。
enum TranscribeRace {
    Done(Result<RawTranscript, TranscribeFail>),
    /// 用户在 Processing（转写）阶段按 Esc / 取消：drop 掉在途 transcribe future。
    Cancelled,
}

/// 轮询 Processing 阶段的取消标志。用户在转写阶段按 Esc 时，cancel_session 只把
/// `state.cancelled` 置 true —— 此刻 ASR 句柄已被 end_session 从 `inner.asr` 槽 take 走，
/// cancel_session 走的 `cancel_asr_for_session` 是 no-op，够不到在途请求。end_session 用
/// 本函数与在途 transcribe future 赛跑：命中即 drop future，从而中断 reqwest HTTP /
/// 停止等待流式最终结果 / 停止本地转写。
///
/// 用 75ms 轮询而非 notify：转写通常 0.2–3s，几次定时器唤醒的开销可忽略，用户也感知不到
/// 这点延迟；换来的是不依赖任何唤醒信号、没有「取消边沿在注册 waiter 之前触发就丢失」的
/// 竞态，逻辑上更稳。
async fn wait_for_processing_cancel(inner: &Arc<Inner>) {
    loop {
        if inner.state.lock().cancelled {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
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
    // 构建时快照（begin_session 存入）。会话中途改设置不影响这份归因。
    let mut asr_call_label = take_asr_label_for_session(inner, current_session_id);
    let asr = match asr_opt {
        Some(a) => a,
        None => {
            restore_prepared_windows_ime_session(inner, current_session_id);
            if !finish_cancelled_processing(inner, current_session_id) {
                set_phase_idle_if_session_matches(inner, current_session_id);
            }
            return Ok(());
        }
    };

    let uses_global_timeout = asr_transcribe_uses_global_timeout(&asr);
    // ASR 句柄内部是 Arc，clone 只是 +1 引用。留一份给取消路径：transcribe future 会把
    // `asr` move 进去，命中取消时那个 future 会被 drop（连同它持有的 Arc），我们再用这份
    // clone 显式 cancel，促使流式 WebSocket 立刻关闭、不残留后台 worker。
    let asr_for_cancel = asr.clone();
    // 「等待转写结果」实测起点：流式 ASR 量的是收尾延迟，批式量完整转写。写进
    // history.asr_ms 供历史详情页展示（含下方的自动静默重试时间——那也是用户等的时间）。
    let transcribe_started = std::time::Instant::now();
    // 每个引擎分支产出 Ok(RawTranscript) 或 Err(TranscribeFail)；失败/超时不再就地 return，
    // 而是把失败值交给 match 之后统一处理：先自动静默重试（从归档音频重转，应对网络/服务端
    // 瞬时抖动），重试拿回文本就当正常转写继续；彻底失败才 fail_dictation 保留录音 + 报错。
    //
    // 整段转写与「用户在 Processing 阶段取消」赛跑：命中取消就直接 drop 掉 transcribe future
    // 中断在途请求，不再傻等它跑完（见 issue「转写中按 Esc 停不下来」）。
    let raced: TranscribeRace = {
        let transcribe_fut = async move {
            let transcribe_outcome: Result<RawTranscript, TranscribeFail> = match asr {
                ActiveAsr::Volcengine(asr) => {
                    debug_assert!(uses_global_timeout);
                    if let Err(e) = asr.send_last_frame().await {
                        log::error!("[coord] send last frame failed: {e}");
                    }
                    // 添加全局超时保护：防止 await_final_result() 永远挂起
                    let timeout_duration =
                        std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
                    match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            log::error!("[coord] await final failed: {e}");
                            // 关闭 WebSocket 连接，避免流式 ASR 资源泄漏
                            asr.cancel();
                            Err(TranscribeFail::new(format!("识别失败: {e}"), e.to_string()))
                        }
                        Err(_) => {
                            // 全局超时：最后的防线
                            log::error!(
                                "[coord] 全局超时 {} 秒 - 强制恢复",
                                COORDINATOR_GLOBAL_TIMEOUT_SECS
                            );
                            // 清理 ASR session，避免资源泄漏
                            asr.cancel();
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "global timeout".to_string(),
                            ))
                        }
                    }
                }
                ActiveAsr::Whisper(w) => {
                    debug_assert!(uses_global_timeout);
                    // Whisper / OpenRouter 动态超时：音频越长、分片越多，给更多
                    // HTTP round-trip 预算。公式见 `whisper_transcribe_timeout`。
                    let audio_secs = (w.buffer_duration_ms() as f64) / 1000.0;
                    let timeout_duration = whisper_transcribe_timeout(audio_secs);
                    log::info!(
                        "[coord] Whisper transcribe: audio={:.2}s timeout={}s",
                        audio_secs,
                        timeout_duration.as_secs()
                    );
                    match tokio::time::timeout(timeout_duration, w.transcribe()).await {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            log::error!("[coord] whisper transcribe failed: {e}");
                            Err(TranscribeFail::new(format!("识别失败: {e}"), e.to_string()))
                        }
                        Err(_) => {
                            log::error!(
                                "[coord] Whisper 动态超时 {}s（音频 {:.2}s）",
                                timeout_duration.as_secs(),
                                audio_secs
                            );
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "whisper global timeout".to_string(),
                            ))
                        }
                    }
                }
                ActiveAsr::Mimo(m) => {
                    debug_assert!(uses_global_timeout);
                    let timeout_duration =
                        std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
                    match tokio::time::timeout(timeout_duration, m.transcribe()).await {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            log::error!("[coord] MiMo ASR transcribe failed: {e}");
                            Err(TranscribeFail::new(format!("识别失败: {e}"), e.to_string()))
                        }
                        Err(_) => {
                            log::error!(
                                "[coord] MiMo ASR 全局超时 {} 秒",
                                COORDINATOR_GLOBAL_TIMEOUT_SECS
                            );
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "mimo global timeout".to_string(),
                            ))
                        }
                    }
                }
                ActiveAsr::DashScopeMultimodal(m) => {
                    debug_assert!(uses_global_timeout);
                    let audio_secs = m.buffer_duration_ms() as f64 / 1000.0;
                    let timeout_duration = m.transcribe_timeout(audio_secs);
                    log::info!(
                        "[coord] DashScope Fun-ASR-Flash dynamic timeout: {}s (audio {:.2}s)",
                        timeout_duration.as_secs(),
                        audio_secs
                    );
                    match tokio::time::timeout(timeout_duration, m.transcribe()).await {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            log::error!("[coord] DashScope Fun-ASR-Flash transcribe failed: {e}");
                            Err(TranscribeFail::new(format!("识别失败: {e}"), e.to_string()))
                        }
                        Err(_) => {
                            log::error!(
                                "[coord] DashScope Fun-ASR-Flash dynamic timeout {}s (audio {:.2}s)",
                                timeout_duration.as_secs(),
                                audio_secs
                            );
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "dashscope multimodal global timeout".to_string(),
                            ))
                        }
                    }
                }
                ActiveAsr::ElevenLabs(e) => {
                    debug_assert!(uses_global_timeout);
                    let audio_secs = e.buffer_duration_ms() as f64 / 1000.0;
                    let timeout_duration = crate::asr::elevenlabs::transcribe_timeout(audio_secs);
                    log::info!(
                        "[coord] ElevenLabs dynamic timeout: {}s (audio {:.2}s)",
                        timeout_duration.as_secs(),
                        audio_secs
                    );
                    match tokio::time::timeout(timeout_duration, e.transcribe()).await {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(error)) => {
                            log::error!("[coord] ElevenLabs ASR transcribe failed: {error}");
                            Err(TranscribeFail::new(
                                format!("识别失败: {error}"),
                                error.to_string(),
                            ))
                        }
                        Err(_) => Err(TranscribeFail::new(
                            "识别超时".to_string(),
                            "elevenlabs dynamic timeout".to_string(),
                        )),
                    }
                }
                ActiveAsr::Bailian(asr) => {
                    debug_assert!(uses_global_timeout);
                    if let Err(e) = asr.send_last_frame().await {
                        log::error!("[coord] Bailian send last frame failed: {e}");
                    }
                    let timeout_duration =
                        std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
                    match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            log::error!("[coord] Bailian await final failed: {e}");
                            // 关闭 WebSocket 连接，避免流式 ASR 资源泄漏
                            asr.cancel();
                            Err(TranscribeFail::new(format!("识别失败: {e}"), e.to_string()))
                        }
                        Err(_) => {
                            log::error!(
                                "[coord] Bailian 全局超时 {} 秒",
                                COORDINATOR_GLOBAL_TIMEOUT_SECS
                            );
                            asr.cancel();
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "bailian global timeout".to_string(),
                            ))
                        }
                    }
                }
                ActiveAsr::Qwen3Realtime(asr) => {
                    debug_assert!(uses_global_timeout);
                    if let Err(e) = asr.send_last_frame().await {
                        log::error!("[coord] Qwen3 realtime send last frame failed: {e}");
                    }
                    let timeout_duration =
                        std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
                    match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            log::error!("[coord] Qwen3 realtime await final failed: {e}");
                            // 关闭 WebSocket 连接，避免流式 ASR 资源泄漏
                            asr.cancel();
                            Err(TranscribeFail::new(format!("识别失败: {e}"), e.to_string()))
                        }
                        Err(_) => {
                            log::error!(
                                "[coord] Qwen3 realtime 全局超时 {} 秒",
                                COORDINATOR_GLOBAL_TIMEOUT_SECS
                            );
                            asr.cancel();
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "qwen3 realtime global timeout".to_string(),
                            ))
                        }
                    }
                }
                ActiveAsr::StepfunRealtime(asr) => {
                    debug_assert!(uses_global_timeout);
                    if let Err(e) = asr.send_last_frame().await {
                        log::error!("[coord] StepFun realtime send last frame failed: {e}");
                    }
                    let timeout_duration =
                        std::time::Duration::from_secs(COORDINATOR_GLOBAL_TIMEOUT_SECS);
                    match tokio::time::timeout(timeout_duration, asr.await_final_result()).await {
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            log::error!("[coord] StepFun realtime await final failed: {e}");
                            // 关闭 WebSocket 连接，避免流式 ASR 资源泄漏
                            asr.cancel();
                            Err(TranscribeFail::new(format!("识别失败: {e}"), e.to_string()))
                        }
                        Err(_) => {
                            log::error!(
                                "[coord] StepFun realtime 全局超时 {} 秒",
                                COORDINATOR_GLOBAL_TIMEOUT_SECS
                            );
                            asr.cancel();
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "stepfun realtime global timeout".to_string(),
                            ))
                        }
                    }
                }
                #[cfg(target_os = "windows")]
                ActiveAsr::FoundryLocalWhisper(local) => {
                    debug_assert!(!uses_global_timeout);
                    let audio_secs = (local.buffer_duration_ms() as f64) / 1000.0;
                    let timeout_duration = windows_local_asr_transcribe_timeout(audio_secs);
                    log::info!(
                        "[coord] Foundry Local Whisper transcribe: audio={:.2}s timeout={}s",
                        audio_secs,
                        timeout_duration.as_secs()
                    );
                    match local.transcribe(timeout_duration).await {
                        Ok(r) => {
                            schedule_foundry_local_asr_release(
                                inner,
                                AsrReleaseSession::Dictation(current_session_id),
                            );
                            Ok(r)
                        }
                        Err(e) => {
                            // 用户取消现在由外层 select! 统一处理（drop 掉本 future 中断在途转写），
                            // 到这里的 Err 一律当作真失败：调度引擎释放 + 交给 match 后的重试/报错。
                            log::error!("[coord] Foundry Local Whisper transcribe failed: {e:#}");
                            schedule_foundry_local_asr_release(
                                inner,
                                AsrReleaseSession::Dictation(current_session_id),
                            );
                            Err(TranscribeFail::new(
                                format!("本地识别失败: {e}"),
                                e.to_string(),
                            ))
                        }
                    }
                }
                // Windows sherpa-onnx offline batch：停止录音后整段转写，再复用现有
                // polish / insert / history 收尾路径。
                #[cfg(target_os = "windows")]
                ActiveAsr::SherpaOnnxLocal(local) => {
                    debug_assert!(!uses_global_timeout);
                    let audio_secs = (local.buffer_duration_ms() as f64) / 1000.0;
                    let timeout_duration = windows_local_asr_transcribe_timeout(audio_secs);
                    log::info!(
                        "[coord] sherpa-onnx transcribe: audio={:.2}s timeout={}s",
                        audio_secs,
                        timeout_duration.as_secs()
                    );
                    match local.transcribe(timeout_duration).await {
                        Ok(r) => {
                            schedule_sherpa_onnx_release(
                                inner,
                                AsrReleaseSession::Dictation(current_session_id),
                            );
                            Ok(r)
                        }
                        Err(e) => {
                            // 取消由外层 select! 统一处理，见 Foundry 分支同款注释。
                            log::error!("[coord] sherpa-onnx transcribe failed: {e:#}");
                            schedule_sherpa_onnx_release(
                                inner,
                                AsrReleaseSession::Dictation(current_session_id),
                            );
                            Err(TranscribeFail::new(
                                format!("本地识别失败: {e}"),
                                e.to_string(),
                            ))
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
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            log::error!("[coord] local Qwen3-ASR transcribe failed: {e:#}");
                            Err(TranscribeFail::new(
                                format!("本地识别失败: {e}"),
                                e.to_string(),
                            ))
                        }
                        Err(_) => {
                            log::error!(
                                "[coord] local Qwen3-ASR 动态超时 {}s（音频 {:.2}s）",
                                timeout_duration.as_secs(),
                                audio_secs
                            );
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "local global timeout".to_string(),
                            ))
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
                        Ok(Ok(r)) => Ok(r),
                        Ok(Err(e)) => {
                            // 取消由外层 select! 统一处理，见 Foundry 分支同款注释。
                            log::error!("[coord] Apple Speech transcribe failed: {e:#}");
                            Err(TranscribeFail::new(
                                format!("本地识别失败: {e}"),
                                e.to_string(),
                            ))
                        }
                        Err(_) => {
                            log::error!(
                                "[coord] Apple Speech 动态超时 {}s（音频 {:.2}s）",
                                timeout_duration.as_secs(),
                                audio_secs
                            );
                            Err(TranscribeFail::new(
                                "识别超时".to_string(),
                                "apple-speech global timeout".to_string(),
                            ))
                        }
                    }
                }
            };
            transcribe_outcome
        };
        tokio::select! {
            // biased：每次先查取消标志，取消优先于「转写恰好同时完成」。
            biased;
            _ = wait_for_processing_cancel(inner) => TranscribeRace::Cancelled,
            outcome = transcribe_fut => TranscribeRace::Done(outcome),
        }
    };

    let transcribe_outcome: Result<RawTranscript, TranscribeFail> = match raced {
        TranscribeRace::Cancelled => {
            log::info!("[coord] cancel during transcribe — 中断在途 ASR 请求，丢弃转写");
            // 上面 select! 已把 transcribe_fut drop 掉（中断 reqwest / 停止等待流式结果 /
            // 停止本地转写）；这里再显式 cancel 一次，促使流式 WebSocket 立即关闭、不残留
            // 后台 worker。asr_for_cancel 与被 drop 的 future 共享同一 Arc 底层。
            let asr_for_release = asr_for_cancel.clone();
            cancel_active_asr(asr_for_cancel);
            // end_session 已经把 ASR 从 inner.asr 取走，cancel_session 无法再触发
            // provider 的释放调度；取消路径必须自己补上，否则本地模型会一直占用缓存。
            schedule_cancelled_asr_release(inner, &asr_for_release, current_session_id);
            restore_prepared_windows_ime_session(inner, current_session_id);
            // 与下方「ASR 完成后 cancel 检查」同款收尾（finish_cancelled_processing 负责
            // 把 phase 收回 Idle、清 focus_target）。
            finish_cancelled_processing(inner, current_session_id);
            return Ok(());
        }
        TranscribeRace::Done(outcome) => outcome,
    };

    // ASR 完成后 cancel 检查：转写恰好跑完、用户几乎同时按 Esc（select! 走了 Done 分支）时
    // 这里兜底命中。上面赛跑分支处理的是「转写还在途中」的取消。
    // 优先级高于 empty 检查 — 用户取消 → 静默丢弃，不写失败历史也不弹错误胶囊。
    if inner.state.lock().cancelled {
        log::info!("[coord] cancel detected after ASR — discarding transcript");
        restore_prepared_windows_ime_session(inner, current_session_id);
        // PR #387 的「cancel 后清 focus_target」契约要在 Processing 路径上也成立。
        // cancel_session 在 Processing 阶段故意跳过 finish_cancel_session_state（让
        // 这里收尾），但此前的 end_session 没把 focus_target 清掉。logic-review
        // 2026-05-10 P3 (🚩) 把这条补完。
        finish_cancelled_processing(inner, current_session_id);
        return Ok(());
    }

    // ASR 失败/超时：先自动静默重试（从刚归档的音频重转，应对网络/服务端瞬时抖动）。上面的
    // cancel 检查已先行——用户主动取消的会话不会走到这里触发重试。重试拿回文本就当作正常转写
    // 继续走润色/插入；彻底失败才 fail_dictation 保留录音 + 报错（音频仍在，可去历史手动重转）。
    let raw = match transcribe_outcome {
        Ok(raw) => raw,
        Err(fail) => match try_silent_retranscribe(inner, current_session_id).await {
            SilentRetryOutcome::Transcript {
                raw,
                asr_call_label: retry_label,
            } => accept_silent_retry_transcript(raw, retry_label, &mut asr_call_label),
            SilentRetryOutcome::Cancelled => {
                log::info!("[coord] cancel during silent ASR retry — discarding transcript");
                restore_prepared_windows_ime_session(inner, current_session_id);
                finish_cancelled_processing(inner, current_session_id);
                return Ok(());
            }
            SilentRetryOutcome::Exhausted(retry_label) => {
                if retry_label.is_some() {
                    asr_call_label = retry_label;
                }
                // 处理最后一次重试结果时也复查一次取消标志，覆盖「重试刚返回
                // Exhausted 与用户同时按 Esc」的窄竞态，避免误走失败提示。
                if inner.state.lock().cancelled {
                    log::info!(
                        "[coord] cancel after silent ASR retry — discarding transcript"
                    );
                    restore_prepared_windows_ime_session(inner, current_session_id);
                    finish_cancelled_processing(inner, current_session_id);
                    return Ok(());
                }
                return fail_dictation(
                    inner,
                    current_session_id,
                    elapsed,
                    transcribe_started.elapsed().as_millis() as u64,
                    fail.user_msg,
                    fail.err,
                    asr_call_label.as_ref(),
                );
            }
        },
    };
    let asr_ms = transcribe_started.elapsed().as_millis() as u64;
    let (asr_provider, asr_model) = match &asr_call_label {
        Some(label) => (Some(label.provider.clone()), label.model.clone()),
        None => (None, None),
    };

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
            // session_id 与归档 wav 同名，empty 录音才能被 read_audio_recording /
            // retranscribe_recording 凭 id 找回（之前用 Uuid::new_v4，与 `<session_id>.wav`
            // 对不上，has_audio_recording 标了 true 但前端永远 404）。
            id: current_session_id.to_string(),
            created_at: Utc::now().to_rfc3339(),
            source: crate::types::HistorySource::Voice,
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
            // 空转写也记下是哪个 ASR 模型给出的空结果 + 等了多久，供模型对比排查。
            asr_provider: asr_provider.clone(),
            asr_model: asr_model.clone(),
            llm_provider: None,
            llm_model: None,
            asr_ms: Some(asr_ms),
            polish_ms: None,
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

    // 拿到非空转写 → 原始音频对「ASR 重试」已无价值。非 debug 用户：删掉刚归档的 wav
    // （隐私——成功的口述不留痕，只保留失败录音供手动重转 / 自动重试），并把
    // audio_archive_active 翻成 false，让下游 history 的 has_audio_recording 读到真实状态
    // （成功条目不会渲染播放/重转按钮再 404）。debug 用户：保留全部录音（原调试行为）。
    // 失败/超时路径在上面的 match 内就产出 Err 并走 fail_dictation，不会走到这里，失败录音始终留存。
    if !inner.prefs.get().record_audio_for_debug
        && inner.audio_archive_active.swap(false, Ordering::Relaxed)
    {
        if let Ok(path) =
            crate::persistence::recording_path_for_session(&current_session_id.to_string())
        {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("[coord] 清理成功口述的归档录音失败: {e}");
                }
            }
        }
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
    // 风格包原有 Prompt 就是录音 / ASR 后处理的完整规则；不要在全局设置再叠一层，
    // 否则会让同一个风格包的导出、复用和运行结果不一致。
    let style_system_prompt = crate::types::style_pack_prompt(
        &pack,
        crate::types::StylePromptKind::DictationAsr,
    );
    let raw_uses_llm = mode == PolishMode::Raw && super::raw_style_pack_uses_llm(&pack);
    let translation_target = prefs.translation_target_language.trim().to_string();
    let translation_active =
        inner.translation_modifier_seen.load(Ordering::SeqCst) && !translation_target.is_empty();
    log::info!(
        "[style-pack] runtime dispatch scope=asr session_id={} active_pack={} kind={:?} mode={:?} raw_chars={} prompt_chars={} raw_uses_llm={} translation_active={} hotwords={} working_languages={:?}",
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
        chinese_script_preference,
        prefs.windows_insertion_mode,
    );
    log::info!(
        "[coord] polish dispatch: translation={translation_active} mode={mode:?} streaming_eligible={streaming_eligible}"
    );

    // Linux: emit_capsule(Polishing) 已通过 fcitx5 auxDown 显示 "✨ 润色中..."，
    // 无需在此重复调用。

    // 翻译会话润色后的源语言文本（译文前的中间产物），仅翻译路径解析成功时有值，
    // 写进 history 供后续普通润色轮复用（剔除译文、避免外语污染）。
    let mut polish_source: Option<String> = None;
    // 一次 LLM 调用的构建时快照：polish 链路在成功构建 provider、即将发起真实调用时
    // 填充（见 polish_flow.rs）。Raw 直通、凭据缺失等 preflight 失败都保持 None——
    // 此时不落 llm_* / polish_ms，避免"没调用却记了模型/耗时"的伪数据（PR #826 review）。
    let mut llm_call: Option<crate::polish::LlmCallLabel> = None;
    // 只累计 provider 请求本身的耗时。流式路径的输入法切换、逐字上屏和队列排空
    // 属于插入阶段，不能混入用于模型对比的 polish_ms。
    let mut llm_elapsed_ms: Option<u64> = None;
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
            &mut llm_call,
            &mut llm_elapsed_ms,
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
            &mut llm_call,
            &mut llm_elapsed_ms,
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
            &mut llm_call,
            &mut llm_elapsed_ms,
        )
        .await;
        (p, e, false)
    };
    // 耗时与标签都以「真的发起了 provider 调用」为准；preflight 失败和 Raw 直通均为 None。
    let polish_ms = llm_elapsed_ms;
    let (llm_provider, llm_model) = match &llm_call {
        Some(label) => (Some(label.provider.clone()), Some(label.model.clone())),
        None => (None, None),
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
        finish_cancelled_processing(inner, current_session_id);
        return Ok(());
    }

    let focus_target = inner.state.lock().focus_target;
    let focus_ready_for_paste = restore_focus_target_if_possible(focus_target);
    let prefs = inner.prefs.get();
    let restore_clipboard = prefs.restore_clipboard_after_paste;
    let allow_non_tsf_insertion_fallback = prefs.allow_non_tsf_insertion_fallback;
    let windows_insertion_mode = prefs.windows_insertion_mode;
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
                match windows_insertion_mode {
                    crate::types::WindowsInsertionMode::SendInput => {
                        let sendinput_options = windows_sendinput_options_from_prefs(&prefs);
                        if allow_non_tsf_insertion_fallback {
                            insert_via_non_tsf_fallback(
                                inner,
                                &polished,
                                restore_clipboard,
                                paste_shortcut,
                            )
                        } else {
                            inner
                                .inserter
                                .insert_via_unicode_keystrokes(&polished, sendinput_options)
                        }
                    }
                    crate::types::WindowsInsertionMode::Paste => inner.inserter.insert(
                        &polished,
                        restore_clipboard,
                        paste_shortcut,
                    ),
                    crate::types::WindowsInsertionMode::Tsf => {
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
                }
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
    let error_code = dictation_error_code(
        status,
        polish_error.is_some(),
        focus_ready_for_paste,
        allow_non_tsf_insertion_fallback,
        windows_insertion_mode,
    )
    .map(str::to_string);
    let tsf_required_insert_failed = error_code.as_deref() == Some("windowsImeTsfRequired");

    // 与 coordinator 内部 SessionId 对齐：方便 recorder 旁路写盘的 `<session_id>.wav`
    // 跟 history 这条 DictationSession.id 同名，前端凭 id 就能找到对应录音文件。
    let history_session_id = current_session_id.to_string();
    let history_created_at = Utc::now().to_rfc3339();
    let prefs_snapshot = inner.prefs.get();
    let session = DictationSession {
        id: history_session_id.clone(),
        created_at: history_created_at.clone(),
        source: crate::types::HistorySource::Voice,
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
        asr_provider,
        asr_model,
        llm_provider,
        llm_model,
        asr_ms: Some(asr_ms),
        polish_ms,
    };
    if let Err(e) = inner.history.append_with_retention(
        session,
        prefs_snapshot.history_retention_days,
        prefs_snapshot.history_max_entries,
    ) {
        log::error!("[coord] history append failed: {e}");
    }
    // 活动计数（概览页热力图数据源）：只有成功完成的听写才点亮格子——转录失败 /
    // 错误收尾的两处 append 不计。写失败不阻断主流程。
    if let Err(e) = inner
        .activity
        .bump(&chrono::Local::now().format("%Y-%m-%d").to_string())
    {
        log::warn!("[coord] activity bump failed: {e}");
    }

    // 远程输入：把本次最终文字回传给手机端。remote_server 的 WS handler 订阅了
    // "remote:result"（mod.rs:614），但此前全仓从未 emit，导致手机结果区永远空（#691）。
    // 与上面的 vocab:updated 同模式：无手机连接时无人转发 = 无害空操作。
    if !polished.trim().is_empty() {
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit("remote:result", polished.clone());
        }
    }

    let done_message = if tsf_required_insert_failed {
        Some("TSF 未上屏，已禁止非 TSF 兜底".to_string())
    } else {
        default_done_message(status, polish_error.is_some())
    };

    // 胶囊只在 error 态渲染 message —— done 态按设计是「冻结光效淡出、不带文字」
    // （见 Capsule.tsx 的 VoiceOrbStage：`state === 'error' && <span>{message}</span>`）。
    // 所以失败信息必须走 error 态才看得见，否则文案算出来就被前端丢掉。
    //
    // 最典型的受害者是润色失败：它会静默回退成未润色的原文，而胶囊照常显示成功态，
    // 用户界面上没有任何痕迹。实际后果是 LLM 凭证失效后，用户连着十几个小时每句话
    // 都在拿原文，只能靠「今天出来的字怎么变笨了」察觉，日志里其实每一句都报了错。
    let session_failed =
        tsf_required_insert_failed || polish_error.is_some() || status == InsertStatus::Failed;
    let capsule_state = if session_failed {
        CapsuleState::Error
    } else {
        CapsuleState::Done
    };

    emit_capsule(
        inner,
        capsule_state,
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

pub(super) fn dictation_error_code(
    status: InsertStatus,
    polish_failed: bool,
    focus_ready_for_paste: bool,
    allow_non_tsf_insertion_fallback: bool,
    windows_insertion_mode: crate::types::WindowsInsertionMode,
) -> Option<&'static str> {
    if !focus_ready_for_paste && status == InsertStatus::Failed {
        Some("focusRestoreFailed")
    } else if cfg!(target_os = "windows")
        && focus_ready_for_paste
        && !allow_non_tsf_insertion_fallback
        && windows_insertion_mode == crate::types::WindowsInsertionMode::Tsf
        && status == InsertStatus::Failed
    {
        Some("windowsImeTsfRequired")
    } else if polish_failed {
        Some("polishFailed")
    } else {
        None
    }
}

pub(super) fn cancel_session(inner: &Arc<Inner>) -> bool {
    let Some(decision) = ({
        let mut state = inner.state.lock();
        let phase = state.phase;
        let decision = begin_cancel_session_state(&mut state);
        if phase == SessionPhase::Inserting {
            log::info!("[coord] cancel ignored — already in Inserting phase, can't undo paste");
        }
        decision
    }) else {
        return false;
    };

    // 顺序要紧：先把 UI 收干净，再去拆麦克风 / ASR。
    //
    // 反过来（原来的顺序）会让胶囊等在 `stop_recorder_for_session` 后面 ——
    // `Recorder::stop()` 要 join 音频线程，而音频线程退出前要 join liveness watchdog，
    // watchdog 又睡在自己的检查间隔里，实测撤销到胶囊消失能差 0.8~1 秒。用户按 Option+Q
    // 或按 Esc 的观感就是「明明已经取消了，胶囊还赖着」。拆资源不需要 UI 等它，反正
    // 这段时间录到的音频整条会话都要丢。
    //
    // 代价：胶囊消失后麦克风还会多开一小会儿（系统菜单栏的录音小圆点晚灭）。这段窗口
    // 必须足够短 —— 否则紧接着那次真想说话的按下会在旧 recorder 还占着麦克风时
    // build_input_stream，而 `Recorder` 没有 Drop 停采，recorder 槽被新会话覆盖后旧音频
    // 线程会继续跑、抓着麦克风不放。所以 watchdog 的检查间隔必须是碎的（见
    // recorder.rs 的 WATCHDOG_*），把这段窗口压到几十毫秒；两处改动是一对，不能只留一个。
    //
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
    // emit_capsule 仍然排在 finish_cancel_session_state 之后：它要读 state.voice_agent /
    // phase 拼 payload，提到前面会发出「还在进行中」的那一帧。
    emit_capsule(inner, CapsuleState::Cancelled, 0.0, 0, None, None);
    log::info!("[coord] session cancelled (was {:?})", decision.phase);
    schedule_capsule_idle(inner, CAPSULE_CANCEL_HIDE_DELAY_MS);
    // 取消时也熄灭整屏彩虹描边（dictation session 没开描边，hide 是无害 no-op）。
    if let Some(app) = inner.app.lock().clone() {
        crate::hide_less_computer_glow(&app);
    }

    stop_recorder_for_session(inner, decision.session_id);
    cancel_asr_for_session(inner, decision.session_id);
    restore_prepared_windows_ime_session(inner, decision.session_id);
    true
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

/// 多轮上下文最多回看的历史轮数。时间窗口（polish_context_window_minutes）只限"多久内"，
/// 不限"多少条"——5 分钟内堆积几十条历史时，全部前置进 LLM 会让输入 token 暴涨、首字延迟
/// （TTFT）显著变长，影响全体用户（#678）。取最近 2 轮即可保留代词/续写所需的对话连续性，
/// 同时把上下文 token 控制在常数量级。sessions 为 newest-first，`.take` 即取最近若干轮。
const MAX_POLISH_CONTEXT_TURNS: usize = 2;

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
        // 限制条数：sessions newest-first，过滤后取最近 MAX_POLISH_CONTEXT_TURNS 轮（#678）。
        .take(MAX_POLISH_CONTEXT_TURNS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        accept_silent_retry_transcript, append_typed_prefix, batch_asr_chunk_limit_ms,
        build_transcribe_failed_session, default_done_message, drain_streaming_insert_deltas_with,
        eligible_polish_context_turns, finalize_polished_text, flush_streaming_insert_buffer_with,
        pcm_duration_ms, pcm_from_wav_bytes, streaming_insert_eligible,
    };
    #[cfg(target_os = "macos")]
    use super::{macos_keyless_dictation_provider, MacosKeylessDictationProvider};
    use crate::types::{
        ChineseScriptPreference, CorrectionRule, DictationSession, InsertStatus, PolishMode,
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn approval_request_is_denied_when_session_cancelled_during_wait() {
        let coordinator = crate::coordinator::Coordinator::new();
        {
            let mut state = coordinator.inner.state.lock();
            state.cancelled = true; // 模拟审批挂起期间用户按 Esc（cancel_session 置位）
        }
        let outcome = super::LessComputerOutcome::Done {
            text: "permission denied: rm -rf".into(),
            cost_usd: None,
        };
        let result = super::maybe_request_approval(&coordinator.inner, &outcome).await;
        assert_eq!(result, None, "会话取消后审批应按 Deny 处理");
        assert!(
            super::less_computer_approvals().lock().unwrap().is_empty(),
            "取消后审批注册表应被清理"
        );
    }

    fn coordinator_with_dictation_hotkey(
        binding: crate::types::ShortcutBinding,
    ) -> super::super::Coordinator {
        let coordinator = super::super::Coordinator::new();
        coordinator
            .inner
            .prefs
            .set(crate::types::UserPreferences {
                dictation_hotkey: binding,
                ..Default::default()
            })
            .unwrap();
        coordinator
    }

    // modifier-only 触发键：按下后必须先过仲裁窗口，才能知道这是说话还是
    // Option+任意字母/数字键。
    #[tokio::test]
    async fn modifier_only_press_waits_out_the_arbitration_window() {
        let coordinator = coordinator_with_dictation_hotkey(crate::types::ShortcutBinding {
            primary: "LeftOption".into(),
            modifiers: vec![],
        });

        let started = std::time::Instant::now();
        // 测试里没装监听器（inner.hotkey = None）→ 读不到叠加标志，按「不是组合键」放行。
        assert!(!super::press_resolves_to_combo(&coordinator.inner, 1).await);
        assert!(started.elapsed() >= super::COMBO_ARBITRATION_GRACE);
    }

    #[tokio::test]
    async fn arbitration_combo_does_not_consume_debounce_window() {
        let coordinator = coordinator_with_dictation_hotkey(crate::types::ShortcutBinding {
            primary: "LeftOption".into(),
            modifiers: vec![],
        });
        coordinator
            .inner
            .hotkey_press_generation
            .store(1, std::sync::atomic::Ordering::SeqCst);
        coordinator
            .inner
            .hotkey_combo_pending_presses
            .lock()
            .push_back(1);
        *coordinator.inner.last_hotkey_dispatch_at.lock() = Some(std::time::Instant::now());

        super::begin_session_from_press(&coordinator.inner, 1).await;

        assert!(coordinator.inner.last_hotkey_dispatch_at.lock().is_none());
        assert_eq!(
            coordinator.inner.state.lock().phase,
            crate::coordinator_state::SessionPhase::Idle
        );
    }

    // 自定义组合键（Cmd+Shift+D）没有歧义 —— 白等这一下就是纯掉延迟。
    #[tokio::test]
    async fn custom_combo_press_skips_the_arbitration_window() {
        let coordinator = coordinator_with_dictation_hotkey(crate::types::ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        });

        let started = std::time::Instant::now();
        assert!(!super::press_resolves_to_combo(&coordinator.inner, 1).await);
        assert!(!super::combo_seen_for_press(&coordinator.inner, 0));
        assert!(started.elapsed() < super::COMBO_ARBITRATION_GRACE);
    }

    #[test]
    fn pending_combo_queue_preserves_multiple_press_ids() {
        let coordinator = super::super::Coordinator::new();
        coordinator
            .inner
            .hotkey_combo_pending_presses
            .lock()
            .extend([11, 12]);

        assert!(super::combo_seen_for_press(&coordinator.inner, 11));
        assert!(super::combo_seen_for_press(&coordinator.inner, 12));
        assert!(!super::combo_seen_for_press(&coordinator.inner, 11));
    }

    #[test]
    fn silent_retry_replaces_initial_asr_attribution() {
        let mut label = Some(super::AsrCallLabel::new(
            "volcengine",
            Some("volc.seedasr.sauc.duration".into()),
        ));
        let retry_label = super::AsrCallLabel::new(
            "bailian-qwen3-realtime",
            Some("qwen3-asr-flash-realtime".into()),
        );
        let raw = super::RawTranscript {
            text: "重试成功".into(),
            duration_ms: 900,
        };

        let accepted = accept_silent_retry_transcript(raw, retry_label.clone(), &mut label);

        assert_eq!(accepted.text, "重试成功");
        assert_eq!(label, Some(retry_label));
    }

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
            source: crate::types::HistorySource::Voice,
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
            asr_provider: None,
            asr_model: None,
            llm_provider: None,
            llm_model: None,
            asr_ms: None,
            polish_ms: None,
        }
    }

    #[test]
    fn polish_context_caps_at_max_turns_keeping_most_recent() {
        // sessions newest-first：超过上限时只保留最近 MAX_POLISH_CONTEXT_TURNS 轮（#678）。
        let sessions = vec![
            history_session("t1", "raw1", "final1", Some("pack.id"), false, None),
            history_session("t2", "raw2", "final2", Some("pack.id"), false, None),
            history_session("t3", "raw3", "final3", Some("pack.id"), false, None),
            history_session("t4", "raw4", "final4", Some("pack.id"), false, None),
        ];

        let turns = eligible_polish_context_turns(sessions, "pack.id", false);

        assert_eq!(turns.len(), super::MAX_POLISH_CONTEXT_TURNS);
        assert_eq!(
            turns,
            vec![
                ("raw1".to_string(), "final1".to_string()),
                ("raw2".to_string(), "final2".to_string()),
            ]
        );
    }

    #[test]
    fn transcribe_failed_history_keeps_session_id_for_recording_lookup() {
        // 修 #613：失败 / empty 历史条目的 id 必须 == coordinator SessionId，这样归档录音
        // `recordings/<session_id>.wav` 才能被 read_audio_recording / retranscribe_recording
        // 凭 id 找回。之前 empty 分支用 Uuid::new_v4()，与 wav 文件名对不上 → 前端永远 404、
        // 录音随 prune 丢失（用户报告「识别失败之前的语音也都丢失了」）。
        let sid = Uuid::new_v4();
        let session =
            build_transcribe_failed_session(sid, 4200, 17_250, PolishMode::Structured, true);
        assert_eq!(session.id, sid.to_string());
    }

    #[test]
    fn transcribe_failed_history_marks_failed_and_recoverable() {
        let sid = Uuid::new_v4();
        let session =
            build_transcribe_failed_session(sid, 1234, 17_250, PolishMode::Structured, true);
        assert!(matches!(session.insert_status, InsertStatus::Failed));
        assert_eq!(session.error_code.as_deref(), Some("transcribeFailed"));
        assert_eq!(session.duration_ms, Some(1234));
        assert_eq!(session.asr_ms, Some(17_250));
        // 归档成功 → 标 has_audio_recording=true，前端据此渲染「重新转录」入口。
        assert_eq!(session.has_audio_recording, Some(true));
    }

    #[test]
    fn transcribe_failed_history_flags_no_audio_when_archive_inactive() {
        // 录音归档失败（has_audio=false）→ 条目仍写（用户看得到这次失败），但不标可重转，
        // 避免前端渲染重转按钮而后端找不到 wav。
        let sid = Uuid::new_v4();
        let session = build_transcribe_failed_session(sid, 1, 250, PolishMode::Structured, false);
        assert_eq!(session.has_audio_recording, Some(false));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keyless_dictation_provider_routes_apple_speech_locally() {
        assert_eq!(
            macos_keyless_dictation_provider(crate::asr::local::APPLE_SPEECH_PROVIDER_ID),
            Some(MacosKeylessDictationProvider::AppleSpeech)
        );
        assert_eq!(
            macos_keyless_dictation_provider(crate::asr::local::PROVIDER_ID),
            Some(MacosKeylessDictationProvider::LocalQwen3)
        );
        assert_eq!(macos_keyless_dictation_provider("volcengine"), None);
    }

    #[test]
    fn pcm_from_wav_strips_44_byte_header() {
        // 自动静默重试从归档 wav 取 PCM：标准 16k/mono/16-bit 头固定 44 字节，PCM = 头之后全部。
        let mut wav = vec![0u8; 44];
        wav.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(pcm_from_wav_bytes(&wav), Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn pcm_from_wav_rejects_headeronly_or_truncated() {
        // <= 44 字节 = 没有音频负载（空录音 / 截断）→ None，不触发无意义的重试。
        assert_eq!(pcm_from_wav_bytes(&[0u8; 44]), None);
        assert_eq!(pcm_from_wav_bytes(&[0u8; 10]), None);
        assert_eq!(pcm_from_wav_bytes(&[]), None);
    }

    #[test]
    fn pcm_duration_ms_matches_16k_mono_16bit_rate() {
        // 16000 样本/秒 × 2 字节/样本 = 32000 字节/秒 = 32 字节/毫秒。
        assert_eq!(pcm_duration_ms(32_000), 1000); // 1s
        assert_eq!(pcm_duration_ms(16_000), 500); // 0.5s
        assert_eq!(pcm_duration_ms(32), 1); // 1ms
        assert_eq!(pcm_duration_ms(0), 0);
    }

    #[test]
    fn queued_chain_press_allowed_within_grace_blocked_after() {
        use super::{is_queued_chain_press, HOTKEY_QUEUE_GRACE_MS};
        use std::time::{Duration, Instant};
        let cooldown_ms = crate::coordinator::POST_SESSION_COOLDOWN_MS;
        // 模拟 end_session 收尾：cooldown_until = idle + POST_SESSION_COOLDOWN_MS。
        let idle = Instant::now();
        let cooldown_until = idle + Duration::from_millis(cooldown_ms);
        // 排队接力：识别中按下、会话收尾后 ~0ms 被取出处理 → 放行开录下一条。
        assert!(is_queued_chain_press(idle, cooldown_until));
        // grace 窗口边缘内 → 仍放行。
        assert!(is_queued_chain_press(
            idle + Duration::from_millis(HOTKEY_QUEUE_GRACE_MS - 1),
            cooldown_until
        ));
        // #545 误触：会话结束后隔了人类反应时间才物理按下 → 仍在冷却期但超出 grace → 拦截。
        assert!(!is_queued_chain_press(
            idle + Duration::from_millis(300),
            cooldown_until
        ));
        // 冷却已过：函数对超期返回 false（此时调用方本就放行，从严也安全）。
        assert!(!is_queued_chain_press(
            idle + Duration::from_millis(cooldown_ms + 10),
            cooldown_until
        ));
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
            ChineseScriptPreference::Auto,
            crate::types::WindowsInsertionMode::SendInput,
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn streaming_disabled_for_windows_tsf_insertion_mode() {
        assert!(!streaming_insert_eligible(
            true,
            false,
            PolishMode::Light,
            false,
            ChineseScriptPreference::Auto,
            crate::types::WindowsInsertionMode::Tsf,
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn streaming_disabled_for_windows_paste_insertion_mode() {
        assert!(!streaming_insert_eligible(
            true,
            false,
            PolishMode::Light,
            false,
            ChineseScriptPreference::Auto,
            crate::types::WindowsInsertionMode::Paste,
        ));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn streaming_ignores_windows_insertion_mode_on_non_windows() {
        for mode in [
            crate::types::WindowsInsertionMode::Tsf,
            crate::types::WindowsInsertionMode::Paste,
        ] {
            assert!(streaming_insert_eligible(
                true,
                false,
                PolishMode::Light,
                false,
                ChineseScriptPreference::Auto,
                mode,
            ));
        }
    }

    #[test]
    fn streaming_script_gate_blocks_only_traditional() {
        // Traditional（s2t）有一简对多繁的真歧义，必须走一次性路径做全文 OpenCC
        // 转换（issue #643）；Simplified（t2s）近乎逐字，on_delta 就地转换即可，
        // 不再挡流式（用户反馈：固定简体导致流式静默失效）。
        assert!(!streaming_insert_eligible(
            true,
            false,
            PolishMode::Light,
            false,
            ChineseScriptPreference::Traditional,
            crate::types::WindowsInsertionMode::SendInput,
        ));
        for pref in [
            ChineseScriptPreference::Auto,
            ChineseScriptPreference::Simplified,
        ] {
            assert!(streaming_insert_eligible(
                true,
                false,
                PolishMode::Light,
                false,
                pref,
                crate::types::WindowsInsertionMode::SendInput,
            ));
        }
    }

    #[test]
    fn polish_output_honors_chinese_script_preference() {
        // issue #643：polish 模式（非 Raw、polish 成功）的成品也按用户字形偏好确定性转换，
        // 不再依赖 LLM 提示——繁中用户因此每次都拿到繁体。
        let finalize = |pref| {
            finalize_polished_text(
                "学习".to_string(),
                false, // translation_active
                false, // raw_uses_llm
                PolishMode::Structured,
                &None, // polish 成功
                pref,
                &[],
                false, // already_streamed
            )
        };
        // 繁体偏好：学习 → 學習（OpenCC S2t），至少不再含简体「学/习」。
        let trad = finalize(ChineseScriptPreference::Traditional);
        assert!(
            !trad.contains('学') && !trad.contains('习'),
            "traditional pref left simplified chars: {trad}"
        );
        // 简体偏好：保持简体（输入已是简体，T2s 无变化）。
        let simp = finalize(ChineseScriptPreference::Simplified);
        assert!(
            simp.contains('学') && simp.contains('习'),
            "simplified pref: {simp}"
        );
        // Auto：不转换，对默认用户零影响。
        assert_eq!(finalize(ChineseScriptPreference::Auto), "学习");
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
