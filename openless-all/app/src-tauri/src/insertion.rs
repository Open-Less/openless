//! 跨平台光标位置文本插入。
//!
//! 通用步骤：先写剪贴板（模拟失败时用户能手动粘贴）→ 模拟粘贴快捷键。
//! - macOS：用 CoreGraphics CGEvent 直接 post Cmd+V。
//! - Windows：用 enigo 按 `PasteShortcut` 模拟。

use crate::types::{InsertStatus, PasteShortcut};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[path = "insertion/clipboard_delivery.rs"]
pub(crate) mod clipboard_delivery;

/// Put a text into the clipboard; used by the "Copy" button on the insertion-failure fallback card.
///
/// A separate entry instead of letting the frontend call `navigator.clipboard`: the card floats
/// above another app and its button does not take focus, and in an unfocused document that API
/// throws `Document is not focused`.
pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    copy_text_to_clipboard_guarded(text, || true)
        .map(|_| ())
        .map_err(|error| {
            log::warn!("[insertion] clipboard write failed: {error}");
            "clipboard write failed".to_string()
        })
}

/// Native ownership token; unlike text equality this detects a later identical user copy.
pub(crate) fn clipboard_sequence() -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        Some(unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() } as u64)
    }
    #[cfg(target_os = "macos")]
    {
        unsafe {
            use objc2::{
                msg_send,
                runtime::{AnyClass, AnyObject},
            };
            let cls = AnyClass::get("NSPasteboard")?;
            let board: *mut AnyObject = msg_send![cls, generalPasteboard];
            if board.is_null() {
                return None;
            }
            let count: isize = msg_send![board, changeCount];
            u64::try_from(count).ok()
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

pub struct TextInserter;

impl TextInserter {
    pub fn new() -> Self {
        Self
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn insert_confirmed(
        &self,
        text: &str,
        restore: bool,
        shortcut: PasteShortcut,
        confirm: impl FnOnce() -> bool,
    ) -> InsertStatus {
        let mut clipboard = match NativeClipboardDelivery::new(shortcut) {
            Ok(clipboard) => clipboard,
            Err(error) => {
                log::warn!("clipboard preparation failed: {error}");
                return InsertStatus::Failed;
            }
        };
        clipboard_delivery::insert_with_confirmation(
            &mut clipboard, text, restore, None, || true, Some(confirm),
        )
    }

    /// Borrow the clipboard through dispatch, the consumption window and optional restore.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) fn insert_guarded(
        &self,
        text: &str,
        restore: bool,
        shortcut: PasteShortcut,
        expected_clipboard: Option<u64>,
        still_current: impl FnMut() -> bool,
    ) -> InsertStatus {
        let mut clipboard = match NativeClipboardDelivery::new(shortcut) {
            Ok(clipboard) => clipboard,
            Err(error) => {
                log::warn!("clipboard preparation failed: {error}");
                return InsertStatus::Failed;
            }
        };
        clipboard_delivery::insert_with(
            &mut clipboard,
            text,
            restore,
            expected_clipboard,
            still_current,
        )
    }

    /// Windows 路径：写剪贴板 + 模拟 `paste_shortcut`。
    /// - `restore_clipboard_after_paste`：粘贴后是否恢复用户原剪贴板。
    /// - `paste_shortcut`：模拟按下的粘贴快捷键（如终端可能要 Ctrl+Shift+V）。
    #[cfg(target_os = "windows")]
    pub fn insert(
        &self,
        text: &str,
        restore_clipboard_after_paste: bool,
        paste_shortcut: PasteShortcut,
    ) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        insert_with_clipboard_restore(text, restore_clipboard_after_paste, paste_shortcut)
    }

    #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
    pub fn insert_via_clipboard_fallback(
        &self,
        text: &str,
        restore_clipboard_after_paste: bool,
        paste_shortcut: PasteShortcut,
    ) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        insert_with_clipboard_restore(text, restore_clipboard_after_paste, paste_shortcut)
    }

    #[cfg(target_os = "windows")]
    pub fn insert_via_unicode_keystrokes(
        &self,
        text: &str,
        options: crate::unicode_keystroke::WindowsSendInputOptions,
    ) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        map_sendinput_type_result(
            text,
            crate::unicode_keystroke::type_unicode_chunk_with_options(text, options),
        )
    }

    /// macOS path: save the original clipboard -> write the transcribed text -> post Cmd+V ->
    /// restore the original clipboard on demand.
    /// `_paste_shortcut` is unused on macOS (fixed Cmd+V); it exists only to match the
    /// cross-platform signature.
    #[cfg(target_os = "macos")]
    pub fn insert(
        &self,
        text: &str,
        restore_clipboard_after_paste: bool,
        _paste_shortcut: PasteShortcut,
    ) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        self.insert_guarded(
            text,
            restore_clipboard_after_paste,
            _paste_shortcut,
            None,
            || true,
        )
    }

    /// Android: cross-app input is handled by the dictation flow per user policy; generic
    /// insertion only writes the clipboard as a fallback.
    #[cfg(target_os = "android")]
    pub fn insert(
        &self,
        text: &str,
        _restore_clipboard_after_paste: bool,
        _paste_shortcut: PasteShortcut,
    ) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        self.copy_fallback(text)
    }

    /// 无原生粘贴快捷键实现的桌面构建：退回剪贴板兜底，与
    /// [`Self::insert_via_clipboard_fallback`] 同语义（典型场景：终端/无合成键平台）。
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    pub fn insert(
        &self,
        text: &str,
        restore_clipboard_after_paste: bool,
        paste_shortcut: PasteShortcut,
    ) -> InsertStatus {
        self.insert_via_clipboard_fallback(text, restore_clipboard_after_paste, paste_shortcut)
    }

    /// 只写剪贴板、不模拟粘贴。用于目标控件活跃状态无法验证时的兜底路径。
    pub fn copy_fallback(&self, text: &str) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        if copy_text_to_clipboard(text).is_ok() {
            InsertStatus::CopiedFallback
        } else {
            InsertStatus::Failed
        }
    }
}

