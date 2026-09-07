use std::sync::Arc;

use eframe::egui;

use crate::marketplace::MarketplaceState;
use crate::settings::SettingsState;
use crate::theme;

const SIDEBAR_WIDTH: f32 = 188.0;
const TITLEBAR_HEIGHT: f32 = 38.0;
fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut value: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        value.push('…');
    }
    value
}

const SUPPORTED_LANGUAGES: [&str; 15] = [
    "简体中文",
    "繁体中文",
    "English",
    "日本語",
    "한국어",
    "Français",
    "Deutsch",
    "Español",
    "Italiano",
    "Português",
    "Русский",
    "العربية",
    "Tiếng Việt",
    "ไทย",
    "हिन्दी",
];

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenLess")
            .with_inner_size([1240.0, 800.0])
            .with_min_inner_size([960.0, 640.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true)
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "openless-egui-frontend",
        options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(MainWindow::default()))
        }),
    )
}

#[derive(Default)]
struct MainWindow {
    active: Tab,
    style_open: bool,
    tools_open: bool,
    app_icon: Option<egui::TextureHandle>,
    history_query: String,
    history_filter: usize,
    history_selected: usize,
    history_cleared: bool,
    history_repolished: bool,
    vocab: VocabState,
    style_selection_workflow: bool,
    style_selected: usize,
    style_editor_open: bool,
    style_prompt: String,
    style_notice: Option<String>,
    marketplace: MarketplaceState,
    settings: SettingsState,
    settings_open: bool,
    history_audio_playing: bool,
    history_style_picker_open: bool,
    qa_save_history: bool,
    translation_working_languages: Vec<String>,
    translation_target_language: String,
}

#[derive(Clone)]
struct VocabEntry {
    phrase: String,
    hits: usize,
    enabled: bool,
    learned: bool,
}

#[derive(Clone)]
struct CorrectionRule {
    pattern: String,
    replacement: String,
    enabled: bool,
    learned: bool,
}

#[derive(Clone)]
struct SavedVocabPreset {
    name: String,
    phrases: String,
}

struct VocabState {
    entries: Vec<VocabEntry>,
    rules: Vec<CorrectionRule>,
    input: String,
    pattern: String,
    replacement: String,
    preset_name: String,
    preset_phrases: String,
    selected_presets: Vec<usize>,
    editing_preset: Option<usize>,
    saved_presets: Vec<SavedVocabPreset>,
    presets_open: bool,
    corrections_open: bool,
    entries_open: bool,
    error: Option<String>,
}

