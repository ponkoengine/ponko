//! Screen-space reflections (see `ssr.wgsl`): a half-resolution trace after
//! the depth prepass, read by the opaque pass, and a copy of the opaque
//! image after it, which the next frame's trace takes its colors from.
//!
//! Material shaders have no texture slot to spare, so the opaque pass reads
//! the reflections where it reads GTAO: its scene color slot holds a
//! mipmapped image, GTAO and contact shadows in the first level (full size,
//! red and green), the reflections in the second (half size), followed by up to four
//! depth-aware filtered reflection levels (one compute dispatch makes them
//! all, see `Blur`). Blue 1 in the
//! first level says the second is there: GTAO writes it, or without GTAO
//! the trace's mark pass. (GTAO's own RG8 image and the opaque pass's
//! black stand-in read 0 there; GLSL ES can't count a texture's levels.)
//!
//! The first level's alpha is the reflection cone's spread (see
//! `ssr.wgsl`): how much a ray's length widens its blur. The trace writes
//! it to a half-size image of its own, a fill pass gives the rays that
//! found nothing their neighbors', and GTAO's denoise (or the mark pass,
//! without GTAO) copies it into that alpha, so the opaque pass blurs each
//! reflection by its roughness and how far its ray went.
use crate::render::traffic::{Counted, Upload};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use glam::{Mat4, Vec3};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Settings {
    inverse: [[f32; 4]; 4],
    projection: [[f32; 4]; 4],
    previous: [[f32; 4]; 4],
    camera: [f32; 4],
    params: [f32; 4],
}

/// The image the opaque pass reads GTAO and the reflections from.
pub(super) const SCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The reflection cone's spread, in full-size pixels (see `ssr.wgsl`).
pub(super) const SPREAD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
/// The trace's second target: the cone's spread and 1 where its ray found
/// something, 0 where it didn't.
pub(super) const HITS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// Lets the cost test time frames with and without the filtered levels.
#[cfg(test)]
pub(super) static SKIP_FILTER: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
fn filter_skipped() -> bool {
    SKIP_FILTER.load(Ordering::Relaxed)
}
#[cfg(not(test))]
fn filter_skipped() -> bool {
    false
}

pub(super) struct Ssr {
    layout: wgpu::BindGroupLayout,
    trace_pipeline: wgpu::RenderPipeline,
    copy_pipeline: wgpu::RenderPipeline,
    /// The cone's spread where rays found nothing.
    fill_layout: wgpu::BindGroupLayout,
    fill_pipeline: wgpu::RenderPipeline,
    /// Without GTAO: the first level's mark and the cone's spread.
    mark_pipeline: wgpu::RenderPipeline,
    blur_layout: wgpu::BindGroupLayout,
    blur_pipeline: wgpu::ComputePipeline,
    uniform: wgpu::Buffer,
    /// What `uniform` holds: only what changed is uploaded.
    uniform_mirror: crate::render::traffic::Mirror,
    sampler: wgpu::Sampler,
    /// The camera the image in the history was drawn with, while it holds
    /// the last frame's.
    previous: Option<Mat4>,
    frame: u32,
}

pub(super) struct SsrTargets {
    /// The last frame's opaque image.
    history: wgpu::TextureView,
    /// The mipmapped image the opaque pass reads (see the module docs).
    pub screen: wgpu::TextureView,
    /// Its first level, GTAO's target.
    pub ao: wgpu::TextureView,
    /// Its second level, the trace's target.
    output: wgpu::TextureView,
    /// The trace's second target, at the second level's size: the cone's
    /// spread where the rays found something, and whether they did.
    hits: wgpu::TextureView,
    /// The same filled in where they didn't, which GTAO's denoise or the
    /// mark pass copies into the first level's alpha.
    spread: wgpu::TextureView,
    /// The filtered levels, if there's room for any of them.
    blur: Option<Blur>,
    /// Without GTAO: the mark pass writes the first level each frame.
    mark: bool,
    trace_group: wgpu::BindGroup,
    copy_group: wgpu::BindGroup,
    fill_group: wgpu::BindGroup,
    mark_group: wgpu::BindGroup,
}

