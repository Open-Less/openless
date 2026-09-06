use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::coding_agent::{
    autonomous_prompt, CodingAgentProvider, CodingAgentRequest, CodingAgentRunner,
};
use crate::domains::{
    LessComputerApi, LessComputerRunOutcome, LessComputerRunRequest, LessComputerRunResult,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::{
    BackendEventKind, BackendEventPublisher, CodingAgentStreamEvent, LessComputerEvent,
    LessComputerEventKind,
};
use crate::types::SessionId;

const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_DSH_CONTINUATION_TURNS: usize = 2;
pub(crate) const VOICE_CAPTURE_FAILED: &str = "Less Computer voice input failed. Please try again.";

struct LessComputerState {
    conversation_active: AtomicBool,
    approvals: Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>,
    events: Mutex<Option<BackendEventPublisher>>,
    runner: Mutex<Option<Arc<CodingAgentRunner>>>,
    active_lease: Mutex<Option<ActiveLease>>,
    completed_turns: Mutex<VecDeque<CompletedTurn>>,
    approval_timeout: Duration,
    voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
}

enum ActiveLease {
    Capture(ActiveCapture),
    Run(ActiveRun),
}

struct ActiveCapture {
    session_id: SessionId,
    cancel: Arc<AtomicBool>,
}

struct ActiveRun {
    session_id: SessionId,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct CompletedTurn {
    user: String,
    outcome: LessComputerRunOutcome,
}

/// Core-owned Less Computer state shared by every host adapter for one backend
/// instance. Clones point to the same conversation and approval registry.
#[derive(Clone)]
pub struct LessComputerService {
    state: Arc<LessComputerState>,
}

impl LessComputerService {
    pub fn new() -> Self {
        Self::with_approval_timeout(DEFAULT_APPROVAL_TIMEOUT)
    }

    fn with_approval_timeout(approval_timeout: Duration) -> Self {
        Self::with_voice_sessions_and_timeout(
            Arc::new(crate::voice_session::VoiceSessionGate::default()),
            approval_timeout,
        )
    }

    pub(crate) fn with_voice_sessions(
        voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
    ) -> Self {
        Self::with_voice_sessions_and_timeout(voice_sessions, DEFAULT_APPROVAL_TIMEOUT)
    }

    fn with_voice_sessions_and_timeout(
        voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
        approval_timeout: Duration,
    ) -> Self {
        Self {
            state: Arc::new(LessComputerState {
                conversation_active: AtomicBool::new(false),
                approvals: Mutex::new(HashMap::new()),
                events: Mutex::new(None),
                runner: Mutex::new(None),
                active_lease: Mutex::new(None),
                completed_turns: Mutex::new(VecDeque::new()),
                approval_timeout,
                voice_sessions,
            }),
        }
    }

    fn remove_approval(&self, token: &str) {
        self.state
            .approvals
            .lock()
            .expect("Less Computer approval lock poisoned")
            .remove(token);
    }

    fn runner(&self) -> Result<Arc<CodingAgentRunner>, BackendError> {
        self.state
            .runner
            .lock()
            .expect("Less Computer runner lock poisoned")
            .clone()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Unsupported,
                    "Less Computer runner is not configured",
                )
            })
    }

    fn publisher(&self) -> Result<BackendEventPublisher, BackendError> {
        self.state
            .events
            .lock()
            .expect("Less Computer event lock poisoned")
            .clone()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Less Computer event publisher is not bound",
                )
            })
    }

    fn begin_capture_inner(&self, session_id: SessionId) -> Result<(), BackendError> {
        let mut active = self
            .state
            .active_lease
            .lock()
            .expect("Less Computer active-lease lock poisoned");
        if active.is_some() {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "Less Computer is already running",
            ));
        }
        self.state.voice_sessions.acquire(
            session_id,
            crate::voice_session::VoiceSessionKind::LessComputer,
        )?;
        *active = Some(ActiveLease::Capture(ActiveCapture {
            session_id,
            cancel: Arc::new(AtomicBool::new(false)),
        }));
        Ok(())
    }

    fn promote_capture_or_start_run(
        &self,
        session_id: SessionId,
    ) -> Result<Arc<AtomicBool>, BackendError> {
        let mut active = self
            .state
            .active_lease
            .lock()
            .expect("Less Computer active-lease lock poisoned");
        match active.as_ref() {
            None => {
                self.state.voice_sessions.acquire(
                    session_id,
                    crate::voice_session::VoiceSessionKind::LessComputer,
                )?;
                let cancel = Arc::new(AtomicBool::new(false));
                *active = Some(ActiveLease::Run(ActiveRun {
                    session_id,
                    cancel: Arc::clone(&cancel),
                }));
                Ok(cancel)
            }
            Some(ActiveLease::Capture(capture)) if capture.session_id == session_id => {
                let cancel = Arc::clone(&capture.cancel);
                self.state.voice_sessions.acquire(
                    session_id,
                    crate::voice_session::VoiceSessionKind::LessComputer,
                )?;
                *active = Some(ActiveLease::Run(ActiveRun {
                    session_id,
                    cancel: Arc::clone(&cancel),
                }));
                Ok(cancel)
            }
            Some(_) => Err(BackendError::new(
                BackendErrorCode::Busy,
                "Less Computer is already running",
            )),
        }
    }

    fn clear_active_lease(&self, session_id: SessionId) {
        let mut active = self
            .state
            .active_lease
            .lock()
            .expect("Less Computer active-lease lock poisoned");
        let matches = match active.as_ref() {
            Some(ActiveLease::Capture(capture)) => capture.session_id == session_id,
            Some(ActiveLease::Run(run)) => run.session_id == session_id,
            None => false,
        };
        if matches {
            active.take();
        }
        drop(active);
        if matches {
            self.state.voice_sessions.release(session_id);
        }
    }

    fn current_cancel(&self, session_id: Option<SessionId>) -> Option<Arc<AtomicBool>> {
        self.state
            .active_lease
            .lock()
            .expect("Less Computer active-lease lock poisoned")
            .as_ref()
            .filter(|lease| match lease {
                ActiveLease::Capture(capture) => {
                    session_id.is_none_or(|id| id == capture.session_id)
                }
                ActiveLease::Run(run) => session_id.is_none_or(|id| id == run.session_id),
            })
            .map(|lease| match lease {
                ActiveLease::Capture(capture) => Arc::clone(&capture.cancel),
                ActiveLease::Run(run) => Arc::clone(&run.cancel),
            })
    }

    fn current_cancel_info(
        &self,
        session_id: Option<SessionId>,
    ) -> Option<(SessionId, Arc<AtomicBool>, bool)> {
        self.state
            .active_lease
            .lock()
            .expect("Less Computer active-lease lock poisoned")
            .as_ref()
            .and_then(|lease| match lease {
                ActiveLease::Capture(capture)
                    if session_id.is_none_or(|id| id == capture.session_id) =>
                {
                    Some((capture.session_id, Arc::clone(&capture.cancel), true))
                }
                ActiveLease::Run(run) if session_id.is_none_or(|id| id == run.session_id) => {
                    Some((run.session_id, Arc::clone(&run.cancel), false))
                }
                _ => None,
            })
    }

    fn active_session_inner(&self) -> Option<SessionId> {
        self.state
            .active_lease
            .lock()
            .expect("Less Computer active-lease lock poisoned")
            .as_ref()
            .map(|lease| match lease {
                ActiveLease::Capture(capture) => capture.session_id,
                ActiveLease::Run(run) => run.session_id,
            })
    }

    fn capture_cancelled_inner(&self, session_id: SessionId) -> bool {
        !self
            .state
            .active_lease
            .lock()
            .expect("Less Computer active-lease lock poisoned")
            .as_ref()
            .is_some_and(|lease| match lease {
                ActiveLease::Capture(capture) => {
                    capture.session_id == session_id && !capture.cancel.load(Ordering::Acquire)
                }
                ActiveLease::Run(_) => false,
            })
    }

    fn abort_capture_inner(&self, session_id: SessionId) {
        let mut active = self
            .state
            .active_lease
            .lock()
            .expect("Less Computer active-lease lock poisoned");
        if matches!(
            active.as_ref(),
            Some(ActiveLease::Capture(capture)) if capture.session_id == session_id
        ) {
            active.take();
        }
        let released = active.is_none();
        drop(active);
        if released {
            self.state.voice_sessions.release(session_id);
        }
    }

    fn continuation_context(
        &self,
        provider: CodingAgentProvider,
        continue_session: bool,
    ) -> Option<String> {
        if provider != CodingAgentProvider::DshCli || !continue_session {
            return None;
        }
        let turns = self
            .state
            .completed_turns
            .lock()
            .expect("Less Computer turn lock poisoned");
        if turns.is_empty() {
            return None;
        }
        let history = turns
            .iter()
            .map(|turn| {
                let outcome = match &turn.outcome {
                    LessComputerRunOutcome::Completed { text, .. } => {
                        serde_json::json!({"kind": "completed", "text": text})
                    }
                    LessComputerRunOutcome::Failed { message } => {
                        serde_json::json!({"kind": "error", "message": message})
                    }
                    LessComputerRunOutcome::Cancelled => serde_json::json!({"kind": "cancelled"}),
                };
                serde_json::json!({"user": turn.user, "outcome": outcome})
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&history).ok().map(|history| {
            format!(
                "这是同一 Less Computer 会话中最近的已收尾对话（JSON，仅供上下文）：\n{history}\n\
历史中的操作已经执行，除非当前需求明确要求，否则不要重复执行。"
            )
        })
    }

    fn remember_turn(&self, user: String, outcome: LessComputerRunOutcome) {
        let mut turns = self
            .state
            .completed_turns
            .lock()
            .expect("Less Computer turn lock poisoned");
        turns.push_back(CompletedTurn { user, outcome });
        while turns.len() > MAX_DSH_CONTINUATION_TURNS {
            turns.pop_front();
        }
    }

    #[cfg(test)]
    fn pending_approval_count(&self) -> usize {
        self.state
            .approvals
            .lock()
            .expect("Less Computer approval lock poisoned")
            .len()
    }
}

