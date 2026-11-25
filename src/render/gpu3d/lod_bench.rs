//! Offscreen benchmark of where level-of-detail work goes under a moving
//! camera, for `ponko_bench` (`render/lod`): a field of identical
//! three-level [`LodGroup`]s drawn either as entities (the CPU picks every
//! group's level in `Frame3D::collect` each frame and uploads the instances
//! it draws) or as one static batch of [`LodGroup::instance_levels`] (the
//! level is picked per instance where the batch is culled: in the GPU's
//! culling pass on the GPU-driven path, cell by cell on the CPU path). The
//! same `Renderer3D` a game uses, driven as `Ctx::prepare_3d` drives it,
//! into a texture, without a window. Not part of the public API. See
//! `crates/ponko_bench/LOD_SPLIT.md`.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec2, Vec3};

use super::{Prepare3D, Renderer3D};
use crate::ctx::Ctx;
use crate::d3::Transform as Transform3D;
use crate::render::gpu::GpuTexture;
use crate::render::timer::GpuTimer;
use crate::render::{
    BatchCulling, GpuTier, InstanceBatchOptions, LodGroup, LodLevel, LodRenderer, MaterialDesc, Mesh, MeshData,
};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

/// How a [`LodBench`] draws its groups.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodPath {
    /// One entity per group: levels picked on the CPU every frame.
    Entities,
    /// One batch of the groups' instance levels, culled (and its levels
    /// picked) in the GPU's culling pass where the device has one.
    GpuBatch,
    /// The same batch culled and picked cell by cell on the CPU: what a
    /// device without compute culling does.
    CpuBatch,
}

impl LodPath {
    /// Every path, in the order the benchmark reports them.
    pub const ALL: [LodPath; 3] = [LodPath::Entities, LodPath::GpuBatch, LodPath::CpuBatch];

    /// The name used in benchmark names.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LodPath::Entities => "entities",
            LodPath::GpuBatch => "gpu-batch",
            LodPath::CpuBatch => "cpu-batch",
        }
    }
}

/// What a [`LodBench`] draws.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct LodScene {
    /// The output's width and height, in pixels.
    pub size: (u32, u32),
    /// Groups, on a square grid 4 m apart.
    pub groups: usize,
    pub path: LodPath,
    /// Sun shadows (the default cascades).
    pub shadows: bool,
    /// How far the camera flies each frame, in metres.
    pub speed: f32,
}

/// What one frame cost.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LodFrame {
    /// `Frame3D::collect`: for entities, picking every group's level,
    /// culling its renderers and listing the instances drawn.
    pub collect: Duration,
    /// `Renderer3D::prepare`: uploads, and a CPU-culled batch's culling.
    pub prepare: Duration,
    /// `Renderer3D::draw`: recording every pass.
    pub encode: Duration,
    pub submit: Duration,
    /// From before collecting until the GPU finished.
    pub frame: Duration,
    /// All passes' GPU time, and the static batches' culling pass's, in
    /// ms, when this frame's timestamps came back.
    pub gpu_ms: Option<f64>,
    pub cull_gpu_ms: Option<f64>,
    /// Instances and triangles the camera drew from instance lists (the
    /// entities': a batch's are not counted), and draw calls.
    pub instances: usize,
    pub triangles: usize,
    pub draw_calls: usize,
    /// Bytes of instances uploaded this frame (entities' instance lists).
    pub instance_bytes: u64,
    /// Bytes uploaded this frame for static batches.
    pub batch_bytes: u64,
}

/// What a [`LodBench`]'s groups keep, in bytes (computed from the sizes of
/// what is held, not read from the allocator or the device).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LodMemory {
    /// The groups as the CPU holds them: the entities' transforms and
    /// `LodGroup`s with their level and renderer lists, or the batch's
    /// copy of its instances, bounding spheres, cells and levels.
    pub cpu: u64,
    /// The frame's instance list (entities: what the camera and the shadow
    /// maps draw this frame, uploaded where it changed), at its capacity.
    pub frame_instances: u64,
    /// The GPU-driven batches' buffer pools (instances, cells, the visible
    /// lists and indirect arguments of every view and level).
    pub gpu_pools: u64,
}

