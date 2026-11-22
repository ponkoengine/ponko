# Legions: 100,000 units that replay bit for bit

[`examples/legions`](../../examples/legions) is the engine's simulation
showcase. Two armies of 50,000 soldiers and archers march across a map of
rocks, lakes and mud on shared flow fields, find their enemies with spatial
grids, fight, shoot arrows that fly and land, and fall. You command red; blue
attacks. Every battle can be recorded and played again, and the replay ends
with the same world hash on any thread count and any machine.

```sh
cargo run --release --example legions                      # 100,000 units
cargo run --release --example legions -- --units 250000    # bigger
cargo run --release --example legions -- --record my.json  # play and record
cargo run --release --example legions -- --replay my.json  # check it headless
cargo test --test legions                                  # 13 headless tests
```

| Action | Input |
|---|---|
| Send red to a point | left click |
| Red attacks / holds | A / H |
| New round | R |
| Pan / zoom | drag with the right button / wheel |
| Pause / speed 1x, 2x, 4x | Space / 1, 2, 3 |
| Quit (writes the recording) | Esc |

It packages like the reference games (`ponko package --example legions`,
see [A complete game, packaged](packaging.md)): it draws its sprites in
code, so it ships no assets, and the package carries a recorded battle,
`skirmish.json`, that `legions --replay skirmish.json` checks. It builds for
the web (`web/build.sh legions`) with 10,000 units, since a browser tab runs
it on one core.

## How the code is laid out

`main.rs` reads the command line. Everything else is in `game.rs`, as three
plugins, so the tests and the offscreen GPU test build the same game:

1. `Legions`: the rules, all simulation systems on the fixed tick. They read
   only the world and the orders given at a tick.
2. `Controls`: a frame system turns the mouse and keys into `Order`s, and
   does nothing else.
3. `View`: the sprite animation, the ground and the HUD. None of it changes
   the game, so a windowed run and a headless one are the same battle.

The rules are five systems, in this order every tick:

| System | Phase | What it does |
|---|---|---|
| `take_orders` | `Input` | applies the orders given since the last tick (and a replay's orders for this tick), and records them |
| `aim` | `AfterPhysics`, every 30th tick | an attacking army's goal becomes the enemy unit nearest to the enemy's middle; the flow field is recomputed when it moved |
| `fight` | `AfterPhysics` | on all cores: every unit finds its target and moves; every arrow flies and, landing, finds whom it hit |
| `resolve` | `AfterPhysics` | in order: applies the hits, looses the arrows, removes the dead and the landed arrows, ends and restarts rounds |
| `keep_neighbors_close` | `AfterPhysics`, every 60th tick | sorts everything in memory by where it stands |

## What the engine does for it

**Flow fields.** Each army has one [`FlowField`](pathfinding.md) over the
map's `NavGrid`, shared by all its units: a unit's step is one lookup,
`fields[team].direction_at(position)`, from inside `par_for_each`. Mud is a
cell cost, so the fields go around it when that is shorter, and units slow
down in it. A field is recomputed only when its goal moves more than three
cells.

**Spatial queries.** Each army is indexed twice per tick in a
[`SpatialGrid`](spatial.md), and the four grids are rebuilt at the
same time with `rayon::scope`: a fine grid (cell 7) for friends that crowd a
unit and for enemies in a soldier's reach, and a coarse one (cell 24) for
enemies seen from afar. The coarse grid carries each unit's velocity as
data (`SpatialGrid::with_data`), so an archer leads its target: it aims where
the target will be when the arrow lands.

**Turns.** Most units are marching, not fighting, so they look for enemies
only on their turn (`ctx.time.turns(4)` for soldiers, `turns(8)` for archers);
a soldier in a fight looks every tick and an archer when it is ready to
shoot. Crowding is refreshed every other tick.

**Instanced sprites.** Every unit, arrow and fallen unit is a frame of one
`SpriteSheet` (8 columns of 8 x 8 pixels: four walking frames, standing, two
of attacking; a row per army and kind, and one for the arrow and the
fallen). The engine's sprite system draws sprites that share a texture as
one instanced draw call, so a whole battle is two draws: the ground and the
sheet. The `animate` frame system picks each unit's frame on all cores, from
the unit's state alone (striking just after an attack, walking while it
moves, each unit at its own step).

**Projectiles and deaths.** An arrow is an entity: it flies straight from
the archer to the aimed point in a whole number of ticks and hits the enemy
nearest to where it lands, if one is within three units, so a target that
dodged is missed. A unit whose health runs out becomes a `Corpse` that fades
over four seconds.

