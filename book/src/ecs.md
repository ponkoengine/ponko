# Entities, components, queries

> **Maturity: [proven](maturity.md).** Game: Coin Dash and Ember Hop, packaged
> and played by the list of checks (recorded on macOS). Tests: `crates/ponko_ecs/tests`, and
> its unsafe code under Miri. Platforms: Linux, macOS and Windows, headless
> (the workspace tests in the list of checks). Graphics: not involved.

The game world lives in `ctx.world`, an entity-component system (ECS).

- An **entity** is an id (`Entity`). It has no data of its own.
- A **component** is a Rust type marked `#[derive(Component)]` (see
  [Making a type a component](#making-a-type-a-component)). In C++ it is any
  plain struct (trivially copyable), found by its type: nothing to register.
- A **query** visits every entity that has a given set of components.

## Spawning

<div class="lang-pair">
<div>

```rust
#[derive(Component)]
struct Health(f32);
#[derive(Component)]
struct Enemy;

let e: Entity = ctx.world.spawn((ponko::d2::Transform::at(vec2(10.0, 20.0)), Health(100.0), Enemy));

// Many at once is much faster than a loop of `spawn`:
ctx.world.spawn_batch((0..100_000).map(|i| (ponko::d2::Transform::at(vec2(i as f32, 0.0)), Health(10.0))));
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_spawn}}
```

</div>
</div>

A tuple of components is a *bundle*; bundles go up to 12 components. C++
passes the components straight to `spawn`.

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_components}}
```

## Making a type a component

A Rust type becomes a component by opting in, with the derive (from the
prelude) or by hand; both mean the same:

```rust
#[derive(Component, Clone, Copy, Debug)]
struct Velocity(Vec2);

struct Name(String);
impl Component for Name {}

# let e = ctx.world.spawn((Velocity(Vec2::ZERO), Name("crab".into())));
```

A component is `Send + Sync + 'static`, because systems may read it from
several threads. Other types are not components, so these mistakes don't
compile, and the error says to add the derive:

- a tuple inserted as one component (`world.insert(e, (a, b))`: insert each
  one, or spawn the tuple as a bundle);
- a bare number, `String`, `Vec<..>` or `Entity`: wrap it in a type that says
  what it means, `struct Target(Entity);`, `struct Score(u32);`.

