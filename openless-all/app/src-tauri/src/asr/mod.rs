//! Streaming ASR providers.
//!
//! Mirrors the Swift `OpenLessASR` library. The Volcengine SAUC bigmodel
//! client is the reference implementation; the wire protocol lives in
//! `frame.rs` (binary frame codec) and the session lifecycle in
//! `volcengine.rs`.

#[cfg(target_os = "macos")]
pub mod apple_speech;

pub mod local;

pub use openless_core::asr::{bailian, pcm, volcengine, wav, whisper, RawTranscript};
