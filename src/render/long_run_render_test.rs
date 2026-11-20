//! A long offscreen run: 2000 frames that each make a texture, a material
//! reading it, a mesh with that material and an instance cell, draw them
//! with the sun's cached shadows on, and free what an earlier frame made;
//! a render target with its own camera comes and goes every few dozen
//! frames. The GPU memory report (`GpuMemory`) is the same at frame 1000 as
//! at frame 2000, and back at the baseline once everything is freed; the
//! texture and mesh tables reuse freed slots instead of growing.
//! Handles go to the offscreen `Renderer3D` the way the window's renderer
//! hands them over. Skips without a GPU (a skip is not a pass).
use std::collections::VecDeque;

use super::gpu::GpuTexture;
use super::gpu_textures::GpuTextures;
use super::gpu3d::*;
use super::memory::{MemoryClass as C, MemoryTally};
use super::render_test_support as support;
use crate::d3::Transform as Transform3D;
use crate::handle::INDEX_MASK;
use crate::perf::GpuMemory;
use crate::prelude::*;
use crate::render::{
    Compute, ComputePass, Effect, GpuBuffer, InstanceBatch, InstanceCell, Material, MaterialDesc, Mesh, MeshData,
    Model, ScreenEffect, Shader, TargetCamera,
};
use crate::terrain::{TextureArray, TextureKind};
use crate::three::{Frame3D, Scene3D, SkinFrame, SkyLight};

const SIZE: (u32, u32) = (128, 128);
/// Frames of churn, and how many frames' worth of things are live at once.
const FRAMES: u32 = 2000;
const KEPT: usize = 8;
/// A render target is made this often and freed as many frames later.
const TARGET_EVERY: u32 = 40;
/// The churned textures' and render targets' sizes.
const TEXTURE: u32 = 32;
const TARGET: u32 = 64;

/// What one frame made, freed together later.
struct Made {
    texture: Texture,
    material: Material,
    mesh: Mesh,
    model: Entity,
    cell: InstanceCell,
}

/// Resources exchanged less often than meshes, alongside each camera target.
#[derive(Clone, Copy)]
struct Programs {
    array: TextureArray,
    shader: Shader,
    effect: Effect,
    compute: Compute,
    material: Material,
}

/// An offscreen 3D renderer and a headless `Ctx` whose handles it follows,
/// with the window renderer's texture table (`textures`, by texture id),
/// which reuses freed slots as the window's does.
struct Rig {
    gpu: support::TestGpu,
    layout: wgpu::BindGroupLayout,
    output: wgpu::Texture,
    target: wgpu::TextureView,
    textures: GpuTextures,
    renderer: Renderer3D,
    ctx: Ctx,
    frame: Frame3D,
    frames: u32,
}

