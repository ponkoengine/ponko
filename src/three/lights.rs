//! Punctual lights and retained, conservative CPU froxel lists.
use super::shadow_atlas::{MIN_TILE, Tile, fit, floor_pow2, pack, round_pow2};
use super::{Camera3D, Transform3D, linear};
use crate::color::Color;
use glam::{Mat4, Vec3};
use ponko_ecs::Component;
use ponko_ecs::World;

/// Omnidirectional light at its entity's [`Transform3D::position`]. Scale is ignored.
/// Intensity is linear radiant intensity; distance attenuation is inverse-square,
/// softened within one world unit and smoothly windowed to zero at `range`.
/// Invalid/nonpositive ranges or intensities disable the light.
/// ```
/// use ponko::prelude::*;
/// let lamp = ponko::d3::PointLight { intensity: 20.0, range: 12.0, ..Default::default() };
/// let mut world = World::new();
/// world.spawn((ponko::d3::Transform::at(vec3(0.0, 3.0, 0.0)), lamp));
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Component)]
#[serde(default)]
pub struct PointLight3D {
    /// sRGB color (alpha ignored).
    pub color: Color,
    /// Linear radiant intensity. Default 1.
    pub intensity: f32,
    /// Reach in world units. Default 10; unaffected by transform scale.
    pub range: f32,
    /// Cast shadows (six shadow views: one per cube face). Default false.
    /// Shadowed lights nearest the camera get views first; see
    /// [`PointLight::MAX_SHADOW_VIEWS`](crate::d3::PointLight::MAX_SHADOW_VIEWS). Lights left over shine unshadowed.
    pub shadows: bool,
    /// Width and height of each of its shadow views in the shadow atlas, in
    /// texels (rounded down to a power of two, 64 to 2048). 0, the default,
    /// sizes them by how big the light looks from the camera, up to
    /// [`Lighting::light_shadow_resolution`](crate::render::Lighting::light_shadow_resolution).
    /// When the atlas is full, the largest views shrink first.
    pub shadow_resolution: u32,
}
impl Default for PointLight3D {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            intensity: 1.0,
            range: 10.0,
            shadows: false,
            shadow_resolution: 0,
        }
    }
}

/// Cone light pointing along its entity's rotated local **-Z** (its forward).
/// Angles are cone **half angles in radians**, `0 <= inner <= outer < pi/2`.
/// Equal angles give a hard cone edge. Invalid angles disable the light.
/// Position, intensity, range, scale and shadows follow [`PointLight3D`]; a
/// shadowed spot light needs one shadow view.
/// ```
/// use ponko::prelude::*;
/// let beam = ponko::d3::SpotLight { inner_angle: 0.2, outer_angle: 0.5, ..Default::default() };
/// let mut world = World::new();
/// world.spawn((ponko::d3::Transform::default(), beam));
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Component)]
#[serde(default)]
pub struct SpotLight3D {
    /// sRGB color (alpha ignored).
    pub color: Color,
    /// Linear radiant intensity. Default 1.
    pub intensity: f32,
    /// Reach in world units. Default 10.
    pub range: f32,
    /// Fully lit half angle in radians. Default 0.3.
    pub inner_angle: f32,
    /// Unlit beyond this half angle, radians. Default 0.5.
    pub outer_angle: f32,
    /// Cast shadows (one shadow view). Default false.
    pub shadows: bool,
    /// Its shadow view's size in the shadow atlas; see
    /// [`PointLight3D::shadow_resolution`]. Default 0: automatic.
    pub shadow_resolution: u32,
}
impl Default for SpotLight3D {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            intensity: 1.0,
            range: 10.0,
            inner_angle: 0.3,
            outer_angle: 0.5,
            shadows: false,
            shadow_resolution: 0,
        }
    }
}

/// The most shadow views point and spot lights can share each frame (see
/// [`Lighting::light_shadow_views`](crate::render::Lighting::light_shadow_views),
/// 12 by default): a shadowed spot light takes one, a shadowed point light
/// six. Each view is a square tile of the lights' shadow atlas.
/// ```
/// assert_eq!(ponko::d3::PointLight::MAX_SHADOW_VIEWS, 24);
/// ```
pub const MAX_LIGHT_SHADOW_VIEWS: usize = 24;
/// Default width and height of the largest point or spot light shadow
/// view, in texels (see [`Lighting::light_shadow_resolution`](crate::render::Lighting)):
/// what a light near the camera gets.
/// ```
/// assert_eq!(ponko::d3::PointLight::SHADOW_SIZE, 1024);
/// ```
pub const LIGHT_SHADOW_SIZE: u32 = 1024;
/// Default width and height of the depth atlas every point and spot light
/// shadow view is packed into, in texels (see
/// [`Lighting::light_shadow_atlas`](crate::render::Lighting::light_shadow_atlas)).
/// ```
/// assert_eq!(ponko::d3::PointLight::SHADOW_ATLAS_SIZE, 4096);
/// ```
pub const LIGHT_SHADOW_ATLAS_SIZE: u32 = 4096;
/// Smallest and largest point or spot light shadow view a game may ask for.
pub(crate) const MIN_LIGHT_SHADOW_RESOLUTION: u32 = 128;
pub(crate) const MAX_LIGHT_SHADOW_RESOLUTION: u32 = 2048;

