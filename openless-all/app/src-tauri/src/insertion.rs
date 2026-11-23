//! 跨平台光标位置文本插入。
//!
//! 通用步骤：先写剪贴板（模拟失败时用户能手动粘贴）→ 模拟粘贴快捷键。
//! - macOS：用 CoreGraphics CGEvent 直接 post Cmd+V。
//! - Windows：用平台层的 SendInput 按 `PasteShortcut` 模拟。

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::time::Duration;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use once_cell::sync::Lazy;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use parking_lot::Mutex;

use crate::types::{InsertStatus, PasteShortcut};

/// 桌面端的平台能力入口；移动端走各自的 JNI 路径。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use openless_platform::clipboard::{
    Clipboard as PlatformClipboard, Intent as ClipboardIntent, Snapshot as ClipboardSnapshot,
};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(750);

/// Put a text into the clipboard; used by the "Copy" button on the insertion-failure fallback card.
///
/// A separate entry instead of letting the frontend call `navigator.clipboard`: the card floats
/// above another app and its button does not take focus, and in an unfocused document that API
/// throws `Document is not focused`.
pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    if copy_to_clipboard(text) {
        Ok(())
    } else {
        Err("clipboard write failed".to_string())
    }
}

/// 一次插入的可选项，避免每个调用点各自拼参数。
#[derive(Debug, Clone, Copy)]
pub struct InsertOptions {
    /// "插入后恢复剪贴板"开关。
    ///
    /// 同一个开关同时决定成功和失败时的行为，不额外增加设置项：
    /// - 开：粘贴成功或失败都还原用户原剪贴板，失败时返回 `Failed`；
    /// - 关：不接管剪贴板，听写文本留在那里，粘贴失败时返回 `CopiedFallback`。
    pub restore_clipboard_after_paste: bool,
    /// 模拟按下的粘贴快捷键（终端可能要 Ctrl+Shift+V）。
    pub paste_shortcut: PasteShortcut,
}

pub struct TextInserter;

impl TextInserter {
    pub fn new() -> Self {
        Self
    }

    /// Windows 路径：写剪贴板 + 模拟 `options.paste_shortcut`。
    #[cfg(target_os = "windows")]
    pub fn insert(&self, text: &str, options: InsertOptions) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        insert_with_clipboard_restore(text, options)
    }

    #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
    pub fn insert_via_clipboard_fallback(&self, text: &str, options: InsertOptions) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        insert_with_clipboard_restore(text, options)
    }

    #[cfg(target_os = "windows")]
    pub fn insert_via_unicode_keystrokes(
        &self,
        text: &str,
        newline_mode: crate::types::WindowsSendInputNewlineMode,
    ) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        map_sendinput_type_result(text, openless_platform::text::type_text(text, newline_mode))
    }

    /// macOS path: save the original clipboard -> write the transcribed text -> post Cmd+V ->
    /// restore the original clipboard on demand.
    /// `options.paste_shortcut` is unused on macOS (fixed Cmd+V).
    #[cfg(target_os = "macos")]
    pub fn insert(&self, text: &str, options: InsertOptions) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        // issue #525: record the user's original clipboard first and restore it only after the
        // paste succeeds and the "restore clipboard" switch is on, so user-copied content is not
        // overwritten. macOS previously implemented no restore at all (the mechanism was
        // excluded by cfg), which made the settings switch a no-op on macOS — the clipboard was
        // left as the transcribed text regardless of the switch.
        let restore_plan = match copy_to_clipboard_with_restore_plan(text) {
            Ok(plan) => plan,
            Err(err) => {
                log::error!("[insertion] clipboard write failed: {}", err);
                return InsertStatus::Failed;
            }
        };
        if let Err(err) = simulate_paste() {
            log::warn!("[insertion] simulated paste failed: {}", err);
            if !options.restore_clipboard_after_paste {
                // 开关关闭：不接管剪贴板，文本留在这里供手动 Cmd+V。
                return InsertStatus::CopiedFallback;
            }
            restore_previous_clipboard(&restore_plan.previous);
            return InsertStatus::Failed;
        }
        if options.restore_clipboard_after_paste {
            schedule_clipboard_restore(restore_plan);
        }
        insertion_success_status()
    }

    /// Android: cross-app input is handled by the dictation flow per user policy; generic
    /// insertion only writes the clipboard as a fallback.
    #[cfg(target_os = "android")]
    pub fn insert(&self, text: &str, _options: InsertOptions) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        self.copy_fallback(text)
    }

    /// 无原生粘贴快捷键实现的桌面构建：退回剪贴板兜底，与
    /// [`Self::insert_via_clipboard_fallback`] 同语义（典型场景：终端/无合成键平台）。
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    pub fn insert(&self, text: &str, options: InsertOptions) -> InsertStatus {
        self.insert_via_clipboard_fallback(text, options)
    }

    /// 只写剪贴板、不模拟粘贴。用于目标控件活跃状态无法验证时的兜底路径。
    pub fn copy_fallback(&self, text: &str) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        // 兜底文本同样是内部写：用户要的是"能用 Ctrl+V 粘上"，历史里留一条没有意义。
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let written = write_clipboard(text, ClipboardIntent::Internal);
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let written = copy_to_clipboard(text);
        if written {
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
    result: Result<usize, openless_platform::text::TextError>,
) -> InsertStatus {
    let expected = openless_platform::text::expected_typed_chars(text);
    match result {
        Ok(typed_chars) if typed_chars == expected => InsertStatus::Inserted,
        Ok(typed_chars) => {
            log::warn!("[insertion] Unicode SendInput typed only {typed_chars}/{expected} chars");
            InsertStatus::CopiedFallback
        }
        Err(err) => {
            log::warn!("[insertion] Unicode SendInput failed: {err}");
            InsertStatus::CopiedFallback
        }
    }
}

