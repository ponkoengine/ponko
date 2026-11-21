//! Screen-space reflections: a mirror floor reflects what's on screen
//! above it. Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{AntiAliasing, GpuFeature, MaterialDesc, Mesh, MeshData, Model};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

const SIZE: u32 = 128;

/// An offscreen renderer with a 1x1 white texture, and a `Ctx` to fill.
struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    output: wgpu::Texture,
    target: wgpu::TextureView,
    textures: Vec<GpuTexture>,
    renderer: Renderer3D,
    ctx: Ctx,
    /// Asks the next frame to forget the history (`reset_temporal_history`).
    reset_history: bool,
}

impl Rig {
    /// `None` without a GPU.
    fn sized(width: u32, height: u32) -> Option<Self> {
        Self::with_limits(width, height, false)
    }

    /// `game`: with the texture slots a game asks for (`gpu.rs`), room for
    /// decals, global illumination's probes and the reflections' spread
    /// image; otherwise the default 16.
    fn with_limits(width: u32, height: u32, game: bool) -> Option<Self> {
        let gpu = crate::render::render_test_support::gpu_with(|adapter| {
            let slots = adapter.limits().max_sampled_textures_per_shader_stage.clamp(16, 24);
            Ok(wgpu::DeviceDescriptor {
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                required_limits: wgpu::Limits {
                    max_sampled_textures_per_shader_stage: if game { slots } else { 16 },
                    ..Default::default()
                },
                ..Default::default()
            })
        })?;
        let (device, queue) = (gpu.device, gpu.queue);
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
        let textures = vec![GpuTexture {
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
                width,
                height,
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
        let renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(width as f32, height as f32));
        ctx.scene3d.sky.atmosphere = false;
        Some(Self {
            device,
            queue,
            output,
            target,
            textures,
            renderer,
            ctx,
            reset_history: false,
        })
    }

    /// A mesh of `data` with `desc`, on the CPU and the GPU.
    fn mesh(&mut self, data: &MeshData, desc: MaterialDesc) -> Mesh {
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// Draws the world and reads the pixels back (RGBA).
    fn draw(&mut self) -> Vec<u8> {
        self.draw_timed(None)
    }

    fn draw_timed(&mut self, mut timer: Option<&mut super::timer::GpuTimer>) -> Vec<u8> {
        let width = self.output.width();
        let height = self.output.height();
        if let Some(t) = &mut timer {
            t.collect(&self.device);
            t.begin_frame();
        }
        let ctx = &mut self.ctx;
        let mut frame = Frame3D::default();
        frame.lod = ctx.scene3d.lod;
        frame.motion.reset = std::mem::take(&mut self.reset_history);
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let scene = Scene3D {
            frame: &frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            // Sunlight only: the fog glows with nothing else.
            light: SkyLight {
                sun: Vec3::splat(ctx.scene3d.lighting.sun_intensity),
                sky: Vec3::ZERO,
                ground: Vec3::ZERO,
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (0.0, 1.0 / 60.0),
            occlusion: false,
        };
        self.renderer.prepare(
            &self.device,
            &self.queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: (width, height),
            },
        );
        let buffers = self.renderer.draw(
            &self.device,
            &self.target,
            wgpu::Color::BLACK,
            &self.textures,
            timer.as_deref(),
        );
        self.queue.submit(buffers);
        if let Some(t) = &mut timer {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            t.resolve(&mut encoder);
            self.queue.submit([encoder.finish()]);
            t.submitted();
        }
        // Rows padded to the copy alignment.
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            self.output.size(),
        );
        self.queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = readback.get_mapped_range(..).unwrap();
        if row == width * 4 {
            return data.to_vec();
        }
        data.chunks(row as usize)
            .flat_map(|r| &r[..(width * 4) as usize])
            .copied()
            .collect()
    }
}

/// The world point under screen pixel `p`'s projection.
fn pixel(view_proj: Mat4, p: Vec3) -> (u32, u32) {
    let c = view_proj * p.extend(1.0);
    let ndc = c.truncate() / c.w;
    (
        ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32,
        ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32,
    )
}

/// A mirror floor and a glowing red box over it: with screen-space
/// reflections, the floor in front of the box shows it.
fn mirror_rig() -> Option<(Rig, Vec3, Vec3)> {
    floor_rig(0.0)
}

fn floor_rig(roughness: f32) -> Option<(Rig, Vec3, Vec3)> {
    floor_rig_with(roughness, false)
}

/// `game`: with the texture slots a game asks for (see `Rig::sized_like_a_game`).
fn floor_rig_with(roughness: f32, game: bool) -> Option<(Rig, Vec3, Vec3)> {
    let mut rig = Rig::with_limits(SIZE, SIZE, game)?;
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::ScreenSpaceReflections) {
        return None;
    }
    let mirror = MaterialDesc {
        color: Color::WHITE,
        roughness,
        metallic: 1.0,
        ..Default::default()
    };
    let glow = MaterialDesc {
        color: Color::BLACK,
        emissive: Color::rgb(4.0, 0.0, 0.0),
        ..Default::default()
    };
    let floor = rig.mesh(&MeshData::cube(1.0), mirror);
    let block = rig.mesh(&MeshData::cube(1.0), glow);
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, -0.5, 0.0),
            scale: vec3(20.0, 1.0, 20.0),
            ..Default::default()
        },
        Model::new(floor),
    ));
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        },
        Model::new(block),
    ));
    rig.ctx.scene3d.lighting.shadows = false;
    rig.ctx.scene3d.lighting.sun_intensity = 0.0;
    rig.ctx.scene3d.post.bloom = 0.0;
    rig.ctx.scene3d.camera.position = vec3(0.0, 2.0, 6.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 0.5, 0.0));
    // The box's center mirrored is (0, -1, 0): seen from the camera through
    // the floor at (0, 0, 2). To the side, the floor reflects only the dark.
    Some((rig, vec3(0.0, 0.0, 2.0), vec3(2.5, 0.0, 2.0)))
}

#[test]
fn a_mirror_floor_reflects_the_box_above_it() {
    let Some((mut rig, reflection, aside)) = mirror_rig() else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::ScreenSpaceReflections) {
        return;
    }
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let at = |pixels: &[u8], p: Vec3| {
        let (x, y) = pixel(view_proj, p);
        let i = ((y * SIZE + x) * 4) as usize;
        (i32::from(pixels[i]), i32::from(pixels[i + 1]))
    };
    let (red, _) = at(&rig.draw(), reflection);
    assert!(red < 40, "without SSR the floor reflects only the dark sky: {red}");

    rig.ctx.scene3d.post.ssr = 1.0;
    // The first frame has no image of the last one to reflect.
    rig.draw();
    let pixels = rig.draw();
    let (red, green) = at(&pixels, reflection);
    assert!(red > 120 && green < 60, "the box's red, mirrored: ({red}, {green})");
    let (side, _) = at(&pixels, aside);
    assert!(side < 40, "nothing to reflect beside it: {side}");

    rig.ctx.scene3d.post.ssr = 0.0;
    let (red, _) = at(&rig.draw(), reflection);
    assert!(red < 40, "off again: {red}");
}

