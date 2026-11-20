# Component hooks (#46)

The question: should a world call functions when a component arrives on an
entity, is replaced, or leaves it (hooks), or call them later from a queue
of events (observers)? Which engine and game work needs either, which is
served by change detection already, and can hooks cost nothing for the
components that have none?

**Decision: ship `on_add`, `on_replace` and `on_remove` hooks per
component type; build no observers.** Hooks serve the one kind of work
change detection can't: bookkeeping that must be right at the moment of
the change (an index other code reads right away), and work that needs the
value that is leaving. Deferred observers would be a second way to do what
`Added`/`Changed` filters and removal logs already do once per tick. The
hierarchy's lost-parent detection moved onto an `on_remove` hook. Physics
cleanup and hierarchy pruning stay batched. Without hooks, spawning compiles
to the same instructions as before; despawn, insert and remove test one
flag; and the benchmarks below show no difference larger than their noise.

## Use cases

| Use case | Where | Needs | Why |
|---|---|---|---|
| Index from a key to entities (names, tags, a registry) | games, tools | hooks: `on_add`, `on_replace`, `on_remove` | Must be right between two systems, and removing a key needs the key: the removal log names the entity, whose value is gone by then; `on_remove` runs while it is still there. |
| Drop the physics body of a despawned entity | 2D and 3D physics | change detection: a sweep in the step | The step's sync query already visits every body and stamps it `seen`; links not seen are dropped in entity index order, so the bodies go in an order that doesn't depend on despawn order (runs stay bit-identical). The physics state lives in `Ctx`, which a hook can't reach, and a hook on `Transform` or `RigidBody` would cost every despawn a call. |
| Take a despawned or detached child out of its parent's list | hierarchy | change detection: batched | One sorted pass per update over the removal log. Pruning per child from a hook searches and shifts the list for each child: clearing a parent of 10,000 children moves about 50M entries, the batch moves each once. |
| Find the children of a despawned parent | hierarchy | hook: `on_remove::<Children>` | Before: when any parent lost its `Children`, the update scanned every link slot (one per entity slot). The hook notes that parent's list while it still exists, and the update looks at those links only. **Moved onto hooks** (`src/hierarchy.rs`, `children_leaving`). |
| Score, sound and effects when a pickup or enemy goes | the reference games (Coin Dash, Ember Hop, Shard Grove) | nothing new | Each game does this at the despawn site, which knows why the entity went. None of them keeps an index of entities. Despawned bodies and children are the engine's job (rows above). |
| React to changes later, as events | systems | change detection | `Added<T>`, `Changed<T>` and `removed::<T>()` already give each system every change once, batched and in order. An observer queue would duplicate them. |

## Design

- **Registration**: `World::on_add::<T>(hook)`, `on_replace::<T>`,
  `on_remove::<T>`: plain function pointers, several per component, run in
  registration order. They are kept per world.
- **What a hook gets**: a `HookWorld`, the world without structural
  changes: `get`, `get_mut`, `query`, resources and `resource_scope`, and
  `commands()`. Its commands go to a queue the world applies when the
  outermost call that ran hooks returns (a `spawn`, `despawn`, `insert`,
  `remove`; a whole `spawn_batch`, `clear` or `apply`). Commands that run
  more hooks are applied after them, first recorded first, until none are
  left. `reset` drops what hooks recorded, since the world starts over.
- **When**: `on_add` after the entity is complete; `on_remove` before the
  value goes, with the entity still whole; `on_replace` before `insert`
  overwrites, with the old value in the world and the new one lent to the
  hook. Hooks of one operation run in component-id order.
- **Zero cost without hooks**:
  - Spawn: the world caches each bundle's archetype index. A bundle whose
    archetype has `on_add` hooks gets the index with its top bit set, so
    the bounds check every spawn already made sends it to a cold path. No
    other bundle checks anything. Registering a hook re-marks the cached
    bundles.
  - Despawn, insert, remove: one flag of the world (`hooked`), set by the
    first registration; then one byte per archetype (despawn) or one entry
    per component (insert, remove).
  - The registry is boxed, so a world without hooks grows by 16 bytes and
    its hot fields keep their offsets.
  - Queries and iteration never look at hooks.
