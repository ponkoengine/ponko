# 3D physics: bodies, terrain, characters and vehicles

> **Maturity: [tested](maturity.md).** Game: Shard Grove (character
> controller), headless only. Tests: `tests/physics3d*.rs`,
> `tests/vehicle3d.rs`, `tests/joints3d.rs`, `tests/ragdoll3d.rs`. Platforms:
> Linux, macOS and Windows, headless (the workspace tests in the list of
> checks). Graphics: not involved. Missing: Shard Grove's package played on
> each OS.

Crates that tumble down a hill, a first-person character that walks up
slopes and stairs, a car that drives over the terrain, rays that find what
the player is looking at: 3D
rigid-body physics on [rapier](https://rapier.rs), designed like the
[2D physics](physics.md). It's behind the `physics3d` feature (which turns
on `3d`):

```toml
ponko = { version = "0.1", features = ["physics3d"] }
```

With `use ponko::d3::*;` the bodies, colliders and queries are `RigidBody`,
`Collider`, `Velocity`, `Physics` and `Hit`, the same names as in
`ponko::d2`; this chapter qualifies them as `d3::RigidBody` and so on.
Saved scenes and reflection name them by their full type names
(`"RigidBody3D"`, `"Collider3D"`, ...) ([Naming and compatibility](naming.md)).

(The C library, `ponko_ffi`, has it on by default.)

Turn it on with gravity in m/s² (+Y is up):

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics3d.rs:setup}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics3d_setup}}
```

</div>
</div>

The physics step is a simulation system in the `Physics`
[phase](systems.md#phases), wherever you call `physics3d`: systems added
with `.add(Phase::BeforePhysics, f)` feed it, `sim_system`s see its result.
It runs 50 times a second unless you chose a rate with `fixed_hz`, and
between steps every body is **drawn blended** between its last two poses,
so motion stays smooth at any frame rate. Simulation systems after the step
see the stepped pose; to read a body's exact physics position from a frame
system, or from a simulation system that runs before the step, use
`ctx.physics3d.position(e)`. [Who runs when](systems.md#physics-navigation-characters-and-interpolation-who-runs-when)
has the whole order. In Rust, `ctx.physics3d.is_body_sleeping(e)` tells you
whether the entity has a dynamic body that is currently asleep. Bodies
join the physics world on the next step.

## Bodies, colliders and terrain

An entity becomes a body with a `d3::Transform`, a `d3::RigidBody` and a
`d3::Collider` (in meters). As in 2D, a body brings a transform and a
`d3::Velocity` the spawn lacks, and a collider without a body brings
`d3::RigidBody::fixed()` ([required components](ecs.md#required-components)).
Terrain gets a **heightfield** made from the same
heights, size and corner as its [terrain tile](3d.md), so what you walk on
is what you see:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics3d.rs:world}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics3d_world}}
```

</div>
</div>

| Rust | C++ | Meaning |
|---|---|---|
| `d3::RigidBody::dynamic()` | `ponko::dynamic_body3d()` | moved by gravity and collisions |
| `d3::RigidBody::fixed()` | `ponko::fixed_body3d()` | never moves: ground, walls, buildings |
| `d3::RigidBody::kinematic()` | `ponko::kinematic_body3d()` | moved only by you; pushes, isn't pushed |
| `d3::Collider::sphere(r)` | `ponko::sphere_collider(r)` | |
| `d3::Collider::cuboid(size)` | `ponko::box_collider(size)` | full width, height, depth |
| `d3::Collider::capsule(h, r)` | `ponko::capsule_collider3d(h, r)` | upright; `h` between the centers of its ends |
| `d3::Collider::mesh(m)` | `ponko::mesh_collider(m)` | a heightfield, convex hull or triangle mesh |
| `d3::Collider::scaled_mesh(m, scale)` | `ponko::scaled_mesh_collider(m, scale)` | that mesh scaled where it is used; a negative component mirrors it |
| `.density(d)`, `.friction(f)`, `.restitution(r)` | fields of the same name | kg/m³ (default 1000), 0 ice..1 rubber, bounciness |
| `.sensor()` | `.sensor = true` | reports overlaps, doesn't push |
| `.offset(v)`, `.groups(g)` | `.offset`, `.groups` | shape moved from the entity; who it touches |
| `d3::RigidBody::dynamic().mass(kg)` | `ponko::with_mass(body, kg)` | a mass of its own instead of density × volume |
| `.center_of_mass(v)` | `ponko::with_center_of_mass(body, v)` | center of mass in the body's frame (low for cars) |

