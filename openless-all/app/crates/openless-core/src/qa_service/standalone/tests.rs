use super::*;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

struct TestModel {
    label: AskModelLabel,
    caps: AskModelCapabilities,
    calls: mpsc::UnboundedSender<Call>,
    intent_tool: bool,
    manual_review: std::sync::atomic::AtomicBool,
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

    fn supports_intent_tool(&self) -> bool {
        self.intent_tool
    }

    fn stream(
        &self,
        request: AskModelRequest,
        _cancellation: AskCancellation,
        progress: AskModelSink,
    ) -> BoxFuture<'_, Result<AskModelCompletion, BackendError>> {
        Box::pin(async move {
            if request.instructions == DELIVERY_REVIEW_RULE
                && !self.manual_review.load(std::sync::atomic::Ordering::Relaxed)
            {
                progress(AskModelProgress::Text("deliver".into()));
                return Ok(AskModelCompletion::Complete);
            }
            let (finish, receiver) = oneshot::channel();
            assert!(self
                .calls
                .send(Call {
                    request,
                    progress,
                    finish,
                    framed: Default::default(),
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
    framed: std::sync::atomic::AtomicBool,
}

impl Call {
    fn tool(self, tool: AskToolCall) {
        assert!(self
            .finish
            .send(Ok(AskModelCompletion::ToolCall(AskModelToolCall {
                tool,
                id: Some("fixture-tool-id".into()),
                resume: Vec::new(),
            })))
            .is_ok());
    }

    fn raw_delta(&self, text: &str) {
        (self.progress)(AskModelProgress::Text(text.to_owned()));
    }

    fn delta(&self, text: &str) {
        if self.request.instructions.contains(FULFILLMENT_RULE)
            && !self.framed.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            self.raw_delta("ASK_RESULT direct\n");
        }
        self.raw_delta(text);
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

    fn complete_raw(self, text: &str) {
        self.raw_delta(text);
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
                    name: "foreground_app".into(),
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
struct Search(
    Mutex<Vec<String>>,
    std::sync::atomic::AtomicBool,
    Mutex<Option<Result<AskSearchResult, BackendError>>>,
);

impl SearchTool for Search {
    fn enabled(&self) -> BoxFuture<'_, Result<bool, BackendError>> {
        Box::pin(async { Ok(self.1.load(std::sync::atomic::Ordering::Relaxed)) })
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
            if let Some(outcome) = self.2.lock().take() {
                return outcome;
            }
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
                evidence: "private-to-model source body: 1 USD = 1.25 quoted currency".into(),
            })
        })
    }
}

#[derive(Default)]
struct Attachments(
    Mutex<HashMap<Uuid, (Uuid, AskImage)>>,
    Mutex<HashMap<Uuid, (Uuid, Arc<str>)>>,
);

impl AttachmentReader for Attachments {
    fn acquire(&self, turn: AskTurnKey, ids: &[Uuid]) -> Result<Vec<AskAttachment>, BackendError> {
        let entries = self.0.lock();
        let selections = self.1.lock();
        ids.iter()
            .map(|id| {
                if let Some((_, image)) = entries
                    .get(id)
                    .filter(|(owner, _)| *owner == turn.conversation_id)
                {
                    Ok(AskAttachment::Image(image.clone()))
                } else if let Some((_, text)) = selections
                    .get(id)
                    .filter(|(owner, _)| *owner == turn.conversation_id)
                {
                    Ok(AskAttachment::Selection { text: text.clone() })
                } else {
                    Err(ask_error(
                        BackendErrorCode::PermissionDenied,
                        "testAttachmentScope",
                    ))
                }
            })
            .collect()
    }

    fn end_conversation(&self, id: Uuid) {
        self.0.lock().retain(|_, (owner, _)| *owner != id);
        self.1.lock().retain(|_, (owner, _)| *owner != id);
    }
}

struct Fixture {
    service: StandaloneQaService,
    calls: mpsc::UnboundedReceiver<Call>,
    context: Arc<Context>,
    search: Arc<Search>,
    attachments: Arc<Attachments>,
    history: Arc<Mutex<Vec<(String, String)>>>,
    model: Arc<TestModel>,
}

impl Fixture {
    fn new(window: usize) -> Self {
        Self::with_intent_tool(window, true)
    }

    fn with_intent_tool(window: usize, intent_tool: bool) -> Self {
        Self::with_capabilities(window, intent_tool, false)
    }

    fn with_capabilities(window: usize, intent_tool: bool, image_input: bool) -> Self {
        let (sender, calls) = mpsc::unbounded_channel();
        let context = Arc::new(Context::default());
        let search = Arc::new(Search::default());
        let attachments = Arc::new(Attachments::default());
        let history = Arc::new(Mutex::new(Vec::new()));
        let recorded = history.clone();
        let model = Arc::new(TestModel {
                label: AskModelLabel {
                    service: "test-service".into(),
                    channel: "test-channel".into(),
                    model: "test-model".into(),
                    protocol: "test-protocol".into(),
                },
                caps: AskModelCapabilities {
                    context_window: Some(window),
                    output_tokens: 256,
                    image_input,
                    image_token_reserve: 0,
                    deep_thinking: false,
                },
                calls: sender,
                intent_tool,
                manual_review: Default::default(),
            });
        let service = StandaloneQaService::new(StandaloneQaDependencies {
            models: Arc::new(Factory(model.clone())),
            context: context.clone(),
            search: search.clone(),
            attachments: attachments.clone(),
            spawner: Arc::new(crate::TokioTaskSpawner),
            history: Some(Arc::new(move |question, answer| {
                recorded.lock().push((question.into(), answer.into()));
            })),
            usage: None,
        });
        Self {
            service,
            calls,
            context,
            search,
            attachments,
            history,
            model,
        }
    }

    fn review_manually(&self) {
        self.model.manual_review.store(true, std::sync::atomic::Ordering::Relaxed);
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

struct WriteAttempt {
    operation: AskWriteOperation,
    body: String,
    claim: AskWriteClaim,
    outcome: oneshot::Sender<AskWriteOutcome>,
}

struct TestWriter(mpsc::UnboundedSender<WriteAttempt>);

impl AskWriter for TestWriter {
    fn write(
        &self,
        operation: AskWriteOperation,
        body: String,
        claim: AskWriteClaim,
    ) -> BoxFuture<'_, AskWriteOutcome> {
        Box::pin(async move {
            let (outcome, result) = oneshot::channel();
            assert!(self
                .0
                .send(WriteAttempt {
                    operation,
                    body,
                    claim,
                    outcome
                })
                .is_ok());
            result.await.unwrap()
        })
    }
}

struct TestClipboard {
    copies: Mutex<Vec<String>>,
    service: StandaloneQaService,
    conversation_id: Uuid,
    fail: bool,
}

impl AskClipboard for TestClipboard {
    fn copy(&self, body: String) -> BoxFuture<'_, Result<(), BackendError>> {
        Box::pin(async move {
            let submission = self
                .service
                .snapshot(self.conversation_id)
                .unwrap()
                .submission
                .unwrap();
            assert_eq!(submission.result_text.as_deref(), Some(body.as_str()));
            assert_eq!(submission.copy_status, AskCopyStatus::Pending);
            assert!(matches!(
                submission.delivery,
                Some(AskDeliveryStatus::Retained | AskDeliveryStatus::Unknown)
            ));
            self.copies.lock().push(body);
            if self.fail {
                Err(ask_error(BackendErrorCode::Internal, "testClipboardFailed"))
            } else {
                Ok(())
            }
        })
    }
}

fn clipboard(
    service: &StandaloneQaService,
    conversation_id: Uuid,
    fail: bool,
) -> Arc<TestClipboard> {
    Arc::new(TestClipboard {
        copies: Mutex::default(),
        service: service.clone(),
        conversation_id,
        fail,
    })
}

async fn prepared(
    service: &StandaloneQaService,
    conversation_id: Uuid,
    text: &str,
    prepare: AskPreparation,
) -> AskTurnKey {
    let turn_id = service
        .submit_prepared(request(conversation_id, text, text), || async {
            Ok(prepare)
        })
        .await
        .unwrap();
    AskTurnKey {
        conversation_id,
        turn_id,
    }
}

fn assert_no_chat(service: &StandaloneQaService, key: AskTurnKey) {
    let events = service.events(key.conversation_id, 0).unwrap();
    for event in events
        .events
        .into_iter()
        .filter(|event| event.turn_id == Some(key.turn_id))
    {
        assert!(!matches!(
            event.kind,
            AskEventKind::Accepted { .. } | AskEventKind::AnswerDelta { .. }
        ));
    }
    let snapshot = service.snapshot(key.conversation_id).unwrap();
    assert!(!snapshot
        .turns
        .iter()
        .any(|turn| turn.turn_id == key.turn_id));
    assert!(!snapshot
        .messages
        .iter()
        .any(|message| message.turn_id == key.turn_id));
}

#[tokio::test]
async fn lazy_capture_is_once_and_chat_acceptance_follows_isolated_hybrid_routing() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut fixture = Fixture::new(32_768);
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    let captures = AtomicUsize::new(0);
    let original = request(
        conversation_id,
        "first",
        "Explain in simpler words why this passage is confusing",
    );
    let selected = Mutex::new("original external selection".to_owned());
    let (entered, capture_started) = oneshot::channel();
    let (resume, capture_resume) = oneshot::channel();
    let submit = service.submit_prepared(original.clone(), || async {
        captures.fetch_add(1, Ordering::SeqCst);
        let frozen = selected.lock().clone();
        entered.send(()).unwrap();
        capture_resume.await.unwrap();
        Ok(AskPreparation {
            selection: ExternalSelection::Selected(frozen),
            ..Default::default()
        })
    });
    let verify = async {
        capture_started.await.unwrap();
        assert!(service
            .snapshot(conversation_id)
            .unwrap()
            .submission
            .is_none());
        *selected.lock() = "later selection".to_owned();
        resume.send(()).unwrap();
        let duplicate = service
            .submit_prepared(original.clone(), || async {
                captures.fetch_add(1, Ordering::SeqCst);
                Ok(AskPreparation::default())
            })
            .await
            .unwrap();
        assert_eq!(
            service
                .submit_prepared(request(conversation_id, "first", "conflicting"), || async {
                    panic!("conflict must not capture");
                })
                .await
                .unwrap_err()
                .message,
            "askRequestConflict"
        );
        assert_eq!(
            service
                .submit_prepared(request(conversation_id, "busy", "busy"), || async {
                    panic!("busy must not capture");
                })
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        duplicate
    };
    let (first, duplicate) = tokio::join!(submit, verify);
    let turn_id = first.unwrap();
    assert_eq!(turn_id, duplicate);
    assert_eq!(captures.load(Ordering::SeqCst), 1);
    let pending = service.snapshot(conversation_id).unwrap();
    assert!(pending.messages.is_empty());
    assert!(pending.turns.is_empty());
    assert!(pending.submission.unwrap().route.is_none());
    assert!(matches!(
        events.recv().await.unwrap().kind,
        AskEventKind::Submitted { .. }
    ));
    let call = fixture.next().await;
    assert!(model_text(&call.request).contains("\"selection\":\"present\""));
    assert!(!model_text(&call.request).contains("external selection"));
    assert!(!call.request.tools.available.any());
    call.complete(r#"{"route":"selectionAnswer"}"#, None, None);
    let call = fixture.next().await;
    let replay = service.events(conversation_id, 0).unwrap();
    let route = replay
        .events
        .iter()
        .position(|event| {
            matches!(
                event.kind,
                AskEventKind::RouteResolved {
                    route: AskRoute::SelectionAnswer
                }
            )
        })
        .unwrap();
    assert!(matches!(
        replay.events[route + 1].kind,
        AskEventKind::Accepted { .. }
    ));
    assert!(model_text(&call.request).contains("original external selection"));
    assert!(!model_text(&call.request).contains("later selection"));
    call.complete("inspection", None, None);
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
    assert_eq!(snapshot.messages.len(), 2);
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains("original external selection"));
}

#[tokio::test]
async fn accepted_chat_bypasses_entry_preparation_but_keeps_current_attachments() {
    let mut fixture = Fixture::new(32_768);
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    let previous = prepared(
        &service,
        conversation_id,
        "What is the capital of Japan?",
        AskPreparation::default(),
    )
    .await;
    fixture.next().await.complete("Tokyo", None, None);
    assert_eq!(
        terminal(&mut events, previous).await,
        AskTurnStatus::Completed
    );
    let attachment_id = Uuid::new_v4();
    fixture.attachments.1.lock().insert(
        attachment_id,
        (conversation_id, Arc::from("private attachment text")),
    );
    let mut submission = request(conversation_id, "second", "What is the capital of France?");
    submission.attachment_ids.push(attachment_id);
    let turn_id = service
        .submit_prepared(submission, || async {
            panic!("a locked chat must not read the external selection or prepare classification")
        })
        .await
        .unwrap();
    let key = AskTurnKey {
        conversation_id,
        turn_id,
    };
    let answer = fixture.next().await;
    assert!(answer.request.tools.intent.is_none());
    assert!(model_text(&answer.request).contains("Tokyo"));
    assert!(model_text(&answer.request).contains("private attachment text"));
    assert!(!model_text(&answer.request).contains("private unrelated"));
    answer.complete("Paris", None, None);
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
}

#[tokio::test]
async fn main_model_tool_continues_into_search_calculation_and_inserts_body_without_chat() {
    let mut fixture = Fixture::new(32_768);
    fixture.review_manually();
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    fixture
        .search
        .1
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let (writer, mut attempts) = mpsc::unbounded_channel();
    let clipboard = clipboard(&service, conversation_id, false);
    let key = prepared(
        &service,
        conversation_id,
        "Look up the exchange rate, calculate the price, then write a quote",
        AskPreparation {
            intent_mode: AskIntentMode::MainModelTool,
            selection: ExternalSelection::None,
            writer: Some(Arc::new(TestWriter(writer))),
            clipboard: Some(clipboard.clone()),
        },
    )
    .await;
    let intent = fixture.next().await;
    assert!(intent.request.tools.available.resolve_intent);
    assert!(!intent.request.tools.available.search && !intent.request.tools.available.calculator);
    let classifier_input = model_text(&intent.request);
    intent.tool(AskToolCall::ResolveIntent {
        route: AskRoute::Write,
    });
    let search = fixture.next().await;
    assert!(search.request.tools.intent.is_none());
    assert!(search.request.tools.available.search);
    assert!(!search.request.tools.available.resolve_intent);
    search.tool(AskToolCall::WebSearch {
        public_query: "USD exchange rate".into(),
    });
    let calc = fixture.next().await;
    let continuation = calc.request.tools.intent.as_ref().unwrap();
    let [AskModelPart::Text(input)] = continuation.input.parts.as_slice() else { panic!("missing classifier input") };
    assert_eq!(input, &classifier_input);
    assert_eq!(continuation.result.call.id.as_deref(), Some("fixture-tool-id"));
    assert!(continuation.result.output.contains("\"route\":\"write\""));
    assert!(calc.request.instructions.contains("只输出一份可直接插入的正文"));
    assert!(!calc.request.tools.available.search);
    assert!(calc.request.tools.available.calculator && calc.request.tools.intent.is_some());
    assert_eq!(fixture.search.0.lock().as_slice(), ["USD exchange rate"]);
    assert!(calc.request.tools.results.is_empty());
    assert!(model_text(&calc.request).contains("1 USD = 1.25"));
    calc.tool(AskToolCall::Calculate {
        expression: "40 * 1.25".into(),
    });
    let compose = fixture.next().await;
    assert!(compose.request.tools.results[0]
        .output
        .contains("\"decimal\":\"50\""));
    compose.delta("Quote\n");
    assert!(attempts.try_recv().is_err());
    compose.complete("Total: $50", None, None);
    let validation = fixture.next().await;
    assert_eq!(validation.request.instructions, DELIVERY_REVIEW_RULE);
    assert!(!validation.request.tools.available.any());
    assert!(model_text(&validation.request).contains("Total: $50"));
    assert!(!model_text(&validation.request).contains("private-to-model source body"));
    validation.complete_raw("deliver");
    let effect = tokio::time::timeout(Duration::from_secs(3), attempts.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(effect.operation, AskWriteOperation::Insert);
    assert_eq!(effect.body, "Quote\nTotal: $50");
    assert_eq!(
        service
            .snapshot(conversation_id)
            .unwrap()
            .submission
            .unwrap()
            .delivery,
        Some(AskDeliveryStatus::Pending)
    );
    assert!(effect.claim.claim());
    effect.outcome.send(AskWriteOutcome::Written).unwrap();
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    let result = service
        .snapshot(conversation_id)
        .unwrap()
        .submission
        .unwrap();
    assert_eq!(result.delivery, Some(AskDeliveryStatus::Written));
    assert_eq!(result.copy_status, AskCopyStatus::NotNeeded);
    assert!(
        result.result_text.is_none()
            && clipboard.copies.lock().is_empty()
            && fixture.history.lock().is_empty()
    );
    assert_no_chat(&service, key);
}

#[tokio::test]
async fn write_with_incidental_selection_retains_once_before_failed_copy_and_never_becomes_history()
{
    let mut fixture = Fixture::new(32_768);
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    let (writer, mut attempts) = mpsc::unbounded_channel();
    let clipboard = clipboard(&service, conversation_id, true);
    let key = prepared(
        &service,
        conversation_id,
        "写一封道歉邮件",
        AskPreparation {
            selection: ExternalSelection::Selected("incidental selected text".into()),
            writer: Some(Arc::new(TestWriter(writer))),
            clipboard: Some(clipboard.clone()),
            ..Default::default()
        },
    )
    .await;
    let compose = fixture.next().await;
    assert!(!model_text(&compose.request).contains("incidental selected"));
    compose.complete("很抱歉。\n谢谢理解。", None, None);
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    let snapshot = service.snapshot(conversation_id).unwrap();
    let retained = snapshot.submission.unwrap();
    assert_eq!(retained.route, Some(AskRoute::Write));
    assert_eq!(retained.delivery, Some(AskDeliveryStatus::Retained));
    assert_eq!(retained.copy_status, AskCopyStatus::Failed);
    assert_eq!(
        retained.result_text.as_deref(),
        Some("很抱歉。\n谢谢理解。")
    );
    assert!(attempts.try_recv().is_err());
    assert_no_chat(&service, key);
    assert!(fixture.history.lock().is_empty());
    let replay = service.events(conversation_id, 0).unwrap();
    assert!(replay.events.iter().any(|event| matches!(&event.kind,
        AskEventKind::Delivery { submission } if submission.copy_status == AskCopyStatus::Pending
            && submission.result_text == retained.result_text)));
    assert_eq!(clipboard.copies.lock().len(), 1);
    let next = prepared(
        &service,
        conversation_id,
        "What is the capital of Japan?",
        AskPreparation::default(),
    )
    .await;
    let answer = fixture.next().await;
    assert!(!model_text(&answer.request).contains("谢谢理解"));
    assert!(service
        .snapshot(conversation_id)
        .unwrap()
        .submission
        .unwrap()
        .result_text
        .is_none());
    answer.complete("Tokyo", None, None);
    assert_eq!(terminal(&mut events, next).await, AskTurnStatus::Completed);
    assert_eq!(clipboard.copies.lock().len(), 1);
}

#[tokio::test]
async fn edit_claim_cancellation_and_fallback_never_duplicate_live_effects() {
    for (cancel_before, cancel_after, outcome, delivery) in [
        (true, false, AskWriteOutcome::Declined, None),
        (
            false,
            true,
            AskWriteOutcome::Written,
            Some(AskDeliveryStatus::Written),
        ),
        (
            false,
            true,
            AskWriteOutcome::Unknown,
            Some(AskDeliveryStatus::Unknown),
        ),
        (
            false,
            false,
            AskWriteOutcome::Unknown,
            Some(AskDeliveryStatus::Unknown),
        ),
        (
            false,
            false,
            AskWriteOutcome::NotWritten,
            Some(AskDeliveryStatus::Retained),
        ),
    ] {
        let mut fixture = Fixture::new(32_768);
        let service = fixture.service.clone();
        let conversation_id = service.open();
        let mut events = service.subscribe();
        let (writer, mut attempts) = mpsc::unbounded_channel();
        let clipboard = clipboard(&service, conversation_id, false);
        let key = prepared(
            &service,
            conversation_id,
            "Make this shorter, please?",
            AskPreparation {
                selection: ExternalSelection::Selected("original\nselection".into()),
                writer: Some(Arc::new(TestWriter(writer))),
                clipboard: Some(clipboard.clone()),
                ..Default::default()
            },
        )
        .await;
        let model = fixture.next().await;
        assert!(
            model_text(&model.request).contains(r#"{"externalSelection":"original\nselection"}"#)
        );
        model.complete("shorter", None, None);
        let effect = tokio::time::timeout(Duration::from_secs(3), attempts.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(effect.operation, AskWriteOperation::Replace);
        if cancel_before {
            service.cancel(key).unwrap();
            assert!(!effect.claim.claim());
            assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Cancelled);
            effect.outcome.send(outcome).unwrap();
        } else {
            if outcome != AskWriteOutcome::NotWritten {
                assert!(effect.claim.claim());
            }
            if cancel_after {
                service.cancel(key).unwrap();
                assert_eq!(service.turn(key).unwrap().status, AskTurnStatus::Running);
            }
            effect.outcome.send(outcome).unwrap();
            assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
        }
        let result = service
            .snapshot(conversation_id)
            .unwrap()
            .submission
            .unwrap();
        let delivery = if cancel_before { Some(AskDeliveryStatus::Retained) } else { delivery };
        assert_eq!(result.delivery, delivery);
        let retained = matches!(
            delivery,
            Some(AskDeliveryStatus::Unknown | AskDeliveryStatus::Retained)
        );
        assert_eq!(result.result_text.as_deref(), retained.then_some("shorter"));
        assert_eq!(
            clipboard.copies.lock().len(),
            usize::from(retained && !cancel_before && !cancel_after)
        );
        assert!(attempts.try_recv().is_err());
        assert_no_chat(&service, key);
    }
}

#[tokio::test]
async fn missing_selection_and_invalid_or_unsupported_classifier_stay_in_composer() {
    let mut fixture = Fixture::with_intent_tool(32_768, false);
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    let clipboard = clipboard(&service, conversation_id, false);
    let key = prepared(
        &service,
        conversation_id,
        "这段是什么意思？",
        AskPreparation {
            selection: ExternalSelection::Unavailable,
            clipboard: Some(clipboard.clone()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
    let result = service
        .snapshot(conversation_id)
        .unwrap()
        .submission
        .unwrap();
    assert_eq!(result.route, Some(AskRoute::Clarify));
    assert_eq!(result.error.unwrap().message, "askSelectionUnavailable");
    assert_no_chat(&service, key);
    assert!(fixture.calls.try_recv().is_err() && clipboard.copies.lock().is_empty());
    let key = prepared(
        &service,
        conversation_id,
        "tool unsupported",
        AskPreparation {
            intent_mode: AskIntentMode::MainModelTool,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
    assert_eq!(
        service.turn(key).unwrap().error.unwrap().message,
        "askIntentToolUnsupported"
    );
    assert!(fixture.calls.try_recv().is_err());
    let key = prepared(
        &service,
        conversation_id,
        "invalid model output",
        AskPreparation {
            intent_mode: AskIntentMode::SeparateModel,
            ..Default::default()
        },
    )
    .await;
    fixture
        .next()
        .await
        .complete(r#"{"route":"edit","sideEffect":"replaceAll"}"#, None, None);
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
    assert_eq!(
        service.turn(key).unwrap().error.unwrap().message,
        "askIntentInvalid"
    );
    assert!(fixture.calls.try_recv().is_err());
    assert_no_chat(&service, key);
}

#[tokio::test]
async fn beta_classifiers_cannot_turn_clear_non_insert_requests_into_writes() {
    for intent_mode in [AskIntentMode::SeparateModel, AskIntentMode::MainModelTool] {
        let mut fixture = Fixture::new(32_768);
        let service = fixture.service.clone();
        let conversation_id = service.open();
        let mut events = service.subscribe();
        let (writer, mut attempts) = mpsc::unbounded_channel();
        let clipboard = clipboard(&service, conversation_id, false);
        for text in [
            "What is the capital of Japan?",
            "What does this mean?",
            "Make this shorter, please?",
        ] {
            let key = prepared(
                &service,
                conversation_id,
                text,
                AskPreparation {
                    intent_mode,
                    selection: ExternalSelection::None,
                    writer: Some(Arc::new(TestWriter(writer.clone()))),
                    clipboard: Some(clipboard.clone()),
                },
            )
            .await;
            let decision = fixture.next().await;
            match intent_mode {
                AskIntentMode::SeparateModel => {
                    assert!(!decision.request.tools.available.any());
                    decision.complete(r#"{"route":"write"}"#, None, None);
                }
                AskIntentMode::MainModelTool => {
                    assert!(decision.request.tools.available.resolve_intent);
                    decision.tool(AskToolCall::ResolveIntent {
                        route: AskRoute::Write,
                    });
                }
                AskIntentMode::SemanticHybrid => unreachable!(),
            }
            let status = tokio::select! {
                status = terminal(&mut events, key) => status,
                _ = fixture.calls.recv() => panic!("misrouting reached generation: {intent_mode:?}, {text}"),
            };
            assert_eq!(status, AskTurnStatus::Failed, "{intent_mode:?}, {text}");
            let submission = service
                .snapshot(conversation_id)
                .unwrap()
                .submission
                .unwrap();
            assert_eq!(submission.error.unwrap().message, "askIntentInvalid");
            assert!(submission.route.is_none() && submission.delivery.is_none());
            assert!(attempts.try_recv().is_err());
            assert!(clipboard.copies.lock().is_empty());
            assert_no_chat(&service, key);
        }
    }
}

#[tokio::test]
async fn main_model_rejects_missing_selection_without_requesting_a_followup() {
    let mut fixture = Fixture::new(32_768);
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    let prepare = || AskPreparation {
        intent_mode: AskIntentMode::MainModelTool,
        selection: ExternalSelection::None,
        ..Default::default()
    };
    let key = prepared(
        &service,
        conversation_id,
        "Please edit my selection",
        prepare(),
    )
    .await;
    fixture.next().await.tool(AskToolCall::ResolveIntent {
        route: AskRoute::Edit,
    });
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
    assert_eq!(service.turn(key).unwrap().error.unwrap().message, "askSelectionRequired");
    assert!(fixture.calls.try_recv().is_err());
    assert_no_chat(&service, key);
    let key = prepared(
        &service,
        conversation_id,
        "Please show me the result",
        prepare(),
    )
    .await;
    fixture
        .next()
        .await
        .complete(r#"{"route":"answer"}"#, None, None);
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
    assert_eq!(
        service.turn(key).unwrap().error.unwrap().message,
        "askIntentInvalid"
    );
    assert!(fixture.calls.try_recv().is_err());
    assert_no_chat(&service, key);
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
    delayed.delta("partial A\n\n");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.turn_id == Some(first_turn)
                && matches!(&event.kind, AskEventKind::AnswerDelta { delta, .. } if delta == "partial A\n\n")
            {
                break;
            }
        }
    }).await.unwrap();
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
    assert_eq!(snapshot.messages[1].text, "partial A\n\n");
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
        &format!("Question one about the current app {}", "x".repeat(500)),
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
    let key = AskTurnKey { conversation_id, turn_id };
    let answer = tokio::select! {
        answer = fixture.next() => answer,
        status = terminal(&mut events, key) => panic!("final provider call missing: {status:?} {:?}", service.turn(key).unwrap().error),
    };
    let answer_text = model_text(&answer.request);
    assert!(answer_text.contains("earlier summary"));
    assert!(answer_text.contains("conversationSummary"));
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
        "Search the web for recent rates",
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
    assert!(!model_text(&call.request).contains("private app metadata"));
    assert!(call.request.tools.results[0].output.contains("private-to-model source body"));
    assert!(!call.request.tools.available.search);
    assert!(!call.request.tools.available.calculator);
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
    for tokens in 1..=EVENT_LIMIT + 1 {
        (call.progress)(AskModelProgress::Usage(ReportedTokens {
            output: Some(tokens as u64),
            ..Default::default()
        }));
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

#[tokio::test]
async fn expanded_opening_never_reclassifies_or_writes_but_a_fresh_opening_can_edit() {
    for mode in [AskIntentMode::SemanticHybrid, AskIntentMode::SeparateModel, AskIntentMode::MainModelTool] {
        let mut fixture = Fixture::new(32_768);
        fixture.review_manually();
        let service = fixture.service.clone();
        let mut events = service.subscribe();
        let first = service.open_transient();
        let question = prepared(&service, first, "Hello", AskPreparation {
            intent_mode: mode, ..Default::default()
        }).await;
        match mode {
            AskIntentMode::SemanticHybrid => {}
            AskIntentMode::SeparateModel => fixture.next().await.complete(r#"{"route":"answer"}"#, None, None),
            AskIntentMode::MainModelTool => fixture.next().await.tool(AskToolCall::ResolveIntent { route: AskRoute::Answer }),
        }
        fixture.next().await.complete("First opening sentinel.", None, None);
        fixture.next().await.complete_raw("deliver");
        assert_eq!(terminal(&mut events, question).await, AskTurnStatus::Completed);

        let following = service.submit_prepared(
            request(first, "chat-edit", "Make this shorter, please?"),
            || async { panic!("no external selection or classifier for locked chat") },
        ).await.unwrap();
        let answer = fixture.next().await;
        assert!(answer.request.tools.intent.is_none() && !answer.request.tools.available.resolve_intent);
        assert!(model_text(&answer.request).contains("First opening sentinel"));
        let confirmation = match mode {
            AskIntentMode::SemanticHybrid => "你想让我做的是哪一件事？\n\n1. 直接替换\n2. 只在这里展示\n\n请确认一下，我再继续。",
            AskIntentMode::SeparateModel => "Should I edit the original or show it here? I’ll wait for your choice.",
            AskIntentMode::MainModelTool => "Précisez l’option retenue avant que je poursuive.",
        };
        let following = AskTurnKey { conversation_id: first, turn_id: following };
        if mode == AskIntentMode::SemanticHybrid {
            let (first_paragraph, rest) = confirmation.split_once("\n\n").unwrap();
            answer.delta(&format!("{first_paragraph}\n\n"));
            assert!(service.snapshot(first).unwrap().messages.last().unwrap().text.is_empty());
            answer.complete(rest, None, None);
        } else {
            answer.complete(confirmation, None, None);
        }
        let review = fixture.next().await;
        assert_eq!(review.request.instructions, DELIVERY_REVIEW_RULE);
        assert!(model_text(&review.request).contains("candidateBody"));
        assert!(!model_text(&review.request).contains("First opening sentinel"));
        assert!(service.snapshot(first).unwrap().messages.last().unwrap().text.is_empty());
        review.complete_raw("reject");
        assert_eq!(terminal(&mut events, following).await, AskTurnStatus::Failed);
        let snapshot = service.snapshot(first).unwrap();
        let rejected = snapshot.submission.unwrap();
        assert!(rejected.delivery.is_none() && rejected.result_text.is_none());
        assert_eq!(rejected.error.unwrap().message, "askAnswerUnusable");
        assert!(snapshot.messages.last().unwrap().text.is_empty());
        assert!(!snapshot.messages.last().unwrap().text.contains(confirmation));
        assert!(!snapshot.messages.last().unwrap().text.contains("直接替换"));
        assert!(!service.events(first, 0).unwrap().events.iter().any(|event| {
            matches!(&event.kind, AskEventKind::AnswerDelta { delta, .. } if delta.contains("请确认") || delta.contains("哪一件事"))
        }));

        let legitimate = service.submit_prepared(
            request(first, "chat-question-content", "Show the shorter invitation here."),
            || async { panic!("no classification within the chat opening") },
        ).await.unwrap();
        let invitation = "Would you like to join us this Friday?";
        fixture.next().await.complete(invitation, None, None);
        fixture.next().await.complete_raw("deliver");
        assert_eq!(terminal(&mut events, AskTurnKey { conversation_id: first, turn_id: legitimate }).await, AskTurnStatus::Completed);
        assert_eq!(service.snapshot(first).unwrap().messages.last().unwrap().text, invitation);

        let second = service.open_transient();
        assert!(service.snapshot(first).is_err());
        assert!(service.snapshot(second).unwrap().messages.is_empty());
        let (writer, mut effects) = mpsc::unbounded_channel();
        let edit = prepared(&service, second, "能把这段改短吗？", AskPreparation {
            intent_mode: mode,
            selection: ExternalSelection::Selected("current selection sentinel".into()),
            writer: Some(Arc::new(TestWriter(writer))),
            ..Default::default()
        }).await;
        if mode != AskIntentMode::SemanticHybrid {
            let classifier = fixture.next().await;
            assert!(!model_text(&classifier.request).contains("current selection sentinel"));
            assert!(!model_text(&classifier.request).contains("First opening sentinel"));
            if mode == AskIntentMode::MainModelTool {
                classifier.tool(AskToolCall::ResolveIntent { route: AskRoute::Edit });
            } else {
                classifier.complete(r#"{"route":"edit"}"#, None, None);
            }
        }
        let model = fixture.next().await;
        let material = model_text(&model.request);
        assert!(material.contains("current selection sentinel"));
        assert!(!material.contains("First opening sentinel"));
        model.complete("Short.", None, None);
        fixture.next().await.complete_raw("deliver");
        let effect = effects.recv().await.unwrap();
        assert_eq!(effect.operation, AskWriteOperation::Replace);
        assert!(effect.claim.claim());
        effect.outcome.send(AskWriteOutcome::Written).unwrap();
        assert_eq!(terminal(&mut events, edit).await, AskTurnStatus::Completed);
        assert!(effects.try_recv().is_err() && fixture.history.lock().is_empty());
    }
}

#[tokio::test]
async fn opening_fence_rejects_queued_preparation_and_drops_a_late_provider_delta() {
    let mut fixture = Fixture::new(32_768);
    let service = fixture.service.clone();
    let old = service.open_transient();
    let fence = AskSessionFence::default();
    let (started, entered) = oneshot::channel();
    let pending = service.submit_prepared_guarded(request(old, "held", "old pending sentinel"), || async move {
        started.send(()).unwrap();
        std::future::pending().await
    }, || true, fence.issue());
    let revoke = async { entered.await.unwrap(); fence.revoke(); };
    assert!(tokio::join!(pending, revoke).0.is_err());
    assert!(fixture.calls.try_recv().is_err());

    service.submit_prepared_guarded(request(old, "active", "Hello"), || async {
        Ok(AskPreparation::default())
    }, || true, fence.issue()).await.unwrap();
    let stale = fixture.next().await;
    fence.revoke();
    let fresh = service.open_transient();
    stale.delta("old late sentinel\n\n");
    let mut events = service.subscribe();
    let next = service.submit(request(fresh, "fresh", "new question sentinel")).await.unwrap();
    let answer = fixture.next().await;
    let payload = model_text(&answer.request);
    assert!(payload.contains("new question sentinel"));
    assert!(!payload.contains("old late sentinel") && !payload.contains("Hello"));
    assert!(answer.request.tools.intent.is_none());
    answer.complete("Fresh answer.", None, None);
    assert_eq!(terminal(&mut events, AskTurnKey { conversation_id: fresh, turn_id: next }).await, AskTurnStatus::Completed);
    assert!(fixture.history.lock().is_empty());
}

#[tokio::test]
async fn explicit_news_and_same_opening_retry_keep_public_scope_but_reopen_does_not() {
    let mut fixture = Fixture::new(32_768);
    fixture.search.1.store(true, std::sync::atomic::Ordering::Relaxed);
    let service = fixture.service.clone();
    let mut events = service.subscribe();
    let first = service.open_transient();
    let public = "Berlin transit strike official 2026-11-19";
    for (index, (message, search)) in [
        ("Search the web for Berlin transit strike on 2026-11-19, official sources only.", true),
        ("用‘搜索’造一个句子", false),
        ("Did the previously mentioned disruption affect the morning service in those reports?", true),
    ].into_iter().enumerate() {
        let id = service.submit(request(first, message, message)).await.unwrap();
        let decision = fixture.next().await;
        let wire = model_text(&decision.request);
        assert!(decision.request.tools.available.search);
        assert!(wire.contains(message) && !wire.contains("private-answer-sentinel"));
        if index > 0 { assert!(wire.contains(public)); }
        if search {
            decision.tool(AskToolCall::WebSearch { public_query: public.into() });
        } else {
            decision.complete_raw(r#""notNeeded""#);
        }
        let answer = fixture.next().await;
        assert!(!answer.request.tools.available.search);
        assert_eq!(answer.request.instructions.contains("本轮搜索已由应用处理"), search);
        if index == 0 {
            assert_eq!(answer.request.tools.results.len(), 1);
        } else if search {
            assert!(answer.request.tools.results.is_empty());
            assert!(model_text(&answer.request).contains("publicSearch"));
        }
        answer.complete(if search { "private-answer-sentinel" } else { "我正在搜索一本书的资料。" }, None, None);
        assert_eq!(terminal(&mut events, AskTurnKey { conversation_id: first, turn_id: id }).await, AskTurnStatus::Completed);
        if !search {
            assert_eq!(service.snapshot(first).unwrap().messages.last().unwrap().text, "我正在搜索一本书的资料。");
            assert_eq!(fixture.search.0.lock().len(), 1);
        }
    }
    assert_eq!(fixture.search.0.lock().as_slice(), [public, public]);
    let fresh = service.open_transient();
    let id = service.submit(request(fresh, "fresh", "Any updates?")).await.unwrap();
    let decision = fixture.next().await;
    assert!(!model_text(&decision.request).contains(public));
    assert!(!model_text(&decision.request).contains("private-answer-sentinel"));
    decision.complete_raw(r#""missingPublicTopic""#);
    assert_eq!(terminal(&mut events, AskTurnKey { conversation_id: fresh, turn_id: id }).await, AskTurnStatus::Completed);
    assert!(fixture.calls.try_recv().is_err());
    assert_eq!(fixture.search.0.lock().len(), 2);
}

#[tokio::test]
async fn search_disabled_rejected_and_unreadable_evidence_are_distinct() {
    for (enabled, decision_rejected, result, expected) in [
        (false, false, None, AskSearchStatus::Disabled),
        (true, true, None, AskSearchStatus::Rejected),
        (true, false, Some(Ok(AskSearchResult::NoResults)), AskSearchStatus::NoResults),
        (true, false, Some(Ok(AskSearchResult::Done { evidence: "[]".into() })), AskSearchStatus::NoResults),
        (true, false, Some(Ok(AskSearchResult::SnippetOnly)), AskSearchStatus::SnippetOnly),
        (true, false, Some(Err(ask_error(BackendErrorCode::Provider, "fixture_search_failed"))), AskSearchStatus::Error),
    ] {
        let mut fixture = Fixture::new(32_768);
        fixture.search.1.store(enabled, std::sync::atomic::Ordering::Relaxed);
        *fixture.search.2.lock() = result;
        let service = fixture.service.clone();
        let conversation_id = service.open_transient();
        let mut events = service.subscribe();
        let turn_id = service.submit(request(conversation_id, "news", "Search for today’s transit news in Berlin.")).await.unwrap();
        let key = AskTurnKey { conversation_id, turn_id };
        if enabled {
            let plan = fixture.next().await;
            assert!(plan.request.tools.available.search);
            if decision_rejected {
                plan.complete_raw("tool refused");
            } else {
                plan.tool(AskToolCall::WebSearch { public_query: "Berlin transit latest official news".into() });
            }
        }
        if !enabled || decision_rejected {
            let answer = fixture.next().await;
            assert!(!answer.request.tools.available.any());
            assert!(!answer.request.instructions.contains(WEB_SEARCH_GUIDANCE));
            answer.complete_raw("ASK_RESULT needs_public_evidence");
        }
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
        assert_eq!(service.turn(key).unwrap().search_status, expected);
        let snapshot = service.snapshot(conversation_id).unwrap();
        let answer = &snapshot.messages.last().unwrap().text;
        assert_eq!(answer.contains("Web Search is off"), !enabled);
        assert!(!answer.contains("source body") && !answer.contains("ASK_RESULT"));
        assert!(fixture.calls.try_recv().is_err());
    }
}

#[tokio::test]
async fn external_delivery_rejects_mislabeled_meta_and_preserves_questions_in_the_body() {
    for (index, (instruction, body, framed, approval)) in [
        ("帮我写一篇邮件", "你想让我做的是哪一件事？\n\n请确认一下，我再继续。", false, false),
        ("Write an email", "What is the recipient’s name?", true, false),
        ("Make this shorter, please?", "Avant de continuer, confirmez ce que je dois faire.", true, false),
        ("帮我写一篇邮件", "主题：[事项]\n\n您好，您方便在本周沟通[事项]吗？\n\n谢谢！\n[姓名]", true, true),
    ].into_iter().enumerate() {
        let mut fixture = Fixture::new(32_768);
        fixture.review_manually();
        let service = fixture.service.clone();
        let conversation_id = service.open_transient();
        let mut events = service.subscribe();
        let (writer, mut effects) = mpsc::unbounded_channel();
        let key = prepared(&service, conversation_id, instruction, AskPreparation {
            selection: if index == 2 { ExternalSelection::Selected("current selection sentinel".into()) } else { ExternalSelection::None },
            writer: Some(Arc::new(TestWriter(writer))),
            ..Default::default()
        }).await;
        let compose = fixture.next().await;
        assert!(compose.request.instructions.contains(FULFILLMENT_RULE));
        if framed {
            compose.complete(body, None, None);
            let validation = fixture.next().await;
            assert!(validation.request.instructions.contains("写给收件人的提问"));
            assert!(model_text(&validation.request).contains("candidateBody"));
            assert!(!model_text(&validation.request).contains("current selection sentinel"));
            assert!(effects.try_recv().is_err());
            validation.complete_raw(if approval { "deliver" } else { "reject" });
        } else {
            compose.complete_raw(body);
        }
        if approval {
            let effect = effects.recv().await.unwrap();
            assert_eq!(effect.operation, AskWriteOperation::Insert);
            assert_eq!(effect.body, body);
            assert!(effect.claim.claim());
            effect.outcome.send(AskWriteOutcome::Written).unwrap();
            assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
        } else {
            assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
            assert_eq!(service.snapshot(conversation_id).unwrap().submission.unwrap().error.unwrap().message, "askDraftUnusable");
        }
        assert!(effects.try_recv().is_err());
        assert!(fixture.history.lock().is_empty());
    }
}

#[tokio::test]
async fn failed_timed_out_and_cancelled_review_retain_generated_body_without_native_effects() {
    for failure in ["transport", "timeout", "cancel", "reopen"] {
        let mut fixture = Fixture::new(32_768);
        fixture.review_manually();
        let service = fixture.service.clone();
        let conversation_id = service.open_transient();
        let mut events = service.subscribe();
        let (writer, mut effects) = mpsc::unbounded_channel();
        let key = prepared(&service, conversation_id, "Write an email", AskPreparation {
            selection: ExternalSelection::None,
            writer: Some(Arc::new(TestWriter(writer))),
            ..Default::default()
        }).await;
        let body = "Subject: [Subject]\n\nHello,\n\nMay we meet on [date]?\n\n[Name]";
        fixture.next().await.complete(body, None, None);
        let review = fixture.next().await;
        assert_eq!(review.request.instructions, DELIVERY_REVIEW_RULE);
        assert!(effects.try_recv().is_err());
        let snapshot = service.snapshot(conversation_id).unwrap().submission.unwrap();
        assert_eq!(snapshot.delivery, Some(AskDeliveryStatus::Pending));
        assert!(snapshot.result_text.is_none());

        match failure {
            "transport" => {
                assert!(review.finish.send(Err(ask_error(BackendErrorCode::Provider, "fixture_review_disconnected"))).is_ok());
            }
            "timeout" => {
                assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
                assert!(review.finish.is_closed());
            }
            "cancel" => {
                service.cancel(key).unwrap();
                review.raw_delta("deliver");
            }
            "reopen" => {
                let fresh = service.open_transient();
                review.raw_delta("deliver");
                assert!(service.snapshot(conversation_id).is_err());
                let snapshot = service.snapshot(fresh).unwrap();
                assert!(snapshot.messages.is_empty() && snapshot.submission.is_none());
                assert!(effects.try_recv().is_err());
                continue;
            }
            _ => unreachable!(),
        }
        if failure != "timeout" {
            let expected = if failure == "cancel" { AskTurnStatus::Cancelled } else { AskTurnStatus::Failed };
            assert_eq!(terminal(&mut events, key).await, expected);
        }
        let snapshot = service.snapshot(conversation_id).unwrap().submission.unwrap();
        assert_eq!(snapshot.delivery, Some(AskDeliveryStatus::Retained));
        assert_eq!(snapshot.result_text.as_deref(), Some(body));
        if failure != "cancel" {
            assert_eq!(snapshot.error.unwrap().message, "askDraftReviewUnavailable");
        }
        assert!(effects.try_recv().is_err());
        assert!(fixture.history.lock().is_empty());
    }
}

#[tokio::test]
async fn reviewed_chat_streams_safe_prefix_and_retains_attached_turn_on_tail_review_outage() {
    let mut fixture = Fixture::with_capabilities(32_768, true, true);
    fixture.review_manually();
    let service = fixture.service.clone();
    let conversation_id = service.open();
    let mut events = service.subscribe();
    for (index, outage) in [false, true].into_iter().enumerate() {
        let attachment_id = Uuid::new_v4();
        fixture.attachments.0.lock().insert(attachment_id, (conversation_id, AskImage {
            mime: AskImageMime::Png, bytes: Arc::from([1_u8, 2, 3]), width: 1, height: 1,
        }));
        service.publish_attachment(conversation_id, AskAttachmentMetadata {
            attachment_id, kind: AskAttachmentKind::Image, display_name: format!("fixture-{index}.png"),
            state: AskAttachmentStatus::Ready, preview: None, source_app: None, page_count: None, error: None,
        }, true).unwrap();
        let mut request = request(conversation_id, &format!("image-{index}"), "Explain this picture.");
        request.attachment_ids.push(attachment_id);
        let turn_id = service.submit(request).await.unwrap();
        let key = AskTurnKey { conversation_id, turn_id };
        let generation = fixture.next().await;
        assert!(generation.request.messages.last().unwrap().parts.iter().any(|part| matches!(part, AskModelPart::Image(_))));
        let approved_prefix = "The first detail is visible.\n\n";
        let tail = if outage { "Unreviewed synthetic tail." } else { "A second visual detail." };
        let started = std::time::Instant::now();
        generation.delta(approved_prefix);
        let early = fixture.next().await;
        assert_eq!(early.request.instructions, DELIVERY_REVIEW_RULE);
        assert!(early.request.messages.iter().all(|message| message.parts.iter().all(|part| matches!(part, AskModelPart::Text(_)))));
        assert!(service.snapshot(conversation_id).unwrap().messages.last().unwrap().text.is_empty());
        early.complete_raw("deliver");
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let event = events.recv().await.unwrap();
                if event.turn_id == Some(turn_id)
                    && matches!(&event.kind, AskEventKind::AnswerDelta { delta, .. } if delta == approved_prefix)
                {
                    break;
                }
            }
        }).await.unwrap();
        let first_delta = started.elapsed();
        assert!(!generation.finish.is_closed());
        assert_eq!(service.snapshot(conversation_id).unwrap().messages.last().unwrap().text, approved_prefix);
        tokio::time::sleep(Duration::from_millis(75)).await;
        let provider_end = started.elapsed();
        eprintln!("synthetic Ask first approved delta {first_delta:?}; fixture provider finish {provider_end:?}");
        generation.complete(tail, None, None);
        let final_review = fixture.next().await;
        let review_payload = model_text(&final_review.request);
        assert!(review_payload.contains("approvedPrefix") && review_payload.contains(tail));
        assert_eq!(service.snapshot(conversation_id).unwrap().messages.last().unwrap().text, approved_prefix);
        if outage {
            assert!(final_review.finish.send(Err(ask_error(BackendErrorCode::Provider, "fixture_review_disconnected"))).is_ok());
            assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
            let state = service.snapshot(conversation_id).unwrap();
            let current = state.submission.unwrap();
            assert!(current.delivery.is_none());
            assert_eq!(current.error.unwrap().message, "askAnswerReviewUnavailable");
            assert_eq!(current.result_text, Some(format!("{approved_prefix}{tail}")));
            assert_eq!(state.attachments[0].attachment_id, attachment_id);
            assert_eq!(state.messages.last().unwrap().text, approved_prefix);
        } else {
            final_review.complete_raw("deliver");
            assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
            let state = service.snapshot(conversation_id).unwrap();
            assert_eq!(state.messages.last().unwrap().text, format!("{approved_prefix}{tail}"));
            assert!(state.attachments.is_empty() && state.submission.unwrap().result_text.is_none());
        }
    }
    assert_eq!(fixture.history.lock().len(), 1);
    assert!(!fixture.history.lock()[0].1.contains("Unreviewed synthetic"));
    let next = service.submit(request(conversation_id, "third", "An unrelated follow-up.")).await.unwrap();
    let answer = fixture.next().await;
    let payload = model_text(&answer.request);
    assert!(payload.contains("A second visual detail.") && !payload.contains("Unreviewed synthetic tail."));
    assert!(answer.request.messages.iter().all(|message| message.parts.iter().all(|part| matches!(part, AskModelPart::Text(_)))));
    answer.complete("A new synthetic answer.", None, None);
    fixture.next().await.complete_raw("deliver");
    assert_eq!(terminal(&mut events, AskTurnKey { conversation_id, turn_id: next }).await, AskTurnStatus::Completed);
}

#[tokio::test]
async fn reopened_chat_does_not_publish_a_late_prefix_review_or_tail() {
    let mut fixture = Fixture::new(32_768);
    fixture.review_manually();
    let service = fixture.service.clone();
    let old = service.open_transient();
    service.submit(request(old, "old", "Old question.")).await.unwrap();
    let generation = fixture.next().await;
    generation.delta("Old preview candidate.\n\n");
    let review = fixture.next().await;
    let fresh = service.open_transient();
    review.raw_delta("deliver");
    let _ = review.finish.send(Ok(AskModelCompletion::Complete));
    generation.delta("Late old tail.");
    let _ = generation.finish.send(Ok(AskModelCompletion::Complete));
    let mut events = service.subscribe();
    let key = AskTurnKey {
        conversation_id: fresh,
        turn_id: service.submit(request(fresh, "new", "Fresh question.")).await.unwrap(),
    };
    let answer = fixture.next().await;
    assert!(!model_text(&answer.request).contains("Old preview"));
    answer.complete("Fresh result.", None, None);
    fixture.next().await.complete_raw("deliver");
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    assert_eq!(service.snapshot(fresh).unwrap().messages.last().unwrap().text, "Fresh result.");
    assert!(fixture.history.lock().is_empty());
}