impl Rig {
    fn new() -> Option<Self> {
        let gpu = support::gpu()?;
        let (layout, white, textures) = support::white_textures(&gpu.device, &gpu.queue);
        let output = support::target(&gpu.device, SIZE, wgpu::TextureFormat::Rgba8UnormSrgb);
        let target = output.create_view(&Default::default());
        let renderer = Renderer3D::new(&gpu.device, &layout, output.format(), 0, &white, 4, true);
        // The white texture is id 0 (the game's ids start at 1), and what
        // freed textures show.
        let white = textures.into_iter().next().expect("the white texture");
        let mut textures = GpuTextures::default();
        textures.set_blank(white.clone());
        assert_eq!(textures.add(white), 0);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE.0 as f32, SIZE.1 as f32));
        // No renderer of its own: this test hands the GPU work over.
        ctx.set_headless(true);
        ctx.scene3d.sky.atmosphere = false;
        // Low: the sun's cached shadows on, at the smallest maps, so 2000
        // frames stay quick on a busy machine.
        ctx.scene3d.set_quality(crate::render::GraphicsQuality::Low);
        ctx.scene3d.lighting.shadows = true;
        ctx.scene3d.camera.position = vec3(0.0, 6.0, 14.0);
        ctx.scene3d.camera.look_at(Vec3::ZERO);
        Some(Self {
            gpu,
            layout,
            output,
            target,
            textures,
            renderer,
            ctx,
            frame: Frame3D::default(),
            frames: 0,
        })
    }

    /// Puts `texture` in the table, under the id the game gave it.
    fn put(&mut self, id: u32, texture: wgpu::Texture) {
        let view = texture.create_view(&Default::default());
        let sampler = self.gpu.device.create_sampler(&Default::default());
        let bind_group = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
        let added = self.textures.add(GpuTexture {
            bind_group,
            texture,
            nearest: false,
            mips: None,
        });
        assert_eq!(added, id, "the renderer's ids follow the game's");
    }

    /// A `TEXTURE`² texture of `shade` on both sides.
    fn texture(&mut self, shade: u8) -> Texture {
        let pixels = vec![shade; (TEXTURE * TEXTURE * 4) as usize];
        let texture = self.ctx.gfx.create_texture(TEXTURE, TEXTURE, &pixels);
        let gpu = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("churned texture"),
            size: wgpu::Extent3d {
                width: TEXTURE,
                height: TEXTURE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        self.gpu.queue.write_texture(
            gpu.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TEXTURE * 4),
                rows_per_image: None,
            },
            gpu.size(),
        );
        self.put(texture.id, gpu);
        texture
    }

    /// A render target that a camera draws into every frame, on both sides
    /// (made as `Renderer::create_render_target` makes it).
    fn render_target(&mut self) -> Texture {
        let handle = self.ctx.gfx.create_render_target(TARGET, TARGET);
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render target"),
            size: wgpu::Extent3d {
                width: TARGET,
                height: TARGET,
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
        self.put(handle.texture.id, texture.clone());
        self.renderer.add_render_target(handle.texture.id, &texture);
        let mut camera = self.ctx.scene3d.camera;
        camera.position = vec3(4.0, 3.0, 6.0);
        camera.look_at(Vec3::ZERO);
        self.ctx
            .scene3d
            .passes
            .target_cameras
            .push(TargetCamera::new(camera, handle));
        handle.texture()
    }

    /// Frees texture or render target `texture` as the window's renderer
    /// does: it shows the white texture (its slot goes to the next one), a
    /// target camera drawing into it goes, and the materials that read it
    /// are bound again.
    fn free_texture(&mut self, texture: Texture) {
        self.ctx.free(texture);
        self.ctx
            .scene3d
            .passes
            .target_cameras
            .retain(|c| c.target.texture() != texture);
        assert!(self.textures.free(texture.id));
        self.renderer.free_render_target(texture.id);
        self.renderer.rebind_materials(
            &self.gpu.device,
            &self.gpu.queue,
            &self.textures,
            Some(texture.id),
            None,
        );
    }

    fn material(&mut self, texture: Texture) -> Material {
        self.material_of(MaterialDesc {
            texture: Some(texture),
            roughness: 0.8,
            ..Default::default()
        })
    }

    fn material_of(&mut self, desc: MaterialDesc) -> Material {
        let material = self.ctx.gfx.create_material(desc);
        self.renderer.create_material_at(
            &self.gpu.device,
            material.id,
            &self.ctx.gfx.gpu_material(&desc),
            &self.textures,
        );
        material
    }

    fn free_material(&mut self, material: Material) {
        self.ctx.free(material);
        self.renderer
            .free_material(&self.gpu.device, &self.gpu.queue, material.id, &self.textures);
    }

    fn effect(&mut self, source: &str) -> Effect {
        let effect = self.ctx.gfx.create_effect("churn effect", source);
        self.renderer
            .create_effect_at(&self.gpu.device, effect.id & INDEX_MASK, "churn effect", source)
            .unwrap();
        effect
    }

    fn shader(&mut self, source: &str) -> Shader {
        let shader = self.ctx.gfx.create_shader("churn shader", source);
        self.renderer
            .create_shader_at(&self.gpu.device, (shader.id & INDEX_MASK) + 1, "churn shader", source)
            .unwrap();
        shader
    }

    fn array(&mut self, pixels: [u8; 64]) -> TextureArray {
        let array = self
            .ctx
            .gfx
            .create_texture_array(4, 4, &[pixels.as_slice(); 2], TextureKind::Color);
        let gpu = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("churn texture array"),
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            gpu.as_image_copy(),
            &[pixels.as_slice(), pixels.as_slice()].concat(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: Some(4),
            },
            gpu.size(),
        );
        self.renderer.add_texture_array_at(array.id & INDEX_MASK, &gpu);
        array
    }

    fn free_array(&mut self, array: TextureArray) {
        self.ctx.free(array);
        self.renderer
            .free_texture_array(&self.gpu.device, &self.gpu.queue, array.id & INDEX_MASK, &self.textures);
    }

    fn free_shader(&mut self, shader: Shader) {
        self.ctx.free(shader);
        self.renderer.free_shader((shader.id & INDEX_MASK) + 1);
    }

    fn programs(&mut self) -> Programs {
        const SURFACE: &str = "fn surface(s: Surface) -> Surface {
            var out = s;
            out.albedo *= textureSample(custom_array0, material_sampler, s.uv, 0).rgb;
            return out;
        }";
        const COMPUTE: &str = "fn update(i: u32) { items[i].x += 1.0; }";
        const EFFECT: &str = "fn effect(p: Pixel) -> vec3<f32> { return p.color; }";
        let shader = self.shader(SURFACE);
        let array = self.array([255; 64]);
        let material =
            self.material_of(
                MaterialDesc::default()
                    .with_shader(shader)
                    .with_custom_arrays([Some(array), None, None]),
            );
        let effect = self.effect(EFFECT);
        let compute = self.ctx.gfx.create_compute("churn compute", COMPUTE);
        self.renderer
            .create_compute_at(&self.gpu.device, compute.id & INDEX_MASK, "churn compute", COMPUTE)
            .unwrap();
        Programs {
            array,
            shader,
            effect,
            compute,
            material,
        }
    }

    fn schedule_programs(&mut self, p: Programs, buffer: GpuBuffer) {
        self.ctx
            .scene3d
            .passes
            .effects
            .push(ScreenEffect::new(p.effect).with_material(p.material));
        self.ctx
            .scene3d
            .passes
            .compute
            .push(ComputePass::for_buffer(p.compute, buffer).with_material(p.material));
    }

    fn clear_program_passes(&mut self) {
        self.ctx.scene3d.passes.effects.clear();
        self.ctx.scene3d.passes.compute.clear();
    }

    fn free_programs(&mut self, p: Programs) {
        self.free_shader(p.shader);
        self.free_array(p.array);
        self.ctx.free(p.compute);
        self.renderer.free_compute(p.compute.id & INDEX_MASK);
        self.ctx.free(p.effect);
        self.renderer.free_effect(p.effect.id & INDEX_MASK);
        self.free_material(p.material);
    }

    /// A mesh on both sides, in the slot the game gave it.
    fn mesh(&mut self, data: &MeshData, material: Material) -> Mesh {
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer.create_mesh_at(
            &self.gpu.device,
            mesh.slot(),
            data,
            &[(0..data.indices.len() as u32, material)],
            false,
        );
        mesh
    }

    fn free_mesh(&mut self, mesh: Mesh) {
        self.ctx.free(mesh);
        self.renderer.free_mesh(mesh.slot());
    }

    /// Hands row `row` of the instance batch table to the renderer.
    fn sync_batch(&mut self, row: u32) {
        let data = self.ctx.gfx.assets3d.instance_batches[row as usize].as_ref();
        self.renderer
            .set_instance_batch(&self.gpu.device, &self.gpu.queue, row, data);
    }

    fn cell(&mut self, batch: InstanceBatch, at: Vec3) -> InstanceCell {
        let transforms: Vec<Mat4> = (0..16)
            .map(|i| Mat4::from_translation(at + vec3((i % 4) as f32, 0.0, (i / 4) as f32) * 0.5))
            .collect();
        let cell = self.ctx.gfx.add_instance_cell(batch, &transforms, &[Color::WHITE; 16]);
        let row = self.ctx.gfx.instance_cell_row(cell).expect("uploaded");
        self.sync_batch(row);
        cell
    }

    fn free_cell(&mut self, cell: InstanceCell) {
        let row = self.ctx.gfx.instance_cell_row(cell).expect("live");
        self.ctx.free(cell);
        self.sync_batch(row);
    }

    /// Draws one frame and waits for the GPU, so what it dropped is freed.
    fn draw(&mut self) {
        self.draw_at(f64::from(self.frames) / 60.0);
    }

    fn draw_at(&mut self, time: f64) {
        let (device, queue) = (self.gpu.device.clone(), self.gpu.queue.clone());
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let ctx = &mut self.ctx;
        let mut frame = std::mem::take(&mut self.frame);
        frame.lod = ctx.scene3d.lod;
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        ctx.gfx.frame3d.views = std::mem::take(&mut frame.views);
        ctx.collect_views(&SkinFrame::default());
        frame.views = std::mem::take(&mut ctx.gfx.frame3d.views);
        ctx.collect_passes(None);
        std::mem::swap(&mut frame.effects, &mut ctx.gfx.frame3d.effects);
        std::mem::swap(&mut frame.computes, &mut ctx.gfx.frame3d.computes);
        self.frames += 1;
        let scene = Scene3D {
            frame: &frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: Vec3::splat(ctx.scene3d.lighting.sun_intensity),
                sky: Vec3::splat(0.3),
                ground: Vec3::splat(0.1),
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (time, 1.0 / 60.0),
            occlusion: false,
        };
        self.renderer.prepare(
            &device,
            &queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: SIZE,
            },
        );
        let buffers = self
            .renderer
            .draw(&device, &self.target, wgpu::Color::BLACK, &self.textures, None);
        queue.submit(buffers);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        if let Some(error) = pollster::block_on(scope.pop()) {
            panic!("frame {}: {error}", self.frames);
        }
        std::mem::swap(&mut frame.effects, &mut ctx.gfx.frame3d.effects);
        std::mem::swap(&mut frame.computes, &mut ctx.gfx.frame3d.computes);
        self.frame = frame;
    }

    /// The GPU memory report: the renderer's, and the texture table's (as
    /// `Renderer::gpu_memory` adds it up; each texture once).
    fn memory(&self) -> GpuMemory {
        let mut t = MemoryTally::default();
        self.renderer.tally_memory(&mut t);
        for texture in self.textures.iter() {
            t.texture(C::Textures, "texture", &texture.texture);
        }
        t.texture(C::FrameTargets, "window", &self.output);
        t.finish()
    }
}

