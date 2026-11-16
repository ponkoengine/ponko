//! 2D physics queries from C: all-hit rays, shape casts, overlaps, closest
//! points, penetrations and collision groups, with the Rust rules.
#![cfg(feature = "physics")]
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
    groups_read: Option<PonkoCollisionGroups>,
    answers: Option<Answers>,
}

#[derive(Debug, Default)]
struct Answers {
    ray: Option<PonkoRayHit>,
    ray_terrain_only: Option<PonkoRayHit>,
    all: Vec<PonkoRayHit>,
    all_total: usize,
    circle: Option<PonkoRayHit>,
    rect: Option<PonkoRayHit>,
    capsule: Option<PonkoRayHit>,
    excluded: Option<PonkoRayHit>,
    zero_direction: bool,
    at_point: Vec<u64>,
    at_point_sensors: Vec<u64>,
    in_circle: Vec<u64>,
    in_box: Vec<u64>,
    in_capsule: Vec<u64>,
    closest: Option<PonkoPointHit2D>,
    pushes: Vec<PonkoPenetration2D>,
    polygon_push: Option<PonkoStatus>,
}

fn value<T>(component: u32, v: &T) -> PonkoValue {
    PonkoValue {
        component,
        data: (v as *const T).cast(),
        size: size_of::<T>(),
    }
}
fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}
fn bits(e: PonkoEntity) -> u64 {
    ponko_entity_bits(e)
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let scene = &mut *user.cast::<Scene>();
        let w = ponko_ctx_world(ctx);
        let (mut transform, mut body, mut collider) = (0, 0, 0);
        assert_eq!(ponko_transform_component(w, &mut transform), PONKO_OK);
        assert_eq!(ponko_rigid_body_component(w, &mut body), PONKO_OK);
        assert_eq!(ponko_collider_component(w, &mut collider), PONKO_OK);
        let fixed = ponko_rigid_body(PONKO_BODY_FIXED as u32);
        let spawn = |x: f32, y: f32, shape: PonkoCollider, out: &mut u64| {
            let at = PonkoTransform2D {
                position: v2(x, y),
                ..Default::default()
            };
            let parts = [value(transform, &at), value(body, &fixed), value(collider, &shape)];
            assert_eq!(ponko_spawn(w, parts.as_ptr(), 3, out), PONKO_OK);
        };
        // A wall whose left face is at x = 90.
        spawn(100.0, 0.0, ponko_collider_rect(v2(20.0, 200.0)), &mut scene.wall);
        let mut trigger = ponko_collider_circle(10.0);
        trigger.sensor = true;
        spawn(0.0, 100.0, trigger, &mut scene.sensor);
        // Behind the wall, in the terrain group only.
        spawn(300.0, 0.0, ponko_collider_circle(10.0), &mut scene.terrain);
        let terrain = PonkoCollisionGroups {
            memberships: TERRAIN,
            filter: u32::MAX,
        };
        assert_eq!(ponko_set_collider_groups(w, scene.terrain, terrain), PONKO_OK);
        // The wall is in its own group, so a terrain-only query skips it.
        let walls = PonkoCollisionGroups {
            memberships: WALL,
            filter: u32::MAX,
        };
        assert_eq!(ponko_set_collider_groups(w, scene.wall, walls), PONKO_OK);
        let mut read = ponko_collision_groups_all();
        assert_eq!(ponko_collider_groups(w, scene.terrain, &mut read), PONKO_OK);
        scene.groups_read = Some(read);
        assert_eq!(ponko_collider_groups(w, 12345, &mut read), PONKO_NOT_FOUND);
        assert_eq!(
            ponko_set_collider_groups(w, 12345, ponko_collision_groups_all()),
            PONKO_NOT_FOUND
        );
    }
}

unsafe fn entities(call: impl FnOnce(*mut PonkoEntity, usize, *mut usize) -> PonkoStatus) -> Vec<u64> {
    let mut buf = [PonkoEntity::default(); 8];
    let mut n = 0;
    assert_eq!(call(buf.as_mut_ptr(), buf.len(), &mut n), PONKO_OK);
    let mut found: Vec<u64> = buf[..n].iter().map(|e| bits(*e)).collect();
    found.sort_unstable();
    found
}

unsafe fn one(call: impl FnOnce(*mut bool, *mut PonkoRayHit) -> PonkoStatus) -> Option<PonkoRayHit> {
    let (mut found, mut hit) = (false, PonkoRayHit::default());
    assert_eq!(call(&mut found, &mut hit), PONKO_OK);
    found.then_some(hit)
}

