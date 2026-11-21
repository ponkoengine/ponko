//! The order models are drawn in, and how many GPU state changes that costs.
//!
//! The frame's runs come from collection in mesh order. Passes that only
//! keep the nearest depth (shadow maps, depth prepasses) or that test
//! against a finished prepass draw the same pixels in any order, so they
//! draw the runs sorted by the state each draw binds: pipeline, then
//! material, then mesh. Neighbouring runs then share pipelines and material
//! bind groups instead of switching back and forth. The transparent pass
//! blends, so it draws models globally far to near: the frame keeps a model's place in its mesh's
//! instances while the camera sees it, so their order there says nothing
//! about where they are, and changes as other models come and go.

use super::{FADED, Instance3D, Renderer3D, Run};

#[derive(Clone, Copy)]
pub(super) struct PartRun {
    pub run: Run,
    pub part: usize,
    pub key: (u32, u32, u32),
    pub distance: f32,
    /// Where its material draws among the solid ones: 0 without a
    /// stencil, after those by the stencil's order otherwise (see
    /// `stencil.rs`). Sorted on before the state.
    pub rank: u64,
}
use std::sync::atomic::{AtomicU32, Ordering};

/// GPU state changes and draws the model passes recorded in a frame
/// (counted from the commands the renderer issues, before wgpu's own
/// redundancy checks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StateChanges {
    pub pipelines: u32,
    pub bind_groups: u32,
    pub vertex_buffers: u32,
    pub index_buffers: u32,
    pub draws: u32,
}

impl std::ops::AddAssign for StateChanges {
    fn add_assign(&mut self, o: Self) {
        self.pipelines += o.pipelines;
        self.bind_groups += o.bind_groups;
        self.vertex_buffers += o.vertex_buffers;
        self.index_buffers += o.index_buffers;
        self.draws += o.draws;
    }
}

/// [`StateChanges`] added up across the passes, which record on different
/// threads; each pass adds its own counts once at its end.
#[derive(Default)]
pub(super) struct StateCounters([AtomicU32; 5]);

impl StateCounters {
    pub fn add(&self, c: &StateChanges) {
        let all = [c.pipelines, c.bind_groups, c.vertex_buffers, c.index_buffers, c.draws];
        for (counter, n) in self.0.iter().zip(all) {
            if n > 0 {
                counter.fetch_add(n, Ordering::Relaxed);
            }
        }
    }

    // The offscreen scene benchmark reads these; it is not built for the web.
    pub fn read(&self) -> [u32; 5] {
        std::array::from_fn(|i| self.0[i].load(Ordering::Relaxed))
    }

    pub fn take(&self) -> StateChanges {
        let [pipelines, bind_groups, vertex_buffers, index_buffers, draws] =
            std::array::from_fn(|i| self.0[i].swap(0, Ordering::Relaxed));
        StateChanges {
            pipelines,
            bind_groups,
            vertex_buffers,
            index_buffers,
            draws,
        }
    }
}

impl Renderer3D {
    /// What a run's opaque draws bind, as a sort key: its first solid
    /// part's pipeline (shader, cut-out, sides, dither, per-model storage),
    /// then that part's material, then the mesh. A run with a material
    /// override is keyed by the override. Runs with only transparent parts,
    /// which these passes skip, go last.
    fn state_key(&self, &(mesh, _, _): &Run) -> (u32, u32, u32) {
        let fading = mesh & FADED != 0;
        let base = &self.resources.meshes[self.base(mesh) as usize];
        let solid = base
            .parts
            .iter()
            .enumerate()
            .map(|(part, &(_, m))| self.part_material(mesh, part, m))
            .map(|m| (m, self.resources.material(m)))
            .find(|(_, m)| !m.transparent);
        let Some((id, m)) = solid else {
            return (u32::MAX, u32::MAX, mesh);
        };
        let pipeline = (m.shader << 4)
            | u32::from(m.cutout) << 3
            | u32::from(m.double_sided) << 2
            | u32::from(fading) << 1
            | u32::from(base.gpu_buffer.is_some());
        (pipeline, id, mesh)
    }

    /// Sorts `runs` by the state their draws bind (see [`Self::state_key`]),
    /// with `keys` as reusable scratch space.
    pub(super) fn sort_by_state(&self, runs: &mut [Run], keys: &mut Vec<((u32, u32, u32), Run)>) {
        if !self.group_draws || runs.len() < 2 {
            return;
        }
        keys.clear();
        keys.extend(runs.iter().map(|r| (self.state_key(r), *r)));
        // Mesh ids (with the fade bit) are unique within a list of runs, so
        // the keys are too and the unstable sort's order is deterministic.
        keys.sort_unstable_by_key(|&(key, _)| key);
        for (run, &(_, sorted)) in runs.iter_mut().zip(keys.iter()) {
            *run = sorted;
        }
    }

