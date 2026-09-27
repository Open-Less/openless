//! Real Vulkan draw/readback regression: non-origin viewports, per-callback
//! uniforms, MSAA, DPI, clipping and transparent pixels (not just shape counts).
use super::*;
use egui_wgpu::wgpu;

const WIDTH: u32 = 512;
const HEIGHT: u32 = 256;
const EFFECT_SIZE: [f32; 2] = [128.0, 96.0];

fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    samples: u32,
    scale: f32,
    effects: &[(SiriEffect, [f32; 2])],
    clipped: bool,
) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = egui_wgpu::Renderer::new(
        device,
        format,
        egui_wgpu::RendererOptions {
            msaa_samples: samples,
            ..Default::default()
        },
    );
    install_renderer(&mut renderer, device, format, samples);
    let primitives: Vec<_> = effects
        .iter()
        .map(|(effect, origin)| {
            let rect = egui::Rect::from_min_size(
                egui::pos2(origin[0] / scale, origin[1] / scale),
                egui::vec2(EFFECT_SIZE[0] / scale, EFFECT_SIZE[1] / scale),
            );
            let clip_rect = if clipped {
                rect.shrink(16.0 / scale)
            } else {
                rect
            };
            egui::ClippedPrimitive {
                clip_rect,
                primitive: egui::epaint::Primitive::Callback(
                    egui_wgpu::Callback::new_paint_callback(
                        rect,
                        SiriCallback {
                            uniforms: SiriUniforms {
                                effect: *effect,
                                size: EFFECT_SIZE,
                            },
                            binding: OnceLock::new(),
                        },
                    ),
                ),
            }
        })
        .collect();
    let texture = |sample_count| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("siri-regression-target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: if sample_count == 1 {
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            },
            view_formats: &[],
        })
    };
    let target = texture(1);
    let view = target.create_view(&Default::default());
    let msaa = (samples > 1).then(|| texture(samples).create_view(&Default::default()));
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [WIDTH, HEIGHT],
        pixels_per_point: scale,
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    let callbacks = renderer.update_buffers(device, queue, &mut encoder, &primitives, &screen);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("siri-regression-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa.as_ref().unwrap_or(&view),
                    resolve_target: msaa.as_ref().map(|_| &view),
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
            })
            .forget_lifetime();
        renderer.render(&mut pass, &primitives, &screen);
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("siri-regression-readback"),
        size: (WIDTH * HEIGHT * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(
        callbacks
            .into_iter()
            .chain(std::iter::once(encoder.finish())),
    );
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("map Siri pixels")
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU draw completes");
    let pixels = slice.get_mapped_range().expect("mapped pixels").to_vec();
    buffer.unmap();
    pixels
}

#[test]
fn vulkan_effects_are_visible_local_independent_and_transparent() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let adapter = match runtime.block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(error) => {
            assert!(
                std::env::var_os("OPENLESS_REQUIRE_GPU_TESTS").is_none(),
                "Vulkan required: {error}"
            );
            eprintln!("Skipping Siri Vulkan readback: {error}");
            return;
        }
    };
    eprintln!("Siri GPU regression adapter: {:?}", adapter.get_info());
    let (device, queue) = runtime
        .block_on(adapter.request_device(&Default::default()))
        .unwrap();
    let effects = [
        (SiriEffect::wave(1.35, 0.52), [32.0, 128.0]),
        (SiriEffect::orb(0.7, 0.5), [192.0, 128.0]),
        (
            SiriEffect::ring(0.5, 12.0, 2.0).with_tint([0.2, 0.6, 1.0]),
            [352.0, 128.0],
        ),
    ];
    for samples in [1, 4] {
        for scale in [1.0, 1.5, 2.0] {
            let combined = draw(&device, &queue, samples, scale, &effects, false);
            let clipped = draw(&device, &queue, samples, scale, &effects, true);
            for (effect, origin) in effects {
                let reference = draw(
                    &device,
                    &queue,
                    samples,
                    scale,
                    &[(effect, [0.0, 0.0])],
                    false,
                );
                let mut visible = 0;
                for y in 0..96 {
                    for x in 0..128 {
                        let from = ((y * WIDTH + x) * 4) as usize;
                        let to =
                            (((y + origin[1] as u32) * WIDTH + x + origin[0] as u32) * 4) as usize;
                        let pixel = &combined[to..to + 4];
                        for channel in 0..4 {
                            assert!(pixel[channel].abs_diff(reference[from + channel]) <= 2,
                                "{:?}: viewport/shared-uniform mismatch at {x},{y}, MSAA={samples}, DPI={scale}", effect.mode);
                            let expected = if (16..112).contains(&x) && (16..80).contains(&y) {
                                pixel[channel]
                            } else {
                                0
                            };
                            assert!(
                                clipped[to + channel].abs_diff(expected) <= 2,
                                "clipping moved effect"
                            );
                        }
                        assert!(
                            pixel[..3].iter().all(|c| *c <= pixel[3]),
                            "RGB must be premultiplied"
                        );
                        if pixel[..3].iter().any(|c| *c > 16) && pixel[3] > 16 {
                            visible += 1;
                        }
                    }
                }
                assert!(
                    visible > 50,
                    "{:?} must draw visible pixels, got {visible}",
                    effect.mode
                );
                assert_eq!(reference[3], 0, "effect corner must remain transparent");
            }
            assert!(
                combined[..(WIDTH * 100 * 4) as usize]
                    .iter()
                    .all(|v| *v == 0),
                "outside callbacks must remain transparent"
            );
        }
    }
}
