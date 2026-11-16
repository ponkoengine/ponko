# Moving things

Walking, jumping, dashing, clicking to move, dragging and rebinding keys. Each recipe is a system you add with `.sim_system` (or `.system` for frame-rate work); the player is any entity with a `Transform`.

## Walk with the keyboard

Bind WASD and the arrow keys to one action, `move`, once. The walking system reads the action, never a key, so it works unchanged with anything you bind later. Two keys together make a vector longer than 1, so it is capped: diagonals are not faster.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:wasd}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:wasd}}
```

</div>
</div>

## Walk with a gamepad too

The same action takes a second binding: the left stick. Small wobbles around the stick's centre (the dead zone) read as zero. `walk` from the last recipe does not change.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:gamepad}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:gamepad}}
```

</div>
</div>

## Jump

A body with a vertical speed: gravity adds to it every tick, a jump sets it upwards, and the floor stops it. Only a player standing on the floor can jump, so there is no double jump. (With physics, see the platformer recipe.)

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:jump}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:jump}}
```

</div>
</div>

## Dash with a cooldown

A short burst of movement in the direction you are steering, and then a wait. The cooldown is a point in simulation time, so it is the same on every machine and pauses with the game.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:dash}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:dash}}
```

</div>
</div>

## Click to move

Click on the ground and the player walks to the point under the pointer. `mouse_world` is the pointer in world units, whatever the camera is doing. A click on a UI button is not a click on the ground.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:click_to_move}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:click_to_move}}
```

</div>
</div>

## Drag a sprite with the mouse

Pick up whatever is under the pointer, keep the spot where it was grabbed so it does not jump to the pointer's centre, and drop it on release.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:drag}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:drag}}
```

</div>
</div>

## Face the mouse

`atan2` of the vector to the pointer is the angle to turn by. The angle 0 points right, and because y points down, a positive angle turns clockwise on screen.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:face_mouse}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:face_mouse}}
```

</div>
</div>

## Stay on the screen

Clamp the position to the window after everything that moves things has run. The margin is half the sprite's size.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:stay_on_screen}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:stay_on_screen}}
```

</div>
</div>

## Let the player rebind a key

While the prompt is open, the first key, mouse button or pad button that goes down becomes the action's new binding. `pressed_this_frame` lists everything that went down, so the same recipe rebinds to any device.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_moving.rs:rebind}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:rebind}}
```

</div>
</div>
