//! The iOS host owns only native adapters and executor/FFI lifetime.
//! Session, cancellation, history and insertion orchestration remain in Core.
use futures_util::future::BoxFuture;
use openless_core::credentials::{ChannelKind, ChannelMutation, ProviderSlot};
use openless_core::provider_registry::{
    DictationEngineRouter, TextPolisherRouter, TranscriptionRouter,
};
use openless_core::{
    SharedCloudTextPolisher, SharedCloudTranscriptionEngine, SharedOmniDictationEngine,
    SHARED_CLOUD_ASR_PROVIDER_TYPES, SHARED_CLOUD_LLM_PROVIDER_TYPES, SHARED_OMNI_PROVIDER_TYPES,
};
mod account_sync;
mod control;
mod credentials;
mod library;
mod vocabulary_bridge;
mod recording_maintenance;
mod recordings;
mod selection;
mod style_library;
mod voice_intent;
use control::ControlAction;
use openless_core::dictation_context::{DictationContext, DictationStartOptions};
use openless_core::errors::{BackendError, BackendErrorCode};
use openless_core::ports::*;
use openless_core::shared_types::{CredentialsStatus, PlatformCapabilities, UserPreferences};
use openless_core::types::{PolishMode, SessionId};
use openless_core::{
    BackendConfig, BackendDependencies, BackendRepositories, CredentialStore, OpenLessBackend,
    PipelineDictationEngine,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::{c_char, CStr, CString},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::{
    runtime::{Handle, Runtime},
    sync::oneshot,
};
use uuid::Uuid;

#[cfg(test)]
mod account_sync_tests;
#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod library_tests;
#[cfg(test)]
mod vocabulary_tests;
#[cfg(test)]
mod tests;

type EventCallback = extern "C" fn(u64, *const u8, usize);
type PCMCallback = extern "C" fn(u64, *const c_char, *const u8, usize);
const MAX_JSON: usize = 256 * 1024;
const MAX_PCM: usize = 64 * 1024;

fn error(code: BackendErrorCode, message: &str) -> BackendError {
    BackendError::new(code, message)
}
fn cancelled() -> BackendError {
    error(BackendErrorCode::Cancelled, "native operation cancelled")
}

#[derive(Clone, Copy)]
struct Callbacks {
    event: EventCallback,
    audio: PCMCallback,
    context: u64,
}
struct Capture {
    consumer: Arc<dyn AudioConsumer>,
    progress: Arc<dyn RecordingProgressSink>,
    bytes: AtomicU64,
    archive: Option<Arc<recordings::WavArchive>>,
}
struct Broker {
    callbacks: Mutex<Option<Callbacks>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, BackendError>>>>,
    captures: Mutex<HashMap<SessionId, Arc<Capture>>>,
    partials: Mutex<HashMap<SessionId, Arc<dyn TextStreamSink>>>,
    closed: AtomicBool,
}
impl Broker {
    fn emit(&self, value: Value) {
        // Copy the callbacks under lock; native callback must never run while
        // holding a lock that can be reentered by a reply/feed call.
        let callback = *self.callbacks.lock().unwrap();
        if let Some(callback) = callback {
            if let Ok(data) = serde_json::to_vec(&value) {
                (callback.event)(callback.context, data.as_ptr(), data.len());
            }
        }
    }
    async fn request(
        &self,
        operation: &str,
        session: SessionId,
        extra: Value,
    ) -> Result<Value, BackendError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);
        self.emit(json!({"kind":"nativeRequest","requestID":id,"operation":operation,"sessionID":session.to_string(),"payload":extra}));
        let result = tokio::time::timeout(Duration::from_secs(30), rx).await;
        self.pending.lock().unwrap().remove(&id);
        match result {
            Ok(Ok(value)) => value,
            _ => {
                self.emit(
                    json!({"kind":"abortNative","sessionID":session.to_string(),"requestID":id}),
                );
                Err(error(
                    BackendErrorCode::Platform,
                    "native operation timed out",
                ))
            }
        }
    }
    fn reply(&self, response: Reply) -> bool {
        let sender = self.pending.lock().unwrap().remove(&response.request_id);
        if let Some(sender) = sender {
            let value = if response.ok {
                Ok(response.value.unwrap_or(Value::Null))
            } else {
                Err(error(
                    BackendErrorCode::InvalidState,
                    response
                        .error
                        .as_deref()
                        .unwrap_or("native operation failed"),
                ))
            };
            sender.send(value).is_ok()
        } else {
            false
        }
    }
    fn abort_pending(&self) {
        let senders = std::mem::take(&mut *self.pending.lock().unwrap());
        for (_, sender) in senders {
            let _ = sender.send(Err(cancelled()));
        }
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        *self.callbacks.lock().unwrap() = None;
        self.abort_pending();
        self.captures.lock().unwrap().clear();
        self.partials.lock().unwrap().clear();
    }
}

