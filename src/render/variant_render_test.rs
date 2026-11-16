//! Every documented material shader variant (see `gpu3d/variants.rs` and the
//! guide's "Shader contract" section) built and drawn offscreen on the real
//! adapter: every pipeline role of every variant compiles without a
//! validation error, and opaque, cut-out and transparent materials, one- and
//! double-sided, at 2x and 4x multisampling, draw through a custom
//! shader with both hooks. Also pins how geometry moved by `vertex` is
//! culled. Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::Blend;
#[allow(unused_imports)]
use crate::render::{MaterialDesc, Mesh, MeshData, Model, PipelineStats, Shader};
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
}

impl Rig {
    /// `None` without a GPU.
    fn new(samples: u32) -> Option<Self> {
        let gpu = crate::render::render_test_support::gpu_with(|adapter| {
            if !multisamples(adapter, samples) {
                return Err(format!("{samples}x multisampling unsupported"));
            }
            // The device the game asks for (`gpu.rs`): 2x multisampling needs
            // the adapter's own format features, and the decal and GI texture
            // slots one more sampled texture than the default 16.
            let descriptor = wgpu::DeviceDescriptor {
                required_features: adapter.features() & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                required_limits: wgpu::Limits {
                    max_sampled_textures_per_shader_stage: adapter
                        .limits()
                        .max_sampled_textures_per_shader_stage
                        .clamp(16, 24),
                    ..wgpu::Limits::default()
                },
                ..Default::default()
            };
            Ok(descriptor)
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
        })
    }

