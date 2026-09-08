use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;

use crate::credentials::SecretValue;
use crate::domains::{
    RemoteAuthResult, RemoteInputApi, RemoteInputConfig, RemoteInputRuntimeAdapter,
    RemoteInputServerConfig, RemoteInputStatus,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::{
    BackendEventKind, BackendEventPublisher, RemoteInputErrorEvent, RemoteInputRuntimeEvent,
};
use crate::types::SessionId;

pub const REMOTE_INPUT_MAX_PCM_FRAME_BYTES: usize = 64 * 1024;
pub const REMOTE_INPUT_PAIRING_PIN_LEN: usize = 6;
const REMOTE_AUDIO_FRAME_HEADER_BYTES: usize = 4 + 16 + 8;
const REMOTE_AUDIO_FRAME_MAGIC: &[u8; 4] = b"OL20";
const SUPPORTED_LOCALES: [&str; 5] = ["zh-CN", "zh-TW", "en", "ja", "ko"];
const PIN_MAX_FAILS: u32 = 5;
const PIN_LOCK_SECS: u64 = 60;
const PIN_FAILS_MAX_ENTRIES: usize = 256;
const PIN_GLOBAL_MAX_FAILS: u32 = 20;
const PIN_GLOBAL_WINDOW_SECS: u64 = 60;

pub fn validate_pairing_pin(pin: &str) -> bool {
    pin.len() == REMOTE_INPUT_PAIRING_PIN_LEN && pin.bytes().all(|byte| byte.is_ascii_digit())
}

/// Constant-time comparison for PINs and other short authentication tokens.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = u8::from(left.len() != right.len());
    let max = left.len().max(right.len());
    for index in 0..max {
        diff |= left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0);
    }
    diff == 0
}

pub struct RemoteFrameCodec;

impl RemoteFrameCodec {
    pub fn encode(
        session_id: SessionId,
        sequence: u64,
        pcm_s16le: &[u8],
    ) -> Result<Vec<u8>, BackendError> {
        validate_remote_pcm(pcm_s16le)?;
        let mut frame = Vec::with_capacity(REMOTE_AUDIO_FRAME_HEADER_BYTES + pcm_s16le.len());
        frame.extend_from_slice(REMOTE_AUDIO_FRAME_MAGIC);
        frame.extend_from_slice(session_id.as_uuid().as_bytes());
        frame.extend_from_slice(&sequence.to_be_bytes());
        frame.extend_from_slice(pcm_s16le);
        Ok(frame)
    }

    pub fn decode(frame: &[u8]) -> Result<(SessionId, u64, Vec<u8>), BackendError> {
        if frame.len() <= REMOTE_AUDIO_FRAME_HEADER_BYTES
            || frame.len() > REMOTE_AUDIO_FRAME_HEADER_BYTES + REMOTE_INPUT_MAX_PCM_FRAME_BYTES
            || &frame[..4] != REMOTE_AUDIO_FRAME_MAGIC
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "remote binary frame header or size is invalid",
            ));
        }
        let session_id = uuid::Uuid::from_slice(&frame[4..20])
            .map(SessionId::from_uuid)
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    format!("remote binary frame session UUID is invalid: {error}"),
                )
            })?;
        let sequence = u64::from_be_bytes(
            frame[20..28]
                .try_into()
                .expect("validated remote frame header has a complete sequence"),
        );
        let pcm_s16le = frame[REMOTE_AUDIO_FRAME_HEADER_BYTES..].to_vec();
        validate_remote_pcm(&pcm_s16le)?;
        Ok((session_id, sequence, pcm_s16le))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteStreamSequence {
    pub session_id: SessionId,
    next: u64,
}

impl RemoteStreamSequence {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            next: 0,
        }
    }

    pub fn accept(&mut self, sequence: u64) -> Result<(), BackendError> {
        if sequence != self.next {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "remote frame sequence is out of order or replayed",
            ));
        }
        self.next = self.next.saturating_add(1);
        Ok(())
    }
}

