//! Live blurred backdrop for in-window modals (the settings sheet).
//!
//! macOS gets this for free: the Tauri window sits on a system vibrancy layer, so
//! a translucent mask reads as frosted glass. Linux has no such layer, and
//! `ViewportCommand::Screenshot` is not a substitute — the captured frame
//! contains the mask itself, so every frame would blur an already-blurred image
//! (the backdrop dissolves into a flat smear within a second) and it could never
//! follow the page. Reading back the framebuffer every frame is also a GPU stall.
//!
//! So the page is drawn a *second* time, offscreen, from the same tessellated
//! shapes, and blurred on the GPU:
//!
//! ```text
//! page shapes ──▶ egui renderer ──▶ MSAA resolve ──▶ blur H ──▶ blur V ──▶ egui image
//!   (1/4 scale)                                              (native texture)
//! ```
//!
//! All of it happens inside `update`, before eframe paints the frame, so the
//! texture the overlay samples already holds the current page: no capture delay,
//! no stale frame, nothing to refresh by hand.

use std::borrow::Cow;
use std::sync::Arc;

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};

/// The offscreen page render is downscaled by this factor. Blurring a quarter
/// resolution copy is both cheaper than a full-resolution convolution and softer,
/// which is what a frosted panel wants.
const DOWNSCALE: f32 = 4.0;

/// Where the overlay finds the blur it should sample. The value is the
/// `(texture, uv)` pair published by [`publish`].
const IMAGE_KEY: &str = "openless-backdrop-image";

/// A separable Gaussian at the downscaled resolution: sigma ≈ 2 texels ≈ 8px on
/// screen, the same ballpark as the desktop `backdrop-filter: blur(6px)`.
/// Five taps with linear filtering equal a nine-tap convolution.
const BLUR_SHADER: &str = r#"
struct BlurParams {
    // xy = source texel size, zw = blur direction.
    texel_and_direction: vec4<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // Full-screen triangle: cheaper than a quad, and it cannot leave a seam.
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    let corner = corners[index];
    var out: VertexOutput;
    out.position = vec4<f32>(corner, 0.0, 1.0);
    // Clip space is y-up; egui textures are y-down.
    out.uv = vec2<f32>((corner.x + 1.0) * 0.5, (1.0 - corner.y) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let step = params.texel_and_direction.xy * params.texel_and_direction.zw;
    var color = textureSample(source, source_sampler, in.uv) * 0.2270270270;
    color += textureSample(source, source_sampler, in.uv + step * 1.3846153846) * 0.3162162162;
    color += textureSample(source, source_sampler, in.uv - step * 1.3846153846) * 0.3162162162;
    color += textureSample(source, source_sampler, in.uv + step * 3.2307692308) * 0.0702702703;
    color += textureSample(source, source_sampler, in.uv - step * 3.2307692308) * 0.0702702703;
    return color;
}
"#;

/// The texture the overlay samples. It covers the window's content rect; use
/// [`uv_for`] to crop any rect inside the window out of it.
pub type BackdropImage = egui::TextureId;

/// Publish (or clear) this frame's blurred backdrop for the overlay.
pub fn publish(ctx: &egui::Context, image: Option<BackdropImage>) {
    ctx.data_mut(|data| {
        if let Some(image) = image {
            data.insert_temp(egui::Id::new(IMAGE_KEY), image);
        } else {
            data.remove::<BackdropImage>(egui::Id::new(IMAGE_KEY));
        }
    });
}

/// The blurred backdrop the overlay should sample, if this frame has one.
pub fn published(ctx: &egui::Context) -> Option<BackdropImage> {
    ctx.data(|data| data.get_temp::<BackdropImage>(egui::Id::new(IMAGE_KEY)))
}

/// Texture coordinates for `rect` (window points) inside the published backdrop.
pub fn uv_for(ctx: &egui::Context, rect: egui::Rect) -> egui::Rect {
    let window = ctx.content_rect();
    let size = egui::vec2(window.width().max(1.0), window.height().max(1.0));
    egui::Rect::from_min_max(
        ((rect.min - window.min) / size).to_pos2(),
        ((rect.max - window.min) / size).to_pos2(),
    )
}

/// `vec4<f32>` as bytes, without dragging in `bytemuck` for one uniform.
fn f32x4_bytes(values: [f32; 4]) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    for (index, value) in values.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
    }
    bytes
}