    /// A shader made from `source`, on the CPU and the GPU.
    fn shader(&mut self, source: &str) -> Shader {
        let shader = self.ctx.gfx.create_shader("test", source);
        self.renderer.create_shader(&self.device, "test", source).unwrap();
        shader
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
        let ctx = &mut self.ctx;
        let mut skins = SkinFrame::default();
        skins.collect(&mut ctx.world, &ctx.gfx.assets3d);
        let mut frame = Frame3D::default();
        frame.lod = ctx.scene3d.lod;
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &skins,
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        frame.skins = skins;
        let scene = Scene3D {
            frame: &frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            // Sunlight only in the shadow scene: elsewhere the cubes glow
            // on black.
            light: SkyLight {
                sun: if ctx.scene3d.lighting.shadows {
                    Vec3::splat(ctx.scene3d.lighting.sun_intensity)
                } else {
                    Vec3::ZERO
                },
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

/// A custom shader with both hooks: `vertex` moves every vertex by
/// `material.custom[0].xyz` (world units), `surface` cuts the half of the
/// mesh with `uv.x < 0.5` (alpha 0 there) and gives the rest the alpha in
/// `material.custom[1].x`, lit only by `emissive`.
const BOTH_HOOKS: &str = "
fn vertex(v: Vertex) -> Vertex {
    var o = v;
    o.position += material.custom[0].xyz;
    return o;
}
fn surface(s: Surface) -> Surface {
    var o = s;
    o.alpha = select(0.0, material.custom[1].x, s.uv.x >= 0.5);
    o.albedo = vec3<f32>(0.0);
    return o;
}";

/// Solid blends keep the uncut half (alpha 1); transparent shows it at 0.3.
fn material(shader: Shader, blend: Blend, double_sided: bool) -> MaterialDesc {
    let alpha = if blend == Blend::Transparent { 0.3 } else { 1.0 };
    MaterialDesc {
        custom: [Vec4::ZERO, vec4(alpha, 0.0, 0.0, 0.0), Vec4::ZERO, Vec4::ZERO],
        color: Color::rgb(0.0, 0.0, 0.0),
        emissive: Color::WHITE,
        roughness: 1.0,
        blend,
        double_sided,
        shader: Some(shader),
        ..Default::default()
    }
}

/// How many pixels are lit (any channel above 12).
fn lit(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[..3].iter().any(|&c| c > 12))
        .count()
}

/// The brightest red value.
fn brightest(pixels: &[u8]) -> u8 {
    pixels.as_chunks::<4>().0.iter().map(|p| p[0]).max().unwrap_or(0)
}

/// Runs `f`, then fails on any validation error the GPU reported meanwhile.
fn no_validation_errors<T>(device: &wgpu::Device, what: &str, f: impl FnOnce() -> T) -> T {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let out = f();
    if let Some(e) = pollster::block_on(scope.pop()) {
        panic!("{what}: {e}");
    }
    out
}

#[test]
fn every_documented_variant_builds_every_role_on_the_adapter() {
    for samples in [2, 4] {
        for source in [
            BOTH_HOOKS,
            include_str!("../../tests/fixtures/shaders/material-unlit.wgsl"),
        ] {
            let Some(mut rig) = Rig::new(samples) else { continue };
            let device = rig.device.clone();
            let id = no_validation_errors(&device, "creating the shader", || {
                rig.renderer
                    .create_shader(&rig.device, "material hooks", source)
                    .unwrap()
            });
            let mut built = 0;
            for variant in ShaderVariant::ALL {
                for role in PipelineRole::ALL {
                    let pipeline = no_validation_errors(&device, &format!("{variant:?} {role:?} x{samples}"), || {
                        rig.renderer.shader_pipeline(id, variant, role).is_some()
                    });
                    // The 3D frame is always multisampled (2x or 4x), so the
                    // coverage roles exist too.
                    assert!(pipeline, "{variant:?} {role:?} x{samples}");
                    built += usize::from(pipeline);
                }
            }
            assert!(rig.renderer.shader_warm(id));
            assert_eq!(built, ShaderVariant::ALL.len() * PipelineRole::ALL.len());
            eprintln!("x{samples}: {built} pipelines built and validated");
        }
    }
}

/// A big plane under the camera (seen from its front) or over it (its
/// back), filling the screen, with shadows on so the depth and shadow
/// passes run too. Returns the pixels.
fn draw_plane(samples: u32, blend: Blend, double_sided: bool, from_below: bool) -> Option<Vec<u8>> {
    let mut rig = Rig::new(samples)?;
    rig.ctx.scene3d.lighting.shadows = true;
    let shader = rig.shader(BOTH_HOOKS);
    let desc = material(shader, blend, double_sided);
    let plane = rig.mesh(&MeshData::plane(1.0), desc);
    rig.ctx.world.spawn((
        Transform3D {
            scale: Vec3::splat(40.0),
            ..Default::default()
        },
        Model::new(plane),
    ));
    let y = if from_below { -6.0 } else { 6.0 };
    rig.ctx.scene3d.camera.position = vec3(0.0, y, 0.001);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let device = rig.device.clone();
    Some(no_validation_errors(
        &device,
        &format!("{blend:?} double_sided={double_sided} x{samples}"),
        || rig.draw(),
    ))
}

#[test]
fn every_blend_and_side_draws_through_a_custom_shader() {
    let all = (SIZE * SIZE) as usize;
    for samples in [2, 4] {
        for blend in [Blend::Opaque, Blend::Cutout, Blend::Transparent] {
            for double_sided in [false, true] {
                let what = format!("{blend:?} double_sided={double_sided} x{samples}");
                let Some(front) = draw_plane(samples, blend, double_sided, false) else {
                    continue;
                };
                let back = draw_plane(samples, blend, double_sided, true).unwrap();
                let (front_lit, back_lit) = (lit(&front), lit(&back));
                // Opaque ignores alpha: the whole screen. Cut-outs lose the
                // half `surface` cut; transparent shows that half not at all
                // (alpha 0) and the other at 0.3 (dimmer than opaque).
                match blend {
                    Blend::Opaque => assert!(front_lit > all * 95 / 100, "{what}: {front_lit}"),
                    _ => assert!(
                        front_lit > all * 40 / 100 && front_lit < all * 60 / 100,
                        "{what}: half the screen, {front_lit}"
                    ),
                }
                let full = brightest(&front);
                // Emissive 1 tone-maps to about 232; 0.3 of it blended
                // over black shows clearly darker.
                if blend == Blend::Transparent {
                    assert!(full > 60 && full < 200, "{what}: blended, {full}");
                } else {
                    assert!(full > 220, "{what}: solid, {full}");
                }
                // The back shows only when double-sided.
                if double_sided {
                    assert!(
                        back_lit.abs_diff(front_lit) < all / 20,
                        "{what}: {back_lit} vs {front_lit}"
                    );
                } else {
                    assert_eq!(back_lit, 0, "{what}: back faces culled");
                }
                eprintln!("{what}: front {front_lit}, back {back_lit}, brightest {full}");
            }
        }
    }
}

/// A 2-unit cube at `x`, moved by its `vertex` hook by `shift`, drawn by a
/// camera looking at the origin from 10 units away; with `grow`, its
/// culling bounds also hold where the hook puts it. Returns lit pixels.
fn displaced_cube(x: f32, shift: f32, grow: bool) -> Option<usize> {
    let mut rig = Rig::new(4)?;
    let shader = rig.shader(BOTH_HOOKS);
    let mut desc = material(shader, Blend::Opaque, false);
    desc.custom[0] = vec4(shift, 0.0, 0.0, 0.0);
    let cube = rig.mesh(&MeshData::cube(2.0), desc);
    if grow {
        rig.ctx.gfx.set_mesh_gpu_bounds(cube, vec3(shift, 0.0, 0.0), 2.0);
    }
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(x, 0.0, 0.0),
            ..Default::default()
        },
        Model::new(cube),
    ));
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 10.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    Some(lit(&rig.draw()))
}

