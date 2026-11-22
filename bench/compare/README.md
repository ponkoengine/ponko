# bench/compare: Ponko against Bevy

The cross-engine scoreboard. Each workload is written twice, as an ordinary
Ponko game (`crates/ponko_bench`, rows named `compare/...`, `ecs/...` and
`render/scene ...`) and as an ordinary Bevy app (this crate). Both are timed
the same way, on the same machine, and listed side by side. The results and
the table are in [`book/src/performance.md`](../../book/src/performance.md),
"Ponko vs Bevy: the scoreboard".

This crate has its own workspace and lock file. Bevy is not a dependency of
the engine, `ponko_bench` or any default build, and the crate is not
published. Bevy is pinned with `=` in `Cargo.toml` (now **0.20.1**, the
newest release), and every result file and table names that version.

## One command

```sh
python3 bench/compare/scoreboard.py              # everything (about an hour)
python3 bench/compare/scoreboard.py --runtime    # only the run-time workloads
python3 bench/compare/scoreboard.py --builds     # only build times and sizes
python3 bench/compare/scoreboard.py --only sprites,ui
python3 bench/compare/scoreboard.py --tuned      # only the second table: both engines tuned
python3 bench/compare/scoreboard.py --tuned --runtime
```

It builds `ponko_bench` (feature `render`) and this crate once each, in
release. Then it runs every workload three times per engine, one process at
a time, changing which engine goes first on each run. Before each run it
waits up to ten minutes for a 1-minute load average under 3. It records the
load next to the run and marks a run taken above it. It writes
`results/scoreboard.json` and `results/scoreboard.md` and prints the table.
A row is "within noise" when the two engines' ranges over the runs overlap.
With no option it makes both tables: the first ("as a new project
starts") and the tuned one (`results/scoreboard-tuned.json` and `.md`,
[below](#the-tuned-table)), whose last column repeats the first table's
ratio and verdict for each row.

Single workloads:

```sh
cd bench/compare
cargo run --release -- ecs                      # the bevy_ecs rows (with Ponko's and hecs')
cargo run --release -- sprites compare/sprites-100k
cargo run --release -- scenes animated          # also reads PONKO_RENDER_* (see RENDER_SCENES.md)
cargo run --release -- sim | physics | ui | first-frame
cargo run --release -p ponko_bench --features render -- compare/  # Ponko's side (from the root)
```

`PONKO_BENCH_JSON=x.json` writes the records in `ponko_bench`'s shape.

## The workloads

| Row | What runs | Bevy's path |
|---|---|---|
| ECS: spawn/despawn, iterate 1M, fragmented iterate (1,024 archetypes), add/remove, parallel iterate, commands | `crates/ponko_bench/src/ecs_suite.rs` and `archetypes.rs`, compiled here as they are, plus the bevy_ecs halves in `src/ecs_bevy.rs`. One process runs Ponko, bevy_ecs and hecs. | A kept `QueryState` with `iter_mut().for_each` ([`QueryState`](https://docs.rs/bevy_ecs/0.20.1/bevy_ecs/query/struct.QueryState.html)), `par_iter_mut` on the compute task pool ([`QueryParIter`](https://docs.rs/bevy_ecs/0.20.1/bevy_ecs/query/struct.QueryParIter.html)), `spawn_batch`, and `Commands` applied with `World::flush` ([`World::commands`](https://docs.rs/bevy_ecs/0.20.1/bevy_ecs/world/struct.World.html#method.commands)). |
| Sprites: 100k and 1M moving, 100k with a different tint each | 32 x 32 sprites of one white texture bouncing (the bunnymark movement, the same random sequence), drawn into a 1280 x 720 image. | `Sprite` entities with one shared image, as Bevy's own [`bevymark`](https://github.com/bevyengine/bevy/blob/v0.20.1/examples/stress_tests/bevymark.rs). Bevy 0.20 draws a `Sprite` as a 2D mesh with a `SpriteMeshMaterial`, and equal sprites share one material (`bevy_sprite_render::sprite_mesh`). One color is therefore its fast path. A tint each means one material each: that is what the `-tinted` row measures. |
| 3D: 100k moving meshes with a 4-cascade sun shadow | The `dynamic-world` render scene at `PONKO_RENDER_SCALE=5`, with `PONKO_RENDER_MATCHED=1`. | `Mesh3d` + `MeshMaterial3d` entities sharing 16 meshes, which Bevy batches and instances on its own (GPU preprocessing); `DirectionalLight` with `CascadeShadowConfig`. |
| 3D: 1,000 skinned characters | The `animated` render scene at scale 2.5: 16-joint figures, every joint posed every frame, shadows. | `SkinnedMesh` with joint entities in a `ChildOf` chain, posed through their `Transform`s ([`custom_skinned_mesh`](https://github.com/bevyengine/bevy/blob/v0.20.1/examples/animation/custom_skinned_mesh.rs)). |
| 2D physics: 10k bodies | 10,000 dynamic 10 px boxes falling into a pile between walls, 1/60 s fixed step, gravity 980, 50 px a meter. | [`bevy_rapier2d`](https://docs.rs/bevy_rapier2d/0.37.0) 0.37 (rapier 0.36.1, the version Ponko uses), `TimestepMode::Fixed` with one substep. |
| UI: 10k nodes, still and all recolored every frame | A 100 x 100 grid of colored boxes filling 1280 x 720, drawn. | `Node` with a CSS grid (`RepeatedGridTrack::flex(100)`), `BackgroundColor`, a `UiTargetCamera`. The recolor row changes every `BackgroundColor` each frame. |
| Headless sim: 1M entities | One system: `pos += vel * dt`, one tick. | `MinimalPlugins` and an `Update` system ([headless example](https://github.com/bevyengine/bevy/blob/v0.20.1/examples/app/headless.rs)). |
| Time to first frame | Process start to the first offscreen frame done on the GPU, process exit included (the script times the process). | `DefaultPlugins`, no window, one frame, exit. |
| Builds and sizes | A hello world and a small game on each side: clean and incremental debug builds with [`scripts/build-times.sh`](../../scripts/build-times.sh) (three clean builds, then rebuilds after one-line edits in `src/lib.rs` and `src/main.rs`), one clean release build, the release binary (and stripped), the wasm32 release build (before `wasm-bindgen`, and gzipped). | `projects/bevy-hello` (`App` + `DefaultPlugins`) and `projects/bevy-small`, a port of Ponko's 2D template (`crates/ponko_cli/templates/2d`). Each project is copied to a fresh folder outside the checkout, so no workspace settings apply. |

## The tuned table

`scoreboard.py --tuned` runs every run-time workload again with each engine
set up the way a careful game built on it would ship, and makes the build
rows again for the small game. Same method as the first table: three runs
per engine, alternating, the load recorded, median and range. Each choice
below names its source; where two documented ways existed, both were
measured on this machine and the faster one kept (the numbers are from
2026-11-22, shipping profile, 1-minute load 2.1 to 2.9, one run each way
twice). `TUNED_PATHS` in `scoreboard.py` holds the choices, and every
result record names the way it was done (`path`).

**Both engines.**

- **One shipping profile**, `ship`: `lto = "fat"`, `codegen-units = 1`,
  `panic = "abort"`, otherwise Cargo's release profile. It is
  `[profile.ship]` in this crate's `Cargo.toml` and in the tuned build
  projects, and `SHIP` (the same settings through `--config`) for the
  engine's workspace. Ponko's templates ship with a `dist` profile of fat
  LTO and one codegen unit that keeps unwinding (the engine can catch a
  panicking job or system batch); none of these workloads panics, and
  both engines build and run with `abort`, so both get it.
- **rapier's performance features**: `parallel` and `simd8`, through
  `bevy_rapier2d`'s features of the same names and `ponko_bench`'s
  `rapier-parallel` and `rapier-simd8` (Ponko has no feature of its own
  for them; naming `rapier2d` with these features turns them on for the
  engine's rapier, which is the same crate). rapier 0.36.1 and
  bevy_rapier2d 0.37.0 have no `simd-stable` feature (their
  `[features]`); `simd8` (parry's 8-wide SIMD) is the SIMD switch there.
  Measured on Bevy's side: no feature 8.17 / 8.40 ms a tick, `parallel`
  4.86 / 4.53 ms, `parallel` + `simd8` 4.59 / 4.37 ms. On Ponko's side
  `parallel` alone 3.38 / 3.67 ms and with `simd8` 3.39 / 3.26 ms
  (within noise).
- **MSAA in 2D: off on both.** Ponko's 2D renderer draws sprites and UI
  with one sample (`src/render/gpu.rs`, `sprite_pipeline`:
  `MultisampleState::default()`, and its 2D passes have no resolve
  target); Bevy's 2D camera has `Msaa::Off`. Neither tone-maps 2D.
- **Time to first frame** ends both programs with `process::exit` right
  after the frame (`PONKO_COMPARE_EXIT=1` for Ponko's `first_frame`).
- **Threads and allocator**: the defaults. Neither engine's performance
  documentation recommends another allocator or thread count for these
  workloads (Ponko's performance chapter has none).

**Bevy 0.20.1.**

- **Pipelined rendering on**, its default: `DefaultPlugins` adds
  `PipelinedRenderingPlugin` when `multi_threaded` is on and the target is
  not wasm (`bevy_internal/src/default_plugins.rs`). The main world then
  runs frame N+1 while the render thread draws frame N. A frame is timed
  from one frame finished on the GPU to the next, in steady state: a
  system at the end of the render world's `Render` schedule
  (`RenderSystems::Cleanup`) waits for the GPU (`PollType::Wait`) and
  stamps the time (`src/finished.rs`). Every frame is still waited for, so
  the GPU never runs ahead; the frame is the longer of the two overlapped
  halves.
- **Only the features each group needs**, one tuned program per group,
  with `default-features = false` and Bevy's own feature collections
  (`docs/cargo_features.md` in the crate): `headless` (ECS, sim,
  physics: `std`, `multi_threaded`, `async_executor`), `sprites` (and
  first frame: `2d_bevy_render`, `default_app`), `ui` (`ui_bevy_render`,
  `default_app`), `scenes` (`bevy_pbr`, `bevy_light`, `bevy_core_pipeline`,
  `bevy_anti_alias`, `bevy_render`, `common_api`, `default_app`). No
  window, audio, gamepad, glTF, picking or scene features in any of them.
- **Many sprites: `Mesh2d` with a shared `ColorMaterial`** (bevymark's
  `--mode mesh2d`, `examples/stress_tests/bevymark.rs`), measured faster
  than `Sprite` entities (bevymark's default): 100k 6.91 / 6.93 ms against
  7.40 / 7.57 ms, 1M 62.5 / 68.0 ms against 66.8 / 73.8 ms.
- **Sprites with a tint each: Bevy 0.20 has no built-in path without a
  material per color.** A `Sprite` gets a `SpriteMeshMaterial` from a cache
  whose key includes the color (`bevy_sprite_render`,
  `src/sprite_mesh/mod.rs`, `SpriteMeshMaterialBucketKey { .., color, .. }`),
  and Bevy's own `many_sprites` stress test says of `--colored`: "This will
  cause the sprites to be rendered in multiple batches, reducing
  performance". A `ColorMaterial` is one material per color too
  (bevymark's `--vary-per-instance`). Both were measured: `Mesh2d` 37.2 to
  37.7 ms, `Sprite` 38.2 to 40.7 ms; the row uses `Mesh2d`. A custom
  `Material2d` reading each sprite's color from a storage buffer by
  `MeshTag` would avoid it (the 3D pattern of `examples/shader/storage_buffer.rs`),
  but that is a shader of the game's own, not a documented way to draw
  sprites, and it is not measured here.
- **Sprite movement and the sim** with `Query::par_iter_mut` (as
  `many_cubes` moves its cubes). The sim: 0.17 ms against 0.44 ms with
  `iter_mut`. The sprites: within noise of `iter_mut` (pipelined, the
  movement is not the longer half).
- **Skinned figures**: unchanged, joints posed through their `Transform`s
  (`examples/animation/custom_skinned_mesh.rs`). Bevy's other documented
  way, an `AnimationPlayer` with clips (`many_foxes`), adds clip
  evaluation and blending on top of the same joint transforms, so it is
  not faster for a pose written each frame.
- **10k UI nodes**: the CSS grid of the first table, measured faster than
  wrapping flex rows of fixed-size nodes (`many_buttons`' default layout):
  still 2.71 / 2.66 ms against 2.75 / 2.73 ms, recolored 4.62 / 4.72 ms
  against 5.49 / 5.31 ms.
- **Builds**: `dynamic_linking` for the debug builds
  (`--features bevy/dynamic_linking`, Bevy's `docs/cargo_features.md`:
  "improves iterative compile times"; not in release or wasm builds, as
  Bevy advises). The linker stays the default: Bevy's
  `.cargo/config_fast_builds.toml` says of macOS "The default ld64 linker
  is faster, you should continue using it instead". Its other advice is
  nightly-only and not used. The small game names only the features it
  uses: `2d_bevy_render`, `ui_bevy_render`, `default_app`, `bevy_winit`,
  `std`, `multi_threaded`, `default_font`, and `webgl2` (needed for the
  wasm build), `x11` and `wayland` (Linux windows; nothing on macOS).

**Ponko.**

- **Sprite movement and the sim** with `par_for_each` (the performance
  chapter's tip for big queries): the sim 0.07 ms against 0.165 ms with
  `for_each`, the sprites 1.03 / 1.04 ms against 1.15 / 1.17 ms (100k) and
  7.78 / 7.79 ms against 8.49 / 8.53 ms (1M).
- Sprites (tinted or not), the UI and the 3D scenes have one documented
  path each, the same as in the first table.
- **Features**: `ponko_bench` already builds the engine with
  `default-features = false`; it adds `physics3d`, `navmesh`, `mods-wasm`
  and `bench-internals` for its other benchmarks. They are compiled in
  and do nothing in these workloads (no system of theirs is added). The
  tuned small game builds the engine with `default-features = false,
  features = ["window"]`: no audio, gamepads, inspector or JPEG.
- **Builds**: the templates' debug profile, as in the first table (the
  2D template's `[profile.dev]`; the guide's "Linkers" leaves macOS on the
  system linker).

The tuned build rows are the small game only; the hello world was not
built again.

## Review note: what is the same and what is not

The same on both sides:

- Optimization: release builds with Cargo's default release profile on
  both sides (`opt-level = 3`, no LTO, 16 codegen units; neither workspace
  changes it), the same rustc, the same machine, native target, nothing
  else running (see the load columns).
- Counts, sizes, random sequences, time steps and the 1280 x 720
  `Rgba8UnormSrgb` target. The tests in `src/ecs_bevy.rs` check that the
  ECS workloads leave both worlds in the same state.
- Timing: a frame is one pass of the loop, everything the engine does for
  it, and the GPU rows wait until the GPU has finished the frame before the
  next. 60 warm-up frames, then 300 timed (120 for 1M sprites). The median
  frame is reported.
- One thread simulates and renders, and pipelines are compiled before use:
  Bevy's pipelined rendering is off (`PipelinedRenderingPlugin` disabled),
  because Ponko has no pipelined renderer, and Bevy's `WinitPlugin` is off
  because there is no window.
- The build projects use the 2D template's dev profile on both sides.
- **MSAA in 2D**: none on either side. Ponko's sprite and UI rows draw
  with one sample (its 2D pipeline, `src/render/gpu.rs`), and Bevy's 2D
  camera here has `Msaa::Off` and no tone mapping. (The first version of
  this note said Ponko's 2D had 4x MSAA; it does not.) The 3D scenes use
  4x MSAA on both sides.

Different in the first table (read the numbers with these in mind; the
tuned table changes what is marked *tuned*):

- **Pipelined rendering.** Bevy can overlap a frame's simulation with the
  previous frame's rendering. That is off here. With it on, Bevy's frame
  time on a multi-core machine would be closer to the larger of the two
  halves, not their sum. *Tuned: on.*
- **Release profile and features.** Cargo's default release profile and
  each engine's default features. *Tuned: the shipping profile on both
  sides, only the Bevy features each workload needs.*
- **Physics** runs inside Ponko's tick and through a plugin in Bevy. Both
  step the same rapier version once per tick. bevy_rapier also copies
  every body's position to its `Transform` each step, and Ponko writes it
  back to `Transform2D`.
- **The skinned figures** are posed differently. Ponko writes the joint
  rotations into the pose of one `Skeleton` component. In Bevy, the joints
  are 16 entities, and Bevy propagates their `Transform`s. That is how each
  engine skins.
- **Ponko's ECS rows** use `ponko_ecs` directly, as `ponko_bench` always
  has. The bevy_ecs rows use a bare `World`, not an `App`.
- **The small game** reads its settings from `assets/settings.json` (with
  live reload) on Ponko. Bevy has no JSON asset loader of its own, so its
  speed is a constant.
- **Adapters.** Each engine picks the high-performance adapter. Ponko never
  forces a lower quality tier in these runs.
- **Default features** differ in scope. Ponko's defaults are 2D with a
  window, audio, gamepads and the inspector; 3D is a feature
  (`--features 3d`). Bevy's defaults include 3D, glTF, animation and more.
  The build rows use each engine's defaults, as a new project does, so
  Bevy's builds compile more.
- **Time to first frame**: Bevy's program calls `process::exit` right after
  its frame. Ponko's returns from `main` and drops its world. One untimed
  launch of each comes first, so the file and shader caches are warm on
  both sides. *Tuned: both call `process::exit`.*
- **Text** is in the small game only. Each engine draws it with its own
  built-in default font (Bevy's `default_font` feature).

What still differs in the tuned table:

- **Pipelining is Bevy's alone.** Ponko has no pipelined renderer: its
  frame is the simulation and the drawing one after the other, Bevy's the
  longer of the two. That is each engine at its best, and it is the main
  reason Bevy's GPU rows move between the tables.
- **The 3D scenes' frame** in Ponko's run starts after the scene's own
  update (moving the meshes, posing the joints; `RENDER_SCENES.md`). In
  Bevy's tuned run that update is on the main thread, overlapped with the
  previous frame's drawing, and inside the measured interval. It only
  ever adds to Bevy's number.
- **Tinted sprites**: one material per color on Bevy (above), one batch
  on Ponko.
- **Physics**, **the skinned figures**, **the ECS rows**, **the small
  game's settings**, **adapters** and **text**: as in the first table.
  The ECS rows run the same code with the shipping profile.
- **The tuned Bevy programs** are four builds with different features;
  Ponko's run-time rows are one `ponko_bench` build. Features change what
  is compiled and which plugins run, so each Bevy group gets only its own.

An empty cell means that side cannot run the workload headless. There are
none at the moment.

## Files

- `src/ecs.rs`: the shared ECS components with both engines' derives, and
  the timing harness (the same semantics as `ponko_bench`'s `Bench`).
- `src/common.rs`: percentiles, the machine and writing results.
- `src/finished.rs`: the tuned programs' frame timing with pipelined
  rendering.
- `src/ecs_bevy.rs`: the bevy_ecs halves and the cross-checks.
- `src/scenes.rs`: the render scenes (see `crates/ponko_bench/RENDER_SCENES.md`).
- `src/workloads.rs`: sim, physics, sprites, UI, first frame.
- `projects/`: the build-time projects. `PONKO_PATH` is replaced with the
  checkout's path when they are copied.
- `scoreboard.py`: the one command. `results/`: its last output
  (`scoreboard.*` the first table, `scoreboard-tuned.*` the second).
- The features in `Cargo.toml`: `stock` (the default, the first table)
  and the tuned groups `headless`, `sprites`, `ui` and `scenes`. A tuned
  program by hand: `cargo run --profile ship --no-default-features
  --features sprites -- sprites compare/sprites-100k`.
