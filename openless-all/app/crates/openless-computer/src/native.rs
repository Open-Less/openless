use std::io::Cursor;

use base64::{Engine, engine::general_purpose::STANDARD};
#[cfg(not(target_os = "windows"))]
use enigo::Coordinate;
use enigo::{Axis, Button, Direction, Enigo, Keyboard, Mouse, Settings};
use image::{DynamicImage, ImageFormat, RgbaImage, imageops::FilterType};
use serde_json::{Value, json};
use xcap::Monitor;

use crate::protocol::{Display, Error, MouseButton, Request, Result, ScrollAxis, parse_key};

fn session_error() -> Option<Error> {
    #[cfg(target_os = "linux")]
    {
        crate::protocol::linux_session_error(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
            std::env::var("DISPLAY").ok().as_deref(),
        )
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        None
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Some(Error::new(
            "unsupported_platform",
            "Supported platforms are Windows, macOS, and Linux X11",
        ))
    }
}

// These preflight calls neither request permissions nor capture screen content.
#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

fn permissions() -> Value {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: both framework functions take no arguments and are pure preflight checks.
        json!({
            "screen_recording": if unsafe { CGPreflightScreenCaptureAccess() } {"granted"} else {"required"},
            "accessibility": if unsafe { AXIsProcessTrusted() } {"granted"} else {"required"}
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        json!({"screen_recording":"not_checked", "accessibility":"not_checked"})
    }
}

fn capabilities() -> Value {
    let unavailable = session_error();
    json!({
        "protocol_version":1,
        "version":env!("CARGO_PKG_VERSION"),
        "platform":std::env::consts::OS,
        "architecture":std::env::consts::ARCH,
        "supported":unavailable.is_none(),
        "availability":"not_probed",
        "backend":{"capture":"xcap", "input":"enigo", "linux_session":"x11_only"},
        "actions":["capabilities","displays","screenshot","move","click","scroll","key","type_text"],
        "coordinate_space":"monitor-local-pixels",
        "permissions":permissions(),
        "unavailable_reason":unavailable,
        "notes":[
            "Preflight only: no screenshot, input event, permission dialog, or clipboard access.",
            "Actual availability is checked on each request inside the signed-in desktop session.",
            "Use screenshot coordinates and its monitor.id; macOS Retina screenshots use logical point resolution.",
            "Windows input cannot control elevated windows from a non-elevated process or the secure desktop."
        ]
    })
}

fn prepare_display() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
            SetThreadDpiAwarenessContext,
        };
        // Set the process first for xcap's DPI probe, and the current thread for
        // accurate cursor coordinates even if an inherited manifest set the process mode.
        // SAFETY: predefined awareness handles are accepted by these process/thread APIs.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            if previous.0.is_null() {
                return Err(Error::new(
                    "display_error",
                    "Cannot enable per-monitor DPI awareness",
                ));
            }
        }
    }
    Ok(())
}

fn capture_permission() -> Result<()> {
    #[cfg(target_os = "macos")]
    // SAFETY: no-argument permission preflight, no dialog or mutation.
    if !unsafe { CGPreflightScreenCaptureAccess() } {
        return Err(Error::new(
            "permission_denied",
            "Enable OpenLess / openless-computer in System Settings > Privacy & Security > Screen Recording, then restart OpenLess",
        ));
    }
    Ok(())
}

fn input_backend() -> Result<Enigo> {
    #[cfg(target_os = "macos")]
    // SAFETY: no-argument permission preflight, no dialog or mutation.
    if !unsafe { AXIsProcessTrusted() } {
        return Err(Error::new(
            "permission_denied",
            "Enable OpenLess / openless-computer in System Settings > Privacy & Security > Accessibility, then restart OpenLess",
        ));
    }
    Enigo::new(&Settings {
        open_prompt_to_get_permissions: false,
        release_keys_when_dropped: true,
        ..Settings::default()
    })
    .map_err(|error| {
        Error::new(
            "input_unavailable",
            format!("Cannot connect to native input backend: {error}"),
        )
    })
}

fn display_error(error: impl std::fmt::Display) -> Error {
    Error::new(
        "display_error",
        format!("Cannot read desktop displays: {error}"),
    )
}

fn input_error(error: impl std::fmt::Display) -> Error {
    Error::new("input_error", format!("Native input failed: {error}"))
}

