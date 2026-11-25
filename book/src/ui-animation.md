# UI animation

> **Maturity: [tested](maturity.md).** Game: none. Tests:
> `tests/ui_animation.rs`, `ui_animation_render_test.rs`. Platforms: Linux,
> macOS and Windows, headless (the workspace tests in the list of checks).
> Graphics: offscreen render tests, required on Windows (DirectX 12 on the
> WARP software adapter) and run on Linux and macOS only where a GPU is found.
> Missing: a reference game that uses it.

The retained UI ([Retained game UI](ui.md)) animates in three ways, all
optional and all off until you ask for them:

- **State transitions.** A control's fill eases between its states (idle,
  hovered, focused, pressed, disabled) instead of jumping, and a pressed
  control can shrink a little. One theme token turns it on for the whole
  tree.
- **Screen transitions.** A screen fades, slides or scales in when it is
  pushed and out when it is popped (by your code, Escape or a closing
  button).
- **Tweens and timelines.** Any node's opacity, scale, offset, background,
  text color, width or height goes to a value over time; several tweens
  placed in time make a timeline that can repeat.

The easing curves are CSS-style cubic beziers (`ponko::skeleton2d::Bezier`): `Bezier::EASE_OUT` for things arriving (the default),
`EASE_IN` for things leaving, `EASE_IN_OUT`, `EASE`, `LINEAR` and
`BACK_OUT`, which overshoots a little and settles. They are the same curves
the 2D skeleton clips use.

## Smooth states and a menu that slides in

<div class="lang-pair">
<div>

```rust
use ponko::ui::{UiLayout, UiScreenTransition, UiTheme, UiTransition};

// Controls ease between their states; a press pushes them in a little.
ctx.ui.set_theme(UiTheme {
    transition: UiTransition::new(0.12),
    press_scale: 0.96,
    ..UiTheme::dark()
});

// The pause menu fades and rises in, and fades out.
let menu = ctx.ui.root().layout(UiLayout::centered(320.0, 220.0)).id();
let resume = ctx.ui.button(menu, "Resume").layout(UiLayout::row(44.0)).focused().id();
ctx.ui.set_screen_transition(
    menu,
    UiScreenTransition::fade(0.2).with_offset(vec2(0.0, 24.0)),
    UiScreenTransition::fade(0.15),
);
ctx.ui.push_modal(menu);
# let _ = resume;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ui_animation}}
```

</div>
</div>

Every built-in theme has `transition: UiTransition::NONE` and
`press_scale: 1.0`, so a game that sets neither looks exactly as before.

