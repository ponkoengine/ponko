//! Several cameras in one window, on the GPU side: copying each 3D view's
//! finished picture into its rectangle of the window, and filling a
//! rectangle with a color (a 2D view's clear). The 2D views' sprites are
//! drawn by the sprite pass (`Renderer::sprites_in_views`).
//!
//! A 3D window view is drawn whole into a texture of its own size
//! ([`ViewOutput`]), then copied into its rectangle by a full-screen
//! triangle under a viewport. Copying keeps the 3D frame's passes as they
//! are (every one of them fills its whole target) for one cheap pass per
//! view.

use crate::color::Color;

const SHADER: &str = r"
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@group(0) @binding(0) var picture: texture_2d<f32>;

@fragment
fn fs_copy(in: Out) -> @location(0) vec4<f32> {
    // The viewport is the picture's size: each pixel reads its own texel.
    let size = vec2<f32>(textureDimensions(picture));
    let texel = min(vec2<i32>(in.uv * size), vec2<i32>(size) - vec2<i32>(1));
    return textureLoad(picture, texel, 0);
}

@fragment
fn fs_fill() -> @location(0) vec4<f32> {
    // One, times the blend constant: the color (see `ViewPipelines::fill`).
    return vec4<f32>(1.0);
}
";

/// The copy and fill pipelines for one target format, made when a frame
/// first has views.
pub(super) struct ViewPipelines {
    format: wgpu::TextureFormat,
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    layout: wgpu::BindGroupLayout,
    copy: wgpu::RenderPipeline,
    fill: wgpu::RenderPipeline,
}

/// One 3D view's picture: drawn by the 3D frame, copied into the window.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(super) struct ViewOutput {
    texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

