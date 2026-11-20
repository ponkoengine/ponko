# Input: keyboard, mouse, gamepads

Input is read from `ctx` in any system. Every kind works the same way:
`_down` while held, `_pressed` on the frame it went down, `_released` on the
frame it came up. A tap shorter than a frame still shows up as pressed and
released.

## Keyboard

```rust
# use ponko::prelude::*;
# fn update(ctx: &mut Ctx, pos: &mut Vec2) {
if ctx.input.key_down(Key::Right) { pos.x += 200.0 * ctx.time.dt(); }
if ctx.input.key_pressed(Key::Space) { println!("jump"); }
# }
```

Keys are named by their **position** on a US keyboard: `Key::W` is the key
above `S` on every layout, so WASD controls stay in place for AZERTY and
Dvorak players too. In C++: `ctx.key_down(PONKO_KEY_RIGHT)`.

## Mouse

- `ctx.input.mouse_pos()`: in window pixels; `ctx.mouse_world()`: through the
  [camera](camera-ui.md), in world units.
- `ctx.input.mouse_down(MouseButton::Left)` (and `_pressed`, `_released`).
- `ctx.input.mouse_delta()` since the last frame, and `ctx.input.mouse_wheel()`.
- `ctx.ui.wants_pointer()`: `true` over a UI panel or the
  [inspector](inspector.md), so a click on a button doesn't also act in the
  world.

### Mouse look

For a first-person or orbit camera, lock the cursor and read raw motion:

- `ctx.input.set_cursor_locked(true)` hides the cursor and holds it in the
  window (where the platform can't hold it in place, it's kept inside the
  window instead); `false` lets it go. It takes effect after the frame and
  comes back when the window regains focus.
- `ctx.input.mouse_motion()`: how far the mouse moved this frame in raw device
  units (+y is down), even while the cursor is locked and can't move.
  `mouse_delta()` stops at the window's edge; this doesn't.

C++: `ctx.input.set_cursor_locked(true)`, `ctx.input.mouse_motion()`. See
[3D physics](physics3d.md) for a walker that uses both.

## Touch

Fingers on a touch screen, the gestures they make (taps, swipes, pinches)
and on-screen sticks and buttons have their own chapter:
[Touch: gestures and on-screen controls](touch.md).

## Gamepads

Xbox, PlayStation, Switch and most other controllers work, plugged in or
over Bluetooth, and can come and go while the game runs.
`ctx.input.gamepads()` lists the connected ones. Each keeps its **slot** while
connected (`pad.slot()`: 0 is player one). A pad that is unplugged gets the
same slot back when it returns, and a new pad takes the lowest free slot
nobody is waiting for. Rumble, button prompts, gyro and assigning players
are in [Controllers](controllers.md).

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/gamepad.rs:gamepad}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:gamepad}}
```

</div>
</div>

Buttons are named by position, so a game's controls mean the same on every
brand:

| `GamepadButton` | Xbox | PlayStation | Nintendo |
|---|---|---|---|
| `South` | A | Cross | B |
| `East` | B | Circle | A |
| `North` | Y | Triangle | X |
| `West` | X | Square | Y |
| `LeftBumper` / `RightBumper` | LB / RB | L1 / R1 | L / R |
| `LeftTrigger` / `RightTrigger` | LT / RT | L2 / R2 | ZL / ZR |
| `Select` / `Start` | View / Menu | Share / Options | Minus / Plus |
| `LeftStick` / `RightStick` | stick clicks | L3 / R3 | stick clicks |
| `DPadUp`, `DPadDown`, `DPadLeft`, `DPadRight` | d-pad | d-pad | d-pad |

Sticks and triggers are axes:

- `ctx.input.gamepad_stick(pad, GamepadStick::Left)` is a `Vec2` from `-1` to `1`,
  **y down** like screen coordinates, so adding it to a position moves that
  way. Small wobbles around the center read as zero.
- `ctx.input.gamepad_axis(pad, GamepadAxis::RightTrigger)` goes from `0` (released)
  to `1` (fully pulled): use it for throttles and charge-ups. The triggers
  also count as buttons once pulled past halfway.

`examples/gamepad.rs` gives every connected pad its own crab:
`cargo run --example gamepad`.

Gamepads need the `gamepad` cargo feature, on by default. Without it, or in
[headless runs](headless.md), `ctx.input.gamepads()` is empty unless a test
installs a backend such as `FakeGamepads` ([Controllers](controllers.md)). On Linux, building
it needs the udev headers (`sudo apt-get install libudev-dev`).

## What was just pressed

`ctx.input.pressed_this_frame()` lists every key, mouse button, gamepad button
and gamepad axis that went down this frame, as bindings an `ActionMap`
accepts: the query behind a rebinding screen. See
[Named input actions](actions.md#rebinding-screens-what-was-just-pressed).

## Feeding synthetic button input

Use the named held state when feeding headless tests or replay input:

```rust
# use ponko::prelude::*;
use ponko::actions::{ButtonState, InputEvent};
use ponko::input::{Gamepad, GamepadButton};
# fn example(ctx: &mut Ctx) {
ctx.input.feed(InputEvent::key(Key::Space, ButtonState::Held));
ctx.input.feed(InputEvent::mouse(MouseButton::Left, ButtonState::Released));
let pad = Gamepad::from_slot(0).unwrap();
ctx.input.feed(InputEvent::gamepad_button(pad, GamepadButton::South, ButtonState::Held));
# }
```

`Held` persists until `Released` (or focus/device loss). The transition to
held generates a pressed-this-frame edge; repeated `Held` events do not repeat
it. Edges clear at frame end, while held state remains. A press and release
within one frame preserve both edges. The existing boolean event variants
remain available (`true` means held). `InputEvent` supports `PartialEq` for
sequence assertions; axis values use ordinary float equality, so NaN is unequal
to itself. In C/C++, the existing input injection calls keep their boolean
held arguments and the same edge behavior.
