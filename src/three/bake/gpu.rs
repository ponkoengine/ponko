//! The GPU baker: the same path tracer in a compute shader (`bake.wgsl`),
//! where the GPU has compute shaders and storage buffers (every native
//! backend: Vulkan, Metal, DirectX 12, and OpenGL with compute). Not
//! WebGL2, which has neither. No ray tracing extensions: the bounding
//! volume hierarchy is walked by hand, as on the CPU.
//!
//! Each lightmap texel and probe is one invocation; its samples are split
//! over several dispatches (each a separate submission, so no single one
//! runs long enough for an operating system's GPU watchdog), and each
//! sample seeds its own random numbers, so the split doesn't change the
//! result. The CPU then smooths, fills and packs the result as after a CPU
//! bake.

use glam::Vec3;

use super::trace::TraceScene;
use super::{BakeStats, BakedLighting, Gathered, GatheredProbe, Gatherer, LightBakeScene, LightBakeSettings, Texel};
use crate::error::{Error, ErrorKind};

/// What a bake dispatch reads besides its buffers (`Params` in bake.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    nodes: u32,
    tris: u32,
    tri_count: u32,
    info: u32,
    surfaces: u32,
    lamps: u32,
    lamp_count: u32,
    maps: u32,
    sun_dir: [f32; 4],
    sun_light: [f32; 4],
    sky: [f32; 4],
    ground: [f32; 4],
    equator: [f32; 4],
    bias: f32,
    bounces: u32,
    kind: u32,
    seed: u32,
    job_start: u32,
    job_count: u32,
    sample_start: u32,
    sample_count: u32,
}

/// About this many paths go in one submission.
const PATHS_PER_SUBMIT: u32 = 1 << 19;

/// Gathers on a GPU: the [`Gatherer`] of a GPU bake.
pub(crate) struct GpuGather<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// The scene as the shader reads it, and its offsets, once made.
    scene: Option<(wgpu::Buffer, Params)>,
}

