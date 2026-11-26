//! Real-time global illumination: light bounced off surfaces, from a grid
//! of probes around the camera (see [`GlobalIllumination`]).
//!
//! Each frame a few probes look at the world around them: the renderer
//! draws six small views from each (lit as usual, with the probes' light
//! as their ambient light, so light bounces once more with every update)
//! and turns them into how much light reaches the probe from each
//! direction, and how far away the surfaces around it are. Surfaces then
//! take their ambient light from the eight probes around them, each counted
//! only as far as it can see the surface (by those distances), so light
//! doesn't leak through walls.
//!
//! Here: the settings, which probes to update, and what their views draw.
//! The renderer's side is `render/gi.rs`.

use glam::{IVec3, Mat3, Mat4, Quat, UVec3, Vec2, Vec3, Vec4};

use super::collect::FADED;
use super::{Assets3D, Camera3D, Frame3D, Lighting, SkinFrame};
use ponko_ecs::World;

/// The most probes a [`GlobalIllumination`] grid has: a bigger
/// [`extent`](GlobalIllumination::extent) at the same spacing is shrunk to
/// fit.
pub const MAX_GI_PROBES: u32 = 8192;

/// The most probes updated in one frame
/// ([`GlobalIllumination::probes_per_frame`]).
pub const MAX_GI_PROBES_PER_FRAME: u32 = 16;

/// Real-time global illumination: sunlight, sky light and lamp light
/// bounced off the world's surfaces onto others, at
/// [`Lighting::gi`](crate::render::Lighting::gi). A red wall in the sun tints the
/// floor beside it red, a room lit through a window is lit all over (and a
/// closed one goes dark instead of taking the sky's light), and it all
/// follows the sun through a day.
///
/// A grid of light probes moves with the camera (`extent` across, one
/// probe every `spacing` world units). Every frame, `probes_per_frame` of
/// them look around: the nearest new probes first, then the ones whose
/// last look is oldest, nearer ones more often. Each draws six small views
/// of the opaque models and instance batches around it, lit by the sun,
/// point and spot lights (each with its shadows), the sky and the probes'
/// own light, so light bounces once more each time a probe looks again.
/// Transparent surfaces and particles are left out. Inside the grid, surfaces take this light
/// instead of the sky and ground ambient light; toward its edges it fades
/// back to that. Probes only count where they can see the surface, which
/// keeps light from leaking through walls thicker than about a tenth of the
/// spacing.
///
/// Cost: the probes' views are drawn on the GPU each frame (six per probe,
/// 16x16 pixels each, of the models and instance batches near the grid),
/// and a shadow map of the grid's surroundings. Point and spot light
/// shadows come from the camera's own shadow maps. Fewer probes per frame are cheaper but slower to
/// catch up when the light changes or the camera moves. The probes'
/// memory is 8 bytes times 320 texels per probe (about 6.5 MB for the
/// default 17x9x17 grid).
///
/// Off by default. Needs a GPU with compute shaders and 18 texture slots
/// per shader (any desktop or laptop GPU); elsewhere (WebGL2) it's off and
/// the ambient light is unchanged.
/// ```
/// use ponko::prelude::*;
/// let mut lighting = ponko::render::Lighting::default();
/// assert!(!lighting.gi.enabled);
/// lighting.gi.enabled = true;
/// assert_eq!(lighting.gi.probe_counts(), [17, 9, 17]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalIllumination {
    /// Whether bounced light is on. Default off.
    pub enabled: bool,
    /// The size of the probe grid around the camera, in world units (x, y,
    /// z). Default 32 x 16 x 32.
    pub extent: Vec3,
    /// How far apart the probes are, in world units: smaller is finer
    /// (light is told apart between smaller spaces) but costs more probes.
    /// Default 2.
    pub spacing: f32,
    /// How many probes look around each frame, 1 to
    /// [`GlobalIllumination::MAX_PROBES_PER_FRAME`](crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME). Default 4.
    pub probes_per_frame: u32,
    /// How bright the bounced light is: 1 is physically based. Default 1.
    pub intensity: f32,
}

