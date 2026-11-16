//! Freeing what models use, drawn offscreen: a model whose texture was
//! freed draws untextured (white), one whose material was freed draws with
//! the default material, and one whose mesh was freed draws nothing; a
//! freed texture's slot taken by a new texture doesn't reach materials
//! that read the freed one. Skips without a GPU (a skip is not a pass).
use super::gpu::GpuTexture;
use super::gpu_textures::GpuTextures;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{ComputePass, Material, MaterialDesc, Mesh, MeshData, Model, ScreenEffect};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

const SIZE: u32 = 128;

/// An offscreen renderer, its textures (0: white), and a headless `Ctx`
/// whose handles the renderer's ids follow.
struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    output: wgpu::Texture,
    target: wgpu::TextureView,
    textures: GpuTextures,
    renderer: Renderer3D,
    ctx: Ctx,
}

impl Rig {
    /// `None` without a GPU.
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
        let white = image(&device, &queue, [255; 4]);
        let renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
        // No renderer of its own: this test hands the GPU work to `renderer`.
        ctx.set_headless(true);
        ctx.scene3d.sky.atmosphere = false;
        let mut rig = Self {
            device,
            queue,
            layout,
            output,
            target,
            textures: GpuTextures::default(),
            renderer,
            ctx,
        };
        let white = rig.gpu_texture(white);
        rig.textures.set_blank(white.clone());
        assert_eq!(rig.textures.add(white), 0);
        Some(rig)
    }

    fn gpu_texture(&self, texture: wgpu::Texture) -> GpuTexture {
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
        GpuTexture {
            bind_group,
            texture,
            nearest: false,
            mips: None,
        }
    }

    /// A 1x1 texture of `rgba`, on the CPU and the GPU (the headless
    /// `Ctx` gives out ids as `textures` does past the white one).
    fn texture(&mut self, rgba: [u8; 4]) -> Texture {
        let texture = self.ctx.gfx.create_texture(1, 1, &rgba);
        let gpu = self.gpu_texture(image(&self.device, &self.queue, rgba));
        assert_eq!(self.textures.add(gpu), texture.id);
        texture
    }

    /// Frees `texture` as the window's renderer does: it shows white (its
    /// slot goes to the next texture), and the materials that read it are
    /// bound again.
    fn free_texture(&mut self, texture: Texture) {
        self.ctx.free(texture);
        assert!(self.textures.free(texture.id));
        self.renderer.free_render_target(texture.id);
        self.renderer
            .rebind_materials(&self.device, &self.queue, &self.textures, Some(texture.id), None);
    }

    fn material(&mut self, desc: MaterialDesc) -> Material {
        let material = self.ctx.gfx.create_material(desc);
        let id = self
            .renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        assert_eq!(id, material.id);
        material
    }

    fn mesh(&mut self, material: Material) -> Mesh {
        let data = MeshData::cube(1.6);
        let mesh = self.ctx.gfx.create_mesh(&data, material);
        self.renderer
            .create_mesh(&self.device, &data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// Draws the world lit by a white sky and reads the pixels back (RGBA).
    fn draw(&mut self) -> Vec<u8> {
        let ctx = &mut self.ctx;
        let mut frame = Frame3D::default();
        frame.lod = ctx.scene3d.lod;
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
            light: SkyLight {
                sun: Vec3::ZERO,
                sky: Vec3::ONE,
                ground: Vec3::ONE,
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

/// A 1x1 sRGB texture of `rgba` that materials can read as data too.
fn image(device: &wgpu::Device, queue: &wgpu::Queue, rgba: [u8; 4]) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
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
        texture.as_image_copy(),
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        texture.size(),
    );
    texture
}

/// The mean RGB of a 9x9 box around where `at` shows.
fn around(pixels: &[u8], view_proj: Mat4, at: Vec3) -> [u32; 3] {
    let ndc = view_proj.project_point3(at);
    let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
    let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
    let mut sum = [0u32; 3];
    for yy in y - 4..=y + 4 {
        for xx in x - 4..=x + 4 {
            let i = ((yy * SIZE + xx) * 4) as usize;
            for c in 0..3 {
                sum[c] += u32::from(pixels[i + c]);
            }
        }
    }
    sum.map(|s| s / 81)
}

#[test]
fn models_whose_texture_material_or_mesh_was_freed_draw_the_default() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    // Left: glows through a red emissive map. Middle: glows green.
    // Right: glows blue. Their bases are black, so only the glow shows.
    let red = rig.texture([255, 0, 0, 255]);
    let glow = |color: Color, map: Option<Texture>| MaterialDesc {
        color: Color::rgb(0.0, 0.0, 0.0),
        emissive: color,
        emissive_map: map,
        roughness: 1.0,
        ..Default::default()
    };
    let mapped = rig.material(glow(Color::WHITE, Some(red)));
    let green = rig.material(glow(Color::rgb(0.0, 1.0, 0.0), None));
    let blue = rig.material(glow(Color::rgb(0.0, 0.0, 1.0), None));
    let left = rig.mesh(mapped);
    let middle = rig.mesh(green);
    let right = rig.mesh(blue);
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 8.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    for (x, mesh) in [(-2.4, left), (0.0, middle), (2.4, right)] {
        rig.ctx
            .world
            .spawn((Transform3D::at(vec3(x, 0.0, 0.0)), Model::new(mesh)));
    }
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let at = |pixels: &[u8], x: f32| around(pixels, view_proj, vec3(x, 0.0, 0.0));

    let pixels = rig.draw();
    let [l, m, r] = [-2.4, 0.0, 2.4].map(|x| at(&pixels, x));
    eprintln!("before: {l:?} {m:?} {r:?}");
    assert!(l[0] > 150 && l[1] < 60 && l[2] < 60, "left: red: {l:?}");
    assert!(m[1] > 150 && m[0] < 60 && m[2] < 60, "middle: green: {m:?}");
    assert!(r[2] > 150 && r[0] < 60 && r[1] < 60, "right: blue: {r:?}");

    rig.free_texture(red);
    rig.ctx.free(green);
    rig.renderer
        .free_material(&rig.device, &rig.queue, green.id, &rig.textures);
    rig.ctx.free(right);
    rig.renderer.free_mesh(right.id);
    assert!(!rig.ctx.is_live(red) && !rig.ctx.is_live(green) && !rig.ctx.is_live(right));

    let pixels = rig.draw();
    let [l, m, r] = [-2.4, 0.0, 2.4].map(|x| at(&pixels, x));
    eprintln!("after: {l:?} {m:?} {r:?}");
    // Its emissive map is gone: the white glow shows as it is.
    assert!(l.iter().all(|&c| c > 150), "left: white without its map: {l:?}");
    // The default material: white, lit grey by the sky; no glow.
    assert!(m.iter().all(|&c| c > 20), "middle: drawn: {m:?}");
    assert!(
        m[1].abs_diff(m[0]) < 30 && m[1].abs_diff(m[2]) < 30,
        "middle: grey, not green: {m:?}"
    );
    // No mesh: nothing drawn, the black background shows.
    assert!(r.iter().all(|&c| c < 10), "right: nothing: {r:?}");
}

/// A material whose base map was freed reads white, and keeps reading
/// white after a new texture takes the freed one's slot and the materials
/// are bound again: the freed id never reaches the new texture.
#[test]
fn a_material_reading_a_freed_texture_stays_white_after_its_slot_is_reused() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let red = rig.texture([255, 0, 0, 255]);
    let material = rig.material(MaterialDesc {
        texture: Some(red),
        roughness: 1.0,
        ..Default::default()
    });
    let mesh = rig.mesh(material);
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let grey = |c: [u32; 3]| c[1] > 20 && c[0].abs_diff(c[1]) < 30 && c[2].abs_diff(c[1]) < 30;

    let before = around(&rig.draw(), view_proj, Vec3::ZERO);
    assert!(before[0] > 2 * before[1].max(10), "red first: {before:?}");
    rig.free_texture(red);
    let freed = around(&rig.draw(), view_proj, Vec3::ZERO);
    assert!(grey(freed), "white (lit grey) once freed: {freed:?}");

    let green = rig.texture([0, 255, 0, 255]);
    let slot = crate::handle::INDEX_MASK;
    assert_eq!(green.id & slot, red.id & slot, "the new texture takes the slot");
    assert_ne!(green, red);
    rig.renderer
        .rebind_materials(&rig.device, &rig.queue, &rig.textures, Some(red.id), None);
    rig.renderer.texture_replaced(&rig.device, &rig.textures, green.id);
    let reused = around(&rig.draw(), view_proj, Vec3::ZERO);
    eprintln!("red {before:?}, freed {freed:?}, slot reused {reused:?}");
    assert!(grey(reused), "still white, not green: {reused:?}");
}

