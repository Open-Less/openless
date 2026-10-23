use super::*;
use crate::control_tests::{control, open_at, result};
use crate::credentials::MemoryVault;
use crate::tests::{next, reply, send};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

fn opened(developer: bool) -> (u64, u64, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let (h, c) = open_at(dir.path(), developer, Arc::new(MemoryVault::default()));
    (h, c, dir)
}
fn configure(h: u64, c: u64, endpoint: &str) -> String {
    let channel = result(control(h, c, json!({"action":"saveChannel","kind":"llm","providerType":"custom",
        "name":"Fixture","endpoint":endpoint,"model":"original-model","apiKey":"fixture-key",
        "credentialFields":{"ark.temperature":"0.2","ark.extra_headers":"{\"X-Fixture\":\"original\"}"}})))["channelID"].as_str().unwrap().to_string();
    result(control(
        h,
        c,
        json!({"action":"setActive","kind":"llm","channelID":channel}),
    ));
    result(control(
        h,
        c,
        json!({"action":"activateStyle","id":"builtin.light"}),
    ));
    channel
}
fn start(h: u64, c: u64, intent: Option<Value>) -> (Uuid, String) {
    start_with_insert(h, c, intent, true)
}
fn start_with_insert(h: u64, c: u64, intent: Option<Value>, insert: bool) -> (Uuid, String) {
    let client = Uuid::new_v4();
    let mut command = json!({"operation":"start","clientSessionID":client,"insert":insert});
    if let Some(intent) = intent {
        command["intent"] = intent;
    }
    assert_eq!(send(h, command), 0);
    loop {
        let event = next(c);
        match event["kind"].as_str() {
            Some("nativeRequest") => {
                assert_ne!(event["operation"], "insert");
                reply(h, &event, Value::Null);
            }
            Some("started") => {
                feed(h, event["sessionID"].as_str().unwrap());
                return (client, event["sessionID"].as_str().unwrap().into());
            }
            Some("failed") | Some("commandError") => panic!("{event}"),
            _ => {}
        }
    }
}
fn feed(h: u64, session: &str) {
    let session = CString::new(session).unwrap();
    let pcm = vec![16u8; 9600];
    assert_eq!(
        unsafe { ol_ios_feed_pcm(h, session.as_ptr(), pcm.as_ptr(), pcm.len()) },
        0
    );
}
fn stop(h: u64, c: u64, client: Uuid, instruction: &str) -> Value {
    assert_eq!(
        send(h, json!({"operation":"stop","clientSessionID":client})),
        0
    );
    finish_native(h, c, instruction)
}
fn finish_native(h: u64, c: u64, instruction: &str) -> Value {
    loop {
        let event = next(c);
        match event["kind"].as_str() {
            Some("nativeRequest") => {
                assert_ne!(
                    event["operation"], "insert",
                    "preview must never request insertion"
                );
                reply(
                    h,
                    &event,
                    if event["operation"] == "speechFinish" {
                        json!({"text":instruction,"durationMs":300})
                    } else {
                        Value::Null
                    },
                );
            }
            Some("completed") | Some("failed") => return event,
            _ => {}
        }
    }
}
/// Loopback-only provider fixture; never connects to a real service. The reply
/// can be held until after cancellation to exercise a late provider response.
fn provider_fixture(
    output: &str,
) -> (
    String,
    mpsc::Receiver<(String, Value)>,
    mpsc::Sender<()>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (request_tx, request_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let output = output.to_string();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut bytes = Vec::new();
        let (headers, body) = loop {
            let mut chunk = [0; 4096];
            let count = socket.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break (
                        headers,
                        serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap(),
                    );
                }
            }
        };
        request_tx.send((headers, body)).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let frame = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"content":output}}]})
        );
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", frame.len(), frame);
        let _ = socket.write_all(response.as_bytes());
    });
    (endpoint, request_rx, release_tx, thread)
}
#[test]
fn voice_intent_configured_off_and_invalid_requests_reject_before_native_capture() {
    let (h, c, _dir) = opened(true);
    for intent in [
        json!({"kind":"helpWrite"}),
        json!({"kind":"translation","targetLanguage":"English"}),
        json!({"kind":"helpWrite","stylePackID":"builtin.light"}),
        json!({"kind":"dictation","targetLanguage":"English"}),
        json!({"kind":"dictation","stylePackID":"missing"}),
    ] {
        assert_eq!(
            send(
                h,
                json!({"operation":"start","clientSessionID":Uuid::new_v4(),"intent":intent})
            ),
            0
        );
        assert_eq!(next(c)["kind"], "failed");
        assert!(host(h).unwrap().admission.lock().unwrap().is_none());
        assert!(host(h).unwrap().broker.captures.lock().unwrap().is_empty());
    }
    ol_ios_destroy(h);
    let (h, c, _dir) = opened(false);
    for intent in [
        json!({"kind":"helpWrite"}),
        json!({"kind":"translation","targetLanguage":"English"}),
        json!({"kind":"dictation","stylePackID":"builtin.light"}),
    ] {
        assert_eq!(
            send(
                h,
                json!({"operation":"start","clientSessionID":Uuid::new_v4(),"intent":intent})
            ),
            0
        );
        assert_eq!(next(c)["code"], "PermissionDenied");
    }
    ol_ios_destroy(h);
}
#[test]
fn translation_from_raw_uses_any_effective_target_and_empty_output_never_inserts_source() {
    for output in ["[[OPENLESS_POLISHED_SOURCE]]\nsource fixture\n[[OPENLESS_TRANSLATION]]\ntranslated fixture", "", "malformed translation"] {
        let (endpoint, request, release, server) = provider_fixture(output);
        let (h,c,_dir) = opened(true);
        configure(h,c,&endpoint);
        result(control(h,c,json!({"action":"activateStyle","id":"builtin.raw"})));
        result(control(h,c,json!({"action":"setTranslationTargets","targets":["English","日本語"]})));
        let owner = host(h).unwrap();
        let mut prefs = owner.backend.get_preferences();
        prefs.working_languages = vec!["English".into()];
        owner.backend.update_settings(prefs, openless_core::SettingsUpdateOptions::STRICT, &openless_core::NoopSettingsRuntime).unwrap();
        let caps = result(control(h,c,json!({"action":"capabilitySnapshot"})));
        assert_eq!(caps["llm"]["canAttemptPolish"], false);
        assert_eq!(caps["llm"]["canAttemptTranslation"], true);
        assert_eq!(caps["translation"]["effectiveTargetLanguages"], json!(["日本語"]));
        let (client,_) = start_with_insert(h,c,Some(json!({"kind":"translation","targetLanguage":"日本語","stylePackID":"builtin.raw"})),false);
        release.send(()).unwrap();
        let event = stop(h,c,client,"source fixture");
        let _ = request.recv_timeout(Duration::from_secs(5)).unwrap();
        if !output.contains("[[OPENLESS_TRANSLATION]]") {
            assert_eq!(event["kind"], "failed");
            assert_ne!(event["kind"], "completed");
        } else {
            assert_eq!(event["kind"], "completed", "{event}");
            assert_eq!(event["result"]["polishedText"], "translated fixture");
            assert_eq!(event["result"]["inserted"], "notRequested");
        }
        assert_eq!(owner.backend.get_preferences().active_style_pack_id, "builtin.raw");
        ol_ios_destroy(h); server.join().unwrap();
    }
}

