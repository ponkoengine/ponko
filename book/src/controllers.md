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
    // Key caps read as on the player's keyboard (Z for `Key::W` in France).
    atlas.set_key_labels(ctx.input.key_labels());
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
- `ctx.input.key_labels().text(&glyphs)` (`"Z/Q/S/D"` for WASD on a French
  keyboard, `"Cross"`) is a text-only prompt and what a screen reader says;
  `glyphs.to_string()` and `Glyph::label()` name keys by their US position.

An action with no binding on the device used last gives no glyphs: bind
both, or show the label alone.

### Key labels follow the keyboard layout

`Key` names a key by its place on a US keyboard, so `Key::W` is the key
above `S` on every layout and controls stay put. Its prompt shows what is
printed on it: `ctx.input.key_labels()` (a `KeyLabels`) labels the letters,
the digit row and the punctuation keys for the player's layout, and every
other key (Space, Shift, the arrows, the numpad) by its usual name. Pass it
to `atlas.set_key_labels` before drawing prompts (it repaints only the caps
that changed, and costs a comparison when none did), and use
`key_labels().text(&glyphs)` for text. `ctx.input.key_labels_changed()` says
when to redraw text prompts.

Where the labels come from:

| | Windows | macOS | Linux and the BSDs (X11, Wayland) | Web |
|---|---|---|---|---|
| The whole layout, from the first frame | yes (`MapVirtualKeyExW`) | yes (the input source's `UCKeyTranslate`) | yes (the active xkb layout) | where the browser has the Keyboard Layout Map |
| Read again | when the window gets the focus back | when the window gets the focus back | as soon as the player switches layouts or the keymap changes | when the page gets the focus back |
| Each character key the player presses | yes | yes | yes | letters only |

Windows and macOS give the whole layout without a window
(`KeyLabels::system()`), so prompts are right from the first frame.

**Linux and the BSDs** use xkb layouts on both display servers, and the
engine reads the keymap the way the session hands it out: on X11 from the X
server's XKB extension (its key map, with the active layout from its state,
and events when either changes), on Wayland from the compositor, which
sends the keymap and the active layout to a keyboard object the engine
keeps next to winit's. Either way it takes the first level of each
character key in the active layout (a dead key reads as its mark, `^`), so
a player with `us,ru` who switches to Russian sees `Ц` for `Key::W` on the
next frame. What a person has to check on a real session (the reading of
the keymap is unit-tested on every system, with US, French + German and
Russian keymaps, but no test has a display): that the labels change when
the layout is switched with the desktop's own shortcut, on GNOME and KDE
under Wayland and on an X11 session (`setxkbmap fr,de -option
grp:alt_shift_toggle`), and that an X11 game sees a keymap loaded while it
runs.

**In a browser** the labels come from `navigator.keyboard.getLayoutMap()`,
asked when the page opens and when it gets the focus back. Chromium-based
browsers have it on pages served over HTTPS or from `localhost`; Firefox,
Safari and pages outside a secure context don't, and a page in a frame may
be refused. There, as before, a prompt shows the US label until the
player presses that key once; the character a browser reports comes with
Shift applied, so only letters (whose case is all Shift changes) are
learned. The headless browser runner `web/smoke/key-labels.mjs` checks all
three: a layout map, none, and a refused one.

Headless runs keep US labels until
`ctx.input.set_key_labels(KeyLabels::from_layout(...))`, which is also how
a game offers a "keyboard layout" option and how tests check a layout:

```rust
# use ponko::prelude::*;
// A French keyboard, as far as the prompts need it.
let azerty = ponko::input::KeyLabels::from_layout(|key| match key {
    Key::W => Some('z'),
    Key::A => Some('q'),
    Key::Q => Some('a'),
    Key::Z => Some('w'),
    Key::Semicolon => Some('m'),
    _ => None, // the same as on a US keyboard
});
Game::new("t", 1, 1).run_headless(1, move |ctx| {
    ctx.input.set_key_labels(azerty.clone());
    assert_eq!(ctx.input.key_labels().label(Key::W), "Z");
});
```

