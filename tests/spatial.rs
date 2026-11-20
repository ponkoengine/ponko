//! The spatial grid: every query is checked against brute force.

#[allow(unused_imports)]
use ponko::d2::{Sprite, Transform as Transform2D};
use ponko::ecs_core::World;
use ponko::prelude::*;
#[allow(unused_imports)]
use ponko::spatial::SpatialGrid;

/// Deterministic random points in `[-span, span)²`.
fn points(n: usize, span: f32, seed: u64) -> Vec<(Entity, Vec2)> {
    let mut world = World::new();
    let mut s = seed;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        ((s >> 40) as f32 / (1u64 << 24) as f32) * 2.0 * span - span
    };
    (0..n)
        .map(|_| {
            let p = vec2(next(), next());
            (world.spawn((Transform2D::at(p),)), p)
        })
        .collect()
}

fn sorted(mut v: Vec<Entity>) -> Vec<Entity> {
    v.sort();
    v
}

fn grid(cell: f32, pts: &[(Entity, Vec2)]) -> SpatialGrid {
    let mut g = SpatialGrid::new(cell);
    g.rebuild_from(pts.iter().copied());
    g
}

#[test]
fn radius_matches_brute_force() {
    let pts = points(5000, 1000.0, 1);
    for cell in [7.0, 50.0, 400.0] {
        let g = grid(cell, &pts);
        assert_eq!(g.len(), pts.len());
        for (center, r) in [
            (vec2(0.0, 0.0), 30.0),
            (vec2(-917.0, 455.0), 120.0),
            (vec2(3.3, -3.3), 0.0),
            (vec2(10.0, 10.0), 5000.0),
        ] {
            let mut got = Vec::new();
            g.for_each_in_radius(center, r, |e| got.push(e.entity));
            let want: Vec<_> = pts
                .iter()
                .filter(|(_, p)| p.distance_squared(center) <= r * r)
                .map(|(e, _)| *e)
                .collect();
            assert_eq!(sorted(got), sorted(want), "cell {cell}, center {center}, r {r}");
            assert_eq!(
                g.count_in_radius(center, r),
                pts.iter().filter(|(_, p)| p.distance(center) <= r).count()
            );
        }
    }
}

#[test]
fn rect_matches_brute_force() {
    let pts = points(3000, 500.0, 2);
    let g = grid(25.0, &pts);
    for (min, max) in [
        (vec2(-100.0, -50.0), vec2(80.0, 300.0)),
        (vec2(-500.0, -500.0), vec2(500.0, 500.0)),
        (vec2(1.0, 1.0), vec2(0.0, 0.0)),
    ] {
        let mut got = Vec::new();
        g.for_each_in_rect(min, max, |e| got.push(e.entity));
        let want: Vec<_> = pts
            .iter()
            .filter(|(_, p)| p.cmpge(min).all() && p.cmple(max).all())
            .map(|(e, _)| *e)
            .collect();
        assert_eq!(sorted(got), sorted(want), "{min}..{max}");
    }
}

#[test]
fn nearest_matches_brute_force_and_respects_accept() {
    let pts = points(2000, 300.0, 3);
    let g = grid(20.0, &pts);
    for center in [vec2(0.0, 0.0), vec2(250.0, -250.0), vec2(-31.0, 77.0)] {
        let best = pts
            .iter()
            .min_by(|a, b| a.1.distance_squared(center).total_cmp(&b.1.distance_squared(center)))
            .unwrap();
        assert_eq!(g.nearest(center, 1000.0, |_| true).map(|e| e.entity), Some(best.0));
        let second = g.nearest(center, 1000.0, |e| e.entity != best.0).unwrap();
        assert_ne!(second.entity, best.0);
        assert!(second.position.distance(center) >= best.1.distance(center));
    }
    assert_eq!(g.nearest(vec2(10_000.0, 10_000.0), 5.0, |_| true), None);
}

/// The nearest of `pts` to `center` within `radius` that `accept` takes;
/// ties go to the lowest entity bits.
fn brute_nearest(pts: &[(Entity, Vec2)], center: Vec2, radius: f32, accept: impl Fn(Entity) -> bool) -> Option<Entity> {
    pts.iter()
        .filter(|(e, p)| p.distance_squared(center) <= radius * radius && accept(*e))
        .min_by(|a, b| {
            a.1.distance_squared(center)
                .total_cmp(&b.1.distance_squared(center))
                .then(a.0.to_bits().cmp(&b.0.to_bits()))
        })
        .map(|(e, _)| *e)
}

