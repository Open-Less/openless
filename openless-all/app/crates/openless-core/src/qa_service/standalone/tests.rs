use super::*;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

struct TestModel {
    label: AskModelLabel,
    caps: AskModelCapabilities,
    calls: mpsc::UnboundedSender<Call>,
    intent_tool: bool,
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
            let (finish, receiver) = oneshot::channel();
            assert!(self
                .calls
                .send(Call {
                    request,
                    progress,
                    finish,
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
    assistants: mpsc::UnboundedReceiver<AssistantCall>,
    search: Arc<Search>,
    attachments: Arc<Attachments>,
    history: Arc<Mutex<Vec<(String, String)>>>,
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
        let (assistant, assistants) = mpsc::unbounded_channel();
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
        });
        let service = StandaloneQaService::new(StandaloneQaDependencies {
            assistant: Arc::new(TestAssistant(assistant)),
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
            assistants,
            search,
            attachments,
            history,
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

struct WriteAttempt {
    operation: AskWriteOperation,
    body: String,
    claim: AskWriteClaim,
    outcome: oneshot::Sender<AskWriteOutcome>,
}

struct TestWriter(mpsc::UnboundedSender<WriteAttempt>);

impl AskWriter for TestWriter {
    fn has_original_caret(&self) -> bool {
        true
    }

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

struct TestAssistant(mpsc::UnboundedSender<AssistantCall>);
struct AssistantCall {
    request: AssistantRequest,
    events: crate::assistant_api::AssistantEventSink,
    cancellation: AskCancellation,
    finish: oneshot::Sender<Result<crate::assistant_api::AssistantResponse, BackendError>>,
}
impl crate::assistant_api::AssistantRuntime for TestAssistant {
    fn run(
        &self,
        request: AssistantRequest,
        cancellation: AskCancellation,
        events: crate::assistant_api::AssistantEventSink,
    ) -> BoxFuture<'_, Result<crate::assistant_api::AssistantResponse, BackendError>> {
        Box::pin(async move {
            let (finish, response) = oneshot::channel();
            assert!(self
                .0
                .send(AssistantCall {
                    request,
                    events,
                    cancellation,
                    finish
                })
                .is_ok());
            response.await.unwrap_or_else(|_| Err(cancelled()))
        })
    }
}
impl AssistantCall {
    fn complete(self, text: &str) {
        assert!(self
            .finish
            .send(Ok(crate::assistant_api::AssistantResponse {
                text: text.into()
            }))
            .is_ok());
    }
}
impl Fixture {
    async fn assistant(&mut self) -> AssistantCall {
        tokio::time::timeout(Duration::from_secs(3), self.assistants.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn route(&mut self, route: AskRoute) {
        let call = self.next().await;
        assert!(call.request.instructions.contains("Classify"));
        assert!(!call.request.tools.available.search);
        assert!(!call.request.tools.available.calculator);
        if call.request.tools.available.resolve_intent {
            call.tool(AskToolCall::ResolveIntent {
                route,
                edit_target: (route == AskRoute::Edit).then_some(AskEditTarget::CurrentSelection),
            });
        } else {
            call.complete_raw(&serde_json::json!({"route":route,"editTarget":(route == AskRoute::Edit).then_some(AskEditTarget::CurrentSelection)}).to_string());
        }
    }
    fn no_more_calls(&mut self) {
        assert!(
            self.calls.try_recv().is_err(),
            "unexpected model/review call"
        );
        assert!(
            self.assistants.try_recv().is_err(),
            "unexpected assistant call"
        );
        assert!(self.search.0.lock().is_empty(), "old chat search path ran");
    }
}
fn setup_writer() -> (Arc<TestWriter>, mpsc::UnboundedReceiver<WriteAttempt>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Arc::new(TestWriter(sender)), receiver)
}
async fn attempt(receiver: &mut mpsc::UnboundedReceiver<WriteAttempt>) -> WriteAttempt {
    tokio::time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .unwrap()
        .unwrap()
}
fn usage(call_id: Uuid, input: u64) -> AskCallUsage {
    AskCallUsage {
        call_id,
        purpose: AskCallPurpose::Answer,
        model: AskModelLabel {
            service: "assistant".into(),
            channel: "primary".into(),
            model: "model".into(),
            protocol: "responses".into(),
        },
        input: Some(input),
        output: Some(5),
        cached_input: Some(10),
        cache_write_input: Some(0),
        reasoning: Some(1),
        input_accounting: Some(CacheInputAccounting::Inclusive),
    }
}

#[tokio::test]
async fn every_submission_routes_after_chat_and_isolates_a_b_writes() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let (writer, mut writes) = setup_writer();
    let cases = [
        ("日本首都是哪里？", AskRoute::Answer),
        ("这段是什么意思？", AskRoute::SelectionAnswer),
        ("帮我写一封道歉邮件", AskRoute::Write),
        ("能把这段改短吗", AskRoute::Edit),
    ];
    for (index, (instruction, route)) in cases.into_iter().enumerate() {
        let key = prepared(
            &f.service,
            id,
            instruction,
            AskPreparation {
                selection: ExternalSelection::Selected(format!("verified B {index}")),
                writer: Some(writer.clone()),
                ..Default::default()
            },
        )
        .await;
        f.route(route).await;
        if route.is_chat() {
            let call = f.assistant().await;
            assert_eq!(call.request.key, key);
            assert_eq!(call.request.client_request_id, instruction);
            assert_eq!(call.request.text, instruction);
            if route == AskRoute::SelectionAnswer {
                assert_eq!(
                    call.request.selection.as_ref().unwrap().text,
                    format!("verified B {index}")
                );
            } else {
                assert!(
                    call.request.selection.is_none(),
                    "unrelated selection leaked into an ordinary answer"
                );
            }
            assert_eq!(call.request.messages.len(), index * 2);
            call.complete("direct assistant answer");
            assert!(writes.try_recv().is_err());
        } else {
            let call = f.next().await;
            assert!(!call.request.tools.available.any());
            assert!(!call.request.instructions.contains("ASK_RESULT"));
            call.complete("final body", Some(50), Some(10));
            let write = attempt(&mut writes).await;
            assert_eq!(
                write.operation,
                if route == AskRoute::Write {
                    AskWriteOperation::Insert
                } else {
                    AskWriteOperation::Replace
                }
            );
            assert_eq!(write.body, "final body");
            assert!((write.claim.still_current())());
            assert!(write.claim.claim());
            assert!(write.outcome.send(AskWriteOutcome::Written).is_ok());
        }
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
        let snapshot = f.service.snapshot(id).unwrap();
        assert_eq!(snapshot.submission.as_ref().unwrap().route, Some(route));
        assert_eq!(
            snapshot.submission.as_ref().unwrap().client_request_id,
            instruction
        );
        if !route.is_chat() {
            assert_no_chat(&f.service, key);
        }
        f.no_more_calls();
    }
    assert_eq!(f.history.lock().len(), 2);
}

#[tokio::test]
async fn semantic_hybrid_uses_a_semantic_call_instead_of_keyword_shortcuts() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    for (text, route) in [
        ("能把这段改短吗", AskRoute::Edit),
        ("日本首都是哪里？", AskRoute::Answer),
    ] {
        let key = prepared(
            &f.service,
            id,
            text,
            AskPreparation {
                intent_mode: AskIntentMode::SemanticHybrid,
                selection: ExternalSelection::Selected("unrelated selection".into()),
                ..Default::default()
            },
        )
        .await;
        let classify = f.next().await;
        assert!(model_text(&classify.request).contains(text));
        assert!(!classify.request.tools.available.resolve_intent);
        classify.complete_raw(&serde_json::json!({"route":route,"editTarget":(route == AskRoute::Edit).then_some(AskEditTarget::CurrentSelection)}).to_string());
        if route == AskRoute::Edit {
            f.next().await.complete_raw("short text");
        } else {
            f.assistant().await.complete("Tokyo");
        }
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
        f.no_more_calls();
    }
}

#[tokio::test]
async fn main_model_tool_then_one_composition_and_no_review() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let key = prepared(
        &f.service,
        id,
        "write a reply",
        AskPreparation {
            intent_mode: AskIntentMode::MainModelTool,
            selection: ExternalSelection::None,
            ..Default::default()
        },
    )
    .await;
    let routing = f.next().await;
    assert!(routing.request.tools.available.resolve_intent);
    routing.tool(AskToolCall::ResolveIntent {
        route: AskRoute::Write,
        edit_target: None,
    });
    let composition = f.next().await;
    assert!(composition.request.tools.intent.is_some());
    assert!(!composition.request.tools.available.any());
    composition.complete_raw(
        "Can we meet tomorrow?

Thanks!",
    );
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    let submission = f.service.snapshot(id).unwrap().submission.unwrap();
    assert_eq!(
        submission.result_text.as_deref(),
        Some(
            "Can we meet tomorrow?

Thanks!"
        )
    );
    assert_eq!(submission.delivery, Some(AskDeliveryStatus::Retained));
    f.no_more_calls();
}

#[tokio::test]
async fn invalid_or_unsupported_intent_never_switches_modes() {
    assert_eq!(AskIntentMode::default(), AskIntentMode::SeparateModel);
    assert!(serde_json::from_str::<AskIntentMode>("\"unknown\"").is_err());
    for (supported, output) in [(false, None), (true, Some("{\"route\":\"write\"}"))] {
        let mut f = Fixture::with_intent_tool(32_000, supported);
        let id = f.service.open();
        let mut events = f.service.subscribe();
        let key = prepared(
            &f.service,
            id,
            "write a memo",
            AskPreparation {
                intent_mode: AskIntentMode::MainModelTool,
                ..Default::default()
            },
        )
        .await;
        if let Some(output) = output {
            f.next().await.complete_raw(output);
        }
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
        assert_eq!(
            f.service.turn(key).unwrap().error.unwrap().message,
            if supported {
                "askIntentInvalid"
            } else {
                "askIntentToolUnsupported"
            }
        );
        f.no_more_calls();
    }
    for output in [
        "not json",
        "{\"route\":\"other\"}",
        "{\"route\":\"answer\",\"extra\":1}",
    ] {
        let mut f = Fixture::new(32_000);
        let id = f.service.open();
        let mut events = f.service.subscribe();
        let key = prepared(&f.service, id, "request", AskPreparation::default()).await;
        f.next().await.complete_raw(output);
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
        assert_eq!(
            f.service.turn(key).unwrap().error.unwrap().message,
            "askIntentInvalid"
        );
        f.no_more_calls();
    }
}

#[tokio::test]
async fn missing_selection_and_unbound_voice_clarification_never_invoke_writer() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let (writer, mut writes) = setup_writer();
    for (index, route) in [AskRoute::Edit, AskRoute::SelectionAnswer, AskRoute::Clarify]
        .into_iter()
        .enumerate()
    {
        let key = prepared(
            &f.service,
            id,
            &format!("刚才那句 {index}"),
            AskPreparation {
                selection: ExternalSelection::Unavailable,
                writer: Some(writer.clone()),
                recent_voice: vec![AskVoiceContext {
                    text: "previous utterance".into(),
                    recorded_at: "2026-11-21T09:00:00-07:00".into(),
                    matches_selection: false,
                }],
                ..Default::default()
            },
        )
        .await;
        let classify = f.next().await;
        assert!(classify
            .request
            .instructions
            .contains("matchesSelection=true"));
        assert!(model_text(&classify.request).contains("2026-11-21"));
        classify.complete_raw(&serde_json::json!({"route":route,"editTarget":(route == AskRoute::Edit).then_some(AskEditTarget::RecentVoice)}).to_string());
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
        assert_eq!(
            f.service.snapshot(id).unwrap().submission.unwrap().route,
            Some(AskRoute::Clarify)
        );
        assert!(writes.try_recv().is_err());
        f.no_more_calls();
    }
}

#[tokio::test]
async fn a_recent_voice_edit_requires_a_verified_binding_even_with_another_selection() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let (writer, mut writes) = setup_writer();
    for (index, bound) in [false, true].into_iter().enumerate() {
        let key = prepared(
            &f.service,
            id,
            &format!("修改刚才那句 {index}"),
            AskPreparation {
                selection: ExternalSelection::Selected("verified B text".into()),
                writer: Some(writer.clone()),
                recent_voice: vec![AskVoiceContext {
                    text: "recent spoken text".into(),
                    recorded_at: "2026-11-21T09:00:00+08:00".into(),
                    matches_selection: bound,
                }],
                ..Default::default()
            },
        )
        .await;
        f.next()
            .await
            .complete_raw(r#"{"route":"edit","editTarget":"recentVoice"}"#);
        if bound {
            f.next().await.complete_raw("replacement");
            let write = attempt(&mut writes).await;
            assert_eq!(write.operation, AskWriteOperation::Replace);
            assert!(write.claim.claim());
            write.outcome.send(AskWriteOutcome::Written).unwrap();
            assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
        } else {
            assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
            assert_eq!(
                f.service.snapshot(id).unwrap().submission.unwrap().route,
                Some(AskRoute::Clarify)
            );
            assert!(writes.try_recv().is_err());
        }
        f.no_more_calls();
    }
}

#[tokio::test]
async fn composition_context_uses_only_its_target_and_authorized_voice_text() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    for route in [AskRoute::Write, AskRoute::Edit] {
        let text = format!("compose {route:?}");
        let key = prepared(
            &f.service,
            id,
            &text,
            AskPreparation {
                selection: ExternalSelection::Selected("verified selection B".into()),
                original_context: AskCompositionContext {
                    app_purpose: Some("email composition A".into()),
                    before: Some("caret before A".into()),
                    after: Some("caret after A".into()),
                    ..Default::default()
                },
                selection_context: AskCompositionContext {
                    app_purpose: Some("document editing B".into()),
                    before: Some("range before B".into()),
                    after: Some("range after B".into()),
                    ..Default::default()
                },
                recent_voice: vec![AskVoiceContext {
                    text: "relevant voice".into(),
                    recorded_at: "2026-11-21T10:01:02-07:00".into(),
                    matches_selection: true,
                }],
                ..Default::default()
            },
        )
        .await;
        f.route(route).await;
        let call = f.next().await;
        let text = model_text(&call.request);
        assert!(text.contains("relevant voice") && text.contains("2026-11-21T10:01:02"));
        assert_eq!(text.contains("caret before A"), route == AskRoute::Write);
        assert_eq!(text.contains("range before B"), route == AskRoute::Edit);
        assert_eq!(
            text.contains("verified selection B"),
            route == AskRoute::Edit
        );
        call.complete_raw("body");
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
        f.no_more_calls();
    }
}