A screen that is popped is gone from the stack at once: it is not
[shown](ui.md#screen-stack-and-modal-dialogs), takes no input, and the
screen under it has the focus and the pointer. It is still drawn while its
exit transition runs, where it was on the stack, so a menu swapped for
another fades out under the new one. Pushing it again while it goes turns it
round from where it is. While a screen comes on, it takes input in its
place at once.

## Tweens and timelines

`ui.animate(node, tween)` moves one property from where it is now to a
value. A second tween on the same property takes over from where the first
one got to, so changing your mind half way never jumps. `UiTween::from`
gives a start value instead.

A `UiTimeline` places tweens in time: `then` starts one when everything
before it has ended, `with` starts it with the one added last, and `at`
starts it at a time of its own. `repeat` plays it several times, in a loop,
or back and forth. Build it once and play it with `ui.play(node, &timeline)`
as often as you like.

```rust
use ponko::skeleton2d::Bezier;
use ponko::ui::{UiEvent, UiLayout, UiRepeat, UiTimeline, UiTween};

let root = ctx.ui.root().layout(UiLayout::fill()).id();
let badge = ctx.ui.label(root, "New!").id();
let reward = ctx.ui.panel(root).layout(UiLayout::fixed(200.0, 0.0)).id();
ctx.ui.push_modeless(root);

// The badge pops in after a short wait: it stays invisible until then,
// because its first tween has a start value.
let pop_in = UiTimeline::new()
    .at(0.3, UiTween::opacity(1.0).from(0.0).seconds(0.15))
    .with(UiTween::scale(1.2).from(0.5).seconds(0.15).with_ease(Bezier::BACK_OUT))
    .then(UiTween::scale(1.0).seconds(0.1));
ctx.ui.play(badge, &pop_in);

// A gentle pulse until stopped.
let pulse = UiTimeline::new()
    .then(UiTween::scale(1.05).seconds(0.6).with_ease(Bezier::EASE_IN_OUT))
    .repeat(UiRepeat::PingPong);
ctx.ui.play(badge, &pulse); // takes over the scale; the fade goes on

// A layout property: the panel opens, and the nodes after it make room.
ctx.ui.animate(reward, UiTween::height(120.0).seconds(0.25));

// Later, in a system: the end of a tween or timeline is an event.
for event in ctx.ui.events() {
    if event == UiEvent::AnimationDone(reward) {
        // fill the panel
    }
}
```

Opacity, scale and offset are drawn, not laid out: the node and its children
fade, scale (around the node's centre) and move together, and input and
layout keep using its place (`ui.rect`). Background and text color change the
node's paint; width and height change its layout, so its neighbours move.
Opacity fades each piece of a node and its children on its own, unless the
node fades as a group (below): half way, a child lets a little of its parent
show through it, and overlapping children show through each other.
`ui.drawn(node)` says how a node is drawn now (its rectangle after
animation, its opacity and scale, its fill), which is what a headless test
reads. `ui.stop_animations(node)` stops a node's tweens where they are, and
`ui.is_animating(node)` tells whether anything moves it.

## Fading a group as one image

`ui.set_fade_as_group(node, true)` (or `.fade_as_group()` when building)
fades the node and everything inside it as one picture: while its opacity
is between 0 and 1, the group is drawn opaque into an offscreen layer the
size of what it draws, and that layer is shown at the group's opacity. A card's
fill no longer shows through its buttons, and a child no longer shows
through the sibling on top of it; what is behind the group shows through it
evenly. The fade that drives it can be the node's own (a tween, a screen
transition) or an ancestor's.

```rust
use ponko::ui::{UiLayout, UiScreenTransition};

let card = ctx.ui.root().layout(UiLayout::centered(300.0, 200.0)).fade_as_group().id();
ctx.ui.button(card, "Play").layout(UiLayout::row(44.0));
ctx.ui.set_screen_transition(card, UiScreenTransition::fade(0.2), UiScreenTransition::fade(0.2));
ctx.ui.push_modal(card);
```

It costs nothing until it is used: a group at rest, or a UI with no group
marked, draws exactly what it drew before, with no layer. While a group
fades, its nodes keep their drawings from frame to frame (only the layer's
opacity changes), and the layer costs one texture, a pass drawing the group
into it and one quad showing it. A group inside a fading group gets a layer
of its own. Groups in world-space panels (`UiPanel3D`) fade the same way,
into layers in the panel's pixels.

The layer covers what the group draws, not just its rectangle: children
that reach out of it are in it. It is cut to the window (or the panel) and
snapped outwards to whole pixels, and draws the same pixels a window-sized
layer would. Its texture is kept from frame to frame and made in steps of
64 pixels: it is made again only when the group no longer fits, or fits
more than twice over, so a group that moves or changes size a little while
it fades makes no texture after its first frame. A 400 x 300 group on a 4K
screen takes a 448 x 320 texture (0.6 MB) where a window-sized one took
33 MB.

What it costs on the GPU, for a menu of 480 x 400 points (twelve rows of a
label on a fill) faded at half, from the engine's pass timer (`ui layers`
is the pass drawing the group into its layer, `sprites` the window's
sprite pass, which shows it), offscreen on an Apple M4 Max, medians of
three runs on an idle machine (2026-11-22, revision `9235ea05`; the runs
agreed within 0.004 ms):

| Window | Faded | `ui layers` | `sprites` | Layer texture |
|---|---|---|---|---|
| 1920 x 1080 | piece by piece | none | 0.021 ms | none |
| 1920 x 1080 | as a group, window-sized layer (before) | 0.021 ms | 0.020 ms | 8.3 MB |
| 1920 x 1080 | as a group | 0.013 ms | 0.014 ms | 0.9 MB |
| 3840 x 2160 | piece by piece | none | 0.052 ms | none |
| 3840 x 2160 | as a group, window-sized layer (before) | 0.052 ms | 0.068 ms | 33.2 MB |
| 3840 x 2160 | as a group | 0.013 ms | 0.044 ms | 0.9 MB |