/// The filtered levels' pass: the compute shader writes them to `levels`,
/// then they're copied into `screen`'s lower levels. (Letting the shader
/// write `screen` itself slowed the passes that draw into and read it: by
/// more than the copies cost, measured on an Apple GPU.)
struct Blur {
    group: wgpu::BindGroup,
    workgroups: (u32, u32),
    levels: wgpu::Texture,
    screen: wgpu::Texture,
}

impl SsrTargets {
    /// Whether [`Ssr::trace`] filters the levels below the trace's.
    pub fn has_levels(&self) -> bool {
        self.blur.is_some() && !filter_skipped()
    }

    /// The history image and the filtered levels' texture, if any: for
    /// checking them against what the frame's passes declare.
    pub fn images(&self) -> (&wgpu::TextureView, Option<&wgpu::Texture>) {
        (&self.history, self.blur.as_ref().map(|b| &b.levels))
    }

    /// The cone's spread, as the trace writes it, and filled in (see the
    /// module docs).
    pub fn spread(&self) -> (&wgpu::TextureView, &wgpu::TextureView) {
        (&self.hits, &self.spread)
    }
}

impl Ssr {
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
            label: Some("screen reflections"),
            entries: &[
                texture(0, wgpu::TextureSampleType::Depth, true),
                texture(1, wgpu::TextureSampleType::Float { filterable: true }, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
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
        let shader = device.create_shader_module(wgpu::include_wgsl!("ssr.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screen reflections"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let target = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let pipeline = |entry, targets: &[Option<wgpu::ColorTargetState>]| {
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
                    targets,
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let trace_pipeline = pipeline("fs_trace", &[target(SCREEN_FORMAT), target(HITS_FORMAT)]);
        let fill_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen reflection spread"),
            entries: &[
                texture(1, wgpu::TextureSampleType::Float { filterable: true }, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture(8, wgpu::TextureSampleType::Float { filterable: true }, false),
            ],
        });
        let fill_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fs_fill"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("screen reflection spread"),
                bind_group_layouts: &[Some(&fill_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_fill"),
                compilation_options: Default::default(),
                targets: &[target(SPREAD_FORMAT)],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let mark_pipeline = pipeline("fs_mark", &[target(SCREEN_FORMAT)]);
        let compute = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let level = |binding| {
            compute(
                binding,
                wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: SCREEN_FORMAT,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
            )
        };
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen reflection levels"),
            entries: &[
                compute(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                ),
                compute(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                compute(
                    3,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                level(4),
                level(5),
                level(6),
                level(7),
            ],
        });
        let blur_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("screen reflection levels"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("screen reflection levels"),
                bind_group_layouts: &[Some(&blur_layout)],
                immediate_size: 0,
            })),
            module: &shader,
            entry_point: Some("cs_blur"),
            compilation_options: Default::default(),
            cache: None,
        });
        let copy_pipeline = pipeline("fs_copy", &[target(super::gpu3d::HDR_FORMAT)]);
        let uniform = device
            .uploading(Upload::Uniforms)
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("screen reflections"),
                contents: bytemuck::bytes_of(&<Settings as bytemuck::Zeroable>::zeroed()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | crate::render::traffic::MIRRORED,
            });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen reflections"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            trace_pipeline,
            copy_pipeline,
            fill_layout,
            fill_pipeline,
            mark_pipeline,
            blur_layout,
            blur_pipeline,
            uniform,
            uniform_mirror: Default::default(),
            sampler,
            previous: None,
            frame: 0,
        }
    }

    /// Forgets the last frame's image (reflections are off, or it can't be
    /// reprojected): the next frame reflects only the environment.
    pub fn forget(&mut self) {
        self.previous = None;
    }

    /// This frame's settings: the camera it's drawn with (jittered, with
    /// TAA), `strength` 0 to 1 and the longest ray in world units. Call
    /// only for frames that are drawn; their image is the next frame's
    /// history.
    pub fn prepare(&mut self, queue: &wgpu::Queue, view_proj: Mat4, camera: Vec3, strength: f32, longest: f32) {
        let previous = self.previous.replace(view_proj);
        self.frame = self.frame.wrapping_add(1);
        self.uniform_mirror.upload(
            queue,
            Upload::Uniforms,
            &self.uniform,
            0,
            bytemuck::bytes_of(&Settings {
                inverse: view_proj.inverse().to_cols_array_2d(),
                projection: view_proj.to_cols_array_2d(),
                previous: previous.unwrap_or(view_proj).to_cols_array_2d(),
                camera: camera.extend(1.0).to_array(),
                params: [
                    if previous.is_some() { strength } else { 0.0 },
                    longest,
                    (self.frame % 64) as f32,
                    0.0,
                ],
            }),
        );
    }

    /// Targets for frames of `size`: `depth` the prepass's, `lit` the image
    /// the opaque pass resolves into; `ao` whether GTAO writes `ao` (and
    /// copies the cone's spread into it).
    pub fn targets(
        &self,
        device: &wgpu::Device,
        size: (u32, u32),
        depth: &wgpu::TextureView,
        lit: &wgpu::TextureView,
        ao: bool,
    ) -> SsrTargets {
        let texture = |label, size: (u32, u32), levels, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | usage,
                view_formats: &[],
            })
        };
        let history = texture(
            "screen reflections history",
            size,
            1,
            super::gpu3d::HDR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .create_view(&Default::default());
        // At least 2 pixels across, for the second level.
        let extent = (size.0.max(2), size.1.max(2));
        let levels = (32 - extent.0.max(extent.1).leading_zeros()).min(6);
        let screen = texture(
            "screen reflections",
            extent,
            levels,
            SCREEN_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
        );
        let level = |level| {
            screen.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let half = |label, format| {
            texture(
                label,
                ((extent.0 >> 1).max(1), (extent.1 >> 1).max(1)),
                1,
                format,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
            .create_view(&Default::default())
        };
        let hits = half("screen reflection hits", HITS_FORMAT);
        let spread = half("screen reflection spread", SPREAD_FORMAT);
        let group = |image: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("screen reflections"),
                layout: &self.layout,
                entries: &[
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
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.uniform.as_entire_binding(),
                    },
                ],
            })
        };
        // The filtered levels; a stand-in for those too small to exist.
        let blur = (levels > 2).then(|| {
            let filtered = texture(
                "screen reflection levels",
                ((extent.0 >> 2).max(1), (extent.1 >> 2).max(1)),
                levels - 2,
                SCREEN_FORMAT,
                wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            );
            let stand_in = texture(
                "screen reflections stand-in",
                (1, 1),
                1,
                SCREEN_FORMAT,
                wgpu::TextureUsages::STORAGE_BINDING,
            );
            let views: Vec<_> = (2..6)
                .map(|mip| {
                    if mip < levels {
                        filtered.create_view(&wgpu::TextureViewDescriptor {
                            base_mip_level: mip - 2,
                            mip_level_count: Some(1),
                            ..Default::default()
                        })
                    } else {
                        stand_in.create_view(&Default::default())
                    }
                })
                .collect();
            let source = level(1);
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&source),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.uniform.as_entire_binding(),
                },
            ];
            entries.extend(views.iter().zip(4..).map(|(view, binding)| wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::TextureView(view),
            }));
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("screen reflection levels"),
                layout: &self.blur_layout,
                entries: &entries,
            });
            // Each workgroup makes 8x8 pixels of level 2.
            let size = filtered.size();
            Blur {
                group,
                workgroups: (size.width.div_ceil(8), size.height.div_ceil(8)),
                levels: filtered,
                screen: screen.clone(),
            }
        });
        SsrTargets {
            blur,
            trace_group: group(&history),
            copy_group: group(lit),
            fill_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("screen reflection spread"),
                layout: &self.fill_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&hits),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    // A coarse filtered level, or the trace's own without
                    // any (see `fs_fill`).
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::TextureView(&level(if levels > 2 {
                            levels.min(5) - 1
                        } else {
                            1
                        })),
                    },
                ],
            }),
            mark_group: group(&spread),
            history,
            ao: level(0),
            output: level(1),
            hits,
            spread,
            mark: !ao,
            screen: screen.create_view(&Default::default()),
        }
    }

    /// The trace, after the depth prepass and before GTAO (whose denoise
    /// reads the cone's spread), the filtered levels and, without GTAO, the
    /// mark pass.
    pub fn trace<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &SsrTargets,
        time: &dyn Fn(&'static str) -> Option<wgpu::RenderPassTimestampWrites<'t>>,
    ) {
        super::gpu3d::mark_step("reflections");
        {
            let attachment = |view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d screen reflections"),
                color_attachments: &[attachment(&targets.output), attachment(&targets.hits)],
                timestamp_writes: time("screen reflections"),
                ..Default::default()
            });
            pass.set_pipeline(&self.trace_pipeline);
            pass.set_bind_group(0, &targets.trace_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(blur) = &targets.blur
            && !filter_skipped()
        {
            super::gpu3d::mark_step("reflection levels");
            let time = time("reflection blur");
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("3d reflection blur"),
                timestamp_writes: time.map(|t| wgpu::ComputePassTimestampWrites {
                    query_set: t.query_set,
                    beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                    end_of_pass_write_index: t.end_of_pass_write_index,
                }),
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &blur.group, &[]);
            pass.dispatch_workgroups(blur.workgroups.0, blur.workgroups.1, 1);
            drop(pass);
            for mip in 0..blur.levels.mip_level_count() {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &blur.levels,
                        mip_level: mip,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &blur.screen,
                        mip_level: mip + 2,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    blur.levels.size().mip_level_size(mip, wgpu::TextureDimension::D2),
                );
            }
        }
        super::gpu3d::mark_step("reflection spread");
        self.full_screen(
            encoder,
            "3d screen reflection spread",
            &self.fill_pipeline,
            &targets.fill_group,
            &targets.spread,
            None,
        );
        if targets.mark {
            super::gpu3d::mark_step("reflection mark");
            self.full_screen(
                encoder,
                "3d screen reflections mark",
                &self.mark_pipeline,
                &targets.mark_group,
                &targets.ao,
                None,
            );
        }
    }

    /// Keeps the opaque image for the next frame, after the opaque pass.
    pub fn keep(&self, encoder: &mut wgpu::CommandEncoder, targets: &SsrTargets) {
        super::gpu3d::mark_step("reflection history");
        self.full_screen(
            encoder,
            "3d screen reflections history",
            &self.copy_pipeline,
            &targets.copy_group,
            &targets.history,
            None,
        );
    }

    fn full_screen(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        pipeline: &wgpu::RenderPipeline,
        group: &wgpu::BindGroup,
        target: &wgpu::TextureView,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes,
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
impl Ssr {
    /// See `Renderer3D::check_mirrors`.
    pub fn check_mirrors(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.uniform_mirror
            .check(device, queue, &self.uniform, 0, "reflection settings");
    }
}

impl Ssr {
    /// Counts its uniform as other memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.buffer(C::Other, "reflection uniforms", &self.uniform);
    }
}

impl SsrTargets {
    /// Counts the reflections' targets and history as frame targets (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        for (label, view) in [
            ("reflection history", &self.history),
            ("reflection screen", &self.screen),
            ("reflection ao", &self.ao),
            ("reflection output", &self.output),
            ("reflection hits", &self.hits),
            ("reflection spread", &self.spread),
        ] {
            t.view(C::FrameTargets, label, view);
        }
        if let Some(blur) = &self.blur {
            t.texture(C::FrameTargets, "reflection blur levels", &blur.levels);
            t.texture(C::FrameTargets, "reflection blur screen", &blur.screen);
        }
    }
}