struct RemoteInputState {
    enabled: bool,
    running: bool,
    starting: bool,
    port: u16,
    urls: Vec<String>,
    urls_stale: bool,
    locale: String,
    pairing_pin: Option<SecretValue>,
    connections: HashMap<SessionId, RemoteConnectionState>,
    pin_fails: HashMap<String, (u32, Option<std::time::Instant>)>,
    global_pin_fails: (u32, std::time::Instant),
}

struct RemoteConnectionState {
    insert_text: bool,
    stream: Option<RemoteStreamState>,
}

struct RemoteStreamState {
    session_id: SessionId,
    sequence: RemoteStreamSequence,
    // Finalization may await a slow provider. Keep the owner available so
    // disconnect/cancel can still revoke it while rejecting further audio.
    finishing: bool,
}

pub struct RemoteInputService {
    runtime: Arc<dyn RemoteInputRuntimeAdapter>,
    events: Arc<Mutex<Option<BackendEventPublisher>>>,
    lifecycle: Arc<tokio::sync::Mutex<()>>,
    state: Arc<Mutex<RemoteInputState>>,
}

impl RemoteInputService {
    pub fn new(
        runtime: Arc<dyn RemoteInputRuntimeAdapter>,
        port: u16,
        locale: impl Into<String>,
    ) -> Result<Self, BackendError> {
        validate_remote_port(port)?;
        let locale = locale.into();
        validate_remote_locale(&locale)?;
        Ok(Self {
            runtime,
            events: Arc::new(Mutex::new(None)),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            state: Arc::new(Mutex::new(RemoteInputState {
                enabled: false,
                running: false,
                starting: false,
                port,
                urls: Vec::new(),
                urls_stale: false,
                locale,
                pairing_pin: None,
                connections: HashMap::new(),
                pin_fails: HashMap::new(),
                global_pin_fails: (0, std::time::Instant::now()),
            })),
        })
    }

    fn event_publisher(&self) -> BackendEventPublisher {
        self.events
            .lock()
            .expect("remote input event publisher lock poisoned")
            .clone()
            .expect("remote input service must be attached to an OpenLessBackend before use")
    }

    async fn ensure_pairing_pin(&self) -> Result<SecretValue, BackendError> {
        if let Some(pin) = self
            .state
            .lock()
            .expect("remote input state lock poisoned")
            .pairing_pin
            .clone()
        {
            return Ok(pin);
        }
        let loaded = self.runtime.load_pairing_pin().await?;
        let pin = match loaded.filter(is_valid_pin) {
            Some(pin) => pin,
            None => {
                let pin = SecretValue::new(generate_pairing_pin());
                self.runtime.persist_pairing_pin(pin.clone()).await?;
                pin
            }
        };
        self.state
            .lock()
            .expect("remote input state lock poisoned")
            .pairing_pin = Some(pin.clone());
        Ok(pin)
    }

    async fn stop_server_and_sessions(&self) -> Result<(), BackendError> {
        let sessions = {
            let mut state = self.state.lock().expect("remote input state lock poisoned");
            let sessions = state
                .connections
                .values_mut()
                .filter_map(|connection| connection.stream.take().map(|stream| stream.session_id))
                .collect::<Vec<_>>();
            state.connections.clear();
            state.running = false;
            state.starting = false;
            state.urls.clear();
            state.urls_stale = false;
            sessions
        };
        let mut first_error = None;
        for session_id in sessions {
            if let Err(error) = self.runtime.cancel_audio_session(session_id).await {
                first_error.get_or_insert(error);
            }
        }
        if let Err(error) = self.runtime.stop_server().await {
            first_error.get_or_insert(error);
        }
        self.publish_status();
        match first_error {
            Some(error) => Err(public_remote_error(&error)),
            None => Ok(()),
        }
    }

