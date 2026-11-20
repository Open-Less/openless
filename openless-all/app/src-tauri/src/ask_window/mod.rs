pub(crate) mod attachment_files;
pub(crate) mod capture;
pub(crate) mod chat;
pub(crate) mod context;
pub(crate) mod entry_settings;
pub(crate) mod hotkey;
#[cfg(target_os = "macos")]
mod macos_window;
pub(crate) mod target;
#[cfg_attr(target_os = "macos", path = "voice_macos.rs")]
pub(crate) mod voice;
mod voice_types;
mod wire;

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use openless_core::{CapsulePayload, DictationHotkeyEdge, OpenLessBackend};
use parking_lot::Mutex;
#[cfg(not(target_os = "macos"))]
use tauri::{LogicalSize, Monitor, PhysicalPosition, PhysicalSize};
use tauri::{
    webview::PageLoadEvent, AppHandle, Emitter, EventTarget, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use capture::{Invocation, Trigger};

pub(crate) const LABEL: &str = "ask-anything";
const PAGE: &str = "ask-window/index.html";
const COMPOSER_WIDTH: f64 = 640.0;
const CONVERSATION_WIDTH: f64 = 800.0;
const COMPOSER_HEIGHT: f64 = 300.0;
const CONVERSATION_HEIGHT: f64 = 920.0;
const WINDOW_GAP: f64 = 12.0;
const COMPOSER_READY_TIMEOUT: Duration = Duration::from_secs(3);
static OPEN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) type Task<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub(crate) trait Integration: Send + Sync {
    fn prepare_open(&self, invocation: Invocation, generation: u64) -> Task<Result<(), String>>;
    fn auto_voice_on_open(&self) -> bool {
        true
    }
    fn private_session(&self) -> bool {
        false
    }
    fn revoke_requests(&self) {}
    fn ordinary_dictation_edge(&self, edge: DictationHotkeyEdge, fn_commit: Option<hotkey::FnCommit>) -> Task<()>;
    fn ordinary_dictation_is_active(&self) -> bool;
    fn capsule(&self, frame: CapsulePayload);
}

pub(crate) struct Desktop {
    app: AppHandle,
    ports: Arc<dyn Integration>,
    backend: Arc<OpenLessBackend>,
    focused: AtomicBool,
    invocation_epoch: AtomicU64,
    pending_open: AtomicU64,
    external_target_epoch: AtomicU64,
    #[cfg(target_os = "windows")]
    native_window: std::sync::atomic::AtomicUsize,
    current_conversation: Mutex<Option<String>>,
    expanded: AtomicBool,
    placement: Mutex<Option<Placement>>,
    composer: Mutex<ComposerFocus>,
    voice_result: Mutex<Option<voice::Delivery>>,
    recent_recordings: Mutex<VecDeque<openless_core::SessionId>>,
    voice: voice::Controller,
}

#[derive(Default)]
struct ComposerFocus {
    epoch: u64,
    request: u64,
    auto_voice: Option<AutoVoice>,
    target: Option<voice::Target>,
}

struct AutoVoice {
    request: u64,
    queued: bool,
    expires_at: Instant,
}

impl ComposerFocus {
    fn request_focus(&mut self, chord: Option<bool>, recording: bool) {
        self.expire_autostart();
        if chord.is_some() && self.auto_voice.is_some() {
            return;
        }
        self.request += 1;
        self.auto_voice = (chord == Some(true) && !recording).then_some(AutoVoice {
            request: self.request,
            queued: false,
            expires_at: Instant::now() + COMPOSER_READY_TIMEOUT,
        });
    }

