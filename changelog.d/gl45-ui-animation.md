### Added
- Retained UI animation, all of it optional and free when unused:
  - State transitions: the theme token `UiTheme::transition`
    (`UiTransition::new(0.12)`) eases a control's fill between its states
    (idle, hovered, focused, pressed, disabled), and `UiTheme::press_scale`
    shrinks a pressed control a little. An interrupted transition turns
    round from what is shown.
  - Screen transitions: `Ui::set_screen_transition(root, enter, exit)` with
    `UiScreenTransition::fade`, `slide`, `zoom` and their combinations. A
    popped screen leaves the stack at once and is drawn, without input,
    while it goes, where it was on the stack.
  - Tweens and timelines: `Ui::animate(node, UiTween)` and
    `Ui::play(node, &UiTimeline)` move a node's opacity, scale, offset,
    background, text color, width or height, in sequence or together, once,
    several times, in a loop or back and forth (`UiRepeat`). A new tween
    takes over its property from where it is. `UiEvent::AnimationDone`
    reports the end; `Ui::stop_animations`, `Ui::is_animating`.
  - `Ui::drawn(node)` (`UiDrawn`) says how a node is drawn now, for tests;
    `Ui::clock()` and `UiTransition::progress` let text drawn by hand
    animate on the same clock; `Ui::set_reduced_motion` makes state and
    screen transitions instant.
  - Every animation runs on the UI clock (`UiInput::dt`): it goes on while
    the simulation is paused, is independent of the frame rate, and adds
    no allocation to a frame.
- `Bezier::EASE`, `EASE_IN`, `EASE_OUT`, `EASE_IN_OUT` and `BACK_OUT`, the
  usual CSS curves (`ponko::skeleton2d::Bezier`, the one curve type of skeletons and UI).
- `DrawList::transform_from`: scale, move and fade the sprites pushed since a
  mark.
- The guide's "UI animation" page. The reference games' menus animate:
  Shard Grove's and Coin Dash's retained screens fade in and out with eased
  controls, and the title and pause text of Coin Dash and Ember Hop fades
  and rises in.
- C: `PonkoUiTransition`, `PonkoUiScreenTransition`, `PonkoUiTween`,
  `PonkoUiDrawn`, `ponko_ui_ease`, `ponko_ui_transition`,
  `ponko_ui_transition_progress`, `ponko_ui_screen_fade` / `_slide` / `_zoom`,
  `ponko_ui_tween_defaults`, `ponko_ctx_ui_animate`, `ponko_ctx_ui_play`,
  `ponko_ctx_ui_stop_animations`, `ponko_ctx_ui_is_animating`,
  `ponko_ctx_ui_set_screen_transition`, `ponko_ctx_ui_screen_transition`,
  `ponko_ctx_ui_drawn`, `ponko_ctx_ui_clock`,
  `ponko_ctx_ui_set_reduced_motion`, `ponko_ctx_ui_reduced_motion` and the
  event kind `PONKO_UI_EVENT_ANIMATION_DONE`. C++: `ui::tree::animate`,
  `play`, `stop_animations`, `is_animating`, `set_screen_transition`,
  `screen_transition`, `drawn`, `clock`, `set_reduced_motion`, and
  `ui::make_transition`, `ui::screen_fade` / `slide` / `zoom`, `ui::tween_to`,
  `ui::named_ease`.

### Changed
- **Breaking:** `UiTheme` has the new fields `transition` and `press_scale`.
  Build themes with `..UiTheme::default()`, `dark()` or `light()`. Every
  built-in theme has no transition and a press scale of 1, so it draws
  exactly as before.
- C: `PonkoUiThemeEx` is version 2, with `transition` and `press_scale` at
  its end. Callers that pass version 1 keep working and get no transition.