impl Default for GlobalIllumination {
    fn default() -> Self {
        Self {
            enabled: false,
            extent: Vec3::new(32.0, 16.0, 32.0),
            spacing: 2.0,
            probes_per_frame: 4,
            intensity: 1.0,
        }
    }
}

impl GlobalIllumination {
    /// Whether these settings draw anything: on, with a positive extent,
    /// spacing and intensity.
    pub(crate) fn active(&self) -> bool {
        self.enabled
            && self.extent.is_finite()
            && self.extent.min_element() > 0.0
            && self.spacing.is_finite()
            && self.spacing > 0.0
            && self.intensity.is_finite()
            && self.intensity > 0.0
    }

    /// How many probes the grid has along x, y and z: `extent / spacing +
    /// 1`, 2 to 64 each, fewer when that is more than
    /// [`GlobalIllumination::MAX_PROBES`](crate::render::GlobalIllumination::MAX_PROBES) in all (the grid shrinks; the spacing stays).
    /// ```
    /// use ponko::prelude::*;
    /// let gi = ponko::render::GlobalIllumination { extent: vec3(8.0, 4.0, 8.0), spacing: 1.0, ..Default::default() };
    /// assert_eq!(gi.probe_counts(), [9, 5, 9]);
    /// let huge = ponko::render::GlobalIllumination { extent: Vec3::splat(1000.0), ..Default::default() };
    /// let [x, y, z] = huge.probe_counts();
    /// assert!(x * y * z <= ponko::render::GlobalIllumination::MAX_PROBES);
    /// ```
    #[must_use]
    pub fn probe_counts(&self) -> [u32; 3] {
        let spacing = if self.spacing.is_finite() && self.spacing > 0.0 {
            self.spacing
        } else {
            1.0
        };
        let extent = if self.extent.is_finite() {
            self.extent.max(Vec3::ZERO)
        } else {
            Vec3::ZERO
        };
        let along = |e: f32| ((e / spacing).round().min(63.0) as u32 + 1).clamp(2, 64);
        let mut counts = [along(extent.x), along(extent.y), along(extent.z)];
        let total = |c: &[u32; 3]| c[0] * c[1] * c[2];
        if total(&counts) > MAX_GI_PROBES {
            let shrink = (MAX_GI_PROBES as f32 / total(&counts) as f32).cbrt();
            for c in &mut counts {
                *c = ((*c as f32 * shrink) as u32).max(2);
            }
        }
        while total(&counts) > MAX_GI_PROBES {
            let largest = (0..3).max_by_key(|&i| counts[i]).unwrap();
            counts[largest] -= 1;
        }
        counts
    }
}

/// Width and height of the shadow map the probes' views are lit with.
pub(crate) const GI_SHADOW_SIZE: u32 = 1024;
/// How much of the light a probe already had stays when it looks again:
/// smooths the steps as probes update one after another.
pub(crate) const GI_KEEP: f32 = 0.3;
/// How far past the grid the probes' views see, in probe spacings.
const CAPTURE_MARGIN: f32 = 4.0;
/// The probes' views: the way each looks and its up direction (+X, -X,
/// +Y, -Y, +Z, -Z). `render/gi_update.wgsl` has the same table.
pub(crate) const GI_FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::NEG_Z),
    (Vec3::NEG_Y, Vec3::Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];
/// Where each update's own views start in [`GiFrame::batch_views`], after
/// the sun shadow map's and the six face directions'.
pub(crate) const GI_BATCH_FACES: usize = 7;
/// Instance batches leave out of a probe's view the instances narrower
/// than one of its pixels: a bounding radius under this fraction of the
/// distance to the probe (a view is 16 pixels across 90 degrees).
pub(crate) const GI_SMALLEST: f32 = 1.0 / 16.0;
/// A run split into more pieces than this for one view is drawn whole
/// (from its first instance the view sees to its last).
const MAX_PIECES: usize = 8;

/// Where the probe grid is: its size, spacing, and the cell of its lowest
/// corner. Probe `cell` sits at `cell * spacing`; the grid wraps around in
/// its slots, so moving it keeps every probe still inside where it was.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GiGrid {
    pub counts: UVec3,
    pub spacing: f32,
    pub origin: IVec3,
}

