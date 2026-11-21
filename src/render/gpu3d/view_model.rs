//! The view-model pass: first-person held items (`SceneSettings::view_model`)
//! drawn at the end of the main view's color pass, with a camera of their
//! own and the sliver of the scene depth nearest the eye.
//!
//! The held items are collected for the view-model camera
//! (`three::view_model`), their instances follow the frame's others in the
//! one instance buffer, and their frame uniform is the main view's with the
//! view-model camera's matrices: the same lights, clusters, shadows and
//! screen, so they are lit where they really are. Drawing them takes no
//! render pass, target or copy of its own: the opaque pass ends with them,
//! under a viewport whose depth range is `1 - VIEW_MODEL_DEPTH ..= 1`.
//! Reverse-Z puts the near plane at 1, so every held item is nearer than
//! the world (unless something is within a hundredth of the main camera's
//! near plane beyond it) and the items still sort among themselves. Solid
//! surfaces draw with the role that writes its own depth (they are in no
//! prepass); cut-outs, which rely on the prepass, are left out.
//!
//! Cut-out held items cut their own holes there too, with a pipeline of
//! their own built the first time one is drawn (`ShaderPipelines::
//! view_model_cutout`). Held items whose material has a stencil state draw
//! with it (`stencil.rs`), after the others and in their stencil's order,
//! as solid stencil materials do in the world: one that marks the stencil
//! draws before one that tests it. Before the held items, one full-screen triangle
//! pushes any world surface inside that sliver (a wall nearer than a
//! hundredth of the near plane's distance beyond it) back to the sliver's
//! far edge, so nothing in the world ever cuts into them.
//!
//! Motion vectors (temporal antialiasing): the motion pass draws the world
//! before the held items are in the depth, so they get a small pass of
//! their own right after the color pass, over the same depth and motion
//! image. Temporal antialiasing works out each pixel's camera motion by
//! reprojecting its depth with the main camera; a held item's depth is
//! the sliver's, seen through another lens, so its vector is what it
//! really did as its own camera saw it (`previous_view_proj`: the view
//! model's camera last frame, with the item's last matrix) minus what that
//! reprojection will make of it (`reprojection`: the main camera's
//! reprojection of the sliver, see `upload_view_model`). An item held
//! still in front of the eye stays put on screen however the eye turns or
//! walks; one that sways moves as much as it sways.
//!
//! Without a view model there is nothing: `view_model` is `None`, no
//! buffer or bind group exists, and the passes skip it on one test.

use super::*;

/// How much of the scene depth's range the held items are drawn into,
/// next to the near plane (reverse-Z: `1 - VIEW_MODEL_DEPTH ..= 1`).
pub(super) const VIEW_MODEL_DEPTH: f32 = 0.01;

/// What the view-model pass keeps while a view model is on.
pub(super) struct ViewModelPass {
    /// The held items' runs: (mesh id, first instance, count), in the
    /// frame's instance buffer.
    pub(super) runs: Vec<Run>,
    /// With a stencil material among them, their mesh parts in the order
    /// they draw: by stencil rank, then as `runs` holds them (empty
    /// otherwise: `runs` is drawn as it is).
    parts: Vec<(Run, usize)>,
    /// Their motion's runs, in `motion`: every held item, with temporal
    /// antialiasing on (empty otherwise, and on a frame without a past).
    motion_runs: Vec<Run>,
    /// The held items' instances with their last matrices
    /// (`MovedInstance`), and how many bytes it holds; made the first
    /// time there is motion to draw.
    motion: Option<(wgpu::Buffer, u64)>,
    /// The view model camera last frame, without jitter.
    previous: Option<Mat4>,
    /// Pushes the world out of the depth sliver (see the module's notes):
    /// made for the scene depth's format and samples.
    push_back: Option<(wgpu::RenderPipeline, wgpu::TextureFormat, u32)>,
    /// Negative controls for the tests: leave out the push back, the
    /// held items' motion.
    #[cfg(test)]
    off: (bool, bool),
    /// The view-model camera as drawn last (with the jitter), for tests.
    #[cfg(test)]
    jittered: Mat4,
    frame_buffer: wgpu::Buffer,
    /// What `frame_buffer` holds (see `sync_frame`).
    frame_mirror: Mirror,
    pub(super) frame_bind_group: wgpu::BindGroup,
}