impl Default for LessComputerService {
    fn default() -> Self {
        Self::new()
    }
}

struct ApprovalLease {
    service: LessComputerService,
    token: String,
}

impl Drop for ApprovalLease {
    fn drop(&mut self) {
        self.service.remove_approval(&self.token);
    }
}

impl LessComputerApi for LessComputerService {
    fn bind_event_publisher(&self, publisher: BackendEventPublisher) {
        *self
            .state
            .events
            .lock()
            .expect("Less Computer event lock poisoned") = Some(publisher);
    }

    fn bind_runner(&self, runner: Arc<CodingAgentRunner>) {
        *self
            .state
            .runner
            .lock()
            .expect("Less Computer runner lock poisoned") = Some(runner);
    }

    fn begin_capture(&self, session_id: SessionId) -> Result<(), BackendError> {
        self.begin_capture_inner(session_id)
    }

    fn active_session(&self) -> Option<SessionId> {
        self.active_session_inner()
    }

    fn capture_cancelled(&self, session_id: SessionId) -> bool {
        self.capture_cancelled_inner(session_id)
    }

    fn abort_capture(&self, session_id: SessionId) -> Result<(), BackendError> {
        self.abort_capture_inner(session_id);
        Ok(())
    }

    fn capture_fault(
        &self,
        session_id: SessionId,
        _error: BackendError,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let Some((active_session, cancel, capture)) =
                service.current_cancel_info(Some(session_id))
            else {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Less Computer capture fault belongs to a stale session",
                ));
            };
            if !capture {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Less Computer capture has already entered the Agent run",
                ));
            }

            // The recorder callback can race with Esc or another native fault.
            // Only the first terminal reporter owns the visible Error event;
            // every path may still call abort because lease release is
            // idempotent and scoped to `active_session`.
            let first = !cancel.swap(true, Ordering::AcqRel);
            service.cancel_pending();
            if !first {
                service.abort_capture_inner(active_session);
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Less Computer capture already has a terminal outcome",
                ));
            }
            let publisher = service.publisher();
            service.abort_capture_inner(active_session);
            publisher?.publish(
                Some(active_session),
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    seq: None,
                    kind: LessComputerEventKind::Error {
                        message: VOICE_CAPTURE_FAILED.to_string(),
                    },
                }),
            );
            Ok(())
        })
    }

    fn submit(
        &self,
        request: LessComputerRunRequest,
    ) -> BoxFuture<'static, Result<LessComputerRunResult, BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.submit_inner(request).await })
    }

    fn cancel(
        &self,
        session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            if let Some((active_session, cancel, capture)) = service.current_cancel_info(session_id)
            {
                let first = !cancel.swap(true, Ordering::AcqRel);
                service.cancel_pending();
                if first && capture {
                    if let Ok(publisher) = service.publisher() {
                        publisher.publish(
                            Some(active_session),
                            BackendEventKind::LessComputerEvent(LessComputerEvent {
                                seq: None,
                                kind: LessComputerEventKind::Cancelled,
                            }),
                        );
                    }
                    service.abort_capture_inner(active_session);
                }
            }
            Ok(())
        })
    }

    fn begin_turn(&self) -> bool {
        self.state.conversation_active.swap(true, Ordering::AcqRel)
    }

    fn dismiss(&self) {
        self.state
            .conversation_active
            .store(false, Ordering::Release);
        if let Some(cancel) = self.current_cancel(None) {
            cancel.store(true, Ordering::Release);
        }
        self.state
            .completed_turns
            .lock()
            .expect("Less Computer turn lock poisoned")
            .clear();
        self.cancel_pending();
    }

    fn request_approval(
        &self,
        command: String,
        reason: String,
    ) -> BoxFuture<'static, Result<bool, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let publisher = service
                .state
                .events
                .lock()
                .expect("Less Computer event lock poisoned")
                .clone()
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "Less Computer event publisher is not bound",
                    )
                })?;
            let token = uuid::Uuid::new_v4().to_string();
            let (sender, receiver) = tokio::sync::oneshot::channel();
            service
                .state
                .approvals
                .lock()
                .expect("Less Computer approval lock poisoned")
                .insert(token.clone(), sender);
            let _lease = ApprovalLease {
                service: service.clone(),
                token: token.clone(),
            };
            publisher.publish(
                service.active_session_inner(),
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    seq: None,
                    kind: LessComputerEventKind::Approval {
                        token,
                        command,
                        reason,
                    },
                }),
            );

            Ok(tokio::select! {
                decision = receiver => decision.unwrap_or(false),
                _ = tokio::time::sleep(service.state.approval_timeout) => false,
            })
        })
    }

    fn approve(
        &self,
        token: String,
        approved: bool,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let sender = state
                .approvals
                .lock()
                .expect("Less Computer approval lock poisoned")
                .remove(&token);
            if let Some(sender) = sender {
                let _ = sender.send(approved);
            }
            Ok(())
        })
    }

    fn cancel_pending(&self) {
        let senders = {
            let mut approvals = self
                .state
                .approvals
                .lock()
                .expect("Less Computer approval lock poisoned");
            approvals
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>()
        };
        for sender in senders {
            let _ = sender.send(false);
        }
    }
}

