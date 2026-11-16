//! Shadow views kept from an earlier frame give the same image as drawing
//! them again. Two renderers draw the same scene offscreen, one keeping
//! unchanged shadow views and one drawing them all every frame, while
//! casters and lights move, appear and go, a material and the shadow
//! quality change and the camera moves; every frame must match exactly.
//! Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::{PointLight as PointLight3D, SpotLight as SpotLight3D, Transform as Transform3D};
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::Blend;
#[allow(unused_imports)]
use crate::render::{InstanceLevel, MaterialDesc, MeshData, Model, ModelMaterial};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

const SIZE: u32 = 128;
/// Sun cascades and light views in the scene: 2 cascades, a spot light's
/// view and a point light's six.
const CASCADES: u32 = 2;
const LIGHT_VIEWS: u32 = 7;
const ALL: u32 = ((1 << CASCADES) - 1) | LIGHTS;
const LIGHTS: u32 = ((1 << LIGHT_VIEWS) - 1) << 4;

struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    textures: [GpuTexture; 1],
    layout: wgpu::BindGroupLayout,
    white: wgpu::Texture,
    output: wgpu::Texture,
    target: wgpu::TextureView,
}

impl Rig {
    fn new() -> Option<Self> {
        let gpu = crate::render::render_test_support::gpu_with(|_| Ok(wgpu::DeviceDescriptor::default()))?;
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
            // Read back by the texture-alpha test.
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
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
        Some(Self {
            device,
            queue,
            textures,
            layout,
            white,
            output,
            target,
        })
    }

    fn renderer(&self) -> Renderer3D {
        let mut renderer = Renderer3D::new(
            &self.device,
            &self.layout,
            self.output.format(),
            0,
            &self.white,
            4,
            true,
        );
        // The GPU-driven path wherever the device has it, whatever the batch's size.
        renderer.batching.path = BatchPath::Gpu;
        renderer
    }

    /// Collects, prepares and draws one frame; returns its pixels.
    fn draw(&self, renderer: &mut Renderer3D, frame: &mut Frame3D, ctx: &mut Ctx) -> Vec<u8> {
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let scene = Scene3D {
            frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: Vec3::splat(2.0),
                sky: Vec3::splat(0.1),
                ground: Vec3::ZERO,
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (0.0, 1.0 / 60.0),
            occlusion: false,
        };
        renderer.prepare(
            &self.device,
            &self.queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: (SIZE, SIZE),
            },
        );
        let buffers = renderer.draw(&self.device, &self.target, wgpu::Color::BLACK, &self.textures, None);
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
        let rgba = readback.get_mapped_range(..).unwrap().to_vec();
        super::render_test_support::keep_image(&super::render_test_support::Image {
            width: SIZE,
            height: SIZE,
            rgba: rgba.clone(),
        });
        rgba
    }
}

#[test]
fn kept_shadow_views_draw_the_same_image_as_redrawing_them() {
    let Some(rig) = Rig::new() else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
    let matte = MaterialDesc {
        color: Color::WHITE,
        roughness: 1.0,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(matte);
    let ground = MeshData::plane(20.0);
    let cube = MeshData::cube(1.0);
    let ground_mesh = ctx.gfx.create_mesh(&ground, material);
    let cube_mesh = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(ground_mesh)));
    let mover = ctx
        .world
        .spawn((Transform3D::at(vec3(0.0, 1.0, 0.0)), Model::new(cube_mesh)));
    ctx.world
        .spawn((Transform3D::at(vec3(3.0, 1.0, 2.0)), Model::new(cube_mesh)));
    let spot = ctx.world.spawn((
        Transform3D::at(vec3(0.0, 6.0, 0.0)).looking_at(vec3(0.0, 0.0, 0.001)),
        SpotLight3D {
            intensity: 300.0,
            range: 20.0,
            inner_angle: 0.5,
            outer_angle: 0.8,
            shadows: true,
            ..Default::default()
        },
    ));
    ctx.world.spawn((
        Transform3D::at(vec3(-4.0, 3.0, -3.0)),
        PointLight3D {
            intensity: 200.0,
            range: 15.0,
            shadows: true,
            ..Default::default()
        },
    ));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.lighting.shadows = true;
    ctx.scene3d.lighting.shadow_cascades = CASCADES;
    ctx.scene3d.lighting.shadow_resolution = 512;
    ctx.scene3d.lighting.shadow_distance = 30.0;
    // Every cascade is drawn each frame unless the renderer keeps it.
    ctx.scene3d.lighting.shadow_refresh = 1;
    ctx.scene3d.lighting.light_shadow_atlas = 1024;
    ctx.scene3d.sky.atmosphere = false;

    // `cached` keeps unchanged views; `reference` draws them all.
    let mut cached = rig.renderer();
    let mut reference = rig.renderer();
    for r in [&mut cached, &mut reference] {
        r.create_material(&rig.device, &GpuMaterialDesc::new(&matte), &rig.textures);
        r.create_mesh(
            &rig.device,
            &ground,
            &[(0..ground.indices.len() as u32, material)],
            true,
        );
        r.create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
    }
    let mut renderers = [cached, reference];
    let mut frames = [Frame3D::default(), Frame3D::default()];
    let mut last = Vec::new();
    // Draws a frame with both; they must match. Returns the views kept.
    let mut step = |renderers: &mut [Renderer3D; 2], ctx: &mut Ctx, what: &str, changes: bool| -> u32 {
        let [cached, reference] = renderers;
        let [cached_frame, reference_frame] = &mut frames;
        let image = rig.draw(cached, cached_frame, ctx);
        reference.forget_shadows();
        let expected = rig.draw(reference, reference_frame, ctx);
        assert_eq!(reference.shadow_views_kept(), 0, "{what}: the reference keeps nothing");
        let differing = image.chunks(4).zip(expected.chunks(4)).filter(|(a, b)| a != b).count();
        assert_eq!(
            differing, 0,
            "{what}: {differing} pixels differ from redrawing every view"
        );
        if changes {
            assert_ne!(image, last, "{what}: the change shows (the test can see stale shadows)");
        }
        last = image;
        cached.shadow_views_kept()
    };

    assert_eq!(step(&mut renderers, &mut ctx, "first frame", false), 0);
    let [(_, _), (casters, triangles)] = renderers[0].counts();
    assert!(casters > 0, "the first frame draws the casters");
    assert_eq!(step(&mut renderers, &mut ctx, "nothing changed", false), ALL);
    let [(_, _), kept_work] = renderers[0].counts();
    eprintln!("shadow work: first frame {casters} casters, {triangles} triangles; unchanged frame {kept_work:?}");
    assert_eq!(kept_work, (0, 0), "an unchanged frame draws no shadow casters");

    // Translate without rotating. A sub-texel move must preserve at least
    // one exact projection; compare every resulting image to a full redraw.
    let mut reused = 0;
    for i in 0..24 {
        ctx.scene3d.camera.position.x += 0.0001;
        let kept = step(&mut renderers, &mut ctx, "sub-texel camera translation", false);
        reused += (kept & 0b11).count_ones();
        if i % 5 == 0 {
            ctx.world.get_mut::<Transform3D>(mover).unwrap().position.x += 0.08;
            let kept = step(&mut renderers, &mut ctx, "actor under moving camera", true);
            assert_ne!(kept & 0b11, 0b11);
        }
    }
    assert!(reused > 24, "sub-texel motion reuses cascades: {reused}/48");

    // A caster moves: the views that see it are drawn again, others kept.
    ctx.world.get_mut::<Transform3D>(mover).unwrap().position.x += 0.6;
    let kept = step(&mut renderers, &mut ctx, "caster moved", true);
    assert_ne!(kept & 0b11, 0b11, "the cascade that sees the caster is drawn: {kept:b}");
    assert!(kept & LIGHTS != 0, "light views that don't see it are kept: {kept:b}");
    assert_eq!(step(&mut renderers, &mut ctx, "settled", false), ALL);

    // The spot light moves: only its view is drawn again (its tile cleared
    // alone, the point light's tiles kept).
    ctx.world.get_mut::<Transform3D>(spot).unwrap().position.x += 0.5;
    let kept = step(&mut renderers, &mut ctx, "spot light moved", true);
    assert_eq!(kept & 0b11, 0b11, "the sun's cascades are kept: {kept:b}");
    assert_eq!(
        (ALL & !kept).count_ones(),
        1,
        "only the spot light's view is drawn: {kept:b}"
    );

    // A caster appears, then goes.
    let extra = ctx
        .world
        .spawn((Transform3D::at(vec3(-1.5, 1.0, 1.5)), Model::new(cube_mesh)));
    assert_ne!(step(&mut renderers, &mut ctx, "caster spawned", true), ALL);
    ctx.world.despawn(extra);
    assert_ne!(step(&mut renderers, &mut ctx, "caster despawned", true), ALL);
    assert_eq!(step(&mut renderers, &mut ctx, "settled again", false), ALL);

    // A material change draws every view again.
    let darker = MaterialDesc {
        color: Color::rgb(0.8, 0.8, 0.8),
        ..matte
    };
    for r in &mut renderers {
        r.update_material(
            &rig.device,
            &rig.queue,
            material.id,
            &GpuMaterialDesc::new(&darker),
            &rig.textures,
        );
    }
    assert_eq!(step(&mut renderers, &mut ctx, "material changed", true), 0);

    // New shadow quality: new sun shadow maps, every cascade drawn.
    ctx.scene3d.lighting.shadow_resolution = 1024;
    let kept = step(&mut renderers, &mut ctx, "shadow resolution changed", false);
    assert_eq!(kept & 0b11, 0, "new maps are drawn: {kept:b}");

    // The camera moves: the cascades follow it; the lights' views stay.
    ctx.scene3d.camera.position = vec3(1.0, 8.0, 10.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let kept = step(&mut renderers, &mut ctx, "camera moved", true);
    assert_eq!(kept & 0b11, 0, "cascades follow the camera: {kept:b}");
    assert_eq!(kept & LIGHTS, LIGHTS, "light views are kept: {kept:b}");
    assert_eq!(step(&mut renderers, &mut ctx, "still", false), ALL);
}

