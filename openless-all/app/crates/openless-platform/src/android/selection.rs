//! Android 选区读取后端（经无障碍服务）。

use crate::selection::CaptureOutcome;

/// Android 直接问无障碍服务要选区，不需要模拟复制、也不碰剪贴板。
pub fn capture_via_simulated_copy() -> CaptureOutcome {
    match super::env() {
        Some(env) => match env.read_selection_via_accessibility() {
            Ok(Some(text)) if text.is_empty() => CaptureOutcome::Unchanged,
            Ok(Some(text)) => CaptureOutcome::Captured(text),
            Ok(None) => CaptureOutcome::Unchanged,
            Err(error) => {
                log::warn!("[selection] android accessibility read failed: {error}");
                CaptureOutcome::Unreadable
            }
        },
        None => CaptureOutcome::Unsupported,
    }
}
