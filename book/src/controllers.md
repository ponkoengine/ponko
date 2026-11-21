# Controllers: rumble, prompts, motion, players

[Input](input.md) covers reading buttons and sticks. This chapter covers the
rest of what a game does with controllers: shaking them, showing the right
button in a prompt, reading a gyro, and sorting out who is player two. All
of it is optional and costs nothing when unused; none of it allocates per
frame.

## Which controller is it

When a pad connects, its backend describes it:
`ctx.input.gamepad_info(pad)` gives its name, USB vendor and product ids
(where the platform reports them), what it can do (`capabilities.rumble`,
`trigger_rumble`, `motion`) and its **family**: `ControllerFamily::Xbox`,
`PlayStation`, `Nintendo` or `Generic`. The family is guessed from the
vendor id, then from words in the name
(`ControllerFamily::detect`);
an unknown make is `Generic`. A game can offer a "button style" option with
`ctx.input.set_gamepad_family(pad, family)`.

`ctx.input.gamepads_connected()` and `gamepads_disconnected()` list the pads
that came or went this frame: the hook for a "controller disconnected"
pause screen.

## Rumble

```rust
# use ponko::prelude::*;
use ponko::input::Rumble;
# fn on_hit(ctx: &mut Ctx, pad: ponko::input::Gamepad) {
// Low-frequency motor hard, high-frequency motor softer, for a quarter second.
ctx.input.rumble(pad, Rumble::new(0.8, 0.3).seconds(0.25));
// A trigger kick, where the controller has trigger motors.
ctx.input.rumble(pad, Rumble::new(0.0, 0.2).triggers(0.0, 1.0).seconds(0.1));
# }
```

`strong` is the low-frequency motor (usually the left grip), `weak` the
high-frequency one. Each value is `0..=1`; a new request replaces the old
one. Time is real time, so rumble ends on schedule while the game is paused
or slowed; `Rumble::seconds(f32::INFINITY)` plays until
`ctx.input.stop_rumble(pad)`. Rumble stops when the window loses focus. The
engine sends the backend a change only when the levels change, not every
frame. `ctx.input.rumble_levels(pad)` reads what is playing.

## Prompts that follow the player

A prompt should show the button the player will press: `Space` after
keyboard input, `A` on an Xbox pad, the cross on a PlayStation pad.
`ctx.input.last_device()` is the device used last (a key or mouse button
pressed, the mouse moved or scrolled, a pad button pressed or a stick or
trigger pushed past halfway; resting sticks and a nudged mouse don't count),
and `ctx.input.action_glyphs("jump")` turns an action's binding into
`Glyph`s for that
device, labelled for the pad's family. `last_device_changed()` says when to
redraw.

`GlyphAtlas::new(cell)` draws every glyph (all keys, the mouse buttons,
each family's buttons, sticks and triggers) into one RGBA image with the
engine's own simple shapes: keycaps, a mouse, round face buttons with
letters or geometric symbols, shoulder buttons, sticks and a d-pad. There is
no platform artwork in it. Make it once, then draw prompt rows:

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
use ponko::input::GlyphAtlas;

fn prompts(ctx: &mut Ctx, atlas: &mut GlyphAtlas) {
    let jump = ctx.input.action_glyphs("jump");
    let fire = ctx.input.action_glyphs("fire");
    let x = 16.0;
    let width = atlas.draw_prompt(&mut ctx.gfx, vec2(x, 16.0), 32.0, &jump, "Jump", Color::WHITE);
    atlas.draw_prompt(&mut ctx.gfx, vec2(x + width + 24.0, 16.0), 32.0, &fire, "Fire", Color::WHITE);
}
# Game::new("t", 1, 1).run_headless(1, |ctx| {
#     let mut atlas = GlyphAtlas::new(32);
#     prompts(ctx, &mut atlas);
# });
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:controllers}}
```

</div>
</div>

Other ways to use it:

- `atlas.region(glyph)` is the glyph's cell for `DrawParams::with_region`
  with `atlas.texture(&mut ctx.gfx)`; `atlas.rgba()` is the whole image.
- `atlas.inline_image(&mut ctx.gfx, &mut ctx.ui, glyph)` names one glyph for
  [rich text](ui.md) and returns its markup, `[img=pad.xbox.south alt=A]`:
  "Press [A] to talk".
- `glyphs.to_string()` (`"W/A/S/D"`, `"Cross"`) and `Glyph::label()` are
  text-only prompts and what a screen reader says.

An action with no binding on the device used last gives no glyphs: bind
both, or show the label alone. Keys are labelled by their position on a US
keyboard, like `Key` itself, so an AZERTY player sees `W` for the key that
reads `Z`.

The reference image of the GPU test `prompt_row_render_test` shows one row
per device kind: keyboard and mouse, then the Xbox, PlayStation, Nintendo
and generic families.

## Motion: gyro and accelerometer

`ctx.input.gamepad_motion(pad)` is the latest gyro (radians per second) and
accelerometer (m/s², gravity included) reading, or `None` when the pad has
sent none. Axes are the pad's held flat: x right, y up, z toward the player.
`motion.aim_delta(dt)` turns it into a yaw/pitch change for gyro aiming.

The built-in backend reads no motion sensors (see the table below), so
motion arrives only from a backend that can read them, from
`InputEvent::GamepadMotion` in tests and replays, and from the scenario step
`gamepad_motion`.

## Local multiplayer: slots and players

Each pad has a **slot** while connected. A pad that is unplugged leaves its
slot waiting for it: when the same device comes back it gets that slot
again, and a new pad takes the lowest slot nobody is waiting for (a waiting
slot is only given away when no other is free). So "pad 2 ran out of
battery" doesn't reshuffle everyone.

Players are separate from slots, so a game decides who plays with what:

```rust
# use ponko::prelude::*;
use ponko::input::{GamepadButton, InputDevice};

