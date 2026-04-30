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
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            phase: SessionPhase::Idle,
            started_at: Instant::now(),
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
        _ => {}
    }
}

async fn handle_released(inner: &Arc<Inner>) {
    let mode = inner.prefs.get().hotkey.mode;
    if mode == HotkeyMode::Hold {
        let phase = inner.state.lock().phase;
        if phase == SessionPhase::Listening {
            let _ = end_session(inner).await;
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
    }

    if let Err(message) = ensure_microphone_permission(inner) {
        log::warn!("[coord] microphone permission gate failed: {message}");
        emit_capsule(inner, CapsuleState::Error, 0.0, 0, Some(message.clone()), None);
        inner.state.lock().phase = SessionPhase::Idle;
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
            return Err(e.to_string());
        }
        let c: Arc<dyn crate::recorder::AudioConsumer> =
            Arc::new(AsrBridge { asr: Arc::clone(&asr) });
        *inner.asr.lock() = Some(ActiveAsr::Volcengine(asr));
        c
    };

    let inner_for_level = Arc::clone(inner);
    let level_handler: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level| {
        let phase = inner_for_level.state.lock().phase;
        if phase == SessionPhase::Listening || phase == SessionPhase::Starting {
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
        }
    });

    match Recorder::start(consumer, level_handler) {
        Ok(rec) => {
            *inner.recorder.lock() = Some(rec);
            inner.state.lock().phase = SessionPhase::Listening;
            log::info!("[coord] session started (asr={})", active_asr);
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
                return Err(e.to_string());
            }
        },
    };

    emit_capsule(inner, CapsuleState::Polishing, 0.0, elapsed, None, None);

    let prefs = inner.prefs.get();
    let mode = prefs.default_mode;
    let hotword_strs = enabled_phrases(inner);
    let polished = polish_or_passthrough(&raw, mode, &hotword_strs).await;

    let status = inner.inserter.insert(&polished);
    let inserted_chars = polished.chars().count() as u32;

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
        dictionary_entry_count: Some(hotword_strs.len() as u32),
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

    let inner_clone = Arc::clone(inner);
    async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        emit_capsule(&inner_clone, CapsuleState::Idle, 0.0, 0, None, None);
    });

    Ok(())
}

fn cancel_session(inner: &Arc<Inner>) {
    let phase = inner.state.lock().phase;
    if phase == SessionPhase::Idle {
        return;
    }
    if let Some(rec) = inner.recorder.lock().take() {
        rec.stop();
    }
    if let Some(asr) = inner.asr.lock().take() {
        match asr {
            ActiveAsr::Volcengine(v) => v.cancel(),
            ActiveAsr::Whisper(w) => w.cancel(),
        }
    }
    inner.state.lock().phase = SessionPhase::Idle;
    emit_capsule(inner, CapsuleState::Cancelled, 0.0, 0, None, None);
    log::info!("[coord] session cancelled");
}

// ─────────────────────────── helpers ───────────────────────────

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
