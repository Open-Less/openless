//! Hardware-free coverage of the actual Apple Speech bridge and Core adapter.
#![cfg(target_os = "macos")]

extern crate self as tauri;

pub mod async_runtime {
    pub use tokio::task::spawn_blocking;
}

mod recorder {
    pub use openless_core::AudioConsumer;
}

mod asr {
    pub use openless_core::asr::{wav, RawTranscript};
}

#[link(name = "Speech", kind = "framework")]
extern "C" {}

#[allow(dead_code)]
#[path = "../../src/asr/apple_speech.rs"]
mod apple_speech;