pub(crate) const GRID: [usize; 3] = [16, 9, 24];
const CELLS: usize = GRID[0] * GRID[1] * GRID[2];
pub(crate) const LIST_WIDTH: usize = 1024;

/// Four RGBA32Float texels, one row per light.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuLight {
    pub position_range: [f32; 4],
    pub color: [f32; 4],
    pub direction_outer: [f32; 4],
    pub inner: [f32; 4],
}

#[derive(Default)]
pub(crate) struct LightGrid {
    pub lights: Vec<GpuLight>,
    /// World to each light shadow view's clip space: a spot light's one
    /// view, or a point light's six faces (+X, -X, +Y, -Y, +Z, -Z).
    pub shadow_views: Vec<Mat4>,
    /// Position of the light owning each shadow view, for distance culling.
    pub shadow_positions: Vec<Vec3>,
    /// Each shadow view's tile in the atlas, in texels.
    pub shadow_tiles: Vec<Tile>,
    /// The atlas's width and height in texels, as of the last `collect`.
    pub atlas: u32,
    /// Shadowed lights by priority: (camera distance to the light's reach,
    /// point (0) or spot (1), index in that query, tile side).
    candidates: Vec<(f32, u8, u32, u32)>,
    /// Scratch for fitting and packing the tiles.
    fitting: Vec<(u32, u32)>,
    sides: Vec<u32>,
    order: Vec<u32>,
    /// The first shadow view of each point light, then each spot light, in
    /// query order (-1: unshadowed).
    first_view: Vec<i32>,
    /// R32Uint texels: (offset, count) for each cluster, then packed indices.
    pub lists: Vec<u32>,
    /// near, inverse log depth span times slice count, padding.
    pub depth: [f32; 4],
    bounds: Vec<([usize; 3], [usize; 3])>,
    counts: Vec<u32>,
    cursors: Vec<u32>,
}

impl LightGrid {
    /// Lists the lights each cluster sees. Shadowed lights nearest the
    /// camera also get shadow views, up to `budget` of them (0: none), each
    /// a tile of an `atlas`² shadow atlas up to `size` texels wide.
    pub fn collect(&mut self, world: &mut World, camera: &Camera3D, budget: usize, size: u32, atlas: u32) {
        self.lights.clear();
        self.bounds.clear();
        self.atlas = floor_pow2(atlas.max(MIN_TILE));
        self.plan_shadows(world, camera, budget, floor_pow2(size.max(MIN_TILE)).min(self.atlas));
        let first_view = std::mem::take(&mut self.first_view);
        let mut first_views = first_view.iter().copied();
        let near = camera.near.max(f32::MIN_POSITIVE);
        let far = camera.far.max(near * 1.001);
        self.depth = [near, GRID[2] as f32 / (far / near).ln(), 0.0, 0.0];
        let view = camera.view();
        let proj = camera.view_proj() * view.inverse();
        for (t, p) in world.query::<(&Transform3D, &PointLight3D)>().iter() {
            let first = first_views.next().unwrap_or(-1);
            self.add(t, *p, None, first, view, proj, far);
        }
        for (t, s) in world.query::<(&Transform3D, &SpotLight3D)>().iter() {
            let first = first_views.next().unwrap_or(-1);
            if !s.inner_angle.is_finite()
                || !s.outer_angle.is_finite()
                || s.inner_angle < 0.0
                || s.outer_angle < s.inner_angle
                || s.outer_angle >= std::f32::consts::FRAC_PI_2
            {
                continue;
            }
            self.add(
                t,
                PointLight3D {
                    color: s.color,
                    intensity: s.intensity,
                    range: s.range,
                    shadows: s.shadows,
                    shadow_resolution: s.shadow_resolution,
                },
                Some(*s),
                first,
                view,
                proj,
                far,
            );
        }
        self.first_view = first_view;
        self.counts.resize(CELLS, 0);
        self.counts.fill(0);
        for &(lo, hi) in &self.bounds {
            visit(lo, hi, |c| self.counts[c] += 1);
        }
        let total: usize = self.counts.iter().map(|&n| n as usize).sum();
        let used = CELLS * 2 + total;
        self.lists.resize(used.div_ceil(LIST_WIDTH) * LIST_WIDTH, 0);
        self.cursors.resize(CELLS, 0);
        let mut offset = CELLS as u32 * 2;
        for c in 0..CELLS {
            self.lists[c * 2] = offset;
            self.lists[c * 2 + 1] = self.counts[c];
            self.cursors[c] = offset;
            offset += self.counts[c];
        }
        for (id, &(lo, hi)) in self.bounds.iter().enumerate() {
            visit(lo, hi, |c| {
                self.lists[self.cursors[c] as usize] = id as u32;
                self.cursors[c] += 1;
            });
        }
    }

