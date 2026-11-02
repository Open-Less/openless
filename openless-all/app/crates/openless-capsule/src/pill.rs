//! Draws the default recording capsule, translation target, transcript rail and
//! selection notice, anchored to the bottom of the helper window.
use crate::motion::{Spring, Tween, EASE, LINEAR, MORPH};
use crate::renderer::CapsuleAction;
use crate::{icons, shapes, Frame};
use egui::emath::TSTransform;
use egui::epaint::{Shadow, TextShape};
use egui::epaint::text::VariationCoords;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};
use std::sync::Arc;

const HEIGHT: f32 = 32.0;
const RADIUS: u8 = 16;
const RECORDING_WIDTH: f32 = 116.0;
const COMPACT_WIDTH: f32 = 68.0;
const STATUS_PADDING: f32 = 14.0;
/// `getCapsuleHostMetrics(.., 'classic').bottomInset`.
const BOTTOM_INSET: f32 = 16.0;
const BUTTON_RADIUS: f32 = 12.0;
/// Buttons sit 4px inside the fixed 116px recording layer.
const BUTTON_OFFSET: f32 = RECORDING_WIDTH / 2.0 - 4.0 - BUTTON_RADIUS;
// Measured from the 60 fps reference: 232×64 physical pixels at 2×.
const ENVELOPE: [f32; 11] = [0.22, 0.34, 0.48, 0.65, 0.82, 1.0, 0.82, 0.65, 0.48, 0.34, 0.22];
const STACK_GAP: f32 = 8.0;
const BADGE_HEIGHT: f32 = 28.0;
const RAIL_HEIGHT: f32 = 52.0;
const RAIL_PAD: f32 = 18.0;
const RAIL_MIN_WIDTH: f32 = 72.0;
/// `maxWidth={hostMetrics.width - 16}` in the 460px Classic host.
const RAIL_MAX_WIDTH: f32 = 444.0;
const RAIL_TAIL_CHARS: usize = 160;

const BLUE: Color32 = Color32::from_rgb(0x25, 0x63, 0xeb);
const BORDER: Color32 = Color32::from_rgb(0xd4, 0xd4, 0xd8);

struct PillPalette {
    fill: Color32,
    border: Color32,
    highlight: Color32,
    ink: Color32,
    sweep: Color32,
    cancel: Color32,
    cancel_hover: Color32,
    cancel_ink: Color32,
    confirm: Color32,
    confirm_hover: Color32,
    error: Color32,
    shadow: Color32,
}

fn pill_palette(dark: bool) -> PillPalette {
    let rgba = Color32::from_rgba_unmultiplied;
    if dark {
        PillPalette {
            fill: rgba(28, 34, 45, 224),
            border: rgba(218, 231, 255, 66),
            highlight: rgba(235, 244, 255, 33),
            ink: Color32::from_rgb(167, 199, 255),
            sweep: rgba(90, 145, 240, 56),
            cancel: rgba(225, 235, 255, 26),
            cancel_hover: rgba(225, 235, 255, 46),
            cancel_ink: Color32::from_rgb(226, 234, 247),
            confirm: Color32::from_rgb(57, 121, 243),
            confirm_hover: Color32::from_rgb(82, 142, 255),
            error: Color32::from_rgb(255, 155, 153),
            shadow: Color32::from_black_alpha(97),
        }
    } else {
        PillPalette {
            fill: rgba(255, 255, 255, 219),
            border: rgba(79, 91, 115, 61),
            highlight: rgba(255, 255, 255, 209),
            ink: Color32::from_rgb(29, 78, 216),
            sweep: rgba(147, 197, 253, 71),
            cancel: rgba(102, 116, 139, 26),
            cancel_hover: rgba(102, 116, 139, 46),
            cancel_ink: Color32::from_rgb(82, 93, 110),
            confirm: BLUE,
            confirm_hover: Color32::from_rgb(29, 78, 216),
            error: Color32::from_rgb(201, 45, 54),
            shadow: rgba(15, 23, 42, 46),
        }
    }
}

/// App-theme surfaces shared with the WebView pill, badge and transcript rail.
struct Tokens {
    pill: PillPalette,
    surface: Color32,
    line: Color32,
    shadow: Color32,
    ring: Color32,
    highlight: Color32,
    ink: Color32,
    medium: f32,
    semibold: f32,
    control: Color32,
    control_line: Color32,
    control_ink: Color32,
}

fn tokens(dark: bool) -> Tokens {
    let rgba = Color32::from_rgba_unmultiplied;
    if dark {
        Tokens {
            pill: pill_palette(dark),
            surface: Color32::from_rgb(0x1b, 0x22, 0x2c),
            line: rgba(226, 232, 240, 31),
            shadow: rgba(0, 0, 0, 122),
            ring: rgba(226, 232, 240, 15),
            highlight: rgba(255, 255, 255, 20),
            ink: Color32::from_rgb(0xf4, 0xf7, 0xfb),
            medium: 460.0,
            semibold: 560.0,
            control: Color32::from_rgb(29, 29, 32),
            control_line: Color32::from_rgb(0x3f, 0x3f, 0x46),
            control_ink: Color32::from_rgb(0xfa, 0xfa, 0xfa),
        }
    } else {
        Tokens {
            pill: pill_palette(dark),
            surface: Color32::from_rgb(0xfa, 0xfa, 0xfa),
            line: rgba(0, 0, 0, 26),
            shadow: rgba(15, 17, 22, 41),
            ring: rgba(0, 0, 0, 15),
            highlight: rgba(255, 255, 255, 184),
            ink: Color32::from_rgb(0x18, 0x18, 0x1b),
            medium: 500.0,
            semibold: 600.0,
            control: Color32::WHITE,
            control_line: BORDER,
            control_ink: Color32::from_rgb(0x09, 0x09, 0x0b),
        }
    }
}

