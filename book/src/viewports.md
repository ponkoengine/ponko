# Several cameras: split screen, minimaps, picture in picture

> **Maturity: [tested](maturity.md).** Game: none. Tests:
> `tests/viewports.rs`, `views_render_test.rs`, `views2d_render_test.rs`.
> Platforms: Linux, macOS and Windows, headless (the workspace tests in the
> list of checks). Graphics: offscreen render tests, required on Windows
> (DirectX 12 on the WARP software adapter) and run on Linux and macOS only
> where a GPU is found. Missing: a reference game that uses it.

A window can show the world through several cameras at once: two or four
players side by side, a minimap in a corner, a rear-view mirror, a
security monitor. Each camera draws into its own rectangle of the window,
in order, and has its own *render layers* (what it draws), clear colour and,
in 3D, its own post settings.

With one camera nothing changes: the main camera covers the whole window
and draws everything, and none of this costs anything.

## What a camera draws into

Every camera has a `rect`, a `Viewport`: the part of the window it draws
into, as fractions of the window's size from its top-left corner.
`Viewport::FULL` (the default) is the whole window. Fractions keep a layout
when the window is resized.

| Viewport | Part of the window |
|---|---|
| `Viewport::FULL` | all of it |
| `Viewport::split(2, 0)`, `split(2, 1)` | left and right halves |
| `Viewport::split(4, i)` | quarters: top-left, top-right, bottom-left, bottom-right |
| `Viewport::split(3, i)` | three of the quarters; the bottom-right one stays free for a map or a score |
| `Viewport::split(5, i)` and more | equal columns |
| `Viewport::new(0.75, 0.0, 0.25, 0.25)` | the top-right corner |

The main cameras are `ctx.camera` (2D) and `ctx.scene3d.camera` (3D).
More cameras go in the view lists, `ctx.views2d` and `ctx.scene3d.views`:
the main camera draws first, then each view in the list's order, on top.

## Two players in 3D

<div class="lang-pair">
<div>

```rust
use ponko::camera::Viewport;
// Player one in the left half, player two in the right one.
ctx.scene3d.camera.rect = Viewport::split(2, 0);
let mut p2 = ctx.scene3d.camera;
p2.rect = Viewport::split(2, 1);
p2.position.x += 30.0;
ctx.scene3d.views.push(d3::View::new(p2));
// Each frame, move both cameras:
ctx.scene3d.views[0].camera.position.x += 0.1;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:split_screen3d}}
```

</div>
</div>

Each 3D view is a whole frame of its own camera, at its rectangle's size:
its own culling, its own temporal history (temporal antialiasing, ambient
occlusion and reflections never mix two cameras' pictures) and its own
`Post` (`View::with_post`; without one it uses `ctx.scene3d.post`). What
doesn't depend on the camera is done once for all of them: skinning,
compute passes, target cameras, global illumination and the shadow maps.
The sun's cascades are fitted around every view, so the farther apart the
cameras are, the coarser the shadows get; keep `Lighting::shadow_distance`
close to what the players need. Point and spot light shadows go to the
shadowed lights nearest any of the cameras (up to
`Lighting::light_shadow_views` of them), in one shadow atlas drawn once a
frame; each light's shadow is as sharp as the camera nearest it needs.

[Crowds](crowds.md) are culled for each view's camera too, at the main
frame's time.

Views draw without occlusion culling: it reads the depth of the camera's
last frame, which only the main camera keeps. A view draws everything in
its frustum, which costs more in a dense city but never hides anything
that should show. The game's scene compute passes run once, for the main
camera.

`Post::msaa` is the main camera's for every view (a view's own is
ignored): every 3D pipeline is built for the renderer's one sample count,
and a count per view would build and keep a second set of them. A cheap
view can turn antialiasing off in its own `Post` instead.

## A minimap with icons

`RenderLayers` says which of 32 layers something is on, and which layers a
camera draws. Things are on layer 0 (`RenderLayers::DEFAULT`) unless they
have a `RenderLayers` component; cameras draw every layer
(`RenderLayers::ALL`) unless told otherwise. A thing is drawn by a camera
when their layers share one.

<div class="lang-pair">
<div>

