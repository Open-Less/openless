//! Vulkan/WGPU Siri visuals.
//!
//! Siri is rendered as an egui WGPU paint callback. There are deliberately no
//! legacy graphics paths: the normal eframe renderer and the native
//! Wayland layer-shell renderer both use the same WGSL pipeline.

use std::borrow::Cow;
use std::sync::OnceLock;

use eframe::egui;
use eframe::egui_wgpu;

/// Which WGSL effect drives the capsule centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiriMode {
    /// Spectral voice wave while recording.
    Wave,
    /// Six fluid points while thinking.
    Orb,
    /// Rounded-rectangle perimeter used by the composer ring.
    Ring,
}

#[derive(Clone, Copy, Debug)]
pub struct SiriEffect {
    pub mode: SiriMode,
    pub time: f32,
    pub level: f32,
    pub resolved: f32,
    pub gather: f32,
    pub radius: f32,
    pub thickness: f32,
    pub tint: [f32; 3],
}

impl SiriEffect {
    pub fn wave(time: f32, level: f32) -> Self {
        Self {
            mode: SiriMode::Wave,
            time,
            level,
            resolved: 1.0,
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
            resolved: 0.0,
            gather,
            radius: 0.0,
            thickness: 2.0,
            tint: [1.0; 3],
        }
    }