    /// Fills `solid_runs` and sorts the shadow views' and target cameras'
    /// runs (none of which blend) by state. After skinning's prepare, as
    /// skinned models' ids name its draws.
    pub(super) fn order_runs(&mut self) {
        let mut keys = std::mem::take(&mut self.lists.sort_keys);
        let mut solid = std::mem::take(&mut self.lists.solid_runs);
        solid.clear();
        solid.extend_from_slice(&self.lists.runs);
        self.sort_by_state(&mut solid, &mut keys);
        self.lists.solid_runs = solid;
        for view in super::shadow_views(self.lists.cascades, self.lists.light_views) {
            let mut runs = std::mem::take(&mut self.lists.shadow_runs[view]);
            self.sort_by_state(&mut runs, &mut keys);
            self.lists.shadow_runs[view] = runs;
        }
        let mut views = std::mem::take(&mut self.lists.views);
        for (_, runs, _) in &mut views {
            self.sort_by_state(runs, &mut keys);
        }
        self.lists.views = views;
        self.lists.sort_keys = keys;
        for view in super::shadow_views(self.lists.cascades, self.lists.light_views) {
            let mut parts = std::mem::take(&mut self.lists.shadow_parts[view]);
            self.expand_parts(&self.lists.shadow_runs[view], &mut parts);
            self.lists.shadow_parts[view] = parts;
        }
        let mut parts = std::mem::take(&mut self.lists.view_parts);
        parts.resize_with(self.lists.views.len(), Vec::new);
        for ((_, runs, _), parts) in self.lists.views.iter().zip(&mut parts) {
            self.expand_parts(runs, parts);
        }
        self.lists.view_parts = parts;
    }

    fn expand_parts(&self, runs: &[Run], parts: &mut Vec<PartRun>) {
        parts.clear();
        for &run in runs {
            let mesh = &self.resources.meshes[self.base(run.0) as usize];
            for (part, &(_, own)) in mesh.parts.iter().enumerate() {
                let material = self.part_material(run.0, part, own);
                let m = self.resources.material(material);
                if m.transparent {
                    continue;
                }
                let pipeline = (m.shader << 4)
                    | u32::from(m.cutout) << 3
                    | u32::from(m.double_sided) << 2
                    | u32::from(run.0 & FADED != 0) << 1
                    | u32::from(mesh.gpu_buffer.is_some());
                let buffers = if mesh.allocation.is_some() {
                    0
                } else {
                    self.base(run.0) + 1
                };
                parts.push(PartRun {
                    run,
                    part,
                    key: (pipeline, material, buffers),
                    distance: 0.0,
                    rank: self.stencil.rank(material),
                });
            }
        }
        if self.group_draws {
            parts.sort_unstable_by_key(|p| (p.rank, p.key, p.run.0, p.part));
        } else if self.stencil.any() {
            // Stable: the rest keep their order.
            parts.sort_by_key(|p| p.rank);
        }
    }

    /// Whether a run draws any part with a transparent material.
    pub(super) fn blends(&self, mesh: u32) -> bool {
        self.resources.meshes[self.base(mesh) as usize]
            .parts
            .iter()
            .enumerate()
            .any(|(part, &(_, m))| self.resources.material(self.part_material(mesh, part, m)).transparent)
    }

    /// Fills `blended_runs` in global back-to-front order, each drawing
    /// a copy (in `blended`) of its instances sorted far to near from `eye`,
    /// by the models' origins. Models as far as each other are ordered by
    /// their instances' bytes, so the same models always draw the same
    /// picture, however the frame holds them. `base`: where `blended`
    /// starts in the instance buffer.
    ///
    /// The main view's particles (`particles`, already far to near) join
    /// that order: `blend_steps` says where each stretch of them is drawn
    /// among the runs (see [`place_particles`]), so a particle behind a
    /// see-through model is drawn before it and takes its tint.
    pub(super) fn sort_blended(
        &mut self,
        instances: &[Instance3D],
        eye: glam::Vec3,
        base: u32,
        particles: &crate::three::particles::ParticleFrame,
    ) {
        let mut runs = std::mem::take(&mut self.lists.blended_runs);
        let mut sorted = std::mem::take(&mut self.lists.blended);
        let mut keys = std::mem::take(&mut self.lists.blend_keys);
        let mut steps = std::mem::take(&mut self.lists.blend_steps);
        runs.clear();
        sorted.clear();
        keys.clear();
        steps.clear();
        for &(mesh, start, count) in &self.lists.runs {
            if !self.blends(mesh) {
                continue;
            }
            keys.extend((start..start + count).map(|i| {
                let [x, y, z, _] = instances[i as usize].model[3];
                (glam::Vec3::new(x, y, z).distance_squared(eye), mesh, i)
            }));
        }
        keys.sort_unstable_by(|a, b| {
            b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)).then_with(|| {
                bytemuck::bytes_of(&instances[a.2 as usize]).cmp(bytemuck::bytes_of(&instances[b.2 as usize]))
            })
        });
        let depths: &[f32] = if self.particles_in_blend_order {
            &particles.depths
        } else {
            &[]
        };
        let forward = glam::Vec3::from_slice(&particles.up[..3]).cross(glam::Vec3::from_slice(&particles.right[..3]));
        let mut placed = 0;
        for &(_, mesh, i) in &keys {
            let at = base + sorted.len() as u32;
            let [x, y, z, _] = instances[i as usize].model[3];
            let depth = (glam::Vec3::new(x, y, z) - eye).dot(forward);
            let stepped = place_particles(depths, &mut placed, depth, runs.len() as u32, &mut steps);
            if let Some((last, start, count)) = runs.last_mut()
                && !stepped
                && *last == mesh
                && *start + *count == at
            {
                *count += 1;
            } else {
                runs.push((mesh, at, 1));
            }
            sorted.push(instances[i as usize]);
        }
        self.lists.blended_runs = runs;
        self.lists.blended = sorted;
        self.lists.blend_keys = keys;
        self.lists.blend_steps = steps;
    }

    pub(crate) fn state_changes(&self) -> [u32; 5] {
        self.state_counters.read()
    }

    /// The state changes and draws the model passes recorded since the last
    /// call (for tests and benchmarks).
    #[cfg(any(test, all(not(target_arch = "wasm32"), feature = "bench-internals")))]
    pub(crate) fn take_state_changes(&self) -> StateChanges {
        self.state_counters.take()
    }

    /// The same for the main view's particles, ribbons, lines and text,
    /// drawn with the see-through models.
    #[cfg(any(test, all(not(target_arch = "wasm32"), feature = "bench-internals")))]
    pub(crate) fn take_particle_changes(&self) -> StateChanges {
        self.particle_counters.take()
    }
}

