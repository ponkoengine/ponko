### Added
- UI groups can fade as one image: `Ui::set_fade_as_group(node, true)` (or
  `UiBuilder::fade_as_group()`, `Ui::fades_as_group` to read it back). While
  the group is partly transparent (its own opacity or an ancestor's), it is
  drawn opaque into an offscreen layer the size of the window and shown at
  its opacity, so overlapping children no longer show through each other
  and a panel's fill no longer shows through its buttons. Groups inside a
  fading group get their own layer. A UI with no group fading draws exactly
  what it drew before: the `ui/draw-*` benchmark rows are unchanged within
  noise (about 1 µs on 22–31 µs frames). A fading group's nodes keep their
  drawings through the fade: the new `ui/draw-fade-group` row takes 0.022 ms
  a frame where fading the same screen piece by piece (`ui/draw-fade`)
  takes 0.030 ms. World-space panels (`UiPanel3D`) still fade each piece.
  From C: `ponko_ctx_ui_set_fade_as_group` and `ponko_ctx_ui_fades_as_group`;
  in C++ `ui::tree::set_fade_as_group` and `fades_as_group`.