impl ViewPipelines {
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("window views"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("window view picture"),
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
        let pipeline = |label, layout: &wgpu::PipelineLayout, entry, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let copy_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("window view copy"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let empty = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("window view fill"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let constant = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        };
        Self {
            format,
            copy: pipeline("window view copy", &copy_layout, "fs_copy", None),
            fill: pipeline(
                "window view fill",
                &empty,
                "fs_fill",
                Some(wgpu::BlendState {
                    color: constant,
                    alpha: constant,
                }),
            ),
            layout,
        }
    }

    /// The format these pipelines draw into.
    pub(super) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// `output`, made again unless it already is `size` (in this format).
    #[cfg(feature = "3d")]
    pub(super) fn output(&self, device: &wgpu::Device, output: &mut Option<ViewOutput>, size: (u32, u32)) {
        if output
            .as_ref()
            .is_some_and(|o| (o.texture.width(), o.texture.height()) == size && o.texture.format() == self.format)
        {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("window view picture"),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("window view picture"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        *output = Some(ViewOutput { texture, view, group });
    }

    /// Copies `output` into `rect` (`[x, y, width, height]` in pixels, its
    /// size) of the pass's target.
    pub(super) fn copy(&self, pass: &mut wgpu::RenderPass<'_>, output: &ViewOutput, rect: [u32; 4]) {
        let [x, y, w, h] = rect;
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, w, h);
        pass.set_pipeline(&self.copy);
        pass.set_bind_group(0, &output.group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Fills `rect` of the pass's target with `color`.
    pub(super) fn fill(&self, pass: &mut wgpu::RenderPass<'_>, rect: [u32; 4], color: Color) {
        let [x, y, w, h] = rect;
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, w, h);
        pass.set_pipeline(&self.fill);
        pass.set_blend_constant(color.to_linear_wgpu());
        pass.draw(0..3, 0..1);
    }
}

/// One 2D camera of the frame, ready to draw the world's sprites with
/// (its camera bind group goes beside it, at the same index): its
/// rectangle of the window (`[x, y, width, height]` in pixels), its render
/// layers and its clear color. Plain values, so the renderer keeps the
/// list between frames.
#[derive(Clone, Copy, Debug)]
pub(super) struct SpriteView {
    pub(super) rect: [u32; 4],
    pub(super) layers: u32,
    pub(super) clear: Option<Color>,
}

/// The renderer's window views: the pipelines (made when first needed),
/// each 3D view's picture, and what this frame draws where.
#[derive(Default)]
pub(super) struct WindowViews {
    pub(super) pipelines: Option<ViewPipelines>,
    /// One picture per 3D view, the main camera's first.
    pub(super) outputs: Vec<Option<ViewOutput>>,
    /// This frame's 3D pictures: (picture, rectangle, whether the 3D frame
    /// drew it). Empty when the main camera fills the window alone: the 3D
    /// frame then draws straight into the window, as without views.
    pub(super) frame3d: Vec<(usize, [u32; 4], bool)>,
    /// The GPU timer's frame was begun when the 3D views were drawn.
    pub(super) timer_begun: bool,
    /// The 3D views' draw calls this frame (the main view's are counted
    /// as without views).
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(super) draw_calls: usize,
    /// The 3D views' command buffers and submissions last frame (one,
    /// unless a write that could not wait made the held ones go early).
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(super) submitted: (usize, u32),
}

impl WindowViews {
    /// The pipelines for `format`, made (again) when needed.
    pub(super) fn pipelines(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) -> &ViewPipelines {
        if self.pipelines.as_ref().is_none_or(|p| p.format() != format) {
            self.pipelines = Some(ViewPipelines::new(device, format));
        }
        self.pipelines.as_ref().unwrap()
    }
}

/// One 3D picture to draw: its frame, its rectangle of the window and its
/// clear color.
#[cfg(feature = "3d")]
pub(super) type ViewScene<'a> = (crate::three::Scene3D<'a>, crate::camera::Viewport, Color);

#[cfg(feature = "3d")]
impl WindowViews {
    /// Prepares, draws and submits the 3D pictures of `main` (the main
    /// camera's) and then each of `views` in order, into pictures of their
    /// rectangles' sizes in a `window` of that size. `lut` finds a post
    /// setting's lookup table. The main frame comes first: it draws the
    /// shadow maps the others read.
    ///
    /// All of them go to the GPU in one submission (`traffic::hold`): each
    /// frame's buffer writes are copied into place by its own first command
    /// buffer, so a view prepared after the main frame was recorded still
    /// reads its own camera and lights, and the main frame its own.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_3d<'a>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        three: &mut super::gpu3d::Renderer3D,
        format: wgpu::TextureFormat,
        window: [u32; 2],
        main: ViewScene<'a>,
        views: impl ExactSizeIterator<Item = ViewScene<'a>>,
        lut: &dyn Fn(&crate::three::Post) -> Option<(u32, u32)>,
        textures: &dyn super::gpu_textures::TextureTable,
        timer: Option<&super::timer::GpuTimer>,
    ) {
        self.frame3d.clear();
        self.draw_calls = 0;
        let count = views.len() + 1;
        three.keep_views(device, count - 1);
        self.pipelines(device, format);
        self.outputs.resize_with(count, || None);
        let pipelines = self.pipelines.as_ref().unwrap();
        let main_frame = main.0.frame;
        super::traffic::hold(queue);
        for (k, (scene, rect, clear)) in std::iter::once(main).chain(views).enumerate() {
            let rect = rect.physical(window);
            let size = (rect[2], rect[3]);
            pipelines.output(device, &mut self.outputs[k], size);
            let output = &self.outputs[k].as_ref().unwrap().view;
            let input = super::gpu3d::Prepare3D {
                scene: &scene,
                lut: lut(scene.post),
                size,
            };
            let clear = clear.to_linear_wgpu();
            let buffers = if k == 0 {
                three.prepare(device, queue, &input);
                if !three.has_draws() {
                    continue;
                }
                three.draw(device, output, clear, textures, timer)
            } else {
                three.prepare_view_frame(device, queue, k - 1, &input, main_frame);
                three.draw_view_frame(device, k - 1, output, clear, textures, timer)
            };
            let drawn = !buffers.is_empty();
            super::traffic::submit(queue, buffers);
            self.frame3d.push((k, rect, drawn));
        }
        self.submitted = super::traffic::release();
        three.submitted();
        self.draw_calls = three.views_draw_calls();
    }
}

impl WindowViews {
    /// Copies this frame's 3D pictures into their rectangles of `view` (a
    /// target of `size`), the rest cleared to `clear`. `false` (and nothing
    /// recorded) when this frame has no 3D views.
    pub(super) fn composite(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: [u32; 2],
        clear: Color,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> bool {
        let Some(pipelines) = self.pipelines.as_ref().filter(|_| !self.frame3d.is_empty()) else {
            return false;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("window views"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear.to_linear_wgpu()),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        for &(k, rect, drawn) in &self.frame3d {
            if let (true, Some(output), Some(rect)) = (drawn, &self.outputs[k], clip_rect(rect, size)) {
                pipelines.copy(&mut pass, output, rect);
            }
        }
        true
    }
}

/// `rect` clipped to a target of `size`; `None` when nothing of it is left.
pub(super) fn clip_rect(rect: [u32; 4], size: [u32; 2]) -> Option<[u32; 4]> {
    let [x, y, w, h] = rect;
    let right = (x.saturating_add(w)).min(size[0]);
    let bottom = (y.saturating_add(h)).min(size[1]);
    (x < right && y < bottom).then(|| [x, y, right - x, bottom - y])
}

#[cfg(test)]
mod tests {
    use super::clip_rect;

    #[test]
    fn rectangles_are_clipped_to_the_target() {
        assert_eq!(clip_rect([10, 10, 50, 50], [40, 100]), Some([10, 10, 30, 50]));
        assert_eq!(clip_rect([40, 0, 5, 5], [40, 100]), None);
        assert_eq!(clip_rect([0, 0, u32::MAX, 2], [8, 8]), Some([0, 0, 8, 2]));
    }
}