impl LessComputerService {
    async fn submit_inner(
        &self,
        mut request: LessComputerRunRequest,
    ) -> Result<LessComputerRunResult, BackendError> {
        let transcript = request.transcript.trim().to_string();
        if transcript.is_empty() {
            self.clear_active_lease(request.session_id);
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "Less Computer transcript cannot be empty",
            ));
        }
        let cancel = self.promote_capture_or_start_run(request.session_id)?;
        let runner = match self.runner() {
            Ok(runner) => runner,
            Err(error) => {
                self.clear_active_lease(request.session_id);
                return Err(error);
            }
        };
        let publisher = match self.publisher() {
            Ok(publisher) => publisher,
            Err(error) => {
                self.clear_active_lease(request.session_id);
                return Err(error);
            }
        };

        let continue_session = self.begin_turn();
        if !continue_session {
            self.state
                .completed_turns
                .lock()
                .expect("Less Computer turn lock poisoned")
                .clear();
        }
        request.transcript = transcript.clone();
        request.continue_session = continue_session;
        request.continuation_context =
            self.continuation_context(request.provider, continue_session);
        publisher.publish(
            Some(request.session_id),
            BackendEventKind::LessComputerEvent(LessComputerEvent {
                seq: None,
                kind: LessComputerEventKind::User {
                    text: transcript.clone(),
                    fresh: !continue_session,
                },
            }),
        );

        let mut outcome = self.run_once(&runner, &request, Arc::clone(&cancel)).await;
        if request.provider.supports_command_approval() {
            if let Some(pattern) = self.approval_pattern(&outcome) {
                let approval = self.request_approval(pattern.clone(), approval_reason(&pattern));
                let approved = tokio::select! {
                    result = approval => result.unwrap_or(false),
                    _ = wait_for_cancel(Arc::clone(&cancel)) => {
                        self.cancel_pending();
                        false
                    }
                };
                if approved {
                    request.approved_patterns = equivalent_approved_patterns(&pattern);
                    outcome = self.run_once(&runner, &request, Arc::clone(&cancel)).await;
                }
            }
        }
        if cancel.load(Ordering::Acquire) {
            outcome = LessComputerRunOutcome::Cancelled;
        }

        let final_event = match &outcome {
            LessComputerRunOutcome::Completed { text, cost_usd } => {
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    seq: None,
                    kind: LessComputerEventKind::Completed {
                        text: text.clone(),
                        cost_usd: *cost_usd,
                    },
                })
            }
            LessComputerRunOutcome::Failed { message } => {
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    seq: None,
                    kind: LessComputerEventKind::Error {
                        message: message.clone(),
                    },
                })
            }
            LessComputerRunOutcome::Cancelled => {
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    seq: None,
                    kind: LessComputerEventKind::Cancelled,
                })
            }
        };
        publisher.publish(Some(request.session_id), final_event);
        self.remember_turn(transcript, outcome.clone());
        self.clear_active_lease(request.session_id);
        Ok(LessComputerRunResult {
            session_id: request.session_id,
            outcome,
        })
    }

    async fn run_once(
        &self,
        runner: &Arc<CodingAgentRunner>,
        request: &LessComputerRunRequest,
        cancel: Arc<AtomicBool>,
    ) -> LessComputerRunOutcome {
        let mut runner_request = CodingAgentRequest::new(
            request.session_id.as_uuid().to_string(),
            autonomous_prompt(&request.transcript),
        );
        runner_request.provider = request.provider;
        runner_request.cwd = request.workdir.clone();
        runner_request.model = request.model.clone();
        runner_request.permission_mode = request.permission_mode;
        runner_request.max_budget_usd = request.provider.max_budget_usd();
        runner_request.continue_session = request.continue_session;
        runner_request.continuation_context = request.continuation_context.clone();
        runner_request.executable = request.executable.clone();
        runner_request.approved_patterns = request.approved_patterns.clone();

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let run_future = runner.run_streaming(runner_request, Arc::clone(&cancel), Some(sender));
        tokio::pin!(run_future);
        let mut final_text = String::new();
        let mut cost_usd = None;
        let mut error_message = None;
        let mut cancelled = false;
        let run_result = loop {
            tokio::select! {
                result = &mut run_future => break result,
                event = receiver.recv() => match event {
                    Some(event) => self.consume_stream_event(
                        request.session_id,
                        event,
                        &mut final_text,
                        &mut cost_usd,
                        &mut error_message,
                        &mut cancelled,
                    ),
                    None => break run_future.await,
                },
            }
        };
        while let Some(event) = receiver.recv().await {
            self.consume_stream_event(
                request.session_id,
                event,
                &mut final_text,
                &mut cost_usd,
                &mut error_message,
                &mut cancelled,
            );
        }

        if cancelled || cancel.load(Ordering::Acquire) {
            return LessComputerRunOutcome::Cancelled;
        }
        if let Some(message) = error_message {
            return LessComputerRunOutcome::Failed { message };
        }
        if let Err(error) = run_result {
            if error.code == BackendErrorCode::Cancelled {
                return LessComputerRunOutcome::Cancelled;
            }
            return LessComputerRunOutcome::Failed {
                message: error.message,
            };
        }
        let text = final_text.trim().to_string();
        if text.is_empty() {
            LessComputerRunOutcome::Failed {
                message: "Agent 无结果（确认已登录且额度充足）".into(),
            }
        } else {
            LessComputerRunOutcome::Completed { text, cost_usd }
        }
    }

    fn consume_stream_event(
        &self,
        session_id: SessionId,
        event: CodingAgentStreamEvent,
        final_text: &mut String,
        cost_usd: &mut Option<f64>,
        error_message: &mut Option<String>,
        cancelled: &mut bool,
    ) {
        let expected = session_id.as_uuid().to_string();
        let publisher = match self.publisher() {
            Ok(publisher) => publisher,
            Err(error) => {
                *error_message = Some(error.message);
                return;
            }
        };
        match event {
            CodingAgentStreamEvent::Started { session_id: actual } if actual == expected => {
                publisher.publish(
                    Some(session_id),
                    BackendEventKind::LessComputerEvent(LessComputerEvent {
                        seq: None,
                        kind: LessComputerEventKind::Started,
                    }),
                );
            }
            CodingAgentStreamEvent::Delta {
                session_id: actual,
                text,
            } if actual == expected => {
                publisher.publish(
                    Some(session_id),
                    BackendEventKind::LessComputerEvent(LessComputerEvent {
                        seq: None,
                        kind: LessComputerEventKind::Delta { text },
                    }),
                );
            }
            CodingAgentStreamEvent::ToolUse {
                session_id: actual,
                name,
            } if actual == expected => {
                publisher.publish(
                    Some(session_id),
                    BackendEventKind::LessComputerEvent(LessComputerEvent {
                        seq: None,
                        kind: LessComputerEventKind::Tool { name },
                    }),
                );
            }
            CodingAgentStreamEvent::Compaction { session_id: actual } if actual == expected => {
                publisher.publish(
                    Some(session_id),
                    BackendEventKind::LessComputerEvent(LessComputerEvent {
                        seq: None,
                        kind: LessComputerEventKind::Compaction,
                    }),
                );
            }
            CodingAgentStreamEvent::Completed {
                session_id: actual,
                text,
                cost_usd: cost,
                ..
            } if actual == expected => {
                *final_text = text;
                *cost_usd = cost;
            }
            CodingAgentStreamEvent::Error {
                session_id: actual,
                message,
            } if actual == expected => *error_message = Some(message),
            CodingAgentStreamEvent::Cancelled { session_id: actual } if actual == expected => {
                *cancelled = true
            }
            _ => {}
        }
    }

    fn approval_pattern(&self, outcome: &LessComputerRunOutcome) -> Option<String> {
        let text = match outcome {
            LessComputerRunOutcome::Completed { text, .. }
            | LessComputerRunOutcome::Failed { message: text } => text,
            LessComputerRunOutcome::Cancelled => return None,
        };
        let lowered = text.to_lowercase();
        if ![
            "denied",
            "permission",
            "not allowed",
            "blocked",
            "拒绝",
            "权限",
            "被拦",
        ]
        .iter()
        .any(|keyword| lowered.contains(keyword))
        {
            return None;
        }
        crate::coding_agent_guard::HIGH_RISK_PATTERNS
            .iter()
            .find(|(pattern, _)| lowered.contains(*pattern))
            .map(|(pattern, _)| (*pattern).to_string())
    }
}

