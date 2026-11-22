use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(feature = "ask-attachments")]
#[tokio::test]
async fn official_suite_and_flash_aliases_carry_the_selected_photo_as_user_bytes() {
    use crate::credentials::{CredentialKey, CredentialNamespace};
    use crate::{InMemoryCredentialStore, SecretValue};
    use image::{ImageEncoder, Rgb, RgbImage};

    let store = InMemoryCredentialStore::default();
    store.write(CredentialKey::new(
        CredentialNamespace::Application, None, crate::deepseek_suite::CONFIG_ACCOUNT,
    ).unwrap(), SecretValue::new(r#"{"apiKey":"disposable-fixture-key","voice":"stella"}"#))
        .await.unwrap();
    let route = crate::deepseek_suite::invocation(false);
    let CloudPolisherProvider::OpenAi(ActiveLLMProvider::OpenAI(suite)) =
        build_cloud_channel_provider(&store, &route, true, false, None).await.unwrap()
    else { panic!("suite resolved to an unexpected protocol") };

    let digits = [0x7b6f_u16, 0x2492, 0x73e7, 0x73cf, 0x5bc9, 0x79cf, 0x79ef, 0x7249, 0x7bef, 0x7bcf];
    let seed = uuid::Uuid::new_v4();
    let mut pixels = RgbImage::from_pixel(100, 42, Rgb([255_u8; 3]));
    for (index, value) in seed.as_bytes().iter().take(4).enumerate() {
        let glyph = digits[usize::from(*value % 10)];
        for row in 0..5 {
            for column in 0..3 {
                if glyph & (1 << (14 - row * 3 - column)) != 0 {
                    for dy in 0..6 {
                        for dx in 0..6 {
                            pixels.put_pixel(4 + index as u32 * 24 + column * 6 + dx, 6 + row * 6 + dy, Rgb([0_u8; 3]));
                        }
                    }
                }
            }
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(pixels.as_raw(), 100, 42, image::ExtendedColorType::Rgb8).unwrap();
    let exact = AskModelPart::Image(AskImage {
        mime: AskImageMime::Png, bytes: Arc::from(png.as_slice()), width: 100, height: 42,
    });

    use LlmRequestFormat::*;
    let mut providers = vec![(suite, ChatCompletions, "/v1/chat/completions")];
    for (format, base, path) in [
        (ChatCompletions, crate::deepseek_suite::ENDPOINT, "/v1/chat/completions"),
        (Responses, "https://api.deepseek.com", "/responses"),
        (Responses, crate::deepseek_suite::ENDPOINT, "/v1/responses"),
        (Messages, "https://api.deepseek.com/anthropic", "/anthropic/messages"),
        (Messages, "https://api.deepseek.com/anthropic/v1", "/anthropic/v1/messages"),
    ] {
        for alias in ["deepseek-flash", "deepseek-v4-flash", "deepseek-v4-flash-vision-exp"] {
            let provider = OpenAICompatibleLLMProvider::new(
                crate::polish::OpenAICompatibleConfig::new("deepseek", "Fixture", base, "fixture", alias)
                    .with_protocol(crate::llm_protocol::LlmProtocolConfig { format, ..Default::default() }),
            );
            providers.push((provider, format, path));
        }
    }
    for (provider, expected_format, path) in providers {
        let model = PreparedModel::new(route.provider_id.clone(), Wire::Compatible(provider));
        assert!(model.capabilities().image_input);
        for already_in_chat in [false, true] {
            let mut request = request();
            request.deep_thinking = false;
            request.messages[0].parts = vec![AskModelPart::Text("Read the visible code.".into()), exact.clone()];
            if already_in_chat {
                request.messages.insert(0, AskModelMessage {
                    role: AskModelRole::Assistant, parts: vec![AskModelPart::Text("Prior disposable turn.".into())],
                });
            }
            let (builder, format, _) = model.request_builder(&request).unwrap();
            assert_eq!(format, expected_format);
            let wire = builder.build().unwrap();
            assert_eq!(wire.url().host_str(), Some("api.deepseek.com"));
            assert_eq!(wire.url().path(), path);
            let body: Value = serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
            assert_eq!(body["model"], model.label().model);
            assert!(body.get("previous_response_id").is_none());
            let history_key = if format == Responses {
                assert_eq!(body["store"], false);
                "input"
            } else { "messages" };
            let final_message = body[history_key].as_array().unwrap().last().unwrap();
            assert_eq!(final_message["role"], "user");
            let part = &final_message["content"][1];
            let data = match format {
                ChatCompletions => {
                    assert_eq!(part["type"], "image_url");
                    part["image_url"]["url"].as_str().unwrap().strip_prefix("data:image/png;base64,").unwrap()
                }
                Responses => {
                    assert_eq!(part["type"], "input_image");
                    part["image_url"].as_str().unwrap().strip_prefix("data:image/png;base64,").unwrap()
                }
                Messages => {
                    assert_eq!(part["type"], "image");
                    assert_eq!(part["source"]["type"], "base64");
                    assert_eq!(part["source"]["media_type"], "image/png");
                    part["source"]["data"].as_str().unwrap()
                }
            };
            use base64::Engine as _;
            assert!(base64::engine::general_purpose::STANDARD.decode(data).unwrap() == png,
                "the actual request must contain the fixture's original PNG");
        }
    }
    for (endpoint, model, format) in [
        (crate::deepseek_suite::ENDPOINT, "deepseek-pro", ChatCompletions),
        (crate::deepseek_suite::ENDPOINT, "deepseek-v4-pro", Responses),
        ("https://api.deepseek.com/anthropic", "deepseek-pro", Messages),
        ("https://fixture.invalid/v1", "deepseek-flash", ChatCompletions),
        ("https://fixture.invalid/anthropic", "deepseek-v4-flash", Messages),
        ("https://api.deepseek.com/v1", "deepseek-flash", Messages),
    ] {
        let unknown = PreparedModel::new("fixture".into(), Wire::Compatible(
            OpenAICompatibleLLMProvider::new(crate::polish::OpenAICompatibleConfig::new(
                "deepseek", "Fixture", endpoint, "fixture", model,
            ).with_protocol(crate::llm_protocol::LlmProtocolConfig { format, ..Default::default() })),
        ));
        assert!(!unknown.capabilities().image_input);
    }
}

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

#[tokio::test]
async fn assistant_http_loop_round_trips_time_and_calculation_before_final_answer() {
    use crate::assistant::{AssistantDependencies, PersonalAssistant};
    use crate::assistant_api::*;

    struct Factory(Arc<PreparedModel>);
    impl AskModelFactory for Factory {
        fn prepare(&self, _: bool) -> BoxFuture<'_, Result<Arc<dyn AskModel>, BackendError>> {
            Box::pin(async { Ok(self.0.clone() as Arc<dyn AskModel>) })
        }
    }
    struct LiveClock;
    impl AssistantContextProvider for LiveClock {
        fn read(
            &self,
            _: AskTurnKey,
            query: AssistantContextQuery,
            _: AskCancellation,
        ) -> BoxFuture<'_, Result<AssistantContextResult, BackendError>> {
            Box::pin(async move {
                assert!(matches!(query, AssistantContextQuery::CurrentTime));
                let now = chrono::Local::now().to_rfc3339();
                Ok(AssistantContextResult::Available {
                    data: json!({"iso":now}),
                    source: "test-live-system-clock".into(),
                    as_of: now,
                })
            })
        }
    }
    async fn request_json(socket: &mut tokio::net::TcpStream) -> Value {
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(header_end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= header_end + 4 + length {
                    return serde_json::from_slice(&bytes[header_end + 4..header_end + 4 + length])
                        .unwrap();
                }
            }
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut final_text = String::new();
        for index in 0..3 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = request_json(&mut socket).await;
            assert!(request["tools"].to_string().contains("read_context"));
            let results: Vec<_> = request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|m| m["role"] == "tool")
                .collect();
            assert_eq!(results.len(), index);
            let frame = match index {
                0 => {
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"clock-1","function":{"name":"read_context","arguments":json!({"kind":"currentTime","query":null,"limit":null}).to_string()}}]},"finish_reason":"tool_calls"}]})
                }
                1 => {
                    let time: Value =
                        serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
                    let parsed =
                        chrono::DateTime::parse_from_rfc3339(time["data"]["iso"].as_str().unwrap())
                            .unwrap();
                    assert!((chrono::Utc::now().timestamp() - parsed.timestamp()).abs() <= 5);
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"calc-1","function":{"name":"calculator","arguments":"{\"expression\":\"0.1+0.2\"}"}}]},"finish_reason":"tool_calls"}]})
                }
                _ => {
                    let time: Value =
                        serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
                    let calculation: Value =
                        serde_json::from_str(results[1]["content"].as_str().unwrap()).unwrap();
                    assert_eq!(calculation["decimal"], "0.3");
                    final_text = format!(
                        "当前时间 {}；计算结果 {}。",
                        time["data"]["iso"].as_str().unwrap(),
                        calculation["decimal"].as_str().unwrap()
                    );
                    json!({"choices":[{"delta":{"content":final_text},"finish_reason":"stop"}]})
                }
            };
            let body = format!("data: {frame}\n\ndata: {{\"choices\":[],\"usage\":{{\"prompt_tokens\":100,\"completion_tokens\":10}}}}\n\ndata: [DONE]\n\n");
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
        final_text
    });
    let provider = OpenAICompatibleLLMProvider::new(crate::polish::OpenAICompatibleConfig::new(
        "fixture",
        "Fixture",
        &endpoint,
        "fixture",
        "fixture-model",
    ));
    let mut model = PreparedModel::new("fixture".into(), Wire::Compatible(provider));
    // The local fixture has this declared window; this does not label an unknown provider.
    model.capabilities.context_window = Some(128_000);
    let runtime = PersonalAssistant::new(AssistantDependencies {
        models: Arc::new(Factory(Arc::new(model))),
        context: Arc::new(LiveClock),
        search: Arc::new(crate::ask_search::WebSearchTool::new(Arc::new(
            crate::InMemoryCredentialStore::default(),
        ))),
        usage: None,
    });
    let received = Arc::new(Mutex::new(Vec::new()));
    let observed = received.clone();
    let answer = tokio::time::timeout(
        Duration::from_secs(10),
        runtime.run(
            AssistantRequest {
                key: AskTurnKey {
                    conversation_id: Uuid::new_v4(),
                    turn_id: Uuid::new_v4(),
                },
                client_request_id: "http-fixture".into(),
                text: "现在几点？再精确算一下0.1+0.2。".into(),
                messages: vec![],
                selection: None,
                attachments: vec![],
                options: AskOptions::default(),
            },
            AskCancellation::default(),
            Arc::new(move |event| observed.lock().push(event)),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(answer.text, server.await.unwrap());
    let events = received.lock();
    let ids: std::collections::HashSet<_> = events
        .iter()
        .filter_map(|event| match event {
            AssistantEvent::Usage(usage) => Some(usage.call_id),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                AssistantEvent::Tool {
                    status: AssistantToolStatus::Completed,
                    ..
                }
            ))
            .count(),
        2
    );
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
