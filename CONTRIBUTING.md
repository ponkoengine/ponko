# Contributing to Ponko

Thanks for your interest! Bug reports, benchmarks on your hardware, docs
fixes, examples and code are all welcome.

## Start with what you know

**Rust-only pull requests are welcome.** You do not need to know C or C++,
write a guide chapter, or work on unrelated subsystems to contribute a Rust
fix or feature. Small fixes, tests, examples and documentation are useful
contributions on their own. No design proposal is needed for an isolated fix.

Keep your change focused, explain the behavior in the PR, and run the checks
relevant to it. Tests for changed behavior matter; a full integration checklist
is not an entry requirement. If a check needs tools or hardware you do not
have, say so and ask for help.

Maintainers own the broader integration work: guide updates, native bindings,
paired examples and the full platform matrix. They can help in the PR or record
separate follow-ups without assigning that work to you. Compatibility of
existing APIs and bindings still needs to be preserved.

[AGENTS.md](AGENTS.md) contains the detailed workflow and maintainer/agent
integration checklist; you only need the parts relevant to your contribution.

## Questions, bugs and ideas

- **Questions:** ask in the [issue tracker](https://github.com/ponkoengine/ponko/issues)
  and say it is a question, so the answer stays findable for the next person.
- **Bugs:** open an [issue](https://github.com/ponkoengine/ponko/issues/new)
  with the smallest code that shows the problem, your OS and GPU, and what
  you expected.
- **Ideas and features:** open an issue describing the idea (or, if you already have a concrete design, the design), so we can agree on
  the shape before you write the code. The engine aims for one obvious way to
  do each thing, so a new API gets discussed before it's built.
- **Made something?** Tell us in an [issue](https://github.com/ponkoengine/ponko/issues).
- **Security issues:** don't open a public issue; see [SECURITY.md](SECURITY.md).

## Finding a manageable contribution

You do not need to understand the whole engine to fix an isolated bug, improve an
example or clarify a guide page. Start with one reproducible behavior and its tests.
Browse the [open issues](https://github.com/ponkoengine/ponko/issues), check for an
existing assignee or active PR, and comment with the slice you want to tackle.

The ECS/rendering backlog (#132–#166) uses `help wanted`, `priority: P0` through
`priority: P3`, subsystem labels such as `area: ecs`, `area: rendering`,
`area: shaders`, `area: assets`, and `type:` labels. P0 establishes foundations;
P1 addresses high-impact limitations, P2 is useful lower-urgency work, and P3 is research. Priority is not difficulty:
this backlog has no difficulty labels. Read acceptance criteria and dependencies,
and ask for a bounded first step if the scope is unclear.

For example, #133 covers ECS
correctness tests and #135
covers visual regressions; a focused reproducer or test case can contribute toward
either without claiming the entire issue. Performance work starts with the baselines
in #132 and
#134.
[ROADMAP.md](ROADMAP.md) explains the sequence; verify live issue status before starting.

## Setting up

You need stable Rust (`rustup update stable`). On Linux, also install the
ALSA and udev headers for audio and gamepads
(`sudo apt-get install libasound2-dev libudev-dev`). For the C++
side, you need CMake and a C++17 compiler.

```sh
git clone https://github.com/ponkoengine/ponko && cd ponko
cargo test --workspace
cargo run --release --example battle
```

## Repository layout

Cargo packages use `ponko` / `ponko_*`; the old native names are gone, and legacy runtime environment variable names remain supported. Code
uses `include/ponko.h`, `include/ponko.hpp`, and `PONKO_*` settings. See
[the naming migration](book/src/naming.md) for precedence and compatibility.

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
| `fuzz/` | Fuzz targets for the loaders of untrusted files and packets ([fuzz/README.md](fuzz/README.md)) |
| `book/` | The guide ([mdBook](https://rust-lang.github.io/mdBook/)) |

## Making a change

1. Fork the repository and create a branch (`fix/<topic>`, `docs/<topic>`, ...).
2. Make one focused change. Add a regression test for a bug fix or a test of
   new behavior where practical; explain any testing gap in the PR.
3. Add or adjust nearby API comments when needed to explain the behavior.
   Broader guide chapters, C/C++ bindings and paired examples are welcome but
   **not required from every contributor**. Mention follow-up needs you notice;
   maintainers will help identify and own the rest.
4. If users will notice the change, add a changelog fragment,
   `changelog.d/<pr-or-topic>.md`, instead of editing `CHANGELOG.md`
   directly (see [changelog.d/README.md](changelog.d/README.md)).
5. Run relevant checks, then open a PR to `main` describing the change and
   what you checked. It is fine to open a draft early for feedback.

### Checks: start with your change

For a Rust change, format it and run the affected package or test, for example:

```sh
cargo fmt --all --check
cargo test -p ponko_ecs --test queries  # an ECS query change
cargo test -p ponko --test headless    # a headless simulation change
```

For guide changes, run `mdbook build book` if mdBook is available. You do not
need a C++ toolchain for an unrelated Rust change. Report checks you could not
run; maintainers cover the broader matrix before merging.

Maintainers/agents use the full integration commands in
[AGENTS.md](AGENTS.md#integration-checks), including workspace builds, tests,
Clippy, relevant native checks and Miri for affected unsafe ECS code. There is
no CI: nothing runs on a pull request by itself, so the checks a change needs
are run locally before it merges, and focused local checks do not waive
compatibility, correctness, privacy or leak checks. The exact
platform/feature matrix is written down in
[scripts/checks.yml](scripts/checks.yml), the list of checks: each job names
its platform and the commands to run. Maintainers run the parts a change
touches (on macOS, Windows or Linux as needed) before merging it.

### Adding or changing a cargo feature

The list of checks is [scripts/checks.yml](scripts/checks.yml): a change
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

### Unsafe code and sanitizers

Every `unsafe` block needs a test that runs it. Outside the ECS (which has
its own Miri commands, in AGENTS.md) the list is
[scripts/unsafe-inventory.txt](scripts/unsafe-inventory.txt): each item with
unsafe code (`function`, `Type::method`, `impl Send for Type`), the test that
runs it, and whether that test runs under Miri or why not (a GPU, a C
library, signals or sockets, another platform, or `window`: a check a person
runs in a real window). The benchmark binary `crates/ponko_bench` is
covered too. The C API crate's helpers need no line when a tested C function
reaches them: the script finds that test.

Checks that need a person (a real window) are listed in the guide's "Unsafe
code" section; run them when you touch the code they cover. For the
accessibility adapter on the engine's window:

```sh
cargo test --features accessibility --test ui_access_window -- --ignored
```

```sh
python3 scripts/check-unsafe.py            # every unsafe item has a test
python3 scripts/check-unsafe.py --list     # each item and its test
python3 scripts/check-ffi-ownership.py     # every C pointer's owner is said
```

`check-unsafe.py` fails for unsafe code without a line, and prints the line to
add; write the test first. If you touched unsafe code outside the ECS, run its
Miri tests, one command per test target (`--miri` prints them from the list):

```sh
python3 scripts/check-unsafe.py --miri | while read -r line; do
  MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-ignore-leaks" bash -c "$line" || break
done
rm -rf target/miri
```

Every C function says who owns each pointer it takes or returns. Most follow
the rules at the top of `include/ponko.h` ("Pointers and who owns them"),
decided by the parameter's type: a `Ponko*` object is borrowed for the call,
a `const` pointer is read during the call only, any other pointer is the
caller's memory the call writes. A returned pointer, a `T **out`, callback
user data and an object the function takes over need an `Ownership:`
paragraph in the function's doc comment; `check-ffi-ownership.py` fails for a
function without one and says what to write.

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

### Performance changes

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

### Breaking changes: the migration note

[STABILITY.md](STABILITY.md) promises that every break comes with what to use
instead. A change that breaks code written against the last release (a
renamed, moved or removed item, a changed signature, behaviour or default; the
semver check below finds some of them, you and the reviewer the rest) carries
its migration note in its own pull request:

- A changelog fragment with a `### Breaking changes` section
  ([changelog.d/README.md](changelog.d/README.md)). Each entry says **what
  changed**, **what to write instead**, and shows the code **before and
  after**:

  ````md
  ### Breaking changes
  - `Foo::bar` takes a `Baz` now.
    Instead: wrap the argument, `Baz::new(x)`.
    Before:
    ```rust
    foo.bar(x);
    ```
    After:
    ```rust
    foo.bar(Baz::new(x));
    ```
  ````

  `python3 scripts/changelog.py --check` refuses an entry that
  lacks any of the three.
- The **`breaking` label** on the pull request. The two go together, in both
  directions: the Semver check fails a pull request that breaks the API
  without the label and a fragment, and fails one that has the label but no
  such fragment (the tool does not see every break, and a label is a promise
  of a migration note). Run the check again with `--breaking` once the
  label is on.
- **Deprecate first where it is cheap**, as STABILITY.md's "Releases" says: a
  replaced item stays for one minor as `#[deprecated(note = "use X")]` (Rust)
  or with a note in the header (C), pointing at its replacement, and the next
  minor removes it. It is cheap when the old item can forward to the new one
  without keeping a second implementation alive: a rename, a moved item, a
  method that became a builder step. It is not cheap when the old and new
  behaviour can't coexist (a changed default, a changed data layout, a type
  that gained a required field): remove it at once, with the entry above.
  Deprecating is itself a change that goes under `### Deprecated`, not
  `Breaking changes`, and names the replacement too. The removal, a minor
  later, is the change that gets the `Breaking changes` entry.

At release time `scripts/changelog.py --version X.Y.Z` folds the entries into
the **Breaking changes** block of the CHANGELOG (the short form: what changed
and what to write instead, which is also the GitHub release's notes) and writes
the same entries, with their snippets, to `book/src/migrating-to-X.Y.md`, which
it lists in `book/src/SUMMARY.md`. Nobody writes the chapter by hand. See
[Releasing](#releasing).

### API snapshots

The `api/` folder holds a generated listing of the public API of every
published crate, one item per line, made with
[cargo-public-api](https://github.com/cargo-public-api/cargo-public-api). The
**API snapshots** check (`python3 scripts/api-snapshots.py check`) regenerates
it and fails when it differs from the
committed files, so a pull request that adds, renames, widens or removes a
public item shows it as a diff, and the reviewer decides whether the surface
is meant. 1.0 needs a deliberate surface; this is how it stays one. It is a
quick check (no tests): `update` took 1.9 minutes on a busy laptop with a warm
dependency build, and a rerun is under half a minute.

**After changing public API, run one command and commit the result:**

```sh
python3 scripts/api-snapshots.py update
git add api
```

- **What is listed.** `ponko`, `ponko_ecs`, `ponko_ffi`, `ponko_mcp` and
  `ponko_unity` (not proc-macro or binary-only crates), as
  `api/<crate>/<set>.txt`: `default` (default features), `no-default-features`
  (for `ponko`: headless), `docs-rs` (the features docs.rs builds: all the
  public ones, `3d` included) and, for `ponko`, `3d` (the defaults plus `3d`).
  A set that lists the same as `default` has no file. Blanket and auto-trait
  impls are left out as noise; derived impls (`Clone`, `Debug`) stay.
- **Reading a failure.** The output shows the diff and the command above. If
  you cannot run the tool, say so in the pull request and a maintainer
  regenerates the folder. A diff you did
  not intend means something became public by accident (`pub` instead of
  `pub(crate)`, a `pub use` that pulls in more than it should): fix the code,
  not the snapshot.
- **Merge conflicts in `api/`** (two pull requests added public API): do not
  merge the files by hand. Resolve the source, then run the command again.
- **Tools.** The script pins `cargo-public-api` and the nightly toolchain it
  needs together (`TOOL_VERSION` and `NIGHTLY`), because the tool reads rustdoc
  JSON, whose format changes with every nightly; `pins` prints them, so there
  is one place to change them. The script tells you the
  install commands if something is missing: `cargo install --locked
  cargo-public-api --version <pin>` and `rustup toolchain install <nightly>
  --profile minimal` (the nightly does not become your default). It also
  needs the audio and udev headers on Linux, like every build here. The
  listing is the one for x86_64 Linux: on macOS the script builds
  for `x86_64-apple-darwin` (same CPU types, no Linux libraries needed; it
  tells you the one `rustup target add` command) and drops the few items that
  exist only on macOS, so `update` gives the same text as on Linux. Another
  host (Windows) needs a maintainer on Linux or macOS to regenerate them. To move to
  a newer nightly, pick a tool release whose README supports it, change both
  pins, run `update`, and review the diff: it should only change where the
  tool's changelog says it does.
- **Relation to the Semver check** (below). The snapshot shows *every*
  change to the public API, growth included, and compares with the commit
  you are based on. Semver shows only what *breaks* code that compiled against
  the last crates.io release, and compares with that release. A new function
  fails the snapshot check (update the snapshot) and passes the semver check; a
  removed function fails both (update the snapshot, and add the `breaking`
  label and fragment, or undo the break). The snapshot cannot say whether a
  change breaks anything: that is what the semver check and the reviewer do with
  the diff.

### Semver checks

The **Semver checks** (`python3 scripts/semver-checks.py run`, the `semver`
job of [scripts/checks.yml](scripts/checks.yml)) compare the public API of every published
crate with its last release on crates.io, using
[cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks), and
reports what a change breaks. [STABILITY.md](STABILITY.md) promises that a
patch release (0.1.x) keeps code compiling that compiled against 0.1.0; this is
the check behind that promise. It is a quick check (a few minutes); run it
before merging a change to public API.

- **What is checked.** `ponko`, `ponko_ecs`, `ponko_ffi`, `ponko_mcp` and
  `ponko_unity`, each with the features docs.rs builds for it
  (`[package.metadata.docs.rs]` in its Cargo.toml; not `hotpatch` or
  `bench-internals`, which are not API). A new publishable crate is picked up
  by itself. `ponko_macros` is listed as not checked: the tool reads no API
  from a proc-macro crate.
- **What fails.** On a `release-*` branch (patch releases) and on a pull request
  into one, every break fails the check. On `main`, breaks pile up until the next
  minor release, so the comparison with crates.io only reports them. What
  fails a pull request is a break *it* adds (found by comparing with the commit
  it is based on), unless it carries the **`breaking` label** *and* a
  [changelog fragment](changelog.d/README.md) with a `### Breaking changes`
  entry (what changed, what to write instead, before and after code). Both
  are needed: the label tells the reviewer, the fragment becomes the migration
  chapter. The check also fails the other way round: the label without such a
  fragment, whether or not the tool found a break.

**Reading a failure.** Read the table at the end of the output (or the
section "before this change: <crate>"). Each failed lint has a name
(`inherent_method_missing`), a description, a link to the Cargo reference,
and a `Failed in:` list of items with the file they were in at the last release.

**Acting on it:**

1. *Undo the break* if it was not meant: keep the old item next to the new
   one and mark it `#[deprecated(note = "use ...")]`, or make the type
   `#[non_exhaustive]` before it grows. On a release branch this is the only way.
2. *Announce it* if it is meant (on `main` only): add the fragment, add the
   `breaking` label, and **run the check again with `--breaking`**.
   Maintainers add the label; a contributor can ask for it in the pull request.
3. *Tool errors* (the check could not build a crate, or the tool does not read
   this Rust version) are not breaks and no label excuses them. See the note at
   the top of the `semver` job in checks.yml about the pinned tool and Rust
   versions.

To run it, with the pinned tool (`cargo install --locked
cargo-semver-checks --version 0.51.0`; it needs the Linux audio and udev headers
the other builds need), after `git fetch origin`:

```sh
python3 scripts/semver-checks.py run                      # a change for main
python3 scripts/semver-checks.py run --breaking           # one with the `breaking` label
python3 scripts/semver-checks.py run --base release-0.1   # one for a release branch
```

It compares with `git merge-base origin/<base> HEAD`; the output says what a
failing change would need. For one crate,
`cargo semver-checks check-release -p ponko_unity --default-features --release-type minor`
shows the lints directly.

**What it does not catch.** The tool compares the public API as rustdoc
describes it, with a fixed list of lints (`cargo semver-checks check-release
--list`); a break no lint describes passes. Know these gaps when you review:

- Behaviour with an unchanged signature: a different result, panic, default
  value, ordering, timing, or determinism; what a saved world or a
  save file contains; the fields of the control endpoint and MCP messages.
- Many type changes in fields, parameters and return values, and tightened
  generic bounds, are covered only where a lint exists for them.
- Features outside the checked set (`hotpatch`, `bench-internals`) and
  platform-specific API on other targets: only the Linux target with the
  docs.rs features is built.
- The C and C++ headers (`include/`) and the layout of `#[repr(C)]` types as
  C sees them: the tool reads the Rust side of `ponko_ffi` only. The C ABI tests
  and `scripts/check-ffi-coverage.py` are what guard the header.
- Anything behind `#[doc(hidden)]` (such as `ponko::__ffi`), which is
  unstable by definition, and a new crate's first release (nothing to compare).
- Proc-macro crates (`ponko_macros`): the names and attributes of the derives
  are not compared.
- Dependencies: when a public type from `glam`, `wgpu` or another crate in our
  API changes major version, our API breaks without a lint saying so.

## Architecture proposals

Read [PROJECT_VISION.md](PROJECT_VISION.md), [ARCHITECTURE.md](ARCHITECTURE.md),
[ROADMAP.md](ROADMAP.md) and [STABILITY.md](STABILITY.md) for the affected work.
Discuss major changes in an issue before implementing them. Describe the problem,
a small proposed interface, ownership and dependency boundaries, alternatives,
compatibility/platform effects and validation plan. Explain why a new abstraction,
crate or dependency is better than a smaller internal change.

The aim is modular internals with an integrated public experience. Keep ordinary
`Game`/`Ctx` workflows usable, preserve optional features and headless operation,
and avoid unrelated rewrites. The proposed parallel system scheduler remains
optional; it needs sound access boundaries and explicit ordering/determinism rules,
never concurrent unrestricted mutable access to `Ctx` or `World`.

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

## Dependency audit

[`deny.toml`](deny.toml) holds the policy that [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)
checks: allowed licenses, security advisories (the RustSec database), banned
and duplicate crates, and allowed sources (crates.io only). Run it on any
pull request that changes `Cargo.toml`, `Cargo.lock` or `deny.toml`;
maintainers also run it on `main` from time to time (and before a release),
since new advisories appear without any change here:

```sh
cargo install --locked cargo-deny --version 0.20.2   # once; the pinned version
cargo deny --locked check -D warnings
```

When it fails, the output names the crate and the dependency chain that
brought it in:

- **A license is not allowed** (`licenses`). Prefer a dependency with an
  allowed license. The list in `deny.toml` is what the C library's
  third-party notices can carry; adding a license is a maintainer decision:
  say in the pull request why it is acceptable for the release archives.
  A crate that declares no license is refused by the notices too.
- **An advisory** (`advisories`: vulnerability, unmaintained, unsound or
  yanked). Update the dependency, with the smallest bump that fixes it
  (`cargo update -p <crate>`; `cargo tree -i <crate>` shows who needs it), and
  run the full checks. If nothing fixed exists, add an `ignore` entry to
  `deny.toml` with the advisory id and a `reason`: how we reach the crate and
  why that is acceptable, and what ends the exception. Never ignore without a
  reason.
- **A duplicate version** (`bans`). Align versions when you can (a newer
  release of the parent, or `cargo update`). A duplicate a dependency we don't
  control forces gets a `skip` entry with a `reason` naming the dependency
  that needs the old version. Don't skip a duplicate caused by our own
  requirements: update those.
- **A banned crate or a source that is not crates.io.** Both are deliberate:
  the release archives must not need a system TLS stack, and releases build
  from crates.io alone. Change the policy only with a maintainer's agreement.
- **A stale exception** (a warning, an error with `-D warnings`): the
  `skip` or `ignore` entry matches nothing any more. Delete it.

A new advisory found on `main` is handled the same way: fix, or ignore with a
reason, in a pull request.

## Fuzzing

The loaders that read data a game doesn't control (scenes, prefabs, glTF,
the Unity importer's readers, action maps, asset packs, network packets)
have fuzz targets in [`fuzz/`](fuzz/README.md). When you change one of
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

## Using coding agents

Coding agents are welcome under the same contribution rules. You remain responsible
for understanding and reviewing their changes, verifying APIs and dependencies,
running the required checks, and keeping the change scoped. Use existing MCP tools
for runtime inspection, screenshots, stepping or profiling when helpful; retain
reproducible commands/tests and explain what the evidence establishes. Do not treat
agent narration as proof. The agent acceptance suite
(`cargo test -p ponko_mcp --test agent_suite`; "The agent acceptance suite" in
`book/src/agents.md`) checks that agents can still finish real tasks with the MCP
tools: a tool change that breaks one of its scripted solutions fails it. Report incomplete work, test skips and platform limitations,
and distinguish implemented behavior from proposals. Keep private code and assets
out of public submissions and external services.

## Releasing

For maintainers. A release is a version tag, and a maintainer builds and
publishes it from that tag on their own machine, so the tag, the crates on
crates.io and the CHANGELOG section always come from one commit. There is no
release service: the steps below are the whole procedure. There are
three kinds of tag, all in the steps below: a release from `main`
(`v0.2.0`), a release candidate before a minor (`v0.2.0-rc.1`, see
[Release candidates](#release-candidates)), and a patch release from a
release branch (`v0.1.1` on `release-0.1`, see
[Patch releases and backports](#patch-releases-and-backports)).

1. Fold the fragments in: `python3 scripts/changelog.py --version X.Y.Z`
   (`--dry-run` shows the result first). If any fragment breaks the API, this
   also writes the migration chapter, `book/src/migrating-to-X.Y.md` (named
   after the minor: `migrating-to-0.2.md` for 0.2.0 and its candidates), from
   the same entries and lists it in `book/src/SUMMARY.md`; read it, then check
   the book (`mdbook build book`). A patch release may not carry breaking
   entries. In `CHANGELOG.md`,
   add a fresh empty `## [Unreleased]` heading above the folded content and
   rename the folded one to `## [X.Y.Z] - YYYY-MM-DD`; point the
   `[Unreleased]` link at `compare/vX.Y.Z...HEAD` and add `[X.Y.Z]` beside it.
   The section becomes the GitHub release's notes, which hold at most
   125,000 characters: summarize, and keep long lists in the guide.
2. Set `version = "X.Y.Z"` in the root `Cargo.toml` and in every published
   `crates/*/Cargo.toml` (`ponko_bench` is `publish = false`), including the
   `version = "..."` requirements the crates put on each other, and refresh
   `Cargo.lock` (`cargo check --workspace`).
3. `python3 scripts/release-check.py check vX.Y.Z` lists what still disagrees
   (it does not need the tag to exist), and `cargo publish --workspace
   --dry-run` packages and builds every crate the way crates.io will. Re-run
   the agent acceptance suite (`cargo run -p ponko_mcp --example agent_suite
   -- replay`; it is also part of `cargo test --workspace`) and, for a minor,
   try a real agent on it ("The agent acceptance suite" in the guide's agents
   chapter) and put the scores in the pull request.

   For a minor release (and its first candidate), record the wall-clock
   baseline on the named benchmark machine, idle, and add it to the pull
   request:

   ```sh
   v=X.Y.Z
   m=macos-aarch64-apple-m4-max   # what bench_track calls the machine (`<os>-<arch>-<cpu>`)
   mkdir -p crates/ponko_bench/releases/$v/$m
   for i in 1 2 3 4 5; do
     PONKO_BENCH_JSON=crates/ponko_bench/releases/$v/$m/run-$i.json \
       cargo run --release -p ponko_bench -- 'ecs/|archetypes-1024|query-build'
   done
   cargo run --release -p ponko_bench --bin bench_track -- \
     compare crates/ponko_bench/releases/<previous>/$m crates/ponko_bench/releases/$v/$m
   ```

   Link the folder and the comparison in the CHANGELOG section (the release
   notes). Run the checks of [scripts/checks.yml](scripts/checks.yml) on the
   release branch (all of them for a minor; on every OS the matrix names, as
   far as the maintainers have the machines; say in the pull request which
   ran where), then open the pull request and merge it.
4. Tag the merge commit on `main` (on `release-X.Y` for a patch release) and
   push the tag:

   ```sh
   git switch main && git pull     # release-X.Y for a patch release
   git tag -a vX.Y.Z -m "Ponko X.Y.Z" && git push origin vX.Y.Z
   ```

5. Build and publish from a clean checkout of the tag
   (`git switch --detach vX.Y.Z`), in this order:

   1. **Verify**: `python3 scripts/release-check.py check vX.Y.Z --on-branch`
      (the tag is `vX.Y.Z` or `vX.Y.Z-rc.N` on a commit merged to `main` or
      to `release-X.Y` of the tag's own minor, every crate is at the tag's
      version, `CHANGELOG.md` has a non-empty `## [X.Y.Z]` section that fits
      in a release body, a release whose section has **Breaking changes** is
      a minor one with its migration chapter written, listed in the book's
      summary and holding every entry, the sub-crates carry the root license
      files and no fragment is left over). Make sure the checks passed on
      this exact commit.
   2. **Build the C library** on each OS (Linux x86_64, macOS arm64, Windows
      x86_64): `cargo build --release -p ponko_ffi`, then a folder
      `ponko-vX.Y.Z-<os>-<arch>` with `include/ponko.h` and
      `include/ponko.hpp` in `include/`, `LICENSE-MIT`, `LICENSE-APACHE` and
      `NOTICE`, the libraries from `target/release` (`libponko_ffi.a`,
      `.so`, `.dylib`, or `ponko_ffi.lib`, `.dll`, `.dll.lib`) in `lib/`, and
      `THIRD-PARTY-NOTICES.txt` for every library linked into it
      (`python3 scripts/package-coin-dash.py notices --crate ponko_ffi
      --title "the Ponko C library" --out <folder>/THIRD-PARTY-NOTICES.txt`).
      Pack it as `.tar.gz` (`.zip` on Windows).
   3. **Package the crates**: `cargo publish --workspace --dry-run`.
   4. **Create the GitHub release** from the CHANGELOG section, with the
      archives:

      ```sh
      python3 scripts/release-check.py notes vX.Y.Z notes.md
      gh release create vX.Y.Z ponko-vX.Y.Z-* --title "Ponko X.Y.Z" --notes-file notes.md
      ```

      Add `--prerelease` for a candidate (`vX.Y.0-rc.N`).
   5. **Publish the crates**: `cargo publish --workspace`, with a crates.io
      API token of a crate owner (`cargo login`; scope it to publishing these
      crates, and revoke or let it expire afterwards). If it stops halfway,
      run it again: crates already published are skipped.
   6. **Redeploy the website** (https://ponko.dev/, which builds the guide
      from `main`; its own repository, `ponkoengine/ponko-website`): retry
      its latest deployment in Cloudflare Pages, or let its next deployment
      pick the guide up. The API reference of the release appears on docs.rs
      by itself.
   7. **Publish the web gallery** of the examples (see
      `web/gallery/README.md`), for a release that is the newest version
      (not for a candidate, nor for a patch of an older minor): build it for
      the tag and check every page in headless Chromium, then put the folder
      on the `gh-pages` branch, which GitHub Pages serves:

      ```sh
      node --test web/gallery/*.test.mjs web/smoke/gallery-lib.test.mjs
      node web/gallery/build.mjs --ref vX.Y.Z --out target/web/gallery
      node web/smoke/gallery.mjs --dir target/web/gallery --browser chromium --backend webgl2
      ```

      A failed page stops only the gallery, not the release.

### One-time setup (a maintainer, in settings)

1. **crates.io ownership**: every maintainer who publishes is an owner of
   each of the six crates `ponko`, `ponko_ecs`, `ponko_ffi`, `ponko_macros`,
   `ponko_mcp`, `ponko_unity` (and every crate added to the release later;
   `cargo owner --add <user> <crate>`). Never store a registry token in the
   repository or a file.
2. **GitHub Pages for the web gallery**: repository Settings, Pages,
   Source: "Deploy from a branch", branch `gh-pages`, folder `/` (the build
   writes `.nojekyll`).

### Release candidates

From 0.2.0 on, STABILITY.md has every minor go out as `vX.Y.0-rc.1` at least a
week before `vX.Y.0`. A candidate is released like any release (steps 1 to 5
above, on `main`) with these differences:

- **The version is `X.Y.0-rc.N`** in the root `Cargo.toml`, every published
  `crates/*/Cargo.toml` and the sibling requirements between them. The tag is
  `vX.Y.0-rc.N` with N counting from 1; `release-check.py` refuses any other
  suffix (`-beta.1`, `-rc`, `-rc.0`) and a candidate of a patch release.
- **Nobody gets it by accident.** crates.io and Cargo treat a version with a
  suffix as a pre-release: `cargo add ponko` and `ponko = "0.2"` never select
  it. A tester asks for it by writing it out: `cargo add ponko@0.2.0-rc.1`.
  The GitHub release is created with `--prerelease` (and a pre-release is
  never the "latest" one), and its notes start with a line saying so.
- **The CHANGELOG section may still be `[Unreleased]`.** The notes come from
  the first of `## [X.Y.0-rc.N]`, `## [X.Y.0]` and `## [Unreleased]` that
  exists, so a candidate needs no rename: fold the fragments in
  (`python3 scripts/changelog.py --version X.Y.0-rc.1`), keep the heading as it
  is and tag. The fragments still have to be folded in first, as for a
  release. The release itself, `vX.Y.0`, needs its own `## [X.Y.0]` section.
- **The migration chapter is needed already.** A candidate whose notes list
  `### Breaking changes` needs `book/src/migrating-to-X.Y.md`, listed in the
  summary and holding every breaking entry, because the people who try a
  candidate need it most. The fold writes it, and the next candidate's fold
  adds its new entries to the same chapter (all candidates and the release of
  a minor share the name).
- **A regression is fixed on `main`** through a pull request, and the next
  candidate, `rc.2`, follows. The release is the last candidate, unchanged but
  for the version: set `version = "X.Y.0"` and the section name, run
  `release-check.py check vX.Y.0`, and tag it.

### Patch releases and backports

`main` may hold breaking changes for the next minor as soon as a minor is
released, so patch releases (0.1.1, 0.1.2, ...) come from a branch, `release-X.Y`
(`release-0.1`), which holds only changes that keep the API (STABILITY.md,
"Releases"). Maintainers protect `release-*` like `main`: pull requests only,
no force pushes. The same checks apply to them: run the Semver check with
`--base release-X.Y` on a pull request into a release branch (it fails any
break there), and run the full list of checks before tagging a patch release.

**Create the branch** from the tag of the minor's first release, once, after
that release is out (a maintainer, once per minor; branch rules are set up for
`release-*` already):

```sh
git fetch origin --tags
git branch release-0.1 v0.1.0 && git push origin release-0.1
```

**Backport a fix.** A fix always lands on `main` first, through its normal
pull request, with its `changelog.d/` fragment (a `### Fixed` or `### Added`
entry; a `### Breaking changes` one can never be backported, and the Semver check
will refuse it). To carry it over:

1. Put the **`backport`** label on the merged pull request (or its issue)
   saying "this belongs on the patch branch". `gh pr list --state merged
   --label backport` lists what still waits.
2. Cherry-pick the merge commit (one commit, since pull requests are squash
   merged) onto a branch of the release branch. `-x` writes the original
   commit's hash into the message:

   ```sh
   git fetch origin
   git switch -c backport/<number>-<topic> origin/release-0.1
   git cherry-pick -x <merge commit on main>
   ```

   The fragment comes along. On a conflict, resolve it keeping the fix and
   nothing else, `git cherry-pick --continue`, and make sure the change still
   compiles against the older code; `cargo test` the affected crates.
3. Open the pull request **against `release-0.1`**, labelled `backport`, with
   "Backport of #<number>" in its description. Run the checks it needs and the
   Semver check (`--base release-0.1`) as for any pull request; merge it when
   they pass.
4. Take the `backport` label off the original pull request: it has been
   carried over.

**Release it** with steps 1 to 5 above, with these changes: work from a branch
of `release-0.1` and open the release pull request against it (fold the
fragments, set `version = "0.1.1"`, add the `## [0.1.1]` section, run
`release-check.py check v0.1.1`); tag the merge commit on `release-0.1`; and,
after the release, bring its notes to `main` through a pull request: copy the
`## [0.1.1]` section and its link line into `CHANGELOG.md` there, and delete
the fragments the release folded in from `main`'s `changelog.d/` (a
cherry-picked fragment has the same file name on both branches), so the next
minor does not list the fix a second time. Never merge `release-0.1` into
`main`.

`release-check.py check --on-branch` accepts `vX.Y.Z` on `main` as well as
on the `release-X.Y` of its own minor: `v0.1.1` on `release-0.1` passes, on
`release-0.2` or on an unmerged branch it does not.

`python3 scripts/release-check.py published` fails if a version of our crates
on crates.io has no matching tag, which is what a publish from the wrong
commit looks like; run it after every release. To fix it, tag the commit the
crate was published from. Its sha is in the `.crate` file:

```sh
curl -L https://static.crates.io/crates/ponko/ponko-X.Y.Z.crate | tar -xzOf - ponko-X.Y.Z/.cargo_vcs_info.json
git tag vX.Y.Z <that sha> && git push origin vX.Y.Z
```

`0.0.1` was published by hand from the last commit of #167 before it was
squash-merged; its tag `v0.0.1` is on the squash commit, whose tree differs
only in repository URLs and docs wording.

## License

By contributing, you agree that your contributions are dual licensed under
[MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE), like the rest of the
project, without any additional terms or conditions.
