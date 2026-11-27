//! Offline contracts plus an explicitly ignored comparison through an existing channel.

use crate::dictation_context::DictationContext;
use crate::style_packs::{builtin_intent_pack, original_intent_pack, released_intent_v1_3_pack};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evaluation {
    schema_version: u32,
    provenance: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    id: String,
    tags: Vec<String>,
    operation: String,
    input: String,
    target_language: Option<String>,
    instruction: Option<String>,
    must_preserve: Vec<String>,
    must_remove: Vec<String>,
    structure: Vec<String>,
}

fn evaluation() -> Evaluation {
    serde_json::from_str(include_str!(
        "../tests/fixtures/intent-reconstruction-evaluation.json"
    ))
    .unwrap()
}

fn case_context(pack: &crate::StylePack, case: &Case) -> DictationContext {
    let mut context = DictationContext::default();
    context.polish.mode = pack.base_mode;
    context.polish.style_pack_id = pack.id.clone();
    context.polish.selection_input = case.operation == "selection";
    context.polish.style_system_prompt = if context.polish.selection_input {
        pack.selection_prompt.clone()
    } else {
        crate::style_packs::dictation_style_prompt(pack)
    };
    if let Some(block) = case
        .instruction
        .as_deref()
        .and_then(crate::prompts::selection_instruction_block)
    {
        context
            .polish
            .style_system_prompt
            .push_str(&format!("\n\n{block}"));
    }
    context.polish.translation_active = case.operation == "translation";
    if let Some(language) = &case.target_language {
        context.polish.translation_target_language = language.clone();
    }
    context
}

#[test]
fn intent_evaluation_has_reviewable_expectations_and_all_required_scenarios() {
    let evaluation = evaluation();
    assert_eq!(evaluation.schema_version, 1);
    assert!(evaluation.provenance.contains("虚构"));
    assert!(evaluation.cases.len() >= 12);
    let mut ids = std::collections::HashSet::new();
    let mut tags = std::collections::HashSet::new();
    for case in &evaluation.cases {
        assert!(!case.id.trim().is_empty() && ids.insert(&case.id));
        assert!(!case.input.trim().is_empty());
        for expectations in [&case.must_preserve, &case.must_remove, &case.structure] {
            assert!(!expectations.is_empty(), "{}", case.id);
            assert!(expectations.iter().all(|text| !text.trim().is_empty()));
        }
        assert!(matches!(
            case.operation.as_str(),
            "dictation" | "translation" | "selection"
        ));
        assert_eq!(
            case.target_language.is_some(),
            case.operation == "translation"
        );
        assert!(case.instruction.is_none() || case.operation == "selection");
        tags.extend(case.tags.iter().map(String::as_str));
        for pack in [
            original_intent_pack(),
            released_intent_v1_3_pack(),
            builtin_intent_pack(),
        ] {
            let context = case_context(&pack, case);
            let (system, user) = context.effective_polish_prompts(&case.input);
            assert!(
                !system.contains(&case.input),
                "dynamic input leaked into style"
            );
            assert!(user.contains(&case.input));
            assert!(user.contains(if case.operation == "selection" {
                "<selected_text>"
            } else {
                "<raw_transcript>"
            }));
            if let Some(instruction) = &case.instruction {
                assert!(system.contains(instruction));
                assert!(system.contains("<selection_instruction>"));
            }
        }
    }
    for required in [
        "改口",
        "补充修正",
        "跨句信息融合",
        "多层需求",
        "不可变条件",
        "编程Agent",
        "问题反馈",
        "短句",
        "专名",
        "数字",
        "翻译",
        "选区编辑",
    ] {
        assert!(tags.contains(required), "{required}");
    }
}