impl<'a> GpuGather<'a> {
    /// A baker on `device`.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Unsupported`] when the device has no compute shaders
    /// or fewer than three storage buffers a stage (WebGL2).
    pub(crate) fn new(device: &'a wgpu::Device, queue: &'a wgpu::Queue) -> Result<Self, Error> {
        let limits = device.limits();
        if limits.max_storage_buffers_per_shader_stage < 3 || limits.max_compute_invocations_per_workgroup < 64 {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "bake_lighting_gpu: this GPU has no compute shaders with storage buffers (WebGL2); bake on the CPU with bake_lighting",
            ));
        }
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light bake"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1, true),
                storage(2, true),
                storage(3, false),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("light bake"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bake.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("light bake"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("light bake"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Ok(Self {
            device,
            queue,
            pipeline,
            layout,
            scene: None,
        })
    }

    /// A storage buffer of `data`, or the limit error.
    fn buffer(&self, label: &str, data: &[[f32; 4]], usage: wgpu::BufferUsages) -> Result<wgpu::Buffer, Error> {
        let bytes = (data.len().max(1) * 16) as u64;
        let most = self.device.limits().max_storage_buffer_binding_size;
        if bytes > most {
            return Err(Error::new(
                ErrorKind::Limit,
                format!(
                    "bake_lighting_gpu: the {label} take {bytes} bytes, more than this GPU's storage buffers hold ({most}); \
                     bake on the CPU, or in parts"
                ),
            ));
        }
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !data.is_empty() {
            self.queue.write_buffer(&buffer, 0, bytemuck::cast_slice(data));
        }
        Ok(buffer)
    }

    /// The scene buffer and its offsets, made on the first call.
    fn scene(&mut self, trace: &TraceScene) -> Result<(), Error> {
        if self.scene.is_some() {
            return Ok(());
        }
        let mut data: Vec<[f32; 4]> = Vec::new();
        let mut p = Params {
            nodes: 0,
            ..Default::default()
        };
        p.tris = trace.bvh.write_gpu(&mut data);
        p.tri_count = trace.tri_surface.len() as u32;
        p.info = data.len() as u32;
        for (k, &surface) in trace.tri_surface.iter().enumerate() {
            let [n0, n1, n2] = trace.tri_corner_normals[k];
            let [u0, u1, u2] = trace.tri_uvs[k];
            data.push(n0.extend(f32::from_bits(surface)).to_array());
            data.push(n1.extend(0.0).to_array());
            data.push(n2.extend(0.0).to_array());
            data.push(trace.tri_normal[k].extend(0.0).to_array());
            data.push([u0.x, u0.y, u1.x, u1.y]);
            data.push([u2.x, u2.y, 0.0, 0.0]);
        }
        p.surfaces = data.len() as u32;
        for s in &trace.surfaces {
            data.push(s.albedo.extend(f32::from(u8::from(s.double_sided))).to_array());
            data.push(s.emissive.extend(f32::from_bits(s.map.unwrap_or(u32::MAX))).to_array());
        }
        p.lamps = data.len() as u32;
        p.lamp_count = trace.lamps.len() as u32;
        for lamp in &trace.lamps {
            let (forward, outer, inner) = lamp.cone.unwrap_or((Vec3::NEG_Z, -1.0, -1.0));
            data.push(lamp.position.extend(lamp.range).to_array());
            data.push(
                lamp.radiance
                    .extend(f32::from(u8::from(lamp.cone.is_some())))
                    .to_array(),
            );
            data.push(forward.extend(outer).to_array());
            data.push([inner, 0.0, 0.0, 0.0]);
        }
        p.maps = data.len() as u32;
        let mut start = p.maps + trace.maps.len() as u32;
        for map in &trace.maps {
            let (w, h) = map.size();
            data.push([f32::from_bits(start), f32::from_bits(w), f32::from_bits(h), 0.0]);
            start += w * h;
        }
        for map in &trace.maps {
            data.extend(map.texels().iter().map(|t| t.extend(0.0).to_array()));
        }
        if let Some((towards, light)) = trace.sun {
            p.sun_dir = towards.extend(1.0).to_array();
            p.sun_light = light.extend(0.0).to_array();
        }
        p.sky = trace.sky.sky.extend(0.0).to_array();
        p.ground = trace.sky.ground.extend(0.0).to_array();
        if let Some(e) = trace.sky.equator {
            p.equator = e.extend(1.0).to_array();
        }
        p.bias = trace.bias;
        p.bounces = trace.bounces;
        let buffer = self.buffer("baked scene", &data, wgpu::BufferUsages::STORAGE)?;
        self.scene = Some((buffer, p));
        Ok(())
    }

    /// Runs `samples` samples of every job (`kind` 0: texels, 1: probes),
    /// and reads back the four sums of each.
    fn run(
        &mut self,
        trace: &TraceScene,
        kind: u32,
        jobs: &[[f32; 4]],
        samples: u32,
        seed: u64,
    ) -> Result<Vec<[[f32; 4]; 4]>, Error> {
        let count = (jobs.len() / 3) as u32;
        if count == 0 {
            return Ok(Vec::new());
        }
        self.scene(trace)?;
        let (scene, base) = self.scene.as_ref().map(|(b, p)| (b, *p)).ok_or_else(|| {
            Error::new(ErrorKind::Invalid, "bake_lighting_gpu: no scene") // made just above
        })?;
        let job_buffer = self.buffer("bake jobs", jobs, wgpu::BufferUsages::STORAGE)?;
        let out_bytes = u64::from(count) * 64;
        let out = self.buffer(
            "bake sums",
            &vec![[0.0; 4]; count as usize * 4],
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        )?;
        let params = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bake params"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light bake"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: scene.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: job_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: out.as_entire_binding(),
                },
            ],
        });
        // Paths of one sample of a job cost about the bounces plus one
        // rays, each with its shadow rays.
        let sample_chunk = samples.clamp(1, 32);
        let max_groups = self.device.limits().max_compute_workgroups_per_dimension.max(1);
        let job_chunk = (PATHS_PER_SUBMIT / sample_chunk).clamp(64, max_groups.saturating_mul(64));
        let mut first_sample = 0;
        while first_sample < samples {
            let n_samples = sample_chunk.min(samples - first_sample);
            let mut first_job = 0;
            while first_job < count {
                let n_jobs = job_chunk.min(count - first_job);
                let p = Params {
                    kind,
                    seed: (seed ^ (seed >> 32)) as u32,
                    job_start: first_job,
                    job_count: n_jobs,
                    sample_start: first_sample,
                    sample_count: n_samples,
                    ..base
                };
                self.queue.write_buffer(&params, 0, bytemuck::bytes_of(&p));
                let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("light bake"),
                });
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("light bake"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(n_jobs.div_ceil(64), 1, 1);
                }
                self.queue.submit([encoder.finish()]);
                // One submission in flight at a time: the next one's
                // parameters are written after this one has run.
                self.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .map_err(|e| Error::new(ErrorKind::Io, format!("bake_lighting_gpu: the GPU stopped ({e})")))?;
                first_job += n_jobs;
            }
            first_sample += n_samples;
        }
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bake readback"),
            size: out_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bake readback"),
        });
        encoder.copy_buffer_to_buffer(&out, 0, &readback, 0, out_bytes);
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = sender.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| Error::new(ErrorKind::Io, format!("bake_lighting_gpu: the GPU stopped ({e})")))?;
        match receiver.recv() {
            Ok(Ok(())) => {}
            _ => {
                return Err(Error::new(
                    ErrorKind::Io,
                    "bake_lighting_gpu: could not read the result back from the GPU",
                ));
            }
        }
        let sums = {
            let mapped = readback.get_mapped_range(..).map_err(|e| {
                Error::new(
                    ErrorKind::Io,
                    format!("bake_lighting_gpu: could not read the result back ({e})"),
                )
            })?;
            bytemuck::cast_slice::<u8, [[f32; 4]; 4]>(&mapped).to_vec()
        };
        readback.unmap();
        Ok(sums)
    }
}

