//! 3D physics for big worlds from C: collision mesh data built on another
//! thread, terrain colliders, and static colliders kept near a focus.
#![cfg(feature = "physics3d")]
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

#[derive(Default)]
struct Ids {
    transform: u32,
    body: u32,
    collider: u32,
    terrain: u32,
    focus: u32,
    ground: u64,
    far_box: u64,
    ball: u64,
    focus_entity: u64,
    /// After the first step: ground in, far box out.
    first: Option<(bool, bool)>,
    /// After the focus walked to the far box: box in.
    later: Option<bool>,
}

fn value<T>(component: u32, v: &T) -> PonkoValue {
    PonkoValue {
        component,
        data: (v as *const T).cast(),
        size: size_of::<T>(),
    }
}
fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let ids = &mut *user.cast::<Ids>();
        let w = ponko_ctx_world(ctx);
        assert_eq!(ponko_transform3d_component(w, &mut ids.transform), PONKO_OK);
        assert_eq!(ponko_rigid_body3d_component(w, &mut ids.body), PONKO_OK);
        assert_eq!(ponko_collider3d_component(w, &mut ids.collider), PONKO_OK);
        assert_eq!(ponko_terrain_collider_component(w, &mut ids.terrain), PONKO_OK);
        assert_eq!(ponko_collider_focus_component(w, &mut ids.focus), PONKO_OK);
        assert_eq!(ponko_ctx_physics3d_stream_colliders(ctx, true, 8), PONKO_OK);
        // Ground 2 m up, built on another thread, 20 m across from (-10, 0, -10).
        let data = std::thread::spawn(|| {
            let heights = [2.0f32; 9];
            ponko_collision_mesh_data_heightfield(heights.as_ptr(), 9, 20.0) as usize
        })
        .join()
        .unwrap() as *mut PonkoCollisionMeshData;
        assert!(!data.is_null());
        let mut mesh = PonkoCollisionMesh::default();
        assert_eq!(ponko_ctx_physics3d_add_mesh(ctx, data, &mut mesh), PONKO_OK);
        ponko_collision_mesh_data_free(data);
        let (at, solid) = (
            ponko_transform3d_at(v3(-10.0, 0.0, -10.0)),
            ponko_terrain_collider(mesh),
        );
        let values = [value(ids.transform, &at), value(ids.terrain, &solid)];
        assert_eq!(ponko_spawn(w, values.as_ptr(), 2, &mut ids.ground), PONKO_OK);
        let (at, body, shape) = (
            ponko_transform3d_at(v3(0.0, 4.0, 0.0)),
            ponko_rigid_body3d(PonkoBodyKind::PONKO_BODY_DYNAMIC as u32),
            ponko_collider3d_sphere(0.5),
        );
        let values = [
            value(ids.transform, &at),
            value(ids.body, &body),
            value(ids.collider, &shape),
        ];
        assert_eq!(ponko_spawn(w, values.as_ptr(), 3, &mut ids.ball), PONKO_OK);
        // A wall 300 m away, beyond the focus's reach.
        let (at, body, shape) = (
            ponko_transform3d_at(v3(300.0, 0.0, 0.0)),
            ponko_rigid_body3d(PonkoBodyKind::PONKO_BODY_FIXED as u32),
            ponko_collider3d_box(v3(1.0, 1.0, 1.0)),
        );
        let values = [
            value(ids.transform, &at),
            value(ids.body, &body),
            value(ids.collider, &shape),
        ];
        assert_eq!(ponko_spawn(w, values.as_ptr(), 3, &mut ids.far_box), PONKO_OK);
        let (at, focus) = (
            ponko_transform3d_at(v3(0.0, 0.0, 0.0)),
            ponko_collider_focus(50.0, 80.0),
        );
        let values = [value(ids.transform, &at), value(ids.focus, &focus)];
        assert_eq!(ponko_spawn(w, values.as_ptr(), 2, &mut ids.focus_entity), PONKO_OK);
    }
}

