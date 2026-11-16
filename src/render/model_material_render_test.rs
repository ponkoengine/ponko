//! `ModelMaterial` and `ModelMaterials` drawn offscreen: one mesh, uploaded
//! once, drawn with its own material and with others, in both prepass
//! modes; per part; and on skinned, morphing, LOD and terrain models.
//! Skips without a GPU (a skip is not a pass).
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::anim::{Joint, MorphTarget, Skeleton, SkinOf};
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{LodGroup, LodLevel, LodRenderer, Material, MaterialDesc, Mesh, MeshData, Model, ModelMaterial};
#[allow(unused_imports)]
use crate::terrain::TerrainTile;
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
    fn new(solid_prepass: bool) -> Option<Self> {
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
        let renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, solid_prepass);
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

    /// A material of `desc`, on the CPU and the GPU.
    fn material(&mut self, desc: MaterialDesc) -> Material {
        let material = self.ctx.gfx.create_material(desc);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        material
    }

    /// A mesh of `data` with `desc`, on the CPU and the GPU.
    fn mesh(&mut self, data: &MeshData, desc: MaterialDesc) -> Mesh {
        let material = self.material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// A mesh of `data` in parts, each with its own material, on the CPU
    /// and the GPU. `skeleton`: skinned to it.
    fn mesh_parts(
        &mut self,
        data: &MeshData,
        parts: &[(std::ops::Range<u32>, MaterialDesc)],
        skeleton: Option<&Skeleton>,
    ) -> Mesh {
        let parts: Vec<_> = parts
            .iter()
            .map(|(range, desc)| (range.clone(), self.material(*desc)))
            .collect();
        let mesh = match skeleton {
            Some(skeleton) => self.ctx.gfx.create_skinned_mesh(data, parts[0].1, skeleton),
            None => self.ctx.gfx.create_mesh(data, parts[0].1),
        };
        let mut weighted = data.clone();
        for w in &mut weighted.weights {
            *w = crate::three::normalized_weights(*w);
        }
        self.renderer.create_mesh(&self.device, &weighted, &parts, true);
        mesh
    }

    /// Draws the world and reads the pixels back (RGBA).
    fn draw(&mut self) -> (Vec<u8>, usize) {
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
            // No light: the cubes glow on black.
            light: SkyLight {
                sun: Vec3::ZERO,
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
        (readback.get_mapped_range(..).unwrap().to_vec(), frame.runs.len())
    }
}

/// A material that shows as a flat `color`, whatever the light.
fn glowing(color: Color) -> MaterialDesc {
    MaterialDesc {
        color: Color::rgb(0.0, 0.0, 0.0),
        emissive: color,
        roughness: 1.0,
        ..Default::default()
    }
}

/// Red, green and blue pixels in an RGBA image.
fn count(pixels: &[u8]) -> [usize; 3] {
    let mut c = [0; 3];
    for p in pixels.as_chunks::<4>().0 {
        let [r, g, b] = [p[0], p[1], p[2]].map(u32::from);
        for (i, (on, off)) in [(r, g + b), (g, r + b), (b, r + g)].into_iter().enumerate() {
            if on > 120 && off < 80 {
                c[i] += 1;
            }
        }
    }
    c
}

/// Red, green and blue pixels in a 9x9 box around where `at` shows.
fn around(pixels: &[u8], view_proj: Mat4, at: Vec3) -> [usize; 3] {
    let ndc = view_proj.project_point3(at);
    let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
    let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
    let mut part = Vec::new();
    for yy in y - 4..=y + 4 {
        for xx in x - 4..=x + 4 {
            let i = ((yy * SIZE + xx) * 4) as usize;
            part.extend_from_slice(&pixels[i..i + 4]);
        }
    }
    count(&part)
}

#[test]
fn one_mesh_draws_with_each_models_material() {
    for solid_prepass in [true, false] {
        let Some(mut rig) = Rig::new(solid_prepass) else {
            return;
        };
        let cube = rig.mesh(&MeshData::cube(1.6), glowing(Color::rgb(0.0, 1.0, 0.0)));
        let red = rig.material(glowing(Color::rgb(1.0, 0.0, 0.0)));
        let blue = rig.material(glowing(Color::rgb(0.0, 0.0, 1.0)));
        // Shadow maps on: the overrides go through the shadow passes too.
        rig.ctx.scene3d.lighting.shadows = true;
        rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 8.0);
        rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
        let world = &mut rig.ctx.world;
        world.spawn((
            Transform3D::at(vec3(-2.4, 0.0, 0.0)),
            Model::new(cube),
            ModelMaterial(red),
        ));
        world.spawn((Transform3D::at(Vec3::ZERO), Model::new(cube)));
        world.spawn((
            Transform3D::at(vec3(2.4, 0.0, 0.0)),
            Model::new(cube),
            ModelMaterial(blue),
        ));
        world.spawn((
            Transform3D::at(vec3(2.4, 2.4, 0.0)),
            Model::new(cube),
            ModelMaterial(blue),
        ));
        let (pixels, runs) = rig.draw();
        let view_proj = rig.ctx.scene3d.camera.view_proj();
        let at = |p: &[u8], x: f32| around(p, view_proj, vec3(x, 0.0, 0.0));
        let [left, middle, right] = [-2.4, 0.0, 2.4].map(|x| at(&pixels, x));
        eprintln!("prepass {solid_prepass}: {left:?} {middle:?} {right:?}, {runs} runs");
        assert!(
            left[0] == 81 && left[1] == 0 && left[2] == 0,
            "left: red only: {left:?}"
        );
        assert!(
            middle[1] == 81 && middle[0] == 0 && middle[2] == 0,
            "middle: its own green: {middle:?}"
        );
        assert!(
            right[2] == 81 && right[0] == 0 && right[1] == 0,
            "right: blue only: {right:?}"
        );
        // Plain, red, and both blue cubes in one run.
        assert_eq!(runs, 3);

        // Taking the override off draws the mesh's own material again.
        let blues: Vec<_> = rig
            .ctx
            .world
            .query::<(ponko_ecs::Entity, &ModelMaterial)>()
            .iter()
            .filter(|(_, m)| m.0 == blue)
            .map(|(e, _)| e)
            .collect();
        for e in blues {
            rig.ctx.world.remove::<ModelMaterial>(e);
        }
        let (pixels, runs) = rig.draw();
        let right = at(&pixels, 2.4);
        assert!(right[1] == 81 && right[2] == 0, "right: green again: {right:?}");
        assert_eq!(runs, 2);
    }
}

