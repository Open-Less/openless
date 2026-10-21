//! Disposable rendering process. It has no credentials, microphone, models,
//! hotkey listener, or business state. Closing the parent pipe closes this UI.
#![cfg_attr(windows, windows_subsystem = "windows")]
use eframe::egui;
use openless_capsule::{
    read_message, write_message, Action, Frame, HostMessage, UiMessage, PROTOCOL_VERSION,
};
use std::sync::{mpsc, Arc, Mutex};
mod platform;

struct CapsuleApp {
    incoming: mpsc::Receiver<Option<HostMessage>>,
    outgoing: std::io::Stdout,
    frame: Option<Frame>,
    shown: bool,
    window: Arc<winit::window::Window>,
}

impl eframe::App for CapsuleApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        while let Ok(message) = self.incoming.try_recv() {
            match message {
                Some(HostMessage::Frame { version, frame }) if version == PROTOCOL_VERSION => {
                    if !frame.visible() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                    if self
                        .frame
                        .as_ref()
                        .is_some_and(|current| frame.sequence < current.sequence)
                    {
                        continue;
                    }
                    // Do not show an empty renderer during font/window initialization.
                    if let Some(bounds) = frame.bounds {
                        platform::place(&self.window, bounds);
                    }
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::MousePassthrough(
                            !frame.interactive || frame.state.style == "siri",
                        ));
                    self.frame = Some(*frame);
                    if !self.shown {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        self.shown = true;
                    }
                }
                _ => {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }
        let Some(frame) = &self.frame else {
            return;
        };
        if !frame.transcript.is_empty() {
            let area = egui::Rect::from_min_size(egui::pos2(12.0, 4.0), egui::vec2(436.0, 40.0));
            ui.painter()
                .rect_filled(area, 12, egui::Color32::from_black_alpha(205));
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = 420.0;
            job.wrap.max_rows = 1;
            job.append(
                &frame.transcript,
                0.0,
                egui::text::TextFormat {
                    font_id: egui::FontId::proportional(
                        frame.transcript_font_size.clamp(11.0, 24.0),
                    ),
                    color: egui::Color32::WHITE,
                    ..Default::default()
                },
            );
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                area.center() - galley.size() * 0.5,
                galley,
                egui::Color32::WHITE,
            );
        }
        let action = openless_capsule::renderer::dictation_capsule(ui, &frame.state, &frame.labels);
        let action = match action {
            openless_capsule::renderer::CapsuleAction::None => None,
            openless_capsule::renderer::CapsuleAction::Cancel => Some(Action::Cancel),
            openless_capsule::renderer::CapsuleAction::Confirm => Some(Action::Confirm),
        };
        if let Some(action) =
            action.filter(|action| frame.accepts(&frame.session_id, frame.sequence, *action))
        {
            let message = UiMessage::Action {
                version: PROTOCOL_VERSION,
                session_id: frame.session_id.clone(),
                sequence: frame.sequence,
                action,
            };
            if write_message(&mut self.outgoing, &message).is_err() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // System CJK fonts stay inside the disposable UI process.
    #[cfg(target_os = "macos")]
    let candidates = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
    ];
    #[cfg(windows)]
    let candidates = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
    ];
    #[cfg(not(any(target_os = "macos", windows)))]
    let candidates = [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "openless-cjk".into(),
                egui::FontData::from_owned(bytes).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("openless-cjk".into());
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    platform::configure_early().map_err(eframe::Error::AppCreation)?;
    let (sender, incoming) = mpsc::sync_channel(2);
    let context = Arc::new(Mutex::new(None::<egui::Context>));
    let context_for_reader = Arc::clone(&context);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        loop {
            let message = read_message::<HostMessage>(&mut input).ok().flatten();
            let done = message.is_none() || matches!(message, Some(HostMessage::Shutdown { .. }));
            if sender.send(message).is_err() {
                return;
            }
            if let Some(ctx) = context_for_reader.lock().unwrap().as_ref() {
                ctx.request_repaint();
            }
            if done {
                return;
            }
        }
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenLess Capsule")
            .with_inner_size([460.0, 110.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_active(false)
            .with_visible(false)
            .with_resizable(false),
        renderer: eframe::Renderer::Glow,
        window_builder: Some(Box::new(platform::configure_window)),
        event_loop_builder: Some(Box::new(platform::configure_event_loop)),
        ..Default::default()
    };
    eframe::run_native(
        "OpenLess Capsule",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            platform::configure_native(cc)?;
            *context.lock().unwrap() = Some(cc.egui_ctx.clone());
            write_message(
                &mut std::io::stdout(),
                &UiMessage::Ready {
                    version: PROTOCOL_VERSION,
                },
            )?;
            cc.egui_ctx.request_repaint();
            let window = cc
                .winit_window()
                .cloned()
                .ok_or("native capsule has no window")?;
            Ok(Box::new(CapsuleApp {
                incoming,
                outgoing: std::io::stdout(),
                frame: None,
                shown: false,
                window,
            }))
        }),
    )
}
