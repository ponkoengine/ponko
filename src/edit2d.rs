//! The 2D editor's math: the move / rotate / scale gizmo with snapping, in
//! world or local space, and the settings the 3D gizmo shares with it
//! (mode, space, snapping). No egui and no window: the inspector draws and
//! drives it, and the tests run it headless.
//!
//! The 2D view maps world to window pixels by a scale and a shift (no
//! turn, no flip), so the gizmo's axes point the same way in the world and
//! on screen: drags are worked out in world units, hit tests in pixels.

use glam::Vec2;
use ponko_ecs::Entity;

use crate::d2::Transform;

/// Snapping for gizmo drags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Snap {
    pub on: bool,
    /// Grid step for moves, in world units.
    pub step: f32,
    /// Angle step for turns, in radians.
    pub angle: f32,
    /// Step for scale factors (0.1: 1.0, 1.1, 1.2, ...).
    pub scale: f32,
}

impl Default for Snap {
    /// The 3D default: half a world unit.
    fn default() -> Self {
        Self {
            on: false,
            step: 0.5,
            angle: 15f32.to_radians(),
            scale: 0.1,
        }
    }
}

impl Snap {
    /// The 2D default: a grid of 8 world pixels.
    pub fn pixels() -> Self {
        Self {
            step: 8.0,
            ..Self::default()
        }
    }
}

/// `v` to the nearest multiple of `step` (unchanged for a step of 0 or less).
pub(crate) fn snap(v: f32, step: f32) -> f32 {
    if step > 0.0 { (v / step).round() * step } else { v }
}

/// What the gizmo does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    #[default]
    Translate,
    Rotate,
    Scale,
}

/// Which axes the gizmo uses: the world's, or the selected entity's own.
/// Scaling always uses the entity's own axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Space {
    #[default]
    World,
    Local,
}

/// The part of the 2D gizmo being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    /// Along axis 0 (x) or 1 (y).
    Axis(usize),
    /// The middle: move freely, or scale evenly.
    Center,
    /// The ring: turn.
    Ring,
}

/// The gizmo's two axes for `rotation` (the primary selection's) in `space`.
pub(crate) fn axes(space: Space, rotation: f32) -> [Vec2; 2] {
    match space {
        Space::World => [Vec2::X, Vec2::Y],
        Space::Local => {
            let turn = Vec2::from_angle(rotation);
            [turn.rotate(Vec2::X), turn.rotate(Vec2::Y)]
        }
    }
}

/// `starts` moved by `delta`. Snapped in world space, the first entity
/// lands on the grid along the moved axes (`moved`, world axis indices)
/// and the others move with it; in local space, the move along each of
/// `axes` is a whole number of steps.
pub(crate) fn translate(
    starts: &[(Entity, Transform)],
    delta: Vec2,
    space: Space,
    axes: [Vec2; 2],
    moved: [bool; 2],
    snapping: Snap,
) -> Vec<(Entity, Transform)> {
    let mut delta = delta;
    if snapping.on
        && let Some((_, first)) = starts.first()
    {
        delta = match space {
            Space::World => {
                let mut to = first.position + delta;
                for i in 0..2 {
                    if moved[i] {
                        to[i] = snap(to[i], snapping.step);
                    }
                }
                to - first.position
            }
            Space::Local => {
                axes[0] * snap(delta.dot(axes[0]), snapping.step) + axes[1] * snap(delta.dot(axes[1]), snapping.step)
            }
        };
    }
    starts
        .iter()
        .map(|&(e, t)| {
            (
                e,
                Transform {
                    position: t.position + delta,
                    ..t
                },
            )
        })
        .collect()
}

/// `starts` turned by `angle` radians about `pivot` (snapped to whole
/// angle steps).
pub(crate) fn rotate(
    starts: &[(Entity, Transform)],
    pivot: Vec2,
    angle: f32,
    snapping: Snap,
) -> Vec<(Entity, Transform)> {
    let angle = if snapping.on {
        snap(angle, snapping.angle)
    } else {
        angle
    };
    let turn = Vec2::from_angle(angle);
    starts
        .iter()
        .map(|&(e, t)| {
            (
                e,
                Transform {
                    position: pivot + turn.rotate(t.position - pivot),
                    rotation: t.rotation + angle,
                    scale: t.scale,
                },
            )
        })
        .collect()
}

