//! A dictation can span several Tencent sessions. Only the 60-second Hunyuan
//! preview needs rotation; other Tencent engines keep a single connection.
use super::*;
use futures_util::future::{AbortHandle, Abortable};

const SEGMENT_TARGET_BYTES: usize = 45 * 32_000;
const SEGMENT_MAX_BYTES: usize = 50 * 32_000;
const SILENCE_BYTES: usize = 500 * 32;
// Conservative amplitude gate, not a speech classifier. The hard limit also
// handles continuous speech, background noise, and silence we fail to detect.
const SILENCE_PEAK: u16 = 180;

enum Input {
    Audio(Vec<u8>),
    Finish(oneshot::Sender<Result<(), TencentCloudASRError>>),
}

#[derive(Default)]
struct InputState {
    sender: Option<mpsc::UnboundedSender<Input>>,
    pending: Vec<u8>,
    opened: bool,
    accepting: bool,
}

#[derive(Default)]
struct Shared {
    active: ParkingMutex<Option<Arc<TencentCloudSession>>>,
    sink: ParkingMutex<Option<Arc<dyn TextStreamSink>>>,
    terminal: ParkingMutex<Option<Result<RawTranscript, TencentCloudASRError>>>,
    final_tx: ParkingMutex<Option<oneshot::Sender<Result<RawTranscript, TencentCloudASRError>>>>,
}

impl Shared {
    fn complete(&self, result: Result<RawTranscript, TencentCloudASRError>) {
        let mut terminal = self.terminal.lock();
        if terminal.is_some() {
            return;
        }
        *terminal = Some(result.clone());
        if let Some(tx) = self.final_tx.lock().take() {
            let _ = tx.send(result);
        }
    }

    fn stop_active(&self) {
        if let Some(session) = self.active.lock().take() {
            session.cancel();
        }
    }

    fn error(&self) -> TencentCloudASRError {
        match self.terminal.lock().as_ref() {
            Some(Err(error)) => error.clone(),
            _ => TencentCloudASRError::ConnectionFailed,
        }
    }
}

struct PrefixedSink {
    shared: std::sync::Weak<Shared>,
    prefix: String,
}

impl TextStreamSink for PrefixedSink {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), crate::BackendError> {
        if let Some(shared) = self.shared.upgrade() {
            if shared.terminal.lock().is_some() {
                return Ok(());
            }
            let sink = shared.sink.lock().clone();
            if let Some(sink) = sink {
                sink.publish(TextStreamChunk {
                    text: super::super::mimo::join_transcript_chunks(&[
                        self.prefix.clone(),
                        chunk.text,
                    ]),
                    offset: 0,
                })?;
            }
        }
        Ok(())
    }
}

pub struct TencentCloudStreamingASR {
    credentials: TencentCloudCredentials,
    endpoint: String,
    task_spawner: Arc<dyn TaskSpawner>,
    shared: Arc<Shared>,
    input: ParkingMutex<InputState>,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, TencentCloudASRError>>>>,
    worker: ParkingMutex<Option<AbortHandle>>,
    target_bytes: usize,
    max_bytes: usize,
}

impl TencentCloudStreamingASR {
    pub fn new(credentials: TencentCloudCredentials) -> Self {
        Self::with_task_spawner(credentials, Arc::new(TokioTaskSpawner))
    }

    pub fn with_task_spawner(
        credentials: TencentCloudCredentials,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self::with_endpoint(credentials, task_spawner, DEFAULT_ENDPOINT.to_string())
    }

    fn with_endpoint(
        credentials: TencentCloudCredentials,
        task_spawner: Arc<dyn TaskSpawner>,
        endpoint: String,
    ) -> Self {
        Self {
            credentials,
            endpoint,
            task_spawner,
            shared: Arc::new(Shared::default()),
            input: ParkingMutex::new(InputState::default()),
            final_rx: ParkingMutex::new(None),
            worker: ParkingMutex::new(None),
            target_bytes: SEGMENT_TARGET_BYTES,
            max_bytes: SEGMENT_MAX_BYTES,
        }
    }

