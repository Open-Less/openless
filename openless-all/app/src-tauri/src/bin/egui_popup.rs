// egui 弹窗独立进程；也可被 Linux 主程序内嵌复用。

use std::io::{BufRead, Write};
use std::sync::mpsc::{self, Receiver, Sender};

use openless_lib::egui_host::fonts::install_cjk_fonts;
use openless_lib::egui_host::{
    qa::{apply_qa_state, QaViewState},
    qa_event::QaStateEvent,
};

#[derive(Debug)]
enum Msg {
    PreviewSet { text: String, source: String },
    QaState(QaStateEvent),
    Hide,
}

struct PopupApp {
    rx: Receiver<Msg>,
    kind: String,
    // 预览状态
    preview_text: String,
    preview_source: String,
    preview_focus: bool,
    // QA 状态
    qa: Option<QaViewState>,
    // 需要发送到 stdout 的操作（本帧累积，帧末 flush）
    outgoing: Vec<String>,
    close_requested: bool,
}

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args
        .iter()
        .find(|arg| arg.as_str() == "--preview" || arg.as_str() == "--qa")
        .map(String::as_str)
        .unwrap_or("--preview")
        .to_string();
    eprintln!("[egui-popup] starting kind={kind}");

    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            if let Some(msg) = parse_stdin(&line) {
                let _ = tx.send(msg);
            }
        }
    });

    let app = PopupApp {
        rx,
        kind: kind.clone(),
        preview_text: String::new(),
        preview_source: String::new(),
        preview_focus: true,
        qa: None,
        outgoing: Vec::new(),
        close_requested: false,
    };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(if kind == "--qa" {
                    "OpenLess QA"
                } else {
                    "OpenLess 选区润色预览"
                })
                .with_inner_size(if kind == "--qa" {
                    [420.0, 540.0]
                } else {
                    [640.0, 440.0]
                })
                .with_min_inner_size([360.0, 320.0])
                .with_resizable(kind != "--qa")
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top(),
            ..Default::default()
        };
        #[cfg(target_os = "linux")]
        {
            options.event_loop_builder = Some(Box::new(|b| {
                use winit::platform::wayland::EventLoopBuilderExtWayland as _;
                b.with_any_thread(true);
            }));
        }
        let _ = eframe::run_native(
            "openless-egui-popup",
            options,
            Box::new(move |cc| {
                install_cjk_fonts(&cc.egui_ctx);
                cc.egui_ctx.set_visuals(egui::Visuals::light());
                cc.egui_ctx.style_mut_of(egui::Theme::Light, |style| {
                    style.visuals.window_corner_radius = egui::CornerRadius::same(12);
                    style.visuals.panel_fill = egui::Color32::TRANSPARENT;
                });
                Ok(Box::new(app))
            }),
        );
    }));
    if let Err(panic) = result {
        eprintln!("[egui-popup] panicked: {panic:?}");
    }
}

impl eframe::App for PopupApp {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| self.render(ui, frame));
    }
}

