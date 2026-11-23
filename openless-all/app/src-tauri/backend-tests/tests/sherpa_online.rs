//! Production Sherpa ownership/queue tests without a microphone or model.
//! The default harness uses the runtime's non-native facade; native inference
//! requires the separately linked platform build and explicit model fixtures.
#![allow(unexpected_cfgs)]

pub(crate) mod recorder {
    #[allow(dead_code)]
    pub trait AudioConsumer: Send + Sync {
        fn consume_pcm_chunk(&self, pcm: &[u8]);
        fn finish_segment(&self) {}
    }
    pub fn voice_diagnostic(_: &str, _: &[&str], _: &str, _: &str, _: serde_json::Value) {}
}

pub mod pcm {
    pub use openless_core::asr::pcm::*;
}
#[path = "../../src/asr/local/blocking_decode.rs"]
pub mod blocking_decode;
#[path = "../../src/asr/local/sherpa.rs"]
pub mod sherpa;
#[path = "../../src/asr/local/sherpa_provider.rs"]
pub mod sherpa_provider;
#[path = "../../src/asr/local/sherpa_runtime.rs"]
pub mod sherpa_runtime;

pub(crate) mod asr {
    pub use crate::pcm;
    pub use openless_core::asr::RawTranscript;
    pub mod local {
        pub use crate::sherpa;
    }
}
