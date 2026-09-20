//! Overlay placement for the popup processes: bottom-centre, never take focus.
//!
//! The recording capsule is a pure overlay: it shows the recording state and
//! offers cancel / confirm, but it must **never** take the keyboard — the user
//! is dictating into *another* window, and stealing focus there would send the
//! insert to the wrong place. Tauri gets exactly this guarantee on macOS from
//! `NSWindow::orderFrontRegardless` ("visible but not the key window", see
//! the Tauri host's `show_qa_window`). Wayland's xdg-shell offers
//! neither an absolute position nor a focus opt-out, so the capsule runs under
//! XWayland, where:
//!
//! * `WM_HINTS.input = False` makes the window manager never assign focus
//!   (pointer clicks still reach the pill's ✕ / ✓ buttons),
//! * `_NET_WM_WINDOW_TYPE_UTILITY` keeps KWin from applying its OSD-style
//!   placement to the pill (the notification look is asked for explicitly with
//!   `_NET_WM_STATE_ABOVE` + `SKIP_TASKBAR` instead), and
//! * an explicit `ConfigureWindow` places it at the bottom centre of the work
//!   area, with the ICCCM `USPosition` hint so the manager keeps those
//!   coordinates, mirroring the Tauri host's
//!   `position_capsule_bottom_center_with_style`.
//!
//! The focus is only taken back when `_NET_ACTIVE_WINDOW` really is the pill:
//! re-reading it after the move keeps the overlay from yanking the keyboard
//! away from whatever window the user moved on to.
//!
//! The maths lives in [`OverlayEnvironment`] so it is unit-testable, and every
//! X11 mutation goes through the [`OverlayX11`] trait so the request sequence
//! can be asserted without an X server.

/// Capsule window size (the 176×42 pill plus room for the translate badge).
pub const CAPSULE_WINDOW_SIZE: (u32, u32) = (200, 100);
/// Gap between the capsule pill and the bottom of the work area — Tauri's
/// `EDGE_GAP` for the classic / siri capsule styles.
pub const CAPSULE_BOTTOM_GAP: i32 = 12;
/// Selection-ask panel size (the chat panel) — Tauri `qa` window is 420×540.
pub const QA_WINDOW_SIZE: (u32, u32) = (420, 540);
/// Less Computer panel size — Tauri `less-computer` is 420×540, the same
/// footprint as the selection-ask panel.
pub const LESS_COMPUTER_WINDOW_SIZE: (u32, u32) = (420, 540);
/// Polish-preview panel size — Tauri `selection-polish-preview` is 640×440.
/// Smallest the user may resize the polish preview to — Tauri `minWidth/minHeight`.

/// Window size for one popup kind, in X11 pixels.
pub fn popup_size(kind: crate::popup::PopupKind) -> (u32, u32) {
    use crate::popup::PopupKind;
    match kind {
        PopupKind::Capsule => CAPSULE_WINDOW_SIZE,
        PopupKind::Qa => QA_WINDOW_SIZE,
        PopupKind::LessComputer => LESS_COMPUTER_WINDOW_SIZE,
    }
}

/// Where one popup is placed inside the work area.
///
/// The capsule hugs the bottom edge, centred: it is the transient overlay the
/// eyes track while dictating, and Tauri's `position_capsule_bottom_center_*`
/// puts it there too. The selection-ask panel and the polish preview are
/// **centred** instead of stacked above the pill — Tauri shows both as centred
/// cards, and stacking would overlap: a 520px panel sitting 50px above the
/// work-area bottom runs into the 100px capsule strip that starts 112px above
/// it.
pub fn popup_position(
    environment: &OverlayEnvironment,
    kind: crate::popup::PopupKind,
) -> Option<(i32, i32)> {
    use crate::popup::PopupKind;
    match kind {
        PopupKind::Capsule => environment.position_for(CAPSULE_WINDOW_SIZE, CAPSULE_BOTTOM_GAP),
        PopupKind::Qa => environment.centred_for(QA_WINDOW_SIZE),
        PopupKind::LessComputer => environment.centred_for(LESS_COMPUTER_WINDOW_SIZE),
    }
}

/// A rectangle in root-window coordinates (pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct X11Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl X11Rect {
    pub fn contains(self, point: (i32, i32)) -> bool {
        point.0 >= self.x
            && point.0 < self.x + self.width as i32
            && point.1 >= self.y
            && point.1 < self.y + self.height as i32
    }
}

/// Tauri `bottom_center_position`: centre horizontally, sit `bottom_gap` above
/// the bottom edge, then pull the window back inside `area` if it overflows.
pub fn bottom_center(area: X11Rect, window: (u32, u32), bottom_gap: i32) -> (i32, i32) {
    let x = area.x + (area.width.saturating_sub(window.0) / 2) as i32;
    let y = area.y + (area.height as i32 - bottom_gap - window.1 as i32).max(0);
    clamp_to_area(x, y, window, area)
}

/// Tauri `clamp_to_monitor`: keep the whole window inside `area`, tolerating an
/// area that is smaller than the window.
pub fn clamp_to_area(x: i32, y: i32, window: (u32, u32), area: X11Rect) -> (i32, i32) {
    let max_x = (area.x + area.width as i32 - window.0 as i32).max(area.x);
    let max_y = (area.y + area.height as i32 - window.1 as i32).max(area.y);
    (x.clamp(area.x, max_x), y.clamp(area.y, max_y))
}

