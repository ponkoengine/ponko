//! Versioned collision descriptors (STABILITY.md, "Versioned descriptors";
//! the reading rules are in `descriptors.rs`).
//!
//! - `PonkoColliderEx`: a 2D collider with its collision groups and, for a
//!   polygon, its corners, set or read in one call. ABI 2's `PonkoCollider`
//!   has no groups (`ponko_set_collider_groups` sets them separately) and
//!   can't describe a polygon (`ponko_set_polygon_collider`).
//! - `PonkoQueryFilterEx`: one query filter for 2D and 3D queries, with
//!   groups. The `ponko_ctx_*_ex` queries take it by pointer, so it can grow;
//!   ABI 2's 2D queries take the groups as a separate argument and its 3D
//!   ones a fixed `PonkoQueryFilter3D`.
//!
//! Everything here forwards to the ABI 2 calls, so the rules (and the
//! `physics` / `physics3d` features they need) are theirs.

use std::mem::{offset_of, size_of};

use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::types::{PonkoEntity, PonkoVec2};
use crate::*;

/// The end of field `$f` of `$t` (its offset plus its size).
macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

// ---- collider ---------------------------------------------------------------

/// The version of `PonkoColliderEx` this header describes.
pub const PONKO_COLLIDER_EX_VERSION: u32 = 1;

/// A 2D collider, in a layout that can grow (Rust `d2::Collider`):
/// `PonkoCollider` plus its collision groups and polygon corners. Start
/// from `ponko_collider_ex_from`, set it with `ponko_set_collider_ex`:
///
/// ```c
/// PonkoColliderEx c = {.struct_size = sizeof c, .version = PONKO_COLLIDER_EX_VERSION};
/// ponko_collider_ex_from(ponko_collider_circle(2), &c);
/// c.groups = (PonkoCollisionGroups){DEBRIS, ~DEBRIS};
/// ponko_set_collider_ex(world, entity, &c);
/// ```
///
/// Version 1: `collider`, `groups`, `point_count`, `points`.
/// ```
/// use ponko_ffi::*;
/// let mut c: PonkoColliderEx = unsafe { std::mem::zeroed() };
/// (c.struct_size, c.version) = (size_of::<PonkoColliderEx>() as u32, PONKO_COLLIDER_EX_VERSION);
/// assert_eq!(unsafe { ponko_collider_ex_from(ponko_collider_circle(2.0), &mut c) }, PonkoStatus::PONKO_OK);
/// assert_eq!((c.collider.radius, c.groups.filter, c.point_count), (2.0, u32::MAX, 0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoColliderEx {
    /// `sizeof(PonkoColliderEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_COLLIDER_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Shape and surface (`PonkoCollider`). For a polygon,
    /// `PONKO_SHAPE_POLYGON` with its corners in `points`; its `size` is
    /// ignored when set and the corners' bounds when read.
    pub collider: PonkoCollider,
    /// Which colliders it touches and which queries see it. Every group by
    /// default.
    pub groups: PonkoCollisionGroups,
    /// How many of `points` a polygon uses, 3 to 8; 0 for other shapes.
    pub point_count: u32,
    /// A polygon's corners around the entity's position (their convex hull is
    /// used), in any order.
    pub points: [PonkoVec2; 8],
}
const _: () = assert!(offset_of!(PonkoColliderEx, struct_size) == 0 && offset_of!(PonkoColliderEx, version) == 4);

impl Versioned for PonkoColliderEx {
    const NAME: &'static str = "PonkoColliderEx";
    const VERSION: u32 = PONKO_COLLIDER_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, points: [PonkoVec2; 8])),
            _ => None,
        }
    }
}

fn collider_ex(collider: PonkoCollider) -> PonkoColliderEx {
    PonkoColliderEx {
        struct_size: size_of::<PonkoColliderEx>() as u32,
        version: PONKO_COLLIDER_EX_VERSION,
        collider,
        groups: ponko_collision_groups_all(),
        point_count: 0,
        points: [PonkoVec2::default(); 8],
    }
}

