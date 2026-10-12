//! Native Wayland surface for the recording capsule: `zwlr_layer_shell_v1`.
//!
//! The capsule must sit at the bottom centre of the screen and must **never**
//! take the keyboard — the user is dictating into another window, and a focus
//! steal would send the insert to the wrong place. `xdg-shell` offers neither an
//! absolute position nor a focus opt-out, so Wayland-only compositors without
//! layer-shell do not show the capsule. Compositors that
//! implement `zwlr_layer_shell_v1` — KWin 6.7+ (verified against
//! `zwlr_layer_shell_v1` version 5), sway, Hyprland, labwc, … — can host the
//! capsule natively instead:
//!
//! * `Anchor::Bottom` + `margin.bottom` places the pill bottom-centre without
//!   the client ever knowing the screen size (a surface with only one
//!   horizontal anchor is centred by the compositor, and the input region stays
//!   the pill's own box instead of the whole bottom strip),
//! * `KeyboardInteractivity::None` makes keyboard focus impossible,
//! * `exclusive_zone(0)` keeps the capsule non-exclusive while asking the
//!   compositor to respect reserved panel/taskbar space.
//!
//! Rendering reuses the popup's existing egui view ([`crate::ui::frontend::popups::dictation_capsule`]):
//! the runner below owns a Vulkan/WGPU surface and the Wayland event loop, and
//! asks the caller for one egui frame at a time.
//!
//! Everything that can be decided without a compositor lives in pure functions
//! ([`has_layer_shell`], [`choose_capsule_path`], [`capsule_geometry`],
//! [`pointer_events`]) so the policy is unit-testable; the I/O half is a thin
//! shell around them and reports failures as `Err`.

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::os::fd::AsRawFd;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_compositor, wl_pointer, wl_registry, wl_seat, wl_surface, wl_touch,
};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

/// The layer-shell global this module needs.
pub const LAYER_SHELL_GLOBAL: &str = "zwlr_layer_shell_v1";
/// Wayland namespace of the capsule surface (shows up in compositor logs and
/// `swaymsg -t get_tree`-style tooling).
pub const LAYER_NAMESPACE: &str = "openless-capsule";
/// How long to wait for the first `configure` event before giving up and
/// falling back to the X11 overlay.
pub const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(3);
/// Upper bound for one sleep between frames, so protocol events are never
/// starved even when the view asks for a long pause.
pub const MAX_FRAME_PAUSE: Duration = Duration::from_millis(100);

// ── pure decision core ──────────────────────────────────────────────────────

/// Whether the compositor advertised `zwlr_layer_shell_v1`.
pub fn has_layer_shell(globals: &[String]) -> bool {
    globals.iter().any(|global| global == LAYER_SHELL_GLOBAL)
}

/// Whether a Wayland session is present (`WAYLAND_DISPLAY` non-empty).
pub fn wayland_display_available(display: Option<&str>) -> bool {
    display.is_some_and(|value| !value.trim().is_empty())
}

/// How the capsule window should be hosted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapsulePath {
    /// Native layer surface (bottom-centre, keyboard-impossible).
    LayerShell,
    /// The existing native X11 overlay in [`crate::popup_window`].
    X11Overlay,
    /// Wayland is active but layer-shell is unavailable. Do not use a focusable
    /// xdg-shell toplevel or route the surface through XWayland.
    PlainWindow,
}

/// Pick the capsule host. Wayland uses layer-shell or no capsule; X11 is used
/// only when the whole desktop session is native X11.
pub fn choose_capsule_path(
    wayland_display: Option<&str>,
    x11_display: Option<&str>,
    globals: &[String],
) -> CapsulePath {
    if wayland_display_available(wayland_display) {
        return if has_layer_shell(globals) {
            CapsulePath::LayerShell
        } else {
            CapsulePath::PlainWindow
        };
    }
    if crate::popup_window::x11_available(x11_display) {
        return CapsulePath::X11Overlay;
    }
    CapsulePath::PlainWindow
}

/// Environment variable that pins the capsule host for local diagnostics.
pub const CAPSULE_PATH_ENV: &str = "OPENLESS_CAPSULE_PATH";

/// Parse [`CAPSULE_PATH_ENV`]. Unknown or empty values are ignored so a typo
/// cannot leave the capsule without a window.
pub fn capsule_path_override(value: Option<&str>) -> Option<CapsulePath> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "layer" | "layer-shell" | "layer_shell" => Some(CapsulePath::LayerShell),
        "x11" => Some(CapsulePath::X11Overlay),
        "plain" | "none" => Some(CapsulePath::PlainWindow),
        _ => None,
    }
}

/// Whether the compositor offers `zwlr_layer_shell_v1`, memoised: the capsule is
/// launched once per dictation and probing opens a Wayland connection.
///
/// [`CAPSULE_PATH_ENV`] wins over the probe, and both this and
/// [`detect_capsule_path`] read it, so the parent (which decides the child's
/// backend) and the child (which decides how to host the window) always agree.
pub fn layer_shell_available() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        if let Some(forced) = capsule_path_override(std::env::var(CAPSULE_PATH_ENV).ok().as_deref())
        {
            return forced == CapsulePath::LayerShell;
        }
        probe_layer_shell(std::env::var("WAYLAND_DISPLAY").ok().as_deref())
    })
}

