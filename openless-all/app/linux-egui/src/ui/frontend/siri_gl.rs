//! Siri-inspired audio visuals shared by the Vulkan eframe windows and the
//! manually-rendered Wayland capsule. Animation state is time- and level-
//! driven; the visible wave/orb/ring is drawn with renderer-independent egui
//! primitives, so a missing GPU path can never replace it with generic bars.
//!
//! The GL callback (with its shader sources and program cache) used to live
//! here as a parity reference; the render path is wgpu now and never called it,
//! so it is gone. The sources are in the git history if a wgpu port needs them.

/// Which shader drives the glow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiriMode {
    /// Spectrum sound wave: recording. `level` is the live voice level and
    /// `resolved` 1 → 0 collapses the wave into the breathing point.
    Wave,
    /// Metaball fluid dots: thinking. `gather` 1 = all six merged in the middle
    /// (catches the wave's collapsed point), 0 = spread into the rotating ring.
    Orb,
    /// Rounded-rect perimeter sweep: the ring that hugs the ask composer and
    /// the recording capsule (red while recording, ink while thinking).
    Ring,
}

/// One frame of glow parameters. Everything is a scalar or a 3-vector: the
/// uniform upload is the only per-frame work besides the draw call.
#[derive(Clone, Copy, Debug)]
pub struct SiriGlow {
    pub mode: SiriMode,
    pub time: f32,
    pub level: f32,
    pub gather: f32,
    /// Ring only: corner radius and band thickness in physical pixels.
    pub radius: f32,
    pub thickness: f32,
    /// Per-call-site tint. `[1.0; 3]` keeps the spectral Siri colors (capsule);
    /// the ask/composer ring uses a flat accent (red while recording, ink while
    /// thinking) exactly like the previous CPU ring.
    pub tint: [f32; 3],
}

impl SiriGlow {
    pub fn wave(time: f32, level: f32) -> Self {
        Self {
            mode: SiriMode::Wave,
            time,
            level,
            gather: 0.0,
            radius: 0.0,
            thickness: 2.0,
            tint: [1.0; 3],
        }
    }

    pub fn orb(time: f32, gather: f32) -> Self {
        Self {
            mode: SiriMode::Orb,
            time,
            level: 0.0,
            gather,
            radius: 0.0,
            thickness: 2.0,
            tint: [1.0; 3],
        }
    }

    /// Perimeter ring for a `rect` of `radius` (px) with a `thickness` (px) band.
    pub fn ring(time: f32, radius: f32, thickness: f32) -> Self {
        Self {
            mode: SiriMode::Ring,
            time,
            level: 0.0,
            gather: 0.0,
            radius,
            thickness,
            tint: [1.0; 3],
        }
    }

    pub fn with_tint(mut self, tint: [f32; 3]) -> Self {
        self.tint = tint;
        self
    }
}

/// Voice level → visual amplitude (Tauri `SiriGL.tsx::visualVoice`): noise gate
/// at 0.012, ceiling 0.34, smoothstep, then a 0.42 power for the VU feel.
pub fn visual_voice(raw: f32) -> f32 {
    const GATE: f32 = 0.012;
    const CEILING: f32 = 0.34;
    let gated = ((raw - GATE) / (CEILING - GATE)).clamp(0.0, 1.0);
    let eased = gated * gated * (3.0 - 2.0 * gated);
    eased.max(0.0).powf(0.42)
}

/// What the caller wants to drive this frame.
#[derive(Clone, Copy, Debug)]
pub struct SiriDrive {
    /// Raw RMS from the audio pipeline (`capsule:state.audio_level`).
    pub level: f32,
    /// 1 = wave expanded (recording), 0 = collapsed into the thinking point.
    pub resolved: f32,
    pub speed: f32,
    /// Microphone not on yet: the wave breathes at a low, obviously-unready
    /// amplitude instead of following the level.
    pub warming: bool,
}

impl Default for SiriDrive {
    fn default() -> Self {
        Self {
            level: 0.0,
            resolved: 1.0,
            speed: 1.0,
            warming: false,
        }
    }
}

/// Smoothed animation state, kept in egui memory because egui only repaints
/// while something animates (`SiriGL.tsx` keeps the same values in refs).
#[derive(Clone, Copy, Debug)]
pub struct SiriClock {
    pub time: f32,
    pub level: f32,
    pub resolved: f32,
    pub speed: f32,
}