A generic type derives it too: `#[derive(Component)] struct Tagged<T>(T);`
is a component for every `T` that is `Send + Sync + 'static`.
`#[component(requires(..))]` is the derive's one option: see
[Required components](#required-components).
Resources (`insert_resource`) and events need no derive. C++ components need
nothing either: the C API registers them by name and size, not as Rust types.

## Required components

Some components only make sense with others: a body needs a transform and a
velocity, an enemy needs health. A component can say so, with the values to
start from, and every spawn or insert that gives an entity the component
adds the ones it lacks. A spawn then names only what differs:

<div class="lang-pair">
<div>

```rust
#[derive(Component)]
struct Health(f32);

#[derive(Component)]
#[component(requires(ponko::d2::Transform, Health = Health(100.0)))]
struct Enemy;

let grunt = ctx.world.spawn((Enemy,));
let boss = ctx.world.spawn((Enemy, Health(1000.0))); // given values win
assert_eq!(ctx.world.get::<Health>(grunt).unwrap().0, 100.0);
assert!(ctx.world.has::<ponko::d2::Transform>(grunt));
assert_eq!(ctx.world.get::<Health>(boss).unwrap().0, 1000.0);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_required}}
```

</div>
</div>

A type alone (`ponko::d2::Transform`) starts at its `Default`; `Type =
expression` at that value, made anew for each entity that needs it.

- **What's given wins.** A component in the bundle, or one the entity has
  already, keeps its value: requirements only fill gaps. Removing a required
  component later is allowed; it is a component like any other.
- **Every way in.** `spawn`, `spawn_batch`, commands, `insert`, loading a
  save, and the dynamic, C and C++ paths all fill the gaps, in the same
  structural change: the entity never exists without them, and `on_add`
  [hooks](#component-hooks) see it complete.
- **Through.** Required components bring what they require in turn. The
  nearest requirement's value wins (one the spawned component names itself
  beats one a required component names); of two at the same depth, the
  one of the component earlier in the bundle.
- **The engine's own.** A `RigidBody` (2D or 3D) brings a transform and a
  `Velocity`; a `Collider` without a body brings `RigidBody::fixed()`. So a
  wall is a transform and a collider ([Physics](physics.md#bodies-and-colliders)).
- **By hand, and for types you don't own.** `#[component(requires(..))]`
  writes `Component::requires`, which you can also write yourself
  (`r.add(Health::default)`). `world.require::<Enemy, _>(|| Health(5.0))`
  adds one at run time, for a type from another crate; it applies to that
  world, from then on (entities that exist keep what they have).
- **What it costs.** Nothing where it isn't used: a spawn whose bundle
  lacks nothing runs the same code as in a world without requirements.
  Filling a gap runs the constructor on a path of its own: a one-by-one
  spawn costs about 20 % (2 ns) more than giving the component in the
  bundle, a batch or a spawn-and-despawn loop about the same;
  `crates/ponko_bench/REQUIRED.md` has the numbers.

From C, `ponko_component_require(world, component, required, &value)`
copies the value's bytes, so the required component must be a plain value
(a C component, `PonkoTransform2D`, `PonkoVelocity`, ...), not a converted
one such as `PonkoSprite`.

## Reading and changing one entity

<div class="lang-pair">
<div>

```rust
# #[derive(Component)]
# struct Health(f32);
# #[derive(Component)]
# struct Enemy;
# let e = ctx.world.spawn((Health(10.0), Enemy));
if let Some(h) = ctx.world.get_mut::<Health>(e) {
    h.0 -= 5.0;
}
ctx.world.insert(e, Enemy);        // add or replace a component
ctx.world.remove::<Enemy>(e);      // returns the removed value
ctx.world.despawn(e);
assert!(!ctx.world.is_alive(e));   // old handles stay dead, even if the slot is reused
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_one}}
```

</div>
</div>

## Queries

<div class="lang-pair">
<div>

```rust
# #[derive(Component)]
# struct Health(f32);
# #[derive(Component)]
# struct Enemy;
ctx.world
    .query::<(&mut ponko::d2::Transform, &Health)>()
    .filter::<Without<Enemy>>()
    .for_each(|(t, h)| t.position.y += h.0);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_query}}
```

</div>
</div>

In C++, `each` reads the components to visit from your function's
parameters: `T&` to change one, `const T&` to read it, and a first
`ponko::entity` parameter gets the entity's id. `world.each_par` is the C++
`par_for_each`: your function runs on all cores at once, so it must only
touch its own entity.

For code that works on whole arrays, `world.query<A, B>(fn)` hands you one
*chunk* (archetype) at a time, with an array per component:

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_columns}}
```

`each` compiles to the same loop, so it's as fast. With `query_par`, size a
buffer with `world.count<A, B>()` and write row `i` of a chunk to slot
`chunk.first() + i` to fill it from all cores: rows are numbered the same
way in every query.

What you can ask for (Rust):

| In the query | You get |
|---|---|
| `&T` | read access to `T` (entity must have it) |
| `&mut T` | write access to `T` |
| `Option<&T>`, `Option<&mut T>` | the component if present; doesn't filter (in `for_each` and the parallel loops, up to four of them cost almost nothing per entity: the loop is chosen per archetype, and vectorizes like one without `Option`s) |
| `Entity` | the entity's id |
| a tuple of the above | all of them, up to 12 (nest tuples for more) |

Filters narrow the match without fetching data: `filter::<With<T>>()` and
`filter::<Without<T>>()` (C++: `ponko::with<T>`, `ponko::without<T>` in the
list of `each`, `query` and `count`). `filter::<F>()` takes every typed
filter: `With<T>`, `Without<T>`, `Or<(...)>` for "either" matches, or a tuple
of them that must all hold, which can be a type alias you reuse across systems
(C++: `ponko::or_<...>` and `ponko::all_of<...>`, which mirror `Or` and
tuples; `or` is a C++ keyword):

<div class="lang-pair">
<div>

```rust
# #[derive(Component)]
# struct Player;
# #[derive(Component)]
# struct Enemy;
# #[derive(Component)]
# struct Dead;
# #[derive(Component)]
# struct Health(f32);
type Living = (Or<(With<Player>, With<Enemy>)>, Without<Dead>);
world.query::<&mut Health>().filter::<Living>().for_each(|h| h.0 -= 1.0);

// An AND group inside an Or: both Player and Enemy, or not Dead.
type Odd = Or<((With<Player>, With<Enemy>), Without<Dead>)>;
let odd = world.query::<&Health>().filter::<Odd>().count();
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_filters}}
```

</div>
</div>

A tuple can also be an `Or` arm: `Or<((With<Player>, With<Enemy>), Without<Dead>)>`
means "both Player and Enemy, or not Dead". Tuples and `Or`s can nest.
`Or<()>` matches nothing; an empty tuple arm matches everything (C++:
`or_<>` and `all_of<>`). The inline
storage expands `(A AND B) OR C` into `(A OR C) AND (B OR C)`: up to 4
clauses and 16 component predicates across them, counting repetitions from
expansion. Exceeding these limits panics in Rust and throws
`std::runtime_error` in C++. These filters work with parallel queries too
(`each_par` and `query_par` in C++), and with the
[archetype index](#which-archetypes-a-query-visits).

**From C**, there are no types, so a filter is built from component ids and
passed to a query. `ponko_filter_with(id)`, `ponko_filter_without(id)`,
`ponko_filter_all(parts, n)` (a tuple, or an AND group inside an any),
`ponko_filter_any(parts, n)` (`Or`), `ponko_filter_always()` (`()`) and
`ponko_filter_never()` (`Or<()>`) build one; `ponko_query_filtered(world, ids,
count, filter, options, callback, user)` is `ponko_query_with` over the
entities that pass it, serial or parallel (`options->parallel`). The result
is the one the Rust query with the same filter has, and the limits are the
same: a filter over them, or one that names a component the world doesn't
know, is `PONKO_INVALID_ARGUMENT` (check one ahead with `ponko_filter_check`)
instead of a panic. `all` and `any` copy their parts, so free the parts and
the filter with `ponko_filter_free`. `all` and `any` need at least one part
(that is what `always` and `never` are for). A component named twice is one
slot, and `with` and `without` of one component match nothing, as for a Rust
tuple. Groups nest at most 16 deep (`always` and `never` are leaves, not
levels), and a filter is compiled when a query uses it, without allocating,
so one filter can be built once and used every frame.

```c
{{#include ../../examples/cpp/guide.c:ecs_filters}}
```

`Added` and `Changed` are separate: from C they are the `changed` and `added`
masks of `PonkoQueryOptions`, which `ponko_query_filtered` takes as well.

Asking for the same component twice with `&mut` (or `Option<&mut T>`) panics.

### Which archetypes a query visits

A query first has to find the archetypes (the sets of entities with the same
components) that hold what it asks for. In a small world it tests every
archetype, which is the fastest way there. In a world with many archetypes
the world keeps an **index**: for a query it has seen before, the list of
archetypes that can match, so the query skips the rest. You don't turn it
on, and it never changes what a query returns or in which order; it only
changes how long the query takes to find its entities.

What is indexed automatically:

- Only worlds with at least 64 archetypes. Below that every query scans.
- A query's *signature*: the components it needs and its `with`, `without`
  and `Or` filters, however they are written (`.filter::<With<A>>()` twice, `With<A>` in a
  tuple, a filter built at run time from C, and filters in another order
  are one signature). `Added` and `Changed` aren't part of it; they pick
  rows inside the archetypes.
- A signature that was used twice and matches at most half of the
  archetypes. One that matches more is scanned: a list of nearly everything
  saves nothing.
- At most 128 signatures and 1 MiB of lists per world. Signatures that are
  no longer used make room for new ones.

The index follows the world on its own: new archetypes are added to the
lists the next time a query runs, and despawning, `clear`, loading a scene
and registering components never leave it stale.
Queries of a [world view](system-access.md) read the same index, also from
several threads at once.

Two hints change the choice for one query, and a policy for a whole world:

<div class="lang-pair">
<div>

```rust
# use ponko::ecs_core::QueryCachePolicy;
# #[derive(Component)]
# struct Health(f32);
# #[derive(Component)]
# struct Boss;
// Runs rarely, but must not test every archetype of a big world when it
// does: index it on first use.
let bosses = world.query::<&Health>().filter::<With<Boss>>().cached().count();
// Never use the index for this query.
let by_scan = world.query::<&Health>().filter::<With<Boss>>().uncached().count();
// Or opt the whole world out.
world.set_query_cache_policy(QueryCachePolicy::off());
# let _ = (bosses, by_scan);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_query_cache}}
```

</div>
</div>

- `cached()` (C++: `ponko::cached` in the list of `each`, `query` and
  `count`) indexes the query on its first use and keeps it indexed when it
  matches most archetypes. Use it for a query that runs once in a while in
  a world with many archetypes. In a world under 64 archetypes it changes
  nothing.
- `uncached()` (C++: `ponko::uncached`) always scans. Use it for a query
  with a filter that is different on every call, so it doesn't take a place
  in the index.
- `world.set_query_cache_policy(policy)` changes the limits above for one
  world (`QueryCachePolicy` in Rust, `PonkoQueryCachePolicy` from C and
  C++); `QueryCachePolicy::off()` (`ponko_query_cache_policy_off()`) turns
  the index off. `world.query_cache_stats()` says how many signatures are
  indexed, how much memory the lists take and how often they were used.

**From C**, `ponko_query`, `ponko_query_with` and `ponko_query_filtered`
use the index automatically. `ponko_query_cached(world, ids, count, filter,
options, cache, callback, user)` is `ponko_query_filtered` with a hint:
`PONKO_QUERY_CACHE_AUTO`, `PONKO_QUERY_CACHE_ALWAYS` or
`PONKO_QUERY_CACHE_NEVER` (`filter` may be `NULL`).
`ponko_world_set_query_cache_policy`, `ponko_world_query_cache_policy` and
`ponko_world_query_cache_stats` are the world's policy and counters.

How much it saves depends on how many archetypes there are and how few of
them the query matches. Measured on one machine (see
`crates/ponko_bench/QUERY_CACHE.md` for the commands and every number): a
query that matches 1 in 16 archetypes takes about the same time either way
at 32 archetypes, 58% of the scan's time at 64, 14% at 1,024 and, for 1 in
64, 6% at 4,096. A query that matches every archetype gains nothing from a
list, which is why those are scanned; looking a signature up costs about
50 ns per query, and only in worlds with at least 64 archetypes.

Ways to run a query:

- `for_each(f)`: the fastest loop; use this by default.
- `par_for_each(f)`: same, split over all CPU cores. Use it for per-entity
  work on many entities. Many small archetypes share the cores too. A loop
  times its first rows and stays on the calling thread when the whole loop
  would be quick (under about 50 µs), whatever its size in entities: the
  work measured decides, so a cheap loop over a big world doesn't wait for
  other threads, and a heavy one over a few thousand entities still uses
  every core. Because
  that choice depends on measured time, which threads run which entities,
  and in what order, can change from run to run even on small worlds: side
  effects that depend on order (pushing into a shared `Vec`, summing floats
  into shared state) aren't reproducible. Per-entity results, the positions
  of `par_for_each_indexed` and the order of `par_collect_into` are.
- `par_collect_into(&mut vec, f)`: fills `vec` with `f` of every match, on
  all cores, in `for_each` order. The vector keeps its memory, so doing it
  every tick allocates nothing.
- `iter()`: a normal iterator, for when you need `map`/`filter`/`collect`. It
  is several times slower than `for_each` on big worlds.
- `count()`, `get(entity)`.

### Read-only queries on a shared world

`world.query` takes `&mut World`, because it may register components and
count writes for change detection. A query that only reads can run on a
shared `&World` with `world.query_ref`: several at once, inside each other,
from code that only has `&World` (a hash, a debug view, another thread):

```rust
# #[derive(Component)]
# struct Pos(Vec2);
# #[derive(Component)]
# struct Team(u8);
fn close_enemies(world: &World) -> usize {
    let units = world.query_ref::<(Entity, &Pos, &Team)>();
    let mut pairs = 0;
    for (a, pa, ta) in units.iter() {
        for (b, pb, tb) in units.iter() {
            if a < b && ta.0 != tb.0 && pa.0.distance(pb.0) < 2.0 {
                pairs += 1;
            }
        }
    }
    pairs
}
```

It takes the same filters (`With`, `Without`, `Or`, `Changed`, `Added`), and
`get`, `single`, `count`, `iter` and `for_each`. Items borrow the world, not
the query. Only `&T`, `Option<&T>` and `Entity` are allowed; `&mut T` doesn't
compile. A component the world has never seen matches nothing (and reads as
`None` in an `Option`) instead of being registered. Change filters need the
component's changes tracked, as with `query`.

## Commands: changing the world while you iterate

You can't spawn or despawn inside a query loop, because the query is borrowing
the world. Record the change in `ctx.commands` instead:

<div class="lang-pair">
<div>

```rust
# #[derive(Component)]
# struct Health(f32);
fn die(ctx: &mut Ctx) {
    let cmds = &mut ctx.commands;
    ctx.world.query::<(Entity, &Health)>().for_each(|(e, h)| {
        if h.0 <= 0.0 { cmds.despawn(e); }
    });
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_commands}}
```

</div>
</div>

The engine applies commands after every system, so the next system sees them.

### A deferred spawn gives you the entity

`commands.spawn` returns the new entity right away, so the commands after it
can use it: give it more components, or store it in another entity. The
entity becomes alive when the commands are applied; until then
`world.is_alive` says no and queries don't see it.

<div class="lang-pair">
<div>

```rust
# #[derive(Component)]
# struct Enemy;
#[derive(Component)]
struct Bullet { speed: f32 }
#[derive(Component)]
struct Aim(Entity);

fn fire_at_guns(ctx: &mut Ctx) {
    let cmds = &mut ctx.commands;
    ctx.world.query::<(Entity, &Enemy)>().for_each(|(gun, _)| {
        // The id exists now; the entity once the commands are applied.
        let bullet = cmds.spawn((Bullet { speed: 9.0 },));
        cmds.insert(gun, Aim(bullet));
    });
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_spawn_ids}}
```

</div>
</div>

The id is reserved in the world when `spawn` returns: nothing else gets it,
also not a spawn made directly in `ctx.world` meanwhile. If the spawn never
happens (the commands are cleared or dropped, an earlier command panics, or
the bundle can't be spawned), the id is given back and its handle stays dead
for good, like a despawned one. C has the same as
`ponko_ctx_spawn_later_entity`.

`ctx.commands` and a view system's `sys.commands` are tied to the game's
world. A buffer of your own comes from `world.commands()`; `Commands::new()`
makes one that isn't tied to a world yet, and `spawn` on it panics until it
is applied once or `bind` is called. Several buffers can record at once, one
per thread, without ever handing out the same id; from a `par_for_each`, put
one behind a `Mutex`:

```rust
# use std::sync::Mutex;
# #[derive(Component)]
# struct Enemy;
# #[derive(Component)]
# struct Bullet { speed: f32 }
# #[derive(Component)]
# struct Aim(Entity);
# let mut world = World::new();
let commands = Mutex::new(world.commands());
world.query::<(Entity, &Enemy)>().par_for_each(|(gun, _)| {
    let mut cmds = commands.lock().unwrap();
    let bullet = cmds.spawn((Bullet { speed: 9.0 },));
    cmds.insert(gun, Aim(bullet));
});
world.apply(&mut commands.into_inner().unwrap());
```

Ids are deterministic wherever the commands are: spawns recorded in the
same order get the same ids, in plain systems and view systems, with the
parallel scheduler on or off and on any number of threads (a system that
records commands runs alone or last in its batch). Inside a parallel query
they are not: which item gets which id depends on the order the threads
take the lock, like the order of the commands they record. Every spawn
still gets its own id and is applied. When the ids matter (anything keyed
by entity, such as random streams per entity or a lockstep checksum),
compute in parallel and spawn in query order:

```rust
# #[derive(Component)]
# struct Enemy;
# #[derive(Component)]
# struct Bullet { speed: f32 }
# #[derive(Component)]
# struct Aim(Entity);
# let mut world = World::new();
# let mut cmds = world.commands();
let mut shots = Vec::new();
world
    .query::<(Entity, &Enemy)>()
    .par_collect_into(&mut shots, |(gun, _)| (gun, 9.0));
