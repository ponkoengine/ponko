//! Instanced camera-facing particles, and the ribbons and lines drawn with
//! them (strips built on the CPU). GPU storage grows only at high-water
//! marks, and each frame uploads only the parts that changed: particles
//! that stood still, ribbons and lines that kept their shape and text that
//! stayed the same cost nothing.
use super::gpu3d::StateChanges;
use super::gpu_textures::TextureTable;
use crate::d3::ParticleBlend as ParticleBlend3D;
use crate::render::traffic::{MAX_WRITES, Synced, Upload};
use crate::three::particles::{ParticleFrame, ParticleInstance, ParticleRun};
use crate::three::strips::{StripRun, StripVertex};
use crate::three::text3d::TextVertex;

const SHADER: &str = include_str!("particles.wgsl");
const STRIP_SHADER: &str = include_str!("strips.wgsl");
const TEXT_SHADER: &str = include_str!("text3d.wgsl");
/// What the particle shaders know of the camera they draw for. The strip
/// and text shaders read the first three only.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Camera {
    view_proj: [[f32; 4]; 4],
    right: [f32; 4],
    up: [f32; 4],
    /// Where the scene's depth (group 2) is in the world, for soft
    /// particles.
    inv_view_proj: [[f32; 4]; 4],
    /// The eye; w: 1 when the pass can read the scene depth.
    eye: [f32; 4],
    forward: [f32; 4],
}
/// The pipelines particles, ribbons, lines and 3D text draw with, and what
/// each view (the main one, then each target camera) draws of them.
pub(super) struct Particles {
    alpha: wgpu::RenderPipeline,
    additive: wgpu::RenderPipeline,
    strip_alpha: wgpu::RenderPipeline,
    strip_additive: wgpu::RenderPipeline,
    text: wgpu::RenderPipeline,
    /// Debug labels on top in target cameras: no depth test.
    text_on_top: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    /// The main view's.
    main: ParticleView,
    /// Each target camera's, by its render target's texture id.
    targets: Vec<(u32, ParticleView)>,
    /// How many of `targets` this frame draws (the rest keep their buffers
    /// for when a camera is back).
    target_count: usize,
}
/// One view's camera and buffers. GPU storage grows only at high-water
/// marks, and each frame uploads only the parts that changed.
pub(super) struct ParticleView {
    camera: Synced,
    group: wgpu::BindGroup,
    instances: Synced,
    pub runs: Vec<ParticleRun>,
    strip_vertices: Synced,
    strip_runs: Vec<StripRun>,
    text_vertices: Synced,
    /// How many text vertices to draw this frame, and their atlas.
    text_count: u32,
    /// How many of the last of them are debug labels on top, drawn by
    /// [`Particles::draw_on_top`] instead.
    text_on_top: u32,
    text_atlas: Option<u32>,
    /// Whether a particle fades against the scene depth (only the main view).
    soft: bool,
}
/// What the particle pipelines read of the scene behind them: the scene
/// bind group's layout (group 2), and how its depth is declared.
pub(super) struct SceneDepth<'a> {
    pub layout: &'a wgpu::BindGroupLayout,
    /// The particle shader as the scene's depth binding needs it (see
    /// `gpu3d::shaders::for_samples`).
    pub adapt: &'a dyn Fn(&str) -> String,
}
impl Particles {
    /// The pipelines, over a scene depth of `depth` format (with stencil
    /// bits once a material has a stencil) and `samples` per pixel.
    pub fn new(
        device: &wgpu::Device,
        texture_layout: &wgpu::BindGroupLayout,
        scene: &SceneDepth<'_>,
        samples: u32,
        depth: wgpu::TextureFormat,
    ) -> Self {
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle camera"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        // Particles read the scene depth (group 2); ribbons, lines and
        // text don't, so they draw where no scene is bound too.
        let particle_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles"),
            bind_group_layouts: &[Some(&camera_layout), Some(texture_layout), Some(scene.layout)],
            immediate_size: 0,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particle strips and text"),
            bind_group_layouts: &[Some(&camera_layout), Some(texture_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl((scene.adapt)(SHADER).into()),
        });
        let strip_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("strips"),
            source: wgpu::ShaderSource::Wgsl(STRIP_SHADER.into()),
        });
        let text_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3d text"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER.into()),
        });
        let text_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<TextVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x2
            ],
        };
        let particle_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<ParticleInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4],
        };
        let strip_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<StripVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4],
        };
        let tested = wgpu::CompareFunction::GreaterEqual; // reverse-Z scene depth
        let pipeline_with = |layout: &wgpu::PipelineLayout,
                             module: &wgpu::ShaderModule,
                             buffer: &wgpu::VertexBufferLayout,
                             blend,
                             compare| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("particles"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
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
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: super::gpu3d::HDR_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = |layout, module, buffer, blend| pipeline_with(layout, module, buffer, blend, tested);
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let additive = wgpu::BlendState {
            color: add,
            alpha: wgpu::BlendComponent::OVER,
        };
        let alpha = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
        let main = ParticleView::new(device, &camera_layout);
        Self {
            alpha: pipeline(&particle_pipeline_layout, &module, &particle_layout, alpha),
            additive: pipeline(&particle_pipeline_layout, &module, &particle_layout, additive),
            strip_alpha: pipeline(&pipeline_layout, &strip_module, &strip_layout, alpha),
            strip_additive: pipeline(&pipeline_layout, &strip_module, &strip_layout, additive),
            text: pipeline(&pipeline_layout, &text_module, &text_layout, alpha),
            text_on_top: pipeline_with(
                &pipeline_layout,
                &text_module,
                &text_layout,
                alpha,
                wgpu::CompareFunction::Always,
            ),
            camera_layout,
            main,
            targets: Vec::new(),
            target_count: 0,
        }
    }

    /// [`Particles::new`] for a test's own pass of `samples` per pixel over
    /// a `Depth32Float` depth, with a scene layout of its own (bind
    /// nothing at group 2 when drawing text, ribbons and lines alone).
    #[cfg(test)]
    pub fn for_test(device: &wgpu::Device, texture_layout: &wgpu::BindGroupLayout, samples: u32) -> Self {
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: binding == 1 && samples > 1,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle test scene"),
            entries: &[
                texture(0, wgpu::TextureSampleType::Float { filterable: true }),
                texture(1, wgpu::TextureSampleType::Depth),
            ],
        });
        let scene = SceneDepth {
            layout: &layout,
            adapt: &|source| super::gpu3d::for_samples(source, samples, false).into_owned(),
        };
        Self::new(
            device,
            texture_layout,
            &scene,
            samples,
            wgpu::TextureFormat::Depth32Float,
        )
    }

    /// Uploads what the main view draws: `frame` with `view_proj`.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &ParticleFrame,
        view_proj: glam::Mat4,
        eye: glam::Vec3,
    ) {
        self.main.prepare(device, queue, frame, view_proj, eye, true);
    }

    /// Starts this frame's target cameras: none drawn yet.
    pub fn begin_targets(&mut self) {
        self.target_count = 0;
    }

    /// Uploads what the target camera into render target `target` draws:
    /// `frame` with `view_proj` from `eye`, without soft particles. Target
    /// cameras' views are kept by target, so each keeps its own storage.
    pub fn prepare_target(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: u32,
        frame: &ParticleFrame,
        view_proj: glam::Mat4,
        eye: glam::Vec3,
    ) {
        let mut i = match self.targets.iter().position(|(t, _)| *t == target) {
            Some(i) => i,
            None => {
                self.targets
                    .push((target, ParticleView::new(device, &self.camera_layout)));
                self.targets.len() - 1
            }
        };
        // This frame's come first: `targets[..target_count]` are drawn.
        if i >= self.target_count {
            self.targets.swap(i, self.target_count);
            i = self.target_count;
            self.target_count += 1;
        }
        self.targets[i].1.prepare(device, queue, frame, view_proj, eye, false);
    }

    /// Ends this frame's target cameras: those no camera drew into stop
    /// drawing, and their storage is freed.
    pub fn end_targets(&mut self) {
        self.targets.truncate(self.target_count);
    }

    /// The target camera view drawing into render target `target`, if it
    /// has something to draw this frame.
    pub fn target(&self, target: u32) -> Option<&ParticleView> {
        self.targets[..self.target_count]
            .iter()
            .find(|(t, _)| *t == target)
            .map(|(_, v)| v)
            .filter(|v| v.has_draws())
    }

    /// See `Renderer3D::check_mirrors`.
    #[cfg(test)]
    pub fn check_mirrors(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        for view in std::iter::once(&self.main).chain(self.targets.iter().map(|(_, v)| v)) {
            for buffer in [&view.camera, &view.instances, &view.strip_vertices, &view.text_vertices] {
                buffer.check(device, queue);
            }
        }
    }

    /// Whether the main view has particles, ribbons, lines or text to draw.
    pub fn has_draws(&self) -> bool {
        self.main.has_draws()
    }

    /// Whether the main view has a soft particle, which reads the scene's
    /// depth: it is then drawn in the transparent pass.
    pub fn soft(&self) -> bool {
        self.main.soft
    }

    /// How many draw calls `draw` makes in the main view, with its
    /// particles cut before each instance in `cuts` (drawn in stretches
    /// among the see-through models): a run a cut falls inside is drawn
    /// in two.
    pub fn draw_count(&self, cuts: impl Iterator<Item = u32> + Clone) -> usize {
        let split = self
            .main
            .runs
            .iter()
            .map(|r| cuts.clone().filter(|&c| r.first < c && c < r.first + r.count).count())
            .sum::<usize>();
        self.main.draw_count() + split
    }

    /// Draws the main view's ribbons and lines, then its particles, then
    /// its text (each set farthest first), in a test's own pass (the
    /// renderer draws them among the see-through models instead).
    #[cfg(test)]
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, textures: &dyn TextureTable, white: u32) {
        self.draw_view(&self.main, pass, textures, white);
    }

    /// The main view's particles from instance `particles.start` to
    /// `particles.end` (far to near), and how many state changes and
    /// draws that recorded: a stretch of them drawn among the see-through
    /// models (see `Renderer3D::draw_see_through`).
    pub fn draw_particles(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        textures: &dyn TextureTable,
        white: u32,
        particles: std::ops::Range<u32>,
    ) -> StateChanges {
        let mut n = StateChanges::default();
        if self.main.has_draws() {
            pass.set_bind_group(0, &self.main.group, &[]);
            n.bind_groups += 1;
            self.draw_particle_runs(&self.main, pass, textures, white, particles, &mut n);
        }
        n
    }

    /// The main view's ribbons and lines, its particles from instance
    /// `from` on and its text: what is drawn after every see-through
    /// model. From 0, everything the main view draws.
    pub fn draw_rest(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        textures: &dyn TextureTable,
        white: u32,
        from: u32,
    ) -> StateChanges {
        self.draw_counted(&self.main, pass, textures, white, from..u32::MAX)
    }

    /// [`draw`](Self::draw) for `view`.
    pub fn draw_view(
        &self,
        view: &ParticleView,
        pass: &mut wgpu::RenderPass<'_>,
        textures: &dyn TextureTable,
        white: u32,
    ) {
        self.draw_counted(view, pass, textures, white, 0..u32::MAX);
    }

    /// `view`'s ribbons and lines, its particles in `particles` and its
    /// text, counted.
    fn draw_counted(
        &self,
        view: &ParticleView,
        pass: &mut wgpu::RenderPass<'_>,
        textures: &dyn TextureTable,
        white: u32,
        particles: std::ops::Range<u32>,
    ) -> StateChanges {
        let mut n = StateChanges::default();
        if !view.has_draws() {
            return n;
        }
        let texture = |id: Option<u32>| {
            textures
                .texture(id.unwrap_or(white))
                .or_else(|| textures.texture(white))
        };
        pass.set_bind_group(0, &view.group, &[]);
        n.bind_groups += 1;
        if !view.strip_runs.is_empty() {
            pass.set_vertex_buffer(0, view.strip_vertices.buffer().slice(..));
            n.vertex_buffers += 1;
            for run in &view.strip_runs {
                let Some(texture) = texture(run.texture) else {
                    continue;
                };
                pass.set_pipeline(if run.blend == ParticleBlend3D::Alpha {
                    &self.strip_alpha
                } else {
                    &self.strip_additive
                });
                pass.set_bind_group(1, &texture.bind_group, &[]);
                pass.draw(run.first..run.first + run.count, 0..1);
                n.pipelines += 1;
                n.bind_groups += 1;
                n.draws += 1;
            }
        }
        self.draw_particle_runs(view, pass, textures, white, particles, &mut n);
        let tested = view.text_count - view.text_on_top;
        if tested > 0
            && let Some(atlas) = view.text_atlas.and_then(|id| textures.texture(id))
        {
            pass.set_pipeline(&self.text);
            pass.set_bind_group(1, &atlas.bind_group, &[]);
            pass.set_vertex_buffer(0, view.text_vertices.buffer().slice(..));
            pass.draw(0..tested, 0..1);
            n.pipelines += 1;
            n.bind_groups += 1;
            n.vertex_buffers += 1;
            n.draws += 1;
        }
        n
    }

    /// `view`'s particles in `particles` (its camera bound): each run's
    /// part in that range, one draw each.
    fn draw_particle_runs(
        &self,
        view: &ParticleView,
        pass: &mut wgpu::RenderPass<'_>,
        textures: &dyn TextureTable,
        white: u32,
        particles: std::ops::Range<u32>,
        n: &mut StateChanges,
    ) {
        let runs = view.runs.iter().filter_map(|run| {
            let first = run.first.max(particles.start);
            let end = (run.first + run.count).min(particles.end);
            (first < end).then_some((run, first..end))
        });
        let mut bound = false;
        for (run, instances) in runs {
            let Some(texture) = textures
                .texture(run.texture.unwrap_or(white))
                .or_else(|| textures.texture(white))
            else {
                continue;
            };
            if !bound {
                pass.set_vertex_buffer(0, view.instances.buffer().slice(..));
                n.vertex_buffers += 1;
                bound = true;
            }
            pass.set_pipeline(if run.blend == ParticleBlend3D::Alpha {
                &self.alpha
            } else {
                &self.additive
            });
            pass.set_bind_group(1, &texture.bind_group, &[]);
            pass.draw(0..6, instances);
            n.pipelines += 1;
            n.bind_groups += 1;
            n.draws += 1;
        }
    }

    /// Draws `view`'s debug labels on top (a target camera's), over
    /// everything drawn before them in `pass`: last, without a depth test.
    pub fn draw_on_top(&self, view: &ParticleView, pass: &mut wgpu::RenderPass<'_>, textures: &dyn TextureTable) {
        if view.text_on_top == 0 {
            return;
        }
        let Some(atlas) = view.text_atlas.and_then(|id| textures.texture(id)) else {
            return;
        };
        pass.set_bind_group(0, &view.group, &[]);
        pass.set_pipeline(&self.text_on_top);
        pass.set_bind_group(1, &atlas.bind_group, &[]);
        pass.set_vertex_buffer(0, view.text_vertices.buffer().slice(..));
        pass.draw(view.text_count - view.text_on_top..view.text_count, 0..1);
    }
}
impl ParticleView {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let camera = Synced::new(
            device,
            "particle camera",
            wgpu::BufferUsages::UNIFORM,
            size_of::<Camera>(),
            Upload::Uniforms,
            (16, 4, MAX_WRITES),
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle camera"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.buffer().as_entire_binding(),
            }],
        });
        Self {
            camera,
            group,
            instances: vertices::<ParticleInstance>(device, "particle instances", 1),
            runs: Vec::new(),
            strip_vertices: vertices::<StripVertex>(device, "strip vertices", 6),
            strip_runs: Vec::new(),
            text_vertices: vertices::<TextVertex>(device, "3d text vertices", 6),
            text_count: 0,
            text_on_top: 0,
            text_atlas: None,
            soft: false,
        }
    }

    /// Uploads `frame` drawn with `view_proj` from `eye`; `depth`: whether
    /// the pass it is drawn in can read the scene depth.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &ParticleFrame,
        view_proj: glam::Mat4,
        eye: glam::Vec3,
        depth: bool,
    ) {
        self.runs.clear();
        self.runs.extend_from_slice(&frame.runs);
        self.strip_runs.clear();
        self.strip_runs.extend_from_slice(&frame.strips.runs);
        let texts = &frame.texts.vertices;
        self.text_count = texts.len() as u32;
        self.text_on_top = frame.texts.on_top.min(texts.len()) as u32;
        self.text_atlas = frame.texts.atlas.map(|t| t.id);
        self.soft = depth && frame.soft && !frame.runs.is_empty();
        if !self.has_draws() {
            return;
        }
        // The buffers are bound when drawn, so a replaced one needs no
        // other rebinding.
        for (buffer, data) in [
            (&mut self.instances, bytemuck::cast_slice(&frame.instances)),
            (&mut self.strip_vertices, bytemuck::cast_slice(&frame.strips.vertices)),
            (&mut self.text_vertices, bytemuck::cast_slice(texts)),
        ] {
            if !data.is_empty() {
                buffer.reserve(device, data.len());
                buffer.write(queue, 0, data);
            }
        }
        // Past the most the device holds (reported), nothing is drawn.
        let fits = |buffer: &Synced, size: usize| (buffer.fits() / size) as u32;
        let instances = fits(&self.instances, size_of::<ParticleInstance>());
        clip(&mut self.runs, instances, |r| (&mut r.first, &mut r.count));
        let strips = fits(&self.strip_vertices, size_of::<StripVertex>());
        clip(&mut self.strip_runs, strips, |r| (&mut r.first, &mut r.count));
        let fitting = self.text_count.min(fits(&self.text_vertices, size_of::<TextVertex>()));
        // Past the most, the labels on top go first (they are the last).
        self.text_on_top = self.text_on_top.saturating_sub(self.text_count - fitting);
        self.text_count = fitting;
        let right = glam::Vec4::from_array(frame.right).truncate();
        let up = glam::Vec4::from_array(frame.up).truncate();
        let camera = Camera {
            view_proj: view_proj.to_cols_array_2d(),
            right: frame.right,
            up: frame.up,
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            eye: eye.extend(f32::from(u8::from(depth))).to_array(),
            forward: up.cross(right).extend(0.0).to_array(),
        };
        self.camera.write(queue, 0, bytemuck::bytes_of(&camera));
    }

    /// Whether there are particles, ribbons, lines or text to draw.
    fn has_draws(&self) -> bool {
        !self.runs.is_empty() || !self.strip_runs.is_empty() || self.text_count > 0
    }

    /// How many draw calls `draw_view` makes.
    fn draw_count(&self) -> usize {
        self.runs.len()
            + self.strip_runs.len()
            + usize::from(self.text_count > self.text_on_top)
            + usize::from(self.text_on_top > 0)
    }

    fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        for (label, synced) in [
            ("particle camera", &self.camera),
            ("particle instances", &self.instances),
            ("trail vertices", &self.strip_vertices),
            ("3d text vertices", &self.text_vertices),
        ] {
            t.buffer(C::Other, label, synced.buffer());
        }
    }
}
/// Shortens `runs` (each `first` and `count` of items) to the first `fits`
/// items.
fn clip<R>(runs: &mut Vec<R>, fits: u32, range: impl Fn(&mut R) -> (&mut u32, &mut u32)) {
    runs.retain_mut(|r| {
        let (first, count) = range(r);
        *count = (*count).min(fits.saturating_sub(*first));
        *count > 0
    });
}

