use std::sync::Arc;

use parking_lot::Mutex;
use uuid::Uuid;

use crate::ask::{
    AskCallPurpose, AskCallUsage, AskCancellation, AskModel, AskModelCompletion, AskModelProgress,
    AskModelRequest, AskUsagePersistence,
};
use crate::assistant_api::{
    AssistantActivity, AssistantEvent, AssistantEventSink, AssistantToolStatus,
};
use crate::usage_ledger::{
    sanitized_identifier, ReportedTokens, ServiceKind, ServiceOutcome, ServicePurpose, UsageRecord,
};
use crate::BackendError;

/// Callbacks may be retained by an adapter. Closing the gate on drop prevents later emissions.
pub(super) struct EventGuard {
    sink: Arc<Mutex<Option<AssistantEventSink>>>,
    cancellation: AskCancellation,
}

impl EventGuard {
    pub(super) fn new(sink: AssistantEventSink, cancellation: AskCancellation) -> Self {
        Self {
            sink: Arc::new(Mutex::new(Some(sink))),
            cancellation,
        }
    }

    pub(super) fn sink(&self) -> AssistantEventSink {
        let sink = self.sink.clone();
        let cancellation = self.cancellation.clone();
        Arc::new(move |event| {
            let sink = sink.lock();
            if !cancellation.is_cancelled() {
                if let Some(sink) = sink.as_ref() {
                    sink(event);
                }
            }
        })
    }

    pub(super) fn emit(&self, event: AssistantEvent) {
        self.sink()(event);
    }

    pub(super) fn cancelled_tool(&self, id: Uuid, name: String) {
        if let Some(sink) = self.sink.lock().as_ref() {
            sink(AssistantEvent::Tool {
                tool_call_id: id,
                name,
                status: AssistantToolStatus::Cancelled,
                error: None,
            });
        }
    }
}

impl Drop for EventGuard {
    fn drop(&mut self) {
        self.sink.lock().take();
    }
}

#[derive(Default)]
struct Observed {
    text: String,
    tokens: ReportedTokens,
    closed: bool,
}

struct CallGuard<'a> {
    state: Arc<Mutex<Observed>>,
    usage: Option<&'a AskUsagePersistence>,
    generation: u64,
    record: UsageRecord,
    cancellation: AskCancellation,
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        state.closed = true;
        let Some(usage) = self.usage else {
            return;
        };
        let tokens = state.tokens;
        self.record.input_tokens = tokens.input;
        self.record.output_tokens = tokens.output;
        self.record.cached_input_tokens = tokens.cached_input;
        self.record.cache_write_input_tokens = tokens.cache_write_input;
        self.record.reasoning_tokens = tokens.reasoning;
        self.record.input_accounting = tokens.input_accounting;
        if self.cancellation.is_cancelled() {
            self.record.outcome = ServiceOutcome::Cancelled;
        }
        usage.recorder.submit(self.generation, self.record.clone());
    }
}

pub(super) async fn run(
    model: &dyn AskModel,
    request: AskModelRequest,
    cancellation: AskCancellation,
    events: &EventGuard,
    usage: Option<&AskUsagePersistence>,
    compact: bool,
) -> Result<(AskModelCompletion, String), BackendError> {
    super::check_cancelled(&cancellation)?;
    let label = model.label();
    let state = Arc::new(Mutex::new(Observed::default()));
    let mut guard = CallGuard {
        state: state.clone(),
        usage,
        generation: usage.map_or(0, |usage| usage.recorder.generation()),
        cancellation: cancellation.clone(),
        record: UsageRecord {
            id: Uuid::new_v4(),
            occurred_at_ms: usage.map_or(0, |u| u.clock.now_utc().timestamp_millis()),
            kind: ServiceKind::Llm,
            purpose: if compact {
                ServicePurpose::Other
            } else {
                ServicePurpose::Question
            },
            outcome: ServiceOutcome::Unknown,
            provider: None,
            service: sanitized_identifier(&label.service),
            channel: sanitized_identifier(&label.channel),
            model: sanitized_identifier(&label.model),
            protocol: sanitized_identifier(&label.protocol),
            input_accounting: None,
            audio_ms: None,
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            cache_write_input_tokens: None,
            reasoning_tokens: None,
            synthesized_characters: None,
        },
    };
    let observed = state.clone();
    let callback_events = events.sink();
    let callback_cancel = cancellation.clone();
    let call_id = guard.record.id;
    let model_label = label.clone();
    let purpose = if compact {
        AskCallPurpose::Compact
    } else {
        AskCallPurpose::Answer
    };
    events.emit(AssistantEvent::Usage(AskCallUsage {
        call_id,
        purpose,
        model: model_label.clone(),
        input: None,
        output: None,
        cached_input: None,
        cache_write_input: None,
        reasoning: None,
        input_accounting: None,
    }));
    let progress = Arc::new(move |event| {
        let mut observed = observed.lock();
        if observed.closed || callback_cancel.is_cancelled() {
            return;
        }
        match event {
            AskModelProgress::Text(delta) => {
                observed.text.push_str(&delta);
                if !compact {
                    callback_events(AssistantEvent::Activity(AssistantActivity::Writing));
                    callback_events(AssistantEvent::TextDelta(delta));
                }
            }
            AskModelProgress::Thinking if !compact => {
                callback_events(AssistantEvent::Activity(AssistantActivity::Thinking))
            }
            AskModelProgress::Usage(tokens) => {
                observed.tokens.merge_report(tokens);
                let tokens = observed.tokens;
                callback_events(AssistantEvent::Usage(AskCallUsage {
                    call_id,
                    purpose,
                    model: model_label.clone(),
                    input: tokens.input,
                    output: tokens.output,
                    cached_input: tokens.cached_input,
                    cache_write_input: tokens.cache_write_input,
                    reasoning: tokens.reasoning,
                    input_accounting: tokens.input_accounting,
                }));
            }
            _ => {}
        }
    });
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(super::cancelled()),
        result = model.stream(request, cancellation.clone(), progress) => result,
    };
    let result = super::check_cancelled(&cancellation).and(result);
    guard.record.outcome = match &result {
        Ok(_) => ServiceOutcome::Succeeded,
        Err(error) if error.code == crate::BackendErrorCode::Cancelled => ServiceOutcome::Cancelled,
        Err(_) => ServiceOutcome::Failed,
    };
    let mut state = state.lock();
    state.closed = true;
    let text = std::mem::take(&mut state.text);
    drop(state);
    result.map(|completion| (completion, text))
}