#[test]
fn reflections_work_with_taa_and_fade_with_distance() {
    let Some((mut rig, reflection, _)) = mirror_rig() else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::ScreenSpaceReflections) {
        return;
    }
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let red = |pixels: &[u8]| {
        let (x, y) = pixel(view_proj, reflection);
        i32::from(pixels[((y * SIZE + x) * 4) as usize])
    };
    rig.ctx.scene3d.post.ssr = 1.0;
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    let mut pixels = Vec::new();
    for _ in 0..8 {
        pixels = rig.draw();
    }
    assert!(red(&pixels) > 120, "TAA keeps the reflection: {}", red(&pixels));

    // The box is about 1.7 units along the reflected ray: out of reach.
    rig.ctx.scene3d.post.ssr_max_distance = 1.0;
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
    rig.draw();
    let pixels = rig.draw();
    assert!(red(&pixels) < 40, "too far to reflect: {}", red(&pixels));
}

#[test]
fn reflections_and_ambient_occlusion_share_the_opaque_pass_image() {
    // With the spread image copied into the first level, and read directly.
    for game in [false, true] {
        reflections_and_ambient_occlusion_take_turns(game);
    }
}

fn reflections_and_ambient_occlusion_take_turns(game: bool) {
    let Some((mut rig, reflection, _)) = floor_rig_with(0.0, game) else {
        return;
    };
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let red = |pixels: &[u8]| {
        let (x, y) = pixel(view_proj, reflection);
        i32::from(pixels[((y * SIZE + x) * 4) as usize])
    };
    rig.ctx.scene3d.post.ambient_occlusion.enabled = true;
    rig.ctx.scene3d.post.ao_half_resolution = true;
    rig.ctx.scene3d.post.ao_temporal = true;
    let pixels = rig.draw();
    assert!(red(&pixels) < 40, "GTAO alone reflects nothing: {}", red(&pixels));
    rig.ctx.scene3d.post.ssr = 1.0;
    rig.draw();
    let pixels = rig.draw();
    assert!(red(&pixels) > 120, "GTAO marks the reflections: {}", red(&pixels));
    rig.ctx.scene3d.post.ambient_occlusion.enabled = false;
    rig.draw();
    let pixels = rig.draw();
    assert!(red(&pixels) > 120, "and so does a clear without it: {}", red(&pixels));
    rig.ctx.scene3d.post.ambient_occlusion.enabled = true;
    rig.draw();
    let pixels = rig.draw();
    assert!(red(&pixels) > 120, "and GTAO again: {}", red(&pixels));
}

#[test]
fn the_spread_image_read_directly_draws_what_its_copy_does() {
    // A rough floor, where the spread decides the blur: with the texture
    // slots a game asks for, the opaque pass reads the spread image itself;
    // with the default 16, a copy in the screen image's first level.
    let draw = |game: bool, ao: bool| {
        let (mut rig, reflection, _) = floor_rig_with(0.4, game)?;
        let slot = crate::render::capabilities::GpuLimits::of(&rig.device).reflection_spread();
        if game && !slot {
            crate::render::render_test_support::skip("no texture slot for the reflection spread image");
            return None;
        }
        assert!(game || !slot, "16 texture slots leave none for the spread image");
        rig.ctx.scene3d.post.ambient_occlusion.enabled = ao;
        rig.ctx.scene3d.post.ssr = 1.0;
        for _ in 0..3 {
            rig.draw();
        }
        let pixels = rig.draw();
        let (x, y) = pixel(rig.ctx.scene3d.camera.view_proj(), reflection);
        let red = i32::from(pixels[((y * SIZE + x) * 4) as usize]);
        Some((pixels, red))
    };
    for ao in [false, true] {
        let Some((copied, copied_red)) = draw(false, ao) else {
            return;
        };
        let Some((direct, direct_red)) = draw(true, ao) else {
            return;
        };
        assert!(
            copied_red > 60 && direct_red > 60,
            "both reflect the box: {copied_red}, {direct_red} (ao {ao})"
        );
        // GTAO's denoise copies no spread where the first depth sample is
        // sky, so edge pixels may differ a little.
        let diffs: Vec<u8> = copied.iter().zip(&direct).map(|(a, b)| a.abs_diff(*b)).collect();
        let worst = diffs.iter().copied().max().unwrap_or(0);
        let off = diffs.iter().filter(|&&d| d > 2).count() as f32 / diffs.len() as f32;
        assert!(
            worst <= 16 && off < 0.005,
            "read directly or copied, the same image: off by up to {worst}, {:.2}% by more than 2 (ao {ao})",
            off * 100.0
        );
    }
    // Negative control: with the reflections off, no red there.
    let Some((mut rig, reflection, _)) = floor_rig_with(0.4, true) else {
        return;
    };
    let (x, y) = pixel(rig.ctx.scene3d.camera.view_proj(), reflection);
    let off = i32::from(rig.draw()[((y * SIZE + x) * 4) as usize]);
    assert!(off < 40, "no reflections, no red: {off}");
}

#[test]
fn a_glossy_floor_has_a_wider_reflection_no_brighter_than_a_mirrors() {
    fn profile(roughness: f32) -> Option<(f32, f32)> {
        let (mut rig, reflection, _) = floor_rig(roughness)?;
        // Keep the bright source below display clipping so peak comparison
        // measures filtering rather than the tone mapper's saturation.
        rig.ctx.scene3d.post.exposure = -4.0;
        let (cx, cy) = pixel(rig.ctx.scene3d.camera.view_proj(), reflection);
        let off = rig.draw();
        rig.ctx.scene3d.post.ssr = 1.0;
        rig.draw();
        let on = rig.draw();
        let mut energy = 0.0f32;
        let mut moment = 0.0f32;
        let mut peak = 0.0f32;
        for x in cx - 24..=cx + 24 {
            let at = ((cy * SIZE + x) * 4) as usize;
            assert!(off[at] < 40, "SSR off leaves the glossy floor dark");
            let value = (f32::from(on[at]) - f32::from(off[at])).max(0.0);
            energy += value;
            moment += value * (x as f32 - cx as f32).powi(2);
            peak = peak.max(value);
        }
        assert!(energy > 100.0, "the floor still reflects the box");
        rig.ctx.scene3d.post.ssr = 0.0;
        let disabled = rig.draw();
        assert!(disabled[((cy * SIZE + cx) * 4) as usize] < 40);
        Some((peak, (moment / energy).sqrt()))
    }
    let Some(mirror) = profile(0.0) else { return };
    let Some(glossy) = profile(0.35) else { return };
    eprintln!("reflection peak / RMS width: mirror {mirror:?}, roughness 0.35 {glossy:?}");
    // The box is close to the floor: blurred by its ray's cone, the middle
    // of its reflection keeps the mirror's brightness.
    assert!(
        glossy.0 <= mirror.0,
        "glossy peak doesn't rise: {mirror:?} -> {glossy:?}"
    );
    assert!(
        glossy.1 > mirror.1 * 1.1,
        "glossy reflection spreads: {mirror:?} -> {glossy:?}"
    );
}

