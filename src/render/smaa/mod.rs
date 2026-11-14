//! SMAA reference implementation, adapted from smaa-rs 0.21 (MIT).
//! Lookup tables and reference shader: Jorge Jimenez et al. (MIT, headers retained).
use crate::render::traffic::{Counted, Upload};
mod shader;
use shader::{ShaderQuality, ShaderSource, ShaderStage};

mod area_tex;
use area_tex::*;

mod search_tex;
use search_tex::*;

use wgpu::util::DeviceExt;

struct BindGroupLayouts {
    edge_detect_bind_group_layout: wgpu::BindGroupLayout,
    blend_weight_bind_group_layout: wgpu::BindGroupLayout,
    neighborhood_blending_bind_group_layout: wgpu::BindGroupLayout,
}
struct Pipelines {
    edge_detect: wgpu::RenderPipeline,
    blend_weight: wgpu::RenderPipeline,
    neighborhood_blending: wgpu::RenderPipeline,
}
struct Resources {
    area_texture: wgpu::Texture,
    search_texture: wgpu::Texture,
    linear_sampler: wgpu::Sampler,
}
struct Targets {
    rt_uniforms: wgpu::Buffer,
    color_target: wgpu::TextureView,
    edges_target: wgpu::TextureView,
    blend_target: wgpu::TextureView,
}
struct BindGroups {
    edge_detect_bind_group: wgpu::BindGroup,
    blend_weight_bind_group: wgpu::BindGroup,
    neighborhood_blending_bind_group: wgpu::BindGroup,
}

impl BindGroupLayouts {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            edge_detect_bind_group_layout: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("smaa.bind_group_layout.edge_detect"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            }),
            blend_weight_bind_group_layout: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("smaa.bind_group_layout.blend_weight"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            }),
            neighborhood_blending_bind_group_layout: device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label: Some("smaa.bind_group_layout.neighborhood_blending"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                    ],
                },
            ),
        }
    }
}

impl Pipelines {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, layouts: &BindGroupLayouts) -> Self {
        let source = ShaderSource {
            quality: ShaderQuality::High,
        };

        let edge_detect_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smaa.pipeline_layout.edge_detect"),
            bind_group_layouts: &[Some(&layouts.edge_detect_bind_group_layout)],
            immediate_size: 0,
        });
        let edge_detect_shader_vert = wgpu::VertexState {
            module: &source.get_shader(device, ShaderStage::EdgeDetectionVS, "smaa.shader.edge_detect.vert"),
            entry_point: Some("main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };
        let edge_detect_shader_frag = wgpu::FragmentState {
            module: &source.get_shader(device, ShaderStage::LumaEdgeDetectionPS, "smaa.shader.edge_detect.frag"),
            entry_point: Some("main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rg8Unorm,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };
        let edge_detect = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smaa.pipeline.edge_detect"),
            layout: Some(&edge_detect_layout),
            vertex: edge_detect_shader_vert,
            fragment: Some(edge_detect_shader_frag),
            primitive: Default::default(),
            multisample: Default::default(),
            depth_stencil: None,
            multiview_mask: None,
            cache: None,
        });

        let blend_weight_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smaa.pipeline_layout.blend_weight"),
            bind_group_layouts: &[Some(&layouts.blend_weight_bind_group_layout)],
            immediate_size: 0,
        });
        let blend_weight_shader_vert = wgpu::VertexState {
            module: &source.get_shader(
                device,
                ShaderStage::BlendingWeightVS,
                "smaa.shader.blending_weight.vert",
            ),
            entry_point: Some("main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };
        let blend_weight_shader_frag = wgpu::FragmentState {
            module: &source.get_shader(
                device,
                ShaderStage::BlendingWeightPS,
                "smaa.shader.blending_weight.frag",
            ),
            entry_point: Some("main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };
        let blend_weight = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smaa.pipeline.blend_weight"),
            layout: Some(&blend_weight_layout),
            vertex: blend_weight_shader_vert,
            fragment: Some(blend_weight_shader_frag),
            primitive: Default::default(),
            multisample: Default::default(),
            depth_stencil: None,
            multiview_mask: None,
            cache: None,
        });

        let neighborhood_blending_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smaa.pipeline_layout.neighborhood_blending"),
            bind_group_layouts: &[Some(&layouts.neighborhood_blending_bind_group_layout)],
            immediate_size: 0,
        });
        let neighborhood_blending_vert = wgpu::VertexState {
            module: &source.get_shader(
                device,
                ShaderStage::NeighborhoodBlendingVS,
                "smaa.shader.neighborhood_blending.vert",
            ),
            entry_point: Some("main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };
        let neighborhood_blending_frag = wgpu::FragmentState {
            module: &source.get_shader(
                device,
                ShaderStage::NeighborhoodBlendingPS,
                "smaa.shader.neighborhood_blending.frag",
            ),
            entry_point: Some("main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };
        let neighborhood_blending = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smaa.pipeline.neighborhood_blending"),
            layout: Some(&neighborhood_blending_layout),
            vertex: neighborhood_blending_vert,
            fragment: Some(neighborhood_blending_frag),
            primitive: Default::default(),
            multisample: Default::default(),
            depth_stencil: None,
            multiview_mask: None,
            cache: None,
        });

        Self {
            edge_detect,
            blend_weight,
            neighborhood_blending,
        }
    }
}
impl Targets {
    pub fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture_desc = wgpu::TextureDescriptor {
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            label: None,
            view_formats: &[],
        };