/// Centre the window in `area` (both axes), then pull it back inside.
pub fn centered(area: X11Rect, window: (u32, u32)) -> (i32, i32) {
    let x = area.x + (area.width.saturating_sub(window.0) / 2) as i32;
    let y = area.y + (area.height.saturating_sub(window.1) / 2) as i32;
    clamp_to_area(x, y, window, area)
}

/// Search the monitor list for the one holding `point` (Tauri follows the
/// pointer on macOS and the foreground window on Windows; on X11 the pointer is
/// what we can read before we map our own window).
pub fn monitor_containing(monitors: &[X11Rect], point: Option<(i32, i32)>) -> Option<X11Rect> {
    let point = point?;
    monitors
        .iter()
        .copied()
        .find(|monitor| monitor.contains(point))
}

/// Everything read from X11 *before* the popup maps its own window: the work
/// area (taskbar excluded), the monitor list, the pointer position and whoever
/// held the focus at the time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverlayEnvironment {
    pub work_area: Option<X11Rect>,
    pub monitors: Vec<X11Rect>,
    pub cursor: Option<(i32, i32)>,
    /// `_NET_ACTIVE_WINDOW` before the popup appeared, used to put the focus
    /// back if the compositor handed it to us anyway.
    pub active_window: Option<u32>,
}

impl OverlayEnvironment {
    /// The rectangle to centre in: the work area of the monitor under the
    /// pointer, else that monitor, else the first monitor.
    pub fn placement_area(&self) -> Option<X11Rect> {
        let monitor = monitor_containing(&self.monitors, self.cursor)
            .or_else(|| self.work_area)
            .or_else(|| self.monitors.first().copied())?;
        // `_NET_WORKAREA` is a single rectangle for the whole virtual desktop;
        // intersect it with the chosen monitor so the pill lands on the screen
        // the user is looking at, still above the taskbar.
        match self.work_area {
            Some(work) => Some(intersect(work, monitor).unwrap_or(monitor)),
            None => Some(monitor),
        }
    }

    pub fn position_for(&self, window: (u32, u32), bottom_gap: i32) -> Option<(i32, i32)> {
        self.placement_area()
            .map(|area| bottom_center(area, window, bottom_gap))
    }

    /// Centre `window` in the same area `position_for` uses.
    pub fn centred_for(&self, window: (u32, u32)) -> Option<(i32, i32)> {
        self.placement_area().map(|area| centered(area, window))
    }
}

/// Intersection of two rectangles, `None` when they do not overlap.
pub fn intersect(a: X11Rect, b: X11Rect) -> Option<X11Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width as i32).min(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).min(b.y + b.height as i32);
    if right <= left || bottom <= top {
        return None;
    }
    Some(X11Rect {
        x: left,
        y: top,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

/// How the overlay window was identified in the X11 tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMatch {
    /// `_NET_WM_PID` matched our own pid — the reliable path.
    Pid,
    /// Fallback: a window publishing no `_NET_WM_PID` whose `WM_CLASS`
    /// mentions OpenLess.
    Class,
    /// Fallback: same, matched on `_NET_WM_NAME` / `WM_NAME`.
    Name,
}

impl WindowMatch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pid => "pid",
            Self::Class => "wm_class",
            Self::Name => "wm_name",
        }
    }
}

/// One window found in the X11 tree, with the properties the selector needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCandidate {
    pub window: u32,
    pub pid: Option<u32>,
    pub wm_class: Option<String>,
    pub name: Option<String>,
}

/// Pick our own popup window out of the tree.
///
/// `_NET_WM_PID` is authoritative, but the window manager is free to keep it
/// off the client window (or the client may not have published it yet when the
/// pre-map pass runs), so `WM_CLASS` / `_NET_WM_NAME` are the documented
/// fallbacks.
///
/// The fallback deliberately only looks at windows that carry **no**
/// `_NET_WM_PID` at all: in an X11 session the *main* OpenLess window is also
/// called "OpenLess" and does publish a pid, so restricting the fallback keeps
/// the overlay from ever grabbing the main window.
pub fn select_overlay_window(
    candidates: &[WindowCandidate],
    pid: u32,
) -> Option<(u32, WindowMatch)> {
    if let Some(candidate) = candidates
        .iter()
        .find(|candidate| candidate.pid == Some(pid))
    {
        return Some((candidate.window, WindowMatch::Pid));
    }
    let unowned = || {
        candidates
            .iter()
            .filter(|candidate| candidate.pid.is_none())
    };
    if let Some(candidate) = unowned()
        .filter(|candidate| {
            candidate
                .wm_class
                .as_deref()
                .is_some_and(|class| class.to_ascii_lowercase().contains("openless"))
        })
        // The popup is created after every other OpenLess window, and the tree
        // lists children in creation order, so the last match is ours.
        .last()
    {
        return Some((candidate.window, WindowMatch::Class));
    }
    if let Some(candidate) = unowned()
        .filter(|candidate| {
            candidate
                .name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case("openless"))
        })
        .last()
    {
        return Some((candidate.window, WindowMatch::Name));
    }
    None
}