/// Two cubes in one mesh: the left one's triangles first, then the right
/// one's, so each can be a part. Their centers are 1.2 left and right.
fn two_cubes() -> (MeshData, [std::ops::Range<u32>; 2]) {
    let mut data = MeshData::cube(1.2);
    let right = MeshData::cube(1.2);
    let corners = data.positions.len() as u32;
    let half = data.indices.len() as u32;
    for p in &mut data.positions {
        p.x -= 1.2;
    }
    data.positions
        .extend(right.positions.iter().map(|p| *p + vec3(1.2, 0.0, 0.0)));
    data.normals.extend(right.normals);
    data.uvs.extend(right.uvs);
    data.indices.extend(right.indices.iter().map(|i| i + corners));
    (data, [0..half, half..half * 2])
}

const RED: Color = Color::rgb(1.0, 0.0, 0.0);
const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

/// Which of red, green and blue fills the 9x9 box around `at`: 0, 1 or 2.
#[track_caller]
fn shows(pixels: &[u8], view_proj: Mat4, at: Vec3) -> usize {
    let c = around(pixels, view_proj, at);
    let most = (0..3).max_by_key(|&i| c[i]).unwrap();
    assert!(
        c[most] == 81 && c.iter().sum::<usize>() == 81,
        "one color at {at}: {c:?}"
    );
    most
}

