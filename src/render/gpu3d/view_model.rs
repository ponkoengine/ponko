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
    /// temporal antialiasing resolves the items as the world).
    pub(super) fn upload_view_model(
        &mut self,
        queue: &wgpu::Queue,
        frame: &Frame3D,
        main: &FrameUniform,
        jitter: Mat4,
    ) {
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
        // Held in front of the eye: no motion of their own to reproject.
        theirs.previous_view_proj = theirs.unjittered_view_proj;
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
        pass.set_bind_group(0, &vm.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.no_scene, &[]);
        if solid {
            self.draw_model_runs(pass, &vm.runs, record::Kind::ViewModel, &self.instances);
        }
        if blended {
            self.draw_model_runs(pass, &vm.runs, record::Kind::Transparent, &self.instances);
        }
    }

    /// Counts the view model's frame uniform (see `memory.rs`): nothing
    /// without a view model.
    pub(super) fn tally_view_model(&self, t: &mut crate::render::memory::MemoryTally) {
        if let Some(pass) = &self.view_model {
            t.buffer(
                crate::render::memory::MemoryClass::Other,
                "view model frame uniform",
                &pass.frame_buffer,
            );
        }
    }

    /// Whether the view model's pass exists (a view model was on in the
    /// last prepared frame).
    #[cfg(test)]
    pub(crate) fn has_view_model(&self) -> bool {
        self.view_model.is_some()
    }
}
