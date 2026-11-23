//! Windows 剪贴板唯一入口。
//!
//! 所有 Windows 侧对剪贴板的读写都必须经过这里，原因有三个：
//!
//! 1. **历史污染**：`arboard` 只能写 `CF_UNICODETEXT`，没法同时附上
//!    `CanIncludeInClipboardHistory=0` 之类的注册格式，于是每次内部写剪贴板都会在
//!    Win+V 里留一条（#1331 / #1335）。这里的 [`set_text`] 带 `Intent::Internal`
//!    时会把这些标记一起放上去，Windows 剪贴板历史就不会记录。
//! 2. **内容降级**：只备份纯文本，还原时图片/富文本会被毁掉，空备份还会被还原成空串
//!    （#1370）。[`snapshot`] / [`restore`] 按 `EnumClipboardFormats` 逐格式搬运
//!    所有 HGLOBAL 内存格式。
//! 3. **事务性**：备份-还原必须整体成功或整体放弃。这里先分配好全部内存块再
//!    `EmptyClipboard`，避免中途失败把用户剪贴板清空。
//!
//! 注意：历史排除标记是"协作协议"，只影响 Windows 自己的历史/云同步监视器，
//! 不改变剪贴板内容本身——`Ctrl+V`、`GetClipboardData` 读到的仍是完整数据。

use std::sync::OnceLock;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::CF_UNICODETEXT;

use crate::common::codec as clipboard_codec;
use crate::clipboard::{ClipboardError, Intent};

#[derive(Debug)]
struct FormatBlock {
    format: u32,
    bytes: Vec<u8>,
}

/// 一次全格式剪贴板快照。
///
/// 只包含可以按字节搬运的 HGLOBAL 格式；GDI 句柄与 owner-display 格式记录在
/// `skipped` 里，用于排查"还原后少了什么"。
#[derive(Debug)]
pub struct Snapshot {
    blocks: Vec<FormatBlock>,
    skipped: Vec<u32>,
}

impl Snapshot {
    /// 快照当时的纯文本内容（`CF_UNICODETEXT`）。
    pub fn text(&self) -> Option<String> {
        let text_format = unicode_text_format();
        self.blocks
            .iter()
            .find(|block| block.format == text_format)
            .and_then(|block| clipboard_codec::decode_utf16_block(&block.bytes))
    }

