# Randomness, determinism and saving

> **Maturity: [proven](maturity.md).** Game: Ember Hop's autopilot gives the
> same result on Linux, macOS and Windows with `physics-determinism`. Tests:
> `tests/determinism.rs`, `tests/world_hash.rs`, `tests/physics.rs`.
> Platforms: Linux, macOS and Windows, headless (the workspace tests in the
> list of checks). Graphics: not involved.

A simulation that comes out the same every time it runs with the same
inputs is easy to debug, test and compare: replay a bug, check a balance
change against the old version, let an agent run experiments. Ponko
simulations are reproducible by default.

## Random numbers

Get random numbers in systems from `ctx.random()`:

<div class="lang-pair">
<div>

```rust
# #[derive(Component)]
# struct Heading(f32);
fn wander(ctx: &mut Ctx) {
    let random = ctx.random();
    ctx.world.query::<(Entity, &mut Heading)>().par_for_each(|(e, h)| {
        h.0 += random.of(e).range(-0.2, 0.2);
    });
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:random}}
```

</div>
</div>

- `random.of(entity)` is that entity's own stream for this tick and
  system. It doesn't depend on which thread runs the entity or in what
  order, so it's safe in `par_for_each`.
- `random.stream(n)` is for numbers that don't belong to an entity (where to
  spawn food, which event fires).
- Both give an `Rng` with `f32()`, `range(lo, hi)`, `below(n)`,
  `chance(p)`, `angle()`, `unit_vec2()` and `u64()`.

For setup code and tests, `Rng::new(seed)` makes a generator directly.

C++ has the same generator in the header (`ponko::rng`, `ponko::random`),
bit for bit: a C++ system gets exactly the numbers its Rust twin would.

## Seeds

Every game has a seed, `DEFAULT_SEED` unless you set one:

```rust
Game::new("Colony", 1280, 720).seed(42)
```

Same seed, same simulation. For a different world each run, seed from the
clock.

## What keeps runs the same

The engine guarantees, for a fixed seed:

- systems run in the order they were added, every tick;
- queries visit entities in the same order;
- `par_for_each` gives the same results as a plain loop, as long as each
  entity only writes its own components (the borrow rules make you do that);
- the spatial grid, spatial sorting and commands are deterministic;
- system intervals (`system.every(n)`) and `ctx.time.turns` depend only on the tick.

The engine's own tests run a simulation with all of these at once
(threads, grid queries, sorting, reduced rates, spawning, despawning)
and check that two runs give byte-identical saves.

Things that break determinism, and what to do instead:

| Don't | Do |
|---|---|
| `rand::thread_rng()`, time-based randomness in systems | `ctx.random()` |
| frame systems (`system`) changing simulation state | do it in a `sim_system` |
| iterating a `HashMap` to decide things | `BTreeMap`, or sort first |
| reading wall-clock time in sim systems | `ctx.time.sim_tick()`, `ctx.time.sim_time()` |

Floating point is deterministic on one machine and build. Across CPU
architectures, transcendental functions (`sin`, `exp`) can differ in the
last bit. `Rng::unit_vec2()` uses none: it picks a point in a disk with
integers and divides by its length, so it gives the same bits everywhere
(Rust and the C++ header alike). `angle()` is a plain number in
`0..TAU`; turning it into a direction with `sin` and `cos` is where
platforms can differ.

## One number for the state

`ponko::reflect::world_hash(&world)` is one `u64` for everything
[reflected](reflection.md) in the world: every reflected component of every
entity and every reflected resource. Compare it between two runs, between
lockstep peers, or between a game and its [recorded
input](actions.md#recording-live-input) replayed, to find out whether they
are still in the same state:

```rust
use ponko::reflect::world_hash;
# #[derive(Component, serde::Serialize, serde::Deserialize)]
# struct Health(f32);

world.reflect::<Health>();
world.spawn((Health(10.0),));
let before = world_hash(world);
world.query::<&mut Health>().for_each(|h| h.0 -= 1.0);
assert_ne!(world_hash(world), before);
```

- **Bit for bit.** Floats are hashed by their bits, so `-0.0` and `0.0`
  differ.
- **Not order.** Each entity's component is hashed with the entity and the
  results are added up, so where entities sit in memory, the order
  components were added in and the order types were reflected in don't
  matter. Entity ids do: the same spawns in the same order give the same ids.
- **Only reflected state.** Components and resources that aren't reflected
  are left out, so reflect what the game state is made of. Sprites are
  hashed, so a view-only difference (a tint) shows up; hash a world
  without the view's components, or keep your own hash of the rules' state,
  when that matters.
- **Fast.** It reads components in place, word by word, without making
  JSON: Rust components through their `Serialize`, components described
  with `reflect_fields` (C) through their fields. Only components with a
  custom reflection (models, animations) go through their JSON. 100,000
  entities with three reflected components hash in 0.45 ms, where saving
  the same world as JSON takes 680 ms (`cargo run --release -p ponko_bench
  -- world_hash`; Apple M4 Max, idle machine, 2026-11-22, the median of
  three runs within 6%).
- **Everywhere the same,** on every platform and thread count. The value can
  change between engine versions: compare hashes made by the same build.

C++: `world.hash()`; C: `ponko_world_hash(world, &hash)`. Components and
resources described from C (`reflect<T>({...})`) are hashed through their
fields.

It reads `&World`, so it runs inside a system without taking the world
away: [`world.query_ref`](ecs.md#read-only-queries-on-a-shared-world) is the
read-only query it is built on.

## Saving and loading the world

With [reflection](reflection.md), the whole world saves to JSON:

<div class="lang-pair">
<div>

```rust
let save = ctx.world.save();            // ponko::reflect::JsonValue
std::fs::write("save.json", save.to_string())?;

let save: ponko::reflect::JsonValue = ponko::reflect::json::from_str(&std::fs::read_to_string("save.json")?)?;
ctx.world.load(&save)?;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:save}}
```

</div>
</div>

- **Ids are kept.** Loading restores every entity with the id it had, so
  components that refer to other entities (`struct Target(Entity)`) still
  point at the right ones. `Entity` itself saves as its 64-bit id.
- **All or nothing.** `load` checks every component name and value first;
  if anything is wrong, the world is left as it was and you get an error
  saying what.
- **Only reflected components are saved.** The save's `"not_saved"` lists
  the other components by name and count, so nothing goes missing silently.
  Reflected resources are saved too (under `"resources"`); other resources
  and the sim clock aren't. C++ components are saved once
  their fields are described (see [Reflection](reflection.md#c-and-c-components)).
- **For a player's save game**, keep it in the player's storage instead of a
  path of your own: `ctx.storage.save_world(&ctx.world, "slot1")` and
  `ctx.load_world("slot1")` write it crash-safely to the data folder (or the
  browser's storage) and load it as a scene load does; see [Save
  slots](storage.md#save-slots).

Agents can do the same through `ponko-mcp` (`save_world`, `load_world`):
checkpoint, try something, roll back.