From C and C++, `ponko_glyph_atlas_draw_prompt` and
`ponko_glyph_atlas_texture` label the caps for the player's layout
themselves. For text, `ponko_ctx_key_label` gives a key's label on the
player's layout and `ponko_ctx_glyph_label` a glyph's (`ponko_glyph_label`
stays the US one); `ponko_ctx_key_labels_changed` says when they changed,
and `ponko_ctx_set_key_labels` sets a layout from an array of
`PonkoKeyLabel { key, character }`. C++ has them on `ponko::ctx`:

```cpp
{{#include ../../examples/cpp/guide.cpp:key_labels}}
```

The reference image of the GPU test `prompt_row_render_test` shows one row
per device kind: keyboard and mouse, then the Xbox, PlayStation, Nintendo
and generic families.

## Motion: gyro and accelerometer

`ctx.input.gamepad_motion(pad)` is the latest gyro (radians per second) and
accelerometer (m/s², gravity included) reading, or `None` when the pad has
sent none. Axes are the pad's held flat: x right, y up, z toward the player.
`motion.aim_delta(dt)` turns it into a yaw/pitch change for gyro aiming.

The default backend (gilrs) reads no motion sensors; the SDL backend
(feature `gamepad-sdl`, below) reads them on DualShock 4, DualSense, Switch
Pro and Joy-Con pads. Motion also arrives from `InputEvent::GamepadMotion`
in tests, from recordings and replays, and from the scenario step
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
[gilrs](https://crates.io/crates/gilrs) (feature `gamepad`), and in a web
build from the browser's `navigator.getGamepads()` itself; headless runs
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
`gamepad_motion`) drive hot-plug and motion in replays; a
`gamepad_connected` step with an `info` object (what an
[input recording](actions.md#recording-live-input) writes) connects a pad
with its name, family and capabilities.

### The SDL backend: gyro and trigger rumble

gilrs, the default, reads no gyro or accelerometer, drives no trigger
motors and plays no rumble on macOS. The feature `gamepad-sdl` makes the
window read pads through [SDL 3](https://libsdl.org)'s gamepad API instead
(on the desktop; the web keeps the browser's pads). Nothing else changes for
the game: the same `ctx.input` calls, slots, dead zone (gilrs's) and
trigger buttons (pressed past three quarters of the travel, released under
two thirds, as gilrs does), with `GamepadInfo::capabilities` saying what SDL
reports each pad can do. If SDL can't start, the window says why on stderr
and uses gilrs when `gamepad` is on too.

```toml
[dependencies]
ponko = { version = "...", features = ["gamepad-sdl"] }
```

It links the SDL 3 library, found with pkg-config or vcpkg: install it to
build (`brew install sdl3`, a Linux distribution's SDL 3 development
package, `vcpkg install sdl3`) and ship it with the game (`SDL3.dll`,
`libSDL3.dylib`, `libSDL3.so.0` next to the executable or in the app
bundle). To build SDL from source instead (it needs CMake and a C
compiler), turn on the `sdl3-sys` crate's `build-from-source` feature in
the game's own `Cargo.toml`.

**Why SDL.** These were the candidates, read from their sources and
documentation (November 2026):

| | gilrs 0.11 (default) | SDL 3 (`sdl3-sys` 0.7, SDL 3.2 to 3.4) | Apple's GameController framework | hidapi plus our own drivers |
|---|---|---|---|---|
| Platforms | Windows, macOS, Linux, web | Windows, macOS, Linux (and more) | macOS only | all, per pad model |
| Gyro and accelerometer | none | DualShock 4, DualSense, Switch Pro, Joy-Cons (its HID drivers, all three systems; GameController on macOS) | DualShock 4, DualSense, Switch Pro | what we write |
| Trigger rumble | none | Xbox One and Series pads (Windows.Gaming.Input, GameController on macOS, its USB driver on Linux) | Xbox pads, through CoreHaptics | what we write; Xbox pads on macOS need GameController anyway |
| Body rumble on macOS | none | yes (GameController and CoreHaptics, or its HID drivers) | yes | what we write |
| License | MIT or Apache-2.0 | Zlib (bindings and library) | system framework | MIT or BSD |
| Cost | pure Rust | a C library to link and ship | one platform | a driver per pad family to write and keep |

SDL is the only one that covers all three features on all three desktop
systems, and the pads it supports are kept up by the project most PC games
already rely on. The cost is a native library to install and ship, which is
why it is an optional feature and gilrs stays the default (a game that
needs neither gyro nor trigger rumble gains nothing from it). `sdl3-sys` is
the low-level binding with no other dependencies; the engine uses about
twenty of its functions and the `sdl3` crate's safe wrapper would only add
another layer. SDL's own handlers for Ctrl-C are left off, so the game's
stay in charge.

Its tests run without a pad: the backend's logic is tested through
`GamepadBackend` with a scripted device layer (connections, buttons,
sticks, triggers as buttons, gyro and accelerometer joined into one reading
a frame, rumble and its refresh, slots for returning pads), and the calls
into SDL with SDL's own virtual gamepads (`cargo test -p ponko --features
gamepad-sdl --lib gamepad_sdl`). What only a real pad shows (the gyro's
values and the rumble felt) is checked by hand.

### What each platform supports

What the default backend can do depends on what gilrs 0.11 implements on
each desktop platform, and on the web on what the browser has. Features it can't
drive are reported as `false` in `GamepadInfo::capabilities`, never faked:

| | Linux (evdev) | Windows (Windows.Gaming.Input) | macOS | Web |
|---|---|---|---|---|
| Buttons, sticks, triggers, hot-plug | yes | yes | yes | yes |
| Name, family | yes | yes | yes | yes |
| USB vendor / product ids | yes | yes | yes | when the browser reports them |
| Body rumble (`strong`, `weak`) | yes, on pads with force feedback | yes, on gamepads with motors | no | where the browser has `vibrationActuator` (Chromium-based browsers) |
| Trigger rumble | no | no (the system has it; gilrs sends zero) | no | where the pad's actuator lists `trigger-rumble` (Chromium on Windows, Xbox pads) |
| Gyro and accelerometer | no | no | no | no |

With `gamepad-sdl` on the desktop, SDL 3 (what SDL implements, from its
source; per pad, `capabilities` says what SDL reports):

| | Linux | Windows | macOS |
|---|---|---|---|
| Buttons, sticks, triggers, hot-plug, name, ids, family | yes | yes | yes |
| Body rumble | yes (evdev force feedback, or SDL's HID drivers) | yes | yes (GameController and CoreHaptics, or SDL's HID drivers) |
| Trigger rumble | Xbox One and Series pads on USB (SDL's driver) | Xbox One and Series pads (Windows.Gaming.Input) | Xbox One and Series pads (GameController) |
| Gyro and accelerometer | DualShock 4, DualSense, Switch Pro, Joy-Cons (reading their HID reports needs access to `/dev/hidraw*`, a udev rule) | the same pads | the same pads |

A DualSense's adaptive triggers resist rather than rumble: SDL's driver for
it plays no trigger motor, so it reports `trigger_rumble: false`.

On the web the engine reads the browser's pads itself, in the standard
layout and with gilrs's dead zone (a tenth of the travel, rescaled), so
sticks and triggers read as on the desktop. It plays rumble itself too: it
calls `playEffect("dual-rumble", ...)` on the pad's
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

For trigger rumble or motion on the desktop, turn on `gamepad-sdl`; for a
platform SDK of your own, install a `GamepadBackend` that reads it.

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
`action_glyphs`, `draw_prompt`, ...) with `ponko::glyph_atlas`. Key
labels: `ponko_ctx_key_label`, `ponko_ctx_glyph_label`,
`ponko_ctx_key_labels_changed`, `ponko_ctx_set_key_labels` (C++
`key_label`, `layout_glyph_label`, `key_labels_changed`, `set_key_labels`).
The SDL backend is a feature of the engine library, so a native game built
with `gamepad-sdl` gets it too. `KeyLabels::text` (join the glyph labels
yourself) and `InputScenario::connect_gamepad` are Rust-only; C and C++ scenarios are
JSON, where a `gamepad_connected` step takes the same `info` object.
Custom backends (`GamepadBackend`) and `FakeGamepads` are Rust-only: a
native game uses the built-in backend, and its tests feed events.
