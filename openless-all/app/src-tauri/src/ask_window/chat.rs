//! Host bindings for the standalone chat. No main-window or legacy QA state enters this surface.

use openless_core::ask::{self, AskAttachmentMetadata, AskTurnKey, StandaloneQaService};
use openless_core::ask_attachment::{
    AttachmentContent, AttachmentError, AttachmentKind, AttachmentMetadata,
    AttachmentPreviewSummary, AttachmentReader as _, AttachmentRegistry, AttachmentScope,
    AttachmentState, ImageMime,
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
use super::entry_settings::{AskEntryIntentMode, AskEntrySettings, AskEntrySettingsStore};
use super::target::{self, TargetTracker};
use super::{context::DesktopContext, wire, LABEL};

pub(crate) struct AskHost {
    service: StandaloneQaService,
    registry: Arc<AttachmentRegistry>,
    pub(crate) context: Arc<DesktopContext>,
    attachment_gate: Mutex<()>,
    backend: Arc<OpenLessBackend>,
    context_preference: Option<PathBuf>,
    entry_settings: Arc<AskEntrySettingsStore>,
    targets: Arc<TargetTracker>,
    original: Mutex<Option<(u64, target::WriteTarget)>>,
}

impl AskHost {
    pub(crate) fn new(app: &AppHandle, backend: Arc<OpenLessBackend>) -> Self {
        let registry = Arc::new(AttachmentRegistry::default());
        let context = Arc::new(DesktopContext::default());
        let context_preference = crate::persistence::data_dir()
            .ok()
            .map(|dir| dir.join("ask-app-context.json"));
        let entry_settings = Arc::new(AskEntrySettingsStore::load(
            app.path()
                .app_local_data_dir()
                .ok()
                .map(|dir| dir.join(AskEntrySettingsStore::FILE_NAME)),
        ));
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
        let targets = Arc::new(TargetTracker::new());
        if let Err(error) = targets.start() {
            log::warn!("[ask] external target observation unavailable: {error}");
        }
        Self {
            service,
            registry,
            context,
            attachment_gate: Mutex::new(()),
            backend,
            context_preference,
            entry_settings,
            targets,
            original: Mutex::new(None),
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

    async fn prepare_submission(
        &self,
        desktop: Option<std::sync::Weak<super::Desktop>>,
        generation: Option<u64>,
    ) -> Result<ask::AskPreparation, BackendError> {
        let committed_settings = self.entry_settings.read().map_err(|_| {
            BackendError::new(BackendErrorCode::Persistence, "askEntrySettingsUnavailable")
        })?;
        let intent_mode = match committed_settings.intent_mode {
            AskEntryIntentMode::SemanticHybrid => ask::AskIntentMode::SemanticHybrid,
            AskEntryIntentMode::SeparateModel => ask::AskIntentMode::SeparateModel,
            AskEntryIntentMode::MainModelTool => ask::AskIntentMode::MainModelTool,
        };
        let locked = self.targets.capture().await;
        let selection = match locked.selection {
            target::ExternalSelection::None => ask::ExternalSelection::None,
            target::ExternalSelection::Selected(text) => ask::ExternalSelection::Selected(text),
            target::ExternalSelection::Unavailable => ask::ExternalSelection::Unavailable,
        };
        let original = self.original.lock().as_ref()
            .filter(|(epoch, _)| Some(*epoch) == generation)
            .map(|(_, target)| target.clone());
        let insert = if cfg!(target_os = "macos") {
            original
        } else {
            locked.write_target.clone()
        };
        let replace = locked.write_target;
        let writer = (insert.is_some() || replace.is_some()).then(|| {
            Arc::new(TargetWriter {
                tracker: self.targets.clone(),
                insert,
                replace,
                desktop,
                generation,
            }) as Arc<dyn ask::AskWriter>
        });
        Ok(ask::AskPreparation {
            intent_mode,
            selection,
            writer,
            clipboard: (!cfg!(target_os = "macos")).then(|| Arc::new(Clipboard) as Arc<dyn ask::AskClipboard>),
        })
    }

    pub(crate) fn stop(&self) {
        let _guard = self.attachment_gate.lock();
        let current = self.current();
        self.service.end_all();
        self.targets.stop();
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

struct TargetWriter {
    tracker: Arc<TargetTracker>,
    insert: Option<target::WriteTarget>,
    replace: Option<target::WriteTarget>,
    desktop: Option<std::sync::Weak<super::Desktop>>,
    generation: Option<u64>,
}

impl ask::AskWriter for TargetWriter {
    fn has_original_caret(&self) -> bool {
        cfg!(target_os = "macos") && self.insert.is_some()
    }

    fn write(
        &self,
        operation: ask::AskWriteOperation,
        text: String,
        claim: ask::AskWriteClaim,
    ) -> futures_util::future::BoxFuture<'_, ask::AskWriteOutcome> {
        let (target, operation) = match operation {
            ask::AskWriteOperation::Insert => (self.insert.as_ref(), target::WriteOperation::Insert),
            ask::AskWriteOperation::Replace => (self.replace.as_ref(), target::WriteOperation::Replace),
        };
        Box::pin(async move {
            let Some(target) = target else {
                return ask::AskWriteOutcome::NotWritten;
            };
            let desktop = self.desktop.clone();
            let generation = self.generation;
            let current = move || {
                !cfg!(target_os = "macos") || generation.is_some_and(|generation| {
                    desktop.as_ref().and_then(std::sync::Weak::upgrade)
                        .is_some_and(|desktop| desktop.external_target_generation() == Some(generation))
                })
            };
            if !current() {
                return ask::AskWriteOutcome::NotWritten;
            }
            match self
                .tracker
                .deliver(target, operation, text, Box::new(move || current() && claim()))
                .await
            {
                target::WriteOutcome::Written => ask::AskWriteOutcome::Written,
                target::WriteOutcome::NotWritten => ask::AskWriteOutcome::NotWritten,
                target::WriteOutcome::Unknown => ask::AskWriteOutcome::Unknown,
                target::WriteOutcome::Declined => ask::AskWriteOutcome::Declined,
            }
        })
    }
}

struct Clipboard;

impl ask::AskClipboard for Clipboard {
    fn copy(&self, text: String) -> futures_util::future::BoxFuture<'_, Result<(), BackendError>> {
        Box::pin(async move {
            let copy = tauri::async_runtime::spawn_blocking(move || {
                crate::insertion::copy_text_to_clipboard(&text)
            })
            .await;
            if matches!(copy, Ok(Ok(()))) {
                Ok(())
            } else {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "Could not copy the Ask result.",
                ))
            }
        })
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

pub(crate) fn restricted_command_allowed(label: &str, command: &str) -> bool {
    label != LABEL
        || matches!(
            command,
            "ask_current"
                | "ask_snapshot"
                | "ask_events"
                | "ask_cancel"
                | "ask_open_source"
                | "ask_window_hide"
                | "ask_expand_chat"
                | "ask_new_conversation"
                | "ask_end_conversation"
                | "ask_submit"
                | "ask_add_attachment"
                | "ask_add_dropped_image"
                | "ask_remove_attachment"
                | "ask_get_attachment_preview"
                | "ask_sync_editor"
                | "ask_ack_voice"
        )
}

type AskState<'a> = State<'a, Arc<AskHost>>;
type IpcResult = Result<Value, Value>;

#[tauri::command]
pub fn ask_current(window: WebviewWindow, host: AskState<'_>) -> IpcResult {
    authorize(&window)?;
    let mut status = json!({
        "conversationId": host.current(),
        "themeMode": host.backend.get_preferences().theme_mode,
        "platform": std::env::consts::OS,
    });
    if window.label() == LABEL {
        if let Some(desktop) = window.try_state::<Arc<super::Desktop>>() {
            let (epoch, request) = desktop.composer_focus();
            status["focusEpoch"] = json!(epoch);
            status["focusRequest"] = json!(request);
        }
    }
    Ok(status)
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
    request: ask::AskSubmit,
) -> IpcResult {
    authorize(&window)?;
    host.require_current(request.conversation_id)
        .map_err(wire::error)?;
    let conversation_id = request.conversation_id;
    let used_attachments = request.attachment_ids.clone();
    let desktop = window.try_state::<Arc<super::Desktop>>();
    let generation = desktop.as_ref().and_then(|desktop| desktop.external_target_generation());
    let desktop = desktop.as_ref().map(|desktop| Arc::downgrade(&**desktop));
    let turn_id = host
        .service
        .submit_prepared(request, || host.prepare_submission(desktop, generation))
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
    fn auto_voice_on_open(&self) -> bool {
        self.ask
            .entry_settings
            .read()
            .is_ok_and(|settings| settings.auto_voice_on_open)
    }

    fn prepare_open(
        &self,
        invocation: super::capture::Invocation,
        generation: u64,
    ) -> super::Task<Result<(), String>> {
        let app = self.app.clone();
        let ask = self.ask.clone();
        Box::pin(async move {
            let conversation = ask.open().map_err(|error| error.message)?;
            if let Some(desktop) = app.try_state::<Arc<super::Desktop>>() {
                desktop.set_conversation(Some(conversation.to_string()));
            }
            if cfg!(target_os = "macos") {
                let target = match invocation.foreground_pid {
                    Some(pid) => ask.targets.capture_before_focus(pid).await.write_target,
                    None => None,
                };
                if app.try_state::<Arc<super::Desktop>>().is_some_and(|desktop| {
                    desktop.external_target_generation() == Some(generation)
                }) {
                    *ask.original.lock() = target.map(|target| (generation, target));
                }
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
            Ok(())
        })
    }

    fn ordinary_dictation_edge(
        &self,
        edge: openless_core::DictationHotkeyEdge,
        fn_commit: Option<super::hotkey::FnCommit>,
    ) -> super::Task<()> {
        let coordinator = self.coordinator.clone();
        Box::pin(async move {
            if let (Some(commit), openless_core::DictationHotkeyEdge::Pressed { press_id, .. }) =
                (fn_commit, edge)
            {
                let backend = coordinator.backend();
                if let Err(error) = backend.dispatch_arbitrated_dictation_hotkey_press(edge, || {
                    super::hotkey::fn_stroke_is_current(press_id, commit)
                }).await {
                    log::warn!("[ask] ordinary Fn dispatch failed: {error}");
                }
                return;
            }
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
pub async fn ask_expand_chat(
    window: WebviewWindow,
    host: AskState<'_>,
    desktop: State<'_, Arc<super::Desktop>>,
    conversation_id: Uuid,
    turn_id: Uuid,
) -> IpcResult {
    authorize(&window)?;
    host.require_current(conversation_id).map_err(wire::error)?;
    let snapshot = host
        .service
        .snapshot(conversation_id)
        .map_err(wire::error)?;
    if !snapshot.submission.is_some_and(|submission| {
        submission.current_turn_id == turn_id
            && matches!(
                submission.route,
                Some(ask::AskRoute::Answer | ask::AskRoute::SelectionAnswer)
            )
    }) {
        return Err(
            json!({"code":"invalid_state","message":"This request is not an Ask conversation."}),
        );
    }
    desktop
        .inner()
        .expand_chat(window, conversation_id)
        .await
        .map_err(|message| json!({"code":"platform","message":message}))?;
    Ok(Value::Null)
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
    focus_epoch: u64,
    focus_request: u64,
    editor: super::voice::Editor,
) -> IpcResult {
    authorize(&window)?;
    if editor.mount_id.len() > 128 || editor.editor_id.len() > 128 {
        return Err(json!({"code":"invalid_argument","message":"Invalid Ask editor state."}));
    }
    Ok(json!(desktop.update_composer(
        focus_epoch,
        focus_request,
        editor
    )))
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

fn settings_reply(result: Result<AskEntrySettings, String>) -> IpcResult {
    result
        .map(|settings| json!(settings))
        .map_err(|_| json!({"code":"persistence","message":"Could not access Ask settings."}))
}

#[tauri::command]
pub fn ask_get_entry_settings(window: WebviewWindow, host: AskState<'_>) -> IpcResult {
    if window.label() != "main" {
        return Err(
            json!({"code":"permission_denied","message":"Open Ask settings in the main window."}),
        );
    }
    settings_reply(host.entry_settings.read())
}

#[tauri::command]
pub async fn ask_set_entry_settings(
    window: WebviewWindow,
    host: AskState<'_>,
    settings: super::entry_settings::AskEntrySettingsPatch,
) -> IpcResult {
    if window.label() != "main" {
        return Err(
            json!({"code":"permission_denied","message":"Open Ask settings in the main window."}),
        );
    }
    let store = host.entry_settings.clone();
    let result = tauri::async_runtime::spawn_blocking(move || store.write(settings))
        .await
        .unwrap_or_else(|error| Err(error.to_string()));
    settings_reply(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_window_is_limited_to_its_own_commands() {
        for command in [
            "ask_snapshot",
            "ask_expand_chat",
            "ask_window_hide",
            "ask_submit",
        ] {
            assert!(restricted_command_allowed(LABEL, command));
        }
        for command in [
            "ask_get_entry_settings",
            "ask_set_entry_settings",
            "copy_text_to_clipboard",
        ] {
            assert!(!restricted_command_allowed(LABEL, command));
            assert!(restricted_command_allowed("main", command));
        }
    }
}