/// Fills the fields of `out`'s version from `collider`, in every group, with
/// no polygon corners. `struct_size` and `version` must be set first.
/// # Safety
/// `out` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_collider_ex_from(collider: PonkoCollider, out: *mut PonkoColliderEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(out, &collider_ex(collider)) })
}

/// A status as a result, keeping the error message the call left.
fn step(status: PonkoStatus) -> Result<(), PonkoStatus> {
    if status == PonkoStatus::PONKO_OK {
        Ok(())
    } else {
        Err(status)
    }
}

/// Reads a descriptor (null: an error), keeping the status.
unsafe fn read_or_status<T: Versioned>(ptr: *const T, defaults: T) -> Result<T, PonkoStatus> {
    let mut value = defaults;
    // SAFETY: forwarded caller contract.
    step(boundary(|| unsafe {
        value = read_desc(ptr, defaults)?;
        Ok(())
    }))?;
    Ok(value)
}

/// Gives `entity` this 2D collider, groups included, adding or replacing it
/// (as setting a `PonkoCollider`, or `ponko_set_polygon_collider` for a
/// polygon, then `ponko_set_collider_groups`). Needs the `physics` feature.
/// # Safety
/// Live world, outside query callbacks; `collider` null or a readable
/// descriptor.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_set_collider_ex(std::ptr::null_mut(), 0, std::ptr::null()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_set_collider_ex(
    world: *mut PonkoWorld,
    entity: u64,
    collider: *const PonkoColliderEx,
) -> PonkoStatus {
    let run = || -> Result<(), PonkoStatus> {
        // SAFETY: forwarded caller contract.
        let c = unsafe { read_or_status(collider, collider_ex(ponko_collider_circle(1.0)))? };
        let polygon = c.collider.shape == PonkoShapeKind::PONKO_SHAPE_POLYGON as u32;
        if polygon != (c.point_count != 0) || c.point_count > 8 {
            return Err(boundary(|| {
                Err(invalid(
                    "a polygon collider has 3 to 8 points in point_count; other shapes have 0",
                ))
            }));
        }
        // SAFETY: forwarded caller contract; the points are in `c`.
        unsafe {
            if polygon {
                step(ponko_set_polygon_collider(
                    world,
                    entity,
                    c.points.as_ptr(),
                    c.point_count,
                    c.collider,
                ))?;
            } else {
                let mut id = 0;
                step(ponko_collider_component(world, &mut id))?;
                step(ponko_set(
                    world,
                    entity,
                    id,
                    (&raw const c.collider).cast(),
                    size_of::<PonkoCollider>(),
                ))?;
            }
            step(ponko_set_collider_groups(world, entity, c.groups))
        }
    };
    run().err().unwrap_or(PonkoStatus::PONKO_OK)
}

/// `entity`'s 2D collider, groups and polygon corners included, into the
/// fields of `out`'s version (set its `struct_size` and `version` first).
/// `PONKO_NOT_FOUND` if it has none.
/// # Safety
/// Live world, outside query callbacks; `out` null or a writable descriptor
/// with its header set.
/// ```
/// use ponko_ffi::*;
/// let s = unsafe { ponko_get_collider_ex(std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_get_collider_ex(
    world: *mut PonkoWorld,
    entity: u64,
    out: *mut PonkoColliderEx,
) -> PonkoStatus {
    let run = || -> Result<(), PonkoStatus> {
        if out.is_null() {
            return Err(boundary(|| Err(invalid("PonkoColliderEx output is null"))));
        }
        let mut c = collider_ex(ponko_collider_circle(1.0));
        // SAFETY: forwarded caller contract; the outputs are locals.
        unsafe {
            let mut id = 0;
            step(ponko_collider_component(world, &mut id))?;
            step(ponko_get(
                world,
                entity,
                id,
                (&raw mut c.collider).cast(),
                size_of::<PonkoCollider>(),
            ))?;
            step(ponko_collider_points(
                world,
                entity,
                c.points.as_mut_ptr(),
                &mut c.point_count,
            ))?;
            step(ponko_collider_groups(world, entity, &mut c.groups))?;
            step(boundary(|| write_desc(out, &c)))
        }
    };
    run().err().unwrap_or(PonkoStatus::PONKO_OK)
}