fn join_screen(ctx: &mut Ctx) {
    // "Press Start to join": a pad no player holds.
    if let (Some(pad), Some(player)) = (ctx.input.join_pressed(GamepadButton::Start), ctx.input.free_player()) {
        ctx.input.assign_player(player, InputDevice::Gamepad(pad));
    }
    if ctx.input.key_pressed(Key::Enter) && ctx.input.player_of(InputDevice::KeyboardMouse).is_none() {
        if let Some(player) = ctx.input.free_player() {
            ctx.input.assign_player(player, InputDevice::KeyboardMouse);
        }
    }
    for player in 0..ponko::input::MAX_PLAYERS {
        if ctx.input.player_device(player).is_some() && !ctx.input.player_connected(player) {
            // "Player {player + 1}: reconnect your controller"
        }
    }
}
# Game::new("t", 1, 1).sim_system(join_screen).run_headless(1, |_| {});
```

A player keeps a pad while it is unplugged and when it comes back.
`ctx.input.action_glyphs_for("jump", device)` gives a player's own prompts.
For per-player actions, write the bindings once for slot 0 and install a
copy per player with `Action::for_pad`:
`ctx.input.set_action("jump.p2", jump.for_pad(pad))`.

## Backends and tests

Pads come from a `GamepadBackend`. In a window the engine reads them with
[gilrs](https://crates.io/crates/gilrs) (feature `gamepad`); headless runs
have none. `Game::new(...).gamepad_backend(backend)` chooses another, polled
once before setup so its pads are already connected there. A backend reports connections, buttons, axes and motion
through `GamepadEvents` at the start of each frame and is told motor levels
when they change; it gives each physical device a `DeviceKey` and reports
the same key when the device returns.

`FakeGamepads` is a scripted backend for tests: connect pads with any name,
family and capabilities, press buttons, move sticks, send motion, unplug and
replug, and read back the rumble the game asked for.

```rust
# use ponko::prelude::*;
use ponko::input::{FakeGamepads, GamepadInfo, Rumble};
let fake = FakeGamepads::new();
let pad = fake.connect(GamepadInfo::new("Wireless Controller").ids(0x054c, 0x09cc));
Game::new("t", 1, 1).gamepad_backend(fake.clone()).run_headless(2, |ctx| {
    let slot = ctx.input.gamepads()[0];
    assert_eq!(ctx.input.action_glyphs("jump").len(), 0); // no actions installed
    ctx.input.rumble(slot, Rumble::new(1.0, 0.0).seconds(1.0));
});
assert_eq!(fake.rumble(pad).strong, 1.0);
```

Without a backend, `InputEvent::GamepadConnected(pad)` and
`InputEvent::GamepadMotion` (scenario steps `gamepad_connected` and
`gamepad_motion`) drive hot-plug and motion in replays.

### What each platform supports

What the built-in backend can do depends on what gilrs 0.11 implements on
each platform, and on the web on what the browser has. Features it can't
drive are reported as `false` in `GamepadInfo::capabilities`, never faked:

| | Linux (evdev) | Windows (Windows.Gaming.Input) | macOS | Web |
|---|---|---|---|---|
| Buttons, sticks, triggers, hot-plug | yes | yes | yes | yes |
| Name, family | yes | yes | yes | yes |
| USB vendor / product ids | yes | yes | yes | when the browser reports them |
| Body rumble (`strong`, `weak`) | yes, on pads with force feedback | yes, on gamepads with motors | no | where the browser has `vibrationActuator` (Chromium-based browsers) |
| Trigger rumble | no | no (the system has it; gilrs sends zero) | no | where the pad's actuator lists `trigger-rumble` (Chromium on Windows, Xbox pads) |
| Gyro and accelerometer | no | no | no | no |

On the web the engine plays rumble itself: gilrs has no force feedback
there, so it calls `playEffect("dual-rumble", ...)` on the pad's
`vibrationActuator`, or `playEffect("trigger-rumble", ...)` with the
trigger levels too when the actuator lists that effect in its `effects`
(or, in older browsers, says yes to `canPlayEffectType`). Browsers end an effect after at most 5 seconds, so the
engine asks for 2-second effects and plays the same levels again every
second until the rumble ends (`Rumble::seconds`, or `stop_rumble`); a game
that stalls stops buzzing within 2 seconds. A pad without an actuator
reports `rumble: false` and stays quiet.

Trigger levels follow the same rules everywhere: a pad that reports
`trigger_rumble` gets them; on any other pad they are dropped and the body
motors still play, so `Rumble::new(0.0, 0.2).triggers(0.0, 1.0)` is a soft
buzz there, and a rumble of the triggers alone plays nothing. The engine
can only report what the browser says it has. The same holds for builds
with [threads](web-threads.md). `web/smoke/threads.mjs --case gamepads`
checks both kinds of pad in a headless browser with fed pads whose
actuators record what they are asked.

For trigger rumble or motion on a platform, install a backend that reads a
library or SDK that has them; the engine's API already carries both.

## From C and C++

C: `ponko_ctx_gamepad_rumble`, `ponko_ctx_gamepad_stop_rumble`,
`ponko_ctx_gamepad_rumble_levels`, `ponko_ctx_gamepad_info`,
`ponko_ctx_gamepad_name`, `ponko_ctx_set_gamepad_family`,
`ponko_ctx_gamepad_motion`, `ponko_ctx_feed_gamepad_motion`,
`ponko_ctx_last_device`, `ponko_ctx_gamepads_changed`, the player calls
(`ponko_ctx_assign_player`, `ponko_ctx_unassign_player`,
`ponko_ctx_player_device`, `ponko_ctx_free_player`,
`ponko_ctx_join_pressed`), `ponko_ctx_action_glyphs`, `ponko_glyph_label`
and the atlas (`ponko_glyph_atlas_new`, `_free`, `_pixels`, `_region`,
`_texture`, `_draw_prompt`). `ponko_ctx_feed_input` kind 8 connects a pad.
C++ wraps them on `ponko::ctx` (`rumble`, `gamepad_info`, `last_device`,
`action_glyphs`, `draw_prompt`, ...) with `ponko::glyph_atlas`.
Custom backends (`GamepadBackend`) and `FakeGamepads` are Rust-only: a
native game uses the built-in backend, and its tests feed events.