/// The X11 mutations the overlay needs. Split out so the whole placement
/// sequence can be driven by a recording fake in tests.
pub trait OverlayX11 {
    /// Find our own window: `_NET_WM_PID` first, then the class / name
    /// fallbacks, reporting which strategy matched.
    fn find_own_window(&mut self, pid: u32) -> Result<Option<(u32, WindowMatch)>, String>;
    /// `WM_HINTS.input = False`: the window manager must never assign focus.
    fn set_never_focus(&mut self, window: u32) -> Result<(), String>;
    /// `_NET_WM_WINDOW_TYPE = _NET_WM_WINDOW_TYPE_UTILITY`.
    fn set_window_type(&mut self, window: u32) -> Result<(), String>;
    /// ICCCM `WM_NORMAL_HINTS` with `USPosition`: the position was chosen by the
    /// program, so the window manager must not re-place the window.
    fn mark_self_placed(&mut self, window: u32) -> Result<(), String>;
    /// `_NET_WM_STATE_ABOVE` + `_NET_WM_STATE_SKIP_TASKBAR`.
    fn set_overlay_states(&mut self, window: u32) -> Result<(), String>;
    fn move_window(&mut self, window: u32, position: (i32, i32)) -> Result<(), String>;
    /// `_NET_ACTIVE_WINDOW` right now; `None` when the root has no value.
    fn active_window(&mut self) -> Result<Option<u32>, String>;
    /// Hand the focus back to `window` (the one that had it before we mapped).
    fn restore_focus(&mut self, window: u32) -> Result<(), String>;
}

/// What [`place_overlay`] managed to do; the caller logs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverlayPlacement {
    pub window: Option<u32>,
    /// How the window was identified (`pid` / `wm_class` / `wm_name`).
    pub matched: Option<WindowMatch>,
    pub moved_to: Option<(i32, i32)>,
    /// The compositor had handed us the keyboard and we took it back.
    pub focus_was_stolen: bool,
    pub focus_restored: bool,
    /// Non-fatal problems, in the order they happened.
    pub warnings: Vec<String>,
}

impl OverlayPlacement {
    pub fn applied(&self) -> bool {
        self.window.is_some()
    }
}

/// Point the popup's own X11 window at the bottom centre of the work area and
/// make sure it never holds the keyboard.
///
/// Best effort by design: the pill must still appear if any single step fails,
/// so every failure is collected into [`OverlayPlacement::warnings`] instead of
/// aborting.
pub fn place_overlay(
    x11: &mut dyn OverlayX11,
    pid: u32,
    environment: &OverlayEnvironment,
    kind: crate::popup::PopupKind,
) -> OverlayPlacement {
    let mut placement = OverlayPlacement::default();
    let (window, matched) = match x11.find_own_window(pid) {
        Ok(Some(found)) => found,
        Ok(None) => {
            placement
                .warnings
                .push("own X11 window not found yet".to_string());
            return placement;
        }
        Err(error) => {
            placement
                .warnings
                .push(format!("window lookup failed: {error}"));
            return placement;
        }
    };
    placement.window = Some(window);
    placement.matched = Some(matched);

    if let Err(error) = x11.set_never_focus(window) {
        placement
            .warnings
            .push(format!("input hint failed: {error}"));
    }
    if let Err(error) = x11.set_window_type(window) {
        placement
            .warnings
            .push(format!("window type failed: {error}"));
    }
    if let Err(error) = x11.mark_self_placed(window) {
        placement
            .warnings
            .push(format!("position hint failed: {error}"));
    }
    if let Err(error) = x11.set_overlay_states(window) {
        placement
            .warnings
            .push(format!("overlay states failed: {error}"));
    }
    if let Some(position) = popup_position(environment, kind) {
        match x11.move_window(window, position) {
            Ok(()) => placement.moved_to = Some(position),
            Err(error) => placement.warnings.push(format!("move failed: {error}")),
        }
    } else {
        placement
            .warnings
            .push("no usable work area or monitor".to_string());
    }

    // Only fight the compositor when it really handed us the keyboard: read
    // `_NET_ACTIVE_WINDOW` again and check it is *our* window before taking the
    // focus away from whatever the user is actually looking at now.
    if let Some(previous) = environment.active_window {
        if previous != window {
            match x11.active_window() {
                Ok(Some(current)) if current == window => match x11.restore_focus(previous) {
                    Ok(()) => {
                        placement.focus_was_stolen = true;
                        placement.focus_restored = true;
                    }
                    Err(error) => {
                        placement.focus_was_stolen = true;
                        placement
                            .warnings
                            .push(format!("focus restore failed: {error}"));
                    }
                },
                // We never took the focus (the `input = False` hint did its job,
                // or the user already moved on): leave the focus alone.
                Ok(_) => {}
                Err(error) => placement
                    .warnings
                    .push(format!("focus check failed: {error}")),
            }
        }
    }
    placement
}

#[cfg(all(target_os = "linux", feature = "x11-overlay"))]
mod x11 {
    //! The real connection. Kept behind a feature so the default build never
    //! links X11 (the same binary also runs under pure Wayland).
    use super::{OverlayEnvironment, OverlayX11, X11Rect};
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, PropMode,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    pub struct X11Overlay {
        connection: RustConnection,
        root: u32,
    }

