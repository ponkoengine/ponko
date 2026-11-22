# Rendering baseline: eight offscreen scenes (#134)

A reproducible measurement of the 3D renderer, to be recorded before and
after rendering work: eight generated scenes drawn by the real renderer into
an offscreen texture, each frame split into its CPU steps and its GPU
passes, with draw calls, uploads, allocations and memory, and with the GPU,
the limits and the quality settings written next to the numbers.

It opens no window and waits for no display. It needs a GPU and the bench's
`render` feature:

```sh
PONKO_BENCH_JSON=render.json PONKO_RENDER_CSV=render.csv \
  cargo run --release -p ponko_bench --features render -- render/scene
```

The scenes live in `src/render/gpu3d/scene_bench.rs` (the hidden
`ponko::render::SceneBench`), the runner in `src/render_scenes.rs`, the
report (JSON, CSV, comparison) in `src/render_report.rs`.

## The scenes

Every scene is generated: meshes from `MeshData`, 64×64 checker textures,
placement from one fixed pseudo-random sequence. Nothing is loaded from disk,
and two runs build the same scene (a test builds every scene twice, draws
both and compares their contents and counts). The camera circles the scene's center, 0.004 radians a frame, so a
run covers many views. Counts are at scale 1.

| Scene | What it holds | What it stresses |
|---|---|---|
| `single-material` | 100 ordinary cube models of one mesh and material | Simple-scene regression control |
| `static-world` | 8 static instance batches of 25,000 boxes and spheres (200,000 instances), a ground | Static batches: culling and drawing instances that never move |
| `dynamic-world` | 20,000 models of 16 meshes, every one moved and turned every frame | Collecting moved models, instance uploads, batching by mesh |
| `multi-material` | 6,000 models of 512 meshes, each mesh with its own material: 32 textures, opaque and cut-out, metallic, emissive, double-sided | Pipeline, bind group and buffer changes between draws |
| `shadow-heavy` | 4,000 shadow casters (half of them moving), a batch of 6,000 pillars, 3 point and 6 spot lights that all cast shadows, four 4096² sun cascades redrawn every frame out to 120 units | Shadow passes: cascades and the lights' shadow atlas |
| `alpha-heavy` | 4,000 moving see-through models and 1,000 cut-out cards over a ground | Sorting and blending, overdraw |
| `animated` | 400 skinned figures (16 joints each) posed every frame | Skinning and the skin upload |
| `game-like` | A small town: terrain, buildings in 4 static batches, 30,000 trees in a batch with two levels of detail, 400 vehicles on the streets, 80 skinned walkers, 48 point and 8 spot lights (6 with shadows), 300 see-through panes, a lake, bloom, occlusion culling | Everything at once, in the proportions of a game |

`game-like` is made of the engine's own generated shapes. It stands in for a
game; it is not one.

All scenes start from a `GraphicsQuality` preset (`high` unless
`PONKO_RENDER_QUALITY` says otherwise) with the atmosphere on; a scene then
changes what it is about (the shadow scene's cascades, the town's bloom).
The settings each scene really drew with are in its report
(`scenes[].scene.quality`).

## What a frame measures

One frame is: move the scene (`update`, not part of the frame time), then

| Step | What it is |
|---|---|
| `collect` | What a game's frame does before the renderer: pose skeletons, gather every model, light and batch the camera and the shadow maps see |
| `prepare` | `Renderer3D::prepare`: culling, sorting, uploads, shadow setup |
| `encode` | `Renderer3D::draw`: recording every pass into command buffers |
| `submit` | `Queue::submit` |
| `cpu` | The four steps above, added |
| `frame` | From the start of `collect` until the GPU has finished the frame (the run waits for it) |

A frame waits for the GPU before the next begins, so `frame` is the cost of
one frame alone, not a frame rate with frames in flight.

GPU times come from timestamp queries around each pass: `gpu_ms.total` adds
the passes of a frame, `gpu_ms.span` is from the first pass's start to the
last one's end, `gpu_ms.passes` lists each pass. Passes that draw nothing in
some frames have fewer samples.

