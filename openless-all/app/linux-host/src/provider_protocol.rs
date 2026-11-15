//! Channel protocol drafts shared by the host and settings renderer.
//! Extra headers are write-only and are never hydrated from stored credentials.
use openless_core::llm_protocol::{LlmProtocolConfig, LlmRequestFormat, CONFIG_ACCOUNTS};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ProviderProtocolDraft {
    /// Same account order as Core's CONFIG_ACCOUNTS; none of these are API keys.
    pub values: [String; 5],
    pub default_format: LlmRequestFormat,
    pub formats: Vec<LlmRequestFormat>,
    #[serde(default)]
    pub temperature: String,
    #[serde(default)]
    pub extra_headers: String,
}

impl ProviderProtocolDraft {
    pub fn format(&self) -> LlmRequestFormat {
        LlmRequestFormat::parse(&self.values[0]).unwrap_or(self.default_format)
    }

    pub fn validate(&self) -> Result<(), openless_core::BackendError> {
        let mut config = LlmProtocolConfig {
            format: self.default_format,
            ..Default::default()
        };
        for (account, value) in CONFIG_ACCOUNTS.iter().zip(&self.values) {
            config.apply(account, value)?;
        }
        config.validate()?;
        openless_core::parse_temperature(&self.temperature)?;
        let headers = openless_core::provider_rules::parse_extra_headers(&self.extra_headers)?;
        config.validate_headers(&headers)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_drafts_use_core_validation_and_provider_defaults() {
        let mut draft = ProviderProtocolDraft {
            default_format: LlmRequestFormat::Messages,
            ..Default::default()
        };
        assert_eq!(draft.format(), LlmRequestFormat::Messages);
        assert!(draft.validate().is_ok());
        draft.values[1] = "budget".into();
        draft.values[2] = "1024".into();
        assert_eq!(
            draft.validate().unwrap_err().message,
            "llmThinkingBudgetInvalid"
        );
        draft.values[2] = "8192".into();
        draft.values[4] = "fast".into();
        assert!(draft.validate().is_ok());
        draft.values[4] = "turbo".into();
        assert_eq!(
            draft.validate().unwrap_err().message,
            "llmServiceTierInvalid"
        );
        draft.values[4].clear();
        draft.values[2] = "4294967296".into();
        assert_eq!(
            draft.validate().unwrap_err().message,
            "llmTokenLimitInvalid"
        );
    }
}
