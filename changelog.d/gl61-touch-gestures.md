### Added
- Touch input: the window feeds every finger (`InputEvent::Touch`, with
  `InputEvent::touch_start`, `touch_move` and `touch_end` for tests), and
  `Input::touches`, `gesture_events`, `drag_delta`, `pinch_scale`,
  `rotation_delta` and `touch_device` read them. Tap, double tap, long
  press, drag, pinch, rotate and four swipes are recognized with thresholds
  in `GestureSettings` (`Input::set_gesture_settings`), measured in game
  time, so headless tests see what a phone sees.
- Gestures bind to actions like keys: `ButtonBinding::Gesture`,
  `AxisBinding::Gesture { axis: GestureAxis, scale }` (pinch, rotation and
  drag amounts) and `Axis2Binding::Drag { scale }`. They are saved in the
  action map file, and `Input::pressed_this_frame` reports a gesture that
  just happened for rebinding screens. The prelude gains `Gesture`.
- On-screen controls: `Input::set_touch_controls` with
  `ui::touch::TouchControls` puts sticks, round buttons and corner buttons
  on the screen, each driving a named action while a finger holds it. They
  show on touch devices only by default (`TouchShow`), take their fingers
  away from gestures and the UI, step aside while a UI screen is open, are
  buttons and groups to assistive technology, and allocate nothing once
  placed. `Input::touch_control_center`, `set_touch_control_shown`,
  `set_touch_controls_show` and `touch_controls_shown` go with them.
- Input scenarios have a `touch` step and the `InputScenario::tap`,
  `touch_hold` and `touch_drag` builders.
- In the browser the game's canvas takes every finger (`touch-action:
  none`), and the platform's touch points say whether it is a touch device.
- Coin Dash, Ember Hop and Shard Grove have on-screen controls and taps on
  their menus, and each has a test that plays it by touch.
- C and C++: `ponko_ctx_feed_touch`, `ponko_ctx_touches`,
  `ponko_ctx_gesture_events`, `ponko_ctx_gesture_deltas`,
  `ponko_ctx_touch_device` and `ponko_ctx_set_touch_device`, the gesture
  settings, `ponko_ctx_set_touch_controls` and its neighbours,
  `ponko_input_scenario_touch`, and the `PONKO_ACTION_GESTURE`,
  `PONKO_ACTION_GESTURE_AXIS` and `PONKO_ACTION_DRAG` binding sources; in
  C++ `ctx.feed_touch`, `ponko::touch_controls` and
  `action_binding::gesture`, `gesture_axis` and `drag`.
- The guide's "Touch: gestures and on-screen controls" chapter.
