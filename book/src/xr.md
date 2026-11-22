# VR and XR: where the engine stands

Ponko has no headset support. This chapter is the evaluation behind that
answer: what drawing for a headset asks of the renderer, of the frame loop
and of input, what a stereo prototype costs today, which platforms could be
tested, and the decision recorded in the roadmap.

**Decision: later.** Headsets are not planned for a release. The renderer
can already draw two eyes (below), but a headset needs an OpenXR session,
the runtime's frame pacing and swapchains, per-eye projections and tracked
input, none of which exist, and no game built on the engine asks for them
yet. The work moves into a release when one does; the steps it would take
are listed at the end.

## Two eyes with what exists

Two views of the same scene are what the engine's split screen already
draws ([Several cameras](viewports.md)): the main camera is the left eye on
the left half of the frame, one `d3::View` is the right eye on the right
half, the two cameras a head's eye distance apart along the head's right
axis. Side by side in one image is a layout headset runtimes accept (one
swapchain image, a rectangle per eye).

```rust
# use ponko::prelude::*;
use ponko::camera::Viewport;

/// Both eyes of a head at `head` looking along `rotation`, `ipd` meters apart.
fn place_eyes(ctx: &mut Ctx, head: Vec3, rotation: Quat, ipd: f32) {
    let mut eye = ctx.scene3d.camera;
    eye.rotation = rotation;
    eye.fov_y = 90f32.to_radians();
    let right = rotation * Vec3::X;
    let mut left = eye;
    left.position = head - right * (ipd / 2.0);
    left.rect = Viewport::split(2, 0);
    let mut right_eye = eye;
    right_eye.position = head + right * (ipd / 2.0);
    right_eye.rect = Viewport::split(2, 1);
    ctx.scene3d.camera = left;
    ctx.scene3d.views.clear();
    ctx.scene3d.views.push(d3::View::new(right_eye));
}
# Game::new("Stereo", 1920, 1080).run_headless(1, |ctx| {
#     place_eyes(ctx, vec3(0.0, 1.7, 4.0), Quat::IDENTITY, 0.063);
#     assert_eq!(ctx.scene3d.views.len(), 1);
# });
```

The prototype (`src/render/stereo_render_test.rs`) draws the 3D reference
game's level ([Shard Grove](shard-grove.md)) this way, offscreen, under the
game's sun with shadows, from a standing head beside the first shrine. Its
tests hold that each eye sees from its own position (a marker 1.5 m ahead
lands the expected number of pixels apart in the two eyes, and with no eye
separation at the same column) and that both eyes read one set of shadow
maps.

### What two views share, and what they don't

