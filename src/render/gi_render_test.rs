//! Global illumination: light bounced off a sunlit red wall tints the
//! floor beside it in a closed room. Skips without a GPU that has it.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{GlobalIllumination, MaterialDesc, Mesh, MeshData, Model};
use crate::three::{GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

pub(super) const SIZE: u32 = 128;

/// How a scene's boxes are made.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Build {
    Models,
    /// One instance batch per mesh, culled on that path.
    Batch(BatchPath),
    /// One empty instance batch per mesh, and a cell of it per box.
    Cells(BatchPath),
}

/// An offscreen renderer with a 1x1 white texture, and a `Ctx` to fill.
pub(super) struct Rig {
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) output: wgpu::Texture,
    pub(super) target: wgpu::TextureView,
    pub(super) textures: Vec<GpuTexture>,
    pub(super) renderer: Box<Renderer3D>,
    pub(super) ctx: Box<Ctx>,
    pub(super) size: (u32, u32),
    /// The GPU drawing (for reference images).
    pub(super) info: wgpu::AdapterInfo,
    /// The instance batches (one per mesh, in the order the meshes first
    /// come) and cells [`blocks`](Self::blocks) made.
    batches: Vec<crate::render::InstanceBatch>,
    cells: Vec<crate::render::InstanceCell>,
    /// How long the last [`frame`](Self::frame)'s `prepare` took, in ms.
    pub(super) prepare_ms: f64,
}

impl Rig {
    /// `None` without a GPU that can draw global illumination. `slots`
    /// false: a device at the default limits (16 texture slots, as WebGL2),
    /// which can't.
    pub(super) fn new(slots: bool) -> Option<Self> {
        Self::sized(slots, (SIZE, SIZE))
    }

    /// [`new`](Self::new), drawing `size` pixels, with GPU timestamps
    /// where the GPU has them.
    pub(super) fn sized(slots: bool, size: (u32, u32)) -> Option<Self> {
        Self::opened(Some(slots), size)
    }

    /// [`new`](Self::new) on whatever texture slots the device (or the
    /// forced tier) has: for what draws on any of them.
    pub(super) fn any() -> Option<Self> {
        Self::opened(None, (SIZE, SIZE))
    }

    /// A rig drawing `size` pixels; `slots`: as [`sized`](Self::sized),
    /// `None` takes the device as it comes.
    fn opened(slots: Option<bool>, size: (u32, u32)) -> Option<Self> {
        Self::opened_with(slots, size, None)
    }

    /// [`new`](Self::new) with global illumination's texture slots, on a
    /// device that has only `storage` storage buffers a shader stage:
    /// fewer than compute culling needs, so crowds and instance batches
    /// take their CPU paths while global illumination still runs.
    pub(super) fn with_storage_buffers(storage: u32) -> Option<Self> {
        Self::opened_with(Some(true), (SIZE, SIZE), Some(storage))
    }

    fn opened_with(slots: Option<bool>, size: (u32, u32), storage: Option<u32>) -> Option<Self> {
        let gpu = crate::render::render_test_support::gpu_with(|adapter| {
            let defaults = wgpu::Limits::default();
            // As the engine asks: more texture slots where the adapter has them.
            Ok(wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_sampled_textures_per_shader_stage: if slots != Some(false) {
                        adapter.limits().max_sampled_textures_per_shader_stage.clamp(16, 24)
                    } else {
                        16
                    },
                    max_storage_buffers_per_shader_stage: storage
                        .unwrap_or(defaults.max_storage_buffers_per_shader_stage),
                    ..defaults
                },
                // As the engine asks: block-compressed textures (baked
                // lightmaps) where the adapter has them.
                required_features: adapter.features()
                    & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TEXTURE_COMPRESSION_BC),
                ..Default::default()
            })
        })?;
        let (device, queue, info) = (gpu.device, gpu.queue, gpu.info);
        if let Some(slots) = slots
            && supports_gi(&device) != slots
        {
            crate::render::render_test_support::skip(format_args!(
                "the device {} the texture slot global illumination needs, and this test wants the opposite",
                if slots { "lacks" } else { "has" }
            ));
            return None;
        }
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
                width: size.0,
                height: size.1,
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
        let renderer = Box::new(Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true));
        let mut ctx = Box::new(Ctx::new(1.0 / 60.0, Vec2::new(size.0 as f32, size.1 as f32)));
        ctx.scene3d.sky.atmosphere = false;
        ctx.gfx.assets3d.decals_supported = supports_decals(&device);
        ctx.gfx.assets3d.gi_supported = true;
        Some(Self {
            device,
            queue,
            info,
            output,
            target,
            textures,
            renderer,
            ctx,
            size,
            batches: Vec::new(),
            cells: Vec::new(),
            prepare_ms: 0.0,
        })
    }

    /// A mesh of `data` with `desc`, on the CPU and the GPU.
    pub(super) fn mesh(&mut self, data: &MeshData, desc: MaterialDesc) -> Mesh {
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// Draws `frames` frames (the probes update a few each) and reads the
    /// last one back (RGBA).
    pub(super) fn draw(&mut self, frames: usize) -> Vec<u8> {
        for _ in 0..frames {
            let _ = self.frame(None);
        }
        self.read_back()
    }

    /// Collects and draws one frame, timed by `timer`; returns how long the
    /// GPU took from the submit to finishing it, in milliseconds.
    pub(super) fn frame(&mut self, mut timer: Option<&mut super::timer::GpuTimer>) -> f64 {
        if let Some(t) = timer.as_deref_mut() {
            t.collect(&self.device);
            t.begin_frame();
        }
        {
            let ctx = &mut self.ctx;
            ctx.gfx.frame3d.lod = ctx.scene3d.lod;
            ctx.gfx.frame3d.collect(
                &mut ctx.world,
                &ctx.gfx.assets3d,
                &SkinFrame::default(),
                &ctx.scene3d.camera,
                &ctx.scene3d.lighting,
            );
            let rgb = |c| {
                let [r, g, b, _] = crate::three::linear(c);
                Vec3::new(r, g, b)
            };
            let scene = Scene3D {
                frame: &ctx.gfx.frame3d,
                lighting: &ctx.scene3d.lighting,
                sky: &ctx.scene3d.sky,
                light: SkyLight {
                    sun: Vec3::splat(ctx.scene3d.lighting.sun_intensity),
                    sky: rgb(ctx.scene3d.sky.sky_color),
                    ground: rgb(ctx.scene3d.sky.ground_color),
                },
                camera: ctx.scene3d.camera.position,
                post: &ctx.scene3d.post,
                time: (0.0, 1.0 / 60.0),
                occlusion: false,
            };
            let prepare = std::time::Instant::now();
            self.renderer.prepare(
                &self.device,
                &self.queue,
                &Prepare3D {
                    scene: &scene,
                    lut: None,
                    size: self.size,
                },
            );
            self.prepare_ms = prepare.elapsed().as_secs_f64() * 1000.0;
            let mut buffers = self.renderer.draw(
                &self.device,
                &self.target,
                wgpu::Color::BLACK,
                &self.textures,
                timer.as_deref(),
            );
            if let Some(t) = timer.as_deref_mut() {
                let mut encoder = self.device.create_command_encoder(&Default::default());
                t.resolve(&mut encoder);
                buffers.push(encoder.finish());
            }
            let start = std::time::Instant::now();
            self.queue.submit(buffers);
            self.renderer.submitted();
            if let Some(t) = timer {
                t.submitted();
            }
            self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            start.elapsed().as_secs_f64() * 1000.0
        }
    }

    /// The last frame drawn (RGBA).
    pub(super) fn read_back(&self) -> Vec<u8> {
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(self.size.0 * self.size.1 * 4),
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
                    bytes_per_row: Some(self.size.0 * 4),
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

    /// A box from `lo` to `hi`.
    fn block(&mut self, mesh: Mesh, lo: Vec3, hi: Vec3) {
        self.ctx.world.spawn((
            Transform3D {
                position: (lo + hi) * 0.5,
                scale: hi - lo,
                ..Default::default()
            },
            Model::new(mesh),
        ));
    }

    /// A closed white room (the inside from -3.5 to 3.5 across, -0.5 to
    /// 3.5 high) whose west wall is red, with the sun shining in through
    /// an opening in the roof onto that wall (not the floor), and the
    /// camera inside, looking west.
    fn red_wall_room(gi: bool, slots: bool) -> Option<Self> {
        Self::red_wall_room_of(gi, slots, Build::Models)
    }

    /// [`red_wall_room`](Self::red_wall_room), its walls, floor and roof
    /// models or instance batches on path `batch`, one per color.
    fn red_wall_room_of(gi: bool, slots: bool, batch: Build) -> Option<Self> {
        let mut rig = Self::new(slots)?;
        let white = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
        let red = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::rgb(0.9, 0.04, 0.04)));
        let (inner, outer) = (3.5, 3.9);
        let blocks = [
            (white, vec3(-outer, -0.9, -outer), vec3(outer, -0.5, outer)), // floor
            (red, vec3(-outer, -0.5, -outer), vec3(-inner, 3.5, outer)),   // west
            (white, vec3(inner, -0.5, -outer), vec3(outer, 3.5, outer)),   // east
            (white, vec3(-inner, -0.5, -outer), vec3(inner, 3.5, -inner)), // north
            (white, vec3(-inner, -0.5, inner), vec3(inner, 3.5, outer)),   // south
            // The roof, with an opening from x -2 to 0 and z -1.5 to 1.5.
            (white, vec3(-outer, 3.5, -outer), vec3(-2.0, 3.9, outer)),
            (white, vec3(0.0, 3.5, -outer), vec3(outer, 3.9, outer)),
            (white, vec3(-2.0, 3.5, -outer), vec3(0.0, 3.9, -1.5)),
            (white, vec3(-2.0, 3.5, 1.5), vec3(0.0, 3.9, outer)),
        ];
        rig.blocks(&blocks, batch);
        let lighting = &mut rig.ctx.scene3d.lighting;
        // From the upper east: through the opening onto the west wall,
        // between 0 and 2 high.
        lighting.sun_direction = vec3(-1.0, -1.0, 0.0);
        lighting.sun_color = Color::WHITE;
        lighting.sun_intensity = 4.0;
        rig.ctx.scene3d.sky.sky_color = Color::rgb(0.5, 0.5, 0.5);
        rig.ctx.scene3d.sky.ground_color = Color::rgb(0.5, 0.5, 0.5);
        lighting.ambient_intensity = 0.3;
        lighting.shadow_distance = 30.0;
        lighting.gi = GlobalIllumination {
            enabled: gi,
            extent: vec3(12.0, 4.0, 8.0),
            spacing: 1.0,
            probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
            intensity: 1.0,
        };
        rig.ctx.scene3d.camera.position = vec3(2.0, 1.3, 0.3);
        rig.ctx.scene3d.camera.look_at(vec3(-3.5, -0.2, 0.0));
        Some(rig)
    }

    /// A white box (the inside from -3.5 to 3.5 across, -0.5 to 3.5 high,
    /// walls 0.4 thick, between the probe planes), no sun and no ambient
    /// light, and a shadowed point light outside it, east and above the
    /// roof. `open`: without the east wall, so the light shines in. The
    /// camera is inside, looking up at the roof, which the light never
    /// reaches directly: whatever lights it is bounced light. `batch`:
    /// the box is one instance batch on that path instead of models.
    fn light_box(open: bool, gi: bool, batch: Build) -> Option<Self> {
        let mut rig = Self::new(true)?;
        let white = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
        let (inner, outer) = (3.5, 3.9);
        let mut walls = vec![
            (white, vec3(-outer, -0.9, -outer), vec3(outer, -0.5, outer)), // floor
            (white, vec3(-outer, 3.5, -outer), vec3(outer, 3.9, outer)),   // roof
            (white, vec3(-outer, -0.5, -outer), vec3(-inner, 3.5, outer)), // west
            (white, vec3(-inner, -0.5, -outer), vec3(inner, 3.5, -inner)), // north
            (white, vec3(-inner, -0.5, inner), vec3(inner, 3.5, outer)),   // south
        ];
        if !open {
            walls.push((white, vec3(inner, -0.5, -outer), vec3(outer, 3.5, outer))); // east
        }
        rig.blocks(&walls, batch);
        rig.ctx.world.spawn((
            Transform3D::at(vec3(7.0, 5.0, 0.0)),
            crate::d3::PointLight {
                intensity: 400.0,
                range: 20.0,
                shadows: true,
                ..Default::default()
            },
        ));
        let lighting = &mut rig.ctx.scene3d.lighting;
        lighting.sun_intensity = 0.0;
        lighting.ambient_intensity = 0.0;
        rig.ctx.scene3d.sky.sky_color = Color::BLACK;
        rig.ctx.scene3d.sky.ground_color = Color::BLACK;
        lighting.gi = GlobalIllumination {
            enabled: gi,
            extent: vec3(12.0, 6.0, 12.0),
            spacing: 1.0,
            probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
            intensity: 1.0,
        };
        rig.ctx.scene3d.camera.position = vec3(-1.5, 0.5, 0.3);
        rig.ctx.scene3d.camera.look_at(vec3(0.5, 3.5, 0.0));
        Some(rig)
    }

    /// Boxes of a mesh each, from `lo` to `hi`, built as `build` says.
    fn blocks(&mut self, blocks: &[(Mesh, Vec3, Vec3)], build: Build) {
        let (path, cells) = match build {
            Build::Models => {
                for &(mesh, lo, hi) in blocks {
                    self.block(mesh, lo, hi);
                }
                return;
            }
            Build::Batch(path) => (path, false),
            Build::Cells(path) => (path, true),
        };
        self.renderer.batching.path = path;
        let transform =
            |lo: Vec3, hi: Vec3| Mat4::from_scale_rotation_translation(hi - lo, Quat::IDENTITY, (lo + hi) * 0.5);
        let mut meshes: Vec<Mesh> = Vec::new();
        for b in blocks {
            if !meshes.contains(&b.0) {
                meshes.push(b.0);
            }
        }
        let options = crate::render::InstanceBatchOptions::default();
        let mut batches = Vec::new();
        for &mesh in &meshes {
            let transforms: Vec<Mat4> = if cells {
                Vec::new()
            } else {
                blocks
                    .iter()
                    .filter(|b| b.0 == mesh)
                    .map(|&(_, lo, hi)| transform(lo, hi))
                    .collect()
            };
            let batch = self.ctx.gfx.create_instance_batch(mesh, &transforms, options);
            self.sync_row(batch.id);
            batches.push(batch);
        }
        self.batches.extend_from_slice(&batches);
        if cells {
            for &(mesh, lo, hi) in blocks {
                let batch = batches[meshes.iter().position(|&m| m == mesh).unwrap()];
                self.add_cell(batch, transform(lo, hi));
            }
        }
    }

    /// Hands row `row` of the batch table to the renderer, as `Graphics`
    /// does.
    fn sync_row(&mut self, row: u32) {
        self.renderer.set_instance_batch(
            &self.device,
            &self.queue,
            row,
            self.ctx.gfx.assets3d.instance_batches[row as usize].as_ref(),
        );
    }

    /// Adds a cell of one instance at `transform` to `batch`, on both sides.
    fn add_cell(&mut self, batch: crate::render::InstanceBatch, transform: Mat4) -> crate::render::InstanceCell {
        let cell = self.ctx.gfx.add_instance_cell(batch, &[transform], &[]);
        let row = self.ctx.gfx.instance_cell_row(cell).expect("uploaded");
        self.sync_row(row);
        self.cells.push(cell);
        cell
    }

    /// Frees `cell` on both sides.
    fn free_cell(&mut self, cell: crate::render::InstanceCell) {
        let row = self.ctx.gfx.instance_cell_row(cell).expect("live");
        self.ctx.free(cell);
        self.sync_row(row);
    }

    /// The color of the world point `p` in `pixels`.
    pub(super) fn at(&self, pixels: &[u8], p: Vec3) -> [i32; 3] {
        let c = self.ctx.scene3d.camera.view_proj() * p.extend(1.0);
        let ndc = c.truncate() / c.w;
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
        let i = ((y * SIZE + x) * 4) as usize;
        [0, 1, 2].map(|k| i32::from(pixels[i + k]))
    }
}