In Rust, use `d3::CollisionMeshData::compound` to build one fixed collider
from several spheres, boxes or capsules. Give each piece a local offset,
rotation and shape. Register the data with `ctx.physics3d.add_mesh` and
attach it with `d3::Collider::mesh`. The pieces share the collider's
surface settings and groups. Give the primitives their final dimensions
when creating the compound. Its runtime handle must be recreated when
loading a world. The collider debug overlay currently draws individual
primitive colliders and regular meshes.

Collision meshes are made once and shared by any number of colliders:

- `ctx.physics3d.heightfield(&heights, size)` (C++
  `ctx.heightfield_collider`): n × n heights, row after row along +Z, NaN
  for a hole you can fall through. Put it on a fixed body at the tile's
  corner.
- `ctx.physics3d.convex_hull(&points)`: the hull around a point cloud, for
  rocks and props that move.
- `ctx.physics3d.trimesh(&vertices, &triangles)`: any triangle mesh, for
  static buildings and level geometry (it has no inside, so give it to
  fixed bodies).
- `ctx.free(m)` (or `ctx.physics3d.free(m)`) lets go of one; colliders
  still using it keep it until they're removed. A collision mesh is a
  [handle](handles.md): `ctx.is_live(m)` tells whether it still is.
- `Collider::scaled_mesh(m, scale)` uses one mesh at any size: a rock made
  once serves a pebble and a boulder, and a negative component mirrors it
  (a heightfield along X and Z; its Y takes the size only, so the ground
  still faces up). The mesh stays
  one; each collider gets a scaled copy when it enters the physics world,
  shared by the colliders at the same scale and dropped with the last of
  them. C: `ponko_collider3d_scaled_mesh(m, scale)` (the shape
  `PONKO_SHAPE3D_SCALED_MESH`, with `size` as the scale). A mesh of a file
  is saved by its file and the collider's scale (below).

Each of these has a twin in `CollisionMeshData` (C++
`ponko::collision_mesh_data`) that builds the mesh on any thread, for
[streamed worlds](#big-worlds-solid-only-near-the-player): build it in the
same `Job` as the rest of a cell, then `ctx.physics3d.add_mesh(&data)` (C++
`ctx.add_collision_mesh`) on the game's thread, which only shares it.

A terrain tile is simplest to make solid with a **`TerrainCollider`** on
the tile's own entity: `TerrainCollider::new(mesh)` (C++
`ponko::solid_terrain(mesh)`) with the heightfield of the tile's heights.
It sits at the tile's `d3::Transform`, turned **and scaled** with it (a tile
drawn twice as high is solid twice as high), so it matches the drawn ground
triangle for triangle, holes included. The scale works as
`Collider::scaled_mesh`'s: a triangle mesh is built again from its scaled
corners, so a piece of ground stretched or mirrored (a negative component)
is solid where it is drawn and its triangles still face up. A heightfield
mirrored along X or Z (a tile flipped to reuse its heights on the other
side of a seam) is built again with its heights, holes and the diagonals
of its cells flipped, so it too is solid exactly where it is drawn. That
copy is made once when the tile enters the physics world (about 0.07 ms for
257 × 257 heights, `cargo run --release -p ponko_bench --
physics3d/heightfield-257`: twenty copies, each row less `shared`,
divided by twenty) and shared by the tiles at one scale; unmirrored tiles
cost what they did.
A negative Y scale takes the size only: the ground still faces up.

A saved world keeps the tile's `d3::Transform`, mirror included, and its
`TerrainCollider`'s surface (`friction`, `restitution`, `groups`), but not
the heightfield: it was made in code and has no file to name. Tell physics
once, in setup, how to make a tile's ground again, and every load
(`ctx.load_world`, `ctx.load_scene`) makes it solid where it is drawn,
before the next step. The rebuild gets the loaded world and the tile, so
the tile's saved components (its transform, or a component of your own
such as a tile coordinate) say which heights it had; `None` leaves that
tile without ground:

<div class="lang-pair">
<div>

```rust
# use ponko::d3::*;
# use ponko::prelude::*;
# fn heights_at(corner: Vec3) -> Vec<f32> { vec![0.0; 33 * 33] }
# fn setup(ctx: &mut Ctx) {
ctx.physics3d.set_terrain_rebuild(|world, tile| {
    let corner = world.get::<Transform>(tile)?.position;
    Some(CollisionMeshData::heightfield(&heights_at(corner), 64.0))
});
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics3d_terrain_rebuild}}
```

</div>
</div>