impl Default for VocabState {
    fn default() -> Self {
        Self {
            entries: [
                ("LLM", 8),
                ("macOS", 8),
                ("openless", 4),
                ("iOS", 3),
                ("GitHub", 3),
                ("Codex", 2),
                ("Cloud", 2),
                ("Hello.", 1),
                ("A1003", 1),
                ("SVG", 1),
                ("TTC", 0),
                ("Swift", 0),
                ("LLMAPI", 0),
                ("TypeLazyWordsForm", 0),
                ("Meta", 0),
                ("Beta", 0),
                ("How", 0),
                ("Request", 0),
                ("Pull", 0),
                ("Table", 0),
                ("README", 0),
                ("issue", 0),
                ("PNG", 0),
                ("coding", 0),
                ("Web", 0),
                ("QQ", 0),
                ("Claude", 0),
            ]
            .into_iter()
            .map(|(phrase, hits)| VocabEntry {
                phrase: phrase.into(),
                hits,
                enabled: true,
                learned: false,
            })
            .collect(),
            rules: vec![
                CorrectionRule {
                    pattern: "{num}粒".into(),
                    replacement: "{num}例".into(),
                    enabled: true,
                    learned: false,
                },
                CorrectionRule {
                    pattern: "扣德克斯".into(),
                    replacement: "Codex".into(),
                    enabled: true,
                    learned: true,
                },
            ],
            input: String::new(),
            pattern: String::new(),
            replacement: String::new(),
            preset_name: String::new(),
            preset_phrases: String::new(),
            selected_presets: Vec::new(),
            editing_preset: None,
            saved_presets: vec![
                SavedVocabPreset {
                    name: "程序员".into(),
                    phrases: "PR, CI, tag, release, issue, Rust, TypeScript, Claude, Codex, Copilot, Cursor, Windsurf, Anthropic, OpenAI, GPT, ChatGPT, Gemini, DeepSeek".into(),
                },
                SavedVocabPreset {
                    name: "厨师".into(),
                    phrases: "出品, 备料, 火候, 刀工, 摆盘, sous vide".into(),
                },
                SavedVocabPreset {
                    name: "公务员".into(),
                    phrases: "公文, 批示, 督办, 政务, 会签, 材料".into(),
                },
            ],
            presets_open: false,
            corrections_open: false,
            entries_open: true,
            error: None,
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    Overview,
    History,
    Vocab,
    Style,
    Marketplace,
    Settings,
    SelectionAsk,
    Translation,
}

#[derive(Clone, Copy)]
enum IconName {
    Overview,
    History,
    Vocab,
    Style,
    SelectionAsk,
    Translation,
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
    Play,
    ChevronDown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StyleCardAction {
    None,
    Activate,
    Export,
    Edit,
}

fn window_icon() -> Arc<egui::IconData> {
    let image = image::load_from_memory(include_bytes!("../assets/AppIcon.png"))
        .expect("OpenLess AppIcon.png must be a valid PNG")
        .into_rgba8();
    Arc::new(egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

fn paint_app_icon(ui: &egui::Ui, rect: egui::Rect, texture: Option<&egui::TextureHandle>) {
    if let Some(texture) = texture {
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

impl eframe::App for MainWindow {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                if self.app_icon.is_none() {
                    let image = image::load_from_memory(include_bytes!("../assets/AppIcon.png"))
                        .expect("OpenLess AppIcon.png must be a valid PNG")
                        .into_rgba8();
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    );
                    self.app_icon = Some(ctx.load_texture(
                        "openless-app-icon",
                        color_image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                let app_icon = self.app_icon.clone();
                let window = ui.max_rect().shrink(6.0);
                let radius = egui::CornerRadius::same(14);
                ui.painter().rect_filled(window, radius, theme::SURFACE);
                let body = egui::Rect::from_min_max(
                    window.min + egui::vec2(0.0, TITLEBAR_HEIGHT),
                    window.max,
                );
                ui.painter().rect_filled(
                    body,
                    egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 14,
                        se: 14,
                    },
                    theme::CANVAS,
                );
                ui.painter().rect_stroke(
                    window,
                    radius,
                    egui::Stroke::new(1.0, theme::LINE),
                    egui::StrokeKind::Inside,
                );
                self.titlebar(ui, window, app_icon.as_ref());

                let sidebar = egui::Rect::from_min_max(
                    body.min,
                    egui::pos2(body.min.x + SIDEBAR_WIDTH, body.max.y),
                );
                let main =
                    egui::Rect::from_min_max(egui::pos2(sidebar.max.x + 1.0, body.min.y), body.max);
                ui.painter().line_segment(
                    [
                        egui::pos2(sidebar.max.x, body.min.y),
                        egui::pos2(sidebar.max.x, body.max.y),
                    ],
                    egui::Stroke::new(1.0, theme::LINE),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(sidebar), |ui| {
                    self.sidebar(ui, app_icon.as_ref())
                });
                let content_rect = egui::Rect::from_min_max(
                    egui::pos2(main.left() + 28.0, main.top()),
                    egui::pos2(main.right() - 2.0, main.bottom() - 8.0),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(content_rect));
                    let scroll = &mut ui.style_mut().spacing.scroll;
                    scroll.floating = true;
                    scroll.bar_width = 8.0;
                    scroll.handle_min_length = 24.0;
                    scroll.bar_inner_margin = 0.0;
                    scroll.bar_outer_margin = 0.0;
                    scroll.foreground_color = false;
                    scroll.floating_width = 6.0;
                    scroll.floating_allocated_width = 0.0;
                    let visuals = &mut ui.style_mut().visuals.widgets;
                    visuals.inactive.corner_radius = egui::CornerRadius::same(6);
                    visuals.hovered.corner_radius = egui::CornerRadius::same(6);
                    visuals.active.corner_radius = egui::CornerRadius::same(6);
                    self.content(ui, body);
                });
                self.resize_handles(ui, window);
                if self.settings_open && self.settings.ui(ctx, body) {
                    self.settings_open = false;
                    self.active = Tab::Overview;
                }
            });
    }
}

impl MainWindow {
    fn titlebar(
        &self,
        ui: &mut egui::Ui,
        window: egui::Rect,
        app_icon: Option<&egui::TextureHandle>,
    ) {
        let titlebar = egui::Rect::from_min_max(
            window.min,
            egui::pos2(window.max.x, window.min.y + TITLEBAR_HEIGHT),
        );
        let drag = ui.interact(
            titlebar,
            ui.id().with("titlebar-drag"),
            egui::Sense::click_and_drag(),
        );
        // Start the native window move only once. Re-sending StartDrag on
        // every `dragged` frame can leave the pointer in a stuck grab state
        // after the mouse button is released.
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let logo = egui::pos2(titlebar.min.x + 16.0, titlebar.center().y);
        paint_app_icon(
            ui,
            egui::Rect::from_center_size(logo, egui::vec2(18.0, 18.0)),
            app_icon,
        );
        ui.painter().text(
            egui::pos2(titlebar.min.x + 34.0, titlebar.center().y + 0.5),
            egui::Align2::LEFT_CENTER,
            "OpenLess",
            egui::FontId::proportional(13.0),
            theme::INK_2,
        );

        let button_width = 40.0;
        let close = egui::Rect::from_min_max(
            egui::pos2(titlebar.max.x - button_width, titlebar.min.y),
            titlebar.max,
        );
        let maximize = close.translate(egui::vec2(-button_width, 0.0));
        let minimize = maximize.translate(egui::vec2(-button_width, 0.0));
        let close_response = ui.interact(close, ui.id().with("close"), egui::Sense::click());
        let maximize_response =
            ui.interact(maximize, ui.id().with("maximize"), egui::Sense::click());
        let minimize_response =
            ui.interact(minimize, ui.id().with("minimize"), egui::Sense::click());
        if close_response.clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if maximize_response.clicked() {
            let maximized = ui
                .ctx()
                .input(|input| input.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if minimize_response.clicked() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        for (rect, response) in [
            (minimize, &minimize_response),
            (maximize, &maximize_response),
            (close, &close_response),
        ] {
            if response.hovered() {
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(6), theme::SURFACE_2);
            }
        }
        ui.painter().line_segment(
            [
                egui::pos2(minimize.center().x - 5.0, minimize.center().y),
                egui::pos2(minimize.center().x + 5.0, minimize.center().y),
            ],
            egui::Stroke::new(1.0, theme::INK_3),
        );
        ui.painter().rect_stroke(
            maximize.shrink(14.0),
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.0, theme::INK_3),
            egui::StrokeKind::Inside,
        );
        ui.painter().line_segment(
            [
                egui::pos2(close.center().x - 5.0, close.center().y - 5.0),
                egui::pos2(close.center().x + 5.0, close.center().y + 5.0),
            ],
            egui::Stroke::new(1.0, theme::INK_3),
        );
        ui.painter().line_segment(
            [
                egui::pos2(close.center().x + 5.0, close.center().y - 5.0),
                egui::pos2(close.center().x - 5.0, close.center().y + 5.0),
            ],
            egui::Stroke::new(1.0, theme::INK_3),
        );
    }

    fn resize_handles(&self, ui: &mut egui::Ui, window: egui::Rect) {
        // Borderless windows do not get native resize hit areas from winit.
        // Keep the hit areas small and outside the content padding, then let
        // the platform perform the actual resize.
        let edge = 10.0;
        let corner = 18.0;
        let left = window.left();
        let right = window.right();
        let top = window.top();
        let bottom = window.bottom();
        let zones = [
            (
                egui::Rect::from_min_max(
                    egui::pos2(left, top),
                    egui::pos2(left + corner, top + corner),
                ),
                egui::ResizeDirection::NorthWest,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(right - corner, top),
                    egui::pos2(right, top + corner),
                ),
                egui::ResizeDirection::NorthEast,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(left, bottom - corner),
                    egui::pos2(left + corner, bottom),
                ),
                egui::ResizeDirection::SouthWest,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(right - corner, bottom - corner),
                    egui::pos2(right, bottom),
                ),
                egui::ResizeDirection::SouthEast,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(left + corner, top),
                    egui::pos2(right - corner, top + edge),
                ),
                egui::ResizeDirection::North,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(left + corner, bottom - edge),
                    egui::pos2(right - corner, bottom),
                ),
                egui::ResizeDirection::South,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(left, top + corner),
                    egui::pos2(left + edge, bottom - corner),
                ),
                egui::ResizeDirection::West,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(right - edge, top + corner),
                    egui::pos2(right, bottom - corner),
                ),
                egui::ResizeDirection::East,
            ),
        ];
        for (index, (rect, direction)) in zones.into_iter().enumerate() {
            let response = ui.interact(rect, ui.id().with(("resize", index)), egui::Sense::drag());
            if response.drag_started() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
            }
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, app_icon: Option<&egui::TextureHandle>) {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(10, 12))
            .show(ui, |ui| {
                ui.set_width(SIDEBAR_WIDTH - 20.0);
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(20.0, 22.0), egui::Sense::hover());
                    paint_app_icon(ui, rect, app_icon);
                    ui.label(egui::RichText::new("OpenLess").strong().size(14.0));
                });
                ui.add_space(16.0);
                self.nav(ui, "概览", Tab::Overview);
                self.nav(ui, "历史", Tab::History);
                self.nav(ui, "词汇表", Tab::Vocab);
                ui.add_space(4.0);
                Self::group(ui, "风格", IconName::Style, &mut self.style_open);
                if self.style_open {
                    self.subnav(ui, "润色模式", Tab::Style);
                    self.subnav(ui, "风格市场", Tab::Marketplace);
                }
                Self::group(ui, "工具", IconName::SelectionAsk, &mut self.tools_open);
                if self.tools_open {
                    self.subnav(ui, "划词追问", Tab::SelectionAsk);
                    self.subnav(ui, "翻译", Tab::Translation);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    self.nav_with_icon(ui, "设置", Tab::Settings, IconName::Settings);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            egui::Frame::new()
                                .fill(theme::BLUE_SOFT)
                                .corner_radius(egui::CornerRadius::same(7))
                                .inner_margin(egui::Margin::symmetric(6, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("BETA")
                                            .size(9.5)
                                            .strong()
                                            .color(theme::BLUE),
                                    );
                                });
                            ui.add_space(3.0);
                            ui.label(
                                egui::RichText::new("版本 1.3.18")
                                    .size(10.5)
                                    .color(theme::INK_4),
                            );
                        });
                    });
                });
            });
    }

    fn nav(&mut self, ui: &mut egui::Ui, label: &str, tab: Tab) {
        self.nav_with_icon(ui, label, tab, Self::nav_icon(tab));
    }

    fn nav_with_icon(&mut self, ui: &mut egui::Ui, label: &str, tab: Tab, icon: IconName) {
        let active = self.active == tab;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 32.0), egui::Sense::click());
        if active {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
        }
        let color = if active { theme::INK } else { theme::INK_3 };
        Self::draw_icon(ui, rect.min + egui::vec2(20.0, 16.0), icon, color);
        ui.painter().text(
            rect.min + egui::vec2(38.0, 16.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            color,
        );
        if response.clicked() {
            self.active = tab;
            if tab == Tab::Settings {
                self.settings_open = true;
            }
        }
    }

    fn subnav(&mut self, ui: &mut egui::Ui, label: &str, tab: Tab) {
        let active = self.active == tab;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 30.0), egui::Sense::click());
        if active {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
        }
        ui.painter().text(
            rect.min + egui::vec2(30.0, 15.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.5),
            if active { theme::INK } else { theme::INK_3 },
        );
        if response.clicked() {
            self.active = tab;
        }
    }

    fn group(ui: &mut egui::Ui, label: &str, icon: IconName, open: &mut bool) {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 32.0), egui::Sense::click());
        let color = if response.hovered() {
            theme::INK_2
        } else {
            theme::INK_3
        };
        Self::draw_icon(ui, rect.min + egui::vec2(20.0, 16.0), icon, color);
        ui.painter().text(
            rect.min + egui::vec2(38.0, 16.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            color,
        );
        let x = rect.max.x - 18.0;
        let y = rect.center().y;
        if *open {
            ui.painter().line_segment(
                [egui::pos2(x - 3.0, y - 1.0), egui::pos2(x, y + 2.0)],
                egui::Stroke::new(1.2, color),
            );
            ui.painter().line_segment(
                [egui::pos2(x, y + 2.0), egui::pos2(x + 3.0, y - 1.0)],
                egui::Stroke::new(1.2, color),
            );
        } else {
            ui.painter().line_segment(
                [egui::pos2(x - 1.0, y - 3.0), egui::pos2(x + 2.0, y)],
                egui::Stroke::new(1.2, color),
            );
            ui.painter().line_segment(
                [egui::pos2(x + 2.0, y), egui::pos2(x - 1.0, y + 3.0)],
                egui::Stroke::new(1.2, color),
            );
        }
        if response.clicked() {
            *open = !*open;
        }
    }

    fn nav_icon(tab: Tab) -> IconName {
        match tab {
            Tab::Overview => IconName::Overview,
            Tab::History => IconName::History,
            Tab::Vocab => IconName::Vocab,
            Tab::Style | Tab::Marketplace => IconName::Style,
            Tab::SelectionAsk => IconName::SelectionAsk,
            Tab::Translation => IconName::Translation,
            Tab::Settings => IconName::Settings,
        }
    }

    fn draw_icon(ui: &egui::Ui, center: egui::Pos2, icon: IconName, color: egui::Color32) {
        let p = ui.painter();
        let stroke = egui::Stroke::new(1.25, color);
        match icon {
            IconName::Overview => {
                // Original Icon.tsx `overview`: M3 3v18h18M18 17V9M13 17V5M8 17v-3.
                let s = 2.0 / 3.0;
                p.add(egui::Shape::line(
                    [
                        center + egui::vec2(-9.0 * s, -9.0 * s),
                        center + egui::vec2(-9.0 * s, 9.0 * s),
                        center + egui::vec2(9.0 * s, 9.0 * s),
                    ]
                    .to_vec(),
                    stroke,
                ));
                for (x, top) in [(6.0, 9.0), (1.0, 5.0), (-4.0, 14.0)] {
                    p.line_segment(
                        [
                            center + egui::vec2(x * s, 5.0 * s),
                            center + egui::vec2(x * s, (top - 12.0) * s),
                        ],
                        stroke,
                    );
                }
            }
            IconName::History | IconName::Clock => {
                p.circle_stroke(center, 6.0, stroke);
                p.line_segment([center, center + egui::vec2(0.0, -3.5)], stroke);
                p.line_segment([center, center + egui::vec2(3.0, 2.0)], stroke);
            }
            IconName::Search => {
                // Original Icon.tsx `search`: M11 19a8 8 0 1 0 0-16 8 8
                // 0 0 0 0 16zM21 21l-4.35-4.35.
                let s = 0.5;
                let stroke = egui::Stroke::new(1.0, color);
                p.circle_stroke(center + egui::vec2(-0.5, -0.5), 4.0, stroke);
                p.line_segment(
                    [
                        center + egui::vec2(4.65 * s, 4.65 * s),
                        center + egui::vec2(9.0 * s, 9.0 * s),
                    ],
                    stroke,
                );
            }
            IconName::Trash => {
                // Original Icon.tsx `trash` path, scaled to the compact
                // history header button.
                let s = 0.54;
                let stroke = egui::Stroke::new(1.0, color);
                let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
                p.line_segment([pt(3.0, 6.0), pt(21.0, 6.0)], stroke);
                p.line_segment([pt(19.0, 6.0), pt(19.0, 20.0)], stroke);
                p.line_segment([pt(19.0, 20.0), pt(17.0, 22.0)], stroke);
                p.line_segment([pt(17.0, 22.0), pt(7.0, 22.0)], stroke);
                p.line_segment([pt(7.0, 22.0), pt(5.0, 20.0)], stroke);
                p.line_segment([pt(5.0, 20.0), pt(5.0, 6.0)], stroke);
                p.line_segment([pt(8.0, 6.0), pt(8.0, 4.0)], stroke);
                p.add(egui::Shape::QuadraticBezier(
                    egui::epaint::QuadraticBezierShape::from_points_stroke(
                        [pt(8.0, 4.0), pt(8.0, 2.0), pt(10.0, 2.0)],
                        false,
                        egui::Color32::TRANSPARENT,
                        stroke,
                    ),
                ));
                p.line_segment([pt(10.0, 2.0), pt(14.0, 2.0)], stroke);
                p.add(egui::Shape::QuadraticBezier(
                    egui::epaint::QuadraticBezierShape::from_points_stroke(
                        [pt(14.0, 2.0), pt(16.0, 2.0), pt(16.0, 4.0)],
                        false,
                        egui::Color32::TRANSPARENT,
                        stroke,
                    ),
                ));
                p.line_segment([pt(16.0, 4.0), pt(16.0, 6.0)], stroke);
                p.line_segment([pt(10.0, 11.0), pt(10.0, 17.0)], stroke);
                p.line_segment([pt(14.0, 11.0), pt(14.0, 17.0)], stroke);
            }
            IconName::Refresh => {
                // Original Icon.tsx `refresh` path.
                let s = 0.54;
                let stroke = egui::Stroke::new(1.0, color);
                let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
                // `M3 12a9 9 0 1 0 9-9`: sample the same 270-degree arc
                // from the left side through the bottom and right to top.
                let arc = (0..=24)
                    .map(|step| {
                        let t = step as f32 / 24.0;
                        let angle = std::f32::consts::PI
                            - t * (std::f32::consts::PI + std::f32::consts::FRAC_PI_2);
                        center + egui::vec2(angle.cos() * 9.0 * s, angle.sin() * 9.0 * s)
                    })
                    .collect::<Vec<_>>();
                p.add(egui::Shape::line(arc, stroke));
                // Keep the source arrowhead separate from the arc at this
                // compact size. This preserves the visible opening that the
                // SVG has around its upper-left corner instead of turning
                // into a closed ring in a 13px toolbar button.
                p.line_segment([pt(3.0, 3.0), pt(3.0, 8.0)], stroke);
                p.line_segment([pt(3.0, 3.0), pt(8.0, 3.0)], stroke);
            }
            IconName::Download => {
                // Original Icon.tsx `download` path.
                let s = 0.54;
                let stroke = egui::Stroke::new(1.0, color);
                let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
                p.add(egui::Shape::line(
                    [
                        pt(21.0, 15.0),
                        pt(21.0, 19.0),
                        pt(19.0, 21.0),
                        pt(5.0, 21.0),
                        pt(3.0, 19.0),
                        pt(3.0, 15.0),
                    ]
                    .to_vec(),
                    stroke,
                ));
                p.add(egui::Shape::line(
                    [pt(7.0, 10.0), pt(12.0, 15.0), pt(17.0, 10.0)].to_vec(),
                    stroke,
                ));
                p.line_segment([pt(12.0, 15.0), pt(12.0, 3.0)], stroke);
            }
            IconName::Play => {
                // Original Icon.tsx `play` path.
                let s = 0.54;
                let stroke = egui::Stroke::new(1.0, color);
                let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
                p.add(egui::Shape::line(
                    [pt(5.0, 3.0), pt(19.0, 12.0), pt(5.0, 21.0), pt(5.0, 3.0)].to_vec(),
                    stroke,
                ));
            }
            IconName::ChevronDown => {
                // Original Icon.tsx `chevDown` path.
                let s = 0.54;
                let stroke = egui::Stroke::new(1.0, color);
                let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
                p.add(egui::Shape::line(
                    [pt(6.0, 9.0), pt(12.0, 15.0), pt(18.0, 9.0)].to_vec(),
                    stroke,
                ));
            }
            IconName::Vocab => {
                // Original Icon.tsx `vocab` path, including its four rounded
                // page corners: M4 19.5A2.5 2.5 0 0 1 6.5 17H20 ...
                let s = 2.0 / 3.0;
                let point = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
                p.add(egui::Shape::QuadraticBezier(
                    egui::epaint::QuadraticBezierShape::from_points_stroke(
                        [point(4.0, 19.5), point(4.0, 17.0), point(6.5, 17.0)],
                        false,
                        egui::Color32::TRANSPARENT,
                        stroke,
                    ),
                ));
                p.add(egui::Shape::line(
                    [point(6.5, 17.0), point(20.0, 17.0)].to_vec(),
                    stroke,
                ));
                p.add(egui::Shape::QuadraticBezier(
                    egui::epaint::QuadraticBezierShape::from_points_stroke(
                        [point(4.0, 19.5), point(4.0, 22.0), point(6.5, 22.0)],
                        false,
                        egui::Color32::TRANSPARENT,
                        stroke,
                    ),
                ));
                p.add(egui::Shape::line(
                    [
                        point(6.5, 22.0),
                        point(20.0, 22.0),
                        point(20.0, 4.0),
                        point(6.5, 4.0),
                    ]
                    .to_vec(),
                    stroke,
                ));
                p.add(egui::Shape::QuadraticBezier(
                    egui::epaint::QuadraticBezierShape::from_points_stroke(
                        [point(6.5, 4.0), point(4.0, 4.0), point(4.0, 6.5)],
                        false,
                        egui::Color32::TRANSPARENT,
                        stroke,
                    ),
                ));
                p.line_segment([point(4.0, 6.5), point(4.0, 19.5)], stroke);
            }
            IconName::Style => {
                // Copied from the original Icon.tsx `style` path, scaled from
                // its 24x24 viewBox to the sidebar icon size.
                let s = 2.0 / 3.0;
                p.line_segment(
                    [
                        center + egui::vec2(0.0, -10.0 * s),
                        center + egui::vec2(0.0, 10.0 * s),
                    ],
                    stroke,
                );
                p.add(egui::Shape::line(
                    [
                        center + egui::vec2(5.0 * s, -7.0 * s),
                        center + egui::vec2(-2.5 * s, -7.0 * s),
                        center + egui::vec2(-5.5 * s, -5.0 * s),
                        center + egui::vec2(-5.5 * s, -1.5 * s),
                        center + egui::vec2(-3.5 * s, 1.5 * s),
                        center + egui::vec2(3.0 * s, 1.5 * s),
                        center + egui::vec2(5.0 * s, 3.5 * s),
                        center + egui::vec2(4.0 * s, 6.0 * s),
                        center + egui::vec2(1.0 * s, 7.0 * s),
                        center + egui::vec2(-6.0 * s, 7.0 * s),
                    ]
                    .to_vec(),
                    stroke,
                ));
            }
            IconName::SelectionAsk => {
                p.rect_stroke(
                    egui::Rect::from_center_size(
                        center + egui::vec2(0.0, -1.0),
                        egui::vec2(13.0, 10.0),
                    ),
                    egui::CornerRadius::same(2),
                    stroke,
                    egui::StrokeKind::Inside,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-2.0, 4.0),
                        center + egui::vec2(-5.0, 7.0),
                    ],
                    stroke,
                );
            }
            IconName::Translation => {
                // Original Icon.tsx `translate`: globe with two meridians.
                p.circle_stroke(center, 6.7, stroke);
                p.line_segment(
                    [
                        center + egui::vec2(-6.7, 0.0),
                        center + egui::vec2(6.7, 0.0),
                    ],
                    stroke,
                );
                for sign in [-1.0, 1.0] {
                    p.add(egui::Shape::line(
                        [
                            center + egui::vec2(0.0, -6.7),
                            center + egui::vec2(sign * 2.8, -4.0),
                            center + egui::vec2(sign * 3.3, 0.0),
                            center + egui::vec2(sign * 2.8, 4.0),
                            center + egui::vec2(0.0, 6.7),
                        ]
                        .to_vec(),
                        stroke,
                    ));
                }
            }
            IconName::Mic => {
                // Original web icon: `M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z...`.
                p.rect_stroke(
                    egui::Rect::from_center_size(
                        center + egui::vec2(0.0, -2.0),
                        egui::vec2(7.0, 11.0),
                    ),
                    egui::CornerRadius::same(4),
                    stroke,
                    egui::StrokeKind::Inside,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-4.0, -2.0),
                        center + egui::vec2(-4.0, 1.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(4.0, -2.0),
                        center + egui::vec2(4.0, 1.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-4.0, 1.0),
                        center + egui::vec2(4.0, 1.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [center + egui::vec2(0.0, 1.0), center + egui::vec2(0.0, 5.0)],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-3.0, 5.0),
                        center + egui::vec2(3.0, 5.0),
                    ],
                    stroke,
                );
            }
            IconName::Sparkle => {
                p.line_segment(
                    [
                        center + egui::vec2(0.0, -7.0),
                        center + egui::vec2(2.5, -2.5),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(2.5, -2.5),
                        center + egui::vec2(7.0, 0.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [center + egui::vec2(7.0, 0.0), center + egui::vec2(2.5, 2.5)],
                    stroke,
                );
                p.line_segment(
                    [center + egui::vec2(2.5, 2.5), center + egui::vec2(0.0, 7.0)],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(0.0, 7.0),
                        center + egui::vec2(-2.5, 2.5),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-2.5, 2.5),
                        center + egui::vec2(-7.0, 0.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-7.0, 0.0),
                        center + egui::vec2(-2.5, -2.5),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-2.5, -2.5),
                        center + egui::vec2(0.0, -7.0),
                    ],
                    stroke,
                );
            }
            IconName::Hash => {
                p.line_segment(
                    [
                        center + egui::vec2(-6.0, -3.0),
                        center + egui::vec2(6.0, -3.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-6.0, 3.0),
                        center + egui::vec2(6.0, 3.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-2.0, -7.0),
                        center + egui::vec2(-4.0, 7.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(4.0, -7.0),
                        center + egui::vec2(2.0, 7.0),
                    ],
                    stroke,
                );
            }
            IconName::Bolt => {
                p.line_segment(
                    [
                        center + egui::vec2(1.0, -8.0),
                        center + egui::vec2(-5.0, 1.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-5.0, 1.0),
                        center + egui::vec2(1.0, 1.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(1.0, 1.0),
                        center + egui::vec2(-1.0, 8.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(-1.0, 8.0),
                        center + egui::vec2(6.0, -1.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(6.0, -1.0),
                        center + egui::vec2(1.0, -1.0),
                    ],
                    stroke,
                );
                p.line_segment(
                    [
                        center + egui::vec2(1.0, -1.0),
                        center + egui::vec2(1.0, -8.0),
                    ],
                    stroke,
                );
            }
            IconName::Copy => {
                p.rect_stroke(
                    egui::Rect::from_center_size(
                        center + egui::vec2(1.5, 1.5),
                        egui::vec2(10.0, 12.0),
                    ),
                    egui::CornerRadius::same(1),
                    stroke,
                    egui::StrokeKind::Inside,
                );
                p.rect_stroke(
                    egui::Rect::from_center_size(
                        center + egui::vec2(-1.5, -2.5),
                        egui::vec2(8.0, 5.0),
                    ),
                    egui::CornerRadius::same(1),
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
            IconName::Settings => {
                // The original settings icon is a toothed outline. Keep its
                // small 24x24 silhouette instead of the previous sun-like
                // four-spoke approximation.
                p.circle_stroke(center, 4.5, stroke);
                for angle in [
                    0.0,
                    std::f32::consts::FRAC_PI_4,
                    std::f32::consts::FRAC_PI_2,
                    3.0 * std::f32::consts::FRAC_PI_4,
                    std::f32::consts::PI,
                    5.0 * std::f32::consts::FRAC_PI_4,
                    3.0 * std::f32::consts::FRAC_PI_2,
                    7.0 * std::f32::consts::FRAC_PI_4,
                ] {
                    let direction = egui::vec2(angle.cos(), angle.sin());
                    p.line_segment([center + direction * 5.0, center + direction * 7.0], stroke);
                }
            }
        }
    }

    fn content(&mut self, ui: &mut egui::Ui, body: egui::Rect) {
        // The style-pack page owns its own scroll viewport. Keep the page
        // title and card header fixed while only the pack grid scrolls.
        if self.active == Tab::Style {
            let width = (ui.available_width() - 24.0).max(1.0);
            ui.set_min_width(width);
            ui.set_max_width(width);
            ui.add_space(28.0);
            ui.label(
                egui::RichText::new(self.title())
                    .size(28.0)
                    .strong()
                    .color(theme::INK),
            );
            ui.add_space(22.0);
            self.style_page(ui, width);
            ui.add_space(32.0);
            return;
        }

        // History owns two independent scroll regions (the list and the
        // detail panel). Do not wrap the whole page in another ScrollArea,
        // otherwise both panels move together and the page itself scrolls.
        if self.active == Tab::History {
            let width = (ui.available_width() - 24.0).max(1.0);
            ui.set_min_width(width);
            ui.set_max_width(width);
            ui.add_space(28.0);
            let header_rect = ui
                .allocate_exact_size(egui::vec2(width, 84.0), egui::Sense::hover())
                .0;
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(header_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("HISTORY")
                                    .size(11.0)
                                    .strong()
                                    .color(theme::INK_4),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("历史记录")
                                    .size(28.0)
                                    .strong()
                                    .color(theme::INK),
                            );
                            ui.add_space(5.0);
                            ui.label(
                                egui::RichText::new("本机保存的识别记录。")
                                    .size(13.0)
                                    .color(theme::INK_3),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            let clear = Self::icon_text_button(ui, "清空", IconName::Trash, 70.0);
                            if clear.clicked() {
                                self.history_cleared = true;
                            }
                            ui.add_space(8.0);
                            let refresh =
                                Self::icon_text_button(ui, "刷新", IconName::Refresh, 70.0);
                            if refresh.clicked() {
                                self.history_cleared = false;
                            }
                        });
                    });
                },
            );
            self.history(ui, width);
            ui.add_space(32.0);
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("openless-main-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Keep a permanent right page margin. The scrollbar floats in
                // the outer viewport, so its appearance never changes the
                // width of the cards or makes the page jump sideways.
                let width = (ui.available_width() - 24.0).max(1.0);
                ui.set_min_width(width);
                ui.set_max_width(width);
                if self.active == Tab::Vocab {
                    self.vocab_page(ui, width);
                } else if self.active == Tab::History {
                    // Match the original PageHeader: the page kicker sits
                    // above the title, the local-storage description sits
                    // below it, and the two actions stay at the far right.
                    ui.add_space(28.0);
                    let header_rect = ui
                        .allocate_exact_size(egui::vec2(width, 84.0), egui::Sense::hover())
                        .0;
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(header_rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new("HISTORY")
                                            .size(11.0)
                                            .strong()
                                            .color(theme::INK_4),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new("历史记录")
                                            .size(28.0)
                                            .strong()
                                            .color(theme::INK),
                                    );
                                    ui.add_space(5.0);
                                    ui.label(
                                        egui::RichText::new("本机保存的识别记录。")
                                            .size(13.0)
                                            .color(theme::INK_3),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Min),
                                    |ui| {
                                        let clear = Self::icon_text_button(
                                            ui,
                                            "清空",
                                            IconName::Trash,
                                            70.0,
                                        );
                                        if clear.clicked() {
                                            self.history_cleared = true;
                                        }
                                        ui.add_space(8.0);
                                        let refresh = Self::icon_text_button(
                                            ui,
                                            "刷新",
                                            IconName::Refresh,
                                            70.0,
                                        );
                                        if refresh.clicked() {
                                            self.history_cleared = false;
                                        }
                                    },
                                );
                            });
                        },
                    );
                } else {
                    ui.add_space(28.0);
                    ui.label(
                        egui::RichText::new(self.title())
                            .size(28.0)
                            .strong()
                            .color(theme::INK),
                    );
                    ui.add_space(22.0);
                }
                match self.active {
                    Tab::Overview => self.overview(ui, width),
                    Tab::History => self.history(ui, width),
                    Tab::Vocab => {}
                    Tab::Style => unreachable!("style page is rendered above"),
                    Tab::Marketplace => self.marketplace.ui(ui, body),
                    Tab::SelectionAsk => self.selection_ask(ui, width),
                    Tab::Translation => self.translation(ui, width),
                    Tab::Settings => {}
                }
                // Leave room for the outer rounded body when the content is
                // scrolled all the way down. Without this, the last cards are
                // clipped against the viewport's square bottom edge.
                ui.add_space(32.0);
            });
    }

    fn history(&mut self, ui: &mut egui::Ui, width: f32) {
        let rows = [
            (
                "今天 14:32",
                "你所说的那些没有提交的改动是什么？再详细统一一下。",
                "7.4 秒",
                "轻度润色",
            ),
            (
                "今天 11:08",
                "现在好了，你可以提 PR 了。",
                "2.9 秒",
                "轻度润色",
            ),
            (
                "昨天 18:46",
                "可以了，你现在可以提 PR 了。",
                "3.8 秒",
                "轻度润色",
            ),
            ("8/24 16:20", "92% 的测试已经通过。", "7.5 秒", "清晰结构"),
            (
                "8/24 10:05",
                "Markdown 是一种轻量级标记语言。",
                "3.3 秒",
                "清晰结构",
            ),
        ];
        let filters = ["全部", "原文", "轻度润色", "清晰结构", "正式表达"];
        let gap = 14.0;
        let list_width = 300.0;
        let detail_width = (width - list_width - gap).max(300.0);
        let body_height = ui.available_height().max(300.0);
        let body = ui
            .allocate_exact_size(egui::vec2(width, body_height), egui::Sense::hover())
            .0;
        let list_rect = egui::Rect::from_min_size(body.min, egui::vec2(list_width, body.height()));
        let detail_rect = egui::Rect::from_min_size(
            egui::pos2(body.left() + list_width + gap, body.top()),
            egui::vec2(detail_width, body.height()),
        );
        Self::card_at(ui, list_rect, |ui| {
            ui.add_space(1.0);
            let search_width = ui.available_width();
            egui::Frame::new()
                .fill(theme::SURFACE_2)
                .stroke(egui::Stroke::new(0.8, theme::LINE))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.set_width((search_width - 20.0).max(1.0));
                    ui.horizontal(|ui| {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(egui::vec2(18.0, 24.0), egui::Sense::hover());
                        Self::draw_icon(ui, icon_rect.center(), IconName::Search, theme::INK_3);
                        ui.add_space(6.0);
                        ui.add_sized(
                            [ui.available_width(), 24.0],
                            egui::TextEdit::singleline(&mut self.history_query)
                                .hint_text("搜索转写内容…（Ctrl+K）")
                                .font(egui::FontId::proportional(12.5))
                                .vertical_align(egui::Align::Center)
                                .frame(false),
                        );
                    });
                });
            ui.label(
                egui::RichText::new(format!("共 {} 条记录 · 显示 {} 条", rows.len(), rows.len()))
                    .size(10.5)
                    .color(theme::INK_4),
            );
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for (index, label) in filters.iter().enumerate() {
                    let selected = self.history_filter == index;
                    let filter_width = (label.chars().count() as f32 * 9.0 + 18.0).max(42.0);
                    let response = ui.add(
                        egui::Button::new(egui::RichText::new(*label).size(11.5).color(
                            if selected {
                                theme::SURFACE
                            } else {
                                theme::INK_3
                            },
                        ))
                        .fill(if selected { theme::INK } else { theme::SURFACE })
                        .stroke(egui::Stroke::new(
                            if selected { 0.0 } else { 0.8 },
                            if selected {
                                egui::Color32::TRANSPARENT
                            } else {
                                theme::LINE
                            },
                        ))
                        .corner_radius(egui::CornerRadius::same(10))
                        .min_size(egui::vec2(filter_width, 24.0)),
                    );
                    if response.clicked() {
                        self.history_filter = index;
                    }
                }
            });
            ui.separator();
            let query = self.history_query.to_lowercase();
            egui::ScrollArea::vertical()
                .id_salt("openless-history-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.history_cleared {
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new("暂无历史记录")
                                .size(12.0)
                                .color(theme::INK_4),
                        );
                        return;
                    }
                    for (index, (time, text, duration, tag)) in rows.iter().enumerate() {
                        if !query.is_empty() && !text.to_lowercase().contains(&query) {
                            continue;
                        }
                        let selected = self.history_selected == index;
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 84.0),
                            egui::Sense::click(),
                        );
                        if selected {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(8),
                                theme::BLUE_SOFT,
                            );
                            let indicator_color = egui::Color32::from_rgb(29, 78, 216);
                            let left = rect.left() + 1.0;
                            let right = rect.left() + 4.0;
                            let top = rect.top() + 2.0;
                            let bottom = rect.bottom() - 2.0;
                            let radius = 3.0;
                            let mut indicator = Vec::with_capacity(18);
                            indicator.push(egui::pos2(right, top));
                            indicator.push(egui::pos2(left + radius, top));
                            for step in 0..=6 {
                                let angle = -std::f32::consts::FRAC_PI_2
                                    - std::f32::consts::FRAC_PI_2 * step as f32 / 6.0;
                                indicator.push(egui::pos2(
                                    left + radius + angle.cos() * radius,
                                    top + radius + angle.sin() * radius,
                                ));
                            }
                            indicator.push(egui::pos2(left, bottom - radius));
                            for step in 0..=6 {
                                let angle = std::f32::consts::PI
                                    - std::f32::consts::FRAC_PI_2 * step as f32 / 6.0;
                                indicator.push(egui::pos2(
                                    left + radius + angle.cos() * radius,
                                    bottom - radius + angle.sin() * radius,
                                ));
                            }
                            indicator.push(egui::pos2(right, bottom));
                            ui.painter().add(egui::Shape::convex_polygon(
                                indicator,
                                indicator_color,
                                egui::Stroke::NONE,
                            ));
                        }
                        ui.painter().text(
                            rect.min + egui::vec2(12.0, 14.0),
                            egui::Align2::LEFT_CENTER,
                            *time,
                            egui::FontId::monospace(10.5),
                            theme::INK_3,
                        );
                        ui.painter().text(
                            egui::pos2(rect.right() - 12.0, rect.top() + 14.0),
                            egui::Align2::RIGHT_CENTER,
                            *duration,
                            egui::FontId::monospace(10.0),
                            theme::INK_4,
                        );
                        // Keep the recognition text inside the selectable
                        // row. The original page clamps it to two lines and
                        // adds an ellipsis instead of letting a long string
                        // paint over the row's rounded boundary.
                        let chars_per_line =
                            (((rect.width() - 24.0) / 11.5).floor() as usize).max(1);
                        let chars = text.chars().collect::<Vec<_>>();
                        let first_line = chars.iter().take(chars_per_line).collect::<String>();
                        let mut second_line = chars
                            .iter()
                            .skip(chars_per_line)
                            .take(chars_per_line)
                            .collect::<String>();
                        if chars.len() > chars_per_line * 2 {
                            second_line.pop();
                            second_line.push('…');
                        }
                        ui.painter().text(
                            rect.min + egui::vec2(12.0, 29.0),
                            egui::Align2::LEFT_TOP,
                            first_line,
                            egui::FontId::proportional(11.5),
                            theme::INK_2,
                        );
                        if !second_line.is_empty() {
                            ui.painter().text(
                                rect.min + egui::vec2(12.0, 44.0),
                                egui::Align2::LEFT_TOP,
                                second_line,
                                egui::FontId::proportional(11.5),
                                theme::INK_2,
                            );
                        }
                        Self::tag(
                            ui,
                            // `tag` takes the pill's top-left corner. Keep
                            // its full 20px height inside the rounded row.
                            egui::pos2(rect.min.x + 12.0, rect.bottom() - 23.0),
                            tag,
                            false,
                        );
                        if response.clicked() {
                            self.history_selected = index;
                        }
                        ui.add_space(1.0);
                    }
                });
        });

        Self::card_at(ui, detail_rect, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("openless-history-detail-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let (time, text, duration, tag) =
                        rows[self.history_selected.min(rows.len() - 1)];
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(time).size(12.0).color(theme::INK_3));
                        ui.add_space(8.0);
                        let _ =
                            Self::small_pill(ui, tag, theme::SURFACE_2, theme::LINE, theme::INK_3);
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!("录音 {duration}"))
                                .size(11.0)
                                .color(theme::INK_4),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let _ = Self::icon_text_button(ui, "删除", IconName::Trash, 70.0);
                            ui.add_space(8.0);
                            let _ =
                                Self::icon_text_button(ui, "导出录音", IconName::Download, 92.0);
                        });
                    });
                    ui.add_space(12.0);
                    Self::soft_separator(ui);
                    ui.add_space(10.0);
                    let play = Self::icon_text_button(
                        ui,
                        if self.history_audio_playing {
                            "停止播放"
                        } else {
                            "播放录音"
                        },
                        IconName::Play,
                        92.0,
                    );
                    if play.clicked() {
                        self.history_audio_playing = !self.history_audio_playing;
                    }
                    if self.history_audio_playing {
                        ui.label(
                            egui::RichText::new("正在播放录音…")
                                .size(11.0)
                                .color(theme::BLUE),
                        );
                    }
                    ui.add_space(10.0);
                    for (step, provider, status) in [
                        ("识别", "智谱 GLM-ASR · glm-asr-1", "1.2 秒"),
                        ("润色", "DeepSeek · deepseek-chat", "2.9 秒"),
                        ("插入", "VS Code · 42 字", "已插入"),
                    ] {
                        ui.horizontal(|ui| {
                            let _ = Self::small_pill(
                                ui,
                                step,
                                theme::SURFACE_2,
                                theme::LINE,
                                theme::INK_3,
                            );
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(provider).size(10.5).color(theme::INK_2));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(status).size(10.5).color(theme::INK_4),
                                    );
                                },
                            );
                        });
                        ui.add_space(4.0);
                    }
                    ui.add_space(8.0);
                    let inner_width = ui.available_width();
                    let column_gap = 12.0;
                    let column_width = ((inner_width - column_gap) / 2.0).max(120.0);
                    // Reserve one horizontal row for the two text cards. Allocating
                    // them one after another makes egui place the polished card
                    // underneath the raw card.
                    let cards_row = ui
                        .allocate_exact_size(egui::vec2(inner_width, 165.0), egui::Sense::hover())
                        .0;
                    let raw_rect = egui::Rect::from_min_size(
                        cards_row.min,
                        egui::vec2(column_width, cards_row.height()),
                    );
                    let polished_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            cards_row.left() + column_width + column_gap,
                            cards_row.top(),
                        ),
                        egui::vec2(column_width, cards_row.height()),
                    );
                    self.detail_text_card(ui, raw_rect, "原文", text, false);
                    self.detail_text_card(
                        ui,
                        polished_rect,
                        "轻度润色",
                        "现在好了，你可以提 PR 了。",
                        true,
                    );
                    ui.add_space(16.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("重新润色")
                                .size(12.0)
                                .strong()
                                .color(theme::INK_2),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.history_repolished {
                                let clear = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("清空结果")
                                            .size(12.0)
                                            .color(theme::INK_2),
                                    )
                                    .fill(theme::SURFACE)
                                    .stroke(egui::Stroke::new(0.8, theme::LINE))
                                    .corner_radius(egui::CornerRadius::same(8))
                                    .min_size(egui::vec2(82.0, 30.0)),
                                );
                                if clear.clicked() {
                                    self.history_repolished = false;
                                }
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new(
                            "拿这条历史的原文再跑一次模型，结果只在本次查看时显示。",
                        )
                        .size(10.5)
                        .color(theme::INK_4),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let retry = ui.add(
                            egui::Button::new(
                                egui::RichText::new(if self.history_repolished {
                                    "✓ 已生成"
                                } else {
                                    "↻ 用原风格重试"
                                })
                                .size(12.0),
                            )
                            .fill(theme::SURFACE)
                            .stroke(egui::Stroke::new(0.8, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(112.0, 32.0)),
                        );
                        if retry.clicked() {
                            self.history_repolished = true;
                        }
                        ui.add_space(6.0);
                        let choose = Self::text_chevron_button(ui, "选择风格", 96.0);
                        if choose.clicked() {
                            self.history_style_picker_open = !self.history_style_picker_open;
                        }
                        let _ = ui.add(
                            egui::Button::new(
                                egui::RichText::new("应用").size(12.0).color(theme::INK_2),
                            )
                            .fill(theme::SURFACE)
                            .stroke(egui::Stroke::new(0.8, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(64.0, 32.0)),
                        );
                    });
                    if self.history_style_picker_open {
                        ui.horizontal(|ui| {
                            // Align the vertical menu directly below the
                            // style button, after the retry button and gap.
                            ui.add_space(118.0);
                            egui::Frame::new()
                                .fill(theme::SURFACE)
                                .stroke(egui::Stroke::new(0.8, theme::LINE))
                                .corner_radius(egui::CornerRadius::same(8))
                                .inner_margin(egui::Margin::same(4))
                                .show(ui, |ui| {
                                    ui.set_min_width(88.0);
                                    ui.set_max_width(88.0);
                                    ui.vertical(|ui| {
                                        for style in ["轻度润色", "清晰结构", "正式表达", "原文"]
                                        {
                                            let picked = ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new(style)
                                                        .size(11.5)
                                                        .color(theme::INK_2),
                                                )
                                                .fill(egui::Color32::TRANSPARENT)
                                                .stroke(egui::Stroke::NONE)
                                                .corner_radius(egui::CornerRadius::ZERO)
                                                .min_size(egui::vec2(96.0, 26.0)),
                                            );
                                            if picked.clicked() {
                                                self.history_style_picker_open = false;
                                            }
                                        }
                                    });
                                });
                        });
                    }
                    if self.history_repolished {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new("重试结果：现在好了，你可以提交 PR 了。")
                                .size(11.5)
                                .color(theme::INK_2),
                        );
                    }
                });
        });
    }

    fn small_pill(
        ui: &mut egui::Ui,
        text: &str,
        fill: egui::Color32,
        border: egui::Color32,
        color: egui::Color32,
    ) -> egui::Response {
        let width = (text.chars().count() as f32 * 10.0 + 16.0).max(42.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(9), fill);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(9),
            egui::Stroke::new(0.7, border),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(10.5),
            color,
        );
        response
    }

    fn soft_separator(ui: &mut egui::Ui) {
        let rect = ui
            .allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover())
            .0;
        ui.painter().line_segment(
            [rect.left_center(), rect.right_center()],
            egui::Stroke::new(0.5, egui::Color32::from_rgb(242, 242, 244)),
        );
    }

    fn tag(ui: &egui::Ui, pos: egui::Pos2, text: &str, blue: bool) {
        let width = (text.chars().count() as f32 * 10.0 + 16.0).max(48.0);
        let rect = egui::Rect::from_min_size(pos, egui::vec2(width, 20.0));
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(9),
            if blue {
                theme::BLUE_SOFT
            } else {
                theme::SURFACE_2
            },
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(10.0),
            if blue { theme::BLUE } else { theme::INK_3 },
        );
    }

    fn detail_text_card(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        title: &str,
        text: &str,
        blue: bool,
    ) {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(10),
            if blue {
                theme::BLUE_SOFT
            } else {
                theme::SURFACE_2
            },
        );
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(10),
            egui::Stroke::new(0.5, if blue { theme::BLUE } else { theme::LINE }),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.min + egui::vec2(14.0, 18.0),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(10.5),
            if blue { theme::BLUE } else { theme::INK_3 },
        );
        // Let egui perform normal line wrapping inside the text column. The
        // old painter call used an unbounded single line, so long raw or
        // polished text could run out through the card edge.
        let text_rect = egui::Rect::from_min_max(
            rect.min + egui::vec2(14.0, 42.0),
            rect.max - egui::vec2(14.0, 40.0),
        );
        let text_painter = ui.painter().with_clip_rect(text_rect);
        let galley = text_painter.layout(
            text.to_owned(),
            egui::FontId::proportional(12.5),
            theme::INK_2,
            text_rect.width(),
        );
        text_painter.galley(text_rect.left_top(), galley, theme::INK_2);
        let copy = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 58.0, rect.top() + 8.0),
            egui::vec2(48.0, 22.0),
        );
        ui.painter().rect_stroke(
            copy,
            egui::CornerRadius::same(6),
            egui::Stroke::new(0.5, theme::LINE),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            copy.center(),
            egui::Align2::CENTER_CENTER,
            "复制",
            egui::FontId::proportional(10.5),
            theme::INK_2,
        );
    }

    fn vocab_page(&mut self, ui: &mut egui::Ui, width: f32) {
        let vocab = &mut self.vocab;
        ui.label(egui::RichText::new("词汇表").size(11.0).color(theme::INK_4));
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("词汇表")
                .size(28.0)
                .strong()
                .color(theme::INK),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("自定义热词，提升专有名词识别率")
                    .size(13.0)
                    .color(theme::INK_3),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("↻  刷新")
                                .size(11.5)
                                .color(theme::INK_2),
                        )
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(70.0, 30.0)),
                    )
                    .clicked()
                {
                    vocab.error = None;
                }
            });
        });
        ui.add_space(24.0);

        Self::vocab_card(
            ui,
            width,
            "预设",
            "选择一组常用词汇快速添加。",
            &mut vocab.presets_open,
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["开发工具", "产品与平台", "技术术语", "英文写作"]
                        .iter()
                        .enumerate()
                    {
                        let selected = vocab.selected_presets.contains(&index);
                        let response = ui.add(
                            egui::Button::new(egui::RichText::new(*name).size(12.5))
                                .fill(if selected {
                                    theme::BLUE_SOFT
                                } else {
                                    theme::SURFACE_2
                                })
                                .stroke(egui::Stroke::new(0.5, theme::LINE))
                                .corner_radius(egui::CornerRadius::same(12))
                                .min_size(egui::vec2(88.0, 32.0)),
                        );
                        if response.clicked() {
                            if selected {
                                vocab.selected_presets.retain(|item| *item != index);
                            } else {
                                vocab.selected_presets.push(index);
                            }
                        }
                    }
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("创建预设").size(12.5))
                                .fill(theme::SURFACE)
                                .stroke(egui::Stroke::new(0.5, theme::LINE))
                                .corner_radius(egui::CornerRadius::same(8))
                                .min_size(egui::vec2(96.0, 34.0)),
                        )
                        .clicked()
                    {
                        vocab.editing_preset = Some(usize::MAX);
                        vocab.preset_name = "新预设".into();
                        vocab.preset_phrases.clear();
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("应用").color(theme::SURFACE).size(12.0),
                            )
                            .fill(theme::INK)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(64.0, 32.0)),
                        )
                        .clicked()
                    {
                        let additions = ["Rust", "TypeScript", "GitHub", "OpenLess"];
                        for phrase in additions {
                            if !vocab
                                .entries
                                .iter()
                                .any(|entry| entry.phrase.eq_ignore_ascii_case(phrase))
                            {
                                vocab.entries.push(VocabEntry {
                                    phrase: phrase.into(),
                                    hits: 0,
                                    enabled: true,
                                    learned: false,
                                });
                            }
                        }
                    }
                });
                if vocab.editing_preset.is_some() {
                    ui.add_space(10.0);
                    let input_width = ui.available_width();
                    let input_content_width = (input_width - 20.0).max(1.0);
                    egui::Frame::new()
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(input_content_width);
                            ui.add_sized(
                                [input_content_width, 20.0],
                                egui::TextEdit::singleline(&mut vocab.preset_name)
                                    .hint_text("预设名称")
                                    .desired_width(input_content_width)
                                    .frame(false),
                            );
                        });
                    egui::Frame::new()
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(input_content_width);
                            ui.add_sized(
                                [input_content_width, 64.0],
                                egui::TextEdit::multiline(&mut vocab.preset_phrases)
                                    .desired_rows(3)
                                    .desired_width(input_content_width)
                                    .hint_text("词汇，用逗号或换行分隔")
                                    .frame(false),
                            );
                        });
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("保存").color(theme::SURFACE).size(12.0),
                                )
                                .fill(theme::INK)
                                .stroke(egui::Stroke::NONE)
                                .corner_radius(egui::CornerRadius::same(8))
                                .min_size(egui::vec2(72.0, 30.0)),
                            )
                            .clicked()
                        {
                            let phrases = vocab
                                .preset_phrases
                                .split([',', '\n'])
                                .map(str::trim)
                                .filter(|phrase| !phrase.is_empty());
                            let phrases = phrases.map(str::to_owned).collect::<Vec<_>>();
                            for phrase in &phrases {
                                if !vocab
                                    .entries
                                    .iter()
                                    .any(|entry| entry.phrase == phrase.as_str())
                                {
                                    vocab.entries.push(VocabEntry {
                                        phrase: phrase.clone(),
                                        hits: 0,
                                        enabled: true,
                                        learned: false,
                                    });
                                }
                            }
                            let saved = SavedVocabPreset {
                                name: vocab.preset_name.trim().to_owned(),
                                phrases: phrases.join(", "),
                            };
                            if !saved.name.is_empty() {
                                match vocab.editing_preset {
                                    Some(index)
                                        if index != usize::MAX
                                            && index < vocab.saved_presets.len() =>
                                    {
                                        vocab.saved_presets[index] = saved;
                                    }
                                    _ => vocab.saved_presets.push(saved),
                                }
                            }
                            vocab.editing_preset = None;
                        }
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("取消").size(12.0))
                                    .fill(theme::SURFACE)
                                    .stroke(egui::Stroke::new(0.8, theme::LINE))
                                    .corner_radius(egui::CornerRadius::same(8))
                                    .min_size(egui::vec2(72.0, 30.0)),
                            )
                            .clicked()
                        {
                            vocab.editing_preset = None;
                        }
                    });
                }
                if vocab.editing_preset.is_none() && !vocab.saved_presets.is_empty() {
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        let saved_presets = vocab.saved_presets.clone();
                        for (index, preset) in saved_presets.iter().enumerate() {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(format!("编辑  {}", preset.name))
                                            .size(12.5),
                                    )
                                    .fill(theme::SURFACE_2)
                                    .stroke(egui::Stroke::new(0.6, theme::LINE))
                                    .corner_radius(egui::CornerRadius::same(14))
                                    .min_size(egui::vec2(92.0, 30.0)),
                                )
                                .clicked()
                            {
                                vocab.preset_name = preset.name.clone();
                                vocab.preset_phrases = preset.phrases.clone();
                                vocab.editing_preset = Some(index);
                            }
                        }
                    });
                }
            },
        );

        Self::vocab_card(
            ui,
            width,
            "纠错规则",
            "将识别结果中的常见错误自动替换为正确写法。",
            &mut vocab.corrections_open,
            |ui| {
                ui.horizontal(|ui| {
                    let spacing = ui.spacing().item_spacing.x;
                    let add_width = 72.0;
                    let arrow_width = 24.0;
                    let input_width =
                        ((ui.available_width() - add_width - arrow_width - spacing * 3.0) / 2.0)
                            .max(60.0);
                    let input_content_width = (input_width - 20.0).max(1.0);
                    egui::Frame::new()
                        .fill(theme::SURFACE_2)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(input_content_width);
                            ui.add_sized(
                                [input_content_width, 20.0],
                                egui::TextEdit::singleline(&mut vocab.pattern)
                                    .desired_width(input_content_width)
                                    .hint_text("原文，例如：{num}粒")
                                    .frame(false),
                            );
                        });
                    ui.add_sized(
                        [arrow_width, 32.0],
                        egui::Label::new(egui::RichText::new("→").color(theme::INK_4))
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    egui::Frame::new()
                        .fill(theme::SURFACE_2)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(input_content_width);
                            ui.add_sized(
                                [input_content_width, 20.0],
                                egui::TextEdit::singleline(&mut vocab.replacement)
                                    .desired_width(input_content_width)
                                    .hint_text("替换为")
                                    .frame(false),
                            );
                        });
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("添加").color(theme::SURFACE).size(12.0),
                            )
                            .fill(theme::INK)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(add_width, 32.0)),
                        )
                        .clicked()
                        && !vocab.pattern.trim().is_empty()
                    {
                        vocab.rules.insert(
                            0,
                            CorrectionRule {
                                pattern: vocab.pattern.trim().into(),
                                replacement: vocab.replacement.trim().into(),
                                enabled: true,
                                learned: false,
                            },
                        );
                        vocab.pattern.clear();
                        vocab.replacement.clear();
                    }
                });
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    for index in 0..vocab.rules.len() {
                        let rule = &vocab.rules[index];
                        let label = format!(
                            "{} → {}{}",
                            rule.pattern,
                            rule.replacement,
                            if rule.learned { "  自动" } else { "" }
                        );
                        let (toggle, remove) = Self::correction_chip(ui, &label, rule.enabled);
                        if remove {
                            vocab.rules.remove(index);
                            break;
                        }
                        if toggle {
                            vocab.rules[index].enabled = !vocab.rules[index].enabled;
                        }
                    }
                    if vocab.rules.is_empty() {
                        ui.label(
                            egui::RichText::new("暂无纠错规则")
                                .size(12.0)
                                .color(theme::INK_4),
                        );
                    }
                });
            },
        );

        Self::vocab_card(
            ui,
            width,
            "词汇",
            "添加需要优先识别的自定义词汇。",
            &mut vocab.entries_open,
            |ui| {
                ui.horizontal(|ui| {
                    let input_width = (ui.available_width() - 90.0).max(80.0);
                    let input_content_width = (input_width - 20.0).max(1.0);
                    egui::Frame::new()
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(input_content_width);
                            ui.add_sized(
                                [input_content_width, 20.0],
                                egui::TextEdit::singleline(&mut vocab.input)
                                    .desired_width(input_content_width)
                                    .hint_text("输入词汇，按回车添加")
                                    .frame(false),
                            );
                        });
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("＋ 添加")
                                    .color(theme::SURFACE)
                                    .size(12.0),
                            )
                            .fill(theme::INK)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(78.0, 32.0)),
                        )
                        .clicked()
                        || (ui.input(|input| input.key_pressed(egui::Key::Enter))
                            && !vocab.input.trim().is_empty())
                    {
                        let phrase = vocab.input.trim().to_string();
                        if !phrase.is_empty()
                            && !vocab
                                .entries
                                .iter()
                                .any(|entry| entry.phrase.eq_ignore_ascii_case(&phrase))
                        {
                            vocab.entries.push(VocabEntry {
                                phrase,
                                hits: 0,
                                enabled: true,
                                learned: false,
                            });
                        }
                        vocab.input.clear();
                    }
                });
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    for index in (0..vocab.entries.len()).collect::<Vec<_>>() {
                        let entry = &vocab.entries[index];
                        let (toggle, remove) = Self::vocab_chip(ui, entry);
                        if remove {
                            vocab.entries.remove(index);
                            break;
                        }
                        if toggle {
                            vocab.entries[index].enabled = !vocab.entries[index].enabled;
                        }
                    }
                });
                let learned = vocab.entries.iter().filter(|entry| entry.learned).count();
                if learned > 0 {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(format!("自动收集 ({learned})"));
                        if ui.button("全部删除").clicked() {
                            vocab.entries.retain(|entry| !entry.learned);
                        }
                    });
                }
                if let Some(error) = &vocab.error {
                    ui.label(
                        egui::RichText::new(error)
                            .size(12.0)
                            .color(egui::Color32::from_rgb(185, 28, 28)),
                    );
                }
            },
        );
    }

    fn vocab_card(
        ui: &mut egui::Ui,
        width: f32,
        title: &str,
        desc: &str,
        open: &mut bool,
        contents: impl FnOnce(&mut egui::Ui),
    ) {
        let frame = egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::same(17));
        frame.show(ui, |ui| {
            ui.set_width(width - 34.0);
            let header = ui.horizontal(|ui| {
                let (arrow_rect, response) =
                    ui.allocate_exact_size(egui::vec2(20.0, 24.0), egui::Sense::click());
                let arrow_stroke = egui::Stroke::new(1.4, theme::INK_4);
                let center = arrow_rect.center();
                if *open {
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(-4.0, -2.0),
                            center + egui::vec2(0.0, 2.0),
                        ],
                        arrow_stroke,
                    );
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(0.0, 2.0),
                            center + egui::vec2(4.0, -2.0),
                        ],
                        arrow_stroke,
                    );
                } else {
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(-2.0, -4.0),
                            center + egui::vec2(2.0, 0.0),
                        ],
                        arrow_stroke,
                    );
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(2.0, 0.0),
                            center + egui::vec2(-2.0, 4.0),
                        ],
                        arrow_stroke,
                    );
                }
                if response.clicked() {
                    *open = !*open;
                }
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).size(13.0).strong());
                    ui.label(egui::RichText::new(desc).size(11.5).color(theme::INK_4));
                });
            });
            let _ = header;
            if *open {
                ui.add_space(12.0);
                contents(ui);
            }
        });
        ui.add_space(12.0);
    }

    fn correction_chip(ui: &mut egui::Ui, label: &str, enabled: bool) -> (bool, bool) {
        let fill = if enabled {
            theme::SURFACE
        } else {
            theme::SURFACE_2
        };
        let text_color = if enabled { theme::INK } else { theme::INK_4 };
        let text_galley = ui.painter().layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(12.5),
            text_color,
        );
        let close_size = 22.0;
        let width = 12.0 + text_galley.size().x + 8.0 + close_size + 10.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::same(16), fill);
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(16),
            egui::Stroke::new(0.6, theme::LINE),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            egui::pos2(
                rect.left() + 12.0,
                rect.center().y - text_galley.size().y / 2.0,
            ),
            text_galley,
            text_color,
        );

        let close_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 10.0 - close_size / 2.0, rect.center().y),
            egui::vec2(close_size, close_size),
        );
        painter.circle_filled(close_rect.center(), close_size / 2.0, theme::SURFACE_2);
        painter.circle_stroke(
            close_rect.center(),
            close_size / 2.0,
            egui::Stroke::new(0.5, theme::LINE),
        );
        let center = close_rect.center();
        let x_stroke = egui::Stroke::new(1.1, theme::INK_4);
        painter.line_segment(
            [
                center + egui::vec2(-3.0, -3.0),
                center + egui::vec2(3.0, 3.0),
            ],
            x_stroke,
        );
        painter.line_segment(
            [
                center + egui::vec2(3.0, -3.0),
                center + egui::vec2(-3.0, 3.0),
            ],
            x_stroke,
        );

        if response.clicked() {
            if response
                .interact_pointer_pos()
                .is_some_and(|pointer| close_rect.contains(pointer))
            {
                return (false, true);
            }
            return (true, false);
        }
        (false, false)
    }

    fn vocab_chip(ui: &mut egui::Ui, entry: &VocabEntry) -> (bool, bool) {
        let fill = if entry.enabled && entry.hits > 0 {
            theme::BLUE_SOFT
        } else if entry.enabled {
            theme::SURFACE
        } else {
            theme::SURFACE_2
        };
        let text_color = if entry.enabled {
            theme::INK
        } else {
            theme::INK_4
        };
        let phrase_galley = ui.painter().layout_no_wrap(
            entry.phrase.clone(),
            egui::FontId::proportional(13.0),
            text_color,
        );
        let hits_text = entry.hits.to_string();
        let hits_color = if entry.enabled && entry.hits > 0 {
            theme::SURFACE
        } else {
            theme::INK_4
        };
        let hits_galley =
            ui.painter()
                .layout_no_wrap(hits_text, egui::FontId::proportional(11.0), hits_color);
        let hits_size = egui::vec2((hits_galley.size().x + 12.0).max(24.0), 22.0);
        let close_size = 22.0;
        let width = 12.0 + phrase_galley.size().x + 8.0 + hits_size.x + 6.0 + close_size + 10.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::same(16), fill);
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(16),
            egui::Stroke::new(0.6, theme::LINE),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            egui::pos2(
                rect.left() + 12.0,
                rect.center().y - phrase_galley.size().y / 2.0,
            ),
            phrase_galley,
            text_color,
        );

        let close_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 10.0 - close_size / 2.0, rect.center().y),
            egui::vec2(close_size, close_size),
        );
        let hits_rect = egui::Rect::from_min_size(
            egui::pos2(
                close_rect.left() - 6.0 - hits_size.x,
                rect.center().y - hits_size.y / 2.0,
            ),
            hits_size,
        );
        painter.rect_filled(
            hits_rect,
            egui::CornerRadius::same(5),
            if entry.enabled && entry.hits > 0 {
                theme::BLUE
            } else {
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 15)
            },
        );
        painter.galley(
            egui::pos2(
                hits_rect.center().x - hits_galley.size().x / 2.0,
                hits_rect.center().y - hits_galley.size().y / 2.0,
            ),
            hits_galley,
            hits_color,
        );
        painter.circle_filled(close_rect.center(), close_size / 2.0, theme::SURFACE_2);
        painter.circle_stroke(
            close_rect.center(),
            close_size / 2.0,
            egui::Stroke::new(0.5, theme::LINE),
        );
        let center = close_rect.center();
        let x_stroke = egui::Stroke::new(1.1, theme::INK_4);
        painter.line_segment(
            [
                center + egui::vec2(-3.0, -3.0),
                center + egui::vec2(3.0, 3.0),
            ],
            x_stroke,
        );
        painter.line_segment(
            [
                center + egui::vec2(3.0, -3.0),
                center + egui::vec2(-3.0, 3.0),
            ],
            x_stroke,
        );

        if response.clicked() {
            if response
                .interact_pointer_pos()
                .is_some_and(|pointer| close_rect.contains(pointer))
            {
                return (false, true);
            }
            return (true, false);
        }
        (false, false)
    }

    fn style_page(&mut self, ui: &mut egui::Ui, width: f32) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("选择润色风格，让每次输出都保持一致")
                    .size(12.0)
                    .color(theme::INK_3),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let import = ui.add(
                    egui::Button::new(egui::RichText::new("▣  导入 ZIP").size(11.5))
                        .fill(theme::BLUE)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(92.0, 29.0)),
                );
                if import.clicked() {
                    self.style_notice = Some("已载入本地风格包（演示）".into());
                }
                ui.add_space(8.0);
                let refresh = ui.add(
                    egui::Button::new(
                        egui::RichText::new("↻  刷新")
                            .size(11.5)
                            .color(theme::INK_2),
                    )
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(0.7, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(70.0, 29.0)),
                );
                if refresh.clicked() {
                    self.style_notice = Some("风格包列表已刷新".into());
                }
            });
        });
        ui.add_space(14.0);

        let packs = [
            (
                "轻度润色",
                "保留原意和语气，把口语整理得自然顺畅。",
                &["推荐", "自然"][..],
                theme::BLUE,
            ),
            (
                "清晰结构",
                "补齐层次与重点，适合会议记录和工作表达。",
                &["结构", "会议"][..],
                theme::OK,
            ),
            (
                "正式表达",
                "更专业、克制的书面表达，适合对外沟通。",
                &["正式", "商务"][..],
                theme::INK_2,
            ),
            (
                "原样保留",
                "只做必要的断句与格式整理，不改变原文。",
                &["原文", "保留"][..],
                theme::INK_3,
            ),
        ];
        let mut selected_action = None;
        let mut editor_prompt = None;
        let style_card_height = ui.available_height().max(320.0);
        Self::card(ui, egui::vec2(width, style_card_height), |ui| {
            let raw_active = !self.style_selection_workflow && self.style_selected == 3;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("本地风格包").size(15.0).strong());
                        let raw = ui.add(
                            egui::Button::new(egui::RichText::new("原文").size(11.5).color(
                                if raw_active {
                                    theme::SURFACE
                                } else {
                                    theme::INK_3
                                },
                            ))
                            .fill(if raw_active {
                                theme::BLUE
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(6))
                            .min_size(egui::vec2(52.0, 24.0)),
                        );
                        if raw.clicked() {
                            self.style_selection_workflow = false;
                            self.style_selected = 3;
                        }
                    });
                    ui.add_space(3.0);
                    ui.label(
                        egui::RichText::new("浏览和切换风格包。")
                            .size(11.5)
                            .color(theme::INK_3),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // right_to_left starts at the right edge. Add items in reverse
                    // source order so the visible order is 语音润色 / 选区润色 / 数量.
                    egui::Frame::new()
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(7, 3))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("4 个风格包")
                                    .size(10.5)
                                    .color(theme::INK_3),
                            );
                        });
                    let selection = ui.add(
                        egui::Button::new(egui::RichText::new("选区润色").size(11.5).color(
                            if self.style_selection_workflow {
                                theme::SURFACE
                            } else {
                                theme::INK_3
                            },
                        ))
                        .fill(if self.style_selection_workflow {
                            theme::BLUE
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(6))
                        .min_size(egui::vec2(70.0, 24.0)),
                    );
                    if selection.clicked() {
                        self.style_selection_workflow = true;
                    }
                    let dictation = ui.add(
                        egui::Button::new(egui::RichText::new("语音润色").size(11.5).color(
                            if !self.style_selection_workflow && !raw_active {
                                theme::SURFACE
                            } else {
                                theme::INK_3
                            },
                        ))
                        .fill(if !self.style_selection_workflow && !raw_active {
                            theme::BLUE
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(6))
                        .min_size(egui::vec2(70.0, 24.0)),
                    );
                    if dictation.clicked() {
                        self.style_selection_workflow = false;
                    }
                });
            });
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(16.0);

            egui::ScrollArea::vertical()
                .id_salt("style-packs-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let grid_width = ui.available_width();
                    let gap = 12.0;
                    let columns = if grid_width >= 820.0 {
                        3
                    } else if grid_width >= 560.0 {
                        2
                    } else {
                        1
                    };
                    let card_width = ((grid_width - gap * (columns - 1) as f32)
                        / columns as f32)
                        .max(1.0);
                    let total_tiles = packs.len() + 1;
                    for (row_index, start) in (0..total_tiles).step_by(columns).enumerate() {
                        if row_index > 0 {
                            ui.add_space(12.0);
                        }
                        ui.horizontal(|ui| {
                            // `horizontal` adds its own item spacing. The card width above
                            // already accounts for our explicit gap, so disable the implicit
                            // spacing or the last card can run into the outer frame.
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for slot in start..(start + columns).min(total_tiles) {
                                if slot == packs.len() {
                                    if Self::new_style_pack_card(ui, egui::vec2(card_width, 232.0)) {
                                        editor_prompt = Some("# 角色\n你是 OpenLess 的润色助手。\n\n# 任务\n把输入整理成自然、清晰、可直接使用的文字。\n\n# 输出\n只输出最终文本，不添加解释。\n".into());
                                    }
                                    continue;
                                }
                                let (n, d, tags, a) = packs[slot];
                                match Self::style_pack_card(
                                    ui,
                                    egui::vec2(card_width, 232.0),
                                    slot,
                                    self.style_selected,
                                    n,
                                    d,
                                    tags,
                                    a,
                                ) {
                                    StyleCardAction::Activate => selected_action = Some(slot),
                                    StyleCardAction::Export => {
                                        self.style_notice = Some(format!("已导出「{n}」风格包（演示）"));
                                    }
                                    StyleCardAction::Edit => {
                                        editor_prompt = Some(format!("# 角色\n你是 OpenLess 的{n}助手。\n\n# 任务\n把输入整理成自然、清晰、可直接使用的文字。\n\n# 输出\n只输出最终文本，不添加解释。\n"));
                                    }
                                    StyleCardAction::None => {}
                                }
                                if slot + 1 < (start + columns).min(total_tiles) {
                                    ui.add_space(gap);
                                }
                            }
                        });
                    }
                });
        });
        if let Some(index) = selected_action {
            self.style_selected = index;
            self.style_notice = Some(format!("已切换到「{}」", packs[index].0));
        }
        if let Some(prompt) = editor_prompt {
            self.style_prompt = prompt;
            self.style_editor_open = true;
        }

        if let Some(notice) = self.style_notice.clone() {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("✓").color(theme::OK).strong());
                ui.label(egui::RichText::new(notice).size(11.5).color(theme::INK_2));
                if ui.small_button("×").clicked() {
                    self.style_notice = None;
                }
            });
        }
        self.style_editor(ui.ctx());
    }

    fn style_pack_card(
        ui: &mut egui::Ui,
        size: egui::Vec2,
        index: usize,
        selected: usize,
        name: &str,
        description: &str,
        tags: &[&str],
        accent: egui::Color32,
    ) -> StyleCardAction {
        let active = selected == index;
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(14),
            if active || response.hovered() {
                theme::BLUE_SOFT
            } else {
                theme::SURFACE
            },
        );
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(14),
            egui::Stroke::new(
                if active { 1.5 } else { 1.0 },
                if active { theme::BLUE } else { theme::LINE },
            ),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(16.0);
        let mut action = StyleCardAction::None;
        let mut card_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                // The card is created inside a horizontal grid row, so do not
                // inherit the row's layout for the card's own content.
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut card_ui;
        ui.style_mut().interaction.selectable_labels = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(name)
                    .size(14.0)
                    .strong()
                    .color(theme::INK),
            );
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(0.7, accent))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("内置").size(10.5).color(accent));
                });
            if active {
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(theme::INK)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(7, 3))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("当前")
                                .size(10.5)
                                .strong()
                                .color(theme::SURFACE),
                        );
                    });
            }
        });
        ui.add_space(9.0);
        let description_width = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(description_width, 48.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(truncate_text(description, 96))
                            .size(11.5)
                            .color(theme::INK_3),
                    )
                    .wrap(),
                );
            },
        );
        ui.add_space(9.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (tag_index, tag) in tags.iter().enumerate() {
                egui::Frame::new()
                    .fill(if tag_index == 0 {
                        theme::BLUE_SOFT
                    } else {
                        theme::SURFACE_2
                    })
                    .stroke(egui::Stroke::new(
                        0.5,
                        if tag_index == 0 { accent } else { theme::LINE },
                    ))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(7, 3))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(*tag)
                                .size(10.5)
                                .color(if tag_index == 0 { accent } else { theme::INK_3 }),
                        );
                    });
            }
        });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let activate = ui.add_enabled(
                    !active,
                    egui::Button::new(egui::RichText::new("激活").size(10.5))
                        .fill(if active { theme::INK } else { theme::BLUE })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(7))
                        .min_size(egui::vec2(64.0, 24.0)),
                );
                if activate.clicked() {
                    action = StyleCardAction::Activate;
                }
                let export = ui.add(
                    egui::Button::new(egui::RichText::new("导出").size(10.5))
                        .fill(theme::SURFACE_2)
                        .stroke(egui::Stroke::new(0.7, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(7))
                        .min_size(egui::vec2(64.0, 24.0)),
                );
                if export.clicked() {
                    action = StyleCardAction::Export;
                }
                let edit = ui.add_enabled(
                    false,
                    egui::Button::new(egui::RichText::new("编辑").size(10.5))
                        .fill(theme::SURFACE_2)
                        .stroke(egui::Stroke::new(0.7, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(7))
                        .min_size(egui::vec2(64.0, 24.0)),
                );
                if edit.clicked() {
                    action = StyleCardAction::Edit;
                }
            });
        });
        action
    }

    fn new_style_pack_card(ui: &mut egui::Ui, size: egui::Vec2) -> bool {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(14),
            if response.hovered() {
                theme::SURFACE_2
            } else {
                theme::SURFACE
            },
        );
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(14),
            egui::Stroke::new(1.0, theme::LINE),
            egui::StrokeKind::Inside,
        );
        let center = rect.center() - egui::vec2(0.0, 20.0);
        ui.painter().circle_filled(center, 22.0, theme::SURFACE_2);
        let stroke = egui::Stroke::new(1.5, theme::BLUE);
        ui.painter().line_segment(
            [center - egui::vec2(8.0, 0.0), center + egui::vec2(8.0, 0.0)],
            stroke,
        );
        ui.painter().line_segment(
            [center - egui::vec2(0.0, 8.0), center + egui::vec2(0.0, 8.0)],
            stroke,
        );
        ui.painter().text(
            rect.center() + egui::vec2(0.0, 22.0),
            egui::Align2::CENTER_CENTER,
            "新建风格包",
            egui::FontId::proportional(14.0),
            theme::INK_2,
        );
        ui.painter().text(
            rect.center() + egui::vec2(0.0, 45.0),
            egui::Align2::CENTER_CENTER,
            "从模板开始创建自己的风格",
            egui::FontId::proportional(11.0),
            theme::INK_4,
        );
        response.clicked()
    }

    fn style_editor(&mut self, ctx: &egui::Context) {
        if !self.style_editor_open {
            return;
        }
        let mut open = true;
        egui::Window::new("编辑风格包")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .default_height(520.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("轻度润色").size(18.0).strong());
                ui.label(
                    egui::RichText::new(
                        "修改提示词后保存，下一次润色将使用新的规则。双击风格卡即可打开此编辑器。",
                    )
                    .size(11.5)
                    .color(theme::INK_3),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("风格描述").size(12.0).strong());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.style_prompt)
                            .hint_text("简短描述这个风格的使用场景")
                            .desired_width(390.0),
                    );
                });
                ui.add_space(10.0);
                ui.label(egui::RichText::new("润色提示词").size(12.0).strong());
                ui.add(
                    egui::TextEdit::multiline(&mut self.style_prompt)
                        .desired_rows(14)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new("保存")
                                .fill(theme::BLUE)
                                .corner_radius(egui::CornerRadius::same(7)),
                        )
                        .clicked()
                    {
                        self.style_notice = Some("风格包已保存（演示）".into());
                        self.style_editor_open = false;
                    }
                    if ui.button("恢复默认").clicked() {
                        self.style_notice = Some("已恢复默认提示词（演示）".into());
                    }
                    if ui.button("取消").clicked() {
                        self.style_editor_open = false;
                    }
                });
            });
        if !open {
            self.style_editor_open = false;
        }
    }

    fn selection_ask(&mut self, ui: &mut egui::Ui, width: f32) {
        // Matches SelectionAsk.tsx: a compact history control followed by the
        // full-width usage card. Persistence is intentionally local for now.
        let history_width = 142.0;
        let history_rect = ui
            .allocate_exact_size(egui::vec2(history_width, 36.0), egui::Sense::hover())
            .0;
        ui.painter().rect_filled(
            history_rect,
            egui::CornerRadius::same(10),
            egui::Color32::from_rgb(241, 241, 242),
        );
        ui.painter().rect_stroke(
            history_rect,
            egui::CornerRadius::same(10),
            egui::Stroke::new(0.5, theme::LINE),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            history_rect.min + egui::vec2(14.0, 18.0),
            egui::Align2::LEFT_CENTER,
            "保存历史",
            egui::FontId::proportional(12.5),
            theme::INK_2,
        );

        let toggle_rect = egui::Rect::from_min_size(
            egui::pos2(history_rect.right() - 50.0, history_rect.center().y - 10.0),
            egui::vec2(36.0, 20.0),
        );
        let toggle = ui.interact(
            toggle_rect,
            ui.id().with("selection-ask-history"),
            egui::Sense::click(),
        );
        let toggle_color = if self.qa_save_history {
            theme::BLUE
        } else {
            egui::Color32::from_rgb(184, 184, 187)
        };
        ui.painter()
            .rect_filled(toggle_rect, egui::CornerRadius::same(10), toggle_color);
        let knob_x = if self.qa_save_history {
            toggle_rect.right() - 10.0
        } else {
            toggle_rect.left() + 10.0
        };
        ui.painter().circle_filled(
            egui::pos2(knob_x, toggle_rect.center().y),
            8.0,
            egui::Color32::WHITE,
        );
        if toggle.clicked() {
            self.qa_save_history = !self.qa_save_history;
        }
        ui.add_space(12.0);

        Self::card(ui, egui::vec2(width, 196.0), |ui| {
            ui.label(
                egui::RichText::new("使用方法")
                    .size(13.0)
                    .strong()
                    .color(theme::INK),
            );
            ui.add_space(10.0);
            let steps = [
                "按 ⌘⇧; 打开浮窗。",
                "在任意 app 选中文字。",
                "按 右Ctrl 录音，再按一次提交。",
                "可继续按 右Ctrl 多轮追问。",
                "按 Esc 关闭浮窗并清空历史。",
            ];
            for (index, step) in steps.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [18.0, 20.0],
                        egui::Label::new(
                            egui::RichText::new(format!("{}.", index + 1))
                                .size(12.5)
                                .color(theme::INK_3),
                        ),
                    );
                    ui.label(egui::RichText::new(step).size(12.5).color(theme::INK_2));
                });
                if index < 4 {
                    ui.add_space(5.0);
                }
            }
        });
    }

    fn translation(&mut self, ui: &mut egui::Ui, width: f32) {
        let gap = 12.0;
        let card_width = width.min(760.0);

        Self::translation_card(ui, card_width, |ui| {
            ui.label(egui::RichText::new("工作语言").size(13.0).strong());
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for language in SUPPORTED_LANGUAGES {
                    let selected = self
                        .translation_working_languages
                        .iter()
                        .any(|value| value == language);
                    let response = ui.add(
                        egui::Button::new(egui::RichText::new(language).size(12.5).color(
                            if selected {
                                egui::Color32::WHITE
                            } else {
                                theme::INK_2
                            },
                        ))
                        .fill(if selected {
                            theme::BLUE
                        } else {
                            theme::SURFACE_2
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(255))
                        .min_size(egui::vec2(0.0, 28.0)),
                    );
                    if response.clicked() {
                        if selected {
                            self.translation_working_languages
                                .retain(|value| value != language);
                        } else {
                            self.translation_working_languages
                                .push(language.to_string());
                        }
                    }
                }
            });
        });
        ui.add_space(gap);

        let target = self.translation_target_language.clone();
        let redundant = !target.is_empty()
            && self.translation_working_languages.len() == 1
            && self.translation_working_languages[0] == target;
        let enabled = !target.is_empty() && !redundant;
        Self::translation_card(ui, card_width, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("翻译目标语言").size(13.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(if enabled { "已启用" } else { "未启用" })
                            .size(10.5)
                            .strong()
                            .color(if enabled { theme::BLUE } else { theme::INK_4 }),
                    );
                });
            });
            ui.add_space(12.0);
            ui.scope(|ui| {
                let style = ui.style_mut();
                style.visuals.menu_corner_radius = egui::CornerRadius::same(10);
                style.spacing.button_padding = egui::vec2(10.0, 0.0);
                style.spacing.icon_spacing = 8.0;
                style.spacing.icon_width = 11.0;
                for widget in [
                    &mut style.visuals.widgets.inactive,
                    &mut style.visuals.widgets.hovered,
                    &mut style.visuals.widgets.active,
                    &mut style.visuals.widgets.open,
                ] {
                    widget.corner_radius = egui::CornerRadius::same(8);
                    widget.weak_bg_fill = theme::SURFACE;
                    widget.bg_fill = theme::SURFACE;
                    widget.bg_stroke = egui::Stroke::new(0.8, theme::LINE);
                    widget.fg_stroke = egui::Stroke::new(1.0, theme::INK_2);
                }
                egui::ComboBox::from_id_salt("translation-target-language")
                    .width(360.0)
                    .height(32.0)
                    .truncate()
                    .icon(|ui, rect, visuals, _| {
                        let center = rect.center();
                        let stroke = egui::Stroke::new(1.1, visuals.fg_stroke.color);
                        ui.painter()
                            .line_segment([center + egui::vec2(-3.5, -1.5), center], stroke);
                        ui.painter()
                            .line_segment([center, center + egui::vec2(3.5, -1.5)], stroke);
                    })
                    .selected_text(if target.is_empty() {
                        egui::RichText::new("不启用（Shift 按下不触发翻译）").color(theme::INK_4)
                    } else {
                        egui::RichText::new(target.as_str()).color(theme::INK)
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.translation_target_language,
                            String::new(),
                            "不启用（Shift 按下不触发翻译）",
                        );
                        for language in SUPPORTED_LANGUAGES {
                            ui.selectable_value(
                                &mut self.translation_target_language,
                                language.to_string(),
                                language,
                            );
                        }
                    });
            });
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("翻译风格").size(12.0).strong());
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("自动继承“风格”页当前激活的风格包。")
                            .size(11.5)
                            .color(theme::INK_4),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let style_name = match self.style_selected {
                        1 => "清晰结构",
                        2 => "正式表达",
                        3 => "原样保留",
                        _ => "轻度润色",
                    };
                    egui::Frame::new()
                        .fill(theme::BLUE_SOFT)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(9, 4))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(style_name)
                                    .size(11.0)
                                    .strong()
                                    .color(theme::BLUE),
                            );
                        });
                });
            });
            if redundant {
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(217, 119, 6, 20))
                    .stroke(egui::Stroke::new(
                        0.5,
                        egui::Color32::from_rgba_unmultiplied(217, 119, 6, 62),
                    ))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(
                                "目标语言与唯一工作语言相同，按翻译快捷键不会触发翻译。",
                            )
                            .size(11.5)
                            .color(egui::Color32::from_rgb(180, 103, 10)),
                        );
                    });
            }
        });
        ui.add_space(gap);

        Self::translation_card(ui, card_width, |ui| {
            ui.label(egui::RichText::new("使用方法").size(13.0).strong());
            ui.add_space(10.0);
            for (number, text) in [
                ("1", "按右 Option 开始录音。"),
                ("2", "再次按右 Option 停止录音。"),
                ("3", "录音过程中按翻译快捷键切换到翻译模式。"),
                ("4", "松开按键后，译文会自动插入当前应用。"),
                ("5", "翻译模式会在胶囊顶部显示状态。"),
            ] {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{number}."))
                            .size(12.5)
                            .color(theme::INK_3),
                    );
                    ui.label(egui::RichText::new(text).size(12.5).color(theme::INK_2));
                });
                ui.add_space(4.0);
            }
        });
    }

    fn overview(&self, ui: &mut egui::Ui, width: f32) {
        let gap = 12.0;
        let provider_width = (width - gap) / 2.0;
        let provider_height = 98.0;
        let provider_row = ui
            .allocate_exact_size(egui::vec2(width, provider_height), egui::Sense::hover())
            .0;
        self.provider(
            ui,
            egui::Rect::from_min_size(
                provider_row.min,
                egui::vec2(provider_width, provider_height),
            ),
            "ASR 语音",
            "智谱 GLM-ASR",
            "zhipu",
            IconName::Mic,
        );
        self.provider(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(
                    provider_row.left() + provider_width + gap,
                    provider_row.top(),
                ),
                egui::vec2(provider_width, provider_height),
            ),
            "LLM 模型",
            "DeepSeek",
            "deepseek",
            IconName::Sparkle,
        );
        // Keep a clear breathing space between the provider cards and the
        // metric row. The provider cards are 98px tall; the extra separation
        // prevents the metric cards from visually crowding their bottoms when
        // the content viewport is resized.
        ui.add_space(32.0);

        let metric_width = (width - gap * 3.0) / 4.0;
        let metric_row = ui
            .allocate_exact_size(egui::vec2(width, 108.0), egui::Sense::hover())
            .0;
        for (index, (icon, label, value, detail, accent)) in [
            (IconName::Hash, "今日字数", "0", "0 段", false),
            (IconName::Mic, "今日总时长", "—", "", false),
            (IconName::Clock, "平均段落", "—", "暂无数据", false),
            (
                IconName::Bolt,
                "累计记录",
                "116",
                "本机存档 (上限 200)",
                true,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            self.metric(
                ui,
                egui::Rect::from_min_size(
                    egui::pos2(
                        metric_row.left() + index as f32 * (metric_width + gap),
                        metric_row.top(),
                    ),
                    egui::vec2(metric_width, 108.0),
                ),
                icon,
                label,
                value,
                detail,
                accent,
            );
        }
        ui.add_space(18.0);
        self.activity(ui, width);
        ui.add_space(18.0);

        // Keep the same 1fr 1.4fr split as the original web layout.
        let row_width = width - gap;
        let left_width = row_width / 2.4;
        let right_width = row_width - left_width;
        let bottom_row = ui
            .allocate_exact_size(egui::vec2(width, 304.0), egui::Sense::hover())
            .0;
        self.period_card(
            ui,
            egui::Rect::from_min_size(bottom_row.min, egui::vec2(left_width, 304.0)),
        );
        self.recent_card(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(bottom_row.left() + left_width + gap, bottom_row.top()),
                egui::vec2(right_width, 304.0),
            ),
        );
    }

    fn card_at(ui: &mut egui::Ui, rect: egui::Rect, contents: impl FnOnce(&mut egui::Ui)) {
        // The parent row owns this rectangle. Card contents are deliberately
        // laid out in a child UI and can never change the sibling positions.
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(14), theme::SURFACE);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(14),
            egui::Stroke::new(1.0, theme::LINE),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(17.0);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(rect));
                contents(ui);
            },
        );
    }

    fn translation_card(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui)) {
        // Let egui measure the child content. Fixed-height cards are unsafe
        // here because the language pills wrap differently as the window is
        // resized, which can make the following card overlap the previous one.
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::same(17))
            .show(ui, |ui| {
                ui.set_width((width - 34.0).max(1.0));
                contents(ui);
            });
    }

    fn card(ui: &mut egui::Ui, size: egui::Vec2, contents: impl FnOnce(&mut egui::Ui)) {
        let rect = ui.allocate_exact_size(size, egui::Sense::hover()).0;
        Self::card_at(ui, rect, contents);
    }

    fn provider(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        kind: &str,
        name: &str,
        id: &str,
        icon: IconName,
    ) {
        Self::card_at(ui, rect, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(10), theme::BLUE_SOFT);
                Self::draw_icon(ui, rect.center(), icon, theme::BLUE);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(kind).size(10.5).color(theme::INK_4));
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(name).size(14.0).strong());
                        ui.label(egui::RichText::new("● 已配置").size(10.5).color(theme::OK));
                    });
                    ui.label(egui::RichText::new(id).size(11.0).color(theme::INK_3));
                });
            });
        });
    }

    fn metric(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        icon: IconName,
        label: &str,
        value: &str,
        detail: &str,
        accent: bool,
    ) {
        Self::card_at(ui, rect, |ui| {
            ui.horizontal(|ui| {
                Self::draw_icon(
                    ui,
                    ui.cursor().min + egui::vec2(7.0, 8.0),
                    icon,
                    theme::INK_3,
                );
                ui.add_space(16.0);
                ui.label(egui::RichText::new(label).size(11.5).color(theme::INK_3));
            });
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(value)
                    .size(26.0)
                    .strong()
                    .color(if accent { theme::BLUE } else { theme::INK }),
            );
            if !detail.is_empty() {
                ui.label(egui::RichText::new(detail).size(10.5).color(theme::INK_4));
            }
        });
    }

    fn activity(&self, ui: &mut egui::Ui, width: f32) {
        Self::card(ui, egui::vec2(width, 184.0), |ui| {
            ui.label(
                egui::RichText::new("年度活动")
                    .size(12.0)
                    .strong()
                    .color(theme::INK_2),
            );
            ui.add_space(10.0);
            let grid_rect = ui
                .allocate_exact_size(
                    egui::vec2(ui.available_width(), 126.0),
                    egui::Sense::hover(),
                )
                .0;
            let painter = ui.painter().with_clip_rect(grid_rect);
            let label_width = 30.0;
            let columns = 53;
            let cell_gap = 3.0;
            let column_step = ((grid_rect.width() - label_width) / columns as f32).max(6.0);
            let cell_width = (column_step - cell_gap).max(5.0);
            let cell_height = ((grid_rect.height() - 22.0 - 6.0 * cell_gap) / 7.0).max(5.0);
            let months = [
                "9月", "10月", "11月", "12月", "1月", "2月", "3月", "4月", "5月", "6月", "7月",
                "8月",
            ];
            let month_columns = [0, 4, 9, 13, 18, 22, 27, 31, 36, 40, 45, 49];
            for (index, month) in months.iter().enumerate() {
                let x = grid_rect.left() + label_width + month_columns[index] as f32 * column_step;
                painter.text(
                    egui::pos2(x, grid_rect.top()),
                    egui::Align2::LEFT_TOP,
                    *month,
                    egui::FontId::proportional(9.0),
                    theme::INK_4,
                );
            }
            for row in 0..7 {
                let y = grid_rect.top() + 22.0 + row as f32 * (cell_height + cell_gap);
                painter.text(
                    egui::pos2(grid_rect.left(), y + cell_height / 2.0),
                    egui::Align2::LEFT_CENTER,
                    match row {
                        0 => "周日",
                        1 => "周一",
                        2 => "周二",
                        3 => "周三",
                        4 => "周四",
                        5 => "周五",
                        _ => "周六",
                    },
                    egui::FontId::proportional(10.0),
                    theme::INK_4,
                );
                for col in 0..columns {
                    let intensity = match (row, col) {
                        (0, 35) | (1, 35) | (4, 40) => 3,
                        (1, 34) | (2, 34) | (2, 38) => 2,
                        (0, 34) | (2, 35) | (3, 34) | (3, 35) => 1,
                        _ => 0,
                    };
                    let color = match intensity {
                        3 => theme::BLUE,
                        2 => theme::BLUE_SOFT,
                        1 => egui::Color32::from_rgb(219, 231, 252),
                        _ => theme::SURFACE_2,
                    };
                    let x = grid_rect.left() + label_width + col as f32 * column_step;
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(x, y),
                            egui::vec2(cell_width, cell_height),
                        ),
                        egui::CornerRadius::same(2),
                        color,
                    );
                }
            }
        });
    }

    fn period_card(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        Self::card_at(ui, rect, |ui| {
            let row_width = ui.available_width();
            let header_rect = ui
                .allocate_exact_size(egui::vec2(row_width, 28.0), egui::Sense::hover())
                .0;
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(header_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    egui::Frame::new()
                        .fill(theme::SURFACE_2)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(2))
                        .show(ui, |ui| {
                            self.segment_button(ui, "近 7 天", true, 52.0);
                            self.segment_button(ui, "近 30 天", false, 52.0);
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::Frame::new()
                            .fill(theme::SURFACE_2)
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(2))
                            .show(ui, |ui| {
                                self.segment_button(ui, "条数", false, 36.0);
                                self.segment_button(ui, "字数", true, 36.0);
                                self.segment_button(ui, "时长", false, 36.0);
                            });
                    });
                },
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("78").size(28.0).strong());
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("日均 11")
                        .size(11.0)
                        .color(theme::INK_4),
                );
            });
            ui.add_space(16.0);
            let values = [0, 0, 28, 11, 39, 0, 0];
            // Draw the chart in one explicit viewport and anchor every bar to
            // its bottom edge. A nested horizontal/vertical layout centers
            // children according to the tallest child, which made the bars
            // appear to start in the middle of the card after resizing.
            let chart_rect = ui
                .allocate_exact_size(
                    egui::vec2(ui.available_width(), ui.available_height().max(1.0)),
                    egui::Sense::hover(),
                )
                .0;
            let painter = ui.painter().with_clip_rect(chart_rect);
            let bar_gap = 10.0;
            let bar_width = ((chart_rect.width() - bar_gap * 6.0) / 7.0).min(34.0);
            let total_width = bar_width * 7.0 + bar_gap * 6.0;
            let start_x = chart_rect.center().x - total_width / 2.0;
            let baseline = chart_rect.bottom();
            for (index, value) in values.into_iter().enumerate() {
                let height = if value == 0 {
                    3.0
                } else {
                    (value as f32 * 1.55).min(chart_rect.height())
                };
                let left = start_x + index as f32 * (bar_width + bar_gap);
                let rect = egui::Rect::from_min_max(
                    egui::pos2(left, baseline - height),
                    egui::pos2(left + bar_width, baseline),
                );
                painter.rect_filled(
                    rect,
                    egui::CornerRadius::same(3),
                    if value > 0 {
                        theme::INK_4
                    } else {
                        theme::SURFACE_2
                    },
                );
            }
        });
    }

    fn segment_button(&self, ui: &mut egui::Ui, label: &str, active: bool, width: f32) {
        let _ = ui.add(
            egui::Button::new(egui::RichText::new(label).color(if active {
                theme::INK
            } else {
                theme::INK_3
            }))
            .fill(if active {
                theme::BLUE
            } else {
                egui::Color32::TRANSPARENT
            })
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(6))
            .min_size(egui::vec2(width, 24.0)),
        );
    }

    fn recent_card(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        Self::card_at(ui, rect, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("最近识别").size(12.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let _ = ui.add(
                        egui::Button::new(
                            egui::RichText::new("全部记录 →")
                                .size(11.5)
                                .color(theme::INK_2),
                        )
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.5, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(82.0, 26.0)),
                    );
                });
            });
            ui.separator();
            let _ = egui::ScrollArea::vertical()
                .id_salt("openless-recent-scroll")
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    let rows = [
                        (
                            "8/24",
                            "你所说的那些没有提交的改动是什么？再详细统...",
                            "7.4 秒",
                            "轻度润色",
                        ),
                        ("8/24", "现在好了，你可以提 PR 了。", "2.9 秒", "轻度润色"),
                        ("8/24", "可以了，你现在可以提 PR 了。", "3.8 秒", "轻度润色"),
                        ("8/24", "92%", "7.5 秒", "清晰结构"),
                        (
                            "8/24",
                            "Markdown 是一种轻量级标记语言。",
                            "3.3 秒",
                            "清晰结构",
                        ),
                    ];
                    for (date, text, time, tag) in rows {
                        let row_height = 66.0;
                        let row_rect = ui
                            .allocate_exact_size(
                                egui::vec2(ui.available_width(), row_height),
                                egui::Sense::hover(),
                            )
                            .0;
                        let right_width = 64.0;
                        let left_rect =
                            egui::Rect::from_min_size(row_rect.min, egui::vec2(60.0, row_height));
                        let right_rect = egui::Rect::from_min_size(
                            egui::pos2(row_rect.right() - right_width, row_rect.top()),
                            egui::vec2(right_width, row_height),
                        );
                        let text_rect = egui::Rect::from_min_max(
                            egui::pos2(left_rect.right() + 12.0, row_rect.top()),
                            egui::pos2(right_rect.left() - 12.0, row_rect.bottom()),
                        );
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(left_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |ui| {
                                ui.label(egui::RichText::new(date).size(10.5).color(theme::INK_3));
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(tag).size(10.0).color(theme::INK_3));
                            },
                        );
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(text_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |ui| {
                                ui.add_sized(
                                    [text_rect.width(), 20.0],
                                    egui::Label::new(
                                        egui::RichText::new(text).size(11.5).color(theme::INK_2),
                                    ),
                                );
                            },
                        );
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(right_rect)
                                .layout(egui::Layout::top_down(egui::Align::Max)),
                            |ui| {
                                ui.label(egui::RichText::new(time).size(10.0).color(theme::INK_4));
                                ui.add_space(6.0);
                                self.copy_button(ui);
                            },
                        );
                        ui.separator();
                    }
                });
        });
    }

    fn icon_text_button(
        ui: &mut egui::Ui,
        label: &str,
        icon: IconName,
        width: f32,
    ) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::click());
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            if response.hovered() {
                theme::SURFACE_2
            } else {
                theme::SURFACE
            },
        );
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(8),
            egui::Stroke::new(0.8, theme::LINE),
            egui::StrokeKind::Inside,
        );
        let text_width = label.chars().count() as f32 * 10.0;
        let content_width = 12.0 + 6.0 + text_width;
        let content_left = rect.left() + ((width - content_width) / 2.0).max(8.0);
        Self::draw_icon(
            ui,
            egui::pos2(content_left + 6.0, rect.center().y),
            icon,
            theme::INK_2,
        );
        ui.painter().text(
            egui::pos2(content_left + 18.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(11.5),
            theme::INK_2,
        );
        response
    }

    fn text_chevron_button(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            if response.hovered() {
                theme::SURFACE_2
            } else {
                theme::SURFACE
            },
        );
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(8),
            egui::Stroke::new(0.8, theme::LINE),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            theme::INK_2,
        );
        Self::draw_icon(
            ui,
            egui::pos2(rect.right() - 14.0, rect.center().y),
            IconName::ChevronDown,
            theme::INK_3,
        );
        response
    }

    fn copy_button(&self, ui: &mut egui::Ui) {
        let rect = ui
            .allocate_exact_size(egui::vec2(60.0, 26.0), egui::Sense::click())
            .0;
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(8),
            egui::Stroke::new(0.5, theme::LINE),
            egui::StrokeKind::Inside,
        );
        Self::draw_icon(
            ui,
            egui::pos2(rect.left() + 14.0, rect.center().y),
            IconName::Copy,
            theme::INK_3,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "复制",
            egui::FontId::proportional(11.5),
            theme::INK_2,
        );
    }

    fn title(&self) -> &'static str {
        match self.active {
            Tab::Overview => "今日概览",
            Tab::History => "历史",
            Tab::Vocab => "词汇表",
            Tab::Style => "润色模式",
            Tab::Marketplace => "风格市场",
            Tab::SelectionAsk => "划词追问",
            Tab::Translation => "翻译",
            Tab::Settings => "设置",
        }
    }
}
