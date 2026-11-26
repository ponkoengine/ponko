//! Offscreen GPU buffer compute and instanced material reads.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::Blend;
#[allow(unused_imports)]
use crate::render::{
    AntiAliasing, ComputePass, GpuBuffer, GpuFeature, InstanceBatchOptions, InstanceLevel, MaterialDesc, Mesh,
    MeshData, Model, TargetCamera,
};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

const SIZE: u32 = 128;

/// An offscreen renderer with a 1x1 white texture, and a `Ctx` to fill.
pub(super) struct Rig {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    output: wgpu::Texture,
    target: wgpu::TextureView,
    textures: Vec<GpuTexture>,
    pub renderer: Box<Renderer3D>,
    pub ctx: Box<Ctx>,
    /// What the last draw collected: kept between draws, as a game's is.
    frame: Frame3D,
    /// The clock the next draw is drawn at: (seconds since the start,
    /// seconds since the last frame).
    pub time: (f32, f32),
    /// CPU time of the last draw's `Renderer3D::prepare` (collecting not
    /// included), in ms.
    pub prepare_ms: f64,
    /// Environment probe bakes the next draws ask for, as a game's frame
    /// lists them: (probe slot, the render targets of its six faces).
    pub probe_bakes: Vec<(u32, [u32; 6])>,
    /// The next draws are prepared and not drawn (a frame a game skips):
    /// they give no pixels.
    pub prepare_only: bool,
}