/// Decide how this process should host the capsule: an explicit override wins,
/// otherwise probe the live session and apply [`choose_capsule_path`]. Never
/// fails; a missing session, a failed connection or an absent global all end up
/// on the fallback path.
pub fn detect_capsule_path() -> CapsulePath {
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let x11 = std::env::var("DISPLAY").ok();
    if wayland_display_available(wayland.as_deref()) {
        if let Some(forced) = capsule_path_override(std::env::var(CAPSULE_PATH_ENV).ok().as_deref())
        {
            if forced != CapsulePath::X11Overlay {
                log::info!("capsule path forced by {CAPSULE_PATH_ENV}: {forced:?}");
                return forced;
            }
            log::warn!("ignoring X11 capsule override in a Wayland session");
        }
        return match probe_globals() {
            Ok(globals) if has_layer_shell(&globals) => CapsulePath::LayerShell,
            Ok(_) => CapsulePath::PlainWindow,
            Err(error) => {
                log::warn!("layer-shell probe failed ({error}); not using XWayland");
                CapsulePath::PlainWindow
            }
        };
    }
    if let Some(forced) = capsule_path_override(std::env::var(CAPSULE_PATH_ENV).ok().as_deref()) {
        log::info!("capsule path forced by {CAPSULE_PATH_ENV}: {forced:?}");
        return forced;
    }
    choose_capsule_path(None, x11.as_deref(), &[])
}

/// Interface names the compositor advertises, or an error when there is no
/// usable Wayland connection.
fn probe_globals() -> Result<Vec<String>, String> {
    let connection = Connection::connect_to_env().map_err(|error| format!("wayland: {error}"))?;
    // `registry_queue_init` already round-trips the registry and hands back the
    // `GlobalList` it collected. Dispatching a second time into our own state
    // collects nothing (the global events are already consumed), which would
    // make every compositor look like it lacks layer-shell.
    let (globals, _queue) = registry_queue_init::<LayerState>(&connection)
        .map_err(|error| format!("wayland registry: {error}"))?;
    Ok(globals
        .contents()
        .with_list(|list| list.iter().map(|global| global.interface.clone()).collect()))
}

/// Layer-surface geometry: the surface is a fixed-size child of the compositor,
/// so only the size and the bottom margin matter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsuleGeometry {
    pub width: u32,
    pub height: u32,
    pub bottom_gap: i32,
}

impl CapsuleGeometry {
    /// The buffer size the compositor is asked for. Zero would be rejected by
    /// the protocol, so both axes clamp to at least one pixel.
    pub fn buffer_size(self) -> (u32, u32) {
        (self.width.max(1), self.height.max(1))
    }

    /// egui viewport for one frame, matching the requested buffer size.
    pub fn rect(self) -> egui::Rect {
        let (width, height) = self.buffer_size();
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32))
    }

    /// Ignore a compositor-suggested size of zero (the protocol allows it while
    /// the surface is still unconfigured) and otherwise follow the compositor.
    pub fn rect_for_configure(self, configure: (u32, u32)) -> egui::Rect {
        let (width, height) = match configure {
            (0, _) | (_, 0) => self.buffer_size(),
            (width, height) => (width, height),
        };
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32))
    }
}

/// Capsule geometry from the shared window-size constants.
pub fn capsule_geometry(width: u32, height: u32, bottom_gap: i32) -> CapsuleGeometry {
    CapsuleGeometry {
        width: width.max(1),
        height: height.max(1),
        bottom_gap: bottom_gap.max(0),
    }
}