#[test]
fn a_sunlit_red_wall_tints_the_floor_beside_it_red() {
    let Some(mut off) = Rig::red_wall_room(false, true) else {
        return;
    };
    let Some(mut on) = Rig::red_wall_room(true, true) else {
        return;
    };
    // Enough frames for every probe to look around twice (two bounces).
    let (dark, lit) = (off.draw(1), on.draw(80));
    let near_red = vec3(-3.0, -0.5, -0.8);
    let far_side = vec3(-0.5, -0.5, -0.8);
    let sunlit_wall = vec3(-3.5, 1.0, 0.0);
    let (a, b) = (off.at(&dark, near_red), on.at(&lit, near_red));
    let far = on.at(&lit, far_side);
    eprintln!(
        "floor by the red wall: {a:?} off, {b:?} on; further away {:?} off, {far:?} on; sunlit wall {:?}",
        off.at(&dark, far_side),
        on.at(&lit, sunlit_wall)
    );
    // The sun lights the wall, not the floor.
    assert!(on.at(&lit, sunlit_wall)[0] > 150, "the wall is in the sun");
    // Without bounced light, the floor takes only the grey ambient light.
    assert!((a[0] - a[1]).abs() <= 4, "no red without bounced light: {a:?}");
    // With it, red from the wall, most beside it.
    assert!(b[0] > b[1] + 20, "red bounce light on the floor: {b:?}");
    assert!(
        b[0] - b[1] > far[0] - far[1],
        "redder by the red wall: {b:?} vs {far:?}"
    );
}

/// How bright the roof inside [`Rig::light_box`] is (the sum of its red,
/// green and blue), over a few points of it.
fn roof_inside(open: bool, gi: bool, batch: Build) -> Option<i32> {
    let mut rig = Rig::light_box(open, gi, batch)?;
    let pixels = rig.draw(if gi { 80 } else { 1 });
    let points = [vec3(0.5, 3.5, 0.0), vec3(-0.5, 3.5, 1.0), vec3(1.0, 3.5, -1.0)];
    Some(
        points
            .iter()
            .map(|&p| rig.at(&pixels, p).iter().sum::<i32>())
            .max()
            .unwrap(),
    )
}

#[test]
fn a_light_outside_a_closed_box_adds_no_bounce_inside() {
    let Some(dark) = roof_inside(false, false, Build::Models) else {
        return;
    };
    let Some(open) = roof_inside(true, true, Build::Models) else {
        return;
    };
    let Some(closed) = roof_inside(false, true, Build::Models) else {
        return;
    };
    eprintln!("roof inside the box: {dark} without bounced light, {open} with the box open, {closed} closed");
    // The light never reaches the roof itself.
    assert!(dark <= 6, "no direct light on the roof: {dark}");
    // Positive control: through the open side, it bounces off the floor
    // and walls onto the roof.
    assert!(open > 60, "bounced light through the open side: {open}");
    // Closed, the probes see the light's shadow: nothing gets in (before
    // they did, the roof was as bright as in the open box). What is left
    // is the probes' own leak along the walls' seams.
    assert!(
        closed * 20 < open,
        "no bounced light in the closed box: {closed} vs {open} open"
    );
}

/// How bright the floor inside a white box under the sun and a grey sky is
/// (the sum of red, green and blue, the brightest of a few points), after
/// `frames` frames with global illumination: the box closed, or open to the
/// east (`open`). The probe grid reaches past the roof, so the camera's
/// view takes all of its light from the probes.
fn floor_in_the_sky(open: bool, frames: usize) -> Option<i32> {
    let mut rig = Rig::new(true)?;
    let white = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    let (inner, outer) = (3.5, 3.9);
    let mut walls = vec![
        (white, vec3(-outer, -0.9, -outer), vec3(outer, -0.5, outer)), // floor
        (white, vec3(-outer, 3.5, -outer), vec3(outer, 3.9, outer)),   // roof
        (white, vec3(-outer, -0.5, -outer), vec3(-inner, 3.5, outer)), // west
        (white, vec3(-inner, -0.5, -outer), vec3(inner, 3.5, -inner)), // north
        (white, vec3(-inner, -0.5, inner), vec3(inner, 3.5, outer)),   // south
    ];
    if !open {
        walls.push((white, vec3(inner, -0.5, -outer), vec3(outer, 3.5, outer))); // east
    }
    rig.blocks(&walls, Build::Models);
    let lighting = &mut rig.ctx.scene3d.lighting;
    lighting.sun_direction = vec3(-1.0, -1.0, 0.3);
    lighting.sun_intensity = 4.0;
    lighting.ambient_intensity = 0.3;
    lighting.shadow_distance = 30.0;
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.5, 0.5, 0.5);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.5, 0.5, 0.5);
    lighting.gi = GlobalIllumination {
        enabled: true,
        extent: vec3(12.0, 8.0, 12.0),
        spacing: 1.0,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        intensity: 1.0,
    };
    rig.ctx.scene3d.camera.position = vec3(-2.5, 2.5, 0.3);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, -0.5, 0.0));
    let pixels = rig.draw(frames);
    let points = [vec3(0.0, -0.5, 0.0), vec3(-1.0, -0.5, 1.0), vec3(1.0, -0.5, -1.0)];
    Some(
        points
            .iter()
            .map(|&p| rig.at(&pixels, p).iter().sum::<i32>())
            .max()
            .unwrap(),
    )
}

#[test]
fn a_closed_box_under_the_sky_keeps_no_sky_light_inside() {
    let Some(open) = floor_in_the_sky(true, 80) else {
        return;
    };
    let Some(closed) = floor_in_the_sky(false, 80) else {
        return;
    };
    eprintln!("floor inside the box under the sky: {open} open, {closed} closed");
    // Positive control: open to the east, the sun's and sky's light comes in.
    assert!(open > 150, "light through the open side: {open}");
    // Closed, nothing gets in. Before, the probes' views took the sky's
    // ambient light while their probes hadn't looked yet, and the closed
    // box kept it: the floor was nearly as bright as with the box open.
    assert!(
        closed * 10 < open,
        "no sky light in the closed box: {closed} vs {open} open"
    );
}

#[test]
fn instance_batches_bounce_and_block_light_like_models() {
    // Both paths: culled on the CPU, and by the compute pass (where the
    // device has it; elsewhere that batch takes the CPU path too).
    for path in [BatchPath::Cpu, BatchPath::Gpu] {
        // The red wall room, its walls, floor and roof as batches: the red
        // wall tints the floor (the probes see it), and the roof keeps the
        // sun off the floor (their sun shadow map has it).
        let Some(mut models) = Rig::red_wall_room_of(true, true, Build::Models) else {
            return;
        };
        let Some(mut batches) = Rig::red_wall_room_of(true, true, Build::Batch(path)) else {
            return;
        };
        let (a, b) = (models.draw(80), batches.draw(80));
        // Batches culled on the GPU keep a list as long as the batch for
        // every view: the probes add seven (their shadow map, six face
        // directions), however many probes update (16 here).
        for batch in batches
            .renderer
            .static_batches
            .iter()
            .flatten()
            .filter(|b| b.route == Route::Gpu)
        {
            let most = 1 + crate::three::MAX_CASCADES + crate::three::gi::GI_BATCH_FACES;
            assert!(batch.views.len() <= most, "{} views", batch.views.len());
        }
        let near_red = vec3(-3.0, -0.5, -0.8);
        let far_side = vec3(-0.5, -0.5, -0.8);
        let (m, n) = (models.at(&a, near_red), batches.at(&b, near_red));
        let (mf, nf) = (models.at(&a, far_side), batches.at(&b, far_side));
        eprintln!("{path:?}: floor by the red wall: {m:?} models, {n:?} batches; further away {mf:?}, {nf:?}");
        assert!(n[0] > n[1] + 20, "{path:?}: red bounce light from a batch wall: {n:?}");
        for (x, y) in [(m, n), (mf, nf)] {
            assert!(
                (0..3).all(|k| (x[k] - y[k]).abs() <= 12),
                "{path:?}: batches light the floor as models do: {x:?} vs {y:?}"
            );
        }
        // A batch box closed around the light's shadow lets nothing in;
        // open, it bounces the light as models do.
        let Some(open) = roof_inside(true, true, Build::Batch(path)) else {
            return;
        };
        let Some(closed) = roof_inside(false, true, Build::Batch(path)) else {
            return;
        };
        eprintln!("{path:?}: roof inside a batch box: {open} open, {closed} closed");
        assert!(open > 60, "{path:?}: bounced light off batch walls: {open}");
        assert!(
            closed * 20 < open,
            "{path:?}: no bounced light in the closed batch box: {closed} vs {open}"
        );
    }
}

