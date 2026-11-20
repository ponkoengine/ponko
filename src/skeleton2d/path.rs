//! Path attachments and path constraints, as Spine's: a chain of cubic
//! beziers on bones, and bones placed and turned along it.

use std::sync::Arc;

use glam::{Affine2, Vec2};

use super::data::MeshVertex;

/// A chain of cubic beziers that follows its bones as a mesh does (Spine's
/// path attachment). It draws nothing: a [`PathConstraint`] places bones
/// along it.
///
/// `vertices` holds three per point of the path: the handle before the
/// point, the point, the handle after it. Between two points the curve
/// goes from the first point through its after-handle and the next
/// point's before-handle. An open path runs from its first point to its
/// last (the first before-handle and the last after-handle are unused); a
/// closed one also goes from the last point back to the first.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::skeleton2d::{MeshVertex, PathShape};
/// let on = |x, y| MeshVertex::new(vec2(x, y), Vec2::ZERO, &[(0, 1.0)]);
/// // A straight line from (0, 0) to (30, 0): handles a third of the way.
/// let line = PathShape::new(vec![on(-10.0, 0.0), on(0.0, 0.0), on(10.0, 0.0), on(20.0, 0.0), on(30.0, 0.0), on(40.0, 0.0)]);
/// assert_eq!(line.curves(), 1);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PathShape {
    /// Three per point (before-handle, point, after-handle), at rest, in
    /// skeleton units.
    pub vertices: Vec<MeshVertex>,
    /// It goes from the last point back to the first.
    pub closed: bool,
    /// Positions along it are measured by distance (`true`, the default).
    /// `false`: each curve counts as long as `lengths` says, and a position
    /// inside a curve is its bezier parameter, which is faster and moves
    /// unevenly on curves whose handles aren't evenly spaced.
    pub constant_speed: bool,
    /// With `constant_speed` off: the length from the start to the end of
    /// each curve, at rest (Spine computes them when it exports).
    pub lengths: Vec<f32>,
}

impl PathShape {
    /// An open path through `vertices` (three per point), measured by
    /// distance.
    #[must_use]
    pub fn new(vertices: Vec<MeshVertex>) -> Self {
        Self {
            vertices,
            closed: false,
            constant_speed: true,
            lengths: Vec::new(),
        }
    }

    /// `Ok` if it has whole points (three vertices each), at least two
    /// (one when closed), and a length per curve when not measured by
    /// distance.
    pub(crate) fn check(&self) -> Result<(), String> {
        let points = self.vertices.len() / 3;
        if !self.vertices.len().is_multiple_of(3) || points < if self.closed { 1 } else { 2 } {
            return Err("a path needs three vertices per point (handle, point, handle) and two points or more".into());
        }
        if !self.constant_speed && self.lengths.len() != self.curves() {
            return Err(format!(
                "a path not measured by distance needs one length per curve ({})",
                self.curves()
            ));
        }
        if !self.lengths.iter().all(|l| l.is_finite()) {
            return Err("a path's lengths must be numbers".into());
        }
        Ok(())
    }

    /// How many curves it has: one fewer than its points, or as many when
    /// closed.
    #[must_use]
    pub fn curves(&self) -> usize {
        let points = self.vertices.len() / 3;
        if self.closed { points } else { points.saturating_sub(1) }
    }
}

/// How a [`PathConstraint`]'s `position` is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PathPosition {
    /// In skeleton units from the path's start.
    Fixed,
    /// As a fraction of the path's length (0 to 1).
    #[default]
    Percent,
}

/// How a [`PathConstraint`]'s `spacing` between its bones is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PathSpacing {
    /// Each bone's length plus `spacing` units.
    #[default]
    Length,
    /// `spacing` units, whatever the bones' lengths.
    Fixed,
    /// `spacing` times the path's length.
    Percent,
    /// The bones' lengths in proportion, scaled so the chain spans `spacing`
    /// times the path.
    Proportional,
}

/// How a [`PathConstraint`] turns its bones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PathRotate {
    /// Along the path's direction where each bone sits.
    #[default]
    Tangent,
    /// Toward the next bone's place on the path, each bone starting where
    /// the one before ends (so the chain keeps its lengths).
    Chain,
    /// As `Chain`, but each bone stretches along its length to reach the
    /// next place.
    ChainScale,
}

