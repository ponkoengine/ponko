//! HDR display output on the GPU, offscreen (no window, no HDR display):
//!
//! - the output pass encodes the linear frame as scRGB, extended sRGB, PQ
//!   and SDR, against reference values computed on the CPU;
//! - each tone mapper's HDR form, through the real `post.wgsl` tone
//!   mapping pass: the SDR curve below the knee bit for bit, the highlights
//!   opened up to the headroom as the CPU twin says, monotonic, reaching
//!   the peak; no curve keeps light up to the headroom;
//! - a game's whole frame with HDR on (the production renderer, opened as
//!   for a window offering HDR): UI white is exactly paper white while the
//!   3D frame's highlights go well above it; switched to SDR while running
//!   (the negative control), both clip to the same white; a screenshot of
//!   an HDR frame is SDR;
//! - with HDR never opened, HDR settings change no byte of the frame.
//!
//! Skips without a GPU adapter (a skip is not a pass).
use super::hdr::{HdrEncoding, HdrOutput, expand_highlights, pq_encode};
use super::hdr_output::{HdrPass, OutputChoice, OutputUniform};
use super::render_test_support::{Image, TestGpu, gpu, keep_image, offscreen_ctx, read_texture, target};
use crate::prelude::*;
use crate::render::Tonemap;
use crate::three::PostUniform;

/// A half float's value.
fn from_f16(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exp = i32::from((bits >> 10) & 0x1f);
    let man = f32::from(bits & 0x3ff) / 1024.0;
    sign * match exp {
        0 => man * 2f32.powi(-14),
        31 => f32::INFINITY,
        e => (1.0 + man) * 2f32.powi(e - 15),
    }
}

/// The nearest half float (normal range; what the tests write).
fn to_f16(v: f32) -> u16 {
    if v == 0.0 {
        return 0;
    }
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    assert!(
        (1..31).contains(&exp),
        "{v} is outside the half float range the tests use"
    );
    let man = ((bits & 0x7f_ffff) + 0x1000) >> 13;
    let (exp, man) = if man == 0x400 { (exp + 1, 0) } else { (exp, man) };
    sign | ((exp as u16) << 10) | man as u16
}

/// Pixels of a read-back texture as RGBA floats.
fn decode(format: wgpu::TextureFormat, bytes: &[u8]) -> Vec<[f32; 4]> {
    match format {
        wgpu::TextureFormat::Rgba16Float => bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|p| [0, 1, 2, 3].map(|i| from_f16(u16::from_le_bytes([p[2 * i], p[2 * i + 1]]))))
            .collect(),
        wgpu::TextureFormat::Rgb10a2Unorm => bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                let c = |shift: u32| ((v >> shift) & 0x3ff) as f32 / 1023.0;
                [c(0), c(10), c(20), (v >> 30) as f32 / 3.0]
            })
            .collect(),
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [0, 1, 2, 3].map(|i| f32::from(p[i]) / 255.0))
            .collect(),
        other => panic!("no decoder for {other:?}"),
    }
}

/// sRGB's curve, continued above 1.
fn srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn close(got: f32, want: f32, rel: f32, abs: f32) -> bool {
    (got - want).abs() <= abs + rel * want.abs()
}

