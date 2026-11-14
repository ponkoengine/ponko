//! The game's custom passes at their named stages: target effects (an
//! effect shader run over the lit 3D frame that writes a render target of
//! its own, after the transparent surfaces, before screen effects), compute
//! passes before the scene and scene compute passes after it, and the
//! hazard check over what they all write (`gpu3d/custom_passes.rs`).
//! Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{MaterialDesc, Mesh, MeshData, Model};
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
    /// Screen effects to run: (effect, material).
    effects: Vec<(u32, u32)>,
    /// Target effects to run: (effect, render target id, material).
    target_effects: Vec<(u32, u32, u32)>,
    /// Texture compute passes to run: (compute, render target id, material).
    computes: Vec<(u32, u32, u32)>,
    /// Scene compute passes to run, as `computes`.
    scene_computes: Vec<(u32, u32, u32)>,
    /// Materials made on the GPU so far (ids follow this count).
    materials: u32,
    /// The bind group layout of sprite textures (for render targets).
    layout: wgpu::BindGroupLayout,
}

impl Rig {
    /// `None` without a GPU.
    fn new() -> Option<Self> {
        Self::with_limits(wgpu::Limits::default())
    }

    /// A rig whose device asks for `limits` (WebGL2's: no compute shaders).
    fn with_limits(limits: wgpu::Limits) -> Option<Self> {
        Self::build(limits, 4)
    }

    /// A rig drawing with `samples` per pixel.
    fn with_samples(samples: u32) -> Option<Self> {
        Self::build(wgpu::Limits::default(), samples)
    }

    fn build(limits: wgpu::Limits, samples: u32) -> Option<Self> {
        let gpu = crate::render::render_test_support::gpu_with(move |_| {
            Ok(wgpu::DeviceDescriptor {
                required_limits: limits,
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
                width: SIZE,
                height: SIZE,
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
        let renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, samples, true);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
        ctx.scene3d.sky.atmosphere = false;
        Some(Self {
            device,
            queue,
            output,
            target,
            textures,
            renderer,
            ctx,
            effects: Vec::new(),
            target_effects: Vec::new(),
            computes: Vec::new(),
            scene_computes: Vec::new(),
            materials: 0,
            layout,
        })
    }

    /// A mesh of `data` with `desc`, on the CPU and the GPU.
    fn mesh(&mut self, data: &MeshData, desc: MaterialDesc) -> Mesh {
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.gpu_material(desc);
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// A material on the GPU only (for effects), returning its id.
    fn gpu_material(&mut self, desc: MaterialDesc) -> u32 {
        let id = self
            .renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        assert_eq!(id, self.materials);
        self.materials += 1;
        id
    }

    /// A `width` x `height` render target, made as the engine makes them:
    /// a texture id and a texture the renderer knows as a render target.
    fn render_target(&mut self, width: u32, height: u32) -> (Texture, wgpu::Texture) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let sampler = self.device.create_sampler(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
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
        });
        let id = self.textures.len() as u32;
        self.renderer.add_render_target(id, &texture);
        self.textures.push(GpuTexture {
            texture: texture.clone(),
            nearest: false,
            mips: Some(false),
            bind_group,
        });
        (Texture::new(id, width, height), texture)
    }

    /// Reads a render target back: RGB per texel, row by row from the top.
    fn read_target(&self, texture: &wgpu::Texture) -> Vec<[f32; 3]> {
        let size = texture.size();
        let row = (size.width * 8).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let bytes = readback.get_mapped_range(..).unwrap().to_vec();
        let mut texels = Vec::with_capacity((size.width * size.height) as usize);
        for y in 0..size.height {
            for x in 0..size.width {
                let at = (y * row + x * 8) as usize;
                let channel = |c: usize| half::f16::from_le_bytes([bytes[at + c * 2], bytes[at + c * 2 + 1]]).to_f32();
                texels.push([channel(0), channel(1), channel(2)]);
            }
        }
        texels
    }

    /// Draws the world and reads the pixels back (RGBA).
    fn draw(&mut self) -> Vec<u8> {
        let ctx = &mut self.ctx;
        let mut frame = Frame3D::default();
        frame.lod = ctx.scene3d.lod;
        frame.effects.clone_from(&self.effects);
        frame.target_effects.clone_from(&self.target_effects);
        frame.computes.clone_from(&self.computes);
        frame.scene_computes.clone_from(&self.scene_computes);
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
            // Sunlight only.
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
                size: (SIZE, SIZE),
            },
        );
        let buffers = self
            .renderer
            .draw(&self.device, &self.target, wgpu::Color::BLACK, &self.textures, None);
        self.queue.submit(buffers);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
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
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: None,
                },
            },
            self.output.size(),
        );
        self.queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        readback.get_mapped_range(..).unwrap().to_vec()
    }
}

