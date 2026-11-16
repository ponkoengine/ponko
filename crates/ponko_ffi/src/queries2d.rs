//! 2D physics queries from C (the `physics` feature): every ray, all-hits
//! ray, circle/box/capsule cast, point/circle/box/capsule overlap, closest
//! point and penetration query, with collision groups, plus a collider's
//! groups. The same rules as Rust's `d2::Physics` queries (and the 3D ones):
//! values pass through unchanged, so a zero direction, a negative range or a
//! non-finite origin finds nothing rather than failing. Without the feature
//! every call here fails with an error naming it.
//!
//! The queries only read the physics world, so they work inside a
//! `ponko_query` callback too (a ground check for each entity a query
//! visits), though not in `ponko_query_par`.
//!
//! `PonkoQueryFilter2D` (ABI 2) has no groups field, so these calls take
//! the groups as their own `PonkoCollisionGroups` argument: pass
//! `ponko_collision_groups_all()` to see every group.
use crate::types::{PonkoEntity, PonkoVec2};
use crate::{PonkoCollider, PonkoCollisionGroups, PonkoCtx, PonkoQueryFilter2D, PonkoRayHit, PonkoStatus, PonkoWorld};

/// The nearest point of a collider to a point (`ponko_ctx_closest_point2d`).
/// ```
/// let h = ponko_ffi::PonkoPointHit2D::default();
/// assert!(!h.inside);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoPointHit2D {
    /// Whose collider.
    pub entity: PonkoEntity,
    /// Its nearest point: the query point itself when that is inside.
    pub point: PonkoVec2,
    /// From the query point to `point` (0 inside).
    pub distance: f32,
    /// Whether the query point is inside the collider.
    pub inside: bool,
}

/// How deep a shape sinks into one collider and the way out
/// (`ponko_ctx_penetrations2d`).
/// ```
/// let p = ponko_ffi::PonkoPenetration2D::default();
/// assert_eq!(p.depth, 0.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoPenetration2D {
    /// Whose collider.
    pub entity: PonkoEntity,
    /// The collider's deepest point inside the shape.
    pub point: PonkoVec2,
    /// The direction that moves the shape out.
    pub normal: PonkoVec2,
    /// How far to move along `normal` to just touch (0 when they only touch).
    pub depth: f32,
}

/// Puts `entity`'s 2D collider in `groups`: it touches only colliders whose
/// groups agree, and only queries whose groups agree see it. Setting a
/// `PonkoCollider` with `ponko_set` gives it every group again
/// (`PonkoCollider` has no groups field in ABI 2), so set groups after it.
/// `PONKO_NOT_FOUND` if the entity has no 2D collider.
/// # Safety
/// Live world, outside query callbacks.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_set_collider_groups(std::ptr::null_mut(), 0, ponko_collision_groups_all()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT); // no world
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_set_collider_groups(
    world: *mut PonkoWorld,
    entity: u64,
    groups: PonkoCollisionGroups,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::set_groups(world, entity, groups) }
}

/// `entity`'s 2D collider groups into `*out_groups`. `PONKO_NOT_FOUND` if
/// it has no 2D collider.
/// # Safety
/// Live world, outside query callbacks; writable output.
/// ```
/// use ponko_ffi::*;
/// let mut g = ponko_collision_groups_all();
/// let s = unsafe { ponko_collider_groups(std::ptr::null_mut(), 0, &mut g) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT); // no world
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_collider_groups(
    world: *mut PonkoWorld,
    entity: u64,
    out_groups: *mut PonkoCollisionGroups,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::groups(world, entity, out_groups) }
}

/// The closest collider a ray from `origin` towards `direction` hits within
/// `max_distance` (Rust `d2::Physics::cast_ray`), seen through `filter` and
/// `groups`. `*out_found` says whether anything was hit; `*out_hit` is
/// written only then.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); writable outputs.
/// ```
/// use ponko_ffi::*;
/// let (mut found, mut hit) = (false, PonkoRayHit::default());
/// let s = unsafe { ponko_ctx_cast_ray2d(std::ptr::null_mut(), Default::default(), Default::default(), 1.0, Default::default(), ponko_collision_groups_all(), &mut found, &mut hit) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_ray2d(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    direction: PonkoVec2,
    max_distance: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::cast(
            ctx,
            imp::Cast::Ray,
            origin,
            direction,
            max_distance,
            filter,
            groups,
            out_found,
            out_hit,
        )
    }
}