Without one, the loaded tile has no ground until the game gives it a new
`TerrainCollider` from its heights (as before); `clear_terrain_rebuild`
removes it. The rebuild runs inside the load and only reads the loaded
world: in C++ (`ctx.set_terrain_rebuild`, `ctx.clear_terrain_rebuild`) it
gets a `ponko::loaded_world` whose `get<T>(tile)` reads a component of a
type the game used on the world before the load, and returns a `collision_mesh_data` or `std::nullopt`; an exception it throws
leaves that tile without ground and comes out of `run`. In C,
`ponko_ctx_physics3d_set_terrain_rebuild(ctx, rebuild, user)` takes a
callback that reads the tile with `ponko_loaded_world_get` and returns a
new `ponko_collision_mesh_data_heightfield`, which the engine frees, or
NULL. Despawning the tile removes it; to
edit the ground (a crater), make a new heightfield, put it in the
component's `mesh` and free the old one: the collider changes at the next
step. Its `friction`, `restitution` and `groups` work as on a `d3::Collider`.

**A glTF file, solid in one call:** `ctx.load_collision_mesh(path)` makes
a triangle mesh of every mesh in the file, for a fixed body, so what the
player walks on is exactly what `ctx.gfx.load_mesh(path)` draws.
`ctx.load_collision_mesh_with(path, CollisionMeshOptions { scale, convex })`
scales the corners first, or makes a convex hull (for a dynamic body)
instead. Each file and options is read once; loading it again gives the same
mesh. Like the other setup loaders they panic, with a message starting
`ponko:`, for a missing file or one without triangles;
`try_load_collision_mesh` and `try_load_collision_mesh_with` return the
error instead ([Data files](data-files.md#loader-failure-policy)).

```rust
# use ponko::d3::*;
# use ponko::prelude::*;
# fn setup(ctx: &mut Ctx) {
let look = ctx.gfx.load_mesh("yard.glb");
let solid = ctx.load_collision_mesh("yard.glb");
ctx.world.spawn((Transform::default(), Model::new(look), RigidBody::fixed(), Collider::mesh(solid)));
# }
```

C++: `ctx.load_collision_mesh(path, scale, convex)` (the last two default to
the file as it is; it throws `ponko::error`, and `try_load_collision_mesh`
returns a `ponko::result`); C: `ponko_ctx_load_collision_mesh(ctx, path,
scale, convex, &mesh)`, which returns a status with the error kind in
`ponko_last_error_kind()`. For triangles of your own, `MeshData::triangles()` gives a
mesh's indices three at a time, the shape `trimesh` takes.

In a scene file, a mesh collider names its mesh file the same way, and
`ctx.load_scene` makes the mesh (the [Unity importer](unity.md) writes
them this way); a scene and `load_collision_mesh` share one mesh for the
same file and options:

```json
"Collider3D": { "shape": { "Mesh": { "file": "town/meshes/wall.glb", "scale": [1, 1, 1], "convex": false } } }
```

A collider of a mesh from `load_collision_mesh` is saved by its file, as a
scene's is. A `Collider::scaled_mesh` of such a mesh is saved by the file
and its own scale, and a scene may name one the same way (loading makes the
file's mesh once, and every scale shares it):

```json
"Collider3D": { "shape": { "ScaledMesh": { "mesh": { "file": "props/rock.glb" }, "scale": [2, 2, -2] } } }
```

One of a mesh made in code has no file to name, so saves and the
editor leave it out, like a sprite of a texture made in code: make it
again in code after loading.

**`d3::Transform` stays the source of truth**, as in 2D: after each step,
dynamic bodies write their pose into it; set a transform yourself and the
body teleports there. Every body's `d3::Velocity` component reads and sets
its speed (in C++ a plain struct and a query column). Physics ignores a
transform's scale: size the collider instead.

**Collision groups** decide who touches whom. A collider is in the groups
of its `memberships` bits and touches colliders whose memberships share a
bit with its `filter`; both sides must agree. `CollisionGroups::ALL` (the
default) touches everything:

```rust
const GROUND: u32 = 1;
const DEBRIS: u32 = 2;
// Debris lands on the ground but passes through other debris.
let debris = ponko::d3::CollisionGroups::new(DEBRIS, GROUND);
```

Collisions send `CollisionStarted` and `CollisionStopped` events, the same
ones as in 2D (C++: `ctx.collisions_started()`).

## A walking character

`d3::Character` is a kinematic capsule that walks: it slides along walls,
climbs slopes up to `max_slope`, steps up ledges up to `step_height`, stays
on the ground going downhill (`snap_distance`), pushes light dynamic bodies
(as if it weighed `push_mass`) and sets off sensors. Its `d3::Transform`
position is at its **feet**. Set `velocity` (x and z to walk, y to jump)
and read `grounded`; the step adds gravity while it's in the air:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics3d.rs:walk}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics3d_walk}}
```

</div>
</div>

| Field | Default | |
|---|---|---|
| `height`, `radius` | 1.8, 0.3 | feet to head; the capsule's width |
| `max_slope` | 45° | steeper ground is a wall |
| `step_height` | 0.35 m | 0: no stepping |
| `snap_distance` | 0.3 m | how far down it follows the ground |
| `push_mass` | 80 kg | 0: doesn't push |
| `gravity_scale` | 1 | 0: floats |
| `groups` | all | what it walks on and bumps into |

For client prediction in a networked game, `ctx.physics3d.move_character(entity,
&mut character, feet, dt)` (Rust-only) runs one tick of that walk from any
feet position without moving a body: the same result as the physics step,
bit for bit, from the same state and world. The client predicts its own
character with it and replays it when the server corrects it (see
[client prediction](networking.md#your-own-player-at-once-client-prediction)).
It reads the world as of the last step and pushes nothing.

For mouse look, lock the cursor and turn the head by the raw mouse motion
([input](input.md#mouse-look)) in a frame system:

```rust
{{#include ../../examples/physics3d.rs:look}}
```

## Vehicles

`d3::Vehicle` turns a dynamic body into a car (or a truck, a buggy, anything
on up to 8 wheels) with **raycast wheels**, the classic wheel-collider
model: each physics step, every wheel casts a ray down from its mount,
pushes the chassis up with a spring and a damper, spins by the torques you
give it and the road's grip, and pushes the chassis along through its
tyre's friction curves. A car tuned for that model elsewhere keeps its
numbers here. The chassis faces -Z; give it a mass and a low center of
mass, or it rolls over in turns:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/vehicle.rs:car}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:vehicle3d_car}}
```

</div>
</div>

Everything that makes a car a particular car is **the game's**: the
engine and its gears, which wheels drive and steer, the brakes and assists.
Set each wheel's `motor_torque` (N·m, positive forward), `brake_torque`
and `steer_angle` (radians, positive steers right) in a simulation system
that runs before the physics step, in the `BeforePhysics`
[phase](systems.md#phases):

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/vehicle.rs:drive}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:vehicle3d_drive}}
```

</div>
</div>

Register it, and anything it reads (keys, an autopilot), in `BeforePhysics`
so the wheels act in the same tick. A plain `sim_system` runs after the
step, so its inputs would arrive one tick late:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/vehicle.rs:setup}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:vehicle3d_setup}}
```

</div>
</div>

| `d3::Wheel` field | Default | |
|---|---|---|
| `position` | | the wheel's center at full compression, in the chassis's frame |
| `radius`, `mass` | 0.5 m, 20 kg | the mass sets how fast torque spins it up |
| `suspension_distance` | 0.3 m | how far the suspension travels, straight down the chassis's -Y |
| `spring`, `damper` | 35 000 N/m, 4 500 N·s/m | |
| `target_position` | 0.5 | where it rests under the car's weight: 0 compressed, 1 extended |
| `force_app_point_distance` | 0 | forces act this far up from the resting wheel's bottom; higher rolls less |
| `damping_rate` | 0.25 | slows the spin, N·m per rad/s |
| `forward_friction`, `sideways_friction` | `FrictionCurve::FORWARD`, `::SIDEWAYS` | grip for slip (below) |

The suspension holds each wheel's share of the weight (its `sprung_mass`,
from where the wheels are around the center of mass) at `target_position`,
whatever the spring rate, so a car at rest sits at its designed height;
the spring and damper act on what's beyond that: bumps, braking, turns.

A tyre's grip follows a **friction curve** of its slip: it rises to a peak
(`extremum_slip`, `extremum_value`), falls to a lower sliding grip
(`asymptote_slip`, `asymptote_value`) and stays there, scaled by
`stiffness` (lower it for wet or icy ground) and by the load on the wheel.
Forward slip compares the tyre's speed with the road's (0 rolling, +
spinning, -1 locked); sideways slip is the angle between where the wheel
points and where it goes, in radians.

After each step, every wheel says what happened: `grounded`,
`contact_point` and `contact_normal`, `compression` and
`suspension_force`, `angular_velocity` (and `rpm()`), `forward_slip` and
`sideways_slip` (for skid marks and tyre sounds). To draw a wheel, its
`pose(chassis)` (C++ `ponko::wheel_pose`) is its center and turn: steer,
spin and suspension travel. Pass the chassis's `d3::Transform` from a frame
system and the wheels move as smoothly as the chassis:

```rust
{{#include ../../examples/vehicle.rs:wheels}}
```

The wheels are rays, so a car can't tunnel through the ground however
fast it goes (a test drives one over a heightfield at 30 m/s), and they
see only what `d3::Vehicle::groups` touches. The road is pushed back too:
a car on a dynamic raft pushes the raft. C++ sees the component as
`ponko::vehicle3d`, a plain struct (and a query column): the wheels are
`wheels[0..wheel_count]`.

Cost: a car on heightfield terrain adds about 2 µs to a physics step,
its chassis body included (`cargo run --release -p ponko_bench --
vehicles`, M4 Max):

| Cars driving, 250 steps | Total | Per step, over the terrain alone |
|---|---|---|
| 1 | 1.9 ms | 5 µs |
| 10 | 5.9 ms | 22 µs |
| 100 | 46.7 ms | 185 µs |

Try it: `cargo run --release --example vehicle --features physics3d`.

## Rays, shape casts and overlaps

`ctx.physics3d` answers questions about the world as of the last step:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics3d.rs:queries}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics3d_queries}}
```

</div>
</div>

- `cast_ray(origin, direction, max_distance, filter)`: the closest hit,
  with the entity, point, surface normal and distance.
- `cast_ray_all(.., &mut out)`: every hit, nearest first.
- `cast_sphere`, `cast_box` and `cast_capsule`: sweep a shape (a wheel,
  a crate, a character) and get the first thing it would touch.
- `overlap_point`, `overlap_sphere`, `overlap_box` and `overlap_capsule`:
  every entity inside, into `out`.
- `closest_point(point, max_distance, filter)`: the nearest collider and
  its nearest point.
- `penetrations(shape, position, rotation, filter, &mut out)`: how deep a
  shape sinks into each collider it overlaps, and the way out.
- `apply_impulse(e, impulse)` and `apply_impulse_at(e, impulse, point)`:
  a push in kg × m/s (multiply by `mass(e)` for a speed change); at a point
  it also spins the body.

Every query takes a `d3::QueryFilter` (C++ `ponko::filter3d`):
`.exclude(e)` looks through one entity (the one asking), `.groups(g)` sees
what a collider in those groups would touch, and `.with_sensors()` sees
sensors too (they're skipped by default). Lists go into a `Vec` you keep,
so queries don't allocate.

The 2D and 3D queries follow the same rules:

- **Direction.** Any non-zero direction works: it is normalized, so
  distances are in world units along it. A zero or non-finite direction
  finds nothing.
- **Range.** `max_distance` may be `f32::INFINITY`; a negative or NaN one,
  or a non-finite origin or rotation, finds nothing.
- **Starting inside.** A ray that starts inside a collider hits it at
  distance 0, at the origin, with a zero normal. A shape cast that starts
  touching or overlapping a collider hits it at distance 0, with the
  contact point and the normal that pushes the shape out.
- **Order.** Lists of hits come nearest first; equal distances are ordered
  by entity id (`Entity::to_bits`, lowest first). Overlap lists come in no
  particular order. A single-hit query with two hits at exactly the same
  distance returns either one.
- **Sizes.** A radius or size below `f32::EPSILON` (or NaN) counts as that
  tiny shape.

`closest_point` treats a point inside a collider as its own nearest point
(distance 0, `inside` set), and `penetrations` lists deepest first, equal
depths by entity id. Every 3D query has a 2D form with the same name,
arguments and rules (a sphere in 3D is a circle in 2D: `cast_circle`,
`overlap_circle`); [2D physics](physics.md#rays-overlaps-and-pushes) has
them, and a test asks both the same questions and compares the answers.

New bodies join at the next step, so a query in the tick you spawned them
won't see them yet.

### Shape casts and the nearest surface

A ray is a point moving along a line; a shape cast moves a whole sphere,
box or capsule, so a crate or a character finds what it would bump into.
`cast_box` takes the box's turn as well (`Quat::IDENTITY` for none):

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics3d.rs:shape_queries}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics3d_shape_queries}}
```

