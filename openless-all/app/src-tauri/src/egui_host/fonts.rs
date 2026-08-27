//! CJK 字体安装（egui 默认字体不含中文字形，不装会显示成 □）。
//! 候选优先级：OPENLESS_IME_FONT 环境变量 > 常见系统中文字体 > HOME 字体目录扫描。

pub fn install_cjk_fonts(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        // Linux
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
    ];

    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("OPENLESS_IME_FONT") {
        paths.push(std::path::PathBuf::from(p));
    }
    paths.extend(CANDIDATES.iter().map(std::path::PathBuf::from));

    if let Ok(home) = std::env::var("HOME") {
        for sub in [".fonts", ".local/share/fonts"] {
            if let Ok(entries) = std::fs::read_dir(std::path::PathBuf::from(&home).join(sub)) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.to_ascii_lowercase().contains("cjk")
                        || name.to_ascii_lowercase().contains("wqy")
                    {
                        paths.push(p);
                    }
                }
            }
        }
    }

    let mut inserted = 0usize;
    for path in paths {
        if !path.exists() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let mut fonts = egui::FontDefinitions::default();
        let name = format!("cjk-{inserted}");
        fonts.font_data.insert(
            name.clone(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.insert(0, name.clone());
            }
        }
        ctx.set_fonts(fonts);
        log::info!("[egui-host] CJK 字体: {}", path.display());
        inserted += 1;
        if inserted >= 2 {
            break;
        }
    }
    if inserted == 0 {
        log::warn!(
            "[egui-host] 未找到 CJK 字体，中文将显示为 □ (OPENLESS_IME_FONT 可指定)"
        );
    }
}
