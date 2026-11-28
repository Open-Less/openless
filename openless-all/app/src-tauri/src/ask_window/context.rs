//! Authorized, read-only Ask context. Foreground metadata always comes from the
//! external invocation, never from whichever OpenLess window currently has focus.

use chrono::{Local, Utc};
use openless_core::ask::{AskCancellation, AskTurnKey};
use openless_core::ask_context::{
    AppMetadata, ContextAuthorization, ContextProvider, DesktopApps, InvocationContext, TurnContext,
};
use openless_core::assistant_api::{
    AssistantContextProvider, AssistantContextQuery, AssistantContextResult,
};
use openless_core::{BackendError, BackendErrorCode};
use openless_core::browser_scene::SceneAuthorization;
use super::browser_context::BrowserReader;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[path = "context_native.rs"]
mod device;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadPermissions {
    pub browser_tab: bool,
    pub browser_selection: bool,
    pub voice_history: bool,
}

struct BrowserSource {
    reader: std::sync::Arc<dyn BrowserReader>,
    grant: SceneAuthorization,
    turn: Option<AskTurnKey>,
}

#[derive(Default)]
pub(crate) struct DesktopContext {
    state: std::sync::Arc<Mutex<ContextState>>,
    backend: Option<std::sync::Arc<openless_core::OpenLessBackend>>,
}

#[derive(Default)]
struct ContextState {
    authorization: ContextAuthorization,
    app_grant_epoch: u64,
    next_capture: u64,
    conversations: HashMap<Uuid, Option<CapturedInvocation>>,
    read_permissions: ReadPermissions,
    read_grant_epoch: u64,
    browser_sources: HashMap<Uuid, BrowserSource>,
    browser_validity: HashMap<Uuid, std::sync::Arc<dyn BrowserReader>>,
    browser_grant_epoch: u64,
    browser_turns: HashMap<Uuid, AskTurnKey>,
    manual_references: HashMap<Uuid, String>,
    manual_owners: HashSet<Uuid>,
    reference_turns: HashMap<Uuid, Option<AskTurnKey>>,
    reference_epochs: HashMap<Uuid, SceneAuthorization>,
}

pub(crate) struct CapturedInvocation {
    grant_epoch: u64,
    capture_order: u64,
    context: InvocationContext,
}

impl DesktopContext {
    pub(crate) fn has_reference_scope(&self, conversation: Uuid) -> bool {
        self.state.lock().reference_turns.contains_key(&conversation)
    }
    pub(crate) fn set_manual_reference(&self, conversation: Uuid, text: String) {
        let mut state = self.state.lock();
        state.manual_references.insert(conversation, text);
        state.manual_owners.insert(conversation);
        state.reference_turns.insert(conversation, None);
        let grant = browser_grant(&state);
        state.reference_epochs.insert(conversation, grant);
    }

    pub(crate) fn reference_submission_allowed(&self, conversation: Uuid) -> bool {
        self.state.lock().reference_turns.get(&conversation).is_none_or(Option::is_none)
    }

    pub(crate) fn reference_snapshot(&self, conversation: Uuid) -> Option<serde_json::Value> {
        let state = self.state.lock();
        if !state.conversations.contains_key(&conversation) { return None; }
        if let Some(text) = state.manual_references.get(&conversation) {
            return Some(serde_json::json!({"source":"userProvided", "text":text, "characters":text.chars().count(), "temporary":true}));
        }
        if state.manual_owners.contains(&conversation) {
            return Some(serde_json::json!({"source":"userProvided", "active":true, "temporary":true}));
        }
        if !state.read_permissions.browser_selection { return None; }
        let reader = state.browser_validity.get(&conversation).cloned();
        drop(state);
        let text = reader?.selected_text()?;
        Some(serde_json::json!({"source":"originalInputAccessibility", "text":text, "characters":text.chars().count(), "temporary":true}))
    }

    pub(crate) fn submission_guard(&self, conversation: Uuid) -> std::sync::Arc<dyn Fn() -> bool + Send + Sync> {
        let state = self.state.clone();
        let grant = {
            let state = state.lock();
            state.reference_epochs.get(&conversation).copied().unwrap_or_else(|| browser_grant(&state))
        };
        std::sync::Arc::new(move || {
            let state = state.lock();
            if !state.conversations.contains_key(&conversation) || browser_grant(&state) != grant { return false; }
            let reader = state.browser_validity.get(&conversation).cloned();
            drop(state);
            reader.is_none_or(|reader| reader.is_current())
        })
    }
    pub(crate) fn with_backend(backend: std::sync::Arc<openless_core::OpenLessBackend>) -> Self {
        Self {
            backend: Some(backend),
            ..Self::default()
        }
    }

