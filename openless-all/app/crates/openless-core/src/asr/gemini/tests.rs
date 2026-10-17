use super::*;
use crate::credentials::{
    ChannelKind, ChannelMutation, ChannelMutationResult, CredentialKey, CredentialNamespace,
    CredentialStore, InMemoryCredentialStore, SecretValue, ASR_API_KEY_ACCOUNT,
    ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT,
};
use crate::ports::AudioConsumer;
use crate::{ProviderApi, ProviderKind, ProviderRequest, ProviderService, TokioTaskSpawner};
use crate::{SharedCloudTranscriptionEngine, TranscriptionEngine};
use base64::Engine;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

mod omni;

struct UnexpectedTextStream;

impl crate::TextStreamSink for UnexpectedTextStream {
    fn publish(&self, _: crate::TextStreamChunk) -> Result<(), crate::BackendError> {
        panic!("cancelled batch transcription must not deliver text")
    }
}

#[test]
fn builds_files_and_interaction_urls() {
    assert_eq!(
        files_upload_url(DEFAULT_ENDPOINT).unwrap(),
        "https://generativelanguage.googleapis.com/upload/v1beta/files"
    );
    assert_eq!(
        interactions_url(DEFAULT_ENDPOINT).unwrap(),
        "https://generativelanguage.googleapis.com/v1beta/interactions"
    );
}

#[test]
fn full_interactions_endpoint_is_normalized_for_every_request() {
    for base in [
        "https://generativelanguage.googleapis.com",
        "https://generativelanguage.googleapis.com/v1beta/",
        "https://generativelanguage.googleapis.com/v1beta/interactions",
        "https://generativelanguage.googleapis.com/v1beta/interactions/?key=discard#fragment",
    ] {
        assert_eq!(
            files_upload_url(base).unwrap(),
            files_upload_url(DEFAULT_ENDPOINT).unwrap()
        );
        assert_eq!(
            interactions_url(base).unwrap(),
            interactions_url(DEFAULT_ENDPOINT).unwrap()
        );
        assert_eq!(
            file_url(base, "files/abc").unwrap(),
            format!("{DEFAULT_ENDPOINT}/files/abc")
        );
        assert_eq!(
            interaction_url(base, "abc").unwrap(),
            format!("{DEFAULT_ENDPOINT}/interactions/abc")
        );
    }
}

#[test]
fn upload_url_cannot_forward_credentials_to_another_origin() {
    assert!(validate_upload_url(DEFAULT_ENDPOINT, "https://example.com/upload").is_err());
    assert!(validate_upload_url(
        DEFAULT_ENDPOINT,
        "http://generativelanguage.googleapis.com/upload"
    )
    .is_err());
    assert!(validate_upload_url(
        DEFAULT_ENDPOINT,
        "https://generativelanguage.googleapis.com/upload/session"
    )
    .is_ok());
}

#[test]
fn serializes_documented_interaction_shape() {
    let body = interaction_body(
        DEFAULT_MODEL,
        "https://generativelanguage.googleapis.com/v1beta/files/abc",
        TranscriptionMode::Verbatim,
        &["zh".to_string()],
        &["Kubernetes".to_string()],
    );
    assert_eq!(body["model"], DEFAULT_MODEL);
    assert_eq!(body["input"][0]["type"], "audio");
    assert_eq!(
        body["input"][0],
        json!({
            "type": "audio",
            "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc",
            "mime_type": "audio/wav"
        })
    );
    assert_eq!(
        body["generation_config"]["transcription_config"]["language_codes"][0],
        "zh"
    );
    assert_eq!(
        body["generation_config"]["transcription_config"]["custom_vocabulary"][0],
        "Kubernetes"
    );
    assert_eq!(
        body["generation_config"]["transcription_config"]["mode"]["type"],
        "verbatim"
    );
}