// ---- query filter -----------------------------------------------------------

/// The version of `PonkoQueryFilterEx` this header describes.
pub const PONKO_QUERY_FILTER_EX_VERSION: u32 = 1;

/// Which colliders a 2D or 3D query sees, in a layout that can grow (Rust
/// `d2::QueryFilter` and `d3::QueryFilter`). The `ponko_ctx_*_ex` queries
/// take it by pointer (null: the defaults). Start from
/// `ponko_query_filter_ex_defaults`: every solid collider in every group, no
/// sensors.
///
/// Version 1: `exclude`, `exclude_entity`, `groups`, `sensors`.
/// ```
/// use ponko_ffi::*;
/// let mut f: PonkoQueryFilterEx = unsafe { std::mem::zeroed() };
/// (f.struct_size, f.version) = (size_of::<PonkoQueryFilterEx>() as u32, PONKO_QUERY_FILTER_EX_VERSION);
/// assert_eq!(unsafe { ponko_query_filter_ex_defaults(&mut f) }, PonkoStatus::PONKO_OK);
/// assert_eq!((f.exclude, f.groups.memberships, f.sensors), (false, u32::MAX, false));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoQueryFilterEx {
    /// `sizeof(PonkoQueryFilterEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_QUERY_FILTER_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Whether to look through `exclude_entity` (the one asking, usually).
    pub exclude: bool,
    /// The entity to look through, with `exclude`.
    pub exclude_entity: u64,
    /// The query touches colliders as a collider with these groups would.
    /// Every group by default.
    pub groups: PonkoCollisionGroups,
    /// Sees sensors as well.
    pub sensors: bool,
}
const _: () = assert!(offset_of!(PonkoQueryFilterEx, struct_size) == 0 && offset_of!(PonkoQueryFilterEx, version) == 4);

impl Versioned for PonkoQueryFilterEx {
    const NAME: &'static str = "PonkoQueryFilterEx";
    const VERSION: u32 = PONKO_QUERY_FILTER_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, sensors: bool)),
            _ => None,
        }
    }
}

fn filter_defaults() -> PonkoQueryFilterEx {
    PonkoQueryFilterEx {
        struct_size: size_of::<PonkoQueryFilterEx>() as u32,
        version: PONKO_QUERY_FILTER_EX_VERSION,
        exclude: false,
        exclude_entity: 0,
        groups: ponko_collision_groups_all(),
        sensors: false,
    }
}

/// Fills the fields of `filter`'s version with the defaults: every solid
/// collider in every group, no sensors. `struct_size` and `version` must be
/// set first.
/// # Safety
/// `filter` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_query_filter_ex_defaults(filter: *mut PonkoQueryFilterEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(filter, &filter_defaults()) })
}

/// A caller's filter, or the defaults for null.
unsafe fn filter_of(filter: *const PonkoQueryFilterEx) -> Result<PonkoQueryFilterEx, PonkoStatus> {
    if filter.is_null() {
        return Ok(filter_defaults());
    }
    // SAFETY: forwarded caller contract.
    unsafe { read_or_status(filter, filter_defaults()) }
}

/// What the ABI 2 2D queries take for a versioned filter.
unsafe fn filter_2d(
    filter: *const PonkoQueryFilterEx,
) -> Result<(PonkoQueryFilter2D, PonkoCollisionGroups), PonkoStatus> {
    // SAFETY: forwarded caller contract.
    let f = unsafe { filter_of(filter)? };
    Ok((
        PonkoQueryFilter2D {
            exclude: f.exclude,
            exclude_entity: f.exclude_entity,
            sensors: f.sensors,
        },
        f.groups,
    ))
}