/// Views drawn with a shader that moves vertices itself (it may read the
/// time or the camera) are drawn every frame; the others are still kept.
#[test]
fn views_with_shader_moved_vertices_are_always_drawn() {
    let Some(rig) = Rig::new() else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    const WAVE: &str =
        "fn vertex(v: Vertex) -> Vertex { var out = v; out.position.x += sin(frame.time.x); return out; }";
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
    let matte = MaterialDesc {
        color: Color::WHITE,
        roughness: 1.0,
        ..Default::default()
    };
    let waving = MaterialDesc {
        shader: Some(ctx.gfx.create_shader("wave", WAVE)),
        ..matte
    };
    let material = ctx.gfx.create_material(matte);
    let wave_material = ctx.gfx.create_material(waving);
    let ground = MeshData::plane(20.0);
    let cube = MeshData::cube(1.0);
    let ground_mesh = ctx.gfx.create_mesh(&ground, material);
    let cube_mesh = ctx.gfx.create_mesh(&cube, wave_material);
    ctx.world.spawn((Transform3D::default(), Model::new(ground_mesh)));
    ctx.world
        .spawn((Transform3D::at(vec3(0.0, 1.0, 0.0)), Model::new(cube_mesh)));
    // A spot light away from the cube: its view sees only the ground.
    ctx.world.spawn((
        Transform3D::at(vec3(6.0, 4.0, 6.0)).looking_at(vec3(6.0, 0.0, 6.001)),
        SpotLight3D {
            intensity: 300.0,
            range: 6.0,
            inner_angle: 0.3,
            outer_angle: 0.4,
            shadows: true,
            ..Default::default()
        },
    ));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.lighting.shadows = true;
    ctx.scene3d.lighting.shadow_cascades = 1;
    ctx.scene3d.lighting.shadow_resolution = 512;
    ctx.scene3d.lighting.shadow_refresh = 1;
    ctx.scene3d.lighting.light_shadow_atlas = 1024;
    ctx.scene3d.sky.atmosphere = false;
    let mut renderer = rig.renderer();
    renderer.create_shader(&rig.device, "wave", WAVE).unwrap();
    renderer.create_material(&rig.device, &GpuMaterialDesc::new(&matte), &rig.textures);
    renderer.create_material(&rig.device, &GpuMaterialDesc::new(&waving), &rig.textures);
    renderer.create_mesh(
        &rig.device,
        &ground,
        &[(0..ground.indices.len() as u32, material)],
        true,
    );
    renderer.create_mesh(
        &rig.device,
        &cube,
        &[(0..cube.indices.len() as u32, wave_material)],
        true,
    );
    let mut frame = Frame3D::default();
    for _ in 0..3 {
        rig.draw(&mut renderer, &mut frame, &mut ctx);
    }
    // The cascade sees the cube; the spot light's view (view 4) doesn't.
    assert_eq!(renderer.shadow_views_kept(), 1 << 4);
}

/// A model's material override changing (here to glass, which casts no
/// shadow) redraws its shadow views, though the override's per-frame id
/// and the model's instance stay the same.
#[test]
fn a_changed_material_override_redraws_its_shadows() {
    let Some(rig) = Rig::new() else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
    let matte = MaterialDesc {
        color: Color::WHITE,
        roughness: 1.0,
        ..Default::default()
    };
    let glass = MaterialDesc {
        blend: Blend::Transparent,
        color: Color::rgba(1.0, 1.0, 1.0, 0.3),
        ..matte
    };
    let material = ctx.gfx.create_material(matte);
    let solid = ctx.gfx.create_material(matte);
    let see_through = ctx.gfx.create_material(glass);
    let ground = MeshData::plane(20.0);
    let cube = MeshData::cube(1.0);
    let ground_mesh = ctx.gfx.create_mesh(&ground, material);
    let cube_mesh = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(ground_mesh)));
    let rock = ctx.world.spawn((
        Transform3D::at(vec3(0.0, 1.0, 0.0)),
        Model::new(cube_mesh),
        ModelMaterial(solid),
    ));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.lighting.shadows = true;
    ctx.scene3d.lighting.shadow_cascades = 1;
    ctx.scene3d.lighting.shadow_resolution = 512;
    ctx.scene3d.lighting.shadow_refresh = 1;
    ctx.scene3d.sky.atmosphere = false;
    let mut renderers = [rig.renderer(), rig.renderer()];
    for r in &mut renderers {
        for desc in [&matte, &matte, &glass] {
            r.create_material(&rig.device, &GpuMaterialDesc::new(desc), &rig.textures);
        }
        r.create_mesh(
            &rig.device,
            &ground,
            &[(0..ground.indices.len() as u32, material)],
            true,
        );
        r.create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
    }
    let mut frames = [Frame3D::default(), Frame3D::default()];
    let mut draw = |ctx: &mut Ctx| {
        let [cached, reference] = &mut renderers;
        let [cached_frame, reference_frame] = &mut frames;
        let image = rig.draw(cached, cached_frame, ctx);
        reference.forget_shadows();
        let expected = rig.draw(reference, reference_frame, ctx);
        let differing = image.chunks(4).zip(expected.chunks(4)).filter(|(a, b)| a != b).count();
        assert_eq!(differing, 0, "{differing} pixels differ from redrawing every view");
        (image, cached.shadow_views_kept())
    };
    draw(&mut ctx);
    let (solid_image, kept) = draw(&mut ctx);
    assert_eq!(kept, 1, "unchanged: the cascade is kept");
    ctx.world.get_mut::<ModelMaterial>(rock).unwrap().0 = see_through;
    let (glass_image, kept) = draw(&mut ctx);
    assert_eq!(kept, 0, "the override changed: the cascade is drawn");
    assert_ne!(solid_image, glass_image);
}