impl GiGrid {
    pub fn total(&self) -> usize {
        (self.counts.x * self.counts.y * self.counts.z) as usize
    }

    /// The slot probe `cell` lives in.
    #[cfg(test)]
    pub fn slot(&self, cell: IVec3) -> usize {
        let n = self.counts.as_ivec3();
        let s = cell.rem_euclid(n);
        (s.x + n.x * (s.y + n.y * s.z)) as usize
    }

    /// The probe of the grid in `slot`.
    pub fn cell(&self, slot: usize) -> IVec3 {
        let n = self.counts.as_ivec3();
        let slot = slot as i32;
        let s = IVec3::new(slot % n.x, slot / n.x % n.y, slot / (n.x * n.y));
        self.origin + (s - self.origin).rem_euclid(n)
    }

    /// What tells which probe a slot holds: the grid wraps around, so the
    /// same slot holds probes a grid's width apart. The shader skips a slot
    /// whose code isn't that of the probe it wants (it holds an older one,
    /// or none yet: 0).
    pub fn code(&self, cell: IVec3) -> f32 {
        let k = cell.div_euclid(self.counts.as_ivec3()).rem_euclid(IVec3::splat(8));
        (1 + k.x + 8 * k.y + 64 * k.z) as f32
    }

    pub fn position(&self, cell: IVec3) -> Vec3 {
        cell.as_vec3() * self.spacing
    }
}

/// A probe to look around this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GiUpdate {
    pub slot: u32,
    pub position: Vec3,
    /// Its [`GiGrid::code`].
    pub code: f32,
    /// How much of the slot's old light stays (0: it held another probe).
    pub keep: f32,
}

/// How much more often a probe whose light is changing looks around: its
/// turn comes `1 + GI_CHANGE_BOOST * change` times sooner, `change` being
/// how much its light changed the last time it looked, as a fraction of
/// the light (at most 1); one whose light has settled changes by under a
/// hundredth at each look. Stronger, the probes that haven't seen a change
/// yet wait too long: at 32 a wall taken away still lit the floor beside
/// it at 92% after a round of updates (at 8: 54%).
pub(crate) const GI_CHANGE_BOOST: f32 = 4.0;

#[cfg(test)]
thread_local! {
    /// Tests: pick the probes by age and distance alone, as before
    /// changing probes went first (the negative control).
    pub(crate) static GI_UNBOOSTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// How much each slot's light changed the last time it looked around, as a
/// fraction of its light: the renderer reads it back from the GPU a frame
/// or two late (see `render/gi.rs`) and leaves it here; empty or another
/// grid's length until then.
pub(crate) type GiChanges = std::sync::Arc<std::sync::Mutex<Vec<f32>>>;

/// Which probe each slot holds, and how long ago it looked around: what
/// picks the probes to update.
#[derive(Default)]
pub(crate) struct GiProbes {
    grid: GiGrid,
    held: Vec<Option<IVec3>>,
    age: Vec<u32>,
    order: Vec<(bool, f32, u32)>,
    changes: GiChanges,
}

impl GiProbes {
    /// Moves the grid to the camera and picks the `budget` probes to update
    /// into `out`: probes no slot holds yet (nearest first), then the ones
    /// that looked longest ago, nearer ones and changing ones sooner (see
    /// [`GI_CHANGE_BOOST`]).
    pub fn plan(
        &mut self,
        counts: [u32; 3],
        spacing: f32,
        camera: Vec3,
        budget: usize,
        out: &mut Vec<GiUpdate>,
    ) -> GiGrid {
        let counts = UVec3::from_array(counts);
        if counts != self.grid.counts || spacing != self.grid.spacing {
            self.held.clear();
            self.age.clear();
        }
        let center = (camera / spacing).round().as_ivec3();
        let origin = center - (counts.as_ivec3() - 1) / 2;
        self.grid = GiGrid {
            counts,
            spacing,
            origin,
        };
        let total = self.grid.total();
        self.held.resize(total, None);
        self.age.resize(total, 0);
        self.order.clear();
        // A poisoned lock only means a renderer panicked mid-write: the
        // changes are a hint, so then they're left out.
        let changes = self.changes.lock().ok();
        let changes = changes.as_deref().filter(|c| c.len() == total);
        #[cfg(test)]
        let changes = changes.filter(|_| !GI_UNBOOSTED.with(|u| u.get()));
        for slot in 0..total {
            let cell = self.grid.cell(slot);
            let distance = self.grid.position(cell).distance(camera) / spacing;
            let missing = self.held[slot] != Some(cell);
            let score = if missing {
                -distance
            } else {
                let change = changes.map_or(0.0, |c| c[slot].clamp(0.0, 1.0));
                self.age[slot] as f32 * (1.0 + GI_CHANGE_BOOST * change) / (1.0 + distance)
            };
            self.order.push((missing, score, slot as u32));
        }
        let budget = budget.min(total);
        // Highest first: missing ones, then by score.
        let first = |a: &(bool, f32, u32), b: &(bool, f32, u32)| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1));
        if budget < total && budget > 0 {
            self.order.select_nth_unstable_by(budget - 1, first);
        }
        self.order[..budget].sort_unstable_by(first);
        for age in &mut self.age {
            *age = age.saturating_add(1);
        }
        out.clear();
        for &(missing, _, slot) in &self.order[..budget] {
            let cell = self.grid.cell(slot as usize);
            out.push(GiUpdate {
                slot,
                position: self.grid.position(cell),
                code: self.grid.code(cell),
                keep: if missing { 0.0 } else { GI_KEEP },
            });
            self.held[slot as usize] = Some(cell);
            self.age[slot as usize] = 0;
        }
        self.grid
    }
}