/// The classes of two reports that differ, by name.
fn differences(a: &GpuMemory, b: &GpuMemory) -> Vec<(&'static str, u64, u64)> {
    a.classes()
        .iter()
        .zip(b.classes())
        .filter(|(x, y)| x.1 != y.1)
        .map(|(x, y)| (x.0, x.1, y.1))
        .collect()
}

/// One frame of churn: makes a texture, a material reading it, a mesh with
/// that material, a model and an instance cell, and now and then a render
/// target with its camera; frees what frame `f - KEPT` made (and the
/// previous render target); draws. Returns the mesh made.
fn churn(
    rig: &mut Rig,
    f: u32,
    batch: InstanceBatch,
    made: &mut VecDeque<Made>,
    targets: &mut VecDeque<Texture>,
) -> (Mesh, Material) {
    let x = (f % 7) as f32 - 3.0;
    let texture = rig.texture((f % 200) as u8 + 30);
    let material = rig.material(texture);
    let mesh = rig.mesh(&MeshData::cube(0.8), material);
    let model = rig
        .ctx
        .world
        .spawn((Transform3D::at(vec3(x, 0.5, (f % 5) as f32 - 2.0)), Model::new(mesh)));
    let cell = rig.cell(batch, vec3(x, 0.0, 3.0));
    made.push_back(Made {
        texture,
        material,
        mesh,
        model,
        cell,
    });
    if made.len() > KEPT {
        let old = made.pop_front().unwrap();
        free(rig, old);
    }
    if f.is_multiple_of(TARGET_EVERY) {
        targets.push_back(rig.render_target());
        if targets.len() > 1 {
            let old = targets.pop_front().unwrap();
            rig.free_texture(old);
        }
    }
    rig.draw();
    (mesh, material)
}