/// A path constraint (Spine's): its bones are placed along the
/// [`PathShape`] that slot `target` shows, one after another from
/// `position` with `spacing` between them, and turned along it. Applied
/// after animation, in its turn among the other constraints. With no path
/// shown in the slot it does nothing.
///
/// ```
/// use ponko::skeleton2d::{PathConstraint, PathRotate, PathSpacing};
/// let mut follow = PathConstraint::new("snake", &[1, 2, 3], 0);
/// follow.rotate_mode = PathRotate::Chain;
/// follow.spacing_mode = PathSpacing::Length;
/// assert_eq!(follow.mix_rotate, 1.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PathConstraint {
    /// Its name, unique among the path constraints.
    pub name: Arc<str>,
    /// The bones it places, in order along the path.
    pub bones: Vec<usize>,
    /// The slot whose path attachment the bones follow.
    pub target: usize,
    /// How `position` is measured.
    pub position_mode: PathPosition,
    /// How `spacing` is measured.
    pub spacing_mode: PathSpacing,
    /// How the bones turn.
    pub rotate_mode: PathRotate,
    /// Added to each bone's rotation (radians, clockwise on screen).
    pub offset_rotation: f32,
    /// Where the first bone goes along the path.
    pub position: f32,
    /// The gap between bones (see [`PathSpacing`]).
    pub spacing: f32,
    /// How much the bones turn, 0 (none) to 1.
    pub mix_rotate: f32,
    /// How much the bones move onto the path, along x and along y.
    pub mix_translate: Vec2,
}

impl PathConstraint {
    /// A constraint named `name` placing `bones` along the path slot
    /// `target` shows: from its start (percent position 0), bone after bone
    /// by their lengths, turned along the path, fully applied.
    #[must_use]
    pub fn new(name: &str, bones: &[usize], target: usize) -> Self {
        Self {
            name: name.into(),
            bones: bones.to_vec(),
            target,
            position_mode: PathPosition::Percent,
            spacing_mode: PathSpacing::Length,
            rotate_mode: PathRotate::Tangent,
            offset_rotation: 0.0,
            position: 0.0,
            spacing: 0.0,
            mix_rotate: 1.0,
            mix_translate: Vec2::ONE,
        }
    }
}

/// A path constraint's values that clips key: position, spacing, rotate
/// mix, x and y mix.
pub(crate) const PATH_VALUES: usize = 5;

impl PathConstraint {
    pub(crate) fn values(&self) -> [f32; PATH_VALUES] {
        [
            self.position,
            self.spacing,
            self.mix_rotate,
            self.mix_translate.x,
            self.mix_translate.y,
        ]
    }
}

/// Space reused by path constraints every update.
#[derive(Debug, Clone, Default)]
pub(crate) struct PathScratch {
    /// The path's vertices as posed.
    vertices: Vec<Vec2>,
    /// Its vertices in curve order (closed: wrapped round).
    world: Vec<Vec2>,
    spaces: Vec<f32>,
    lengths: Vec<f32>,
    curves: Vec<f32>,
    /// Per place: x, y and the direction there.
    positions: Vec<f32>,
}

impl PathScratch {
    /// The list to fill with the path's vertices as posed.
    pub(crate) fn posed(&mut self) -> &mut Vec<Vec2> {
        self.vertices.clear();
        &mut self.vertices
    }
}

const EPSILON: f32 = 0.00001;