/// Families the helper installs for static CJK faces at CSS weights 500 and
/// 600 (`Proportional` is 400). The variable system face covers every weight.
pub const MEDIUM_FAMILY: &str = "openless-medium";
pub const SEMIBOLD_FAMILY: &str = "openless-semibold";

/// `--ol-font-sans` at a CSS weight: the static face CSS weight matching
/// would pick, with the variable face's weight and optical size set exactly.
fn format(size: f32, weight: f32, line_height: f32, color: Color32) -> TextFormat {
    let family = if weight < 450.0 {
        egui::FontFamily::Proportional
    } else if weight < 550.0 {
        egui::FontFamily::Name(MEDIUM_FAMILY.into())
    } else {
        egui::FontFamily::Name(SEMIBOLD_FAMILY.into())
    };
    TextFormat {
        font_id: egui::FontId::new(size, family),
        line_height: Some(line_height),
        color,
        coords: VariationCoords::new([("wght", weight), ("opsz", size)]),
        ..Default::default()
    }
}

/// One line, ellipsized like `text-overflow: ellipsis` (cut at any glyph).
fn line(painter: &Painter, text: &str, format: TextFormat, max_width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), format);
    job.wrap = TextWrapping {
        max_width: max_width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

fn text(pos: Pos2, galley: Arc<egui::Galley>, opacity: f32) -> Shape {
    TextShape::new(pos, galley, Color32::PLACEHOLDER)
        .with_opacity_factor(opacity)
        .into()
}

/// Physical-pixel aligned square for a texture rasterized at `size` points.
fn snapped_square(center: Pos2, size: f32, pixels_per_point: f32) -> Rect {
    let side = (size * pixels_per_point).round() / pixels_per_point;
    let min = center - vec2(side, side) * 0.5;
    let min = pos2(
        (min.x * pixels_per_point).round() / pixels_per_point,
        (min.y * pixels_per_point).round() / pixels_per_point,
    );
    Rect::from_min_size(min, vec2(side, side))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Cancel,
    Confirm,
    /// `.ol-default-processing-cancel`: the whole processing pill.
    Processing,
}

/// `CapsuleWaveform`: the level is smoothed per frame, then shaped by the
/// envelope and a slow ripple.
#[derive(Default)]
struct Waveform {
    smoothed: f32,
    previous: Option<f64>,
    active: Option<bool>,
}

impl Waveform {
    fn heights(&mut self, now: f64, level: f32, active: bool, reduced: bool) -> [f32; 11] {
        if self.active != Some(active) {
            // The effect restarts from rest whenever `active` flips.
            self.active = Some(active);
            self.smoothed = 0.0;
            self.previous = None;
        }
        let voice = if active {
            ((level - 0.008) / 0.32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if reduced {
            // Under reduced motion the WebView draws a single frame per level.
            self.smoothed = voice * (1.0 - (-16.0_f32 / 35.0).exp());
        } else {
            let dt = self
                .previous
                .map_or(16.0, |previous| ((now - previous) * 1000.0).min(50.0) as f32);
            self.previous = Some(now);
            let tau = if voice > self.smoothed { 35.0 } else { 90.0 };
            self.smoothed += (voice - self.smoothed) * (1.0 - (-dt / tau).exp());
        }
        let phase = (now * 1000.0 / 85.0) as f32;
        let strength = self.smoothed.powf(0.6);
        std::array::from_fn(|index| {
            let ripple = if reduced {
                1.0
            } else {
                0.86 + 0.14 * (phase + index as f32 * 0.8).sin()
            };
            1.5 + 14.5 * strength * ENVELOPE[index] * ripple
        })
    }
}

/// `CapsuleTranscriptRail` (frost): a right-anchored line whose shell hugs the
/// text, growing on a spring and re-centring a little later.
struct Rail {
    text: String,
    births: Vec<f64>,
    galley_width: f32,
    width: Spring,
    right: Spring,
    shift: Spring,
    width_at: Option<(f64, f32)>,
    right_at: Option<(f64, f32)>,
    previous: Option<f64>,
}

impl Rail {
    fn new(stage_center: f32) -> Self {
        Self {
            text: String::new(),
            births: Vec::new(),
            galley_width: 0.0,
            // INSERT_TEXT_MOTION.widthSpring / recenterSpring / shiftSpring.
            width: Spring::new(RAIL_MIN_WIDTH, 330.0, 32.0, 0.9),
            right: Spring::new(stage_center + RAIL_MIN_WIDTH / 2.0, 180.0, 27.0, 1.0),
            shift: Spring::new(0.0, 260.0, 30.0, 1.0),
            width_at: None,
            right_at: None,
            previous: None,
        }
    }

    /// Births are per character of the whole transcript; only the tail is drawn.
    fn tail(&self) -> (&str, &[f64]) {
        let count = self.births.len();
        let skip = count.saturating_sub(RAIL_TAIL_CHARS);
        let start = self
            .text
            .char_indices()
            .nth(skip)
            .map_or(self.text.len(), |(index, _)| index);
        (&self.text[start..], &self.births[skip..])
    }

    fn job(&self, now: f64, size: f32, weight: f32, ink: Color32) -> LayoutJob {
        let (tail, births) = self.tail();
        let mut job = LayoutJob::default();
        let mut format = format(size, weight, size * 1.2, ink);
        // `glyphGapPx`: every measured glyph carries 1.4px of air.
        format.extra_letter_spacing = 1.4;
        let mut start = 0;
        let mut current = None;
        for ((index, _), birth) in tail.char_indices().zip(births) {
            // INSERT_TEXT_MOTION.bornFadeMs, quantized so a line keeps few sections.
            let alpha = (((now - birth) / 0.18).clamp(0.0, 1.0) * 16.0).round() as u8;
            if current.is_some_and(|value| value != alpha) {
                let mut section = format.clone();
                section.color = ink.gamma_multiply(current.unwrap() as f32 / 16.0);
                job.append(&tail[start..index], 0.0, section);
                start = index;
            }
            current = Some(alpha);
        }
        if let Some(alpha) = current {
            format.color = ink.gamma_multiply(alpha as f32 / 16.0);
            job.append(&tail[start..], 0.0, format);
        }
        job
    }

    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        painter: &Painter,
        text: &str,
        size: f32,
        weight: f32,
        ink: Color32,
        stage_center: f32,
        now: f64,
        reduced: bool,
    ) {
        if text != self.text {
            let common = self
                .text
                .chars()
                .zip(text.chars())
                .take_while(|(old, new)| old == new)
                .count();
            self.births.truncate(common);
            let born = text.chars().count() - common;
            // bornStaggerMs between the glyphs of one insertion.
            self.births
                .extend((0..born).map(|index| if reduced { 0.0 } else { now + 0.008 * index as f64 }));
            self.text = text.to_owned();
            let galley = painter.layout_job(self.job(now, size, weight, ink));
            let width = galley.size().x;
            let old_content = self.galley_width.min(RAIL_MAX_WIDTH - 2.0 * RAIL_PAD);
            let delta = width - self.galley_width;
            self.galley_width = width;
            let content = width.min(RAIL_MAX_WIDTH - 2.0 * RAIL_PAD);
            let target = (content + 2.0 * RAIL_PAD).clamp(RAIL_MIN_WIDTH, RAIL_MAX_WIDTH);
            let right = stage_center + target / 2.0;
            if reduced {
                self.width.jump(target);
                self.right.jump(right);
            } else {
                // `planCapsuleInsertMotion`: the change ripples out from where
                // the new glyphs are born, then the shell re-centres.
                let width_delay = (12.0 + (old_content / 2.35).min(96.0)) as f64 / 1000.0;
                self.width_at = Some((now + width_delay, target));
                self.right_at = Some((now + width_delay + 0.14, right));
                // Retained glyphs keep their place and slide to the new anchor.
                self.shift.value += delta;
            }
        }
        let dt = self
            .previous
            .map_or(0.0, |previous| (now - previous) as f32);
        self.previous = Some(now);
        for (spring, pending) in [
            (&mut self.width, &mut self.width_at),
            (&mut self.right, &mut self.right_at),
        ] {
            match *pending {
                // A stopped motion value holds still until its delayed spring starts.
                Some((at, _)) if now < at => {}
                Some((_, target)) => {
                    spring.jump(spring.value);
                    spring.target = target;
                    *pending = None;
                    spring.step(dt);
                }
                None => spring.step(dt),
            }
        }
        self.shift.step(dt);
    }

    fn animating(&self, now: f64) -> bool {
        self.width_at.is_some()
            || self.right_at.is_some()
            || self.width.active()
            || self.right.active()
            || self.shift.active()
            || self.births.last().is_some_and(|birth| now - birth < 0.18)
    }
}

pub struct DefaultCapsule {
    glass: Option<(Rect, f32)>,
    started: bool,
    width: Tween,
    opacity: Tween,
    lift: Tween,
    layer: Tween,
    actions: Tween,
    status: Tween,
    cancel_hover: Tween,
    confirm_hover: Tween,
    badge_opacity: Tween,
    badge_lift: Tween,
    badge_scale: Tween,
    sweep_since: Option<f64>,
    waveform: Waveform,
    pressed: Option<Target>,
    rail: Option<Rail>,
}

impl Default for DefaultCapsule {
    fn default() -> Self {
        Self {
            glass: None,
            started: false,
            width: Tween::at(COMPACT_WIDTH),
            opacity: Tween::at(0.0),
            lift: Tween::at(6.0),
            layer: Tween::at(0.0),
            actions: Tween::at(0.0),
            status: Tween::at(0.0),
            cancel_hover: Tween::at(0.0),
            confirm_hover: Tween::at(0.0),
            badge_opacity: Tween::at(0.0),
            badge_lift: Tween::at(7.0),
            badge_scale: Tween::at(0.94),
            sweep_since: None,
            waveform: Waveform::default(),
            pressed: None,
            rail: None,
        }
    }
}

/// `OpenLessPill`'s label for the non-recording states.
fn status_label(frame: &Frame) -> String {
    let state = &frame.state;
    let labels = &frame.labels;
    // Capsule.tsx shows the local recognition while transcribing.
    let live = frame.transcript.trim();
    let message = if state.phase == "transcribing" && !live.is_empty() {
        live
    } else {
        state.text.trim()
    };
    let fallback = |text: &str| {
        if message.is_empty() {
            text.to_owned()
        } else {
            message.to_owned()
        }
    };
    match state.phase.as_str() {
        "transcribing" | "polishing" => fallback(if state.operating {
            &labels.using
        } else {
            &labels.thinking
        }),
        "completed" => fallback(
            &labels
                .inserted
                .replace("{{count}}", &state.inserted_chars.unwrap_or(0).to_string()),
        ),
        "cancelled" => labels.cancelled.clone(),
        _ => fallback(&labels.error),
    }
}

impl DefaultCapsule {
    /// The platform material follows the painted pill, never the transparent host rectangle.
    pub fn glass_surface(&self) -> Option<(Rect, f32)> {
        self.glass
    }

    /// Draws the capsule for `frame` and reports a clicked control. `dark`
    /// is the resolved app theme for the pill and every companion surface.
    pub fn show(
        &mut self,
        ui: &egui::Ui,
        frame: &Frame,
        dark: bool,
        reduced: bool,
    ) -> CapsuleAction {
        let painter = ui.painter().clone();
        let now = ui.input(|input| input.time);
        let stage = ui.max_rect();
        let tokens = tokens(dark);
        self.glass = None;
        if frame.state.selection_polish {
            if notice(&painter, stage, frame, &tokens, now) && !reduced {
                ui.ctx().request_repaint();
            }
            return CapsuleAction::None;
        }

        let state = &frame.state;
        let phase = state.phase.as_str();
        let recording = phase == "recording";
        let warming = recording && state.warming;
        let processing = matches!(phase, "transcribing" | "polishing");
        let leaving = frame.leaving;
        let label = status_label(frame);
        let label_format = format(12.0, 500.0, 18.0, if phase == "failed" { tokens.pill.error } else { tokens.pill.ink });
        let label_width = painter
            .layout_job(LayoutJob::single_section(label.clone(), label_format.clone()))
            .size()
            .x;
        // `scrollWidth + 28`, rounded up so a label that fits is never ellipsized.
        let status_width = (label_width.ceil() + 2.0 * STATUS_PADDING).clamp(80.0, 300.0);
        let time = |seconds: f64| if reduced { 0.0 } else { seconds };

        if !self.started {
            self.started = true;
            let layer = (recording || warming) as u8 as f32;
            self.layer.jump(layer);
            self.status.jump(1.0 - layer);
            self.actions.jump(recording as u8 as f32);
            if reduced {
                self.width.jump(if warming { COMPACT_WIDTH } else if recording { RECORDING_WIDTH } else { status_width });
                self.opacity.jump(1.0);
                self.lift.jump(0.0);
            }
        }
        // Animate shell geometry independently from the content opacity.
        let width = if leaving || warming {
            COMPACT_WIDTH
        } else if recording {
            RECORDING_WIDTH
        } else {
            status_width
        };
        self.width.set(now, width, time(0.22), 0.0, MORPH);
        self.opacity.set(
            now,
            if leaving { 0.0 } else { 1.0 },
            time(if leaving { 0.18 } else { 0.24 }),
            0.0,
            MORPH,
        );
        self.lift.set(
            now,
            if leaving { 5.0 } else { 0.0 },
            time(if leaving { 0.18 } else { 0.24 }),
            0.0,
            MORPH,
        );
        // CSS opacity transitions of the layers inside the shell.
        let layer_visible = recording || warming;
        self.layer
            .set(now, layer_visible as u8 as f32, time(0.09), time(0.06), LINEAR);
        self.actions.set(
            now,
            recording as u8 as f32,
            time(0.08),
            time(if processing { 0.05 } else { 0.0 }),
            LINEAR,
        );
        self.status.set(
            now,
            !layer_visible as u8 as f32,
            time(0.08),
            time(if layer_visible { 0.0 } else { 0.12 }),
            LINEAR,
        );
        let badge_visible = state.translation_active && recording && !leaving;
        let (badge_duration, badge_delay) = if badge_visible { (0.2, 0.04) } else { (0.12, 0.0) };
        for (tween, target) in [
            (&mut self.badge_opacity, if badge_visible { 1.0 } else { 0.0 }),
            (&mut self.badge_lift, if badge_visible { 0.0 } else { 7.0 }),
            (&mut self.badge_scale, if badge_visible { 1.0 } else { 0.94 }),
        ] {
            tween.set(now, target, time(badge_duration), time(badge_delay), MORPH);
        }
        if processing {
            self.sweep_since.get_or_insert(now);
        } else {
            self.sweep_since = None;
        }

        // Geometry: the Classic stack is bottom-anchored in the 460px host.
        let center_x = stage.center().x;
        let body_bottom = stage.bottom() - BOTTOM_INSET;
        let body_top = body_bottom - HEIGHT;
        let width = self.width.value(now);
        let opacity = self.opacity.value(now);
        let lift = self.lift.value(now);
        let shell = Rect::from_min_max(
            pos2(center_x - width / 2.0, body_top + lift),
            pos2(center_x + width / 2.0, body_bottom + lift),
        );
        self.glass = Some((shell, opacity));
        let outline = shapes::stadium(shell);
        let cancel_center = pos2(center_x - BUTTON_OFFSET, shell.center().y);
        let confirm_center = pos2(center_x + BUTTON_OFFSET, shell.center().y);

        // Pointer: the buttons while recording, the whole pill while processing.
        let (pointer, pressed, released, down) = ui.input(|input| {
            (
                input.pointer.hover_pos(),
                input.pointer.primary_pressed(),
                input.pointer.primary_released(),
                input.pointer.primary_down(),
            )
        });
        let hovered = pointer
            .filter(|&point| {
                frame.interactive
                    && !leaving
                    && shapes::rounded_rect_distance(point, shell, HEIGHT / 2.0) <= 0.0
            })
            .and_then(|point| {
                if recording {
                    if point.distance(cancel_center) <= BUTTON_RADIUS {
                        Some(Target::Cancel)
                    } else if point.distance(confirm_center) <= BUTTON_RADIUS {
                        Some(Target::Confirm)
                    } else {
                        None
                    }
                } else if processing {
                    Some(Target::Processing)
                } else {
                    None
                }
            });
        if pressed {
            self.pressed = hovered;
        }
        let mut action = CapsuleAction::None;
        if released {
            if let Some(target) = self.pressed.take().filter(|&target| Some(target) == hovered) {
                action = match target {
                    Target::Confirm => CapsuleAction::Confirm,
                    Target::Cancel | Target::Processing => CapsuleAction::Cancel,
                };
            }
        } else if !down {
            self.pressed = None;
        }
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        self.cancel_hover.set(
            now,
            (hovered == Some(Target::Cancel)) as u8 as f32,
            time(0.1),
            0.0,
            EASE,
        );
        self.confirm_hover.set(
            now,
            (hovered == Some(Target::Confirm)) as u8 as f32,
            time(0.1),
            0.0,
            EASE,
        );

        // Rail and badge sit above the pill and never move with its lift.
        let font_size = frame.transcript_font_size.clamp(12.0, 20.0);
        let rail_text = frame.transcript.trim();
        let rail_visible = frame.show_transcript
            && !leaving
            && matches!(phase, "recording" | "transcribing" | "polishing")
            && !rail_text.is_empty();
        let rail_bottom = body_top - STACK_GAP;
        if rail_visible {
            let rail = self.rail.get_or_insert_with(|| Rail::new(center_x));
            rail.update(
                &painter,
                rail_text,
                font_size,
                tokens.medium,
                tokens.ink,
                center_x,
                now,
                reduced,
            );
            draw_rail(&painter, stage, rail, rail_bottom, font_size, &tokens, now);
        } else {
            self.rail = None;
        }
        let badge_bottom = body_top
            - STACK_GAP
            - if rail_visible && state.translation_active {
                RAIL_HEIGHT + STACK_GAP
            } else {
                0.0
            };
        let badge_opacity = self.badge_opacity.value(now);
        if badge_opacity > 0.0 {
            let badge = badge(
                &painter,
                frame,
                &tokens,
                pos2(center_x, badge_bottom),
                badge_opacity,
                ui.ctx().pixels_per_point(),
            );
            let scale = self.badge_scale.value(now);
            let pivot = pos2(center_x, badge_bottom);
            let transform = TSTransform::new(
                pivot.to_vec2() * (1.0 - scale) + vec2(0.0, self.badge_lift.value(now)),
                scale,
            );
            for mut shape in badge {
                shape.transform(transform);
                painter.add(shape);
            }
        }

        // The pill keeps a translucent material, a lit rim and readable controls in both themes.
        let fade = |color: Color32| color.gamma_multiply(opacity);
        let pixels_per_point = ui.ctx().pixels_per_point();
        painter.add(
            Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: fade(tokens.pill.shadow),
            }
            .as_shape(shell, RADIUS),
        );
        painter.rect_filled(shell, RADIUS, fade(tokens.pill.fill));
        painter.add(shapes::inset_shadow(
            shell.shrink(1.0), HEIGHT / 2.0 - 1.0, pixels_per_point,
            1.0, 1.0, 0.0, fade(tokens.pill.highlight),
        ));
        if state.translation_active && layer_visible {
            painter.add(shapes::inset_shadow(
                shell.shrink(1.0),
                HEIGHT / 2.0 - 1.0,
                pixels_per_point,
                4.0,
                7.0,
                -3.0,
                fade(Color32::from_rgba_unmultiplied(37, 99, 235, 166)),
            ));
        }
        painter.rect_stroke(shell, RADIUS, Stroke::new(1.0, fade(tokens.pill.border)), StrokeKind::Inside);
        if let Some(since) = self.sweep_since.filter(|_| !reduced) {
            // `ol-capsule-sweep`: −100% → 0 over 85% of 2.4s, linear, then hold.
            let progress = (((now - since) % 2.4) / (2.4 * 0.85)).min(1.0) as f32;
            let sweep = shapes::stadium(shell.translate(vec2(-width * (1.0 - progress), 0.0)));
            painter.add(shapes::fill_clipped(&sweep, &outline, fade(tokens.pill.sweep)));
        }

        // Recording layer: waveform and the two buttons, clipped by the shell.
        let layer = self.layer.value(now) * opacity;
        if layer > 0.0 {
            let heights = self.waveform.heights(
                now,
                state.audio_level.unwrap_or(0.0),
                recording && !leaving,
                reduced,
            );
            for (index, height) in heights.into_iter().enumerate() {
                let x = center_x - 17.5 + 3.5 * index as f32;
                painter.rect_filled(
                    Rect::from_center_size(pos2(x, shell.center().y), vec2(1.5, height)),
                    1,
                    tokens.pill.ink.gamma_multiply(layer),
                );
            }
            let actions = layer * self.actions.value(now);
            if actions > 0.0 {
                let held = if down { self.pressed } else { None };
                for (target, center, fill, ink, icon, size, stroke) in [
                    (
                        Target::Cancel,
                        cancel_center,
                        lerp_color(tokens.pill.cancel, tokens.pill.cancel_hover, self.cancel_hover.value(now)),
                        tokens.pill.cancel_ink,
                        icons::IconName::Close,
                        15.0,
                        1.8,
                    ),
                    (
                        Target::Confirm,
                        confirm_center,
                        lerp_color(tokens.pill.confirm, tokens.pill.confirm_hover, self.confirm_hover.value(now)),
                        Color32::WHITE,
                        icons::IconName::Check,
                        17.0,
                        2.0,
                    ),
                ] {
                    // `:active { scale: 0.92 }`, around the button centre.
                    let scale = if held == Some(target) { 0.92 } else { 1.0 };
                    painter.add(shapes::fill_clipped(
                        &shapes::circle(center, BUTTON_RADIUS * scale),
                        &outline,
                        fill.gamma_multiply(actions),
                    ));
                    if let Some(texture) = icons::texture(ui.ctx(), icon, size, stroke) {
                        let rect = if scale == 1.0 {
                            snapped_square(center, size, pixels_per_point)
                        } else {
                            Rect::from_center_size(center, vec2(size, size) * scale)
                        };
                        painter.add(shapes::image_clipped(
                            texture.id(),
                            rect,
                            &outline,
                            ink.gamma_multiply(actions),
                        ));
                    }
                }
            }
        }

        // Status: one centred, ellipsized line inside 14px padding.
        let status = self.status.value(now) * opacity;
        if status > 0.0 {
            let galley = line(
                &painter,
                &label,
                label_format,
                width - 2.0 * STATUS_PADDING,
            );
            let pos = shell.center() - galley.size() * 0.5;
            painter.add(text(pos, galley, status));
        }

        let animating = [
            &self.width,
            &self.opacity,
            &self.lift,
            &self.layer,
            &self.actions,
            &self.status,
            &self.cancel_hover,
            &self.confirm_hover,
            &self.badge_opacity,
            &self.badge_lift,
            &self.badge_scale,
        ]
        .iter()
        .any(|tween| tween.active(now))
            || (!reduced && ((recording && !leaving) || self.sweep_since.is_some()))
            || self.rail.as_ref().is_some_and(|rail| rail.animating(now));
        if animating {
            ui.ctx().request_repaint();
        }
        action
    }
}

fn lerp_color(from: Color32, to: Color32, t: f32) -> Color32 {
    from.lerp_to_gamma(to, t.clamp(0.0, 1.0))
}

/// `.ol-classic-translation`: "Translate to" and the target language chip.
/// The chip is a label here: the native helper has no language menu.
fn badge(
    painter: &Painter,
    frame: &Frame,
    tokens: &Tokens,
    bottom_center: Pos2,
    opacity: f32,
    pixels_per_point: f32,
) -> Vec<Shape> {
    let fade = |color: Color32| color.gamma_multiply(opacity);
    let label = painter.layout_job(LayoutJob::single_section(
        frame.labels.translate_to.clone(),
        format(11.0, 400.0, 16.5, tokens.pill.ink),
    ));
    let target = frame
        .state
        .translation_target
        .as_deref()
        .filter(|target| !target.trim().is_empty())
        .unwrap_or("…");
    // SelectLite trigger: 0.5px border + 7px padding, 8px gap and an 11px chevron.
    let value_width = painter
        .layout_job(LayoutJob::single_section(
            target.to_owned(),
            format(11.0, 400.0, 16.5, tokens.control_ink),
        ))
        .size()
        .x;
    let chip_width = (value_width.ceil() + 34.0).clamp(92.0, 174.0);
    let value = line(
        painter,
        target,
        format(11.0, 400.0, 16.5, tokens.control_ink),
        chip_width - 34.0,
    );
    let width = 1.0 + 8.0 + label.size().x + 6.0 + chip_width + 4.0 + 1.0;
    let rect = Rect::from_min_max(
        pos2(bottom_center.x - width / 2.0, bottom_center.y - BADGE_HEIGHT),
        pos2(bottom_center.x + width / 2.0, bottom_center.y),
    );
    let center_y = rect.center().y;
    let label_pos = pos2(rect.left() + 9.0, center_y - label.size().y / 2.0);
    let chip = Rect::from_min_size(
        pos2(label_pos.x + label.size().x + 6.0, center_y - 11.0),
        vec2(chip_width, 22.0),
    );
    vec![
        Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: fade(Color32::from_black_alpha(26)),
        }
        .as_shape(rect, 6)
        .into(),
        egui::epaint::RectShape::filled(rect, 6, fade(tokens.pill.fill)).into(),
        shapes::inset_shadow(
            rect.shrink(1.0),
            5.0,
            pixels_per_point,
            -2.0,
            4.0,
            0.0,
            fade(Color32::from_rgba_unmultiplied(37, 99, 235, 31)),
        ),
        egui::epaint::RectShape::stroke(
            rect,
            6,
            Stroke::new(1.0, fade(tokens.pill.sweep)),
            StrokeKind::Inside,
        )
        .into(),
        text(label_pos, label, opacity),
        egui::epaint::RectShape::new(
            chip,
            4,
            fade(tokens.control),
            Stroke::new(0.5, fade(tokens.control_line)),
            StrokeKind::Inside,
        )
        .into(),
        text(
            pos2(chip.left() + 7.5, center_y - value.size().y / 2.0),
            value,
            opacity,
        ),
    ]
}