#[test]
fn keyboard_explicit_raw_never_uses_globally_selected_llm_style() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (h,c,_dir) = opened(true);
    configure(h,c,&endpoint);
    assert_eq!(host(h).unwrap().backend.get_preferences().active_style_pack_id,"builtin.light");
    let (client,_) = start_with_insert(h,c,Some(json!({"kind":"dictation","stylePackID":"builtin.raw"})),false);
    let event = stop(h,c,client,"untouched source");
    assert_eq!(event["kind"],"completed","{event}");
    assert_eq!(event["result"]["polishedText"],"untouched source");
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    assert_eq!(host(h).unwrap().backend.get_preferences().active_style_pack_id,"builtin.light");
    ol_ios_destroy(h);
}

#[test]
fn voice_intent_helpwrite_uses_spoken_instruction_frozen_request_and_preview_only() {
    let (endpoint, request, release, server) = provider_fixture("Generated draft");
    let (h, c, _dir) = opened(true);
    let channel = configure(h, c, &endpoint);
    assert_eq!(
        result(control(h, c, json!({"action":"capabilitySnapshot"})))["llm"]["canAttemptHelpWrite"],
        true
    );
    let (client, session) = start(h, c, Some(json!({"kind":"helpWrite"})));
    let owner = host(h).unwrap();
    // Bypass Host management deliberately: even direct Core/vault mutations
    // must not change the prepared invocation or chosen source/style.
    let mut prefs = owner.backend.get_preferences();
    prefs.active_style_pack_id = "builtin.formal".into();
    owner
        .backend
        .update_settings(
            prefs,
            openless_core::SettingsUpdateOptions::STRICT,
            &openless_core::NoopSettingsRuntime,
        )
        .unwrap();
    for (account, value) in [
        ("ark.endpoint", "http://127.0.0.1:1/changed"),
        ("ark.api_key", "changed-key"),
        ("ark.model_id", "changed-model"),
        ("ark.temperature", "1.5"),
        ("ark.request_format", "messages"),
        ("ark.extra_headers", "{\"X-Fixture\":\"changed\"}"),
    ] {
        owner
            .executor
            .block_on(
                owner.backend.set_credential(
                    openless_core::credentials::CredentialKey::new(
                        openless_core::credentials::CredentialNamespace::Llm,
                        Some(channel.clone()),
                        account,
                    )
                    .unwrap(),
                    openless_core::credentials::SecretValue::new(value),
                ),
            )
            .unwrap();
    }
    assert_eq!(
        send(h, json!({"operation":"stop","clientSessionID":client})),
        0
    );
    // Answer only native operations; provider waits for the explicit fixture release.
    loop {
        let event = next(c);
        if event["kind"] == "level" {
            continue;
        }
        assert_eq!(event["kind"], "nativeRequest", "{event}");
        assert_ne!(event["operation"], "insert");
        reply(
            h,
            &event,
            if event["operation"] == "speechFinish" {
                json!({"text":"Write a kind invitation","durationMs":300})
            } else {
                Value::Null
            },
        );
        if event["operation"] == "speechFinish" {
            break;
        }
    }
    let (headers, body) = request.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(headers.starts_with("POST /v1/chat/completions "));
    assert!(headers.contains("fixture-key"));
    assert!(headers.to_lowercase().contains("x-fixture: original"));
    assert!(!headers.contains("changed"));
    assert_eq!(body["model"], "original-model");
    assert!((body["temperature"].as_f64().unwrap() - 0.2).abs() < 0.001);
    let prompt = body["messages"].to_string();
    assert!(prompt.contains("Write a kind invitation"));
    assert!(prompt.contains("帮我写"));
    assert!(!prompt.contains("Generated draft"));
    release.send(()).unwrap();
    let completed = finish_native(h, c, "unused");
    assert_eq!(completed["kind"], "completed", "{completed}");
    assert_eq!(completed["outputDisposition"], "preview");
    assert_eq!(completed["intent"]["kind"], "helpWrite");
    assert_eq!(completed["preview"]["sessionID"], session);
    assert_eq!(completed["preview"]["text"], "Generated draft");
    assert_eq!(completed["preview"]["promptKind"], "voice_compose");
    assert_eq!(completed["preview"]["model"], "original-model");
    assert_eq!(completed["result"]["rawText"], "Write a kind invitation");
    assert_eq!(
        completed["result"]["polishedText"],
        "Write a kind invitation"
    );
    assert_eq!(completed["result"]["inserted"], "notRequested");
    let history = owner.backend.list_history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].raw_transcript, "Write a kind invitation");
    assert_ne!(history[0].final_text, "Generated draft");
    assert!(!completed.to_string().contains("fixture-key"));
    server.join().unwrap();
    ol_ios_destroy(h);
}
#[test]
fn voice_intent_translation_freezes_configured_target_and_style() {
    let (endpoint, request, release, server) = provider_fixture(
        "[[OPENLESS_POLISHED_SOURCE]]\nsource\n[[OPENLESS_TRANSLATION]]\ntranslated",
    );
    let (h, c, _dir) = opened(true);
    configure(h, c, &endpoint);
    result(control(
        h,
        c,
        json!({"action":"setTranslationTargets","targets":["English","日本語"]}),
    ));
    let (client, _) = start(
        h,
        c,
        Some(
            json!({"kind":"translation","targetLanguage":"English","stylePackID":"builtin.light"}),
        ),
    );
    let owner = host(h).unwrap();
    let expected_style = owner
        .backend
        .get_style_pack("builtin.light")
        .unwrap()
        .prompt;
    let mut prefs = owner.backend.get_preferences();
    prefs.translation_target_language = "日本語".into();
    prefs.active_style_pack_id = "builtin.formal".into();
    owner
        .backend
        .update_settings(
            prefs,
            openless_core::SettingsUpdateOptions::STRICT,
            &openless_core::NoopSettingsRuntime,
        )
        .unwrap();
    // This request opted into insertion, so acknowledge the normal final receipt.
    assert_eq!(
        send(h, json!({"operation":"stop","clientSessionID":client})),
        0
    );
    loop {
        let event = next(c);
        if event["kind"] == "level" {
            continue;
        }
        assert_eq!(event["kind"], "nativeRequest", "{event}");
        reply(
            h,
            &event,
            if event["operation"] == "speechFinish" {
                json!({"text":"source","durationMs":300})
            } else {
                Value::Null
            },
        );
        if event["operation"] == "speechFinish" {
            break;
        }
    }
    let (_, body) = request.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(body["messages"][0]["content"].as_str().unwrap().contains(
        expected_style
            .lines()
            .find(|line| line.starts_with("你是"))
            .unwrap()
    ));
    assert!(body["messages"].to_string().contains("English"));
    assert!(!body["messages"].to_string().contains("日本語"));
    release.send(()).unwrap();
    loop {
        let event = next(c);
        if event["kind"] == "nativeRequest" {
            assert_eq!(event["operation"], "insert");
            assert_eq!(event["payload"]["text"], "translated");
            reply(h, &event, json!({"status":"verified"}));
        } else if event["kind"] == "completed" {
            assert_eq!(event["result"]["polishedText"], "translated");
            break;
        } else if event["kind"] == "failed" {
            panic!("{event}");
        }
    }
    server.join().unwrap();
    ol_ios_destroy(h);
}
#[test]
fn voice_intent_cancel_pending_llm_suppresses_late_preview_and_releases_admission() {
    let (endpoint, request, release, server) = provider_fixture("Must not preview");
    let (h, c, _dir) = opened(true);
    configure(h, c, &endpoint);
    let (client, _) = start(h, c, Some(json!({"kind":"helpWrite"})));
    assert_eq!(
        send(h, json!({"operation":"stop","clientSessionID":client})),
        0
    );
    loop {
        let event = next(c);
        if event["kind"] == "level" {
            continue;
        }
        assert_eq!(event["kind"], "nativeRequest", "{event}");
        reply(
            h,
            &event,
            if event["operation"] == "speechFinish" {
                json!({"text":"Instruction","durationMs":100})
            } else {
                Value::Null
            },
        );
        if event["operation"] == "speechFinish" {
            break;
        }
    }
    request.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        send(
            h,
            json!({"operation":"start","clientSessionID":Uuid::new_v4()})
        ),
        -2
    );
    loop {
        let event = next(c);
        if event["kind"] == "preview" {
            continue;
        }
        assert_eq!(event["kind"], "commandError");
        break;
    }
    let denied = control(
        h,
        c,
        json!({"action":"activateStyle","id":"builtin.formal"}),
    );
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["message"], "recording or processing is active");
    assert_eq!(
        send(h, json!({"operation":"cancel","clientSessionID":client})),
        0
    );
    loop {
        let event = next(c);
        assert_ne!(event["kind"], "completed");
        if event["kind"] == "cancelled" {
            break;
        }
        if event["kind"] == "nativeRequest" {
            reply(h, &event, Value::Null);
        }
    }
    release.send(()).unwrap();
    server.join().unwrap();
    for _ in 0..200 {
        if host(h).unwrap().admission.lock().unwrap().is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(host(h).unwrap().admission.lock().unwrap().is_none());
    // A subsequent valid request creates an event fence: no late completed event.
    assert_eq!(send(h, json!({"operation":"history"})), 0);
    loop {
        let event = next(c);
        assert_ne!(event["kind"], "completed");
        if event["kind"] == "history" {
            break;
        }
    }
    ol_ios_destroy(h);
}
#[test]
fn voice_intent_old_start_and_ordinary_custom_raw_never_invoke_llm() {
    let (h, c, _dir) = opened(false);
    let owner = host(h).unwrap();
    let mut pack = owner.backend.get_style_pack("builtin.raw").unwrap();
    pack.prompt = "Custom raw would invoke an LLM".into();
    owner.backend.update_style_pack(pack).unwrap();
    for intent in [
        None,
        Some(json!({"kind":"dictation","stylePackID":"builtin.raw"})),
    ] {
        let client = Uuid::new_v4();
        let mut command = json!({"operation":"start","clientSessionID":client,"insert":false});
        if let Some(intent) = intent {
            command["intent"] = intent;
        }
        assert_eq!(send(h, command), 0);
        loop {
            let event = next(c);
            if event["kind"] == "nativeRequest" {
                reply(h, &event, Value::Null);
            } else if event["kind"] == "started" {
                feed(h, event["sessionID"].as_str().unwrap());
                break;
            } else if event["kind"] == "failed" {
                panic!("{event}");
            }
        }
        let completed = stop(h, c, client, "Real raw transcript");
        assert_eq!(completed["kind"], "completed", "{completed}");
        assert_eq!(completed["result"]["rawText"], "Real raw transcript");
        assert_eq!(completed["result"]["polishedText"], "Real raw transcript");
    }
    assert_eq!(owner.backend.list_history().unwrap().len(), 2);
    assert_eq!(
        owner.backend.get_style_pack("builtin.raw").unwrap().prompt,
        "Custom raw would invoke an LLM"
    );
    ol_ios_destroy(h);
}