fn approval_reason(pattern: &str) -> String {
    crate::coding_agent_guard::HIGH_RISK_PATTERNS
        .iter()
        .find(|(candidate, _)| *candidate == pattern)
        .map(|(_, reason)| (*reason).to_string())
        .unwrap_or_else(|| "高风险命令".to_string())
}

fn equivalent_approved_patterns(pattern: &str) -> Vec<String> {
    let group = crate::coding_agent_guard::risk_equivalent_patterns(pattern);
    if group.is_empty() {
        vec![pattern.to_string()]
    } else {
        group.into_iter().map(str::to_string).collect()
    }
}

async fn wait_for_cancel(cancel: Arc<AtomicBool>) {
    while !cancel.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_agent::{
        AgentCommand, CancellationToken, CodingAgentPermissionMode, CodingAgentProcessAdapter,
        CodingAgentProvider, ProcessExit, ProcessOutputLine, ProcessOutputSink, ProcessStream,
    };
    use crate::domains::{LessComputerRunOutcome, LessComputerRunRequest};
    use crate::events::EventBus;

    #[derive(Default)]
    struct FixtureRuntime {
        requests: Mutex<Vec<AgentCommand>>,
    }

    impl CodingAgentProcessAdapter for FixtureRuntime {
        fn execute(
            &self,
            command: AgentCommand,
            output: Arc<dyn ProcessOutputSink>,
            _cancel: CancellationToken,
        ) -> BoxFuture<'static, Result<ProcessExit, BackendError>> {
            self.requests
                .lock()
                .expect("fixture runtime lock poisoned")
                .push(command);
            Box::pin(async move {
                output.write(ProcessOutputLine {
                    stream: ProcessStream::Stdout,
                    line: r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"答"}}}"#.into(),
                });
                output.write(ProcessOutputLine {
                    stream: ProcessStream::Stdout,
                    line:
                        r#"{"type":"result","result":"答案","total_cost_usd":0.01,"duration_ms":3}"#
                            .into(),
                });
                Ok(ProcessExit {
                    code: Some(0),
                    success: true,
                })
            })
        }
    }

    struct BlockingRuntime;

    impl CodingAgentProcessAdapter for BlockingRuntime {
        fn execute(
            &self,
            _command: AgentCommand,
            _output: Arc<dyn ProcessOutputSink>,
            cancel: CancellationToken,
        ) -> BoxFuture<'static, Result<ProcessExit, BackendError>> {
            Box::pin(async move {
                while !cancel.is_cancelled() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Ok(ProcessExit {
                    code: None,
                    success: false,
                })
            })
        }
    }

    struct StaleRuntime;

    impl CodingAgentProcessAdapter for StaleRuntime {
        fn execute(
            &self,
            _command: AgentCommand,
            output: Arc<dyn ProcessOutputSink>,
            _cancel: CancellationToken,
        ) -> BoxFuture<'static, Result<ProcessExit, BackendError>> {
            Box::pin(async move {
                output.write(ProcessOutputLine {
                    stream: ProcessStream::Stdout,
                    line: r#"{"type":"unknown","sessionId":"stale"}"#.into(),
                });
                Ok(ProcessExit {
                    code: Some(0),
                    success: true,
                })
            })
        }
    }

    struct PartialErrorRuntime;

    impl CodingAgentProcessAdapter for PartialErrorRuntime {
        fn execute(
            &self,
            _command: AgentCommand,
            output: Arc<dyn ProcessOutputSink>,
            _cancel: CancellationToken,
        ) -> BoxFuture<'static, Result<ProcessExit, BackendError>> {
            Box::pin(async move {
                output.write(ProcessOutputLine {
                    stream: ProcessStream::Stdout,
                    line: r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"部分输出"}}}"#.into(),
                });
                output.write(ProcessOutputLine {
                    stream: ProcessStream::Stdout,
                    line: r#"{"type":"result","is_error":true,"result":"协议错误"}"#.into(),
                });
                Ok(ProcessExit {
                    code: Some(0),
                    success: true,
                })
            })
        }
    }

    fn runner(adapter: Arc<dyn CodingAgentProcessAdapter>) -> Arc<CodingAgentRunner> {
        Arc::new(CodingAgentRunner::new(adapter))
    }

    fn service_with_events() -> (LessComputerService, crate::events::EventSubscription) {
        let bus = Arc::new(EventBus::new(8));
        let subscription = bus.subscribe();
        let service = LessComputerService::with_approval_timeout(Duration::from_secs(2));
        service.bind_event_publisher(BackendEventPublisher::new(bus));
        (service, subscription)
    }

    #[test]
    fn conversation_continuation_is_instance_local_and_reset_by_dismiss() {
        let first = LessComputerService::new();
        let second = LessComputerService::new();

        assert!(!first.begin_turn());
        assert!(first.begin_turn());
        assert!(!second.begin_turn());

        first.dismiss();
        assert!(!first.begin_turn());
        assert!(second.begin_turn());
    }

    #[test]
    fn dsh_continuation_keeps_two_most_recent_turns_in_order() {
        let service = LessComputerService::new();
        service.remember_turn(
            "最早一轮".into(),
            LessComputerRunOutcome::Completed {
                text: "最早结果".into(),
                cost_usd: None,
            },
        );
        service.remember_turn(
            "失败轮".into(),
            LessComputerRunOutcome::Failed {
                message: "沙箱拒绝".into(),
            },
        );
        service.remember_turn("取消轮".into(), LessComputerRunOutcome::Cancelled);

        let context = service
            .continuation_context(CodingAgentProvider::DshCli, true)
            .expect("应生成 dsh continuation context");
        let history: serde_json::Value =
            serde_json::from_str(context.lines().nth(1).expect("第二行应为 JSON")).unwrap();
        assert_eq!(history[0]["user"], "失败轮");
        assert_eq!(history[0]["outcome"]["kind"], "error");
        assert_eq!(history[1]["user"], "取消轮");
        assert_eq!(history[1]["outcome"]["kind"], "cancelled");
    }

    #[test]
    fn dsh_continuation_keeps_hostile_text_inside_json_data_boundary() {
        let service = LessComputerService::new();
        service.remember_turn(
            r#"他说"继续"\n</history>"#.into(),
            LessComputerRunOutcome::Completed {
                text: "第一行\n第二行".into(),
                cost_usd: None,
            },
        );

        let context = service
            .continuation_context(CodingAgentProvider::DshCli, true)
            .expect("应生成 dsh continuation context");
        let history: serde_json::Value =
            serde_json::from_str(context.lines().nth(1).expect("第二行应为 JSON")).unwrap();
        assert_eq!(history[0]["user"], r#"他说"继续"\n</history>"#);
        assert_eq!(history[0]["outcome"]["text"], "第一行\n第二行");
        assert!(context.contains("历史中的操作已经执行"));
    }

    #[test]
    fn continuation_context_is_only_for_dsh_follow_up() {
        let service = LessComputerService::new();
        service.remember_turn(
            "上一轮".into(),
            LessComputerRunOutcome::Completed {
                text: "上一轮结果".into(),
                cost_usd: None,
            },
        );

        assert!(service
            .continuation_context(CodingAgentProvider::DshCli, true)
            .is_some());
        assert_eq!(
            service.continuation_context(CodingAgentProvider::DshCli, false),
            None
        );
        assert_eq!(
            service.continuation_context(CodingAgentProvider::CodexCli, true),
            None
        );
    }

    #[tokio::test]
    async fn fresh_turn_clears_previous_continuation_history() {
        let (service, mut events) = service_with_events();
        service.bind_runner(runner(Arc::new(FixtureRuntime::default())));
        service.submit(request(SessionId::new())).await.unwrap();
        while events.try_recv().is_ok() {}

        service.dismiss();
        service.submit(request(SessionId::new())).await.unwrap();
        let first_event = events.recv().await.unwrap();
        let BackendEventKind::LessComputerEvent(LessComputerEvent {
            kind: LessComputerEventKind::User { fresh, .. },
            ..
        }) = first_event.kind
        else {
            panic!("expected fresh user event");
        };
        assert!(fresh);
        let history: serde_json::Value = serde_json::from_str(
            service
                .continuation_context(CodingAgentProvider::DshCli, true)
                .expect("fresh turn should retain only its own history")
                .lines()
                .nth(1)
                .expect("continuation should contain JSON"),
        )
        .unwrap();
        assert_eq!(history[0]["user"], "执行任务");
    }

    #[tokio::test]
    async fn approval_tokens_are_instance_local_idempotent_and_event_driven() {
        let (owner, mut events) = service_with_events();
        let (other, _) = service_with_events();
        let owner_for_request = owner.clone();
        let waiting = tokio::spawn(async move {
            owner_for_request
                .request_approval("rm file".into(), "destructive".into())
                .await
        });

        let event = events.recv().await.unwrap();
        let BackendEventKind::LessComputerEvent(LessComputerEvent {
            kind:
                LessComputerEventKind::Approval {
                    token,
                    command,
                    reason,
                },
            ..
        }) = event.kind
        else {
            panic!("expected approval event");
        };
        assert_eq!(command, "rm file");
        assert_eq!(reason, "destructive");

        other.approve(token.clone(), true).await.unwrap();
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());

        owner.approve(token.clone(), true).await.unwrap();
        owner.approve(token, false).await.unwrap();
        assert!(waiting.await.unwrap().unwrap());
        assert_eq!(owner.pending_approval_count(), 0);
    }

    #[tokio::test]
    async fn dismiss_denies_and_cleans_pending_approvals() {
        let (service, mut events) = service_with_events();
        let service_for_request = service.clone();
        let waiting = tokio::spawn(async move {
            service_for_request
                .request_approval("command".into(), "reason".into())
                .await
        });
        events.recv().await.unwrap();
        assert_eq!(service.pending_approval_count(), 1);

        service.dismiss();

        assert!(!waiting.await.unwrap().unwrap());
        assert_eq!(service.pending_approval_count(), 0);
    }

    #[tokio::test]
    async fn submit_streams_runtime_events_and_publishes_one_terminal_outcome() {
        let bus = Arc::new(EventBus::new(16));
        let mut events = bus.subscribe();
        let service = LessComputerService::new();
        let runtime = Arc::new(FixtureRuntime::default());
        service.bind_event_publisher(BackendEventPublisher::new(bus));
        service.bind_runner(runner(runtime.clone()));
        let session_id = crate::types::SessionId::new();

        let result = service
            .submit(LessComputerRunRequest {
                session_id,
                transcript: "执行任务".into(),
                provider: CodingAgentProvider::ClaudeCodeCli,
                executable: None,
                model: None,
                permission_mode: CodingAgentPermissionMode::AcceptEdits,
                workdir: None,
                continue_session: false,
                continuation_context: None,
                approved_patterns: Vec::new(),
            })
            .await
            .unwrap();
        assert_eq!(result.session_id, session_id);
        assert_eq!(
            result.outcome,
            LessComputerRunOutcome::Completed {
                text: "答案".into(),
                cost_usd: Some(0.01),
            }
        );
        let request = runtime.requests.lock().unwrap().first().cloned().unwrap();
        assert!(matches!(
            request.prompt,
            crate::coding_agent::PromptPayload::Stdin(ref prompt) if prompt.contains("执行任务")
        ));

        let mut kinds = Vec::new();
        for _ in 0..4 {
            let event = events.recv().await.unwrap();
            if let BackendEventKind::LessComputerEvent(event) = event.kind {
                kinds.push(event.kind);
            }
        }
        assert!(matches!(kinds[0], LessComputerEventKind::User { .. }));
        assert!(matches!(kinds[1], LessComputerEventKind::Started));
        assert!(matches!(kinds[2], LessComputerEventKind::Delta { .. }));
        assert!(matches!(kinds[3], LessComputerEventKind::Completed { .. }));
    }

    fn request(session_id: SessionId) -> LessComputerRunRequest {
        LessComputerRunRequest {
            session_id,
            transcript: "执行任务".into(),
            provider: CodingAgentProvider::ClaudeCodeCli,
            executable: None,
            model: None,
            permission_mode: CodingAgentPermissionMode::AcceptEdits,
            workdir: None,
            continue_session: false,
            continuation_context: None,
            approved_patterns: Vec::new(),
        }
    }

    #[test]
    fn capture_lease_is_session_scoped_and_abort_is_idempotent() {
        let service = LessComputerService::new();
        let session_id = SessionId::new();
        let other_session = SessionId::new();

        service.begin_capture(session_id).unwrap();
        assert_eq!(service.active_session(), Some(session_id));
        let duplicate = service.begin_capture(other_session).unwrap_err();
        assert_eq!(duplicate.code, BackendErrorCode::Busy);

        service.abort_capture(other_session).unwrap();
        assert_eq!(service.active_session(), Some(session_id));
        service.abort_capture(session_id).unwrap();
        service.abort_capture(session_id).unwrap();
        assert_eq!(service.active_session(), None);
    }

    #[tokio::test]
    async fn capture_cancellation_releases_the_lease_and_publishes_one_terminal() {
        let (service, mut events) = service_with_events();
        let session_id = SessionId::new();
        service.begin_capture(session_id).unwrap();

        service.cancel(Some(SessionId::new())).await.unwrap();
        assert!(!service.capture_cancelled(session_id));
        service.cancel(Some(session_id)).await.unwrap();
        assert_eq!(service.active_session(), None);
        assert!(service.capture_cancelled(session_id));
        let successor = SessionId::new();
        service.begin_capture(successor).unwrap();
        assert!(service.capture_cancelled(session_id));
        assert!(!service.capture_cancelled(successor));
        let event = events.recv().await.unwrap();
        assert_eq!(event.session_id, Some(session_id));
        assert!(matches!(
            event.kind,
            BackendEventKind::LessComputerEvent(LessComputerEvent {
                kind: LessComputerEventKind::Cancelled,
                ..
            })
        ));
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn matching_capture_is_promoted_to_the_agent_run_and_cleared_on_terminal() {
        let (service, _events) = service_with_events();
        service.bind_runner(runner(Arc::new(FixtureRuntime::default())));
        let session_id = SessionId::new();
        service.begin_capture(session_id).unwrap();

        let result = service.submit(request(session_id)).await.unwrap();

        assert_eq!(result.session_id, session_id);
        assert!(matches!(
            result.outcome,
            LessComputerRunOutcome::Completed { .. }
        ));
        assert_eq!(service.active_session(), None);
    }

    #[tokio::test]
    async fn duplicate_submit_is_busy_and_cancel_forces_cancelled_terminal_state() {
        let (service, mut events) = service_with_events();
        service.bind_runner(runner(Arc::new(BlockingRuntime)));
        let first_id = SessionId::new();
        let first = {
            let service = service.clone();
            tokio::spawn(async move { service.submit(request(first_id)).await })
        };
        loop {
            if matches!(
                events.recv().await.unwrap().kind,
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    kind: LessComputerEventKind::Started,
                    ..
                })
            ) {
                break;
            }
        }

        let duplicate = service.submit(request(SessionId::new())).await.unwrap_err();
        assert_eq!(duplicate.code, BackendErrorCode::Busy);
        assert!(
            service.capture_cancelled(first_id),
            "an Agent run is no longer a capture"
        );
        assert!(!service
            .current_cancel(Some(first_id))
            .unwrap()
            .load(Ordering::Acquire));
        service.cancel(Some(first_id)).await.unwrap();
        let result = first.await.unwrap().unwrap();
        assert_eq!(result.outcome, LessComputerRunOutcome::Cancelled);

        let mut terminals = 0;
        while let Ok(event) = events.try_recv() {
            if matches!(
                event.kind,
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    kind: LessComputerEventKind::Cancelled,
                    ..
                })
            ) {
                terminals += 1;
            }
        }
        assert_eq!(terminals, 1);
    }

    #[tokio::test]
    async fn missing_runtime_is_explicitly_unsupported() {
        let (service, mut events) = service_with_events();
        let error = service.submit(request(SessionId::new())).await.unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Unsupported);
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));
    }

    #[tokio::test]
    async fn stale_stream_events_are_dropped_and_still_have_one_terminal_failure() {
        let (service, mut events) = service_with_events();
        service.bind_runner(runner(Arc::new(StaleRuntime)));
        let result = service.submit(request(SessionId::new())).await.unwrap();
        assert!(matches!(
            result.outcome,
            LessComputerRunOutcome::Failed { .. }
        ));

        let mut kinds = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let BackendEventKind::LessComputerEvent(event) = event.kind {
                kinds.push(event.kind);
            }
        }
        assert!(matches!(
            kinds.first(),
            Some(LessComputerEventKind::User { .. })
        ));
        assert!(kinds
            .iter()
            .all(|kind| !matches!(kind, LessComputerEventKind::Delta { .. })));
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| matches!(kind, LessComputerEventKind::Error { .. }))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn runtime_error_wins_over_partial_output() {
        let (service, mut events) = service_with_events();
        service.bind_runner(runner(Arc::new(PartialErrorRuntime)));
        let result = service.submit(request(SessionId::new())).await.unwrap();
        assert_eq!(
            result.outcome,
            LessComputerRunOutcome::Failed {
                message: "协议错误".into()
            }
        );

        let mut terminal_count = 0;
        while let Ok(event) = events.try_recv() {
            if matches!(
                event.kind,
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    kind: LessComputerEventKind::Error { .. },
                    ..
                })
            ) {
                terminal_count += 1;
            }
        }
        assert_eq!(terminal_count, 1);
    }

    #[tokio::test]
    async fn dismiss_clears_continuation_and_shutdown_path_can_be_reused() {
        let (service, mut events) = service_with_events();
        service.bind_runner(runner(Arc::new(FixtureRuntime::default())));
        let first = service.submit(request(SessionId::new())).await.unwrap();
        assert!(matches!(
            first.outcome,
            LessComputerRunOutcome::Completed { .. }
        ));
        while events.try_recv().is_ok() {}
        assert!(service.begin_turn());
        service.dismiss();
        assert!(!service.begin_turn());
        service.cancel(None).await.unwrap();
        assert_eq!(service.pending_approval_count(), 0);
    }
}
