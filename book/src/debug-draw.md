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
// For two seconds, over everything, 3 pixels wide.
ctx.debug.sphere(hit, 0.5, Color::GREEN).seconds(2.0).on_top().width(3.0);
// In window pixels: a frame around the minimap.
ctx.debug.aabb(vec2(110.0, 110.0), vec2(200.0, 200.0), Color::WHITE).screen();
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
- `.screen()` (or `.space(DrawSpace::Screen)`): 2D points are window pixels
  from the top-left corner instead of world units.
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
| `Colliders` | 2D and 3D physics colliders (green, sensors yellow); a 3D collision mesh shows its bounding box |
| `NavMesh` | the navmesh's polygon edges |
| `Bounds` | 3D models' bounding spheres and 2D sprites' rectangles |
| `Lights` | the reach of 2D and 3D point and spot lights |
| `Hierarchy` | the axes of each attached entity, and a line to what it is attached to |

## At runtime and from tools

`ctx.debug.set_enabled(on)` turns it on or off while the game runs (off forgets
everything drawn). `Game::debug_draw(on)` chooses before the game starts,
overriding the build's default. A running game with the control endpoint
answers `debug.draw` (`on`, `views`, `clear`; see
[the control endpoint](control.md)), and its MCP server has the `debug_draw`
tool, so an agent can show the colliders before taking a screenshot.

## Cost

All of a frame's 3D lines are one buffer and two draw calls (the depth-tested
ones, then the ones on top), expanded into quads on the GPU. 2D lines are
sprites in one batch, and labels are screen text. Buffers keep their capacity,
so steady drawing doesn't allocate.

Handing 100,000 lines to the renderer costs about 0.5 ms a frame for 3D lines
and 0.6 ms for 2D lines on a laptop (`cargo run --release -p ponko_bench --
debug/`), measured while the machine was busy with other work. Turned off, the
100,000 calls cost about 0.1 ms.

Notes:

- 3D lines are drawn in the lit scene, after particles, so exposure and tone
  mapping apply to their colors as to an unlit surface.
- Labels are always drawn on top, and a 3D point's label hides while the point
  is behind the camera.
- A headless game records shapes but draws nothing.

## From C and C++

The C functions are `ponko_ctx_debug_line2d`, `ponko_ctx_debug_line3d` and
the like (`ray`, `arrow`, `box`, `circle2d`, `sphere3d`, `capsule`,
`polyline`, `grid`, `frustum3d`, `text`, `axes3d`), each with a
`PonkoDebugStyle*` (null for the defaults, or from
`ponko_debug_style_default()`), plus `ponko_ctx_debug_set_enabled`,
`ponko_ctx_debug_show` with a `PONKO_DEBUG_VIEW_*`, `ponko_ctx_debug_clear` and
`ponko_ctx_debug_stats`. In C++, `ctx.debug()` returns a `ponko::debug_draw`
with `vec2` and `vec3` overloads.