    pub(crate) fn read_permissions(&self) -> ReadPermissions {
        self.state.lock().read_permissions
    }

    pub(crate) fn set_read_permissions(&self, mut permissions: ReadPermissions) {
        let mut state = self.state.lock();
        if !permissions.browser_tab || !state.authorization.app_metadata { permissions.browser_selection = false; }
        if state.read_permissions != permissions {
            state.read_grant_epoch += 1;
            state.browser_grant_epoch += 1;
            state.browser_sources.clear();
        }
        state.read_permissions = permissions;
        if !permissions.browser_tab {
            state.browser_sources.clear();
        }
    }

    pub(crate) fn browser_authorization(&self) -> SceneAuthorization {
        browser_grant(&self.state.lock())
    }

    pub(crate) fn browser_guard(&self, grant: SceneAuthorization) -> std::sync::Arc<dyn Fn() -> bool + Send + Sync> {
        let state = self.state.clone();
        std::sync::Arc::new(move || browser_grant(&state.lock()) == grant && grant.app_metadata && grant.browser_metadata)
    }

    /// Take once before submitting the external invocation's first Capture.
    /// The guard always compares against this exact epoch, including regrants.
    pub(crate) fn capture_browser_authorization(&self) -> Option<super::browser_capture::CaptureAuthorization> {
        let grant = self.browser_authorization();
        let authorization = super::browser_capture::CaptureAuthorization::new(grant, self.browser_guard(grant));
        authorization.is_current().then_some(authorization)
    }

    pub(crate) fn bind_browser_reference(
        &self, conversation_id: Uuid, grant: SceneAuthorization,
        reader: std::sync::Arc<dyn BrowserReader>,
    ) {
        let has_quote = grant.selected_text && grant.app_metadata && grant.browser_metadata
            && reader.selected_text().is_some();
        let mut state = self.state.lock();
        if grant.app_metadata && grant.browser_metadata && browser_grant(&state) == grant
            && state.conversations.contains_key(&conversation_id)
        {
            state.browser_validity.insert(conversation_id, reader.clone());
            if has_quote {
                state.reference_turns.insert(conversation_id, None);
                state.reference_epochs.insert(conversation_id, grant);
            }
            state.browser_sources.insert(
                conversation_id,
                BrowserSource {
                    reader, grant, turn: None,
                },
            );
        }
    }

