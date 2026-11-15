//! SVG import boundary for style-pack icons. Core only accepts bounded PNGs.
use resvg::{tiny_skia, usvg};

const MAX_SVG_BYTES: usize = 256 * 1024;
const MAX_PNG_BYTES: usize = 64 * 1024;
const SIZE: u32 = 128;

pub fn rasterize_svg(source: &[u8]) -> Result<Vec<u8>, &'static str> {
    if source.is_empty() || source.len() > MAX_SVG_BYTES {
        return Err("invalid SVG size");
    }
    let text = std::str::from_utf8(source).map_err(|_| "invalid SVG encoding")?;
    let lower = text.to_ascii_lowercase();
    if lower.contains("<!doctype") || lower.contains("<!entity") || lower.contains("@import") {
        return Err("SVG document type or external stylesheet is not allowed");
    }
    let document = roxmltree::Document::parse(text).map_err(|_| "invalid SVG XML")?;
    if document.root_element().tag_name().name() != "svg" {
        return Err("expected an SVG root element");
    }
    for node in document.descendants().filter(|node| node.is_element()) {
        let tag = node.tag_name().name().to_ascii_lowercase();
        if [
            "script",
            "foreignobject",
            "image",
            "feimage",
            "iframe",
            "object",
            "embed",
            "animate",
            "animatemotion",
            "animatetransform",
            "set",
        ]
        .contains(&tag.as_str())
        {
            return Err("unsafe SVG element");
        }
        for attribute in node.attributes() {
            let key = attribute.name().to_ascii_lowercase();
            let value = attribute.value().trim().to_ascii_lowercase();
            if key.starts_with("on")
                || (key == "href" && !value.starts_with('#'))
                || value.contains("@import")
                || value.contains("url(") && !value.contains("url(#")
                || value.contains("http://")
                || value.contains("https://")
                || value.contains("file:")
            {
                return Err("unsafe SVG attribute or external reference");
            }
        }
        if tag == "style"
            && node.text().is_some_and(|style| {
                let style = style.to_ascii_lowercase();
                style.contains("@import") || style.contains("url(") && !style.contains("url(#")
            })
        {
            return Err("unsafe SVG stylesheet");
        }
    }
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(source, &options).map_err(|_| "invalid SVG image")?;
    let dimensions = tree.size();
    let scale = (SIZE as f32 / dimensions.width()).min(SIZE as f32 / dimensions.height());
    let x = (SIZE as f32 - dimensions.width() * scale) / 2.0;
    let y = (SIZE as f32 - dimensions.height() * scale) / 2.0;
    let mut pixmap = tiny_skia::Pixmap::new(SIZE, SIZE).ok_or("invalid SVG dimensions")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale).post_translate(x, y),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png().map_err(|_| "could not encode icon")?;
    if png.len() > MAX_PNG_BYTES {
        return Err("PNG icon exceeds 64 KiB");
    }
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_svg_is_a_bounded_png() {
        let png = rasterize_svg(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="red"/></svg>"#).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.len() <= MAX_PNG_BYTES);
    }

    #[test]
    fn rejects_executable_and_external_references() {
        for source in [
            r#"<!DOCTYPE svg><svg/>"#,
            r#"<svg><script>alert(1)</script></svg>"#,
            r#"<svg><image href="https://example.test/a.png"/></svg>"#,
            r#"<svg onload="evil()"/>"#,
            r#"<svg><path fill="url(https://x.test/a)"/></svg>"#,
        ] {
            assert!(rasterize_svg(source.as_bytes()).is_err(), "{source}");
        }
    }
}