/// What the ABI 2 3D queries take for a versioned filter.
unsafe fn filter_3d(filter: *const PonkoQueryFilterEx) -> Result<PonkoQueryFilter3D, PonkoStatus> {
    // SAFETY: forwarded caller contract.
    let f = unsafe { filter_of(filter)? };
    Ok(PonkoQueryFilter3D {
        exclude: f.exclude,
        exclude_entity: f.exclude_entity,
        groups: f.groups,
        sensors: f.sensors,
    })
}

// ---- queries with a versioned filter ----------------------------------------

/// `ponko_ctx_cast_ray2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_ray2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_ray2d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    direction: PonkoVec2,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_cast_ray2d(ctx, origin, direction, max_distance, f, g, out_found, out_hit) }
}

/// `ponko_ctx_cast_ray2d_all` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_ray2d_all`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_ray2d_all_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    direction: PonkoVec2,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_hits: *mut PonkoRayHit,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe {
        ponko_ctx_cast_ray2d_all(
            ctx,
            origin,
            direction,
            max_distance,
            f,
            g,
            out_hits,
            capacity,
            out_count,
        )
    }
}

/// `ponko_ctx_cast_circle2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_circle2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_circle2d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    radius: f32,
    direction: PonkoVec2,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_cast_circle2d(ctx, origin, radius, direction, max_distance, f, g, out_found, out_hit) }
}

/// `ponko_ctx_cast_box2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_box2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_box2d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    size: PonkoVec2,
    rotation: f32,
    direction: PonkoVec2,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe {
        ponko_ctx_cast_box2d(
            ctx,
            origin,
            size,
            rotation,
            direction,
            max_distance,
            f,
            g,
            out_found,
            out_hit,
        )
    }
}

/// `ponko_ctx_cast_capsule2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_capsule2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_capsule2d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    height: f32,
    radius: f32,
    direction: PonkoVec2,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe {
        ponko_ctx_cast_capsule2d(
            ctx,
            origin,
            height,
            radius,
            direction,
            max_distance,
            f,
            g,
            out_found,
            out_hit,
        )
    }
}

/// `ponko_ctx_overlap_point2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_overlap_point2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_point2d_ex(
    ctx: *mut PonkoCtx,
    point: PonkoVec2,
    filter: *const PonkoQueryFilterEx,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_overlap_point2d(ctx, point, f, g, out_entities, capacity, out_count) }
}

/// `ponko_ctx_overlap_circle2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_overlap_circle2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_circle2d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    radius: f32,
    filter: *const PonkoQueryFilterEx,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_overlap_circle2d(ctx, center, radius, f, g, out_entities, capacity, out_count) }
}

/// `ponko_ctx_overlap_box2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_overlap_box2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_box2d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    size: PonkoVec2,
    rotation: f32,
    filter: *const PonkoQueryFilterEx,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_overlap_box2d(ctx, center, size, rotation, f, g, out_entities, capacity, out_count) }
}

/// `ponko_ctx_overlap_capsule2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_overlap_capsule2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_capsule2d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    height: f32,
    radius: f32,
    filter: *const PonkoQueryFilterEx,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_overlap_capsule2d(ctx, center, height, radius, f, g, out_entities, capacity, out_count) }
}

/// `ponko_ctx_closest_point2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_closest_point2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_closest_point2d_ex(
    ctx: *mut PonkoCtx,
    point: PonkoVec2,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoPointHit2D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_closest_point2d(ctx, point, max_distance, f, g, out_found, out_hit) }
}

/// `ponko_ctx_penetrations2d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_penetrations2d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_penetrations2d_ex(
    ctx: *mut PonkoCtx,
    shape: PonkoCollider,
    position: PonkoVec2,
    rotation: f32,
    filter: *const PonkoQueryFilterEx,
    out_penetrations: *mut PonkoPenetration2D,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let (f, g) = match unsafe { filter_2d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe {
        ponko_ctx_penetrations2d(
            ctx,
            shape,
            position,
            rotation,
            f,
            g,
            out_penetrations,
            capacity,
            out_count,
        )
    }
}

