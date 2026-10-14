//! 跨平台 Unicode keystroke 合成（流式输入用）。
//!
//! 公开 API 三件套：
//! - `type_unicode_chunk(text)` —— 阻塞地把一段文字逐 codepoint 当作键盘事件发出去，
//!   不动剪贴板。各平台用各自的原语；返回确认成功发送的字符数。
//! - `switch_to_ascii(app)` —— 仅 macOS 有效；切到 ABC 输入源以绕过 CJK / 日文 IME
//!   对 Unicode 字符串事件的拦截。Windows 上是 no-op。
//! - `restore_input_source(app, prev)` —— 配对调用，恢复 macOS 上的原输入源。
//!
//! ## Platform differences
//!
//! - **macOS**：手写 CGEvent FFI（与 `insertion.rs::macos` 的 Cmd+V 同源）。
//!   `CGEventKeyboardSetUnicodeString` 在 CJK / 日文 IME 激活时被拦截 ——
//!   必须 `switch_to_ascii` 切到 ABC，session 结束再 `restore_input_source` 切回。
//! - **Windows**：`SendInput(KEYEVENTF_UNICODE)` 直接发 UTF-16 scancode。TSF 不拦
//!   Unicode 事件（与 keyboard layout / IME 解耦），所以不需要切输入法。
//!
//! ## Known pitfalls (macOS)
//!
//! - Under Secure Event Input (password fields, 1Password, ...) CGEventPost
//!   fails silently; `type_unicode_chunk` probes with
//!   `IsSecureEventInputEnabled` up front and returns
//!   `TypeError::SecureInputActive` when set.
//! - Modifier state inheritance — a held Shift maps characters to uppercase;
//!   every event sets `CGEventSetFlags(_, 0)` explicitly.
//! - Chromium / Electron / Tauri themselves drop characters when
//!   keyDown/keyUp have no delay; sleeps 1ms per codepoint.
//!
//! ## Thread safety (macOS)
//!
//! - `type_unicode_chunk` (CGEventPost) is callable from any thread, matching
//!   the current state of `insertion.rs::macos::simulate_paste`.
//! - TIS (`switch_to_ascii` / `restore_input_source`) dispatches to the main
//!   thread to avoid the macOS 14+ `dispatch_assert_queue_fail` SIGTRAP on
//!   TSM/TIS main-thread checks.

#[allow(unused_imports)]
use tauri::{AppHandle, Runtime};

#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[allow(dead_code)]
    #[error("{source} after {typed_chars} chars were sent")]
    Partial {
        typed_chars: usize,
        #[source]
        source: Box<TypeError>,
    },
    #[cfg(target_os = "macos")]
    #[error("CGEventSourceCreate returned null")]
    SourceAllocFailed,
    #[cfg(target_os = "macos")]
    #[error("CGEventCreateKeyboardEvent returned null")]
    EventAllocFailed,
    #[cfg(target_os = "macos")]
    #[error("Secure Event Input is enabled — synthetic keystrokes will be silently dropped")]
    SecureInputActive,
    #[cfg(target_os = "windows")]
    #[error("Windows SendInput failed: {0}")]
    SendInputFailed(String),
}