impl Rig {
    /// `None` without a GPU.
    pub fn new() -> Option<Self> {
        Self::with_compute(true)
    }
    fn with_compute(compute: bool) -> Option<Self> {
        Self::with_limits(compute, |_| {})
    }
    /// A rig whose device has the limits `shrink` leaves (below what the
    /// adapter has).
    pub fn with_limits(compute: bool, shrink: impl FnOnce(&mut wgpu::Limits)) -> Option<Self> {
        let gpu = crate::render::render_test_support::gpu_adjusted(
            |_| {
                Ok(wgpu::DeviceDescriptor {
                    required_limits: if compute {
                        wgpu::Limits::default()
                    } else {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    },
                    ..Default::default()
                })
            },
            |descriptor| shrink(&mut descriptor.required_limits),
        )?;
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
        // Boxed: a test that holds two rigs by value comes close to a test
        // thread's 2 MiB stack.
        let mut renderer = Box::new(Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true));
        // These suites test the GPU-driven path wherever the device has it,
        // whatever the batch's size.
        renderer.batching.path = BatchPath::Gpu;
        let mut ctx = Box::new(Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32)));
        ctx.scene3d.sky.atmosphere = false;
        Some(Self {
            device,
            queue,
            output,
            target,
            textures,
            renderer,
            ctx,
            frame: Frame3D::default(),
            time: (0.0, 1.0 / 60.0),
            prepare_ms: 0.0,
            probe_bakes: Vec::new(),
            prepare_only: false,
        })
    }

    /// Panics unless the renderer's buffers that are written a change at a
    /// time hold what was written (see `Renderer3D::check_mirrors`).
    pub fn check_mirrors(&self) {
        self.renderer.check_mirrors(&self.device, &self.queue);
    }

    /// A mesh of `data` with `desc`, on the CPU and the GPU.
    pub fn mesh(&mut self, data: &MeshData, desc: MaterialDesc) -> Mesh {
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// How many model instances the last draw's views drew (the camera's
    /// and the shadow maps' together).
    pub fn models_drawn(&self) -> usize {
        self.frame.drawn
    }

    /// Draws the world and reads the pixels back (RGBA).
    pub fn draw(&mut self) -> Vec<u8> {
        self.draw_with_grid(None)
    }
    fn draw_with_grid(&mut self, grid: Option<crate::three::occlusion::OcclusionGrid>) -> Vec<u8> {
        let ctx = &mut self.ctx;
        let mut frame = std::mem::take(&mut self.frame);
        frame.lod = ctx.scene3d.lod;
        frame.occlusion = grid;
        // Motion vectors, as a game's frame lists them.
        frame.motion.enabled = ctx.scene3d.post.anti_aliasing == AntiAliasing::Taa
            || (ctx.scene3d.post.ao_temporal && ctx.scene3d.post.ambient_occlusion.enabled);
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        ctx.gfx.frame3d.views = std::mem::take(&mut frame.views);
        ctx.collect_views(&SkinFrame::default(), None);
        frame.views = std::mem::take(&mut ctx.gfx.frame3d.views);
        frame.probe_bakes.clone_from(&self.probe_bakes);
        // Like the frame does: a freed buffer's passes are left out.
        frame.computes.clear();
        frame.computes.extend(
            ctx.scene3d
                .passes
                .compute
                .iter()
                .filter(|c| ctx.gfx.gpu_buffer_len(c.buffer.unwrap()).is_some())
                .map(|c| (c.compute.id, c.buffer.unwrap().slot() | (1 << 31), 0)),
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
            time: (f64::from(self.time.0), self.time.1),
            occlusion: ctx.scene3d.camera.occlusion_culling,
        };
        let start = std::time::Instant::now();
        self.renderer.prepare(
            &self.device,
            &self.queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: (SIZE, SIZE),
            },
        );
        self.prepare_ms = start.elapsed().as_secs_f64() * 1e3;
        if self.prepare_only {
            self.frame = frame;
            return Vec::new();
        }
        let buffers = self
            .renderer
            .draw(&self.device, &self.target, wgpu::Color::BLACK, &self.textures, None);
        self.queue.submit(buffers);
        self.frame = frame;
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

/// The world point under screen pixel `p`'s projection.
fn pixel(view_proj: Mat4, p: Vec3) -> (u32, u32) {
    let c = view_proj * p.extend(1.0);
    let ndc = c.truncate() / c.w;
    (
        ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32,
        ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32,
    )
}

#[test]
fn buffer_compute_places_two_instances_before_drawing() {
    buffer_draw();
}
fn buffer_draw() {
    let Some(mut rig) = Rig::new() else { return };
    if rig.device.limits().max_storage_buffers_per_shader_stage == 0 {
        return;
    }
    let source = "fn vertex(v: Vertex) -> Vertex { var o = v; o.position += items[v.instance_index].xyz; return o; }";
    let shader = rig.ctx.gfx.create_shader("buffer draw", source);
    rig.renderer.create_shader(&rig.device, "buffer draw", source).unwrap();
    let mesh = rig.mesh(
        &MeshData::cube(0.4),
        MaterialDesc {
            shader: Some(shader),
            emissive: Color::WHITE,
            ..Default::default()
        },
    );
    let mut data = vec![vec4(99.0, -1.0, 0.0, 1.0); 65];
    data[0].x = -2.0;
    let b = rig.ctx.gfx.create_gpu_buffer(65, &data);
    rig.renderer.create_gpu_buffer(&rig.device, b.slot(), 65, &data);
    rig.renderer
        .write_gpu_buffer(&rig.queue, b.slot(), 64, &[vec4(0.0, -1.0, 0.0, 1.0)]);
    rig.renderer.set_mesh_gpu_buffer(mesh.slot(), b.slot(), 65);
    let first = "fn update(i: u32) { items[i].x += 1.0; }";
    let second = "fn update(i: u32) { items[i].y += 1.0 + frame.time.y * 0.0; }";
    for source in [first, second] {
        let c = rig.ctx.gfx.create_compute("positions", source);
        rig.renderer.create_compute(&rig.device, "positions", source).unwrap();
        rig.ctx.scene3d.passes.compute.push(ComputePass::for_buffer(c, b));
    }
    rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    rig.ctx.scene3d.lighting.shadows = false;
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, 5.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let pixels = rig.draw();
    let red = |p| {
        let (x, y) = pixel(view_proj, p);
        pixels[((y * SIZE + x) * 4) as usize]
    };
    assert!(red(vec3(-1.0, 0.0, 0.0)) > 150, "first computed instance");
    assert!(red(vec3(1.0, 0.0, 0.0)) > 150, "second computed instance");
    assert!(red(Vec3::ZERO) < 20, "nothing at the identity origin");
    assert!(red(vec3(-1.0, -1.0, 0.0)) < 20, "ordered second pass moved it");
}

#[test]
fn persistent_batches_cull_on_gpu() {
    persistent_batch_draw(true);
}
#[test]
fn persistent_batches_fallback_draws_visible_cells() {
    persistent_batch_draw(false);
}
fn persistent_batch_draw(compute: bool) {
    let Some(mut rig) = Rig::with_compute(compute) else {
        return;
    };
    if compute && !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::ComputeCulling) {
        return;
    }
    let mesh = rig.mesh(
        &MeshData::cube(0.5),
        MaterialDesc {
            emissive: Color::WHITE,
            ..Default::default()
        },
    );
    rig.ctx.scene3d.camera.position = Vec3::ZERO;
    rig.ctx.scene3d.camera.look_at(-Vec3::Z);
    rig.ctx.scene3d.lighting.shadows = false;
    let options = InstanceBatchOptions {
        cell_size: if compute { 32.0 } else { 1.0 },
        max_distance: 10.0,
        ..Default::default()
    };
    let at = [vec3(-1.0, 0.0, -5.0), vec3(1.0, 0.0, 5.0), vec3(-8.0, 0.0, -20.0)];
    let transforms = at.map(Mat4::from_translation);
    let b = rig.ctx.gfx.create_instance_batch(mesh, &transforms, options);
    rig.renderer.set_instance_batch(
        &rig.device,
        &rig.queue,
        b.id,
        rig.ctx.gfx.assets3d.instance_batches[b.id as usize].as_ref(),
    );
    let image = rig.draw();
    if let Ok(path) = std::env::var("PONKO_TEST_IMAGE") {
        image::save_buffer(path, &image, SIZE, SIZE, image::ColorType::Rgba8).unwrap();
    }
    let vp = rig.ctx.scene3d.camera.view_proj();
    let (x, y) = pixel(vp, at[0]);
    assert!(
        image[((y * SIZE + x) * 4) as usize] > 100,
        "visible batch absent ({compute})"
    );
    // The far instance projects inside the view, but its maximum distance excludes it.
    let (x, y) = pixel(vp, at[2]);
    assert!(
        image[((y * SIZE + x) * 4) as usize] < 50,
        "distant instance drawn ({compute})"
    );
    if compute {
        let (args, at) = rig.renderer.batch_view_args(b.id, 0).unwrap();
        let read = rig.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 20,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = rig.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(args, at, &read, 0, 20);
        rig.queue.submit([encoder.finish()]);
        read.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        rig.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let bytes = read.get_mapped_range(..).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            1,
            "behind/distant instances survived GPU culling"
        );
    }
    rig.ctx.scene3d.camera.occlusion_culling = true;
    let wall = vp * vec3(0.0, 0.0, -2.0).extend(1.0);
    let grid = crate::three::occlusion::OcclusionGrid::new(vp, Vec3::ZERO, 16, 16, vec![wall.z / wall.w; 256]);
    let hidden = rig.draw_with_grid(Some(grid));
    let (x, y) = pixel(vp, at[0]);
    assert!(
        hidden[((y * SIZE + x) * 4) as usize] < 50,
        "occluded instance survived ({compute})"
    );
    // The batch's view settings and its culling header and grid were
    // written only where they changed.
    rig.check_mirrors();
    rig.renderer.set_instance_batch(&rig.device, &rig.queue, b.id, None);
    // Cutout-only batches must trigger the coverage prepass even without model entities.
    let cutout = rig.mesh(
        &MeshData::cube(0.5),
        MaterialDesc {
            blend: Blend::Cutout,
            emissive: Color::WHITE,
            ..Default::default()
        },
    );
    let cb = rig
        .ctx
        .gfx
        .create_instance_batch(cutout, &[Mat4::from_translation(at[0])], options);
    rig.renderer.set_instance_batch(
        &rig.device,
        &rig.queue,
        cb.id,
        rig.ctx.gfx.assets3d.instance_batches[cb.id as usize].as_ref(),
    );
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.camera.occlusion_culling = false;
    let image = rig.draw();
    assert!(
        image[((y * SIZE + x) * 4) as usize] > 100,
        "cutout batch absent ({compute})"
    );
    rig.renderer.set_instance_batch(&rig.device, &rig.queue, cb.id, None);
    rig.renderer.set_instance_batch(&rig.device, &rig.queue, b.id, None);
    let image = rig.draw();
    let (x, y) = pixel(vp, at[0]);
    assert!(image[((y * SIZE + x) * 4) as usize] < 50, "freed batch still drawn");
}