/// Tests: what the probes' views leave out (see `Frame3D::gi_leave_out`).
/// Both off by default: they see everything they draw.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GiLeaveOut {
    /// Point and spot light shadows: the probes' views are lit by those
    /// lights as if nothing stood in their way.
    pub light_shadows: bool,
    /// Instance batches and crowds: no batch views, so neither the probes'
    /// faces nor their sun shadow map draw them.
    pub instance_batches: bool,
}

/// This frame's global illumination, as the renderer gets it (see
/// [`Frame3D::collect_gi`]).
#[derive(Default)]
pub(crate) struct GiFrame {
    pub grid: GiGrid,
    pub intensity: f32,
    /// The probes to update this frame.
    pub updates: Vec<GiUpdate>,
    /// What's around the grid: what the probes' views draw, and the lights
    /// they're lit by.
    pub capture: Frame3D,
    /// The camera `capture` was collected with (the lights' grid uses it).
    pub capture_view_proj: Mat4,
    /// Each update's six views, in [`GI_FACES`] order: what each draws
    /// (`face_runs[start..end]`, into `capture`'s instances) and its camera.
    pub faces: Vec<(u32, u32)>,
    pub face_runs: Vec<(u32, u32, u32)>,
    pub face_view_proj: Vec<Mat4>,
    /// The views' near and far planes.
    pub near: f32,
    pub far: f32,
    /// The sun's shadow map of the views: world to its clip space, and a
    /// texel's size in world units.
    pub sun_view_proj: Mat4,
    pub sun_texel: f32,
    /// What the instance batches are culled for, for the probes: the sun
    /// shadow map's view; one view per face direction ([`GI_FACES`] order)
    /// holding that face of every probe updated this frame (for batches
    /// culled on the GPU, whose every view keeps a list as long as the
    /// batch); then each update's six views (as `faces`, for batches
    /// culled on the CPU), with their probe. Cut off at the side of the
    /// views' box ahead. None without updates.
    pub batch_views: Vec<(Mat4, Option<Vec3>)>,
    probes: GiProbes,
    /// Each captured instance's bounding sphere in the world.
    spheres: Vec<Vec4>,
}

