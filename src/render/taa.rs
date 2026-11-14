//! Temporal antialiasing of the lit HDR frame (see `taa.wgsl`): the sub-pixel
//! jitter sequence, the history images, the motion vector target and the
//! resolve pass.
//!
//! The resolve reads an *input* image (the frame as the 3D passes resolved
//! it), the history and the motion vectors, and writes an *output*: the new
//! history and the frame post processing goes on with. Input and output sizes
//! are separate numbers throughout, so a lower-resolution input can later
//! be resolved up to the output's size; for now the renderer uses one size
//! for both.

use crate::render::traffic::{NewBuffer, Upload};
use glam::Mat4;

use super::gpu3d::HDR_FORMAT;

/// How many positions the jitter visits before it repeats.
pub(super) const JITTER_SAMPLES: u32 = 16;

/// Object-only backward UV motion, added to depth-derived camera motion.
/// Zero for static surfaces; MSAA resolves coverage without a validity channel.
pub(super) const VELOCITY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// The `index`th number of the Halton sequence in `base`, in 0..1.
pub(super) fn halton(mut index: u32, base: u32) -> f32 {
    let (mut result, mut f) = (0.0, 1.0 / base as f32);
    while index > 0 {
        result += f * (index % base) as f32;
        index /= base;
        f /= base as f32;
    }
    result
}

/// Frame `frame`'s jitter in pixels, each in -0.5..0.5: Halton (2, 3), the
/// sequence's zeroth element skipped.
pub(super) fn jitter(frame: u32) -> [f32; 2] {
    let i = frame % JITTER_SAMPLES + 1;
    [halton(i, 2) - 0.5, halton(i, 3) - 0.5]
}

/// `view_proj` with the sub-pixel `jitter` (pixels, +x right, +y down) of a
/// `size` image applied after projection.
pub(super) fn jittered(view_proj: Mat4, jitter: [f32; 2], size: (u32, u32)) -> Mat4 {
    let ndc = glam::Vec3::new(
        2.0 * jitter[0] / size.0.max(1) as f32,
        -2.0 * jitter[1] / size.1.max(1) as f32,
        0.0,
    );
    Mat4::from_translation(ndc) * view_proj
}

/// What the resolve shader reads (`Taa` in `taa.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TaaUniform {
    inv_view_proj: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    previous_view_proj: [[f32; 4]; 4],
    input_size: [f32; 4],
    output_size: [f32; 4],
    params: [f32; 4],
}

/// One frame's resolve settings.
pub(super) struct Resolve {
    /// The jittered camera (the one the frame was drawn with), unjittered
    /// and the unjittered one of the last frame.
    pub jittered: Mat4,
    pub current: Mat4,
    pub previous: Mat4,
    pub input_size: (u32, u32),
    pub output_size: (u32, u32),
    pub sharpness: f32,
    /// Ignore the history (first frame, camera cut, resize).
    pub reset: bool,
}

/// How much of a still frame is blended into the history each frame.
const STILL_BLEND: f32 = 0.1;

/// The resolve pass and its pipeline.
pub(super) struct Taa {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    /// What `uniform` holds: only what changed is uploaded.
    uniform_mirror: crate::render::traffic::Mirror,
    sampler: wgpu::Sampler,
}

/// The images temporal antialiasing needs, as big as the window.
pub(super) struct TaaTargets {
    /// The lit frame the 3D passes resolve into (instead of `hdr`).
    pub input: wgpu::Texture,
    pub input_view: wgpu::TextureView,
    history: [wgpu::TextureView; 2],
    /// `groups[i]` reads history `i`.
    groups: [wgpu::BindGroup; 2],
}

impl TaaTargets {
    /// A history image: for checking it against what the frame's passes
    /// declare.
    pub fn history(&self) -> &wgpu::TextureView {
        &self.history[0]
    }
}

/// Shared by TAA and temporal AO, without allocating TAA color histories.
pub(super) struct VelocityTargets {
    /// Multisampled motion of moved models, resolved into `velocity`.
    pub velocity_color: wgpu::TextureView,
    velocity: wgpu::TextureView,
    #[cfg(test)]
    pub velocity_texture: wgpu::Texture,
}
impl VelocityTargets {
    pub fn velocity(&self) -> &wgpu::TextureView {
        &self.velocity
    }
    pub fn new(device: &wgpu::Device, size: (u32, u32), samples: u32) -> Self {
        let make = |samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("object velocity"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: VELOCITY_FORMAT,
                usage,
                view_formats: &[],
            })
        };
        let velocity_texture = make(
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let velocity = velocity_texture.create_view(&Default::default());
        let velocity_color = make(samples, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        Self {
            velocity,
            velocity_color,
            #[cfg(test)]
            velocity_texture,
        }
    }
}