/// `starts` with their own scale multiplied by `factors` (per local axis;
/// each snapped to whole steps from 1, never 0). Positions stay.
pub(crate) fn scale(starts: &[(Entity, Transform)], factors: Vec2, snapping: Snap) -> Vec<(Entity, Transform)> {
    let factors = if snapping.on {
        factors.map(|f| 1.0 + snap(f - 1.0, snapping.scale))
    } else {
        factors
    };
    let factors = factors.map(|f| if f.abs() < 1e-3 { 1e-3f32.copysign(f) } else { f });
    starts
        .iter()
        .map(|&(e, t)| {
            (
                e,
                Transform {
                    scale: t.scale * factors,
                    ..t
                },
            )
        })
        .collect()
}

/// A 2D gizmo drag in progress: the entities and where they started, and
/// where (in the world) the mouse grabbed which handle.
#[derive(Debug, Clone)]
pub(crate) struct Drag {
    pub mode: Mode,
    pub handle: Handle,
    pub space: Space,
    pub pivot: Vec2,
    pub axes: [Vec2; 2],
    /// An axis' length in world units: a drag that long scales by 2.
    size: f32,
    grab: Vec2,
    pub starts: Vec<(Entity, Transform)>,
}

impl Drag {
    /// Starts dragging `handle` of the gizmo at the middle of `starts`
    /// (world transforms, the primary selection first), grabbed at world
    /// point `at`; `size` is an axis' length in world units. `None` with
    /// nothing to drag.
    pub fn begin(
        mode: Mode,
        handle: Handle,
        space: Space,
        starts: Vec<(Entity, Transform)>,
        at: Vec2,
        size: f32,
    ) -> Option<Self> {
        let first = starts.first()?.1;
        let pivot = starts.iter().map(|(_, t)| t.position).sum::<Vec2>() / starts.len() as f32;
        let axes = match mode {
            Mode::Scale => axes(Space::Local, first.rotation),
            _ => axes(space, first.rotation),
        };
        Some(Self {
            mode,
            handle,
            space,
            pivot,
            axes,
            size: size.max(1e-6),
            grab: at,
            starts,
        })
    }

    /// The entities' world transforms with the mouse now at world point `at`.
    pub fn update(&self, at: Vec2, snapping: Snap) -> Vec<(Entity, Transform)> {
        let delta = at - self.grab;
        match (self.mode, self.handle) {
            (Mode::Translate, Handle::Axis(i)) => {
                let axis = self.axes[i];
                let mut moved = [false; 2];
                if self.space == Space::World {
                    moved[i] = true;
                } else {
                    moved = [true; 2];
                }
                translate(
                    &self.starts,
                    axis * delta.dot(axis),
                    self.space,
                    self.axes,
                    moved,
                    snapping,
                )
            }
            (Mode::Translate, _) => translate(&self.starts, delta, self.space, self.axes, [true; 2], snapping),
            (Mode::Rotate, _) => {
                let (from, to) = (self.grab - self.pivot, at - self.pivot);
                let angle = if from.length_squared() < 1e-12 || to.length_squared() < 1e-12 {
                    0.0
                } else {
                    from.angle_to(to)
                };
                rotate(&self.starts, self.pivot, angle, snapping)
            }
            (Mode::Scale, Handle::Axis(i)) => {
                let mut factors = Vec2::ONE;
                factors[i] = 1.0 + delta.dot(self.axes[i]) / self.size;
                scale(&self.starts, factors, snapping)
            }
            (Mode::Scale, _) => {
                // Right (or down) grows, left (or up) shrinks.
                let f = 1.0 + (delta.x + delta.y) * std::f32::consts::FRAC_1_SQRT_2 / self.size;
                scale(&self.starts, Vec2::splat(f), snapping)
            }
        }
    }
}

