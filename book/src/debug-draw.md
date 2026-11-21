# Debug drawing

`ctx.debug` draws lines, shapes and text labels over the game, in 2D or 3D,
from any system, to show what code is thinking: a ray cast, an AI's target,
a trigger's reach, a path. It is meant for development: it is on in debug
builds of the engine and off in release builds, where every call returns
after a single check.

<div class="lang-pair">
<div>

```rust
# let (eye, aim, hit) = (vec3(0.0, 1.5, 0.0), vec3(0.0, 0.0, -4.0), vec3(0.0, 0.5, -4.0));
ctx.debug.set_enabled(true); // on by default in debug builds of the engine
// Until this system runs again: a ray, a box and a label in the 3D world.
ctx.debug.ray(eye, aim, Color::YELLOW);
ctx.debug.aabb(hit, Vec3::ONE, Color::RED);
ctx.debug.text(hit, "target", Color::WHITE);
// A label hidden behind nearer surfaces, like the lines.
ctx.debug.text(hit + Vec3::Y, "behind the wall?", Color::WHITE).depth_tested();
// For two seconds, over everything, 3 pixels wide.
ctx.debug.sphere(hit, 0.5, Color::GREEN).seconds(2.0).on_top().width(3.0);
// In window pixels: a frame around the minimap.
ctx.debug.aabb(vec2(110.0, 110.0), vec2(200.0, 200.0), Color::WHITE).space(DrawSpace::Screen);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:debug_draw}}
```

</div>
</div>

Every shape takes 2D points (`Vec2`, world units) or 3D points (`Vec3`), the
same call for both:

| Call | Draws |
|---|---|
| `line(a, b, color)` | a line |
| `ray(origin, direction, color)` | a line from `origin` along `direction` |
| `arrow(from, to, color)` | a line with a head at `to` |
| `polyline(points, closed, color)` | lines through the points |
| `aabb(center, size, color)`, `obb(center, size, rotation, color)` | a rectangle or box, turned by `rotation` for `obb` |
| `circle(center, radius, color)` | a circle (on the ground, XZ, in 3D) |
| `sphere(center, radius, color)` | three rings, one around each axis (3D) |
| `capsule(a, b, radius, color)` | the points within `radius` of a segment |
| `frustum(&camera, color)` | what a 3D camera sees (3D) |
| `grid(center, cell, cells, color)` | a grid (on the ground in 3D) |
| `text(at, "label", color)` | a label whose corner is at the point |
| `axes(at, rotation, length)` | x red, y green, z blue (3D) |

Each returns the shape, to change it:

- `.seconds(s)`: keep it for `s` seconds of frame time.
- `.on_top()`: 3D lines show through nearer surfaces instead of being hidden
  by them; 2D lines go over the UI instead of under it.
- `.space(DrawSpace::Screen)` (or `.screen()`): 2D points are window pixels
  from the top-left corner instead of world units. `DrawSpace` is the one
  type sprite draws take too (`DrawParams::with_space`; it is `ponko::debug::DrawSpace`
  and in the prelude). In C it is
  `PonkoDrawSpace`, for sprites (`ponko_draw_params_with_space`) and debug
  styles (`ponko_debug_style_with_space`); in C++ `ponko::draw_space` with
  `ponko::with_space` for both.
- `.depth_tested()`: a 3D label is hidden behind nearer 3D surfaces, as 3D
  lines are, instead of drawn over everything. It is drawn in the 3D frame
  of every 3D camera that shows debug drawing (split-screen views and
  target cameras too, see below), facing that camera and `size` pixels high
  where it is.
- `.width(px)`: line width in pixels (1 by default).
- `.size(px)`: label height in pixels (14 by default).

## How long a shape stays

A shape stays until the system that drew it runs again, so a system that draws
every time it runs shows its shapes steadily, however its rate compares with
the frame rate:

- From a simulation system (`.sim_system`): until the next tick ends, so the
  frames between two ticks show the last tick's shapes.
- From a frame system: one frame.
- With `.seconds(s)`: `s` seconds, then it is gone.

`ctx.debug.clear()` forgets everything, timed shapes included.

## From view systems

Systems declared with an access list (see [System access](system-access.md))
draw through `sys.debug`, with the same calls. Systems of a parallel batch each
draw into their own buffer, merged when the batch ends, so drawing needs no
lock and its order doesn't depend on the threads.

## Built-in views

The engine can draw what it knows by itself, every frame while debug drawing
is on:

<div class="lang-pair">
<div>