/// A ground block under one half of the view (the right one on screen),
/// sky (nothing) under the other, seen from straight above.
fn half_ground(rig: &mut Rig) {
    let ground = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(-10.0, -0.5, 0.0),
            scale: vec3(20.0, 1.0, 40.0),
            ..Default::default()
        },
        Model::new(ground),
    ));
    rig.ctx.scene3d.camera.position = vec3(0.0, 12.0, 0.01);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
}

/// Whether the device reported a validation error while running `f`.
fn validation_error(rig: &mut Rig, f: impl FnOnce(&mut Rig)) -> Option<wgpu::Error> {
    let scope = rig.device.push_error_scope(wgpu::ErrorFilter::Validation);
    f(rig);
    pollster::block_on(scope.pop())
}

#[test]
fn target_effects_write_their_target_and_leave_the_frame_alone() {
    let Some(mut rig) = Rig::new() else { return };
    half_ground(&mut rig);
    let before = rig.draw();
    // A target of another size and shape than the frame: each texel reads
    // the frame at the matching point.
    let (target, texture) = rig.render_target(32, 16);
    let effect = rig
        .renderer
        .create_effect(
            &rig.device,
            "mark",
            "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(select(1.0, 0.0, p.sky), p.uv.x, p.uv.y); }",
        )
        .unwrap();
    rig.target_effects.push((effect, target.id, 0));
    let mut after = Vec::new();
    assert!(validation_error(&mut rig, |rig| after = rig.draw()).is_none());
    assert_eq!(rig.renderer.target_effect_count(), 1);
    assert!(before == after, "the frame is unchanged");
    let texels = rig.read_target(&texture);
    for y in 0..16 {
        for x in 0..32 {
            let [ground, u, v] = texels[(y * 32 + x) as usize];
            // The frame pixel under the texel's centre, and its centre in uv.
            // A centre on the line between two pixels (here every one: 4 frame
            // pixels a texel) may read either.
            let under = |t: u32, n: u32| ((t as f32 + 0.5) * SIZE as f32 / n as f32).floor() + 0.5;
            let (px, py) = (under(x, 32), under(y, 16));
            let near = 1.0 / SIZE as f32 + 2e-3;
            assert!((u - px / SIZE as f32).abs() < near, "texel {x},{y}: u {u}");
            assert!((v - py / SIZE as f32).abs() < near, "texel {x},{y}: v {v}");
            // Away from the edge between ground and sky.
            if x < 14 {
                assert_eq!(ground, 0.0, "texel {x},{y} sees the sky");
            } else if x > 17 {
                assert_eq!(ground, 1.0, "texel {x},{y} sees the ground");
            }
        }
    }
}

