//! The sky kit's GPU passes (see `SkyKit`): the cloud shadow map, and the
//! volumetric clouds' march, temporal resolve and composite. Made the first
//! frame the kit is on (the volumetric half the first frame a volumetric
//! preset is), so a game without the kit has none of it.
//!
//! - **Cloud shadows** (`sky_kit_shadow.wgsl`): each frame, a 128 x 128
//!   square of the IBL atlas, rows 1408 on (below the probes' maps), gets
//!   how much sunlight passes the cloud layer at each point of its base.
//!   The atlas is already every lit pass's `sky_texture`, so material
//!   shaders read the map (`cloud_light` in mesh.wgsl) without a binding of
//!   their own.
//! - **Volumetric clouds** (`sky_kit_clouds.wgsl`): the march at half the
//!   frame's width and height into `trace`, the resolve of that with last
//!   frame's result into one of the two `history` images (they swap each
//!   frame), and the composite of the resolved image over the sky's pixels
//!   in the opaque pass, right after the sky. The interleaved presets
//!   (`CloudQuality::interleaved`) march a quarter of the history's pixels
//!   each frame: `trace` is half its width and height, and the resolve
//!   fills in the other three of each 2 x 2 block from their history.
//!   No upsample needs the depth: the march never stops at geometry (it
//!   traces every pixel to the sky), and the composite's depth test keeps
//!   the clouds to the sky's pixels at the frame's full size.
//!
//! The kit's sky itself is a sky shader (`sky_kit.wgsl`), compiled and
//! held with the game's sky shaders (`gpu3d/sky_shaders.rs`).

use glam::{Mat4, Vec3};

use crate::render::traffic::{Counted, Mirror, Upload};
use crate::three::{CLOUD_SHADOW_TEXELS, CloudQuality};

use super::gpu3d::HDR_FORMAT;

/// The cloud model all the kit's shaders share.
pub(super) const SKY_KIT_COMMON: &str = include_str!("sky_kit_common.wgsl");
/// The kit's sky shader (its `sky` function).
const SKY_KIT: &str = include_str!("sky_kit.wgsl");
const CLOUDS: &str = include_str!("sky_kit_clouds.wgsl");
const SHADOW: &str = include_str!("sky_kit_shadow.wgsl");
/// The air lookup's entry point, composed after the kit's sky shader.
pub(super) const AIR_MAIN: &str = include_str!("sky_kit_air.wgsl");

/// Where the cloud shadow map is in the IBL atlas (`CLOUD_SHADOW_ORIGIN` in
/// mesh.wgsl and sky_kit_shadow.wgsl): x, y, side.
pub(super) const SHADOW_RECT: [u32; 3] = [0, 1408, CLOUD_SHADOW_TEXELS];

/// Where the kit's air lookup (the sky texture's layout, 256 x 128) is in
/// the IBL atlas, beside the shadow map (`KIT_AIR_ORIGIN` in sky_kit.wgsl).
pub(super) const AIR_ORIGIN: [u32; 2] = [CLOUD_SHADOW_TEXELS, 1408];

/// The kit's sky shader as the game's code of a sky shader (what
/// `compose_sky` takes).
pub(crate) fn sky_source() -> String {
    format!("{SKY_KIT_COMMON}\n{SKY_KIT}")
}

/// The cloud passes' code, as the game's code of a sky shader: composed with
/// `compose_sky`, it has the mesh prelude, the sky helpers and the kit's sky
/// before it (and the sky shader's entry points after, unused).
pub(crate) fn clouds_source() -> String {
    format!("{SKY_KIT_COMMON}\n{SKY_KIT}\n{CLOUDS}")
}

/// The cloud shadow map's code (it stands alone: no frame, no prelude).
pub(crate) fn shadow_source() -> String {
    format!("{SKY_KIT_COMMON}\n{SHADOW}")
}

/// The cloud shadow pass's uniform (`CloudShadow` in sky_kit_shadow.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ShadowUniform {
    kit: [[f32; 4]; 6],
    sun: [f32; 4],
}

/// The volumetric passes' uniform (`CloudPass` in sky_kit_clouds.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassUniform {
    previous: [[f32; 4]; 4],
    params: [f32; 4],
    size: [f32; 4],
    step: [f32; 4],
}

/// The cloud image's size for a frame of `size`: half its width and height.
pub(super) fn cloud_size(size: (u32, u32)) -> (u32, u32) {
    (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1))
}