#[tokio::test]
async fn target_expiry_retains_exact_body_and_reports_clipboard_failure() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let (writer, mut writes) = setup_writer();
    let clipboard = clipboard(&f.service, id, true);
    let key = prepared(
        &f.service,
        id,
        "edit this",
        AskPreparation {
            selection: ExternalSelection::Selected("B".into()),
            writer: Some(writer),
            clipboard: Some(clipboard.clone()),
            ..Default::default()
        },
    )
    .await;
    f.route(AskRoute::Edit).await;
    f.next().await.complete_raw("  final body\n");
    let write = attempt(&mut writes).await;
    assert!(write.outcome.send(AskWriteOutcome::NotWritten).is_ok());
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    let submission = f.service.snapshot(id).unwrap().submission.unwrap();
    assert_eq!(submission.delivery, Some(AskDeliveryStatus::Retained));
    assert_eq!(submission.result_text.as_deref(), Some("  final body\n"));
    assert_eq!(submission.copy_status, AskCopyStatus::Failed);
    assert_eq!(&*clipboard.copies.lock(), &["  final body\n"]);
    f.no_more_calls();
}

#[tokio::test]
async fn cancellation_during_generation_drops_late_body_and_never_writes() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let (writer, mut writes) = setup_writer();
    let key = prepared(
        &f.service,
        id,
        "edit selection",
        AskPreparation {
            selection: ExternalSelection::Selected("B".into()),
            writer: Some(writer),
            ..Default::default()
        },
    )
    .await;
    f.route(AskRoute::Edit).await;
    let call = f.next().await;
    f.service.cancel(key).unwrap();
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Cancelled);
    let sequence = f.service.snapshot(id).unwrap().sequence;
    call.raw_delta("late replacement");
    let _ = call.finish.send(Ok(AskModelCompletion::Complete));
    tokio::task::yield_now().await;
    assert!(writes.try_recv().is_err());
    assert_eq!(f.service.snapshot(id).unwrap().sequence, sequence);
}