#[test]
fn instance_cells_bounce_and_block_light_like_one_batch() {
    for path in [BatchPath::Cpu, BatchPath::Gpu] {
        // The red wall room as one batch per color, and as one cell per
        // wall (of an empty batch per color): the same picture.
        let Some(mut whole) = Rig::red_wall_room_of(true, true, Build::Batch(path)) else {
            return;
        };
        let Some(mut cells) = Rig::red_wall_room_of(true, true, Build::Cells(path)) else {
            return;
        };
        let (a, b) = (whole.draw(80), cells.draw(80));
        let near_red = vec3(-3.0, -0.5, -0.8);
        let far_side = vec3(-0.5, -0.5, -0.8);
        let (w, c) = (whole.at(&a, near_red), cells.at(&b, near_red));
        let (wf, cf) = (whole.at(&a, far_side), cells.at(&b, far_side));
        eprintln!("{path:?}: floor by the red wall: {w:?} one batch, {c:?} cells; further away {wf:?}, {cf:?}");
        assert!(c[0] > c[1] + 20, "{path:?}: red bounce light from a cell: {c:?}");
        for (x, y) in [(w, c), (wf, cf)] {
            assert!(
                (0..3).all(|k| (x[k] - y[k]).abs() <= 12),
                "{path:?}: cells light the floor as one batch does: {x:?} vs {y:?}"
            );
        }
        // What the batch rows keep for their views, with every wall in.
        let (views, lists, most) = cells.renderer.batch_view_lists();
        let shadow_views = 1 + crate::three::MAX_CASCADES;
        let red_row = cells.ctx.gfx.instance_cell_row(cells.cells[1]).unwrap();
        let bound = if cells.renderer.batch_route(red_row) == Some(Route::Gpu) {
            // One view per culling view a GPU row is seen in.
            shadow_views + crate::three::gi::GI_BATCH_FACES
        } else {
            // A list per view: the probes' at most 7 + 6 x 16.
            shadow_views
                + crate::three::gi::GI_BATCH_FACES
                + 6 * crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME as usize
        };
        assert!(
            most <= bound,
            "{path:?}: a row keeps {most} views and lists, at most {bound}"
        );
        // The red wall's cell freed: the probes that look from then on
        // don't see it, so its light fades with every round of updates
        // (585 probes, 16 a frame). Not at once: each update keeps some of
        // the old light, and the probes' red light on the floor is what
        // other probes see, bounce after bounce.
        let tint = |rig: &Rig, pixels: &[u8]| {
            let c = rig.at(pixels, near_red);
            c[0] - c[1]
        };
        let before = tint(&cells, &b);
        let red = cells.cells[1];
        cells.free_cell(red);
        let round = cells.draw(37);
        let later = cells.draw(400);
        let (after_round, after_more) = (tint(&cells, &round), tint(&cells, &later));
        eprintln!("{path:?}: red tint by the wall {before}, freed: {after_round} after a round, {after_more} later");
        assert!(
            after_round * 10 < before * 7,
            "{path:?}: a freed cell stops bouncing light: {after_round} vs {before}"
        );
        assert!(after_more <= 4, "{path:?}: none of its light is left: {after_more}");
        // Cells coming and going reuse rows: what the rows keep for their
        // views doesn't grow, and stays within the bound.
        for _ in 0..10 {
            let red_batch = cells.batches[1];
            let wall = Mat4::from_scale_rotation_translation(vec3(0.4, 4.0, 7.8), Quat::IDENTITY, vec3(-3.7, 1.5, 0.0));
            let cell = cells.add_cell(red_batch, wall);
            let _ = cells.draw(3);
            let now = cells.renderer.batch_view_lists();
            assert!(
                now.0 <= views && now.1 <= lists && now.2 <= bound,
                "{path:?}: rows keep {now:?} views, lists and most, against {:?} before",
                (views, lists, most)
            );
            cells.free_cell(cell);
            let _ = cells.draw(3);
        }
    }
}

#[test]
fn probe_views_leave_out_subpixel_instances_of_level_batches_too() {
    // The same instances three times, on the CPU path: tiny cubes as one
    // mesh, as two distance levels, and big ones. The probes' views keep
    // the tiny ones only near the probes, whether the batch has levels
    // or not.
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    rig.renderer.batching.path = BatchPath::Cpu;
    let cube = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    let mut tiny = Vec::new();
    let mut big = Vec::new();
    for i in 0..41 {
        for j in 0..41 {
            let at = vec3(i as f32 * 0.5 - 10.0, 0.5, j as f32 * 0.5 - 10.0);
            tiny.push(Mat4::from_scale_rotation_translation(
                Vec3::splat(0.1),
                Quat::IDENTITY,
                at,
            ));
            big.push(Mat4::from_scale_rotation_translation(
                Vec3::splat(2.0),
                Quat::IDENTITY,
                at,
            ));
        }
    }
    let options = crate::render::InstanceBatchOptions::default();
    let one = rig.ctx.gfx.create_instance_batch(cube, &tiny, options);
    let level = |min_distance, max_distance| crate::render::InstanceLevel {
        mesh: cube,
        min_distance,
        max_distance,
        camera: true,
        shadows: true,
    };
    let levels =
        rig.ctx
            .gfx
            .create_instance_batch_levels(&[level(0.0, 5.0), level(5.0, f32::INFINITY)], &tiny, &[], options);
    let large = rig.ctx.gfx.create_instance_batch(cube, &big, options);
    for batch in [one, levels, large] {
        rig.sync_row(batch.id);
    }
    rig.ctx.scene3d.lighting.gi = GlobalIllumination {
        enabled: true,
        extent: vec3(12.0, 6.0, 12.0),
        spacing: 1.0,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        intensity: 1.0,
    };
    rig.ctx.scene3d.camera.position = vec3(0.3, 2.0, 0.2);
    rig.ctx.scene3d.camera.look_at(vec3(5.0, 0.0, 3.0));
    let _ = rig.draw(1);
    let count = |b: crate::render::InstanceBatch| rig.renderer.probe_view_instances(b.id);
    let (one, levels, large) = (count(one), count(levels), count(large));
    eprintln!("instances in this frame's probe views: {one} tiny, {levels} tiny in levels, {large} big");
    // Big ones are all wider than a probe pixel: what the views hold.
    assert!(large > 1000, "positive control: the probes see the field: {large}");
    assert!(
        one * 4 < large,
        "most tiny ones are under a probe pixel: {one} of {large}"
    );
    assert!(one > 0, "the nearest tiny ones are kept: {one}");
    // Levels: the same cut (before, their cells were drawn whole).
    assert_eq!(levels, one, "a batch of levels keeps what a batch of one mesh keeps");
}

/// A field of `n` x `n` cubes 0.5 apart around the origin, each `size` wide.
fn cube_field(n: usize, size: f32) -> Vec<Mat4> {
    let half = (n - 1) as f32 * 0.25;
    (0..n * n)
        .map(|k| {
            let at = vec3((k / n) as f32 * 0.5 - half, 0.5, (k % n) as f32 * 0.5 - half);
            Mat4::from_scale_rotation_translation(Vec3::splat(size), Quat::IDENTITY, at)
        })
        .collect()
}

/// The probe grid of the subpixel tests, its 16 probes a frame by the camera.
fn field_gi(rig: &mut Rig, enabled: bool) {
    rig.ctx.scene3d.lighting.gi = GlobalIllumination {
        enabled,
        extent: vec3(12.0, 6.0, 12.0),
        spacing: 1.0,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        intensity: 1.0,
    };
    rig.ctx.scene3d.camera.position = vec3(0.3, 2.0, 0.2);
    rig.ctx.scene3d.camera.look_at(vec3(5.0, 0.0, 3.0));
}

#[test]
fn level_batches_keep_probe_radii_only_while_gi_is_on() {
    // A batch of distance levels on the CPU path keeps 4 bytes an
    // instance for the probes' size cut: none while global illumination
    // is off, and none again once it was off for good.
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    rig.renderer.batching.path = BatchPath::Cpu;
    let cube = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    let tiny = cube_field(41, 0.1);
    let level = |min_distance, max_distance| crate::render::InstanceLevel {
        mesh: cube,
        min_distance,
        max_distance,
        camera: true,
        shadows: true,
    };
    let levels = [level(0.0, 5.0), level(5.0, f32::INFINITY)];
    let options = crate::render::InstanceBatchOptions::default();
    let batch = rig.ctx.gfx.create_instance_batch_levels(&levels, &tiny, &[], options);
    let one = rig.ctx.gfx.create_instance_batch(cube, &tiny, options);
    rig.sync_row(batch.id);
    rig.sync_row(one.id);
    let radii = tiny.len().div_ceil(4) * 16;
    field_gi(&mut rig, false);
    let _ = rig.draw(3);
    let (off, shared) = rig.renderer.probe_radii_bytes(batch.id);
    eprintln!("{} instances: {off} bytes of probe radii with GI off", tiny.len());
    assert_eq!(off, 0, "GI off: nothing per instance");
    assert!(shared, "the bounding spheres are the engine's, not a copy");
    // On: built the first frame, and the cut is the same as before.
    rig.ctx.scene3d.lighting.gi.enabled = true;
    let _ = rig.draw(1);
    let on = rig.renderer.probe_radii_bytes(batch.id).0;
    eprintln!("{on} bytes with GI on");
    assert_eq!(on, radii, "on: 4 bytes an instance");
    let count = |rig: &Rig, b: crate::render::InstanceBatch| rig.renderer.probe_view_instances(b.id);
    let (in_levels, in_one) = (count(&rig, batch), count(&rig, one));
    assert!(in_one > 0, "the probes see the nearest ones: {in_one}");
    assert_eq!(in_levels, in_one, "levels cut as a batch of one mesh does");
    // A batch made while it is on has them at once.
    let later = rig.ctx.gfx.create_instance_batch_levels(&levels, &tiny, &[], options);
    rig.sync_row(later.id);
    assert_eq!(rig.renderer.probe_radii_bytes(later.id).0, radii);
    // Off for a moment: kept. Off for good: dropped.
    rig.ctx.scene3d.lighting.gi.enabled = false;
    let _ = rig.draw(Renderer3D::PROBE_RADII_FRAMES as usize - 1);
    assert_eq!(rig.renderer.probe_radii_bytes(batch.id).0, radii, "off for a moment");
    rig.ctx.scene3d.lighting.gi.enabled = true;
    let _ = rig.draw(1);
    rig.ctx.scene3d.lighting.gi.enabled = false;
    let _ = rig.draw(Renderer3D::PROBE_RADII_FRAMES as usize);
    for b in [batch, later] {
        assert_eq!(rig.renderer.probe_radii_bytes(b.id).0, 0, "off for good");
    }
    // And on again: the same cut.
    rig.ctx.scene3d.lighting.gi.enabled = true;
    let _ = rig.draw(1);
    assert_eq!(rig.renderer.probe_radii_bytes(batch.id).0, radii);
    assert_eq!(count(&rig, batch), count(&rig, one));
}

#[test]
fn gpu_probe_views_leave_out_subpixel_instances() {
    // The CPU path's size cut, on the GPU path: tiny cubes and big ones at
    // the same places, culled on the GPU. The probes' face views keep the
    // tiny ones only near this frame's probes.
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    rig.renderer.batching.path = BatchPath::Gpu;
    let cube = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    let options = crate::render::InstanceBatchOptions::default();
    let tiny = rig.ctx.gfx.create_instance_batch(cube, &cube_field(41, 0.1), options);
    let large = rig.ctx.gfx.create_instance_batch(cube, &cube_field(41, 2.0), options);
    for batch in [tiny, large] {
        rig.sync_row(batch.id);
        if rig.renderer.batch_route(batch.id) != Some(Route::Gpu) {
            crate::render::render_test_support::skip("no GPU culling");
            return;
        }
    }
    field_gi(&mut rig, true);
    let mut counts = [[0; 2]; 2];
    for (cut, row) in [false, true].into_iter().zip(&mut counts) {
        rig.renderer.batching.no_probe_cut = !cut;
        // The same probes, the frame with and without the cut: the grid
        // starts over for each.
        rig.ctx.scene3d.lighting.gi.enabled = false;
        let _ = rig.draw(1);
        rig.ctx.scene3d.lighting.gi.enabled = true;
        let _ = rig.draw(1);
        for (count, batch) in row.iter_mut().zip([tiny, large]) {
            *count = rig.renderer.gpu_probe_view_instances(&rig.device, &rig.queue, batch.id);
        }
    }
    let [[tiny_before, large_before], [tiny_after, large_after]] = counts;
    eprintln!(
        "instances drawn in the probes' face views, before the cut: {tiny_before} tiny, \
         {large_before} big; with it: {tiny_after} tiny, {large_after} big"
    );
    assert!(
        large_before > 1000,
        "positive control: the views see the field: {large_before}"
    );
    assert_eq!(
        large_after, large_before,
        "big ones are all wider than a pixel: all kept"
    );
    assert!(
        tiny_before > 1000,
        "without the cut, the tiny ones are drawn too: {tiny_before}"
    );
    assert!(
        tiny_after * 4 < tiny_before,
        "most tiny ones are left out: {tiny_after} of {tiny_before}"
    );
    assert!(tiny_after > 0, "the ones near the probes are kept: {tiny_after}");
}

