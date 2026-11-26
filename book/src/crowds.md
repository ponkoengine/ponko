# Crowds: thousands of animated characters

> **Maturity: [tested](maturity.md).** Game: none. Tests: `tests/crowds.rs`,
> `crowd_render_test.rs`. Platforms: Linux, macOS and Windows, headless (the
> workspace tests in the list of checks). Graphics: offscreen render tests,
> required on Windows (DirectX 12 on the WARP software adapter) and run on
> Linux and macOS only where a GPU is found. Missing: a reference game that
> uses it.

A stadium, a market square, an army on the march: thousands of characters
that share a skeleton, a few meshes and a few clips. As entities, each would
be posed on the CPU, skinned on its own and drawn on its own: 10,000 of them
take about 20,000 draw calls and most of a frame before anything else is drawn. A
**crowd** draws them all in a handful of draws.

A crowd is the skinned sibling of an [instance batch](3d.md#gpu-driven-instances):
one record per character, culled per view on the GPU, drawn by indirect
draws in the normal depth, color, shadow and motion passes, in global
illumination's probes and in window views, and left out where occlusion
culling finds it hidden. Its clips are
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

A level can **crossfade** into the next instead of popping:
`with_fade(4.0)` on a level whose band ends at 40 m starts, at 36 m, to
draw its characters through a dither pattern that keeps fewer and fewer
of their pixels, while the level whose band starts at 40 m draws the
pixels it leaves out; by 40 m the next level draws them whole. Together
the two cover every pixel of the character once. A level with a fade and
no level starting where it ends fades out to nothing. The fade is in
world units, at most the band's width (0, the default, switches at
once); levels cast shadows through the same dither. While characters
crossfade, both levels draw them, and those draws run a fragment shader
in the depth and shadow passes too: on the GPU path every crossfading
level adds one more indirect draw per view and mesh part, made only for
crowds that have a fade.

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

## Long sessions: the clock

The times a character plays count from the crowd's last update, and the
clips run on from there on the GPU. A crowd keeps when it was last updated
in 64-bit seconds of game time; the shaders get the 32-bit seconds since
then. So that number never grows large, every 16 seconds of game time the
characters' clip times are moved on to the present (wrapped into a
looping clip, held at the end of one that holds) and counted from there: a
crowd that stands for a whole day steps its clips, and writes its motion
vectors, as finely as in its first minute. On the GPU path that is one
small compute dispatch over the characters every 16 seconds; on the CPU
path a loop over them. A test runs a crowd through 24 hours of game time
and checks its pose and its motion vectors.

The times you hand an update are 32-bit too: give a time within its clip
(a looping clip's time modulo its length, worked out in `f64` if it comes
from a long-running clock), not seconds since the game began. At a day's
86,400 s a 32-bit time is only precise to about 1/128 s.

## In probes, window views and occlusion culling

- **Global illumination.** The probes see a crowd's levels that are drawn
  in cameras, lit as models are, and its shadow levels cast in the probes'
  sun shadow map. That holds on both paths. Global illumination needs
  compute shaders, so a device without them (WebGL2) has no probes at all;
  a device with compute shaders but fewer than the four storage buffers a
  shader stage that compute culling needs runs global illumination while
  its crowds take the CPU path, and a test draws the probes there too. A
  crowd is culled for the probes in seven more views
  whatever the size of the grid (the probes' shadow map and one per face
  direction, as GPU-driven instance batches are), and a face direction
  keeps only the characters at least one probe pixel wide seen from one of
  the frame's probes. A character's level there is the one for its
  distance from the camera. Each of these views adds a visible list to the
  crowd (4 bytes per character and level), only on frames that update
  probes.
- **Window views.** A [window view](viewports.md) culls and draws crowds
  for its own camera, at the main frame's time, with its own motion
  vectors: one more cull dispatch on the GPU path, one more CPU cull on
  the CPU path. Crowds have no render layers: every view draws them.