/// 插入前用户剪贴板的样子。
///
/// 桌面端统一保存平台层给出的快照：Windows 是全格式，macOS 目前是纯文本。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Clone, Default)]
struct PreviousClipboard {
    /// 快照里的纯文本，仅用于诊断日志与测试断言。
    text: Option<String>,
    snapshot: Option<std::sync::Arc<ClipboardSnapshot>>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug)]
struct ClipboardRestorePlan {
    inserted_text: String,
    previous: PreviousClipboard,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Clone, Default)]
struct PendingClipboardRestore {
    latest_restore_id: u64,
    original: PreviousClipboard,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
static NEXT_CLIPBOARD_RESTORE_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(not(any(target_os = "android", target_os = "ios")))]
static PENDING_CLIPBOARD_RESTORE: Lazy<Mutex<Option<PendingClipboardRestore>>> =
    Lazy::new(|| Mutex::new(None));

/// 桌面端写剪贴板的唯一入口。
///
/// 统一走 `openless-platform`：由平台层决定历史排除等细节，宿主不再直接依赖
/// `arboard` 或 Win32 剪贴板 API。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn write_clipboard(text: &str, intent: ClipboardIntent) -> bool {
    if let Err(error) = PlatformClipboard::set_text(text, intent) {
        log::error!("[insertion] clipboard write failed: {error}");
        return false;
    }
    true
}