/// Before a see-through model at view depth `depth`, which would be
/// run `runs` (a new one, if it was a step), the particles farther than
/// it, from `*placed` on in `depths` (far to near, never rising), are
/// drawn: pushes that step `(runs, particles up to)` and says whether it
/// did. Particles as far as the model are drawn after it. The particles
/// left after the last model are drawn after every see-through model, as
/// before (see `Renderer3D::draw_see_through`).
pub(super) fn place_particles(
    depths: &[f32],
    placed: &mut usize,
    depth: f32,
    runs: u32,
    steps: &mut Vec<(u32, u32)>,
) -> bool {
    let farther = depths[*placed..].iter().take_while(|&&d| d > depth).count();
    if farther == 0 {
        return false;
    }
    *placed += farther;
    steps.push((runs, *placed as u32));
    true
}

impl Renderer3D {
    /// Expand multi-material meshes into independently sortable part draws.
    /// The effective material includes whole-model and per-part overrides.
    /// With no prepass, preserve instancing and sort equal-state runs
    /// by distance without introducing extra buffer binds.
    pub(super) fn order_solid_parts(&mut self, instances: &[Instance3D], eye: glam::Vec3) {
        let mut draws = std::mem::take(&mut self.lists.solid_parts);
        draws.clear();
        let depth = |instance: &Instance3D| {
            let [x, y, z, _] = instance.model[3];
            glam::Vec3::new(x, y, z).distance_squared(eye)
        };
        for &(mesh, start, count) in &self.lists.solid_runs {
            let geometry = &self.resources.meshes[self.base(mesh) as usize];
            let run = (mesh, start, count);
            let mut near = f32::INFINITY;
            if !self.full_prepass() && self.group_draws {
                near = instances[start as usize..(start + count) as usize]
                    .iter()
                    .map(depth)
                    .fold(f32::INFINITY, f32::min);
            }
            for (part, &(_, own)) in geometry.parts.iter().enumerate() {
                let material = self.part_material(mesh, part, own);
                let m = self.resources.material(material);
                if m.transparent {
                    continue;
                }
                let pipeline = (m.shader << 4)
                    | u32::from(m.cutout) << 3
                    | u32::from(m.double_sided) << 2
                    | u32::from(mesh & FADED != 0) << 1
                    | u32::from(geometry.gpu_buffer.is_some());
                // Arena meshes share both bindings. Dedicated meshes keep
                // their own group so depth ordering cannot add switches.
                let buffers = if geometry.allocation.is_some() {
                    0
                } else {
                    self.base(mesh) + 1
                };
                draws.push(PartRun {
                    run,
                    part,
                    key: (pipeline, material, buffers),
                    distance: near,
                    rank: self.stencil.rank(material),
                });
            }
        }
        if self.group_draws {
            draws.sort_unstable_by(|a, b| {
                a.rank
                    .cmp(&b.rank)
                    .then_with(|| a.key.cmp(&b.key))
                    .then_with(|| a.distance.total_cmp(&b.distance))
                    .then_with(|| a.run.0.cmp(&b.run.0))
                    .then(a.part.cmp(&b.part))
            });
        }
        if !self.group_draws && self.stencil.any() {
            // Stable: the rest keep their order.
            draws.sort_by_key(|d| d.rank);
        }
        self.lists.solid_parts = draws;
    }
}

#[cfg(test)]
impl Renderer3D {
    pub(crate) fn flush_meshes_for_test(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.resources.mesh_arena.flush(device, queue);
    }
    pub(crate) fn group_blended_by_mesh_for_test(&mut self) {
        self.lists.blended_runs.sort_unstable_by_key(|r| r.0);
    }
    pub(crate) fn solid_part_order_for_test(&self) -> Vec<(u32, usize)> {
        self.lists.solid_parts.iter().map(|d| (d.run.0, d.part)).collect()
    }
}
