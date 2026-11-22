# Attachments: things that follow things

Health bars over units, turrets on tanks, selection rings, a carried flag:
entities that should stay at some place relative to another entity. Give
them an `Attached` component and add the built-in `systems::follow_attachments`
system after the systems that move things:

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/attach.rs:attach}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:attach}}
```

</div>
</div>

Every tick, each attached entity's `d2::Transform` gets:

- **position**: the followed entity's position plus `offset`, turned by its
  rotation and stretched by its scale (so flipping a sprite with a negative
  x scale mirrors what hangs off it);
- **rotation**: the followed entity's rotation plus `rotation`.

Its own **scale** is left alone, so a small icon stays small on a big unit.

| Rust | C++ | Meaning |
|---|---|---|
| `Attached::to(e)` | `ponko::attach_to(e)` | right on top of `e` |
| `.offset(v)` | `attach_to(e, v)` | `v` away, in `e`'s space |
| `.rotated(r)` | `attach_to(e, v, r)` | turned `r` radians more |

## Chains and cleanup

Attachments can be attached to attachments (an arm on a body, a hand on the
arm): each link is placed in its parent's space, from the root down, all in
one tick, whatever order the entities were spawned in.

When the entity at the top of a chain is despawned, everything attached to
it (directly or down the chain) is despawned in the same tick. A chain that
loops back on itself is treated as broken and despawned too. Chains can be
as deep as you like.

Attachments are the links of the [entity hierarchy](hierarchy.md): each
parent also keeps its children in order, and `World` has methods to
reparent without moving (`set_parent`), reorder, walk the tree and despawn
a whole subtree.

## Cost

Placing runs on all cores when there is enough of it (a small update
stays on the calling thread) and allocates nothing in steady state: about
**1.0 ms per tick for 100,000 attachments** following 100,000 moving
entities, the move included (release, Apple M4 Max, idle machine,
2026-11-22, `cargo run --release -p ponko_bench -- attach`). Entities without
`Attached` cost nothing.

Attachments are a sim system, so they're deterministic, saved with the world
(`Attached` is reflected) and visible to tools and agents like any other
component.

## In 3D

`attach::Attached3D` does the same for `Transform3D`: its
`offset` is a whole transform in the followed entity's space, so the
attached entity's position, rotation **and** scale are the parent's
composed with it (a lamp on a post, a sword in a hand, a turret on a tank
that turns with it). The same `follow_attachments` system places both
kinds, parents first; chains, loops and despawns work as in 2D.

```rust
# use ponko::prelude::*;
# fn f(world: &mut World) {
let post = world.spawn((ponko::d3::Transform::at(vec3(4.0, 0.0, 0.0)),));
let lamp = world.spawn((
    ponko::d3::Transform::default(),
    ponko::attach::Attached3D::to(post).offset(ponko::d3::Transform::at(vec3(0.0, 3.0, 0.0))),
));
# let _ = lamp;
# }
```

C and C++ have it too: `PonkoAttached3D` and `ponko_attached3d_component`
in C, `ponko::attach3d_to(e, transform3d_at(...))` in C++.

The [editor](inspector.md#the-hierarchy-and-the-asset-browser) parents
entities by adding these: dragging one onto another in its hierarchy
attaches it without moving it.