```rust
use ponko::debug::DebugView;
ctx.debug.show(DebugView::Colliders, true);
ctx.debug.show(DebugView::Hierarchy, true);
ctx.debug.set_collider_distance(30.0); // mesh and terrain triangles within 30 m of the camera
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:debug_draw_views}}
```

</div>
</div>

| View | Shows |
|---|---|
| `Colliders` | 2D and 3D physics colliders (green, sensors yellow); 3D collision meshes and terrain show their triangles within `collider_distance` (50 m by default) of the 3D camera, convex hulls all their edges |
| `NavMesh` | the navmesh's polygon edges |
| `Bounds` | 3D models' bounding spheres and 2D sprites' rectangles |
| `Lights` | the reach of 2D and 3D point and spot lights |
| `Hierarchy` | the axes of each attached entity, and a line to what it is attached to |

## At runtime and from tools

`ctx.debug.set_enabled(on)` turns it on or off while the game runs (off forgets
everything drawn). The [inspector](inspector.md)'s **Debug drawing** section
shows and flips the same switches with the mouse: debug drawing, each
built-in view, the colliders view's distance and the game's own toggles. `Game::debug_draw(on)` chooses before the game starts,
overriding the build's default. A running game with the control endpoint
answers `debug.draw` (`on`, `views`, `toggles`, `clear`; see
[the control endpoint](control.md)), and its MCP server has the `debug_draw`
tool, so an agent can show the colliders before taking a screenshot.

