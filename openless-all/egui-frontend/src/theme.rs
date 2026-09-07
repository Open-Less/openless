use std::path::PathBuf;

pub const BLUE: egui::Color32 = egui::Color32::from_rgb(37, 99, 235);
pub const BLUE_SOFT: egui::Color32 = egui::Color32::from_rgb(239, 245, 255);
pub const CANVAS: egui::Color32 = egui::Color32::from_rgb(250, 250, 250);
pub const SURFACE: egui::Color32 = egui::Color32::WHITE;
pub const SURFACE_2: egui::Color32 = egui::Color32::from_rgb(244, 244, 245);
pub const LINE: egui::Color32 = egui::Color32::from_rgb(228, 228, 231);
pub const INK: egui::Color32 = egui::Color32::from_rgb(9, 9, 11);
pub const INK_2: egui::Color32 = egui::Color32::from_rgb(63, 63, 70);
pub const INK_3: egui::Color32 = egui::Color32::from_rgb(113, 113, 122);
pub const INK_4: egui::Color32 = egui::Color32::from_rgb(161, 161, 170);
pub const OK: egui::Color32 = egui::Color32::from_rgb(22, 163, 74);

pub fn install_fonts(ctx: &egui::Context) {
    // Prefer the bundled HarmonyOS Sans faces for the scripts they support,
    // then add system fallbacks. The original UI exposes Arabic, Thai and
    // Devanagari language names too; no single HarmonyOS face covers all of
    // them, so egui needs the remaining script-specific fallbacks as well.
    let mut candidates: Vec<(&str, PathBuf)> = Vec::new();
    if let Some(path) = std::env::var_os("OPENLESS_IME_FONT").map(PathBuf::from) {
        candidates.push(("openless-primary", path));
    }
    candidates.extend([
        (
            "openless-cjk",
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        ),
        (
            "openless-cjk-fallback",
            PathBuf::from("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf"),
        ),
        (
            "openless-latin",
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf"),
        ),
        (
            "openless-arabic",
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf"),
        ),
        (
            "openless-thai",
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"),
        ),
        (
            "openless-devanagari",
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf"),
        ),
    ]);

    let mut fonts = egui::FontDefinitions::default();
    let bundled_fonts: [(&str, &'static [u8]); 4] = [
        (
            "harmony-latin",
            include_bytes!("../assets/fonts/HarmonyOS_Sans.ttf"),
        ),
        (
            "harmony-sc",
            include_bytes!("../assets/fonts/HarmonyOS_Sans_SC.ttf"),
        ),
        (
            "harmony-tc",
            include_bytes!("../assets/fonts/HarmonyOS_Sans_TC.ttf"),
        ),
        (
            "harmony-arabic",
            include_bytes!("../assets/fonts/HarmonyOS_Sans_Naskh_Arabic_UI.ttf"),
        ),
    ];
    let mut loaded_fonts = Vec::with_capacity(bundled_fonts.len() + candidates.len());
    for (name, bytes) in bundled_fonts {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(bytes).into());
        loaded_fonts.push(name);
    }
    for (name, path) in candidates {
        if !path.exists() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("[egui-frontend] failed to read font {}", path.display());
            continue;
        };
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_owned(bytes).into());
        loaded_fonts.push(name);
    }
    if loaded_fonts.is_empty() {
        eprintln!("[egui-frontend] no compatible UI font found");
        return;
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let family_fonts = fonts.families.entry(family).or_default();
        for name in loaded_fonts.iter().rev() {
            family_fonts.insert(0, (*name).into());
        }
    }
    ctx.set_fonts(fonts);
}
