#![cfg(target_os = "linux")]

use std::sync::Arc;

use openless_core::{
    AudioConsumer, AudioRecorder, BackendError, DictationContext, RecordingProgressSink, SessionId,
};
use openless_linux_egui::LinuxCpalRecorder;

struct NoopConsumer;

impl AudioConsumer for NoopConsumer {
    fn consume_pcm_chunk(&self, _pcm: &[u8]) {}
}

struct NoopProgress;

impl RecordingProgressSink for NoopProgress {
    fn publish_level(&self, _elapsed_ms: u64, _level: f32) -> Result<(), BackendError> {
        Ok(())
    }
}

/// Exercise native cpal device discovery without making it part of the
/// normal headless suite.  A machine may legitimately have no input device,
/// so the contract accepts the classified platform/permission error and also
/// verifies that a discovered stream can be stopped cleanly.
#[tokio::test]
#[ignore = "requires a Linux audio host; run explicitly on the native runner"]
async fn cpal_device_discovery_and_lifecycle_are_classified() {
    let recorder = LinuxCpalRecorder::new(None);
    let result = recorder
        .start(
            SessionId::new(),
            Arc::new(DictationContext::default()),
            Arc::new(NoopConsumer),
            Arc::new(NoopProgress),
        )
        .await;

    match result {
        Ok(recording) => {
            recording
                .stop()
                .await
                .expect("native cpal recording should stop cleanly");
        }
        Err(error) => assert!(
            matches!(
                error.code,
                openless_core::BackendErrorCode::Platform
                    | openless_core::BackendErrorCode::PermissionDenied
                    | openless_core::BackendErrorCode::Unsupported
            ),
            "unexpected native cpal error: {error:?}"
        ),
    }
}
