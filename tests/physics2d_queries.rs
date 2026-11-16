//! 2D physics queries (feature `physics`): rays, shape casts, overlaps,
//! closest points and penetrations, with the same rules as the 3D queries
//! (direction, range, starting inside, order and ties, filters).
#![cfg(feature = "physics")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
#[cfg(feature = "physics3d")]
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
use std::rc::Rc;

use ponko::d2::{Collider, CollisionGroups, Hit, Physics, QueryFilter, RigidBody, Shape, Transform};
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

fn wall(at: Vec2, size: Vec2) -> (Transform, RigidBody, Collider) {
    (Transform::at(at), RigidBody::fixed(), Collider::rect(size))
}
fn post(at: Vec2, radius: f32) -> (Transform, RigidBody, Collider) {
    (Transform::at(at), RigidBody::fixed(), Collider::circle(radius))
}

/// Spawns with `spawn`, steps once so physics knows the bodies, then asks
/// `question` of `ctx.physics` and returns its answer and what `spawn`
/// returned.
fn ask<T, R: 'static>(spawn: impl FnOnce(&mut World) -> T, question: impl FnOnce(&Physics) -> R + 'static) -> (T, R) {
    let answer = Rc::new(RefCell::new(None));
    let slot = answer.clone();
    let mut question = Some(question);
    let mut spawned = None;
    Game::new("queries", 1, 1)
        .physics(Vec2::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1
                && let Some(q) = question.take()
            {
                *slot.borrow_mut() = Some(q(&ctx.physics));
            }
        })
        .run_headless(2, |ctx| spawned = Some(spawn(&mut ctx.world)));
    let answer = answer.borrow_mut().take().expect("the question was asked");
    (spawned.unwrap(), answer)
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-3
}
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn cast_ray_all_lists_every_hit_nearest_first() {
    let (posts, hits) = ask(
        |w| {
            // Spawned far to near, so the order can't come from spawn order.
            [300.0, 200.0, 100.0].map(|x| w.spawn(post(vec2(x, 0.0), 10.0)))
        },
        |p| {
            let mut hits = vec![Hit {
                entity: Entity::from_bits(7),
                point: Vec2::ZERO,
                normal: Vec2::ZERO,
                distance: -1.0,
            }];
            p.cast_ray_all(Vec2::ZERO, Vec2::X, 1000.0, NO_FILTER, &mut hits);
            hits
        },
    );
    let order: Vec<Entity> = hits.iter().map(|h| h.entity).collect();
    assert_eq!(order, [posts[2], posts[1], posts[0]], "nearest first, buffer cleared");
    for (hit, x) in hits.iter().zip([90.0, 190.0, 290.0]) {
        assert!(close(hit.distance, x), "{}", hit.distance);
        assert!(near(hit.point, vec2(x, 0.0)));
        assert!(near(hit.normal, vec2(-1.0, 0.0)));
    }
}

#[test]
fn cast_ray_all_stops_at_max_distance() {
    let (_, hits) = ask(
        |w| [100.0, 200.0].map(|x| w.spawn(post(vec2(x, 0.0), 10.0))),
        |p| {
            let mut hits = Vec::new();
            p.cast_ray_all(Vec2::ZERO, Vec2::X, 150.0, NO_FILTER, &mut hits);
            hits
        },
    );
    assert_eq!(hits.len(), 1);
}

#[test]
fn equal_distances_are_ordered_by_entity_id() {
    let (posts, hits) = ask(
        |w| {
            [
                w.spawn(post(vec2(100.0, 0.0), 10.0)),
                w.spawn(post(vec2(100.0, 0.0), 10.0)),
            ]
        },
        |p| {
            let mut hits = Vec::new();
            p.cast_ray_all(Vec2::ZERO, Vec2::X, 1000.0, NO_FILTER, &mut hits);
            hits
        },
    );
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].distance, hits[1].distance);
    let mut ids = posts.map(Entity::to_bits);
    ids.sort_unstable();
    assert_eq!([hits[0].entity.to_bits(), hits[1].entity.to_bits()], ids);
}

#[test]
fn any_non_zero_direction_is_normalized() {
    let (_, d) = ask(
        |w| w.spawn(post(vec2(100.0, 0.0), 10.0)),
        |p| {
            [
                p.cast_ray(Vec2::ZERO, vec2(25.0, 0.0), 1000.0, NO_FILTER),
                p.cast_ray(Vec2::ZERO, vec2(1e-3, 0.0), 1000.0, NO_FILTER),
                p.cast_circle(Vec2::ZERO, 5.0, vec2(40.0, 0.0), 1000.0, NO_FILTER),
                p.cast_box(Vec2::ZERO, vec2(10.0, 10.0), 0.0, vec2(40.0, 0.0), 1000.0, NO_FILTER),
                p.cast_capsule(Vec2::ZERO, 0.0, 5.0, vec2(40.0, 0.0), 1000.0, NO_FILTER),
            ]
            .map(|h| h.expect("hits").distance)
        },
    );
    assert!(close(d[0], 90.0) && close(d[1], 90.0), "{d:?}");
    assert!(close(d[2], 85.0) && close(d[3], 85.0) && close(d[4], 85.0), "{d:?}");
}