| Work | Shared by the eyes | Notes |
|---|---|---|
| Sun shadow cascades | yes | fitted once, around every view (the right eye is one of the main frame's shadow views); drawn once a frame |
| Point and spot light shadows | yes | planned for every camera, drawn once |
| Skinning, global illumination, probes, target cameras, compute passes | yes | the main frame updates them; views read them |
| Meshes, materials, pipelines | yes | one copy |
| Culling and draw lists | no | each eye collects and culls its own models on the CPU, and uploads its own instance buffer |
| Depth prepass, opaque, transparent, sky, post | no | each eye records and runs every pass, at its own size |
| Temporal history (TAA, temporal AO, SSR) | no | each eye keeps its own, which is what a headset needs |
| Submission | no | each view is submitted before the next is prepared (they reuse the renderer's uniform and light buffers) |

So a stereo frame does the per-camera work twice and the per-scene work
once. Nothing is drawn in a single pass for both eyes: there is no
multiview rendering (one draw writing two array layers), no culling against
the union of both eyes' frustums, and no reuse of one eye's lists for the
other.

### Cost against one view

The ignored benchmark in the same file draws 60 frames per row, three
rounds, best medians, in release:

```sh
cargo test --release --lib --features 3d stereo_frame_cost -- --ignored --nocapture
```

Apple M4 Max (Metal), Shard Grove's level, default post settings, shadows
on. **The machine was heavily loaded** (a load average above 100: many other
builds were running), so the wall-clock numbers moved from run to run; the
ranges are three runs.

| Eye size | Frame | CPU (ms) | Until the GPU is idle (ms) |
|---|---|---|---|
| 960 x 1080 | one view, one eye's size | 1.03 – 1.07 | 2.2 – 3.2 |
| | one view, both eyes' size | 1.11 – 1.16 | 3.6 – 4.7 |
| | stereo: two views | 1.80 – 2.12 | 4.9 – 7.2 |
| 1440 x 1600 | one view, one eye's size | 1.11 – 1.33 | 4.0 – 6.7 |
| | one view, both eyes' size | 0.76 – 1.24 | 5.6 – 7.7 |
| | stereo: two views | 1.83 – 2.39 | 5.4 – 13.6 |

What holds across the runs:

- The CPU side of a stereo frame costs 1.6 to 2.1 times one eye's: the
  collecting, preparing and recording run once per eye. A view as wide as
  both eyes costs about what one eye costs: the CPU work is per camera, not
  per pixel.
- Until the GPU is idle, stereo took 1.2 to 2.3 times one eye's frame, and
  in most runs somewhat longer than one view of the same pixels: the second
  eye adds its pixels, its own fixed passes and a submission of its own.
  The noise on this machine is too large to split that further.
- GPU pass times are not quoted: on this adapter only 4 to 6 of the frame's
  passes come back timed, and the timer holds 32 passes a frame, which two
  full views can pass.

The grove is a small level; on a heavy scene the doubled CPU work (culling
and recording per eye) is what multiview and shared culling would save.

## What a headset needs from the renderer

- **Per-eye projections.** A runtime gives each eye an asymmetric field of
  view (four angles: left, right, up, down) and a pose, every frame.
  `d3::Camera` has a symmetric `fov_y` and takes its aspect ratio from its
  rectangle; it needs an off-axis projection.
- **The runtime's images.** The eyes draw into swapchain images the runtime
  owns (acquired, waited on and released each frame), either one per eye,
  one with two array layers, or one with a rectangle per eye. The engine
  draws into its window's surface or its own textures; it needs to draw a
  frame into images it was handed.
- **Multiview (optional, for cost).** Drawing both eyes in one pass into a
  two-layer image halves the CPU recording and the draw calls. wgpu exposes
  it as a device feature on some backends; every pass and shader would need
  a view index.
- **Depth for reprojection.** Runtimes reproject the last frame when one is
  late; giving them the eyes' depth (an extra composition layer) makes that
  better. The engine's depth is reverse-Z in a 32-bit float, which runtimes
  accept with the near and far planes stated.
- **The Vulkan instance and device the runtime asks for.** OpenXR on
  desktop and standalone headsets goes through Vulkan (or Direct3D on
  Windows): the runtime names the instance and device extensions and the
  physical device to use. wgpu has to be created from those raw objects
  (through its hal interop), not from its own adapter request.

## Frame timing

A headset paces the frame, not the display's vsync:

1. `xrWaitFrame` blocks until it is time to start a frame and returns the
   time the frame will be shown (the predicted display time);
2. the eyes' poses are asked for at that time (`xrLocateViews`), as late as
   possible, just before the cameras are set;
3. `xrBeginFrame`, draw both eyes into the acquired images, release them,
   and `xrEndFrame` with the composition layers.

For the engine's loop that means: the presentation wait becomes the
runtime's wait; the fixed simulation steps still run as now, but the camera
is set from the head pose after them, at render time, never from a
simulation tick; and the frame budget is the headset's refresh (72, 90 or
120 Hz: 13.9, 11.1 or 8.3 ms for both eyes). A late frame is reprojected by
the runtime, so hitches show as smearing rather than a stall, and
automatic quality (`ctx.scene3d.autotune`) has to aim at that budget.

## Input and tracking through OpenXR

OpenXR input is action based, which matches the engine's
[named actions](actions.md): an application declares action sets
(boolean, float, 2D vector and pose actions, and haptic outputs), suggests
bindings per controller profile, and the runtime maps them to whatever
controllers the player has. An OpenXR backend would feed the existing
`ActionMap` and add:

- **Poses**: the head, each hand's grip and aim, as spaces located at the
  predicted display time; the aim pose is what retained UI panels already
  take as their pointer ray (`ctx.ui.set_pointer_ray`, see
  [Retained game UI](ui.md));
- **Haptics**: a vibration action per hand;
- **The play area**: the reference space (standing or room scale) and its
  bounds;
- **Session state**: focus lost and regained (the headset taken off, the
  system menu), which pauses the game as losing the window's focus does.

Spatial audio already follows a listener; the head pose would drive it.

## Platforms that could be tested

| Platform | Runtime | Graphics | Could be tested |
|---|---|---|---|
| Linux | Monado (open source) | Vulkan | yes, also without a headset: Monado has a simulated device, so a CI job could run a session |
| Windows | SteamVR, Meta's PC runtime, others | Vulkan or Direct3D 12 | with a headset |
| Standalone headsets (Android) | the vendor's OpenXR runtime | Vulkan | needs the Android target, which is not supported (the mobile decision is in the roadmap) |
| macOS | none | | no OpenXR runtime |
| Apple's headset | its own API, not OpenXR | Metal | a separate backend; not covered by OpenXR |
| Web | WebXR, in the browser | WebGL2 (WebGPU bindings are early) | a separate backend; the stereo drawing above applies |

## If it becomes a goal

In order, each verifiable on its own:

1. An off-axis projection on `d3::Camera` (four field-of-view angles), with
   picking and culling following it.
2. Drawing a frame into images the game hands the renderer (array layers or
   rectangles), offscreen-tested as the prototype is.
3. An optional `xr` feature with an OpenXR session on Vulkan: instance and
   device from the runtime, its frame loop in place of the presentation
   wait, the head pose applied at render time; tested against Monado's
   simulated device.
4. Controllers through the action map, poses, haptics and session focus.
5. Measured, if CPU cost asks for it: multiview, and culling both eyes once
   against their combined frustum.
