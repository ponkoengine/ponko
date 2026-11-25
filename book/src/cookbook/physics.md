# Bodies and collisions

> **Maturity: [tested](../maturity.md).** Game: none. Tests:
> `tests/cookbook_physics.rs` runs every recipe. Platforms: Linux, macOS and
> Windows, headless (the workspace tests in the list of checks). Graphics: not
> involved.

Damage on contact, a knockback, line of sight, clicking a body and a platformer's walk and jump, with 2D physics (`Game::physics`).

## Damage on contact

Physics sends `CollisionStarted` once when two colliders begin to touch. Look at both orders of the pair, because you do not know which one is the hazard.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_physics.rs:contact_damage}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:contact_damage}}
```

</div>
</div>

## Knock something back

An impulse is mass times the speed you want, so the same call works on a crate and on a crab.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_physics.rs:knockback}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:knockback}}
```

</div>
</div>

## Line of sight

Cast a ray from the looker to the target, ignoring the looker itself. If the first thing it hits is the target, nothing is in between.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_physics.rs:line_of_sight}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:line_of_sight}}
```

</div>
</div>

## Click a body to select it

Ask the physics world what is under the pointer; the first hit is the topmost. A click on empty ground selects nothing.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_physics.rs:click_body}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:click_body}}
```

</div>
</div>

## A platformer player

Set the horizontal speed from the input and leave gravity to the physics. "On the ground" is a short ray down from the feet. It runs before the physics step, so the new speed is what the step uses.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_physics.rs:platformer}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:platformer}}
```

</div>
</div>
