# In 3D

Clicking a 3D object and walking a character with a third-person camera, with 3D physics (`Game::physics3d`).

## Click a 3D object

A ray from the camera through the pointer, cast into the 3D physics world: the first collider it hits is what you clicked.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_3d.rs:click_3d}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:click_3d}}
```

</div>
</div>

## Walk with a third-person camera

Walk relative to the camera, which follows from behind and comes in front of walls. `flat_forward` and `flat_right` are the camera's directions with the tilt taken out.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_3d.rs:walk_3d}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:walk_3d}}
```

</div>
</div>