/// Offscreen render targets, recreated whenever the window size changes.
struct Targets {
    /// Offscreen page size in pixels (1/4 of the content rect).
    size: [u32; 2],
    /// Multisampled colour target the page is drawn into; `None` without MSAA.
    msaa_view: Option<wgpu::TextureView>,
    /// The resolved (1 sample) page: the source of the horizontal blur pass.
    page_view: wgpu::TextureView,
    /// Intermediate target of the horizontal blur pass.
    scratch_view: wgpu::TextureView,
    /// Final blurred image, sampled by the overlay.
    blurred_view: wgpu::TextureView,
    #[cfg(test)]
    blurred_texture: wgpu::Texture,
    horizontal: wgpu::BindGroup,
    vertical: wgpu::BindGroup,
    texture_id: egui::TextureId,
}

impl Targets {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut egui_wgpu::Renderer,
        format: wgpu::TextureFormat,
        msaa_samples: u32,
        bind_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: [u32; 2],
    ) -> Self {
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let target = |label: &str| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[format],
            })
        };
        let msaa_view = (msaa_samples > 1).then(|| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("openless-backdrop-msaa"),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: msaa_samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[format],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        });
        let page_view = target("openless-backdrop-page").create_view(&Default::default());
        let scratch_view =
            target("openless-backdrop-blur-scratch").create_view(&Default::default());
        let blurred_texture = target("openless-backdrop-blurred");
        let blurred_view = blurred_texture.create_view(&Default::default());
        let texel = [1.0 / size[0] as f32, 1.0 / size[1] as f32];
        let uniform = |direction: [f32; 2]| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("openless-backdrop-blur-params"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(
                &buffer,
                0,
                &f32x4_bytes([texel[0], texel[1], direction[0], direction[1]]),
            );
            buffer
        };
        let bind = |texture: &wgpu::TextureView, buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("openless-backdrop-blur"),
                layout: bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(texture),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let horizontal = bind(&page_view, &uniform([1.0, 0.0]));
        let vertical = bind(&scratch_view, &uniform([0.0, 1.0]));
        let texture_id =
            renderer.register_native_texture(device, &blurred_view, wgpu::FilterMode::Linear);
        Self {
            size,
            msaa_view,
            page_view,
            scratch_view,
            blurred_view,
            #[cfg(test)]
            blurred_texture,
            horizontal,
            vertical,
            texture_id,
        }
    }
}

/// GPU state for the blurred backdrop. Lives on the UI thread; every method is a
/// no-op until the first frame that needs a blur.
pub struct BackdropBlur {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    format: wgpu::TextureFormat,
    msaa_samples: u32,
    sampler: wgpu::Sampler,
    bind_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    targets: Option<Targets>,
}

impl BackdropBlur {
    /// Build from the running eframe render state. `msaa_samples` must match the
    /// renderer's MSAA setting (`NativeOptions::multisampling`), because the
    /// offscreen pass reuses eframe's pipelines.
    pub fn new(state: &egui_wgpu::RenderState, msaa_samples: u32) -> Self {
        Self::new_with(
            state.device.clone(),
            state.queue.clone(),
            state.target_format,
            msaa_samples,
            Arc::clone(&state.renderer),
        )
    }

