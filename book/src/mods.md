# Mods: what players can add and change

A mod is something a player adds to a game that the game's makers didn't
ship: a new enemy, harder levels, other art. Ponko supports **data-only
mods**: packs of asset files, mounted in a load order the player picks,
checked before anything of them is used. A mod adds files (a new enemy's
prefab and sprite sheet) and replaces the game's own by using the same path.
With the feature `mods-wasm`, a mod may also bring [code](#code-mods): a
WebAssembly module the game calls, run in a sandbox.

This chapter shows how a game takes mods, what is checked, how code mods
run and what they cost, and, at the end, [why data first](#why-data-only-mods-first):
the comparison with mods that run code (WebAssembly modules and an embedded
scripting language) on a real sample.

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
`"code": "frost/crawl.wasm"` names the mod's WebAssembly module, for a
[code mod](#code-mods). Everything but `format`, `version` and `id` may be
left out.

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
- **Every asset those files name**: sprite and animation textures, particle
  textures, 2D skeletons, meshes and their materials, collision meshes, the
  prefabs they include, and the fields the game registered with
  [`scene_asset_field`](asset-sources.md). Each must be a file of a mod in
  the set or one of the game's assets. `mount` lists **every** missing one in
  one `ModProblem::MissingAssets`, each with the file that names it, so a
  modder fixes them all at once instead of finding them one by one in play:

  ```text
  mod "art", file "enemies/bat.json": 2 asset(s) named that no mod and not the game has: "sprites/bat.png" (in "enemies/bat.json"), "sprites/rat.png" (in "enemies/rat.json")
  ```

  `ModSet::problems` doesn't check them: it has only the world, not the
  game's assets.
- **Its code**, for a code mod: the module compiles, imports only the host
  functions, and starts within its limits (see [Code mods](#code-mods)).

What a check can only find when the file is used fails then, with that file's
error, as for the game's own files: an image that doesn't decode. A name the
game made in code rather than read from a file (a texture it drew itself)
isn't an asset to a mod: name files.

`ModError` turns into an engine `Error` with `?` (its `path()` is the file):
`NotFound` for a missing mod, component, resource or asset, `Limit` for
sizes, `Json` for bad values and manifests, `Decode` for damaged packs,
`Invalid` for code that can't be used.

## Code mods

A data-only mod can't make a frost drip that *behaves* differently. With the
feature `mods-wasm`, a mod's manifest may name a WebAssembly module (built
from Rust, C, Zig, AssemblyScript...), and the game calls the functions it
exports, when and with what it decides:

```rust
fn crawl_frost_drips(ctx: &mut Ctx, drips: &mut [(Vec2, f32)]) {
    ponko::mods::with_code(ctx, |code| {
        let Some(crawl) = code.function("frost-drips", "crawl") else { return };
        for (at, heading) in drips.iter_mut() {
            let args = [at.x.into(), at.y.into(), (*heading).into()];
            if let Ok(Some(turned)) = code.call(crawl, &args) {
                *heading = turned.as_f32().unwrap_or(*heading);
            }
        }
    });
}
```

`with_code` hands the mounted code (`ModCode`) to the closure, its host
functions reaching this tick's `dt` and the game's 2D physics; it returns
`None` when no mod has code. `code.function(mod, name)` finds a function
once (keep the `ModFn`); `code.exporting(name)` lists every mod's function of
that name in load order, for hooks every mod may answer; `code.call(f, args)`
calls it with numbers (`i32`, `i64`, `f32`, `f64`) and returns its one result
or none, allocating nothing. `code.take_log()` gives what mods logged.

A module reaches only what the engine links, imported from module `"ponko"`:

| Import | Type | What it does |
|---|---|---|
| `log` | `(ptr: i32, len: i32)` | Text from the module's exported `memory`, for `take_log` (4096 bytes a line, 1024 lines between takes) |
| `dt` | `() -> f32` | The tick's step in seconds |
| `cast_ray` | `(ox, oy, dx, dy, max: f32) -> f32` | A 2D physics ray cast (feature `physics`): the distance to the first collider hit, or -1 |
| `get_field` | `(entity: i64, path: i32, len: i32) -> f64` | A number or bool field of a reflected component of an exposed entity: `"Health.current"`, `"Transform2D.position.1"` (array items by index); NaN when refused |
| `set_field` | `(entity: i64, path: i32, len: i32, value: f64) -> i32` | Writes that field: 0, or `-1` (entity not exposed, or gone), `-2` (no such component or field), `-3` (not a number field, or the value doesn't fit: a fraction for an integer, out of range, NaN) |
| `spawn_prefab` | `(path: i32, len: i32, x: f32, y: f32) -> i64` | Spawns an exposed prefab at `(x, y)` (as `ctx.spawn_prefab`) and exposes its root: the root's bits, or `-1` (not exposed), `-2` (it didn't spawn) |
| `random` | `() -> f32` | A number in `[0, 1)` from the mod's own generator, seeded by the game |

Text arguments (a field path, a prefab path) are UTF-8 in the module's
exported `memory`, at most 256 bytes; other text is refused (`-3`). Every
refusal also adds a line starting with `refused:` to `take_log`, naming the
call, the entity or prefab and why, so a game can show a misbehaving mod.

The frost drip's crawl rule, as the sample's code mod
(`tests/mod_sample.rs` checks it turns on exactly the ticks the game's Rust
rule does):

```text
(module
  (import "ponko" "cast_ray" (func $ray (param f32 f32 f32 f32 f32) (result f32)))
  (func (export "crawl") (param $x f32) (param $y f32) (param $h f32) (result f32)
    ;; turn at a wall ahead or no floor ahead, else keep the heading
    ...))
```

### Reaching the game

A mod reaches only what the game hands it. The game exposes entities, by
passing them to `code.expose(e)` and their bits to the mod as an `i64`, and
prefabs, by path with `code.expose_prefab(path)`; `code.hide(e)` and
`code.hide_all()` take entities back. The root of a prefab a mod spawns is
exposed for it, so it can set up what it made. Random numbers come from a
seed the game sets with `code.seed_random(seed)` (0 until then): the same
seed gives the same sequence, each mod has its own (its id is mixed in), so
one mod drawing more doesn't change another's numbers, and replays and
lockstep peers agree.

```rust
# use ponko::prelude::*;
# #[derive(Component)] struct FrostDrip;
fn frost_drips_think(ctx: &mut Ctx) {
    let drips: Vec<Entity> = ctx.world.query::<(Entity, &FrostDrip)>().iter().map(|(e, _)| e).collect();
    let tick = ctx.time.sim_tick();
    ponko::mods::with_code(ctx, |code| {
        code.seed_random(tick); // a sequence per tick: replays draw the same numbers
        code.expose_prefab("prefabs/icicle.json");
        let Some(think) = code.function("frost-drips", "think") else { return };
        for e in drips {
            code.expose(e);
            let _ = code.call(think, &[(e.to_bits() as i64).into()]);
        }
        for (mod_id, line) in code.take_log() {
            eprintln!("{mod_id}: {line}");
        }
    });
}
```

A mod's `think` may then read the drip's `"Transform2D.position.0"`, write
its `"FrostDrip.speed"`, spawn `"prefabs/icicle.json"` where it hangs and draw a
random number to decide when; it can't touch the player, or any entity or
prefab the game didn't name.

**What a mod can never do.** Whatever its code, a mod can't:

- reach an entity the game didn't expose (or one that died, or a guessed id:
  an entity's bits include its generation), nor a component the game didn't
  reflect, nor any field that isn't a number or a bool;
- add or remove components, despawn entities, or spawn anything but the
  prefabs the game exposed, at a 2D position;
- read files, the clock, the network, environment variables or other mods'
  memory, or import anything but the host functions above (WASI included);
  a module asking for more is refused at mount;
- run past its fuel or grow past its memory limit, host calls included: a
  host call costs the caller's fuel like any call;
- run when the game doesn't call it: mods run only inside `with_code`, on the
  game's thread, in the order the game calls them, so they can't race the
  game or each other.

**The sandbox.** A module that imports anything else (files, clocks, WASI)
is refused at mount. Each call (and each module's start function, at mount)
gets `CodeLimits::fuel_per_call` fuel, about one unit per instruction
(default 10 million, a few milliseconds): an endless loop stops with a
`ModProblem::Trap` and the game goes on. A module's memory grows to
`CodeLimits::memory_bytes` at most (default 16 MiB): one that asks for more
up front is refused at mount, and `memory.grow` past it fails. A call that
traps returns its error; the mod stays callable, and what to do with a mod
that fails (turn it off, show it in the mods menu) is the game's choice. Set
other limits with `ModSet::new().code_limits(CodeLimits { .. })`.

**Where it runs, and what it costs.** Code mods run on
[wasmi](https://github.com/wasmi-labs/wasmi), a WebAssembly interpreter in
pure Rust: the same backend on every platform the engine builds for, the
web (`wasm32-unknown-unknown`) and consoles or phones that forbid a JIT
included. There is no JIT backend: wasmtime was measured below at two to
three times faster per call, but it adds 8 MB and runs neither in a browser
nor where a platform forbids writable code, so it isn't in the engine. The
frost drip's crawl for 1000 drips per tick (`cargo run --release -p
ponko_bench -- mods/`), Apple M4 Max on an idle machine (2026-11-22,
revision `9235ea05`, load average 2.2), the median of three runs that
agreed within 8%:

| Backend | Where | 1000 drips per tick (median) | Per drip | Added size |
|---|---|---|---|---|
| The game's own Rust rule (data-only mods) | Everywhere | 0.095 ms | 95 ns | 0 |
| Code mod on wasmi (`mods-wasm`) | Everywhere, the web included | 0.190 ms | 190 ns (about 95 ns over Rust: the call, the interpreter, two host calls) | about 2.2 MB (measured below) |

Both include the two physics ray casts each drip makes. A game that calls a mod function a few thousand times per tick spends well under a millisecond; one that runs a mod's whole simulation per entity should measure first.

What one host call costs, from 1000 calls per tick of a mod function that
makes one (`cargo run --release -p ponko_bench -- mods/host-call`), against
the same function making none. Same machine and runs as above (the
medians agreed within 0.004 ms, 0.045 ms for `get_field`):

| Host call | 1000 calls per tick (median) | One host call, over the call itself |
|---|---|---|
| None (the call alone) | 0.058 ms | 58 ns for the call |
| `random` | 0.070 ms | about 12 ns |
| `get_field` (`"Transform2D.rotation"`) | 0.359 ms | about 300 ns |
| `set_field` (`"Transform2D.rotation"`) | 0.679 ms | about 620 ns |

The call alone measured 0.046 ms before; 0.058 ms here is slower by more
than the runs' spread (0.054 to 0.058 ms), the one number in this section
that moved the wrong way. Run back to back on the same idle machine, the
earlier revision gives 0.045 to 0.046 ms and this one 0.058 ms, and this
revision with `src/mods/code.rs` as it was before gives 0.045 to 0.046
ms: the cost came with the change that ties a mod's game pointer to its
`with_code` call (a per-thread call number). That runs once a tick, not
once a call, so why it costs about 12 ns a call is not yet known.

`get_field` and `set_field` go through reflection's JSON (the component is
read whole, and written back for a set), so they cost what `get_json` and
`set_json` do: fine for a few thousand per tick, not for every field of
every entity. A mod that needs many numbers per entity is better passed
them as arguments, returning its answer.

**Determinism.** wasmi runs with NaN results canonicalized, so the same
module and inputs give the same bits on every machine; the sample's crawl is
bit-identical to the Rust rule. The module's bytes are part of
`mods.fingerprint()`. Lockstep games call mods from their simulation in a
fixed order, as they would any system.

## Determinism

A mod is data, so a modded game is as deterministic as the game: the same
mods in the same order give the same files, on every machine. Lockstep
peers and replays should agree on `mods.fingerprint()`, a hash of the mods'
ids, versions and order, their files' names and sizes and the bytes of their
JSON files and code (art and sound count by name and size only).

## In C and C++

`ponko_ctx_mount_mods(ctx, paths, count)` mounts mod packs by path in that
order; `ponko_ctx_mount_mods_from_memory(ctx, mods, count)` mounts packs
held in memory (downloaded, built into the game), each a `PonkoModBytes`
with a name for errors and the pack's bytes, copied during the call. On a refused set it returns `PONKO_NOT_FOUND` (a missing mod,
component or resource) or `PONKO_INVALID_ARGUMENT`, with the mod, file and
reason in `ponko_last_error()` and the file in `ponko_last_error_path()`.
`ponko_ctx_mod_count`, `ponko_ctx_mod_id`, `ponko_ctx_mod_provider`,
`ponko_ctx_mod_file_under` and `ponko_ctx_mods_fingerprint` read the
`Mods` resource. In C++: `ctx.mount_mods({"a.pack", "b.pack"})`,
`ctx.mount_mods_from_memory({{"a.pack", bytes}})`, `ctx.mod_ids()`,
`ctx.mod_provider(name)`, `ctx.mod_files_under(prefix)` and
`ctx.mods_fingerprint()`. Loose files (`ModPack::from_files`; `ponko pack`
makes a pack of them) and `ModSet::problems` are Rust-only for now
([Still Rust-only](c-cpp.md#still-rust-only)).

**Code mods from C and C++.** Build the library with the feature
(`cargo build --release -p ponko_ffi --features mods-wasm`); mods with code
then mount as from Rust, with the default limits (10 million fuel a call,
16 MiB of memory). `ponko_ctx_mod_function(ctx, mod_id, name, &fn)` looks a
function up once (`PONKO_NOT_FOUND` when no mod has code or no such
function) and `ponko_ctx_mod_call(ctx, fn, args, count, &result,
&has_result)` calls it with `PonkoModValue` numbers (`kind`
`PONKO_MOD_I32`, `_I64`, `_F32`, `_F64`; integers in `i`, floats in `f`). A
call that doesn't fit, runs out of fuel or memory or traps returns
`PONKO_INVALID_ARGUMENT` with the mod and the reason in `ponko_last_error()`;
the mod stays callable. `ponko_ctx_mod_expose_entity`,
`ponko_ctx_mod_hide_entity`, `ponko_ctx_mod_expose_prefab` and
`ponko_ctx_mod_seed_random` choose what the host functions reach. In C++:

```cpp
{{#include ../../examples/cpp/guide.cpp:code_mods}}
```

Built without the feature (the default), each of these calls returns
`PONKO_UNSUPPORTED` ("code mods are not compiled in") and a mod with code is
refused at mount like any bad mod (`PONKO_INVALID_ARGUMENT`); data-only mods
work the same. Custom limits (`ModSet::code_limits`) and the mods' log
(`ModCode::take_log`) are Rust-only for now.

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

**The decision**: data-only mods first, code mods as an optional feature on
wasmi. Both ship now: data-only mods always, [code mods](#code-mods) with
the feature `mods-wasm`, off by default, with a small host API (log, `dt`, 2D
ray casts) that grows as games need it.
