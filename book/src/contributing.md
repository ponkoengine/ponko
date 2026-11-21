# Contributing

Start with [`CONTRIBUTING.md`](../../CONTRIBUTING.md).
The detailed maintainer/agent integration checklist is in
[`AGENTS.md`](../../AGENTS.md).
In short:

- Work on a branch, open a pull request to `main`, wait for CI.
- Add relevant tests for changed behavior and helpful nearby API comments.
  You do not need a doctest for every item or unrelated local toolchains.
- Performance claims need release-build numbers in the PR.
- Rust-only changes are welcome; guide chapters and C/C++ bindings are not
  required from every contributor. Maintainers coordinate that coverage and
  own follow-ups. Preserve existing compatibility and describe behavior and tests.

## Adding a cargo feature

Every feature needs a clippy line, a test line and, when docs.rs builds the
crate with it, a doc line in `.github/workflows/ci.yml` (the list of checks).
`python3 scripts/check-feature-coverage.py` fails for one without; see
`CONTRIBUTING.md`, "Adding or changing a cargo feature".

## Changing the renderer

The renderer's offscreen GPU tests, their reference images and what a
failure means on other hardware are described in
[GPU regression tests](render-tests.md). Run them on your GPU and under
`PONKO_GPU_TIER=baseline` and `webgl2` before a pull request, and say in it
which adapter they ran on and what was skipped.

## Testing the C API

Every function the C library exports (`extern "C" fn` in `crates/ponko_ffi/src`)
needs a test that calls it, in `crates/ponko_ffi/tests/` or a native suite in
`examples/cpp/`: a doctest that passes a null pointer proves argument checking,
not that the function works. CI runs `python3 scripts/check-ffi-coverage.py`,
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

The C API is one long unsafe boundary, so its tests also run under
AddressSanitizer and UndefinedBehaviorSanitizer: the C and C++ test programs
(`-DPONKO_SANITIZE=ON` in `examples/cpp`) and the `ponko_ffi` tests. The
commands are in `CONTRIBUTING.md`, "Unsafe code and sanitizers".

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

## The C++ wrapper

`include/ponko.hpp` wraps every C function, or the function is on the short
C-only list, `scripts/cpp-c-only.txt`, with the reason. When you add a C
function, add its C++ wrapper with a `///` comment (what it does, units,
ownership, what it throws) and a test in `examples/cpp/tests.cpp`. CI runs
`python3 scripts/check-cpp-api.py`, which fails for a C function without a
wrapper, a public C++ declaration without a doc comment, and a C or C++ block
in this book that isn't an `\{{#include}}` of a compiled file. Put a guide's
C++ example in `examples/cpp/guide.cpp` (C in `guide.c`) between
`// ANCHOR: name` and `// ANCHOR_END: name` lines and include it by its anchor;
the native CI build compiles and runs both.

## Editing this book

The book is plain Markdown in `book/src/`, built with
[mdBook](https://rust-lang.github.io/mdBook/):

```sh
cargo install mdbook --locked
mdbook serve book --open
```

Every page has an edit button (top right) that opens the file in the project's repository.

### Rust snippets compile

CI compiles every ` ```rust ` block in the book (`scripts/check-book-snippets.py`,
one `cargo check` for all of them), so a rename can't silently break the guide.
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
- The check enables the `3d`, `physics`, `physics3d`, `navmesh` and `steam`
  features, so snippets for those compile without extra setup.

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
  An allocation test fixes the pool size and starts the workers before it
  measures, then allows exactly that much, or counts only its own thread.

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
