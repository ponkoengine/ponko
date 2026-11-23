# Mesh shaders and modern GPU geometry paths (#164)

**Recommendation: defer.** Do not change the default renderer, and do not
build a mesh-shader path into it now. Keep the probe and the offscreen
prototype in this change, so the question can be measured again when its
conditions move (see "What would change it").

Why, in one paragraph. On the one GPU that has them here (Apple M4 Max,
Metal), a task and mesh shader pipeline drew the same pictures as the
engine's way of drawing instance batches (a compute culling pass plus
indexed indirect draws with distance levels; checked on a small test scene,
within a tolerance, see "Correctness first"), but this prototype on this GPU
took **1.5 to 4.1 times as long on the GPU** in every scene tried: 1.8 to 3.2 times
as long with no culling at all (145 million triangles; 9 million in the
light scene), and with the same culling and levels 3.2 to 4.1 times as long
as the task shader design and 1.5 to 1.9 times as long as a mesh pipeline fed
by the engine's own culling pass, the closest of all. wgpu 30 offers the feature only as an experimental opt-in that needs
an `unsafe` token, it exists on no browser backend and no GL backend (so
never on the WebGL2 and baseline tiers the engine keeps), and hosted CI
runners can't test it. The thing mesh shaders add that this workload
could show, culling meshlets (frustum and normal cone) beyond whole
instances, removed 1 to 6% of the triangles and did not show a gain in time. Nothing
measured argues for a second geometry path.

The numbers are from one GPU, one prototype that was not tuned for Apple's
mesh pipeline (its meshlets are about half full, and its task workgroups
leave lanes idle; see "Why this prototype may be slow"), and a workload of
one kind of mesh; "Limits of this study" says what they don't show.

## Support matrix

wgpu 30.0.1 and naga 30.0.1 (the versions in `Cargo.lock`), read from their
sources in `~/.cargo/registry` rather than from memory. One row was measured.

| | Vulkan | Metal | DX12 | WebGPU (browser) | GL, WebGL2 |
|---|---|---|---|---|---|
| Feature flags | `EXPERIMENTAL_MESH_SHADER`, `_MULTIVIEW`, `_POINTS` (extension `VK_EXT_mesh_shader`) | the same three | `EXPERIMENTAL_MESH_SHADER`, `_MULTIVIEW` (no points) | none | none |
| Status in wgpu | experimental | experimental | experimental | not implemented: `create_mesh_pipeline` and `draw_mesh_tasks` panic ("MESH_SHADER feature must be enabled") | not implemented: `unreachable!()` in the GL backend; limits are 0 |
| Opt-in | `Features::EXPERIMENTAL_MESH_SHADER` in the device descriptor **and** `ExperimentalFeatures::enabled()`, an `unsafe` constructor whose contract is "you agree there may be undefined behaviour from bugs in these APIs" | same | same | - | - |
| Shader language | WGSL through naga to SPIR-V | WGSL through naga to MSL (**measured working here**; wgpu's own feature docs still say "naga is only supported on vulkan", which is out of date: the 30.0.1 Metal backend builds `MTLMeshRenderPipelineDescriptor` from naga's MSL) | WGSL through naga to HLSL (`naga::back::hlsl::mesh_shader`); not run here | - | - |
| WGSL syntax | non-standard: `enable wgpu_mesh_shader;`, `@task @payload(p)`, `@mesh(output_var)`, `var<task_payload>`, a `var<workgroup>` output struct with `@builtin(vertices)`, `@builtin(primitives)`, `@builtin(vertex_count)`, `@builtin(primitive_count)`, `@builtin(triangle_indices)`, `@builtin(mesh_task_size)`. Browsers' WGSL compilers don't know the extension; the engine's web build could not compile such a shader | same | same | - | - |
| Passthrough SPIR-V, MSL, HLSL | `Features::PASSTHROUGH_SHADERS` | same | same | same (listed) | no |
| Adapter condition (wgpu-hal) | `VK_EXT_mesh_shader` advertised | `family_check` and (Metal3 or Apple7 or Mac2 family), and the device name doesn't contain "virtual" ("mesh shaders don't work on virtual devices... CI thing") | D3D12 `MeshShaderTier` above none and shader model 6.5 or newer | - | - |
| Limits reported | the driver's `VkPhysicalDeviceMeshShaderPropertiesEXT` | by family: 1,024 threads per task and mesh workgroup, 256 output vertices and 256 primitives, task payload 16,352 B, task dispatch up to 2^32−1, mesh dispatch 2^22 (Apple10), 2^20 (Apple9), else 1,024 | fixed: 768 task threads per workgroup (the compute limit, `D3D12_CS_4_X_THREAD_GROUP_MAX_THREADS_PER_GROUP`), 128 mesh threads, 256 output vertices and 256 primitives, 8 output layers, payload 16,384 B, task and mesh dispatch 2^22 workgroups in total and 65,535 per dimension | - | - |
| Validation (wgpu-core) | feature required at pipeline creation; output vertex and primitive counts against the limits; task payload size against the limit and against the mesh stage's declaration; topology against the primitive state; direct `draw_mesh_tasks` against the per-dimension and total workgroup limits | same | same | - | - |
| Not validated | indirect draws: `draw_mesh_tasks_indirect` takes a GPU-written `x, y, z` that wgpu can't check against the limits ("we are not currently able to validate these"); the caller has to keep the kept-instance count times the meshlet count under the total limit | same | same | - | - |
| Known problems | Mesa: llvmpipe (the CI software Vulkan driver) fails to run naga's generated code, probably a Mesa bug (wgpu#8727, Mesa#14376); RADV works | on Apple GPUs a render pass's begin and end timestamps are the vertex stage's start and the fragment stage's end; for mesh pipelines the pass timings were unusable here (below) | not measured | - | - |