</div>
</div>

A body pushed into a wall (by a moving platform, or after a teleport) gets
out with `penetrations`: each entry has the collider, how deep the shape
is in (`depth`) and the direction out (`normal`), deepest first. Moving by
`normal * depth` of the first one frees the shape from that collider. The
nearest surface is `closest_point`, for snapping to the ground or "how far
is the nearest wall":

```rust
# use ponko::prelude::*;
fn unstick(ctx: &mut Ctx, me: Entity) {
    let shape = ponko::d3::Collider::capsule(1.2, 0.3).shape;
    let Some(at) = ctx.world.get::<d3::Transform>(me).map(|t| t.position) else {
        return;
    };
    let mut pushes = Vec::new();
    let not_me = ponko::d3::QueryFilter::default().exclude(me);
    ctx.physics3d.penetrations(shape, at, Quat::IDENTITY, not_me, &mut pushes);
    if let (Some(out), Some(t)) = (pushes.first(), ctx.world.get_mut::<d3::Transform>(me)) {
        t.position += out.normal * out.depth;
    }
}

fn wall_distance(ctx: &Ctx, at: Vec3) -> Option<f32> {
    let near = ctx.physics3d.closest_point(at, 10.0, ponko::d3::QueryFilter::default())?;
    Some(near.distance)
}
```

