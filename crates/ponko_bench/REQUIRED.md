# Required components (#47)

The question: should a component type name the components an entity with
it needs, with starting values, so that a spawn or insert adds the ones it
lacks? Can that cost nothing for the worlds and bundles that don't use it,
and does it remove boilerplate from the tutorials?

**Decision: ship it.** `#[component(requires(Transform2D, Velocity))]`, or
`Type = expression` for a value other than `Default`; `Component::requires`
by hand; `World::require` at run time; `RawWorld::require_dynamic` and
`ponko_component_require` for C and C++. The engine's physics bodies require
a transform and a velocity, and colliders a body (fixed unless given). The
tutorials lost 8 lines of boilerplate. Without requirements, and for bundles
that lack nothing, spawning runs the same instructions as before; an insert
tests one more byte of the world; the paired benchmarks below show no
difference larger than this machine's noise.

## Design

- **Declaring.** `Component::requires(&mut Requires)` gets a default body
  that does nothing; the derive writes it from `#[component(requires(..))]`.
  It runs once per world, when the type is registered. `World::require`
  adds one later, for types from other crates; it applies from then on.
  `RawWorld::require_dynamic` copies a byte template, so its component
  must have no destructor (it panics otherwise, naming `World::require`).
- **What's added.** The missing components of the closure: breadth first
  through what required components require; the nearest requirement's
  value wins, and at the same depth the component earlier in the bundle.
  Cycles are fine. What the bundle gives, or the entity has, is kept.
- **When.** In the same structural change as the spawn or insert, so the
  entity never exists without them and `on_add` hooks see it complete
  (and run for the required components too). Required components count as
  added for change detection.
