# ECS benchmarks: methodology and baselines

The ECS matrix compares `ponko_ecs` with `bevy_ecs` and `hecs` on the same
operations at 1k, 100k and 1M entities, plus many small archetypes, query
construction and one game-like tick. The code is
[`src/ecs_suite.rs`](src/ecs_suite.rs) and [`src/archetypes.rs`](src/archetypes.rs).

## The command

```sh
PONKO_BENCH_JSON=crates/ponko_bench/baselines/ecs.json \
  cargo run --release -p ponko_bench -- 'ecs/|archetypes-1024|query-build'
```

It prints one line per result and writes every result, with the build and
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
  `multi_threaded`, `hecs` 0.11.2 with its defaults. All versions are pinned
  by the workspace `Cargo.lock`; the JSON's `env` records them, the
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
  `total / (4 × threads)`, clamped to 512–8192 rows, and runs serially only
  when everything fits one chunk; the "stay on the calling thread if the
  loop would take under about 50 µs" check applies only when every matched
  archetype has at most 512 rows. The `par` worlds are one archetype of `n`
  rows, so even at 1k ponko splits them into two chunks. Every ponko row
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

## Baselines (2026-11-20)

Apple M4 Max (14 logical cores), macOS, rustc 1.100.0, release, revision
`e60e15a8`. The machine was idle: nothing else was running, and the runs were
one at a time. The load average (1, 5, 15 minutes) was 15.23, 5.82, 3.52 when
the run started (the JSON's figure; the high 1-minute value is probably the build
just before the run, and the 5- and 15-minute values show the quiet machine).
These replace the numbers of 2026-11-12 (revision `90bfe8e`, load average
1.91 3.47 2.75), which agree with them within a few percent at p50 in most
rows. One run of the command above produced every row here. p50 / p95
per timed run. The p95 columns are noisier than in the 2026-11-12 recording
(for example `iter` at 1M: ponko 272 µs, bevy 600 µs, hecs 348 µs), so read
p50 and the minimums first:

| Case | Size | ponko_ecs | bevy_ecs | hecs |
|---|---|---:|---:|---:|
| iter | 1k | 0.12 µs / 0.12 µs | 0.12 µs / 0.17 µs | 0.21 µs / 0.25 µs |
| iter | 100k | 16.3 µs / 16.5 µs | 40.3 µs / 45.4 µs | 18.6 µs / 19.1 µs |
| iter | 1M | 171 µs / 272 µs | 407 µs / 600 µs | 192 µs / 348 µs |
| spawn-loop | 1k | 13.9 µs / 15.5 µs | 21.1 µs / 24.5 µs | 16.4 µs / 16.6 µs |
| spawn-loop | 100k | 1.20 ms / 1.35 ms | 1.76 ms / 1.90 ms | 1.59 ms / 1.70 ms |
| spawn-loop | 1M | 13.6 ms / 14.2 ms | 19.7 ms / 21.1 ms | 16.4 ms / 17.1 ms |
| spawn-batch | 1k | 10.7 µs / 10.8 µs | 12.5 µs / 12.8 µs | 11.7 µs / 12.5 µs |
| spawn-batch | 100k | 999 µs / 1.07 ms | 1.19 ms / 1.26 ms | 1.15 ms / 1.33 ms |
| spawn-batch | 1M | 10.1 ms / 10.4 ms | 12.9 ms / 13.1 ms | 11.7 ms / 12.2 ms |
| spawn-despawn | 1k | 2.21 µs / 2.29 µs | 4.00 µs / 4.33 µs | 2.96 µs / 3.04 µs |
| spawn-despawn | 100k | 212 µs / 235 µs | 439 µs / 611 µs | 308 µs / 433 µs |
| spawn-despawn | 1M | 2.10 ms / 2.21 ms | 4.58 ms / 4.97 ms | 3.03 ms / 3.14 ms |
| add-remove | 1k | 44.6 µs / 50.6 µs | 62.2 µs / 71.3 µs | 58.5 µs / 66.0 µs |
| add-remove | 100k | 4.82 ms / 5.33 ms | 9.68 ms / 10.6 ms | 6.31 ms / 6.70 ms |
| add-remove | 1M | 44.3 ms / 45.7 ms | 108.1 ms / 114.2 ms | 60.2 ms / 61.2 ms |
| optional | 1k | 0.50 µs / 0.58 µs | 0.17 µs / 0.17 µs | 0.17 µs / 0.21 µs |
| optional | 100k | 43.8 µs / 54.7 µs | 37.4 µs / 52.4 µs | 19.4 µs / 22.9 µs |
| optional | 1M | 480 µs / 588 µs | 399 µs / 469 µs | 210 µs / 350 µs |
| with | 1k | 0.08 µs / 0.12 µs | 0.08 µs / 0.08 µs | 0.12 µs / 0.12 µs |
| with | 100k | 7.58 µs / 10.6 µs | 17.3 µs / 21.0 µs | 9.50 µs / 25.9 µs |
| with | 1M | 96.2 µs / 116 µs | 201 µs / 242 µs | 105 µs / 130 µs |
| without | 1k | 0.08 µs / 0.12 µs | 0.08 µs / 0.08 µs | 0.12 µs / 0.12 µs |
| without | 100k | 8.29 µs / 8.42 µs | 18.6 µs / 20.2 µs | 9.46 µs / 14.2 µs |
| without | 1M | 85.4 µs / 114 µs | 209 µs / 271 µs | 104 µs / 122 µs |
| game | 10k | 14.6 µs / 22.5 µs | 16.4 µs / 28.2 µs | 15.2 µs / 23.5 µs |
| game | 100k | 149 µs / 257 µs | 170 µs / 314 µs | 154 µs / 215 µs |
| archetypes-1024 | 16k | 5.12 µs / 5.25 µs | 9.79 µs / 11.1 µs | 6.33 µs / 6.50 µs |
| query-build-x1000 | 16k | 18.7 µs / 18.9 µs | 8.42 ms / 9.01 ms | 0.25 µs / 0.29 µs |
| query-build+count-x1000 | 16k | 2.38 ms / 2.48 ms | 9.15 ms / 9.81 ms | 216 µs / 245 µs (cache hit) |
| query-build+count-cold | 16k | 2.62 µs / 4.75 µs | 23.3 µs / 41.5 µs | 17.9 µs / 22.9 µs |
| query-build+count-cold, min | 16k | 2.46 µs | 9.92 µs | 13.1 µs |