Collision meshes work as query shapes too (`penetrations` takes any
`d3::Shape`, and a mesh keeps the offset it was made with), and every
query sees convex hulls, triangle meshes and heightfield terrain. From C:
`ponko_ctx_cast_box3d`, `ponko_ctx_overlap_point3d`,
`ponko_ctx_closest_point3d` and `ponko_ctx_penetrations3d` (the shape is a
`PonkoCollider3D`; its settings other than the shape are ignored); C++
`ctx::cast_box3d`, `overlap_point3d`, `closest_point3d` and
`penetrations3d`.

`ctx.physics3d.set_paused(true)` holds the step while the other sim systems
run, for a pause screen
([Pausing only 3D physics](sim-clock.md#pausing-only-3d-physics)).

## Cost

Bodies cost only where they exist, and steady-state steps add no
allocations of their own on top of rapier's. Compared with calling rapier
directly, the engine's syncing adds 13–25%
(`cargo run --release -p ponko_bench -- physics3d`, M4 Max):

| Boxes falling into a pit, 250 ticks | Ponko | rapier alone |
|---|---|---|
| 1,000 | 30.4 ms | 24.5 ms |
| 10,000 | 440 ms | 389 ms |

Sleeping bodies are skipped when positions are written back, and the pass that hands
velocity from heavier to lighter bodies (`dominance`) only runs when some
body has a non-zero dominance group.

Characters cost more than bodies: each one sweeps its capsule against the
world several times a step (walking, stepping, snapping to the ground).
Those sweeps only read the world, so from 8 characters up they run on all
cores; the results are applied one after another in a fixed order, so a
crowd moves exactly the same way on every run. Walking over heightfield
terrain:

| Characters, 250 ticks | Time | Per step |
|---|---|---|
| 100 | 218 ms | 0.9 ms |
| 1,000 | 2.45 s | 9.8 ms |

Characters do allocate a little every step, and almost all of it is rapier's:
its character controller builds fresh contact lists on every move, about 8
small allocations per walking character per tick (counted with four
walkers in `tests/physics3d_alloc.rs`, 1717 of the 1894 allocations in 60
ticks inside rapier's `move_shape`). The engine's own buffers (the list
of bodies a character pushed, the plans) are reused, and the test fails if
the engine allocates more than rapier alone does for the same walkers.
Cutting rapier's share needs a change upstream, so it is a known cost: it
grows with the number of characters, not with the world.

A thousand is about the limit of a 50 Hz step. Characters also sweep
against each other; if a crowd doesn't need to, give them a group that
leaves itself out of its filter (`groups = CollisionGroups::new(CROWD, !CROWD)`),
and each sweep only meets the level.
For bigger crowds, move them yourself, with a ray down to find the ground.
Level geometry as a triangle mesh is somewhat cheaper than a heightfield.

Queries only read the world, and with reused buffers they don't allocate.
10,000 of each through a level of 1,000 fixed boxes and spheres
(`cargo run --release -p ponko_bench -- queries-10k`, M4 Max, median of 5;
the 2D `physics/rays-10k` runs the same counts):

| 10,000 queries | Time |
|---|---|
| `cast_ray` | 1.4 ms (rapier alone: 1.2 ms) |
| `cast_ray_all` | 11.4 ms |
| `cast_sphere` | 9.0 ms |
| `cast_box`, turned | 9.5 ms (rapier alone: 9.1 ms) |
| `closest_point` | 1.2 ms |
| `penetrations`, a turned box | 7.9 ms |

Ponko's shape casts also check whether the shape starts inside something
(the "starting inside" rule), which rapier's own cast doesn't; with that,
a box cast costs about 4% more than rapier's.

Try it: `cargo run --release --example physics3d --features physics3d`.

## Big worlds: solid only near the player

A world that streams in kilometers of terrain and thousands of buildings
doesn't need all of them solid: only the ground near the player and the
vehicles. `ctx.physics3d.stream_colliders(Some(max_new))` (C++
`ctx.stream_colliders(max_new)`) turns that on. From then, the colliders of
fixed bodies and `TerrainCollider`s are only in the physics world near
entities with a **`ColliderFocus`** (C++ `ponko::focus_colliders(radius,
keep_radius)`) or a Rust `d3::SquareColliderFocus`. Square foci measure
distance on the X and Z axes independently, so a collider near a square's
corner can load even if it lies outside a circular focus of the same size.
The radii of a square focus are its half-widths.

- A collider within a focus's `radius` (measured on the ground, to the
  nearest point of the collider) comes in, nearest first, at most
  `max_new` a step, so a big area doesn't arrive in one step.
- It stays until it's beyond `keep_radius` from every focus. The gap
  between the two stops colliders at the edge from coming and going as
  the player walks back and forth.
- A collider never goes while a moving body (dynamic, kinematic or a
  character) is over it, so a crate left behind doesn't fall through the
  ground. Bodies far from every focus find no new ground, though: give
  anything that travels far on its own, like a car, its own focus, a
  bigger one if it's fast.
- Ray casts and other queries only see colliders that are in.
  `ctx.physics3d.is_collider_active(e)` says whether `e`'s is.
- Drawing doesn't change: the world is still drawn out to the streamer's
  load radius. `stream_colliders(None)` brings every collider back.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/physics3d_streaming.rs:collider_streaming}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:collider_streaming}}
