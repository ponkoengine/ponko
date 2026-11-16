//! 3D physics queries (feature `physics3d`): rays, shape casts, overlaps,
//! closest points and penetrations, with the same rules as the 2D queries
//! (direction, range, starting inside, order and ties, filters).
#![cfg(feature = "physics3d")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::f32::consts::FRAC_PI_4;
use std::rc::Rc;

use ponko::d3::{Collider, CollisionGroups, Hit, Physics, QueryFilter, RigidBody, Shape, Transform};
use ponko::prelude::*;

// Counts allocations made by the measuring thread only (the physics step and
// the queries run on it), so other threads can't make the count flaky.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn record_allocation() {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
    }
}

// SAFETY: forwards to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the caller's contract, passed on.
        unsafe { System.alloc(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: the caller's contract, passed on.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, passed on.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const NO_FILTER: QueryFilter = QueryFilter {
    exclude: None,
    groups: CollisionGroups::ALL,
    sensors: false,
};
const Q: Quat = Quat::IDENTITY;

fn wall(at: Vec3, size: Vec3) -> (Transform, RigidBody, Collider) {
    (Transform::at(at), RigidBody::fixed(), Collider::cuboid(size))
}
fn post(at: Vec3, radius: f32) -> (Transform, RigidBody, Collider) {
    (Transform::at(at), RigidBody::fixed(), Collider::sphere(radius))
}
/// A wall whose -X face is at x = 0, 10 deep and 40 × 40 across.
fn left_face_at_zero() -> (Transform, RigidBody, Collider) {
    wall(vec3(5.0, 0.0, 0.0), vec3(10.0, 40.0, 40.0))
}

/// Spawns with `spawn`, steps once so physics knows the bodies, then asks
/// `question` of `ctx.physics3d` and returns its answer and what `spawn`
/// returned.
fn ask<T, R: 'static>(spawn: impl FnOnce(&mut Ctx) -> T, question: impl FnOnce(&Physics) -> R + 'static) -> (T, R) {
    let answer = Rc::new(RefCell::new(None));
    let slot = answer.clone();
    let mut question = Some(question);
    let mut spawned = None;
    Game::new("queries", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1
                && let Some(q) = question.take()
            {
                *slot.borrow_mut() = Some(q(&ctx.physics3d));
            }
        })
        .run_headless(2, |ctx| spawned = Some(spawn(ctx)));
    let answer = answer.borrow_mut().take().expect("the question was asked");
    (spawned.unwrap(), answer)
}

/// [`ask`], with `question` also given what `spawn` returned (meshes made
/// in the setup).
fn ask_with<T: Copy + 'static, R: 'static>(
    spawn: impl FnOnce(&mut Ctx) -> T,
    question: impl FnOnce(&Physics, T) -> R + 'static,
) -> R {
    let made = Rc::new(Cell::new(None));
    let seen = made.clone();
    let (_, answer) = ask(
        move |c| made.set(Some(spawn(c))),
        move |p| question(p, seen.get().expect("spawned first")),
    );
    answer
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3
}
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn cast_ray_all_lists_every_hit_nearest_first() {
    let (posts, hits) = ask(
        // Spawned far to near, so the order can't come from spawn order.
        |c| [30.0, 20.0, 10.0].map(|x| c.world.spawn(post(vec3(x, 0.0, 0.0), 1.0))),
        |p| {
            let mut hits = vec![Hit {
                entity: Entity::from_bits(7),
                point: Vec3::ZERO,
                normal: Vec3::ZERO,
                distance: -1.0,
            }];
            p.cast_ray_all(Vec3::ZERO, Vec3::X, 100.0, NO_FILTER, &mut hits);
            hits
        },
    );
    let order: Vec<Entity> = hits.iter().map(|h| h.entity).collect();
    assert_eq!(order, [posts[2], posts[1], posts[0]], "nearest first, buffer cleared");
    for (hit, x) in hits.iter().zip([9.0, 19.0, 29.0]) {
        assert!(close(hit.distance, x), "{}", hit.distance);
        assert!(near(hit.point, vec3(x, 0.0, 0.0)));
        assert!(near(hit.normal, Vec3::NEG_X));
    }
}

#[test]
fn any_non_zero_direction_is_normalized() {
    let (_, d) = ask(
        |c| c.world.spawn(post(vec3(10.0, 0.0, 0.0), 1.0)),
        |p| {
            [
                p.cast_ray(Vec3::ZERO, vec3(25.0, 0.0, 0.0), 100.0, NO_FILTER),
                p.cast_ray(Vec3::ZERO, vec3(1e-3, 0.0, 0.0), 100.0, NO_FILTER),
                p.cast_sphere(Vec3::ZERO, 0.5, vec3(40.0, 0.0, 0.0), 100.0, NO_FILTER),
                p.cast_box(Vec3::ZERO, Vec3::ONE, Q, vec3(40.0, 0.0, 0.0), 100.0, NO_FILTER),
                p.cast_capsule(Vec3::ZERO, 0.0, 0.5, vec3(40.0, 0.0, 0.0), 100.0, NO_FILTER),
            ]
            .map(|h| h.expect("hits").distance)
        },
    );
    assert!(close(d[0], 9.0) && close(d[1], 9.0), "{d:?}");
    assert!(close(d[2], 8.5) && close(d[3], 8.5) && close(d[4], 8.5), "{d:?}");
}