/// Advance one call site's clock by `dt` (seconds).
pub fn tick(ctx: &egui::Context, id: &str, drive: SiriDrive, dt: f32) -> SiriClock {
    let key = egui::Id::new(("openless-siri-clock", id));
    let dt = dt.clamp(0.0, 0.05);
    let mut clock = ctx.data_mut(|data| {
        data.get_temp::<SiriClock>(key).unwrap_or(SiriClock {
            time: 0.0,
            level: 0.0,
            resolved: drive.resolved,
            speed: drive.speed,
        })
    });
    // Speed is eased before it scales dt, so changing it mid-animation stays
    // continuous (Tauri: `smoothSpeed += (speed - smoothSpeed) * (1-e^{-dt*2.5})`).
    clock.speed += (drive.speed - clock.speed) * (1.0 - (-dt * 2.5).exp());
    clock.time += dt * clock.speed;
    let target = if drive.warming {
        0.12 + 0.06 * (clock.time * 3.0).sin()
    } else if drive.resolved < 0.5 {
        0.14 + 0.07 * (clock.time * 2.2).sin()
    } else {
        visual_voice(drive.level)
    };
    // Fast attack, slow release — VU-meter feel.
    let attack = if target > clock.level { 14.0 } else { 5.0 };
    clock.level += (target - clock.level) * (1.0 - (-dt * attack).exp());
    clock.resolved += (drive.resolved - clock.resolved) * (1.0 - (-dt * 3.0).exp());
    ctx.data_mut(|data| data.insert_temp(key, clock));
    clock
}