/// `ponko_ctx_cast_ray3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_ray3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_ray3d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec3,
    direction: PonkoVec3,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoHit3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_cast_ray3d(ctx, origin, direction, max_distance, f, out_found, out_hit) }
}

/// `ponko_ctx_cast_ray3d_all` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_ray3d_all`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_ray3d_all_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec3,
    direction: PonkoVec3,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_hits: *mut PonkoHit3D,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_cast_ray3d_all(ctx, origin, direction, max_distance, f, out_hits, capacity, out_count) }
}

/// `ponko_ctx_cast_sphere3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_sphere3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_sphere3d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec3,
    radius: f32,
    direction: PonkoVec3,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoHit3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_cast_sphere3d(ctx, origin, radius, direction, max_distance, f, out_found, out_hit) }
}

/// `ponko_ctx_cast_capsule3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_capsule3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_capsule3d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec3,
    height: f32,
    radius: f32,
    direction: PonkoVec3,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoHit3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe {
        ponko_ctx_cast_capsule3d(
            ctx,
            origin,
            height,
            radius,
            direction,
            max_distance,
            f,
            out_found,
            out_hit,
        )
    }
}

/// `ponko_ctx_cast_box3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_cast_box3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_cast_box3d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec3,
    size: PonkoVec3,
    rotation: PonkoQuat,
    direction: PonkoVec3,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoHit3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe {
        ponko_ctx_cast_box3d(
            ctx,
            origin,
            size,
            rotation,
            direction,
            max_distance,
            f,
            out_found,
            out_hit,
        )
    }
}

/// `ponko_ctx_overlap_point3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_overlap_point3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_point3d_ex(
    ctx: *mut PonkoCtx,
    point: PonkoVec3,
    filter: *const PonkoQueryFilterEx,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_overlap_point3d(ctx, point, f, out_entities, capacity, out_count) }
}

/// `ponko_ctx_overlap_sphere3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_overlap_sphere3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_sphere3d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec3,
    radius: f32,
    filter: *const PonkoQueryFilterEx,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_overlap_sphere3d(ctx, center, radius, f, out_entities, capacity, out_count) }
}

/// `ponko_ctx_overlap_box3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_overlap_box3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_box3d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec3,
    size: PonkoVec3,
    rotation: PonkoQuat,
    filter: *const PonkoQueryFilterEx,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_overlap_box3d(ctx, center, size, rotation, f, out_entities, capacity, out_count) }
}

/// `ponko_ctx_overlap_capsule3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_overlap_capsule3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_overlap_capsule3d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec3,
    height: f32,
    radius: f32,
    filter: *const PonkoQueryFilterEx,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_overlap_capsule3d(ctx, center, height, radius, f, out_entities, capacity, out_count) }
}

/// `ponko_ctx_closest_point3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_closest_point3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_closest_point3d_ex(
    ctx: *mut PonkoCtx,
    point: PonkoVec3,
    max_distance: f32,
    filter: *const PonkoQueryFilterEx,
    out_found: *mut bool,
    out_hit: *mut PonkoPointHit3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_closest_point3d(ctx, point, max_distance, f, out_found, out_hit) }
}

/// `ponko_ctx_penetrations3d` seeing through a versioned filter (`filter` null:
/// the defaults, as `ponko_query_filter_ex_defaults`).
/// # Safety
/// As `ponko_ctx_penetrations3d`; `filter` null or a readable descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_penetrations3d_ex(
    ctx: *mut PonkoCtx,
    shape: PonkoCollider3D,
    position: PonkoVec3,
    rotation: PonkoQuat,
    filter: *const PonkoQueryFilterEx,
    out_penetrations: *mut PonkoPenetration3D,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let f = match unsafe { filter_3d(filter) } {
        Ok(x) => x,
        Err(status) => return status,
    };
    // SAFETY: forwarded caller contract.
    unsafe { ponko_ctx_penetrations3d(ctx, shape, position, rotation, f, out_penetrations, capacity, out_count) }
}
