use super::*;
use crate::{
    llm_protocol::{LlmRequestFormat, StreamEvent, TextEventStream},
    polish::{LLMError, StreamingTimeouts},
};

/// A separate admission policy over the shared prompt and SSE machinery.
pub(crate) fn request_body(model: &str, messages: Vec<Value>) -> Value {
    let input: Vec<Value> = messages
        .into_iter()
        .map(|mut message| {
            if message["role"] == "system" {
                message["role"] = json!("developer");
            }
            message
        })
        .collect();
    json!({"model": model, "input": input, "stream": true, "store": false})
}
impl SiwcAccess {
    pub(crate) async fn infer<F, C>(
        &self,
        body: &Value,
        timeouts: StreamingTimeouts,
        on_delta: F,
        should_cancel: C,
    ) -> Result<String, LLMError>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send + Sync,
    {
        let call = async {
            let token = self.token().await.map_err(LLMError::Siwc)?;
            let url = self.endpoint();
            let started = std::time::Instant::now();
            let request = crate::net::credential_http_for_url(&url)
                .post(&url)
                .bearer_auth(token.expose_secret())
                .header("Accept", "text/event-stream")
                .json(body);
            // Do not replay a possibly accepted inference or switch its billing source.
            let mut response = tokio::time::timeout(timeouts.first_token, request.send())
                .await
                .map_err(|_| LLMError::Timeout)?
                .map_err(|_| LLMError::Siwc(error("network_failed")))?;
            let status = response.status().as_u16();
            let request_id = response
                .headers()
                .get("x-request-id")
                .or_else(|| response.headers().get("openai-request-id"))
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            if !(200..300).contains(&status) {
                let failure = match tokio::time::timeout(
                    timeouts.first_token.saturating_sub(started.elapsed()),
                    json_response(Ok(response)),
                )
                .await
                {
                    Ok(Err(failure)) => failure,
                    Ok(Ok(_)) => error("request_failed"),
                    Err(_) => error("network_failed"),
                };
                self.record_error(failure.clone())
                    .await
                    .map_err(LLMError::Siwc)?;
                return Err(LLMError::Siwc(failure));
            }
            let mut events =
                TextEventStream::new(LlmRequestFormat::Responses).for_siwc(request_id.clone());
            let mut text = String::new();
            while !events.done {
                let budget = if text.is_empty() {
                    timeouts.first_token.saturating_sub(started.elapsed())
                } else {
                    timeouts.idle
                };
                let chunk = tokio::time::timeout(budget, response.chunk())
                    .await
                    .map_err(|_| LLMError::Timeout)?
                    .map_err(|_| LLMError::Siwc(error("network_failed")))?;
                let Some(chunk) = chunk else {
                    break;
                };
                events.push(&chunk)?;
                loop {
                    let event = match events.next() {
                        Err(LLMError::Siwc(failure)) => {
                            let failure = failure.with_http_context(status, request_id.as_deref());
                            self.record_error(failure.clone())
                                .await
                                .map_err(LLMError::Siwc)?;
                            return Err(LLMError::Siwc(failure));
                        }
                        other => other?,
                    };
                    let Some(event) = event else {
                        break;
                    };
                    if should_cancel() {
                        return Err(LLMError::Siwc(error("cancelled")));
                    }
                    if let StreamEvent::Text(delta) = event {
                        text.push_str(&delta);
                        on_delta(&delta);
                    }
                }
            }
            events.finish().map_err(|_| {
                LLMError::Siwc(
                    error("response_incomplete").with_http_context(status, request_id.as_deref()),
                )
            })?;
            self.check().await.map_err(LLMError::Siwc)?;
            if text.trim().is_empty() {
                return Err(LLMError::Siwc(error("empty_response")));
            }
            Ok(text)
        };
        let cancelled = async {
            while !should_cancel() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        tokio::select! {
            biased;
            _ = cancelled => Err(LLMError::Siwc(error("cancelled"))),
            failure = self.cancelled() => Err(LLMError::Siwc(failure)),
            result = call => result,
        }
    }
}