#[test]
fn listed_parts_draw_with_their_materials_and_the_rest_keep_theirs() {
    for solid_prepass in [true, false] {
        let Some(mut rig) = Rig::new(solid_prepass) else {
            return;
        };
        let (data, [left, right]) = two_cubes();
        let pair = rig.mesh_parts(&data, &[(left, glowing(GREEN)), (right, glowing(GREEN))], None);
        let red = rig.material(glowing(RED));
        let blue = rig.material(glowing(BLUE));
        rig.ctx.scene3d.lighting.shadows = true;
        rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 12.0);
        rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
        let world = &mut rig.ctx.world;
        let at = |y: f32| Transform3D::at(vec3(0.0, y, 0.0));
        // Top: part 1 red. Middle: as made. Bottom: all blue, part 1 red.
        let top = world.spawn((
            at(2.6),
            Model::new(pair),
            crate::render::ModelMaterials::new(&[(1, red)]),
        ));
        world.spawn((at(0.0), Model::new(pair)));
        world.spawn((
            at(-2.6),
            Model::new(pair),
            ModelMaterial(blue),
            crate::render::ModelMaterials::new(&[(1, red)]),
        ));
        let (pixels, runs) = rig.draw();
        let view_proj = rig.ctx.scene3d.camera.view_proj();
        let row = |p: &[u8], y: f32| [-1.2, 1.2].map(|x| shows(p, view_proj, vec3(x, y, 0.0)));
        assert_eq!(row(&pixels, 2.6), [1, 0], "top: green, red");
        assert_eq!(row(&pixels, 0.0), [1, 1], "middle: green, green");
        assert_eq!(row(&pixels, -2.6), [2, 0], "bottom: blue, red");
        assert_eq!(runs, 3);

        // A part the mesh doesn't have is ignored; the same list on another
        // model shares its run.
        let world = &mut rig.ctx.world;
        *world.get_mut::<crate::render::ModelMaterials>(top).unwrap() =
            crate::render::ModelMaterials::new(&[(1, red), (7, blue)]);
        world.spawn((
            Transform3D::at(vec3(0.0, 2.6, -3.0)),
            Model::new(pair),
            crate::render::ModelMaterials::new(&[(7, blue), (1, red)]),
        ));
        let (pixels, runs) = rig.draw();
        assert_eq!(row(&pixels, 2.6), [1, 0], "top: still green, red");
        assert_eq!(runs, 3);

        // Changing the list changes the picture (and the cached shadow maps' key).
        *rig.ctx.world.get_mut::<crate::render::ModelMaterials>(top).unwrap() =
            crate::render::ModelMaterials::new(&[(0, blue)]);
        let (pixels, _) = rig.draw();
        assert_eq!(row(&pixels, 2.6), [2, 1], "top: blue, green");
    }
}