/// Encodes a frame image cleared to `color` with the output pass in
/// `uniform`'s encoding into a `format` image; its first pixel.
fn encode(gpu: &TestGpu, color: [f32; 3], uniform: OutputUniform, format: wgpu::TextureFormat) -> [f32; 4] {
    let mut pass = HdrPass::new(&gpu.device, &gpu.queue);
    pass.write(&gpu.queue, uniform);
    pass.prepare_pipeline(&gpu.device, format);
    let frame = pass.frame_view(&gpu.device, (4, 1));
    let out = target(&gpu.device, (4, 1), format);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("fill the frame"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &frame,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: f64::from(color[0]),
                    g: f64::from(color[1]),
                    b: f64::from(color[2]),
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    let view = out.create_view(&Default::default());
    pass.record(&mut encoder, &view, format, false, None);
    gpu.queue.submit([encoder.finish()]);
    decode(format, &read_texture(&gpu.device, &gpu.queue, &out))[0]
}

fn choice(encoding: HdrEncoding, relative: bool) -> OutputChoice {
    let (format, color_space) = match encoding {
        HdrEncoding::ScRgb => (
            wgpu::TextureFormat::Rgba16Float,
            wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
        ),
        HdrEncoding::ExtendedSrgb => (wgpu::TextureFormat::Rgba16Float, wgpu::SurfaceColorSpace::ExtendedSrgb),
        HdrEncoding::Pq => (wgpu::TextureFormat::Rgb10a2Unorm, wgpu::SurfaceColorSpace::Bt2100Pq),
        HdrEncoding::Sdr => (wgpu::TextureFormat::Rgba8UnormSrgb, wgpu::SurfaceColorSpace::Srgb),
    };
    OutputChoice {
        format,
        color_space,
        encoding,
        relative,
    }
}

#[test]
fn output_pass_encodes_scrgb_extended_srgb_pq_and_sdr_as_the_reference_math() {
    let Some(gpu) = gpu() else {
        return;
    };
    let paper = 200.0;
    let f16 = wgpu::TextureFormat::Rgba16Float;
    for v in [0.0, 0.18, 1.0, 4.0] {
        // A colored frame, so the PQ primaries' matrix shows.
        let c = [v, v * 0.5, v * 0.25];
        // scRGB on Windows: 1.0 is 80 nits, so paper white is 2.5.
        let got = encode(
            &gpu,
            c,
            OutputUniform::new(&choice(HdrEncoding::ScRgb, false), paper),
            f16,
        );
        for i in 0..3 {
            assert!(close(got[i], c[i] * 2.5, 2e-3, 1e-4), "scRGB {v}: {got:?}");
        }
        assert_eq!(got[3], 1.0, "opaque");
        // scRGB on macOS: 1.0 is the system's SDR white.
        let got = encode(
            &gpu,
            c,
            OutputUniform::new(&choice(HdrEncoding::ScRgb, true), paper),
            f16,
        );
        for i in 0..3 {
            assert!(close(got[i], c[i], 2e-3, 1e-4), "EDR {v}: {got:?}");
        }
        // Extended sRGB (a browser's HDR canvas): the curve above 1 too.
        let got = encode(
            &gpu,
            c,
            OutputUniform::new(&choice(HdrEncoding::ExtendedSrgb, true), paper),
            f16,
        );
        for i in 0..3 {
            assert!(close(got[i], srgb(c[i]), 2e-3, 2e-4), "extended sRGB {v}: {got:?}");
        }
        // PQ, in a float image (precise) and in the 10-bit one windows use.
        let want = pq_encode(c, paper);
        let pq = OutputUniform::new(&choice(HdrEncoding::Pq, false), paper);
        let got = encode(&gpu, c, pq, f16);
        for i in 0..3 {
            assert!(close(got[i], want[i], 2e-3, 2e-4), "PQ {v}: {got:?} want {want:?}");
        }
        let got = encode(&gpu, c, pq, wgpu::TextureFormat::Rgb10a2Unorm);
        for i in 0..3 {
            assert!(
                close(got[i], want[i], 0.0, 1.5 / 1023.0),
                "PQ 10-bit {v}: {got:?} want {want:?}"
            );
        }
        // SDR (HDR switched off, screenshots): clamped to white, sRGB.
        let got = encode(&gpu, c, OutputUniform::SDR, wgpu::TextureFormat::Rgba8UnormSrgb);
        for i in 0..3 {
            let want = (srgb(c[i].min(1.0)) * 255.0).round() / 255.0;
            assert!(close(got[i], want, 0.0, 1.0 / 255.0), "SDR {v}: {got:?}");
        }
    }
    // The negative control: paper white moves scRGB on Windows only.
    let bright = encode(
        &gpu,
        [1.0; 3],
        OutputUniform::new(&choice(HdrEncoding::ScRgb, false), 400.0),
        f16,
    );
    assert!(close(bright[0], 5.0, 2e-3, 0.0), "{bright:?}");
}

/// The tone mapping pass of `post.wgsl` (`fs_tonemap`) run on one row of
/// `input` colors with `post` and HDR output at `headroom`; the row out.
fn tonemap_row(gpu: &TestGpu, input: &[[f32; 3]], post: &crate::render::Post, headroom: Option<f32>) -> Vec<[f32; 4]> {
    let device = &gpu.device;
    let width = input.len() as u32;
    let f16 = wgpu::TextureFormat::Rgba16Float;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("post"),
        source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tone mapping"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_full"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_tonemap"),
            targets: &[Some(f16.into())],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let texture = |w: u32, format: wgpu::TextureFormat| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    // The lit frame: alpha 1 where models were drawn.
    let source = texture(width, f16);
    let texels: Vec<u16> = input
        .iter()
        .flat_map(|c| [to_f16(c[0]), to_f16(c[1]), to_f16(c[2]), to_f16(1.0)])
        .collect();
    gpu.queue.write_texture(
        source.as_image_copy(),
        bytemuck::cast_slice(&texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 8),
            rows_per_image: None,
        },
        source.size(),
    );
    let bloom = texture(1, f16);
    let lut = texture(1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let uniform = PostUniform::new(post, None)
        .scaled(post, (width, 1), (width, 1))
        .hdr(headroom);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: size_of::<PostUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&uniform));
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let view = |t: &wgpu::Texture| t.create_view(&Default::default());
    let (source_view, bloom_view, lut_view) = (view(&source), view(&bloom), view(&lut));
    let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&bloom_view),
            },
        ],
    });
    let group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&lut_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let out = target(device, (width, 1), f16);
    let out_view = view(&out);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tone mapping"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &out_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations::default(),
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group0, &[]);
        pass.set_bind_group(1, &group1, &[]);
        pass.draw(0..3, 0..1);
    }
    gpu.queue.submit([encoder.finish()]);
    decode(f16, &read_texture(device, &gpu.queue, &out))
}