#[tokio::test]
async fn cancellation_before_claim_retains_generated_body_and_revokes_writer() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let (writer, mut writes) = setup_writer();
    let key = prepared(
        &f.service,
        id,
        "write a body",
        AskPreparation {
            selection: ExternalSelection::None,
            writer: Some(writer),
            ..Default::default()
        },
    )
    .await;
    f.route(AskRoute::Write).await;
    f.next().await.complete_raw("generated body");
    let write = attempt(&mut writes).await;
    f.service.cancel(key).unwrap();
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Cancelled);
    assert!(!(write.claim.still_current())());
    assert!(!write.claim.claim());
    assert!(write.outcome.send(AskWriteOutcome::Declined).is_ok());
    tokio::task::yield_now().await;
    let snapshot = f.service.snapshot(id).unwrap();
    let submission = snapshot.submission.unwrap();
    assert_eq!(submission.result_text.as_deref(), Some("generated body"));
    assert_eq!(submission.delivery, Some(AskDeliveryStatus::Retained));
    assert_eq!(submission.status, AskTurnStatus::Cancelled);
}

#[tokio::test]
async fn history_off_keeps_live_messages_and_current_validated_attachments() {
    let mut f = Fixture::new(32_000);
    Arc::get_mut(&mut f.service.0).unwrap().deps.history = None;
    let id = f.service.open_transient();
    let mut events = f.service.subscribe();
    let first = prepared(
        &f.service,
        id,
        "Remember the name Ada",
        AskPreparation::default(),
    )
    .await;
    f.route(AskRoute::Answer).await;
    f.assistant().await.complete("Ada noted");
    assert_eq!(terminal(&mut events, first).await, AskTurnStatus::Completed);
    assert!(f.history.lock().is_empty());
    let attachment_id = Uuid::new_v4();
    f.attachments
        .1
        .lock()
        .insert(attachment_id, (id, Arc::from("validated attachment")));
    let mut input = request(id, "followup", "What was the name?");
    input.attachment_ids.push(attachment_id);
    let turn_id = f
        .service
        .submit_prepared(input, || async { Ok(AskPreparation::default()) })
        .await
        .unwrap();
    let key = AskTurnKey {
        conversation_id: id,
        turn_id,
    };
    f.route(AskRoute::Answer).await;
    let call = f.assistant().await;
    assert_eq!(
        call.request
            .messages
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>(),
        ["Remember the name Ada", "Ada noted"]
    );
    assert_eq!(call.request.client_request_id, "followup");
    assert!(
        matches!(&call.request.attachments[..], [AskAttachment::Selection { text }] if &**text == "validated attachment")
    );
    call.complete("Ada");
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    assert!(f.history.lock().is_empty());
}

