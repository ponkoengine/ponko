# Agent instructions

Rules for any coding agent (Claude Code, Codex, Cursor, ...) working in this repo.

## Git workflow (required)

1. **Never work directly on `main`.** Before changing anything, create a branch from an up-to-date `main`:
   - `git switch main && git pull && git switch -c <type>/<short-name>`
   - There is also an integration branch, `dev`: agent work is based on `origin/dev`, and maintainers bring `dev` into `main` through a pull request.
   - Branch names: `m<N>/<topic>` for roadmap milestone work (e.g. `m3/ecs-crate`), `fix/<topic>`, `chore/<topic>`, `docs/<topic>`.
   - One branch per task. Start unrelated work on its own branch (use `git worktree` if another branch is in progress).
2. **Commit and push along the way**, not only at the end. Make a commit at every meaningful, working step (it builds, and tests pass) and push it right away (`git push -u origin <branch>` the first time).
   - Commit messages: short imperative subject line, then a body listing what changed.
   - Never commit `docs/`, which holds local planning notes and is git-ignored. Never commit `.DS_Store`, build output or secrets.
   - Run `scripts/leak-check.sh install` once per checkout. The hook checks every commit against a private list of words that must stay out of this public repo (check a PR's title and description with `scripts/leak-check.sh text <file>`). If it stops a commit, take the private details out and describe the change generically. Don't weaken or skip the check.
3. **Everything reaches `main` through a pull request.** Never push to `main` and never merge locally into `main`.
   - **One PR per big chunk of work**, not per small feature or fix: keep committing and pushing on the branch, and open the PR when a whole part is done (for example a milestone's worth: reflection + control endpoint + MCP server). Don't stack many small PRs.
   - Open the PR with `gh pr create --base main`. The body says what changed and how it was verified (commands run, on which OS, benchmark numbers).
   - **There is no CI.** Nothing runs on a pull request or on `main` by itself: you run the checks on your own machine before the PR is merged. The quick ones are in [Integration checks](#integration-checks); run the applicable jobs from `scripts/checks.yml` with `python3 scripts/run-checks.py`. For risky platform changes, run the affected jobs on macOS, Windows and Linux, or ask a maintainer who has the machine.
   - Merge with `gh pr merge --squash --delete-branch` once the checks pass, then `git switch main && git pull`. A PR doesn't have to be rebased on the newest `main` to merge.
   - `main` is protected: a pull request is required and history stays linear (squash merges). Never bypass protection, even as admin.
   - If a check fails, fix it on the branch and push again; don't merge red. If something on `main` is found broken, fixing it comes first: open a fix PR right away.
   - **Don't edit `CHANGELOG.md` in a PR.** Add a fragment, `changelog.d/<pr-or-topic>.md`, with `### Added` / `### Changed` / `### Fixed` sections and bullets (see `changelog.d/README.md`). The release folds them in with `python3 scripts/changelog.py`.
4. Don't rewrite published history (`push --force` on shared branches, rebasing `main`).

## Tests

Where tests live:

| What | Where | Example |
|---|---|---|
| Public API of a crate | `crates/<crate>/tests/<area>.rs`, one file per area | `crates/ponko_ecs/tests/queries.rs` |
| Shared test helpers | `tests/common/mod.rs` next to them | components used by several ECS test files |
| Private internals (clock math, schedule order) | `#[cfg(test)] mod tests` at the bottom of the file | `src/time.rs` |
| Usage examples in docs | doctests for public usage and behavior where useful | `World::spawn` |
| Engine and game logic | headless runs, no window or GPU (`Game::run_headless`) | `tests/` in the root crate |
| Performance | `crates/ponko_bench` (`cargo run --release -p ponko_bench [-- filter]`) | `iter/ponko` |

Rules:
- New public behavior gets a test in the matching area file. A new area is just a new file: `cargo test --workspace` picks it up. Only a new feature combination needs its own step in `scripts/checks.yml`.
- Test names say what must hold: `stale_handles_stay_dead`, not `test_entity_2`.
- Panics that are part of the API get `#[should_panic(expected = "...")]` tests, so the message stays helpful.
- Benchmarks are not tests: never assert on timings in tests, except for coarse bounds such as "≥ 6 ms for three 2 ms steps".
- Run one area locally with `cargo test -p ponko_ecs --test queries`.
- Offscreen GPU tests (`src/render/*_render_test.rs`) follow the book's "GPU regression tests" (`book/src/render-tests.md`): devices through `render_test_support`, semantic assertions with a negative control, tolerances instead of byte equality across adapters. Reference images in `tests/fixtures/render` change only with `PONKO_UPDATE_REFERENCES=1 PONKO_REFERENCE_REASON="..."`, the before/after images in the pull request, and the `references-reviewed` label from a maintainer.

## Integration checks

Contributors may submit focused Rust-only changes with relevant tests; do not
require them to implement C/C++ bindings, write guide chapters, install unrelated
toolchains or run the entire platform matrix. Help identify and own follow-ups.
For docs-only changes, check Markdown, links and the book when affected; no
workspace rebuild is needed unless executable examples or code changed.

For code changes, maintainers and coding agents run the relevant full checks
before declaring the PR ready to merge. Use the local inventory runner.

```sh
python3 scripts/run-checks.py --list
python3 scripts/run-checks.py --quick
python3 scripts/run-checks.py --job test --job test-features
python3 scripts/run-checks.py
```

Review skipped checks and failures in `target/check-results/summary.txt`. The
full failure output is in `results.json` alongside it. Windowed checks require
explicit opt-in. See [Local checks](CONTRIBUTING.md#local-checks) for tools and
nightly scheduling.

If you add or change a Cargo feature or a web smoke runner, give it its lines
in `scripts/checks.yml` and run `python3 scripts/check-feature-coverage.py`.
Run the feature's own commands locally. CONTRIBUTING.md, "Adding or changing
a cargo feature", has the details. Use the relevant inventory checks for
changes to the panic audit, maturity levels, unsafe code and C pointer
ownership.

`collect-panics.py --check` fails for a new or changed `panic!`, `assert!`,
`unwrap` or `expect` in the engine or ECS production code. Review the site
(`--help`, and "Panics" in `book/src/contributing.md`): a message that names
the misuse and the fix, an `Error`, or an `invariant` recorded with its reason.
Don't bless a site without looking at it.

If you touched unsafe code in `crates/ponko_ecs`, also run the ECS tests under Miri:

```sh
cd crates/ponko_ecs
# Our unsafe code, strict model, without rayon:
cargo +nightly miri test --tests --no-default-features --features derive
# The parallel path. rayon's dependency crossbeam-epoch trips Stacked Borrows
# (not our code), and its global thread pool outlives main:
MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-ignore-leaks" cargo +nightly miri test --test parallel --test changes --test view_groups --test query_cache --test query_cache_views --test hooks --test required
```

If you touched unsafe code outside the ECS, run its Miri tests (the `miri`
lines of `scripts/unsafe-inventory.txt`), then delete `target/miri`:

```sh
python3 scripts/check-unsafe.py --miri | while read -r line; do
  MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-ignore-leaks" bash -c "$line" || break
done
```

If you touched `crates/ponko_ffi`, `include/` or `examples/cpp`, also run the C
and C++ test programs and the `ponko_ffi` tests under AddressSanitizer and
UndefinedBehaviorSanitizer: the commands are in CONTRIBUTING.md, "Unsafe code
and sanitizers".

## Project rules

- Performance claims need numbers: measure in release builds and put the results in the PR.
- No per-frame heap allocations in steady state on hot paths.
- Every `unsafe` block needs a test that exercises it.
- Public API: simple first. One obvious way to do each thing.
- Rust-only contributions are welcome. Guide updates and C/C++ coverage are
  project goals maintained collectively, not mandatory work for every author.
  Maintainers may complete them in the PR or record and own scoped follow-ups;
  do not make an unrelated language/toolchain a barrier to a focused contribution.
- Add or update nearby Rust API documentation when needed to explain changed
  behavior. A standalone Rust PR need not add a guide chapter, a doctest for
  every public item, native bindings or paired guide examples.
- When writing a paired guide example, use `<div class="lang-pair">` and pull
  the C++ snippet from `examples/cpp/guide.cpp` so the native check compiles it. A
  Rust-only guide example is acceptable; label native coverage gaps honestly.
- Preserve existing Rust/C/C++ compatibility. If a change touches existing
  bindings or layouts, maintainers must verify them before merging. New Rust
  features may land before new bindings; record the coverage gap and owner.
- Keep documentation accurate: correct claims invalidated by the change or
  clearly note the limitation, with maintainer-owned follow-up for broader
  guide coverage. Run `mdbook build book` when editing the guide. Reuse tested
  examples through `{{#include}}` anchors where practical.
- Inline Rust snippets in the book must compile (run
  `python3 scripts/check-book-snippets.py` when you edit the guide). Give a
  snippet the context it needs on hidden `# ` lines; `ignore` is a last resort
  (at most three in the book) and needs a `// ignore: <reason>` comment. See
  "Rust snippets compile" in `book/src/contributing.md`.

## Performance work

Measure first, in release builds; the book's "Profiling a frame"
(`book/src/performance.md`) has the details.
- Benchmark a game at a fixed camera: `PONKO_BENCH=600 PONKO_BENCH_OUT=x.json
  PONKO_BENCH_MESHES=x.csv cargo run --release`. It prints fps and
  percentiles, CPU vs GPU, GPU time per pass, triangles for the camera and
  the shadows, the slowest systems and the heaviest meshes, then quits.
  Frames are drawn offscreen, so the display's refresh rate doesn't cap them.
- Where the time and allocations go: `profile_scope!("name")` around suspect code,
  `Game::profile_headless(warmup, ticks, setup)` for a `PerfReport` (systems, scopes,
  allocations with `CountingAllocator`, worst frames, warnings), `report.assert_within(&Budget)`
  in tests; `PONKO_BENCH_REPORT=now.json` saves one from a benchmark run. See
  `book/src/profiling.md`.
- Hitches: `PONKO_TRACE=trace.json` (a Perfetto trace of every system,
  render, display wait and GPU pass); look for the slow frames in it.
- A running game (`PONKO_CONTROL=1`): the MCP tools `perf_report`,
  `record_trace` and `perf_hud`.
- CPU hot spots: `sample <pid> 5` on macOS, or `--features profiling` with
  puffin_viewer.
- Read the numbers before changing anything: separate display pacing/wait from
  CPU and GPU work. Use GPU pass timings and `render_stats` to investigate
  triangles (camera or shadows) and pixel cost; do not infer a GPU bottleneck
  from display wait alone.

## Architecture and scope

Read [PROJECT_VISION.md](PROJECT_VISION.md), [ARCHITECTURE.md](ARCHITECTURE.md),
[ROADMAP.md](ROADMAP.md) and [STABILITY.md](STABILITY.md) before architectural
work. Inspect the affected implementation and tests; a roadmap item describes
future work, not proof that an API exists. Discuss major design changes in an
issue before implementing them, and deliver small, independently verifiable steps.

- Preserve the integrated public workflow (`Game::new()`, `.sim_system()`,
  `.physics()`, `.run()`) and current compatibility promises: Rust APIs, C ABI,
  saved worlds, determinism and MCP contracts. Document conflicts separately;
  prefer additive designs rather than weakening stability guarantees.
- Keep ownership and dependencies explicit within each subsystem. Change the
  narrowest responsible module; avoid unrelated ECS, renderer, networking or UI
  rewrites. Separate internal responsibilities before extracting public crates.
- Before adding a crate, dependency, trait, interface or plugin abstraction,
  explain the concrete need, why existing modules are insufficient, the benefit
  to performance/testing/maintenance/extensibility, its complexity and platform
  costs, and the smaller alternative considered. Avoid dependency cycles,
  hidden hot-path allocations and speculative replacement frameworks.
- Preserve compile-time feature selection and headless builds. Optional tooling
  must not impose GPU, window or audio dependencies on minimal configurations.
  Common operations need sensible defaults; advanced extension points stay optional.
- Keep archetype storage and the wgpu renderer. Proposed system scheduling is
  optional and requires sound access metadata, explicit ordering, deferred-command
  and event barriers, and documented determinism tradeoffs. Never share arbitrary
  mutable `Ctx` or `World` access concurrently to implement parallel systems.
- Renderer refactors preserve resource ownership, lifetimes and pass ordering.
  Establish regression and performance baselines first; prefer small pass
  dependencies and targeted extension points over a full renderer rewrite.

## Evidence and agent responsibility

Measure representative release workloads before and after performance changes.
Record commands, revision, toolchain, features, hardware, workload and warm-up;
include CPU/GPU cost, memory, allocations and frame-time percentiles where relevant.
Mark unavailable metrics explicitly. A microbenchmark does not establish overall
engine superiority, and correctness must be verified separately from throughput.

For GPU changes, inspect comparable before/after images as well as test results.
Record adapter/backend, driver, resolution and quality settings; validate supported
capabilities and fallbacks. An unsupported adapter skip or headless test is not
visual proof. Preserve desktop and web behavior and report untested platforms.

Use existing MCP/runtime inspection, screenshots, headless stepping and profiling
when appropriate to reproduce and validate behavior. Verify tool availability and
contracts before calling them; keep a reproducible code/test path alongside runtime
evidence. Do not describe proposed tools or planned scheduler features as shipped.

Agents follow the same review, tests, privacy and leak checks as human contributors.
Inspect generated changes, identify documentation/binding follow-ups, and
report exact checks, failures, skips and remaining limitations. Never claim a build,
visual result, passing check or completed issue without observing the evidence. Keep
private projects, assets, screenshots and identifying details out of public changes,
issue text, benchmark artifacts and external services.