#[test]
fn each_tonemapper_keeps_its_sdr_curve_below_the_knee_and_reaches_the_peak_in_hdr() {
    let Some(gpu) = gpu() else {
        return;
    };
    // Grey from 1/64 to 64 times white, in even stops.
    let n = 256;
    let grey: Vec<[f32; 3]> = (0..n)
        .map(|i| [(-6.0 + 12.0 * i as f32 / (n - 1) as f32).exp2(); 3])
        .collect();
    let headroom = 5.0;
    let knee = super::hdr::HIGHLIGHT_KNEE;
    for tonemap in [Tonemap::Aces, Tonemap::AgX, Tonemap::Neutral, Tonemap::None] {
        let post = crate::render::Post {
            tonemap,
            ..Default::default()
        };
        let sdr = tonemap_row(&gpu, &grey, &post, None);
        let hdr = tonemap_row(&gpu, &grey, &post, Some(headroom));
        // The negative control: headroom 1 is the SDR frame exactly.
        assert_eq!(
            tonemap_row(&gpu, &grey, &post, Some(1.0)),
            sdr,
            "{tonemap:?}: headroom 1 is SDR"
        );
        assert!(
            sdr.iter().all(|p| p[0] <= 1.0),
            "{tonemap:?}: SDR stays at or below white"
        );
        let mut last = 0.0;
        for (i, (s, h)) in sdr.iter().zip(&hdr).enumerate() {
            assert!(h[0] + 1e-3 >= last, "{tonemap:?}: monotonic at {i}: {h:?} after {last}");
            last = h[0];
            if tonemap == Tonemap::None {
                let want = grey[i][0].min(headroom);
                assert!(
                    close(h[0], want, 1e-2, 1e-3),
                    "{tonemap:?} keeps light to the headroom: {h:?} for {want}"
                );
                continue;
            }
            if s[0] < knee - 0.01 {
                assert_eq!(h, s, "{tonemap:?}: below the knee the SDR curve, bit for bit ({i})");
            } else {
                // The CPU twin from the SDR value as read back (a half
                // float, whose rounding the steep part above the knee
                // amplifies): within a few percent.
                let want = expand_highlights([s[0], s[1], s[2]], headroom);
                assert!(
                    close(h[0], want[0], 3e-2, 2e-2),
                    "{tonemap:?} at {i}: {h:?}, twin {want:?}"
                );
            }
        }
        let top = hdr.last().unwrap()[0];
        assert!(
            top >= 0.95 * headroom && top <= headroom + 1e-2,
            "{tonemap:?} reaches the peak: {top}"
        );
        assert!(
            top > 4.0 * sdr.last().unwrap()[0],
            "{tonemap:?}: brighter than SDR white"
        );
    }
    // A colored highlight keeps its hue: the channels' ratios of the SDR
    // picture (the expansion scales all three by one factor).
    let post = crate::render::Post {
        tonemap: Tonemap::Neutral,
        ..Default::default()
    };
    let orange = [[6.0, 2.0, 0.5]];
    let (s, h) = (
        tonemap_row(&gpu, &orange, &post, None)[0],
        tonemap_row(&gpu, &orange, &post, Some(headroom))[0],
    );
    assert!(h[0] > 1.5 * s[0], "brighter: {h:?} vs {s:?}");
    for i in 1..3 {
        assert!(
            close(h[i] / h[0], s[i] / s[0], 1e-2, 1e-3),
            "ratios kept: {h:?} vs {s:?}"
        );
    }
}