/// A vertex buffer for `capacity` `T`s, written one changed `T` at a time
/// (with up to four unchanged ones between two changed ones along, and in
/// at most [`MAX_WRITES`] writes a frame).
fn vertices<T>(device: &wgpu::Device, label: &'static str, capacity: usize) -> Synced {
    let size = size_of::<T>();
    let usage = wgpu::BufferUsages::VERTEX;
    Synced::new(
        device,
        label,
        usage,
        capacity * size,
        Upload::Other,
        (size, 4, MAX_WRITES),
    )
}

impl Particles {
    /// Counts the particle, trail and 3D text buffers of every view as
    /// other memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        self.main.tally_memory(t);
        for (_, view) in &self.targets {
            view.tally_memory(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::gpu::GpuTexture;
    use super::super::gpu3d::*;
    #[allow(unused_imports)]
    use crate::d3::{
        Line as Line3D, ParticleBlend as ParticleBlend3D, ParticleEmitter as ParticleEmitter3D,
        ParticleSettings as ParticleSettings3D, Transform as Transform3D,
    };
    use crate::prelude::*;
    #[allow(unused_imports)]
    use crate::render::Blend;
    #[allow(unused_imports)]
    use crate::render::{MaterialDesc, MeshData, Model};
    use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};
    const SIZE: u32 = 64;

    #[test]
    fn particles_shader_is_valid() {
        super::super::gpu3d::check_shader("particles", super::SHADER).unwrap();
        super::super::gpu3d::check_shader("strips", super::STRIP_SHADER).unwrap();
        super::super::gpu3d::check_shader("3d text", super::TEXT_SHADER).unwrap();
    }

    #[test]
    fn particles_additive_brightens_pixels_offscreen() {
        let Some(crate::render::render_test_support::TestGpu { device, queue, .. }) =
            crate::render::render_test_support::gpu()
        else {
            return;
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        queue.write_texture(
            white.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            white.size(),
        );
        let sampler = device.create_sampler(&Default::default());
        let view = white.create_view(&Default::default());
        let textures = [GpuTexture {
            texture: white.clone(),
            nearest: false,
            mips: None,
            bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            }),
        }];
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target = output.create_view(&Default::default());

        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
        ctx.scene3d.camera.position = vec3(0.0, 0.0, 4.0);
        ctx.scene3d.camera.look_at(Vec3::ZERO);
        ctx.scene3d.lighting.shadows = false;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.post.bloom = 0.0;
        let mut renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
        let draw = |ctx: &mut Ctx, renderer: &mut Renderer3D| -> Vec<u8> {
            let mut skins = SkinFrame::default();
            skins.collect(&mut ctx.world, &ctx.gfx.assets3d);
            let mut frame = Frame3D::default();
            frame.collect(
                &mut ctx.world,
                &ctx.gfx.assets3d,
                &skins,
                &ctx.scene3d.camera,
                &ctx.scene3d.lighting,
            );
            frame.skins = skins;
            let scene = Scene3D {
                frame: &frame,
                lighting: &ctx.scene3d.lighting,
                sky: &ctx.scene3d.sky,
                light: SkyLight {
                    sun: Vec3::ZERO,
                    sky: Vec3::ZERO,
                    ground: Vec3::ZERO,
                },
                camera: ctx.scene3d.camera.position,
                post: &ctx.scene3d.post,
                time: (0.0, 1.0 / 60.0),
                occlusion: false,
            };
            renderer.prepare(
                &device,
                &queue,
                &Prepare3D {
                    scene: &scene,
                    lut: None,
                    size: (SIZE, SIZE),
                },
            );
            let buffers = renderer.draw(&device, &target, wgpu::Color::BLACK, &textures, None);
            queue.submit(buffers);
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(SIZE * SIZE * 4),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                output.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(SIZE * 4),
                        rows_per_image: None,
                    },
                },
                output.size(),
            );
            queue.submit([encoder.finish()]);
            readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            readback.get_mapped_range(..).unwrap().to_vec()
        };

        let before = draw(&mut ctx, &mut renderer);
        let center = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;

        // A glowing line across the middle lights it; the corner stays dark.
        let mut line = Line3D::new(
            vec![vec3(-2.0, 0.0, 0.0), vec3(2.0, 0.0, 0.0)],
            0.3,
            Color::rgb(1.0, 0.2, 0.0),
        );
        line.blend = ParticleBlend3D::Additive;
        let beam = ctx.world.spawn((Transform3D::default(), line));
        let lit = draw(&mut ctx, &mut renderer);
        assert!(lit[center] > before[center] + 40, "the line must brighten its pixels");
        assert_eq!(&lit[0..3], &before[0..3]);
        ctx.world.despawn(beam);
        assert_eq!(draw(&mut ctx, &mut renderer)[center], before[center]);
        let e = ctx.world.spawn((
            Transform3D::default(),
            ParticleEmitter3D::new(ParticleSettings3D {
                rate: 0.0,
                burst: 1,
                speed: (0.0, 0.0),
                gravity: Vec3::ZERO,
                size: (1.0, 1.0),
                blend: ParticleBlend3D::Additive,
                color_over_life: crate::particles::ParticleGradient::new(vec![(0.0, Color::rgba(1.0, 0.2, 0.0, 0.5))]),
                ..Default::default()
            }),
        ));
        ctx.update_particles();
        let after = draw(&mut ctx, &mut renderer);
        assert!(
            after[center] > before[center] + 40,
            "additive particles must brighten their location"
        );
        assert_eq!(
            &after[0..3],
            &before[0..3],
            "background outside particles stays unchanged"
        );

        // A second overlapping particle contributes light: particles do not write depth.
        ctx.world.get_mut::<ParticleEmitter3D>(e).unwrap().burst(1);
        ctx.update_particles();
        let twice = draw(&mut ctx, &mut renderer);
        assert!(twice[center] > after[center] + 10);

        // Removing the last emitter clears cached GPU draws; a later emitter can reuse storage.
        let saved = ctx.world.get::<ParticleEmitter3D>(e).unwrap().clone();
        ctx.world.despawn(e);
        assert_eq!(draw(&mut ctx, &mut renderer)[center], before[center]);
        assert!(!renderer.has_draws());
        let e = ctx.world.spawn((Transform3D::default(), saved));

        // A scene-reading transparent model exercises the separate transparent pass.
        let source = "fn surface(s: Surface) -> Surface { var o = s; o.emissive = scene_color(s.screen); o.alpha = 0.0; return o; }";
        let shader = ctx.gfx.create_shader("particle test glass", source);
        renderer.create_shader(&device, "particle test glass", source).unwrap();
        let desc = MaterialDesc {
            shader: Some(shader),
            blend: Blend::Transparent,
            ..Default::default()
        };
        let mat = ctx.gfx.create_material(desc);
        renderer.create_material(&device, &GpuMaterialDesc::new(&desc), &textures);
        let cube = MeshData::cube(0.5);
        let mesh = ctx.gfx.create_mesh(&cube, mat);
        renderer.create_mesh(&device, &cube, &[(0..cube.indices.len() as u32, mat)], false);
        ctx.world
            .spawn((Transform3D::at(vec3(1.2, 0.0, 0.0)), Model::new(mesh)));
        assert_eq!(draw(&mut ctx, &mut renderer)[center], twice[center]);

        // Moving the emitter does not move existing world-space particles.
        ctx.world.get_mut::<Transform3D>(e).unwrap().position = vec3(0.0, 0.0, 6.0);
        // World-space particles stay at the original position.
        assert_eq!(draw(&mut ctx, &mut renderer)[center], twice[center]);

        // Opaque geometry in front occludes the particles in the split pass too.
        let desc = MaterialDesc {
            color: Color::BLACK,
            ..Default::default()
        };
        let mat = ctx.gfx.create_material(desc);
        renderer.create_material(&device, &GpuMaterialDesc::new(&desc), &textures);
        let cube = MeshData::cube(1.0);
        let mesh = ctx.gfx.create_mesh(&cube, mat);
        renderer.create_mesh(&device, &cube, &[(0..cube.indices.len() as u32, mat)], false);
        ctx.world
            .spawn((Transform3D::at(vec3(0.0, 0.0, 1.0)), Model::new(mesh)));
        assert!(draw(&mut ctx, &mut renderer)[center] < twice[center] - 40);
    }
}
