### Added
- On-screen controls a game restyles: `ui::touch::TouchStyle` (colors of
  the stick, knob, buttons idle and held and labels, images of the game's
  own in place of the disc, size, label size) with `TouchControls::style`
  and `control_style`, and `TouchControls::place` with `TouchPlace` (a
  point of the view plus an offset, kept from its corner when the window
  changes).
- Players arrange the on-screen controls: `Input::edit_touch_controls`
  lets a finger or the mouse drag them (pressing nothing), and their
  places, sizes and opacity are a `TouchLayout` (`Input::touch_layout`,
  `set_touch_layout`), saved in the player's storage with
  `Input::save_touch_layout` and read back with `restore_touch_layout`.
- `InputEvent::Trackpad` and the `trackpad` scenario step: a Mac
  trackpad's pinch, rotation and two-finger double tap feed the touch
  gestures (`Gesture::Pinch`, `Rotate`, `DoubleTap` and their amounts).
- Windows and Linux desktops with a touch screen count as touch devices
  before the first finger, so on-screen controls set to show on touch
  devices show from the start.

### Fixed
- Input a system feeds (a replay, a test, a bot) reaches the retained UI
  in the frame it was fed: the UI takes what is new before the next
  system, so a tap or click fed by a system clicks, focuses and closes in
  that frame instead of being lost.
- A desktop's mouse events made up for a finger no longer click a UI
  button a second time or take the UI's pointer from the finger (Windows'
  made-up presses, X11's cursor following the first finger).
- Input recordings keep touches (and trackpad gestures), so a recorded
  touch session replays.