/// Moves `c`'s bones (their skeleton-space matrices in `world`, by index)
/// along `path`, as constraint `c` with keyed values `v` says. Returns
/// `false` if it moved nothing.
/// The path's vertices as posed go in `s.vertices` first
/// ([`PathScratch::posed`]).
pub(crate) fn apply(
    c: &PathConstraint,
    v: [f32; PATH_VALUES],
    lengths_of: impl Fn(usize) -> f32,
    path: &PathShape,
    target_mirrored: bool,
    world: &mut [Affine2],
    s: &mut PathScratch,
) -> bool {
    let [position, spacing, mix_rotate, mix_x, mix_y] = v;
    if mix_rotate == 0.0 && mix_x == 0.0 && mix_y == 0.0 {
        return false;
    }
    if s.vertices.len() < 6 || (!path.constant_speed && path.lengths.len() < path.curves()) {
        return false;
    }
    let tangents = c.rotate_mode == PathRotate::Tangent;
    let scale = c.rotate_mode == PathRotate::ChainScale;
    let bone_count = c.bones.len();
    let spaces_count = if tangents { bone_count } else { bone_count + 1 };
    s.spaces.clear();
    s.spaces.resize(spaces_count, 0.0);
    s.lengths.clear();
    s.lengths.resize(bone_count, 0.0);
    // A bone's length as posed: its rest length along its x axis.
    let posed_length = |bone: usize, world: &[Affine2]| lengths_of(bone) * world[bone].matrix2.x_axis.length();
    match c.spacing_mode {
        PathSpacing::Percent => {
            if scale {
                for i in 0..spaces_count - 1 {
                    s.lengths[i] = posed_length(c.bones[i], world);
                }
            }
            for space in &mut s.spaces[1..] {
                *space = spacing;
            }
        }
        PathSpacing::Proportional => {
            let mut sum = 0.0;
            for i in 0..spaces_count - 1 {
                let bone = c.bones[i];
                if lengths_of(bone) < EPSILON {
                    s.lengths[i] = 0.0;
                    s.spaces[i + 1] = spacing;
                } else {
                    let length = posed_length(bone, world);
                    s.lengths[i] = length;
                    s.spaces[i + 1] = length;
                    sum += length;
                }
            }
            if sum > 0.0 {
                let k = spaces_count as f32 / sum * spacing;
                for space in &mut s.spaces[1..] {
                    *space *= k;
                }
            }
        }
        PathSpacing::Length | PathSpacing::Fixed => {
            let by_length = c.spacing_mode == PathSpacing::Length;
            for i in 0..spaces_count - 1 {
                let bone = c.bones[i];
                let rest = lengths_of(bone);
                if rest < EPSILON {
                    s.lengths[i] = 0.0;
                    s.spaces[i + 1] = spacing;
                } else {
                    let length = posed_length(bone, world);
                    s.lengths[i] = length;
                    s.spaces[i + 1] = if by_length { rest + spacing } else { spacing } * length / rest;
                }
            }
        }
    }
    positions(c, path, position, spaces_count, tangents, s);
    let mut bone_x = s.positions[0];
    let mut bone_y = s.positions[1];
    let mut offset_rotation = c.offset_rotation;
    let tip = if offset_rotation == 0.0 {
        c.rotate_mode == PathRotate::Chain
    } else {
        if target_mirrored {
            offset_rotation = -offset_rotation;
        }
        false
    };
    let mut p = 3;
    for i in 0..bone_count {
        let bone = c.bones[i];
        let m = &mut world[bone];
        m.translation.x += (bone_x - m.translation.x) * mix_x;
        m.translation.y += (bone_y - m.translation.y) * mix_y;
        let (x, y) = (s.positions[p], s.positions[p + 1]);
        let (dx, dy) = (x - bone_x, y - bone_y);
        if scale {
            let length = s.lengths[i];
            if length >= EPSILON {
                let k = ((dx * dx + dy * dy).sqrt() / length - 1.0) * mix_rotate + 1.0;
                m.matrix2.x_axis *= k;
            }
        }
        bone_x = x;
        bone_y = y;
        if mix_rotate > 0.0 {
            let (a, cc) = (m.matrix2.x_axis.x, m.matrix2.x_axis.y);
            let mut r = if tangents {
                s.positions[p - 1]
            } else if s.spaces[i + 1] < EPSILON {
                s.positions[p + 2]
            } else {
                dy.atan2(dx)
            };
            r -= cc.atan2(a);
            if tip {
                let (sin, cos) = r.sin_cos();
                let length = lengths_of(bone);
                bone_x += (length * (cos * a - sin * cc) - dx) * mix_rotate;
                bone_y += (length * (sin * a + cos * cc) - dy) * mix_rotate;
            } else {
                r += offset_rotation;
            }
            if r > std::f32::consts::PI {
                r -= std::f32::consts::TAU;
            } else if r < -std::f32::consts::PI {
                r += std::f32::consts::TAU;
            }
            m.matrix2 = glam::Mat2::from_angle(r * mix_rotate) * m.matrix2;
        }
        p += 3;
    }
    true
}

