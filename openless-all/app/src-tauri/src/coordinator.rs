//! Dictation coordinator.
//!
//! Mirrors the Swift `DictationCoordinator` state machine. Single owner of
//! session state. Receives hotkey edges, drives recorder + ASR + polish +
//! insertion, persists history, emits `capsule:state` events to the capsule
//! window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use parking_lot::Mutex;
use tauri::{async_runtime, AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::asr::{
    DictionaryHotword, RawTranscript, VolcengineCredentials, VolcengineStreamingASR,
    WhisperBatchASR,
};
use crate::hotkey::{HotkeyEvent, HotkeyMonitor};
use crate::insertion::TextInserter;
use crate::persistence::{
    CredentialAccount, CredentialsVault, DictionaryStore, HistoryStore, PreferencesStore,
};

use crate::polish::{OpenAICompatibleConfig, OpenAICompatibleLLMProvider};
use crate::qa_hotkey::{QaHotkeyError, QaHotkeyEvent, QaHotkeyMonitor};
use crate::recorder::{Recorder, RecorderError};
use crate::selection::{capture_selection, SelectionContext};
use crate::types::{
    CapsulePayload, CapsuleState, DictationSession, HotkeyCapability, HotkeyMode, HotkeyStatus,
    HotkeyStatusState, InsertStatus, PolishMode,
};
#[cfg(target_os = "windows")]
use crate::windows_ime_ipc::ImeSubmitTarget;
#[cfg(target_os = "windows")]
use crate::windows_ime_session::{PreparedWindowsImeSession, WindowsImeSessionController};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPhase {
    Idle,
    Starting,
    Listening,
    Processing,
    /// 已经过了最后一次 cancel 检查、即将 / 正在调用 inserter.insert 的窗口。
    /// cancel_session 在此阶段拒绝介入：Cmd+V 模拟点击已开始或已发出，
    /// 无法撤销，硬把 cancelled=true 也救不回来，只会让 UI 出现 cancelled
    /// 但实际还是插入了的诡异状态。详见 PR 修 Codex audit HIGH #2。
    Inserting,
}

enum ActiveAsr {
    Volcengine(Arc<VolcengineStreamingASR>),
    Whisper(Arc<WhisperBatchASR>),
}

struct SessionResource<T> {
    session_id: u64,
    resource: T,
}

impl<T> SessionResource<T> {
    fn new(session_id: u64, resource: T) -> Self {
        Self {
            session_id,
            resource,
        }
    }

    fn into_inner(self) -> T {
        self.resource
    }
}

struct SessionState {
    phase: SessionPhase,
    started_at: Instant,
    /// Starting 阶段（ASR 握手中）按下 stop 边沿（toggle 第二次按 / hold 松开）→
    /// 等握手完成 phase=Listening 后立刻 end_session，不丢边沿。issue #51。
    pending_stop: bool,
    /// 用户在 Processing 阶段按 Esc 取消：end_session 在 polish/insert 检查点跳过插入 +
    /// 跳过 history.append。issue #52。
    cancelled: bool,
    focus_target: Option<usize>,
    /// 单调递增的 session id。begin_session 自增。
    /// recorder error monitor 持有 captured id，处理时若与当前不等说明
    /// 是上一 session 的迟到错误，必须 drop，不要 abort 当前 active session。
    session_id: u64,
    /// 用户开始 dictation 时所处的前台 app 标签（"Mail (com.apple.mail)" / Windows 窗口标题）。
    /// 用作 LLM polish/translate 的上下文前提，让模型按 app 调风格。详见 issue #116。
    front_app: Option<String>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            phase: SessionPhase::Idle,
            started_at: Instant::now(),
            pending_stop: false,
            cancelled: false,
            focus_target: None,
            session_id: 0,
            front_app: None,
        }
    }
}

pub struct Coordinator {
    inner: Arc<Inner>,
}