#[test]
fn instance_levels_refresh_cached_sun_cascades_across_shadow_bands() {
    let Some(rig) = Rig::new() else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::splat(SIZE as f32));
    ctx.scene3d.camera.position = vec3(0.0, 6.0, 6.0);
    ctx.scene3d.camera.fov_y = 90.0_f32.to_radians();
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.lighting.shadow_cascades = 4;
    ctx.scene3d.lighting.shadow_resolution = 512;
    ctx.scene3d.lighting.shadow_refresh = 32;
    ctx.scene3d.lighting.sun_direction = -Vec3::Y;
    let desc = MaterialDesc {
        color: Color::WHITE,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(desc);
    let ground = MeshData::plane(120.0);
    let cube = MeshData::cube(2.0);
    let floor = ctx.gfx.create_mesh(&ground, material);
    let proxy = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(floor)));
    let b = ctx.gfx.create_instance_batch_levels(
        &[
            InstanceLevel {
                mesh: proxy,
                min_distance: 0.0,
                max_distance: 7.0,
                camera: false,
                shadows: true,
            },
            InstanceLevel {
                mesh: proxy,
                min_distance: 7.0,
                max_distance: 20.0,
                camera: false,
                shadows: false,
            },
        ],
        &[Mat4::from_translation(vec3(0.0, 3.0, 0.0))],
        &[],
        Default::default(),
    );
    // This caster lies in a deferred far cascade and crosses its own band
    // during the same half-unit camera move as the nearby caster.
    let far_batch = ctx.gfx.create_instance_batch_levels(
        &[InstanceLevel {
            mesh: proxy,
            min_distance: 0.0,
            max_distance: 46.25,
            camera: false,
            shadows: true,
        }],
        &[Mat4::from_scale_rotation_translation(
            Vec3::splat(4.0),
            Quat::IDENTITY,
            vec3(0.0, 3.0, -40.0),
        )],
        &[],
        Default::default(),
    );
    let mut renderers = [rig.renderer(), rig.renderer()];
    for r in &mut renderers {
        r.create_material(&rig.device, &GpuMaterialDesc::new(&desc), &rig.textures);
        r.create_mesh(
            &rig.device,
            &ground,
            &[(0..ground.indices.len() as u32, material)],
            true,
        );
        r.create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
        for batch in [b, far_batch] {
            r.set_instance_batch(
                &rig.device,
                &rig.queue,
                batch.id,
                ctx.gfx.assets3d.instance_batches[batch.id as usize].as_ref(),
            );
        }
    }
    let mut frames = [Frame3D::default(), Frame3D::default()];
    // The reference collects the same frames (holding the same boxes) and
    // draws every cascade; the cached renderer must match it every frame.
    let compare = |renderers: &mut [Renderer3D; 2], frames: &mut [Frame3D; 2], ctx: &mut Ctx, equal: bool| {
        let [cached, reference] = renderers;
        let [a, b] = frames;
        let image = rig.draw(cached, a, ctx);
        reference.forget_shadows();
        let expected = rig.draw(reference, b, ctx);
        assert_eq!(reference.shadow_cascades_redrawn(), 4);
        assert_eq!(a.sun_view_proj, b.sun_view_proj, "the same boxes");
        let matches = image == expected;
        if equal {
            assert!(matches, "cached shadow bands differ from drawing every cascade");
        }
        (image, a.redrawn, cached.shadow_cascades_redrawn(), matches)
    };
    compare(&mut renderers, &mut frames, &mut ctx, true);
    let (with_proxy, collected, redrawn, _) = compare(&mut renderers, &mut frames, &mut ctx, true);
    assert_eq!(
        collected & 0b1100,
        0,
        "stationary banded casters retain deferred far cascades"
    );
    assert_eq!(
        redrawn, 0,
        "stationary content redraws no cascades in either cache layer"
    );
    ctx.gfx.set_mesh_shadows(proxy, false);
    let (without_proxy, _, redrawn, _) = compare(&mut renderers, &mut frames, &mut ctx, true);
    assert_eq!(redrawn, 4, "mesh shadow flags invalidate cached cascades");
    assert_ne!(
        with_proxy, without_proxy,
        "shadow-only geometry must cast a visible sun shadow"
    );
    ctx.gfx.set_mesh_shadows(proxy, true);
    compare(&mut renderers, &mut frames, &mut ctx, true);
    // Half a unit closer: both casters cross a band edge, the far one
    // while its cascade's box is held. Every frame matches the reference.
    ctx.scene3d.camera.position.z = 6.5;
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let (crossed, refit, redrawn, _) = compare(&mut renderers, &mut frames, &mut ctx, true);
    assert_ne!(refit & 0b1100, 0b1100, "a far box is held: {refit:b}");
    assert_eq!(redrawn, 4, "the crossing redraws the held far cascades too");
    assert_ne!(crossed, with_proxy, "the crossing shows");
    for _ in 0..3 {
        compare(&mut renderers, &mut frames, &mut ctx, true);
    }
    let (_, _, redrawn, _) = compare(&mut renderers, &mut frames, &mut ctx, true);
    assert_eq!(redrawn, 0, "still again: no cascade is drawn");

    // Negative control: a cache that ignores what changed under its held
    // far cascades misses the far caster crossing back.
    renderers[0].ignore_shadow_casters();
    ctx.scene3d.camera.position.z = 6.0;
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let stale = (0..2).any(|_| !compare(&mut renderers, &mut frames, &mut ctx, false).3);
    assert!(stale, "a lagging far cascade shows in the comparison");
}

