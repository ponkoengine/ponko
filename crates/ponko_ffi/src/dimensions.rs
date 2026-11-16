//! Additive native access to dimension-independent query and prefab placement behavior.
use std::ffi::c_char;

use crate::ctx::{ctx_access, out, text};
use crate::types::{PonkoEntity, PonkoTransform2D, PonkoVec2, to_c};
use crate::{PonkoCtx, PonkoRayHit, PonkoStatus, PonkoTransform3D, PonkoVec3, boundary, invalid, invalid_owned};

/// Which colliders a filtered 2D query sees; zero initialization selects solids only.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoQueryFilter2D {
    /// Look through `exclude_entity` when true.
    pub exclude: bool,
    /// Entity ignored when `exclude` is true.
    pub exclude_entity: u64,
    /// Include sensors as well as solids.
    pub sensors: bool,
}

#[cfg(feature = "physics")]
impl PonkoQueryFilter2D {
    fn into_engine(self) -> ponko::d2::QueryFilter {
        ponko::d2::QueryFilter {
            exclude: self.exclude.then(|| ponko_ecs::Entity::from_bits(self.exclude_entity)),
            groups: ponko::d2::CollisionGroups::ALL,
            sensors: self.sensors,
        }
    }
}

/// Closest 2D hit using an explicit filter. The old three-argument ray API is unchanged.
/// # Safety
/// Live context outside query callbacks; both outputs writable and non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_cast_ray_filtered(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    direction: PonkoVec2,
    max_distance: f32,
    filter: PonkoQueryFilter2D,
    out_found: *mut bool,
    out_hit: *mut PonkoRayHit,
) -> PonkoStatus {
    boundary(|| {
        if out_found.is_null() || out_hit.is_null() {
            return Err(invalid("output is null"));
        }
        #[cfg(feature = "physics")]
        {
            if !origin.x.is_finite()
                || !origin.y.is_finite()
                || !direction.x.is_finite()
                || !direction.y.is_finite()
                || !max_distance.is_finite()
                || max_distance < 0.0
            {
                return Err(invalid("ray values must be finite and distance non-negative"));
            }
            // SAFETY: the caller supplies a live context; ctx_access enforces its borrow guard.
            let hit = unsafe {
                ctx_access(ctx, |c| {
                    Ok(c.physics
                        .cast_ray(to_c(origin), to_c(direction), max_distance, filter.into_engine()))
                })?
            };
            // SAFETY: outputs were checked non-null and the caller supplies writable storage.
            unsafe {
                if let Some(h) = hit {
                    out_hit.write(PonkoRayHit {
                        entity: to_c(h.entity),
                        point: to_c(h.point),
                        normal: to_c(h.normal),
                        distance: h.distance,
                    });
                }
                out(out_found, hit.is_some())
            }
        }
        #[cfg(not(feature = "physics"))]
        {
            let _ = (ctx, origin, direction, max_distance, filter);
            Err(invalid("filtered 2D queries require the physics feature"))
        }
    })
}

/// Filtered ball overlap (a circle in 2D). Counts all matches and writes up to capacity.
/// # Safety
/// Live context outside query callbacks; output writable for capacity elements (null for
/// zero capacity is allowed), and a writable non-null count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_overlap_ball(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    radius: f32,
    filter: PonkoQueryFilter2D,
    out_entities: *mut PonkoEntity,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if out_count.is_null() || (out_entities.is_null() && capacity != 0) {
            return Err(invalid("output buffer or count is null"));
        }
        #[cfg(feature = "physics")]
        {
            use std::cell::RefCell;
            thread_local! { static FOUND: RefCell<Vec<ponko_ecs::Entity>> = const { RefCell::new(Vec::new()) }; }
            if !center.x.is_finite() || !center.y.is_finite() || !radius.is_finite() || radius < 0.0 {
                return Err(invalid("overlap center must be finite and radius non-negative"));
            }
            FOUND.with_borrow_mut(|found| {
                // SAFETY: the caller's context and output contracts are forwarded; ctx_access
                // guards the world and collect validates/caps every output write.
                unsafe {
                    ctx_access(ctx, |c| {
                        c.physics
                            .overlap_circle(to_c(center), radius, filter.into_engine(), found);
                        Ok(())
                    })?;
                    let mut n = 0;
                    crate::ctx::collect(out_entities, capacity, out_count, &mut n, |sink| {
                        for e in found {
                            sink.push(to_c(*e));
                        }
                    })
                }
            })
        }
        #[cfg(not(feature = "physics"))]
        {
            let _ = (ctx, center, radius, filter);
            Err(invalid("filtered 2D queries require the physics feature"))
        }
    })
}

unsafe fn spawn_placed(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    at: ponko::prelude::Placement,
    output: *mut u64,
) -> PonkoStatus {
    boundary(|| {
        if output.is_null() {
            return Err(invalid("entity output is null"));
        }
        // SAFETY: forwarded live-context/string/output contract; validate strings before spawning.
        unsafe {
            let path = text(path)?;
            let entity = ctx_access(ctx, |c| c.spawn_prefab(path, at).map_err(invalid_owned))?;
            out(output, entity.to_bits())
        }
    })
}

/// Repositions a 3D prefab, preserving its saved rotation and scale.
/// # Safety
/// Live context, NUL-terminated UTF-8 path, writable non-null entity output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_spawn_prefab3d(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    at: PonkoVec3,
    output: *mut u64,
) -> PonkoStatus {
    let position: ponko::prelude::Vec3 = to_c(at);
    // SAFETY: forwards the caller's context, path and output contract.
    unsafe { spawn_placed(ctx, path, position.into(), output) }
}

/// Places a 3D prefab with a complete position, rotation and scale.
/// # Safety
/// Live context, NUL-terminated UTF-8 path, writable non-null entity output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_spawn_prefab_transform3d(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    at: PonkoTransform3D,
    output: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwards the caller's context, path and output contract.
    unsafe { spawn_placed(ctx, path, crate::three::transform_from_c(at).into(), output) }
}

/// Places a 2D prefab with a complete position, rotation and scale; the old Vec2 API stays available.
/// # Safety
/// Live context, NUL-terminated UTF-8 path, writable non-null entity output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_spawn_prefab_transform2d(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    at: PonkoTransform2D,
    output: *mut u64,
) -> PonkoStatus {
    let transform: ponko::d2::Transform = to_c(at);
    // SAFETY: forwards the caller's context, path and output contract.
    unsafe { spawn_placed(ctx, path, transform.into(), output) }
}