**Measured on this machine** (`the_adapter_reports_which_mesh_shader_features_it_has`,
which prints the row, and skips with a logged reason where there is no
adapter):

```
Apple M4 Max (Metal, IntegratedGpu): mesh_shader=true multiview=true points=true passthrough=true subgroup=true
timestamps=true indirect_first_instance=true multi_draw_count=false
| task: groups/dim 4294967295 total 4294967295 threads 1024 payload 16352 B
| mesh: groups/dim 1048576 total 1048576 threads 1024 vertices 256 primitives 256 layers 2048
```

**Hardware that has them** (the hardware classes behind the API conditions
above, from vendors' public documentation; only the Apple row was run):

- Apple silicon with Metal 3: the A14 and M1 generation (Apple7) and newer.
- NVIDIA from Turing (GeForce RTX 20 series) on, on Vulkan and DX12.
- AMD from RDNA 2 (Radeon RX 6000) on, on Vulkan (RADV and the vendor
  drivers) and DX12.
- Intel Arc and newer integrated Xe graphics, where the driver exposes
  `VK_EXT_mesh_shader` or D3D12 mesh shader tier 1.
- Not on: older desktop GPUs, most phones and tablets running Vulkan or GLES,
  WebGL2, and any browser's WebGPU today. A shipping game still has to run on
  all of those.

The engine's tiers (`src/render/capabilities.rs`: `WebGl2`, `Baseline`,
`Desktop`) are decided by limits, not by these features, and the engine's own
device asks for neither (`src/render/gpu.rs` requests only timestamp queries
and adapter-specific texture format features; a test,
`the_default_renderer_asks_for_no_mesh_shader_feature`, scans every source
file under `src/render` and `src/three` and keeps it so).

## The prototype

`src/render/mesh_shader_proto.rs` (a test-only module; no renderer code
changed), driven by `mesh_shader_render_test.rs` (tests) and
`mesh_shader_bench.rs` (the benchmark). It draws offscreen, 1280×720
`Rgba8Unorm` plus `Depth32Float`, no multisampling, no window.

The workload: a field of spheres (`MeshData::sphere`, 3 distance levels of
48, 24 and 12 segments: 2,208, 528 and 120 triangles) seen by a camera that
flies 0.1 m a frame, in the scenes below. Both ways of drawing it read the
same instance data (position and uniform scale), shade the same way (a
hashed colour per instance and a Lambert term) and use the same raster state
(back faces culled, depth `Less`).

