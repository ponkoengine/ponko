//! Floating physics bodies on water: [`Buoyancy`] and [`float_bodies`].
#![cfg(feature = "physics3d")]

use glam::{Quat, Vec2, Vec3};
use ponko_ecs::{Component, Entity};

use super::Water;
use crate::ctx::Ctx;

/// Makes a 3D physics body float on [`Water`]: each step,
/// [`float_bodies`] pushes it up by the water it displaces and drags it
/// along with the water's motion (waves and current).
///
/// The body's volume is split evenly over its sample `points` (in the
/// body's own space). Each point stands for a column `height` metres tall
/// around it: under water when the surface is `height / 2` above it, out
/// of it when the surface is that far below. So a level box of height
/// `h` made with [`cuboid`](Self::cuboid), of density `ρ` in water of
/// density `ρw`, settles with its centre `h · (0.5 − ρ / ρw)` above the
/// surface.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::water::Buoyancy;
/// let crate_ = Buoyancy::cuboid(vec3(1.0, 0.6, 1.0));
/// assert_eq!(crate_.points.len(), 8);
/// assert!((crate_.height - 0.3).abs() < 1e-6);
/// assert!((crate_.volume - 0.6).abs() < 1e-6);
/// ```
#[derive(Debug, Clone, PartialEq, Component)]
pub struct Buoyancy {
    /// Where the body samples the water, in its own space.
    pub points: Vec<Vec3>,
    /// The water it displaces when wholly under, in m³.
    pub volume: f32,
    /// The height of the column each point stands for, in metres.
    pub height: f32,
    /// The water's density, in kg/m³ (fresh water: 1000).
    pub water_density: f32,
    /// How strongly the water drags the body towards its own motion: the
    /// force is the velocity difference times `drag` times the mass of the
    /// water it displaces now. A floating body (whose displaced water
    /// weighs what it does) so closes `drag` of the gap per second.
    pub drag: f32,
    /// Bodies crossing the surface start contact ripples on it.
    pub ripples: bool,
    /// How much of the body was under water at the last step (0 to 1).
    /// Written by [`float_bodies`].
    pub submerged: f32,
}

impl Buoyancy {
    /// A box of `size` centred on the body: eight points, two layers of
    /// four, each standing for a quarter of its footprint and half its
    /// height, so a level box is under water by exactly as much as the
    /// surface covers.
    #[must_use]
    pub fn cuboid(size: Vec3) -> Self {
        let q = size * 0.25;
        let mut points = Vec::with_capacity(8);
        for y in [-q.y, q.y] {
            for x in [-q.x, q.x] {
                for z in [-q.z, q.z] {
                    points.push(Vec3::new(x, y, z));
                }
            }
        }
        Self::points(points, size.x * size.y * size.z, size.y * 0.5)
    }

    /// A ball of `radius`: seven points (the centre and six around it).
    #[must_use]
    pub fn sphere(radius: f32) -> Self {
        let r = radius * 0.5;
        let points = vec![
            Vec3::ZERO,
            Vec3::X * r,
            -Vec3::X * r,
            Vec3::Z * r,
            -Vec3::Z * r,
            Vec3::Y * r * 0.5,
            -Vec3::Y * r * 0.5,
        ];
        Self::points(points, 4.0 / 3.0 * std::f32::consts::PI * radius.powi(3), radius * 2.0)
    }

    /// Sample `points` (body space) sharing `volume` m³, each standing for
    /// a column `height` tall. Fresh water, drag 4, ripples on.
    #[must_use]
    pub fn points(points: Vec<Vec3>, volume: f32, height: f32) -> Self {
        Self {
            points,
            volume,
            height: height.max(1e-3),
            water_density: 1000.0,
            drag: 4.0,
            ripples: true,
            submerged: 0.0,
        }
    }

    /// The same, with `drag` per second.
    #[must_use]
    pub fn with_drag(self, drag: f32) -> Self {
        Self { drag, ..self }
    }