```

</div>
</div>

Bringing a collider in or taking it out is cheap: the mesh was built on
the worker, and the collider only joins or leaves rapier's broad phase.
Checking which colliders to bring in looks at every static collider each
step: with 10,000 fixed boxes, 100 balls and a moving focus, a step takes
about 0.2 ms with streaming on or off (M4 Max, release). Building a
129 × 129 heightfield takes about 0.05 ms on the worker, and adding it on
the game's thread under a microsecond.

## Joints

A `d3::Joint` component on a separate entity connects two body entities. Both
endpoints need `d3::Transform`, `d3::RigidBody` and `d3::Collider`. Connections join
at the next physics step; missing endpoints are retried. Despawning the
connection, removing its component, or removing either endpoint's body removes
the Rapier impulse joint on the next step. Several connections can share a body.
Changing the component rebuilds the joint and wakes both bodies.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/physics3d_joint_guide.rs:joints}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:joints}}
```

</div>
</div>

Both snippets are compiled and stepped headless (the Rust one by the
`physics3d_joint_guide` test). In C++ the axes are `ponko::joint_axis::*`
indices into `axes` and `drives`, and `swing_cone` is negative for none.

`fixed(a, b)` locks all axes; `spherical(a, b)` locks translation;
`revolute(a, b)` leaves only rotation around local X free;
`prismatic(a, b)` leaves only translation along local X free.
`generic(a, b)` starts with all six axes free. Use `.motion(axis, Free)`,
`.motion(axis, Locked)` or `.motion(axis, Limited(min, max))` to configure
each axis. Names are qualified by `d3::JointMotion`; axes by `d3::JointAxis`.
Distances are meters and angles radians. Limits must be finite and ordered;
angular ranges must span less than a full turn.

