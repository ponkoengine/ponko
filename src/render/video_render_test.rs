//! Offscreen GPU test of video frames (feature `video`): a frame decoded by
//! `Video` goes into an sRGB RGBA8 texture the way the renderer creates and
//! updates sprite textures (`Renderer::create_texture`, `update_texture`),
//! is sampled texel for texel into a target and read back. The read-back
//! frame matches the decoded one (orientation, channel order, color space)
//! and is the updated frame, not the first one (the negative control).
//!
//! The renderer converts a frame's Y'CbCr planes to RGBA on the GPU
//! (`video_yuv.rs`): through the production renderer, the drawn frame
//! matches the CPU conversion `frame_rgba` gives within a step of
//! rounding, and so does every chroma layout and matrix at an odd size,
//! while swapped chroma planes (the negative control) do not.
use super::render_test_support::{Image, TestGpu, Tolerance, diff_within, gpu, read_texture, target};
use crate::prelude::*;
use crate::video::Video;

const CLIP: &[u8] = include_bytes!("../../tests/fixtures/video/clip.webm");

/// An sRGB texture sampled into an sRGB target texel for texel round-trips
/// exactly; one step of rounding is allowed for adapters that filter in
/// lower precision.
const SAME: Tolerance = Tolerance {
    channel: 1,
    outliers: 0.0,
    edge: 0,
};

/// The clip's frames 0 and 13, as `Video` decodes them (headless).
fn frames() -> (Vec<u8>, Vec<u8>) {
    let mut out = None;
    Game::new("video frames", 64, 64).run_headless(0, |ctx| {
        let mut video = Video::from_bytes(ctx, CLIP.to_vec()).expect("the test clip loads");
        let first = video.frame_rgba().to_vec();
        video.seek(ctx, 1.3).expect("frame 13 decodes");
        out = Some((first, video.frame_rgba().to_vec()));
    });
    out.expect("the setup ran")
}

struct Rig {
    gpu: TestGpu,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

impl Rig {
    fn new() -> Option<Self> {
        let gpu = gpu()?;
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video test"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
                @group(0) @binding(0) var image: texture_2d<f32>;
                @group(0) @binding(1) var nearest: sampler;
                struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
                @vertex fn vs(@builtin(vertex_index) i: u32) -> Out {
                    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
                    return Out(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
                }
                @fragment fn fs(in: Out) -> @location(0) vec4<f32> {
                    return textureSampleLevel(image, nearest, in.uv, 0.0);
                }
                "#
                .into(),
            ),
        });
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8UnormSrgb.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        Some(Self {
            gpu,
            layout,
            pipeline,
            sampler,
        })
    }

    /// A sprite texture of `rgba`, as the renderer creates one.
    fn create(&self, rgba: &[u8]) -> wgpu::Texture {
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("video frame"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.update(&texture, rgba);
        texture
    }

    /// Replaces every pixel, as `Renderer::update_texture` does.
    fn update(&self, texture: &wgpu::Texture, rgba: &[u8]) {
        self.gpu.queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * 64),
                rows_per_image: Some(64),
            },
            texture.size(),
        );
    }

    /// Samples `texture` texel for texel into an sRGB target and reads it back.
    fn draw(&self, texture: &wgpu::Texture) -> Image {
        let device = &self.gpu.device;
        let output = target(device, (64, 64), wgpu::TextureFormat::Rgba8UnormSrgb);
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let target_view = output.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit([encoder.finish()]);
        Image {
            width: 64,
            height: 64,
            rgba: read_texture(device, &self.gpu.queue, &output),
        }
    }
}

fn image(rgba: Vec<u8>) -> Image {
    Image {
        width: 64,
        height: 64,
        rgba,
    }
}

#[test]
fn an_updated_video_frame_samples_on_the_gpu_as_decoded() {
    let Some(rig) = Rig::new() else { return };
    let (first, thirteenth) = frames();
    let texture = rig.create(&first);
    rig.update(&texture, &thirteenth);
    let drawn = rig.draw(&texture);

    let d = diff_within(&drawn, &image(thirteenth.clone()), SAME);
    eprintln!("video frame on the GPU: {d:?}");
    assert!(d.within(SAME), "the drawn frame differs from the decoded one: {d:?}");
    // The quarters are where the file has them: red top left, blue bottom
    // left, frame 13's gray (20 + 130) bottom right.
    let [r, g, b, _] = drawn.pixel(16, 16);
    assert!(r > 240 && g < 15 && b < 15, "top left is red: {:?}", [r, g, b]);
    let [r, g, b, _] = drawn.pixel(16, 48);
    assert!(b > 240 && r < 15 && g < 15, "bottom left is blue: {:?}", [r, g, b]);
    let gray = drawn.pixel(48, 48)[0];
    assert!(gray.abs_diff(150) <= 6, "bottom right is frame 13's gray: {gray}");

    // Negative control: the first frame (still in a texture never updated)
    // is beyond the tolerance from the drawn one.
    let stale = rig.draw(&rig.create(&first));
    let d = diff_within(&stale, &image(thirteenth), SAME);
    assert!(!d.within(SAME), "the first frame should not pass for frame 13: {d:?}");
}

/// The GPU converter's pixels against the CPU converter's: a step of
/// rounding apart (float math against 16.16 fixed point), never more.
const CONVERTED: Tolerance = Tolerance {
    channel: 2,
    outliers: 0.0,
    edge: 0,
};