#[test]
fn a_zero_or_non_finite_direction_finds_nothing() {
    let (_, found) = ask(
        |c| c.world.spawn(post(vec3(10.0, 0.0, 0.0), 1.0)),
        |p| {
            let mut all = vec![];
            let mut any = Vec::new();
            for dir in [Vec3::ZERO, vec3(f32::NAN, 0.0, 0.0), vec3(f32::INFINITY, 0.0, 0.0)] {
                all.push(p.cast_ray(Vec3::ZERO, dir, 100.0, NO_FILTER).is_some());
                all.push(p.cast_sphere(Vec3::ZERO, 0.5, dir, 100.0, NO_FILTER).is_some());
                all.push(p.cast_box(Vec3::ZERO, Vec3::ONE, Q, dir, 100.0, NO_FILTER).is_some());
                all.push(p.cast_capsule(Vec3::ZERO, 0.4, 0.2, dir, 100.0, NO_FILTER).is_some());
                p.cast_ray_all(Vec3::ZERO, dir, 100.0, NO_FILTER, &mut any);
                all.push(!any.is_empty());
            }
            all
        },
    );
    assert!(found.iter().all(|f| !f), "{found:?}");
}

#[test]
fn a_negative_or_nan_range_finds_nothing_and_an_infinite_one_works() {
    let (_, (bad, infinite)) = ask(
        |c| c.world.spawn(post(vec3(10.0, 0.0, 0.0), 1.0)),
        |p| {
            let mut bad = vec![];
            let mut any = Vec::new();
            for max in [-1.0, f32::NAN] {
                bad.push(p.cast_ray(Vec3::ZERO, Vec3::X, max, NO_FILTER).is_some());
                bad.push(p.cast_sphere(Vec3::ZERO, 0.5, Vec3::X, max, NO_FILTER).is_some());
                bad.push(p.cast_box(Vec3::ZERO, Vec3::ONE, Q, Vec3::X, max, NO_FILTER).is_some());
                bad.push(p.cast_capsule(Vec3::ZERO, 0.4, 0.2, Vec3::X, max, NO_FILTER).is_some());
                bad.push(p.closest_point(Vec3::ZERO, max, NO_FILTER).is_some());
                p.cast_ray_all(Vec3::ZERO, Vec3::X, max, NO_FILTER, &mut any);
                bad.push(!any.is_empty());
            }
            let inf = f32::INFINITY;
            let infinite = [
                p.cast_ray(Vec3::ZERO, Vec3::X, inf, NO_FILTER).map(|h| h.distance),
                p.cast_sphere(Vec3::ZERO, 0.5, Vec3::X, inf, NO_FILTER)
                    .map(|h| h.distance),
                p.cast_box(Vec3::ZERO, Vec3::ONE, Q, Vec3::X, inf, NO_FILTER)
                    .map(|h| h.distance),
                p.closest_point(Vec3::ZERO, inf, NO_FILTER).map(|h| h.distance),
            ];
            (bad, infinite)
        },
    );
    assert!(bad.iter().all(|f| !f), "{bad:?}");
    let [ray, sphere, cube, nearest] = infinite.map(Option::unwrap);
    assert!(close(ray, 9.0) && close(sphere, 8.5) && close(cube, 8.5) && close(nearest, 9.0));
}

#[test]
fn a_non_finite_origin_or_rotation_finds_nothing() {
    let (_, found) = ask(
        |c| c.world.spawn(wall(Vec3::ZERO, Vec3::splat(100.0))),
        |p| {
            let nan = vec3(f32::NAN, 0.0, 0.0);
            let bad_turn = Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0);
            let ball = Shape::Sphere { radius: 1.0 };
            let mut out = Vec::new();
            let mut pushes = Vec::new();
            let mut hits = Vec::new();
            let mut found = vec![
                p.cast_ray(nan, Vec3::X, 10.0, NO_FILTER).is_some(),
                p.cast_sphere(nan, 1.0, Vec3::X, 10.0, NO_FILTER).is_some(),
                p.cast_capsule(nan, 1.0, 1.0, Vec3::X, 10.0, NO_FILTER).is_some(),
                p.cast_box(nan, Vec3::ONE, Q, Vec3::X, 10.0, NO_FILTER).is_some(),
                p.cast_box(Vec3::ZERO, Vec3::ONE, bad_turn, Vec3::X, 10.0, NO_FILTER)
                    .is_some(),
                p.closest_point(nan, 10.0, NO_FILTER).is_some(),
            ];
            p.cast_ray_all(nan, Vec3::X, 10.0, NO_FILTER, &mut hits);
            found.push(!hits.is_empty());
            p.overlap_point(nan, NO_FILTER, &mut out);
            found.push(!out.is_empty());
            p.overlap_sphere(nan, 1.0, NO_FILTER, &mut out);
            found.push(!out.is_empty());
            p.overlap_capsule(nan, 1.0, 1.0, NO_FILTER, &mut out);
            found.push(!out.is_empty());
            p.overlap_box(Vec3::ZERO, Vec3::ONE, bad_turn, NO_FILTER, &mut out);
            found.push(!out.is_empty());
            p.penetrations(ball, nan, Q, NO_FILTER, &mut pushes);
            found.push(!pushes.is_empty());
            p.penetrations(ball, Vec3::ZERO, bad_turn, NO_FILTER, &mut pushes);
            found.push(!pushes.is_empty());
            found
        },
    );
    assert!(found.iter().all(|f| !f), "{found:?}");
}

