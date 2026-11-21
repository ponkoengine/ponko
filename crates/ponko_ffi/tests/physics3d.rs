//! 3D physics from C: body, collider, velocity and character components,
//! collision meshes, the game setting, queries and impulses.
#![cfg(feature = "physics3d")]
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoBodyKind::*;
use PonkoStatus::*;

#[derive(Default)]
struct Ids {
    transform: u32,
    body: u32,
    collider: u32,
    velocity: u32,
    character: u32,
    ball: u64,
    ground: u64,
    walker: u64,
    hit: Option<PonkoHit3D>,
    all_hits: usize,
    overlapping: Vec<u64>,
    mass: f32,
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
fn bits(e: PonkoEntity) -> u64 {
    ponko_entity_bits(e)
}

unsafe fn ids_of(w: *mut PonkoWorld, ids: &mut Ids) {
    unsafe {
        assert_eq!(ponko_transform3d_component(w, &mut ids.transform), PONKO_OK);
        assert_eq!(ponko_rigid_body3d_component(w, &mut ids.body), PONKO_OK);
        assert_eq!(ponko_collider3d_component(w, &mut ids.collider), PONKO_OK);
        assert_eq!(ponko_velocity3d_component(w, &mut ids.velocity), PONKO_OK);
        assert_eq!(ponko_character3d_component(w, &mut ids.character), PONKO_OK);
    }
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let ids = &mut *user.cast::<Ids>();
        let w = ponko_ctx_world(ctx);
        ids_of(w, ids);
        // Flat 3 x 3 ground, 20 m across, its corner at (-10, 0, -10).
        let heights = [0.0f32; 9];
        let mut ground = PonkoCollisionMesh::default();
        assert_eq!(
            ponko_ctx_physics3d_heightfield(ctx, heights.as_ptr(), 9, 20.0, &mut ground),
            PONKO_OK
        );
        let (fixed, dynamic) = (
            ponko_rigid_body3d(PONKO_BODY_FIXED as u32),
            ponko_rigid_body3d(PONKO_BODY_DYNAMIC as u32),
        );
        let (ground_at, ground_shape) = (
            ponko_transform3d_at(v3(-10.0, 0.0, -10.0)),
            ponko_collider3d_mesh(ground),
        );
        let values = [
            value(ids.transform, &ground_at),
            value(ids.body, &fixed),
            value(ids.collider, &ground_shape),
        ];
        assert_eq!(ponko_spawn(w, values.as_ptr(), 3, &mut ids.ground), PONKO_OK);

        let (ball_at, ball_shape, still) = (
            ponko_transform3d_at(v3(0.0, 3.0, 0.0)),
            ponko_collider3d_sphere(0.5),
            PonkoVelocity3D::default(),
        );
        let values = [
            value(ids.transform, &ball_at),
            value(ids.body, &dynamic),
            value(ids.collider, &ball_shape),
            value(ids.velocity, &still),
        ];
        assert_eq!(ponko_spawn(w, values.as_ptr(), 4, &mut ids.ball), PONKO_OK);

        let mut walker = ponko_character3d(1.8, 0.3);
        walker.velocity = v3(1.0, 0.0, 0.0);
        let walker_at = ponko_transform3d_at(v3(-5.0, 0.5, 0.0));
        let values = [value(ids.transform, &walker_at), value(ids.character, &walker)];
        assert_eq!(ponko_spawn(w, values.as_ptr(), 2, &mut ids.walker), PONKO_OK);
    }
}