#[test]
fn culling_uses_the_mesh_bounds_before_vertex_displacement() {
    let Some(still) = displaced_cube(0.0, 0.0, false) else {
        return;
    };
    assert!(still > 100, "the cube shows: {still}");
    // Moved into view from outside it: culled by where its bounds are.
    let into_view = displaced_cube(60.0, -60.0, false).unwrap();
    assert_eq!(into_view, 0, "culled by its undisplaced bounds");
    // Bounds grown over where the hook puts it: drawn there.
    let grown = displaced_cube(60.0, -60.0, true).unwrap();
    assert!(grown.abs_diff(still) < still / 10, "{grown} vs {still}");
    // Moved out of view from inside it: drawn (off screen), so not seen.
    let out_of_view = displaced_cube(0.0, 60.0, false).unwrap();
    assert_eq!(out_of_view, 0);
    // A small displacement within view is drawn where `vertex` put it.
    let nudged = displaced_cube(0.0, 1.0, false).unwrap();
    assert!(nudged.abs_diff(still) < still / 5, "{nudged} vs {still}");
}

/// Draws a plane with `BOTH_HOOKS` through shader `id` (made by `make`,
/// which may make other shaders first), cut out, at 4x multisampling.
fn plane_with(make: impl FnOnce(&mut Rig) -> Shader) -> Option<(Vec<u8>, crate::render::PipelineStats)> {
    let mut rig = Rig::new(4)?;
    rig.ctx.scene3d.lighting.shadows = true;
    let shader = make(&mut rig);
    let plane = rig.mesh(&MeshData::plane(1.0), material(shader, Blend::Cutout, false));
    rig.ctx.world.spawn((
        Transform3D {
            scale: Vec3::splat(40.0),
            ..Default::default()
        },
        Model::new(plane),
    ));
    rig.ctx.scene3d.camera.position = vec3(0.0, 6.0, 0.001);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let pixels = rig.draw();
    Some((pixels, rig.renderer.pipeline_stats()))
}

#[test]
fn a_shader_sharing_another_ones_pipelines_draws_the_same_pixels() {
    let Some((own, own_stats)) = plane_with(|rig| rig.shader(BOTH_HOOKS)) else {
        return;
    };
    let Some((shared, shared_stats)) = plane_with(|rig| {
        rig.shader(BOTH_HOOKS);
        rig.shader(BOTH_HOOKS)
    }) else {
        return;
    };
    assert_eq!(
        shared_stats.shared,
        own_stats.shared + 1,
        "the second shader shared the first's set"
    );
    assert_eq!(shared_stats.compiles, own_stats.compiles);
    assert!(lit(&own) > 0);
    assert_eq!(own, shared, "identical pixels through the shared pipelines");
}

