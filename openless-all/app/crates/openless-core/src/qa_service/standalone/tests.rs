use super::*;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

struct TestModel {
    label: AskModelLabel,
    caps: AskModelCapabilities,
    calls: mpsc::UnboundedSender<Call>,
}

impl AskModel for TestModel {
    fn label(&self) -> &AskModelLabel {
        &self.label
    }

    fn capabilities(&self) -> AskModelCapabilities {
        self.caps
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn stream(
        &self,
        request: AskModelRequest,
        _cancellation: AskCancellation,
        progress: AskModelSink,
    ) -> BoxFuture<'_, Result<AskModelCompletion, BackendError>> {
        Box::pin(async move {
            let (finish, receiver) = oneshot::channel();
            assert!(self
                .calls
                .send(Call {
                    request,
                    progress,
                    finish
                })
                .is_ok());
            receiver.await.expect("test response supplied")
        })
    }
}

struct Call {
    request: AskModelRequest,
    progress: AskModelSink,
    finish: oneshot::Sender<Result<AskModelCompletion, BackendError>>,
}

impl Call {
    fn delta(&self, text: &str) {
        (self.progress)(AskModelProgress::Text(text.to_owned()));
    }

    fn complete(self, text: &str, input: Option<u64>, cached: Option<u64>) {
        self.delta(text);
        (self.progress)(AskModelProgress::Usage(ReportedTokens {
            input_accounting: Some(CacheInputAccounting::Inclusive),
            input,
            cached_input: cached,
            output: Some(5),
            ..Default::default()
        }));
        assert!(self.finish.send(Ok(AskModelCompletion::Complete)).is_ok());
    }
}

struct Factory(Arc<TestModel>);

impl AskModelFactory for Factory {
    fn prepare(&self, _: bool) -> BoxFuture<'_, Result<Arc<dyn AskModel>, BackendError>> {
        Box::pin(async move { Ok(self.0.clone() as Arc<dyn AskModel>) })
    }
}

#[derive(Default)]
struct Context(Mutex<Option<String>>);

impl ContextProvider for Context {
    fn context(
        &self,
        _: AskTurnKey,
        _: AskCancellation,
    ) -> BoxFuture<'_, Result<AskContext, BackendError>> {
        Box::pin(async move {
            let facts = self
                .0
                .lock()
                .iter()
                .map(|text| AskContextFact {
                    name: "app".into(),
                    value: serde_json::json!(text),
                    source: "local".into(),
                    as_of: "2026-11-20T09:00:00-07:00".into(),
                })
                .collect();
            Ok(AskContext {
                static_version: "ask-test-v1".into(),
                static_instructions: "Test instructions".into(),
                facts,
                missing: Vec::new(),
            })
        })
    }
}

#[derive(Default)]
struct Search(Mutex<Vec<String>>, std::sync::atomic::AtomicBool);

impl SearchTool for Search {
    fn enabled(&self) -> BoxFuture<'_, bool> {
        Box::pin(async { self.1.load(std::sync::atomic::Ordering::Relaxed) })
    }

    fn search(
        &self,
        request: AskSearchRequest,
        _: AskCancellation,
        progress: AskToolSink,
    ) -> BoxFuture<'_, Result<AskSearchResult, BackendError>> {
        Box::pin(async move {
            self.0.lock().push(request.public_query);
            progress(AskToolProgress::Status(AskSearchStatus::Searching));
            let mut source = AskSource {
                source_id: "source-a".into(),
                url: "https://example.com/result".into(),
                title: "Example".into(),
                snippet: None,
                read_status: AskReadStatus::Discovered,
            };
            progress(AskToolProgress::Source(source.clone()));
            progress(AskToolProgress::Status(AskSearchStatus::Reading));
            source.read_status = AskReadStatus::Read;
            progress(AskToolProgress::Source(source));
            Ok(AskSearchResult::Done {
                evidence: "private-to-model source body".into(),
            })
        })
    }
}