struct NativeRecorder(Arc<Broker>, PathBuf);
struct Recording {
    broker: Arc<Broker>,
    id: SessionId,
    archive: Option<Arc<recordings::WavArchive>>,
}
impl AudioRecorder for NativeRecorder {
    fn start(
        &self,
        id: SessionId,
        context: Arc<DictationContext>,
        consumer: Arc<dyn AudioConsumer>,
        progress: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        let broker = self.0.clone();
        let directory = self.1.clone();
        Box::pin(async move {
            // Archiving is optional: storage failures must not disable typing or ASR.
            let archive = context.recording.archive_enabled
                .then(|| recordings::WavArchive::create(&directory, id).ok()).flatten();
            broker.captures.lock().unwrap().insert(
                id,
                Arc::new(Capture {
                    consumer,
                    progress,
                    bytes: AtomicU64::new(0),
                    archive: archive.clone(),
                }),
            );
            if let Err(err) = broker.request("captureStart", id, Value::Null).await {
                broker.captures.lock().unwrap().remove(&id);
                if let Some(archive) = &archive { let _ = archive.discard().await; }
                broker.emit(json!({"kind":"abortNative","sessionID":id.to_string()}));
                return Err(err);
            }
            Ok(Box::new(Recording { broker, id, archive }) as Box<dyn ActiveRecording>)
        })
    }
}
impl ActiveRecording for Recording {
    fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
        self.archive.clone().map(|archive| archive as Arc<dyn RecordingArchive>)
    }
    fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move {
            // Swift stops the hardware, then drains already captured chunks.
            let result = self
                .broker
                .request("captureStop", self.id, Value::Null)
                .await;
            self.broker.captures.lock().unwrap().remove(&self.id);
            if let Some(archive) = &self.archive { archive.finish(); }
            result.map(|_| ())
        })
    }
}