#[test]
fn a_ray_starting_inside_hits_at_distance_zero_with_a_zero_normal() {
    let (wall_e, (hit, all)) = ask(
        |c| c.world.spawn(wall(Vec3::ZERO, Vec3::splat(10.0))),
        |p| {
            let origin = vec3(1.0, 0.5, 0.0);
            let mut all = Vec::new();
            p.cast_ray_all(origin, Vec3::X, 100.0, NO_FILTER, &mut all);
            (p.cast_ray(origin, Vec3::X, 100.0, NO_FILTER), all)
        },
    );
    let hit = hit.expect("hits the wall it starts in");
    assert_eq!(hit.entity, wall_e);
    assert_eq!(hit.distance, 0.0);
    assert!(near(hit.point, vec3(1.0, 0.5, 0.0)));
    assert_eq!(hit.normal, Vec3::ZERO);
    assert_eq!(all.len(), 1);
    assert_eq!((all[0].distance, all[0].normal), (0.0, Vec3::ZERO));
}

#[test]
fn a_shape_cast_starting_inside_hits_at_distance_zero_with_a_push_out_normal() {
    let (wall_e, hits) = ask(
        |c| c.world.spawn(left_face_at_zero()),
        |p| {
            // Radius 1 centered 0.4 left of the face: 0.6 in.
            let at = vec3(-0.4, 0.0, 0.0);
            [
                p.cast_sphere(at, 1.0, Vec3::X, 10.0, NO_FILTER),
                p.cast_box(at, Vec3::splat(2.0), Q, Vec3::X, 10.0, NO_FILTER),
                p.cast_capsule(at, 2.0, 1.0, Vec3::X, 10.0, NO_FILTER),
                // Moving away from the wall still reports the overlap.
                p.cast_box(at, Vec3::splat(2.0), Q, Vec3::NEG_X, 10.0, NO_FILTER),
            ]
        },
    );
    for hit in hits {
        let hit = hit.expect("starts overlapping");
        assert_eq!(hit.entity, wall_e);
        assert_eq!(hit.distance, 0.0);
        assert!(near(hit.normal, Vec3::NEG_X), "{:?}", hit.normal);
        assert!(close(hit.point.x, 0.0), "{:?}", hit.point);
    }
}

#[test]
fn cast_box_reports_where_the_box_first_touches() {
    let (ground, hits) = ask(
        // Ground whose top is at y = -10.
        |c| c.world.spawn(wall(vec3(0.0, -11.0, 0.0), vec3(40.0, 2.0, 40.0))),
        |p| {
            let down = |rotation| p.cast_box(Vec3::ZERO, Vec3::splat(2.0), rotation, Vec3::NEG_Y, 100.0, NO_FILTER);
            [
                down(Q),
                // Turned 45° about Z: an edge reaches √2 down.
                down(Quat::from_rotation_z(FRAC_PI_4)),
                // Turned about Y only: still flat underneath.
                down(Quat::from_rotation_y(FRAC_PI_4)),
                // A wide, flat slab turned to stand up: 5 down.
                p.cast_box(
                    Vec3::ZERO,
                    vec3(10.0, 1.0, 1.0),
                    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                    Vec3::NEG_Y,
                    100.0,
                    NO_FILTER,
                ),
            ]
        },
    );
    let [flat, edge, spun, standing] = hits.map(|h| h.expect("hits"));
    for hit in [flat, edge, spun, standing] {
        assert_eq!(hit.entity, ground);
        assert!(near(hit.normal, Vec3::Y), "{:?}", hit.normal);
        assert!(close(hit.point.y, -10.0), "{:?}", hit.point);
    }
    assert!(close(flat.distance, 9.0), "{}", flat.distance);
    assert!((edge.distance - (10.0 - 2f32.sqrt())).abs() < 1e-3, "{}", edge.distance);
    assert!(close(spun.distance, 9.0), "{}", spun.distance);
    assert!(close(standing.distance, 5.0), "{}", standing.distance);
}

#[test]
fn overlap_point_finds_the_colliders_containing_it() {
    let (spawned, (inside, both, outside)) = ask(
        |c| {
            [
                c.world.spawn(post(Vec3::ZERO, 1.0)),
                c.world.spawn(wall(vec3(1.0, 0.0, 0.0), Vec3::ONE)),
            ]
        },
        |p| {
            let mut inside = Vec::new();
            let mut both = Vec::new();
            let mut outside = vec![Entity::from_bits(3)];
            p.overlap_point(vec3(0.0, 0.5, 0.0), NO_FILTER, &mut inside);
            p.overlap_point(vec3(0.75, 0.0, 0.0), NO_FILTER, &mut both);
            both.sort_unstable_by_key(|e| e.to_bits());
            p.overlap_point(vec3(0.0, 2.0, 0.0), NO_FILTER, &mut outside);
            (inside, both, outside)
        },
    );
    assert_eq!(inside, [spawned[0]]);
    let mut ids = spawned.to_vec();
    ids.sort_unstable_by_key(|e| e.to_bits());
    assert_eq!(both, ids);
    assert!(outside.is_empty(), "the buffer is cleared");
}

