//! Supplied-text-only selection port: Core builds the existing selection prompt
//! and returns its preview. This port cannot capture or replace foreground text.
use super::*;
use openless_core::domains::{SelectionCapture, SelectionRuntimeAdapter};

pub(crate) struct SuppliedTextRuntime;
impl SelectionRuntimeAdapter for SuppliedTextRuntime {
    fn capture(
        &self,
        _session: SessionId,
        supplied_text: Option<String>,
    ) -> BoxFuture<'static, Result<SelectionCapture, BackendError>> {
        Box::pin(async move {
            let text = supplied_text
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| {
                    error(
                        BackendErrorCode::InvalidArgument,
                        "explicit rewrite text required",
                    )
                })?;
            Ok(SelectionCapture {
                text,
                source_app: None,
            })
        })
    }
    fn apply(
        &self,
        _session: SessionId,
        _source: String,
        _replacement: String,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        Box::pin(async {
            Err(error(
                BackendErrorCode::Unsupported,
                "rewrite never inserts text",
            ))
        })
    }
    fn revert(
        &self,
        _session: SessionId,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        Box::pin(async {
            Err(error(
                BackendErrorCode::Unsupported,
                "rewrite has no insertion to revert",
            ))
        })
    }
    fn cancel(&self, _session: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}
pub(crate) struct SelectionPreviewActions;
impl HostActions for SelectionPreviewActions {
    fn request(&self, action: HostAction) -> Result<(), BackendError> {
        match action {
            // The controlResult carries the preview; no desktop window exists.
            HostAction::ShowSelectionPreview
            | HostAction::HideSelectionPreview
            | HostAction::ShowDictationFeedback
            | HostAction::HideDictationFeedback => Ok(()),
            _ => Err(error(
                BackendErrorCode::Unsupported,
                "desktop host action unavailable on iOS",
            )),
        }
    }
}
