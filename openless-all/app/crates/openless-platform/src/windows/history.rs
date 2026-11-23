//! Windows 剪贴板历史清理。
//!
//! 我们自己对剪贴板的写入都带了 `CanIncludeInClipboardHistory=0`，正常不会进历史。
//! 但抓选区时是**目标应用**响应 Ctrl+C 写剪贴板，标记加不到它身上，只能事后删掉
//! （`Clipboard.GetHistoryItemsAsync` + `DeleteItemFromHistory`）。
//!
//! 安全约束，任何一条不满足都放弃清理：
//! 1. 只检查**最新一条**历史，更早的条目不是这次操作产生的；
//! 2. 内容必须与预期文本**完全相等**；
//! 3. 只在短暂窗口内尝试——历史服务是异步接收剪贴板变化的，条目可能延迟出现，
//!    但窗口之外出现的条目大概率已经和用户的新复制混淆；
//! 4. 历史未启用 / 访问被拒 / 任何 WinRT 失败都静默放弃，绝不影响插入结果。

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::ApplicationModel::DataTransfer::{
    Clipboard, ClipboardHistoryItem, ClipboardHistoryItemsResultStatus,
};
use windows::Foundation::{AsyncStatus, IAsyncInfo, IAsyncOperation};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

/// 历史服务接收剪贴板变化有延迟，在这个窗口内反复查询。
const PURGE_WINDOW: Duration = Duration::from_millis(1500);
const PURGE_STEP: Duration = Duration::from_millis(60);
/// 单次异步调用（取历史列表 / 取条目文本）的最长等待。
const OPERATION_TIMEOUT: Duration = Duration::from_millis(1000);

/// 删除"最新一条且内容等于 `text`"的历史条目。
///
/// 返回是否真的删掉了一条。调用方应放在后台线程里，别挡插入路径。
pub fn purge_newest_matching(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let _com = WinRtGuard::enter();
    match Clipboard::IsHistoryEnabled() {
        Ok(true) => {}
        Ok(false) => {
            log::debug!("[clipboard-history] history disabled; nothing to purge");
            return false;
        }
        Err(error) => {
            log::debug!("[clipboard-history] IsHistoryEnabled failed: {error}");
            return false;
        }
    }

    let deadline = Instant::now() + PURGE_WINDOW;
    loop {
        match newest_history_entry() {
            Some((item, entry_text)) if entry_text == text => {
                let deleted = Clipboard::DeleteItemFromHistory(&item).unwrap_or(false);
                if deleted {
                    log::info!(
                        "[clipboard-history] purged newest matching entry ({} chars)",
                        text.chars().count()
                    );
                } else {
                    log::warn!("[clipboard-history] DeleteItemFromHistory returned false");
                }
                return deleted;
            }
            Some((_, entry_text)) => {
                // 最新一条已经不是我们要清理的内容：用户在这期间复制了别的东西，
                // 继续找下去就有误删风险，直接放弃。
                log::debug!(
                    "[clipboard-history] newest entry is not ours ({} chars); skip purge",
                    entry_text.chars().count()
                );
                return false;
            }
            None => {}
        }

        if Instant::now() >= deadline {
            log::debug!(
                "[clipboard-history] no matching entry within {}ms; nothing to purge",
                PURGE_WINDOW.as_millis()
            );
            return false;
        }
        std::thread::sleep(PURGE_STEP);
    }
}

/// 读最新一条历史条目的文本。没有历史 / 访问被拒 / 无文本时为 `None`。
fn newest_history_entry() -> Option<(ClipboardHistoryItem, String)> {
    let operation = Clipboard::GetHistoryItemsAsync().ok()?;
    let result = wait_for(&operation)?;
    if result.Status().ok()? != ClipboardHistoryItemsResultStatus::Success {
        return None;
    }
    let items = result.Items().ok()?;
    if items.Size().ok()? == 0 {
        return None;
    }
    let item = items.GetAt(0).ok()?;
    let content = item.Content().ok()?;
    let text_operation = content.GetTextAsync().ok()?;
    let text = wait_for(&text_operation)?;
    Some((item, text.to_string()))
}

/// 同步等待 WinRT 异步操作。超时、出错、取消都当作失败。
fn wait_for<T: windows::core::RuntimeType + 'static>(operation: &IAsyncOperation<T>) -> Option<T> {
    let info: IAsyncInfo = operation.cast().ok()?;
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    loop {
        let status = info.Status().ok()?;
        if status == AsyncStatus::Completed {
            return operation.GetResults().ok();
        }
        if status == AsyncStatus::Error || status == AsyncStatus::Canceled {
            return None;
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 进入 WinRT。清理跑在临时后台线程上，需要自己初始化 MTA。
struct WinRtGuard {
    needs_uninitialize: bool,
}

impl WinRtGuard {
    fn enter() -> Self {
        // `RPC_E_CHANGED_MODE` 表示线程已经用别的套间模式初始化过 WinRT，
        // 此时依然可调用 WinRT，只是不能再 Uninitialize。
        let needs_uninitialize = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
        Self {
            needs_uninitialize,
        }
    }
}

impl Drop for WinRtGuard {
    fn drop(&mut self) {
        if self.needs_uninitialize {
            unsafe { RoUninitialize() }
        }
    }
}
