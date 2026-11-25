# Pathfinding with flow fields

> **Maturity: [tested](maturity.md).** Game: none (the Legions showcase uses
> it). Tests: `tests/flow.rs`. Platforms: Linux, macOS and Windows, headless
> (the workspace tests in the list of checks). Graphics: not involved.
> Missing: a reference game that uses it.

When thousands of units head for the same place (a rally point, the
player, the nearest food), searching a path for each of them is wasteful.
A **flow field** does one search from the goal outward and stores, for
every cell of the map, which way to step. Each unit then just looks up the
cell it stands on: one array read, the same cost for 10 units as for a
million.

Two types do this:

- `NavGrid`: the map. A cost per cell: `1` is normal ground, up to `255` is
  slower (mud, shallow water), `NavGrid::WALL` (`0`) can't be entered.
- `FlowField`: the answer. `compute(&nav, &goals)` fills it; then
  `direction_at(pos)` gives a unit vector toward the nearest goal and
  `distance_at(pos)` how far away it is.

## The map

<div class="lang-pair">
<div>

```rust
let mut nav = ponko::flow::NavGrid::new(100, 80, 4.0); // 100 × 80 cells, 4 world units each
for y in 0..60 {
    nav.set_cost(40, y, ponko::flow::NavGrid::WALL);
}
for x in 50..70 {
    for y in 20..40 {
        nav.set_cost(x, y, 6); // mud: six times slower
    }
}
let mut flow = ponko::flow::FlowField::new();
flow.compute(&nav, &[vec2(390.0, 10.0)]); // or several goals: the nearest wins
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:flow_map}}

{{#include ../../examples/cpp/guide.cpp:flow_compute}}
```

</div>
</div>

`nav.origin` (C++: `set_origin`) moves the grid: it is where cell (0, 0)'s
top-left corner is in the world. `nav.cell_at(pos)`, `nav.center_of(cell)`
and `nav.walkable(pos)` convert between world points and cells. To fill a
whole map at once (from a tilemap, an image, a level file), write
`nav.costs_mut()` (C++: `nav.costs()`), one byte per cell, row by row.

Paths move in 8 directions. Diagonal steps cost √2 times a straight one,
and never squeeze between two walls that touch at a corner. Slow ground
counts as longer: a unit walks around a mud pit when the way around is
shorter than wading through.

## Steering

`direction_at` only reads, so any number of threads can use one field at
once. Look it up from a parallel query:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/flow.rs:steer}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:flow_steer}}
```

</div>
</div>

In C++, `flow.view()` gives a `flow_view` whose `direction_at` is inline,
with no call into the engine per lookup. Take a new view after every
`compute`.

The direction is zero at a goal, on a wall, outside the grid and where no
goal can be reached; `distance_at` returns nothing (`None`, C++
`std::nullopt`) in the last case. Moving a unit straight along the
direction can clip a wall corner, so the example slides along walls with
`nav.walkable` instead of stepping onto them.

## When the goal moves

Call `compute` again: the field reuses its memory, so recomputing every
time the player clicks or a target moves is fine. For many different
destinations (each squad to its own target), keep one `FlowField` per
destination, not one per unit.

Measured in release on a MacBook Pro (M4 Max) (`cargo run --release -p ponko_bench -- flow`):

| What | Time |
|---|---|
| `compute`, 256 × 256 cells | 1.25 ms |
| `compute`, 1024 × 1024 cells | 13.0 ms |
| 1 million `direction_at` lookups, parallel | 0.29 ms |

The `flow` example (`cargo run --release --example flow`) has 20 000 units
finding their way around walls and through mud to wherever you click; `H`
shows the distance field. Headless with a million units
(`HEADLESS=1 AGENTS=1000000 cargo run --release --example flow`) a tick
takes 3.2 ms, steering and all.

For a whole game built on flow fields, see the `battle` example: two
armies, one field each, rerouted when you click a new rally point
(`cargo run --release --example battle`).
