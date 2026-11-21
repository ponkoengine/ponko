### Changed
- The retained UI keeps each node's drawing (its fill, border, image, widget
  parts and glyphs) until something it is drawn from changes: its rectangle,
  clip, paint, text, value, scroll, caret or selection, its state's fill or an
  animation of it, or the window size, scales, theme or fonts for every node.
  A still frame lays out no glyph; a changed label is drawn again alone. What
  is drawn is unchanged. In `ponko_bench`, `frame2d/ui-still` went from about
  0.106 ms to 0.033 ms and `frame2d/ui-counting` from about 0.109 ms to
  0.037 ms (medians of six interleaved runs each).

### Added
- `ponko_ctx_ui_layout_passes` (C) and `ui::layout_passes()` (C++): how many
  times the tree has been laid out, for tests that a still screen costs no
  layout (`Ui::layout_passes` in Rust).
- `UiPaint` implements `PartialEq`.