unsafe extern "C" fn watch(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let ids = &mut *user.cast::<Ids>();
        let active = |e: u64| {
            let mut a = false;
            assert_eq!(ponko_ctx_physics3d_is_collider_active(ctx, e, &mut a), PONKO_OK);
            a
        };
        match ponko_ctx_sim_tick(ctx) {
            0 => {
                ids.first = Some((active(ids.ground), active(ids.far_box)));
                // The focus walks to the wall.
                let w = ponko_ctx_world(ctx);
                let at = ponko_transform3d_at(v3(290.0, 0.0, 0.0));
                let size = size_of::<PonkoTransform3D>();
                assert_eq!(
                    ponko_set(w, ids.focus_entity, ids.transform, (&raw const at).cast(), size),
                    PONKO_OK
                );
            }
            1 => ids.later = Some(active(ids.far_box)),
            _ => {}
        }
    }
}

#[test]
fn c_terrain_colliders_hold_a_ball_and_static_colliders_follow_the_focus() {
    let mut ids = Ids::default();
    let user = (&raw mut ids).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"stream3d".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(g, c"watch".as_ptr(), 1, Some(watch), user),
            PONKO_OK
        );
        assert_eq!(ponko_game_run_headless(g, 2, Some(setup), user, &mut world), PONKO_OK);
        assert_eq!(ids.first, Some((true, false)), "the ground in, the far wall out");
        assert_eq!(ids.later, Some(true), "the wall came in as the focus reached it");
        // The components read back as written.
        let mut terrain = ponko_terrain_collider(PonkoCollisionMesh::default());
        let size = size_of::<PonkoTerrainCollider>();
        assert_eq!(
            ponko_get(world, ids.ground, ids.terrain, (&raw mut terrain).cast(), size),
            PONKO_OK
        );
        assert_eq!(terrain.friction, 0.5);
        let mut focus = PonkoColliderFocus::default();
        let size = size_of::<PonkoColliderFocus>();
        assert_eq!(
            ponko_get(world, ids.focus_entity, ids.focus, (&raw mut focus).cast(), size),
            PONKO_OK
        );
        assert_eq!(focus, ponko_collider_focus(50.0, 80.0));
        // Bad values are refused.
        terrain.friction = -1.0;
        let size = size_of::<PonkoTerrainCollider>();
        assert_eq!(
            ponko_set(world, ids.ground, ids.terrain, (&raw const terrain).cast(), size),
            PONKO_INVALID_ARGUMENT
        );
        ponko_world_free(world);
    }
}

#[test]
fn ball_lands_on_terrain_built_from_c() {
    // The landing itself, longer: the ball rests on the 2 m ground.
    let mut ids = Ids::default();
    let user = (&raw mut ids).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"land".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(ponko_game_run_headless(g, 120, Some(setup), user, &mut world), PONKO_OK);
        let mut t = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let size = size_of::<PonkoTransform3D>();
        assert_eq!(
            ponko_get(world, ids.ball, ids.transform, (&raw mut t).cast(), size),
            PONKO_OK
        );
        assert!(
            (t.position.y - 2.5).abs() < 0.05,
            "rests on the ground: {}",
            t.position.y
        );
        ponko_world_free(world);
    }
}

#[test]
fn collision_mesh_data_is_checked() {
    unsafe {
        let h = [0.0f32; 9];
        assert!(ponko_collision_mesh_data_heightfield(h.as_ptr(), 9, -1.0).is_null());
        assert!(ponko_collision_mesh_data_heightfield(null(), 9, 1.0).is_null());
        let v = [v3(0.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)];
        assert!(ponko_collision_mesh_data_trimesh(v.as_ptr(), 3, [0u32, 1, 7].as_ptr(), 3).is_null());
        assert!(ponko_collision_mesh_data_trimesh(v.as_ptr(), 3, [0u32, 1].as_ptr(), 2).is_null());
        let p = [v[0], v[1], v[2], v3(0.0, 1.0, 0.0)];
        let hull = ponko_collision_mesh_data_convex_hull(p.as_ptr(), 4);
        assert!(!hull.is_null());
        ponko_collision_mesh_data_free(hull);
        let mut mesh = PonkoCollisionMesh::default();
        assert_eq!(
            ponko_ctx_physics3d_add_mesh(null_mut(), null(), &mut mesh),
            PONKO_INVALID_ARGUMENT
        );
    }
}

