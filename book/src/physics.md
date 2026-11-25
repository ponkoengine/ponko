# Physics: bodies that fall, stack and bounce

> **Maturity: [proven](maturity.md).** Game: Ember Hop, packaged and played on
> Linux, macOS and Windows. Tests: `tests/physics.rs`,
> `tests/physics2d_queries.rs`, `tests/tilemap_physics.rs`; with
> `physics-determinism` the same pile and Ember Hop's autopilot give the same
> result on each OS. Platforms: Linux, macOS and Windows, headless (the
> workspace tests in the list of checks). Graphics: not involved.

Crates that tumble, balls that bounce, doors that push things, triggers that
notice who walked in: 2D rigid-body physics, built on
[rapier](https://rapier.rs). It's behind the `physics` feature:

```toml
ponko = { version = "0.1", features = ["physics"] }
```

(The C library, `ponko_ffi`, has it on by default.) For 3D bodies, terrain
and a walking character, see [3D physics](physics3d.md).

Turn it on with gravity in pixels per second² (y is down) and add the
systems that react to it:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics.rs:setup}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics_setup}}
```

</div>
</div>

The physics step is a simulation system in the `Physics`
[phase](systems.md#phases), wherever you call `physics`. Systems that move
things for this tick's step (forces, velocities) go in `BeforePhysics` with
`.add(Phase::BeforePhysics, f)` (C++: `.add(ponko::phase::before_physics, f)`);
`sim_system`s run after the step and see its result.

## Bodies and colliders

An entity becomes a physics body with three components: a `d2::Transform`, a
`RigidBody` (how it moves) and a `Collider` (its shape, in pixels). The
components bring what they need
([required components](ecs.md#required-components)): a `RigidBody` brings a
`d2::Transform` (at the origin) and a `Velocity` (at rest) if the spawn has
none, and a `Collider` without a body brings `RigidBody::fixed()`. So a wall
is a transform and a collider, and a spawn names only what differs:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics.rs:bodies}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics_bodies}}
```

</div>
</div>

| Rust | C++ | Meaning |
|---|---|---|
| `RigidBody::dynamic()` | `ponko::dynamic_body()` | moved by gravity and collisions |
| `RigidBody::fixed()` | `ponko::fixed_body()` | never moves: ground, walls |
| `RigidBody::kinematic()` | `ponko::kinematic_body()` | moved only by you; pushes, isn't pushed |
| `Collider::circle(r)` | `ponko::circle_collider(r)` | |
| `Collider::rect(size)` | `ponko::rect_collider(size)` | |
| `Collider::capsule(h, r)` | `ponko::capsule_collider(h, r)` | vertical, good for characters |
| `Collider::polygon(&points)` | `ponko::polygon_collider({points...})` | convex, 3 to 8 corners: ramps, rocks, ships |
| `.density(d)`, `.friction(f)`, `.restitution(r)` | fields of the same name | mass per pixel², 0 ice..1 rubber, bounciness |
| `.sensor()` | `.sensor = true` | reports overlaps, doesn't push |
| `.offset(v)` | `.offset = v` | shape moved from the entity's position |

A polygon is the convex hull of its corners, given around the entity's
position in any order; `collider.shape.points()` reads them back (C++:
`world.collider_points(e)`). For a concave shape, such as an L, split it
into convex pieces on separate bodies.

A rigid body also has `gravity_scale`, `linear_damping`, `angular_damping`,
`lock_rotation` (characters that shouldn't tip over) and `ccd` (continuous
collision detection, so bullets don't pass through thin walls).

**`d2::Transform` stays the source of truth.** After each step, dynamic bodies
write their new position and rotation into it, so sprites, attachments, the
spatial grid and everything else just follow. Set a transform yourself and
the body teleports there. Change a `RigidBody` or `Collider` and the body is
rebuilt; despawn the entity (or remove either component) and it's gone.

Every body has a `Velocity` component (the body requires one): read it for
the body's speed after every step, or set it (a jump, a thrown crate, a
moving platform):

```rust
# let player = ctx.world.spawn((ponko::d2::Transform::default(),));
ctx.world.insert(player, ponko::d2::Velocity::linear(vec2(0.0, -400.0))); // jump: y is down
```

In C++, `ponko::velocity` is a plain struct, so it also works as a query
column. Bodies and colliders need converting, so like sprites they're read
and written with `get`/`set`, not queried.

## Collisions

When two colliders start or stop touching, the step sends a
`CollisionStarted` or `CollisionStopped` event. `sensor` tells overlaps with
a sensor apart from real contacts:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics.rs:collisions}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics_collisions}}
```

</div>
</div>

Like all [events](events.md), each system sees each collision once.

## Rays, overlaps and pushes

`ctx.physics` answers questions about the world as of the last step, and
gives bodies a push:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics.rs:queries}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics_queries}}
```