impl Frame3D {
    /// With global illumination on (and a GPU that has it), moves the probe
    /// grid to the camera, picks the probes to update and collects what
    /// their views draw. Off, it forgets the probes.
    pub(crate) fn collect_gi(
        &mut self,
        world: &mut World,
        assets: &Assets3D,
        skins: &SkinFrame,
        camera: &Camera3D,
        lighting: &Lighting,
    ) {
        let settings = lighting.gi;
        if !settings.active() || !assets.gi_supported {
            self.gi = None;
            return;
        }
        let mut gi = self.gi.take().unwrap_or_default();
        let budget = settings.probes_per_frame.clamp(1, MAX_GI_PROBES_PER_FRAME) as usize;
        let grid = gi.probes.plan(
            settings.probe_counts(),
            settings.spacing,
            camera.position,
            budget,
            &mut gi.updates,
        );
        gi.grid = grid;
        gi.intensity = settings.intensity;

        // What the views see: the grid and a margin around it.
        let margin = grid.spacing * CAPTURE_MARGIN;
        let lo = grid.position(grid.origin) - margin;
        let hi = grid.position(grid.origin + grid.counts.as_ivec3() - 1) + margin;
        let (center, half) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
        gi.near = 0.05;
        gi.far = (hi - lo).length();

        // Collected with a camera high above that box, looking straight
        // down, its view just wide enough at the box's top.
        let reach = half.x.max(half.z);
        let above = 2.0 * reach;
        let mut eye = Camera3D::new(Vec2::splat(64.0));
        eye.position = Vec3::new(center.x, hi.y + above, center.z);
        eye.rotation = Quat::from_mat3(&Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y));
        eye.fov_y = 2.0 * 0.5f32.atan();
        eye.near = above * 0.05;
        eye.far = above + 2.0 * half.y;
        // Particles aren't drawn in the probes' views: not collected either.
        eye.particles = false;
        let inner = Lighting {
            shadows: false,
            gi: GlobalIllumination {
                enabled: false,
                ..settings
            },
            ..*lighting
        };
        gi.capture.lod = self.lod;
        gi.capture.size_limit = self.size_limit;
        // Lit by the main view's point and spot light shadows (its atlas is
        // drawn before the probes' views): no shadow views of their own.
        #[cfg(test)]
        let shadowed = !self.gi_leave_out.light_shadows;
        #[cfg(not(test))]
        let shadowed = true;
        if shadowed {
            gi.capture.lights.share_shadows(&self.lights);
        }
        gi.capture.collect(world, assets, skins, &eye, &inner);
        gi.capture_view_proj = eye.view_proj();

        // The sun's shadow map: a box around the views' box, reaching back
        // towards the sun.
        let light = lighting.sun_direction.try_normalize().unwrap_or(Vec3::NEG_Y);
        let up = if light.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let radius = half.length();
        let texel = 2.0 * radius / GI_SHADOW_SIZE as f32;
        let rotation = glam::camera::rh::view::look_to_mat4(Vec3::ZERO, light, up);
        let snapped = (rotation.transform_point3(center) / texel).round() * texel;
        let center = rotation.inverse().transform_point3(snapped);
        let back = radius * 3.0;
        let view = glam::camera::rh::view::look_to_mat4(center - light * back, light, up);
        let projection =
            glam::camera::rh::proj::directx::orthographic(-radius, radius, -radius, radius, 0.0, back + radius);
        gi.sun_view_proj = projection * view;
        gi.sun_texel = texel;

        gi.cull_faces(assets);
        gi.plan_batch_views(lo, hi);
        #[cfg(test)]
        if self.gi_leave_out.instance_batches {
            gi.batch_views.clear();
        }
        self.gi = Some(gi);
    }
}

impl GiFrame {
    /// Where the renderer leaves how much each slot's light changed (see
    /// [`GiChanges`]).
    pub(crate) fn changes(&self) -> &GiChanges {
        &self.probes.changes
    }