/// Every collider a ray hits within `max_distance`, nearest first (equal
/// distances by entity id; Rust `d2::Physics::cast_ray_all`). Writes up to
/// `capacity` hits and sets `*out_count` to how many there are, so a
/// too-small buffer can be grown and the call repeated.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); `out_hits` writable for `capacity`
/// (may be null if 0); writable count.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_ctx_cast_ray2d_all(std::ptr::null_mut(), Default::default(), Default::default(), 1.0, Default::default(), ponko_collision_groups_all(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_ray2d_all(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    direction: PonkoVec2,
    max_distance: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_hits: *mut PonkoRayHit,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::cast_all(
            ctx,
            origin,
            direction,
            max_distance,
            filter,
            groups,
            out_hits,
            capacity,
            out_count,
        )
    }
}

/// Sweeps a circle from `origin` towards `direction`: the first collider it
/// touches within `max_distance` (Rust `d2::Physics::cast_circle`). Outputs
/// as `ponko_ctx_cast_ray2d`.
/// # Safety
/// As `ponko_ctx_cast_ray2d`.
/// ```
/// use ponko_ffi::*;
/// let (mut found, mut hit) = (false, PonkoRayHit::default());
/// let s = unsafe { ponko_ctx_cast_circle2d(std::ptr::null_mut(), Default::default(), 4.0, Default::default(), 1.0, Default::default(), ponko_collision_groups_all(), &mut found, &mut hit) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_circle2d(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    radius: f32,
    direction: PonkoVec2,
    max_distance: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::cast(
            ctx,
            imp::Cast::Circle(radius),
            origin,
            direction,
            max_distance,
            filter,
            groups,
            out_found,
            out_hit,
        )
    }
}

/// Sweeps a rectangle of `size` centered on `origin` and turned by
/// `rotation` radians (Rust `d2::Physics::cast_box`). Outputs as
/// `ponko_ctx_cast_ray2d`.
/// # Safety
/// As `ponko_ctx_cast_ray2d`.
/// ```
/// use ponko_ffi::*;
/// let (mut found, mut hit) = (false, PonkoRayHit::default());
/// let s = unsafe { ponko_ctx_cast_box2d(std::ptr::null_mut(), Default::default(), Default::default(), 0.0, Default::default(), 1.0, Default::default(), ponko_collision_groups_all(), &mut found, &mut hit) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_box2d(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    size: PonkoVec2,
    rotation: f32,
    direction: PonkoVec2,
    max_distance: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::cast(
            ctx,
            imp::Cast::Box(size, rotation),
            origin,
            direction,
            max_distance,
            filter,
            groups,
            out_found,
            out_hit,
        )
    }
}

/// Sweeps an upright capsule centered on `origin` (`height` between the
/// centers of its ends; Rust `d2::Physics::cast_capsule`). Outputs as
/// `ponko_ctx_cast_ray2d`.
/// # Safety
/// As `ponko_ctx_cast_ray2d`.
/// ```
/// use ponko_ffi::*;
/// let (mut found, mut hit) = (false, PonkoRayHit::default());
/// let s = unsafe { ponko_ctx_cast_capsule2d(std::ptr::null_mut(), Default::default(), 10.0, 4.0, Default::default(), 1.0, Default::default(), ponko_collision_groups_all(), &mut found, &mut hit) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_capsule2d(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    height: f32,
    radius: f32,
    direction: PonkoVec2,
    max_distance: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::cast(
            ctx,
            imp::Cast::Capsule(height, radius),
            origin,
            direction,
            max_distance,
            filter,
            groups,
            out_found,
            out_hit,
        )
    }
}