#[test]
fn screen_effects_see_this_frames_target_effects() {
    let Some(mut rig) = Rig::new() else { return };
    half_ground(&mut rig);
    let (target, _) = rig.render_target(4, 4);
    let fill = rig
        .renderer
        .create_effect(
            &rig.device,
            "fill",
            "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(0.0, 1.0, 0.0); }",
        )
        .unwrap();
    // A screen effect showing what the target holds, everywhere.
    let show = rig
        .renderer
        .create_effect(
            &rig.device,
            "show",
            "fn effect(p: Pixel) -> vec3<f32> { return textureLoad(custom_texture0, vec2<i32>(1, 1), 0).rgb; }",
        )
        .unwrap();
    let reads_target = rig.gpu_material(MaterialDesc {
        custom_textures: [Some(target), None],
        ..Default::default()
    });
    rig.effects.push((show, reads_target));
    // Without the target effect the target holds 0: a black frame.
    let black = rig.draw();
    assert!(black.chunks(4).all(|p| p[..3] == [0, 0, 0]), "the target starts at 0");
    rig.target_effects.push((fill, target.id, 0));
    let shown = rig.draw();
    let centre = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    let [r, g, b] = [shown[centre], shown[centre + 1], shown[centre + 2]];
    assert!(g > 150 && r < 20 && b < 20, "this frame's green: {r} {g} {b}");
}

#[test]
fn target_effects_sampling_their_own_target_are_skipped() {
    let Some(mut rig) = Rig::new() else { return };
    half_ground(&mut rig);
    let before = rig.draw();
    let (target, texture) = rig.render_target(8, 8);
    let fill = rig
        .renderer
        .create_effect(
            &rig.device,
            "fill",
            "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(1.0); }",
        )
        .unwrap();
    let reads_itself = rig.gpu_material(MaterialDesc {
        custom_textures: [None, Some(target)],
        ..Default::default()
    });
    rig.target_effects.push((fill, target.id, reads_itself));
    let mut after = Vec::new();
    assert!(
        validation_error(&mut rig, |rig| after = rig.draw()).is_none(),
        "no read-write hazard reaches the GPU"
    );
    assert_eq!(rig.renderer.target_effect_count(), 0, "skipped");
    assert!(before == after);
    assert!(
        rig.read_target(&texture).iter().all(|t| *t == [0.0; 3]),
        "never written"
    );
}

#[test]
fn target_effects_follow_hot_reloaded_effects() {
    let Some(mut rig) = Rig::new() else { return };
    half_ground(&mut rig);
    let (target, texture) = rig.render_target(4, 4);
    let effect = rig
        .renderer
        .create_effect(
            &rig.device,
            "e",
            "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(0.25); }",
        )
        .unwrap();
    rig.target_effects.push((effect, target.id, 0));
    rig.draw();
    assert_eq!(rig.read_target(&texture)[0], [0.25; 3]);
    rig.renderer
        .replace_effect(
            &rig.device,
            effect,
            "e",
            "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(0.75); }",
        )
        .unwrap();
    rig.draw();
    assert_eq!(rig.read_target(&texture)[0], [0.75; 3]);
}

/// The pixel in the middle of a drawn frame.
fn centre(frame: &[u8]) -> [u8; 3] {
    let at = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    [frame[at], frame[at + 1], frame[at + 2]]
}