    impl X11Overlay {
        pub fn connect() -> Result<Self, String> {
            let (connection, screen) = RustConnection::connect(None).map_err(|e| e.to_string())?;
            let root = connection.setup().roots[screen].root;
            Ok(Self { connection, root })
        }

        pub fn probe(&self) -> Result<OverlayEnvironment, String> {
            Ok(OverlayEnvironment {
                work_area: self.work_area()?,
                monitors: self.monitors()?,
                cursor: self.cursor()?,
                active_window: self.active_window_property()?,
            })
        }

        fn atom(&self, name: &[u8]) -> Result<u32, String> {
            Ok(self
                .connection
                .intern_atom(false, name)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?
                .atom)
        }

        fn work_area(&self) -> Result<Option<X11Rect>, String> {
            let atom = self.atom(b"_NET_WORKAREA")?;
            let reply = self
                .connection
                .get_property(false, self.root, atom, AtomEnum::CARDINAL, 0, 4)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            let values: Vec<u32> = reply.value32().map(|it| it.collect()).unwrap_or_default();
            if values.len() < 4 {
                return Ok(None);
            }
            Ok(Some(X11Rect {
                x: values[0] as i32,
                y: values[1] as i32,
                width: values[2],
                height: values[3],
            }))
        }

        fn monitors(&self) -> Result<Vec<X11Rect>, String> {
            if let Ok(reply) = self
                .connection
                .randr_get_monitors(self.root, true)
                .map_err(|e| e.to_string())?
                .reply()
            {
                let rects: Vec<X11Rect> = reply
                    .monitors
                    .iter()
                    .map(|monitor| X11Rect {
                        x: monitor.x as i32,
                        y: monitor.y as i32,
                        width: u32::from(monitor.width),
                        height: u32::from(monitor.height),
                    })
                    .collect();
                if !rects.is_empty() {
                    return Ok(rects);
                }
            }
            let screen = self
                .connection
                .setup()
                .roots
                .iter()
                .find(|root| root.root == self.root)
                .ok_or_else(|| "root screen missing".to_string())?;
            Ok(vec![X11Rect {
                x: 0,
                y: 0,
                width: u32::from(screen.width_in_pixels),
                height: u32::from(screen.height_in_pixels),
            }])
        }

        fn cursor(&self) -> Result<Option<(i32, i32)>, String> {
            let reply = self
                .connection
                .query_pointer(self.root)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            Ok(Some((i32::from(reply.root_x), i32::from(reply.root_y))))
        }

        fn active_window_property(&self) -> Result<Option<u32>, String> {
            let atom = self.atom(b"_NET_ACTIVE_WINDOW")?;
            let reply = self
                .connection
                .get_property(false, self.root, atom, AtomEnum::WINDOW, 0, 1)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            Ok(reply
                .value32()
                .and_then(|mut it| it.next())
                .filter(|window| *window != 0))
        }

        fn pid_of(&self, window: u32) -> Result<Option<u32>, String> {
            let atom = self.atom(b"_NET_WM_PID")?;
            let reply = self
                .connection
                .get_property(false, window, atom, AtomEnum::CARDINAL, 0, 1)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            Ok(reply.value32().and_then(|mut it| it.next()))
        }

        fn send_state(&self, window: u32, state: u32) -> Result<(), String> {
            let atom = self.atom(b"_NET_WM_STATE")?;
            let event = ClientMessageEvent::new(
                32,
                window,
                atom,
                [
                    1, /* _NET_WM_STATE_ADD */
                    state, 0, 1, /* application */
                    0,
                ],
            );
            self.connection
                .send_event(
                    false,
                    self.root,
                    EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                    event,
                )
                .map_err(|e| e.to_string())?;
            self.connection.flush().map_err(|e| e.to_string())
        }
    }

    impl X11Overlay {
        /// Every window the overlay could plausibly be: the root's children and
        /// one level deeper, because a window manager may have reparented the
        /// client into a frame window.
        fn own_candidates(&self) -> Result<Vec<super::WindowCandidate>, String> {
            let tree = self
                .connection
                .query_tree(self.root)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            let mut windows = Vec::with_capacity(tree.children.len());
            for &child in &tree.children {
                windows.push(child);
                if let Ok(inner) = self
                    .connection
                    .query_tree(child)
                    .map_err(|e| e.to_string())?
                    .reply()
                {
                    windows.extend(inner.children.iter().copied());
                }
            }
            let mut candidates = Vec::with_capacity(windows.len());
            for window in windows {
                candidates.push(super::WindowCandidate {
                    window,
                    pid: self.pid_of(window)?,
                    // Class / name are only read for the fallback path (see
                    // `own_candidate_details`), so they stay empty here.
                    wm_class: None,
                    name: None,
                });
            }
            Ok(candidates)
        }

        /// `WM_CLASS` instance + class, or `_NET_WM_NAME` / `WM_NAME`.
        fn own_candidate_details(
            &self,
            candidates: &mut [super::WindowCandidate],
        ) -> Result<(), String> {
            for candidate in candidates.iter_mut() {
                if candidate.pid.is_some() {
                    continue;
                }
                candidate.wm_class = self.string_property(candidate.window, AtomEnum::WM_CLASS)?;
                candidate.name = self
                    .utf8_property(candidate.window, b"_NET_WM_NAME")?
                    .or(self.string_property(candidate.window, AtomEnum::WM_NAME)?);
            }
            Ok(())
        }