```rust
use ponko::camera::{RenderLayers, Viewport};
# let icon = ctx.world.spawn((d3::Transform::default(),));
// Icons on layer 1: only the map draws them.
ctx.world.insert(icon, RenderLayers::layer(1));
ctx.scene3d.camera.layers = RenderLayers::ALL.without(1);
let mut map = ctx.scene3d.camera;
map.rect = Viewport::new(0.75, 0.0, 0.25, 0.25); // the top-right corner
map.position = vec3(0.0, 80.0, 0.0);
map.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2); // straight down, -Z up
map.layers = RenderLayers::ALL;
let mut cheap = ctx.scene3d.post;
cheap.bloom = 0.0;
cheap.ambient_occlusion.enabled = false;
ctx.scene3d.views.push(d3::View::new(map).with_post(cheap));
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:minimap}}
```

</div>
</div>

A 3D model off a camera's layers still casts its shadow in that camera's
picture: hide a player's own body from their first-person camera and
their shadow stays on the ground.

## Two players in 2D

2D views work the same way. A view can fill its rectangle with a colour
first (`with_clear`); without one it draws over what is there. What you
draw goes on the layers set with `ctx.gfx.set_layers` (until you set them
again; every frame starts on layer 0), and sprite entities on their
`RenderLayers` component.

<div class="lang-pair">
<div>

```rust
use ponko::camera::{RenderLayers, Viewport};
ctx.camera.rect = Viewport::split(2, 0);
let mut right = ctx.camera;
right.rect = Viewport::split(2, 1);
right.position = vec2(2000.0, 0.0);
ctx.views2d.push(d2::View::new(right).with_clear(Color::rgb(0.1, 0.1, 0.2)));
// Draw a marker only the cameras on layer 1 show:
ctx.gfx.set_layers(RenderLayers::layer(1));
ctx.gfx.draw_rect(Rect::new(10.0, 10.0, 10.0, 10.0), Color::YELLOW);
ctx.gfx.set_layers(RenderLayers::DEFAULT);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:split_screen2d}}
```

</div>
</div>

The UI (`ctx.gfx` screen drawing, panels, the retained UI) is drawn once,
over every view. To put something in one player's half, place it inside
that camera's `screen_rect()`, its rectangle in window pixels:

```rust
# ctx.camera.rect = ponko::camera::Viewport::split(2, 0);
let half = ctx.camera.screen_rect();
ctx.gfx.draw_text("Player 1", vec2(half.min.x + 12.0, half.max.y - 32.0), 22.0, Color::WHITE);
```

2D lights (lights, shadow casters, ambient light and normal maps) light
every view: each view's sprites are lit in its own rectangle, by the
lights near what that camera sees.

## Debug drawing

`ctx.debug` draws on layer 0 (`RenderLayers::DEFAULT`), so every camera
that shows layer 0 shows it, in 2D and in 3D: depth-tested lines, lines on
top and labels, and the built-in views (colliders, bounds, lights, ...).
Depth-tested 3D labels (`.depth_tested()`) are drawn in each view's own 3D
frame, facing that view's camera and hidden by what is nearer to it.
A minimap whose camera leaves layer 0 out stays clean. Shapes in window
pixels (`.screen()`) are drawn once, over the whole window.

## Picking through the right camera

The screen and world conversions of [Camera, text and UI](camera-ui.md)
follow each camera's rectangle: `screen_to_world`, `world_to_screen`,
`screen_ray` and `screen_to_ground` take and give window pixels, and
`viewport()` is the size of the camera's rectangle. To pick, ask for the
camera under the point first: `ctx.scene3d.camera_at` (3D) and
`d2::View::camera_at` (2D) give the last view drawn there, or the main
camera.

<div class="lang-pair">
<div>

```rust
let mouse = ctx.input.mouse_pos();
let ground = ctx.scene3d.camera_at(mouse).screen_to_ground(mouse, 0.0);
let clicked = d2::View::camera_at(&ctx.camera, &ctx.views2d, mouse).screen_to_world(mouse); // 2D
# let _ = (ground, clicked);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:pick_views}}
```

</div>
</div>

`ctx.mouse_world()` already goes through the 2D camera under the mouse, and
so does the inspector's picking.