struct Inner {
    app: Mutex<Option<AppHandle>>,
    history: HistoryStore,
    prefs: PreferencesStore,
    vocab: DictionaryStore,
    inserter: TextInserter,
    #[cfg(target_os = "windows")]
    windows_ime: WindowsImeSessionController,
    #[cfg(target_os = "windows")]
    prepared_windows_ime_session: Arc<Mutex<Vec<PreparedWindowsImeSessionSlot>>>,
    state: Mutex<SessionState>,
    asr: Mutex<Option<SessionResource<ActiveAsr>>>,
    recorder: Mutex<Option<SessionResource<Recorder>>>,
    hotkey: Mutex<Option<HotkeyMonitor>>,
    hotkey_status: Mutex<HotkeyStatus>,
    hotkey_trigger_held: AtomicBool,
    /// 翻译模式触发标志。每次 begin_session 重置为 false；hotkey 监听器在
    /// Listening / Starting 阶段看到 Shift down 边沿时 set true。
    /// end_session 在调 polish/translate 前读这个 flag + translation_target_language
    /// 决定走哪条管线。详见 issue #4。
    translation_modifier_seen: AtomicBool,
    /// 划词语音问答（issue #118）：与 dictation hotkey 平行的全局快捷键
    /// 监听器（global-hotkey crate）。`None` 表示功能关闭或还没成功安装。
    qa_hotkey: Mutex<Option<QaHotkeyMonitor>>,
    /// QA 单独的 session 状态，与 dictation 的 SessionPhase 不冲突。
    qa_state: Mutex<QaSessionState>,
    /// 最近一次应用到 capsule 窗口的几何状态。避免录音 level tick 反复触发
    /// resize / reposition。
    capsule_layout: Mutex<Option<CapsuleLayoutState>>,
    /// QA 用的 ASR 句柄（始终是 Volcengine 流式）。
    qa_asr: Mutex<Option<Arc<VolcengineStreamingASR>>>,
    /// QA 用的 Recorder 句柄。
    qa_recorder: Mutex<Option<Recorder>>,
    /// QA SSE 流取消标志。begin_qa_session 重置为 false；cancel_qa_session 设 true；
    /// polish::chat_completion_history_streaming 的 loop 每帧检查，true 时 break loop
    /// 避免取消后 LLM 仍 drain HTTP body 烧 token。详见 issue #161。
    qa_stream_cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QaPhase {
    Idle,
    Recording,
    Processing,
}

struct QaSessionState {
    phase: QaPhase,
    cancelled: bool,
    selection: Option<SelectionContext>,
    front_app: Option<String>,
    /// 用于忽略迟到的 RMS / runtime error。
    session_id: u64,
    /// QA 浮窗是否被用户钉住（pinned）。pinned=true 时不自动隐藏。
    pinned: bool,
    /// 浮窗是否对用户可见。Cmd+Shift+; 边沿 toggle 此 flag；
    /// 主听写 hotkey（rightOption）边沿来时，看这个 flag 决定是走 QA 还是走 dictation。
    /// 详见 issue #118 v2。
    panel_visible: bool,
    /// 多轮对话累积。每轮 user→assistant 加两条；关浮窗清空。
    messages: Vec<crate::types::QaChatMessage>,
}

impl Default for QaSessionState {
    fn default() -> Self {
        Self {
            phase: QaPhase::Idle,
            cancelled: false,
            selection: None,
            front_app: None,
            session_id: 0,
            pinned: false,
            panel_visible: false,
            messages: Vec::new(),
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct PreparedWindowsImeSessionSlot {
    session_id: u64,
    prepared: PreparedWindowsImeSession,
}

impl Coordinator {
    pub fn new() -> Self {
        let history = HistoryStore::new().unwrap_or_else(|e| {
            log::error!("[coord] HistoryStore init failed: {e}; falling back to empty");
            HistoryStore::new().expect("history store init")
        });
        let prefs = PreferencesStore::new().expect("preferences store init");
        let vocab = DictionaryStore::new().expect("dictionary store init");

        Self {
            inner: Arc::new(Inner {
                app: Mutex::new(None),
                history,
                prefs,
                vocab,
                inserter: TextInserter::new(),
                #[cfg(target_os = "windows")]
                windows_ime: WindowsImeSessionController::new(),
                #[cfg(target_os = "windows")]
                prepared_windows_ime_session: Arc::new(Mutex::new(Vec::new())),
                state: Mutex::new(SessionState::default()),
                asr: Mutex::new(None),
                recorder: Mutex::new(None),
                hotkey: Mutex::new(None),
                hotkey_status: Mutex::new(HotkeyStatus::default()),
                hotkey_trigger_held: AtomicBool::new(false),
                translation_modifier_seen: AtomicBool::new(false),
                qa_hotkey: Mutex::new(None),
                qa_state: Mutex::new(QaSessionState::default()),
                capsule_layout: Mutex::new(None),
                qa_asr: Mutex::new(None),
                qa_recorder: Mutex::new(None),
                qa_stream_cancelled: Arc::new(AtomicBool::new(false)),
            }),
        }
    }

    pub fn bind_app(&self, handle: AppHandle) {
        *self.inner.app.lock() = Some(handle);
    }

    pub fn start_hotkey_listener(&self) {
        // 起一个守护线程，反复尝试安装 hotkey hook。Accessibility 一被授予就立即生效，
        // 用户不需要手动重启 OpenLess。
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-hotkey-supervisor".into())
            .spawn(move || hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_hotkey_listener(&self) {
        self.inner.hotkey.lock().take();
    }

    /// 启动 QA hotkey supervisor（issue #118）。和 `start_hotkey_listener` 平行：
    /// 守护线程反复尝试注册（用户可能改了组合键），失败则 3s 后重试。
    pub fn start_qa_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-qa-hotkey-supervisor".into())
            .spawn(move || qa_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_qa_hotkey_listener(&self) {
        // QaHotkeyMonitor::drop 在 macOS 底层是 Carbon RemoveEventHotKey，要求主线程。
        // RunEvent::Exit 回调不保证在 AppKit 主线程跑，drop 漏到 tokio worker 上会
        // 触发 macOS dispatch_assert_queue_fail SIGTRAP。包到 run_on_main_thread 让
        // drop 在主线程发生；AppHandle 已 None 时直接 drop（最坏 crash 也是退出时刻）。
        // 详见 issue #169。
        let app = self.inner.app.lock().clone();
        if let Some(app) = app {
            let inner = Arc::clone(&self.inner);
            let _ = app.run_on_main_thread(move || {
                inner.qa_hotkey.lock().take();
            });
        } else {
            self.inner.qa_hotkey.lock().take();
        }
    }

    /// 用户在设置里改了 QA 组合键时调用。先持久化（由 prefs.set 完成），
    /// 然后通知活着的 monitor 重新注册；monitor 不存在时 supervisor 会自然
    /// 在下一次循环里读到新的 prefs。
    pub fn update_qa_hotkey_binding(&self) {
        let prefs = self.inner.prefs.get();
        let Some(binding) = prefs.qa_hotkey.clone() else {
            // 用户把功能关了 → 直接 drop monitor。drop 也得在主线程，否则 Carbon
            // unregister 会失败/UB。
            let app = self.inner.app.lock().clone();
            if let Some(app) = app {
                let inner_clone = Arc::clone(&self.inner);
                let _ = app.run_on_main_thread(move || {
                    inner_clone.qa_hotkey.lock().take();
                });
            } else {
                self.inner.qa_hotkey.lock().take();
            }
            log::info!("[coord] QA hotkey 已关闭");
            return;
        };
        // global-hotkey crate 的 manager.register/unregister 必须主线程跑。
        // 没在主线程会让 Carbon 句柄注册看似成功但事件不派发。
        let app = self.inner.app.lock().clone();
        let Some(app) = app else {
            log::warn!("[coord] update QA hotkey binding: AppHandle 未 bind，跳过");
            return;
        };
        let inner_clone = Arc::clone(&self.inner);
        let binding_for_main = binding.clone();
        let _ = app.run_on_main_thread(move || {
            // 路径 1：当前已有 monitor → 在主线程换绑定。
            if let Some(monitor) = inner_clone.qa_hotkey.lock().as_ref() {
                if let Err(e) = monitor.update_binding(binding_for_main.clone()) {
                    log::warn!("[coord] update QA hotkey binding 失败: {e}");
                }
                return;
            }
            // 路径 2：之前还没装上 → 主线程上重装一次（supervisor 也会重试，
            // 但用户体感更快：set_qa_hotkey 命令一返回，hotkey 立即生效）。
            let (tx, rx) = mpsc::channel::<QaHotkeyEvent>();
            match QaHotkeyMonitor::start(binding_for_main, tx) {
                Ok(monitor) => {
                    *inner_clone.qa_hotkey.lock() = Some(monitor);
                    log::info!("[coord] QA hotkey listener installed on main thread (via update)");
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
        });
    }

    /// 给前端 Settings 渲染当前 QA 快捷键 label（如 "Cmd+Shift+;"）。
    /// `qa_hotkey == None` 时返回空串，UI 据此显示「未启用」。
    pub fn qa_hotkey_label(&self) -> String {
        self.inner
            .prefs
            .get()
            .qa_hotkey
            .as_ref()
            .map(|b| b.display_label())
            .unwrap_or_default()
    }

    /// 用户点 ✕ / 按 Esc 关 QA 浮窗时调。等价于：取消任何进行中的录音 +
    /// 清空多轮对话历史 + 隐藏窗口。详见 issue #118 v2。
    pub fn qa_window_dismiss(&self) {
        close_qa_panel(&self.inner);
    }

    /// 用户点 📌 切换 pinned 状态。pinned=true 时浮窗不自动隐藏。
    pub fn qa_window_pin(&self, pinned: bool) {
        self.inner.qa_state.lock().pinned = pinned;
        log::info!("[coord] QA window pinned={pinned}");
    }

    pub fn history(&self) -> &HistoryStore {
        &self.inner.history
    }
    pub fn prefs(&self) -> &PreferencesStore {
        &self.inner.prefs
    }
    pub fn vocab(&self) -> &DictionaryStore {
        &self.inner.vocab
    }

    pub fn update_hotkey_binding(&self) {
        if let Some(monitor) = self.inner.hotkey.lock().as_ref() {
            monitor.update_binding(self.inner.prefs.get().hotkey);
        }
    }

    pub fn hotkey_status(&self) -> HotkeyStatus {
        self.inner.hotkey_status.lock().clone()
    }

    pub fn hotkey_capability(&self) -> HotkeyCapability {
        HotkeyMonitor::capability()
    }

    pub async fn start_dictation(&self) -> Result<(), String> {
        begin_session(&self.inner).await
    }

    pub async fn stop_dictation(&self) -> Result<(), String> {
        if self.inner.state.lock().phase == SessionPhase::Starting {
            request_stop_during_starting(&self.inner, "manual stop");
            return Ok(());
        }
        end_session(&self.inner).await
    }

    pub fn cancel_dictation(&self) {
        cancel_session(&self.inner);
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
        handle_pressed(&self.inner).await;
        handle_released(&self.inner).await;
        cancel_session(&self.inner);
        Ok(())
    }

    pub async fn repolish(&self, raw_text: String, mode: PolishMode) -> Result<String, String> {
        let hotwords = enabled_phrases(&self.inner);
        let working_languages = self.inner.prefs.get().working_languages;
        // repolish 是历史记录里手动重新润色，不再绑定原 session 的前台 app；
        // 当下用户调起的 app 才是相关上下文（如果可拿）。
        let front_app = capture_frontmost_app();
        polish_text(
            &raw_text,
            mode,
            &hotwords,
            &working_languages,
            front_app.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
    }
}

// ─────────────────────────── hotkey bridging ───────────────────────────

fn hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts: u32 = 0;
    let capability = HotkeyMonitor::capability();
    loop {
        if inner.hotkey.lock().is_some() {
            return;
        }
        *inner.hotkey_status.lock() = HotkeyStatus {
            adapter: capability.adapter,
            state: HotkeyStatusState::Starting,
            message: Some(format!("正在安装全局快捷键监听（第 {} 次）", attempts + 1)),
            last_error: None,
        };
        let (tx, rx) = mpsc::channel::<HotkeyEvent>();
        let binding = inner.prefs.get().hotkey;
        match HotkeyMonitor::start(binding, tx) {
            Ok(monitor) => {
                let adapter = monitor.kind();
                *inner.hotkey.lock() = Some(monitor);
                *inner.hotkey_status.lock() = HotkeyStatus {
                    adapter,
                    state: HotkeyStatusState::Installed,
                    message: Some(format!("{} 已安装", adapter.display_name())),
                    last_error: None,
                };
                log::info!(
                    "[coord] hotkey listener installed (after {} attempt(s))",
                    attempts + 1
                );
                let inner_clone = Arc::clone(&inner);
                std::thread::Builder::new()
                    .name("openless-hotkey-bridge".into())
                    .spawn(move || hotkey_bridge_loop(inner_clone, rx))
                    .ok();
                return;
            }
            Err(e) => {
                attempts += 1;
                let error_message = e.message.clone();
                *inner.hotkey_status.lock() = HotkeyStatus {
                    adapter: capability.adapter,
                    state: HotkeyStatusState::Failed,
                    message: Some(error_message.clone()),
                    last_error: Some(e),
                };
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] hotkey listener attempt #{attempts} failed: {}; retrying in 3s",
                        error_message
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

// ─────────────────────────── QA hotkey supervisor ───────────────────────────

fn qa_hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts: u32 = 0;
    loop {
        // 用户已经把 QA 关掉就睡着等 prefs 改动；改动通过 update_qa_hotkey_binding 唤醒。
        let binding = match inner.prefs.get().qa_hotkey.clone() {
            Some(b) => b,
            None => {
                inner.qa_hotkey.lock().take();
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        };

        if inner.qa_hotkey.lock().is_some() {
            // 已注册成功 → 不重复装；睡 5s 复查（ binding 变化由 update 路径手动触发 ）。
            std::thread::sleep(std::time::Duration::from_secs(5));
            continue;
        }

        // global-hotkey crate 在 macOS 走 Carbon RegisterEventHotKey，要求 manager
        // 在主线程构造，否则 register() 看起来 Ok 但事件根本不会派发——这是 issue #118
        // PR #119 第一版漏掉的关键步骤，导致用户按了 hotkey 完全无反应。这里通过
        // run_on_main_thread 把 QaHotkeyMonitor::start 跳到主线程跑，结果再回 channel。
        let app = inner.app.lock().clone();
        let app = match app {
            Some(a) => a,
            None => {
                // 启动期 AppHandle 还没 bind，再等。
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        };

        let (tx, rx) = mpsc::channel::<QaHotkeyEvent>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<QaHotkeyMonitor, QaHotkeyError>>(1);
        let binding_for_main = binding.clone();
        let _ = app.run_on_main_thread(move || {
            let result = QaHotkeyMonitor::start(binding_for_main, tx);
            let _ = init_tx.send(result);
        });

        // run_on_main_thread 是 fire-and-forget；等主线程跑完结果回来。给 5s 上限避免
        // 主线程繁忙时 supervisor 永久阻塞。
        let init_result = match init_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] QA hotkey 第 {attempts} 次注册超时（主线程未回执）；3s 后重试"
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }
        };

        match init_result {
            Ok(monitor) => {
                *inner.qa_hotkey.lock() = Some(monitor);
                log::info!(
                    "[coord] QA hotkey listener installed on main thread (after {} attempt(s))",
                    attempts + 1
                );
                let inner_clone = Arc::clone(&inner);
                std::thread::Builder::new()
                    .name("openless-qa-hotkey-bridge".into())
                    .spawn(move || qa_hotkey_bridge_loop(inner_clone, rx))
                    .ok();
                attempts = 0;
            }
            Err(e) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!("[coord] QA hotkey 第 {attempts} 次注册失败: {e}; 3s 后重试");
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

fn qa_hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<QaHotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        let inner_cloned = Arc::clone(&inner);
        match evt {
            QaHotkeyEvent::Pressed => {
                async_runtime::spawn(async move { handle_qa_hotkey_pressed(&inner_cloned).await });
            }
        }
    }
}

async fn handle_qa_hotkey_pressed(inner: &Arc<Inner>) {
    // QA hotkey（默认 Cmd+Shift+;）现在只 toggle 浮窗可见性。
    // 浮窗内的录音 / 提问由 Option 边沿驱动（handle_pressed_edge → handle_qa_option_edge）。
    let visible = inner.qa_state.lock().panel_visible;
    log::info!("[coord] QA hotkey edge (panel_visible={visible})");
    if visible {
        close_qa_panel(inner);
    } else {
        open_qa_panel(inner);
    }
}

/// 浮窗可见时，主听写 hotkey（rightOption）边沿改打到这里：
/// Idle → 录音 / Recording → 停录音并提问。
async fn handle_qa_option_edge(inner: &Arc<Inner>) {
    let phase = inner.qa_state.lock().phase;
    log::info!("[coord] QA option edge (phase={phase:?})");
    match phase {
        QaPhase::Idle => {
            let _ = begin_qa_session(inner).await;
        }
        QaPhase::Recording => {
            let _ = end_qa_session(inner).await;
        }
        // Processing 阶段再次按键忽略（避免与正在跑的 LLM 冲突）。
        QaPhase::Processing => {}
    }
}

fn open_qa_panel(inner: &Arc<Inner>) {
    {
        let mut state = inner.qa_state.lock();
        state.panel_visible = true;
        state.phase = QaPhase::Idle;
        state.cancelled = false;
        state.messages.clear();
        state.selection = None;
        state.front_app = capture_frontmost_app();
    }
    // 先把胶囊清干净，避免主听写上一次 Done 状态残留的 message/insertedChars
    // 在 QA Done 阶段被 capsule UI 错误复用（"已之一粘贴这个 0" 那种）。
    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
    if let Some(app) = inner.app.lock().clone() {
        crate::show_qa_window(&app, "idle");
        let _ = app.emit_to(
            "qa",
            "qa:state",
            serde_json::json!({
                "kind": "idle",
                "messages": Vec::<crate::types::QaChatMessage>::new(),
            }),
        );
    }
    log::info!("[coord] QA panel opened (awaiting Option to record)");
}

fn close_qa_panel(inner: &Arc<Inner>) {
    cancel_qa_session(inner);
    {
        let mut state = inner.qa_state.lock();
        state.panel_visible = false;
        state.pinned = false;
        state.messages.clear();
        state.selection = None;
        state.front_app = None;
        state.phase = QaPhase::Idle;
        state.cancelled = false;
    }
    if let Some(app) = inner.app.lock().clone() {
        crate::hide_qa_window(&app);
    }
    // 胶囊一同收掉，避免浮窗关了胶囊还在显示。
    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
    log::info!("[coord] QA panel closed, history cleared");
}

fn hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<HotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        let inner_cloned = Arc::clone(&inner);
        match evt {
            HotkeyEvent::Pressed => {
                async_runtime::spawn(async move { handle_pressed_edge(&inner_cloned).await });
            }
            HotkeyEvent::Released => {
                async_runtime::spawn(async move { handle_released_edge(&inner_cloned).await });
            }
            HotkeyEvent::Cancelled => {
                cancel_session(&inner_cloned);
            }
            HotkeyEvent::TranslationModifierPressed => {
                // 仅在 Starting / Listening 阶段把 Shift 边沿计入"翻译模式触发"。
                // Idle 阶段按 Shift 不应该影响下一段录音；Processing/Inserting 已经过了
                // 决定走哪条管线的检查点，再 set 也没意义。
                let phase = inner_cloned.state.lock().phase;
                if matches!(phase, SessionPhase::Starting | SessionPhase::Listening) {
                    inner_cloned
                        .translation_modifier_seen
                        .store(true, Ordering::SeqCst);
                    log::info!("[coord] translation modifier seen during {phase:?}");
                }
            }
        }
    }
}

async fn handle_pressed_edge(inner: &Arc<Inner>) {
    let was_held = inner.hotkey_trigger_held.swap(true, Ordering::SeqCst);
    if !was_held {
        // 路由：QA 浮窗可见时，rightOption 边沿走 QA；否则走主听写。详见 issue #118 v2。
        let panel_visible = inner.qa_state.lock().panel_visible;
        if panel_visible {
            handle_qa_option_edge(inner).await;
        } else {
            handle_pressed(inner).await;
        }
    }
}

async fn handle_pressed(inner: &Arc<Inner>) {
    let mode = inner.prefs.get().hotkey.mode;
    let phase = inner.state.lock().phase;
    log::info!("[coord] hotkey pressed (mode={mode:?}, phase={phase:?})");
    match (mode, phase) {
        (HotkeyMode::Toggle, SessionPhase::Idle) => {
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

async fn handle_released_edge(inner: &Arc<Inner>) {
    let was_held = inner.hotkey_trigger_held.swap(false, Ordering::SeqCst);
    if was_held {
        // QA 浮窗可见时，Option 行为是 press-toggle（不分 hold/release），release 边沿忽略。
        let panel_visible = inner.qa_state.lock().panel_visible;
        if panel_visible {
            return;
        }
        handle_released(inner).await;
    }
}

async fn handle_released(inner: &Arc<Inner>) {
    let mode = inner.prefs.get().hotkey.mode;
    let phase = inner.state.lock().phase;
    log::info!("[coord] hotkey released (mode={mode:?}, phase={phase:?})");
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

fn request_stop_during_starting(inner: &Arc<Inner>, reason: &str) {
    {
        let mut state = inner.state.lock();
        if state.phase != SessionPhase::Starting {
            return;
        }
        state.pending_stop = true;
    }
    log::info!("[coord] {reason} during Starting — queued");
    stop_recorder_if_pending_start_stop(inner);
}

fn take_session_resource<T>(slot: &mut Option<SessionResource<T>>, session_id: u64) -> Option<T> {
    if slot
        .as_ref()
        .map(|resource| resource.session_id == session_id)
        .unwrap_or(false)
    {
        slot.take().map(SessionResource::into_inner)
    } else {
        None
    }
}

fn store_asr_for_session(inner: &Arc<Inner>, session_id: u64, asr: ActiveAsr) {
    *inner.asr.lock() = Some(SessionResource::new(session_id, asr));
}

fn take_asr_for_session(inner: &Arc<Inner>, session_id: u64) -> Option<ActiveAsr> {
    let mut slot = inner.asr.lock();
    take_session_resource(&mut slot, session_id)
}

fn cancel_active_asr(asr: ActiveAsr) {
    match asr {
        ActiveAsr::Volcengine(v) => v.cancel(),
        ActiveAsr::Whisper(w) => w.cancel(),
    }
}

fn cancel_asr_for_session(inner: &Arc<Inner>, session_id: u64) {
    if let Some(asr) = take_asr_for_session(inner, session_id) {
        cancel_active_asr(asr);
    }
}

fn store_recorder_for_session(inner: &Arc<Inner>, session_id: u64, recorder: Recorder) {
    *inner.recorder.lock() = Some(SessionResource::new(session_id, recorder));
}

fn take_recorder_for_session(inner: &Arc<Inner>, session_id: u64) -> Option<Recorder> {
    let mut slot = inner.recorder.lock();
    take_session_resource(&mut slot, session_id)
}

fn stop_recorder_for_session(inner: &Arc<Inner>, session_id: u64) {
    if let Some(recorder) = take_recorder_for_session(inner, session_id) {
        recorder.stop();
    }
}

fn discard_startup_resources_for_session(inner: &Arc<Inner>, session_id: u64) {
    stop_recorder_for_session(inner, session_id);
    cancel_asr_for_session(inner, session_id);
}

fn stop_recorder_if_pending_start_stop(inner: &Arc<Inner>) {
    let (should_stop, session_id) = {
        let state = inner.state.lock();
        (
            state.phase == SessionPhase::Starting && state.pending_stop,
            state.session_id,
        )
    };
    if !should_stop {
        return;
    }
    if let Some(rec) = take_recorder_for_session(inner, session_id) {
        rec.stop();
        let elapsed = inner.state.lock().started_at.elapsed().as_millis() as u64;
        emit_capsule(inner, CapsuleState::Transcribing, 0.0, elapsed, None, None);
        log::info!("[coord] stopped recorder while ASR is still connecting");
    }
}

async fn handle_window_hotkey_event(
    inner: &Arc<Inner>,
    event_type: String,
    key: String,
    code: String,
    repeat: bool,
) -> Result<(), String> {
    if event_type == "keydown" && key == "Escape" {
        // Esc 路由（issue #161）：QA 浮窗可见时优先取消 QA（不动 dictation）；
        // 否则走 dictation 取消通路。之前无条件 cancel_session 导致 QA 浮窗
        // 按 Esc 杀的是 dictation 而 QA 流还在烧 token。
        let qa_active = {
            let st = inner.qa_state.lock();
            st.panel_visible || st.phase != QaPhase::Idle
        };
        if qa_active {
            close_qa_panel(inner);
        } else {
            cancel_session(inner);
        }
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (inner, event_type, key, code, repeat);
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        if !window_hotkey_fallback_enabled() {
            if event_type == "keydown" && !repeat {
                log::info!(
                    "[window-hotkey] ignored because Windows lifecycle owner is the low-level hook"
                );
            }
            return Ok(());
        }

        let trigger = inner.prefs.get().hotkey.trigger;
        if !window_key_matches_trigger(trigger, &key, &code) {
            return Ok(());
        }

        match event_type.as_str() {
            "keydown" => {
                if repeat {
                    return Ok(());
                }
                log::info!(
                    "[window-hotkey] pressed trigger={trigger:?} code={code} repeat={repeat}"
                );
                handle_pressed_edge(inner).await;
            }
            "keyup" => {
                log::info!("[window-hotkey] released trigger={trigger:?} code={code}");
                handle_released_edge(inner).await;
            }
            _ => {}
        }
        Ok(())
    }
}

fn window_hotkey_fallback_enabled() -> bool {
    crate::types::HotkeyCapability::current().explicit_fallback_available
}

#[cfg(any(target_os = "windows", test))]
fn window_key_matches_trigger(trigger: crate::types::HotkeyTrigger, key: &str, code: &str) -> bool {
    use crate::types::HotkeyTrigger;

    match trigger {
        HotkeyTrigger::RightControl => key == "Control" && code == "ControlRight",
        HotkeyTrigger::LeftControl => key == "Control" && code == "ControlLeft",
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => {
            (key == "Alt" || key == "AltGraph") && code == "AltRight"
        }
        HotkeyTrigger::LeftOption => (key == "Alt" || key == "AltGraph") && code == "AltRight",
        HotkeyTrigger::RightCommand => key == "Meta" && code == "MetaRight",
        HotkeyTrigger::Fn => key == "Control" && code == "ControlRight",
    }
}

// ─────────────────────────── session lifecycle ───────────────────────────

async fn begin_session(inner: &Arc<Inner>) -> Result<(), String> {
    let current_session_id = {
        let mut state = inner.state.lock();
        if state.phase != SessionPhase::Idle {
            return Ok(());
        }
        state.phase = SessionPhase::Starting;
        state.started_at = Instant::now();
        // 新会话清掉旧 pending_stop / cancelled，避免上一会话遗留触发奇怪行为
        state.pending_stop = false;
        state.cancelled = false;
        state.focus_target = capture_focus_target();
        // 自增 session_id；spawn 出去的 recorder error monitor 会捕获这个值，
        // 如果迟到错误到达时 id 已不匹配就 drop，不会误中止后续 session。
        state.session_id = state.session_id.wrapping_add(1);
        state.front_app = capture_frontmost_app();
        if let Some(label) = state.front_app.as_deref() {
            log::info!("[coord] front_app captured: {label}");
        }
        state.session_id
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

    emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);

    let active_asr = CredentialsVault::get_active_asr();

    if is_whisper_compatible_provider(&active_asr) {
        let (api_key, base_url, model) = read_whisper_credentials();
        let whisper = Arc::new(WhisperBatchASR::new(api_key, base_url, model));
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
        start_recorder_for_starting(inner, current_session_id, &active_asr, consumer)?;

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

fn start_recorder_for_starting(
    inner: &Arc<Inner>,
    session_id: u64,
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

    match Recorder::start(consumer, level_handler) {
        Ok((rec, runtime_errors)) => {
            store_recorder_for_session(inner, session_id, rec);
            spawn_recorder_error_monitor(inner, runtime_errors);
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
            inner.state.lock().phase = SessionPhase::Idle;
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            return Err(e.to_string());
        }
    }

    Ok(())
}

fn spawn_recorder_error_monitor(inner: &Arc<Inner>, rx: mpsc::Receiver<RecorderError>) {
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

/// QA 录音 runtime error 监听器。镜像 `spawn_recorder_error_monitor` 的语义但走 QA
/// 收尾路径（`finish_qa_with_error` 替代 `abort_recording_with_error`）。
/// 用 qa_state.session_id 守卫 stale 事件。详见 issue #168。
fn spawn_qa_recorder_error_monitor(inner: &Arc<Inner>, rx: mpsc::Receiver<RecorderError>) {
    let captured_session_id = inner.qa_state.lock().session_id;
    let inner = Arc::clone(inner);
    std::thread::Builder::new()
        .name("openless-qa-recorder-error-monitor".into())
        .spawn(move || {
            if let Ok(err) = rx.recv() {
                let current_session_id = inner.qa_state.lock().session_id;
                if captured_session_id != current_session_id {
                    log::warn!(
                        "[coord] QA recorder error from stale session {} dropped (current={}, err={})",
                        captured_session_id,
                        current_session_id,
                        err
                    );
                    return;
                }
                log::error!("[coord] QA recorder runtime error: {err}");
                finish_qa_with_error(&inner, format!("录音设备异常: {err}"));
            }
        })
        .ok();
}

fn abort_recording_with_error(inner: &Arc<Inner>, message: String) {
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

struct RecordingAbort {
    elapsed: u64,
    session_id: u64,
}

fn begin_recording_abort_before_restore(state: &mut SessionState) -> Option<RecordingAbort> {
    if state.cancelled
        || !matches!(
            state.phase,
            SessionPhase::Starting | SessionPhase::Listening
        )
    {
        return None;
    }
    state.cancelled = true;
    Some(RecordingAbort {
        elapsed: state.started_at.elapsed().as_millis() as u64,
        session_id: state.session_id,
    })
}

fn publish_abort_idle_after_restore(state: &mut SessionState, session_id: u64) {
    if state.session_id == session_id {
        state.phase = SessionPhase::Idle;
    }
}

async fn start_recorder_and_enter_listening(
    inner: &Arc<Inner>,
    session_id: u64,
    active_asr: &str,
    consumer: Arc<dyn crate::recorder::AudioConsumer>,
) -> Result<(), String> {
    start_recorder_for_starting(inner, session_id, active_asr, consumer)?;
    finish_starting_session(inner, session_id).await;
    Ok(())
}

async fn finish_starting_session(inner: &Arc<Inner>, session_id: u64) {
    // audit HIGH #1：转 Listening 之前在同一 lock 内检查 cancel race。
    // 之前是无条件 phase=Listening，会把 cancel_session 在 await 期间设的 Idle
    // 反向覆盖回 Listening → 用户的 cancel 边沿被吞掉。
    let outcome = {
        let mut state = inner.state.lock();
        if state.session_id != session_id {
            BeginOutcome::StaleContinuation
        } else if state.cancelled || state.phase != SessionPhase::Starting {
            BeginOutcome::CancelRaced
        } else {
            state.phase = SessionPhase::Listening;
            let pending = std::mem::replace(&mut state.pending_stop, false);
            if pending {
                BeginOutcome::PendingStop
            } else {
                BeginOutcome::Started
            }
        }
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

async fn end_session(inner: &Arc<Inner>) -> Result<(), String> {
    let current_session_id = {
        let mut state = inner.state.lock();
        if state.phase != SessionPhase::Listening {
            return Ok(());
        }
        state.phase = SessionPhase::Processing;
        state.session_id
    };

    let elapsed = inner.state.lock().started_at.elapsed().as_millis() as u64;
    emit_capsule(inner, CapsuleState::Transcribing, 0.0, elapsed, None, None);

    if let Some(rec) = take_recorder_for_session(inner, current_session_id) {
        rec.stop();
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

    let raw = match asr {
        ActiveAsr::Volcengine(asr) => {
            if let Err(e) = asr.send_last_frame().await {
                log::error!("[coord] send last frame failed: {e}");
            }
            match asr.await_final_result().await {
                Ok(r) => r,
                Err(e) => {
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
            }
        }
        ActiveAsr::Whisper(w) => match w.transcribe().await {
            Ok(r) => r,
            Err(e) => {
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
        },
    };

    // ASR 完成后 cancel 检查：用户在 transcribe 进行中按 Esc 时，这里就会命中。
    // 优先级高于 empty 检查 — 用户取消 → 静默丢弃，不写失败历史也不弹错误胶囊。
    if inner.state.lock().cancelled {
        log::info!("[coord] cancel detected after ASR — discarding transcript");
        restore_prepared_windows_ime_session(inner, current_session_id);
        inner.state.lock().phase = SessionPhase::Idle;
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
            app_bundle_id: None,
            app_name: None,
            insert_status: InsertStatus::Failed,
            error_code: Some("emptyTranscript".to_string()),
            duration_ms: Some(raw.duration_ms),
            dictionary_entry_count: Some(enabled_phrases(inner).len() as u32),
        };
        if let Err(e) = inner.history.append(session) {
            log::error!("[coord] history append failed: {e}");
        }
        emit_capsule(
            inner,
            CapsuleState::Error,
            0.0,
            elapsed,
            Some("ASR returned empty transcript".to_string()),
            None,
        );
        restore_prepared_windows_ime_session(inner, current_session_id);
        inner.state.lock().phase = SessionPhase::Idle;
        schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
        return Err("ASR returned empty transcript".to_string());
    }

    emit_capsule(inner, CapsuleState::Polishing, 0.0, elapsed, None, None);

    let prefs = inner.prefs.get();
    let mode = prefs.default_mode;
    let hotword_strs = enabled_phrases(inner);
    let working_languages = prefs.working_languages.clone();
    let front_app = inner.state.lock().front_app.clone();
    let translation_target = prefs.translation_target_language.trim().to_string();
    let translation_active =
        inner.translation_modifier_seen.load(Ordering::SeqCst) && !translation_target.is_empty();
    let (polished, polish_error) = if translation_active {
        log::info!(
            "[coord] translation mode → target=\u{300C}{}\u{300D} working={:?} front_app={:?}",
            translation_target,
            working_languages,
            front_app
        );
        translate_or_passthrough(
            &raw,
            &translation_target,
            &working_languages,
            front_app.as_deref(),
        )
        .await
    } else {
        polish_or_passthrough(
            &raw,
            mode,
            &hotword_strs,
            &working_languages,
            front_app.as_deref(),
        )
        .await
    };

    // 原子化最后一次 cancel 检查 + 转 Inserting：
    // 在同一 lock 内决定「丢弃」还是「进入 Inserting」。一旦设到 Inserting，
    // cancel_session 就拒绝介入（Cmd+V 已发出，撤销不掉）。这是 audit HIGH #2 的修复，
    // 之前 check 与 inserter.insert 之间有窗口期。
    let proceed_to_insert = {
        let mut state = inner.state.lock();
        if state.cancelled {
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
    let status = if focus_ready_for_paste {
        #[cfg(target_os = "windows")]
        {
            let ime_target = capture_ime_submit_target();
            insert_with_windows_ime_first(
                inner,
                current_session_id,
                &polished,
                restore_clipboard,
                allow_non_tsf_insertion_fallback,
                ime_target,
            )
            .await
        }
        #[cfg(not(target_os = "windows"))]
        {
            inner.inserter.insert(&polished, restore_clipboard)
        }
    } else {
        log::warn!(
            "[coord] original insertion target is not foreground; copied output without paste"
        );
        if allow_non_tsf_insertion_fallback {
            inner.inserter.copy_fallback(&polished)
        } else {
            InsertStatus::Failed
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
    )
    .map(str::to_string);
    let tsf_required_insert_failed = error_code.as_deref() == Some("windowsImeTsfRequired");

    let session = DictationSession {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        raw_transcript: raw.text.clone(),
        final_text: polished.clone(),
        mode,
        app_bundle_id: None,
        app_name: None,
        insert_status: status,
        error_code,
        duration_ms: Some(raw.duration_ms),
        // 历史详情页的"X 个热词"显示：用本次实际命中次数（每个匹配实例算一次），
        // 比"启用词条总数"更能反映本段口述命中了多少。u64 → u32 截断对单段听写足够。
        dictionary_entry_count: Some(total_hits.min(u32::MAX as u64) as u32),
    };
    if let Err(e) = inner.history.append(session) {
        log::error!("[coord] history append failed: {e}");
    }

    let done_message = if tsf_required_insert_failed {
        Some("TSF 未上屏，已禁止非 TSF 兜底".to_string())
    } else if polish_error.is_some() {
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
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);

    Ok(())
}

fn dictation_error_code(
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

fn cancel_session(inner: &Arc<Inner>) {
    let (phase, session_id) = {
        let mut state = inner.state.lock();
        let phase = state.phase;
        if phase == SessionPhase::Idle {
            return;
        }
        // Inserting 阶段已经过了最后一次 cancel 检查 + 锁内转换，inserter.insert 即将
        // 或正在执行 → Cmd+V 已发出无法撤销。这里硬设 cancelled=true 只会让 UI 显示
        // "已取消" 但文本仍被插入，与用户预期相反。直接拒绝，让本次 session 走完。
        if phase == SessionPhase::Inserting {
            log::info!("[coord] cancel ignored — already in Inserting phase, can't undo paste");
            return;
        }
        // Processing 阶段 cancel 不能直接干掉 in-flight polish task（已经 await 了），
        // 但可以打 cancelled 标记，让 end_session 在插入前检查并丢弃结果。
        state.cancelled = true;
        (phase, state.session_id)
    };

    stop_recorder_for_session(inner, session_id);
    cancel_asr_for_session(inner, session_id);
    restore_prepared_windows_ime_session(inner, session_id);
    // Processing 阶段保持 phase=Processing 让 end_session 自己走完检查 + 收尾；
    // 其他阶段直接转 Idle。
    if phase != SessionPhase::Processing {
        let mut state = inner.state.lock();
        state.phase = SessionPhase::Idle;
        state.focus_target = None;
    }
    emit_capsule(inner, CapsuleState::Cancelled, 0.0, 0, None, None);
    log::info!("[coord] session cancelled (was {phase:?})");
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
}

#[cfg(target_os = "windows")]
fn store_prepared_windows_ime_session(
    slots: &mut Vec<PreparedWindowsImeSessionSlot>,
    session_id: u64,
    prepared: PreparedWindowsImeSession,
) {
    slots.retain(|slot| slot.session_id != session_id);
    slots.push(PreparedWindowsImeSessionSlot {
        session_id,
        prepared,
    });
}

#[cfg(target_os = "windows")]
fn take_matching_prepared_windows_ime_session(
    slots: &mut Vec<PreparedWindowsImeSessionSlot>,
    session_id: u64,
) -> Option<PreparedWindowsImeSession> {
    let index = slots
        .iter()
        .position(|slot| slot.session_id == session_id)?;
    Some(slots.remove(index).prepared)
}

#[cfg(target_os = "windows")]
fn take_current_prepared_windows_ime_session_for_restore(
    slots: &mut Vec<PreparedWindowsImeSessionSlot>,
    session_id: u64,
    current_session_id: u64,
) -> Option<PreparedWindowsImeSession> {
    let prepared = take_matching_prepared_windows_ime_session(slots, session_id)?;
    if current_session_id == session_id {
        Some(prepared)
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn restore_prepared_windows_ime_session(inner: &Arc<Inner>, session_id: u64) {
    let state = inner.state.lock();
    let prepared = {
        let mut slot = inner.prepared_windows_ime_session.lock();
        take_current_prepared_windows_ime_session_for_restore(
            &mut slot,
            session_id,
            state.session_id,
        )
    };
    if let Some(prepared) = prepared {
        inner.windows_ime.restore_session(prepared);
    }
}

#[cfg(not(target_os = "windows"))]
fn restore_prepared_windows_ime_session(_inner: &Arc<Inner>, _session_id: u64) {}

#[cfg(target_os = "windows")]
async fn insert_with_windows_ime_first(
    inner: &Arc<Inner>,
    session_id: u64,
    polished: &str,
    restore_clipboard: bool,
    allow_non_tsf_insertion_fallback: bool,
    ime_target: Option<ImeSubmitTarget>,
) -> InsertStatus {
    let prepared = {
        let mut slot = inner.prepared_windows_ime_session.lock();
        take_matching_prepared_windows_ime_session(&mut slot, session_id)
    };
    let Some(prepared) = prepared else {
        log::warn!("[windows-ime] no prepared TSF session for this dictation");
        if should_try_non_tsf_insertion_fallback(
            allow_non_tsf_insertion_fallback,
            InsertStatus::Failed,
        ) {
            return insert_via_non_tsf_fallback(inner, polished, restore_clipboard);
        }
        log::warn!("[windows-ime] non-TSF insertion fallback is disabled; failing insert");
        return InsertStatus::Failed;
    };

    let request = crate::windows_ime_ipc::ImeSubmitRequest {
        session_id: Uuid::new_v4().to_string(),
        text: polished.to_string(),
        created_at: Utc::now().to_rfc3339(),
        target: ime_target,
    };

    let ime_status = match inner.windows_ime.submit_prepared(&prepared, request).await {
        Ok(status) => status,
        Err(error) => {
            log::warn!("[windows-ime] TSF submit failed: {error}");
            InsertStatus::Failed
        }
    };
    inner.windows_ime.restore_session(prepared);

    if ime_status == InsertStatus::Inserted {
        ime_status
    } else if should_try_non_tsf_insertion_fallback(allow_non_tsf_insertion_fallback, ime_status) {
        insert_via_non_tsf_fallback(inner, polished, restore_clipboard)
    } else {
        log::warn!("[windows-ime] TSF did not insert; non-TSF insertion fallback is disabled");
        InsertStatus::Failed
    }
}

#[cfg(target_os = "windows")]
fn should_try_non_tsf_insertion_fallback(
    allow_non_tsf_insertion_fallback: bool,
    ime_status: InsertStatus,
) -> bool {
    allow_non_tsf_insertion_fallback && ime_status != InsertStatus::Inserted
}

#[cfg(target_os = "windows")]
fn insert_via_non_tsf_fallback(
    inner: &Arc<Inner>,
    polished: &str,
    restore_clipboard: bool,
) -> InsertStatus {
    if inner.inserter.insert_via_unicode_keystrokes(polished) == InsertStatus::Inserted {
        log::info!("[windows-ime] TSF unavailable; inserted via Unicode SendInput");
        InsertStatus::Inserted
    } else {
        inner
            .inserter
            .insert_via_clipboard_fallback(polished, restore_clipboard)
    }
}

// ─────────────────────────── helpers ───────────────────────────

#[cfg(any(debug_assertions, test))]
fn hotkey_injection_dry_run_enabled() -> bool {
    std::env::var_os("OPENLESS_HOTKEY_INJECTION_DRY_RUN").is_some()
}

#[cfg(any(debug_assertions, test))]
fn debug_transcript_override_text() -> Option<String> {
    let path = std::env::var_os("OPENLESS_DEBUG_TRANSCRIPT_FILE")?;
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn ensure_microphone_permission(_inner: &Arc<Inner>) -> Result<(), String> {
    use crate::permissions::{self, PermissionStatus};

    #[cfg(target_os = "windows")]
    {
        if permissions::windows_microphone_access_explicitly_denied() {
            return Err("需要麦克风权限，当前状态: Denied".to_string());
        }
        return Ok(());
    }

    let status = permissions::check_microphone();
    if matches!(
        status,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        return Ok(());
    }

    // 听写路径不抢前台焦点：缺 mic 权限时直接请求系统授权，不再先 show_main_window。
    // 用户在设置页手动点“请求权限”仍走 request_microphone_from_foreground，那是显式操作。
    // 这里若系统不弹框，后续会通过 capsule error 引导用户主动去权限页处理。详见 #166。
    let requested = permissions::request_microphone();
    if matches!(
        requested,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        Ok(())
    } else {
        Err(format!("需要麦克风权限，当前状态: {requested:?}"))
    }
}

fn ensure_asr_credentials() -> Result<(), String> {
    let active_asr = CredentialsVault::get_active_asr();
    if is_whisper_compatible_provider(&active_asr) {
        let api_key = CredentialsVault::get(CredentialAccount::AsrApiKey)
            .ok()
            .flatten()
            .unwrap_or_default();
        if api_key.trim().is_empty() {
            return Err("请先在设置中填写 ASR 服务商 API Key".to_string());
        }
        return Ok(());
    }

    let creds = read_volc_credentials();
    if creds.app_id.trim().is_empty() || creds.access_token.trim().is_empty() {
        Err("请先在设置中填写火山引擎 ASR App Key 和 Access Key".to_string())
    } else {
        Ok(())
    }
}

/// `whisper` 是 OpenAI 原生；`siliconflow` / `zhipu` / `groq` 都暴露
/// OpenAI 兼容的 `/audio/transcriptions`，统一走 `WhisperBatchASR`。
/// 新增 OpenAI 兼容 ASR 时只需在这里加一项。
///
/// 注：DashScope 的 Qwen3-ASR-Flash 不在此列——它用 MultiModalConversation
/// (messages=[{content:[{audio:...}]}]) 协议，不是 Whisper multipart，需要
/// 单独 ASR 客户端，留给 V2。
fn is_whisper_compatible_provider(id: &str) -> bool {
    matches!(id, "whisper" | "siliconflow" | "zhipu" | "groq")
}

/// QA 路径专用：begin_qa_session 永远走 Volcengine 流式（低延迟要求），所以
/// 凭据校验也只看 Volcengine 字段，不依赖 active_asr。dictation 路径请用
/// `ensure_asr_credentials`。
fn ensure_qa_volcengine_credentials() -> Result<(), String> {
    let creds = read_volc_credentials();
    if creds.app_id.trim().is_empty() || creds.access_token.trim().is_empty() {
        Err("请先在设置中填写火山引擎 ASR App Key 和 Access Key".to_string())
    } else {
        Ok(())
    }
}

/// 润色文本；失败时返回原文 + 失败原因，调用方据此弹错误胶囊 + 写历史 error_code。
/// 之前固定返回 String，调用方拿不到失败信号 → 用户感知"为什么风格设置没生效"。issue #57。
async fn polish_or_passthrough(
    raw: &RawTranscript,
    mode: PolishMode,
    hotwords: &[String],
    working_languages: &[String],
    front_app: Option<&str>,
) -> (String, Option<String>) {
    if mode == PolishMode::Raw {
        return (raw.text.clone(), None);
    }
    match polish_text(&raw.text, mode, hotwords, working_languages, front_app).await {
        Ok(s) => (s, None),
        Err(e) => {
            let reason = e.to_string();
            log::error!("[coord] polish failed, falling back to raw: {reason}");
            (raw.text.clone(), Some(reason))
        }
    }
}

async fn polish_text(
    raw: &str,
    mode: PolishMode,
    hotwords: &[String],
    working_languages: &[String],
    front_app: Option<&str>,
) -> anyhow::Result<String> {
    let api_key = CredentialsVault::get(CredentialAccount::ArkApiKey)?.unwrap_or_default();
    let model = CredentialsVault::get(CredentialAccount::ArkModelId)?
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "deepseek-v3-2".to_string());
    let endpoint = resolve_ark_endpoint(&api_key)?;
    let base_url = endpoint
        .trim_end_matches("/chat/completions")
        .trim_end_matches('/')
        .to_string();

    let config = OpenAICompatibleConfig::new("ark", "Doubao Ark", base_url, api_key, model);
    let provider = OpenAICompatibleLLMProvider::new(config);
    Ok(provider
        .polish(raw, mode, hotwords, working_languages, front_app)
        .await?)
}

/// 翻译路径——和 polish 一样失败时返回原文 + 失败原因，避免"不丢字"约定被违反（CLAUDE.md）。
async fn translate_or_passthrough(
    raw: &RawTranscript,
    target_language: &str,
    working_languages: &[String],
    front_app: Option<&str>,
) -> (String, Option<String>) {
    match translate_text(&raw.text, target_language, working_languages, front_app).await {
        Ok(s) => (s, None),
        Err(e) => {
            let reason = e.to_string();
            log::error!("[coord] translate failed, falling back to raw: {reason}");
            (raw.text.clone(), Some(reason))
        }
    }
}

async fn translate_text(
    raw: &str,
    target_language: &str,
    working_languages: &[String],
    front_app: Option<&str>,
) -> anyhow::Result<String> {
    let api_key = CredentialsVault::get(CredentialAccount::ArkApiKey)?.unwrap_or_default();
    let model = CredentialsVault::get(CredentialAccount::ArkModelId)?
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "deepseek-v3-2".to_string());
    let endpoint = resolve_ark_endpoint(&api_key)?;
    let base_url = endpoint
        .trim_end_matches("/chat/completions")
        .trim_end_matches('/')
        .to_string();

    let config = OpenAICompatibleConfig::new("ark", "Doubao Ark", base_url, api_key, model);
    let provider = OpenAICompatibleLLMProvider::new(config);
    Ok(provider
        .translate_to(raw, target_language, working_languages, front_app)
        .await?)
}

fn read_whisper_credentials() -> (String, String, String) {
    let api_key = CredentialsVault::get(CredentialAccount::AsrApiKey)
        .ok()
        .flatten()
        .unwrap_or_default();
    let base_url = CredentialsVault::get(CredentialAccount::AsrEndpoint)
        .ok()
        .flatten()
        .unwrap_or_default();
    let model = CredentialsVault::get(CredentialAccount::AsrModel)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "whisper-1".to_string());
    (api_key, base_url, model)
}

fn read_volc_credentials() -> VolcengineCredentials {
    let app_id = CredentialsVault::get(CredentialAccount::VolcengineAppKey)
        .ok()
        .flatten()
        .unwrap_or_default();
    let access_token = CredentialsVault::get(CredentialAccount::VolcengineAccessKey)
        .ok()
        .flatten()
        .unwrap_or_default();
    let resource_id = CredentialsVault::get(CredentialAccount::VolcengineResourceId)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| VolcengineCredentials::default_resource_id().to_string());
    VolcengineCredentials {
        app_id,
        access_token,
        resource_id,
    }
}

fn enabled_hotwords(inner: &Arc<Inner>) -> Vec<DictionaryHotword> {
    inner
        .vocab
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|e| DictionaryHotword {
            phrase: e.phrase,
            enabled: e.enabled,
        })
        .collect()
}

// ─────────────────────────── QA session lifecycle ───────────────────────────

/// 划词语音问答会话（issue #118）。
///
/// 与 dictation 完全分离：
/// - 不进 SessionPhase（互不抢锁）
/// - 不写 history.json（除非 prefs.qa_save_history=true 才旁路写一条 placeholder）
/// - 用独立的 qa_recorder + qa_asr，复用现有 Volcengine ASR 通路
async fn begin_qa_session(inner: &Arc<Inner>) -> Result<(), String> {
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
        state.session_id = state.session_id.wrapping_add(1);
        state.front_app = capture_frontmost_app();
        state.selection = None;
    }
    // 重置 SSE 取消标志：上一轮可能 set 过的 true 留着会让本轮流式立即 break。
    inner.qa_stream_cancelled.store(false, Ordering::SeqCst);

    // 抓选区。每轮按 Option 都重新抓一次：用户多轮提问中可以重新选别处文字。
    // 浮窗 focus:false，原 app 仍是 frontmost，AX/Cmd+C fallback 都能拿到。
    let selection = capture_selection();
    let selection_preview_text = selection.as_ref().map(|s| s.text.clone());
    inner.qa_state.lock().selection = selection.clone();

    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            "qa",
            "qa:state",
            serde_json::json!({
                "kind": "recording",
                "selection_preview": selection_preview_text,
                "messages": messages,
            }),
        );
    }

    // 2. 凭据缺失走静默 fallback：与 dictation 一致的"用户的话不丢"约定。
    //    缺火山凭据 → 后续 Recorder 仍会跑，只是 ASR 拿不到结果，end_qa_session
    //    会发 idle 事件关浮窗。
    //    注意：QA 强制走 Volcengine 流式（见下方注释），所以这里必须直接校验
    //    Volcengine 字段，不能复用 `ensure_asr_credentials`——后者会按用户在设置
    //    里选的 active_asr 走 OpenAI 兼容分支，让 QA 把 `asr.api_key` 当成必要项，
    //    或在 Volcengine 凭据其实为空时误判通过。Codex P1，PR #213。
    if let Err(message) = ensure_qa_volcengine_credentials() {
        log::warn!("[coord] QA: ASR credentials missing: {message}");
        finish_qa_with_error(inner, format!("缺少 ASR 凭据：{message}"));
        return Err(message);
    }

    if let Err(message) = ensure_microphone_permission(inner) {
        log::warn!("[coord] QA: microphone permission gate failed: {message}");
        finish_qa_with_error(inner, message.clone());
        return Err(message);
    }

    // 3. 启动 Recorder + ASR（强制走 Volcengine 流式：QA 必须低延迟）。
    let hotwords = enabled_hotwords(inner);
    let creds = read_volc_credentials();
    let asr = Arc::new(VolcengineStreamingASR::new(creds, hotwords));
    let bridge = Arc::new(DeferredAsrBridge::new());
    let consumer: Arc<dyn crate::recorder::AudioConsumer> = bridge.clone();
    *inner.qa_asr.lock() = Some(Arc::clone(&asr));

    // QA recorder 不需要 RMS 节流到胶囊；前端 QA 浮窗有自己的电平视图，
    // 这里发一份事件给 "qa" label 用就够了。
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
            let _ = app.emit_to("qa", "qa:level", serde_json::json!({ "level": level }));
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

    match Recorder::start(consumer, level_handler) {
        Ok((rec, runtime_errors)) => {
            *inner.qa_recorder.lock() = Some(rec);
            // QA 也跟主听写一样监听 cpal runtime error。设备中途消失 / panic 时
            // 不能让 QA 永远卡在 Recording 没反馈。详见 issue #168。
            spawn_qa_recorder_error_monitor(inner, runtime_errors);
        }
        Err(e) => {
            log::error!("[coord] QA recorder start failed: {e}");
            inner.qa_asr.lock().take();
            finish_qa_with_error(inner, format!("录音启动失败: {e}"));
            return Err(e.to_string());
        }
    }

    if let Err(e) = asr.open_session().await {
        log::error!("[coord] QA: open ASR session failed: {e}");
        if let Some(rec) = inner.qa_recorder.lock().take() {
            rec.stop();
        }
        if let Some(asr) = inner.qa_asr.lock().take() {
            asr.cancel();
        }
        finish_qa_with_error(inner, format!("ASR 连接失败: {e}"));
        return Err(e.to_string());
    }

    // cancel race：在 await 期间用户可能 dismiss 了浮窗。
    if inner.qa_state.lock().cancelled {
        log::info!("[coord] QA cancel raced during open_session — aborting begin");
        asr.cancel();
        if let Some(rec) = inner.qa_recorder.lock().take() {
            rec.stop();
        }
        inner.qa_state.lock().phase = QaPhase::Idle;
        return Ok(());
    }

    let target: Arc<dyn crate::asr::AudioConsumer> = asr;
    let flushed = bridge.attach(target);
    log::info!("[coord] QA ASR connected; flushed {flushed} deferred audio bytes");

    // 显式弹胶囊到 Recording。level_handler 后续会持续推电平，胶囊里"录音中…"
    // 的视觉反馈跟主听写完全一致。
    emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);

    Ok(())
}

async fn end_qa_session(inner: &Arc<Inner>) -> Result<(), String> {
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
        let _ = app.emit_to("qa", "qa:state", serde_json::json!({ "kind": "loading" }));
    }

    if let Some(rec) = inner.qa_recorder.lock().take() {
        rec.stop();
    }

    let asr = match inner.qa_asr.lock().take() {
        Some(a) => a,
        None => {
            inner.qa_state.lock().phase = QaPhase::Idle;
            return Ok(());
        }
    };

    if let Err(e) = asr.send_last_frame().await {
        log::error!("[coord] QA: send last frame failed: {e}");
    }
    let raw = match asr.await_final_result().await {
        Ok(r) => r,
        Err(e) => {
            log::error!("[coord] QA: await final failed: {e}");
            finish_qa_with_error(inner, format!("识别失败: {e}"));
            return Err(e.to_string());
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
            "qa",
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
                "qa",
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
            "qa",
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
            app_bundle_id: None,
            app_name: front_app.clone(),
            insert_status: InsertStatus::CopiedFallback,
            error_code: Some("qaSession".to_string()),
            duration_ms: Some(raw.duration_ms),
            dictionary_entry_count: None,
        };
        if let Err(e) = inner.history.append(session) {
            log::error!("[coord] QA history append failed: {e}");
        }
    }

    inner.qa_state.lock().phase = QaPhase::Idle;
    Ok(())
}

/// 把出错状态送到前端浮窗 + 胶囊错误闪一下 + 复位 phase。
/// 浮窗保持可见（v2：错误后用户可以再按 Option 重试）；messages 一并送过去
/// 让前端继续渲染历史对话。
fn finish_qa_with_error(inner: &Arc<Inner>, message: String) {
    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            "qa",
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
fn finish_qa_idle_silently(inner: &Arc<Inner>) {
    if let Some(app) = inner.app.lock().clone() {
        let messages = inner.qa_state.lock().messages.clone();
        let _ = app.emit_to(
            "qa",
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

fn cancel_qa_session(inner: &Arc<Inner>) {
    let phase = inner.qa_state.lock().phase;
    if phase == QaPhase::Idle {
        return;
    }
    inner.qa_state.lock().cancelled = true;
    // SSE 流取消旗标——polish::chat_completion_history_streaming 的 loop 每帧检查
    // 这个 flag，true 时立即 break 不再 drain HTTP body，避免取消后 LLM 仍烧 token。
    // 详见 issue #161。
    inner.qa_stream_cancelled.store(true, Ordering::SeqCst);
    if let Some(rec) = inner.qa_recorder.lock().take() {
        rec.stop();
    }
    if let Some(asr) = inner.qa_asr.lock().take() {
        asr.cancel();
    }
    // Processing 阶段保持 phase 让 end_qa_session 自然走完 cancel 检查；
    // 否则直接复位。
    if phase != QaPhase::Processing {
        inner.qa_state.lock().phase = QaPhase::Idle;
    }
    log::info!("[coord] QA session cancelled (was {phase:?})");
}

async fn answer_chat_dispatch<F, C>(
    messages: &[crate::types::QaChatMessage],
    working_languages: &[String],
    front_app: Option<&str>,
    on_delta: F,
    should_cancel: C,
) -> anyhow::Result<String>
where
    F: Fn(&str) + Send + Sync,
    C: Fn() -> bool + Send + Sync,
{
    let api_key = CredentialsVault::get(CredentialAccount::ArkApiKey)?.unwrap_or_default();
    let model = CredentialsVault::get(CredentialAccount::ArkModelId)?
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "deepseek-v3-2".to_string());
    let endpoint = resolve_ark_endpoint(&api_key)?;
    let base_url = endpoint
        .trim_end_matches("/chat/completions")
        .trim_end_matches('/')
        .to_string();
    let config = OpenAICompatibleConfig::new("ark", "Doubao Ark", base_url, api_key, model);
    let provider = OpenAICompatibleLLMProvider::new(config);
    Ok(provider
        .answer_chat_streaming(
            messages,
            working_languages,
            front_app,
            on_delta,
            should_cancel,
        )
        .await?)
}

fn resolve_ark_endpoint(api_key: &str) -> anyhow::Result<String> {
    let endpoint = CredentialsVault::get(CredentialAccount::ArkEndpoint)?.filter(|s| !s.is_empty());
    resolve_ark_endpoint_with_policy(api_key, endpoint)
}

fn resolve_ark_endpoint_with_policy(
    api_key: &str,
    endpoint: Option<String>,
) -> anyhow::Result<String> {
    if api_key.trim().is_empty() && endpoint.is_none() {
        anyhow::bail!("API Key 为空");
    }
    Ok(endpoint
        .unwrap_or_else(|| "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HotkeyTrigger;

    #[tokio::test]
    async fn hotkey_injection_gate_logs_pressed_and_cancels() {
        let _ = env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .is_test(false)
            .try_init();
        std::env::set_var("OPENLESS_HOTKEY_INJECTION_DRY_RUN", "1");

        let coordinator = Coordinator::new();
        coordinator.inject_hotkey_click_for_dev().await.unwrap();

        assert_eq!(coordinator.inner.state.lock().phase, SessionPhase::Idle);
        std::env::remove_var("OPENLESS_HOTKEY_INJECTION_DRY_RUN");
    }

    #[test]
    fn window_key_matcher_mirrors_windows_trigger_aliases() {
        let cases = [
            (HotkeyTrigger::RightControl, "Control", "ControlRight"),
            (HotkeyTrigger::LeftControl, "Control", "ControlLeft"),
            (HotkeyTrigger::RightOption, "Alt", "AltRight"),
            (HotkeyTrigger::RightAlt, "AltGraph", "AltRight"),
            (HotkeyTrigger::RightCommand, "Meta", "MetaRight"),
            // Mirrors Windows trigger_to_vk_code aliases.
            (HotkeyTrigger::LeftOption, "Alt", "AltRight"),
            (HotkeyTrigger::Fn, "Control", "ControlRight"),
        ];
        for (trigger, key, code) in cases {
            assert!(
                window_key_matches_trigger(trigger, key, code),
                "{trigger:?} should match {key}/{code}"
            );
        }

        assert!(!window_key_matches_trigger(
            HotkeyTrigger::RightControl,
            "Control",
            "ControlLeft"
        ));
        assert!(!window_key_matches_trigger(
            HotkeyTrigger::LeftOption,
            "Alt",
            "AltLeft"
        ));
        assert!(!window_key_matches_trigger(HotkeyTrigger::Fn, "Fn", "Fn"));
    }

    #[test]
    fn resolve_ark_endpoint_rejects_blank_key_without_custom_endpoint() {
        assert_eq!(
            resolve_ark_endpoint_with_policy("", None)
                .unwrap_err()
                .to_string(),
            "API Key 为空"
        );
    }

    #[test]
    fn resolve_ark_endpoint_allows_blank_key_with_custom_endpoint() {
        let endpoint = resolve_ark_endpoint_with_policy(
            "",
            Some("https://example.com/v1/chat/completions".to_string()),
        )
        .unwrap();
        assert_eq!(endpoint, "https://example.com/v1/chat/completions");
    }

    #[test]
    fn deferred_asr_bridge_flushes_startup_audio_before_live_chunks() {
        #[derive(Default)]
        struct RecordingConsumer {
            bytes: Mutex<Vec<u8>>,
        }

        impl crate::asr::AudioConsumer for RecordingConsumer {
            fn consume_pcm_chunk(&self, pcm: &[u8]) {
                self.bytes.lock().extend_from_slice(pcm);
            }
        }

        let bridge = DeferredAsrBridge::new();
        crate::recorder::AudioConsumer::consume_pcm_chunk(&bridge, &[1, 2]);
        crate::recorder::AudioConsumer::consume_pcm_chunk(&bridge, &[3, 4]);

        let target = Arc::new(RecordingConsumer::default());
        let target_for_attach: Arc<dyn crate::asr::AudioConsumer> = target.clone();
        assert_eq!(bridge.attach(target_for_attach), 4);

        crate::recorder::AudioConsumer::consume_pcm_chunk(&bridge, &[5, 6]);
        assert_eq!(&*target.bytes.lock(), &[1, 2, 3, 4, 5, 6]);
    }

    #[tokio::test]
    async fn manual_stop_during_starting_is_queued() {
        let coordinator = Coordinator::new();
        {
            let mut state = coordinator.inner.state.lock();
            state.phase = SessionPhase::Starting;
            state.pending_stop = false;
        }

        coordinator.stop_dictation().await.unwrap();

        let state = coordinator.inner.state.lock();
        assert_eq!(state.phase, SessionPhase::Starting);
        assert!(state.pending_stop);
    }

    #[test]
    fn recorder_runtime_error_aborts_active_session() {
        let coordinator = Coordinator::new();
        {
            let mut state = coordinator.inner.state.lock();
            state.phase = SessionPhase::Listening;
            state.cancelled = false;
        }

        abort_recording_with_error(&coordinator.inner, "录音中断: stream failed".to_string());

        let state = coordinator.inner.state.lock();
        assert_eq!(state.phase, SessionPhase::Idle);
        assert!(state.cancelled);
        assert!(coordinator.inner.recorder.lock().is_none());
        assert!(coordinator.inner.asr.lock().is_none());
    }

    #[test]
    fn abort_recording_keeps_session_non_idle_until_restore_can_run() {
        let mut state = SessionState::default();
        state.phase = SessionPhase::Listening;
        state.cancelled = false;
        state.session_id = 7;

        let abort = begin_recording_abort_before_restore(&mut state).unwrap();

        assert_eq!(abort.session_id, 7);
        assert!(state.cancelled);
        assert_eq!(state.phase, SessionPhase::Listening);

        publish_abort_idle_after_restore(&mut state, abort.session_id);

        assert_eq!(state.phase, SessionPhase::Idle);
    }

    #[tokio::test]
    async fn pressed_edge_during_inserting_does_not_start_new_session() {
        let coordinator = Coordinator::new();
        {
            let mut state = coordinator.inner.state.lock();
            state.phase = SessionPhase::Inserting;
            state.session_id = 41;
        }

        handle_pressed_edge(&coordinator.inner).await;

        let state = coordinator.inner.state.lock();
        assert_eq!(state.phase, SessionPhase::Inserting);
        assert_eq!(state.session_id, 41);
    }

    #[tokio::test]
    async fn repeated_pressed_edge_during_hold_session_does_not_restart() {
        let coordinator = Coordinator::new();
        coordinator
            .inner
            .prefs
            .set(crate::types::UserPreferences {
                hotkey: crate::types::HotkeyBinding {
                    trigger: HotkeyTrigger::RightControl,
                    mode: HotkeyMode::Hold,
                },
                ..Default::default()
            })
            .unwrap();
        coordinator.inner.state.lock().phase = SessionPhase::Listening;
        coordinator
            .inner
            .hotkey_trigger_held
            .store(true, Ordering::SeqCst);

        handle_pressed_edge(&coordinator.inner).await;

        assert_eq!(
            coordinator.inner.state.lock().phase,
            SessionPhase::Listening
        );
        assert!(coordinator.inner.hotkey_trigger_held.load(Ordering::SeqCst));
    }

    #[test]
    fn window_hotkey_fallback_is_disabled_when_no_explicit_fallback_is_advertised() {
        assert_eq!(
            window_hotkey_fallback_enabled(),
            crate::types::HotkeyCapability::current().explicit_fallback_available
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn prepared_windows_ime_slot_is_taken_only_for_matching_session() {
        let mut slots = vec![PreparedWindowsImeSessionSlot {
            session_id: 2,
            prepared: PreparedWindowsImeSession::unavailable(),
        }];

        assert!(take_matching_prepared_windows_ime_session(&mut slots, 1).is_none());
        assert_eq!(
            slots.iter().map(|slot| slot.session_id).collect::<Vec<_>>(),
            vec![2]
        );

        assert!(take_matching_prepared_windows_ime_session(&mut slots, 2).is_some());
        assert!(slots.is_empty());
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn prepared_windows_ime_sessions_keep_overlapping_snapshots() {
        let mut slots = Vec::new();
        store_prepared_windows_ime_session(&mut slots, 1, PreparedWindowsImeSession::unavailable());
        store_prepared_windows_ime_session(&mut slots, 2, PreparedWindowsImeSession::unavailable());

        assert_eq!(
            slots.iter().map(|slot| slot.session_id).collect::<Vec<_>>(),
            vec![1, 2]
        );

        assert!(take_matching_prepared_windows_ime_session(&mut slots, 1).is_some());
        assert_eq!(
            slots.iter().map(|slot| slot.session_id).collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn stale_prepared_windows_ime_restore_discards_old_snapshot_without_restoring() {
        let mut slots = Vec::new();
        store_prepared_windows_ime_session(&mut slots, 1, PreparedWindowsImeSession::unavailable());
        store_prepared_windows_ime_session(&mut slots, 2, PreparedWindowsImeSession::unavailable());

        assert!(take_current_prepared_windows_ime_session_for_restore(&mut slots, 1, 2).is_none());
        assert_eq!(
            slots.iter().map(|slot| slot.session_id).collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn non_tsf_insertion_fallback_gate_blocks_only_when_disabled() {
        assert!(should_try_non_tsf_insertion_fallback(
            true,
            InsertStatus::CopiedFallback
        ));
        assert!(should_try_non_tsf_insertion_fallback(
            true,
            InsertStatus::Failed
        ));
        assert!(!should_try_non_tsf_insertion_fallback(
            true,
            InsertStatus::Inserted
        ));
        assert!(!should_try_non_tsf_insertion_fallback(
            false,
            InsertStatus::CopiedFallback
        ));
        assert!(!should_try_non_tsf_insertion_fallback(
            false,
            InsertStatus::Failed
        ));
    }

    #[test]
    fn focus_restore_failure_uses_specific_error_code_when_insert_fails() {
        assert_eq!(
            dictation_error_code(InsertStatus::Failed, false, false, false),
            Some("focusRestoreFailed")
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn missing_windows_hwnd_is_not_present() {
        use windows::Win32::Foundation::HWND;

        assert!(!windows_hwnd_is_present(HWND::default()));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn tsf_required_failure_keeps_tsf_error_when_focus_was_ready() {
        assert_eq!(
            dictation_error_code(InsertStatus::Failed, false, true, false),
            Some("windowsImeTsfRequired")
        );
    }

    #[test]
    fn startup_race_check_treats_newer_session_as_stale() {
        let mut state = SessionState::default();
        state.phase = SessionPhase::Starting;
        state.cancelled = false;
        state.session_id = 2;

        assert_eq!(
            startup_race_status(&state, 1),
            StartupRaceStatus::StaleContinuation
        );
    }

    #[test]
    fn stale_startup_cleanup_keeps_newer_asr_resource() {
        let coordinator = Coordinator::new();
        let newer_asr = Arc::new(WhisperBatchASR::new(
            "key".to_string(),
            "http://localhost".to_string(),
            "model".to_string(),
        ));
        *coordinator.inner.asr.lock() = Some(SessionResource::new(
            2,
            ActiveAsr::Whisper(Arc::clone(&newer_asr)),
        ));

        discard_startup_resources_for_session(&coordinator.inner, 1);

        assert_eq!(
            coordinator
                .inner
                .asr
                .lock()
                .as_ref()
                .map(|resource| resource.session_id),
            Some(2)
        );

        discard_startup_resources_for_session(&coordinator.inner, 2);

        assert!(coordinator.inner.asr.lock().is_none());
    }
}

fn enabled_phrases(inner: &Arc<Inner>) -> Vec<String> {
    inner
        .vocab
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.enabled)
        .map(|e| e.phrase)
        .collect()
}

/// 终止态（Done / Cancelled / Error）后延迟 N ms 把胶囊改回 Idle，让浮窗自动消失。
/// 用户点 ✕ / ✓ / 中途出错 / 按 Esc 都走这里，统一 2 秒。
const CAPSULE_AUTO_HIDE_DELAY_MS: u64 = 2000;

/// begin_session 中各 await 之间的 cancel race 检查结果。
enum BeginOutcome {
    /// 启动 continuation 属于旧 session；不能改动当前 session 状态。
    StaleContinuation,
    /// 正常进入 Listening。
    Started,
    /// Starting 阶段积累了 pending_stop 边沿，应立即 end_session（hold 快速松开 / toggle 快速双击）。
    PendingStop,
    /// 期间 cancel_session 触发（cancelled=true 或 phase 被外部改回 Idle）。
    /// 必须回滚 recorder + ASR 资源，不进 Listening。
    CancelRaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupRaceStatus {
    ActiveStarting,
    CancelRaced,
    StaleContinuation,
}

fn startup_race_status(state: &SessionState, captured_session_id: u64) -> StartupRaceStatus {
    if state.session_id != captured_session_id {
        StartupRaceStatus::StaleContinuation
    } else if state.cancelled || state.phase != SessionPhase::Starting {
        StartupRaceStatus::CancelRaced
    } else {
        StartupRaceStatus::ActiveStarting
    }
}

/// 检查 begin_session 的 await 间隙是否被 cancel_session 打断。
/// 必须在持有 state lock 的瞬间读，结果一拿就过期，所以用 helper 名字提醒只在
/// 「准备做下一步副作用前」用。
fn startup_race_status_for_starting(
    inner: &Arc<Inner>,
    captured_session_id: u64,
) -> StartupRaceStatus {
    let state = inner.state.lock();
    startup_race_status(&state, captured_session_id)
}

fn set_phase_idle_if_session_matches(inner: &Arc<Inner>, session_id: u64) {
    let mut state = inner.state.lock();
    if state.session_id == session_id {
        state.phase = SessionPhase::Idle;
    }
}

fn schedule_capsule_idle(inner: &Arc<Inner>, delay_ms: u64) {
    let inner_clone = Arc::clone(inner);
    async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        // 必须 dictation **和** QA 同时空闲才能隐藏胶囊。否则旧 dictation Done timer
        // 的尾巴会在新 QA 录音/思考中把胶囊意外收掉（issue #118 v2 复现）。
        let dictation_idle = inner_clone.state.lock().phase == SessionPhase::Idle;
        let qa_idle = inner_clone.qa_state.lock().phase == QaPhase::Idle;
        if dictation_idle && qa_idle {
            emit_capsule(&inner_clone, CapsuleState::Idle, 0.0, 0, None, None);
        }
    });
}

#[cfg(target_os = "windows")]
fn capture_focus_target() -> Option<usize> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        None
    } else {
        Some(foreground.0 as usize)
    }
}

#[cfg(not(target_os = "windows"))]
fn capture_focus_target() -> Option<usize> {
    None
}

/// 捕获用户开始 dictation 时的前台 app 标签（"localizedName (bundle.id)"），用作 LLM
/// polish/translate 的上下文前提，让模型按 app 调风格。详见 issue #116。
///
/// macOS 走 NSWorkspace.frontmostApplication（公开 API，无需额外权限）；
/// Windows 复用前台 HWND 拿窗口标题；Linux/其他平台返回 None。
#[cfg(target_os = "macos")]
fn capture_frontmost_app() -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let cls = AnyClass::get("NSWorkspace")?;
        let workspace: *mut AnyObject = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let name_obj: *mut AnyObject = msg_send![app, localizedName];
        let bundle_obj: *mut AnyObject = msg_send![app, bundleIdentifier];
        let name = nsstring_to_string(name_obj);
        let bundle = nsstring_to_string(bundle_obj);
        match (name, bundle) {
            (Some(n), Some(b)) => Some(format!("{n} ({b})")),
            (Some(n), None) => Some(n),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn nsstring_to_string(ns_string: *mut objc2::runtime::AnyObject) -> Option<String> {
    use objc2::msg_send;
    if ns_string.is_null() {
        return None;
    }
    let utf8: *const std::os::raw::c_char = unsafe { msg_send![ns_string, UTF8String] };
    if utf8.is_null() {
        return None;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(utf8) };
    let s = cstr.to_string_lossy().into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "windows")]
fn capture_frontmost_app() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&buf[..copied as usize]);
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn capture_frontmost_app() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
fn restore_focus_target_if_possible(target: Option<usize>) -> bool {
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
fn restore_focus_target_if_possible(_target: Option<usize>) -> bool {
    true
}

#[cfg(target_os = "windows")]
fn windows_hwnd_is_present(hwnd: windows::Win32::Foundation::HWND) -> bool {
    hwnd != windows::Win32::Foundation::HWND::default()
}

#[cfg(target_os = "windows")]
fn capture_ime_submit_target() -> Option<ImeSubmitTarget> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };

    let foreground = unsafe { GetForegroundWindow() };
    if !windows_hwnd_is_present(foreground) {
        return None;
    }

    let mut foreground_process_id = 0;
    let foreground_thread_id =
        unsafe { GetWindowThreadProcessId(foreground, Some(&mut foreground_process_id)) };
    if foreground_thread_id == 0 {
        return None;
    }

    let mut gui_info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    let target_window = if unsafe { GetGUIThreadInfo(foreground_thread_id, &mut gui_info).is_ok() }
        && windows_hwnd_is_present(gui_info.hwndFocus)
    {
        gui_info.hwndFocus
    } else {
        foreground
    };

    let mut process_id = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(target_window, Some(&mut process_id)) };
    if process_id == 0 || thread_id == 0 {
        return None;
    }

    Some(ImeSubmitTarget {
        process_id,
        thread_id,
    })
}

#[cfg(target_os = "windows")]
fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    };

    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return false;
    };
    let hwnd = HWND(raw.hwnd.get() as *mut _);

    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
    true
}

// macOS / Linux 上不走 no-activate 路径：胶囊由 emit_capsule 的 fallback
// `window.show()` 直接显示，再用 restore_main_window_key_if_active 把焦点还给
// 主窗口。这是 1.2.11 的实现 — 单独走 orderFrontRegardless 会让胶囊在 webview
// 未完整初始化时偶发不可见。
#[cfg(not(target_os = "windows"))]
fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    _window: &tauri::WebviewWindow<R>,
) -> bool {
    false
}

#[cfg(target_os = "windows")]
fn hide_capsule_window_if_present() {
    use std::iter::once;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowPos, ShowWindow, HWND_NOTOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    };

    let title: Vec<u16> = "OpenLess Capsule".encode_utf16().chain(once(0)).collect();
    let hwnd = match unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) } {
        Ok(hwnd) => hwnd,
        Err(_) => return,
    };
    if hwnd == HWND::default() || hwnd.0.is_null() {
        return;
    }

    let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
        )
    };
}

#[cfg(not(target_os = "windows"))]
fn hide_capsule_window_if_present() {}

fn emit_capsule(
    inner: &Arc<Inner>,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    message: Option<String>,
    inserted_chars: Option<u32>,
) {
    let app_opt = inner.app.lock().clone();
    let Some(app) = app_opt else { return };
    let payload = CapsulePayload {
        state,
        level,
        elapsed_ms,
        message,
        inserted_chars,
        translation: inner.translation_modifier_seen.load(Ordering::SeqCst),
    };

    let show_capsule = inner.prefs.get().show_capsule;
    if let Some(window) = app.get_webview_window("capsule") {
        // 三平台统一：Done / Cancelled / Error 状态保留 ~1.5s toast
        // （schedule_capsule_idle 之后会回 Idle 隐藏）。
        // Windows 上 linger 的真实问题（截图选中 / 死区 / 拖拽卡顿）由 #140 加的
        // `hide_capsule_window_if_present()` Win32 hard-hide 在 visible=false 分支
        // 处理，不依赖把 Done/Cancelled/Error 打成 invisible。详见 PR #140 评论。
        let visible = !matches!(state, CapsuleState::Idle);
        maybe_position_capsule_bottom_center(inner, &window, payload.translation);
        if show_capsule && visible {
            if !show_capsule_window_no_activate(&app, &window) {
                let _ = window.show();
            }
            // macOS/Windows 优先走 no-activate show，避免录音胶囊抢走主窗口点击焦点。
            // 若 fallback 到 show()，OpenLess 已是前台 app 时再把 key window 还给 main。
            #[cfg(target_os = "macos")]
            crate::restore_main_window_key_if_active(&app);
        } else {
            hide_capsule_window_if_present();
            let _ = window.hide();
        }
    }

    let _ = app.emit_to("capsule", "capsule:state", payload);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CapsuleLayoutState {
    translation_active: bool,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
    scale_bits: u64,
}

fn maybe_position_capsule_bottom_center<R: tauri::Runtime>(
    inner: &Arc<Inner>,
    window: &tauri::WebviewWindow<R>,
    translation_active: bool,
) {
    let Some(monitor) = window.current_monitor().ok().flatten() else {
        return;
    };
    let next = CapsuleLayoutState {
        translation_active,
        monitor_x: monitor.position().x,
        monitor_y: monitor.position().y,
        monitor_width: monitor.size().width,
        monitor_height: monitor.size().height,
        scale_bits: monitor.scale_factor().to_bits(),
    };
    {
        let last = inner.capsule_layout.lock();
        if last.as_ref() == Some(&next) {
            return;
        }
    }
    if crate::position_capsule_bottom_center(window, translation_active).is_ok() {
        let mut last = inner.capsule_layout.lock();
        *last = Some(next);
    }
}

// ─────────────────────────── audio bridge ───────────────────────────

struct DeferredAsrBridge {
    state: Mutex<DeferredAsrState>,
}

struct DeferredAsrState {
    target: Option<Arc<dyn crate::asr::AudioConsumer>>,
    pending_audio: Vec<u8>,
    attaching: bool,
}

impl DeferredAsrBridge {
    fn new() -> Self {
        Self {
            state: Mutex::new(DeferredAsrState {
                target: None,
                pending_audio: Vec::new(),
                attaching: false,
            }),
        }
    }

    fn attach(&self, target: Arc<dyn crate::asr::AudioConsumer>) -> usize {
        let mut flushed_bytes = 0;
        {
            let mut state = self.state.lock();
            state.attaching = true;
        }

        loop {
            let pending = {
                let mut state = self.state.lock();
                if state.pending_audio.is_empty() {
                    state.target = Some(Arc::clone(&target));
                    state.attaching = false;
                    return flushed_bytes;
                }
                std::mem::take(&mut state.pending_audio)
            };
            flushed_bytes += pending.len();
            target.consume_pcm_chunk(&pending);
        }
    }
}

impl crate::recorder::AudioConsumer for DeferredAsrBridge {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let target = {
            let mut state = self.state.lock();
            if state.attaching {
                state.pending_audio.extend_from_slice(pcm);
                return;
            }
            if let Some(target) = state.target.as_ref() {
                Some(Arc::clone(target))
            } else {
                state.pending_audio.extend_from_slice(pcm);
                None
            }
        };

        if let Some(target) = target {
            target.consume_pcm_chunk(pcm);
        }
    }
}