/// A 40 x 40 field of tiny cubes (0.1 wide, 0.5 apart) in cells of 2,
/// with one big cube (2 wide) among them at (15, 0.5, 15), far from this
/// frame's probes: as one batch of one mesh and as a batch of two distance
/// levels, on `path`. The probes' views are drawn with and without leaving
/// out cells far from every probe first; gives the instances in the probe
/// views of either batch each way, as `count` reads them, and the cells the
/// CPU path left out whole.
fn far_cells(path: BatchPath, count: impl Fn(&Rig, u32) -> u32) -> Option<([[u32; 2]; 2], [u32; 2])> {
    let mut rig = Rig::new(true)?;
    rig.renderer.batching.path = path;
    let cube = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    let mut field: Vec<Mat4> = (0..80 * 80)
        .map(|k| {
            let at = vec3((k / 80) as f32 * 0.5 - 20.0, 0.5, (k % 80) as f32 * 0.5 - 20.0);
            Mat4::from_scale_rotation_translation(Vec3::splat(0.1), Quat::IDENTITY, at)
        })
        .collect();
    field.push(Mat4::from_scale_rotation_translation(
        Vec3::splat(2.0),
        Quat::IDENTITY,
        vec3(15.0, 0.5, 15.0),
    ));
    let options = crate::render::InstanceBatchOptions {
        cell_size: 2.0,
        ..Default::default()
    };
    let one = rig.ctx.gfx.create_instance_batch(cube, &field, options);
    let level = |min_distance, max_distance| crate::render::InstanceLevel {
        mesh: cube,
        min_distance,
        max_distance,
        camera: true,
        shadows: true,
    };
    let levels =
        rig.ctx
            .gfx
            .create_instance_batch_levels(&[level(0.0, 5.0), level(5.0, f32::INFINITY)], &field, &[], options);
    for batch in [one, levels] {
        rig.sync_row(batch.id);
        let route = rig.renderer.batch_route(batch.id);
        let gpu = route == Some(Route::Gpu);
        if gpu != (path == BatchPath::Gpu) {
            crate::render::render_test_support::skip("the batch did not take the path asked for");
            return None;
        }
    }
    field_gi(&mut rig, true);
    let mut counts = [[0; 2]; 2];
    let mut skipped = [0; 2];
    for (skip, (row, cells)) in [false, true].into_iter().zip(counts.iter_mut().zip(&mut skipped)) {
        rig.renderer.batching.no_far_cells = !skip;
        // The same probes, the frame with and without: the probes' lists
        // start over for each.
        rig.ctx.scene3d.lighting.gi.enabled = false;
        let _ = rig.draw(1);
        rig.ctx.scene3d.lighting.gi.enabled = true;
        let _ = rig.draw(1);
        *cells = rig.renderer.batching.far_cells;
        for (n, batch) in row.iter_mut().zip([one, levels]) {
            *n = count(&rig, batch.id);
        }
    }
    Some((counts, skipped))
}

#[test]
fn probe_views_leave_out_whole_cells_far_from_every_probe() {
    // The CPU path: a probe's view leaves out a cell whose widest instance
    // is under one of its pixels at the cell's nearest point before
    // testing its instances, and keeps exactly what it kept before.
    let Some(([before, after], [off, on])) = far_cells(BatchPath::Cpu, |rig, id| rig.renderer.probe_view_instances(id))
    else {
        return;
    };
    eprintln!(
        "probe view instances (one mesh, levels): {before:?} testing every cell, {after:?} leaving out {on} far cells"
    );
    assert!(
        before[0] > 0,
        "positive control: the probes see the near cubes: {before:?}"
    );
    assert_eq!(off, 0, "without it, no cell is left out whole");
    assert!(on > 1000, "most cells are far from every probe: {on}");
    assert_eq!(after, before, "the same instances drawn either way");
    assert_eq!(after[0], after[1], "levels keep what one mesh keeps");
}

#[test]
fn gpu_probe_views_leave_out_whole_cells_far_from_every_probe() {
    // The GPU culling shader's cell test: the same instances drawn in the
    // probes' face views with and without it.
    let Some(([before, after], _)) = far_cells(BatchPath::Gpu, |rig, id| {
        rig.renderer.gpu_probe_view_instances(&rig.device, &rig.queue, id)
    }) else {
        return;
    };
    eprintln!(
        "GPU probe face view instances (one mesh, levels): {before:?} testing every cell, {after:?} leaving far ones out"
    );
    assert!(
        before[0] > 0,
        "positive control: the probes see the near cubes: {before:?}"
    );
    assert_eq!(after, before, "the same instances drawn either way");
}

#[test]
fn without_the_texture_slot_bounced_light_is_skipped() {
    // As on WebGL2: the frame is the same as with it off.
    let Some(mut off) = Rig::red_wall_room(false, false) else {
        return;
    };
    let Some(mut on) = Rig::red_wall_room(true, false) else {
        return;
    };
    on.ctx.gfx.assets3d.gi_supported = true; // even if the engine asked for it
    assert_eq!(off.draw(1), on.draw(3));
}

/// GPU cost of global illumination in a town-like scene at 1920x1080: off,
/// at the defaults, and at the most probes a frame. The town has 900
/// buildings (models), 1682 bushes in one instance batch and a shadowed
/// point light by the camera. Run in release (`cargo test --release --lib
/// --features 3d -- --ignored --nocapture gi_cost`); prints the GPU frame
/// time and the GI passes' times from the engine's timestamp profiler, and
/// the CPU time to collect a frame. Then the town without the bushes and
/// with the lamp's shadow left out of the probes (what instance batches
/// and light shadows in the probes' views cost), and with a glass pane in
/// front of every building: transparent (the probes skip it), and the same panes
/// opaque (drawn in the probes' views): the difference is what drawing
/// see-through surfaces in the probes would at least cost.
#[test]
#[ignore = "benchmark: run in release"]
fn gi_cost() {
    gi_cost_in(None, true, false);
    // What the bushes cost: before instance batches were in the probes'
    // views, the town cost them about this much.
    gi_cost_in(None, false, false);
    // What the lamp's shadow costs the probes: their views as before they
    // read point and spot light shadows.
    gi_cost_in(None, true, true);
    gi_cost_in(Some(crate::render::Blend::Transparent), true, false);
    gi_cost_in(Some(crate::render::Blend::Opaque), true, false);
}

/// One scene of [`gi_cost`]: `glass` panes in front of the buildings, the
/// bushes or not, and `unshadowed`: the probes' views light with the lamp
/// as if nothing stood in its way.
fn gi_cost_in(glass: Option<crate::render::Blend>, with_bushes: bool, unshadowed: bool) {
    let Some(mut rig) = Rig::sized(true, (1920, 1080)) else {
        return;
    };
    let Some(mut timer) = super::timer::GpuTimer::new(&rig.device, &rig.queue) else {
        crate::render::render_test_support::skip("no GPU timestamps");
        return;
    };
    let white = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    let tinted = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::rgb(0.8, 0.5, 0.3)));
    let bush = rig.mesh(
        &MeshData::sphere(0.6, 12),
        MaterialDesc::color(Color::rgb(0.2, 0.5, 0.2)),
    );
    let pane = glass.map(|blend| {
        rig.mesh(
            &MeshData::cube(1.0),
            MaterialDesc {
                blend,
                ..MaterialDesc::color(Color::rgba(0.6, 0.8, 0.9, 0.4))
            },
        )
    });
    rig.block(white, vec3(-60.0, -1.0, -60.0), vec3(60.0, 0.0, 60.0));
    // 30 x 30 buildings, 4 apart, 3 to 12 high.
    let mut seed = 7u32;
    let mut random = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    let mut bushes = Vec::new();
    for i in 0..30 {
        for j in 0..30 {
            let at = vec3(i as f32 * 4.0 - 58.0, 0.0, j as f32 * 4.0 - 58.0);
            let mesh = if (i + j) % 3 == 0 { tinted } else { white };
            let height = 3.0 + random() * 9.0;
            rig.block(mesh, at, at + vec3(2.5, height, 2.5));
            if let Some(pane) = pane {
                rig.block(pane, at + vec3(0.0, 0.5, 2.6), at + vec3(2.5, 2.5, 2.65));
            }
            if with_bushes && i < 29 && j < 29 {
                for k in 0..2 {
                    let p = at + vec3(3.25 + k as f32 * 0.4 - 0.2, 0.5, 3.25 + random() - 0.5);
                    bushes.push(Mat4::from_translation(p));
                }
            }
        }
    }
    if with_bushes {
        let batch = rig
            .ctx
            .gfx
            .create_instance_batch(bush, &bushes, crate::render::InstanceBatchOptions::default());
        rig.renderer.set_instance_batch(
            &rig.device,
            &rig.queue,
            batch.id,
            rig.ctx.gfx.assets3d.instance_batches[batch.id as usize].as_ref(),
        );
    }
    rig.ctx.world.spawn((
        Transform3D::at(vec3(6.0, 4.0, 5.0)),
        crate::d3::PointLight {
            intensity: 30.0,
            range: 15.0,
            shadows: true,
            ..Default::default()
        },
    ));
    rig.ctx.scene3d.lighting.sun_direction = vec3(-0.4, -1.0, -0.3);
    rig.ctx.scene3d.camera.position = vec3(1.0, 1.7, 1.0);
    rig.ctx.scene3d.camera.look_at(vec3(20.0, 3.0, 12.0));
    rig.ctx.scene3d.camera.far = 200.0;
    rig.ctx.gfx.frame3d.gi_leave_out.light_shadows = unshadowed;
    // Rounds of every setting, each setting's best round kept: this runs
    // on machines doing other things too.
    let all = [
        ("off", None),
        (
            "defaults (4 probes a frame)",
            Some(GlobalIllumination::default().probes_per_frame),
        ),
        (
            "16 probes a frame",
            Some(crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME),
        ),
    ];
    let settings = if glass.is_some() || !with_bushes || unshadowed {
        &all[..2]
    } else {
        &all[..]
    };
    let mut best = vec![(f32::MAX, f64::MAX, f64::MAX, String::new()); settings.len()];
    for _ in 0..5 {
        for (k, &(_, gi)) in settings.iter().enumerate() {
            rig.ctx.scene3d.lighting.gi = GlobalIllumination {
                enabled: gi.is_some(),
                probes_per_frame: gi.unwrap_or(1),
                ..Default::default()
            };
            for _ in 0..20 {
                let _ = rig.frame(Some(&mut timer));
            }
            let (mut frames, mut total) = (0, 0.0);
            let mut parts = std::collections::BTreeMap::<&str, f32>::new();
            let mut read = timer.frames_read;
            let mut waits = Vec::with_capacity(60);
            for _ in 0..60 {
                waits.push(rig.frame(Some(&mut timer)));
                if timer.frames_read != read {
                    read = timer.frames_read;
                    frames += 1;
                    let spans = &timer.last_frame;
                    let end = spans.iter().map(|p| p.start_ms + p.ms).fold(0.0, f32::max);
                    total += end - spans.iter().map(|p| p.start_ms).fold(f32::MAX, f32::min);
                    for p in spans {
                        *parts.entry(p.name).or_default() += p.ms;
                    }
                }
            }
            let mut frame = crate::three::Frame3D::default();
            let cpu_start = std::time::Instant::now();
            for _ in 0..30 {
                frame.collect(
                    &mut rig.ctx.world,
                    &rig.ctx.gfx.assets3d,
                    &SkinFrame::default(),
                    &rig.ctx.scene3d.camera,
                    &rig.ctx.scene3d.lighting,
                );
            }
            let cpu = cpu_start.elapsed().as_secs_f64() * 1000.0 / 30.0;
            waits.sort_by(f64::total_cmp);
            let n = frames.max(1) as f32;
            let gpu = total / n;
            let b = &mut best[k];
            if gpu < b.0 {
                let shown: Vec<String> = parts
                    .iter()
                    .filter(|(name, _)| name.starts_with("gi") || **name == "opaque")
                    .map(|(name, ms)| format!("{name} {:.3}", ms / n))
                    .collect();
                b.0 = gpu;
                b.3 = shown.join(", ");
            }
            b.1 = b.1.min(waits[waits.len() / 2]);
            b.2 = b.2.min(cpu);
        }
    }
    let scene = match glass {
        None if !with_bushes => "town without the bushes",
        None if unshadowed => "town, the lamp's shadow left out of the probes",
        None => "town",
        Some(crate::render::Blend::Transparent) => "town, transparent panes",
        Some(_) => "town, the panes opaque",
    };
    for ((label, _), (gpu, wait, cpu, parts)) in settings.iter().zip(&best) {
        println!(
            "gi {label} ({scene}): GPU frame {gpu:.3} ms ({parts}); submit to done {wait:.3} ms; collect {cpu:.3} ms"
        );
    }
}