/// Queue the GPU glow for `rect`.
///
/// Returns `true` when the caller must *not* draw its CPU fallback: that is the
/// case only after the program has compiled and drawn successfully once, so the
/// first frames paint both (the callback is a no-op until it has a program, so
/// nothing is double-drawn) and a broken driver keeps the old look forever.
pub fn paint(ui: &egui::Ui, rect: egui::Rect, glow: SiriGlow) -> bool {
    if !rect.is_positive() || !rect.is_finite() {
        return false;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let color = |rgb: [f32; 3], alpha: f32| {
        egui::Color32::from_rgba_unmultiplied(
            (rgb[0].clamp(0.0, 1.0) * 255.0) as u8,
            (rgb[1].clamp(0.0, 1.0) * 255.0) as u8,
            (rgb[2].clamp(0.0, 1.0) * 255.0) as u8,
            (alpha.clamp(0.0, 1.0) * 255.0) as u8,
        )
    };
    match glow.mode {
        SiriMode::Wave => {
            let amplitude =
                (rect.height() * (0.10 + glow.level * 0.36)).clamp(2.0, rect.height() * 0.48);
            let center_y = rect.center().y;
            let hues = [
                [0.35, 0.74, 1.0],
                [0.45, 0.45, 1.0],
                [0.95, 0.48, 0.94],
                [1.0, 0.55, 0.72],
            ];
            for (index, hue) in hues.into_iter().enumerate() {
                let phase = index as f32 * 0.72;
                let points = (0..=48)
                    .map(|step| {
                        let t = step as f32 / 48.0;
                        let envelope = (std::f32::consts::PI * t).sin().powf(0.7);
                        let y = center_y
                            + (glow.time * 2.1 + t * 10.0 + phase).sin() * amplitude * envelope;
                        egui::pos2(rect.left() + rect.width() * t, y)
                    })
                    .collect::<Vec<_>>();
                // Stack a broad, low-alpha halo beneath the crisp filament.
                // A single opaque polyline reads as neon wire; Siri's reference
                // has a soft colored bloom around a bright, thin wave.
                painter.add(egui::Shape::line(
                    points.clone(),
                    egui::Stroke::new(7.0 + glow.level * 3.0, color(hue, 0.055)),
                ));
                painter.add(egui::Shape::line(
                    points.clone(),
                    egui::Stroke::new(3.8 + glow.level * 1.5, color(hue, 0.14)),
                ));
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(if index == 1 { 1.8 } else { 1.2 }, color(hue, 0.72)),
                ));
            }
        }
        SiriMode::Orb => {
            let gather = glow.gather.clamp(0.0, 1.0);
            for index in 0..7 {
                let angle = glow.time * 0.8 + index as f32 * std::f32::consts::TAU / 7.0;
                let radius = rect.width().min(rect.height()) * (0.06 + (1.0 - gather) * 0.24);
                let point =
                    rect.center() + egui::vec2(angle.cos() * radius, angle.sin() * radius * 0.42);
                let hue = [[0.35, 0.78, 1.0], [0.63, 0.52, 1.0], [1.0, 0.49, 0.82]][index % 3];
                let dot_radius = 2.2 + (0.5 + (glow.time * 2.0 + index as f32).sin() * 0.5) * 1.8;
                painter.circle_filled(point, dot_radius * 2.6, color(hue, 0.075));
                painter.circle_filled(point, dot_radius * 1.55, color(hue, 0.24));
                painter.circle_filled(point, dot_radius, color(hue, 0.92));
            }
        }
        SiriMode::Ring => {
            let color = color(glow.tint, 0.9);
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(glow.radius.round().clamp(0.0, 255.0) as u8),
                egui::Stroke::new(glow.thickness.max(1.0), color),
                egui::StrokeKind::Inside,
            );
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 着色器路径已从渲染路径上摘除（见本文件第 7 行的 `allow(dead_code)`），
    /// `paint` 现在直接用 CPU 形状画出每一种光效，并恒返回 `true`（调用方不得
    /// 再画自己的回退）。等 wgpu 光效落地后，这里要恢复成「GPU 回调 + 所有权」
    /// 的断言。
    #[test]
    fn paint_draws_every_mode_on_the_cpu_and_claims_ownership() {
        let ctx = egui::Context::default();
        let mut modes = 0;
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 240.0),
                )),
                ..Default::default()
            },
            |ui| {
                for glow in [
                    SiriGlow::wave(0.5, 0.3),
                    SiriGlow::orb(0.5, 1.0),
                    SiriGlow::ring(0.5, 12.0, 2.0),
                ] {
                    assert!(
                        paint(ui, ui.max_rect(), glow),
                        "{:?} must paint its CPU fallback and claim the centre",
                        glow.mode
                    );
                    modes += 1;
                }
            },
        );
        assert_eq!(modes, 3, "every mode must be exercised");
        assert!(
            !output.shapes.is_empty(),
            "the glow must still reach the frame as CPU shapes"
        );
        let callbacks = output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
            .count();
        assert_eq!(
            callbacks, 0,
            "the shader path is off: nothing may queue a GPU callback"
        );
    }

    #[test]
    fn clock_smooths_level_time_and_speed() {
        let ctx = egui::Context::default();
        let start = tick(&ctx, "test", SiriDrive::default(), 1.0 / 60.0);
        assert!(
            (start.time - 1.0 / 60.0).abs() < 1e-5,
            "one frame of dt*1.0 speed: {}",
            start.time
        );
        assert_eq!(start.level, 0.0, "level starts from the stored clock");
        let mut clock = start;
        for _ in 0..30 {
            clock = tick(
                &ctx,
                "test",
                SiriDrive {
                    level: 0.5,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
        }
        assert!(
            clock.time > 0.4 && clock.time < 0.6,
            "0.5s of frames: {}",
            clock.time
        );
        assert!(clock.level > 0.0, "level follows the drive");
        assert!(clock.level <= visual_voice(0.5) + f32::EPSILON);
        // A speed change is eased into the accumulated time (no jump).
        let before = clock.time;
        let after = tick(
            &ctx,
            "test",
            SiriDrive {
                speed: 3.0,
                ..Default::default()
            },
            1.0 / 60.0,
        );
        assert!(
            after.time - before < 0.06,
            "speed eased, not applied at once"
        );
    }

    #[test]
    fn visual_voice_gates_and_eases() {
        assert_eq!(visual_voice(0.0), 0.0);
        assert_eq!(visual_voice(0.012), 0.0, "noise gate");
        assert_eq!(visual_voice(0.34), 1.0, "ceiling maps to a full bar");
        let mid = visual_voice(0.18);
        assert!(
            mid > 0.4 && mid < 1.0,
            "curve stays inside the unit range: {mid}"
        );
    }
}