    pub(crate) fn recent_voice(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<openless_core::ask::AskVoiceContext>, BackendError> {
        if !self.read_permissions().voice_history {
            return Ok(Vec::new());
        }
        let Some(backend) = &self.backend else {
            return Ok(Vec::new());
        };
        let rows = backend.recent_history_within_minutes(30)?;
        Ok(voice_rows(rows, query, limit))
    }

    pub(crate) fn original_app(&self, conversation_id: Uuid) -> Option<AppMetadata> {
        let state = self.state.lock();
        if !state.authorization.app_metadata {
            return None;
        }
        state
            .conversations
            .get(&conversation_id)?
            .as_ref()
            .filter(|capture| capture.grant_epoch == state.app_grant_epoch)?
            .context
            .foreground_app
            .clone()
    }

    pub(crate) fn app_metadata_allowed(&self) -> bool {
        self.app_metadata_grant().is_some()
    }

    pub(crate) fn app_metadata_grant(&self) -> Option<u64> {
        let state = self.state.lock();
        state
            .authorization
            .app_metadata
            .then_some(state.app_grant_epoch)
    }

    /// Use only Ask-specific grants. Neither OS permission nor the search
    /// switch nor history settings convey permission to share App metadata.
    pub(crate) fn set_authorization(&self, authorization: ContextAuthorization) {
        let mut state = self.state.lock();
        if state.authorization.app_metadata != authorization.app_metadata {
            state.app_grant_epoch += 1;
            state.browser_grant_epoch += 1;
            state.browser_sources.clear();
            for invocation in state.conversations.values_mut() {
                *invocation = None;
            }
        }
        state.authorization = authorization;
        if !authorization.app_metadata { state.read_permissions.browser_selection = false; }
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
        let mut state = self.state.lock();
        state.conversations.remove(&conversation_id);
        state.browser_sources.remove(&conversation_id);
        state.browser_validity.remove(&conversation_id);
        state.browser_turns.remove(&conversation_id);
        state.manual_references.remove(&conversation_id);
        state.manual_owners.remove(&conversation_id);
        state.reference_turns.remove(&conversation_id);
        state.reference_epochs.remove(&conversation_id);
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

fn browser_grant(state: &ContextState) -> SceneAuthorization {
    SceneAuthorization {
        app_metadata: state.authorization.app_metadata,
        browser_metadata: state.read_permissions.browser_tab,
        selected_text: state.read_permissions.browser_selection,
        epoch: state.browser_grant_epoch,
    }
}

impl ContextProvider for DesktopContext {
    fn begin_turn(&self, key: AskTurnKey) {
        let mut state = self.state.lock();
        // A late admitted-turn callback cannot recreate context after close.
        if !state.conversations.contains_key(&key.conversation_id) {
            return;
        }
        state.browser_turns.insert(key.conversation_id, key);
        if let Some(turn) = state.reference_turns.get_mut(&key.conversation_id) {
            if turn.is_none() { *turn = Some(key); }
        }
        if let Some(source) = state.browser_sources.get_mut(&key.conversation_id) {
            if source.turn.is_none() {
                source.turn = Some(key);
            } else if source.turn != Some(key) {
                state.browser_sources.remove(&key.conversation_id);
            }
        }
    }

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

impl AssistantContextProvider for DesktopContext {
    fn turn_reference(&self, key: AskTurnKey, cancellation: AskCancellation)
        -> futures_util::future::BoxFuture<'_, Result<Option<AssistantContextResult>, BackendError>> {
        Box::pin(async move {
            let wanted = {
                let state = self.state.lock();
                state.manual_references.contains_key(&key.conversation_id)
                    || (state.read_permissions.browser_selection && state.browser_sources.contains_key(&key.conversation_id))
            };
            if wanted { self.read(key, AssistantContextQuery::BrowserTab, cancellation).await.map(Some) }
            else { Ok(None) }
        })
    }
    fn read(
        &self,
        key: AskTurnKey,
        query: AssistantContextQuery,
        cancellation: AskCancellation,
    ) -> futures_util::future::BoxFuture<'_, Result<AssistantContextResult, BackendError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "askCancelled",
                ));
            }
            if let AssistantContextQuery::BrowserTab = query {
                {
                    let mut state = self.state.lock();
                    if state.reference_turns.get(&key.conversation_id) == Some(&Some(key))
                        && state.reference_epochs.get(&key.conversation_id) == Some(&browser_grant(&state)) {
                        if let Some(text) = state.manual_references.remove(&key.conversation_id) {
                            return Ok(AssistantContextResult::Available {
                                data: serde_json::json!({"referenceType":"untrustedUserProvidedReference", "handling":openless_core::browser_scene::BROWSER_REFERENCE_RULE, "text":text}),
                                source: "userProvided".into(), as_of: Local::now().to_rfc3339(),
                            });
                        }
                    }
                }
                let (source, grant) = {
                    let mut state = self.state.lock();
                    if !state.conversations.contains_key(&key.conversation_id) {
                        return Ok(unavailable("askConversationUnavailable"));
                    }
                    let grant = browser_grant(&state);
                    if !grant.app_metadata || !grant.browser_metadata {
                        return Ok(AssistantContextResult::Denied);
                    }
                    if state.browser_turns.get(&key.conversation_id) != Some(&key) {
                        return Ok(unavailable("browserReferenceExpired"));
                    }
                    (
                        state.browser_sources.remove(&key.conversation_id)
                            .filter(|source| source.turn == Some(key) && source.grant == grant),
                        grant,
                    )
                };
                let Some(source) = source else {
                    return Ok(unavailable("noExternalBrowser"));
                };
                let state = self.state.clone();
                let upstream = cancellation.clone();
                let reading = cancellation.clone().requiring(move || {
                    if upstream.is_cancelled() { return false; }
                    let state = state.lock();
                    browser_grant(&state) == grant
                        && state.browser_turns.get(&key.conversation_id) == Some(&key)
                        && state.conversations.contains_key(&key.conversation_id)
                });
                let result = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(BackendError::new(BackendErrorCode::Cancelled, "askCancelled")),
                    result = source.reader.read(key, grant, reading) => result,
                };
                let cancelled = cancellation.is_cancelled();
                let state = self.state.lock();
                if cancelled {
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "askCancelled",
                    ));
                }
                if !state.conversations.contains_key(&key.conversation_id) {
                    return Ok(unavailable("askConversationUnavailable"));
                }
                if browser_grant(&state) != grant {
                    return Ok(AssistantContextResult::Denied);
                }
                if state.browser_turns.get(&key.conversation_id) != Some(&key) {
                    return Ok(unavailable("browserReferenceExpired"));
                }
                return Ok(result);
            }
            if let AssistantContextQuery::VoiceHistory { ref query, limit } = query {
                let epoch = {
                    let state = self.state.lock();
                    if !state.conversations.contains_key(&key.conversation_id) {
                        return Ok(unavailable("askConversationUnavailable"));
                    }
                    if !state.read_permissions.voice_history {
                        return Ok(AssistantContextResult::Denied);
                    }
                    if self.backend.is_none() {
                        return Ok(unavailable("voiceHistoryUnavailable"));
                    }
                    state.read_grant_epoch
                };
                let rows = self.recent_voice(query, limit)?;
                let cancelled = cancellation.is_cancelled();
                let state = self.state.lock();
                if state.read_grant_epoch != epoch
                    || !state.conversations.contains_key(&key.conversation_id)
                {
                    return Ok(AssistantContextResult::Denied);
                }
                if cancelled {
                    return Err(BackendError::new(BackendErrorCode::Cancelled, "askCancelled"));
                }
                return Ok(AssistantContextResult::Available {
                    data: serde_json::json!({"entries":rows}),
                    source: "voiceTranscriptionHistory".into(),
                    as_of: Local::now().to_rfc3339(),
                });
            }
            let state = self.state.lock();
            let Some(invocation) = state.conversations.get(&key.conversation_id) else {
                return Ok(unavailable("askConversationUnavailable"));
            };
            let now = Local::now().fixed_offset().to_rfc3339();
            Ok(match query {
                AssistantContextQuery::CurrentTime => AssistantContextResult::Available {
                    data: serde_json::json!({ "iso8601": now }),
                    source: "deviceClock".into(),
                    as_of: now,
                },
                AssistantContextQuery::System => AssistantContextResult::Available {
                    data: serde_json::json!({
                        "os": std::env::consts::OS,
                        "architecture": device::architecture(),
                        "processArchitecture": std::env::consts::ARCH,
                        "version": device::system_version(),
                    }),
                    source: "hostPlatform".into(),
                    as_of: now,
                },
                AssistantContextQuery::ForegroundApp => {
                    if !state.authorization.app_metadata {
                        AssistantContextResult::Denied
                    } else if let Some(capture) = invocation
                        .as_ref()
                        .filter(|capture| capture.grant_epoch == state.app_grant_epoch)
                    {
                        match capture.context.foreground_app.as_ref() {
                            Some(app) => AssistantContextResult::Available {
                                data: serde_json::json!({ "app": app }),
                                source: "externalInvocation".into(),
                                as_of: capture.context.captured_at.to_rfc3339(),
                            },
                            None => unavailable("foregroundAppUnavailable"),
                        }
                    } else {
                        unavailable("noExternalInvocation")
                    }
                }
                AssistantContextQuery::BrowserTab | AssistantContextQuery::VoiceHistory { .. } => {
                    unreachable!()
                }
            })
        })
    }
}

