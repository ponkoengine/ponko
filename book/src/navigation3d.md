# 3D navigation meshes

Enable `navmesh` for pure Rust Recast baking and Detour pathfinding. It implies
`3d`, but needs no window, GPU, physics, Bevy or async runtime. Servers can build
with `--no-default-features --features navmesh`. The existing 2D `NavGrid` and
`FlowField` are independent.

Gather static geometry as world-space `NavSource` triangle soups, or use
`NavSource::from_mesh` with a `MeshData` and `d3::Transform`. Include solid walls,
ceilings and platforms, not only floors. Floors face up with counter-clockwise
winding. Sources are copied: later mesh edits require another bake.

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/navmesh3d.rs:bake}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:nav_bake}}
```

</div>
</div>

Settings use world units, and `max_slope` uses degrees. Recast voxelizes triangles,
filters low ceilings and ledges, erodes for the agent radius, partitions regions,
and builds tiled polygons with detail heights. Radius and height round up to
cells; climb rounds down. Smaller cells improve accuracy and cost more baking
work and memory. Bake uses bounded backend budgets and returns `NavBakeError`
on invalid inputs or resource exhaustion. A scene with no walkable surface may
produce an empty mesh.

### Large levels

The level is baked as a grid of square tiles, `tile_size` cells (default 32,
6.4 m at the default cell size) on a side, and a navmesh holds at most 16,384
tiles. When a level would need more, the baker grows `tile_size` for you, up to
255 cells, so kilometre-wide terrain bakes with the default settings: 0.8 km
square at the default cell size keeps 32-cell tiles, and the grown tiles reach
about 6.5 km. Beyond that, raise `cell_size`; the error says which limit was hit
and which setting to change. Bake time grows with the number of cells, about a
second for a 500 m square at the defaults, so use a coarser `cell_size` and
`cell_height` (for example 0.5 and 0.25) for the largest levels. Smaller tiles
make carving cheaper, which is why the default is not larger.

Sample explicitly when accepting a distant click or spawn position. Sampling
uses Euclidean distance, including vertical distance. `find_path` accepts endpoints
close to the surface (two cells horizontally and climb plus two cell heights
vertically) and returns string-pulled world-space corners. Disconnected or truncated
routes return `None`, never a partial success. Queries currently cap the search and
corridor at 8192 nodes/polygons; an oversized route can therefore return `None`.
Agent route preparation also caps surface samples at one million; an oversized
route leaves the agent stopped.

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/navmesh3d.rs:query}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:nav_query}}
```

</div>
</div>

