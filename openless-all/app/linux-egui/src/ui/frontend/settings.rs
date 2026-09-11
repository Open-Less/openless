use eframe::egui;

use super::theme;
use super::view_model::{
    FrontendAction, FrontendViewModel, SettingsActionField, SettingsComboField, SettingsField,
    SettingsSection, SettingsTextField,
};

const RAIL_WIDTH: f32 = 198.0;

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

impl SettingsSection {
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

/// Paint the in-window settings modal. Returns true when the caller should
/// close it. Actions are pushed into the provided vec.
pub fn settings_overlay(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    body: egui::Rect,
) {
    let width = (body.width() - 56.0).min(900.0).max(680.0);
    let height = (body.height() - 48.0).min(650.0).max(440.0);
    let size = egui::vec2(
        width.min(body.width() - 20.0),
        height.min(body.height() - 20.0),
    );
    let pos = body.center() - size / 2.0;

    // Backdrop
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
    // Input capture
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

    egui::Area::new(egui::Id::new("openless-settings-modal"))
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
                            |ui| rail(ui, vm, actions),
                        );
                        ui.separator();
                        ui.allocate_ui_with_layout(
                            egui::vec2((size.x - RAIL_WIDTH - 1.0).max(0.0), size.y),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                panel(ui, vm, actions);
                            },
                        );
                    });
                });
        });
}

fn rail(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 18))
        .show(ui, |ui| {
            for section in [
                SettingsSection::General,
                SettingsSection::Services,
                SettingsSection::Privacy,
                SettingsSection::Advanced,
                SettingsSection::About,
            ] {
                let active = vm.settings_section == section;
                let response = rail_item(ui, section.label(), section.icon(), active);
                if response.clicked() {
                    actions.push(FrontendAction::SettingsSection(section));
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
                let response = rail_item(ui, label, icon, false);
                let row = response.rect;
                draw_rail_icon(
                    ui,
                    egui::pos2(row.right() - 14.0, row.center().y),
                    SettingsIcon::External,
                    theme::INK_4,
                );
                if response.clicked() {
                    vm.settings_notice = Some(message.into());
                }
            }
        });
}

fn rail_item(ui: &mut egui::Ui, label: &str, icon: SettingsIcon, active: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
    }
    let color = if active { theme::INK } else { theme::INK_3 };
    let icon_center = egui::pos2(rect.left() + 17.0, rect.center().y);
    draw_rail_icon(ui, icon_center, icon, color);
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
                painter.line_segment([center + direction * 5.0, center + direction * 7.0], stroke);
            }
        }
        SettingsIcon::Cloud => {
            painter.circle_stroke(point(-2.6, 0.0), 4.2, stroke);
            painter.circle_stroke(point(2.7, -2.1), 4.0, stroke);
            painter.circle_stroke(point(5.5, 1.0), 3.4, stroke);
            painter.line_segment([point(-6.0, 4.0), point(5.7, 4.0)], stroke);
            painter.line_segment([point(-6.0, 4.0), point(-6.0, 2.0)], stroke);
            painter.line_segment([point(5.7, 4.0), point(6.8, 2.0)], stroke);
        }
        SettingsIcon::Shield => {
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

fn panel(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
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
                    egui::RichText::new(vm.settings_section.label())
                        .size(22.0)
                        .strong()
                        .color(theme::INK),
                );
                ui.add_space(ui.available_width() - 38.0);
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("×").size(22.0).color(theme::INK_3))
                            .fill(theme::SURFACE_2)
                            .stroke(egui::Stroke::new(0.7, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(30.0, 30.0)),
                    )
                    .clicked()
                {
                    actions.push(FrontendAction::CloseSettings);
                }
            });
            if let Some(notice) = &vm.settings_notice {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(notice).size(11.0).color(theme::BLUE));
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("openless-settings-content")
                .auto_shrink([false, false])
                .show(ui, |ui| match vm.settings_section {
                    SettingsSection::General => general(ui, vm, actions),
                    SettingsSection::Services => services(ui, vm, actions),
                    SettingsSection::Privacy => privacy(ui, vm, actions),
                    SettingsSection::Advanced => advanced(ui, vm, actions),
                    SettingsSection::About => about(ui, vm, actions),
                });
        });
}