| Path | What it does |
|---|---|
| indirect plain | one instanced indexed draw of the nearest level, every instance, no culling |
| mesh plain | mesh shaders only: one workgroup per meshlet of every instance, the nearest level, no culling |
| **indirect (cull + levels)** | the engine's structure: a compute pass tests every instance's bounding sphere against the six frustum planes and picks its level, appending it to the level's visible list with an atomic; one `draw_indexed_indirect` per level |
| indirect mesh | the same culling pass, then one `draw_mesh_tasks_indirect` per level: a mesh workgroup per meshlet of every kept instance, which culls its own meshlet by sphere and normal cone before it emits |
| mesh (task cull + levels) | a task shader workgroup culls up to 26 instances (a lane each), picks their levels and walks their meshlets, culling each by sphere, and hands the survivors to mesh workgroups through the payload; no compute pass, no visible list |
| mesh + cones | the same, and a meshlet facing away from the camera is dropped |

Meshlets are at most 64 vertices and 124 triangles (the usual NVIDIA
guidance, which every adapter's limits cover), built by a greedy walk over
the triangles in index order (`build_meshlets`); a test checks they hold every
triangle exactly once and rebuild the mesh. Mesh workgroups have 64 threads;
task workgroups 32.

The greedy walk fills meshlets **about half full**: the 64-vertex limit is
reached long before the 124-triangle one on a sphere's strip order. The
meshlet test prints the averages (`meshlets_hold_every_triangle_once_within_the_pipeline_limits`,
and the benchmark prints them per scene and level):

| Sphere | Triangles | Meshlets | Triangles per meshlet (of 124) |
|---|---|---|---|
| 96 segments (dense, nearest) | 9,024 | 150 | 60.2 |
| 48 segments (field and light nearest; dense middle) | 2,208 | 38 | 58.1 |
| 24 segments | 528 | 8 | 66.0 |
| 12 segments (field and light farthest) | 120 | 2 | 60.0 |

So every scene's meshlets average 58 to 66 triangles, 47% to 53% of the limit.

**Correctness first** (`the_mesh_shader_path_draws_the_picture_the_indexed_path_draws`):
in the small test scene (24×24 spheres, rendered at **192×108**, at **two
camera positions**, frames 0 and 40, on **one adapter**, the M4 Max), every
mesh path's image is compared with the indexed path's under a tolerance (24
levels a channel, 0.4% of pixels beyond it, edges one pixel apart allowed),
which is what the test asserts. On this adapter it **printed** zero differing
pixels for all four mesh paths; that figure is the printed diff, not an
assertion, and other adapters may differ within the tolerance. The
comparison's negative control, `the_comparison_can_tell_paths_apart`, shows
two different views fail it (it printed 29% of pixels differing; the test
asserts only that the tolerance is exceeded). The GPU's culling matches a CPU
replica instance for instance (`the_cpu_replica_of_the_culling_counts_what_the_gpu_culled`).
**The benchmark's scenes have no equivalence check**: their images are not
compared, only their triangle counts reported.
Meshlet culling really drops work: in the small test scene the plain mesh
path sends 304,128 triangles through the mesh stage, culling and levels
41,712 and cones 41,162 (`the_cone_and_meshlet_culling_drops_triangles_the_plain_path_draws`).
The unchecked shader build draws what the checked one draws (both mesh
pipelines on one device, compared exactly), and the number of instances per
task workgroup (1, 5, 32) doesn't change the picture (against the indexed
path within the tolerance, and between the mesh images exactly).

### How it was measured

Command (headless, offscreen; needs a GPU with mesh shaders and skips
otherwise):

```sh
cargo test --release --lib --features 3d mesh_shader_benchmark -- --ignored --nocapture
```

The branch's head when this note was committed (the benchmark code of all
three runs), `rustc 1.99.1`, release profile, Apple M4 Max (14 cores), macOS 27.2.1 (Metal; wgpu reports no driver
version). Per row: 3 warm-up batches and 12 measured batches of 4 frames
drawn back to back in one command buffer; the CPU figure is encoding plus
submitting, per frame; 5 rounds per row, each round's median kept. The rounds
of one path ran back to back, then the next path's: paths were **not
interleaved**, so a change in the machine's load between rows lands on one
row and not the other (one reason the measurement ran three times).

