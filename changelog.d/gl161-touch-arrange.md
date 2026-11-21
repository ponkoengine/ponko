### Added
- `ctx.input.arrange_touch_controls()` opens a ready-made screen, built with the
  retained UI, where the player arranges the on-screen touch controls with
  a finger or the mouse: drag to move, pick one and press `-` or `+` to
  resize it, `Size` and `Opacity` sliders for all of them, `Reset` to the
  game's placement, and `Save` to the player's storage. `Cancel`, Escape or
  the game closing it put back the arrangement it had when it opened. The
  panel keeps clear of the controls and fits phones and tablets either way
  up; its nodes are named `touch.arrange.<part>` for the game's own words.
