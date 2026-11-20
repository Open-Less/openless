pub(crate) mod attachment_files;
pub(crate) mod capture;
pub(crate) mod chat;
pub(crate) mod context;
pub(crate) mod entry_settings;
pub(crate) mod hotkey;
pub(crate) mod target;
pub(crate) mod voice;
mod wire;

use std::{
    collections::{HashSet, VecDeque},
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
    webview::PageLoadEvent, AppHandle, Emitter, EventTarget, LogicalSize, Manager, Monitor,
    PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use capture::{Invocation, Trigger};

pub(crate) const LABEL: &str = "ask-anything";
pub(crate) const CONVERSATION_LABEL: &str = "ask-conversation";
const PAGE: &str = "ask-window/index.html";
const WIDTH: f64 = 640.0;
const COMPOSER_HEIGHT: f64 = 300.0;
const CONVERSATION_HEIGHT: f64 = 600.0;
const WINDOW_GAP: f64 = 12.0;
static OPEN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) type Task<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub(crate) trait Integration: Send + Sync {
    fn prepare_open(&self, invocation: Invocation) -> Task<Result<(), String>>;
    fn auto_voice_on_open(&self) -> bool {
        true
    }
    fn ordinary_dictation_edge(&self, edge: DictationHotkeyEdge) -> Task<()>;
    fn ordinary_dictation_is_active(&self) -> bool;
    fn capsule(&self, frame: CapsulePayload);
}

pub(crate) struct Desktop {
    app: AppHandle,
    ports: Arc<dyn Integration>,
    backend: Arc<OpenLessBackend>,
    focused: AtomicBool,
    invocation_epoch: AtomicU64,
    #[cfg(target_os = "windows")]
    native_window: std::sync::atomic::AtomicUsize,
    current_conversation: Mutex<Option<String>>,
    revealed_turns: Mutex<HashSet<uuid::Uuid>>,
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
}

impl ComposerFocus {
    fn request_focus(&mut self, chord: Option<bool>, recording: bool) {
        if chord.is_some() && self.auto_voice.is_some() {
            return;
        }
        self.request += 1;
        self.auto_voice = (chord == Some(true) && !recording).then_some(AutoVoice {
            request: self.request,
            queued: false,
        });
    }

    fn acknowledge(&mut self, epoch: u64, request: u64, editor: voice::Editor) -> Option<bool> {
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
            #[cfg(target_os = "windows")]
            native_window: std::sync::atomic::AtomicUsize::new(0),
            current_conversation: Mutex::new(None),
            revealed_turns: Mutex::new(HashSet::new()),
            placement: Mutex::new(None),
            composer: Mutex::new(ComposerFocus::default()),
            voice_result: Mutex::new(None),
            recent_recordings: Mutex::new(VecDeque::new()),
            voice: voice::Controller::new(),
        });
        desktop.voice.listen(Arc::downgrade(&desktop));
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
            self.revealed_turns.lock().clear();
            self.revoke_target();
        }
        let (focus_epoch, focus_request) = self.composer_focus();
        for label in [LABEL, CONVERSATION_LABEL] {
            let _ = self.app.emit_to(
                EventTarget::webview_window(label),
                "ask:window",
                serde_json::json!({
                    "type": "sessionChanged", "conversationId": conversation_id,
                    "focusEpoch": focus_epoch, "focusRequest": focus_request
                }),
            );
        }
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
        let pending = self.composer.lock().auto_voice.take().is_some();
        let delivery = self.voice_result.lock().take().is_some();
        if self.owns_capsule() {
            self.voice.cancel();
            true
        } else {
            pending || delivery
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
        self.composer
            .lock()
            .auto_voice
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
        self.composer.lock().auto_voice.is_some() || self.owns_capsule()
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
        let label = window.label();
        if !matches!(label, LABEL | CONVERSATION_LABEL) {
            return Err("Not an Ask window.".into());
        }
        if label == LABEL {
            self.invocation_epoch.fetch_add(1, Ordering::AcqRel);
        }
        window.hide().map_err(|error| error.to_string())?;
        if label == LABEL {
            self.mark_unfocused();
        }
        let _ = self.app.emit_to(
            EventTarget::webview_window(label),
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
        self.open_with(trigger, None).await
    }

    async fn open_with(
        self: &Arc<Self>,
        trigger: Option<Trigger>,
        chord: Option<bool>,
    ) -> Result<(), String> {
        let generation = self.invocation_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let _opening = OPEN.lock().await;
        let is_latest = || self.invocation_epoch.load(Ordering::Acquire) == generation;
        if !is_latest() {
            return Ok(());
        }
        let placement = (trigger.is_some() || self.placement.lock().is_none())
            .then(|| Placement::at_cursor(&self.app));
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
        let chord = chord.map(|start| start && self.ports.auto_voice_on_open());
        let window = ensure_window(&self.app, self, LABEL)?;
        let desktop = self.clone();
        on_ui_thread(window, move |window| {
            if desktop.invocation_epoch.load(Ordering::Acquire) != generation {
                return Ok(());
            }
            if let Some(placement) = placement {
                placement.position(&window, LABEL)?;
                if let Some(reader) = desktop.app.get_webview_window(CONVERSATION_LABEL) {
                    placement.position(&reader, CONVERSATION_LABEL)?;
                }
                *desktop.placement.lock() = Some(placement);
            }
            desktop
                .composer
                .lock()
                .request_focus(chord, desktop.owns_capsule());
            if !desktop.is_focused() {
                if let Err(error) = focus_composer(&window) {
                    desktop.composer.lock().auto_voice = None;
                    return Err(error);
                }
            }
            desktop.notify_frontend("ask-window:shown");
            Ok(())
        })
        .await
    }

    pub(crate) async fn show_conversation_for_chat(
        self: &Arc<Self>,
        conversation_id: uuid::Uuid,
        turn_id: uuid::Uuid,
    ) -> Result<(), String> {
        let conversation_id = conversation_id.to_string();
        let _opening = OPEN.lock().await;
        {
            let current = self.current_conversation.lock();
            if current.as_ref() != Some(&conversation_id)
                || !self.revealed_turns.lock().insert(turn_id)
            {
                return Ok(());
            }
        }
        let window = ensure_window(&self.app, self, CONVERSATION_LABEL)?;
        let desktop = self.clone();
        on_ui_thread(window, move |window| {
            if desktop.current_conversation.lock().as_ref() != Some(&conversation_id) {
                return Ok(());
            }
            let placement = desktop
                .placement
                .lock()
                .clone()
                .unwrap_or_else(|| Placement::at_cursor(&desktop.app));
            placement.position(&window, CONVERSATION_LABEL)?;
            reveal_without_focus(&window)?;
            let _ = desktop.app.emit_to(
                EventTarget::webview_window(CONVERSATION_LABEL),
                "ask:window",
                serde_json::json!({"type": "activated"}),
            );
            Ok(())
        })
        .await
    }
}