    pub fn set_partial_sink(&self, sink: Arc<dyn TextStreamSink>) {
        *self.shared.sink.lock() = Some(sink);
    }

    pub fn connect_url(&self) -> String {
        connect_url_at(
            &self.endpoint,
            &self.credentials,
            chrono::Utc::now().timestamp(),
            random_nonce(),
            Uuid::new_v4().to_string(),
        )
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), TencentCloudASRError> {
        let (final_tx, final_rx) = oneshot::channel();
        let (connect_abort, connect_registration) = AbortHandle::new_pair();
        let session = Arc::new(TencentCloudSession::with_endpoint(
            self.credentials.clone(),
            self.task_spawner.clone(),
            self.endpoint.clone(),
        ));
        session.set_partial_sink(Arc::new(PrefixedSink {
            shared: Arc::downgrade(&self.shared),
            prefix: String::new(),
        }));
        {
            // Install cancellation before starting either handshake. Keep the
            // final receiver and active session atomic with respect to cancel.
            let mut input = self.input.lock();
            if input.opened || self.shared.terminal.lock().is_some() {
                return Err(TencentCloudASRError::ConnectionFailed);
            }
            input.opened = true;
            *self.final_rx.lock() = Some(final_rx);
            *self.shared.final_tx.lock() = Some(final_tx);
            *self.shared.active.lock() = Some(session.clone());
            *self.worker.lock() = Some(connect_abort);
        }
        let connection = Abortable::new(session.open_session(), connect_registration).await;
        let error = match connection {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(_) => Some(self.shared.error()),
        };
        if let Some(error) = error {
            self.shared.complete(Err(error.clone()));
            // cancel may already have removed shared.active before the socket
            // was installed; the local session still needs explicit cleanup.
            session.cancel();
            self.shared.stop_active();
            return Err(error);
        }
        let (sender, receiver) = mpsc::unbounded_channel();
        let (abort, registration) = AbortHandle::new_pair();
        {
            // Serialize startup with cancel so cancellation cannot leave a worker
            // running or accept audio after the caller has discarded the session.
            let mut input = self.input.lock();
            if self.shared.terminal.lock().is_some() {
                session.cancel();
                self.shared.stop_active();
                return Err(self.shared.error());
            }
            input.sender = Some(sender);
            input.accepting = true;
            *self.worker.lock() = Some(abort);
        }
        let shared = self.shared.clone();
        let credentials = self.credentials.clone();
        let endpoint = self.endpoint.clone();
        let spawner = self.task_spawner.clone();
        let limits = (self.target_bytes, self.max_bytes);
        self.task_spawner.spawn(Box::pin(async move {
            let work = run(
                receiver,
                session,
                shared.clone(),
                credentials,
                endpoint,
                spawner,
                limits,
            );
            if let Ok(Err(error)) = Abortable::new(work, registration).await {
                shared.complete(Err(error));
            }
            shared.stop_active();
        }));
        Ok(())
    }

    pub async fn send_last_frame(&self) -> Result<(), TencentCloudASRError> {
        let (done_tx, done_rx) = oneshot::channel();
        {
            let mut input = self.input.lock();
            if !input.accepting {
                return Err(self.shared.error());
            }
            input.accepting = false;
            let sender = input.sender.take().ok_or_else(|| self.shared.error())?;
            if !input.pending.is_empty() {
                let tail = std::mem::take(&mut input.pending);
                sender
                    .send(Input::Audio(tail))
                    .map_err(|_| self.shared.error())?;
            }
            sender
                .send(Input::Finish(done_tx))
                .map_err(|_| self.shared.error())?;
        }
        // Includes queued audio replay and all intermediate segment handshakes.
        // Each socket write, handshake and final-result wait is separately bounded.
        done_rx.await.map_err(|_| self.shared.error())?
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, TencentCloudASRError> {
        self.await_final_result_with_timeout(FINAL_RESULT_TIMEOUT)
            .await
    }

    pub async fn await_final_result_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<RawTranscript, TencentCloudASRError> {
        let receiver = self
            .final_rx
            .lock()
            .take()
            .ok_or(TencentCloudASRError::NoFinalResult)?;
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(self.shared.error()),
            Err(_) => {
                self.cancel();
                Err(TencentCloudASRError::FinalResultTimeout)
            }
        }
    }