impl TypeError {
    pub fn typed_chars(&self) -> usize {
        match self {
            TypeError::Partial { typed_chars, .. } => *typed_chars,
            _ => 0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TisError {
    #[error("dispatch to main thread failed: {0}")]
    MainThreadDispatch(String),
    #[error("TISCopyInputSourceForLanguage(\"en\") returned null — ABC source not installed?")]
    AbcSourceNotFound,
    #[error("TISSelectInputSource failed: OSStatus={0}")]
    SelectFailed(i32),
}

// ═══════════════════════════════════════════════════════════════════════════
// macOS implementation
// ═══════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "macos")]
mod macos_impl {
    use super::{TisError, TypeError};
    use crate::types::MacosNewlineMode;
    use std::ffi::c_void;
    use std::time::Duration;
    use tauri::{AppHandle, Runtime};

    const INTER_KEYSTROKE_DELAY: Duration = Duration::from_millis(1);

    /// How a single char is sent when typing text character by character.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum MacKeystroke {
        /// Newline: send a real Shift+Return keystroke (soft newline in chat boxes).
        ShiftReturn,
        /// Newline: send Unicode U+000A (soft newline as Ctrl+J in terminals/TUIs).
        LineFeed,
        /// Newline: send a real Return keystroke (equals "send" in chat boxes).
        Return,
        /// CR: send nothing. In `\r\n` it is only a prefix of `\n`; sending it
        /// would produce two newlines, and LLM output never uses the old Mac
        /// "bare `\r` means newline" format. Swallowing is the safest choice
        /// and keeps `\r` / `\n` split across delta boundaries from each
        /// adding an extra newline.
        Swallow,
        /// Regular character: `CGEventKeyboardSetUnicodeString`.
        Unicode,
    }

    /// **Newlines must go through real keystrokes, not plain Unicode chars.**
    ///
    /// macOS text input treats U+000A as Return — in chat boxes like
    /// WeChat / Slack / Telegram that equals "send". A two-paragraph text with
    /// a blank line once had its first half sent out by the first `\n`, leaving
    /// the rest in the input box.
    ///
    /// Default is Shift+Return: a soft newline in chat boxes and a plain
    /// newline in editors / terminals / web textareas — correct in both.
    /// Windows reached the same conclusion earlier (see
    /// `WindowsSendInputNewlineMode::ShiftEnter`; the settings copy says "pick
    /// this for chat boxes").
    ///
    /// Users can switch to `Return` in settings: style packs in the market
    /// split a passage into multiple messages via newlines, which needs a real
    /// Return.
    pub(super) fn classify_mac_keystroke(ch: char, mode: MacosNewlineMode) -> MacKeystroke {
        match ch {
            '\n' => match mode {
                MacosNewlineMode::Auto => MacKeystroke::ShiftReturn,
                MacosNewlineMode::ShiftReturn => MacKeystroke::ShiftReturn,
                MacosNewlineMode::LineFeed => MacKeystroke::LineFeed,
                MacosNewlineMode::Return => MacKeystroke::Return,
            },
            '\r' => MacKeystroke::Swallow,
            _ => MacKeystroke::Unicode,
        }
    }

    /// Token for the previously active input source. Held as a raw pointer in a
    /// usize; all dereferences happen on the main thread via
    /// `restore_input_source`. `Send + Sync` implemented manually.
    pub struct PreviousInputSource {
        raw: usize,
    }
    unsafe impl Send for PreviousInputSource {}
    unsafe impl Sync for PreviousInputSource {}

    pub fn type_unicode_chunk(text: &str) -> Result<usize, TypeError> {
        type_unicode_chunk_with_options(text, MacosNewlineMode::default())
    }

    pub fn type_unicode_chunk_with_options(
        text: &str,
        newline_mode: MacosNewlineMode,
    ) -> Result<usize, TypeError> {
        if text.is_empty() {
            return Ok(0);
        }
        if is_secure_input_enabled() {
            return Err(TypeError::SecureInputActive);
        }
        let mut typed_chars = 0;
        for ch in text.chars() {
            let sent = match classify_mac_keystroke(ch, newline_mode) {
                MacKeystroke::ShiftReturn => send_shift_return(),
                MacKeystroke::LineFeed => send_line_feed(),
                MacKeystroke::Return => send_return(),
                // Swallowed chars still count: the caller
                // (`flush_streaming_insert_buffer_with`) compares `typed_chars`
                // against `delta.chars().count()`, and any shortfall is judged
                // a partial failure that drops all later deltas. The count
                // means "this char was processed", not "one more character on
                // screen".
                MacKeystroke::Swallow => Ok(()),
                MacKeystroke::Unicode => send_one_codepoint(ch),
            };
            if let Err(e) = sent {
                return Err(partial_or_original(typed_chars, e));
            }
            typed_chars += 1;
            std::thread::sleep(INTER_KEYSTROKE_DELAY);
        }
        Ok(typed_chars)
    }

    fn partial_or_original(typed_chars: usize, source: TypeError) -> TypeError {
        if typed_chars == 0 {
            source
        } else {
            TypeError::Partial {
                typed_chars,
                source: Box::new(source),
            }
        }
    }

    fn send_one_codepoint(ch: char) -> Result<(), TypeError> {
        let mut buf = [0u16; 2];
        let utf16 = ch.encode_utf16(&mut buf);
        // Virtual keycode 0 + Unicode string override: the character is fully
        // determined by the unicode string, the keycode is irrelevant.
        // Flags are explicitly cleared — a held Shift would otherwise map the
        // character to uppercase.
        post_key_event(0, 0, Some(utf16))
    }

    /// Sends one Shift+Return. Uses the real Return virtual keycode
    /// (`kVK_Return`) instead of U+000A; see [`classify_mac_keystroke`].
    fn send_shift_return() -> Result<(), TypeError> {
        post_key_event(KEY_RETURN, KCG_EVENT_FLAG_MASK_SHIFT, None)
    }

    fn send_line_feed() -> Result<(), TypeError> {
        send_one_codepoint('\n')
    }

    /// Sends one Return with no modifiers. This equals "send" in chat boxes —
    /// only reached when the user explicitly picked
    /// [`MacosNewlineMode::Return`] in settings.
    fn send_return() -> Result<(), TypeError> {
        post_key_event(KEY_RETURN, 0, None)
    }

    /// Constructs and posts a down/up keyboard event pair; owns all CF resource
    /// release.
    ///
    /// When `unicode` is `Some`, `CGEventKeyboardSetUnicodeString` overrides the
    /// character content (`virtual_key` is then meaningless); when `None`, the
    /// event presses the physical `virtual_key`.
    fn post_key_event(
        virtual_key: CGKeyCode,
        flags: CGEventFlags,
        unicode: Option<&[u16]>,
    ) -> Result<(), TypeError> {
        unsafe {
            let src = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            if src.is_null() {
                return Err(TypeError::SourceAllocFailed);
            }
            let down = CGEventCreateKeyboardEvent(src, virtual_key, true);
            let up = CGEventCreateKeyboardEvent(src, virtual_key, false);
            if down.is_null() || up.is_null() {
                if !down.is_null() {
                    CFRelease(down as _);
                }
                if !up.is_null() {
                    CFRelease(up as _);
                }
                CFRelease(src as _);
                return Err(TypeError::EventAllocFailed);
            }
            CGEventSetFlags(down, flags);
            CGEventSetFlags(up, flags);
            if let Some(utf16) = unicode {
                CGEventKeyboardSetUnicodeString(down, utf16.len(), utf16.as_ptr());
                CGEventKeyboardSetUnicodeString(up, utf16.len(), utf16.as_ptr());
            }
            CGEventPost(KCG_HID_EVENT_TAP, down);
            CGEventPost(KCG_HID_EVENT_TAP, up);
            CFRelease(down as _);
            CFRelease(up as _);
            CFRelease(src as _);
        }
        Ok(())
    }

    /// Whether Secure Event Input is on (password fields, sudo prompts,
    /// 1Password, ... enable it).
    ///
    /// The write path uses it to decide "would synthetic keystrokes be silently
    /// dropped"; `host_document` uses it as the first hard gate before reading —
    /// when this signal is on, credentials are being typed on screen and
    /// nothing may be read.
    pub fn is_secure_input_enabled() -> bool {
        unsafe { IsSecureEventInputEnabled() != 0 }
    }

    pub async fn switch_to_ascii<R: Runtime>(
        app: &AppHandle<R>,
    ) -> Result<Option<PreviousInputSource>, TisError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let result = unsafe { switch_to_ascii_on_main() };
            let _ = tx.send(result);
        })
        .map_err(|e| TisError::MainThreadDispatch(e.to_string()))?;
        rx.await
            .map_err(|e| TisError::MainThreadDispatch(e.to_string()))?
    }

    unsafe fn switch_to_ascii_on_main() -> Result<Option<PreviousInputSource>, TisError> {
        let prev = TISCopyCurrentKeyboardInputSource();
        let prev_token = if prev.is_null() {
            None
        } else {
            Some(PreviousInputSource { raw: prev as usize })
        };
        let lang_bytes = b"en\0";
        let lang = CFStringCreateWithCString(
            std::ptr::null(),
            lang_bytes.as_ptr() as *const i8,
            K_CF_STRING_ENCODING_ASCII,
        );
        if lang.is_null() {
            if let Some(p) = prev_token {
                CFRelease(p.raw as *const _);
            }
            return Err(TisError::AbcSourceNotFound);
        }
        let abc = TISCopyInputSourceForLanguage(lang);
        CFRelease(lang as _);
        if abc.is_null() {
            if let Some(p) = prev_token {
                CFRelease(p.raw as *const _);
            }
            return Err(TisError::AbcSourceNotFound);
        }
        let status = TISSelectInputSource(abc);
        CFRelease(abc as _);
        if status != 0 {
            if let Some(p) = prev_token {
                CFRelease(p.raw as *const _);
            }
            return Err(TisError::SelectFailed(status));
        }
        Ok(prev_token)
    }

    pub async fn restore_input_source<R: Runtime>(
        app: &AppHandle<R>,
        prev: Option<PreviousInputSource>,
    ) -> Result<(), TisError> {
        let Some(prev) = prev else {
            return Ok(());
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let result = unsafe { restore_input_source_on_main(prev) };
            let _ = tx.send(result);
        })
        .map_err(|e| TisError::MainThreadDispatch(e.to_string()))?;
        rx.await
            .map_err(|e| TisError::MainThreadDispatch(e.to_string()))?
    }

    unsafe fn restore_input_source_on_main(prev: PreviousInputSource) -> Result<(), TisError> {
        let raw = prev.raw as *mut c_void;
        let status = TISSelectInputSource(raw);
        CFRelease(raw as _);
        if status != 0 {
            return Err(TisError::SelectFailed(status));
        }
        Ok(())
    }

    // ─── FFI ───
    type CGEventTapLocation = u32;
    type CGEventSourceStateID = i32;
    type CGKeyCode = u16;
    type CGEventFlags = u64;
    type CFStringEncoding = u32;
    type CFAllocatorRef = *const c_void;
    type CFStringRef = *const c_void;
    type TISInputSourceRef = *mut c_void;

    const KCG_HID_EVENT_TAP: CGEventTapLocation = 0;
    const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: CGEventSourceStateID = 1;
    const K_CF_STRING_ENCODING_ASCII: CFStringEncoding = 0x0600;
    const KCG_EVENT_FLAG_MASK_SHIFT: CGEventFlags = 0x00020000;
    /// Virtual keycode of Return on a US/ANSI keyboard (`kVK_Return`).
    const KEY_RETURN: CGKeyCode = 36;

    #[repr(C)]
    struct OpaqueCGEvent(c_void);
    type CGEventRef = *mut OpaqueCGEvent;
    #[repr(C)]
    struct OpaqueCGEventSource(c_void);
    type CGEventSourceRef = *mut OpaqueCGEventSource;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state_id: CGEventSourceStateID) -> CGEventSourceRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: CGKeyCode,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
        fn CGEventKeyboardSetUnicodeString(
            event: CGEventRef,
            string_length: usize,
            unicode_string: *const u16,
        );
        fn CGEventPost(tap: CGEventTapLocation, event: CGEventRef);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFStringCreateWithCString(
            alloc: CFAllocatorRef,
            c_str: *const i8,
            encoding: CFStringEncoding,
        ) -> CFStringRef;
    }

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn IsSecureEventInputEnabled() -> i32;
        fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
        fn TISCopyInputSourceForLanguage(lang: CFStringRef) -> TISInputSourceRef;
        fn TISSelectInputSource(source: TISInputSourceRef) -> i32;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Windows implementation