    async fn start_server(&self, port: u16) -> Result<(), BackendError> {
        self.ensure_pairing_pin().await?;
        {
            let mut state = self.state.lock().expect("remote input state lock poisoned");
            state.starting = true;
            state.running = false;
            state.urls.clear();
            state.urls_stale = false;
        }
        self.publish_status();
        match self
            .runtime
            .start_server(RemoteInputServerConfig { port })
            .await
        {
            Ok(binding) => {
                let mut state = self.state.lock().expect("remote input state lock poisoned");
                state.starting = false;
                state.running = true;
                state.port = binding.port;
                state.urls = binding.urls;
                state.urls_stale = binding.urls_stale;
                drop(state);
                self.publish_status();
                Ok(())
            }
            Err(error) => {
                {
                    let mut state = self.state.lock().expect("remote input state lock poisoned");
                    state.starting = false;
                    state.running = false;
                    state.urls.clear();
                    state.urls_stale = false;
                }
                let public = public_remote_error(&error);
                self.event_publisher().publish(
                    None,
                    BackendEventKind::RemoteInputFailed(RemoteInputErrorEvent {
                        reason: public.message.clone(),
                        port,
                    }),
                );
                Err(public)
            }
        }
    }

    fn publish_status(&self) {
        let state = self.state.lock().expect("remote input state lock poisoned");
        self.event_publisher().publish(
            None,
            BackendEventKind::RemoteInputStatusChanged(RemoteInputRuntimeEvent {
                running: state.running,
                port: state.running.then_some(state.port),
                urls: state.urls.clone(),
            }),
        );
    }

    async fn configure_inner(&self, config: RemoteInputConfig) -> Result<(), BackendError> {
        validate_remote_port(config.port)?;
        let _lifecycle = self.lifecycle.lock().await;
        let (was_running, old_port, old_enabled) = {
            let state = self.state.lock().expect("remote input state lock poisoned");
            (state.running, state.port, state.enabled)
        };
        if old_enabled == config.enabled
            && old_port == config.port
            && (!config.enabled || was_running)
        {
            return Ok(());
        }
        {
            let mut state = self.state.lock().expect("remote input state lock poisoned");
            state.enabled = config.enabled;
            state.port = config.port;
        }
        if was_running {
            self.stop_server_and_sessions().await?;
        }
        if config.enabled {
            self.start_server(config.port).await
        } else {
            self.publish_status();
            Ok(())
        }
    }

    async fn regenerate_pairing_pin_inner(&self) -> Result<(), BackendError> {
        let _lifecycle = self.lifecycle.lock().await;
        let pin = SecretValue::new(generate_pairing_pin());
        self.runtime
            .persist_pairing_pin(pin.clone())
            .await
            .map_err(|error| public_remote_error(&error))?;
        let (restart, port) = {
            let mut state = self.state.lock().expect("remote input state lock poisoned");
            state.pairing_pin = Some(pin);
            (state.running && state.enabled, state.port)
        };
        if restart {
            self.stop_server_and_sessions().await?;
            self.start_server(port).await?;
        }
        Ok(())
    }