// xcap reports X11 geometry divided by Xft.dpi and rounded down. Query RandR
// directly so negative origins, fractional scaling, and edge pixels stay exact.
#[cfg(target_os = "linux")]
fn native_bounds(monitor: &Monitor) -> Result<(i32, i32, u32, u32)> {
    use x11rb::{connection::Connection, protocol::randr::ConnectionExt};
    let id = monitor.id().map_err(display_error)?;
    let (connection, screen) = x11rb::connect(None).map_err(display_error)?;
    let root = connection.setup().roots[screen].root;
    let reply = connection
        .randr_get_monitors(root, true)
        .map_err(display_error)?
        .reply()
        .map_err(display_error)?;
    let info = reply
        .monitors
        .iter()
        .find(|info| info.outputs.contains(&id))
        .ok_or_else(|| {
            Error::new(
                "display_error",
                "Display disappeared while reading its X11 geometry",
            )
        })?;
    Ok((
        i32::from(info.x),
        i32::from(info.y),
        u32::from(info.width),
        u32::from(info.height),
    ))
}

#[cfg(not(target_os = "linux"))]
fn native_bounds(monitor: &Monitor) -> Result<(i32, i32, u32, u32)> {
    Ok((
        monitor.x().map_err(display_error)?,
        monitor.y().map_err(display_error)?,
        monitor.width().map_err(display_error)?,
        monitor.height().map_err(display_error)?,
    ))
}

fn describe(monitor: &Monitor) -> Result<Display> {
    let (x, y, width, height) = native_bounds(monitor)?;
    let scale_factor = monitor.scale_factor().unwrap_or(1.0);
    let display = Display {
        id: monitor.id().map_err(display_error)?,
        name: monitor.name().map_err(display_error)?,
        x,
        y,
        width,
        height,
        scale_factor: if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        },
        is_primary: monitor.is_primary().map_err(display_error)?,
    };
    display.validate_size()?;
    Ok(display)
}

fn displays() -> Result<Vec<(Monitor, Display)>> {
    prepare_display()?;
    let monitors = Monitor::all().map_err(display_error)?;
    if monitors.is_empty() {
        return Err(Error::new(
            "no_display",
            "No active desktop display is available",
        ));
    }
    monitors
        .into_iter()
        .map(|monitor| describe(&monitor).map(|info| (monitor, info)))
        .collect()
}

fn selected(displays: &[(Monitor, Display)], monitor_id: Option<u32>) -> Result<usize> {
    if let Some(id) = monitor_id {
        displays
            .iter()
            .position(|(_, info)| info.id == id)
            .ok_or_else(|| {
                Error::new(
                    "monitor_not_found",
                    "Selected display is not connected; request displays again",
                )
            })
    } else {
        Ok(displays
            .iter()
            .position(|(_, info)| info.is_primary)
            .unwrap_or(0))
    }
}

fn move_to(enigo: &mut Enigo, x: i32, y: i32) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
        let _ = enigo;
        // Enigo's absolute SendInput coordinates are relative to the primary
        // monitor. SetCursorPos handles the complete virtual desktop instead.
        // SAFETY: x/y have been bounded to a connected monitor in execute().
        unsafe { SetCursorPos(x, y) }.map_err(input_error)
    }
    #[cfg(not(target_os = "windows"))]
    {
        enigo.move_mouse(x, y, Coordinate::Abs).map_err(input_error)
    }
}

fn encode_capture(raw_image: RgbaImage, info: &Display) -> Result<String> {
    info.validate_size()?;
    if raw_image.width() == 0 || raw_image.height() == 0 {
        return Err(Error::new(
            "capture_error",
            "Screen capture returned an empty image",
        ));
    }
    let image = DynamicImage::ImageRgba8(raw_image);
    let image = if image.width() != info.width || image.height() != info.height {
        image.resize_exact(info.width, info.height, FilterType::Triangle)
    } else {
        image
    };
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| Error::new("capture_error", format!("PNG encoding failed: {error}")))?;
    // 32 MiB PNG becomes <43 MiB Base64, within PI's 48 MiB response limit.
    if png.get_ref().len() > 32 * 1024 * 1024 {
        return Err(Error::new(
            "capture_error",
            "Screenshot PNG exceeds 32 MiB; lower this display's resolution and retry",
        ));
    }
    Ok(STANDARD.encode(png.into_inner()))
}