#[test]
fn small_and_odd_sizes_filter_reflections() {
    // Too small for any filtered level, for some of them, and odd sizes.
    for (width, height) in [(1, 1), (3, 2), (5, 3), (37, 19), (130, 67)] {
        let Some(mut rig) = Rig::sized(width, height) else {
            return;
        };
        let floor = rig.mesh(
            &MeshData::cube(1.0),
            MaterialDesc {
                roughness: 0.6,
                metallic: 1.0,
                ..Default::default()
            },
        );
        rig.ctx.world.spawn((
            Transform3D {
                scale: vec3(20.0, 1.0, 20.0),
                ..Default::default()
            },
            Model::new(floor),
        ));
        rig.ctx.scene3d.camera.position = vec3(0.0, 2.0, 6.0);
        rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
        rig.ctx.scene3d.post.ssr = 1.0;
        for _ in 0..3 {
            assert_eq!(rig.draw().len(), (width * height * 4) as usize);
        }
    }
}

/// Offscreen timestamp measurement; run with --release --ignored --nocapture.
/// It prints the trace's span and the frame's GPU time with and without the
/// filtered levels, in alternating blocks of frames so both see the same
/// GPU clocks. (The filter's own span isn't printed: tile-based GPUs run
/// it alongside the start of the opaque pass, so the profiler, which
/// charges each pass from where the one before it ended, charges little or
/// none of it there.)
#[test]
#[ignore]
fn reflection_filter_gpu_cost() {
    let Some(mut rig) = Rig::sized(1920, 1080) else { return };
    let Some(mut timer) = super::timer::GpuTimer::new(&rig.device, &rig.queue) else {
        crate::render::render_test_support::skip("timestamp queries unavailable");
        return;
    };
    let floor = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            roughness: 0.35,
            metallic: 1.0,
            ..Default::default()
        },
    );
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, -0.5, 0.0),
            scale: vec3(20.0, 1.0, 20.0),
            ..Default::default()
        },
        Model::new(floor),
    ));
    let block = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            emissive: Color::rgb(4.0, 0.0, 0.0),
            ..Default::default()
        },
    );
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        },
        Model::new(block),
    ));
    rig.ctx.scene3d.camera.position = vec3(0.0, 2.0, 6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.post.ssr = 1.0;
    rig.ctx.scene3d.post.bloom = 0.0;
    rig.ctx.scene3d.lighting.shadows = false;
    // [with the filter, without]
    let mut frames = [Vec::new(), Vec::new()];
    let mut trace = Vec::new();
    for frame in 0..1230 {
        let skip = (frame / 20) % 2 == 1;
        super::ssr::SKIP_FILTER.store(skip, std::sync::atomic::Ordering::Relaxed);
        rig.draw_timed(Some(&mut timer));
        // Results come back a few frames late: keep each block's last 15.
        if frame < 30 || frame % 20 < 5 {
            continue;
        }
        frames[usize::from(skip)].push(timer.last_frame.iter().map(|s| s.ms).sum::<f32>());
        if let Some(pass) = timer.last_frame.iter().find(|p| p.name == "screen reflections") {
            trace.push(pass.ms);
        }
    }
    super::ssr::SKIP_FILTER.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(!trace.is_empty(), "GPU timestamps must be collected");
    let median = |samples: &mut Vec<f32>| {
        samples.sort_by(f32::total_cmp);
        samples[samples.len() / 2]
    };
    let [mut with, mut without] = frames;
    let (with, without) = (median(&mut with), median(&mut without));
    eprintln!(
        "1920x1080: trace median {:.4} ms; frame median {with:.4} ms with the filtered levels, \
         {without:.4} ms without (+{:.4} ms)",
        median(&mut trace),
        with - without
    );
}

#[test]
fn a_history_reset_drops_the_last_frames_image_from_the_reflections() {
    let Some((mut rig, reflection, _)) = mirror_rig() else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::ScreenSpaceReflections) {
        return;
    }
    // A dark box far off to the side, to swap with the glowing one.
    let shade = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            roughness: 1.0,
            ..Default::default()
        },
    );
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(30.0, 1.0, 0.0)), Model::new(shade)));
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let red = |pixels: &[u8]| {
        let (x, y) = pixel(view_proj, reflection);
        i32::from(pixels[((y * SIZE + x) * 4) as usize])
    };
    let swap = |rig: &mut Rig| {
        rig.ctx.world.query::<&mut Transform3D>().for_each(|t| {
            if t.position.y == 1.0 {
                t.position.x = if t.position.x == 0.0 { 30.0 } else { 0.0 };
            }
        });
    };
    rig.ctx.scene3d.post.ssr = 1.0;
    rig.draw();
    let settled = red(&rig.draw());
    assert!(settled > 120, "the glowing box is reflected: {settled}");
    // Negative control: swapped without a reset, the dark box is reflected
    // in the colors the last frame had there.
    swap(&mut rig);
    let stale = red(&rig.draw());
    let next = red(&rig.draw());
    eprintln!("reflection: settled {settled}, swapped without a reset {stale}, a frame later {next}");
    assert!(stale > 120, "the stale image shows the old box: {stale}");
    assert!(next < 40, "and the next frame the dark one: {next}");
    swap(&mut rig);
    rig.draw();
    assert!(red(&rig.draw()) > 120, "the glowing box is back");
    // With a reset the swap frame reflects no old colors.
    swap(&mut rig);
    rig.reset_history = true;
    let reset = red(&rig.draw());
    let after = red(&rig.draw());
    eprintln!("reflection: swapped with a reset {reset}, a frame later {after}");
    assert!(reset < 40, "a reset frame reflects nothing stale: {reset}");
    assert!(after < 40, "and the next shows the dark box: {after}");
}

// What screen-space reflections cannot see, measured (the quality baseline
// of `crates/ponko_bench/RAY_TRACING.md`): a mirror reflects only what the
// depth buffer holds, so the box drops out of the floor wherever it is off
// the screen or hidden behind something else, although the floor point and
// its reflection ray are the same. A ray-traced reflection keeps it (see
// `ray_query_prototype_render_test.rs`).

/// The red at `reflection` once reflections have settled (a few frames, so
/// the history holds this camera's image).
fn settled_red(rig: &mut Rig, reflection: Vec3) -> i32 {
    rig.ctx.scene3d.post.ssr = 1.0;
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let (x, y) = pixel(view_proj, reflection);
    assert!(x < SIZE && y < SIZE, "the floor point is on screen");
    let mut pixels = Vec::new();
    for _ in 0..4 {
        pixels = rig.draw();
    }
    i32::from(pixels[((y * SIZE + x) * 4) as usize])
}

/// The camera of `floor_rig`, turned down 48 degrees: the glowing box
/// (corners from (-0.5, 0.5, -0.5) to (0.5, 1.5, 0.5)) is above the top of
/// the screen, the floor point that mirrors it, (0, 0, 2), on it.
fn look_below_the_box(rig: &mut Rig) {
    let pitch = 48f32.to_radians();
    let eye = rig.ctx.scene3d.camera.position;
    rig.ctx
        .scene3d
        .camera
        .look_at(eye + vec3(0.0, -pitch.sin(), -pitch.cos()));
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    for corner in 0..8 {
        let p = vec3(
            if corner & 1 == 0 { -0.5 } else { 0.5 },
            if corner & 2 == 0 { 0.5 } else { 1.5 },
            if corner & 4 == 0 { -0.5 } else { 0.5 },
        );
        let clip = view_proj * p.extend(1.0);
        assert!(clip.y / clip.w > 1.0, "box corner {p} is off the top of the screen");
    }
}

