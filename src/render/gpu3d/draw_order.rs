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
use crate::three::strips::StripRun;

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

/// One entry of the main view's see-through list (see
/// [`Renderer3D::sort_blended`]): what the transparent pass draws, in
/// order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SeeThrough {
    /// See-through models: `blended_runs[first..end]`.
    Models(u32, u32),
    /// Instances `start..start + count` of see-through batch `batch`'s
    /// far-to-near copy, all of level `level` (see `blend_order.rs`).
    Batch {
        batch: u32,
        level: u32,
        start: u32,
        count: u32,
    },
    /// The see-through batches with no order of their own (GPU-driven,
    /// or too many to sort) and the crowds.
    Unsorted,
    /// The main view's ribbons and lines `first..end` (far to near).
    Strips(u32, u32),
    /// The main view's particles `first..end` (far to near).
    Particles(u32, u32),
}

/// What a see-through model in the list is drawn by, before it is placed:
/// its view depth, where it can cover the screen and whether it may be
/// moved (see [`Renderer3D::screen_rect`]).
#[derive(Clone, Copy, Debug)]
pub(super) struct ModelItem {
    depth: f32,
    rect: [f32; 4],
    /// The screen's 8 x 8 tiles `rect` touches, a bit each.
    tiles: u64,
    movable: bool,
}

/// One item of the merged see-through order: a model (its index in the
/// sorted models), an instance of a sorted batch (batch, place in its
/// far-to-near copy), a ribbon or line, or a particle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Merged {
    Model(u32),
    Batch(u32, u32),
    Strip(u32),
    Particle(u32),
}

/// A rectangle that overlaps every other: a model that cannot be moved.
const FULL_SCREEN: [f32; 4] = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::INFINITY];
/// A rectangle that overlaps none.
#[cfg(test)]
const NOWHERE: [f32; 4] = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
/// How many models after one are looked at for others of its mesh.
const GROUP_WINDOW: usize = 32;

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

impl std::ops::Add for StateChanges {
    type Output = Self;
    fn add(mut self, o: Self) -> Self {
        self += o;
        self
    }
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