    /// Gives shadowed lights shadow views, nearest the camera first, while
    /// views are left, sizes each light's tiles (see [`tile_side`]), fits and
    /// packs them into the atlas, and works out each view's matrix. Fills
    /// `first_view` for every point light, then every spot light, in query
    /// order.
    fn plan_shadows(&mut self, world: &mut World, camera: &Camera3D, budget: usize, max_side: u32) {
        let shadows = budget > 0;
        self.shadow_views.clear();
        self.shadow_positions.clear();
        self.shadow_tiles.clear();
        self.candidates.clear();
        self.first_view.clear();
        let reach = |t: &Transform3D, range: f32| t.position.distance(camera.position) - range;
        let side = |t: &Transform3D, range: f32, hint: u32| tile_side(t.position, range, hint, camera, max_side);
        for (i, (t, p)) in world.query::<(&Transform3D, &PointLight3D)>().iter().enumerate() {
            self.first_view.push(-1);
            if shadows && p.shadows && shadow_light_ok(t, p.range, p.intensity) {
                let side = side(t, p.range, p.shadow_resolution);
                self.candidates.push((reach(t, p.range), 0, i as u32, side));
            }
        }
        let points = self.first_view.len();
        for (i, (t, s)) in world.query::<(&Transform3D, &SpotLight3D)>().iter().enumerate() {
            self.first_view.push(-1);
            if shadows
                && s.shadows
                && shadow_light_ok(t, s.range, s.intensity)
                && s.outer_angle.is_finite()
                && s.outer_angle > 0.0
                && s.outer_angle < std::f32::consts::FRAC_PI_2
            {
                let side = side(t, s.range, s.shadow_resolution);
                self.candidates.push((reach(t, s.range), 1, i as u32, side));
            }
        }
        if self.candidates.is_empty() {
            return;
        }
        self.candidates
            .sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut taken = 0;
        let mut chosen = std::mem::take(&mut self.candidates);
        chosen.retain(|&(_, kind, _, _)| {
            let views = if kind == 0 { 6 } else { 1 };
            if taken + views > budget.min(MAX_LIGHT_SHADOW_VIEWS) {
                return false;
            }
            taken += views;
            true
        });
        // Then shrink tiles (or drop the least important lights) until
        // they fit the atlas.
        self.fitting.clear();
        self.fitting.extend(
            chosen
                .iter()
                .map(|&(_, kind, _, side)| (side, if kind == 0 { 6 } else { 1 })),
        );
        let kept = fit(&mut self.fitting, self.atlas);
        chosen.truncate(kept);
        for (c, &(side, _)) in chosen.iter_mut().zip(&self.fitting) {
            c.3 = side;
        }
        // Then take them in query order, so matrices come from one pass per kind.
        chosen.sort_unstable_by_key(|&(_, kind, i, _)| (kind, i));
        self.sides.clear();
        let mut next = chosen.iter().peekable();
        for (i, (t, p)) in world.query::<(&Transform3D, &PointLight3D)>().iter().enumerate() {
            if let Some(c) = next.next_if(|c| c.1 == 0 && c.2 == i as u32) {
                self.first_view[i] = self.shadow_views.len() as i32;
                self.shadow_views.extend(point_shadow_views(t.position, p.range));
                self.shadow_positions.extend([t.position; 6]);
                self.sides.extend([c.3; 6]);
            }
        }
        for (i, (t, s)) in world.query::<(&Transform3D, &SpotLight3D)>().iter().enumerate() {
            if let Some(c) = next.next_if(|c| c.1 == 1 && c.2 == i as u32) {
                self.first_view[points + i] = self.shadow_views.len() as i32;
                self.shadow_views.push(spot_shadow_view(t, s));
                self.shadow_positions.push(t.position);
                self.sides.push(c.3);
            }
        }
        pack(&self.sides, self.atlas, &mut self.order, &mut self.shadow_tiles);
        self.candidates = chosen;
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        t: &Transform3D,
        p: PointLight3D,
        spot: Option<SpotLight3D>,
        first_view: i32,
        view: Mat4,
        proj: Mat4,
        far: f32,
    ) {
        if !t.position.is_finite()
            || !p.range.is_finite()
            || p.range <= 0.0
            || !p.intensity.is_finite()
            || p.intensity <= 0.0
        {
            return;
        }
        let rgba = linear(p.color);
        if !rgba[..3].iter().all(|v| v.is_finite() && *v >= 0.0) {
            return;
        }
        let center = view.transform_point3(t.position);
        let z = -center.z;
        let r = p.range;
        let near = self.depth[0];
        if z + r < near || z - r > far {
            return;
        }
        let mut lo = [0; 3];
        let mut hi = [GRID[0] - 1, GRID[1] - 1, GRID[2] - 1];
        // Project an enclosing view-space box. If it crosses the near plane,
        // cover all XY tiles: dividing by a behind-camera corner would miss lights.
        if z - r > near {
            for axis in 0..2 {
                let scale = if axis == 0 { proj.x_axis.x } else { proj.y_axis.y };
                let a = (center[axis] - r) * scale;
                let b = (center[axis] + r) * scale;
                let min = (a / (z - r)).min(a / (z + r));
                let max = (b / (z - r)).max(b / (z + r));
                if min > 1.0 || max < -1.0 {
                    return;
                }
                let tile = |v: f32| {
                    (((v * 0.5 + 0.5) * GRID[axis] as f32).floor() as i32).clamp(0, GRID[axis] as i32 - 1) as usize
                };
                lo[axis] = tile(min);
                hi[axis] = tile(max);
            }
        }
        let slice = |d: f32| {
            ((d.max(near) / near).ln() * self.depth[1])
                .floor()
                .clamp(0.0, (GRID[2] - 1) as f32) as usize
        };
        lo[2] = slice(z - r);
        hi[2] = slice(z + r);
        let direction = t.forward().try_normalize().unwrap_or(Vec3::NEG_Z);
        if spot.is_some() && (!t.rotation.is_finite() || t.rotation.length_squared() < 1e-8) {
            return;
        }
        let outer = spot.map_or(-2.0, |s| s.outer_angle.cos());
        let side = usize::try_from(first_view)
            .ok()
            .and_then(|v| self.shadow_tiles.get(v))
            .map_or(1, |t| t[2].max(1));
        self.lights.push(GpuLight {
            position_range: t.position.extend(r).to_array(),
            color: [rgba[0] * p.intensity, rgba[1] * p.intensity, rgba[2] * p.intensity, 0.0],
            direction_outer: direction.extend(outer).to_array(),
            // y: the first shadow view (-1: none); z: its world texel size
            // per unit of distance from the light (a cube face's, or the cone's).
            inner: [
                spot.map_or(1.0, |s| s.inner_angle.cos()),
                first_view as f32,
                2.0 * (shadow_fov(spot.map(|s| s.outer_angle)) * 0.5).tan() / side as f32,
                0.0,
            ],
        });
        self.bounds.push((lo, hi));
    }
}
/// How wide a light's shadow tiles are: its `hint` when it has one, else
/// `max_side` times how much of the screen's height the light's reach can
/// cover from the camera (all of it from inside the reach), to the nearest
/// power of two. So a light twice as far away gets tiles half as wide.
fn tile_side(position: Vec3, range: f32, hint: u32, camera: &Camera3D, max_side: u32) -> u32 {
    if hint > 0 {
        return floor_pow2(hint.clamp(MIN_TILE, MAX_LIGHT_SHADOW_RESOLUTION));
    }
    let distance = position.distance(camera.position);
    let half_height = (camera.fov_y * 0.5).tan().max(1e-3);
    let cover = if distance <= range {
        1.0
    } else {
        (range / (distance * half_height)).min(1.0)
    };
    round_pow2(max_side as f32 * cover).clamp(MIN_TILE, max_side.max(MIN_TILE))
}

