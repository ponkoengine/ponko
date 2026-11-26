# Menus and screens

> **Maturity: [tested](../maturity.md).** Game: none. Tests:
> `tests/cookbook_menus.rs` runs every recipe. Platforms: Linux, macOS and
> Windows, headless (the workspace tests in the list of checks). Graphics: not
> involved.

A pause menu, a title screen, game over, a heads-up display, a volume slider and a panel that slides in. All of them are retained UI (`ctx.ui`) plus game states; the tests click the buttons with an input scenario.

## A pause menu

Escape pushes the `Paused` state, which stops the simulation systems that run only while playing. `state_screen` ties a UI screen to the state: the menu is built each time the game enters `Paused` and taken down when it leaves. Closing the screen leaves the state too: Escape closes it, and so does Resume, a button marked `closes_screen`. So the only code of the game's own is the key that pauses.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_menus.rs:pause}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:pause}}
```

</div>
</div>

## A title screen

A column of buttons. Clicks are events, read once per frame; a button's name is how a system and a test find it, looked up when a click arrives rather than every frame.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_menus.rs:title}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:title}}
```

</div>
</div>

## Game over and play again

Losing the last life changes the state to `GameOver`; Enter changes it to `Playing`. Setting the state you are already in leaves and enters it, so the `on_enter` work that resets the game runs again.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_menus.rs:game_over}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:game_over}}
```

</div>
</div>

## A heads-up display

Labels that are built once and shown with `show_hud` (always on screen, never takes clicks). The text is set only when it changed. The label's id is kept when it is built, so the system that runs every frame looks no name up.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_menus.rs:hud}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:hud}}
```

</div>
</div>

## A volume slider

The slider's value, 0 to 1, is the sound bus's volume. Setting a bus volume affects every sound on it, including the ones already playing.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_menus.rs:volume_slider}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:volume_slider}}
```

</div>
</div>

## A panel that slides in

Move a panel's layout from off the left edge to its place in 0.4 seconds, easing out. The same arithmetic as the tween recipe, applied to a UI layout.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_menus.rs:slide_in}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:slide_in}}
```

</div>
</div>


## Add a developer console

Register `echo` with a required integer argument and give the game a
`show_paths` switch. Press Backquote, enter `echo 42`, then try
`show_paths true`. Read the switch from a drawing system before drawing
debug paths. The [console guide](../control.md#developer-console) shows how
to enable the console in a release build and call it through MCP.

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# use ponko::console::{ConsoleArg, ConsoleType, ConsoleValue};
{{#include ../../../tests/console.rs:developer_console}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:developer_console}}
```

</div>
</div>