const OFFSETS: &str =
    "fn vertex(v: Vertex) -> Vertex { var o = v; o.position += items[v.instance_index].xyz; return o; }
    fn surface(s: Surface) -> Surface { var o = s; o.emissive = s.albedo * 4.0; return o; }";

impl Rig {
    /// A cube mesh drawn from GPU buffer elements by `OFFSETS`.
    fn offset_mesh(&mut self) -> Mesh {
        let shader = self.ctx.gfx.create_shader("offsets", OFFSETS);
        self.renderer.create_shader(&self.device, "offsets", OFFSETS).unwrap();
        self.mesh(
            &MeshData::cube(0.4),
            MaterialDesc {
                shader: Some(shader),
                ..Default::default()
            },
        )
    }
    /// A buffer of `data` on the CPU and the GPU.
    fn buffer(&mut self, data: &[Vec4]) -> GpuBuffer {
        let b = self.ctx.gfx.create_gpu_buffer(data.len() as u32, data);
        self.renderer
            .create_gpu_buffer(&self.device, b.slot(), data.len() as u32, data);
        b
    }
    fn look_at_origin(&mut self) -> Mat4 {
        self.ctx.scene3d.lighting.shadows = false;
        self.ctx.scene3d.camera.position = vec3(0.0, 0.0, 5.0);
        self.ctx.scene3d.camera.look_at(Vec3::ZERO);
        self.ctx.scene3d.camera.view_proj()
    }
}