    pub fn ring(time: f32, radius: f32, thickness: f32) -> Self {
        Self {
            mode: SiriMode::Ring,
            time,
            level: 0.0,
            resolved: 1.0,
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

/// Audio drive for the persistent egui animation clock.
#[derive(Clone, Copy, Debug)]
pub struct SiriDrive {
    pub level: f32,
    pub resolved: f32,
    pub speed: f32,
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

#[derive(Clone, Copy, Debug)]
pub struct SiriClock {
    pub time: f32,
    pub level: f32,
    pub resolved: f32,
    pub speed: f32,
}

pub fn visual_voice(raw: f32) -> f32 {
    const GATE: f32 = 0.012;
    const CEILING: f32 = 0.34;
    let gated = ((raw - GATE) / (CEILING - GATE)).clamp(0.0, 1.0);
    let eased = gated * gated * (3.0 - 2.0 * gated);
    eased.max(0.0).powf(0.42)
}

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
    clock.speed += (drive.speed - clock.speed) * (1.0 - (-dt * 2.5).exp());
    clock.time += dt * clock.speed;
    let target = if drive.warming {
        0.12 + 0.06 * (clock.time * 3.0).sin()
    } else if drive.resolved < 0.5 {
        0.14 + 0.07 * (clock.time * 2.2).sin()
    } else {
        visual_voice(drive.level)
    };
    let attack = if target > clock.level { 14.0 } else { 5.0 };
    clock.level += (target - clock.level) * (1.0 - (-dt * attack).exp());
    clock.resolved += (drive.resolved - clock.resolved) * (1.0 - (-dt * 3.0).exp());
    ctx.data_mut(|data| data.insert_temp(key, clock));
    clock
}

const SIRI_WGSL: &str = r#"
struct Uniforms {
    time: f32,
    level: f32,
    resolved: f32,
    gather: f32,
    mode: f32,
    radius: f32,
    thickness: f32,
    _pad: f32,
    tint: vec4<f32>,
    size: vec2<f32>,
    _pad2: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    var points = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0)
    );
    var out: VertexOut;
    let point = points[index];
    out.position = vec4<f32>(point, 0.0, 1.0);
    // Interpolated local coordinates, independent of the callback's viewport
    // origin, DPI, clipping and offscreen backdrop scale.
    out.uv = vec2<f32>((point.x + 1.0) * 0.5, (1.0 - point.y) * 0.5);
    return out;
}

fn spectral4(index: i32) -> vec3<f32> {
    let x = f32(index);
    return clamp(vec3<f32>(abs(x - 3.0) - 1.0, 2.0 - abs(x - 2.0), 2.0 - abs(x - 4.0)), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hue2rgb(h0: f32) -> vec3<f32> {
    let h = h0 - floor(h0);
    return vec3<f32>(
        clamp(abs(h * 6.0 - 3.0) - 1.0, 0.0, 1.0),
        clamp(2.0 - abs(h * 6.0 - 2.0), 0.0, 1.0),
        clamp(2.0 - abs(h * 6.0 - 4.0), 0.0, 1.0)
    );
}

fn wave(frag: vec2<f32>) -> vec4<f32> {
    let pi = 3.14159265359;
    let aspect = u.size.x / max(u.size.y, 1.0);
    var p = (frag / u.size) * 2.0 - vec2<f32>(1.0);
    let y_screen = p.y;
    p.y = -p.y;
    p.x *= aspect;
    p /= 0.6;
    let res = clamp(u.resolved, 0.0, 1.0);
    let level = clamp(u.level, 0.0, 1.0);
    let low = clamp(0.45 + 0.45 * sin(u.time * 0.8) * sin(u.time * 0.37 + 1.0), 0.0, 1.0) * level;
    let mid = clamp(0.40 + 0.40 * sin(u.time * 1.7 + 2.0) * sin(u.time * 0.53), 0.0, 1.0) * level;
    let high = clamp(0.30 + 0.30 * sin(u.time * 2.9 + 4.0) * sin(u.time * 0.71 + 2.0), 0.0, 1.0) * level;
    var pw = p;
    pw.x *= mix(5.0, 1.0, res);
    let x_norm = pw.x / max(aspect, 1.0);
    let envelope = pow(max(cos(pi * 0.5 * min(abs(0.9 * x_norm), 1.0)), 0.0), 2.0);
    let a1 = 0.32 * mix(0.14, 1.0, level) + 0.01 * low * 6.0;
    let a2 = a1 + mid * 0.05 + high * 0.06;
    let drift = u.time * 2.4;
    let aberr = (2.6 + mid * 0.8 + high * 0.5) * res;
    let soft = 0.01 * res * max(0.0, 2.5 + mid * 0.4);
    let unres = max(length(p) - mix(0.14, 0.14, res), 0.0);
    let y_main = a1 * envelope * res * sin(pw.x * 1.1 + drift);
    var col = vec3<f32>(0.0);
    for (var s: i32 = 0; s < 4; s = s + 1) {
        let hue = mix(vec3<f32>(1.0), spectral4(s), res);
        let ab = mix(-aberr, aberr, f32(s) / 3.0);
        let y_line = a2 * envelope * res * sin(pw.x + drift + ab);
        let distance = mix(unres, abs(p.y - y_line), res);
        let line = (0.01 * (2.0 + low * 1.5)) / (sqrt(distance * distance + soft * soft) + mix(0.1, 0.03, res));
        col += hue * line;
    }
    let main_distance = mix(unres, abs(p.y - y_main), res);
    let halo = 0.5 * (0.01 * (2.0 + low * 1.5)) / (sqrt(main_distance * main_distance + soft * soft) + 0.03);
    col += vec3<f32>(halo * (1.0 + (1.0 - res) * (3.0 * low + 1.2)));
    let edge_t = clamp((abs(y_screen) - 1.0 + 0.4) / -0.4, 0.0, 1.0);
    let edge = edge_t * edge_t * (3.0 - 2.0 * edge_t);
    let gaussian = exp(-pow(x_norm * 1.7, 2.0));
    col *= edge * gaussian * mix(0.55, 1.0, res);
    col = pow(max(col, vec3<f32>(0.0)), vec3<f32>(1.5));
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    let alpha = clamp(max(col.r, max(col.g, col.b)), 0.0, 1.0);
    return vec4<f32>(col, alpha);
}

fn orb(frag: vec2<f32>) -> vec4<f32> {
    let aspect = u.size.x / max(u.size.y, 1.0);
    var p = (frag / u.size) * 2.0 - vec2<f32>(1.0);
    p.y = -p.y;
    p.x *= aspect;
    let gather = clamp(u.gather, 0.0, 1.0);
    let radius = mix(0.17, 0.008, gather);
    var col = vec3<f32>(0.0);
    for (var i: i32 = 0; i < 6; i = i + 1) {
        let fi = f32(i);
        let angle = fi / 6.0 * 6.28318530718 + u.time * 0.35;
        let pos = vec2<f32>(cos(angle), sin(angle)) * radius;
        let dot_radius = mix(0.036, 0.030, gather) + 0.006 * sin(u.time * 1.3 + fi);
        let distance = max(length(p - pos) - dot_radius, 0.0);
        let bloom = exp(-pow(distance * 18.0, 1.35));
        let hue = hue2rgb(fi / 6.0 + u.time * 0.06);
        col += bloom * mix(vec3<f32>(1.0), hue, 0.22) * (1.0 + 0.30 * gather);
    }
    col += exp(-length(p) * 15.0) * gather * vec3<f32>(0.7, 0.8, 1.0);
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    let alpha = clamp(max(col.r, max(col.g, col.b)), 0.0, 1.0);
    return vec4<f32>(col, alpha);
}

fn round_box(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn ring(frag: vec2<f32>) -> vec4<f32> {
    let p = frag - u.size * 0.5;
    let radius = clamp(u.radius, 0.0, min(u.size.x, u.size.y) * 0.5);
    let distance = round_box(p, u.size * 0.5 - vec2<f32>(radius), radius);
    let edge = 1.0 - smoothstep(0.0, max(u.thickness, 1.0), abs(distance));
    let alpha = edge * 0.9;
    return vec4<f32>(u.tint.rgb * alpha, alpha);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let local = in.uv * u.size;
    if u.mode < 0.5 { return wave(local); }
    if u.mode < 1.5 { return orb(local); }
    return ring(local);
}
"#;

#[derive(Clone, Copy)]
struct SiriUniforms {
    effect: SiriEffect,
    size: [f32; 2],
}

struct SiriGpu {
    pipeline: egui_wgpu::wgpu::RenderPipeline,
    bind_layout: egui_wgpu::wgpu::BindGroupLayout,
}

fn uniform_bytes(uniforms: SiriUniforms) -> [u8; 64] {
    let mode = match uniforms.effect.mode {
        SiriMode::Wave => 0.0,
        SiriMode::Orb => 1.0,
        SiriMode::Ring => 2.0,
    };
    let values = [
        uniforms.effect.time,
        uniforms.effect.level,
        uniforms.effect.resolved,
        uniforms.effect.gather,
        mode,
        uniforms.effect.radius,
        uniforms.effect.thickness,
        0.0,
        uniforms.effect.tint[0],
        uniforms.effect.tint[1],
        uniforms.effect.tint[2],
        1.0,
        uniforms.size[0],
        uniforms.size[1],
    ];
    // WGSL's uniform layout rounds the trailing `vec2` padding to 16-byte
    // alignment: 14 values (56 bytes) are followed by `_pad2: vec2<f32>`.
    // wgpu validates the complete binding size when this callback is drawn.
    let mut bytes = [0_u8; 64];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
    }
    bytes
}

fn create_gpu(
    device: &egui_wgpu::wgpu::Device,
    format: egui_wgpu::wgpu::TextureFormat,
    msaa_samples: u32,
) -> SiriGpu {
    use egui_wgpu::wgpu;
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("openless-siri-bind-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(64),
            },
            count: None,
        }],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("openless-siri-wgsl"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SIRI_WGSL)),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("openless-siri-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("openless-siri-wgpu"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: msaa_samples.max(1),
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    SiriGpu {
        pipeline,
        bind_layout,
    }
}

/// Install the Siri pipeline into an eframe renderer before the first frame.
pub fn install_wgpu(state: &eframe::egui_wgpu::RenderState, msaa_samples: u32) {
    let mut renderer = state.renderer.write();
    install_renderer(
        &mut renderer,
        &state.device,
        state.target_format,
        msaa_samples,
    );
}

/// Same installation hook for the standalone Wayland layer-shell renderer.
pub fn install_renderer(
    renderer: &mut egui_wgpu::Renderer,
    device: &egui_wgpu::wgpu::Device,
    format: egui_wgpu::wgpu::TextureFormat,
    msaa_samples: u32,
) {
    if renderer.callback_resources.get::<SiriGpu>().is_none() {
        renderer
            .callback_resources
            .insert(create_gpu(device, format, msaa_samples));
    }
}

struct SiriCallback {
    uniforms: SiriUniforms,
    // egui prepares every callback before painting any of them. A shared
    // queue-written uniform would make every draw use the last effect's data.
    binding: OnceLock<egui_wgpu::wgpu::BindGroup>,
}

impl egui_wgpu::CallbackTrait for SiriCallback {
    fn prepare(
        &self,
        device: &egui_wgpu::wgpu::Device,
        _queue: &egui_wgpu::wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut egui_wgpu::wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<egui_wgpu::wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get::<SiriGpu>() {
            self.binding.get_or_init(|| {
                use egui_wgpu::wgpu::{self, util::DeviceExt as _};
                let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("openless-siri-uniform"),
                    contents: &uniform_bytes(self.uniforms),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("openless-siri-bind-group"),
                    layout: &gpu.bind_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                })
            });
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut egui_wgpu::wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(gpu) = resources.get::<SiriGpu>() else {
            return;
        };
        let Some(binding) = self.binding.get() else {
            return;
        };
        render_pass.set_pipeline(&gpu.pipeline);
        render_pass.set_bind_group(0, binding, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

/// Queue one Vulkan/WGPU paint callback. Returning `true` means the caller must
/// not draw an egui fallback over it.
pub fn paint(ui: &egui::Ui, rect: egui::Rect, effect: SiriEffect) -> bool {
    if !rect.is_positive() || !rect.is_finite() {
        return false;
    }
    let scale = ui.ctx().pixels_per_point();
    let callback = egui_wgpu::Callback::new_paint_callback(
        rect,
        SiriCallback {
            binding: OnceLock::new(),
            uniforms: SiriUniforms {
                effect,
                size: [rect.width() * scale, rect.height() * scale],
            },
        },
    );
    ui.painter().add(egui::Shape::Callback(callback));
    true
}

#[cfg(test)]
#[path = "siri_wgpu_gpu_tests.rs"]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_queues_wgpu_callbacks_for_every_mode() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 240.0),
                )),
                ..Default::default()
            },
            |ui| {
                for effect in [
                    SiriEffect::wave(0.5, 0.3),
                    SiriEffect::orb(0.5, 1.0),
                    SiriEffect::ring(0.5, 12.0, 2.0),
                ] {
                    assert!(paint(ui, ui.max_rect(), effect));
                }
            },
        );
        let callbacks = output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
            .count();
        assert_eq!(callbacks, 3);
        output.textures_delta.clear();
    }

    #[test]
    fn clock_smooths_level_time_and_speed() {
        let ctx = egui::Context::default();
        let start = tick(&ctx, "test", SiriDrive::default(), 1.0 / 60.0);
        assert!((start.time - 1.0 / 60.0).abs() < 1e-5);
        assert_eq!(start.level, 0.0);
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
        assert!(clock.time > 0.4 && clock.time < 0.6);
        assert!(clock.level > 0.0);
        assert!(clock.level <= visual_voice(0.5) + f32::EPSILON);
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
        assert!(after.time - before < 0.06);
    }

    #[test]
    fn uniform_buffer_matches_wgsl_alignment() {
        let bytes = uniform_bytes(SiriUniforms {
            effect: SiriEffect::wave(1.35, 0.52),
            size: [320.0, 80.0],
        });
        assert_eq!(bytes.len(), 64);
    }

    #[test]
    fn visual_voice_gates_and_eases() {
        assert_eq!(visual_voice(0.0), 0.0);
        assert_eq!(visual_voice(0.012), 0.0);
        assert_eq!(visual_voice(0.34), 1.0);
        let mid = visual_voice(0.18);
        assert!(mid > 0.4 && mid < 1.0);
    }
}