    async fn authenticate_inner(
        &self,
        connection_id: SessionId,
        peer: String,
        candidate: SecretValue,
    ) -> Result<RemoteAuthResult, BackendError> {
        let _lifecycle = self.lifecycle.lock().await;
        // PIN rotation holds this same gate through persistence and server
        // restart. Read the secret only after acquiring it: otherwise an auth
        // queued during rotation can accept the old PIN on the new server.
        let expected = self.ensure_pairing_pin().await?;
        let mut state = self.state.lock().expect("remote input state lock poisoned");
        if !state.running {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "remote input server is not running",
            ));
        }
        let now = std::time::Instant::now();
        if now.duration_since(state.global_pin_fails.1).as_secs() >= PIN_GLOBAL_WINDOW_SECS {
            state.global_pin_fails = (0, now);
        }
        if state.global_pin_fails.0 >= PIN_GLOBAL_MAX_FAILS {
            return Ok(RemoteAuthResult::Locked);
        }
        if let Some((_, Some(until))) = state.pin_fails.get(&peer) {
            if now < *until {
                return Ok(RemoteAuthResult::Locked);
            }
            state.pin_fails.remove(&peer);
        }
        let pin_ok = constant_time_eq(
            candidate.expose_secret().as_bytes(),
            expected.expose_secret().as_bytes(),
        );
        if pin_ok {
            state.pin_fails.remove(&peer);
            state.global_pin_fails.0 = 0;
            state.connections.insert(
                connection_id,
                RemoteConnectionState {
                    insert_text: true,
                    stream: None,
                },
            );
            drop(state);
            self.publish_status();
            return Ok(RemoteAuthResult::Ok);
        }
        state.global_pin_fails.0 = state.global_pin_fails.0.saturating_add(1);
        if state.global_pin_fails.0 >= PIN_GLOBAL_MAX_FAILS {
            return Ok(RemoteAuthResult::Locked);
        }
        if state.pin_fails.len() >= PIN_FAILS_MAX_ENTRIES {
            state
                .pin_fails
                .retain(|_, (_, until)| until.is_some_and(|until| until > now));
        }
        let failure = state.pin_fails.entry(peer).or_insert((0, None));
        failure.0 = failure.0.saturating_add(1);
        if failure.0 >= PIN_MAX_FAILS {
            failure.1 = Some(now + std::time::Duration::from_secs(PIN_LOCK_SECS));
        }
        Ok(RemoteAuthResult::BadPin)
    }

    async fn disconnect_inner(&self, connection_id: SessionId) -> Result<(), BackendError> {
        let _lifecycle = self.lifecycle.lock().await;
        let session_id = self
            .state
            .lock()
            .expect("remote input state lock poisoned")
            .connections
            .remove(&connection_id)
            .and_then(|connection| connection.stream.map(|stream| stream.session_id));
        if let Some(session_id) = session_id {
            self.runtime
                .cancel_audio_session(session_id)
                .await
                .map_err(|error| public_remote_error(&error))?;
        }
        self.publish_status();
        Ok(())
    }

    async fn start_stream_inner(
        &self,
        connection_id: SessionId,
    ) -> Result<SessionId, BackendError> {
        let _lifecycle = self.lifecycle.lock().await;
        {
            let state = self.state.lock().expect("remote input state lock poisoned");
            match state.connections.get(&connection_id) {
                Some(RemoteConnectionState { stream: None, .. }) if state.running => {}
                Some(RemoteConnectionState {
                    stream: Some(_), ..
                }) => {
                    return Err(BackendError::new(
                        BackendErrorCode::Busy,
                        "remote input connection already has an active stream",
                    ));
                }
                _ => {
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "remote input connection is no longer active",
                    ));
                }
            }
        }
        let insert_text = self
            .state
            .lock()
            .expect("remote input state lock poisoned")
            .connections
            .get(&connection_id)
            .map(|connection| connection.insert_text)
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "remote input connection is no longer active",
                )
            })?;
        let session_id = self
            .runtime
            .start_audio_session(insert_text)
            .await
            .map_err(|error| public_remote_error(&error))?;
        let accepted = {
            let mut state = self.state.lock().expect("remote input state lock poisoned");
            let running = state.running;
            match state.connections.get_mut(&connection_id) {
                Some(connection) if connection.stream.is_none() && running => {
                    connection.stream = Some(RemoteStreamState {
                        session_id,
                        sequence: RemoteStreamSequence::new(session_id),
                        finishing: false,
                    });
                    true
                }
                _ => false,
            }
        };
        if !accepted {
            let _ = self.runtime.cancel_audio_session(session_id).await;
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "remote input connection closed while the stream was starting",
            ));
        }
        self.publish_status();
        Ok(session_id)
    }

    async fn feed_pcm_inner(
        &self,
        connection_id: SessionId,
        session_id: SessionId,
        sequence: u64,
        pcm_s16le: Vec<u8>,
    ) -> Result<(), BackendError> {
        validate_remote_pcm(&pcm_s16le)?;
        let _lifecycle = self.lifecycle.lock().await;
        {
            let mut state = self.state.lock().expect("remote input state lock poisoned");
            let stream = ensure_remote_stream_mut(&mut state, connection_id, session_id)?;
            if stream.finishing {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "remote audio stream is already finalizing",
                ));
            }
            stream.sequence.accept(sequence)?;
        }
        self.runtime
            .feed_audio(session_id, pcm_s16le)
            .await
            .map_err(|error| public_remote_error(&error))
    }

    async fn finish_stream_inner(
        &self,
        connection_id: SessionId,
        session_id: SessionId,
        cancel: bool,
    ) -> Result<(), BackendError> {
        {
            let _lifecycle = self.lifecycle.lock().await;
            let mut state = self.state.lock().expect("remote input state lock poisoned");
            let stream = ensure_remote_stream_mut(&mut state, connection_id, session_id)?;
            if cancel {
                state.connections.get_mut(&connection_id).unwrap().stream = None;
            } else {
                if stream.finishing {
                    return Err(BackendError::new(
                        BackendErrorCode::Busy,
                        "remote audio stream is already finalizing",
                    ));
                }
                stream.finishing = true;
            }
        }
        // Never hold the global lifecycle gate across ASR/LLM finalization.
        // The socket can send cancel, disappear, or rotate its PIN meanwhile;
        // all of those must be able to reach the still-owned audio session.
        let result = if cancel {
            self.runtime.cancel_audio_session(session_id).await
        } else {
            self.runtime.stop_audio_session(session_id).await
        };
        if !cancel {
            let mut state = self.state.lock().expect("remote input state lock poisoned");
            let connection = state.connections.get_mut(&connection_id);
            match connection {
                Some(connection)
                    if connection
                        .stream
                        .as_ref()
                        .is_some_and(|stream| stream.session_id == session_id) =>
                {
                    connection.stream = None;
                }
                _ => {
                    // Cancellation already removed this generation. In
                    // particular, a late finish must not clear a newer stream.
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "remote audio stream was cancelled during finalization",
                    ));
                }
            }
        }
        self.publish_status();
        result.map_err(|error| public_remote_error(&error))
    }
}

