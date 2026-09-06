use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::{
    BackendError, BackendErrorCode, InsertOutcome, SelectionCapture, SelectionRuntimeAdapter,
    SessionId,
};

trait LinuxSelectionBridge: Send + Sync + 'static {
    fn capture_target(&self, session_id: SessionId) -> Result<String, BackendError>;
    fn apply_target(
        &self,
        session_id: SessionId,
        source: &str,
        replacement: &str,
    ) -> Result<(), BackendError>;
    fn revert_target(&self, session_id: SessionId) -> Result<(), BackendError>;
    fn cancel_target(&self, session_id: SessionId) -> Result<(), BackendError>;
}

struct Fcitx5SelectionBridge;

impl LinuxSelectionBridge for Fcitx5SelectionBridge {
    fn capture_target(&self, session_id: SessionId) -> Result<String, BackendError> {
        crate::fcitx5::capture_selection_target(&session_id.to_string())
    }

    fn apply_target(
        &self,
        session_id: SessionId,
        source: &str,
        replacement: &str,
    ) -> Result<(), BackendError> {
        crate::fcitx5::apply_selection_target(&session_id.to_string(), source, replacement)
    }

    fn revert_target(&self, session_id: SessionId) -> Result<(), BackendError> {
        crate::fcitx5::revert_selection_target(&session_id.to_string())
    }

    fn cancel_target(&self, session_id: SessionId) -> Result<(), BackendError> {
        crate::fcitx5::cancel_selection_target(&session_id.to_string())
    }
}

#[derive(Clone)]
struct LinuxSelectionTarget {
    // The UUID is the Core session/ticket generation. Every Host effect checks
    // it before touching the retained native input context in the plugin.
    session_id: SessionId,
    source_text: String,
    // Retained only after an acknowledged apply, so revert cannot delete text
    // for a preview that was never committed.
    replacement_text: Option<String>,
}

#[derive(Clone)]
pub struct LinuxSelectionRuntime {
    bridge: Arc<dyn LinuxSelectionBridge>,
    // Core owns Capturing/Preview/Applying/Completed. This mutex serializes the
    // single opaque fcitx5 target and prevents stale async calls from replacing
    // the ticket installed by a newer session.
    target: Arc<Mutex<Option<LinuxSelectionTarget>>>,
}

impl Default for LinuxSelectionRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxSelectionRuntime {
    pub fn new() -> Self {
        Self::with_bridge(Arc::new(Fcitx5SelectionBridge))
    }

    fn with_bridge(bridge: Arc<dyn LinuxSelectionBridge>) -> Self {
        Self {
            bridge,
            target: Arc::new(Mutex::new(None)),
        }
    }
}