/// Channel `c` of the pixel under world point `p`.
fn channel(pixels: &[u8], view_proj: Mat4, p: Vec3, c: usize) -> u8 {
    let (x, y) = pixel(view_proj, p);
    pixels[((y * SIZE + x) * 4) as usize + c]
}

#[test]
fn buffer_instances_move_and_tint_with_their_entity() {
    let Some(mut rig) = Rig::new() else { return };
    if rig.device.limits().max_storage_buffers_per_shader_stage == 0 {
        return;
    }
    let mesh = rig.offset_mesh();
    let b = rig.buffer(&[vec4(-1.0, 0.0, 0.0, 0.0), vec4(1.0, 0.0, 0.0, 0.0)]);
    rig.renderer.set_mesh_gpu_buffer(mesh.slot(), b.slot(), 2);
    let entity = rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    let view_proj = rig.look_at_origin();

    let pixels = rig.draw();
    for x in [-1.0, 1.0] {
        let p = vec3(x, 0.0, 0.0);
        let rgb = [0, 1, 2].map(|c| channel(&pixels, view_proj, p, c));
        assert!(rgb.iter().all(|&v| v > 150), "white instance at {x}: {rgb:?}");
    }
    assert!(channel(&pixels, view_proj, Vec3::ZERO, 0) < 20, "nothing between them");

    // Moved up and tinted red: the instances went along.
    let up = vec3(0.0, 1.5, 0.0);
    rig.ctx.world.get_mut::<Transform3D>(entity).unwrap().position = up;
    rig.ctx.world.get_mut::<Model>(entity).unwrap().color = Color::rgb(1.0, 0.0, 0.0);
    let pixels = rig.draw();
    for x in [-1.0, 1.0] {
        let p = up + vec3(x, 0.0, 0.0);
        let rgb = [0, 1, 2].map(|c| channel(&pixels, view_proj, p, c));
        assert!(
            rgb[0] > 150 && rgb[1] < 30 && rgb[2] < 30,
            "red instance at {p}: {rgb:?}"
        );
        let old = vec3(x, 0.0, 0.0);
        assert!(channel(&pixels, view_proj, old, 0) < 20, "nothing left at {old}");
    }

    // Scaled by two, the cube each instance draws is too.
    rig.ctx.world.get_mut::<Transform3D>(entity).unwrap().scale = Vec3::splat(2.0);
    rig.ctx.world.get_mut::<Model>(entity).unwrap().color = Color::WHITE;
    let pixels = rig.draw();
    // Beyond the unscaled cube's reach (0.4), inside the scaled one's.
    let wide = up + vec3(-1.0, 0.5, 0.0);
    assert!(
        channel(&pixels, view_proj, wide, 0) > 150,
        "a scaled cube covers {wide}"
    );
}