fn general(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    card(
        ui,
        "录音与输入",
        "全局录音的快捷键与触发方式。",
        |ui| {
            text_row(ui, "录音快捷键", "右 Option");
            combo_bool_row(
                ui,
                "录音方式",
                vm.settings.realtime_mode,
                &["切换式", "按住说话"],
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::RealtimeMode));
                },
            );
            combo_index_row(
                ui,
                "麦克风",
                vm.settings.provider,
                &["系统默认", "内置麦克风", "外接麦克风"],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::Provider,
                        val,
                    ));
                },
            );
            toggle_row(
                ui,
                "自动插入光标位置",
                vm.settings.recording_enabled,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::RecordingEnabled,
                    ));
                },
            );
            toggle_row(ui, "流式插入", vm.settings.streaming_insert, || {
                actions.push(FrontendAction::SettingsToggle(
                    SettingsField::StreamingInsert,
                ));
            });
            toggle_row(ui, "录音时静音", vm.settings.selection_voice, || {
                actions.push(FrontendAction::SettingsToggle(
                    SettingsField::SelectionVoice,
                ));
            });
        },
    );
    card(
        ui,
        "插入与剪贴板",
        "识别结果如何回到当前光标位置。",
        |ui| {
            toggle_row(ui, "恢复剪贴板", vm.settings.restore_clipboard, || {
                actions.push(FrontendAction::SettingsToggle(
                    SettingsField::RestoreClipboard,
                ));
            });
            text_row(ui, "模拟粘贴快捷键", "Ctrl V");
            toggle_row(
                ui,
                "流式结果保存剪贴板",
                vm.settings.streaming_insert,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::StreamingInsert,
                    ));
                },
            );
        },
    );
    card(
        ui,
        "布局",
        "让浮层和内容更适合你的工作方式。",
        |ui| {
            toggle_row(ui, "堆叠设置行", vm.settings.stacked_layout, || {
                actions.push(FrontendAction::SettingsToggle(SettingsField::StackedLayout));
            });
            toggle_row(ui, "紧凑布局", vm.settings.conservative_layout, || {
                actions.push(FrontendAction::SettingsToggle(
                    SettingsField::ConservativeLayout,
                ));
            });
        },
    );
    card(
        ui,
        "远程输入",
        "通过局域网接收来自其他设备的输入。",
        |ui| {
            toggle_row(ui, "启用远程输入", vm.settings.remote_input, || {
                actions.push(FrontendAction::SettingsToggle(SettingsField::RemoteInput));
            });
            text_row(ui, "监听端口", &vm.settings.remote_port);
            combo_index_row(
                ui,
                "默认模式",
                vm.settings.provider,
                &["润色", "原文", "翻译"],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::Provider,
                        val,
                    ));
                },
            );
        },
    );
    card(
        ui,
        "快捷键",
        "开始/停止、翻译、问答和风格切换。",
        |ui| {
            text_row(ui, "翻译", "⌥ T");
            text_row(ui, "划词追问", "⌥ Q");
            text_row(ui, "切换风格", "⌥ S");
            text_row(ui, "唤起 OpenLess", "⌥ Space");
            action_row(
                ui,
                "风格直达",
                "添加快捷键",
                vm.settings_notice.clone(),
                actions,
            );
        },
    );
    card(ui, "主题", "外观与概览页显示选项。", |ui| {
        combo_index_row(
            ui,
            "主题",
            vm.settings.theme,
            &["跟随系统", "浅色", "深色"],
            |val| {
                actions.push(FrontendAction::SettingsCombo(
                    SettingsComboField::Theme,
                    val,
                ));
            },
        );
        toggle_row(
            ui,
            "显示活动热力图",
            vm.settings.activity_heatmap,
            || {
                actions.push(FrontendAction::SettingsToggle(
                    SettingsField::ActivityHeatmap,
                ));
            },
        );
    });
    card(
        ui,
        "界面语言",
        "选择 OpenLess 使用的界面语言。",
        |ui| {
            combo_index_row(
                ui,
                "语言",
                vm.settings.language,
                &[
                    "跟随系统",
                    "简体中文",
                    "繁体中文",
                    "English",
                    "日本語",
                    "한국어",
                ],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::Language,
                        val,
                    ));
                },
            );
        },
    );
    card(ui, "启动", "控制 OpenLess 启动时的行为。", |ui| {
        toggle_row(
            ui,
            "启动时最小化",
            vm.settings.start_minimized,
            || {
                actions.push(FrontendAction::SettingsToggle(
                    SettingsField::StartMinimized,
                ));
            },
        );
    });
}

