use enigo::Key;
use serde::{Deserialize, Serialize};

pub const MAX_REQUEST_BYTES: usize = 128 * 1024;
pub const MAX_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_SCREEN_PIXELS: u64 = 40_000_000;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Capabilities {},
    Displays {},
    Screenshot {
        monitor_id: Option<u32>,
    },
    Move {
        monitor_id: Option<u32>,
        x: i32,
        y: i32,
    },
    Click {
        monitor_id: Option<u32>,
        x: i32,
        y: i32,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "one_click")]
        clicks: u8,
    },
    Scroll {
        amount: i32,
        #[serde(default)]
        axis: ScrollAxis,
    },
    Key {
        key: String,
        #[serde(default)]
        modifiers: Vec<Modifier>,
    },
    TypeText {
        text: String,
    },
}

fn one_click() -> u8 {
    1
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollAxis {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

impl Modifier {
    pub fn key(self) -> Key {
        match self {
            Self::Ctrl => Key::Control,
            Self::Alt => Key::Alt,
            Self::Shift => Key::Shift,
            Self::Meta => Key::Meta,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }

    pub fn exit_code(&self) -> u8 {
        if self.code == "invalid_request" { 2 } else { 1 }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn parse_request(bytes: &[u8]) -> Result<Request> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Error::invalid("Request exceeds 128 KiB"));
    }
    let request: Request = serde_json::from_slice(bytes)
        .map_err(|error| Error::invalid(format!("Invalid request JSON: {error}")))?;
    request.validate()?;
    Ok(request)
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Move { x, y, .. } | Self::Click { x, y, .. } if *x < 0 || *y < 0 => {
                return Err(Error::invalid(
                    "x and y must be non-negative monitor-local pixels",
                ));
            }
            Self::Click { clicks, .. } if !matches!(clicks, 1 | 2) => {
                return Err(Error::invalid("clicks must be 1 or 2"));
            }
            Self::Scroll { amount, .. } if *amount == 0 || !(-100..=100).contains(amount) => {
                return Err(Error::invalid(
                    "scroll amount must be -100..100, excluding zero",
                ));
            }
            Self::TypeText { text } => {
                if text.is_empty() || text.len() > MAX_TEXT_BYTES || text.contains('\0') {
                    return Err(Error::invalid(
                        "text must contain 1..32768 UTF-8 bytes and no NUL",
                    ));
                }
            }
            Self::Key { key, modifiers } => {
                parse_key(key)?;
                if modifiers.len() > 4
                    || modifiers
                        .iter()
                        .enumerate()
                        .any(|(i, value)| modifiers[..i].contains(value))
                {
                    return Err(Error::invalid(
                        "modifiers must contain at most four distinct modifiers",
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn parse_key(name: &str) -> Result<Key> {
    // Preserve case for Unicode key characters. Named keys are case insensitive.
    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if !ch.is_control() {
            return Ok(Key::Unicode(ch));
        }
    }
    let key = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "page_up" => Key::PageUp,
        "pagedown" | "page_down" => Key::PageDown,
        "up" | "arrowup" => Key::UpArrow,
        "down" | "arrowdown" => Key::DownArrow,
        "left" | "arrowleft" => Key::LeftArrow,
        "right" | "arrowright" => Key::RightArrow,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => {
            return Err(Error::invalid(
                "Unsupported key; use a named navigation/function key or one Unicode character",
            ));
        }
    };
    Ok(key)
}

#[derive(Debug, Clone, Serialize)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub is_primary: bool,
}

impl Display {
    pub fn validate_size(&self) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || u64::from(self.width) * u64::from(self.height) > MAX_SCREEN_PIXELS
        {
            return Err(Error::new(
                "display_error",
                "Display size must contain 1..40000000 pixels",
            ));
        }
        Ok(())
    }

    pub fn desktop_point(&self, x: i32, y: i32) -> Result<(i32, i32)> {
        self.validate_size()?;
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return Err(Error::invalid(
                "Coordinates are outside the selected monitor; capture a fresh screenshot",
            ));
        }
        let desktop_x = self
            .x
            .checked_add(x)
            .ok_or_else(|| Error::invalid("x coordinate overflow"))?;
        let desktop_y = self
            .y
            .checked_add(y)
            .ok_or_else(|| Error::invalid("y coordinate overflow"))?;
        Ok((desktop_x, desktop_y))
    }
}

#[cfg(any(target_os = "linux", test))]
pub fn linux_session_error(
    session_type: Option<&str>,
    wayland: Option<&str>,
    display: Option<&str>,
) -> Option<Error> {
    if session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
        || wayland.is_some_and(|value| !value.is_empty())
    {
        Some(Error::new(
            "unsupported_session",
            "Wayland Computer control is not supported by this build; use a native X11 session",
        ))
    } else if !display.is_some_and(|value| !value.is_empty()) {
        Some(Error::new(
            "no_display",
            "No X11 DISPLAY is available; run inside the signed-in desktop session",
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_unknown_actions_fields_and_incomplete_moves() {
        for json in [
            r#"{"action":"shell","command":"whoami"}"#,
            r#"{"action":"capabilities","command":"whoami"}"#,
            r#"{"action":"move","x":1}"#,
            r#"{"action":"key","key":"a","direction":"press"}"#,
        ] {
            assert_eq!(
                parse_request(json.as_bytes()).unwrap_err().code,
                "invalid_request"
            );
        }
    }

    #[test]
    fn reject_invalid_input_before_creating_a_native_backend() {
        for json in [
            r#"{"action":"click","x":0,"y":0,"clicks":3}"#,
            r#"{"action":"move","x":-1,"y":0}"#,
            r#"{"action":"scroll","amount":101}"#,
            r#"{"action":"scroll","amount":0}"#,
            r#"{"action":"key","key":"enter","modifiers":["ctrl","ctrl"]}"#,
            r#"{"action":"type_text","text":"\u0000"}"#,
        ] {
            assert!(parse_request(json.as_bytes()).is_err(), "{json}");
        }
    }

    #[test]
    fn chinese_text_round_trips_without_shell_or_clipboard_escaping() {
        let text = "中文 hello 'quoted' $(`literal`)\n";
        let json = serde_json::json!({"action":"type_text", "text":text}).to_string();
        match parse_request(json.as_bytes()).unwrap() {
            Request::TypeText { text: parsed } => assert_eq!(parsed, text),
            _ => panic!("expected text request"),
        }
    }

    #[test]
    fn limits_are_measured_in_utf8_bytes_and_requests_reject_trailing_data() {
        let json =
            serde_json::json!({"action":"type_text", "text":"中".repeat(MAX_TEXT_BYTES / 3 + 1)})
                .to_string();
        assert!(parse_request(json.as_bytes()).is_err());
        assert!(parse_request(&vec![b' '; MAX_REQUEST_BYTES + 1]).is_err());
        assert!(parse_request(b"{\"action\":\"capabilities\"}\n{}").is_err());
        assert!(parse_request(&[0xff]).is_err());
    }

    #[test]
    fn coordinates_support_negative_desktop_origins_and_exclude_outer_edges() {
        let display = Display {
            id: 1,
            name: "left".into(),
            x: -1920,
            y: -200,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
            is_primary: false,
        };
        assert_eq!(display.desktop_point(10, 20).unwrap(), (-1910, -180));
        assert_eq!(display.desktop_point(1919, 1079).unwrap(), (-1, 879));
        assert!(display.desktop_point(1920, 0).is_err());
        assert!(display.desktop_point(0, 1080).is_err());
        assert!(display.desktop_point(-1, 0).is_err());
        let overflowing = Display {
            x: i32::MAX,
            ..display
        };
        assert!(overflowing.desktop_point(1, 0).is_err());
    }

    #[test]
    fn named_keys_and_unicode_preserve_case() {
        assert_eq!(parse_key("RETURN").unwrap(), Key::Return);
        assert_eq!(parse_key("A").unwrap(), Key::Unicode('A'));
        assert_eq!(parse_key("中").unwrap(), Key::Unicode('中'));
        assert!(parse_key("ctrl+c").is_err());
        assert!(parse_key("\n").is_err());
    }

    #[test]
    fn wayland_is_rejected_even_when_xwayland_sets_display() {
        assert!(linux_session_error(Some("x11"), None, Some(":0")).is_none());
        assert_eq!(
            linux_session_error(Some("wayland"), None, Some(":0"))
                .unwrap()
                .code,
            "unsupported_session"
        );
        assert_eq!(
            linux_session_error(None, Some("wayland-0"), Some(":0"))
                .unwrap()
                .code,
            "unsupported_session"
        );
        assert_eq!(
            linux_session_error(None, None, None).unwrap().code,
            "no_display"
        );
    }
}
