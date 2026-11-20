//! Authorized pre-activation desktop metadata for Ask only. No document or window titles.

use chrono::{Local, Utc};
use openless_core::ask_context::{
    AppMetadata, ContextAuthorization, ContextProvider, DesktopApps, InvocationContext, TurnContext,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct DesktopContext {
    state: Mutex<ContextState>,
}

#[derive(Default)]
struct ContextState {
    authorization: ContextAuthorization,
    app_grant_epoch: u64,
    next_capture: u64,
    conversations: HashMap<Uuid, Option<CapturedInvocation>>,
}

pub(crate) struct CapturedInvocation {
    grant_epoch: u64,
    capture_order: u64,
    context: InvocationContext,
}

impl DesktopContext {
    pub(crate) fn app_metadata_allowed(&self) -> bool {
        self.state.lock().authorization.app_metadata
    }

    /// Use only Ask-specific grants. Neither OS permission nor the search
    /// switch nor history settings convey permission to share App metadata.
    pub(crate) fn set_authorization(&self, authorization: ContextAuthorization) {
        let mut state = self.state.lock();
        if state.authorization.app_metadata != authorization.app_metadata {
            state.app_grant_epoch += 1;
            for invocation in state.conversations.values_mut() {
                *invocation = None;
            }
        }
        state.authorization = authorization;
    }

    pub(crate) fn record_external_app(
        &self,
        conversation_id: Uuid,
        invocation_id: Uuid,
        captured_at: chrono::DateTime<Utc>,
        name: Option<&str>,
        app_id: Option<&str>,
    ) {
        let Some(app) = AppMetadata::new(name.unwrap_or_default(), app_id) else {
            return;
        };
        let mut state = self.state.lock();
        if !state.authorization.app_metadata || !state.conversations.contains_key(&conversation_id)
        {
            return;
        }
        if state
            .conversations
            .get(&conversation_id)
            .and_then(Option::as_ref)
            .is_some_and(|previous| previous.context.captured_at >= captured_at)
        {
            return;
        }
        state.next_capture += 1;
        let record = CapturedInvocation {
            grant_epoch: state.app_grant_epoch,
            capture_order: state.next_capture,
            context: InvocationContext {
                invocation_id,
                captured_at,
                foreground_app: Some(app),
                desktop_apps: None,
            },
        };
        state.conversations.insert(conversation_id, Some(record));
    }

    pub(crate) fn open_conversation(&self, conversation_id: Uuid) {
        self.state
            .lock()
            .conversations
            .entry(conversation_id)
            .or_default();
    }

    pub(crate) fn close_conversation(&self, conversation_id: Uuid) {
        self.state.lock().conversations.remove(&conversation_id);
    }

    /// Call only from the external native invocation path, synchronously
    /// before activation. On macOS, use the AppKit/main thread while the
    /// original focus is still current. Never call for Ask-internal turns.
    pub(crate) fn capture_before_activation(
        &self,
        invocation_id: Uuid,
    ) -> Option<CapturedInvocation> {
        self.capture(invocation_id, native::capture)
    }

    fn capture(
        &self,
        invocation_id: Uuid,
        read: impl FnOnce() -> Option<(Option<AppMetadata>, Option<DesktopApps>)>,
    ) -> Option<CapturedInvocation> {
        let (grant_epoch, capture_order) = {
            let mut state = self.state.lock();
            if !state.authorization.app_metadata {
                return None;
            }
            state.next_capture += 1;
            (state.app_grant_epoch, state.next_capture)
        };
        let captured_at = Utc::now();
        let (foreground_app, desktop_apps) = read()?;
        Some(CapturedInvocation {
            grant_epoch,
            capture_order,
            context: InvocationContext {
                invocation_id,
                captured_at,
                foreground_app,
                desktop_apps,
            },
        })
    }

    /// Bind once to the conversation selected by this exact invocation.
    /// A late completion cannot resurrect a closed conversation or old grant.
    pub(crate) fn record_invocation(
        &self,
        conversation_id: Uuid,
        capture: CapturedInvocation,
    ) -> bool {
        let mut state = self.state.lock();
        if !state.authorization.app_metadata || state.app_grant_epoch != capture.grant_epoch {
            return false;
        }
        let Some(latest) = state.conversations.get_mut(&conversation_id) else {
            return false;
        };
        if latest
            .as_ref()
            .is_some_and(|current| current.capture_order >= capture.capture_order)
        {
            return false;
        }
        *latest = Some(capture);
        true
    }
}

impl ContextProvider for DesktopContext {
    fn context_for_turn(&self, conversation_id: Uuid) -> TurnContext {
        let state = self.state.lock();
        let invocation = state
            .conversations
            .get(&conversation_id)
            .and_then(Option::as_ref)
            .map(|capture| &capture.context);
        TurnContext::build(Local::now().fixed_offset(), state.authorization, invocation)
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::{AppMetadata, DesktopApps};
    use objc2::{
        msg_send, msg_send_id,
        rc::{autoreleasepool, Retained},
        runtime::{AnyClass, AnyObject},
    };
    use openless_core::ask_context::DesktopAppScope;
    use std::ffi::CStr;

    pub(super) fn capture() -> Option<(Option<AppMetadata>, Option<DesktopApps>)> {
        autoreleasepool(|_| unsafe {
            let workspace_class = AnyClass::get("NSWorkspace")?;
            let workspace: Retained<AnyObject> = msg_send_id![workspace_class, sharedWorkspace];
            let front: Option<Retained<AnyObject>> =
                msg_send_id![&*workspace, frontmostApplication];
            let front = front?;
            let foreground_pid: i32 = msg_send![&*front, processIdentifier];
            let self_pid = std::process::id() as i32;
            if foreground_pid <= 0 || foreground_pid == self_pid {
                return None;
            }
            let foreground_app = metadata(&front);
            let running: Retained<AnyObject> = msg_send_id![&*workspace, runningApplications];
            let count: usize = msg_send![&*running, count];
            let apps = (0..count.min(128)).filter_map(|index| {
                let app: Retained<AnyObject> = msg_send_id![&*running, objectAtIndex: index];
                let pid: i32 = msg_send![&*app, processIdentifier];
                let policy: isize = msg_send![&*app, activationPolicy];
                (pid > 0 && pid != self_pid && pid != foreground_pid && policy == 0)
                    .then(|| metadata(&app))
                    .flatten()
            });
            let desktop_apps = DesktopApps::new(DesktopAppScope::OpenUserApps, apps);
            let current: Option<Retained<AnyObject>> =
                msg_send_id![&*workspace, frontmostApplication];
            let current_pid: i32 = msg_send![&*current?, processIdentifier];
            (foreground_pid == current_pid).then_some((foreground_app, Some(desktop_apps)))
        })
    }

    unsafe fn metadata(app: &AnyObject) -> Option<AppMetadata> {
        let name: Option<Retained<AnyObject>> = msg_send_id![app, localizedName];
        let app_id: Option<Retained<AnyObject>> = msg_send_id![app, bundleIdentifier];
        let name = name.as_deref().and_then(read_string);
        let app_id = app_id.as_deref().and_then(read_string);
        AppMetadata::new(name.as_deref().unwrap_or_default(), app_id.as_deref())
    }

    fn read_string(string: &AnyObject) -> Option<String> {
        let utf8: *const std::ffi::c_char = unsafe { msg_send![string, UTF8String] };
        (!utf8.is_null()).then(|| {
            unsafe { CStr::from_ptr(utf8) }
                .to_string_lossy()
                .into_owned()
        })
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::{AppMetadata, DesktopApps};
    use openless_core::ask_context::DesktopAppScope;
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{CloseHandle, BOOL, HWND, LPARAM},
            Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            UI::WindowsAndMessaging::{
                EnumWindows, GetForegroundWindow, GetWindowLongW, GetWindowThreadProcessId,
                IsIconic, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
            },
        },
    };

    pub(super) fn capture() -> Option<(Option<AppMetadata>, Option<DesktopApps>)> {
        unsafe {
            let window = GetForegroundWindow();
            let foreground_pid = window_pid(window)?;
            let self_pid = std::process::id();
            if foreground_pid == self_pid {
                return None;
            }
            let foreground_app = metadata(foreground_pid);
            let mut scan = WindowScan {
                foreground_pid,
                self_pid,
                checked: 0,
                owners: Vec::new(),
            };
            let enumeration = EnumWindows(
                Some(collect_window),
                LPARAM(&mut scan as *mut WindowScan as isize),
            );
            let desktop_apps = (enumeration.is_ok() || scan.checked >= 128).then(|| {
                DesktopApps::new(
                    DesktopAppScope::VisibleOrMinimizedWindowOwners,
                    scan.owners.into_iter().filter_map(|pid| metadata(pid)),
                )
            });
            (GetForegroundWindow() == window && window_pid(window) == Some(foreground_pid))
                .then_some((foreground_app, desktop_apps))
        }
    }

    struct WindowScan {
        foreground_pid: u32,
        self_pid: u32,
        checked: usize,
        owners: Vec<u32>,
    }

    unsafe extern "system" fn collect_window(window: HWND, state: LPARAM) -> BOOL {
        let scan = &mut *(state.0 as *mut WindowScan);
        if scan.checked >= 128 {
            return BOOL(0);
        }
        scan.checked += 1;
        if !IsWindowVisible(window).as_bool() && !IsIconic(window).as_bool() {
            return BOOL(1);
        }
        if GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return BOOL(1);
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(window, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4)
            .is_err()
            || cloaked != 0
        {
            return BOOL(1);
        }
        if let Some(pid) = window_pid(window) {
            if pid != scan.self_pid && pid != scan.foreground_pid && !scan.owners.contains(&pid) {
                scan.owners.push(pid);
            }
        }
        BOOL(1)
    }

    unsafe fn window_pid(window: HWND) -> Option<u32> {
        if window.0.is_null() {
            return None;
        }
        let mut pid = 0;
        (GetWindowThreadProcessId(window, Some(&mut pid)) != 0 && pid != 0).then_some(pid)
    }

    unsafe fn metadata(pid: u32) -> Option<AppMetadata> {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(buffer.get(..length as usize)?);
        AppMetadata::new(path.rsplit(['/', '\\']).next()?, None)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod native {
    use super::{AppMetadata, DesktopApps};

    pub(super) fn capture() -> Option<(Option<AppMetadata>, Option<DesktopApps>)> {
        Some((None, None))
    }
}

impl openless_core::ask::ContextProvider for DesktopContext {
    fn context(
        &self,
        turn: openless_core::ask::AskTurnKey,
        cancel: openless_core::ask::AskCancellation,
    ) -> futures_util::future::BoxFuture<
        '_,
        Result<openless_core::ask::AskContext, openless_core::BackendError>,
    > {
        use openless_core::{
            ask::{AskContext, AskContextFact, AskMissingFact},
            BackendError, BackendErrorCode,
        };
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Ask cancelled",
                ));
            }
            let snapshot = self.context_for_turn(turn.conversation_id);
            let facts = snapshot
                .dynamic
                .facts
                .into_iter()
                .map(|fact| {
                    let value = serde_json::to_value(&fact.value).unwrap_or_default();
                    let source = serde_json::to_value(&fact.source).unwrap_or_default();
                    AskContextFact {
                        name: value["kind"].as_str().unwrap_or("context").to_owned(),
                        value,
                        source: source.to_string(),
                        as_of: fact.as_of,
                    }
                })
                .collect();
            let missing = snapshot
                .dynamic
                .unavailable
                .into_iter()
                .map(|fact| {
                    let value = serde_json::to_value(fact).unwrap_or_default();
                    AskMissingFact {
                        name: value["kind"].as_str().unwrap_or("context").to_owned(),
                        reason: value["reason"].as_str().unwrap_or("unknown").to_owned(),
                    }
                })
                .collect();
            Ok(AskContext {
                static_version: snapshot.instructions_version.to_string(),
                static_instructions: snapshot.static_instructions.to_owned(),
                facts,
                missing,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_GRANTED: ContextAuthorization = ContextAuthorization {
        app_metadata: true,
        usage: false,
    };

    fn capture(provider: &DesktopContext, app: &str) -> CapturedInvocation {
        provider
            .capture(Uuid::new_v4(), || Some((AppMetadata::new(app, None), None)))
            .unwrap()
    }

    fn model_data(provider: &DesktopContext, conversation: Uuid) -> String {
        serde_json::to_string(&provider.context_for_turn(conversation).dynamic).unwrap()
    }

    #[test]
    fn no_collection_or_reuse_after_revocation_even_if_regranted() {
        let provider = DesktopContext::default();
        let conversation = Uuid::new_v4();
        provider.open_conversation(conversation);
        assert!(provider
            .capture(Uuid::new_v4(), || panic!("collected without consent"))
            .is_none());
        provider.set_authorization(APP_GRANTED);
        let old = capture(&provider, "Old App");
        assert!(provider.record_invocation(conversation, capture(&provider, "New App")));
        assert!(model_data(&provider, conversation).contains("New App"));
        provider.set_authorization(ContextAuthorization::default());
        provider.set_authorization(APP_GRANTED);
        assert!(!provider.record_invocation(conversation, old));
        assert!(!model_data(&provider, conversation).contains("New App"));
    }

    #[test]
    fn conversations_reject_stale_and_post_close_captures() {
        let provider = DesktopContext::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        provider.open_conversation(first);
        provider.open_conversation(second);
        provider.set_authorization(APP_GRANTED);
        let older = capture(&provider, "Earlier");
        let newer = capture(&provider, "Later");
        assert!(provider.record_invocation(first, newer));
        assert!(!provider.record_invocation(first, older));
        assert!(!model_data(&provider, second).contains("Later"));
        let late = capture(&provider, "Closing");
        provider.close_conversation(first);
        assert!(!provider.record_invocation(first, late));
        assert!(!model_data(&provider, first).contains("Closing"));
    }
}
