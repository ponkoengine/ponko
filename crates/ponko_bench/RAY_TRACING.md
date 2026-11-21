# Hardware ray tracing and path-traced lighting: evaluation (#165)

A decision note, not a renderer change. It maps what the engine's wgpu
version offers for ray tracing, measures what today's global illumination
(GI) and screen-space reflections (SSR) get wrong, compares an isolated
ray-query prototype against them, and says how a ray-traced path would sit
next to them. Nothing here changes the default renderer, and nothing here
promises production integration.

**Recommendation: defer.** The benefit is real and measured (§2, §3), but
the API is experimental behind an `unsafe` opt-in, it never reaches the web
or GPUs without ray tracing hardware, and the prototype leaves out the costs
that decide the question (shading at hit points, denoising, moving and
skinned geometry). §5 lists what would change this.

## Setup

- Apple M4 Max, Metal backend (integrated GPU, driver reported as unknown),
  macOS (Darwin 27.2), Rust 1.99.1, wgpu 30.0.1 (from `Cargo.lock`), branch
  `m/ray-tracing-eval-165` off `main` at `5a7bae0b`.
- **The machine was busy**: other agents were compiling the whole time (load
  average 104 to 124 on its CPU cores). GPU timings come from timestamp
  queries, which CPU load affects less than wall time, but some still
  switched between two clock states between runs; both values are given
  where that happened. No claim below rests on a difference smaller than the
  spread seen.
- Every measurement is an offscreen test; no window was opened.
- The ray-query tests have run only on this Metal GPU. They have never run
  on a software Vulkan adapter with ray queries (lavapipe), or on any
  Vulkan or DX12 GPU; elsewhere they skip without the feature.

| What | Where | Command |
|---|---|---|
| Adapter probe (§1) | `src/render/ray_tracing_probe_render_test.rs` | `cargo test --lib --features 3d ray_tracing_probe -- --nocapture` |
| SSR failures (§2) | `src/render/ssr_render_test.rs`, `screen_space_reflections_lose_*` | `cargo test --lib --features 3d screen_space_reflections_lose -- --nocapture` |
| GI leak (§2) | `src/render/gi_render_test.rs`, `bounced_light_stops_at_a_thin_partition_on_a_probe_plane` | `cargo test --lib --features 3d bounced_light_stops -- --nocapture` |
| Prototype (§3) | `src/render/ray_query_prototype_render_test.rs` | `cargo test --lib --features 3d ray_query_prototype -- --nocapture` |
| Prototype cost (§3) | same file, `ray_query_reflection_cost` (ignored) | `cargo test --release --lib --features 3d -- --ignored --nocapture --test-threads=1 ray_query_reflection_cost` |
| SSR cost baseline (§3) | `ssr_render_test.rs`, `reflection_filter_gpu_cost` (ignored, existing) | `cargo test --release --lib --features 3d -- --ignored --nocapture --test-threads=1 reflection_filter_gpu_cost` |

## 1. What wgpu 30.0.1 offers

Read from the crate sources (`wgpu-types/src/features.rs`, `limits.rs`,
`tokens.rs`; `wgpu-hal/src/{vulkan,metal,dx12,gles}/adapter.rs`;
`wgpu-core/src/instance.rs`, `device/ray_tracing.rs`,
`command/ray_tracing.rs`; `wgpu/src/backend/webgpu.rs`; `naga/src/back`).
The published crates carry no changelog, so the sources are the reference.

**API.** `Device::create_blas` (triangle or AABB geometry), `create_tlas`,
`TlasInstance` (3x4 transform, 24-bit custom data, 8-bit mask),
`CommandEncoder::build_acceleration_structures` (BLAS and TLAS builds in one
call; TLAS-only rebuilds for moved instances), BLAS compaction
(`ALLOW_COMPACTION`, `prepare_compaction_async`, `Queue::compact_blas`),
`BindingType::AccelerationStructure`, and in WGSL `enable wgpu_ray_query;`
with `ray_query`, `rayQueryInitialize`, `rayQueryProceed`,
`rayQueryGetCommittedIntersection` in compute and render shaders. Naga
writes ray queries to SPIR-V, MSL and HLSL; its GLSL writer refuses them. Ray tracing *pipelines*
(ray generation, hit and miss shaders) exist only on Vulkan.