#[test]
fn hdr_off_leaves_the_post_settings_the_sdr_shader_reads_unchanged() {
    let post = crate::render::Post::default();
    let plain = PostUniform::new(&post, Some(16)).camera(0.1, 500.0);
    for off in [None, Some(1.0), Some(0.5), Some(f32::NAN)] {
        let same = plain.hdr(off);
        assert_eq!(bytemuck::bytes_of(&same), bytemuck::bytes_of(&plain), "{off:?}");
    }
    let on = plain.hdr(Some(4.0));
    assert_eq!((on.mode[3], on.camera[2]), (1, 4.0));
}

/// The window and its parts: UI white in the top left corner, a very
/// bright emissive box in the middle.
const SIZE: (u32, u32) = (64, 64);
const UI: (u32, u32) = (4, 4);
const BOX: (u32, u32) = (32, 32);

/// A game frame on `ctx`: a black background, no sky or glow, the box and
/// a white UI square.
fn scene(ctx: &mut crate::ctx::Ctx) {
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.lighting.shadows = false;
    ctx.scene3d.post.bloom = 0.0;
    ctx.gfx.clear_color = Color::BLACK;
    let material = ctx
        .gfx
        .create_material(crate::render::MaterialDesc::color(Color::BLACK).with_emissive(Color::rgb(6.0, 6.0, 6.0)));
    let cube = ctx.gfx.create_mesh(&crate::render::MeshData::cube(1.5), material);
    ctx.world
        .spawn((crate::d3::Transform::default(), crate::render::Model::new(cube)));
    ctx.scene3d.camera.position = vec3(0.0, 0.0, 3.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let (w, h) = (SIZE.0 as f32, SIZE.1 as f32);
    let ui = &mut ctx.ui;
    let root = ui
        .root()
        .layout(crate::ui::UiLayout::anchored(0.0, 0.0, 12.0 / w, 12.0 / h))
        .background(Color::WHITE)
        .border_width(0.0)
        .id();
    ui.push_modeless(root);
    let still = crate::ui::UiInput {
        pointer: vec2(-10.0, -10.0),
        ..Default::default()
    };
    ctx.ui.frame(vec2(w, h), &still);
    // The meshes reach the GPU with the first frame drawn.
    ctx.render_frame();
}

/// A frame of `ctx`, as the offscreen window holds it: format and pixels.
fn output(ctx: &mut crate::ctx::Ctx) -> (wgpu::TextureFormat, Vec<[f32; 4]>) {
    assert_eq!(ctx.render_frame(), crate::render::FrameStatus::Offscreen);
    let (format, bytes) = ctx.gfx.renderer.as_ref().unwrap().read_output().expect("a frame");
    (format, decode(format, &bytes))
}

fn at(pixels: &[[f32; 4]], (x, y): (u32, u32)) -> [f32; 4] {
    pixels[(y * SIZE.0 + x) as usize]
}

/// A screenshot of the next frame.
fn shot(ctx: &mut crate::ctx::Ctx) -> Image {
    let path = std::env::temp_dir().join(format!(
        "ponko-hdr-{}-{:?}.png",
        std::process::id(),
        std::thread::current().id()
    ));
    ctx.gfx.queue_screenshot(path.clone(), 0, None);
    ctx.render_frame();
    let shot = image::open(&path).expect("the frame was captured").to_rgba8();
    let _ = std::fs::remove_file(&path);
    let image = Image {
        width: shot.width(),
        height: shot.height(),
        rgba: shot.into_raw(),
    };
    keep_image(&image);
    image
}

/// A game context drawing through the production renderer opened for HDR
/// output as on a window offering `choice`.
fn hdr_ctx(choice: OutputChoice, settings: HdrOutput) -> Option<crate::ctx::Ctx> {
    let gpu = gpu()?;
    let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, vec2(SIZE.0 as f32, SIZE.1 as f32));
    ctx.audio.headless = true;
    ctx.gfx.hdr_output = settings;
    ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen_hdr(gpu, SIZE, choice, settings));
    Some(ctx)
}

fn settings() -> HdrOutput {
    let mut hdr = HdrOutput::on();
    hdr.paper_white_nits = 200.0;
    hdr.peak_nits = 1000.0;
    hdr
}