impl Renderer3D {
    /// Lists the view model's runs, its instances starting at `offset` in
    /// the instance buffer; makes the pass's buffer on the first frame
    /// with a view model and drops it on the first without.
    pub(super) fn prepare_view_model(&mut self, device: &wgpu::Device, frame: &Frame3D, offset: u32) {
        if self.extra {
            return; // a window view: the main frame's pass stays as it is
        }
        let Some(vm_frame) = frame.view_model.as_deref() else {
            self.view_model = None;
            return;
        };
        let pass = self.view_model.get_or_insert_with(|| {
            let frame_buffer = device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("3d view model frame"),
                size: size_of::<FrameUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | MIRRORED,
                mapped_at_creation: false,
            });
            let frame_bind_group = self.frame_sources.bind_group(device, &frame_buffer, &self.lights);
            ViewModelPass {
                runs: Vec::new(),
                parts: Vec::new(),
                motion_runs: Vec::new(),
                motion: None,
                previous: None,
                push_back: None,
                #[cfg(test)]
                off: (false, false),
                #[cfg(test)]
                jittered: Mat4::IDENTITY,
                frame_buffer,
                frame_mirror: Mirror::default(),
                frame_bind_group,
            }
        });
        pass.runs.clear();
        pass.runs.extend(
            vm_frame
                .runs
                .iter()
                .map(|&(mesh, start, count)| (mesh, start + offset, count)),
        );
        let mut parts = std::mem::take(&mut pass.parts);
        self.order_view_model_parts(&mut parts);
        let Some(pass) = self.view_model.as_mut() else { return };
        pass.parts = parts;
        let (format, samples) = (self.shaders.depth_format(), self.samples);
        if !pass.push_back.as_ref().is_some_and(|p| (p.1, p.2) == (format, samples)) {
            pass.push_back = Some((push_back_pipeline(device, format, samples), format, samples));
        }
    }

    /// Fills `parts` with the held items' mesh parts in stencil order
    /// (see [`ViewModelPass::parts`]), or leaves it empty when none of
    /// their materials has a stencil.
    fn order_view_model_parts(&self, parts: &mut Vec<(Run, usize)>) {
        parts.clear();
        if !self.stencil.any() {
            return;
        }
        let mut ranked = false;
        for &run in self.view_model_runs() {
            let mesh = &self.resources.meshes[self.base(run.0 & !FADED) as usize];
            for (part, &(_, own)) in mesh.parts.iter().enumerate() {
                ranked |= self.stencil.has(self.part_material(run.0 & !FADED, part, own));
                parts.push((run, part));
            }
        }
        if !ranked {
            parts.clear();
            return;
        }
        // Stable: the rest keep their order.
        parts.sort_by_key(|&(run, part)| {
            let mesh = run.0 & !FADED;
            let own = self.resources.meshes[self.base(mesh) as usize].parts[part].1;
            self.stencil.rank(self.part_material(mesh, part, own))
        });
    }

    /// Step 6, for the view model: its held items' motion instances (all
    /// of them, moved or not), when temporal antialiasing reads them.
    pub(super) fn upload_view_model_motion(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame3D) {
        let on = self.screen.taa_on && self.screen.velocity_on && !self.extra;
        let (Some(pass), Some(vm_frame)) = (&mut self.view_model, frame.view_model.as_deref()) else {
            return;
        };
        pass.motion_runs.clear();
        if !on || vm_frame.motion.runs.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&vm_frame.motion.instances);
        let size = bytes.len() as u64;
        if pass.motion.as_ref().is_none_or(|(_, capacity)| *capacity < size) {
            let capacity = size.next_power_of_two().max(1024);
            let buffer = device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("3d view model motion"),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            pass.motion = Some((buffer, capacity));
        }
        let Some((buffer, _)) = &pass.motion else { return };
        queue.uploading(Upload::Instances).write_buffer(buffer, 0, bytes);
        pass.motion_runs.extend_from_slice(&vm_frame.motion.runs);
    }

    /// Whether this frame draws the held items' motion (see
    /// `record_view_model_motion`): the main motion pass then keeps its
    /// multisampled image for it.
    pub(super) fn view_model_motion(&self) -> bool {
        !self.extra
            && self.screen.taa_on
            && self.screen.velocity_on
            && self.view_model.as_ref().is_some_and(|p| {
                #[cfg(test)]
                if p.off.1 {
                    return false;
                }
                !p.motion_runs.is_empty()
            })
    }

    /// Mesh parts the held items' motion pass draws (for `draw_calls`).
    pub(super) fn view_model_motion_parts(&self) -> usize {
        if !self.view_model_motion() {
            return 0;
        }
        self.view_model.as_ref().map_or(0, |p| {
            p.motion_runs
                .iter()
                .map(|&(mesh, _, _)| self.resources.meshes[self.base(mesh) as usize].parts.len())
                .sum()
        })
    }

    /// Records the held items' motion vectors over the motion image, after
    /// the color pass put them in the depth (see the module's notes).
    pub(super) fn record_view_model_motion(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        time: &PassTimer<'_, '_>,
    ) {
        let (true, Some(velocity), Some(vm)) = (self.view_model_motion(), &targets.velocity, &self.view_model) else {
            return;
        };
        let Some((buffer, _)) = &vm.motion else { return };
        passes::mark("view model motion");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3d view model motion"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &velocity.velocity_color,
                depth_slice: None,
                resolve_target: Some(velocity.velocity()),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth,
                depth_ops: None,
                stencil_ops: None,
            }),
            timestamp_writes: time("view model motion"),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let size = targets.size;
        pass.set_viewport(0.0, 0.0, size.0 as f32, size.1 as f32, 1.0 - VIEW_MODEL_DEPTH, 1.0);
        pass.set_bind_group(0, &vm.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.no_scene, &[]);
        self.draw_model_runs(&mut pass, &vm.motion_runs, record::Kind::Velocity, buffer);
    }

    /// Makes the view model's frame bind group again, after the main
    /// view's sources (shadow maps, light textures, decals) changed.
    pub(super) fn rebind_view_model(&mut self, device: &wgpu::Device) {
        if let Some(pass) = &mut self.view_model {
            pass.frame_bind_group = self.frame_sources.bind_group(device, &pass.frame_buffer, &self.lights);
        }
    }

    /// Writes the view model's frame uniform: the main view's `main`, seen
    /// through the view-model camera (with the main view's jitter, so
    /// temporal antialiasing resolves the items as the world), and its
    /// motion: the view-model camera's last frame, and the main camera's
    /// reprojection of the depth sliver (see [`sliver_reprojection`]).
    /// `past`: the last frame is a past of this one (no history reset).
    pub(super) fn upload_view_model(
        &mut self,
        queue: &wgpu::Queue,
        frame: &Frame3D,
        main: &FrameUniform,
        jitter: Mat4,
    ) {
        let past = self.history.previous.is_some();
        let (Some(pass), Some(vm_frame), false) = (&mut self.view_model, frame.view_model.as_deref(), self.extra)
        else {
            return;
        };
        let mut theirs = *main;
        let unjittered = vm_frame.view_proj;
        let view_proj = jitter * unjittered;
        theirs.view_proj = view_proj.to_cols_array_2d();
        theirs.inv_view_proj = view_proj.inverse().to_cols_array_2d();
        theirs.unjittered_view_proj = unjittered.to_cols_array_2d();
        #[cfg(test)]
        {
            pass.jittered = view_proj;
        }
        let previous = pass.previous.filter(|_| past).unwrap_or(unjittered);
        pass.previous = Some(unjittered);
        theirs.previous_view_proj = previous.to_cols_array_2d();
        theirs.reprojection = sliver_reprojection(
            Mat4::from_cols_array_2d(&main.unjittered_view_proj),
            Mat4::from_cols_array_2d(&main.previous_view_proj),
            unjittered,
        )
        .to_cols_array_2d();
        sync_frame(queue, &pass.frame_buffer, 0, &mut pass.frame_mirror, &theirs);
    }

    /// The instances of the view model's runs, for the skinning and the
    /// buffer's size: every run's.
    pub(super) fn view_model_runs(&self) -> &[Run] {
        self.view_model.as_ref().map_or(&[], |p| p.runs.as_slice())
    }

    /// Draws the held items into `pass` (the main view's color pass, on
    /// the scene depth): with `solid`, their solid surfaces; with
    /// `blended`, their see-through ones. Leaves the pass's viewport and
    /// group 0 and 2 changed: the caller draws nothing after it.
    pub(super) fn draw_view_model(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        size: (u32, u32),
        solid: bool,
        blended: bool,
    ) {
        let Some(vm) = self.view_model.as_ref().filter(|p| !self.extra && !p.runs.is_empty()) else {
            return;
        };
        // Part of the pass's "color" step: the same color and depth.
        pass.set_viewport(0.0, 0.0, size.0 as f32, size.1 as f32, 1.0 - VIEW_MODEL_DEPTH, 1.0);
        #[cfg(test)]
        let solid_push = solid && !vm.off.0;
        #[cfg(not(test))]
        let solid_push = solid;
        if solid_push && let Some((push_back, _, _)) = &vm.push_back {
            // The world out of the sliver first: nothing of it cuts in.
            pass.set_pipeline(push_back);
            pass.draw(0..3, 0..1);
        }
        pass.set_bind_group(0, &vm.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.no_scene, &[]);
        let draw = |pass: &mut wgpu::RenderPass<'_>, kind| {
            if vm.parts.is_empty() {
                self.draw_model_runs(pass, &vm.runs, kind, &self.instances);
            } else {
                let parts = vm.parts.iter().map(|&(run, part)| (run, Some(part)));
                self.record_model_parts(pass, parts, kind, &self.instances);
            }
        };
        if solid {
            draw(pass, record::Kind::ViewModel);
        }
        if blended {
            draw(pass, record::Kind::Transparent);
        }
    }

    /// Counts the view model's frame uniform and motion instances (see
    /// `memory.rs`): nothing without a view model.
    pub(super) fn tally_view_model(&self, t: &mut crate::render::memory::MemoryTally) {
        if let Some(pass) = &self.view_model {
            t.buffer(
                crate::render::memory::MemoryClass::Other,
                "view model frame uniform",
                &pass.frame_buffer,
            );
            if let Some((buffer, _)) = &pass.motion {
                t.buffer(crate::render::memory::MemoryClass::Other, "view model motion", buffer);
            }
        }
    }

    /// Whether the view model's pass exists (a view model was on in the
    /// last prepared frame).
    #[cfg(test)]
    pub(crate) fn has_view_model(&self) -> bool {
        self.view_model.is_some()
    }

    /// Negative controls: leaves out the push back of the world out of
    /// the sliver and the held items' motion (once the pass exists).
    #[cfg(test)]
    pub(crate) fn view_model_without(&mut self, push_back: bool, motion: bool) {
        if let Some(pass) = &mut self.view_model {
            pass.off = (push_back, motion);
        }
    }

    /// The view-model camera the last frame was drawn with, jitter and all.
    #[cfg(test)]
    pub(crate) fn view_model_jittered(&self) -> Option<Mat4> {
        self.view_model.as_ref().map(|p| p.jittered)
    }

    /// Whether the last recorded frame drew the held items' motion.
    #[cfg(test)]
    pub(crate) fn draws_view_model_motion(&self) -> bool {
        self.view_model_motion()
    }
}