/// Pins a known limitation, not wanted behaviour: screen-space reflections
/// can't show what is off the screen. It is the measured baseline in §2 of
/// `crates/ponko_bench/RAY_TRACING.md`. When the limitation is fixed, flip
/// `lost < 40` to `lost > 120` and update the note.
#[test]
fn screen_space_reflections_lose_a_box_above_the_screen() {
    let Some((mut on_screen, reflection, _)) = mirror_rig() else {
        return;
    };
    // Positive control: the box on screen is reflected at the floor point.
    let seen = settled_red(&mut on_screen, reflection);
    drop(on_screen);
    let Some((mut rig, _, _)) = mirror_rig() else {
        return;
    };
    look_below_the_box(&mut rig);
    let lost = settled_red(&mut rig, reflection);
    eprintln!("ssr reflection of the glowing box: on screen {seen}, off screen {lost} (red, 0-255)");
    assert!(seen > 120, "the box on screen is reflected: {seen}");
    assert!(lost < 40, "the box above the screen is not: {lost}");
}

/// Pins a known limitation, not wanted behaviour: screen-space reflections
/// can't show what is hidden behind something on screen. It is the
/// measured baseline in §2 of `crates/ponko_bench/RAY_TRACING.md`. When the
/// limitation is fixed, flip `hidden < 40` to `hidden > 120` and update the
/// note.
#[test]
fn screen_space_reflections_lose_a_box_hidden_behind_a_wall() {
    let Some((mut open, reflection, _)) = mirror_rig() else {
        return;
    };
    let seen = settled_red(&mut open, reflection);
    drop(open);
    let Some((mut rig, _, _)) = mirror_rig() else {
        return;
    };
    // A dark wall between the camera and the box, from y 0.6 to 2.4 at
    // z 1.2: it hides the box from the camera at (0, 2, 6) (the box's
    // lowest front edge is seen through y 0.69 there), while the floor
    // point's ray to the box passes under it (y 0.4 at z 1.2).
    let wall = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            roughness: 1.0,
            ..Default::default()
        },
    );
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 1.5, 1.2),
            scale: vec3(3.0, 1.8, 0.1),
            ..Default::default()
        },
        Model::new(wall),
    ));
    let hidden = settled_red(&mut rig, reflection);
    eprintln!("ssr reflection of the glowing box: in view {seen}, behind a wall {hidden} (red, 0-255)");
    assert!(seen > 120, "the box in view is reflected: {seen}");
    assert!(hidden < 40, "the box behind the wall is not: {hidden}");
}

// ---- The reflection cone: blur by roughness and hit distance ----

use super::regression_stage::{EFFECT_REFERENCE, Stage};
use super::render_test_support::{Image, check_reference, reference_rejects};

/// Two thin glowing red bars (0.06 thick, 2.4 long, along x) over a white metal floor
/// of `roughness`, nothing else lit, the bars kept below display clipping
/// (exposure -5). The low bar's reflection is a contact reflection: the
/// floor's ray reaches it after about 0.7. The high bar's, after about 2.6.
/// Returns the floor points that mirror a point on each, low then high.
fn bars_over_a_floor(stage: &mut Stage, roughness: f32) -> (Vec3, Vec3) {
    let floor = stage.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::WHITE,
            roughness,
            metallic: 1.0,
            ..Default::default()
        },
    );
    let glow = stage.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            emissive: Color::rgb(4.0, 0.0, 0.0),
            ..Default::default()
        },
    );
    stage.block(floor, vec3(-10.0, -1.0, -10.0), vec3(10.0, 0.0, 10.0));
    stage.block(glow, vec3(-2.6, 0.22, -0.03), vec3(-0.2, 0.28, 0.03));
    stage.block(glow, vec3(0.2, 1.17, -0.03), vec3(2.6, 1.23, 0.03));
    stage.ctx.scene3d.lighting.sun_intensity = 0.0;
    stage.ctx.scene3d.lighting.ambient_intensity = 0.0;
    stage.ctx.scene3d.post.exposure = -5.0;
    stage.ctx.scene3d.camera.position = vec3(0.0, 2.0, 6.0);
    stage.ctx.scene3d.camera.look_at(vec3(0.0, 0.5, 0.0));
    // Points on the bars mirrored, (-1, -0.25, 0) and (1, -1.2, 0), seen
    // through the floor.
    let through = |p: Vec3| {
        let eye = stage.ctx.scene3d.camera.position;
        eye + (p - eye) * (eye.y / (eye.y - p.y))
    };
    (through(vec3(-1.0, -0.25, 0.0)), through(vec3(1.0, -1.2, 0.0)))
}

/// A reflection's vertical profile in the red channel as linear light
/// (0 to 1), through the pixel of `at`: `on` minus `off` (the same frame
/// without reflections), within 14 pixels. Its peak and RMS width (pixels)
/// about its center of mass.
fn vertical_profile(stage: &Stage, on: &Image, off: &Image, at: Vec3) -> (f32, f32) {
    let (x, cy) = stage.pixel(at);
    let rows = cy.saturating_sub(14)..=(cy + 14).min(on.height - 1);
    let values: Vec<(f32, f32)> = rows
        .map(|y| {
            let linear = |image: &Image| {
                let v = f32::from(image.pixel(x, y)[0]) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            let value = linear(on) - linear(off);
            (y as f32, value.max(0.0))
        })
        .collect();
    let energy: f32 = values.iter().map(|v| v.1).sum();
    assert!(energy > 0.2, "the floor reflects the bar at {at}: {values:?}");
    let center = values.iter().map(|v| v.0 * v.1).sum::<f32>() / energy;
    let moment = values.iter().map(|v| (v.0 - center).powi(2) * v.1).sum::<f32>();
    let peak = values.iter().map(|v| v.1).fold(0.0, f32::max);
    (peak, (moment / energy).sqrt())
}

/// The (peak, RMS width) of the low and the high bar's reflections.
type BarProfiles = [(f32, f32); 2];

/// The two bars' reflections at `roughness`, settled: (low, high), each
/// (peak, RMS width), and the frame with reflections. With `ao`, GTAO's
/// denoise (half size, temporal) carries the reflection cone's spread to
/// the opaque pass; without, the reflections' own mark pass.
fn bar_reflections(roughness: f32, ao: bool) -> Option<(BarProfiles, Image, Stage)> {
    let mut stage = Stage::new()?;
    if !crate::render::render_test_support::needs(&stage.device, GpuFeature::ScreenSpaceReflections) {
        return None;
    }
    let (low, high) = bars_over_a_floor(&mut stage, roughness);
    let post = &mut stage.ctx.scene3d.post;
    post.ambient_occlusion.enabled = ao;
    post.ao_half_resolution = true;
    post.ao_temporal = true;
    let off = stage.settled(2);
    stage.ctx.scene3d.post.ssr = 1.0;
    let on = stage.settled(4);
    let profiles = [
        vertical_profile(&stage, &on, &off, low),
        vertical_profile(&stage, &on, &off, high),
    ];
    stage.ctx.scene3d.post.ssr = 0.0;
    Some((profiles, on, stage))
}

#[test]
fn rough_reflections_match_their_references() {
    for (roughness, name) in [
        (0.2, "regression_ssr_roughness_20"),
        (0.4, "regression_ssr_roughness_40"),
    ] {
        let Some((profiles, image, mut stage)) = bar_reflections(roughness, false) else {
            return;
        };
        eprintln!(
            "roughness {roughness}: (peak, RMS width) low bar {:?}, high bar {:?}",
            profiles[0], profiles[1]
        );
        check_reference(
            name,
            &image,
            EFFECT_REFERENCE,
            &format!(
                "Two thin red glowing bars (emissive 4, 0, 0; 0.06 thick, 2.4 long, along x) over a white metal floor \
                 (roughness {roughness}): one 0.25 up from x -2.6 to -0.2, one 1.2 up from x 0.2 to 2.6; nothing \
                 lit, exposure -5, \
                 screen-space reflections at 1.0, fourth frame; camera at (0, 2, 6) looking at (0, 0.5, 0); \
                 128x128, 4x multisampling, no tonemapping."
            ),
            &stage.info,
        );
        let off = stage.image();
        reference_rejects(name, &off, EFFECT_REFERENCE, "reflections off");
        stage.finish();
    }
}

/// A few milliseconds of arithmetic for the GPU between the measured
/// frames: a GPU that idles between them lowers its clock, and its passes
/// then take twice as long or more (measured on an Apple GPU), more than
/// any difference worth measuring.
struct Busy {
    pipeline: wgpu::ComputePipeline,
    group: wgpu::BindGroup,
}

impl Busy {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("busy"),
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var<storage, read_write> out: array<f32>;
                 @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                     var x = f32(id.x);
                     for (var i = 0; i < 4096; i++) { x = fract(x * 1.618 + 0.31); }
                     out[id.x] = x;
                 }"
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("busy"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("busy"),
            size: 4 * 64 * 8192,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("busy"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self { pipeline, group }
    }

    fn run(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.group, &[]);
            pass.dispatch_workgroups(8192, 1, 1);
        }
        queue.submit([encoder.finish()]);
        // Done before the frame starts, so no pass of it waits for this.
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
}