#[test]
fn two_screen_effects_run_in_order_swapping_between_two_images() {
    let Some(mut rig) = Rig::new() else { return };
    half_ground(&mut rig);
    let effect = |rig: &mut Rig, name, source| rig.renderer.create_effect(&rig.device, name, source).unwrap();
    // The first leaves red everywhere; the second moves red into green.
    let red = effect(
        &mut rig,
        "red",
        "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(1.0, 0.0, 0.0); }",
    );
    let turn = effect(
        &mut rig,
        "turn",
        "fn effect(p: Pixel) -> vec3<f32> { return p.color.grb; }",
    );
    let material = rig.gpu_material(MaterialDesc::default());
    rig.effects.push((red, material));
    let [r, g, b] = centre(&rig.draw());
    assert!(r > 150 && g < 20 && b < 20, "one effect: red: {r} {g} {b}");
    let one = last_features().unwrap();
    assert!(one.contains(FrameFeatures::EFFECTS.with(FrameFeatures::EFFECTS_ODD)));
    assert!(!one.contains(FrameFeatures::EFFECTS_BACK));
    // The second reads what the first wrote and writes back into the lit
    // frame, where bloom and tone mapping then read it.
    rig.effects.push((turn, material));
    let [r, g, b] = centre(&rig.draw());
    assert!(g > 150 && r < 20 && b < 20, "two effects: green: {r} {g} {b}");
    let two = last_features().unwrap();
    assert!(two.contains(FrameFeatures::EFFECTS.with(FrameFeatures::EFFECTS_BACK)));
    assert!(!two.contains(FrameFeatures::EFFECTS_ODD));
    // A third ends in the other image again.
    rig.effects.push((turn, material));
    let [r, g, b] = centre(&rig.draw());
    assert!(r > 150 && g < 20 && b < 20, "three effects: red again: {r} {g} {b}");
    let three = FrameFeatures::EFFECTS_BACK.with(FrameFeatures::EFFECTS_ODD);
    assert!(last_features().unwrap().contains(three));
}

#[test]
fn depth_of_field_after_an_odd_number_of_screen_effects_writes_back_into_the_lit_frame() {
    let Some(mut rig) = Rig::new() else { return };
    half_ground(&mut rig);
    let red = rig
        .renderer
        .create_effect(
            &rig.device,
            "red",
            "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(1.0, 0.0, 0.0); }",
        )
        .unwrap();
    let material = rig.gpu_material(MaterialDesc::default());
    rig.effects.push((red, material));
    let dof = &mut rig.ctx.scene3d.post.depth_of_field;
    dof.enabled = true;
    dof.blur = 0.05;
    // The effect leaves red in the scratch; depth of field reads it there
    // and writes the lit frame, which tone mapping then reads: still red.
    let [r, g, b] = centre(&rig.draw());
    assert!(r > 150 && g < 20 && b < 20, "red through depth of field: {r} {g} {b}");
    let features = last_features().unwrap();
    assert!(features.contains(FrameFeatures::DOF.with(FrameFeatures::EFFECTS_ODD)));
    // With two effects it reads the lit frame and ends in the scratch.
    rig.effects.push((red, material));
    let [r, g, b] = centre(&rig.draw());
    assert!(
        r > 150 && g < 20 && b < 20,
        "two effects, then depth of field: {r} {g} {b}"
    );
    assert!(!last_features().unwrap().contains(FrameFeatures::EFFECTS_ODD));
}

#[test]
fn a_texture_compute_pass_fills_its_target_before_the_frame_reads_it() {
    let Some(mut rig) = Rig::new() else { return };
    if rig.device.limits().max_storage_textures_per_shader_stage == 0 {
        return;
    }
    half_ground(&mut rig);
    let (target, texture) = rig.render_target(4, 4);
    let fill = rig
        .renderer
        .create_compute(
            &rig.device,
            "fill",
            "fn compute(c: Cell) -> vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }",
        )
        .unwrap();
    let show = rig
        .renderer
        .create_effect(
            &rig.device,
            "show",
            "fn effect(p: Pixel) -> vec3<f32> { return textureLoad(custom_texture0, vec2<i32>(1, 1), 0).rgb; }",
        )
        .unwrap();
    let reads_target = rig.gpu_material(MaterialDesc {
        custom_textures: [Some(target), None],
        ..Default::default()
    });
    let plain = rig.gpu_material(MaterialDesc::default());
    rig.effects.push((show, reads_target));
    rig.computes.push((fill, target.id, plain));
    let [r, g, b] = centre(&rig.draw());
    assert!(last_features().unwrap().contains(FrameFeatures::TEXTURE_COMPUTE));
    assert!(
        g > 150 && r < 20 && b < 20,
        "the frame shows this frame's fill: {r} {g} {b}"
    );
    let texels = rig.read_target(&texture);
    assert!(texels.iter().all(|t| t[1] > 0.9 && t[0] < 0.1), "{:?}", texels[0]);
}