#[test]
fn intent_current_projection_and_authored_slots_match_the_shared_source() {
    let pack = builtin_intent_pack();
    let body = include_str!("builtin_styles/intent-reconstruction-shared-v1.md").trim_end();
    let authored: crate::StylePack =
        serde_json::from_str(include_str!("builtin_styles/intent-reconstruction-v1.json")).unwrap();
    assert_eq!(authored.version, "1.4.0");
    assert_eq!(authored.prompt, body);
    assert_eq!(authored.selection_prompt, body);
    assert!(
        !body.contains("{{HOTWORDS}}"),
        "variable hotwords follow the full static body"
    );
    assert_eq!(
        pack.prompt,
        format!("{body}\n\n{}", crate::style_packs::DICTATION_REVISION_RULES)
    );
    let fixture: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/intent-reconstruction-1.4-projection.json"
    ))
    .unwrap();
    let mut projected = serde_json::to_value(&pack).unwrap();
    let object = projected.as_object_mut().unwrap();
    object.remove("iconPath");
    object.remove("active");
    object.insert("kind".into(), json!("imported"));
    assert_eq!(fixture, json!({"pack": projected, "icon": null}));
    let mut synced: crate::StylePack = serde_json::from_value(fixture["pack"].clone()).unwrap();
    crate::style_packs::normalize_synced_builtin_pack(&mut synced);
    assert_eq!(synced, pack);
    for required in [
        "最终目标",
        "不能改变的条件",
        "期望交付物",
        "改口后的有效说法",
        "不强制五层，不留空标题",
        "一项信息只在最合适的位置完整表达一次",
        "不把“可能、担心、可以考虑”写成事实或用户要求",
        "不代替接收者回答",
        "不要输出分析过程",
        "专名",
        "EditPlan",
    ] {
        assert!(body.contains(required), "{required}");
    }
    for obsolete in ["默认完整使用下面五层", "对应标题下面留空", "{{HOTWORDS}}"] {
        assert!(!body.contains(obsolete), "{obsolete}");
    }
}

