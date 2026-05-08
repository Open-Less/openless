//! Cross-platform text insertion at the current cursor position.
//!
//! Strategy:
//! 1. Always copy the text to the clipboard first (so the user can manually
//!    `Cmd+V` / `Ctrl+V` if simulation fails).
//! 2. On macOS, simulate Cmd+V via raw `CGEventPost` FFI — **不能用 enigo**：
//!    enigo 在 macOS 上的 keycode_to_string 会同步调 `TSMGetInputSourceProperty`，
//!    macOS 14+ 强制断言主线程，从 tokio worker 线程调就 SIGTRAP（已踩坑）。
//!    Swift 原版 `TextInserter.simulatePaste()` 用的就是 CGEventCreateKeyboardEvent
//!    → CGEventPost，跟我们这里完全同源。
//! 3. 其他平台 (Windows/Linux) 仍用 enigo。

#[cfg(not(target_os = "macos"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_os = "macos"))]
use std::time::Duration;

#[cfg(not(target_os = "macos"))]
use once_cell::sync::Lazy;
#[cfg(not(target_os = "macos"))]
use parking_lot::Mutex;

use crate::types::InsertStatus;

#[cfg(target_os = "windows")]
const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(750);

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(750);

pub struct TextInserter;

impl TextInserter {
    pub fn new() -> Self {
        Self
    }

    /// Insert `text` at the current cursor position.
    /// `restore_clipboard_after_paste` 仅在 Windows/Linux 路径下决定 paste 之后是否恢复
    /// 用户原剪贴板。macOS 走 AX 直写，参数被忽略。详见 issue #111。
    #[cfg(not(target_os = "macos"))]
    pub fn insert(&self, text: &str, restore_clipboard_after_paste: bool) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        insert_with_clipboard_restore(text, restore_clipboard_after_paste)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn insert_via_clipboard_fallback(
        &self,
        text: &str,
        restore_clipboard_after_paste: bool,
    ) -> InsertStatus {
        self.insert(text, restore_clipboard_after_paste)
    }

    #[cfg(target_os = "windows")]
    pub fn insert_via_unicode_keystrokes(&self, text: &str) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        // Don't actually inject Unicode keystrokes here. Per-codepoint
        // KEYEVENTF_UNICODE SendInput on a Japanese host competes with the
        // IME's composition state (ATOK / Microsoft IME), so hiragana ends up
        // queued in the IME composition window while kanji / ascii get
        // inserted ahead of it — the user sees text reordered with kanji
        // pushed to the end. Route through the clipboard + Ctrl+V path
        // instead.
        //
        // Important: callers in coordinator.rs gate the non-TSF fallback on
        // `== InsertStatus::Inserted`. `self.insert` returns `PasteSent` on
        // Windows (we sent Ctrl+V but can't prove the target swallowed it),
        // so if we returned that as-is the caller would treat it as failure
        // and run the fallback path, double-pasting the text. Force
        // `Inserted` here to suppress the redundant fallback.
        let _ = self.insert(text, true);
        InsertStatus::Inserted
    }

    /// Insert `text` at the current cursor position.
    #[cfg(target_os = "macos")]
    pub fn insert(&self, text: &str, _restore_clipboard_after_paste: bool) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        if !copy_to_clipboard(text) {
            return InsertStatus::Failed;
        }
        if let Err(err) = simulate_paste() {
            log::warn!("[insertion] simulated paste failed: {}", err);
            return InsertStatus::CopiedFallback;
        }
        insertion_success_status()
    }

    /// Copy text without attempting a synthetic paste. Used when the platform cannot
    /// prove the original input target is active enough to safely receive Ctrl/Cmd+V.
    pub fn copy_fallback(&self, text: &str) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        if copy_to_clipboard(text) {
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

#[cfg(not(target_os = "macos"))]
#[derive(Debug)]
struct ClipboardRestorePlan {
    inserted_text: String,
    previous_text: Option<String>,
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone)]
struct PendingClipboardRestore {
    latest_restore_id: u64,
    original_text: Option<String>,
}

#[cfg(not(target_os = "macos"))]
static NEXT_CLIPBOARD_RESTORE_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(not(target_os = "macos"))]
static PENDING_CLIPBOARD_RESTORE: Lazy<Mutex<Option<PendingClipboardRestore>>> =
    Lazy::new(|| Mutex::new(None));