/// Whether this device runs compute passes; skips the test (with the
/// reason) when it doesn't.
fn computes(rig: &Rig) -> bool {
    crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::Compute)
}

/// A compute pass writing, at each texel, how far the scene is under the
/// matching frame pixel (red) and how lit it is there (green).
const SCENE_PROBE: &str = "fn compute(c: Cell) -> vec4<f32> {
    let at = (vec2<f32>(c.pixel) + 0.5) / vec2<f32>(c.size) * frame.screen.xy;
    return vec4<f32>(scene_distance(at), scene_color(at).r, 0.0, 1.0);
}";

#[test]
fn scene_compute_passes_read_this_frames_scene_and_earlier_compute_passes_do_not() {
    scene_and_early_probes(Rig::new());
}

#[test]
fn scene_compute_passes_read_the_scene_without_multisampling() {
    // The scene depth is then an ordinary depth texture: the shader is
    // built for it.
    scene_and_early_probes(Rig::with_samples(1));
}

fn scene_and_early_probes(rig: Option<Rig>) {
    let Some(mut rig) = rig else { return };
    if !computes(&rig) {
        return;
    }
    half_ground(&mut rig);
    let before = rig.draw();
    let probe = rig.renderer.create_compute(&rig.device, "probe", SCENE_PROBE).unwrap();
    let plain = rig.gpu_material(MaterialDesc::default());
    let (scene_target, scene_texture) = rig.render_target(32, 16);
    let (early_target, early_texture) = rig.render_target(32, 16);
    // The same shader at both stages.
    rig.scene_computes.push((probe, scene_target.id, plain));
    rig.computes.push((probe, early_target.id, plain));
    let mut after = Vec::new();
    assert!(validation_error(&mut rig, |rig| after = rig.draw()).is_none());
    let features = last_features().unwrap();
    assert!(
        features.contains(FrameFeatures::SCENE_TEXTURE_COMPUTE.with(FrameFeatures::TEXTURE_COMPUTE)),
        "{features}"
    );
    assert_eq!(rig.renderer.custom_pass_state().0, 1);
    assert!(before == after, "a compute pass leaves the frame alone");
    let scene = rig.read_target(&scene_texture);
    let early = rig.read_target(&early_texture);
    for y in 0..16 {
        for x in 0..32 {
            let [distance, lit, _] = scene[(y * 32 + x) as usize];
            // Away from the edge between ground and sky. The camera is 12
            // above the ground; the sky is as far as the camera sees.
            if x > 17 {
                assert!((11.0..16.0).contains(&distance), "texel {x},{y}: ground at {distance}");
                assert!(lit > 0.01, "texel {x},{y}: the lit ground: {lit}");
            } else if x < 14 {
                // As far as the camera sees: large, or not a number.
                assert!(
                    distance.is_nan() || distance >= 100.0,
                    "texel {x},{y}: sky at {distance}"
                );
            }
            // Negative control: before the scene is drawn, the same shader
            // sees the 1x1 stand-in (black, one depth everywhere), never
            // the ground.
            let [distance, lit, _] = early[(y * 32 + x) as usize];
            assert!(
                !(11.0..16.0).contains(&distance),
                "texel {x},{y}: no scene yet, but {distance}"
            );
            assert_eq!(lit, 0.0, "texel {x},{y}");
        }
    }
}

/// A ground block filling the view whose light is `target`'s first texel
/// (its emissive map; black otherwise), seen from straight above.
fn ground_lit_by(rig: &mut Rig, target: Texture) {
    let ground = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            color: Color::BLACK,
            emissive: Color::WHITE,
            emissive_map: Some(target),
            ..Default::default()
        },
    );
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, -0.5, 0.0),
            scale: vec3(40.0, 1.0, 40.0),
            ..Default::default()
        },
        Model::new(ground),
    ));
    rig.ctx.scene3d.camera.position = vec3(0.0, 12.0, 0.01);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
}

