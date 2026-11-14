//! Fragment-only GTAO and contact shadows, and bilateral
//! denoising: occlusion in red, the contact shadow in green. With
//! screen-space reflections, the denoised result goes into the first level
//! of their image instead (see `ssr.rs`).
use crate::render::traffic::{Counted, Upload};
use glam::{Mat4, Vec3};
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Settings {
    inverse: [[f32; 4]; 4],
    projection: [[f32; 4]; 4],
    previous: [[f32; 4]; 4],
    temporal: [f32; 4],
    camera: [f32; 4],
    params: [f32; 4],
    sun: [f32; 4],
}
pub(super) struct Ao {
    layout: wgpu::BindGroupLayout,
    raw_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    spatial_pipeline: wgpu::RenderPipeline,
    spatial_wide_pipeline: wgpu::RenderPipeline,
    /// Denoising into the screen-space reflections' image.
    blur_wide_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    /// What `uniform` holds: only what changed is uploaded.
    uniform_mirror: crate::render::traffic::Mirror,
    previous: Option<Mat4>,
    frame: u32,
    pub half: bool,
    pub temporal: bool,
    signature: Option<(bool, bool, [f32; 4])>,
}
pub(super) struct AoTargets {
    raw: wgpu::TextureView,
    pub output: wgpu::TextureView,
    raw_group: wgpu::BindGroup,
    blur_groups: [wgpu::BindGroup; 2],
    history: [wgpu::TextureView; 2],
    index: AtomicUsize,
    pub half: bool,
    pub temporal: bool,
    /// Whether `output` is the screen-space reflections' image.
    wide: bool,
}
impl AoTargets {
    /// The raw image and, with temporal accumulation, a history image: for
    /// checking them against what the frame's passes declare.
    pub fn images(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        (&self.raw, self.temporal.then_some(&self.history[0]))
    }
}
impl Ao {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = |binding, sample_type, multisampled| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GTAO"),
            entries: &[
                texture(0, wgpu::TextureSampleType::Depth, true),
                texture(4, wgpu::TextureSampleType::Float { filterable: false }, false),
                texture(3, wgpu::TextureSampleType::Float { filterable: false }, false),
                texture(1, wgpu::TextureSampleType::Float { filterable: false }, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("ao.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GTAO"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[
                        Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        }),
                        (entry == "fs_denoise").then_some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba16Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        }),
                    ][..if entry == "fs_denoise" { 2 } else { 1 }],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let raw_pipeline = pipeline("fs_ao", wgpu::TextureFormat::Rg8Unorm);
        let blur_pipeline = pipeline("fs_denoise", wgpu::TextureFormat::Rg8Unorm);
        let blur_wide_pipeline = pipeline("fs_denoise", super::ssr::SCREEN_FORMAT);
        let spatial_pipeline = pipeline("fs_spatial", wgpu::TextureFormat::Rg8Unorm);
        let spatial_wide_pipeline = pipeline("fs_spatial", super::ssr::SCREEN_FORMAT);
        let uniform = device
            .uploading(Upload::Uniforms)
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("GTAO"),
                contents: bytemuck::bytes_of(&Settings::zeroed()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | crate::render::traffic::MIRRORED,
            });
        Self {
            layout,
            raw_pipeline,
            blur_pipeline,
            spatial_pipeline,
            spatial_wide_pipeline,
            blur_wide_pipeline,
            uniform,
            uniform_mirror: Default::default(),
            previous: None,
            frame: 0,
            half: false,
            temporal: false,
            signature: None,
        }
    }
    /// `ao`: `None` for contact shadows alone. `sun`: toward the sun, and
    /// the contact shadows' length (0: none).
    pub fn reset(&mut self) {
        self.previous = None;
        self.signature = None;
    }
    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        view_proj: Mat4,
        camera: Vec3,
        ao: Option<&crate::render::AmbientOcclusion>,
        sun: (Vec3, f32),
        modes: (bool, bool),
    ) {
        let off = crate::render::AmbientOcclusion {
            intensity: 0.0,
            ..Default::default()
        };
        let ao = ao.unwrap_or(&off);
        let (half, temporal) = modes;
        self.half = half && ao.enabled;
        self.temporal = temporal && ao.enabled;
        let signature = (
            self.half,
            temporal,
            [ao.radius, ao.intensity, ao.slices as f32, ao.steps as f32],
        );
        let valid = self.signature == Some(signature);
        self.signature = Some(signature);
        self.uniform_mirror.upload(
            queue,
            Upload::Uniforms,
            &self.uniform,
            0,
            bytemuck::bytes_of(&Settings {
                inverse: view_proj.inverse().to_cols_array_2d(),
                projection: view_proj.to_cols_array_2d(),
                previous: self.previous.unwrap_or(view_proj).to_cols_array_2d(),
                temporal: [
                    if temporal && valid { 0.85 } else { 0.0 },
                    if temporal { (self.frame % 8) as f32 / 8.0 } else { 0.0 },
                    self.half as u32 as f32,
                    0.0,
                ],
                camera: camera.extend(1.0).to_array(),
                params: [
                    ao.radius.max(0.001),
                    ao.intensity.max(0.0),
                    ao.slices.clamp(1, 8) as f32,
                    ao.steps.clamp(1, 16) as f32,
                ],
                sun: sun.0.normalize_or_zero().extend(sun.1).to_array(),
            }),
        );
        self.previous = Some(view_proj);
        self.frame = self.frame.wrapping_add(1);
    }
    pub fn targets(
        &self,
        device: &wgpu::Device,
        size: (u32, u32),
        depth: &wgpu::TextureView,
        dummy_color: &wgpu::TextureView,
        reflections: Option<&wgpu::TextureView>,
        velocity: &wgpu::TextureView,
    ) -> AoTargets {
        let target = |half: bool, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("GTAO"),
                    size: wgpu::Extent3d {
                        width: if half { size.0.div_ceil(2) } else { size.0 },
                        height: if half { size.1.div_ceil(2) } else { size.1 },
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let raw = target(self.half, wgpu::TextureFormat::Rg8Unorm);
        let output = reflections
            .cloned()
            .unwrap_or_else(|| target(false, wgpu::TextureFormat::Rg8Unorm));
        let history = std::array::from_fn(|_| {
            if self.temporal {
                target(false, wgpu::TextureFormat::Rgba16Float)
            } else {
                dummy_color.clone()
            }
        });
        let group = |image: &wgpu::TextureView, history: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("GTAO"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(velocity),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(history),
                    },
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(image),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.uniform.as_entire_binding(),
                    },
                ],
            })
        };
        let raw_group = group(dummy_color, &history[0]);
        let blur_groups = std::array::from_fn(|i| group(&raw, &history[1 - i]));
        AoTargets {
            raw,
            output,
            raw_group,
            blur_groups,
            history,
            index: AtomicUsize::new(0),
            half: self.half,
            temporal: self.temporal,
            wide: reflections.is_some(),
        }
    }
    pub fn draw<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &AoTargets,
        time: &dyn Fn(&'static str) -> Option<wgpu::RenderPassTimestampWrites<'t>>,
    ) {
        let index = targets.index.load(Ordering::Relaxed);
        targets.index.store(1 - index, Ordering::Relaxed);
        for (label, pipeline, group, target) in [
            ("GTAO horizons", &self.raw_pipeline, &targets.raw_group, &targets.raw),
            (
                "GTAO bilateral",
                match (targets.temporal, targets.wide) {
                    (true, true) => &self.blur_wide_pipeline,
                    (true, false) => &self.blur_pipeline,
                    (false, true) => &self.spatial_wide_pipeline,
                    (false, false) => &self.spatial_pipeline,
                },
                &targets.blur_groups[index],
                &targets.output,
            ),
        ] {
            super::gpu3d::mark_step(if label == "GTAO horizons" {
                "ambient occlusion"
            } else {
                "ambient occlusion denoise"
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                timestamp_writes: time(label),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    (label == "GTAO bilateral" && targets.temporal).then_some(wgpu::RenderPassColorAttachment {
                        view: &targets.history[index],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ][..if label == "GTAO bilateral" && targets.temporal {
                    2
                } else {
                    1
                }],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
use bytemuck::Zeroable;

#[cfg(test)]
impl Ao {
    /// See `Renderer3D::check_mirrors`.
    pub fn check_mirrors(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.uniform_mirror
            .check(device, queue, &self.uniform, 0, "ambient occlusion settings");
    }
}

impl Ao {
    /// Counts its uniform as other memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.buffer(C::Other, &self.uniform);
    }
}

impl AoTargets {
    /// Counts ambient occlusion's targets and history as frame targets (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.view(C::FrameTargets, &self.raw);
        t.view(C::FrameTargets, &self.output);
        for view in &self.history {
            t.view(C::FrameTargets, view);
        }
    }
}