On a fresh world, with nothing cached anywhere, matching the 1024 archetypes
once takes ponko about 2.5 µs at the minimum and 2.6 µs at the median; bevy
takes 10 µs at the minimum and 23 µs at the median, hecs 13 and 18 µs.

Parallel scaling, the `par` steering workload, p50 / p95. `ponko par` is
ponko on the global pool without `install`, the setup that matches bevy's:

| Size | ponko serial | ponko t1 | t2 | t4 | t8 | t14 | ponko par | bevy serial | bevy par | hecs serial |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1k | 5.92 µs / 7.25 µs | 10.9 µs / 17.7 µs | 11.1 µs / 22.4 µs | 10.1 µs / 19.2 µs | 20.0 µs / 35.4 µs | 28.0 µs / 58.8 µs | 25.5 µs / 60.1 µs | 6.12 µs / 6.38 µs | 10.4 µs / 18.7 µs | 5.88 µs / 6.38 µs |
| 100k | 568 µs / 630 µs | 589 µs / 653 µs | 308 µs / 345 µs | 175 µs / 225 µs | 132 µs / 171 µs | 111 µs / 145 µs | 115 µs / 169 µs | 583 µs / 655 µs | 127 µs / 163 µs | 573 µs / 654 µs |
| 1M | 5.20 ms / 6.04 ms | 5.16 ms / 5.67 ms | 2.61 ms / 2.90 ms | 1.31 ms / 1.51 ms | 732 µs / 856 µs | 647 µs / 830 µs | 606 µs / 666 µs | 5.06 ms / 5.77 ms | 761 µs / 911 µs | 4.98 ms / 5.69 ms |

- At 1M the curve is linear up to 4 threads (4.0×) and reaches 7.1× on 8 and
  8.0× on 14; at 100k it reaches 3.3× on 4 and 5.1× on 14. bevy's pool
  reaches 6.7× at 1M and 4.6× at 100k.
- At 100k and 1M, `ponko par` matches the `t14` row (4.9× and 8.6× over
  serial, against 5.1× and 8.0×); the `install` hop costs nothing measurable
  there.
- At 1k every parallel row is slower than the serial loop (5.9 µs): ponko
  10–11 µs with 1–4 threads and 20–28 µs with 8, 14 or the global pool; bevy
  10 µs. The cost grows with the number of threads woken, not with
  `install`: `ponko par` (no `install`, 14 threads) is as slow as `t14`.

Allocations per timed run, where any library made one (all other rows make
none in all three libraries):

| Case | ponko_ecs | bevy_ecs | hecs |
|---|---:|---:|---:|
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

What the numbers point at, for later ECS work (each needs its own measured
change): an `Option<&T>` query in ponko takes 2.7–2.8× as long as `iter` over
the same number of entities at 100k and 1M, and is about 17–20% slower than
bevy's there; hecs runs the same query in about the time of its own `iter`
(210 µs at 1M, against 480 µs for ponko), so on this row ponko is the slowest
of the three. Spawning one entity at a time costs a fifth to a third more than
`spawn_batch` (1.2× at 100k, 1.3× at 1k and 1M).
