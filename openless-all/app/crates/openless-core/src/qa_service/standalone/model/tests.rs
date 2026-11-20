use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn request() -> AskModelRequest {
    AskModelRequest {
        instructions: "fixed".to_owned(),
        messages: vec![AskModelMessage {
            role: AskModelRole::User,
            parts: vec![
                AskModelPart::Text("question".to_owned()),
                AskModelPart::Image(AskImage {
                    mime: AskImageMime::Png,
                    bytes: Arc::from([1_u8, 2, 3]),
                    width: 1,
                    height: 1,
                }),
            ],
        }],
        output_tokens: 123,
        deep_thinking: true,
        tools: AskModelTools::default(),
    }
}

#[test]
fn vision_and_thinking_require_verified_origin_format_and_model() {
    use LlmRequestFormat::*;
    let chat = crate::llm_protocol::LlmProtocolConfig::default();
    let messages = crate::llm_protocol::LlmProtocolConfig {
        format: Messages,
        ..chat.clone()
    };
    assert!(matches!(
        compatible_caps("openai", "gpt-5.4", &chat),
        (Thinking::OpenAi, true, Some(_))
    ));
    assert!(matches!(
        compatible_caps("custom", "gpt-5.4", &chat),
        (Thinking::Unverified, false, None)
    ));
    assert!(matches!(
        compatible_caps("openai", "gpt-5.4", &messages),
        (Thinking::Unverified, false, None)
    ));
    assert!(matches!(
        compatible_caps("anthropic", "claude-sonnet-4-6", &messages),
        (Thinking::ClaudeAdaptive, true, Some(1_000_000))
    ));

    let request = request();
    for (format, expected) in [
        (
            ChatCompletions,
            json!({"type":"image_url","image_url":{"url":"data:image/png;base64,AQID"}}),
        ),
        (
            Responses,
            json!({"type":"input_image","image_url":"data:image/png;base64,AQID","detail":"auto"}),
        ),
        (
            Messages,
            json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}}),
        ),
    ] {
        let wire = compatible_messages(format, &request);
        assert_eq!(wire[0]["content"], "fixed");
        assert_eq!(wire[1]["content"][1], expected);
    }
    let wire = gemini_body(&request, Thinking::GeminiLevel, 0.3);
    assert_eq!(
        wire["contents"][0]["parts"][1]["inlineData"]["data"],
        "AQID"
    );
    assert_eq!(wire["generationConfig"]["maxOutputTokens"], 123);
    assert_eq!(
        wire["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "HIGH"
    );
}

async fn read_request(socket: &mut tokio::net::TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2048];
    loop {
        let count = socket.read(&mut buffer).await.unwrap();
        assert!(count > 0);
        request.extend_from_slice(&buffer[..count]);
        if request.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
}

#[tokio::test]
async fn http_fragment_streams_before_explicit_completion_and_archives_reported_usage() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (allow_finish, finish) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        let delta = "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\r\n\r\n";
        let tail = concat!(
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":9,\"prompt_tokens_details\":{\"cached_tokens\":10}}}\r\n\r\n",
            "data: [DONE]\r\n\r\n"
        );
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n", delta.len() + tail.len()).as_bytes()).await.unwrap();
        let split = delta.find('你').unwrap() + 1;
        socket.write_all(&delta.as_bytes()[..split]).await.unwrap();
        socket.write_all(&delta.as_bytes()[split..]).await.unwrap();
        finish.await.unwrap();
        socket.write_all(tail.as_bytes()).await.unwrap();
    });
    let (send, mut progress) = tokio::sync::mpsc::unbounded_channel();
    let request = crate::net::credential_http_for_url(&url).post(url);
    let model = tokio::spawn(stream(
        request,
        LlmRequestFormat::ChatCompletions,
        false,
        Arc::new(move |progress| {
            assert!(send.send(progress).is_ok());
        }),
    ));
    let event = tokio::time::timeout(Duration::from_secs(3), progress.recv())
        .await
        .unwrap();
    assert!(matches!(event, Some(AskModelProgress::Text(text)) if text == "你好"));
    assert!(!model.is_finished());
    allow_finish.send(()).unwrap();
    assert!(matches!(
        model.await.unwrap(),
        Ok(AskModelCompletion::Complete)
    ));
    assert!(
        matches!(progress.recv().await, Some(AskModelProgress::Usage(tokens)) if tokens.cached_input == Some(10) && tokens.input == Some(20))
    );
    server.await.unwrap();
}

#[tokio::test]
async fn eof_and_length_stop_are_not_successful_completions() {
    for tail in [
        "",
        "data: {\"choices\":[{\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n",
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            let body = format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"partial\"}}}}]}}\n\n{tail}"
            );
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let partial = Arc::new(Mutex::new(String::new()));
        let output = partial.clone();
        let result = stream(
            crate::net::credential_http_for_url(&url).post(url),
            LlmRequestFormat::ChatCompletions,
            false,
            Arc::new(move |event| {
                if let AskModelProgress::Text(text) = event {
                    output.lock().push_str(&text);
                }
            }),
        )
        .await;
        assert!(matches!(result, Err(error) if error.message == "askModelIncomplete"));
        assert_eq!(partial.lock().as_str(), "partial");
        server.await.unwrap();
    }
}

#[test]
fn provider_completion_checks_never_show_native_thought_text() {
    for (format, gemini, delivery, terminal) in [
        (
            LlmRequestFormat::Responses,
            false,
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"answer\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
        ),
        (
            LlmRequestFormat::Messages,
            false,
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"answer\"}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n",
        ),
        (
            LlmRequestFormat::ChatCompletions,
            true,
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"thought\":true,\"text\":\"hidden\"},{\"text\":\"answer\"}]}}]}\n\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":20,\"cachedContentTokenCount\":4,\"thoughtsTokenCount\":2}}\n\n",
        ),
    ] {
        let mut parser = TextEventStream::new(format).for_ask(gemini);
        parser.push(delivery.as_bytes()).unwrap();
        assert!(matches!(parser.next().unwrap(), Some(StreamEvent::Text(text)) if text == "answer"));
        assert!(parser.finish().is_err());
        parser.push(terminal.as_bytes()).unwrap();
        while parser.next().unwrap().is_some() {}
        assert!(parser.finish().is_ok());
        if gemini {
            assert_eq!(parser.reported_tokens().cached_input, Some(4));
            assert_eq!(parser.reported_tokens().reasoning, Some(2));
        }
    }
}