A changed `rect` takes effect at once: the camera's conversions follow it
in the same frame.

## Saving

`RenderLayers` is a component like any other: it saves with the world and
loads back, and a scene file writes it as its bits (`"RenderLayers": 2`
for layer 1). The view lists and camera rectangles belong to the game's
code, not to the world.

## Cost

Each view costs about what the main camera costs at its size: collecting
what it sees on the CPU, and its depth, opaque, transparent and post passes
on the GPU, plus a copy of its picture into the window. A view's
scene-sized textures are made when it first draws and freed when it leaves
the list. 2D views cost one more pass over the frame's sprite batches each,
skipping batches off their layers. With no views and the main camera on the
whole window, nothing of this runs.

The main camera's frame and every view's go to the GPU together, in one
submission: each frame's buffer writes (its camera, lights, light grid and
culling data) are copied into place by its own first command buffer, so a
view drawn after the main camera still reads its own and the main camera
its own.

The scene benchmark measures it (`PONKO_RENDER_VIEWS=2`, `4` or `minimap`,
see `crates/ponko_bench/RENDER_SCENES.md`). On an Apple M4 Max at
1280 x 720, high quality, with GPU pass timing on, its town scene made
these heap allocations per frame (whole process; 2026-11-22, revision
`9235ea05`, three runs on an idle machine, identical counts; CPU time and
frame are medians, the runs within 0.03 ms):

| Cameras | Allocations | Each view adds | CPU ms | Frame ms |
|---|---|---|---|---|
| 1 | 772 | | 0.61 | 3.37 |
| 2 (halves) | 1335 | 562 | 1.22 | 5.06 |
| 4 (quarters) | 2302 | 510 | 2.42 | 8.68 |
| 1 and a minimap (a quarter-sized corner view with bloom, ambient occlusion and screen-space reflections off, FXAA) | 953 | 180 | 0.95 | 4.88 |

Earlier counts (767, 1262, 2094 and 926) were made before `high` turned on
screen reflections' thin objects, which records two more compute passes
for each view that has screen reflections.

Collecting a view, preparing it and recording its passes allocate nothing
in the engine once its buffers have grown. Almost all of the rest is made
inside wgpu, about 24 for each pass a view records (29 render and 3 compute
passes for the town's main camera): its command list growing, the barriers
and attachment checks of each pass, the timestamp queries' reset map and
Metal's own pass objects, plus a few for each buffer copy and about 25 for
each submission. Those can only shrink in wgpu or by recording fewer passes, so
a frame of this scene stays around 750 allocations and a full-size view
around 450. A view with fewer effects records fewer passes, which is why
the minimap costs a third of a split-screen view: give a small view its
own post settings (`d3::View::with_post`) with what it doesn't need turned
off.

Timing the GPU passes is part of that: each timed pass costs wgpu its
timestamp queries' reset map and bookkeeping. A game times one frame in 16
(and every frame only while profiling, the overlay, a trace, a benchmark run
or automatic quality reads every frame; see `ctx.perf.gpu_passes()`). The
same benchmark with `PONKO_RENDER_GPU_EVERY=16` against every frame
(issue #178; the counts repeat exactly from run to run):

| Cameras | Every frame timed | One frame in 16 timed |
|---|---|---|
| 1 | 772 | 695 |
| 2 (halves) | 1335 | 1226 |

(2026-11-22; before, 753 and 676, 1262 and 1184.) With one camera, about
53 of the 77 saved are the timed passes' query
bookkeeping; the rest is the benchmark's separate command buffer that
resolves the timestamps (a game resolves them in its frame's own).

CPU time grows by roughly what the main camera's collecting and recording
cost at the view's size: in the table above each split-screen view adds
about 0.6 ms of CPU, and the minimap 0.34 ms. Measure your own scene with
the benchmark.

The `split_screen` example (`cargo run --release --example split_screen
--features 3d`) has two or four players, a minimap and picking.

In C and C++, `PonkoView2D` and `PonkoView3D` describe a view (versioned:
start from `ponko_view2d_defaults()` / `ponko_view3d_defaults()`); see
[C and C++](c-cpp.md) for the conventions.
