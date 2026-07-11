#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! 跨平台 Unicode keystroke 合成（流式输入用）。
//!
//! 公开 API 三件套：
//! - `type_unicode_chunk(text)` —— 阻塞地把一段文字逐 codepoint 当作键盘事件发出去，
//!   不动剪贴板。各平台用各自的原语；返回确认成功发送的字符数。
//! - `switch_to_ascii(app)` —— 仅 macOS 有效；切到 ABC 输入源以绕过 CJK / 日文 IME
//!   对 Unicode 字符串事件的拦截。Windows / Linux 上是 no-op。
//! - `restore_input_source(app, prev)` —— 配对调用，恢复 macOS 上的原输入源。
//!
//! ## 平台差异
//!
//! - **macOS**：手写 CGEvent FFI（与 `insertion.rs::macos` 的 Cmd+V 同源）。
//!   `CGEventKeyboardSetUnicodeString` 在 CJK / 日文 IME 激活时被拦截 ——
//!   必须 `switch_to_ascii` 切到 ABC，session 结束再 `restore_input_source` 切回。
//! - **Windows**：`SendInput(KEYEVENTF_UNICODE)` 直接发 UTF-16 scancode。TSF 不拦
//!   Unicode 事件（与 keyboard layout / IME 解耦），所以不需要切输入法。
//! - **Linux**：走 fcitx5 插件 commitString 直写（DBus）或剪贴板回落。
//!
//! ## 已知坑（macOS）
//!
//! - Secure Event Input（密码框、1Password 等）下 CGEventPost 静默失败；
//!   `type_unicode_chunk` 开头先用 `IsSecureEventInputEnabled` 探测，命中即返
//!   `TypeError::SecureInputActive`。
//! - Modifier 状态继承 —— 用户按着 Shift 不清零会被映射成大写，每个事件显式
//!   `CGEventSetFlags(_, 0)`。
//! - Chromium / Electron / Tauri 自身在 keyDown/keyUp 之间无延迟时会丢字，每 codepoint
//!   sleep 1ms。
//!
//! ## 线程安全（macOS）
//!
//! - `type_unicode_chunk`（CGEventPost）任意线程可调，对齐 `insertion.rs::macos::
//!   simulate_paste` 现状。
//! - TIS（`switch_to_ascii` / `restore_input_source`）调度到主线程，规避 macOS 14+
//!   对 TSM/TIS 主线程的 `dispatch_assert_queue_fail` SIGTRAP。

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
    #[cfg(target_os = "linux")]
    #[error("enigo init failed: {0}")]
    EnigoInit(String),
    #[cfg(target_os = "linux")]
    #[error("enigo text input failed: {0}")]
    EnigoText(String),
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
// macOS 实现
// ═══════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "macos")]
mod macos_impl {
    use super::{TisError, TypeError};
    use std::ffi::c_void;
    use std::time::Duration;
    use tauri::{AppHandle, Runtime};

    const INTER_KEYSTROKE_DELAY: Duration = Duration::from_millis(1);

    /// 之前激活的 input source 引用 token。携带 raw ptr 的 usize 表示，所有解引用都
    /// 通过 `restore_input_source` 调度到主线程执行；手动 `Send + Sync`。
    pub struct PreviousInputSource {
        raw: usize,
    }
    unsafe impl Send for PreviousInputSource {}
    unsafe impl Sync for PreviousInputSource {}

