# API reference

> **Maturity: not a feature.** Where the API reference is.

Every public item is documented in rustdoc, most with an example. Items
behind a cargo feature say which one ("Available on crate feature `3d`
only").

## Which version the guide matches

**This guide matches the engine's `main` branch: the unreleased 0.2.** The
latest release is 0.1.0 (its `Cargo.toml` says so on `main` too, until 0.2 is
released). The two tutorials and `ponko_cli` need 0.2, and other pages
describe APIs that are on `main` only (the note under each title says that
the guide follows `main`), so the 0.1.0 reference on docs.rs lacks some of
what the guide uses.

To read the reference that goes with the guide, build it from a checkout of
`main`, or of the commit you depend on (`git log -1` tells which one you
have):

```sh
git clone https://github.com/ponkoengine/ponko
cd ponko
cargo doc -p ponko --no-deps --features "3d,physics,physics3d,navmesh" --open
```

The features turn on the reference for what the guide's 3D, physics and
navigation chapters describe; without them those items are missing from the
reference, not undocumented. docs.rs builds everything public, which is the
list in `[package.metadata.docs.rs]` in `Cargo.toml` (it adds
`physics-determinism`, `steam`, `accessibility`, `control-env` and
`profiling`); pass that list to `--features` to get the same pages. The
other crates have no features to choose: `cargo doc --workspace --no-deps
--open` builds them all. `api/ponko/docs-rs.txt` is a plain-text list of
every public item of that build, one per line, which `grep` can search.

Once 0.2 is released, its docs.rs pages are this reference: replace `0.1.0`
in the links below with `0.2.0` (or `latest`).

## The crates

Links go to the 0.1.0 release on docs.rs:

- [`ponko`](https://docs.rs/ponko/0.1.0/ponko/): the engine. Start here;
  `use ponko::prelude::*;` brings in what a typical game needs; `ponko::d2`,
  `ponko::d3` and the service modules (`net`, `perf`, `render`, ...) hold the rest.
- [`ponko_ecs`](https://docs.rs/ponko_ecs/0.1.0/ponko_ecs/): the ECS on its own, including
  the C-compatible path (`world.raw().register_component`, `world.raw().for_each_chunk`) for other
  languages.
- [`ponko_macros`](https://docs.rs/ponko_macros/0.1.0/ponko_macros/): `#[derive(Component)]`,
  used through `ponko` or `ponko_ecs`.
- [`ponko_ffi`](https://docs.rs/ponko_ffi/0.1.0/ponko_ffi/): the C ABI and safety contracts.
  The [C and C++ guide](c-cpp.md) covers building, examples and the C++ wrapper.
- [`ponko_mcp`](https://docs.rs/ponko_mcp/0.1.0/ponko_mcp/): the MCP server for AI agents
  (the `ponko-mcp` binary); see [AI agents](agents.md).
- [`ponko_unity`](https://docs.rs/ponko_unity/0.1.0/ponko_unity/): reads Unity projects saved
  as text, for importing; see [Importing from Unity](unity.md).

The C++ reference is `include/ponko.hpp` itself: every public type and
function has a `///` comment saying what it does, its units, who owns what and
what it throws (Doxygen and editors read them). The C functions are documented
with their Rust definitions in `ponko_ffi`, on docs.rs.