impl PopupApp {
    fn render(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 强制持续重绘，确保按钮点击、键盘输入、流式消息都能及时响应。
        ui.ctx().request_repaint();

        // 读取 stdin 消息。
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::PreviewSet { text, source } => {
                    self.preview_text = text;
                    self.preview_source = source;
                    self.preview_focus = true;
                }
                Msg::QaState(event) => {
                    if self.qa.is_none() {
                        self.qa = Some(QaViewState::new());
                    }
                    if let Some(qa) = self.qa.as_mut() {
                        apply_qa_state(qa, &event);
                    }
                }
                Msg::Hide => {
                    self.close_requested = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // Escape 是两个弹窗一致的取消/关闭快捷键；不能只依赖窗口管理器的关闭事件，
        // 否则主进程收不到 cancel/dismiss，pending 状态会残留。
        let escape_pressed = ui.ctx().input(|input| {
            input.key_pressed(egui::Key::Escape)
                || input.raw.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Escape,
                            pressed: true,
                            ..
                        }
                    )
                })
        });
        if escape_pressed {
            if self.kind == "--qa" {
                self.outgoing.push("{ \"action\": \"dismiss\" }".into());
            } else {
                self.outgoing.push("{ \"action\": \"cancel\" }".into());
            }
            self.close_requested = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // 渲染内容。
        // 所有绘制和布局共享同一个显式 Card 矩形，避免 Frame 的内容 rect 与
        // viewport max_rect 不一致造成分隔线、footer 越过圆角边界。
        let card = ui.max_rect().shrink(8.0);
        ui.painter().rect_filled(
            card,
            egui::CornerRadius::same(24),
            egui::Color32::from_rgb(255, 255, 255),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
            if self.kind == "--qa" {
                if self.qa.is_none() {
                    self.qa = Some(QaViewState::new());
                }
                let qa = self.qa.as_mut().unwrap();
                render_qa(ui, qa, &mut self.outgoing);
            } else {
                render_preview(
                    ui,
                    &mut self.preview_text,
                    &mut self.preview_source,
                    &mut self.preview_focus,
                    &mut self.outgoing,
                );
            }
        });

        // 帧末 flush 所有待发送的 stdout 消息。
        if !self.outgoing.is_empty() {
            let drain: Vec<String> = std::mem::take(&mut self.outgoing);
            flush_stdout(&drain);
        }
    }
}

// ───────────────────────── preview UI ─────────────────────────

fn render_preview(
    ui: &mut egui::Ui,
    text: &mut String,
    source: &mut String,
    focus: &mut bool,
    outgoing: &mut Vec<String>,
) {
    let rect = ui.max_rect();
    let header = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + 54.0));
    let footer = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 60.0), rect.max);
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, header.max.y),
        egui::pos2(rect.max.x, footer.min.y),
    );
    let drag = ui.interact(header, ui.id().with("preview_drag"), egui::Sense::drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(header.shrink(10.0)), |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new("选区润色预览").strong().size(16.0))
                        .selectable(false),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new("可直接编辑；点击确认后才会替换原选区。")
                            .size(11.0)
                            .color(egui::Color32::from_gray(145)),
                    )
                    .selectable(false),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if close_button(ui) {
                    outgoing.push("{ \"action\": \"cancel\" }".into());
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(body.shrink(10.0)), |ui| {
        // 原版中原文是 textarea 后、按钮前的普通内容块，不属于 footer，
        // 最多占两行（42px）。必须预留完整高度，避免长原文压到按钮区域。
        let source_h = if source.is_empty() { 0.0 } else { 50.0 };
        let editor_rect = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(
                ui.available_width(),
                (ui.available_height() - source_h).max(80.0),
            ),
        );
        ui.allocate_rect(editor_rect, egui::Sense::hover());
        ui.painter().rect_filled(
            editor_rect,
            egui::CornerRadius::same(10),
            egui::Color32::WHITE,
        );
        ui.painter().rect_stroke(
            editor_rect,
            egui::CornerRadius::same(10),
            egui::Stroke::new(1.0, egui::Color32::from_gray(210)),
            egui::StrokeKind::Inside,
        );
        let resp = ui.put(
            editor_rect.shrink(8.0),
            egui::TextEdit::multiline(text)
                .desired_width(f32::INFINITY)
                .frame(false),
        );
        if *focus {
            *focus = false;
            resp.request_focus();
        }
        if !source.is_empty() {
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("原文：{source}"))
                        .size(11.0)
                        .color(egui::Color32::from_gray(145)),
                )
                .wrap(),
            );
        }
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(footer.shrink(10.0)), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !text.trim().is_empty(),
                    egui::Button::new(
                        egui::RichText::new("✓  确认并替换").color(egui::Color32::WHITE),
                    )
                    .fill(egui::Color32::from_rgb(40, 98, 235))
                    .corner_radius(egui::CornerRadius::same(7))
                    .min_size(egui::vec2(112.0, 34.0)),
                )
                .clicked()
            {
                outgoing.push(format!(
                    "{{ \"action\": \"confirm\", \"text\": {} }}",
                    json_escape(text)
                ));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if ui
                .add(
                    egui::Button::new("取消")
                        .fill(egui::Color32::WHITE)
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(205)))
                        .corner_radius(egui::CornerRadius::same(7))
                        .min_size(egui::vec2(52.0, 34.0)),
                )
                .clicked()
            {
                outgoing.push("{ \"action\": \"cancel\" }".into());
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    });
}