for (gun, speed) in shots {
    let bullet = cmds.spawn((Bullet { speed },));
    cmds.insert(gun, Aim(bullet));
}
world.apply(&mut cmds);
```

A `World::reset` (or loading a saved world) makes ids reserved before it
meaningless: applying a spawn recorded before it panics.

So that deferred spawns reuse slots, the world sets free slots aside for
command buffers: as many as they took in one frame lately. The engine sizes
that set once per frame with `world.return_idle_ids()`, which gives the rest
back, so a game that stops spawning through commands doesn't keep them; a
world you drive yourself calls it once per frame too.

Spawns and despawns made directly in the world pay nothing for this: the
world takes new indices for its own spawns 256 at a time, so `world.spawn`
only compares the slot table's length with the end of its own range, as it
compared it with the largest index before. A deferred spawn costs about
2 ns more than before (on top of the 24 ns a spawn and despawn through
commands cost): its slot is set aside when an earlier buffer is applied,
its id is written to the buffer's list and read back when the spawn is
applied, and the slot is touched once more to claim it.

Measured with `ponko_bench` in release builds on an M4 Max that was busy
with other builds (load average 58–61): each benchmark's fastest run
(of 10 for the `spawn`/`despawn` rows, of 50 for the `commands` rows),
three runs of base and of this change interleaved, milliseconds:

| Benchmark | Before | Reserved ids |
|---|---|---|
| `spawn/ponko` (1,000,000 spawns) | 13.85–13.99 | 13.74–14.51 |
| `spawn/ponko-batch` | 10.34–10.64 | 9.96–10.45 |
| `despawn/ponko` (1,000,000 despawns) | 8.14–8.54 | 8.23–8.26 |
| `commands/insert-remove` | 4.59–4.76 | 4.71–4.77 |
| `commands/spawn-despawn` (100,000 deferred) | 2.38–2.42 | 2.59–2.83 |
| `commands/spawn-then-despawn` (callback design) | | 2.39–2.40 |
| `commands/spawn-link` | | 3.18–3.34 |
| `commands/spawn-then-link` (callback design) | | 2.90–3.09 |

The first four are equal within the noise of a busy machine (five more
rounds of the two closest: `commands/insert-remove` 4.65 against 4.66,
`despawn/ponko` 8.06 against 8.03). Across sessions the deferred spawn
measured 1.6–2.1 ns more.

Two other designs were measured:

- A `spawn_then(bundle, |world, entity| ...)` that hands the entity to a
  callback when applied costs what a spawn did (`commands/spawn-then-*`).
  We took reserved ids anyway: with them the entity is an ordinary value as
  soon as it is spawned, so it can be stored in locals, components and
  events, returned from functions, linked both ways between two new
  entities and passed to C, none of which a callback that runs later can
  do without nesting.
- Lock-free reservation, where buffers take ids with one atomic operation
  on a cursor over the world's free list and the world settles them when
  it next has `&mut` access, would make deferred spawns as cheap as direct
  ones. Here a buffer isn't a borrow of the world: it can record on another
  thread while the world spawns directly, so every direct spawn would have
  to update that cursor atomically too. One uncontended atomic
  read-modify-write in `world.spawn` alone made `spawn/ponko` 3.5% and
  `spawn/ponko-batch` 2.4% slower, so direct spawns would pay for what only
  deferred ones use. The world's free list stays its own; buffers take
  slots from a set it puts aside when it applies them, under a lock taken
  about once per apply (and once per 256 new indices), not per spawn.

## Change detection

Some work only needs the entities that changed: resize a health bar when
`Health` changes, rebuild a path when a `Target` moves, upload an instance
when its transform changes. Ask for them with the filters `Changed<T>` and
`Added<T>`:

<div class="lang-pair">
<div>

```rust
# #[derive(Component)]
# struct Health(f32);
# #[derive(Component)]
# struct HealthBar { width: f32 }
fn setup(ctx: &mut Ctx) {
    // Once, before the first system that filters on Health.
    ctx.world.track_changes::<Health>();
}
fn resize_health_bars(ctx: &mut Ctx) {
    // Only the entities whose Health changed since this system last ran.
    ctx.world
        .query::<(&Health, &mut HealthBar)>()
        .filter::<Changed<Health>>()
        .for_each(|(hp, bar)| bar.width = hp.0);
    // The ones that are new, and the ones that lost it (or despawned).
    let spawned = ctx.world.query::<Entity>().filter::<Added<Health>>().count();
    let gone: &[Entity] = ctx.world.removed::<Health>();
#   let _ = (spawned, gone);
}
fn regenerate(ctx: &mut Ctx) {
    // `&mut Health` can write, so every entity visited counts as changed.
    // Ask for `&Health` when you only read.
    ctx.world.query::<&mut Health>().for_each(|hp| hp.0 += 1.0);
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ecs_changes}}
```

</div>
</div>

**It is opt-in, per component, and explicit.** Nothing is tracked until
`world.track_changes::<T>()` asks for it (C++: `world.track_changes<T>()`).
Call it in `setup` for every component whose changes or removals you read:
in Rust a `Changed<T>` or `Added<T>` filter and `removed::<T>()` panic for a
component that isn't tracked, and say which call is missing. In C and C++ a
`changed` or `added` filter (`ponko::changed<T>`, `ponko::added<T>`) still
starts tracking on its first use; `removed` needs `track_changes` there too.
A game that tracks nothing keeps no stamps and runs the same loops as
before.

**Each system has its own view.** In a system, the filters and `removed`
report what happened since that system last ran, so every system sees each
change exactly once, whether it runs before or after the system that made
it, and a system that runs every tenth tick sees ten ticks of changes. A
system isn't shown its own changes (or its own commands) again. The first
time a system runs, and in code outside systems such as `setup`, everything
counts as added and changed.

What counts, for a tracked component:

| Operation | Added | Changed |
|---|---|---|
| `spawn`, `spawn_batch`, `spawn_at`, and their `_dynamic` forms | yes | yes |
| `insert` on an entity without one | yes | yes |
| `insert` replacing a value | no | yes |
| `get_mut` | no | yes |
| `&mut T` or `Option<&mut T>` in a query | no | yes, for every entity the query visits |
| the same in a view system (`sys.world`), and its `get_mut` | no | yes |
| `mark_changed::<T>(e)` | no | yes |
| `get`, `has`, `&T` in a query, `count()`, `sort_by` | no | no |
| `remove`, `despawn`, `clear` | goes to `removed::<T>()`; the entity's other components keep their state |

- **A mutable borrow is a change, written or not.** The world can't see a
  write through `&mut T`, so it assumes one. A query with `&mut T` marks
  every entity it visits: all it matches, or only the ones that pass its
  `Added`/`Changed` filters. To change only a few entities, query `&T` and
  call `get_mut` (or record a command) for those. A system that both reads
  `Changed<T>` and writes `T` to every entity makes every other reader of
  `T` see everything, every tick.
- **`Changed` includes `Added`**: a new value is also a changed one. Use
  `Added<T>` alone for "new here". Inserting `T` again after removing it
  counts as added again.
- **Filters combine** with the others: `(Changed<A>, Without<Dead>)` needs
  both, `Or<(Changed<A>, Changed<B>)>` needs either. A filter on `T` also
  requires `T`, like `With<T>`. Up to 4 `Added`/`Changed` filters per query
  outside `Or`.
- **Commands** are stamped when they are applied, which is when the
  recording system returns: the other systems see them as changes of that
  system.
- **Removals** list each entity that lost `T` since the system last ran,
  in the order it happened, despawned entities included (check
  `world.is_alive(e)` before using one). The engine drops a removal once
  every system has been shown it, so a system that doesn't run holds
  removals back: while the simulation is paused, the removals that frame
  systems make stay in the logs until the simulation systems run again.
- **Saving and loading**: stamps and removals are not part of a saved
  world. Loading a save removes every entity and spawns the saved ones, so
  every system sees them as removed and then added.
- **Replacing the world** (`ctx.world = World::new()`) starts every system
  over: the new world has its own history, so on its next run each system
  sees everything in the new world as added and changed, and the removals
  logged there so far, however rarely it runs. Nothing is reported for the
  old world's entities; they were not removed, their world is gone. A
  `Tick` you kept yourself belongs to the world it came from: keep
  `world.id()` next to it and start again from `Tick::default()` when the
  id differs.
- **The tick counter doesn't run out.** The world counts ticks in 64 bits
  and stores 32 bits per stamp. A stamp older than two billion ticks is
  kept at that age, so a very old change is never mistaken for a new one;
  a reader that slept that long is shown everything.

C and C++ components work the same way. In C++, an `each` parameter taken
as `T&` can write and counts as a change, while `const T&` or `T` only
reads; `world.query<const A, B>(fn)` does the same for whole arrays, and
`ponko::any_of<ponko::changed<A>, ponko::changed<B>>` is the `Or`. In C,
`ponko_query` marks every column it hands out, and `ponko_query_with` takes
a `PonkoQueryOptions` saying which columns it writes and which must have
changed. `ponko_set` (C++ `world.set`) counts like `insert`, and
`ponko_mark_changed` (C++ `world.mark_changed<T>(e)`) marks one entity by
hand, for a value that depends on something the world can't see.

Outside the game loop (a plain `World` in a tool or a test) there are no
systems, so keep the tick yourself: `let seen = world.advance_change_tick()`
after looking, then `.since(seen)` on the next query and
`world.removed_since::<T>(seen)`; call `world.forget_removed(seen)` so the
removal logs don't grow.

**What it costs.** For a component that isn't tracked: nothing. For a
tracked one: 8 bytes per entity plus 4 bytes per 64 entities, and one
stamp per entity for each mutable visit. The 4 bytes keep the newest
change of each run of 64 rows, so a `Changed<T>` or `Added<T>` filter
skips 64 entities at a time where nobody wrote, and a still world costs
per chunk, not per entity. With 1M entities on an Apple M-series laptop,
a `&mut` loop takes 0.20 ms instead of 0.17 ms, a `&T` loop is
unchanged, and a `Changed<T>` loop takes 0.02 ms when it finds nothing
(0.5 ms when it read every stamp), 0.4 ms when 1% of the entities
changed, spread out (most chunks then hold one), and 1.0 ms when all
did. Despawning from an archetype with a tracked component logs its
removal and runs fewer instructions than a despawn did before the
per-64 summary existed. Moving an entity into such an archetype (an
insert or a remove) keeps its stamps, so a `Changed` loop afterwards
still skips the blocks where nobody wrote. `cargo run --release -p ponko_bench -- changes` prints the numbers
for your machine.

## Component hooks

Change detection tells a system what changed the next time it runs. Some
bookkeeping can't wait for that: an index from names to entities that any
code may read right after a spawn, a count that must match the world at
every moment. A *hook* is a function the world calls inside the
operation itself, registered per component type:

| Hook | Runs |
|---|---|
| `on_add::<T>` | after a `T` arrived on an entity that had none (every kind of spawn, `insert`) |
| `on_replace::<T>` | before `insert` overwrites an entity's `T`; the old value is in the world, the new one is passed in |
| `on_remove::<T>` | before a `T` leaves (`remove`, `despawn`, `clear`), while it is still there |

<div class="lang-pair">
<div>

```rust
use std::collections::HashMap;

