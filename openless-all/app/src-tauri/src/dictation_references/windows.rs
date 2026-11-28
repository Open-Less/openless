//! Copy native bytes while the clipboard is open; release it before parsing / encoding / IO.
use super::{Content, MAX_BYTES, windows_formats};
use std::ffi::c_void;
type Handle = *mut c_void;
#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(window: Handle) -> i32;
    fn CloseClipboard() -> i32;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn GetClipboardData(format: u32) -> Handle;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GlobalLock(memory: Handle) -> *const u8;
    fn GlobalUnlock(memory: Handle) -> i32;
    fn GlobalSize(memory: Handle) -> usize;
    fn MultiByteToWideChar(page: u32, flags: u32, input: *const u8, len: i32, output: *mut u16, capacity: i32) -> i32;
}
struct Clipboard;
impl Drop for Clipboard { fn drop(&mut self) { unsafe { CloseClipboard(); } } }
unsafe fn bytes(format: u32) -> Result<Vec<u8>, &'static str> {
    let handle = GetClipboardData(format);
    if handle.is_null() { return Err("captureFailed"); }
    let size = GlobalSize(handle);
    if size as u64 > MAX_BYTES { return Err("tooLarge"); }
    if size == 0 { return Err("captureFailed"); }
    let data = GlobalLock(handle);
    if data.is_null() { return Err("captureFailed"); }
    let copied = std::slice::from_raw_parts(data, size).to_vec();
    GlobalUnlock(handle);
    Ok(copied)
}
// CF_HDROP may be ANSI. Convert its native ACP payload before using the shared UTF-16 parser.
fn wide_hdrop(mut bytes: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    if bytes.len() < 20 { return Err("captureFailed"); }
    if u32::from_le_bytes(bytes[16..20].try_into().unwrap()) != 0 { return Ok(bytes); }
    let offset = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    if offset < 20 || offset >= bytes.len() { return Err("captureFailed"); }
    let raw = &bytes[offset..];
    let count = unsafe { MultiByteToWideChar(0, 0, raw.as_ptr(), raw.len() as i32, std::ptr::null_mut(), 0) };
    if count <= 0 { return Err("captureFailed"); }
    if count as u64 * 2 + 20 > MAX_BYTES { return Err("tooLarge"); }
    let mut wide = vec![0u16; count as usize];
    if unsafe { MultiByteToWideChar(0, 0, raw.as_ptr(), raw.len() as i32, wide.as_mut_ptr(), count) } != count { return Err("captureFailed"); }
    bytes.truncate(20); bytes[..4].copy_from_slice(&20u32.to_le_bytes()); bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend(wide.into_iter().flat_map(u16::to_le_bytes));
    Ok(bytes)
}
pub(super) fn read() -> Result<Option<Content>, &'static str> {
    let png_name: Vec<u16> = "PNG\0".encode_utf16().collect();
    let png = unsafe { RegisterClipboardFormatW(png_name.as_ptr()) };
    let mut opened = false;
    for attempt in 0..3 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 { opened = true; break; }
        if attempt < 2 { std::thread::sleep(std::time::Duration::from_millis(10)); }
    }
    if !opened { return Err("captureFailed"); }
    let guard = Clipboard;
    // Prefer file references, then exact PNG / DIB, then Unicode text.
    let format = [15, png, 17, 8, 13].into_iter().find(|f| *f != 0 && unsafe { IsClipboardFormatAvailable(*f) } != 0);
    let Some(format) = format else { return Ok(None); };
    let data = unsafe { bytes(format) };
    drop(guard);
    let data = data?;
    Ok(Some(match format {
        15 => Content::Files(windows_formats::hdrop(&wide_hdrop(data)?)?),
        17 | 8 => Content::Image(windows_formats::dib_to_png(&data)?, "png"),
        13 => Content::Text(windows_formats::unicode_text(&data)?),
        _ => {
            if !data.starts_with(b"\x89PNG\r\n\x1a\n") { return Err("captureFailed"); }
            Content::Image(data, "png")
        }
    }))
}
