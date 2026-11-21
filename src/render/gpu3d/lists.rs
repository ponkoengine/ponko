//! A frame's draw lists: what `prepare` decided the passes draw.
//!
//! [`FrameLists`] is per-frame state. `Renderer3D::prepare` rewrites every
//! list (reusing the allocations), `Renderer3D::draw` reads them, and they
//! stay as they are until the next `prepare`: the frame statistics
//! (`draw_calls`, `counts`, `mesh_costs`) read them after the frame was
//! drawn. Nothing in them outlives a frame in meaning: a list never says
//! anything about the frame before.
//!
//! A run is `(mesh, first instance, count)`: one instanced draw of a mesh.
//! A run's mesh id is a mesh, a posed skinned model (ids past the meshes),
//! or a material override (ids from `override_base`); the lookups at the
//! end of this file resolve them.

use super::{Renderer3D, Run};
use crate::three::shadow_atlas::Tile;
use crate::three::{FADED, Instance3D, MAX_SHADOW_VIEWS, MaterialOverride};

/// This frame's draw lists, written by `prepare` and read by `draw`.
pub(super) struct FrameLists {
    /// The camera's runs.
    pub(super) runs: Vec<Run>,
    /// Each shadow view's runs: the sun's cascades, then the lights' views.
    pub(super) shadow_runs: [Vec<Run>; MAX_SHADOW_VIEWS],
    /// How many cascades the sun's shadow map has this frame, and which
    /// are drawn (one bit each); the others keep an earlier frame's.
    pub(super) cascades: usize,
    pub(super) redrawn: u32,
    /// How many point and spot light shadow views, and their tiles in the atlas.
    pub(super) light_views: usize,
    pub(super) light_tiles: Vec<Tile>,
    /// `runs` sorted by the state they bind, for the passes whose result
    /// doesn't depend on the order (see `draw_order.rs`).
    pub(super) solid_runs: Vec<Run>,
    /// Scratch space for sorting runs.
    pub(super) sort_keys: Vec<((u32, u32, u32), Run)>,
    /// The camera's runs that blend, each drawing a copy of its instances
    /// sorted far to near (see `sort_blended`): the copies, which follow
    /// every other instance in the instance buffer, and scratch space.
    pub(super) blended_runs: Vec<Run>,
    pub(super) blended: Vec<Instance3D>,
    pub(super) blend_keys: Vec<(f32, u32, u32)>,
    /// Where the main view's particles are drawn among `blended_runs`:
    /// `(r, p)` draws the particles up to instance `p` after the runs up
    /// to `r` (see `draw_order::place_particles`). The particles after
    /// the last step follow every see-through model.
    pub(super) blend_steps: Vec<(u32, u32)>,
    pub(super) solid_parts: Vec<super::draw_order::PartRun>,
    pub(super) shadow_parts: [Vec<super::draw_order::PartRun>; MAX_SHADOW_VIEWS],
    pub(super) view_parts: Vec<Vec<super::draw_order::PartRun>>,
    /// The runs of the models that moved, for the motion pass (their
    /// records are in the renderer's `moved` buffer).
    pub(super) moved_runs: Vec<Run>,
    /// The target cameras: (index into the render targets' `views`, runs
    /// with the first instance offset already added, batch visibility slot).
    pub(super) views: Vec<(usize, Vec<Run>, usize)>,
    /// Global illumination: what the probes' views draw, and each view's
    /// slice of `gi_face_runs`.
    pub(super) gi_runs: Vec<Run>,
    pub(super) gi_face_runs: Vec<Run>,
    pub(super) gi_faces: Vec<(u32, u32)>,
    /// The instance batches' first visibility slot for the probes: their
    /// sun shadow map's, then one per face direction, then one per view in
    /// `gi_faces` order (see `GiFrame::batch_views`). None when no probe
    /// updates.
    pub(super) gi_batch_slot: Option<usize>,
    /// The compute passes, in order: (compute, index into the render
    /// targets' `computes` or a GPU buffer id with the top bit set, material).
    pub(super) computes: Vec<(u32, usize, u32)>,
    /// The scene compute passes, in order, as `computes`: recorded in the
    /// post part, once the lit frame is settled.
    pub(super) scene_computes: Vec<(u32, usize, u32)>,
    /// Per skinned model, whether any pass draws it.
    pub(super) skin_drawn: Vec<bool>,
    /// The material overrides (see `Frame3D::material_overrides`): mesh id
    /// `override_base + k` draws entry `k`'s mesh with its materials.
    pub(super) material_overrides: Vec<MaterialOverride>,
    pub(super) override_base: u32,
    /// Per skinned model, the materials it is drawn with instead of its
    /// mesh's (the entry's mesh isn't used). Empty when none has any.
    pub(super) skin_overrides: Vec<MaterialOverride>,
    /// The `(part, material)` lists the two above point into.
    pub(super) override_parts: Vec<(u32, u32)>,
}