/// The trace's size for a cloud image of `size`: the same, or interleaved
/// half its width and height (one pixel of each 2 x 2 block a frame).
pub(super) fn trace_size(size: (u32, u32), interleaved: bool) -> (u32, u32) {
    if interleaved {
        (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1))
    } else {
        size
    }
}

/// Which pixel of each 2 x 2 block an interleaved trace marches on frame
/// `frame`: all four in turn, the diagonal ones first.
pub(super) fn interleave_step(frame: u32) -> (u32, u32) {
    [(0, 0), (1, 1), (1, 0), (0, 1)][(frame % 4) as usize]
}

/// What the passes are made from, from the 3D renderer.
pub(super) struct Sources<'a> {
    /// The frame bind group's layout (group 0 of the cloud passes).
    pub frame_layout: &'a wgpu::BindGroupLayout,
    /// Adapts a whole sky shader module to the frame (its samples and
    /// depth reads): `for_samples(compose_sky(code))`.
    pub compose: &'a dyn Fn(&str) -> String,
    /// The frame's samples per pixel and depth format, for the composite.
    pub samples: u32,
    pub depth_format: wgpu::TextureFormat,
}

pub(super) struct Clouds {
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_buffer: wgpu::Buffer,
    shadow_mirror: Mirror,
    shadow_group: wgpu::BindGroup,
    /// The volumetric passes, once a volumetric preset was used.
    volume: Option<Volume>,
    /// This frame: whether the shadow map is drawn and whether the
    /// volumetric passes run (main view only).
    pub shadows: bool,
    pub volumetric: bool,
    /// Tests: march every pixel each frame whatever the preset (the
    /// presets' cost before the interleaved march).
    #[cfg(test)]
    pub full_march: bool,
}

struct Volume {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    trace: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    buffer: wgpu::Buffer,
    mirror: Mirror,
    images: Option<Images>,
    /// Last frame's camera, while the history holds its clouds.
    previous: Option<Mat4>,
    /// Frames drawn: moves the march's offsets on.
    frames: u32,
    /// The history image this frame resolves into (the other is last frame's).
    current: usize,
    /// The preset the history was drawn with.
    quality: CloudQuality,
}

struct Images {
    size: (u32, u32),
    /// Whether the trace is interleaved (a quarter of the history).
    interleaved: bool,
    trace: wgpu::Texture,
    history: [wgpu::Texture; 2],
    /// The march's group (it reads neither image).
    trace_group: wgpu::BindGroup,
    /// Resolving into history `i`: the trace and history `1 - i`.
    resolve_groups: [wgpu::BindGroup; 2],
    /// Compositing history `i`.
    composite_groups: [wgpu::BindGroup; 2],
}

fn uniform_buffer(device: &wgpu::Device, label: &'static str, size: usize) -> wgpu::Buffer {
    device
        .uploading(Upload::Uniforms)
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: &vec![0; size],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | crate::render::traffic::MIRRORED,
        })
}

