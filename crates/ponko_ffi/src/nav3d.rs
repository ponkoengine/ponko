//! 3D navigation meshes from C (Rust `NavMesh3D`, `NavSource`,
//! `NavBakeSettings`, `NavAgent3D`, `NavObstacle3D`, `step_navmesh3d`).
//! Gather walkable triangles in a `PonkoNavSources`, bake them into the
//! context's navmesh, then query paths or give entities agents.

use ponko::d3::NavAgent as NavAgent3D;
use ponko::d3::NavMesh as NavMesh3D;
use ponko::d3::NavObstacle as NavObstacle3D;
use ponko::nav::NavBakeSettings;
use ponko::nav::NavSource;
use ponko::prelude::{Ctx, Entity, IntoSystem, Vec3};
use ponko::systems::{step_navmesh3d, step_navmesh3d_access};

use crate::ctx::{ctx_access, out};
use crate::game::{build, ok};
use crate::three::{PonkoMeshData, PonkoTransform3D, PonkoVec3, transform_from_c};
use crate::types::to_c;
use crate::{Failure, PonkoCtx, PonkoGame, PonkoStatus, boundary, error, invalid, invalid_owned};

/// Walkable geometry to bake, from `ponko_nav_sources_new`. Free it with
/// `ponko_nav_sources_free`.
#[derive(Default)]
pub struct PonkoNavSources(Vec<NavSource>);

/// How a navmesh is baked (Rust `NavBakeSettings`). Start from
/// `ponko_nav_bake_settings_default()`.
/// ```
/// assert_eq!(ponko_ffi::ponko_nav_bake_settings_default().agent_height, 1.8);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoNavBakeSettings {
    /// How far agents keep from walls, in meters. Default 0.3.
    pub agent_radius: f32,
    /// The headroom agents need. Default 1.8.
    pub agent_height: f32,
    /// The highest step agents climb. Default 0.4.
    pub max_climb: f32,
    /// The steepest walkable slope, in degrees. Default 45.
    pub max_slope: f32,
    /// Sampling size across the ground; smaller is finer and slower. Default 0.2.
    pub cell_size: f32,
    /// Sampling size in height. Default 0.1.
    pub cell_height: f32,
    /// Cells per tile side (8 to 255), grown automatically for very large levels
    /// (a navmesh holds at most 16,384 tiles). Default 32.
    pub tile_size: u32,
    /// Keep cached layers so carving obstacles can cut real holes. Costs bake
    /// time and memory; without it carving obstacles block like ordinary ones.
    /// Default false.
    pub carving: bool,
}

/// The default bake: a 0.3 m wide, 1.8 m tall agent on 45° slopes.
/// ```
/// assert_eq!(ponko_ffi::ponko_nav_bake_settings_default().max_slope, 45.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_nav_bake_settings_default() -> PonkoNavBakeSettings {
    settings_to_c(NavBakeSettings::default())
}
fn settings_to_c(s: NavBakeSettings) -> PonkoNavBakeSettings {
    PonkoNavBakeSettings {
        agent_radius: s.agent_radius,
        agent_height: s.agent_height,
        max_climb: s.max_climb,
        max_slope: s.max_slope,
        cell_size: s.cell_size,
        cell_height: s.cell_height,
        tile_size: s.tile_size,
        carving: s.carving,
    }
}
fn settings_from_c(s: &PonkoNavBakeSettings) -> NavBakeSettings {
    NavBakeSettings {
        agent_radius: s.agent_radius,
        agent_height: s.agent_height,
        max_climb: s.max_climb,
        max_slope: s.max_slope,
        cell_size: s.cell_size,
        cell_height: s.cell_height,
        tile_size: s.tile_size,
        carving: s.carving,
    }
}
fn sources<'a>(s: *mut PonkoNavSources) -> Result<&'a mut PonkoNavSources, Failure> {
    // SAFETY: callers pass null or live sources.
    unsafe { s.as_mut() }.ok_or_else(|| invalid("sources is null"))
}
fn entity(c: &Ctx, bits: u64) -> Result<Entity, Failure> {
    let e = Entity::from_bits(bits);
    if c.world.is_alive(e) {
        Ok(e)
    } else {
        Err(invalid("no such entity"))
    }
}
fn navmesh(c: &Ctx) -> Result<&NavMesh3D, Failure> {
    c.world
        .get_resource::<NavMesh3D>()
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no navmesh: bake one first"))
}

/// An empty set of walkable geometry.
///
/// Ownership: the returned source list is the caller's (null on failure); free
/// it with ponko_nav_sources_free.
///
/// ```
/// let s = ponko_ffi::ponko_nav_sources_new();
/// unsafe { ponko_ffi::ponko_nav_sources_free(s) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_nav_sources_new() -> *mut PonkoNavSources {
    Box::into_raw(Box::default())
}

