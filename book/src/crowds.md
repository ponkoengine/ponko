# Crowds: thousands of animated characters

A stadium, a market square, an army on the march: thousands of characters
that share a skeleton, a few meshes and a few clips. As entities, each would
be posed on the CPU, skinned on its own and drawn on its own: 10,000 of them
take about 20,000 draw calls and most of a frame before anything else is drawn. A
**crowd** draws them all in a handful of draws.

A crowd is the skinned sibling of an [instance batch](3d.md#gpu-driven-instances):
one record per character, culled per view on the GPU, drawn by indirect
draws in the normal depth, color, shadow and motion passes. Its clips are
baked once, every vertex is skinned in the vertex shader from the bake, and
each character's clips play on by themselves between updates: a crowd that
only stands and cheers costs no upload at all.

Use a crowd for many characters that act alike and aren't touched one by
one. Keep a character an entity (a [skinned model](3d.md#skinned-meshes))
when it needs what a crowd doesn't do: an animation state machine, IK or
look-at, things attached to its joints, morph targets, root motion,
physics, or a material of its own.

The `crowd` example (`cargo run --example crowd --features 3d`) is a plaza
of 10,000 people, every two seconds another section cheering. Its pieces
follow.

## Bake the clips

A `CrowdAnimation` samples each clip at a fixed rate, every joint's matrix
at every frame, from 0 to the clip's end (a one-second clip at 30 frames a
second is 31 frames). Characters show the clip between its two nearest
frames, so 30 frames a second is plenty for most clips; a lower rate takes
less memory. A clip either loops (`CrowdClip::looping`, and a plain
`AnimationClip` converts to one) or holds its last frame (`CrowdClip::once`).
Clip `k` of the list is index `k`; `animation.clip("cheer")` finds one by
name.

```rust
{{#include ../../examples/crowd.rs:crowd_bake}}
```

Bake once and share the bake: crowds made from it keep what they need. On
the GPU a crowd's bake takes 48 bytes per joint of each level's mesh, per
baked frame: for 4 clips of 1 s at 30 frames, a 60-joint mesh is 350 KiB.
A crowd holds at most 2,097,152 rows (32 MiB, 16 bytes a row, three rows a
joint and frame, per level); `create_crowd` says so if a bake needs more.

## Levels of detail

Each `CrowdLevel` draws its mesh for characters whose origin is in a band
of distances from the view's eye (the camera, also for the sun's cascades;
the light, for a point or spot light's shadows; a target camera):
`min_distance` inclusive, `max_distance` exclusive, as for
[instance batch levels](3d.md#distance-levels-and-colours). A crowd takes up
to 8 levels. A far level is a plainer mesh, and it can move each corner by
fewer joints (`with_influences(1)` to `4`: the strongest ones, their
weights scaled back to a whole). `with_camera(false)` makes a level that
only casts shadows, `with_shadows(false)` one that casts none.

Each level's mesh is a skinned mesh made for the bake's skeleton
(`create_skinned_mesh`, or a skinned glTF mesh). A mesh made for another
skeleton follows the bake's joints by name; a joint the bake hasn't stays
at rest. The C++ beside the example's setup does it all in one function:
the bake, two levels, 10,000 characters and a section that starts cheering.

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/crowd.rs:crowd_create}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:crowd}}
```

</div>
</div>

## Characters and updates

A `CrowdInstance` is where a character stands (any affine transform), its
tint, the clip it plays and how far in, a second clip it blends to and how
much, and how fast both play on (`with_speed`: 1 for the clips' own pace, 0
to hold the pose, negative to play backwards). The times count from the
crowd's last update: the clips run on the GPU from there, at the speed,
and loop or hold at their ends.

`update_crowd` replaces the whole list. Call it when something changes: a
section starts cheering, a character walks on, a crowd thins out. A walking
crowd can be updated every frame; an update uploads 96 bytes a character.

```rust
{{#include ../../examples/crowd.rs:crowd_update}}
```

A crowd is a handle: `ctx.free(crowd)` frees it, `crowd_len` counts its
characters. While one of its level meshes is freed, the whole crowd draws
nothing: free a crowd before its meshes. `try_create_crowd` and
`try_update_crowd` return errors instead of panicking; on an error the
crowd keeps its characters.

## Shadows and motion

A crowd casts sun, point-light and spot-light shadows from each level whose
`shadows` is on, unless `CrowdOptions::with_cast_shadows(false)`. The
characters move, so a shadow map that a crowd reaches is drawn anew every
frame (the [shadow cache](3d.md#light-and-shadows) keeps only maps no crowd
reaches).

Under [temporal antialiasing](3d.md#temporal-antialiasing) a crowd writes
motion vectors: each vertex is skinned twice, at the frame's time and the
last frame's, so the animation itself moves the history, and an update that
moves a character runs its motion from its last record (by its index in the
list). A character an update appends starts from its new record, still.

## How it is drawn: two paths

Every frame, on devices with compute culling and storage buffers
(`GpuFeature::ComputeCulling`: most native GPUs, and WebGPU in the browser):

- A compute pass tests each character's bounding sphere (around every level
  in every baked pose) against every view of the frame (the camera, the sun's
  cascades, the shadow views of point and spot lights, target cameras), and
  its distance against each level's band, and appends it to that view and
  level's list.
- Each (view, level, mesh part) is one indirect draw over its list. The
  vertex shader reads the character's record, finds its clips' two nearest
  frames, blends the joints' matrices between them and with the second clip,
  and skins the corner.
- A crowd's cull pass costs about 10 µs of GPU time for 10,000 characters
  (40 µs with two levels and sun shadows) on the machine measured below.

**WebGL2** has no compute shaders, no storage buffers in vertex shaders,
every one of the 16 texture slots of a vertex shader already taken by the
mesh shaders' material and lighting textures (so the bake can't go in a
texture either), and no base vertex or base instance in its draws. There a
crowd takes the **CPU path**:

- When the crowd is made, each level's mesh is posed on the CPU at every
  baked frame, kept in one vertex buffer per level (at most 64 MiB for all
  levels; past that every second frame is kept, then every third, and so on).
- Each frame the characters are culled on the CPU, grouped by (view, level,
  frame), and each group is one draw of its frame's corners. A character
  shows the baked frame nearest its time, without blending between frames
  or clips (a blend of a half or more shows the second clip).
- Crowds write no motion vectors there (WebGL2 has no temporal antialiasing).

A device takes the GPU path where `ctx.gfx.gpu_capabilities()` says
`compute_culling` is available (see
[What this GPU can do](3d.md#what-this-gpu-can-do)). Both draw the same
picture at the baked frames: a test draws a character of each path over the
same mesh drawn as a skinned model in the same pose.

## Cost

A scene without crowds pays nothing: no buffers, passes, pipelines, uniforms
or uploads are made until the first crowd is (a test counts them, and the
CPU allocations of a crowd's steady frame: none).

`crowd_benchmark` in `src/render/crowd_bench.rs` measures 10,000 animated
characters of 1,296 triangles (two joints, two looping clips baked at 30
frames a second) on a ground, 1.6 m apart, seen from 30 m up, against the
same characters as 10,000 skinned models posed every frame by the same
clips:

```sh
cargo test --release --lib --features 3d crowd_benchmark -- --ignored --nocapture
```

Measured on an Apple M4 Max (Metal), 1280x720 with 4x MSAA and a depth
prepass, three runs each, the median of the runs' medians (and their
range). **The machine was busy** (a load average of 90 to 140 from other
builds) **while these were taken: they are to be repeated on a quiet
machine.** "Prepare + record, 1 thread" is the CPU work of the frame
measured on one thread, apart from waits for worker threads that a busy
machine adds to the parallel recording (those also take about 11 ms a
frame of an empty scene here).

| Scene | GPU ms | Prepare + record ms, 1 thread | Draws | Posing ms (CPU) |
|---|---|---|---|---|
| Ground only | 0.79 (0.79–0.79) | 0.72 (0.44–0.73) | 2 | – |
| Crowd | 6.68 (6.27–7.26) | 1.07 (1.02–1.08) | 4 | – |
| Crowd, sun shadows | 7.40 (7.39–7.40) | 1.29 (0.92–1.33) | 12 | – |
| Crowd, two levels (36 triangles past 40 m), sun shadows | 4.07 (4.07–4.08) | 1.44 (1.41–1.53) | 18 | – |
| Crowd, CPU path (WebGL2's limits) | 4.76 (4.70–5.95) | 1.58 (1.17–1.86) | 126 | – |
| Skinned models | 27.57 (27.42–27.58) | 48.74 (34.10–59.23) | 19,246 | 1.41 |
| Skinned models, sun shadows | 36.20 (28.66–37.55) | 47.72 (44.68–53.80) | 20,310 | 1.41 |

So on this machine 10,000 animated characters as a crowd **meet 60 fps by
their measured cost**: 4 to 7.4 ms of GPU time and 1.1 to 1.6 ms of CPU
work, together under 9 ms of the 16.7 ms a frame has. As skinned models the
same characters don't (28 to 36 ms of GPU time and about 48 ms of CPU work
a frame). The frame times this benchmark also prints (17 to 21 ms for the
crowds) wait for the GPU after the CPU work and for workers the busy
machine kept from running, so they are not the frame rate a game would
reach; a quiet run gives a cleaner number.

The CPU path's GPU time is lower than the GPU path's here because its
corners are posed in advance: it trades blending between frames and clips
for that, and it draws once per frame shown.

## Not yet

- Crowds are not drawn into global illumination's probes or into
  additional window views.
- No occlusion culling for crowds (instance batches have it), and no
  crossfades between levels.
- Stencil states of a material are ignored on the GPU path. Transparent
  materials are drawn unsorted; give crowds opaque or cutout materials.
- The order characters are drawn in within a level isn't fixed.
- In C, clips with joint tracks come from glTF files (`ponko_ctx_load_clip`)
  or retargeting (`ponko_retarget_clip`); they can't be built in code yet.

## C and C++

`ponko_ctx_bake_crowd_animation` bakes clips (`PonkoCrowdClip`: a clip and
whether it loops) on the skeleton of an entity, `ponko_crowd_animation_clip`
finds one by name and `ponko_crowd_animation_free` frees the bake.
`ponko_ctx_create_crowd` takes the bake, `PonkoCrowdLevel`s and
`PonkoCrowdInstance`s (start each from `ponko_crowd_instance_at`: white,
clip 0, speed 1), `ponko_ctx_update_crowd` replaces the characters,
`ponko_ctx_crowd_len` counts them and `ponko_ctx_free_crowd` frees the
crowd. Bad input is `PONKO_INVALID_ARGUMENT` with the engine's message. In
C++, `ctx::bake_crowd_animation` returns a move-only `ponko::crowd_animation`
(`crowd_clip_of` wraps a clip), and `ctx::create_crowd`, `update_crowd`,
`crowd_len` (`std::nullopt` once freed) and `free_crowd` take `ponko::crowd`
handles (see the C++ above).