    fn new_with(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        msaa_samples: u32,
        renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("openless-backdrop-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("openless-backdrop-blur-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("openless-backdrop-blur"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(BLUR_SHADER)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("openless-backdrop-blur-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("openless-backdrop-blur"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            device,
            queue,
            renderer,
            format,
            msaa_samples,
            sampler,
            bind_layout,
            pipeline,
            targets: None,
        }
    }

    /// Resize the offscreen targets and return the texture the overlay should
    /// sample this frame. `None` when there is nothing to blur (modal closed).
    pub fn prepare(&mut self, ctx: &egui::Context, open: bool) -> Option<egui::TextureId> {
        if !open {
            self.release();
            return None;
        }
        let content = ctx.content_rect();
        if content.width() < 1.0 || content.height() < 1.0 {
            return None;
        }
        let scale = ctx.pixels_per_point() / DOWNSCALE;
        let size = [
            ((content.width() * scale).round() as u32).max(1),
            ((content.height() * scale).round() as u32).max(1),
        ];
        self.ensure(size);
        self.targets.as_ref().map(|targets| targets.texture_id)
    }

    /// Draw the page (everything under the modal) offscreen and blur it. Runs
    /// before eframe paints, so the overlay samples this frame's pixels.
    pub fn render_page(&mut self, ctx: &egui::Context) {
        if self.targets.is_none() {
            return;
        }
        let primitives = page_primitives(ctx);
        if primitives.is_empty() {
            return;
        }
        self.blur_primitives(ctx, &primitives);
    }

    fn blur_primitives(&mut self, ctx: &egui::Context, primitives: &[egui::ClippedPrimitive]) {
        let targets = self.targets.as_ref().expect("checked by render_page");
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: targets.size,
            pixels_per_point: ctx.pixels_per_point() / DOWNSCALE,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("openless-backdrop"),
            });
        let callbacks = {
            let mut renderer = self.renderer.write();
            let callbacks = renderer.update_buffers(
                &self.device,
                &self.queue,
                &mut encoder,
                primitives,
                &screen,
            );
            {
                let (view, resolve) = match &targets.msaa_view {
                    Some(msaa) => (msaa, Some(&targets.page_view)),
                    None => (&targets.page_view, None),
                };
                let mut pass = encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("openless-backdrop-page"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: resolve,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Discard,
                            },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    })
                    .forget_lifetime();
                renderer.render(&mut pass, primitives, &screen);
            }
            callbacks
        };
        self.blur_pass(&mut encoder, &targets.scratch_view, &targets.horizontal);
        self.blur_pass(&mut encoder, &targets.blurred_view, &targets.vertical);
        if !callbacks.is_empty() {
            self.queue.submit(callbacks);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    fn blur_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("openless-backdrop-blur"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn ensure(&mut self, size: [u32; 2]) {
        if self
            .targets
            .as_ref()
            .is_some_and(|targets| targets.size == size)
        {
            return;
        }
        let mut renderer = self.renderer.write();
        if let Some(previous) = self.targets.take() {
            renderer.free_texture(&previous.texture_id);
        }
        self.targets = Some(Targets::new(
            &self.device,
            &self.queue,
            &mut renderer,
            self.format,
            self.msaa_samples,
            &self.bind_layout,
            &self.sampler,
            size,
        ));
    }

    /// Drop the GPU targets (modal closed). The pipeline and layouts are cheap
    /// and stay around, like eframe's own renderer.
    pub fn release(&mut self) {
        let Some(targets) = self.targets.take() else {
            return;
        };
        self.renderer.write().free_texture(&targets.texture_id);
    }
}

/// Every layer painted below the modal: the window surface, titlebar, sidebar and
/// the page itself. `Order::Foreground` is where the modal lives, so it is
/// excluded — the backdrop can never contain the mask it sits under.
fn backdrop_layers(ctx: &egui::Context) -> Vec<egui::LayerId> {
    // The window surface is painted through a bare `layer_painter`, so it is not
    // registered as an area and `layer_ids` cannot see it.
    let mut layers = vec![egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("openless-window-background"),
    )];
    ctx.memory(|memory| {
        for layer in memory.layer_ids() {
            if layer.order < egui::Order::Foreground && !layers.contains(&layer) {
                layers.push(layer);
            }
        }
    });
    layers
}