/// A field of level-of-detail groups, drawn a frame at a time.
#[doc(hidden)]
pub struct LodBench {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: wgpu::TextureView,
    textures: Vec<GpuTexture>,
    renderer: Renderer3D,
    timer: Option<GpuTimer>,
    gpu_frames_read: u64,
    ctx: Ctx,
    frame: Frame3D,
    skins: SkinFrame,
    scene: LodScene,
    /// The batch, for the batch paths.
    batch: Option<u32>,
    /// Where the camera starts, and the field's width.
    start: Vec3,
    extent: f32,
    frames: u32,
    adapter: String,
    cpu_bytes: u64,
}

impl LodBench {
    /// Opens the GPU as a game does (the high-performance adapter, default
    /// limits, timestamp queries where there are any, `PONKO_GPU_TIER`
    /// honored) and builds the field. `Err` says why there is no GPU.
    pub fn new(scene: LodScene) -> Result<Self, String> {
        // Backends and flags from wgpu's environment variables, as the window's
        // instance takes them: `WGPU_BACKEND=dx12` benchmarks DirectX 12.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter ({e})"))?;
        let info = adapter.get_info();
        let mut descriptor = wgpu::DeviceDescriptor {
            label: Some("ponko LOD benchmark"),
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        };
        if let Some(tier) = GpuTier::forced() {
            descriptor.required_features = tier.device_features(descriptor.required_features);
            descriptor.required_limits = tier.device_limits(descriptor.required_limits);
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))
            .map_err(|e| format!("no GPU device on {} ({e})", info.name))?;
        let (layout, _sampler, white) = super::record_bench::white_texture(&device, &queue);
        let textures = vec![white];
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("LOD benchmark output"),
            size: wgpu::Extent3d {
                width: scene.size.0,
                height: scene.size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target = output.create_view(&Default::default());
        // As a game's window: Apple's GPUs skip the solid depth prepass.
        let solid_prepass = info.backend != wgpu::Backend::Metal;
        let renderer = Renderer3D::new(
            &device,
            &layout,
            output.format(),
            0,
            &textures[0].texture,
            4,
            solid_prepass,
        );
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(scene.size.0 as f32, scene.size.1 as f32));
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.lighting.shadows = scene.shadows;
        ctx.scene3d.camera.far = 400.0;
        let timer = GpuTimer::new(&device, &queue);
        let side = (scene.groups as f32).sqrt().ceil().max(1.0) as usize;
        let extent = side as f32 * 4.0;
        let mut bench = Self {
            device,
            queue,
            target,
            textures,
            renderer,
            timer,
            gpu_frames_read: 0,
            ctx,
            frame: Frame3D::default(),
            skins: SkinFrame::default(),
            scene,
            batch: None,
            start: Vec3::new(0.0, 8.0, extent * 0.5),
            extent,
            frames: 0,
            adapter: format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type),
            cpu_bytes: 0,
        };
        bench.build(side);
        let _ = bench.device.poll(wgpu::PollType::wait_indefinitely());
        crate::render::traffic::take();
        Ok(bench)
    }

    /// A lit mesh of `data`, on the CPU and the GPU.
    fn mesh(&mut self, data: &MeshData, color: crate::color::Color) -> Mesh {
        let desc = MaterialDesc {
            color,
            ..Default::default()
        };
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        self.renderer.create_mesh_at(
            &self.device,
            mesh.slot(),
            data,
            &[(0..data.indices.len() as u32, material)],
            true,
        );
        mesh
    }

    /// The group every copy is: a 2 m sphere of 16 segments while it covers
    /// 15% of the screen's height, 6 segments down to 4%, a box down to 1%,
    /// then nothing (11, 43 and 173 m away at the default field of view).
    fn build(&mut self, side: usize) {
        let near = self.mesh(&MeshData::sphere(1.0, 16), crate::color::Color::rgb(0.3, 0.7, 0.3));
        let middle = self.mesh(&MeshData::sphere(1.0, 6), crate::color::Color::rgb(0.4, 0.7, 0.3));
        let far = self.mesh(&MeshData::cube(1.6), crate::color::Color::rgb(0.5, 0.6, 0.3));
        let group = LodGroup::new(
            2.0,
            vec![
                LodLevel::new(0.15, vec![LodRenderer::new(near)]),
                LodLevel::new(0.04, vec![LodRenderer::new(middle)]),
                LodLevel::new(0.01, vec![LodRenderer::new(far)]),
            ],
        );
        // A fixed scatter around the grid points, each turned its own way.
        let transforms: Vec<Transform3D> = (0..self.scene.groups)
            .map(|i| {
                let hash = (i as u32).wrapping_mul(2_654_435_761);
                let jitter = |shift: u32| ((hash >> shift) & 0xff) as f32 / 255.0 - 0.5;
                Transform3D {
                    position: Vec3::new(
                        (i % side) as f32 * 4.0 - self.extent * 0.5 + jitter(0) * 2.0,
                        0.0,
                        -((i / side) as f32) * 4.0 + self.extent * 0.5 + jitter(8) * 2.0,
                    ),
                    rotation: Quat::from_rotation_y(jitter(16) * std::f32::consts::TAU),
                    scale: Vec3::ONE,
                }
            })
            .collect();
        match self.scene.path {
            LodPath::Entities => {
                let per_group = size_of::<Transform3D>()
                    + size_of::<LodGroup>()
                    + group.levels.capacity() * size_of::<LodLevel>()
                    + group
                        .levels
                        .iter()
                        .map(|l| l.renderers.capacity() * size_of::<LodRenderer>())
                        .sum::<usize>();
                self.cpu_bytes = (per_group * transforms.len()) as u64;
                for t in transforms {
                    self.ctx.world.spawn((t, group.clone()));
                }
            }
            LodPath::GpuBatch | LodPath::CpuBatch => {
                let levels = group
                    .instance_levels(1.0, self.ctx.scene3d.camera.fov_y, &self.ctx.scene3d.lod)
                    .expect("a group a batch can draw");
                let matrices: Vec<Mat4> = transforms.iter().map(Transform3D::matrix).collect();
                let culling = if self.scene.path == LodPath::GpuBatch {
                    BatchCulling::PreferGpu
                } else {
                    BatchCulling::PreferCpu
                };
                let batch = self.ctx.gfx.create_instance_batch_levels(
                    &levels,
                    &matrices,
                    &[],
                    InstanceBatchOptions {
                        culling,
                        ..Default::default()
                    },
                );
                let data = self.ctx.gfx.assets3d.instance_batches[batch.id as usize].as_ref();
                if let Some(b) = data {
                    self.cpu_bytes = (b.instances.capacity() * size_of::<crate::three::Instance3D>()
                        + b.spheres.capacity() * size_of::<[f32; 4]>()
                        + b.cells.capacity() * size_of::<crate::three::instances::Cell>()
                        + b.levels.capacity() * size_of::<crate::render::InstanceLevel>())
                        as u64;
                }
                self.renderer
                    .set_instance_batch(&self.device, &self.queue, batch.id, data);
                self.batch = Some(batch.id);
            }
        }
    }

    /// The GPU the frames are drawn on: name, backend and type.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// The path the batch takes and why (`gpu`, `cpu`, `cpu: no compute
    /// culling`, `cpu: over a device limit`), or `entities`.
    #[must_use]
    pub fn route(&self) -> &'static str {
        let stats = self.renderer.instance_batch_stats();
        match self.batch {
            None => "entities",
            Some(_) if stats.gpu_driven > 0 => "gpu",
            Some(_) if stats.cpu_no_compute > 0 => "cpu: no compute culling",
            Some(_) if stats.cpu_over_limit > 0 => "cpu: over a device limit",
            Some(_) => "cpu",
        }
    }

    /// Whether the device times passes.
    #[must_use]
    pub fn timed(&self) -> bool {
        self.timer.is_some()
    }

    /// What the groups keep, after the frames drawn so far.
    #[must_use]
    pub fn memory(&self) -> LodMemory {
        LodMemory {
            cpu: self.cpu_bytes,
            frame_instances: (self.frame.instances.capacity() * size_of::<crate::three::Instance3D>()) as u64,
            gpu_pools: self.renderer.instance_batch_stats().pool_bytes,
        }
    }

    /// Flies the camera on, draws one frame, waits for the GPU and says
    /// what each step cost.
    pub fn frame(&mut self) -> LodFrame {
        // Straight across the field and back along the same line, low,
        // looking ahead and down: the levels of thousands of groups change
        // every frame.
        let travel = self.extent * 0.8;
        let along = (self.frames as f32 * self.scene.speed) % (2.0 * travel);
        let along = if along > travel { 2.0 * travel - along } else { along };
        let ctx = &mut self.ctx;
        let position = self.start - Vec3::Z * along;
        ctx.scene3d.camera.position = position;
        let sway = (self.frames as f32 * 0.02).sin() * 0.3;
        ctx.scene3d
            .camera
            .look_at(position + Vec3::new(sway.sin(), -0.25, -sway.cos()) * 10.0);
        let started = Instant::now();
        let frame = &mut self.frame;
        frame.lod = ctx.scene3d.lod;
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &self.skins,
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let collected = Instant::now();
        let scene = Scene3D {
            frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: Vec3::splat(ctx.scene3d.lighting.sun_intensity),
                sky: Vec3::splat(0.3),
                ground: Vec3::splat(0.1),
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (self.frames as f64 / 60.0, 1.0 / 60.0),
            occlusion: false,
        };
        self.renderer.prepare(
            &self.device,
            &self.queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: self.scene.size,
            },
        );
        let prepared = Instant::now();
        if let Some(timer) = &mut self.timer {
            timer.begin_frame();
        }
        let mut buffers = self.renderer.draw(
            &self.device,
            &self.target,
            wgpu::Color::BLACK,
            &self.textures,
            self.timer.as_ref(),
        );
        if let Some(timer) = &mut self.timer {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            timer.resolve(&mut encoder);
            buffers.push(encoder.finish());
        }
        let encoded = Instant::now();
        self.queue.submit(buffers);
        let submitted = Instant::now();
        if let Some(timer) = &mut self.timer {
            timer.submitted();
        }
        self.renderer.submitted();
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let done = Instant::now();
        let (mut gpu_ms, mut cull_gpu_ms) = (None, None);
        if let Some(timer) = &mut self.timer {
            timer.collect(&self.device);
            if timer.frames_read != self.gpu_frames_read {
                self.gpu_frames_read = timer.frames_read;
                let mut total = 0.0;
                let mut cull = 0.0;
                for span in &timer.last_frame {
                    total += f64::from(span.ms);
                    if span.name == "static instance cull" {
                        cull += f64::from(span.ms);
                    }
                }
                gpu_ms = Some(total);
                cull_gpu_ms = Some(cull);
            }
        }
        let [(instances, triangles), _] = self.renderer.counts();
        let uploads = crate::render::traffic::take();
        self.frames += 1;
        LodFrame {
            collect: collected - started,
            prepare: prepared - collected,
            encode: encoded - prepared,
            submit: submitted - encoded,
            frame: done - started,
            gpu_ms,
            cull_gpu_ms,
            instances,
            triangles,
            draw_calls: self.renderer.draw_calls(),
            instance_bytes: uploads.instances,
            batch_bytes: uploads.static_batches,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny(path: LodPath) -> Option<LodBench> {
        match LodBench::new(LodScene {
            size: (64, 64),
            groups: 400,
            path,
            shadows: false,
            speed: 2.0,
        }) {
            Ok(bench) => Some(bench),
            Err(reason) => {
                crate::render::render_test_support::no_gpu(reason);
                None
            }
        }
    }

    #[test]
    fn every_path_draws_the_field_under_a_moving_camera() {
        for path in LodPath::ALL {
            let Some(mut bench) = tiny(path) else {
                return;
            };
            let frames: Vec<LodFrame> = (0..3).map(|_| bench.frame()).collect();
            assert!(bench.memory().cpu > 0, "{}: the groups take memory", path.name());
            if path == LodPath::Entities {
                assert_eq!(bench.route(), "entities");
                // The CPU picks and lists the groups the camera sees.
                assert!(frames.iter().all(|f| f.instances > 10), "{frames:?}");
            } else {
                // The batch holds them all; what it draws, the GPU or the
                // cells decide (not counted in `instances`).
                assert_eq!(bench.renderer.instance_batch_stats().instances, 400);
                assert!(
                    frames.iter().all(|f| f.instances == 0 && f.draw_calls > 0),
                    "{frames:?}"
                );
            }
        }
    }
}