`frame_a` and `frame_b` are anchor poses in each body's local space (scale
ignored). Rotate these frames to orient a hinge or slider. The frames coincide
at the joint's rest pose. For spherical joints, `.swing_cone(angle)` couples
local Y/Z angular motion into a circular cone around X. Local X remains the
independent twist axis. The cone overrides the Y/Z limits and drives; it is
not two independent rectangular angular limits.

`.drive(axis, d3::JointDrive::velocity(speed, damping))` sets a velocity motor.
`d3::JointDrive::spring(target, stiffness, damping)` drives toward an angle or
position. Set its `max_force` to cap force/torque. Drives use Rapier's default
acceleration model; locked axes ignore drives. For an arbitrary target rotation,
express the target in the joint's local angular coordinates and drive the
unlocked angular axes, or rotate the rest anchor frame. These coordinates are
Rapier's joint coordinates, not a general quaternion orientation servo.

Joined bodies do not collide by default; set `contacts_enabled = true` to
restore contact response. Contact dominance does not make joints one-way.
There is no automatic break-force option: Rapier does not expose a simple
break threshold on impulse joints. Despawn the connection to detach explicitly.
`ctx.physics3d.joint_count()` reports live connections after the last step.

## Ragdolls

`ctx.start_ragdoll(entity, &bones)` hands a skinned character's skeleton
over to physics: each `RagdollBone` names a joint and gets a dynamic capsule
running to the joint's first child (a sphere for a joint without children).
Each capsule is held to the nearest chosen bone above it by a spherical
joint, limited to a cone of `.swing(radians)` (default 0.7) and a
`.twist(radians)` either way (default 0.3). Pick the bones that matter
(hips, spine, head, upper and lower limbs). Joints you leave out keep their
pose relative to the bone above them, so hands and fingers ride along.