#[test]
fn freed_effects_compute_passes_and_targets_are_left_out_of_the_frame() {
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(64.0, 64.0));
    ctx.set_headless(true);
    let effect = ctx
        .gfx
        .create_effect("e", "fn effect(p: Pixel) -> vec3<f32> { return p.color; }");
    let compute = ctx
        .gfx
        .create_compute("c", "fn compute(c: Cell) -> vec4<f32> { return c.value; }");
    let target = ctx.gfx.create_render_target(8, 8);
    ctx.scene3d.passes.effects.push(ScreenEffect::new(effect));
    ctx.scene3d.passes.compute.push(ComputePass::new(compute, target));
    let material = ctx.gfx.default_material();
    ctx.collect_passes(Some(material));
    assert_eq!(ctx.gfx.frame3d.effects.len(), 1);
    assert_eq!(ctx.gfx.frame3d.computes.len(), 1);
    ctx.free(effect);
    ctx.free(compute);
    ctx.collect_passes(Some(material));
    assert!(ctx.gfx.frame3d.effects.is_empty() && ctx.gfx.frame3d.computes.is_empty());
    // A live pass into a freed target is left out too.
    let other = ctx
        .gfx
        .create_compute("d", "fn compute(c: Cell) -> vec4<f32> { return c.value; }");
    ctx.scene3d.passes.compute.push(ComputePass::new(other, target));
    ctx.collect_passes(Some(material));
    assert_eq!(ctx.gfx.frame3d.computes.len(), 1);
    ctx.free(target);
    ctx.collect_passes(Some(material));
    assert!(ctx.gfx.frame3d.computes.is_empty());
}

