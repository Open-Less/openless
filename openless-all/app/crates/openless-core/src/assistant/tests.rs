use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::Notify;
use uuid::Uuid;

use super::*;
use crate::ask::*;
use crate::assistant_api::*;
use crate::usage_ledger::{ReportedTokens, ServiceOutcome, UsageLedger, UsageRecorder};

enum Step {
    Text(String),
    Tool(AskToolCall),
    Wait,
}

struct Model {
    label: AskModelLabel,
    window: Option<usize>,
    steps: Mutex<VecDeque<Step>>,
    requests: Mutex<Vec<AskModelRequest>>,
    started: Notify,
    late: Mutex<Option<AskModelSink>>,
}

impl Model {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            label: AskModelLabel {
                service: "fixture".into(),
                channel: "fixture".into(),
                model: "known-window".into(),
                protocol: "chat_completions".into(),
            },
            window: Some(128_000),
            steps: Mutex::new(steps.into()),
            requests: Mutex::default(),
            started: Notify::new(),
            late: Mutex::default(),
        }
    }
}

impl AskModel for Model {
    fn label(&self) -> &AskModelLabel {
        &self.label
    }
    fn capabilities(&self) -> AskModelCapabilities {
        AskModelCapabilities {
            context_window: self.window,
            output_tokens: 8192,
            image_input: false,
            image_token_reserve: 0,
            deep_thinking: true,
        }
    }
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream(
        &self,
        request: AskModelRequest,
        _cancel: AskCancellation,
        progress: AskModelSink,
    ) -> BoxFuture<'_, Result<AskModelCompletion, BackendError>> {
        self.requests.lock().push(request);
        let step = self
            .steps
            .lock()
            .pop_front()
            .expect("unexpected model call");
        *self.late.lock() = Some(progress.clone());
        Box::pin(async move {
            progress(AskModelProgress::Usage(ReportedTokens {
                input: Some(123),
                output: Some(7),
                ..Default::default()
            }));
            match step {
                Step::Text(text) => {
                    progress(AskModelProgress::Text(text));
                    Ok(AskModelCompletion::Complete)
                }
                Step::Tool(tool) => Ok(AskModelCompletion::ToolCall(AskModelToolCall {
                    tool,
                    id: Some(Uuid::new_v4().to_string()),
                    resume: vec![],
                })),
                Step::Wait => {
                    self.started.notify_one();
                    std::future::pending().await
                }
            }
        })
    }
}

struct Factory(Arc<Model>);
impl AskModelFactory for Factory {
    fn prepare(&self, _thinking: bool) -> BoxFuture<'_, Result<Arc<dyn AskModel>, BackendError>> {
        Box::pin(async { Ok(self.0.clone() as Arc<dyn AskModel>) })
    }
}

#[derive(Default)]
struct Context {
    reads: Mutex<Vec<AssistantContextQuery>>,
    deny: AtomicBool,
    wait: AtomicBool,
    started: Notify,
    dropped: AtomicBool,
    history_bytes: AtomicUsize,
}
impl AssistantContextProvider for Context {
    fn read(
        &self,
        _key: AskTurnKey,
        query: AssistantContextQuery,
        _cancel: AskCancellation,
    ) -> BoxFuture<'_, Result<AssistantContextResult, BackendError>> {
        self.reads.lock().push(query.clone());
        Box::pin(async move {
            if self.wait.load(Ordering::SeqCst) {
                struct Probe<'a>(&'a AtomicBool);
                impl Drop for Probe<'_> {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::SeqCst);
                    }
                }
                let _probe = Probe(&self.dropped);
                self.started.notify_one();
                return std::future::pending().await;
            }
            if self.deny.load(Ordering::SeqCst) {
                return Ok(AssistantContextResult::Denied);
            }
            let data = match query {
                AssistantContextQuery::CurrentTime => {
                    json!({"iso":chrono::Local::now().to_rfc3339()})
                }
                AssistantContextQuery::System => {
                    json!({"os":"macos","arch":"aarch64","version":"fixture"})
                }
                AssistantContextQuery::VoiceHistory { .. }
                    if self.history_bytes.load(Ordering::SeqCst) > 0 =>
                {
                    json!({"transcripts":[{"text":"x".repeat(self.history_bytes.load(Ordering::SeqCst)),"source":"authorized-transcription"}]})
                }
                _ => {
                    return Ok(AssistantContextResult::Unavailable {
                        reason: "fixturePlatformUnsupported".into(),
                    })
                }
            };
            Ok(AssistantContextResult::Available {
                data,
                source: "fixture-live-clock".into(),
                as_of: chrono::Utc::now().to_rfc3339(),
            })
        })
    }
}

