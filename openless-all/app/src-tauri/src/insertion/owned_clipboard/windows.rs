use super::*;
use std::ffi::c_void;
type Handle = *mut c_void;
#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(window: Handle) -> i32; fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32; fn SetClipboardData(format: u32, data: Handle) -> Handle;
    fn GetClipboardData(format: u32) -> Handle; fn GetClipboardSequenceNumber() -> u32;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
    fn CreateWindowExW(ex: u32, class: *const u16, name: *const u16, style: u32, x: i32, y: i32, width: i32, height: i32, parent: Handle, menu: Handle, instance: Handle, param: Handle) -> Handle;
    fn DestroyWindow(window: Handle) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle; fn GlobalFree(memory: Handle) -> Handle;
    fn GlobalLock(memory: Handle) -> *mut u8; fn GlobalUnlock(memory: Handle) -> i32; fn GlobalSize(memory: Handle) -> usize;
}
struct Clipboard;
struct Owner(Handle);
impl Drop for Owner { fn drop(&mut self) { unsafe { DestroyWindow(self.0); } } }
thread_local! { static OWNER: std::cell::RefCell<Option<Owner>> = const { std::cell::RefCell::new(None) }; }
fn owner() -> Result<Handle, String> {
    OWNER.with(|owner| {
        let mut owner = owner.borrow_mut();
        if owner.is_none() {
            let class = "STATIC\0".encode_utf16().collect::<Vec<_>>();
            let window = unsafe { CreateWindowExW(0, class.as_ptr(), std::ptr::null(), 0, 0, 0, 0, 0, -3isize as Handle, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
            if window.is_null() { return Err("clipboard owner window unavailable".into()); }
            *owner = Some(Owner(window));
        }
        Ok(owner.as_ref().unwrap().0)
    })
}
impl Clipboard {
    fn open() -> Result<Self, String> {
        let owner = owner()?;
        for attempt in 0..3 { if unsafe { OpenClipboard(owner) } != 0 { return Ok(Self); }
            if attempt < 2 { std::thread::sleep(std::time::Duration::from_millis(10)); } }
        Err("clipboard unavailable".into())
    }
}
impl Drop for Clipboard { fn drop(&mut self) { unsafe { CloseClipboard(); } } }
unsafe fn format(name: &str) -> u32 { let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>(); RegisterClipboardFormatW(name.as_ptr()) }
unsafe fn put(format: u32, bytes: &[u8]) -> Result<(), String> {
    let memory = GlobalAlloc(0x42, bytes.len().max(1)); // GMEM_MOVEABLE | GMEM_ZEROINIT.
    if memory.is_null() { return Err("clipboard allocation failed".into()); }
    let data = GlobalLock(memory);
    if data.is_null() { GlobalFree(memory); return Err("clipboard lock failed".into()); }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len()); GlobalUnlock(memory);
    if SetClipboardData(format, memory).is_null() { GlobalFree(memory); return Err("clipboard write failed".into()); }
    Ok(()) // Windows owns the HGLOBAL only after SetClipboardData succeeds.
}
unsafe fn same(format: u32, expected: &[u8]) -> bool {
    let handle = GetClipboardData(format); if handle.is_null() { return false; }
    let size = GlobalSize(handle); if size < expected.len() || size as u64 > openless_core::dictation_references::MAX_SESSION_BYTES + 4096 { return false; }
    let bytes = GlobalLock(handle); if bytes.is_null() { return false; }
    let matches = std::slice::from_raw_parts(bytes, expected.len()) == expected;
    GlobalUnlock(handle); matches
}
fn write(format_id: u32, bytes: Vec<u8>, expected: Option<u64>) -> Result<Receipt, String> {
    let _clipboard = Clipboard::open()?;
    unsafe {
        if expected.is_some_and(|r| r != GetClipboardSequenceNumber() as u64) { return Err("clipboard ownership changed".into()); }
        let token = marker(&format_id.to_string(), &bytes);
        let marker = format(MARKER); if marker == 0 { return Err("clipboard marker unavailable".into()); }
        if EmptyClipboard() == 0 { return Err("clipboard clear failed".into()); }
        put(format_id, &bytes)?;
        let token_bytes = token.bytes().chain(Some(0)).collect::<Vec<_>>();
        put(marker, &token_bytes)?;
        // Capture attributable revision while OpenClipboard still excludes
        // external writers. Registration never samples a later global value.
        Ok(Receipt { revision: GetClipboardSequenceNumber() as u64, token, format: format_id.to_string(), digest: digest(&bytes), size: bytes.len() })
    }
}
pub(super) fn write_text(text: &str, expected: Option<u64>) -> Result<Receipt, String> {
    write(13, text.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect(), expected)
}
pub(super) fn write_image(_: &Path, bytes: Vec<u8>) -> Result<Receipt, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default(); limits.max_alloc = Some(openless_core::dictation_references::MAX_SESSION_BYTES); reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
    let (width, height) = image.dimensions();
    let mut dib = vec![0u8; 124];
    dib[..4].copy_from_slice(&124u32.to_le_bytes()); dib[4..8].copy_from_slice(&(width as i32).to_le_bytes());
    dib[8..12].copy_from_slice(&(-(height as i32)).to_le_bytes()); dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes()); dib[16..20].copy_from_slice(&3u32.to_le_bytes());
    for (at, mask) in [(40, 0x00ff0000u32), (44, 0x0000ff00), (48, 0x000000ff), (52, 0xff000000)] { dib[at..at+4].copy_from_slice(&mask.to_le_bytes()); }
    dib[56..60].copy_from_slice(&0x73524742u32.to_le_bytes()); // LCS_sRGB.
    for pixel in image.pixels() { dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]); }
    write(17, dib, None)
}
pub(super) fn owns(receipt: &Receipt) -> bool {
    let Ok(_clipboard) = Clipboard::open() else { return false; };
    unsafe {
        if GetClipboardSequenceNumber() as u64 != receipt.revision || !same(format(MARKER), receipt.token.as_bytes()) { return false; }
        let Ok(format_id) = receipt.format.parse() else { return false; };
        let memory = GetClipboardData(format_id); if memory.is_null() { return false; }
        let size = GlobalSize(memory); if size < receipt.size { return false; }
        let data = GlobalLock(memory); if data.is_null() { return false; }
        let matches = digest(std::slice::from_raw_parts(data, receipt.size)) == receipt.digest;
        GlobalUnlock(memory); matches
    }
}
pub(super) fn is_own_offer() -> bool {
    let token = {
        let Ok(_clipboard) = Clipboard::open() else { return false; };
        unsafe {
            let memory = GetClipboardData(format(MARKER)); if memory.is_null() { return false; }
            let size = GlobalSize(memory); if size > 4096 { return false; }
            let data = GlobalLock(memory); if data.is_null() { return false; }
            let bytes = std::slice::from_raw_parts(data, size);
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(size);
            let token = String::from_utf8(bytes[..end].to_vec()).ok(); GlobalUnlock(memory);
            token.and_then(|token| from_marker(token, GetClipboardSequenceNumber() as u64))
        }
    };
    token.is_some_and(|receipt| owns(&receipt))
}