#[derive(Default)]
struct Attachments(Mutex<HashMap<Uuid, (Uuid, AskImage)>>);

impl AttachmentReader for Attachments {
    fn acquire(&self, turn: AskTurnKey, ids: &[Uuid]) -> Result<Vec<AskAttachment>, BackendError> {
        let entries = self.0.lock();
        ids.iter()
            .map(|id| match entries.get(id) {
                Some((owner, image)) if *owner == turn.conversation_id => {
                    Ok(AskAttachment::Image(image.clone()))
                }
                _ => Err(ask_error(
                    BackendErrorCode::PermissionDenied,
                    "testAttachmentScope",
                )),
            })
            .collect()
    }

    fn end_conversation(&self, id: Uuid) {
        self.0.lock().retain(|_, (owner, _)| *owner != id);
    }
}

struct Fixture {
    service: StandaloneQaService,
    calls: mpsc::UnboundedReceiver<Call>,
    context: Arc<Context>,
    search: Arc<Search>,
    attachments: Arc<Attachments>,
}

impl Fixture {
    fn new(window: usize) -> Self {
        let (sender, calls) = mpsc::unbounded_channel();
        let context = Arc::new(Context::default());
        let search = Arc::new(Search::default());
        let attachments = Arc::new(Attachments::default());
        let service = StandaloneQaService::new(StandaloneQaDependencies {
            models: Arc::new(Factory(Arc::new(TestModel {
                label: AskModelLabel {
                    service: "test-service".into(),
                    channel: "test-channel".into(),
                    model: "test-model".into(),
                    protocol: "test-protocol".into(),
                },
                caps: AskModelCapabilities {
                    context_window: Some(window),
                    output_tokens: 256,
                    image_input: false,
                    image_token_reserve: 0,
                    deep_thinking: false,
                },
                calls: sender,
            }))),
            context: context.clone(),
            search: search.clone(),
            attachments: attachments.clone(),
            spawner: Arc::new(crate::TokioTaskSpawner),
            history: None,
            usage: None,
        });
        Self {
            service,
            calls,
            context,
            search,
            attachments,
        }
    }

    async fn next(&mut self) -> Call {
        tokio::time::timeout(Duration::from_secs(3), self.calls.recv())
            .await
            .expect("model called")
            .expect("call present")
    }
}

fn request(conversation_id: Uuid, client_request_id: &str, text: &str) -> AskSubmit {
    AskSubmit {
        conversation_id,
        client_request_id: client_request_id.into(),
        text: text.into(),
        attachment_ids: Vec::new(),
        options: AskOptions::default(),
    }
}

fn model_text(request: &AskModelRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            AskModelPart::Text(text) => Some(text.as_str()),
            AskModelPart::Image(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn terminal(
    events: &mut broadcast::Receiver<Arc<AskEvent>>,
    key: AskTurnKey,
) -> AskTurnStatus {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.conversation_id == key.conversation_id && event.turn_id == Some(key.turn_id) {
                if let AskEventKind::Terminal { status, .. } = event.kind {
                    return status;
                }
            }
        }
    })
    .await
    .expect("turn has a terminal event")
}

