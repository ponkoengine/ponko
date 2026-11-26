//! Offscreen validation and benchmark: uses the example scene, real passes,
//! real timestamp queries, no event loop or native window.
use super::gpu::GpuTexture;
use super::gpu3d::*;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{Material, MaterialDesc, RenderStats};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkyLight};
mod scene {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/common/lights_scene.rs"));
}

#[test]
#[ignore = "requires a GPU; run explicitly in release for timings"]
fn offscreen_light_benchmark() {
    // A test takes one of the places GPU tests share (render_test_support).
    #[cfg(all(test, feature = "window", feature = "3d"))]
    crate::render::render_test_support::take_gpu_place();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("GPU adapter");
    println!("adapter: {:?}", adapter.get_info());
    let features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        required_limits: wgpu::Limits::default(),
        ..Default::default()
    }))
    .unwrap();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
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
        ],
    });
    let white = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    queue.write_texture(
        white.as_image_copy(),
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        white.size(),
    );
    let sampler = device.create_sampler(&Default::default());
    let view = white.create_view(&Default::default());
    let textures = [GpuTexture {
        texture: white.clone(),
        nearest: false,
        mips: None,
        bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }),
    }];
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1280,
            height: 720,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = output.create_view(&Default::default());
    let mut brightness = Vec::new();
    for count in [0, 100, 1000] {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(1280.0, 720.0));
        scene::setup(&mut ctx, count);
        let mut renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
        let desc = GpuMaterialDesc::new(&crate::render::MaterialDesc {
            roughness: 0.5,
            ..Default::default()
        });
        renderer.create_material(&device, &desc, &textures);
        for data in scene::geometry() {
            renderer.create_mesh(
                &device,
                &data,
                &[(0..data.indices.len() as u32, crate::render::Material { id: 0 })],
                false,
            );
        }
        let mut timer =
            super::timer::GpuTimer::new(&device, &queue).expect("GPU timestamps required for this benchmark");
        let mut frame = Frame3D::default();
        let mut cpu = 0.0;
        let mut gpu = 0.0;
        let warmup = 60;
        let measured = 180;
        for n in 0..warmup + measured {
            timer.collect(&device);
            timer.begin_frame();
            let start = std::time::Instant::now();
            frame.collect(
                &mut ctx.world,
                &ctx.gfx.assets3d,
                &crate::three::SkinFrame::default(),
                &ctx.scene3d.camera,
                &ctx.scene3d.lighting,
            );
            let scene = Scene3D {
                frame: &frame,
                lighting: &ctx.scene3d.lighting,
                sky: &ctx.scene3d.sky,
                light: SkyLight {
                    sun: Vec3::ZERO,
                    sky: Vec3::splat(0.25),
                    ground: Vec3::splat(0.1),
                },
                camera: ctx.scene3d.camera.position,
                post: &ctx.scene3d.post,
                time: (0.0, 1.0 / 60.0),
                occlusion: false,
            };
            renderer.prepare(
                &device,
                &queue,
                &Prepare3D {
                    scene: &scene,
                    lut: None,
                    size: (1280, 720),
                },
            );
            let mut buffers = renderer.draw(&device, &target, wgpu::Color::BLACK, &textures, Some(&timer));
            let mut encoder = device.create_command_encoder(&Default::default());
            timer.resolve(&mut encoder);
            buffers.push(encoder.finish());
            queue.submit(buffers);
            timer.submitted();
            ctx.perf.last_render_stats = RenderStats {
                cpu_ms: start.elapsed().as_secs_f32() * 1000.0,
                gpu_ms: timer.smoothed_ms,
                draw_calls: renderer.draw_calls(),
                ..RenderStats::default()
            };
            // Completion is outside CPU timing, as display/GPU waits are in normal stats.
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            if n >= warmup {
                let stats = ctx.perf.render_stats();
                cpu += stats.cpu_ms;
                gpu += stats.gpu_ms.unwrap();
            }
        }
        // Validate actual pixels as well as pipeline creation: colored lights
        // must brighten the same scene, not merely populate CPU lists.
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 1280 * 720 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1280 * 4),
                    rows_per_image: None,
                },
            },
            output.size(),
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let pixels = readback.get_mapped_range(..).unwrap();
        let sum: u64 = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[..3].iter().map(|&v| v as u64).sum::<u64>())
            .sum();
        brightness.push(sum);
        drop(pixels);
        readback.unmap();
        assert!(
            frame.lights.lights.len() > count / 2 || count == 0,
            "most lights must be in view"
        );
        println!(
            "lights={count}, visible={}, frame_cpu_ms={:.4}, frame_gpu_ms={:.4}, draws={}",
            frame.lights.lights.len(),
            cpu / measured as f32,
            gpu / measured as f32,
            ctx.perf.render_stats().draw_calls
        );
    }
    assert!(
        brightness[1] > brightness[0] * 2,
        "100 lights must illuminate pixels: {brightness:?}"
    );
    assert!(
        brightness[2] > brightness[1],
        "more lights must brighten this scene: {brightness:?}"
    );
}