- **Where it hooks in.** Requirements live with the hooks (#46) in the
  world's boxed registry and set the same `hooked` flag; archetypes whose
  components require something get a third flag bit.
  - Spawn: each bundle's archetype is cached by type. A bundle that lacks a
    required component is cached with the archetype of its components plus
    the required ones, and its index is tagged like a bundle with `on_add`
    hooks, so the bounds check every spawn already makes sends it to the
    cold path, which makes the missing values first. Its list of
    (column, constructor) is cached next to it. Untagged bundles run the
    code they ran before.
  - Insert: one early test of the `hooked` byte; only a world with hooks or
    requirements looks up whether the inserted component requires
    something, and only then goes to a cold path that makes the missing
    values and moves the entity once. Components with only hooks keep
    #46's path (its hook check at the end stays as it was).
  - Dynamic and C spawns: the archetype's flag byte, read only when the
    world is `hooked`.
- **Panics.** Defaults are made before anything moves (inserts) or into a
  reserved row whose entity id is freed if a constructor panics (spawns):
  the world stays usable and nothing is dropped twice (tests below).

## Tutorials (lines before → after)

| File | Before | After | What went |
|---|---|---|---|
| `examples/tutorial_2d/game.rs` | 504 | 502 | the crab's `Velocity::default()`, the sensors' `insert(e, RigidBody::fixed())` |
| `examples/tutorial_3d/game.rs` | 445 | 439 | the yard's body and transform, the crates' `Velocity::default()`, the posts' `insert(post, RigidBody::fixed())` |

The guide chapters for both tutorials and the physics chapters say what a
body and a collider bring.

## Soundness

- Every value is dropped once (`required_values_are_dropped_once`), a
  panicking constructor leaves the world working
  (`a_panicking_constructor_leaves_the_world_working`).
- Requirements are registered only with `&mut World`, so no spawn runs
  while they change; bundle caches are rebuilt in archetype order, so ids
  don't depend on hash order.
- Miri: see [Miri](#miri).

Tests: `crates/ponko_ecs/tests/required.rs` (22), the derive's renamed-crate
test, `crates/ponko_bench/src/required.rs` (the variants make the same
entities), `examples/cpp/tests.cpp` (`required_components`), and the
headless tutorial and physics tests.

## Measurements

Apple M4 Max (14 cores), macOS 27.2, rustc 1.100.0, release profile,
headless. **The machine was very busy throughout** (load average 60–170,
other agents' builds): single numbers swing by 2× and only paired runs
mean anything. "Pairs" interleave the baseline binary (4a69c18e, #46's
head) and the new one, alternating which goes first; both built with
`RUSTFLAGS="-C llvm-args=-align-all-functions=6"`, as HOOKS.md found
necessary. A sign count is the test: noise gives about half. The 1k rows
are p50s of a few µs at the timer's 1 µs resolution, so many pairs tie.

**Instructions.** In the new binary, compared with the baseline after
normalizing addresses and register names:

- `spawn_batch::<(Pos, Vel, Spr)>`'s loop: the same 80 instructions.
- The benchmark closures for spawning, churn, add/remove and iteration:
  the same.
- `bundle_archetype`'s cached path: the same; its uncached path moved into
  `place_bundle`.
- Insert (`insert_dynamic_inner`): one `ldrb` of the `hooked` byte and a
  branch at the start, then the old code (#46's end check included).

**No requirements in the world** (`ponko` rows):

| Row | median after/before | after slower in |
|---|---|---|
| `ecs/spawn-loop-1k/ponko` | 1.0056 | 16/30 |
| `ecs/spawn-loop-100k/ponko` | 0.9894 | 10/30 |
| `ecs/spawn-batch-1k/ponko` | 1.0000 | 14/30 |
| `ecs/spawn-batch-100k/ponko` | 0.9743 | 14/30 |
| `ecs/spawn-despawn-1k/ponko` | 0.9736 | 8/30 |
| `ecs/spawn-despawn-100k/ponko` | 0.9661 | 11/30 |
| `ecs/iter-100k/ponko` | 1.0023 | 7/13 |
| `ecs/add-remove-1k/ponko` | 1.0000 / 0.9889 | 7/13, 6/14 |
| `ecs/add-remove-100k/ponko` | 1.0415 / 1.0323 | 7/13, 10/14 |

**Hooks, as #46 left them** (the `hooked` flag is set, so inserts also look
up requirements):

| Row | median after/before | after slower in |
|---|---|---|
| `ecs/spawn-loop-1k/ponko-hooks-elsewhere` | 0.9907 | 12/30 |
| `ecs/spawn-loop-100k/ponko-hooks-elsewhere` | 1.0005 | 15/30 |
| `ecs/spawn-despawn-1k/ponko-hooked` | 1.0001 | 15/30 |
| `ecs/spawn-despawn-100k/ponko-hooked` | 1.0056 | 20/30 |
| `ecs/add-remove-1k/ponko-hooks-elsewhere` | 1.0086 / 0.9939 | 8/13, 5/14 |
| `ecs/add-remove-100k/ponko-hooks-elsewhere` | 1.0240 | 8/14 |
| `ecs/add-remove-1k/ponko-hooked` | 1.0156 | 11/14 |
| `ecs/add-remove-100k/ponko-hooked` | 1.0320 | 8/14 |

`add-remove-100k` leans slower by 2–4 % in medians, but its sign counts
(7/13, 10/14, 8/14) are what noise gives, and another 8-pair run under a
load spike ranged from 0.74 to 1.02. The two added instructions run once
per insert, next to a hash lookup and a row move; this machine can't
resolve a difference that small. A first design sent inserts of hooked
components through the requirements' cold path and was 10–12 % slower
(13 of 13 pairs); it was changed to the path above.

**Using requirements.** The same binary, `ponko-required` (spawns
`(Pos, Spr)` and `Pos` requires the `Vel` the plain row spawns) against
`ponko` (spawns `(Pos, Vel, Spr)`): the same entities. 9 runs, p50 in µs,
median of the runs:

| Workload | given in the bundle | filled by the requirement | ratio (median) |
|---|---|---|---|
| spawn-loop-1k | 15.58 | 17.92 | 1.14 |
| spawn-loop-100k | 1343 | 1603 | 1.18 |
| spawn-batch-1k | 11.83 | 12.38 | 1.06 |
| spawn-batch-100k | 1102 | 1193 | 1.04 |
| spawn-despawn-1k | 2.38 | 2.67 | 1.13 |
| spawn-despawn-100k | 238 | 268 | 1.14 |

Filling a gap costs about 2.6 ns per spawn here (the cold path writes
column by column through function pointers instead of one inlined bundle
write), less in batches. `ponko-requires-elsewhere` (a component the
workload never uses requires another) is the "no requirements" case for
every spawn: its bundles lack nothing.



## Miri

Both runs from AGENTS.md, one test binary at a time (nightly Miri, on the
merge with `dev`):

- **Stacked Borrows, without rayon**: the library's tests and all 31 test
  files pass. It found one bug, since fixed: a byte requirement's `Make`
  kept a pointer into a `Box<[u8]>` that was then moved into the registry,
  and moving a `Box` asserts unique access, which invalidated the pointer
  (`dynamic_spawns_and_inserts_get_what_is_required`). The template is now
  owned through a raw pointer (`hooks::Template`), and `required` (22) and
  `hooks` (19 without rayon) pass again.
- **Tree Borrows, with rayon**: AGENTS.md's list (`parallel` 20, `changes`
  61, `view_groups` 16, `query_cache` 3 of 20, the rest ignored under
  Miri as before, `query_cache_views` 4), plus `required` (22) and
  `hooks` (20): all pass. CI's Tree Borrows job doesn't list `required`
  yet; the Stacked Borrows job runs it with every test file.

## Reproduce

```sh
cargo run --release -p ponko_bench -- 'ecs/'     # plain, requires-elsewhere, required
cargo test -p ponko_ecs --test required
cargo test -p ponko_bench --bin ponko_bench required::
```

For a before/after comparison, build both revisions with
`RUSTFLAGS="-C llvm-args=-align-all-functions=6"` into separate binaries and
run them in alternating pairs with `PONKO_BENCH_JSON=out.json`.