#[test]
fn closest_point_finds_the_nearest_surface_and_inside_points_are_their_own() {
    let (posts, (outside, inside, far, face)) = ask(
        |c| {
            [
                c.world.spawn(post(vec3(10.0, 0.0, 0.0), 1.0)),
                c.world.spawn(post(vec3(-20.0, 0.0, 0.0), 1.0)),
                c.world.spawn(wall(vec3(0.0, 0.0, -30.0), Vec3::splat(2.0))),
            ]
        },
        |p| {
            (
                p.closest_point(Vec3::ZERO, 100.0, NO_FILTER),
                p.closest_point(vec3(10.3, 0.0, 0.0), 100.0, NO_FILTER),
                p.closest_point(Vec3::ZERO, 5.0, NO_FILTER),
                p.closest_point(vec3(0.5, 0.5, -25.0), 100.0, NO_FILTER),
            )
        },
    );
    let outside = outside.expect("finds the near post");
    assert_eq!(outside.entity, posts[0]);
    assert!(near(outside.point, vec3(9.0, 0.0, 0.0)));
    assert!(close(outside.distance, 9.0));
    assert!(!outside.inside);
    let inside = inside.expect("inside the post");
    assert_eq!((inside.entity, inside.distance, inside.inside), (posts[0], 0.0, true));
    assert!(near(inside.point, vec3(10.3, 0.0, 0.0)));
    assert!(far.is_none(), "nothing within 5");
    let face = face.expect("the box's near face");
    assert_eq!(face.entity, posts[2]);
    assert!(near(face.point, vec3(0.5, 0.5, -29.0)), "{:?}", face.point);
    assert!(close(face.distance, 4.0));
}

#[test]
fn penetrations_report_depth_and_way_out_deepest_first() {
    let (walls, pushes) = ask(
        |c| {
            [
                // -X face at x = 0.
                c.world.spawn(left_face_at_zero()),
                // Top face at y = 0.
                c.world.spawn(wall(vec3(0.0, -5.0, 0.0), vec3(40.0, 10.0, 40.0))),
            ]
        },
        |p| {
            let mut pushes = Vec::new();
            // Radius 1 centered at (-0.7, 0.2, 0): 0.3 into the first wall,
            // 0.8 into the second.
            p.penetrations(
                Shape::Sphere { radius: 1.0 },
                vec3(-0.7, 0.2, 0.0),
                Q,
                NO_FILTER,
                &mut pushes,
            );
            pushes
        },
    );
    assert_eq!(pushes.len(), 2);
    assert_eq!(pushes[0].entity, walls[1], "the deeper one first");
    assert!(close(pushes[0].depth, 0.8), "{}", pushes[0].depth);
    assert!(near(pushes[0].normal, Vec3::Y), "{:?}", pushes[0].normal);
    assert!(close(pushes[0].point.y, 0.0));
    assert_eq!(pushes[1].entity, walls[0]);
    assert!(close(pushes[1].depth, 0.3), "{}", pushes[1].depth);
    assert!(near(pushes[1].normal, Vec3::NEG_X));
}

#[test]
fn equal_depths_are_ordered_by_entity_id() {
    let (walls, pushes) = ask(
        |c| [0, 1].map(|_| c.world.spawn(left_face_at_zero())),
        |p| {
            let mut pushes = Vec::new();
            p.penetrations(
                Shape::Sphere { radius: 1.0 },
                vec3(-0.5, 0.0, 0.0),
                Q,
                NO_FILTER,
                &mut pushes,
            );
            pushes
        },
    );
    assert_eq!(pushes.len(), 2);
    assert_eq!(pushes[0].depth, pushes[1].depth);
    let mut ids = walls.map(Entity::to_bits);
    ids.sort_unstable();
    assert_eq!([pushes[0].entity.to_bits(), pushes[1].entity.to_bits()], ids);
}

#[test]
fn moving_by_the_deepest_penetration_frees_the_shape() {
    let shape = Collider::capsule(2.0, 0.6).shape;
    let turn = Quat::from_rotation_z(0.3) * Quat::from_rotation_x(0.2);
    let (_, (before, just_out, clear)) = ask(
        |c| c.world.spawn(left_face_at_zero()),
        move |p| {
            let mut pushes = Vec::new();
            let start = vec3(0.2, 0.3, 0.1);
            p.penetrations(shape, start, turn, NO_FILTER, &mut pushes);
            let first = pushes[0];
            let before = (pushes.len(), first.depth);
            // Moved by exactly the depth (plus rounding), it only touches.
            let out = |extra: f32| start + first.normal * (first.depth + extra);
            p.penetrations(shape, out(1e-3), turn, NO_FILTER, &mut pushes);
            let just_out = pushes.first().map(|p| p.depth);
            p.penetrations(shape, out(0.05), turn, NO_FILTER, &mut pushes);
            (before, just_out, pushes.len())
        },
    );
    assert_eq!(before.0, 1);
    assert!(before.1 > 0.5, "{before:?}");
    assert!(just_out.is_none_or(|d| d < 1e-4), "{just_out:?}");
    assert_eq!(clear, 0);
}