Counts per frame: draw calls, recorded draws, instances and triangles for
the camera and for the shadow maps, models skipped by occlusion culling,
bytes uploaded (split by kind), buffers created, pipelines built, pipeline /
bind group / vertex buffer / index buffer changes, command buffers and
submissions (with views: every camera's command buffers and the copy into
the target).
Triangles and instances count what the CPU hands the renderer: the instances
of static batches are drawn from their own buffers and are reported under
`instance_batches` instead.

Allocations per frame are the whole process's during the measured frames
(the bench's counting allocator), wgpu's included.

### What is unavailable, and how the report says so

Nothing is left out silently, and nothing unavailable is written as 0:

- **GPU pass times** need timestamp queries. Without them (a software
  adapter, WebGL2, `PONKO_GPU_TIER=webgl2`), `gpu_ms.total` and `span` are
  `null`, `passes` is empty, `timed_frames` is 0 and `gpu_ms.unavailable`
  says why. The CSV writes the rows with empty numbers and the reason in the
  `note` column.
- **Features** the device lacks are listed in `gpu.capabilities.features`
  with `available: false`, the `reason` and the `fallback` the renderer uses.
- **Memory** comes in three kinds, kept apart:
  - *Counted* from the renderer's own tables (`memory.counted_bytes`): mesh
    buffers, the game's GPU buffers, the GPU-driven batches' pools and the
    scene's textures.
  - *Computed from sizes* (`memory.computed_bytes`, with `memory.computed`
    saying so): `render_target_bytes` (the scene's multisampled color and
    depth, the HDR and scene-copy targets, the output), `shadow_map_bytes`
    (the sun's cascades) and `light_shadow_atlas_bytes`, each width × height
    × layers × samples × the format's bytes. These are what the textures
    hold, not what the device reports having allocated.
  - *Uncounted*, named in `memory.uncounted`: the effects' own targets
    (ambient occlusion, reflections, antialiasing, motion vectors, bloom and
    the post chain), history textures, the instance buffers of static
    batches culled on the CPU and the per-frame instance, uniform and light
    buffers.

  The device's allocator totals are there where wgpu reports them; on Metal
  it does not, and `memory.device_unavailable` says so.
- **Peak resident memory** of the process is read with `getrusage` on Unix;
  elsewhere `process.peak_rss_unavailable` says it is not read. It is one
  number for the whole run, not per scene.
- **The driver** is whatever wgpu reports; Metal reports none (`""`).

## Settings

| Variable | Default | Meaning |
|---|---|---|
| `PONKO_RENDER_FRAMES` | 600 | Measured frames per scene |
| `PONKO_RENDER_WARMUP` | 120 | Frames drawn before measuring (pipelines, first uploads) |
| `PONKO_RENDER_SIZE` | `1280x720` | The offscreen target |
| `PONKO_RENDER_SAMPLES` | 4 | MSAA samples asked for: 1, 2, 4 or 8. Where the adapter can't multisample the 3D targets that much, 4 is used; the report has both (`gpu.target.samples_requested`, `samples`) |
| `PONKO_RENDER_QUALITY` | `high` | `low`, `medium`, `high` or `ultra` |
| `PONKO_RENDER_SCALE` | 1 | Multiplies every scene's object counts |
| `PONKO_RENDER_MATCHED` | 0 | 1: draw only what the Bevy copies draw too (see below) |
| `PONKO_RENDER_TAA` | 0 | 1: temporal antialiasing instead of the preset's, so motion vectors are drawn (no preset turns TAA on) |
| `PONKO_RENDER_SHADOW_FILTER` | the preset's | The sun shadow filter instead of the preset's (and the scene's): 1 `Sharp`, 2 `Soft`, 3 `Softest`, 4 `ContactHardening` |
| `PONKO_RENDER_WIND` | 0 | 1: `game-like`'s tree crowns sway in a material `vertex` function, and 4,096 cubes are placed from a GPU buffer of 262,144 elements that keeps its last frame (what motion vectors of `vertex` functions and buffers cost) |
| `PONKO_RENDER_VERTEX_MOTION` | 1 | 0: the motion pass ignores what `vertex` functions do (the control for `PONKO_RENDER_WIND` with TAA) |
| `PONKO_RENDER_VIEWS` | 1 | 2 or 4: split screen, the main camera and one or three views (`ctx.scene3d.views`), each circling the scene from its own side, drawn into their parts of the target as a game's window draws them. `minimap`: the main camera on the whole target and a quarter-sized view in its top right corner with bloom, ambient occlusion, contact shadows and screen-space reflections off and FXAA (`View3D::with_post`), as a game's minimap. Preparing and encoding interleave there, so `prepare` is 0 and `encode` holds both; `submit` is the copy into the target |
| `PONKO_RENDER_GPU_EVERY` | 1 | Times the GPU passes of one frame in this many, as a game does outside profiling (16; see "Cost" in the guide's viewports chapter): what timing every frame costs in allocations. GPU times then have fewer samples |
| `PONKO_GPU_TIER` | unset | Opens the device as a weaker GPU (`webgl2`, `baseline`), as the engine's tests do |

A value that doesn't parse stops the run and names the variable.

The filter picks scenes: `-- 'render/scene shadow-heavy'`, or several with
`|`.

## Output

| Variable | File |
|---|---|
| `PONKO_BENCH_JSON=file.json` | The bench's usual document (`schema_version`, `env`, `results`) with a `render` section |
| `PONKO_RENDER_CSV=file.csv` | One row per scene and metric: `scene,group,metric,unit,samples,min,p50,p95,p99,max,mean,note` |
| `PONKO_RENDER_FRAMES_CSV=file.csv` | One row per measured frame: every step, GPU total and span, the counts |
| `PONKO_RENDER_PNG=dir` | The last frame of each scene as `dir/<scene>.png`, to look at what was measured |
| `PONKO_ALLOC_SITES=n` | After the measured frames, `n` more frames with the call stack of every allocation taken, printed to stdout grouped by site: the innermost engine function, the outermost `wgpu` call under it and the innermost function that allocated, with the count a frame. `PONKO_ALLOC_SITES_DUMP=1` also prints the whole stacks of allocations outside the engine. Slow (a stack per allocation): use a few frames |

The JSON document:

- `env`: Rust version, git revision and whether the tree was dirty, OS, CPU,
  cores, features, and the machine's load averages when the run started.
- `results`: one record per scene and step (`render/scene <scene> collect`,
  `prepare`, `encode`, `submit`, `cpu`, `frame`), the same record as every
  other benchmark, with `p99_ms` added.
- `render.settings`: resolution, samples, quality, scale, frames, warm-up,
  and that frames are offscreen.
- `render.gpu`: adapter (name, backend, device type, driver), vendor and
  device ids, the limits the renderer depends on, every optional feature
  with its availability, the forced tier if any, the target's format and
  sample count, whether timestamp queries exist.
- `render.process`: peak resident memory.
- `render.scenes[]`: `scene` (content counts and the quality settings
  drawn with), `timings_ms` (a distribution per step: samples, min, p50,
  p95, p99, max, mean), `gpu_ms`, `counts`, `upload_bytes_per_frame`,
  `instance_batches`, `state_changes`, `command_buffers`, `submits`,
  `allocations_per_frame`, `allocation_bytes_per_frame`, `memory`.

`render.schema_version` is 1. The section reuses the engine's own types and
counters (`GpuCapabilities`, `GpuTimer` spans, `GpuUploads`,
`InstanceBatchStats`, `GraphicsQuality`); it is not a second schema for them.

## Comparing two runs

```sh
cargo run -p ponko_bench --bin render_compare -- before.json after.json
```

prints the medians and 95th percentiles side by side with the change, per
scene and step. It first checks that the runs can be compared: the same
adapter, backend and driver, resolution, samples asked for and used,
quality, scale, measured and warm-up frame counts, GPU tier, OS and CPU,
the same scenes in both, and the same content and settings in each. If
anything differs it prints every difference, prints no table, and exits
with 1.

The table does not say which differences are noise. Read the 95th and 99th
percentiles and both runs' `env.load_average`.

## The committed baseline

`baselines/render.json` and `baselines/render.csv`, recorded 2026-11-22 at
`9235ea05` (the engine's sources as at that revision; the files record
`git_dirty` because only these two baseline files differed) on an Apple M4
Max (14 cores), Metal, wgpu 30.0.1, Rust 1.100.0, release profile, features
`physics`, `physics3d`, `navmesh`, `window`; 1280×720, 4× MSAA, `high`,
scale 1, 120 warm-up and 600 measured frames. Peak resident memory of the
run: 225 MB. The files hold the memory sizes computed for render targets
and shadow maps (`render_target_bytes`, `shadow_map_bytes`,
`light_shadow_atlas_bytes`, `computed_bytes`).

The machine was idle (load averages 1.64 3.23 3.37 when the run started).
Two more runs right before it gave frame medians within 0.1 ms of these in
every scene except `dynamic-world` (6.33 to 6.54 ms). Re-record it with:

```sh
PONKO_BENCH_JSON=crates/ponko_bench/baselines/render.json \
PONKO_RENDER_CSV=crates/ponko_bench/baselines/render.csv \
  cargo run --release -p ponko_bench --features render -- render/scene
```

Milliseconds, medians unless a percentile is named:

| Scene | CPU | Frame | Frame p95 | Frame p99 | GPU passes | Draw calls | Triangles (camera / shadows) |
|---|---:|---:|---:|---:|---:|---:|---:|
| `single-material` | 0.299 | 1.634 | 1.850 | 1.941 | 1.018 | 25 | 1,200 / 2,220 |
| `static-world` | 0.421 | 4.886 | 5.681 | 5.793 | 4.151 | 26 | 2 / 6 |
| `dynamic-world` | 2.876 | 6.457 | 6.733 | 6.915 | 2.892 | 107 | 721,530 / 241,252 |
| `multi-material` | 1.740 | 5.513 | 5.975 | 6.260 | 2.966 | 2,079 | 219,480 / 314,276 |
| `shadow-heavy` | 1.186 | 6.353 | 6.568 | 6.743 | 4.890 | 259 | 133,686 / 614,002 |
| `alpha-heavy` | 2.554 | 17.731 | 19.027 | 21.169 | 14.247 | 87 | 142,866 / 339,270 |
| `animated` | 1.440 | 6.670 | 7.507 | 7.616 | 4.852 | 2,139 | 145,922 / 373,256 |
| `game-like` | 0.640 | 3.411 | 3.648 | 3.870 | 2.484 | 112 | 38,280 / 319,360 |

`static-world` shows 2 triangles because its 200,000 instances are in static
batches (see the counts note above).

| Scene | Collect | Prepare | Encode | Submit | Allocations a frame | Counted GPU bytes |
|---|---:|---:|---:|---:|---:|---:|
| `single-material` | 0.010 | 0.023 | 0.227 | 0.039 | 542.0 | 2,308 |
| `static-world` | 0.008 | 0.074 | 0.299 | 0.041 | 563.4 | 24,580 |
| `dynamic-world` | 1.429 | 0.959 | 0.410 | 0.050 | 661.0 | 49,156 |
| `multi-material` | 0.152 | 0.229 | 1.223 | 0.120 | 901.9 | 2,271,876 |
| `shadow-heavy` | 0.371 | 0.434 | 0.347 | 0.056 | 764.9 | 40,964 |
| `alpha-heavy` | 0.465 | 1.044 | 0.980 | 0.065 | 731.6 | 42,328 |
| `animated` | 0.123 | 0.224 | 0.994 | 0.096 | 951.1 | 41,044 |
| `game-like` | 0.098 | 0.133 | 0.348 | 0.050 | 772.3 | 1,630,140 |

Against the baseline of 2026-11-20 (`e60e15a8`, also idle), the CPU got
faster in every scene (by 3 to 35%: `alpha-heavy` 3.91 → 2.55 ms, its
encode step 2.67 → 0.98 ms), but **the GPU got slower in every scene and
the frames with it**: GPU passes 0.75 → 1.02 (`single-material`), 3.70 →
4.15, 2.13 → 2.89, 2.23 → 2.97, 3.40 → 4.89, 12.65 → 14.25, 3.63 → 4.85
and 1.87 → 2.48 ms; frame medians up by 1 to 27% (`shadow-heavy` 5.02 →
6.35, `animated` 5.81 → 6.67, `dynamic-world` 5.84 → 6.46). Draw calls and
triangles are unchanged. The cause is one setting: the `high` preset now
turns on screen reflections' thin objects (`ssr_thin_objects`, the only
quality field that differs between the two files). The screen reflections
pass went from 0.24 to 0.47 ms to 0.34 to 2.31 ms (`shadow-heavy` 0.47 →
1.88, `static-world` 0.43 → 1.15, `animated` 0.38 → 1.33, `alpha-heavy`
from none to 2.31), plus the coarse depth and reflection depth passes
(0.01 to 0.03 ms each). That is far over the 0.2 ms the setting was
measured at over a floor and rails in the guide: these scenes have edges
everywhere. Allocations a frame rose by 14 to 34 in every scene; that was
not looked into.

On 2026-11-20, against the heavily loaded baseline of 2026-11-11, frame
medians moved like
this: `dynamic-world` 8.34 → 5.84, `multi-material` 8.89 → 4.93,
`shadow-heavy` 9.25 → 5.02, `static-world` 4.93 → 4.84, `game-like` 2.86 →
3.03, `animated` 4.24 → 5.81 and `alpha-heavy` 9.54 → 17.47. The first three
mostly show what the old load did to them. Two scenes got slower then
because of the engine: see-through models drawn one by one in sorted order
(`alpha-heavy`, 18 → 3,513 recorded draws) and skinning keeping last
frame's joints for the motion vectors (`animated`, the skinning pass 1.14
→ 1.82 ms); both also drew more shadow casters.

The counted bytes are small because what dominates GPU memory here is not
in the renderer's tables: render targets and shadow maps (computed from
their sizes) and the CPU-culled batches' instances (uncounted), and Metal
gives no allocator totals. The per-pass GPU times, upload split and state
changes are in the files.

`cargo test -p ponko_bench` checks that the committed files hold every
scene with every timing, GPU times or the reason, and memory counted or
named as uncounted.

## Side by side with Bevy (optional)

`bench/compare` (the cross-engine scoreboard, see its README) builds four
of the scenes again with Bevy 0.20.1: `static-world`, `dynamic-world`,
`multi-material` and `animated` (1,000 skinned figures of 16 joints, each
joint an entity, posed every frame). It is a crate of its own with its own
workspace and lock file: Bevy is not a dependency of the engine, the bench
or any default build, and the crate is not published.

```sh
# Ponko, drawing only what the Bevy copies draw too:
PONKO_RENDER_MATCHED=1 PONKO_BENCH_JSON=ponko.json PONKO_RENDER_CSV=ponko.csv \
  cargo run --release -p ponko_bench --features render -- \
  'render/scene static-world|render/scene dynamic-world|render/scene multi-material|render/scene animated'

# Bevy (reads the same PONKO_RENDER_FRAMES, WARMUP, SIZE, SCALE and PNG;
# a scene name after `scenes` runs only that scene):
cd bench/compare
PONKO_BENCH_JSON=bevy.json PONKO_RENDER_CSV=bevy.csv cargo run --release -- scenes
```

Both write the same CSV columns, so the rows can be put in one sheet.

**Matched** (the same code or the same numbers on both sides):

- Meshes: the vertices and triangles of `MeshData::cube`, `sphere` and
  `plane`, ported line for line, so both draw the same triangles. The
  animated figure: the same column of 17 rings of 12 sides, the same joint
  weights (0.7 and 0.3 to the two nearest joints), the same sway per joint.
- Placement, sizes, rotations and motion: the same sequence and formulas.
- Materials: base color, roughness, metallic, emissive, cut-out at 0.5,
  double-sided; the same generated textures with a full mip chain, linear
  filtering, repeat.
- Camera: 60° tall, near 0.1, the same far plane, the same circle, the same
  frame index.
- Sun: the same direction, color and strength, shadows from 4 cascades of
  2048² out to 80 units, all redrawn every frame.
- Target: 1280×720 `Rgba8UnormSrgb`, 4× MSAA, FXAA, ACES tone mapping, no
  bloom, no ambient occlusion, no reflections, black behind the scene.
- Loop: one thread simulates and renders (Bevy's pipelined rendering is
  off), pipelines are compiled before use, each frame waits for the GPU.
  `cpu` is everything from the scene's world to the submit, `frame` adds
  the wait for the GPU.

**Not matched** (read the numbers with these in mind):

- **Skinning.** Ponko poses a figure by writing its joints' rotations into
  the pose of one `Skeleton` component; Bevy's figure is 16 joint entities in a parent chain whose
  `Transform`s a system sets each frame, and Bevy propagates them, which is
  how Bevy skins (`SkinnedMesh`). The poses are the same.
- **Static instances.** Ponko's `static-world` uses its static instance
  batch API: 8 batches. Bevy has no such API; its copy is 200,000 entities
  that Bevy batches itself each frame. This is the largest difference and
  it is a difference in what each engine offers, not in the scene.
- **GPU times.** The Bevy program reports none, with the reason: Bevy's
  render diagnostics record GPU timestamps on Vulkan and DX12 only, and the
  baseline machine runs Metal. Only `cpu` and `frame` can be set side by side.
- **Ambient light.** Ponko blends a ground color and a sky color by the
  surface's direction; Bevy's ambient light is one color (the middle of the
  two is used). Bevy's shaded sides come out somewhat darker.
- **Tone curve.** Ponko uses Narkowicz's ACES fit, Bevy the fitted ACES
  transform; the Bevy camera's exposure is set to 1/0.6 to bring the
  brightness together. Saturated colors still differ.
- **Shadow filtering and bias**, cascade split distances and the shading
  model's details (specular terms, Fresnel) are each engine's defaults.
- **Culling and batching** are each engine's own: frustum culling on both,
  no occlusion culling on either.
- **Texture mips** are made by each side's own box filter.
- The Bevy copy counts no draw calls, uploads or memory.

The pictures (`PONKO_RENDER_PNG`) of both were compared by eye at the same
frame: objects, camera and shadows line up; brightness and color differ as
described above. That is the extent of the image match: the two renderers'
images are close, not equal.

Recorded together with the baseline, minutes apart, on the same idle
machine at `e60e15a8` (`baselines/render-matched.json`, `render-matched.csv`,
`render-bevy.json`, `render-bevy.csv`, 2026-11-20; load averages 1.56 4.99 4.82
and 2.08 5.04 4.83 when each started). Milliseconds:

| Scene | CPU p50 Ponko | CPU p50 Bevy | Frame p50 Ponko | Frame p50 Bevy | Frame p95 Ponko | Frame p95 Bevy | Frame p99 Ponko | Frame p99 Bevy |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `static-world` | 0.220 | 7.006 | 3.042 | 10.303 | 3.247 | 12.374 | 3.324 | 14.039 |
| `dynamic-world` | 0.858 | 4.468 | 2.163 | 6.791 | 2.397 | 8.252 | 2.479 | 9.125 |
| `multi-material` | 1.634 | 2.079 | 3.394 | 3.779 | 3.781 | 4.437 | 3.976 | 5.647 |

What these numbers support, and what they do not:

- They are three generated scenes on one Apple GPU, one run each, on a quiet
  machine. They are not a claim that either engine is faster in general.
- `static-world` mostly measures that Ponko has an API for instances that
  never move and this Bevy scene does not use an equivalent. It is not a
  like-for-like comparison, and the 3.4× gap in frame time (3.04 against
  10.30 ms) should not be read as one engine drawing the same scene faster.
- `dynamic-world` is the like-for-like scene where Ponko is ahead: 2.16
  against 6.79 ms (3.1×), with a CPU step of 0.86 against 4.47 ms.
- `multi-material` is close: Ponko's frame median is 3.39 ms against
  Bevy's 3.78 ms (about 10% less), its CPU step 1.63 against 2.08 ms, and its
  95th and 99th percentiles (3.78 and 3.98 ms) are below Bevy's (4.44 and
  5.65 ms). One run each is not enough to call a winner.