/// `--ol-capsule-pill-shadow` + `--ol-capsule-pill-inset` around a frost shell.
fn frost_shell(painter: &Painter, rect: Rect, stage: Rect, tokens: &Tokens, border: Color32) {
    let radius = rect.height() / 2.0;
    // 0 18px 50px -10px: σ = 25 around the box shrunk by 10 and lowered 18.
    painter.add(shapes::soft_shadow(
        rect.shrink(10.0).translate(vec2(0.0, 18.0)),
        25.0,
        tokens.shadow,
        stage,
    ));
    painter.rect_stroke(rect, radius, Stroke::new(0.5, tokens.ring), StrokeKind::Outside);
    painter.rect_filled(rect, radius, tokens.surface);
    painter.add(shapes::inset_shadow(
        rect.shrink(1.0),
        radius - 1.0,
        painter.ctx().pixels_per_point(),
        1.0,
        0.0,
        0.0,
        tokens.highlight,
    ));
    painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
}

fn draw_rail(
    painter: &Painter,
    stage: Rect,
    rail: &Rail,
    bottom: f32,
    font_size: f32,
    tokens: &Tokens,
    now: f64,
) {
    let width = rail.width.value;
    // The rendered frame stays inside the stage while the shell re-centres.
    let right = rail.right.value.min(stage.right()).max(stage.left() + width);
    let rect = Rect::from_min_max(pos2(right - width, bottom - RAIL_HEIGHT), pos2(right, bottom));
    frost_shell(painter, rect, stage, tokens, tokens.line);
    let track = rect.shrink2(vec2(RAIL_PAD, 0.0));
    let galley = painter.layout_job(rail.job(now, font_size, tokens.medium, tokens.ink));
    let size = galley.size();
    let clipped = painter.with_clip_rect(track.intersect(painter.clip_rect()));
    clipped.add(text(
        pos2(
            track.right() + rail.shift.value - size.x,
            rect.center().y - size.y / 2.0,
        ),
        galley,
        1.0,
    ));
    if size.x > track.width() {
        // `mask-image: linear-gradient(to right, transparent, #000 22px)`.
        let mut mesh = egui::epaint::Mesh::default();
        let fade = Rect::from_min_size(track.min, vec2(22.0, track.height()));
        mesh.colored_vertex(fade.left_top(), tokens.surface);
        mesh.colored_vertex(fade.right_top(), Color32::TRANSPARENT);
        mesh.colored_vertex(fade.right_bottom(), Color32::TRANSPARENT);
        mesh.colored_vertex(fade.left_bottom(), tokens.surface);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        clipped.add(Shape::mesh(mesh));
    }
}