#[tokio::test]
async fn assistant_failure_has_no_legacy_fallback_and_keeps_attachments() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let attachment_id = Uuid::new_v4();
    f.attachments
        .1
        .lock()
        .insert(attachment_id, (id, Arc::from("attachment")));
    f.service
        .publish_attachment(
            id,
            AskAttachmentMetadata {
                attachment_id,
                kind: AskAttachmentKind::Selection,
                display_name: "selection".into(),
                preview: None,
                page_count: None,
                source_app: None,
                state: AskAttachmentStatus::Ready,
                error: None,
            },
            true,
        )
        .unwrap();
    let mut input = request(id, "failure", "question");
    input.attachment_ids.push(attachment_id);
    let turn_id = f.service.submit(input).await.unwrap();
    let key = AskTurnKey {
        conversation_id: id,
        turn_id,
    };
    f.route(AskRoute::Answer).await;
    let call = f.assistant().await;
    assert!(call
        .finish
        .send(Err(ask_error(BackendErrorCode::Provider, "fixtureFailure")))
        .is_ok());
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Failed);
    assert_eq!(f.service.snapshot(id).unwrap().attachments.len(), 1);
    assert!(f.history.lock().is_empty());
    f.no_more_calls();
}

struct NoCaretWriter;

impl AskWriter for NoCaretWriter {
    fn write(
        &self,
        _: AskWriteOperation,
        _: String,
        _: AskWriteClaim,
    ) -> BoxFuture<'_, AskWriteOutcome> {
        panic!("an unbound A caret must never be used for insertion")
    }
}