</div>
</div>

- `cast_ray(origin, direction, max_distance, filter)`: the closest hit,
  with the entity, point, surface normal and distance.
- `cast_ray_all(.., &mut out)`: every hit, nearest first.
- `cast_circle`, `cast_box` and `cast_capsule`: sweep a shape (a bullet
  with size, a crate, a character) and get the first thing it would touch.
- `overlap_point`, `overlap_circle`, `overlap_box` and `overlap_capsule`:
  every entity inside, into `out`.
- `closest_point(point, max_distance, filter)`: the nearest collider and
  its nearest point.
- `penetrations(shape, position, rotation, filter, &mut out)`: how deep a
  shape sinks into each collider it overlaps, and the way out.

Every query takes a `d2::QueryFilter` (C++ `ponko::filter2d`):
`.exclude(e)` looks through one entity (the one asking), `.groups(g)` sees
what a collider in those groups would touch, and `.with_sensors()` sees
sensors too (they're skipped by default). Lists go into a `Vec` you keep,
so queries don't allocate. The C and C++ `cast_ray`, `entities_at` and
`entities_in_circle` take no filter and see sensors too.
- `apply_impulse(e, impulse)`: a one-off push in mass × pixels/s (multiply by
  `mass(e)` to get a speed change).
- `set_gravity(g)`: change the pull for everyone.
- `set_paused(true)`: hold the step while the other sim systems run, for a
  pause screen ([Pausing only physics](sim-clock.md#pausing-only-physics)).

New bodies join at the next step, so a query in the tick you spawned them
won't see them yet.

### Shape casts and the nearest surface

A ray is a point moving along a line; a shape cast moves a whole circle,
box or capsule, so a crate or a character finds what it would bump into:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/physics.rs:shape_queries}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics_shape_queries}}
```

</div>
</div>

A character pushed into a wall (by a moving platform, or after a
teleport) gets out with `penetrations`: each entry has the collider, how
deep the shape is in (`depth`) and the direction out (`normal`), deepest
first. Moving by `normal * depth` of the first one frees the shape from
that collider:

```rust
# use ponko::prelude::*;
fn unstick(ctx: &mut Ctx, player: Entity) {
    let shape = ponko::d2::Collider::capsule(24.0, 8.0).shape;
    let Some(at) = ctx.world.get::<d2::Transform>(player).map(|t| t.position) else {
        return;
    };
    let mut pushes = Vec::new();
    let not_me = ponko::d2::QueryFilter::default().exclude(player);
    ctx.physics.penetrations(shape, at, 0.0, not_me, &mut pushes);
    if let (Some(out), Some(t)) = (pushes.first(), ctx.world.get_mut::<d2::Transform>(player)) {
        t.position += out.normal * out.depth;
    }
}
```

Polygons work as query shapes too (`penetrations` takes any `d2::Shape`).

### Collision groups

A collider is in some of 32 groups and touches colliders in others, as in
3D: `Collider::circle(4.0).groups(CollisionGroups::new(DEBRIS, !DEBRIS))`
makes debris that bounces off everything except other debris. Queries use
the same test with their filter's `.groups(g)`, so a ground probe can see
only `TERRAIN`. From C, `ponko_set_collider_groups` sets a collider's
groups (C++ `world.set_collider_groups(e, groups)`); `PonkoCollider` has no
groups field, so setting the collider again gives it every group.

### Query rules

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

In both, `closest_point` treats a point inside a collider as its own
nearest point (distance 0, `inside` set), and `penetrations` lists deepest
first, equal depths by entity id. Every 2D query has a 3D form with the same
name, arguments and rules (a circle in 2D is a sphere in 3D:
`cast_sphere`, `overlap_sphere`;
[3D physics](physics3d.md#rays-shape-casts-and-overlaps)).

## Determinism and cost

The same start gives the same result, tick after tick, on one machine: the
physics step runs inside the [simulation clock](sim-clock.md) with a fixed
time step. For bit-identical results across different CPUs and operating
systems (lockstep multiplayer, replays), use the `physics-determinism`
feature instead, which turns on rapier's cross-platform determinism at a
small speed cost.

Bodies cost only where they exist: entities without a `RigidBody` are never
looked at, so the million-entity paths stay as fast as before. Compared with
calling rapier directly, Ponko's syncing adds about 3–8%
(`cargo run --release -p ponko_bench -- physics`):

| Boxes falling into a pile, 300 ticks | Ponko | rapier alone |
|---|---|---|
| 1,000 | 35.1 ms | 32.6 ms |
| 10,000 | ~1.9 s | ~1.8 s |

Try it: `cargo run --release --example physics --features physics`. Left
click drops crates, right click balls, Space blasts things away.