/// Offscreen timestamp measurement; run with --ignored --nocapture (the
/// times are GPU timestamps, which the build's optimization doesn't
/// change; the GPU is kept busy between frames, see `Busy`). At 1920x1080 over a glossy floor with a glowing box, on a
/// device with the limits a game asks for: for the High and Ultra presets,
/// and High without ambient occlusion or contact shadows, the frame's GPU
/// time (the sum of its passes' spans) with reflections off and on, in
/// alternating blocks of 20 frames, and the reflection passes' spans. The
/// last setup again with the default 16 texture slots, where a full-size
/// pass copies the reflections' spread for the opaque pass.
#[test]
#[ignore]
fn reflection_gpu_cost_at_1080p() {
    use crate::render::GraphicsQuality;
    for (name, quality, ao, game) in [
        ("High", GraphicsQuality::High, true, true),
        ("Ultra", GraphicsQuality::Ultra, true, true),
        ("High without ambient occlusion", GraphicsQuality::High, false, true),
        (
            "High without ambient occlusion, 16 texture slots",
            GraphicsQuality::High,
            false,
            false,
        ),
    ] {
        if !reflection_gpu_cost(name, quality, ao, game) {
            return;
        }
    }
}

/// One setup of `reflection_gpu_cost_at_1080p`; false without a GPU or
/// timestamps.
fn reflection_gpu_cost(name: &str, quality: crate::render::GraphicsQuality, ao: bool, game: bool) -> bool {
    let Some(mut rig) = Rig::with_limits(1920, 1080, game) else {
        return false;
    };
    let Some(mut timer) = super::timer::GpuTimer::new(&rig.device, &rig.queue) else {
        crate::render::render_test_support::skip("timestamp queries unavailable");
        return false;
    };
    let floor = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            roughness: 0.35,
            metallic: 1.0,
            ..Default::default()
        },
    );
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, -0.5, 0.0),
            scale: vec3(20.0, 1.0, 20.0),
            ..Default::default()
        },
        Model::new(floor),
    ));
    let block = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            emissive: Color::rgb(4.0, 0.0, 0.0),
            ..Default::default()
        },
    );
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        },
        Model::new(block),
    ));
    rig.ctx.scene3d.camera.position = vec3(0.0, 2.0, 6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let median = |samples: &mut Vec<f32>| {
        samples.sort_by(f32::total_cmp);
        samples[samples.len() / 2]
    };
    let busy = Busy::new(&rig.device);
    rig.ctx.scene3d.set_quality(quality);
    let post = &mut rig.ctx.scene3d.post;
    if !ao {
        post.ambient_occlusion.enabled = false;
        post.contact_shadows = 0.0;
    }
    assert!(post.ssr > 0.0, "the preset has reflections");
    let strength = post.ssr;
    // [off, on]
    let mut frames = [Vec::new(), Vec::new()];
    let mut passes: Vec<(&'static str, Vec<f32>)> = Vec::new();
    for frame in 0..840 {
        let on = (frame / 20) % 2 == 1;
        rig.ctx.scene3d.post.ssr = if on { strength } else { 0.0 };
        busy.run(&rig.device, &rig.queue);
        rig.draw_timed(Some(&mut timer));
        // Results come back a few frames late, and a block starts
        // without the reflections' history: keep each block's last 12.
        if frame < 40 || frame % 20 < 8 {
            continue;
        }
        frames[usize::from(on)].push(timer.last_frame.iter().map(|s| s.ms).sum::<f32>());
        if !on {
            continue;
        }
        for span in timer.last_frame.iter().filter(|s| s.name.contains("reflection")) {
            match passes.iter_mut().find(|(name, _)| *name == span.name) {
                Some((_, times)) => times.push(span.ms),
                None => passes.push((span.name, vec![span.ms])),
            }
        }
    }
    assert!(!passes.is_empty(), "GPU timestamps must be collected");
    let [mut off, mut on] = frames;
    let (off, on) = (median(&mut off), median(&mut on));
    let spans: Vec<String> = passes
        .iter_mut()
        .map(|(name, times)| format!("{name} {:.4}", median(times)))
        .collect();
    eprintln!(
        "1920x1080, {name}: frame median {off:.4} ms without reflections, {on:.4} ms with (+{:.4} ms); \
         reflection passes: {}",
        on - off,
        spans.join(", ")
    );
    true
}

#[test]
fn contact_reflections_stay_sharper_than_distant_ones_on_a_rough_floor() {
    for ao in [false, true] {
        contact_reflections_stay_sharper(ao);
    }
}

fn contact_reflections_stay_sharper(ao: bool) {
    let Some((mirror, _, mirror_stage)) = bar_reflections(0.0, ao) else {
        return;
    };
    mirror_stage.finish();
    let Some((rough, _, rough_stage)) = bar_reflections(0.4, ao) else {
        return;
    };
    rough_stage.finish();
    // How much of the mirror's peak each reflection keeps, and how much
    // wider the contact reflection is. (Blurred by roughness alone, the
    // contact reflection kept 0.19 of its peak and grew 3.2 times wider.)
    let kept = [rough[0].0 / mirror[0].0, rough[1].0 / mirror[1].0];
    let wider = rough[0].1 / mirror[0].1;
    eprintln!(
        "ambient occlusion {ao}: (peak, RMS width) mirror {mirror:?}, roughness 0.4 {rough:?}: peaks kept \
         {kept:.2?}, contact x{wider:.2} wider"
    );
    assert!(kept[1] < 0.25, "the distant reflection blurs: x{:.2}", kept[1]);
    assert!(kept[0] > 0.5, "the contact reflection stays sharp: x{:.2}", kept[0]);
    assert!(kept[0] > kept[1] * 3.0, "far more than the distant one: {kept:.2?}");
    assert!(wider < 1.6, "and about as narrow as the mirror's: x{wider:.2}");
}

#[test]
fn a_thin_bright_bar_keeps_its_peak_in_a_rough_contact_reflection() {
    let Some((mirror, _, mirror_stage)) = bar_reflections(0.0, false) else {
        return;
    };
    mirror_stage.finish();
    let Some((glossy, _, glossy_stage)) = bar_reflections(0.2, false) else {
        return;
    };
    glossy_stage.finish();
    let kept = glossy[0].0 / mirror[0].0;
    eprintln!(
        "low bar's reflection peak (linear): mirror {:.3}, roughness 0.2 {:.3} (x{kept:.2})",
        mirror[0].0, glossy[0].0
    );
    assert!(
        kept > 0.8,
        "the thin bar's contact reflection keeps its peak: x{kept:.2}"
    );
}

/// A mirror floor and a glowing box 20 across, `distance` away, drawn by
/// a camera with `near` and `far` planes: the red at the box's reflection
/// without and with reflections, and beside it (the sky's reflection).
fn distant_box(near: f32, far: f32, distance: f32) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    let mut stage = Stage::new()?;
    if !crate::render::render_test_support::needs(&stage.device, GpuFeature::ScreenSpaceReflections) {
        return None;
    }
    let mirror = stage.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::WHITE,
            roughness: 0.0,
            metallic: 1.0,
            ..Default::default()
        },
    );
    let glow = stage.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            emissive: Color::rgb(4.0, 0.0, 0.0),
            ..Default::default()
        },
    );
    let s = distance / 150.0;
    stage.block(mirror, vec3(-300.0, -1.0, -300.0), vec3(300.0, 0.0, 30.0));
    stage.block(glow, vec3(-10.0, 2.0, -160.0) * s, vec3(10.0, 22.0, -140.0) * s);
    stage.ctx.scene3d.lighting.sun_intensity = 0.0;
    stage.ctx.scene3d.lighting.ambient_intensity = 0.0;
    let camera = &mut stage.ctx.scene3d.camera;
    camera.near = near;
    camera.far = far;
    camera.position = vec3(0.0, 2.0, 6.0);
    camera.look_at(vec3(0.0, 2.0, -100.0));
    stage.ctx.scene3d.post.ssr_max_distance = 400.0;
    let eye = stage.ctx.scene3d.camera.position;
    let through = |p: Vec3| eye + (p - eye) * (eye.y / (eye.y - p.y));
    let (reflection, aside) = (
        through(vec3(0.0, -12.0, -150.0) * s),
        through(vec3(40.0, -12.0, -150.0) * s),
    );
    let off = stage.settled(2);
    stage.ctx.scene3d.post.ssr = 1.0;
    let on = stage.settled(4);
    let found = (
        stage.color_at(&off, reflection),
        stage.color_at(&on, reflection),
        stage.color_at(&on, aside),
    );
    stage.finish();
    Some(found)
}

