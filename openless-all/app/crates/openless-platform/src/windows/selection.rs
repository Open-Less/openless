//! Windows 选区捕获：模拟 Ctrl+C + 剪贴板事务。
//!
//! 这是"最后手段"的读取通道：目标应用响应 Ctrl+C 把选区写进剪贴板，我们读走再把
//! 剪贴板整体还原。因此它必须保证三件事：
//!
//! 1. **备份失败就不复制**：剪贴板打不开时直接放弃，绝不把它当空剪贴板；
//! 2. **按序列号判断有没有复制成功**：不写哨兵，避免污染剪贴板历史（#1331/#1335）；
//! 3. **还原并清理历史**：还原整份快照；目标应用写进历史的那一条事后删掉。

use std::time::{Duration, Instant};

use crate::clipboard::{Clipboard, ClipboardError};
use crate::selection::CaptureOutcome;
use crate::windows::input;

/// 等待 Ctrl+C 落地的上限；一次复制通常几十毫秒。
const MAX_WAIT_MS: u64 = 450;
const STEP_MS: u64 = 25;
/// 中途重发一次 Ctrl+C 的时机：第一次可能撞上正忙的剪贴板。
const RESEND_AFTER_MS: u64 = 150;

pub fn capture_via_simulated_copy() -> CaptureOutcome {
    // 1) 备份 + 记序列号。打开失败必须中止。
    let snapshot = match Clipboard::snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            log::warn!("[selection] clipboard backup failed; skipping simulated copy: {error}");
            return match error {
                ClipboardError::Busy => CaptureOutcome::ClipboardBusy,
                ClipboardError::Write(_) => CaptureOutcome::Unreadable,
            };
        }
    };
    let baseline = Clipboard::sequence_number();

    // 2) 发 Ctrl+C，然后轮询序列号。
    let mut posted = input::send_copy_shortcut().is_ok();
    if !posted {
        log::warn!("[selection] post_copy_shortcut failed");
    }
    let outcome = poll_after_copy(baseline, &mut posted);

    // 3) 只要剪贴板确实变过，就还原整份快照（序列号没动说明没人写过，无需还原）。
    if !matches!(outcome, CaptureOutcome::Unchanged) {
        if let Err(error) = Clipboard::restore(&snapshot) {
            log::warn!("[selection] clipboard restore failed: {error}");
        }
    }

    // 4) 删掉目标应用这次复制写进历史的那一条（我们加不了排除标记，只能事后删）。
    if let CaptureOutcome::Captured(text) = &outcome {
        if !text.is_empty() {
            let captured = text.clone();
            std::thread::spawn(move || {
                crate::clipboard::history::purge_newest_matching(&captured);
            });
        }
    }

    if !posted {
        return CaptureOutcome::CopyFailed;
    }
    outcome
}

/// 轮询剪贴板序列号，直到读到文本或超时。
fn poll_after_copy(baseline: u32, posted: &mut bool) -> CaptureOutcome {
    let start = Instant::now();
    let mut attempts = 0u32;
    let mut resent = false;
    let mut changed = false;

    loop {
        attempts += 1;
        if Clipboard::sequence_number() != baseline {
            changed = true;
            // 剪贴板可能被历史服务/云剪贴板短暂持有，继续等到超时而不是立刻失败。
            if let Ok(Some(text)) = Clipboard::read_text() {
                log::info!(
                    "[selection] clipboard_poll: waited_ms={} attempts={} resent={} changed=true",
                    start.elapsed().as_millis(),
                    attempts,
                    resent
                );
                return CaptureOutcome::Captured(text);
            }
        }

        if start.elapsed().as_millis() as u64 >= MAX_WAIT_MS {
            break;
        }
        if !resent && start.elapsed().as_millis() as u64 >= RESEND_AFTER_MS {
            if input::send_copy_shortcut().is_ok() {
                *posted = true;
                resent = true;
                log::info!("[selection] clipboard_poll: resent Ctrl+C after sequence miss");
            }
        }
        std::thread::sleep(Duration::from_millis(STEP_MS));
    }

    log::info!(
        "[selection] clipboard_poll: waited_ms={} attempts={} resent={} changed={}",
        start.elapsed().as_millis(),
        attempts,
        resent,
        changed
    );
    if changed {
        CaptureOutcome::Unreadable
    } else {
        CaptureOutcome::Unchanged
    }
}

// ─────────────────── 零剪贴板选区读取（TSF → UIA） ───────────────────

/// 这次选区是从哪条通道读到的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionSource {
    /// 我们自己的 TSF 文本服务（`ITfContext::GetSelection`），完全不碰剪贴板。
    Tsf,
    /// UIA `TextPattern.GetSelection`，覆盖没激活 TSF 服务的应用。
    Uia,
}

impl SelectionSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tsf => "TSF",
            Self::Uia => "UIA",
        }
    }
}

/// 焦点选区及其来源。
#[derive(Debug, Clone)]
pub struct FocusedSelection {
    pub text: String,
    pub source: SelectionSource,
}

/// **不碰剪贴板**读取当前焦点选区：先 TSF，再 UIA。
///
/// 两条都失败时返回 `None`，由调用方决定是否回退到模拟 Ctrl+C
/// （那条路必然写剪贴板，恢复后还要清理历史）。
/// 返回的文本没有 trim，截断策略由调用方决定。
pub fn read_focused_selection() -> Option<FocusedSelection> {
    if let Some(text) = crate::ime::ipc::read_focused_selection_text() {
        if !text.trim().is_empty() {
            return Some(FocusedSelection {
                text,
                source: SelectionSource::Tsf,
            });
        }
        log::debug!("[selection] TSF selection read returned whitespace-only text");
    }

    if let Some(text) = crate::ime::uia::read_qa_selection() {
        if !text.trim().is_empty() {
            return Some(FocusedSelection {
                text,
                source: SelectionSource::Uia,
            });
        }
        log::debug!("[selection] UIA selection read returned whitespace-only text");
    }

    None
}
