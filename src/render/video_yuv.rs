//! A video frame's Y'CbCr planes to RGBA on the GPU (feature `video`).
//!
//! The decoder gives three 8-bit planes (luma, and two chroma planes that
//! are usually half as wide and high). Converting them on the CPU writes
//! four bytes for every pixel and does the matrix math for each; here the
//! planes are uploaded as they are (1.5 bytes a pixel for 4:2:0) and a
//! full-screen triangle does the math, reading the chroma sample the CPU
//! converter reads (nearest, no filtering), with the same matrix and
//! range. It draws into a scratch `Rgba8Unorm` texture of gamma-coded
//! values, which is copied into the video's sprite texture (sRGB, the same
//! bytes): no round trip through linear values, so the bytes match the CPU
//! converter's to within a step of rounding.
//!
//! One converter serves every video; it keeps one set of plane textures
//! per frame size.

use std::collections::HashMap;

use crate::render::traffic::{self, Counted, Upload};
use crate::video::yuv::{Planes, Subsampling, coefficients_f32};

const SHADER: &str = r"
struct Params {
    // r = l + rv * v, g = l - gu * u - gv * v, b = l + bu * u, with
    // l = (y - y_offset) * y_scale, in 0-255 code values.
    y_scale: f32,
    y_offset: f32,
    rv: f32,
    gu: f32,
    gv: f32,
    bu: f32,
    // Chroma columns and rows per luma one, as shifts (4:2:0: 1 and 1).
    shift_x: u32,
    shift_y: u32,
};
@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var cb: texture_2d<f32>;
@group(0) @binding(2) var cr: texture_2d<f32>;
@group(0) @binding(3) var<uniform> p: Params;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

@fragment fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let xy = vec2<u32>(at.xy);
    let c = min(vec2<u32>(xy.x >> p.shift_x, xy.y >> p.shift_y), textureDimensions(cb) - vec2<u32>(1u));
    let l = (textureLoad(luma, xy, 0).r * 255.0 - p.y_offset) * p.y_scale;
    let u = textureLoad(cb, c, 0).r * 255.0 - 128.0;
    let v = textureLoad(cr, c, 0).r * 255.0 - 128.0;
    let rgb = vec3<f32>(l + p.rv * v, l - p.gu * u - p.gv * v, l + p.bu * u) / 255.0;
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
";

/// The textures one frame size needs.
struct Set {
    luma: wgpu::Texture,
    cb: wgpu::Texture,
    cr: wgpu::Texture,
    params: wgpu::Buffer,
    scratch: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// Converts decoded frames into RGBA textures on the GPU.
pub(crate) struct YuvToRgba {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// By luma and chroma size.
    sets: HashMap<[u32; 4], Set>,
}

/// A chroma plane's size: none (a gray picture) is one neutral sample.
fn chroma_size(planes: &Planes<'_>) -> (u32, u32) {
    let (w, h) = (planes.width as u32, planes.height as u32);
    match planes.subsampling {
        Subsampling::Both => (w.div_ceil(2), h.div_ceil(2)),
        Subsampling::Horizontal => (w.div_ceil(2), h),
        Subsampling::None => (w, h),
        Subsampling::Gray => (1, 1),
    }
}

impl YuvToRgba {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video yuv to rgba"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let plane = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video yuv to rgba"),
            entries: &[
                plane(0),
                plane(1),
                plane(2),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video yuv to rgba"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video yuv to rgba"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
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
            sets: HashMap::new(),
        }
    }

    /// Converts `planes` into `target`, an RGBA8 texture (sRGB or not) of
    /// their size that can be copied to. Submits its own commands, after
    /// every write made so far.
    pub fn convert(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, planes: &Planes<'_>, target: &wgpu::Texture) {
        let (width, height) = (planes.width as u32, planes.height as u32);
        let (chroma_width, chroma_height) = chroma_size(planes);
        let layout = &self.layout;
        let set = self
            .sets
            .entry([width, height, chroma_width, chroma_height])
            .or_insert_with(|| Set::new(device, layout, (width, height), (chroma_width, chroma_height)));

        let write = |texture: &wgpu::Texture, bytes: &[u8], stride: usize| {
            queue.uploading(Upload::Textures).write_texture(
                texture.as_image_copy(),
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: None,
                },
                texture.size(),
            );
        };
        write(&set.luma, planes.y, planes.luma_stride);
        if planes.subsampling == Subsampling::Gray {
            write(&set.cb, &[128], 1);
            write(&set.cr, &[128], 1);
        } else {
            write(&set.cb, planes.u, planes.chroma_stride);
            write(&set.cr, planes.v, planes.chroma_stride);
        }
        let [y_scale, y_offset, rv, gu, gv, bu] = coefficients_f32(planes.matrix, planes.full_range);
        let shift_x = u32::from(chroma_width < width);
        let shift_y = u32::from(chroma_height < height);
        let mut params = Vec::with_capacity(32);
        for f in [y_scale, y_offset, rv, gu, gv, bu] {
            params.extend_from_slice(&f.to_le_bytes());
        }
        params.extend_from_slice(&shift_x.to_le_bytes());
        params.extend_from_slice(&shift_y.to_le_bytes());
        queue.uploading(Upload::Uniforms).write_buffer(&set.params, 0, &params);

        // After the planes' writes, which may be gathered into the frame's
        // write batch (small ones are).
        let pipeline = &self.pipeline;
        traffic::submit_copy(device, queue, Some("video yuv to rgba"), |encoder| {
            let view = set.scratch.create_view(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("video yuv to rgba"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
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
                pass.set_bind_group(0, &set.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_texture(set.scratch.as_image_copy(), target.as_image_copy(), set.scratch.size());
        });
    }
}

impl Set {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, luma: (u32, u32), chroma: (u32, u32)) -> Self {
        let texture = |label, (width, height): (u32, u32), format, usage| {
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
                format,
                usage,
                view_formats: &[],
            })
        };
        let plane_usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let luma_texture = texture("video luma", luma, wgpu::TextureFormat::R8Unorm, plane_usage);
        let cb = texture("video cb", chroma, wgpu::TextureFormat::R8Unorm, plane_usage);
        let cr = texture("video cr", chroma, wgpu::TextureFormat::R8Unorm, plane_usage);
        let scratch = texture(
            "video rgba",
            luma,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video yuv params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let views = [&luma_texture, &cb, &cr].map(|t| t.create_view(&Default::default()));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video yuv to rgba"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        Self {
            luma: luma_texture,
            cb,
            cr,
            params,
            scratch,
            bind_group,
        }
    }
}