        fn string_property(
            &self,
            window: u32,
            property: impl Into<u32>,
        ) -> Result<Option<String>, String> {
            let reply = self
                .connection
                .get_property(false, window, property, AtomEnum::STRING, 0, 1024)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            Ok(decode_strings(&reply.value))
        }

        fn utf8_property(&self, window: u32, name: &[u8]) -> Result<Option<String>, String> {
            let atom = self.atom(name)?;
            let reply = self
                .connection
                .get_property(false, window, atom, AtomEnum::ANY, 0, 1024)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            if reply.value.is_empty() {
                return Ok(None);
            }
            Ok(Some(String::from_utf8_lossy(&reply.value).to_string()))
        }
    }

    /// `WM_CLASS` holds two NUL separated strings (instance, class); join them
    /// so the selector can look for "openless" in either one.
    pub(super) fn decode_strings(bytes: &[u8]) -> Option<String> {
        let joined = bytes
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        (!joined.is_empty()).then_some(joined)
    }

    impl OverlayX11 for X11Overlay {
        fn find_own_window(
            &mut self,
            pid: u32,
        ) -> Result<Option<(u32, super::WindowMatch)>, String> {
            let mut candidates = self.own_candidates()?;
            // Fast path: the pid is published, so no class / name round trips.
            if let Some((window, matched)) = super::select_overlay_window(&candidates, pid) {
                log::debug!(
                    "capsule x11: window {window:#x} matched by {}",
                    matched.as_str()
                );
                return Ok(Some((window, matched)));
            }
            // Slow path: only the pid-less windows are of interest.
            self.own_candidate_details(&mut candidates)?;
            Ok(super::select_overlay_window(&candidates, pid))
        }

        fn set_never_focus(&mut self, window: u32) -> Result<(), String> {
            let reply = self
                .connection
                .get_property(false, window, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, 0, 9)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            let mut hints: Vec<u32> = reply
                .value32()
                .map(|it| it.collect::<Vec<u32>>())
                .unwrap_or_default();
            hints.resize(9, 0);
            const INPUT_HINT: u32 = 1 << 0;
            hints[0] |= INPUT_HINT; // flags
            hints[1] = 0; // input = False
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    AtomEnum::WM_HINTS,
                    AtomEnum::WM_HINTS,
                    &hints,
                )
                .map_err(|e| e.to_string())?;
            self.connection.flush().map_err(|e| e.to_string())
        }

        /// `_NET_WM_WINDOW_TYPE_UTILITY`.
        ///
        /// UTILITY rather than NOTIFICATION: KWin treats NOTIFICATION as a
        /// special OSD-style window and applies its own placement / stacking
        /// policy to it, which would fight the explicit geometry we ask for.
        /// A utility window is an ordinary window as far as placement goes (it
        /// honours the client position), while the two properties we *do* want
        /// to inherit from the notification look — never focus, never in the
        /// taskbar, always above — are set explicitly through `WM_HINTS` and
        /// `_NET_WM_STATE` instead of relying on the window type.
        fn set_window_type(&mut self, window: u32) -> Result<(), String> {
            let property = self.atom(b"_NET_WM_WINDOW_TYPE")?;
            let utility = self.atom(b"_NET_WM_WINDOW_TYPE_UTILITY")?;
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    property,
                    AtomEnum::ATOM,
                    &[utility],
                )
                .map_err(|e| e.to_string())?;
            self.connection.flush().map_err(|e| e.to_string())
        }

        /// ICCCM `WM_NORMAL_HINTS`: flag the position as program-specified
        /// (`USPosition` | `PPosition`) so the window manager keeps the
        /// coordinates instead of running its own placement.
        fn mark_self_placed(&mut self, window: u32) -> Result<(), String> {
            let reply = self
                .connection
                .get_property(
                    false,
                    window,
                    AtomEnum::WM_NORMAL_HINTS,
                    AtomEnum::WM_SIZE_HINTS,
                    0,
                    18,
                )
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            let mut hints: Vec<u32> = reply
                .value32()
                .map(|it| it.collect::<Vec<u32>>())
                .unwrap_or_default();
            hints.resize(18, 0);
            const US_POSITION: u32 = 1 << 0;
            const P_POSITION: u32 = 1 << 2;
            hints[0] |= US_POSITION | P_POSITION;
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    AtomEnum::WM_NORMAL_HINTS,
                    AtomEnum::WM_SIZE_HINTS,
                    &hints,
                )
                .map_err(|e| e.to_string())?;
            self.connection.flush().map_err(|e| e.to_string())
        }

        fn active_window(&mut self) -> Result<Option<u32>, String> {
            self.active_window_property()
        }

        fn set_overlay_states(&mut self, window: u32) -> Result<(), String> {
            let above = self.atom(b"_NET_WM_STATE_ABOVE")?;
            let skip = self.atom(b"_NET_WM_STATE_SKIP_TASKBAR")?;
            self.send_state(window, above)?;
            self.send_state(window, skip)
        }

        fn move_window(&mut self, window: u32, position: (i32, i32)) -> Result<(), String> {
            use x11rb::protocol::xproto::ConfigureWindowAux;
            self.connection
                .configure_window(
                    window,
                    &ConfigureWindowAux::new().x(position.0).y(position.1),
                )
                .map_err(|e| e.to_string())?;
            self.connection.flush().map_err(|e| e.to_string())
        }

        fn restore_focus(&mut self, window: u32) -> Result<(), String> {
            let atom = self.atom(b"_NET_ACTIVE_WINDOW")?;
            let event = ClientMessageEvent::new(
                32,
                window,
                atom,
                // source indication 2 = pager: the compositor may refuse to let
                // a normal application move the focus around, but a pager
                // request is the documented way to hand it back.
                [2, 0, 0, 0, 0],
            );
            self.connection
                .send_event(
                    false,
                    self.root,
                    EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                    event,
                )
                .map_err(|e| e.to_string())?;
            self.connection.flush().map_err(|e| e.to_string())
        }
    }
}