fn voice_rows(
    rows: Vec<openless_core::DictationSession>,
    query: &str,
    limit: usize,
) -> Vec<openless_core::ask::AskVoiceContext> {
    let query = query.trim().to_lowercase();
    rows.into_iter()
        .filter(|row| row.source == openless_core::HistorySource::Voice && row.error_code.is_none())
        .filter_map(|row| {
            let text = if row.final_text.trim().is_empty() {
                row.raw_transcript
            } else {
                row.final_text
            };
            (!text.trim().is_empty()
                && text.chars().count() <= 4000
                && (query.is_empty()
                    || text.to_lowercase().contains(&query)
                    || query.contains(&text.to_lowercase())))
            .then_some(openless_core::ask::AskVoiceContext {
                text,
                recorded_at: row.created_at,
                // Durable history has no native control/range lease. Equal text alone is not proof.
                matches_selection: false,
            })
        })
        .take(limit.min(8))
        .collect()
}

fn unavailable(reason: &str) -> AssistantContextResult {
    AssistantContextResult::Unavailable {
        reason: reason.into(),
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
    fn has_turn_reference(&self, turn: AskTurnKey) -> bool {
        self.state.lock().reference_turns.get(&turn.conversation_id) == Some(&Some(turn))
    }
    fn begin_turn(&self, turn: AskTurnKey) {
        // StandaloneQaService owns admission and the real key. Both provider
        // contracts must reach the same grant/turn state, never a no-op shim.
        <Self as openless_core::ask_context::ContextProvider>::begin_turn(self, turn);
    }

    fn recent_voice(
        &self,
        turn: AskTurnKey,
        selection: String,
        cancel: AskCancellation,
    ) -> futures_util::future::BoxFuture<
        '_,
        Result<Vec<openless_core::ask::AskVoiceContext>, BackendError>,
    > {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "askCancelled",
                ));
            }
            let epoch = {
                let state = self.state.lock();
                if !state.conversations.contains_key(&turn.conversation_id)
                    || !state.read_permissions.voice_history
                {
                    return Ok(Vec::new());
                }
                state.read_grant_epoch
            };
            let rows = self.recent_voice(&selection, 4)?;
            let cancelled = cancel.is_cancelled();
            let state = self.state.lock();
            if cancelled {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "askCancelled",
                ));
            }
            if state.read_grant_epoch != epoch
                || !state.conversations.contains_key(&turn.conversation_id)
            {
                return Ok(Vec::new());
            }
            Ok(rows)
        })
    }

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
    fn standalone_turn_hook_registers_active_and_leaves_no_marker_after_close() {
        let provider = DesktopContext::default();
        let key = AskTurnKey { conversation_id: Uuid::new_v4(), turn_id: Uuid::new_v4() };
        provider.open_conversation(key.conversation_id);
        let standalone: &dyn openless_core::ask::ContextProvider = &provider;

        standalone.begin_turn(key);
        {
            let state = provider.state.lock();
            assert!(state.conversations.contains_key(&key.conversation_id));
            assert_eq!(state.browser_turns.get(&key.conversation_id), Some(&key));
        }
        provider.close_conversation(key.conversation_id);
        {
            let state = provider.state.lock();
            assert!(!state.conversations.contains_key(&key.conversation_id));
            assert!(!state.browser_turns.contains_key(&key.conversation_id));
        }
        // Inspect immediately after the late real trait-object callback.
        // Deleting the production close guard would reinsert this marker.
        standalone.begin_turn(key);
        let state = provider.state.lock();
        assert!(!state.browser_turns.contains_key(&key.conversation_id));
        assert!(!state.conversations.contains_key(&key.conversation_id));
        assert!(!state.browser_sources.contains_key(&key.conversation_id));
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

    #[test]
    fn voice_queries_return_only_bounded_related_text_and_never_grant_a_replacement_range() {
        let row = |id: usize, source: &str, text: &str| {
            serde_json::from_value::<openless_core::DictationSession>(serde_json::json!({
            "id":id.to_string(), "source":source, "createdAt":"2026-11-21T20:00:00+08:00",
            "rawTranscript":"raw private audio transcription", "finalText":text, "mode":"light", "insertStatus":"inserted",
            "hasAudioRecording":true,
        })).unwrap()
        };
        let mut rows = vec![
            row(0, "qa", "matching chat is not voice history"),
            row(1, "voice", "unrelated"),
        ];
        rows.extend((2..14).map(|index| row(index, "voice", "Matching dictated text")));
        let excerpts = voice_rows(rows, "matching", 100);
        assert_eq!(excerpts.len(), 8);
        assert!(excerpts
            .iter()
            .all(|entry| entry.text == "Matching dictated text" && !entry.matches_selection));
        let public = serde_json::to_value(&excerpts).unwrap().to_string();
        assert!(!public.contains("hasAudioRecording"));
        assert!(!public.contains("raw private"));
        assert!(!public.contains("\"id\""));
    }
}