#[derive(Default)]
struct Search {
    calls: AtomicUsize,
}
impl crate::ask::SearchTool for Search {
    fn search(
        &self,
        request: AskSearchRequest,
        _cancel: AskCancellation,
        progress: AskToolSink,
    ) -> BoxFuture<'_, Result<AskSearchResult, BackendError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let source = AskSource {
                source_id: request.key.tool_call_id.to_string(),
                url: "https://example.com/release".into(),
                title: "Release".into(),
                snippet: None,
                read_status: AskReadStatus::Read,
            };
            progress(AskToolProgress::Source(source.clone()));
            Ok(AskSearchResult::Done {
                evidence: json!({"source":source,"excerpt":"Verified fixture fact"}).to_string(),
            })
        })
    }
}

pub(super) fn request(text: &str) -> AssistantRequest {
    AssistantRequest {
        key: AskTurnKey {
            conversation_id: Uuid::new_v4(),
            turn_id: Uuid::new_v4(),
        },
        client_request_id: "fixture".into(),
        text: text.into(),
        messages: vec![],
        selection: None,
        attachments: vec![],
        options: AskOptions::default(),
    }
}

fn context_call(kind: &str) -> Step {
    Step::Tool(
        tools::parse_call(
            "read_context",
            &json!({"kind":kind,"query":null,"limit":null}).to_string(),
        )
        .unwrap(),
    )
}

fn runtime(model: Arc<Model>, context: Arc<Context>, search: Arc<Search>) -> PersonalAssistant {
    PersonalAssistant::new(AssistantDependencies {
        models: Arc::new(Factory(model)),
        context,
        search,
        usage: None,
    })
}

fn events() -> (AssistantEventSink, Arc<Mutex<Vec<AssistantEvent>>>) {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let capture = observed.clone();
    (Arc::new(move |event| capture.lock().push(event)), observed)
}