    pub fn cancel(&self) {
        let mut input = self.input.lock();
        input.accepting = false;
        input.pending.clear();
        input.sender = None;
        self.shared
            .complete(Err(TencentCloudASRError::NoFinalResult));
        if let Some(worker) = self.worker.lock().take() {
            worker.abort();
        }
        self.shared.stop_active();
    }
}

impl Drop for TencentCloudStreamingASR {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl AudioConsumer for TencentCloudStreamingASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let mut input = self.input.lock();
        if !input.accepting || pcm.is_empty() {
            return;
        }
        let Some(sender) = input.sender.clone() else {
            return;
        };
        input.pending.extend_from_slice(pcm);
        let complete_bytes =
            input.pending.len() / TARGET_AUDIO_CHUNK_BYTES * TARGET_AUDIO_CHUNK_BYTES;
        // Keep at most one incomplete frame; never drain from the front once per
        // frame (history replay may supply hours of PCM in a single call).
        for frame in input.pending[..complete_bytes].chunks(TARGET_AUDIO_CHUNK_BYTES) {
            if sender.send(Input::Audio(frame.to_vec())).is_err() {
                input.accepting = false;
                input.pending.clear();
                return;
            }
        }
        input.pending.drain(..complete_bytes);
    }
}

async fn finish_segment(
    session: &TencentCloudSession,
) -> Result<RawTranscript, TencentCloudASRError> {
    session.send_last_frame().await?;
    session.await_final_result().await
}