#[test]
fn buffer_instances_are_culled_by_the_grown_bounds() {
    let Some(mut rig) = Rig::new() else { return };
    if rig.device.limits().max_storage_buffers_per_shader_stage == 0 {
        return;
    }
    let mesh = rig.offset_mesh();
    // The entity is far off to the left; its instance is shifted back on screen.
    let b = rig.buffer(&[vec4(12.0, 0.0, 0.0, 0.0)]);
    rig.renderer.set_mesh_gpu_buffer(mesh.slot(), b.slot(), 1);
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(-12.0, 0.0, 0.0)), Model::new(mesh)));
    let view_proj = rig.look_at_origin();

    let pixels = rig.draw();
    assert!(
        channel(&pixels, view_proj, Vec3::ZERO, 0) < 20,
        "culled by the mesh's own bounds"
    );

    rig.ctx.gfx.set_mesh_gpu_bounds(mesh, vec3(12.0, 0.0, 0.0), 1.0);
    let pixels = rig.draw();
    assert!(
        channel(&pixels, view_proj, Vec3::ZERO, 0) > 150,
        "the grown bounds keep it"
    );
}

#[test]
fn freed_buffers_are_skipped_and_their_slots_reused() {
    let Some(mut rig) = Rig::new() else { return };
    if rig.device.limits().max_storage_buffers_per_shader_stage == 0 {
        return;
    }
    let mesh = rig.offset_mesh();
    let b = rig.buffer(&[vec4(-1.0, 0.0, 0.0, 0.0), vec4(1.0, 0.0, 0.0, 0.0)]);
    rig.renderer.set_mesh_gpu_buffer(mesh.slot(), b.slot(), 2);
    let source = "fn update(i: u32) { items[i].y += 1.0; }";
    let c = rig.ctx.gfx.create_compute("up", source);
    rig.renderer.create_compute(&rig.device, "up", source).unwrap();
    rig.ctx.scene3d.passes.compute.push(ComputePass::for_buffer(c, b));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    let view_proj = rig.look_at_origin();
    let drawn = |rig: &mut Rig, p| channel(&rig.draw(), view_proj, p, 0) > 150;

    assert!(drawn(&mut rig, vec3(-1.0, 1.0, 0.0)), "the pass moved it up");

    rig.ctx.free(b);
    rig.renderer.free_gpu_buffer(b.slot());
    assert_eq!(rig.ctx.gfx.gpu_buffer_len(b), None);
    assert!(rig.ctx.scene3d.passes.compute.is_empty(), "its passes went with it");
    assert!(!drawn(&mut rig, vec3(-1.0, 1.0, 0.0)) && !drawn(&mut rig, vec3(-1.0, 0.0, 0.0)));

    // A stale pass is skipped, not a panic.
    rig.ctx.scene3d.passes.compute.push(ComputePass::for_buffer(c, b));
    assert!(!drawn(&mut rig, vec3(-1.0, 0.0, 0.0)));
    rig.ctx.scene3d.passes.compute.clear();
    // Freeing it again does nothing.
    rig.ctx.free(b);

    // The slot comes back for a new buffer, under a new id; the mesh isn't bound to it.
    let again = rig.buffer(&[
        vec4(0.0, 0.0, 0.0, 0.0),
        vec4(0.0, -1.0, 0.0, 0.0),
        vec4(0.0, 1.0, 0.0, 0.0),
    ]);
    assert_eq!(again.slot(), b.slot());
    assert_ne!(again, b);
    assert_eq!(rig.ctx.gfx.gpu_buffer_len(b), None, "the old handle stays dead");
    assert_eq!(rig.ctx.gfx.gpu_buffer_len(again), Some(3));
    assert!(!drawn(&mut rig, Vec3::ZERO), "the old binding was cleared");
    rig.renderer.set_mesh_gpu_buffer(mesh.slot(), again.slot(), 3);
    assert!(
        drawn(&mut rig, Vec3::ZERO) && drawn(&mut rig, vec3(0.0, 1.0, 0.0)),
        "bound again"
    );
    assert!(!drawn(&mut rig, vec3(-1.0, 0.0, 0.0)), "nothing of the old data");
}

