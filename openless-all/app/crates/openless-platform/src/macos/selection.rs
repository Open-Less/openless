//! macOS 选区捕获：哨兵 + Cmd+C。
//!
//! macOS 没有可查询的剪贴板序列号，所以用"先写一个哨兵"来判断 Cmd+C 有没有真的
//! 覆盖剪贴板：读回来还是哨兵，说明目标应用没有选区或 Cmd+C 没生效。

use std::time::Duration;

use crate::clipboard::{Clipboard, Intent};
use crate::selection::CaptureOutcome;

pub fn capture_via_simulated_copy() -> CaptureOutcome {
    // a) 备份原内容（只有纯文本通道；macOS 后端无法区分"无文本"和"读失败"）。
    let original = match Clipboard::read_text() {
        Ok(text) => text,
        Err(error) => {
            log::warn!("[selection] clipboard read failed: {error}");
            return CaptureOutcome::ClipboardBusy;
        }
    };

    // b) 写哨兵后再 Cmd+C：哨兵还在就说明没复制成功。
    let sentinel = format!("__openless_qa_sentinel_{}__", sentinel_token());
    if let Err(error) = Clipboard::set_text(&sentinel, Intent::Internal) {
        log::warn!("[selection] clipboard set_text(sentinel) failed: {error}");
        return CaptureOutcome::Unreadable;
    }

    let posted = crate::macos::cgevent::send_copy_shortcut().is_ok();
    log::info!(
        "[selection] post_copy: post_ok={posted} original_was_some={}",
        original.is_some()
    );
    if !posted {
        log::warn!("[selection] post_copy_shortcut failed");
    }

    std::thread::sleep(Duration::from_millis(80));
    let captured = Clipboard::read_text().ok().flatten();

    // c) 还原：原本没有文本就写空串，避免把哨兵留给用户。
    let restore_text = original.as_deref().unwrap_or("");
    if let Err(error) = Clipboard::set_text(restore_text, Intent::Internal) {
        log::warn!("[selection] clipboard restore failed: {error}");
    }

    if !posted {
        return CaptureOutcome::CopyFailed;
    }
    match captured {
        None => CaptureOutcome::Unreadable,
        Some(text) if text == sentinel => CaptureOutcome::Unchanged,
        Some(text) => CaptureOutcome::Captured(text),
    }
}

fn sentinel_token() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0)
}