/// Entities whose collider contains `point` (Rust
/// `d2::Physics::overlap_point`). Writes up to `capacity` of them and sets
/// `*out_count` to how many there are in total.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); `out_entities` writable for
/// `capacity` (may be null if 0); writable count.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_ctx_overlap_point2d(std::ptr::null_mut(), Default::default(), Default::default(), ponko_collision_groups_all(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_overlap_point2d(
    ctx: *mut PonkoCtx,
    point: PonkoVec2,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::overlap(
            ctx,
            imp::Overlap::Point,
            point,
            filter,
            groups,
            out_entities,
            capacity,
            out_count,
        )
    }
}

/// Entities whose collider overlaps the circle (Rust
/// `d2::Physics::overlap_circle`). Outputs as `ponko_ctx_overlap_point2d`.
/// # Safety
/// As `ponko_ctx_overlap_point2d`.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_ctx_overlap_circle2d(std::ptr::null_mut(), Default::default(), 4.0, Default::default(), ponko_collision_groups_all(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_circle2d(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    radius: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::overlap(
            ctx,
            imp::Overlap::Circle(radius),
            center,
            filter,
            groups,
            out_entities,
            capacity,
            out_count,
        )
    }
}

/// Entities whose collider overlaps the rectangle of `size` centered on
/// `center` and turned by `rotation` radians (Rust
/// `d2::Physics::overlap_box`). Outputs as `ponko_ctx_overlap_point2d`.
/// # Safety
/// As `ponko_ctx_overlap_point2d`.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_ctx_overlap_box2d(std::ptr::null_mut(), Default::default(), Default::default(), 0.0, Default::default(), ponko_collision_groups_all(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_box2d(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    size: PonkoVec2,
    rotation: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::overlap(
            ctx,
            imp::Overlap::Box(size, rotation),
            center,
            filter,
            groups,
            out_entities,
            capacity,
            out_count,
        )
    }
}

/// Entities whose collider overlaps the upright capsule centered on
/// `center` (Rust `d2::Physics::overlap_capsule`). Outputs as
/// `ponko_ctx_overlap_point2d`.
/// # Safety
/// As `ponko_ctx_overlap_point2d`.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_ctx_overlap_capsule2d(std::ptr::null_mut(), Default::default(), 10.0, 4.0, Default::default(), ponko_collision_groups_all(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_capsule2d(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    height: f32,
    radius: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::overlap(
            ctx,
            imp::Overlap::Capsule(height, radius),
            center,
            filter,
            groups,
            out_entities,
            capacity,
            out_count,
        )
    }
}

/// The nearest collider to `point` within `max_distance` and its nearest
/// point (Rust `d2::Physics::closest_point`); a point inside a collider is
/// its own nearest point. `*out_found` says whether one was found;
/// `*out_hit` is written only then.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); writable outputs.
/// ```
/// use ponko_ffi::*;
/// let (mut found, mut hit) = (false, PonkoPointHit2D::default());
/// let s = unsafe { ponko_ctx_closest_point2d(std::ptr::null_mut(), Default::default(), 10.0, Default::default(), ponko_collision_groups_all(), &mut found, &mut hit) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_closest_point2d(
    ctx: *mut PonkoCtx,
    point: PonkoVec2,
    max_distance: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_found: *mut bool,
    out_hit: *mut PonkoPointHit2D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::closest_point(ctx, point, max_distance, filter, groups, out_found, out_hit) }
}