**"GPU ms" is not a GPU timer.** It is the batch's wall time from submit to
done, divided by 4. That includes the submit and the wake-up when the GPU
finishes, a floor of about a millisecond per batch (about 0.25 ms a frame).
The indexed rows with culling and levels (0.5 to 0.8 ms a frame) sit close to
that floor, so **their ratios against the mesh rows are imprecise**: a fixed
floor in both would make the true GPU ratio larger, not smaller, but under
this load the floor itself was not constant. Read the "× indirect" column
for those rows as an order of magnitude.

**The machine was busy.** Other agents were compiling and running GPU work
at the same time: the load average was between 80 and 180 on 14 cores
throughout (97, 157 and 178 at the starts of the three runs). The
measurement ran three times. In the tables, a cell is the best round's median over the
three runs, with the range of the three runs' best medians beside it; the
spread between the rounds of one row was often 20% to 250%, so **no
difference smaller than the range between runs is claimed**. Where two rows'
ranges overlap, the note says so.

GPU timestamps were tried and dropped for the render pass: on Metal the
pass's start sample is the vertex stage's start and its end the fragment
stage's end, and for mesh pipelines (and for the second frame of an indexed
pass) the end sample read back older than the start. Only the culling
compute pass's timestamp is used (in the "cull" column; it's tiny).

### Results

GPU milliseconds per frame, best (range over three runs). The last column
is the best time divided by the best time of "indirect (cull + levels)" in the
same scene. Triangles are the ones the mesh stage emitted or the indirect
draws drew, in millions.

**field: 65,536 spheres, 3 levels** (48, 24 and 12 segments, seen out to
150 m)

| Path | Triangles | GPU ms/frame | × indirect |
|---|---|---|---|
| indirect plain | 144.7 | 34.3 (34.3 to 43.3) | 49 |
| mesh plain | 144.7 | 60.9 (60.9 to 78.0) | 87 |
| **indirect (cull + levels)** | 2.42 | **0.70 (0.70 to 1.04)** | 1.0 |
| indirect mesh (cull + levels) | 2.38 | 1.32 (1.32 to 1.55) | 1.9 |
| mesh (task cull + levels) | 2.41 | 2.37 (2.37 to 2.48) | 3.4 |
| mesh + cones | 2.38 | 2.38 (2.38 to 2.64) | 3.4 |
| mesh, unchecked shaders | 2.41 | 2.10 (2.10 to 2.54) | 3.0 |
| mesh, 1 instance per task workgroup | 2.41 | 5.09 (5.09 to 5.52) | 7.3 |

**field, nearest level only** (the same field with one 2,208-triangle level)

| Path | Triangles | GPU ms/frame | × indirect |
|---|---|---|---|
| indirect plain | 144.7 | 25.3 (25.3 to 30.6) | 37 |
| mesh plain | 144.7 | 60.3 (60.3 to 103.7) | 88 |
| **indirect (cull + levels)** | 2.08 | **0.69 (0.69 to 0.90)** | 1.0 |
| indirect mesh | 2.01 | 1.00 (1.00 to 1.24) | 1.5 |
| mesh (task) | 2.06 | 2.70 (2.70 to 2.81) | 3.9 |
| mesh + cones | 2.01 | 2.57 (2.57 to 2.70) | 3.7 |
| mesh, unchecked shaders | 2.06 | 2.52 (2.52 to 2.68) | 3.7 |
| mesh, 1 instance per task workgroup | 2.06 | 5.18 (5.18 to 5.93) | 7.5 |

**dense: 16,384 spheres, 3 levels of 96, 48 and 24 segments** (9,024-triangle
nearest level, 150 meshlets of 60 triangles on average, 6 instances per task
workgroup)

| Path | Triangles | GPU ms/frame | × indirect |
|---|---|---|---|
| indirect plain | 147.9 | 25.8 (25.8 to 30.1) | 34 |
| mesh plain | 147.9 | 61.3 (61.3 to 103.4) | 80 |
| **indirect (cull + levels)** | 3.19 | **0.76 (0.76 to 0.83)** | 1.0 |
| indirect mesh | 2.99 | 1.47 (1.47 to 2.81) | 1.9 |
| mesh (task) | 3.18 | 3.10 (3.10 to 3.80) | 4.1 |
| mesh + cones | 2.99 | 3.16 (3.16 to 3.47) | 4.1 |
| mesh, unchecked shaders | 3.18 | 3.39 (3.39 to 3.64) | 4.4 |
| mesh, 1 instance per task workgroup | 3.18 | 3.88 (3.88 to 4.20) | 5.1 |