/// Fills `s.positions` with `spaces_count` places along `path` (x, y and
/// direction each), from `position` on, `s.spaces` apart.
fn positions(c: &PathConstraint, path: &PathShape, position: f32, spaces_count: usize, tangents: bool, s: &mut PathScratch) {
    let PathScratch {
        vertices,
        world,
        spaces,
        curves,
        positions: out,
        ..
    } = s;
    out.clear();
    out.resize(spaces_count * 3 + 2, 0.0);
    let n = vertices.len();
    let closed = path.closed;
    let mut position = position;
    let multiplier_of = |length: f32| match c.spacing_mode {
        PathSpacing::Percent => length,
        PathSpacing::Proportional => length / spaces_count as f32,
        _ => 1.0,
    };
    if !path.constant_speed {
        let curve_count = n / 3 - if closed { 1 } else { 2 };
        let path_length = path.lengths[curve_count];
        if c.position_mode == PathPosition::Percent {
            position *= path_length;
        }
        let multiplier = multiplier_of(path_length);
        let mut previous = None;
        let mut w = [Vec2::ZERO; 4];
        let mut curve = 0;
        for i in 0..spaces_count {
            let o = 3 * i;
            let space = spaces[i] * multiplier;
            position += space;
            let mut p = position;
            if closed {
                p %= path_length;
                if p < 0.0 {
                    p += path_length;
                }
                curve = 0;
            } else if p < 0.0 {
                before(p, vertices[1], vertices[2], &mut out[o..o + 3]);
                continue;
            } else if p > path_length {
                after(p - path_length, vertices[n - 3], vertices[n - 2], &mut out[o..o + 3]);
                continue;
            }
            loop {
                let length = path.lengths[curve];
                if p > length && curve < curve_count {
                    curve += 1;
                    continue;
                }
                if curve == 0 {
                    p /= length;
                } else {
                    let prev = path.lengths[curve - 1];
                    p = (p - prev) / (length - prev);
                }
                break;
            }
            if previous != Some(curve) {
                previous = Some(curve);
                w = if closed && curve == curve_count {
                    [vertices[n - 2], vertices[n - 1], vertices[0], vertices[1]]
                } else {
                    [
                        vertices[3 * curve + 1],
                        vertices[3 * curve + 2],
                        vertices[3 * curve + 3],
                        vertices[3 * curve + 4],
                    ]
                };
            }
            on_curve(
                p,
                w,
                &mut out[o..o + 3],
                tangents || (i > 0 && space < EPSILON),
            );
        }
        return;
    }
    // Measured by distance: the vertices in curve order.
    world.clear();
    let curve_count;
    if closed {
        world.extend_from_slice(&vertices[1..]);
        world.push(vertices[0]);
        world.push(vertices[1]);
        curve_count = n / 3;
    } else {
        world.extend_from_slice(&vertices[1..n - 1]);
        curve_count = n / 3 - 1;
    }
    curves.clear();
    let mut path_length = 0.0;
    for i in 0..curve_count {
        let [p1, c1, c2, p2] = [world[3 * i], world[3 * i + 1], world[3 * i + 2], world[3 * i + 3]];
        path_length += forward_length(p1, c1, c2, p2, 4, &mut |_, _| {});
        curves.push(path_length);
    }
    if c.position_mode == PathPosition::Percent {
        position *= path_length;
    }
    let multiplier = multiplier_of(path_length);
    let mut segments = [0.0f32; 10];
    let mut curve_length = 0.0;
    let mut previous = None;
    let mut curve = 0;
    let mut segment = 0;
    let mut w = [Vec2::ZERO; 4];
    for i in 0..spaces_count {
        let o = 3 * i;
        let space = spaces[i] * multiplier;
        position += space;
        let mut p = position;
        if closed {
            p %= path_length;
            if p < 0.0 {
                p += path_length;
            }
            curve = 0;
            segment = 0;
        } else if p < 0.0 {
            before(p, world[0], world[1], &mut out[o..o + 3]);
            continue;
        } else if p > path_length {
            let last = world.len() - 2;
            after(p - path_length, world[last], world[last + 1], &mut out[o..o + 3]);
            continue;
        }
        loop {
            let length = curves[curve];
            if p > length && curve + 1 < curve_count {
                curve += 1;
                continue;
            }
            if curve == 0 {
                p /= length;
            } else {
                let prev = curves[curve - 1];
                p = (p - prev) / (length - prev);
            }
            break;
        }
        if previous != Some(curve) {
            previous = Some(curve);
            w = [world[3 * curve], world[3 * curve + 1], world[3 * curve + 2], world[3 * curve + 3]];
            curve_length = forward_length(w[0], w[1], w[2], w[3], 10, &mut |k, total| segments[k] = total);
            segment = 0;
        }
        // Weighted by segment length.
        p *= curve_length;
        loop {
            let length = segments[segment];
            if p > length && segment < 9 {
                segment += 1;
                continue;
            }
            if segment == 0 {
                p /= length;
            } else {
                let prev = segments[segment - 1];
                p = segment as f32 + (p - prev) / (length - prev);
            }
            break;
        }
        on_curve(
            p * 0.1,
            w,
            &mut out[o..o + 3],
            tangents || (i > 0 && space < EPSILON),
        );
    }
}

