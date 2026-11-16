# Levels and objects

Loading a level, spawning a prefab, attaching one entity to another and drawing debug shapes.

## Load a level from a Tiled map

`spawn_level` builds tiles and colliders and gives every Tiled object an entity. The recipe turns the objects into game entities by their class: where the hero starts, and each coin.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_world.rs:load_level}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:load_level}}
```

</div>
</div>

## Spawn a prefab

Three crabs from one file, each turned its own way. Components in files must be reflected first; after that the file is data, and `get_mut` changes this copy only.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_world.rs:prefab}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:prefab}}
```

</div>
</div>

## Attach one thing to another

An `Attached` entity follows its parent at an offset and goes when the parent goes. `follow_attachments` runs after the systems that move things.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_world.rs:attach}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:attach}}
```

</div>
</div>

## Draw what the AI is thinking

Circles, arrows and text over the world, for you and not for the player. Shapes last until the system runs again, and only show in debug builds.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_world.rs:debug_draw}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:debug_draw}}
```

</div>
</div>
