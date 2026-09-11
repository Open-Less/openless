//! The seven-section 2.0 settings shell. Each pane is supplied by the host's
//! actual Core-backed editor, including provider credentials and model actions.
use super::theme;
use eframe::egui;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    General,
    Shortcuts,
    Services,
    Appearance,
    Privacy,
    Advanced,
    About,
}
impl Section {
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Shortcuts,
        Self::Services,
        Self::Appearance,
        Self::Privacy,
        Self::Advanced,
        Self::About,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::General => "录音与输入",
            Self::Shortcuts => "快捷键与选区",
            Self::Services => "AI 服务与模型",
            Self::Appearance => "外观与语言",
            Self::Privacy => "权限与数据",
            Self::Advanced => "实验与扩展",
            Self::About => "关于与更新",
        }
    }
    fn keywords(self) -> &'static str {
        match self {
            Self::General => "麦克风 静音 提示音 手机 远程 输入 microphone remote",
            Self::Shortcuts => "按住 听写 翻译 划词 热键 shortcut hotkey",
            Self::Services => "渠道 语音 识别 语言 模型 下载 网络 代理 ASR LLM Omni provider model",
            Self::Appearance => "主题 深色 浅色 字体 语言 外观 theme language font",
            Self::Privacy => "权限 历史 录音 上下文 云同步 存储 privacy sync history",
            Self::Advanced => "Less Computer Agent 多模态 调试 日志 multimodal debug",
            Self::About => "版本 更新 Beta 帮助 update version",
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct SettingsState {
    pub section: Section,
    pub query: String,
    pub service: usize,
    pub advanced: usize,
    pub confirmation: Option<String>,
}

pub fn modal(
    ctx: &egui::Context,
    state: &mut SettingsState,
    mut content: impl FnMut(&mut egui::Ui, &mut SettingsState),
) -> bool {
    let mut close = false;
    let size = egui::vec2(
        (ctx.content_rect().width() - 40.0).clamp(280.0, 960.0),
        (ctx.content_rect().height() - 40.0).clamp(240.0, 680.0),
    );
    let response = egui::Modal::new(egui::Id::new("openless-settings-2.0"))
        .frame(
            egui::Frame::new()
                .fill(theme::surface())
                .corner_radius(14)
                .stroke(egui::Stroke::new(1.0, theme::line()))
                .inner_margin(0),
        )
        .show(ctx, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.horizontal_top(|ui| {
                let rail_width = 214.0_f32.min(size.x * 0.36);
                egui::Frame::new()
                    .fill(theme::settings_rail_bg())
                    .inner_margin(16)
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(rail_width - 32.0, size.y - 32.0));
                        ui.set_max_width(rail_width - 32.0);
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        ui.heading(theme::text("设置"));
                        ui.add_space(12.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut state.query)
                                .hint_text(theme::text("搜索设置…"))
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(16.0);
                        for section in Section::ALL {
                            let haystack = format!("{} {}", section.title(), section.keywords())
                                .to_lowercase();
                            if !state
                                .query
                                .split_whitespace()
                                .all(|word| haystack.contains(&word.to_lowercase()))
                            {
                                continue;
                            }
                            let selected = state.section == section;
                            let button = egui::Button::new(
                                egui::RichText::new(theme::text(section.title()))
                                    .size(13.0)
                                    .color(if selected {
                                        theme::ink()
                                    } else {
                                        theme::ink_3()
                                    }),
                            )
                            .fill(if selected {
                                theme::surface()
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .corner_radius(8);
                            if ui.add_sized([rail_width - 32.0, 36.0], button).clicked() {
                                state.section = section;
                                state.query.clear();
                            }
                        }
                    });
                egui::Frame::new()
                    .fill(theme::settings_content_bg())
                    .inner_margin(24)
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(
                            (size.x - rail_width - 48.0).max(1.0),
                            size.y - 48.0,
                        ));
                        ui.set_max_width((size.x - rail_width - 48.0).max(1.0));
                        ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                        ui.horizontal(|ui| {
                            ui.heading(theme::text(state.section.title()));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    close |= ui
                                        .button("×")
                                        .on_hover_text(theme::text("关闭设置"))
                                        .clicked();
                                },
                            );
                        });
                        ui.add_space(12.0);
                        egui::ScrollArea::vertical()
                            .id_salt((
                                "settings-section",
                                state.section as usize,
                                state.service,
                                state.advanced,
                            ))
                            .auto_shrink([false, false])
                            .max_height(size.y - 112.0)
                            .show(ui, |ui| content(ui, state));
                    });
            });
        });
    close || response.should_close()
}

pub fn card(ui: &mut egui::Ui, title: &str, draw: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::line()))
        .corner_radius(14)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(egui::RichText::new(theme::text(title)).size(14.0).strong());
                ui.add_space(8.0);
            }
            draw(ui);
        });
    ui.add_space(12.0);
}