/// The main camera's reprojection of the depth sliver the held items are
/// drawn into, as a matrix from the world (as the view-model camera
/// `held` sees it now) to the main camera's clip space last frame:
/// `previous · main⁻¹ · sliver · held`. `sliver` maps clip depth into the
/// sliver (`z' = VIEW_MODEL_DEPTH · z + (1 - VIEW_MODEL_DEPTH) · w`), as
/// the viewport does. Temporal antialiasing reprojects each pixel's depth
/// with the main camera, jittered both ways: the jitter (a shear of x and
/// y by w) commutes with `sliver`, so it cancels. Worked out in doubles:
/// the sliver is thin.
pub(super) fn sliver_reprojection(main: Mat4, previous: Mat4, held: Mat4) -> Mat4 {
    let d = f64::from(VIEW_MODEL_DEPTH);
    let sliver = glam::DMat4::from_cols(
        glam::DVec4::X,
        glam::DVec4::Y,
        glam::DVec4::new(0.0, 0.0, d, 0.0),
        glam::DVec4::new(0.0, 0.0, 1.0 - d, 1.0),
    );
    (previous.as_dmat4() * main.as_dmat4().inverse() * sliver * held.as_dmat4()).as_mat4()
}

/// The pipeline that pushes the world out of the depth sliver: a
/// full-screen triangle at the sliver's far edge (clip depth 0 under its
/// viewport) that writes its depth wherever the stored one is nearer, and
/// no color.
fn push_back_pipeline(device: &wgpu::Device, depth: wgpu::TextureFormat, samples: u32) -> wgpu::RenderPipeline {
    const SOURCE: &str = "
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_none() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
";
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("3d view model push back"),
        source: wgpu::ShaderSource::Wgsl(SOURCE.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("3d view model push back"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("3d view model push back"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_full"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth,
            depth_write_enabled: Some(true),
            // Reverse-Z: the sliver's edge is farther than what is in it.
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_none"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera at `at` turned by `yaw`, with `fov` (radians).
    fn camera(at: Vec3, yaw: f32, fov: f32) -> Mat4 {
        let view = Mat4::from_rotation_translation(glam::Quat::from_rotation_y(yaw), at).inverse();
        crate::three::reverse_z_perspective(fov, 1.5, 0.1, 500.0) * view
    }

    #[test]
    fn the_sliver_reprojection_is_what_taa_makes_of_the_sliver_depth() {
        let held = camera(Vec3::new(0.0, 1.6, 0.0), 0.3, 0.9);
        let main = camera(Vec3::new(0.0, 1.6, 0.0), 0.3, 1.2);
        let previous = camera(Vec3::new(0.1, 1.6, 0.2), 0.25, 1.2);
        let r = sliver_reprojection(main, previous, held);
        for p in [Vec3::new(0.2, 1.4, -0.5), Vec3::new(-0.1, 1.7, -0.3)] {
            // As drawn: the held camera's clip depth squeezed into the sliver.
            let ndc = held.project_point3(p);
            let depth = 1.0 - VIEW_MODEL_DEPTH + VIEW_MODEL_DEPTH * ndc.z;
            // As temporal antialiasing reads it back.
            let world = main.inverse().project_point3(Vec3::new(ndc.x, ndc.y, depth));
            let want = previous.project_point3(world);
            let got = r.project_point3(p);
            assert!(got.truncate().abs_diff_eq(want.truncate(), 1e-4), "{got} vs {want}");
        }
        // A camera that held still reprojects the sliver where it is.
        let still = sliver_reprojection(main, main, held);
        let p = Vec3::new(0.2, 1.4, -0.5);
        assert!(
            still
                .project_point3(p)
                .truncate()
                .abs_diff_eq(held.project_point3(p).truncate(), 1e-5)
        );
    }
}