#[tokio::test]
async fn missing_original_caret_never_inserts_into_b_or_current_focus() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let key = prepared(
        &f.service,
        id,
        "write new content",
        AskPreparation {
            selection: ExternalSelection::Selected("B must remain unchanged".into()),
            writer: Some(Arc::new(NoCaretWriter)),
            ..Default::default()
        },
    )
    .await;
    f.route(AskRoute::Write).await;
    f.next().await.complete_raw("new body for A");
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    let submission = f.service.snapshot(id).unwrap().submission.unwrap();
    assert_eq!(submission.delivery, Some(AskDeliveryStatus::Retained));
    assert_eq!(submission.result_text.as_deref(), Some("new body for A"));
    f.no_more_calls();
}

#[tokio::test]
async fn idempotency_preserves_request_turn_event_ownership_and_one_preparation() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let input = request(id, "same-request", "question");
    let turn_id = f
        .service
        .submit_prepared(input.clone(), || async { Ok(AskPreparation::default()) })
        .await
        .unwrap();
    let key = AskTurnKey {
        conversation_id: id,
        turn_id,
    };
    let duplicate = f
        .service
        .submit_prepared(input.clone(), || async {
            panic!("duplicate recaptured targets")
        })
        .await
        .unwrap();
    assert_eq!(turn_id, duplicate);
    let mut conflict = input.clone();
    conflict.text = "changed".into();
    assert_eq!(
        f.service.submit(conflict).await.unwrap_err().message,
        "askRequestConflict"
    );
    assert_eq!(
        f.service
            .submit(request(id, "busy", "other"))
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::Busy
    );
    f.route(AskRoute::Answer).await;
    f.assistant().await.complete("answer");
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    assert_eq!(f.service.submit(input).await.unwrap(), turn_id);
    let snapshot = f.service.snapshot(id).unwrap();
    assert_eq!(snapshot.turns[0].client_request_id, "same-request");
    let replay = f.service.events(id, 0).unwrap();
    for event in replay.events {
        assert_eq!(event.conversation_id, id);
        assert_eq!(event.turn_id, Some(turn_id));
        assert_eq!(event.client_request_id.as_deref(), Some("same-request"));
    }
    f.no_more_calls();
}