fn copy_to_clipboard(text: &str) -> bool {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(err) => {
            log::error!("[insertion] clipboard init failed: {}", err);
            return false;
        }
    };
    if let Err(err) = clipboard.set_text(text.to_string()) {
        log::error!("[insertion] clipboard set_text failed: {}", err);
        return false;
    }
    true
}

#[cfg(not(target_os = "macos"))]
fn copy_to_clipboard_with_restore_plan(text: &str) -> Result<ClipboardRestorePlan, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let previous_text = match clipboard.get_text() {
        Ok(existing) => Some(existing),
        Err(err) => {
            log::warn!(
                "[insertion] clipboard get_text failed before overwrite: {}",
                err
            );
            None
        }
    };
    clipboard
        .set_text(text.to_string())
        .map_err(|e| e.to_string())?;
    Ok(ClipboardRestorePlan {
        inserted_text: text.to_string(),
        previous_text,
    })
}

#[cfg(not(target_os = "macos"))]
fn insert_with_clipboard_restore(text: &str, restore_clipboard_after_paste: bool) -> InsertStatus {
    let restore_plan = match copy_to_clipboard_with_restore_plan(text) {
        Ok(plan) => plan,
        Err(err) => {
            log::error!("[insertion] clipboard write failed: {}", err);
            return InsertStatus::Failed;
        }
    };

    if let Err(err) = simulate_paste() {
        log::warn!("[insertion] simulated paste failed: {}", err);
        return InsertStatus::CopiedFallback;
    }

    if restore_clipboard_after_paste {
        schedule_clipboard_restore(restore_plan);
    }
    // 关掉 → 听写文本留在剪贴板里，simulate_paste 没真正落地时用户能手动 Ctrl+V 找回。
    insertion_success_status()
}

#[cfg(not(target_os = "macos"))]
fn schedule_clipboard_restore(plan: ClipboardRestorePlan) {
    let restore_id = NEXT_CLIPBOARD_RESTORE_ID.fetch_add(1, Ordering::SeqCst);
    let original_text = {
        let mut pending = PENDING_CLIPBOARD_RESTORE.lock();
        let original = pending
            .as_ref()
            .map(|batch| batch.original_text.clone())
            .unwrap_or_else(|| plan.previous_text.clone());
        *pending = Some(PendingClipboardRestore {
            latest_restore_id: restore_id,
            original_text: original.clone(),
        });
        original
    };
    std::thread::spawn(move || {
        restore_clipboard_after_delay(plan, original_text, restore_id, CLIPBOARD_RESTORE_DELAY)
    });
}

#[cfg(not(target_os = "macos"))]
fn restore_clipboard_after_delay(
    plan: ClipboardRestorePlan,
    original_text: Option<String>,
    restore_id: u64,
    delay: Duration,
) {
    std::thread::sleep(delay);

    if !is_latest_clipboard_restore(restore_id) {
        return;
    }

    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(err) => {
            log::warn!(
                "[insertion] clipboard re-open failed during restore: {}",
                err
            );
            clear_pending_clipboard_restore(restore_id);
            return;
        }
    };

    let current_text = match clipboard.get_text() {
        Ok(current) => Some(current),
        Err(err) => {
            log::warn!(
                "[insertion] clipboard get_text failed during restore: {}",
                err
            );
            None
        }
    };

    if should_restore_clipboard(current_text.as_deref(), &plan.inserted_text) {
        if let Some(previous_text) = original_text {
            if let Err(err) = clipboard.set_text(previous_text) {
                log::warn!("[insertion] clipboard restore failed: {}", err);
            }
        }
    } else {
        log::info!(
            "[insertion] skip clipboard restore: latest clipboard no longer matches inserted text"
        );
    }

    clear_pending_clipboard_restore(restore_id);
}

#[cfg(not(target_os = "macos"))]
fn is_latest_clipboard_restore(restore_id: u64) -> bool {
    matches!(
        PENDING_CLIPBOARD_RESTORE.lock().as_ref(),
        Some(batch) if batch.latest_restore_id == restore_id
    )
}

#[cfg(not(target_os = "macos"))]
fn clear_pending_clipboard_restore(restore_id: u64) {
    let mut pending = PENDING_CLIPBOARD_RESTORE.lock();
    if matches!(pending.as_ref(), Some(batch) if batch.latest_restore_id == restore_id) {
        pending.take();
    }
}