    fn acknowledge(&mut self, epoch: u64, request: u64, editor: voice::Editor) -> Option<bool> {
        self.expire_autostart();
        if self.epoch != epoch
            || self.request != request
            || self.target.as_ref().is_some_and(|previous| {
                editor.mount_id != previous.editor.mount_id
                    || editor.editor_id != previous.editor.editor_id
                    || editor.revision < previous.editor.revision
                    || (editor.revision == previous.editor.revision && editor != previous.editor)
            })
        {
            return None;
        }
        let start = self
            .auto_voice
            .as_mut()
            .filter(|pending| !pending.queued && editor.editable)
            .is_some_and(|pending| {
                pending.queued = true;
                true
            });
        self.target = Some(voice::Target {
            focus_epoch: epoch,
            editor,
        });
        Some(start)
    }

    fn invalidate(&mut self) {
        self.epoch += 1;
        self.target = None;
        self.auto_voice = None;
    }

    fn expire_autostart(&mut self) {
        if self.auto_voice.as_ref().is_some_and(|pending| pending.expires_at <= Instant::now()) {
            self.auto_voice = None;
        }
    }
}

impl Desktop {
    pub(crate) fn install(
        app: AppHandle,
        backend: Arc<OpenLessBackend>,
        ports: Arc<dyn Integration>,
    ) -> Arc<Self> {
        let desktop = Arc::new(Self {
            app,
            ports,
            backend,
            focused: AtomicBool::new(false),
            invocation_epoch: AtomicU64::new(0),
            pending_open: Default::default(),
            external_target_epoch: AtomicU64::new(0),
            #[cfg(target_os = "windows")]
            native_window: std::sync::atomic::AtomicUsize::new(0),
            current_conversation: Mutex::new(None),
            expanded: AtomicBool::new(false),
            placement: Mutex::new(None),
            composer: Mutex::new(ComposerFocus::default()),
            voice_result: Mutex::new(None),
            recent_recordings: Mutex::new(VecDeque::new()),
            voice: voice::Controller::new(),
        });
        desktop.voice.listen(Arc::downgrade(&desktop));
        #[cfg(target_os = "macos")]
        macos_window::observe_spaces();
        hotkey::install(Arc::downgrade(&desktop));
        desktop
    }

    pub(crate) fn set_conversation(&self, conversation_id: Option<String>) {
        {
            let mut current = self.current_conversation.lock();
            if *current == conversation_id {
                return;
            }
            *current = conversation_id.clone();
            self.revoke_target();
        }
        let (focus_epoch, focus_request) = self.composer_focus();
        let _ = self.app.emit_to(
            EventTarget::webview_window(LABEL),
            "ask:window",
            serde_json::json!({
                "type": "sessionChanged", "conversationId": conversation_id,
                "focusEpoch": focus_epoch, "focusRequest": focus_request
            }),
        );
    }

    pub(crate) fn composer_focus(&self) -> (u64, u64) {
        let composer = self.composer.lock();
        (composer.epoch, composer.request)
    }

    pub(crate) fn update_composer(
        &self,
        focus_epoch: u64,
        focus_request: u64,
        editor: voice::Editor,
    ) -> bool {
        let conversation = self.current_conversation.lock();
        if conversation.as_ref() != Some(&editor.conversation_id) || !self.is_focused() {
            return false;
        }
        let mut composer = self.composer.lock();
        let Some(start) = composer.acknowledge(focus_epoch, focus_request, editor) else {
            return false;
        };
        if start {
            self.voice
                .autostart(focus_request, composer.target.clone().unwrap());
        }
        self.voice.revalidate();
        true
    }

    fn current_voice_delivery(&self) -> Option<voice::Delivery> {
        let delivery = self.voice_result.lock().clone()?;
        self.target_is_current(&delivery.target).then_some(delivery)
    }

    pub(crate) fn acknowledge_voice_delivery(
        &self,
        recording_id: openless_core::SessionId,
    ) -> bool {
        let delivery = {
            let mut pending = self.voice_result.lock();
            if !pending
                .as_ref()
                .is_some_and(|item| item.recording_id == recording_id)
            {
                return false;
            }
            pending.take().unwrap()
        };
        self.target_is_current(&delivery.target)
    }