impl Default for TextInserter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
fn map_sendinput_type_result(
    text: &str,
    result: Result<usize, crate::unicode_keystroke::TypeError>,
) -> InsertStatus {
    let expected = crate::unicode_keystroke::expected_sendinput_typed_chars(text);
    match result {
        Ok(typed_chars) if typed_chars == expected => InsertStatus::Inserted,
        Ok(typed_chars) => {
            log::warn!("[insertion] Unicode SendInput typed only {typed_chars}/{expected} chars");
            InsertStatus::PasteSent
        }
        Err(err) => {
            log::warn!("[insertion] Unicode SendInput failed: {err}");
            InsertStatus::PasteSent
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
struct NativeClipboardDelivery {
    clipboard: arboard::Clipboard,
    shortcut: PasteShortcut,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl NativeClipboardDelivery {
    fn new(shortcut: PasteShortcut) -> Result<Self, String> {
        Ok(Self {
            clipboard: arboard::Clipboard::new().map_err(|error| error.to_string())?,
            shortcut,
        })
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl clipboard_delivery::ClipboardDelivery for NativeClipboardDelivery {
    fn text(&mut self) -> Result<Option<String>, String> {
        match self.clipboard.get_text() {
            Ok(text) => Ok(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        let _own_write = crate::dictation_references::self_write();
        self.clipboard
            .set_text(text.to_string())
            .map_err(|error| error.to_string())
    }

    fn sequence(&self) -> Option<u64> {
        clipboard_sequence()
    }

    fn paste(&mut self) -> Result<(), clipboard_delivery::DispatchError> {
        #[cfg(target_os = "macos")]
        let result = {
            let _ = self.shortcut;
            simulate_paste()
        };
        #[cfg(not(target_os = "macos"))]
        let result = simulate_paste(self.shortcut);
        result.map_err(|error| {
            log::warn!("[insertion] simulated paste failed: {error}");
            if cfg!(target_os = "macos") {
                clipboard_delivery::DispatchError::NotSent
            } else {
                // Windows may have sent Ctrl+V before a key-up error.
                clipboard_delivery::DispatchError::OutcomeUnknown
            }
        })
    }

    fn wait_for_consumption(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(750));
    }
}

pub(crate) fn copy_text_to_clipboard_guarded(
    text: &str,
    still_current: impl FnOnce() -> bool,
) -> Result<Option<u64>, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        clipboard_delivery::copy_with(
            &mut NativeClipboardDelivery::new(PasteShortcut::CtrlV)?,
            text,
            still_current,
        )
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        if !still_current() {
            return Err("selection session cancelled".into());
        }
        copy_to_clipboard(text)
            .then_some(None)
            .ok_or_else(|| "clipboard write failed".into())
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn copy_text_to_clipboard_if_current(
    text: &str,
    expected: Option<u64>,
    still_current: impl FnOnce() -> bool,
) -> Result<bool, String> {
    clipboard_delivery::copy_if_current_with(
        &mut NativeClipboardDelivery::new(PasteShortcut::CtrlV)?,
        text,
        expected,
        still_current,
    )
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn copy_to_clipboard(text: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        return crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::copy_to_clipboard(env, context, text)
        })
        .unwrap_or_else(|error| {
            log::error!("[insertion] android clipboard failed: {error}");
            false
        });
    }

    #[cfg(target_os = "ios")]
    {
        let _ = text;
        log::warn!("[insertion] mobile clipboard fallback unavailable");
        false
    }
}

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn insert_with_clipboard_restore(
    text: &str,
    restore_clipboard_after_paste: bool,
    paste_shortcut: PasteShortcut,
) -> InsertStatus {
    TextInserter::new().insert_guarded(
        text,
        restore_clipboard_after_paste,
        paste_shortcut,
        None,
        || true,
    )
}

pub(crate) fn clipboard_revision_matches(expected: Option<u64>, current: Option<u64>) -> bool {
    expected.is_some() && expected == current
}

#[cfg(any(target_os = "macos", test))]
fn simulate_macos_paste_with<P>(
    accessibility_granted: bool,
    secure_input_active: bool,
    post_cmd_v: P,
) -> Result<(), String>
where
    P: FnOnce() -> Result<(), String>,
{
    if !accessibility_granted {
        return Err("accessibility permission is not granted".into());
    }
    if secure_input_active {
        return Err("secure input is active".into());
    }
    // CoreGraphics can only confirm the synthetic event was constructed and delivered; it
    // cannot confirm the foreground app accepted Cmd+V or actually inserted text.
    post_cmd_v()
}

#[cfg(target_os = "macos")]
fn simulate_paste() -> Result<(), String> {
    simulate_macos_paste_with(
        matches!(
            crate::permissions::check_accessibility(),
            crate::permissions::PermissionStatus::Granted
        ),
        crate::unicode_keystroke::is_secure_input_enabled(),
        macos::post_cmd_v,
    )
}

/// Split `PasteShortcut` into `(modifiers, primary)`; the order defines press/release order.
#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn paste_keys(shortcut: PasteShortcut) -> (Vec<enigo::Key>, enigo::Key) {
    use enigo::Key;
    match shortcut {
        PasteShortcut::CtrlV => (vec![Key::Control], Key::Unicode('v')),
        PasteShortcut::CtrlShiftV => (vec![Key::Control, Key::Shift], Key::Unicode('v')),
        PasteShortcut::ShiftInsert => (vec![Key::Shift], Key::Insert),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn simulate_paste(shortcut: PasteShortcut) -> Result<(), String> {
    use enigo::{Direction, Enigo, Keyboard, Settings};
    let (modifiers, primary) = paste_keys(shortcut);
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;

    // Order: press modifiers -> tap the primary key -> release modifiers in reverse.
    // If any step fails, release the already-pressed modifiers in reverse to avoid stuck keys.
    let mut pressed = 0usize;
    let mut first_err: Option<String> = None;

    for modifier in &modifiers {
        if let Err(e) = enigo.key(*modifier, Direction::Press) {
            first_err = Some(e.to_string());
            break;
        }
        pressed += 1;
    }

    if first_err.is_none() {
        if let Err(e) = enigo.key(primary, Direction::Click) {
            first_err = Some(e.to_string());
        }
    }

    for modifier in modifiers[..pressed].iter().rev() {
        if let Err(e) = enigo.key(*modifier, Direction::Release) {
            if first_err.is_none() {
                first_err = Some(e.to_string());
            }
        }
    }

    match first_err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

// ── macOS CGEvent paste ──
// Call the CoreGraphics FFI directly to send Cmd+V, avoiding TSM assertions enigo triggers
// off the main thread.

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    #[repr(C)]
    struct OpaqueCGEvent(c_void);
    type CGEventRef = *mut OpaqueCGEvent;

    #[repr(C)]
    struct OpaqueCGEventSource(c_void);
    type CGEventSourceRef = *mut OpaqueCGEventSource;

    type CGEventTapLocation = u32;
    type CGEventSourceStateID = i32;
    type CGKeyCode = u16;
    type CGEventFlags = u64;

    const KCG_HID_EVENT_TAP: CGEventTapLocation = 0;
    const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: CGEventSourceStateID = 1;
    const KCG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x00100000;
    /// Virtual key code of "V" on a US/ANSI keyboard (kVK_ANSI_V).
    const KEY_V: CGKeyCode = 9;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state_id: CGEventSourceStateID) -> CGEventSourceRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: CGKeyCode,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
        fn CGEventPost(tap: CGEventTapLocation, event: CGEventRef);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    /// Simulate Cmd+V: build V down/up events, add the Cmd flag, and post both to the HID event stream.
    pub fn post_cmd_v() -> Result<(), String> {
        unsafe {
            let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            // Posting is still legal when source is NULL (allowed by Apple's docs); not fatal.
            let down = CGEventCreateKeyboardEvent(source, KEY_V, true);
            let up = CGEventCreateKeyboardEvent(source, KEY_V, false);
            if down.is_null() || up.is_null() {
                if !source.is_null() {
                    CFRelease(source as *const c_void);
                }
                if !down.is_null() {
                    CFRelease(down as *const c_void);
                }
                if !up.is_null() {
                    CFRelease(up as *const c_void);
                }
                return Err("CGEventCreateKeyboardEvent returned null".into());
            }
            CGEventSetFlags(down, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventSetFlags(up, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(KCG_HID_EVENT_TAP, down);
            CGEventPost(KCG_HID_EVENT_TAP, up);

            CFRelease(down as *const c_void);
            CFRelease(up as *const c_void);
            if !source.is_null() {
                CFRelease(source as *const c_void);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The configured shortcut must really map to the corresponding keys. Compares only the
    /// modifier count + primary key, bypassing enigo's internal PartialEq.
    #[test]
    #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
    fn paste_keys_match_configured_shortcut() {
        use enigo::Key;

        let (mods, primary) = paste_keys(PasteShortcut::CtrlV);
        assert_eq!(mods.len(), 1);
        assert!(matches!(mods[0], Key::Control));
        assert!(matches!(primary, Key::Unicode('v')));

        let (mods, primary) = paste_keys(PasteShortcut::CtrlShiftV);
        assert_eq!(mods.len(), 2);
        assert!(matches!(mods[0], Key::Control));
        assert!(matches!(mods[1], Key::Shift));
        assert!(matches!(primary, Key::Unicode('v')));

        let (mods, primary) = paste_keys(PasteShortcut::ShiftInsert);
        assert_eq!(mods.len(), 1);
        assert!(matches!(mods[0], Key::Shift));
        assert!(matches!(primary, Key::Insert));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn crlf_sendinput_success_uses_expected_typed_count() {
        // Includes the safely swallowed CR: the receipt reports the consumed source prefix,
        // not the number of keystrokes sent.
        let text = "a\r\nb";
        let expected = crate::unicode_keystroke::expected_sendinput_typed_chars(text);
        let status = map_sendinput_type_result(text, Ok(expected));
        assert_eq!(status, InsertStatus::Inserted);
        assert_eq!(expected, 4);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn crlf_sendinput_partial_mismatch_remains_uncertain() {
        // Only a + CR + LF were consumed; the trailing b was not, so the partial-failure
        // semantics must hold.
        let text = "a\r\nb";
        let expected = crate::unicode_keystroke::expected_sendinput_typed_chars(text);
        let mismatched = 3;
        assert_ne!(mismatched, expected);
        let status = map_sendinput_type_result(text, Ok(mismatched));
        assert_eq!(status, InsertStatus::PasteSent);
    }

    #[test]
    fn empty_insertions_never_touch_clipboard_or_paste_path() {
        let inserter = TextInserter::new();

        assert_eq!(
            inserter.insert("", true, PasteShortcut::CtrlV),
            InsertStatus::CopiedFallback
        );
        #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
        {
            assert_eq!(
                inserter.insert_via_clipboard_fallback("", true, PasteShortcut::CtrlV),
                InsertStatus::CopiedFallback
            );
        }
        assert_eq!(inserter.copy_fallback(""), InsertStatus::CopiedFallback);
    }

    #[test]
    fn clipboard_ownership_rejects_a_new_copy_even_when_text_is_identical() {
        assert!(clipboard_revision_matches(Some(11), Some(11)));
        assert!(!clipboard_revision_matches(Some(11), Some(12)));
        assert!(!clipboard_revision_matches(None, None));
    }

    #[test]
    fn macos_paste_preflight_rejects_missing_accessibility_before_posting() {
        let mut posted = false;
        let result = simulate_macos_paste_with(false, false, || {
            posted = true;
            Ok(())
        });

        assert_eq!(
            result.unwrap_err(),
            "accessibility permission is not granted"
        );
        assert!(!posted);
    }

    #[test]
    fn macos_paste_preflight_rejects_secure_input_before_posting() {
        let mut posted = false;
        let result = simulate_macos_paste_with(true, true, || {
            posted = true;
            Ok(())
        });

        assert_eq!(result.unwrap_err(), "secure input is active");
        assert!(!posted);
    }

    #[test]
    fn macos_paste_preflight_posts_when_guards_are_clear() {
        let mut posted = false;
        let result = simulate_macos_paste_with(true, false, || {
            posted = true;
            Ok(())
        });

        assert_eq!(result, Ok(()));
        assert!(posted);
    }
}
