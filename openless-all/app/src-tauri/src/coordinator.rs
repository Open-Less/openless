//! Dictation coordinator.
//!
//! Mirrors the Swift `DictationCoordinator` state machine. Single owner of
//! session state. Receives hotkey edges, drives recorder + ASR + polish +
//! insertion, persists history, emits `capsule:state` events to the capsule
//! window.

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
use crate::recorder::Recorder;
use crate::types::{
    CapsulePayload, CapsuleState, DictationSession, HotkeyCapability, HotkeyMode, HotkeyStatus,
    HotkeyStatusState, InsertStatus, PolishMode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPhase {
    Idle,
    Starting,
    Listening,
    Processing,
}

enum ActiveAsr {
    Volcengine(Arc<VolcengineStreamingASR>),
    Whisper(Arc<WhisperBatchASR>),
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
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            phase: SessionPhase::Idle,
            started_at: Instant::now(),
            pending_stop: false,
            cancelled: false,
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
    state: Mutex<SessionState>,
    asr: Mutex<Option<ActiveAsr>>,
    recorder: Mutex<Option<Recorder>>,
    hotkey: Mutex<Option<HotkeyMonitor>>,
    hotkey_status: Mutex<HotkeyStatus>,
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
                state: Mutex::new(SessionState::default()),
                asr: Mutex::new(None),
                recorder: Mutex::new(None),
                hotkey: Mutex::new(None),
                hotkey_status: Mutex::new(HotkeyStatus::default()),
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
        end_session(&self.inner).await
    }

    pub fn cancel_dictation(&self) {
        cancel_session(&self.inner);
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
        polish_text(&raw_text, mode, &hotwords)
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

fn hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<HotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        let inner_cloned = Arc::clone(&inner);
        match evt {
            HotkeyEvent::Pressed => {
                async_runtime::spawn(async move { handle_pressed(&inner_cloned).await });
            }
            HotkeyEvent::Released => {
                async_runtime::spawn(async move { handle_released(&inner_cloned).await });
            }
            HotkeyEvent::Cancelled => {
                cancel_session(&inner_cloned);
            }
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
            inner.state.lock().pending_stop = true;
            log::info!("[coord] toggle stop edge during Starting — queued");
        }
        _ => {}
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
                inner.state.lock().pending_stop = true;
                log::info!("[coord] hold release edge during Starting — queued");
            }
            _ => {}
        }
    }
}

// ─────────────────────────── session lifecycle ───────────────────────────

async fn begin_session(inner: &Arc<Inner>) -> Result<(), String> {
    {
        let mut state = inner.state.lock();
        if state.phase != SessionPhase::Idle {
            return Ok(());
        }
        state.phase = SessionPhase::Starting;
        state.started_at = Instant::now();
        // 新会话清掉旧 pending_stop / cancelled，避免上一会话遗留触发奇怪行为
        state.pending_stop = false;
        state.cancelled = false;
    }

    #[cfg(any(debug_assertions, test))]
    if hotkey_injection_dry_run_enabled() {
        emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);
        inner.state.lock().phase = SessionPhase::Listening;
        log::info!("[coord] session started (hotkey-injection dry-run)");
        return Ok(());
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
        inner.state.lock().phase = SessionPhase::Idle;
        schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
        return Err(message);
    }

    emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);

    let active_asr = CredentialsVault::get_active_asr();
    let hotwords = enabled_hotwords(inner);

    let consumer: Arc<dyn crate::recorder::AudioConsumer> = if active_asr == "whisper" {
        let (api_key, base_url, model) = read_whisper_credentials();
        let whisper = Arc::new(WhisperBatchASR::new(api_key, base_url, model));
        *inner.asr.lock() = Some(ActiveAsr::Whisper(Arc::clone(&whisper)));
        let c: Arc<dyn crate::recorder::AudioConsumer> = whisper;
        c
    } else {
        let creds = read_volc_credentials();
        let asr = Arc::new(VolcengineStreamingASR::new(creds, hotwords));
        if let Err(e) = asr.open_session().await {
            log::error!("[coord] open ASR session failed: {e}");
            emit_capsule(
                inner,
                CapsuleState::Error,
                0.0,
                0,
                Some(format!("ASR 连接失败: {e}")),
                None,
            );
            inner.state.lock().phase = SessionPhase::Idle;
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            return Err(e.to_string());
        }
        let c: Arc<dyn crate::recorder::AudioConsumer> = Arc::new(AsrBridge {
            asr: Arc::clone(&asr),
        });
        *inner.asr.lock() = Some(ActiveAsr::Volcengine(asr));
        c
    };

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
        Ok(rec) => {
            *inner.recorder.lock() = Some(rec);
            // 转 Listening 同时检查 Starting 期间是否积累了 pending_stop 边沿。
            // hold 模式快速松开 / toggle 快速双击会到这里：握手刚完就要立即停。
            let should_stop_immediately = {
                let mut state = inner.state.lock();
                state.phase = SessionPhase::Listening;
                std::mem::replace(&mut state.pending_stop, false)
            };
            log::info!("[coord] session started (asr={})", active_asr);
            if should_stop_immediately {
                log::info!("[coord] applying pending_stop edge → end_session immediately");
                let _ = end_session(inner).await;
            }
        }
        Err(e) => {
            log::error!("[coord] recorder start failed: {e}");
            if let Some(asr) = inner.asr.lock().take() {
                match asr {
                    ActiveAsr::Volcengine(v) => v.cancel(),
                    ActiveAsr::Whisper(w) => w.cancel(),
                }
            }
            emit_capsule(
                inner,
                CapsuleState::Error,
                0.0,
                0,
                Some(format!("录音启动失败: {e}")),
                None,
            );
            inner.state.lock().phase = SessionPhase::Idle;
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            return Err(e.to_string());
        }
    }

    Ok(())
}

