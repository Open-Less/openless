#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! 跨平台光标位置文本插入。
//!
//! 通用步骤：先写剪贴板（模拟失败时用户能手动粘贴）→ 模拟粘贴快捷键。
//! - macOS：用 CoreGraphics CGEvent 直接 post Cmd+V。
//! - Windows / Linux：用 enigo 按 `PasteShortcut` 模拟。

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::time::Duration;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use once_cell::sync::Lazy;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use parking_lot::Mutex;

use crate::types::{InsertStatus, PasteShortcut};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(750);

pub struct TextInserter;

impl TextInserter {
    pub fn new() -> Self {
        Self
    }

    /// Linux 路径：优先走 fcitx5 CommitText；插件不可用或提交失败时回退剪贴板粘贴。
    #[cfg(target_os = "linux")]
    pub fn insert(
        &self,
        text: &str,
        restore_clipboard_after_paste: bool,
        paste_shortcut: PasteShortcut,
    ) -> InsertStatus {
        insert_with_fcitx_or_clipboard_fallback(
            text,
            restore_clipboard_after_paste,
            paste_shortcut,
            crate::linux_fcitx::commit_text,
            insert_with_clipboard_restore,
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

    /// macOS 路径：保存原剪贴板 → 写转写文字 → post Cmd+V → 按需恢复原剪贴板。
    /// `_paste_shortcut` 在 macOS 不使用（固定 Cmd+V），仅为对齐跨平台签名。
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
        // issue #525：先记下用户原剪贴板，粘贴成功且「恢复剪贴板」开关开启时再恢复，避免覆盖
        // 用户手动复制的内容。此前 macOS 完全不实现恢复（恢复机制曾被 cfg 排除），导致设置里
        // 的开关在 macOS 上无效——无论开关如何，剪贴板都被留成转写文字。
        let restore_plan = match copy_to_clipboard_with_restore_plan(text) {
            Ok(plan) => plan,
            Err(err) => {
                log::error!("[insertion] clipboard write failed: {}", err);
                return InsertStatus::Failed;
            }
        };
        if let Err(err) = simulate_paste() {
            log::warn!("[insertion] simulated paste failed: {}", err);
            // 粘贴失败：把转写文字留在剪贴板供用户手动粘贴，不恢复。
            return InsertStatus::CopiedFallback;
        }
        if restore_clipboard_after_paste {
            schedule_clipboard_restore(restore_plan);
        }
        insertion_success_status()
    }

    /// Android：跨应用输入由 dictation 流程按用户策略处理；通用插入只写剪贴板兜底。
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

    /// 只写剪贴板、不模拟粘贴。用于目标控件活跃状态无法验证时的兜底路径。
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

#[cfg(target_os = "linux")]
fn insert_with_fcitx_or_clipboard_fallback<C, F>(
    text: &str,
    restore_clipboard_after_paste: bool,
    paste_shortcut: PasteShortcut,
    commit_text: C,
    clipboard_fallback: F,
) -> InsertStatus
where
    C: FnOnce(&str) -> Result<(), String>,
    F: FnOnce(&str, bool, PasteShortcut) -> InsertStatus,
{
    if text.is_empty() {
        return InsertStatus::CopiedFallback;
    }
    match commit_text(text) {
        Ok(()) => InsertStatus::Inserted,
        Err(err) => {
            log::warn!(
                "[insertion] fcitx commit_text failed, falling back to clipboard paste: {err}"
            );
            clipboard_fallback(text, restore_clipboard_after_paste, paste_shortcut)
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
            log::warn!(
                "[insertion] Unicode SendInput typed only {typed_chars}/{expected} chars"
            );
            InsertStatus::CopiedFallback
        }
        Err(err) => {
            log::warn!("[insertion] Unicode SendInput failed: {err}");
            InsertStatus::CopiedFallback
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug)]
struct ClipboardRestorePlan {
    inserted_text: String,
    previous_text: Option<String>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Clone)]
struct PendingClipboardRestore {
    latest_restore_id: u64,
    original_text: Option<String>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
static NEXT_CLIPBOARD_RESTORE_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(not(any(target_os = "android", target_os = "ios")))]
static PENDING_CLIPBOARD_RESTORE: Lazy<Mutex<Option<PendingClipboardRestore>>> =
    Lazy::new(|| Mutex::new(None));

#[cfg(not(any(target_os = "android", target_os = "ios")))]
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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
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

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn insert_with_clipboard_restore(
    text: &str,
    restore_clipboard_after_paste: bool,
    paste_shortcut: PasteShortcut,
) -> InsertStatus {
    let restore_plan = match copy_to_clipboard_with_restore_plan(text) {
        Ok(plan) => plan,
        Err(err) => {
            log::error!("[insertion] clipboard write failed: {}", err);
            return InsertStatus::Failed;
        }
    };

    if let Err(err) = simulate_paste(paste_shortcut) {
        log::warn!("[insertion] simulated paste failed: {}", err);
        return InsertStatus::CopiedFallback;
    }

    if restore_clipboard_after_paste {
        schedule_clipboard_restore(restore_plan);
    }
    insertion_success_status()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn schedule_clipboard_restore(plan: ClipboardRestorePlan) {
    let (restore_id, original_text) =
        remember_pending_clipboard_restore(plan.previous_text.clone());
    std::thread::spawn(move || {
        restore_clipboard_after_delay(plan, original_text, restore_id, CLIPBOARD_RESTORE_DELAY)
    });
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn remember_pending_clipboard_restore(previous_text: Option<String>) -> (u64, Option<String>) {
    let restore_id = NEXT_CLIPBOARD_RESTORE_ID.fetch_add(1, Ordering::SeqCst);
    let original_text = {
        let mut pending = PENDING_CLIPBOARD_RESTORE.lock();
        let original = pending
            .as_ref()
            .map(|batch| batch.original_text.clone())
            .unwrap_or(previous_text);
        *pending = Some(PendingClipboardRestore {
            latest_restore_id: restore_id,
            original_text: original.clone(),
        });
        original
    };
    (restore_id, original_text)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn is_latest_clipboard_restore(restore_id: u64) -> bool {
    matches!(
        PENDING_CLIPBOARD_RESTORE.lock().as_ref(),
        Some(batch) if batch.latest_restore_id == restore_id
    )
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn clear_pending_clipboard_restore(restore_id: u64) {
    let mut pending = PENDING_CLIPBOARD_RESTORE.lock();
    if matches!(pending.as_ref(), Some(batch) if batch.latest_restore_id == restore_id) {
        pending.take();
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
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

/// 把 `PasteShortcut` 拆成 `(modifiers, primary)`，顺序决定按下/释放顺序。
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

    // 顺序：按 modifier → 点击主键 → 反向释放 modifier。
    // 任一步失败也把已按下的 modifier 反向释放回来，避免卡键。
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

#[cfg(target_os = "macos")]
fn insertion_success_status() -> InsertStatus {
    InsertStatus::Inserted
}

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn insertion_success_status() -> InsertStatus {
    InsertStatus::PasteSent
}

// ── macOS CGEvent paste ──
// 直接调 CoreGraphics FFI 发送 Cmd+V，避开 enigo 在主线程外触发的 TSM 断言。

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
    /// US/ANSI 键盘上 "V" 的虚拟键码（kVK_ANSI_V）。
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

    /// 模拟 Cmd+V：构造 V 的 down/up 事件，加 Cmd flag 后依次 post 到 HID 事件流。
    pub fn post_cmd_v() -> Result<(), String> {
        unsafe {
            let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            // source 为 NULL 时 post 仍合法（Apple 文档允许），不视为致命错误。
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
    #[cfg(target_os = "windows")]
    use std::sync::{Arc, Mutex};
    #[cfg(target_os = "windows")]
    use std::thread;
    #[cfg(target_os = "windows")]
    use std::time::Duration;

    #[test]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
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

    /// 配置的快捷键必须真实映射到对应按键。只比较 modifier 数 + 主键，规避 enigo 内部 PartialEq。
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
        // 期望值直接取自 expected_sendinput_typed_chars（同一分类真相），而非硬编码 3，
        // 这样即便 classify_sendinput_char 的规则合法变更，本用例仍表达「实际==期望→Inserted」。
        let text = "a\r\nb";
        let expected = crate::unicode_keystroke::expected_sendinput_typed_chars(text);
        let status = map_sendinput_type_result(text, Ok(expected));
        assert_eq!(status, InsertStatus::Inserted);
        // `\r` 被跳过，故期望值必然小于原始 char 数——这正是三处规则若漂移就会出错的点。
        assert_ne!(text.chars().count(), expected);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn crlf_sendinput_partial_mismatch_falls_back_to_clipboard() {
        // 实际发出的 typed char 数比期望多（把 `\r` 也算进去了）→ 视为部分失败 → 回落剪贴板。
        let text = "a\r\nb";
        let expected = crate::unicode_keystroke::expected_sendinput_typed_chars(text);
        let mismatched = text.chars().count();
        assert_ne!(mismatched, expected);
        let status = map_sendinput_type_result(text, Ok(mismatched));
        assert_eq!(status, InsertStatus::CopiedFallback);
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
    #[cfg(target_os = "linux")]
    fn linux_commit_text_success_skips_clipboard_fallback() {
        let mut fallback_called = false;

        let status = insert_with_fcitx_or_clipboard_fallback(
            "dictated text",
            true,
            PasteShortcut::CtrlV,
            |text| {
                assert_eq!(text, "dictated text");
                Ok(())
            },
            |_, _, _| {
                fallback_called = true;
                InsertStatus::CopiedFallback
            },
        );

        assert_eq!(status, InsertStatus::Inserted);
        assert!(!fallback_called);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_commit_text_failure_uses_clipboard_fallback() {
        let mut fallback_args = None;

        let status = insert_with_fcitx_or_clipboard_fallback(
            "dictated text",
            true,
            PasteShortcut::CtrlShiftV,
            |_| Err("plugin unavailable".to_string()),
            |text, restore_clipboard_after_paste, paste_shortcut| {
                fallback_args = Some((
                    text.to_string(),
                    restore_clipboard_after_paste,
                    paste_shortcut,
                ));
                InsertStatus::CopiedFallback
            },
        );

        assert_eq!(status, InsertStatus::CopiedFallback);
        assert_eq!(
            fallback_args,
            Some(("dictated text".to_string(), true, PasteShortcut::CtrlShiftV))
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_empty_insert_skips_commit_text_and_clipboard_fallback() {
        let mut commit_called = false;
        let mut fallback_called = false;

        let status = insert_with_fcitx_or_clipboard_fallback(
            "",
            true,
            PasteShortcut::CtrlV,
            |_| {
                commit_called = true;
                Ok(())
            },
            |_, _, _| {
                fallback_called = true;
                InsertStatus::CopiedFallback
            },
        );

        assert_eq!(status, InsertStatus::CopiedFallback);
        assert!(!commit_called);
        assert!(!fallback_called);
    }

    #[test]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn pending_clipboard_restore_keeps_first_original_until_latest_restore() {
        *PENDING_CLIPBOARD_RESTORE.lock() = None;

        let (first_id, first_original) =
            remember_pending_clipboard_restore(Some("user clipboard".to_string()));
        let (second_id, second_original) =
            remember_pending_clipboard_restore(Some("first dictated text".to_string()));

        assert_ne!(first_id, second_id);
        assert_eq!(first_original.as_deref(), Some("user clipboard"));
        assert_eq!(second_original.as_deref(), Some("user clipboard"));
        assert!(!is_latest_clipboard_restore(first_id));
        assert!(is_latest_clipboard_restore(second_id));

        clear_pending_clipboard_restore(first_id);
        assert!(is_latest_clipboard_restore(second_id));
        clear_pending_clipboard_restore(second_id);
        assert!(!is_latest_clipboard_restore(second_id));
    }

    #[test]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn clipboard_restore_skips_when_clipboard_no_longer_matches_inserted_text() {
        assert!(should_restore_clipboard(
            Some("dictated text"),
            "dictated text"
        ));
        assert!(!should_restore_clipboard(
            Some("user edited clipboard"),
            "dictated text"
        ));
        assert!(!should_restore_clipboard(None, "dictated text"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_paste_success_reports_inserted_and_guards_restore() {
        // 粘贴成功 → Inserted；恢复仅在剪贴板仍是刚插入的转写文字时进行（issue #525），
        // 即「恢复剪贴板」开关在 macOS 上真正生效。
        assert_eq!(insertion_success_status(), InsertStatus::Inserted);
        assert!(should_restore_clipboard(
            Some("dictated text"),
            "dictated text"
        ));
        assert!(!should_restore_clipboard(
            Some("user changed clipboard"),
            "dictated text"
        ));
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
