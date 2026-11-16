# Benchmarks

Started as the M3 ECS spike: it compares a minimal archetype ECS of our own (`src/own.rs`, the spike that became `ponko_ecs`) with `bevy_ecs` 0.20 and `hecs` 0.11. It answers two questions:

1. Which ECS do we build the engine on?
2. **D12 condition:** does routing every component through runtime registration (needed for the C/C++ API) cost the Rust path anything?

```sh
cargo run --release -p ponko_bench            # everything
cargo run --release -p ponko_bench -- iter    # filter by name (substring)
PONKO_BENCH_JSON=results.json cargo run --release -p ponko_bench   # also export JSON
cargo run --release -p ponko_bench -- 'iter|spawn'                   # names containing either
```

**The ECS matrix** (ponko_ecs vs bevy_ecs vs hecs at 1k/100k/1M entities:
iteration, parallel scaling, spawning, churn, optional and filtered queries,
a game-like tick), its methodology, the limits of each comparison and the
committed baselines: [ECS.md](ECS.md). One command re-records them:
`PONKO_BENCH_JSON=crates/ponko_bench/baselines/ecs.json cargo run --release -p ponko_bench -- 'ecs/|archetypes-1024|query-build'`.

Each benchmark prints its median (p50), p95 and minimum. *Steady* benchmarks build their state once, run the operation 5 times untimed, then time it repeatedly (like a system running every frame); *fresh* benchmarks rebuild the state untimed before every run and drop one warmup run. Percentiles are nearest-rank, so with 10 runs p95 is the slowest run.

## Machine-readable results

With `PONKO_BENCH_JSON=<path>`, the runner writes every result it ran (after the filter) to one JSON document:

```json
{
  "schema_version": 1,
  "env": {
    "rustc": "rustc 1.99.1 (58cf8f9c3 2026-10-13)", "git_rev": "<commit>", "git_dirty": false,
    "os": "macos", "arch": "aarch64", "cpu": "Apple M4 Max",
    "logical_cores": 14, "rayon_threads": 14, "debug_assertions": false,
    "ponko_features": ["physics", "physics3d"], "bevy_ecs": "0.20.1",
    "bevy_ecs_features": ["default", "multi_threaded"], "hecs": "0.11.2",
    "filter": "archetypes", "load_average": "9.12 8.40 7.95", "machine": null,
    "unix_time": 1794224632
  },
  "results": [
    {"name": "archetypes-1024/ponko", "kind": "steady", "warmup": 5, "runs": 200,
     "min_ms": 0.0069, "p50_ms": 0.0078, "p95_ms": 0.0088, "max_ms": 0.0090, "mean_ms": 0.0078,
     "world_bytes": 1081344, "allocs_per_run": 0.000, "alloc_bytes_per_run": 0.000}
  ]
}
```