#[derive(Clone, Default)]
struct Placement(Option<Monitor>);

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

    fn position(&self, window: &WebviewWindow, label: &str) -> Result<(), String> {
        let Some(monitor) = &self.0 else {
            return Ok(());
        };
        let work = monitor.work_area();
        let scale = monitor.scale_factor();
        let size: LogicalSize<f64> = work.size.to_logical(scale);
        let gap = WINDOW_GAP.min(size.height / 24.0);
        let width = WIDTH.min(size.width - gap * 2.0);
        let composer_height = COMPOSER_HEIGHT.min((size.height - gap * 3.0) * 0.5);
        let conversation_height =
            CONVERSATION_HEIGHT.min(size.height - composer_height - gap * 3.0);
        let (height, bottom) = if label == LABEL {
            (composer_height, gap)
        } else {
            (conversation_height, composer_height + gap * 2.0)
        };
        window
            .set_position(PhysicalPosition::new(
                work.position.x + ((size.width - width) * scale / 2.0).round() as i32,
                work.position.y + ((size.height - bottom - height) * scale).round() as i32,
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

fn ensure_window(
    app: &AppHandle,
    desktop: &Arc<Desktop>,
    label: &str,
) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }
    let composer = label == LABEL;
    let page_owner = Arc::downgrade(desktop);
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App(PAGE.into()))
        .title(if composer {
            "OpenLess — Ask Anything"
        } else {
            "OpenLess — Ask Conversation"
        })
        .inner_size(
            WIDTH,
            if composer {
                COMPOSER_HEIGHT
            } else {
                CONVERSATION_HEIGHT
            },
        )
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .disable_drag_drop_handler()
        .center()
        .visible(false)
        .focused(false)
        .on_page_load(move |_window, payload| {
            if composer && payload.event() == PageLoadEvent::Started {
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
    if composer {
        if let Ok(hwnd) = window.hwnd() {
            desktop
                .native_window
                .store(hwnd.0 as usize, Ordering::Release);
        }
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
        if composer {
            if let Some(desktop) = desktop {
                desktop.on_window_event(event);
            }
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
    window.show().map_err(|error| error.to_string())?;
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
    window
        .unminimize()
        .and_then(|()| window.set_focus())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn reveal_without_focus(window: &WebviewWindow) -> Result<(), String> {
    let handle = window.ns_window().map_err(|error| error.to_string())?;
    if handle.is_null() {
        return Err("Ask conversation window is unavailable.".into());
    }
    unsafe {
        let _: () = objc2::msg_send![
            handle.cast::<objc2::runtime::AnyObject>(),
            orderFrontRegardless
        ];
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn reveal_without_focus(window: &WebviewWindow) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };
    let handle = window.hwnd().map_err(|error| error.to_string())?;
    unsafe {
        SetWindowPos(
            windows::Win32::Foundation::HWND(handle.0),
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        )
    }
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
}