#[tokio::test]
async fn tools_usage_and_authoritative_response_survive_truncated_replay() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let key = prepared(
        &f.service,
        id,
        "answer with a tool",
        AskPreparation::default(),
    )
    .await;
    f.route(AskRoute::Answer).await;
    let call = f.assistant().await;
    assert_eq!(call.request.key, key);
    let sink = call.events.clone();
    let tool_call_id = Uuid::new_v4();
    sink(AssistantEvent::Tool {
        tool_call_id,
        name: "read_context".into(),
        status: AssistantToolStatus::Running,
        error: None,
    });
    for _ in 0..EVENT_LIMIT + 2 {
        sink(AssistantEvent::TextDelta("x".into()));
    }
    sink(AssistantEvent::Tool {
        tool_call_id,
        name: "read_context".into(),
        status: AssistantToolStatus::Completed,
        error: None,
    });
    let first_call = Uuid::new_v4();
    let second_call = Uuid::new_v4();
    sink(AssistantEvent::Usage(usage(first_call, 100)));
    sink(AssistantEvent::Usage(usage(first_call, 120)));
    sink(AssistantEvent::Usage(usage(second_call, 80)));
    call.complete("authoritative final answer without a marker");
    // The event queue intentionally overflowed; replay/snapshot are the recovery path.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match events.recv().await {
                Ok(event) if matches!(event.kind, AskEventKind::Terminal { .. }) => break,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(error) => panic!("{error}"),
            }
        }
    })
    .await
    .unwrap();
    let snapshot = f.service.snapshot(id).unwrap();
    assert_eq!(
        snapshot.messages[1].text,
        "authoritative final answer without a marker"
    );
    assert_eq!(snapshot.turns[0].tools.len(), 1);
    assert!(matches!(
        snapshot.turns[0].tools[0].status,
        AssistantToolStatus::Completed
    ));
    assert_eq!(
        snapshot.submission.unwrap().tools[0].tool_call_id,
        tool_call_id
    );
    assert_eq!(snapshot.turns[0].usage.len(), 2);
    assert_eq!(snapshot.turns[0].usage[0].input, Some(120));
    let replay = f.service.events(id, 0).unwrap();
    assert!(replay.truncated);
    assert!(replay.events.iter().any(|event| matches!(&event.kind, AskEventKind::Terminal { text: Some(text), .. } if text == "authoritative final answer without a marker")));
    let sequence = snapshot.sequence;
    sink(AssistantEvent::TextDelta("late".into()));
    sink(AssistantEvent::Usage(usage(Uuid::new_v4(), 300)));
    assert_eq!(f.service.snapshot(id).unwrap().sequence, sequence);
    f.no_more_calls();
}

