//! The 3D physics queries 2D also has (the `physics3d` feature): an oriented
//! box cast, a point overlap, the closest point and penetration depths. The
//! same rules as Rust's `d3::Physics` queries (and the 2D ones): values pass
//! through unchanged, so a zero direction, a negative range or a non-finite
//! origin or rotation finds nothing rather than failing. Without the feature
//! every call here fails with an error naming it.
//!
//! The queries only read the physics world, so they work inside a
//! `ponko_query` callback too (a ground check for each entity a query
//! visits), though not in `ponko_query_par`.
//!
//! `PonkoQueryFilter3D` already carries collision groups, so unlike the 2D
//! calls these take no separate groups argument.
use crate::three::{PonkoQuat, PonkoVec3};
use crate::types::PonkoEntity;
use crate::{PonkoCollider3D, PonkoCtx, PonkoHit3D, PonkoQueryFilter3D, PonkoStatus};

/// The nearest point of a 3D collider to a point
/// (`ponko_ctx_closest_point3d`; Rust `d3::PointHit`).
/// ```
/// let h = ponko_ffi::PonkoPointHit3D::default();
/// assert!(!h.inside);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoPointHit3D {
    /// Whose collider.
    pub entity: PonkoEntity,
    /// Its nearest point: the query point itself when that is inside.
    pub point: PonkoVec3,
    /// From the query point to `point` (0 inside).
    pub distance: f32,
    /// Whether the query point is inside the collider.
    pub inside: bool,
}

/// How deep a shape sinks into one 3D collider and the way out
/// (`ponko_ctx_penetrations3d`; Rust `d3::Penetration`).
/// ```
/// let p = ponko_ffi::PonkoPenetration3D::default();
/// assert_eq!(p.depth, 0.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoPenetration3D {
    /// Whose collider.
    pub entity: PonkoEntity,
    /// The collider's deepest point inside the shape.
    pub point: PonkoVec3,
    /// The direction that moves the shape out.
    pub normal: PonkoVec3,
    /// How far to move along `normal` to just touch (0 when they only touch).
    pub depth: f32,
}

/// Sweeps a box of `size` centered on `origin` and turned by `rotation`
/// towards `direction`: the first collider it touches within `max_distance`
/// (Rust `d3::Physics::cast_box`). Outputs as `ponko_ctx_cast_ray3d`.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); writable outputs.
/// ```
/// use ponko_ffi::*;
/// let (mut found, mut hit) = (false, PonkoHit3D::default());
/// let s = unsafe { ponko_ctx_cast_box3d(std::ptr::null_mut(), Default::default(), Default::default(), ponko_quat_identity(), Default::default(), 1.0, ponko_query_filter3d(), &mut found, &mut hit) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_box3d(
    ctx: *mut PonkoCtx,
    origin: PonkoVec3,
    size: PonkoVec3,
    rotation: PonkoQuat,
    direction: PonkoVec3,
    max_distance: f32,
    filter: PonkoQueryFilter3D,
    out_found: *mut bool,
    out_hit: *mut PonkoHit3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        imp::cast_box(
            ctx,
            origin,
            size,
            rotation,
            direction,
            max_distance,
            filter,
            out_found,
            out_hit,
        )
    }
}

/// Entities whose 3D collider contains `point` (Rust
/// `d3::Physics::overlap_point`). Writes up to `capacity` of them and sets
/// `*out_count` to how many there are in total.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); `out_entities` writable for
/// `capacity` (may be null if 0); writable count.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_ctx_overlap_point3d(std::ptr::null_mut(), Default::default(), ponko_query_filter3d(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_overlap_point3d(
    ctx: *mut PonkoCtx,
    point: PonkoVec3,
    filter: PonkoQueryFilter3D,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::overlap_point(ctx, point, filter, out_entities, capacity, out_count) }
}

