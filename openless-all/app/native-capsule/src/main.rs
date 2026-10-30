//! Disposable rendering process. It has no credentials, microphone, models,
//! hotkey listener, or business state. Closing the parent pipe closes this UI.
#![cfg_attr(windows, windows_subsystem = "windows")]
use eframe::egui;
use openless_capsule::pill::{self, DefaultCapsule};
use openless_capsule::renderer::CapsuleAction;
use openless_capsule::{
    read_message, write_message, Action, Frame, HostMessage, ThemeMode, UiMessage,
    PROTOCOL_VERSION,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
mod platform;

struct CapsuleApp {
    incoming: mpsc::Receiver<Option<HostMessage>>,
    outgoing: std::io::Stdout,
    frame: Option<Frame>,
    shown: bool,
    window: Arc<winit::window::Window>,
    capsule: DefaultCapsule,
    reduce_motion: bool,
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
                            !frame.interactive
                                || frame.leaving
                                || frame.state.selection_polish
                                || frame.state.style == "siri",
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
        let action = if frame.state.style == "siri" && !frame.state.selection_polish {
            siri_transcript(ui, frame);
            openless_capsule::renderer::dictation_capsule(ui, &frame.state, &frame.labels)
        } else {
            let dark = match frame.theme {
                ThemeMode::Light => false,
                ThemeMode::Dark => true,
                ThemeMode::System => ui.ctx().system_theme() == Some(egui::Theme::Dark),
            };
            self.capsule.show(ui, frame, dark, self.reduce_motion)
        };
        let action = match action {
            CapsuleAction::None => None,
            CapsuleAction::Cancel => Some(Action::Cancel),
            CapsuleAction::Confirm => Some(Action::Confirm),
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

/// The spectral style keeps a compact transcript strip above the orb.
fn siri_transcript(ui: &egui::Ui, frame: &Frame) {
    if !frame.show_transcript || frame.transcript.is_empty() {
        return;
    }
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
            font_id: egui::FontId::proportional(frame.transcript_font_size.clamp(11.0, 24.0)),
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

/// One fallback slot: the candidate (file, face) for the regular, medium and
/// semibold families, as CSS weight matching picks static faces.
type Slot = [Vec<(PathBuf, u32)>; 3];

fn same(path: impl Into<PathBuf>, faces: [u32; 3]) -> Slot {
    let path = path.into();
    faces.map(|face| vec![(path.clone(), face)])
}

/// `--ol-font-sans` (system-ui, -apple-system, 'PingFang SC', 'Microsoft
/// YaHei') followed by the system fallbacks for Hangul and symbols.
#[cfg(target_os = "macos")]
fn font_slots() -> Vec<Slot> {
    // PingFang SC ships as an on-demand font asset; the UI copy
    // (PingFangUI.ttc) uses an outline format egui cannot draw.
    let pingfang = std::fs::read_dir("/System/Library/AssetsV2")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|kind| {
            kind.file_name()
                .to_string_lossy()
                .starts_with("com_apple_MobileAsset_Font")
        })
        .flat_map(|kind| std::fs::read_dir(kind.path()).into_iter().flatten().flatten())
        .map(|asset| asset.path().join("AssetData/PingFang.ttc"))
        .find(|path| path.is_file());
    let mut slots = vec![same("/System/Library/Fonts/SFNS.ttf", [0; 3])];
    slots.extend(pingfang.map(|path| same(path, [3, 7, 11])));
    slots.push(same("/System/Library/Fonts/Hiragino Sans GB.ttc", [0, 0, 2]));
    slots.push(same("/System/Library/Fonts/AppleSDGothicNeo.ttc", [0, 2, 4]));
    slots.push(same("/System/Library/Fonts/Apple Symbols.ttf", [0; 3]));
    slots
}

#[cfg(windows)]
fn font_slots() -> Vec<Slot> {
    let fonts = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
        .join("Fonts");
    let face = |file: &str| (fonts.join(file), 0);
    vec![
        [
            vec![face("SegUIVar.ttf"), face("segoeui.ttf")],
            vec![face("SegUIVar.ttf"), face("segoeui.ttf")],
            vec![face("SegUIVar.ttf"), face("seguisb.ttf")],
        ],
        [vec![face("msyh.ttc")], vec![face("msyh.ttc")], vec![face("msyhbd.ttc")]],
        [vec![face("malgun.ttf")], vec![face("malgun.ttf")], vec![face("malgunbd.ttf")]],
        same(fonts.join("seguisym.ttf"), [0; 3]),
    ]
}

#[cfg(not(any(target_os = "macos", windows)))]
fn font_slots() -> Vec<Slot> {
    vec![
        same("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", [0; 3]),
        same("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", [0; 3]),
    ]
}

/// Installs the regular family plus the medium and semibold families the
/// pill asks for. Files are memory-mapped, so only pages holding drawn glyphs
/// become resident instead of reading ~60 MB of collections per helper.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let bundled = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut maps: HashMap<PathBuf, Option<Arc<memmap2::Mmap>>> = HashMap::new();
    let mut primary_baseline = None;
    let mut chains: [Vec<String>; 3] = Default::default();
    for slot in font_slots() {
        for (chain, candidates) in chains.iter_mut().zip(slot) {
            let found = candidates.into_iter().find_map(|(path, face)| {
                let map = maps
                    .entry(path.clone())
                    .or_insert_with(|| {
                        let file = std::fs::File::open(&path).ok()?;
                        // SAFETY: system font files are not truncated while the helper runs.
                        unsafe { memmap2::Mmap::map(&file) }.ok().map(Arc::new)
                    })
                    .clone()?;
                let baseline = baseline(&map, face)?;
                Some((format!("{}#{face}", path.display()), map, face, baseline))
            });
            let Some((name, map, face, baseline)) = found else {
                continue;
            };
            if !fonts.font_data.contains_key(&name) {
                // egui centres a fallback face's own line box in the row;
                // CSS keeps every glyph on the primary face's baseline.
                let primary = *primary_baseline.get_or_insert(baseline);
                let tweak = egui::FontTweak {
                    y_offset_factor: primary - baseline,
                    ..Default::default()
                };
                let data = egui::FontData::from_blob(map, face).tweak(tweak);
                fonts.font_data.insert(name.clone(), Arc::new(data));
            }
            chain.push(name);
        }
    }
    let [regular, medium, semibold] = chains;
    for (family, chain) in [
        (egui::FontFamily::Proportional, regular),
        (egui::FontFamily::Name(pill::MEDIUM_FAMILY.into()), medium),
        (egui::FontFamily::Name(pill::SEMIBOLD_FAMILY.into()), semibold),
    ] {
        // egui's bundled faces stay behind the system ones as a last resort.
        let names = chain.into_iter().chain(bundled.iter().cloned()).collect();
        fonts.families.insert(family, names);
    }
    ctx.set_fonts(fonts);
}

/// Baseline below the centre of egui's line box for this face, in em: half
/// of ascent + descent − line gap (descent is negative). Faces without
/// supported outlines are rejected before entering the fallback chain.
fn baseline(bytes: &[u8], face: u32) -> Option<f32> {
    use skrifa::{instance::LocationRef, instance::Size, raw::TableProvider, MetadataProvider, Tag};
    let font = skrifa::FontRef::from_index(bytes, face).ok()?;
    [*b"glyf", *b"CFF ", *b"CFF2"]
        .into_iter()
        .any(|tag| font.table_data(Tag::new(&tag)).is_some())
        .then_some(())?;
    let _ = font.head().ok()?;
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let em = f32::from(metrics.units_per_em.max(1));
    Some(0.5 * (metrics.ascent + metrics.descent - metrics.leading) / em)
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
            .with_inner_size([460.0, 158.0])
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
                capsule: DefaultCapsule::default(),
                reduce_motion: platform::reduce_motion(),
            }))
        }),
    )
}
