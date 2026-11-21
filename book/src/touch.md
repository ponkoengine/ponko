# Touch: gestures and on-screen controls

Fingers are input like keys and buttons. On a phone, a tablet or a touch
screen in the browser, the window feeds every finger to `ctx.input`; a
headless test feeds the same events by hand or from an
[input scenario](actions.md#input-scenarios). From the touches the engine
recognizes gestures, which bind to [named actions](actions.md) like keys,
and it can put a stick and buttons on the screen that drive actions while a
finger holds them. A game written for keys and a gamepad becomes playable by
touch by adding gesture sources to its actions and a line of on-screen
controls.

## Gestures are action sources

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# use ponko::input::GestureAxis;
# use ponko::ui::touch::TouchControls;
# fn setup(ctx: &mut Ctx) {
// A tap fires, a pinch zooms and a drag pans, next to the keys. On a touch
// screen a stick walks, a round button fires and a corner button pauses.
ctx.input.set_action(
    "fire",
    Action::Button(vec![ButtonBinding::Key(Key::Space), ButtonBinding::Gesture(Gesture::Tap)]),
);
ctx.input.set_action(
    "zoom",
    Action::Axis(vec![AxisBinding::Gesture { axis: GestureAxis::Pinch, scale: 1.0 }]),
);
ctx.input.set_action("pan", Action::Axis2(vec![Axis2Binding::Drag { scale: 1.0 }]));
ctx.input.set_action(
    "move",
    Action::Axis2(vec![Axis2Binding::Keys { left: Key::A, right: Key::D, up: Key::W, down: Key::S }]),
);
ctx.input.set_touch_controls(TouchControls::new().stick("move").button("fire", "Fire").corner_button("pause", "II"));
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:touch}}
```

</div>
</div>

| Gesture | When it is down |
| --- | --- |
| `Tap` | one frame, when a short touch lifts near where it started |
| `DoubleTap` | one frame, for a second tap soon after a tap and near it (instead of a `Tap`) |
| `LongPress` | from when a touch has been held still long enough until it lifts |
| `Drag` | from when one finger moved farther than a tap may until it lifts |
| `Pinch`, `Rotate` | from when two fingers spread or turned past a threshold until one lifts |
| `SwipeLeft`, `SwipeRight`, `SwipeUp`, `SwipeDown` | one frame, when a quick, long enough drag lifts |

A gesture bound to a button action makes `action_pressed`, `action_down`
and `action_released` work as for a key. `AxisBinding::Gesture` reads a
gesture's amount this frame (`GestureAxis::Pinch`: how much the fingers
spread, `0.1` for 10 % farther apart; `Rotate`: radians, clockwise
positive; `DragX` and `DragY`: pixels), and `Axis2Binding::Drag` a
one-finger drag's movement in pixels, each times its `scale`. Like the
mouse wheel these are changes: zero in a frame where nothing moved. Gesture
bindings are saved in the action map file like any other, and
`ctx.input.pressed_this_frame()` reports a gesture that just happened, so a
rebinding screen can bind a swipe as easily as a key.

The thresholds are `GestureSettings` (`ctx.input.set_gesture_settings`):
distances in window pixels, times in seconds of game time, so a headless
test recognizes exactly what a phone does. The defaults: a tap moves at
most 10 pixels and lasts at most 0.3 s; a double tap follows within 0.3 s
and 40 pixels; a long press holds for 0.5 s; a swipe covers at least 50
pixels within 0.5 s; a pinch starts at 16 pixels of change and a rotation
at 0.2 radians. A pinch or a rotation counts from where the fingers
started, so the frame it is recognized in reports all of the change so far.

To read the fingers themselves: `ctx.input.touches()` lists each one (id,
position, start, movement this frame, and whether an on-screen control
holds it), `gesture_events()` lists this frame's gestures with where they
happened, and `drag_delta()`, `pinch_scale()` and `rotation_delta()` give
this frame's change.

## On-screen controls

`ctx.input.set_touch_controls` puts a set of controls on the screen: they
take fingers at once and are drawn from the next frame. A stick sits
in a bottom corner and drives a vector action (or a scalar one, from its
sideways movement); a round button sits at the bottom right and a small
corner button at the top right, each holding a button action down while a
finger is on it. The controls lay themselves out from the window's size
and the UI scale, so they keep their place on a phone held either way. This
is Coin Dash's set:

```rust
{{#include ../../examples/coin_dash/game.rs:touch}}
```

- **Touch devices only.** By default (`TouchShow::Auto`) the controls show
  when the platform has a touch screen (in the browser, when it reports
  touch points) or once a finger has been seen, and stay hidden on a
  desktop played with a mouse. `TouchShow::Always` and `Never` override it
  (`ctx.input.set_touch_controls_show`, for a settings option), and
  `ctx.input.set_touch_device` says which it is. An empty `TouchControls`
  takes the controls away, and each control's node is named
  `touch.<action>` for `ctx.ui.find`.
- **A finger on a control is the control's.** It makes no gesture and does
  not press the UI under it. Fingers elsewhere still tap, swipe and pinch,
  so a stick under one thumb and a tap to fire with the other work at once.
- **Menus come first.** While a UI screen is open the controls step aside
  so its buttons take taps (unless `over_screens(true)`); the first free
  finger presses UI buttons as the mouse does. A control that makes sense
  only on some screens is hidden with `ctx.input.set_touch_control_shown`: Coin
  Dash shows its pause button only during a round.
- **Assistive technology** sees each button as a button named by its label
  and each stick as a group named after its action, in the
  [accessibility tree](ui-accessibility.md).
- **The mouse never presses them**, so a desktop with `TouchShow::Always`
  still plays with the keyboard and mouse.

Keeping a held control, the knob following a finger and the highlight of a
held button allocate nothing once the controls are placed.

## Testing by touch

`InputScenario::tap`, `touch_hold` and `touch_drag` script fingers by tick,
and the `touch` step is in the scenario file. `ctx.input.touch_control_center`
says where a control is, which is where a test puts its finger. This test
plays a round of Coin Dash by touch alone, with the stick chasing the coins:

```rust
{{#include ../../tests/coin_dash.rs:touch_test}}
```

A system that feeds touches runs after the UI read this frame's input, so a
tap on a menu button belongs in a scenario (fed before the frame, as a
window's events are); fingers on the on-screen controls and gestures work
from a system, as above. Each reference game has such a test.

## In the browser

The web build turns off the browser's own touch scrolling and zooming on
the game's canvas (`touch-action: none`), so every finger reaches the game.
Whether the controls feel right under a thumb is something only a person
with a phone can tell; the tests prove what the fingers do, not how it
feels.

## C and C++

C feeds fingers with `ponko_ctx_feed_touch` (and `ponko_input_scenario_touch`),
binds gestures with the `PONKO_ACTION_GESTURE`, `PONKO_ACTION_GESTURE_AXIS` and
`PONKO_ACTION_DRAG` sources (the binding's `deadzone` field holds the scale,
0 meaning 1), reads `ponko_ctx_touches`, `ponko_ctx_gesture_events` and
`ponko_ctx_gesture_deltas`, and sets controls with
`ponko_ctx_set_touch_controls` and its neighbours; C++ wraps them as
`action_binding::gesture`, `gesture_axis` and `drag`, `ponko::touch_controls`
and `ctx.feed_touch`. The scenario builders are
`ponko_input_scenario_tap`, `_touch_hold` and `_touch_drag` (C++
`tap`, `touch_hold` and `touch_drag`), and write the same steps as Rust's:

```cpp
{{#include ../../examples/cpp/guide.cpp:touch_scenario}}
```