unsafe extern "C" fn ask(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let ids = &mut *user.cast::<Ids>();
        if ponko_ctx_sim_tick(ctx) != 1 {
            return;
        }
        // Bodies exist after the first step.
        let (mut found, mut hit) = (false, PonkoHit3D::default());
        let mut filter = ponko_query_filter3d();
        filter.exclude = true;
        filter.exclude_entity = ids.ball;
        assert_eq!(
            ponko_ctx_cast_ray3d(
                ctx,
                v3(0.0, 10.0, 0.0),
                v3(0.0, -1.0, 0.0),
                100.0,
                filter,
                &mut found,
                &mut hit
            ),
            PONKO_OK
        );
        ids.hit = found.then_some(hit);
        let mut buf = [PonkoHit3D::default(); 1];
        assert_eq!(
            ponko_ctx_cast_ray3d_all(
                ctx,
                v3(0.0, 10.0, 0.0),
                v3(0.0, -1.0, 0.0),
                100.0,
                ponko_query_filter3d(),
                buf.as_mut_ptr(),
                1,
                &mut ids.all_hits
            ),
            PONKO_OK
        );
        let mut found_s = false;
        assert_eq!(
            ponko_ctx_cast_sphere3d(
                ctx,
                v3(0.0, 10.0, 0.0),
                0.25,
                v3(0.0, -1.0, 0.0),
                100.0,
                filter,
                &mut found_s,
                &mut hit
            ),
            PONKO_OK
        );
        assert!(found_s);
        let mut near = [PonkoEntity::default(); 4];
        let mut n = 0;
        assert_eq!(
            ponko_ctx_overlap_sphere3d(
                ctx,
                v3(0.0, 0.0, 0.0),
                0.5,
                ponko_query_filter3d(),
                near.as_mut_ptr(),
                4,
                &mut n
            ),
            PONKO_OK
        );
        ids.overlapping = near[..n.min(4)].iter().map(|e| bits(*e)).collect();
        assert_eq!(ponko_ctx_body_mass3d(ctx, ids.ball, &mut ids.mass), PONKO_OK);
        let mut none = 0.0;
        assert_eq!(ponko_ctx_body_mass3d(ctx, 12345, &mut none), PONKO_NOT_FOUND);
        assert_eq!(
            ponko_ctx_apply_impulse3d(ctx, 12345, v3(1.0, 0.0, 0.0)),
            PONKO_NOT_FOUND
        );
        assert_eq!(ponko_ctx_apply_impulse3d(ctx, ids.ball, v3(0.0, 0.0, 0.0)), PONKO_OK);
    }
}

#[test]
fn c_bodies_land_on_a_heightfield_characters_walk_and_queries_answer() {
    let mut ids = Ids::default();
    let user = (&raw mut ids).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"physics3d".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(ponko_game_sim_system(g, c"ask".as_ptr(), 1, Some(ask), user), PONKO_OK);
        assert_eq!(ponko_game_run_headless(g, 150, Some(setup), user, &mut world), PONKO_OK);

        let mut t = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let size = size_of::<PonkoTransform3D>();
        assert_eq!(
            ponko_get(world, ids.ball, ids.transform, (&raw mut t).cast(), size),
            PONKO_OK
        );
        assert!(
            (t.position.y - 0.5).abs() < 0.05,
            "rests on the ground: {}",
            t.position.y
        );

        let mut walker = ponko_character3d(1.0, 1.0);
        let size = size_of::<PonkoCharacter3D>();
        assert_eq!(
            ponko_get(world, ids.walker, ids.character, (&raw mut walker).cast(), size),
            PONKO_OK
        );
        assert!(walker.grounded, "stands on the ground");
        assert_eq!(
            ponko_get(
                world,
                ids.walker,
                ids.transform,
                (&raw mut t).cast(),
                size_of::<PonkoTransform3D>()
            ),
            PONKO_OK
        );
        assert!(t.position.x > -2.5, "walked 3 s at 1 m/s: {}", t.position.x);
        assert!(t.position.y.abs() < 0.05, "feet on the ground: {}", t.position.y);

        let hit = ids.hit.expect("the ray hits the ground through the ball");
        assert_eq!(bits(hit.entity), ids.ground);
        assert!((hit.distance - 10.0).abs() < 1e-3, "{}", hit.distance);
        assert!((hit.normal.y - 1.0).abs() < 1e-3);
        assert_eq!(ids.all_hits, 2, "the ball and the ground; one written");
        assert_eq!(ids.overlapping, [ids.ground]);
        let ball_mass = 1000.0 * 4.0 / 3.0 * std::f32::consts::PI * 0.125;
        assert!((ids.mass - ball_mass).abs() < 1.0, "{}", ids.mass);
        ponko_world_free(world);
    }
}

