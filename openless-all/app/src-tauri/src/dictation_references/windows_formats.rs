//! Bounded clipboard format parsing; runs in host tests on every desktop OS.
use super::{MAX_BYTES, MAX_REFERENCES};
use std::path::PathBuf;
type ParseResult<T> = Result<T, &'static str>;
fn u16_at(bytes: &[u8], at: usize) -> ParseResult<u16> { Ok(u16::from_le_bytes(bytes.get(at..at + 2).ok_or("captureFailed")?.try_into().unwrap())) }
fn u32_at(bytes: &[u8], at: usize) -> ParseResult<u32> { Ok(u32::from_le_bytes(bytes.get(at..at + 4).ok_or("captureFailed")?.try_into().unwrap())) }
pub(super) fn unicode_text(bytes: &[u8]) -> ParseResult<String> {
    if bytes.len() as u64 > MAX_BYTES { return Err("tooLarge"); }
    if bytes.len() % 2 != 0 { return Err("captureFailed"); }
    let units: Vec<u16> = bytes.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).take_while(|v| *v != 0).collect();
    if units.len() * 2 == bytes.len() { return Err("captureFailed"); }
    String::from_utf16(&units).map_err(|_| "captureFailed")
}
pub(super) fn hdrop(bytes: &[u8]) -> ParseResult<Vec<PathBuf>> {
    if bytes.len() as u64 > MAX_BYTES { return Err("tooLarge"); }
    let offset = u32_at(bytes, 0)? as usize;
    if offset < 20 || offset > bytes.len() || u32_at(bytes, 16)? == 0 { return Err("captureFailed"); }
    let bytes = &bytes[offset..];
    if bytes.len() % 2 != 0 { return Err("captureFailed"); }
    let mut paths = Vec::new();
    let mut start = 0;
    for at in (0..bytes.len()).step_by(2) {
        if u16_at(bytes, at)? != 0 { continue; }
        if at == start { return Ok(paths); } // Double-NUL terminated list.
        paths.push(PathBuf::from(unicode_text(&bytes[start..at + 2])?));
        if paths.len() > MAX_REFERENCES { return Err("tooLarge"); }
        start = at + 2;
    }
    Err("captureFailed")
}
fn channel(pixel: u32, mask: u32) -> ParseResult<u8> {
    if mask == 0 { return Ok(255); }
    let shifted = mask >> mask.trailing_zeros();
    if shifted & shifted.wrapping_add(1) != 0 { return Err("captureFailed"); }
    Ok((((pixel & mask) >> mask.trailing_zeros()) as u64 * 255 / shifted as u64) as u8)
}
pub(super) fn dib_to_png(bytes: &[u8]) -> ParseResult<Vec<u8>> {
    if bytes.len() as u64 > MAX_BYTES { return Err("tooLarge"); }
    let header = u32_at(bytes, 0)? as usize;
    if ![40, 52, 56, 108, 124].contains(&header) || bytes.len() < header { return Err("captureFailed"); }
    let width = u32_at(bytes, 4)? as i32;
    let signed_height = u32_at(bytes, 8)? as i32;
    let height = signed_height.unsigned_abs();
    let bits = u16_at(bytes, 14)? as usize;
    let compression = u32_at(bytes, 16)?;
    if width <= 0 || height == 0 || u16_at(bytes, 12)? != 1 || ![1, 4, 8, 16, 24, 32].contains(&bits) || ![0, 3, 6].contains(&compression) || (compression != 0 && ![16, 32].contains(&bits)) { return Err("captureFailed"); }
    let width = width as usize;
    let decoded_len = width.checked_mul(height as usize).and_then(|n| n.checked_mul(4)).ok_or("tooLarge")?;
    if decoded_len as u64 > MAX_BYTES { return Err("tooLarge"); }
    let stride = (width * bits).div_ceil(32) * 4;
    let mut offset = header;
    let mut masks = if bits == 16 { [0x7c00, 0x3e0, 0x1f, 0] } else { [0x00ff0000, 0x0000ff00, 0x000000ff, 0] };
    if compression != 0 {
        let count = if compression == 6 || header >= 56 { 4 } else { 3 };
        for (i, mask) in masks.iter_mut().enumerate().take(count) { *mask = u32_at(bytes, 40 + i * 4)?; }
        if header == 40 { offset += count * 4; }
        if masks[..3].contains(&0) { return Err("captureFailed"); }
        for a in 0..4 { for b in a + 1..4 { if masks[a] & masks[b] != 0 { return Err("captureFailed"); } } }
    }
    let colors = u32_at(bytes, 32)? as usize;
    let colors = if colors == 0 && bits <= 8 { 1 << bits } else { colors };
    if bits <= 8 && colors > 1 << bits { return Err("captureFailed"); }
    let palette_end = offset.checked_add(colors.checked_mul(4).ok_or("captureFailed")?).ok_or("captureFailed")?;
    let palette = bytes.get(offset..palette_end).ok_or("captureFailed")?;
    offset = palette_end;
    let pixels = bytes.get(offset..offset.checked_add(stride * height as usize).ok_or("captureFailed")?).ok_or("captureFailed")?;
    let mut rgba = vec![0; decoded_len];
    for y in 0..height as usize {
        let source_y = if signed_height > 0 { height as usize - 1 - y } else { y };
        for x in 0..width {
            if bits <= 8 {
                let packed = pixels[source_y * stride + x * bits / 8];
                let shift = 8 - bits - x * bits % 8;
                let index = ((packed >> shift) & ((1u16 << bits) - 1) as u8) as usize;
                let color = palette.get(index * 4..index * 4 + 4).ok_or("captureFailed")?;
                rgba[(y * width + x) * 4..(y * width + x) * 4 + 4].copy_from_slice(&[color[2], color[1], color[0], 255]);
                continue;
            }
            let at = source_y * stride + x * (bits / 8);
            let pixel = u32::from_le_bytes([pixels[at], pixels[at + 1], if bits >= 24 { pixels[at + 2] } else { 0 }, if bits == 32 { pixels[at + 3] } else { 0 }]);
            for c in 0..4 { rgba[(y * width + x) * 4 + c] = channel(pixel, masks[c])?; }
        }
    }
    use image::ImageEncoder;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&rgba, width as u32, height, image::ExtendedColorType::Rgba8).map_err(|_| "captureFailed")?;
    if png.len() as u64 > MAX_BYTES { return Err("tooLarge"); }
    Ok(png)
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn dib24() -> Vec<u8> {
        let mut b = vec![0; 48];
        b[..4].copy_from_slice(&40u32.to_le_bytes()); b[4..8].copy_from_slice(&1i32.to_le_bytes()); b[8..12].copy_from_slice(&2i32.to_le_bytes());
        b[12..14].copy_from_slice(&1u16.to_le_bytes()); b[14..16].copy_from_slice(&24u16.to_le_bytes());
        b[40..44].copy_from_slice(&[255, 0, 0, 0]); b[44..48].copy_from_slice(&[0, 0, 255, 0]); b
    }
    fn wide(s: &str) -> Vec<u8> { s.encode_utf16().flat_map(u16::to_le_bytes).collect() }
    #[test]
    fn references_windows_unicode_and_file_drop_parse_without_native_clipboard() {
        assert_eq!(unicode_text(&wide("复制🙂\0")).unwrap(), "复制🙂");
        assert!(unicode_text(&[1]).is_err()); assert!(unicode_text(&wide("unterminated")).is_err());
        let mut b = vec![0; 20]; b[..4].copy_from_slice(&20u32.to_le_bytes()); b[16..20].copy_from_slice(&1u32.to_le_bytes());
        b.extend(wide("C:\\one.txt\0D:\\目录\0\0"));
        assert_eq!(hdrop(&b).unwrap(), [PathBuf::from("C:\\one.txt"), PathBuf::from("D:\\目录")]);
        b.truncate(b.len() - 2); assert!(hdrop(&b).is_err());
        b[..4].copy_from_slice(&u32::MAX.to_le_bytes()); assert!(hdrop(&b).is_err());
    }
    #[test]
    fn references_windows_dib_padding_orientation_v5_masks_and_limits() {
        let b = dib24();
        let image = image::load_from_memory(&dib_to_png(&b).unwrap()).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]); assert_eq!(image.get_pixel(0, 1).0, [0, 0, 255, 255]);
        let mut top = b.clone(); top[8..12].copy_from_slice(&(-2i32).to_le_bytes());
        assert_eq!(image::load_from_memory(&dib_to_png(&top).unwrap()).unwrap().to_rgba8().get_pixel(0, 0).0, [0, 0, 255, 255]);
        let mut v5 = vec![0; 128]; v5[..40].copy_from_slice(&b[..40]); v5[..4].copy_from_slice(&124u32.to_le_bytes());
        v5[8..12].copy_from_slice(&1u32.to_le_bytes()); v5[14..16].copy_from_slice(&32u16.to_le_bytes()); v5[16..20].copy_from_slice(&3u32.to_le_bytes());
        for (i, mask) in [0xff0000u32, 0xff00, 0xff, 0xff000000].into_iter().enumerate() { v5[40 + i * 4..44 + i * 4].copy_from_slice(&mask.to_le_bytes()); }
        v5[124..].copy_from_slice(&[3, 2, 1, 128]);
        assert_eq!(image::load_from_memory(&dib_to_png(&v5).unwrap()).unwrap().to_rgba8().get_pixel(0, 0).0, [1, 2, 3, 128]);
        assert!(dib_to_png(&b[..47]).is_err());
        let mut huge = b; huge[4..8].copy_from_slice(&i32::MAX.to_le_bytes()); assert_eq!(dib_to_png(&huge), Err("tooLarge"));
        huge[4..8].copy_from_slice(&(-1i32).to_le_bytes()); assert_eq!(dib_to_png(&huge), Err("captureFailed"));
    }
    #[test]
    fn references_windows_dib_palette_and_rgb565() {
        let mut b = dib24(); b.truncate(40); b[8..12].copy_from_slice(&1u32.to_le_bytes()); b[14..16].copy_from_slice(&1u16.to_le_bytes());
        b.extend([0, 0, 0, 0, 0, 0, 255, 0, 0x80, 0, 0, 0]);
        assert_eq!(image::load_from_memory(&dib_to_png(&b).unwrap()).unwrap().to_rgba8().get_pixel(0, 0).0, [255, 0, 0, 255]);
        b.truncate(40); b[14..16].copy_from_slice(&16u16.to_le_bytes()); b[16..20].copy_from_slice(&3u32.to_le_bytes());
        for mask in [0xf800u32, 0x7e0, 0x1f] { b.extend(mask.to_le_bytes()); }
        b.extend([0xe0, 0x07, 0, 0]);
        assert_eq!(image::load_from_memory(&dib_to_png(&b).unwrap()).unwrap().to_rgba8().get_pixel(0, 0).0, [0, 255, 0, 255]);
    }
}