/// Every collider that `shape` (its kind and sizes; density, friction,
/// restitution, sensor and offset are ignored), placed at `position` and
/// turned by `rotation` radians, overlaps: how deep and which way out,
/// deepest first (Rust `d2::Physics::penetrations`). Writes up to
/// `capacity` and sets `*out_count` to how many there are. A polygon or an
/// invalid shape is `PONKO_INVALID_ARGUMENT`.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); `out_penetrations` writable for
/// `capacity` (may be null if 0); writable count.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_ctx_penetrations2d(std::ptr::null_mut(), ponko_collider_circle(4.0), Default::default(), 0.0, Default::default(), ponko_collision_groups_all(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_penetrations2d(
    ctx: *mut PonkoCtx,
    shape: PonkoCollider,
    position: PonkoVec2,
    rotation: f32,
    filter: PonkoQueryFilter2D,
    groups: PonkoCollisionGroups,
    out_penetrations: *mut PonkoPenetration2D,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::penetrations(
            ctx,
            shape,
            position,
            rotation,
            filter,
            groups,
            out_penetrations,
            capacity,
            out_count,
        )
    }
}

#[cfg(feature = "physics")]
mod imp {
    use std::cell::RefCell;

    use ponko::d2::{Collider, CollisionGroups, Hit, Penetration, QueryFilter};
    use ponko::prelude::Vec2;
    use ponko_ecs::Entity;

    use super::*;
    use crate::ctx::{collect, out, physics_read};
    use crate::types::to_c;
    use crate::{CMirror, boundary, error, invalid};

    fn groups_from_c(g: PonkoCollisionGroups) -> CollisionGroups {
        CollisionGroups::new(g.memberships, g.filter)
    }

    fn filter_from_c(f: PonkoQueryFilter2D, groups: PonkoCollisionGroups) -> QueryFilter {
        QueryFilter {
            exclude: f.exclude.then(|| Entity::from_bits(f.exclude_entity)),
            groups: groups_from_c(groups),
            sensors: f.sensors,
        }
    }

    fn hit_to_c(h: Hit) -> PonkoRayHit {
        PonkoRayHit {
            entity: to_c(h.entity),
            point: to_c(h.point),
            normal: to_c(h.normal),
            distance: h.distance,
        }
    }

    fn vec(v: PonkoVec2) -> Vec2 {
        Vec2::new(v.x, v.y)
    }