#[derive(Component)]
struct Name(String);

/// Every named entity, by name: always the same as the world.
#[derive(Default)]
struct Names(HashMap<String, Entity>);

fn named(world: &mut HookWorld, e: Entity) {
    world.resource_scope::<Names, _>(|world, names| {
        if let Some(n) = world.get::<Name>(e) {
            names.0.insert(n.0.clone(), e);
        }
    });
}

fn renamed(world: &mut HookWorld, e: Entity, new: &Name) {
    world.resource_scope::<Names, _>(|world, names| {
        if let Some(old) = world.get::<Name>(e) {
            names.0.remove(&old.0);
        }
        names.0.insert(new.0.clone(), e);
    });
}

fn unnamed(world: &mut HookWorld, e: Entity) {
    world.resource_scope::<Names, _>(|world, names| {
        if let Some(n) = world.get::<Name>(e) {
            names.0.remove(&n.0);
        }
    });
}

fn setup(ctx: &mut Ctx) {
    ctx.world.insert_resource(Names::default());
    ctx.world.on_add::<Name>(named);
    ctx.world.on_replace::<Name>(renamed);
    ctx.world.on_remove::<Name>(unnamed);
}
# setup(ctx);
let crab = ctx.world.spawn((Name("crab".into()),));
assert_eq!(ctx.world.resource::<Names>().0.get("crab"), Some(&crab));
ctx.world.insert(crab, Name("king crab".into()));
ctx.world.despawn(crab);
assert!(ctx.world.resource::<Names>().0.is_empty());
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:component_hooks}}
```

</div>
</div>

- **A hook can't change the world's structure right away.** It gets a
  `HookWorld`: components, resources and queries, read and write, but
  spawns, despawns, inserts and removes go to `world.commands()`. They are
  applied before the call that ran the hook returns (after the whole call
  for `spawn_batch`, `clear` or applying commands), so nothing a hook does
  moves or drops the entity the world is busy with.
- **The order is fixed.** The hooks of one operation run in the order of
  their components' ids (the order the world first saw each type), several
  hooks of one component in the order they were registered, and commands
  from hooks first recorded first. Hooks never run during a parallel query
  or a view system: only structural changes run them, and those need the
  whole world.
- **Loads, resets and dropping.** Loading a save resets the world, which
  runs `on_remove`, then spawns the saved entities, which runs `on_add`,
  so an index like `Names` above stays right. Hooks belong to the world they
  are registered on: a world made with `World::new()` has none. Dropping a
  world runs no hooks.
- **What it costs.** Nothing for components without hooks: spawning,
  despawning, inserting and removing them run the same code as in a world
  without hooks, and queries never look at hooks. A hook call costs a few
  nanoseconds besides its own work; `crates/ponko_bench/HOOKS.md` has the
  numbers.

**Hooks or change detection?** Use a hook when something must be true the
moment a component arrives or leaves, like the index above. Use change
detection for work done once per tick over everything that changed:
resizing health bars, uploading transforms. It runs in a batch, can run in
parallel and costs nothing at the moment of the change. The engine does
both: the transform hierarchy notes a despawned parent's children with an
`on_remove` hook on `Children`, while physics drops the bodies of despawned
entities once per step, in entity order, which keeps runs deterministic.

From C, `ponko_component_on_add`, `_on_replace` and `_on_remove` register
a hook on a plain component (registered from C, or a built-in C sees as
its own bytes): it gets a `PonkoHookWorld` and a `PonkoHookEvent` with
the entity, the value and, for a replace, the new one. Inside it,
`ponko_hook_get` and `ponko_hook_write` read and write components in
place, and `ponko_hook_spawn_later`, `_set_later`, `_remove_later` and
`_despawn_later` record structural changes, the same rules as
`HookWorld`; the world's own calls return `PONKO_BUSY` there. C++ wraps
them as `world.on_add<T>`, `on_replace<T>` and `on_remove<T>` with a
`ponko::hook_world`; an exception a hook throws is rethrown when the
system returns. Hooks registered from any language run for changes made
from any language.

## Resources

A resource is one global value of a type, stored in the world and found
by its type:

<div class="lang-pair">
<div>

```rust
struct Score(u32);
ctx.world.insert_resource(Score(0));
ctx.world.resource_mut::<Score>().0 += 1;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:resources}}
```

</div>
</div>

C++ resources are plain structs, copied in and out: `resource<T>()` (throws
if the world has none), `try_resource<T>()`, `set_resource(value)` and
`remove_resource<T>()`. Read them before an `each` loop, not inside it (the
world is borrowed while the loop runs). In C: `ponko_resource_set`,
`ponko_resource_get` and `ponko_resource_remove`, by name.

## Borrowing several things at once

A query borrows the whole world while it loops, and so does `resource_mut`.
You don't need to copy values out first, take a resource out and put it
back, or collect entities to change them after the loop. Say up front what
else the code needs, and the world hands it over together. These calls are
checked once, when the borrow is made, and none of them allocates.

**Resources inside a query loop.** `with_resources` carries resources into
the loop: `&T` to read one, `&mut T` to change it, or a tuple of those.

```rust
# struct Speed(f32);
# struct Score(u32);
# ctx.world.insert_resource(Speed(2.0));
# ctx.world.insert_resource(Score(0));
ctx.world
    .query::<&mut ponko::d2::Transform>()
    .with_resources::<(&Speed, &mut Score)>()
    .for_each(|t, (speed, score)| {
        t.position.x += speed.0;
        score.0 += 1;
    });