    pub(crate) fn owns_capsule(&self) -> bool {
        !cfg!(target_os = "macos") && self.voice.owner().is_some()
    }

    pub(crate) fn capsule_stop(&self, recording_id: &str) -> bool {
        self.voice.capsule_action(recording_id, false)
    }

    pub(crate) fn capsule_cancel(&self, recording_id: &str) -> bool {
        self.voice.capsule_action(recording_id, true)
    }

    pub(crate) fn recognizes_capsule(&self, recording_id: &str) -> bool {
        !cfg!(target_os = "macos") && uuid::Uuid::parse_str(recording_id)
            .ok()
            .map(openless_core::SessionId::from_uuid)
            .is_some_and(|id| self.recent_recordings.lock().contains(&id))
    }

    fn remember_recording(&self, id: openless_core::SessionId) {
        let mut recent = self.recent_recordings.lock();
        recent.push_back(id);
        if recent.len() > 8 {
            recent.pop_front();
        }
    }

    pub(crate) fn cancel_active_voice(&self) -> bool {
        let pending = self.composer.lock().auto_voice.take().is_some();
        let delivery = self.voice_result.lock().take().is_some();
        if self.voice.owner().is_some() {
            self.voice.cancel();
            true
        } else {
            pending || delivery
        }
    }

    pub(crate) fn shutdown(&self) {
        hotkey::set_suspended(true);
        self.invocation_epoch.fetch_add(1, Ordering::AcqRel);
        self.pending_open.store(0, Ordering::Release);
        self.external_target_epoch.store(0, Ordering::Release);
        self.mark_unfocused();
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.is_native_target(capture::foreground().as_ref())
    }

    fn is_native_target(&self, foreground: Option<&capture::Foreground>) -> bool {
        self.focused.load(Ordering::Acquire)
            && foreground.is_some_and(|foreground| {
                if foreground.pid != std::process::id() {
                    return false;
                }
                #[cfg(target_os = "windows")]
                if self.native_window.load(Ordering::Acquire) != foreground.window {
                    return false;
                }
                true
            })
    }

    fn target_at_press(&self) -> Option<voice::Target> {
        self.composer.lock().target.clone()
    }

    fn target_is_current(&self, target: &voice::Target) -> bool {
        self.is_focused()
            && self
                .composer
                .lock()
                .target
                .as_ref()
                .is_some_and(|current| target.matches(current))
    }

    fn autostart_is_pending(&self, request: u64) -> bool {
        let mut composer = self.composer.lock();
        composer.expire_autostart();
        composer.auto_voice
            .as_ref()
            .is_some_and(|pending| pending.request == request && pending.queued)
    }

    fn complete_autostart(&self, request: u64) {
        let mut composer = self.composer.lock();
        if composer
            .auto_voice
            .as_ref()
            .is_some_and(|pending| pending.request == request)
        {
            composer.auto_voice = None;
        }
    }

    fn voice_is_pending(&self) -> bool {
        let mut composer = self.composer.lock();
        composer.expire_autostart();
        composer.auto_voice.is_some() || self.voice.owner().is_some()
    }

    fn revoke_target(&self) {
        self.composer.lock().invalidate();
        self.voice_result.lock().take();
        self.voice.cancel();
    }

    fn mark_unfocused(&self) {
        self.focused.store(false, Ordering::Release);
        self.revoke_target();
    }

    fn notify_frontend(&self, event: &str) {
        let payload = match event {
            "ask-window:shown" => {
                let (focus_epoch, focus_request) = self.composer_focus();
                serde_json::json!({
                    "type": "activated", "focusEpoch": focus_epoch, "focusRequest": focus_request
                })
            }
            #[cfg(not(target_os = "macos"))]
            "ask-window:voice-unavailable" => serde_json::json!({"type": "notice", "error": {
                "code": "voice_unavailable", "message": "Could not start voice input."
            }}),
            _ => return,
        };
        let _ = self
            .app
            .emit_to(EventTarget::webview_window(LABEL), "ask:window", payload);
    }