A game or plugin adds switches of its own for what it draws:
`Game::debug_toggle("ai.paths", false)`, then
`if ctx.debug.toggled("ai.paths") { ... }` in the system that draws them
(true while debug drawing is on and so is the toggle). Keys turn them on
with `ctx.debug.set_toggle`, tools with `debug.draw`'s `toggles`, which
also lists them ([Plugins for other crates](plugins.md#debug-toggles)).

## In every camera

Debug drawing is on the default layer (layer 0), so every camera that shows
that layer shows it: the main camera, each view of a split screen (see
[Viewports](viewports.md#debug-drawing)), and each target camera drawing
into a [render target](3d.md#render-targets) that draws particles (a
mirror or a monitor shows the 3D lines and the depth-tested labels, a
reflection probe's capture doesn't).

A camera's `debug_draw` changes that: `Some(true)` draws it, `Some(false)`
leaves it out, whatever the camera's `particles` says; `None`, the default,
is the rule above. A monitor can show the debug drawing without the smoke
(`TargetCamera::with_debug_draw(true)` after `with_particles(false)`), or a
top-down map can be the only place it shows, with the window's camera's
`debug_draw` at `Some(false)`. From C and C++ it is the `debug_draw` field
of `PonkoCamera3DEx` and `PonkoTargetCameraEx`, a `PonkoDebugDraw`
(`ponko::camera_debug_draw`). The inspector's **Debug drawing** section
lists every 3D camera (the window's, each extra view's, each target
camera's) under **Cameras** with three buttons for its `debug_draw`:
**Default**, **On** and **Off**.

In a render target the 3D lines and
depth-tested labels are drawn against that camera's own depth, and 3D
labels on top are on top there as they are on screen: drawn last, over the
scene, the particles and the lines, laid out facing that camera, the size
in pixels of the target. In the window they stay screen text. 2D shapes
are sprites and stay out of render targets.

## Cost

All of a frame's 3D lines are one buffer and at most four draw calls a
camera (the depth-tested ones, then the ones on top, each opaque first and
see-through second), expanded into quads on the GPU. Opaque lines (alpha 1,
nearly all) are drawn without blending; see-through ones (alpha below 1)
with alpha to coverage when the frame has several samples a pixel (the
default 4x MSAA), blended with one. 2D lines are sprites in one batch, and labels are
screen text (depth-tested 3D labels are 3D text). Buffers keep their capacity,
so steady drawing doesn't allocate: `tests/debug_draw_alloc.rs` checks it with
a counting allocator, and `cargo run --release -p ponko_bench -- debug/`
prints each case's allocations per frame.

Handing 100,000 lines to the renderer costs about 0.5 ms a frame for 3D lines
and 0.6 ms for 2D lines on a laptop (`cargo run --release -p ponko_bench --
debug/`), measured while the machine was busy with other work. Turned off, the
100,000 calls cost about 0.1 ms.

On the GPU, the offscreen test `hundred_thousand_lines_gpu_time` (`cargo test
-p ponko --lib --features 3d hundred_thousand_lines_gpu_time -- --ignored
--nocapture`) times 100,000 one-pixel lines with GPU timestamps, at 1280x720
over a small scene whose opaque pass takes 0.38 ms. Medians of about 160
frames, cases taking turns, two runs each, on an Apple M4 Max (Metal):

| 100,000 lines | 4 samples a pixel | 1 sample |
|---|---|---|
| in view (half on top), blended as before | +4.5 ms | +1.2 ms |
| in view (half on top), opaque without blending | +1.5 ms | +0.9 ms |
| all on top, blended as before / now | +5.0 to +5.8 ms / +1.6 ms | +1.9 ms / +0.9 ms |
| behind the camera (vertex work only) | +0.6 ms | +0.6 ms |

Most of the cost was blending: a line one pixel wide covers only some of a
pixel's samples, and blending those into the 4-sample HDR frame cost about
three times as much as writing them. Vertex work is about 0.6 ms of what is
left; the rest is filling the pixels the lines cover. The machine was busy with
other builds and GPU tests while this was measured (the spread was within 3%
between runs); the numbers are to be repeated on a quiet machine.

See-through lines don't blend either with several samples a pixel: with
alpha to coverage a line of alpha 0.5 covers half of each pixel's samples,
and the frame's resolve averages them with what is behind, as blending
would. The same test times the 100,000 lines (half on top) at alpha 0.5
both ways, taking turns in one run; the opaque pass's median over about 160
frames in four runs, on the same M4 Max while about a dozen other builds
ran (load 130 to 170):

| 100,000 see-through lines | 4 samples a pixel | 1 sample |
|---|---|---|
| blended (as before) | 3.70, 4.81, 4.67, 4.84 ms | 1.67 ms each run |
| alpha to coverage (now, 4 samples) | 3.03, 3.03, 3.00, 3.06 ms | blended: 1.67 ms each run |
| for comparison: the same lines opaque / no lines | 1.89 / 0.41 ms | 1.30 / 0.44 ms |

So the see-through lines' own cost at 4 samples went from about 3.3 to 4.4
ms to about 2.6 ms; blended runs were the noisy ones (single frames from 1.5
to 5.1 ms), the coverage runs stayed within 0.06 ms of each other. Coverage
comes in steps of a quarter with 4 samples (how a GPU rounds alpha to
samples, and whether it dithers, varies), and
overlapping see-through lines no longer add up: the second covers the same
samples as the first. With one sample a pixel (WebGL2, or MSAA off)
see-through lines are still blended and cost what they did.

Notes:

- 3D lines are drawn in the lit scene, after particles, so exposure and tone
  mapping apply to their colors as to an unlit surface.
- Labels are drawn on top unless a 3D one is `depth_tested`, and a 3D point's
  label hides while the point is behind the camera.
- A headless game records shapes but draws nothing.

## From C and C++

The C functions are `ponko_ctx_debug_line2d`, `ponko_ctx_debug_line3d` and
the like (`ray`, `arrow`, `box`, `circle2d`, `sphere3d`, `capsule`,
`polyline`, `grid`, `frustum3d`, `text`, `axes3d`), each with a
`PonkoDebugStyle*` (null for the defaults, or from
`ponko_debug_style_default()`), plus `ponko_ctx_debug_set_enabled`,
`ponko_ctx_debug_show` with a `PONKO_DEBUG_VIEW_*`, `ponko_ctx_debug_clear` and
`ponko_ctx_debug_stats`. `ponko_ctx_debug_text3d_depth_tested` draws a
depth-tested 3D label, `ponko_ctx_debug_set_collider_distance` sets how far the
colliders view draws mesh triangles, and `ponko_debug_style_with_space` puts a
style's 2D points in a `PonkoDrawSpace`. In C++, `ctx.debug()` returns a
`ponko::debug_draw` with `vec2` and `vec3` overloads, `text_depth_tested` and
`set_collider_distance`; `ponko::with_space(style, ponko::draw_space::screen)`
works on debug styles as on draw parameters.

For a 2D shape in screen pixels, initialize a `PonkoDebugStyleEx` with
`struct_size = sizeof(style)` and `version = PONKO_DEBUG_STYLE_EX_VERSION`,
then call `ponko_debug_style_ex_defaults`. Set `style.space` to
`PONKO_DRAW_SPACE_SCREEN` and pass the descriptor to an `_ex` entry point
such as `ponko_ctx_debug_line2d_ex`. The default is
`PONKO_DRAW_SPACE_WORLD`, the same coordinate space sprites use.
In C++, initialize the descriptor with
`ponko::camera_debug_draw::style_ex(ponko::draw_space::screen)` and pass it to a method
such as `ctx.debug().line_ex`. Existing ABI 2 calls continue to read
`PonkoDebugStyle::screen`.