/// Instance batches culled by distance from the camera for their shadows:
/// a camera moving inside the held far cascades' boxes keeps their maps
/// while no band edge lies near any of the batch's cells, and they match
/// drawing every cascade again.
#[test]
fn a_camera_move_crossing_no_band_edge_keeps_held_cascades() {
    let Some(rig) = Rig::new() else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::splat(SIZE as f32));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.lighting.shadow_cascades = CASCADES;
    ctx.scene3d.lighting.shadow_resolution = 512;
    ctx.scene3d.lighting.shadow_distance = 30.0;
    ctx.scene3d.lighting.shadow_refresh = 8;
    let desc = MaterialDesc {
        color: Color::WHITE,
        roughness: 1.0,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(desc);
    let ground = MeshData::plane(60.0);
    let cube = MeshData::cube(1.0);
    let floor = ctx.gfx.create_mesh(&ground, material);
    let proxy = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(floor)));
    // Pillars out to 20 units away, casting within 80 of the camera: no
    // band edge near any of them.
    let pillars: Vec<Mat4> = (0..8)
        .map(|i| {
            Mat4::from_scale_rotation_translation(
                vec3(0.5, 3.0, 0.5),
                Quat::IDENTITY,
                vec3(-6.0 + i as f32 * 1.7, 1.5, -4.0 - i as f32 * 2.0),
            )
        })
        .collect();
    let batch = ctx.gfx.create_instance_batch_levels(
        &[InstanceLevel {
            mesh: proxy,
            min_distance: 0.0,
            max_distance: 80.0,
            camera: true,
            shadows: true,
        }],
        &pillars,
        &[],
        Default::default(),
    );
    let mut renderers = [rig.renderer(), rig.renderer()];
    for r in &mut renderers {
        r.create_material(&rig.device, &GpuMaterialDesc::new(&desc), &rig.textures);
        r.create_mesh(
            &rig.device,
            &ground,
            &[(0..ground.indices.len() as u32, material)],
            true,
        );
        r.create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
        r.set_instance_batch(
            &rig.device,
            &rig.queue,
            batch.id,
            ctx.gfx.assets3d.instance_batches[batch.id as usize].as_ref(),
        );
    }
    let mut frames = [Frame3D::default(), Frame3D::default()];
    let far = 1 << (CASCADES - 1);
    let mut kept = 0;
    for i in 0..16 {
        if i >= 2 {
            ctx.scene3d.camera.position.x += 0.01;
        }
        let [cached, reference] = &mut renderers;
        let [a, b] = &mut frames;
        let image = rig.draw(cached, a, &mut ctx);
        reference.forget_shadows();
        let expected = rig.draw(reference, b, &mut ctx);
        assert_eq!(a.sun_view_proj, b.sun_view_proj, "the same boxes");
        assert!(image == expected, "frame {i}: kept cascades differ from drawing them");
        if i >= 2 && a.redrawn & far == 0 {
            // The far box is held: the moving eye alone doesn't redraw it.
            assert_ne!(
                cached.shadow_views_kept() & far,
                0,
                "frame {i}: the held far cascade is kept"
            );
            kept += 1;
        }
    }
    assert!(kept >= 8, "the far box was held on most frames: {kept}");
}

#[test]
fn pure_custom_functions_and_texture_channels_only_refresh_affected_shadows() {
    let Some(rig) = Rig::new() else { return };
    const PURE: &str = "fn vertex(v: Vertex) -> Vertex { return v; } fn surface(s: Surface) -> Surface { return s; }";
    const MOVE: &str = "fn vertex(v: Vertex) -> Vertex { var out = v; out.position.x += 0.8; return out; } fn surface(s: Surface) -> Surface { return s; }";
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::splat(SIZE as f32));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.lighting.shadow_cascades = 1;
    ctx.scene3d.lighting.shadow_resolution = 512;
    ctx.scene3d.lighting.shadow_distance = 30.0;
    ctx.scene3d.lighting.shadow_refresh = 1;
    ctx.scene3d.sky.atmosphere = false;
    let shader = ctx.gfx.create_shader("pure", PURE);
    let texture = crate::texture::Texture::new(0, 1, 1);
    let floor_desc = MaterialDesc {
        color: Color::WHITE,
        ..Default::default()
    };
    let mut desc = MaterialDesc {
        blend: Blend::Cutout,
        texture: Some(texture),
        shader: Some(shader),
        ..floor_desc
    };
    let floor_material = ctx.gfx.create_material(floor_desc);
    let material = ctx.gfx.create_material(desc);
    let ground = MeshData::plane(20.0);
    let cube = MeshData::cube(1.0);
    let floor = ctx.gfx.create_mesh(&ground, floor_material);
    let caster = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(floor)));
    ctx.world
        .spawn((Transform3D::at(vec3(0.0, 1.0, 0.0)), Model::new(caster)));
    let mut renderers = [rig.renderer(), rig.renderer()];
    for r in &mut renderers {
        r.create_shader(&rig.device, "pure", PURE).unwrap();
        for d in [floor_desc, desc] {
            r.create_material(&rig.device, &GpuMaterialDesc::new(&d), &rig.textures);
        }
        r.create_mesh(
            &rig.device,
            &ground,
            &[(0..ground.indices.len() as u32, floor_material)],
            true,
        );
        r.create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
    }
    let mut frames = [Frame3D::default(), Frame3D::default()];
    let mut compare = |renderers: &mut [Renderer3D; 2], ctx: &mut Ctx| {
        let [cached, reference] = renderers;
        let [a, b] = &mut frames;
        let image = rig.draw(cached, a, ctx);
        reference.forget_shadows();
        let expected = rig.draw(reference, b, ctx);
        assert_eq!(image, expected, "cached custom/cutout shadows must match full redraw");
        (image, cached.shadow_views_kept())
    };
    compare(&mut renderers, &mut ctx);
    assert_eq!(
        compare(&mut renderers, &mut ctx).1,
        1,
        "pure custom functions are reusable"
    );
    for r in &mut renderers {
        r.replace_shader(&rig.device, 1, "moved", MOVE).unwrap();
    }
    let (moved, kept) = compare(&mut renderers, &mut ctx);
    assert_eq!(kept, 0, "vertex shader reload refreshes depth");
    assert_eq!(compare(&mut renderers, &mut ctx).1, 1);
    for r in &mut renderers {
        r.free_shader(1);
    }
    let (freed, kept) = compare(&mut renderers, &mut ctx);
    assert_eq!(kept, 0, "freeing a cached custom vertex shader refreshes depth");
    assert_ne!(moved, freed, "shader removal restores the original geometry");
    // Built-in cutout: only alpha from its base texture affects shadows.
    desc.shader = None;
    for r in &mut renderers {
        r.update_material(
            &rig.device,
            &rig.queue,
            material.id,
            &GpuMaterialDesc::new(&desc),
            &rig.textures,
        );
    }
    let (solid, _) = compare(&mut renderers, &mut ctx);
    assert_ne!(moved, solid, "shader movement is visible");
    compare(&mut renderers, &mut ctx);
    for r in &mut renderers {
        r.texture_alpha_changed(0, false);
    }
    assert_eq!(
        compare(&mut renderers, &mut ctx).1,
        1,
        "RGB-only uploads retain cutout shadows"
    );
    rig.queue.write_texture(
        rig.textures[0].texture.as_image_copy(),
        &[255, 255, 255, 0],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        rig.textures[0].texture.size(),
    );
    for r in &mut renderers {
        r.texture_alpha_changed(0, true);
    }
    let (holes, kept) = compare(&mut renderers, &mut ctx);
    assert_eq!(kept, 0, "base alpha updates refresh cutout shadows");
    assert_ne!(solid, holes, "the alpha change must be visible");
    // Rebinding a freed base texture must invalidate its cached alpha.
    rig.queue.write_texture(
        rig.textures[0].texture.as_image_copy(),
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        rig.textures[0].texture.size(),
    );
    for r in &mut renderers {
        r.rebind_materials(&rig.device, &rig.queue, &rig.textures, Some(0), None);
    }
    let (rebound, kept) = compare(&mut renderers, &mut ctx);
    assert_eq!(kept, 0, "freeing/rebinding base textures refreshes cached cutout depth");
    assert_ne!(holes, rebound);
    // A normal texture, even its alpha, cannot affect built-in cutout depth.
    desc.texture = None;
    desc.normal_map = Some(texture);
    for r in &mut renderers {
        r.update_material(
            &rig.device,
            &rig.queue,
            material.id,
            &GpuMaterialDesc::new(&desc),
            &rig.textures,
        );
    }
    compare(&mut renderers, &mut ctx);
    compare(&mut renderers, &mut ctx);
    for r in &mut renderers {
        r.texture_alpha_changed(0, true);
    }
    assert_eq!(
        compare(&mut renderers, &mut ctx).1,
        1,
        "normal-map changes retain depth"
    );
    // A texture-sampling custom cutout can be retained too: its texture is
    // tracked, whereas render targets and storage arrays remain volatile.
    const SAMPLED: &str = "fn surface(s: Surface) -> Surface { var out = s; out.alpha = textureSample(base_texture, material_sampler, s.uv).a; return out; }";
    desc.shader = Some(shader);
    desc.texture = Some(texture);
    desc.normal_map = None;
    for r in &mut renderers {
        r.replace_shader(&rig.device, 1, "sampled", SAMPLED).unwrap();
        r.update_material(
            &rig.device,
            &rig.queue,
            material.id,
            &GpuMaterialDesc::new(&desc),
            &rig.textures,
        );
    }
    compare(&mut renderers, &mut ctx);
    assert_eq!(
        compare(&mut renderers, &mut ctx).1,
        1,
        "tracked custom texture inputs are reusable"
    );
    rig.queue.write_texture(
        rig.textures[0].texture.as_image_copy(),
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        rig.textures[0].texture.size(),
    );
    for r in &mut renderers {
        r.texture_alpha_changed(0, true);
    }
    assert_eq!(
        compare(&mut renderers, &mut ctx).1,
        0,
        "custom texture changes invalidate dependent depth"
    );
    // Surface color changes on an opaque material change the camera image,
    // but the shadow pass has no fragment stage and must retain its depth.
    const COLOR: &str =
        "fn surface(s: Surface) -> Surface { var out = s; out.albedo = vec3<f32>(0.2, 0.4, 0.7); return out; }";
    let color_shader = ctx.gfx.create_shader("color", "");
    let opaque = MaterialDesc {
        shader: Some(color_shader),
        ..floor_desc
    };
    for r in &mut renderers {
        r.create_shader(&rig.device, "color", "").unwrap();
        r.update_material(
            &rig.device,
            &rig.queue,
            floor_material.id,
            &GpuMaterialDesc::new(&opaque),
            &rig.textures,
        );
    }
    let (before, _) = compare(&mut renderers, &mut ctx);
    compare(&mut renderers, &mut ctx);
    for r in &mut renderers {
        r.replace_shader(&rig.device, 2, "color", COLOR).unwrap();
    }
    let (after, kept) = compare(&mut renderers, &mut ctx);
    assert_eq!(kept, 1, "opaque fragment-only shader reloads retain shadow depth");
    assert_ne!(before, after, "the fragment shader reload must change the image");
}

