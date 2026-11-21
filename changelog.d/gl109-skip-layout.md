### Added
- `Ui::layout_passes` counts how many times the retained UI was laid out,
  so a test can check that a still screen is not laid out every frame.
- `ponko_bench` has `frame2d/ui-still`, `frame2d/ui-counting` and
  `frame2d/no-ui`: the CPU side of a whole 2D frame without a window
  (2,000 sprites under a HUD and a settings screen of about 150 nodes).

### Changed
- The retained UI is laid out only when something changed since the last
  layout: the window size, the scale or the fonts, or a node's layout,
  text, font or size, or a node added, removed, shown or hidden. Before,
  every node's text was measured again two or three times a frame. When a
  label's text changes, only that label is measured again. Scrolling and
  animations of drawn properties still move nodes every frame. In the
  `frame2d` benchmark a frame with a still UI went from 1.07 ms to 0.11 ms
  of CPU, and one where a label's text changes every frame from 1.08 ms to
  0.11 ms.
- `Ui::set_layout` with the layout a node already has changes nothing.