#[cfg(all(target_os = "linux", feature = "x11-overlay"))]
pub use x11::X11Overlay;

/// Whether the host has an X server (XWayland counts) for the capsule to use.
pub fn x11_available(display: Option<&str>) -> bool {
    display.is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::PopupKind;

    fn monitor(x: i32, y: i32, width: u32, height: u32) -> X11Rect {
        X11Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn bottom_center_centres_on_a_1080p_screen() {
        let area = monitor(0, 0, 1920, 1080);
        // 1920 - 200 = 1720 / 2 = 860; 1080 - 12 - 100 = 968
        assert_eq!(bottom_center(area, (200, 100), 12), (860, 968));
    }

    #[test]
    fn bottom_center_uses_the_monitor_offset_on_a_second_display() {
        let area = monitor(1920, 0, 2560, 1440);
        assert_eq!(
            bottom_center(area, (200, 100), 12),
            (1920 + 1180, 1440 - 112)
        );
    }

    #[test]
    fn bottom_center_keeps_a_window_wider_than_the_area_inside() {
        let area = monitor(0, 0, 160, 120);
        assert_eq!(bottom_center(area, (200, 100), 12), (0, 8));
    }

    #[test]
    fn bottom_center_never_goes_negative_when_the_area_is_tiny() {
        let area = monitor(100, 50, 40, 30);
        assert_eq!(bottom_center(area, (200, 100), 12), (100, 50));
    }

    #[test]
    fn placement_area_prefers_the_monitor_under_the_cursor() {
        let environment = OverlayEnvironment {
            work_area: Some(monitor(0, 0, 3840, 1080)),
            monitors: vec![monitor(0, 0, 1920, 1080), monitor(1920, 0, 1920, 1080)],
            cursor: Some((2500, 500)),
            ..Default::default()
        };
        // The work area is the whole desktop; intersecting it with the right
        // hand monitor keeps the pill on that screen.
        assert_eq!(
            environment.placement_area(),
            Some(monitor(1920, 0, 1920, 1080))
        );
        assert_eq!(
            environment.position_for((200, 100), 12),
            Some((1920 + 860, 968))
        );
    }

    #[test]
    fn placement_area_falls_back_to_the_first_monitor_without_a_cursor() {
        let environment = OverlayEnvironment {
            monitors: vec![monitor(0, 0, 1920, 1080), monitor(1920, 0, 1920, 1080)],
            ..Default::default()
        };
        assert_eq!(
            environment.placement_area(),
            Some(monitor(0, 0, 1920, 1080))
        );
    }

    #[test]
    fn placement_area_uses_the_work_area_when_there_are_no_monitors() {
        let environment = OverlayEnvironment {
            work_area: Some(monitor(0, 40, 1920, 1040)),
            ..Default::default()
        };
        assert_eq!(
            environment.placement_area(),
            Some(monitor(0, 40, 1920, 1040))
        );
        // A 40px taskbar at the top moves the work area's *top* edge only, so
        // the pill keeps hugging the same bottom edge (1080 - 112).
        assert_eq!(environment.position_for((200, 100), 12), Some((860, 968)));
    }

    #[test]
    fn placement_area_is_none_without_any_x11_geometry() {
        assert_eq!(OverlayEnvironment::default().placement_area(), None);
        assert_eq!(
            OverlayEnvironment::default().position_for((200, 100), 12),
            None
        );
    }

    #[derive(Default)]
    struct FakeX11 {
        window: Option<u32>,
        matched: Option<WindowMatch>,
        fail_input: bool,
        /// `_NET_ACTIVE_WINDOW` when the placement re-reads it after the move.
        current_active: Option<u32>,
        calls: Vec<String>,
    }

    impl OverlayX11 for FakeX11 {
        fn find_own_window(&mut self, pid: u32) -> Result<Option<(u32, WindowMatch)>, String> {
            self.calls.push(format!("find({pid})"));
            Ok(self
                .window
                .map(|window| (window, self.matched.unwrap_or(WindowMatch::Pid))))
        }
        fn set_never_focus(&mut self, window: u32) -> Result<(), String> {
            self.calls.push(format!("never_focus({window})"));
            if self.fail_input {
                return Err("nope".to_string());
            }
            Ok(())
        }
        fn set_window_type(&mut self, window: u32) -> Result<(), String> {
            self.calls.push(format!("window_type({window})"));
            Ok(())
        }
        fn mark_self_placed(&mut self, window: u32) -> Result<(), String> {
            self.calls.push(format!("self_placed({window})"));
            Ok(())
        }
        fn set_overlay_states(&mut self, window: u32) -> Result<(), String> {
            self.calls.push(format!("states({window})"));
            Ok(())
        }
        fn move_window(&mut self, window: u32, position: (i32, i32)) -> Result<(), String> {
            self.calls
                .push(format!("move({window},{},{})", position.0, position.1));
            Ok(())
        }
        fn active_window(&mut self) -> Result<Option<u32>, String> {
            self.calls.push("active_window".to_string());
            Ok(self.current_active)
        }
        fn restore_focus(&mut self, window: u32) -> Result<(), String> {
            self.calls.push(format!("focus({window})"));
            Ok(())
        }
    }

    fn environment() -> OverlayEnvironment {
        OverlayEnvironment {
            work_area: Some(monitor(0, 0, 1920, 1080)),
            monitors: vec![monitor(0, 0, 1920, 1080)],
            cursor: Some((10, 10)),
            active_window: Some(0x40),
        }
    }

    #[test]
    fn popup_size_maps_every_kind() {
        assert_eq!(popup_size(PopupKind::Capsule), CAPSULE_WINDOW_SIZE);
        assert_eq!(popup_size(PopupKind::Qa), QA_WINDOW_SIZE);
        // Tauri 的 `less-computer` 窗口与 qa 同为 420×540。
        assert_eq!(
            popup_size(PopupKind::LessComputer),
            LESS_COMPUTER_WINDOW_SIZE
        );
        assert_eq!(LESS_COMPUTER_WINDOW_SIZE, (420, 540));
    }

    /// The capsule hugs the bottom edge of the work area, centred.
    #[test]
    fn popup_position_puts_the_capsule_at_the_bottom_centre() {
        assert_eq!(
            popup_position(&environment(), PopupKind::Capsule),
            Some((860, 968))
        );
    }

    /// The panels are centred dialogs, and centring is what keeps them from
    /// running into the capsule strip: a 540px panel 50px above the bottom edge
    /// would start at y=490 and end at 1030, i.e. inside the pill's 968..1068.
    #[test]
    fn popup_position_centres_the_panels_clear_of_the_capsule() {
        // 尺寸取自 Tauri：qa 420×540（润色结果已并入这个面板，不再有独立预览窗口），
        // 居中于 1920×1080 工作区。
        assert_eq!(
            popup_position(&environment(), PopupKind::Qa),
            Some((750, 270))
        );
        let area = monitor(0, 0, 1920, 1080);
        let (_, capsule_y) = bottom_center(area, CAPSULE_WINDOW_SIZE, CAPSULE_BOTTOM_GAP);
        for kind in [PopupKind::Qa] {
            let (_, y) = popup_position(&environment(), kind).expect("centred");
            let height = popup_size(kind).1 as i32;
            assert!(
                y + height <= capsule_y,
                "{kind:?} overlaps the capsule strip: {}..{} vs {}",
                y,
                y + height,
                capsule_y
            );
        }
    }

    /// A work area smaller than the window keeps the window at its origin
    /// instead of producing a negative position.
    #[test]
    fn popup_position_keeps_an_oversized_panel_inside_a_tiny_area() {
        let environment = OverlayEnvironment {
            work_area: Some(monitor(0, 0, 400, 300)),
            monitors: vec![monitor(0, 0, 400, 300)],
            cursor: None,
            active_window: None,
        };
        assert_eq!(popup_position(&environment, PopupKind::Qa), Some((0, 0)),);
        assert_eq!(
            popup_position(&environment, PopupKind::Capsule),
            Some((100, 188)),
        );
    }

    /// A capsule that did take the keyboard: everything is asserted on one
    /// request sequence, including the order (input hint + window type +
    /// position hint before the EWMH states and the move).
    #[test]
    fn place_overlay_never_focuses_moves_and_restores_the_previous_window() {
        let mut x11 = FakeX11 {
            window: Some(0x2a),
            current_active: Some(0x2a),
            ..Default::default()
        };
        let placement = place_overlay(&mut x11, 4242, &environment(), PopupKind::Capsule);
        assert_eq!(
            x11.calls,
            vec![
                "find(4242)",
                "never_focus(42)", // 0x2a
                "window_type(42)",
                "self_placed(42)",
                "states(42)",
                "move(42,860,968)",
                "active_window",
                "focus(64)", // 0x40
            ]
        );
        assert_eq!(placement.window, Some(0x2a));
        assert_eq!(placement.matched, Some(WindowMatch::Pid));
        assert_eq!(placement.moved_to, Some((860, 968)));
        assert!(placement.focus_was_stolen);
        assert!(placement.focus_restored);
        assert!(placement.warnings.is_empty());
        assert!(placement.applied());
    }

    #[test]
    fn place_overlay_keeps_going_when_the_input_hint_fails() {
        let mut x11 = FakeX11 {
            window: Some(0x2a),
            fail_input: true,
            ..Default::default()
        };
        let placement = place_overlay(&mut x11, 1, &environment(), PopupKind::Capsule);
        assert!(placement.applied());
        assert_eq!(placement.moved_to, Some((860, 968)));
        assert_eq!(placement.warnings, vec!["input hint failed: nope"]);
    }

    #[test]
    fn place_overlay_reports_a_missing_window() {
        let mut x11 = FakeX11::default();
        let placement = place_overlay(&mut x11, 7, &environment(), PopupKind::Capsule);
        assert!(!placement.applied());
        assert_eq!(placement.warnings, vec!["own X11 window not found yet"]);
        assert_eq!(x11.calls, vec!["find(7)"]);
    }

    #[test]
    fn place_overlay_does_not_restore_focus_when_we_already_had_it() {
        let mut x11 = FakeX11 {
            window: Some(0x40),
            ..Default::default()
        };
        let mut environment = environment();
        environment.active_window = Some(0x40);
        let placement = place_overlay(&mut x11, 1, &environment, PopupKind::Capsule);
        assert!(!placement.focus_was_stolen);
        assert!(!placement.focus_restored);
        assert!(!x11.calls.iter().any(|call| call.starts_with("focus(")));
    }

    /// The `WM_HINTS.input = False` hint (or the user moving on) means the
    /// focus never landed on us: the overlay must not yank it to a stale window.
    #[test]
    fn place_overlay_leaves_the_focus_alone_when_it_was_never_stolen() {
        let mut x11 = FakeX11 {
            window: Some(0x2a),
            current_active: Some(0x99),
            ..Default::default()
        };
        let placement = place_overlay(&mut x11, 1, &environment(), PopupKind::Capsule);
        assert!(placement.applied());
        assert!(placement.focus_was_stolen == false);
        assert!(placement.focus_restored == false);
        assert!(x11.calls.contains(&"active_window".to_string()));
        assert!(!x11.calls.iter().any(|call| call.starts_with("focus(")));
        assert!(placement.warnings.is_empty());
    }

    fn candidate(window: u32, pid: Option<u32>) -> WindowCandidate {
        WindowCandidate {
            window,
            pid,
            wm_class: None,
            name: None,
        }
    }

    #[test]
    fn select_overlay_window_prefers_the_pid() {
        let candidates = vec![candidate(0x1, Some(7)), candidate(0x2, Some(4242))];
        assert_eq!(
            select_overlay_window(&candidates, 4242),
            Some((0x2, WindowMatch::Pid))
        );
    }

    #[test]
    fn select_overlay_window_falls_back_to_the_wm_class() {
        let candidates = vec![
            candidate(0x1, None),
            WindowCandidate {
                wm_class: Some("openless OpenLess".to_string()),
                ..candidate(0x2, None)
            },
        ];
        assert_eq!(
            select_overlay_window(&candidates, 4242),
            Some((0x2, WindowMatch::Class))
        );
    }

    #[test]
    fn select_overlay_window_falls_back_to_the_window_name() {
        let candidates = vec![WindowCandidate {
            name: Some("OpenLess".to_string()),
            ..candidate(0x5, None)
        }];
        assert_eq!(
            select_overlay_window(&candidates, 4242),
            Some((0x5, WindowMatch::Name))
        );
    }

    /// The main window is also called "OpenLess" but publishes a pid, so the
    /// class / name fallback must never claim it.
    #[test]
    fn select_overlay_window_ignores_windows_owned_by_another_process() {
        let candidates = vec![
            WindowCandidate {
                wm_class: Some("openless OpenLess".to_string()),
                ..candidate(0x1, Some(99))
            },
            WindowCandidate {
                name: Some("OpenLess".to_string()),
                ..candidate(0x2, Some(99))
            },
        ];
        assert_eq!(select_overlay_window(&candidates, 4242), None);
    }

    #[test]
    fn select_overlay_window_takes_the_newest_class_match() {
        let candidates = vec![
            WindowCandidate {
                wm_class: Some("openless OpenLess".to_string()),
                ..candidate(0x1, None)
            },
            WindowCandidate {
                wm_class: Some("openless OpenLess".to_string()),
                ..candidate(0x2, None)
            },
        ];
        assert_eq!(
            select_overlay_window(&candidates, 4242),
            Some((0x2, WindowMatch::Class))
        );
    }

    #[test]
    fn select_overlay_window_reports_nothing_without_a_match() {
        let candidates = vec![
            candidate(0x1, Some(7)),
            WindowCandidate {
                wm_class: Some("firefox Firefox".to_string()),
                ..candidate(0x2, None)
            },
        ];
        assert_eq!(select_overlay_window(&candidates, 4242), None);
    }

    #[cfg(all(target_os = "linux", feature = "x11-overlay"))]
    #[test]
    fn wm_class_is_decoded_into_a_searchable_string() {
        // `WM_CLASS` is two NUL separated strings: instance + class.
        assert_eq!(
            super::x11::decode_strings(b"openless\0OpenLess\0").as_deref(),
            Some("openless OpenLess")
        );
        // Some clients send only the class (or nothing at all).
        assert_eq!(
            super::x11::decode_strings(b"OpenLess\0").as_deref(),
            Some("OpenLess")
        );
        assert_eq!(super::x11::decode_strings(b""), None);
    }

    #[test]
    fn x11_available_follows_the_display_variable() {
        assert!(x11_available(Some(":0")));
        assert!(!x11_available(Some("  ")));
        assert!(!x11_available(None));
    }
}
