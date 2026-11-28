use eframe::egui;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone)]
struct Glyph {
    text: String,
    width: f32,
    born: f64,
    from: f32,
    target: f32,
    moved: f64,
}

impl Glyph {
    fn x(&self, now: f64) -> f32 {
        self.from + (self.target - self.from) * spring(now - self.moved, 260.0, 30.0, 1.0)
    }
}

#[derive(Clone, Default)]
struct TranscriptMotion {
    glyphs: Vec<Glyph>,
    width_from: f32,
    width: f32,
    right_from: f32,
    right: f32,
    changed: f64,
}

fn spring(elapsed: f64, stiffness: f32, damping: f32, mass: f32) -> f32 {
    if elapsed <= 0.0 {
        return 0.0;
    }
    if elapsed >= 1.0 {
        return 1.0;
    }
    let t = elapsed as f32;
    let decay = damping / (2.0 * mass);
    let discriminant = stiffness / mass - decay * decay;
    let response = if discriminant.abs() < f32::EPSILON {
        1.0 + decay * t
    } else if discriminant > 0.0 {
        let frequency = discriminant.sqrt();
        (frequency * t).cos() + decay / frequency * (frequency * t).sin()
    } else {
        let frequency = (-discriminant).sqrt();
        (frequency * t).cosh() + decay / frequency * (frequency * t).sinh()
    };
    1.0 - (-decay * t).exp() * response
}

impl TranscriptMotion {
    fn width_at(&self, now: f64) -> f32 {
        self.width_from
            + (self.width - self.width_from) * spring(now - self.changed - 0.012, 330.0, 32.0, 0.9)
    }

    fn right_at(&self, now: f64) -> f32 {
        self.right_from
            + (self.right - self.right_from) * spring(now - self.changed - 0.152, 180.0, 27.0, 1.0)
    }