- Bevy's pipelined rendering is off for both, so neither engine's CPU step
  overlaps the GPU as it would in a shipped game.
- Each engine is used in its plain, documented way. Neither side is tuned;
  a Bevy user could restructure these scenes, and so could a Ponko user.
- Other GPUs, backends, resolutions and scenes may order them differently.
  Record both on the machine you care about.

## Known artifact

With the atmosphere on (every scene of the plain run), the pictures show a
thin dark streak in the sky toward −X. It is an engine defect outside this
issue: `sky_radiance` in `src/render/mesh.wgsl` samples the sky atlas with a
linear, repeating sampler, and at the seam of the sky's azimuth it filters
across neighboring regions of the atlas. It does not change what the
baseline measures.

## Limits

- Offscreen only: no swapchain, no present, no display wait. A windowed
  game adds those; `PONKO_BENCH` (see the book's "Benchmark runs") measures
  a real game's frames.
- The scenes go through `Renderer3D` directly, mirroring what a game's
  frame does before it. The 2D renderer, UI and the window's final blit are
  not part of it.
- One machine, one backend. So far the scenes and their tests have run
  only on Metal (an Apple M4 Max), as the device is and with
  `PONKO_GPU_TIER=webgl2` and `baseline` forcing the weaker tiers on it.
  The tests open their device through the engine's shared render test
  helpers (a missing adapter is logged, or fails under `PONKO_REQUIRE_GPU`).
  They are written to pass on software adapters and other backends (small
  targets and shadow maps, no timing assertions, missing or unusable GPU
  timestamps accepted, nothing that depends on when occlusion results come
  back), but that has not been observed yet: the first runs
  on Vulkan, DX12 and software adapters will be CI's after the merge. The
  tests check that the scenes draw and report, never how fast.
- The harness adds no timing code to the renderer's frame path. It reads
  counters the renderer already keeps; the one change to the renderer is
  that `Renderer3D::take_state_changes` is no longer test-only (the counters
  it reads were already kept in release builds).

## Shadow-cache workload controls

`PONKO_SHADOW_WORKLOAD` specializes the same generated scenes for shadow
measurements, always with `shadow_refresh = 1`:

- `static`: fixed camera and fixed actors.
- `slow-camera`: static actors, camera orbit step 0.00001 radians per frame.
- `actors`: fixed camera, the scene's normal actor animation.
- `thrash`: normal camera and actor motion, plus a moving sun every frame.
- `vegetation`: as `slow-camera`, and `game-like` leaves out its skinned
  walkers (which draw every cascade they are in every frame) and circles
  low among the trees outside the town, so the trees' distance levels
  (their band edge 60 units away) lie all around the camera.
  `vegetation-moving` is the same with the camera at the usual speed.

`PONKO_RENDER_SHADOW_REFRESH` (1 to 8) sets `Lighting::shadow_refresh`
after the workload, and `PONKO_RENDER_SHADOW_LAG=1` turns on
`Lighting::shadow_lag`: see [far cascades that may lag](SHADOW_LAG.md).

Unset preserves the original baseline. The selected workload is recorded in
`scenes[].scene.shadow_workload`. Use `static-world` for the first two,
`dynamic-world` for moving actors and full invalidation; the latter moves all
models every frame. For example:

```sh
PONKO_SHADOW_WORKLOAD=slow-camera PONKO_RENDER_MATCHED=1 \
PONKO_RENDER_FRAMES=300 PONKO_RENDER_WARMUP=60 PONKO_RENDER_SCALE=0.1 \
PONKO_BENCH_JSON=shadows.json PONKO_RENDER_FRAMES_CSV=shadows.csv \
  cargo run --release -p ponko_bench --features render -- 'render/scene static-world'
```

Compare average shadow triangles and shadow GPU passes over all measured
frames, rather than the last frame alone (snapping deliberately creates
occasional refreshes). The frame CSV includes the work counts; JSON includes
CPU/GPU distributions, allocation counts, memory and adapter metadata.


For shared mesh arenas, global transparent order, and the cost of retaining
instancing versus sorting by depth, see [the draw-order measurements](DRAW_ORDER_156.md).