    pub fn type_unicode_chunk(text: &str) -> Result<usize, TypeError> {
        if text.is_empty() {
            return Ok(0);
        }
        if is_secure_input_enabled() {
            return Err(TypeError::SecureInputActive);
        }
        let mut typed_chars = 0;
        for ch in text.chars() {
            if let Err(e) = send_one_codepoint(ch) {
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
        let len = utf16.len();
        unsafe {
            let src = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            if src.is_null() {
                return Err(TypeError::SourceAllocFailed);
            }
            let down = CGEventCreateKeyboardEvent(src, 0, true);
            let up = CGEventCreateKeyboardEvent(src, 0, false);
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
            CGEventSetFlags(down, 0);
            CGEventSetFlags(up, 0);
            CGEventKeyboardSetUnicodeString(down, len, utf16.as_ptr());
            CGEventKeyboardSetUnicodeString(up, len, utf16.as_ptr());
            CGEventPost(KCG_HID_EVENT_TAP, down);
            CGEventPost(KCG_HID_EVENT_TAP, up);
            CFRelease(down as _);
            CFRelease(up as _);
            CFRelease(src as _);
        }
        Ok(())
    }

    fn is_secure_input_enabled() -> bool {
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
// Windows 实现
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

    /// Windows / Linux 上没有 input source 概念，token 留空。Send/Sync 自动派生。
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
        if text.is_empty() {
            return Ok(0);
        }
        let mut typed_chars = 0;
        let mut sent_in_chunk = 0usize;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            // 分类与计数复用 `classify_sendinput_char` / `sendinput_char_is_typed`——与
            // `expected_sendinput_typed_chars` 同一真相，避免规则漂移导致成功的 SendInput
            // 被误判为回落。
            match super::classify_sendinput_char(ch) {
                super::SendInputCharKind::Skip => continue,
                super::SendInputCharKind::Newline => {
                    if let Err(e) = send_newline(options.newline_mode) {
                        return Err(partial_or_original(typed_chars, e));
                    }
                }
                super::SendInputCharKind::Tab => {
                    if let Err(e) = press_vk(VK_TAB) {
                        return Err(partial_or_original(typed_chars, e));
                    }
                }
                super::SendInputCharKind::Unicode => {
                    let mut buf = [0u16; 2];
                    for unit in ch.encode_utf16(&mut buf) {
                        if let Err(e) = send_utf16_unit(*unit, false) {
                            return Err(partial_or_original(typed_chars, e));
                        }
                        if let Err(e) = send_utf16_unit(*unit, true) {
                            return Err(partial_or_original(typed_chars, e));
                        }
                    }
                }
            }
            typed_chars += 1;
            sent_in_chunk += 1;

            if sent_in_chunk >= SENDINPUT_CHUNK_CHARS && chars.peek().is_some() {
                std::thread::sleep(SENDINPUT_CHUNK_DELAY);
                sent_in_chunk = 0;
            }
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

    /// Windows SendInput Unicode 绕过 TSF 与 IME，无需切换输入法。返回 `Ok(None)`，
    /// `restore_input_source` 也是 no-op。
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

/// SendInput 单字符分类的唯一真相。`type_unicode_chunk_with_options` 的实际打字路径与
/// `expected_sendinput_typed_chars`（用于校验实际发出的 typed char 数）都复用它，避免三处
/// 独立的字符规则手工同步——一旦漏改，一次成功的 SendInput 会被 `map_sendinput_type_result`
/// 误判成 `CopiedFallback`，在已打字的基础上又把整段复制到剪贴板，用户 Ctrl+V 看到重复。
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendInputCharKind {
    /// `\r`：跳过（CRLF 只产生一次换行），不计入 typed char。
    Skip,
    /// `\n`：按换行发出（模式由 `WindowsSendInputOptions::newline_mode` 决定）。
    Newline,
    /// `\t`：按 Tab 键发出。
    Tab,
    /// 其余字符：按 UTF-16 Unicode 事件逐 code unit 发出。
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

/// 该字符是否计入「已发出的 typed char」。`Skip`（`\r`）不计入，其余都计入。
#[cfg(target_os = "windows")]
pub(crate) fn sendinput_char_is_typed(kind: SendInputCharKind) -> bool {
    !matches!(kind, SendInputCharKind::Skip)
}

// ═══════════════════════════════════════════════════════════════════════════
// Linux 实现（实验性）
// ═══════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "linux")]
mod linux_impl {
    use super::{TisError, TypeError};
    #[allow(unused_imports)]
    use tauri::{AppHandle, Runtime};

    pub struct PreviousInputSource;

    /// 通过 fcitx5 插件一次性提交整段文字（支持中文、Wayland/X11 均可）。
    /// 如果插件未加载返回 Err，调用方降级到剪贴板拷贝。
    pub fn type_unicode_chunk(text: &str) -> Result<usize, TypeError> {
        if text.is_empty() {
            return Ok(0);
        }
        if crate::linux_fcitx::commit_text(text).is_ok() {
            Ok(text.chars().count())
        } else {
            Err(TypeError::EnigoText(
                "fcitx5 plugin unavailable, try clipboard fallback".into(),
            ))
        }
    }

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

#[cfg(test)]
mod tests {
    use super::TypeError;

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

    #[cfg(target_os = "linux")]
    fn platform_error() -> TypeError {
        TypeError::EnigoText("fail".into())
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn expected_sendinput_typed_chars_skips_carriage_return() {
        assert_eq!(super::expected_sendinput_typed_chars("a\r\nb"), 3);
        assert_eq!(super::expected_sendinput_typed_chars("hello"), 5);
        assert_eq!(super::expected_sendinput_typed_chars("\r\r\n"), 1);
    }

    #[cfg(target_os = "windows")]
    mod windows_sendinput_char_tests {
        use super::super::{
            classify_sendinput_char, expected_sendinput_typed_chars, sendinput_char_is_typed,
            SendInputCharKind,
        };

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
            assert!(matches!(classify_sendinput_char('\t'), SendInputCharKind::Tab));
        }

        #[test]
        fn classify_regular_text_as_unicode() {
            assert!(matches!(
                classify_sendinput_char('你'),
                SendInputCharKind::Unicode
            ));
        }

        /// 只有 `Skip`（`\r`）不计入 typed char，其余三类都计入。这是
        /// `expected_sendinput_typed_chars` 与实际打字循环 `typed_chars += 1` 之间保持一致
        /// 的核心不变量。
        #[test]
        fn only_carriage_return_is_not_counted() {
            assert!(!sendinput_char_is_typed(SendInputCharKind::Skip));
            assert!(sendinput_char_is_typed(SendInputCharKind::Newline));
            assert!(sendinput_char_is_typed(SendInputCharKind::Tab));
            assert!(sendinput_char_is_typed(SendInputCharKind::Unicode));
        }

        /// 期望计数必须与「逐字符分类后计入的数量」逐字节一致——即 expected 复用了同一分类
        /// 真相。若二者用不同规则表达，成功的 SendInput 会被 `map_sendinput_type_result`
        /// 误判为 `CopiedFallback`（重复粘贴）。
        #[test]
        fn expected_count_matches_per_char_classification() {
            for sample in ["a\r\nb", "hello", "\r\r\n", "行1\n\t行2", ""] {
                let manual = sample
                    .chars()
                    .filter(|ch| sendinput_char_is_typed(classify_sendinput_char(*ch)))
                    .count();
                assert_eq!(expected_sendinput_typed_chars(sample), manual, "{sample:?}");
            }
        }
    }
}

/// Windows SendInput 路径上 `type_unicode_chunk` 计入的 typed char 数。
/// `\r` 会被跳过（CRLF 只产生一次换行），因此不能与 `text.chars().count()` 直接比较。
///
/// 复用 `classify_sendinput_char` / `sendinput_char_is_typed`——与实际打字路径同一真相，
/// 保证 `map_sendinput_type_result` 的期望值与真正发出的 typed char 数永远一致。
#[cfg(target_os = "windows")]
pub fn expected_sendinput_typed_chars(text: &str) -> usize {
    text.chars()
        .filter(|ch| sendinput_char_is_typed(classify_sendinput_char(*ch)))
        .count()
}

// ═══════════════════════════════════════════════════════════════════════════
// 公共导出（按 cfg 分发到对应实现）
// ═══════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "macos")]
#[allow(unused_imports)]
pub use macos_impl::{
    restore_input_source, switch_to_ascii, type_unicode_chunk, PreviousInputSource,
};

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
pub use windows_impl::{
    restore_input_source, switch_to_ascii, type_unicode_chunk, type_unicode_chunk_with_options,
    PreviousInputSource, WindowsSendInputOptions,
};

#[cfg(target_os = "linux")]
#[allow(unused_imports)]
pub use linux_impl::{
    restore_input_source, switch_to_ascii, type_unicode_chunk, PreviousInputSource,
};
