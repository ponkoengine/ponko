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
