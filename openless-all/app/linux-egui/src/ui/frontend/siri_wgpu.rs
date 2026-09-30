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
    /// 整体不透明度（预乘 alpha）：wave→orb 交叉淡出用，`1.0` = 原样。
    pub opacity: f32,
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
            opacity: 1.0,
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
            opacity: 1.0,
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
            opacity: 1.0,
        }
    }

    pub fn with_tint(mut self, tint: [f32; 3]) -> Self {
        self.tint = tint;
        self
    }

    /// 预乘 alpha 的整体淡出（wave→orb 交叉淡出；Tauri `opacity .6s ease-out .55s`）。
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
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
    /// 预备态预期时长（ms），驱动光条的「预测式展开」（Tauri `warmupMs`）。
    pub warmup_ms: f32,
    /// 思考圆点的收尾：`true` 时六点干脆合回中央一颗圆（Tauri `merging`）。
    pub merging: bool,
}

impl Default for SiriDrive {
    fn default() -> Self {
        Self {
            level: 0.0,
            resolved: 1.0,
            speed: 1.0,
            warming: false,
            warmup_ms: DEFAULT_WARMUP_MS,
            merging: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SiriClock {
    pub time: f32,
    pub level: f32,
    pub resolved: f32,
    pub speed: f32,
    /// 预备态展开进度（0 = 收拢/加载中，1 = 展开到位）。对齐 Tauri `warmProgress`。
    pub warm_progress: f32,
    /// 思考圆点的「聚拢度」：1 = 六点全在圆心（出场那一拍），0 = 散开成环。
    pub gather: f32,
    /// 该时钟存活了多久（真实秒，不乘 speed）：驱动 `gather` 的 hold 计时。
    pub elapsed: f32,
}

/// 预备态光条的「收拢度」：wave 停在 0.2 —— 光条明显还没展开（「加载中」）。
/// 与 Tauri `SiriGL.tsx` 的 `WARMING_RESOLVED` 同值。
pub const WARMING_RESOLVED: f32 = 0.2;
/// 思考圆点出场时「全聚圆心」保持的时长，之后才缓缓散开成环（Tauri `GATHER_HOLD_S`）。
const GATHER_HOLD_SECONDS: f32 = 0.3;
/// 预备态预期时长的默认值（Tauri `WARMUP_MS_DEFAULT`）。
pub const DEFAULT_WARMUP_MS: f32 = 150.0;
/// 学习出来的预热时长上下限（Tauri `readWarmupMs` 的 clamp）。
pub const MIN_WARMUP_MS: f32 = 60.0;
pub const MAX_WARMUP_MS: f32 = 600.0;

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
            // 预备态一上场就从收拢态起步（未成形 = 加载中），避免「先展开一下又收拢」。
            resolved: if drive.warming {
                WARMING_RESOLVED
            } else {
                drive.resolved
            },
            speed: drive.speed,
            warm_progress: if drive.warming { 0.0 } else { 1.0 },
            // 圆点出场那一拍：六点全聚在圆心。
            gather: 1.0,
            elapsed: 0.0,
        })
    });
    clock.speed += (drive.speed - clock.speed) * (1.0 - (-dt * 2.5).exp());
    clock.time += dt * clock.speed;
    clock.elapsed += dt;

    // 预测式展开（Tauri 用户方案）：入场不再死等就绪信号，而是按历史平均加载时长
    // `warmup_ms` 从按下就平滑推进；未就绪 cap 在 0.9（留一截给就绪收尾），一旦就绪
    // 快速补到 1 —— 就绪早 = 剩得多，看上去「迅速展开完成」。
    let warmup_sec = (drive.warmup_ms.clamp(MIN_WARMUP_MS, MAX_WARMUP_MS) / 1000.0).max(0.06);
    if drive.warming {
        clock.warm_progress = (clock.warm_progress + dt / warmup_sec).min(0.9);
    } else {
        clock.warm_progress += (1.0 - clock.warm_progress) * (1.0 - (-dt * 9.0).exp());
    }

    // resolved：思考态（0）走 Tauri 原版的「从容汇聚」；入场/录音由 warmProgress
    // 从收拢态展开到满。
    let thinking = drive.resolved < 0.5;
    let resolved_target = if thinking {
        drive.resolved
    } else {
        WARMING_RESOLVED + (1.0 - WARMING_RESOLVED) * clock.warm_progress
    };
    let resolved_k = if thinking { 2.2 } else { 9.0 };
    clock.resolved += (resolved_target - clock.resolved) * (1.0 - (-dt * resolved_k).exp());

    let level_target = if drive.warming {
        0.12 + 0.06 * (clock.time * 3.0).sin()
    } else if thinking {
        0.14 + 0.07 * (clock.time * 2.2).sin()
    } else {
        visual_voice(drive.level)
    };
    let attack = if level_target > clock.level {
        14.0
    } else {
        5.0
    };
    clock.level += (level_target - clock.level) * (1.0 - (-dt * attack).exp());

    // 圆点环的生命周期（Tauri 用户拍板）：出场全聚圆心接住 wave 收缩成的光点 →
    // 稳住一拍后缓缓「从中心散开」成环转动 → `merging`（终态）时干脆地合回中央一颗圆。
    if drive.merging {
        clock.gather += (1.0 - clock.gather) * (1.0 - (-dt * 4.0).exp());
    } else if clock.elapsed > GATHER_HOLD_SECONDS {
        clock.gather += (0.0 - clock.gather) * (1.0 - (-dt * 1.6).exp());
    }

    ctx.data_mut(|data| data.insert_temp(key, clock));
    clock
}