    pub(crate) fn hide(&self, window: &WebviewWindow) -> Result<(), String> {
        if window.label() != LABEL {
            return Err("Not an Ask window.".into());
        }
        self.invocation_epoch.fetch_add(1, Ordering::AcqRel);
        self.pending_open.store(0, Ordering::Release);
        self.external_target_epoch.store(0, Ordering::Release);
        self.ports.revoke_requests();
        let hidden = window.hide().map_err(|error| error.to_string());
        if self.app.try_state::<Arc<chat::AskHost>>().is_some_and(|host| host.clear_on_hide()) {
            self.set_conversation(None);
        }
        hidden?;
        self.expanded.store(false, Ordering::Release);
        self.mark_unfocused();
        let _ = self.app.emit_to(
            EventTarget::webview_window(LABEL),
            "ask:window",
            serde_json::json!({"type":"hidden"}),
        );
        Ok(())
    }

    fn dismiss_composer_from_escape(self: &Arc<Self>) {
        let focus = self.composer_focus();
        self.cancel_active_voice();
        let desktop = self.clone();
        let _ = self.app.run_on_main_thread(move || {
            if desktop.composer_focus() == focus && desktop.is_focused() {
                if let Some(window) = desktop.app.get_webview_window(LABEL) {
                    let _ = desktop.hide(&window);
                }
            }
        });
    }

    fn notify_voice(
        &self,
        event: &str,
        recording_id: openless_core::SessionId,
        target: &voice::Target,
        text: Option<&str>,
    ) {
        let mut payload = serde_json::json!({
            "type": event,
            "recordingId": recording_id,
            "conversationId": target.editor.conversation_id,
            "focusEpoch": target.focus_epoch,
            "mountId": target.editor.mount_id,
            "editorId": target.editor.editor_id,
            "revision": target.editor.revision,
            "selectionStart": target.editor.selection_start,
            "selectionEnd": target.editor.selection_end
        });
        if let Some(text) = text {
            payload[if event == "failed" { "stage" } else { "text" }] = serde_json::json!(text);
        }
        let _ = self
            .app
            .emit_to(EventTarget::webview_window(LABEL), "ask:voice", payload);
    }

