use eframe::egui;

use crate::theme;

const RAIL_WIDTH: f32 = 198.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    Services,
    Privacy,
    Advanced,
    About,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Services => "服务",
            Self::Privacy => "隐私",
            Self::Advanced => "高级",
            Self::About => "关于",
        }
    }

    fn icon(self) -> SettingsIcon {
        match self {
            Self::General => SettingsIcon::Settings,
            Self::Services => SettingsIcon::Cloud,
            Self::Privacy => SettingsIcon::Shield,
            Self::Advanced => SettingsIcon::Bolt,
            Self::About => SettingsIcon::Info,
        }
    }
}

#[derive(Clone, Copy)]
enum SettingsIcon {
    Settings,
    Cloud,
    Shield,
    Bolt,
    Info,
    Help,
    Document,
    External,
}

/// Runtime-only state for the egui settings surface.
///
/// The first egui migration intentionally keeps persistence and IPC out of this
/// screen. Controls are real egui interactions and provide useful visual
/// placeholders while the native settings bridge is migrated section by section.
pub(crate) struct SettingsState {
    section: Section,
    notice: Option<String>,
    recording_enabled: bool,
    realtime_mode: bool,
    streaming_insert: bool,
    restore_clipboard: bool,
    start_minimized: bool,
    auto_update: bool,
    remote_input: bool,
    selection_assistant: bool,
    selection_voice: bool,
    stacked_layout: bool,
    conservative_layout: bool,
    activity_heatmap: bool,
    system_proxy: bool,
    local_model: bool,
    marketplace: bool,
    remember_history: bool,
    record_audio: bool,
    less_computer: bool,
    multimodal: bool,
    beta_channel: bool,
    claude_expanded: bool,
    provider: usize,
    language: usize,
    theme: usize,
    retention: usize,
    api_key: String,
    endpoint: String,
    model: String,
    remote_port: String,
    claude_prompt: String,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            section: Section::General,
            notice: None,
            recording_enabled: true,
            realtime_mode: true,
            streaming_insert: true,
            restore_clipboard: true,
            start_minimized: false,
            auto_update: true,
            remote_input: false,
            selection_assistant: true,
            selection_voice: false,
            stacked_layout: false,
            conservative_layout: false,
            activity_heatmap: true,
            system_proxy: false,
            local_model: false,
            marketplace: true,
            remember_history: true,
            record_audio: false,
            less_computer: false,
            multimodal: false,
            beta_channel: false,
            claude_expanded: false,
            provider: 0,
            language: 0,
            theme: 0,
            retention: 1,
            api_key: String::new(),
            endpoint: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            remote_port: "32123".into(),
            claude_prompt: String::new(),
        }
    }
}