/// A texture streamed to another resolution keeps its id (as
/// `Renderer::replace_texture` does): materials that read it are bound to
/// the new pixels at once, whatever their size.
#[test]
fn a_texture_replaced_at_another_resolution_shows_in_the_materials_that_use_it() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let streamed = rig.texture([255, 0, 0, 255]);
    let material = rig.material(MaterialDesc {
        color: Color::rgb(0.0, 0.0, 0.0),
        emissive: Color::WHITE,
        emissive_map: Some(streamed),
        roughness: 1.0,
        ..Default::default()
    });
    let mesh = rig.mesh(material);
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let before = around(&rig.draw(), view_proj, Vec3::ZERO);
    assert!(before[0] > 150 && before[2] < 60, "red first: {before:?}");

    // The next resolution: 4 x 4 blue, under the same id.
    let blue = rig.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    rig.queue.write_texture(
        blue.as_image_copy(),
        &[0, 0, 255, 255].repeat(16),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16),
            rows_per_image: None,
        },
        blue.size(),
    );
    let replaced = rig.gpu_texture(blue);
    *rig.textures.live_mut(streamed.id).expect("live") = replaced;
    rig.renderer.texture_replaced(&rig.device, &rig.textures, streamed.id);
    let after = around(&rig.draw(), view_proj, Vec3::ZERO);
    assert!(after[2] > 150 && after[0] < 60, "blue after: {after:?}");
}

#[test]
fn mesh_cost_visits_count_drawn_models_and_stop_counting_freed_meshes() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let material = rig.material(MaterialDesc::default());
    let mesh = rig.mesh(material);
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 8.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    rig.draw();
    let mut models = 0;
    let mut triangles = 0;
    rig.renderer.visit_mesh_costs(|slot, n, t, _, _| {
        assert_eq!(slot, mesh.slot());
        models += n;
        triangles += t;
    });
    assert_eq!(models, 1);
    assert!(triangles > 0);
    rig.ctx.free(mesh);
    rig.renderer.free_mesh(mesh.id);
    rig.draw();
    rig.renderer.visit_mesh_costs(|_, n, t, _, _| {
        assert_eq!((n, t), (0, 0));
    });
}