#[test]
fn components_round_trip_and_are_checked() {
    let mut ids = Ids::default();
    unsafe {
        let w = ponko_world_new();
        ids_of(w, &mut ids);
        let mut body = ponko_rigid_body3d(PONKO_BODY_KINEMATIC as u32);
        body.linear_damping = 0.5;
        body.ccd = true;
        let mut capsule = ponko_collider3d_capsule(1.2, 0.3);
        capsule.offset = v3(0.0, 0.9, 0.0);
        capsule.restitution = 0.25;
        capsule.sensor = true;
        capsule.groups = PonkoCollisionGroups {
            memberships: 2,
            filter: 5,
        };
        let mut walker = ponko_character3d(1.7, 0.25);
        walker.step_height = 0.2;
        walker.velocity = v3(1.0, 2.0, 3.0);
        let values = [
            value(ids.body, &body),
            value(ids.collider, &capsule),
            value(ids.character, &walker),
        ];
        let mut e = 0;
        assert_eq!(ponko_spawn(w, values.as_ptr(), 3, &mut e), PONKO_OK);

        let mut back = ponko_rigid_body3d(0);
        let n = size_of::<PonkoRigidBody3D>();
        assert_eq!(ponko_get(w, e, ids.body, (&raw mut back).cast(), n), PONKO_OK);
        assert_eq!(back, body);
        let mut shape = ponko_collider3d_sphere(1.0);
        let n = size_of::<PonkoCollider3D>();
        assert_eq!(ponko_get(w, e, ids.collider, (&raw mut shape).cast(), n), PONKO_OK);
        assert_eq!(shape, capsule);
        let mut ch = ponko_character3d(1.0, 1.0);
        let n = size_of::<PonkoCharacter3D>();
        assert_eq!(ponko_get(w, e, ids.character, (&raw mut ch).cast(), n), PONKO_OK);
        assert_eq!(ch, walker);

        let bad = PonkoRigidBody3D { kind: 7, ..body };
        let n = size_of::<PonkoRigidBody3D>();
        assert_eq!(
            ponko_set(w, e, ids.body, (&raw const bad).cast(), n),
            PONKO_INVALID_ARGUMENT
        );
        let flat = ponko_collider3d_box(v3(1.0, 0.0, 1.0));
        let n = size_of::<PonkoCollider3D>();
        assert_eq!(
            ponko_set(w, e, ids.collider, (&raw const flat).cast(), n),
            PONKO_INVALID_ARGUMENT
        );
        let tiny = ponko_character3d(0.0, 0.3);
        let n = size_of::<PonkoCharacter3D>();
        assert_eq!(
            ponko_set(w, e, ids.character, (&raw const tiny).cast(), n),
            PONKO_INVALID_ARGUMENT
        );

        // Bodies, colliders and characters need conversion, so they can't be
        // query columns; velocity can.
        unsafe extern "C" fn nothing(_: *const PonkoChunk, _: *mut c_void) {}
        for id in [ids.body, ids.collider, ids.character] {
            let cols = [id];
            let s = ponko_query(w, cols.as_ptr(), 1, Some(nothing), null_mut());
            assert_eq!(s, PONKO_INVALID_ARGUMENT);
        }
        let cols = [ids.velocity];
        assert_eq!(ponko_query(w, cols.as_ptr(), 1, Some(nothing), null_mut()), PONKO_OK);
        ponko_world_free(w);
    }
}

