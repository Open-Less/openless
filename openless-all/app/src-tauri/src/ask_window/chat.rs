//! Host bindings for the standalone chat. No main-window or legacy QA state enters this surface.

use openless_core::ask::{self, AskAttachmentMetadata, AskTurnKey, StandaloneQaService};
use openless_core::ask_attachment::{
    AttachmentContent, AttachmentError, AttachmentKind, AttachmentMetadata,
    AttachmentPreviewSummary, AttachmentReader as _, AttachmentRegistry, AttachmentScope,
    AttachmentState, ImageMime, SelectedText,
};
use openless_core::{BackendError, BackendErrorCode, OpenLessBackend};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, EventTarget, Manager, State, WebviewWindow};
use tauri_plugin_shell::ShellExt;
use uuid::Uuid;

use super::attachment_files::{decode_dropped_image, pick_attachment_file, NativeAttachmentFile};
use super::{context::DesktopContext, wire, LABEL};

pub(crate) struct AskHost {
    service: StandaloneQaService,
    registry: Arc<AttachmentRegistry>,
    pub(crate) context: Arc<DesktopContext>,
    attachment_gate: Mutex<()>,
    backend: Arc<OpenLessBackend>,
    context_preference: Option<PathBuf>,
    pending_notice: Mutex<Option<Value>>,
}

