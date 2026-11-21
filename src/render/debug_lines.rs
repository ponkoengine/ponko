//! Debug lines (`ctx.debug`) in the lit 3D frame: one instance per line,
//! expanded on the GPU into a quad of the line's width in pixels. All of a
//! frame's lines are one buffer and a draw call for each kind there is:
//! the depth-tested ones (behind nearer surfaces), then the ones on top,
//! each opaque first, then see-through. Opaque lines are drawn without
//! blending: thin quads with several samples a pixel cost about three
//! times as much blended (100,000 lines at 1280x720 with 4 samples, see
//! `hundred_thousand_lines_gpu_time`). See-through lines are drawn with
//! alpha to coverage instead of blending when there are several samples a
//! pixel (their alpha covers that share of a pixel's samples), which costs
//! what opaque lines cost; with one sample they are blended. They are
//! drawn last in the pass that draws the particles (the opaque pass, or
//! the transparent one when it has a pass of its own), against the scene's
//! depth, which they never write. Target cameras that show the default
//! layer draw the same lines, from the same buffer, each through a camera
//! of its own. Made the first time a frame has a line.

use crate::debug::{GpuLines, LineInstance};
use crate::render::traffic::{MAX_WRITES, Synced, Upload};

const SHADER: &str = include_str!("debug_lines.wgsl");

/// The camera as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Camera {
    view_proj: [[f32; 4]; 4],
    viewport: [f32; 4],
}

/// How see-through lines are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SeeThrough {
    /// Blended over the frame: what one sample a pixel uses.
    Blend,
    /// Alpha to coverage, without blending: alpha is the share of a pixel's
    /// samples the line covers. Needs several samples a pixel.
    Coverage,
}

impl SeeThrough {
    /// The cheaper way for a pass of `samples` per pixel.
    pub(super) fn for_samples(samples: u32) -> Self {
        if samples > 1 { Self::Coverage } else { Self::Blend }
    }
}

/// A camera the lines are drawn through, and its bind group.
struct CameraGroup {
    camera: Synced,
    group: wgpu::BindGroup,
}

impl CameraGroup {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, label: &'static str) -> Self {
        let camera = Synced::new(
            device,
            label,
            wgpu::BufferUsages::UNIFORM,
            size_of::<Camera>(),
            Upload::Uniforms,
            (16, 8, MAX_WRITES),
        );
        let group = DebugLines::group(device, layout, &camera);
        Self { camera, group }
    }

    /// Seen through `view_proj` into a target of `size` pixels.
    fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        view_proj: glam::Mat4,
        size: (u32, u32),
    ) {
        let camera = Camera {
            view_proj: view_proj.to_cols_array_2d(),
            viewport: [size.0 as f32, size.1 as f32, 0.0, 0.0],
        };
        if self.camera.reserve(device, size_of::<Camera>()) {
            self.group = DebugLines::group(device, layout, &self.camera);
        }
        self.camera.write(queue, 0, bytemuck::bytes_of(&camera));
    }
}

/// The four pipelines (as `GpuLines::lists`: depth-tested opaque and
/// see-through, on top opaque and see-through), the cameras (the frame's,
/// then each target camera's) and this frame's lines.
pub(super) struct DebugLines {
    pipelines: [wgpu::RenderPipeline; 4],
    layout: wgpu::BindGroupLayout,
    main: CameraGroup,
    /// Whether the frame's own camera draws the lines (it shows the
    /// default layer).
    main_draws: bool,
    /// Target cameras by render target: `targets[..target_count]` draw
    /// this frame; the rest keep their storage for later frames.
    targets: Vec<(u32, CameraGroup)>,
    target_count: usize,
    lines: Synced,
    /// Lines of each kind this frame (what fits), in the order of `pipelines`.
    counts: [u32; 4],
    /// The sample count, depth format and way of drawing see-through lines
    /// the pipelines are for.
    pub(super) made_for: (u32, wgpu::TextureFormat, SeeThrough),
}

