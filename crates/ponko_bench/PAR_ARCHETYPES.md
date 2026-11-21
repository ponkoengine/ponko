# Parallel queries across many small archetypes (#141)

Run `cargo run --release -p ponko_bench --bin ponko_bench -- par-arch`
(`src/par_archetypes.rs`). Every workload has `L` matching archetypes of
`R` entities each (`LxR`; `+N` adds one more archetype of `N` entities),
all `(Pos, Vel)` plus a different subset of zero-sized markers. *Light* is
`pos += vel * dt`; *heavy* is four `sin_cos` per entity (about 25 ns).
`seq` is `for_each` on one core, `par` is `par_for_each`, `collect` is
`par_collect_into` into a reused `Vec`. Steady benchmarks: 5 warmup runs,
then 200 timed runs (50 from 500k entities up).

## What changed

Before, `par_for_each_indexed` (and so `par_for_each` and
`par_collect_into`) cut each archetype into chunks and waited for it before
starting the next one, and ran every archetype of at most 512 rows on the
calling thread. Many small archetypes therefore used one core, and a few big
ones paid one parallel join per archetype.

Now the calling thread lines up the matching rows of all archetypes as
(archetype, row range, first query position) entries, stamping change ticks
and applying `Added`/`Changed` row filters as before, and one parallel pass
runs fixed-size chunks of the positions: a chunk can hold many small
archetypes or part of a big one. Positions, and the output order of
`par_collect_into`, are exactly as before. The plan's storage (32 bytes per
entry) is owned by the world and reused, so warm parallel queries allocate
nothing (`tests/parallel_alloc.rs`). After a burst (a `Changed` query with
many short runs over a big world), it shrinks: every 64 plans, storage over
4096 entries that is more than four times the largest plan of those 64 is
cut to twice that plan. Plans of a steady size never reallocate.

Thresholds:

| Case | Behavior |
|---|---|
| total rows ≤ one chunk (`clamp(total / (4 × threads), 512, 8192)`) | calling thread (unchanged) |
| some archetype has more than 512 matching rows | parallel plan (until #181, see below) |
| all archetypes ≤ 512 rows, more than one chunk | first 512 rows run on the calling thread, timed; if the whole query projects under **50 µs**, the rest runs there too (as before), otherwise in parallel |
| the same on the web (no clock) | calling thread (unchanged) |

Since #181 every loop of more than one chunk is timed, whatever the size
of its archetypes: its first 64 rows, and if those already project it over
50 µs (and took long enough to read on the clock) the rest goes parallel at
once; otherwise up to 512 rows, as above. On the web, which has no clock,
the table above still holds. The measurements are in
[ECS.md](ECS.md#measured-work-not-entity-counts-181); the tables below
predate it.

## Measurements (2026-11-10)

Apple M4 Max (14 cores, 14 rayon threads), macOS, aarch64-apple-darwin,
rustc 1.99.1 (58cf8f9c3), Cargo release profile, default benchmark
features. *Old* is `929d125` (main plus this benchmark), *new* is the
branch's final code. Both binaries were built back to back and run
alternately, round by round, so both saw the same machine load. GPU metrics
are not applicable.

Other builds were running on the machine (load averages are given per run).
Waking rayon's threads takes longer under load, so parallel times of small
queries (≈ 0.01 ms) are noisy. The `seq` (`for_each`) cells run code this
change doesn't touch: their old/new spread shows the noise of each run.

### Speedups (five paired rounds, load 15–43, best median in ms)

Under one-sided interference (other processes only ever slow a run down)
the best of the five per-round medians is the most stable figure.

| Workload | seq | old par | new par | new vs old |
|---|---:|---:|---:|---:|
| 1x1000000 light | 0.743 | 0.129 | 0.120 | 1.08× |
| 1x1000000 heavy | 25.1 | 2.86 | 2.69 | 1.06× |
| 4x250000 light | 0.746 | 0.231 | 0.121 | **1.9×** |
| 4x250000 heavy | 26.6 | 3.41 | 2.88 | 1.19× |
| 4096x256 light | 0.926 | 0.860 | 0.187 | **4.6×** |
| 4096x256 heavy | 30.6 | 29.4 | 3.27 | **9.0×** |
| 4096x16 heavy | 1.65 | 1.63 | 0.311 | **5.2×** |
| 1024x64 heavy | 1.68 | 1.67 | 0.287 | **5.8×** |
| 2048x16 heavy | 0.820 | 0.815 | 0.186 | **4.4×** |
| 1024x16 heavy | 0.416 | 0.414 | 0.101 | **4.1×** |
| 512x16 heavy | 0.201 | 0.199 | 0.093 | **2.1×** |
| 256x16 heavy | 0.104 | 0.105 | 0.083 | 1.26× |
| 64x8 heavy (one chunk) | 0.0115 | 0.0116 | 0.0118 | 0.99× |
| 1024x64+500000 light | 0.426 | 0.145 | 0.091 | **1.6×** |
| 1024x64+500000 heavy | 14.6 | 3.08 | 1.63 | **1.9×** |
| `par_collect_into` 4096x256 light | – | 0.641 | 0.137 | **4.7×** |
| `par_collect_into` 4x250000 light | – | 0.214 | 0.091 | **2.3×** |
| cached query 4096x16 light | – | 0.048 | 0.038 | 1.26× |

### Single archetypes and cheap small queries (seven paired rounds, load 7–15)

The regression check: medians of seven per-round medians, in ms, at the
quietest load available.

| Workload | old | new | new vs old |
|---|---:|---:|---:|
| 1x1000000 light par | 0.1270 | 0.1204 | 1.06× |
| 1x1000000 heavy par | 2.669 | 2.728 | 0.98× |
| 1x1000000 collect | 0.0999 | 0.0935 | 1.07× |
| 1x65536 light par | 0.0500 | 0.0501 | 1.00× |
| 1x65536 heavy par | 0.2766 | 0.2783 | 0.99× |
| 1x65536 collect | 0.0500 | 0.0478 | 1.04× |
| 1x16384 light par | 0.0427 | 0.0416 | 1.03× |
| 1x16384 heavy par | 0.0882 | 0.0872 | 1.01× |
| 1x16384 collect | 0.0407 | 0.0400 | 1.02× |
| 1x4096 light par | 0.0278 | 0.0299 | 0.93× |
| 1x4096 heavy par | 0.0586 | 0.0573 | 1.02× |
| 1x4096 collect | 0.0276 | 0.0271 | 1.02× |
| 1x1024 light par | 0.0178 | 0.0177 | 1.01× |
| 1x1024 heavy par | 0.0378 | 0.0379 | 1.00× |
| 1x1024 collect | 0.0168 | 0.0173 | 0.97× |
| 4096x16 light par | 0.0640 | 0.0658 | 0.97× |
| 4096x16 light collect | 0.0462 | 0.0622 | 0.74× |
| 2048x16 light par / collect | 0.0320 / 0.0231 | 0.0356 / 0.0260 | 0.90× / 0.89× |
| 1024x16 light par / collect | 0.0160 / 0.0116 | 0.0177 / 0.0130 | 0.90× / 0.89× |
| 512x16 light par / collect | 0.0080 / 0.0058 | 0.0090 / 0.0065 | 0.90× / 0.89× |
| 256x16 light par / collect | 0.0040 / 0.0029 | 0.0045 / 0.0031 | 0.90× / 0.92× |

- **Single archetypes: no regression.** Every `par` and `collect` cell is
  within 0.93–1.07×, and the untouched `seq` cells of the same rounds spread
  0.92–1.05×. The 0.80× seen for 1x4096 heavy in an earlier best-of-3 was
  noise: here it is 1.02×.
- **Cheap queries over small archetypes cost 8–11% more** (0.4–3.6 µs): the
  plan reads every archetype once before any row runs, and `visit` resolves
  its columns again. They still run on the calling thread, as before.
- **4096x16 light collect (0.74×)** sits at the 50 µs budget: its first
  chunk sometimes projects just over it, and then pays for waking the pool
  for no gain. Queries well below or above the budget don't see this.

### The first build, without the timed fallback

The first paired run measured the plan running every query over more than
4096 rows of small archetypes in parallel. Cheap work lost there:

| Workload | old par | plan, always parallel |
|---|---:|---:|
| 512x16 light (8k rows) | 0.0079 | 0.0192 (0.41×) |
| 1024x16 light (16k) | 0.0160 | 0.0442 (0.36×) |
| 2048x16 light (32k) | 0.0320 | 0.0460 (0.70×) |
| 4096x16 light (64k) | 0.0640 | 0.0620 (1.03×) |

while heavy work at the same sizes already ran 2.4–5.7× faster. The
crossover is a matter of time, not rows: it sits where the serial loop takes
about 50–60 µs (4096x16 light, 1x1024–1x4096 heavy). That is the budget the
timed first chunk compares against.

## Crossovers and tradeoffs

- **Many small archetypes:** parallel pays off once the loop takes more than
  about 50 µs on one core. Below that the query stays on the calling thread,
  as before. Heavy work crosses over at a few thousand rows (256x16 heavy,
  100 µs serial: 1.26×; 512x16 heavy, 200 µs: 2.1×); light work at about
  64k rows.
- **Cost of the fallback:** cheap queries over small archetypes that stay on
  the calling thread take 8–11% longer than before (0.4–3.6 µs from 256 to
  4096 archetypes; about 2 ns per archetype): building the plan reads every
  archetype once more before any row runs. Queries that fit one chunk skip
  the timing but not the plan. Near the 50 µs budget a query may go to the
  pool for little gain (4096x16 light collect: 0.74×).
- **Tried and dropped: running small archetypes while planning.** A version
  ran runs of at most 512 rows on the calling thread as the plan builder
  found them (one pass, as the old loop), planning the rest only after
  25 µs. In a paired run (load 13–42) cheap queries cost as much as now
  (0.85–0.89×), and heavy work lost most of its gain because at least 25 µs
  always ran serially first (1024x16 heavy 1.7× instead of 4.1×, 256x16
  heavy 0.82×). It was reverted.
- **Timing makes the choice of thread, not the result:** positions, output
  order and change stamps are the same either way. A query slowed down by
  load during its timed first chunk may run the rest in parallel.
- **Single archetypes:** same chunks and same threading decision as before,
  but the query now goes through the plan: a fixed cost per call (one plan
  entry, the storage handed back) and per chunk (a binary search for its
  first entry and one column lookup). That cost is within run-to-run noise
  (seven paired rounds above). As before, `par_for_each` over one archetype
  goes parallel from 513 rows, which costs a few microseconds for very cheap
  work (1x4096 light: 0.003 ms serial vs 0.006 ms parallel); `for_each`
  stays the right choice there.
- **Reproducibility:** for queries over small archetypes only, the choice
  between the calling thread and the pool depends on measured time, so the
  threads and order that run `f` can differ run to run even on small
  worlds. Order-dependent side effects in `f` (pushing into a shared `Vec`,
  summing floats into shared state) are not reproducible; per-entity
  results, `par_for_each_indexed` positions and `par_collect_into` order
  are.
- **The web:** `std::time::Instant` isn't available on `wasm32-unknown-unknown`,
  so queries over small archetypes keep running on the calling thread there.

## Correctness

`crates/ponko_ecs/tests/parallel.rs` checks positions, `par_for_each`
visits and `par_collect_into` output (plain and owning results) against
`iter()` order, with cheap and with slow per-entity work, for many tiny, a
few huge, mixed and empty archetypes, `with`/`without`/`Or` filters,
required and optional components, the cached query path, `Changed` runs
across many archetypes, mutable loops, and mapper panics in the timed first
chunk and in the parallel part. Each query meant to reach the workers asserts
more than four chunks of rows and uses slow per-entity work, and
`a_slow_query_over_tiny_archetypes_runs_on_more_than_one_thread` checks that
more than one thread really ran. `tests/parallel_alloc.rs` checks that warm
parallel queries don't allocate on the querying thread; unit tests in
`query.rs` check that the plan storage shrinks after a burst and keeps a
steady size. The unsafe code
passes under Miri (`parallel` and `changes` with tree borrows, and every
test without the `parallel` feature).