#[test]
fn materials_see_last_frames_scene_compute_results_and_effects_this_frames() {
    let Some(mut rig) = Rig::new() else { return };
    if !computes(&rig) {
        return;
    }
    let (target, _) = rig.render_target(1, 1);
    ground_lit_by(&mut rig, target);
    let green = rig
        .renderer
        .create_compute(
            &rig.device,
            "green",
            "fn compute(c: Cell) -> vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }",
        )
        .unwrap();
    let plain = rig.gpu_material(MaterialDesc::default());
    rig.scene_computes.push((green, target.id, plain));
    // The opaque pass ran before the scene stage: the target still held 0.
    let [r, g, b] = centre(&rig.draw());
    assert!(
        g < r + 20 && g < b + 20,
        "first frame: last frame's 0, no green: {r} {g} {b}"
    );
    let [r, g, b] = centre(&rig.draw());
    assert!(
        g > 150 && g > r + 100 && g > b + 100,
        "second frame: the first frame's green: {r} {g} {b}"
    );
    // A screen effect showing the target sees this frame's result.
    let (fresh, _) = rig.render_target(1, 1);
    let show = rig
        .renderer
        .create_effect(
            &rig.device,
            "show",
            "fn effect(p: Pixel) -> vec3<f32> { return textureLoad(custom_texture0, vec2<i32>(0, 0), 0).rgb; }",
        )
        .unwrap();
    let reads_fresh = rig.gpu_material(MaterialDesc {
        custom_textures: [Some(fresh), None],
        ..Default::default()
    });
    rig.scene_computes.push((green, fresh.id, plain));
    rig.effects.push((show, reads_fresh));
    let [r, g, b] = centre(&rig.draw());
    assert!(
        g > 150 && g > r + 100 && g > b + 100,
        "the effect shows this frame's green: {r} {g} {b}"
    );
}

#[test]
fn materials_see_this_frames_compute_results_before_the_scene() {
    // Negative control for the test above: the stage decides what the
    // opaque pass sees.
    let Some(mut rig) = Rig::new() else { return };
    if !computes(&rig) {
        return;
    }
    let (target, _) = rig.render_target(1, 1);
    ground_lit_by(&mut rig, target);
    let green = rig
        .renderer
        .create_compute(
            &rig.device,
            "green",
            "fn compute(c: Cell) -> vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }",
        )
        .unwrap();
    let plain = rig.gpu_material(MaterialDesc::default());
    rig.computes.push((green, target.id, plain));
    let [r, g, b] = centre(&rig.draw());
    assert!(
        g > 150 && g > r + 100 && g > b + 100,
        "first frame: this frame's green: {r} {g} {b}"
    );
}

#[test]
fn a_target_effect_clearing_a_scene_compute_target_is_skipped_and_named() {
    let Some(mut rig) = Rig::new() else { return };
    if !computes(&rig) {
        return;
    }
    half_ground(&mut rig);
    let (shared, shared_texture) = rig.render_target(4, 4);
    let (own, own_texture) = rig.render_target(4, 4);
    let add = rig
        .renderer
        .create_compute(
            &rig.device,
            "add",
            "fn compute(c: Cell) -> vec4<f32> { return c.value + vec4<f32>(0.25, 0.0, 0.0, 0.0); }",
        )
        .unwrap();
    let fill = rig
        .renderer
        .create_effect(
            &rig.device,
            "fill",
            "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(1.0); }",
        )
        .unwrap();
    let plain = rig.gpu_material(MaterialDesc::default());
    rig.scene_computes.push((add, shared.id, plain));
    // Negative control: a target effect into a target of its own runs.
    rig.target_effects.push((fill, own.id, plain));
    rig.draw();
    assert_eq!(rig.renderer.target_effect_count(), 1);
    assert!(rig.renderer.custom_pass_state().2.is_empty());
    assert_eq!(rig.read_target(&own_texture)[0], [1.0; 3]);
    // The same target effect into the scene compute pass's target would
    // replace what that pass accumulates: it is left out, by name.
    rig.target_effects.push((fill, shared.id, plain));
    assert!(validation_error(&mut rig, |rig| drop(rig.draw())).is_none());
    assert_eq!(rig.renderer.target_effect_count(), 1, "only the first runs");
    let (scene_passes, checked, hazards) = rig.renderer.custom_pass_state();
    assert_eq!(scene_passes, 1, "the scene compute pass still runs");
    assert!(checked >= 2);
    assert_eq!(hazards.len(), 1, "{hazards:?}");
    let named = &hazards[0];
    assert!(
        named.contains("target effect 1 (ctx.scene3d.passes.target_effects")
            && named.contains(&format!("render target {}", shared.id))
            && named.contains("scene compute pass 0 (ctx.scene3d.passes.scene_compute"),
        "{named}"
    );
    // The scene compute pass kept accumulating: 0.25 a frame, two frames.
    let [red, ..] = rig.read_target(&shared_texture)[0];
    assert!((red - 0.5).abs() < 1e-3, "{red}");
}