**Reflection.** `Unit`, `Arrow`, `Corpse` and the `Score` resource are
reflected, so the [inspector](inspector.md) and the
[`ponko-mcp`](agents.md) tools can read and change a running battle.

## Orders are the input, and a replay is a list of orders

The player's input never touches the game directly. `Controls` turns a
click into `Order::Rally { x, y }` (in world units, so the window size and
the camera don't matter) and a key into `Order::Attack`, `Order::Hold` or
`Order::Restart`, and pushes it onto the `Orders` resource. The first
simulation system of the next tick applies it and stamps it with that tick:

```rust
{{#include ../../examples/legions/game.rs:take_orders}}
```

A recording is those stamped orders plus the seed, the army size, the number
of ticks and the world hash at the end. `--record` writes it when you quit
with Escape and every ten seconds while you play:

```json
{
  "format": "ponko-legions-replay",
  "version": 1,
  "seed": 1,
  "units": 2000,
  "ticks": 600,
  "orders": [
    { "tick": 61, "order": "Hold" },
    { "tick": 152, "order": { "Rally": { "x": 700.0, "y": 375.0 } } },
    { "tick": 301, "order": "Attack" }
  ],
  "hash": "..."
}
```

Playing it again is the rules alone, headless, with a plugin that feeds the
orders at their ticks: `play_replay` in `game.rs`, `legions --replay` on the
command line. Raw input (keys and the pointer in window pixels) is not what
gets recorded: it would tie the replay to the window size and the camera.
The engine's [input scenarios](headless.md) replay raw input, which is how
the tests play the windowed game: they script the mouse and keys through
headless frames, let the game's own recorder write the file, and check that
the rules alone replay it to the same hash:

```rust
{{#include ../../tests/legions.rs:record}}
```

## What keeps it bit for bit

The engine's [determinism rules](determinism.md) do most of it: the fixed
tick, systems in the order they were added, queries in the same order,
`random.of(entity)` for per-unit randomness in `par_for_each`, deterministic
grids and sorting. The game adds four habits:

- **Decide in parallel, apply in order.** `fight` only writes each unit's
  own components and an `act` (hit this enemy, shoot at this point);
  `resolve` applies the acts one after another, so the result never depends
  on which thread finished first.
- **No sine or cosine in the rules.** Platforms' math libraries can differ
  in the last bit of `sin`, `cos` and `atan2`, so the rules use plain
  arithmetic: random offsets come from a square, not a circle, and an
  arrow's angle (`atan2`) is only for its picture.
- **The view stays out.** Sprites, the camera and timings are not game
  state; a test checks that the same battle with and without sprites ends
  with the same hash.
- **A hash by entity.** `world_hash` hashes every unit, arrow and fallen
  unit by its entity, so the order they sit in memory doesn't matter, then
  the score, the goals and the stances:

```rust
{{#include ../../examples/legions/game.rs:hash}}
```

## How it is verified

`tests/legions.rs` (no window, no GPU):

| Test | What must hold |
|---|---|
| `the_same_seed_gives_the_same_battle_and_the_armies_really_fight` | two runs end with the same hash, both armies lost units |
| `the_hash_is_the_same_on_one_thread_and_on_many` | one thread, four, and all cores end with the same hash |
| `sprites_do_not_change_the_battle` | with sprites (one per unit, arrow and corpse) and without, the same hash |
| `recorded_input_replays_to_the_same_world_hash` | scripted mouse and keys on the windowed game, recorded, replay to its hash; without the orders, a different battle |
| `quitting_with_escape_writes_a_replay_of_every_tick_so_far` | Escape writes the ticks run so far, and they replay to the hash |
| `watching_a_replay_ignores_the_mouse_and_keys` | while a replay plays (`--replay --watch`), the controls give no orders |
| `the_committed_replay_ends_with_its_hash_on_one_thread_and_on_many` | `examples/legions/replays/skirmish.json` ends with its recorded hash |
| `arrows_fly_land_and_kill`, `in_a_full_battle_arrows_hit_and_both_armies_lose_units` | arrows alone kill a soldier; arrows hit in a battle |
| `hold_keeps_the_red_army_in_place_and_rally_moves_it` | orders steer the army |
| `a_round_ends_and_the_next_begins` | rounds end and restart on their own |

On other platforms: the committed replay ended with its hash on macOS on
Apple silicon (aarch64) and on the same Mac running the x86_64 build under
Rosetta, and the test suite runs it on every platform that runs
`cargo test`. The replay is not checked in a browser: there is no headless
runner for the web build.

The picture is checked offscreen by `src/render/legions_render_test.rs`
(see [GPU regression tests](render-tests.md)): the example's own rules play
330 ticks of a 4,000-unit battle, its own frame systems and the engine's
sprite drawing fill the draw list, and the window's sprite pass draws it into
a texture. Both armies show in their colours on their sides, the ground
shows around them, all 4,241 sprites are two instanced draws, the same battle
without sprites shows no army (the negative control), and the frame matches
the reference image `tests/fixtures/render/legions_battle.png`.

## Numbers

Measured on an idle Apple M4 Max (14 cores) on 2026-11-22, revision
`9235ea05`, three runs each (1-minute load 1.9 to 2.8). Release build,
headless, seed 1, `legions --headless --units N --ticks 600`; the first 600
ticks are the march and the first clash:

| Units | Map (cells) | Sim tick, all cores (mean of a run, 3 runs) | p95 | CPU work per tick, one thread |
|---:|---|---|---|---:|
| 10,000 | 400 x 225 | 0.50, 0.50, 0.52 ms | 0.75 to 0.76 ms | 1.65 ms |
| 100,000 | 894 x 503 | 2.53, 2.55, 2.56 ms | 3.76 to 3.82 ms | 13.4 ms |
| 250,000 | 1414 x 795 | 5.52, 5.56, 5.76 ms | 8.0 to 8.2 ms | 30.0 ms |

The same runs on a machine busy with other jobs (load average 100 to 150)
took 12 to 16, 23 to 28 and 43 to 58 ms a tick on all cores, and 2.3 to
2.7 and 16.6 ms of CPU work.

"CPU work" is the process's CPU time per tick with `--threads 1`, less a
run of no ticks (the three runs agreed within 1%): the work itself, which
load changes far less than wall time. On all cores 100,000 and 250,000
units take about a fifth of the one-thread work.
`fight` is 70 to 85% of a tick (85% at 10,000 units, 70% at 250,000, where
`aim` and `resolve` take 0.8 ms each). `--headless` prints the tick-time
percentiles and each system's share.

**Frame time.** Opening a window is not part of these runs, so the frame
is measured offscreen by the ignored test `legions_frame_cost` in
`src/render/legions_render_test.rs`, at 1280 x 720 with the whole map in
view, 300 ticks into a battle: "fill" is the `animate` frame system, the
ground and the engine's sprite drawing into the draw list; "frame" adds
sorting and batching, the instance upload, the GPU pass and reading the
picture back (which a window doesn't do). Medians of 40 frames, three runs
on an idle machine (2026-11-22, revision `9235ea05`, load average 2.4 to
2.7), which agreed within 0.03 ms:

| Units | Fill | Whole frame |
|---:|---|---|
| 10,000 | 0.08 ms | 1.16 to 1.17 ms |
| 100,000 | 0.24 ms | 2.68 to 2.71 ms |
| 250,000 | 0.40 to 0.41 ms | 5.78 to 5.79 ms |

Measured before on the busy machine above, the same frames took 5 to
43 ms, with a spread between runs bigger than the difference between
100,000 and 250,000 units.

```sh
cargo test --release --lib --features 3d legions_frame_cost -- --ignored --nocapture
```

**Against Bevy.** The issue asks for the same workload on Bevy through the
engine comparison scoreboard crate (issue #21). That crate doesn't exist
yet, so there is no comparison here.

## What it doesn't do

- **Fog of war** (optional in the issue): not done. Each army sees the whole
  map.
- **No raw-input replay.** Recordings hold orders, not keys and the pointer
  (see above).
- **C and C++.** Legions is Rust only and adds no engine API; the flow
  fields and grids it uses are in the C++ header too, as the
  [C++ battle demo](c-cpp.md#speed) shows.

## Engine gaps it found

Filed as follow-up issues; the first five are in the engine now:

- A public, fast world hash: [`world_hash`](determinism.md#one-number-for-the-state)
  hashes the reflected state in place. The game keeps its own hash, which
  covers exactly its rules' state without reflecting it.
- Recording live input into an `InputScenario`:
  [`ctx.input.start_recording`](actions.md#recording-live-input).
- Read-only queries on `&World`: `world.query_ref`, which the game's hash
  uses.
- `Rng::unit_vec2` used the platform's `sin` and `cos`; it is bit-identical
  across platforms now.
- `SpatialGrid::nearest` scanned every cell in the radius; it goes outward
  ring by ring now.
- `ponko package` puts extra files only at the package's top level (`to`
  is a file name), so the replay can't go in a `replays/` folder.
- A headless whole-frame benchmark for 2D games: the frame cost above comes
  from a test that drives the sprite pass by hand.