    fn on_window_event(self: &Arc<Self>, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(true) => {
                self.focused.store(true, Ordering::Release);
                self.notify_frontend("ask-window:shown");
            }
            WindowEvent::Focused(false) | WindowEvent::Destroyed => self.mark_unfocused(),
            _ => {}
        }
    }

    pub(crate) async fn open(self: &Arc<Self>) -> Result<(), String> {
        let foreground = capture::foreground();
        let trigger = (!self.is_native_target(foreground.as_ref()))
            .then(|| Trigger::at_key_event(foreground));
        self.open_with(trigger, None, self.begin_open()).await
    }

    fn begin_open(&self) -> u64 {
        let generation = self.invocation_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.pending_open.fetch_max(generation, Ordering::AcqRel);
        self.external_target_epoch.store(0, Ordering::Release);
        self.ports.revoke_requests();
        generation
    }

    pub(super) fn opening(&self) -> bool {
        self.pending_open.load(Ordering::Acquire) != 0
    }

    fn cancel_pending_open(&self) {
        let pending = self.pending_open.swap(0, Ordering::AcqRel);
        if pending != 0 {
            let _ = self.invocation_epoch.compare_exchange(
                pending, pending + 1, Ordering::AcqRel, Ordering::Acquire,
            );
        }
    }

    pub(crate) fn external_target_generation(&self) -> Option<u64> {
        Some(self.external_target_epoch.load(Ordering::Acquire)).filter(|epoch| *epoch != 0)
    }

    #[cfg(target_os = "macos")]
    fn recover_after_write(self: &Arc<Self>, generation: u64, proof: target::FocusRecovery) {
        let desktop = self.clone();
        let _ = self.app.run_on_main_thread(move || {
            if desktop.external_target_generation() != Some(generation) || desktop.opening()
                || !proof.is_current()
            {
                return;
            }
            if let Some(window) = desktop.app.get_webview_window(LABEL)
                .filter(|window| window.is_visible().unwrap_or(false))
            {
                let _ = focus_composer(&window);
            }
        });
    }

    fn warm_dictation(&self) {
        let backend = self.backend.clone();
        tauri::async_runtime::spawn(async move {
            backend.warm_dictation_provider().await;
        });
    }

    async fn open_with(
        self: &Arc<Self>,
        trigger: Option<Trigger>,
        chord: Option<bool>,
        generation: u64,
    ) -> Result<(), String> {
        let _pending = PendingOpen(&self.pending_open, generation);
        let _opening = OPEN.lock().await;
        let is_latest = || self.invocation_epoch.load(Ordering::Acquire) == generation;
        if !is_latest() || (chord.is_some() && trigger.is_none() && !self.is_focused()) {
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        let placement = (trigger.is_some() || self.placement.lock().is_none())
            .then(|| Placement::at_cursor(&self.app));
        #[cfg(target_os = "macos")]
        let placement = trigger.as_ref().map(|trigger| Placement(trigger.screen))
            .or_else(|| self.placement.lock().is_none().then(|| Placement::at_cursor(&self.app)));
        #[cfg(target_os = "macos")]
        let original_foreground = trigger.as_ref().and_then(|trigger| trigger.foreground.as_ref().map(|app| app.pid));
        let reset = chord.is_some() || trigger.is_some() || self.ports.private_session();
        if reset {
            self.expanded.store(false, Ordering::Release);
        }
        if trigger.is_some() || reset {
            self.external_target_epoch.store(generation, Ordering::Release);
            let invocation = if let Some(trigger) = trigger {
                let captured_at = trigger.captured_at;
                let captured_pid = trigger.foreground.as_ref().map(|app| app.pid);
                match tokio::time::timeout(
                    Duration::from_millis(450),
                    tauri::async_runtime::spawn_blocking(move || trigger.read()),
                ).await {
                    Ok(Ok(invocation)) => invocation,
                    _ => {
                        let mut invocation = Invocation::empty(captured_at);
                        invocation.foreground_pid = captured_pid
                            .filter(|pid| *pid != std::process::id());
                        invocation
                    }
                }
            } else {
                Invocation::empty(chrono::Utc::now())
            };
            if !is_latest() {
                return Ok(());
            }
            self.ports.prepare_open(invocation, generation).await?;
        }
        if !is_latest() {
            return Ok(());
        }
        let chord = chord.map(|start| start && self.ports.auto_voice_on_open());
        let window = ensure_window(&self.app, self)?;
        let desktop = self.clone();
        on_ui_thread(window, move |window| {
            if desktop.invocation_epoch.load(Ordering::Acquire) != generation {
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            if placement.as_ref().is_some_and(|place| !place.0.is_current())
                || original_foreground.is_some_and(|pid| foreground_pid() != Some(pid))
            {
                desktop.external_target_epoch.store(0, Ordering::Release);
                return Ok(());
            }
            let was_visible = window.is_visible().map_err(|error| error.to_string())?;
            if let Some(placement) = placement {
                *desktop.placement.lock() = Some(placement);
            }
            if reset || !was_visible || !desktop.is_focused() {
                let placement = desktop.placement.lock().clone().unwrap_or_default();
                placement.position(&window, desktop.expanded.load(Ordering::Acquire))?;
            }
            desktop
                .composer
                .lock()
                .request_focus(chord, desktop.voice.owner().is_some());
            if !desktop.is_focused() {
                if let Err(error) = focus_composer(&window) {
                    desktop.composer.lock().auto_voice = None;
                    return Err(error);
                }
            }
            desktop.notify_frontend("ask-window:shown");
            if cfg!(target_os = "macos") && chord.is_some() {
                desktop.backend.retain_dictation_warmth();
            }
            Ok(())
        })
        .await
    }

    pub(crate) async fn expand_chat(
        self: &Arc<Self>,
        window: WebviewWindow,
        conversation_id: uuid::Uuid,
    ) -> Result<(), String> {
        let conversation_id = conversation_id.to_string();
        let generation = self.invocation_epoch.load(Ordering::Acquire);
        let desktop = self.clone();
        on_ui_thread(window, move |window| {
            if desktop.invocation_epoch.load(Ordering::Acquire) != generation
                || desktop.current_conversation.lock().as_ref() != Some(&conversation_id)
                || !window.is_visible().map_err(|error| error.to_string())?
            {
                return Err("Ask is no longer shown for this request.".into());
            }
            if desktop.expanded.load(Ordering::Acquire) {
                return Ok(());
            }
            desktop
                .placement
                .lock()
                .clone()
                .unwrap_or_default()
                .position(&window, true)?;
            desktop.expanded.store(true, Ordering::Release);
            Ok(())
        })
        .await
    }
}

struct PendingOpen<'a>(&'a AtomicU64, u64);