- Times are milliseconds per timed operation. `kind` is `steady` or `fresh` (above).
- `world_bytes` is `World::allocated_bytes()` (component columns and entity handles) after the timed runs, for benchmarks that record it; otherwise `null`. It is not process memory, and bevy_ecs and hecs don't report a comparable number.
- `allocs_per_run` and `alloc_bytes_per_run` are heap allocations (with reallocations) and the bytes they requested, per timed run, counted for the whole process by the runner's global allocator (`src/alloc.rs`); `null` for benchmarks that time themselves.
- `load_average` is the 1, 5 and 15 minute load average when the run started.
- `machine` is `PONKO_BENCH_MACHINE`, or `null`. Leave it unset unless two
  machines share a CPU name: `bench_track` then names the machine
  `<os>-<arch>-<cpu>` (`macos-aarch64-apple-m4-max`), which is how runs from
  the same machine find each other (see [Tracking results over
  time](#tracking-results-over-time)).
- The counting allocator adds two relaxed atomic adds to every allocation, shared by all threads. Rows that allocate a lot from several threads at once can shift slightly against runs from before it was added (before `allocs_per_run` existed); compare such rows only between runs that both have it.
- `rustc` is the `rustc` on `PATH` when the runner starts, which matches the build when run through `cargo run`. Unknown values are `"unknown"` or `null`.
- `schema_version` changes only when a field changes meaning or is removed.

The numbers depend on the machine and its load: compare runs from the same machine and build, and never use them as pass/fail thresholds in CI. The workloads' correctness is checked separately by `cargo test -p ponko_bench`, which builds each new workload in both libraries and checks they contain and update the same entities.

## Tracking results over time

`bench_track` (`src/bin/bench_track/`) keeps a history of results per commit
and machine and flags changes beyond noise. It works with two kinds of
number:

- **Instruction counts** of seven small workloads, counted by cachegrind
  (valgrind): ECS iteration (`ecs/iter/10k`), a filtered query
  (`ecs/query-filtered/10k`), lookups by entity (`ecs/get/10k`), spawning
  and despawning (`ecs/spawn/10k`, `ecs/despawn/10k`), a headless game tick
  (`sim/tick/10k`) and collecting a 3D frame (`render/collect/10k`). A
  count doesn't depend on what else the machine is doing, so it is stable on
  shared CI runners, where wall-clock times are not. Each workload runs once
  with its operation and once without (same setup, same warm-up, same
  loop), and the difference divided by the iterations is the cost of one
  operation. A count is not a time: memory traffic and parallelism don't
  show in it, so the wall-clock runs below stay the measure of speed.
  `bench_track list` describes each workload.
- **Wall-clock runs** of `ponko_bench` on a named machine: its JSON
  documents (`PONKO_BENCH_JSON`), one per run.

```sh
# On Linux with valgrind: counts, 3 runs per workload (the repeats show the spread).
scripts/bench-counts.sh . . new.json            # or: target/release/bench_track counts --repeat 3 --out new.json [FILTER]
# Wall clock, on one quiet machine: 5 runs per side, each its own document.
for i in 1 2 3 4 5; do
  PONKO_BENCH_JSON=before/run-$i.json cargo run --release -p ponko_bench -- 'ecs/'
done
# Compare: files, or folders of repeated runs.
cargo run --release -p ponko_bench --bin bench_track -- compare before/ after/
```

**The comparison.** For every benchmark, `compare` takes the median of each
side's runs and the spread of each side, `(max - min) / median`. A change
counts when it is beyond the threshold, the larger of a floor (1% for
instruction counts, 5% for wall clock; `--floor`) and twice the larger
spread (`--noise-factor`). One wild run widens the threshold instead of
moving the median. The output is a Markdown table with both medians, the
change, both spreads, the threshold and the verdict (**slower**, faster or
nothing), under a first line `<!-- bench-track: slower=N faster=M -->` for
tools. It refuses two different machines (`--any-machine` overrides) and
notes a different compiler, different workloads, or a side with fewer than
three runs, whose spread it can't see. `--fail-on-regression` exits with 1
when something is slower. The rule is tested with synthetic runs, a
regression that must be flagged and noise that must not
(`cargo test -p ponko_bench --bin bench_track`); no test asserts on a
timing.

**In CI** (`.github/workflows/bench.yml`, not part of the required check):

- A pull request that touches the engine, the ECS or the benchmarks gets
  its merge commit and its base counted on the same runner, with the same
  workloads (the pull request's `bench_track` is copied onto the base), 3
  runs each. The table goes to the job summary, and
  `bench-comment.yml` comments it on the pull request when a benchmark moved
  beyond noise, naming the benchmark, or updates its earlier comment.
- Every commit on `main` is counted and recorded on the **`bench-history`
  branch** as `<machine>/<commit>.json` (machine `github-ubuntu-24.04-x64`),
  and the branch's README is regenerated: a table per machine, newest commit
  first, with ▲ and ▼ where a benchmark moved beyond noise against the
  commit before. The branch shares no history with `main`; read it on
  GitHub or with `git fetch origin bench-history`. It needs only the
  workflow's own token, no service or secret.
- A version tag is recorded as `releases/<tag>/<machine>.json` on the same
  branch, and the README lists the releases in their own table.
- `gh workflow run bench.yml -f ref=<revision>` measures an older revision
  with the current workloads (they use only APIs 0.1.0 already had), and
  records it as a release when the revision is a tag. Run it once with
  `ref=v0.1.0` to give 0.1.0 its counts.

`bench_track history <folder>` prints the same README from a local copy of
the branch (`--write` saves it as `README.md`).

**Release baselines on a named machine.** Instruction counts can't see
everything, so every release also records a wall-clock baseline in
`releases/<version>/<machine>/` here, from 5 runs on one idle machine (the
maintainer's checklist in CONTRIBUTING.md, "Releasing", has the command),
and the release notes link it. Compare two releases with
`bench_track compare releases/<old>/<machine> releases/<new>/<machine>`.
0.1.0 predates the folder: its record is [`baselines/ecs.json`](baselines/ecs.json),
one run of the ECS matrix on an Apple M4 Max at commit `90bfe8ec`, the last
re-recording before the 0.1.0 tag (six engine commits came between; the
ones in `crates/ponko_ecs` are listed by
`git log 90bfe8ec..v0.1.0 -- crates/ponko_ecs`). Compare 0.2.0 with it as
`bench_track compare baselines/ecs.json releases/0.2.0/macos-aarch64-apple-m4-max/`,
and mind the note about its single run.

## Results (2026-11-02, Apple M4 Max, release, single thread)

1M entities with `(Pos, Vel, Spr)`; add/remove uses 100k entities. Median of 10 runs (200 for iteration).

| Benchmark | own | hecs | bevy_ecs |
|---|---:|---:|---:|
| spawn 1M (loop) | **12.6 ms** | 16.3 ms | 18.5 ms |
| spawn 1M (batch) | **10.9 ms** | 12.8 ms | 12.6 ms |
| iterate `pos += vel*dt`, 1M | **0.15 ms** | 1.77 ms | 0.36 ms |
| iterate, 1M over 16 archetypes | **0.16 ms** | 1.75 ms | 0.35 ms |
| get by entity, 1M random | **9.7 ms** | 15.8 ms | 13.8 ms |
| add + remove component, 100k | **5.0 ms** | 6.6 ms | 9.1 ms |
| despawn 1M | **7.0 ms** | 9.9 ms | 24.7 ms |
| clean debug build of the dependency | ~0 s (600 lines) | 0.8 s, 5 crates | 9.0 s, 80 crates |

Baselines for the iteration loop:

| | median | min |
|---|---:|---:|
| plain `Vec` loop, buffers grown with `push` | 0.17 ms | 0.15 ms |
| own, typed query (`for_each::<(&mut Pos, &Vel)>`) | 0.15 ms | 0.15 ms |
| own, C-style chunk system on components registered by name/size/align | 0.16 ms | 0.15 ms |

**D12:** typed queries and the C-style column path both run as fast as a hand-written loop, so runtime registration costs the Rust path nothing. C++ support stays.

## Simulation at scale: `par_for_each` (M4, 2026-11-02)

`ponko_ecs` only, M4 Max (14 cores), release. *Light* = `pos += vel*dt`; *heavy* = wander steering with `sin`/`sin_cos`/`rem_euclid` per agent (closer to a real sim tick).

| | 1 core (`for_each`) | all cores (`par_for_each`) | speedup |
|---|---:|---:|---:|
| light, 1M | 0.23 ms | 0.09 ms | 2.5× |
| heavy, 1M | 6.0 ms | 0.93 ms | 6.5× |
| light, 10M | 1.6 ms | 0.72 ms | 2.3× |
| heavy, 10M | 57.6 ms | **8.1 ms** | 7.1× |

Light updates are memory-bandwidth-bound, so extra cores help less. Heavy updates scale with cores: **10M agents fit a 60 Hz tick** (16.7 ms budget).

Parallel queries across few huge versus many small archetypes, with their
crossover thresholds: [PAR_ARCHETYPES.md](PAR_ARCHETYPES.md) (`-- par-arch`).

Sparse, frequently toggled components (many independent tags and effects):
archetypes against a sparse-set prototype and against flags in one dense
component, and why the engine keeps archetype storage only:
[SPARSE_SET.md](SPARSE_SET.md) (`-- sparse-set/`). The group makes a full,
unfiltered benchmark run about 90 s longer, and its largest case peaks at
about 170 MB of resident memory.

Archetype membership and column lookup across widths, at query level hot
and cache-cold, and structural-change costs: [MEMBERSHIP.md](MEMBERSHIP.md)
(`-- membership`).

Static instance batches, GPU-driven against CPU culling, over batch counts
and sizes, occlusion and shadows, with the crossover the renderer uses:
[STATIC_BATCHES.md](STATIC_BATCHES.md) (an offscreen renderer benchmark:
`cargo test --release --lib --features 3d static_batch_benchmark -- --ignored --nocapture`).

Instance cells streamed around a flying camera, one cell at a time against
one batch made again, with upload time per cell and frame-time percentiles:
[INSTANCE_CELLS.md](INSTANCE_CELLS.md) (an offscreen renderer benchmark:
`cargo test --release --lib --features 3d instance_cells_benchmark -- --ignored --nocapture`).

A whole 3D frame of ordinary models (prepare, record, submit, GPU), before
and after the renderer's split into subsystems:
[RENDERER_FRAME.md](RENDERER_FRAME.md) (an offscreen renderer benchmark:
`cargo test --release --lib --features 3d renderer_frame_benchmark -- --ignored --nocapture`).

Levels of detail under a moving camera: picking every `LodGroup`'s level on
the CPU against the same groups as a static batch whose levels the GPU's
culling pass (or the CPU's cell culling) picks, with memory:
[LOD_SPLIT.md](LOD_SPLIT.md) (`--features render -- render/lod`).

What recording a 3D frame's passes into command buffers costs, offscreen:
[RECORDING.md](RECORDING.md) (`--features render -- render/record`).

The rendering baseline: seven offscreen 3D scenes (static, dynamic,
multi-material, shadow-heavy, alpha-heavy, animated, game-like) with CPU
steps, GPU passes, draw calls, memory and frame-time distributions, as JSON
and CSV, and three of them side by side with Bevy:
[RENDER_SCENES.md](RENDER_SCENES.md) (`--features render -- render/scene`).

Whether mesh shaders are worth adopting (issue #164): the support matrix
from wgpu and naga, a meshlet prototype measured against indexed indirect
draws with distance levels, the maintenance and portability cost, and the
recommendation: [MESH_SHADERS.md](MESH_SHADERS.md)
(`cargo test --release --lib --features 3d mesh_shader_benchmark -- --ignored --nocapture`).

Whole systems at the same time (the parallel system scheduler), its
overhead and its crossover: [SCHEDULE.md](SCHEDULE.md) (`-- schedule`).

Hardware ray tracing through wgpu's ray queries: the backend support
matrix, what screen-space reflections and probe GI miss, an isolated
ray-query reflection prototype's cost and memory, and the decision (defer):
[RAY_TRACING.md](RAY_TRACING.md) (offscreen tests:
`cargo test --release --lib --features 3d -- --ignored --nocapture --test-threads=1 ray_query_reflection_cost`).

## Many small archetypes and query construction (2026-11-09)

`ponko_ecs` vs `bevy_ecs` 0.20.1, M4 Max, release, single thread, rustc 1.99.1. The world: 1024 archetypes of 16 entities (16,384 total), each `(Pos, Vel, Spr)` plus a different subset of 10 zero-sized markers. `cargo run --release -p ponko_bench -- archetypes-1024` and `-- query-build`. Medians of 200 runs (iteration) or 100 runs (queries); the machine was shared with other work (load average around 40), so treat these as indicative and rerun before comparing.

| Benchmark | ponko_ecs | bevy_ecs |
|---|---:|---:|
| iterate `pos += vel*dt` over 1024 archetypes | 0.006–0.008 ms | 0.011–0.014 ms |
| build the `(&mut Pos, &Vel)` query, ×1000 | 0.014 ms | 8.3–8.4 ms |
| build the query and count its matches, ×1000 | 1.5 ms | 9.0–9.3 ms |
| ECS storage for this world (`world_bytes`) | 1056 KiB | not reported |

- The two "build" rows measure different work: building a bevy `QueryState` matches it against every archetype, while a ponko `Query` only resolves component ids and matches while it runs. The "build and count" row does the same work in both (resolve, match all 1024 archetypes, sum lengths), so compare that one. bevy systems normally keep their `QueryState` and update it incrementally, which this benchmark does not measure.
- The iteration row builds the query once for bevy (a kept `QueryState`) and once per run for ponko, as each library's systems use them.
- These rows predate hecs in these cases. hecs is in [ECS.md](ECS.md), whose baselines were also recorded under load (load average 12.5 before and 24.5 after the main run; 36.2 before the second run), not on an idle machine. Re-record them with `PONKO_BENCH_JSON=crates/ponko_bench/baselines/ecs.json cargo run --release -p ponko_bench -- 'ecs/|archetypes-1024|query-build'`.

## Caveats

- The spike has no change detection, query filters, parallel iteration or scheduler. `bevy_ecs` pays for change ticks on every `&mut` access, which accounts for part of its 2.4× gap on iteration. When we add change detection, it must be opt-in or column-level so the plain loop stays this fast.
- A plain `Vec` allocated in one go measured *slower* (0.23 ms median) than one grown by `push`. The difference is memory placement, not code: the minimums match. Only compare steady-state numbers.
- `hecs` iteration was ~0.8 ms when timed on a freshly built world and ~1.75 ms in steady state. It's slowest either way, and it has no runtime-defined components, so it was out because of D12 anyway.
- The unsafe code passes its tests under Miri.

## Character controller threshold

`cargo run --release -p ponko_bench --bin characters [-- 8 16 32]` prints
`step_physics3d`'s time per tick with N `Character3D`s walking on a flat
floor (best of 7 runs of 300 ticks). It places `PARALLEL_FROM` in
`src/physics3d/character.rs`; to compare the paths, change that constant and
run it twice. Apple M4 Pro (12 cores, load average about 9), release:

| characters | one thread (ms) | rayon pool (ms) |
|---|---|---|
| 4 | 0.014 | 0.031 |
| 8 | 0.036 | 0.044 |
| 12 | 0.040 | 0.039–0.044 |
| 16 | 0.053 | 0.041–0.047 |
| 24 | 0.079 | 0.051–0.056 |
| 32 | 0.107 | 0.059–0.061 |
| 64 | 0.52–0.54 | 0.226–0.228 |
| 256 | 1.82 | 0.44 |

So characters move on the pool from 16.