unsafe extern "C" fn ask(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let scene = &mut *user.cast::<Scene>();
        if ponko_ctx_sim_tick(ctx) != 1 {
            return;
        }
        let all_groups = ponko_collision_groups_all();
        let solid = PonkoQueryFilter2D::default();
        let right = v2(1.0, 0.0);
        let mut a = Answers {
            ray: one(|f, h| ponko_ctx_cast_ray2d(ctx, v2(0.0, 0.0), v2(5.0, 0.0), 1000.0, solid, all_groups, f, h)),
            ray_terrain_only: one(|f, h| {
                let terrain_only = PonkoCollisionGroups {
                    memberships: u32::MAX,
                    filter: TERRAIN,
                };
                ponko_ctx_cast_ray2d(ctx, v2(0.0, 0.0), right, 1000.0, solid, terrain_only, f, h)
            }),
            circle: one(|f, h| {
                ponko_ctx_cast_circle2d(ctx, v2(0.0, 0.0), 10.0, right, 1000.0, solid, all_groups, f, h)
            }),
            rect: one(|f, h| {
                ponko_ctx_cast_box2d(
                    ctx,
                    v2(0.0, 0.0),
                    v2(20.0, 20.0),
                    0.0,
                    right,
                    1000.0,
                    solid,
                    all_groups,
                    f,
                    h,
                )
            }),
            capsule: one(|f, h| {
                ponko_ctx_cast_capsule2d(ctx, v2(0.0, 0.0), 20.0, 5.0, right, 1000.0, solid, all_groups, f, h)
            }),
            excluded: one(|f, h| {
                let not_wall = PonkoQueryFilter2D {
                    exclude: true,
                    exclude_entity: scene.wall,
                    sensors: false,
                };
                ponko_ctx_cast_ray2d(ctx, v2(0.0, 0.0), right, 1000.0, not_wall, all_groups, f, h)
            }),
            zero_direction: one(|f, h| {
                ponko_ctx_cast_ray2d(ctx, v2(0.0, 0.0), v2(0.0, 0.0), 1000.0, solid, all_groups, f, h)
            })
            .is_some(),
            ..Answers::default()
        };
        // All hits, into a buffer too small at first: the count says how many.
        let mut hits = [PonkoRayHit::default(); 1];
        assert_eq!(
            ponko_ctx_cast_ray2d_all(
                ctx,
                v2(0.0, 0.0),
                right,
                1000.0,
                solid,
                all_groups,
                hits.as_mut_ptr(),
                1,
                &mut a.all_total
            ),
            PONKO_OK
        );
        let mut hits = vec![PonkoRayHit::default(); a.all_total];
        let mut n = 0;
        assert_eq!(
            ponko_ctx_cast_ray2d_all(
                ctx,
                v2(0.0, 0.0),
                right,
                1000.0,
                solid,
                all_groups,
                hits.as_mut_ptr(),
                hits.len(),
                &mut n
            ),
            PONKO_OK
        );
        a.all = hits[..n].to_vec();
        a.at_point = entities(|o, c, n| ponko_ctx_overlap_point2d(ctx, v2(0.0, 100.0), solid, all_groups, o, c, n));
        let sensors = PonkoQueryFilter2D { sensors: true, ..solid };
        a.at_point_sensors =
            entities(|o, c, n| ponko_ctx_overlap_point2d(ctx, v2(0.0, 100.0), sensors, all_groups, o, c, n));
        a.in_circle =
            entities(|o, c, n| ponko_ctx_overlap_circle2d(ctx, v2(200.0, 0.0), 95.0, solid, all_groups, o, c, n));
        // A long thin box, turned a quarter: it reaches down to the sensor only.
        a.in_box = entities(|o, c, n| {
            ponko_ctx_overlap_box2d(
                ctx,
                v2(0.0, 50.0),
                v2(120.0, 4.0),
                std::f32::consts::FRAC_PI_2,
                sensors,
                all_groups,
                o,
                c,
                n,
            )
        });
        a.in_capsule =
            entities(|o, c, n| ponko_ctx_overlap_capsule2d(ctx, v2(300.0, 0.0), 10.0, 2.0, solid, all_groups, o, c, n));
        let (mut found, mut near) = (false, PonkoPointHit2D::default());
        assert_eq!(
            ponko_ctx_closest_point2d(ctx, v2(0.0, 0.0), 500.0, solid, all_groups, &mut found, &mut near),
            PONKO_OK
        );
        a.closest = found.then_some(near);
        let mut pushes = [PonkoPenetration2D::default(); 4];
        let mut n = 0;
        // Radius 10 centered 4 px left of the wall's face: 6 px in.
        assert_eq!(
            ponko_ctx_penetrations2d(
                ctx,
                ponko_collider_circle(10.0),
                v2(86.0, 0.0),
                0.0,
                solid,
                all_groups,
                pushes.as_mut_ptr(),
                pushes.len(),
                &mut n
            ),
            PONKO_OK
        );
        a.pushes = pushes[..n].to_vec();
        let mut polygon = ponko_collider_circle(1.0);
        polygon.shape = PonkoShapeKind::PONKO_SHAPE_POLYGON as u32;
        a.polygon_push = Some(ponko_ctx_penetrations2d(
            ctx,
            polygon,
            v2(0.0, 0.0),
            0.0,
            solid,
            all_groups,
            pushes.as_mut_ptr(),
            pushes.len(),
            &mut n,
        ));
        scene.answers = Some(a);
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn c_2d_queries_follow_the_rust_rules() {
    let mut scene = Scene::default();
    let user = (&raw mut scene).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"queries".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics(g, PonkoVec2::default()), PONKO_OK);
        assert_eq!(ponko_game_sim_system(g, c"ask".as_ptr(), 1, Some(ask), user), PONKO_OK);
        assert_eq!(ponko_game_run_headless(g, 2, Some(setup), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    let groups = scene.groups_read.unwrap();
    assert_eq!((groups.memberships, groups.filter), (TERRAIN, u32::MAX));
    let a = scene.answers.expect("asked");

    let ray = a.ray.expect("hits the wall");
    assert_eq!(bits(ray.entity), scene.wall);
    assert!(
        close(ray.distance, 90.0),
        "a long direction is normalized: {}",
        ray.distance
    );
    assert_eq!(bits(a.ray_terrain_only.unwrap().entity), scene.terrain);
    assert_eq!(bits(a.excluded.unwrap().entity), scene.terrain);
    assert!(!a.zero_direction);

    assert_eq!(a.all_total, 2);
    let order: Vec<u64> = a.all.iter().map(|h| bits(h.entity)).collect();
    assert_eq!(order, [scene.wall, scene.terrain], "nearest first");
    assert!(close(a.all[1].distance, 290.0));

    assert!(close(a.circle.unwrap().distance, 80.0));
    assert!(close(a.rect.unwrap().distance, 80.0));
    assert!(close(a.capsule.unwrap().distance, 85.0));

    assert!(a.at_point.is_empty(), "sensors only with the flag");
    assert_eq!(a.at_point_sensors, [scene.sensor]);
    let mut both = vec![scene.wall, scene.terrain];
    both.sort_unstable();
    assert_eq!(a.in_circle, both);
    assert_eq!(a.in_box, [scene.sensor]);
    assert_eq!(a.in_capsule, [scene.terrain]);

    let near = a.closest.expect("the sensor is skipped: the wall is nearest");
    assert_eq!(bits(near.entity), scene.wall);
    assert!(close(near.distance, 90.0) && !near.inside);

    assert_eq!(a.pushes.len(), 1);
    assert_eq!(bits(a.pushes[0].entity), scene.wall);
    assert!(close(a.pushes[0].depth, 6.0), "{:?}", a.pushes[0]);
    assert!(close(a.pushes[0].normal.x, -1.0));
    assert_eq!(a.polygon_push, Some(PONKO_INVALID_ARGUMENT));
}

#[test]
fn setting_a_collider_from_c_keeps_every_group_until_set() {
    let w = ponko_world_new();
    unsafe {
        let (mut transform, mut collider, mut e) = (0, 0, 0);
        assert_eq!(ponko_transform_component(w, &mut transform), PONKO_OK);
        assert_eq!(ponko_collider_component(w, &mut collider), PONKO_OK);
        let (at, shape) = (PonkoTransform2D::default(), ponko_collider_circle(4.0));
        let parts = [value(transform, &at), value(collider, &shape)];
        assert_eq!(ponko_spawn(w, parts.as_ptr(), 2, &mut e), PONKO_OK);
        let mut g = PonkoCollisionGroups {
            memberships: 0,
            filter: 0,
        };
        assert_eq!(ponko_collider_groups(w, e, &mut g), PONKO_OK);
        assert_eq!((g.memberships, g.filter), (u32::MAX, u32::MAX));
        let mine = PonkoCollisionGroups {
            memberships: 1,
            filter: 2,
        };
        assert_eq!(ponko_set_collider_groups(w, e, mine), PONKO_OK);
        assert_eq!(ponko_collider_groups(w, e, &mut g), PONKO_OK);
        assert_eq!((g.memberships, g.filter), (1, 2));
        assert_eq!(ponko_collider_groups(w, e, null_mut()), PONKO_INVALID_ARGUMENT);
        ponko_world_free(w);
    }
}