/// The gizmo's size on screen, in pixels from its center to an axis tip
/// (and the ring's radius).
pub(crate) const GIZMO_PIXELS: f32 = 70.0;
/// How close (pixels) the mouse must be to a handle to grab it.
#[cfg_attr(not(feature = "inspector"), allow(dead_code))]
pub(crate) const GRAB_PIXELS: f32 = 8.0;

/// Where the 2D gizmo's parts are on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(feature = "inspector"), allow(dead_code))]
pub(crate) struct Layout {
    pub center: Vec2,
    /// Each axis' tip.
    pub tips: [Vec2; 2],
    /// The ring's radius (rotating).
    pub radius: f32,
}

/// Lays the gizmo out around window point `center` with `axes`.
#[cfg_attr(not(feature = "inspector"), allow(dead_code))]
pub(crate) fn layout(center: Vec2, axes: [Vec2; 2]) -> Layout {
    Layout {
        center,
        tips: [center + axes[0] * GIZMO_PIXELS, center + axes[1] * GIZMO_PIXELS],
        radius: GIZMO_PIXELS,
    }
}

/// The handle under window point `mouse`, if any: rotating, the ring;
/// else the center first, then the nearest axis.
#[cfg_attr(not(feature = "inspector"), allow(dead_code))]
pub(crate) fn hit(layout: &Layout, mode: Mode, mouse: Vec2) -> Option<Handle> {
    if mode == Mode::Rotate {
        let off = (mouse.distance(layout.center) - layout.radius).abs();
        return (off <= GRAB_PIXELS).then_some(Handle::Ring);
    }
    if mouse.distance(layout.center) <= GRAB_PIXELS {
        return Some(Handle::Center);
    }
    let mut best: Option<(f32, usize)> = None;
    for (i, &tip) in layout.tips.iter().enumerate() {
        let d = to_segment(mouse, layout.center, tip);
        if d <= GRAB_PIXELS && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| Handle::Axis(i))
}