    /// Fills `blended_runs` and the main view's see-through list
    /// (`see_through`): everything the main view blends, far to near
    /// together.
    ///
    /// The see-through models are sorted far to near from `eye` by their
    /// origins, each run drawing a copy (in `blended`) of its instances in
    /// that order; models as far as each other are ordered by their
    /// instances' bytes, so the same models always draw the same picture,
    /// however the frame holds them. `base`: where `blended` starts in the
    /// instance buffer.
    ///
    /// The see-through batches drawn far to near (see `blend_order.rs`),
    /// the ribbons and lines and the particles (`effects`, each already
    /// far to near) join that order by their view depth: wherever one is
    /// farther than the next model, it is drawn first (see [`merge`]). At
    /// equal depth models come first, then batches, ribbons and lines,
    /// then particles. The batches and crowds with no order of their own
    /// follow the last model or batch, and the 3D text comes last.
    ///
    /// Then (with `group_see_through`) a model is drawn early, next to an
    /// earlier one of its mesh, where nothing it jumps over covers any of
    /// the same pixels (see [`group`]): blending gives the same picture,
    /// with one draw for both and no state change between them.
    pub(super) fn sort_blended(
        &mut self,
        instances: &[Instance3D],
        eye: glam::Vec3,
        base: u32,
        effects: &crate::three::particles::ParticleFrame,
        view_proj: glam::Mat4,
    ) {
        let mut runs = std::mem::take(&mut self.lists.blended_runs);
        let mut sorted = std::mem::take(&mut self.lists.blended);
        let mut keys = std::mem::take(&mut self.lists.blend_keys);
        let mut items = std::mem::take(&mut self.lists.blend_items);
        let mut merged = std::mem::take(&mut self.lists.blend_merged);
        let mut grouped = std::mem::take(&mut self.lists.blend_grouped);
        let mut used = std::mem::take(&mut self.lists.blend_used);
        let mut cursors = std::mem::take(&mut self.lists.blend_cursors);
        let mut list = std::mem::take(&mut self.lists.see_through);
        runs.clear();
        sorted.clear();
        keys.clear();
        items.clear();
        list.clear();
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
        let forward = effects.forward();
        let depth = |instance: &Instance3D| {
            let [x, y, z, _] = instance.model[3];
            (glam::Vec3::new(x, y, z) - eye).dot(forward)
        };
        // A margin of two pixels around each model's rectangle: rasterizing
        // (and the temporal jitter) never reaches past it.
        let margin = self.targets.as_ref().map_or(glam::Vec2::splat(0.02), |t| {
            glam::Vec2::new(4.0 / t.size.0.max(1) as f32, 4.0 / t.size.1.max(1) as f32)
        });
        let group = self.group_see_through;
        items.extend(keys.iter().map(|&(_, mesh, i)| {
            let instance = &instances[i as usize];
            let (rect, movable) = if group {
                self.screen_rect(mesh, instance, view_proj, margin)
            } else {
                (FULL_SCREEN, false)
            };
            ModelItem {
                depth: depth(instance),
                rect,
                tiles: tiles(rect),
                movable,
            }
        }));
        // The sources the list merges, in their order at equal depth.
        let joined = self.sort_strips_and_batches;
        cursors.clear();
        if joined {
            for (b, batch) in self.static_batches.iter().enumerate() {
                if batch.as_ref().is_some_and(|b| b.sorted_see_through()) {
                    cursors.push((b as u32, 0));
                }
            }
        }
        let batch_copies = &self.static_batches;
        let batch_sorted = |b: u32| -> &[Instance3D] {
            batch_copies[b as usize]
                .as_ref()
                .and_then(|b| b.blend.as_ref())
                .map_or(&[], |o| o.sorted())
        };
        let strips: &[crate::three::strips::StripRun] = if joined { &effects.strips.runs } else { &[] };
        let particles: &[f32] = if self.particles_in_blend_order {
            &effects.depths
        } else {
            &[]
        };
        merge(
            &items,
            (&mut cursors, batch_sorted),
            strips,
            particles,
            depth,
            &mut merged,
        );
        let order = if group {
            group_models(&merged, &items, &keys, &mut used, &mut grouped);
            &grouped
        } else {
            &merged
        };
        // The batches and crowds with no order, after the last model or
        // batch.
        let mut unsorted_at = 0;
        for &entry in order.iter() {
            match entry {
                Merged::Model(item) => {
                    let (_, mesh, i) = keys[item as usize];
                    let at = base + sorted.len() as u32;
                    let run = runs.len() as u32;
                    match (list.last_mut(), runs.last_mut()) {
                        // The last run is the list's last entry's last one.
                        (Some(SeeThrough::Models(..)), Some((last, start, count)))
                            if *last == mesh && *start + *count == at =>
                        {
                            *count += 1;
                        }
                        (Some(SeeThrough::Models(_, end)), _) => {
                            runs.push((mesh, at, 1));
                            *end = run + 1;
                        }
                        _ => {
                            runs.push((mesh, at, 1));
                            list.push(SeeThrough::Models(run, run + 1));
                        }
                    }
                    sorted.push(instances[i as usize]);
                    unsorted_at = list.len();
                }
                Merged::Batch(b, at) => {
                    let level = batch_copies[b as usize]
                        .as_ref()
                        .and_then(|b| b.blend.as_ref())
                        .map_or(0, |o| o.level_at(at));
                    match list.last_mut() {
                        Some(SeeThrough::Batch {
                            batch,
                            level: l,
                            start,
                            count,
                        }) if *batch == b && *l == level && *start + *count == at => *count += 1,
                        _ => list.push(SeeThrough::Batch {
                            batch: b,
                            level,
                            start: at,
                            count: 1,
                        }),
                    }
                    unsorted_at = list.len();
                }
                Merged::Strip(r) => match list.last_mut() {
                    Some(SeeThrough::Strips(_, end)) if *end == r => *end += 1,
                    _ => list.push(SeeThrough::Strips(r, r + 1)),
                },
                Merged::Particle(p) => match list.last_mut() {
                    Some(SeeThrough::Particles(_, end)) if *end == p => *end += 1,
                    _ => list.push(SeeThrough::Particles(p, p + 1)),
                },
            }
        }
        if !joined {
            // As before: each sorted batch in its own order after the
            // models, then the ribbons and lines.
            let mut at = unsorted_at;
            for (b, batch) in self.static_batches.iter().enumerate() {
                if let Some(order) = batch
                    .as_ref()
                    .filter(|b| b.sorted_see_through())
                    .and_then(|b| b.blend.as_ref())
                {
                    for &(level, start, count) in &order.runs {
                        let batch = b as u32;
                        list.insert(
                            at,
                            SeeThrough::Batch {
                                batch,
                                level,
                                start,
                                count,
                            },
                        );
                        at += 1;
                    }
                }
            }
            unsorted_at = at;
            if !effects.strips.runs.is_empty() {
                list.push(SeeThrough::Strips(0, effects.strips.runs.len() as u32));
            }
        }
        list.insert(unsorted_at, SeeThrough::Unsorted);
        if !self.particles_in_blend_order && !effects.instances.is_empty() {
            list.push(SeeThrough::Particles(0, effects.instances.len() as u32));
        }
        self.lists.blended_runs = runs;
        self.lists.blended = sorted;
        self.lists.blend_keys = keys;
        self.lists.blend_items = items;
        self.lists.blend_merged = merged;
        self.lists.blend_grouped = grouped;
        self.lists.blend_used = used;
        self.lists.blend_cursors = cursors;
        self.lists.see_through = list;
    }