The bodies start where the skeleton is posed now and are linked to physics
at once, so you can push them in the same frame: a hit, an explosion. From
then on the bodies pose the skeleton after every step (`d3::Ragdoll` on the
entity holds them; `body(joint)` finds one) and its `Animator` waits.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/ragdoll3d.rs:ragdoll}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ragdoll}}
```

</div>
</div>

`ctx.stop_ragdoll(entity, seconds)` removes the bodies and joints and blends
the skeleton from where it lies back to what the animator plays, over
`seconds` (0 snaps). The entity's `d3::Transform` doesn't move by itself: set
it to where the ragdoll lies (its root bone's body) first if the animation
should start from there. The bodies are entities of their own, so stop the
ragdoll before despawning the character. Starting again replaces a running
ragdoll; a bad call (an unknown or repeated joint, a radius that isn't
positive) returns an error and leaves the running one alone. Bodies of
one ragdoll don't collide with the bones they're jointed to, but do with
the rest of the world and the rest of the body.

## Contact modification

`d3::ContactModifier` changes how one body's contacts behave. It is a component:
put it on an entity that has a collider.

- **One-way surfaces.** `d3::ContactModifier::one_way(up)` makes a body solid
  only from the side `up` points to. A ball thrown up from below passes
  through the platform and lands on top of it. `up` is in the body's own
  frame, so a tilted platform turns it with it. `one_way_angle` (radians,
  default 1.2, about 70 degrees) is how far a contact's direction may lean
  from `up` and still count as "from above", so a steep slope of the
  surface still holds; narrow it to make only the flat top solid. Something already partly inside the platform
  keeps passing until it is out, so it is never pushed up from within.
- **Conveyors.** `d3::ContactModifier::conveyor(velocity)` makes the surface
  carry whatever touches it at `velocity`, in metres per second. The
  velocity is in the body's frame, so a belt on a turning body turns its
  direction too. Only the part along the surface counts; the part into it is
  ignored. It works whether the belt or the thing on it comes first in the
  pair.
- **A custom hook.** `d3::ContactModifier::hook()` sends the body's contacts
  to the function you set with `ctx.physics3d.set_contact_hook(...)`
  (`ctx.set_contact_hook3d(...)` in C++; a null function clears it). The hook
  gets a `d3::ContactPair`: the two entities (`a` and `b`), the contact
  `normal()` and its points, and it can `ignore()` the whole pair, drop one
  `remove_point(i)`, change `set_friction` and `set_restitution`, or
  `set_surface_velocity` in world space.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/contacts3d.rs:contacts}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:contacts}}
```

</div>
</div>

The hook runs during the physics step and sees only the pair, so capture
whatever else it needs (an entity, a flag) when you set it; it can't reach
the world. Its changes last for one step: the next step builds the contacts
again and calls the hook again. Keep it short, since it runs for every
contact of every hooked body, every step. In C++ an exception thrown by the
hook ends the run after that step and comes out of `run` or `run_headless`.

Modified bodies cost a little: their contacts are rebuilt every step instead
of being kept while the bodies rest. Bodies without a `d3::ContactModifier`
are not affected, and removing the component makes a body plain again.

## One-way pushing

A higher `d3::RigidBody::dominance` group pushes lower groups without receiving
a contact impulse back. Equal groups react normally. Both bodies remain dynamic,
so the higher body still responds to gravity, fixed walls and equal-group
contacts. Groups default to zero and range from -128 to 127 (C++: `ponko::with_dominance`):

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/physics3d_joint_guide.rs:dominance}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:dominance}}
```

</div>
</div>

This combines Rapier's built-in dominance with a normal-velocity correction on
the lower body. In Rapier 0.36.1, the dominance solver treats the higher side
as stationary: it prevents pushback but does not transfer a moving pusher's
velocity. After solving, the engine applies a normal impulse only to the lower
body, using its effective mass and inertia at the manifold's average contact
point. Contact hooks expose no normal target velocity or per-side inverse-mass
control, so changing contact points alone does not solve this. A headless regression test compares equal-mass dynamic boxes:
the faster higher-group box keeps its velocity while accelerating the other;
equal groups exchange momentum. The setting is a body-wide ordering, not an
arbitrary per-pair rule; it deliberately violates equal-and-opposite contact
reaction. It does not suppress forces from joints, explicit impulses, or vehicle
suspension code. The correction is a single post-solve normal impulse per
manifold, not a full second constraint solve: complex stacks and simultaneous
joint/contact constraints can need smaller steps. It does not transfer tangential
surface velocity or reproduce full restitution against the moving surface.
Fixed and kinematic bodies retain their usual dominance over
dynamic bodies regardless of group.