#[test]
fn batches_draw_into_target_textures() {
    batch_target(true, false);
    batch_target(false, false);
    batch_target(true, true);
    batch_target(false, true);
}
fn batch_target(compute: bool, reflection: bool) {
    batch_target_levels(compute, reflection, false);
}
#[test]
fn instance_levels_use_target_camera_distance() {
    for compute in [true, false] {
        batch_target_levels(compute, false, true);
    }
}
fn batch_target_levels(compute: bool, reflection: bool, levels: bool) {
    let Some(mut rig) = Rig::with_compute(compute) else {
        return;
    };
    let mesh = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            emissive: Color::WHITE,
            color: Color::WHITE,
            ..Default::default()
        },
    );
    let b = if levels {
        rig.ctx.gfx.create_instance_batch_levels(
            &[
                InstanceLevel {
                    mesh,
                    min_distance: 0.0,
                    max_distance: 10.0,
                    camera: true,
                    shadows: false,
                },
                InstanceLevel {
                    mesh,
                    min_distance: 10.0,
                    max_distance: 1000.0,
                    camera: false,
                    shadows: true,
                },
            ],
            &[Mat4::from_translation(vec3(0.0, 1.0, 0.0))],
            &[],
            Default::default(),
        )
    } else {
        rig.ctx.gfx.create_instance_batch(
            mesh,
            &[Mat4::from_translation(vec3(0.0, 1.0, 0.0))],
            InstanceBatchOptions {
                cast_shadows: false,
                max_distance: 10.0,
                ..Default::default()
            },
        )
    };
    rig.renderer.set_instance_batch(
        &rig.device,
        &rig.queue,
        b.id,
        rig.ctx.gfx.assets3d.instance_batches[b.id as usize].as_ref(),
    );
    let texture = rig.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("batch target test"),
        size: rig.output.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    // The test owns the offscreen renderer separately from the context.
    rig.ctx.set_headless(true);
    let handle = rig.ctx.gfx.create_render_target(SIZE, SIZE);
    rig.renderer.add_render_target(handle.texture.id, &texture);
    let mut camera = rig.ctx.scene3d.camera;
    camera.position = vec3(0.0, 1.0, 4.0);
    camera.look_at(vec3(0.0, 1.0, 0.0));
    let target = if reflection {
        camera.position.y = -1.0;
        camera.look_at(vec3(0.0, 1.0, 0.0));
        let mut target = TargetCamera::new(camera, handle);
        target.clip_plane = Some(Vec4::new(0.0, 1.0, 0.0, 0.0));
        target
    } else {
        TargetCamera::new(camera, handle)
    };
    rig.ctx.scene3d.passes.target_cameras.push(target);
    // Main camera is beyond max_distance; only the target view can see this batch.
    rig.ctx.scene3d.camera.position = vec3(100.0, 100.0, 100.0);
    rig.draw();
    let read = rig.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (SIZE * SIZE * 8) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = rig.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 8),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    rig.queue.submit([encoder.finish()]);
    read.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    rig.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = read.get_mapped_range(..).unwrap();
    let center = ((SIZE / 2 * SIZE + SIZE / 2) * 8) as usize;
    // White unlit geometry has RGB half floats near 1; clear black has zero RGB.
    assert!(
        u16::from_le_bytes([bytes[center], bytes[center + 1]]) >= 0x3800,
        "target batch absent (compute={compute}, reflection={reflection})"
    );
}