Register `systems::step_navmesh3d` with `Game::sim_system` (C++: `game::navmesh3d()`), like an exposed physics step.
There is no automatic navigation update. Insert a `d3::NavMesh` resource, then spawn
entities with `d3::Transform` and `d3::NavAgent`. This works in `run_headless` and obeys
the simulation clock. Put destination/obstacle systems before the navigation step.
With physics in the game, [who runs when](systems.md#physics-navigation-characters-and-interpolation-who-runs-when)
says where the step goes; `systems::step_navmesh3d_access()` is its access declaration.

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/navmesh3d.rs:agents}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:nav_agents}}
{{#include ../../examples/cpp/guide.cpp:nav_agent_spawn}}
```

</div>
</div>

The follower accelerates up to its speed and follows a surface-sampled path.
Its transform represents its feet. It has no gravity, physical collision response,
turning inertia. Crowd avoidance is opt-in (see below). Do not also attach a physics body or character
controller that writes the same transform. Use a separate presentation offset if
your model origin is above its feet. Bake for the largest agent radius and height;
components with radius larger than the bake radius stop.

`d3::NavObstacle` is a world-space obstacle component. The sim system gathers
all obstacles each tick. Changed, moved or removed obstacles invalidate cached paths, as
does replacing the mesh resource, changing a destination or teleporting an agent.
Unreachable agents wait until one of these changes. An agent trapped inside a
newly blocked polygon stops; it is not automatically pushed out.

Dynamic obstacles **block entire overlapping polygons**, tested against the
obstacle's real footprint grown by the agent radius, and vertically from the
agent height below it to its top. This is conservative: a small obstacle may
close a much larger polygon, especially with large tiles. Opt-in carving instead
cuts new polygon edges around the exact shape from cached heightfield layers.
Non-carving changes scan all polygons; no spatial obstacle
index is maintained. Use geometry in the bake for permanent walls and holes.
For manual queries without the sim system, call `set_obstacles`; when registered,
the system owns the obstacle list and replaces it from ECS components.

With avoidance disabled, no path queries or heap allocations occur in steady agent movement; replans build
paths and sample their heights. Baking and obstacle scans are synchronous, so
schedule large bakes outside latency-sensitive ticks. Ordered source geometry,
settings and updates are deterministic; the backend enables `libm` for consistent
math. This does not establish cross-platform determinism for the rest of a game.

The backend is `navmesh-rs`, a Rust port of Recast/Detour. It avoids a native C++
build and keeps generation and 3D queries together. Landmass would still require
a baker, Polyanya requires extra 3D surface handling, and oxidized_navigation is
a Bevy plugin. The C API and C++ wrapper expose baking, queries, agents, avoidance and obstacles.


### Obstacle shapes

An obstacle is an upright prism `center.y ± half_extents.y` tall. Its
`shape` (`d3::NavObstacleShape`) picks the footprint and `yaw` turns it
around world Y, in radians, the way `Quat::from_rotation_y(yaw)` turns a
model:

| Shape | Constructor | Footprint |
|---|---|---|
| `Box` (default) | `NavObstacle::new(center, half_extents)` | `half_extents.x` by `half_extents.z`, turned by `yaw` |
| `Cylinder` | `NavObstacle::cylinder(center, radius, half_height)` | radius `half_extents.x` along the obstacle's X and `half_extents.z` along its Z |
| `Convex(polygon)` | `NavObstacle::convex(center, points, half_height)` | a convex polygon of 3 to 8 `(x, z)` offsets from `center` (`d3::NavConvexPolygon`), turned by `yaw` |

```rust
# use ponko::prelude::*;
# fn add(ctx: &mut Ctx) {
let mut crate_box = ponko::d3::NavObstacle::new(vec3(4.0, 0.5, 0.0), vec3(1.0, 0.5, 0.5));
crate_box.yaw = 0.6;
let pillar = ponko::d3::NavObstacle::cylinder(vec3(-3.0, 1.5, 2.0), 0.4, 1.5);
let ramp = [vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(0.0, 1.5)];
// None when the points are not a convex polygon.
let wedge = ponko::d3::NavObstacle::convex(vec3(0.0, 0.5, 5.0), &ramp, 0.5).unwrap();
ctx.world.spawn((crate_box,));
ctx.world.spawn((pillar,));
ctx.world.spawn((wedge,));
# }
```

Blocking tests polygon bounds against the box or polygon itself, and against
a 16-sided polygon drawn around a cylinder; carving cuts the cells whose centres
are within the agent radius (plus half a cell) of the exact shape. An obstacle
with a nonfinite center, size or `yaw`, or a negative size, fails closed: it
blocks the whole mesh (a carving one stops navigation until it is corrected).
Shapes and turns are Rust-only for now: the C and C++ obstacle calls make
unturned boxes, as before.

## Crowd avoidance

Set `d3::NavAgent::avoidance` to opt into the backend's DetourCrowd manager.
The default is false so existing path following and arrival behaviour stay intact.
Only opted-in agents participate, including agents whose destination is `None`.
Do not mix opted-out followers into a crowd and expect them to collide.

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
let mut agent = ponko::d3::NavAgent::default();
agent.avoidance = true;
agent.avoidance_weight = 2.0;
agent.neighbour_distance = 3.6;
agent.destination = Some(Vec3::X * 5.0);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:nav_avoidance}}
```

</div>
</div>

Crowd agents retain radius, maximum speed and acceleration. The nonnegative
`avoidance_weight` controls separation steering, rather than right of way.
`neighbour_distance` must be positive; choose it to cover approaching traffic.
The C equivalent is `ponko_ctx_set_nav_avoidance(ctx, entity, enabled, weight,
neighbour_distance)`. Invalid C parameters return an error; invalid Rust agent
parameters stop that agent and remove it from the crowd. Crowd radii must be
positive and no larger than the bake radius.

The backend uses a proximity grid, keeps the six nearest neighbours and samples
velocities against their predicted motion and local navmesh walls, with a 2.5 s
time horizon. Nearest-neighbour and sampling fixes are enabled. An internal 30%
radius margin reduces penetration from its iterative overlap correction. Movement
uses connected surface corridors, including height, rather than projecting onto a
globally nearest floor. Ticks are subdivided toward 60 Hz, up to 64 substeps.

This is approximate local avoidance, not a collision solver or a guarantee of
progress in arbitrarily dense traffic. Large time steps, excessive speeds, initial
overlaps, narrow passages and opposing bottlenecks can cause penetration or
stalling. Stopped agents can be displaced by overlap correction. Several agents
sharing one destination settle in a region around it: nonzero discs cannot all
arrive at one exact point. Give agents distinct reachable targets when precise
arrival is required. Moving obstacles invalidate crowd corridors. Carving is available as described above;
there is no predictive obstacle velocity.

Slots and corridors persist across ticks. Capacity grows in powers of two up to
16383 agents; excess agents wait. Mesh or obstacle changes rebuild the crowd,
and teleports recreate the affected slot. The backend currently allocates query
scratch during updates; avoid assuming allocation-free crowd simulation. The
original follower retains its allocation-free steady path. Baking, full-route
validation on a new target, and crowd updates are synchronous and headless.


## Dynamic carving

Set `carve = true` to cut real holes of the obstacle's exact shape, rebuilt with
the backend's tile-cache layer pipeline (regions, contours, polygons). The default remains
conservative polygon blocking. Carving needs the heightfield layers, which cost
bake time and memory, so bake with `NavBakeSettings::carving` set to `true`
(`carving` in the C struct). Without it, a `carve` obstacle blocks like an
ordinary one. A carving bake builds the mesh, detail heights included, from the
layers alone, so it costs about the same as a normal bake. Tile size is configured in bake settings (8–255
cells, default 32). Static heightfield layers are retained using the raw codec;
updates never rasterize source triangles again.

<div class="lang-pair">

```rust,no_run
# use ponko::prelude::*;
# fn add(ctx: &mut Ctx) {
// Bake with layers: ponko::nav::NavBakeSettings { carving: true, ..Default::default() }.
let mut obstacle = ponko::d3::NavObstacle::new(vec3(6.0, 1.0, 6.0), Vec3::ONE);
obstacle.carve = true;
ctx.world.spawn((obstacle,));
// step_navmesh3d gathers components and rebuilds one cached layer per tick.
let nav = ctx.world.resource_mut::<ponko::d3::NavMesh>();
let steps = nav.update_carving(0).unwrap();
let pending = nav.carving_pending();
# let _ = (steps, pending);
# }
```

```cpp
{{#include ../../examples/cpp/guide.cpp:nav_carve}}
```

</div>

For manual Rust meshes, `set_obstacles` queues changes and `update_carving(budget)`
advances at most that many layer rebuilds. C/C++ components are gathered by the
simulation before extra manual updates can advance them. A zero budget only
queries pending work. Changes arriving during rebuilds are retained. Movement
under half a cell from the last applied position does not rebuild; size changes
do, and so does a turn that moves the footprint's far edge half a cell. Ordered
obstacle lists supply identity, so reordering can cause extra work.

Only layers touched by changed obstacles are rebuilt; moving or turning an
obstacle touches its old and new bounds. Every changed obstacle is taken at once
and its layers queued in order. All agent paths and
crowd corridors are invalidated after tile updates, conservatively including
agents whose paths do not cross those tiles. Paths see each tile as it completes,
so carving is eventually consistent rather than atomic across tiles.

Simulation stops navigation on an invalid carving obstacle until it is
corrected; a manual update reports the error. Agents already enclosed by a hole
stop rather than being ejected. A budget unit is one layer rebuild; an edit that
touches no layer costs nothing. The simulation uses one unit per tick. Rebuilds
allocate scratch and refresh polygon bounds across the mesh; the budget bounds
tile count, not wall-clock duration. A carving bake keeps every tile's
heightfield layers in memory. A carve obstacle that fully
cuts a corridor leaves the two sides unconnected: `find_path` returns `None` and
agents wait until the hole is removed.

### Detail heights and rebuild cost

Every tile built from the layers, at bake time and after each carve, gets a
detail mesh: the floor heights inside each polygon are sampled from the layer
the way an ordinary bake samples its heightfield (every six cells, within one
cell height), so agents follow uneven ground next to a carved hole as closely
as on an ordinary bake instead of walking the flat plane between polygon
corners. On a 2 m hill, an ordinary bake and a carved one both stay within
about 0.25 m of the ground; the same polygons without detail heights miss it
by up to 2 m.

One layer rebuild (marking the carving obstacles, regions, contours, polygons
and detail heights, swapping the tile and refreshing polygon flags) took
about 0.1 ms per 32-cell tile on a 100 m level, flat or bumpy (median of 200;
`cargo run --release -p ponko_bench -- nav3d/carve`, Apple M4 Max with other
builds running, p95 up to 0.4 ms). Each rebuild runs under the bake's per-tile
memory and work budget.
