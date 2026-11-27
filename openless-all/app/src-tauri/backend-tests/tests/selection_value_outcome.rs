//! Production ValuePattern transaction -> delivery classification -> Core history.
//! Provider faults occur after mutating the bound control, not as canned backend errors.
#![allow(dead_code)]
mod types {
    pub use openless_core::shared_types::InsertStatus;
}
#[path = "../../src/insertion/clipboard_delivery.rs"]
mod clipboard_delivery;
#[path = "../../src/selection/direct_delivery.rs"]
mod direct_delivery;
#[path = "../../src/host_document/windows/selection_value.rs"]
mod selection_value;
#[path = "../../src/selection/target_position.rs"]
mod target_position;

use futures_util::future::BoxFuture;
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, InsertOutcome,
    NoopSettingsRuntime, OpenLessBackend, PolishMode, SelectionCapture, SelectionPhase,
    SelectionPolishOutputMode, SelectionPolishRequest, SelectionRuntimeAdapter, SessionId,
    SettingsUpdateOptions,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    Preflight,
    SetterAfterWrite,
    ReadbackError,
    ReadbackMismatch,
    TargetLost,
    TargetReadError,
    CancelledAfterWrite,
    None,
}

struct State {
    control: String,
    captured_identity: (u32, u64, u64), // PID, window, control
    identity: (u32, u64, u64),
    captured_position: Option<(u64, u64)>,
    position: Option<(u64, u64)>,
    result_position: (u64, u64),
    cancelled: bool,
    range_checks: usize,
    identity_checks: usize,
    setters: usize,
    readbacks: usize,
    applies: usize,
    clipboard: String,
    revision: u64,
    protected_revision: Option<u64>,
    copies: Vec<String>,
}

impl State {
    fn identity_current(&self) -> bool {
        !self.cancelled && self.identity == self.captured_identity
    }
}

#[derive(Clone)]
struct Clipboard(Arc<Mutex<State>>);
impl clipboard_delivery::ClipboardDelivery for Clipboard {
    fn text(&mut self) -> Result<Option<String>, String> {
        Ok(Some(self.0.lock().unwrap().clipboard.clone()))
    }
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        state.clipboard = text.into();
        state.revision += 1;
        state.copies.push(text.into());
        Ok(())
    }
    fn sequence(&self) -> Option<u64> {
        Some(self.0.lock().unwrap().revision)
    }
    fn paste(&mut self) -> Result<(), clipboard_delivery::DispatchError> {
        panic!("selection writes must not paste")
    }
    fn wait_for_consumption(&mut self) {
        panic!("selection writes must not paste")
    }
}

#[derive(Clone)]
struct Runtime {
    original: String,
    fault: Fault,
    state: Arc<Mutex<State>>,
    history: std::path::PathBuf,
}

impl SelectionRuntimeAdapter for Runtime {
    fn capture(
        &self,
        _: SessionId,
        _: Option<String>,
    ) -> BoxFuture<'static, Result<SelectionCapture, BackendError>> {
        let text = self.original.clone();
        Box::pin(async move {
            Ok(SelectionCapture {
                text,
                source_app: Some("ValuePattern fixture".into()),
            })
        })
    }

    fn protect_original(
        &self,
        _: SessionId,
        text: String,
    ) -> BoxFuture<'static, Result<Option<u64>, BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            let rows = openless_core::HistoryStore::at_path(this.history).list()?;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].raw_transcript, text);
            let revision =
                clipboard_delivery::copy_with(&mut Clipboard(this.state.clone()), &text, || true)
                    .unwrap();
            this.state.lock().unwrap().protected_revision = revision;
            Ok(revision)
        })
    }

    fn apply(
        &self,
        _: SessionId,
        original: String,
        replacement: String,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            this.state.lock().unwrap().applies += 1;
            direct_delivery::deliver(
                || {
                    direct_delivery::native_result(selection_value::replace(
                        || {
                            assert_eq!(this.state.lock().unwrap().control, original);
                            if this.fault == Fault::Preflight {
                                Err("selectionPolishTargetUnavailable".into())
                            } else {
                                Ok(())
                            }
                        },
                        || {
                            let mut state = this.state.lock().unwrap();
                            state.range_checks += 1;
                            state.identity_current()
                                && target_position::unchanged(
                                    state.captured_position,
                                    state.position,
                                )
                        },
                        || {
                            let mut state = this.state.lock().unwrap();
                            state.identity_checks += 1;
                            state.identity_current()
                        },
                        || {
                            let mut state = this.state.lock().unwrap();
                            state.setters += 1;
                            state.control = replacement.clone();
                            state.position = Some(state.result_position);
                            if this.fault == Fault::TargetLost {
                                state.identity.2 += 1;
                            }
                            if this.fault == Fault::CancelledAfterWrite {
                                state.cancelled = true;
                            }
                            if this.fault == Fault::SetterAfterWrite {
                                Err("provider timed out after applying")
                            } else {
                                Ok(())
                            }
                        },
                        || {
                            let mut state = this.state.lock().unwrap();
                            state.readbacks += 1;
                            match this.fault {
                                Fault::ReadbackError => Err("CurrentValue failed"),
                                Fault::ReadbackMismatch => {
                                    state.control = "provider changed value".into();
                                    Ok(state.control == replacement)
                                }
                                Fault::TargetReadError => Err("CompareElements failed"),
                                _ => Ok(state.control == replacement),
                            }
                        },
                    ))
                },
                |error| {
                    let revision = this.state.lock().unwrap().protected_revision;
                    let copied = clipboard_delivery::copy_if_current_with(
                        &mut Clipboard(this.state.clone()),
                        &replacement,
                        revision,
                        || !this.state.lock().unwrap().cancelled,
                    )
                    .unwrap();
                    direct_delivery::fallback_result(
                        error,
                        if copied {
                            Ok(InsertOutcome::CopiedFallback)
                        } else {
                            Err(BackendError::new(
                                BackendErrorCode::InvalidState,
                                "selectionResultInHistory",
                            ))
                        },
                    )
                },
            )
        })
    }

    fn revert(&self, _: SessionId) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        Box::pin(async { panic!("an uncertain write must never be rolled back") })
    }
    fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