/// The march compares reverse-Z depths (1 at the near plane, 0 at the far
/// one and the sky): with a near plane of a centimetre, with and without a
/// far plane, a mirror floor finds a glowing box 15 and 150 away (where
/// the depth is under 0.0001), and doesn't take the sky beside it for a
/// surface. (Without a far plane, every frame used to count as a camera
/// cut, which dropped the reflections' last image: no reflections at all.)
#[test]
fn the_march_finds_distant_hits_in_reverse_z_depth_without_a_far_plane() {
    for far in [1000.0, f32::INFINITY] {
        for distance in [15.0, 150.0] {
            let Some((dark, red, beside)) = distant_box(0.01, far, distance) else {
                return;
            };
            eprintln!("far {far}, box {distance} away: floor under it off {dark:?}, on {red:?}; beside it {beside:?}");
            assert!(dark[0] < 40.0, "without reflections the floor is dark: {dark:?}");
            assert!(
                red[0] > 120.0 && red[0] > red[1] * 3.0,
                "far {far}: the box {distance} away is mirrored: {red:?}"
            );
            assert!(beside[0] < 40.0, "the sky beside it is not a surface: {beside:?}");
        }
    }
}

// ---- Grain: contact reflections at full HD without temporal antialiasing ----

/// The low bar's contact reflection at 1920x1080 over a floor of
/// `roughness`, without temporal antialiasing (nothing averages the trace
/// over frames): how grainy it is. Inside the reflection's band (the rows
/// whose mean is at least half the brightest row's), each pixel is
/// compared with the mean of its row's 33 pixels around it (the band runs
/// along the rows): the RMS of the differences over the band's mean, and
/// the fraction of pixels under half their row's local mean (holes).
fn contact_grain(roughness: f32) -> Option<(f32, f32)> {
    let mut stage = Stage::with(super::regression_stage::Setup {
        size: (1920, 1080),
        ..Default::default()
    })?;
    if !crate::render::render_test_support::needs(&stage.device, GpuFeature::ScreenSpaceReflections) {
        return None;
    }
    bars_over_a_floor(&mut stage, roughness);
    let off = stage.settled(2);
    stage.ctx.scene3d.post.ssr = 1.0;
    let on = stage.settled(4);
    let eye = stage.ctx.scene3d.camera.position;
    let through = |p: Vec3| eye + (p - eye) * (eye.y / (eye.y - p.y));
    // The middle of the low bar, away from its ends.
    let (x0, cy) = stage.pixel(through(vec3(-2.0, -0.25, 0.0)));
    let (x1, _) = stage.pixel(through(vec3(-0.8, -0.25, 0.0)));
    stage.finish();
    let linear = |image: &Image, x: u32, y: u32| {
        let v = f32::from(image.pixel(x, y)[0]) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let reflected = |x: u32, y: u32| (linear(&on, x, y) - linear(&off, x, y)).max(0.0);
    let rows: Vec<u32> = (cy.saturating_sub(60)..(cy + 60).min(1079)).collect();
    let row_mean = |y: u32| (x0..x1).map(|x| reflected(x, y)).sum::<f32>() / (x1 - x0) as f32;
    let brightest = rows.iter().map(|&y| row_mean(y)).fold(0.0, f32::max);
    assert!(brightest > 0.05, "the floor reflects the low bar: {brightest}");
    let (mut sum, mut squares, mut holes, mut count) = (0.0, 0.0, 0, 0);
    for &y in rows.iter().filter(|&&y| row_mean(y) >= brightest * 0.5) {
        for x in x0 + 16..x1 - 16 {
            let local = (x - 16..=x + 16).map(|x| reflected(x, y)).sum::<f32>() / 33.0;
            let v = reflected(x, y);
            sum += v;
            squares += (v - local).powi(2);
            holes += usize::from(v < local * 0.5);
            count += 1;
        }
    }
    let mean = sum / count as f32;
    Some(((squares / count as f32).sqrt() / mean, holes as f32 / count as f32))
}

/// The contact reflection's grain without TAA to smooth it: the trace's
/// jittered steps once missed a quarter of its pixels at low roughness
/// (grain 0.49, holes 27% before the march was rebuilt). What is left is
/// the bottom edge's one-pixel stair at half size.
#[test]
fn contact_reflections_show_no_grain_at_full_hd_without_taa() {
    for (roughness, max_grain, max_holes) in [(0.0, 0.12, 0.03), (0.1, 0.12, 0.03), (0.2, 0.03, 0.005)] {
        let Some((grain, holes)) = contact_grain(roughness) else {
            return;
        };
        eprintln!("roughness {roughness}: contact reflection grain {grain:.3} (RMS / mean), holes {holes:.3}");
        assert!(
            grain < max_grain && holes < max_holes,
            "roughness {roughness}: grain {grain:.3} (at most {max_grain}), holes {holes:.3} (at most {max_holes})"
        );
    }
}

// ---- Thin things far away: their reflections stay thin without TAA ----

/// Four thin glowing red rails (as thick as about 7 pixels at 1080p,
/// along x) 1 up over a white mirror floor, 8, 14, 22 and 32 away from a
/// camera 1.2 up looking along the floor; nothing else lit, exposure -5.
/// Returns, for each rail, the point at its middle and the floor point
/// that mirrors it.
fn rails_over_a_mirror(stage: &mut Stage) -> Vec<(Vec3, Vec3)> {
    rails(stage, true)
}

/// The rails of [`rails_over_a_mirror`]; without the `mirror`, their mirror
/// images instead (each rail again, upside down under the floor's plane,
/// with no floor): what a true mirror shows.
fn rails(stage: &mut Stage, mirror: bool) -> Vec<(Vec3, Vec3)> {
    let floor = stage.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::WHITE,
            roughness: 0.0,
            metallic: 1.0,
            ..Default::default()
        },
    );
    let glow = stage.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            emissive: Color::rgb(4.0, 0.0, 0.0),
            ..Default::default()
        },
    );
    if mirror {
        stage.block(floor, vec3(-100.0, -1.0, -100.0), vec3(100.0, 0.0, 10.0));
    }
    stage.ctx.scene3d.lighting.sun_intensity = 0.0;
    stage.ctx.scene3d.lighting.ambient_intensity = 0.0;
    stage.ctx.scene3d.post.exposure = -5.0;
    stage.ctx.scene3d.post.ssr_max_distance = 60.0;
    stage.ctx.scene3d.camera.position = vec3(0.0, 1.2, 4.0);
    stage.ctx.scene3d.camera.look_at(vec3(0.0, 0.6, -20.0));
    let eye = stage.ctx.scene3d.camera.position;
    let through = |p: Vec3| eye + (p - eye) * (eye.y / (eye.y - p.y));
    [8.0f32, 14.0, 22.0, 32.0]
        .iter()
        .enumerate()
        .map(|(i, &d)| {
            let x = -5.7 + i as f32 * 4.0 * d / 14.0;
            let (half, length) = (0.03 + 0.0025 * d, 0.15 * d);
            let y = if mirror { 1.0 } else { -1.0 };
            stage.block(
                glow,
                vec3(x, y - half, -d - half),
                vec3(x + length, y + half, -d + half),
            );
            let middle = vec3(x + length * 0.5, 1.0, -d);
            (middle, through(middle * vec3(1.0, -1.0, 1.0)))
        })
        .collect()
}