    /// [`batch_views`](Self::batch_views), for the views' box `lo..hi`.
    ///
    /// A probe's face looking along `forward` sees a point at least as far
    /// ahead of the probe as it lies to the side of it (a 90 degree view).
    /// So every face of this frame's probes looking that way sees only
    /// what a 90 degree view sees from behind them all: centred on them
    /// across, as far behind the rearmost as they spread across. Each
    /// face's own view reaches to the box's side ahead of it (models come
    /// from that box too), not the whole way its depth goes.
    fn plan_batch_views(&mut self, lo: Vec3, hi: Vec3) {
        self.batch_views.clear();
        if self.updates.is_empty() {
            return;
        }
        self.batch_views.push((self.sun_view_proj, None));
        let ahead = |forward: Vec3| Vec3::select(forward.cmpgt(Vec3::ZERO), hi, lo).dot(forward);
        let perspective = |near: f32, far: f32| {
            glam::camera::rh::proj::directx::perspective(std::f32::consts::FRAC_PI_2, 1.0, near, far)
        };
        for (forward, up) in GI_FACES {
            let right = forward.cross(up);
            // The probes in the face's own axes: ahead, right, up.
            let (mut min, mut max) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
            for u in &self.updates {
                let p = Vec3::new(u.position.dot(forward), u.position.dot(right), u.position.dot(up));
                min = min.min(p);
                max = max.max(p);
            }
            let center = (min + max) * 0.5;
            let spread = 0.5 * (max.y - min.y).max(max.z - min.z);
            let back = min.x - spread;
            let eye = forward * back + right * center.y + up * center.z;
            // The faces see from their near plane on, past the probes.
            let near = spread + 0.5 * self.near;
            let far = (ahead(forward) - back).max(2.0 * near);
            let view = glam::camera::rh::view::look_to_mat4(eye, forward, up);
            self.batch_views.push((perspective(near, far) * view, None));
        }
        for update in &self.updates {
            for (forward, up) in GI_FACES {
                let far = (ahead(forward) - update.position.dot(forward)).max(2.0 * self.near);
                let view = glam::camera::rh::view::look_to_mat4(update.position, forward, up);
                self.batch_views
                    .push((perspective(self.near, far) * view, Some(update.position)));
            }
        }
    }

    /// Each update's six views and what each of them draws: the runs of
    /// `capture` cut down to the instances inside the view.
    fn cull_faces(&mut self, assets: &Assets3D) {
        let capture = &self.capture;
        self.spheres.clear();
        self.spheres
            .resize(capture.instances.len(), Vec4::new(0.0, 0.0, 0.0, f32::INFINITY));
        let real = assets.bounds.len() as u32;
        for &(mesh, start, count) in &capture.runs {
            // Material overrides are culled as their mesh; skinned models
            // (past the real meshes) are always drawn.
            let mesh = mesh & !FADED;
            let mesh = mesh
                .checked_sub(capture.override_base)
                .and_then(|k| capture.material_overrides.list.get(k as usize))
                .map_or(mesh, |o| o.mesh);
            let Some((center, radius)) = assets.live_bounds(mesh).filter(|_| mesh < real) else {
                continue;
            };
            for i in start..start + count {
                let m = capture.instances[i as usize].model;
                let column = |c: usize| Vec3::new(m[c][0], m[c][1], m[c][2]);
                let matrix = Mat4::from_cols(
                    column(0).extend(0.0),
                    column(1).extend(0.0),
                    column(2).extend(0.0),
                    column(3).extend(1.0),
                );
                let scale = column(0).length().max(column(1).length()).max(column(2).length());
                self.spheres[i as usize] = matrix.transform_point3(center).extend(radius * scale);
            }
        }
        self.faces.clear();
        self.face_runs.clear();
        self.face_view_proj.clear();
        // Reverse-Z, as every view drawn into the scene depth's format.
        let projection = super::reverse_z_perspective(std::f32::consts::FRAC_PI_2, 1.0, self.near, self.far);
        let far = self.far;
        for update in &self.updates {
            let probe = update.position;
            for (forward, up) in GI_FACES {
                let view = glam::camera::rh::view::look_to_mat4(probe, forward, up);
                self.face_view_proj.push(projection * view);
                let right = forward.cross(up);
                let sees = |s: Vec4| {
                    let d = s.truncate() - probe;
                    let r = s.w;
                    if !r.is_finite() {
                        return true;
                    }
                    let z = d.dot(forward);
                    let edge = z + r * std::f32::consts::SQRT_2;
                    z > -r && d.dot(right).abs() <= edge && d.dot(up).abs() <= edge && d.length() <= far + r
                };
                let first = self.face_runs.len() as u32;
                for &(mesh, start, count) in &capture.runs {
                    let at = self.face_runs.len();
                    let (mut open, mut lowest, mut highest) = (None, u32::MAX, 0);
                    for i in start..start + count {
                        if sees(self.spheres[i as usize]) {
                            lowest = lowest.min(i);
                            highest = i + 1;
                            open.get_or_insert(i);
                        } else if let Some(o) = open.take() {
                            self.face_runs.push((mesh, o, i - o));
                        }
                    }
                    if let Some(o) = open {
                        self.face_runs.push((mesh, o, start + count - o));
                    }
                    if self.face_runs.len() - at > MAX_PIECES {
                        self.face_runs.truncate(at);
                        self.face_runs.push((mesh, lowest, highest - lowest));
                    }
                }
                self.faces.push((first, self.face_runs.len() as u32));
            }
        }
    }
}