impl RemoteInputApi for RemoteInputService {
    fn bind_event_publisher(&self, publisher: BackendEventPublisher) {
        *self
            .events
            .lock()
            .expect("remote input event publisher lock poisoned") = Some(publisher);
    }

    fn status(&self) -> Result<RemoteInputStatus, BackendError> {
        let state = self.state.lock().expect("remote input state lock poisoned");
        Ok(RemoteInputStatus {
            enabled: state.enabled,
            running: state.running,
            starting: state.starting,
            port: state.port,
            urls: state.urls.clone(),
            urls_stale: state.urls_stale,
            locale: state.locale.clone(),
            connection_count: state.connections.len(),
            active_session_id: state
                .connections
                .values()
                .find_map(|connection| connection.stream.as_ref().map(|stream| stream.session_id)),
        })
    }

    fn read_pairing_pin(&self) -> BoxFuture<'static, Result<SecretValue, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let _lifecycle = service.lifecycle.lock().await;
            service.ensure_pairing_pin().await
        })
    }

    fn regenerate_pairing_pin(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.regenerate_pairing_pin_inner().await })
    }

    fn set_locale(&self, locale: String) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            validate_remote_locale(&locale)?;
            let _lifecycle = service.lifecycle.lock().await;
            {
                let mut state = service
                    .state
                    .lock()
                    .expect("remote input state lock poisoned");
                if state.locale == locale {
                    return Ok(());
                }
                state.locale = locale;
            }
            service.publish_status();
            Ok(())
        })
    }

    fn list_local_ips(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
        self.runtime.list_local_ips()
    }

    fn configure(&self, config: RemoteInputConfig) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.configure_inner(config).await })
    }

    fn authenticate(
        &self,
        connection_id: SessionId,
        peer: String,
        pin: SecretValue,
    ) -> BoxFuture<'static, Result<RemoteAuthResult, BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.authenticate_inner(connection_id, peer, pin).await })
    }

    fn disconnect(&self, connection_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.disconnect_inner(connection_id).await })
    }

    fn start_stream(
        &self,
        connection_id: SessionId,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.start_stream_inner(connection_id).await })
    }

    fn feed_pcm(
        &self,
        connection_id: SessionId,
        session_id: SessionId,
        sequence: u64,
        pcm_s16le: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            service
                .feed_pcm_inner(connection_id, session_id, sequence, pcm_s16le)
                .await
        })
    }

    fn set_insert(
        &self,
        connection_id: SessionId,
        insert_text: bool,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let mut state = state.lock().expect("remote input state lock poisoned");
            let connection = state.connections.get_mut(&connection_id).ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "remote input connection is no longer active",
                )
            })?;
            if connection.stream.is_some() {
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "remote insert preference cannot change during an active stream",
                ));
            }
            connection.insert_text = insert_text;
            Ok(())
        })
    }

    fn stop_stream(
        &self,
        connection_id: SessionId,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            service
                .finish_stream_inner(connection_id, session_id, false)
                .await
        })
    }

    fn cancel_stream(
        &self,
        connection_id: SessionId,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            service
                .finish_stream_inner(connection_id, session_id, true)
                .await
        })
    }
}