/// Frees sources. Null does nothing.
/// ```
/// unsafe { ponko_ffi::ponko_nav_sources_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `sources` null or live; not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_sources_free(sources: *mut PonkoNavSources) {
    if !sources.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(sources) });
    }
}

/// Adds `count` world-space triangles: `3 * count` corners at `corners`,
/// counter-clockwise seen from above.
/// ```
/// let s = ponko_ffi::ponko_nav_sources_new();
/// assert_eq!(unsafe { ponko_ffi::ponko_nav_sources_add_triangles(s, std::ptr::null(), 0) } as u32, 0);
/// unsafe { ponko_ffi::ponko_nav_sources_free(s) };
/// ```
/// # Safety
/// `sources` null or live; `corners` readable for `3 * count` points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_sources_add_triangles(
    sources_: *mut PonkoNavSources,
    corners: *const PonkoVec3,
    count: usize,
) -> PonkoStatus {
    boundary(|| {
        let s = sources(sources_)?;
        if count == 0 {
            return Ok(());
        }
        if corners.is_null() {
            return Err(invalid("corners is null"));
        }
        let len = count.checked_mul(3).ok_or_else(|| invalid("too many triangles"))?;
        // SAFETY: forwarded caller contract.
        let corners = unsafe { std::slice::from_raw_parts(corners, len) };
        let triangles = corners
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| [to_c(t[0]), to_c(t[1]), to_c(t[2])])
            .collect::<Vec<[Vec3; 3]>>();
        s.0.push(NavSource { triangles });
        Ok(())
    })
}

/// Adds a mesh's triangles placed by `transform` (e.g. the ground's model).
/// ```
/// let t = ponko_ffi::ponko_transform3d_at(Default::default());
/// assert_eq!(unsafe { ponko_ffi::ponko_nav_sources_add_mesh(std::ptr::null_mut(), std::ptr::null(), t) } as u32, 1);
/// ```
/// # Safety
/// `sources` null or live; `data` null or live mesh data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_sources_add_mesh(
    sources_: *mut PonkoNavSources,
    data: *const PonkoMeshData,
    transform: PonkoTransform3D,
) -> PonkoStatus {
    boundary(|| {
        let s = sources(sources_)?;
        // SAFETY: forwarded caller contract.
        let data = unsafe { data.as_ref() }.ok_or_else(|| invalid("mesh data is null"))?;
        s.0.push(NavSource::from_mesh(&data.0, transform_from_c(transform)));
        Ok(())
    })
}

/// Bakes `sources` into this context's navmesh, replacing any old one
/// (agents replan). `settings` may be null for the defaults. Fails with the
/// reason for empty or broken geometry.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_bake_navmesh(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `sources` null or live; `settings` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_bake_navmesh(
    ctx: *mut PonkoCtx,
    sources_: *const PonkoNavSources,
    settings: *const PonkoNavBakeSettings,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = sources_.as_ref().ok_or_else(|| invalid("sources is null"))?;
            let settings = settings.as_ref().map_or_else(NavBakeSettings::default, settings_from_c);
            let nav = NavMesh3D::bake(&s.0, settings).map_err(|e| invalid_owned(format!("navmesh: {e}")))?;
            c.world.insert_resource(nav);
            Ok(())
        })
    })
}

/// The nearest walkable point within `max_distance` of `point`;
/// `*out_found` is false when there is none.
/// ```
/// let p = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_navmesh_sample_position(std::ptr::null_mut(), p, 1.0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_navmesh_sample_position(
    ctx: *mut PonkoCtx,
    point: PonkoVec3,
    max_distance: f32,
    out_point: *mut PonkoVec3,
    out_found: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_point.is_null() || out_found.is_null() {
                return Err(invalid("output is null"));
            }
            match navmesh(c)?.sample_position(to_c(point), max_distance) {
                Some(p) => {
                    out_point.write(to_c(p));
                    out(out_found, true)
                }
                None => out(out_found, false),
            }
        })
    })
}

/// A path from `from` to `to` along the navmesh: its corners, start and end
/// included. Writes up to `capacity` corners to `out_corners` and the full
/// count to `*out_len` (call again with more room if it's larger).
/// `*out_len` is 0 when there is no path.
/// ```
/// let p = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_navmesh_find_path(std::ptr::null_mut(), p, p, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_corners` writable for `capacity` points (or
/// null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_navmesh_find_path(
    ctx: *mut PonkoCtx,
    from: PonkoVec3,
    to: PonkoVec3,
    out_corners: *mut PonkoVec3,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_corners.is_null() && capacity > 0 {
                return Err(invalid("output is null"));
            }
            let path = navmesh(c)?.find_path(to_c(from), to_c(to)).unwrap_or_default();
            for (i, p) in path.iter().take(capacity).enumerate() {
                out_corners.add(i).write(to_c(*p));
            }
            out(out_len, path.len())
        })
    })
}

