### Changed
- A UI node that only moves (scrolled, or pushed along by a sibling that
  resized) keeps its laid-out text: its glyphs are placed again and snapped
  to the physical pixels of where it is now, without shaping, measuring or
  wrapping its text again (rich text included). A virtual list scrolled a
  row lays out only the item that came into view. The tooltip measures and
  lays out its text once while it shows, not each frame it follows the
  pointer, and an item dragged with the pointer lays out its text once.
  What is drawn is unchanged. NUMBERS
- `ponko_bench` has `ui/draw-scroll` (the settings screen's list of 40
  actions scrolling a fraction of a pixel a frame) and `ui/draw-tooltip`
  (its tooltip following the pointer).