/// GPU cost of global illumination's passes at 1920x1080 over a field of
/// small instances culled on the GPU: 160,000 pebbles (12-segment spheres
/// 0.1 wide, 0.25 apart over 100 x 100) on a ground, 16 probes a frame.
/// Rounds with the probes' size cut and without it (as before it) take
/// turns, so both see the same load; each round is 40 frames after 10 to
/// settle. Run in release (`cargo test --release --lib --features 3d --
/// --ignored --nocapture gi_cost_small_instances`); prints each setting's
/// median round: the GI passes' GPU time, the GPU frame, and the pebbles
/// drawn in the probes' face views.
#[test]
#[ignore = "benchmark: run in release"]
fn gi_cost_small_instances() {
    let Some(mut rig) = Rig::sized(true, (1920, 1080)) else {
        return;
    };
    let Some(mut timer) = super::timer::GpuTimer::new(&rig.device, &rig.queue) else {
        crate::render::render_test_support::skip("no GPU timestamps");
        return;
    };
    let ground = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    rig.block(ground, vec3(-60.0, -1.0, -60.0), vec3(60.0, 0.0, 60.0));
    let pebble = rig.mesh(
        &MeshData::sphere(0.05, 12),
        MaterialDesc::color(Color::rgb(0.5, 0.45, 0.4)),
    );
    let n = 400;
    let pebbles: Vec<Mat4> = (0..n * n)
        .map(|k| Mat4::from_translation(vec3((k / n) as f32 * 0.25 - 50.0, 0.05, (k % n) as f32 * 0.25 - 50.0)))
        .collect();
    rig.renderer.batching.path = BatchPath::Gpu;
    let batch = rig
        .ctx
        .gfx
        .create_instance_batch(pebble, &pebbles, crate::render::InstanceBatchOptions::default());
    rig.sync_row(batch.id);
    if rig.renderer.batch_route(batch.id) != Some(Route::Gpu) {
        crate::render::render_test_support::skip("no GPU culling");
        return;
    }
    rig.ctx.scene3d.lighting.sun_direction = vec3(-0.4, -1.0, -0.3);
    rig.ctx.scene3d.lighting.gi = GlobalIllumination {
        enabled: true,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        ..Default::default()
    };
    rig.ctx.scene3d.camera.position = vec3(1.0, 1.7, 1.0);
    rig.ctx.scene3d.camera.look_at(vec3(20.0, 0.0, 12.0));
    rig.ctx.scene3d.camera.far = 200.0;
    // (GI passes, frame, pebbles in the probe views, culling) of each round.
    let mut rounds: [Vec<(f32, f32, u32, f32)>; 2] = [Vec::new(), Vec::new()];
    for _ in 0..6 {
        for (cut, round) in [false, true].into_iter().zip(&mut rounds) {
            rig.renderer.batching.no_probe_cut = !cut;
            for _ in 0..10 {
                let _ = rig.frame(Some(&mut timer));
            }
            let (mut frames, mut gi, mut total, mut cull) = (0, 0.0, 0.0, 0.0);
            let mut read = timer.frames_read;
            for _ in 0..40 {
                let _ = rig.frame(Some(&mut timer));
                if timer.frames_read != read {
                    read = timer.frames_read;
                    frames += 1;
                    let spans = &timer.last_frame;
                    let end = spans.iter().map(|p| p.start_ms + p.ms).fold(0.0, f32::max);
                    total += end - spans.iter().map(|p| p.start_ms).fold(f32::MAX, f32::min);
                    gi += spans
                        .iter()
                        .filter(|p| p.name.starts_with("gi"))
                        .map(|p| p.ms)
                        .sum::<f32>();
                    cull += spans
                        .iter()
                        .filter(|p| p.name == "static instance cull")
                        .map(|p| p.ms)
                        .sum::<f32>();
                }
            }
            let drawn = rig.renderer.gpu_probe_view_instances(&rig.device, &rig.queue, batch.id);
            let n = frames.max(1) as f32;
            round.push((gi / n, total / n, drawn, cull / n));
        }
    }
    for (label, round) in ["without the size cut", "with the size cut"].iter().zip(&mut rounds) {
        round.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (gi, frame, drawn, cull) = round[round.len() / 2];
        let all: Vec<String> = round.iter().map(|r| format!("{:.3}", r.0)).collect();
        println!(
            "gi small instances, {label}: GI passes {gi:.3} ms (rounds {}), instance culling \
             {cull:.3} ms, GPU frame {frame:.3} ms, {drawn} pebbles in the probes' face views",
            all.join(" ")
        );
    }
}

/// What leaving out cells far from every probe saves, in a large
/// scattered scene at 1920x1080: 250,000 rocks (12-segment spheres 0.3
/// wide) strewn over 1 km x 1 km in cells of 16, 16 probes a frame by the
/// camera. On each path (CPU and GPU culling), rounds with the far-cell
/// test and without it (as before it) take turns, so both see the same
/// load; each round is 30 frames after 10 to settle. Run in release
/// (`cargo test --release --lib --features 3d -- --ignored --nocapture
/// gi_cost_far_cells`); prints each setting's median round: the CPU's
/// `prepare` (where the CPU path builds the probes' lists), the GPU's
/// instance culling and GI passes, and the rocks in the probes' views.
#[test]
#[ignore = "benchmark: run in release"]
fn gi_cost_far_cells() {
    for path in [BatchPath::Cpu, BatchPath::Gpu] {
        let Some(mut rig) = Rig::sized(true, (1920, 1080)) else {
            return;
        };
        let Some(mut timer) = super::timer::GpuTimer::new(&rig.device, &rig.queue) else {
            crate::render::render_test_support::skip("no GPU timestamps");
            return;
        };
        let ground = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
        rig.block(ground, vec3(-500.0, -1.0, -500.0), vec3(500.0, 0.0, 500.0));
        let rock = rig.mesh(
            &MeshData::sphere(0.15, 12),
            MaterialDesc::color(Color::rgb(0.5, 0.45, 0.4)),
        );
        // A fixed pseudo-random scatter.
        let mut seed = 0x9e37_79b9_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        let rocks: Vec<Mat4> = (0..250_000)
            .map(|_| Mat4::from_translation(vec3(next() * 1000.0 - 500.0, 0.15, next() * 1000.0 - 500.0)))
            .collect();
        rig.renderer.batching.path = path;
        let options = crate::render::InstanceBatchOptions {
            cell_size: 16.0,
            ..Default::default()
        };
        let batch = rig.ctx.gfx.create_instance_batch(rock, &rocks, options);
        rig.sync_row(batch.id);
        let gpu = rig.renderer.batch_route(batch.id) == Some(Route::Gpu);
        if gpu != (path == BatchPath::Gpu) {
            crate::render::render_test_support::skip("the batch did not take the path asked for");
            continue;
        }
        rig.ctx.scene3d.lighting.sun_direction = vec3(-0.4, -1.0, -0.3);
        rig.ctx.scene3d.lighting.gi = GlobalIllumination {
            enabled: true,
            probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
            ..Default::default()
        };
        rig.ctx.scene3d.camera.position = vec3(1.0, 1.7, 1.0);
        rig.ctx.scene3d.camera.look_at(vec3(20.0, 0.0, 12.0));
        rig.ctx.scene3d.camera.far = 400.0;
        // (prepare, culling, GI passes, rocks in the probe views) of each round.
        let mut rounds: [Vec<(f64, f32, f32, u32)>; 2] = [Vec::new(), Vec::new()];
        for _ in 0..5 {
            for (skip, round) in [false, true].into_iter().zip(&mut rounds) {
                rig.renderer.batching.no_far_cells = !skip;
                for _ in 0..10 {
                    let _ = rig.frame(Some(&mut timer));
                }
                let (mut frames, mut prepare, mut gi, mut cull) = (0, 0.0, 0.0, 0.0);
                let mut read = timer.frames_read;
                for _ in 0..30 {
                    let _ = rig.frame(Some(&mut timer));
                    prepare += rig.prepare_ms;
                    if timer.frames_read != read {
                        read = timer.frames_read;
                        frames += 1;
                        let spans = &timer.last_frame;
                        let sum = |f: &dyn Fn(&str) -> bool| -> f32 {
                            spans.iter().filter(|p| f(p.name)).map(|p| p.ms).sum()
                        };
                        gi += sum(&|n| n.starts_with("gi"));
                        cull += sum(&|n| n == "static instance cull");
                    }
                }
                let drawn = if gpu {
                    rig.renderer.gpu_probe_view_instances(&rig.device, &rig.queue, batch.id)
                } else {
                    rig.renderer.probe_view_instances(batch.id)
                };
                let n = frames.max(1) as f32;
                round.push((prepare / 30.0, cull / n, gi / n, drawn));
            }
        }
        let name = if gpu { "GPU" } else { "CPU" };
        for (label, round) in ["testing every cell", "leaving out far cells"].iter().zip(&mut rounds) {
            round.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (prepare, cull, gi, drawn) = round[round.len() / 2];
            let all: Vec<String> = round.iter().map(|r| format!("{:.3}/{:.3}", r.0, r.1)).collect();
            println!(
                "gi far cells, {name} path, {label}: prepare {prepare:.3} ms, instance culling {cull:.3} ms, \
                 GI passes {gi:.3} ms, {drawn} rocks in the probes' views (rounds prepare/cull: {})",
                all.join(" ")
            );
        }
    }
}

/// What the probes cannot tell apart, measured (the quality baseline of
/// `crates/ponko_bench/RAY_TRACING.md`). In the red wall room (probes every
/// 1 unit, at whole x), a 2 cm white partition from floor to roof splits
/// the room at `x`; the floor 20 cm east of it cannot see the red wall, so
/// its true red tint is none. Returns its red tint (red minus green), or,
/// without `x`, the tint there in the open room.
fn tint_east_of_a_partition(x: Option<f32>) -> Option<i32> {
    let mut rig = Rig::red_wall_room(true, true)?;
    let mut floor = -0.8;
    if let Some(x) = x {
        let white = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
        rig.block(white, vec3(x - 0.01, -0.5, -3.5), vec3(x + 0.01, 3.5, 3.5));
        floor = x + 0.2;
    }
    let pixels = rig.draw(80);
    let c = rig.at(&pixels, vec3(floor, -0.5, -0.8));
    Some(c[0] - c[1])
}

/// A wall much thinner than the probe spacing, lying on a plane of probes,
/// buries them: they see light on both sides of it. Their neighbours'
/// distances give them away, and the camera's view leaves them out, so the
/// red stays on its side. The baseline in §2 of
/// `crates/ponko_bench/RAY_TRACING.md`.
#[test]
fn bounced_light_stops_at_a_thin_partition_on_a_probe_plane() {
    let Some(open) = tint_east_of_a_partition(None) else {
        return;
    };
    let Some(between) = tint_east_of_a_partition(Some(-1.5)) else {
        return;
    };
    let Some(on_plane) = tint_east_of_a_partition(Some(-1.0)) else {
        return;
    };
    eprintln!(
        "red tint on the floor past a 2 cm partition: {open} with none, {between} with it between probe planes, \
         {on_plane} with it on one (truth: 0)"
    );
    assert!(open > 40, "positive control: the open room's floor is tinted: {open}");
    assert!(between <= 5, "between the probes the partition holds: {between}");
    assert!(on_plane <= 5, "on a probe plane the partition holds too: {on_plane}");
}

