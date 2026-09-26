use eframe::egui;
use resvg::{tiny_skia, usvg};

use super::theme;

#[derive(Clone, Copy, Debug)]
pub enum IconName {
    Overview,
    History,
    Vocab,
    Style,
    SelectionAsk,
    Settings,
    Mic,
    Sparkle,
    Hash,
    Clock,
    Bolt,
    Copy,
    Search,
    Trash,
    Refresh,
    Download,
    Upload,
    Plus,
    Play,
    Stop,
    Close,
    Check,
    Send,
    Pin,
    Chat,
    // GitHub avatar fallback isn't a Lucide icon in the Tauri UI.
    Github,
    More,
    ChevronRight,
    Feather,
    Layout,
    Doc,
    Pencil,
    Cloud,
    Shield,
    Info,
    Help,
    External,
    Monitor,
}

// These masks are generated from the pinned lucide-react package by
// scripts/sync-egui-lucide-icons.mjs. Unlike the old hand-drawn approximations,
// History, Vocab, Style, SelectionAsk and Settings retain every SVG path.
fn lucide_svg(icon: IconName) -> Option<(&'static str, &'static str)> {
    Some(match icon {
        IconName::Overview => (
            "Overview",
            include_str!("../../../assets/lucide/Overview.svg"),
        ),
        IconName::History => (
            "History",
            include_str!("../../../assets/lucide/History.svg"),
        ),
        IconName::Vocab => ("Vocab", include_str!("../../../assets/lucide/Vocab.svg")),
        IconName::Style => ("Style", include_str!("../../../assets/lucide/Style.svg")),
        IconName::SelectionAsk => (
            "SelectionAsk",
            include_str!("../../../assets/lucide/SelectionAsk.svg"),
        ),
        IconName::Settings => (
            "Settings",
            include_str!("../../../assets/lucide/Settings.svg"),
        ),
        IconName::Mic => ("Mic", include_str!("../../../assets/lucide/Mic.svg")),
        IconName::Sparkle => (
            "Sparkle",
            include_str!("../../../assets/lucide/Sparkle.svg"),
        ),
        IconName::Hash => ("Hash", include_str!("../../../assets/lucide/Hash.svg")),
        IconName::Clock => ("Clock", include_str!("../../../assets/lucide/Clock.svg")),
        IconName::Bolt => ("Bolt", include_str!("../../../assets/lucide/Bolt.svg")),
        IconName::Copy => ("Copy", include_str!("../../../assets/lucide/Copy.svg")),
        IconName::Search => ("Search", include_str!("../../../assets/lucide/Search.svg")),
        IconName::Trash => ("Trash", include_str!("../../../assets/lucide/Trash.svg")),
        IconName::Refresh => (
            "Refresh",
            include_str!("../../../assets/lucide/Refresh.svg"),
        ),
        IconName::Download => (
            "Download",
            include_str!("../../../assets/lucide/Download.svg"),
        ),
        IconName::Upload => ("Upload", include_str!("../../../assets/lucide/Upload.svg")),
        IconName::Plus => ("Plus", include_str!("../../../assets/lucide/Plus.svg")),
        IconName::Play => ("Play", include_str!("../../../assets/lucide/Play.svg")),
        IconName::Stop => ("Stop", include_str!("../../../assets/lucide/Stop.svg")),
        IconName::Close => ("Close", include_str!("../../../assets/lucide/Close.svg")),
        IconName::Check => ("Check", include_str!("../../../assets/lucide/Check.svg")),
        IconName::Send => ("Send", include_str!("../../../assets/lucide/Send.svg")),
        IconName::Pin => ("Pin", include_str!("../../../assets/lucide/Pin.svg")),
        IconName::Chat => ("Chat", include_str!("../../../assets/lucide/Chat.svg")),
        IconName::More => ("More", include_str!("../../../assets/lucide/More.svg")),
        IconName::ChevronRight => (
            "ChevronRight",
            include_str!("../../../assets/lucide/ChevronRight.svg"),
        ),
        IconName::Feather => (
            "Feather",
            include_str!("../../../assets/lucide/Feather.svg"),
        ),
        IconName::Layout => ("Layout", include_str!("../../../assets/lucide/Layout.svg")),
        IconName::Doc => ("Doc", include_str!("../../../assets/lucide/Doc.svg")),
        IconName::Pencil => ("Pencil", include_str!("../../../assets/lucide/Pencil.svg")),
        IconName::Cloud => ("Cloud", include_str!("../../../assets/lucide/Cloud.svg")),
        IconName::Shield => ("Shield", include_str!("../../../assets/lucide/Shield.svg")),
        IconName::Info => ("Info", include_str!("../../../assets/lucide/Info.svg")),
        IconName::Help => ("Help", include_str!("../../../assets/lucide/Help.svg")),
        IconName::External => (
            "External",
            include_str!("../../../assets/lucide/External.svg"),
        ),
        IconName::Monitor => (
            "Monitor",
            include_str!("../../../assets/lucide/Monitor.svg"),
        ),
        IconName::Github => return None,
    })
}

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
    for pixel in pixmap.data().chunks_exact(4) {
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
    let id = egui::Id::new(("lucide-icon", name));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lucide_icon_rasterizes_to_a_nonempty_mask() {
        let icons = [
            IconName::Overview,
            IconName::History,
            IconName::Vocab,
            IconName::Style,
            IconName::SelectionAsk,
            IconName::Settings,
            IconName::Mic,
            IconName::Sparkle,
            IconName::Hash,
            IconName::Clock,
            IconName::Bolt,
            IconName::Copy,
            IconName::Search,
            IconName::Trash,
            IconName::Refresh,
            IconName::Download,
            IconName::Upload,
            IconName::Plus,
            IconName::Play,
            IconName::Stop,
            IconName::Close,
            IconName::Check,
            IconName::Send,
            IconName::Pin,
            IconName::Chat,
            IconName::More,
            IconName::ChevronRight,
            IconName::Feather,
            IconName::Layout,
            IconName::Doc,
            IconName::Pencil,
            IconName::Cloud,
            IconName::Shield,
            IconName::Info,
            IconName::Help,
            IconName::External,
            IconName::Monitor,
        ];
        for icon in icons {
            let (name, svg) = lucide_svg(icon).unwrap();
            let image = rasterize_mask(svg).unwrap_or_else(|| panic!("invalid SVG: {name}"));
            assert!(
                image.pixels.iter().any(|pixel| pixel.a() != 0),
                "empty SVG: {name}"
            );
        }
    }
}
