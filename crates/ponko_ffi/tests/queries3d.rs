//! The 3D queries 2D also has, from C: oriented box casts, point overlaps,
//! closest points and penetrations, with the Rust rules.
#![cfg(feature = "physics3d")]
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::null_mut;

use PonkoBodyKind::*;
use PonkoStatus::*;

const WALL: u32 = 1 << 1;
const TERRAIN: u32 = 1 << 2;

#[derive(Default)]
struct Scene {
    wall: u64,
    sensor: u64,
    terrain: u64,
    answers: Option<Answers>,
}

#[derive(Debug, Default)]
struct Answers {
    flat: Option<PonkoHit3D>,
    turned: Option<PonkoHit3D>,
    terrain_only: Option<PonkoHit3D>,
    excluded: Option<PonkoHit3D>,
    nan_turn: bool,
    zero_direction: bool,
    at_point: Vec<u64>,
    at_point_sensors: Vec<u64>,
    at_point_total: usize,
    closest: Option<PonkoPointHit3D>,
    closest_inside: Option<PonkoPointHit3D>,
    pushes: Vec<PonkoPenetration3D>,
    mesh_pushes: Vec<PonkoPenetration3D>,
    bad_shape: Option<PonkoStatus>,
    null_output: Option<PonkoStatus>,
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

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let scene = &mut *user.cast::<Scene>();
        let w = ponko_ctx_world(ctx);
        let (mut transform, mut body, mut collider) = (0, 0, 0);
        assert_eq!(ponko_transform3d_component(w, &mut transform), PONKO_OK);
        assert_eq!(ponko_rigid_body3d_component(w, &mut body), PONKO_OK);
        assert_eq!(ponko_collider3d_component(w, &mut collider), PONKO_OK);
        let fixed = ponko_rigid_body3d(PONKO_BODY_FIXED as u32);
        let spawn = |at: PonkoVec3, shape: PonkoCollider3D, out: &mut u64| {
            let at = ponko_transform3d_at(at);
            let parts = [value(transform, &at), value(body, &fixed), value(collider, &shape)];
            assert_eq!(ponko_spawn(w, parts.as_ptr(), 3, out), PONKO_OK);
        };
        // A wall whose -X face is at x = 9, in its own group so a
        // terrain-only query skips it.
        let mut wall = ponko_collider3d_box(v3(2.0, 20.0, 20.0));
        wall.groups = PonkoCollisionGroups {
            memberships: WALL,
            filter: u32::MAX,
        };
        spawn(v3(10.0, 0.0, 0.0), wall, &mut scene.wall);
        let mut trigger = ponko_collider3d_sphere(1.0);
        trigger.sensor = true;
        spawn(v3(0.0, 10.0, 0.0), trigger, &mut scene.sensor);
        // Behind the wall, in the terrain group only.
        let mut post = ponko_collider3d_sphere(1.0);
        post.groups = PonkoCollisionGroups {
            memberships: TERRAIN,
            filter: u32::MAX,
        };
        spawn(v3(30.0, 0.0, 0.0), post, &mut scene.terrain);
    }
}

unsafe fn one(call: impl FnOnce(*mut bool, *mut PonkoHit3D) -> PonkoStatus) -> Option<PonkoHit3D> {
    let (mut found, mut hit) = (false, PonkoHit3D::default());
    assert_eq!(call(&mut found, &mut hit), PONKO_OK);
    found.then_some(hit)
}

unsafe fn near(call: impl FnOnce(*mut bool, *mut PonkoPointHit3D) -> PonkoStatus) -> Option<PonkoPointHit3D> {
    let (mut found, mut hit) = (false, PonkoPointHit3D::default());
    assert_eq!(call(&mut found, &mut hit), PONKO_OK);
    found.then_some(hit)
}

unsafe fn pushes(
    call: impl FnOnce(*mut PonkoPenetration3D, usize, *mut usize) -> PonkoStatus,
) -> Vec<PonkoPenetration3D> {
    let mut buf = [PonkoPenetration3D::default(); 4];
    let mut n = 0;
    assert_eq!(call(buf.as_mut_ptr(), buf.len(), &mut n), PONKO_OK);
    buf[..n].to_vec()
}