fn free(rig: &mut Rig, old: Made) {
    rig.ctx.world.despawn(old.model);
    rig.free_mesh(old.mesh);
    rig.free_material(old.material);
    rig.free_texture(old.texture);
    rig.free_cell(old.cell);
}

/// Frees everything the churn holds and draws twice, so the GPU lets go.
fn free_all(rig: &mut Rig, made: &mut VecDeque<Made>, targets: &mut VecDeque<Texture>) {
    for old in made.drain(..) {
        free(rig, old);
    }
    for old in targets.drain(..) {
        rig.free_texture(old);
    }
    rig.draw();
    rig.draw();
}

#[test]
fn two_thousand_frames_of_churn_bring_gpu_memory_back_to_the_baseline() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    // What stays all along: a lit ground, and an instance batch whose
    // cells come and go.
    let white = rig.material_of(MaterialDesc::default());
    let ground_material = rig.material_of(MaterialDesc {
        color: Color::rgb(0.4, 0.5, 0.4),
        ..Default::default()
    });
    let ground = rig.mesh(&MeshData::plane(20.0), ground_material);
    rig.ctx.world.spawn((Transform3D::at(Vec3::ZERO), Model::new(ground)));
    let cube = MeshData::cube(0.4);
    let cell_mesh = rig.mesh(&cube, white);
    let batch = rig.ctx.gfx.create_instance_batch(cell_mesh, &[], Default::default());
    rig.sync_batch(batch.id);
    // Warm-up: a few frames of the churn, then everything freed. What the
    // engine makes once (the stand-in freed materials share, the shadow
    // cache's maps) is made by then.
    let buffer = rig.ctx.gfx.create_gpu_buffer(4, &[]);
    rig.renderer.create_gpu_buffer(&rig.gpu.device, buffer.slot(), 4, &[]);
    let programs = rig.programs();
    rig.schedule_programs(programs, buffer);
    let mut made: VecDeque<Made> = VecDeque::new();
    let mut targets: VecDeque<Texture> = VecDeque::new();
    for f in 0..2 * KEPT as u32 {
        churn(&mut rig, f, batch, &mut made, &mut targets);
        rig.clear_program_passes();
    }
    free_all(&mut rig, &mut made, &mut targets);
    rig.free_programs(programs);
    rig.draw();
    let baseline = rig.memory();
    eprintln!("baseline: {baseline}");

    let started = std::time::Instant::now();
    let mut halfway = None;
    let mut slots = std::collections::HashSet::new();
    let mut material_slots = std::collections::HashSet::new();
    let mut programs = None;
    for f in 0..FRAMES {
        if f.is_multiple_of(TARGET_EVERY) {
            let previous = programs.take();
            if let Some(old) = previous {
                // Leave one stale compute and effect in the scene until after
                // their replacements take the slots.
                rig.free_programs(old);
                rig.schedule_programs(old, buffer);
            }
            let next = rig.programs();
            rig.schedule_programs(next, buffer);
            assert_eq!(next.shader.id & INDEX_MASK, 0);
            assert_eq!(next.compute.id & INDEX_MASK, 0);
            assert_eq!(next.effect.id & INDEX_MASK, 0);
            assert_eq!(next.array.id & INDEX_MASK, 0);
            rig.ctx.collect_passes(None);
            assert_eq!(rig.ctx.gfx.frame3d.effects.len(), 1);
            assert_eq!(rig.ctx.gfx.frame3d.computes.len(), 1);
            programs = Some(next);
        }
        let (mesh, material) = churn(&mut rig, f, batch, &mut made, &mut targets);
        rig.clear_program_passes();
        material_slots.insert(material.id & INDEX_MASK);
        slots.insert(mesh.slot());
        // Halfway and at the end, the same things are live.
        if f == FRAMES / 2 - 1 {
            halfway = Some(rig.memory());
        }
    }
    let end = rig.memory();
    let halfway = halfway.unwrap();
    eprintln!(
        "{FRAMES} frames in {:.1?}; halfway: {halfway}\nend: {end}",
        started.elapsed()
    );
    assert!(
        differences(&halfway, &end).is_empty(),
        "the second thousand frames held more than the first: {:?}",
        differences(&halfway, &end)
    );
    assert!(end.total > baseline.total, "the churn holds something while it runs");
    assert!(slots.len() <= KEPT + 2, "mesh slots are reused: {} used", slots.len());
    // 2000 textures and 50 render targets made: the table holds the most
    // live at once (the white one, the warm-up's, KEPT + 1 and two targets).
    assert!(
        rig.textures.len() <= 2 * KEPT + 4,
        "texture slots are reused: {} slots",
        rig.textures.len()
    );

    assert!(
        material_slots.len() <= KEPT + 3,
        "material slots are reused: {}",
        material_slots.len()
    );
    // Everything the churn made, freed: back at the baseline, class by class.
    rig.free_programs(programs.unwrap());
    free_all(&mut rig, &mut made, &mut targets);
    let after = rig.memory();
    eprintln!("after freeing: {after}");
    assert!(
        differences(&baseline, &after).is_empty(),
        "GPU memory left behind (class, baseline, after): {:?}",
        differences(&baseline, &after)
    );
    let live = rig.ctx.live_counts();
    assert_eq!(
        (live.textures, live.render_targets, live.instance_cells),
        (0, 0, 0),
        "{live:?}"
    );
}

