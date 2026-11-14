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
    let mut compare = |ctx: &mut Ctx, equal: bool| {
        let [cached, reference] = &mut renderers;
        let [a, b] = &mut frames;
        let image = rig.draw(cached, a, ctx);
        // Bypass both cache layers in the reference, including deferred turns.
        *b = Frame3D::default();
        reference.forget_shadows();
        let expected = rig.draw(reference, b, ctx);
        assert_eq!(reference.shadow_cascades_redrawn(), 4);
        let matches = image == expected;
        if equal {
            assert!(matches, "cached shadow bands differ from a fresh draw");
        }
        (image, a.redrawn, cached.shadow_cascades_redrawn(), matches)
    };
    compare(&mut ctx, true);
    let (with_proxy, collected, redrawn, _) = compare(&mut ctx, true);
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
    let (without_proxy, _, redrawn, _) = compare(&mut ctx, true);
    assert_eq!(redrawn, 4, "mesh shadow flags invalidate cached cascades");
    assert_ne!(
        with_proxy, without_proxy,
        "shadow-only geometry must cast a visible sun shadow"
    );
    ctx.gfx.set_mesh_shadows(proxy, true);
    compare(&mut ctx, true);
    ctx.scene3d.camera.position.z = 6.5;
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let mut refreshed = 0;
    let mut stale_seen = false;
    let refresh = ctx.scene3d.lighting.shadow_refresh;
    for turn in 0..refresh {
        let (_, collected, _, matches) = compare(&mut ctx, turn + 1 == refresh);
        assert!(
            (collected & 0b1100).count_ones() <= 1,
            "band crossings preserve far-cascade turns"
        );
        refreshed |= collected;
        stale_seen |= !matches;
    }
    assert!(
        stale_seen,
        "the far caster must exercise deferred shadow content before its turn"
    );
    assert_eq!(refreshed, 0b1111, "every cascade converges within one refresh cycle");
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