// ───────────────────────── QA UI ─────────────────────────

fn render_qa(ui: &mut egui::Ui, s: &mut QaViewState, outgoing: &mut Vec<String>) {
    let rect = ui.max_rect();
    let recording = s.current_kind == "recording";
    let has_recording_preview = recording
        && s.selection_preview
            .as_deref()
            .is_some_and(|preview| !preview.trim().is_empty());
    let footer_height = if has_recording_preview { 132.0 } else { 106.0 };
    let header = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + 88.0));
    let footer =
        egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - footer_height), rect.max);
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, header.max.y),
        egui::pos2(rect.max.x, footer.min.y),
    );
    let drag = ui.interact(header, ui.id().with("qa_drag"), egui::Sense::drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    ui.painter().hline(
        rect.x_range(),
        header.max.y,
        egui::Stroke::new(1.0, egui::Color32::from_gray(225)),
    );
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(header.shrink2(egui::vec2(20.0, 14.0))),
        |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new("划词追问").strong().size(16.0))
                            .selectable(false),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new("随时提问")
                                .size(14.0)
                                .color(egui::Color32::from_gray(125)),
                        )
                        .selectable(false),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if close_button(ui) {
                        outgoing.push("{ \"action\": \"dismiss\" }".into());
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        },
    );
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(footer.shrink2(egui::vec2(20.0, 9.0))),
        |ui| {
            let busy = matches!(s.current_kind.as_str(), "thinking" | "loading") || recording;
            let inner = ui.max_rect();
            let composer =
                egui::Rect::from_min_max(egui::pos2(inner.min.x, inner.max.y - 78.0), inner.max);
            if has_recording_preview {
                let preview = s.selection_preview.as_deref().unwrap_or_default();
                let chip = egui::Rect::from_min_max(
                    inner.min,
                    egui::pos2(inner.max.x, composer.min.y - 6.0),
                );
                ui.painter().rect_filled(
                    chip,
                    egui::CornerRadius::same(12),
                    egui::Color32::from_rgb(242, 242, 244),
                );
                ui.put(
                    chip.shrink2(egui::vec2(10.0, 5.0)),
                    egui::Label::new(
                        egui::RichText::new(format!("基于选中文本：{}", truncate(preview, 60)))
                            .size(11.0)
                            .color(egui::Color32::from_gray(120)),
                    )
                    .selectable(false)
                    .truncate(),
                );
            }
            ui.painter().rect_filled(
                composer,
                egui::CornerRadius::same(16),
                egui::Color32::from_rgb(244, 244, 246),
            );
            let input_rect = egui::Rect::from_min_size(
                composer.min + egui::vec2(10.0, 7.0),
                egui::vec2(composer.width() - 20.0, 28.0),
            );
            let resp = ui.put(
                input_rect,
                egui::TextEdit::singleline(&mut s.composer_text)
                    .hint_text("输入问题，Enter 发送")
                    .frame(false),
            );
            let submit_key = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mic_rect = egui::Rect::from_min_size(
                composer.min + egui::vec2(10.0, 39.0),
                egui::vec2(32.0, 32.0),
            );
            if ui
                .put(
                    mic_rect,
                    egui::Button::new(if recording { "■" } else { "♩" }).frame(false),
                )
                .clicked()
            {
                outgoing.push("{ \"action\": \"toggle_record\" }".into());
            }
            let send_rect = egui::Rect::from_min_size(
                egui::pos2(composer.max.x - 42.0, composer.min.y + 39.0),
                egui::vec2(32.0, 32.0),
            );
            let can_send = !busy && !s.composer_text.trim().is_empty();
            if ui
                .put(
                    send_rect,
                    egui::Button::new("↑")
                        .corner_radius(egui::CornerRadius::same(16))
                        .min_size(egui::vec2(32.0, 32.0)),
                )
                .clicked()
                && can_send
                || (submit_key && can_send)
            {
                let text = s.composer_text.trim().to_string();
                if !text.is_empty() {
                    s.composer_text.clear();
                    outgoing.push(format!(
                        "{{ \"action\": \"submit\", \"text\": {} }}",
                        json_escape(&text)
                    ));
                }
            }
        },
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(body.shrink(20.0)), |ui| {
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(body.width() - 40.0);
                let empty = s.messages.is_empty() && s.streaming_answer.is_empty();
                if empty {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("◌").size(28.0)); ui.add_space(12.0);
                            ui.label(egui::RichText::new("有什么可以帮你？").strong().size(20.0)); ui.add_space(8.0);
                            ui.add(egui::Label::new(egui::RichText::new("选中任意文字后开始追问，或直接在下方输入问题。回答会显示在这里，可以连续多轮。").size(13.0).color(egui::Color32::from_gray(120))).wrap());
                        });
                    });
                    return;
                }
                for msg in &s.messages {
                    let is_user = msg.role == "user";
                    let message_width = body.width() - 40.0;
                    if is_user {
                        let (selection, question) =
                            split_qa_user_message(&msg.content, msg.selection_text.as_deref());
                        if !selection.is_empty() {
                            let bubble_width = estimated_bubble_width(
                                &format!("“{}”", truncate(&selection, 120)),
                                message_width * 0.8,
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(bubble_width, 0.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::Frame::new()
                                            .corner_radius(egui::CornerRadius::same(20))
                                            .fill(egui::Color32::from_rgb(242, 242, 244))
                                            .inner_margin(egui::Margin::symmetric(12, 8))
                                            .show(ui, |ui| {
                                                ui.set_width((bubble_width - 24.0).max(20.0));
                                                ui.add(
                                                    egui::Label::new(
                                                        egui::RichText::new(format!(
                                                            "“{}”",
                                                            truncate(&selection, 120)
                                                        ))
                                                        .italics()
                                                        .size(12.0)
                                                        .color(egui::Color32::from_gray(105)),
                                                    )
                                                    .wrap(),
                                                );
                                            });
                                    },
                                );
                            });
                            ui.add_space(4.0);
                        }
                        let bubble_width =
                            estimated_bubble_width(&question, message_width * 0.8);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(bubble_width, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::Frame::new()
                                        .corner_radius(egui::CornerRadius::same(20))
                                        .fill(egui::Color32::from_rgb(24, 24, 27))
                                        .inner_margin(egui::Margin::symmetric(12, 8))
                                        .show(ui, |ui| {
                                            ui.set_width((bubble_width - 24.0).max(20.0));
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(question)
                                                        .size(13.0)
                                                        .color(egui::Color32::WHITE),
                                                )
                                                .wrap(),
                                            );
                                        });
                                },
                            );
                        });
                    } else {
                        // Markdown 会生成多个 Label。必须在独立的纵向布局中渲染，
                        // 否则继承水平消息布局后，剩余宽度会逐行缩窄成单字竖排。
                        ui.allocate_ui_with_layout(
                            egui::vec2(message_width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| render_markdown(ui, &msg.content),
                        );
                    }
                    ui.add_space(6.0);
                }
                if !s.streaming_answer.is_empty() {
                    ui.allocate_ui_with_layout(
                        egui::vec2(body.width() - 40.0, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| render_markdown(ui, &s.streaming_answer),
                    );
                }
                if matches!(s.current_kind.as_str(), "thinking" | "loading")
                    && s.streaming_answer.is_empty()
                {
                    ui.label(egui::RichText::new("思考中…").size(13.0).color(ui.visuals().weak_text_color()));
                }
                if let Some(error) = &s.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            });
    });
}

