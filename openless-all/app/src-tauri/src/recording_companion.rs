//! A recording shortcut plus one auxiliary key. Uses the existing physical-key
//! tap, preserving left/right keys, Fn and custom multi-key recording bindings.
use crate::types::ShortcutBinding;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

type Action = Arc<dyn Fn() + Send + Sync>;
#[derive(Default)]
struct Route {
    state: ChordState,
    action: Option<Action>,
    suspended: bool,
}
#[derive(Default)]
struct ChordState {
    recording: Option<ShortcutBinding>,
    auxiliary: String,
    held: HashSet<String>,
    consumed: bool,
}
static ROUTE: OnceLock<Mutex<Route>> = OnceLock::new();
fn route() -> &'static Mutex<Route> {
    ROUTE.get_or_init(|| Mutex::new(Route::default()))
}
fn key(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "rightoption" | "rightalt" | "alt-right" => "altright".into(),
        "leftoption" | "leftalt" | "alt-left" => "altleft".into(),
        "rightcontrol" | "ctrl-right" => "ctrlright".into(),
        "leftcontrol" | "ctrl-left" => "ctrlleft".into(),
        "rightcommand" | "rightsuper" | "cmd-right" | "super-right" => "cmdright".into(),
        "leftcommand" | "leftsuper" | "cmd-left" | "super-left" => "cmdleft".into(),
        "rightshift" | "shift-right" => "shiftright".into(),
        "leftshift" | "shift-left" => "shiftleft".into(),
        "super" | "command" => "cmd".into(),
        "control" => "ctrl".into(),
        "option" => "alt".into(),
        " " => "space".into(),
        other => other.into(),
    }
}
fn matches(required: &str, physical: &str) -> bool {
    required == physical
        || matches!(required, "cmd" | "ctrl" | "alt" | "shift") && physical.starts_with(required)
}
impl ChordState {
    fn event(&mut self, physical: &str, pressed: bool) -> (bool, bool) {
        let physical = key(physical);
        let auxiliary = key(&self.auxiliary);
        let was_down = self.held.contains(&physical);
        if pressed {
            self.held.insert(physical.clone());
        } else {
            self.held.remove(&physical);
        }
        if physical == auxiliary && !pressed {
            let consume = std::mem::take(&mut self.consumed);
            return (consume, false);
        }
        if physical != auxiliary || !pressed {
            return (false, false);
        }
        if self.consumed {
            return (true, false);
        }
        if was_down || auxiliary.is_empty() {
            return (false, false);
        }
        let Some(binding) = &self.recording else {
            return (false, false);
        };
        let mut required: Vec<String> = binding.modifiers.iter().map(|value| key(value)).collect();
        if !binding.primary.eq_ignore_ascii_case("ModifierChord") {
            required.push(key(&binding.primary));
        }
        if required.is_empty()
            || required
                .iter()
                .any(|value| value.is_empty() || *value == auxiliary)
        {
            return (false, false);
        }
        let complete = required
            .iter()
            .all(|required| self.held.iter().any(|held| matches(required, held)))
            && self.held.iter().all(|held| {
                held == &auxiliary || required.iter().any(|required| matches(required, held))
            });
        if complete {
            self.consumed = true;
        }
        (complete, complete)
    }
}
pub(crate) fn configure(recording: ShortcutBinding, auxiliary: String) {
    let mut route = route().lock().unwrap();
    route.state = ChordState {
        recording: Some(recording),
        auxiliary,
        ..Default::default()
    };
}
pub(crate) fn install(action: Action) {
    route().lock().unwrap().action = Some(action);
}
pub(crate) fn suspend(suspended: bool) {
    let mut route = route().lock().unwrap();
    route.suspended = suspended;
    route.state.held.clear();
    route.state.consumed = false;
}
pub(crate) fn reset_held() {
    let mut route = route().lock().unwrap();
    route.state.held.clear();
    route.state.consumed = false;
}
pub(crate) fn claimed() -> bool {
    route().lock().unwrap().state.consumed
}
/// Returns true only for a claimed auxiliary key, including its autorepeat and release.
pub(crate) fn event(physical: &str, pressed: bool) -> bool {
    let (consume, action) = {
        let mut route = route().lock().unwrap();
        if route.suspended {
            return false;
        }
        let (consume, fire) = route.state.event(physical, pressed);
        (consume, if fire { route.action.clone() } else { None })
    };
    if let Some(action) = action {
        action();
    }
    consume
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(primary: &str, modifiers: &[&str]) -> ChordState {
        ChordState {
            recording: Some(ShortcutBinding {
                primary: primary.into(),
                modifiers: modifiers.iter().map(|v| v.to_string()).collect(),
            }),
            auxiliary: "Space".into(),
            ..Default::default()
        }
    }
    #[test]
    fn side_specific_recording_is_the_only_anchor_and_repeats_are_suppressed() {
        let mut s = state("RightAlt", &[]);
        s.event("LeftAlt", true);
        assert_eq!(s.event("Space", true), (false, false));
        s.event("Space", false);
        s.event("LeftAlt", false);
        s.event("RightAlt", true);
        assert_eq!(s.event("Space", true), (true, true));
        assert_eq!(s.event("Space", true), (true, false));
        s.event("RightAlt", false);
        assert_eq!(s.event("Space", false), (true, false));
        assert_eq!(s.event("Space", true), (false, false));
    }
    #[test]
    fn fn_and_custom_recording_chords_keep_the_entire_base() {
        let mut s = state("Fn", &[]);
        s.event("Fn", true);
        assert_eq!(s.event("Space", true), (true, true));
        let mut s = state("D", &["cmd", "shift"]);
        s.event("LeftCommand", true);
        s.event("LeftShift", true);
        assert_eq!(s.event("Space", true), (false, false));
        s.event("Space", false);
        s.event("D", true);
        assert_eq!(s.event("Space", true), (true, true));
    }
    #[test]
    fn mouse_recording_anchors_and_updated_auxiliary_keys_work() {
        let mut s = state("Mouse4", &[]);
        s.auxiliary = "Q".into();
        s.event("Mouse4", true);
        assert_eq!(s.event("Space", true), (false, false));
        s.event("Space", false);
        assert_eq!(s.event("Q", true), (true, true));
    }
    #[test]
    fn extra_modifiers_and_auxiliary_pressed_before_base_do_not_trigger() {
        let mut s = state("RightControl", &[]);
        s.event("Space", true);
        s.event("RightControl", true);
        assert_eq!(s.event("Space", true), (false, false));
        s.event("Space", false);
        s.event("LeftShift", true);
        assert_eq!(s.event("Space", true), (false, false));
    }
}