/// A 2 m cube centered on the origin: corners and closed triangles.
fn cube_mesh() -> (Vec<Vec3>, [[u32; 3]; 12]) {
    let corners = (0..8u8)
        .map(|i| vec3(f32::from(i & 1), f32::from((i >> 1) & 1), f32::from((i >> 2) & 1)) * 2.0 - 1.0)
        .collect();
    let faces = [
        [0, 2, 1],
        [1, 2, 3],
        [4, 5, 6],
        [5, 7, 6],
        [0, 1, 4],
        [1, 5, 4],
        [2, 6, 3],
        [3, 6, 7],
        [0, 4, 2],
        [2, 4, 6],
        [1, 3, 5],
        [3, 7, 5],
    ];
    (corners, faces)
}

#[test]
fn penetrations_work_for_every_shape() {
    let depths = ask_with(
        |c| {
            c.world.spawn(left_face_at_zero());
            let (corners, faces) = cube_mesh();
            let hull = c.physics3d.convex_hull(&corners).unwrap();
            let tri = c.physics3d.trimesh(&corners, &faces).unwrap();
            // Off-center points: the mesh keeps its offset.
            let shifted: Vec<Vec3> = corners.iter().map(|&p| p + vec3(5.0, 0.0, 0.0)).collect();
            let off = c.physics3d.convex_hull(&shifted).unwrap();
            let freed = c.physics3d.convex_hull(&corners).unwrap();
            c.physics3d.free(freed);
            [hull, tri, off, freed]
        },
        |p, [hull, tri, off, freed]| {
            let mut pushes = Vec::new();
            [
                (Shape::Sphere { radius: 1.0 }, Vec3::ZERO),
                (Shape::Cuboid { size: Vec3::splat(2.0) }, Vec3::ZERO),
                (
                    Shape::Capsule {
                        height: 1.0,
                        radius: 1.0,
                    },
                    Vec3::ZERO,
                ),
                (Shape::Mesh(hull), Vec3::ZERO),
                (Shape::Mesh(tri), Vec3::ZERO),
                (Shape::Mesh(off), vec3(-5.0, 0.0, 0.0)),
                (Shape::Mesh(freed), Vec3::ZERO),
            ]
            .map(|(shape, shift)| {
                // Centered 0.6 left of the face: 0.4 in.
                p.penetrations(shape, vec3(-0.6, 0.0, 0.0) + shift, Q, NO_FILTER, &mut pushes);
                pushes.first().map(|p| (p.depth, p.normal))
            })
        },
    );
    let [solid @ .., freed] = depths;
    for found in solid {
        let (depth, normal) = found.expect("overlaps the wall");
        assert!(close(depth, 0.4), "{depths:?}");
        assert!(near(normal, Vec3::NEG_X), "{depths:?}");
    }
    assert!(freed.is_none(), "a freed mesh finds nothing");
}

#[test]
fn penetrations_reach_into_hulls_triangle_meshes_and_terrain() {
    let found = ask_with(
        |c| {
            let (corners, faces) = cube_mesh();
            // Ten-meter cubes: their -X faces at x = 0, 20 and 40.
            let big: Vec<Vec3> = corners.iter().map(|&p| p * 5.0).collect();
            let hull = c.physics3d.convex_hull(&big).unwrap();
            let tri = c.physics3d.trimesh(&big, &faces).unwrap();
            let hull_e = c.world.spawn((
                Transform::at(vec3(5.0, 0.0, 0.0)),
                RigidBody::fixed(),
                Collider::mesh(hull),
            ));
            let tri_e = c.world.spawn((
                Transform::at(vec3(25.0, 0.0, 0.0)),
                RigidBody::fixed(),
                Collider::mesh(tri),
            ));
            // Flat ground at y = -20, 40 m square, its corner at (-20, -20, -20).
            let ground = c.physics3d.heightfield(&[0.0; 9], 40.0);
            let ground_e = c.world.spawn((
                Transform::at(vec3(-20.0, -20.0, -20.0)),
                RigidBody::fixed(),
                Collider::mesh(ground),
            ));
            [hull_e, tri_e, ground_e]
        },
        |p, [hull_e, tri_e, ground_e]| {
            let mut pushes = Vec::new();
            let mut seen = vec![];
            let ball = Shape::Sphere { radius: 1.0 };
            let cube = Shape::Cuboid { size: Vec3::splat(2.0) };
            for (shape, at, entity) in [
                // 0.4 into each face.
                (ball, vec3(-0.6, 0.0, 0.0), hull_e),
                (cube, vec3(-0.6, 0.0, 0.0), hull_e),
                (ball, vec3(19.4, 0.0, 0.0), tri_e),
                (cube, vec3(19.4, 0.0, 0.0), tri_e),
                (ball, vec3(0.0, -19.4, -10.0), ground_e),
                (cube, vec3(0.0, -19.4, -10.0), ground_e),
                // Deep inside the hull: 2 m from its -X face, the nearest.
                (ball, vec3(2.0, 0.0, 0.0), hull_e),
            ] {
                p.penetrations(shape, at, Q, NO_FILTER, &mut pushes);
                seen.push((
                    pushes.len(),
                    pushes.first().map(|p| (p.entity == entity, p.depth, p.normal)),
                ));
            }
            seen
        },
    );
    let expect = [
        (0.4, Vec3::NEG_X),
        (0.4, Vec3::NEG_X),
        (0.4, Vec3::NEG_X),
        (0.4, Vec3::NEG_X),
        (0.4, Vec3::Y),
        (0.4, Vec3::Y),
        (3.0, Vec3::NEG_X),
    ];
    for ((count, first), (depth, normal)) in found.iter().zip(expect) {
        let (right, d, n) = first.unwrap_or_else(|| panic!("overlaps: {found:?}"));
        assert!(*count == 1 && right, "{found:?}");
        assert!((d - depth).abs() < 1e-2 && near(n, normal), "{found:?}");
    }
}