- **Occlusion culling.** With the camera's
  [occlusion culling](3d.md#occlusion-culling) on, the camera's view
  leaves out the characters whose culling box (see below, as a box along
  the world's axes, grown by how far the camera moved since) lies behind
  the depth grid everywhere it covers. On the GPU path the cull pass reads the grid
  as a uniform: the finest level of its pyramid that fits a uniform binding
  (64 KiB, or the device's smaller limit; a level is the farthest depth of
  four of the one below, so a coarser one hides less, never more). Hidden
  characters still cast shadows. As with models, a character that comes
  out from behind a wall can show a frame or two late, and since the grid
  is of an earlier frame, so can one standing behind a character that
  walked away.

## Culling bounds

The bake keeps a box per frame range of each clip: every 4 frames of a
clip (0.13 s at 30 frames a second) share a box around every level's mesh
in those frames and the frame after them, which skinning blends with
their last. A character is culled by the box of the range its clip shows
at the frame's time, under its transform (the range its blend clip shows
instead once the blend is whole, and both joined while it blends): an
oriented box against each view's planes, a box along the world's axes
against the occlusion grid, and its half diagonal for the probes' size
cut. So a crowd that mostly stands and sometimes jumps culls its standing
characters by their standing box, and a jumping one by where it is in
the jump: only near the top does it reach as high as the jump does. On
the CPU path a character is culled by the range of the baked frame it
shows. A box costs 32 bytes; the cull pass reads one more vec4 per clip
a character plays.

In the tests' scenes, a box around the whole clip against a box per frame
range:

| Scene | Box per clip | Box per frame range |
| --- | --- | --- |
| 13 figures held at points across a jump, below the camera view's bottom edge (only the middle of the jump lifts a head into it) | 13 drawn | 10 drawn, the picture the same as the 13 drawn as skinned models |
| 26 figures behind a wall lower than the jump, each at its own point of it: kept by occlusion culling (13 clear the wall's line) | 26 | 17 on the GPU path; 26 on the CPU path, whose grid test reads coarser cells for a box over many |
| The same, jumping on for 10 frames | 260 | 190 on the GPU path |
| 169 small figures around global illumination's probes, every fifth held at the top of a jump: kept by the probes' face views over 8 frames | 1,047 | 918 |
| The same, every one held at the top of a jump | 1,677 | 1,040 |

Before boxes per clip, crowds used a single sphere around every clip: in
a test scene of 26 characters behind a low wall, 4 of them jumping,
occlusion culling kept all 26 with it and keeps only the 4 jumping ones
since. In the frustum and probe scenes both paths keep the same
characters.

## How it is drawn: two paths

Every frame, on devices with compute culling and storage buffers
(`GpuFeature::ComputeCulling`: most native GPUs, and WebGPU in the browser):

- A compute pass tests each character's culling box against every view
  of the frame (the camera, the sun's
  cascades, the shadow views of point and spot lights, target cameras,
  global illumination's views), the camera's against the occlusion grid,
  and its distance against each level's band, and appends it to that view
  and level's list. Every 16 seconds of game time a dispatch before it
  moves the characters' clip times on (see the clock above).
- Each (view, level, mesh part) is one indirect draw over its list. The
  vertex shader reads the character's record, finds its clips' two nearest
  frames, blends the joints' matrices between them and with the second clip,
  and skins the corner.
- A crowd's cull pass costs about 0.02 ms of GPU time for 10,000
  characters, with or without two levels and sun shadows (the benchmark
  prints it to a hundredth of a millisecond; three runs on an idle
  machine on 2026-11-22, with culling by each clip's box and by frame
  ranges' boxes). Earlier figures were 10 µs (40 µs with two levels and
  sun shadows), and 30 to 50 µs on a busy machine. A crossfading
  level adds its dithered draws: 30 draws instead of 18 for the
  benchmark's two levels with sun shadows (`crowd_benchmark`'s
  crossfading scene).

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
prepass, on 2026-11-26 on an idle machine (revision `431fae00` plus the
CPU path's grouping below, 1-minute load 2.5 to 3.0, nothing else
running): three runs of the command, each building and measuring every
scene three times; the median of the runs' medians and, in brackets, the
lowest and highest of them. "Prepare + record, 1 thread" is the CPU work
of the frame measured on one thread.

| Scene | GPU ms | Prepare + record ms, 1 thread | Draws | Posing ms (CPU) |
|---|---|---|---|---|
| Ground only | 0.26 (0.23–0.32) | 0.13 (0.13–0.13) | 2 | – |
| Crowd | 3.89 (3.88–3.89) | 0.16 (0.16–0.16) | 4 | – |
| Crowd, sun shadows | 4.90 (4.89–4.90) | 0.23 (0.23–0.24) | 12 | – |
| Crowd, two levels (36 triangles past 40 m), sun shadows | 0.89 (0.89–0.89) | 0.24 (0.24–0.26) | 18 | – |
| Crowd, two levels crossfading over 5 m, sun shadows | 0.90 (0.89–0.90) | 0.25 (0.25–0.27) | 30 | – |
| Crowd, CPU path (WebGL2's limits) | 3.60 (3.60–3.60) | 0.47 (0.45–0.48) | 126 | – |
| Skinned models | 6.76 (6.76–6.77) | 3.50 (3.44–3.51) | 19,246 | 0.35 |
| Skinned models, sun shadows | 7.17 (7.16–7.18) | 3.69 (3.63–3.71) | 20,310 | 0.36 |

So on this machine 10,000 animated characters as a crowd **meet 60 fps by
their measured cost**: 0.9 to 4.9 ms of GPU time and 0.16 to 0.47 ms of CPU
work. As skinned models the same characters take 6.8 to 7.2 ms of GPU time
and about 3.5 to 3.7 ms of CPU work a frame, with 0.35 ms more to pose
them: about 10.5 ms together, inside 16.7 ms too, but two to ten times the
crowd's. The frame times this benchmark also prints (1.5 to 5.6 ms for the
crowds, 10.0 to 10.5 ms for the skinned models) wait for the GPU after the
CPU work, so they are not the frame rate a game would reach.

The table of 2026-11-20 (revision `e60e15a8`, also idle) had the crowd at
7.01 ms of GPU time (8.42 with sun shadows, 1.06 with two levels) and the
skinned models at 27.89 ms of GPU time and 9.39 ms of CPU work (28.89 and
9.96 ms with shadows). The skinned models got faster when skinning was
batched, one dispatch per mesh instead of one per model (#637). The crowd's
GPU time was measured again on 2026-11-22 within 0.03 ms of 7.01 ms; what
brought it to 3.89 ms since was not traced to one change.

**The CPU path's cull.** On 2026-11-22 the CPU path's prepare + record had
gone from 0.44 ms to 0.55 ms (0.54–0.56), and measured again before the
change below it was 0.55 ms (0.53–0.57), almost all of it in the prepare
step (0.38–0.41 ms), where the CPU path culls the characters. A profile
(`sample`) of that step split it about 60/40 between culling and sorting
the 10,000 draw keys. Since 2026-11-21 each character is culled by the box
of the frame range it shows, under its own transform (a box against each
view's planes), instead of by a sphere worked out when the crowd was set:
that is where the 0.1 ms came from, the price of culling each character by
what it shows. The sort is now a count of how many characters each key
(view, level, crossfade, posed frame) has, linear in the characters and
giving the same order (`grouped_keys_come_out_as_sorting_them_orders_them`):
prepare + record 0.47 ms (0.45–0.48), the prepare step 0.32 to 0.34 ms.
The rest above 0.44 ms, about 0.03 ms, is the box test.

The CPU path's GPU time is lower than the GPU path's here because its
corners are posed in advance: it trades blending between frames and clips
for that, and it draws once per frame shown.

## Not yet

- Stencil states of a material are ignored on the GPU path. Transparent
  materials are drawn unsorted; give crowds opaque or cutout materials.
- The order characters are drawn in within a level isn't fixed.

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

Clips can also be made in code: `ponko_ctx_create_clip` takes
`PonkoJointTrack`s (a joint of the skeleton, a `PonkoJointChannel`, an
interpolation and the keys) and makes a clip to bake into a crowd or play
on a skeleton; C++ builds the tracks with `joint_track_of`:

```cpp
{{#include ../../examples/cpp/guide.cpp:crowd_clip_tracks}}
```

`ponko_ctx_live_counts_ex` (C++ `ctx.live_counts_ex()`) counts the live
crowds next to the other handles, with instance cells, collision meshes,
sprite shaders and materials, and sky shaders: a leak check that sees
them all.

Crossfading levels take the versioned `PonkoCrowdDesc`: start it with
`ponko_crowd_desc_defaults()`, fill in the bake, the levels and the
characters, point `fades` at one fade per level (null: none), and make the
crowd with `ponko_ctx_create_crowd_ex`; `PonkoCrowdLevel` keeps its layout.
C++ has a `ctx::create_crowd` that takes the fades after the levels:

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# use ponko::render::{CrowdAnimation, CrowdInstance, CrowdLevel, CrowdOptions, Mesh};
# fn f(ctx: &mut Ctx, bake: &CrowdAnimation, near: Mesh, far: Mesh, people: &[CrowdInstance]) {
let levels = [
    CrowdLevel::new(near, 0.0, 40.0).with_fade(4.0), // into the far level from 36 m
    CrowdLevel::new(far, 40.0, f32::INFINITY).with_influences(1),
];
let crowd = ctx.gfx.create_crowd(bake, &levels, people, CrowdOptions::default());
# let _ = crowd;
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:crowd_fade}}
```

</div>
</div>
