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
//!   (pointer clicks still reach the pill's ✕ / ✓ buttons), and
//! * an explicit `ConfigureWindow` can place it at the bottom centre of the
//!   work area, mirroring the Tauri host's `position_capsule_bottom_center_with_style`.
//!
//! The maths lives in [`OverlayEnvironment`] so it is unit-testable, and every
//! X11 mutation goes through the [`OverlayX11`] trait so the request sequence
//! can be asserted without an X server.

/// Capsule window size (the 176×42 pill plus room for the translate badge).
pub const CAPSULE_WINDOW_SIZE: (u32, u32) = (200, 100);
/// Gap between the capsule pill and the bottom of the work area — Tauri's
/// `EDGE_GAP` for the classic / siri capsule styles.
pub const CAPSULE_BOTTOM_GAP: i32 = 12;
/// Selection-ask panel size (the chat panel).
pub const QA_WINDOW_SIZE: (u32, u32) = (520, 520);
/// Selection-ask panel bottom gap: where the capsule pill would sit plus
/// Tauri's `QA_WINDOW_GAP_TO_CAPSULE` (8), so the panel clears the pill.
pub const QA_BOTTOM_GAP: i32 = 42 + 8;
/// Polish-preview panel size.
pub const PREVIEW_WINDOW_SIZE: (u32, u32) = (480, 320);
/// Polish-preview bottom gap: centred a little above the capsule.
pub const PREVIEW_BOTTOM_GAP: i32 = 120;

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

/// The X11 mutations the overlay needs. Split out so the whole placement
/// sequence can be driven by a recording fake in tests.
pub trait OverlayX11 {
    fn find_window_for_pid(&mut self, pid: u32) -> Result<Option<u32>, String>;
    /// `WM_HINTS.input = False`: the window manager must never assign focus.
    fn set_never_focus(&mut self, window: u32) -> Result<(), String>;
    /// `_NET_WM_STATE_ABOVE` + `_NET_WM_STATE_SKIP_TASKBAR`.
    fn set_overlay_states(&mut self, window: u32) -> Result<(), String>;
    fn move_window(&mut self, window: u32, position: (i32, i32)) -> Result<(), String>;
    /// Hand the focus back to `window` (the one that had it before we mapped).
    fn restore_focus(&mut self, window: u32) -> Result<(), String>;
}

/// What [`place_overlay`] managed to do; the caller logs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverlayPlacement {
    pub window: Option<u32>,
    pub moved_to: Option<(i32, i32)>,
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
    window_size: (u32, u32),
    bottom_gap: i32,
) -> OverlayPlacement {
    let mut placement = OverlayPlacement::default();
    let window = match x11.find_window_for_pid(pid) {
        Ok(Some(window)) => window,
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

    if let Err(error) = x11.set_never_focus(window) {
        placement
            .warnings
            .push(format!("input hint failed: {error}"));
    }
    if let Err(error) = x11.set_overlay_states(window) {
        placement
            .warnings
            .push(format!("overlay states failed: {error}"));
    }
    if let Some(position) = environment.position_for(window_size, bottom_gap) {
        match x11.move_window(window, position) {
            Ok(()) => placement.moved_to = Some(position),
            Err(error) => placement.warnings.push(format!("move failed: {error}")),
        }
    } else {
        placement
            .warnings
            .push("no usable work area or monitor".to_string());
    }

    // Only fight the compositor when it actually handed us the focus.
    if let Some(previous) = environment.active_window {
        if previous != window {
            match x11.restore_focus(previous) {
                Ok(()) => placement.focus_restored = true,
                Err(error) => placement
                    .warnings
                    .push(format!("focus restore failed: {error}")),
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
                active_window: self.active_window()?,
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

        fn active_window(&self) -> Result<Option<u32>, String> {
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

    impl OverlayX11 for X11Overlay {
        fn find_window_for_pid(&mut self, pid: u32) -> Result<Option<u32>, String> {
            let tree = self
                .connection
                .query_tree(self.root)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            for &child in &tree.children {
                if self.pid_of(child)? == Some(pid) {
                    return Ok(Some(child));
                }
            }
            // A window manager may already have reparented the client into a
            // frame window, so look one level deeper as well.
            for &child in &tree.children {
                let Ok(inner) = self
                    .connection
                    .query_tree(child)
                    .map_err(|e| e.to_string())?
                    .reply()
                else {
                    continue;
                };
                for &grandchild in &inner.children {
                    if self.pid_of(grandchild)? == Some(pid) {
                        return Ok(Some(grandchild));
                    }
                }
            }
            Ok(None)
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
        fail_input: bool,
        calls: Vec<String>,
    }

    impl OverlayX11 for FakeX11 {
        fn find_window_for_pid(&mut self, pid: u32) -> Result<Option<u32>, String> {
            self.calls.push(format!("find({pid})"));
            Ok(self.window)
        }
        fn set_never_focus(&mut self, window: u32) -> Result<(), String> {
            self.calls.push(format!("never_focus({window})"));
            if self.fail_input {
                return Err("nope".to_string());
            }
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
    fn place_overlay_never_focuses_moves_and_restores_the_previous_window() {
        let mut x11 = FakeX11 {
            window: Some(0x2a),
            ..Default::default()
        };
        let placement = place_overlay(&mut x11, 4242, &environment(), (200, 100), 12);
        assert_eq!(
            x11.calls,
            vec![
                "find(4242)",
                "never_focus(42)", // 0x2a
                "states(42)",
                "move(42,860,968)",
                "focus(64)", // 0x40
            ]
        );
        assert_eq!(placement.window, Some(0x2a));
        assert_eq!(placement.moved_to, Some((860, 968)));
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
        let placement = place_overlay(&mut x11, 1, &environment(), (200, 100), 12);
        assert!(placement.applied());
        assert_eq!(placement.moved_to, Some((860, 968)));
        assert_eq!(placement.warnings, vec!["input hint failed: nope"]);
    }

    #[test]
    fn place_overlay_reports_a_missing_window() {
        let mut x11 = FakeX11::default();
        let placement = place_overlay(&mut x11, 7, &environment(), (200, 100), 12);
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
        let placement = place_overlay(&mut x11, 1, &environment, (200, 100), 12);
        assert!(!placement.focus_restored);
        assert!(!x11.calls.iter().any(|call| call.starts_with("focus(")));
    }

    #[test]
    fn x11_available_follows_the_display_variable() {
        assert!(x11_available(Some(":0")));
        assert!(!x11_available(Some("  ")));
        assert!(!x11_available(None));
    }
}
