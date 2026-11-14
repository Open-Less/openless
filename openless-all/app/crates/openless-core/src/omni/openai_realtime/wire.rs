//! Explicit text-dictation protocols over the shared WebSocket lifecycle.

use tokio_tungstenite::tungstenite::Message;

use super::{dashscope, protocol, transport};
use crate::omni::OmniConfig;
use crate::polish::LLMError;
use crate::provider_rules::{
    DASHSCOPE_REALTIME_LEGACY_OMNI_PROVIDER_ID, DASHSCOPE_REALTIME_OMNI_PROVIDER_ID,
    OPENAI_REALTIME_OMNI_PROVIDER_ID,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Profile {
    OpenAiGa,
    DashScope,
    DashScopeLegacy,
}

impl Profile {
    pub(super) fn from_provider(provider: &str) -> Result<Self, LLMError> {
        match provider.trim() {
            OPENAI_REALTIME_OMNI_PROVIDER_ID => Ok(Self::OpenAiGa),
            DASHSCOPE_REALTIME_OMNI_PROVIDER_ID => Ok(Self::DashScope),
            DASHSCOPE_REALTIME_LEGACY_OMNI_PROVIDER_ID => Ok(Self::DashScopeLegacy),
            _ => Err(LLMError::ParseError(
                "Unsupported Realtime wire protocol".into(),
            )),
        }
    }

    pub(super) fn endpoint(self, config: &OmniConfig) -> Result<url::Url, LLMError> {
        match self {
            Self::OpenAiGa => transport::endpoint(&config.base_url, &config.model),
            Self::DashScope | Self::DashScopeLegacy => {
                dashscope::endpoint(&config.base_url, &config.model)
            }
        }
    }

    pub(super) fn setup(self, config: &OmniConfig, instructions: &str) -> Message {
        match self {
            Self::OpenAiGa => protocol::setup(config, instructions),
            Self::DashScope => dashscope::setup(instructions, false),
            Self::DashScopeLegacy => dashscope::setup(instructions, true),
        }
    }

    pub(super) fn instructions(self, value: &str) -> Message {
        match self {
            Self::OpenAiGa => protocol::instructions(value),
            Self::DashScope | Self::DashScopeLegacy => dashscope::instructions(value),
        }
    }

    pub(super) fn response(self) -> Message {
        match self {
            Self::OpenAiGa => protocol::response(),
            Self::DashScope | Self::DashScopeLegacy => dashscope::response(),
        }
    }

    pub(super) fn needs_24k(self) -> bool {
        self == Self::OpenAiGa
    }

    pub(super) fn supports_text_input(self) -> bool {
        self == Self::OpenAiGa
    }
}
