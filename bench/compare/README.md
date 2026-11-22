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
```

It builds `ponko_bench` (feature `render`) and this crate once each, in
release. Then it runs every workload three times per engine, one process at
a time, changing which engine goes first on each run. Before each run it
waits up to ten minutes for a 1-minute load average under 3. It records the
load next to the run and marks a run taken above it. It writes
`results/scoreboard.json` and `results/scoreboard.md` and prints the table.
A row is "within noise" when the two engines' ranges over the runs overlap.

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

Different (read the numbers with these in mind):

- **Pipelined rendering.** Bevy can overlap a frame's simulation with the
  previous frame's rendering. That is off here. With it on, Bevy's frame
  time on a multi-core machine would be closer to the larger of the two
  halves, not their sum.
- **MSAA in 2D.** Ponko's sprite and UI rows draw with its window renderer,
  which has 4x MSAA. Bevy's 2D camera here has `Msaa::Off` and no tone
  mapping, its cheapest setting. The 3D scenes use 4x MSAA on both sides.
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
  both sides.
- **Text** is in the small game only. Each engine draws it with its own
  built-in default font (Bevy's `default_font` feature).

An empty cell means that side cannot run the workload headless. There are
none at the moment.

## Files

- `src/ecs.rs`: the shared ECS components with both engines' derives, and
  the timing harness (the same semantics as `ponko_bench`'s `Bench`).
- `src/ecs_bevy.rs`: the bevy_ecs halves and the cross-checks.
- `src/scenes.rs`: the render scenes (see `crates/ponko_bench/RENDER_SCENES.md`).
- `src/workloads.rs`: sim, physics, sprites, UI, first frame.
- `projects/`: the build-time projects. `PONKO_PATH` is replaced with the
  checkout's path when they are copied.
- `scoreboard.py`: the one command. `results/`: its last output.
