# ECS benchmarks: methodology and baselines

The ECS matrix compares `ponko_ecs` with `bevy_ecs` and `hecs` on the same
operations at 1k, 100k and 1M entities, plus many small archetypes, query
construction and one game-like tick. The code is
[`src/ecs_suite.rs`](src/ecs_suite.rs) and [`src/archetypes.rs`](src/archetypes.rs).

`ponko_bench` runs the `ponko_ecs` and `hecs` rows. The `bevy_ecs` rows run
in [`bench/compare`](../../bench/compare/README.md), which compiles these
two files as they are and adds bevy's halves, so bevy is not in the engine's
dependency tree: `cd bench/compare && cargo run --release -- ecs` prints and
records all three libraries' rows (`PONKO_BENCH_JSON=...` as below). The
committed baselines were recorded before the move, in one process, as that
command still does.

## The command

```sh
cd bench/compare
PONKO_BENCH_JSON=../../crates/ponko_bench/baselines/ecs.json \
  cargo run --release -- ecs 'ecs/|archetypes-1024|query-build'
```

(`cargo run --release -p ponko_bench -- 'ecs/|archetypes-1024|query-build'`
from the root runs the same rows without bevy's.) It prints one line per result and writes every result, with the build and
machine context, to [`baselines/ecs.json`](baselines/ecs.json) (schema in the
[README](README.md#machine-readable-results)). Re-recording the baselines is
this one command on a clean, committed checkout (so `git_dirty` is `false`),
followed by updating the tables below; then `rm -rf target/release`. It takes
about a minute on an M4 Max. A filter is a substring of the names to run, and
`a|b` runs names containing either, so `-- ecs/par-` runs only the parallel
rows.

The workloads' correctness is checked apart from timing by
`cargo test -p ponko_bench`: for every case, the tests build the same world
in all three libraries at small sizes, run the operation and compare the
results (sorted positions bit for bit, live entities, component counts,
the game tick's whole state), and check the parallel loops against the
serial ones. CI runs those tests; it never runs the timings and has no
wall-clock thresholds.

## Cases

Names are `ecs/<case>-<size>/<library>[-<variant>]`. Entities are
`(Pos, Vel, Spr)` (8 + 8 + 20 bytes) unless stated.

| Case | What one timed run does | Kind |
|---|---|---|
| `iter` | `pos += vel * dt` over every entity, one archetype | steady |
| `par` | steering per entity (`sin`, `sin_cos`, `rem_euclid`), serial and parallel | steady |
| `spawn-loop` | spawn `n` entities into an empty world, one call each | fresh |
| `spawn-batch` | the same with `spawn_batch` | fresh |
| `spawn-despawn` | despawn the oldest `n/10` entities and spawn `n/10` new ones | steady |
| `add-remove` | insert a marker into all `n` entities, then remove it from all | steady |
| `optional` | `(&mut Pos, &Vel, Option<&Boost>)`, four archetypes, half with `Boost` | steady |
| `with` / `without` | `(&mut Pos, &Vel)` filtered by a marker half the entities have | steady |
| `game` | one tick: 4 layouts, 5 systems, projectiles expire and respawn | steady |
| `archetypes-1024` | `pos += vel * dt` over 1024 archetypes of 16 entities | steady |
| `query-build-x1000` | build the `(&mut Pos, &Vel)` query 1000 times | steady |
| `query-build+count-x1000` | build it and count its matches, 1000 times | steady |
| `query-build+count-cold` | build it and count its matches once, in a fresh world | fresh |

The game world is, out of every 20 entities: 10 mobs `(Pos, Vel, Hp, Ai)`,
4 projectiles `(Pos, Vel, Life)`, 5 props `(Pos, Spr)` and 1 pickup
`(Pos, Spr, Tag)`. Its systems run in order: AI steering, movement, health,
lifetime (collect the expired, despawn them, spawn as many replacements) and
a read-only draw pass over `(Pos, Spr)` without `Tag`.

## Method

- **Timing**: wall clock (`Instant`) around each run of the operation.
  *Steady* cases build their state once, run the operation 5 times untimed
  (warm-up), then time every run; *fresh* cases rebuild the state untimed
  before each run and drop 1 warm-up run. Runs per case: 1000/200/100
  (iteration and queries at 1k/100k/1M), 500/100/30 (parallel, game),
  500/50/10 (churn), 200/20/10 (spawning).
- **Statistics**: min, p50, p95, max and mean per case, nearest-rank
  percentiles. The tables show p50 / p95.
- **Allocations**: the runner installs a counting global allocator
  ([`src/alloc.rs`](src/alloc.rs)); every result reports allocations and
  bytes requested per timed run (`allocs_per_run`, `alloc_bytes_per_run`),
  for the whole process, so allocations on worker threads count.
- **Memory**: `world_bytes` is `World::allocated_bytes()` (component columns
  and entity handles) after the timed runs, for ponko only. It is not process
  memory; bevy_ecs and hecs report no comparable number.
- **Load**: `load_average` in the JSON is read when the runner starts.
- **Build**: release profile of the workspace, `ponko_ecs` with its default
  features (`parallel`), `bevy_ecs` 0.20.1 with `default` and
  `multi_threaded`, `hecs` 0.11.2 with its defaults. The versions are pinned
  by the workspace `Cargo.lock` and, for bevy, `bench/compare/Cargo.lock`; the JSON's `env` records them, the
  toolchain, CPU, core count, rayon threads and the load average.
- **Threads**: everything runs on the calling thread except the `par` rows.
  ponko's `t<k>` rows run `par_for_each` inside a rayon pool of `k` threads
  (1, 2, 4, 8 and the core count), entered with `ThreadPool::install`.
  ponko's `par` row calls `par_for_each` straight from the benchmark thread
  on rayon's global pool (one thread per logical core), as bevy's `par` row
  calls `par_iter_mut` on its compute task pool, started once with its
  default size (every logical core).

## Limits of each comparison

- Each library runs its own idiomatic loop: ponko `Query::for_each`, bevy a
  kept `QueryState` with `iter_mut(..).for_each`, hecs
  `query_mut().into_iter().for_each` (its `fold` fast path; the older
  `iter/hecs` row in the README uses a `for` loop, which is slower).
- bevy pays for change ticks on every `&mut` access; ponko only tracks
  changes for components that opted in, and hecs has none. Rows that write
  components include that difference.
- Query matching differs per library. bevy matches archetypes when it builds
  a `QueryState` and keeps them in it. ponko matches when a query runs; its
  per-world archetype index ([QUERY_CACHE.md](QUERY_CACHE.md)) stays off for
  `(&mut Pos, &Vel)` here, which matches every archetype, so every ponko
  query scans. hecs matches when a query runs too, but keeps a per-world
  cache of each query type's matching archetypes, rebuilt only when the
  archetype set changes. So `query-build-x1000` measures different work per
  library (hecs only borrows the world), and in `query-build+count-x1000`
  ponko and bevy match all 1024 archetypes every time while hecs hits its
  cache. `query-build+count-cold` is the row where all three really match:
  the first query of a freshly built world, with each library's bookkeeping
  for it (a few allocations, listed below). It is one query per run, so it
  is short and noisy; compare minimums.
- No scheduler is measured: the game tick calls its systems in a row, in
  every library. System scheduling is in [SCHEDULE.md](SCHEDULE.md).
- hecs has no parallel iteration, so its `par` row is serial only. bevy's
  pool size is fixed once per process, so it has no scaling curve.
- Dispatch differs. ponko's `par_for_each` splits rows into chunks of
  `total / (4 × threads)`, clamped to 512–8192 rows, and runs serially
  when everything fits one chunk. A loop of more than one chunk times its
  first rows on the calling thread and stays there if the whole loop would
  take under about 50 µs (since #181; before, only loops whose archetypes
  all had at most 512 rows were timed, so the baselines below split even
  the 1k `par` world, one archetype, into two chunks). Every ponko row
  hands the work to rayon from a thread outside the pool: with
  `ThreadPool::install` (`t<k>`) the whole loop is sent to a sleeping pool
  thread and the caller blocks; on the global pool (`par`) rayon injects the
  parallel part and the caller blocks without running any of it. bevy's
  `par_iter_mut` splits by its own batch size and runs tasks on the calling
  thread too while it waits. Small worlds measure mostly this dispatch
  cost.
- bevy's structural changes (insert, remove, spawn, despawn) also run its
  component hooks and check for observers, even when none are registered;
  the `spawn-*`, `add-remove` and `spawn-despawn` rows include that.
  ponko and hecs have neither.
- Wall-clock numbers on a shared machine move with its load. Compare runs
  from the same machine, build and load, and prefer min and p50 to p95.

## Baselines (2026-11-22)

Apple M4 Max (14 logical cores), macOS 27.2, on mains power, rustc 1.100.0,
release, revision `9235ea05`. The machine was idle: nothing else was
running, and the runs were one at a time. The command above ran three
times (load average at the start of each: 2.46, 2.25 and 2.05 for the last
minute); `baselines/ecs.json` is the third run, the one closest to the
three runs' medians, and every row below comes from it. The three runs'
medians agreed within 5% in most rows (`bench_track compare` against the
2026-11-20 file used the larger of 5% and twice the spread). p50 / p95 per
timed run:

| Case | Size | ponko_ecs | bevy_ecs | hecs |
|---|---|---:|---:|---:|
| iter | 1k | 0.12 µs / 0.12 µs | 0.12 µs / 0.12 µs | 0.21 µs / 0.21 µs |
| iter | 100k | 16.2 µs / 16.3 µs | 36.1 µs / 40.3 µs | 16.7 µs / 18.6 µs |
| iter | 1M | 167 µs / 175 µs | 372 µs / 396 µs | 182 µs / 191 µs |
| spawn-loop | 1k | 10.0 µs / 10.1 µs | 21.6 µs / 22.1 µs | 16.4 µs / 16.5 µs |
| spawn-loop | 100k | 834 µs / 839 µs | 1.84 ms / 1.85 ms | 1.56 ms / 1.58 ms |
| spawn-loop | 1M | 9.66 ms / 9.85 ms | 20.0 ms / 20.3 ms | 15.8 ms / 16.1 ms |
| spawn-batch | 1k | 5.17 µs / 5.38 µs | 12.2 µs / 15.9 µs | 11.6 µs / 11.7 µs |
| spawn-batch | 100k | 443 µs / 446 µs | 1.18 ms / 1.27 ms | 1.15 ms / 1.16 ms |
| spawn-batch | 1M | 4.80 ms / 4.83 ms | 12.4 ms / 13.0 ms | 12.2 ms / 12.9 ms |
| spawn-despawn | 1k | 1.67 µs / 1.75 µs | 4.92 µs / 5.79 µs | 2.83 µs / 2.96 µs |
| spawn-despawn | 100k | 176 µs / 181 µs | 434 µs / 462 µs | 273 µs / 280 µs |
| spawn-despawn | 1M | 1.83 ms / 1.90 ms | 4.49 ms / 4.57 ms | 2.79 ms / 2.83 ms |
| add-remove | 1k | 43.4 µs / 43.8 µs | 62.1 µs / 65.0 µs | 58.7 µs / 61.2 µs |
| add-remove | 100k | 4.29 ms / 4.35 ms | 8.54 ms / 8.66 ms | 5.80 ms / 5.99 ms |
| add-remove | 1M | 43.9 ms / 44.4 ms | 99.1 ms / 107.7 ms | 59.2 ms / 60.1 ms |
| optional | 1k | 0.21 µs / 0.21 µs | 0.17 µs / 0.17 µs | 0.17 µs / 0.21 µs |
| optional | 100k | 19.2 µs / 19.3 µs | 34.2 µs / 38.1 µs | 17.2 µs / 19.2 µs |
| optional | 1M | 197 µs / 205 µs | 380 µs / 392 µs | 197 µs / 200 µs |
| with | 1k | 0.08 µs / 0.08 µs | 0.04 µs / 0.08 µs | 0.08 µs / 0.12 µs |
| with | 100k | 8.17 µs / 8.25 µs | 17.3 µs / 17.3 µs | 9.33 µs / 9.38 µs |
| with | 1M | 93.2 µs / 93.7 µs | 182 µs / 192 µs | 98.5 µs / 99.8 µs |
| without | 1k | 0.08 µs / 0.08 µs | 0.04 µs / 0.08 µs | 0.08 µs / 0.12 µs |
| without | 100k | 8.25 µs / 8.29 µs | 17.3 µs / 19.3 µs | 9.33 µs / 9.46 µs |
| without | 1M | 77.5 µs / 82.7 µs | 180 µs / 200 µs | 95.2 µs / 96.4 µs |
| game | 10k | 14.2 µs / 17.5 µs | 16.1 µs / 23.7 µs | 15.0 µs / 19.8 µs |
| game | 100k | 139 µs / 170 µs | 164 µs / 244 µs | 144 µs / 193 µs |
| archetypes-1024 | 16k | 4.92 µs / 4.96 µs | 9.46 µs / 9.88 µs | 6.42 µs / 6.88 µs |
| query-build-x1000 | 16k | 18.4 µs / 18.6 µs | 7.87 ms / 8.08 ms | 0.25 µs / 0.29 µs |
| query-build+count-x1000 | 16k | 2.04 ms / 2.06 ms | 8.76 ms / 9.01 ms | 215 µs / 218 µs |
| query-build+count-cold | 16k | 2.25 µs / 2.42 µs | 9.79 µs / 12.1 µs | 12.9 µs / 13.5 µs |
| query-build+count-cold, min | 16k | 2.00 µs | 8.46 µs | 11.5 µs |

On a fresh world, with nothing cached anywhere, matching the 1024 archetypes
once takes ponko about 2.0 µs at the minimum and 2.3 µs at the median; bevy
takes 8.5 µs at the minimum and 9.8 µs at the median, hecs 11.5 and 12.9 µs.

**What moved since 2026-11-20** (revision `e60e15a8`, one run, also on an
idle machine), beyond the threshold above: ponko's spawns (`spawn-loop`
29% faster at 1M, `spawn-batch` 53%, `spawn-despawn` 19%; the work of
#136), `optional` (59% faster at 1M: #136 and #196), `add-remove` at 100k
(12%), every `par` row at 100k and 1M (7–15%), and at 1k the parallel
rows (below). `query-build+count-x1000` went from 2.38 to 2.04 ms (15%)
and `query-build+count-cold` from 2.6 to 2.3 µs. bevy_ecs and hecs moved
too, by 5–13% in some rows (for example bevy's `iter` at 1M, 407 to
372 µs), on code that didn't change: that is the size of the difference
between two idle runs two days apart, and smaller changes in ponko's rows
are within it. One row is slower: `par-1k/ponko-t4`, 10.1 to 12.6 µs.

Parallel scaling, the `par` steering workload, p50 / p95. `ponko par` is
ponko on the global pool without `install`, the setup that matches bevy's:

| Size | ponko serial | ponko t1 | t2 | t4 | t8 | t14 | ponko par | bevy serial | bevy par | hecs serial |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1k | 5.67 µs / 5.92 µs | 10.2 µs / 11.5 µs | 11.4 µs / 13.4 µs | 12.6 µs / 16.4 µs | 15.0 µs / 21.3 µs | 16.0 µs / 20.6 µs | 5.25 µs / 5.88 µs | 5.75 µs / 6.04 µs | 11.2 µs / 18.1 µs | 5.29 µs / 5.83 µs |
| 100k | 566 µs / 570 µs | 540 µs / 574 µs | 290 µs / 305 µs | 161 µs / 178 µs | 130 µs / 165 µs | 99.3 µs / 132 µs | 94.6 µs / 124 µs | 573 µs / 592 µs | 130 µs / 137 µs | 567 µs / 575 µs |
| 1M | 4.87 ms / 5.54 ms | 4.70 ms / 5.53 ms | 2.48 ms / 2.86 ms | 1.30 ms / 1.49 ms | 797 µs / 912 µs | 573 µs / 723 µs | 562 µs / 645 µs | 4.99 ms / 5.71 ms | 731 µs / 840 µs | 4.88 ms / 5.61 ms |

- Over the serial loop, at 1M the curve is nearly linear up to 4 threads
  (3.7×) and reaches 6.1× on 8 and 8.5× on 14; at 100k it reaches 3.5× on
  4 and 5.7× on 14. bevy's pool
  reaches 6.8× at 1M and 4.4× at 100k.
- At 100k and 1M, `ponko par` is as fast as the `t14` row or a little
  faster (6.0× and 8.7× over serial).
- At 1k, `ponko par` now stays on the calling thread (5.3 µs, the serial
  loop's 5.7 µs): a parallel loop times its first rows and keeps a loop
  that small where it is (#181). The `t1` to `t14` rows still take 10–16 µs:
  each runs inside `install` on its own pool, which hands the loop to one
  of that pool's threads and waits for it. bevy takes 11 µs.

Allocations per timed run, where any library made one (all other rows make
none in all three libraries):

| Case | ponko_ecs | bevy_ecs | hecs |
|---:|---:|---:|---:|
| spawn-loop 1k / 100k / 1M | 58 / 93 / 108 | 131 / 215 / 251 | 41 / 83 / 101 |
| spawn-batch 1k / 100k / 1M | 30 / 37 / 40 | 43 / 50 / 53 | 13 / 13 / 13 |
| spawn-despawn 1k / 100k / 1M | 0 | 0.04 / 0.24 / 0.6 | 0 |
| add-remove 1k / 100k / 1M | 0 | 0.01 / 0.08 / 0.1 | 0 |
| par (parallel rows) | 0–0.03 (rayon) | 17.5 (task pool) | — |
| game 10k / 100k | 0.02 / 0.11 | 0.11 / 0.6 | 0.02 / 0.11 |
| query-build(+count)-x1000 | 0 | 29,000 (29 per build) | 0 |
| query-build+count-cold | 1 | 29 | 11 |

The fractional counts are buffers that still grow now and then (bevy's
entity lists, the game's list of expired projectiles, which all three share).
`world_bytes` for ponko: 43 KiB / 4.2 MiB / 42 MiB for `iter` at 1k / 100k / 1M
(44 bytes per entity: 36 bytes of components and 8 of entity handle),
1056 KiB for `archetypes-1024`, 474 KiB / 4.3 MiB for `game` at 10k / 100k.
None of these changed since 2026-11-20.

What the numbers point at, for later ECS work (each needs its own measured
change): the `Option<&T>` gap of the earlier baselines is closed (197 µs
at 1M, as hecs, against bevy's 380 µs). Spawning one entity at a time now
costs about twice `spawn_batch` (1.9× at 1k and 100k, 2.0× at 1M): the
batch got faster by more than the loop did.

## Costs won back since the baselines

Issue #136 took back what required components (#47) and the change
summary (#89) had cost spawns and despawns, and the `Option<&T>` gap
above. Not a re-recording of the baselines: other builds kept the machine
busy (load average 75–185), so only interleaved before/after runs count.

**The rows.** `ponko_bench` from `origin/dev` (before) and from the issue's
branch (after), both built with
`RUSTFLAGS="-C llvm-args=-align-all-functions=6"`, run alternately (the
order flipped every round), 8 rounds. Median of the rounds' p50s, ms; the
ratio is after/before per round (median), and "faster" counts the rounds
where after won:

| Row | before | after | ratio | faster |
|---|---:|---:|---:|---:|
| `ecs/spawn-loop-100k/ponko` | 1.324 | 0.929 | 0.71 | 8/8 |
| `ecs/spawn-batch-100k/ponko` | 1.069 | 0.493 | 0.46 | 8/8 |
| `ecs/spawn-despawn-100k/ponko` | 0.235 | 0.194 | 0.83 | 8/8 |
| `ecs/spawn-loop-100k/ponko-required` | 1.549 | 1.134 | 0.73 | 8/8 |
| `ecs/spawn-batch-100k/ponko-required` | 1.142 | 0.491 | 0.43 | 8/8 |
| `ecs/spawn-despawn-100k/ponko-required` | 0.267 | 0.200 | 0.78 | 7/8 |
| `ecs/spawn-loop-100k/ponko-hooked` | 2.170 | 1.698 | 0.78 | 8/8 |
| `ecs/optional-100k/ponko` | 0.0475 | 0.0215 | 0.45 | 8/8 |
| `ecs/optional-1m/ponko` | 0.507 | 0.220 | 0.43 | 8/8 |
| `ecs/optional-1m/hecs` (unchanged code) | 0.228 | 0.227 | 0.99 | 5/8 |
| `ecs/iter-1m/ponko` | 0.181 | 0.182 | 1.01 | 3/8 |
| `archetypes-1024/ponko` | 0.006 | 0.006 | 1.00 | 1/8 |
| `query-build+count-x1000/ponko` | 2.350 | 2.358 | 1.00 | 3/8 |

The 1M spawn rows moved the same way but swung by 2–5× between rounds
(memory pressure from the other builds); `despawn/ponko` (9.4–82 ms) and
`changes/ponko-despawn-tracked` (14–148 ms) swung too much to read, so the
despawn numbers below come from instruction counts.

**Against the code before #47 and #89.** A harness that links three
copies of `ponko_ecs` (`dc3ca02d`, before both; `origin/dev`; the branch)
and runs the same workloads in one process, interleaved, 40 rounds at 100k
entities, reading instructions and cycles from `proc_pid_rusage`.
Instruction counts are exact; cycles move by about 2% between builds of
the same code. Branch / `dc3ca02d`:

| Workload | instructions | cycles | rounds slower |
|---|---:|---:|---:|
| spawn one by one, `(Pos, Vel, Spr)` | 0.750 | 0.704 | 7/40 |
| `spawn_batch` | 0.503 | 0.550 | 0/40 |
| spawn-and-despawn churn | 0.838 | 0.790 | 4/40 |
| despawn, nothing tracked | 0.957 | 1.012 | 23/40 |
| despawn, `Pos` tracked | 0.968 | 0.965 | 9/40 |
| despawn, `Pos` and `Vel` tracked | 0.991 | 1.030 | 30/40 |

`origin/dev` was at 1.10 instructions and 1.07 cycles (one tracked
component) and 1.14 and 1.17 (two) on the tracked despawns. A spawn that
fills a gap (`Pos` requires `Vel`) runs 39 instructions more than one that
names `Vel` (81 on `origin/dev`): 1.18× its cycles one by one, 0.91× in a
batch, 1.05× in churn.

**`query-build+count-x1000`: 1.5 ms (2026-11-09) to 2.4 ms (2026-11-20).**
The same harness with `ponko_ecs` at every revision between the records
that changed the query code. Instructions per 1000 builds and counts over
1024 archetypes:

| Revision | Minstr |
|---|---:|
| `de3fb5c7` (the 2026-11-09 record) | 33.1 |
| `de3660dd` (#223, the query index) | 36.1 |
| `ec495a4d` (#269, typed `With`/`Without`/`Or` filters) | 64.8 |
| `348c9efb` | 41.4 |
| `9d90dcc3` through `origin/dev` and the branch | 42.2 |

Since 2026-11-11 the count hasn't moved: the code costs about 40
instructions per archetype (a build plus a count over one archetype is
about 420; skipping the index lookup with `uncached()` saves about 840 per
build). The rise came with the typed filters and the index, which test
every archetype against the filters and the occupied-archetype list. The
rest of the gap between records is the machine and the layout: the same
`origin/dev` source measured 8.6 M and 9.6 M cycles in two binaries that
placed it differently, with the same instruction count.

## Change tracking follow-ups (#159)

Measured like the section above, on a machine kept busier still by other
builds (load average 55–195): only interleaved runs and instruction counts
count, and no wall-clock win is claimed below the swing between rounds.
The harness links `dc3ca02d` (before #47 and #89), `origin/dev` at
`52173b15` (after #136) and the branch.

**Parallel loops with `Option` fetches.** `par_for_each`, its indexed form
and `par_collect_into` now split each archetype's rows on which `Option`
components it has, as `for_each` does, and run each chunk in a function
that borrows the closure, so a value the closure captured is loaded once.
`(&mut Pos, &Vel, Option<&Spr>)` over 1M entities in four archetypes, ten
runs on a one-thread pool, instructions per entity (3 rounds, the medians):

| Loop | `origin/dev` | branch | cycles, branch / dev |
|---|---:|---:|---:|
| `par_for_each`, closure captures `dt` | 19.7 | 2.87 (0.145×) | 0.31 |
| `par_for_each`, `move` capture | 17.2 | 2.77 | 0.29 |
| `par_for_each_indexed`, capture | 18.6 | 2.78 | 0.41 |
| `par_for_each`, no capture | 16.7 | 2.75 | 0.49 |
| `for_each` (for reference) | 2.63 | 2.63 | 0.99 |
| `par_for_each` without the `Option` | 2.23 | 2.20 | 0.84 |

The split alone left a capturing closure scalar (16.6 instructions per
entity): the compiler reloaded the capture at every row, because a write
through `&mut Pos` might have changed it. The `ecs/optional-par-*` rows
(new) agree in their fastest runs, 1M entities: 0.72 to 0.22 ms on one
thread, 0.19–0.21 to 0.08–0.09 ms on all cores; their medians swung by
up to 10× between rounds.

**Despawning tracked entities.** The removal logs keep one run per tick
instead of a tick per entity, and a despawn finds its component's log by
index. 100k entities despawned in scattered order, instructions per
entity:

| Tracked components | `dc3ca02d` | `origin/dev` | branch |
|---|---:|---:|---:|
| none | 220 | 208.4 | 208.4 |
| `Pos` | 347.6 | 333.6 | 324 |
| `Pos`, `Vel` | 397.5 | 393 | 370 |

Before #136 the two-tracked despawn ran 1.14× `dc3ca02d`. Cycles of the
scattered despawn are bound by memory and swung from 1.0× to 1.6× between
rounds; despawning from the back (nothing moves) with two tracked
components took 0.90× the cycles of both `dc3ca02d` and `origin/dev`
(244.8 instructions, against 266.9 and 258.9).

**The change summary after a move.** An insert or remove used to mark the
whole block of 64 rows the entity landed in as changed now; the summary
now takes the newer of itself and the moved entity's own stamp, so it
stays exact. An insert and remove with two tracked components costs 1.015×
the instructions; a `Changed<Pos>` loop (100 runs) after 1% of 100k
entities moved costs 0.68× the instructions and 0.76× the cycles.

**Change filters over many archetypes.** A query with `Added`/`Changed`
filters reused nothing between archetypes: a `Changed` loop cost about 494
instructions per archetype, against 174 for a plain loop. The row filter
and the run cursor are now filled in place. 1024 archetypes × 16
entities, `Pos` tracked, 1000 runs, branch / `origin/dev`:

| Workload | instructions | cycles |
|---|---:|---:|
| `Changed<Pos>` loop, nothing changed | 0.774 | 0.67 |
| `Changed<Pos>` loop, 1 in 16 changed | 0.787 | 0.71 |
| `Changed<Pos>` count, nothing changed | 0.597 | 0.45 |
| `Changed<Pos>` count, 1 in 16 changed | 0.682 | |
| plain `&Pos` loop and count, same world | 0.91 | |

The new `ponko_bench` rows, 4 interleaved rounds (after/before, rounds
faster): `archetypes-1024-changed-none` 0.55 (4/4),
`archetypes-1024-changed-1in16` 0.76 (4/4),
`query-build+count-changed-x1000` 0.68 (3/4), `archetypes-1024` 0.83
(4/4); `archetypes-1024-tracked` was within the noise.

**The attachments row.** `attachments` (hierarchy propagation) measured
13 ms on a quiet machine and 60–480 ms here. A steady update takes about
1 ms of work; under load, each parallel pass waited 20–90 ms for worker
threads the system had descheduled, and an update ran about five of them.
Indexing a level of the hierarchy took 0.16–0.23 ms on all cores and
0.15 ms on one, so that pass and the count of placed entities now run on
the calling thread. Timed inside the update, 4 interleaved rounds
(median / p90, ms):

| Step | before | after |
|---|---:|---:|
| whole steady update | 28.7 / 115 | 15.1 / 48 |
| indexing a level | 2.1 / 60 | 0.15 / 0.9 |
| placing | 7.2 / — | 3.7 / — |

The row's median fell from 389–651 ms to 222–374 ms (4/4 rounds). What
remains is the other parallel passes waiting on a loaded machine; on a
quiet one the row should return to about its recorded 13 ms.

## Measured work, not entity counts (#181)

Measured like #159, on a machine kept busier still by other builds (load
average 100–200): only interleaved runs and instruction and cycle counts
count, and wall-clock numbers are quoted only where the change is far
larger than the swing between rounds. Two harnesses, built in release:
one links `ponko_ecs` twice (`origin/dev` at `b2d319ba` and the branch)
and runs the same loops alternately in one process; the other is a
hierarchy driver built once against each tree, the two binaries run
alternately (6 rounds, the order flipped every round). Counters are
`proc_pid_rusage` for the whole process, so they include the pool's
threads, waking and spinning included.

**Parallel loops time their first rows, whatever the archetypes.** A
`par_for_each`, `par_for_each_indexed` or `par_collect_into` of more than
one chunk runs its first 64 rows on the calling thread, timed. If they
already project the loop over 50 µs (and took at least 250 ns, a few ticks
of the clock), the rest goes to the pool at once; otherwise it times up
to 512 rows and, under 50 µs, finishes the loop where it is. Before, only
loops over small archetypes (none over 512 rows) were timed, and a cheap
loop over one big archetype always woke the pool. `par_for_each` over one
archetype on the global pool, per entity, 7 rounds (15 for 1M):

| Loop | Rows | instructions dev / branch | cycles dev / branch | rounds faster |
|---|---:|---:|---:|---:|
| light (`pos += vel * dt`) | 1k | 82.8 / 3.96 | 55.3 / 0.81 | 7/7 |
| light | 4k | 28.1 / 2.59 | 20.4 / 0.50 | 7/7 |
| light | 16k | 12.3 / 2.36 | 9.11 / 0.79 | 7/7 |
| light | 100k | 3.76 / 2.18 | 2.16 / 0.69 | 7/7 |
| light | 1M | 2.52 / 2.50 | 1.24 / 1.25 | 9/15 |
| heavy (steering, about 20 cycles) | 1k | 242 / 118 | 109 / 19.4 | 7/7 |
| heavy | 4k | 179 / 123 | 65.1 / 22.8 | 7/7 |
| heavy | 16k | 139 / 143 | 41.0 / 40.3 | 5/7 |
| heavy | 100k | 129 / 128 | 26.9 / 26.0 | 4/7 |
| heavy | 1M | 125 / 125 | 25.6 / 25.1 | 11/15 |

Loops that now stay on the calling thread cost a fraction of the cycles
(the pool's waking and spinning are gone) and no longer wait for it; the
loops that still go parallel are unchanged within the noise (16k rows and
up of the heavy loop, 1M of the light one). On a quiet machine a loop
just under the budget can take a few microseconds longer on one thread
than on all cores; under load the wait for woken threads is the larger
cost, by far.

**The hierarchy update.** Its passes are the roots compared with where
they were (`par_collect_into`), a level placed in one pass over every
attached entity (`par_collect_into`), and a level placed by lookups. The
first two now measure their work as above; the third times its first 256
children and shares the rest out only when the level projects over 50 µs
(it went parallel from 2048 children before). 2D transforms, an update
per step, medians of the per-round medians (p50 in µs):

| Update | instructions | cycles | rounds faster | p50 dev / branch |
|---|---:|---:|---:|---:|
| 2000 roots × 4 children, still | 0.593 | 0.201 | 6/6 | 12.6 / 4.2 |
| 2000 roots × 4 children, moving | 0.749 | 0.271 | 6/6 | 2014 / 35.1 |
| 500 roots × 4 children, moving | 1.005 | 0.970 | 5/6 | 27.1 / 26.2 |
| 20,000 roots × 1 child, still | 0.778 | 0.508 | 6/6 | 749 / 41.7 |
| 20,000 roots × 1 child, moving | 1.035 | 1.008 | 3/6 | noise |
| 100,000 roots × 1 child, moving | 1.003 | 1.014 | 3/6 | noise |
| 1 root × 100,000 children, moving | 0.976 | 1.081 | 2/6 | noise |
| the same, transforms tracked (lookups) | 0.968 | 0.925 | 4/6 | noise |
| 4000 roots × 4 children, tracked, moving | 0.984 | 1.035 | 2/6 | noise |

A small steady update no longer waits for pool workers: its median fell
from milliseconds (a woken worker the system didn't run) to the time of
its work. The large updates are level with `origin/dev` in instructions
(0.97–1.00); their cycles moved both ways by up to 8% between rounds, and
their wall-clock medians swung by several times, so no change is claimed
for them either way. The fastest single 20,000-root still update was
37 µs on the branch and 29 µs on `origin/dev`: under the budget the pass
stays on one thread even when the pool happened to be awake.

**`par_collect_into` for results that need `Drop`.** Each chunk is filled
in a function that borrows the closure shared, as the other parallel loops
do (`ParWork::visit_collect`), so a value the closure captured is loaded
once and the loop vectorizes. `(&Pos, &Vel)` into a `Vec` of a type with
an (empty) `Drop`, closure capturing `dt`, 1M entities, one-thread pool,
7 rounds: 11.0 to 4.0 instructions per entity (0.36×) and 0.48× the
cycles, 7/7 rounds; the same loop into an `f32` (no `Drop`) is 3.9. With a
`Drop` that does work, clearing the old results adds its own cost to both.
Keeping the count in a local that a guard writes back measured the same
(4.0), so the count stays a plain `&mut`.

**Three and four `Option` fetches: split.** `for_each` and the parallel
loops now pick one loop per combination of `Option` components for three
and four of them too (8 and 16 loops), as for one and two; five or more
still test each row (32 loops and more). `(&mut Pos, &Vel)` plus the
`Option`s, 1M entities spread evenly over the 8 or 16 archetypes, closure
capturing `dt`, 7 rounds, per entity:

| Loop | instructions, per row / split | cycles, per row / split |
|---|---:|---:|
| 3 `Option`s, `for_each` | 23.8 / 16.4 (0.69×) | 3.26–3.35 / 2.08–2.22 (0.60–0.63×) |
| 3 `Option`s, `par_for_each`, one thread | 23.8 / 17.7 (0.74×) | 0.81× |
| 4 `Option`s, `for_each` | 28.3 / 19.7 (0.70×) | 3.89–4.09 / 2.49–2.52 (0.60–0.63×) |
| 4 `Option`s, `par_for_each`, one thread | 27.3 / 20.5 (0.75×) | 0.74× |
| 2 `Option`s (already split), same world | 13.0 | 1.8–1.9 |
| no `Option`, same world | 2.0 | 0.7 |

7/7 rounds faster in each. The cost is code: the parallel chunk function
for this query grew from 872 to 1908 bytes with three `Option`s and from
996 to 3104 with four; the `for_each` caller went from 9584 to 8940 bytes
(three) and 9580 to 11144 (four). One to two KB per loop that uses them,
for about 0.6× the cycles: done.

**Owed to a quiet-machine run (#117).** These rows, touched by #159 and
here, have no number from an idle machine yet; the numbers above are
instruction counts and interleaved ratios. On an idle machine, from a
clean, committed checkout:

```sh
PONKO_BENCH_JSON=crates/ponko_bench/baselines/ecs.json \
  cargo run --release -p ponko_bench -- 'ecs/|archetypes-1024|query-build'
cargo run --release -p ponko_bench -- 'changes/|despawn/|attach/|hierarchy/|par-arch'
```

- `ecs/par-*` (all of them: the 1k rows now stay on the calling thread,
  so the parallel scaling table above changes at 1k) and
  `ecs/optional-par-*` (new in #159);
- `ecs/optional-*`, `archetypes-1024`, `archetypes-1024-tracked`,
  `archetypes-1024-changed-none`, `archetypes-1024-changed-1in16`,
  `query-build+count-changed-x1000` (#159's change filters);
- `changes/ponko-despawn-tracked` and `despawn/ponko` (#159's removal
  logs);
- `attach/100k-spawn+0-ticks`, `attach/100k-spawn+10-ticks` and
  `hierarchy/100k-{deep,wide}-{moving,still}` (the hierarchy passes; the
  attachments row should return to about its recorded 13 ms);
- `par-arch/*` ([PAR_ARCHETYPES.md](PAR_ARCHETYPES.md): single big
  archetypes are now timed too).

## Split `Option` loops that vectorize (#196)

After #181 a loop with two `Option` fetches whose closure combines their
values still cost 13–14 instructions per entity against 2 with none,
although each archetype got its own loop. Measured with a probe built in
release (Rust 1.100.0, Apple M4 Max, a machine kept busy by other builds,
load average 100–170): `(&mut Pos, &Vel)` plus the `Option`s, the closure
multiplying their values (`a.map_or(1.0, |a| a.0) * b.map_or(..) * dt`)
into `pos += vel * s`, 1M entities over the 16 archetypes of four optional
components, `proc_pid_rusage` instruction and cycle counts per entity.

**Why it didn't vectorize.** On worlds where every entity has the same
combination, only "neither component" ran at 2.0 instructions; "first
only" was 9.5, "second only" 9.6 and "both" 14.0. In the generated code
(arm64) the loops for the other three combinations were one loop again,
testing the two column pointers (`cbz`/`cbnz`, both loop-invariant) on
every row, scalar. `fetch_split` made `Some(&*base.add(row))` from a raw
column pointer that LLVM can't prove non-null, so the closure's
`map_or` test survived in the "present" loops; loops that then differ
only by "pointer or null" are what LLVM merges back into one with a
per-row test. With one `Option` the merged loop was small enough to be
unswitched again, so one `Option` vectorized and two did not.

Three approaches were allowed; two were needed:

1. **Each split loop its own `#[inline(never)]` function** (so LLVM can't
   merge them): worse. Every `Option` query fell to 14–16 instructions,
   one `Option` included: the closure's captured `dt`, now behind two
   levels of `&mut`, was loaded again on every row, which blocks
   vectorizing. Dropped.
2. **Tell LLVM a present column isn't null**
   (`std::hint::assert_unchecked(!base.is_null())` in `fetch_split`; a
   column's base is a `NonNull`, dangling when empty or zero-sized): the
   `Option` test folds away in the present loops, they stay apart and
   each vectorizes (`ldp q`/`fmul.4s`). Kept.

Interleaved, `origin/dev` with #181 (`58ccc1a4`) and the branch built as
two binaries, run alternately 3 times, 4 rounds of 20 passes each,
medians per entity:

| Loop | instructions before / after | cycles before / after |
|---|---:|---:|
| no `Option` | 2.01 / 2.01 | 0.78 / 0.81 |
| 1 `Option` | 2.50 / 2.51 | 0.85 / 0.92 |
| 2 `Option`s, combined | 14.00 / 2.83 | 2.01 / 1.00 |
| 2 `Option`s, only the first used | 9.50 / 2.51 | 1.43 / 0.93 |
| 2 `Option`s, one `&mut` written | 15.51 / 2.88 | 2.02 / 1.04 |
| 3 `Option`s, combined | 17.13 / 3.08 | 2.42 / 1.12 |
| 4 `Option`s, combined | 19.65 / 3.31 | 2.90 / 1.18 |
| 2 `Option`s, `par_for_each`, one thread | 14.08 / 2.89 | 2.20 / 1.06 |

The cycle ranges of the rounds don't overlap for any `Option` row with
two or more (2 `Option`s: 1.96–2.39 before, 0.92–1.22 after); the rows
without or with one `Option` are unchanged within the noise. The cost is
code, now that every combination's loop is vectorized: the probe's
`for_each` callers grew from 7920 to 9076 bytes (two `Option`s), 9104 to
11548 (three) and 11380 to 17332 (four), the parallel chunk function from
1244 to 1904 bytes (two). About half the cycles for 1–6 KB per loop that
uses them: kept.