impl SettingsState {
    /// Paint the in-window modal. Returns true when the caller should close it.
    pub(crate) fn ui(&mut self, ctx: &egui::Context, body: egui::Rect) -> bool {
        let width = (body.width() - 56.0).min(900.0).max(680.0);
        let height = (body.height() - 48.0).min(650.0).max(440.0);
        let size = egui::vec2(
            width.min(body.width() - 20.0),
            height.min(body.height() - 20.0),
        );
        let pos = body.center() - size / 2.0;

        // Keep the dim layer aligned with the same body rectangle as the main
        // window. Draw it directly so a cached Area size cannot leave square
        // corners or stale strips after the window is resized.
        let backdrop_layer = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("openless-settings-backdrop"),
        );
        ctx.layer_painter(backdrop_layer).rect_filled(
            body,
            egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 14,
                se: 14,
            },
            egui::Color32::from_black_alpha(56),
        );
        // Separate transparent input layer: it blocks clicks from reaching
        // the page underneath without participating in shadow rendering.
        egui::Area::new(egui::Id::new("openless-settings-backdrop-input"))
            .order(egui::Order::Foreground)
            .fixed_pos(body.min)
            .default_size(body.size())
            .constrain(false)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(body.size());
                ui.set_max_size(body.size());
                let _ = ui.allocate_exact_size(body.size(), egui::Sense::click());
            });

        // Do not use the raw pointer click here: the settings button that opens
        // this surface is painted earlier in the same egui frame, so that very
        // click would otherwise be mistaken for a click outside the modal.
        // The explicit close button is deterministic and matches the other
        // in-window overlays.
        let mut close = false;
        egui::Area::new(egui::Id::new("openless-settings-modal"))
            // Keep the modal above the interactive backdrop. Both areas are
            // in-window surfaces, but clicking the backdrop may reorder its
            // layer; Tooltip guarantees it can never cover this card.
            .order(egui::Order::Tooltip)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .shadow(egui::Shadow {
                        offset: [0, 12],
                        blur: 28,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(42),
                    })
                    .show(ui, |ui| {
                        ui.set_min_size(size);
                        ui.set_max_size(size);
                        ui.horizontal(|ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(RAIL_WIDTH, size.y),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| self.rail(ui),
                            );
                            ui.separator();
                            ui.allocate_ui_with_layout(
                                egui::vec2((size.x - RAIL_WIDTH - 1.0).max(0.0), size.y),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    if self.panel(ui) {
                                        close = true;
                                    }
                                },
                            );
                        });
                    });
            });
        close
    }

    fn rail(&mut self, ui: &mut egui::Ui) {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(12, 18))
            .show(ui, |ui| {
                for section in [
                    Section::General,
                    Section::Services,
                    Section::Privacy,
                    Section::Advanced,
                    Section::About,
                ] {
                    let active = self.section == section;
                    let response = Self::rail_item(ui, section.label(), section.icon(), active);
                    if response.clicked() {
                        self.section = section;
                        self.notice = None;
                    }
                }
                ui.add_space(14.0);
                ui.separator();
                ui.add_space(7.0);
                for (label, icon, message) in [
                    (
                        "帮助中心",
                        SettingsIcon::Help,
                        "帮助中心将在 egui 外链桥接完成后打开",
                    ),
                    (
                        "发布日志",
                        SettingsIcon::Document,
                        "发布日志将在 egui 外链桥接完成后打开",
                    ),
                ] {
                    let response = Self::rail_item(ui, label, icon, false);
                    let row = response.rect;
                    Self::draw_rail_icon(
                        ui,
                        egui::pos2(row.right() - 14.0, row.center().y),
                        SettingsIcon::External,
                        theme::INK_4,
                    );
                    if response.clicked() {
                        self.notice = Some(message.into());
                    }
                }
            });
    }

    /// The React settings rail uses the shared SVG Icon component. egui does
    /// not have an SVG widget, so these are the same 24x24 source paths
    /// redrawn with the painter rather than Unicode glyphs (which vary by OS).
    fn rail_item(
        ui: &mut egui::Ui,
        label: &str,
        icon: SettingsIcon,
        active: bool,
    ) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
        if active {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
        }
        let color = if active { theme::INK } else { theme::INK_3 };
        let icon_center = egui::pos2(rect.left() + 17.0, rect.center().y);
        Self::draw_rail_icon(ui, icon_center, icon, color);
        ui.painter().text(
            egui::pos2(rect.left() + 34.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            color,
        );
        response
    }

    fn draw_rail_icon(ui: &egui::Ui, center: egui::Pos2, icon: SettingsIcon, color: egui::Color32) {
        let painter = ui.painter();
        let stroke = egui::Stroke::new(1.35, color);
        let point = |x: f32, y: f32| center + egui::vec2(x, y);
        match icon {
            SettingsIcon::Settings => {
                // Icon.tsx: settings
                painter.circle_stroke(center, 4.2, stroke);
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
                    painter
                        .line_segment([center + direction * 5.0, center + direction * 7.0], stroke);
                }
            }
            SettingsIcon::Cloud => {
                // Icon.tsx: cloud — a compact outline preserving its three
                // rounded lobes and the straight lower edge.
                painter.circle_stroke(point(-2.6, 0.0), 4.2, stroke);
                painter.circle_stroke(point(2.7, -2.1), 4.0, stroke);
                painter.circle_stroke(point(5.5, 1.0), 3.4, stroke);
                painter.line_segment([point(-6.0, 4.0), point(5.7, 4.0)], stroke);
                painter.line_segment([point(-6.0, 4.0), point(-6.0, 2.0)], stroke);
                painter.line_segment([point(5.7, 4.0), point(6.8, 2.0)], stroke);
            }
            SettingsIcon::Shield => {
                // Icon.tsx: shield
                painter.add(egui::Shape::line(
                    [
                        point(0.0, 8.0),
                        point(6.0, 5.0),
                        point(6.0, -4.0),
                        point(0.0, -7.0),
                        point(-6.0, -4.0),
                        point(-6.0, 5.0),
                        point(0.0, 8.0),
                    ]
                    .to_vec(),
                    stroke,
                ));
            }
            SettingsIcon::Bolt => {
                // Icon.tsx: bolt
                painter.add(egui::Shape::line(
                    [
                        point(1.0, -8.0),
                        point(-5.0, 1.0),
                        point(1.0, 1.0),
                        point(-1.0, 8.0),
                        point(6.0, -1.0),
                        point(1.0, -1.0),
                        point(1.0, -8.0),
                    ]
                    .to_vec(),
                    stroke,
                ));
            }
            SettingsIcon::Info => {
                painter.circle_stroke(center, 8.0, stroke);
                painter.line_segment([point(0.0, -1.0), point(0.0, 5.0)], stroke);
                painter.circle_filled(point(0.0, -4.0), 0.8, color);
            }
            SettingsIcon::Help => {
                painter.circle_stroke(center, 8.0, stroke);
                painter.add(egui::Shape::line(
                    [
                        point(-2.2, -2.3),
                        point(-1.2, -4.0),
                        point(1.2, -4.0),
                        point(2.2, -2.2),
                        point(0.4, 0.0),
                        point(0.4, 2.0),
                    ]
                    .to_vec(),
                    stroke,
                ));
                painter.circle_filled(point(0.4, 5.0), 0.75, color);
            }
            SettingsIcon::Document => {
                painter.rect_stroke(
                    egui::Rect::from_center_size(
                        center + egui::vec2(-1.0, 0.0),
                        egui::vec2(12.0, 16.0),
                    ),
                    egui::CornerRadius::same(1),
                    stroke,
                    egui::StrokeKind::Inside,
                );
                painter.add(egui::Shape::line(
                    [point(1.0, -8.0), point(1.0, -3.0), point(6.0, -3.0)].to_vec(),
                    stroke,
                ));
            }
            SettingsIcon::External => {
                painter.add(egui::Shape::line(
                    vec![
                        point(-5.0, 4.0),
                        point(-5.0, 7.0),
                        point(4.0, 7.0),
                        point(4.0, -2.0),
                        point(1.0, -2.0),
                    ],
                    stroke,
                ));
                painter.line_segment([point(-1.0, 3.0), point(7.0, -5.0)], stroke);
                painter.add(egui::Shape::line(
                    [point(3.0, -5.0), point(7.0, -5.0), point(7.0, -1.0)].to_vec(),
                    stroke,
                ));
            }
        }
    }

    fn panel(&mut self, ui: &mut egui::Ui) -> bool {
        let mut close = false;
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(24, 16))
            .show(ui, |ui| {
                {
                    let style = ui.style_mut();
                    style.visuals.menu_corner_radius = egui::CornerRadius::same(10);
                    for widget in [
                        &mut style.visuals.widgets.inactive,
                        &mut style.visuals.widgets.hovered,
                        &mut style.visuals.widgets.active,
                        &mut style.visuals.widgets.open,
                    ] {
                        widget.corner_radius = egui::CornerRadius::same(8);
                        widget.bg_stroke = egui::Stroke::new(1.0, theme::LINE);
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(self.section.label())
                            .size(22.0)
                            .strong()
                            .color(theme::INK),
                    );
                    ui.add_space(ui.available_width() - 38.0);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("×").size(22.0).color(theme::INK_3),
                            )
                            .fill(theme::SURFACE_2)
                            .stroke(egui::Stroke::new(0.7, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(30.0, 30.0)),
                        )
                        .clicked()
                    {
                        close = true;
                    }
                });
                if let Some(notice) = &self.notice {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(notice).size(11.0).color(theme::BLUE));
                }
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("openless-settings-content")
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.section {
                        Section::General => self.general(ui),
                        Section::Services => self.services(ui),
                        Section::Privacy => self.privacy(ui),
                        Section::Advanced => self.advanced(ui),
                        Section::About => self.about(ui),
                    });
            });
        close
    }

    fn general(&mut self, ui: &mut egui::Ui) {
        self.card(
            ui,
            "录音与输入",
            "全局录音的快捷键与触发方式。",
            |this, ui| {
                Self::text_row(ui, "录音快捷键", "右 Option");
                Self::combo_bool_row(
                    ui,
                    "录音方式",
                    &mut this.realtime_mode,
                    &["切换式", "按住说话"],
                );
                Self::combo_index_row(
                    ui,
                    "麦克风",
                    &mut this.provider,
                    &["系统默认", "内置麦克风", "外接麦克风"],
                );
                Self::toggle_row(ui, "自动插入光标位置", &mut this.recording_enabled);
                Self::toggle_row(ui, "流式插入", &mut this.streaming_insert);
                Self::toggle_row(ui, "录音时静音", &mut this.selection_voice);
            },
        );
        self.card(
            ui,
            "插入与剪贴板",
            "识别结果如何回到当前光标位置。",
            |this, ui| {
                Self::toggle_row(ui, "恢复剪贴板", &mut this.restore_clipboard);
                Self::text_row(ui, "模拟粘贴快捷键", "Ctrl V");
                Self::toggle_row(ui, "流式结果保存剪贴板", &mut this.streaming_insert);
            },
        );
        self.card(
            ui,
            "布局",
            "让浮层和内容更适合你的工作方式。",
            |this, ui| {
                Self::toggle_row(ui, "堆叠设置行", &mut this.stacked_layout);
                Self::toggle_row(ui, "紧凑布局", &mut this.conservative_layout);
            },
        );
        self.card(
            ui,
            "远程输入",
            "通过局域网接收来自其他设备的输入。",
            |this, ui| {
                Self::toggle_row(ui, "启用远程输入", &mut this.remote_input);
                Self::text_row(ui, "监听端口", &this.remote_port.clone());
                Self::combo_index_row(
                    ui,
                    "默认模式",
                    &mut this.provider,
                    &["润色", "原文", "翻译"],
                );
            },
        );
        self.card(
            ui,
            "选区助手",
            "对选中的文本进行润色或语音编辑。",
            |this, ui| {
                Self::toggle_row(ui, "启用选区助手", &mut this.selection_assistant);
                Self::toggle_row(ui, "启用选区语音编辑", &mut this.selection_voice);
                Self::combo_bool_row(
                    ui,
                    "结果处理方式",
                    &mut this.realtime_mode,
                    &["直接替换", "预览后确认"],
                );
                Self::toggle_row(ui, "自动判断编辑意图", &mut this.selection_voice);
            },
        );
        self.card(
            ui,
            "快捷键",
            "开始/停止、翻译、问答和风格切换。",
            |this, ui| {
                Self::text_row(ui, "翻译", "⌥ T");
                Self::text_row(ui, "划词追问", "⌥ Q");
                Self::text_row(ui, "切换风格", "⌥ S");
                Self::text_row(ui, "唤起 OpenLess", "⌥ Space");
                this.action_row(ui, "风格直达", "添加快捷键", this.notice.clone());
            },
        );
        self.card(
            ui,
            "主题",
            "外观与概览页显示选项。",
            |this, ui| {
                Self::combo_index_row(ui, "主题", &mut this.theme, &["跟随系统", "浅色", "深色"]);
                Self::toggle_row(ui, "显示活动热力图", &mut this.activity_heatmap);
            },
        );
        self.card(
            ui,
            "界面语言",
            "选择 OpenLess 使用的界面语言。",
            |this, ui| {
                Self::combo_index_row(
                    ui,
                    "语言",
                    &mut this.language,
                    &[
                        "跟随系统",
                        "简体中文",
                        "繁体中文",
                        "English",
                        "日本語",
                        "한국어",
                    ],
                );
            },
        );
        self.card(
            ui,
            "启动",
            "控制 OpenLess 启动时的行为。",
            |this, ui| {
                Self::toggle_row(ui, "启动时最小化", &mut this.start_minimized);
            },
        );
    }

    fn services(&mut self, ui: &mut egui::Ui) {
        self.card(
            ui,
            "AI 提供商",
            "配置语音识别、润色与翻译所使用的服务。",
            |this, ui| {
                Self::combo_index_row(
                    ui,
                    "当前提供商",
                    &mut this.provider,
                    &["OpenAI 兼容", "百炼", "SiliconFlow", "本地服务"],
                );
                Self::text_edit_row(ui, "API Key", &mut this.api_key, "sk-…");
                Self::text_edit_row(ui, "服务地址", &mut this.endpoint, "https://…");
                Self::text_edit_row(ui, "模型", &mut this.model, "模型名称");
                this.action_row(
                    ui,
                    "连接测试",
                    "测试配置",
                    Some("配置仅在当前运行期间保存".into()),
                );
            },
        );
        self.card(
            ui,
            "网络",
            "网络请求的代理和连接选项。",
            |this, ui| {
                Self::toggle_row(ui, "使用系统代理", &mut this.system_proxy);
                Self::toggle_row(ui, "失败时自动重试", &mut this.auto_update);
            },
        );
        self.card(
            ui,
            "本地模型",
            "在桌面端使用本地语音识别模型。",
            |this, ui| {
                Self::toggle_row(ui, "启用本地模型", &mut this.local_model);
                Self::text_row(ui, "模型目录", "未选择目录");
                this.action_row(ui, "模型管理", "选择目录", None);
            },
        );
        self.card(
            ui,
            "扩展市场",
            "浏览和安装风格包及输入扩展。",
            |this, ui| {
                Self::toggle_row(ui, "启用扩展市场", &mut this.marketplace);
                this.action_row(ui, "已安装扩展", "管理扩展", None);
            },
        );
    }

    fn privacy(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(theme::BLUE_SOFT)
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("本地优先").strong().color(theme::BLUE));
                    ui.label(
                        egui::RichText::new(
                            "设置和历史记录默认保存在本机，只有请求服务时才会发送内容。",
                        )
                        .size(11.5)
                        .color(theme::INK_3),
                    );
                });
            });
        self.card(
            ui,
            "权限",
            "查看 OpenLess 使用麦克风、辅助功能和网络的权限。",
            |this, ui| {
                Self::status_row(ui, "麦克风", "已授权", theme::OK);
                Self::status_row(ui, "辅助功能", "待检查", theme::INK_4);
                Self::status_row(ui, "键盘输入", "待检查", theme::INK_4);
                this.action_row(ui, "权限管理", "打开系统设置", None);
            },
        );
        self.card(
            ui,
            "数据存储",
            "控制历史记录、上下文和调试音频的保留方式。",
            |this, ui| {
                Self::toggle_row(ui, "保存历史记录", &mut this.remember_history);
                Self::combo_index_row(
                    ui,
                    "历史记录保留",
                    &mut this.retention,
                    &["不限制", "最近 30 天", "最近 7 天", "不保存"],
                );
                Self::text_row(ui, "历史记录上限", "500 条");
                this.action_row(
                    ui,
                    "数据管理",
                    "清空历史记录",
                    Some("清空操作将在数据桥接完成后执行".into()),
                );
            },
        );
    }

    fn advanced(&mut self, ui: &mut egui::Ui) {
        self.card(
            ui,
            "Less Computer",
            "实验性的编码代理入口，仅 macOS 提供完整能力。",
            |this, ui| {
                Self::toggle_row(ui, "启用 Less Computer", &mut this.less_computer);
                Self::combo_index_row(
                    ui,
                    "权限模式",
                    &mut this.retention,
                    &["接受编辑", "计划模式", "默认", "绕过权限"],
                );
                Self::text_row(ui, "工作目录", "当前项目");
            },
        );
        self.card(
            ui,
            "Claude 控制台",
            "检测 Claude CLI、MCP 和 computer use 状态。",
            |this, ui| {
                let label = if this.claude_expanded {
                    "收起控制台"
                } else {
                    "展开控制台"
                };
                this.action_row(
                    ui,
                    "检测状态",
                    "重新检测",
                    Some("未连接到 Claude CLI（占位）".into()),
                );
                this.action_row(ui, "控制台", label, None);
                if ui.button(label).clicked() {
                    this.claude_expanded = !this.claude_expanded;
                }
                if this.claude_expanded {
                    Self::text_edit_row(
                        ui,
                        "测试提示词",
                        &mut this.claude_prompt,
                        "输入一条安全的测试指令",
                    );
                    this.action_row(
                        ui,
                        "运行测试",
                        "执行",
                        Some("执行能力将在编码代理桥接完成后启用".into()),
                    );
                }
            },
        );
        self.card(
            ui,
            "多模态管线",
            "实验性的图片和音频上下文处理。",
            |this, ui| {
                Self::toggle_row(ui, "启用多模态处理", &mut this.multimodal);
                Self::text_row(ui, "处理方式", "跟随当前服务");
            },
        );
        self.card(
            ui,
            "调试工具",
            "导出诊断信息，帮助定位输入和插入问题。",
            |this, ui| {
                Self::toggle_row(ui, "为调试保存录音", &mut this.record_audio);
                Self::toggle_row(ui, "记录光标探针", &mut this.conservative_layout);
                this.action_row(ui, "诊断信息", "导出错误日志", None);
            },
        );
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        self.card(
            ui,
            "OpenLess",
            "版本信息、更新渠道与项目链接。",
            |this, ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("OpenLess").size(17.0).strong());
                    ui.label(
                        egui::RichText::new("1.3.18-Beta.7")
                            .size(12.0)
                            .color(theme::INK_3),
                    );
                });
                this.action_row(
                    ui,
                    "正式版更新",
                    "检查更新",
                    Some("当前已是最新版本（占位）".into()),
                );
                Self::toggle_row(ui, "加入 Beta 渠道", &mut this.beta_channel);
                Self::toggle_row(ui, "自动检查更新", &mut this.auto_update);
            },
        );
        self.card(
            ui,
            "文档与反馈",
            "获取帮助、查看源码或提交问题。",
            |this, ui| {
                for (label, action) in [
                    ("GitHub", "打开 GitHub"),
                    ("文档", "打开帮助中心"),
                    ("发行说明", "打开发行说明"),
                    ("反馈", "打开问题反馈"),
                ] {
                    this.action_row(ui, label, action, None);
                }
                Self::text_row(ui, "QQ群", "1078960553");
                this.action_row(ui, "QQ群", "复制群号", Some("复制操作占位".into()));
            },
        );
    }

    fn card(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        description: &str,
        contents: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(14.0)
                        .strong()
                        .color(theme::INK),
                )
                .on_hover_text(description);
                ui.add_space(4.0);
                contents(self, ui);
            });
        ui.add_space(10.0);
    }

    fn toggle_row(ui: &mut egui::Ui, label: &str, value: &mut bool) {
        Self::row(ui, label, |ui| {
            let text = if *value { "开" } else { "关" };
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(text).size(11.5))
                        .fill(if *value {
                            theme::BLUE_SOFT
                        } else {
                            theme::SURFACE_2
                        })
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(48.0, 26.0)),
                )
                .clicked()
            {
                *value = !*value;
            }
        });
    }

    fn combo_index_row(ui: &mut egui::Ui, label: &str, value: &mut usize, options: &[&str]) {
        Self::row(ui, label, |ui| {
            egui::ComboBox::from_id_salt(("settings", label))
                .selected_text(options.get(*value).copied().unwrap_or("选择"))
                .show_ui(ui, |ui| {
                    for (index, option) in options.iter().enumerate() {
                        let response = ui.selectable_label(*value == index, *option);
                        if response.clicked() {
                            *value = index;
                            ui.close();
                        }
                    }
                });
        });
    }

    fn combo_bool_row(ui: &mut egui::Ui, label: &str, value: &mut bool, options: &[&str]) {
        Self::row(ui, label, |ui| {
            let selected = if *value {
                options.first()
            } else {
                options.get(1)
            };
            egui::ComboBox::from_id_salt(("settings", label))
                .selected_text(selected.copied().unwrap_or("选择"))
                .show_ui(ui, |ui| {
                    for (index, option) in options.iter().enumerate() {
                        let response = ui.selectable_label((index == 0) == *value, *option);
                        if response.clicked() {
                            *value = index == 0;
                            ui.close();
                        }
                    }
                });
        });
    }

    fn text_row(ui: &mut egui::Ui, label: &str, value: &str) {
        Self::row(ui, label, |ui| {
            ui.label(egui::RichText::new(value).size(12.0).color(theme::INK_2));
        });
    }

    fn text_edit_row(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
        Self::row(ui, label, |ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(hint)
                    .desired_width(ui.available_width().min(300.0)),
            );
        });
    }

    fn status_row(ui: &mut egui::Ui, label: &str, status: &str, color: egui::Color32) {
        Self::row(ui, label, |ui| {
            ui.label(egui::RichText::new(status).size(12.0).color(color));
        });
    }

    fn action_row(&mut self, ui: &mut egui::Ui, label: &str, action: &str, notice: Option<String>) {
        Self::row(ui, label, |ui| {
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(action).size(11.5))
                        .fill(theme::SURFACE_2)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(0.0, 26.0)),
                )
                .clicked()
            {
                self.notice = notice.or_else(|| Some(format!("{}：{}（占位交互）", label, action)));
            }
        });
    }

    fn row(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui)) {
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);
            ui.allocate_ui_with_layout(
                egui::vec2(176.0, 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(egui::RichText::new(label).size(12.0).color(theme::INK_2));
                },
            );
            control(ui);
        });
        ui.separator();
    }
}