/// A closed room with white walls (the inside from -2 to 2 across, -0.5 to
/// 2.5 high, walls 0.4 thick), lit only by a shadowed lamp inside it, no
/// sun, sky or ambient light; probes every unit (7 x 5 x 7 = 245, so a
/// full round of updates is 16 frames at 16 a frame). Each bounce keeps
/// 94% of the light, so the probes' own light takes over a thousand frames
/// to settle. Draws `checkpoints` frames in turn and returns, after each,
/// how bright the roof is (the linear sum of red, green and blue over a
/// few points; no tonemapping). `unsettled`: the camera's view reads the
/// probes' latest light instead of the settled estimate.
fn closed_lamp_room(checkpoints: &[usize], unsettled: bool) -> Option<Vec<f32>> {
    let mut rig = Rig::new(true)?;
    rig.renderer.gi_unsettled = unsettled;
    let white = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc::color(Color::WHITE).with_roughness(1.0),
    );
    let (inner, outer) = (2.0, 2.4);
    let walls = [
        (white, vec3(-outer, -0.9, -outer), vec3(outer, -0.5, outer)), // floor
        (white, vec3(-outer, 2.5, -outer), vec3(outer, 2.9, outer)),   // roof
        (white, vec3(-outer, -0.5, -outer), vec3(-inner, 2.5, outer)), // west
        (white, vec3(inner, -0.5, -outer), vec3(outer, 2.5, outer)),   // east
        (white, vec3(-inner, -0.5, -outer), vec3(inner, 2.5, -inner)), // north
        (white, vec3(-inner, -0.5, inner), vec3(inner, 2.5, outer)),   // south
    ];
    rig.blocks(&walls, Build::Models);
    rig.ctx.world.spawn((
        Transform3D::at(vec3(1.2, 0.0, 1.2)),
        crate::d3::PointLight {
            intensity: 0.6,
            range: 10.0,
            shadows: true,
            ..Default::default()
        },
    ));
    dark_world(&mut rig);
    rig.ctx.scene3d.lighting.gi = GlobalIllumination {
        enabled: true,
        extent: vec3(6.0, 4.0, 6.0),
        spacing: 1.0,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        intensity: 1.0,
    };
    rig.ctx.scene3d.camera.position = vec3(-1.5, 0.0, -1.5);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 2.5, 0.0));
    let points = [vec3(0.0, 2.5, 0.0), vec3(-0.5, 2.5, 0.5), vec3(0.5, 2.5, -0.5)];
    let mut drawn = 0;
    let mut out = Vec::new();
    for &at in checkpoints {
        let pixels = rig.draw(at - drawn);
        drawn = at;
        let linear = |c: i32| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let sum: f32 = points
            .iter()
            .map(|&p| rig.at(&pixels, p).iter().map(|&c| linear(c)).sum::<f32>())
            .sum();
        out.push(sum);
    }
    Some(out)
}

/// No sun, sky or ambient light, no tonemapping or bloom: only the lamps
/// and what bounces off the walls.
fn dark_world(rig: &mut Rig) {
    let lighting = &mut rig.ctx.scene3d.lighting;
    lighting.sun_intensity = 0.0;
    lighting.ambient_intensity = 0.0;
    rig.ctx.scene3d.sky.sky_color = Color::BLACK;
    rig.ctx.scene3d.sky.ground_color = Color::BLACK;
    rig.ctx.scene3d.post.tonemap = crate::render::Tonemap::None;
    rig.ctx.scene3d.post.bloom = 0.0;
}

/// The bound: from 400 frames (25 rounds of probe updates) after the light
/// comes on, the roof of a bright closed room stays within 6% of where it
/// settles, and on its way it never overshoots by more than 2%. The
/// probes' latest light alone is still more than 10% short then.
#[test]
fn probe_light_in_a_bright_closed_room_settles_within_400_frames() {
    let checkpoints: Vec<usize> = (1..=16).map(|k| k * 100).collect();
    let Some(settled) = closed_lamp_room(&checkpoints, false) else {
        return;
    };
    let unsettled = closed_lamp_room(&checkpoints, true).unwrap();
    let last = *unsettled.last().unwrap();
    let print = |b: &[f32]| {
        b.iter()
            .map(|v| format!("{:.3}", v / last))
            .collect::<Vec<_>>()
            .join(" ")
    };
    eprintln!("roof against where it settles, every 100 frames: {}", print(&settled));
    eprintln!("without the settled estimate: {}", print(&unsettled));
    let at_400 = settled[3] / last;
    assert!(
        (0.94..=1.02).contains(&at_400),
        "settled within 400 frames: {at_400:.3}"
    );
    for (k, v) in settled.iter().enumerate().skip(3) {
        assert!(
            (v / last - 1.0).abs() <= 0.06,
            "stays settled at frame {}: {:.3}",
            (k + 1) * 100,
            v / last
        );
    }
    assert!(
        settled.iter().all(|v| v / last <= 1.02),
        "never overshoots: {}",
        print(&settled)
    );
    let at_1600 = settled[15] / last;
    assert!((at_1600 - 1.0).abs() <= 0.02, "both reach the same light: {at_1600:.3}");
    assert!(
        unsettled[3] / last < 0.9,
        "negative control: the latest light alone is short: {}",
        print(&unsettled)
    );
}

/// Two closed-off rooms side by side under a black sky, no roof, seen from
/// straight above: the west one lit by a shadowed lamp, the east one dark,
/// a 10 cm white wall between them at `x` from the floor to the top of the
/// outer walls. Probes every 2 units, so at 0 the wall runs through a
/// plane of probes. Returns the rig and the image after `frames`.
fn lit_and_dark_rooms(x: f32, wall: bool, frames: usize) -> Option<(Rig, Vec<u8>)> {
    let whole = [(vec2(-0.5, -3.0), vec2(4.5, 3.0))];
    split_rooms(x, -0.5, if wall { &whole } else { &[] }, frames).map(|(rig, pixels, _)| (rig, pixels))
}

/// [`lit_and_dark_rooms`] with the floor's top at `floor` (the outer walls
/// still reach 4.5) and the wall between the rooms made of `pieces`, each
/// from (y, z) `lo` to `hi`: one that stops short of the floor, or leaves
/// a doorway. Also returns the white boxes, from `lo` to `hi`, for the
/// path tracer.
#[allow(clippy::type_complexity)]
fn split_rooms(
    x: f32,
    floor: f32,
    pieces: &[(Vec2, Vec2)],
    frames: usize,
) -> Option<(Rig, Vec<u8>, Vec<(Vec3, Vec3)>)> {
    let mut rig = Rig::new(true)?;
    let white = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc::color(Color::WHITE).with_roughness(1.0),
    );
    let (lo, hi, t) = (-6.0, 6.0, 0.4);
    let mut boxes = vec![
        (vec3(lo - t, floor - 0.4, -3.0 - t), vec3(hi + t, floor, 3.0 + t)), // floor
        (vec3(lo - t, floor, -3.0 - t), vec3(lo, 4.5, 3.0 + t)),             // west
        (vec3(hi, floor, -3.0 - t), vec3(hi + t, 4.5, 3.0 + t)),             // east
        (vec3(lo, floor, -3.0 - t), vec3(hi, 4.5, -3.0)),                    // north
        (vec3(lo, floor, 3.0), vec3(hi, 4.5, 3.0 + t)),                      // south
    ];
    for &(a, b) in pieces {
        boxes.push((vec3(x - 0.05, a.x, a.y), vec3(x + 0.05, b.x, b.y)));
    }
    let walls: Vec<_> = boxes.iter().map(|&(a, b)| (white, a, b)).collect();
    rig.blocks(&walls, Build::Models);
    rig.ctx.world.spawn((
        Transform3D::at(vec3(x - 3.0, 1.0, 0.0)),
        crate::d3::PointLight {
            intensity: 3.0,
            range: 12.0,
            shadows: true,
            ..Default::default()
        },
    ));
    dark_world(&mut rig);
    rig.ctx.scene3d.lighting.gi = GlobalIllumination {
        enabled: true,
        extent: vec3(16.0, 14.0, 10.0),
        spacing: 2.0,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        intensity: 1.0,
    };
    // Above the wall (inside the probe grid, which moves with the camera),
    // so it hides neither floor.
    rig.ctx.scene3d.camera.position = vec3(x, 6.5, 0.01);
    rig.ctx.scene3d.camera.look_at(vec3(x, -0.5, 0.0));
    rig.ctx.scene3d.camera.fov_y = 100f32.to_radians();
    let pixels = rig.draw(frames);
    Some((rig, pixels, boxes))
}

/// The white `boxes` and the lamps of `rig`'s world as its camera sees
/// them, path traced (`samples` rays a pixel) with light bounced until it
/// has all but run out (256 times): what realtime global illumination
/// settles on. Linear color, row by row from the top.
fn path_traced(rig: &mut Rig, boxes: &[(Vec3, Vec3)], samples: u32) -> Vec<Vec3> {
    use crate::render::{BakeModel, LightBakeScene};
    let mut scene = LightBakeScene::new(&rig.ctx.scene3d.lighting, &rig.ctx.scene3d.sky);
    let cube = MeshData::cube(1.0);
    let white = MaterialDesc::color(Color::WHITE);
    for &(lo, hi) in boxes {
        let place = Mat4::from_scale_rotation_translation(hi - lo, Quat::IDENTITY, (lo + hi) * 0.5);
        scene.add(BakeModel::new(&cube, place).with_material(&white));
    }
    scene.add_lights_from(&mut rig.ctx.world);
    let camera = &rig.ctx.scene3d.camera;
    crate::three::bake::reference_image(&scene, camera.position, camera.view_proj(), (SIZE, SIZE), samples, 256)
}

/// sRGB (0..255) to linear.
fn linear_of(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear to sRGB (0..255, clamped).
fn srgb_of(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    255.0
        * if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
}

/// How bright the floor is at `p` in the drawn `pixels` and in the path
/// traced `truth`: the mean linear color of the 3 x 3 pixels around it
/// (the path tracer's noise mostly averages out), as the sum of its red,
/// green and blue in 8-bit sRGB, as [`floor_east_of`] measures.
fn drawn_and_traced(rig: &Rig, pixels: &[u8], truth: &[Vec3], p: Vec3) -> (f32, f32) {
    let c = rig.ctx.scene3d.camera.view_proj() * p.extend(1.0);
    let ndc = c.truncate() / c.w;
    let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as i32;
    let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as i32;
    let (mut a, mut b) = (Vec3::ZERO, Vec3::ZERO);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (px, py) = ((x + dx).clamp(0, SIZE as i32 - 1), (y + dy).clamp(0, SIZE as i32 - 1));
            let i = (py * SIZE as i32 + px) as usize;
            a += Vec3::new(
                linear_of(pixels[i * 4]),
                linear_of(pixels[i * 4 + 1]),
                linear_of(pixels[i * 4 + 2]),
            );
            b += truth[i];
        }
    }
    let sum = |v: Vec3| srgb_of(v.x / 9.0) + srgb_of(v.y / 9.0) + srgb_of(v.z / 9.0);
    (sum(a), sum(b))
}

/// The probes on the plane x = `x` whose spot lies within `y` and `z`
/// (lo, hi), and whether the update found each buried in a wall: from the
/// probe slots' codes, read back.
fn probes_on_plane(rig: &Rig, x: f32, y: (f32, f32), z: (f32, f32)) -> Vec<(Vec3, bool)> {
    use crate::three::gi::GiGrid;
    let gi = &rig.ctx.scene3d.lighting.gi;
    let counts = glam::UVec3::from_array(gi.probe_counts());
    let spacing = gi.spacing;
    let center = (rig.ctx.scene3d.camera.position / spacing).round().as_ivec3();
    let grid = GiGrid {
        counts,
        spacing,
        origin: center - (counts.as_ivec3() - 1) / 2,
    };
    let codes = rig
        .renderer
        .gi_probe_codes(&rig.device, &rig.queue)
        .expect("global illumination is on");
    let mut found = Vec::new();
    for k in 0..grid.total() {
        let cell = grid.cell(k);
        let p = grid.position(cell);
        let inside = |v: f32, (lo, hi): (f32, f32)| v > lo && v < hi;
        if (p.x - x).abs() < 1e-3 && inside(p.y, y) && inside(p.z, z) {
            let code = codes[grid.slot(cell)];
            assert!(
                (code.abs() - grid.code(cell)).abs() < 0.25,
                "probe {p} has looked around: slot code {code}"
            );
            found.push((p, code < 0.0));
        }
    }
    found.sort_by(|a, b| a.0.to_array().partial_cmp(&b.0.to_array()).unwrap());
    found
}

/// How bright the floor is (the sum of red, green and blue) at `points`
/// east of `x`: the brightest of each across the room.
fn floor_east_of(rig: &Rig, pixels: &[u8], x: f32, points: &[f32]) -> Vec<i32> {
    points
        .iter()
        .map(|&d| {
            [-1.5, 0.0, 1.5]
                .map(|z| rig.at(pixels, vec3(x + d, -0.5, z)).iter().sum::<i32>())
                .into_iter()
                .max()
                .unwrap()
        })
        .collect()
}