/// Tessellate the page for the offscreen pass. The shapes come from this frame's
/// paint lists, so the backdrop follows scrolling, resizing and page switches.
fn page_primitives(ctx: &egui::Context) -> Vec<egui::ClippedPrimitive> {
    let layers = backdrop_layers(ctx);
    let shapes = ctx.graphics(|graphics| {
        let mut shapes = Vec::new();
        for layer in &layers {
            if let Some(list) = graphics.get(*layer) {
                shapes.extend(list.all_entries().cloned());
            }
        }
        shapes
    });
    if shapes.is_empty() {
        return Vec::new();
    }
    ctx.tessellate(shapes, ctx.pixels_per_point())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无头测试里没有渲染器消费 `TexturesDelta`，直接 drop 会 panic。
    fn end_pass(ctx: &egui::Context) {
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
    }

    /// A GPU is not guaranteed (CI has none), so every device test bows out when
    /// no adapter answers instead of failing the suite.
    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        let runtime = tokio::runtime::Builder::new_current_thread().build().ok()?;
        let adapter = runtime
            .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
        runtime
            .block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .ok()
    }

    #[test]
    fn backdrop_uv_crops_the_content_rect_out_of_the_full_window_texture() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        });
        let uv = uv_for(
            &ctx,
            egui::Rect::from_min_max(egui::pos2(0.0, 40.0), egui::pos2(800.0, 600.0)),
        );
        assert!((uv.min.x - 0.0).abs() < 1e-4, "{uv:?}");
        assert!((uv.min.y - 40.0 / 600.0).abs() < 1e-4, "{uv:?}");
        assert!((uv.max.x - 1.0).abs() < 1e-4, "{uv:?}");
        assert!((uv.max.y - 1.0).abs() < 1e-4, "{uv:?}");
        end_pass(&ctx);
    }

    #[test]
    fn publishing_and_clearing_the_backdrop_round_trips() {
        let ctx = egui::Context::default();
        assert!(published(&ctx).is_none());
        publish(&ctx, Some(egui::TextureId::User(7)));
        assert_eq!(published(&ctx), Some(egui::TextureId::User(7)));
        publish(&ctx, None);
        assert!(published(&ctx).is_none());
    }

    #[test]
    fn the_backdrop_only_collects_layers_below_the_modal() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        });
        for (order, id) in [
            (egui::Order::Middle, "page"),
            (egui::Order::Foreground, "openless-settings-modal"),
            (egui::Order::Tooltip, "tooltip"),
        ] {
            egui::Area::new(egui::Id::new(id))
                .order(order)
                .fixed_pos(egui::pos2(10.0, 10.0))
                .show(&ctx, |ui| {
                    ui.label("layer");
                });
        }
        let layers = backdrop_layers(&ctx);
        assert!(layers.contains(&egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("page")
        )));
        assert!(!layers
            .iter()
            .any(|layer| layer.id == egui::Id::new("openless-settings-modal")));
        assert!(!layers
            .iter()
            .any(|layer| layer.id == egui::Id::new("tooltip")));
        end_pass(&ctx);
    }

    /// End-to-end GPU check: the page pass, the MSAA resolve and both blur passes
    /// must produce a smeared copy of the page, the right way up. Skipped without
    /// an adapter, so CI without a GPU still passes.
    #[test]
    fn the_offscreen_page_is_blurred_without_flipping_it() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no wgpu adapter available; skipping the GPU blur test");
            return;
        };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let msaa_samples = 4;
        let renderer = Arc::new(egui::mutex::RwLock::new(egui_wgpu::Renderer::new(
            &device,
            format,
            egui_wgpu::RendererOptions {
                msaa_samples,
                ..Default::default()
            },
        )));
        let mut blur = BackdropBlur::new_with(
            device.clone(),
            queue.clone(),
            format,
            msaa_samples,
            Arc::clone(&renderer),
        );

        // 256x256 points at 1 px/point -> a 64x64 backdrop (`DOWNSCALE` = 4).
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(256.0, 256.0),
            )),
            ..Default::default()
        };
        for frame in 0..2 {
            ctx.begin_pass(raw.clone());
            // 黑色整页 + 右上象限一块白：翻转或转置过的背板不可能把白色留在同一处。
            // 用裸 `layer_painter` 而不是 `Area`：Area 的 `fade_in` 在无头测试里会
            // 把首帧画成 `Shape::Noop`，那样这里就什么都测不到了。
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("openless-window-background"),
            ));
            painter.rect_filled(
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(256.0, 256.0)),
                egui::CornerRadius::ZERO,
                egui::Color32::BLACK,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(128.0, 0.0), egui::pos2(256.0, 128.0)),
                egui::CornerRadius::ZERO,
                egui::Color32::WHITE,
            );
            if frame == 1 {
                assert!(blur.prepare(&ctx, true).is_some());
                assert_eq!(
                    blur.targets.as_ref().map(|targets| targets.size),
                    Some([64, 64])
                );
                blur.render_page(&ctx);
            }
            let mut output = ctx.end_pass();
            let mut guard = renderer.write();
            for (id, deltas) in &output.textures_delta.set {
                for delta in deltas {
                    guard.update_texture(&device, &queue, *id, delta);
                }
            }
            for id in &output.textures_delta.free {
                guard.free_texture(id);
            }
            drop(guard);
            output.textures_delta.clear();
        }

        let pixels = read_pixels(&device, &queue, &blur);
        let at = |x: usize, y: usize| {
            let index = (y * 64 + x) * 4;
            pixels[index]
        };
        // Inside the square: still white, so nothing moved or flipped.
        assert!(
            at(56, 8) > 200,
            "top-right should stay white: {}",
            at(56, 8)
        );
        // The opposite corner: still black.
        assert!(
            at(8, 56) < 32,
            "bottom-left should stay black: {}",
            at(8, 56)
        );
        // Straddling the square's left edge: smeared, not a hard step.
        assert!(
            (32..=224).contains(&at(32, 8)),
            "the edge should be blurred: {}",
            at(32, 8)
        );
    }

    fn read_pixels(device: &wgpu::Device, queue: &wgpu::Queue, blur: &BackdropBlur) -> Vec<u8> {
        let targets = blur.targets.as_ref().expect("blur targets");
        let [width, height] = targets.size;
        let unpadded = width * 4;
        let padded = unpadded.div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("openless-backdrop-readback"),
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("openless-backdrop-readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.blurred_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let mapped = slice.get_mapped_range().expect("readback buffer mapped");
        let mut pixels = Vec::with_capacity((unpadded * height) as usize);
        for row in 0..height {
            let start = (row * padded) as usize;
            pixels.extend_from_slice(&mapped[start..start + unpadded as usize]);
        }
        drop(mapped);
        buffer.unmap();
        pixels
    }
}