#[test]
fn sensors_are_seen_only_with_sensors_on() {
    let (sensor, (without, with)) = ask(
        |c| {
            c.world.spawn((
                Transform::at(vec3(10.0, 0.0, 0.0)),
                RigidBody::fixed(),
                Collider::sphere(1.0).sensor(),
            ))
        },
        |p| {
            let run = |filter: QueryFilter| {
                let at = vec3(10.0, 0.0, 0.0);
                let mut out = Vec::new();
                let mut hits = Vec::new();
                let mut pushes = Vec::new();
                let mut seen = vec![
                    p.cast_ray(Vec3::ZERO, Vec3::X, 100.0, filter).map(|h| h.entity),
                    p.cast_sphere(Vec3::ZERO, 0.5, Vec3::X, 100.0, filter).map(|h| h.entity),
                    p.cast_box(Vec3::ZERO, Vec3::ONE, Q, Vec3::X, 100.0, filter)
                        .map(|h| h.entity),
                    p.cast_capsule(Vec3::ZERO, 0.4, 0.2, Vec3::X, 100.0, filter)
                        .map(|h| h.entity),
                    p.closest_point(Vec3::ZERO, 100.0, filter).map(|h| h.entity),
                ];
                p.cast_ray_all(Vec3::ZERO, Vec3::X, 100.0, filter, &mut hits);
                seen.push(hits.first().map(|h| h.entity));
                p.overlap_point(at, filter, &mut out);
                seen.push(out.first().copied());
                p.overlap_sphere(at, 0.1, filter, &mut out);
                seen.push(out.first().copied());
                p.overlap_box(at, Vec3::splat(0.1), Q, filter, &mut out);
                seen.push(out.first().copied());
                p.overlap_capsule(at, 0.2, 0.1, filter, &mut out);
                seen.push(out.first().copied());
                p.penetrations(Shape::Sphere { radius: 0.2 }, at, Q, filter, &mut pushes);
                seen.push(pushes.first().map(|h| h.entity));
                seen
            };
            (run(NO_FILTER), run(NO_FILTER.with_sensors()))
        },
    );
    assert!(without.iter().all(Option::is_none), "{without:?}");
    assert!(with.iter().all(|e| *e == Some(sensor)), "{with:?}");
}

const PLAYER: u32 = 1 << 0;
const ENEMY: u32 = 1 << 1;
const TERRAIN: u32 = 1 << 2;

#[test]
fn query_groups_see_only_the_colliders_they_touch() {
    let (spawned, (terrain_only, players_only, all)) = ask(
        |c| {
            let player = c.world.spawn((
                Transform::at(vec3(10.0, 0.0, 0.0)),
                RigidBody::fixed(),
                Collider::sphere(1.0).groups(CollisionGroups::new(PLAYER, u32::MAX)),
            ));
            let ground = c.world.spawn((
                Transform::at(vec3(20.0, 0.0, 0.0)),
                RigidBody::fixed(),
                Collider::sphere(1.0).groups(CollisionGroups::new(TERRAIN, u32::MAX)),
            ));
            [player, ground]
        },
        |p| {
            let ask = |groups| {
                let filter = NO_FILTER.groups(groups);
                let mut pushes = Vec::new();
                let mut out = Vec::new();
                p.penetrations(
                    Shape::Cuboid {
                        size: vec3(30.0, 1.0, 1.0),
                    },
                    vec3(15.0, 0.0, 0.0),
                    Q,
                    filter,
                    &mut pushes,
                );
                let mut pushed: Vec<Entity> = pushes.iter().map(|h| h.entity).collect();
                pushed.sort_unstable_by_key(|e| e.to_bits());
                p.overlap_point(vec3(10.0, 0.0, 0.0), filter, &mut out);
                out.extend(
                    p.cast_box(Vec3::ZERO, Vec3::ONE, Q, Vec3::X, 100.0, filter)
                        .map(|h| h.entity),
                );
                out.extend(p.closest_point(vec3(15.0, 0.0, 0.0), 100.0, filter).map(|h| h.entity));
                (out, pushed)
            };
            (
                ask(CollisionGroups::new(u32::MAX, TERRAIN)),
                ask(CollisionGroups::new(ENEMY, PLAYER)),
                ask(CollisionGroups::ALL),
            )
        },
    );
    let [player, ground] = spawned;
    // overlap_point at the player, then cast_box, then closest_point.
    assert_eq!(terrain_only, (vec![ground, ground], vec![ground]));
    assert_eq!(players_only, (vec![player, player, player], vec![player]));
    let mut both = vec![player, ground];
    both.sort_unstable_by_key(|e| e.to_bits());
    assert_eq!(all.1, both);
    assert_eq!(all.0[..2], [player, player]);
}