fn services(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    card(
        ui,
        "AI 提供商",
        "配置语音识别、润色与翻译所使用的服务。",
        |ui| {
            combo_index_row(
                ui,
                "当前提供商",
                vm.settings.provider,
                &["OpenAI 兼容", "百炼", "SiliconFlow", "本地服务"],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::Provider,
                        val,
                    ));
                },
            );
            let api_key = vm.settings.api_key.clone();
            text_edit_row(ui, "API Key", &mut vm.settings.api_key, "sk-…", || {
                actions.push(FrontendAction::SettingsText(
                    SettingsTextField::ApiKey,
                    api_key,
                ));
            });
            let endpoint = vm.settings.endpoint.clone();
            text_edit_row(
                ui,
                "服务地址",
                &mut vm.settings.endpoint,
                "https://…",
                || {
                    actions.push(FrontendAction::SettingsText(
                        SettingsTextField::Endpoint,
                        endpoint,
                    ));
                },
            );
            let model = vm.settings.model.clone();
            text_edit_row(ui, "模型", &mut vm.settings.model, "模型名称", || {
                actions.push(FrontendAction::SettingsText(
                    SettingsTextField::Model,
                    model,
                ));
            });
            action_row(
                ui,
                "连接测试",
                "测试配置",
                Some("配置仅在当前运行期间保存".into()),
                actions,
            );
        },
    );
    card(
        ui,
        "网络",
        "网络请求的代理和连接选项。",
        |ui| {
            toggle_row(ui, "使用系统代理", vm.settings.system_proxy, || {
                actions.push(FrontendAction::SettingsToggle(SettingsField::SystemProxy));
            });
            toggle_row(ui, "失败时自动重试", vm.settings.auto_update, || {
                actions.push(FrontendAction::SettingsToggle(SettingsField::AutoUpdate));
            });
        },
    );
    card(
        ui,
        "扩展市场",
        "浏览和安装风格包及输入扩展。",
        |ui| {
            toggle_row(
                ui,
                "启用扩展市场",
                vm.settings.marketplace_enabled,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::MarketplaceEnabled,
                    ));
                },
            );
            action_row(ui, "已安装扩展", "管理扩展", None, actions);
        },
    );
}

fn privacy(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
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
    card(
        ui,
        "权限",
        "查看 OpenLess 使用麦克风、辅助功能和网络的权限。",
        |ui| {
            status_row(ui, "麦克风", "已授权", theme::OK);
            status_row(ui, "辅助功能", "待检查", theme::INK_4);
            status_row(ui, "键盘输入", "待检查", theme::INK_4);
            action_row(ui, "权限管理", "打开系统设置", None, actions);
        },
    );
    card(
        ui,
        "数据存储",
        "控制历史记录、上下文和调试音频的保留方式。",
        |ui| {
            toggle_row(
                ui,
                "保存历史记录",
                vm.settings.remember_history,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::RememberHistory,
                    ));
                },
            );
            combo_index_row(
                ui,
                "历史记录保留",
                vm.settings.retention,
                &["不限制", "最近 30 天", "最近 7 天", "不保存"],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::Retention,
                        val,
                    ));
                },
            );
            text_row(ui, "历史记录上限", "500 条");
            action_row(
                ui,
                "数据管理",
                "清空历史记录",
                Some("清空操作将在数据桥接完成后执行".into()),
                actions,
            );
        },
    );
}