async fn end_session(inner: &Arc<Inner>) -> Result<(), String> {
    {
        let mut state = inner.state.lock();
        if state.phase != SessionPhase::Listening {
            return Ok(());
        }
        state.phase = SessionPhase::Processing;
    }

    let elapsed = inner.state.lock().started_at.elapsed().as_millis() as u64;
    emit_capsule(inner, CapsuleState::Transcribing, 0.0, elapsed, None, None);

    if let Some(rec) = inner.recorder.lock().take() {
        rec.stop();
    }

    let asr_opt = inner.asr.lock().take();
    let asr = match asr_opt {
        Some(a) => a,
        None => {
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
                inner.state.lock().phase = SessionPhase::Idle;
                schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
                return Err(e.to_string());
            }
        },
    };

    // ASR 完成后 cancel 检查：用户在 transcribe 进行中按 Esc 时，这里就会命中。
    if inner.state.lock().cancelled {
        log::info!("[coord] cancel detected after ASR — discarding transcript");
        inner.state.lock().phase = SessionPhase::Idle;
        return Ok(());
    }

    emit_capsule(inner, CapsuleState::Polishing, 0.0, elapsed, None, None);

    let prefs = inner.prefs.get();
    let mode = prefs.default_mode;
    let hotword_strs = enabled_phrases(inner);
    let polished = polish_or_passthrough(&raw, mode, &hotword_strs).await;

    // Polish 完成后再 check 一次：即使 polish 已经返回，只要还没插入，仍可丢弃。
    if inner.state.lock().cancelled {
        log::info!("[coord] cancel detected after polish — discarding output (chars={})", polished.chars().count());
        inner.state.lock().phase = SessionPhase::Idle;
        return Ok(());
    }

    let status = inner.inserter.insert(&polished);
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

    let session = DictationSession {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        raw_transcript: raw.text.clone(),
        final_text: polished.clone(),
        mode,
        app_bundle_id: None,
        app_name: None,
        insert_status: status,
        error_code: None,
        duration_ms: Some(raw.duration_ms),
        // 历史详情页的"X 个热词"显示：用本次实际命中次数（每个匹配实例算一次），
        // 比"启用词条总数"更能反映本段口述命中了多少。u64 → u32 截断对单段听写足够。
        dictionary_entry_count: Some(total_hits.min(u32::MAX as u64) as u32),
    };
    if let Err(e) = inner.history.append(session) {
        log::error!("[coord] history append failed: {e}");
    }

    let done_message = match status {
        InsertStatus::Inserted => None,
        InsertStatus::CopiedFallback => Some(if cfg!(target_os = "windows") {
            "已复制，请 Ctrl+V".to_string()
        } else {
            "已复制，请粘贴".to_string()
        }),
        InsertStatus::Failed => Some("插入失败".to_string()),
    };

    emit_capsule(
        inner,
        CapsuleState::Done,
        0.0,
        elapsed,
        done_message,
        Some(inserted_chars),
    );

    inner.state.lock().phase = SessionPhase::Idle;
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);

    Ok(())
}