/// A 10 cm wall on a plane of probes 2 units apart keeps the bounced light
/// in the lit room: the dark room's floor stays dark right up to the wall,
/// under a fiftieth as bright as the lit room's floor a unit from the wall
/// (the sums of the image's 8-bit channels). The probes in the wall half a
/// unit below its top are found buried too: before, rays over the top in
/// their neighbours' views hid them, and they lit the dark room with the
/// lit one's light (the floor at about a twentieth).
#[test]
fn a_thin_wall_on_a_plane_of_coarse_probes_keeps_the_dark_room_dark() {
    use crate::render::render_test_support::{Image, keep_image};
    let points = [0.3, 0.6, 1.0, 2.0, 3.0];
    let Some((rig, pixels)) = lit_and_dark_rooms(0.0, true, 200) else {
        return;
    };
    let dark = floor_east_of(&rig, &pixels, 0.0, &points);
    let lit = floor_east_of(&rig, &pixels, -2.0, &[1.0])[0];
    let image = Image {
        width: rig.size.0,
        height: rig.size.1,
        rgba: pixels,
    };
    keep_image(&image);
    let (open_rig, open_pixels) = lit_and_dark_rooms(0.0, false, 200).unwrap();
    let open = floor_east_of(&open_rig, &open_pixels, 0.0, &points);
    keep_image(&Image {
        width: open_rig.size.0,
        height: open_rig.size.1,
        rgba: open_pixels,
    });
    eprintln!(
        "dark room's floor 0.3 to 3 units from the wall: {dark:?}, lit room's floor {lit}, without the wall {open:?}"
    );
    assert!(
        open[0] > lit / 4,
        "positive control: without the wall the light reaches there: {open:?} vs {lit}"
    );
    assert!(
        dark.iter().all(|&d| d * 50 <= lit),
        "the dark room stays dark: {dark:?} vs {lit}"
    );
    crate::render::render_test_support::check_reference(
        "gi_thin_wall",
        &image,
        crate::render::regression_stage::EFFECT_REFERENCE,
        "Two white rooms 12 x 6 units without a roof under a black sky, no sun or ambient light, split by a 10 cm \
         white wall at x = 0 that lies on a plane of probes 2 units apart; a shadowed lamp at (-3, 1, 0) lights \
         the west room only; 200 frames of probe updates at 16 a frame, camera at (0, 6.5, 0.01) looking straight \
         down, 100 degree field of view; 128x128, 4x multisampling, no tonemapping.",
        &rig.info,
    );
}

/// A probe looked at from its neighbours along x: in a wall with one edge
/// close to it, part of their cones passes the edge, so it hangs on how
/// much of them stops at the wall. How much the dark room's floor at
/// `points` east of the wall (8-bit sums, as [`floor_east_of`]) is over
/// the path traced one, at worst, as a share of the lit room's floor a
/// unit west of the wall; and the drawn floor at those points.
fn over_the_truth(rig: &mut Rig, pixels: &[u8], boxes: &[(Vec3, Vec3)], floor: f32) -> (f32, Vec<f32>) {
    let truth = path_traced(rig, boxes, 64);
    let (lit, _) = drawn_and_traced(rig, pixels, &truth, vec3(-1.0, floor, 0.0));
    let mut worst = f32::MIN;
    let mut drawn = Vec::new();
    for d in [0.3, 0.6, 1.0, 2.0, 3.0] {
        for z in [-2.5, -1.5, 0.0, 1.5, 2.5] {
            let (a, b) = drawn_and_traced(rig, pixels, &truth, vec3(d, floor, z));
            worst = worst.max((a - b) / lit);
            drawn.push(a);
        }
    }
    (worst, drawn)
}

/// A 10 cm wall that stops short of the floor (the gap under it lets the
/// lamp's light through), or hangs from above, its bottom edge 0.1 below
/// a row of probes in it: every probe in the wall is found buried, so the
/// dark room's floor is nowhere brighter than the path tracer's (by more
/// than 2% of the lit floor; it is darker: what comes through a gap
/// narrower than the spacing the probes hardly carry). Near a wall's top
/// the neighbours' cones that tell a probe is buried lean over the edge;
/// near its bottom they lean away. With no wall, no probe on that plane
/// is taken for buried.
#[test]
fn a_raised_partition_buries_the_probes_just_inside_its_bottom_edge() {
    let Some((open, _, _)) = split_rooms(0.0, -0.5, &[], 200) else {
        return;
    };
    let unwalled = probes_on_plane(&open, 0.0, (-0.5, 4.5), (-3.0, 3.0));
    assert!(
        unwalled.len() == 9 && unwalled.iter().all(|p| !p.1),
        "negative control: in open air no probe is buried: {unwalled:?}"
    );
    // Probes are at y 0, 2 and 4: the floor at -0.9 and the wall from
    // -0.1 up, or the wall from 1.9 up over a floor at -0.5.
    for (floor, edge) in [(-0.9, -0.1), (-0.5, 1.9)] {
        let pieces = [(vec2(edge, -3.0), vec2(4.5, 3.0))];
        let (mut rig, pixels, boxes) = split_rooms(0.0, floor, &pieces, 200).unwrap();
        let inside = probes_on_plane(&rig, 0.0, (edge, 4.5), (-3.0, 3.0));
        let (over, drawn) = over_the_truth(&mut rig, &pixels, &boxes, floor);
        eprintln!(
            "wall from {edge} up: probes in it (buried?) {inside:?}; dark floor {drawn:?}, at worst {:.1}% of the lit \
             floor over the path tracer's",
            over * 100.0
        );
        assert!(
            !inside.is_empty() && inside.iter().all(|p| p.1),
            "every probe in the wall is buried: {inside:?}"
        );
        assert!(over <= 0.02, "no light through the wall: {:.1}%", over * 100.0);
    }
}

/// Probes in open air 0.1 under a wall's bottom edge see the gap they are
/// in and stay lit: they are not taken for buried, and the floor beside
/// the gap is as bright as the path tracer's (within a fifth; 8-bit sums).
/// Before, the cone each neighbour's distance tile kept for that way
/// leaned up into the wall, and they were: the floor took about half the
/// light.
#[test]
fn probes_in_open_air_just_under_a_walls_bottom_edge_are_not_taken_for_buried() {
    let pieces = [(vec2(0.1, -3.0), vec2(4.5, 3.0))];
    let Some((mut rig, pixels, boxes)) = split_rooms(0.0, -0.9, &pieces, 200) else {
        return;
    };
    let under = probes_on_plane(&rig, 0.0, (-0.9, 0.1), (-3.0, 3.0));
    let truth = path_traced(&mut rig, &boxes, 64);
    let near: Vec<_> = [0.3, 0.6]
        .iter()
        .flat_map(|&d| [-1.5, 0.0, 1.5].map(|z| drawn_and_traced(&rig, &pixels, &truth, vec3(d, -0.9, z))))
        .collect();
    eprintln!("probes under the wall (buried?) {under:?}; floor beside the gap, drawn and traced: {near:?}");
    assert!(
        !under.is_empty() && under.iter().all(|p| !p.1),
        "probes in the gap stay lit: {under:?}"
    );
    assert!(
        near.iter().all(|&(a, b)| a >= b * 0.8),
        "the floor beside the gap is lit: {near:?}"
    );
}

/// At a doorway, a probe in the wall 0.1 from the door's frame isn't found
/// (half of its neighbours' cones pass the frame), but it lights the next
/// room no more than a probe 0.1 outside the frame, in the doorway, does
/// (by 2% of the lit floor at most, at any point): what reaches the dark
/// room past the door is the doorway's own light, which probes 2 units
/// apart spread a unit or more around the door.
#[test]
fn a_probe_in_a_doorways_frame_lights_the_next_room_no_more_than_one_in_the_doorway() {
    let frame = |end: f32| [(vec2(-0.5, -3.0), vec2(4.5, end))];
    let Some((inside, inside_pixels, _)) = split_rooms(0.0, -0.5, &frame(2.1), 200) else {
        return;
    };
    let (outside, outside_pixels, _) = split_rooms(0.0, -0.5, &frame(1.9), 200).unwrap();
    let points = [0.3, 0.6, 1.0, 2.0, 3.0];
    let lit = floor_east_of(&inside, &inside_pixels, -2.0, &[1.0])[0];
    let floor = |rig: &Rig, pixels: &[u8]| {
        [-2.5, -1.5, 0.0, 1.5, 2.5]
            .iter()
            .flat_map(|&z| points.map(|d| rig.at(pixels, vec3(d, -0.5, z)).iter().sum::<i32>()))
            .collect::<Vec<_>>()
    };
    let (a, b) = (floor(&inside, &inside_pixels), floor(&outside, &outside_pixels));
    eprintln!("dark room's floor with the probe in the frame {a:?}, in the doorway {b:?}, lit floor {lit}");
    assert!(
        a.iter().zip(&b).all(|(&a, &b)| (a - b) * 50 <= lit),
        "no more light than through the door: {a:?} vs {b:?}"
    );
}

/// A closed room 10 x 4 x 10 inside with light gray walls (each bounce
/// keeps about half the light) and a shadowed lamp, under a black sky, in
/// a default-size probe grid (17 x 9 x 17 at 2 units, 4 probes a frame: a
/// full round of updates is 650 frames, and only about a twentieth of the
/// probes are in the room). The lamp comes on after every probe has
/// looked around once in the dark. Returns how bright the roof is after
/// each `step` frames, `steps` times (the linear sum of red, green and
/// blue over a few points). `unboosted`: the probes take turns by age
/// and distance alone, as before changing ones went first.
fn lamp_on_in_a_default_grid(step: usize, steps: usize, unboosted: bool) -> Option<Vec<f32>> {
    crate::three::gi::GI_UNBOOSTED.with(|u| u.set(unboosted));
    let mut rig = Rig::new(true)?;
    let gray = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc::color(Color::rgb(0.7, 0.7, 0.7)).with_roughness(1.0),
    );
    let (inner, outer) = (5.0, 5.4);
    let walls = [
        (gray, vec3(-outer, -0.9, -outer), vec3(outer, -0.5, outer)), // floor
        (gray, vec3(-outer, 3.5, -outer), vec3(outer, 3.9, outer)),   // roof
        (gray, vec3(-outer, -0.5, -outer), vec3(-inner, 3.5, outer)), // west
        (gray, vec3(inner, -0.5, -outer), vec3(outer, 3.5, outer)),   // east
        (gray, vec3(-inner, -0.5, -outer), vec3(inner, 3.5, -inner)), // north
        (gray, vec3(-inner, -0.5, inner), vec3(inner, 3.5, outer)),   // south
    ];
    rig.blocks(&walls, Build::Models);
    let lamp = rig.ctx.world.spawn((
        Transform3D::at(vec3(2.5, 0.5, 2.5)),
        crate::d3::PointLight {
            intensity: 0.0,
            range: 14.0,
            shadows: true,
            ..Default::default()
        },
    ));
    dark_world(&mut rig);
    rig.ctx.scene3d.lighting.gi = GlobalIllumination {
        enabled: true,
        ..Default::default()
    };
    rig.ctx.scene3d.camera.position = vec3(-3.5, 0.5, -3.5);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 3.5, 0.0));
    rig.draw(700);
    rig.ctx.world.get_mut::<crate::d3::PointLight>(lamp).unwrap().intensity = 2.0;
    let points = [vec3(0.0, 3.5, 0.0), vec3(-1.0, 3.5, 1.0), vec3(1.0, 3.5, -1.0)];
    let linear = |c: i32| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let out = (0..steps)
        .map(|_| {
            let pixels = rig.draw(step);
            points
                .iter()
                .map(|&p| rig.at(&pixels, p).iter().map(|&c| linear(c)).sum::<f32>())
                .sum()
        })
        .collect();
    crate::three::gi::GI_UNBOOSTED.with(|u| u.set(false));
    Some(out)
}

/// The first of `curve`'s steps from which it stays within `within` of
/// `settled`.
fn settles_at(curve: &[f32], settled: f32, within: f32) -> usize {
    curve
        .iter()
        .rposition(|v| (v / settled - 1.0).abs() > within)
        .map_or(0, |k| k + 1)
}