/// 丢掉某个时钟，让入场动画（wave 展开 / 圆点聚拢）从头走一遍。
///
/// Tauri 里胶囊组件每次会话重新挂载，动画状态天然是新的；Linux 这边两个窗口进程都
/// 常驻，所以由调用方在新会话开始时显式复位。
pub fn reset(ctx: &egui::Context, id: &str) {
    let key = egui::Id::new(("openless-siri-clock", id));
    ctx.data_mut(|data| {
        data.remove::<SiriClock>(key);
    });
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
    let drift = (u.time % (20.0 * pi)) * 2.4;
    let aberr = (2.6 + mid * 0.8 + high * 0.5) * res;
    let soft = 0.01 * res * max(0.0, 2.5 + mid * 0.4);
    let unres = max(length(p) - mix(0.14, 0.14, res), 0.0);
    let y_main = a1 * envelope * res * sin(pw.x * 1.1 + drift);
    // Keep the original SiriGL spectral normalization and filled ribbons.
    // Summing the four hues without dividing by their weights biases yellow.
    let thickness = mix(0.1, 0.03, res);
    let intensity = mix(0.1, 0.01 * (2.0 + low * 1.5), res);
    let band_amount = 3.0 * intensity;
    var numerator = vec3<f32>(0.0);
    var denominator = vec3<f32>(0.0);
    for (var s: i32 = 0; s < 4; s = s + 1) {
        let hue = mix(vec3<f32>(1.0), spectral4(s), res);
        let ab = mix(-aberr, aberr, f32(s) / 3.0);
        let y_line = a2 * envelope * res * sin(pw.x + drift + ab);
        let distance = mix(unres, abs(p.y - y_line), res);
        let lorentz = mix(1.0 / (1.0 + pow(0.02 * distance, 2.0)), 1.0, res);
        let line = intensity / (sqrt(distance * distance + soft * soft) + thickness);
        let band_distance = max(0.0, max(p.y - max(y_main, y_line), min(y_main, y_line) - p.y));
        let band = band_amount / (band_distance + 0.08);
        numerator += hue * lorentz * (line + band);
        denominator += hue;
    }
    let main_distance = mix(unres, abs(p.y - y_main), res);
    let main_lorentz = mix(1.0 / (1.0 + pow(0.02 * main_distance, 2.0)), 1.0, res);
    let boost = (1.0 - res) * (3.0 * low + 1.2);
    var col = numerator / denominator;
    col += vec3<f32>(0.5 * intensity * (main_lorentz + boost) / (sqrt(main_distance * main_distance + soft * soft) + thickness));
    col = pow(max(col, vec3<f32>(0.0)), vec3<f32>(1.5));
    let edge_t = clamp((abs(y_screen) - 1.0) / -0.4, 0.0, 1.0);
    let edge = edge_t * edge_t * (3.0 - 2.0 * edge_t);
    let gaussian = exp(-pow(x_norm * 1.7, 2.0));
    col *= edge * gaussian * mix(0.55, 1.0, res);
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
    return fade(mode_color(local));
}

