//! Lucide outlines (`Icon.tsx`), rasterized at the device size and tinted.
use resvg::{tiny_skia, usvg};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    Close,
    Check,
}

fn paths(icon: IconName) -> &'static str {
    match icon {
        // lucide `x`
        IconName::Close => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
        // lucide `check`
        IconName::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
    }
}

/// White coverage mask for `size` points. As in lucide-react, `stroke_width`
/// is in the icon's 24-unit space and scales with `size`.
pub fn texture(
    ctx: &egui::Context,
    icon: IconName,
    size: f32,
    stroke_width: f32,
) -> Option<egui::TextureHandle> {
    let pixels = (size * ctx.pixels_per_point()).round().max(1.0) as u32;
    let id = egui::Id::unique(("lucide-icon", icon, pixels, (stroke_width * 100.0) as u32));
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return Some(texture);
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="{stroke_width}" stroke-linecap="round" stroke-linejoin="round">{}</svg>"##,
        paths(icon)
    );
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(pixels, pixels)?;
    let scale = pixels as f32 / 24.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia returns premultiplied white; the mask is the alpha channel.
    let mut rgba = Vec::with_capacity(pixmap.data().len());
    for pixel in pixmap.data().as_chunks::<4>().0 {
        rgba.extend_from_slice(&[255, 255, 255, pixel[3]]);
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([pixels as usize; 2], &rgba);
    let texture = ctx.load_texture(
        format!("lucide-{icon:?}-{pixels}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    Some(texture)
}
