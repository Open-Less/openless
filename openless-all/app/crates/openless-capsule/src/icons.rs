use resvg::{tiny_skia, usvg};
#[derive(Clone, Copy)]
pub enum IconName {
    Close,
    Check,
}
fn lucide_svg(icon: IconName) -> Option<(&'static str, &'static str)> {
    Some(match icon {
        IconName::Close => (
            "Close",
            include_str!("../../../linux-egui/assets/lucide/Close.svg"),
        ),
        IconName::Check => (
            "Check",
            include_str!("../../../linux-egui/assets/lucide/Check.svg"),
        ),
    })
}
use super::theme;
const ICON_PIXELS: usize = 48; // 3x the 16-point sidebar/control icon.
const ICON_SIZE: f32 = 16.0;

fn rasterize_mask(source: &str) -> Option<egui::ColorImage> {
    let tree = usvg::Tree::from_str(source, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(ICON_PIXELS as u32, ICON_PIXELS as u32)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(ICON_PIXELS as f32 / 24.0, ICON_PIXELS as f32 / 24.0),
        &mut pixmap.as_mut(),
    );
    // tiny-skia returns premultiplied white; ColorImage expects straight RGBA.
    // SVGs contain only white strokes, so opacity is the entire mask.
    let mut pixels = Vec::with_capacity(ICON_PIXELS * ICON_PIXELS * 4);
    for pixel in pixmap.data().as_chunks::<4>().0 {
        pixels.extend_from_slice(&[255, 255, 255, pixel[3]]);
    }
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [ICON_PIXELS, ICON_PIXELS],
        &pixels,
    ))
}

/// Draw the same SVG outlines used by Tauri, recolored for this surface.
/// Texture handles live in egui's context and are reused across frames/windows.
pub fn draw_icon(ui: &egui::Ui, center: egui::Pos2, icon: IconName, color: egui::Color32) {
    let Some((name, svg)) = lucide_svg(icon) else {
        // Not part of Icon.tsx: the account avatar fallback is deliberately
        // separate from the monochrome Lucide interface controls.
        let p = ui.painter();
        p.circle_filled(center, 7.0, color.gamma_multiply(0.75));
        p.circle_filled(center + egui::vec2(0.0, 3.0), 3.4, theme::SURFACE_2);
        return;
    };
    let ctx = ui.ctx();
    let id = egui::Id::unique(("lucide-icon", name));
    let texture = ctx
        .data(|data| data.get_temp::<egui::TextureHandle>(id))
        .or_else(|| {
            let image = rasterize_mask(svg)?;
            let texture = ctx.load_texture(
                format!("lucide-{name}"),
                image,
                egui::TextureOptions::LINEAR,
            );
            ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
            Some(texture)
        });
    if let Some(texture) = texture {
        ui.painter().image(
            texture.id(),
            egui::Rect::from_center_size(center, egui::vec2(ICON_SIZE, ICON_SIZE)),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            color,
        );
    }
}