#[cfg(not(target_os = "macos"))]
fn should_restore_clipboard(current_text: Option<&str>, inserted_text: &str) -> bool {
    matches!(current_text, Some(current) if current == inserted_text)
}

#[cfg(target_os = "macos")]
fn simulate_paste() -> Result<(), String> {
    if !matches!(
        crate::permissions::check_accessibility(),
        crate::permissions::PermissionStatus::Granted
    ) {
        return Err("accessibility permission is not granted".into());
    }
    macos::post_cmd_v()
}

#[cfg(not(target_os = "macos"))]
fn simulate_paste() -> Result<(), String> {
    // Synthesize Ctrl+V (Cmd+V on macOS is handled separately above).
    // Note: Ctrl+V as a *keyboard accelerator* does NOT compete with IME
    // composition state — the IME treats it as a shortcut, not as text input.
    // The IME-vs-text-injection bug specifically affects KEYEVENTF_UNICODE
    // SendInput in `insert_via_unicode_keystrokes`, which we route through
    // the clipboard path instead. So this Ctrl+V path is safe to keep.
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    let modifier = Key::Control;
    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| e.to_string())?;
    let press_v = enigo.key(Key::Unicode('v'), Direction::Click);
    let release_modifier = enigo.key(modifier, Direction::Release);
    if let Err(e) = release_modifier {
        return Err(e.to_string());
    }
    press_v.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn insertion_success_status() -> InsertStatus {
    InsertStatus::Inserted
}

#[cfg(not(target_os = "macos"))]
fn insertion_success_status() -> InsertStatus {
    // Windows/Linux 的 Ctrl+V 只能证明粘贴快捷键已发送，不能证明目标控件已接收。
    InsertStatus::PasteSent
}

#[cfg(target_os = "windows")]
mod windows_unicode {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };

    pub fn send_text(text: &str) -> Result<(), String> {
        for unit in text.encode_utf16() {
            send_utf16_unit(unit, false)?;
            send_utf16_unit(unit, true)?;
        }
        Ok(())
    }

    fn send_utf16_unit(unit: u16, key_up: bool) -> Result<(), String> {
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
            Err(std::io::Error::last_os_error().to_string())
        }
    }
}

// ─────────────────────────── macOS native CGEvent paste ───────────────────────────

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
    /// Virtual keycode for "V" on US/ANSI layouts (kVK_ANSI_V).
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

    /// 与 Swift `TextInserter.simulatePaste()` 同源:
    ///   下 V + 加 Cmd flag → post → 上 V + 加 Cmd flag → post
    /// 全部走 C 层 CGEvent，不会触发 enigo 那条 TSM 主线程断言路径。
    pub fn post_cmd_v() -> Result<(), String> {
        unsafe {
            let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            // 即使 source 是空也能 post（Apple 文档允许 NULL source），所以不当致命错误。
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
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn restore_only_when_clipboard_still_holds_inserted_text() {
        assert!(should_restore_clipboard(
            Some("dictated text"),
            "dictated text"
        ));
        assert!(!should_restore_clipboard(
            Some("user changed clipboard"),
            "dictated text"
        ));
        assert!(!should_restore_clipboard(None, "dictated text"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn delayed_terminal_paste_must_see_dictated_text_before_clipboard_restore() {
        let inserted_text = "dictated text".to_string();
        let previous_text = "older clipboard".to_string();
        let clipboard = Arc::new(Mutex::new(inserted_text.clone()));
        let pasted = Arc::new(Mutex::new(None::<String>));

        let clipboard_for_paste = Arc::clone(&clipboard);
        let pasted_for_paste = Arc::clone(&pasted);
        let reader = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            let seen = clipboard_for_paste.lock().unwrap().clone();
            *pasted_for_paste.lock().unwrap() = Some(seen);
        });

        thread::sleep(CLIPBOARD_RESTORE_DELAY);
        let current_text = Some(clipboard.lock().unwrap().clone());
        if should_restore_clipboard(current_text.as_deref(), &inserted_text) {
            *clipboard.lock().unwrap() = previous_text;
        }

        reader.join().unwrap();

        assert_eq!(
            pasted.lock().unwrap().as_deref(),
            Some(inserted_text.as_str())
        );
    }
}