/// Draws `video`'s texture 1:1 over a 64 x 64 offscreen frame and reads it.
fn screenshot(ctx: &mut Ctx, video: &Video, name: &str) -> Image {
    let path = super::render_test_support::output_dir().join(format!("video-{name}-{}.png", std::process::id()));
    ctx.gfx.draw_ex(
        video.texture(),
        vec2(32.0, 32.0),
        DrawParams::default().with_space(DrawSpace::Screen),
    );
    ctx.gfx.save_screenshot(&path);
    assert!(ctx.render_frame().drew());
    let shot = image::open(&path).expect("the screenshot was written").to_rgba8();
    let _ = std::fs::remove_file(&path);
    image(shot.into_raw())
}

#[test]
fn a_video_frame_converted_on_the_gpu_matches_the_cpu_conversion() {
    let Some(mut ctx) = super::render_test_support::offscreen_ctx((64, 64)) else {
        return;
    };
    let mut video = Video::from_bytes(&mut ctx, CLIP.to_vec()).expect("the test clip loads");
    let first = screenshot(&mut ctx, &video, "first");
    video.seek(&mut ctx, 1.3).expect("frame 13 decodes");
    let drawn = screenshot(&mut ctx, &video, "thirteenth");
    // Made on the CPU only now, from the planes the GPU converted.
    let cpu = image(video.frame_rgba().to_vec());

    let d = diff_within(&drawn, &cpu, CONVERTED);
    eprintln!("video frame converted on the GPU vs the CPU: {d:?}");
    assert!(d.within(CONVERTED), "the GPU's frame differs from the CPU's: {d:?}");
    let gray = drawn.pixel(48, 48)[0];
    assert!(gray.abs_diff(150) <= 6, "bottom right is frame 13's gray: {gray}");
    let [r, g, b, a] = drawn.pixel(16, 16);
    assert!(
        r > 240 && g < 15 && b < 15 && a == 255,
        "top left is opaque red: {:?}",
        [r, g, b, a]
    );

    // Negative control: the frame before the seek is beyond the tolerance.
    let d = diff_within(&first, &cpu, CONVERTED);
    assert!(!d.within(CONVERTED), "frame 0 should not pass for frame 13: {d:?}");
    video.free(&mut ctx);
}

/// Planes of every layout, at an odd size, with a pattern that uses the
/// whole range of every plane.
fn planes_of(subsampling: crate::video::yuv::Subsampling, width: usize, height: usize) -> [Vec<u8>; 3] {
    use crate::video::yuv::Subsampling;
    let (cw, ch) = match subsampling {
        Subsampling::Both => (width.div_ceil(2), height.div_ceil(2)),
        Subsampling::Horizontal => (width.div_ceil(2), height),
        Subsampling::None => (width, height),
        Subsampling::Gray => (0, 0),
    };
    let y = (0..width * height)
        .map(|i| ((i * 37 + i / width * 11) % 256) as u8)
        .collect();
    let u = (0..cw * ch).map(|i| ((i * 53 + 17) % 256) as u8).collect();
    let v = (0..cw * ch).map(|i| ((i * 29 + 101) % 256) as u8).collect();
    [y, u, v]
}

#[test]
fn every_chroma_layout_and_matrix_converts_on_the_gpu_as_on_the_cpu() {
    use crate::video::yuv::{Matrix, Planes, Subsampling, to_rgba};
    let Some(gpu) = gpu() else { return };
    let mut converter = super::video_yuv::YuvToRgba::new(&gpu.device);
    let (width, height) = (37usize, 23usize);
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("converted"),
        size: wgpu::Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let cases = [
        (Subsampling::Both, Matrix::Bt709, false),
        (Subsampling::Horizontal, Matrix::Bt601, true),
        (Subsampling::None, Matrix::Bt2020, false),
        (Subsampling::Gray, Matrix::Bt709, true),
    ];
    for (subsampling, matrix, full_range) in cases {
        let [y, u, v] = planes_of(subsampling, width, height);
        let chroma_stride = if u.is_empty() {
            0
        } else {
            u.len()
                / if subsampling == Subsampling::Both {
                    height.div_ceil(2)
                } else {
                    height
                }
        };
        let planes = Planes {
            width,
            height,
            subsampling,
            y: &y,
            u: &u,
            v: &v,
            luma_stride: width,
            chroma_stride,
            matrix,
            full_range,
        };
        converter.convert(&gpu.device, &gpu.queue, &planes, &target);
        let gpu_rgba = read_texture(&gpu.device, &gpu.queue, &target);
        let mut cpu_rgba = vec![0u8; width * height * 4];
        to_rgba(&planes, &mut cpu_rgba);
        let as_image = |rgba: Vec<u8>| Image {
            width: width as u32,
            height: height as u32,
            rgba,
        };
        let d = diff_within(&as_image(gpu_rgba.clone()), &as_image(cpu_rgba.clone()), CONVERTED);
        let case = format!("{subsampling:?} {matrix:?} full range {full_range}");
        eprintln!("{case}: {d:?}");
        assert!(
            d.within(CONVERTED),
            "{case}: the GPU's pixels differ from the CPU's: {d:?}"
        );
        assert!(gpu_rgba.chunks(4).all(|p| p[3] == 255), "{case}: opaque");

        // Negative control: the chroma planes swapped are far off (a gray
        // picture has none to swap).
        if subsampling != Subsampling::Gray {
            let swapped = Planes { u: &v, v: &u, ..planes };
            converter.convert(&gpu.device, &gpu.queue, &swapped, &target);
            let d = diff_within(
                &as_image(read_texture(&gpu.device, &gpu.queue, &target)),
                &as_image(cpu_rgba),
                CONVERTED,
            );
            assert!(!d.within(CONVERTED), "{case}: swapped chroma should not pass: {d:?}");
        }
    }
}