impl Clone for RemoteInputService {
    fn clone(&self) -> Self {
        Self {
            runtime: Arc::clone(&self.runtime),
            events: Arc::clone(&self.events),
            lifecycle: Arc::clone(&self.lifecycle),
            state: Arc::clone(&self.state),
        }
    }
}

fn ensure_remote_stream_mut(
    state: &mut RemoteInputState,
    connection_id: SessionId,
    session_id: SessionId,
) -> Result<&mut RemoteStreamState, BackendError> {
    if !state.running {
        return Err(BackendError::new(
            BackendErrorCode::Cancelled,
            "remote input stream is no longer active",
        ));
    }
    state
        .connections
        .get_mut(&connection_id)
        .and_then(|connection| connection.stream.as_mut())
        .filter(|stream| stream.session_id == session_id)
        .ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::Cancelled,
                "remote input stream is no longer active",
            )
        })
}

fn validate_remote_port(port: u16) -> Result<(), BackendError> {
    if port == 0 {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "remote input port must be between 1 and 65535",
        ));
    }
    Ok(())
}

fn validate_remote_pcm(pcm_s16le: &[u8]) -> Result<(), BackendError> {
    if pcm_s16le.len() < 2
        || !pcm_s16le.len().is_multiple_of(2)
        || pcm_s16le.len() > REMOTE_INPUT_MAX_PCM_FRAME_BYTES
    {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "remote PCM frame must be non-empty signed Int16LE and at most 65536 bytes",
        ));
    }
    Ok(())
}

fn validate_remote_locale(locale: &str) -> Result<(), BackendError> {
    if !SUPPORTED_LOCALES.contains(&locale) {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            format!("unsupported remote input locale: {locale}"),
        ));
    }
    Ok(())
}

fn is_valid_pin(pin: &SecretValue) -> bool {
    let value = pin.expose_secret();
    validate_pairing_pin(value)
}

fn generate_pairing_pin() -> String {
    const LIMIT: u32 = u32::MAX - (u32::MAX % 1_000_000);
    loop {
        let bytes = uuid::Uuid::new_v4().into_bytes();
        let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if value < LIMIT {
            return format!("{:06}", value % 1_000_000);
        }
    }
}

fn public_remote_error(error: &BackendError) -> BackendError {
    let message = if error.message == "port-in-use" {
        "port-in-use"
    } else {
        match error.code {
            BackendErrorCode::PermissionDenied => "remote input permission denied",
            BackendErrorCode::Unsupported => "remote input is unsupported by this host",
            BackendErrorCode::Cancelled => "remote input operation was cancelled",
            _ => "remote input operation failed",
        }
    };
    BackendError::new(error.code, message).retryable(error.retryable)
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn pairing_validation_and_comparison_are_constant_time_safe() {
        assert!(validate_pairing_pin("123456"));
        assert!(!validate_pairing_pin("12345"));
        assert!(!validate_pairing_pin("12345x"));
        assert!(constant_time_eq(b"123456", b"123456"));
        assert!(!constant_time_eq(b"123456", b"123457"));
        assert!(!constant_time_eq(b"123456", b"123"));
    }

    #[test]
    fn sequence_guard_rejects_replay_and_out_of_order_frames() {
        let mut guard = RemoteStreamSequence::new(SessionId::new());
        assert!(guard.accept(0).is_ok());
        assert!(guard.accept(0).is_err());
        assert!(guard.accept(2).is_err());
        assert!(guard.accept(1).is_ok());
    }
}