/// A full-screen pass's pipeline.
#[allow(clippy::too_many_arguments)]
fn full_screen(
    device: &wgpu::Device,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    (vertex, fragment): (&str, &str),
    blend: Option<wgpu::BlendState>,
    depth: Option<wgpu::TextureFormat>,
    samples: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        // The composite: behind everything, only where the depth is still
        // the far plane (0, reverse-Z), as the sky.
        depth_stencil: depth.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The composite's blend: the clouds' light added over the sky, which shows
/// through by their alpha (how much of it they let through).
const OVER_SKY: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::SrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

impl Clouds {
    /// The shadow pass (what every preset needs).
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cloud shadows"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cloud shadows"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cloud shadows"),
            source: wgpu::ShaderSource::Wgsl(shadow_source().into()),
        });
        let shadow_pipeline = full_screen(
            device,
            "cloud shadows",
            &pipeline_layout,
            &module,
            ("vs_cloud_shadow", "fs_cloud_shadow"),
            None,
            None,
            1,
        );
        let shadow_buffer = uniform_buffer(device, "cloud shadows", size_of::<ShadowUniform>());
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cloud shadows"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: shadow_buffer.as_entire_binding(),
            }],
        });
        Self {
            shadow_pipeline,
            shadow_buffer,
            shadow_mirror: Mirror::default(),
            shadow_group,
            volume: None,
            shadows: false,
            volumetric: false,
            #[cfg(test)]
            full_march: false,
        }
    }

    /// This frame's cloud shadow settings: the kit's (`frame_params`) and
    /// the direction toward the sun.
    pub fn prepare_shadows(&mut self, queue: &wgpu::Queue, kit: [[f32; 4]; 6], to_sun: Vec3) {
        let uniform = ShadowUniform {
            kit,
            sun: to_sun.extend(0.0).to_array(),
        };
        self.shadow_mirror.upload(
            queue,
            Upload::Uniforms,
            &self.shadow_buffer,
            0,
            bytemuck::bytes_of(&uniform),
        );
    }

    /// Draws the cloud shadow map into its square of `atlas`.
    pub fn record_shadows<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        atlas: &wgpu::Texture,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'t>>,
    ) {
        let view = atlas.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cloud shadows"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let [x, y, side] = SHADOW_RECT;
        pass.set_viewport(x as f32, y as f32, side as f32, side as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, side, side);
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_bind_group(0, &self.shadow_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Makes the volumetric passes, the first time; rebuilds the composite
    /// when the frame's samples or depth format changed.
    pub fn ensure_volume(&mut self, device: &wgpu::Device, sources: &Sources<'_>) {
        if self.volume.is_some() {
            return;
        }
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("clouds"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture(2),
                texture(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("clouds"),
            bind_group_layouts: &[Some(sources.frame_layout), None, None, Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clouds"),
            source: wgpu::ShaderSource::Wgsl((sources.compose)(&clouds_source()).into()),
        });
        let pass = |label, entry| {
            full_screen(
                device,
                label,
                &pipeline_layout,
                &module,
                ("vs_far", entry),
                None,
                None,
                1,
            )
        };
        let trace = pass("cloud trace", "fs_cloud_trace");
        let resolve = pass("cloud resolve", "fs_cloud_resolve");
        let composite = full_screen(
            device,
            "cloud composite",
            &pipeline_layout,
            &module,
            ("vs_far", "fs_cloud_composite"),
            Some(OVER_SKY),
            Some(sources.depth_format),
            sources.samples,
        );
        self.volume = Some(Volume {
            layout,
            pipeline_layout,
            module,
            trace,
            resolve,
            composite,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("clouds"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            buffer: uniform_buffer(device, "clouds", size_of::<PassUniform>()),
            mirror: Mirror::default(),
            images: None,
            previous: None,
            frames: 0,
            current: 0,
            quality: CloudQuality::Medium,
        });
    }

    /// Builds the composite again for the frame's new samples or depth format.
    pub fn rebuild_composite(&mut self, device: &wgpu::Device, samples: u32, depth_format: wgpu::TextureFormat) {
        if let Some(v) = &mut self.volume {
            v.composite = full_screen(
                device,
                "cloud composite",
                &v.pipeline_layout,
                &v.module,
                ("vs_far", "fs_cloud_composite"),
                Some(OVER_SKY),
                Some(depth_format),
                samples,
            );
        }
    }

    /// Forgets the clouds' history: the next frame starts it over.
    pub fn forget(&mut self) {
        if let Some(v) = &mut self.volume {
            v.previous = None;
        }
    }

    /// This frame's volumetric settings, for a frame of `size` seen through
    /// `view_proj` (without jitter): sizes the images, swaps the history.
    pub fn prepare_volume(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        view_proj: Mat4,
        quality: CloudQuality,
    ) {
        #[cfg(test)]
        let full_march = self.full_march;
        #[cfg(not(test))]
        let full_march = false;
        let Some(v) = &mut self.volume else { return };
        let size = cloud_size(size);
        let interleaved = quality.interleaved() && !full_march;
        if v.images
            .as_ref()
            .is_none_or(|i| i.size != size || i.interleaved != interleaved)
        {
            v.images = Some(Images::new(device, v, size, interleaved));
            v.previous = None;
        }
        if v.quality != quality {
            v.quality = quality;
            v.previous = None;
        }
        v.current = 1 - v.current;
        v.frames = v.frames.wrapping_add(1);
        let previous = v.previous.replace(view_proj);
        let (steps, light_steps) = quality.steps();
        let uniform = PassUniform {
            previous: previous.unwrap_or(view_proj).to_cols_array_2d(),
            params: [
                steps as f32,
                light_steps as f32,
                (v.frames % 1024) as f32,
                f32::from(previous.is_some()),
            ],
            size: [size.0 as f32, size.1 as f32, 1.0 / size.0 as f32, 1.0 / size.1 as f32],
            step: {
                let (x, y) = interleave_step(v.frames);
                [x as f32, y as f32, f32::from(interleaved), 0.0]
            },
        };
        v.mirror
            .upload(queue, Upload::Uniforms, &v.buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// The march and the resolve, each its own timed pass.
    pub fn record_volume<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame_group: &wgpu::BindGroup,
        time: &dyn Fn(&'static str) -> Option<wgpu::RenderPassTimestampWrites<'t>>,
    ) {
        let Some((v, images)) = self.volume.as_ref().and_then(|v| Some((v, v.images.as_ref()?))) else {
            return;
        };
        let steps = [
            ("cloud trace", "clouds", &images.trace, &v.trace, &images.trace_group),
            (
                "cloud resolve",
                "cloud resolve",
                &images.history[v.current],
                &v.resolve,
                &images.resolve_groups[v.current],
            ),
        ];
        for (label, timer, target, pipeline, group) in steps {
            super::gpu3d::mark_step(timer);
            let view = target.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: time(timer),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, frame_group, &[]);
            pass.set_bind_group(3, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Draws the resolved clouds over the sky, inside the opaque pass (its
    /// frame bind group is set at group 0).
    pub fn composite(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((v, images)) = self.volume.as_ref().and_then(|v| Some((v, v.images.as_ref()?))) else {
            return;
        };
        pass.set_pipeline(&v.composite);
        pass.set_bind_group(3, &images.composite_groups[v.current], &[]);
        pass.draw(0..3, 0..1);
    }

    /// Whether the volumetric passes and their images are made (they are
    /// drawn this frame when `volumetric` is set too).
    pub fn has_volume(&self) -> bool {
        self.volume.as_ref().is_some_and(|v| v.images.is_some())
    }

    /// The cloud images, for the frame checks: the trace and the history
    /// this frame resolves into.
    pub fn images(&self) -> Option<(&wgpu::Texture, &wgpu::Texture)> {
        let v = self.volume.as_ref()?;
        let images = v.images.as_ref()?;
        Some((&images.trace, &images.history[v.current]))
    }

    /// Counts its images and uniforms (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.buffer(C::Other, "cloud shadow uniforms", &self.shadow_buffer);
        if let Some(v) = &self.volume {
            t.buffer(C::Other, "cloud uniforms", &v.buffer);
            if let Some(images) = &v.images {
                t.texture(C::FrameTargets, "cloud trace", &images.trace);
                for history in &images.history {
                    t.texture(C::FrameTargets, "cloud history", history);
                }
            }
        }
    }
}

#[cfg(test)]
impl Clouds {
    /// See `Renderer3D::check_mirrors`.
    pub fn check_mirrors(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.shadow_mirror
            .check(device, queue, &self.shadow_buffer, 0, "cloud shadow settings");
        if let Some(v) = &self.volume {
            v.mirror.check(device, queue, &v.buffer, 0, "cloud pass settings");
        }
    }
}

impl Images {
    fn new(device: &wgpu::Device, v: &Volume, size: (u32, u32), interleaved: bool) -> Self {
        let image = |label, (width, height): (u32, u32)| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | if cfg!(test) {
                        wgpu::TextureUsages::COPY_SRC
                    } else {
                        wgpu::TextureUsages::empty()
                    },
                view_formats: &[],
            })
        };
        let trace = image("cloud trace", trace_size(size, interleaved));
        let history = [image("cloud history", size), image("cloud history", size)];
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let (trace_view, history_views) = (view(&trace), [view(&history[0]), view(&history[1])]);
        let group = |a: &wgpu::TextureView, b: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("clouds"),
                layout: &v.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: v.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&v.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(a),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(b),
                    },
                ],
            })
        };
        Self {
            size,
            interleaved,
            trace_group: group(&history_views[0], &history_views[1]),
            resolve_groups: [
                group(&trace_view, &history_views[1]),
                group(&trace_view, &history_views[0]),
            ],
            composite_groups: [
                group(&history_views[0], &trace_view),
                group(&history_views[1], &trace_view),
            ],
            trace,
            history,
        }
    }
}
