# Cameras

Following the player, shaking the screen, zooming toward the cursor, pixel art and a minimap. The tests run with frames (`headless_frames(true)`), because cameras are updated every frame.

## Follow the player

A follow camera with a dead zone (the player can move a little without scrolling), a look-ahead and bounds so it never shows past the level's edge. Keep it as a resource (it holds the camera's state) and give the player the [`CameraTarget`](../camera-ui.md#following-something) part: every frame, after everything moved, the engine moves the camera to it, so there is no camera system to write. Something that runs after the camera was placed (a shake, a reading of where it is) goes in `Phase::Late`. The same works in 3D with the [3D rigs](../3d.md#camera-rigs).

C and C++ keep the rig themselves and update it once a frame, after the player moved: `CameraTarget` is Rust-only for now.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_camera.rs:follow}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:follow}}
```

</div>
</div>

## Shake the screen

Trauma goes from 0 to 1, adds up with every hit and wears off by itself. The shake is applied after the camera was placed (with a rig the engine updates, add it in `Phase::Late`), so it never changes where the camera really is.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_camera.rs:shake}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:shake}}
```

</div>
</div>

## Zoom toward the cursor

Zooming around the point under the cursor keeps that point where it was on the screen, which is what feels right when you scroll a map.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_camera.rs:zoom}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:zoom}}
```

</div>
</div>

## Pixel-perfect pixel art

The camera zooms only by whole numbers, the largest that fits, so every art pixel is the same whole number of window pixels.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_camera.rs:pixel_perfect}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:pixel_perfect}}
```

</div>
</div>

## A minimap

Draw in window pixels with `DrawSpace::Screen`, ignoring the camera: a dark rectangle in the corner and a dot for each marker, placed by the ratio of the world to the map. A sprite is drawn around its middle unless you say otherwise, so the recipe sets the pivot to the top left corner to place the map and its dots exactly.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_camera.rs:minimap}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:minimap}}
```

</div>
</div>