/// What the terrain rebuild tests share between setup, the rebuild and a
/// system: component ids, what the rebuild saw, what the rays found.
#[derive(Default)]
struct Rebuilt {
    transform: u32,
    terrain: u32,
    focus: u32,
    /// The tile corners the rebuild read.
    corners: Vec<PonkoVec3>,
    /// The terrain colliders after the load.
    surfaces: Vec<PonkoTerrainCollider>,
    /// Whether a ray down onto the near tile, then the far one, hit ground.
    hits: Option<(bool, bool)>,
    /// Clear the rebuild again before loading.
    clear: bool,
}

fn tiles_dir(name: &str) -> std::ffi::CString {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-terrain-rebuild-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::ffi::CString::new(dir.to_str().unwrap()).unwrap()
}

/// Two tiles 2 m high, saved: one near the origin (friction 0.2), one far.
unsafe extern "C" fn save_tiles(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let w = ponko_ctx_world(ctx);
        let (mut transform, mut terrain) = (0, 0);
        assert_eq!(ponko_transform3d_component(w, &mut transform), PONKO_OK);
        assert_eq!(ponko_terrain_collider_component(w, &mut terrain), PONKO_OK);
        let heights = [2.0f32; 9];
        let data = ponko_collision_mesh_data_heightfield(heights.as_ptr(), 9, 20.0);
        let mut mesh = PonkoCollisionMesh::default();
        assert_eq!(ponko_ctx_physics3d_add_mesh(ctx, data, &mut mesh), PONKO_OK);
        ponko_collision_mesh_data_free(data);
        for (corner, friction) in [(v3(-10.0, 0.0, -10.0), 0.2), (v3(100.0, 0.0, 100.0), 0.5)] {
            let at = ponko_transform3d_at(corner);
            let solid = PonkoTerrainCollider {
                friction,
                ..ponko_terrain_collider(mesh)
            };
            let values = [value(transform, &at), value(terrain, &solid)];
            let mut e = 0;
            assert_eq!(ponko_spawn(w, values.as_ptr(), 2, &mut e), PONKO_OK);
        }
        assert_eq!(ponko_ctx_save_world(ctx, c"tiles".as_ptr()), PONKO_OK);
    }
}

/// The game's rebuild: the near tile's ground from its saved corner, none
/// for the far one.
unsafe extern "C" fn rebuild(
    world: *const PonkoLoadedWorld,
    tile: u64,
    user: *mut c_void,
) -> *mut PonkoCollisionMeshData {
    unsafe {
        let r = &mut *user.cast::<Rebuilt>();
        let mut at = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let size = size_of::<PonkoTransform3D>();
        assert_eq!(
            ponko_loaded_world_get(world, tile, r.transform, (&raw mut at).cast(), size),
            PONKO_OK
        );
        // Checked like ponko_get: the size, a component the tile lacks.
        assert_eq!(
            ponko_loaded_world_get(world, tile, r.transform, (&raw mut at).cast(), size - 1),
            PONKO_INVALID_ARGUMENT
        );
        let mut focus = PonkoColliderFocus::default();
        let fsize = size_of::<PonkoColliderFocus>();
        assert_eq!(
            ponko_loaded_world_get(world, tile, r.focus, (&raw mut focus).cast(), fsize),
            PONKO_NOT_FOUND
        );
        r.corners.push(at.position);
        if at.position.x > 50.0 {
            return null_mut();
        }
        let heights = [2.0f32; 9];
        ponko_collision_mesh_data_heightfield(heights.as_ptr(), 9, 20.0)
    }
}

unsafe extern "C" fn load_tiles(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let r = &mut *user.cast::<Rebuilt>();
        let w = ponko_ctx_world(ctx);
        assert_eq!(ponko_transform3d_component(w, &mut r.transform), PONKO_OK);
        assert_eq!(ponko_terrain_collider_component(w, &mut r.terrain), PONKO_OK);
        assert_eq!(ponko_collider_focus_component(w, &mut r.focus), PONKO_OK);
        assert_eq!(
            ponko_ctx_physics3d_set_terrain_rebuild(ctx, None, user),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_physics3d_set_terrain_rebuild(ctx, Some(rebuild), user),
            PONKO_OK
        );
        if r.clear {
            assert_eq!(ponko_ctx_physics3d_clear_terrain_rebuild(ctx), PONKO_OK);
        }
        assert_eq!(ponko_ctx_load_world(ctx, c"tiles".as_ptr()), PONKO_OK);
    }
}