#[test]
fn instance_levels_select_boundary_meshes_and_colors_on_gpu_and_fallback() {
    for compute in [true, false] {
        let Some(mut rig) = Rig::with_compute(compute) else {
            continue;
        };
        let source = "fn surface(s: Surface) -> Surface { var o=s; o.emissive=s.albedo*4.0; return o; }";
        let shader = rig.ctx.gfx.create_shader("instance colours", source);
        rig.renderer
            .create_shader(&rig.device, "instance colours", source)
            .unwrap();
        let mesh = |rig: &mut Rig, color| {
            rig.mesh(
                &MeshData::cube(1.0),
                MaterialDesc {
                    color,
                    shader: Some(shader),
                    ..Default::default()
                },
            )
        };
        let near = mesh(&mut rig, Color::RED);
        let far = mesh(&mut rig, Color::BLUE);
        rig.ctx.scene3d.camera.position = Vec3::ZERO;
        rig.ctx.scene3d.camera.look_at(-Vec3::Z);
        rig.ctx.scene3d.lighting.shadows = false;
        let levels = [
            InstanceLevel {
                mesh: near,
                min_distance: 0.0,
                max_distance: 10.0,
                camera: true,
                shadows: false,
            },
            InstanceLevel {
                mesh: far,
                min_distance: 10.0,
                max_distance: 20.0,
                camera: true,
                shadows: false,
            },
        ];
        let batch = rig.ctx.gfx.create_instance_batch_levels(
            &levels,
            &[Mat4::from_translation(vec3(0.0, 0.0, -10.0))],
            &[],
            Default::default(),
        );
        rig.renderer.set_instance_batch(
            &rig.device,
            &rig.queue,
            batch.id,
            rig.ctx.gfx.assets3d.instance_batches[batch.id as usize].as_ref(),
        );
        for (z, channel) in [(-0.125, 0), (0.0, 2), (0.125, 2)] {
            rig.ctx.scene3d.camera.position.z = z;
            let pixels = rig.draw();
            let p = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
            assert!(
                pixels[p + channel] > 150 && pixels[p + 2 - channel] < 50,
                "wrong band at camera z={z}, compute={compute}: {:?}",
                &pixels[p..p + 4]
            );
        }
        let b = rig.renderer.static_batches[batch.id as usize].as_ref().unwrap();
        assert!(b.views.len() <= 2, "camera-only levels allocated shadow slots");
        rig.ctx.scene3d.camera.position.z = 15.0;
        assert!(
            rig.draw().as_chunks::<4>().0.iter().all(|p| p[0] < 50 && p[2] < 50),
            "no band must draw nothing"
        );
        rig.renderer.set_instance_batch(&rig.device, &rig.queue, batch.id, None);
        let white = mesh(&mut rig, Color::WHITE);
        rig.ctx.scene3d.camera.position = Vec3::ZERO;
        let at = [vec3(-1.0, 0.0, -5.0), vec3(1.0, 0.0, -5.0)];
        let b = rig.ctx.gfx.create_instance_batch_levels(
            &[InstanceLevel {
                mesh: white,
                min_distance: 0.0,
                max_distance: 20.0,
                camera: true,
                shadows: false,
            }],
            &at.map(Mat4::from_translation),
            &[Color::RED, Color::GREEN],
            Default::default(),
        );
        rig.renderer.set_instance_batch(
            &rig.device,
            &rig.queue,
            b.id,
            rig.ctx.gfx.assets3d.instance_batches[b.id as usize].as_ref(),
        );
        let pixels = rig.draw();
        let vp = rig.ctx.scene3d.camera.view_proj();
        for (position, c) in at.into_iter().zip([0, 1]) {
            let (x, y) = pixel(vp, position);
            let i = ((y * SIZE + x) * 4) as usize;
            assert!(
                pixels[i + c] > 150 && pixels[i + 2] < 50,
                "instance colour absent: {:?}",
                &pixels[i..i + 4]
            );
        }
        if let Ok(path) = std::env::var("PONKO_LEVEL_IMAGE") {
            image::save_buffer(
                format!("{path}-{compute}.png"),
                &pixels,
                SIZE,
                SIZE,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}

#[test]
fn instance_levels_overlap_exactly_without_copying_source_records() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    // Instance levels are chosen per instance by the GPU culling pass.
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::ComputeCulling) {
        return;
    }
    let mesh = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            emissive: Color::WHITE,
            ..Default::default()
        },
    );
    rig.ctx.scene3d.camera.position = Vec3::ZERO;
    rig.ctx.scene3d.camera.look_at(-Vec3::Z);
    rig.ctx.scene3d.lighting.shadows = false;
    let levels =
        [(0.0, 10.0), (10.0, 20.0), (9.0, 11.0), (30.0, 40.0)].map(|(min_distance, max_distance)| InstanceLevel {
            mesh,
            min_distance,
            max_distance,
            camera: true,
            shadows: false,
        });
    let b = rig.ctx.gfx.create_instance_batch_levels(
        &levels,
        &[Mat4::from_translation(vec3(0.0, 0.0, -10.0))],
        &[],
        Default::default(),
    );
    rig.renderer.set_instance_batch(
        &rig.device,
        &rig.queue,
        b.id,
        rig.ctx.gfx.assets3d.instance_batches[b.id as usize].as_ref(),
    );
    rig.draw();
    let batch = rig.renderer.static_batches[b.id as usize].as_ref().unwrap();
    assert_eq!(
        rig.renderer.batch_record_bytes(b.id),
        Some(96 + 16 + 32),
        "one shared source record, including bounds"
    );
    assert_eq!(batch.views.len(), 3, "distant level and shadow views allocate nothing");
    let read = rig.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 60,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = rig.device.create_command_encoder(&Default::default());
    for i in 0..batch.views.len() {
        let (args, at) = rig.renderer.batch_view_args(b.id, i).unwrap();
        encoder.copy_buffer_to_buffer(args, at, &read, (i * 20) as u64, 20);
    }
    rig.queue.submit([encoder.finish()]);
    read.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    rig.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = read.get_mapped_range(..).unwrap();
    let counts: Vec<_> = (0..3)
        .map(|i| u32::from_le_bytes(bytes[i * 20 + 4..i * 20 + 8].try_into().unwrap()))
        .collect();
    assert_eq!(
        counts,
        [0, 1, 1],
        "the exclusive boundary and intentional overlap are evaluated per instance"
    );
}

