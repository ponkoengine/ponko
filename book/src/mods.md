# Mods: what players can add and change

A mod is something a player adds to a game that the game's makers didn't
ship: a new enemy, harder levels, other art. Ponko supports **data-only
mods**: packs of asset files, mounted in a load order the player picks,
checked before anything of them is used. A mod adds files (a new enemy's
prefab and sprite sheet) and replaces the game's own by using the same path.
It runs no code.

This chapter shows how a game takes mods, what is checked, and, at the end,
[why data first](#why-data-only-mods-first): the comparison with mods that run
code (WebAssembly modules and an embedded scripting language) on a real
sample.

## A mod

A mod is a folder with a `mod.json` at its root, packed into one file with
[`ponko pack`](packaging.md#assets-in-one-pack-ponko-pack):

```text
frost-drips/
  mod.json
  ember_hop/prefabs/frost_drip.json
  ember_hop/sprites/frost_drip.png
```

```json
{
  "format": "ponko-mod",
  "version": 1,
  "id": "frost-drips",
  "name": "Frost drips",
  "mod_version": "1.0.0",
  "requires": ["deep-caves"],
  "after": ["hd-sprites"]
}
```

`ponko pack frost-drips --out frost-drips.pack` makes the pack. The `id` is
lowercase letters, digits, `-` and `_`. `requires` names mods that must be on
too and load first; `after` names mods that load first when they are on.
Everything but `format`, `version` and `id` may be left out.

The files are what a game already reads by name: prefabs and scenes of
[reflected components](reflection.md), data files, images, sounds. The frost
drip of the sample below is the game's drip with its own art and twice the
speed:

```json
{
  "format": "ponko-prefab",
  "version": 1,
  "components": {
    "Transform2D": {},
    "Sprite": { "texture": "ember_hop/sprites/frost_drip.png", "z": 0.3,
                "region": { "min": [0.0, 0.0], "max": [16.0, 16.0] } },
    "RigidBody": { "kind": "Kinematic" },
    "Collider": { "shape": { "Rect": { "size": [12.0, 9.0] } }, "sensor": true },
    "Drip": { "speed": 56.0, "heading": 1.0 }
  }
}
```

## Mounting mods

`ModPack::open(path)` opens a pack and reads its manifest;
`ModPack::from_pack_bytes` takes one held in memory (downloaded, or fetched
in a browser) and `ModPack::from_files` loose files (a mods folder the game
reads itself, or a test). A `ModSet` holds them in the order the player put
them, and `mount` checks them all, then mounts them:

```rust
{{#include ../../tests/mod_sample.rs:mount}}
```

Mount in setup, **after** reflecting the game's components (mods are checked
against them) and **before** loading anything a mod may replace: an asset
loaded before is cached and stays as it was. Mods mount once per game,
and their files aren't hot reloaded (like every added
[asset source](asset-sources.md)).

From then on every load by name (`ctx.spawn_prefab`, `ctx.gfx.load_texture`,
`ctx.assets.load_resource`, scenes, sounds) looks in the mods first. The
`Mods` resource says what is on:

- `mods.list()`: the mods in load order, with their names, versions and
  where they came from: a mods menu.
- `mods.provider(name)`: the mod a file loads from, or `None` for the game's
  own.
- `mods.files_under("enemies/")`: the mods' files under a folder, sorted:
  how a game finds what mods added, such as every enemy prefab, to spawn
  them where its levels say.