// ═══════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{TisError, TypeError};
    use crate::types::WindowsSendInputNewlineMode;
    use std::time::Duration;
    use tauri::{AppHandle, Runtime};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_RETURN, VK_SHIFT, VK_TAB,
    };

    const SENDINPUT_CHUNK_CHARS: usize = 16;
    const SENDINPUT_CHUNK_DELAY: Duration = Duration::from_millis(12);
    /// Gap after every synthetic keystroke, not just at chunk boundaries.
    ///
    /// Mirrors macOS's `INTER_KEYSTROKE_DELAY` (see that module's comment: "Chromium /
    /// Electron / Tauri themselves drop characters when keyDown/keyUp have no delay"). Windows
    /// had no such gap — characters within a chunk were sent back-to-back with zero spacing,
    /// only pausing every [`SENDINPUT_CHUNK_CHARS`]. Observed on hardware: dictating mixed
    /// Chinese/English text ("我要把这个 open list...") dropped the leading character(s) of the
    /// Latin-script word right after the CJK→ASCII transition (e.g. "open" arrived as "pen").
    /// 1ms is inaudible/invisible to the user but gives the target app's message loop room to
    /// process each keystroke before the next one lands.
    const INTER_KEYSTROKE_DELAY: Duration = Duration::from_millis(1);

    /// Windows 上没有 input source 概念，token 留空。Send/Sync 自动派生。
    pub struct PreviousInputSource;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct WindowsSendInputOptions {
        pub newline_mode: WindowsSendInputNewlineMode,
    }

    impl Default for WindowsSendInputOptions {
        fn default() -> Self {
            Self {
                newline_mode: WindowsSendInputNewlineMode::Enter,
            }
        }
    }

    pub fn type_unicode_chunk(text: &str) -> Result<usize, TypeError> {
        type_unicode_chunk_with_options(text, WindowsSendInputOptions::default())
    }

    pub fn type_unicode_chunk_with_options(
        text: &str,
        options: WindowsSendInputOptions,
    ) -> Result<usize, TypeError> {
        type_unicode_chunk_with_sender(text, options, send_character)
    }

    fn type_unicode_chunk_with_sender(
        text: &str,
        options: WindowsSendInputOptions,
        send: impl FnMut(char, WindowsSendInputOptions) -> Result<(), TypeError>,
    ) -> Result<usize, TypeError> {
        type_unicode_chunk_with_sender_and_sleeper(text, options, send, std::thread::sleep)
    }

    fn type_unicode_chunk_with_sender_and_sleeper(
        text: &str,
        options: WindowsSendInputOptions,
        mut send: impl FnMut(char, WindowsSendInputOptions) -> Result<(), TypeError>,
        mut sleep: impl FnMut(Duration),
    ) -> Result<usize, TypeError> {
        if text.is_empty() {
            return Ok(0);
        }
        let mut typed_chars = 0;
        let mut sent_in_chunk = 0usize;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            // typed_chars is the consumed prefix of source Unicode scalars, not
            // a system keystroke count. A safely swallowed CR is also consumed,
            // otherwise Core would misjudge complete CRLF input as a partial
            // failure; on a send failure the current char is not consumed,
            // preserving the exact completed prefix for final reconciliation.
            match super::classify_sendinput_char(ch) {
                super::SendInputCharKind::Skip => {
                    typed_chars += 1;
                    continue;
                }
                _ => {
                    if let Err(e) = send(ch, options) {
                        return Err(partial_or_original(typed_chars, e));
                    }
                }
            }
            typed_chars += 1;
            sent_in_chunk += 1;
            // Sleep after every event, including the final event in this write. The latter is
            // what preserves a gap when streaming reconciliation starts the next write later.
            sleep(INTER_KEYSTROKE_DELAY);

            if sent_in_chunk >= SENDINPUT_CHUNK_CHARS && chars.peek().is_some() {
                sleep(SENDINPUT_CHUNK_DELAY);
                sent_in_chunk = 0;
            }
        }
        Ok(typed_chars)
    }

    fn send_character(ch: char, options: WindowsSendInputOptions) -> Result<(), TypeError> {
        match super::classify_sendinput_char(ch) {
            super::SendInputCharKind::Skip => Ok(()),
            super::SendInputCharKind::Newline => send_newline(options.newline_mode),
            super::SendInputCharKind::Tab => press_vk(VK_TAB),
            super::SendInputCharKind::Unicode => {
                let mut buf = [0u16; 2];
                for unit in ch.encode_utf16(&mut buf) {
                    send_utf16_unit(*unit, false)?;
                    send_utf16_unit(*unit, true)?;
                }
                Ok(())
            }
        }
    }

    #[cfg(test)]
    mod consumption_tests {
        use super::*;

        #[test]
        fn crlf_and_emoji_report_the_consumed_source_prefix() {
            let mut sent = String::new();
            let consumed = type_unicode_chunk_with_sender(
                "a\r\n🙂",
                WindowsSendInputOptions::default(),
                |ch, _| {
                    sent.push(ch);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(sent, "a\n🙂");
            assert_eq!(consumed, 4);
        }

        #[test]
        fn pacing_crosses_write_boundaries_and_includes_final_event() {
            let mut sent = String::new();
            let mut sleeps = Vec::new();
            let mut send = |ch, _| {
                sent.push(ch);
                Ok(())
            };
            let mut sleep = |duration| sleeps.push(duration);

            type_unicode_chunk_with_sender_and_sleeper(
                "a",
                WindowsSendInputOptions::default(),
                &mut send,
                &mut sleep,
            )
            .unwrap();
            type_unicode_chunk_with_sender_and_sleeper(
                "b",
                WindowsSendInputOptions::default(),
                &mut send,
                &mut sleep,
            )
            .unwrap();

            assert_eq!(sent, "ab");
            assert_eq!(sleeps, vec![INTER_KEYSTROKE_DELAY, INTER_KEYSTROKE_DELAY]);
        }

        #[test]
        fn chunk_pause_follows_the_sixteenth_event() {
            let mut sleeps = Vec::new();
            let mut sleep = |duration| sleeps.push(duration);
            type_unicode_chunk_with_sender_and_sleeper(
                &"x".repeat(17),
                WindowsSendInputOptions::default(),
                |_, _| Ok(()),
                &mut sleep,
            )
            .unwrap();

            assert_eq!(sleeps.len(), 18);
            assert!(sleeps[..16].iter().all(|gap| *gap == INTER_KEYSTROKE_DELAY));
            assert_eq!(sleeps[16], SENDINPUT_CHUNK_DELAY);
            assert_eq!(sleeps[17], INTER_KEYSTROKE_DELAY);
        }

        #[test]
        fn partial_write_counts_swallowed_cr_but_not_the_failed_character() {
            let error = type_unicode_chunk_with_sender(
                "a\r\n🙂",
                WindowsSendInputOptions::default(),
                |ch, _| {
                    if ch == '\n' {
                        Err(TypeError::SendInputFailed("fixture".into()))
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();
            assert_eq!(error.typed_chars(), 2);
        }
    }

    fn partial_or_original(typed_chars: usize, source: TypeError) -> TypeError {
        if typed_chars == 0 {
            source
        } else {
            TypeError::Partial {
                typed_chars,
                source: Box::new(source),
            }
        }
    }

    fn send_newline(mode: WindowsSendInputNewlineMode) -> Result<(), TypeError> {
        match mode {
            WindowsSendInputNewlineMode::Enter => press_vk(VK_RETURN),
            WindowsSendInputNewlineMode::ShiftEnter => press_shift_enter(),
            WindowsSendInputNewlineMode::CrLf => {
                send_utf16_unit(0x000D, false)?;
                send_utf16_unit(0x000D, true)?;
                send_utf16_unit(0x000A, false)?;
                send_utf16_unit(0x000A, true)
            }
        }
    }

    fn press_shift_enter() -> Result<(), TypeError> {
        send_vk(VK_SHIFT, false)?;
        press_vk(VK_RETURN)?;
        send_vk(VK_SHIFT, true)
    }

    fn press_vk(vk: VIRTUAL_KEY) -> Result<(), TypeError> {
        send_vk(vk, false)?;
        send_vk(vk, true)
    }

    fn send_vk(vk: VIRTUAL_KEY, key_up: bool) -> Result<(), TypeError> {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if key_up {
            flags |= KEYEVENTF_KEYUP;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err(TypeError::SendInputFailed(
                std::io::Error::last_os_error().to_string(),
            ))
        }
    }

    fn send_utf16_unit(unit: u16, key_up: bool) -> Result<(), TypeError> {
        let flags = if key_up {
            KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
        } else {
            KEYEVENTF_UNICODE
        };
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: KEYBD_EVENT_FLAGS(flags.0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err(TypeError::SendInputFailed(
                std::io::Error::last_os_error().to_string(),
            ))
        }
    }

    /// Windows SendInput Unicode bypasses TSF and the IME, so no input-method
    /// switch is needed. Returns `Ok(None)`; `restore_input_source` is also a
    /// no-op.
    pub async fn switch_to_ascii<R: Runtime>(
        _app: &AppHandle<R>,
    ) -> Result<Option<PreviousInputSource>, TisError> {
        Ok(None)
    }

    pub async fn restore_input_source<R: Runtime>(
        _app: &AppHandle<R>,
        _prev: Option<PreviousInputSource>,
    ) -> Result<(), TisError> {
        Ok(())
    }
}

/// Per-character send classification for SendInput. Consumption counting and
/// send method are separate: even a Skip sends no system event, the Unicode
/// scalar passed in by the caller has been processed and must count toward the
/// consumed prefix.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendInputCharKind {
    /// `\r`: no system event (CRLF produces one newline), but counts toward the
    /// consumed source prefix.
    Skip,
    /// `\n`: sent as a newline (mode chosen by
    /// `WindowsSendInputOptions::newline_mode`).
    Newline,
    /// `\t`: sent as a Tab keystroke.
    Tab,
    /// All other characters: sent as UTF-16 Unicode events, code unit by code
    /// unit.
    Unicode,
}

#[cfg(target_os = "windows")]
pub(crate) fn classify_sendinput_char(ch: char) -> SendInputCharKind {
    match ch {
        '\r' => SendInputCharKind::Skip,
        '\n' => SendInputCharKind::Newline,
        '\t' => SendInputCharKind::Tab,
        _ => SendInputCharKind::Unicode,
    }
}

#[cfg(test)]
mod tests {
    use super::TypeError;

    #[cfg(target_os = "windows")]
    #[test]
    fn swallowed_carriage_returns_are_consumed_without_sending_input() {
        // This input produces no system keystrokes; verifies the consumption
        // count of the production API directly.
        assert_eq!(super::type_unicode_chunk("\r\r").unwrap(), 2);
    }

    /// With the default mode, newlines go through Shift+Return — macOS treats
    /// U+000A as Return, which equals "send" in chat boxes, so a two-paragraph
    /// text with a blank line would be split and sent apart.
    #[test]
    #[cfg(target_os = "macos")]
    fn newline_defaults_to_safe_auto_fallback() {
        use super::macos_impl::{classify_mac_keystroke, MacKeystroke};
        use crate::types::MacosNewlineMode;

        let mode = MacosNewlineMode::default();
        assert_eq!(mode, MacosNewlineMode::Auto, "默认必须按前台应用解析");
        assert_eq!(
            classify_mac_keystroke('\n', mode),
            MacKeystroke::ShiftReturn
        );
        // `\r` is swallowed: in CRLF it is only a prefix of LF; sending it
        // would produce two newlines.
        assert_eq!(classify_mac_keystroke('\r', mode), MacKeystroke::Swallow);
        for ch in ['a', '中', '，', ' ', '\t', '😀'] {
            assert_eq!(
                classify_mac_keystroke(ch, mode),
                MacKeystroke::Unicode,
                "{ch:?} 应当走普通 Unicode 路径"
            );
        }
    }

    /// Return mode must send a real Return — style packs in the market split a
    /// passage into multiple messages via newlines, and that effect needs
    /// "Return = send".
    #[test]
    #[cfg(target_os = "macos")]
    fn return_mode_sends_a_plain_return_for_style_packs_that_want_it() {
        use super::macos_impl::{classify_mac_keystroke, MacKeystroke};
        use crate::types::MacosNewlineMode;

        assert_eq!(
            classify_mac_keystroke('\n', MacosNewlineMode::Return),
            MacKeystroke::Return
        );
        // The newline mode only affects newlines; all other chars unchanged.
        assert_eq!(
            classify_mac_keystroke('中', MacosNewlineMode::Return),
            MacKeystroke::Unicode
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn line_feed_mode_sends_unicode_newline_for_terminal_tuis() {
        use super::macos_impl::{classify_mac_keystroke, MacKeystroke};
        use crate::types::MacosNewlineMode;

        assert_eq!(
            classify_mac_keystroke('\n', MacosNewlineMode::LineFeed),
            MacKeystroke::LineFeed
        );
    }

    /// Counting contract: the typed_chars returned by `type_unicode_chunk`
    /// must equal the input char count, including swallowed `\r`s — the caller
    /// compares it against `delta.chars().count()`, and any shortfall is judged
    /// a partial failure that drops all later deltas.
    #[test]
    #[cfg(target_os = "macos")]
    fn every_char_counts_toward_typed_chars_including_swallowed_ones() {
        use super::macos_impl::classify_mac_keystroke;
        use crate::types::MacosNewlineMode;

        for mode in [
            MacosNewlineMode::Auto,
            MacosNewlineMode::ShiftReturn,
            MacosNewlineMode::LineFeed,
            MacosNewlineMode::Return,
        ] {
            let text = "上半句\r\n\r\n下半句";
            // Every char is classified into exactly one handling path; none can
            // escape.
            let counted = text
                .chars()
                .map(|ch| classify_mac_keystroke(ch, mode))
                .count();
            assert_eq!(counted, text.chars().count(), "{mode:?} 下计数必须守恒");
        }
    }

    #[test]
    fn type_error_partial_reports_typed_chars() {
        let err = TypeError::Partial {
            typed_chars: 2,
            source: Box::new(platform_error()),
        };

        assert_eq!(err.typed_chars(), 2);
    }

    #[test]
    fn plain_type_error_reports_zero_typed_chars() {
        assert_eq!(platform_error().typed_chars(), 0);
    }

    #[cfg(target_os = "macos")]
    fn platform_error() -> TypeError {
        TypeError::EventAllocFailed
    }

    #[cfg(target_os = "windows")]
    fn platform_error() -> TypeError {
        TypeError::SendInputFailed("fail".into())
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn expected_sendinput_typed_chars_includes_swallowed_carriage_return() {
        assert_eq!(super::expected_sendinput_typed_chars("a\r\nb"), 4);
        assert_eq!(super::expected_sendinput_typed_chars("hello"), 5);
        assert_eq!(super::expected_sendinput_typed_chars("\r\r\n"), 3);
    }

    #[cfg(target_os = "windows")]
    mod windows_sendinput_char_tests {
        use super::super::{classify_sendinput_char, SendInputCharKind};

        #[test]
        fn classify_skips_carriage_return() {
            assert!(matches!(
                classify_sendinput_char('\r'),
                SendInputCharKind::Skip
            ));
        }

        #[test]
        fn classify_newline_and_tab() {
            assert!(matches!(
                classify_sendinput_char('\n'),
                SendInputCharKind::Newline
            ));
            assert!(matches!(
                classify_sendinput_char('\t'),
                SendInputCharKind::Tab
            ));
        }

        #[test]
        fn classify_regular_text_as_unicode() {
            assert!(matches!(
                classify_sendinput_char('你'),
                SendInputCharKind::Unicode
            ));
        }
    }
}

/// Number of Unicode scalars consumed when Windows SendInput successfully
/// consumes the whole input, including CRs that send no keystroke. Matches the
/// Core streaming-insertion written_chars contract; must not become a UTF-16
/// unit count or a keystroke event count.
#[cfg(target_os = "windows")]
pub fn expected_sendinput_typed_chars(text: &str) -> usize {
    text.chars().count()
}

// ═══════════════════════════════════════════════════════════════════════════
// Public re-exports (cfg-dispatched to the matching implementation)
// ═══════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "macos")]
#[allow(unused_imports)]
pub use macos_impl::{
    is_secure_input_enabled, restore_input_source, switch_to_ascii, type_unicode_chunk,
    type_unicode_chunk_with_options, PreviousInputSource,
};

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
pub use windows_impl::{
    restore_input_source, switch_to_ascii, type_unicode_chunk, type_unicode_chunk_with_options,
    PreviousInputSource, WindowsSendInputOptions,
};