unsafe extern "C" fn meshes(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let mut m = PonkoCollisionMesh::default();
        // Not square, null, or too small.
        let h = [0.0f32; 6];
        assert_eq!(
            ponko_ctx_physics3d_heightfield(ctx, h.as_ptr(), 6, 1.0, &mut m),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_physics3d_heightfield(ctx, null(), 4, 1.0, &mut m),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_physics3d_heightfield(ctx, h.as_ptr(), 1, 1.0, &mut m),
            PONKO_INVALID_ARGUMENT
        );
        let cube: Vec<PonkoVec3> = (0..8)
            .map(|i| v3((i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32))
            .collect();
        assert_eq!(ponko_ctx_physics3d_convex_hull(ctx, cube.as_ptr(), 8, &mut m), PONKO_OK);
        assert_eq!(ponko_ctx_physics3d_free_mesh(ctx, m), PONKO_OK);
        let flat = [v3(0.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)];
        assert_eq!(
            ponko_ctx_physics3d_convex_hull(ctx, flat.as_ptr(), 3, &mut m),
            PONKO_INVALID_ARGUMENT
        );
        let tri = [0u32, 1, 2];
        assert_eq!(
            ponko_ctx_physics3d_trimesh(ctx, flat.as_ptr(), 3, tri.as_ptr(), 3, &mut m),
            PONKO_OK
        );
        let out_of_range = [0u32, 1, 3];
        assert_eq!(
            ponko_ctx_physics3d_trimesh(ctx, flat.as_ptr(), 3, out_of_range.as_ptr(), 3, &mut m),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_physics3d_trimesh(ctx, flat.as_ptr(), 3, tri.as_ptr(), 2, &mut m),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_set_gravity3d(ctx, v3(0.0, f32::NAN, 0.0)),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_set_gravity3d(ctx, v3(0.0, -1.0, 0.0)), PONKO_OK);
    }
}

#[test]
fn a_scaled_mesh_collider_round_trips_with_its_scale() {
    let mut ids = Ids::default();
    unsafe {
        let w = ponko_world_new();
        ids_of(w, &mut ids);
        let scaled = ponko_collider3d_scaled_mesh(PonkoCollisionMesh { id: 7 }, v3(-1.0, 2.0, 0.5));
        assert_eq!(scaled.shape, PonkoShape3DKind::PONKO_SHAPE3D_SCALED_MESH as u32);
        assert_eq!(
            (scaled.density, scaled.friction),
            (1000.0, 0.5),
            "the defaults of the other shapes"
        );
        let values = [value(ids.collider, &scaled)];
        let mut e = 0;
        assert_eq!(ponko_spawn(w, values.as_ptr(), 1, &mut e), PONKO_OK);
        let mut back = ponko_collider3d_sphere(1.0);
        let n = size_of::<PonkoCollider3D>();
        assert_eq!(ponko_get(w, e, ids.collider, (&raw mut back).cast(), n), PONKO_OK);
        assert_eq!(back, scaled, "the scale comes back, mirrored component and all");
        let bad = PonkoCollider3D {
            size: v3(f32::NAN, 1.0, 1.0),
            ..scaled
        };
        assert_eq!(
            ponko_set(w, e, ids.collider, (&raw const bad).cast(), n),
            PONKO_INVALID_ARGUMENT
        );
        ponko_world_free(w);
    }
}

#[test]
fn collision_meshes_are_checked() {
    unsafe {
        let g = ponko_game_new(c"meshes".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(meshes), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}

unsafe extern "C" fn mouse(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let mut motion = PonkoVec2 { x: 1.0, y: 1.0 };
        assert_eq!(ponko_ctx_mouse_motion(ctx, &mut motion), PONKO_OK);
        assert_eq!((motion.x, motion.y), (0.0, 0.0), "no mouse in a headless run");
        let mut locked = true;
        assert_eq!(ponko_ctx_cursor_locked(ctx, &mut locked), PONKO_OK);
        assert!(!locked);
        assert_eq!(ponko_ctx_set_cursor_locked(ctx, true), PONKO_OK);
        assert_eq!(ponko_ctx_cursor_locked(ctx, &mut locked), PONKO_OK);
        assert!(locked);
    }
}

#[test]
fn mouse_motion_and_cursor_lock_work_headless() {
    unsafe {
        let g = ponko_game_new(c"mouse".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(mouse), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}