/// A cutout caster's base texture keeps only an alpha summary until a
/// region upload needs a mask: it is then read back from the GPU once, and
/// the shadows kept or redrawn from it match a full redraw every time.
#[test]
fn lazily_built_alpha_masks_keep_cutout_shadows_exact() {
    use super::texture_alpha::{TextureAlphas, read_alpha};
    let Some(rig) = Rig::new() else { return };
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::splat(SIZE as f32));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.lighting.shadow_cascades = 1;
    ctx.scene3d.lighting.shadow_resolution = 512;
    ctx.scene3d.lighting.shadow_distance = 30.0;
    ctx.scene3d.lighting.shadow_refresh = 1;
    ctx.scene3d.sky.atmosphere = false;
    let texture = crate::texture::Texture::new(0, 1, 1);
    let floor_desc = MaterialDesc {
        color: Color::WHITE,
        ..Default::default()
    };
    let desc = MaterialDesc {
        blend: Blend::Cutout,
        texture: Some(texture),
        ..floor_desc
    };
    let floor_material = ctx.gfx.create_material(floor_desc);
    let material = ctx.gfx.create_material(desc);
    let ground = MeshData::plane(20.0);
    let cube = MeshData::cube(1.0);
    let floor = ctx.gfx.create_mesh(&ground, floor_material);
    let caster = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(floor)));
    ctx.world
        .spawn((Transform3D::at(vec3(0.0, 1.0, 0.0)), Model::new(caster)));
    // The texture as the renderer made it: opaque white, summary only.
    let mut alphas = TextureAlphas::default();
    alphas.created(0, 1, 1, &[255; 4]);
    assert_eq!(alphas.memory().masks, 0, "a new texture keeps no mask");
    let mut renderers = [rig.renderer(), rig.renderer()];
    assert!(!renderers[0].alpha_tested(0), "no material reads it yet");
    for r in &mut renderers {
        for d in [floor_desc, desc] {
            r.create_material(&rig.device, &GpuMaterialDesc::new(&d), &rig.textures);
        }
        r.create_mesh(
            &rig.device,
            &ground,
            &[(0..ground.indices.len() as u32, floor_material)],
            true,
        );
        r.create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
    }
    assert!(renderers[0].alpha_tested(0), "the cutout material reads its alpha");
    let mut frames = [Frame3D::default(), Frame3D::default()];
    // Draws with kept shadows and with every shadow redrawn.
    let mut draw = |renderers: &mut [Renderer3D; 2], ctx: &mut Ctx| {
        let [cached, reference] = renderers;
        let [a, b] = &mut frames;
        let image = rig.draw(cached, a, ctx);
        reference.forget_shadows();
        (image, rig.draw(reference, b, ctx), cached.shadow_views_kept())
    };
    // A region upload the way `Renderer::update_texture_region` makes one.
    let upload = |alphas: &mut TextureAlphas, renderers: &mut [Renderer3D; 2], pixel: [u8; 4], tell: Option<bool>| {
        let tested = renderers[0].alpha_tested(0);
        let changed = alphas.region(0, [0, 0], [1, 1], &pixel, tested, |_, _| {
            read_alpha(&rig.device, &rig.queue, &rig.textures[0].texture)
        });
        rig.queue.write_texture(
            rig.textures[0].texture.as_image_copy(),
            &pixel,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            rig.textures[0].texture.size(),
        );
        for r in renderers.iter_mut() {
            r.texture_alpha_changed(0, tell.unwrap_or(changed));
        }
        changed
    };
    let (solid, expected, _) = draw(&mut renderers, &mut ctx);
    assert_eq!(solid, expected);
    // RGB only: the mask is read back (once) and the shadows are kept.
    assert!(!upload(&mut alphas, &mut renderers, [40, 90, 200, 255], None));
    assert_eq!(alphas.masks_built(), 1);
    assert_eq!(alphas.memory().mask_bytes, 1);
    let (image, expected, kept) = draw(&mut renderers, &mut ctx);
    assert_eq!(kept, 1, "RGB-only uploads keep cutout shadows");
    assert_eq!(image, expected, "kept cutout shadows match a full redraw");
    // Alpha cut out: the mask says so, the shadows are redrawn.
    assert!(upload(&mut alphas, &mut renderers, [40, 90, 200, 0], None));
    let (holes, expected, kept) = draw(&mut renderers, &mut ctx);
    assert_eq!(kept, 0, "alpha changes redraw cutout shadows");
    assert_eq!(holes, expected);
    assert_ne!(holes, solid, "the cut-out caster drops its shadow");
    assert_eq!(alphas.masks_built(), 1, "the mask is built exactly once");
    // Negative control: claiming an alpha change kept nothing would keep
    // stale shadows, which the full redraw shows.
    assert!(upload(&mut alphas, &mut renderers, [255; 4], Some(false)));
    let (stale, expected, kept) = draw(&mut renderers, &mut ctx);
    assert_eq!(kept, 1);
    assert_ne!(stale, expected, "stale cutout shadows differ from a full redraw");
    assert_eq!(expected, solid, "back to the first image");
}