impl DebugLines {
    /// For a pass of `samples` per pixel into the HDR frame over a depth of
    /// `depth`, drawing see-through lines as `see_through` says.
    pub(super) fn new(
        device: &wgpu::Device,
        samples: u32,
        depth: wgpu::TextureFormat,
        see_through: SeeThrough,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug lines"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("debug lines"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("debug lines"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let buffer = wgpu::VertexBufferLayout {
            array_stride: size_of::<LineInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x3, 3 => Unorm8x4],
        };
        let pipeline = |label, compare, blend: Option<wgpu::BlendState>, coverage: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(buffer.clone())],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    alpha_to_coverage_enabled: coverage,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: super::gpu3d::HDR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let main = CameraGroup::new(device, &layout, "debug lines camera");
        let lines = Synced::new(
            device,
            "debug lines",
            wgpu::BufferUsages::VERTEX,
            size_of::<LineInstance>() * 1024,
            Upload::Other,
            (4096, 4, MAX_WRITES),
        );
        // Reverse-Z: nearer is greater.
        let (tested, top) = (wgpu::CompareFunction::GreaterEqual, wgpu::CompareFunction::Always);
        let coverage = see_through == SeeThrough::Coverage && samples > 1;
        let blended = (!coverage).then_some(wgpu::BlendState::ALPHA_BLENDING);
        Self {
            pipelines: [
                pipeline("debug lines", tested, None, false),
                pipeline("debug lines see-through", tested, blended, coverage),
                pipeline("debug lines on top", top, None, false),
                pipeline("debug lines on top see-through", top, blended, coverage),
            ],
            layout,
            main,
            main_draws: false,
            targets: Vec::new(),
            target_count: 0,
            lines,
            counts: [0; 4],
            made_for: (samples, depth, see_through),
        }
    }

    fn group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, camera: &Synced) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("debug lines"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.buffer().as_entire_binding(),
            }],
        })
    }

    /// Uploads this frame's `lines`, seen through `view_proj` into a
    /// target of `size` pixels; `for_targets`: whether target cameras draw
    /// them too (the lines are then uploaded even while the frame's own
    /// camera hides them). Target cameras are added after this with
    /// [`prepare_target`](Self::prepare_target).
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lines: &GpuLines,
        view_proj: glam::Mat4,
        size: (u32, u32),
        for_targets: bool,
    ) {
        self.target_count = 0;
        self.main_draws = !lines.hidden;
        if !self.main_draws && !for_targets {
            self.counts = [0; 4];
            return;
        }
        self.main.write(device, queue, &self.layout, view_proj, size);
        let stride = size_of::<LineInstance>();
        self.lines.reserve(device, lines.line_count() * stride);
        let mut room = self.lines.fits() / stride;
        let mut at = 0;
        for (count, list) in self.counts.iter_mut().zip(lines.lists()) {
            let n = list.len().min(room);
            self.lines.write(queue, at * stride, bytemuck::cast_slice(&list[..n]));
            (at, room) = (at + n, room - n);
            *count = n as u32;
        }
    }

    /// The target camera into render target `target` draws this frame's
    /// lines too, seen through `view_proj` into its `size` pixels. Each
    /// target keeps its camera's storage from frame to frame.
    pub(super) fn prepare_target(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: u32,
        view_proj: glam::Mat4,
        size: (u32, u32),
    ) {
        if self.count() == 0 {
            return;
        }
        let mut i = match self.targets.iter().position(|(t, _)| *t == target) {
            Some(i) => i,
            None => {
                let camera = CameraGroup::new(device, &self.layout, "debug lines target camera");
                self.targets.push((target, camera));
                self.targets.len() - 1
            }
        };
        // This frame's come first.
        if i >= self.target_count {
            self.targets.swap(i, self.target_count);
            i = self.target_count;
            self.target_count += 1;
        }
        self.targets[i].1.write(device, queue, &self.layout, view_proj, size);
    }

    /// Forgets the target cameras that didn't draw this frame (their render
    /// target may be gone).
    pub(super) fn end_targets(&mut self) {
        self.targets.truncate(self.target_count);
    }

    /// Lines uploaded this frame.
    pub(super) fn count(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// Draw calls this frame: one per kind with lines, for the frame's own
    /// camera and each target camera that draws them.
    pub(super) fn draw_count(&self) -> usize {
        let cameras = usize::from(self.main_draws) + self.target_count;
        self.counts.iter().filter(|&&n| n > 0).count() * cameras
    }

    /// Draws the lines into `pass` (the HDR frame over the scene's depth).
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.main_draws {
            self.draw_with(pass, &self.main.group);
        }
    }

    /// Draws the lines into the pass of the target camera into render
    /// target `target`, if it draws them this frame.
    pub(super) fn draw_target(&self, pass: &mut wgpu::RenderPass<'_>, target: u32) {
        if let Some((_, camera)) = self.targets[..self.target_count].iter().find(|(t, _)| *t == target) {
            self.draw_with(pass, &camera.group);
        }
    }

    fn draw_with(&self, pass: &mut wgpu::RenderPass<'_>, group: &wgpu::BindGroup) {
        if self.count() == 0 {
            return;
        }
        pass.set_bind_group(0, group, &[]);
        pass.set_vertex_buffer(0, self.lines.buffer().slice(..));
        let mut first = 0;
        for (pipeline, &n) in self.pipelines.iter().zip(&self.counts) {
            if n > 0 {
                pass.set_pipeline(pipeline);
                pass.draw(0..6, first..first + n);
            }
            first += n;
        }
    }

    /// Its buffers, for the memory report.
    pub(super) fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.buffer(C::Other, "debug lines", self.lines.buffer());
        t.buffer(C::Other, "debug lines camera", self.main.camera.buffer());
        for (_, target) in &self.targets {
            t.buffer(C::Other, "debug lines target camera", target.camera.buffer());
        }
    }

    /// Checks that the buffers hold what was written (tests).
    #[cfg(test)]
    pub(super) fn check_mirrors(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.main.camera.check(device, queue);
        for (_, target) in &self.targets {
            target.camera.check(device, queue);
        }
        self.lines.check(device, queue);
    }
}
