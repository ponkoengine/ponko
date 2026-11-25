# Spatial queries

> **Maturity: [tested](maturity.md).** Game: none (the Legions showcase uses
> it). Tests: `tests/spatial.rs`. Platforms: Linux, macOS and Windows,
> headless (the workspace tests in the list of checks). Graphics: not
> involved. Missing: a reference game that uses it.

"Who is near me?" is the core question of most simulations. Checking every
pair is O(n²); a **spatial grid** answers it by looking only at nearby cells.

## The built-in grid

Turn it on with a cell size near your usual query radius:

```rust
# fn flock(_: &mut Ctx) {}
Game::new("Boids", 1280, 720)
    .spatial_grid(24.0)
    .sim_system(flock);
```

At the start of every tick, every entity with a `d2::Transform` is indexed into
`ctx.spatial`. Query it from any sim system:

<div class="lang-pair">
<div>

```rust
#[derive(Component)]
struct Neighbors(usize);

fn count_neighbors(ctx: &mut Ctx) {
    let grid = &ctx.spatial;
    ctx.world
        .query::<(Entity, &ponko::d2::Transform, &mut Neighbors)>()
        .par_for_each(|(me, t, n)| {
            n.0 = 0;
            grid.for_each_in_radius(t.position, 10.0, |o| {
                if o.entity != me { n.0 += 1; }
            });
        });
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:spatial}}
```

</div>
</div>

The grid is read-only while you query the world, so this runs on all cores.
In C++, `ctx.spatial_view()` gives a view that walks the grid inline (no
call into the engine per query); `ctx.spatial_radius(center, r, hits)`
fills a vector instead, which is simpler but slower in hot loops.

| Method | Visits |
|---|---|
| `for_each_in_radius(center, r, f)` | entries within `r` of `center` |
| `for_each_in_rect(min, max, f)` | entries inside the rectangle |
| `count_in_radius(center, r)` | how many entries are within `r` |
| `nearest(center, max_r, accept)` | the closest entry `accept` agrees to |

Each visit gives a `GridEntry` with `entity` and `position`.

### The nearest one

`nearest` starts at the cell `center` is in and goes outward ring by ring,
and stops once no farther ring can hold anything closer than what it found.
Its cost follows how far the nearest match is, not the radius, so a long
reach (a tower that shoots the closest enemy within 300 m) costs about what
a short one does:

```rust
# #[derive(Component)]
# struct Enemy;
fn closest_enemy(ctx: &Ctx, tower: Vec2) -> Option<Entity> {
    ctx.spatial
        .nearest(tower, 300.0, |e| ctx.world.get::<Enemy>(e.entity).is_some())
        .map(|e| e.entity)
}
```

`accept` is asked about each entry at most once, and only about entries
closer than the best so far. Of two entries at the same distance, the one
with the lower entity id wins, so the answer doesn't depend on the order
the grid was filled in. With `f32::INFINITY` as the radius it searches the
whole grid; when the rings would visit more cells than there are entries,
it looks at the remaining entries directly.

Measured with `cargo run --release -p ponko_bench -- nearest/` (100,000
entities in a 2,000 × 2,000 square, cell size 16, 10,000 searches; Apple M4
Max on an idle machine, 2026-11-22, revision `9235ea05`, load average
under 2.8, the median of three runs): 1.53 ms against 3.45 ms for visiting
every cell in a 32 radius, and 1.46 ms against 77 ms in a 256 radius. The
runs agreed within 10% (the scans) and 8% (the rings).

Every 32 ticks (`SpatialGrid::SORT_EVERY`) the built-in system also reorders
entities by grid cell (`World::sort_by` with `spatial_key`), so neighbors in
space are neighbors in memory. With a million boids this cut a tick from
62 ms to 41 ms.

## Your own grid, with data

The built-in grid stores only entity and position. When neighbors need more
data, for example a boid's velocity, keep your own `SpatialGrid<T>` inside
the system and fill it with `rebuild_with_data`:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/boids.rs:flocking}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/boids.cpp:cpp_flocking}}
```

</div>
</div>

C++ uses a `ponko::grid`, whose items carry a `uint32_t` of your own (here
an index into a velocity array), and walks it with `grid->view()`.

## Choosing a cell size

- About your query radius is a good start: a radius query then reads a 3×3
  block of cells.
- Much smaller cells mean more cells to visit; much larger means more
  entries per cell to reject.
- Queries much larger than the world fall back to a plain scan, so huge
  radii stay correct and don't get slow.