unsafe extern "C" fn probe_tiles(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let r = &mut *user.cast::<Rebuilt>();
        if ponko_ctx_sim_tick(ctx) != 1 {
            return;
        }
        let ground = |x: f32, z: f32| {
            let (mut found, mut hit) = (false, std::mem::zeroed::<PonkoHit3D>());
            let status = ponko_ctx_cast_ray3d(
                ctx,
                v3(x, 10.0, z),
                v3(0.0, -1.0, 0.0),
                20.0,
                ponko_query_filter3d(),
                &mut found,
                &mut hit,
            );
            assert_eq!(status, PONKO_OK);
            found && (hit.point.y - 2.0).abs() < 0.01
        };
        r.hits = Some((ground(0.0, 0.0), ground(110.0, 110.0)));
        // The colliders kept their saved surface.
        let w = ponko_ctx_world(ctx);
        let mut tiles: Vec<u64> = Vec::new();
        let columns = [r.transform];
        let list = (&raw mut tiles).cast::<c_void>();
        assert_eq!(ponko_query(w, columns.as_ptr(), 1, Some(entities), list), PONKO_OK);
        for e in tiles {
            let mut t = ponko_terrain_collider(PonkoCollisionMesh::default());
            let size = size_of::<PonkoTerrainCollider>();
            assert_eq!(ponko_get(w, e, r.terrain, (&raw mut t).cast(), size), PONKO_OK);
            r.surfaces.push(t);
        }
    }
}

/// Every entity of the query, into the `Vec<u64>` behind `user`.
unsafe extern "C" fn entities(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let chunk = &*chunk;
        let list = &mut *user.cast::<Vec<u64>>();
        list.extend(
            std::slice::from_raw_parts(chunk.entities, chunk.count)
                .iter()
                .map(|&e| ponko_entity_bits(e)),
        );
    }
}

fn save_then_load(name: &str, clear: bool) -> Rebuilt {
    let dir = tiles_dir(name);
    let mut r = Rebuilt {
        clear,
        ..Rebuilt::default()
    };
    let user = (&raw mut r).cast::<c_void>();
    unsafe {
        let g = ponko_game_new(c"tiles".as_ptr(), 1, 1);
        assert_eq!(ponko_game_data_dir(g, dir.as_ptr()), PONKO_OK);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, 0.0, 0.0)), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(save_tiles), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
        let g = ponko_game_new(c"tiles".as_ptr(), 1, 1);
        assert_eq!(ponko_game_data_dir(g, dir.as_ptr()), PONKO_OK);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, 0.0, 0.0)), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(g, c"probe".as_ptr(), 1, Some(probe_tiles), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_run_headless(g, 3, Some(load_tiles), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    let _ = std::fs::remove_dir_all(dir.to_str().unwrap());
    r
}

#[test]
fn a_c_terrain_rebuild_makes_loaded_tiles_solid_where_they_were() {
    let r = save_then_load("rebuild", false);
    let mut corners: Vec<f32> = r.corners.iter().map(|c| c.x).collect();
    corners.sort_by(f32::total_cmp);
    assert_eq!(
        corners,
        [-10.0, 100.0],
        "called once per tile, with its saved transform"
    );
    assert_eq!(
        r.hits,
        Some((true, false)),
        "the near tile is solid again, the far one has no ground"
    );
    let mut frictions: Vec<f32> = r.surfaces.iter().map(|t| t.friction).collect();
    frictions.sort_by(f32::total_cmp);
    assert_eq!(frictions, [0.2, 0.5], "both colliders kept their surface");
}

#[test]
fn a_cleared_c_terrain_rebuild_leaves_loaded_tiles_without_ground() {
    let r = save_then_load("cleared", true);
    assert!(r.corners.is_empty(), "not called");
    assert_eq!(r.hits, Some((false, false)));
}