#[test]
fn nearest_goes_ring_by_ring_and_still_matches_brute_force() {
    // Dense and sparse grids, cells smaller and larger than the gaps,
    // radii from nothing to everything, centres inside and far outside,
    // and a filter that rejects most entities.
    for (n, span, cell, seed) in [
        (3000, 500.0, 8.0, 1),
        (200, 2000.0, 16.0, 2),
        (5000, 100.0, 50.0, 3),
        (40, 50.0, 1.0, 4),
    ] {
        let pts = points(n, span, seed);
        let g = grid(cell, &pts);
        let mut s = seed * 77 + 5;
        let mut next = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 40) as f32 / (1u64 << 24) as f32
        };
        for _ in 0..300 {
            let center = vec2(next() * 3.0 - 1.5, next() * 3.0 - 1.5) * span;
            let radius = match (next() * 4.0) as u32 {
                0 => next() * cell,
                1 => next() * span * 0.25,
                2 => next() * span * 4.0,
                _ => f32::INFINITY,
            };
            let odd = |e: Entity| e.to_bits() % 7 == 3;
            assert_eq!(
                g.nearest(center, radius, |_| true).map(|e| e.entity),
                brute_nearest(&pts, center, radius, |_| true),
                "{center} r {radius}"
            );
            assert_eq!(
                g.nearest(center, radius, |e| odd(e.entity)).map(|e| e.entity),
                brute_nearest(&pts, center, radius, odd),
                "{center} r {radius}"
            );
        }
    }
}

#[test]
fn nearest_breaks_ties_by_entity_and_asks_accept_once_per_entity() {
    // Four entities at the same distance in four different cells.
    let mut world = World::new();
    let pts: Vec<(Entity, Vec2)> = [vec2(5.0, 0.0), vec2(-5.0, 0.0), vec2(0.0, 5.0), vec2(0.0, -5.0)]
        .into_iter()
        .map(|p| (world.spawn((Transform2D::at(p),)), p))
        .collect();
    let lowest = pts.iter().map(|p| p.0).min_by_key(|e| e.to_bits()).unwrap();
    let g = grid(2.0, &pts);
    assert_eq!(g.nearest(Vec2::ZERO, 100.0, |_| true).map(|e| e.entity), Some(lowest));
    let mut asked = Vec::new();
    let none = g.nearest(Vec2::ZERO, f32::INFINITY, |e| {
        asked.push(e.entity);
        false
    });
    assert_eq!(none, None);
    assert_eq!(sorted(asked), sorted(pts.iter().map(|p| p.0).collect()));
}

#[test]
fn every_entry_is_found_once_despite_hash_collisions() {
    // Few entries → few buckets, spread over many cells: buckets hold several cells.
    let pts = points(40, 10_000.0, 4);
    let g = grid(1.0, &pts);
    for (e, p) in &pts {
        let mut hits = 0;
        g.for_each_in_radius(*p, 0.0, |x| hits += (x.entity == *e) as usize);
        assert_eq!(hits, 1);
    }
}

#[test]
fn rebuild_replaces_old_entries() {
    let a = points(100, 50.0, 5);
    let b = points(10, 50.0, 6);
    let mut g = grid(10.0, &a);
    g.rebuild_from(b.iter().copied());
    assert_eq!(g.len(), 10);
    g.rebuild_from(std::iter::empty());
    assert!(g.is_empty());
    assert_eq!(g.count_in_radius(Vec2::ZERO, 1e9), 0);
}

#[test]
fn rebuild_reads_transforms_from_the_world() {
    let mut world = World::new();
    world.spawn((Transform2D::at(vec2(1.0, 1.0)),));
    world.spawn((Transform2D::at(vec2(100.0, 100.0)),));
    world.spawn((Sprite::new(dummy_texture()),)); // no transform: not indexed
    let mut g = SpatialGrid::new(16.0);
    g.rebuild(&mut world);
    assert_eq!(g.len(), 2);
    assert_eq!(g.count_in_radius(Vec2::ZERO, 5.0), 1);
}

