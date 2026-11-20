use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock, Weak,
    },
    time::{Duration, Instant},
};

use global_hotkey::hotkey::Code;
use openless_core::DictationHotkeyEdge;
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::{
    shortcut_binding::{
        is_side_specific_modifier_tag, legacy_modifier_trigger, normalize_side_modifier_tag,
        parse_primary,
    },
    types::{HotkeyMode, HotkeyTrigger, ShortcutBinding},
};

use super::{capture, voice, Desktop};

const CHORD_GRACE: Duration = Duration::from_millis(170);
const FN_TAP_LIMIT: Duration = Duration::from_millis(350);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Availability {
    Available,
    Unbound,
    UnsupportedBinding,
    ConflictingSpace,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Binding {
    key: Option<u16>,
    requirements: [u16; 4],
    mode: HotkeyMode,
    fn_tap: bool,
    swallow_primary: bool,
    modifier_only: bool,
}

const CMD: u16 = 0b11;
const CTRL: u16 = 0b11 << 2;
const ALT: u16 = 0b11 << 4;
const SHIFT: u16 = 0b11 << 6;
const FN: u16 = 1 << 8;
const EITHER_SIDE: u16 = 1 << 9;
const MODIFIER_GROUPS: [u16; 4] = [CMD, CTRL, ALT, SHIFT];

fn modifiers_match(expected: [u16; 4], actual: u16) -> bool {
    actual & FN == 0
        && MODIFIER_GROUPS
            .into_iter()
            .zip(expected)
            .all(|(group, required)| {
                let down = actual & group;
                if required == EITHER_SIDE {
                    down != 0
                } else {
                    down == required
                }
            })
}

fn parse_binding(binding: &ShortcutBinding, mode: HotkeyMode) -> Result<Binding, Availability> {
    let mut physical = Binding {
        key: None,
        requirements: [0; 4],
        mode,
        fn_tap: false,
        swallow_primary: false,
        modifier_only: false,
    };
    if let Some(trigger) = legacy_modifier_trigger(binding) {
        physical.key = Some(native_trigger(trigger).ok_or(Availability::UnsupportedBinding)?);
        physical.fn_tap = cfg!(target_os = "macos")
            && trigger == HotkeyTrigger::Fn
            && matches!(mode, HotkeyMode::Auto | HotkeyMode::Toggle);
        physical.modifier_only = true;
        physical.swallow_primary = cfg!(target_os = "windows");
        return Ok(physical);
    }
    if cfg!(target_os = "macos")
        && binding.primary == "MacDictationKey"
        && binding.modifiers.is_empty()
    {
        physical.key = Some(176);
        physical.swallow_primary = true;
        return Ok(physical);
    }
    let modifier_chord = openless_core::is_modifier_chord_binding(binding);
    if !modifier_chord && binding.primary.trim().is_empty() && binding.primary != " " {
        return Err(Availability::Unbound);
    }
    for raw in &binding.modifiers {
        let tag = normalize_side_modifier_tag(raw);
        let (index, bits) = match tag.as_str() {
            "cmd-left" => (0, 1),
            "cmd-right" => (0, 2),
            "ctrl-left" => (1, 1),
            "ctrl-right" => (1, 2),
            "alt-left" => (2, 1),
            "alt-right" => (2, 2),
            "shift-left" => (3, 1),
            "shift-right" => (3, 2),
            "cmd" | "command" if cfg!(target_os = "windows") => (1, 3),
            "cmd" | "command" | "meta" | "win" | "super" => (0, 3),
            "ctrl" | "control" => (1, 3),
            "alt" | "opt" | "option" => (2, 3),
            "shift" => (3, 3),
            _ => return Err(Availability::UnsupportedBinding),
        };
        if bits == 3 {
            if physical.requirements[index] != 0 {
                return Err(Availability::UnsupportedBinding);
            }
            physical.requirements[index] = EITHER_SIDE;
        } else {
            if physical.requirements[index] == EITHER_SIDE {
                return Err(Availability::UnsupportedBinding);
            }
            physical.requirements[index] |= bits << (index * 2);
        }
    }
    if modifier_chord {
        physical.modifier_only = true;
    } else {
        let code = parse_primary(&binding.primary).map_err(|_| Availability::UnsupportedBinding)?;
        if code == Code::Space {
            return Err(Availability::ConflictingSpace);
        }
        physical.key = native_code(code);
        if physical.key.is_none() {
            return Err(Availability::UnsupportedBinding);
        }
        physical.swallow_primary = !binding
            .modifiers
            .iter()
            .any(|tag| is_side_specific_modifier_tag(tag));
    }
    Ok(physical)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disposition {
    Stock,
    Pass,
    Consume,
}

struct Runtime {
    desktop: Weak<Desktop>,
    tx: mpsc::UnboundedSender<Message>,
    native: Mutex<Native>,
    suspended: AtomicBool,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub(super) fn install(desktop: Weak<Desktop>) {
    let (tx, rx) = mpsc::unbounded_channel();
    if RUNTIME
        .set(Runtime {
            desktop: desktop.clone(),
            tx,
            native: Mutex::new(Native::default()),
            suspended: AtomicBool::new(false),
        })
        .is_ok()
    {
        tauri::async_runtime::spawn(dispatch(desktop, rx));
    }
}

pub(crate) fn set_binding(binding: &ShortcutBinding, mode: HotkeyMode) -> Availability {
    let parsed = parse_binding(binding, mode);
    if let Some(runtime) = RUNTIME.get() {
        let mut native = runtime.native.lock();
        if native.binding != parsed.ok() {
            native.invalidate(&runtime.tx);
            native.binding = parsed.ok();
        }
    }
    parsed.map_or_else(|availability| availability, |_| Availability::Available)
}

pub(crate) fn reset_physical_state() {
    if let Some(runtime) = RUNTIME.get() {
        let mut native = runtime.native.lock();
        native.invalidate(&runtime.tx);
        native.modifiers = 0;
    }
}

pub(crate) fn set_suspended(suspended: bool) {
    if let Some(runtime) = RUNTIME.get() {
        runtime.suspended.store(suspended, Ordering::Release);
        if suspended {
            runtime.native.lock().invalidate(&runtime.tx);
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn fn_is_held() -> bool {
    RUNTIME
        .get()
        .is_some_and(|runtime| runtime.native.lock().modifiers & FN != 0)
}

#[cfg(target_os = "macos")]
pub(crate) fn abort_fn_companion() {
    if let Some(runtime) = RUNTIME.get() {
        let mut native = runtime.native.lock();
        if native.modifiers & FN != 0 {
            native.abort_for_companion(&runtime.tx);
        }
    }
}

pub(crate) fn keyboard_edge(key: u16, down: bool, repeat: bool, disabled: bool) -> Disposition {
    let Some(runtime) = RUNTIME.get() else {
        return Disposition::Stock;
    };
    let Some(desktop) = runtime.desktop.upgrade() else {
        return Disposition::Stock;
    };
    runtime.native.lock().key_edge(
        desktop.as_ref(),
        &runtime.tx,
        key,
        down,
        repeat,
        disabled || runtime.suspended.load(Ordering::Acquire),
    )
}

#[derive(Default)]
struct SpaceLatch {
    down: bool,
    captured: bool,
}

impl SpaceLatch {
    fn edge(&mut self, down: bool, repeat: bool, claim: impl FnOnce() -> bool) -> bool {
        if !down {
            self.down = false;
            return std::mem::take(&mut self.captured);
        }
        if !self.down && !repeat {
            self.down = true;
            self.captured = claim();
        }
        self.captured
    }
}

#[derive(Default)]
struct Native {
    binding: Option<Binding>,
    modifiers: u16,
    space: SpaceLatch,
    held: Option<Held>,
    paired_primary: Option<(u16, Disposition)>,
    escape_captured: bool,
}

struct Held {
    stroke: Arc<Stroke>,
    chord_taken: bool,
    companion: bool,
}

struct Stroke {
    id: u64,
    at: Instant,
    binding: Binding,
    origin: Origin,
}

enum Origin {
    Ordinary,
    Ask(Option<voice::Target>),
}

struct Chord {
    trigger: Option<capture::Trigger>,
    autostart: bool,
}

enum Message {
    Press(Arc<Stroke>),
    Release(u64, Instant),
    Grace(u64),
    Chord(u64, Chord),
    Abort(u64),
    Escape,
}

impl Native {
    fn invalidate(&mut self, tx: &mpsc::UnboundedSender<Message>) {
        if let Some(held) = self.held.take() {
            let _ = tx.send(Message::Abort(held.stroke.id));
        }
    }

    fn key_edge(
        &mut self,
        desktop: &Desktop,
        tx: &mpsc::UnboundedSender<Message>,
        key: u16,
        down: bool,
        repeat: bool,
        disabled: bool,
    ) -> Disposition {
        let modifier = native_modifier(key);
        if down {
            self.modifiers |= modifier;
        } else {
            self.modifiers &= !modifier;
        }
        if disabled {
            self.invalidate(tx);
        }
        if key == SPACE {
            let held = &mut self.held;
            let modifiers = self.modifiers;
            let handled = self.space.edge(down, repeat, || {
                if disabled {
                    return false;
                }
                #[cfg(target_os = "macos")]
                if crate::unicode_keystroke::is_secure_input_enabled() {
                    return false;
                }
                let Some(held) = held.as_mut().filter(|held| {
                    !held.chord_taken
                        && !held.companion
                        && still_matches(held.stroke.binding, modifiers)
                }) else {
                    return false;
                };
                let foreground = capture::foreground();
                let trigger = match &held.stroke.origin {
                    Origin::Ask(_) => {
                        if !desktop.is_native_target(foreground.as_ref()) {
                            return false;
                        }
                        None
                    }
                    Origin::Ordinary => {
                        if desktop.is_native_target(foreground.as_ref()) {
                            return false;
                        }
                        Some(capture::Trigger::at_key_event(foreground))
                    }
                };
                let chord = Chord {
                    trigger,
                    autostart: !desktop.voice_is_pending(),
                };
                held.chord_taken = tx.send(Message::Chord(held.stroke.id, chord)).is_ok();
                held.chord_taken
            });
            if down && !handled {
                self.abort_for_companion(tx);
            }
            return if handled {
                Disposition::Consume
            } else {
                Disposition::Stock
            };
        }
        if key == ESCAPE {
            if !down && std::mem::take(&mut self.escape_captured) {
                return Disposition::Consume;
            }
            if down
                && (self.escape_captured
                    || (!disabled && desktop.voice_is_pending() && desktop.is_focused()))
            {
                if !self.escape_captured {
                    let _ = tx.send(Message::Escape);
                }
                self.escape_captured = true;
                return Disposition::Consume;
            }
        }
        if let Some((primary, disposition)) = self.paired_primary {
            if primary == key {
                if !down {
                    self.paired_primary = None;
                    if let Some(held) = self.held.take() {
                        let _ = tx.send(Message::Release(held.stroke.id, Instant::now()));
                    }
                }
                return disposition;
            }
        }
        let Some(binding) = self.binding.filter(|_| !disabled) else {
            return Disposition::Stock;
        };
        if binding.key.is_none()
            && modifier != 0
            && self.held.is_some()
            && !still_matches(binding, self.modifiers)
        {
            if let Some(held) = self.held.take() {
                let _ = tx.send(Message::Release(held.stroke.id, Instant::now()));
            }
            return Disposition::Pass;
        }
        if repeat || !down {
            return Disposition::Stock;
        }
        let matches_binding = if let Some(primary) = binding.key {
            primary == key && (binding.modifier_only || still_matches(binding, self.modifiers))
        } else {
            modifier != 0 && still_matches(binding, self.modifiers)
        };
        if !matches_binding {
            if modifier == 0 {
                self.abort_for_companion(tx);
            }
            return Disposition::Stock;
        }
        if self.held.is_some() {
            return Disposition::Pass;
        }
        let foreground = capture::foreground();
        let origin = if desktop.is_native_target(foreground.as_ref()) {
            Origin::Ask(desktop.target_at_press())
        } else {
            Origin::Ordinary
        };
        let stroke = Arc::new(Stroke {
            id: crate::hotkey::next_press_id(),
            at: Instant::now(),
            binding,
            origin,
        });
        if tx.send(Message::Press(stroke.clone())).is_err() {
            return Disposition::Stock;
        }
        let disposition = if binding.swallow_primary {
            Disposition::Consume
        } else {
            Disposition::Pass
        };
        if binding.key.is_some() {
            self.paired_primary = Some((key, disposition));
        }
        self.held = Some(Held {
            stroke,
            chord_taken: false,
            companion: false,
        });
        disposition
    }

    fn abort_for_companion(&mut self, tx: &mpsc::UnboundedSender<Message>) {
        if let Some(held) = self
            .held
            .as_mut()
            .filter(|held| !held.chord_taken && !held.companion)
        {
            held.companion = true;
            if held.stroke.binding.modifier_only {
                let _ = tx.send(Message::Abort(held.stroke.id));
            }
        }
    }
}

fn still_matches(binding: Binding, physical_modifiers: u16) -> bool {
    let primary = binding.key.map(native_modifier).unwrap_or(0);
    modifiers_match(binding.requirements, physical_modifiers & !primary)
}

struct Dispatch {
    stroke: Arc<Stroke>,
    started: bool,
    stop_only: bool,
    completed: bool,
}

async fn dispatch(desktop: Weak<Desktop>, mut receiver: mpsc::UnboundedReceiver<Message>) {
    let (edges, mut ordinary) = mpsc::unbounded_channel();
    let ordinary_desktop = desktop.clone();
    tauri::async_runtime::spawn(async move {
        while let Some((edge, mode)) = ordinary.recv().await {
            let Some(desktop) = ordinary_desktop.upgrade() else {
                break;
            };
            let delivery = desktop.ports.ordinary_dictation_edge(edge);
            if mode == HotkeyMode::Hold {
                tauri::async_runtime::spawn(delivery);
            } else {
                delivery.await;
            }
        }
    });
    let mut current: Option<Dispatch> = None;
    while let Some(message) = receiver.recv().await {
        let Some(desktop) = desktop.upgrade() else {
            break;
        };
        match message {
            Message::Press(stroke) => {
                let active = desktop.ports.ordinary_dictation_is_active();
                let preexisting_grace = matches!(stroke.origin, Origin::Ordinary)
                    && stroke.binding.modifier_only
                    && !stroke.binding.fn_tap
                    && !active;
                if !preexisting_grace
                    && !stroke.binding.fn_tap
                    && matches!(stroke.binding.mode, HotkeyMode::Hold | HotkeyMode::Auto)
                    && !active
                    && (matches!(stroke.origin, Origin::Ordinary) || !desktop.voice_is_pending())
                {
                    if let Some(runtime) = RUNTIME.get() {
                        let tx = runtime.tx.clone();
                        let id = stroke.id;
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(CHORD_GRACE).await;
                            let _ = tx.send(Message::Grace(id));
                        });
                    }
                }
                let mut next = Dispatch {
                    stroke,
                    started: false,
                    stop_only: active,
                    completed: false,
                };
                if preexisting_grace {
                    next.start(&desktop, &edges, false);
                }
                current = Some(next);
            }
            Message::Grace(id) => {
                if let Some(state) = current
                    .as_mut()
                    .filter(|state| state.stroke.id == id && !state.completed)
                {
                    if !matches!(state.stroke.origin, Origin::Ordinary)
                        || !desktop.ports.ordinary_dictation_is_active()
                    {
                        state.start(&desktop, &edges, false);
                    }
                }
            }
            Message::Release(id, at) => {
                let Some(mut state) = current.take() else {
                    continue;
                };
                if state.stroke.id != id {
                    current = Some(state);
                    continue;
                }
                if state.completed
                    || (state.stroke.binding.fn_tap
                        && at.saturating_duration_since(state.stroke.at) >= FN_TAP_LIMIT)
                {
                    continue;
                }
                state.start(&desktop, &edges, true);
                state.release(&desktop, &edges, at);
            }
            Message::Chord(id, chord) => {
                let Some(state) = current
                    .as_mut()
                    .filter(|state| state.stroke.id == id && !state.completed)
                else {
                    continue;
                };
                let cancel = chord.trigger.is_some() && state.started && !state.stop_only;
                let autostart = chord.autostart && !desktop.voice_is_pending();
                let desktop = desktop.clone();
                tauri::async_runtime::spawn(async move {
                    if cancel {
                        desktop
                            .ports
                            .ordinary_dictation_edge(DictationHotkeyEdge::Combined {
                                press_id: id,
                                at: Instant::now(),
                            })
                            .await;
                    }
                    if let Err(error) = desktop.open_with(chord.trigger, Some(autostart)).await {
                        log::warn!("[ask-window] could not open: {error}");
                    }
                });
                state.completed = true;
            }
            Message::Abort(id) => {
                if let Some(state) = current
                    .as_mut()
                    .filter(|state| state.stroke.id == id && !state.completed)
                {
                    state.abort(&desktop);
                    state.completed = true;
                }
            }
            Message::Escape => {
                desktop.dismiss_composer_from_escape();
            }
        }
    }
}

type EdgeSender = mpsc::UnboundedSender<(DictationHotkeyEdge, HotkeyMode)>;

impl Dispatch {
    fn start(&mut self, desktop: &Desktop, ordinary: &EdgeSender, toggle: bool) {
        if self.started {
            return;
        }
        self.started = true;
        let mode = self.stroke.binding.mode;
        match &self.stroke.origin {
            Origin::Ordinary => self.emit(
                ordinary,
                DictationHotkeyEdge::Pressed {
                    press_id: self.stroke.id,
                    at: self.stroke.at,
                },
            ),
            Origin::Ask(target) => desktop.voice.manual(
                target.clone(),
                self.stroke.id,
                toggle && mode != HotkeyMode::Hold,
            ),
        }
    }

    fn emit(&self, ordinary: &EdgeSender, edge: DictationHotkeyEdge) {
        let _ = ordinary.send((edge, self.stroke.binding.mode));
    }

    fn release(&self, desktop: &Desktop, ordinary: &EdgeSender, at: Instant) {
        match self.stroke.origin {
            Origin::Ordinary => self.emit(
                ordinary,
                DictationHotkeyEdge::Released {
                    press_id: self.stroke.id,
                    at,
                },
            ),
            Origin::Ask(_) => desktop.voice.release(
                self.stroke.id,
                at.saturating_duration_since(self.stroke.at),
                self.stroke.binding.mode == HotkeyMode::Hold,
                self.stroke.binding.mode == HotkeyMode::Auto,
            ),
        }
    }

    fn abort(&self, desktop: &Desktop) {
        if !self.started || self.stop_only {
            return;
        }
        match self.stroke.origin {
            Origin::Ordinary => {
                tauri::async_runtime::spawn(desktop.ports.ordinary_dictation_edge(
                    DictationHotkeyEdge::Combined {
                        press_id: self.stroke.id,
                        at: Instant::now(),
                    },
                ));
            }
            Origin::Ask(_) => desktop.voice.cancel_press(self.stroke.id),
        }
    }
}

#[cfg(target_os = "macos")]
const SPACE: u16 = 49;
#[cfg(target_os = "macos")]
const ESCAPE: u16 = 53;
#[cfg(target_os = "windows")]
const SPACE: u16 = 0x20;
#[cfg(target_os = "windows")]
const ESCAPE: u16 = 0x1B;

#[cfg(target_os = "macos")]
fn native_modifier(key: u16) -> u16 {
    match key {
        55 => 1,
        54 => 2,
        59 => 4,
        62 => 8,
        58 => 16,
        61 => 32,
        56 => 64,
        60 => 128,
        63 => FN,
        _ => 0,
    }
}

#[cfg(target_os = "windows")]
fn native_modifier(key: u16) -> u16 {
    match key {
        0x5B => 1,
        0x5C => 2,
        0xA2 => 4,
        0xA3 => 8,
        0xA4 => 16,
        0xA5 => 32,
        0xA0 => 64,
        0xA1 => 128,
        _ => 0,
    }
}

#[cfg(target_os = "macos")]
fn native_trigger(trigger: HotkeyTrigger) -> Option<u16> {
    Some(match trigger {
        HotkeyTrigger::LeftCommand => 55,
        HotkeyTrigger::RightCommand => 54,
        HotkeyTrigger::LeftControl => 59,
        HotkeyTrigger::RightControl => 62,
        HotkeyTrigger::LeftOption => 58,
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => 61,
        HotkeyTrigger::LeftShift => 56,
        HotkeyTrigger::RightShift => 60,
        HotkeyTrigger::Fn => 63,
        HotkeyTrigger::MediaPlayPause | HotkeyTrigger::Custom => return None,
    })
}

#[cfg(target_os = "windows")]
fn native_trigger(trigger: HotkeyTrigger) -> Option<u16> {
    Some(match trigger {
        HotkeyTrigger::LeftCommand => 0x5B,
        HotkeyTrigger::RightCommand => 0x5C,
        HotkeyTrigger::LeftControl => 0xA2,
        HotkeyTrigger::RightControl | HotkeyTrigger::Fn => 0xA3,
        HotkeyTrigger::LeftOption => 0xA4,
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => 0xA5,
        HotkeyTrigger::LeftShift => 0xA0,
        HotkeyTrigger::RightShift => 0xA1,
        HotkeyTrigger::MediaPlayPause => 0xB3,
        HotkeyTrigger::Custom => return None,
    })
}

#[cfg(target_os = "macos")]
fn native_code(code: Code) -> Option<u16> {
    match code {
        Code::Home => Some(115),
        Code::End => Some(119),
        Code::PageUp => Some(116),
        Code::PageDown => Some(121),
        _ => (0..=127).find(|key| {
            crate::side_aware_combo::macos_keycode_to_primary(i64::from(*key))
                .and_then(|primary| parse_primary(primary).ok())
                == Some(code)
        }),
    }
}

#[cfg(target_os = "windows")]
fn native_code(code: Code) -> Option<u16> {
    (0..=255).find(|key| {
        crate::side_aware_combo::platform::primary_from_vk(u32::from(*key))
            .and_then(|primary| parse_primary(&primary).ok())
            == Some(code)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_space_balances_repeats_and_release_after_binding_changes() {
        let mut space = SpaceLatch::default();
        assert!(space.edge(true, false, || true));
        assert!(space.edge(true, true, || unreachable!()));
        assert!(space.edge(false, false, || unreachable!()));
        assert!(!space.edge(true, false, || false));
        assert!(!space.edge(true, true, || unreachable!()));
        assert!(!space.edge(false, false, || unreachable!()));
    }

    #[test]
    fn space_held_before_the_binding_never_becomes_a_chord() {
        let mut space = SpaceLatch::default();
        assert!(!space.edge(true, false, || false));
        assert!(!space.edge(true, false, || unreachable!()));
        assert!(!space.edge(false, false, || unreachable!()));
        assert!(space.edge(true, false, || true));
    }

    #[test]
    fn only_the_configured_modifier_side_or_group_matches() {
        let left_ctrl = [0, 4, 0, 0];
        assert!(modifiers_match(left_ctrl, 4));
        assert!(!modifiers_match(left_ctrl, 8));
        assert!(!modifiers_match(left_ctrl, 4 | SHIFT));
        assert!(modifiers_match([0, EITHER_SIDE, 0, 0], 8));
        assert!(!modifiers_match([0, CTRL, 0, 0], 8));
        assert!(modifiers_match([0, CTRL, 0, 0], CTRL));
    }

    #[test]
    fn replacing_the_active_binding_does_not_assign_a_second_space_primary() {
        let mut shortcut = ShortcutBinding {
            primary: "F8".into(),
            modifiers: vec!["ctrl-right".into()],
        };
        let first = parse_binding(&shortcut, HotkeyMode::Hold).unwrap();
        assert!(still_matches(first, 8));
        assert!(!still_matches(first, 4));
        shortcut.modifiers = vec!["ctrl-left".into()];
        let second = parse_binding(&shortcut, HotkeyMode::Hold).unwrap();
        assert!(still_matches(second, 4));
        assert!(!still_matches(second, 8));
        shortcut.primary = "Space".into();
        assert!(matches!(
            parse_binding(&shortcut, HotkeyMode::Hold),
            Err(Availability::ConflictingSpace)
        ));
    }
}