/// Translate pointer samples into egui events. Pure so the mapping is testable
/// without a compositor; `pressed` carries the button state at that point.
pub fn pointer_events(
    position: Option<egui::Pos2>,
    buttons: &[(egui::Pos2, egui::PointerButton, bool)],
    left: bool,
) -> Vec<egui::Event> {
    let mut events = Vec::new();
    if let Some(position) = position {
        events.push(egui::Event::PointerMoved(position));
    }
    for (position, button, pressed) in buttons {
        events.push(egui::Event::PointerButton {
            pos: *position,
            button: *button,
            pressed: *pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    if left {
        events.push(egui::Event::PointerGone);
    }
    events
}

/// One frame produced by the caller: the egui output plus the loop's control
/// flow.
pub struct LayerFrame {
    /// Result of `egui::Context::run` for this frame.
    pub output: egui::FullOutput,
    /// Ask the runner to stop (host requested shutdown, or the user pressed
    /// cancel / confirm).
    pub exit: bool,
    /// When to draw the next frame.
    pub repaint_after: Duration,
    pub capsule_layout: Option<(CapsuleGeometry, bool)>,
}

// ── compositor probe ────────────────────────────────────────────────────────

/// Ask the running compositor whether it implements `zwlr_layer_shell_v1`.
///
/// Returns `false` (never an error) when there is no Wayland session, when the
/// connection fails, or when the global is missing — the caller then falls back.
pub fn probe_layer_shell(wayland_display: Option<&str>) -> bool {
    if !wayland_display_available(wayland_display) {
        return false;
    }
    probe_globals().is_ok_and(|globals| has_layer_shell(&globals))
}

// ── wayland state ───────────────────────────────────────────────────────────

/// Everything the capsule surface needs from the compositor, plus the pointer
/// events collected between frames.
#[derive(Default)]
struct LayerState {
    compositor: Option<wl_compositor::WlCompositor>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    touch: Option<wl_touch::WlTouch>,
    touch_input: TouchInput,
    pointer_position: Option<egui::Pos2>,
    /// Set when the pointer actually moved (or entered) since the last frame, so
    /// a stationary pointer does not emit a motion event every frame.
    pointer_moved: bool,
    pressed: Vec<(egui::Pos2, egui::PointerButton, bool)>,
    pointer_left: bool,
    configure: Option<(u32, u32)>,
    closed: bool,
}

impl LayerState {
    /// Take the pointer events collected since the last frame.
    fn take_input(&mut self, rect: egui::Rect) -> egui::RawInput {
        let events = pointer_events(
            None,
            &std::mem::take(&mut self.pressed),
            std::mem::take(&mut self.pointer_left),
        );
        // PointerMoved must lead so egui's interaction position is current
        // before the button events are applied.
        let mut all = Vec::with_capacity(events.len() + 1);
        if std::mem::take(&mut self.pointer_moved) {
            if let Some(position) = self.pointer_position {
                all.push(egui::Event::PointerMoved(position));
            }
        }
        all.extend(events);
        all.append(&mut self.touch_input.events);
        egui::RawInput {
            screen_rect: Some(rect),
            events: all,
            focused: true,
            ..Default::default()
        }
    }
}

/// Native touchscreen contacts also need pointer emulation for egui buttons,
/// just as egui-winit does. Do not promote another finger during a gesture.
#[derive(Default)]
struct TouchInput {
    contacts: BTreeMap<i32, egui::Pos2>,
    primary: Option<i32>,
    events: Vec<egui::Event>,
}

impl TouchInput {
    fn emit(&mut self, id: i32, phase: egui::TouchPhase, pos: egui::Pos2) {
        self.events.push(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId::from(id),
            phase,
            pos,
            force: None,
        });
    }

    fn button(&mut self, pos: egui::Pos2, pressed: bool) {
        self.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }

    fn cancel_pointer(&mut self) {
        if self.primary.take().is_some() {
            // PointerGone alone leaves egui's button held. Move off-surface
            // before releasing, so cancellation cannot become a button click.
            let outside = egui::pos2(-100_000.0, -100_000.0);
            self.events.push(egui::Event::PointerMoved(outside));
            self.button(outside, false);
            self.events.push(egui::Event::PointerGone);
        }
    }

    fn down(&mut self, id: i32, pos: egui::Pos2) {
        let single = self.contacts.is_empty();
        let std::collections::btree_map::Entry::Vacant(contact) = self.contacts.entry(id) else {
            return;
        };
        contact.insert(pos);
        self.emit(id, egui::TouchPhase::Start, pos);
        if single {
            self.primary = Some(id);
            self.events.push(egui::Event::PointerMoved(pos));
            self.button(pos, true);
        } else {
            // Multi-finger gestures must not accidentally stop dictation.
            self.cancel_pointer();
        }
    }

    fn motion(&mut self, id: i32, pos: egui::Pos2) {
        let Some(contact) = self.contacts.get_mut(&id) else {
            return;
        };
        *contact = pos;
        self.emit(id, egui::TouchPhase::Move, pos);
        if self.primary == Some(id) {
            self.events.push(egui::Event::PointerMoved(pos));
        }
    }

    fn up(&mut self, id: i32) {
        let Some(pos) = self.contacts.remove(&id) else {
            return;
        };
        self.emit(id, egui::TouchPhase::End, pos);
        if self.primary == Some(id) {
            self.primary = None;
            self.button(pos, false);
            self.events.push(egui::Event::PointerGone);
        }
    }

    fn cancel(&mut self) {
        self.cancel_pointer();
        for (id, pos) in std::mem::take(&mut self.contacts) {
            self.emit(id, egui::TouchPhase::Cancel, pos);
        }
    }
}

fn button_from_code(button: u32) -> Option<egui::PointerButton> {
    match button {
        0x110 => Some(egui::PointerButton::Primary),
        0x111 => Some(egui::PointerButton::Secondary),
        0x112 => Some(egui::PointerButton::Middle),
        _ => None,
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for LayerState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &GlobalListContents,
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "wl_compositor" => {
                state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
            }
            // Layer shell is at version 4 in the widest-deployed compositors and
            // 5 in KWin 6.7; everything this module sets exists since version 1.
            LAYER_SHELL_GLOBAL => {
                state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()));
            }
            "wl_seat" => {
                state.seat = Some(registry.bind(name, version.min(7), qh, ()));
            }
            _ => {}
        }
    }
}

macro_rules! ignore_events {
    ($interface:ty) => {
        impl Dispatch<$interface, ()> for LayerState {
            fn event(
                _state: &mut Self,
                _proxy: &$interface,
                _event: <$interface as Proxy>::Event,
                _data: &(),
                _connection: &Connection,
                _qh: &QueueHandle<Self>,
            ) {
            }
        }
    };
}

ignore_events!(wl_compositor::WlCompositor);
ignore_events!(wl_surface::WlSurface);