fn dummy_texture() -> Texture {
    let mut tex = None;
    Game::new("t", 1, 1).run_headless(0, |ctx| tex = Some(ctx.gfx.create_texture(1, 1, &[255; 4])));
    tex.unwrap()
}

#[test]
#[should_panic(expected = "cell size must be a positive number")]
fn zero_cell_size_panics() {
    let _ = SpatialGrid::new(0.0);
}

#[derive(Component)]
struct Neighbors(usize);

fn count_neighbors(ctx: &mut Ctx) {
    let grid = &ctx.spatial;
    ctx.world
        .query::<(Entity, &Transform2D, &mut Neighbors)>()
        .par_for_each(|(me, t, n)| {
            n.0 = 0;
            grid.for_each_in_radius(t.position, 10.0, |o| n.0 += (o.entity != me) as usize);
        });
}

fn drift(ctx: &mut Ctx) {
    ctx.world
        .query::<&mut Transform2D>()
        .for_each(|t| t.position.x += 100.0);
}

#[test]
fn game_rebuilds_the_grid_every_tick_before_other_systems() {
    // Two clusters of 3; `drift` moves everyone after counting, so stale
    // positions would break the second tick's counts.
    let mut world = Game::new("t", 1, 1)
        .sim_system(count_neighbors)
        .sim_system(drift)
        .spatial_grid(10.0)
        .run_headless(2, |ctx| {
            for x in [0.0, 1.0, 2.0, 500.0, 501.0, 502.0] {
                ctx.world.spawn((Transform2D::at(vec2(x, 0.0)), Neighbors(0)));
            }
        });
    assert!(world.query::<&Neighbors>().iter().all(|n| n.0 == 2));
}

#[test]
fn without_spatial_grid_it_stays_empty() {
    let mut world = Game::new("t", 1, 1).sim_system(count_neighbors).run_headless(1, |ctx| {
        ctx.world.spawn((Transform2D::default(), Neighbors(99)));
    });
    assert!(world.query::<&Neighbors>().iter().all(|n| n.0 == 0));
}

#[test]
fn rebuild_system_is_removable() {
    let g = Game::new("t", 1, 1)
        .spatial_grid(5.0)
        .remove(Systems::REBUILD_SPATIAL_GRID);
    assert_eq!(g.system_count(Systems::REBUILD_SPATIAL_GRID), 0);
}

#[test]
fn entries_carry_their_data() {
    let pts = points(500, 100.0, 7);
    let mut g = SpatialGrid::with_data(10.0);
    g.rebuild_with_data(pts.iter().enumerate().map(|(i, (e, p))| (*e, *p, i as u32)));
    g.for_each_in_radius(Vec2::ZERO, 1e6, |e| assert_eq!(pts[e.data as usize].0, e.entity));
    assert_eq!(g.len(), 500);
}

#[test]
fn spatial_key_orders_nearby_cells_together() {
    // Same cell → same key; negative cells sort below positive ones.
    assert_eq!(
        ponko::spatial::spatial_key(vec2(1.0, 1.0), 10.0),
        ponko::spatial::spatial_key(vec2(9.0, 9.0), 10.0)
    );
    assert!(ponko::spatial::spatial_key(vec2(-5.0, -5.0), 10.0) < ponko::spatial::spatial_key(vec2(5.0, 5.0), 10.0));
    // Within a 2x2 block of cells, keys are consecutive (Z-order).
    let mut block: Vec<u64> = [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 10.0)]
        .iter()
        .map(|&(x, y)| ponko::spatial::spatial_key(vec2(x, y), 10.0))
        .collect();
    block.sort();
    assert!(block.windows(2).all(|w| w[1] == w[0] + 1), "{block:?}");
}

#[test]
fn built_in_grid_sorts_entities_by_cell() {
    let mut world = Game::new("t", 1, 1).spatial_grid(10.0).run_headless(1, |ctx| {
        for x in [900.0, -50.0, 300.0, 5.0, 610.0] {
            ctx.world.spawn((Transform2D::at(vec2(x, 0.0)),));
        }
    });
    let keys: Vec<u64> = world
        .query::<&Transform2D>()
        .iter()
        .map(|t| ponko::spatial::spatial_key(t.position, 10.0))
        .collect();
    assert!(keys.windows(2).all(|w| w[0] <= w[1]), "{keys:?}");
}