#[tokio::test]
async fn cancelled_assistant_closes_tools_and_drops_late_events_in_a_new_turn() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let old = prepared(&f.service, id, "old question", AskPreparation::default()).await;
    f.route(AskRoute::Answer).await;
    let old_call = f.assistant().await;
    let tool_call_id = Uuid::new_v4();
    (old_call.events)(AssistantEvent::Tool {
        tool_call_id,
        name: "read_page".into(),
        status: AssistantToolStatus::Running,
        error: None,
    });
    f.service.cancel(old).unwrap();
    assert!(old_call.cancellation.is_cancelled());
    assert_eq!(terminal(&mut events, old).await, AskTurnStatus::Cancelled);
    assert!(matches!(
        f.service.turn(old).unwrap().tools[0].status,
        AssistantToolStatus::Cancelled
    ));
    let next = prepared(&f.service, id, "new question", AskPreparation::default()).await;
    f.route(AskRoute::Answer).await;
    let next_call = f.assistant().await;
    let sequence = f.service.snapshot(id).unwrap().sequence;
    (old_call.events)(AssistantEvent::TextDelta("late answer".into()));
    (old_call.events)(AssistantEvent::Tool {
        tool_call_id,
        name: "read_page".into(),
        status: AssistantToolStatus::Completed,
        error: None,
    });
    let _ = old_call
        .finish
        .send(Ok(crate::assistant_api::AssistantResponse {
            text: "late".into(),
        }));
    f.service.cancel(old).unwrap();
    assert_eq!(f.service.snapshot(id).unwrap().sequence, sequence);
    assert!(!next_call.cancellation.is_cancelled());
    next_call.complete("fresh");
    assert_eq!(terminal(&mut events, next).await, AskTurnStatus::Completed);
}

#[tokio::test]
async fn opening_fence_revokes_preparation_and_inflight_assistant() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let fence = AskSessionFence::default();
    let permit = fence.issue();
    let service = f.service.clone();
    let pending = service.submit_prepared_guarded(
        request(id, "pending", "question"),
        || async { std::future::pending::<Result<AskPreparation, BackendError>>().await },
        || true,
        permit,
    );
    tokio::pin!(pending);
    tokio::select! { result = &mut pending => panic!("unexpected result {result:?}"), _ = tokio::task::yield_now() => {} }
    fence.revoke();
    assert_eq!(pending.await.unwrap_err().code, BackendErrorCode::Cancelled);
    assert!(f.service.snapshot(id).unwrap().turns.is_empty());
    let key = prepared(&f.service, id, "running", AskPreparation::default()).await;
    f.route(AskRoute::Answer).await;
    let call = f.assistant().await;
    f.service.end_conversation(id).unwrap();
    assert!(call.cancellation.is_cancelled());
    (call.events)(AssistantEvent::TextDelta("late".into()));
    assert!(f.service.snapshot(key.conversation_id).is_err());
}