/// Every orthogonal material feature with a posed mesh and with static
/// persistent instances. The old blend/back-face test supplies negative
/// controls for coverage; this test compares lighting on/off as well.
#[test]
fn lit_and_unlit_features_render_on_skinned_and_instanced_geometry() {
    use crate::three::{Joint, Skeleton};
    for blend in [Blend::Opaque, Blend::Cutout, Blend::Transparent] {
        for double_sided in [false, true] {
            for path in ["plain", "skinned", "batch"] {
                for unlit in [false, true] {
                    let Some(mut rig) = Rig::new(4) else { continue };
                    let source = include_str!("../../tests/fixtures/shaders/material-unlit.wgsl")
                        .replace("out.unlit = true;", &format!("out.unlit = {unlit};"));
                    let shader = rig.shader(&source);
                    let desc = MaterialDesc {
                        color: Color::rgba(1.0, 1.0, 1.0, if blend == Blend::Transparent { 0.3 } else { 1.0 }),
                        blend,
                        double_sided,
                        shader: Some(shader),
                        roughness: 1.0,
                        ..Default::default()
                    };
                    let mut data = MeshData::plane(1.0);
                    let skeleton = Skeleton::new(vec![Joint::new("root", None)]);
                    let material = rig.ctx.gfx.create_material(desc);
                    let mesh = if path == "skinned" {
                        data.joints = vec![[0; 4]; data.positions.len()];
                        data.weights = vec![[1.0, 0.0, 0.0, 0.0]; data.positions.len()];
                        rig.ctx.gfx.create_skinned_mesh(&data, material, &skeleton)
                    } else {
                        rig.ctx.gfx.create_mesh(&data, material)
                    };
                    rig.renderer
                        .create_material(&rig.device, &GpuMaterialDesc::new(&desc), &rig.textures);
                    rig.renderer
                        .create_mesh(&rig.device, &data, &[(0..data.indices.len() as u32, material)], true);
                    let transform = Transform3D {
                        scale: Vec3::splat(40.0),
                        ..Default::default()
                    };
                    if path == "batch" {
                        let b = rig.ctx.gfx.create_instance_batch(
                            mesh,
                            &[Mat4::from_scale(Vec3::splat(40.0))],
                            crate::three::InstanceBatchOptions {
                                culling: crate::three::BatchCulling::PreferGpu,
                                ..Default::default()
                            },
                        );
                        rig.renderer.set_instance_batch(
                            &rig.device,
                            &rig.queue,
                            b.id,
                            rig.ctx.gfx.assets3d.instance_batches[b.id as usize].as_ref(),
                        );
                    } else if path == "skinned" {
                        rig.ctx.world.spawn((transform, Model::new(mesh), skeleton));
                    } else {
                        rig.ctx.world.spawn((transform, Model::new(mesh)));
                    }
                    rig.ctx.scene3d.camera.position = vec3(0.0, 6.0, 0.001);
                    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
                    // No lights: lit is black, unlit retains its albedo.
                    rig.ctx.scene3d.lighting.shadows = false;
                    let device = rig.device.clone();
                    let dark = no_validation_errors(&device, &source, || rig.draw());
                    let count = lit(&dark);
                    let all = (SIZE * SIZE) as usize;
                    if unlit {
                        match blend {
                            Blend::Opaque => assert!(count > all * 95 / 100, "{path} {blend:?}: {count}"),
                            _ => assert!(
                                count > all * 40 / 100 && count < all * 60 / 100,
                                "{path} {blend:?}: {count}"
                            ),
                        }
                    } else {
                        assert_eq!(count, 0, "unlit negative control {path} {blend:?}");
                    }
                    rig.ctx.scene3d.lighting.shadows = true;
                    let bright = no_validation_errors(&device, &source, || rig.draw());
                    if std::env::var_os("PONKO_MATERIAL_IMAGES").is_some() && path == "plain" && !double_sided {
                        let dir = std::path::Path::new("target/material-contract-images");
                        std::fs::create_dir_all(dir).unwrap();
                        for (name, pixels) in [("dark", &dark), ("sun", &bright)] {
                            image::save_buffer(
                                dir.join(format!("{blend:?}-unlit-{unlit}-{name}.png")),
                                pixels,
                                SIZE,
                                SIZE,
                                image::ColorType::Rgba8,
                            )
                            .unwrap();
                        }
                    }
                    if unlit {
                        assert_eq!(dark, bright, "unlit ignores sunlight: {path} {blend:?}");
                    } else {
                        assert!(lit(&bright) > all * 30 / 100, "lit receives sunlight: {path} {blend:?}");
                    }
                }
            }
        }
    }
}
