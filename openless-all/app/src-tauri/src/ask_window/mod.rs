pub(crate) mod attachment_files;
pub(crate) mod capture;
pub(crate) mod chat;
pub(crate) mod context;
pub(crate) mod hotkey;
pub(crate) mod voice;
mod wire;

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use openless_core::{CapsulePayload, DictationHotkeyEdge, OpenLessBackend};
use parking_lot::Mutex;
use tauri::{
    AppHandle, Emitter, EventTarget, LogicalSize, Manager, PhysicalPosition, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use capture::{Invocation, Trigger};

pub(crate) const LABEL: &str = "ask-anything";
const PAGE: &str = "ask-window/index.html";
const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 920.0;
static OPEN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) type Task<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub(crate) trait Integration: Send + Sync {
    fn prepare_open(&self, invocation: Invocation) -> Task<Result<(), String>>;
    fn ordinary_dictation_edge(&self, edge: DictationHotkeyEdge) -> Task<()>;
    fn ordinary_dictation_is_active(&self) -> bool;
    fn capsule(&self, frame: CapsulePayload);
}

pub(crate) struct Desktop {
    app: AppHandle,
    ports: Arc<dyn Integration>,
    backend: Arc<OpenLessBackend>,
    focused: AtomicBool,
    focus_epoch: AtomicU64,
    invocation_epoch: AtomicU64,
    #[cfg(target_os = "windows")]
    native_window: std::sync::atomic::AtomicUsize,
    current_conversation: Mutex<Option<String>>,
    composer: Mutex<Option<voice::Target>>,
    voice_result: Mutex<Option<voice::Delivery>>,
    recent_recordings: Mutex<VecDeque<openless_core::SessionId>>,
    voice: voice::Controller,
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
            focus_epoch: AtomicU64::new(0),
            invocation_epoch: AtomicU64::new(0),
            #[cfg(target_os = "windows")]
            native_window: std::sync::atomic::AtomicUsize::new(0),
            current_conversation: Mutex::new(None),
            composer: Mutex::new(None),
            voice_result: Mutex::new(None),
            recent_recordings: Mutex::new(VecDeque::new()),
            voice: voice::Controller::new(),
        });
        desktop.voice.listen(Arc::downgrade(&desktop));
        hotkey::install(Arc::downgrade(&desktop));
        desktop
    }

    pub(crate) fn set_conversation(&self, conversation_id: Option<String>) {
        let mut current = self.current_conversation.lock();
        if *current != conversation_id {
            *current = conversation_id;
            self.revoke_target();
        }
    }

    pub(crate) fn focus_epoch(&self) -> u64 {
        self.focus_epoch.load(Ordering::Acquire)
    }

    pub(crate) fn update_composer(
        &self,
        expected_focus_epoch: u64,
        editor: Option<voice::Editor>,
    ) -> bool {
        let conversation = self.current_conversation.lock();
        if self.focus_epoch() != expected_focus_epoch {
            return false;
        }
        match editor {
            Some(editor)
                if conversation.as_ref() == Some(&editor.conversation_id) && self.is_focused() =>
            {
                let mut composer = self.composer.lock();
                if self.focus_epoch() != expected_focus_epoch {
                    return false;
                }
                let changed = composer.as_ref().is_some_and(|target| {
                    target.editor.editor_id != editor.editor_id
                        || target.editor.mount_id != editor.mount_id
                });
                if changed {
                    self.voice.cancel();
                    self.voice_result.lock().take();
                    self.focus_epoch.fetch_add(1, Ordering::AcqRel);
                }
                if composer
                    .as_ref()
                    .is_some_and(|previous| !changed && editor.revision < previous.editor.revision)
                {
                    return false;
                }
                *composer = Some(voice::Target {
                    focus_epoch: self.focus_epoch.load(Ordering::Acquire),
                    editor,
                });
                self.voice.revalidate();
                true
            }
            Some(_) => false,
            None => {
                self.revoke_target();
                true
            }
        }
    }

    pub(crate) fn current_voice_delivery(&self) -> Option<voice::Delivery> {
        let delivery = self.voice_result.lock().clone()?;
        self.target_is_current(&delivery.target, delivery.mode)
            .then_some(delivery)
    }

    pub(crate) fn accepts_voice_delivery(
        &self,
        recording_id: openless_core::SessionId,
        conversation_id: &str,
        mode: voice::Mode,
    ) -> bool {
        self.current_voice_delivery().is_some_and(|delivery| {
            delivery.recording_id == recording_id
                && delivery.mode == mode
                && delivery.target.editor.conversation_id == conversation_id
        })
    }

    pub(crate) fn acknowledge_voice_delivery(
        &self,
        recording_id: openless_core::SessionId,
    ) -> bool {
        let mut pending = self.voice_result.lock();
        if pending
            .as_ref()
            .is_some_and(|pending| pending.recording_id == recording_id)
        {
            pending.take();
            return true;
        }
        false
    }

    pub(crate) fn platform(&self) -> &'static str {
        std::env::consts::OS
    }

    pub(crate) fn owns_capsule(&self) -> bool {
        self.voice.owner().is_some()
    }

    pub(crate) fn capsule_stop(&self, recording_id: &str) -> bool {
        self.voice.capsule_action(recording_id, false)
    }

    pub(crate) fn capsule_cancel(&self, recording_id: &str) -> bool {
        self.voice.capsule_action(recording_id, true)
    }

    pub(crate) fn recognizes_capsule(&self, recording_id: &str) -> bool {
        uuid::Uuid::parse_str(recording_id)
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
        if self.owns_capsule() {
            self.voice.cancel();
            true
        } else {
            false
        }
    }

    pub(crate) fn shutdown(&self) {
        hotkey::set_suspended(true);
        self.invocation_epoch.fetch_add(1, Ordering::AcqRel);
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
        self.composer.lock().clone()
    }

    fn target_is_current(&self, target: &voice::Target, mode: voice::Mode) -> bool {
        self.is_focused()
            && self
                .composer
                .lock()
                .as_ref()
                .is_some_and(|current| target.matches(current, mode))
    }

    fn revoke_target(&self) {
        self.focus_epoch.fetch_add(1, Ordering::AcqRel);
        self.composer.lock().take();
        self.voice_result.lock().take();
        self.voice.cancel();
    }

    fn mark_unfocused(&self) {
        self.focused.store(false, Ordering::Release);
        self.revoke_target();
    }

    fn notify_frontend(&self, event: &str) {
        let payload = match event {
            "ask-window:shown" => serde_json::json!({"type": "activated"}),
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
        window.hide().map_err(|error| error.to_string())?;
        self.mark_unfocused();
        let _ = self.app.emit_to(
            EventTarget::webview_window(LABEL),
            "ask:window",
            serde_json::json!({"type":"hidden"}),
        );
        Ok(())
    }

    fn notify_voice(
        &self,
        event: &str,
        recording_id: openless_core::SessionId,
        target: &voice::Target,
        mode: voice::Mode,
        text: Option<&str>,
    ) {
        let mut payload = serde_json::json!({
            "type": event,
            "recordingId": recording_id,
            "conversationId": target.editor.conversation_id,
            "mode": mode
        });
        if let Some(text) = text {
            payload["text"] = serde_json::json!(text);
        }
        let _ = self
            .app
            .emit_to(EventTarget::webview_window(LABEL), "ask:voice", payload);
    }

    fn on_window_event(self: &Arc<Self>, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(true) => {
                self.focused.store(true, Ordering::Release);
                self.notify_frontend("ask-window:focused");
            }
            WindowEvent::Focused(false) | WindowEvent::Destroyed => self.mark_unfocused(),
            _ => {}
        }
    }

    pub(crate) async fn open(self: &Arc<Self>) -> Result<(), String> {
        let foreground = capture::foreground();
        let trigger = (!self.is_native_target(foreground.as_ref()))
            .then(|| Trigger::at_key_event(foreground));
        self.open_with(trigger).await
    }

    async fn open_with(self: &Arc<Self>, trigger: Option<Trigger>) -> Result<(), String> {
        let generation = self.invocation_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let _opening = OPEN.lock().await;
        let is_latest = || self.invocation_epoch.load(Ordering::Acquire) == generation;
        if !is_latest() {
            return Ok(());
        }
        if let Some(trigger) = trigger {
            let captured_at = trigger.captured_at;
            let invocation = match tokio::time::timeout(
                Duration::from_millis(450),
                tauri::async_runtime::spawn_blocking(move || trigger.read()),
            )
            .await
            {
                Ok(Ok(invocation)) => invocation,
                _ => Invocation::empty(captured_at),
            };
            if !is_latest() {
                return Ok(());
            }
            self.ports.prepare_open(invocation).await?;
        }
        if !is_latest() {
            return Ok(());
        }
        let window = ensure_window(&self.app, self)?;
        show(window).await?;
        self.notify_frontend("ask-window:shown");
        Ok(())
    }
}