impl Dispatch<wl_seat::WlSeat, ()> for LayerState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
            }
            if capabilities.contains(wl_seat::Capability::Touch) {
                if state.touch.is_none() {
                    state.touch = Some(seat.get_touch(qh, ()));
                }
            } else if let Some(touch) = state.touch.take() {
                state.touch_input.cancel();
                if touch.version() >= 3 {
                    touch.release();
                }
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for LayerState {
    fn event(
        state: &mut Self,
        _pointer: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            }
            | wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                state.pointer_position = Some(egui::pos2(surface_x as f32, surface_y as f32));
                state.pointer_moved = true;
            }
            wl_pointer::Event::Leave { .. } => {
                state.pointer_position = None;
                state.pointer_left = true;
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                let WEnum::Value(button_state) = button_state else {
                    return;
                };
                let Some(position) = state.pointer_position else {
                    return;
                };
                let Some(button) = button_from_code(button) else {
                    return;
                };
                let pressed = button_state == wl_pointer::ButtonState::Pressed;
                state.pressed.push((position, button, pressed));
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_touch::WlTouch, ()> for LayerState {
    fn event(
        state: &mut Self,
        _touch: &wl_touch::WlTouch,
        event: wl_touch::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_touch::Event::Down { id, x, y, .. } => {
                state.touch_input.down(id, egui::pos2(x as f32, y as f32));
            }
            wl_touch::Event::Motion { id, x, y, .. } => {
                state.touch_input.motion(id, egui::pos2(x as f32, y as f32));
            }
            wl_touch::Event::Up { id, .. } => state.touch_input.up(id),
            wl_touch::Event::Cancel => state.touch_input.cancel(),
            _ => {}
        }
    }
}

ignore_events!(zwlr_layer_shell_v1::ZwlrLayerShellV1);

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for LayerState {
    fn event(
        state: &mut Self,
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer_surface.ack_configure(serial);
                state.configure = Some((width, height));
            }
            zwlr_layer_surface_v1::Event::Closed => state.closed = true,
            _ => {}
        }
    }
}

// ── Vulkan/WGPU plumbing ────────────────────────────────────────────────────

fn capsule_alpha_mode(
    supported: &[eframe::egui_wgpu::wgpu::CompositeAlphaMode],
) -> Result<eframe::egui_wgpu::wgpu::CompositeAlphaMode, String> {
    use eframe::egui_wgpu::wgpu::CompositeAlphaMode;
    if supported.contains(&CompositeAlphaMode::PreMultiplied) {
        Ok(CompositeAlphaMode::PreMultiplied)
    } else {
        Err(format!(
            "Vulkan capsule surface lacks premultiplied transparency: {supported:?}"
        ))
    }
}

struct WgpuSurface {
    _instance: eframe::egui_wgpu::wgpu::Instance,
    surface: eframe::egui_wgpu::wgpu::Surface<'static>,
    device: eframe::egui_wgpu::wgpu::Device,
    queue: eframe::egui_wgpu::wgpu::Queue,
    config: eframe::egui_wgpu::wgpu::SurfaceConfiguration,
    renderer: eframe::egui_wgpu::Renderer,
}

impl WgpuSurface {
    fn new(
        connection: &Connection,
        wl_surface: &wl_surface::WlSurface,
        size: (u32, u32),
    ) -> Result<Self, String> {
        use eframe::egui_wgpu::wgpu;
        use raw_window_handle::{
            RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
        };

        let display_ptr = connection.backend().display_ptr().cast::<c_void>();
        let surface_ptr = wl_surface.id().as_ptr().cast::<c_void>();
        let raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(display_ptr).ok_or("null wayland display")?,
        ));
        let raw_window = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(surface_ptr).ok_or("null wayland surface")?,
        ));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        // The Wayland connection and wl_surface are owned by run_layer_capsule
        // and outlive this surface. RawHandle is therefore the correct unsafe
        // constructor: no legacy graphics context is created or retained.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display),
                raw_window_handle: raw_window,
            })
        }
        .map_err(|error| format!("vulkan surface: {error}"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("wgpu runtime: {error}"))?;
        let adapter = runtime
            .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .map_err(|error| format!("vulkan adapter: {error}"))?;
        let (device, queue) = runtime
            .block_on(adapter.request_device(&crate::wgpu_device::device_descriptor()))
            .map_err(|error| format!("vulkan device: {error}"))?;
        let mut config = surface
            .get_default_config(&adapter, size.0.max(1), size.1.max(1))
            .ok_or("Wayland surface has no Vulkan format")?;
        // Auto may select Opaque: clearing to transparent would then present
        // a black rectangle. Both egui and Siri output premultiplied alpha.
        config.alpha_mode = capsule_alpha_mode(&surface.get_capabilities(&adapter).alpha_modes)?;
        // Zero-delay animation requests are paced by the compositor, never an
        // immediate/mailbox swapchain followed by an unrelated fixed timer.
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        let format = config.format;
        let mut renderer = eframe::egui_wgpu::Renderer::new(
            &device,
            format,
            eframe::egui_wgpu::RendererOptions {
                msaa_samples: 1,
                ..Default::default()
            },
        );
        crate::siri_wgpu::install_renderer(&mut renderer, &device, format, 1);
        Ok(Self {
            _instance: instance,
            surface,
            device,
            queue,
            config,
            renderer,
        })
    }

    fn paint(
        &mut self,
        size: (u32, u32),
        primitives: &[egui::ClippedPrimitive],
        textures_delta: &mut egui::TexturesDelta,
        scale: f32,
    ) -> Result<bool, String> {
        let width = size.0.max(1);
        let height = size.1.max(1);
        if self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
        // Texture deltas are incremental (not re-sent next frame). Upload them
        // even if surface acquisition times out, so fonts/icons aren't lost.
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        let output = match self.surface.get_current_texture() {
            eframe::egui_wgpu::wgpu::CurrentSurfaceTexture::Success(texture)
            | eframe::egui_wgpu::wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            eframe::egui_wgpu::wgpu::CurrentSurfaceTexture::Timeout
            | eframe::egui_wgpu::wgpu::CurrentSurfaceTexture::Occluded => {
                for id in &textures_delta.free {
                    self.renderer.free_texture(id);
                }
                textures_delta.clear();
                return Ok(false);
            }
            other => return Err(format!("vulkan acquire: {other:?}")),
        };
        let view = output
            .texture
            .create_view(&eframe::egui_wgpu::wgpu::TextureViewDescriptor::default());
        let screen = eframe::egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: scale,
        };
        let mut encoder = self.device.create_command_encoder(
            &eframe::egui_wgpu::wgpu::CommandEncoderDescriptor {
                label: Some("openless-capsule-wgpu"),
            },
        );
        let callbacks = self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            primitives,
            &screen,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&eframe::egui_wgpu::wgpu::RenderPassDescriptor {
                    label: Some("openless-capsule"),
                    color_attachments: &[Some(
                        eframe::egui_wgpu::wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: eframe::egui_wgpu::wgpu::Operations {
                                load: eframe::egui_wgpu::wgpu::LoadOp::Clear(
                                    eframe::egui_wgpu::wgpu::Color::TRANSPARENT,
                                ),
                                store: eframe::egui_wgpu::wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        },
                    )],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, primitives, &screen);
        }
        self.queue.submit(callbacks);
        self.queue.submit(std::iter::once(encoder.finish()));
        // In wgpu 30 presentation belongs to the queue, not SurfaceTexture.
        self.queue.present(output);
        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }
        textures_delta.clear();
        Ok(true)
    }
}