#[test]
fn exclude_looks_through_one_entity_in_every_query() {
    let (posts, seen) = ask(
        // Two posts in a row, overlapping where the overlaps look.
        |c| {
            [
                c.world.spawn(post(vec3(10.0, 0.0, 0.0), 1.0)),
                c.world.spawn(post(vec3(10.5, 0.0, 0.0), 1.0)),
            ]
        },
        |p| {
            // Excludes the near post.
            let filter = NO_FILTER.exclude(p.cast_ray(Vec3::ZERO, Vec3::X, 100.0, NO_FILTER).unwrap().entity);
            let at = vec3(10.2, 0.0, 0.0);
            let mut out = Vec::new();
            let mut hits = Vec::new();
            let mut pushes = Vec::new();
            let mut seen = vec![
                p.cast_ray(Vec3::ZERO, Vec3::X, 100.0, filter).map(|h| h.entity),
                p.cast_sphere(Vec3::ZERO, 0.2, Vec3::X, 100.0, filter).map(|h| h.entity),
                p.cast_box(Vec3::ZERO, Vec3::splat(0.1), Q, Vec3::X, 100.0, filter)
                    .map(|h| h.entity),
                p.cast_capsule(Vec3::ZERO, 0.2, 0.1, Vec3::X, 100.0, filter)
                    .map(|h| h.entity),
                p.closest_point(Vec3::ZERO, 100.0, filter).map(|h| h.entity),
            ];
            p.cast_ray_all(Vec3::ZERO, Vec3::X, 100.0, filter, &mut hits);
            seen.push((hits.len() == 1).then(|| hits[0].entity));
            p.overlap_point(at, filter, &mut out);
            seen.push((out.len() == 1).then(|| out[0]));
            p.overlap_sphere(at, 0.1, filter, &mut out);
            seen.push((out.len() == 1).then(|| out[0]));
            p.overlap_box(at, Vec3::splat(0.1), Q, filter, &mut out);
            seen.push((out.len() == 1).then(|| out[0]));
            p.overlap_capsule(at, 0.2, 0.1, filter, &mut out);
            seen.push((out.len() == 1).then(|| out[0]));
            p.penetrations(Shape::Sphere { radius: 0.1 }, at, Q, filter, &mut pushes);
            seen.push((pushes.len() == 1).then(|| pushes[0].entity));
            seen
        },
    );
    assert!(seen.iter().all(|e| *e == Some(posts[1])), "{seen:?} {posts:?}");
}