#[test]
fn ui_stays_at_paper_white_while_3d_highlights_go_above_it_in_an_hdr_frame() {
    let Some(mut ctx) = hdr_ctx(choice(HdrEncoding::ScRgb, false), settings()) else {
        return;
    };
    scene(&mut ctx);
    let status = ctx.gfx.hdr_status();
    assert!(status.active && status.available, "{status:?}");
    assert_eq!(
        (
            status.encoding,
            status.paper_white_nits,
            status.peak_nits,
            status.headroom
        ),
        (HdrEncoding::ScRgb, 200.0, 1000.0, 5.0)
    );
    let (format, hdr) = output(&mut ctx);
    assert_eq!(format, wgpu::TextureFormat::Rgba16Float);
    // scRGB: 1.0 is 80 nits, so 200-nit paper white is 2.5.
    let ui = at(&hdr, UI);
    for c in &ui[..3] {
        assert!(close(*c, 2.5, 2e-3, 0.0), "UI white is paper white: {ui:?}");
    }
    let lit = at(&hdr, BOX);
    assert!(
        lit[0] > 2.0 * ui[0],
        "the box's highlight is far above paper white: {lit:?} vs {ui:?}"
    );
    assert!(lit[0] <= 5.0 * 2.5 + 0.05, "and never above the peak: {lit:?}");
    // A screenshot of an HDR frame is SDR: both clip to white.
    let image = shot(&mut ctx);
    assert_eq!(image.pixel(UI.0, UI.1), [255; 4]);
    assert_eq!(image.pixel(BOX.0, BOX.1), [255; 4]);

    // Switched to SDR while running (the negative control): the window
    // takes SDR, and the box clips to the same white as the UI.
    let mut off = settings();
    off.enabled = false;
    ctx.gfx.set_hdr_output(off);
    let status = ctx.gfx.hdr_status();
    assert!(!status.active && status.available);
    assert_eq!(status.reason, Some(crate::render::HdrOffReason::Off));
    let (format, sdr) = output(&mut ctx);
    assert_eq!(format, wgpu::TextureFormat::Rgba8UnormSrgb);
    assert_eq!(at(&sdr, UI), [1.0; 4]);
    assert_eq!(at(&sdr, BOX), at(&sdr, UI), "SDR: the highlight clips to UI white");

    // And back: the same HDR frame as before.
    ctx.gfx.set_hdr_output(settings());
    let (_, again) = output(&mut ctx);
    assert_eq!(again, hdr, "HDR again draws the same frame");
    // Paper white moves the UI (and the frame with it).
    let mut brighter = settings();
    brighter.paper_white_nits = 320.0;
    ctx.gfx.set_hdr_output(brighter);
    let (_, bright) = output(&mut ctx);
    assert!(close(at(&bright, UI)[0], 4.0, 2e-3, 0.0), "{:?}", at(&bright, UI));
}

#[test]
fn a_pq_window_gets_paper_white_ui_in_hdr10() {
    let Some(mut ctx) = hdr_ctx(choice(HdrEncoding::Pq, false), settings()) else {
        return;
    };
    scene(&mut ctx);
    let (format, pq) = output(&mut ctx);
    assert_eq!(format, wgpu::TextureFormat::Rgb10a2Unorm);
    let want = pq_encode([1.0; 3], 200.0)[0];
    let ui = at(&pq, UI);
    for c in &ui[..3] {
        assert!(
            close(*c, want, 0.0, 1.5 / 1023.0),
            "UI at 200 nits in PQ: {ui:?} want {want}"
        );
    }
    let lit = at(&pq, BOX);
    let peak = pq_encode([5.0; 3], 200.0)[0];
    assert!(
        lit[0] > ui[0] + 0.1 && lit[0] <= peak + 2.0 / 1023.0,
        "{lit:?}, peak {peak}"
    );
}

#[test]
fn hdr_settings_change_no_byte_of_a_frame_drawn_without_hdr() {
    let Some(mut ctx) = offscreen_ctx(SIZE) else {
        return;
    };
    scene(&mut ctx);
    let before = shot(&mut ctx);
    // `PONKO_HDR_SDR_DUMP=file` keeps the frame's bytes, to compare a build
    // without HDR output against this one.
    if let Some(path) = std::env::var_os("PONKO_HDR_SDR_DUMP") {
        std::fs::write(path, &before.rgba).unwrap();
    }
    // The window didn't open with HDR: asking for it changes nothing.
    let mut hdr = HdrOutput::on();
    hdr.paper_white_nits = 300.0;
    hdr.peak_nits = 2000.0;
    ctx.gfx.set_hdr_output(hdr);
    let status = ctx.gfx.hdr_status();
    assert!(!status.active, "{status:?}");
    assert!(status.reason.is_some());
    let after = shot(&mut ctx);
    assert!(before == after, "the SDR frame is byte for byte the same");
    assert_eq!(before.pixel(UI.0, UI.1), [255; 4]);
}