    pub(super) unsafe fn set_groups(world: *mut PonkoWorld, entity: u64, groups: PonkoCollisionGroups) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            unsafe {
                crate::access(world, |w, _| {
                    let c = w
                        .get_mut::<Collider>(Entity::from_bits(entity))
                        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity or collider not found"))?;
                    c.groups = groups_from_c(groups);
                    Ok(())
                })
            }
        })
    }

    pub(super) unsafe fn groups(
        world: *mut PonkoWorld,
        entity: u64,
        out_groups: *mut PonkoCollisionGroups,
    ) -> PonkoStatus {
        boundary(|| {
            if out_groups.is_null() {
                return Err(invalid("output is null"));
            }
            // SAFETY: forwarded caller contract.
            unsafe {
                let g = crate::access(world, |w, _| {
                    w.get::<Collider>(Entity::from_bits(entity))
                        .map(|c| c.groups)
                        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity or collider not found"))
                })?;
                out(
                    out_groups,
                    PonkoCollisionGroups {
                        memberships: g.memberships,
                        filter: g.filter,
                    },
                )
            }
        })
    }

    pub(super) enum Cast {
        Ray,
        Circle(f32),
        Box(PonkoVec2, f32),
        Capsule(f32, f32),
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn cast(
        ctx: *mut PonkoCtx,
        shape: Cast,
        origin: PonkoVec2,
        direction: PonkoVec2,
        max_distance: f32,
        filter: PonkoQueryFilter2D,
        groups: PonkoCollisionGroups,
        out_found: *mut bool,
        out_hit: *mut PonkoRayHit,
    ) -> PonkoStatus {
        boundary(|| {
            if out_found.is_null() || out_hit.is_null() {
                return Err(invalid("output is null"));
            }
            let (origin, direction, filter) = (vec(origin), vec(direction), filter_from_c(filter, groups));
            // SAFETY: forwarded caller contract.
            let hit = unsafe {
                physics_read(ctx, |p| {
                    Ok(match shape {
                        Cast::Ray => p.cast_ray(origin, direction, max_distance, filter),
                        Cast::Circle(r) => p.cast_circle(origin, r, direction, max_distance, filter),
                        Cast::Box(size, rotation) => {
                            p.cast_box(origin, vec(size), rotation, direction, max_distance, filter)
                        }
                        Cast::Capsule(h, r) => p.cast_capsule(origin, h, r, direction, max_distance, filter),
                    })
                })?
            };
            // SAFETY: checked non-null; the caller supplies writable outputs.
            unsafe {
                if let Some(h) = hit {
                    out_hit.write(hit_to_c(h));
                }
                out(out_found, hit.is_some())
            }
        })
    }

    thread_local! {
        /// Reused between queries, so they don't allocate each time.
        static HITS: RefCell<Vec<Hit>> = const { RefCell::new(Vec::new()) };
        static FOUND: RefCell<Vec<Entity>> = const { RefCell::new(Vec::new()) };
        static PUSHES: RefCell<Vec<Penetration>> = const { RefCell::new(Vec::new()) };
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn cast_all(
        ctx: *mut PonkoCtx,
        origin: PonkoVec2,
        direction: PonkoVec2,
        max_distance: f32,
        filter: PonkoQueryFilter2D,
        groups: PonkoCollisionGroups,
        out_hits: *mut PonkoRayHit,
        capacity: usize,
        out_count: *mut usize,
    ) -> PonkoStatus {
        boundary(|| {
            HITS.with_borrow_mut(|hits| {
                let filter = filter_from_c(filter, groups);
                // SAFETY: forwarded caller contract.
                unsafe {
                    physics_read(ctx, |p| {
                        p.cast_ray_all(vec(origin), vec(direction), max_distance, filter, hits);
                        Ok(())
                    })?;
                    let mut n = 0;
                    collect(out_hits, capacity, out_count, &mut n, |sink| {
                        for &h in hits.iter() {
                            sink.push(hit_to_c(h));
                        }
                    })
                }
            })
        })
    }

    pub(super) enum Overlap {
        Point,
        Circle(f32),
        Box(PonkoVec2, f32),
        Capsule(f32, f32),
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn overlap(
        ctx: *mut PonkoCtx,
        shape: Overlap,
        center: PonkoVec2,
        filter: PonkoQueryFilter2D,
        groups: PonkoCollisionGroups,
        out_entities: *mut PonkoEntity,
        capacity: usize,
        out_count: *mut usize,
    ) -> PonkoStatus {
        boundary(|| {
            FOUND.with_borrow_mut(|found| {
                let (center, filter) = (vec(center), filter_from_c(filter, groups));
                // SAFETY: forwarded caller contract.
                unsafe {
                    physics_read(ctx, |p| {
                        match shape {
                            Overlap::Point => p.overlap_point(center, filter, found),
                            Overlap::Circle(r) => p.overlap_circle(center, r, filter, found),
                            Overlap::Box(size, rotation) => p.overlap_box(center, vec(size), rotation, filter, found),
                            Overlap::Capsule(h, r) => p.overlap_capsule(center, h, r, filter, found),
                        }
                        Ok(())
                    })?;
                    let mut n = 0;
                    collect(out_entities, capacity, out_count, &mut n, |sink| {
                        for &e in found.iter() {
                            sink.push(to_c(e));
                        }
                    })
                }
            })
        })
    }

    pub(super) unsafe fn closest_point(
        ctx: *mut PonkoCtx,
        point: PonkoVec2,
        max_distance: f32,
        filter: PonkoQueryFilter2D,
        groups: PonkoCollisionGroups,
        out_found: *mut bool,
        out_hit: *mut PonkoPointHit2D,
    ) -> PonkoStatus {
        boundary(|| {
            if out_found.is_null() || out_hit.is_null() {
                return Err(invalid("output is null"));
            }
            let filter = filter_from_c(filter, groups);
            // SAFETY: forwarded caller contract.
            let hit = unsafe { physics_read(ctx, |p| Ok(p.closest_point(vec(point), max_distance, filter)))? };
            // SAFETY: checked non-null; the caller supplies writable outputs.
            unsafe {
                if let Some(h) = hit {
                    out_hit.write(PonkoPointHit2D {
                        entity: to_c(h.entity),
                        point: to_c(h.point),
                        distance: h.distance,
                        inside: h.inside,
                    });
                }
                out(out_found, hit.is_some())
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn penetrations(
        ctx: *mut PonkoCtx,
        shape: PonkoCollider,
        position: PonkoVec2,
        rotation: f32,
        filter: PonkoQueryFilter2D,
        groups: PonkoCollisionGroups,
        out_penetrations: *mut PonkoPenetration2D,
        capacity: usize,
        out_count: *mut usize,
    ) -> PonkoStatus {
        boundary(|| {
            // Sizes are checked as a collider's; the other settings don't matter.
            let shape = Collider::from_c(&PonkoCollider {
                density: 1.0,
                friction: 0.0,
                restitution: 0.0,
                offset: PonkoVec2::default(),
                ..shape
            })?
            .shape;
            let filter = filter_from_c(filter, groups);
            PUSHES.with_borrow_mut(|pushes| {
                // SAFETY: forwarded caller contract.
                unsafe {
                    physics_read(ctx, |p| {
                        p.penetrations(shape, vec(position), rotation, filter, pushes);
                        Ok(())
                    })?;
                    let mut n = 0;
                    collect(out_penetrations, capacity, out_count, &mut n, |sink| {
                        for p in pushes.iter() {
                            sink.push(PonkoPenetration2D {
                                entity: to_c(p.entity),
                                point: to_c(p.point),
                                normal: to_c(p.normal),
                                depth: p.depth,
                            });
                        }
                    })
                }
            })
        })
    }
}

#[cfg(not(feature = "physics"))]
#[allow(clippy::too_many_arguments, dead_code)] // the shapes' sizes aren't read without physics
mod imp {
    use super::*;
    use crate::{boundary, invalid};

    fn off() -> PonkoStatus {
        boundary(|| Err::<(), _>(invalid("2D physics queries need the physics feature")))
    }

    pub(super) enum Cast {
        Ray,
        Circle(f32),
        Box(PonkoVec2, f32),
        Capsule(f32, f32),
    }
    pub(super) enum Overlap {
        Point,
        Circle(f32),
        Box(PonkoVec2, f32),
        Capsule(f32, f32),
    }

    pub(super) unsafe fn set_groups(_: *mut PonkoWorld, _: u64, _: PonkoCollisionGroups) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn groups(_: *mut PonkoWorld, _: u64, _: *mut PonkoCollisionGroups) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn cast(
        _: *mut PonkoCtx,
        _: Cast,
        _: PonkoVec2,
        _: PonkoVec2,
        _: f32,
        _: PonkoQueryFilter2D,
        _: PonkoCollisionGroups,
        _: *mut bool,
        _: *mut PonkoRayHit,
    ) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn cast_all(
        _: *mut PonkoCtx,
        _: PonkoVec2,
        _: PonkoVec2,
        _: f32,
        _: PonkoQueryFilter2D,
        _: PonkoCollisionGroups,
        _: *mut PonkoRayHit,
        _: usize,
        _: *mut usize,
    ) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn overlap(
        _: *mut PonkoCtx,
        _: Overlap,
        _: PonkoVec2,
        _: PonkoQueryFilter2D,
        _: PonkoCollisionGroups,
        _: *mut PonkoEntity,
        _: usize,
        _: *mut usize,
    ) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn closest_point(
        _: *mut PonkoCtx,
        _: PonkoVec2,
        _: f32,
        _: PonkoQueryFilter2D,
        _: PonkoCollisionGroups,
        _: *mut bool,
        _: *mut PonkoPointHit2D,
    ) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn penetrations(
        _: *mut PonkoCtx,
        _: PonkoCollider,
        _: PonkoVec2,
        _: f32,
        _: PonkoQueryFilter2D,
        _: PonkoCollisionGroups,
        _: *mut PonkoPenetration2D,
        _: usize,
        _: *mut usize,
    ) -> PonkoStatus {
        off()
    }
}