    /// The forces on a body at `position`, turned by `rotation`, whose
    /// point at `p` moves at `velocity(p)`, in water whose
    /// surface at a ground point is `surface(xz)` (height, water velocity;
    /// `None`: no water there). Calls `push(force, point)` for each
    /// point under water and returns how much of the body is under.
    pub fn forces(
        &self,
        position: Vec3,
        rotation: Quat,
        gravity: f32,
        velocity: impl Fn(Vec3) -> Vec3,
        surface: impl Fn(Vec2) -> Option<(f32, Vec3)>,
        mut push: impl FnMut(Vec3, Vec3),
    ) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        let share = 1.0 / self.points.len() as f32;
        let mut under = 0.0;
        for &local in &self.points {
            let p = position + rotation * local;
            let Some((height, water)) = surface(Vec2::new(p.x, p.z)) else {
                continue;
            };
            let s = ((height - p.y) / self.height + 0.5).clamp(0.0, 1.0);
            if s <= 0.0 {
                continue;
            }
            under += s * share;
            let displaced = self.water_density * self.volume * share * s;
            let lift = Vec3::Y * (displaced * gravity);
            let drag = (water - velocity(p)) * (self.drag * displaced);
            push(lift + drag, p);
        }
        under
    }
}

/// Floats every body with [`Buoyancy`] on the [`Water`] under it (the
/// highest surface where several overlap), at the simulation's time:
/// pushes each sample point under water up by the water it displaces
/// and drags it with the water. Bodies crossing the surface start contact
/// ripples. Added by [`WaterPlugin`](super::WaterPlugin) before the
/// physics step; deterministic on the fixed step.
pub fn float_bodies(ctx: &mut Ctx) {
    let t = ctx.time.sim_time() as f32;
    let dt = ctx.time.dt();
    let gravity = -ctx.physics3d.gravity().y;
    let FloatScratch(mut floating, mut waters) = ctx
        .world
        .get_resource_mut::<FloatScratch>()
        .map(std::mem::take)
        .unwrap_or_default();
    floating.clear();
    waters.clear();
    ctx.world.query::<(Entity, &Water)>().for_each(|(e, _)| waters.push(e));
    if !waters.is_empty() {
        ctx.world
            .query::<(Entity, &Buoyancy)>()
            .for_each(|(e, _)| floating.push(e));
    }
    for &e in &floating {
        let (Some(position), Some(rotation)) = (ctx.physics3d.position(e), ctx.physics3d.rotation(e)) else {
            continue;
        };
        let Some(buoyancy) = ctx.world.get::<Buoyancy>(e) else {
            continue;
        };
        let world = &ctx.world;
        let surface = |xz: Vec2| -> Option<(f32, Vec3)> {
            let mut best: Option<(f32, Vec3)> = None;
            for &w in &waters {
                let Some(water) = world.get::<Water>(w) else { continue };
                if !water.contains(xz) {
                    continue;
                }
                let s = water.sample(xz, t);
                if best.is_none_or(|(h, _)| s.height > h) {
                    best = Some((s.height, s.velocity));
                }
            }
            best
        };
        let physics = &ctx.physics3d;
        let mut pushes: [(Vec3, Vec3); 32] = [(Vec3::ZERO, Vec3::ZERO); 32];
        let mut count = 0;
        let under = buoyancy.forces(
            position,
            rotation,
            gravity,
            |p| physics.velocity_at(e, p).unwrap_or(Vec3::ZERO),
            surface,
            |force, at| {
                if count < pushes.len() {
                    pushes[count] = (force, at);
                    count += 1;
                }
            },
        );
        let was = buoyancy.submerged;
        let ripples = buoyancy.ripples;
        let crossing = ripples && ((was <= 0.0) != (under <= 0.0) || (was >= 1.0) != (under >= 1.0));
        for &(force, at) in &pushes[..count] {
            ctx.physics3d.apply_impulse_at(e, force * dt, at);
        }
        if let Some(b) = ctx.world.get_mut::<Buoyancy>(e) {
            b.submerged = under;
        }
        if crossing || (ripples && under > 0.0 && under < 1.0 && ctx.time.sim_tick().is_multiple_of(20)) {
            let xz = Vec2::new(position.x, position.z);
            let speed = ctx.physics3d.velocity_at(e, position).unwrap_or(Vec3::ZERO).length();
            for &w in &waters {
                if let Some(water) = ctx.world.get_mut::<Water>(w)
                    && water.contains(xz)
                {
                    water.touch(xz, 0.5, (0.02 + speed * 0.02).min(0.15));
                }
            }
        }
    }
    let scratch = FloatScratch(floating, waters);
    match ctx.world.get_resource_mut::<FloatScratch>() {
        Some(kept) => *kept = scratch,
        None => ctx.world.insert_resource(scratch),
    }
}

/// The floating bodies and water bodies of this step (kept, so the step
/// doesn't allocate).
#[derive(Default)]
struct FloatScratch(Vec<Entity>, Vec<Entity>);