/// `SelectionPolishNotice`: a non-interactive one-line status. Returns true
/// while its spinner turns.
fn notice(painter: &Painter, stage: Rect, frame: &Frame, tokens: &Tokens, now: f64) -> bool {
    let state = &frame.state;
    let labels = &frame.labels;
    let phase = state.phase.as_str();
    let processing = phase == "polishing";
    let (color, symbol, fallback) = match phase {
        "failed" => (Color32::from_rgb(0xff, 0x82, 0x78), Some(("!", 16.0)), &labels.polish_failed),
        "completed" => (Color32::from_rgb(0x63, 0xd5, 0x96), Some(("✓", 15.0)), &labels.replaced),
        "cancelled" => (tokens.ink, Some(("·", 16.0)), &labels.no_selection),
        _ => (
            Color32::from_rgb(0x8f, 0xc0, 0xff),
            None,
            if processing { &labels.polishing } else { &labels.polish_failed },
        ),
    };
    let message = state.text.trim();
    let label = if message.is_empty() { fallback.as_str() } else { message };
    let icon_width = if processing { 12.0 } else { 13.0 };
    let mut label_format = format(13.0, tokens.semibold, 16.25, color);
    label_format.extra_letter_spacing = 0.13;
    let galley = line(painter, label, label_format, 360.0 - 2.0 - 28.0 - icon_width - 8.0);
    let width = 2.0 + 28.0 + icon_width + 8.0 + galley.size().x;
    let height = 2.0 + 16.0 + 16.25;
    let bottom = stage.bottom() - BOTTOM_INSET;
    let rect = Rect::from_min_max(
        pos2(stage.center().x - width / 2.0, bottom - height),
        pos2(stage.center().x + width / 2.0, bottom),
    );
    let border = if phase == "failed" {
        Color32::from_rgba_unmultiplied(255, 112, 102, 107)
    } else {
        tokens.line
    };
    frost_shell(painter, rect, stage, tokens, border);
    let icon_center = pos2(rect.left() + 15.0 + icon_width / 2.0, rect.center().y);
    if processing {
        // 12px ring, 2px border, the top quarter coloured, turning every .75s.
        painter.circle_stroke(
            icon_center,
            5.0,
            Stroke::new(2.0, Color32::from_rgba_unmultiplied(143, 192, 255, 82)),
        );
        let turn = ((now % 0.75) / 0.75) as f32 * std::f32::consts::TAU;
        let points = (0..=12)
            .map(|step| {
                let angle = turn - 0.75 * std::f32::consts::PI
                    + step as f32 / 12.0 * 0.5 * std::f32::consts::PI;
                icon_center + 5.0 * vec2(angle.cos(), angle.sin())
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(2.0, color)));
    } else if let Some((symbol, size)) = symbol {
        let glyph = painter.layout_job(LayoutJob::single_section(
            symbol.to_owned(),
            format(size, 800.0, size, color),
        ));
        painter.add(text(icon_center - glyph.size() * 0.5, glyph, 1.0));
    }
    painter.add(text(
        pos2(rect.left() + 15.0 + icon_width + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        1.0,
    ));
    processing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapsuleLabels, CapsulePopupState, ThemeMode};

    #[test]
    fn both_pill_themes_keep_status_text_readable_over_light_and_dark_desktops() {
        let luminance = |rgb: [f32; 3]| {
            let linear = rgb.map(|c| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) });
            0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
        };
        for dark in [false, true] {
            let palette = pill_palette(dark);
            let fill = palette.fill.to_srgba_unmultiplied();
            assert_eq!(fill[0] < 128, dark, "the pill must follow the selected theme");
            for desktop in [0.0, 1.0] {
                let alpha = fill[3] as f32 / 255.0;
                let background = [0, 1, 2].map(|i| fill[i] as f32 / 255.0 * alpha + desktop * (1.0 - alpha));
                let ink = palette.ink.to_srgba_unmultiplied();
                let foreground = [0, 1, 2].map(|i| ink[i] as f32 / 255.0);
                let (a, b) = (luminance(background), luminance(foreground));
                assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
            }
        }
    }

    fn frame(phase: &str) -> Frame {
        Frame {
            session_id: "s".into(),
            sequence: 1,
            state: CapsulePopupState {
                phase: phase.into(),
                ..Default::default()
            },
            transcript: String::new(),
            show_transcript: false,
            transcript_font_size: 15.0,
            labels: CapsuleLabels::for_locale("zh-CN"),
            theme: ThemeMode::System,
            leaving: false,
            bounds: None,
            interactive: true,
        }
    }

    #[test]
    fn status_labels_follow_the_webview_rules() {
        let mut done = frame("completed");
        done.state.inserted_chars = Some(12);
        assert_eq!(status_label(&done), "已插入 12");
        done.state.text = "Pasted".into();
        assert_eq!(status_label(&done), "Pasted");

        let mut thinking = frame("polishing");
        assert_eq!(status_label(&thinking), "thinking");
        thinking.state.operating = true;
        assert_eq!(status_label(&thinking), "using");

        // While transcribing, the live recognition replaces the message.
        let mut transcribing = frame("transcribing");
        transcribing.state.text = "server".into();
        transcribing.transcript = "  你好  ".into();
        assert_eq!(status_label(&transcribing), "你好");

        let mut cancelled = frame("cancelled");
        cancelled.state.text = "ignored".into();
        assert_eq!(status_label(&cancelled), "已取消");
        assert_eq!(status_label(&frame("failed")), "出错了");
    }

    #[test]
    fn the_waveform_rests_until_the_voice_clears_the_noise_floor() {
        let mut waveform = Waveform::default();
        let quiet = waveform.heights(0.0, 0.008, true, false);
        assert!(quiet.iter().all(|height| (height - 1.5).abs() < 1e-4));
        let mut loud = quiet;
        for frame in 1..30 {
            loud = waveform.heights(frame as f64 / 60.0, 1.0, true, false);
        }
        // Full voice: the centre bar approaches 1.5 + 14.5 × ripple.
        assert!(loud[5] > 13.0 && loud[5] <= 16.0);
        assert!(loud[0] < loud[5] * 0.3);
        // Leaving (inactive) drops straight back to dots.
        let dots = waveform.heights(1.0, 1.0, false, false);
        assert!(dots.iter().all(|height| (height - 1.5).abs() < 1e-4));
    }
}
