//! Windows that can't be drawn as sRGB: the frame is drawn into an sRGB
//! image and encoded into the window by one last pass.
//!
//! The renderer draws in linear light into an sRGB target, which encodes
//! what is written (and blends in linear light). Most windows offer such a
//! format, or an sRGB view of their own (browser canvases). An OpenGL ES
//! window whose EGL has no sRGB color space (EGL older than 1.5 without
//! `EGL_KHR_gl_colorspace`, as the Android emulator's) offers neither:
//! wgpu lists only `Rgba8Unorm` for it and no view formats. Drawn into
//! directly, the linear values would be shown as they are, too dark.
//!
//! There the renderer draws everything (the 3D frame, sprites, UI, the
//! inspector) into [`FRAME_FORMAT`], an sRGB image of the window's size, as
//! it would into an sRGB window, and [`SrgbEncode`] copies it into the
//! window, applying the sRGB curve in its shader. The window then holds the
//! bytes an sRGB window would have.

/// The image the frame is drawn into: every OpenGL ES 3 driver renders to
/// and samples it.
pub(crate) const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The frame image and the pass that encodes it into the window.
pub(crate) struct SrgbEncode {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// What the pass writes: the window's (linear) format.
    target: wgpu::TextureFormat,
    frame: Option<(wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,
}

impl SrgbEncode {
    /// The pass writing into `target`, the window's format.
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("srgb encode layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("srgb encode pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("srgb_output.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("srgb encode"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_full"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_encode"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout,
            pipeline,
            target,
            frame: None,
        }
    }

    /// The window's format, which the pass writes.
    pub fn target(&self) -> wgpu::TextureFormat {
        self.target
    }

    /// A view of the frame image at `size`, made again when the size changed.
    pub fn frame_view(&mut self, device: &wgpu::Device, (width, height): (u32, u32)) -> wgpu::TextureView {
        if let Some((texture, view, _)) = &self.frame
            && texture.width() == width
            && texture.height() == height
        {
            return view.clone();
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("srgb frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("srgb encode"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        self.frame = Some((texture, view.clone(), group));
        view
    }

    /// Records the pass encoding the frame image into `target` (a view of
    /// the window's image, in [`target`](Self::target)'s format).
    pub fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let Some((_, _, group)) = &self.frame else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("srgb encode"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The frame image, for the memory report.
    pub fn frame_texture(&self) -> Option<&wgpu::Texture> {
        self.frame.as_ref().map(|(t, _, _)| t)
    }
}

/// Whether a window configured as `config` needs [`SrgbEncode`]: its
/// format isn't sRGB and it has no sRGB view (`views_allowed`: the
/// adapter's `SURFACE_VIEW_FORMATS`).
pub(crate) fn needed(format: wgpu::TextureFormat, views_allowed: bool) -> bool {
    format.add_srgb_suffix() != format && !views_allowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::TextureFormat as F;

    #[test]
    fn only_a_linear_window_without_srgb_views_is_encoded_by_the_pass() {
        // OpenGL ES without EGL's sRGB color space: Rgba8Unorm, no views.
        assert!(needed(F::Rgba8Unorm, false));
        assert!(needed(F::Bgra8Unorm, false));
        // Browser canvases: linear, but drawn through an sRGB view.
        assert!(!needed(F::Bgra8Unorm, true));
        // sRGB windows (Vulkan, Metal, DX12, EGL with sRGB, WebGL2).
        assert!(!needed(F::Rgba8UnormSrgb, false));
        assert!(!needed(F::Bgra8UnormSrgb, true));
        // HDR formats have no sRGB twin; HDR output encodes them itself.
        assert!(!needed(F::Rgba16Float, false));
    }
}