/// With `Lighting::shadow_sun_step`, a sun turning a little every frame
/// leaves the sun's shadow maps bit for bit the same, and kept by the
/// cache, until it has turned past the step; then they change once.
/// Negative control: with no step they change every frame.
#[test]
fn a_slowly_turning_sun_keeps_its_shadow_maps_until_the_step() {
    let Some(rig) = Rig::new() else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
    let matte = MaterialDesc {
        color: Color::WHITE,
        roughness: 1.0,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(matte);
    let ground = MeshData::plane(20.0);
    let cube = MeshData::cube(1.0);
    let ground_mesh = ctx.gfx.create_mesh(&ground, material);
    let cube_mesh = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(ground_mesh)));
    ctx.world
        .spawn((Transform3D::at(vec3(0.0, 1.0, 0.0)), Model::new(cube_mesh)));
    ctx.world
        .spawn((Transform3D::at(vec3(3.0, 1.0, 2.0)), Model::new(cube_mesh)));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.lighting.shadows = true;
    ctx.scene3d.lighting.shadow_cascades = CASCADES;
    ctx.scene3d.lighting.shadow_resolution = 512;
    ctx.scene3d.lighting.shadow_distance = 30.0;
    ctx.scene3d.lighting.shadow_refresh = 1;
    ctx.scene3d.sky.atmosphere = false;
    // The sun turns 0.1 degrees about the vertical each frame, 50 degrees
    // down: 0.064 degrees of turn a frame.
    let sun = |frame: usize| {
        let (s, c) = (frame as f32 * 0.1).to_radians().sin_cos();
        vec3(c * 0.64, -0.77, s * 0.64)
    };

    // Each frame: whether the shadow maps changed since the frame before,
    // and how many cascades the cache kept.
    let run = |ctx: &mut Ctx, step: f32, frames: usize| -> Vec<(bool, u32)> {
        ctx.scene3d.lighting.shadow_sun_step = step;
        let mut renderer = rig.renderer();
        renderer.create_material(&rig.device, &GpuMaterialDesc::new(&matte), &rig.textures);
        renderer.create_mesh(
            &rig.device,
            &ground,
            &[(0..ground.indices.len() as u32, material)],
            true,
        );
        renderer.create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
        let mut frame = Frame3D::default();
        let mut last = None;
        (0..frames)
            .map(|i| {
                ctx.scene3d.lighting.sun_direction = sun(i);
                rig.draw(&mut renderer, &mut frame, ctx);
                let map = renderer.shadow_map_bytes(&rig.device, &rig.queue);
                let changed = last.as_ref().is_some_and(|l| *l != map);
                last = Some(map);
                (changed, (renderer.shadow_views_kept() & 0b11).count_ones())
            })
            .collect()
    };

    // A 1 degree step: the turn passes it after 16 frames (1 / 0.064).
    let held = run(&mut ctx, 1f32.to_radians(), 40);
    let changed: Vec<usize> = (1..held.len()).filter(|&i| held[i].0).collect();
    assert_eq!(changed, [16, 32], "the maps change only at each step");
    for (i, &(change, kept)) in held.iter().enumerate().skip(1) {
        let expect = if change { 0 } else { CASCADES };
        assert_eq!(kept, expect, "frame {i}: cascades kept between steps, drawn at a step");
    }

    // Negative control: the exact direction changes the maps every frame.
    let exact = run(&mut ctx, 0.0, 6);
    assert!(
        exact[1..].iter().all(|&(change, kept)| change && kept == 0),
        "{exact:?}"
    );
}