fn advanced(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    card(
        ui,
        "Less Computer",
        "实验性的编码代理入口，仅 macOS 提供完整能力。",
        |ui| {
            toggle_row(
                ui,
                "启用 Less Computer",
                vm.settings.less_computer,
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::LessComputer));
                },
            );
            combo_index_row(
                ui,
                "权限模式",
                vm.settings.retention,
                &["接受编辑", "计划模式", "默认", "绕过权限"],
                |val| {
                    actions.push(FrontendAction::SettingsCombo(
                        SettingsComboField::Retention,
                        val,
                    ));
                },
            );
            text_row(ui, "工作目录", "当前项目");
        },
    );
    card(
        ui,
        "Claude 控制台",
        "检测 Claude CLI、MCP 和 computer use 状态。",
        |ui| {
            action_row(
                ui,
                "检测状态",
                "重新检测",
                Some("未连接到 Claude CLI（占位）".into()),
                actions,
            );
            action_row(
                ui,
                "控制台",
                if vm.settings.claude_expanded {
                    "收起控制台"
                } else {
                    "展开控制台"
                },
                None,
                actions,
            );
        },
    );
    card(
        ui,
        "多模态管线",
        "实验性的图片和音频上下文处理。",
        |ui| {
            toggle_row(ui, "启用多模态处理", vm.settings.multimodal, || {
                actions.push(FrontendAction::SettingsToggle(SettingsField::Multimodal));
            });
            text_row(ui, "处理方式", "跟随当前服务");
        },
    );
    card(
        ui,
        "调试工具",
        "导出诊断信息，帮助定位输入和插入问题。",
        |ui| {
            toggle_row(
                ui,
                "为调试保存录音",
                vm.settings.record_audio,
                || {
                    actions.push(FrontendAction::SettingsToggle(SettingsField::RecordAudio));
                },
            );
            toggle_row(
                ui,
                "记录光标探针",
                vm.settings.conservative_layout,
                || {
                    actions.push(FrontendAction::SettingsToggle(
                        SettingsField::ConservativeLayout,
                    ));
                },
            );
            action_row(ui, "诊断信息", "导出错误日志", None, actions);
        },
    );
}

fn about(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    card(
        ui,
        "OpenLess",
        "版本信息、更新渠道与项目链接。",
        |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("OpenLess").size(17.0).strong());
                ui.label(
                    egui::RichText::new(format!("{}-Beta", vm.version))
                        .size(12.0)
                        .color(theme::INK_3),
                );
            });
            action_row(
                ui,
                "正式版更新",
                "检查更新",
                Some("当前已是最新版本（占位）".into()),
                actions,
            );
            toggle_row(ui, "加入 Beta 渠道", vm.settings.beta_channel, || {
                actions.push(FrontendAction::SettingsToggle(SettingsField::BetaChannel));
            });
            toggle_row(ui, "自动检查更新", vm.settings.auto_update, || {
                actions.push(FrontendAction::SettingsToggle(SettingsField::AutoUpdate));
            });
        },
    );
    card(
        ui,
        "文档与反馈",
        "获取帮助、查看源码或提交问题。",
        |ui| {
            for (label, action) in [
                ("GitHub", "打开 GitHub"),
                ("文档", "打开帮助中心"),
                ("发行说明", "打开发行说明"),
                ("反馈", "打开问题反馈"),
            ] {
                action_row(ui, label, action, None, actions);
            }
            text_row(ui, "QQ群", "1078960553");
            action_row(ui, "QQ群", "复制群号", Some("复制操作占位".into()), actions);
        },
    );
}

// ── Card & row helpers ──────────────────────────────────────────────────────