#[test]
fn skinned_and_morphing_models_draw_with_the_override() {
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    let skeleton = Skeleton::new(vec![Joint::new("root", None)]);
    let (mut bent, [left, right]) = two_cubes();
    bent.joints = vec![[0; 4]; bent.positions.len()];
    bent.weights = vec![[1.0, 0.0, 0.0, 0.0]; bent.positions.len()];
    let skinned = rig.mesh_parts(
        &bent,
        &[(left.clone(), glowing(GREEN)), (right.clone(), glowing(GREEN))],
        Some(&skeleton),
    );
    let (mut shaped, _) = two_cubes();
    shaped
        .morph_targets
        .push(MorphTarget::new("still", vec![Vec3::ZERO; shaped.positions.len()]));
    let morphing = rig.mesh_parts(&shaped, &[(left, glowing(GREEN)), (right, glowing(GREEN))], None);
    let red = rig.material(glowing(RED));
    let blue = rig.material(glowing(BLUE));
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 14.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let world = &mut rig.ctx.world;
    let at = |y: f32| Transform3D::at(vec3(0.0, y, 0.0));
    // Skinned: as made; all red; all red with part 1 blue.
    world.spawn((at(3.9), Model::new(skinned), skeleton.clone()));
    let owner = world.spawn((at(1.3), Model::new(skinned), skeleton.clone(), ModelMaterial(red)));
    world.spawn((
        at(-1.3),
        Model::new(skinned),
        skeleton,
        ModelMaterial(red),
        crate::render::ModelMaterials::new(&[(1, blue)]),
    ));
    // Morphing, not skinned: part 0 blue.
    world.spawn((
        at(-3.9),
        Model::new(morphing),
        crate::anim::MorphWeights::default(),
        crate::render::ModelMaterials::new(&[(0, blue)]),
    ));
    let (pixels, _) = rig.draw();
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let row = |p: &[u8], y: f32| [-1.2, 1.2].map(|x| shows(p, view_proj, vec3(x, y, 0.0)));
    assert_eq!(row(&pixels, 3.9), [1, 1], "skinned, as made: green");
    assert_eq!(row(&pixels, 1.3), [0, 0], "skinned with a material: red");
    assert_eq!(row(&pixels, -1.3), [0, 2], "skinned with both: red, blue");
    assert_eq!(row(&pixels, -3.9), [2, 1], "morphing with a part list: blue, green");

    // A model that borrows another's skeleton takes its own entity's override.
    let world = &mut rig.ctx.world;
    world.remove::<ModelMaterial>(owner);
    world.spawn((Model::new(skinned), SkinOf(owner), ModelMaterial(blue)));
    let (pixels, _) = rig.draw();
    // Both are at the owner's place; whichever is in front, none is red.
    let both = around(&pixels, view_proj, vec3(-1.2, 1.3, 0.0));
    assert!(
        both[0] == 0 && both[1] + both[2] == 81,
        "green or blue, no red: {both:?}"
    );
    assert!(
        count(&pixels)[2] > 81,
        "the borrowed skeleton's model is blue somewhere"
    );
}

#[test]
fn lod_groups_and_terrain_tiles_draw_with_the_override() {
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    let (data, [left, right]) = two_cubes();
    let pair = rig.mesh_parts(&data, &[(left, glowing(GREEN)), (right, glowing(GREEN))], None);
    let red = rig.material(glowing(RED));
    let blue = rig.material(glowing(BLUE));
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 14.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let world = &mut rig.ctx.world;
    let at = |y: f32| Transform3D::at(vec3(0.0, y, 0.0));
    let group = || LodGroup::new(4.0, vec![LodLevel::new(0.0, vec![LodRenderer::new(pair)])]);
    world.spawn((at(3.9), group()));
    world.spawn((at(1.3), group(), ModelMaterial(red)));
    world.spawn((at(-1.3), group(), crate::render::ModelMaterials::new(&[(1, blue)])));
    let tile = TerrainTile {
        lods: [pair; crate::terrain::TerrainTile::LODS],
        lod_distance: 100.0,
    };
    world.spawn((
        at(-3.9),
        tile,
        ModelMaterial(blue),
        crate::render::ModelMaterials::new(&[(0, red)]),
    ));
    let (pixels, runs) = rig.draw();
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let row = |p: &[u8], y: f32| [-1.2, 1.2].map(|x| shows(p, view_proj, vec3(x, y, 0.0)));
    assert_eq!(row(&pixels, 3.9), [1, 1], "group, as made: green");
    assert_eq!(row(&pixels, 1.3), [0, 0], "group with a material: red");
    assert_eq!(row(&pixels, -1.3), [1, 2], "group with a part list: green, blue");
    assert_eq!(row(&pixels, -3.9), [0, 2], "terrain with both: red, blue");
    assert_eq!(runs, 4);
}
