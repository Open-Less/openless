pub fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(
                text.to_owned(),
                egui::FontId::proportional(size),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .x
    })
}

pub fn text_galley(
    ui: &egui::Ui,
    text: &str,
    color: egui::Color32,
    size: f32,
    max_width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = max_rows;
    job.append(
        text,
        0.0,
        egui::text::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}