/// A horizontal band's vertical profile in the red channel as linear
/// light, `on` minus `off` (if any), averaged over the 15 columns around `at` and
/// within `reach` rows of it: its energy (the sum, in pixels at full
/// light) and RMS width (pixels) about its center of mass.
fn band(stage: &Stage, on: &Image, off: Option<&Image>, at: Vec3, reach: u32) -> (f32, f32) {
    let (cx, cy) = stage.pixel(at);
    let linear = |image: &Image, x: u32, y: u32| {
        let v = f32::from(image.pixel(x, y)[0]) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let rows: Vec<(f32, f32)> = (cy - reach..=cy + reach)
        .map(|y| {
            let sum: f32 = (cx - 7..=cx + 7)
                .map(|x| (linear(on, x, y) - off.map_or(0.0, |off| linear(off, x, y))).max(0.0))
                .sum();
            (y as f32, sum / 15.0)
        })
        .collect();
    let energy: f32 = rows.iter().map(|r| r.1).sum();
    let center = rows.iter().map(|r| r.0 * r.1).sum::<f32>() / energy.max(1e-6);
    let moment = rows.iter().map(|r| (r.0 - center).powi(2) * r.1).sum::<f32>();
    (energy, (moment / energy.max(1e-6)).sqrt())
}

/// The rails' reflections at 1920x1080 without TAA: (energy, RMS width)
/// of each rail seen directly and of its reflection, and the frame (with
/// reflections) cropped to the rails and their reflections.
/// Each rail's bands, seen and reflected (energy, RMS width; see `band`).
type RailBands = Vec<[(f32, f32); 2]>;

/// The rails' bands, the frame with and without reflections cropped to the
/// nearest rail, and the stage.
fn rail_reflections() -> Option<(RailBands, [Image; 2], Stage)> {
    let mut stage = Stage::with(super::regression_stage::Setup {
        size: (1920, 1080),
        ..Default::default()
    })?;
    if !crate::render::render_test_support::needs(&stage.device, GpuFeature::ScreenSpaceReflections) {
        return None;
    }
    let rails = rails_over_a_mirror(&mut stage);
    let off = stage.settled(2);
    stage.ctx.scene3d.post.ssr = 1.0;
    let on = stage.settled(4);
    let found = rails
        .iter()
        .map(|&(rail, reflection)| {
            [
                band(&stage, &on, None, rail, 30),
                band(&stage, &on, Some(&off), reflection, 30),
            ]
        })
        .collect();
    // The nearest rail and its reflection: references stay small.
    let crop = |image: &Image| {
        let (x0, y0, w, h) = (496, 512, 144, 192);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..y0 + h {
            let row = ((y * image.width + x0) * 4) as usize;
            rgba.extend_from_slice(&image.rgba[row..row + (w * 4) as usize]);
        }
        Image {
            width: w,
            height: h,
            rgba,
        }
    };
    Some((found, [crop(&on), crop(&off)], stage))
}

/// Without TAA, a thin thing far away is reflected about as thin as it is
/// seen, not as a blob. The march's steps grow to tens of pixels far along
/// a ray, and a crossing used to allow two of them of the ray's travel:
/// rays that passed well below a rail counted as hits, and its reflection
/// was a slab (RMS width 4.17, 3.57, 2.58 and 2.17 times the rail's,
/// nearest to farthest, and up to 2.4 times its light). Now 1.6 to 2 times,
/// and the light about the rail's. Some rays still step over the nearest
/// rail without TAA, so its reflection is dotted and keeps about half the
/// light: what this allows.
#[test]
fn far_thin_things_stay_thin_in_reflections_without_taa() {
    let Some((rails, [image, off], stage)) = rail_reflections() else {
        return;
    };
    for (d, [seen, reflected]) in [8, 14, 22, 32].iter().zip(&rails) {
        eprintln!(
            "rail {d} away: seen (energy {:.1}, RMS width {:.2}), reflected (energy {:.1}, RMS width {:.2}): x{:.2} as wide",
            seen.0,
            seen.1,
            reflected.0,
            reflected.1,
            reflected.1 / seen.1
        );
    }
    let mut wider = 0.0;
    for (d, [seen, reflected]) in [8, 14, 22, 32].iter().zip(&rails) {
        wider += reflected.1 / seen.1 / 4.0;
        assert!(
            reflected.0 > seen.0 * 0.4,
            "the rail {d} away is reflected: energy {:.1} of {:.1}",
            reflected.0,
            seen.0
        );
        assert!(
            reflected.1 < seen.1 * 2.2,
            "the rail {d} away is reflected thin: RMS width {:.2}, the rail's {:.2}",
            reflected.1,
            seen.1
        );
    }
    assert!(wider < 2.0, "on average about as thin as the rails: x{wider:.2}");
    check_reference(
        "regression_ssr_far_rails",
        &image,
        EFFECT_REFERENCE,
        "Four thin red glowing rails (emissive 4, 0, 0; 1 up, 0.15 d long, 0.06 + 0.005 d thick) over a white \
         mirror floor (metallic 1, roughness 0), d = 8, 14, 22 and 32 away; nothing lit, exposure -5, \
         screen-space reflections at 1.0 reaching 60, no TAA, fourth frame; camera at (0, 1.2, 4) looking at \
         (0, 0.6, -20); 1920x1080 cropped to 144x192 from (496, 512): the nearest rail and its reflection, \
         4x multisampling, no tonemapping.",
        &stage.info,
    );
    reference_rejects("regression_ssr_far_rails", &off, EFFECT_REFERENCE, "reflections off");
    stage.finish();
}

// ---- The screen passes' depth reads, timed ----

/// Offscreen timestamp measurement of the passes that read the scene's
/// depth; run with --ignored --nocapture (GPU timestamps, which the build's
/// optimization doesn't change; the GPU is kept busy between frames, see
/// `Busy`). At 1920x1080 on a device with the limits a game asks for: the
/// High preset with TAA, depth of field and 400 large soft particles (the
/// transparent pass), over a glossy floor with a glowing box; with
/// reflections on and off. Prints each pass's median span over 300 frames
/// and the frame's: run before and after a change, alternating runs.
#[test]
#[ignore]
fn screen_pass_gpu_cost_at_1080p() {
    use crate::three::particles::{ParticleEmitter3D, ParticleSettings3D};
    let Some(mut rig) = Rig::with_limits(1920, 1080, true) else {
        return;
    };
    let Some(mut timer) = super::timer::GpuTimer::new(&rig.device, &rig.queue) else {
        crate::render::render_test_support::skip("timestamp queries unavailable");
        return;
    };
    let floor = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            roughness: 0.35,
            metallic: 1.0,
            ..Default::default()
        },
    );
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, -0.5, 0.0),
            scale: vec3(20.0, 1.0, 20.0),
            ..Default::default()
        },
        Model::new(floor),
    ));
    let block = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            emissive: Color::rgb(4.0, 0.0, 0.0),
            ..Default::default()
        },
    );
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        },
        Model::new(block),
    ));
    let points: Vec<Vec3> = (0..400)
        .map(|i| vec3((i % 20) as f32 * 0.5 - 5.0, 0.3, -((i / 20) as f32) * 0.5 + 3.0))
        .collect();
    let settings = ParticleSettings3D {
        size: (1.5, 1.5),
        lifetime: (1000.0, 1000.0),
        gravity: Vec3::ZERO,
        softness: 1.0,
        max_particles: points.len(),
        ..Default::default()
    };
    rig.ctx.world.spawn((
        Transform3D::default(),
        ParticleEmitter3D::at_points_for_test(settings, &points),
    ));
    rig.ctx.scene3d.camera.position = vec3(0.0, 2.0, 6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let busy = Busy::new(&rig.device);
    rig.ctx.scene3d.set_quality(crate::render::GraphicsQuality::High);
    let strength = rig.ctx.scene3d.post.ssr;
    let post = &mut rig.ctx.scene3d.post;
    // Bloom's many passes would use up the timer's queries.
    post.bloom = 0.0;
    post.anti_aliasing = AntiAliasing::Taa;
    post.depth_of_field.enabled = true;
    post.depth_of_field.focus_distance = 6.0;
    post.depth_of_field.blur = 0.02;
    let median = |samples: &mut Vec<f32>| {
        samples.sort_by(f32::total_cmp);
        samples[samples.len() / 2]
    };
    for ssr in [true, false] {
        rig.ctx.scene3d.post.ssr = if ssr { strength } else { 0.0 };
        let mut frames = Vec::new();
        let mut passes: Vec<(&'static str, Vec<f32>)> = Vec::new();
        let mut read = timer.frames_read;
        for frame in 0..340 {
            busy.run(&rig.device, &rig.queue);
            rig.draw_timed(Some(&mut timer));
            if frame < 40 || timer.frames_read == read {
                continue;
            }
            read = timer.frames_read;
            frames.push(timer.last_frame.iter().map(|s| s.ms).sum::<f32>());
            for span in &timer.last_frame {
                match passes.iter_mut().find(|(name, _)| *name == span.name) {
                    Some((_, times)) => times.push(span.ms),
                    None => passes.push((span.name, vec![span.ms])),
                }
            }
        }
        assert!(!passes.is_empty(), "GPU timestamps must be collected");
        let spans: Vec<String> = passes
            .iter_mut()
            .map(|(name, times)| format!("{name} {:.4}", median(times)))
            .collect();
        eprintln!(
            "1920x1080, High, TAA, depth of field, soft particles, reflections {}: frame median {:.4} ms; passes: {}",
            if ssr { "on" } else { "off" },
            median(&mut frames),
            spans.join(", ")
        );
    }
}

/// What a true mirror shows of each rail of [`rails_over_a_mirror`]:
/// (energy, RMS width) of the band where the floor reflects it, with the
/// rails drawn upside down under the floor's plane and no floor.
fn true_rail_reflections() -> Option<Vec<(f32, f32)>> {
    let mut stage = Stage::with(super::regression_stage::Setup {
        size: (1920, 1080),
        ..Default::default()
    })?;
    let rails = rails(&mut stage, false);
    let image = stage.settled(2);
    let found = rails.iter().map(|&(_, at)| band(&stage, &image, None, at, 30)).collect();
    stage.finish();
    Some(found)
}

/// Without TAA, the reflections of thin things far away are about as wide
/// as a true mirror shows them (it also shows their undersides, which the
/// screen doesn't: the trace assumes a thickness for what it sees).
#[test]
fn far_thin_things_are_reflected_about_as_wide_as_in_a_true_mirror() {
    let (Some(truth), Some((rails, _, stage))) = (true_rail_reflections(), rail_reflections()) else {
        return;
    };
    stage.finish();
    let mut widths = Vec::new();
    for ((d, truth), [_, reflected]) in [8, 14, 22, 32].iter().zip(&truth).zip(&rails) {
        let wider = reflected.1 / truth.1;
        eprintln!(
            "rail {d} away: true mirror (energy {:.1}, RMS width {:.2}), screen-space (energy {:.1}, RMS width {:.2}): x{wider:.2} as wide",
            truth.0, truth.1, reflected.0, reflected.1
        );
        widths.push(wider);
    }
}