```

**Several resources together.** `resources_mut` gives one `&mut` per type
of a tuple (up to 8 different types):

```rust
# struct Menu(Vec<String>);
# struct Session(String);
# ctx.world.insert_resource(Menu(Vec::new()));
# ctx.world.insert_resource(Session(String::new()));
let (menu, session) = ctx.world.resources_mut::<(Menu, Session)>();
menu.0.push(session.0.clone());
```

**A resource and the rest of the world.** `resource_scope` hands you one
resource and the world next to it, for code that queries, spawns or reads
other resources while it changes that one:

<div class="lang-pair">
<div>

```rust
struct Total(u32);
#[derive(Component)]
struct Coins(u32);
ctx.world.insert_resource(Total(0));
// The resource and the rest of the world together; Total is back afterwards.
ctx.world.resource_scope::<Total, _>(|world, total| {
    world.query::<&Coins>().for_each(|coins| total.0 += coins.0);
});
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:resource_scope}}
```

</div>
</div>

Inside the scope the resource is not in the world (looking it up finds
nothing). It is back, with your changes, when the scope ends, also on an
early return or a panic. Scopes for different resources can be nested.

**Other entities inside a query loop.** `with_lookup` gives the loop a way
to reach entities by handle: `get` reads, `get_mut` changes. Write what to
fetch from them like a query:

```rust
# #[derive(Component)]
# struct Health(f32);
#[derive(Component)]
struct Bullet {
    target: Entity,
    damage: f32,
}

