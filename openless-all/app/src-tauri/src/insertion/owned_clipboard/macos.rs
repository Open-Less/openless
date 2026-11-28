use super::*;
use objc2::{msg_send, runtime::{AnyClass, AnyObject}, rc::autoreleasepool};
use objc2_foundation::NSString;
#[link(name = "AppKit", kind = "framework")]
extern "C" {}
unsafe fn board() -> Option<*mut AnyObject> {
    let board: *mut AnyObject = msg_send![AnyClass::get("NSPasteboard")?, generalPasteboard];
    (!board.is_null()).then_some(board)
}
unsafe fn marker_token(board: *mut AnyObject) -> Option<String> {
    let data: *mut AnyObject = msg_send![board, dataForType: &*NSString::from_str(MARKER)];
    if data.is_null() { return None; }
    let length: usize = msg_send![data, length]; if length > 4096 { return None; }
    let token: *mut NSString = msg_send![board, stringForType: &*NSString::from_str(MARKER)];
    (!token.is_null()).then(|| (&*token).to_string())
}
unsafe fn inspect(board: *mut AnyObject, receipt: &Receipt) -> bool {
    let before: isize = msg_send![board, changeCount];
    if before < 0 || before as u64 != receipt.revision { return false; }
    if marker_token(board).as_deref() != Some(receipt.token.as_str()) { return false; }
    let value: *mut AnyObject = msg_send![board, dataForType: &*NSString::from_str(&receipt.format)];
    if value.is_null() { return false; }
    let size: usize = msg_send![value, length];
    let bytes: *const u8 = msg_send![value, bytes];
    if size != receipt.size || bytes.is_null() && size != 0 { return false; }
    let payload = if size == 0 { &[][..] } else { std::slice::from_raw_parts(bytes, size) };
    let matches = digest(payload) == receipt.digest;
    let after: isize = msg_send![board, changeCount];
    matches && before == after
}
fn write(format: &str, bytes: &[u8], expected: Option<u64>) -> Result<Receipt, String> {
    write_after(format, bytes, expected, || {})
}
fn write_after(format: &str, bytes: &[u8], expected: Option<u64>, after_native_write: impl FnOnce()) -> Result<Receipt, String> {
    autoreleasepool(|_| unsafe {
        let board = board().ok_or("pasteboard unavailable")?;
        let token = marker(format, bytes);
        let item: *mut AnyObject = msg_send![AnyClass::get("NSPasteboardItem").ok_or("pasteboard item unavailable")?, new];
        if item.is_null() { return Err("pasteboard item unavailable".into()); }
        let result = (|| {
            let data: *mut AnyObject = msg_send![AnyClass::get("NSData").ok_or("data unavailable")?, dataWithBytes: bytes.as_ptr().cast::<std::ffi::c_void>(), length: bytes.len()];
            let value_ok: bool = msg_send![item, setData: data, forType: &*NSString::from_str(format)];
            let marker_ok: bool = msg_send![item, setString: &*NSString::from_str(&token), forType: &*NSString::from_str(MARKER)];
            if !value_ok || !marker_ok { return Err("clipboard item preparation failed".into()); }
            let objects: *mut AnyObject = msg_send![AnyClass::get("NSArray").ok_or("array unavailable")?, arrayWithObject: item];
            let before: isize = msg_send![board, changeCount];
            if expected.is_some_and(|revision| before < 0 || revision != before as u64) { return Err("clipboard ownership changed".into()); }
            let _: isize = msg_send![board, clearContents];
            let written: bool = msg_send![board, writeObjects: objects];
            if !written { return Err("clipboard write failed".into()); }
            after_native_write();
            let revision: isize = msg_send![board, changeCount];
            if revision < 0 { return Err("clipboard revision unavailable".into()); }
            let receipt = Receipt { revision: revision as u64, token, format: format.into(), digest: digest(bytes), size: bytes.len() };
            // If B replaced A before this sample, B lacks both our marker and
            // expected payload. It can never be registered as our write.
            if !inspect(board, &receipt) { return Err("clipboard ownership changed".into()); }
            Ok(receipt)
        })();
        let _: () = msg_send![item, release];
        result
    })
}
pub(super) fn write_text(text: &str, expected: Option<u64>) -> Result<Receipt, String> { write("public.utf8-plain-text", text.as_bytes(), expected) }
pub(super) fn write_image(path: &Path, bytes: Vec<u8>) -> Result<Receipt, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    write(match ext.as_str() { "tif" | "tiff" => "public.tiff", "jpg" | "jpeg" => "public.jpeg", _ => "public.png" }, &bytes, None)
}
pub(super) fn owns(receipt: &Receipt) -> bool { autoreleasepool(|_| unsafe { board().is_some_and(|b| inspect(b, receipt)) }) }
pub(super) fn is_own_offer() -> bool {
    autoreleasepool(|_| unsafe {
        let Some(board) = board() else { return false; };
        let revision: isize = msg_send![board, changeCount]; if revision < 0 { return false; }
        marker_token(board).and_then(|token| from_marker(token, revision as u64)).is_some_and(|receipt| inspect(board, &receipt))
    })
}

#[cfg(test)]
mod receipt_tests {
    use super::*;
    fn external_copy(text: &str) {
        autoreleasepool(|_| unsafe {
            let board = board().unwrap();
            let _: isize = msg_send![board, clearContents];
            let written: bool = msg_send![board, setString: &*NSString::from_str(text), forType: &*NSString::from_str("public.utf8-plain-text")];
            assert!(written);
        });
    }
    #[test]
    fn references_native_receipt_never_claims_external_B_in_the_write_to_receipt_window() {
        let result = write_after("public.utf8-plain-text", b"application A", None, || external_copy("external B"));
        assert!(result.is_err(), "B must not acquire a receipt for A's write");
        assert!(!is_own_offer());
        let receipt = write_text("application A", None).unwrap();
        assert!(owns(&receipt));
        // Identical bytes from a new external owner are still a user copy.
        external_copy("application A");
        assert!(!owns(&receipt));
        assert!(!is_own_offer());
    }
}