/// The bound: with a default-size grid at its default 4 probes a frame,
/// after a lamp comes on in a closed room the roof stays within 5% of
/// where it settles from 600 frames on, because the probes whose light is
/// still changing (those in the room) take more of the turns. Taking
/// turns by age and distance alone (the negative control) misses that
/// bound and takes at least twice as long: 1200 frames on an Apple GPU,
/// 800 against 400 on a Radeon RX Vega (Vulkan and Direct3D 12 alike).
/// The roof's brightness comes in 8-bit steps, and there the control's
/// curve reached 0.950 of the settled value at 900 frames, exactly on the
/// 5% line, so "still off at 900" held on one GPU and not the other.
#[test]
fn probes_whose_light_changes_take_more_turns_so_a_lamp_settles_sooner() {
    let Some(boosted) = lamp_on_in_a_default_grid(100, 15, false) else {
        return;
    };
    let unboosted = lamp_on_in_a_default_grid(100, 15, true).unwrap();
    let settled = *boosted.last().unwrap();
    let print = |c: &[f32]| {
        c.iter()
            .map(|v| format!("{:.3}", v / settled))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let (fast, slow) = (
        settles_at(&boosted, settled, 0.05) * 100,
        settles_at(&unboosted, settled, 0.05) * 100,
    );
    eprintln!(
        "roof every 100 frames after the lamp comes on, changing probes first: {}",
        print(&boosted)
    );
    eprintln!("by age and distance alone: {}", print(&unboosted));
    eprintln!("within 5% from {fast} frames, against {slow}");
    assert!(boosted[0] > 0.0, "the lamp lights the roof: {}", print(&boosted));
    assert!(fast <= 600, "settles within 600 frames: {}", print(&boosted));
    assert!(
        slow > 600 && slow >= 2 * fast,
        "negative control: by age alone it is still off at 600 frames and takes twice as long ({slow} against {fast}): {}",
        print(&unboosted)
    );
}

/// The frame `rig` drew last, as an image (kept with
/// `PONKO_RENDER_TEST_IMAGES=1`).
fn kept(rig: &Rig, pixels: Vec<u8>) -> crate::render::render_test_support::Image {
    let image = crate::render::render_test_support::Image {
        width: rig.size.0,
        height: rig.size.1,
        rgba: pixels,
    };
    crate::render::render_test_support::keep_image(&image);
    image
}

/// Two closed-off rooms side by side under a black sky, no roof, seen from
/// straight above: a 40 cm wall between them from x 0.8 to 1.2 (between
/// two planes of probes 2 units apart), and in the west room a shadowed
/// spot light 3.5 up, aimed east and down at that wall, its cone wide
/// enough to reach the east room's floor past it. The camera's own view
/// has the light's shadow, so no direct light gets into the east room;
/// `unshadowed`: the probes' views light their surfaces as if the wall
/// weren't there (as they did before they read that shadow). After
/// `frames` frames.
fn spot_rooms(unshadowed: bool, frames: usize) -> Option<(Rig, Vec<u8>)> {
    let mut rig = Rig::new(true)?;
    let white = rig.mesh(
        &MeshData::cube(1.0),
        MaterialDesc::color(Color::WHITE).with_roughness(1.0),
    );
    let (lo, hi, t, floor) = (-6.0, 6.0, 0.4, -0.5);
    let walls = [
        (white, vec3(lo - t, floor - 0.4, -3.0 - t), vec3(hi + t, floor, 3.0 + t)), // floor
        (white, vec3(lo - t, floor, -3.0 - t), vec3(lo, 4.5, 3.0 + t)),             // west
        (white, vec3(hi, floor, -3.0 - t), vec3(hi + t, 4.5, 3.0 + t)),             // east
        (white, vec3(lo, floor, -3.0 - t), vec3(hi, 4.5, -3.0)),                    // north
        (white, vec3(lo, floor, 3.0), vec3(hi, 4.5, 3.0 + t)),                      // south
        (white, vec3(0.8, floor, -3.0), vec3(1.2, 4.5, 3.0)),                       // between
    ];
    rig.blocks(&walls, Build::Models);
    let at = vec3(-2.0, 3.5, 0.0);
    rig.ctx.world.spawn((
        Transform3D::at(at).looking_at(at + vec3(1.0, -0.6, 0.0)),
        crate::d3::SpotLight {
            intensity: 6.0,
            range: 14.0,
            inner_angle: 0.35,
            outer_angle: 0.6,
            shadows: true,
            ..Default::default()
        },
    ));
    dark_world(&mut rig);
    rig.ctx.scene3d.lighting.gi = GlobalIllumination {
        enabled: true,
        extent: vec3(16.0, 14.0, 10.0),
        spacing: 2.0,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        intensity: 1.0,
    };
    rig.ctx.gfx.frame3d.gi_leave_out.light_shadows = unshadowed;
    rig.ctx.scene3d.camera.position = vec3(1.0, 6.5, 0.01);
    rig.ctx.scene3d.camera.look_at(vec3(1.0, -0.5, 0.0));
    rig.ctx.scene3d.camera.fov_y = 100f32.to_radians();
    let pixels = rig.draw(frames);
    Some((rig, pixels))
}

/// An interior lit by a shadowed spot light: the probes see the light's
/// shadow, so the room on the far side of the wall it shines at stays
/// dark. Negative control: probes that light their views as if the wall
/// weren't there (as before they read point and spot light shadows) put
/// the light that never gets past the wall on the dark room's floor.
#[test]
fn a_shadowed_spot_light_bounces_no_light_past_the_wall_it_shines_at() {
    let points = [1.0, 2.0, 3.0, 4.0];
    let Some((rig, pixels)) = spot_rooms(false, 200) else {
        return;
    };
    let dark = floor_east_of(&rig, &pixels, 1.2, &points);
    let lit = floor_east_of(&rig, &pixels, -4.0, &[0.0, 2.0, 3.0]);
    let after = kept(&rig, pixels);
    let (old_rig, old_pixels) = spot_rooms(true, 200).unwrap();
    let leaked = floor_east_of(&old_rig, &old_pixels, 1.2, &points);
    let before = kept(&old_rig, old_pixels);
    eprintln!(
        "east room's floor 1 to 4 units past the wall: {dark:?} with the spot light's shadow in the probes, \
         {leaked:?} without; west room's floor {lit:?}"
    );
    let lit = *lit.iter().max().unwrap();
    assert!(lit > 60, "the spot light's bounce lights the west room: {lit}");
    assert!(
        dark.iter().all(|&d| d * 20 <= lit),
        "the east room stays dark: {dark:?} vs {lit}"
    );
    assert!(
        leaked.iter().max().unwrap() * 2 > lit,
        "negative control: probes without the shadow light the east room: {leaked:?} vs {lit}"
    );
    use crate::render::render_test_support::{check_reference, reference_rejects};
    let tolerance = crate::render::regression_stage::EFFECT_REFERENCE;
    check_reference(
        "gi_spot_interior",
        &after,
        tolerance,
        "Two white rooms 12 x 6 units without a roof under a black sky, no sun or ambient light, split by a 40 cm \
         white wall from x = 0.8 to 1.2 between planes of probes 2 units apart; a shadowed spot light at \
         (-2, 3.5, 0) aimed along (1, -0.6, 0), cone 0.35 to 0.6 radians, lights the west room's side of the \
         wall; 200 frames of probe updates at 16 a frame, camera at (1, 6.5, 0.01) looking straight down, 100 \
         degree field of view; 128x128, 4x multisampling, no tonemapping.",
        &rig.info,
    );
    reference_rejects(
        "gi_spot_interior",
        &before,
        tolerance,
        "the probes lit without the spot light's shadow",
    );
}

/// A room built of instance batches, lit through a roof opening onto its
/// red wall: the probes see the batches, so the floor by the red wall
/// turns red and the roof keeps the sky out. Negative control: probes
/// that leave the batches out (as before they drew them) see only the
/// sky through the room, and the floor takes grey sky light, untinted.
#[test]
fn instance_batches_in_the_probes_views_tint_a_batch_room() {
    for path in [BatchPath::Gpu, BatchPath::Cpu] {
        let Some(mut rig) = Rig::red_wall_room_of(true, true, Build::Batch(path)) else {
            return;
        };
        let pixels = rig.draw(80);
        let mut old = Rig::red_wall_room_of(true, true, Build::Batch(path)).unwrap();
        old.ctx.gfx.frame3d.gi_leave_out.instance_batches = true;
        let old_pixels = old.draw(80);
        let near_red = vec3(-3.0, -0.5, -0.8);
        let (n, o) = (rig.at(&pixels, near_red), old.at(&old_pixels, near_red));
        let far_side = vec3(-0.5, -0.5, -0.8);
        let (f, fo) = (rig.at(&pixels, far_side), old.at(&old_pixels, far_side));
        eprintln!(
            "{path:?}: floor by the red wall {n:?} with batches in the probes, {o:?} without; \
             further from it {f:?} and {fo:?}"
        );
        let after = kept(&rig, pixels);
        let before = kept(&old, old_pixels);
        assert!(
            n[0] > n[1] + 20,
            "{path:?}: red bounce light from the batch wall: {n:?}"
        );
        assert!(
            (o[0] - o[1]).abs() <= 6,
            "{path:?}: negative control: without the batches the probes see no red wall: {o:?}"
        );
        // Both paths against one image: they draw the same room.
        use crate::render::render_test_support::{check_reference, reference_rejects};
        let tolerance = crate::render::regression_stage::EFFECT_REFERENCE;
        check_reference(
            "gi_instanced_room",
            &after,
            tolerance,
            "A closed white room 7 x 4 x 7 units inside, walls 0.4 thick, its west wall red, built of two \
             instance batches (white and red boxes); the sun (intensity 4) shines in from the upper east through \
             a 2 x 3 opening in the roof onto the red wall; grey sky and ground, ambient 0.3; probes every unit \
             over 12 x 4 x 8, 80 frames at 16 a frame; camera at (2, 1.3, 0.3) looking at (-3.5, -0.2, 0); \
             128x128, 4x multisampling.",
            &rig.info,
        );
        reference_rejects(
            "gi_instanced_room",
            &before,
            tolerance,
            &format!("the probes without the instance batches ({path:?} path)"),
        );
    }
}

/// A closed white room with no light but a glowing panel on its floor: the
/// probes see the panel as they see any surface, so its light reaches the
/// roof. Negative control: without global illumination the roof is black
/// (an emissive surface lights nothing directly).
#[test]
fn a_glowing_panel_lights_a_closed_room_through_the_probes() {
    let roof = |gi: bool| -> Option<i32> {
        let mut rig = Rig::new(true)?;
        let white = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
        let glow = rig.mesh(
            &MeshData::cube(1.0),
            MaterialDesc {
                emissive: Color::rgb(1.0, 0.6, 0.2),
                ..MaterialDesc::color(Color::BLACK)
            },
        );
        let (inner, outer) = (3.5, 3.9);
        let walls = [
            (white, vec3(-outer, -0.9, -outer), vec3(outer, -0.5, outer)), // floor
            (white, vec3(-outer, 3.5, -outer), vec3(outer, 3.9, outer)),   // roof
            (white, vec3(-outer, -0.5, -outer), vec3(-inner, 3.5, outer)), // west
            (white, vec3(inner, -0.5, -outer), vec3(outer, 3.5, outer)),   // east
            (white, vec3(-inner, -0.5, -outer), vec3(inner, 3.5, -inner)), // north
            (white, vec3(-inner, -0.5, inner), vec3(inner, 3.5, outer)),   // south
            (glow, vec3(-1.5, -0.5, -1.5), vec3(1.5, -0.4, 1.5)),          // panel
        ];
        rig.blocks(&walls, Build::Models);
        dark_world(&mut rig);
        rig.ctx.scene3d.lighting.gi = GlobalIllumination {
            enabled: gi,
            extent: vec3(12.0, 6.0, 12.0),
            spacing: 1.0,
            probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
            intensity: 1.0,
        };
        rig.ctx.scene3d.camera.position = vec3(-2.5, 0.5, 0.3);
        rig.ctx.scene3d.camera.look_at(vec3(0.5, 3.5, 0.0));
        let pixels = rig.draw(if gi { 120 } else { 1 });
        let points = [vec3(0.5, 3.5, 0.0), vec3(-0.5, 3.5, 1.0), vec3(1.0, 3.5, -1.0)];
        let lit = points
            .iter()
            .map(|&p| rig.at(&pixels, p))
            .max_by_key(|c| c.iter().sum::<i32>());
        let lit = lit.unwrap();
        let _image = kept(&rig, pixels);
        eprintln!("roof over the glowing panel, global illumination {gi}: {lit:?}");
        // Warm like the panel.
        assert!(!gi || lit[0] > lit[2], "the bounce has the panel's color: {lit:?}");
        Some(lit.iter().sum())
    };
    let Some(dark) = roof(false) else {
        return;
    };
    let lit = roof(true).unwrap();
    assert!(dark <= 6, "negative control: nothing lights the roof directly: {dark}");
    assert!(lit > 60, "the panel's light bounces onto the roof: {lit}");
}