struct NativeTranscriber(Arc<Broker>);
struct NativeSpeechSession {
    broker: Arc<Broker>,
    id: SessionId,
}
impl TranscriptionEngine for NativeTranscriber {
    fn start(
        &self,
        id: SessionId,
        context: Arc<DictationContext>,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let broker = self.0.clone();
        Box::pin(async move {
            let provider = match context.asr.provider_type.as_str() {
                "ios-sensevoice" => "sensevoice",
                "apple-speech" => "apple-speech",
                _ => {
                    return Err(error(
                        BackendErrorCode::Unsupported,
                        "unknown native speech provider",
                    ))
                }
            };
            broker.partials.lock().unwrap().insert(id, partials);
            if let Err(err) = broker
                .request("speechStart", id, json!({"provider":provider}))
                .await
            {
                broker.partials.lock().unwrap().remove(&id);
                return Err(err);
            }
            Ok(Arc::new(NativeSpeechSession { broker, id }) as Arc<dyn TranscriptionSession>)
        })
    }
}
impl AudioConsumer for NativeSpeechSession {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let callback = *self.broker.callbacks.lock().unwrap();
        if let Some(callback) = callback {
            let id = CString::new(self.id.to_string()).unwrap();
            (callback.audio)(callback.context, id.as_ptr(), pcm.as_ptr(), pcm.len());
        }
    }
}
impl TranscriptionSession for NativeSpeechSession {
    fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
        let broker = self.broker.clone();
        let id = self.id;
        Box::pin(async move {
            let result = broker.request("speechFinish", id, Value::Null).await;
            broker.partials.lock().unwrap().remove(&id);
            let result = result?;
            let text = result["text"].as_str().ok_or_else(|| {
                error(
                    BackendErrorCode::InvalidArgument,
                    "invalid transcription response",
                )
            })?;
            if text.len() > 128 * 1024 {
                return Err(error(
                    BackendErrorCode::InvalidArgument,
                    "transcript too large",
                ));
            }
            Ok(TranscriptOutput {
                text: text.into(),
                duration_ms: result["durationMs"].as_u64().unwrap_or(0),
            })
        })
    }
    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let broker = self.broker.clone();
        let id = self.id;
        Box::pin(async move {
            broker.partials.lock().unwrap().remove(&id);
            broker
                .request("speechCancel", id, Value::Null)
                .await
                .map(|_| ())
        })
    }
}
struct KeyboardInserter(Arc<Broker>);
struct KeyboardInsertion {
    broker: Arc<Broker>,
    id: SessionId,
}
impl TextInserter for KeyboardInserter {
    fn begin(
        &self,
        id: SessionId,
        _context: Arc<DictationContext>,
    ) -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>> {
        let broker = self.0.clone();
        Box::pin(async move {
            Ok(Arc::new(KeyboardInsertion { broker, id }) as Arc<dyn TextInsertionSession>)
        })
    }
}
impl TextInsertionSession for KeyboardInsertion {
    fn supports_streaming(&self) -> bool {
        false
    }
    fn write(&self, _text: String) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
        Box::pin(async {
            Err(error(
                BackendErrorCode::Unsupported,
                "iOS uses final insertion receipts",
            ))
        })
    }
    fn copy(&self, _text: String) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async {
            Err(error(
                BackendErrorCode::Unsupported,
                "automatic clipboard access is disabled",
            ))
        })
    }
    fn finish(&self, text: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let broker = self.broker.clone();
        let id = self.id;
        Box::pin(async move {
            let response = broker.request("insert", id, json!({"text":text})).await?;
            match response["status"].as_str() {
                Some("verified") => Ok(InsertOutcome::Inserted),
                Some("submitted") => Ok(InsertOutcome::PasteSent),
                _ => Err(error(
                    BackendErrorCode::InvalidState,
                    "text retained for manual confirmation",
                )),
            }
        })
    }
    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}