fn estimated_bubble_width(text: &str, max_width: f32) -> f32 {
    let longest_line = text
        .lines()
        .map(|line| {
            line.chars()
                .map(|ch| if ch.is_ascii() { 7.0 } else { 13.0 })
                .sum::<f32>()
        })
        .fold(0.0_f32, f32::max);
    (longest_line + 24.0).clamp(44.0, max_width)
}

/// 与 WebView 版 `splitQaUserMessage` 保持相同协议：安全选区信封只作为
/// 灰色引用上下文展示，`# 我的问题` 后面的内容才是用户气泡正文。
fn split_qa_user_message(content: &str, selection_text: Option<&str>) -> (String, String) {
    const ENVELOPE_START: &str = "<selected_text>\n";
    const ENVELOPE_END: &str = "\n</selected_text>\n\n# 我的问题\n";
    const LEGACY_START: &str = "# 选区原文\n";
    const LEGACY_END: &str = "\n\n# 我的问题\n";

    let parsed = content
        .strip_prefix(ENVELOPE_START)
        .and_then(|rest| rest.split_once(ENVELOPE_END))
        .or_else(|| {
            content
                .strip_prefix(LEGACY_START)
                .and_then(|rest| rest.split_once(LEGACY_END))
        });

    let (parsed_selection, question) = parsed
        .map(|(selection, question)| (selection.trim(), question.trim()))
        .unwrap_or(("", content.trim()));
    let selection = selection_text.unwrap_or(parsed_selection).trim();
    (selection.to_string(), question.to_string())
}