#[tokio::test]
async fn submit_is_idempotent_and_late_cancellation_stays_with_its_owner() {
    let mut fixture = Fixture::new(32_768);
    let service = fixture.service.clone();
    let mut events = service.subscribe();
    let first = service.open();
    let other = service.new_conversation();
    assert_eq!(service.open(), other);
    let original = request(first, "same-id", "only conversation A");
    let (left, right) = tokio::join!(
        service.submit(original.clone()),
        service.submit(original.clone())
    );
    let first_turn = left.unwrap();
    assert_eq!(right.unwrap(), first_turn);
    let delayed = fixture.next().await;
    delayed.delta("partial A");
    let mut conflicting = original;
    conflicting.text.push('!');
    assert_eq!(
        service.submit(conflicting).await.unwrap_err().message,
        "askRequestConflict"
    );
    assert_eq!(
        service
            .submit(request(first, "busy", "wait"))
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::Busy
    );
    let other_turn = service
        .submit(request(other, "same-id", "only conversation B"))
        .await
        .unwrap();
    let other_call = fixture.next().await;
    assert!(!model_text(&other_call.request).contains("conversation A"));
    other_call.complete("B", Some(200), None);
    assert_eq!(
        terminal(
            &mut events,
            AskTurnKey {
                conversation_id: other,
                turn_id: other_turn
            }
        )
        .await,
        AskTurnStatus::Completed
    );
    let old = AskTurnKey {
        conversation_id: first,
        turn_id: first_turn,
    };
    service.cancel(old).unwrap();
    assert_eq!(
        service
            .cancel(AskTurnKey {
                conversation_id: other,
                turn_id: first_turn
            })
            .unwrap_err()
            .message,
        "askTurnNotFound"
    );
    let later = service
        .submit(request(first, "later", "new A"))
        .await
        .unwrap();
    let current = fixture.next().await;
    assert!(!model_text(&current.request).contains("conversation A"));
    delayed.delta("stale A");
    service.cancel(old).unwrap();
    current.complete("new answer", Some(120), Some(60));
    assert_eq!(
        terminal(
            &mut events,
            AskTurnKey {
                conversation_id: first,
                turn_id: later
            }
        )
        .await,
        AskTurnStatus::Completed
    );
    let snapshot = service.snapshot(first).unwrap();
    assert_eq!(snapshot.messages[1].text, "partial A");
    assert_eq!(snapshot.messages[3].text, "new answer");
    let replay = service.events(first, 0).unwrap();
    assert_eq!(replay.latest_sequence, snapshot.sequence);
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.kind, AskEventKind::Terminal { .. }))
            .count(),
        2
    );
    assert_eq!(snapshot.cache[0].hit_rate, Some(0.5));
    assert_eq!(service.snapshot(other).unwrap().cache[0].hit_rate, None);
}

#[tokio::test]
async fn compact_never_receives_old_facts() {
    let mut fixture = Fixture::new(5_200);
    let service = fixture.service.clone();
    let mut events = service.subscribe();
    let conversation_id = service.open();
    *fixture.context.0.lock() = Some("revoked-fact-marker".into());
    let first = request(
        conversation_id,
        "one",
        &format!("Question one {}", "x".repeat(500)),
    );
    let turn_id = service.submit(first).await.unwrap();
    let call = fixture.next().await;
    assert!(model_text(&call.request).contains("revoked-fact-marker"));
    call.complete(&"a".repeat(220), Some(100), Some(0));
    assert_eq!(
        terminal(
            &mut events,
            AskTurnKey {
                conversation_id,
                turn_id
            }
        )
        .await,
        AskTurnStatus::Completed
    );
    assert!(fixture.search.0.lock().is_empty());
    *fixture.context.0.lock() = None;
    for index in 2..=3 {
        let turn_id = service
            .submit(request(
                conversation_id,
                &index.to_string(),
                &format!("Question {index} {}", "x".repeat(500)),
            ))
            .await
            .unwrap();
        let call = fixture.next().await;
        assert!(!model_text(&call.request).contains("revoked-fact-marker"));
        call.complete(&"b".repeat(220), Some(300), Some(150));
        assert_eq!(
            terminal(
                &mut events,
                AskTurnKey {
                    conversation_id,
                    turn_id
                }
            )
            .await,
            AskTurnStatus::Completed
        );
    }

    let turn_id = service
        .submit(request(
            conversation_id,
            "four",
            &format!("New question {}", "y".repeat(600)),
        ))
        .await
        .unwrap();
    let compact = fixture.next().await;
    let compact_text = model_text(&compact.request);
    assert!(compact_text.contains("Question one"));
    assert!(!compact_text.contains("Question 2"));
    assert!(!compact_text.contains("revoked-fact-marker"));
    compact.complete("earlier summary", Some(250), Some(100));
    let answer = fixture.next().await;
    let answer_text = model_text(&answer.request);
    assert!(answer_text.contains("earlier summary"));
    assert!(answer_text.contains("Question 2"));
    assert!(answer_text.contains("Question 3"));
    assert!(!answer_text.contains("Question one"));
    answer.complete("answer", Some(50), Some(50));
    assert_eq!(
        terminal(
            &mut events,
            AskTurnKey {
                conversation_id,
                turn_id
            }
        )
        .await,
        AskTurnStatus::Completed
    );
    let snapshot = service.snapshot(conversation_id).unwrap();
    assert!(snapshot.messages[0].text.contains("Question one"));
    assert_eq!(snapshot.cache[0].comparable_reports, 5);
    assert_eq!(snapshot.cache[0].hit_rate, Some(450.0 / 1000.0));
    assert_eq!(snapshot.turns[3].usage[0].purpose, AskCallPurpose::Compact);
    assert_eq!(snapshot.turns[3].usage[1].purpose, AskCallPurpose::Answer);
}