#[tokio::test]
async fn transcribes_through_files_and_interactions_lifecycle() {
    let (endpoint, server) = transcription_server(
        "200 OK",
        r#"{"id":"interactions/i-1","status":"completed","steps":[{"type":"model_output","content":[{"type":"text","text":"hello Gemini"}]}]}"#,
    );
    let provider = GeminiBatchASR::new(
        "key".to_string(),
        endpoint,
        DEFAULT_MODEL.to_string(),
        Some("en".to_string()),
        vec![" Kubernetes ".to_string()],
    );
    provider.consume_pcm_chunk(&vec![0_u8; 32_000]);
    let transcript = provider.transcribe().await.unwrap();
    assert_eq!(transcript.text, "hello Gemini");
    assert_eq!(transcript.duration_ms, 1_000);
    assert_eq!(provider.buffer_duration_ms(), 0);
    server.join().unwrap();
}

/// Validate through the same service used by the settings Verify action.
#[tokio::test]
async fn verifies_completed_silence_without_transcript() {
    // This is the shape returned by the live API for the 500 ms silence probe.
    let (endpoint, server) = transcription_server("200 OK", r#"{"id":"i-1","status":"completed"}"#);
    let (service, request) = validation_service(&endpoint).await;
    assert!(service.validate(request).await.unwrap().ok);
    server.join().unwrap();
}

#[tokio::test]
async fn verify_preserves_http_status_and_cleans_up_after_failure() {
    for status in [
        "400 Bad Request",
        "401 Unauthorized",
        "403 Forbidden",
        "429 Too Many Requests",
        "500 Internal Server Error",
    ] {
        let (endpoint, server) = transcription_server(
            status,
            r#"{"error":{"message":"secret echoed by a gateway"}}"#,
        );
        let (service, request) = validation_service(&endpoint).await;
        let error = service.validate(request).await.unwrap_err();
        assert_eq!(
            error.message,
            format!("providerHttpStatus:{}", &status[..3])
        );
        assert!(!error.message.contains("secret"));
        server.join().unwrap();
    }
}

#[tokio::test]
async fn failed_or_malformed_interaction_never_validates_as_silence() {
    for body in [
        r#"{"status":"failed"}"#,
        r#"{"status":"incomplete"}"#,
        r#"{}"#,
    ] {
        let (endpoint, server) = transcription_server("200 OK", body);
        let (service, request) = validation_service(&endpoint).await;
        assert!(service.validate(request).await.is_err());
        server.join().unwrap();
    }
}

async fn validation_service(endpoint: &str) -> (ProviderService, ProviderRequest) {
    let (credentials, channel) = asr_credentials(endpoint).await;
    (
        ProviderService::new(credentials, Arc::new(TokioTaskSpawner)),
        ProviderRequest {
            kind: ProviderKind::Asr,
            channel_id: Some(channel),
            thinking_enabled: false,
        },
    )
}

async fn asr_credentials(endpoint: &str) -> (Arc<InMemoryCredentialStore>, String) {
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let ChannelMutationResult::Created(channel) = credentials
        .mutate_channel(ChannelMutation::Create {
            kind: ChannelKind::Asr,
            provider_type: PROVIDER_ID.to_owned(),
            name: "Gemini test".to_owned(),
        })
        .await
        .unwrap()
    else {
        panic!("channel not created")
    };
    for (account, value) in [
        (ASR_API_KEY_ACCOUNT, "key"),
        (ASR_ENDPOINT_ACCOUNT, endpoint),
        (ASR_MODEL_ACCOUNT, DEFAULT_MODEL),
    ] {
        credentials
            .write(
                CredentialKey::new(CredentialNamespace::Asr, Some(channel.clone()), account)
                    .unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    (credentials, channel)
}

/// Record the upload, transcription and cleanup contract at the HTTP boundary.
fn transcription_server(
    status: &'static str,
    body: &'static str,
) -> (String, thread::JoinHandle<()>) {
    transcription_server_check(status, body, |request| {
        assert_eq!(
            request["generation_config"]["transcription_config"]["mode"]["type"],
            "verbatim"
        );
    })
}

/// Exercise either pipeline's request contract, including pending-request cancellation.
fn transcription_server_check(
    status: &'static str,
    body: &'static str,
    check: impl FnOnce(&Value) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    transcription_server_with_poll(status, body, check, || {})
}

/// Observe polling only after the provider has stored the active interaction ID.
fn transcription_server_with_poll(
    status: &'static str,
    body: &'static str,
    check: impl FnOnce(&Value) + Send + 'static,
    on_poll: impl FnOnce() + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut upload = accept_upload(&listener);
        write_response(
            &mut upload,
            "200 OK",
            &[],
            &format!(
                r#"{{"file":{{"name":"files/file-1","uri":"http://{address}/v1beta/files/file-1"}},"state":"ACTIVE"}}"#
            ),
        );

        let mut request = accept_request(&listener);
        assert!(request.0.starts_with("GET /v1beta/files/file-1 HTTP/1.1"));
        write_response(
            &mut request.2,
            "200 OK",
            &[],
            r#"{"name":"files/file-1","state":"ACTIVE"}"#,
        );

        request = accept_request(&listener);
        assert!(request.0.starts_with("POST /v1beta/interactions HTTP/1.1"));
        let request_body: Value = serde_json::from_slice(&request.1).unwrap();
        assert_eq!(request_body["model"], DEFAULT_MODEL);
        assert_eq!(request_body["input"][0]["type"], "audio");
        assert_eq!(
            request_body["input"][0]["uri"],
            format!("http://{address}/v1beta/files/file-1")
        );
        assert_eq!(request_body["input"][0]["mime_type"], "audio/wav");
        assert!(request_body["input"][0].get("audio").is_none());
        check(&request_body);
        write_response(&mut request.2, status, &[], body);

        let mut file_deleted = false;
        if body.contains("in_progress") {
            let mut on_poll = Some(on_poll);
            loop {
                request = accept_request(&listener);
                // The cancellation task and worker cleanup may reach the server in either order.
                if request
                    .0
                    .starts_with("DELETE /v1beta/files/file-1 HTTP/1.1")
                {
                    assert!(!file_deleted, "audio file was deleted twice");
                    file_deleted = true;
                    write_response(&mut request.2, "200 OK", &[], "");
                    continue;
                }
                if request
                    .0
                    .starts_with("POST /v1beta/interactions/i-1/cancel ")
                {
                    write_response(&mut request.2, "200 OK", &[], r#"{"status":"cancelled"}"#);
                    break;
                }
                assert!(request.0.starts_with("GET /v1beta/interactions/i-1 "));
                write_response(&mut request.2, "200 OK", &[], body);
                if let Some(on_poll) = on_poll.take() {
                    on_poll();
                }
            }
        }
        if !file_deleted {
            request = accept_request(&listener);
            assert!(
                request
                    .0
                    .starts_with("DELETE /v1beta/files/file-1 HTTP/1.1"),
                "expected file cleanup, received {:?}",
                request.0.lines().next()
            );
            write_response(&mut request.2, "200 OK", &[], "");
        }
    });

    (format!("http://{address}/v1beta/interactions/"), server)
}

fn accept_upload(listener: &TcpListener) -> TcpStream {
    let mut request = accept_request(listener);
    assert!(request.0.starts_with("POST /upload/v1beta/files HTTP/1.1"));
    assert!(request
        .0
        .to_ascii_lowercase()
        .contains("x-goog-api-key: key"));
    assert!(request
        .0
        .to_ascii_lowercase()
        .contains("x-goog-upload-command: start"));
    assert!(String::from_utf8_lossy(&request.1).contains("openless-dictation.wav"));
    write_response(
        &mut request.2,
        "200 OK",
        &[(
            "X-Goog-Upload-URL",
            &format!("http://{}/upload-session", listener.local_addr().unwrap()),
        )],
        "",
    );
    request = accept_request(listener);
    assert!(request.0.starts_with("POST /upload-session HTTP/1.1"));
    assert!(request
        .0
        .to_ascii_lowercase()
        .contains("x-goog-upload-command: upload, finalize"));
    assert!(request
        .0
        .to_ascii_lowercase()
        .contains("x-goog-upload-offset: 0"));
    assert!(request.1.starts_with(b"RIFF"));
    request.2
}

#[tokio::test]
async fn cancelled_asr_verification_cancels_once_and_deletes_audio() {
    let (ready, received) = tokio::sync::oneshot::channel();
    let (endpoint, server) = transcription_server_with_poll(
        "200 OK",
        r#"{"id":"i-1","status":"in_progress"}"#,
        |_| {},
        move || ready.send(()).unwrap(),
    );
    let (service, request) = validation_service(&endpoint).await;
    let cancellation = crate::provider_transport::ProviderCancellation::new();
    let worker_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        service
            .validate_with_cancellation(request, worker_cancellation)
            .await
    });
    received.await.unwrap();
    cancellation.cancel();
    assert_eq!(
        task.await.unwrap().unwrap_err().code,
        crate::BackendErrorCode::Cancelled
    );
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn dropped_asr_finish_cancels_once_and_deletes_audio() {
    let (ready, received) = tokio::sync::oneshot::channel();
    let (endpoint, server) = transcription_server_with_poll(
        "200 OK",
        r#"{"id":"i-1","status":"in_progress"}"#,
        |_| {},
        move || ready.send(()).unwrap(),
    );
    let (credentials, channel) = asr_credentials(&endpoint).await;
    let engine = SharedCloudTranscriptionEngine::new(credentials);
    let context = crate::DictationContext {
        asr: crate::ProviderInvocation::new(&channel, PROVIDER_ID),
        ..Default::default()
    };
    let session = engine
        .start(
            crate::SessionId::new(),
            Arc::new(context),
            Arc::new(UnexpectedTextStream),
        )
        .await
        .unwrap();
    session.consume_pcm_chunk(&vec![0; 16_000]);
    let task = tokio::spawn(session.finish());
    received.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_asr_upload_deletes_file_without_creating_interaction() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (ready, received) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let mut upload = accept_upload(&listener);
        ready.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(5)).unwrap();
        write_response(
            &mut upload,
            "200 OK",
            &[],
            &format!(
                r#"{{"file":{{"name":"files/file-1","uri":"http://{address}/v1beta/files/file-1"}}}}"#
            ),
        );
        let mut cleanup = accept_request(&listener);
        assert!(cleanup
            .0
            .starts_with("DELETE /v1beta/files/file-1 HTTP/1.1"));
        write_response(&mut cleanup.2, "200 OK", &[], "");
    });
    let (service, request) = validation_service(&format!("http://{address}/v1beta")).await;
    let cancellation = crate::provider_transport::ProviderCancellation::new();
    let worker_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        service
            .validate_with_cancellation(request, worker_cancellation)
            .await
    });
    received.await.unwrap();
    cancellation.cancel();
    assert_eq!(
        task.await.unwrap().unwrap_err().code,
        crate::BackendErrorCode::Cancelled
    );
    release.send(()).unwrap();
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_upload_initialization_cancels_session_without_sending_audio() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (ready, received) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let mut initialization = accept_request(&listener);
        assert!(initialization
            .0
            .starts_with("POST /upload/v1beta/files HTTP/1.1"));
        ready.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(5)).unwrap();
        write_response(
            &mut initialization.2,
            "200 OK",
            &[(
                "X-Goog-Upload-URL",
                &format!("http://{address}/upload-session"),
            )],
            "",
        );
        let mut cleanup = accept_request(&listener);
        assert!(cleanup.0.starts_with("POST /upload-session HTTP/1.1"));
        assert!(cleanup
            .0
            .to_ascii_lowercase()
            .contains("x-goog-upload-command: cancel"));
        assert!(
            cleanup.1.is_empty(),
            "cancelled recording must not be uploaded"
        );
        write_response(&mut cleanup.2, "200 OK", &[], "");
    });
    let provider = Arc::new(GeminiBatchASR::new(
        "key".into(),
        format!("http://{address}/v1beta"),
        DEFAULT_MODEL.into(),
        None,
        Vec::new(),
    ));
    provider.consume_pcm_chunk(&[0; 16_000]);
    let worker = Arc::clone(&provider);
    let task = tokio::spawn(async move { worker.transcribe().await });
    received.await.unwrap();
    provider.cancel().await;
    release.send(()).unwrap();
    assert!(task
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("cancelled"));
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancellation_during_polling_cancels_once_and_deletes_audio() {
    let (ready, received) = tokio::sync::oneshot::channel();
    let (endpoint, server) = transcription_server_with_poll(
        "200 OK",
        r#"{"id":"i-1","status":"in_progress"}"#,
        |_| {},
        move || ready.send(()).unwrap(),
    );
    let provider = Arc::new(GeminiBatchASR::new(
        "key".into(),
        endpoint,
        DEFAULT_MODEL.into(),
        None,
        Vec::new(),
    ));
    provider.consume_pcm_chunk(&vec![0; 32_000]);
    let worker = Arc::clone(&provider);
    let task = tokio::spawn(async move { worker.transcribe().await });
    received.await.unwrap();
    provider.cancel().await;
    assert!(task.await.unwrap().is_err());
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}

