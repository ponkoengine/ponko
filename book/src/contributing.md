# Contributing

<!-- Maturity: not a feature. How to contribute. -->

Start with [`CONTRIBUTING.md`](../../CONTRIBUTING.md): your first change in
four steps, which checks a change needs, how to claim an issue and how fast
maintainers reply. This chapter is the reference behind it: the local checks,
cargo features, unsafe code and sanitizers, GPU evidence and the other rules
a change in one area needs. Read the part you need when you need it.
API snapshots, semver checks and the dependency audit are in
[`MAINTAINING.md`](../../MAINTAINING.md), the release procedure in
[`RELEASING.md`](../../RELEASING.md), the maintainer and agent integration
checklist in [`AGENTS.md`](../../AGENTS.md), and how decisions are made and
who can merge in [`GOVERNANCE.md`](../../GOVERNANCE.md).

In short:

- Talk in [GitHub Discussions](https://github.com/ponkoengine/ponko/discussions).
  A new issue or an outside pull request gets a first reply within three
  days, and a pull request a review within a week
  ([How fast we reply](../../CONTRIBUTING.md#how-fast-we-reply)). Everyone is welcome, and no
  question or contribution is too small.
- To take an issue, comment on it; a maintainer assigns it to you and nobody
  else takes it ([Claiming an issue](../../CONTRIBUTING.md#claiming-an-issue)).
- Work on a branch from `main` (there is no other integration branch), open a
  pull request to `main`, and run the checks your change needs locally (there
  is no CI).
- Add relevant tests for changed behavior and helpful nearby API comments.
  You do not need a doctest for every item or unrelated local toolchains.
- Performance claims need release-build numbers in the PR.
- Rust-only changes are welcome; guide chapters and C/C++ bindings are not
  required from every contributor. Maintainers coordinate that coverage and
  own follow-ups. Preserve existing compatibility and describe behavior and tests.

## Repository layout

Cargo packages use `ponko` / `ponko_*`; the old native names are gone, and legacy runtime environment variable names remain supported. Code
uses `include/ponko.h`, `include/ponko.hpp`, and `PONKO_*` settings. See
[the naming migration](naming.md) for precedence and compatibility.

| Path | What's there |
|---|---|
| `src/` | The engine (`ponko` crate) |
| `crates/ponko_ecs` | The `ponko_ecs` ECS, usable on its own |
| `crates/ponko_macros` | Derive macros (`#[derive(Component)]`), re-exported by `ponko_ecs` and `ponko` |
| `crates/ponko_ffi` | The C API; `include/ponko.h` is generated from it |
| `include/ponko.hpp` | The header-only C++ wrapper |
| `crates/ponko_mcp` | The MCP server for AI agents |
| `crates/ponko_bench` | Benchmarks (`cargo run --release -p ponko_bench [-- filter]`) |
| `examples/`, `examples/cpp/` | Rust and C++ examples |
| `tests/`, `crates/*/tests/` | Tests, one file per area |
| `fuzz/` | Fuzz targets for the loaders of untrusted files and packets ([fuzz/README.md](../../fuzz/README.md)) |
| `book/` | The guide ([mdBook](https://rust-lang.github.io/mdBook/)) |

## Running local checks

For a Rust change, format it and run the affected package or test, for example:

```sh
cargo fmt --all --check
cargo test -p ponko_ecs --test queries  # an ECS query change
cargo test -p ponko --test headless    # a headless simulation change
```

`cargo fmt --all` hands every target's file of the workspace to one `rustfmt`
command; on Windows, in a checkout with a long path, that passes the
command-line limit ("The filename or extension is too long (os error 206)").
`python3 scripts/fmt-workspace.py --check` checks the same files in batches
and works everywhere; the format check in `scripts/checks.yml` uses it.

On a card with little memory the renderer's GPU tests can fail with `Not enough memory left`: add `-- --test-threads=4` (see [A card with little memory](render-tests.md#a-card-with-little-memory)).

For guide changes, run `mdbook build book` if mdBook is available. You do not
need a C++ toolchain for an unrelated Rust change. Report checks you could not
run; maintainers cover the broader matrix before merging.

### The list of checks

Run the applicable checks from [scripts/checks.yml](../../scripts/checks.yml) with
Python 3.10 or later, Git, and Bash. On Windows use Git for Windows and start
from a Visual Studio Developer PowerShell with the C++ linker and SDK on
`PATH`. Replace `python3` with `py -3` if needed. The runner uses the same
Python interpreter for its steps. Install the required Rust toolchains and
tools before the run.

```sh
python3 scripts/run-checks.py --list
python3 scripts/run-checks.py --quick
python3 scripts/run-checks.py --job test --job test-features
python3 scripts/run-checks.py --tag lint
python3 scripts/run-checks.py
```

`--job` and `--tag` can each be repeated. Combine them to select jobs that
match both filters. `--list` prints the plan with prerequisites and skip
reasons. The default runs the current OS lanes without opening windows.
On a test machine with a display, add `--include-windowed` to include packaged
game playback and other steps tagged `windowed`. Offscreen GPU tests can run
in either mode.

Setup actions (tool installers, system packages, caches and artifact uploads)
are for machine provisioning. Use the versions and requirements in the
inventory. On Linux install the ALSA and udev development headers before any
native builds and AT-SPI plus D-Bus for the Linux accessibility lane. For the
Linux browser job, install the pinned wasm-bindgen
CLI, run `npm ci --prefix web/smoke`, and provision the Playwright browsers and
OS dependencies listed in that job. The C++ browser check requires
`WASI_SDK_PATH`. The WebKit audio checks expect a PulseAudio null sink on the
test host.

The runner prints each step's result and time and writes `results.json` and
`summary.txt` to `target/check-results/` by default. Use `--out DIRECTORY` to
save a report elsewhere. Reports include the checked revision, time, OS,
Rust versions, skipped checks, and adapter and driver details when offscreen
tests report them. The JSON also includes failed step output. Exit code 1
means a check failed, and exit code 2 means a usage error or zero checks ran.
The [integration instructions](../../AGENTS.md#integration-checks) describe when
maintainers need additional checks.

### Checks that need a person

Some checks need a real machine, display or device. [scripts/checklists/](../../scripts/checklists/README.md)
has a checklist and record sheet for each (hardware checks per OS, a
newcomer test, input methods and screen readers, low-end hardware and the
Steam Deck, the whole list on Linux), with scripts that make the runs quick.
If you have the hardware, pick one and report on its issue.

The nightly runs on maintainer machines are in
[`MAINTAINING.md`](../../MAINTAINING.md#nightly-runs-on-maintainer-machines).

## Adding a cargo feature

The list of checks is [scripts/checks.yml](../../scripts/checks.yml): a change
breaks nothing unseen only if some line there builds it. Every cargo feature of
every crate in the workspace needs, in checks.yml,

- a **clippy** line that turns it on (a `matrix.include` entry of the `clippy`
  job: its `scope` is the package, `args` the features),
- a **test** line that turns it on (a step of the `test-features` job, meant
  for Linux, macOS and Windows),
- and, if docs.rs builds the crate with it (`[package.metadata.docs.rs]`), a
  **doc** line (the "Docs with every public feature" step of the `docs` job).

`python3 scripts/check-feature-coverage.py` reads Cargo.toml and checks.yml and
fails for a feature without one; `--list` shows each feature's checks. A feature
counts as on in a line when the line names it, when a feature that is on lists
it (`image-formats` turns on `image-bmp`), when it is a default feature, or when
a crate built together with it asks for it (cargo unifies features: building the
workspace builds `ponko` with whatever `ponko_lobby` and `ponko_ffi` ask of it).
The checks that go with a feature are in the same file:

| What | Where in checks.yml |
|---|---|
| `editor`, `video`, `lobby`, `webrtc` and the others, linted and tested | `clippy` matrix; `test-features` steps "Tests, editor", "Tests, video", "Tests, lobby client and WebRTC endpoints" |
| The C and C++ test programs against a library with `webrtc` | `native`: `--features control-env,webrtc` with `-DPONKO_WEBRTC=ON` |
| The ECS hooks and required components under Miri (Tree Borrows) | `miri`, "par_for_each, tree borrows" |
| The engine's unsafe code under Miri (the `miri` tests of `scripts/unsafe-inventory.txt`) | `miri`, "engine unsafe code, tree borrows" |
| The C and C++ test programs and the `ponko_ffi` tests under AddressSanitizer and UndefinedBehaviorSanitizer | `sanitizers` |
| The threaded web build (with `-D warnings`, and its self-test `--case warning`), its fallback without the isolation headers, and its controllers (`--case gamepads`) | `web-smoke`: `web/smoke/threads.mjs` |
| Browser multiplayer and replication | `web-smoke`: `web/smoke/multiplayer.mjs`, `web/smoke/replication.mjs` |

Only if a feature has nothing a cargo test can check, put it in
`scripts/feature-checks.txt` with a reason that names what checks it instead.
The script also fails for an entry that is not needed any more, so the list
only shrinks. The same file lists the browser runners (`web/smoke`,
`web/gallery`) that checks.yml doesn't run; a new runner without a step in
checks.yml fails the check.

Run it, with the selftest of its parsing, before a change to Cargo.toml's
features or to checks.yml:

```sh
python3 scripts/check-feature-coverage.py
python3 scripts/check-feature-coverage.py --selftest
```

## Changing the renderer

The renderer's offscreen GPU tests, their reference images and what a
failure means on other hardware are described in
[GPU regression tests](render-tests.md). Run them on your GPU and under
`PONKO_GPU_TIER=baseline` and `webgl2` before a pull request, and say in it
which adapter they ran on and what was skipped.

## Reproductions and GPU evidence

A useful bug report includes a minimal runnable example or failing test, commands,
engine revision, Rust version, feature flags, expected and actual results, and logs.
For nondeterministic failures, include the seed or input sequence when available.
For rendering, add OS, GPU/driver/backend, resolution and quality settings, plus
screenshots when safe to share. Never attach private project assets or secrets.

For GPU changes, run relevant offscreen/visual tests and inspect comparable
before/after images. Exercise capability fallbacks and resize/history behavior
when affected. Record skipped tests and unsupported adapters; a headless logic
test does not establish rendering correctness. If hardware is unavailable, say
which validation is missing in the PR so reviewers can help.

The offscreen GPU suites live in `src/render/*_render_test.rs`. Their shared
conventions (getting the adapter with a logged skip reason, tolerance-based image
comparison and semantic pixel checks, reference images with metadata under
`tests/fixtures/render/`) are documented in `src/render/render_test_support.rs`.
Run them with `cargo test -p ponko --lib --features 3d render_test -- --nocapture`
and report which ran and which logged `render test SKIPPED`. Reference images are
only rewritten with `PONKO_UPDATE_REFERENCES=1` (refused when the `CI`
environment variable is set); a regenerated
reference needs the before/after images in the PR.

Performance PRs should record the revision, commands, toolchain, features, hardware,
workload, warm-up and comparable before/after results. Include CPU and GPU times,
frame-time percentiles, memory and allocations where relevant, and identify metrics
that could not be collected. Verify correctness independently; never generalize
one benchmark into an overall engine ranking.

## Testing the C API

Every function the C library exports (`extern "C" fn` in `crates/ponko_ffi/src`)
needs a test that calls it, in `crates/ponko_ffi/tests/` or a native suite in
`examples/cpp/`: a doctest that passes a null pointer proves argument checking,
not that the function works. `python3 scripts/check-ffi-coverage.py`
which fails for an export no test calls. The only exceptions are listed, each
with the reason it can't run on a machine without a window or a Steam client,
in `scripts/ffi-coverage-allowlist.txt`. Test the function instead of adding to
that list; the check also fails when a listed function gets a test, so the
list only shrinks.

## Unsafe code

Every `unsafe` block needs a test that runs it. The ECS checks its own under
Miri; for the rest of the workspace, `scripts/unsafe-inventory.txt` lists
each item with unsafe code, the test that runs it, and whether that test runs
under Miri (an interpreter that catches undefined behaviour) or why it can't:
it needs a GPU, a C library, signals or sockets, or another platform.
`python3 scripts/check-unsafe.py` fails for unsafe code that has no line,
and prints the line to add; `--list` shows every item with its test, and
`--miri` the commands that run the Miri tests. The C API crate's helpers need
no line when a tested C function reaches them; the script finds that test.
The benchmark binary (`crates/ponko_bench`) is held to the same rule: its
prototype worlds, counting allocator and raw-column loops have tests, and
most run under Miri. A component registered from C replicates through
`Replication::raw_component`; its read and write code is tested over the
in-memory scripted network, so it runs under Miri too.

A few checks need a person, because they open a real window or need a
system's own screen reader. They are written down, compile with the rest
and do nothing unless asked:

| Check | Command | What it runs |
|---|---|---|
| The accessibility adapter on the engine's window (macOS, Windows) | `cargo test --features accessibility --test ui_access_window -- --ignored` | `Game::run` attaches the adapter (`for_window`, and through it `for_ns_view` or `for_hwnd`) to a window placed far off the screen; the system's accessibility API presses a button through it |
| The same on macOS, in view | `PONKO_ACCESS_WINDOW=1 cargo test --features accessibility --test ui_access_macos engine_window` | the sample menu read and operated through the window's content view |
| A screen reader's own speech | by hand, see [Screen reader records](ui-accessibility.md) | VoiceOver, Narrator, NVDA, Orca |

In `scripts/unsafe-inventory.txt` such a check is named like any test, with
`window` for why it can't run under Miri. A check of a test program without
the harness (`harness = false`, for code that needs the main thread) is
named by its function: `tests/ui_access_window.rs::the_engine_window_...`.

Code that only runs on iOS is checked on the simulator, on a Mac with Xcode:
`scripts/ios-simulator.sh` runs the engine's iOS unit tests in a simulated
iPhone and then an app that needs a real scene (see [iOS](ios.md)). Its
lines name such a test, or the script itself for what only an app reaches,
with `ios` for why it can't run under Miri.

The C API is one long unsafe boundary, so its tests also run under
AddressSanitizer and UndefinedBehaviorSanitizer: the C and C++ test programs
(`-DPONKO_SANITIZE=ON` in `examples/cpp`) and the `ponko_ffi` tests. The
commands are in [Running the unsafe-code checks](#running-the-unsafe-code-checks).

Every C function also says who owns each pointer it takes or returns. The
rules at the top of `include/ponko.h` cover most of them by type: a `Ponko*`
object is borrowed for the call, a `const` pointer is read during the call
only, and any other pointer is the caller's memory that the call writes. A
returned pointer, a pointer to a pointer, callback user data and an object a
function takes over get an `Ownership:` paragraph in the function's doc
comment, such as:

```text
/// Ownership: the returned filter is the caller's (null on failure); free it
/// with ponko_filter_free.
```

`python3 scripts/check-ffi-ownership.py` fails for a function that leaves one
unsaid, and says what to write.

### Running the unsafe-code checks

```sh
python3 scripts/check-unsafe.py            # every unsafe item has a test
python3 scripts/check-unsafe.py --list     # each item and its test
python3 scripts/check-ffi-ownership.py     # every C pointer's owner is said
```

`check-unsafe.py` fails for unsafe code without a line, and prints the line to
add; write the test first. If you touched unsafe code outside the ECS (the
ECS has its own Miri commands, in AGENTS.md), run its
Miri tests, one command per test target (`--miri` prints them from the list):

```sh
python3 scripts/check-unsafe.py --miri | while read -r line; do
  MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-ignore-leaks" bash -c "$line" || break
done
rm -rf target/miri
```

If you touched `crates/ponko_ffi`, `include/` or `examples/cpp`, run the C
API under AddressSanitizer and UndefinedBehaviorSanitizer (checked on macOS
on Apple silicon; on Linux use the `x86_64-unknown-linux-gnu` target). The
C and C++ test programs, built with both sanitizers against a debug library:

```sh
cargo build -p ponko_ffi --features control-env,webrtc
cmake -S examples/cpp -B target/native-asan -DCMAKE_BUILD_TYPE=Debug \
  -DPONKO_SANITIZE=ON -DPONKO_WEBRTC=ON -DPONKO_LIBRARY_DIR="$PWD/target/debug"
cmake --build target/native-asan
ASAN_OPTIONS=detect_container_overflow=0 UBSAN_OPTIONS=print_stacktrace=1 \
  ctest --test-dir target/native-asan --output-on-failure
```

and the `ponko_ffi` tests, the Rust side built with AddressSanitizer (Rust
has no separate UB sanitizer; its UB checks are Miri's):

```sh
RUSTFLAGS="-Zsanitizer=address" RUSTDOCFLAGS="-Zsanitizer=address" \
  cargo +nightly test -p ponko_ffi --target aarch64-apple-darwin --tests
rm -rf target/native-asan target/aarch64-apple-darwin
```

## The C++ wrapper

`include/ponko.hpp` wraps every C function, or the function is on the short
C-only list, `scripts/cpp-c-only.txt`, with the reason. When you add a C
function, add its C++ wrapper with a `///` comment (what it does, units,
ownership, what it throws) and a test in `examples/cpp/tests.cpp`.
`python3 scripts/check-cpp-api.py` fails for a C function without a
wrapper, a public C++ declaration without a doc comment, and a C or C++ block
in this book that isn't an `\{{#include}}` of a compiled file. Put a guide's
C++ example in `examples/cpp/guide.cpp` (C in `guide.c`) between
`// ANCHOR: name` and `// ANCHOR_END: name` lines and include it by its anchor;
the native check compiles and runs both.

## Performance changes

Claims need numbers: measure in release builds before and after, and put
both in the pull request. Avoid per-frame heap allocations on hot paths.

The `ponko_bench` README, "Tracking results over time", describes the
tracked workloads: the instruction counts of a few small workloads (ECS
iteration, queries, spawn/despawn, a sim tick, 3D frame collection), which a
pull request that touches the engine, the ECS or the benchmarks compares with
its base. Nothing measures them for you: run them locally and put the
comparison in the pull request. A slower benchmark doesn't block the merge,
but explain it in the pull request, or fix it. To compare, run
`bench_track compare` on repeated runs of each side (a single run has no spread, so its threshold
is only the floor); instruction counts need valgrind, so on macOS compare
wall-clock runs of `ponko_bench`.

## Breaking changes

A change that breaks code written against the last release carries its
migration note in its own pull request: a changelog fragment with a
`### Breaking changes` entry and the `breaking` label.
[`MAINTAINING.md`](../../MAINTAINING.md#breaking-changes-the-migration-note)
has the rules, and how the API snapshots and the semver check find a break.

## Architecture proposals

Read [PROJECT_VISION.md](../../PROJECT_VISION.md), [ARCHITECTURE.md](../../ARCHITECTURE.md),
[ROADMAP.md](../../ROADMAP.md) and [STABILITY.md](../../STABILITY.md) for the affected work.
Discuss major changes in an issue before implementing them. Describe the problem,
a small proposed interface, ownership and dependency boundaries, alternatives,
compatibility/platform effects and validation plan. Explain why a new abstraction,
crate or dependency is better than a smaller internal change.

The aim is modular internals with an integrated public experience. Keep ordinary
`Game`/`Ctx` workflows usable, preserve optional features and headless operation,
and avoid unrelated rewrites. The proposed parallel system scheduler remains
optional; it needs sound access boundaries and explicit ordering/determinism rules,
never concurrent unrestricted mutable access to `Ctx` or `World`.

## Fuzzing

The loaders that read data a game doesn't control (scenes, prefabs, glTF,
the Unity importer's readers, action maps, asset packs, network packets)
have fuzz targets in [`fuzz/`](../../fuzz/README.md). When you change one of
those loaders, run its target for a few minutes (nightly Rust and
cargo-fuzz):

```sh
fuzz/seed-corpus.sh
fuzz/run-all.sh 300 scene   # five minutes of the scene target
```

A crash, hang or huge allocation it finds is a bug in the loader: fix it
there and add a regression test next to the loader's tests. Maintainers run
every target from time to time on `main` (`fuzz/run-all.sh` with no
arguments uses the seconds in `fuzz/targets.txt`) and open an issue,
"Fuzzing: `<target>` (`<kind>`)", for what it finds.

## Editing this book

The book is plain Markdown in `book/src/`, built with
[mdBook](https://rust-lang.github.io/mdBook/):

```sh
cargo install mdbook --locked
mdbook serve book --open
```

Every page has an edit button (top right) that opens the file in the project's repository.

### Every chapter has a maturity level

Every chapter opens, right under its title, with its maturity level and what
the level rests on: the reference game that uses the feature, the tests and
platforms in the list of checks, and the graphics backends they reach
(`STABILITY.md`, "Maturity levels", defines the levels and how to pick one).
The rustdoc of every top-level module of `ponko` repeats its chapter's
level. A new chapter starts like this:

```md
# My feature

> **Maturity: [tested](maturity.md).** Game: none. Tests: `tests/my_feature.rs`.
> Platforms: Linux, macOS and Windows, headless (the workspace tests in the
> list of checks). Graphics: not involved. Missing: a reference game that uses it.
```

Claim only what the repository records, and choose the lower level when in
doubt. A page that describes no feature (an index, a tutorial, a migration
guide) says so and why in a comment in the same place, which readers don't
see: `<!-- Maturity: not a feature. A migration guide for one release. -->`.
The [list of levels](maturity.md) is generated, and so are the module
paragraphs that name a chapter:

```sh
python3 scripts/maturity.py           # fails for a chapter or module without a level
python3 scripts/maturity.py --write   # regenerates maturity.md and the module docs
```

### Rust snippets compile

`scripts/check-book-snippets.py` compiles every ` ```rust ` block in the book
(one `cargo check` for all of them), so a rename can't silently break the guide.
Run it before pushing a chapter change:

```sh
python3 scripts/check-book-snippets.py
```

A block is wrapped in `fn snippet(ctx: &mut Ctx, world: &mut World) -> Result<..>`
with `use ponko::prelude::*;` in scope, so it can use `ctx`, `world` and `?` directly.
Anything else it needs goes on hidden lines, which mdBook doesn't show and the
checker keeps (`# ` prefix, as in rustdoc):

````md
```rust
# struct Health(f32);
# let enemy = ctx.world.spawn((Health(10.0),));
ctx.world.despawn(enemy);
```
````

- `no_run` and `should_panic` blocks are compiled too; nothing is ever run.
- A block that is only an `\{{#include}}` of an example file is skipped:
  that file is compiled by `cargo build --examples` and `cargo test`. Prefer an
  include for anything longer than a few lines.
- ` ```rust,ignore ` is a last resort (at most three in the whole book) and
  needs a reason in a comment, ideally hidden: `# // ignore: needs a GPU`.
- Rust code for another engine (the Bevy half of [Coming from Bevy](from-bevy.md))
  goes in ` ```rs ` blocks: mdBook highlights them as Rust and the checker
  leaves them alone. Include it from a file that compiles against that engine
  (`bench/compare/tests/from_bevy.rs`, checked with `cargo check --tests` in
  `bench/compare`) instead of writing it inline, and keep `ignore` for Ponko code.
- The check enables the `3d`, `physics`, `physics3d`, `navmesh` and `steam`
  features, so snippets for those compile without extra setup.

## Runs of the list of checks

Nothing runs `scripts/checks.yml` by itself: a maintainer runs it on their own
machine, and records what ran there, on what hardware and toolchain, and what
passed, failed, was skipped or needs a person. The records:

- [macOS](check-results/macos.md)

## Tests on a busy machine

`cargo test --workspace` runs many test binaries at once, often next to other
builds, so any thread can be paused for a while. A test that passes alone and
fails under load is a broken test. When a test is about time or frames:

- **Count the work, not the time.** Assert how many reads, jobs, uploads or
  ticks happened, or wait for a state (an asset becomes resident, a job
  finishes) instead of a number of frames. A source that blocks on a gate the
  test opens proves "this never waited on the game thread" without a clock.
- **A lower bound is safe.** `sleep` and timers only end late, so "at least
  185 ms passed" holds on any machine.
- **An upper bound is a hang detector.** A loop that ends as soon as the
  condition holds can have a long limit at no cost: use 30 to 60 seconds,
  never a few hundred milliseconds, and say in a comment that it is a hang
  detector. Under a load average of more than ten times the core count, a
  sleep overshot by at most about 20 ms, but a whole process can wait
  seconds for its turn.
- **A coarse complexity guard** ("parsing this file takes under 5 s, not
  minutes") keeps its bound only with a comment on why the margin covers a
  paused process.
- **Process-wide counters see other threads.** The global allocator counts
  every thread: a Rayon worker that starts or parks late allocates its sleep
  lock once, and the pool's job queue allocates a block every few dozen jobs.
  An allocation test counts per thread instead: `profile_scope!` around the
  code it checks, and inside the parallel work, counts the allocations on
  its own thread ([Allocations per thread](profiling.md#allocations-per-thread)).
  What is left of the pool's own is on the calling thread, a fixed number
  per job handed in, and the test says so.
- **Count the engine's work.** The engine keeps counters for this: the crowd
  of a navigation mesh counts its steps, the agents they moved and their
  work units (`NavMesh::crowd_stats`), so "every agent moved every tick and
  the work per agent did not grow with the crowd" needs no clock.
- **A spin is a lower bound too.** A loop that spins until some time has
  passed only runs longer on a paused thread. Use it to make work slow
  enough for a parallel path, never to give other threads time to wake:
  assert what holds whichever thread ran what.

Which assets finished before a group failed, which worker took a job and how
many frames a load took all depend on scheduling; assert them only where the
engine promises them.

## Panics and error messages

A panic or error message says what the caller did and how to fix it
("…; call `world.track_changes::<T>()` first"), not only what went wrong.
Panics that are part of the API get a `#[should_panic(expected = "...")]`
test, so the wording stays helpful.

`scripts/collect-panics.py` lists every `panic!`, `assert!`, `unwrap` and
`expect` in the engine and ECS production code with its reviewed
classification from `scripts/panic-audit.json`:

- `caller-contract`: a caller can reach it, and the message names the misuse
  and the fix;
- `invariant`: internal, no caller can reach it;
- `forwarded-error`: re-raises an `Error` whose message already says both;
- `non-panic-method`: a method that only shares a name.

`--check` fails while a new or changed site has no decision. Give the site a
message with a remedy, turn it into an `Error`, or record why it is an
internal invariant:

```sh
python3 scripts/collect-panics.py --check
python3 scripts/collect-panics.py --record src/gfx.rs:120 invariant "set just above"
python3 scripts/test_collect_panics.py
```

It is a lexical scan, not a proof: it finds the sites, a person decides.
Run `python3 scripts/collect-panics.py --check` with the other checks before you
declare a change ready, next to `cargo fmt`, `cargo clippy` and
`scripts/check-naming.py`.

## License

Ponko is dual licensed under
[MIT](../../LICENSE-MIT) or
[Apache-2.0](../../LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