#[test]
fn voice_intent_cancel_pending_asr_rejects_late_transcript_and_never_composes() {
    let (h, c, _dir) = opened(true);
    // This endpoint is never contacted: cancellation occurs before composition.
    configure(h, c, "http://127.0.0.1:1/v1");
    let (client, _) = start(h, c, Some(json!({"kind":"helpWrite"})));
    assert_eq!(
        send(h, json!({"operation":"stop","clientSessionID":client})),
        0
    );
    let pending = loop {
        let event = next(c);
        if event["kind"] == "level" {
            continue;
        }
        assert_eq!(event["kind"], "nativeRequest", "{event}");
        if event["operation"] == "speechFinish" {
            break event;
        }
        reply(h, &event, Value::Null);
    };
    assert_eq!(
        send(h, json!({"operation":"cancel","clientSessionID":client})),
        0
    );
    loop {
        let event = next(c);
        assert_ne!(event["kind"], "completed");
        if event["kind"] == "nativeRequest" {
            reply(h, &event, Value::Null);
        }
        if event["kind"] == "cancelled" {
            break;
        }
    }
    let bytes = serde_json::to_vec(&json!({"requestID":pending["requestID"],"ok":true,
        "value":{"text":"Late instruction","durationMs":100}}))
    .unwrap();
    assert_eq!(unsafe { ol_ios_reply(h, bytes.as_ptr(), bytes.len()) }, 1);
    for _ in 0..200 {
        if host(h).unwrap().admission.lock().unwrap().is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(host(h).unwrap().admission.lock().unwrap().is_none());
    assert_eq!(send(h, json!({"operation":"history"})), 0);
    loop {
        let event = next(c);
        assert_ne!(event["kind"], "completed");
        if event["kind"] == "nativeRequest" {
            reply(h, &event, Value::Null);
        }
        if event["kind"] == "history" {
            break;
        }
    }
    ol_ios_destroy(h);
}

#[test]
fn voice_intent_unregistered_local_adapter_fails_without_cloud_or_native_fallback() {
    let (h, _c, _dir) = opened(true);
    let owner = host(h).unwrap();
    let err = owner
        .executor
        .block_on(
            owner
                .backend
                .start_local_raw_dictation("unregistered-local", false),
        )
        .unwrap_err();
    assert_eq!(err.code, BackendErrorCode::Unsupported);
    assert!(owner.broker.captures.lock().unwrap().is_empty());
    assert!(owner.broker.partials.lock().unwrap().is_empty());
    ol_ios_destroy(h);
}

#[test]
fn voice_intent_destroy_pending_compose_closes_callbacks_and_cancels_operation() {
    let (endpoint, request, release, server) = provider_fixture("Must not survive close");
    let (h, c, _dir) = opened(true);
    configure(h, c, &endpoint);
    let (client, _) = start(h, c, Some(json!({"kind":"helpWrite"})));
    assert_eq!(
        send(h, json!({"operation":"stop","clientSessionID":client})),
        0
    );
    loop {
        let event = next(c);
        if event["kind"] == "level" {
            continue;
        }
        assert_eq!(event["kind"], "nativeRequest", "{event}");
        reply(
            h,
            &event,
            if event["operation"] == "speechFinish" {
                json!({"text":"Captured instruction","durationMs":100})
            } else {
                Value::Null
            },
        );
        if event["operation"] == "speechFinish" {
            break;
        }
    }
    request.recv_timeout(Duration::from_secs(10)).unwrap();
    ol_ios_destroy(h);
    release.send(()).unwrap();
    server.join().unwrap();
    assert!(host(h).is_none());
    assert_eq!(send(h, json!({"operation":"history"})), -1);
}
