# Plugins for other crates

A [`Plugin`](systems.md#plugins) is one module of a game: `build` adds its
systems, `setup` does its startup work. The same trait is how a crate
extends games it has never seen: a weather system, an AI kit or a debug
overlay published on crates.io is a plugin that games add with one call.
This chapter is for the authors of such crates. A game that uses no
plugins never needs it: `Game::new`, systems and `run` stay as they are.

## What a plugin can add

Everything goes through the public builder and `Ctx`, the same calls a
game makes:

| What | Where | With |
|---|---|---|
| Systems, in phases | `build` | `game.add(Phase::..., f)`; `add_system` returns an id for `Game::order` and `Game::remove` |
| Components and resources | `setup` | `ctx.world.register::<T>()`, `ctx.world.insert_resource(r)` |
| Their reflection (tools, saves, the inspector) | `setup` | `ctx.world.reflect::<T>()`, `ctx.world.reflect_resource::<R>()` |
| Asset sources and decoders | `setup` | `ctx.assets.add_source(...)` ([asset sources](asset-sources.md)); a public loader function over `ctx.assets.load_with` that returns your type |
| UI widgets | `setup` and a system | build them with `ctx.ui` ([retained UI](ui.md)); a system reads their `ctx.ui.events()` |
| Debug toggles | `build` | `game.debug_toggle(name, on)`, checked with `ctx.debug.toggled(name)` |
| Control endpoint methods: tools for agents and scripts | `build` | `game.control_method(name, about, f)` |

`ponko new --plugin` makes a crate that uses most of them:

```rust
{{#include ../../tests/plugin-crate/src/lib.rs}}
```

The engine's own tests build that crate as a separate Cargo project
outside the workspace and run its tests, so it is known to compile against
the public API alone, with no hidden engine item (`#[doc(hidden)]`, such as
`ponko::__ffi`) in sight.

A decoder that fails stops startup with the asset's name in the message
when it runs from `try_setup`, as any fallible setup does. Reflected types
are known to tools by their short type name, and two types of the same
short name can't both be reflected: name yours so they won't meet another
crate's (`WeatherCloud`, not `Cloud`).

## Names, dependencies and duplicates

A plugin is known by its name: `Plugin::name`, the path of its type
(`acme_weather::WeatherPlugin`) unless it says otherwise. The rules:

- **Each name is added once.** Adding the same plugin twice is an error:
  its `build` doesn't run, and the game fails to start. `try_run` and
  `try_run_headless` return the error (`ErrorKind::Invalid`), `run` and
  `run_headless` panic with it, before any setup has run.
- **A plugin type added several times, configured differently**, gives
  each instance its own name (`fn name(&self) -> &str { self.level }`).
- **Dependencies are explicit.** `Plugin::dependencies` lists the names
  of the plugins that must be added before this one; the default name of
  a type `P` is `std::any::type_name::<P>()`. A missing one fails startup
  the same way, with a message that names what to add first.
- **A plugin may bring a dependency along**, when the game hasn't added
  it: `game.has_plugin(name)` says whether it did.

```rust
# struct Physics;
# impl Plugin for Physics { fn build(&self, game: Game) -> Game { game } }
struct Vehicles;

impl Plugin for Vehicles {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::BeforePhysics, |_: &mut Ctx| { /* steer */ })
    }
    fn dependencies(&self) -> Vec<&str> {
        vec![std::any::type_name::<Physics>()]
    }
}

// In order: starts.
Game::new("ok", 1, 1).plugin(Physics).plugin(Vehicles).try_run_headless(0, |_| {})?;
// Vehicles first: an error that says to add Physics first.
let error = Game::new("bad", 1, 1).plugin(Vehicles).try_run_headless(0, |_| {}).err().unwrap();
assert_eq!(error.kind(), ErrorKind::Invalid);
```

## Order between plugins

A dependency is added before its dependent, so:

- its `setup` runs first: the dependent's setup sees the resources it
  inserted;
- in every phase they share, its systems run first, because systems of a
  phase run in the order they were added.

Systems in different phases run in phase order, whoever added them. For
one exact pair, `Game::order(first, second)` with the ids `add_system`
returned states it, and the schedule checks it. Plugins with no
dependency between them are independent: don't rely on the order a game
happens to add them in.

## Debug toggles

`game.debug_toggle("weather.clouds", false)` adds a switch for the
plugin's own [debug drawing](debug-draw.md). A system asks
`ctx.debug.toggled("weather.clouds")`, which is true when debug drawing is
on and so is the toggle, before drawing (or computing what to draw). A key
binding turns it on with `ctx.debug.set_toggle(name, on)`; tools with the
control endpoint's `debug.draw` method (`{"toggles": {"weather.clouds":
true}}`) and ponko-mcp's `debug_draw` tool, which also list every toggle.
A game has at most 64 toggles; view systems in parallel batches see them
too. Name them after the plugin.

## Control methods

`game.control_method("weather.set_rain", about, f)` adds a method to the
[control endpoint](control.md): `f` gets the game's `Ctx` and the
request's params (JSON) between frames, and returns the answer or an
`Error`, whose message becomes the reply's error. The endpoint's `methods`
and `capabilities` list it with `about`, marked `"added": true`, and
`capabilities` lists the game's plugins by name. Agents call it with
ponko-mcp's `call_game_method` tool, which calls only methods a game
added. A name is `namespace.method` in lowercase letters, digits and `_`;
the engine's namespaces (`entity`, `debug`, `game`, ...) are taken, so use
the crate's. A method costs nothing while the endpoint is off.

## Making a plugin crate

```sh
ponko new acme-weather --plugin            # depends on the released engine
ponko new acme-weather --plugin --path ../ponko
cd acme-weather && cargo test
```

The crate is a library: `src/lib.rs` with the plugin, `tests/plugin.rs`
with headless games that use it, a README with a table of what it adds and
a version table, and an `AGENTS.md`. It depends on the engine with
`default-features = false`. The same folder is a cargo-generate template
(`crates/ponko_cli/templates/plugin`).

## Naming

- **The crate.** The `ponko` prefix belongs to the project: the engine's
  crates (`ponko_ecs`, `ponko_mcp`, ...) and the crates of the companion
  plugins repository use `ponko_<thing>`, and crates.io treats `ponko-x`
  and `ponko_x` as one name. Name yours after what it does, with `ponko`
  at the end when the name alone doesn't say it (`weather_ponko`), and
  add `ponko` to its `keywords` so it is found.
- **The plugin type** says what it adds: `WeatherPlugin`, or a plain noun
  (`Weather`) when the crate has one plugin and nothing else of that name.
  Export it from the crate root.
- **What it adds to a game** carries the crate's name, so two plugins
  can't collide: debug toggles and control methods `acme_weather.<what>`,
  reflected types that don't share a short name with another crate's.
- **Keep the default plugin name** (the type path) unless instances need
  names of their own; dependents then write
  `std::any::type_name::<acme_weather::WeatherPlugin>()`.

## Versioning

- **Follow semver.** Before 1.0, a breaking change to your API is a new
  minor version (0.1 to 0.2); after 1.0, a new major one.
- **Depend on a Ponko minor**, `ponko = { version = "0.1", default-features
  = false }`, never an exact patch (`=0.1.3`) or `*`. Ponko's patch
  releases don't break code (see [STABILITY.md](../../STABILITY.md)),
  so the range takes the game's newest.
- **A new Ponko minor is a breaking release of your crate.** Your API
  carries Ponko's types (`Game`, `Ctx`, components), and a game with two
  Ponko versions has two engines whose types don't mix. Release a new
  minor (or major) of your crate for it, and keep the old line for fixes
  while games move.
- **Turn on only the engine features your code needs**
  (`features = ["3d"]`); the game decides the rest, and a feature you turn
  on is on for every game that uses you.
- **Say which versions go together**: a table in the README (the template
  has one) and the Ponko range in `Cargo.toml`.
- **Use only covered API.** STABILITY.md lists what Ponko's versions
  cover and its unstable surfaces (`#[doc(hidden)]` items, `unstable_*`
  methods, the `bench-internals` feature); code that uses those can break
  in any release. The `Plugin` trait, the builder and `Ctx` services a
  plugin uses are covered like every other public item.

## The companion plugins repository

Plugins the project maintains outside the engine live in a companion
repository of their own,
[ponko-extras](https://github.com/ponkoengine/ponko-extras): a Cargo
workspace with one crate per plugin in `crates/`. Its crates are made with
`ponko new --plugin` (`--git` pointing at the engine, or the released
version), follow the naming and versioning rules above with the `ponko_`
prefix, and keep the template's tests: headless runs, and the duplicate
check. A new engine minor is a new minor of each of its crates. Its first
crate, `ponko_pulse`, is the template's own plugin, kept so every check
builds the template against the engine.

Until the engine is on crates.io, its crates build against an engine
checkout beside it (`../ponko`, on `main`). Its README lists the steps to
add a plugin; `scripts/check.sh` there checks each crate's layout (the
name, the workspace's engine dependency, a README with a versions table,
headless tests), then formatting, clippy and the tests.

## From C and C++

Native games add plugins with `ponko_game_plugin` (C) and `game::plugin`
(C++), as [Systems and the frame](systems.md#plugins) shows; those
plugins have no names or dependencies, are never duplicates of one
another, and debug toggles and control methods are Rust-only for now
(`Game::debug_toggle`, `Game::control_method`).