/// Honor egui's earliest root-viewport request, and budget rendering/acquisition
/// inside the frame interval instead of adding a full sleep after presentation.
fn frame_pause(frame: &LayerFrame, spent: Duration, presented: bool) -> Duration {
    let requested = frame
        .output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map_or(frame.repaint_after, |root| {
            root.repaint_delay.min(frame.repaint_after)
        });
    let interval = if !presented && requested.is_zero() {
        // Occluded surfaces can return immediately without FIFO backpressure.
        // Keep pumping IPC/input, but do not spin a hidden animation at full CPU.
        Duration::from_millis(16)
    } else {
        requested
    };
    interval.min(MAX_FRAME_PAUSE).saturating_sub(spent)
}

// ── runner ──────────────────────────────────────────────────────────────────

/// Host the capsule on a `wlr-layer-shell` surface until `frame` asks to exit or
/// the compositor closes it.
///
/// Fails (with a human-readable reason) when there is no Wayland session, the
/// compositor lacks the protocol, the configure never arrives, or Vulkan/WGPU could not
/// be initialised — the caller then uses the X11 overlay instead. The renderer is
/// created here; the caller installs fonts/visuals on the first frame (egui's
/// built-in fonts are enough for the pill, but a host font setup should run
/// once).
/// Bind the registry objects this module needs from the list
/// `registry_queue_init` collected.
///
/// The global events are consumed during `registry_queue_init`, so a later
/// `roundtrip` into our own state never sees them: binding through the returned
/// `GlobalList` is the only way the compositor, layer-shell and seat objects
/// exist at all.
fn bind_globals(
    globals: &wayland_client::globals::GlobalList,
    qh: &QueueHandle<LayerState>,
) -> LayerState {
    let registry = globals.registry();
    let mut state = LayerState::default();
    for global in globals.contents().clone_list() {
        match global.interface.as_str() {
            "wl_compositor" => {
                state.compositor = Some(registry.bind(global.name, global.version.min(4), qh, ()));
            }
            // Layer shell is at version 4 in the widest-deployed compositors and
            // 5 in KWin 6.7; everything this module sets exists since version 1.
            LAYER_SHELL_GLOBAL => {
                state.layer_shell = Some(registry.bind(global.name, global.version.min(4), qh, ()));
            }
            "wl_seat" => {
                state.seat = Some(registry.bind(global.name, global.version.min(7), qh, ()));
            }
            _ => {}
        }
    }
    state
}