    fn update(&mut self, next: Vec<(String, f32)>, now: f64, max_width: f32) {
        if self.glyphs.len() == next.len()
            && self
                .glyphs
                .iter()
                .zip(&next)
                .all(|(old, (text, width))| old.text == *text && old.width == *width)
        {
            return;
        }
        let mut prefix = 0;
        while prefix < self.glyphs.len().min(next.len())
            && self.glyphs[prefix].text == next[prefix].0
        {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < self.glyphs.len() - prefix
            && suffix < next.len() - prefix
            && self.glyphs[self.glyphs.len() - 1 - suffix].text == next[next.len() - 1 - suffix].0
        {
            suffix += 1;
        }
        let total = next.iter().map(|(_, width)| width + 1.4).sum::<f32>() - 1.4;
        let width = (total + 36.0).clamp(72.0, max_width.max(72.0));
        let birth_advance = next[prefix..next.len() - suffix]
            .iter()
            .map(|(_, width)| width + 1.4)
            .sum::<f32>()
            .clamp(14.0, 28.0);
        let mut x = -total;
        let mut glyphs = Vec::with_capacity(next.len());
        for (index, (text, glyph_width)) in next.into_iter().enumerate() {
            let old = if index < prefix {
                self.glyphs.get(index)
            } else {
                None
            };
            glyphs.push(Glyph {
                text,
                width: glyph_width,
                born: old.map_or(now + index.saturating_sub(prefix) as f64 * 0.008, |old| {
                    old.born
                }),
                from: old.map_or(x + birth_advance, |old| old.x(now)),
                target: x,
                moved: now + (x.abs() as f64 / 2350.0).min(0.096) + 0.026,
            });
            x += glyph_width + 1.4;
        }
        // Keep suffix glyph identities and birth times across recognition corrections.
        if suffix > 0 {
            let next_len = glyphs.len();
            for offset in 0..suffix {
                let old = &self.glyphs[self.glyphs.len() - suffix + offset];
                let next = &mut glyphs[next_len - suffix + offset];
                next.born = old.born;
                next.from = old.x(now);
            }
        }
        if self.glyphs.is_empty() {
            self.width_from = width;
            self.right_from = width / 2.0;
        } else {
            self.width_from = self.width_at(now);
            self.right_from = self.right_at(now);
        }
        self.width = width;
        self.right = width / 2.0;
        self.changed = now;
        self.glyphs = glyphs;
    }
}

pub(super) struct RailStyle {
    pub stage: egui::Rect,
    pub bottom: f32,
    pub height: f32,
    pub font: egui::FontId,
    pub ink: egui::Color32,
    pub background: egui::Color32,
    pub border: egui::Color32,
}

pub(super) fn clear(ui: &egui::Ui) {
    ui.data_mut(|data| {
        data.remove_temp::<TranscriptMotion>(egui::Id::unique("capsule-transcript-motion"))
    });
}

pub(super) fn paint(ui: &egui::Ui, text: &str, style: RailStyle) {
    let id = egui::Id::unique("capsule-transcript-motion");
    let now = ui.input(|input| input.time);
    let mut motion = ui
        .data(|data| data.get_temp::<TranscriptMotion>(id))
        .unwrap_or_default();
    // Grapheme units match Intl.Segmenter: do not split combining marks or emoji.
    // Keep only the 64-unit visible tail and the three-unit overflow buffer.
    let mut units = text.graphemes(true).rev().take(67).collect::<Vec<_>>();
    units.reverse();
    let galleys = units
        .iter()
        .map(|unit| {
            ui.painter()
                .layout_no_wrap((*unit).to_owned(), style.font.clone(), style.ink)
        })
        .collect::<Vec<_>>();
    motion.update(
        units
            .iter()
            .zip(&galleys)
            .map(|(unit, galley)| ((*unit).to_owned(), galley.size().x))
            .collect(),
        now,
        style.stage.width().min(440.0),
    );
    let reduce_motion = ui.style().animation_time == 0.0;
    let width = if reduce_motion {
        motion.width
    } else {
        motion.width_at(now)
    };
    let right = style.stage.center().x
        + if reduce_motion {
            motion.right
        } else {
            motion.right_at(now)
        };
    let rail = egui::Rect::from_min_size(
        egui::pos2(right - width, style.bottom - style.height),
        egui::vec2(width, style.height),
    );
    ui.painter().rect_filled(
        rail,
        egui::CornerRadius::same((style.height / 2.0) as u8),
        style.background,
    );
    ui.painter().rect_stroke(
        rail,
        egui::CornerRadius::same((style.height / 2.0) as u8),
        egui::Stroke::new(1.0, style.border),
        egui::StrokeKind::Inside,
    );
    let clip = rail.shrink2(egui::vec2(18.0, 0.0));
    let painter = ui.painter().with_clip_rect(clip);
    for (glyph, galley) in motion.glyphs.iter().zip(galleys) {
        let age = now - glyph.born;
        let alpha = if reduce_motion {
            1.0
        } else {
            (age / 0.180).clamp(0.0, 1.0) as f32
        };
        let y = if reduce_motion {
            0.0
        } else {
            9.0 * (1.0 - spring(age, 460.0, 23.0, 0.8))
        };
        let x = if reduce_motion {
            glyph.target
        } else {
            glyph.x(now)
        };
        painter.galley_with_override_text_color(
            egui::pos2(
                clip.right() + x,
                rail.center().y - galley.size().y / 2.0 + y,
            ),
            galley,
            style.ink.gamma_multiply(alpha),
        );
    }
    // Only a genuinely clipped leading edge fades; short text stays opaque.
    let content_width = motion
        .glyphs
        .iter()
        .map(|glyph| glyph.width + 1.4)
        .sum::<f32>()
        - 1.4;
    if content_width > clip.width() {
        for offset in 0..18 {
            let fade = (1.0 - offset as f32 / 18.0).powi(2);
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(clip.left() + offset as f32, clip.top()),
                    egui::vec2(1.0, clip.height()),
                ),
                0,
                style.background.gamma_multiply(fade),
            );
        }
    }
    if !reduce_motion && now - motion.changed < 1.0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
    ui.data_mut(|data| data.insert_temp(id, motion));
}