    /// 快照里实际保存的格式数量（仅用于日志诊断）。
    pub fn format_count_for_log(&self) -> usize {
        self.blocks.len()
    }
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn unicode_text_format() -> u32 {
    CF_UNICODETEXT.0 as u32
}

/// 打开剪贴板。被别的进程短暂持有很常见（剪贴板历史服务、云剪贴板），
/// 所以重试几次再报 `Busy`。
fn open() -> Result<ClipboardGuard, ClipboardError> {
    const ATTEMPTS: u32 = 3;
    for attempt in 0..ATTEMPTS {
        if unsafe { OpenClipboard(HWND::default()) }.is_ok() {
            return Ok(ClipboardGuard);
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Err(ClipboardError::Busy)
}

unsafe fn free_global(hglobal: HGLOBAL) {
    if !hglobal.0.is_null() {
        let _ = unsafe { GlobalFree(hglobal) };
    }
}

/// 把字节写进一个新的 HGLOBAL。调用方负责把它交给 `SetClipboardData` 或自行释放。
unsafe fn copy_into_global(bytes: &[u8]) -> Result<HGLOBAL, ClipboardError> {
    let size = bytes.len().max(1);
    let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }
        .map_err(|_| ClipboardError::Write("GlobalAlloc"))?;
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        unsafe { free_global(hglobal) };
        return Err(ClipboardError::Write("GlobalLock"));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(hglobal);
    }
    Ok(hglobal)
}

fn register_format(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
}

fn internal_history_format() -> u32 {
    static FORMAT: OnceLock<u32> = OnceLock::new();
    *FORMAT.get_or_init(|| register_format("CanIncludeInClipboardHistory"))
}

fn internal_cloud_format() -> u32 {
    static FORMAT: OnceLock<u32> = OnceLock::new();
    *FORMAT.get_or_init(|| register_format("CanUploadToCloudClipboard"))
}

fn monitor_exclusion_format() -> u32 {
    static FORMAT: OnceLock<u32> = OnceLock::new();
    *FORMAT.get_or_init(|| register_format("ExcludeClipboardContentFromMonitorProcessing"))
}

/// 剪贴板当前的序列号。模拟 Ctrl+C 时用它判断目标应用是否真的写了剪贴板。
pub fn sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// 剪贴板里有没有文本格式。
pub fn has_text() -> bool {
    unsafe { IsClipboardFormatAvailable(unicode_text_format()) }.is_ok()
}

/// 读当前剪贴板文本。
///
/// `Ok(None)` 表示"没有文本格式"或"数据读不出来"，`Err(Busy)` 表示剪贴板打不开。
/// 需要区分这两种情况时（备份前）用 [`snapshot`]，它会把打不开当成错误。
pub fn read_text() -> Result<Option<String>, ClipboardError> {
    if !has_text() {
        return Ok(None);
    }
    let _guard = open()?;
    let handle = match unsafe { GetClipboardData(unicode_text_format()) } {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };
    if handle.0.is_null() {
        return Ok(None);
    }
    let hglobal = HGLOBAL(handle.0);
    let size = unsafe { GlobalSize(hglobal) };
    if size < 2 {
        return Ok(None);
    }
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return Ok(None);
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();
    unsafe {
        let _ = GlobalUnlock(hglobal);
    }
    Ok(clipboard_codec::decode_utf16_block(&bytes))
}

/// 备份剪贴板的全部内存格式。
///
/// 打开失败返回 `Err(Busy)`——调用方必须把它当成"不能继续"，绝不能当作空剪贴板，
/// 否则一次模拟复制就会把用户内容彻底覆盖掉。
pub fn snapshot() -> Result<Snapshot, ClipboardError> {
    let _guard = open()?;
    let mut blocks = Vec::new();
    let mut skipped = Vec::new();
    let mut format = 0u32;
    loop {
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if !clipboard_codec::is_memory_backed_format(format) {
            skipped.push(format);
            continue;
        }
        let handle = match unsafe { GetClipboardData(format) } {
            Ok(handle) if !handle.0.is_null() => handle,
            _ => {
                skipped.push(format);
                continue;
            }
        };
        let hglobal = HGLOBAL(handle.0);
        let size = unsafe { GlobalSize(hglobal) };
        if size == 0 {
            continue;
        }
        let ptr = unsafe { GlobalLock(hglobal) };
        if ptr.is_null() {
            skipped.push(format);
            continue;
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();
        unsafe {
            let _ = GlobalUnlock(hglobal);
        }
        blocks.push(FormatBlock { format, bytes });
    }
    if !skipped.is_empty() {
        log::debug!(
            "[clipboard] snapshot kept {} formats, skipped {} (GDI/owner-display)",
            blocks.len(),
            skipped.len()
        );
    }
    Ok(Snapshot { blocks, skipped })
}

/// 用快照整体还原剪贴板。
///
/// 先把所有内存块分配成功，再 `EmptyClipboard`，避免中途失败留下空剪贴板。
pub fn restore(snapshot: &Snapshot) -> Result<(), ClipboardError> {
    let mut prepared: Vec<(u32, HGLOBAL)> = Vec::with_capacity(snapshot.blocks.len());
    for block in &snapshot.blocks {
        match unsafe { copy_into_global(&block.bytes) } {
            Ok(hglobal) => prepared.push((block.format, hglobal)),
            Err(error) => {
                for (_, hglobal) in prepared {
                    unsafe { free_global(hglobal) };
                }
                return Err(error);
            }
        }
    }

    if !snapshot.skipped.is_empty() {
        log::warn!(
            "[clipboard] restore will drop {} non-byte-restorable formats (GDI/owner-display)",
            snapshot.skipped.len()
        );
    }

    let _guard = open()?;
    if unsafe { EmptyClipboard() }.is_err() {
        for (_, hglobal) in prepared {
            unsafe { free_global(hglobal) };
        }
        return Err(ClipboardError::Write("EmptyClipboard"));
    }
    for (format, hglobal) in prepared {
        if unsafe { SetClipboardData(format, HANDLE(hglobal.0)) }.is_err() {
            log::warn!("[clipboard] restore dropped format 0x{format:X}");
            unsafe { free_global(hglobal) };
        }
    }
    Ok(())
}

/// 写纯文本。
///
/// `Intent::Internal` 会同时放入三个历史/同步排除格式：
/// - `ExcludeClipboardContentFromMonitorProcessing`：任何数据即排除历史与云同步；
/// - `CanIncludeInClipboardHistory = 0`：不进本地历史；
/// - `CanUploadToCloudClipboard = 0`：不同步到其他设备。
///
/// 这些标记只影响历史记录，不影响粘贴读取。
pub fn set_text(text: &str, intent: Intent) -> Result<(), ClipboardError> {
    let mut prepared: Vec<(u32, HGLOBAL)> = Vec::new();
    if intent == Intent::Internal {
        let zero = 0u32.to_le_bytes();
        for format in [
            monitor_exclusion_format(),
            internal_history_format(),
            internal_cloud_format(),
        ] {
            match unsafe { copy_into_global(&zero) } {
                Ok(hglobal) => prepared.push((format, hglobal)),
                Err(error) => {
                    for (_, hglobal) in prepared {
                        unsafe { free_global(hglobal) };
                    }
                    return Err(error);
                }
            }
        }
    }

    let text_bytes = clipboard_codec::encode_utf16_block(text);
    let text_global = match unsafe { copy_into_global(&text_bytes) } {
        Ok(hglobal) => hglobal,
        Err(error) => {
            for (_, hglobal) in prepared {
                unsafe { free_global(hglobal) };
            }
            return Err(error);
        }
    };

    let _guard = open()?;
    if unsafe { EmptyClipboard() }.is_err() {
        for (_, hglobal) in prepared {
            unsafe { free_global(hglobal) };
        }
        unsafe { free_global(text_global) };
        return Err(ClipboardError::Write("EmptyClipboard"));
    }

    // 先放排除标记再放正文：万一下面失败，也只会留下一条被排除的空记录。
    for (format, hglobal) in prepared {
        if unsafe { SetClipboardData(format, HANDLE(hglobal.0)) }.is_err() {
            log::warn!("[clipboard] could not set exclusion format 0x{format:X}");
            unsafe { free_global(hglobal) };
        }
    }
    if unsafe { SetClipboardData(unicode_text_format(), HANDLE(text_global.0)) }.is_err() {
        unsafe { free_global(text_global) };
        return Err(ClipboardError::Write("SetClipboardData(CF_UNICODETEXT)"));
    }
    Ok(())
}