        let mut uniform_data = Vec::new();
        for f in &[1.0 / width as f32, 1.0 / height as f32, width as f32, height as f32] {
            uniform_data.extend_from_slice(&f.to_ne_bytes());
        }
        let rt_uniforms = device
            .uploading(Upload::Uniforms)
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("smaa.uniforms"),
                usage: wgpu::BufferUsages::UNIFORM,
                contents: &uniform_data,
            });

        Self {
            rt_uniforms,
            color_target: device
                .create_texture(&wgpu::TextureDescriptor { format, ..texture_desc })
                .create_view(&wgpu::TextureViewDescriptor {
                    label: Some("smaa.color_target.view"),
                    ..Default::default()
                }),
            edges_target: device
                .create_texture(&wgpu::TextureDescriptor {
                    format: wgpu::TextureFormat::Rg8Unorm,
                    label: Some("smaa.texture.edge_target"),
                    ..texture_desc
                })
                .create_view(&wgpu::TextureViewDescriptor {
                    label: Some("smaa.texture_view.edge_target"),
                    ..Default::default()
                }),

            blend_target: device
                .create_texture(&wgpu::TextureDescriptor {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    label: Some("smaa.texture.blend_target"),
                    ..texture_desc
                })
                .create_view(&wgpu::TextureViewDescriptor {
                    label: Some("smaa.texture_view.blend_target"),
                    ..Default::default()
                }),
        }
    }
}
impl Resources {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let area_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("smaa.texture.area"),
                size: wgpu::Extent3d {
                    width: AREATEX_WIDTH,
                    height: AREATEX_HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &AREATEX_BYTES,
        );

        let search_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("smaa.texture.search"),
                size: wgpu::Extent3d {
                    width: SEARCHTEX_WIDTH,
                    height: SEARCHTEX_HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &SEARCHTEX_BYTES,
        );

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("smaa.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            area_texture,
            search_texture,
            linear_sampler,
        }
    }
}

impl BindGroups {
    fn new(device: &wgpu::Device, layouts: &BindGroupLayouts, resources: &Resources, targets: &Targets) -> Self {
        Self {
            edge_detect_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("smaa.bind_group.edge_detect"),
                layout: &layouts.edge_detect_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&resources.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &targets.rt_uniforms,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&targets.color_target),
                    },
                ],
            }),

            blend_weight_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("smaa.bind_group.blend_weight"),
                layout: &layouts.blend_weight_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&resources.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &targets.rt_uniforms,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&targets.edges_target),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(
                            &resources.area_texture.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(
                            &resources.search_texture.create_view(&Default::default()),
                        ),
                    },
                ],
            }),
            neighborhood_blending_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("smaa.bind_group.neighborhood_blending"),
                layout: &layouts.neighborhood_blending_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&resources.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &targets.rt_uniforms,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&targets.color_target),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&targets.blend_target),
                    },
                ],
            }),
        }
    }
}

pub(super) struct Smaa {
    pipelines: Pipelines,
    /// Kept for the GPU memory report: the bind groups hold its textures.
    resources: Resources,
    targets: Targets,
    groups: BindGroups,
}
impl Smaa {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Self {
        let layouts = BindGroupLayouts::new(device);
        let pipelines = Pipelines::new(device, format, &layouts);
        let resources = Resources::new(device, queue);
        let targets = Targets::new(device, width, height, format);
        let groups = BindGroups::new(device, &layouts, &resources, &targets);
        Self {
            pipelines,
            resources,
            targets,
            groups,
        }
    }
    /// Counts its targets as frame targets and its lookup textures as
    /// other memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        let targets = &self.targets;
        for view in [&targets.color_target, &targets.edges_target, &targets.blend_target] {
            t.view(C::FrameTargets, view);
        }
        t.buffer(C::Other, &targets.rt_uniforms);
        t.texture(C::Other, &self.resources.area_texture);
        t.texture(C::Other, &self.resources.search_texture);
    }
    pub fn input(&self) -> &wgpu::TextureView {
        &self.targets.color_target
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        for (label, pipeline, group, target) in [
            (
                "SMAA edges",
                &self.pipelines.edge_detect,
                &self.groups.edge_detect_bind_group,
                &self.targets.edges_target,
            ),
            (
                "SMAA weights",
                &self.pipelines.blend_weight,
                &self.groups.blend_weight_bind_group,
                &self.targets.blend_target,
            ),
            (
                "SMAA neighborhood",
                &self.pipelines.neighborhood_blending,
                &self.groups.neighborhood_blending_bind_group,
                output,
            ),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