| Feature | Vulkan | Metal | DX12 | GL / WebGL2 | WebGPU (browser) |
|---|---|---|---|---|---|
| `EXPERIMENTAL_RAY_QUERY` (acceleration structures + ray queries) | with `VK_KHR_acceleration_structure`, `VK_KHR_ray_query`, buffer device address, deferred host operations | macOS 15 / iOS 18 and later, where the device supports ray tracing *and* ray tracing from render stages | raytracing tier 1.1 and shader model 6.5 | no | no: `create_blas` is `unimplemented!` |
| `EXPERIMENTAL_RAY_TRACING_PIPELINES` | with `VK_KHR_ray_tracing_pipeline` | no | no ("will go here" in the source) | no | no |
| `EXPERIMENTAL_RAY_HIT_VERTEX_RETURN` | with `VK_KHR_ray_tracing_position_fetch` | no | no | no | no |
| `EXTENDED_ACCELERATION_STRUCTURE_VERTEX_FORMATS` (f16, snorm16, 2D positions) | with ray queries or pipelines | no | with ray queries | no | no |
| `ACCELERATION_STRUCTURE_BINDING_ARRAY` | with ray queries and update-after-bind | no | with ray queries | no | no |

**Constraints.**

- *Experimental opt-in.* Every `EXPERIMENTAL_*` feature is refused at
  device creation unless the descriptor carries
  `ExperimentalFeatures::enabled()`, an `unsafe` acknowledgement that the
  API "may result in undefined behavior" and is "expected to be subject to
  breaking changes". The probe's second test checks that the ray tracing
  flags are in that mask and in no WebGPU mask, and that the default
  descriptor (where the engine's device starts) asks for none of it. The
  refusal itself is read from `wgpu-core/src/instance.rs`, not tested: the
  shared test helpers treat a refused device as a missing GPU.
- *Limits.* `Limits::default()` allows no acceleration structures
  (`max_tlas_instance_count`, `max_blas_primitive_count`, ... are 0); a
  device must ask for `using_minimum_supported_acceleration_structure_values()`
  or the adapter's values. The guaranteed minimum is one acceleration
  structure per shader stage (Metal shares them with buffer slots).
- *Geometry.* Without the extended formats a BLAS takes `Float32x3`
  positions only, with a stride of at least 12 bytes and a multiple of 4,
  from buffers created with `BufferUsages::BLAS_INPUT`. The engine's mesh
  vertices (`gpu3d/resources.rs`) start with an `f32` position and are 44
  bytes apart, so its mesh arena could feed BLAS builds directly, but only
  once its buffers carry `BLAS_INPUT`: a renderer change.
- *Validation.* wgpu-core checks builds (formats, counts, strides, that
  every BLAS a TLAS uses is built before or with it, that instances are
  valid) and bind groups (the TLAS is built).
- *Memory reporting.* wgpu doesn't say how big an acceleration structure
  is. Its counters count acceleration-structure memory only on DX12 (the
  Metal counter is commented out). The prototype asks the backend directly
  (Metal only, see §3).

**Measured row** (the probe on this machine):

| Adapter | Backend | Ray query | RT pipelines | Vertex return | Extended formats | AS binding arrays | Limits |
|---|---|---|---|---|---|---|---|
| Apple M4 Max | Metal | **yes** | no | no | no | no | 268,435,456 BLAS primitives, 16,777,216 BLAS geometries, 16,777,216 TLAS instances, 29 acceleration structures per stage, recursion depth 0; BLAS formats `[Float32x3]` |

Other adapters weren't available here; the probe prints the same row on any
GPU and skips cleanly without one. On GL and in the browser it asserts that
nothing is reported.

## 2. What today's GI and SSR cannot see

### Screen-space reflections (`src/render/ssr.rs`, `ssr.wgsl`)

From the shader: each half-resolution pixel marches its reflected ray in 32
steps (then 6 halvings) across this frame's depth buffer and takes the
color at the hit from the *last* frame's lit image, reprojected. So:

1. **Off-screen geometry** is invisible: the march stops when the ray leaves
   the screen, and fades over the last tenth before every edge.
2. **Hidden geometry** is invisible: the depth buffer holds only the nearest
   surface, so whatever is behind something on screen can't be hit; a ray
   passing behind an occluder by more than the assumed thickness
   (`0.1 + 0.02 w`) goes on and finds nothing.
3. **Thin objects** are hit or missed by that thickness guess: a thin pole
   in front of a wall reflects as a solid slab, or not at all.
4. **Disocclusion and lag**: colors come from the previous frame, so what
   was just uncovered has no color yet, and the first frame (or one after a
   history reset) reflects nothing.
5. **Flat normals**: normals come from the depth, so normal maps don't bend
   the reflections; rough surfaces past 0.7 roughness fade to the
   environment.

Measured, on the existing mirror-floor scene (128x128, glowing box over a
mirror floor, red channel 0 to 255 at the floor point that mirrors the box,
settled over four frames):

| Scene | SSR red at the mirror point | Truth |
|---|---:|---|
| Box in view (positive control) | 255 | box |
| Camera turned down 48°: box above the top of the screen (failure 1) | **0** | box |
| Dark wall between camera and box; the floor's ray passes under it (failure 2) | **0** | box |

The tests assert the box's eight corners project above the screen in the
first case, and the prototype's `the_wall_hides_the_box_from_the_camera`
checks that no pixel shows the box in the second.

### Global illumination (`src/render/gi.rs`, `gi.wgsl`, `gi_update.wgsl`, `three/gi.rs`)

From the code: a grid of probes around the camera (default 32 x 16 x 32
units, one every 2 units: 17 x 9 x 17 = 2,601 probes) each draws six 16x16
raster views of its surroundings and keeps a 6x6 octahedral irradiance tile
and a 14x14 distance-moments tile. A surface blends its eight probes,
weighting each by a Chebyshev test on the distances that probe saw. So:

1. **Light leaks** through walls thinner than the probe spacing where the
   wall is close to a probe. Since then fixed for the case measured below:
   a probe buried in a thin wall (its neighbours' distances show the wall
   on both sides of it) is left out of the blend. A little light still
   gets through where a wall meets a surface that runs on past it into the
   next room (see the guide's "Coarse light").
2. **Small things are coarse**: a view texel covers about 5.6°, and
   irradiance is kept at 6x6 directions per probe: no light is told apart
   inside a cell, and the bounce is diffuse only (glossy bounce is left to
   SSR and reflection probes).
3. **What the views see**: opaque models near the grid only (four spacings
   past it), at coarse levels of detail, no instance batches, transparent
   surfaces or particles; point and spot lights cast no shadows in them.
4. **Lag**: probes take turns; 2,601 probes at the default 4 a frame is 650
   frames (about 11 seconds at 60 fps) for a full round, 163 frames at the
   maximum of 16.
5. **Range**: past the grid (16 units from the camera sideways) the light
   falls back to the sky and ground ambient light.

Measured, on the existing red wall room (probes every unit, at whole x;
red tint = red minus green on the floor 20 cm past a 2 cm white partition
that runs floor to roof, so the floor there can't see the sunlit red wall;
80 frames):

| Partition | Red tint | Truth |
|---|---:|---:|
| None (positive control, same floor point) | 56 | 56 |
| At x = -1.5, between probe planes | 0 | 0 |
| At x = -1.0, on a probe plane (failure 1) | **34** (now 1) | 0 |

So the leak depended on where a thin wall falls against the grid, not only
on its thickness: 60% of the open room's bounce came through the partition
when it lay on the probe plane. With buried probes left out it measures 1
(the open room now reads 78, as the probes' light is shown where it
settles). The test now asserts `<= 5` on the plane as between planes.
Failures 2 to 5 follow from the code and the guide's own numbers and
weren't measured separately.

The two SSR tests pin known limitations, not wanted behaviour: each says in
its doc comment which assertion to flip once the limitation is fixed. They
measure 0 against a limit of 40 (and 255 against 120).

### What a ray-traced technique would fix

- Ray-traced reflections (one reflection ray per pixel against the whole
  scene) fix SSR failures 1 to 3 and 5 outright, and 4 when hits are shaded
  in the same frame. §3 shows 1 and 2 fixed.
- Ray-traced probe updates (each probe traces rays against the scene
  instead of drawing six raster views, as in ray-traced irradiance-field
  methods) remove GI failure 3 (what the views see, including instance
  batches) and cheapen updates, which helps 4. Per-pixel ray-traced
  visibility, or short ambient-occlusion rays, would fix the leak class (1)
  and the coarseness (2) where it matters. Not prototyped here.
- Full path tracing (many bounces per pixel, denoised) would replace both,
  but needs far more rays than the two per pixel measured below, a
  denoiser, and, for practical cost, ray tracing pipelines, which wgpu 30
  has only on Vulkan. Out of reach for now.

## 3. Prototype: ray-query reflections

`src/render/ray_query_prototype_render_test.rs`, outside the renderer: its
own BLAS per mesh, one TLAS, a compute shader that shoots one ray per pixel
from the camera and, where it meets the floor (a perfect mirror), one along
the reflection; a pixel is red when a ray finds the glowing box. Shading is
a constant per object: what is compared is what the rays find, not
materials. It opens its own device with `EXPERIMENTAL_RAY_QUERY` and the
experimental opt-in (the only place in the repository that does), and skips
without the feature and under a forced GPU tier.

### Visible benefit

Same scenes, camera and floor point as the SSR tests (128x128, red 0 to
255; the box masked out of every ray is the negative control):

| Scene | SSR | Ray query | Ray query, box masked out |
|---|---:|---:|---:|
| Box above the screen | 0 | **255** | 0 |
| Box behind a wall | 0 | **255** | 0 |

### Cost and memory

A town-sized scene: a 200 m floor of 131,072 triangles, 1,024 box
buildings, 1,024 spheres of 4,096 triangles: 135,180 unique triangles in
three BLASes, 2,049 instances, 4,337,664 instanced triangles. Camera at
street level down a diagonal street. Release build, timestamp queries
around the trace pass; each figure is the median of 200 timed dispatches,
with no discarded warm-up (the scene was built and one image drawn just
before), and each was measured twice in a row; two separate runs. The shader
traces one camera ray per pixel, plus a reflection ray where that ray meets
the floor (the floor is the only mirror).

| Measurement | Run 1 | Run 2 |
|---|---:|---:|
| Trace, 960x540, camera + reflection rays | 0.842 / 0.837 ms | 0.828 / 0.843 ms |
| Trace, 960x540, camera rays only | 0.727 / 0.730 ms | 0.730 / 0.418 ms |
| Trace, 1920x1080, camera + reflection rays | 2.938 / 2.932 ms | 2.939 / 2.952 ms |
| Trace, 1920x1080, camera rays only | 1.409 / 2.569 ms | 2.566 / 2.597 ms |
| Build every BLAS and the TLAS (wall time, submit to done) | 9.85 ms | 5.97 ms |
| Rebuild the TLAS alone (wall time) | 1.45 ms | 1.86 ms |

The camera-only numbers, at 1920x1080 above all, jumped between two GPU
clock states (1.4 and 2.6 ms) while the reflection numbers held within 2%,
so the split between camera and reflection rays is much noisier than the
reflection timings themselves; only the latter are compared below. Wall times include a submit and a wait (an empty
submit took 0.03 to 0.05 ms) on a loaded CPU, so they are upper bounds.

Memory, as Metal reports it for these builds (`backend_sizes`, which asks
the backend because wgpu doesn't say): BLASes **12.87 MB** for 135,180
triangles (about 95 bytes a triangle), TLAS **0.44 MB** for 2,049 instances
(about 215 bytes an instance), build scratch 4.87 MB while building (wgpu
frees it after), plus wgpu's own TLAS instance buffer.

### Against the raster baseline

| | SSR (today) | Ray-query reflections (prototype) |
|---|---|---|
| GPU, 1920x1080 frame | trace 1.20 ms (1.197 / 1.205, two runs; its trace is half size, 960x540), filtered levels +0.19 ms | 0.84 ms for camera + reflection rays at 960x540; 2.93 ms at 1920x1080 |
| Scene in the measurement | one box on a floor | 4.3 M instanced triangles |
| Memory at 1920x1080 | about 26.6 MB of its own (history 16.6 MB, reflection levels 5.5 MB, filtered levels 1.4 MB, all RGBA16F; half-size hit and footprint images 3.1 MB), plus a full-size level shared with ambient occlusion | 13.3 MB of acceleration structures for this scene (grows with unique geometry, not resolution), plus an output image |
| Off-screen and hidden geometry | missing (measured) | present (measured) |
| Per frame for moving objects | nothing extra | a TLAS rebuild (1.4 to 1.9 ms wall for 2,049 instances); skinned and morphed meshes need BLAS refits, not measured |

This is **not** a like-for-like win, and the note claims none:

- The prototype doesn't shade what its rays hit. A production pass would
  evaluate the hit's material and its lights (with shadow rays), and rough
  surfaces would need several rays and a denoiser. That is most of the cost
  of ray-traced reflections in practice, and it is unmeasured.
- Its camera rays stand in for what the depth prepass already provides; a
  production pass would trace only the reflection rays.
- The two scenes differ (SSR's cost barely depends on the scene; ray
  traversal does), and the prototype's numbers come from one GPU.

What it does show: on this GPU, tracing one camera ray per half-size pixel,
plus a reflection ray where the floor is seen, against a few million
instanced triangles costs about as much as
today's SSR trace, the structures for a mid-sized scene take about as much
memory as SSR's screen targets, and the visible result fixes exactly the
failures measured in §2.

## 4. Fallback: how it would sit next to GI and SSR

Nothing changes for GPUs without ray queries; the ray-traced path is an
optional layer above today's, never a requirement.

- **Opt-in and device.** The engine's device keeps asking for no
  experimental feature by default. A game (or a quality preset) would opt
  in; only then, and only where the adapter reports
  `EXPERIMENTAL_RAY_QUERY`, would the engine request it, with the
  acceleration-structure limits and the experimental acknowledgement.
- **Capabilities.** One more `GpuFeature` entry (for example ray-traced
  reflections) in `src/render/capabilities.rs`, with the fallback named
  ("screen-space reflections") and the reason ("the adapter reports no ray
  queries" or "not enabled"). The `GpuTier`s (`webgl2`, `baseline`,
  `desktop`) stay floors and don't gain a ray tracing tier: the feature is
  reported per adapter above `desktop`, and `PONKO_GPU_TIER` keeps it off.
  WebGL2 and browser WebGPU never report it.
- **Reflections.** SSR stays and stays first: trace the screen, and send
  only the rays that leave the screen or find nothing (alpha 0 in its trace
  image) to a ray-query pass that writes into the same half-size level. The
  opaque pass, the depth-aware filter and the environment fallback read
  that image as they do now, so material shaders don't change. Without ray
  queries the pass isn't built and the frame is today's.
- **GI.** The probe grid, its atlas and its lookup in the material shaders
  stay. Ray queries would only replace how a probe looks around (rays
  against the TLAS instead of six raster views), filling the same tiles;
  without them, the raster views remain.
- **Scene data.** One BLAS per mesh asset, built when the mesh is uploaded
  (the mesh arena's buffers would need `BLAS_INPUT`); one TLAS per frame
  from the frame's instance list, rebuilt only when instances move. Skinned
  meshes and instance batches come later (refits from the skinning pass's
  output; batches as instances).
- **Where it plugs in.** As an extension of the renderer through the
  custom-pass work, not a second renderer; no public API until the backend
  feature is stable. No C or C++ API is proposed.

## 5. Decision

**Defer**: keep the probe and the prototype as tests, add nothing to the
renderer.

Why not adopt now:

1. **Experimental API.** Ray queries need wgpu's `unsafe` experimental
   opt-in ("UB-containing bugs" possible, breaking changes expected). The
   engine would take on that risk in a public release.
2. **Coverage.** No web at all (WebGL2, browser WebGPU), none on GPUs
   without ray tracing hardware, Metal only from macOS 15 / iOS 18, DX12
   only at raytracing tier 1.1. Every game keeps needing SSR and probe GI,
   so a ray-traced path is a second path to keep correct, not a
   replacement.
3. **Unmeasured costs that decide it.** Hit shading, rough-surface
   sampling and denoising, BLAS refits for skinned meshes, and per-frame
   TLAS rebuilds at real instance counts (1.4 to 1.9 ms wall here for
   2,049).

Why not reject: the benefit is measured. Both SSR blind spots are real in
the engine today (the GI leak measured here has since been fixed in the
raster probes), ray queries fix the reflection
ones on this machine, and the cost of tracing alone is in the same range as
SSR's trace.

**What would change the decision:**

- *Toward adopt:* wgpu stabilises ray queries (no experimental token), or
  WebGPU gains them; a game targets ray-query-capable hardware and shows
  SSR or GI artifacts that matter in a measured scene (large glossy floors
  or water reflecting off-screen geometry, interiors with thin walls);
  and the renderer has the extension points to host the pass outside its
  core. Then the next step is the hybrid reflection fallback of §4, opt-in,
  measured with hit shading on at least one Vulkan and one Metal GPU.
- *Toward reject:* wgpu drops or stalls ray queries on Metal or DX12, or a
  shaded prototype costs much more than SSR at equal quality on the target
  hardware.
- *Path tracing* stays out of scope until ray tracing pipelines reach more
  than Vulkan and a denoiser is in place.

The C and C++ API has nothing to add: no engine API is proposed.
