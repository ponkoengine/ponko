# Sound and saves

> **Maturity: [tested](../maturity.md).** Game: none. Tests:
> `tests/cookbook_sound_and_saves.rs` runs every recipe. Platforms: Linux,
> macOS and Windows, headless (the workspace tests in the list of checks).
> Graphics: not involved.

Footsteps, a sound for an event, saving and loading a game, settings and a best score, and saving on quit. A headless run opens no audio device, but it still lists the sounds each tick started (`ctx.audio.started()`, `ctx.sounds_started()` in C++), so the tests check what was played; `ctx.quit_requested()` says whether the game asked to quit. The saves go to a folder of the test's own.

## Footsteps

A step sound every 40 pixels walked, a little different in pitch each time so it does not sound like a machine.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_sound_and_saves.rs:footsteps}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:footsteps}}
```

</div>
</div>

## A sound for an event

One sound per pickup, but never more than one every 50 milliseconds: ten coins in the same tick would only be louder, not better.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_sound_and_saves.rs:sound_on_event}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:sound_on_event}}
```

</div>
</div>

## Save and load the game

F5 saves and F9 loads a named slot. Only reflected components go in a save. A failed save or load is reported and the game goes on.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_sound_and_saves.rs:save_game}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:save_game}}
```

</div>
</div>

## Keep settings and a best score

A struct that is saved as JSON, with defaults for a missing file or field, so a first run and an update both work. The write is safe against crashes.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_sound_and_saves.rs:settings}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:settings}}
```

</div>
</div>

## Save and quit

Save first, and quit only if that worked, so a failed save is never lost along with the game.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_sound_and_saves.rs:save_on_quit}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:save_on_quit}}
```

</div>
</div>