ctx.world
    .query::<&Bullet>()
    .with_lookup::<&mut Health>()
    .for_each(|bullet, others| {
        if let Some(health) = others.get_mut(bullet.target) {
            health.0 -= bullet.damage;
        }
    });
```

The lookup may read what the query reads (follow a leader's `d2::Transform`
while writing your own `Velocity`). It can't touch a component the query
writes, or write one the query reads: that would be two references to one
value, and it panics when the query is built. A lookup ignores the query's
filters. A write through `get_mut` counts as a change, as with
`world.get_mut`, so a later `Changed<T>` query sees it; for that reason a
lookup can't write a component the query itself filters on with `Added` or
`Changed` (it panics when the query is built).

**A few entities together.** `get_many_mut` borrows the same component of
several entities, for work on a pair that needs both sides. It returns
`None` if one of them is dead or lacks the component, or if you name the
same entity twice:

```rust
# #[derive(Component)]
# struct Health(f32);
# let a = ctx.world.spawn((Health(1.0),));
# let b = ctx.world.spawn((Health(2.0),));
if let Some([a, b]) = ctx.world.get_many_mut::<Health, 2>([a, b]) {
    std::mem::swap(&mut a.0, &mut b.0);
}
```

| You need | Use |
|---|---|
| a resource in a query loop | `query.with_resources::<(&A, &mut B)>()` |
| several resources | `world.resources_mut::<(A, B)>()` |
| one resource and the whole world | `world.resource_scope::<A, _>(\|world, a\| ...)` |
| another entity in a query loop | `query.with_lookup::<&mut T>()` |
| the same component of a few entities | `world.get_many_mut::<T, N>([a, b])` |
| to spawn, despawn, add or remove in a loop | `ctx.commands` (above) |

From C++, `world.resource_scope<T>(fn)` lends `fn` the world and a `T&`
that it may change, and returns what `fn` returns. The resource is back,
with the changes, when `fn` returns or throws (the exception is rethrown). Inside `fn` the world is unlocked, so it can
query (the C++ form of `with_resources`), spawn, and open a scope for another
resource (the form of `resources_mut`). C: `ponko_resource_scope(world, name,
size, fn, user)` copies the resource out, removes it, calls `fn(world, value,
user)`, and puts the value back (a resource of that name added inside the
scope is replaced by it). `with_lookup` and `get_many_mut` stay Rust calls: C
and C++ work on whole columns and component pointers instead (see [C and
C++](c-cpp.md)).

## Sorting for cache locality

`world.sort_by::<T>(key)` reorders entities that have `T` by a `u64` key.
Entity ids stay valid. The built-in spatial grid uses it to keep entities
that are close in space close in memory; see [Spatial queries](spatial.md).