async fn fixture(fault: Fault) -> (OpenLessBackend, Runtime, tempfile::TempDir, SessionId) {
    let directory = tempfile::Builder::new()
        .prefix("selection-value-outcome-")
        .tempdir()
        .unwrap();
    let original = if fault == Fault::None {
        "abcdef".to_string()
    } else {
        format!(
            " \t{} VALUE_MIDDLE {}\r\n ",
            "字".repeat(9000),
            "尾".repeat(9000)
        )
    };
    let position = Some((0, original.encode_utf16().count() as u64));
    let runtime = Runtime {
        state: Arc::new(Mutex::new(State {
            control: original.clone(),
            captured_identity: (11, 22, 33),
            identity: (11, 22, 33),
            captured_position: position,
            position,
            result_position: (3, 0),
            cancelled: false,
            range_checks: 0,
            identity_checks: 0,
            setters: 0,
            readbacks: 0,
            applies: 0,
            clipboard: String::new(),
            revision: 0,
            protected_revision: None,
            copies: Vec::new(),
        })),
        original,
        fault,
        history: directory.path().join("history.json"),
    };
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.selection_runtime = Some(Arc::new(runtime.clone()));
    dependencies.selection_polisher = Some(Arc::new(
        openless_core::testing::FixtureTextPolisher::successful("XYZ"),
    ));
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: directory.path().to_path_buf(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    let mut preferences = backend.get_preferences();
    preferences.selection_polish_output_mode = SelectionPolishOutputMode::PreviewConfirm;
    backend
        .update_settings(
            preferences,
            SettingsUpdateOptions::STRICT,
            &NoopSettingsRuntime,
        )
        .unwrap();
    backend.start().await.unwrap();
    let id = backend
        .services()
        .selection
        .begin_polish(SelectionPolishRequest {
            selected_text: None,
            mode: PolishMode::Formal,
            instruction: None,
        })
        .await
        .unwrap();
    (backend, runtime, directory, id)
}

#[tokio::test]
async fn post_submission_faults_remain_terminal_even_if_result_was_copied() {
    for fault in [
        Fault::SetterAfterWrite,
        Fault::ReadbackError,
        Fault::ReadbackMismatch,
        Fault::TargetLost,
        Fault::TargetReadError,
        Fault::CancelledAfterWrite,
    ] {
        for later_copy in [false, true] {
            let (backend, runtime, directory, id) = fixture(fault).await;
            if later_copy {
                // Even copying the *same* original text changes ownership.
                clipboard_delivery::copy_with(
                    &mut Clipboard(runtime.state.clone()),
                    &runtime.original,
                    || true,
                )
                .unwrap();
            }
            let error = backend
                .services()
                .selection
                .confirm(id, None)
                .await
                .unwrap_err();
            assert_eq!(error.code, BackendErrorCode::OutcomeUnknown, "{fault:?}");
            assert_eq!(error.message, "selectionPolishWriteUnconfirmed");
            assert_eq!(
                error.details,
                Some(serde_json::json!({
                    "original_saved": true,
                    "result_copied": !later_copy && fault != Fault::CancelledAfterWrite,
                }))
            );
            let snapshot = backend.services().selection.snapshot().await.unwrap();
            assert_eq!(snapshot.phase, SelectionPhase::Failed);
            assert_ne!(snapshot.insert_outcome, Some(InsertOutcome::CopiedFallback));
            let history = backend.list_history().unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].raw_transcript, runtime.original);
            assert_eq!(history[0].final_text, "XYZ");
            assert_eq!(
                history[0].error_code.as_deref(),
                Some("selectionPolishWriteUnconfirmed")
            );
            assert!(backend
                .services()
                .selection
                .confirm(id, None)
                .await
                .is_err());
            {
                let state = runtime.state.lock().unwrap();
                assert_eq!(state.setters, 1);
                assert_eq!(state.applies, 1, "no resubmission after an unknown write");
                assert_eq!(
                    state.readbacks,
                    usize::from(!matches!(
                        fault,
                        Fault::SetterAfterWrite | Fault::TargetLost | Fault::CancelledAfterWrite
                    ))
                );
                assert_eq!(
                    state.range_checks, 1,
                    "the original range is only a pre-write fence"
                );
                assert_eq!(
                    state.copies.len(),
                    if fault == Fault::CancelledAfterWrite && !later_copy {
                        1
                    } else {
                        2
                    }
                );
                assert_eq!(state.copies[0], runtime.original);
                assert_eq!(
                    state.clipboard,
                    if later_copy || fault == Fault::CancelledAfterWrite {
                        runtime.original.as_str()
                    } else {
                        "XYZ"
                    }
                );
                assert_eq!(
                    state.control,
                    if fault == Fault::ReadbackMismatch {
                        "provider changed value"
                    } else {
                        "XYZ"
                    }
                );
            }
            backend.shutdown().await.unwrap();
            directory.close().unwrap();
        }
    }
}