/// 轻量 Markdown 渲染：覆盖 QA 常见的标题、列表、引用、代码块和行内强调。
/// 不引入 WebView 或重量级 Markdown 组件，保证 Linux 单文件产物仍然自包含。
fn render_markdown(ui: &mut egui::Ui, markdown: &str) {
    let mut in_code = false;
    let mut code = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") && trimmed.ends_with("```") && trimmed.len() > 6 {
            render_code_block(ui, trimmed[3..trimmed.len() - 3].trim());
            continue;
        }
        if trimmed.starts_with("```") {
            if in_code {
                render_code_block(ui, code.trim_end());
                code.clear();
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push_str(line);
            code.push('\n');
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        let (text, size, color, strong, italics) = if let Some(value) = trimmed.strip_prefix("### ")
        {
            (value, 14.0, ui.visuals().text_color(), true, false)
        } else if let Some(value) = trimmed.strip_prefix("## ") {
            (value, 15.0, ui.visuals().text_color(), true, false)
        } else if let Some(value) = trimmed.strip_prefix("# ") {
            (value, 16.0, ui.visuals().text_color(), true, false)
        } else if let Some(value) = trimmed.strip_prefix("> ") {
            (value, 13.0, ui.visuals().weak_text_color(), false, true)
        } else if let Some(value) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            (value, 13.0, ui.visuals().text_color(), false, false)
        } else {
            (trimmed, 13.0, ui.visuals().text_color(), false, false)
        };
        let display = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            format!("• {text}")
        } else {
            text.to_string()
        };
        render_inline_markdown(ui, &display, size, color, strong, italics);
    }
    if in_code && !code.is_empty() {
        render_code_block(ui, code.trim_end());
    }
}

fn render_code_block(ui: &mut egui::Ui, code: &str) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(244, 244, 246))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(code).monospace().size(12.0)).wrap());
        });
}

fn render_inline_markdown(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    color: egui::Color32,
    strong: bool,
    italics: bool,
) {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    append_inline_segments(
        &mut job,
        text,
        size,
        color,
        ui.visuals().strong_text_color(),
        strong,
        italics,
    );
    ui.add(egui::Label::new(job).wrap());
}

