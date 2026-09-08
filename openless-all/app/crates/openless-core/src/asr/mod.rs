//! Cross-platform ASR protocol implementations shared by every host.

pub mod bailian;
pub mod dashscope_multimodal;
pub mod elevenlabs;
mod frame;
pub mod mimo;
pub mod pcm;
pub mod qwen_realtime;
pub mod stepfun_realtime;
pub mod volcengine;
pub mod wav;
pub mod whisper;
pub mod xfyun;

pub use crate::ports::AudioConsumer;
pub use bailian::{BailianCredentials, BailianRealtimeASR};
pub use dashscope_multimodal::DashScopeMultimodalASR;
pub use elevenlabs::ElevenLabsBatchASR;
pub use mimo::MimoBatchASR;
pub use qwen_realtime::{Qwen3RealtimeASR, Qwen3RealtimeCredentials};
pub use stepfun_realtime::{StepfunRealtimeASR, StepfunRealtimeCredentials};
pub use volcengine::{VolcengineCredentials, VolcengineStreamingASR};
pub use whisper::WhisperBatchASR;
pub use xfyun::{XfyunCredentials, XfyunStreamingASR};

/// What a provider yielded after the stream or batch request completed.
pub type RawTranscript = crate::ports::TranscriptOutput;

/// User-defined hotword used to bias providers that expose that capability.
#[derive(Debug, Clone)]
pub struct DictionaryHotword {
    pub phrase: String,
    pub enabled: bool,
}