#[test]
fn a_zero_or_non_finite_direction_finds_nothing() {
    let (_, found) = ask(
        |w| w.spawn(post(vec2(100.0, 0.0), 10.0)),
        |p| {
            let mut all = vec![];
            let mut any = Vec::new();
            for dir in [Vec2::ZERO, vec2(f32::NAN, 0.0), vec2(f32::INFINITY, 0.0)] {
                all.push(p.cast_ray(Vec2::ZERO, dir, 1000.0, NO_FILTER).is_some());
                all.push(p.cast_circle(Vec2::ZERO, 5.0, dir, 1000.0, NO_FILTER).is_some());
                all.push(p.cast_box(Vec2::ZERO, Vec2::ONE, 0.0, dir, 1000.0, NO_FILTER).is_some());
                all.push(p.cast_capsule(Vec2::ZERO, 4.0, 2.0, dir, 1000.0, NO_FILTER).is_some());
                p.cast_ray_all(Vec2::ZERO, dir, 1000.0, NO_FILTER, &mut any);
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
        |w| w.spawn(post(vec2(100.0, 0.0), 10.0)),
        |p| {
            let mut bad = vec![];
            let mut any = Vec::new();
            for max in [-1.0, f32::NAN] {
                bad.push(p.cast_ray(Vec2::ZERO, Vec2::X, max, NO_FILTER).is_some());
                bad.push(p.cast_circle(Vec2::ZERO, 5.0, Vec2::X, max, NO_FILTER).is_some());
                bad.push(p.closest_point(Vec2::ZERO, max, NO_FILTER).is_some());
                p.cast_ray_all(Vec2::ZERO, Vec2::X, max, NO_FILTER, &mut any);
                bad.push(!any.is_empty());
            }
            let infinite = [
                p.cast_ray(Vec2::ZERO, Vec2::X, f32::INFINITY, NO_FILTER)
                    .map(|h| h.distance),
                p.cast_circle(Vec2::ZERO, 5.0, Vec2::X, f32::INFINITY, NO_FILTER)
                    .map(|h| h.distance),
                p.closest_point(Vec2::ZERO, f32::INFINITY, NO_FILTER)
                    .map(|h| h.distance),
            ];
            (bad, infinite)
        },
    );
    assert!(bad.iter().all(|f| !f), "{bad:?}");
    assert!(close(infinite[0].unwrap(), 90.0));
    assert!(close(infinite[1].unwrap(), 85.0));
    assert!(close(infinite[2].unwrap(), 90.0));
}

#[test]
fn a_non_finite_origin_or_rotation_finds_nothing() {
    let (_, found) = ask(
        |w| w.spawn(wall(Vec2::ZERO, vec2(1000.0, 1000.0))),
        |p| {
            let nan = vec2(f32::NAN, 0.0);
            let mut out = Vec::new();
            let mut pushes = Vec::new();
            let mut hits = Vec::new();
            let mut found = vec![
                p.cast_ray(nan, Vec2::X, 10.0, NO_FILTER).is_some(),
                p.cast_circle(nan, 1.0, Vec2::X, 10.0, NO_FILTER).is_some(),
                p.cast_box(Vec2::ZERO, Vec2::ONE, f32::NAN, Vec2::X, 10.0, NO_FILTER)
                    .is_some(),
                p.closest_point(nan, 10.0, NO_FILTER).is_some(),
            ];
            p.cast_ray_all(nan, Vec2::X, 10.0, NO_FILTER, &mut hits);
            found.push(!hits.is_empty());
            p.overlap_point(nan, NO_FILTER, &mut out);
            found.push(!out.is_empty());
            p.overlap_circle(nan, 1.0, NO_FILTER, &mut out);
            found.push(!out.is_empty());
            p.overlap_box(Vec2::ZERO, Vec2::ONE, f32::NAN, NO_FILTER, &mut out);
            found.push(!out.is_empty());
            p.penetrations(Shape::Circle { radius: 1.0 }, nan, 0.0, NO_FILTER, &mut pushes);
            found.push(!pushes.is_empty());
            found
        },
    );
    assert!(found.iter().all(|f| !f), "{found:?}");
}

#[test]
fn a_ray_starting_inside_hits_at_distance_zero_with_a_zero_normal() {
    let (wall_e, (hit, all)) = ask(
        |w| w.spawn(wall(Vec2::ZERO, vec2(100.0, 100.0))),
        |p| {
            let origin = vec2(10.0, 5.0);
            let mut all = Vec::new();
            p.cast_ray_all(origin, Vec2::X, 1000.0, NO_FILTER, &mut all);
            (p.cast_ray(origin, Vec2::X, 1000.0, NO_FILTER), all)
        },
    );
    let hit = hit.expect("hits the wall it starts in");
    assert_eq!(hit.entity, wall_e);
    assert_eq!(hit.distance, 0.0);
    assert!(near(hit.point, vec2(10.0, 5.0)));
    assert_eq!(hit.normal, Vec2::ZERO);
    assert_eq!(all.len(), 1);
    assert_eq!((all[0].distance, all[0].normal), (0.0, Vec2::ZERO));
}

#[test]
fn a_shape_cast_starting_inside_hits_at_distance_zero_with_a_push_out_normal() {
    let (wall_e, hits) = ask(
        // A wall whose left face is at x = 0.
        |w| w.spawn(wall(vec2(50.0, 0.0), vec2(100.0, 400.0))),
        |p| {
            // Circle of radius 10 centered 4 px left of the face: 6 px in.
            let at = vec2(-4.0, 0.0);
            [
                p.cast_circle(at, 10.0, Vec2::X, 100.0, NO_FILTER),
                p.cast_box(at, vec2(20.0, 20.0), 0.0, Vec2::X, 100.0, NO_FILTER),
                p.cast_capsule(at, 20.0, 10.0, Vec2::X, 100.0, NO_FILTER),
                // Moving away from the wall still reports the overlap.
                p.cast_circle(at, 10.0, Vec2::NEG_X, 100.0, NO_FILTER),
            ]
        },
    );
    for hit in hits {
        let hit = hit.expect("starts overlapping");
        assert_eq!(hit.entity, wall_e);
        assert_eq!(hit.distance, 0.0);
        assert!(near(hit.normal, vec2(-1.0, 0.0)), "{:?}", hit.normal);
        assert!(close(hit.point.x, 0.0), "{:?}", hit.point);
    }
}

#[test]
fn shape_casts_report_where_the_shape_first_touches() {
    let (_, hits) = ask(
        |w| {
            w.spawn(post(vec2(100.0, 0.0), 10.0));
            // Ground whose top is at y = 100 (y is down).
            w.spawn(wall(vec2(0.0, 110.0), vec2(400.0, 20.0)))
        },
        |p| {
            [
                p.cast_circle(Vec2::ZERO, 10.0, Vec2::X, 1000.0, NO_FILTER),
                p.cast_box(Vec2::ZERO, vec2(20.0, 20.0), 0.0, Vec2::Y, 1000.0, NO_FILTER),
                // Turned 45°: its corner reaches √2 × 10 down.
                p.cast_box(
                    Vec2::ZERO,
                    vec2(20.0, 20.0),
                    std::f32::consts::FRAC_PI_4,
                    Vec2::Y,
                    1000.0,
                    NO_FILTER,
                ),
                // Height 20 between the end centers, radius 5: 15 below the center.
                p.cast_capsule(Vec2::ZERO, 20.0, 5.0, Vec2::Y, 1000.0, NO_FILTER),
            ]
        },
    );
    let [circle, square, diamond, capsule] = hits.map(|h| h.expect("hits"));
    assert!(close(circle.distance, 80.0), "{}", circle.distance);
    assert!(near(circle.point, vec2(90.0, 0.0)), "{:?}", circle.point);
    assert!(near(circle.normal, vec2(-1.0, 0.0)));
    assert!(close(square.distance, 90.0), "{}", square.distance);
    assert!(near(square.normal, vec2(0.0, -1.0)));
    assert!(
        (diamond.distance - (100.0 - 200f32.sqrt())).abs() < 1e-2,
        "{}",
        diamond.distance
    );
    assert!(close(capsule.distance, 85.0), "{}", capsule.distance);
    assert!(close(capsule.point.y, 100.0));
}

#[test]
fn overlap_box_and_capsule_find_what_they_cover() {
    let (targets, (plain, turned, capsule, short)) = ask(
        |w| [w.spawn(post(vec2(0.0, 40.0), 5.0)), w.spawn(post(vec2(40.0, 0.0), 5.0))],
        |p| {
            let mut plain = Vec::new();
            let mut turned = Vec::new();
            let mut capsule = Vec::new();
            let mut short = Vec::new();
            // A long thin box along x reaches the right post only...
            p.overlap_box(Vec2::ZERO, vec2(80.0, 4.0), 0.0, NO_FILTER, &mut plain);
            // ...and turned a quarter, the lower one only.
            p.overlap_box(
                Vec2::ZERO,
                vec2(80.0, 4.0),
                std::f32::consts::FRAC_PI_2,
                NO_FILTER,
                &mut turned,
            );
            // An upright capsule 70 tall (60 + 2 × 5) reaches the lower post.
            p.overlap_capsule(Vec2::ZERO, 60.0, 5.0, NO_FILTER, &mut capsule);
            p.overlap_capsule(Vec2::ZERO, 20.0, 5.0, NO_FILTER, &mut short);
            (plain, turned, capsule, short)
        },
    );
    assert_eq!(plain, [targets[1]]);
    assert_eq!(turned, [targets[0]]);
    assert_eq!(capsule, [targets[0]]);
    assert!(short.is_empty());
}

#[test]
fn closest_point_finds_the_nearest_surface_and_inside_points_are_their_own() {
    let (posts, (outside, inside, far)) = ask(
        |w| {
            [
                w.spawn(post(vec2(100.0, 0.0), 10.0)),
                w.spawn(post(vec2(-200.0, 0.0), 10.0)),
            ]
        },
        |p| {
            (
                p.closest_point(Vec2::ZERO, 1000.0, NO_FILTER),
                p.closest_point(vec2(103.0, 0.0), 1000.0, NO_FILTER),
                p.closest_point(Vec2::ZERO, 50.0, NO_FILTER),
            )
        },
    );
    let outside = outside.expect("finds the near post");
    assert_eq!(outside.entity, posts[0]);
    assert!(near(outside.point, vec2(90.0, 0.0)));
    assert!(close(outside.distance, 90.0));
    assert!(!outside.inside);
    let inside = inside.expect("inside the post");
    assert_eq!((inside.entity, inside.distance, inside.inside), (posts[0], 0.0, true));
    assert!(near(inside.point, vec2(103.0, 0.0)));
    assert!(far.is_none(), "nothing within 50");
}

#[test]
fn penetrations_report_depth_and_way_out_deepest_first() {
    let (walls, pushes) = ask(
        |w| {
            [
                // Left face at x = 0.
                w.spawn(wall(vec2(50.0, 0.0), vec2(100.0, 400.0))),
                // Top face at y = 0.
                w.spawn(wall(vec2(0.0, 50.0), vec2(400.0, 100.0))),
            ]
        },
        |p| {
            let mut pushes = Vec::new();
            // Radius 10 centered at (-7, -2): 3 px into the first wall, 8 px
            // into the second.
            p.penetrations(
                Shape::Circle { radius: 10.0 },
                vec2(-7.0, -2.0),
                0.0,
                NO_FILTER,
                &mut pushes,
            );
            pushes
        },
    );
    assert_eq!(pushes.len(), 2);
    assert_eq!(pushes[0].entity, walls[1], "the deeper one first");
    assert!(close(pushes[0].depth, 8.0), "{}", pushes[0].depth);
    assert!(near(pushes[0].normal, vec2(0.0, -1.0)), "{:?}", pushes[0].normal);
    assert!(close(pushes[0].point.y, 0.0));
    assert_eq!(pushes[1].entity, walls[0]);
    assert!(close(pushes[1].depth, 3.0), "{}", pushes[1].depth);
    assert!(near(pushes[1].normal, vec2(-1.0, 0.0)));
}

#[test]
fn moving_by_the_deepest_penetration_frees_the_shape() {
    let shape = Collider::capsule(20.0, 6.0).shape;
    let (_, (before, just_out, clear)) = ask(
        |w| w.spawn(wall(vec2(50.0, 0.0), vec2(100.0, 400.0))),
        move |p| {
            let mut pushes = Vec::new();
            let start = vec2(2.0, 3.0);
            p.penetrations(shape, start, 0.3, NO_FILTER, &mut pushes);
            let first = pushes[0];
            let before = (pushes.len(), first.depth);
            // Moved by exactly the depth (plus rounding), it only touches.
            let out = |extra: f32| start + first.normal * (first.depth + extra);
            p.penetrations(shape, out(0.01), 0.3, NO_FILTER, &mut pushes);
            let just_out = pushes.first().map(|p| p.depth);
            p.penetrations(shape, out(0.5), 0.3, NO_FILTER, &mut pushes);
            (before, just_out, pushes.len())
        },
    );
    assert_eq!(before.0, 1);
    assert!(before.1 > 5.0, "{before:?}");
    assert!(just_out.is_none_or(|d| d < 1e-3), "{just_out:?}");
    assert_eq!(clear, 0);
}

#[test]
fn penetrations_work_for_every_shape() {
    let polygon = Collider::polygon(&[vec2(-10.0, -10.0), vec2(10.0, -10.0), vec2(0.0, 10.0)]).shape;
    let (_, depths) = ask(
        |w| w.spawn(wall(vec2(50.0, 0.0), vec2(100.0, 400.0))),
        move |p| {
            let mut pushes = Vec::new();
            [
                Shape::Circle { radius: 10.0 },
                Shape::Rect { size: vec2(20.0, 20.0) },
                Shape::Capsule {
                    height: 10.0,
                    radius: 10.0,
                },
                polygon,
            ]
            .map(|shape| {
                p.penetrations(shape, vec2(-6.0, 0.0), 0.0, NO_FILTER, &mut pushes);
                pushes.first().map(|p| p.depth)
            })
        },
    );
    assert!(depths.iter().all(|d| d.is_some_and(|d| close(d, 4.0))), "{depths:?}");
}

#[test]
fn sensors_are_seen_only_with_sensors_on() {
    let (sensor, (without, with)) = ask(
        |w| {
            w.spawn((
                Transform::at(vec2(100.0, 0.0)),
                RigidBody::fixed(),
                Collider::circle(10.0).sensor(),
            ))
        },
        |p| {
            let run = |filter: QueryFilter| {
                let mut out = Vec::new();
                let mut hits = Vec::new();
                let mut pushes = Vec::new();
                let mut seen = vec![
                    p.cast_ray(Vec2::ZERO, Vec2::X, 1000.0, filter).map(|h| h.entity),
                    p.cast_circle(Vec2::ZERO, 5.0, Vec2::X, 1000.0, filter)
                        .map(|h| h.entity),
                    p.cast_box(Vec2::ZERO, Vec2::ONE, 0.0, Vec2::X, 1000.0, filter)
                        .map(|h| h.entity),
                    p.cast_capsule(Vec2::ZERO, 4.0, 2.0, Vec2::X, 1000.0, filter)
                        .map(|h| h.entity),
                    p.closest_point(Vec2::ZERO, 1000.0, filter).map(|h| h.entity),
                ];
                p.cast_ray_all(Vec2::ZERO, Vec2::X, 1000.0, filter, &mut hits);
                seen.push(hits.first().map(|h| h.entity));
                p.overlap_point(vec2(100.0, 0.0), filter, &mut out);
                seen.push(out.first().copied());
                p.overlap_circle(vec2(100.0, 0.0), 1.0, filter, &mut out);
                seen.push(out.first().copied());
                p.overlap_box(vec2(100.0, 0.0), Vec2::ONE, 0.0, filter, &mut out);
                seen.push(out.first().copied());
                p.overlap_capsule(vec2(100.0, 0.0), 2.0, 1.0, filter, &mut out);
                seen.push(out.first().copied());
                p.penetrations(
                    Shape::Circle { radius: 2.0 },
                    vec2(100.0, 0.0),
                    0.0,
                    filter,
                    &mut pushes,
                );
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
        |w| {
            let player = w.spawn((
                Transform::at(vec2(100.0, 0.0)),
                RigidBody::fixed(),
                Collider::circle(10.0).groups(CollisionGroups::new(PLAYER, u32::MAX)),
            ));
            let ground = w.spawn((
                Transform::at(vec2(200.0, 0.0)),
                RigidBody::fixed(),
                Collider::circle(10.0).groups(CollisionGroups::new(TERRAIN, u32::MAX)),
            ));
            [player, ground]
        },
        |p| {
            let ask = |groups| {
                let filter = NO_FILTER.groups(groups);
                let mut hits = Vec::new();
                let mut out = Vec::new();
                p.cast_ray_all(Vec2::ZERO, Vec2::X, 1000.0, filter, &mut hits);
                p.overlap_circle(vec2(150.0, 0.0), 60.0, filter, &mut out);
                out.sort_unstable_by_key(|e| e.to_bits());
                (
                    p.cast_ray(Vec2::ZERO, Vec2::X, 1000.0, filter).map(|h| h.entity),
                    hits.iter().map(|h| h.entity).collect::<Vec<_>>(),
                    out,
                )
            };
            (
                ask(CollisionGroups::new(u32::MAX, TERRAIN)),
                ask(CollisionGroups::new(ENEMY, PLAYER)),
                ask(CollisionGroups::ALL),
            )
        },
    );
    let [player, ground] = spawned;
    assert_eq!(terrain_only, (Some(ground), vec![ground], vec![ground]));
    assert_eq!(players_only, (Some(player), vec![player], vec![player]));
    let mut both = vec![player, ground];
    both.sort_unstable_by_key(|e| e.to_bits());
    assert_eq!(all, (Some(player), vec![player, ground], both));
}

#[test]
fn a_collider_seen_by_no_query_group_is_skipped_both_ways() {
    // The query filter must accept the collider and the collider must accept
    // the query, as between two colliders.
    let (_, hit) = ask(
        |w| {
            w.spawn((
                Transform::at(vec2(100.0, 0.0)),
                RigidBody::fixed(),
                // Touches only players.
                Collider::circle(10.0).groups(CollisionGroups::new(TERRAIN, PLAYER)),
            ))
        },
        |p| {
            p.cast_ray(
                Vec2::ZERO,
                Vec2::X,
                1000.0,
                NO_FILTER.groups(CollisionGroups::new(ENEMY, u32::MAX)),
            )
        },
    );
    assert!(hit.is_none());
}

#[test]
fn colliders_in_groups_that_dont_touch_pass_through_each_other() {
    let mut bodies = None;
    let mut world = Game::new("groups", 1, 1)
        .physics(vec2(0.0, 980.0))
        .run_headless(120, |ctx| {
            let ghost = ctx.world.spawn((
                Transform::at(vec2(0.0, 0.0)),
                RigidBody::dynamic(),
                Collider::circle(10.0).groups(CollisionGroups::new(ENEMY, PLAYER)),
            ));
            let solid = ctx.world.spawn((
                Transform::at(vec2(50.0, 0.0)),
                RigidBody::dynamic(),
                Collider::circle(10.0),
            ));
            // The ground is terrain: the enemy (touching only players) skips it.
            ctx.world.spawn((
                Transform::at(vec2(0.0, 100.0)),
                RigidBody::fixed(),
                Collider::rect(vec2(400.0, 20.0)).groups(CollisionGroups::new(TERRAIN, u32::MAX)),
            ));
            bodies = Some((ghost, solid));
        });
    let (ghost, solid) = bodies.unwrap();
    let y = |w: &mut World, e| w.get::<Transform>(e).unwrap().position.y;
    assert!(y(&mut world, ghost) > 200.0, "the enemy falls through the ground");
    assert!((y(&mut world, solid) - 80.0).abs() < 1.5, "the other ball rests on it");
}

#[test]
fn exclude_looks_through_one_entity_in_every_query() {
    let (posts, seen) = ask(
        // Two posts in a row, overlapping where the overlaps look.
        |w| {
            [
                w.spawn(post(vec2(100.0, 0.0), 10.0)),
                w.spawn(post(vec2(105.0, 0.0), 10.0)),
            ]
        },
        |p| {
            // Excludes the first entity spawned (index 0), the near post.
            let filter = NO_FILTER.exclude(Entity::from_bits(
                p.cast_ray(Vec2::ZERO, Vec2::X, 1000.0, NO_FILTER)
                    .unwrap()
                    .entity
                    .to_bits(),
            ));
            let mut out = Vec::new();
            let mut hits = Vec::new();
            let mut pushes = Vec::new();
            let mut seen = vec![
                p.cast_ray(Vec2::ZERO, Vec2::X, 1000.0, filter).map(|h| h.entity),
                p.cast_circle(Vec2::ZERO, 2.0, Vec2::X, 1000.0, filter)
                    .map(|h| h.entity),
                p.cast_box(Vec2::ZERO, Vec2::ONE, 0.0, Vec2::X, 1000.0, filter)
                    .map(|h| h.entity),
                p.cast_capsule(Vec2::ZERO, 2.0, 1.0, Vec2::X, 1000.0, filter)
                    .map(|h| h.entity),
                p.closest_point(Vec2::ZERO, 1000.0, filter).map(|h| h.entity),
            ];
            p.cast_ray_all(Vec2::ZERO, Vec2::X, 1000.0, filter, &mut hits);
            seen.push((hits.len() == 1).then(|| hits[0].entity));
            p.overlap_point(vec2(102.0, 0.0), filter, &mut out);
            seen.push((out.len() == 1).then(|| out[0]));
            p.overlap_circle(vec2(102.0, 0.0), 1.0, filter, &mut out);
            seen.push((out.len() == 1).then(|| out[0]));
            p.overlap_box(vec2(102.0, 0.0), Vec2::ONE, 0.0, filter, &mut out);
            seen.push((out.len() == 1).then(|| out[0]));
            p.overlap_capsule(vec2(102.0, 0.0), 2.0, 1.0, filter, &mut out);
            seen.push((out.len() == 1).then(|| out[0]));
            p.penetrations(
                Shape::Circle { radius: 1.0 },
                vec2(102.0, 0.0),
                0.0,
                filter,
                &mut pushes,
            );
            seen.push((pushes.len() == 1).then(|| pushes[0].entity));
            seen
        },
    );
    assert!(seen.iter().all(|e| *e == Some(posts[1])), "{seen:?} {posts:?}");
}

/// Every query, asked of the same scene, as `(name, numbers)`: distances,
/// normals' x, entity ids, flags. The 2D and 3D sides must agree.
#[cfg(feature = "physics3d")]
type Answers = Vec<(&'static str, Vec<f32>)>;

/// The 2D answers of [`the_rules_match_3d`].
#[cfg(feature = "physics3d")]
fn answers_2d(p: &Physics) -> Answers {
    let f = NO_FILTER;
    let ids = |es: &[Entity]| {
        let mut ids: Vec<f32> = es.iter().map(|e| e.to_bits() as f32).collect();
        ids.sort_unstable_by(f32::total_cmp);
        ids
    };
    let hit = |h: Option<Hit>| h.map_or(vec![-1.0], |h| vec![h.distance, h.normal.x, h.entity.to_bits() as f32]);
    let mut hits = Vec::new();
    let mut out = Vec::new();
    let mut pushes = Vec::new();
    let mut a: Answers = vec![];
    p.cast_ray_all(Vec2::ZERO, vec2(3.0, 0.0), 1000.0, f, &mut hits);
    a.push((
        "cast_ray_all",
        hits.iter()
            .flat_map(|h| [h.distance, h.normal.x, h.entity.to_bits() as f32])
            .collect(),
    ));
    let left = vec2(-200.0, 0.0);
    a.push(("cast_ray", hit(p.cast_ray(left, Vec2::X, 1000.0, f))));
    a.push(("cast_ray infinite", hit(p.cast_ray(left, Vec2::X, f32::INFINITY, f))));
    a.push(("cast_ray negative range", hit(p.cast_ray(left, Vec2::X, -1.0, f))));
    a.push(("cast_ray zero direction", hit(p.cast_ray(left, Vec2::ZERO, 1000.0, f))));
    let nan_direction = Vec2::splat(f32::NAN);
    a.push((
        "cast_ray nan direction",
        hit(p.cast_ray(left, nan_direction, 1000.0, f)),
    ));
    a.push((
        "ball cast nan direction",
        hit(p.cast_circle(left, 10.0, nan_direction, 1000.0, f)),
    ));
    a.push((
        "cast_ray nan origin",
        hit(p.cast_ray(vec2(f32::NAN, 0.0), Vec2::X, 1000.0, f)),
    ));
    a.push(("cast_ray inside", hit(p.cast_ray(vec2(10.0, 0.0), Vec2::X, 1000.0, f))));
    let inside = vec2(-46.0, 0.0);
    a.push((
        "ball cast inside, out",
        hit(p.cast_circle(inside, 10.0, Vec2::NEG_X, 100.0, f)),
    ));
    a.push((
        "ball cast inside, in",
        hit(p.cast_circle(inside, 10.0, Vec2::X, 100.0, f)),
    ));
    a.push(("ball cast", hit(p.cast_circle(left, 10.0, Vec2::X, 1000.0, f))));
    a.push((
        "box cast",
        hit(p.cast_box(left, vec2(20.0, 20.0), 0.0, Vec2::X, 1000.0, f)),
    ));
    a.push((
        "box cast turned",
        hit(p.cast_box(left, vec2(20.0, 20.0), FRAC_PI_4, Vec2::X, 1000.0, f)),
    ));
    a.push((
        "box cast inside",
        hit(p.cast_box(inside, vec2(20.0, 20.0), 0.0, Vec2::X, 100.0, f)),
    ));
    a.push((
        "box cast nan turn",
        hit(p.cast_box(left, Vec2::ONE, f32::NAN, Vec2::X, 1000.0, f)),
    ));
    a.push(("capsule cast", hit(p.cast_capsule(left, 20.0, 5.0, Vec2::X, 1000.0, f))));
    a.push((
        "capsule cast inside",
        hit(p.cast_capsule(inside, 20.0, 10.0, Vec2::X, 100.0, f)),
    ));
    for (name, at) in [
        ("overlap_point posts", vec2(200.0, 0.0)),
        ("overlap_point wall", vec2(10.0, 0.0)),
        ("overlap_point none", vec2(120.0, 0.0)),
    ] {
        p.overlap_point(at, f, &mut out);
        a.push((name, ids(&out)));
    }
    p.overlap_circle(vec2(150.0, 0.0), 45.0, f, &mut out);
    a.push(("ball overlap", ids(&out)));
    p.overlap_box(vec2(130.0, 0.0), vec2(140.0, 4.0), 0.0, f, &mut out);
    a.push(("box overlap", ids(&out)));
    p.overlap_box(vec2(130.0, 0.0), vec2(140.0, 4.0), FRAC_PI_2, f, &mut out);
    a.push(("box overlap turned", ids(&out)));
    p.overlap_box(vec2(130.0, 0.0), vec2(140.0, 4.0), f32::NAN, f, &mut out);
    a.push(("box overlap nan turn", ids(&out)));
    p.overlap_capsule(vec2(125.0, 0.0), 0.0, 70.0, f, &mut out);
    a.push(("capsule overlap", ids(&out)));
    let near = |h: Option<ponko::d2::PointHit>| {
        h.map_or(vec![-1.0], |h| {
            vec![
                h.distance,
                h.point.x,
                f32::from(u8::from(h.inside)),
                h.entity.to_bits() as f32,
            ]
        })
    };
    a.push(("closest_point", near(p.closest_point(left, 1000.0, f))));
    a.push((
        "closest_point inside",
        near(p.closest_point(vec2(10.0, 0.0), 1000.0, f)),
    ));
    a.push(("closest_point out of range", near(p.closest_point(left, 100.0, f))));
    a.push(("closest_point nan range", near(p.closest_point(left, f32::NAN, f))));
    let push = |pushes: &Vec<ponko::d2::Penetration>| {
        pushes
            .iter()
            .flat_map(|p| [p.depth, p.normal.x, p.entity.to_bits() as f32])
            .collect::<Vec<f32>>()
    };
    p.penetrations(Shape::Circle { radius: 10.0 }, vec2(-55.0, 0.0), 0.0, f, &mut pushes);
    a.push(("penetrations ball", push(&pushes)));
    p.penetrations(
        Shape::Rect { size: vec2(20.0, 20.0) },
        vec2(-55.0, 0.0),
        0.0,
        f,
        &mut pushes,
    );
    a.push(("penetrations box", push(&pushes)));
    p.penetrations(Shape::Circle { radius: 1.0 }, vec2(200.0, 0.0), 0.0, f, &mut pushes);
    a.push(("penetrations tie", push(&pushes)));
    p.penetrations(
        Shape::Circle { radius: 1.0 },
        vec2(200.0, 0.0),
        f32::NAN,
        f,
        &mut pushes,
    );
    a.push(("penetrations nan turn", push(&pushes)));
    a
}

/// The 3D answers of [`the_rules_match_3d`]: the same questions, in the
/// plane z = 0.
#[cfg(feature = "physics3d")]
fn answers_3d(p: &ponko::d3::Physics) -> Answers {
    use ponko::d3::{Hit as Hit3, Penetration as Penetration3, PointHit as PointHit3, Shape as Shape3};
    let f = ponko::d3::QueryFilter::default();
    let q = Quat::IDENTITY;
    let turn = Quat::from_rotation_z;
    let ids = |es: &[Entity]| {
        let mut ids: Vec<f32> = es.iter().map(|e| e.to_bits() as f32).collect();
        ids.sort_unstable_by(f32::total_cmp);
        ids
    };
    let hit = |h: Option<Hit3>| h.map_or(vec![-1.0], |h| vec![h.distance, h.normal.x, h.entity.to_bits() as f32]);
    let mut hits = Vec::new();
    let mut out = Vec::new();
    let mut pushes = Vec::new();
    let mut a: Answers = vec![];
    p.cast_ray_all(Vec3::ZERO, vec3(3.0, 0.0, 0.0), 1000.0, f, &mut hits);
    a.push((
        "cast_ray_all",
        hits.iter()
            .flat_map(|h| [h.distance, h.normal.x, h.entity.to_bits() as f32])
            .collect(),
    ));
    let left = vec3(-200.0, 0.0, 0.0);
    a.push(("cast_ray", hit(p.cast_ray(left, Vec3::X, 1000.0, f))));
    a.push(("cast_ray infinite", hit(p.cast_ray(left, Vec3::X, f32::INFINITY, f))));
    a.push(("cast_ray negative range", hit(p.cast_ray(left, Vec3::X, -1.0, f))));
    a.push(("cast_ray zero direction", hit(p.cast_ray(left, Vec3::ZERO, 1000.0, f))));
    let nan_direction = Vec3::splat(f32::NAN);
    a.push((
        "cast_ray nan direction",
        hit(p.cast_ray(left, nan_direction, 1000.0, f)),
    ));
    a.push((
        "ball cast nan direction",
        hit(p.cast_sphere(left, 10.0, nan_direction, 1000.0, f)),
    ));
    a.push((
        "cast_ray nan origin",
        hit(p.cast_ray(vec3(f32::NAN, 0.0, 0.0), Vec3::X, 1000.0, f)),
    ));
    a.push((
        "cast_ray inside",
        hit(p.cast_ray(vec3(10.0, 0.0, 0.0), Vec3::X, 1000.0, f)),
    ));
    let inside = vec3(-46.0, 0.0, 0.0);
    a.push((
        "ball cast inside, out",
        hit(p.cast_sphere(inside, 10.0, Vec3::NEG_X, 100.0, f)),
    ));
    a.push((
        "ball cast inside, in",
        hit(p.cast_sphere(inside, 10.0, Vec3::X, 100.0, f)),
    ));
    a.push(("ball cast", hit(p.cast_sphere(left, 10.0, Vec3::X, 1000.0, f))));
    let cube = Vec3::splat(20.0);
    a.push(("box cast", hit(p.cast_box(left, cube, q, Vec3::X, 1000.0, f))));
    a.push((
        "box cast turned",
        hit(p.cast_box(left, cube, turn(FRAC_PI_4), Vec3::X, 1000.0, f)),
    ));
    a.push(("box cast inside", hit(p.cast_box(inside, cube, q, Vec3::X, 100.0, f))));
    let nan_turn = Quat::from_xyzw(0.0, 0.0, f32::NAN, 1.0);
    a.push((
        "box cast nan turn",
        hit(p.cast_box(left, Vec3::ONE, nan_turn, Vec3::X, 1000.0, f)),
    ));
    a.push(("capsule cast", hit(p.cast_capsule(left, 20.0, 5.0, Vec3::X, 1000.0, f))));
    a.push((
        "capsule cast inside",
        hit(p.cast_capsule(inside, 20.0, 10.0, Vec3::X, 100.0, f)),
    ));
    for (name, at) in [
        ("overlap_point posts", vec3(200.0, 0.0, 0.0)),
        ("overlap_point wall", vec3(10.0, 0.0, 0.0)),
        ("overlap_point none", vec3(120.0, 0.0, 0.0)),
    ] {
        p.overlap_point(at, f, &mut out);
        a.push((name, ids(&out)));
    }
    p.overlap_sphere(vec3(150.0, 0.0, 0.0), 45.0, f, &mut out);
    a.push(("ball overlap", ids(&out)));
    let bar = vec3(140.0, 4.0, 4.0);
    p.overlap_box(vec3(130.0, 0.0, 0.0), bar, q, f, &mut out);
    a.push(("box overlap", ids(&out)));
    p.overlap_box(vec3(130.0, 0.0, 0.0), bar, turn(FRAC_PI_2), f, &mut out);
    a.push(("box overlap turned", ids(&out)));
    p.overlap_box(vec3(130.0, 0.0, 0.0), bar, nan_turn, f, &mut out);
    a.push(("box overlap nan turn", ids(&out)));
    p.overlap_capsule(vec3(125.0, 0.0, 0.0), 0.0, 70.0, f, &mut out);
    a.push(("capsule overlap", ids(&out)));
    let near = |h: Option<PointHit3>| {
        h.map_or(vec![-1.0], |h| {
            vec![
                h.distance,
                h.point.x,
                f32::from(u8::from(h.inside)),
                h.entity.to_bits() as f32,
            ]
        })
    };
    a.push(("closest_point", near(p.closest_point(left, 1000.0, f))));
    a.push((
        "closest_point inside",
        near(p.closest_point(vec3(10.0, 0.0, 0.0), 1000.0, f)),
    ));
    a.push(("closest_point out of range", near(p.closest_point(left, 100.0, f))));
    a.push(("closest_point nan range", near(p.closest_point(left, f32::NAN, f))));
    let push = |pushes: &Vec<Penetration3>| {
        pushes
            .iter()
            .flat_map(|p| [p.depth, p.normal.x, p.entity.to_bits() as f32])
            .collect::<Vec<f32>>()
    };
    p.penetrations(
        Shape3::Sphere { radius: 10.0 },
        vec3(-55.0, 0.0, 0.0),
        q,
        f,
        &mut pushes,
    );
    a.push(("penetrations ball", push(&pushes)));
    p.penetrations(Shape3::Cuboid { size: cube }, vec3(-55.0, 0.0, 0.0), q, f, &mut pushes);
    a.push(("penetrations box", push(&pushes)));
    p.penetrations(Shape3::Sphere { radius: 1.0 }, vec3(200.0, 0.0, 0.0), q, f, &mut pushes);
    a.push(("penetrations tie", push(&pushes)));
    p.penetrations(
        Shape3::Sphere { radius: 1.0 },
        vec3(200.0, 0.0, 0.0),
        nan_turn,
        f,
        &mut pushes,
    );
    a.push(("penetrations nan turn", push(&pushes)));
    a
}

/// The same scene in 2D and 3D gives the same answers under each rule, for
/// every query: each one exists in both dimensions (a circle in 2D is a
/// sphere in 3D).
#[cfg(feature = "physics3d")]
#[test]
fn the_rules_match_3d() {
    use ponko::d3;
    // 2D: a box 100 wide at the origin, two posts at x = 200 in the same spot.
    let (_, two) = ask(
        |w| {
            w.spawn(wall(Vec2::ZERO, vec2(100.0, 100.0)));
            w.spawn(post(vec2(200.0, 0.0), 10.0));
            w.spawn(post(vec2(200.0, 0.0), 10.0));
        },
        answers_2d,
    );
    // 3D: the same, as a box and spheres.
    let answer = Rc::new(RefCell::new(None));
    let slot = answer.clone();
    Game::new("queries", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1 {
                *slot.borrow_mut() = Some(answers_3d(&ctx.physics3d));
            }
        })
        .run_headless(2, |ctx| {
            let body = |at: Vec3, collider| (d3::Transform::at(at), d3::RigidBody::fixed(), collider);
            ctx.world
                .spawn(body(Vec3::ZERO, d3::Collider::cuboid(Vec3::splat(100.0))));
            ctx.world.spawn(body(vec3(200.0, 0.0, 0.0), d3::Collider::sphere(10.0)));
            ctx.world.spawn(body(vec3(200.0, 0.0, 0.0), d3::Collider::sphere(10.0)));
        });
    let three = answer.borrow_mut().take().unwrap();
    assert_eq!(two.len(), three.len());
    for ((name, a), (name3, b)) in two.iter().zip(&three) {
        assert_eq!(name, name3);
        let same = a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
        assert!(same, "{name}: 2D {a:?}, 3D {b:?}");
    }
    // And the answers are the rules', not merely the same mistake twice.
    let get = |name: &str| &two.iter().find(|(n, _)| *n == name).unwrap().1;
    let starts =
        |got: &[f32], want: &[f32]| got.len() >= want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-3);
    let all = get("cast_ray_all");
    assert_eq!(all.len(), 9);
    assert_eq!((all[0], all[1]), (0.0, 0.0), "starts inside: distance 0, zero normal");
    assert!(all[5] < all[8], "ties by entity id");
    assert!(starts(get("cast_ray"), &[150.0, -1.0]));
    assert!(starts(get("cast_ray infinite"), &[150.0, -1.0]));
    for name in [
        "cast_ray negative range",
        "cast_ray zero direction",
        "cast_ray nan direction",
        "ball cast nan direction",
        "cast_ray nan origin",
        "box cast nan turn",
        "closest_point out of range",
        "closest_point nan range",
    ] {
        assert_eq!(get(name), &[-1.0], "{name} finds nothing");
    }
    assert!(starts(get("cast_ray inside"), &[0.0, 0.0]));
    for name in [
        "ball cast inside, out",
        "ball cast inside, in",
        "box cast inside",
        "capsule cast inside",
    ] {
        assert!(starts(get(name), &[0.0, -1.0]), "{name}: distance 0, pushed out");
    }
    assert!(starts(get("box cast"), &[140.0]));
    assert!((get("box cast turned")[0] - (150.0 - 200f32.sqrt())).abs() < 1e-2);
    assert!(starts(get("capsule cast"), &[145.0]));
    assert_eq!(get("overlap_point posts").len(), 2);
    assert_eq!(get("overlap_point wall").len(), 1);
    assert!(get("overlap_point none").is_empty());
    assert_eq!(get("ball overlap").len(), 2);
    assert_eq!(get("box overlap").len(), 2);
    assert!(get("box overlap turned").is_empty());
    assert!(get("box overlap nan turn").is_empty());
    assert_eq!(get("capsule overlap").len(), 2);
    assert!(starts(get("closest_point"), &[150.0, -50.0, 0.0]));
    assert!(starts(get("closest_point inside"), &[0.0, 10.0, 1.0]));
    assert!(starts(get("penetrations ball"), &[5.0, -1.0]));
    assert!(starts(get("penetrations box"), &[5.0, -1.0]));
    let tie = get("penetrations tie");
    assert!(
        tie.len() == 6 && tie[0] == tie[3] && tie[2] < tie[5],
        "equal depths by entity id: {tie:?}"
    );
    assert!(get("penetrations nan turn").is_empty());
}

fn polygon() -> Collider {
    Collider::polygon(&[
        vec2(-10.0, -10.0),
        vec2(10.0, -10.0),
        vec2(10.0, 10.0),
        vec2(-10.0, 10.0),
    ])
}

#[test]
fn shape_casts_starting_inside_every_collider_hit_at_zero_in_both_directions() {
    for collider in [
        Collider::circle(10.0),
        Collider::rect(Vec2::splat(20.0)),
        Collider::capsule(10.0, 10.0),
        polygon(),
    ] {
        let (entity, hits) = ask(
            move |w| w.spawn((Transform::default(), RigidBody::fixed(), collider)),
            |p| {
                let mut hits = Vec::new();
                for direction in [Vec2::X, Vec2::NEG_X] {
                    for range in [0.0, 100.0] {
                        let origin = vec2(-9.0, 0.0);
                        hits.push(p.cast_circle(origin, 2.0, direction, range, NO_FILTER));
                        hits.push(p.cast_box(origin, Vec2::splat(4.0), 0.2, direction, range, NO_FILTER));
                        hits.push(p.cast_capsule(origin, 4.0, 2.0, direction, range, NO_FILTER));
                    }
                }
                hits
            },
        );
        for (i, hit) in hits.into_iter().enumerate() {
            let hit = hit.unwrap_or_else(|| panic!("{collider:?}, cast {i} missed"));
            assert_eq!(hit.entity, entity);
            assert_eq!(hit.distance, 0.0, "{collider:?}, cast {i}");
            assert!(hit.point.is_finite() && hit.normal.is_finite());
            assert!(hit.normal.x < -0.8, "{collider:?}, cast {i}: {hit:?}");
        }
    }
}

#[test]
fn nonfinite_directions_and_rotations_find_nothing_and_clear_outputs() {
    ask(
        |w| w.spawn(wall(Vec2::ZERO, Vec2::splat(20.0))),
        |p| {
            let mut hits = Vec::new();
            for direction in [vec2(f32::NAN, 0.0), vec2(0.0, f32::NAN), Vec2::splat(f32::INFINITY)] {
                assert!(p.cast_ray(Vec2::ZERO, direction, 100.0, NO_FILTER).is_none());
                p.cast_ray_all(Vec2::ZERO, Vec2::X, 100.0, NO_FILTER, &mut hits);
                assert!(!hits.is_empty());
                p.cast_ray_all(Vec2::ZERO, direction, 100.0, NO_FILTER, &mut hits);
                assert!(hits.is_empty());
                assert!(p.cast_circle(Vec2::ZERO, 2.0, direction, 100.0, NO_FILTER).is_none());
                assert!(
                    p.cast_box(Vec2::ZERO, Vec2::ONE, 0.0, direction, 100.0, NO_FILTER)
                        .is_none()
                );
                assert!(
                    p.cast_capsule(Vec2::ZERO, 2.0, 1.0, direction, 100.0, NO_FILTER)
                        .is_none()
                );
            }
            let mut out = Vec::new();
            let mut pushes = Vec::new();
            for rotation in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                assert!(
                    p.cast_box(Vec2::ZERO, Vec2::ONE, rotation, Vec2::X, 100.0, NO_FILTER)
                        .is_none()
                );
                p.overlap_box(Vec2::ZERO, Vec2::ONE, 0.0, NO_FILTER, &mut out);
                assert!(!out.is_empty());
                p.overlap_box(Vec2::ZERO, Vec2::ONE, rotation, NO_FILTER, &mut out);
                assert!(out.is_empty());
                p.penetrations(Collider::rect(Vec2::ONE).shape, Vec2::ZERO, 0.0, NO_FILTER, &mut pushes);
                assert!(!pushes.is_empty());
                p.penetrations(
                    Collider::rect(Vec2::ONE).shape,
                    Vec2::ZERO,
                    rotation,
                    NO_FILTER,
                    &mut pushes,
                );
                assert!(pushes.is_empty());
            }
        },
    );
}

#[test]
fn queries_with_reused_buffers_dont_allocate() {
    let (_, counts) = ask(
        |w| {
            for i in 0..50 {
                let collider = match i % 4 {
                    0 => Collider::circle(10.0),
                    1 => Collider::rect(Vec2::splat(20.0)),
                    2 => Collider::capsule(10.0, 10.0),
                    _ => polygon(),
                };
                w.spawn((Transform::at(vec2(30.0 * i as f32, 0.0)), RigidBody::fixed(), collider));
            }
        },
        |p| {
            let mut hits = Vec::with_capacity(64);
            let mut out = Vec::with_capacity(64);
            let mut pushes = Vec::with_capacity(64);
            let mut run = || {
                p.cast_ray_all(vec2(-50.0, 0.0), Vec2::X, 10_000.0, NO_FILTER, &mut hits);
                let _ = p.cast_ray(vec2(-50.0, 0.0), Vec2::X, 10_000.0, NO_FILTER);
                let _ = p.cast_circle(vec2(-50.0, 0.0), 5.0, Vec2::X, 10_000.0, NO_FILTER);
                let _ = p.cast_box(vec2(-50.0, 0.0), Vec2::ONE, 0.3, Vec2::X, 10_000.0, NO_FILTER);
                let _ = p.cast_capsule(vec2(-50.0, 0.0), 4.0, 2.0, Vec2::X, 10_000.0, NO_FILTER);
                let _ = p.closest_point(vec2(15.0, 30.0), 100.0, NO_FILTER);
                p.overlap_point(vec2(30.0, 0.0), NO_FILTER, &mut out);
                p.overlap_circle(vec2(300.0, 0.0), 100.0, NO_FILTER, &mut out);
                p.overlap_box(vec2(300.0, 0.0), vec2(200.0, 20.0), 0.2, NO_FILTER, &mut out);
                p.overlap_capsule(vec2(300.0, 0.0), 20.0, 10.0, NO_FILTER, &mut out);
                p.penetrations(
                    Shape::Rect { size: vec2(100.0, 8.0) },
                    vec2(300.0, 0.0),
                    0.0,
                    NO_FILTER,
                    &mut pushes,
                );
                for shape in [Collider::circle(8.0).shape, Collider::capsule(10.0, 8.0).shape] {
                    p.penetrations(shape, vec2(328.0, 0.0), 0.2, NO_FILTER, &mut pushes);
                    assert!(!pushes.is_empty());
                }
                hits.len() + out.len() + pushes.len()
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
fn colliders_saved_without_groups_load_with_every_group() {
    let c: Collider = serde_json::from_str(r#"{"shape":{"Circle":{"radius":4.0}}}"#).unwrap();
    assert_eq!(c.groups, CollisionGroups::ALL);
    assert_eq!(Collider::circle(1.0).groups, CollisionGroups::ALL);
    let back: Collider = serde_json::from_str(
        &serde_json::to_string(&Collider::circle(1.0).groups(CollisionGroups::new(1, 2))).unwrap(),
    )
    .unwrap();
    assert_eq!(back.groups, CollisionGroups::new(1, 2));
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