/// With contact-hardening sun shadows (`ShadowFilter::ContactHardening`),
/// far cascades kept from earlier frames (their box held by
/// `Lighting::shadow_refresh`, their map kept by the renderer) draw exactly
/// like the same cascades drawn again into the same boxes: while nothing
/// moves, while the camera moves inside the held boxes, while the sun
/// turns within its held step and past it, and while a caster moves. The
/// reference is a second renderer collecting the same frames (so holding
/// the same boxes) which keeps no view. Negative control: a cache that
/// ignores caster changes (the lag kept far cascades used to have) shows
/// the moving caster's stale shadow in the comparison.
#[test]
fn contact_hardening_shadows_from_kept_cascades_match_redrawn_ones() {
    let Some(rig) = Rig::new() else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
    let matte = MaterialDesc {
        color: Color::WHITE,
        roughness: 1.0,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(matte);
    let ground = MeshData::plane(40.0);
    let cube = MeshData::cube(1.0);
    let ground_mesh = ctx.gfx.create_mesh(&ground, material);
    let cube_mesh = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(ground_mesh)));
    // Tall posts and a block: their shadows' far ends have wide edges.
    for (x, z) in [(-2.0, 0.0), (1.5, -2.0), (3.0, 2.0)] {
        ctx.world.spawn((
            Transform3D {
                position: vec3(x, 2.0, z),
                scale: vec3(0.4, 4.0, 0.4),
                ..Default::default()
            },
            Model::new(cube_mesh),
        ));
    }
    let mover = ctx
        .world
        .spawn((Transform3D::at(vec3(0.0, 1.0, 1.0)), Model::new(cube_mesh)));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.sky.atmosphere = false;
    let lighting = &mut ctx.scene3d.lighting;
    lighting.shadows = true;
    lighting.shadow_cascades = CASCADES;
    lighting.shadow_resolution = 512;
    lighting.shadow_distance = 30.0;
    // The far cascade is drawn every fourth frame and kept in between.
    lighting.shadow_refresh = 4;
    lighting.shadow_filter = crate::render::ShadowFilter::ContactHardening;
    lighting.shadow_sun_size = 3.0;
    lighting.shadow_sun_step = 1f32.to_radians();
    // The sun turns 0.1 degrees about the vertical a frame, 50 degrees
    // down: 0.064 degrees of turn a frame, past the step every 16 frames.
    let sun = |frame: usize| {
        let (s, c) = (frame as f32 * 0.1).to_radians().sin_cos();
        vec3(c * 0.64, -0.77, s * 0.64)
    };
    lighting.sun_direction = sun(0);

    let mut renderers = [rig.renderer(), rig.renderer()];
    for r in &mut renderers {
        r.create_material(&rig.device, &GpuMaterialDesc::new(&matte), &rig.textures);
        r.create_mesh(
            &rig.device,
            &ground,
            &[(0..ground.indices.len() as u32, material)],
            true,
        );
        r.create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
    }
    let mut frames = [Frame3D::default(), Frame3D::default()];
    // Draws a frame with both: how many pixels differ, whether the cached
    // renderer kept the far cascade, and its image.
    let mut draw = |renderers: &mut [Renderer3D; 2], ctx: &mut Ctx| -> (usize, bool, Vec<u8>) {
        let [cached, reference] = renderers;
        let [cached_frame, reference_frame] = &mut frames;
        let image = rig.draw(cached, cached_frame, ctx);
        reference.forget_shadows();
        let expected = rig.draw(reference, reference_frame, ctx);
        assert_eq!(
            cached_frame.sun_view_proj, reference_frame.sun_view_proj,
            "the reference draws into the boxes the cache keeps"
        );
        assert_eq!(cached_frame.cascade_texel, reference_frame.cascade_texel);
        assert_eq!(
            reference.shadow_cascades_redrawn(),
            CASCADES,
            "the reference draws every cascade"
        );
        let far = 1 << (CASCADES - 1);
        let kept = cached.shadow_views_kept() & far != 0;
        let differing = image.chunks(4).zip(expected.chunks(4)).filter(|(a, b)| a != b).count();
        (differing, kept, image)
    };
    let kept_frames = std::cell::Cell::new(0);
    let mut check = |renderers: &mut [Renderer3D; 2], ctx: &mut Ctx, what: &str| -> Vec<u8> {
        let (differing, kept, image) = draw(renderers, ctx);
        assert_eq!(
            differing, 0,
            "{what}: {differing} pixels differ from redrawing every cascade"
        );
        kept_frames.set(kept_frames.get() + usize::from(kept));
        image
    };

    for _ in 0..8 {
        check(&mut renderers, &mut ctx, "still");
    }
    for _ in 0..12 {
        ctx.scene3d.camera.position.x += 0.003;
        check(&mut renderers, &mut ctx, "camera moving inside the kept boxes");
    }
    // The shadow maps change only where the held direction jumps (lighting
    // follows the exact direction, so the picture itself changes a little
    // every frame).
    check(&mut renderers, &mut ctx, "before the sun turns");
    let mut last = renderers[0].shadow_map_bytes(&rig.device, &rig.queue);
    let mut steps = Vec::new();
    for i in 1..=40 {
        ctx.scene3d.lighting.sun_direction = sun(i);
        check(&mut renderers, &mut ctx, &format!("sun turned for {i} frames"));
        let map = renderers[0].shadow_map_bytes(&rig.device, &rig.queue);
        if map != last {
            steps.push(i);
        }
        last = map;
    }
    let kept_frames = kept_frames.get();
    eprintln!("far cascade kept in {kept_frames} of 61 frames; the shadow maps changed at frames {steps:?}");
    assert!(kept_frames >= 30, "the far cascade is kept most frames: {kept_frames}");
    assert_eq!(steps, [16, 32], "the cached maps change only at the sun's steps");

    // A caster moving while the far cascade's box is held: its shadow
    // follows it every frame.
    for _ in 0..8 {
        ctx.world.get_mut::<Transform3D>(mover).unwrap().position.x += 0.3;
        check(&mut renderers, &mut ctx, "a caster moving under the held far cascade");
    }
    for _ in 0..3 {
        check(&mut renderers, &mut ctx, "settled");
    }
    let [(_, _), work] = renderers[0].counts();
    assert_eq!(work, (0, 0), "a still frame draws no shadow casters");
    assert_eq!(renderers[0].shadow_cascades_redrawn(), 0, "nor any cascade pass");

    // Negative control: ignoring caster changes, the far cascade lags
    // behind the moving caster, and the comparison sees that.
    renderers[0].ignore_shadow_casters();
    let mut stale = 0;
    for _ in 0..8 {
        ctx.world.get_mut::<Transform3D>(mover).unwrap().position.x += 0.3;
        let (differing, kept, _) = draw(&mut renderers, &mut ctx);
        stale += usize::from(kept && differing > 0);
    }
    assert!(stale > 0, "a lagging far cascade shows in the comparison");
}