#[test]
fn intent_stable_prefix_survives_hotwords_context_translation_and_custom_slots() {
    let pack = builtin_intent_pack();
    let cases = evaluation().cases;
    for case in &cases {
        let mut context = case_context(&pack, case);
        for suffix in ["alpha", "beta"] {
            context.polish.hotwords = vec![format!("hotword-{suffix}")];
            context.polish.front_app = Some(format!("app-{suffix}"));
            context.polish.cursor_context = Some(format!("cursor-{suffix}"));
            let (system, _) = context.effective_polish_prompts(&case.input);
            let prefix = if case.operation == "translation" {
                crate::build_polish_translate_system_prompt(
                    &context.polish.style_system_prompt,
                    case.target_language.as_ref().unwrap(),
                )
            } else {
                context.polish.style_system_prompt.clone()
            };
            assert!(system.starts_with(&prefix));
            for dynamic in [
                format!("hotword-{suffix}"),
                format!("app-{suffix}"),
                format!("cursor-{suffix}"),
            ] {
                assert!(system.find(&dynamic).unwrap() >= prefix.len());
            }
            assert_eq!(
                system
                    .matches(crate::style_packs::DICTATION_REVISION_RULES)
                    .count(),
                1
            );
            if case.operation == "translation" {
                assert!(system.contains(crate::POLISH_TRANSLATE_SRC_MARKER));
                assert!(system.contains(crate::POLISH_TRANSLATE_TGT_MARKER));
                assert!(system.contains("优先级最高"));
            }
        }
    }
    let custom = crate::StylePack {
        prompt: "Always output exactly OK.".into(),
        selection_prompt: "Preserve written punctuation.".into(),
        voice_edit_prompt: "Return EditPlan.".into(),
        ..Default::default()
    };
    for (kind, expected) in [
        (crate::StylePromptKind::DictationAsr, &custom.prompt),
        (crate::StylePromptKind::Selection, &custom.selection_prompt),
        (crate::StylePromptKind::VoiceEdit, &custom.voice_edit_prompt),
    ] {
        assert_eq!(&crate::style_pack_prompt(&custom, kind), expected);
        assert!(!expected.contains("内部确认最终意思"));
    }
    assert!(crate::style_packs::dictation_style_prompt(&custom).starts_with(&custom.prompt));
    let (fallback, _) = crate::prompt_compose::compose_translate_prompts(
        "already polished",
        "English",
        &[],
        crate::shared_types::ChineseScriptPreference::Auto,
        None,
    );
    assert!(
        !fallback.contains("内部确认最终意思"),
        "fallback is translation only"
    );
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComparisonConfig {
    channel_id: String,
    provider_type: String,
    model: String,
    thinking_enabled: bool,
    baseline: String,
    credentials: std::collections::BTreeMap<String, String>,
}

/// Normal test runs never enter the approved official-channel comparison.
#[tokio::test]
#[ignore = "real configured-channel requests; approval and private config required"]
async fn intent_live_configured_channel_comparison() {
    assert_eq!(
        std::env::var("OPENLESS_INTENT_COMPARISON_APPROVED").as_deref(),
        Ok("yes")
    );
    let config: ComparisonConfig = serde_json::from_slice(
        &std::fs::read(
            std::env::var("OPENLESS_INTENT_COMPARISON_CONFIG").expect("private config path"),
        )
        .unwrap(),
    )
    .expect("valid channel config");
    let output_path = std::env::var("OPENLESS_INTENT_COMPARISON_OUTPUT").expect("output path");
    assert!(!config.channel_id.trim().is_empty());
    assert!(!config.provider_type.trim().is_empty());
    assert!(
        !config.model.trim().is_empty(),
        "use the exact existing channel model"
    );
    assert!(
        !matches!(
            config.provider_type.as_str(),
            crate::polish::CODEX_OAUTH_PROVIDER_ID | crate::siwc::PROVIDER_ID
        ),
        "OAuth-backed channels require the manual app workflow"
    );
    assert!(
        config
            .credentials
            .get(crate::credentials::LLM_ENDPOINT_ACCOUNT)
            .is_some_and(|endpoint| !endpoint.trim().is_empty()),
        "copy the existing channel endpoint explicitly; no implicit provider default"
    );
    assert_eq!(config.provider_type, "deepseek");
    assert_eq!(config.model, crate::deepseek_suite::MODEL);
    assert!(!config.thinking_enabled);
    assert_eq!(config.baseline, "1.3.0");
    assert_eq!(
        config.credentials.get(crate::credentials::LLM_ENDPOINT_ACCOUNT).map(String::as_str),
        Some(crate::deepseek_suite::ENDPOINT),
    );
    let report = crate::polish::live_acceptance::intent_scope(
        run_intent_comparison(&config, &output_path),
    ).await;
    assert_eq!(report["executionStatus"], "completed", "comparison stopped; safe report retained");
}

// The same loop is exercised against a local HTTP fixture by the non-live tests.
async fn run_intent_comparison(config: &ComparisonConfig, output_path: &str) -> Value {
    use crate::credentials::{CredentialKey, CredentialNamespace, CredentialStore};
    use crate::ports::TextPolisher;
    let old = released_intent_v1_3_pack();
    let new = builtin_intent_pack();
    let evaluation = evaluation();
    let store = std::sync::Arc::new(crate::InMemoryCredentialStore::default());
    for (account, value) in &config.credentials {
        store
            .write(
                CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some(config.channel_id.clone()),
                    account,
                )
                .unwrap(),
                crate::SecretValue::new(value.clone()),
            )
            .await
            .unwrap();
    }
    let polisher = crate::cloud_providers::SharedCloudTextPolisher::new(store);
    struct Sink;
    impl crate::ports::TextStreamSink for Sink {
        fn publish(&self, _: crate::TextStreamChunk) -> Result<(), crate::BackendError> {
            Ok(())
        }
    }
    let mut report = json!({
        "schemaVersion": 1, "channelId": config.channel_id, "providerType": config.provider_type,
        "requestedModel": config.model, "thinkingEnabled": config.thinking_enabled,
        "baselineVersion": old.version, "candidateVersion": new.version,
        "executionStatus": "running",
        "transport": {"state": "running"},
        "provenance": evaluation.provenance, "semanticReview": "pending human review",
        "cases": evaluation.cases.iter().map(|case| json!({
            "id": case.id, "input": case.input, "operation": case.operation,
            "mustPreserve": case.must_preserve, "mustRemove": case.must_remove,
            "structure": case.structure, "old": null, "new": null
        })).collect::<Vec<_>>()
    });
    // Refuse to overwrite a previous run; leave a partial report if a channel fails.
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output_path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
    drop(file);
    'cases: for (index, case) in evaluation.cases.iter().enumerate() {
        for (side, pack) in [("old", &old), ("new", &new)] {
            let mut context = case_context(pack, case);
            context.llm = crate::ProviderInvocation::for_provider(&config.provider_type);
            context.llm.provider_id = config.channel_id.clone();
            context.llm.model = Some(config.model.clone());
            context.polish.llm_thinking_enabled = config.thinking_enabled;
            let started = std::time::Instant::now();
            let result = polisher
                .polish(
                    crate::SessionId::new(),
                    std::sync::Arc::new(context),
                    case.input.clone(),
                    std::sync::Arc::new(Sink),
                )
                .await;
            report["cases"][index][side] = match &result {
                Ok(output) => json!({
                    "text": output.text, "sourceText": output.source_text,
                    "elapsedMs": started.elapsed().as_millis(),
                }),
                Err(error) => json!({"errorCode": format!("{:?}", error.code)}),
            };
            let mut counts = crate::polish::live_acceptance::intent_counts();
            counts["state"] = json!("running");
            report["transport"] = counts;
            std::fs::write(output_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
            if result.is_err() {
                report["executionStatus"] = json!("failed");
                break 'cases;
            }
        }
    }
    if report["executionStatus"] == "running" {
        report["executionStatus"] = json!("completed");
    }
    let mut counts = crate::polish::live_acceptance::intent_counts();
    counts["state"] = json!("final");
    report["transport"] = counts;
    std::fs::write(output_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    report
}

// These are local HTTP fixtures, never provider calls. They use the exact loop,
// production assembly, translation fallback and common send boundary above.
async fn comparison_mock(fallback: bool, failure: Option<(usize, bool)>) -> (Value, usize) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (stop, mut stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let mut seen = 0;
        loop {
            let accepted = tokio::select! {
                _ = &mut stopping => break,
                accepted = listener.accept() => accepted.unwrap(),
            };
            let mut socket = accepted.0;
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = socket.read(&mut buffer).await.unwrap();
                if count == 0 { break; }
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers.lines().find_map(|line| {
                        line.to_ascii_lowercase().strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    }).unwrap();
                    if request.len() >= end + 4 + length { break; }
                }
            }
            seen += 1;
            if failure == Some((seen, true)) {
                // A request/read error that the production retry arm would retry.
                drop(socket);
                continue;
            }
            let content = if fallback {
                "fixture output"
            } else {
                "[[OPENLESS_POLISHED_SOURCE]]\n源文。\n[[OPENLESS_TRANSLATION]]\nTranslation."
            };
            let event = json!({"choices":[{"delta":{"content":content}}]});
            let body = format!("data: {event}\n\ndata: [DONE]\n\n");
            let status = if failure == Some((seen, false)) { 500 } else { 200 };
            let response = format!(
                "HTTP/1.1 {status} fixture\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
        seen
    });
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }
    let scratch = Scratch(std::env::temp_dir().join(format!("intent-mock-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&scratch.0).unwrap();
    let output = scratch.0.join("results.json");
    let config = ComparisonConfig {
        channel_id: "fixture".into(), provider_type: "deepseek".into(),
        model: crate::deepseek_suite::MODEL.into(), thinking_enabled: false, baseline: "1.3.0".into(),
        credentials: [
            ("ark.api_key".into(), "fixture-key".into()),
            ("ark.endpoint".into(), endpoint),
        ].into_iter().collect(),
    };
    let report = crate::polish::live_acceptance::intent_scope(
        run_intent_comparison(&config, output.to_str().unwrap()),
    ).await;
    stop.send(()).unwrap();
    (report, server.await.unwrap())
}

#[tokio::test]
async fn intent_comparison_http_loop_32_and_36() {
    for (fallback, expected) in [(false, 32), (true, 36)] {
        let (report, seen) = comparison_mock(fallback, None).await;
        assert_eq!(report["executionStatus"], "completed");
        assert_eq!(seen, expected);
        assert_eq!(report["transport"]["attemptedSends"], expected);
        assert_eq!(report["transport"]["fallbackSends"], if fallback { 4 } else { 0 });
        assert_eq!(report["transport"]["state"], "final");
    }
}

#[tokio::test]
async fn intent_comparison_http_loop_stops_on_first_error() {
    for (fallback, failure, expected_fallbacks) in [
        (false, (1, true), 0), (false, (1, false), 0),
        (true, (24, false), 1), (true, (24, true), 1),
    ] {
        let (report, seen) = comparison_mock(fallback, Some(failure)).await;
        assert_eq!(report["executionStatus"], "failed");
        assert_eq!(seen, failure.0);
        assert_eq!(report["transport"]["attemptedSends"], failure.0);
        assert_eq!(report["transport"]["fallbackSends"], expected_fallbacks);
    }
}

// These transport fixtures receive POST requests with Content-Length framing.
// A response requires a complete request, including across partial TCP reads.
async fn read_mock_request(socket: &mut tokio::net::TcpStream) {
    use tokio::io::AsyncReadExt;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert!(count > 0, "mock request ended before completion");
            request.extend_from_slice(&buffer[..count]);
            assert!(request.len() <= 64 * 1024, "mock request exceeds size limit");
            if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&request[..end]).unwrap();
                assert!(headers.starts_with("POST "), "mock expected a POST request");
                let length = headers.lines().find_map(|line| {
                    line.to_ascii_lowercase().strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                }).unwrap_or(0);
                let total = (end + 4).checked_add(length).unwrap();
                assert!(total <= 64 * 1024, "mock request exceeds size limit");
                if request.len() >= total { break; }
            }
        }
    }).await.expect("mock request timed out before completion");
}

