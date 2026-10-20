use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendErrorCode {
    InvalidArgument,
    InvalidState,
    Busy,
    Cancelled,
    PermissionDenied,
    Unsupported,
    Provider,
    Persistence,
    Platform,
    OutcomeUnknown,
    Internal,
}

#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq, Eq)]
#[error("{message}")]
pub struct BackendError {
    pub code: BackendErrorCode,
    pub message: String,
    pub retryable: bool,
    pub details: Option<serde_json::Value>,
}

impl BackendError {
    pub fn new(code: BackendErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            details: None,
        }
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn target_unavailable(mut self) -> Self {
        self.details = Some(serde_json::json!({"target_unavailable": true}));
        self
    }

    pub fn is_target_unavailable(&self) -> bool {
        self.details
            .as_ref()
            .and_then(|details| details.get("target_unavailable"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable_and_errors_do_not_invent_secret_fields() {
        let value = serde_json::to_value(
            BackendError::new(BackendErrorCode::PermissionDenied, "permission required")
                .retryable(false),
        )
        .unwrap();
        assert_eq!(value["code"], "permission_denied");
        assert_eq!(value["retryable"], false);
        assert!(value.get("token").is_none());
        assert!(value.get("authorization").is_none());
        assert!(value.get("pin").is_none());
    }
}
