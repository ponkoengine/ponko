### Changed
- The retained UI redraws only the part of its tree that changed. A node
  added, removed, shown or hidden is drawn alone, with the nodes its size
  moves: the nodes after it in drawing order keep their sprites and move in
  depth instead of being drawn again. What is drawn is unchanged.
- A UI node's text keeps its glyphs while its text, font, size, wrapping,
  alignment and place stay the same: a node whose opacity, scale, offset,
  background or text color animates, or whose fill fades on hover, is drawn
  without shaping or laying out its text again (rich text included; a
  focused text field still measures its caret). In `ponko_bench` (medians
  of three interleaved runs on a busy machine), a frame of the settings
  screen fading went from 0.204 ms to 0.030 ms, one with 40 labels pulsing
  in color from 0.047 ms to 0.027 ms, and one where a HUD slot is added or
  removed from 0.207 ms to 0.027 ms; a still frame stays at 0.024 ms, and
  `frame2d/ui-still` and `frame2d/ui-counting` are unchanged.
- `ponko_bench` has `ui/draw-still`, `ui/draw-fade`, `ui/draw-pulse` and
  `ui/draw-insert`: the `frame2d` HUD and settings screen with text drawn as
  with a GPU, still, fading, with 40 labels pulsing in color, and with a HUD
  slot added and removed every frame.