struct Spawner(Handle);
impl openless_core::TaskSpawner for Spawner {
    fn spawn(&self, future: BoxFuture<'static, ()>) {
        self.0.spawn(future);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    data_dir: PathBuf,
    #[serde(default)]
    developer_mode: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply {
    #[serde(rename = "requestID")]
    request_id: String,
    ok: bool,
    value: Option<Value>,
    error: Option<String>,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    Start {
        #[serde(rename = "clientSessionID")]
        client: Uuid,
        #[serde(default)]
        insert: bool,
        #[serde(default)]
        intent: Option<voice_intent::VoiceIntent>,
    },
    Stop {
        #[serde(rename = "clientSessionID")]
        client: Uuid,
    },
    Cancel {
        #[serde(rename = "clientSessionID")]
        client: Uuid,
    },
    History,
    ClearHistory,
    Transcript {
        #[serde(rename = "sessionID")]
        session: SessionId,
        text: String,
    },
}
struct Admission {
    client: Uuid,
    finishing: bool,
    cancelled: bool,
    execution: Arc<voice_intent::Execution>,
}
struct Host {
    account_sync: account_sync::AccountSync,
    backend: Arc<OpenLessBackend>,
    broker: Arc<Broker>,
    admission: Mutex<Option<Admission>>,
    executor: Handle,
    developer_mode: bool,
    recordings_directory: PathBuf,
    control_busy: AtomicBool,
}
struct Entry {
    host: Arc<Host>,
    runtime: Runtime,
}
static HOSTS: OnceLock<Mutex<HashMap<u64, Entry>>> = OnceLock::new();
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
fn hosts() -> &'static Mutex<HashMap<u64, Entry>> {
    HOSTS.get_or_init(Default::default)
}
fn host(id: u64) -> Option<Arc<Host>> {
    hosts()
        .lock()
        .ok()?
        .get(&id)
        .map(|entry| entry.host.clone())
}

fn public_error_message(err: &BackendError) -> String {
    if err.code == BackendErrorCode::Provider {
        "provider request failed; check endpoint, model and credentials".into()
    } else {
        err.message.clone()
    }
}

impl Host {
    fn failed(&self, client: Uuid, err: &BackendError) {
        self.broker.emit(json!({"kind":"failed","clientSessionID":client,"message":public_error_message(err),"code":format!("{:?}",err.code)}));
    }
    fn dispatch(self: &Arc<Self>, command: Command) -> Result<(), BackendError> {
        match command {
            Command::Start { client, insert, intent } => self.start_voice(client, insert, intent)?,
            Command::Stop { client } => self.stop_voice(client)?,
            Command::Cancel { client } => self.cancel_voice(client)?,
            Command::History => {
                let entries = self.backend.list_history()?;
                let entries: Vec<_> = entries.iter().map(library::history_summary).collect();
                self.broker.emit(json!({"kind":"history","entries":entries}));
            }
            Command::ClearHistory => {
                let admission = self.admission.lock().unwrap();
                if admission.is_some() || self.control_busy.swap(true, Ordering::AcqRel) {
                    return Err(error(BackendErrorCode::Busy, "recording or management is active"));
                }
                drop(admission);
                let result = self.clear_history_with_recordings();
                self.control_busy.store(false, Ordering::Release);
                result?;
                self.dispatch(Command::History)?;
            }
            Command::Transcript { session, text } => {
                if text.len() > 128 * 1024 {
                    return Err(error(
                        BackendErrorCode::InvalidArgument,
                        "transcript too large",
                    ));
                }
                let sink = self.broker.partials.lock().unwrap().get(&session).cloned();
                if let Some(sink) = sink {
                    sink.publish(TextStreamChunk { text, offset: 0 })?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(not(test))]
fn create(config: Config, callbacks: Callbacks) -> Result<u64, BackendError> {
    if !config.data_dir.is_absolute() {
        return Err(error(
            BackendErrorCode::InvalidArgument,
            "absolute data directory required",
        ));
    }
    let credentials = Arc::new(credentials::IosCredentialStore::keychain(config.developer_mode)?);
    create_with_store(config, callbacks, credentials)
}
#[cfg(test)]
fn create(_config: Config, _callbacks: Callbacks) -> Result<u64, BackendError> {
    Err(error(
        BackendErrorCode::Unsupported,
        "tests must inject an explicit memory vault",
    ))
}

fn create_with_store(
    config: Config,
    callbacks: Callbacks,
    credentials: Arc<dyn CredentialStore>,
) -> Result<u64, BackendError> {
    create_with_marketplace(config, callbacks, credentials, openless_core::MarketplaceConfig::production())
}

fn create_with_marketplace(
    mut config: Config,
    callbacks: Callbacks,
    credentials: Arc<dyn CredentialStore>,
    marketplace: openless_core::MarketplaceConfig,
) -> Result<u64, BackendError> {
    if !config.data_dir.is_absolute() {
        return Err(error(
            BackendErrorCode::InvalidArgument,
            "absolute data directory required",
        ));
    }
    // Register the durable write gate before opening stores or applying startup
    // preferences. Reopened roots must not be mistaken for an unowned restore.
    let _write_gate = openless_core::cloud_sync_e2ee_store::gate::open_for_data_dir(&config.data_dir)
        .map_err(|failure| error(BackendErrorCode::InvalidState, &failure.to_string()))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| error(BackendErrorCode::InvalidState, "executor unavailable"))?;
    let broker = Arc::new(Broker {
        callbacks: Mutex::new(Some(callbacks)),
        pending: Mutex::new(HashMap::new()),
        captures: Mutex::new(HashMap::new()),
        partials: Mutex::new(HashMap::new()),
        closed: AtomicBool::new(false),
    });
    runtime.block_on(async {
        let channels = credentials.list_channels(ChannelKind::Asr).await?;
        if channels.is_empty() || !config.developer_mode {
            let native = channels
                .into_iter()
                .find(|c| c.provider_type == "ios-sensevoice");
            let id = match native {
                Some(channel) => channel.id,
                None => match credentials
                    .mutate_channel(ChannelMutation::Create {
                        kind: ChannelKind::Asr,
                        provider_type: "ios-sensevoice".into(),
                        name: "SenseVoice (iOS)".into(),
                    })
                    .await?
                {
                    openless_core::credentials::ChannelMutationResult::Created(id) => id,
                    _ => return Err(error(BackendErrorCode::Internal, "native channel missing")),
                },
            };
            credentials
                .set_active_provider(ProviderSlot::Asr, id)
                .await?;
        }
        Ok::<_, BackendError>(())
    })?;
    let has_saved_preferences = config.data_dir.join("preferences.json").is_file();
    let repos = BackendRepositories::open(&config.data_dir)?;
    config.data_dir = std::fs::canonicalize(&config.data_dir)
        .map_err(|_| error(BackendErrorCode::Persistence, "data directory is unavailable"))?;
    let mut prefs: UserPreferences = repos.preferences.get();
    if !has_saved_preferences {
        // Entering developer mode does not opt the user into an unconfigured
        // LLM. Later explicit choices remain untouched on every reload.
        prefs.default_mode = PolishMode::Raw;
        prefs.active_style_pack_id = "builtin.raw".into();
        prefs.pipeline_mode = openless_core::shared_types::PipelineMode::Traditional;
        prefs.multimodal_pipeline_enabled = false;
    }
    if !config.developer_mode {
        // Core reconciles disabled selections to another enabled style. Keep
        // ordinary mode raw even when this data root was used by a developer.
        if !repos.style_packs.get("builtin.raw")?.enabled {
            repos.style_packs.set_enabled("builtin.raw", true)?;
        }
        prefs.active_asr_provider = "ios-sensevoice".into();
        prefs.default_mode = PolishMode::Raw;
        prefs.active_style_pack_id = "builtin.raw".into();
        prefs.pipeline_mode = openless_core::shared_types::PipelineMode::Traditional;
        prefs.multimodal_pipeline_enabled = false;
    }
    // iOS never streams insertion or performs desktop hardware side effects.
    // Do not overwrite developer channel/style/pipeline choices on reload.
    prefs.streaming_insert = false;
    prefs.streaming_insert_save_clipboard = false;
    prefs.stable_transcription_enabled = false;
    prefs.record_audio_for_debug = false;
    prefs.auto_update_check = false;
    prefs.audio_cue_on_record = false;
    prefs.mute_during_recording = false;
    prefs.silence_auto_stop_enabled = false;
    prefs.selection_polish_output_mode =
        openless_core::shared_types::SelectionPolishOutputMode::PreviewConfirm;
    repos.preferences.set(prefs)?;
    let spawner = Arc::new(Spawner(runtime.handle().clone()));
    let transcription = Arc::new(TranscriptionRouter::default());
    let cloud_asr = Arc::new(SharedCloudTranscriptionEngine::with_task_spawner(
        credentials.clone(),
        spawner.clone(),
    ));
    for provider in SHARED_CLOUD_ASR_PROVIDER_TYPES {
        transcription.register(*provider, cloud_asr.clone())?;
    }
    let native = Arc::new(NativeTranscriber(broker.clone()));
    transcription.register("ios-sensevoice", native.clone())?;
    transcription.register("apple-speech", native)?;
    let polisher = Arc::new(TextPolisherRouter::default());
    let cloud_llm = Arc::new(SharedCloudTextPolisher::new(credentials.clone()));
    for provider in SHARED_CLOUD_LLM_PROVIDER_TYPES.iter().filter(|&&provider| provider != "codex_oauth") {
        polisher.register(*provider, cloud_llm.clone())?;
    }
    style_library::prepare_staging(&config.data_dir)?;
    let recordings_directory = config.data_dir.join("recordings");
    let recorder = Arc::new(NativeRecorder(broker.clone(), recordings_directory.clone()));
    let traditional = Arc::new(PipelineDictationEngine::new(
        recorder.clone(),
        transcription.clone(),
        polisher.clone(),
    ));
    let dictation = Arc::new(DictationEngineRouter::new(traditional));
    let omni = Arc::new(SharedOmniDictationEngine::new(
        credentials.clone(),
        recorder,
    ));
    for provider in SHARED_OMNI_PROVIDER_TYPES {
        dictation.register_omni(*provider, omni.clone())?;
    }
    let mut deps = BackendDependencies::unsupported();
    deps.task_spawner = spawner.clone();
    deps.credential_store = credentials.clone();
    deps.dictation_engine = dictation;
    #[cfg(not(test))]
    let provider_service = openless_core::ProviderService::new(credentials.clone(), spawner);
    #[cfg(test)]
    let provider_service = openless_core::ProviderService::new_with_transport(
        credentials.clone(),
        spawner,
        Arc::new(control_tests::ModelListFixture),
    );
    deps.services.provider = Arc::new(provider_service.with_native_transcription(transcription.clone()));
    // Constructing the public marketplace service performs no request.
    deps.marketplace_config = Some(marketplace);
    deps.services.configure_auxiliary_runtime(polisher.clone(), transcription);
    deps.selection_runtime = Some(Arc::new(selection::SuppliedTextRuntime));
    deps.selection_polisher = Some(polisher);
    deps.host_actions = Arc::new(selection::SelectionPreviewActions);
    deps.text_inserter = Arc::new(KeyboardInserter(broker.clone()));
    let developer_mode = config.developer_mode;
    let config = BackendConfig {
        cache_dir: config.data_dir.join("cache"),
        data_dir: config.data_dir,
        platform: PlatformCapabilities {
            platform: "ios".into(),
            supports_ime_input: true,
            supports_local_asr: true,
            supports_in_app_dictation: true,
            ..Default::default()
        },
        locale: "zh-CN".into(),
        ..Default::default()
    };
    let backend = Arc::new(OpenLessBackend::new_with_repositories(config, deps, repos)?);
    runtime.block_on(backend.start())?;
    // Before publishing the Host handle; never scan an active capture.
    match recording_maintenance::at_startup(&backend, &recordings_directory) {
        Ok(report) if !report.recovered.is_empty() || !report.quarantined.is_empty()
            || !report.removed.is_empty() || !report.rejected.is_empty() =>
            broker.emit(json!({"kind":"recordingMaintenance","report":report})),
        Ok(_) => {},
        Err(_) => broker.emit(json!({"kind":"recordingMaintenance","error":"recording maintenance is unavailable"})),
    }
    control::forward_previews(&backend, broker.clone(), runtime.handle());
    let owner = Arc::new(Host {
        account_sync: account_sync::AccountSync::new(backend.services().marketplace.clone(), credentials),
        backend,
        broker,
        admission: Mutex::new(None),
        executor: runtime.handle().clone(),
        developer_mode,
        recordings_directory,
        control_busy: AtomicBool::new(false),
    });
    let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    hosts().lock().unwrap().insert(
        id,
        Entry {
            host: owner,
            runtime,
        },
    );
    Ok(id)
}

unsafe fn json_input<T: for<'de> Deserialize<'de>>(data: *const u8, len: usize) -> Option<T> {
    if data.is_null() || len == 0 || len > MAX_JSON {
        return None;
    }
    serde_json::from_slice(unsafe { std::slice::from_raw_parts(data, len) }).ok()
}
#[no_mangle]
pub extern "C" fn ol_ios_contract_version() -> u32 {
    1
}
#[no_mangle]
pub unsafe extern "C" fn ol_ios_create(
    data: *const u8,
    len: usize,
    event: Option<EventCallback>,
    audio: Option<PCMCallback>,
    context: u64,
) -> u64 {
    std::panic::catch_unwind(|| {
        let (Some(config), Some(event), Some(audio)) =
            (unsafe { json_input::<Config>(data, len) }, event, audio)
        else {
            return 0;
        };
        create(
            config,
            Callbacks {
                event,
                audio,
                context,
            },
        )
        .unwrap_or(0)
    })
    .unwrap_or(0)
}
#[no_mangle]
pub unsafe extern "C" fn ol_ios_command(id: u64, data: *const u8, len: usize) -> i32 {
    std::panic::catch_unwind(|| {
        let (Some(owner), Some(value)) = (host(id), unsafe { json_input::<Value>(data, len) }) else { return -1; };
        if value["operation"] == "control" {
            let Some(request_id) = value["requestID"].as_str().map(str::to_string) else { return -1; };
            match serde_json::from_value::<ControlAction>(value) {
                Ok(action) => owner.dispatch_control(request_id, action),
                Err(_) => owner.broker.emit(json!({"kind":"controlResult","requestID":request_id,"ok":false,"message":"invalid control fields"})),
            }
            return 0;
        }
        let Ok(command) = serde_json::from_value::<Command>(value) else { return -1; };
        match owner.dispatch(command) {
            Ok(()) => 0,
            Err(err) => {
                owner
                    .broker
                    .emit(json!({"kind":"commandError","message":err.message}));
                -2
            }
        }
    })
    .unwrap_or(-3)
}
#[no_mangle]
pub unsafe extern "C" fn ol_ios_reply(id: u64, data: *const u8, len: usize) -> i32 {
    std::panic::catch_unwind(|| {
        let (Some(owner), Some(reply)) = (host(id), unsafe { json_input::<Reply>(data, len) })
        else {
            return -1;
        };
        if owner.broker.reply(reply) {
            0
        } else {
            1
        }
    })
    .unwrap_or(-3)
}
#[no_mangle]
pub unsafe extern "C" fn ol_ios_feed_pcm(
    id: u64,
    session: *const c_char,
    pcm: *const u8,
    len: usize,
) -> i32 {
    std::panic::catch_unwind(|| {
        if pcm.is_null() || session.is_null() || len == 0 || len > MAX_PCM || len % 2 != 0 { return -1; }
        let Some(owner) = host(id) else { return -1; };
        let Some(session) = unsafe { CStr::from_ptr(session) }.to_str().ok().and_then(|s|Uuid::parse_str(s).ok()).map(SessionId::from_uuid) else { return -1; };
        let capture = owner.broker.captures.lock().unwrap().get(&session).cloned();
        let Some(capture) = capture else { return 1; };
        let bytes = unsafe { std::slice::from_raw_parts(pcm,len) };
        if let Some(archive) = &capture.archive { archive.append(bytes); }
        capture.consumer.consume_pcm_chunk(bytes);
        let elapsed = capture.bytes.fetch_add(len as u64,Ordering::Relaxed).saturating_add(len as u64) * 1000 / 32_000;
        let level = openless_core::external_audio::pcm_i16_le_rms(bytes);
        let _ = capture.progress.publish_level(elapsed,level);
        owner.broker.emit(json!({"kind":"level","sessionID":session.to_string(),"level":level,"elapsedMs":elapsed}));
        0
    }).unwrap_or(-3)
}
#[no_mangle]
pub extern "C" fn ol_ios_destroy(id: u64) {
    let _ = std::panic::catch_unwind(|| {
        let entry = hosts().lock().unwrap().remove(&id);
        if let Some(entry) = entry {
            entry.host.close_voice();
            entry.host.broker.close();
            let _ = entry.runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(2), entry.host.backend.shutdown()).await
            });
            entry.runtime.shutdown_timeout(Duration::from_secs(1));
        }
    });
}