impl SelectionRuntimeAdapter for LinuxSelectionRuntime {
    fn capture(
        &self,
        session_id: SessionId,
        supplied_text: Option<String>,
    ) -> BoxFuture<'static, Result<SelectionCapture, BackendError>> {
        let bridge = Arc::clone(&self.bridge);
        let target = Arc::clone(&self.target);
        Box::pin(async move {
            if supplied_text.is_some() {
                return Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "Linux selection replacement requires a live fcitx5 target",
                ));
            }
            tokio::task::spawn_blocking(move || {
                let mut target = target.lock().expect("Linux selection target lock poisoned");
                if target
                    .as_ref()
                    .is_some_and(|active| active.session_id == session_id)
                {
                    return Err(BackendError::new(
                        BackendErrorCode::Busy,
                        "the Linux selection session is already captured",
                    ));
                }
                if let Some(previous) = target.take() {
                    let _ = bridge.cancel_target(previous.session_id);
                }
                let text = bridge.capture_target(session_id)?;
                *target = Some(LinuxSelectionTarget {
                    session_id,
                    source_text: text.clone(),
                    replacement_text: None,
                });
                Ok(SelectionCapture {
                    text,
                    source_app: None,
                })
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("Linux selection capture task failed: {error}"),
                )
            })?
        })
    }

    fn apply(
        &self,
        session_id: SessionId,
        source_text: String,
        replacement_text: String,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let bridge = Arc::clone(&self.bridge);
        let target = Arc::clone(&self.target);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut target = target.lock().expect("Linux selection target lock poisoned");
                let Some(active) = target.as_mut() else {
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "the Linux selection target is no longer active",
                    ));
                };
                if active.session_id != session_id || active.source_text != source_text {
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "the Linux selection changed before replacement",
                    ));
                }
                bridge.apply_target(session_id, &source_text, &replacement_text)?;
                active.replacement_text = Some(replacement_text);
                Ok(InsertOutcome::Inserted)
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("Linux selection apply task failed: {error}"),
                )
            })?
        })
    }

    fn prepare_preview(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let target = Arc::clone(&self.target);
        Box::pin(async move {
            let target = target.lock().expect("Linux selection target lock poisoned");
            if target
                .as_ref()
                .is_some_and(|active| active.session_id == session_id)
            {
                Ok(())
            } else {
                Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "the Linux selection preview target is stale",
                ))
            }
        })
    }

    fn revert(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let bridge = Arc::clone(&self.bridge);
        let target = Arc::clone(&self.target);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut target = target.lock().expect("Linux selection target lock poisoned");
                let active = target.as_ref().ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::Cancelled,
                        "the Linux selection target is no longer active",
                    )
                })?;
                if active.session_id != session_id {
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "the Linux selection revert ticket is stale",
                    ));
                }
                active.replacement_text.as_deref().ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "the Linux selection has not been applied",
                    )
                })?;
                bridge.revert_target(session_id)?;
                *target = None;
                Ok(InsertOutcome::Inserted)
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("Linux selection revert task failed: {error}"),
                )
            })?
        })
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let bridge = Arc::clone(&self.bridge);
        let target = Arc::clone(&self.target);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut target = target.lock().expect("Linux selection target lock poisoned");
                if target
                    .as_ref()
                    .is_some_and(|active| active.session_id == session_id)
                {
                    bridge.cancel_target(session_id)?;
                    *target = None;
                }
                Ok(())
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("Linux selection cancel task failed: {error}"),
                )
            })?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestSelectionBridge {
        selected_text: Mutex<String>,
        committed: Mutex<Vec<String>>,
        reverted: Mutex<Vec<(String, String)>>,
    }

    impl LinuxSelectionBridge for TestSelectionBridge {
        fn capture_target(
            &self,
            _session_id: SessionId,
        ) -> Result<String, openless_core::BackendError> {
            Ok(self.selected_text.lock().unwrap().clone())
        }

        fn apply_target(
            &self,
            _session_id: SessionId,
            source: &str,
            replacement: &str,
        ) -> Result<(), openless_core::BackendError> {
            if *self.selected_text.lock().unwrap() != source {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "selection changed",
                ));
            }
            self.committed.lock().unwrap().push(replacement.to_string());
            Ok(())
        }

        fn revert_target(&self, _session_id: SessionId) -> Result<(), openless_core::BackendError> {
            let original = self.selected_text.lock().unwrap().clone();
            let replacement = self.committed.lock().unwrap().last().cloned().unwrap();
            self.reverted.lock().unwrap().push((original, replacement));
            Ok(())
        }

        fn cancel_target(&self, _session_id: SessionId) -> Result<(), openless_core::BackendError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn direct_apply_revalidates_the_selection_before_committing() {
        let bridge = Arc::new(TestSelectionBridge {
            selected_text: Mutex::new("source".to_string()),
            committed: Mutex::new(Vec::new()),
            reverted: Mutex::new(Vec::new()),
        });
        let runtime = LinuxSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        runtime.capture(session_id, None).await.unwrap();

        let outcome = runtime
            .apply(session_id, "source".to_string(), "replacement".to_string())
            .await
            .unwrap();

        assert_eq!(outcome, InsertOutcome::Inserted);
        assert_eq!(
            bridge.committed.lock().unwrap().as_slice(),
            &["replacement"]
        );
    }

    #[tokio::test]
    async fn changed_selection_is_rejected_without_committing() {
        let bridge = Arc::new(TestSelectionBridge {
            selected_text: Mutex::new("source".to_string()),
            committed: Mutex::new(Vec::new()),
            reverted: Mutex::new(Vec::new()),
        });
        let runtime = LinuxSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        runtime.capture(session_id, None).await.unwrap();
        *bridge.selected_text.lock().unwrap() = "changed".to_string();

        let error = runtime
            .apply(session_id, "source".to_string(), "replacement".to_string())
            .await
            .expect_err("a changed selection must not be replaced");

        assert_eq!(error.code, BackendErrorCode::Cancelled);
        assert!(bridge.committed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelled_selection_is_rejected_without_committing() {
        let bridge = Arc::new(TestSelectionBridge {
            selected_text: Mutex::new("source".to_string()),
            committed: Mutex::new(Vec::new()),
            reverted: Mutex::new(Vec::new()),
        });
        let runtime = LinuxSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        runtime.capture(session_id, None).await.unwrap();
        runtime.cancel(session_id).await.unwrap();

        let error = runtime
            .apply(session_id, "source".to_string(), "replacement".to_string())
            .await
            .expect_err("a cancelled selection must not be replaced");

        assert_eq!(error.code, BackendErrorCode::Cancelled);
        assert!(bridge.committed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_new_capture_invalidates_the_previous_session_target() {
        let bridge = Arc::new(TestSelectionBridge {
            selected_text: Mutex::new("first".to_string()),
            committed: Mutex::new(Vec::new()),
            reverted: Mutex::new(Vec::new()),
        });
        let runtime = LinuxSelectionRuntime::with_bridge(bridge.clone());
        let first = SessionId::new();
        let second = SessionId::new();
        runtime.capture(first, None).await.unwrap();
        *bridge.selected_text.lock().unwrap() = "second".to_string();
        runtime.capture(second, None).await.unwrap();

        let error = runtime
            .apply(first, "first".to_string(), "replacement".to_string())
            .await
            .expect_err("the previous session must lose target ownership");

        assert_eq!(error.code, BackendErrorCode::Cancelled);
        assert!(bridge.committed.lock().unwrap().is_empty());
        runtime
            .apply(second, "second".to_string(), "replacement".to_string())
            .await
            .expect("stale apply must not discard the new session target");
        assert_eq!(
            bridge.committed.lock().unwrap().as_slice(),
            &["replacement"]
        );
    }

    #[tokio::test]
    async fn duplicate_capture_is_busy_and_preserves_the_original_target() {
        let bridge = Arc::new(TestSelectionBridge {
            selected_text: Mutex::new("original".to_string()),
            committed: Mutex::new(Vec::new()),
            reverted: Mutex::new(Vec::new()),
        });
        let runtime = LinuxSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        runtime.capture(session_id, None).await.unwrap();
        *bridge.selected_text.lock().unwrap() = "changed".to_string();

        let error = runtime
            .capture(session_id, None)
            .await
            .expect_err("duplicate capture must be rejected");

        assert_eq!(error.code, BackendErrorCode::Busy);
        *bridge.selected_text.lock().unwrap() = "original".to_string();
        runtime
            .apply(
                session_id,
                "original".to_string(),
                "replacement".to_string(),
            )
            .await
            .expect("duplicate capture must not overwrite the original target");
        assert_eq!(
            bridge.committed.lock().unwrap().as_slice(),
            &["replacement"]
        );
    }

    #[tokio::test]
    async fn supplied_text_without_a_live_target_is_unsupported() {
        let bridge = Arc::new(TestSelectionBridge::default());
        let runtime = LinuxSelectionRuntime::with_bridge(bridge);

        let error = runtime
            .capture(SessionId::new(), Some("detached text".to_string()))
            .await
            .expect_err("detached text cannot prove a Linux replacement target");

        assert_eq!(error.code, BackendErrorCode::Unsupported);
    }

    #[tokio::test]
    async fn preview_retains_only_the_captured_session_target() {
        let bridge = Arc::new(TestSelectionBridge {
            selected_text: Mutex::new("source".to_string()),
            ..TestSelectionBridge::default()
        });
        let runtime = LinuxSelectionRuntime::with_bridge(bridge);
        let session_id = SessionId::new();
        runtime.capture(session_id, None).await.unwrap();

        runtime.prepare_preview(session_id).await.unwrap();
        assert_eq!(
            runtime
                .prepare_preview(SessionId::new())
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Cancelled
        );
    }

    #[tokio::test]
    async fn revert_uses_the_same_session_and_exact_applied_text() {
        let bridge = Arc::new(TestSelectionBridge {
            selected_text: Mutex::new("source".to_string()),
            ..TestSelectionBridge::default()
        });
        let runtime = LinuxSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        runtime.capture(session_id, None).await.unwrap();
        runtime
            .apply(session_id, "source".into(), "replacement".into())
            .await
            .unwrap();

        assert_eq!(
            runtime.revert(session_id).await.unwrap(),
            InsertOutcome::Inserted
        );
        assert_eq!(
            bridge.reverted.lock().unwrap().as_slice(),
            &[("source".to_string(), "replacement".to_string())]
        );
    }
}