impl GlobalIllumination {
    /// Maximum resident probes.
    pub const MAX_PROBES: u32 = MAX_GI_PROBES;
    /// Probe update budget per frame.
    pub const MAX_PROBES_PER_FRAME: u32 = MAX_GI_PROBES_PER_FRAME;
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::vec3;

    #[test]
    fn probe_counts_follow_extent_and_spacing_within_limits() {
        let gi = GlobalIllumination::default();
        assert_eq!(gi.probe_counts(), [17, 9, 17]);
        let tiny = GlobalIllumination {
            extent: Vec3::splat(0.1),
            ..gi
        };
        assert_eq!(tiny.probe_counts(), [2, 2, 2]);
        let huge = GlobalIllumination {
            extent: Vec3::new(100.0, 20.0, 100.0),
            spacing: 1.0,
            ..gi
        };
        let c = huge.probe_counts();
        assert!(c.iter().all(|&n| (2..=64).contains(&n)));
        assert!(c[0] * c[1] * c[2] <= MAX_GI_PROBES);
        assert!(c[0] > c[1], "the shape stays: {c:?}");
        let broken = GlobalIllumination {
            spacing: f32::NAN,
            ..gi
        };
        assert!(!broken.active());
        assert_eq!(broken.probe_counts().map(|n| n.clamp(2, 64)), broken.probe_counts());
    }

    #[test]
    fn slots_wrap_so_moving_the_grid_keeps_what_stays_inside() {
        let grid = GiGrid {
            counts: UVec3::new(4, 3, 5),
            spacing: 2.0,
            origin: IVec3::new(-7, 2, 11),
        };
        let mut seen = vec![false; grid.total()];
        for slot in 0..grid.total() {
            let cell = grid.cell(slot);
            assert_eq!(grid.slot(cell), slot);
            let local = cell - grid.origin;
            assert!(local.cmpge(IVec3::ZERO).all() && local.cmplt(grid.counts.as_ivec3()).all());
            // Every cell of the grid, once.
            let c = grid.counts.as_ivec3();
            let at = (local.x + c.x * (local.y + c.y * local.z)) as usize;
            assert!(!seen[at]);
            seen[at] = true;
        }
        assert!(seen.iter().all(|&s| s));
        // The same slot one grid width over holds another probe: another code.
        let cell = grid.cell(5);
        let next = cell + IVec3::new(4, 0, 0);
        assert_eq!(grid.slot(next), 5);
        assert_ne!(grid.code(next), grid.code(cell));
        assert!(grid.code(cell) >= 1.0 && grid.code(IVec3::splat(-1000)) <= 512.0);
    }