impl Drop for PendingOpen<'_> {
    fn drop(&mut self) {
        let _ = self.0.compare_exchange(self.1, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[derive(Clone, Default)]
#[cfg(not(target_os = "macos"))]
struct Placement(Option<Monitor>);

#[cfg(not(target_os = "macos"))]
impl Placement {
    fn at_cursor(app: &AppHandle) -> Self {
        Self(
            app.cursor_position()
                .ok()
                .and_then(|position| {
                    app.monitor_from_point(position.x, position.y)
                        .ok()
                        .flatten()
                })
                .or_else(|| app.primary_monitor().ok().flatten()),
        )
    }

    fn position(&self, window: &WebviewWindow, expanded: bool) -> Result<(), String> {
        let (nominal_width, nominal_height) = if expanded {
            (CONVERSATION_WIDTH, CONVERSATION_HEIGHT)
        } else {
            (COMPOSER_WIDTH, COMPOSER_HEIGHT)
        };
        let Some(monitor) = &self.0 else {
            return window
                .set_size(LogicalSize::new(nominal_width, nominal_height))
                .map_err(|error| error.to_string());
        };
        let work = monitor.work_area();
        let scale = monitor.scale_factor();
        let size: LogicalSize<f64> = work.size.to_logical(scale);
        let gap = WINDOW_GAP.min(size.height / 24.0);
        let fit = ((size.width - gap * 2.0) / nominal_width)
            .min((size.height - gap * 2.0) / nominal_height)
            .clamp(0.01, 1.0);
        let width = nominal_width * fit;
        let height = nominal_height * fit;
        window
            .set_position(PhysicalPosition::new(
                work.position.x + ((size.width - width) * scale / 2.0).round() as i32,
                work.position.y + ((size.height - gap - height) * scale).round() as i32,
            ))
            .and_then(|()| {
                window.set_size(PhysicalSize::new(
                    (width * scale).floor() as u32,
                    (height * scale).floor() as u32,
                ))
            })
            .map_err(|error| error.to_string())
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Default)]
struct Placement(macos_window::ScreenPoint);

#[cfg(target_os = "macos")]
impl Placement {
    fn at_cursor(_app: &AppHandle) -> Self {
        Self(macos_window::ScreenPoint::capture())
    }

    fn position(&self, window: &WebviewWindow, expanded: bool) -> Result<(), String> {
        macos_window::position(window, self.0, expanded)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn active_space_epoch() -> u64 {
    macos_window::space_epoch()
}

#[cfg(target_os = "macos")]
pub(crate) fn foreground_pid() -> Option<u32> {
    capture::foreground().map(|app| app.pid)
}

fn ensure_window(app: &AppHandle, desktop: &Arc<Desktop>) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let page_owner = Arc::downgrade(desktop);
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(PAGE.into()))
        .title("OpenLess — Ask Anything")
        .inner_size(COMPOSER_WIDTH, COMPOSER_HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .disable_drag_drop_handler()
        .center()
        .visible(false)
        .focused(false)
        .on_page_load(move |_window, payload| {
            if payload.event() == PageLoadEvent::Started {
                if let Some(desktop) = page_owner.upgrade() {
                    let has_editor = desktop.composer.lock().target.is_some();
                    if has_editor {
                        desktop.revoke_target();
                    }
                }
            }
        })
        .build()
        .map_err(|error| error.to_string())?;
    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        desktop
            .native_window
            .store(hwnd.0 as usize, Ordering::Release);
    }
    let desktop = Arc::downgrade(desktop);
    let handle = window.clone();
    window.on_window_event(move |event| {
        let desktop = desktop.upgrade();
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(desktop) = desktop.as_ref() {
                let _ = desktop.hide(&handle);
            } else {
                let _ = handle.hide();
            }
            return;
        }
        if let Some(desktop) = desktop {
            desktop.on_window_event(event);
        }
    });
    Ok(window)
}

async fn on_ui_thread(
    window: WebviewWindow,
    action: impl FnOnce(WebviewWindow) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(action(target));
        })
        .map_err(|error| error.to_string())?;
    rx.await.map_err(|error| error.to_string())?
}

