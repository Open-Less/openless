use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::{
    BackendError, BackendErrorCode, InsertOutcome, SelectionCapture, SelectionRuntimeAdapter,
    SessionId,
};

pub(crate) fn validate_selection_replacement_source(text: &str) -> Result<(), BackendError> {
    // Same fixed head/tail stand-in as Tauri; merely mentioning the marker
    // in ordinary text is not a reason to reject a real selection.
    const MARKER: &str = "\n[\u{2026}truncated\u{2026}]\n";
    let stand_in_length = 4000 + MARKER.chars().count();
    if text.contains(MARKER) && text.chars().take(stand_in_length + 1).count() == stand_in_length {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "selectionPolishSelectionTooLong",
        ));
    }
    Ok(())
}

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
    /// 选区只从合成器 PRIMARY 读到（本次热键按下时焦点不在 fcitx5 的输入上下文里）：
    /// 可以预览/编辑/复制，但**无法回写替换**（没有输入上下文可 commit）。
    primary_only: bool,
    // Retained only after an acknowledged apply, so revert cannot delete text
    // for a preview that was never committed.
    replacement_text: Option<String>,
}

/// 直接读合成器的 PRIMARY 选区。
///
/// 为什么需要它：fcitx5 插件只在「热键按下时焦点在 IME 输入上下文里」时才拿得到
/// 选区（`selectionIc_`）。浏览器地址栏、终端、看片窗口这些地方它没有上下文，
/// 于是选区润色整条流程直接失败（日志：`selection polish had no captured
/// selection`），而 `wl-paste --primary` 明明读得到。这里做宿主侧兜底。
fn read_primary_selection() -> Option<String> {
    use std::process::{Command, Stdio};
    let candidates: [(&str, &[&str]); 2] = [
        ("wl-paste", &["--primary", "--no-newline"]),
        ("xclip", &["-o", "-selection", "primary"]),
    ];
    for (program, args) in candidates {
        let Ok(mut child) = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        // 没选区时 `wl-paste` 会一直阻塞等数据：给一个上限，然后杀掉。
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                _ => {
                    let _ = child.kill();
                    break;
                }
            }
        }
        if let Some(output) = child.stdout.take() {
            use std::io::Read;
            let mut text = String::new();
            let mut output = output;
            let _ = output.read_to_string(&mut text);
            let text = text.trim_end_matches(['\n', '\r']).to_string();
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }
    None
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
                // 插件拿不到（焦点不在 IME 上下文）时，用合成器 PRIMARY 兜底：
                // 面板能出来、结果能看能复制，但替换会被明确拒绝（见 `apply`）。
                let (text, primary_only) = if text.trim().is_empty() {
                    match read_primary_selection() {
                        Some(primary) => (primary, true),
                        None => (text, false),
                    }
                } else {
                    (text, false)
                };
                if let Err(error) = validate_selection_replacement_source(&text) {
                    let _ = bridge.cancel_target(session_id);
                    return Err(error);
                }
                *target = Some(LinuxSelectionTarget {
                    session_id,
                    source_text: text.clone(),
                    primary_only,
                    replacement_text: None,
                });
                // Honest `source_app`: fcitx5 exposes the surrounding text via
                // DBus but gives the host no reliable foreground-application
                // identity that holds across both X11 and Wayland clients, and
                // the PRIMARY/clipboard cannot prove the original control is
                // still the current focus. There is no IBus/global-hotkey
                // fallback by design (Linux supports fcitx5 only), so we report
                // the app as unknown instead of faking an identity. Post-insertion
                // edit observation for streamed dictation is likewise unsupported
                // on Linux: Core's Noop HostContextAdapter/EditObservationAdapter
                // (the factory does not inject real ones) keep those use-cases on
                // the explicit `Unsupported` path rather than simulating edits.
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
            validate_selection_replacement_source(&source_text)?;
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
                if active.primary_only {
                    // 选区只从 PRIMARY 读到：这个窗口没有可写回的输入上下文。
                    // 明确拒绝，而不是假装成功或往别的窗口写字。
                    return Err(BackendError::new(
                        BackendErrorCode::Unsupported,
                        "This window cannot be written back (the selection came from the system PRIMARY buffer). Copy the result instead.",
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