fn card(ui: &mut egui::Ui, title: &str, description: &str, contents: impl FnOnce(&mut egui::Ui)) {
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
            contents(ui);
        });
    ui.add_space(10.0);
}

fn toggle_row(ui: &mut egui::Ui, label: &str, value: bool, on_toggle: impl FnOnce()) {
    row(ui, label, |ui| {
        let text = if value { "开" } else { "关" };
        if ui
            .add(
                egui::Button::new(egui::RichText::new(text).size(11.5))
                    .fill(if value {
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
            on_toggle();
        }
    });
}

fn combo_index_row(
    ui: &mut egui::Ui,
    label: &str,
    value: usize,
    options: &[&str],
    on_change: impl FnOnce(usize),
) {
    row(ui, label, |ui| {
        let mut selected = value;
        egui::ComboBox::from_id_salt(("settings", label))
            .selected_text(options.get(value).copied().unwrap_or("选择"))
            .show_ui(ui, |ui| {
                for (index, option) in options.iter().enumerate() {
                    let response = ui.selectable_label(selected == index, *option);
                    if response.clicked() {
                        selected = index;
                        ui.close();
                    }
                }
            });
        if selected != value {
            on_change(selected);
        }
    });
}

fn combo_bool_row(
    ui: &mut egui::Ui,
    label: &str,
    value: bool,
    options: &[&str],
    on_change: impl FnOnce(),
) {
    row(ui, label, |ui| {
        let selected = if value {
            options.first()
        } else {
            options.get(1)
        };
        let mut new_value = value;
        egui::ComboBox::from_id_salt(("settings", label))
            .selected_text(selected.copied().unwrap_or("选择"))
            .show_ui(ui, |ui| {
                for (index, option) in options.iter().enumerate() {
                    let response = ui.selectable_label((index == 0) == value, *option);
                    if response.clicked() {
                        new_value = index == 0;
                        ui.close();
                    }
                }
            });
        if new_value != value {
            on_change();
        }
    });
}

fn text_row(ui: &mut egui::Ui, label: &str, value: &str) {
    row(ui, label, |ui| {
        ui.label(egui::RichText::new(value).size(12.0).color(theme::INK_2));
    });
}

fn text_edit_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    _on_change: impl FnOnce(),
) {
    row(ui, label, |ui| {
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .desired_width(ui.available_width().min(300.0)),
        );
    });
}

fn status_row(ui: &mut egui::Ui, label: &str, status: &str, color: egui::Color32) {
    row(ui, label, |ui| {
        ui.label(egui::RichText::new(status).size(12.0).color(color));
    });
}

fn action_row(
    ui: &mut egui::Ui,
    label: &str,
    action: &str,
    _notice: Option<String>,
    actions: &mut Vec<FrontendAction>,
) {
    row(ui, label, |ui| {
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
            // Map common action labels to specific action fields
            match action {
                "测试配置" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::ConnectionTest,
                )),
                "选择目录" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::ModelManagement,
                )),
                "管理扩展" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::ExtensionManagement,
                )),
                "打开系统设置" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::Permissions,
                )),
                "清空历史记录" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::ClearHistory,
                )),
                "重新检测" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::ClaudeDetect,
                )),
                "展开控制台" | "收起控制台" => actions.push(
                    FrontendAction::SettingsAction(SettingsActionField::ClaudeConsole),
                ),
                "执行" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::ClaudeRunTest,
                )),
                "导出错误日志" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::ExportDiagnostics,
                )),
                "检查更新" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::CheckUpdate,
                )),
                "打开 GitHub" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::OpenGitHub,
                )),
                "打开帮助中心" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::OpenHelp,
                )),
                "打开发行说明" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::OpenReleaseNotes,
                )),
                "打开问题反馈" => actions.push(FrontendAction::SettingsAction(
                    SettingsActionField::OpenFeedback,
                )),
                "复制群号" => {
                    actions.push(FrontendAction::SettingsAction(SettingsActionField::CopyQQ))
                }
                _ => {}
            }
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