#[test]
fn instance_levels_keep_off_center_meshes_and_hide_shadow_only_geometry() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    // Instance levels are chosen per instance by the GPU culling pass.
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::ComputeCulling) {
        return;
    }
    let mut data = MeshData::cube(1.0);
    for p in &mut data.positions {
        *p += vec3(0.0, 0.0, -10.0);
    }
    let mesh = rig.mesh(
        &data,
        MaterialDesc {
            emissive: Color::WHITE,
            ..Default::default()
        },
    );
    rig.ctx.scene3d.camera.position = Vec3::ZERO;
    rig.ctx.scene3d.camera.look_at(-Vec3::Z);
    rig.ctx.scene3d.lighting.shadows = false;
    for camera in [true, false] {
        let b = rig.ctx.gfx.create_instance_batch_levels(
            &[InstanceLevel {
                mesh,
                min_distance: 0.0,
                max_distance: 1.0,
                camera,
                shadows: true,
            }],
            &[Mat4::IDENTITY],
            &[],
            Default::default(),
        );
        rig.renderer.set_instance_batch(
            &rig.device,
            &rig.queue,
            b.id,
            rig.ctx.gfx.assets3d.instance_batches[b.id as usize].as_ref(),
        );
        let pixels = rig.draw();
        let i = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
        assert_eq!(
            pixels[i] > 150,
            camera,
            "origin bands must ignore an off-centre mesh's sphere distance, and honour camera flags"
        );
        rig.renderer.set_instance_batch(&rig.device, &rig.queue, b.id, None);
    }
}

#[test]
fn gpu_buffer_instances_explain_the_storage_requirement() {
    // Open an actual downlevel device even on desktop, so the unsupported
    // combination is checked without depending on the test runner's tier.
    let Some(mut rig) = Rig::with_limits(false, |_| {}) else {
        return;
    };
    let error = rig.renderer.create_shader(&rig.device, "storage.wgsl",
        "// instance positions\nfn vertex(v: Vertex) -> Vertex { var out = v; out.position += items[v.instance_index].xyz; return out; }").unwrap_err();
    assert_eq!((error.line, error.in_engine_code), (Some(2), false), "{error}");
    assert!(error.message.contains("this GPU has none"), "{error}");
    assert!(error.message.contains("use create_instance_batch"), "{error}");
    rig.renderer
        .create_shader(
            &rig.device,
            "comment.wgsl",
            "// items is only a comment\nfn surface(s: Surface) -> Surface { return s; }",
        )
        .unwrap();
    // A comment must not suppress validation of the custom shader either:
    // the old text-based fallback incorrectly accepted this broken hook.
    let error = rig
        .renderer
        .create_shader(
            &rig.device,
            "broken-comment.wgsl",
            "// items is only a comment\nfn surface(s: Surface) -> Surface { return 1.0; }",
        )
        .unwrap_err();
    assert_eq!((error.line, error.in_engine_code), (Some(2), false), "{error}");
    assert!(error.message.contains("Surface"), "{error}");
    let mesh = rig.mesh(&MeshData::cube(1.0), MaterialDesc::default());
    rig.renderer.create_gpu_buffer(&rig.device, 0, 1, &[]);
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rig.renderer.set_mesh_gpu_buffer(mesh.slot(), 0, 1);
    }))
    .expect_err("unsupported storage must be an actionable error, not an invisible draw");
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap();
    assert!(message.contains("require vertex storage buffers"), "{message}");
    assert!(message.contains("use create_instance_batch"), "{message}");
}