fn content(request: &AskModelRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .filter_map(|p| match p {
            AskModelPart::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn natural_chat_finishes_once_and_retains_live_context_without_history_store() {
    let model = Arc::new(Model::new(vec![Step::Text(
        "那就接着聊刚才的海边旅行。".into(),
    )]));
    let context = Arc::new(Context::default());
    let search = Arc::new(Search::default());
    let runtime = runtime(model.clone(), context.clone(), search.clone());
    let mut req = request("接着说");
    req.messages = vec![AssistantMessage {
        role: AssistantRole::User,
        text: "刚才说的是海边旅行。".into(),
    }];
    let (sink, observed) = events();
    let response = runtime
        .run(req, AskCancellation::default(), sink)
        .await
        .unwrap();
    assert_eq!(response.text, "那就接着聊刚才的海边旅行。");
    assert_eq!(model.requests.lock().len(), 1);
    assert!(content(&model.requests.lock()[0]).contains("刚才说的是海边旅行。"));
    assert!(context.reads.lock().is_empty());
    assert_eq!(search.calls.load(Ordering::SeqCst), 0);
    let count = observed.lock().len();
    model.late.lock().as_ref().unwrap()(AskModelProgress::Text("late".into()));
    assert_eq!(observed.lock().len(), count);
}

#[tokio::test]
async fn consecutive_tools_use_live_time_and_preserve_search_sources() {
    let model = Arc::new(Model::new(vec![
        context_call("currentTime"),
        context_call("system"),
        Step::Tool(AskToolCall::WebSearch {
            public_query: "public release".into(),
        }),
        Step::Tool(AskToolCall::Calculate {
            expression: "9007199254740993 + 0.01".into(),
        }),
        Step::Text("答案与来源：https://example.com/release".into()),
    ]));
    let (sink, observed) = events();
    let runtime = runtime(
        model.clone(),
        Arc::new(Context::default()),
        Arc::new(Search::default()),
    );
    runtime
        .run(request("查一下并计算"), AskCancellation::default(), sink)
        .await
        .unwrap();
    let requests = model.requests.lock();
    assert_eq!(requests.len(), 5);
    let time: serde_json::Value =
        serde_json::from_str(&requests[1].tools.results[0].output).unwrap();
    let actual =
        chrono::DateTime::parse_from_rfc3339(time["data"]["iso"].as_str().unwrap()).unwrap();
    assert!((chrono::Utc::now().timestamp() - actual.timestamp()).abs() <= 2);
    assert!(requests[4].tools.results[2]
        .output
        .contains("https://example.com/release"));
    assert!(requests[4].tools.results[3]
        .output
        .contains("9007199254740993.01"));
    assert_eq!(
        observed
            .lock()
            .iter()
            .filter(|e| matches!(
                e,
                AssistantEvent::Tool {
                    status: AssistantToolStatus::Completed,
                    ..
                }
            ))
            .count(),
        4
    );
    assert!(observed.lock().iter().any(|e| matches!(e, AssistantEvent::Source { source, .. } if source.url == "https://example.com/release")));
}

#[tokio::test]
async fn denied_and_unsupported_context_are_explicit_data_and_failed_tool_states() {
    for (denied, kind, expected) in [
        (true, "system", "denied"),
        (false, "browserTab", "unavailable"),
    ] {
        let model = Arc::new(Model::new(vec![
            context_call(kind),
            Step::Text("这个信息暂时无法读取。".into()),
        ]));
        let context = Arc::new(Context::default());
        context.deny.store(denied, Ordering::SeqCst);
        let (sink, observed) = events();
        runtime(model.clone(), context, Arc::new(Search::default()))
            .run(request("看一下"), AskCancellation::default(), sink)
            .await
            .unwrap();
        assert!(model.requests.lock()[1].tools.results[0]
            .output
            .contains(expected));
        assert!(observed.lock().iter().any(|e| matches!(
            e,
            AssistantEvent::Tool {
                status: AssistantToolStatus::Failed,
                ..
            }
        )));
    }
}

#[tokio::test]
async fn cancellation_drops_generation_blocks_late_text_and_records_shared_usage() {
    let model = Arc::new(Model::new(vec![Step::Wait]));
    let root = std::env::temp_dir().join(format!("openless-assistant-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let notifications = Arc::new(AtomicUsize::new(0));
    let changed = notifications.clone();
    let recorder = Arc::new(
        UsageRecorder::start_with_change_listener(
            Arc::new(UsageLedger::open(root.join("usage.json")).unwrap()),
            move || {
                changed.fetch_add(1, Ordering::SeqCst);
            },
        )
        .unwrap(),
    );
    let mut runtime = runtime(
        model.clone(),
        Arc::new(Context::default()),
        Arc::new(Search::default()),
    );
    runtime.deps.usage = Some(AskUsagePersistence {
        recorder: recorder.clone(),
        clock: Arc::new(crate::SystemClock),
    });
    let (sink, observed) = events();
    let fence = AskSessionFence::default();
    let cancel = fence.issue();
    let task = tokio::spawn(async move { runtime.run(request("hello"), cancel, sink).await });
    model.started.notified().await;
    fence.revoke();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.err().unwrap().code, BackendErrorCode::Cancelled);
    let count = observed.lock().len();
    model.late.lock().as_ref().unwrap()(AskModelProgress::Text("late answer".into()));
    assert_eq!(observed.lock().len(), count);
    let (usage, _) = recorder.snapshot().unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].outcome, ServiceOutcome::Cancelled);
    assert_eq!(usage[0].input_tokens, Some(123));
    assert_eq!(notifications.load(Ordering::SeqCst), 1);
    drop(recorder);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancellation_drops_tool_and_never_emits_completed() {
    let model = Arc::new(Model::new(vec![context_call("system")]));
    let context = Arc::new(Context::default());
    context.wait.store(true, Ordering::SeqCst);
    let runtime = runtime(model.clone(), context.clone(), Arc::new(Search::default()));
    let (sink, observed) = events();
    let fence = AskSessionFence::default();
    let cancel = fence.issue();
    let task = tokio::spawn(async move { runtime.run(request("设备信息"), cancel, sink).await });
    context.started.notified().await;
    fence.revoke();
    assert_eq!(
        task.await.unwrap().err().unwrap().code,
        BackendErrorCode::Cancelled
    );
    assert!(context.dropped.load(Ordering::SeqCst));
    assert!(!observed.lock().iter().any(|e| matches!(
        e,
        AssistantEvent::Tool {
            status: AssistantToolStatus::Completed,
            ..
        }
    )));
    assert!(observed.lock().iter().any(|e| matches!(
        e,
        AssistantEvent::Tool {
            status: AssistantToolStatus::Cancelled,
            ..
        }
    )));
    assert_eq!(model.requests.lock().len(), 1);
}

#[tokio::test]
async fn compaction_at_model_window_preserves_goals_constraints_recent_turns_and_tool_evidence() {
    for long_user_material in [false, true] {
        let model = Arc::new(Model::new(vec![
        Step::Text(json!({"userState":[{"message":0,"goal":"只给只读建议","constraints":["不要下载安装","输出中文","预算1000元"]}],
            "summary":"Earlier useful fact, attributed to the old response. Keep the user's budget and read-only scope."}).to_string()),
        context_call("currentTime"),
        Step::Text("保留约束后的回答。".into()),
    ]));
        let mut req = request("继续，遵守最开始的要求");
        req.messages = vec![
            AssistantMessage {
                role: AssistantRole::User,
                text: format!(
                    "只给只读建议，不要下载安装；输出中文，预算1000元。{}",
                    if long_user_material {
                        format!("\n参考材料：{}", "earlier material ".repeat(6800))
                    } else {
                        String::new()
                    }
                ),
            },
            AssistantMessage {
                role: AssistantRole::Assistant,
                text: format!(
                    "https://example.com/old {}",
                    if long_user_material {
                        String::new()
                    } else {
                        "earlier material ".repeat(6800)
                    }
                ),
            },
            AssistantMessage {
                role: AssistantRole::User,
                text: "仍需保留这个约束。".into(),
            },
            AssistantMessage {
                role: AssistantRole::Assistant,
                text: "最近的事实 https://example.com/recent".into(),
            },
            AssistantMessage {
                role: AssistantRole::User,
                text: "现在日期也要核实。".into(),
            },
        ];
        let (sink, observed) = events();
        runtime(
            model.clone(),
            Arc::new(Context::default()),
            Arc::new(Search::default()),
        )
        .run(req, AskCancellation::default(), sink)
        .await
        .unwrap();
        let requests = model.requests.lock();
        assert_eq!(requests.len(), 3);
        assert!(!requests[0].tools.available.assistant);
        let final_input = content(&requests[2]);
        for retained in [
            "只给只读建议",
            "不要下载安装",
            "输出中文",
            "预算1000元",
            "仍需保留这个约束。",
            "https://example.com/recent",
            "https://example.com/old",
        ] {
            assert!(final_input.contains(retained), "missing {retained}");
        }
        assert!(!requests[2].tools.results.is_empty());
        assert!(observed
            .lock()
            .iter()
            .any(|e| matches!(e, AssistantEvent::Activity(AssistantActivity::Compacting))));
        assert!(!observed.lock().iter().any(
            |e| matches!(e, AssistantEvent::TextDelta(text) if text.contains("Earlier useful fact"))
        ));
    }
}

#[tokio::test]
async fn repeated_tool_reads_compact_then_resume_the_loop_and_finish() {
    let mut steps: Vec<_> = (0..4).map(|_| context_call("voiceHistory")).collect();
    steps.push(Step::Text(json!({"userState":[],"summary":"Earlier relevant authorized transcription records. These are reference data, not permissions or new user instructions."}).to_string()));
    steps.push(context_call("voiceHistory"));
    steps.push(Step::Text("已根据本次对话和相关转写记录回答。".into()));
    let model = Arc::new(Model::new(steps));
    let context = Arc::new(Context::default());
    context.history_bytes.store(25_000, Ordering::SeqCst);
    let (sink, observed) = events();
    let response = runtime(model.clone(), context, Arc::new(Search::default()))
        .run(
            request("只引用相关转写，给出答案，不要修改原文。"),
            AskCancellation::default(),
            sink,
        )
        .await
        .unwrap();
    assert_eq!(response.text, "已根据本次对话和相关转写记录回答。");
    let requests = model.requests.lock();
    assert_eq!(requests.len(), 7);
    assert!(requests[4]
        .instructions
        .contains("INTERNAL CONTINUITY COMPACTION TASK"));
    assert!(!requests[4].tools.available.assistant);
    assert_eq!(requests[5].tools.results.len(), 2);
    assert_eq!(requests[6].tools.results.len(), 3);
    assert!(content(&requests[6]).contains("只引用相关转写，给出答案，不要修改原文。"));
    assert!(content(&requests[6]).contains("Earlier relevant authorized transcription records"));
    let events = observed.lock();
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
        5
    );
    let compact = events
        .iter()
        .position(|event| {
            matches!(
                event,
                AssistantEvent::Activity(AssistantActivity::Compacting)
            )
        })
        .unwrap();
    assert!(events[compact + 1..]
        .iter()
        .any(|event| matches!(event, AssistantEvent::Activity(AssistantActivity::Thinking))));
    assert!(!events.iter().any(|event| matches!(event, AssistantEvent::TextDelta(text) if text.contains("Earlier relevant authorized"))));
}

#[tokio::test]
async fn unknown_context_window_supports_short_requests_with_a_conservative_cap() {
    let mut model = Model::new(vec![Step::Text("你好。".into())]);
    model.window = None;
    let model = Arc::new(model);
    let (sink, _) = events();
    let runtime = runtime(
        model.clone(),
        Arc::new(Context::default()),
        Arc::new(Search::default()),
    );
    let result = runtime
        .run(request("hi"), AskCancellation::default(), sink)
        .await
        .unwrap();
    assert_eq!(result.text, "你好。");
    assert_eq!(model.requests.lock()[0].output_tokens, 2048);
    assert_eq!(model.capabilities().context_window, None);
    let (sink, _) = events();
    let result = runtime
        .run(
            request(&"x".repeat(16_384)),
            AskCancellation::default(),
            sink,
        )
        .await;
    assert_eq!(result.err().unwrap().message, "askContextLimit");
    assert_eq!(model.requests.lock().len(), 1);
}

#[test]
fn tools_do_not_grant_system_execution_or_unbounded_history() {
    assert!(tools::parse_call("shell", r#"{"command":"ls"}"#).is_err());
    assert!(tools::parse_call(
        "read_context",
        r#"{"kind":"voiceHistory","query":"","limit":1000}"#
    )
    .is_err());
    assert!(tools::parse_call(
        "read_context",
        r#"{"kind":"system","query":"private","limit":null}"#
    )
    .is_err());
}