    /// Where a see-through model can cover the screen: a rectangle in
    /// normalized device coordinates (x0, y0, x1, y1) around its mesh's
    /// bounding sphere, `margin` wider, and whether it may be moved in
    /// the draw order at all: a skinned model, one drawn from a GPU
    /// buffer, one whose shader moves its corners and one with more than
    /// one see-through part (whose parts are drawn part by part across a
    /// run) are not, and cover the whole screen.
    fn screen_rect(
        &self,
        mesh: u32,
        instance: &Instance3D,
        view_proj: glam::Mat4,
        margin: glam::Vec2,
    ) -> ([f32; 4], bool) {
        let geometry = &self.resources.meshes[self.base(mesh) as usize];
        let mut see_through = 0;
        for (part, &(_, own)) in geometry.parts.iter().enumerate() {
            let m = self.resources.material(self.part_material(mesh, part, own));
            if m.transparent {
                see_through += 1;
            }
            if self.shaders.sets[m.shader as usize].moves_vertices {
                return (FULL_SCREEN, false);
            }
        }
        if geometry.gpu_buffer.is_some() || !geometry.radius.is_finite() {
            return (FULL_SCREEN, false);
        }
        let m = glam::Mat4::from_cols_array_2d(&instance.model);
        let scale = m
            .x_axis
            .truncate()
            .length()
            .max(m.y_axis.truncate().length())
            .max(m.z_axis.truncate().length());
        let rect = sphere_rect(view_proj, m.w_axis.truncate(), geometry.radius * scale, margin);
        match rect {
            Some(rect) => (rect, see_through == 1),
            None => (FULL_SCREEN, false),
        }
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
    #[cfg(test)]
    pub(crate) fn take_particle_changes(&self) -> StateChanges {
        self.particle_counters.take()
    }

    /// What the main view's see-through list (models, batches, crowds,
    /// particles, ribbons, lines and text) recorded in the last frame
    /// drawn: part of [`Self::state_changes`].
    pub(crate) fn see_through_changes(&self) -> [u32; 5] {
        self.see_through_counters.read()
    }

    /// The main view's see-through list, for tests.
    #[cfg(test)]
    pub(crate) fn see_through_for_test(&self) -> Vec<SeeThrough> {
        self.lists.see_through.clone()
    }
}

/// Before a see-through model at view depth `depth`, which would be
/// run `runs` (a new one, if it was a step), the particles farther than
/// it, from `*placed` on in `depths` (far to near, never rising), are
/// drawn: pushes that step `(runs, particles up to)` and says whether it
/// did. Particles as far as the model are drawn after it. The particles
/// left after the last model are drawn after every see-through model.
/// (The model of [`merge`] that the transparency evaluation draws with.)
#[cfg(test)]
pub(in crate::render) fn place_particles(
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

/// Merges the see-through models (`models`, in their far-to-near order),
/// the sorted batches (`batches`: each batch's index and a cursor, with
/// its far-to-near copy by index; their view depth by `depth`), the
/// ribbons and lines (`strips`) and the particles (by their view depths,
/// `particles`) into one far-to-near order, each source keeping its own:
/// the next item is always the farthest of the sources' next ones, the
/// earlier source on a tie. Into `out`.
fn merge<'a>(
    models: &[ModelItem],
    (batches, sorted): (&mut [(u32, u32)], impl Fn(u32) -> &'a [Instance3D]),
    strips: &[StripRun],
    particles: &[f32],
    depth: impl Fn(&Instance3D) -> f32,
    out: &mut Vec<Merged>,
) {
    #[derive(Clone, Copy)]
    enum Source {
        Model,
        Batch(usize),
        Strip,
        Particle,
    }
    out.clear();
    let (mut m, mut s, mut p) = (0, 0, 0);
    loop {
        let mut best: Option<(f32, Source)> = None;
        let mut consider = |d: f32, source| {
            // Only a farther one replaces it: on a tie, the earlier source.
            if best.is_none_or(|(far, _)| d > far) {
                best = Some((d, source));
            }
        };
        if let Some(item) = models.get(m) {
            consider(item.depth, Source::Model);
        }
        for (k, &(b, at)) in batches.iter().enumerate() {
            if let Some(instance) = sorted(b).get(at as usize) {
                consider(depth(instance), Source::Batch(k));
            }
        }
        if let Some(run) = strips.get(s) {
            consider(run.depth, Source::Strip);
        }
        if let Some(&d) = particles.get(p) {
            consider(d, Source::Particle);
        }
        let Some((_, source)) = best else { break };
        out.push(match source {
            Source::Model => {
                m += 1;
                Merged::Model(m as u32 - 1)
            }
            Source::Batch(k) => {
                batches[k].1 += 1;
                Merged::Batch(batches[k].0, batches[k].1 - 1)
            }
            Source::Strip => {
                s += 1;
                Merged::Strip(s as u32 - 1)
            }
            Source::Particle => {
                p += 1;
                Merged::Particle(p as u32 - 1)
            }
        });
    }
}

/// Reorders `merged` into `out` to draw models of one mesh together where
/// blending gives the same picture: after each movable model, the next
/// models of its mesh (within [`GROUP_WINDOW`] models) are drawn right
/// after it, unless one of them covers pixels a model it would jump over
/// covers too (by their rectangles, `items`). Two draws that cover no
/// pixel in common give the same picture in either order, so the picture
/// is that of far-to-near blending. Nothing jumps over anything but
/// models; `keys` names each model's mesh, `used` is scratch space.
fn group_models(
    merged: &[Merged],
    items: &[ModelItem],
    keys: &[(f32, u32, u32)],
    used: &mut Vec<bool>,
    out: &mut Vec<Merged>,
) {
    out.clear();
    used.clear();
    used.resize(merged.len(), false);
    // The models passed over so far (at most the window's), and the tiles
    // they touch: a model whose tiles they don't touch overlaps none.
    let mut passed = [0u32; GROUP_WINDOW];
    for i in 0..merged.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        out.push(merged[i]);
        let Merged::Model(first) = merged[i] else { continue };
        if !items[first as usize].movable {
            continue;
        }
        let mesh = keys[first as usize].1;
        let (mut tiles, mut skipped, mut looked) = (0u64, 0, 0);
        for j in i + 1..merged.len() {
            if used[j] {
                continue;
            }
            let Merged::Model(next) = merged[j] else { break };
            looked += 1;
            if looked > GROUP_WINDOW {
                break;
            }
            let item = &items[next as usize];
            if keys[next as usize].1 == mesh
                && item.movable
                && (item.tiles & tiles == 0
                    || passed[..skipped]
                        .iter()
                        .all(|&k| !overlap(item.rect, items[k as usize].rect)))
            {
                used[j] = true;
                out.push(merged[j]);
                continue;
            }
            if !item.movable && item.rect == FULL_SCREEN {
                break;
            }
            tiles |= item.tiles;
            passed[skipped] = next;
            skipped += 1;
        }
    }
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// The tiles of an 8 x 8 grid over the screen that `rect` touches, a bit
/// each (what is off screen counts as the edge tiles).
fn tiles(rect: [f32; 4]) -> u64 {
    if rect[0] > rect[2] || rect[1] > rect[3] {
        return 0;
    }
    let tile = |v: f32| (((v + 1.0) * 4.0).floor().clamp(0.0, 7.0)) as u32;
    let (x0, x1, y0, y1) = (tile(rect[0]), tile(rect[2]), tile(rect[1]), tile(rect[3]));
    let row = ((1u64 << (x1 - x0 + 1)) - 1) << x0;
    (y0..=y1).fold(0, |m, y| m | row << (y * 8))
}

/// The rectangle (x0, y0, x1, y1, normalized device coordinates) a
/// sphere at `center` of `radius` can cover through `view_proj`, `margin`
/// wider; `None` where it reaches the camera's plane (it can cover
/// anything). Each clip coordinate of a point in the sphere is within
/// the radius times its row's length of the centre's, and x / w over
/// those ranges is smallest and largest at their ends.
pub(super) fn sphere_rect(
    view_proj: glam::Mat4,
    center: glam::Vec3,
    radius: f32,
    margin: glam::Vec2,
) -> Option<[f32; 4]> {
    let c = view_proj * center.extend(1.0);
    let reach = |row: glam::Vec4| radius * row.truncate().length();
    let (wl, wh) = (c.w - reach(view_proj.row(3)), c.w + reach(view_proj.row(3)));
    if !(wl > 1e-4) || !wh.is_finite() {
        return None;
    }
    let range = |v: f32, row: glam::Vec4| {
        let (low, high) = (v - reach(row), v + reach(row));
        ((low / wl).min(low / wh), (high / wl).max(high / wh))
    };
    let (x0, x1) = range(c.x, view_proj.row(0));
    let (y0, y1) = range(c.y, view_proj.row(1));
    let rect = [x0 - margin.x, y0 - margin.y, x1 + margin.x, y1 + margin.y];
    rect.iter().all(|v| v.is_finite()).then_some(rect)
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

#[cfg(test)]
mod see_through_tests {
    use super::*;
    use glam::{Vec2, Vec3};

    fn model(depth: f32, rect: [f32; 4]) -> ModelItem {
        ModelItem {
            depth,
            rect,
            tiles: tiles(rect),
            movable: rect != FULL_SCREEN,
        }
    }

    fn strip(depth: f32) -> StripRun {
        StripRun {
            first: 0,
            count: 6,
            texture: None,
            blend: crate::d3::ParticleBlend::Alpha,
            depth,
        }
    }

    #[test]
    fn the_merge_draws_the_farthest_next_item_and_models_first_on_a_tie() {
        let models = [model(10.0, NOWHERE), model(6.0, NOWHERE), model(2.0, NOWHERE)];
        let instance = |z: f32| {
            let mut i = Instance3D::default();
            i.model[3] = [0.0, 0.0, -z, 1.0];
            i
        };
        let batch = [instance(8.0), instance(6.0)];
        let mut cursors = [(5, 0)];
        let mut out = Vec::new();
        merge(
            &models,
            (&mut cursors, |_| &batch[..]),
            &[strip(9.0), strip(1.0)],
            &[12.0, 6.0, 3.0],
            |i: &Instance3D| -i.model[3][2],
            &mut out,
        );
        use Merged::*;
        assert_eq!(
            out,
            [
                Particle(0),
                Model(0),
                Strip(0),
                Batch(5, 0),
                // At depth 6: the model, the batch, then the particle.
                Model(1),
                Batch(5, 1),
                Particle(1),
                Particle(2),
                Model(2),
                Strip(1),
            ]
        );
    }

    fn keys(meshes: &[u32]) -> Vec<(f32, u32, u32)> {
        meshes.iter().enumerate().map(|(i, &m)| (0.0, m, i as u32)).collect()
    }

    fn grouped(items: &[ModelItem], meshes: &[u32], merged: &[Merged]) -> Vec<Merged> {
        let (mut used, mut out) = (Vec::new(), Vec::new());
        group_models(merged, items, &keys(meshes), &mut used, &mut out);
        out
    }

    const LEFT: [f32; 4] = [-1.0, -1.0, -0.5, 1.0];
    const MIDDLE: [f32; 4] = [-0.25, -1.0, 0.25, 1.0];
    const RIGHT: [f32; 4] = [0.5, -1.0, 1.0, 1.0];

    #[test]
    fn a_model_joins_an_earlier_one_of_its_mesh_only_past_models_it_shares_no_pixel_with() {
        use Merged::*;
        let all = [Model(0), Model(1), Model(2)];
        // Mesh 1 between two of mesh 0, apart on screen: they draw together.
        let apart = [model(3.0, LEFT), model(2.0, MIDDLE), model(1.0, RIGHT)];
        assert_eq!(grouped(&apart, &[0, 1, 0], &all), [Model(0), Model(2), Model(1)]);
        // The near one overlaps the middle one: it stays after it.
        let crossing = [model(3.0, LEFT), model(2.0, MIDDLE), model(1.0, [-0.3, -1.0, 1.0, 1.0])];
        assert_eq!(grouped(&crossing, &[0, 1, 0], &all), all);
        // A model that cannot move covers everything: nothing passes it.
        let stuck = [model(3.0, LEFT), model(2.0, FULL_SCREEN), model(1.0, RIGHT)];
        assert_eq!(grouped(&stuck, &[0, 1, 0], &all), all);
        // Past two models apart on either side of it: it overlaps neither,
        // though it is between them.
        let between = [
            model(4.0, MIDDLE),
            model(3.0, LEFT),
            model(2.0, RIGHT),
            model(1.0, MIDDLE),
        ];
        let four = [Model(0), Model(1), Model(2), Model(3)];
        assert_eq!(
            grouped(&between, &[0, 1, 2, 0], &four),
            [Model(0), Model(3), Model(1), Model(2)]
        );
        // Nor does anything pass a particle.
        let with_particle = [Model(0), Particle(0), Model(2)];
        assert_eq!(grouped(&apart, &[0, 1, 0], &with_particle), with_particle);
    }

    #[test]
    fn grouping_looks_only_a_window_of_models_ahead() {
        use Merged::*;
        let n = GROUP_WINDOW + 2;
        // Mesh 0 first and last, other meshes between, all apart.
        let items: Vec<ModelItem> = (0..n)
            .map(|i| {
                let x = i as f32 * 0.01;
                model(1.0, [x, 0.0, x + 0.005, 0.005])
            })
            .collect();
        let meshes: Vec<u32> = (0..n as u32).map(|i| if i == n as u32 - 1 { 0 } else { i }).collect();
        let merged: Vec<Merged> = (0..n as u32).map(Model).collect();
        assert_eq!(grouped(&items, &meshes, &merged), merged, "too far ahead to look for");
    }

    #[test]
    fn a_sphere_projects_inside_its_rectangle() {
        let mut camera = crate::d3::Camera::new(Vec2::new(160.0, 90.0));
        camera.position = Vec3::new(1.0, 2.0, 6.0);
        camera.look_at(Vec3::ZERO);
        let view_proj = camera.view_proj();
        let mut rng = crate::random::Rng::new(226);
        for _ in 0..200 {
            let center = Vec3::new(rng.range(-4.0, 4.0), rng.range(-3.0, 3.0), rng.range(-6.0, 2.0));
            let radius = rng.range(0.05, 2.0);
            let Some(rect) = sphere_rect(view_proj, center, radius, Vec2::ZERO) else {
                // Only a sphere that reaches the camera's plane has none.
                assert!((center - camera.position).dot(camera.forward()) < radius + camera.near + 1e-3);
                continue;
            };
            for _ in 0..64 {
                let d = Vec3::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0));
                let p = center + d.normalize_or_zero() * radius;
                let clip = view_proj * p.extend(1.0);
                let (x, y) = (clip.x / clip.w, clip.y / clip.w);
                assert!(
                    rect[0] <= x + 1e-4 && x <= rect[2] + 1e-4 && rect[1] <= y + 1e-4 && y <= rect[3] + 1e-4,
                    "{p} at ({x}, {y}) is outside {rect:?}"
                );
            }
        }
        // Behind the camera: anywhere.
        assert_eq!(sphere_rect(view_proj, camera.position, 0.5, Vec2::ZERO), None);
    }
}
