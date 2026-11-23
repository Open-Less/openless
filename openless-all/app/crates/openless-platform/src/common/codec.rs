//! 剪贴板数据的纯编解码与格式分类。
//!
//! 这里刻意不依赖 Win32：格式判定和 UTF-16 布局可以在任何平台单测，
//! 真正的系统调用留在同目录的 `windows` 后端。

/// `CF_UNICODETEXT` 的数据布局：UTF-16LE，通常带末尾 NUL。
///
/// 统一按字节搬运，方便和"全格式快照"共用一套读写路径。
pub fn encode_utf16_block(text: &str) -> Vec<u8> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    let mut bytes = Vec::with_capacity(units.len() * 2);
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// 解出 `CF_UNICODETEXT` 的内容。最后一对字节不完整或只有 NUL 时按空串处理。
pub fn decode_utf16_block(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

const CF_BITMAP: u32 = 2;
const CF_METAFILEPICT: u32 = 3;
const CF_PALETTE: u32 = 9;
const CF_ENHMETAFILE: u32 = 14;
const CF_OWNERDISPLAY: u32 = 0x0080;
const CF_DSPTEXT: u32 = 0x0081;
const CF_DSPBITMAP: u32 = 0x0082;
const CF_DSPMETAFILEPICT: u32 = 0x0083;
const CF_DSPENHMETAFILE: u32 = 0x008E;
const CF_PRIVATEFIRST: u32 = 0x0200;
const CF_PRIVATELAST: u32 = 0x02FF;
const CF_GDIOBJFIRST: u32 = 0x0300;
const CF_GDIOBJLAST: u32 = 0x03FF;

/// 该剪贴板格式的数据是不是 HGLOBAL 内存块，能不能按字节原样备份/还原。
///
/// 不能搬的三类：
/// - GDI 句柄（`CF_BITMAP` / `CF_PALETTE` / `CF_METAFILEPICT` / `CF_ENHMETAFILE`），
///   句柄本身不是内存块，复制字节没有意义；
/// - owner-display 与 `CF_DSP*`，数据由剪贴板所有者按需渲染；
/// - `CF_PRIVATEFIRST..CF_PRIVATELAST`、`CF_GDIOBJFIRST..CF_GDIOBJLAST`，
///   系统按私有/设备对象语义释放，不能由我们重新 `SetClipboardData`。
pub fn is_memory_backed_format(format: u32) -> bool {
    if format == 0 {
        return false;
    }
    if matches!(
        format,
        CF_BITMAP
            | CF_METAFILEPICT
            | CF_PALETTE
            | CF_ENHMETAFILE
            | CF_OWNERDISPLAY
            | CF_DSPTEXT
            | CF_DSPBITMAP
            | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    ) {
        return false;
    }
    if (CF_PRIVATEFIRST..=CF_PRIVATELAST).contains(&format) {
        return false;
    }
    if (CF_GDIOBJFIRST..=CF_GDIOBJLAST).contains(&format) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_block_round_trips_and_keeps_trailing_nul() {
        let bytes = encode_utf16_block("你好 OpenLess 😀");
        assert_eq!(bytes.len() % 2, 0);
        assert_eq!(&bytes[bytes.len() - 2..], &[0, 0]);
        assert_eq!(decode_utf16_block(&bytes).as_deref(), Some("你好 OpenLess 😀"));
    }

    #[test]
    fn empty_text_decodes_to_empty_string_not_none() {
        assert_eq!(decode_utf16_block(&[0, 0]).as_deref(), Some(""));
        assert_eq!(decode_utf16_block(&[]), None);
        assert_eq!(decode_utf16_block(&[0x41]), None);
    }

    #[test]
    fn text_without_trailing_nul_still_decodes() {
        let mut bytes = Vec::new();
        for unit in "abc".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_utf16_block(&bytes).as_deref(), Some("abc"));
    }

    #[test]
    fn embedded_nul_ends_the_text() {
        let bytes = encode_utf16_block("ab");
        let decoded = decode_utf16_block(&bytes);
        assert_eq!(decoded.as_deref(), Some("ab"));
    }

    #[test]
    fn gdi_and_owner_display_formats_are_not_byte_preserved() {
        for format in [2u32, 3, 9, 14, 0x0080, 0x0081, 0x0083, 0x008E] {
            assert!(!is_memory_backed_format(format), "format {format}");
        }
    }

    #[test]
    fn private_and_gdi_object_ranges_are_not_byte_preserved() {
        for format in [0x0200u32, 0x0250, 0x02FF, 0x0300, 0x03FF] {
            assert!(!is_memory_backed_format(format), "format {format}");
        }
    }

    #[test]
    fn text_and_registered_formats_are_byte_preserved() {
        for format in [1u32, 7, 13, 15, 16, 17, 0xC000, 0xC123, 0x0082] {
            let expected = format != 0x0082; // CF_DSPBITMAP 例外
            assert_eq!(is_memory_backed_format(format), expected, "format {format}");
        }
        assert!(!is_memory_backed_format(0));
    }
}