#[test]
fn offscreen_shader_reads_precise_time_beside_the_legacy_clock_at_ten_hours() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let material = rig.material_of(MaterialDesc::default());
    let mesh = rig.mesh(&MeshData::cube(1.0), material);
    rig.ctx.world.spawn((Transform3D::at(Vec3::ZERO), Model::new(mesh)));
    let effect = rig.effect(
        "fn effect(p: Pixel) -> vec3<f32> {
        let legacy = select(0.0, 0.3, frame.time.x == 36000.0);
        return vec3<f32>(legacy, 2000.0 * ponko_elapsed_time_wrapped(1u), 0.0);
    }",
    );
    rig.ctx
        .scene3d
        .passes
        .effects
        .push(ScreenEffect::new(effect).with_material(material));
    let pixel = |rig: &mut Rig, elapsed| {
        rig.draw_at(elapsed);
        let rgba = support::read_texture(&rig.gpu.device, &rig.gpu.queue, &rig.output);
        let center = ((SIZE.0 / 2 + SIZE.1 / 2 * SIZE.0) * 4) as usize;
        [rgba[center], rgba[center + 1]]
    };
    let first = pixel(&mut rig, 36_000.0);
    let next = pixel(&mut rig, 36_000.000_1);
    assert!(
        first[0] > 20 && first[0].abs_diff(next[0]) <= 2,
        "legacy samples: {first:?}, {next:?}"
    );
    assert!(next[1] > first[1] + 20, "precise samples: {first:?}, {next:?}");
}