fn shadow_light_ok(t: &Transform3D, range: f32, intensity: f32) -> bool {
    t.position.is_finite() && range.is_finite() && range > 0.0 && intensity.is_finite() && intensity > 0.0
}

/// A shadow view's field of view: a spot light's cone, or a cube face's 90
/// degrees, both a little wider so filtering at the edges stays inside.
fn shadow_fov(outer_angle: Option<f32>) -> f32 {
    outer_angle.map_or(std::f32::consts::FRAC_PI_2, |a| 2.0 * a) + 0.04
}

/// Near plane of a light's shadow views: small, but not so small that depth
/// precision runs out over its range.
fn shadow_near(range: f32) -> f32 {
    (range * 0.002).max(0.02)
}

fn spot_shadow_view(t: &Transform3D, s: &SpotLight3D) -> Mat4 {
    let forward = t.forward().try_normalize().unwrap_or(Vec3::NEG_Z);
    let up = if forward.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let projection = glam::camera::rh::proj::directx::perspective(
        shadow_fov(Some(s.outer_angle)),
        1.0,
        shadow_near(s.range),
        s.range,
    );
    projection * glam::camera::rh::view::look_to_mat4(t.position, forward, up)
}

/// A point light's six cube-face views: +X, -X, +Y, -Y, +Z, -Z.
pub(crate) fn point_shadow_views(position: Vec3, range: f32) -> [Mat4; 6] {
    let projection = glam::camera::rh::proj::directx::perspective(shadow_fov(None), 1.0, shadow_near(range), range);
    let faces = [
        (Vec3::X, Vec3::Y),
        (Vec3::NEG_X, Vec3::Y),
        (Vec3::Y, Vec3::Z),
        (Vec3::NEG_Y, Vec3::Z),
        (Vec3::Z, Vec3::Y),
        (Vec3::NEG_Z, Vec3::Y),
    ];
    faces.map(|(dir, up)| projection * glam::camera::rh::view::look_to_mat4(position, dir, up))
}