**light: 4,096 spheres, 3 levels**

| Path | Triangles | GPU ms/frame | × indirect |
|---|---|---|---|
| indirect plain | 9.04 | 1.22 (1.22 to 1.34) | 2.4 |
| mesh plain | 9.04 | 3.85 (3.85 to 4.24) | 7.4 |
| **indirect (cull + levels)** | 1.58 | **0.52 (0.52 to 0.60)** | 1.0 |
| indirect mesh | 1.54 | 0.84 (0.84 to 1.09) | 1.6 |
| mesh (task) | 1.57 | 1.64 (1.64 to 1.99) | 3.2 |
| mesh + cones | 1.54 | 1.50 (1.50 to 1.94) | 2.9 |
| mesh, unchecked shaders | 1.57 | 1.71 (1.71 to 2.00) | 3.3 |
| mesh, 1 instance per task workgroup | 1.57 | 1.83 (1.83 to 2.04) | 3.5 |

What the tables support, and what they don't:

- **Raw geometry throughput: the indexed path wins by 1.8 to 3.2 times**
  ("mesh plain" against "indirect plain" on the best runs: 4.2 to 5.7
  billion triangles a second in the three scenes of about 145 million triangles and 7.4
  in the light one, against 2.4 for the mesh path in every scene; across all
  runs 3.3 to 7.4 against 1.4 to 2.4; the ranges of the two never touch in
  any scene). The mesh pipeline is not hiding a faster rasteriser behind a
  slower front end on this GPU.
- **With the same culling and levels, the indirect path wins by 1.5 to 1.9
  times over the mesh pipeline that reuses its culling pass**, and by about
  3.2 to 4.1 times over the task-shader design (ranges separate in every
  scene).
  The culling pass itself is 0.006 to 0.019 ms of the indirect path's 0.5 to
  1.0 ms (its own timestamp; noisy as the machine was). These are the rows
  near the submit floor (above), so the factors are approximate.
  Note that "indirect mesh" culls each meshlet by its normal cone too, while
  "mesh (task)" does not: compare it with "mesh + cones" for the same culling
  (it is 1.8 to 2.6 times faster than that row, ranges separate).
- **Cones and unchecked shaders: no difference is claimed.** The ranges of
  "mesh + cones" and "mesh, unchecked shaders" overlap those of "mesh (task)"
  (or differ by under 8%, against round spreads of 20% to 250%) in every
  scene, although cones drop 1 to 6% of the triangles. (The mesh docs
  recommend the unchecked build to skip zeroing workgroup memory; at these
  output sizes it did not show.)
- **Instances per task workgroup matters when instances are many:** one
  instance per workgroup was 1.9 to 2.1 times slower than 26 in the
  65,536-instance scenes (5.1 against 2.4 ms and 5.2 against 2.7 ms; the
  ranges don't touch). In the dense and light
  scenes the two overlap and nothing is claimed.
- **CPU time doesn't differ.** Encoding and submitting a frame took 0.12 to
  0.21 ms per frame on every path (0.21 to 0.32 for the indirect plain draw
  in the nearest-level and dense scenes), inside the noise. The indirect and mesh paths both
  record a handful of commands. A mesh path wouldn't make the CPU cheaper,
  here.
- **The prototype is the lower bound on the engine's cost, not the engine's
  cost.** The engine's culling is four kernels (local ranks, prefixes, group
  prefixes, scatter) so the visible list keeps source order; the prototype
  appends with an atomic, and draws no shadows, no depth prepass, no
  multisampling, no materials and no skinning.

### Why this prototype may be slow

"Slower on every scene" is a statement about this prototype on this GPU, not
about mesh shaders. Likely contributors, none of them measured apart:

- **Half-full meshlets.** At 58 to 66 triangles against a 124 limit, every
  mesh workgroup does about half the work it could, so the mesh stage runs
  about twice as many workgroups (and pays their launch and output setup
  twice) as a locality-aware builder that fills them would.