#[test]
fn offscreen_mesh_keeps_its_fallbacks_when_resource_slots_are_reused() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    // Two distinct generations must look visibly different if accidentally
    // used by the old mesh (red array vs blue array or replacement shader).
    let red = std::array::from_fn(|i| [255, 0, 0, 255][i % 4]);
    let blue = std::array::from_fn(|i| [0, 0, 255, 255][i % 4]);
    let array = rig.array(red);
    let shader = rig.shader(
        "fn surface(s: Surface) -> Surface {
        var out = s;
        out.albedo = textureSample(custom_array0, material_sampler, s.uv, 0).rgb;
        out.unlit = true;
        return out;
    }",
    );
    let material =
        rig.material_of(
            MaterialDesc::default()
                .with_shader(shader)
                .with_custom_arrays([Some(array), None, None]),
        );
    let mesh = rig.mesh(&MeshData::cube(3.0), material);
    rig.ctx.world.spawn((Transform3D::at(Vec3::ZERO), Model::new(mesh)));
    let center = |rig: &mut Rig| {
        rig.draw_at(0.0);
        let rgba = support::read_texture(&rig.gpu.device, &rig.gpu.queue, &rig.output);
        let at = ((SIZE.0 / 2 + SIZE.1 / 2 * SIZE.0) * 4) as usize;
        [rgba[at], rgba[at + 1], rgba[at + 2]]
    };
    let red_pixel = center(&mut rig);
    assert!(
        red_pixel[0] > 80 && red_pixel[1] < 10 && red_pixel[2] < 10,
        "{red_pixel:?}"
    );

    rig.free_array(array);
    let replacement_array = rig.array(blue);
    assert_eq!(array.id & INDEX_MASK, replacement_array.id & INDEX_MASK);
    let white_pixel = center(&mut rig);
    assert!(
        white_pixel.iter().all(|&c| c > 80),
        "freed array reads white: {white_pixel:?}"
    );

    rig.free_shader(shader);
    let replacement = rig.shader(
        "fn surface(s: Surface) -> Surface {
        var out = s; out.albedo = vec3<f32>(0.0, 0.0, 1.0); out.unlit = true; return out;
    }",
    );
    assert_eq!(shader.id & INDEX_MASK, replacement.id & INDEX_MASK);
    let builtin_pixel = center(&mut rig);
    assert!(
        builtin_pixel.iter().all(|&c| c > 20),
        "freed shader uses built-in: {builtin_pixel:?}"
    );

    rig.free_material(material);
    let replacement_material = rig.material_of(MaterialDesc::default().with_shader(replacement));
    assert_eq!(material.id & INDEX_MASK, replacement_material.id & INDEX_MASK);
    assert_eq!(
        center(&mut rig),
        builtin_pixel,
        "old mesh still draws with the plain material"
    );
}