impl Taa {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = |binding, depth| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: if depth {
                    wgpu::TextureSampleType::Depth
                } else {
                    wgpu::TextureSampleType::Float { filterable: true }
                },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: depth,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("TAA layout"),
            entries: &[
                texture(0, false),
                texture(1, false),
                texture(2, false),
                texture(3, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
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
        let shader = device.create_shader_module(wgpu::include_wgsl!("taa.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("TAA pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let target = Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("TAA resolve"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_resolve"),
                compilation_options: Default::default(),
                targets: &[target.clone(), target],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("TAA"),
            size: size_of::<TaaUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | crate::render::traffic::MIRRORED,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("TAA history"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            pipeline,
            uniform,
            uniform_mirror: Default::default(),
            sampler,
        }
    }

    /// Images for a `size` window, reading `depth` (the multisampled depth
    /// the frame was drawn with).
    pub fn targets(
        &self,
        device: &wgpu::Device,
        size: (u32, u32),
        velocity: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) -> TaaTargets {
        let make = |label, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let render = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let bound = render | wgpu::TextureUsages::TEXTURE_BINDING;
        let input = make("TAA input", HDR_FORMAT, 1, bound);
        let input_view = input.create_view(&Default::default());
        let history = [
            make("TAA history 0", HDR_FORMAT, 1, bound).create_view(&Default::default()),
            make("TAA history 1", HDR_FORMAT, 1, bound).create_view(&Default::default()),
        ];
        let group = |history: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("TAA"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&input_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(history),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(velocity),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.uniform.as_entire_binding(),
                    },
                ],
            })
        };
        let groups = [group(&history[0]), group(&history[1])];
        TaaTargets {
            input,
            input_view,
            history,
            groups,
        }
    }

    pub fn prepare(&mut self, queue: &wgpu::Queue, resolve: &Resolve) {
        let size = |(w, h): (u32, u32)| {
            let (w, h) = (w.max(1) as f32, h.max(1) as f32);
            [w, h, 1.0 / w, 1.0 / h]
        };
        self.uniform_mirror.upload(
            queue,
            Upload::Uniforms,
            &self.uniform,
            0,
            bytemuck::bytes_of(&TaaUniform {
                inv_view_proj: resolve.jittered.inverse().to_cols_array_2d(),
                view_proj: resolve.current.to_cols_array_2d(),
                previous_view_proj: resolve.previous.to_cols_array_2d(),
                input_size: size(resolve.input_size),
                output_size: size(resolve.output_size),
                params: [
                    resolve.sharpness.clamp(0.0, 1.0),
                    STILL_BLEND,
                    f32::from(resolve.reset),
                    0.0,
                ],
            }),
        );
    }

    /// Resolves the input and `read` history into history `1 - read` and
    /// `hdr`.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &TaaTargets,
        read: usize,
        hdr: &wgpu::TextureView,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("TAA resolve"),
            color_attachments: &[attachment(&targets.history[1 - read]), attachment(hdr)],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &targets.groups[read], &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
impl Taa {
    /// See `Renderer3D::check_mirrors`.
    pub fn check_mirrors(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.uniform_mirror
            .check(device, queue, &self.uniform, 0, "antialiasing settings");
    }
}

impl Taa {
    /// Counts its uniform as other memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.buffer(C::Other, &self.uniform);
    }
}

impl TaaTargets {
    /// Counts the lit input and both histories as frame targets (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.texture(C::FrameTargets, &self.input);
        for view in &self.history {
            t.view(C::FrameTargets, view);
        }
    }
}

impl VelocityTargets {
    /// Counts the motion vectors as frame targets (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.view(C::FrameTargets, &self.velocity_color);
        t.view(C::FrameTargets, &self.velocity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halton_covers_the_pixel_without_repeating() {
        let points: Vec<[f32; 2]> = (0..JITTER_SAMPLES).map(jitter).collect();
        for p in &points {
            assert!(p[0].abs() < 0.5 && p[1].abs() < 0.5, "{p:?}");
        }
        for (i, a) in points.iter().enumerate() {
            for b in &points[i + 1..] {
                assert!(a != b);
            }
        }
        let mean = points.iter().fold([0.0, 0.0], |m, p| [m[0] + p[0], m[1] + p[1]]);
        assert!(mean[0].abs() / (JITTER_SAMPLES as f32) < 0.05, "{mean:?}");
        assert_eq!(jitter(0), jitter(JITTER_SAMPLES));
        assert_eq!(halton(1, 2), 0.5);
        assert_eq!(halton(3, 2), 0.75);
    }

    #[test]
    fn the_resolve_shader_is_valid() {
        super::super::gpu3d::check_shader("taa.wgsl", include_str!("taa.wgsl")).unwrap();
    }

    #[test]
    fn jitter_moves_the_image_by_the_requested_pixels() {
        let view_proj = glam::camera::rh::proj::directx::perspective(1.0, 1.0, 0.1, 10.0);
        let size = (100, 50);
        let p = glam::Vec3::new(0.3, -0.2, -3.0);
        let before = view_proj.project_point3(p);
        let after = jittered(view_proj, [0.5, -0.25], size).project_point3(p);
        // +x is right: half a pixel is a hundredth of the width in NDC.
        assert!((after.x - before.x - 2.0 * 0.5 / 100.0).abs() < 1e-6);
        // +y is down in pixels, up in NDC: -0.25 px is 0.25 / 50 * 2 up.
        assert!((after.y - before.y - 2.0 * 0.25 / 50.0).abs() < 1e-6);
        assert!((after.z - before.z).abs() < 1e-6);
    }
}