- **Poor task-lane use.** A task workgroup has 32 lanes but culls at most 26
  instances (one lane each) in the field and light scenes and only 6 in the
  dense one, because the payload holds 1,024 meshlet entries and the nearest
  level has 38 or 150 meshlets. It then walks its kept instances one after
  another, testing up to 32 of an instance's meshlets at a time; the far
  levels have only 2 to 8 meshlets an instance, so on them 24 to 30 of the
  32 lanes idle in each step, and the workgroup hands on few mesh workgroups.
- **Not tuned for Apple's pipeline** (see "Limits of this study"): workgroup
  sizes, meshlet sizes and payload layout come from NVIDIA and AMD guidance.

The "plain" rows, which have neither culling nor task shaders, are still 1.8
to 3.2 times slower, so the task stage alone doesn't explain the gap.

### The engine's own path, for context

The same scenes through the real renderer (`engine_anchor` in the
benchmark: `BatchRig`, one batch of 65,536 or 4,096 instances, the same
three levels, compute culling and indirect draws, shadows and the atmosphere
off, **a full frame: shading, a depth prepass and 4× multisampling**). It is
not a like-for-like row; it shows the prototype's numbers aren't an
artefact of the prototype being far lighter than a game. Two runs (the
anchor was added after the first of the three); the GPU figure is the sum of
the engine's own per-pass timestamps, the frame runs from before `prepare`
until the GPU is done and so includes the CPU steps (4.7 to 14.7 ms, inflated
by the load):

| Scene | Engine GPU passes (ms) | Engine frame (ms) | Prototype indirect (ms) |
|---|---|---|---|
| field, 3 levels | 4.37, 4.24 | 16.4, 9.7 | 0.70 |
| field, nearest level | 3.48, 3.52 | 19.6, 16.3 | 0.69 |
| dense | 5.39, 5.45 | 20.2, 12.5 | 0.76 |
| light | 4.01, 4.14 | 18.7, 11.2 | 0.52 |

The engine's passes cost 5 to 8 times the prototype's: the extra is shading,
the depth prepass (the geometry twice) and 4× multisampling, none of which a
mesh shader changes. Mesh shaders would have to win on the geometry part of
that, and the prototype says they lose there on this GPU. (The engine's
timer reported a culling pass of 0.13 to 0.19 ms in three scenes and a
meaningless -0.000 in the fourth; the figure isn't used.)

### Memory

Buffers, from the header the benchmark prints (the same spheres in each
layout; instances are shared by both):

| Scene | Vertices | Instances | Indexed path extra | Mesh path extra |
|---|---|---|---|---|
| field (65,536) | 0.04 MiB | 1.00 MiB | 0.78 MiB | 0.02 MiB |
| dense (16,384) | 0.14 MiB | 0.25 MiB | 0.32 MiB | 0.10 MiB |
| light (4,096) | 0.04 MiB | 0.06 MiB | 0.08 MiB | 0.02 MiB |

"Indexed extra" is the index buffers (`u32`, 12 B a triangle) plus the three
levels' visible lists (12 B an instance: `3 × u32`) and the draw arguments;
"mesh extra" is the meshlet table (48 B a meshlet), each meshlet's vertex list
(4 B per meshlet vertex) and its triangles packed in one `u32` each (4 B a
triangle). So the mesh layout is smaller where there are many instances (no
visible lists) and a little smaller for the geometry itself against `u32`
indices (0.10 against about 0.13 MiB for the dense scene's meshes), but it
would be larger than 16-bit indices (6 B a triangle) for small meshes. The
gap is under 1 MiB on these scenes: memory is not a reason either way.
Process and GPU memory weren't measured (wgpu has no per-device figure).

## Maintenance, portability and tooling cost

What a second geometry path next to the existing ones would cost, from what
the code has and what the prototype needed.

**A third tier of paths.** The renderer already chooses between tiers by
limits (`GpuTier`: `WebGl2`, `Baseline`, `Desktop`; `capabilities.rs`) and
has two instance-batch paths (CPU culling with direct draws, compute culling
with indirect draws, with a crossover constant measured on one GPU:
`STATIC_BATCHES.md`). A mesh path is a third, gated by an experimental
feature the other tiers can't have, so every geometry change (a new vertex
attribute, skinning, morph targets, impostors, a new pass) would be made
for three paths and tested on hardware that has the third. It would be the
only path that needs meshlets built offline or at load (the prototype's
greedy builder is 70 lines for a mesh with good triangle order; a general
mesh needs a locality-aware builder such as meshoptimizer's, a new
dependency or a new importer step), and every instance batch's levels would
need meshlets, too.

**The pass and pipeline matrix.** `PipelineRole` has ten roles (prepass,
cut-out prepass, coverage prepass, opaque, coverage opaque, solid, transparent,
shadow, cut-out shadow, velocity), each multiplied by `ShaderVariant` (sides
and instancing) and the module key, and cached in `pipeline_cache.rs`. A mesh
path needs the same matrix again with mesh and task stages, so the cache key
gains a geometry-path dimension and a game's own material shaders would have
to be composed twice (below). Building the prototype's three mesh pipelines
plus module took 2.5 to 4.4 ms against 1.2 to 2.7 ms for the culling and
indexed pair (this machine, shader caches warm, one run each; no claim beyond
"the same order per pipeline").

**Shader tooling.** Material shaders are the game's WGSL composed with the
engine's prelude and entry points (`compose_shader`, `compose_effect`,
`compose_compute` in `gpu3d/shaders.rs`); `shader_diagnostics.rs` maps naga's
errors back to a line of the game's code, and hot reload keeps the last good
pipelines on a failure (`shader_reload_render_test.rs`). A mesh path means a
fourth composition (`ShaderKind` has none): `enable wgpu_mesh_shader;` must be
the first directive of the module and no browser compiler accepts it; the
output struct, payload and `@mesh(...)` binding rules are checked by naga's
validator and diagnosed with its entry-point errors, not the surface errors
the game author sees today; a payload larger than the limit or one that
disagrees between the task and mesh stage fails at pipeline creation, in
wgpu-core, not in naga, so the diagnostics layer would need a second source.
Hot reload would rebuild three kinds of pipeline per material. The prototype
needed the unchecked build (an `unsafe` call) to even test wgpu's advice
about workgroup memory zeroing.