/// Far cascades whose box is held from an earlier frame
/// (`Lighting::shadow_refresh`) draw exactly what drawing every shadow view
/// again draws whenever their casters change: a caster moving, spawned and
/// despawned, a level-of-detail group switching level, a mesh's data made
/// again, an instance batch made and freed. The reference is a second
/// renderer collecting the same frames (so holding the same boxes) which
/// keeps no view. Still frames keep every cascade and draw no caster;
/// a change redraws only the cascades it reaches. Negative control: a cache
/// that ignores caster changes (the lag held far cascades used to have)
/// shows stale shadows in the same comparison.
#[test]
fn held_cascades_follow_every_caster_change() {
    use crate::render::{LodGroup, LodLevel, LodRenderer};
    let Some(rig) = Rig::new() else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
    let matte = MaterialDesc {
        color: Color::WHITE,
        roughness: 1.0,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(matte);
    let ground = MeshData::plane(80.0);
    let cube = MeshData::cube(1.0);
    let ball = MeshData::sphere(0.5, 12);
    let ground_mesh = ctx.gfx.create_mesh(&ground, material);
    let cube_mesh = ctx.gfx.create_mesh(&cube, material);
    // Made again with other data, on the GPU only, halfway through.
    let remade_mesh = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(ground_mesh)));
    ctx.world
        .spawn((Transform3D::at(vec3(0.0, 1.0, 0.0)), Model::new(cube_mesh)));
    // Far casters, in the far cascade only.
    let far_post = |x: f32, z: f32| Transform3D {
        position: vec3(x, 1.5, z),
        scale: vec3(0.6, 3.0, 0.6),
        ..Default::default()
    };
    let mover = ctx.world.spawn((far_post(2.0, -14.0), Model::new(cube_mesh)));
    ctx.world.spawn((far_post(5.0, -17.0), Model::new(remade_mesh)));
    let tall = LodRenderer {
        offset: Transform3D {
            position: vec3(0.0, 2.5, 0.0),
            scale: vec3(0.5, 5.0, 0.5),
            ..Default::default()
        },
        ..LodRenderer::new(cube_mesh)
    };
    let group = LodGroup::new(
        5.0,
        vec![
            LodLevel::new(0.3, vec![tall]),
            LodLevel::new(0.001, vec![LodRenderer::new(cube_mesh)]),
        ],
    );
    ctx.world.spawn((Transform3D::at(vec3(-4.0, 0.5, -15.0)), group));
    ctx.scene3d.camera.position = vec3(0.0, 9.0, 9.0);
    ctx.scene3d.camera.look_at(vec3(0.0, 0.0, -4.0));
    ctx.scene3d.sky.atmosphere = false;
    let lighting = &mut ctx.scene3d.lighting;
    lighting.shadows = true;
    lighting.shadow_cascades = CASCADES;
    lighting.shadow_resolution = 512;
    lighting.shadow_distance = 40.0;
    // The far cascade's box is held for up to 8 frames.
    lighting.shadow_refresh = 8;
    lighting.sun_direction = vec3(0.4, -0.8, 0.45);

    let mut renderers = [rig.renderer(), rig.renderer()];
    for r in &mut renderers {
        r.create_material(&rig.device, &GpuMaterialDesc::new(&matte), &rig.textures);
        for data in [&ground, &cube, &cube] {
            r.create_mesh(&rig.device, data, &[(0..data.indices.len() as u32, material)], true);
        }
    }
    let mut frames = [Frame3D::default(), Frame3D::default()];
    let near = 1u32;
    let far = 1u32 << (CASCADES - 1);
    let last = std::cell::RefCell::new(Vec::new());
    // Draws a frame with both: how many pixels differ, the views the
    // cached renderer kept, whether the far box was held, and the cascade
    // passes it drew.
    let draw = |renderers: &mut [Renderer3D; 2], frames: &mut [Frame3D; 2], ctx: &mut Ctx| {
        let [cached, reference] = renderers;
        let [cached_frame, reference_frame] = frames;
        let image = rig.draw(cached, cached_frame, ctx);
        reference.forget_shadows();
        let expected = rig.draw(reference, reference_frame, ctx);
        assert_eq!(
            cached_frame.sun_view_proj, reference_frame.sun_view_proj,
            "the reference draws into the boxes the cache holds"
        );
        assert_eq!(reference.shadow_cascades_redrawn(), CASCADES);
        let differing = image.chunks(4).zip(expected.chunks(4)).filter(|(a, b)| a != b).count();
        let held = cached_frame.redrawn & far == 0;
        let changed = image != *last.borrow();
        *last.borrow_mut() = image;
        (
            differing,
            cached.shadow_views_kept(),
            held,
            cached.shadow_cascades_redrawn(),
            changed,
        )
    };
    // A frame that must match; returns (views kept, far box held, cascade
    // passes drawn, whether the picture changed).
    let check = |renderers: &mut [Renderer3D; 2], frames: &mut [Frame3D; 2], ctx: &mut Ctx, what: &str| {
        let (differing, kept, held, passes, changed) = draw(renderers, frames, ctx);
        assert_eq!(
            differing, 0,
            "{what}: {differing} pixels differ from drawing every view"
        );
        (kept, held, passes, changed)
    };

    check(&mut renderers, &mut frames, &mut ctx, "first frame");
    for _ in 0..3 {
        let (kept, held, passes, _) = check(&mut renderers, &mut frames, &mut ctx, "still");
        assert_eq!(kept & (near | far), near | far, "still: every cascade is kept");
        assert_eq!(passes, 0, "still: no cascade pass");
        let [_, (casters, triangles)] = renderers[0].counts();
        assert_eq!((casters, triangles), (0, 0), "still: no shadow caster is drawn");
        assert_eq!(renderers[0].shadow_views_compared(), 0, "still: no caster is compared");
        let _ = held;
    }

    // Each change: the far cascade is drawn again (the near one, which it
    // doesn't reach, is kept), and the picture shows the change. With the
    // camera still, the far box is the same whether it is held or fitted
    // anew on its turn; most changes land on frames that hold it, where
    // the cache used to keep the map.
    let changes = std::cell::Cell::new(0);
    let held_changes = std::cell::Cell::new(0);
    let change =
        |renderers: &mut [Renderer3D; 2], frames: &mut [Frame3D; 2], ctx: &mut Ctx, what: &str, visible: bool| {
            let box_before = frames[0].sun_view_proj;
            let (kept, held, passes, changed) = check(renderers, frames, ctx, what);
            assert_eq!(frames[0].sun_view_proj, box_before, "{what}: the same boxes");
            held_changes.set(held_changes.get() + usize::from(held));
            assert_eq!(kept & far, 0, "{what}: the far cascade is drawn again");
            assert_ne!(kept & near, 0, "{what}: the near cascade is kept");
            assert_eq!(passes, 1, "{what}: one cascade pass");
            if visible {
                assert!(changed, "{what}: the change shows");
            }
            changes.set(changes.get() + 1);
            // Settled: kept again.
            let (kept, _, passes, _) = check(renderers, frames, ctx, &format!("after {what}"));
            assert_eq!((kept & far, passes), (far, 0), "after {what}: kept again");
        };
    ctx.world.get_mut::<Transform3D>(mover).unwrap().position.x += 0.5;
    change(&mut renderers, &mut frames, &mut ctx, "far caster moved", true);
    let extra = ctx.world.spawn((far_post(-1.0, -19.0), Model::new(cube_mesh)));
    change(&mut renderers, &mut frames, &mut ctx, "far caster spawned", true);
    ctx.world.despawn(extra);
    change(&mut renderers, &mut frames, &mut ctx, "far caster despawned", true);
    for f in &mut frames {
        f.lod.bias = 4.0;
    }
    change(&mut renderers, &mut frames, &mut ctx, "level of detail switched", true);
    for f in &mut frames {
        f.lod.bias = 1.0;
    }
    change(
        &mut renderers,
        &mut frames,
        &mut ctx,
        "level of detail switched back",
        true,
    );

    // A mesh made again with other data (the renderer's own change, which
    // the collector doesn't see): every view is drawn again.
    for r in &mut renderers {
        r.create_mesh_at(
            &rig.device,
            remade_mesh.id,
            &ball,
            &[(0..ball.indices.len() as u32, material)],
            true,
        );
    }
    let (kept, _, _, changed) = check(&mut renderers, &mut frames, &mut ctx, "mesh made again");
    assert!(changed, "mesh made again: the change shows");
    assert_eq!(kept & far, 0, "mesh made again: the far cascade is drawn again");
    check(&mut renderers, &mut frames, &mut ctx, "after the mesh");

    // An instance batch made and freed in the far cascade.
    let batch = ctx.gfx.create_instance_batch(
        cube_mesh,
        &[Mat4::from_scale_rotation_translation(
            vec3(0.6, 3.0, 0.6),
            Quat::IDENTITY,
            vec3(-1.0, 1.5, -19.0),
        )],
        Default::default(),
    );
    let sync = |renderers: &mut [Renderer3D; 2], ctx: &Ctx| {
        for r in renderers {
            r.set_instance_batch(
                &rig.device,
                &rig.queue,
                batch.id,
                ctx.gfx.assets3d.instance_batches[batch.id as usize].as_ref(),
            );
        }
    };
    sync(&mut renderers, &ctx);
    change(&mut renderers, &mut frames, &mut ctx, "instance batch made", true);
    ctx.gfx.free(batch);
    sync(&mut renderers, &ctx);
    change(&mut renderers, &mut frames, &mut ctx, "instance batch freed", true);
    assert_eq!(changes.get(), 7);
    assert!(held_changes.get() >= 4, "{} changes on held frames", held_changes.get());

    // Negative control: ignoring caster changes, the far cascade lags
    // behind the moving caster, and the comparison sees that.
    renderers[0].ignore_shadow_casters();
    let mut stale = 0;
    for _ in 0..4 {
        ctx.world.get_mut::<Transform3D>(mover).unwrap().position.x += 0.5;
        let (differing, kept, _, _, _) = draw(&mut renderers, &mut frames, &mut ctx);
        stale += usize::from(kept & far != 0 && differing > 0);
    }
    assert!(stale > 0, "a lagging far cascade shows in the comparison");
}