- **Access recording** (the scheduler's checks in debug builds) is paused
  while hooks and their commands run: their work isn't the calling
  system's.

## Soundness

- **No concurrent hooks.** Hooks run only inside structural changes, which
  need `&mut World`. Parallel queries, view systems and parallel command
  recording hold the world shared or split, and can't make structural
  changes, so no hook runs during them.
- **Deterministic order.** The order depends only on what the program did:
  component ids, registration order, and commands in recording order.
  Commands recorded in parallel are applied buffer by buffer in a fixed
  order, so their hooks run in the same order every time (test
  `hooks_run_in_the_same_order_however_commands_were_recorded_in_parallel`).
- **No aliasing.** A hook can't spawn, despawn, insert or remove right away,
  so nothing it does moves or drops the entity the world is working on. The
  new value of `on_replace` is still the caller's, not in the world.
- **Values dropped once, panics contained.** Tests check that every value is
  dropped exactly once around hooks (`values_are_dropped_once_around_hooks`)
  and that a panicking hook leaves the world working, its queue released
  (`a_panicking_hook_leaves_the_world_working`).
- **Miri.** Every ECS test passed under Miri with Stacked Borrows, without
  rayon (the first run AGENTS.md describes; `tests/hooks.rs` included).
  `tests/hooks.rs` also passed with Tree Borrows and rayon, which covers
  its test of commands recorded in parallel. CI's Tree Borrows job runs a
  fixed list of tests that doesn't include it yet.

Tests: `crates/ponko_ecs/tests/hooks.rs` (20), the crate docs' examples,
`tests/hierarchy.rs` (two more for the hook-based detection), and
`crates/ponko_bench/src/hooks.rs` (the benchmark variants do the same work
as the plain ones).

## Measurements

Apple M4 Max (14 cores), macOS 27.2, rustc 1.100.0, release profile,
headless. Other builds ran throughout: load average 50–250. Every row is
the p50 of one run of `ponko_bench`; "pairs" interleave the baseline
binary and the new one, alternating which goes first. The baseline is the
commit before hooks (a299c24b: the hierarchy benchmark added, nothing
else). A sign count ("after slower in 20 of 30") is the test: noise
gives about 15 of 30.

**Noise control.** The baseline against a copy of itself, 30 pairs:

| Row | median after/before | after slower in |
|---|---|---|
| `ecs/spawn-loop-1k/ponko` | 1.0014 | 16/30 |
| `ecs/spawn-loop-100k/ponko` | 0.9978 | 14/30 |
| `ecs/spawn-despawn-1k/ponko` | 1.0083 | 15/30 |
| `ecs/spawn-despawn-100k/ponko` | 0.9996 | 15/30 |

**Spawning one at a time, design by design** (30 pairs each):

| Design | `spawn-loop-1k` | `spawn-loop-100k` |
|---|---|---|
| A flag checked on every spawn | slower in 26/30, +2 to +5 % | slower in 28/30, +2 to +5 % |
| Tagged bundle index (check moved to the function start) | 1.0330, 20/30 | 1.0447, 20/30 |
| Same instructions as before, registry inline | 1.0119, 20/30 | 1.0188, 19/30 |
| Same, registry boxed (final) | 1.0200, 25/30 | 1.0210, 23/30 |
| Final, both builds with functions aligned to 64 bytes | **1.0000, 14/30** | **0.9997, 15/30** |

The first design was a regression and was redesigned. From the third row on,
the spawn path (`spawn_in`, its caller, `Entities::alloc`, `reserve_row`,
the bundle write) compiles to the same instructions as the baseline,
register names aside. The cold path's tail call sits after the return. The
difference left followed where the linker put the code: with every
function aligned the same way in both builds
(`RUSTFLAGS="-C llvm-args=-align-all-functions=6"`), it is gone.

**The other no-hooks rows**, both builds aligned:

| Row | median after/before | after slower in |
|---|---|---|
| `ecs/iter-100k/ponko` | 1.0000 | 6/15 |
| `ecs/add-remove-1k/ponko` | 1.0000 | 7/15 |
| `ecs/add-remove-100k/ponko` | 0.9864 | 6/15 |
| `ecs/spawn-batch-1k/ponko` | 1.0000 / 1.0033 | 7/15, 16/30 |
| `ecs/spawn-batch-100k/ponko` | 1.0087 / 1.0025 | 11/15, 19/30 |
| `ecs/spawn-despawn-1k/ponko` | 1.0000 / 1.0343 / 1.0080 | 13/30, 10/15, 15/30 |
| `ecs/spawn-despawn-100k/ponko` | 1.0238 / 1.0000 / 1.0113 | 18/30, 8/15, 16/30 |

`spawn-batch-100k` leans slower (30 of 45 pairs, +0.3 to +0.9 %). Its loop
is the same 80 instructions as the baseline's, register names aside. That
is at the edge of what this machine resolves; no other row shows a
difference.

**With hooks**, the final build, 16 runs (µs, p50: median of the runs and
their range). `hooks elsewhere`: the world has all three hooks on a
component the workload never uses. `hooked`: the workload's own components
have hooks that count into a resource (`on_add` and `on_remove` on `Pos`
for spawns and despawns, on `Tag` for add/remove).

| Workload | plain | hooks elsewhere | hooked | hooked / plain |
|---|---|---|---|---|
| iter-1k | 0.12 (0.12–0.17) | 0.15 (0.12–0.17) | 0.17 (0.12–0.38) | below timer resolution |
| iter-100k | 19.04 (19.00–20.12) | 19.10 (18.25–41.71) | 19.17 (18.17–42.67) | 1.01 |
| spawn-loop-1k | 16.35 (15.79–17.21) | 16.65 (15.92–40.29) | 24.79 (23.50–26.38) | 1.52 |
| spawn-loop-100k | 1406 (1376–1501) | 1442 (1346–1484) | 2303 (2112–6107) | 1.64 |
| spawn-batch-1k | 12.40 (12.00–13.21) | 12.79 (12.04–30.62) | 19.35 (18.25–21.29) | 1.56 |
| spawn-batch-100k | 1205 (1134–2223) | 1168 (1112–2332) | 1838 (1634–1936) | 1.53 |
| spawn-despawn-1k | 2.54 (2.38–2.71) | 2.54 (2.33–5.79) | 4.21 (4.00–10.04) | 1.66 |
| spawn-despawn-100k | 257 (240–574) | 252 (233–272) | 423 (375–1007) | 1.65 |
| add-remove-1k | 51.10 (49.54–54.79) | 51.92 (47.12–53.54) | 66.98 (62.88–71.38) | 1.31 |
| add-remove-100k | 5384 (5098–17885) | 5361 (4905–9186) | 6951 (6193–12498) | 1.29 |

"Hooks elsewhere" is within the spread of "plain" in every row. A hook
call costs about 8 ns here (spawn-loop: 8.4 µs per 1,000 spawns; churn:
8.3 ns per hook; add/remove: 7.8 ns per hook). That includes the counting
hook's own resource lookup, and the cost doesn't depend on the world's size.

**The hierarchy** (ms; the minimum of each run's samples, because the
p50s swung 0.9–24 ms with the load; 6 pairs, aligned builds):

| Row | before: lowest, median of minimums | after |
|---|---|---|
| `hierarchy/100k-pairs-parent-churn` (parents despawned every update) | 0.510, 1.017 | **0.407, 0.706** |
| `hierarchy/100k-wide-churn` | 0.045, 0.047 | 0.045, 0.047 |
| `hierarchy/100k-deep-moving` | 2.524, 2.537 | 2.481, 2.556 |

`100k-deep-moving` was first 4–15 % slower in every pair. The new
lost-parent pass, inlined into the update, changed how the per-level
placement loop compiled. It runs once per update, and with
`#[inline(never)]` the difference is gone.

## Reproduce

```sh
cargo run --release -p ponko_bench -- 'ecs/'        # plain, hooks elsewhere, hooked
cargo run --release -p ponko_bench -- 'hierarchy/'
cargo test -p ponko_ecs --test hooks
cargo test -p ponko_bench --bin ponko_bench hooks::  # the variants do the same work
```

For a before/after comparison, build the two revisions into separate
binaries and run them in alternating pairs with `PONKO_BENCH_JSON=out.json`.
Align functions in both builds before reading a difference of a few
percent into code changes.