**The safety opt-in.** Using the feature means passing
`ExperimentalFeatures::enabled()`, an `unsafe` constructor, in the one device
the whole engine shares. That puts every renderer path, not just the mesh
one, under the "there may be undefined behaviour from bugs in these APIs"
contract, and the repository's rule is that every `unsafe` needs a test. The prototype has two
`unsafe` blocks (the token and the unchecked shader module), exercised by
every test and the benchmark.

**Testing on hosted runners.** (As the list of checks, now `scripts/checks.yml`, was run then.) Linux runners have no GPU and use
lavapipe (software Vulkan, from Mesa), which wgpu documents as failing to run
naga's mesh shader output, though a newer Mesa may advertise the extension;
macOS runners are virtual Apple devices, and wgpu turns the feature off for
any device whose name contains "virtual"; the Windows lane uses WARP through
DX12 (with `PONKO_REQUIRE_GPU=1`), whose mesh shader tier was not checked
here. **These tests never ran on WARP or lavapipe.** So the prototype's rig
skips, with a logged reason, on any software adapter (`DeviceType::Cpu`) and
whenever `PONKO_GPU_TIER` forces a tier below `desktop` (those tiers' limits
allow no task or mesh workgroups), besides any adapter without the feature
or with smaller limits. These are capability skips, which
`PONKO_REQUIRE_GPU` leaves as skips (only a missing adapter or device fails
under it); this was checked with `PONKO_GPU_TIER=webgl2 PONKO_REQUIRE_GPU=1`
on this machine. The probe itself only reads what the adapter reports and
runs everywhere. The mesh path would be tested only by a maintainer's own
machine or a self-hosted runner with a GPU. The WebGL2 and baseline-tier
tests would be unaffected, and they should not be: they force a device with
fewer limits, they can't enable this feature, and no path may depend on it.

**Fallbacks stay.** Nothing here changes them: no renderer file is touched
(only a new test module, registered in `render/mod.rs`, and this note), the
default device requests no mesh feature, and the WebGL2 path, the baseline
tier and the CPU batch path are as they were.

## What would change it

