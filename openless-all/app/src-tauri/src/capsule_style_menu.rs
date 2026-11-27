//! Session-bound picker intent and keyboard ownership. No window activation or
//! synthetic input: a captured down is drained with its own physical release.
use std::sync::{Arc, OnceLock};
use parking_lot::{Mutex, RwLock};
use openless_core::{DictationPhase, DictationStateSnapshot, DictationStyleSelection, DictationStyleTarget, SessionId};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Request {
    pub session_id: SessionId,
    pub request_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Owner {
    pub session_id: SessionId,
    pub menu_id: String,
    pub window_label: String,
    pub keyboard_available: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum Key { ArrowUp, ArrowDown, Enter, Escape }

impl Key {
    fn index(self) -> usize {
        match self { Self::ArrowUp => 0, Self::ArrowDown => 1, Self::Enter => 2, Self::Escape => 3 }
    }
    pub(crate) fn from_primary(primary: &str) -> Option<Self> {
        match primary.trim().to_ascii_lowercase().as_str() {
            "arrowup" | "up" => Some(Self::ArrowUp),
            "arrowdown" | "down" => Some(Self::ArrowDown),
            "enter" | "return" => Some(Self::Enter),
            "escape" | "esc" => Some(Self::Escape),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyEvent {
    pub session_id: SessionId,
    pub menu_id: String,
    pub key: Key,
    pub repeat: bool,
    #[serde(skip)]
    pub window_label: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClosedEvent {
    pub session_id: SessionId,
    pub menu_id: String,
    #[serde(skip)]
    pub window_label: String,
}
pub(crate) enum Event { Request(Request), Key(KeyEvent), Closed(ClosedEvent), Validate(Owner) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Focus(pub u64, pub u64);

#[derive(Clone)]
pub(crate) struct Context {
    pub session_id: SessionId,
    pub style_epoch: u64,
    pub mutable: bool,
    pub skip_auto: bool,
    pub picker_enabled: bool,
    pub keyboard_available: bool,
    pub focus: Option<Focus>,
    pub blocked_keys: [bool; 4],
}

pub(crate) fn blocked_keys(target: &openless_core::HotkeyRuntimeTarget) -> [bool; 4] {
    let mut blocked = [false; 4];
    for binding in [
        Some(&target.dictation), Some(&target.translation), Some(&target.skip_auto_style),
        target.qa.as_ref(), target.quick_note.as_ref(), target.switch_style.as_ref(),
        target.open_app.as_ref(), target.selection_polish.as_ref(),
        target.coding_agent_voice.as_ref(), target.coding_agent_panel.as_ref(),
        target.coding_agent_quick.as_ref(),
    ].into_iter().flatten().chain(target.style_packs.iter().map(|item| &item.binding)) {
        if binding.modifiers.is_empty() {
            if let Some(key) = Key::from_primary(&binding.primary) { blocked[key.index()] = true; }
        }
    }
    blocked
}

struct Pending { request: Request, style_epoch: u64, focus: Option<Focus> }
struct Active { owner: Owner, request_id: Option<String>, style_epoch: u64, focus: Option<Focus> }
struct Prepared { owner: Owner, style_epoch: u64, focus: Option<Focus> }
#[derive(Default, Clone)]
struct Press { down: bool, captured: bool, menu_id: Option<String> }

#[derive(Default)]
pub(crate) struct State {
    serial: u64,
    last_hotkey_epoch: Option<u64>,
    pending: Option<Pending>,
    active: Option<Active>,
    prepared: Option<Prepared>,
    presses: [Press; 4],
    closed: Vec<ClosedEvent>,
}

impl State {
    fn revoke(&mut self) {
        if let Some(active) = self.active.take() {
            self.closed.push(ClosedEvent { session_id: active.owner.session_id,
                menu_id: active.owner.menu_id, window_label: active.owner.window_label });
        }
    }
    fn next_id(&mut self) -> String {
        self.serial = self.serial.checked_add(1).expect("style menu sequence exhausted");
        self.serial.to_string()
    }
    fn invalidate_before(&mut self, epoch: u64) {
        if self.pending.as_ref().is_some_and(|p| p.style_epoch < epoch) { self.pending = None; }
        if self.prepared.as_ref().is_some_and(|p| p.style_epoch < epoch) { self.prepared = None; }
        if self.active.as_ref().is_some_and(|a| a.style_epoch < epoch) { self.revoke(); }
        // This fence is authoritative even if the native view is unstable.
        // Keep presses, and never revoke a record from the current/newer epoch.
    }
    fn reconcile(&mut self, context: Option<&Context>) {
        if self.prepared.as_ref().is_some_and(|prepared| !context.is_some_and(|c|
            c.session_id == prepared.owner.session_id && c.mutable && c.picker_enabled
                && c.style_epoch == prepared.style_epoch && c.focus == prepared.focus)) {
            self.prepared = None;
        }
        if self.pending.as_ref().is_some_and(|pending| !context.is_some_and(|c|
            c.session_id == pending.request.session_id && c.skip_auto && c.picker_enabled
                && c.style_epoch == pending.style_epoch && c.focus == pending.focus)) {
            self.pending = None;
        }
        if self.active.as_ref().is_some_and(|active| !context.is_some_and(|c|
            c.session_id == active.owner.session_id && c.mutable && c.picker_enabled
                && c.style_epoch == active.style_epoch && c.focus == active.focus
                && (!active.owner.keyboard_available || c.keyboard_available))) {
            self.revoke();
        }
        // presses deliberately survive invalidation: previously swallowed downs
        // still own their releases, while passed downs never acquire an owner.
    }
    fn hotkey_toggled(&mut self, target: &DictationStyleTarget, style_epoch: u64,
        context: Option<&Context>) -> Option<Request> {
        // A delayed mutation receipt must never clear a newer request/owner.
        let c = context.filter(|c| c.session_id == target.session_id && c.style_epoch == style_epoch)?;
        if self.last_hotkey_epoch.is_some_and(|epoch| epoch >= style_epoch) { return None; }
        self.reconcile(context);
        self.last_hotkey_epoch = Some(style_epoch);
        if target.selection != DictationStyleSelection::SkipAuto {
            self.pending = None;
            self.prepared = None;
            self.revoke();
            return None;
        }
        if !(c.picker_enabled && c.skip_auto) { return None; }
        self.prepared = None;
        let request = Request { session_id: target.session_id, request_id: self.next_id() };
        self.pending = Some(Pending { request: request.clone(), style_epoch, focus: c.focus });
        Some(request)
    }
    fn pending(&mut self, label: &str, context: Option<&Context>) -> Option<Request> {
        self.reconcile(context);
        (label == "capsule").then(|| self.pending.as_ref().map(|p| p.request.clone())).flatten()
    }
    fn reservation(
        &mut self, label: &str, session: SessionId, request: Option<&str>, context: Option<&Context>,
    ) -> Result<Owner, String> {
        self.reconcile(context);
        if !capsule_label(label) { return Err("capsule_window_required".into()); }
        if let Some(request_id) = request {
            if let Some(active) = self.active.as_ref().filter(|active|
                active.owner.window_label == label && active.owner.session_id == session
                    && active.request_id.as_deref() == Some(request_id)) {
                return Ok(active.owner.clone());
            }
            if label != "capsule" || !self.pending.as_ref().is_some_and(|pending|
                pending.request.session_id == session && pending.request.request_id == request_id) {
                return Err("capsule_style_request_stale".into());
            }
        }
        let c = context.filter(|c| c.session_id == session && c.mutable && c.picker_enabled)
            .ok_or("recording_style_required")?;
        if c.focus.is_none() { return Err("capsule_style_menu_target_unavailable".into()); }
        let owner = Owner { session_id: session, menu_id: self.next_id(), window_label: label.into(),
            keyboard_available: c.keyboard_available && c.focus.is_some() };
        self.prepared = Some(Prepared { owner: owner.clone(), style_epoch: c.style_epoch, focus: c.focus });
        Ok(owner)
    }
    fn commit(
        &mut self, owner: Owner, request: Option<&str>, context: Option<&Context>,
    ) -> Result<(), String> {
        self.reconcile(context);
        if self.active.as_ref().is_some_and(|active| active.owner == owner
            && active.request_id.as_deref() == request) { return Ok(()); }
        // A reservation that lost its original qualification is a cancellation,
        // including when the Core session no longer supplies any Context.
        let prepared = self.prepared.as_ref().filter(|prepared| prepared.owner == owner)
            .ok_or("capsule_style_menu_reservation_stale")?;
        let c = context.filter(|c| c.session_id == owner.session_id && c.mutable
            && c.picker_enabled && c.style_epoch == prepared.style_epoch)
            .ok_or("capsule_style_menu_reservation_stale")?;
        if let Some(id) = request {
            if owner.window_label != "capsule" || !self.pending.as_ref().is_some_and(|pending|
                pending.request.session_id == owner.session_id && pending.request.request_id == id) {
                return Err("capsule_style_request_stale".into());
            }
        }
        if !self.prepared.as_ref().is_some_and(|prepared| prepared.owner == owner
            && prepared.focus == c.focus && owner.keyboard_available == c.keyboard_available) {
            return Err("capsule_style_menu_reservation_stale".into());
        }
        self.prepared = None;
        self.pending = None;
        if self.active.as_ref().is_some_and(|active| active.owner != owner) { self.revoke(); }
        self.active = Some(Active { owner, request_id: request.map(str::to_owned),
            style_epoch: c.style_epoch, focus: c.focus });
        Ok(())
    }
    fn close(&mut self, label: &str, session: SessionId, menu_id: &str) -> bool {
        if self.active.as_ref().is_some_and(|a|
            a.owner.window_label == label && a.owner.session_id == session && a.owner.menu_id == menu_id) {
            self.revoke();
            true
        } else { false }
    }
    fn is_owner(&mut self, event: &KeyEvent, context: Option<&Context>) -> bool {
        self.reconcile(context);
        self.active.as_ref().is_some_and(|a| a.owner.session_id == event.session_id
            && a.owner.menu_id == event.menu_id && a.owner.window_label == event.window_label)
    }
    fn key(
        &mut self, key: Key, down: bool, repeat: bool, modified: bool,
        context: Option<&Context>, sink: &dyn Fn(KeyEvent) -> bool,
    ) -> bool {
        self.reconcile(context);
        let index = key.index();
        let press = self.presses[index].clone();
        if !down {
            self.presses[index] = Press::default();
            return press.captured;
        }
        self.presses[index].down = true;
        if press.down || repeat {
            if press.captured && !modified && matches!(key, Key::ArrowUp | Key::ArrowDown) {
                if let Some(event) = self.key_event(key, true, context)
                    .filter(|event| Some(&event.menu_id) == press.menu_id.as_ref()) { let _ = sink(event); }
            }
            return press.captured;
        }
        if modified { return false; }
        let Some(event) = self.key_event(key, false, context) else { return false; };
        // Queue acceptance is required before claiming a fresh physical down.
        let menu_id = event.menu_id.clone();
        if !sink(event) { return false; }
        self.presses[index].captured = true;
        self.presses[index].menu_id = Some(menu_id);
        true
    }
    /// Without a stable Core view, decline fresh downs but retain the physical
    /// pair decision already made for a held key. Never invalidate a newer owner.
    fn drain_key(&mut self, key: Key, down: bool) -> bool {
        let press = &mut self.presses[key.index()];
        let captured = press.captured;
        if down { press.down = true; } else { *press = Press::default(); }
        captured
    }
    fn key_event(&self, key: Key, repeat: bool, context: Option<&Context>) -> Option<KeyEvent> {
        let c = context.filter(|c| c.keyboard_available && !c.blocked_keys[key.index()])?;
        let active = self.active.as_ref().filter(|a| a.owner.keyboard_available && a.focus.is_some()
            && c.session_id == a.owner.session_id && c.style_epoch == a.style_epoch && c.focus == a.focus)?;
        Some(KeyEvent { session_id: active.owner.session_id, menu_id: active.owner.menu_id.clone(),
            window_label: active.owner.window_label.clone(), key, repeat })
    }
}

pub(crate) fn capsule_label(label: &str) -> bool {
    label == "capsule" || label.strip_prefix("capsule-mirror-")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
}

/// A delayed Automatic frame must not hide the badge and close a newer picker.
/// Preserve existing phase/audio/job and translation presentation semantics.
pub(crate) fn project_current_style(frame: &mut DictationStateSnapshot, current: &DictationStateSnapshot) {
    if current.session_id.is_some() && frame.session_id == current.session_id
        && matches!(current.phase, DictationPhase::Starting | DictationPhase::Recording) {
        frame.skip_auto_style = current.skip_auto_style;
        frame.style_pack_id = current.style_pack_id.clone();
        frame.style_pack_name = current.style_pack_name.clone();
    }
}

type Source = Arc<dyn Fn() -> Option<Context> + Send + Sync>;
type Sink = Arc<dyn Fn(Event) -> bool + Send + Sync>;
type EpochSource = Arc<dyn Fn() -> u64 + Send + Sync>;
#[derive(Default)]
struct Runtime {
    state: Mutex<State>,
    source: RwLock<Option<Source>>,
    sink: RwLock<Option<Sink>>,
    epoch: RwLock<Option<EpochSource>>,
}
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
fn runtime() -> &'static Runtime { RUNTIME.get_or_init(Runtime::default) }
fn context() -> Option<Context> {
    let source = runtime().source.read().clone()?;
    source()
}
#[cfg(test)]
pub(crate) fn configure(source: Source, sink: Sink) {
    *runtime().source.write() = Some(source);
    *runtime().sink.write() = Some(sink);
    *runtime().epoch.write() = None;
}
pub(crate) fn configure_fenced(source: Source, sink: Sink, epoch: EpochSource) {
    *runtime().source.write() = Some(source);
    *runtime().sink.write() = Some(sink);
    *runtime().epoch.write() = Some(epoch);
}
fn flush_closed(closed: Vec<ClosedEvent>) {
    let sink = runtime().sink.read().clone();
    for event in closed {
        if let Some(sink) = &sink { let _ = sink(Event::Closed(event)); }
    }
}
fn with_state_unchecked<T>(operation: impl FnOnce(&mut State) -> T) -> T {
    let (result, closed) = {
        let mut state = runtime().state.lock();
        let result = operation(&mut state);
        (result, std::mem::take(&mut state.closed))
    };
    flush_closed(closed);
    result
}
fn with_state<T>(operation: impl FnOnce(&mut State, Option<&Context>) -> T,
    stale: impl FnOnce() -> T) -> T {
    let epoch = runtime().epoch.read().clone();
    // Native focus checks stay outside the menu lock. Before applying their
    // result, recheck the cheap Core-only fence *under* that lock. This also
    // prevents an older absent view from revoking a newer session/menu.
    let before = epoch.as_ref().map(|source| source());
    let context = context();
    let mut state = runtime().state.lock();
    let after = epoch.as_ref().map(|source| source());
    if before != after || context.as_ref().is_some_and(|c|
        after.is_some_and(|current| c.style_epoch != current)) {
        if let Some(current) = after { state.invalidate_before(current); }
        let closed = std::mem::take(&mut state.closed);
        drop(state);
        flush_closed(closed);
        // Do not repeat a native focus query on a keyboard-hook deadline.
        // Decline fresh input; only the existing physical pair may be drained.
        return stale();
    }
    let result = operation(&mut state, context.as_ref());
    let closed = std::mem::take(&mut state.closed);
    drop(state);
    flush_closed(closed);
    result
}
pub(crate) fn hotkey_toggled(target: &DictationStyleTarget, epoch: u64) -> Option<Request> {
    with_state(|state, context| state.hotkey_toggled(target, epoch, context), || None)
}
pub(crate) fn pending(label: &str) -> Option<Request> {
    with_state(|state, context| state.pending(label, context), || None)
}
pub(crate) fn reservation(label: &str, session: SessionId, request: Option<&str>) -> Result<Owner, String> {
    with_state(|state, context| state.reservation(label, session, request, context),
        || Err(if request.is_some() { "capsule_style_request_stale" } else { "capsule_style_menu_reservation_stale" }.into()))
}
pub(crate) fn commit(owner: Owner, request: Option<&str>) -> Result<(), String> {
    with_state(|state, context| state.commit(owner, request, context),
        || Err("capsule_style_menu_reservation_stale".into()))
}
pub(crate) fn close(label: &str, session: SessionId, menu_id: &str) -> bool {
    with_state_unchecked(|state| state.close(label, session, menu_id))
}
pub(crate) fn invalidate() {
    with_state_unchecked(|state| { state.pending = None; state.prepared = None; state.revoke(); });
}
pub(crate) fn window_lost(label: &str) {
    with_state_unchecked(|state| {
        if label == "capsule" { state.pending = None; }
        if state.prepared.as_ref().is_some_and(|p| p.owner.window_label == label) { state.prepared = None; }
        if state.active.as_ref().is_some_and(|a| a.owner.window_label == label) { state.revoke(); }
    });
}
pub(crate) fn owner_current(owner: &Owner) -> bool {
    with_state(|state, context| {
        state.reconcile(context);
        state.active.as_ref().is_some_and(|active| active.owner == *owner)
    }, || false)
}
pub(crate) fn refresh() {
    // Ordinary audio/state traffic must not query foreground windows when
    // there is no picker intent or owner to validate.
    {
        let state = runtime().state.lock();
        if state.pending.is_none() && state.active.is_none() && state.prepared.is_none() { return; }
    }
    with_state(|state, context| state.reconcile(context), || ());
}
pub(crate) fn is_owner(event: &KeyEvent) -> bool {
    with_state(|state, context| state.is_owner(event, context), || false)
}
pub(crate) fn key(key: Key, down: bool, repeat: bool, modified: bool) -> bool {
    {
        let mut state = runtime().state.lock();
        if state.pending.is_none() && state.active.is_none() {
            return state.drain_key(key, down);
        }
    }
    let sink = runtime().sink.read().clone();
    with_state(|state, context| state.key(key, down, repeat, modified, context, &|event|
        sink.as_ref().is_some_and(|sink| sink(Event::Key(event)))),
        || runtime().state.lock().drain_key(key, down))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn context() -> Context {
        Context { session_id: SessionId::new(), style_epoch: 1, mutable: true, skip_auto: true,
            picker_enabled: true, keyboard_available: true, focus: Some(Focus(9, 1)),
            blocked_keys: [false; 4] }
    }
    fn target(c: &Context, selection: DictationStyleSelection) -> DictationStyleTarget {
        DictationStyleTarget { session_id: c.session_id, skip_auto_style: selection != DictationStyleSelection::Automatic,
            selection, mutable: c.mutable, style_pack_id: None, style_pack_name: None,
            polish_style_pack_id: None, available_styles: vec![] }
    }
    fn open(state: &mut State, c: &Context) -> Owner {
        let owner = state.reservation("capsule", c.session_id, None, Some(c)).unwrap();
        state.commit(owner.clone(), None, Some(c)).unwrap();
        owner
    }
    fn edge(state: &mut State, c: Option<&Context>, key: Key, down: bool) -> bool {
        state.key(key, down, false, false, c, &|_| true)
    }

    #[test]
    fn starting_hotkey_intent_survives_late_mount_and_is_consumed_once_after_reservation() {
        let mut state = State::default();
        let mut c = context();
        c.mutable = false;
        // State replay / skipAuto start / programmatic selection alone cannot
        // create an intent; only the explicit native-provenance method can.
        state.reconcile(Some(&c));
        assert!(state.pending("capsule", Some(&c)).is_none());
        let request = state.hotkey_toggled(&target(&c, DictationStyleSelection::SkipAuto), c.style_epoch, Some(&c)).unwrap();
        for _ in 0..3 {
            assert_eq!(state.pending("capsule", Some(&c)), Some(request.clone()));
        }
        assert!(state.pending("capsule-mirror-0", Some(&c)).is_none());
        assert!(state.reservation("capsule", c.session_id, Some(&request.request_id), Some(&c)).is_err());
        c.mutable = true;
        let owner = state.reservation("capsule", c.session_id, Some(&request.request_id), Some(&c)).unwrap();
        // Geometry has not succeeded yet: no ownership or input consumption.
        assert!(!edge(&mut state, Some(&c), Key::Enter, true));
        assert!(!edge(&mut state, Some(&c), Key::Enter, false));
        assert_eq!(state.pending("capsule", Some(&c)), Some(request.clone()));
        state.commit(owner.clone(), Some(&request.request_id), Some(&c)).unwrap();
        assert!(state.pending("capsule", Some(&c)).is_none());
        let replay = state.reservation("capsule", c.session_id, Some(&request.request_id), Some(&c)).unwrap();
        assert_eq!(replay, owner);
        state.commit(replay, Some(&request.request_id), Some(&c)).unwrap();
        assert!(state.closed.is_empty());
        assert!(state.close("capsule", c.session_id, &owner.menu_id));
        assert!(!state.close("capsule", c.session_id, &owner.menu_id));
        assert!(state.reservation("capsule", c.session_id, Some(&request.request_id), Some(&c)).is_err());
    }

    #[test]
    fn pending_intents_drop_on_automatic_disabled_picker_focus_or_session_loss() {
        for reason in 0..5 {
            let mut state = State::default();
            let c = context();
            state.hotkey_toggled(&target(&c, DictationStyleSelection::SkipAuto), c.style_epoch, Some(&c)).unwrap();
            let mut changed = c.clone();
            match reason {
                0 => changed.skip_auto = false,
                1 => changed.picker_enabled = false,
                2 => changed.focus = Some(Focus(10, 1)),
                3 => changed.session_id = SessionId::new(),
                _ => {},
            }
            assert!(state.pending("capsule", if reason == 4 { None } else { Some(&changed) }).is_none());
        }
        let mut state = State::default();
        let c = context();
        assert!(state.hotkey_toggled(&target(&c, DictationStyleSelection::Automatic), c.style_epoch, Some(&c)).is_none());
        assert!(state.hotkey_toggled(&target(&c, DictationStyleSelection::StylePack { style_pack_id: "x".into() }), c.style_epoch, Some(&c)).is_none());
        let mut disabled = c.clone();
        disabled.picker_enabled = false;
        assert!(state.hotkey_toggled(&target(&c, DictationStyleSelection::SkipAuto), c.style_epoch, Some(&disabled)).is_none());
    }

    #[test]
    fn reservation_does_not_retarget_when_focus_or_capability_changes_during_geometry() {
        let c = context();
        for changed_focus in [false, true] {
            let mut state = State::default();
            let owner = state.reservation("capsule", c.session_id, None, Some(&c)).unwrap();
            let mut changed = c.clone();
            if changed_focus { changed.focus = Some(Focus(10, 1)); }
            else { changed.keyboard_available = false; }
            assert!(state.commit(owner, None, Some(&changed)).is_err());
            assert!(!edge(&mut state, Some(&changed), Key::Enter, true));
            assert!(!edge(&mut state, Some(&changed), Key::Enter, false));
        }
        let mut state = State::default();
        let old = state.reservation("capsule", c.session_id, None, Some(&c)).unwrap();
        let current = state.reservation("capsule", c.session_id, None, Some(&c)).unwrap();
        assert!(state.commit(old, None, Some(&c)).is_err());
        state.commit(current.clone(), None, Some(&c)).unwrap();
        assert_eq!(state.active.as_ref().unwrap().owner, current);
        let mut unknown = c;
        unknown.focus = None;
        assert!(state.reservation("capsule", unknown.session_id, None, Some(&unknown)).is_err());
    }

    #[test]
    fn captured_releases_drain_after_close_cancel_focus_loss_and_replacement() {
        for reason in 0..4 {
            let mut state = State::default();
            let c = context();
            let owner = open(&mut state, &c);
            assert!(edge(&mut state, Some(&c), Key::Enter, true));
            let mut changed = c.clone();
            match reason {
                0 => { assert!(state.close("capsule", c.session_id, &owner.menu_id)); },
                1 => state.reconcile(None),
                2 => { changed.focus = Some(Focus(10, 1)); state.reconcile(Some(&changed)); },
                _ => { open(&mut state, &c); },
            }
            assert!(state.key(Key::Enter, true, true, false, Some(&changed), &|_| panic!("repeat must not retarget")));
            assert!(edge(&mut state, Some(&changed), Key::Enter, false), "captured down must drain its up");
            assert!(!edge(&mut state, None, Key::Enter, false), "duplicate up is never swallowed");
        }
    }

    #[test]
    fn passed_or_preheld_down_never_loses_release_when_a_menu_opens() {
        let c = context();
        for repeat_before_first_observation in [false, true] {
            let mut state = State::default();
            assert!(!state.key(Key::ArrowDown, true, repeat_before_first_observation, false, Some(&c), &|_| true));
            open(&mut state, &c);
            assert!(!state.key(Key::ArrowDown, true, true, false, Some(&c), &|_| panic!("preheld key")));
            assert!(!edge(&mut state, Some(&c), Key::ArrowDown, false));
            assert!(edge(&mut state, Some(&c), Key::ArrowDown, true));
            assert!(edge(&mut state, Some(&c), Key::ArrowDown, false));
        }
    }

    #[test]
    fn only_plain_unbound_keys_with_live_successful_owner_are_claimed() {
        let c = context();
        let mut state = State::default();
        assert!(state.reservation("main", c.session_id, None, Some(&c)).is_err());
        assert!(state.reservation("capsule-mirror-not-a-window", c.session_id, None, Some(&c)).is_err());
        assert!(state.reservation("capsule", SessionId::new(), None, Some(&c)).is_err());
        open(&mut state, &c);
        for key in [Key::ArrowUp, Key::ArrowDown, Key::Enter, Key::Escape] {
            assert!(!state.key(key, true, false, true, Some(&c), &|_| panic!("modified chord")));
            assert!(!edge(&mut state, Some(&c), key, false));
            let mut blocked = c.clone();
            blocked.blocked_keys[key.index()] = true;
            assert!(!edge(&mut state, Some(&blocked), key, true));
            assert!(!edge(&mut state, Some(&blocked), key, false));
            assert!(!state.key(key, true, false, false, Some(&c), &|_| false), "queue rejection must pass");
            assert!(!edge(&mut state, Some(&c), key, false));
        }
        let mut unavailable = c.clone();
        unavailable.keyboard_available = false;
        let owner = open(&mut state, &unavailable);
        assert!(!owner.keyboard_available);
        assert!(!edge(&mut state, Some(&unavailable), Key::Enter, true));
        assert!(!edge(&mut state, Some(&unavailable), Key::Enter, false));
        assert!(state.active.is_some(), "mouse-only visual owner is supported");
    }

    #[test]
    fn old_closed_notifications_and_old_owners_cannot_close_or_receive_new_events() {
        let mut state = State::default();
        let c = context();
        let a = open(&mut state, &c);
        let b = open(&mut state, &c);
        assert_ne!(a.menu_id, b.menu_id);
        assert_eq!(state.closed.len(), 1);
        assert_eq!(state.closed[0].menu_id, a.menu_id);
        assert!(!state.close("capsule", c.session_id, &a.menu_id));
        assert!(!state.close("capsule-mirror-0", c.session_id, &b.menu_id));
        let events = RefCell::new(Vec::new());
        assert!(state.key(Key::ArrowUp, true, false, false, Some(&c), &|event| { events.borrow_mut().push(event); true }));
        assert!(state.key(Key::ArrowUp, true, true, false, Some(&c), &|event| { events.borrow_mut().push(event); true }));
        assert_eq!(events.borrow().len(), 2);
        assert!(events.borrow().iter().all(|event| event.menu_id == b.menu_id));
        let event = events.borrow()[0].clone();
        assert!(state.is_owner(&event, Some(&c)));
        let mut changed = c.clone();
        changed.focus = Some(Focus(10, 1));
        assert!(!state.is_owner(&event, Some(&changed)));
        assert_eq!(state.closed.len(), 2);
        assert_eq!(state.closed[1].menu_id, b.menu_id);
        assert!(edge(&mut state, Some(&changed), Key::ArrowUp, false));
        assert!(!edge(&mut state, Some(&changed), Key::ArrowDown, true));
        assert!(!state.close("capsule", c.session_id, &b.menu_id), "duplicate explicit/host close is inert");
    }

    #[test]
    fn all_plain_native_shortcut_owners_take_precedence_and_wire_ids_are_opaque_strings() {
        let binding = openless_core::shared_types::ShortcutBinding { primary: "Enter".into(), modifiers: vec![] };
        for field in 0..11 {
            let mut target = openless_core::HotkeyRuntimeTarget::from(&openless_core::UserPreferences::default());
            match field {
                0 => target.dictation = binding.clone(), 1 => target.translation = binding.clone(),
                2 => target.skip_auto_style = binding.clone(), 3 => target.qa = Some(binding.clone()),
                4 => target.quick_note = Some(binding.clone()), 5 => target.switch_style = Some(binding.clone()),
                6 => target.open_app = Some(binding.clone()), 7 => target.selection_polish = Some(binding.clone()),
                8 => target.coding_agent_voice = Some(binding.clone()), 9 => target.coding_agent_panel = Some(binding.clone()),
                _ => target.coding_agent_quick = Some(binding.clone()),
            }
            assert!(blocked_keys(&target)[Key::Enter.index()]);
        }
        let c = context();
        let mut state = State::default();
        let owner = open(&mut state, &c);
        let event = state.key_event(Key::Escape, false, Some(&c)).unwrap();
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["menuId"], owner.menu_id);
        assert_eq!(json["key"], "Escape");
        assert!(json.get("windowLabel").is_none());
        state.close("capsule", c.session_id, &owner.menu_id);
        let json = serde_json::to_value(&state.closed[0]).unwrap();
        assert_eq!(json["sessionId"], c.session_id.to_string());
        assert_eq!(json["menuId"], owner.menu_id);
        assert_eq!(json.as_object().unwrap().len(), 2);
    }
}