#[tokio::test]
async fn search_publishes_sources_not_evidence_and_replays_a_bounded_tail() {
    let mut fixture = Fixture::new(32_000);
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    fixture
        .search
        .1
        .store(true, std::sync::atomic::Ordering::Relaxed);
    *fixture.context.0.lock() = Some("private app metadata".into());
    let submission = request(
        conversation_id,
        "request",
        "Find the latest public facts about the topic",
    );
    let turn_id = service.submit(submission).await.unwrap();
    let key = AskTurnKey {
        conversation_id,
        turn_id,
    };
    let decision = fixture.next().await;
    assert!(decision.request.tools.available.search);
    assert!(!decision.request.tools.available.calculator);
    assert!(!model_text(&decision.request).contains("private app metadata"));
    assert!(fixture.search.0.lock().is_empty());
    decision
        .finish
        .send(Ok(AskModelCompletion::ToolCall(AskModelToolCall {
            id: Some("call1".into()),
            tool: AskToolCall::WebSearch {
                public_query: "public query".into(),
            },
            resume: Vec::new(),
        })))
        .ok()
        .unwrap();
    let call = fixture.next().await;
    assert_eq!(fixture.search.0.lock().as_slice(), ["public query"]);
    assert!(model_text(&call.request).contains("private app metadata"));
    assert!(!model_text(&call.request).contains("private-to-model source body"));
    assert!(call.request.tools.results[0]
        .output
        .contains("private-to-model source body"));
    assert!(!call.request.tools.available.search);
    assert!(call.request.tools.available.calculator);
    call.finish
        .send(Ok(AskModelCompletion::ToolCall(AskModelToolCall {
            id: Some("math-call".into()),
            tool: AskToolCall::Calculate {
                expression: "0.1 + 0.2".into(),
            },
            resume: Vec::new(),
        })))
        .ok()
        .unwrap();
    let call = fixture.next().await;
    assert_eq!(call.request.tools.results.len(), 2);
    assert!(call.request.tools.results[1]
        .output
        .contains("\"decimal\":\"0.3\""));
    assert_eq!(fixture.search.0.lock().as_slice(), ["public query"]);
    let search_replay =
        serde_json::to_string(&service.events(conversation_id, 0).unwrap()).unwrap();
    assert!(!search_replay.contains("private-to-model source body"));
    assert_eq!(
        service.turn(key).unwrap().search_status,
        AskSearchStatus::Done
    );
    assert_eq!(
        service.source_url(key, "source-a").unwrap(),
        "https://example.com/result"
    );
    assert!(service.source_url(key, "not-a-real-source").is_err());
    for _ in 0..EVENT_LIMIT + 1 {
        call.delta("x");
    }
    let mut terminal_events = service.subscribe();
    call.complete("done", None, None);
    assert_eq!(
        terminal(&mut terminal_events, key).await,
        AskTurnStatus::Completed
    );
    let snapshot = service.snapshot(conversation_id).unwrap();
    let reply = service.events(conversation_id, 0).unwrap();
    assert!(reply.truncated);
    assert_eq!(reply.events.len(), EVENT_LIMIT);
    assert!(reply
        .events
        .windows(2)
        .all(|events| events[0].sequence + 1 == events[1].sequence));
    let current = service.events(conversation_id, snapshot.sequence).unwrap();
    assert!(!current.truncated);
    assert!(fixture.service.end_conversation(conversation_id).is_ok());
    assert!(service.snapshot(conversation_id).is_err());
    assert!(matches!(
        events.try_recv(),
        Err(broadcast::error::TryRecvError::Lagged(_))
    ));
}