pub fn run_layer_capsule<F>(mut geometry: CapsuleGeometry, mut frame: F) -> Result<(), String>
where
    F: FnMut(&egui::Context, egui::RawInput, bool) -> LayerFrame,
{
    let connection = Connection::connect_to_env().map_err(|error| format!("wayland: {error}"))?;
    let (globals, mut queue) = registry_queue_init::<LayerState>(&connection)
        .map_err(|error| format!("wayland registry: {error}"))?;
    let qh = queue.handle();
    let mut state = bind_globals(&globals, &qh);
    // One roundtrip so the seat capabilities arrive and the pointer exists.
    queue
        .roundtrip(&mut state)
        .map_err(|error| format!("wayland roundtrip: {error}"))?;
    let compositor = state
        .compositor
        .clone()
        .ok_or("no wl_compositor on this compositor")?;
    let layer_shell = state
        .layer_shell
        .clone()
        .ok_or("compositor has no zwlr_layer_shell_v1")?;

    let wl_surface = compositor.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &wl_surface,
        None,
        // Stay below true overlay surfaces such as desktop panels.
        zwlr_layer_shell_v1::Layer::Top,
        LAYER_NAMESPACE.to_string(),
        &qh,
        (),
    );
    let (width, height) = geometry.buffer_size();
    layer_surface.set_size(width, height);
    let _ = (width, height);
    // Bottom only: a single horizontal anchor leaves the surface centred while
    // keeping the input region at the pill instead of the whole bottom strip.
    layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::Bottom);
    layer_surface.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    // Zero does not reserve extra space, but keeps placement inside the
    // compositor's usable area. -1 explicitly allows overlap with panels.
    layer_surface.set_exclusive_zone(0);
    layer_surface.set_margin(0, 0, geometry.bottom_gap, 0);
    wl_surface.commit();

    let deadline = Instant::now() + CONFIGURE_TIMEOUT;
    while state.configure.is_none() {
        if state.closed {
            return Err("layer surface closed before configure".to_string());
        }
        if Instant::now() >= deadline {
            return Err("layer surface configure timed out".to_string());
        }
        queue
            .blocking_dispatch(&mut state)
            .map_err(|error| format!("wayland dispatch: {error}"))?;
    }

    // Milestone lines for real-machine verification: they land on the popup
    // process' inherited stderr (terminal or journal), since the popup installs
    // no logger of its own.
    let (configured_width, configured_height) = state.configure.unwrap_or((width, height));
    eprintln!(
        "OpenLess capsule: layer surface configured {configured_width}x{configured_height} \
         (layer=top, anchor=bottom, margin.bottom={}, keyboard-interactivity=none, exclusive-zone=0)",
        geometry.bottom_gap
    );
    let mut renderer = WgpuSurface::new(&connection, &wl_surface, geometry.buffer_size())?;
    eprintln!("OpenLess capsule: Vulkan/WGPU ready on the layer surface");
    let context = egui::Context::default();
    // This layer surface is single-sample; feather egui geometry instead.
    context.tessellation_options_mut(|options| {
        options.feathering = true;
        options.feathering_size_in_pixels = 1.0;
    });
    let scale = 1.0;
    let mut first = true;
    let mut right_aligned = false;
    let animation_started = Instant::now();

    loop {
        let frame_started = Instant::now();
        // dispatch_pending alone never reads the display socket. Without this,
        // pointer events arriving after initial configure cannot reach egui.
        if let Some(reader) = queue.prepare_read() {
            let mut fd = libc::pollfd {
                fd: reader.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: fd is a valid, borrowed display descriptor and the pollfd
            // remains live for this non-blocking call. The reader owns the read.
            let ready = unsafe { libc::poll(&mut fd, 1, 0) };
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(format!("wayland input poll: {error}"));
                }
            } else if fd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                match reader.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(error))
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(format!("wayland input read: {error}")),
                }
            }
        }
        queue
            .dispatch_pending(&mut state)
            .map_err(|error| format!("wayland dispatch: {error}"))?;
        if state.closed {
            return Ok(());
        }
        let _ = connection.flush();
        let rect = geometry.rect_for_configure(state.configure.unwrap_or(geometry.buffer_size()));
        let mut input = state.take_input(rect);
        input.time = Some(animation_started.elapsed().as_secs_f64());
        let mut frame = frame(&context, input, first);
        first = false;
        let primitives = context.tessellate(frame.output.shapes.clone(), scale);
        let presented = renderer.paint(
            (rect.width().max(1.0) as u32, rect.height().max(1.0) as u32),
            &primitives,
            &mut frame.output.textures_delta,
            scale,
        )?;
        if frame.exit {
            return Ok(());
        }
        if let Some((next, align_right)) = frame.capsule_layout {
            if next != geometry || align_right != right_aligned {
                geometry = next;
                right_aligned = align_right;
                let (width, height) = geometry.buffer_size();
                layer_surface.set_size(width, height);
                layer_surface.set_anchor(if right_aligned {
                    zwlr_layer_surface_v1::Anchor::Bottom | zwlr_layer_surface_v1::Anchor::Right
                } else {
                    zwlr_layer_surface_v1::Anchor::Bottom
                });
                layer_surface.set_margin(
                    0,
                    if right_aligned { 24 } else { 0 },
                    geometry.bottom_gap,
                    0,
                );
                state.configure = None;
                wl_surface.commit();
                connection
                    .flush()
                    .map_err(|error| format!("wayland resize flush: {error}"))?;
            }
        }
        let pause = frame_pause(&frame, frame_started.elapsed(), presented);
        if !pause.is_zero() {
            std::thread::sleep(pause);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn globals(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn detects_the_layer_shell_global() {
        assert!(has_layer_shell(&globals(&[
            "wl_compositor",
            LAYER_SHELL_GLOBAL,
            "wl_seat",
        ])));
        assert!(!has_layer_shell(&globals(&["wl_compositor", "wl_seat"])));
        assert!(!has_layer_shell(&[]));
    }

    #[test]
    fn layer_shell_wins_over_the_x11_overlay() {
        let advertised = globals(&[LAYER_SHELL_GLOBAL]);
        assert_eq!(
            choose_capsule_path(Some("wayland-0"), Some(":0"), &advertised),
            CapsulePath::LayerShell
        );
        // Wayland without layer-shell must not use the XWayland display.
        assert_eq!(
            choose_capsule_path(Some("wayland-0"), Some(":0"), &globals(&["wl_compositor"])),
            CapsulePath::PlainWindow
        );
        // Wayland session without XWayland and without layer-shell.
        assert_eq!(
            choose_capsule_path(Some("wayland-0"), None, &globals(&["wl_compositor"])),
            CapsulePath::PlainWindow
        );
        // Pure X11 session.
        assert_eq!(
            choose_capsule_path(None, Some(":0"), &globals(&[LAYER_SHELL_GLOBAL])),
            CapsulePath::X11Overlay
        );
    }

    #[test]
    fn a_blank_display_variable_is_not_a_wayland_session() {
        assert!(!wayland_display_available(Some("")));
        assert!(!wayland_display_available(Some("   ")));
        assert!(!wayland_display_available(None));
        assert!(wayland_display_available(Some("wayland-0")));
    }

    /// 真机验证用：在一台真的连着合成器的机器上跑
    /// `cargo test -p openless-linux-egui -- --ignored --nocapture capsule_decision`
    /// 就能看到这台机器实际会走哪条路径。
    #[test]
    #[ignore = "requires a live Wayland / X11 session"]
    fn capsule_decision_on_this_machine() {
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let x11 = std::env::var("DISPLAY").ok();
        let globals = probe_globals();
        println!("WAYLAND_DISPLAY={wayland:?} DISPLAY={x11:?}");
        match &globals {
            Ok(globals) => println!(
                "globals with layer-shell: {}",
                globals
                    .iter()
                    .filter(|g| g.as_str() == LAYER_SHELL_GLOBAL)
                    .count()
            ),
            Err(error) => println!("globals unavailable: {error}"),
        }
        println!(
            "probe_layer_shell={} layer_shell_available={} detect_capsule_path={:?}",
            probe_layer_shell(wayland.as_deref()),
            layer_shell_available(),
            detect_capsule_path()
        );
    }

    #[test]
    fn capsule_path_override_parses_the_documented_values() {
        assert_eq!(
            capsule_path_override(Some("layer")),
            Some(CapsulePath::LayerShell)
        );
        assert_eq!(
            capsule_path_override(Some(" Layer-Shell ")),
            Some(CapsulePath::LayerShell)
        );
        assert_eq!(
            capsule_path_override(Some("x11")),
            Some(CapsulePath::X11Overlay)
        );
        assert_eq!(capsule_path_override(Some("XWayland")), None);
        assert_eq!(
            capsule_path_override(Some("plain")),
            Some(CapsulePath::PlainWindow)
        );
        // A typo must never take the capsule off every path.
        assert_eq!(capsule_path_override(Some("")), None);
        assert_eq!(capsule_path_override(Some("layerish")), None);
        assert_eq!(capsule_path_override(None), None);
    }

    /// The parent picks the child's backend from the same override the child
    /// uses to pick its window, so the two can never disagree: an override of
    /// `layer` is the only one that must keep the Wayland connection alive.
    #[test]
    fn the_override_keeps_the_parent_and_child_in_agreement() {
        for (value, layer_shell, path) in [
            ("layer", true, CapsulePath::LayerShell),
            ("x11", false, CapsulePath::X11Overlay),
            ("plain", false, CapsulePath::PlainWindow),
        ] {
            let forced = capsule_path_override(Some(value));
            assert_eq!(forced, Some(path), "override {value}");
            // What `layer_shell_available` returns for that override…
            assert_eq!(forced == Some(CapsulePath::LayerShell), layer_shell);
            // …and what the child derives from the same value.
            assert_eq!(forced, Some(path));
        }
    }

    #[test]
    fn capsule_requires_explicit_premultiplied_transparency() {
        use eframe::egui_wgpu::wgpu::CompositeAlphaMode as Alpha;
        assert_eq!(
            capsule_alpha_mode(&[Alpha::Opaque, Alpha::PreMultiplied]).unwrap(),
            Alpha::PreMultiplied,
        );
        for modes in [
            vec![],
            vec![Alpha::Opaque],
            vec![Alpha::Auto],
            vec![Alpha::PostMultiplied],
        ] {
            assert!(capsule_alpha_mode(&modes).is_err());
        }
    }

    #[test]
    fn capsule_geometry_clamps_to_a_paintable_surface() {
        let geometry = capsule_geometry(200, 100, 12);
        assert_eq!(geometry.buffer_size(), (200, 100));
        assert_eq!(geometry.bottom_gap, 12);
        let degenerate = capsule_geometry(0, 0, -5);
        assert_eq!(degenerate.buffer_size(), (1, 1));
        assert_eq!(degenerate.bottom_gap, 0);
        assert_eq!(degenerate.rect().size(), egui::vec2(1.0, 1.0));
    }

    #[test]
    fn a_zero_configure_keeps_the_requested_size() {
        let geometry = capsule_geometry(200, 100, 12);
        assert_eq!(
            geometry.rect_for_configure((0, 0)).size(),
            egui::vec2(200.0, 100.0)
        );
        // A compositor that resizes the surface wins once it reports pixels.
        assert_eq!(
            geometry.rect_for_configure((240, 120)).size(),
            egui::vec2(240.0, 120.0)
        );
    }

    fn pacing_frame(delay: Duration) -> LayerFrame {
        let mut output = egui::FullOutput::default();
        output.viewport_output.insert(
            egui::ViewportId::ROOT,
            egui::ViewportOutput {
                parent: egui::ViewportId::ROOT,
                class: egui::ViewportClass::Root,
                builder: egui::ViewportBuilder::default(),
                viewport_ui_cb: None,
                commands: Vec::new(),
                repaint_delay: delay,
            },
        );
        LayerFrame {
            output,
            exit: false,
            capsule_layout: None,
            repaint_after: Duration::from_millis(33),
        }
    }

    #[test]
    fn repaint_requests_win_and_render_time_is_not_added_twice() {
        let frame = pacing_frame(Duration::from_millis(16));
        assert_eq!(
            frame_pause(&frame, Duration::from_millis(5), true),
            Duration::from_millis(11)
        );
        assert_eq!(
            frame_pause(&frame, Duration::from_millis(20), true),
            Duration::ZERO
        );
        let immediate = pacing_frame(Duration::ZERO);
        assert_eq!(
            frame_pause(&immediate, Duration::from_millis(5), true),
            Duration::ZERO
        );
        let mut idle = pacing_frame(Duration::MAX);
        idle.repaint_after = Duration::from_secs(2);
        assert_eq!(
            frame_pause(&idle, Duration::from_millis(4), true),
            Duration::from_millis(96)
        );
    }

    #[test]
    fn an_occluded_immediate_animation_does_not_busy_spin() {
        let frame = pacing_frame(Duration::ZERO);
        assert_eq!(
            frame_pause(&frame, Duration::from_millis(3), false),
            Duration::from_millis(13)
        );
        assert_eq!(
            frame_pause(&frame, Duration::from_millis(20), false),
            Duration::ZERO
        );
    }

    #[test]
    fn pointer_samples_become_egui_events() {
        let position = Some(egui::pos2(4.0, 5.0));
        let events = pointer_events(
            position,
            &[(egui::pos2(4.0, 5.0), egui::PointerButton::Primary, true)],
            false,
        );
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], egui::Event::PointerMoved(_)));
        match &events[1] {
            egui::Event::PointerButton {
                button, pressed, ..
            } => {
                assert_eq!(*button, egui::PointerButton::Primary);
                assert!(pressed);
            }
            other => panic!("expected a button event, got {other:?}"),
        }
    }

    #[test]
    fn leaving_the_surface_reports_a_gone_pointer() {
        let events = pointer_events(None, &[], true);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], egui::Event::PointerGone));
    }

    #[test]
    fn mouse_buttons_map_to_egui_buttons() {
        assert_eq!(button_from_code(0x110), Some(egui::PointerButton::Primary));
        assert_eq!(
            button_from_code(0x111),
            Some(egui::PointerButton::Secondary)
        );
        assert_eq!(button_from_code(0x112), Some(egui::PointerButton::Middle));
        assert_eq!(button_from_code(0x113), None);
    }

    fn touch_button_frame(ctx: &egui::Context, state: &mut LayerState) -> bool {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0));
        let mut clicked = false;
        let mut output = ctx.run_ui(state.take_input(rect), |ui| {
            clicked |= ui
                .put(
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(80.0, 40.0)),
                    egui::Button::new("Stop"),
                )
                .clicked();
        });
        output.textures_delta.clear();
        clicked
    }

    #[test]
    fn a_native_touch_tap_clicks_once_and_drains_input() {
        let ctx = egui::Context::default();
        let mut state = LayerState::default();
        touch_button_frame(&ctx, &mut state);
        state.touch_input.down(7, egui::pos2(30.0, 25.0));
        state.touch_input.motion(7, egui::pos2(31.0, 25.0));
        state.touch_input.up(7);
        assert!(touch_button_frame(&ctx, &mut state));
        assert!(!touch_button_frame(&ctx, &mut state));
        assert!(state.touch_input.contacts.is_empty());
        assert!(state.touch_input.primary.is_none());
        assert!(state.touch_input.events.is_empty());
    }

    #[test]
    fn cancelling_or_adding_a_finger_cannot_click_or_leave_a_button_held() {
        let ctx = egui::Context::default();
        let mut state = LayerState::default();
        touch_button_frame(&ctx, &mut state);
        for multi in [false, true] {
            state.touch_input.down(1, egui::pos2(30.0, 25.0));
            assert!(!touch_button_frame(&ctx, &mut state));
            if multi {
                state.touch_input.down(2, egui::pos2(40.0, 25.0));
                state.touch_input.up(1);
                state.touch_input.up(2);
            } else {
                state.touch_input.cancel();
            }
            assert!(!touch_button_frame(&ctx, &mut state));
            assert!(!ctx.input(|input| input.pointer.any_down()));
            assert!(state.touch_input.contacts.is_empty());
            state.touch_input.down(3, egui::pos2(30.0, 25.0));
            state.touch_input.up(3);
            assert!(touch_button_frame(&ctx, &mut state));
        }
    }

    #[test]
    fn input_carries_the_viewport_and_pending_clicks() {
        let mut state = LayerState {
            pointer_position: Some(egui::pos2(1.0, 2.0)),
            pointer_moved: true,
            pressed: vec![(egui::pos2(1.0, 2.0), egui::PointerButton::Primary, true)],
            ..Default::default()
        };
        let rect = capsule_geometry(200, 100, 12).rect();
        let input = state.take_input(rect);
        assert_eq!(input.screen_rect, Some(rect));
        assert_eq!(input.events.len(), 2);
        // Drained: the next frame starts clean.
        let next = state.take_input(rect);
        assert!(next.events.is_empty());
    }
}
