//! Production session transitions with real Core admission and write claims;
//! no Tauri window, native editor, microphone, or provider connection.
#![allow(dead_code)]

#[path = "../../src/ask_window/context.rs"]
mod context;
#[path = "../../src/ask_window/session.rs"]
mod session;

use futures_util::future::BoxFuture;
use openless_core::ask::*;
use openless_core::ask_context::{AppMetadata, ContextAuthorization};
use openless_core::assistant_api::*;
use openless_core::{BackendError, BackendErrorCode, TokioTaskSpawner};
use parking_lot::Mutex;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), future)
        .await
        .expect("session transition must complete")
}

struct Model {
    label: AskModelLabel,
    requests: Mutex<Vec<AskModelRequest>>,
}

impl AskModel for Model {
    fn label(&self) -> &AskModelLabel {
        &self.label
    }

    fn capabilities(&self) -> AskModelCapabilities {
        AskModelCapabilities {
            context_window: Some(32_000),
            output_tokens: 256,
            image_input: false,
            image_token_reserve: 0,
            deep_thinking: false,
        }
    }

    fn stream(
        &self,
        request: AskModelRequest,
        _: AskCancellation,
        progress: AskModelSink,
    ) -> BoxFuture<'_, Result<AskModelCompletion, BackendError>> {
        Box::pin(async move {
            let response = if request.instructions.contains("Classify") {
                let text = match &request.messages[0].parts[0] {
                    AskModelPart::Text(text) => text,
                    _ => panic!("intent input must be text"),
                };
                let input: serde_json::Value = serde_json::from_str(text).unwrap();
                if input["instruction"] == "answer" {
                    r#"{"route":"answer","editTarget":null}"#
                } else {
                    r#"{"route":"write","editTarget":null}"#
                }
            } else {
                "generated body"
            };
            self.requests.lock().push(request);
            progress(AskModelProgress::Text(response.into()));
            Ok(AskModelCompletion::Complete)
        })
    }
}

struct Factory {
    model: Arc<Model>,
    pause: Mutex<Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>>,
}

impl AskModelFactory for Factory {
    fn prepare(&self, _: bool) -> BoxFuture<'_, Result<Arc<dyn AskModel>, BackendError>> {
        Box::pin(async move {
            let pause = self.pause.lock().take();
            if let Some((entered, resume)) = pause {
                entered.send(()).unwrap();
                let _ = resume.await;
            }
            Ok(self.model.clone() as Arc<dyn AskModel>)
        })
    }
}

struct UnusedPorts;

impl SearchTool for UnusedPorts {
    fn search(
        &self,
        _: AskSearchRequest,
        _: AskCancellation,
        _: AskToolSink,
    ) -> BoxFuture<'_, Result<AskSearchResult, BackendError>> {
        panic!("session tests must not search")
    }
}

impl AttachmentReader for UnusedPorts {
    fn acquire(&self, _: AskTurnKey, _: &[Uuid]) -> Result<Vec<AskAttachment>, BackendError> {
        panic!("session tests have no attachments")
    }

    fn end_conversation(&self, _: Uuid) {}
}

impl AssistantRuntime for UnusedPorts {
    fn run(
        &self,
        _: AssistantRequest,
        _: AskCancellation,
        _: AssistantEventSink,
    ) -> BoxFuture<'_, Result<AssistantResponse, BackendError>> {
        Box::pin(async {
            Ok(AssistantResponse {
                text: "saved answer".into(),
            })
        })
    }
}

struct WriteAttempt {
    claim: AskWriteClaim,
    outcome: oneshot::Sender<AskWriteOutcome>,
}

struct Writer(mpsc::UnboundedSender<WriteAttempt>);

impl AskWriter for Writer {
    fn has_original_caret(&self) -> bool {
        true
    }