fn append_inline_segments(
    job: &mut egui::text::LayoutJob,
    text: &str,
    size: f32,
    color: egui::Color32,
    strong_color: egui::Color32,
    base_strong: bool,
    base_italics: bool,
) {
    let mut rest = text;
    while !rest.is_empty() {
        let paired = [
            ("**", "**", true, false, false),
            ("__", "__", true, false, false),
            ("`", "`", false, false, true),
            ("*", "*", false, true, false),
            ("_", "_", false, true, false),
        ];
        let mut matched = false;
        for (open, close, strong, italics, code) in paired {
            if let Some(after_open) = rest.strip_prefix(open) {
                if let Some(end) = after_open.find(close) {
                    let value = &after_open[..end];
                    append_text_format(
                        job,
                        value,
                        size,
                        if strong || base_strong {
                            strong_color
                        } else {
                            color
                        },
                        base_italics || italics,
                        code,
                    );
                    rest = &after_open[end + close.len()..];
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            continue;
        }

        let next = ["**", "__", "`", "*", "_"]
            .iter()
            .filter_map(|marker| rest.find(marker))
            .min()
            .unwrap_or(rest.len());
        if next > 0 {
            append_text_format(
                job,
                &rest[..next],
                size,
                if base_strong { strong_color } else { color },
                base_italics,
                false,
            );
            rest = &rest[next..];
        } else {
            let len = rest.chars().next().map(char::len_utf8).unwrap_or(0);
            append_text_format(
                job,
                &rest[..len],
                size,
                if base_strong { strong_color } else { color },
                base_italics,
                false,
            );
            rest = &rest[len..];
        }
    }
}

fn append_text_format(
    job: &mut egui::text::LayoutJob,
    text: &str,
    size: f32,
    color: egui::Color32,
    italics: bool,
    code: bool,
) {
    let format = egui::TextFormat {
        font_id: egui::FontId::new(
            size,
            if code {
                egui::FontFamily::Monospace
            } else {
                egui::FontFamily::Proportional
            },
        ),
        color,
        background: if code {
            egui::Color32::from_rgb(238, 238, 240)
        } else {
            egui::Color32::TRANSPARENT
        },
        italics,
        ..Default::default()
    };
    job.append(text, 0.0, format);
}

/// 不依赖字体字形的关闭按钮。某些 CJK 字体没有 `✕`，直接显示会变成方块。
fn close_button(ui: &mut egui::Ui) -> bool {
    let response = ui.add(
        egui::Button::new("")
            .frame(false)
            .min_size(egui::vec2(28.0, 28.0)),
    );
    let rect = response.rect;
    let stroke = egui::Stroke::new(1.5, ui.visuals().text_color());
    let inset = 8.0;
    ui.painter().line_segment(
        [
            rect.left_top() + egui::vec2(inset, inset),
            rect.right_bottom() - egui::vec2(inset, inset),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            rect.right_top() + egui::vec2(-inset, inset),
            rect.left_bottom() + egui::vec2(inset, -inset),
        ],
        stroke,
    );
    response.clicked()
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

// ───────────────────────── stdio 辅助 ─────────────────────────

fn flush_stdout(lines: &[String]) {
    let mut out = std::io::stdout().lock();
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
}

fn parse_stdin(line: &str) -> Option<Msg> {
    if line.contains("\"hide\"") {
        return Some(Msg::Hide);
    }
    if line.contains("\"qa:state\"") {
        // 提取 payload 字段解析。
        if let Some(payload) = extract_str(line, "payload") {
            if let Ok(event) = serde_json::from_str::<QaStateEvent>(&payload) {
                return Some(Msg::QaState(event));
            }
        }
        return None;
    }
    let text = extract_str(line, "text");
    if text.is_some() {
        return Some(Msg::PreviewSet {
            text: text.unwrap_or_default(),
            source: extract_str(line, "source").unwrap_or_default(),
        });
    }
    None
}

fn extract_str(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let mut rest = line[start..].trim_start();
    // 处理 "payload": { ... } 这种嵌套对象：找到匹配的闭合括号。
    if rest.starts_with('{') {
        let mut depth = 0;
        let mut in_str = false;
        let mut out = String::new();
        for ch in rest.chars() {
            if in_str {
                out.push(ch);
                if ch == '"' {
                    in_str = false;
                }
                continue;
            }
            match ch {
                '\\' => {
                    out.push(ch);
                    in_str = true;
                }
                '"' => {
                    out.push(ch);
                    in_str = false;
                }
                '{' => {
                    depth += 1;
                    out.push(ch);
                }
                '}' => {
                    depth -= 1;
                    out.push(ch);
                    if depth == 0 {
                        break;
                    }
                }
                _ => out.push(ch),
            }
        }
        return Some(out);
    }
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        let raw = &stripped[..end];
        Some(
            raw.replace("\\\"", "\"")
                .replace("\\n", "\n")
                .replace("\\\\", "\\"),
        )
    } else {
        None
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}