unsafe extern "C" fn ask(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let scene = &mut *user.cast::<Scene>();
        if ponko_ctx_sim_tick(ctx) != 1 {
            return;
        }
        let solid = ponko_query_filter3d();
        let q = ponko_quat_identity();
        let origin = v3(0.0, 0.0, 0.0);
        let right = v3(5.0, 0.0, 0.0);
        let cube = v3(2.0, 2.0, 2.0);
        // 45° about Z: an edge leads, √2 from the center.
        let s = std::f32::consts::FRAC_PI_8.sin();
        let turned = PonkoQuat {
            x: 0.0,
            y: 0.0,
            z: s,
            w: std::f32::consts::FRAC_PI_8.cos(),
        };
        let terrain_only = PonkoQueryFilter3D {
            groups: PonkoCollisionGroups {
                memberships: u32::MAX,
                filter: TERRAIN,
            },
            ..solid
        };
        let not_wall = PonkoQueryFilter3D {
            exclude: true,
            exclude_entity: scene.wall,
            ..solid
        };
        let nan_turn = PonkoQuat { x: f32::NAN, ..q };
        let mut a = Answers {
            flat: one(|f, h| ponko_ctx_cast_box3d(ctx, origin, cube, q, right, 100.0, solid, f, h)),
            turned: one(|f, h| ponko_ctx_cast_box3d(ctx, origin, cube, turned, right, 100.0, solid, f, h)),
            terrain_only: one(|f, h| ponko_ctx_cast_box3d(ctx, origin, cube, q, right, 100.0, terrain_only, f, h)),
            excluded: one(|f, h| ponko_ctx_cast_box3d(ctx, origin, cube, q, right, 100.0, not_wall, f, h)),
            nan_turn: one(|f, h| ponko_ctx_cast_box3d(ctx, origin, cube, nan_turn, right, 100.0, solid, f, h))
                .is_some(),
            zero_direction: one(|f, h| ponko_ctx_cast_box3d(ctx, origin, cube, q, origin, 100.0, solid, f, h))
                .is_some(),
            closest: near(|f, h| ponko_ctx_closest_point3d(ctx, origin, 100.0, solid, f, h)),
            closest_inside: near(|f, h| ponko_ctx_closest_point3d(ctx, v3(10.0, 1.0, 0.0), 100.0, solid, f, h)),
            ..Answers::default()
        };
        // A point inside the sensor: only seen with sensors on. A buffer of
        // zero still gets the count.
        let at = v3(0.0, 10.0, 0.0);
        let mut buf = [PonkoEntity::default(); 4];
        let mut n = 0;
        assert_eq!(
            ponko_ctx_overlap_point3d(ctx, at, solid, buf.as_mut_ptr(), 4, &mut n),
            PONKO_OK
        );
        a.at_point = buf[..n].iter().map(|e| bits(*e)).collect();
        let sensors = PonkoQueryFilter3D { sensors: true, ..solid };
        assert_eq!(
            ponko_ctx_overlap_point3d(ctx, at, sensors, null_mut(), 0, &mut a.at_point_total),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_overlap_point3d(ctx, at, sensors, buf.as_mut_ptr(), 4, &mut n),
            PONKO_OK
        );
        a.at_point_sensors = buf[..n].iter().map(|e| bits(*e)).collect();
        // Radius 1 centered 0.6 left of the wall's face: 0.4 in.
        a.pushes = pushes(|o, c, n| {
            ponko_ctx_penetrations3d(ctx, ponko_collider3d_sphere(1.0), v3(8.4, 0.0, 0.0), q, solid, o, c, n)
        });
        // The same with a convex hull: a 2 m cube.
        let corners: Vec<PonkoVec3> = (0..8)
            .map(|i| {
                let c = |bit: i32| if i & bit != 0 { 1.0 } else { -1.0 };
                v3(c(1), c(2), c(4))
            })
            .collect();
        let mut hull = PonkoCollisionMesh::default();
        assert_eq!(
            ponko_ctx_physics3d_convex_hull(ctx, corners.as_ptr(), corners.len(), &mut hull),
            PONKO_OK
        );
        a.mesh_pushes = pushes(|o, c, n| {
            ponko_ctx_penetrations3d(ctx, ponko_collider3d_mesh(hull), v3(8.4, 0.0, 0.0), q, solid, o, c, n)
        });
        let mut bad = ponko_collider3d_sphere(1.0);
        bad.radius = -1.0;
        let mut out = [PonkoPenetration3D::default(); 1];
        a.bad_shape = Some(ponko_ctx_penetrations3d(
            ctx,
            bad,
            origin,
            q,
            solid,
            out.as_mut_ptr(),
            1,
            &mut n,
        ));
        let mut found = false;
        a.null_output = Some(ponko_ctx_cast_box3d(
            ctx,
            origin,
            cube,
            q,
            right,
            100.0,
            solid,
            &mut found,
            null_mut(),
        ));
        scene.answers = Some(a);
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn c_3d_queries_follow_the_rust_rules() {
    let mut scene = Scene::default();
    let user = (&raw mut scene).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"queries".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, 0.0, 0.0)), PONKO_OK);
        assert_eq!(ponko_game_sim_system(g, c"ask".as_ptr(), 1, Some(ask), user), PONKO_OK);
        assert_eq!(ponko_game_run_headless(g, 2, Some(setup), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    let a = scene.answers.expect("asked");

    let flat = a.flat.expect("hits the wall");
    assert_eq!(bits(flat.entity), scene.wall);
    assert!(close(flat.distance, 8.0), "a long direction is normalized: {flat:?}");
    assert!(close(flat.normal.x, -1.0));
    let turned = a.turned.expect("hits the wall");
    assert!(
        (turned.distance - (9.0 - 2f32.sqrt())).abs() < 1e-3,
        "turned 45°: {turned:?}"
    );
    assert_eq!(bits(a.terrain_only.unwrap().entity), scene.terrain);
    assert_eq!(bits(a.excluded.unwrap().entity), scene.terrain);
    assert!(!a.nan_turn && !a.zero_direction, "find nothing");

    assert!(a.at_point.is_empty(), "sensors only with the flag");
    assert_eq!(a.at_point_total, 1);
    assert_eq!(a.at_point_sensors, [scene.sensor]);

    let closest = a.closest.expect("the sensor is skipped: the wall is nearest");
    assert_eq!(bits(closest.entity), scene.wall);
    assert!(close(closest.distance, 9.0) && close(closest.point.x, 9.0) && !closest.inside);
    let inside = a.closest_inside.expect("inside the wall");
    assert!(inside.inside && inside.distance == 0.0 && close(inside.point.y, 1.0));

    for pushes in [&a.pushes, &a.mesh_pushes] {
        assert_eq!(pushes.len(), 1, "{pushes:?}");
        assert_eq!(bits(pushes[0].entity), scene.wall);
        assert!(close(pushes[0].depth, 0.4), "{pushes:?}");
        assert!(close(pushes[0].normal.x, -1.0));
    }
    assert_eq!(a.bad_shape, Some(PONKO_INVALID_ARGUMENT));
    assert_eq!(a.null_output, Some(PONKO_INVALID_ARGUMENT));
}
