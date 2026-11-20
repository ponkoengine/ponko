//! Determinism helpers: `SpatialGrid::nearest` (ring by ring) against the
//! scan of every cell in the radius it replaced, and `world_hash` against
//! hashing the world's JSON save.
use std::hash::{Hash, Hasher};
use std::hint::black_box;

use ponko::prelude::*;
use ponko::reflect::world_hash;
use ponko::spatial::SpatialGrid;
use serde::{Deserialize, Serialize};

use crate::Bench;

/// Entities in the grid and in the hashed world.
const ENTITIES: usize = 100_000;
/// Nearest searches per timed run.
const QUERIES: usize = 10_000;
/// The square the entities stand in, and the grid's cell size.
const SIDE: f32 = 2_000.0;
const CELL: f32 = 16.0;

/// A deterministic point in the square.
fn point(i: usize) -> Vec2 {
    let mut s = (i as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    s ^= s >> 29;
    s = s.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let x = (s >> 40) as f32 / (1u64 << 24) as f32;
    let y = ((s >> 16) & 0xFF_FFFF) as f32 / (1u64 << 24) as f32;
    vec2(x, y) * SIDE
}

fn grid() -> (SpatialGrid, Vec<Vec2>) {
    let mut world = World::new();
    let items: Vec<(Entity, Vec2)> = (0..ENTITIES)
        .map(|i| (world.spawn((Pos(point(i)),)), point(i)))
        .collect();
    let mut grid = SpatialGrid::new(CELL);
    grid.rebuild_from(items);
    let centers = (0..QUERIES).map(|i| point(i + ENTITIES)).collect();
    (grid, centers)
}

/// What `nearest` did before: visit every cell in the radius, keep the closest.
fn scan_nearest(grid: &SpatialGrid, center: Vec2, radius: f32) -> Option<Entity> {
    let mut best = (f32::INFINITY, None);
    grid.for_each_in_radius(center, radius, |e| {
        let d = e.position.distance_squared(center);
        if d < best.0 {
            best = (d, Some(e.entity));
        }
    });
    best.1
}

#[derive(Component, Serialize, Deserialize)]
struct Pos(Vec2);
#[derive(Component, Serialize, Deserialize)]
struct Vel(Vec2);
#[derive(Component, Serialize, Deserialize)]
struct Unit {
    hp: f32,
    team: u8,
    cooldown: u32,
}

fn reflected_world() -> World {
    let mut world = World::new();
    world.reflect::<Pos>();
    world.reflect::<Vel>();
    world.reflect::<Unit>();
    for i in 0..ENTITIES {
        world.spawn((
            Pos(point(i)),
            Vel(point(i + 1) / SIDE),
            Unit {
                hp: 10.0,
                team: (i % 2) as u8,
                cooldown: i as u32 % 30,
            },
        ));
    }
    world
}

pub fn run(b: &mut Bench) {
    for radius in [32.0, 256.0] {
        let r = radius as u32;
        b.steady(&format!("nearest/rings-r{r}-10k"), 20, grid, |(grid, centers)| {
            for &c in centers.iter() {
                black_box(grid.nearest(c, radius, |_| true));
            }
        });
        b.steady(&format!("nearest/radius-scan-r{r}-10k"), 20, grid, |(grid, centers)| {
            for &c in centers.iter() {
                black_box(scan_nearest(grid, c, radius));
            }
        });
    }
    b.steady("world_hash/100k-entities", 20, reflected_world, |world| {
        black_box(world_hash(world));
    });
    b.steady("world_hash/json-save-100k-entities", 5, reflected_world, |world| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        world.save().to_string().hash(&mut h);
        black_box(h.finish());
    });
}
