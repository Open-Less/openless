//! egui 弹窗独立进程。
//!
//! 每个弹窗一个独立进程 + 单窗口（Linux Wayland/X11），比 eframe 多 viewport 稳定。
//! 与主进程用 stdin/stdout JSON 行通信。
//!
//! 用法：
//!   `egui_popup --preview` 选区润色预览（一次性：打开→确认/取消→退出）
//!   `egui_popup --qa`       QA 问答面板（常驻：直到用户关窗）
//!
//! stdin（主进程 → 本进程）：
//!   预览: `{"action":"set","text":"...","source":"..."}`  `{"action":"hide"}`
//!   QA:   `{"action":"qa:state","payload":{kind,session_id,messages,selection_preview,chunk,error}}`
//!         `{"action":"hide"}`
//!
//! stdout（本进程 → 主进程）：
//!   预览: `{"action":"confirm","text":"..."}`  `{"action":"cancel"}`
//!   QA:   `{"action":"submit","text":"..."}`  `{"action":"toggle_record"}`  `{"action":"dismiss"}`

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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args.get(1).map(|s| s.as_str()).unwrap_or("--preview").to_string();
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
                .with_title(if kind == "--qa" { "OpenLess QA" } else { "OpenLess 选区润色预览" })
                .with_inner_size(if kind == "--qa" { [440.0, 560.0] } else { [640.0, 440.0] })
                .with_min_inner_size([360.0, 320.0])
                .with_decorations(false)
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
        egui::Color32::from_rgb(255, 255, 255).to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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

        // 渲染内容。
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
    egui::CentralPanel::default().show(ui, |ui| {
        // 顶栏
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("选区润色预览").strong().size(15.0));
            if ui.button("✕").clicked() {
                outgoing.push("{ \"action\": \"cancel\" }".into());
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.separator();
        // 可编辑区
        let edit_height = (ui.available_height() - 100.0).max(120.0);
        let resp = ui.add_sized(
            [ui.available_width(), edit_height],
            egui::TextEdit::multiline(text).hint_text("润色结果").desired_rows(8),
        );
        if *focus {
            *focus = false;
            resp.request_focus();
        }
        if !source.is_empty() {
            ui.label(egui::RichText::new(format!("原文：{source}")).size(11.0));
        }
        // 底部按钮
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!text.trim().is_empty(), egui::Button::new("确认并替换")).clicked() {
                outgoing.push(format!("{{ \"action\": \"confirm\", \"text\": {} }}", json_escape(text)));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if ui.button("取消").clicked() {
                outgoing.push("{ \"action\": \"cancel\" }".into());
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    });
}

// ───────────────────────── QA UI ─────────────────────────

fn render_qa(ui: &mut egui::Ui, s: &mut QaViewState, outgoing: &mut Vec<String>) {
    egui::CentralPanel::default().show(ui, |ui| {
        // 顶栏
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("AI 问答").strong().size(15.0));
            if ui.button("✕").clicked() {
                outgoing.push("{ \"action\": \"dismiss\" }".into());
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.separator();

        // 消息滚动区
        let avail = ui.available_size();
        let body_height = (avail.y - 70.0).max(80.0);
        egui::ScrollArea::vertical().stick_to_bottom(true).max_height(body_height).show(ui, |ui| {
            let empty = s.messages.is_empty() && s.streaming_answer.is_empty();
            if empty {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("还没有问题，试试按住说话或输入问题。").size(13.0).color(ui.visuals().weak_text_color()));
                });
                return;
            }
            for msg in &s.messages {
                let is_user = msg.role == "user";
                ui.with_layout(if is_user { egui::Layout::right_to_left(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Min) }, |ui| {
                    egui::Frame::new()
                        .corner_radius(6.0)
                        .fill(if is_user { ui.visuals().selection.bg_fill } else { ui.visuals().extreme_bg_color })
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_max_width(ui.available_width() * 0.8);
                            ui.label(egui::RichText::new(&msg.content).size(13.0));
                        });
                });
                ui.add_space(6.0);
            }
            if !s.streaming_answer.is_empty() {
                ui.label(egui::RichText::new(&s.streaming_answer).size(13.0).color(ui.visuals().weak_text_color()));
            }
            if matches!(s.current_kind.as_str(), "thinking" | "loading") && s.streaming_answer.is_empty() {
                ui.label(egui::RichText::new("思考中…").size(13.0).color(ui.visuals().weak_text_color()));
            }
            if let Some(error) = &s.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });

        // 录音选区 chip
        if s.current_kind == "recording" {
            if let Some(preview) = &s.selection_preview {
                ui.label(egui::RichText::new(format!("选区：{}", truncate(preview, 60))).size(11.0).color(ui.visuals().weak_text_color()));
            }
        }

        ui.separator();
        let recording = s.current_kind == "recording";
        let busy = matches!(s.current_kind.as_str(), "thinking" | "loading") || recording;
        ui.horizontal(|ui| {
            let input = egui::TextEdit::singleline(&mut s.composer_text)
                .hint_text("输入问题，回车发送")
                .desired_width(ui.available_width() - 40.0);
            let resp = ui.add(input);
            let submit_key = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mic_label = if recording { "■ 停止" } else { "🎤 说话" };
            if ui.add_enabled(!busy || recording, egui::Button::new(mic_label)).clicked() {
                outgoing.push("{ \"action\": \"toggle_record\" }".into());
            }
            let can_send = !busy && !s.composer_text.trim().is_empty();
            if ui.add_enabled(can_send, egui::Button::new("发送")).clicked() || (submit_key && can_send) {
                let text = s.composer_text.trim().to_string();
                if !text.is_empty() {
                    s.composer_text.clear();
                    outgoing.push(format!("{{ \"action\": \"submit\", \"text\": {} }}", json_escape(&text)));
                }
            }
        });
    });
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max { text.to_string() } else { format!("{}…", text.chars().take(max).collect::<String>()) }
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
                if ch == '"' { in_str = false; }
                continue;
            }
            match ch {
                '\\' => { out.push(ch); in_str = true; }
                '"' => { out.push(ch); in_str = false; }
                '{' => { depth += 1; out.push(ch); }
                '}' => { depth -= 1; out.push(ch); if depth == 0 { break; } }
                _ => out.push(ch),
            }
        }
        return Some(out);
    }
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        let raw = &stripped[..end];
        Some(raw.replace("\\\"", "\"").replace("\\n", "\n").replace("\\\\", "\\"))
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
