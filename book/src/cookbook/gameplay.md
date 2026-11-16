# Fighting and collecting

Bullets, coins, damage, health bars, waves, enemies and loot. These are plain components and systems; nothing needs physics.

## Shoot bullets

A gun that fires while the key is held, but no faster than its cooldown allows. The bullet is spawned with a command, and `spawn` hands back the new entity at once, so it can be given more components straight away.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:shoot}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:shoot}}
```

</div>
</div>

## Bullets that fly and expire

Each bullet moves along its velocity and removes itself after its time is up. Without that, a game that shoots all day ends up simulating a million bullets that left the screen long ago.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:bullet_life}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:bullet_life}}
```

</div>
</div>

## Pick up coins

The spatial grid files every entity with a `Transform`, so "what is near the player" is a lookup instead of a loop over every coin. Picking up is a despawn and a count.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:pickup}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:pickup}}
```

</div>
</div>

## Damage and death

Anything that hurts sends a `Damage` event; one system keeps the books. A second one sends `Died` and removes whatever ran out of hit points, so other systems (loot, score, sound) can listen for `Died` without knowing who killed what.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:damage}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:damage}}
```

</div>
</div>

## Draw a health bar

A bar is two rectangles: the empty bar and a part of it in proportion to the health. It is drawn in world units, so it follows the unit and the camera.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:health_bar}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:health_bar}}
```

</div>
</div>

## Waves of enemies

When the field is clear, the next wave comes, with one more enemy each time. The positions come from a random stream seeded by the wave number, so every run, and every replay, has the same waves.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:waves}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:waves}}
```

</div>
</div>

## An enemy that chases the player

Step towards the player every tick, and stop at arm's length so the enemy does not walk into the player.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:chase}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:chase}}
```

</div>
</div>

## A guard that patrols and gives chase

A component can be an enum: the state is its value, and the AI is one `match` on it. The guard walks between two points, chases when the player comes near, and starts a new route from where it stands when they leave.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:patrol}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:patrol}}
```

</div>
</div>

## Drop loot when something dies

Each death may leave a coin behind, with a chance. It listens for `Died` from the damage recipe. The luck comes from the game's random stream, so a replay drops the same things.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_gameplay.rs:loot}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:loot}}
```

</div>
</div>