#[tokio::test]
async fn calculator_runs_without_web_search_and_stops_at_its_call_budget() {
    let mut fixture = Fixture::new(32_000);
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    let turn_id = service
        .submit(request(conversation_id, "calc", "What is 1/3?"))
        .await
        .unwrap();
    let key = AskTurnKey {
        conversation_id,
        turn_id,
    };
    for index in 0..MAX_CALCULATIONS {
        let call = fixture.next().await;
        assert!(!call.request.tools.available.search);
        assert!(call.request.tools.available.calculator);
        assert_eq!(call.request.tools.results.len(), index);
        call.finish
            .send(Ok(AskModelCompletion::ToolCall(AskModelToolCall {
                id: Some(format!("calc-{index}")),
                tool: AskToolCall::Calculate {
                    expression: if index == 0 { "1/0" } else { "1/3" }.into(),
                },
                resume: Vec::new(),
            })))
            .ok()
            .unwrap();
    }
    let final_call = fixture.next().await;
    assert!(!final_call.request.tools.available.any());
    let results = &final_call.request.tools.results;
    assert_eq!(results.len(), MAX_CALCULATIONS);
    assert!(results[0].output.contains("division_by_zero"));
    assert!(results[1].output.contains("\"exact\":\"1/3\""));
    assert!(fixture.search.0.lock().is_empty());
    assert_eq!(
        service.turn(key).unwrap().search_status,
        AskSearchStatus::Idle
    );
    final_call.complete("1/3 exactly.", None, None);
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
}

#[tokio::test]
async fn unsupported_images_and_thinking_reject_before_acceptance_and_keep_the_draft() {
    let mut fixture = Fixture::new(100_000);
    let service = fixture.service.clone();
    let owner = service.open();
    let other = service.new_conversation();
    let image_id = Uuid::new_v4();
    fixture.attachments.0.lock().insert(
        image_id,
        (
            owner,
            AskImage {
                mime: AskImageMime::Png,
                bytes: Arc::from([1_u8, 2]),
                width: 1,
                height: 1,
            },
        ),
    );
    let mut submit = request(other, "test", "");
    submit.attachment_ids.push(image_id);
    assert_eq!(
        service.submit(submit.clone()).await.unwrap_err().code,
        BackendErrorCode::PermissionDenied
    );
    submit.conversation_id = owner;
    assert_eq!(
        service.submit(submit).await.unwrap_err().message,
        "askImageUnsupported"
    );
    let mut thinking = request(owner, "think", "question");
    thinking.options.deep_thinking = true;
    assert_eq!(
        service.submit(thinking).await.unwrap_err().message,
        "askDeepThinkingUnsupported"
    );
    assert!(service.snapshot(owner).unwrap().messages.is_empty());
    assert!(fixture.calls.try_recv().is_err());
    service.end_all();
    assert!(fixture.attachments.0.lock().is_empty());
}

#[tokio::test]
async fn a_large_model_window_does_not_raise_the_total_cap() {
    let mut fixture = Fixture::new(1_000_000);
    let service = fixture.service.clone();
    let id = service.open();
    let text = "x".repeat(255_000);
    assert_eq!(
        service
            .submit(request(id, "large", &text))
            .await
            .unwrap_err()
            .message,
        "askContextLimit"
    );
    assert!(service.snapshot(id).unwrap().messages.is_empty());
    assert!(fixture.calls.try_recv().is_err());
}