impl Default for FrameLists {
    fn default() -> Self {
        Self {
            runs: Vec::new(),
            shadow_runs: Default::default(),
            cascades: 0,
            redrawn: 0,
            light_views: 0,
            light_tiles: Vec::new(),
            solid_runs: Vec::new(),
            sort_keys: Vec::new(),
            blended_runs: Vec::new(),
            blended: Vec::new(),
            blend_keys: Vec::new(),
            blend_steps: Vec::new(),
            solid_parts: Vec::new(),
            shadow_parts: Default::default(),
            view_parts: Vec::new(),
            moved_runs: Vec::new(),
            views: Vec::new(),
            gi_runs: Vec::new(),
            gi_face_runs: Vec::new(),
            gi_faces: Vec::new(),
            gi_batch_slot: None,
            computes: Vec::new(),
            scene_computes: Vec::new(),
            skin_drawn: Vec::new(),
            material_overrides: Vec::new(),
            // No run's mesh id reaches it: no overrides until a frame has some.
            override_base: u32::MAX,
            skin_overrides: Vec::new(),
            override_parts: Vec::new(),
        }
    }
}

impl Renderer3D {
    /// The mesh a run's mesh id draws: itself, or for a skinned model (ids
    /// past the meshes), the skinned mesh.
    pub(super) fn base(&self, mesh: u32) -> u32 {
        let mesh = mesh & !FADED;
        if let Some(o) = self.material_override(mesh) {
            return o.mesh;
        }
        match (mesh as usize).checked_sub(self.resources.meshes.len()) {
            None => mesh,
            Some(k) => self.skinning.draws.get(k).map_or(0, |d| d.0),
        }
    }

    /// The mesh and materials a run's mesh id stands for, if it's a
    /// material override (see `Frame3D::material_overrides`).
    pub(super) fn material_override(&self, mesh: u32) -> Option<&MaterialOverride> {
        let k = (mesh & !FADED).checked_sub(self.lists.override_base)?;
        self.lists.material_overrides.get(k as usize)
    }

    /// The materials a run draws with instead of its mesh's: a material
    /// override's, or a skinned model's own.
    pub(super) fn run_override(&self, run_mesh: u32) -> Option<&MaterialOverride> {
        let mesh = run_mesh & !FADED;
        match mesh.checked_sub(self.lists.override_base) {
            Some(k) => self.lists.material_overrides.get(k as usize),
            None => {
                let k = (mesh as usize).checked_sub(self.resources.meshes.len())?;
                self.lists.skin_overrides.get(k)
            }
        }
    }

    /// The `(part, material)` list of an override.
    pub(super) fn override_parts(&self, o: &MaterialOverride) -> &[(u32, u32)] {
        let (start, len) = (o.parts.0 as usize, o.parts.1 as usize);
        self.lists.override_parts.get(start..start + len).unwrap_or(&[])
    }

    /// The material a run draws mesh part `part` with: the part's own, or
    /// the run's override (a per-part one first, then the whole mesh's).
    pub(super) fn part_material(&self, run_mesh: u32, part: usize, own: u32) -> u32 {
        let id = match self.run_override(run_mesh) {
            Some(o) => MaterialOverride::resolve(o.material, self.override_parts(o), part, own, |id| {
                self.resources.material_ids.was_made(id)
            }),
            None => own,
        };
        self.resources
            .material_slot(id)
            .unwrap_or(super::FALLBACK_MATERIAL_SLOT)
    }
}