#[test]
fn extracts_output_text_and_steps_text() {
    assert_eq!(
        extract_interaction_text(&json!({ "output_text": " hello " })),
        Some("hello".to_string())
    );
    assert_eq!(
        extract_interaction_text(&json!({
            "steps": [{ "content": [{ "type": "text", "text": "你好" }] }]
        })),
        Some("你好".to_string())
    );
}

#[test]
fn transcript_excludes_input_and_thought_steps() {
    let text = extract_interaction_text(&json!({"steps": [
        {"type": "user_input", "content": [{"type": "text", "text": "input"}]},
        {"type": "model_output", "content": [
            {"type": "thought", "text": "thinking"},
            {"type": "text", "text": "Actual transcript."}
        ]}
    ]}));
    assert_eq!(text.as_deref(), Some("Actual transcript."));
}

#[test]
fn pending_interaction_requires_polling() {
    assert_eq!(
        interaction_status(&json!({ "status": "in_progress", "id": "abc" })),
        InteractionStatus::Pending
    );
    assert_eq!(
        interaction_status(&json!({ "status": "completed", "output_text": "ok" })),
        InteractionStatus::Completed
    );
}

#[test]
fn vocabulary_is_bounded_and_deduplicated() {
    let words = normalize_vocabulary(vec![" foo ".into(), "foo".into(), "bar".into()]);
    assert_eq!(words, ["foo", "bar"]);
}

#[test]
fn encodes_canonical_pcm_as_wav_before_upload() {
    let samples = vec![0_i16; 16_000];
    let wav = encode_wav_16k_mono(&samples);
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(wav.len(), 44 + 32_000);
    assert_eq!(
        base64::engine::general_purpose::STANDARD.encode(&wav).len(),
        42_728
    );
}

fn accept_request(listener: &TcpListener) -> (String, Vec<u8>, TcpStream) {
    let started = Instant::now();
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "HTTP request timed out"
                );
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "client closed before sending HTTP headers");
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let content_length = header
        .lines()
        .find_map(|line| {
            let line = line.to_ascii_lowercase();
            line.strip_prefix("content-length:")
                .map(str::trim)
                .and_then(|value| value.parse::<usize>().ok())
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "client closed before sending HTTP body");
        bytes.extend_from_slice(&buffer[..count]);
    }
    (
        header,
        bytes[header_end..header_end + content_length].to_vec(),
        stream,
    )
}

fn write_response(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &str) {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes()).unwrap();
    stream.write_all(body.as_bytes()).unwrap();
    stream.flush().unwrap();
}