#[tokio::test]
async fn invalid_attachments_and_unsupported_thinking_reject_before_acceptance() {
    let mut f = Fixture::new(32_000);
    let id = f.service.open();
    let foreign = Uuid::new_v4();
    let image_id = Uuid::new_v4();
    f.attachments.0.lock().insert(
        image_id,
        (
            foreign,
            AskImage {
                mime: AskImageMime::Png,
                bytes: Arc::from([1u8]),
                width: 1,
                height: 1,
            },
        ),
    );
    let mut input = request(id, "image", "describe");
    input.attachment_ids.push(image_id);
    assert_eq!(
        f.service.submit(input.clone()).await.unwrap_err().code,
        BackendErrorCode::PermissionDenied
    );
    f.attachments.0.lock().get_mut(&image_id).unwrap().0 = id;
    assert_eq!(
        f.service.submit(input).await.unwrap_err().message,
        "askImageUnsupported"
    );
    let mut input = request(id, "thinking", "question");
    input.options.deep_thinking = true;
    assert_eq!(
        f.service.submit(input).await.unwrap_err().message,
        "askDeepThinkingUnsupported"
    );
    assert!(f.service.snapshot(id).unwrap().submission.is_none());
    f.no_more_calls();
}

#[test]
fn composer_budget_does_not_start_compaction_or_exceed_total_cap() {
    let caps = AskModelCapabilities {
        context_window: Some(1_000_000),
        output_tokens: 256,
        image_input: false,
        image_token_reserve: 0,
        deep_thinking: false,
    };
    assert!(prompt::Material::new(
        &"a".repeat(256_000),
        Vec::new(),
        caps,
        &AskOptions::default()
    )
    .is_err());
    let material =
        prompt::Material::new("write a memo", Vec::new(), caps, &AskOptions::default()).unwrap();
    let request = material
        .compose(
            AskRoute::Write,
            &AskPreparation::default(),
            vec![AssistantMessage {
                role: AssistantRole::User,
                text: "x".repeat(256_000),
            }],
            None,
        )
        .unwrap();
    assert_eq!(request.messages.len(), 1);
    assert!(!request.tools.available.any());
}

#[tokio::test]
async fn routing_and_composition_record_separate_calls_and_merge_partial_usage() {
    let mut f = Fixture::new(32_000);
    let root = std::env::temp_dir().join(format!("openless-entry-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let notifications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let changed = notifications.clone();
    let recorder = Arc::new(
        UsageRecorder::start_with_change_listener(
            Arc::new(crate::usage_ledger::UsageLedger::open(root.join("usage.json")).unwrap()),
            move || {
                changed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
        )
        .unwrap(),
    );
    Arc::get_mut(&mut f.service.0).unwrap().deps.usage = Some(AskUsagePersistence {
        recorder: recorder.clone(),
        clock: Arc::new(crate::SystemClock),
    });
    let id = f.service.open();
    let mut events = f.service.subscribe();
    let key = prepared(&f.service, id, "write a memo", AskPreparation::default()).await;
    // No reported quantities still represents a real, distinct routing call.
    f.route(AskRoute::Write).await;
    let call = f.next().await;
    (call.progress)(AskModelProgress::Usage(ReportedTokens {
        input: Some(123),
        cached_input: Some(23),
        input_accounting: Some(CacheInputAccounting::Inclusive),
        ..Default::default()
    }));
    (call.progress)(AskModelProgress::Usage(ReportedTokens {
        output: Some(7),
        ..Default::default()
    }));
    call.complete_raw("memo");
    assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
    let (records, failures) = recorder.snapshot().unwrap();
    assert_eq!(failures, 0);
    assert_eq!(records.len(), 2);
    assert_ne!(records[0].id, records[1].id);
    let compose = records
        .iter()
        .find(|record| record.purpose == ServicePurpose::Question)
        .unwrap();
    assert_eq!(compose.input_tokens, Some(123));
    assert_eq!(compose.output_tokens, Some(7));
    assert_eq!(compose.cached_input_tokens, Some(23));
    assert_eq!(compose.outcome, ServiceOutcome::Succeeded);
    assert_eq!(notifications.load(std::sync::atomic::Ordering::SeqCst), 2);
    let view = f.service.turn(key).unwrap();
    assert_eq!(view.usage[0].input, Some(123));
    assert_eq!(view.usage[0].output, Some(7));
    drop(f);
    drop(recorder);
    std::fs::remove_dir_all(root).unwrap();
}