fn focus_composer(window: &WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos_window::focus(window);
    #[cfg(not(target_os = "macos"))]
    window
        .show()
        .and_then(|()| window.unminimize())
        .and_then(|()| window.set_focus())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn ask_window_open(window: WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Ask can only be opened from the main window or its shortcut.".into());
    }
    let desktop = window
        .try_state::<Arc<Desktop>>()
        .ok_or("Ask Desktop is not initialized")?;
    desktop.open().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(revision: u64, editable: bool) -> voice::Editor {
        voice::Editor {
            conversation_id: "conversation".into(),
            mount_id: "mount".into(),
            editor_id: "editor".into(),
            revision,
            editable,
            selection_start: 2,
            selection_end: 2,
        }
    }

    #[test]
    fn only_a_current_editable_chord_ack_arms_the_capsule_once() {
        let mut composer = ComposerFocus::default();
        composer.request_focus(Some(true), false);
        let epoch = composer.epoch;
        let request = composer.request;
        assert_eq!(
            composer.acknowledge(epoch, request - 1, editor(1, true)),
            None
        );
        assert_eq!(
            composer.acknowledge(epoch, request, editor(1, false)),
            Some(false)
        );
        composer.request_focus(Some(false), false);
        assert_eq!(composer.request, request);
        assert_eq!(
            composer.acknowledge(epoch, request, editor(2, true)),
            Some(true)
        );
        assert_eq!(
            composer.acknowledge(epoch, request, editor(2, true)),
            Some(false)
        );
        composer.invalidate();
        assert_eq!(composer.acknowledge(epoch, request, editor(3, true)), None);
    }

    #[test]
    fn menu_disabled_setting_and_active_capsule_can_only_refocus() {
        for (chord, recording) in [(None, false), (Some(false), false), (Some(true), true)] {
            let mut composer = ComposerFocus::default();
            composer.request_focus(chord, recording);
            assert!(composer.auto_voice.is_none());
            assert_eq!(
                composer.acknowledge(composer.epoch, composer.request, editor(1, true)),
                Some(false)
            );
        }
    }

    #[test]
    fn expired_focus_ack_cannot_start_a_recording() {
        let mut composer = ComposerFocus::default();
        composer.request_focus(Some(true), false);
        composer.auto_voice.as_mut().unwrap().expires_at = Instant::now();
        assert_eq!(composer.acknowledge(composer.epoch, composer.request, editor(1, true)), Some(false));
    }
}