#[test]
fn a_frame_without_custom_passes_does_no_custom_pass_work() {
    let Some(mut rig) = Rig::new() else { return };
    half_ground(&mut rig);
    rig.draw();
    let custom = FrameFeatures::BUFFER_COMPUTE
        .with(FrameFeatures::TEXTURE_COMPUTE)
        .with(FrameFeatures::SCENE_BUFFER_COMPUTE)
        .with(FrameFeatures::SCENE_TEXTURE_COMPUTE)
        .with(FrameFeatures::TARGET_EFFECTS)
        .with(FrameFeatures::EFFECTS);
    let features = last_features().unwrap();
    // Debug builds also check that each part recorded exactly the steps
    // these features declare: no custom pass step runs.
    assert!(!features.intersects(custom), "{features}");
    assert_eq!(
        rig.renderer.custom_pass_state(),
        (0, 0, Vec::new()),
        "the check never ran"
    );
    // Nothing reads the scene depth after the opaque pass: a GPU that draws
    // in tiles doesn't write it back to memory.
    assert_eq!(depth_after_opaque(&rig.renderer), Some(wgpu::StoreOp::Discard));
    if !computes(&rig) {
        return;
    }
    // One custom pass turns its step and the check on, for that frame only.
    let (target, _) = rig.render_target(4, 4);
    let probe = rig.renderer.create_compute(&rig.device, "probe", SCENE_PROBE).unwrap();
    let plain = rig.gpu_material(MaterialDesc::default());
    rig.scene_computes.push((probe, target.id, plain));
    rig.draw();
    assert!(last_features().unwrap().contains(FrameFeatures::SCENE_TEXTURE_COMPUTE));
    assert_eq!(rig.renderer.custom_pass_state().1, 1);
    // The scene compute pass reads the depth, so the opaque pass keeps it.
    assert_eq!(depth_after_opaque(&rig.renderer), Some(wgpu::StoreOp::Store));
    rig.scene_computes.clear();
    rig.draw();
    assert!(!last_features().unwrap().intersects(custom));
    assert_eq!(rig.renderer.custom_pass_state(), (0, 1, Vec::new()));
    assert_eq!(depth_after_opaque(&rig.renderer), Some(wgpu::StoreOp::Discard));
}