/// The length of the cubic from `p1` through `c1` and `c2` to `p2`,
/// measured along `steps` (4 or 10) straight pieces by forward
/// differencing, as Spine measures it; `each(k, total)` sees the running
/// total after piece `k`.
fn forward_length(p1: Vec2, c1: Vec2, c2: Vec2, p2: Vec2, steps: usize, each: &mut impl FnMut(usize, f32)) -> f32 {
    let (k1, k2, k3) = if steps == 4 {
        (0.75, 0.1875, 0.09375)
    } else {
        (0.3, 0.03, 0.006)
    };
    let tmp = (p1 - c1 * 2.0 + c2) * k2;
    let dddf = ((c1 - c2) * 3.0 - p1 + p2) * k3;
    let mut ddf = tmp * 2.0 + dddf;
    let mut df = (c1 - p1) * k1 + tmp + dddf * 0.166_666_67;
    let mut total = df.length();
    each(0, total);
    for k in 1..steps - 2 {
        df += ddf;
        ddf += dddf;
        total += df.length();
        each(k, total);
    }
    df += ddf;
    total += df.length();
    each(steps - 2, total);
    df += ddf + dddf;
    total += df.length();
    each(steps - 1, total);
    total
}

/// A place `p` (negative) before the path's start, along its first handle.
fn before(p: f32, start: Vec2, handle: Vec2, out: &mut [f32]) {
    let r = (handle.y - start.y).atan2(handle.x - start.x);
    out[0] = start.x + p * r.cos();
    out[1] = start.y + p * r.sin();
    out[2] = r;
}

/// A place `p` past the path's end, along its last handle.
fn after(p: f32, handle: Vec2, end: Vec2, out: &mut [f32]) {
    let r = (end.y - handle.y).atan2(end.x - handle.x);
    out[0] = end.x + p * r.cos();
    out[1] = end.y + p * r.sin();
    out[2] = r;
}

/// The place at parameter `p` of the cubic `w`, and its direction when
/// `tangents`.
fn on_curve(p: f32, w: [Vec2; 4], out: &mut [f32], tangents: bool) {
    let [p1, c1, c2, p2] = w;
    if p < EPSILON || p.is_nan() {
        out[0] = p1.x;
        out[1] = p1.y;
        out[2] = (c1.y - p1.y).atan2(c1.x - p1.x);
        return;
    }
    let (tt, u) = (p * p, 1.0 - p);
    let (ttt, uu) = (tt * p, u * u);
    let uuu = uu * u;
    let ut = u * p;
    let ut3 = ut * 3.0;
    let (uut3, utt3) = (u * ut3, ut3 * p);
    let at = p1 * uuu + c1 * uut3 + c2 * utt3 + p2 * ttt;
    out[0] = at.x;
    out[1] = at.y;
    if tangents {
        out[2] = if p < 0.001 {
            (c1.y - p1.y).atan2(c1.x - p1.x)
        } else {
            let back = p1 * uu + c1 * (ut * 2.0) + c2 * tt;
            (at.y - back.y).atan2(at.x - back.x)
        };
    }
}