async fn run(
    mut receiver: mpsc::UnboundedReceiver<Input>,
    first: Arc<TencentCloudSession>,
    shared: Arc<Shared>,
    credentials: TencentCloudCredentials,
    endpoint: String,
    spawner: Arc<dyn TaskSpawner>,
    (target, maximum): (usize, usize),
) -> Result<(), TencentCloudASRError> {
    let rotate = credentials.resolved_model() == DEFAULT_MODEL;
    let mut active = Some(first);
    let mut segment_bytes = 0usize;
    let mut quiet_bytes = 0usize;
    let mut total_bytes = 0u64;
    let mut completed = String::new();
    while let Some(input) = receiver.recv().await {
        match input {
            Input::Audio(audio) => {
                // Delayed creation avoids an empty trailing connection when stop
                // lands exactly on the segment boundary.
                if active.is_none() {
                    let next = Arc::new(TencentCloudSession::with_endpoint(
                        credentials.clone(),
                        spawner.clone(),
                        endpoint.clone(),
                    ));
                    next.set_partial_sink(Arc::new(PrefixedSink {
                        shared: Arc::downgrade(&shared),
                        prefix: completed.clone(),
                    }));
                    *shared.active.lock() = Some(next.clone());
                    next.open_session().await?;
                    active = Some(next);
                }
                let session = active.as_ref().unwrap();
                session.consume_pcm_chunk(&audio);
                total_bytes += audio.len() as u64;
                segment_bytes += audio.len();
                if audio
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs() <= SILENCE_PEAK)
                {
                    quiet_bytes += audio.len();
                } else {
                    quiet_bytes = 0;
                }
                if rotate
                    && (segment_bytes >= maximum
                        || (segment_bytes >= target && quiet_bytes >= SILENCE_BYTES))
                {
                    let result = finish_segment(session).await?;
                    completed =
                        super::super::mimo::join_transcript_chunks(&[completed, result.text]);
                    shared.active.lock().take();
                    active = None;
                    segment_bytes = 0;
                    quiet_bytes = 0;
                }
            }
            Input::Finish(done) => {
                let result = async {
                    if let Some(session) = active.as_ref() {
                        let result = finish_segment(session).await?;
                        completed =
                            super::super::mimo::join_transcript_chunks(&[completed, result.text]);
                    }
                    Ok(RawTranscript {
                        text: completed,
                        duration_ms: total_bytes / BYTES_PER_MS,
                    })
                }
                .await;
                shared.complete(result.clone());
                let _ = done.send(result.map(|_| ()));
                return Ok(());
            }
        }
    }
    Err(TencentCloudASRError::NoFinalResult)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    fn credentials() -> TencentCloudCredentials {
        TencentCloudCredentials {
            app_id: "test-app".into(),
            secret_id: "test-id".into(),
            secret_key: "test-key".into(),
            model: DEFAULT_MODEL.into(),
        }
    }

    fn client(endpoint: String, max_bytes: usize, model: &str) -> Arc<TencentCloudStreamingASR> {
        let mut creds = credentials();
        creds.model = model.into();
        let mut asr =
            TencentCloudStreamingASR::with_endpoint(creds, Arc::new(TokioTaskSpawner), endpoint);
        asr.max_bytes = max_bytes;
        asr.target_bytes = max_bytes;
        Arc::new(asr)
    }

    async fn listener() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/asr/v2", listener.local_addr().unwrap());
        (listener, endpoint)
    }

    async fn serve_segments(
        listener: TcpListener,
        expected: Vec<Vec<u8>>,
        texts: Vec<&'static str>,
    ) {
        let mut ids = std::collections::HashSet::new();
        for (audio, text) in expected.into_iter().zip(texts) {
            let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut voice_id = String::new();
            // Tungstenite's Callback API requires an unboxed ErrorResponse;
            // the mock cannot change that return type to satisfy this lint.
            #[allow(clippy::result_large_err)]
            let capture_voice_id = |request: &Request, response: Response| {
                let url = url::Url::parse(&format!("ws://localhost{}", request.uri())).unwrap();
                voice_id = url
                    .query_pairs()
                    .find(|(name, _)| name == "voice_id")
                    .unwrap()
                    .1
                    .into_owned();
                Ok(response)
            };
            let mut ws = tokio_tungstenite::accept_hdr_async(stream, capture_voice_id)
                .await
                .unwrap();
            assert!(
                ids.insert(voice_id),
                "each segment must have a fresh voice_id"
            );
            ws.send(Message::Text(r#"{"code":0}"#.into()))
                .await
                .unwrap();
            let mut received = Vec::new();
            loop {
                match ws.next().await.unwrap().unwrap() {
                    Message::Binary(bytes) => {
                        received.extend_from_slice(&bytes);
                        assert!(
                            received.len() <= audio.len(),
                            "session exceeded its audio budget"
                        );
                        // Index restarts at zero for every connection.
                        ws.send(Message::Text(serde_json::json!({"code":0,"result":{"index":0,"slice_type":1,"voice_text_str":text}}).to_string())).await.unwrap();
                    }
                    Message::Text(end) => {
                        assert_eq!(end, r#"{"type":"end"}"#);
                        break;
                    }
                    other => panic!("unexpected frame {other:?}"),
                }
            }
            assert_eq!(
                received, audio,
                "no skipped or duplicated PCM at a boundary"
            );
            ws.send(Message::Text(serde_json::json!({"code":0,"result":{"index":0,"slice_type":2,"voice_text_str":text},"final":1}).to_string())).await.unwrap();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "do not open an empty trailing session"
        );
    }

    #[tokio::test]
    async fn rotation_preserves_pcm_duration_and_prefixed_live_snapshots() {
        let (listener, endpoint) = listener().await;
        let limit = TARGET_AUDIO_CHUNK_BYTES * 2;
        let pcm: Vec<u8> = (0..limit * 2 + 100).map(|i| (i % 251) as u8).collect();
        let expected = pcm.chunks(limit).map(|part| part.to_vec()).collect();
        let server = tokio::spawn(serve_segments(
            listener,
            expected,
            vec!["第一段。", "第二段。", "尾段。"],
        ));
        let asr = client(endpoint, limit, DEFAULT_MODEL);
        let sink = Arc::new(super::super::super::TranscriptCapture::default());
        asr.set_partial_sink(sink.clone());
        asr.open_session().await.unwrap();
        // Producer calls need not align with frames or even sample boundaries.
        for bytes in pcm.chunks(777) {
            asr.consume_pcm_chunk(bytes);
        }
        asr.send_last_frame().await.unwrap();
        let transcript = asr.await_final_result().await.unwrap();
        assert_eq!(transcript.text, "第一段。第二段。尾段。");
        assert_eq!(transcript.duration_ms, pcm.len() as u64 / BYTES_PER_MS);
        sink.assert_snapshots(&[
            "第一段。",
            "第一段。",
            "第一段。",
            "第一段。第二段。",
            "第一段。第二段。",
            "第一段。第二段。",
            "第一段。第二段。尾段。",
            "第一段。第二段。尾段。",
        ]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn exact_boundary_does_not_create_empty_segment_or_deduplicate_repeated_speech() {
        let (listener, endpoint) = listener().await;
        let limit = TARGET_AUDIO_CHUNK_BYTES;
        let server = tokio::spawn(serve_segments(
            listener,
            vec![vec![1; limit]; 2],
            vec!["再说一次。", "再说一次。"],
        ));
        let asr = client(endpoint, limit, DEFAULT_MODEL);
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; limit * 2]);
        asr.send_last_frame().await.unwrap();
        assert_eq!(
            asr.await_final_result().await.unwrap().text,
            "再说一次。再说一次。"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn other_tencent_models_keep_one_session() {
        let (listener, endpoint) = listener().await;
        let limit = TARGET_AUDIO_CHUNK_BYTES;
        let pcm = vec![1; limit * 3];
        let server = tokio::spawn(serve_segments(listener, vec![pcm.clone()], vec!["完整。"]));
        let asr = client(endpoint, limit, "16k_zh");
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&pcm);
        asr.send_last_frame().await.unwrap();
        assert_eq!(asr.await_final_result().await.unwrap().text, "完整。");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn quiet_boundary_rotates_after_target_before_hard_limit() {
        let (listener, endpoint) = listener().await;
        let mut instance = TencentCloudStreamingASR::with_endpoint(
            credentials(),
            Arc::new(TokioTaskSpawner),
            endpoint,
        );
        instance.target_bytes = TARGET_AUDIO_CHUNK_BYTES * 4;
        instance.max_bytes = TARGET_AUDIO_CHUNK_BYTES * 6;
        let asr = Arc::new(instance);
        let mut first = vec![100; TARGET_AUDIO_CHUNK_BYTES];
        first.extend(vec![0; TARGET_AUDIO_CHUNK_BYTES * 3]);
        let tail = vec![100; 100];
        let server = tokio::spawn(serve_segments(
            listener,
            vec![first.clone(), tail.clone()],
            vec!["停顿。", "继续。"],
        ));
        asr.open_session().await.unwrap();
        first.extend(tail);
        asr.consume_pcm_chunk(&first);
        asr.send_last_frame().await.unwrap();
        assert_eq!(asr.await_final_result().await.unwrap().text, "停顿。继续。");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn disconnect_after_partial_is_an_error_not_success() {
        let (listener, endpoint) = listener().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(r#"{"code":0}"#.into()))
                .await
                .unwrap();
            ws.next().await.unwrap().unwrap();
            ws.send(Message::Text(
                r#"{"code":0,"result":{"index":0,"slice_type":1,"voice_text_str":"仅前半段"}}"#
                    .into(),
            ))
            .await
            .unwrap();
            ws.close(None).await.unwrap();
        });
        let asr = client(endpoint, SEGMENT_MAX_BYTES, DEFAULT_MODEL);
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; TARGET_AUDIO_CHUNK_BYTES * 2]);
        let _ = asr.send_last_frame().await;
        assert!(asr.await_final_result().await.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancel_during_initial_websocket_handshake_unblocks_open_and_closes_socket() {
        use tokio::io::AsyncReadExt;

        let (listener, endpoint) = listener().await;
        let (connected_tx, connected_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            connected_tx.send(()).unwrap();
            // Withhold the WebSocket upgrade response. Cancellation should drop
            // the connection instead of waiting for CONNECT_TIMEOUT.
            let mut request = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut request))
                .await
                .unwrap()
                .unwrap();
        });
        let asr = client(endpoint, SEGMENT_MAX_BYTES, DEFAULT_MODEL);
        let opening = {
            let asr = asr.clone();
            tokio::spawn(async move { asr.open_session().await })
        };
        connected_rx.await.unwrap();
        asr.cancel();
        assert!(tokio::time::timeout(Duration::from_secs(1), opening)
            .await
            .expect("initial connection must be cancellable")
            .unwrap()
            .is_err());
        assert!(asr.await_final_result().await.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancel_during_initial_application_handshake_unblocks_open_and_closes_socket() {
        let (listener, endpoint) = listener().await;
        let (connected_tx, connected_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            // Complete the WebSocket upgrade but withhold Tencent's code=0
            // application handshake. Cancellation must close this socket too.
            connected_tx.send(()).unwrap();
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(2), ws.next())
                    .await
                    .unwrap(),
                Some(Ok(Message::Close(_))) | None | Some(Err(_))
            ));
        });
        let asr = client(endpoint, SEGMENT_MAX_BYTES, DEFAULT_MODEL);
        let opening = {
            let asr = asr.clone();
            tokio::spawn(async move { asr.open_session().await })
        };
        connected_rx.await.unwrap();
        asr.cancel();
        assert!(tokio::time::timeout(Duration::from_secs(1), opening)
            .await
            .expect("initial application handshake must be cancellable")
            .unwrap()
            .is_err());
        assert!(asr.await_final_result().await.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancel_during_segment_finalization_unblocks_finish_and_closes_socket() {
        let (listener, endpoint) = listener().await;
        let (ended_tx, ended_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(r#"{"code":0}"#.into()))
                .await
                .unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Binary(_)
            ));
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            ended_tx.send(()).unwrap();
            assert!(matches!(
                ws.next().await,
                Some(Ok(Message::Close(_))) | None | Some(Err(_))
            ));
        });
        let asr = client(endpoint, TARGET_AUDIO_CHUNK_BYTES, DEFAULT_MODEL);
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; TARGET_AUDIO_CHUNK_BYTES * 3]);
        let finishing = {
            let asr = asr.clone();
            tokio::spawn(async move { asr.send_last_frame().await })
        };
        ended_rx.await.unwrap();
        asr.cancel();
        assert!(tokio::time::timeout(Duration::from_secs(2), finishing)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(asr.await_final_result().await.is_err());
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn seventy_three_seconds_rotates_at_fifty_and_keeps_the_tail() {
        let (listener, endpoint) = listener().await;
        let first = vec![100; 50 * 32_000];
        let second = vec![101; 23 * 32_000];
        let mut pcm = first.clone();
        pcm.extend_from_slice(&second);
        let server = tokio::spawn(serve_segments(
            listener,
            vec![first, second],
            vec!["前五十秒。", "后面二十三秒。"],
        ));
        let asr = client(endpoint, SEGMENT_MAX_BYTES, DEFAULT_MODEL);
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&pcm);
        // Exercise the same whole-file replay shape used by retranscription.
        tokio::time::timeout(Duration::from_secs(90), asr.send_last_frame())
            .await
            .unwrap()
            .unwrap();
        let result = asr.await_final_result().await.unwrap();
        assert_eq!(result.duration_ms, 73_000);
        assert_eq!(result.text, "前五十秒。后面二十三秒。");
        server.await.unwrap();
    }
    #[tokio::test]
    async fn next_segment_rejection_fails_the_whole_transcript() {
        let (listener, endpoint) = listener().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(r#"{"code":0}"#.into()))
                .await
                .unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Binary(_)
            ));
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            ws.send(Message::Text(r#"{"code":0,"result":{"index":0,"slice_type":2,"voice_text_str":"已经识别的前段。"},"final":1}"#.into())).await.unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                r#"{"code":4006,"message":"SENSITIVE_SERVER_MESSAGE"}"#.into(),
            ))
            .await
            .unwrap();
        });
        let asr = client(endpoint, TARGET_AUDIO_CHUNK_BYTES, DEFAULT_MODEL);
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; TARGET_AUDIO_CHUNK_BYTES * 2]);
        assert!(asr.send_last_frame().await.is_err());
        let error = asr.await_final_result().await.unwrap_err();
        assert!(matches!(error, TencentCloudASRError::RateLimited));
        assert!(!error.to_string().contains("SENSITIVE"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancel_during_next_handshake_closes_new_connection() {
        let (listener, endpoint) = listener().await;
        let (connected_tx, connected_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(r#"{"code":0}"#.into()))
                .await
                .unwrap();
            ws.next().await.unwrap().unwrap();
            ws.next().await.unwrap().unwrap();
            ws.send(Message::Text(r#"{"code":0,"final":1}"#.into()))
                .await
                .unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let mut next = tokio_tungstenite::accept_async(stream).await.unwrap();
            connected_tx.send(()).unwrap();
            // Never send the application handshake; cancellation must interrupt it.
            assert!(matches!(
                next.next().await,
                Some(Ok(Message::Close(_))) | None | Some(Err(_))
            ));
        });
        let asr = client(endpoint, TARGET_AUDIO_CHUNK_BYTES, DEFAULT_MODEL);
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; TARGET_AUDIO_CHUNK_BYTES * 2]);
        connected_rx.await.unwrap();
        asr.cancel();
        assert!(asr.await_final_result().await.is_err());
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn rotation_happens_while_microphone_input_is_still_open() {
        let (listener, endpoint) = listener().await;
        let (rotated_tx, rotated_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            for index in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                ws.send(Message::Text(r#"{"code":0}"#.into()))
                    .await
                    .unwrap();
                assert!(matches!(
                    ws.next().await.unwrap().unwrap(),
                    Message::Binary(_)
                ));
                if index == 0 {
                    assert!(matches!(
                        ws.next().await.unwrap().unwrap(),
                        Message::Text(_)
                    ));
                    ws.send(Message::Text(r#"{"code":0,"result":{"index":0,"slice_type":2,"voice_text_str":"前段。"},"final":1}"#.into())).await.unwrap();
                } else {
                    // Notify only once the next session has received live audio.
                    rotated_tx.send(()).unwrap();
                    assert!(matches!(
                        ws.next().await.unwrap().unwrap(),
                        Message::Text(_)
                    ));
                    ws.send(Message::Text(r#"{"code":0,"result":{"index":0,"slice_type":2,"voice_text_str":"后段。"},"final":1}"#.into())).await.unwrap();
                    break;
                }
            }
        });
        let asr = client(endpoint, TARGET_AUDIO_CHUNK_BYTES, DEFAULT_MODEL);
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; TARGET_AUDIO_CHUNK_BYTES]);
        // Recording is still open when the first session is finalized.
        asr.consume_pcm_chunk(&vec![2; TARGET_AUDIO_CHUNK_BYTES]);
        tokio::time::timeout(Duration::from_secs(2), rotated_rx)
            .await
            .unwrap()
            .unwrap();
        asr.send_last_frame().await.unwrap();
        assert_eq!(asr.await_final_result().await.unwrap().text, "前段。后段。");
        server.await.unwrap();
    }
}
