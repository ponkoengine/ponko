# Changing code while the game runs

> **Maturity: [experimental](maturity.md).** Game: none. Tests: the `hotpatch`
> feature's run of the headless, determinism and events tests through
> subsecond's jump table, with no patch applied. Platforms: Linux, macOS and
> Windows, headless (the workspace tests in the list of checks). Graphics: not
> involved. Missing: a patch applied by `dx` in the list of checks.

Edit a system, save, and the running game uses the new code from the next
frame on. Nothing restarts: every entity, resource, the camera and the
tick count stay where they were. With a battle half fought, you can retune
how units pick targets and watch the same fight continue.

This uses [subsecond](https://crates.io/crates/subsecond), the hot-patching
engine from the Dioxus project, run by Dioxus's CLI `dx`.

## Setup

Install the CLI once:

```sh
cargo install dioxus-cli@0.7.10
```

(`dx` is also the name of a Deno tool; if that one comes first on your
`PATH`, call this one as `~/.cargo/bin/dx`.)

Give your game a `hotpatch` feature that turns on the engine's:

```toml
[dependencies]
ponko = "0.2.0-rc.1"

[features]
hotpatch = ["ponko/hotpatch"]
```

Then start the game through `dx` instead of `cargo run`:

```sh
dx serve --hot-patch --features hotpatch
```

Edit any system in your game's crate and save. The console says
`Hot-patching: src/main.rs took 349ms`, then `ponko: code patched`,
and the system runs its new code.

## Try it

`examples/hotpatch.rs` spins a few crabs and prints the first one's
rotation every second:

```rust
{{#include ../../examples/hotpatch.rs:spin}}
```

Change `+= 1.5` to `-= 3.0` and save. The printed rotation turns around
from wherever it had got to:

```text
tick  4200  rotation   66.40
tick  4260  rotation   63.40
tick  4320  rotation   60.40
```

## What can change

| Change | Patched live? |
|---|---|
| A system's body, and any function it calls | yes |
| Constants and literals in them (speeds, colors, ranges) | yes |
| New helper functions called from a system | yes |
| Adding or removing systems (`.sim_system(...)` in `main`) | no: `main` already ran; restart |
| A component's or resource's fields (its size or layout) | no: restart (the old values in memory have the old layout) |
| Code outside your game's crate (the engine, other crates in a workspace) | no: only the crate with `main` is patched |

Code that changes a type's layout and runs anyway can crash, so restart after
changing struct fields. For tuning numbers without code at all, see
[data files](data-files.md); for looking at and editing the running world,
the [inspector](inspector.md).

## Cost

Without the `hotpatch` feature, systems are plain calls, as before. With
it, each system call looks itself up in a table, which is negligible (one
lookup per system per frame, not per entity), and only in debug builds;
release builds call directly even with the feature on.

The engine's own systems (`draw_sprites`, `camera_controls`, ...) can't be
patched, since they're outside your crate.

## C and C++

Not available: hot patching works on Rust code compiled by `dx`. C and C++
games can keep their game data in [data files](data-files.md), which
reload when saved.