Revisit when any of these holds, then run the benchmark again (the command
above) on the new hardware and read the same tables:

1. **wgpu drops the `EXPERIMENTAL_` prefix and the `unsafe` token**, so that
   opting in is not a safety statement about the whole renderer.
2. **A second vendor's hardware is measured and mesh shaders win there.**
   This study covers one Apple GPU on one backend. The prototype's design
   (64-vertex meshlets, 64-thread workgroups) comes from NVIDIA and AMD
   guidance, where a mesh pipeline is the first-class geometry front end; run
   the benchmark on a Vulkan NVIDIA or AMD GPU and on DX12 before deciding
   anything about desktop PCs. If a hardware class shows mesh shaders
   winning, that is the one case for an *optional* path.
3. **Content changes**: a game that draws a hundred million or more source
   triangles a frame in static detail (clustered level of detail à la virtual
   geometry, which needs meshlets and per-meshlet culling to work at all).
   The engine's instance batches, three levels and cell culling are what the
   current games need; the benchmark's heaviest views drew 2 to 3 million
   triangles in about 0.7 ms.
4. **WebGPU gains mesh shaders** in the specification and the browsers, so
   one WGSL source could serve the web build too. Today the web build
   can't use them.
5. **Hosted runners can run them** (a GPU runner, or a software driver that
   passes), so a mesh path can be tested on hosted machines.

The gap between the engine's frame and the prototype's (5 to 8 times) is
mostly work a mesh shader doesn't change (shading, the depth prepass,
multisampling). If frame time ever needs cutting, that is where to look first;
this study did not profile those passes.

## Limits of this study

- **One GPU, one backend**: an Apple M4 Max on Metal. Vulkan (NVIDIA, AMD,
  Intel) and DX12 results are not measured; their rows in the matrix are
  read from the sources. Browsers and GL were not run (no mesh path exists
  there).
- **The machine was heavily loaded** (load average 80 to 180 on 14 cores
  while measuring): the rows' own spreads were 20% to 250%, and only
  differences much larger than that are claimed.
- **Not tuned for Apple's mesh pipeline**: one meshlet size (64 vertices,
  124 triangles, filled about half), one workgroup size (64), meshlets from a greedy walk, and
  no use of subgroup operations or Apple's own guidance. A tuned
  implementation could be faster than the prototype; this study doesn't show
  by how much, and the plain rows (1.8 to 3.2 times slower with no culling at
  all) suggest the front end, not the culling, is where the gap is.
- **Spheres only**: smooth, regular, locality-friendly index order. Meshes
  with long thin triangles, hard edges or poor locality would fill meshlets
  differently.
- **Simplified like-for-like**: translation and uniform scale per instance
  (the engine has full matrices), no materials, shadows, depth prepass,
  multisampling or skinning in the prototype; no overdraw or fill-rate
  analysis; no GPU memory figure; no allocation counts.
- **GPU time is the wall time of batches of four frames**, not a pass
  timestamp (the timestamps were unusable for the render pass on this
  backend); the sub-millisecond indexed rows sit near the submit and wake
  floor, so their ratios are imprecise. Paths ran one after another, not
  interleaved.
- **Equivalence was checked on the small test scene only** (192×108, two
  frames, one adapter, with a tolerance); the benchmark scenes' images were
  not compared.
- **Shader and pipeline build times** are single cold-ish runs.
- **Native bindings**: nothing in this study touches the C ABI or C++; a
  mesh-shader path would need none until it's a public setting.

## Reproducing

```sh
# Probe (prints this adapter's row of the matrix) and the tests:
cargo test -p ponko --lib --features 3d mesh_shader -- --nocapture
# Benchmark (release; delete target/release afterwards):
cargo test --release --lib --features 3d mesh_shader_benchmark -- --ignored --nocapture
```

Tests (`src/render/mesh_shader_render_test.rs`): the probe, its negative
control, meshlet construction, culling against a CPU replica, picture
equivalence of all mesh paths with the indexed path within a tolerance, with
a negative control, unchecked against checked shaders, instances-per-task
invariance, indirect dispatch limits, and that no file of the renderer
(`src/render/**`, `src/three/**`, except the prototype's own modules)
requests a mesh feature or opts in to experimental features.