#[tokio::test]
async fn preflight_failure_preserves_classification_and_never_calls_the_setter() {
    let error = selection_value::replace(
        || Err("selectionPolishTargetUnavailable".into()),
        || panic!("preflight must stop the selection fence"),
        || panic!("preflight must stop the identity fence"),
        || -> Result<(), ()> { panic!("preflight must stop submission") },
        || -> Result<bool, ()> { panic!("no post-write read before submission") },
    )
    .unwrap_err();
    assert_eq!(
        direct_delivery::native_result(Err(error)).unwrap_err().code,
        BackendErrorCode::Platform
    );
    let (backend, runtime, directory, id) = fixture(Fault::Preflight).await;
    backend
        .services()
        .selection
        .confirm(id, None)
        .await
        .unwrap();
    assert_eq!(
        backend
            .services()
            .selection
            .snapshot()
            .await
            .unwrap()
            .insert_outcome,
        Some(InsertOutcome::CopiedFallback)
    );
    {
        let state = runtime.state.lock().unwrap();
        assert_eq!(state.setters, 0);
        assert_eq!(state.readbacks, 0);
        assert_eq!(state.control, runtime.original);
        assert_eq!(state.copies, [runtime.original.clone(), "XYZ".into()]);
    }
    backend.shutdown().await.unwrap();
    directory.close().unwrap();
}

#[tokio::test]
async fn confirmed_value_write_keeps_the_original_clipboard_and_history() {
    for result_position in [(3, 0), (0, 3)] {
        let (backend, runtime, directory, id) = fixture(Fault::None).await;
        {
            let mut state = runtime.state.lock().unwrap();
            assert_eq!(state.captured_position, Some((0, 6)));
            state.result_position = result_position;
        }
        backend
            .services()
            .selection
            .confirm(id, None)
            .await
            .unwrap();
        let snapshot = backend.services().selection.snapshot().await.unwrap();
        assert_eq!(snapshot.phase, SelectionPhase::Completed);
        assert_eq!(snapshot.insert_outcome, Some(InsertOutcome::Inserted));
        {
            let state = runtime.state.lock().unwrap();
            assert_eq!(state.setters, 1);
            assert_eq!(state.readbacks, 1);
            assert_eq!(state.range_checks, 1);
            assert_eq!(
                state.identity_checks, 2,
                "identity must fence both sides of readback"
            );
            assert_eq!(state.position, Some(result_position));
            assert_eq!(state.identity, state.captured_identity);
            assert_eq!(state.control, "XYZ");
            assert_eq!(state.copies, [runtime.original.clone()]);
            assert_eq!(state.clipboard, runtime.original);
        }
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].raw_transcript, runtime.original);
        assert_eq!(history[0].final_text, "XYZ");
        backend.shutdown().await.unwrap();
        directory.close().unwrap();
    }
}

#[tokio::test]
async fn moving_the_original_range_before_submission_blocks_the_setter() {
    let (backend, runtime, directory, id) = fixture(Fault::None).await;
    runtime.state.lock().unwrap().position = Some((1, 5));
    backend
        .services()
        .selection
        .confirm(id, None)
        .await
        .unwrap();
    assert_eq!(
        backend
            .services()
            .selection
            .snapshot()
            .await
            .unwrap()
            .insert_outcome,
        Some(InsertOutcome::CopiedFallback)
    );
    {
        let state = runtime.state.lock().unwrap();
        assert_eq!(state.range_checks, 1);
        assert_eq!(state.identity_checks, 0);
        assert_eq!(state.setters, 0);
        assert_eq!(state.readbacks, 0);
        assert_eq!(state.control, runtime.original);
        assert_eq!(state.identity, state.captured_identity);
    }
    backend.shutdown().await.unwrap();
    directory.close().unwrap();
}