impl Gatherer for GpuGather<'_> {
    fn texels(
        &mut self,
        trace: &TraceScene,
        texels: &[Texel],
        samples: u32,
        seed: u64,
    ) -> Result<Vec<Gathered>, Error> {
        let jobs: Vec<[f32; 4]> = texels
            .iter()
            .flat_map(|t| {
                [
                    t.position.extend(0.0).to_array(),
                    t.face.extend(0.0).to_array(),
                    t.normal.extend(0.0).to_array(),
                ]
            })
            .collect();
        let sums = self.run(trace, 0, &jobs, samples, seed)?;
        Ok(texels
            .iter()
            .zip(sums)
            .map(|(t, [a, b, c, _])| {
                let weight = b[3];
                Gathered {
                    light: Vec3::new(a[0], a[1], a[2]) / samples as f32,
                    direction: if weight > 0.0 {
                        Vec3::new(b[0], b[1], b[2]) / weight
                    } else {
                        t.normal
                    },
                    inside: a[3] as u32 * 4 > samples,
                    rays: c[0] as u64,
                }
            })
            .collect())
    }

    fn probes(
        &mut self,
        trace: &TraceScene,
        positions: &[Vec3],
        samples: u32,
        seed: u64,
    ) -> Result<Vec<GatheredProbe>, Error> {
        let jobs: Vec<[f32; 4]> = positions
            .iter()
            .flat_map(|p| [p.extend(0.0).to_array(), [0.0; 4], [0.0; 4]])
            .collect();
        let sums = self.run(trace, 1, &jobs, samples, seed)?;
        let rgb = |v: [f32; 4]| Vec3::new(v[0], v[1], v[2]);
        Ok(sums
            .into_iter()
            .map(|[a, b, c, d]| {
                (
                    super::probe_from_sums([rgb(a), rgb(b), rgb(c), rgb(d)], a[3] as u32, samples),
                    d[3] as u64,
                )
            })
            .collect())
    }
}

/// [`bake_lighting`](super::bake_lighting) with the light gathered on
/// `device`: the same lightmaps and probes within the noise of the rays
/// (the GPU's random numbers are its own).
pub(crate) fn bake_on(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &LightBakeScene,
    settings: &LightBakeSettings,
) -> Result<BakedLighting, Error> {
    let mut gather = GpuGather::new(device, queue)?;
    super::bake_with(scene, settings, &mut gather)
}

/// Bakes the indirect light of `scene` as [`bake_lighting`](super::bake_lighting)
/// does, gathering it on this machine's GPU with a compute shader: the
/// same scene, settings, lightmaps and probes, within the noise of the
/// rays (the GPU's random numbers are its own, so the result is not
/// byte for byte the CPU's, nor the same on every GPU). The smoothing,
/// filling and packing stay on the CPU. It makes its own GPU device, without
/// a window: for tools and build steps.
///
/// Needs a GPU with compute shaders and storage buffers (Vulkan, Metal,
/// DirectX 12, or OpenGL with compute); not on the web.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(scene: &ponko::render::LightBakeScene) -> Result<(), Error> {
/// let settings = ponko::render::LightBakeSettings::default();
/// let baked = match ponko::render::bake_lighting_gpu(scene, &settings) {
///     Ok(baked) => baked,
///     // No GPU with compute shaders here: the CPU bakes it instead.
///     Err(e) if e.kind() == ErrorKind::Unsupported => ponko::render::bake_lighting(scene, &settings)?,
///     Err(e) => return Err(e),
/// };
/// # let _ = baked; Ok(()) }
/// ```
///
/// # Errors
///
/// As [`bake_lighting`](super::bake_lighting); also
/// [`ErrorKind::Unsupported`] without such a GPU and
/// [`ErrorKind::Limit`] for a scene larger than its storage buffers
/// (128 MiB on most GPUs: about a million triangles).
pub fn bake_lighting_gpu(scene: &LightBakeScene, settings: &LightBakeSettings) -> Result<BakedLighting, Error> {
    let unsupported = |why: String| Error::new(ErrorKind::Unsupported, format!("bake_lighting_gpu: {why}"));
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.flags = wgpu::InstanceFlags::from_env_or_default();
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .map_err(|e| unsupported(format!("no GPU ({e})")))?;
    if !adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
    {
        return Err(unsupported("this GPU has no compute shaders".into()));
    }
    let limits = adapter.limits();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("light bake"),
        required_limits: wgpu::Limits {
            max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
            max_buffer_size: limits.max_buffer_size,
            ..wgpu::Limits::downlevel_defaults()
        },
        ..Default::default()
    }))
    .map_err(|e| unsupported(format!("no GPU device ({e})")))?;
    let start = std::time::Instant::now();
    let mut baked = bake_on(&device, &queue, scene, settings)?;
    baked.stats = BakeStats {
        seconds: start.elapsed().as_secs_f64(),
        ..baked.stats
    };
    Ok(baked)
}