fn ensure_window(app: &AppHandle, desktop: &Arc<Desktop>) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|position| {
            app.monitor_from_point(position.x, position.y)
                .ok()
                .flatten()
        })
        .or_else(|| app.primary_monitor().ok().flatten());
    let scale = monitor
        .as_ref()
        .map(|monitor| {
            let work: LogicalSize<f64> =
                monitor.work_area().size.to_logical(monitor.scale_factor());
            (work.width / WIDTH).min(work.height / HEIGHT).min(1.0)
        })
        .unwrap_or(1.0);
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(PAGE.into()))
        .title("OpenLess — Ask Anything")
        .inner_size(WIDTH * scale, HEIGHT * scale)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .disable_drag_drop_handler()
        .center()
        .visible(false)
        .focused(false)
        .build()
        .map_err(|error| error.to_string())?;
    if let (Some(monitor), Ok(size)) = (monitor, window.outer_size()) {
        let work = monitor.work_area();
        let x = work.position.x + (work.size.width.saturating_sub(size.width) / 2) as i32;
        let y = work.position.y + (work.size.height.saturating_sub(size.height) / 2) as i32;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
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

async fn show(window: WebviewWindow) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let result = target.show().and_then(|()| {
                #[cfg(target_os = "macos")]
                {
                    use objc2::{
                        msg_send,
                        runtime::{AnyClass, AnyObject, Bool},
                    };
                    unsafe {
                        if let Some(class) = AnyClass::get("NSApplication") {
                            let app: *mut AnyObject = msg_send![class, sharedApplication];
                            let _: () = msg_send![app, activateIgnoringOtherApps: Bool::YES];
                        }
                    }
                }
                target.unminimize()?;
                target.set_focus()
            });
            let _ = tx.send(result.map_err(|error| error.to_string()));
        })
        .map_err(|error| error.to_string())?;
    rx.await.map_err(|error| error.to_string())?
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