#[tokio::test]
async fn intent_comparison_transport_budget_and_redirects() {
    use crate::polish::{live_acceptance as probe, send_with_transient_retry as send};
    use tokio::io::AsyncWriteExt;
    probe::intent_scope(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for _ in 0..36 {
                let (mut socket, _) = listener.accept().await.unwrap();
                read_mock_request(&mut socket).await;
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            }
            assert!(tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept()).await.is_err());
        });
        let client = probe::intent_client(&url, 1);
        for _ in 0..36 { send(client.post(&url)).await.unwrap(); }
        assert!(send(client.post(&url)).await.is_err());
        assert_eq!(probe::intent_counts()["attemptedSends"], 36);
        server.await.unwrap();
    }).await;
    probe::intent_scope(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let redirect = format!("{url}/must-not-follow");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_mock_request(&mut socket).await;
            socket.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {redirect}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            assert!(tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept()).await.is_err());
        });
        let client = probe::intent_client(&url, 1);
        assert_eq!(send(client.post(&url)).await.unwrap().status(), 307);
        assert!(send(client.post(&url)).await.is_err());
        assert_eq!(probe::intent_counts()["attemptedSends"], 1);
        server.await.unwrap();
    }).await;
}

#[tokio::test]
async fn intent_comparison_transport_connect_request_and_timeout_stop() {
    use crate::polish::{live_acceptance as probe, send_with_transient_retry as send};
    // A closed port (connect) and invalid URI (request building) cannot retry.
    for invalid_uri in [false, true] {
        probe::intent_scope(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            drop(listener);
            let client = probe::intent_client(&url, 1);
            assert!(send(client.post(if invalid_uri { "http://[" } else { &url })).await.is_err());
            assert!(send(client.post(&url)).await.is_err());
            assert_eq!(probe::intent_counts()["attemptedSends"], 1);
        }).await;
    }
    probe::intent_scope(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let client = probe::intent_client(&url, 1);
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(20), send(client.post(&url)),
        ).await.is_err());
        assert!(send(client.post(&url)).await.is_err());
        assert_eq!(probe::intent_counts()["attemptedSends"], 1);
    }).await;
}

#[test]
fn intent_comparison_legacy_limits_remain_independent() {
    let probe = crate::polish::live_acceptance::legacy_limit;
    assert_eq!(probe(false), 10);
    assert_eq!(probe(true), 1);
    assert!(!crate::polish::live_acceptance::intent_active());
}
