//! Mip chains of mipmapped render targets
//! (`Graphics::create_mipmapped_render_target`): each level below the first
//! is the one above it halved (a bilinear sample in the middle of every 2x2
//! block, as `mips.rs` does for material textures), made again on the GPU
//! whenever something wrote the first level.
//!
//! When: in the target camera stage of a frame, for every mipmapped target
//! written since its levels were last made: by this frame's compute passes
//! or target cameras (and their screen effects), or after that stage last
//! frame (target effects, UI panels). Materials read the levels from the
//! main view on.
//!
//! Each target's views and bind groups are made the first time its levels
//! are, and kept: a frame that makes them again allocates nothing.

use super::HDR_FORMAT;

/// The pipeline every chain is drawn with.
pub(super) struct MipMaker {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

/// One target's levels: for each level below the first, the bind group
/// reading the level above and the view of the level itself.
pub(super) struct MipChain {
    levels: Vec<(wgpu::BindGroup, wgpu::TextureView)>,
}

impl MipMaker {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render target mipmaps"),
            source: wgpu::ShaderSource::Wgsl(crate::render::mips::SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render target mipmaps"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render target mipmaps"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render target mipmaps"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout,
            pipeline,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("render target mipmaps"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    /// The views and bind groups that make `texture`'s levels.
    pub(super) fn chain(&self, device: &wgpu::Device, texture: &wgpu::Texture) -> MipChain {
        let level = |level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("render target mip level"),
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let levels = (1..texture.mip_level_count())
            .map(|i| {
                let above = level(i - 1);
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("render target mipmaps"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&above),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                (group, level(i))
            })
            .collect();
        MipChain { levels }
    }

    /// Records the passes that make every level of `chain` from the one
    /// above, top down.
    pub(super) fn record(&self, encoder: &mut wgpu::CommandEncoder, chain: &MipChain) {
        for (group, view) in &chain.levels {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render target mipmaps"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Every texel is drawn.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

pub(super) use crate::render::gpu_textures::first_level;