    fn write(
        &self,
        operation: AskWriteOperation,
        text: String,
        claim: AskWriteClaim,
    ) -> BoxFuture<'_, AskWriteOutcome> {
        Box::pin(async move {
            assert_eq!(operation, AskWriteOperation::Insert);
            assert_eq!(text, "generated body");
            let (outcome, response) = oneshot::channel();
            assert!(self.0.send(WriteAttempt { claim, outcome }).is_ok());
            response.await.unwrap()
        })
    }
}

struct Fixture {
    service: StandaloneQaService,
    fence: AskSessionFence,
    context: Arc<context::DesktopContext>,
    factory: Arc<Factory>,
    history: Arc<Mutex<Vec<(String, String)>>>,
}

impl Fixture {
    fn new() -> Self {
        let context = Arc::new(context::DesktopContext::default());
        context.set_authorization(ContextAuthorization {
            app_metadata: true,
            usage: false,
        });
        let factory = Arc::new(Factory {
            model: Arc::new(Model {
                label: AskModelLabel {
                    service: "fixture".into(),
                    channel: "fixture".into(),
                    model: "fixture".into(),
                    protocol: "fixture".into(),
                },
                requests: Mutex::new(Vec::new()),
            }),
            pause: Mutex::new(None),
        });
        let history = Arc::new(Mutex::new(Vec::new()));
        let saved = history.clone();
        let service = StandaloneQaService::new(StandaloneQaDependencies {
            assistant: Arc::new(UnusedPorts),
            models: factory.clone(),
            context: context.clone(),
            search: Arc::new(UnusedPorts),
            attachments: Arc::new(UnusedPorts),
            spawner: Arc::new(TokioTaskSpawner),
            history: Some(Arc::new(move |question, answer| {
                saved.lock().push((question.into(), answer.into()));
            })),
            usage: None,
        });
        let id = service.open();
        context.open_conversation(id);
        Self {
            service,
            fence: AskSessionFence::default(),
            context,
            factory,
            history,
        }
    }

    fn transition(&self, new_conversation: bool) -> Uuid {
        if new_conversation {
            let id = session::new_persistent_conversation(&self.service, &self.fence);
            self.context.open_conversation(id);
            id
        } else {
            session::revoke_app_context(&self.context, &self.service, &self.fence);
            assert!(!self.context.app_metadata_allowed());
            self.service.current().unwrap()
        }
    }

    async fn submit(&self, text: &str, preparation: AskPreparation) -> AskTurnKey {
        let id = self.service.current().unwrap();
        let turn_id = bounded(self.service.submit_prepared_guarded(
            request(id, text),
            || async { Ok(preparation) },
            || true,
            self.fence.issue(),
        ))
        .await
        .unwrap();
        AskTurnKey {
            conversation_id: id,
            turn_id,
        }
    }
}

fn request(conversation_id: Uuid, text: &str) -> AskSubmit {
    AskSubmit {
        conversation_id,
        client_request_id: Uuid::new_v4().to_string(),
        text: text.into(),
        attachment_ids: Vec::new(),
        options: AskOptions::default(),
    }
}