/// The nearest 3D collider to `point` within `max_distance` and its nearest
/// point (Rust `d3::Physics::closest_point`); a point inside a collider is
/// its own nearest point. `*out_found` says whether one was found;
/// `*out_hit` is written only then.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); writable outputs.
/// ```
/// use ponko_ffi::*;
/// let (mut found, mut hit) = (false, PonkoPointHit3D::default());
/// let s = unsafe { ponko_ctx_closest_point3d(std::ptr::null_mut(), Default::default(), 10.0, ponko_query_filter3d(), &mut found, &mut hit) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_closest_point3d(
    ctx: *mut PonkoCtx,
    point: PonkoVec3,
    max_distance: f32,
    filter: PonkoQueryFilter3D,
    out_found: *mut bool,
    out_hit: *mut PonkoPointHit3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::closest_point(ctx, point, max_distance, filter, out_found, out_hit) }
}

/// Every 3D collider that `shape` (its kind, sizes and mesh; density,
/// friction, restitution, sensor, offset and groups are ignored), placed at
/// `position` and turned by `rotation`, overlaps: how deep and which way
/// out, deepest first (equal depths by entity id; Rust
/// `d3::Physics::penetrations`). Writes up to `capacity` and sets
/// `*out_count` to how many there are. An invalid shape is
/// `PONKO_INVALID_ARGUMENT`; a freed mesh finds nothing.
/// # Safety
/// Live ctx (inside a `ponko_query` callback too, not `ponko_query_par`); `out_penetrations` writable for
/// `capacity` (may be null if 0); writable count.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_ctx_penetrations3d(std::ptr::null_mut(), ponko_collider3d_sphere(0.5), Default::default(), ponko_quat_identity(), ponko_query_filter3d(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_penetrations3d(
    ctx: *mut PonkoCtx,
    shape: PonkoCollider3D,
    position: PonkoVec3,
    rotation: PonkoQuat,
    filter: PonkoQueryFilter3D,
    out_penetrations: *mut PonkoPenetration3D,
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
            out_penetrations,
            capacity,
            out_count,
        )
    }
}

#[cfg(feature = "physics3d")]
mod imp {
    use std::cell::RefCell;

    use ponko::d3::{Collider, CollisionGroups, Hit, Penetration, QueryFilter};
    use ponko::prelude::{Quat, Vec3};
    use ponko_ecs::Entity;

    use super::*;
    use crate::ctx::{collect, out, physics3d_read};
    use crate::types::to_c;
    use crate::{CMirror, boundary, invalid};

    fn filter_from_c(f: PonkoQueryFilter3D) -> QueryFilter {
        QueryFilter {
            exclude: f.exclude.then(|| Entity::from_bits(f.exclude_entity)),
            groups: CollisionGroups::new(f.groups.memberships, f.groups.filter),
            sensors: f.sensors,
        }
    }

    fn hit_to_c(h: Hit) -> PonkoHit3D {
        PonkoHit3D {
            entity: to_c(h.entity),
            point: to_c(h.point),
            normal: to_c(h.normal),
            distance: h.distance,
        }
    }

    fn vec(v: PonkoVec3) -> Vec3 {
        Vec3::new(v.x, v.y, v.z)
    }

    fn quat(q: PonkoQuat) -> Quat {
        Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }

    thread_local! {
        /// Reused between queries, so they don't allocate each time.
        static FOUND: RefCell<Vec<Entity>> = const { RefCell::new(Vec::new()) };
        static PUSHES: RefCell<Vec<Penetration>> = const { RefCell::new(Vec::new()) };
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn cast_box(
        ctx: *mut PonkoCtx,
        origin: PonkoVec3,
        size: PonkoVec3,
        rotation: PonkoQuat,
        direction: PonkoVec3,
        max_distance: f32,
        filter: PonkoQueryFilter3D,
        out_found: *mut bool,
        out_hit: *mut PonkoHit3D,
    ) -> PonkoStatus {
        boundary(|| {
            if out_found.is_null() || out_hit.is_null() {
                return Err(invalid("output is null"));
            }
            let filter = filter_from_c(filter);
            // SAFETY: forwarded caller contract.
            let hit = unsafe {
                physics3d_read(ctx, |p| {
                    Ok(p.cast_box(
                        vec(origin),
                        vec(size),
                        quat(rotation),
                        vec(direction),
                        max_distance,
                        filter,
                    ))
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

    pub(super) unsafe fn overlap_point(
        ctx: *mut PonkoCtx,
        point: PonkoVec3,
        filter: PonkoQueryFilter3D,
        out_entities: *mut PonkoEntity,
        capacity: usize,
        out_count: *mut usize,
    ) -> PonkoStatus {
        boundary(|| {
            FOUND.with_borrow_mut(|found| {
                let filter = filter_from_c(filter);
                // SAFETY: forwarded caller contract.
                unsafe {
                    physics3d_read(ctx, |p| {
                        p.overlap_point(vec(point), filter, found);
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
        point: PonkoVec3,
        max_distance: f32,
        filter: PonkoQueryFilter3D,
        out_found: *mut bool,
        out_hit: *mut PonkoPointHit3D,
    ) -> PonkoStatus {
        boundary(|| {
            if out_found.is_null() || out_hit.is_null() {
                return Err(invalid("output is null"));
            }
            let filter = filter_from_c(filter);
            // SAFETY: forwarded caller contract.
            let hit = unsafe { physics3d_read(ctx, |p| Ok(p.closest_point(vec(point), max_distance, filter)))? };
            // SAFETY: checked non-null; the caller supplies writable outputs.
            unsafe {
                if let Some(h) = hit {
                    out_hit.write(PonkoPointHit3D {
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
        shape: PonkoCollider3D,
        position: PonkoVec3,
        rotation: PonkoQuat,
        filter: PonkoQueryFilter3D,
        out_penetrations: *mut PonkoPenetration3D,
        capacity: usize,
        out_count: *mut usize,
    ) -> PonkoStatus {
        boundary(|| {
            // Sizes are checked as a collider's; the other settings don't matter.
            let shape = Collider::from_c(&PonkoCollider3D {
                density: 1.0,
                friction: 0.0,
                restitution: 0.0,
                offset: PonkoVec3::default(),
                ..shape
            })?
            .shape;
            let filter = filter_from_c(filter);
            PUSHES.with_borrow_mut(|pushes| {
                // SAFETY: forwarded caller contract.
                unsafe {
                    physics3d_read(ctx, |p| {
                        p.penetrations(shape, vec(position), quat(rotation), filter, pushes);
                        Ok(())
                    })?;
                    let mut n = 0;
                    collect(out_penetrations, capacity, out_count, &mut n, |sink| {
                        for p in pushes.iter() {
                            sink.push(PonkoPenetration3D {
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

#[cfg(not(feature = "physics3d"))]
#[allow(clippy::too_many_arguments)]
mod imp {
    use super::*;
    use crate::{boundary, invalid};

    fn off() -> PonkoStatus {
        boundary(|| Err::<(), _>(invalid("3D physics queries need the physics3d feature")))
    }

    pub(super) unsafe fn cast_box(
        _: *mut PonkoCtx,
        _: PonkoVec3,
        _: PonkoVec3,
        _: PonkoQuat,
        _: PonkoVec3,
        _: f32,
        _: PonkoQueryFilter3D,
        _: *mut bool,
        _: *mut PonkoHit3D,
    ) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn overlap_point(
        _: *mut PonkoCtx,
        _: PonkoVec3,
        _: PonkoQueryFilter3D,
        _: *mut PonkoEntity,
        _: usize,
        _: *mut usize,
    ) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn closest_point(
        _: *mut PonkoCtx,
        _: PonkoVec3,
        _: f32,
        _: PonkoQueryFilter3D,
        _: *mut bool,
        _: *mut PonkoPointHit3D,
    ) -> PonkoStatus {
        off()
    }
    pub(super) unsafe fn penetrations(
        _: *mut PonkoCtx,
        _: PonkoCollider3D,
        _: PonkoVec3,
        _: PonkoQuat,
        _: PonkoQueryFilter3D,
        _: *mut PonkoPenetration3D,
        _: usize,
        _: *mut usize,
    ) -> PonkoStatus {
        off()
    }
}
