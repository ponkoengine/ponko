# The entity hierarchy

A body with an arm and a hand on the arm, a cart with its wheels, a level's
props grouped under one entity: entities that hang off other entities and
move with them. The hierarchy is built on [attachments](attachments.md): a
child's `Attached` (2D) or `Attached3D` (3D) names its parent and holds its
transform in the parent's space. On top of that the engine keeps each
parent's **children, in order**, and gives you one set of methods to edit
and walk the tree.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/hierarchy.rs:hierarchy}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:hierarchy}}
```

</div>
</div>

## Editing the tree

The methods are on `World` (the `WorldHierarchy` trait, in the prelude),
and on `ponko::world` in C++:

| Rust | C++ | Does |
|---|---|---|
| `set_parent(child, parent)` | `set_parent(child, parent)` | links `child` to `parent`, last among its children, **without moving it**; refuses loops |
| `remove_parent(child)` | `remove_parent(child)` | makes it a root where it is |
| `set_child_index(child, i)` | `set_child_index(child, i)` | moves it to place `i` among its siblings |
| `despawn_recursive(e)` | `despawn_recursive(e)` | despawns `e` and everything under it |
| `update_hierarchy()` | `update_hierarchy()` | children lists and transforms up to date now |

`set_parent` picks the link from the transforms: an `Attached3D` when both
entities have a 3D transform, an `Attached` when both have a 2D one. Its
offset is where the child is now in the parent's space, so parenting
never moves anything. To put a child at an offset of your own, insert the
attachment yourself:

```rust
# use ponko::prelude::*;
# fn f(world: &mut World, body: Entity) {
let arm = world.spawn((
    d2::Transform::default(),
    ponko::attach::Attached::to(body).offset(vec2(2.0, 0.0)),
));
# let _ = arm;
# }
```

Attachments added that way (and by scenes, prefabs, the editor, C) join
their parent's children at the next update, after the children already
there, in entity order.

## Despawning a subtree from a system

A system that is iterating a query can't despawn while it iterates, and a
view system or the threads of a parallel query don't hold the world at
all. They record the despawn in their commands instead
(`CommandsHierarchy`, in the prelude; C++ `ctx.despawn_recursive_later`):

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
#[derive(Component)]
struct Wreck {
    parts: u32,
}

fn clear_wrecks(ctx: &mut Ctx) {
    ctx.world.query::<(Entity, &Wreck)>().for_each(|(e, _)| {
        ctx.commands.despawn_recursive(e); // it and everything under it, after the system
    });
}
# let _ = clear_wrecks;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:hierarchy_commands}}
```

</div>
</div>

The despawn takes the subtree **as it is when the commands are applied**:
children linked before then (by earlier commands of the same buffer too)
go with it, children moved elsewhere stay, and an entity that is dead by
then is left alone. A view system records it in `sys.commands`; a
parallel query shares one `Commands` behind a `Mutex`, as for spawns.

## Walking the tree

| Rust | C++ | Gives |
|---|---|---|
| `parent(e)` | `parent(e)` (`std::optional`) | the entity `e` hangs off |
| `children(e)` | `children(e)` | its children, in order |
| `descendants(e)` | `descendants(e)` | everything under it: depth first, each entity before its children, siblings in order |
| `ancestors(e)` | `ancestors(e)` | the chain above it, its parent first |

The order is deterministic: it is the order the children were linked in
(or set with `set_child_index`), the same on every machine and every run,
and it is saved.

## Transforms

`systems::follow_attachments` runs `update_hierarchy` once per tick: add
it after the systems that move things (C++: `game::follow_attachments()`).
Each update

- adds attachments made since the last one to their parents' children,
- despawns entities whose parent is gone, with everything under them, and
  links that loop,
- places every subtree whose **root moved** or whose **links changed**
  (an attachment added, re-pointed or its offset edited), parents before
  children, and
- leaves every other subtree alone.

A child's `Transform` is where its parent and offset put it: to move a
child, change its offset (or move its parent), not its transform. A write
to an attached entity's transform lasts until its subtree is placed again.
Chains can be as deep as you like; there is no depth limit.