fn visit(lo: [usize; 3], hi: [usize; 3], mut f: impl FnMut(usize)) {
    for z in lo[2]..=hi[2] {
        for y in lo[1]..=hi[1] {
            for x in lo[0]..=hi[0] {
                f((z * GRID[1] + y) * GRID[0] + x);
            }
        }
    }
}

impl PointLight3D {
    /// Maximum simultaneous shadow views.
    pub const MAX_SHADOW_VIEWS: usize = MAX_LIGHT_SHADOW_VIEWS;
    /// Default local shadow resolution.
    pub const SHADOW_SIZE: u32 = LIGHT_SHADOW_SIZE;
    /// Default local shadow atlas resolution.
    pub const SHADOW_ATLAS_SIZE: u32 = LIGHT_SHADOW_ATLAS_SIZE;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    struct Counting;
    thread_local! { static ALLOCS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
    // Marked code (see `marked`): whether this thread is in it now, how
    // often it was entered, and how often it allocated, while counting.
    thread_local! { static MARKED: std::cell::Cell<(bool, usize, usize)> = const { std::cell::Cell::new((false, 0, 0)) }; }
    fn count() {
        let _ = ALLOCS.try_with(|n| {
            if let Some(v) = n.get() {
                n.set(Some(v + 1));
                let _ = MARKED.try_with(|m| {
                    let (on, entered, allocations) = m.get();
                    m.set((on, entered, allocations + usize::from(on)));
                });
            }
        });
    }
    // SAFETY: delegates every allocation unchanged to the system allocator.
    unsafe impl std::alloc::GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
            count();
            // SAFETY: forwarded allocation contract.
            unsafe { std::alloc::System.alloc(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
            count();
            // SAFETY: forwarded allocation contract.
            unsafe { std::alloc::System.realloc(ptr, layout, size) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
            // SAFETY: forwarded allocation contract.
            unsafe { std::alloc::System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    /// Restores what [`marked`] or [`unmarked`] changed.
    pub(crate) struct Mark(bool);
    impl Drop for Mark {
        fn drop(&mut self) {
            MARKED.with(|m| m.set((self.0, m.get().1, m.get().2)));
        }
    }
    fn mark(on: bool) -> Mark {
        MARKED.with(|m| {
            let (was, entered, allocations) = m.get();
            m.set((on, entered + usize::from(on && !was), allocations));
            Mark(was)
        })
    }
    /// Until the guard drops, this thread's allocations count as made by
    /// marked code: a test can then tell a module's own allocations from
    /// those of the libraries it calls, without reading call stacks (which
    /// need debug info the CI builds leave out).
    pub(crate) fn marked() -> Mark {
        mark(true)
    }
    /// Until the guard drops, allocations don't count as marked code's: put
    /// around a call into a library from marked code.
    pub(crate) fn unmarked() -> Mark {
        mark(false)
    }
    /// Runs `f` and returns how often this thread allocated (or
    /// reallocated) in it, how often it entered marked code, and how many
    /// of the allocations marked code made itself.
    pub(crate) fn count_marked_allocations<R>(f: impl FnOnce() -> R) -> (R, [usize; 3]) {
        MARKED.with(|m| m.set((false, 0, 0)));
        let (result, all) = count_allocations(f);
        let (_, entered, marked) = MARKED.with(|m| m.get());
        (result, [all, entered, marked])
    }

    /// Runs `f` and returns how often it allocated (or reallocated) on
    /// this thread: the lib tests' counting allocator, for other modules.
    pub(crate) fn count_allocations<R>(f: impl FnOnce() -> R) -> (R, usize) {
        ALLOCS.with(|n| n.set(Some(0)));
        let result = f();
        (result, ALLOCS.with(|n| n.replace(None).unwrap()))
    }

    fn camera() -> Camera3D {
        let mut c = Camera3D::new(glam::Vec2::new(800.0, 600.0));
        c.position = Vec3::ZERO;
        c.rotation = glam::Quat::IDENTITY;
        c.far = 100.0;
        c
    }
    #[test]
    fn culls_outside_and_keeps_near_crossing_lights() {
        let mut w = World::new();
        for at in [
            Vec3::new(0.0, 0.0, -5.0),
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 30.0),
            Vec3::new(100.0, 0.0, -5.0),
            Vec3::new(0.0, 0.0, -150.0),
        ] {
            w.spawn((
                Transform3D::at(at),
                PointLight3D {
                    range: 2.0,
                    ..Default::default()
                },
            ));
        }
        let mut g = LightGrid::default();
        g.collect(&mut w, &camera(), 0, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        assert_eq!(g.lights.len(), 2);
        assert_eq!(g.bounds[1].0[..2], [0, 0]);
        assert_eq!(g.bounds[1].1[..2], [15, 8]);
    }
    #[test]
    fn overlapping_lights_are_not_truncated_and_capacity_is_retained() {
        let mut w = World::new();
        for _ in 0..1000 {
            w.spawn((Transform3D::default(), PointLight3D::default()));
        }
        let mut g = LightGrid::default();
        g.collect(&mut w, &camera(), 0, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        assert_eq!(g.lists[1], 1000);
        let ptrs = (
            g.lights.as_ptr(),
            g.lists.as_ptr(),
            g.bounds.as_ptr(),
            g.counts.as_ptr(),
            g.cursors.as_ptr(),
        );
        ALLOCS.with(|n| n.set(Some(0)));
        for _ in 0..20 {
            g.collect(&mut w, &camera(), 0, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        }
        let allocations = ALLOCS.with(|n| n.replace(None).unwrap());
        assert_eq!(allocations, 0, "light collection must not allocate once warm");
        g.collect(&mut w, &camera(), 0, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        assert_eq!(
            ptrs,
            (
                g.lights.as_ptr(),
                g.lists.as_ptr(),
                g.bounds.as_ptr(),
                g.counts.as_ptr(),
                g.cursors.as_ptr()
            )
        );
    }
    #[test]
    fn decal_collection_does_not_allocate_once_warm() {
        use crate::three::projected::{Decal3D, DecalGrid};
        let mut w = World::new();
        for i in 0..500 {
            w.spawn((
                Transform3D::at(Vec3::new((i % 10) as f32 - 5.0, 0.0, -5.0 - (i / 10) as f32 * 0.5)),
                Decal3D {
                    order: i % 7,
                    ..Default::default()
                },
            ));
        }
        let c = camera();
        let depth = [c.near, GRID[2] as f32 / (c.far / c.near).ln(), 0.0, 0.0];
        let mut g = DecalGrid::default();
        g.collect(&mut w, &c, depth, 1, 8192, 3);
        assert!(!g.records.is_empty());
        ALLOCS.with(|n| n.set(Some(0)));
        for _ in 0..20 {
            g.collect(&mut w, &c, depth, 1, 8192, 3);
        }
        let allocations = ALLOCS.with(|n| n.replace(None).unwrap());
        assert_eq!(allocations, 0, "decal collection must not allocate once warm");
    }
    #[test]
    fn tiny_camera_near_plane_keeps_tiny_near_lights() {
        let mut c = camera();
        c.near = 1e-6;
        let mut w = World::new();
        w.spawn((
            Transform3D::at(Vec3::new(0.0, 0.0, -2e-5)),
            PointLight3D {
                range: 1e-5,
                ..Default::default()
            },
        ));
        let mut g = LightGrid::default();
        g.collect(&mut w, &c, 0, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        assert_eq!(g.lights.len(), 1);
    }

    #[test]
    fn every_lit_visible_sample_has_its_light_in_the_cluster() {
        let c = camera();
        let mut w = World::new();
        for i in 0..80 {
            let position = Vec3::new((i % 9) as f32 * 3.1 - 12.0, (i % 5) as f32 - 2.0, -(i as f32 * 1.1));
            w.spawn((
                Transform3D::at(position),
                PointLight3D {
                    range: 4.0,
                    ..Default::default()
                },
            ));
        }
        let mut g = LightGrid::default();
        g.collect(&mut w, &c, 0, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        for (id, light) in g.lights.iter().enumerate() {
            let center = Vec3::from_slice(&light.position_range);
            for dx in [-2.0, 0.0, 2.0] {
                for dy in [-2.0, 0.0, 2.0] {
                    for dz in [-2.0, 0.0, 2.0] {
                        let p = center + Vec3::new(dx, dy, dz);
                        let clip = c.view_proj() * p.extend(1.0);
                        let ndc = clip.truncate() / clip.w;
                        if clip.w <= 0.0 || ndc.x.abs() >= 1.0 || ndc.y.abs() >= 1.0 || ndc.z <= 0.0 || ndc.z > 1.0 {
                            continue;
                        }
                        let x = ((ndc.x * 0.5 + 0.5) * GRID[0] as f32) as usize;
                        let y = ((ndc.y * 0.5 + 0.5) * GRID[1] as f32) as usize;
                        let z = (((clip.w / g.depth[0]).ln() * g.depth[1]) as usize).min(GRID[2] - 1);
                        let cell = (z * GRID[1] + y) * GRID[0] + x;
                        let offset = g.lists[cell * 2] as usize;
                        let count = g.lists[cell * 2 + 1] as usize;
                        assert!(
                            g.lists[offset..offset + count].contains(&(id as u32)),
                            "light {id}, sample {p:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_lights_are_disabled_and_spots_rotate() {
        let mut w = World::new();
        for range in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            w.spawn((
                Transform3D::default(),
                PointLight3D {
                    range,
                    ..Default::default()
                },
            ));
        }
        w.spawn((
            Transform3D::default(),
            SpotLight3D {
                inner_angle: 1.0,
                outer_angle: 0.5,
                ..Default::default()
            },
        ));
        w.spawn((
            Transform3D::default().rotated(glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
            SpotLight3D::default(),
        ));
        let mut g = LightGrid::default();
        g.collect(&mut w, &camera(), 0, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        assert_eq!(g.lights.len(), 1);
        assert!((Vec3::from_slice(&g.lights[0].direction_outer) - Vec3::NEG_X).length() < 1e-5);
    }

    #[test]
    fn nearest_shadowed_lights_get_views_until_they_run_out() {
        use glam::vec3;
        use ponko_ecs::Entity;
        let mut w = World::new();
        let shadowed = PointLight3D {
            intensity: 5.0,
            shadows: true,
            ..Default::default()
        };
        // Nearest first: with a budget of 12, the point at z -60 doesn't
        // fit after two points, and the far spot doesn't either.
        let far = w.spawn((Transform3D::at(vec3(0.0, 0.0, -60.0)), shadowed));
        let near = w.spawn((Transform3D::at(vec3(0.0, 0.0, -15.0)), shadowed));
        let nearer = w.spawn((Transform3D::at(vec3(0.0, 0.0, -12.0)), shadowed));
        w.spawn((Transform3D::at(vec3(0.0, 0.0, -5.0)), PointLight3D::default())); // no shadows
        let spot = SpotLight3D {
            shadows: true,
            ..Default::default()
        };
        w.spawn((Transform3D::at(vec3(0.0, 0.0, -70.0)), spot));
        let mut g = LightGrid::default();
        g.collect(&mut w, &camera(), 12, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        assert_eq!(g.shadow_views.len(), 12);
        let order: Vec<Entity> = w.query::<(Entity, &PointLight3D)>().iter().map(|(e, _)| e).collect();
        let first = |e| g.lights[order.iter().position(|&x| x == e).unwrap()].inner[1];
        assert_eq!(first(far), -1.0, "no views left for the farthest point light");
        assert!(first(near) >= 0.0 && first(nearer) >= 0.0);
        assert_eq!(g.lights.iter().filter(|l| l.inner[1] >= 0.0).count(), 2);
        // Without shadows on (or in other views), no light gets views.
        g.collect(&mut w, &camera(), 0, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        assert!(g.shadow_views.is_empty() && g.lights.iter().all(|l| l.inner[1] < 0.0));
        assert!(g.shadow_positions.is_empty());
        // With room, a shadowed spot light takes one.
        w.despawn(far);
        w.despawn(near);
        g.collect(&mut w, &camera(), 12, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        assert_eq!(g.shadow_views.len(), 7);
        assert_eq!(g.shadow_positions.len(), 7);
        assert_eq!(&g.shadow_positions[..6], &[vec3(0.0, 0.0, -12.0); 6]);
        assert_eq!(g.shadow_positions[6], vec3(0.0, 0.0, -70.0));
    }

    #[test]
    fn shadow_tiles_shrink_with_distance_and_follow_hints() {
        use glam::vec3;
        let mut w = World::new();
        let spot = |z: f32, hint: u32| {
            (
                Transform3D::at(vec3(0.0, 0.0, z)),
                SpotLight3D {
                    range: 5.0,
                    shadows: true,
                    shadow_resolution: hint,
                    ..Default::default()
                },
            )
        };
        // Inside its reach, 3 x its reach away, 12 x, far, and far with a hint.
        let lights = [
            spot(-2.0, 0),
            spot(-15.0, 0),
            spot(-60.0, 0),
            spot(-900.0, 0),
            spot(-900.0, 700),
        ];
        for light in lights {
            w.spawn(light);
        }
        let mut g = LightGrid::default();
        g.collect(&mut w, &camera(), 12, LIGHT_SHADOW_SIZE, LIGHT_SHADOW_ATLAS_SIZE);
        let sides: Vec<u32> = g.shadow_tiles.iter().map(|t| t[2]).collect();
        // 1024 * 5 / (15 tan 30°) = 591, 1024 * 5 / (60 tan 30°) = 148.
        assert_eq!(sides, [1024, 512, 128, MIN_TILE, 512]);
        // A light's texel size follows its tile.
        assert!((g.lights[1].inner[2] / g.lights[0].inner[2] - 2.0).abs() < 1e-4);
        // A smaller atlas holds them too, smaller.
        g.collect(&mut w, &camera(), 12, 2048, 1024);
        assert_eq!(g.atlas, 1024);
        let area: u32 = g.shadow_tiles.iter().map(|t| t[2] * t[2]).sum();
        assert!(area <= 1024 * 1024 && g.shadow_tiles.len() == 5, "{:?}", g.shadow_tiles);
        assert!(
            g.shadow_tiles
                .iter()
                .all(|t| t[0] + t[2] <= 1024 && t[1] + t[2] <= 1024)
        );
    }

    #[test]
    fn shadowed_light_collection_does_not_allocate_once_warm() {
        use glam::vec3;
        let mut w = World::new();
        for i in 0..40 {
            let at = Transform3D::at(vec3((i % 8) as f32 * 3.0 - 12.0, 1.0, -3.0 - i as f32 * 2.0));
            if i % 5 == 0 {
                w.spawn((
                    at,
                    PointLight3D {
                        shadows: true,
                        ..Default::default()
                    },
                ));
            } else {
                w.spawn((
                    at,
                    SpotLight3D {
                        shadows: true,
                        ..Default::default()
                    },
                ));
            }
        }
        let mut g = LightGrid::default();
        g.collect(
            &mut w,
            &camera(),
            MAX_LIGHT_SHADOW_VIEWS,
            LIGHT_SHADOW_SIZE,
            LIGHT_SHADOW_ATLAS_SIZE,
        );
        assert_eq!(g.shadow_views.len(), MAX_LIGHT_SHADOW_VIEWS);
        ALLOCS.with(|n| n.set(Some(0)));
        for _ in 0..20 {
            g.collect(
                &mut w,
                &camera(),
                MAX_LIGHT_SHADOW_VIEWS,
                LIGHT_SHADOW_SIZE,
                LIGHT_SHADOW_ATLAS_SIZE,
            );
        }
        let allocations = ALLOCS.with(|n| n.replace(None).unwrap());
        assert_eq!(allocations, 0, "shadow planning must not allocate once warm");
    }

    #[test]
    fn shadow_views_look_where_their_lights_shine() {
        use glam::vec3;
        let light = vec3(1.0, 2.0, 3.0);
        let faces = point_shadow_views(light, 10.0);
        let dirs = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
        for (m, d) in faces.iter().zip(dirs) {
            let ndc = m.project_point3(light + d * 4.0);
            assert!(ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4, "{d}: {ndc}");
            assert!(ndc.z > 0.0 && ndc.z < 1.0);
            // Behind the face is outside it.
            let back = *m * (light - d * 4.0).extend(1.0);
            assert!(back.w < 0.0);
        }
        let t = Transform3D::at(light).looking_at(light + Vec3::NEG_Y);
        let m = spot_shadow_view(&t, &SpotLight3D::default());
        let ndc = m.project_point3(light + Vec3::NEG_Y * 5.0);
        assert!(ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4 && ndc.z > 0.0 && ndc.z < 1.0);
        // The cone's edge falls inside the view.
        let edge = light + (Vec3::NEG_Y + Vec3::X * 0.5f32.tan()) * 5.0;
        assert!(m.project_point3(edge).x.abs() < 1.0);
    }
}