// 预乘 alpha 的整体淡出：`u.tint.a` 是 opacity（RGB 与 alpha 一起缩，
// 才能让 PREMULTIPLIED_ALPHA_BLENDING 下的淡出走对）。
fn fade(color: vec4<f32>) -> vec4<f32> {
    let opacity = clamp(u.tint.a, 0.0, 1.0);
    return vec4<f32>(color.rgb * opacity, color.a * opacity);
}

fn mode_color(local: vec2<f32>) -> vec4<f32> {
    if u.mode < 0.5 { return wave(local); }
    if u.mode < 1.5 { return orb(local); }
    return ring(local);
}

// Match egui's framebuffer handling: an sRGB target will encode RGB again,
// so decode our gamma-space Siri colors first. Alpha is always linear.
@fragment
fn fs_main_linear(in: VertexOut) -> @location(0) vec4<f32> {
    let local = in.uv * u.size;
    let color = mode_color(local);
    let lower = color.rgb / 12.92;
    let higher = pow((color.rgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    let linear = vec4<f32>(select(higher, lower, color.rgb < vec3<f32>(0.04045)), color.a);
    return fade(linear);
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
        uniforms.effect.opacity.clamp(0.0, 1.0),
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
            entry_point: Some(if format.is_srgb() {
                "fs_main_linear"
            } else {
                "fs_main"
            }),
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

    /// 预备态：光条从收拢态起步，按 `warmup_ms` 预测式展开，就绪后快速补满。
    #[test]
    fn the_wave_expands_on_the_learned_warmup_schedule() {
        let ctx = egui::Context::default();
        let predict = |elapsed_ms: f32| {
            let ctx = egui::Context::default();
            let mut clock = tick(
                &ctx,
                "warm",
                SiriDrive {
                    warming: true,
                    warmup_ms: 200.0,
                    ..Default::default()
                },
                0.0,
            );
            let frames = (elapsed_ms / 1000.0 / (1.0 / 60.0)) as usize;
            for _ in 0..frames {
                clock = tick(
                    &ctx,
                    "warm",
                    SiriDrive {
                        warming: true,
                        warmup_ms: 200.0,
                        ..Default::default()
                    },
                    1.0 / 60.0,
                );
            }
            clock.resolved
        };
        // 挂载那一帧：收拢（未成形），而不是直接展开。
        let mount = tick(
            &ctx,
            "mount",
            SiriDrive {
                warming: true,
                ..Default::default()
            },
            0.0,
        );
        assert!((mount.resolved - WARMING_RESOLVED).abs() < 1e-6);
        assert_eq!(mount.warm_progress, 0.0);
        // 200ms 的预期时长：越等越展开，但未就绪时封在 0.9 的 cap 之下。
        let early = predict(0.0);
        let halfway = predict(100.0);
        let capped = predict(400.0);
        assert!(
            early < halfway && halfway < capped,
            "{early} {halfway} {capped}"
        );
        assert!(
            capped < 1.0,
            "warming must stay short of fully open: {capped}"
        );
        // 就绪：快速补到满。
        let mut clock = tick(
            &ctx,
            "ready",
            SiriDrive {
                warming: true,
                warmup_ms: 200.0,
                ..Default::default()
            },
            0.0,
        );
        for _ in 0..60 {
            clock = tick(
                &ctx,
                "ready",
                SiriDrive {
                    warming: true,
                    warmup_ms: 200.0,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
        }
        for _ in 0..60 {
            clock = tick(&ctx, "ready", SiriDrive::default(), 1.0 / 60.0);
        }
        assert!(
            clock.resolved > 0.99,
            "a ready microphone must open the ribbon: {}",
            clock.resolved
        );
    }

    /// 思考圆点：出场全聚圆心 → 稳住 0.3s → 散开成环；终态 `merging` 时合回一颗圆。
    #[test]
    fn the_orb_gathers_then_spreads_and_merges_at_the_end() {
        let ctx = egui::Context::default();
        let orb = |merging: bool| SiriDrive {
            level: 0.0,
            resolved: 0.0,
            speed: 1.5,
            warming: false,
            warmup_ms: DEFAULT_WARMUP_MS,
            merging,
        };
        let mount = tick(&ctx, "orb", orb(false), 0.0);
        assert_eq!(mount.gather, 1.0, "the six dots start merged in the centre");
        let mut clock = mount;
        // 0.2s（还在 hold 里）：仍是全聚。
        for _ in 0..12 {
            clock = tick(&ctx, "orb", orb(false), 1.0 / 60.0);
        }
        assert!(clock.elapsed < GATHER_HOLD_SECONDS);
        assert!(clock.gather > 0.99, "hold: {}", clock.gather);
        // 又过了 1.5s：应该已经散开成环。
        for _ in 0..90 {
            clock = tick(&ctx, "orb", orb(false), 1.0 / 60.0);
        }
        assert!(clock.gather < 0.2, "spread into a ring: {}", clock.gather);
        // 终态：合回中央一颗圆。
        for _ in 0..60 {
            clock = tick(&ctx, "orb", orb(true), 1.0 / 60.0);
        }
        assert!(
            clock.gather > 0.9,
            "merging into one circle: {}",
            clock.gather
        );
    }

    /// 新会话复位：时钟丢掉之后，下一帧回到「刚出场」的状态。
    #[test]
    fn resetting_the_clock_replays_the_entry_animation() {
        let ctx = egui::Context::default();
        for _ in 0..30 {
            let _ = tick(&ctx, "session", SiriDrive::default(), 1.0 / 60.0);
        }
        reset(&ctx, "session");
        let fresh = tick(
            &ctx,
            "session",
            SiriDrive {
                warming: true,
                ..Default::default()
            },
            0.0,
        );
        assert_eq!(fresh.time, 0.0);
        assert_eq!(fresh.elapsed, 0.0);
        assert_eq!(fresh.warm_progress, 0.0);
        assert_eq!(fresh.gather, 1.0);
        assert!((fresh.resolved - WARMING_RESOLVED).abs() < 1e-6);
    }

    /// 整体不透明度（预乘 alpha）写进 tint 的第四个分量。
    #[test]
    fn opacity_lands_in_the_tint_alpha_channel() {
        let opaque = uniform_bytes(SiriUniforms {
            effect: SiriEffect::wave(1.0, 0.5),
            size: [64.0, 64.0],
        });
        let faded = uniform_bytes(SiriUniforms {
            effect: SiriEffect::wave(1.0, 0.5).with_opacity(0.25),
            size: [64.0, 64.0],
        });
        let read = |bytes: &[u8; 64]| f32::from_ne_bytes(bytes[44..48].try_into().unwrap());
        assert_eq!(read(&opaque), 1.0);
        assert_eq!(read(&faded), 0.25);
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
