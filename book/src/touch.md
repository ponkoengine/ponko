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
  still plays with the keyboard and mouse (it only moves them while the
  player [arranges them](#players-arrange-the-controls)).

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

A system can feed fingers, clicks, keys and text too (a bot, a replay, a
test): what it feeds reaches the retained UI in the same frame. The UI
takes the frame's input before the first system; when a system feeds more,
the engine runs the UI again before the next system, over only what is
new, so the next system already sees the button clicked, the field
focused, the screen closed. A click fed by the last system of a frame is
read by the next frame's systems, once. The second pass moves no
animation and lays out only what the input changed. Each reference game
has a touch test.

## Restyling and placing the controls

A game gives its controls its own look with a `TouchStyle`: the colors of
a stick's base and knob, of a button idle and held and of its label, an
image of its own in place of the engine's disc (drawn white and tinted by
those colors: a square, a ring, a pictogram), and a size. `style` sets it
for every control, `control_style` for one, and `place` puts a control
where the game wants it instead of where the engine lays it out: a
`TouchPlace` is a point of the view (`(0, 0)` top left to `(1, 1)` bottom
right) and an offset from it in UI units, so a control anchored to a
corner keeps its distance from it when the phone turns.

```rust
# use ponko::prelude::*;
# use ponko::ui::touch::{TouchControls, TouchPlace, TouchStyle};
# fn setup(ctx: &mut Ctx) {
let warm = TouchStyle { button: Color::rgba(1.0, 0.8, 0.5, 0.35), size: 1.2, ..TouchStyle::default() };
let attack = TouchStyle { button: Color::rgba(1.0, 0.2, 0.2, 0.45), held: Color::rgba(1.0, 0.4, 0.4, 0.85), ..warm };
ctx.input.set_touch_controls(
    TouchControls::new()
        .stick("move")
        .button("jump", "A")
        .button("attack", "B")
        .style(warm)
        .control_style("attack", attack)
        // The attack button 120 units in from the bottom right corner.
        .place("attack", TouchPlace::new(vec2(1.0, 1.0), vec2(-120.0, -120.0))),
);
# }
```

A control placed or sized so that it would stick out of the view is moved
in.

## Players arrange the controls

Thumbs differ, and so do phones: players move and resize the controls, and
the game keeps their arrangement with its settings.

- **Moving.** `ctx.input.edit_touch_controls(true)` (a settings screen's
  "arrange controls") shows the controls, also under a UI screen and on a
  desktop, and a finger or the mouse that drags one moves it; it presses no
  action and clicks nothing under it. Where it is let go it stays,
  anchored to the nearest corner of the view. `edit_touch_controls(false)`
  ends it. A see-through screen with a "Done" button over the controls is
  the usual way to show it, or [the engine's own](#the-ready-made-arrange-screen).
- **Sizing and opacity.** The player's arrangement is a `TouchLayout`:
  each control's place and size (by the action it drives), and the size
  and opacity of them all. A settings screen's sliders change it with
  `ctx.input.set_touch_layout`; `TouchLayout::default()` resets it.
- **Saving.** `ctx.input.save_touch_layout(&mut ctx.storage)` writes it to
  `touch_controls.json` in the player's [storage](storage.md), next to the
  window's settings, and `restore_touch_layout(&ctx.storage)` reads it back
  at setup (no file: nothing changes). `TouchLayout::to_json` and
  `from_json` are there for a settings file of the game's own.

```rust
# use ponko::prelude::*;
# fn setup(ctx: &mut Ctx) {
// At setup: the player's arrangement from last time.
ctx.input.restore_touch_layout(&ctx.storage).ok();
# }
# fn arrange_done(ctx: &mut Ctx) {
// When the player leaves the arrange screen:
ctx.input.edit_touch_controls(false);
if let Err(e) = ctx.input.save_touch_layout(&mut ctx.storage) {
    eprintln!("could not save the touch layout: {e}");
}
# }
```

The game's own placement stays the default under the player's: a control
the player never moved follows the game, and a new set of controls keeps
the places the player gave the actions it has.

### The ready-made arrange screen

Most games want the same screen for this, so the engine has one:
`ctx.input.arrange_touch_controls()` opens it, from a settings page's "Arrange
controls" button. It is built with the [retained UI](ui.md) over the edit
mode and the player's `TouchLayout` above, and works with a finger and
with the mouse:

```rust
# use ponko::prelude::*;
# fn settings_page(ctx: &mut Ctx) {
let arrange = ctx.ui.find("settings.arrange");
while let Some(event) = ctx.ui.next_event() {
    if arrange.is_some() && event.clicked() == arrange {
        // The controls show (on a desktop too), with a small panel beside them.
        ctx.input.arrange_touch_controls();
    }
}
# }
```

- **Move** a control by dragging it; it stays where it is let go,
  anchored to the nearest corner of the view.
- **Resize** one by tapping or clicking it (it lights up) and pressing `-`
  or `+`, 10 % a press, from half to twice its size. `Size` makes them all
  bigger or smaller, `Opacity` fainter.
- **Reset** puts every control back where the game puts it.
- **Save** writes the arrangement to the player's storage
  (`save_touch_layout`, so `restore_touch_layout` at setup finds it) and
  closes the screen. **Cancel**, Escape, a gamepad's East button or the game
  closing the screen put back the arrangement it had when it opened.

The panel goes where it hides the fewest controls (the middle of the
view, else its top, bottom or a side), steps aside while a control is
dragged, and fits a phone held either way: it is 240 UI units tall and at
most 360 wide. A press on it is the panel's, even over a control.
However the screen closes, edit mode ends and `UiEvent::ScreenPopped` is
queued with its root (`ctx.ui.find("touch.arrange")` while it is open). Its nodes
are named `touch.arrange.<part>` (`title`, `note`, `size_label`,
`opacity_label`, `reset`, `cancel`, `save`, ...): give them the game's own
words with `ctx.ui.set_text` or [`set_text_key`](localization.md) after
opening it. The panel stays inside the window's
[safe area](#notches-home-bars-and-rounded-corners), 12 units from its
edges.

The test that drives it with scripted fingers, as a phone would:

```rust
{{#include ../../tests/touch_arrange.rs:fingers}}
```

## Notches, home bars and rounded corners

A phone's notch or camera hole, its home bar and its rounded corners
cover parts of the screen. `ctx.safe_area()` is the part nothing covers,
a `Rect` in the same logical pixels as `ctx.window_size()`, and
`ctx.safe_insets()` says how far in from each edge the covered parts
reach. Where nothing covers the window (desktops), it is the whole
window.

- **The engine's controls stay inside it.** On-screen controls are laid
  out in the safe area, a `TouchPlace` counts from its corners, a control
  the player drags stops at its edge and is kept anchored to its nearest
  corner, and the [arrange screen](#the-ready-made-arrange-screen)'s panel
  stays in it. When the phone turns, the safe area moves and they follow.
- **The game's UI chooses.** A UI root made with `.inside_safe_area()`
  (or `ctx.ui.set_inside_safe_area(root, true)`) is laid out in the safe
  area: its sizes, fractions, anchors and `auto` margins count from the
  safe area's edges, so a HUD in a corner keeps clear of the notch and a
  centred dialog is centred in what the player sees. Roots are laid out in
  the whole window by default, so a backdrop still reaches under the notch.

```rust
# use ponko::prelude::*;
# fn setup(ctx: &mut Ctx) {
// A backdrop under everything, the notch too.
let backdrop = ctx.ui.root().layout(UiLayout::fill()).id();
ctx.ui.show_hud(backdrop);
// The score in the top left corner of what the player can see.
let hud = ctx.ui.root().layout(UiLayout::fill()).inside_safe_area().id();
ctx.ui.label(hud, "Score 0").layout(UiLayout::fixed(120.0, 30.0));
ctx.ui.show_hud(hud);
# }
```

- **Where it comes from.** In the browser the engine reads the page's
  `env(safe-area-inset-*)`, at start and whenever the window changes size
  (turning the phone does). A browser reports insets only for a page that
  asks to draw under the notch, with `viewport-fit=cover` in its viewport
  `<meta>` tag; without it the browser keeps the whole page clear of the
  notch, and the safe area is the whole canvas. Desktop windows have no
  insets (a full-screen window on a Mac with a notch is placed below it).
  Ponko has no iOS or Android builds yet.
- **Testing.** `ctx.feed_safe_insets(SafeInsets::new(top, right, bottom, left))`
  feeds the insets a phone would report, as a test does, and so can a
  platform layer of the game's own. The UI lays out with them from the
  next frame.

```rust
# use ponko::prelude::*;
use ponko::window::SafeInsets;
Game::new("notch", 844, 390).run_headless(1, |ctx| {
    // A phone held sideways: the notch on the left, the home bar below.
    ctx.feed_safe_insets(SafeInsets::new(0.0, 0.0, 21.0, 47.0));
    assert_eq!(ctx.safe_area(), Rect::new(47.0, 0.0, 797.0, 369.0));
});
```

What needs a person: the tests feed the insets a phone reports and check
that the controls, the panel and the game's UI keep out of them; that a
given phone's browser reports its notch, and that the controls sit
comfortably beside it, only someone with that phone can see. No real
notch has been tested yet.

## Touch screens on desktops

The window's touches are the same on every system that has them, and so
are the gestures and on-screen controls:

| Desktop | Touches and gestures | Counts as a touch device before the first finger |
| --- | --- | --- |
| Windows | yes (the window takes the system's touch messages) | yes, when the system reports touch points |
| Linux, X11 and Wayland | yes (XInput 2 multitouch on X11, `wl_touch` on Wayland) | yes, when the kernel lists a direct multitouch device |
| macOS | no: Macs have no touch screens | no |

- **Mouse events made up for fingers.** Windows may follow a finger's
  touches with a made-up mouse press and release, and X11 moves the cursor
  with the first finger. The engine drops a mouse press at a finger (or
  where one lifted this frame or the last), and the cursor moving with a
  finger doesn't take the UI's pointer from it, so a tap clicks a button
  once and the prompts don't switch to the keyboard and mouse.
- **A Mac's trackpad** is not a touch screen (its fingers have no place on
  the screen), but its pinch, rotation and two-finger double tap feed the
  same gestures as two fingers would: `Gesture::Pinch`, `Rotate` and
  `DoubleTap`, the pinch and rotation amounts, at the pointer. It doesn't
  make the Mac a touch device, so on-screen controls stay hidden. Tests and
  replays feed `InputEvent::Trackpad` (the `trackpad` scenario step), and
  recordings keep fingers and trackpad gestures.
- **What needs a person.** The tests feed the events each system sends, in
  the order it sends them; whether a given touch screen and its driver
  send them is something only someone with one can see.

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

Styles, placement, the player's layout and its edit mode, the ready-made
arrange screen (`arrange_touch_controls`), the safe area (`safe_area`,
`feed_safe_insets`, `inside_safe_area`), and trackpad events are
Rust-only so far.