/// Distance from `p` to the segment `a`–`b`.
#[cfg_attr(not(feature = "inspector"), allow(dead_code))]
fn to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-6 {
        0.0
    } else {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::vec2;
    use std::f32::consts::FRAC_PI_2;

    fn e(n: u32) -> Entity {
        Entity::from_bits(u64::from(n))
    }
    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn an_axis_move_follows_the_mouse_only_along_that_axis() {
        let starts = vec![(e(1), Transform::at(vec2(10.0, 10.0)))];
        let drag = Drag::begin(
            Mode::Translate,
            Handle::Axis(0),
            Space::World,
            starts,
            vec2(80.0, 10.0),
            70.0,
        )
        .unwrap();
        let moved = drag.update(vec2(95.0, 40.0), Snap::pixels());
        assert!(close(moved[0].1.position, vec2(25.0, 10.0)));
    }

    #[test]
    fn snapped_world_moves_put_the_first_entity_on_the_grid_and_keep_the_others_with_it() {
        let starts = vec![
            (e(1), Transform::at(vec2(3.0, 5.0))),
            (e(2), Transform::at(vec2(13.0, 5.0))),
        ];
        let snapping = Snap {
            on: true,
            ..Snap::pixels()
        };
        let drag = Drag::begin(
            Mode::Translate,
            Handle::Center,
            Space::World,
            starts,
            vec2(8.0, 5.0),
            70.0,
        )
        .unwrap();
        let moved = drag.update(vec2(19.0, 10.0), snapping);
        assert!(close(moved[0].1.position, vec2(16.0, 8.0)), "{:?}", moved[0].1.position);
        assert!(close(moved[1].1.position, vec2(26.0, 8.0)));
    }

    #[test]
    fn a_local_axis_move_follows_the_entitys_own_axis_in_whole_steps() {
        let start = Transform::at(Vec2::ZERO).rotated(FRAC_PI_2);
        let snapping = Snap {
            on: true,
            ..Snap::pixels()
        };
        // Local x points along world +y.
        let drag = Drag::begin(
            Mode::Translate,
            Handle::Axis(0),
            Space::Local,
            vec![(e(1), start)],
            Vec2::ZERO,
            70.0,
        )
        .unwrap();
        let moved = drag.update(vec2(3.0, 13.0), snapping);
        assert!(close(moved[0].1.position, vec2(0.0, 16.0)), "{:?}", moved[0].1.position);
    }

    #[test]
    fn turning_the_ring_turns_the_selection_about_its_middle_in_snapped_steps() {
        let starts = vec![
            (e(1), Transform::at(vec2(-10.0, 0.0))),
            (e(2), Transform::at(vec2(10.0, 0.0))),
        ];
        let drag = Drag::begin(Mode::Rotate, Handle::Ring, Space::World, starts, vec2(70.0, 0.0), 70.0).unwrap();
        // 80 degrees, snapped to 75.
        let to = Vec2::from_angle(80f32.to_radians()) * 70.0;
        let snapping = Snap {
            on: true,
            ..Snap::pixels()
        };
        let moved = drag.update(to, snapping);
        let angle = 75f32.to_radians();
        assert!((moved[1].1.rotation - angle).abs() < 1e-4);
        assert!(close(moved[1].1.position, Vec2::from_angle(angle) * 10.0));
        assert!(close(moved[0].1.position, -Vec2::from_angle(angle) * 10.0));
        let free = drag.update(to, Snap::pixels());
        assert!((free[0].1.rotation - 80f32.to_radians()).abs() < 1e-4);
    }

    #[test]
    fn scaling_an_axis_scales_only_that_local_axis_and_never_to_zero() {
        let start = Transform {
            scale: vec2(2.0, 3.0),
            ..Transform::at(vec2(5.0, 5.0))
        };
        let drag = Drag::begin(
            Mode::Scale,
            Handle::Axis(0),
            Space::World,
            vec![(e(1), start)],
            vec2(75.0, 5.0),
            70.0,
        )
        .unwrap();
        let moved = drag.update(vec2(145.0, 50.0), Snap::pixels());
        assert!(close(moved[0].1.scale, vec2(4.0, 3.0)));
        assert_eq!(moved[0].1.position, start.position);
        let flat = drag.update(vec2(5.0, 5.0), Snap::pixels());
        assert!(flat[0].1.scale.x.abs() > 0.0);
        let snapping = Snap {
            on: true,
            ..Snap::pixels()
        };
        let snapped = drag.update(vec2(75.0 + 70.0 * 0.26, 5.0), snapping);
        assert!(
            close(snapped[0].1.scale, vec2(2.0 * 1.3, 3.0)),
            "{:?}",
            snapped[0].1.scale
        );
    }

    #[test]
    fn the_center_scales_evenly() {
        let start = Transform::at(Vec2::ZERO);
        let drag = Drag::begin(
            Mode::Scale,
            Handle::Center,
            Space::World,
            vec![(e(1), start)],
            Vec2::ZERO,
            70.0,
        )
        .unwrap();
        let d = 70.0 * std::f32::consts::FRAC_1_SQRT_2;
        let moved = drag.update(vec2(d, d), Snap::pixels());
        assert!(close(moved[0].1.scale, vec2(2.0, 2.0)), "{:?}", moved[0].1.scale);
    }

    #[test]
    fn hits_find_the_center_then_the_nearest_axis_and_the_ring_when_turning() {
        let l = layout(vec2(100.0, 100.0), axes(Space::World, 0.0));
        assert_eq!(hit(&l, Mode::Translate, vec2(102.0, 101.0)), Some(Handle::Center));
        assert_eq!(hit(&l, Mode::Translate, vec2(150.0, 103.0)), Some(Handle::Axis(0)));
        assert_eq!(hit(&l, Mode::Scale, vec2(98.0, 160.0)), Some(Handle::Axis(1)));
        assert_eq!(hit(&l, Mode::Translate, vec2(150.0, 150.0)), None);
        assert_eq!(hit(&l, Mode::Rotate, vec2(100.0, 100.0)), None);
        assert_eq!(hit(&l, Mode::Rotate, vec2(100.0, 33.0)), Some(Handle::Ring));
    }

    #[test]
    fn local_axes_turn_with_the_entity() {
        let [x, y] = axes(Space::Local, FRAC_PI_2);
        assert!(close(x, Vec2::Y) && close(y, -Vec2::X));
        assert_eq!(axes(Space::World, 1.0), [Vec2::X, Vec2::Y]);
    }
}