- `mods.fingerprint()`: see [Determinism](#determinism).

## Load order and overrides

Mods load in the order they were added, each moved only as far as its
`requires` and `after` need. Where two mods have a file of the same name, the
later one's is used; the game's own files (`assets/`, its `assets.pack`) come
before every mod. So a mod that ships `ember_hop/prefabs/drip.json` changes
every drip the levels spawn, and a mod after it that ships the same file wins
over it. `ModSet::order()` gives the order without mounting, for a menu.

## What is checked

`mount` checks every mod before it mounts any, so a bad mod is refused whole:
nothing of it, or of the mods with it, is used, and the game runs as without
mods. `ModSet::problems(&ctx.world)` runs the same checks and lists one
problem per bad mod, for a menu that greys them out. The `ModError` names the
mod, the file and the reason:

```text
mod "frost-drips", file "ember_hop/prefabs/frost_drip.json": component "Frozen" isn't one this game has (not reflected)
mod "frost-drips", file "ember_hop/prefabs/frost_drip.json": "Drip" doesn't fit: invalid type: string "fast", expected f32
mod "frost-drips", file "mod.json": requires mod "deep-caves", which isn't mounted
mod "a", file "mod.json": mods must load after each other in a cycle: a -> c -> b -> a
```

What is checked:

- **The pack**: it is one (damaged, cut-short and foreign files are refused),
  the JSON files' CRC-32 checked as they are read, no more files and bytes than
  `ModLimits` allow (10 000 files, 64 MiB a file and 1 GiB a mod by default;
  a file is refused on the size its index claims, before it is unpacked).
- **Names**: every file name is a plain relative path: no `..` or `.` parts,
  no leading `/`, no `\`, `:` or control characters, so no name points
  outside the game's assets wherever it ends up.
- **The manifest**: present, `format` and `version` known, no unknown
  fields, ids well formed and unique, required mods there, no cycle.
- **Every `.json` file**: it parses (nesting deeper than 128 is refused, not a
  stack overflow), and the prefabs and scenes in it name only components and
  resources the game reflects, with fields that fit their types. A change to
  an included prefab is partial, so only its component names are checked.

What a check can only find when the file is used fails then, with that file's
error, as for the game's own files: an image that doesn't decode, a prefab
that includes one that isn't there.

`ModError` turns into an engine `Error` with `?` (its `path()` is the file):
`NotFound` for a missing mod, component or resource, `Limit` for sizes,
`Json` for bad values and manifests, `Decode` for damaged packs.

## Determinism

A mod is data, so a modded game is as deterministic as the game: the same
mods in the same order give the same files, on every machine. Lockstep
peers and replays should agree on `mods.fingerprint()`, a hash of the mods'
ids, versions and order, their files' names and sizes and the bytes of their
JSON files (art and sound count by name and size only).

## In C and C++

`ponko_ctx_mount_mods(ctx, paths, count)` mounts mod packs by path in that
order. On a refused set it returns `PONKO_NOT_FOUND` (a missing mod,
component or resource) or `PONKO_INVALID_ARGUMENT`, with the mod, file and
reason in `ponko_last_error()` and the file in `ponko_last_error_path()`.
`ponko_ctx_mod_count`, `ponko_ctx_mod_id`, `ponko_ctx_mod_provider`,
`ponko_ctx_mod_file_under` and `ponko_ctx_mods_fingerprint` read the
`Mods` resource. In C++: `ctx.mount_mods({"a.pack", "b.pack"})`,
`ctx.mod_ids()`, `ctx.mod_provider(name)`, `ctx.mod_files_under(prefix)` and
`ctx.mods_fingerprint()`. Mods from memory (`ModPack::from_files`,
`ModPack::from_pack_bytes`) and `ModSet::problems` are Rust-only for now
([Still Rust-only](c-cpp.md#still-rust-only)).

## Why data-only mods first

Mods that run code can do more: a new enemy that *behaves* differently, not
only one with other numbers and art. Three ways were compared on one sample,
a mod for the [Ember Hop](ember-hop.md) platformer that adds an enemy type,
the "frost drip":

1. **Data only**: a pack with the frost drip's prefab and sprite sheet. The
   game's own `Drip` component and crawl rule drive it
   (`tests/mod_sample.rs`).
2. **WebAssembly modules** calling engine functions through the host, the
   way the C API is called, with [wasmtime](https://wasmtime.dev) (a
   compiler) and [wasmi](https://github.com/wasmi-labs/wasmi) (an
   interpreter).
3. **An embedded scripting language**: [Rhai](https://rhai.rs) and Lua 5.4
   through [mlua](https://github.com/mlua-rs/mlua).

For 2 and 3 the frost drip's crawl rule (turn at walls and ledges, move by
speed × dt) was written as the mod: one call from the game per drip per
tick, and two calls back into the engine (the ray casts ahead and down).
Each runtime ran it for 1000 drips for 1.5 s after 600 ticks of warm-up; then a hostile mod tried to spin forever
and to allocate forever. The measurements were made in November 2026 on an
Apple Silicon laptop that was busy compiling other work at the same time
(load average 70 to 150), so the times are rough: two runs each, both shown.
Sizes are exact: the release build of Ember Hop with and without each
runtime linked in and used.

| | Data only | WASM: wasmtime 51 | WASM: wasmi 2.0 | Rhai 1.27 | Lua 5.4 (mlua 0.12) |
|---|---|---|---|---|---|
| **What a mod can do** | Recombine reflected components; new art, sound, levels, tuning | Any logic, through the functions the host exposes | Same as wasmtime | Any logic, through registered functions | Same as Rhai |
| **Hostile mod: endless loop** | Can't: no code | Stopped by fuel (10 M instructions: 3 to 88 ms) | Stopped by fuel (5 to 53 ms) | Stopped by `max_operations` (4 to 34 ms) | Stopped by an instruction hook (60 to 260 ms) |
| **Hostile mod: endless memory** | Can't; file sizes are limited | Refused at the store limit (16 MiB) | Refused at the store limit | `max_array_size` stops it, but each push re-measures the array: **9.5 to 11.4 s** to reach 65 536 elements, a stall a mod can cause on purpose | Refused at `set_memory_limit` (97 to 232 ms) |
| **Sandbox by default** | Yes | Yes: a module reaches only what the host links | Yes | Yes: no file or network access unless registered | **No**: `Lua::new()` loads `io` and `os`; a sandbox must list its libraries (`new_with`) |
| **Determinism** | Same files → same game | Bit-identical to the Rust rule (f32); NaN bits canonicalized with one setting | Bit-identical to the Rust rule | f64 only (or f32 with a feature): 488 of 1000 drips end elsewhere than the f32 rule; math functions come from the platform | Numbers are f64: same 488 of 1000; table iteration order (`pairs`) depends on a per-run hash seed |
| **Cost per drip update** (the same rule in Rust: 2 to 9 ns) | 0 (the game's own rule) | 67 to 166 ns | 200 to 434 ns | 1 500 to 3 400 ns | 425 to 606 ns |
| **Added size** (Ember Hop, release, stripped; 21.46 MB without) | 0 | +7.98 MB (+37 %) | +2.17 MB (+10 %) | +2.93 MB (+14 %) | +0.38 MB (+2 %) |
| **Web** (`wasm32-unknown-unknown`) | Yes | **No**: it doesn't build (no `mmap`), and a browser can't run a JIT inside WASM | Yes | Yes, without its default `runtime-rng` (`getrandom` needs a flag otherwise) | **No**: the C library doesn't build for it |
| **C and C++ games** | Same packs, `ponko_ctx_mount_mods` | Host functions are C functions: natural | Same | Rust-side registration only | Has its own C API, but a second one beside Ponko's |

What the numbers say:

- **Data-only mods cost nothing**, run everywhere the game runs (the web
  included), can't hang or crash the game, and keep replays and lockstep
  working. What they can't do is new behaviour: a frost drip that freezes
  the hero needs code. That limit is real, but most mods of most games are
  new content and new numbers, and a game widens it by reflecting more
  components with more settings.
- **For code mods, WebAssembly through wasmi is the strongest candidate**:
  sandboxed by construction (fuel and memory limits held in every test),
  bit-identical results, builds for the web, and its host functions have the
  same shape as the C API, for 2.2 MB. wasmtime is about two to three times
  faster per call but adds 8 MB and doesn't run in a browser. Rhai stalls
  for seconds on a hostile allocation and is the slowest per call. Lua is
  the smallest (0.4 MB) and quick, but isn't sandboxed by default, isn't
  deterministic across runs and doesn't build for the web.

**The decision**: data-only mods ship now (this chapter). Code mods are a
later, optional step: a `mods-wasm` feature on wasmi, off by default, with
host functions generated from the C API so a mod calls the same engine
functions a native game does. Nothing of it is in the build today; the
prototypes were measured and removed.