/// 用户可见的复制（失败卡"复制"按钮）。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn copy_to_clipboard(text: &str) -> bool {
    write_clipboard(text, ClipboardIntent::UserVisible)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn copy_to_clipboard(text: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        return crate::android::jni::with_android_env(|env, context| {
            crate::android::jni::copy_to_clipboard(env, context, text)
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
    // 先整份备份：打开失败就直接放弃，绝不把它当成空剪贴板，
    // 否则模拟粘贴会把用户内容彻底覆盖掉。
    let snapshot = PlatformClipboard::snapshot().map_err(|error| error.to_string())?;
    let previous = PreviousClipboard {
        text: snapshot.text(),
        snapshot: Some(std::sync::Arc::new(snapshot)),
    };
    PlatformClipboard::set_text(text, ClipboardIntent::Internal)
        .map_err(|error| error.to_string())?;
    Ok(ClipboardRestorePlan {
        inserted_text: text.to_string(),
        previous,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn insert_with_clipboard_restore(text: &str, options: InsertOptions) -> InsertStatus {
    let restore_plan = match copy_to_clipboard_with_restore_plan(text) {
        Ok(plan) => plan,
        Err(err) => {
            log::error!("[insertion] clipboard write failed: {}", err);
            return InsertStatus::Failed;
        }
    };

    if let Err(err) = simulate_paste(options.paste_shortcut) {
        log::warn!("[insertion] simulated paste failed: {}", err);
        if !options.restore_clipboard_after_paste {
            // 开关关闭：不接管剪贴板，文本留在这里供手动粘贴。
            return InsertStatus::CopiedFallback;
        }
        // 开关开启：失败也还原，不留残留。
        restore_previous_clipboard(&restore_plan.previous);
        schedule_history_purge(&restore_plan.inserted_text);
        return InsertStatus::Failed;
    }

    if options.restore_clipboard_after_paste {
        schedule_clipboard_restore(restore_plan);
    }
    insertion_success_status()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn schedule_clipboard_restore(plan: ClipboardRestorePlan) {
    let (restore_id, original) = remember_pending_clipboard_restore(plan.previous.clone());
    std::thread::spawn(move || {
        restore_clipboard_after_delay(plan, original, restore_id, CLIPBOARD_RESTORE_DELAY)
    });
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn remember_pending_clipboard_restore(previous: PreviousClipboard) -> (u64, PreviousClipboard) {
    let restore_id = NEXT_CLIPBOARD_RESTORE_ID.fetch_add(1, Ordering::SeqCst);
    let original = {
        let mut pending = PENDING_CLIPBOARD_RESTORE.lock();
        // 连续多次听写时，保留最早那份"用户原剪贴板"，不要被上一次的听写文本覆盖。
        let original = pending
            .as_ref()
            .map(|batch| batch.original.clone())
            .unwrap_or(previous);
        *pending = Some(PendingClipboardRestore {
            latest_restore_id: restore_id,
            original: original.clone(),
        });
        original
    };
    (restore_id, original)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn restore_clipboard_after_delay(
    plan: ClipboardRestorePlan,
    original: PreviousClipboard,
    restore_id: u64,
    delay: Duration,
) {
    std::thread::sleep(delay);

    if !is_latest_clipboard_restore(restore_id) {
        return;
    }

    let current_text = current_clipboard_text();
    if should_restore_clipboard(current_text.as_deref(), &plan.inserted_text) {
        restore_previous_clipboard(&original);
        // 我们的写入带了历史排除标记；万一系统/旧版本没遵守，补一刀删除。
        schedule_history_purge(&plan.inserted_text);
    } else {
        log::info!(
            "[insertion] skip clipboard restore: latest clipboard no longer matches inserted text"
        );
    }

    clear_pending_clipboard_restore(restore_id);
}

/// 后台清理这次写入留下的历史条目。
///
/// 正常情况下排除标记已经让它没进历史，这里只是兜底；失败不影响插入结果。
#[cfg(target_os = "windows")]
fn schedule_history_purge(text: &str) {
    if text.is_empty() {
        return;
    }
    let text = text.to_string();
    std::thread::spawn(move || {
        openless_platform::clipboard::history::purge_newest_matching(&text);
    });
}

#[cfg(not(target_os = "windows"))]
fn schedule_history_purge(_text: &str) {}

/// 当前剪贴板文本，读不到时返回 `None`（还原阶段没有"必须中止"的语义）。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn current_clipboard_text() -> Option<String> {
    match PlatformClipboard::read_text() {
        Ok(text) => text,
        Err(error) => {
            log::warn!("[insertion] clipboard get_text failed during restore: {error}");
            None
        }
    }
}

/// 用快照还原用户原来的剪贴板。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn restore_previous_clipboard(previous: &PreviousClipboard) {
    let Some(snapshot) = previous.snapshot.as_ref() else {
        return;
    };
    log::debug!(
        "[insertion] restoring clipboard: {} formats, previous_text_chars={}",
        snapshot.format_count_for_log(),
        previous
            .text
            .as_deref()
            .map(|text| text.chars().count())
            .unwrap_or(0)
    );
    if let Err(error) = PlatformClipboard::restore(snapshot) {
        log::warn!("[insertion] clipboard restore failed: {error}");
    }
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
    // 前置校验（可访问性授权 / Secure Input）留在宿主，因为它是权限与安全策略；
    // 真正投递 Cmd+V 由平台层完成。
    simulate_macos_paste_with(
        matches!(
            crate::permissions::check_accessibility(),
            crate::permissions::PermissionStatus::Granted
        ),
        crate::unicode_keystroke::is_secure_input_enabled(),
        || {
            openless_platform::text::send_paste_shortcut(crate::types::PasteShortcut::CtrlV)
                .map_err(|error| error.to_string())
        },
    )
}

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn simulate_paste(shortcut: PasteShortcut) -> Result<(), String> {
    openless_platform::text::send_paste_shortcut(shortcut).map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn insertion_success_status() -> InsertStatus {
    InsertStatus::Inserted
}

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn insertion_success_status() -> InsertStatus {
    InsertStatus::PasteSent
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

    #[test]
    #[cfg(target_os = "windows")]
    fn crlf_sendinput_success_uses_expected_typed_count() {
        // Includes the safely swallowed CR: the receipt reports the consumed source prefix,
        // not the number of keystrokes sent.
        let text = "a\r\nb";
        let expected = openless_platform::text::expected_typed_chars(text);
        let status = map_sendinput_type_result(text, Ok(expected));
        assert_eq!(status, InsertStatus::Inserted);
        assert_eq!(expected, 4);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn crlf_sendinput_partial_mismatch_falls_back_to_clipboard() {
        // Only a + CR + LF were consumed; the trailing b was not, so the partial-failure
        // semantics must hold.
        let text = "a\r\nb";
        let expected = openless_platform::text::expected_typed_chars(text);
        let mismatched = 3;
        assert_ne!(mismatched, expected);
        let status = map_sendinput_type_result(text, Ok(mismatched));
        assert_eq!(status, InsertStatus::CopiedFallback);
    }

    #[test]
    fn empty_insertions_never_touch_clipboard_or_paste_path() {
        let inserter = TextInserter::new();

        let options = InsertOptions {
            restore_clipboard_after_paste: true,
            paste_shortcut: PasteShortcut::CtrlV,
        };
        assert_eq!(
            inserter.insert("", options),
            InsertStatus::CopiedFallback
        );
        #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
        {
            assert_eq!(
                inserter.insert_via_clipboard_fallback("", options),
                InsertStatus::CopiedFallback
            );
        }
        assert_eq!(inserter.copy_fallback(""), InsertStatus::CopiedFallback);
    }

    #[test]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn pending_clipboard_restore_keeps_first_original_until_latest_restore() {
        *PENDING_CLIPBOARD_RESTORE.lock() = None;

        fn previous(text: &str) -> PreviousClipboard {
            PreviousClipboard {
                text: Some(text.to_string()),
                ..PreviousClipboard::default()
            }
        }

        let (first_id, first_original) =
            remember_pending_clipboard_restore(previous("user clipboard"));
        let (second_id, second_original) =
            remember_pending_clipboard_restore(previous("first dictated text"));

        assert_ne!(first_id, second_id);
        assert_eq!(first_original.text.as_deref(), Some("user clipboard"));
        assert_eq!(second_original.text.as_deref(), Some("user clipboard"));
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
        // Paste success -> Inserted; restore runs only while the clipboard still holds the
        // just-inserted text (issue #525), i.e. the "restore clipboard" switch really works on
        // macOS.
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