/// Makes `entity` (with a 3D transform) a navmesh agent walking at `speed`
/// (m/s), speeding up by `acceleration`, `radius` wide (at most the bake's
/// agent radius). Keeps its destination if it already was one. Needs
/// `ponko_game_navmesh3d` to move.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_nav_agent(std::ptr::null_mut(), 0, 3.0, 0.3, 10.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_nav_agent(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    speed: f32,
    radius: f32,
    acceleration: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            if !(speed > 0.0 && speed.is_finite() && acceleration > 0.0 && acceleration.is_finite() && radius >= 0.0) {
                return Err(invalid(
                    "agent speed and acceleration must be positive, radius not negative",
                ));
            }
            if let Some(a) = c.world.get_mut::<NavAgent3D>(e) {
                (a.speed, a.radius, a.acceleration) = (speed, radius, acceleration);
            } else {
                let mut a = NavAgent3D::default();
                (a.speed, a.radius, a.acceleration) = (speed, radius, acceleration);
                c.world.insert(e, a);
            }
            Ok(())
        })
    })
}

/// Sends `entity`'s agent to `destination`, or stops it when that is null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_nav_destination(std::ptr::null_mut(), 0, std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `destination` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_nav_destination(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    destination: *const PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let a = c
                .world
                .get_mut::<NavAgent3D>(e)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity is not a nav agent"))?;
            a.destination = destination.as_ref().map(|d| to_c(*d));
            Ok(())
        })
    })
}

/// Makes `entity` an obstacle: a box (`center`, `half_extents`, world space)
/// that agents path around; the navmesh polygons it touches are blocked.
/// Setting it again moves it.
/// ```
/// let v = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_nav_obstacle(std::ptr::null_mut(), 0, v, v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_nav_obstacle(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    center: PonkoVec3,
    half_extents: PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            c.world.insert(e, NavObstacle3D::new(to_c(center), to_c(half_extents)));
            Ok(())
        })
    })
}

/// Sets an axis-aligned carving obstacle. Tile updates run during simulation or
/// through `ponko_ctx_navmesh_update_carving`.
/// ```
/// let v = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_nav_carve_obstacle(std::ptr::null_mut(), 0, v, v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_nav_carve_obstacle(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    center: PonkoVec3,
    half_extents: PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let mut obstacle = NavObstacle3D::new(to_c(center), to_c(half_extents));
            obstacle.carve = true;
            c.world.insert(e, obstacle);
            Ok(())
        })
    })
}

/// Runs at most `budget` tile-cache updates, reporting steps and pending work.
/// Call after a simulation tick has gathered obstacle components.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_navmesh_update_carving(std::ptr::null_mut(), 1, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_navmesh_update_carving(
    ctx: *mut PonkoCtx,
    budget: usize,
    steps: *mut usize,
    pending: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let nav = c
                .world
                .get_resource_mut::<NavMesh3D>()
                .ok_or_else(|| invalid("no navmesh"))?;
            let n = nav.update_carving(budget).map_err(|e| invalid_owned(e.to_string()))?;
            out(steps, n)?;
            out(pending, nav.carving_pending())?;
            Ok(())
        })
    })
}

/// Takes `entity`'s agent and obstacle off (whichever it has).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_remove_nav(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_nav(ctx: *mut PonkoCtx, entity_bits: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            c.world.remove::<NavAgent3D>(e);
            c.world.remove::<NavObstacle3D>(e);
            Ok(())
        })
    })
}

/// Moves nav agents every simulation tick (adds Rust's `step_navmesh3d` as
/// a sim system at this point in the order, with its access declaration, so
/// `ponko_game_access_report` lists what the step touches).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_navmesh3d(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `game` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_navmesh3d(game: *mut PonkoGame) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| {
            ok(g.add(
                ponko::prelude::Phase::AfterPhysics,
                step_navmesh3d.access(step_navmesh3d_access()),
            ))
        })
    }
}

/// Enables crowd avoidance and sets nonnegative separation weight and positive
/// neighbour distance. Disabled agents use the original follower and are ignored
/// by the crowd. The collision time horizon is Detour's default 2.5 seconds.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_nav_avoidance(std::ptr::null_mut(), 0, true, 2.0, 3.6) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_nav_avoidance(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    enabled: bool,
    weight: f32,
    neighbour_distance: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            if !weight.is_finite() || weight < 0.0 || !neighbour_distance.is_finite() || neighbour_distance <= 0.0 {
                return Err(invalid(
                    "avoidance weight must be nonnegative and neighbour distance positive",
                ));
            }
            let a = c
                .world
                .get_mut::<NavAgent3D>(e)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity is not a nav agent"))?;
            a.avoidance = enabled;
            a.avoidance_weight = weight;
            a.neighbour_distance = neighbour_distance;
            Ok(())
        })
    })
}