impl AskHost {
    pub(crate) fn new(backend: Arc<OpenLessBackend>) -> Self {
        let registry = Arc::new(AttachmentRegistry::default());
        let context = Arc::new(DesktopContext::default());
        let context_preference = crate::persistence::data_dir()
            .ok()
            .map(|dir| dir.join("ask-app-context.json"));
        let allowed = context_preference
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|data| serde_json::from_slice::<bool>(&data).ok())
            .unwrap_or(false);
        context.set_authorization(openless_core::ask_context::ContextAuthorization {
            app_metadata: allowed,
            usage: false,
        });
        let service = backend
            .create_standalone_ask(context.clone(), Arc::new(AttachmentPort(registry.clone())));
        Self {
            service,
            registry,
            context,
            attachment_gate: Mutex::new(()),
            backend,
            context_preference,
            pending_notice: Mutex::new(None),
        }
    }

    pub(crate) fn open(&self) -> Result<Uuid, BackendError> {
        self.backend.ensure_runtime_ready()?;
        let _guard = self.attachment_gate.lock();
        let id = self.service.open();
        self.initialize_scope(id);
        Ok(id)
    }

    fn initialize_scope(&self, id: Uuid) {
        self.registry.open_conversation(&id.to_string());
        self.context.open_conversation(id);
    }

    pub(crate) fn current(&self) -> Option<Uuid> {
        self.service.current()
    }

    fn require_current(&self, id: Uuid) -> Result<(), BackendError> {
        self.backend.ensure_runtime_ready()?;
        if self.current() == Some(id) {
            Ok(())
        } else {
            Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "This Ask conversation is no longer active.",
            ))
        }
    }

    fn scope(&self, id: Uuid) -> Result<AttachmentScope, BackendError> {
        self.require_current(id)?;
        self.registry
            .scope(&id.to_string())
            .map_err(attachment_error)
    }

    pub(crate) fn register_selection(
        &self,
        id: Uuid,
        text: String,
        source_app: Option<String>,
    ) -> Result<(), BackendError> {
        let _guard = self.attachment_gate.lock();
        let metadata = self
            .scope(id)?
            .register_selection(SelectedText::new(text, source_app).map_err(attachment_error)?)
            .map_err(attachment_error)?;
        self.service
            .publish_attachment(id, attachment_metadata(&metadata)?, true)
    }

    pub(crate) fn stop(&self) {
        let _guard = self.attachment_gate.lock();
        let current = self.current();
        self.service.end_all();
        self.registry.clear();
        if let Some(id) = current {
            self.context.close_conversation(id);
        }
    }

    pub(crate) fn bind_feed(app: AppHandle) {
        let Some(host) = app.try_state::<Arc<Self>>() else {
            return;
        };
        let mut events = host.service.subscribe();
        tauri::async_runtime::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        let Some(host) = app.try_state::<Arc<Self>>() else {
                            return;
                        };
                        if host.current() == Some(event.conversation_id) {
                            let _ = app.emit_to(
                                EventTarget::webview_window(LABEL),
                                "ask:event",
                                wire::event(&event),
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    fn add_file(
        self: &Arc<Self>,
        id: Uuid,
        file: NativeAttachmentFile,
    ) -> Result<AttachmentMetadata, BackendError> {
        let job = {
            let _guard = self.attachment_gate.lock();
            let job = file.register(&self.scope(id)?).map_err(attachment_error)?;
            self.service
                .publish_attachment(id, attachment_metadata(job.metadata())?, true)?;
            job
        };
        let metadata = job.metadata().clone();
        let host = self.clone();
        tauri::async_runtime::spawn(async move {
            let finished = job.process().await;
            let _guard = host.attachment_gate.lock();
            if let Some(metadata) = finished.commit() {
                if let Ok(metadata) = attachment_metadata(&metadata) {
                    let _ = host.service.publish_attachment(id, metadata, false);
                }
            }
        });
        Ok(metadata)
    }
}

struct AttachmentPort(Arc<AttachmentRegistry>);

impl ask::AttachmentReader for AttachmentPort {
    fn acquire(
        &self,
        turn: AskTurnKey,
        ids: &[Uuid],
    ) -> Result<Vec<ask::AskAttachment>, BackendError> {
        let ids: Vec<_> = ids.iter().map(Uuid::to_string).collect();
        let lease = self
            .0
            .lease(
                &turn.conversation_id.to_string(),
                &turn.turn_id.to_string(),
                &ids,
            )
            .map_err(attachment_error)?;
        Ok(lease
            .attachments()
            .iter()
            .map(|entry| match entry.content() {
                AttachmentContent::Selection(text) => ask::AskAttachment::Selection {
                    text: Arc::from(text.text()),
                },
                AttachmentContent::Pdf(pdf) => ask::AskAttachment::Pdf {
                    pages: pdf
                        .pages()
                        .iter()
                        .map(|page| ask::AskPdfPage {
                            number: page.page_number,
                            text: Arc::from(page.text.as_str()),
                        })
                        .collect(),
                },
                AttachmentContent::Image(image) => {
                    let (width, height) = image.dimensions();
                    ask::AskAttachment::Image(ask::AskImage {
                        mime: match image.mime() {
                            ImageMime::Png => ask::AskImageMime::Png,
                            ImageMime::Jpeg => ask::AskImageMime::Jpeg,
                        },
                        bytes: Arc::from(image.bytes()),
                        width,
                        height,
                    })
                }
            })
            .collect())
    }

    fn end_conversation(&self, id: Uuid) {
        self.0.end_conversation(&id.to_string());
    }
}

fn attachment_metadata(value: &AttachmentMetadata) -> Result<AskAttachmentMetadata, BackendError> {
    Ok(AskAttachmentMetadata {
        attachment_id: Uuid::parse_str(&value.attachment_id)
            .map_err(|_| BackendError::new(BackendErrorCode::Internal, "Invalid attachment ID."))?,
        kind: match value.kind {
            AttachmentKind::Image => ask::AskAttachmentKind::Image,
            AttachmentKind::Pdf => ask::AskAttachmentKind::Pdf,
            AttachmentKind::Selection => ask::AskAttachmentKind::Selection,
        },
        display_name: value.display_name.clone(),
        preview: value.preview.as_ref().map(|preview| match preview {
            AttachmentPreviewSummary::Selection { text, has_more } => {
                ask::AskAttachmentPreview::Selection {
                    excerpt: text.clone(),
                    has_more: *has_more,
                }
            }
            AttachmentPreviewSummary::Image {
                width,
                height,
                thumbnail_available,
            } => ask::AskAttachmentPreview::Image {
                width: *width,
                height: *height,
                thumbnail_available: *thumbnail_available,
            },
        }),
        page_count: value.page_count,
        source_app: value.source_app.clone(),
        state: match value.state {
            AttachmentState::Processing => ask::AskAttachmentStatus::Processing,
            AttachmentState::Ready => ask::AskAttachmentStatus::Ready,
            AttachmentState::Error => ask::AskAttachmentStatus::Error,
        },
        error: value
            .error
            .as_ref()
            .map(|error| attachment_error(error.code)),
    })
}

fn attachment_error(error: AttachmentError) -> BackendError {
    let code = match error {
        AttachmentError::Cancelled => BackendErrorCode::Cancelled,
        AttachmentError::PickerBusy | AttachmentError::CapacityExceeded => BackendErrorCode::Busy,
        AttachmentError::NotFound | AttachmentError::NotReady => BackendErrorCode::InvalidState,
        AttachmentError::PdfUnavailable | AttachmentError::PickerUnavailable => {
            BackendErrorCode::Unsupported
        }
        _ => BackendErrorCode::InvalidArgument,
    };
    BackendError::new(code, error.to_string())
}

fn authorize(window: &WebviewWindow) -> Result<(), Value> {
    if window.label() == LABEL {
        Ok(())
    } else {
        Err(
            json!({"code": "permission_denied", "message": "This command belongs to the Ask window."}),
        )
    }
}

type AskState<'a> = State<'a, Arc<AskHost>>;
type IpcResult = Result<Value, Value>;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitEnvelope {
    #[serde(flatten)]
    request: ask::AskSubmit,
    voice_recording_id: Option<openless_core::SessionId>,
}

#[tauri::command]
pub fn ask_current(window: WebviewWindow, host: AskState<'_>) -> IpcResult {
    authorize(&window)?;
    Ok(json!({
        "conversationId": host.current(),
        "themeMode": host.backend.get_preferences().theme_mode,
        "platform": std::env::consts::OS,
        "notice": host.pending_notice.lock().take(),
    }))
}

#[tauri::command]
pub fn ask_new_conversation(window: WebviewWindow, host: AskState<'_>) -> IpcResult {
    authorize(&window)?;
    host.backend.ensure_runtime_ready().map_err(wire::error)?;
    let _guard = host.attachment_gate.lock();
    let conversation_id = host.service.new_conversation();
    host.initialize_scope(conversation_id);
    if let Some(desktop) = window.try_state::<Arc<super::Desktop>>() {
        desktop.set_conversation(Some(conversation_id.to_string()));
    }
    let _ = window.emit_to(
        EventTarget::webview_window(LABEL),
        "ask:window",
        json!({"type":"activated"}),
    );
    Ok(json!({"conversationId": conversation_id}))
}

#[tauri::command]
pub fn ask_end_conversation(
    window: WebviewWindow,
    host: AskState<'_>,
    conversation_id: Uuid,
) -> IpcResult {
    authorize(&window)?;
    let _guard = host.attachment_gate.lock();
    host.require_current(conversation_id).map_err(wire::error)?;
    host.service
        .end_conversation(conversation_id)
        .map_err(wire::error)?;
    host.context.close_conversation(conversation_id);
    if let Some(desktop) = window.try_state::<Arc<super::Desktop>>() {
        desktop.set_conversation(None);
    }
    let _ = window.emit_to(
        EventTarget::webview_window(LABEL),
        "ask:window",
        json!({"type":"activated"}),
    );
    Ok(Value::Null)
}

#[tauri::command]
pub fn ask_snapshot(window: WebviewWindow, host: AskState<'_>, conversation_id: Uuid) -> IpcResult {
    authorize(&window)?;
    host.require_current(conversation_id).map_err(wire::error)?;
    host.service
        .snapshot(conversation_id)
        .map(wire::snapshot)
        .map_err(wire::error)
}

#[tauri::command]
pub fn ask_events(
    window: WebviewWindow,
    host: AskState<'_>,
    conversation_id: Uuid,
    after_sequence: u64,
) -> IpcResult {
    authorize(&window)?;
    host.require_current(conversation_id).map_err(wire::error)?;
    let replay = host
        .service
        .events(conversation_id, after_sequence)
        .map_err(wire::error)?;
    Ok(
        json!({"events": replay.events.iter().map(wire::event).collect::<Vec<_>>(), "latestSequence":replay.latest_sequence, "truncated":replay.truncated}),
    )
}

#[tauri::command]
pub async fn ask_submit(
    window: WebviewWindow,
    host: AskState<'_>,
    request: SubmitEnvelope,
) -> IpcResult {
    authorize(&window)?;
    let SubmitEnvelope {
        request,
        voice_recording_id,
    } = request;
    host.require_current(request.conversation_id)
        .map_err(wire::error)?;
    let conversation_id = request.conversation_id;
    let used_attachments = request.attachment_ids.clone();
    let desktop = window
        .try_state::<Arc<super::Desktop>>()
        .map(|state| state.inner().clone());
    let proof = voice_recording_id.map(|id| (id, request.clone()));
    let turn_id = host
        .service
        .submit_guarded(request, || {
            proof.as_ref().is_none_or(|(id, request)| {
                desktop.as_ref().is_some_and(|desktop| {
                    desktop.current_voice_delivery().is_some_and(|delivery| {
                        let editor = delivery.target.editor;
                        delivery.recording_id == *id
                            && delivery.mode == super::voice::Mode::Submit
                            && editor.conversation_id == request.conversation_id.to_string()
                            && request.client_request_id == id.to_string()
                            && delivery.text.trim() == request.text
                            && editor.attachment_ids.iter().eq(request
                                .attachment_ids
                                .iter()
                                .map(Uuid::to_string)
                                .collect::<Vec<_>>()
                                .iter())
                            && editor.options.deep_thinking == request.options.deep_thinking
                            && editor.options.web_search == request.options.web_search
                            && request.options.public_search_query.is_none()
                    })
                })
            })
        })
        .await
        .map_err(wire::error)?;
    let _guard = host.attachment_gate.lock();
    if let Ok(scope) = host.registry.scope(&conversation_id.to_string()) {
        for id in used_attachments {
            scope.remove(&id.to_string());
        }
    }
    Ok(json!({"turnId":turn_id}))
}

#[tauri::command]
pub fn ask_cancel(
    window: WebviewWindow,
    host: AskState<'_>,
    conversation_id: Uuid,
    turn_id: Uuid,
) -> IpcResult {
    authorize(&window)?;
    host.require_current(conversation_id).map_err(wire::error)?;
    host.service
        .cancel(AskTurnKey {
            conversation_id,
            turn_id,
        })
        .map_err(wire::error)?;
    Ok(Value::Null)
}

#[tauri::command]
pub async fn ask_add_attachment(
    window: WebviewWindow,
    host: AskState<'_>,
    conversation_id: Uuid,
) -> IpcResult {
    authorize(&window)?;
    host.require_current(conversation_id).map_err(wire::error)?;
    let Some(file) = pick_attachment_file(&window)
        .await
        .map_err(attachment_error)
        .map_err(wire::error)?
    else {
        return Ok(Value::Null);
    };
    Ok(json!(host
        .inner()
        .add_file(conversation_id, file)
        .map_err(wire::error)?))
}

#[tauri::command]
pub async fn ask_add_dropped_image(
    window: WebviewWindow,
    host: AskState<'_>,
    conversation_id: Uuid,
    display_name: String,
    data_base64: String,
) -> IpcResult {
    authorize(&window)?;
    host.require_current(conversation_id).map_err(wire::error)?;
    let file = decode_dropped_image(&display_name, &data_base64)
        .map_err(attachment_error)
        .map_err(wire::error)?;
    Ok(json!(host
        .inner()
        .add_file(conversation_id, file)
        .map_err(wire::error)?))
}

#[tauri::command]
pub fn ask_remove_attachment(
    window: WebviewWindow,
    host: AskState<'_>,
    conversation_id: Uuid,
    attachment_id: Uuid,
) -> IpcResult {
    authorize(&window)?;
    let _guard = host.attachment_gate.lock();
    let scope = host.scope(conversation_id).map_err(wire::error)?;
    if scope.remove(&attachment_id.to_string()) {
        host.service
            .publish_attachment_removed(conversation_id, attachment_id)
            .map_err(wire::error)?;
    }
    Ok(Value::Null)
}

#[tauri::command]
pub fn ask_get_attachment_preview(
    window: WebviewWindow,
    host: AskState<'_>,
    conversation_id: Uuid,
    attachment_id: Uuid,
) -> IpcResult {
    authorize(&window)?;
    let scope = host.scope(conversation_id).map_err(wire::error)?;
    Ok(json!(scope
        .preview(&attachment_id.to_string())
        .map_err(attachment_error)
        .map_err(wire::error)?))
}

#[tauri::command]
pub fn ask_open_source(
    window: WebviewWindow,
    host: AskState<'_>,
    conversation_id: Uuid,
    turn_id: Uuid,
    source_id: String,
) -> IpcResult {
    authorize(&window)?;
    host.require_current(conversation_id).map_err(wire::error)?;
    let url = host
        .service
        .source_url(
            AskTurnKey {
                conversation_id,
                turn_id,
            },
            &source_id,
        )
        .map_err(wire::error)?;
    #[allow(deprecated)]
    window
        .shell()
        .open(url, None)
        .map_err(|_| json!({"code":"platform", "message":"Could not open this source."}))?;
    Ok(Value::Null)
}

pub(crate) struct DesktopPorts {
    pub(crate) app: AppHandle,
    pub(crate) ask: Arc<AskHost>,
    pub(crate) coordinator: Arc<crate::coordinator::Coordinator>,
}

impl super::Integration for DesktopPorts {
    fn prepare_open(
        &self,
        invocation: super::capture::Invocation,
    ) -> super::Task<Result<(), String>> {
        let app = self.app.clone();
        let ask = self.ask.clone();
        Box::pin(async move {
            let conversation = ask.open().map_err(|error| error.message)?;
            if let Some(desktop) = app.try_state::<Arc<super::Desktop>>() {
                desktop.set_conversation(Some(conversation.to_string()));
            }
            if let Some(source) = &invocation.foreground_app {
                if ask.context.app_metadata_allowed() {
                    let context = ask.context.clone();
                    let pid = invocation.foreground_pid;
                    let invocation_id = invocation.invocation_id;
                    let (send, receive) = tokio::sync::oneshot::channel();
                    let scheduled = app.run_on_main_thread(move || {
                        let original = || {
                            super::capture::foreground().map(|app| app.pid) == pid && pid.is_some()
                        };
                        let captured = original()
                            .then(|| context.capture_before_activation(invocation_id))
                            .flatten()
                            .filter(|_| original());
                        let _ = send.send(captured);
                    });
                    let captured = match scheduled {
                        Ok(()) => {
                            tokio::time::timeout(std::time::Duration::from_millis(180), receive)
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .flatten()
                        }
                        Err(_) => None,
                    };
                    if let Some(capture) = captured {
                        ask.context.record_invocation(conversation, capture);
                    } else {
                        ask.context.record_external_app(
                            conversation,
                            invocation_id,
                            invocation.captured_at,
                            source.name.as_deref(),
                            source.bundle_id.as_deref(),
                        );
                    }
                }
            }
            let source_app = invocation.foreground_app.and_then(|source| source.name);
            let selection_result = match invocation.selection {
                super::capture::Selection::Text(text) => {
                    ask.register_selection(conversation, text, source_app)
                }
                super::capture::Selection::TooLarge => {
                    Err(attachment_error(AttachmentError::SelectionTooLarge))
                }
                super::capture::Selection::None => Ok(()),
            };
            if let Err(error) = selection_result {
                *ask.pending_notice.lock() = Some(wire::error(error));
            }
            Ok(())
        })
    }

    fn ordinary_dictation_edge(&self, edge: openless_core::DictationHotkeyEdge) -> super::Task<()> {
        let coordinator = self.coordinator.clone();
        Box::pin(async move {
            coordinator.dispatch_ask_passthrough(edge).await;
        })
    }

    fn ordinary_dictation_is_active(&self) -> bool {
        matches!(
            self.coordinator.backend().snapshot().dictation.phase,
            openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording
        )
    }

    fn capsule(&self, frame: openless_core::CapsulePayload) {
        self.coordinator.present_ask_capsule(frame);
    }
}

#[tauri::command]
pub fn ask_window_hide(
    window: WebviewWindow,
    desktop: State<'_, Arc<super::Desktop>>,
) -> IpcResult {
    authorize(&window)?;
    desktop
        .hide(&window)
        .map_err(|message| json!({"code":"platform","message":message}))?;
    Ok(Value::Null)
}

#[tauri::command]
pub fn ask_sync_editor(
    window: WebviewWindow,
    desktop: State<'_, Arc<super::Desktop>>,
    editor: super::voice::Editor,
) -> IpcResult {
    authorize(&window)?;
    if editor.mount_id.len() > 128
        || editor.editor_id.len() > 128
        || editor.attachment_ids.len() > 4
    {
        return Err(json!({"code":"invalid_argument","message":"Invalid Ask editor state."}));
    }
    Ok(json!(
        desktop.update_composer(desktop.focus_epoch(), Some(editor))
    ))
}

#[tauri::command]
pub fn ask_ack_voice(
    window: WebviewWindow,
    desktop: State<'_, Arc<super::Desktop>>,
    recording_id: openless_core::SessionId,
) -> IpcResult {
    authorize(&window)?;
    Ok(json!(desktop.acknowledge_voice_delivery(recording_id)))
}

#[tauri::command]
pub fn ask_get_context_permission(window: WebviewWindow, host: AskState<'_>) -> IpcResult {
    if window.label() != "main" {
        return Err(
            json!({"code":"permission_denied","message":"Open Ask settings in the main window."}),
        );
    }
    Ok(json!(host.context.app_metadata_allowed()))
}

#[tauri::command]
pub fn ask_set_context_permission(
    window: WebviewWindow,
    host: AskState<'_>,
    allowed: bool,
) -> IpcResult {
    if window.label() != "main" {
        return Err(
            json!({"code":"permission_denied","message":"Open Ask settings in the main window."}),
        );
    }
    let _guard = host.attachment_gate.lock();
    let grant = |enabled| {
        host.context
            .set_authorization(openless_core::ask_context::ContextAuthorization {
                app_metadata: enabled,
                usage: false,
            })
    };
    if !allowed {
        grant(false);
        host.service.cancel_active_turns();
    }
    let stored = host.context_preference.as_ref().is_some_and(|path| {
        crate::persistence::atomic_write(path, if allowed { b"true" } else { b"false" }).is_ok()
    });
    if !stored {
        return Err(
            json!({"code":"persistence","message":"Could not save the Ask context permission."}),
        );
    }
    if allowed {
        grant(true);
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_submission_carries_separate_recording_and_conversation_ids() {
        let conversation = Uuid::new_v4();
        let recording = Uuid::new_v4();
        let payload: SubmitEnvelope = serde_json::from_value(json!({
            "conversationId": conversation,
            "clientRequestId": recording,
            "voiceRecordingId": recording,
            "text": "hi",
            "attachmentIds": [],
            "options": { "deepThinking": false, "webSearch": false }
        }))
        .unwrap();
        assert_eq!(payload.request.conversation_id, conversation);
        assert_eq!(payload.request.client_request_id, recording.to_string());
        assert_eq!(payload.voice_recording_id.unwrap().as_uuid(), recording);
    }
}