#[test]
fn queries_with_reused_buffers_dont_allocate() {
    let counts = ask_with(
        |c| {
            for i in 0..50 {
                c.world.spawn(post(vec3(3.0 * i as f32, 0.0, 0.0), 1.0));
            }
            let (corners, faces) = cube_mesh();
            // Big hull, triangle mesh and ground colliders for the shapes to
            // sink into.
            let big: Vec<Vec3> = corners.iter().map(|&p| p * 5.0).collect();
            let (big_hull, big_tri) = (
                c.physics3d.convex_hull(&big).unwrap(),
                c.physics3d.trimesh(&big, &faces).unwrap(),
            );
            let ground = c.physics3d.heightfield(&[0.0; 9], 40.0);
            c.world.spawn((
                Transform::at(vec3(0.0, 20.0, 0.0)),
                RigidBody::fixed(),
                Collider::mesh(big_hull),
            ));
            c.world.spawn((
                Transform::at(vec3(20.0, 20.0, 0.0)),
                RigidBody::fixed(),
                Collider::mesh(big_tri),
            ));
            c.world.spawn((
                Transform::at(vec3(-20.0, -20.0, -20.0)),
                RigidBody::fixed(),
                Collider::mesh(ground),
            ));
            c.physics3d.convex_hull(&corners).unwrap()
        },
        |p, hull| {
            let mut hits = Vec::with_capacity(64);
            let mut out = Vec::with_capacity(64);
            let mut pushes = Vec::with_capacity(64);
            let from = vec3(-5.0, 0.0, 0.0);
            let turn = Quat::from_rotation_y(0.3);
            let mut run = || {
                p.cast_ray_all(from, Vec3::X, 1000.0, NO_FILTER, &mut hits);
                let mut n = hits.len();
                n += usize::from(p.cast_ray(from, Vec3::X, 1000.0, NO_FILTER).is_some());
                n += usize::from(p.cast_sphere(from, 0.5, Vec3::X, 1000.0, NO_FILTER).is_some());
                n += usize::from(p.cast_box(from, Vec3::ONE, turn, Vec3::X, 1000.0, NO_FILTER).is_some());
                n += usize::from(p.cast_capsule(from, 0.4, 0.2, Vec3::X, 1000.0, NO_FILTER).is_some());
                n += usize::from(p.closest_point(vec3(1.5, 3.0, 0.0), 10.0, NO_FILTER).is_some());
                p.overlap_point(vec3(3.0, 0.0, 0.0), NO_FILTER, &mut out);
                n += out.len();
                p.overlap_sphere(vec3(30.0, 0.0, 0.0), 10.0, NO_FILTER, &mut out);
                n += out.len();
                p.overlap_box(vec3(30.0, 0.0, 0.0), vec3(20.0, 2.0, 2.0), turn, NO_FILTER, &mut out);
                n += out.len();
                p.overlap_capsule(vec3(30.0, 0.0, 0.0), 2.0, 1.0, NO_FILTER, &mut out);
                n += out.len();
                p.penetrations(
                    Shape::Cuboid {
                        size: vec3(10.0, 0.8, 0.8),
                    },
                    vec3(30.0, 0.0, 0.0),
                    Q,
                    NO_FILTER,
                    &mut pushes,
                );
                n += pushes.len();
                p.penetrations(Shape::Mesh(hull), vec3(30.0, 0.0, 0.0), turn, NO_FILTER, &mut pushes);
                n += pushes.len();
                // Into a hull, a triangle mesh and the ground, deep and shallow.
                for (shape, at) in [
                    (Shape::Sphere { radius: 1.0 }, vec3(1.0, 20.0, 0.0)),
                    (
                        Shape::Capsule {
                            height: 1.0,
                            radius: 0.5,
                        },
                        vec3(-4.6, 20.0, 0.0),
                    ),
                    (Shape::Mesh(hull), vec3(15.5, 20.0, 0.0)),
                    (Shape::Cuboid { size: Vec3::ONE }, vec3(5.0, -19.8, 0.0)),
                    (Shape::Sphere { radius: 1.0 }, vec3(5.0, -19.5, 3.0)),
                ] {
                    p.penetrations(shape, at, turn, NO_FILTER, &mut pushes);
                    n += pushes.len();
                    n += usize::from(p.cast_box(at, Vec3::ONE, turn, Vec3::X, 10.0, NO_FILTER).is_some());
                }
                n
            };
            let warm = run();
            ALLOCATIONS.set(0);
            COUNTING.set(true);
            let again = run();
            COUNTING.set(false);
            (warm, again, ALLOCATIONS.get())
        },
    );
    let (warm, again, allocations) = counts;
    assert!(warm > 0 && warm == again, "the queries found things: {warm} {again}");
    assert_eq!(allocations, 0, "queries into reused buffers allocate nothing");
}

#[test]
fn query_results_name_their_dimension_free_types() {
    // `d3::PointHit` and `d3::Penetration` are the 3D forms of `d2::PointHit`
    // and `d2::Penetration`, with the same fields.
    let hit = ponko::d3::PointHit {
        entity: Entity::from_bits(1),
        point: Vec3::ZERO,
        distance: 0.0,
        inside: true,
    };
    let push = ponko::d3::Penetration {
        entity: hit.entity,
        point: hit.point,
        normal: Vec3::Y,
        depth: 0.0,
    };
    assert_eq!(push.entity, hit.entity);
}

#[test]
fn query_filter_builders_set_their_field() {
    let e = Entity::from_bits(42);
    let f = QueryFilter::default()
        .exclude(e)
        .groups(CollisionGroups::new(1, 2))
        .with_sensors();
    assert_eq!(
        f,
        QueryFilter {
            exclude: Some(e),
            groups: CollisionGroups::new(1, 2),
            sensors: true
        }
    );
    assert_eq!(QueryFilter::default(), NO_FILTER);
}

#[test]
fn box_and_capsule_queries_see_heightfield_terrain() {
    let (ground, found) = ask(
        |c| {
            // Flat ground at y = 0, 40 m square, its corner at (-20, 0, -20).
            let mesh = c.physics3d.heightfield(&[0.0; 9], 40.0);
            c.world.spawn((
                Transform::at(vec3(-20.0, 0.0, -20.0)),
                RigidBody::fixed(),
                Collider::mesh(mesh),
            ))
        },
        |p| {
            let mut out = Vec::new();
            let mut seen = vec![];
            // Sinking 0.1 into the ground, then hovering 0.1 above it.
            for y in [0.9, 1.1] {
                p.overlap_box(vec3(3.0, y, 2.0), Vec3::splat(2.0), Q, NO_FILTER, &mut out);
                seen.push(out.len());
                p.overlap_capsule(vec3(3.0, y, 2.0), 0.0, 1.0, NO_FILTER, &mut out);
                seen.push(out.len());
                p.overlap_box(
                    vec3(3.0, y, 2.0),
                    Vec3::splat(2.0),
                    Quat::from_rotation_y(0.7),
                    NO_FILTER,
                    &mut out,
                );
                seen.push(out.len());
            }
            let landing = p.cast_box(vec3(3.0, 5.0, 2.0), Vec3::splat(2.0), Q, Vec3::NEG_Y, 10.0, NO_FILTER);
            (seen, landing)
        },
    );
    let (seen, landing) = found;
    assert_eq!(seen, [1, 1, 1, 0, 0, 0]);
    let landing = landing.expect("lands on the ground");
    assert_eq!(landing.entity, ground);
    assert!(close(landing.distance, 4.0), "{landing:?}");
}