## Cost

`cargo run --release -p ponko_bench -- hierarchy`, Apple M4 Max (14
cores), Rust 1.99.1, one update of 100,000 2D transforms; the fastest of
twenty updates after five warm-up ones, the range over three runs:

| Tree | Root moves | Nothing moves |
|---|---|---|
| deep (one chain 100,000 long) | 2.4–2.5 ms | 0.002–0.003 ms |
| wide (100,000 children of one root) | 0.20–0.40 ms | 0.002–0.003 ms |
| wide, 3D | 0.26–0.52 ms | |
| 100,000 roots with one child each, all moving | 0.63–1.1 ms | |

A subtree that didn't move gets no traversal and no transform writes, and
a still tree costs per chunk of 64 entities, not per entity: the scan for
changed attachments skips chunks nobody wrote to (see [change
detection](ecs.md#change-detection)). Removing a child and attaching a
new one under a still 100,000-wide root costs 0.04–0.12 ms: the update
takes removed links out of their old parent's list directly, without
walking the tree. A despawned parent costs the links of its own children:
the hierarchy notes its children list as it goes, with an `on_remove`
hook on `Children` (see [component hooks](ecs.md#component-hooks)), and
the update looks at those links only. With 100,000 root and child pairs
and one root despawned per update, an update takes 0.41 ms at best, where
scanning every link took 0.51 ms.

A level with many children (a wide tree, many moving roots) is placed in
one pass over the attached entities, each finding its parent's new
transform in a table. Each pass of the update (finding the roots that
moved, placing a level) times its first entities and runs on all cores
only when the whole pass would take over about 50 µs: the work measured
decides, not the number of entities. A small update stays on the
calling thread and never waits for other threads, which on a busy machine
can take milliseconds to wake. A deep chain is placed a link at a time.

Linking 100,000 new children to 100,000 parents costs 5.5 ms in the first
update and about 100 allocations in all: a children list keeps its first
four entries in the component itself. Steady updates allocate nothing (a
list that grows past four, or past its capacity, reallocates now and
then).

The machine was busy with other builds during these runs (load average
60–90), so treat the figures as rough; under that load the medians of
the parallel cases spread too widely to quote.

The update turns on change tracking for `Attached`, `Attached3D` and
`Children` the first time it runs; a game that never uses attachments
pays nothing.

## Saves, scenes, prefabs and the editor

- **Saves and scenes** keep the tree: the links are reflected components,
  and so is the children list (`Children`, a list of entities), so the
  order comes back as saved.
- **Prefabs** with children keep their links and order; every spawn's
  copies name each other (see [Prefabs with
  children](scenes.md#prefabs-with-children)). `despawn_prefab` removes the
  prefab's parts; `despawn_recursive` removes what hangs off an entity.
- **The editor** parents by these links: dragging a row onto another in
  the [inspector's hierarchy](inspector.md#the-hierarchy-and-the-asset-browser)
  makes the same link (with undo), and the panel shows children in their
  order. The
  3D editor's gizmos move an attached entity by changing its offset, so
  what hangs off it follows.

## Attachments and the hierarchy

They are the same thing: every attachment is a parent link, and
`follow_attachments` is the hierarchy's update. What the hierarchy adds is
the children list (order, iteration, recursive despawn) and updates that
skip still subtrees. The editor's tree also shows links that aren't
transforms: prefab parts that aren't attached, joint attachments and skins
on a skeleton. Those aren't in `Children`.

## C

`ponko_set_parent`, `ponko_remove_parent`, `ponko_parent`,
`ponko_children`, `ponko_descendants`, `ponko_ancestors` (buffer and count,
like `ponko_ctx_prefab_parts`), `ponko_set_child_index`,
`ponko_despawn_recursive` and `ponko_update_hierarchy`, all on a
`PonkoWorld`. A system records a recursive despawn in its commands with
`ponko_ctx_despawn_recursive_later` (C++ `ctx.despawn_recursive_later`).