fn cancel_session(inner: &Arc<Inner>) {
    let phase = inner.state.lock().phase;
    if phase == SessionPhase::Idle {
        return;
    }
    // Processing 阶段 cancel 不能直接干掉 in-flight polish task（已经 await 了），
    // 但可以打 cancelled 标记，让 end_session 在插入前检查并丢弃结果。
    inner.state.lock().cancelled = true;

    if let Some(rec) = inner.recorder.lock().take() {
        rec.stop();
    }
    if let Some(asr) = inner.asr.lock().take() {
        match asr {
            ActiveAsr::Volcengine(v) => v.cancel(),
            ActiveAsr::Whisper(w) => w.cancel(),
        }
    }
    // Processing 阶段保持 phase=Processing 让 end_session 自己走完检查 + 收尾；
    // 其他阶段直接转 Idle。
    if phase != SessionPhase::Processing {
        inner.state.lock().phase = SessionPhase::Idle;
    }
    emit_capsule(inner, CapsuleState::Cancelled, 0.0, 0, None, None);
    log::info!("[coord] session cancelled (was {phase:?})");
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
}

// ─────────────────────────── helpers ───────────────────────────

#[cfg(any(debug_assertions, test))]
fn hotkey_injection_dry_run_enabled() -> bool {
    std::env::var_os("OPENLESS_HOTKEY_INJECTION_DRY_RUN").is_some()
}

fn ensure_microphone_permission(inner: &Arc<Inner>) -> Result<(), String> {
    use crate::permissions::{self, PermissionStatus};

    let status = permissions::check_microphone();
    if matches!(
        status,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        return Ok(());
    }

    let requested = if let Some(app) = inner.app.lock().clone() {
        crate::request_microphone_from_foreground(&app)
    } else {
        permissions::request_microphone()
    };
    if matches!(
        requested,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        Ok(())
    } else {
        Err(format!("需要麦克风权限，当前状态: {requested:?}"))
    }
}

async fn polish_or_passthrough(
    raw: &RawTranscript,
    mode: PolishMode,
    hotwords: &[String],
) -> String {
    if mode == PolishMode::Raw {
        return raw.text.clone();
    }
    match polish_text(&raw.text, mode, hotwords).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("[coord] polish failed, falling back to raw: {e}");
            raw.text.clone()
        }
    }
}

async fn polish_text(raw: &str, mode: PolishMode, hotwords: &[String]) -> anyhow::Result<String> {
    let api_key = CredentialsVault::get(CredentialAccount::ArkApiKey)?.unwrap_or_default();
    if api_key.is_empty() {
        anyhow::bail!("ark api key missing");
    }
    let model = CredentialsVault::get(CredentialAccount::ArkModelId)?
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "deepseek-v3-2".to_string());
    let endpoint = CredentialsVault::get(CredentialAccount::ArkEndpoint)?
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_string());
    let base_url = endpoint
        .trim_end_matches("/chat/completions")
        .trim_end_matches('/')
        .to_string();

    let config = OpenAICompatibleConfig::new("ark", "Doubao Ark", base_url, api_key, model);
    let provider = OpenAICompatibleLLMProvider::new(config);
    Ok(provider.polish(raw, mode, hotwords).await?)
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

#[cfg(test)]
mod tests {
    use super::*;

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

fn schedule_capsule_idle(inner: &Arc<Inner>, delay_ms: u64) {
    let inner_clone = Arc::clone(inner);
    async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        // 仅在仍然 Idle 时（即用户没在这 2s 内重新触发）才 hide。
        // 否则可能把新启动的 Recording 状态意外覆盖回 Idle。
        if inner_clone.state.lock().phase == SessionPhase::Idle {
            emit_capsule(&inner_clone, CapsuleState::Idle, 0.0, 0, None, None);
        }
    });
}

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
    };

    let show_capsule = inner.prefs.get().show_capsule;
    if let Some(window) = app.get_webview_window("capsule") {
        let visible = !matches!(state, CapsuleState::Idle);
        if show_capsule && visible {
            let _ = window.show();
            // 胶囊 show() 在 macOS 会调 makeKeyAndOrderFront: 抢走主窗口焦点。
            // 若 OpenLess 已是前台 app，用 makeKeyWindow 还原主窗口（不激活 NSApp）。
            #[cfg(target_os = "macos")]
            crate::restore_main_window_key_if_active(&app);
        } else {
            let _ = window.hide();
        }
    }

    let _ = app.emit_to("capsule", "capsule:state", payload);
}

// ─────────────────────────── audio bridge ───────────────────────────

struct AsrBridge {
    asr: Arc<VolcengineStreamingASR>,
}

impl crate::recorder::AudioConsumer for AsrBridge {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        crate::asr::AudioConsumer::consume_pcm_chunk(&*self.asr, pcm);
    }
}
