//! Read-only personal assistant. Routing, native targets and durable history belong to hosts.

use std::sync::Arc;

use crate::ask::{
    AskCancellation, AskModel, AskModelCompletion, AskModelFactory, AskToolCall, AskToolResult,
    AskUsagePersistence, SearchTool,
};
use crate::assistant_api::{
    AssistantActivity, AssistantContextProvider, AssistantEvent, AssistantEventSink,
    AssistantRequest, AssistantResponse, AssistantRuntime, AssistantToolStatus,
};
use crate::{BackendError, BackendErrorCode};
use futures_util::future::BoxFuture;

mod budget;
mod call;
mod prompt;
#[cfg(test)]
mod tests;
pub(crate) mod tools;
pub mod web;

/// Use the existing ConfiguredAskModels, WebSearchTool and services.usage_recorder.
/// No history store, writer, native focus target or general-purpose execution port is accepted.
pub struct AssistantDependencies {
    pub models: Arc<dyn AskModelFactory>,
    pub context: Arc<dyn AssistantContextProvider>,
    pub search: Arc<dyn SearchTool>,
    pub usage: Option<AskUsagePersistence>,
}

pub struct PersonalAssistant {
    deps: AssistantDependencies,
}

impl PersonalAssistant {
    pub fn new(deps: AssistantDependencies) -> Self {
        Self { deps }
    }

    async fn execute(
        &self,
        request: AssistantRequest,
        cancellation: AskCancellation,
        events: AssistantEventSink,
    ) -> Result<AssistantResponse, BackendError> {
        let events = call::EventGuard::new(events, cancellation.clone());
        check_cancelled(&cancellation)?;
        events.emit(AssistantEvent::Activity(AssistantActivity::Thinking));
        let model = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(cancelled()),
            model = self.deps.models.prepare(request.options.deep_thinking) => model?,
        };
        check_cancelled(&cancellation)?;
        let key = request.key;
        let mut plan = budget::Plan::new(request, model.capabilities(), model.supports_tools())?;
        let mut answer = String::new();
        // A finite loop prevents a faulty provider from issuing tools indefinitely. This is
        // a request limit, not a keyword router or a one-search restriction.
        for _ in 0..24 {
            self.compact(&mut plan, model.as_ref(), &cancellation, &events)
                .await?;
            events.emit(AssistantEvent::Activity(AssistantActivity::Thinking));
            let (completion, text) = call::run(
                model.as_ref(),
                plan.request()?,
                cancellation.clone(),
                &events,
                self.deps.usage.as_ref(),
                false,
            )
            .await?;
            check_cancelled(&cancellation)?;
            answer.push_str(&text);
            match completion {
                AskModelCompletion::Complete => {
                    if text.trim().is_empty() {
                        return Err(error(BackendErrorCode::Provider, "askEmptyAnswer"));
                    }
                    return Ok(AssistantResponse { text: answer });
                }
                AskModelCompletion::ToolCall(call) => {
                    if matches!(call.tool, AskToolCall::ResolveIntent { .. })
                        || !model.supports_tools()
                    {
                        return Err(error(BackendErrorCode::Provider, "askUnexpectedToolCall"));
                    }
                    let id = uuid::Uuid::new_v4();
                    let name = tools::name(&call.tool).to_owned();
                    events.emit(AssistantEvent::Tool {
                        tool_call_id: id,
                        name: name.clone(),
                        status: AssistantToolStatus::Running,
                        error: None,
                    });
                    let result = tokio::select! {
                        biased;
                        _ = cancellation.cancelled() => Err(cancelled()),
                        result = tokio::time::timeout(
                            std::time::Duration::from_secs(25),
                            tools::execute(&self.deps, key, id, &call.tool, &cancellation, &events),
                        ) => result.unwrap_or_else(|_| Err(error(BackendErrorCode::Provider, "askToolTimeout").retryable(true))),
                    };
                    if cancellation.is_cancelled()
                        || result
                            .as_ref()
                            .is_err_and(|e| e.code == BackendErrorCode::Cancelled)
                    {
                        events.cancelled_tool(id, name);
                        return Err(cancelled());
                    }
                    let output = match result {
                        Ok(result) => {
                            events.emit(AssistantEvent::Tool {
                                tool_call_id: id,
                                name,
                                status: result.status,
                                error: result.error,
                            });
                            result.output
                        }
                        Err(err) => {
                            // Raw platform/provider diagnostics are never exposed to the UI or model.
                            let public = error(err.code, "askToolFailed").retryable(err.retryable);
                            events.emit(AssistantEvent::Tool {
                                tool_call_id: id,
                                name,
                                status: AssistantToolStatus::Failed,
                                error: Some(public.clone()),
                            });
                            serde_json::json!({"status":"failed", "code":public.code, "reason":public.message}).to_string()
                        }
                    };
                    plan.results.push(AskToolResult { call, output });
                }
            }
        }
        Err(error(BackendErrorCode::Provider, "askToolLimit"))
    }

    async fn compact(
        &self,
        plan: &mut budget::Plan,
        model: &dyn AskModel,
        cancellation: &AskCancellation,
        events: &call::EventGuard,
    ) -> Result<(), BackendError> {
        while let Some((selection, request)) = plan.compaction()? {
            events.emit(AssistantEvent::Activity(AssistantActivity::Compacting));
            let (completion, summary) = call::run(
                model,
                request,
                cancellation.clone(),
                events,
                self.deps.usage.as_ref(),
                true,
            )
            .await?;
            check_cancelled(cancellation)?;
            if !matches!(completion, AskModelCompletion::Complete) {
                return Err(error(BackendErrorCode::Provider, "askUnexpectedToolCall"));
            }
            plan.apply_summary(&selection, summary)?;
        }
        Ok(())
    }
}

impl AssistantRuntime for PersonalAssistant {
    fn run(
        &self,
        request: AssistantRequest,
        cancellation: AskCancellation,
        events: AssistantEventSink,
    ) -> BoxFuture<'_, Result<AssistantResponse, BackendError>> {
        Box::pin(self.execute(request, cancellation, events))
    }
}

fn error(code: BackendErrorCode, reason: &str) -> BackendError {
    BackendError::new(code, reason)
}

fn cancelled() -> BackendError {
    error(BackendErrorCode::Cancelled, "askCancelled")
}

fn check_cancelled(cancellation: &AskCancellation) -> Result<(), BackendError> {
    if cancellation.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}