pub fn execute(request: Request) -> Result<Value> {
    // Keep validation ahead of every permission check and native side effect.
    request.validate()?;
    if matches!(request, Request::Capabilities {}) {
        return Ok(capabilities());
    }
    if let Some(error) = session_error() {
        return Err(error);
    }
    match request {
        Request::Capabilities {} => unreachable!(),
        Request::Displays {} => {
            let displays = displays()?;
            Ok(
                json!({"coordinate_space":"monitor-local-pixels", "displays":displays.iter().map(|(_, info)| info).collect::<Vec<_>>()}),
            )
        }
        Request::Screenshot { monitor_id } => {
            capture_permission()?;
            let displays = displays()?;
            let (monitor, info) = &displays[selected(&displays, monitor_id)?];
            let raw_image = monitor.capture_image().map_err(|error| {
                Error::new("capture_error", format!("Screen capture failed: {error}"))
            })?;
            let source_width = raw_image.width();
            let source_height = raw_image.height();
            let image_base64 = encode_capture(raw_image, info)?;
            Ok(json!({
                "image_base64":image_base64,
                "mime_type":"image/png",
                "width":info.width, "height":info.height,
                "source_width":source_width, "source_height":source_height,
                "coordinate_space":"monitor-local-pixels",
                "monitor":info,
                "displays":displays.iter().map(|(_, info)| info).collect::<Vec<_>>()
            }))
        }
        Request::Move { monitor_id, x, y } => {
            let displays = displays()?;
            let (_, info) = &displays[selected(&displays, monitor_id)?];
            let (desktop_x, desktop_y) = info.desktop_point(x, y)?;
            let mut enigo = input_backend()?;
            move_to(&mut enigo, desktop_x, desktop_y)?;
            Ok(json!({"action":"move", "monitor_id":info.id, "x":x, "y":y}))
        }
        Request::Click {
            monitor_id,
            x,
            y,
            button,
            clicks,
        } => {
            let displays = displays()?;
            let (_, info) = &displays[selected(&displays, monitor_id)?];
            let (desktop_x, desktop_y) = info.desktop_point(x, y)?;
            let mut enigo = input_backend()?;
            move_to(&mut enigo, desktop_x, desktop_y)?;
            let button = match button {
                MouseButton::Left => Button::Left,
                MouseButton::Right => Button::Right,
                MouseButton::Middle => Button::Middle,
            };
            for index in 0..clicks {
                if index > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(80));
                }
                enigo
                    .button(button, Direction::Click)
                    .map_err(input_error)?;
            }
            Ok(json!({"action":"click", "monitor_id":info.id, "x":x, "y":y, "clicks":clicks}))
        }
        Request::Scroll { amount, axis } => {
            let mut enigo = input_backend()?;
            let native_axis = match axis {
                ScrollAxis::Vertical => Axis::Vertical,
                ScrollAxis::Horizontal => Axis::Horizontal,
            };
            enigo.scroll(amount, native_axis).map_err(input_error)?;
            Ok(json!({"action":"scroll", "amount":amount}))
        }
        Request::Key { key, modifiers } => {
            let key = parse_key(&key)?;
            let mut enigo = input_backend()?;
            let mut pressed = Vec::new();
            let operation = (|| {
                for modifier in &modifiers {
                    let key = modifier.key();
                    enigo.key(key, Direction::Press).map_err(input_error)?;
                    pressed.push(key);
                }
                enigo.key(key, Direction::Click).map_err(input_error)
            })();
            // Release in reverse order on success and failure; Enigo Drop retries
            // releasing any key whose release failed. No cross-request key state.
            let mut cleanup = Ok(());
            for key in pressed.iter().rev() {
                if let Err(error) = enigo.key(*key, Direction::Release) {
                    cleanup = Err(input_error(error));
                }
            }
            operation?;
            cleanup?;
            Ok(json!({"action":"key"}))
        }
        Request::TypeText { text } => {
            let mut enigo = input_backend()?;
            enigo.text(&text).map_err(input_error)?;
            Ok(json!({"action":"type_text", "characters":text.chars().count()}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_does_not_capture_or_connect_to_input() {
        let result = execute(Request::Capabilities {}).unwrap();
        assert_eq!(result["protocol_version"], 1);
        assert_eq!(result["availability"], "not_probed");
        assert_eq!(result["coordinate_space"], "monitor-local-pixels");
        assert!(
            result["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "screenshot")
        );
        assert!(result.get("image_base64").is_none());
    }

    #[test]
    fn invalid_native_requests_are_rejected_without_desktop_access() {
        let result = execute(Request::TypeText {
            text: String::new(),
        });
        assert_eq!(result.unwrap_err().code, "invalid_request");
    }

    #[test]
    fn retina_image_encodes_at_the_exact_coordinate_resolution() {
        let info = Display {
            id: 1,
            name: "synthetic".into(),
            x: 0,
            y: 0,
            width: 2,
            height: 1,
            scale_factor: 2.0,
            is_primary: true,
        };
        let image = RgbaImage::from_pixel(4, 2, image::Rgba([20, 40, 60, 255]));
        let png = STANDARD
            .decode(encode_capture(image, &info).unwrap())
            .unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::load_from_memory_with_format(&png, ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 1));
        assert_eq!(decoded.to_rgba8().get_pixel(1, 0).0, [20, 40, 60, 255]);
    }
}