#[test]
fn without_compute_shaders_scene_compute_passes_are_refused_and_the_frame_draws() {
    // WebGL2's limits: no compute shaders or storage.
    let Some(mut rig) = Rig::with_limits(wgpu::Limits::downlevel_webgl2_defaults()) else {
        return;
    };
    if crate::render::GpuLimits::of(&rig.device).compute() {
        crate::render::render_test_support::skip("the device has compute shaders after all");
        return;
    }
    half_ground(&mut rig);
    let before = rig.draw();
    let refused = rig
        .renderer
        .create_compute(&rig.device, "probe", SCENE_PROBE)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("can't run compute passes"), "{refused}");
    // A buffer pass compiles to nothing there, and is left out of the frame.
    let update = rig
        .renderer
        .create_compute(&rig.device, "update", "fn update(i: u32) { items[i].x += 1.0; }")
        .unwrap();
    let plain = rig.gpu_material(MaterialDesc::default());
    rig.scene_computes.push((update, 1 << 31, plain));
    let mut after = Vec::new();
    assert!(validation_error(&mut rig, |rig| after = rig.draw()).is_none());
    assert_eq!(rig.renderer.custom_pass_state().0, 0);
    assert!(!last_features().unwrap().intersects(FrameFeatures::SCENE_BUFFER_COMPUTE));
    assert!(before == after, "the frame draws as without the pass");
}

/// A shader constant of `examples/scene_compute.rs`, by name.
fn example_shader(name: &str) -> String {
    let source = include_str!("../../examples/scene_compute.rs").replace("\r\n", "\n");
    let marker = format!("const {name}: &str = \"");
    let start = source.find(&marker).expect("the constant") + marker.len();
    source[start..start + source[start..].find("\";").unwrap()].to_string()
}

#[test]
fn the_scene_compute_example_measures_the_frame_and_evens_out_its_light() {
    let Some(mut rig) = Rig::new() else { return };
    if !computes(&rig) {
        return;
    }
    half_ground(&mut rig);
    let (average, average_texture) = rig.render_target(1, 1);
    let meter = rig
        .renderer
        .create_compute(&rig.device, "meter", &example_shader("METER"))
        .unwrap();
    let expose = rig
        .renderer
        .create_effect(&rig.device, "expose", &example_shader("EXPOSE"))
        .unwrap();
    // Adapting at once (a fast rate), to a key of 0.18.
    let at_once = rig.gpu_material(MaterialDesc {
        custom: [vec4(1e6, 0.0, 0.0, 0.0), Vec4::ZERO, Vec4::ZERO, Vec4::ZERO],
        ..Default::default()
    });
    let settings = rig.gpu_material(MaterialDesc {
        custom: [vec4(0.0, 0.18, 0.0, 0.0), Vec4::ZERO, Vec4::ZERO, Vec4::ZERO],
        custom_textures: [Some(average), None],
        ..Default::default()
    });
    rig.scene_computes.push((meter, average.id, at_once));
    // The ground's pixel, on the right half of the frame.
    let ground = |frame: &[u8]| frame[((SIZE / 2 * SIZE + SIZE * 3 / 4) * 4) as usize];
    let sun = rig.ctx.scene3d.lighting.sun_intensity;
    let measure = |rig: &mut Rig, intensity: f32| {
        rig.ctx.scene3d.lighting.sun_intensity = intensity;
        let frame = rig.draw();
        (rig.read_target(&average_texture)[0][0], ground(&frame))
    };
    let (dim, dim_raw) = measure(&mut rig, sun);
    let (bright, bright_raw) = measure(&mut rig, sun * 4.0);
    eprintln!("average light {dim} -> {bright}; ground pixel {dim_raw} -> {bright_raw} without the effect");
    assert!(
        dim > 0.0 && bright > dim * 2.0,
        "the meter follows the frame: {dim} -> {bright}"
    );
    // Negative control above: without the effect the ground got brighter.
    assert!(bright_raw > dim_raw + 20, "{dim_raw} -> {bright_raw}");
    // With it, the light is scaled to the same average: the ground looks
    // about the same under both suns.
    rig.effects.push((expose, settings));
    let (_, dim_exposed) = measure(&mut rig, sun);
    let (_, bright_exposed) = measure(&mut rig, sun * 4.0);
    eprintln!("ground pixel {dim_exposed} -> {bright_exposed} with the effect");
    assert!(
        dim_exposed.abs_diff(bright_exposed) <= 12,
        "evened out: {dim_exposed} -> {bright_exposed}"
    );
}