    #[test]
    fn new_probes_come_first_nearest_first_then_the_oldest() {
        let mut probes = GiProbes::default();
        let mut out = Vec::new();
        let counts = [5, 3, 5];
        let camera = Vec3::new(0.3, 0.0, 0.2);
        let grid = probes.plan(counts, 1.0, camera, 4, &mut out);
        assert_eq!(grid.origin, IVec3::new(-2, -1, -2));
        assert_eq!(out.len(), 4);
        // The probe at the camera first, all new (nothing to keep).
        assert_eq!(out[0].position, Vec3::ZERO);
        assert!(out.iter().all(|u| u.keep == 0.0));
        let d = |u: &GiUpdate| u.position.distance(camera);
        assert!(out.windows(2).all(|w| d(&w[0]) <= d(&w[1])));
        // All 75 probes get a turn before any looks again.
        let mut updated = std::collections::HashSet::new();
        updated.extend(out.iter().map(|u| u.slot));
        for _ in 0..18 {
            probes.plan(counts, 1.0, camera, 4, &mut out);
            updated.extend(out.iter().filter(|u| u.keep == 0.0).map(|u| u.slot));
        }
        assert_eq!(updated.len(), 75);
        // Then the oldest, keeping some of their light.
        probes.plan(counts, 1.0, camera, 4, &mut out);
        assert!(out.iter().all(|u| u.keep == GI_KEEP));
        // Moving one cell along x: only the new slice is missing.
        probes.plan(counts, 1.0, camera + Vec3::X, 15, &mut out);
        let fresh = out.iter().filter(|u| u.keep == 0.0).count();
        assert_eq!(fresh, 15);
        assert!(out.iter().all(|u| u.position.x == 3.0));
    }

    #[test]
    fn batch_views_hold_everything_the_probes_faces_see() {
        let mut gi = GiFrame {
            near: 0.05,
            ..Default::default()
        };
        let (lo, hi) = (Vec3::new(-9.0, -5.0, -9.0), Vec3::new(9.0, 7.0, 9.0));
        // Probes spread apart, as the oldest ones are, not only clustered.
        for (i, p) in [vec3(0.0, 0.0, 0.0), vec3(4.0, 2.0, -3.0), vec3(-6.0, -1.0, 5.0)]
            .into_iter()
            .enumerate()
        {
            gi.updates.push(GiUpdate {
                slot: i as u32,
                position: p,
                code: 1.0,
                keep: 0.0,
            });
        }
        gi.plan_batch_views(lo, hi);
        assert_eq!(
            gi.batch_views.len(),
            GI_BATCH_FACES + 6 * 3,
            "the sun's, each face direction's, then each probe face's"
        );
        let inside = |m: Mat4, p: Vec3| {
            let c = m * p.extend(1.0);
            let slack = 1e-3 * c.w.abs() + 1e-4;
            c.w > 0.0 && c.x.abs() <= c.w + slack && c.y.abs() <= c.w + slack && c.z >= -slack && c.z <= c.w + slack
        };
        let mut seed = 3u32;
        let mut random = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1 << 24) as f32
        };
        let mut seen = 0;
        for _ in 0..20_000 {
            let p = lo + (hi - lo) * Vec3::new(random(), random(), random());
            for (k, u) in gi.updates.iter().enumerate() {
                for (face, (forward, up)) in GI_FACES.iter().enumerate() {
                    let d = p - u.position;
                    let ahead = d.dot(*forward);
                    let sees =
                        ahead >= gi.near && d.dot(forward.cross(*up)).abs() <= ahead && d.dot(*up).abs() <= ahead;
                    if sees {
                        seen += 1;
                        assert!(
                            inside(gi.batch_views[GI_BATCH_FACES + 6 * k + face].0, p),
                            "face {face} of the probe at {} sees {p}",
                            u.position
                        );
                        assert!(
                            inside(gi.batch_views[1 + face].0, p),
                            "the view of every face {face} holds what the probe at {} sees: {p}",
                            u.position
                        );
                    }
                }
            }
        }
        assert!(seen > 10_000, "the faces see the box: {seen}");
        // No probe updates: nothing to cull for.
        gi.updates.clear();
        gi.plan_batch_views(lo, hi);
        assert!(gi.batch_views.is_empty());
    }

    #[test]
    fn changing_the_grid_forgets_its_probes() {
        let mut probes = GiProbes::default();
        let mut out = Vec::new();
        for _ in 0..10 {
            probes.plan([2, 2, 2], 1.0, Vec3::ZERO, 4, &mut out);
        }
        assert!(out.iter().all(|u| u.keep > 0.0));
        probes.plan([2, 2, 2], 2.0, Vec3::ZERO, 4, &mut out);
        assert!(out.iter().all(|u| u.keep == 0.0));
    }
}
