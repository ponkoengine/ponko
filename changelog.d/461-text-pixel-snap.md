### Fixed
- Screen-space text lands on whole physical pixels at any position and window
  scale factor: `Graphics::draw_text_ex` (and so `draw_text`, the immediate UI,
  retained UI labels and shaped text) rounds each run's origin to the physical
  pixel grid before placing its glyphs. Text placed at fractional positions
  (centred labels, flexbox layouts, a scaled UI) no longer looks smeared or
  unevenly spaced, and a centred label looks the same wherever it sits.
- Rich text runs in different fonts and sizes keep one baseline per line after
  snapping, and inline images get whole-pixel edges.
- The retained UI measures text for layout, wrapping and caret placement at the
  size it is drawn, in physical pixels, so layout and drawing agree.