The layer's pass costs 0.013 ms at either size, and the `sprites` pass
draws the faded group as one quad, so fading as a group costs about what
fading piece by piece does: 0.028 against 0.021 ms at 1080p, 0.058
against 0.052 ms at 4K. (Measured before on a busy machine, every figure
here was two to five times as large.) `group_fade_gpu_timings` in
`src/render/ui_group_fade_render_test.rs` measures it.

On the CPU, the `ui/draw-fade-group` benchmark row (the settings screen of
about 150 nodes fading as one image) takes 0.024 ms a frame where fading it
piece by piece (`ui/draw-fade`) takes 0.028 ms (an idle M4 Max, 2026-11-22,
medians of three runs within 9%; finding what the layer covers walks its
sprites once a frame).

Limits: sprites with an additive or multiply blend inside a group blend
with the group, not with what is behind it.

## The UI clock

Every animation runs on the UI clock: the [`UiInput::dt`](ui.md) of each
frame, which the engine fills with the frame time. It keeps running while
the simulation is paused (`ctx.time.pause()`) or slowed down, so a pause
menu still animates, and it stands still when `dt` is zero. Values depend on
the time since an animation started, not on the number of frames: the same
at 30, 60 or 144 frames a second. `ui.clock()` reads it, and
`UiTransition::progress(elapsed)` gives the eased fraction of a transition,
for text or sprites you draw yourself next to the UI. The two 2D reference
games draw their title and pause text that way.

Animating adds no allocation to a frame: playing a timeline copies its
tweens into a list the UI reuses, the rest is plain data on each node, and a
screen going off keeps the layout it had. `tests/ui_animation_alloc.rs`
plays five seconds of a menu with every kind of animation going and checks
it allocates exactly as often as the same frames without animation.
(Animating the width or height of a grid container copies its track lists
each frame; flex layouts copy nothing.)

## Reduced motion

`ui.set_reduced_motion(true)` makes state and screen transitions happen at
once, for players who ask for less movement; a settings screen can offer
it. Tweens and timelines the game plays still run: check
`ui.reduced_motion()` before playing decoration.

## In tests

`tests/ui_animation.rs` drives all of it headless and checks the values over
time: a state transition ends exactly on the state's fill, an interrupted
one turns round without a jump, a screen's pose at each moment, timelines
that hold, loop, ping-pong and report their end, and the same values at
different frame rates. `src/render/ui_animation_render_test.rs` draws a
faded, moved and scaled screen offscreen and checks where its pixels land.
`src/render/ui_group_fade_render_test.rs` draws two overlapping squares in a
group at half opacity through the renderer: faded piece by piece the lower
square shows through the upper one, faded as a group it does not, on the
screen and in a world-space panel's render target. It also checks that a
layer sized to its group draws the same pixels as a window-sized one, and
that a moving fading group makes no texture once its first frame made one.

## From C

`PonkoUiThemeEx` (version 2) has the `transition` and `press_scale` tokens.
`ponko_ctx_ui_set_screen_transition` takes two `PonkoUiScreenTransition`s
(`ponko_ui_screen_fade`, `_slide`, `_zoom` make them; null is none).
`ponko_ctx_ui_animate` plays one `PonkoUiTween` (`ponko_ui_tween_defaults`
fills one; its `start` is a delay) and `ponko_ctx_ui_play` an array of them
as a timeline with a `PonkoUiRepeat`. `ponko_ctx_ui_stop_animations`,
`ponko_ctx_ui_is_animating`, `ponko_ctx_ui_drawn`, `ponko_ctx_ui_clock`,
`ponko_ctx_ui_set_reduced_motion` and `ponko_ui_transition_progress` do the
rest, `ponko_ctx_ui_set_fade_as_group` and `ponko_ctx_ui_fades_as_group`
mark a group to fade as one image, `ponko_ui_ease` names the curves, and `PONKO_UI_EVENT_ANIMATION_DONE`
arrives through `ponko_ctx_ui_next_event_ex`. In C++ they are
`ui::tree::animate`, `play`, `set_screen_transition`, `drawn`,
`set_fade_as_group` and the rest,
with `ui::make_transition`, `ui::screen_fade`, `ui::tween_to` and
`ui::named_ease`.