async fn terminal(
    events: &mut broadcast::Receiver<Arc<AskEvent>>,
    key: AskTurnKey,
) -> AskTurnStatus {
    bounded(async {
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
}

#[tokio::test]
async fn transitions_revoke_prepared_context_while_model_factory_is_paused() {
    for new_conversation in [true, false] {
        let f = Fixture::new();
        let id = f.service.current().unwrap();
        let (entered, waiting) = oneshot::channel();
        let (resume, paused) = oneshot::channel();
        *f.factory.pause.lock() = Some((entered, paused));
        let service = f.service.clone();
        let permit = f.fence.issue();
        let pending = tokio::spawn(async move {
            service
                .submit_prepared_guarded(
                    request(id, "write"),
                    || async {
                        Ok(AskPreparation {
                            original_context: AskCompositionContext {
                                source_app: AppMetadata::new("captured private app", None),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                    },
                    || true,
                    permit,
                )
                .await
        });
        bounded(waiting).await.unwrap();
        assert!(f.service.snapshot(id).unwrap().current_turn_id.is_none());
        f.transition(new_conversation);
        // Cancellation must finish admission even while prepare() is blocked.
        assert_eq!(
            bounded(pending).await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        let _ = resume.send(());
        assert!(
            f.factory.model.requests.lock().is_empty(),
            "cached metadata reached a model"
        );
        assert!(f.service.snapshot(id).unwrap().submission.is_none());
        assert!(
            !f.fence.issue().is_cancelled(),
            "future requests need a fresh permit"
        );
    }
}

#[tokio::test]
async fn transitions_decline_old_unclaimed_writes() {
    for new_conversation in [true, false] {
        let f = Fixture::new();
        let mut events = f.service.subscribe();
        let (tx, mut writes) = mpsc::unbounded_channel();
        let key = f
            .submit(
                "write",
                AskPreparation {
                    writer: Some(Arc::new(Writer(tx))),
                    ..Default::default()
                },
            )
            .await;
        let write = bounded(writes.recv()).await.unwrap();
        assert!((write.claim.still_current())());
        f.transition(new_conversation);
        assert!(!(write.claim.still_current())());
        assert!(
            !write.claim.claim(),
            "an old request retained write authority"
        );
        write.outcome.send(AskWriteOutcome::Declined).unwrap();
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Cancelled);
        let old = f.service.snapshot(key.conversation_id).unwrap();
        assert!(old.current_turn_id.is_none());
        assert_eq!(
            old.submission.unwrap().delivery,
            Some(AskDeliveryStatus::Retained)
        );
    }
}

#[tokio::test]
async fn transitions_preserve_unknown_outcomes_of_already_claimed_writes() {
    for new_conversation in [true, false] {
        let f = Fixture::new();
        let mut events = f.service.subscribe();
        let (tx, mut writes) = mpsc::unbounded_channel();
        let key = f
            .submit(
                "write",
                AskPreparation {
                    writer: Some(Arc::new(Writer(tx))),
                    ..Default::default()
                },
            )
            .await;
        let write = bounded(writes.recv()).await.unwrap();
        assert!(write.claim.claim());
        f.transition(new_conversation);
        assert_eq!(
            f.service
                .snapshot(key.conversation_id)
                .unwrap()
                .current_turn_id,
            Some(key.turn_id)
        );
        write.outcome.send(AskWriteOutcome::Unknown).unwrap();
        assert_eq!(terminal(&mut events, key).await, AskTurnStatus::Completed);
        let old = f.service.snapshot(key.conversation_id).unwrap();
        assert!(old.current_turn_id.is_none());
        let result = old.submission.unwrap();
        assert_eq!(result.delivery, Some(AskDeliveryStatus::Unknown));
        assert_eq!(result.result_text.as_deref(), Some("generated body"));
    }
}

#[tokio::test]
async fn new_persistent_conversation_keeps_completed_dialogue_and_history() {
    let f = Fixture::new();
    let mut events = f.service.subscribe();
    let old = f.submit("answer", AskPreparation::default()).await;
    assert_eq!(terminal(&mut events, old).await, AskTurnStatus::Completed);
    // finish() emits before invoking the history sink; let that same task finish.
    tokio::task::yield_now().await;
    assert_eq!(
        &*f.history.lock(),
        &[("answer".into(), "saved answer".into())]
    );
    let next = f.transition(true);
    assert_ne!(next, old.conversation_id);
    assert_eq!(f.service.current(), Some(next));
    assert!(!f.service.current_is_transient());
    assert_eq!(
        f.service.snapshot(old.conversation_id).unwrap().messages[1].text,
        "saved answer"
    );
    assert!(f.service.snapshot(next).unwrap().turns.is_empty());
    assert_eq!(f.history.lock().len(), 1);
}
